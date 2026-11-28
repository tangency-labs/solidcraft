//! Rendering pipeline for grouped sketch constraint glyphs, badges, and expanded boxes.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Weak};

use egui::{Pos2, Stroke};
use solidcore_record::{Plane, Vec3};
use solidcraft_engine::Sel;
use solidcraft_engine::doc::SolvedSketch;
use solidcraft_engine::geom::prelude::*;
use solidcraft_engine::sketch::{ConstraintKind, CurveKind};

use crate::SolveApp;
use crate::icons::markers::glyph_icon;
use crate::viewport::{Hit, Proj};

use super::actions::{active_sketch, detach_distance_mm_for_view, resolve_detach_targets, sketch_px_per_mm};
use super::grouping::{
    AnchorKey, ConstraintAnchorGroup, CurveRelGroup, CurveRelKind, anchor_groups_with_rel_mode, constraint_entities, find_point_cluster,
    glyph_constraints, glyph_group, glyph_group_visible_when_selected, point_cluster_map,
};
use super::layout::{PlacementScene, glyph_label_rect, glyph_label_wrap_width, square_grid_dims};
use super::{
    BADGE_HOVER_DELAY_SECS, CONSTRAINT_GROUP_NEVER, DetachItem, DetachPreview, DrawnRelInfo, GLYPH, MIN_GLYPH_SPAN_PX, OVERLAY, SketchGlyphCache,
    VC_HOVER_DELAY_SECS, ViewKey, constraint_glyphs_visible_at_scale, glyph_px, pattern_icon_for,
};

const MIN_VIEW_NORMAL_COS: f64 = 0.342_020_143_325_668_8;

fn visible_at_angle(plane: &Plane, cam: &solidcraft_viewport::Camera) -> bool {
    cam.facing_cos(plane.normal()) >= MIN_VIEW_NORMAL_COS - 1e-6
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
    vc_lit: bool,
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
        || hovered_at.is_some_and(|(hid, pos)| hid == id && (pos.x - center.x).abs() < 1.0 && (pos.y - center.y).abs() < 1.0)
        || meta.rel.is_some_and(|(rg, _, _)| meta.hovered_rel == Some((rg.kind, rg.index)))
        || vc_lit;
    let col = if failing {
        crate::viewport::Conflict::glyph_colour(app, tk)
    } else if lit {
        tk.accent
    } else if redundant {
        tk.glyph_text.gamma_multiply(0.45)
    } else {
        tk.glyph_text
    };
    let r = egui::Rect::from_center_size(center, egui::vec2(glyph, glyph));
    let bg = if lit { tk.accent_soft } else { tk.glyph_bg };
    let stroke = Stroke::new(1.0, if failing || lit { col } else { tk.glyph_edge });
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
    if let Some(icon) = pattern_icon_for(&ss.sketch, id).or_else(|| glyph_icon(c.kind.name())) {
        crate::icons::paint_icon(painter, icon_rect, icon, f32::from(col.a()) / 255.0);
    }
    if let Some((rg, _, curve_id)) = meta.rel {
        if rg.multi {
            painter.text(
                r.right_bottom() - egui::vec2(1.0 * scale, 0.5 * scale),
                egui::Align2::RIGHT_BOTTOM,
                rg.index.to_string(),
                egui::FontId::proportional(tk.font_triad * scale),
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

/// Per-frame constraint glyph layout and rendering entry point.
pub fn show(app: &SolveApp, painter: &egui::Painter, proj: &Proj, hover: Option<Pos2>, view: ViewKey) {
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

    if ss.sketch.view.hide_constraints || !visible_at_angle(&ss.plane, &proj.cam) {
        OVERLAY.with(|c| c.borrow_mut().clear_frame());
        return;
    }
    if let Some(px) = sketch_px_per_mm(&ss.plane, proj)
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
    let fast_drag = app.viewport.point_drag.is_some() || app.viewport.transform_drag.is_some();
    let world_weak = Arc::downgrade(&st);
    let sketch_cache = OVERLAY
        .with(|c| {
            c.borrow().sketch_cache.as_ref().filter(|sc| sc.sid == sid && sc.fast_drag == fast_drag && Weak::ptr_eq(&sc.world, &world_weak)).cloned()
        })
        .unwrap_or_else(|| {
            let (raw_groups, rel_groups) = anchor_groups_with_rel_mode(&ss.sketch, fast_drag);
            let point_parent = point_cluster_map(&ss.sketch);
            let vc_points = ss.sketch.virtual_corners().into_iter().map(|(pi, _)| pi).collect();
            let visible_polylines = (0..ss.sketch.curves.len())
                .filter(|&ci| solidcraft_engine::view::sketch_curve_visible(&ss.sketch, ci))
                .map(|ci| ss.sketch.polyline(ci))
                .collect();
            let built =
                Arc::new(SketchGlyphCache { world: world_weak, sid, fast_drag, raw_groups, rel_groups, point_parent, vc_points, visible_polylines });
            OVERLAY.with(|c| c.borrow_mut().sketch_cache = Some(Arc::clone(&built)));
            built
        });

    let curves: Vec<(egui::Rect, Vec<Pos2>)> = sketch_cache
        .visible_polylines
        .iter()
        .filter_map(|poly| {
            let pts: Vec<Pos2> = poly.iter().filter_map(|&p| proj.to_screen(ss.plane.to_world(p))).collect();
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
    let now = painter.ctx().input(|i| i.time);
    let expand_margin = 10.0 * scale;
    let mut point_parent = sketch_cache.point_parent.clone();

    // Virtual-corner hover/selection reveal (400 ms dwell on hover, immediate on left-click).
    let vc_points = &sketch_cache.vc_points;
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

    let (prev_badge_expanded, vc_hovered_active) = OVERLAY.with(|cell| {
        let mut st = cell.borrow_mut();
        let prev_exp = if let Some((exp_key, prev_rect)) = st.badge_expanded {
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
        let vc_act = st.vc_hover.update(raw_vc_hover, now, VC_HOVER_DELAY_SECS, painter.ctx());
        (prev_exp, vc_act)
    });

    let vc_poc_revealed = |p: usize| vc_hovered_active == Some(p) || revealed_vc_selected.contains(&p);
    let (mut vc_revealed_ids, mut hidden_vc_ids) = (HashSet::new(), HashSet::new());
    if !vc_points.is_empty() {
        for c in glyph_constraints(&ss.sketch) {
            if let ConstraintKind::PointOnCurve { p, c: curve } = c.kind
                && vc_points.contains(&p)
                && matches!(ss.sketch.curves.get(curve).map(|cv| &cv.kind), Some(CurveKind::Line { .. } | CurveKind::Arc { .. }))
            {
                if vc_poc_revealed(p) {
                    vc_revealed_ids.insert(c.id.clone());
                } else {
                    let forced = ss.report.failing.contains(&c.id)
                        || picked.as_deref() == Some(c.id.as_str())
                        || app.session.selection.iter().any(|s| matches!(s, Sel::SketchConstraint { id: x } if x == &c.id));
                    if !forced {
                        hidden_vc_ids.insert(c.id.clone());
                    }
                }
            }
        }
    }

    let mut raw_groups = sketch_cache.raw_groups.clone();
    let rel_groups = &sketch_cache.rel_groups;
    if !hidden_vc_ids.is_empty() {
        for g in &mut raw_groups {
            g.constraint_ids.retain(|id| !hidden_vc_ids.contains(id));
        }
        raw_groups.retain(|g| !g.constraint_ids.is_empty());
    }
    if app.ui.constraint_glyphs_selected_only {
        let is_failing = |cid: &String| ss.report.failing.contains(cid);
        for g in &mut raw_groups {
            let key = g.key;
            g.constraint_ids.retain(|id| {
                if glyph_group_visible_when_selected(&ss.sketch, id, &app.session.selection, picked.as_deref(), is_failing) {
                    return true;
                }
                if let AnchorKey::Curve(ci) = key
                    && let Some(rg) = rel_groups.iter().find(|rg| rg.curves.contains(&ci) && rg.constraint_ids.iter().any(|x| x == id))
                {
                    let curve_id = ss.sketch.curves.get(ci).map(|c| c.id.as_str());
                    if let (Some(cid), Some((rid, kind, pcid, _))) = (curve_id, picked_rel.as_ref())
                        && rid == id
                        && *kind == rg.kind
                        && pcid == cid
                    {
                        return true;
                    }
                    return rg.constraint_ids.iter().any(|cid| {
                        let applies_here = rg.kind == CurveRelKind::Symmetry
                            || ss.sketch.constraints.iter().find(|c| &c.id == cid).is_some_and(|c| constraint_entities(&c.kind).0.contains(&ci));
                        applies_here && glyph_group_visible_when_selected(&ss.sketch, cid, &app.session.selection, picked.as_deref(), is_failing)
                    });
                }
                false
            });
        }
        raw_groups.retain(|g| !g.constraint_ids.is_empty());
    }

    let prev_dirs = OVERLAY.with(|c| c.borrow().prev_anchor_dirs.clone());
    let mut next_dirs: HashMap<AnchorKey, egui::Vec2> = HashMap::new();
    let mut placed_groups: Vec<PlacedGroup> = Vec::new();
    for group in raw_groups {
        if group.constraint_ids.is_empty() {
            continue;
        }
        let Some(base) = proj.to_screen(ss.plane.to_world(group.at)) else { continue };
        let dir_screen = group.dir.and_then(|d| proj.to_screen(ss.plane.to_world(group.at + d)));
        let grouped = !matches!(group.key, AnchorKey::CurveJoint(..))
            && threshold < CONSTRAINT_GROUP_NEVER
            && group.constraint_ids.len() > usize::from(threshold);
        let has_box = !grouped && group.constraint_ids.len() >= 2;
        let (cols, rows) = if grouped { (1, 1) } else { square_grid_dims(group.constraint_ids.len()) };
        let box_pad = if has_box { pad * 2.0 } else { 0.0 };
        let size = egui::vec2(
            cols as f32 * glyph + cols.saturating_sub(1) as f32 * cell_gap + box_pad,
            rows as f32 * glyph + rows.saturating_sub(1) as f32 * cell_gap + box_pad,
        );
        let rect = scene.place_group(base, dir_screen, size, glyph, None, prev_dirs.get(&group.key).copied());
        let d = (rect.center() - base).normalized();
        if d.length_sq() > 0.1 {
            next_dirs.insert(group.key, d);
        }
        scene.placed.push(rect);
        placed_groups.push(PlacedGroup { group, grouped, has_box, cols, base, rect });
    }

    // Resolve direct hover/click expansion of a collapsed badge.
    let badge_expanded_idx = if let Some((exp_key, _)) = prev_badge_expanded {
        let idx = placed_groups.iter().position(|p| p.grouped && p.group.key == exp_key);
        if idx.is_none() {
            OVERLAY.with(|c| c.borrow_mut().badge_expanded = None);
        }
        idx
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

    let expanded_indices: Vec<usize> = badge_expanded_idx.into_iter().collect();

    struct ExpandedBox {
        idx: usize,
        rect: egui::Rect,
        cols: usize,
        cell_px: f32,
        gap_px: f32,
        pad_px: f32,
    }
    let mut expanded_boxes: Vec<ExpandedBox> = Vec::new();
    for &idx in &expanded_indices {
        let Some(exp) = placed_groups.get(idx) else { continue };
        let (cols, rows) = square_grid_dims(exp.group.constraint_ids.len());
        let raw_w = cols as f32 * glyph + cols.saturating_sub(1) as f32 * cell_gap + pad * 2.0;
        let raw_h = rows as f32 * glyph + rows.saturating_sub(1) as f32 * cell_gap + pad * 2.0;
        let max_w = (proj.rect.width() - 8.0).max(16.0);
        let max_h = (proj.rect.height() - 8.0).max(16.0);
        let fit = (max_w / raw_w.max(1.0)).min(max_h / raw_h.max(1.0)).min(1.0);
        let cell_px = (glyph * fit).max(4.0);
        let gap_px = cell_gap * fit;
        let pad_px = pad * fit;
        let box_size = egui::vec2(
            cols as f32 * cell_px + cols.saturating_sub(1) as f32 * gap_px + pad_px * 2.0,
            rows as f32 * cell_px + rows.saturating_sub(1) as f32 * gap_px + pad_px * 2.0,
        );
        let hard_blockers: Vec<egui::Rect> = expanded_boxes.iter().map(|b| b.rect).collect();
        let neighbors: Vec<egui::Rect> =
            placed_groups.iter().enumerate().filter(|(i, _)| *i != idx && !expanded_indices.contains(i)).map(|(_, p)| p.rect).collect();
        let exp_rect = scene.place_expanded(exp.rect, exp.base, box_size, proj.rect, true, &hard_blockers, &neighbors, 4.0 * scale);
        OVERLAY.with(|c| {
            c.borrow_mut().badge_expanded = Some((exp.group.key, exp_rect));
        });
        expanded_boxes.push(ExpandedBox { idx, rect: exp_rect, cols, cell_px, gap_px, pad_px });
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
        let (rect, cols, inner_pad, c_px, g_px) = match expanded_boxes.iter().find(|b| b.idx == idx) {
            Some(eb) => (eb.rect, eb.cols, eb.pad_px, eb.cell_px, eb.gap_px),
            None if !p.grouped => (p.rect, p.cols, if p.has_box { pad } else { 0.0 }, glyph, cell_gap),
            None => continue,
        };
        for (k, cid) in p.group.constraint_ids.iter().enumerate() {
            let c = egui::pos2(
                rect.left() + inner_pad + c_px * 0.5 + (k % cols) as f32 * (c_px + g_px),
                rect.top() + inner_pad + c_px * 0.5 + (k / cols) as f32 * (c_px + g_px),
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
    if let Some((rg, _, _)) = hovered_vg.and_then(|(_, _, r)| *r) {
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
                let meta = GlyphMeta { rel, hovered_rel, picked_rel: picked_rel.as_ref() };
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
                    vc_revealed_ids.contains(cid),
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
                eb.rect.left() + eb.pad_px + eb.cell_px * 0.5 + (k % eb.cols) as f32 * (eb.cell_px + eb.gap_px),
                eb.rect.top() + eb.pad_px + eb.cell_px * 0.5 + (k / eb.cols) as f32 * (eb.cell_px + eb.gap_px),
            );
            let rel = rel_for_cell(p.group.key, cid);
            let meta = GlyphMeta { rel, hovered_rel, picked_rel: picked_rel.as_ref() };
            paint_glyph_cell(
                app,
                ss,
                painter,
                &tk,
                cid,
                center,
                eb.cell_px,
                eb.cell_px / GLYPH,
                picked.as_deref(),
                hovered_at.as_ref(),
                vc_revealed_ids.contains(cid),
                &meta,
                &mut drawn,
                &mut drawn_rel,
            );
        }
    }

    // Simulated coincident break-off glyph(s): single glyph when a selected curve touches the
    // hovered shared point, or a constraint-style group box for all connected non-linked curves
    // after hovering the point.
    let mut special_tooltip: Option<(egui::Rect, String)> = None;
    let next_detach = resolve_detach_targets(app, sid, &ss.sketch, &mut point_parent, hover, scale, now, painter.ctx()).and_then(|resolved| {
        let base = proj.to_screen(ss.plane.to_world(ss.sketch.point(resolved.anchor_pi)?))?;
        let point_rect = egui::Rect::from_center_size(base, egui::vec2(6.0 * scale, 6.0 * scale));
        let has_box = resolved.targets.len() >= 2;
        let (cols, rows) = square_grid_dims(resolved.targets.len());
        let inner_pad = if has_box { pad } else { 0.0 };
        let box_size = egui::vec2(
            cols as f32 * glyph + cols.saturating_sub(1) as f32 * cell_gap + inner_pad * 2.0,
            rows as f32 * glyph + rows.saturating_sub(1) as f32 * cell_gap + inner_pad * 2.0,
        );
        let (reused, prev_dir) = OVERLAY.with(|c| {
            let prev = c.borrow().detach_preview.clone();
            let same = prev.filter(|p| {
                p.cluster_root == resolved.cluster_root
                    && p.selected_curve_id == resolved.selected_curve_id
                    && p.items.len() == resolved.targets.len()
            });
            let reused = same.as_ref().filter(|p| p.point_rect.center().distance(base) < 2.0).map(|p| p.box_rect);
            let dir = same.map(|p| (p.box_rect.center() - base).normalized()).filter(|d| d.length_sq() > 0.1);
            (reused, dir)
        });
        let box_rect = reused.unwrap_or_else(|| scene.place_group(base, None, box_size, glyph, None, prev_dir));
        scene.placed.push(box_rect);

        let item_rects: Vec<egui::Rect> = (0..resolved.targets.len())
            .map(|k| {
                let center = egui::pos2(
                    box_rect.left() + inner_pad + glyph * 0.5 + (k % cols) as f32 * (glyph + cell_gap),
                    box_rect.top() + inner_pad + glyph * 0.5 + (k / cols) as f32 * (glyph + cell_gap),
                );
                egui::Rect::from_center_size(center, egui::vec2(glyph, glyph))
            })
            .collect();

        let lit_idx = hover.and_then(|h| {
            item_rects
                .iter()
                .enumerate()
                .filter(|(_, r)| r.expand(2.0).contains(h))
                .min_by(|(_, a), (_, b)| a.center().distance(h).total_cmp(&b.center().distance(h)))
                .map(|(i, _)| i)
        });

        // Highlight the target curve and shared point under the detach box when hovering an option.
        if let Some(i) = lit_idx
            && let Some(target) = resolved.targets.get(i)
        {
            let mut budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
            crate::tools::paint_curve(
                &ss.sketch,
                target.curve_idx,
                &ss.plane,
                painter,
                proj,
                Stroke::new(3.0, tk.accent.gamma_multiply(0.7)),
                &mut budget,
            );
            painter.circle_filled(base, 4.5, tk.accent);
        } else if has_box && hover.is_some_and(|h| box_rect.contains(h)) {
            painter.circle_filled(base, 4.5, tk.accent);
        }

        painter.line_segment([base, box_rect.center()], Stroke::new(1.0, tk.accent.gamma_multiply(0.8)));
        if has_box {
            painter.rect_filled(box_rect, box_rounding, tk.panel);
            painter.rect_stroke(box_rect, box_rounding, Stroke::new(1.0, tk.dialog_border), egui::StrokeKind::Inside);
            hit_rects.push(box_rect.expand(1.5));
        }

        let badge_rounding = tk.radius_sm.saturating_sub(1);
        let mut items = Vec::with_capacity(resolved.targets.len());
        for (k, (target, glyph_rect)) in resolved.targets.iter().zip(item_rects).enumerate() {
            let lit = lit_idx == Some(k);
            painter.rect_filled(glyph_rect, badge_rounding, if lit { tk.accent_soft } else { tk.glyph_bg });
            painter.rect_stroke(glyph_rect, badge_rounding, Stroke::new(1.2, tk.accent), egui::StrokeKind::Inside);
            crate::icons::paint_icon(painter, glyph_rect.shrink(2.0 * scale), "c_coincident", 1.0);
            if !has_box {
                hit_rects.push(glyph_rect.expand(1.5));
            }
            if lit {
                special_tooltip = Some((glyph_rect, format!("Coincident on {} (right-click to detach)", target.curve_id)));
            }
            items.push(DetachItem { curve_id: target.curve_id.clone(), point_id: target.point_id.clone(), glyph_rect });
        }

        let distance_mm = detach_distance_mm_for_view(&ss.plane, proj);
        Some(DetachPreview {
            cluster_root: resolved.cluster_root,
            selected_curve_id: resolved.selected_curve_id,
            point_rect,
            box_rect,
            items,
            distance_mm,
        })
    });

    // Tooltip chip for hovered break-off glyph or individual constraint glyph (never on collapsed badges).
    let rounding = tk.radius_sm.saturating_sub(1);
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
    for &vpi in vc_points {
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
        st.vc_boxes = next_vc_boxes;
        if !crate::dim_view::editing() {
            st.prev_anchor_dirs = next_dirs;
        }
        st.published_view = Some(view);
    });
}
