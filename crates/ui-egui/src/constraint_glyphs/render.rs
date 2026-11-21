//! Rendering pipeline for sketch constraint glyphs, badges, expanded boxes, simulated break-offs, and flash ghosts.

use std::collections::{HashMap, HashSet};

use egui::{Pos2, Stroke};
use solidcore_record::{Plane, Vec3};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::SolvedSketch;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{ConstraintKind, CurveKind, Sketch};

use crate::SolveApp;
use crate::icons::markers::{glyph_icon, glyph_letter};
use crate::viewport::{Hit, Proj};

use super::actions::{active_sketch, compute_detach_nudge, resolve_detach_target, selected_symmetric_point, sketch_px_per_mm};
use super::grouping::{
    AnchorKey, ConstraintAnchorGroup, CurveRelGroup, CurveRelKind, HoveredGeom, SelectedGeomKey, anchor_groups_with_rel, constraint_entities,
    constraint_relates_to_geom, find_point_cluster, glyph_constraints, glyph_group, point_cluster_map,
};
use super::layout::{PlacementScene, glyph_label_rect, glyph_label_wrap_width, square_grid_dims};
use super::{
    BADGE_HOVER_DELAY_SECS, CONSTRAINT_GROUP_NEVER, ConstraintFlashGhost, DetachPreview, DrawnRelInfo, GEOM_HOVER_DELAY_SECS, GLYPH, HISTORY_FADE_MS,
    HistoryFlashState, MIN_GLYPH_SPAN_PX, OVERLAY, SymmetryBreakPreview, constraint_glyphs_visible_at_scale, glyph_px, pattern_icon_for,
};

const MIN_VIEW_NORMAL_COS: f64 = 0.342_020_143_325_668_8;

fn visible_at_angle(plane: &Plane, cam: &solidcraft_viewport::Camera) -> bool {
    cam.back().dot(plane.normal()).abs() >= MIN_VIEW_NORMAL_COS - 1e-6
}

fn sketch_span_px(ss: &SolvedSketch, proj: &Proj) -> f32 {
    let mut r = egui::Rect::NOTHING;
    for p in &ss.sketch.points {
        if let Some(q) = proj.to_screen(ss.plane.to_world(p.pos)) {
            r.extend_with(q);
            if r.width().max(r.height()) >= MIN_GLYPH_SPAN_PX {
                return MIN_GLYPH_SPAN_PX;
            }
        }
    }
    for ci in 0..ss.sketch.curves.len() {
        for p in ss.sketch.polyline(ci) {
            if let Some(q) = proj.to_screen(ss.plane.to_world(p)) {
                r.extend_with(q);
                if r.width().max(r.height()) >= MIN_GLYPH_SPAN_PX {
                    return MIN_GLYPH_SPAN_PX;
                }
            }
        }
    }
    if r.min.x <= r.max.x { r.width().max(r.height()) } else { 0.0 }
}

fn hovered_sketch_geom(app: &SolveApp, sid: u64, sk: &Sketch, parent: &mut [usize]) -> Option<HoveredGeom> {
    if !app.preferences.show_constraints_on_geometry_hover || app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        return None;
    }
    match app.viewport.hover.as_ref()? {
        Hit::SketchCurve { sketch, id, .. } | Hit::SketchMidpoint { sketch, id, .. } if *sketch == sid => sk.curve_index(id).map(HoveredGeom::Curve),
        Hit::SketchPoint { sketch, id, .. } if *sketch == sid => {
            sk.resolve_point(id).map(|p| HoveredGeom::PointCluster(find_point_cluster(parent, p)))
        }
        _ => None,
    }
}

fn last_selected_sketch_geom(app: &SolveApp, sid: u64, sk: &Sketch, parent: &mut [usize]) -> Option<(SelectedGeomKey, HoveredGeom)> {
    if !app.preferences.show_constraints_on_geometry_hover || app.tool.is_some() || app.dialog.as_ref().is_some_and(|d| d.wants_picks()) {
        return None;
    }
    app.session.selection.iter().rev().find_map(|sel| match sel {
        Sel::SketchCurve { id, sketch } if sketch.is_none_or(|s| s == sid) => {
            sk.curve_index(id).map(|ci| (SelectedGeomKey::Curve(id.clone()), HoveredGeom::Curve(ci)))
        }
        Sel::SketchMidpoint { id } => sk.curve_index(id).map(|ci| (SelectedGeomKey::Curve(id.clone()), HoveredGeom::Curve(ci))),
        Sel::SketchPoint { id } => {
            sk.resolve_point(id).map(|p| (SelectedGeomKey::Point(id.clone()), HoveredGeom::PointCluster(find_point_cluster(parent, p))))
        }
        _ => None,
    })
}

fn highlight_constraints_entities(ss: &SolvedSketch, painter: &egui::Painter, proj: &Proj, tk: &solidcraft_theme::Tokens, ids: &[String]) {
    let (mut cs, mut ps) = (Vec::new(), Vec::new());
    for c in ss.sketch.constraints.iter().filter(|c| ids.contains(&c.id)) {
        let (c_ids, p_ids) = constraint_entities(&c.kind);
        cs.extend(c_ids);
        ps.extend(p_ids);
    }
    let mut budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
    for ci in cs {
        crate::tools::paint_curve(&ss.sketch, ci, &ss.plane, painter, proj, Stroke::new(3.0, tk.accent.gamma_multiply(0.7)), &mut budget);
    }
    for pi in ps {
        if let Some(q) = ss.sketch.point(pi).and_then(|q| proj.to_screen(ss.plane.to_world(q))) {
            painter.circle_filled(q, 4.5, tk.accent);
        }
    }
}

struct PlacedGroup {
    group: ConstraintAnchorGroup,
    grouped: bool,
    has_box: bool,
    cols: usize,
    base: Pos2,
    rect: egui::Rect,
}

struct GlyphMeta<'a> {
    rel: Option<(&'a CurveRelGroup, usize, &'a str)>,
    hovered_rel: Option<(CurveRelKind, usize)>,
    picked_rel: Option<&'a (String, CurveRelKind, String, Vec<String>)>,
    flash_alpha: Option<f32>,
}

#[allow(clippy::too_many_arguments)]
fn paint_glyph_cell(
    app: &SolveApp,
    ss: &SolvedSketch,
    painter: &egui::Painter,
    tk: &solidcraft_theme::Tokens,
    id: &str,
    center: Pos2,
    glyph: f32,
    scale: f32,
    picked: Option<&str>,
    hovered_at: Option<&(String, Pos2)>,
    geom_lit: bool,
    meta: &GlyphMeta<'_>,
    drawn: &mut Vec<(String, Pos2)>,
    drawn_rel: &mut Vec<DrawnRelInfo>,
) {
    let Some(c) = ss.sketch.constraints.iter().find(|c| c.id == id) else { return };
    let group = glyph_group(&ss.sketch, id);
    let check_ids: &[String] = match meta.rel {
        Some((rg, _, _)) => &rg.constraint_ids,
        None => &group,
    };
    let failing = check_ids.iter().any(|g| ss.report.failing.contains(g));
    let redundant = !failing && check_ids.iter().all(|g| ss.report.redundant.contains(g));
    let selected = if let Some((rg, _, curve_id)) = meta.rel {
        meta.picked_rel.is_some_and(|(rid, kind, cid, _)| rid == id && *kind == rg.kind && cid == curve_id)
            || (meta.picked_rel.is_none()
                && app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if rg.constraint_ids.contains(x))))
    } else {
        picked == Some(id) || app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if group.contains(x)))
    };
    let lit = selected
        || hovered_at.is_some_and(|(hid, pos)| hid == id && pos.distance(center) < 1.0)
        || meta.rel.is_some_and(|(rg, _, _)| meta.hovered_rel == Some((rg.kind, rg.index)))
        || geom_lit;
    let col = if let Some(alpha) = meta.flash_alpha {
        tk.agent_ok.gamma_multiply(alpha)
    } else if failing {
        crate::viewport::Conflict::glyph_colour(app, tk)
    } else if lit {
        tk.accent
    } else if redundant {
        tk.glyph_text.gamma_multiply(0.45)
    } else {
        tk.glyph_text
    };
    let r = egui::Rect::from_center_size(center, egui::vec2(glyph, glyph));
    let bg = if let Some(alpha) = meta.flash_alpha {
        tk.agent_ok.gamma_multiply(0.24 * alpha)
    } else if lit {
        tk.accent_soft
    } else {
        tk.glyph_bg
    };
    let stroke = if let Some(alpha) = meta.flash_alpha {
        Stroke::new(1.5, tk.agent_ok.gamma_multiply(alpha))
    } else {
        Stroke::new(1.0, if failing || lit { col } else { tk.glyph_edge })
    };
    let rounding = tk.radius_sm.saturating_sub(1);
    painter.rect_filled(r, rounding, bg);
    painter.rect_stroke(r, rounding, stroke, egui::StrokeKind::Inside);

    let has_group_num = meta.rel.is_some_and(|(rg, _, _)| rg.multi);
    let icon_rect = if has_group_num {
        let side = (glyph * 0.62).max(6.0);
        egui::Rect::from_min_size(r.left_top() + egui::vec2(scale, scale), egui::vec2(side, side))
    } else {
        r.shrink(2.0 * scale)
    };
    match pattern_icon_for(&ss.sketch, id).or_else(|| glyph_icon(c.kind.name())) {
        Some(icon) => crate::icons::paint_icon(painter, icon_rect, icon, f32::from(col.a()) / 255.0),
        None => {
            painter.text(
                icon_rect.center(),
                egui::Align2::CENTER_CENTER,
                glyph_letter(c.kind.name()),
                egui::FontId::proportional(tk.font_panel * scale),
                col,
            );
        }
    }
    if let Some((rg, _, curve_id)) = meta.rel {
        if rg.multi {
            painter.text(
                r.right_bottom() - egui::vec2(1.0 * scale, 0.5 * scale),
                egui::Align2::RIGHT_BOTTOM,
                rg.index.to_string(),
                egui::FontId::proportional((tk.font_small * 0.68 * scale).max(7.5)),
                col,
            );
        }
        drawn_rel.push(DrawnRelInfo {
            id: id.to_owned(),
            center,
            kind: rg.kind,
            group_index: rg.index,
            multi: rg.multi,
            curve_id: curve_id.to_owned(),
            all_constraint_ids: rg.constraint_ids.clone(),
        });
    }
    drawn.push((id.to_owned(), center));
}

#[allow(clippy::too_many_arguments)]
fn paint_group_badge(
    app: &SolveApp,
    ss: &SolvedSketch,
    painter: &egui::Painter,
    tk: &solidcraft_theme::Tokens,
    ids: &[String],
    rect: egui::Rect,
    scale: f32,
    picked: Option<&str>,
    hovered: bool,
) {
    let failing = ids.iter().any(|id| glyph_group(&ss.sketch, id).iter().any(|g| ss.report.failing.contains(g)));
    let redundant = !failing && ids.iter().all(|id| glyph_group(&ss.sketch, id).iter().all(|g| ss.report.redundant.contains(g)));
    let selected = ids.iter().any(|id| {
        let g = glyph_group(&ss.sketch, id);
        picked == Some(id.as_str()) || app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if g.contains(x)))
    });
    let lit = selected || hovered;
    let col = if failing {
        crate::viewport::Conflict::glyph_colour(app, tk)
    } else if lit {
        tk.accent
    } else if redundant {
        tk.glyph_text.gamma_multiply(0.45)
    } else {
        tk.glyph_text
    };
    let rounding = tk.radius_sm.saturating_sub(1);
    painter.rect_filled(rect, rounding, if lit { tk.accent_soft } else { tk.glyph_bg });
    painter.rect_stroke(rect, rounding, Stroke::new(1.0, if failing || lit { col } else { tk.glyph_edge }), egui::StrokeKind::Inside);
    let label = ids.len().to_string();
    let font_size = if label.len() > 1 { tk.font_small * scale } else { tk.font_panel * scale };
    painter.text(rect.center(), egui::Align2::CENTER_CENTER, label, egui::FontId::proportional(font_size), col);
}

#[allow(clippy::too_many_arguments)]
fn paint_accent_icon_badge(
    painter: &egui::Painter,
    tk: &solidcraft_theme::Tokens,
    base: Pos2,
    rect: egui::Rect,
    scale: f32,
    icon_name: &str,
    lit: bool,
    draw_leader: bool,
) {
    if draw_leader {
        painter.line_segment([base, rect.center()], Stroke::new(1.0, tk.accent.gamma_multiply(0.8)));
    }
    let rounding = tk.radius_sm.saturating_sub(1);
    painter.rect_filled(rect, rounding, if lit { tk.accent_soft } else { tk.glyph_bg });
    painter.rect_stroke(rect, rounding, Stroke::new(1.2, tk.accent), egui::StrokeKind::Inside);
    crate::icons::paint_icon(painter, rect.shrink(2.0 * scale), icon_name, 1.0);
}

fn collect_history_flash(sid: u64, ctx: &egui::Context) -> (HashMap<String, f32>, Vec<(ConstraintFlashGhost, f32)>) {
    let now_ms = crate::now_ms();
    OVERLAY.with(|cell| {
        let fl = &mut cell.borrow_mut().history_flash;
        if fl.sketch != Some(sid) {
            *fl = HistoryFlashState::new();
            return (HashMap::new(), Vec::new());
        }
        fl.added.retain(|_, &mut exp| exp > now_ms);
        fl.ghosts.retain(|g| g.expires_ms > now_ms);
        let deleted_alpha = |exp: f64| ((exp - now_ms).max(0.0) / HISTORY_FADE_MS).clamp(0.0, 1.0) as f32;
        let added = fl.added.keys().map(|id| (id.clone(), 1.0_f32)).collect();
        let ghosts = fl.ghosts.iter().map(|g| (g.clone(), if g.is_added { 1.0_f32 } else { deleted_alpha(g.expires_ms) })).collect();
        if !fl.added.is_empty() || !fl.ghosts.is_empty() {
            let fading = fl.ghosts.iter().any(|g| !g.is_added && g.expires_ms - now_ms <= HISTORY_FADE_MS + 30.0);
            ctx.request_repaint_after(std::time::Duration::from_millis(if fading { 16 } else { 50 }));
        }
        (added, ghosts)
    })
}

/// Per-frame constraint glyph layout and rendering entry point.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj, hover: Option<Pos2>) {
    let Some(sid) = active_sketch(app) else {
        OVERLAY.with(|c| c.borrow_mut().clear_frame());
        return;
    };
    let st = crate::dim_view::preview_world(app).unwrap_or_else(|| crate::open_meshing::session_world(&app.session));
    let Some(ss) = st.sketch(sid) else {
        OVERLAY.with(|c| c.borrow_mut().clear_frame());
        return;
    };
    let glyph = glyph_px(app.preferences.constraint_glyph_scale);
    let scale = glyph / GLYPH;
    OVERLAY.with(|c| c.borrow_mut().glyph_px = glyph);

    let (history_added, history_ghosts) = collect_history_flash(sid, painter.ctx());
    let has_history_flash = !history_added.is_empty() || !history_ghosts.is_empty();
    let only_flashed = ss.sketch.view.hide_constraints && has_history_flash;
    if (ss.sketch.view.hide_constraints && !has_history_flash) || !visible_at_angle(&ss.plane, &proj.cam) {
        OVERLAY.with(|c| c.borrow_mut().clear_frame());
        return;
    }
    let px_per_mm = sketch_px_per_mm(&ss.plane, proj);
    OVERLAY.with(|c| c.borrow_mut().last_px_per_mm = px_per_mm.unwrap_or(1.0));
    if !has_history_flash
        && let Some(px) = px_per_mm
        && !constraint_glyphs_visible_at_scale(px, app.preferences.hide_constraint_glyphs_zoomed_out, || sketch_span_px(ss, proj))
    {
        OVERLAY.with(|c| c.borrow_mut().clear_frame());
        return;
    }

    let tk = solidcraft_theme::Tokens::get();
    let (picked, picked_rel) = OVERLAY.with(|c| {
        let s = c.borrow();
        (s.picked.clone(), s.picked_rel.clone())
    });
    let vis_points = solidcraft_engine::view::SketchPointVisibility::new(&ss.sketch);
    let mut point_obstacles: Vec<egui::Rect> = ss
        .sketch
        .points
        .iter()
        .filter(|p| vis_points.takes(p))
        .filter_map(|p| proj.to_screen(ss.plane.to_world(p.pos)))
        .map(|sp| egui::Rect::from_center_size(sp, egui::vec2(6.0 * scale, 6.0 * scale)))
        .collect();
    if app.origin_visible()
        && let Some(op) = proj.to_screen(Vec3::ZERO)
    {
        point_obstacles.push(egui::Rect::from_center_size(op, egui::vec2(6.0 * scale, 6.0 * scale)));
    }
    let curves: Vec<(egui::Rect, Vec<Pos2>)> = (0..ss.sketch.curves.len())
        .filter(|&ci| solidcraft_engine::view::sketch_curve_visible(&ss.sketch, ci))
        .filter_map(|ci| {
            let pts: Vec<Pos2> = ss.sketch.polyline(ci).into_iter().filter_map(|p| proj.to_screen(ss.plane.to_world(p))).collect();
            let first = *pts.first()?;
            let mut aabb = egui::Rect::from_min_max(first, first);
            for &pt in &pts[1..] {
                aabb.extend_with(pt);
            }
            Some((aabb, pts))
        })
        .collect();
    let mut scene = PlacementScene {
        dim_texts: crate::dim_view::text_rects().into_iter().map(|(_, r)| r.expand(2.0)).collect(),
        points: point_obstacles,
        curves,
        placed: Vec::new(),
    };

    let cell_gap = (2.0 * scale).round().max(2.0);
    let pad = (4.0 * scale).round().max(3.0);
    let threshold = app.preferences.constraint_group_threshold;
    let only_on_hover = app.preferences.show_constraints_on_geometry_hover && app.preferences.only_show_constraints_on_geometry_hover;
    let now = painter.ctx().input(|i| i.time);
    let expand_margin = 10.0 * scale;
    let mut point_parent = point_cluster_map(&ss.sketch);
    let hovered_geom = hovered_sketch_geom(app, sid, &ss.sketch, &mut point_parent);
    let last_sel = last_selected_sketch_geom(app, sid, &ss.sketch, &mut point_parent);

    // Virtual-corner hover/selection reveal (400 ms dwell on hover, immediate on left-click).
    let vc_points: HashSet<usize> = ss.sketch.virtual_corners().into_iter().map(|(pi, _)| pi).collect();
    let mut raw_vc_hover: Option<usize> = None;
    let mut revealed_vc_selected: HashSet<usize> = HashSet::new();
    if !vc_points.is_empty() {
        if let Some(Hit::SketchPoint { sketch, id, .. }) = app.viewport.hover.as_ref()
            && *sketch == sid
            && let Some(pi) = ss.sketch.resolve_point(id)
            && vc_points.contains(&pi)
        {
            raw_vc_hover = Some(pi);
        }
        if raw_vc_hover.is_none()
            && let Some(h) = hover
        {
            raw_vc_hover = vc_points
                .iter()
                .copied()
                .filter_map(|pi| ss.sketch.point(pi).and_then(|p| proj.to_screen(ss.plane.to_world(p))).map(|sp| (pi, sp.distance(h))))
                .filter(|&(_, d)| d <= 9.0 * scale)
                .min_by(|a, b| a.1.total_cmp(&b.1))
                .map(|(pi, _)| pi)
                .or_else(|| {
                    OVERLAY.with(|c| {
                        c.borrow().vc_boxes.iter().find(|&&(pi, r)| vc_points.contains(&pi) && r.expand(expand_margin).contains(h)).map(|&(pi, _)| pi)
                    })
                });
        }
        for sel in &app.session.selection {
            if let Sel::SketchPoint { id } = sel
                && let Some(pi) = ss.sketch.resolve_point(id)
                && vc_points.contains(&pi)
            {
                revealed_vc_selected.insert(pi);
            }
        }
    }

    let (geom_active, geom_pinned, locked_geom, prev_badge_expanded, vc_hovered_active) = OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        if !app.preferences.show_constraints_on_geometry_hover {
            st.geom_hover.clear();
            st.geom_expanded.clear();
            st.geom_pinned = None;
            st.locked_sel = None;
        } else {
            let prev_geom = st.geom_hover.active;
            st.geom_hover.update(hovered_geom, now, GEOM_HOVER_DELAY_SECS, painter.ctx());
            if let Some((pg, _, prect)) = st.geom_pinned
                && st.geom_hover.active.is_none_or(|a| a == pg)
                && hover.is_some_and(|h| prect.expand(expand_margin).contains(h))
            {
                // Keep geom_pinned alive while pointer stays inside its expanded box.
            } else if hovered_geom.is_none()
                && let Some(prev_g) = prev_geom
                && let Some(h) = hover
                && let Some(&(key, rect)) = st.geom_expanded.iter().find(|(_, r)| r.expand(expand_margin).contains(h))
            {
                if only_on_hover {
                    st.geom_pinned = Some((prev_g, key, rect));
                } else {
                    st.geom_pinned = None;
                    st.badge_expanded = Some((key, rect));
                }
            } else {
                st.geom_pinned = None;
            }
            if st.locked_sel.as_ref() != last_sel.as_ref().map(|(k, _)| k) {
                st.locked_sel = None;
            }
            if let (Some((sel_key, sel_geom)), Some(active_g)) = (&last_sel, st.geom_hover.active)
                && *sel_geom == active_g
            {
                st.locked_sel = Some(sel_key.clone());
            }
        }
        st.geom_expanded.clear();
        let locked = st.locked_sel.as_ref().and_then(|lk| last_sel.as_ref().filter(|(k, _)| k == lk).map(|(_, g)| *g));
        let prev_exp = if only_on_hover {
            st.badge_expanded = None;
            None
        } else if let Some((exp_key, prev_rect)) = st.badge_expanded {
            if hover.is_some_and(|h| prev_rect.expand(expand_margin).contains(h)) {
                st.badge_hover.clear();
                Some((exp_key, prev_rect))
            } else {
                st.badge_expanded = None;
                None
            }
        } else {
            None
        };
        let vc_act = st.vc_hover.update(raw_vc_hover, now, GEOM_HOVER_DELAY_SECS, painter.ctx());
        (st.geom_hover.active, st.geom_pinned, locked, prev_exp, vc_act)
    });

    let vc_poc_revealed = |p: usize| vc_hovered_active == Some(p) || revealed_vc_selected.contains(&p);
    let (mut vc_revealed_ids, mut vc_selected_ids, mut hidden_vc_ids) = (HashSet::new(), HashSet::new(), HashSet::new());
    if !vc_points.is_empty() {
        for c in glyph_constraints(&ss.sketch) {
            if let ConstraintKind::PointOnCurve { p, c: curve } = c.kind
                && vc_points.contains(&p)
                && matches!(ss.sketch.curves.get(curve).map(|cv| &cv.kind), Some(CurveKind::Line { .. } | CurveKind::Arc { .. }))
            {
                if revealed_vc_selected.contains(&p) {
                    vc_selected_ids.insert(c.id.clone());
                }
                if vc_poc_revealed(p) {
                    vc_revealed_ids.insert(c.id.clone());
                } else {
                    let forced = history_added.contains_key(&c.id)
                        || ss.report.failing.contains(&c.id)
                        || picked.as_deref() == Some(c.id.as_str())
                        || app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if x == &c.id));
                    if !forced {
                        hidden_vc_ids.insert(c.id.clone());
                    }
                }
            }
        }
    }

    let (mut raw_groups, rel_groups) = anchor_groups_with_rel(&ss.sketch);
    if !hidden_vc_ids.is_empty() {
        for g in &mut raw_groups {
            g.constraint_ids.retain(|id| !hidden_vc_ids.contains(id));
        }
        raw_groups.retain(|g| !g.constraint_ids.is_empty());
    }

    let effective_geom = geom_active.or(geom_pinned.map(|(g, _, _)| g)).or(locked_geom);
    let mut geom_lit_ids: HashSet<String> = match effective_geom {
        Some(geom) => {
            glyph_constraints(&ss.sketch).filter(|c| constraint_relates_to_geom(&mut point_parent, &c.kind, geom)).map(|c| c.id.clone()).collect()
        }
        None => HashSet::new(),
    };
    geom_lit_ids.extend(vc_revealed_ids);
    let mut flashed_ids: HashSet<String> = history_added.keys().cloned().collect();
    for rg in &rel_groups {
        if matches!(effective_geom, Some(HoveredGeom::Curve(ci)) if rg.curves.contains(&ci))
            || rg.constraint_ids.iter().any(|id| geom_lit_ids.contains(id))
        {
            geom_lit_ids.extend(rg.constraint_ids.iter().cloned());
        }
        if rg.constraint_ids.iter().any(|cid| history_added.contains_key(cid)) {
            flashed_ids.extend(rg.constraint_ids.iter().cloned());
        }
    }

    let always_visible_ids: HashSet<String> = if only_flashed {
        flashed_ids.clone()
    } else if only_on_hover {
        let mut set: HashSet<String> = glyph_constraints(&ss.sketch)
            .filter(|c| {
                let g = glyph_group(&ss.sketch, &c.id);
                g.iter().any(|x| ss.report.failing.contains(x))
                    || flashed_ids.contains(&c.id)
                    || vc_selected_ids.contains(&c.id)
                    || picked.as_deref() == Some(c.id.as_str())
                    || app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if g.contains(x)))
            })
            .map(|c| c.id.clone())
            .collect();
        for rg in &rel_groups {
            if rg.constraint_ids.iter().any(|g| {
                ss.report.failing.contains(g)
                    || flashed_ids.contains(g)
                    || picked.as_deref() == Some(g.as_str())
                    || app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if x == g))
            }) {
                set.extend(rg.constraint_ids.iter().cloned());
            }
        }
        set
    } else {
        HashSet::new()
    };

    let prev_dirs = OVERLAY.with(|c| c.borrow().prev_anchor_dirs.clone());
    let mut next_dirs: HashMap<AnchorKey, egui::Vec2> = HashMap::new();
    let cursor_obstacle = hover.map(|h| egui::Rect::from_center_size(h, egui::vec2(18.0 * scale, 18.0 * scale)));
    let mut placed_groups: Vec<PlacedGroup> = Vec::new();
    for mut group in raw_groups {
        if only_flashed {
            group.constraint_ids.retain(|id| always_visible_ids.contains(id));
        } else if only_on_hover {
            group.constraint_ids.retain(|id| always_visible_ids.contains(id) || geom_lit_ids.contains(id));
        }
        if group.constraint_ids.is_empty() {
            continue;
        }
        let Some(base) = proj.to_screen(ss.plane.to_world(group.at)) else { continue };
        let dir_screen = group.dir.and_then(|d| proj.to_screen(ss.plane.to_world(group.at + d)));
        let has_flashed = group.constraint_ids.iter().any(|id| flashed_ids.contains(id));
        let grouped = !only_on_hover
            && !only_flashed
            && !has_flashed
            && !matches!(group.key, AnchorKey::CurveJoint(..))
            && threshold < CONSTRAINT_GROUP_NEVER
            && group.constraint_ids.len() > usize::from(threshold);
        let has_box = !grouped && group.constraint_ids.len() >= 2;
        let (cols, rows) = if grouped { (1, 1) } else { square_grid_dims(group.constraint_ids.len()) };
        let box_pad = if has_box { pad * 2.0 } else { 0.0 };
        let size = egui::vec2(
            cols as f32 * glyph + cols.saturating_sub(1) as f32 * cell_gap + box_pad,
            rows as f32 * glyph + rows.saturating_sub(1) as f32 * cell_gap + box_pad,
        );
        let pinned_rect = geom_pinned.filter(|&(_, k, r)| k == group.key && (r.size() - size).length() <= 1.0).map(|(_, _, r)| r);
        let rect = pinned_rect.unwrap_or_else(|| {
            let avoid_cursor = if only_on_hover && geom_active.is_some() { cursor_obstacle } else { None };
            scene.place_group(base, dir_screen, size, glyph, avoid_cursor, prev_dirs.get(&group.key).copied())
        });
        let d = (rect.center() - base).normalized();
        if d.length_sq() > 0.1 {
            next_dirs.insert(group.key, d);
        }
        if only_on_hover && (geom_active.is_some() || geom_pinned.is_some_and(|(_, k, _)| k == group.key)) {
            OVERLAY.with(|c| c.borrow_mut().geom_expanded.push((group.key, rect)));
        }
        scene.placed.push(rect);
        placed_groups.push(PlacedGroup { group, grouped, has_box, cols, base, rect });
    }

    // Resolve direct hover/click expansion of a collapsed badge.
    let badge_expanded_idx = if only_on_hover {
        None
    } else if let Some((exp_key, _)) = prev_badge_expanded {
        let idx = placed_groups.iter().position(|p| p.grouped && p.group.key == exp_key);
        if idx.is_none() {
            OVERLAY.with(|c| c.borrow_mut().badge_expanded = None);
        }
        idx
    } else if geom_active.is_some() {
        OVERLAY.with(|c| c.borrow_mut().badge_hover.clear());
        None
    } else {
        let hovered_badge = hover.and_then(|h| {
            placed_groups
                .iter()
                .enumerate()
                .filter(|(_, p)| p.grouped && p.rect.expand(glyph * 0.1).contains(h))
                .min_by(|(_, a), (_, b)| a.rect.center().distance(h).total_cmp(&b.rect.center().distance(h)))
                .map(|(i, p)| (i, p.group.key))
        });
        OVERLAY.with(|c| {
            let mut st = c.borrow_mut();
            let active_key = st.badge_hover.update(hovered_badge.map(|(_, k)| k), now, BADGE_HOVER_DELAY_SECS, painter.ctx());
            active_key.and_then(|k| hovered_badge.filter(|(_, hk)| *hk == k).map(|(i, _)| i))
        })
    };

    let mut expanded_indices: Vec<usize> = badge_expanded_idx.into_iter().collect();
    if !only_on_hover && effective_geom.is_some() {
        for (i, p) in placed_groups.iter().enumerate() {
            if p.grouped && !expanded_indices.contains(&i) && p.group.constraint_ids.iter().any(|id| geom_lit_ids.contains(id)) {
                expanded_indices.push(i);
            }
        }
    }

    struct ExpandedBox {
        idx: usize,
        rect: egui::Rect,
        cols: usize,
    }
    let mut expanded_boxes: Vec<ExpandedBox> = Vec::new();
    for &idx in &expanded_indices {
        let Some(exp) = placed_groups.get(idx) else { continue };
        let (cols, rows) = square_grid_dims(exp.group.constraint_ids.len());
        let box_size = egui::vec2(
            cols as f32 * glyph + cols.saturating_sub(1) as f32 * cell_gap + pad * 2.0,
            rows as f32 * glyph + rows.saturating_sub(1) as f32 * cell_gap + pad * 2.0,
        );
        let must_enclose = badge_expanded_idx == Some(idx);
        let mut hard_blockers: Vec<egui::Rect> = expanded_boxes.iter().map(|b| b.rect).collect();
        if !must_enclose
            && geom_active.is_some()
            && let Some(c) = cursor_obstacle
        {
            hard_blockers.push(c);
        }
        let neighbors: Vec<egui::Rect> =
            placed_groups.iter().enumerate().filter(|(i, _)| *i != idx && !expanded_indices.contains(i)).map(|(_, p)| p.rect).collect();
        let exp_rect = scene.place_expanded(exp.rect, exp.base, box_size, proj.rect, must_enclose, &hard_blockers, &neighbors, 4.0 * scale);
        OVERLAY.with(|c| {
            let mut st = c.borrow_mut();
            if must_enclose {
                st.badge_expanded = Some((exp.group.key, exp_rect));
            }
            if geom_active.is_some() {
                st.geom_expanded.push((exp.group.key, exp_rect));
            }
        });
        expanded_boxes.push(ExpandedBox { idx, rect: exp_rect, cols });
    }
    if !expanded_boxes.is_empty() {
        let blockers: Vec<egui::Rect> = expanded_boxes.iter().map(|eb| eb.rect.expand(4.0 * scale)).collect();
        let mut settled: Vec<egui::Rect> = placed_groups
            .iter()
            .enumerate()
            .filter(|(i, p)| !expanded_indices.contains(i) && blockers.iter().all(|b| !p.rect.intersects(*b)))
            .map(|(_, p)| p.rect)
            .collect();
        for (i, p) in placed_groups.iter_mut().enumerate() {
            if !expanded_indices.contains(&i) && blockers.iter().any(|b| p.rect.intersects(*b)) {
                p.rect = scene.nudge_clear(p.rect, &blockers, &settled, proj.rect);
                settled.push(p.rect);
            }
        }
    }

    let rel_for_cell = |key: AnchorKey, cid: &str| -> Option<(&CurveRelGroup, usize, &str)> {
        let AnchorKey::Curve(ci) = key else { return None };
        let curve_id = ss.sketch.curves.get(ci).map(|c| c.id.as_str())?;
        let rg = rel_groups.iter().find(|g| g.curves.contains(&ci) && g.constraint_ids.iter().any(|x| x == cid))?;
        Some((rg, ci, curve_id))
    };

    let mut visible_glyphs: Vec<(String, Pos2, Option<(&CurveRelGroup, usize, &str)>)> = Vec::new();
    for (idx, p) in placed_groups.iter().enumerate() {
        let (rect, cols, inner_pad) = match expanded_boxes.iter().find(|b| b.idx == idx) {
            Some(eb) => (eb.rect, eb.cols, pad),
            None if !p.grouped => (p.rect, p.cols, if p.has_box { pad } else { 0.0 }),
            None => continue,
        };
        for (k, cid) in p.group.constraint_ids.iter().enumerate() {
            let c = egui::pos2(
                rect.left() + inner_pad + glyph * 0.5 + (k % cols) as f32 * (glyph + cell_gap),
                rect.top() + inner_pad + glyph * 0.5 + (k / cols) as f32 * (glyph + cell_gap),
            );
            visible_glyphs.push((cid.clone(), c, rel_for_cell(p.group.key, cid)));
        }
    }

    let hovered_vg = hover.and_then(|h| {
        visible_glyphs.iter().filter(|(_, pos, _)| pos.distance(h) <= glyph * 0.6).min_by(|a, b| a.1.distance(h).total_cmp(&b.1.distance(h)))
    });
    let hovered_at = hovered_vg.map(|(id, pos, _)| (id.clone(), *pos));
    let hovered_rel = hovered_vg.and_then(|(_, _, rel)| rel.map(|(rg, _, _)| (rg.kind, rg.index)));
    let hovered_collapsed_idx = if hovered_at.is_none() && expanded_boxes.is_empty() {
        hover.and_then(|h| {
            placed_groups
                .iter()
                .enumerate()
                .filter(|(_, p)| p.grouped && p.rect.expand(glyph * 0.1).contains(h))
                .min_by(|(_, a), (_, b)| a.rect.center().distance(h).total_cmp(&b.rect.center().distance(h)))
                .map(|(i, _)| i)
        })
    } else {
        None
    };

    // Highlight affected geometry underneath glyph boxes.
    let hovered_sym_prev = hover.and_then(|h| {
        OVERLAY.with(|c| {
            c.borrow()
                .sym_break_previews
                .iter()
                .filter(|p| p.glyph_rect.expand(2.0).contains(h))
                .min_by(|a, b| a.glyph_rect.center().distance(h).total_cmp(&b.glyph_rect.center().distance(h)))
                .map(|p| (p.axis_curve, p.other_point))
        })
    });
    if let Some((axis_curve, other_point)) = hovered_sym_prev {
        let mut budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
        crate::tools::paint_curve(&ss.sketch, axis_curve, &ss.plane, painter, proj, Stroke::new(3.0, tk.accent.gamma_multiply(0.7)), &mut budget);
        if let Some(q) = ss.sketch.point(other_point).and_then(|q| proj.to_screen(ss.plane.to_world(q))) {
            painter.circle_filled(q, 4.5, tk.accent);
        }
    } else if let Some((rg, _, _)) = hovered_vg.and_then(|(_, _, r)| *r) {
        highlight_constraints_entities(ss, painter, proj, &tk, &rg.constraint_ids);
    } else if let Some((id, _)) = &hovered_at {
        highlight_constraints_entities(ss, painter, proj, &tk, &glyph_group(&ss.sketch, id));
    } else if let Some((_, kind, curve_id, _)) = &picked_rel
        && let Some(ci) = ss.sketch.curve_index(curve_id)
        && let Some(rg) = rel_groups.iter().find(|g| g.kind == *kind && g.curves.contains(&ci))
    {
        highlight_constraints_entities(ss, painter, proj, &tk, &rg.constraint_ids);
    } else if let Some(id) = picked.as_ref() {
        highlight_constraints_entities(ss, painter, proj, &tk, &glyph_group(&ss.sketch, id));
    } else if let Some(p) = hovered_collapsed_idx.and_then(|i| placed_groups.get(i)) {
        highlight_constraints_entities(ss, painter, proj, &tk, &p.group.constraint_ids);
    } else if let Some(eb) = expanded_boxes.iter().find(|eb| hover.is_some_and(|h| eb.rect.contains(h)))
        && let Some(p) = placed_groups.get(eb.idx)
    {
        highlight_constraints_entities(ss, painter, proj, &tk, &p.group.constraint_ids);
    }

    let (mut drawn, mut drawn_rel, mut collapsed_badges, mut hit_rects) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
    let box_rounding = tk.radius_sm + 2;
    let flash_for = |cid: &str, rel: Option<(&CurveRelGroup, usize, &str)>| -> Option<f32> {
        history_added.get(cid).copied().or_else(|| {
            rel.and_then(|(rg, _, _)| {
                rg.constraint_ids
                    .iter()
                    .filter_map(|id| history_added.get(id).copied())
                    .fold(None, |a: Option<f32>, b| Some(a.map_or(b, |x| x.max(b))))
            })
        })
    };

    // Draw non-expanded groups first, then expanded boxes on top.
    for (idx, p) in placed_groups.iter().enumerate() {
        if expanded_boxes.iter().any(|eb| eb.idx == idx) {
            continue;
        }
        if (matches!(p.group.key, AnchorKey::CurveJoint(..)) && p.rect.center().distance(p.base) > glyph * 0.9)
            || p.rect.center().distance(p.base) > glyph * 1.6
        {
            painter.line_segment([p.base, p.rect.center()], Stroke::new(0.8, tk.text_dim.gamma_multiply(0.6)));
        }
        if p.grouped {
            paint_group_badge(app, ss, painter, &tk, &p.group.constraint_ids, p.rect, scale, picked.as_deref(), hovered_collapsed_idx == Some(idx));
            collapsed_badges.push((p.group.key, p.rect));
            hit_rects.push(p.rect.expand(1.5));
        } else {
            if p.has_box {
                painter.rect_filled(p.rect, box_rounding, tk.panel);
                painter.rect_stroke(p.rect, box_rounding, Stroke::new(1.0, tk.dialog_border), egui::StrokeKind::Inside);
                hit_rects.push(p.rect.expand(1.5));
            }
            let inner_pad = if p.has_box { pad } else { 0.0 };
            for (k, cid) in p.group.constraint_ids.iter().enumerate() {
                let center = egui::pos2(
                    p.rect.left() + inner_pad + glyph * 0.5 + (k % p.cols) as f32 * (glyph + cell_gap),
                    p.rect.top() + inner_pad + glyph * 0.5 + (k / p.cols) as f32 * (glyph + cell_gap),
                );
                let rel = rel_for_cell(p.group.key, cid);
                let meta = GlyphMeta { rel, hovered_rel, picked_rel: picked_rel.as_ref(), flash_alpha: flash_for(cid, rel) };
                paint_glyph_cell(
                    app,
                    ss,
                    painter,
                    &tk,
                    cid,
                    center,
                    glyph,
                    scale,
                    picked.as_deref(),
                    hovered_at.as_ref(),
                    geom_lit_ids.contains(cid),
                    &meta,
                    &mut drawn,
                    &mut drawn_rel,
                );
                if !p.has_box {
                    hit_rects.push(egui::Rect::from_center_size(center, egui::vec2(glyph, glyph)).expand(1.5));
                }
            }
        }
    }
    for eb in &expanded_boxes {
        let Some(p) = placed_groups.get(eb.idx) else { continue };
        if eb.rect.center().distance(p.base) > glyph * 1.6 {
            painter.line_segment([p.base, eb.rect.center()], Stroke::new(0.9, tk.accent.gamma_multiply(0.6)));
        }
        painter.rect_filled(eb.rect, box_rounding, tk.panel);
        painter.rect_stroke(eb.rect, box_rounding, Stroke::new(1.0, tk.dialog_border), egui::StrokeKind::Inside);
        hit_rects.push(eb.rect.expand(1.5));
        for (k, cid) in p.group.constraint_ids.iter().enumerate() {
            let center = egui::pos2(
                eb.rect.left() + pad + glyph * 0.5 + (k % eb.cols) as f32 * (glyph + cell_gap),
                eb.rect.top() + pad + glyph * 0.5 + (k / eb.cols) as f32 * (glyph + cell_gap),
            );
            let rel = rel_for_cell(p.group.key, cid);
            let meta = GlyphMeta { rel, hovered_rel, picked_rel: picked_rel.as_ref(), flash_alpha: flash_for(cid, rel) };
            paint_glyph_cell(
                app,
                ss,
                painter,
                &tk,
                cid,
                center,
                glyph,
                scale,
                picked.as_deref(),
                hovered_at.as_ref(),
                geom_lit_ids.contains(cid),
                &meta,
                &mut drawn,
                &mut drawn_rel,
            );
        }
    }

    // Simulated coincident break-off glyph (selected curve + hovered shared endpoint).
    let mut special_tooltip: Option<(egui::Rect, String)> = None;
    let next_detach = resolve_detach_target(app, sid, &ss.sketch, &mut point_parent, hover, scale).and_then(|(curve_id, ci, pi, point_id)| {
        let base = proj.to_screen(ss.plane.to_world(ss.sketch.point(pi)?))?;
        let point_rect = egui::Rect::from_center_size(base, egui::vec2(6.0 * scale, 6.0 * scale));
        let (reused, prev_dir) = OVERLAY.with(|c| {
            let prev = c.borrow().detach_preview.clone();
            let same = prev.filter(|p| p.curve_id == curve_id && p.point_id == point_id);
            let reused = same.as_ref().filter(|p| p.point_rect.center().distance(base) < 2.0).map(|p| p.glyph_rect);
            let dir = same.map(|p| (p.glyph_rect.center() - base).normalized()).filter(|d| d.length_sq() > 0.1);
            (reused, dir)
        });
        let glyph_rect = reused.unwrap_or_else(|| scene.place_group(base, None, egui::vec2(glyph, glyph), glyph, None, prev_dir));
        scene.placed.push(glyph_rect);
        let lit = hover.is_some_and(|h| glyph_rect.expand(2.0).contains(h));
        paint_accent_icon_badge(painter, &tk, base, glyph_rect, scale, "c_coincident", lit, true);
        hit_rects.push(glyph_rect.expand(1.5));
        if lit {
            special_tooltip = Some((glyph_rect, format!("Coincident on {curve_id} (right-click to detach)")));
        }
        let nudge_2d = compute_detach_nudge(&ss.sketch, ci, pi, &ss.plane, proj);
        Some(DetachPreview { curve_id, point_id, point_rect, glyph_rect, nudge_2d })
    });

    // Simulated per-point symmetry break-off glyphs (one per Symmetric constraint on the selected point).
    let mut next_sym_breaks: Vec<SymmetryBreakPreview> = Vec::new();
    if let Some((pi, point_id, syms)) = selected_symmetric_point(app, &ss.sketch)
        && let Some(base) = ss.sketch.point(pi).and_then(|p| proj.to_screen(ss.plane.to_world(p)))
    {
        for (sym_id, axis_curve, other_point) in syms {
            let prev_dir = OVERLAY.with(|c| {
                c.borrow()
                    .sym_break_previews
                    .iter()
                    .find(|p| p.point_id == point_id && p.constraint_id == sym_id && p.prev_dir.length_sq() > 0.1)
                    .map(|p| p.prev_dir)
            });
            let glyph_rect = scene.place_group(base, None, egui::vec2(glyph, glyph), glyph, None, prev_dir);
            scene.placed.push(glyph_rect);
            next_sym_breaks.push(SymmetryBreakPreview {
                point_id: point_id.clone(),
                constraint_id: sym_id,
                axis_curve,
                other_point,
                glyph_rect,
                prev_dir: (glyph_rect.center() - base).normalized(),
            });
        }
        let hovered_sym_idx = hover.and_then(|h| {
            next_sym_breaks
                .iter()
                .enumerate()
                .filter(|(_, s)| s.glyph_rect.expand(2.0).contains(h))
                .min_by(|(_, a), (_, b)| a.glyph_rect.center().distance(h).total_cmp(&b.glyph_rect.center().distance(h)))
                .map(|(i, _)| i)
        });
        for (idx, s) in next_sym_breaks.iter().enumerate() {
            let lit = hovered_sym_idx == Some(idx);
            paint_accent_icon_badge(painter, &tk, base, s.glyph_rect, scale, "c_symmetry", lit, true);
            hit_rects.push(s.glyph_rect.expand(1.5));
            if lit && special_tooltip.is_none() {
                let axis_id = ss.sketch.curves.get(s.axis_curve).map_or("axis", |c| c.id.as_str());
                special_tooltip = Some((s.glyph_rect, format!("Symmetry ({}, axis {axis_id}) (right-click to break)", s.constraint_id)));
            }
        }
    }

    // Undo/Redo & delete flash ghosts (red fading for deleted, solid green for re-created Symmetric points).
    let rounding = tk.radius_sm.saturating_sub(1);
    for (ghost, alpha) in &history_ghosts {
        let Some(base) = proj.to_screen(ss.plane.to_world(ghost.at)) else { continue };
        let dir_screen = ghost.dir.and_then(|d| proj.to_screen(ss.plane.to_world(ghost.at + d)));
        let r = scene.place_group(base, dir_screen, egui::vec2(glyph, glyph), glyph, None, None);
        scene.placed.push(r);
        let base_col = if ghost.is_added { tk.agent_ok } else { tk.error };
        let col = base_col.gamma_multiply(*alpha);
        if r.center().distance(base) > glyph * 1.3 {
            painter.line_segment([base, r.center()], Stroke::new(1.0, col.gamma_multiply(0.75)));
        }
        painter.rect_filled(r, rounding, base_col.gamma_multiply(0.24 * *alpha));
        painter.rect_stroke(r, rounding, Stroke::new(1.5, col), egui::StrokeKind::Inside);
        match ghost.pattern_icon.or_else(|| glyph_icon(ghost.kind_name)) {
            Some(icon) => crate::icons::paint_icon(painter, r.shrink(2.0 * scale), icon, f32::from(col.a()) / 255.0),
            None => {
                painter.text(
                    r.center(),
                    egui::Align2::CENTER_CENTER,
                    glyph_letter(ghost.kind_name),
                    egui::FontId::proportional(tk.font_panel * scale),
                    col,
                );
            }
        }
    }

    // Tooltip chip for hovered break-off glyph or individual constraint glyph (never on collapsed badges).
    let paint_chip = |h: Pos2, anchor_rect: egui::Rect, label: String| {
        let g = painter.layout(label, egui::FontId::proportional(tk.font_small), tk.text, glyph_label_wrap_width(proj.rect));
        let chip = glyph_label_rect(h, anchor_rect, g.size() + egui::vec2(6.0, 6.0), proj.rect);
        crate::scenario::publish_rect("sketch:glyph_label", chip);
        painter.rect_filled(chip, rounding, tk.panel);
        painter.with_clip_rect(chip.intersect(painter.clip_rect())).galley(chip.min + egui::vec2(3.0, 3.0), g, tk.text);
    };
    if let (Some((anchor_rect, label)), Some(h)) = (special_tooltip, hover) {
        paint_chip(h, anchor_rect, label);
    } else if let (Some((id, at_glyph)), Some(h)) = (hovered_at, hover)
        && let Some(c) = ss.sketch.constraints.iter().find(|c| c.id == id)
    {
        let label = if let Some(rel) = drawn_rel.iter().find(|r| r.id == id && r.center.distance(at_glyph) < 1.0) {
            let ids = rel.all_constraint_ids.join(", ");
            let redundant = !rel.all_constraint_ids.is_empty() && rel.all_constraint_ids.iter().all(|g| ss.report.redundant.contains(g));
            let prefix = if rel.multi { format!("{} group {}", rel.kind.label(), rel.group_index) } else { rel.kind.label().to_owned() };
            if redundant { format!("{prefix} ({ids}), redundant: already implied by other constraints") } else { format!("{prefix} ({ids})") }
        } else {
            let ids = glyph_group(&ss.sketch, &id).join(", ");
            if ss.report.redundant.contains(&id) {
                format!("{} ({ids}), redundant: already implied by other constraints", c.kind.name())
            } else {
                format!("{} ({ids})", c.kind.name())
            }
        };
        let anchor_rect = expanded_boxes
            .iter()
            .find(|eb| eb.rect.contains(at_glyph))
            .map(|eb| eb.rect)
            .or_else(|| placed_groups.iter().find(|p| p.has_box && !p.grouped && p.rect.contains(at_glyph)).map(|p| p.rect))
            .unwrap_or_else(|| egui::Rect::from_center_size(at_glyph, egui::vec2(glyph, glyph)));
        paint_chip(h, anchor_rect, label);
    }

    let mut next_vc_boxes = Vec::new();
    for &vpi in &vc_points {
        if vc_poc_revealed(vpi) {
            let root = find_point_cluster(&mut point_parent, vpi);
            for (idx, p) in placed_groups.iter().enumerate() {
                if p.group.key == AnchorKey::PointCluster(root) {
                    next_vc_boxes.push((vpi, expanded_boxes.iter().find(|eb| eb.idx == idx).map_or(p.rect, |eb| eb.rect)));
                }
            }
        }
    }

    OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        st.glyphs = drawn;
        st.drawn_rel = drawn_rel;
        st.collapsed_badges = collapsed_badges;
        st.hit_rects = hit_rects;
        st.detach_preview = next_detach;
        st.sym_break_previews = next_sym_breaks;
        st.vc_boxes = next_vc_boxes;
        st.prev_anchor_dirs = next_dirs;
    });
}
