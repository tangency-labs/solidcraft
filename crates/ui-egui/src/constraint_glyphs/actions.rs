//! Constraint glyph hit-testing, click/right-click/middle-click actions, detach nudges, and undo/redo flash tracking.

use std::collections::HashSet;

use egui::Pos2;
use serde_json::json;
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{ConstraintKind, Sketch};

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

use super::grouping::{
    AnchorKey, CurveRelKind, anchor_groups_with_rel, constraint_entities, find_point_cluster, glyph_constraints, glyph_deletion, glyph_group,
    poly_point_at_frac,
};
use super::{ConstraintFlashGhost, DrawnRelInfo, HISTORY_FLASH_MS, HistoryFlashState, OVERLAY, pattern_icon_for};

pub(crate) fn active_sketch(app: &SolveApp) -> Option<u64> {
    app.session.active_sketch
}

/// [`glyph_deletion`] in the active sketch.
pub fn active_glyph_deletion(app: &SolveApp, id: &str) -> Vec<Sel> {
    let st = crate::open_meshing::session_world(&app.session);
    match active_sketch(app).and_then(|s| st.sketch(s)) {
        Some(ss) => glyph_deletion(&ss.sketch, id),
        None => vec![Sel::SketchConstraint { id: id.to_string() }],
    }
}

/// The constraint glyph under a screen position (its constraint id).
pub fn glyph_at(pos: Pos2) -> Option<String> {
    OVERLAY.with(|cell| {
        let st = cell.borrow();
        let reach = st.glyph_px * 0.6;
        let half = st.glyph_px * 0.5 + 1.0;
        st.glyphs
            .iter()
            .filter(|(_, c)| ((c.x - pos.x).abs() <= half && (c.y - pos.y).abs() <= half) || c.distance(pos) <= reach)
            .min_by(|a, b| a.1.distance(pos).total_cmp(&b.1.distance(pos)))
            .map(|(id, _)| id.clone())
    })
}

/// Constraint glyphs drawn last frame: (constraint id, screen centre).
pub fn glyph_positions() -> Vec<(String, Pos2)> {
    OVERLAY.with(|cell| cell.borrow().glyphs.clone())
}

/// Record which Equal/Parallel/Symmetry group a left-click targeted so Delete removes only that entry.
pub fn note_clicked_constraint(id: &str, pos: Pos2) {
    OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        let reach = st.glyph_px * 0.8;
        st.picked_rel = st
            .drawn_rel
            .iter()
            .filter(|r| r.id == id && r.center.distance(pos) <= reach)
            .min_by(|a, b| a.center.distance(pos).total_cmp(&b.center.distance(pos)))
            .map(|r| (id.to_owned(), r.kind, r.curve_id.clone(), r.all_constraint_ids.clone()));
    });
}

/// Clear any curve-specific Equal/Parallel/Symmetry click target when selection moves elsewhere.
pub fn clear_clicked_constraint() {
    OVERLAY.with(|cell| cell.borrow_mut().picked_rel = None);
}

fn rel_at_pos(id: &str, pos: Pos2) -> Option<DrawnRelInfo> {
    OVERLAY.with(|cell| {
        let st = cell.borrow();
        let reach = st.glyph_px * 0.8;
        st.drawn_rel
            .iter()
            .filter(|r| r.id == id && r.center.distance(pos) <= reach)
            .min_by(|a, b| a.center.distance(pos).total_cmp(&b.center.distance(pos)))
            .cloned()
    })
}

fn point_is_protected_for_nudge(sk: &Sketch, pi: usize) -> bool {
    pi == 0 || sk.points.get(pi).is_some_and(|pt| pt.fixed || pt.link.is_some() || pt.exprs.is_some())
}

pub(crate) fn sketch_px_per_mm(plane: &Plane, proj: &Proj) -> Option<f32> {
    let a = proj.to_screen(plane.to_world(Vec2::ZERO))?;
    let b = proj.to_screen(plane.to_world(Vec2::new(1.0, 0.0)))?;
    Some(a.distance(b))
}

pub(crate) fn compute_detach_nudge_with_px(sk: &Sketch, ci: usize, pi: usize, px_per_mm: f32) -> Vec2 {
    let scale_px = px_per_mm.max(1e-3) as f64;
    let target_mm = 10.0 / scale_px;
    if let Some(pt) = sk.point(pi) {
        let mut poly = sk.polyline(ci);
        if poly.len() >= 2 {
            if let (Some(&first), Some(&last)) = (poly.first(), poly.last())
                && last.dist(pt) < first.dist(pt)
            {
                poly.reverse();
            }
            let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
            let dist = target_mm.min(total * 0.5);
            let start = poly.first().copied().unwrap_or(pt);
            let mut acc = 0.0;
            for w in poly.windows(2) {
                let seg = w[0].dist(w[1]);
                if acc + seg >= dist && seg > 1e-12 {
                    return w[0].lerp(w[1], ((dist - acc) / seg).clamp(0.0, 1.0)) - start;
                }
                acc += seg;
            }
            if let Some(&end) = poly.last() {
                return end - start;
            }
        }
    }
    Vec2::new(target_mm, 0.0)
}

pub(crate) fn compute_detach_nudge(sk: &Sketch, ci: usize, pi: usize, plane: &Plane, proj: &Proj) -> Vec2 {
    compute_detach_nudge_with_px(sk, ci, pi, sketch_px_per_mm(plane, proj).unwrap_or(1.0))
}

pub(crate) fn selected_detach_curve(app: &SolveApp, sid: u64, sk: &Sketch) -> Option<(String, usize)> {
    if app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        return None;
    }
    app.session.selection.iter().rev().find_map(|sel| match sel {
        Sel::SketchCurve { id, sketch } if sketch.is_none_or(|s| s == sid) => {
            sk.curve_index(id).filter(|&ci| !sk.is_linked_curve(ci)).map(|ci| (id.clone(), ci))
        }
        Sel::SketchMidpoint { id } => sk.curve_index(id).filter(|&ci| !sk.is_linked_curve(ci)).map(|ci| (id.clone(), ci)),
        _ => None,
    })
}

pub(crate) fn selected_symmetric_point(app: &SolveApp, sk: &Sketch) -> Option<(usize, String, Vec<(String, usize, usize)>)> {
    if app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        return None;
    }
    let point_id = app.session.selection.iter().rev().find_map(|sel| match sel {
        Sel::SketchPoint { id } => Some(id.as_str()),
        _ => None,
    })?;
    let pi = sk.resolve_point(point_id)?;
    let pt_id = sk.points.get(pi)?.id.clone();
    let syms: Vec<(String, usize, usize)> = glyph_constraints(sk)
        .filter_map(|c| match c.kind {
            ConstraintKind::Symmetric { p, q, l } if p == pi => Some((c.id.clone(), l, q)),
            ConstraintKind::Symmetric { p, q, l } if q == pi => Some((c.id.clone(), l, p)),
            _ => None,
        })
        .collect();
    if syms.is_empty() { None } else { Some((pi, pt_id, syms)) }
}

fn point_is_intersection_for_curve(sk: &Sketch, ci: usize, pi: usize) -> bool {
    if !sk.curves.get(ci).is_some_and(|c| c.kind.uses(pi)) {
        return false;
    }
    let users = sk.curves.iter().filter(|c| c.kind.uses(pi)).count();
    let has_coincident = sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Coincident { p, q } if p == pi || q == pi));
    users > 1 || has_coincident || pi == 0
}

pub(crate) fn resolve_detach_target(
    app: &SolveApp,
    sid: u64,
    sk: &Sketch,
    parent: &mut [usize],
    hover: Option<Pos2>,
    scale: f32,
) -> Option<(String, usize, usize, String)> {
    let (curve_id, ci) = selected_detach_curve(app, sid, sk)?;
    let curve = sk.curves.get(ci)?;
    if let Some(Hit::SketchPoint { sketch, id, .. }) = app.viewport.hover.as_ref()
        && *sketch == sid
        && let Some(hovered_pi) = sk.resolve_point(id)
    {
        let target_cluster = find_point_cluster(parent, hovered_pi);
        if let Some(target_pi) = curve.kind.point_ids().into_iter().find(|&cp| find_point_cluster(parent, cp) == target_cluster)
            && point_is_intersection_for_curve(sk, ci, target_pi)
            && let Some(pt) = sk.points.get(target_pi)
        {
            return Some((curve_id, ci, target_pi, pt.id.clone()));
        }
    }
    let h = hover?;
    OVERLAY.with(|cell| {
        let st = cell.borrow();
        let prev = st.detach_preview.as_ref()?;
        if prev.curve_id != curve_id {
            return None;
        }
        let keep = prev.glyph_rect.expand(10.0 * scale).contains(h) || prev.point_rect.expand(8.0 * scale).contains(h);
        if !keep {
            return None;
        }
        let target_pi = sk.resolve_point(&prev.point_id)?;
        if point_is_intersection_for_curve(sk, ci, target_pi) { Some((curve_id, ci, target_pi, prev.point_id.clone())) } else { None }
    })
}

fn coincident_nudge_delete(app: &mut SolveApp, id: &str) -> bool {
    let Some(sid) = active_sketch(app) else { return false };
    let px_per_mm = OVERLAY.with(|c| c.borrow().last_px_per_mm);
    let target = {
        let st = crate::open_meshing::session_world(&app.session);
        let Some(ss) = st.sketch(sid) else { return false };
        let sk = &ss.sketch;
        let Some(ConstraintKind::Coincident { p, q }) = sk.constraints.iter().find(|c| c.id == id).map(|c| &c.kind) else {
            return false;
        };
        let (p, q) = (*p, *q);
        let sel_ci = selected_detach_curve(app, sid, sk).map(|(_, ci)| ci);
        let curve_using = |pi: usize| {
            sel_ci.filter(|&ci| sk.curves.get(ci).is_some_and(|cu| cu.kind.uses(pi))).or_else(|| sk.curves.iter().position(|cu| cu.kind.uses(pi)))
        };
        let chosen = if let Some(ci) = sel_ci
            && sk.curves.get(ci).is_some_and(|cu| cu.kind.uses(q))
            && !point_is_protected_for_nudge(sk, q)
        {
            Some((ci, q))
        } else if let Some(ci) = sel_ci
            && sk.curves.get(ci).is_some_and(|cu| cu.kind.uses(p))
            && !point_is_protected_for_nudge(sk, p)
        {
            Some((ci, p))
        } else if !point_is_protected_for_nudge(sk, q)
            && let Some(ci) = curve_using(q)
        {
            Some((ci, q))
        } else if !point_is_protected_for_nudge(sk, p)
            && let Some(ci) = curve_using(p)
        {
            Some((ci, p))
        } else if let Some(ci) = curve_using(q) {
            Some((ci, q))
        } else {
            curve_using(p).map(|ci| (ci, p))
        };
        chosen.and_then(|(ci, pi)| {
            Some((sk.curves.get(ci)?.id.clone(), sk.points.get(pi)?.id.clone(), compute_detach_nudge_with_px(sk, ci, pi, px_per_mm)))
        })
    };
    let Some((curve, point, offset)) = target else { return false };
    app.run("sketch.detach_point", json!({ "curve": curve, "point": point, "offset": [offset.x, offset.y], "constraint": id })).is_ok()
}

fn midpoint_downgrade_delete(app: &mut SolveApp, id: &str) -> bool {
    let Some(sid) = active_sketch(app) else { return false };
    let (mid_ids, replacements): (Vec<String>, Vec<(String, String)>) = {
        let st = crate::open_meshing::session_world(&app.session);
        let Some(ss) = st.sketch(sid) else { return false };
        let sk = &ss.sketch;
        if !sk.constraints.iter().any(|c| c.id == id && matches!(c.kind, ConstraintKind::Midpoint { .. })) {
            return false;
        }
        let (mut ids, mut repls) = (Vec::new(), Vec::new());
        for cid in glyph_group(sk, id) {
            if let Some(c) = sk.constraints.iter().find(|c| c.id == cid)
                && let ConstraintKind::Midpoint { p, l } = c.kind
                && let (Some(pt), Some(cv)) = (sk.points.get(p), sk.curves.get(l))
            {
                ids.push(c.id.clone());
                repls.push((pt.id.clone(), cv.id.clone()));
            }
        }
        (ids, repls)
    };
    if mid_ids.is_empty() {
        return false;
    }
    let undo_before = app.session.undo.len();
    if app.run("sketch.delete", json!({ "entities": mid_ids })).is_err() {
        return false;
    }
    for (pt_id, line_id) in replacements {
        let _ = app.run("sketch.constraint.coincident", json!({ "a": pt_id, "b": line_id }));
    }
    app.session.undo.truncate(undo_before + 1);
    true
}

fn delete_glyph_by_id(app: &mut SolveApp, id: &str) -> bool {
    if midpoint_downgrade_delete(app, id) || coincident_nudge_delete(app, id) {
        return true;
    }
    let entities: Vec<String> = active_glyph_deletion(app, id)
        .into_iter()
        .filter_map(|s| match s {
            Sel::SketchConstraint { id } | Sel::SketchPoint { id } => Some(id),
            _ => None,
        })
        .collect();
    if entities.is_empty() {
        return false;
    }
    let _ = app.run("sketch.delete", json!({ "entities": entities }));
    true
}

fn delete_rel_group(app: &mut SolveApp, kind: CurveRelKind, curve_id: &str, all_ids: &[String]) {
    match kind {
        CurveRelKind::Equal => {
            let _ = app.run("sketch.unlink_curve_group", json!({ "curve": curve_id, "kind": "equal" }));
        }
        CurveRelKind::Parallel => {
            let _ = app.run("sketch.unlink_curve_group", json!({ "curve": curve_id, "kind": "parallel" }));
        }
        CurveRelKind::Symmetry => {
            if !all_ids.is_empty() {
                let _ = app.run("sketch.delete", json!({ "entities": all_ids }));
            }
        }
    }
}

/// Delete the selected constraint glyph (true when one was selected).
pub fn delete_glyph(app: &mut SolveApp) -> bool {
    let (picked, picked_rel) = OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        (st.picked.take(), st.picked_rel.take())
    });
    let Some(id) = picked else { return false };
    if let Some((rid, kind, curve_id, all_ids)) = picked_rel
        && rid == id
    {
        delete_rel_group(app, kind, &curve_id, &all_ids);
        return true;
    }
    delete_glyph_by_id(app, &id)
}

/// Delete the individual constraint glyph under `pos`, if any (never deletes a collapsed group badge).
pub fn delete_glyph_at(app: &mut SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    let Some(id) = glyph_at(pos) else { return false };
    OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        if st.picked.as_deref() == Some(id.as_str()) {
            st.picked = None;
        }
        if st.picked_rel.as_ref().is_some_and(|(rid, _, _, _)| rid == &id) {
            st.picked_rel = None;
        }
    });
    if let Some(rel) = rel_at_pos(&id, pos) {
        delete_rel_group(app, rel.kind, &rel.curve_id, &rel.all_constraint_ids);
        return true;
    }
    delete_glyph_by_id(app, &id)
}

/// If `pos` is over the simulated coincident break-off glyph, detach that curve from the point.
pub fn detach_simulated_at(app: &mut SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    let hit = OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        let preview = st.detach_preview.as_ref()?;
        if preview.glyph_rect.expand(3.0).contains(pos) {
            let out = (preview.curve_id.clone(), preview.point_id.clone(), preview.nudge_2d);
            st.detach_preview = None;
            Some(out)
        } else {
            None
        }
    });
    let Some((curve, point, offset)) = hit else { return false };
    let detach_at = active_sketch(app)
        .and_then(|sid| app.session.doc.sketch(sid).ok().and_then(|sk| sk.resolve_point(&point).and_then(|pi| sk.point(pi)).map(|at| (sid, at))));
    if app.run("sketch.detach_point", json!({ "curve": curve, "point": point, "offset": [offset.x, offset.y] })).is_ok()
        && let Some((sid, at)) = detach_at
    {
        let expires_ms = crate::now_ms() + HISTORY_FLASH_MS;
        OVERLAY.with(|cell| {
            let fl = &mut cell.borrow_mut().history_flash;
            if fl.sketch != Some(sid) {
                *fl = HistoryFlashState::new();
                fl.sketch = Some(sid);
            }
            if !fl.ghosts.iter().any(|g| !g.is_added && g.kind_name == "Coincident" && g.at.dist(at) < 1e-4) {
                fl.ghosts.push(ConstraintFlashGhost {
                    id: format!("detach:{curve}:{point}"),
                    kind_name: "Coincident",
                    pattern_icon: None,
                    at,
                    dir: None,
                    is_added: false,
                    expires_ms,
                });
            }
        });
    }
    true
}

/// If `pos` is over one of the simulated per-point Symmetry break-off glyphs, remove only that Symmetry constraint.
pub fn break_symmetry_at(app: &mut SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    let hit = OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        let best_idx = st
            .sym_break_previews
            .iter()
            .enumerate()
            .filter(|(_, p)| p.glyph_rect.expand(3.0).contains(pos))
            .min_by(|(_, a), (_, b)| a.glyph_rect.center().distance(pos).total_cmp(&b.glyph_rect.center().distance(pos)))
            .map(|(i, _)| i)?;
        Some(st.sym_break_previews.swap_remove(best_idx).constraint_id)
    });
    let Some(id) = hit else { return false };
    let _ = app.run("sketch.delete", json!({ "entities": [id] }));
    true
}

fn constraint_tool_command(kind: &ConstraintKind) -> Option<&'static str> {
    use ConstraintKind::*;
    match kind {
        Coincident { .. } | PointOnCurve { .. } => Some("sketch.constraint.coincident"),
        Horizontal { .. } | HorizontalPoints { .. } | Vertical { .. } | VerticalPoints { .. } => Some("sketch.constraint.horizontal_vertical"),
        Parallel { .. } => Some("sketch.constraint.parallel"),
        Perpendicular { .. } => Some("sketch.constraint.perpendicular"),
        Collinear { .. } => Some("sketch.constraint.collinear"),
        Tangent { .. } => Some("sketch.constraint.tangent"),
        Equal { .. } => Some("sketch.constraint.equal"),
        Concentric { .. } => Some("sketch.constraint.concentric"),
        Midpoint { .. } | PointAlong { .. } => Some("sketch.constraint.midpoint"),
        Symmetric { .. } => Some("sketch.constraint.symmetry"),
        Fix { .. } => Some("sketch.constraint.fix"),
        Smooth { .. } => Some("sketch.constraint.curvature"),
        _ => None,
    }
}

/// If `pos` is over a constraint glyph (or simulated detach/symmetry break-off glyph), activate its tool.
pub fn activate_tool_at(app: &mut SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    let simulated_cmd = OVERLAY.with(|cell| {
        let st = cell.borrow();
        if st.detach_preview.as_ref().is_some_and(|d| d.glyph_rect.expand(3.0).contains(pos)) {
            Some("sketch.constraint.coincident")
        } else if st.sym_break_previews.iter().any(|s| s.glyph_rect.expand(3.0).contains(pos)) {
            Some("sketch.constraint.symmetry")
        } else {
            None
        }
    });
    let cmd = simulated_cmd.or_else(|| {
        let id = glyph_at(pos)?;
        let st = crate::open_meshing::shown_world(app);
        active_sketch(app)
            .and_then(|s| st.sketch(s))
            .and_then(|ss| ss.sketch.constraints.iter().find(|c| c.id == id))
            .and_then(|c| constraint_tool_command(&c.kind))
    });
    let Some(tool_id) = cmd else { return false };
    clear_clicked_constraint();
    let _ = app.session.execute("select.clear", &json!({}));
    app.start(tool_id);
    true
}

/// Click a collapsed constraint group badge at `pos` to expand it immediately without waiting for hover delay.
pub fn click_group_badge(app: &SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        let reach = st.glyph_px * 0.1;
        let Some((key, rect)) = st.collapsed_badges.iter().find(|(_, r)| r.expand(reach).contains(pos)).copied() else {
            return false;
        };
        st.badge_expanded = Some((key, rect));
        st.badge_hover.clear();
        true
    })
}

/// True when `pos` is over a collapsed group badge or inside an expanded group container.
pub fn over_group_badge_or_expanded(pos: Pos2) -> bool {
    OVERLAY.with(|cell| {
        let st = cell.borrow();
        let reach = st.glyph_px * 0.1;
        st.collapsed_badges.iter().any(|(_, r)| r.expand(reach).contains(pos))
            || st.badge_expanded.is_some_and(|(_, r)| r.contains(pos))
            || st.geom_expanded.iter().any(|(_, r)| r.contains(pos))
    })
}

/// True when `pos` is over any visible constraint glyph, collapsed group badge, or group box container.
pub fn over_constraint_ui(app: &SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    OVERLAY.with(|cell| cell.borrow().hit_rects.iter().any(|r| r.contains(pos))) || glyph_at(pos).is_some() || over_group_badge_or_expanded(pos)
}

#[derive(Clone, Debug)]
struct ConstraintSnap {
    id: String,
    kind_name: &'static str,
    pattern_icon: Option<&'static str>,
    anchors: Vec<(Vec2, Option<Vec2>)>,
}

pub struct HistoryConstraintSnapshot {
    sketch: u64,
    items: Vec<ConstraintSnap>,
}

/// Snapshot non-dimension constraints of the active sketch before undo/redo or deletion.
pub fn snapshot_active_constraints(app: &SolveApp) -> Option<HistoryConstraintSnapshot> {
    let sid = active_sketch(app)?;
    let sk = app.session.doc.sketch(sid).ok()?;
    let (groups, rel_groups) = anchor_groups_with_rel(sk);
    let mut items = Vec::new();
    for c in &sk.constraints {
        if c.kind.is_dimension() {
            continue;
        }
        let mut anchors: Vec<(Vec2, Option<Vec2>)> =
            groups.iter().filter(|g| g.constraint_ids.iter().any(|id| id == &c.id)).map(|g| (g.at, g.dir)).collect();
        if anchors.is_empty() {
            for rg in &rel_groups {
                if rg.constraint_ids.iter().any(|id| id == &c.id) {
                    for &ci in &rg.curves {
                        if let Some(g) = groups.iter().find(|g| g.key == AnchorKey::Curve(ci)) {
                            anchors.push((g.at, g.dir));
                        }
                    }
                }
            }
        }
        if let ConstraintKind::Symmetric { p, q, .. } = c.kind {
            for pi in [p, q] {
                if let Some(pt) = sk.point(pi) {
                    anchors.push((pt, None));
                }
            }
        }
        if anchors.is_empty() {
            let (cs, ps) = constraint_entities(&c.kind);
            for pi in ps {
                if let Some(pt) = sk.point(pi) {
                    anchors.push((pt, None));
                }
            }
            for ci in cs {
                let poly = sk.polyline(ci);
                let total: f64 = poly.windows(2).map(|w| w[0].dist(w[1])).sum();
                if let Some((pt, dir)) = poly_point_at_frac(&poly, total, 0.5) {
                    anchors.push((pt, dir));
                }
            }
        }
        items.push(ConstraintSnap { id: c.id.clone(), kind_name: c.kind.name(), pattern_icon: pattern_icon_for(sk, &c.id), anchors });
    }
    Some(HistoryConstraintSnapshot { sketch: sid, items })
}

/// Diff active sketch constraints after undo/redo or deletion and record green/red flash entries.
pub fn record_history_constraint_diff(app: &SolveApp, before: HistoryConstraintSnapshot, flash_added: bool) {
    let Some(sid) = active_sketch(app).filter(|&s| s == before.sketch) else {
        OVERLAY.with(|c| c.borrow_mut().history_flash = HistoryFlashState::new());
        return;
    };
    let Ok(sk) = app.session.doc.sketch(sid) else { return };
    let expires_ms = crate::now_ms() + HISTORY_FLASH_MS;
    let before_ids: HashSet<String> = before.items.iter().map(|i| i.id.clone()).collect();
    let after_ids: HashSet<String> = sk.constraints.iter().filter(|c| !c.kind.is_dimension()).map(|c| c.id.clone()).collect();

    OVERLAY.with(|cell| {
        let fl = &mut cell.borrow_mut().history_flash;
        if fl.sketch != Some(sid) {
            *fl = HistoryFlashState::new();
            fl.sketch = Some(sid);
        }
        if flash_added {
            for c in &sk.constraints {
                if c.kind.is_dimension() || before_ids.contains(c.id.as_str()) {
                    continue;
                }
                fl.added.insert(c.id.clone(), expires_ms);
                fl.ghosts.retain(|g| !(g.id == c.id && !g.is_added));
                if let ConstraintKind::Symmetric { p, q, .. } = c.kind {
                    for pi in [p, q] {
                        if let Some(pt) = sk.point(pi) {
                            fl.ghosts.push(ConstraintFlashGhost {
                                id: c.id.clone(),
                                kind_name: c.kind.name(),
                                pattern_icon: None,
                                at: pt,
                                dir: None,
                                is_added: true,
                                expires_ms,
                            });
                        }
                    }
                }
            }
        }
        for snap in before.items {
            if after_ids.contains(snap.id.as_str()) {
                continue;
            }
            fl.added.remove(&snap.id);
            fl.ghosts.retain(|g| g.id != snap.id);
            for (at, dir) in snap.anchors {
                fl.ghosts.push(ConstraintFlashGhost {
                    id: snap.id.clone(),
                    kind_name: snap.kind_name,
                    pattern_icon: snap.pattern_icon,
                    at,
                    dir,
                    is_added: false,
                    expires_ms,
                });
            }
        }
    });
}
