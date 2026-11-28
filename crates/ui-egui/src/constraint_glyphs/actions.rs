//! Constraint glyph hit-testing, badge click expansion, and Delete-key removal.

use egui::Pos2;
use serde_json::json;
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{ConstraintKind, Sketch};

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

use super::grouping::{CurveRelKind, find_point_cluster, glyph_deletion};
use super::{DETACH_HOVER_DELAY_SECS, OVERLAY, ViewKey};

fn view_matches(view: Option<ViewKey>) -> bool {
    let Some(v) = view else { return true };
    OVERLAY.with(|cell| cell.borrow().published_view == Some(v))
}

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
        let in_box = st.glyphs.iter().filter(|(_, c)| (c.x - pos.x).abs() <= half && (c.y - pos.y).abs() <= half).map(|(id, c)| (id.as_str(), *c));
        let hit_box = crate::sketch_tools::nearest_glyph(in_box, pos, f32::INFINITY);
        let hit_reach = crate::sketch_tools::nearest_glyph(st.glyphs.iter().map(|(id, c)| (id.as_str(), *c)), pos, reach);
        crate::sketch_tools::nearest_glyph(hit_box.into_iter().chain(hit_reach), pos, f32::INFINITY).map(|(id, _)| id.to_owned())
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
        let candidates = st.drawn_rel.iter().filter(|r| r.id == id).map(|r| (r, r.center));
        let picked = crate::sketch_tools::nearest_glyph(candidates, pos, reach)
            .map(|(r, _)| (id.to_owned(), r.kind, r.curve_id.clone(), r.all_constraint_ids.clone()));
        st.picked_rel = picked;
    });
}

/// Clear any curve-specific Equal/Parallel/Symmetry click target when selection moves elsewhere.
pub fn clear_clicked_constraint() {
    OVERLAY.with(|cell| cell.borrow_mut().picked_rel = None);
}

pub(crate) fn sketch_px_per_mm(plane: &Plane, proj: &Proj) -> Option<f32> {
    let a = proj.to_screen(plane.to_world(Vec2::ZERO))?;
    let b = proj.to_screen(plane.to_world(Vec2::new(1.0, 0.0)))?;
    Some(a.distance(b))
}

pub(crate) fn detach_distance_mm_for_view(plane: &Plane, proj: &Proj) -> f64 {
    10.0 / sketch_px_per_mm(plane, proj).unwrap_or(1.0).max(1e-3) as f64
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

fn point_is_intersection_for_curve(sk: &Sketch, ci: usize, pi: usize) -> bool {
    if !sk.curves.get(ci).is_some_and(|c| c.kind.uses(pi)) {
        return false;
    }
    let users = sk.curves.iter().filter(|c| c.kind.uses(pi)).count();
    let has_coincident = sk.constraints.iter().any(|c| matches!(c.kind, ConstraintKind::Coincident { p, q } if p == pi || q == pi));
    users > 1 || has_coincident || pi == 0
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct DetachTarget {
    pub(crate) curve_id: String,
    pub(crate) curve_idx: usize,
    pub(crate) point_idx: usize,
    pub(crate) point_id: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct ResolvedDetach {
    pub(crate) cluster_root: usize,
    pub(crate) anchor_pi: usize,
    pub(crate) selected_curve_id: Option<String>,
    pub(crate) targets: Vec<DetachTarget>,
}

/// Screen-pixel distance from `p` to segment `[a, b]` for the point-hover detach corridor.
fn screen_point_to_segment_dist(p: Pos2, a: Pos2, b: Pos2) -> f32 {
    let ab = b - a;
    let len_sq = ab.length_sq();
    let t = if len_sq > 1e-6 { ((p - a).dot(ab) / len_sq).clamp(0.0, 1.0) } else { 0.0 };
    p.distance(a + ab * t)
}

pub(crate) fn detach_targets_for_cluster(
    sk: &Sketch,
    parent: &mut [usize],
    cluster_root: usize,
    selected_curve: Option<(&str, usize)>,
) -> Option<(Option<String>, Vec<DetachTarget>)> {
    if let Some((sel_id, sel_ci)) = selected_curve
        && let Some(curve) = sk.curves.get(sel_ci)
        && let Some(target_pi) = curve.kind.point_ids().into_iter().find(|&cp| find_point_cluster(parent, cp) == cluster_root)
        && point_is_intersection_for_curve(sk, sel_ci, target_pi)
        && let Some(pt) = sk.points.get(target_pi)
    {
        return Some((
            Some(sel_id.to_owned()),
            vec![DetachTarget { curve_id: sel_id.to_owned(), curve_idx: sel_ci, point_idx: target_pi, point_id: pt.id.clone() }],
        ));
    }
    let mut targets = Vec::new();
    for (ci, curve) in sk.curves.iter().enumerate() {
        if sk.is_linked_curve(ci) {
            continue;
        }
        if let Some(target_pi) = curve.kind.point_ids().into_iter().find(|&cp| find_point_cluster(parent, cp) == cluster_root)
            && point_is_intersection_for_curve(sk, ci, target_pi)
            && let Some(pt) = sk.points.get(target_pi)
        {
            targets.push(DetachTarget { curve_id: curve.id.clone(), curve_idx: ci, point_idx: target_pi, point_id: pt.id.clone() });
        }
    }
    if targets.is_empty() { None } else { Some((None, targets)) }
}

#[allow(clippy::too_many_arguments)]
pub(crate) fn resolve_detach_targets(
    app: &SolveApp,
    sid: u64,
    sk: &Sketch,
    parent: &mut [usize],
    hover: Option<Pos2>,
    scale: f32,
    now: f64,
    ctx: &egui::Context,
) -> Option<ResolvedDetach> {
    if app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        OVERLAY.with(|cell| cell.borrow_mut().detach_hover.clear());
        return None;
    }
    let selected = selected_detach_curve(app, sid, sk);
    let selected_ref = selected.as_ref().map(|(id, ci)| (id.as_str(), *ci));

    if let Some(Hit::SketchPoint { sketch, id, .. }) = app.viewport.hover.as_ref()
        && *sketch == sid
        && let Some(hovered_pi) = sk.resolve_point(id)
    {
        let cluster_root = find_point_cluster(parent, hovered_pi);
        if let Some((selected_curve_id, targets)) = detach_targets_for_cluster(sk, parent, cluster_root, selected_ref) {
            if selected_curve_id.is_some() {
                OVERLAY.with(|cell| cell.borrow_mut().detach_hover.clear());
                return Some(ResolvedDetach { cluster_root, anchor_pi: hovered_pi, selected_curve_id, targets });
            }
            let ready = OVERLAY.with(|cell| {
                let mut st = cell.borrow_mut();
                if st.detach_preview.as_ref().is_some_and(|p| p.cluster_root == cluster_root && p.selected_curve_id.is_none()) {
                    st.detach_hover.active = Some(cluster_root);
                    st.detach_hover.pending = None;
                    true
                } else {
                    st.detach_hover.update(Some(cluster_root), now, DETACH_HOVER_DELAY_SECS, ctx) == Some(cluster_root)
                }
            });
            return ready.then_some(ResolvedDetach { cluster_root, anchor_pi: hovered_pi, selected_curve_id, targets });
        }
    }

    let sticky = hover.and_then(|h| {
        OVERLAY.with(|cell| {
            let st = cell.borrow();
            let prev = st.detach_preview.as_ref()?;
            let corridor_r = (prev.box_rect.width().min(prev.box_rect.height()) * 0.5).max(12.0 * scale);
            let keep = prev.box_rect.expand(10.0 * scale).contains(h)
                || prev.point_rect.expand(8.0 * scale).contains(h)
                || screen_point_to_segment_dist(h, prev.point_rect.center(), prev.box_rect.center()) <= corridor_r;
            keep.then(|| (prev.cluster_root, prev.selected_curve_id.clone()))
        })
    });
    if let Some((cluster_root, prev_sel)) = sticky
        && let Some((selected_curve_id, targets)) = detach_targets_for_cluster(sk, parent, cluster_root, selected_ref)
        && selected_curve_id == prev_sel
    {
        let anchor_pi = targets[0].point_idx;
        return Some(ResolvedDetach { cluster_root, anchor_pi, selected_curve_id, targets });
    }

    OVERLAY.with(|cell| cell.borrow_mut().detach_hover.clear());
    None
}

/// If `pos` is over a simulated coincident break-off glyph, detach that curve from the point.
pub fn detach_simulated_at(app: &mut SolveApp, view: Option<ViewKey>, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") || !view_matches(view) {
        return false;
    }
    let hit = OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        let preview = st.detach_preview.as_ref()?;
        let candidates = preview.items.iter().filter(|it| it.glyph_rect.expand(3.0).contains(pos)).map(|it| (it, it.glyph_rect.center()));
        let (item, _) = crate::sketch_tools::nearest_glyph(candidates, pos, f32::INFINITY)?;
        let out = (item.curve_id.clone(), item.point_id.clone(), preview.distance_mm);
        st.detach_preview = None;
        st.detach_hover.clear();
        Some(out)
    });
    let Some((curve, point, distance)) = hit else { return false };
    let _ = app.run("sketch.detach_point", json!({ "curve": curve, "point": point, "distance": distance }));
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
    let entities: Vec<String> = active_glyph_deletion(app, &id)
        .into_iter()
        .filter_map(|s| match s {
            Sel::SketchConstraint { id } | Sel::SketchPoint { id } => Some(id),
            _ => None,
        })
        .collect();
    if entities.is_empty() {
        return false;
    }
    let _ = app.run("sketch.delete", json!({ "entities": entities, "preserve_projection_link": app.ui.project_link_tools }));
    true
}

/// Click a collapsed constraint group badge at `pos` to expand it immediately without waiting for hover delay.
pub fn click_group_badge(app: &SolveApp, view: Option<ViewKey>, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") || !view_matches(view) {
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
        st.collapsed_badges.iter().any(|(_, r)| r.expand(reach).contains(pos)) || st.badge_expanded.is_some_and(|(_, r)| r.contains(pos))
    })
}

/// True when `pos` is over any visible constraint glyph, collapsed group badge, or group box container.
pub fn over_constraint_ui(app: &SolveApp, pos: Pos2) -> bool {
    if app.session.active_sketch.is_none() || app.ui.pick_off.iter().any(|x| x == "constraints") {
        return false;
    }
    OVERLAY.with(|cell| cell.borrow().hit_rects.iter().any(|r| r.contains(pos))) || glyph_at(pos).is_some() || over_group_badge_or_expanded(pos)
}
