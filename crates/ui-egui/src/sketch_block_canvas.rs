//! What a sketch block shows on the canvas while its sketch is edited: one glyph beside its
//! origin, and a light outline round its curves while the pointer is over one of them or over the
//! glyph. A click on the glyph selects the block's members, a right-click opens the block's menu
//! and a double-click opens Edit Block. The constraints a block holds are drawn faded, by
//! [`HELD_FADE`].

use std::cell::RefCell;
use std::collections::{HashMap, HashSet};

use egui::{Pos2, Stroke};
use solidcraft_engine::doc::SolvedSketch;
use solidcraft_engine::geom::prelude::*;

use crate::SolveApp;
use crate::viewport::{Hit, Proj};

/// The share of its usual colour a constraint glyph or dimension a block holds is drawn in.
pub(crate) const HELD_FADE: f32 = 0.45;

thread_local! {
    /// The block glyphs drawn last frame: the block's id and the glyph's centre.
    static DRAWN: RefCell<Vec<(String, Pos2)>> = const { RefCell::new(Vec::new()) };
}

/// The ids of the constraints the sketch's blocks hold, looked up once a frame.
pub(crate) fn held_set(sk: &solidcraft_engine::sketch::Sketch) -> HashSet<&str> {
    sk.blocks.iter().flat_map(|b| b.held.iter().map(String::as_str)).collect()
}

/// Whether a glyph draws faded: a block holds every constraint of its `group`.
pub(crate) fn held(held: &HashSet<&str>, group: &[String]) -> bool {
    !held.is_empty() && !group.is_empty() && group.iter().all(|id| held.contains(id.as_str()))
}

/// The block whose curve (a member or its axis) the pointer is over.
fn hovered_curve_block(app: &SolveApp, ss: &SolvedSketch) -> Option<String> {
    let Some(Hit::SketchCurve { sketch, id, .. }) = &app.viewport.hover else { return None };
    (*sketch == ss.feature).then(|| ss.sketch.block_of(id).map(|b| b.id.clone())).flatten()
}

/// Paint each block's glyph beside its origin, clear of the constraint glyphs at `taken`, and
/// the outline of the block under the pointer. `glyph` is the side of a glyph in pixels. Returns
/// the label of the glyph under the pointer and that glyph's centre.
pub(crate) fn paint(
    app: &SolveApp,
    ss: &SolvedSketch,
    painter: &egui::Painter,
    proj: &Proj,
    glyph: f32,
    hover: Option<Pos2>,
    taken: &[Pos2],
) -> Option<(String, Pos2)> {
    let sk = &ss.sketch;
    if sk.blocks.is_empty() {
        DRAWN.with(|d| d.borrow_mut().clear());
        return None;
    }
    let tk = solidcraft_theme::Tokens::get();
    let index: HashMap<&str, usize> = sk.curves.iter().enumerate().map(|(i, c)| (c.id.as_str(), i)).collect();
    let mut drawn: Vec<(String, Pos2)> = Vec::new();
    for block in &sk.blocks {
        let Some(origin) = index.get(block.axis.as_str()).and_then(|a| sk.polyline(*a).first().copied()) else { continue };
        let Some(base) = proj.to_screen(ss.plane.to_world(origin)) else { continue };
        let free = |p: Pos2| {
            crate::sketch_tools::nearest_glyph(taken.iter().copied().chain(drawn.iter().map(|(_, q)| *q)).map(|q| ((), q)), p, glyph + 1.0).is_none()
        };
        let start = base + egui::vec2(-0.7, 0.7) * (glyph * 1.1);
        let at = (0..8).map(|k| start + egui::vec2(-(glyph + 2.0), 0.0) * k as f32).find(|p| free(*p)).unwrap_or(start);
        drawn.push((block.id.clone(), at));
    }
    let on_glyph = hover.and_then(|h| crate::sketch_tools::nearest_glyph(drawn.iter().map(|(id, q)| (id.as_str(), *q)), h, glyph * 0.6));
    let tip = on_glyph.and_then(|(id, at)| sk.blocks.iter().find(|b| b.id == id).map(|b| (label(b), at)));
    let lit = on_glyph.map(|(id, _)| id.to_string()).or_else(|| hovered_curve_block(app, ss));
    if let Some(block) = lit.as_deref().and_then(|id| sk.blocks.iter().find(|b| b.id == id)) {
        let mut budget = solidcraft_engine::render::SamplingBudget::new(32_768, 65_536);
        let outline = Stroke::new(6.0, tk.sketch_block.gamma_multiply(0.3));
        for curve in block.curves.iter().chain(std::iter::once(&block.axis)).filter_map(|id| index.get(id.as_str())) {
            crate::tools::paint_curve(sk, *curve, &ss.plane, painter, proj, outline, &mut budget);
        }
    }
    for (id, at) in &drawn {
        let lit = lit.as_deref() == Some(id.as_str());
        let r = egui::Rect::from_center_size(*at, egui::vec2(glyph, glyph));
        painter.rect_filled(r, tk.radius_sm.saturating_sub(1), if lit { tk.accent_soft } else { tk.glyph_bg });
        painter.rect_stroke(
            r,
            tk.radius_sm.saturating_sub(1),
            Stroke::new(1.0, if lit { tk.sketch_block } else { tk.glyph_edge }),
            egui::StrokeKind::Inside,
        );
        crate::icons::paint_icon(painter, r.shrink(2.0 * glyph / 15.0), "c_block", 1.0);
        crate::scenario::publish_handle(&format!("sketch:block-glyph:{id}"), *at);
    }
    DRAWN.with(|d| *d.borrow_mut() = drawn);
    tip
}

/// A block glyph's label: the block's name, its scale and how many curves it holds, and that a
/// double-click edits it.
pub(crate) fn label(block: &solidcraft_engine::sketch::SketchBlock) -> String {
    let scale = solidcraft_ui_text::tr(match block.scale {
        solidcraft_engine::sketch::BlockScale::Fixed => "ui.block_glyph_scale_fixed",
        solidcraft_engine::sketch::BlockScale::Free => "ui.block_glyph_scale_free",
    });
    let count = block.curves.len();
    solidcraft_ui_text::format(
        if count == 1 { "ui.block_glyph_tip_one" } else { "ui.block_glyph_tip_other" },
        &[("name", &crate::sketch_block_tree::label(block)), ("scale", scale), ("count", &count.to_string())],
    )
}

/// Forget the glyphs, for a frame that draws none.
pub(crate) fn clear() {
    DRAWN.with(|d| d.borrow_mut().clear());
}

/// The block whose glyph is under a screen position (its id).
pub(crate) fn glyph_at(pos: Pos2) -> Option<String> {
    let reach = crate::sketch_tools::glyph_px_now() * 0.6;
    DRAWN.with(|d| crate::sketch_tools::nearest_glyph(d.borrow().iter().map(|(id, q)| (id.as_str(), *q)), pos, reach).map(|(id, _)| id.to_owned()))
}

/// Double-click on a block's glyph: Edit Block.
pub(crate) fn double_click(app: &mut SolveApp, pos: Pos2) -> bool {
    glyph_at(pos).is_some_and(|id| crate::sketch_modify_ui::edit_block(app, &id))
}
