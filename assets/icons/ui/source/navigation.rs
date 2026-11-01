//! Navigation drawing definitions. Camera projection and picking remain in the UI widgets.
use crate::theme::Tokens;
use egui::{Color32, FontId, Pos2, Rect, Shape, Stroke, pos2, vec2};
use solidcraft_engine::geom::{Ring, Vec3};
pub(crate) struct Facet {
    pub(crate) direction: Vec3,
    pub(crate) points: Ring<Pos2>,
    pub(crate) outer_edges: Vec<bool>,
    pub(crate) centre: Pos2,
    pub(crate) label: String,
    pub(crate) text_axes: Option<(egui::Vec2, egui::Vec2)>,
    pub(crate) depth: f64,
}
// Private marker for label meshes with atlas-pixel UVs. The output hook replaces
// it with the font texture before any renderer sees the frame.
pub(crate) const LABEL_PIXELS: egui::TextureId = egui::TextureId::User(u64::from_le_bytes(*b"cubetext"));

struct LabelAtlas;

impl egui::plugin::Plugin for LabelAtlas {
    fn debug_name(&self) -> &'static str {
        "navigation cube label atlas"
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        let size = ctx.fonts(|f| f.font_image_size());
        let normalize = vec2(1.0 / size[0].max(1) as f32, 1.0 / size[1].max(1) as f32);
        for shape in &mut output.shapes {
            if let Shape::Mesh(mesh) = &mut shape.shape
                && mesh.texture_id == LABEL_PIXELS
            {
                let mesh = std::sync::Arc::make_mut(mesh);
                mesh.texture_id = egui::TextureId::default();
                for v in &mut mesh.vertices {
                    v.uv = (v.uv.to_vec2() * normalize).to_pos2();
                }
            }
        }
    }
}

/// Warp the font mesh into the projected face, so labels turn with the model.
pub(crate) fn face_text(painter: &egui::Painter, face: &Facet, ink: Color32, font_size: f32) {
    let Some((r, down)) = face.text_axes else { return };
    let opacity = ((face.depth - 0.18) / 0.17).clamp(0.0, 1.0) as f32;
    if opacity <= 0.0 {
        return;
    }
    let ink = ink.gamma_multiply(opacity);
    let galley = painter.layout_no_wrap(face.label.clone(), FontId::proportional(font_size), ink);
    let centre = galley.rect.center();
    // Keep atlas-pixel UVs until the frame is complete: another label or widget
    // can grow the atlas after this mesh is created, invalidating early UVs.
    painter.ctx().add_plugin(LabelAtlas);
    let text = Shape::Text(egui::epaint::TextShape::new(Pos2::ZERO, galley, ink));
    let atlas = painter.ctx().fonts(|f| f.font_image_size());
    let primitives =
        painter.ctx().tessellate(vec![egui::epaint::ClippedShape { clip_rect: Rect::EVERYTHING, shape: text }], painter.ctx().pixels_per_point());
    for primitive in primitives {
        if let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive {
            mesh.texture_id = LABEL_PIXELS;
            for v in &mut mesh.vertices {
                v.uv = pos2(v.uv.x * atlas[0] as f32, v.uv.y * atlas[1] as f32);
                let local = v.pos - centre;
                v.pos = face.centre + r * local.x + down * (local.y * 1.35);
            }
            painter.add(Shape::mesh(mesh));
        }
    }
}

pub(crate) fn paint_cube(painter: &egui::Painter, faces: &[Facet], hovered: Option<usize>, t: Tokens, labels: bool, scale: f32) {
    // A shared, opaque triangle mesh has no antialiasing fringes between facets.
    // Per-polygon fringes and miter joins overlap as a face becomes edge-on.
    let mut fill_mesh = egui::epaint::Mesh::default();
    let mut edges: Vec<(Pos2, Pos2, usize, bool)> = Vec::new();
    let hovered_direction = hovered.and_then(|i| faces.get(i)).map(|f| f.direction);
    for face in faces {
        let base = t.cube_face;
        let shade = (0.72 + 0.28 * face.depth.min(1.0)) as f32;
        let highlighted = hovered_direction == Some(face.direction);
        let fill = if highlighted {
            t.cube_hover
        } else {
            Color32::from_rgb((f32::from(base.r()) * shade) as u8, (f32::from(base.g()) * shade) as u8, (f32::from(base.b()) * shade) as u8)
        };
        let centre = fill_mesh.vertices.len() as u32;
        fill_mesh.colored_vertex(face.centre, fill);
        for &p in &face.points {
            fill_mesh.colored_vertex(p, fill);
        }
        for (j, (&a, &b)) in face.points.edges().enumerate() {
            fill_mesh.add_triangle(centre, centre + 1 + j as u32, centre + 1 + ((j + 1) % face.points.len()) as u32);
            // Coplanar picking regions have no visible seams. Only actual cube
            // edges and the hovered target's boundary receive an outline.
            let outer = face.outer_edges.get(j).copied().unwrap_or(false);
            if !highlighted && !outer {
                continue;
            }
            if let Some(edge) = edges
                .iter_mut()
                .find(|(p, q, _, _)| (p.distance_sq(a) < 1e-6 && q.distance_sq(b) < 1e-6) || (p.distance_sq(b) < 1e-6 && q.distance_sq(a) < 1e-6))
            {
                edge.2 += usize::from(highlighted);
                edge.3 |= outer;
            } else {
                edges.push((a, b, usize::from(highlighted), outer));
            }
        }
    }
    painter.add(Shape::mesh(fill_mesh));
    for (a, b, highlights, outer) in edges {
        // Two highlighted coplanar patches share an internal subdivision, not
        // a target boundary. Actual cube edges remain visible across faces.
        if !outer && highlights != 1 {
            continue;
        }
        let highlighted = highlights > 0;
        painter.line_segment([a, b], Stroke::new(1.0, if highlighted { t.accent } else { t.cube_border }));
    }
    if labels {
        for face in faces {
            face_text(painter, face, t.cube_text, t.font_heading * scale);
        }
    }
}
pub(crate) fn arrow(painter: &egui::Painter, at: Pos2, direction: egui::Vec2, hover: bool, t: Tokens) {
    let side = vec2(-direction.y, direction.x);
    painter.add(Shape::convex_polygon(
        vec![at + direction * 7.0, at - direction * 5.0 + side * 7.0, at - direction * 5.0 - side * 7.0],
        if hover { t.accent_soft } else { t.cube_face },
        Stroke::new(1.0, if hover { t.accent } else { t.cube_border }),
    ));
}
pub(crate) fn curved_arrow(painter: &egui::Painter, at: Pos2, clockwise: bool, hover: bool, t: Tokens, scale: f32) {
    let (start, end) =
        if clockwise { (-std::f32::consts::FRAC_PI_4 + 0.12, 0.0) } else { (-std::f32::consts::FRAC_PI_4 - 0.12, -std::f32::consts::FRAC_PI_2) };
    let points: Vec<_> = (0..=16)
        .map(|i| {
            let angle = egui::lerp(start..=end, f32::from(i as u8) / 16.0);
            at + vec2(angle.cos(), angle.sin()) * (32.0 * scale)
        })
        .collect();
    painter.add(Shape::line(points.clone(), Stroke::new(5.0 * scale, if hover { t.accent } else { t.cube_border })));
    painter.add(Shape::line(points.clone(), Stroke::new(3.0 * scale, if hover { t.accent_soft } else { t.cube_face })));
    if let Some(tip) = points.last() {
        let sign = if clockwise { 1.0 } else { -1.0 };
        arrow(painter, *tip, vec2(-end.sin(), end.cos()) * (sign * scale), hover, t);
    }
}
/// The pointer target around a compass end: larger than the drawn disc, so it is easy to hit.
pub(crate) const AXIS_RADIUS: f32 = 15.0;
pub(crate) fn axis_tip(painter: &egui::Painter, c: Pos2, name: &str, d: &Vec3, at: Pos2, depth: f64, hovered: bool, t: Tokens) {
    let col = if d.x != 0.0 {
        t.axis_x
    } else if d.y != 0.0 {
        t.axis_y
    } else {
        t.axis_z
    };
    let positive = !name.starts_with('-');
    let light = t.panel.r() > 128;
    // Positive shafts are the full axis colour; negative ones are a dimmed tint (and any end
    // pointing away from the viewer dims further).
    let ink = if !positive || depth < -0.01 { col.gamma_multiply(0.55) } else { col };
    let radius = if positive { 11.0 } else { 4.8 };
    let direction = (at - c).normalized();
    let width = 3.6;
    // A negative shaft stops at the outer edge of its ring; a positive one runs under its disc.
    let end = if positive { at - direction * radius } else { at - direction * (radius + 1.2) };
    painter.line_segment([c, end], Stroke::new(width, ink));
    // The round cap sits at the origin; the other end is covered by the disc or ring.
    painter.circle_filled(c, width / 2.0, ink);
    if positive {
        painter.circle_filled(at, radius, col);
    }
    // Negative tips are hollow: neither the axis shaft nor a fill covers their
    // centres, so the translucent compass background shows through. A hovered tip of either
    // kind turns fully white.
    if hovered {
        // Same outer size as the resting tip: a ring's outline is half its stroke outside.
        painter.circle_filled(at, if positive { radius } else { radius + 1.2 }, t.on_accent);
        // On the light theme a white disc vanishes into the compass, so it carries an outline in
        // the axis colour, drawn inside the disc so it does not grow.
        if light && positive {
            painter.circle_stroke(at, radius - 1.0, Stroke::new(2.0, col));
        }
    }
    if positive {
        // On a hovered (white) disc the letter takes the axis colour.
        let letter = if hovered { col } else { t.on_accent };
        let label = name.to_uppercase();
        let galley = painter.layout_no_wrap(label, crate::fonts::bold(crate::theme::Tokens::get().font_compass), letter);
        let origin = at - galley.size() / 2.0;
        let ppp = painter.ctx().pixels_per_point().max(0.5);
        let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
        painter.galley(origin, galley, letter);
    }
    if !positive {
        painter.circle_stroke(
            at,
            radius,
            Stroke::new(
                2.4,
                if !hovered {
                    ink
                } else if light {
                    col
                } else {
                    t.on_accent
                },
            ),
        );
    }
}
