//! Paint a weld's paper drawing independently of panels and editor state.
//!
//! Paper coordinates are millimetres, x right and y up, local to the reference-line elbow.
//! A viewport overlay host passes its own projection/billboard mapping and a Painter clipped
//! to the overlay. It need not construct a Ui. Text follows the mapping's local x direction;
//! pixels_per_mm controls stroke widths and text height independently of projected positions.
//!
//! This app-layer adapter uses geom::weld::Drawing. Shared viewport-egui/ui-kit cannot depend
//! on geom (L0), and geom cannot depend on egui; the overlay's app host calls this module.
use egui::{Color32, FontId, Painter, Pos2, Rect, Shape, Stroke, pos2};
use solidcore_record::Seg2;
use solidcraft_engine::geom::{
    Seg2Geom,
    paper::{Dashes, Item},
    weld,
};

/// A uniform paper-to-screen placement, centred in a target rectangle.
#[derive(Clone, Copy, Debug)]
pub struct Fit {
    pub pixels_per_mm: f32,
    /// Screen size of the drawing's ink at this scale.
    pub size: egui::Vec2,
    translation: egui::Vec2,
}

impl Fit {
    pub fn map(self, point: [f64; 2]) -> Pos2 {
        pos2(point[0] as f32, -point[1] as f32) * self.pixels_per_mm + self.translation
    }
}

/// Fit the drawing's painted ink (including text and stroke widths) into `rect`.
/// Returns None for empty or non-finite drawings/rectangles.
pub fn fit_into_rect(drawing: &weld::Drawing, rect: Rect, painter: &Painter) -> Option<Fit> {
    if !rect.is_finite() || !rect.is_positive() {
        return None;
    }
    let bounds = bounds(&shapes(painter, drawing, Color32::PLACEHOLDER));
    if !bounds.is_finite() || !bounds.is_positive() {
        return None;
    }
    let pixels_per_mm = (rect.width() / bounds.width()).min(rect.height() / bounds.height());
    centred(bounds, rect, pixels_per_mm)
}

/// Like `fit_into_rect` but at a fixed scale: the ink is centred in `rect` and may overflow it.
pub fn centre_in_rect(drawing: &weld::Drawing, rect: Rect, pixels_per_mm: f32, painter: &Painter) -> Option<Fit> {
    if !rect.is_finite() || !pixels_per_mm.is_finite() || pixels_per_mm <= 0.0 {
        return None;
    }
    let bounds = bounds(&shapes(painter, drawing, Color32::PLACEHOLDER));
    if !bounds.is_finite() || !bounds.is_positive() {
        return None;
    }
    centred(bounds, rect, pixels_per_mm)
}

fn centred(bounds: Rect, rect: Rect, pixels_per_mm: f32) -> Option<Fit> {
    Some(Fit { pixels_per_mm, size: bounds.size() * pixels_per_mm, translation: rect.center().to_vec2() - bounds.center().to_vec2() * pixels_per_mm })
}

fn bounds(shapes: &[Shape]) -> Rect {
    shapes.iter().fold(Rect::NOTHING, |bounds, shape| bounds.union(shape.visual_bounding_rect()))
}

/// Paint a locally generated `Weld::draw` result, returning its unclipped screen ink bounds.
/// `map` accepts x-right/y-up paper mm. Non-finite placements paint nothing. Opacity is
/// clamped to 0..=1; invalid opacity or a nonpositive/non-finite scale paints nothing.
/// Text stays legible at `pixels_per_mm`, with its baseline rotated by the local mapping.
pub fn paint(drawing: &weld::Drawing, map: impl Fn([f64; 2]) -> Pos2, pixels_per_mm: f32, ink: Color32, opacity: f32, painter: &Painter) -> Rect {
    if !pixels_per_mm.is_finite() || pixels_per_mm <= 0.0 || !opacity.is_finite() {
        return Rect::NOTHING;
    }
    let mut shapes = shapes(painter, drawing, ink.gamma_multiply(opacity.clamp(0.0, 1.0)));
    let place = |point: Pos2| map([f64::from(point.x), -f64::from(point.y)]);
    for shape in &mut shapes {
        match shape {
            Shape::LineSegment { points, stroke } => {
                *points = points.map(place);
                if points.iter().any(|point| !point.is_finite()) {
                    return Rect::NOTHING;
                }
                stroke.width *= pixels_per_mm;
            }
            Shape::Path(path) => {
                for point in &mut path.points {
                    *point = place(*point);
                    if !point.is_finite() {
                        return Rect::NOTHING;
                    }
                }
                path.stroke.width *= pixels_per_mm;
            }
            Shape::Text(text) => {
                // Glyphs keep a uniform screen height; the mapping owns baseline placement.
                let baseline = text.galley.rows.first().and_then(|r| r.glyphs.first().map(|g| r.pos.y + g.pos.y)).unwrap_or(0.0);
                let local = text.pos + egui::vec2(0.0, baseline);
                let at = place(local);
                let direction = place(local + egui::vec2(1.0, 0.0)) - at;
                if !at.is_finite() || !direction.is_finite() {
                    return Rect::NOTHING;
                }
                let angle = direction.angle();
                text.transform(egui::emath::TSTransform::from_scaling(pixels_per_mm));
                text.pos = at - egui::emath::Rot2::from_angle(angle) * egui::vec2(0.0, baseline * pixels_per_mm);
                text.angle = angle;
            }
            _ => {}
        }
    }
    let bounds = bounds(&shapes);
    if shapes.iter().any(|shape| !shape.visual_bounding_rect().is_finite()) {
        return Rect::NOTHING;
    }
    painter.extend(shapes);
    bounds
}

fn points(path: &[Seg2]) -> Vec<egui::Pos2> {
    // These are bounded, locally generated symbol curves, flattened for display only.
    let mut points: Vec<_> = path.iter().flat_map(|seg| seg.polyline(0.05)).map(|p| pos2(p.x as f32, -p.y as f32)).collect();
    points.dedup();
    if points.len() > 1 && points.first() == points.last() {
        points.pop();
    }
    points
}

/// A dashed polyline as separate segments on the screen canvas. The paper phase consumes the
/// pattern from its start (egui's own dash offset would add a gap instead). The pattern is
/// validated by `Weld::draw`: an even count of positive lengths. Anything else draws nothing.
fn dashed(points: &[egui::Pos2], stroke: Stroke, pattern: &Dashes) -> Vec<Shape> {
    const MAX_DASHES: usize = 100_000;
    let lengths: Vec<f32> = pattern.lengths_mm.iter().map(|length| *length as f32).collect();
    let phase = pattern.phase_mm as f32;
    if lengths.is_empty()
        || !lengths.len().is_multiple_of(2)
        || lengths.iter().any(|length| !length.is_finite() || *length <= 0.0)
        || !phase.is_finite()
        || phase < 0.0
        || points.iter().any(|point| !point.is_finite())
    {
        return Vec::new();
    }
    let mut phase = phase % lengths.iter().sum::<f32>();
    let mut index = 0;
    while phase >= lengths[index] {
        phase -= lengths[index];
        index = (index + 1) % lengths.len();
    }
    let mut remaining = lengths[index] - phase;
    let mut shapes = Vec::new();
    for pair in points.windows(2) {
        let vector = pair[1] - pair[0];
        let length = vector.length();
        if !length.is_finite() || length <= 0.0 {
            continue;
        }
        let mut along = 0.0;
        while along < length {
            let end = (along + remaining).min(length);
            if index.is_multiple_of(2) {
                if shapes.len() >= MAX_DASHES {
                    return shapes;
                }
                shapes.push(Shape::line_segment([pair[0] + vector * (along / length), pair[0] + vector * (end / length)], stroke));
            }
            remaining -= end - along;
            along = end;
            if remaining <= 0.0 {
                index = (index + 1) % lengths.len();
                remaining = lengths[index];
            }
        }
    }
    shapes
}

fn shapes(painter: &Painter, drawing: &weld::Drawing, color: Color32) -> Vec<Shape> {
    let mut shapes = Vec::new();
    for item in &drawing.items {
        match item {
            Item::Stroke { path, closed: is_loop, width_mm, pattern } => {
                let mut points = points(path);
                let stroke = Stroke::new(*width_mm as f32, color);
                if let Some(pattern) = pattern {
                    if *is_loop && let Some(first) = points.first().copied() {
                        points.push(first);
                    }
                    shapes.extend(dashed(&points, stroke, pattern));
                } else if *is_loop {
                    shapes.push(Shape::closed_line(points, stroke));
                } else {
                    shapes.push(Shape::line(points, stroke));
                }
            }
            // Weld::draw currently emits only a convex triangular field flag, without holes.
            Item::Fill { region } => shapes.push(Shape::convex_polygon(points(&region.outer.segs), color, Stroke::NONE)),
            Item::Text(_) | Item::Picture(_) => {}
        }
    }
    for label in &drawing.text {
        let galley = painter.layout_no_wrap(label.text.clone(), FontId::proportional(label.height_mm as f32), color);
        let baseline = galley.rows.first().and_then(|r| r.glyphs.first().map(|g| r.pos.y + g.pos.y)).unwrap_or(0.0);
        let at = pos2(label.baseline.x as f32, -label.baseline.y as f32 - baseline);
        shapes.push(Shape::galley(at, galley, color));
    }
    shapes
}

#[cfg(test)]
#[path = "weld_symbol_paint_tests.rs"]
mod tests;
