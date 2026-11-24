//! Technical weld sections, in the specification's 176 × 120 drawing coordinates.
//! These are illustrations only: no model geometry or interaction lives here.

use egui::{Color32, FontId, Painter, Pos2, Sense, Shape, Stroke, TextFormat, Vec2, pos2, vec2};
use solidcraft_theme::Tokens;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum SizeFigure {
    Fillet,
    Groove,
    Spot,
    Plug,
    Stud,
    Overlay,
}

pub(crate) fn paint_size_figure(ui: &mut egui::Ui, tokens: &Tokens, figure: SizeFigure, advanced: bool, width: f32) -> egui::Response {
    // The caller supplies layout points; reject invalid sizes before allocating or painting.
    let width = if width.is_finite() { width.max(0.0) } else { 0.0 };
    let (rect, response) = ui.allocate_exact_size(vec2(width, width * 120.0 / 176.0), Sense::hover());
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Image, ui.is_enabled(), "Size sketch"));
    if width == 0.0 || !ui.is_rect_visible(rect) {
        return response;
    }
    let drawing = Drawing { painter: ui.painter(), origin: rect.min, scale: width / 176.0, tokens };
    match figure {
        SizeFigure::Fillet => drawing.fillet(advanced),
        SizeFigure::Groove => drawing.groove(),
        SizeFigure::Spot => drawing.spot(),
        SizeFigure::Plug => drawing.plug(),
        SizeFigure::Stud => drawing.stud(),
        SizeFigure::Overlay => drawing.overlay(),
    }
    response
}

struct Drawing<'a> {
    painter: &'a Painter,
    origin: Pos2,
    scale: f32,
    tokens: &'a Tokens,
}

impl Drawing<'_> {
    fn point(&self, p: Pos2) -> Pos2 {
        self.origin + p.to_vec2() * self.scale
    }

    fn stroke(&self, width: f32, color: Color32) -> Stroke {
        let pixels_per_point = self.painter.ctx().pixels_per_point();
        Stroke::new((width * self.scale * pixels_per_point).round().max(1.0) / pixels_per_point, color)
    }

    fn line(&self, a: Pos2, b: Pos2, width: f32, color: Color32) {
        self.painter.line_segment([self.point(a), self.point(b)], self.stroke(width, color));
    }

    fn extension(&self, a: Pos2, b: Pos2) {
        self.line(a, b, 0.7, self.tokens.text_dim);
    }

    fn outline(&self, polygon: &[Pos2]) {
        let stroke = self.stroke(1.3, self.tokens.icon);
        // Separate edges and disks produce round joins, including the T-joint's reentrant corners.
        for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
            self.painter.line_segment([self.point(*a), self.point(*b)], stroke);
            self.painter.circle_filled(self.point(*a), stroke.width * 0.5, stroke.color);
        }
    }

    fn plate(&self, polygon: &[Pos2]) {
        for [a, b] in hatch_segments(polygon) {
            self.line(a, b, 0.7, self.tokens.text_dim.gamma_multiply(0.75));
        }
        self.outline(polygon);
    }

    /// All callers here are convex; the wavy overlay uses a strip mesh instead.
    fn weld(&self, polygon: &[Pos2]) {
        self.painter.add(Shape::convex_polygon(polygon.iter().map(|p| self.point(*p)).collect(), self.tokens.accent, Stroke::NONE));
        self.outline(polygon);
    }

    fn arrow(&self, tip: Pos2, direction: Vec2) {
        let base = tip - direction * 5.2;
        let side = vec2(-direction.y, direction.x) * 1.7;
        self.painter.add(Shape::convex_polygon([tip, base + side, base - side].map(|p| self.point(p)).to_vec(), self.tokens.text, Stroke::NONE));
    }

    fn dimension(&self, a: Pos2, b: Pos2, outside: bool) {
        let direction = (b - a).normalized();
        if outside {
            self.arrow(a, direction);
            self.arrow(b, -direction);
            self.line(a - direction * 12.0, a, 0.8, self.tokens.text_dim);
            self.line(b, b + direction * 12.0, 0.8, self.tokens.text_dim);
        } else {
            self.arrow(a, -direction);
            self.arrow(b, direction);
            self.line(a, b, 0.8, self.tokens.text_dim);
        }
    }

    /// `anchor` is SVG text-anchor (0 = start, 0.5 = middle, 1 = end); y is the baseline.
    fn text(&self, at: Pos2, text: &str, anchor: f32) {
        let font_size = 10.5 * self.scale;
        let mut job = egui::text::LayoutJob::default();
        job.append(text, 0.0, TextFormat { font_id: FontId::monospace(font_size), color: self.tokens.text, italics: true, ..Default::default() });
        let galley = self.painter.layout_job(job);
        let baseline = galley.rows.first().and_then(|row| row.glyphs.first().map(|glyph| row.pos.y + glyph.pos.y)).unwrap_or(galley.size().y);
        let origin = self.point(at) - vec2(galley.size().x * anchor, baseline);
        self.painter.galley(origin, galley, self.tokens.text);
    }

    fn fillet(&self, advanced: bool) {
        self.plate(&[
            pos2(6., 86.),
            pos2(62., 86.),
            pos2(62., 22.),
            pos2(78., 22.),
            pos2(78., 86.),
            pos2(170., 86.),
            pos2(170., 104.),
            pos2(6., 104.),
        ]);
        self.weld(&[pos2(78., 86.), pos2(78., 56.), pos2(108., 86.)]);
        self.extension(pos2(108., 86.), pos2(108., 40.));
        self.dimension(pos2(78., 48.), pos2(108., 48.), false);
        self.text(pos2(93., 42.), "z", 0.5);
        self.dimension(pos2(78., 86.), pos2(93., 71.), false);
        self.text(pos2(92., 84.), "a", 0.5);
        if advanced {
            self.weld(&[pos2(62., 86.), pos2(62., 56.), pos2(32., 86.)]);
            self.extension(pos2(32., 86.), pos2(32., 40.));
            self.dimension(pos2(32., 48.), pos2(62., 48.), false);
            self.text(pos2(47., 42.), "z1", 0.5);
        }
    }

    fn groove(&self) {
        self.plate(&[pos2(24., 52.), pos2(60., 52.), pos2(68., 92.), pos2(24., 92.)]);
        self.plate(&[pos2(170., 52.), pos2(108., 52.), pos2(100., 92.), pos2(170., 92.)]);
        self.weld(&[pos2(60., 52.), pos2(108., 52.), pos2(100., 92.), pos2(68., 92.)]);
        let mut cap = vec![pos2(60., 52.)];
        quadratic(&mut cap, pos2(60., 52.), pos2(84., 32.), pos2(108., 52.));
        self.weld(&cap);
        self.extension(pos2(84., 42.), pos2(128., 42.));
        self.extension(pos2(108., 52.), pos2(128., 52.));
        self.dimension(pos2(124., 42.), pos2(124., 52.), true);
        self.text(pos2(131., 33.), "h", 0.);
        self.extension(pos2(24., 52.), pos2(6., 52.));
        self.extension(pos2(24., 92.), pos2(6., 92.));
        self.dimension(pos2(11., 52.), pos2(11., 92.), false);
        self.text(pos2(11., 45.), "s", 0.5);
    }

    fn spot(&self) {
        self.plate(&[pos2(6., 46.), pos2(112., 46.), pos2(112., 60.), pos2(6., 60.)]);
        self.plate(&[pos2(58., 60.), pos2(170., 60.), pos2(170., 74.), pos2(58., 74.)]);
        let ellipse: Vec<_> = (0..96)
            .map(|i| {
                let angle = std::f32::consts::TAU * i as f32 / 96.0;
                pos2(85. + 17. * angle.cos(), 60. + 11. * angle.sin())
            })
            .collect();
        self.weld(&ellipse);
        self.extension(pos2(68., 62.), pos2(68., 98.));
        self.extension(pos2(102., 62.), pos2(102., 98.));
        self.dimension(pos2(68., 92.), pos2(102., 92.), false);
        self.text(pos2(85., 108.), "d", 0.5);
    }

    fn plug(&self) {
        self.plate(&[pos2(6., 44.), pos2(40., 44.), pos2(40., 64.), pos2(6., 64.)]);
        self.plate(&[pos2(86., 44.), pos2(120., 44.), pos2(120., 64.), pos2(86., 64.)]);
        self.plate(&[pos2(6., 64.), pos2(120., 64.), pos2(120., 92.), pos2(6., 92.)]);
        self.weld(&[pos2(40., 44.), pos2(86., 44.), pos2(86., 64.), pos2(40., 64.)]);
        self.extension(pos2(120., 44.), pos2(146., 44.));
        self.extension(pos2(120., 64.), pos2(146., 64.));
        self.dimension(pos2(141., 44.), pos2(141., 64.), false);
        self.text(pos2(147., 57.), "fill", 0.);
    }

    fn stud(&self) {
        self.plate(&[
            pos2(6., 88.),
            pos2(70., 88.),
            pos2(70., 28.),
            pos2(90., 28.),
            pos2(90., 88.),
            pos2(170., 88.),
            pos2(170., 106.),
            pos2(6., 106.),
        ]);
        self.weld(&[pos2(90., 88.), pos2(90., 66.), pos2(112., 88.)]);
        self.weld(&[pos2(70., 88.), pos2(70., 66.), pos2(48., 88.)]);
        self.extension(pos2(48., 88.), pos2(48., 54.));
        self.dimension(pos2(48., 58.), pos2(70., 58.), false);
        self.text(pos2(66., 49.), "collar", 1.);
    }

    fn overlay(&self) {
        self.plate(&[pos2(6., 78.), pos2(150., 78.), pos2(150., 102.), pos2(6., 102.)]);
        let mut wave = vec![pos2(6., 62.)];
        quadratic(&mut wave, pos2(6., 62.), pos2(28., 54.), pos2(50., 62.));
        // SVG T reflects the preceding control point around the preceding end point.
        quadratic(&mut wave, pos2(50., 62.), pos2(72., 70.), pos2(94., 62.));
        quadratic(&mut wave, pos2(94., 62.), pos2(116., 54.), pos2(138., 62.));
        wave.push(pos2(150., 62.));
        // The wave is concave: triangulate strips, rather than asking egui for a convex fill.
        let mut mesh = egui::Mesh::default();
        for p in &wave {
            mesh.colored_vertex(self.point(*p), self.tokens.accent);
            mesh.colored_vertex(self.point(pos2(p.x, 78.)), self.tokens.accent);
        }
        for i in 0..wave.len().saturating_sub(1) {
            let a = (i * 2) as u32;
            mesh.add_triangle(a, a + 2, a + 1);
            mesh.add_triangle(a + 1, a + 2, a + 3);
        }
        self.painter.add(mesh);
        wave.extend([pos2(150., 78.), pos2(6., 78.)]);
        self.outline(&wave);
        self.extension(pos2(150., 62.), pos2(166., 62.));
        self.extension(pos2(150., 78.), pos2(166., 78.));
        self.dimension(pos2(162., 62.), pos2(162., 78.), false);
        self.text(pos2(166., 56.), "t", 0.);
    }
}

/// Flatten a quadratic at 32 steps: subpixel chord error even at twice the design size.
fn quadratic(points: &mut Vec<Pos2>, start: Pos2, control: Pos2, end: Pos2) {
    for i in 1..=32 {
        let t = i as f32 / 32.0;
        let u = 1.0 - t;
        points.push((start.to_vec2() * (u * u) + control.to_vec2() * (2.0 * u * t) + end.to_vec2() * (t * t)).to_pos2());
    }
}

/// Even/odd scan conversion of x+y=c lines. Half-open edge crossings handle vertices once;
/// pairing sorted intersections also clips concave plates, without bridging their notches.
fn hatch_segments(polygon: &[Pos2]) -> Vec<[Pos2; 2]> {
    if polygon.len() < 3 || polygon.iter().any(|p| !p.is_finite()) {
        return Vec::new();
    }
    let spacing = 4.5 * std::f32::consts::SQRT_2;
    let min = polygon.iter().map(|p| p.x + p.y).fold(f32::INFINITY, f32::min);
    let max = polygon.iter().map(|p| p.x + p.y).fold(f32::NEG_INFINITY, f32::max);
    let first = (min / spacing).ceil() as i32;
    let last = (max / spacing).floor() as i32;
    let mut result = Vec::new();
    for index in first..=last {
        let c = index as f32 * spacing;
        let mut crossings = Vec::new();
        for (a, b) in polygon.iter().zip(polygon.iter().cycle().skip(1)).take(polygon.len()) {
            let av = a.x + a.y;
            let bv = b.x + b.y;
            if (av <= c && c < bv) || (bv <= c && c < av) {
                let t = (c - av) / (bv - av);
                crossings.push(a.x + t * (b.x - a.x));
            }
        }
        crossings.sort_by(f32::total_cmp);
        for [a, b] in crossings.as_chunks::<2>().0 {
            result.push([pos2(*a, c - a), pos2(*b, c - b)]);
        }
    }
    result
}

#[cfg(test)]
#[path = "weld_figures_tests.rs"]
mod tests;
