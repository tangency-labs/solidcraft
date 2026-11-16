//! Original agent pointer and label. Animation stays with the UI controller.
use egui::{Color32, FontId, Pos2, Shape, Stroke, vec2};
/// The agent pointer's colour (a theme token, so a colour vision mode can reach it).
pub fn agent() -> Color32 {
    solidcraft_theme::Tokens::get().icons.agent
}
/// [`Glyphs::pointer`](solidcraft_ui_kit::icons::glyphs::Glyphs::pointer): the agent pointer and its label, tip at `p`.
pub fn paint(painter: &egui::Painter, p: Pos2) {
    let ic = solidcraft_theme::Tokens::get().icons;
    // An arrow pointer, tip at `p`.
    let pts: Vec<Pos2> =
        [(0.0, 0.0), (0.0, 17.0), (4.5, 13.0), (7.5, 20.0), (10.5, 18.8), (7.5, 12.0), (13.0, 12.0)].iter().map(|(x, y)| p + vec2(*x, *y)).collect();
    painter.add(Shape::convex_polygon(pts.clone(), ic.agent, Stroke::NONE));
    painter.add(Shape::closed_line(pts, Stroke::new(1.5, ic.highlight)));
    let label = painter.layout_no_wrap("Agent".into(), FontId::proportional(solidcraft_theme::Tokens::get().font_small), ic.highlight);
    let r = egui::Rect::from_min_size(p + vec2(15.0, 18.0), label.size() + vec2(10.0, 4.0));
    painter.rect_filled(r, 4.0, ic.agent);
    painter.galley(r.min + vec2(5.0, 2.0), label, ic.highlight);
}
