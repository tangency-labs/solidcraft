//! Deliberately composed preference illustrations. A shared orthographic projection keeps
//! faces, ground contact, rotation cues and axes consistent at their small display size.
use crate::theme::Tokens;
use egui::{Color32, Painter, Pos2, Rect, Shape, Stroke, Vec2, vec2};

struct Canvas {
    rect: Rect,
}
impl Canvas {
    fn at(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x * self.rect.width() / 120.0, y * self.rect.height() / 80.0)
    }
    fn width(&self, width: f32) -> f32 {
        width * (self.rect.width() / 120.0).min(self.rect.height() / 80.0)
    }
    fn project(&self, origin: (f32, f32), x: f32, y: f32, z: f32, scale: f32) -> Pos2 {
        self.at(origin.0 + (x - y) * 0.866_025_4 * scale, origin.1 + ((x + y) * 0.5 - z) * scale)
    }
}

fn face(p: &Painter, points: [Pos2; 4], fill: Color32) {
    p.add(Shape::convex_polygon(points.to_vec(), fill, Stroke::NONE));
}

/// Face order follows the actual orthographic camera: top, screen-left and screen-right.
fn solid(p: &Painter, c: &Canvas, centre: (f32, f32), size: f32, height: f32, scale: f32, t: &Tokens) -> [Pos2; 4] {
    let h = size * 0.5;
    let at = |x, y, z| c.project(centre, x, y, z, scale);
    let back = at(-h, -h, height);
    let left = at(-h, h, height);
    let front = at(h, h, height);
    let right = at(h, -h, height);
    let bottom_left = at(-h, h, 0.0);
    let bottom_front = at(h, h, 0.0);
    let bottom_right = at(h, -h, 0.0);
    face(p, [left, front, bottom_front, bottom_left], t.body.lerp_to_gamma(t.body_edge, 0.15));
    face(p, [front, right, bottom_right, bottom_front], t.body.lerp_to_gamma(t.body_edge, 0.30));
    face(p, [back, right, front, left], t.body);
    let edge = t.body_edge.lerp_to_gamma(t.form_dim, 0.35);
    let stroke = Stroke::new(c.width(1.2), edge);
    p.add(Shape::closed_line(vec![back, right, bottom_right, bottom_front, bottom_left, left], stroke));
    p.add(Shape::line(vec![left, front, right], stroke));
    p.line_segment([front, bottom_front], stroke);
    [at(-h, -h, 0.0), bottom_right, bottom_front, bottom_left]
}

fn arrow(p: &Painter, a: Pos2, b: Pos2, ink: Color32, width: f32, head: f32) {
    let tangent = (b - a).normalized();
    let normal = vec2(-tangent.y, tangent.x);
    let base = b - tangent * head;
    p.line_segment([a, base + tangent * width], Stroke::new(width, ink));
    p.add(Shape::convex_polygon(vec![b, base + normal * head * 0.44, base - normal * head * 0.44], ink, Stroke::NONE));
}

fn radial_shadow(p: &Painter, centre: Pos2, radius: Vec2, ink: Color32) {
    const SEGMENTS: u32 = 40;
    let mut mesh = egui::Mesh::default();
    mesh.colored_vertex(centre, ink);
    for i in 0..=SEGMENTS {
        let angle = std::f32::consts::TAU * i as f32 / SEGMENTS as f32;
        mesh.colored_vertex(centre + vec2(angle.cos() * radius.x, angle.sin() * radius.y), Color32::TRANSPARENT);
    }
    for i in 1..=SEGMENTS {
        mesh.add_triangle(0, i, i + 1);
    }
    p.add(Shape::mesh(mesh));
}

/// Orbit cards share a deliberately positioned model; only the pivot and its explanatory
/// cue move. The arc's arrowhead is tangent to the projected ground-plane ellipse.
pub(crate) fn paint_orbit(p: &Painter, rect: Rect, pivot: usize) {
    let t = Tokens::get();
    let c = Canvas { rect };
    let p = p.with_clip_rect(rect);
    let model_centre = (61.0, 53.0);
    radial_shadow(&p, c.at(61.0, 59.0), vec2(c.width(25.0), c.width(5.0)), t.body_edge.gamma_multiply(0.12));
    solid(&p, &c, model_centre, 24.0, 24.0, 0.9, &t);
    let (x, y) = match pivot {
        0 => (22.0, 60.0),
        2 => (88.0, 31.0),
        _ => (61.0, 42.0),
    };
    let centre = c.at(x, y);
    let orange = t.surface_ink;
    if pivot == 0 {
        let grey = Stroke::new(c.width(1.3), t.form_dim);
        p.line_segment([centre, c.at(x, y - 11.0)], grey);
        p.line_segment([centre, c.at(x + 11.0, y + 5.5)], grey);
        p.line_segment([centre, c.at(x - 11.0, y + 5.5)], grey);
    }
    if pivot == 1 {
        // A restrained screen-space crosshair makes "View centre" distinct from origin.
        let guide = Stroke::new(c.width(1.0), t.form_dim.gamma_multiply(0.7));
        for (a, b) in [((x - 9.0, y), (x - 5.0, y)), ((x + 5.0, y), (x + 9.0, y)), ((x, y - 9.0), (x, y - 5.0))] {
            p.line_segment([c.at(a.0, a.1), c.at(b.0, b.1)], guide);
        }
    }
    let mut arc = Vec::with_capacity(33);
    for i in 0..=32 {
        let angle = std::f32::consts::PI * (0.88 - 0.76 * i as f32 / 32.0);
        arc.push(c.at(x + 15.0 * angle.cos(), y + 5.0 + 8.5 * angle.sin()));
    }
    let end_angle = std::f32::consts::PI * 0.12;
    let end = c.at(x + 15.0 * end_angle.cos(), y + 5.0 + 8.5 * end_angle.sin());
    let direction = vec2(15.0 * end_angle.sin(), -8.5 * end_angle.cos()).normalized();
    p.add(Shape::line(arc, Stroke::new(c.width(1.8), orange)));
    arrow(&p, end - direction * c.width(5.0), end + direction * c.width(1.0), orange, c.width(1.8), c.width(5.5));
    p.circle_filled(centre, c.width(4.4), t.viewport_top);
    p.circle_filled(centre, c.width(3.0), orange);
    if pivot == 2 {
        // Seven straight vertices describe a cursor with a clean neck and substantial stem.
        let points = [(88.0, 35.0), (88.0, 49.0), (91.5, 45.8), (95.2, 53.0), (98.2, 51.5), (94.4, 44.5), (99.0, 44.5)].map(|(x, y)| c.at(x, y));
        let mut mesh = egui::Mesh::default();
        for point in points {
            mesh.colored_vertex(point, t.form_ink);
        }
        for [a, b, d] in [[0, 1, 2], [0, 2, 5], [0, 5, 6], [2, 3, 4], [2, 4, 5]] {
            mesh.add_triangle(a, b, d);
        }
        p.add(Shape::mesh(mesh));
    }
}

/// Orientation uses the same 30-degree ground plane as the model cards, with unmistakable
/// axis arrows and readable labels rather than an unrelated generic cube.
pub(crate) fn paint_orientation(p: &Painter, rect: Rect, y_up: bool) {
    let t = Tokens::get();
    let c = Canvas { rect };
    let p = p.with_clip_rect(rect);
    let centre = c.at(60.0, 48.0);
    let ground = [c.at(60.0, 32.0), c.at(97.0, 50.5), c.at(60.0, 69.0), c.at(23.0, 50.5)];
    face(&p, ground, t.form_dim.gamma_multiply(0.035));
    p.add(Shape::closed_line(ground.to_vec(), Stroke::new(c.width(0.8), t.form_dim.gamma_multiply(0.20))));
    let axes = if y_up {
        [(94.0, 65.0, 0, "X"), (60.0, 13.0, 1, "Y"), (26.0, 65.0, 2, "Z")]
    } else {
        [(94.0, 65.0, 0, "X"), (26.0, 65.0, 1, "Y"), (60.0, 13.0, 2, "Z")]
    };
    for (x, y, index, label) in axes {
        let colour = t.triad.get(index).copied().unwrap_or(t.form_ink);
        let end = c.at(x, y);
        // A halo keeps every shaft distinct from the subtle ground and the other axes.
        p.line_segment([centre, end], Stroke::new(c.width(4.2), t.viewport_top));
        arrow(&p, centre, end, colour, c.width(2.3), c.width(6.5));
        let text_pos = if y < 20.0 { c.at(x, y - 6.0) } else { c.at(x + if x < 60.0 { -7.0 } else { 7.0 }, y + 3.0) };
        let ink = if crate::theme::contrast_ratio(colour, t.viewport_top) >= 4.5 { colour } else { t.form_ink };
        p.text(text_pos, egui::Align2::CENTER_CENTER, label, crate::fonts::semibold(t.font_small), ink);
    }
    p.circle_filled(centre, c.width(2.7), t.form_ink);
    p.circle_filled(centre, c.width(1.2), t.viewport_top);
}

/// Contact shading is visible in a physically grounded model, with a broad penumbra and
/// a tight occlusion core following its footprint. The unshaded option retains a light
/// ground reference so the model never appears to float.
pub(crate) fn paint_contact_shading(p: &Painter, rect: Rect, on: bool) {
    let t = Tokens::get();
    let c = Canvas { rect };
    let p = p.with_clip_rect(rect);
    let centre = (61.0, 51.0);
    let ground = [c.at(61.0, 37.0), c.at(105.0, 59.0), c.at(61.0, 80.0), c.at(17.0, 59.0)];
    face(&p, ground, t.form_dim.gamma_multiply(0.035));
    if on {
        radial_shadow(&p, c.at(61.0, 60.0), vec2(c.width(35.0), c.width(12.5)), t.body_edge.gamma_multiply(0.24));
        radial_shadow(&p, c.at(61.0, 60.0), vec2(c.width(25.0), c.width(7.5)), t.body_edge.gamma_multiply(0.35));
    } else {
        radial_shadow(&p, c.at(61.0, 60.0), vec2(c.width(28.0), c.width(7.0)), t.body_edge.gamma_multiply(0.07));
    }
    let footprint = solid(&p, &c, centre, 28.0, 25.0, 0.9, &t);
    if on {
        // The front two contact edges align exactly with the projected solid, never an
        // arbitrary ellipse painted over the faces.
        let [_, right, front, left] = footprint;
        p.add(Shape::line(vec![left, front, right], Stroke::new(c.width(2.0), t.body_edge.gamma_multiply(0.38))));
    }
}

/// A single carefully composed shell preview demonstrates the four layout settings.
/// It is explanatory vector content: no second viewport, live scene, or nested host.
/// A restrained workspace schematic: panel placement remains primary, with a few
/// tool marks, tree rows and one simple solid providing recognisable CAD context.
pub(crate) fn paint_layout(ui: &mut egui::Ui, state: super::preferences::LayoutPreview) {
    let t = Tokens::get();
    let size = vec2(super::metrics::dimension(ui.available_width()), t.space_24 * 7.0 + t.space_6);
    let (outer, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    let p = ui.painter().with_clip_rect(outer);
    t.container_rect(&p, outer, t.radius_lg, t.control_fill, t.dialog_rule);
    let r = outer.shrink(t.space_8);
    let c = Canvas { rect: r };
    p.rect(r, t.control_radius, t.viewport_top, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
    let inset = if state.compact { 0.0 } else { 4.0 };
    let gap = if state.compact { 0.0 } else { 3.0 };
    let ribbon_height = if state.labels { 16.0 } else { 10.0 };
    let top = inset + ribbon_height + gap;
    let bottom = 80.0 - inset;
    let left_history = if state.history == 1 { 10.0 + gap } else { 0.0 };
    let bx = inset + left_history;
    let bw = 30.0;
    let browser_height = if state.history == 2 {
        (bottom - top - gap) * 0.5
    } else if state.history == 0 {
        bottom - top - 10.0 - gap
    } else {
        bottom - top
    };
    let panel = |x: f32, y: f32, w: f32, h: f32, label: &str, fill: Color32| {
        let rect = Rect::from_min_max(c.at(x, y), c.at(x + w, y + h));
        p.rect(rect, if state.compact { 0 } else { t.radius_sm }, fill, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
        if !label.is_empty() {
            let centre = if h > 20.0 { c.at(x + w * 0.5, y + 7.0) } else { rect.center() };
            p.text(centre, egui::Align2::CENTER_CENTER, crate::i18n::tr_label(label), egui::FontId::proportional(t.font_small), t.form_dim);
        }
        rect
    };
    panel(inset, inset, 120.0 - inset * 2.0, ribbon_height, "", t.toolbar);
    panel(bx, top, bw, browser_height, "Browser", if state.translucent && !state.compact { t.panel.gamma_multiply(0.60) } else { t.panel });
    // A full row of quiet tool symbols, with label marks only when labels are enabled.
    let quiet = t.form_dim.lerp_to_gamma(t.viewport_top, 0.55);
    for index in 0..10 {
        let x = inset + 7.0 + index as f32 * (106.0 - inset * 2.0) / 9.0;
        let y = inset + if state.labels { 5.5 } else { ribbon_height * 0.5 };
        p.circle_filled(c.at(x, y), c.width(1.8), quiet);
        if state.labels {
            p.line_segment([c.at(x - 3.0, y + 4.5), c.at(x + 3.0, y + 4.5)], Stroke::new(c.width(0.65), quiet));
        }
    }
    for index in 0..3 {
        let y = top + 15.0 + index as f32 * 5.0;
        if y < top + browser_height - 3.0 {
            let indent = if index == 1 { 2.5 } else { 0.0 };
            p.line_segment([c.at(bx + 5.0 + indent, y), c.at(bx + bw - 5.0, y)], Stroke::new(c.width(0.7), t.form_dim.gamma_multiply(0.4)));
        }
    }
    let viewport_left = bx + bw + gap;
    let viewport_bottom = if state.history == 0 { bottom - 10.0 - gap } else { bottom };
    let object_centre = c.at((viewport_left + 120.0 - inset) * 0.5, (top + viewport_bottom) * 0.5);
    crate::icons::glyphs::brand_mark_silhouette(&p, object_centre, c.width(22.0), quiet);
    p.circle_filled(c.at(110.0 - inset, top + 7.0), c.width(3.5), quiet);
    let history = match state.history {
        1 => panel(inset, top, 10.0, bottom - top, "", t.selected_bg),
        2 => panel(bx, top + browser_height + gap, bw, bottom - top - browser_height - gap, "History", t.selected_bg),
        _ => panel(inset, bottom - 10.0, 120.0 - inset * 2.0, 10.0, "History", t.selected_bg),
    };
    if state.history == 1 {
        // A single vertical mark reads as the narrow icon-only history rail.
        p.line_segment(
            [history.center_top() + vec2(0.0, t.space_8), history.center_bottom() - vec2(0.0, t.space_8)],
            Stroke::new(2.0, t.form_primary),
        );
    }
    crate::scenario::publish_rect("prefs:layout_preview", outer);
    super::text(ui, crate::i18n::tr("prefs.preview_choices"), super::Text::Note("preview"));
}
