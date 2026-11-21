//! Navigation drawing definitions. Camera projection and picking remain in the UI widgets.
use egui::{Pos2, Shape, Stroke, vec2};
use solidcraft_theme::Tokens;
pub(crate) fn arrow(painter: &egui::Painter, at: Pos2, direction: egui::Vec2, hover: bool, t: Tokens) {
    let side = vec2(-direction.y, direction.x);
    painter.add(Shape::convex_polygon(
        vec![at + direction * 7.0, at - direction * 5.0 + side * 7.0, at - direction * 5.0 - side * 7.0],
        if hover { t.accent_soft } else { t.navigator_face },
        Stroke::new(1.0, if hover { t.accent } else { t.navigator_border }),
    ));
}
pub(crate) fn curved_arrow(painter: &egui::Painter, at: Pos2, clockwise: bool, hover: bool, t: Tokens, scale: f32) {
    let (start, end) =
        if clockwise { (-std::f32::consts::FRAC_PI_4 + 0.12, 0.0) } else { (-std::f32::consts::FRAC_PI_4 - 0.12, -std::f32::consts::FRAC_PI_2) };
    arc_arrow(painter, at, 32.0 * scale, (start, end), clockwise, hover, t, scale);
}

/// An arrow along the circle about `centre` of `radius`, from angle `span.0` to `span.1` (radians,
/// screen y down), its head at the end; `clockwise` says which way the head points along the circle.
#[allow(clippy::too_many_arguments)]
pub(crate) fn arc_arrow(painter: &egui::Painter, centre: Pos2, radius: f32, span: (f32, f32), clockwise: bool, hover: bool, t: Tokens, scale: f32) {
    let (start, end) = span;
    let points: Vec<_> = (0..=16)
        .map(|i| {
            let angle = egui::lerp(start..=end, f32::from(i as u8) / 16.0);
            centre + vec2(angle.cos(), angle.sin()) * radius
        })
        .collect();
    painter.add(Shape::line(points.clone(), Stroke::new(5.0 * scale, if hover { t.accent } else { t.navigator_border })));
    painter.add(Shape::line(points.clone(), Stroke::new(3.0 * scale, if hover { t.accent_soft } else { t.navigator_face })));
    if let Some(tip) = points.last() {
        let sign = if clockwise { 1.0 } else { -1.0 };
        arrow(painter, *tip, vec2(-end.sin(), end.cos()) * (sign * scale), hover, t);
    }
}
