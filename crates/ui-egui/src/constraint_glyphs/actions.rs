//! Constraint glyph hit-testing, badge click expansion, and Delete-key removal.

use egui::Pos2;
use serde_json::json;
use solidcore_record::{Plane, Vec2};
use solidcraft_engine::Sel;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::viewport::Proj;

use super::grouping::{CurveRelKind, glyph_deletion};
use super::{OVERLAY, ViewKey};

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

pub(crate) fn sketch_px_per_mm(plane: &Plane, proj: &Proj) -> Option<f32> {
    let a = proj.to_screen(plane.to_world(Vec2::ZERO))?;
    let b = proj.to_screen(plane.to_world(Vec2::new(1.0, 0.0)))?;
    Some(a.distance(b))
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
    let _ = app.run("sketch.delete", json!({ "entities": entities }));
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
