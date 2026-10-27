//! Axis compass cube: a third view navigator. A fixed ring carries Home and Fit at the
//! top; its left/right 10% arcs snap ±90° yaw orbit and its top/bottom 10% arcs snap
//! ±90° pitch orbit (over the top, underneath), and dragging the ring rolls about
//! the viewing direction (the inner area orbits instead). Coloured XYZ spheres,
//! translucent faces and generous corner hotspots complete it.
//! Faces snap to orthographic views, edges to 45° diagonals and corners to isometric views.
use super::*;
use crate::icons::navigation::{Facet, face_text};
use solidcraft_engine::geom::Ring;

pub(super) const SIZE: f32 = 140.0;
// Paint and picking share one design space, scaled as a group.
pub(super) const DESIGN_SIZE: f32 = 184.0;
const HALF: f64 = 38.0;
// Thirds with a little extra in the middle: corners and edges stay easy to hit while the
// face keeps a readable label area.
const INSET: f64 = 0.60;
const RING_RADIUS: f32 = 80.0;
/// Half-width of the ring's pointer band (design units each side of the radius).
const RING_BAND: f32 = 10.0;
/// Each snap zone covers 10% of the circle: 36° wide, so 18° each side of its axis.
const ARC_HALF_ANGLE: f32 = 0.314;
const CORNER_HIT: f32 = 14.0;
const EDGE_HIT: f32 = 10.0;
const SPHERE_HIT: f32 = 14.0;
const SPHERE_RADIUS: f32 = 11.5;
// Spheres sit outside the cube but inside the ring.
const SPHERE_OFFSET: f64 = 1.6;

fn project(cam: &Camera, v: Vec3, scale: f64) -> egui::Vec2 {
    let (r, u, _) = cam.basis();
    let v = cam.from_frame(v);
    vec2((v.dot(r) * scale) as f32, -(v.dot(u) * scale) as f32)
}

fn direction_label(d: Vec3) -> String {
    [(d.z, "BOTTOM", "TOP"), (d.y, "FRONT", "BACK"), (d.x, "LEFT", "RIGHT")]
        .into_iter()
        .filter_map(|(v, neg, pos)| {
            if v < 0.0 {
                Some(neg)
            } else if v > 0.0 {
                Some(pos)
            } else {
                None
            }
        })
        .collect::<Vec<_>>()
        .join(" · ")
}

struct CompassFacets {
    facets: Vec<Facet>,
    // The face each patch belongs to (its outward normal), for hover highlighting.
    normals: Vec<Vec3>,
}

fn facets(cam: &Camera, c: Pos2, half: f64) -> CompassFacets {
    let mut out: Vec<Facet> = Vec::new();
    let mut normals: Vec<Vec3> = Vec::new();
    if !cam.is_valid() {
        return CompassFacets { facets: out, normals };
    }
    let mut add = |d: Vec3, normal: Vec3, mut vertices: Vec<Vec3>, axes: Option<(Vec3, Vec3)>| {
        let depth = cam.from_frame(normal).dot(cam.back());
        if depth <= 1e-5 {
            return;
        }
        let centre = vertices.iter().copied().fold(Vec3::ZERO, |sum, p| sum + p) / vertices.len() as f64;
        let at = c + project(cam, centre, half);
        vertices.sort_by(|a, b| {
            let a = project(cam, *a - centre, half);
            let b = project(cam, *b - centre, half);
            a.y.atan2(a.x).total_cmp(&b.y.atan2(b.x))
        });
        let vertices = Ring::new(vertices);
        let outer_edges = vertices
            .edges()
            .map(|(a, b)| [(a.x, b.x), (a.y, b.y), (a.z, b.z)].into_iter().filter(|(a, b)| a.abs() == 1.0 && a == b).count() >= 2)
            .collect();
        let points = vertices.into_iter().map(|v| c + project(cam, v, half)).collect();
        out.push(Facet {
            direction: d,
            points,
            outer_edges,
            centre: at,
            label: direction_label(d),
            text_axes: axes.map(|(r, u)| (project(cam, r, 1.0), -project(cam, u, 1.0))),
            depth,
        });
        normals.push(normal);
    };
    // Nine patches per face. The middle is the orthographic face view, edge middles are
    // 45° diagonals and corners are isometric views: 6 + 12 + 8 unique targets.
    for (n, r, u) in [
        (Vec3::X, Vec3::Y, Vec3::Z),
        (-Vec3::X, -Vec3::Y, Vec3::Z),
        (Vec3::Y, -Vec3::X, Vec3::Z),
        (-Vec3::Y, Vec3::X, Vec3::Z),
        (Vec3::Z, Vec3::X, Vec3::Y),
        (-Vec3::Z, Vec3::X, -Vec3::Y),
    ] {
        let bands = [(-1.0, -INSET, -1.0), (-INSET, INSET, 0.0), (INSET, 1.0, 1.0)];
        for (x0, x1, sx) in bands {
            for (y0, y1, sy) in bands {
                let direction = if sx == 0.0 && sy == 0.0 {
                    n
                } else if sx != 0.0 && sy != 0.0 {
                    n + r * sx + u * sy
                } else if sx != 0.0 {
                    n + r * sx
                } else {
                    n + u * sy
                };
                add(
                    direction,
                    n,
                    [(x0, y0), (x1, y0), (x1, y1), (x0, y1)].into_iter().map(|(x, y)| n + r * x + u * y).collect(),
                    if sx == 0.0 && sy == 0.0 { Some((r, u)) } else { None },
                );
            }
        }
    }
    // Keep back-to-front order for picking (later facets win).
    let mut paired: Vec<(Facet, Vec3)> = out.into_iter().zip(normals).collect();
    paired.sort_by(|a, b| a.0.depth.total_cmp(&b.0.depth));
    let (facets, normals) = paired.into_iter().unzip();
    CompassFacets { facets, normals }
}

fn contains(points: &Ring<Pos2>, p: Pos2) -> bool {
    if !p.is_finite() || points.len() < 3 {
        return false;
    }
    let mut sign = 0.0_f32;
    for (a, b) in points.edges() {
        let edge = *b - *a;
        let offset = p - *a;
        let cross = edge.x * offset.y - edge.y * offset.x;
        if cross.abs() > 1e-3 {
            if sign != 0.0 && sign.signum() != cross.signum() {
                return false;
            }
            sign = cross;
        }
    }
    sign != 0.0
}

fn hit(faces: &[Facet], p: Pos2) -> Option<usize> {
    faces.iter().enumerate().rev().find(|(_, f)| contains(&f.points, p)).map(|(i, _)| i)
}

/// Visible cube corners: position, direction and whether the point faces the viewer.
fn corners(cam: &Camera, c: Pos2, half: f64, scale: f32) -> Vec<(Pos2, Vec3)> {
    let mut out = Vec::new();
    if !cam.is_valid() {
        return out;
    }
    for sx in [-1.0, 1.0] {
        for sy in [-1.0, 1.0] {
            for sz in [-1.0, 1.0] {
                let corner = Vec3::new(sx, sy, sz);
                let adjacent = [Vec3::new(sx, 0.0, 0.0), Vec3::new(0.0, sy, 0.0), Vec3::new(0.0, 0.0, sz)];
                let visible = adjacent.iter().any(|n| cam.from_frame(*n).dot(cam.back()) > 1e-5);
                if visible {
                    out.push((c + project(cam, corner, half), corner));
                }
            }
        }
    }
    let _ = scale;
    out
}

/// Visible edge midpoints: position and diagonal direction.
fn edges(cam: &Camera, c: Pos2, half: f64) -> Vec<(Pos2, Vec3)> {
    let mut out = Vec::new();
    if !cam.is_valid() {
        return out;
    }
    let candidates = [
        (Vec3::new(1.0, 1.0, 0.0), [Vec3::X, Vec3::Y]),
        (Vec3::new(1.0, -1.0, 0.0), [Vec3::X, -Vec3::Y]),
        (Vec3::new(-1.0, 1.0, 0.0), [-Vec3::X, Vec3::Y]),
        (Vec3::new(-1.0, -1.0, 0.0), [-Vec3::X, -Vec3::Y]),
        (Vec3::new(1.0, 0.0, 1.0), [Vec3::X, Vec3::Z]),
        (Vec3::new(1.0, 0.0, -1.0), [Vec3::X, -Vec3::Z]),
        (Vec3::new(-1.0, 0.0, 1.0), [-Vec3::X, Vec3::Z]),
        (Vec3::new(-1.0, 0.0, -1.0), [-Vec3::X, -Vec3::Z]),
        (Vec3::new(0.0, 1.0, 1.0), [Vec3::Y, Vec3::Z]),
        (Vec3::new(0.0, 1.0, -1.0), [Vec3::Y, -Vec3::Z]),
        (Vec3::new(0.0, -1.0, 1.0), [-Vec3::Y, Vec3::Z]),
        (Vec3::new(0.0, -1.0, -1.0), [-Vec3::Y, -Vec3::Z]),
    ];
    for (mid, adjacent) in candidates {
        let visible = adjacent.iter().any(|n| cam.from_frame(*n).dot(cam.back()) > 1e-5);
        if visible {
            out.push((c + project(cam, mid, half), mid));
        }
    }
    out
}

fn sphere_pos(cam: &Camera, c: Pos2, half: f64, axis: Vec3) -> Pos2 {
    c + project(cam, axis * SPHERE_OFFSET, half)
}

fn sphere_depth(cam: &Camera, axis: Vec3) -> f64 {
    cam.from_frame(axis).dot(cam.back())
}

pub(super) fn drag_origin(cam: &Camera, centre: Pos2, pointer: Pos2, scale: f32) -> bool {
    // The whole widget drags: the ring rolls, the inner area orbits. A plain click
    // (no drag) on a snap arc still turns the view 90°.
    if pointer.distance(centre) <= (RING_RADIUS + RING_BAND) * scale {
        return true;
    }
    hit(&facets(cam, centre, HALF * f64::from(scale)).facets, pointer).is_some()
}

/// Whether the pointer is on the ring's band (its snap arcs live there too).
pub(super) fn on_ring(centre: Pos2, pointer: Pos2, scale: f32) -> bool {
    (pointer.distance(centre) - RING_RADIUS * scale).abs() <= RING_BAND * scale
}

fn navigate(app: &mut SolveApp, action: Option<&str>, dir: Option<Vec3>) {
    let mut params = json!({"camera":app.cam});
    if let Some(action) = action {
        params["action"] = json!(action);
    }
    if let Some(d) = dir {
        params["direction"] = json!([d.x, d.y, d.z]);
    }
    let _ = app.run("view.navigate", params);
}

fn control(app: &mut SolveApp, ui: &mut egui::Ui, bounds: Rect, action: &str, tooltip: &str, prefix: &str) -> bool {
    crate::scenario::publish_handle(&format!("{prefix}:control:{action}"), bounds.center());
    // Keep the original cube's automation working beside the new widget.
    if prefix == "compass_cube" {
        crate::scenario::publish_handle(&format!("navigation_cube:control:{action}"), bounds.center());
    }
    let response = ui.interact(bounds, ui.id().with(("compass_cube", action)), Sense::click()).on_hover_text(tooltip);
    let hover = response.hovered();
    if response.clicked() {
        navigate(app, Some(action), None);
    }
    hover
}

fn axis_count(d: Vec3) -> usize {
    [d.x, d.y, d.z].into_iter().filter(|v| *v != 0.0).count()
}

fn paint_compass(
    painter: &egui::Painter,
    faces: &[Facet],
    normals: &[Vec3],
    hovered_dir: Option<Vec3>,
    hovered_normal: Option<Vec3>,
    t: Tokens,
    active: bool,
    scale: f32,
) {
    let mut fill_mesh = egui::epaint::Mesh::default();
    let mut outline: Vec<(Pos2, Pos2, bool, bool)> = Vec::new();
    for (face, normal) in faces.iter().zip(normals.iter()) {
        let is_target = hovered_dir.is_some_and(|d| d == face.direction);
        let adjacent = hovered_dir.is_some_and(|d| {
            if axis_count(d) == 3 {
                (d.x != 0.0 && *normal == Vec3::new(d.x.signum(), 0.0, 0.0))
                    || (d.y != 0.0 && *normal == Vec3::new(0.0, d.y.signum(), 0.0))
                    || (d.z != 0.0 && *normal == Vec3::new(0.0, 0.0, d.z.signum()))
            } else if axis_count(d) == 2 {
                *normal == Vec3::new(d.x.signum(), 0.0, 0.0)
                    || *normal == Vec3::new(0.0, d.y.signum(), 0.0)
                    || *normal == Vec3::new(0.0, 0.0, d.z.signum())
            } else {
                hovered_normal.is_some_and(|n| n == *normal)
            }
        });
        let base = t.cube_face;
        let shade = (0.72 + 0.28 * face.depth.min(1.0)) as f32;
        let (r, g, b) = (f32::from(base.r()) * shade, f32::from(base.g()) * shade, f32::from(base.b()) * shade);
        let (fill, alpha) = if is_target {
            (t.cube_hover, 255_u8)
        } else if adjacent {
            (t.cube_hover, 150_u8)
        } else if active {
            (Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, 150), 150_u8)
        } else {
            (Color32::from_rgba_unmultiplied(r as u8, g as u8, b as u8, 110), 110_u8)
        };
        let fill = if fill.a() == 255 { fill } else { Color32::from_rgba_unmultiplied(fill.r(), fill.g(), fill.b(), alpha) };
        let centre = fill_mesh.vertices.len() as u32;
        fill_mesh.colored_vertex(face.centre, fill);
        for p in &face.points {
            fill_mesh.colored_vertex(*p, fill);
        }
        for (j, (a, b)) in face.points.edges().enumerate() {
            fill_mesh.add_triangle(centre, centre + 1 + j as u32, centre + 1 + ((j + 1) % face.points.len()) as u32);
            let outer = face.outer_edges.get(j).copied().unwrap_or(false);
            if !outer && !is_target && !adjacent {
                continue;
            }
            if let Some(edge) = outline
                .iter_mut()
                .find(|(p, q, _, _)| (p.distance_sq(*a) < 1e-6 && q.distance_sq(*b) < 1e-6) || (p.distance_sq(*b) < 1e-6 && q.distance_sq(*a) < 1e-6))
            {
                if is_target {
                    edge.2 = true;
                }
                edge.3 |= outer;
            } else {
                outline.push((*a, *b, is_target, outer));
            }
        }
    }
    painter.add(egui::Shape::mesh(fill_mesh));
    // Slightly rounded silhouette corners: cap true corners with discs and pull the
    // meeting edges back, so the cube reads soft. Hit-testing keeps the sharp,
    // generous polygons above; only the paint is rounded.
    let radius = 3.0 * scale.max(0.6);
    let mut corners: Vec<(Pos2, bool)> = Vec::new();
    let mut verts: Vec<Pos2> = Vec::new();
    for (a, b, _, outer) in &outline {
        if *outer {
            for end in [*a, *b] {
                if !verts.iter().any(|v| v.distance_sq(end) < 1e-6) {
                    verts.push(end);
                }
            }
        }
    }
    for v in verts {
        let mut dirs: Vec<egui::Vec2> = Vec::new();
        let mut accent = false;
        for (a, b, highlighted, outer) in &outline {
            if *highlighted && (a.distance_sq(v) < 1e-6 || b.distance_sq(v) < 1e-6) {
                accent = true;
            }
            if *outer {
                if a.distance_sq(v) < 1e-6 {
                    dirs.push(*b - v);
                } else if b.distance_sq(v) < 1e-6 {
                    dirs.push(*a - v);
                }
            }
        }
        // Exactly two non-collinear outer edges: a silhouette corner. Straight
        // continuations (picking sub-segment joints) stay sharp.
        if let [d0, d1] = dirs.as_slice() {
            let (d0, d1) = (d0.normalized(), d1.normalized());
            if d0.dot(d1).abs() < 0.96 {
                corners.push((v, accent));
            }
        }
    }
    for (a, b, highlighted, outer) in &outline {
        if !outer && !highlighted {
            continue;
        }
        let width = if *highlighted {
            2.0
        } else if active {
            1.4
        } else {
            1.0
        } * scale.max(0.6);
        // Pull outer edges back from rounded corners so the caps make the curve.
        let trim = |end: Pos2, other: Pos2| -> Pos2 {
            if *outer && corners.iter().any(|(v, _)| v.distance_sq(end) < 1e-6) {
                let along = other - end;
                if along.length() > radius {
                    return end + along.normalized() * radius;
                }
            }
            end
        };
        painter.line_segment([trim(*a, *b), trim(*b, *a)], Stroke::new(width, if *highlighted { t.accent } else { t.cube_border }));
    }
    for (v, accent) in corners {
        painter.circle_filled(v, radius, if accent { t.accent } else { t.cube_border });
    }
}

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect) {
    let outer = axis_rect(app, rect);
    if !outer.is_positive() {
        app.viewport.axis_drag = None;
        app.viewport.axis_drag_roll = false;
        crate::scenario::publish_count("axis_view_highlights", 0.0);
        return;
    }
    crate::scenario::publish_rect("axis_view:outer", outer);
    let scale = outer.width() / DESIGN_SIZE;
    let c = axis_center(app, rect);
    let orbiting = axis_view_orbit(app, ui, rect);
    let active = orbiting || ui.rect_contains_pointer(outer);
    let t = Tokens::get().navigation_cube(active);
    crate::scenario::publish_count("compass_cube:active", f64::from(u8::from(active)));
    crate::scenario::publish_count("navigation_cube:active", f64::from(u8::from(active)));
    let compiled = facets(&app.cam, c, HALF * f64::from(scale));
    let faces = &compiled.facets;
    let normals = &compiled.normals;
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| ui.is_enabled() && ui.rect_contains_pointer(rect) && !crate::shell::covered(app, *p));
    let corner_list = corners(&app.cam, c, HALF * f64::from(scale), scale);
    let edge_list = edges(&app.cam, c, HALF * f64::from(scale));
    let sphere_axes = [Vec3::X, Vec3::Y, Vec3::Z];
    let sphere_at: Vec<Pos2> = sphere_axes.iter().map(|a| sphere_pos(&app.cam, c, HALF * f64::from(scale), *a)).collect();
    // Hover priority: spheres, corners, edges, then faces. Ring buttons are separate interacts.
    let mut hovered_sphere: Option<usize> = None;
    let mut hovered_corner: Option<Vec3> = None;
    let mut hovered_edge: Option<Vec3> = None;
    let mut hovered_facet: Option<usize> = None;
    if !orbiting && let Some(p) = hover {
        for (i, at) in sphere_at.iter().enumerate() {
            if p.distance(*at) <= SPHERE_HIT * scale {
                hovered_sphere = Some(i);
                break;
            }
        }
        if hovered_sphere.is_none() {
            for (at, dir) in &corner_list {
                if p.distance(*at) <= CORNER_HIT * scale {
                    hovered_corner = Some(*dir);
                    break;
                }
            }
        }
        if hovered_sphere.is_none() && hovered_corner.is_none() {
            for (at, dir) in &edge_list {
                if p.distance(*at) <= EDGE_HIT * scale {
                    hovered_edge = Some(*dir);
                    break;
                }
            }
        }
        if hovered_sphere.is_none() && hovered_corner.is_none() && hovered_edge.is_none() {
            hovered_facet = hit(faces, p);
        }
    }
    let hovered_dir = hovered_sphere
        .and_then(|i| sphere_axes.get(i).copied())
        .or(hovered_corner)
        .or(hovered_edge)
        .or_else(|| hovered_facet.and_then(|i| faces.get(i).map(|f| f.direction)));
    let hovered_normal = hovered_facet.and_then(|i| normals.get(i).copied());
    let painter = ui.painter_at(outer);
    // Fixed ring. Home and Fit stay at the top while the cube turns inside it.
    painter.circle_stroke(c, RING_RADIUS * scale, Stroke::new(7.0 * scale.max(0.6), t.cube_border));
    paint_compass(&painter, faces, normals, hovered_dir, hovered_normal, t, active, scale);
    // Face labels on the orthographic centres.
    if active || hovered_dir.is_some() {
        for face in faces.iter() {
            if face.text_axes.is_some() {
                face_text(&painter, face, t.cube_text, t.font_heading * scale);
            }
        }
    }
    // Publish cube targets under both the new and the original names.
    for (i, face) in faces.iter().enumerate() {
        if faces.iter().take(i).any(|f| f.direction == face.direction) {
            continue;
        }
        let pieces: Vec<_> = faces.iter().filter(|f| f.direction == face.direction).collect();
        let centre = pieces.iter().fold(egui::Vec2::ZERO, |sum, f| sum + f.centre.to_vec2()) / pieces.len() as f32;
        let name = face.label.to_ascii_lowercase().replace(" · ", "_");
        crate::scenario::publish_handle(&format!("compass_cube:{name}"), centre.to_pos2());
        crate::scenario::publish_handle(&format!("navigation_cube:{name}"), centre.to_pos2());
        let world = app.cam.from_frame(face.direction);
        let axis = match (world.x, world.y, world.z) {
            (1.0, 0.0, 0.0) => Some("x"),
            (-1.0, 0.0, 0.0) => Some("-x"),
            (0.0, 1.0, 0.0) => Some("y"),
            (0.0, -1.0, 0.0) => Some("-y"),
            (0.0, 0.0, 1.0) => Some("z"),
            (0.0, 0.0, -1.0) => Some("-z"),
            _ => None,
        };
        if let Some(axis) = axis {
            crate::scenario::publish_handle(&format!("axis_view:{axis}"), centre.to_pos2());
        }
    }
    // Corner and edge hotspots get generous handles even where the paint stays thin.
    for (at, dir) in corner_list.iter().chain(edge_list.iter()) {
        let name = direction_label(*dir).to_ascii_lowercase().replace(" · ", "_");
        crate::scenario::publish_handle(&format!("compass_cube:{name}"), *at);
        crate::scenario::publish_handle(&format!("navigation_cube:{name}"), *at);
    }
    // Coloured XYZ spheres for quick axis orientation.
    for (i, axis) in sphere_axes.iter().enumerate() {
        let at = sphere_at.get(i).copied().unwrap_or(c);
        let name = ["x", "y", "z"].get(i).copied().unwrap_or("x");
        crate::scenario::publish_handle(&format!("compass_cube:{name}"), at);
        crate::scenario::publish_handle(&format!("axis_view:{name}"), at);
        let depth = sphere_depth(&app.cam, *axis);
        let col = match i {
            0 => t.axis_x,
            1 => t.axis_y,
            _ => t.axis_z,
        };
        let hovered = hovered_sphere == Some(i);
        let radius = SPHERE_RADIUS * scale;
        if depth > -0.01 {
            painter.circle_filled(at, radius, col);
        } else {
            painter.circle_stroke(at, radius * 0.55, Stroke::new(2.0 * scale.max(0.6), col.gamma_multiply(0.6)));
        }
        if hovered {
            painter.circle_filled(at, radius, egui::Color32::WHITE);
            painter.circle_stroke(at, radius - 1.0, Stroke::new(2.0, col));
        }
        let label = name.to_uppercase();
        let galley =
            painter.layout_no_wrap(label, crate::fonts::bold(crate::theme::Tokens::get().font_compass), if hovered { col } else { t.on_accent });
        let origin = at - galley.size() / 2.0;
        let ppp = painter.ctx().pixels_per_point().max(0.5);
        let origin = pos2((origin.x * ppp).round() / ppp, (origin.y * ppp).round() / ppp);
        // Behind the cube the letter sits on the hollow ring instead of a filled disc.
        if depth > -0.01 || hovered {
            painter.galley(origin, galley, if hovered { col } else { t.on_accent });
        }
    }
    // Corner hotspot: dashed ring and centre dot where three faces meet.
    if let Some(dir) = hovered_corner.or_else(|| hovered_dir.filter(|d| axis_count(*d) == 3)) {
        let at = corner_list
            .iter()
            .find(|(_, d)| *d == dir)
            .map(|(p, _)| *p)
            .unwrap_or_else(|| hovered_facet.and_then(|i| faces.get(i).map(|f| f.centre)).unwrap_or(c));
        painter.circle_filled(at, 16.0 * scale, t.warning.gamma_multiply(0.22));
        let dashed: Vec<Pos2> = (0..=24)
            .map(|k| {
                let a = k as f32 / 24.0 * std::f32::consts::TAU;
                at + vec2(a.cos(), a.sin()) * 13.0 * scale
            })
            .collect();
        painter.add(egui::Shape::dashed_line(&dashed, Stroke::new(1.6 * scale.max(0.6), t.warning), 4.0 * scale, 3.0 * scale));
        painter.circle_filled(at, 4.5 * scale, egui::Color32::WHITE);
        painter.circle_stroke(at, 4.5 * scale, Stroke::new(1.6 * scale.max(0.6), t.warning));
    } else if let Some(dir) = hovered_edge.or_else(|| hovered_dir.filter(|d| axis_count(*d) == 2)) {
        let at = edge_list.iter().find(|(_, d)| *d == dir).map(|(p, _)| *p).unwrap_or(c);
        let r = 7.0 * scale;
        let diamond = vec![at + vec2(0.0, -r), at + vec2(r, 0.0), at + vec2(0.0, r), at + vec2(-r, 0.0)];
        painter.add(egui::Shape::convex_polygon(diamond, t.warning.gamma_multiply(0.30), Stroke::new(1.6 * scale.max(0.6), t.warning)));
        painter.circle_filled(at, 2.5 * scale, egui::Color32::WHITE);
    }
    let any_hover = hovered_dir.is_some();
    crate::scenario::publish_count("axis_view_highlights", f64::from(u8::from(any_hover)));
    // The ring's snap arcs (left/right yaw, top/bottom roll) cover 10% each; the rest
    // of the ring drags to roll.
    let on_snap_arc = if orbiting {
        false
    } else {
        hover.is_some_and(|p| {
            if (p.distance(c) - RING_RADIUS * scale).abs() > RING_BAND * scale {
                return false;
            }
            let d = p - c;
            let ang = d.y.atan2(d.x);
            let dist_to = |target: f32| {
                let wrapped = (ang - target + std::f32::consts::PI).rem_euclid(std::f32::consts::TAU) - std::f32::consts::PI;
                wrapped.abs()
            };
            dist_to(0.0) <= ARC_HALF_ANGLE
                || dist_to(std::f32::consts::PI) <= ARC_HALF_ANGLE
                || dist_to(-std::f32::consts::FRAC_PI_2) <= ARC_HALF_ANGLE
                || dist_to(std::f32::consts::FRAC_PI_2) <= ARC_HALF_ANGLE
        })
    };
    if orbiting {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    } else if any_hover || on_snap_arc {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    } else if active && hover.is_some_and(|p| p.distance(c) <= (RING_RADIUS + RING_BAND) * scale) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    // Tooltip and click for the cube / spheres / hotspots.
    if let Some(dir) = hovered_dir {
        let label = direction_label(dir);
        let tip = if hovered_sphere.is_some() {
            format!(
                "View from {}. Click again to reverse. Drag to orbit{}.",
                label,
                if app.preferences.free_orbit { "" } else { "; hold Shift to snap" }
            )
        } else {
            format!("{label} view. Drag to orbit{}.", if app.preferences.free_orbit { "" } else { "; hold Shift to snap" })
        };
        ui.interact(Rect::from_center_size(hover.unwrap_or(c), vec2(2.0, 2.0)), ui.id().with("compass_cube_tip"), Sense::hover()).on_hover_text(tip);
        if ui.input(|i| i.pointer.primary_clicked()) {
            if hovered_sphere.is_some() {
                let frame = app.cam.to_frame(dir);
                let target = if app.cam.back().dot(dir) > 0.9999 { -dir } else { dir };
                let mut to = app.cam.looking_from(target);
                if frame.z.abs() > 0.5 {
                    to.yaw = crate::viewport::nearest_quarter_turn(app.cam.yaw);
                }
                app.animate_to(to);
            } else {
                navigate(app, None, Some(dir));
            }
        }
    }
    // Fixed ring controls: Home and Fit sit on the ring at the top, flanking the top arc.
    let home = Rect::from_center_size(c + vec2(-56.5, -56.5) * scale, vec2(26.0, 26.0) * scale);
    crate::scenario::publish_target("axis_view:home", home);
    let home_hover = control(app, ui, home, "home", "Home: isometric view and fit model", "compass_cube");
    if active {
        painter.circle(home.center(), 12.0 * scale, t.overlay.gamma_multiply(if home_hover { 0.9 } else { 0.65 }), Stroke::new(1.0, t.border));
        icons::paint_icon(&painter, home.shrink(4.0 * scale), "home", f32::from(t.icon.a()) / 255.0);
    }
    let fit = Rect::from_center_size(c + vec2(56.5, -56.5) * scale, vec2(26.0, 26.0) * scale);
    crate::scenario::publish_target("compass_cube:fit", fit);
    let fit_hover = control(app, ui, fit, "fit", "Fit model in view", "compass_cube");
    if active {
        painter.circle(fit.center(), 11.0 * scale, t.overlay.gamma_multiply(if fit_hover { 0.9 } else { 0.65 }), Stroke::new(1.0, t.border));
        painter.circle_stroke(fit.center(), 5.0 * scale, Stroke::new(1.6 * scale.max(0.6), t.icon));
        painter.line_segment(
            [fit.center() + vec2(-8.0, -8.0) * scale, fit.center() + vec2(-4.0, -4.0) * scale],
            Stroke::new(1.6 * scale.max(0.6), t.icon),
        );
        painter.line_segment(
            [fit.center() + vec2(8.0, -8.0) * scale, fit.center() + vec2(4.0, -4.0) * scale],
            Stroke::new(1.6 * scale.max(0.6), t.icon),
        );
        painter.line_segment(
            [fit.center() + vec2(-8.0, 8.0) * scale, fit.center() + vec2(-4.0, 4.0) * scale],
            Stroke::new(1.6 * scale.max(0.6), t.icon),
        );
        painter
            .line_segment([fit.center() + vec2(8.0, 8.0) * scale, fit.center() + vec2(4.0, 4.0) * scale], Stroke::new(1.6 * scale.max(0.6), t.icon));
    }
    // Snap arcs live on the ring itself: left/right quarter-turn yaw orbit about the
    // vertical axis (Front → Right → Back → Left, elevation preserved), top/bottom
    // quarter-turn pitch orbit (over the top, underneath, heading preserved). No icons
    // or labels: each arc lights up on hover and names itself in its tooltip.
    for (action, base, extent, tooltip) in [
        ("yaw_left", std::f32::consts::PI, vec2(24.0, 50.0), "Orbit left 90° around the vertical axis"),
        ("yaw_right", 0.0, vec2(24.0, 50.0), "Orbit right 90° around the vertical axis"),
        ("pitch_up", -std::f32::consts::FRAC_PI_2, vec2(50.0, 24.0), "Tilt up 90° over the top"),
        ("pitch_down", std::f32::consts::FRAC_PI_2, vec2(50.0, 24.0), "Tilt down 90° underneath"),
    ] {
        let at = c + vec2(base.cos(), base.sin()) * RING_RADIUS * scale;
        let bounds = Rect::from_center_size(at, extent * scale);
        let hovered = control(app, ui, bounds, action, tooltip, "compass_cube");
        if active {
            let arc: Vec<Pos2> = (0..=12)
                .map(|k| {
                    let a = base + (k as f32 / 12.0 - 0.5) * 2.0 * ARC_HALF_ANGLE;
                    c + vec2(a.cos(), a.sin()) * RING_RADIUS * scale
                })
                .collect();
            painter.add(egui::Shape::line(arc, Stroke::new(7.0 * scale.max(0.6), if hovered { t.accent } else { t.accent.gamma_multiply(0.55) })));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::f64::consts::FRAC_PI_2;

    /// The ring's snap arcs turn the view 90° (left/right yaw orbit with elevation,
    /// top/bottom pitch orbit over the top and underneath), while a drag starting
    /// on the ring rolls the model after the finger.
    #[test]
    fn ring_arcs_snap_and_ring_drag_rolls() {
        let mut h = crate::scenario::Harness::new();
        h.call("ui.prefs", json!({"navigator_style":"compass_cube"}));
        h.app.preferences.look_at_anim = false;
        h.call("ui.view", json!({"view":"front"}));
        // An elevated view, so preserved elevation is observable.
        h.app.cam.pitch = 0.5;
        let before = h.app.cam;
        let p = crate::scenario::handle_at("compass_cube:control:yaw_right").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!((h.app.cam.pitch - before.pitch).abs() < 1e-9, "yaw_right keeps elevation");
        assert!((h.app.cam.yaw - before.yaw + FRAC_PI_2).abs() < 1e-9, "yaw_right is Front → Right");
        assert_eq!((h.app.cam.target, h.app.cam.distance), (before.target, before.distance));
        assert!(h.app.viewport.axis_drag.is_none());
        let p = crate::scenario::handle_at("compass_cube:control:yaw_left").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!((h.app.cam.yaw - before.yaw).abs() < 1e-9, "yaw_left returns");
        assert!((h.app.cam.pitch - before.pitch).abs() < 1e-9);
        // The top/bottom arcs tip over the top and underneath, keeping the heading.
        let lim = std::f64::consts::FRAC_PI_2 - 1e-6;
        let p = crate::scenario::handle_at("compass_cube:control:pitch_up").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!((h.app.cam.pitch - lim).abs() < 1e-9, "top arc tips over the top");
        assert!((h.app.cam.yaw - before.yaw).abs() < 1e-9, "pitch_up keeps the heading");
        assert!(h.app.cam.back().dist(Vec3::Z) < 1e-5, "over the top looks down");
        assert_eq!((h.app.cam.target, h.app.cam.distance), (before.target, before.distance));
        let p = crate::scenario::handle_at("compass_cube:control:pitch_down").unwrap();
        h.call("ui.click", json!({"x":p.x,"y":p.y}));
        assert!((h.app.cam.pitch - (lim - std::f64::consts::FRAC_PI_2)).abs() < 1e-9, "bottom arc tips back down");
        assert!((h.app.cam.yaw - before.yaw).abs() < 1e-9);
        assert_eq!((h.app.cam.target, h.app.cam.distance), (before.target, before.distance));
        // A drag starting on the ring (diagonal, outside every snap arc) rolls after the finger.
        let settled = h.app.cam;
        let c = h.app.viewport.layout.as_ref().unwrap().axis_view.center();
        let outer = crate::scenario::rect_of("axis_view:outer").unwrap();
        let scale = outer.width() / DESIGN_SIZE;
        let diagonal = std::f32::consts::FRAC_PI_4;
        let p0 = c + vec2(diagonal.cos(), diagonal.sin()) * RING_RADIUS * scale;
        let p1 = p0 + vec2(20.0, 12.0);
        h.call("ui.drag", json!({"x0":p0.x,"y0":p0.y,"x1":p1.x,"y1":p1.y,"steps":8}));
        let a0 = f64::from(p0.y - c.y).atan2(f64::from(p0.x - c.x));
        let a1 = f64::from(p1.y - c.y).atan2(f64::from(p1.x - c.x));
        let turn = (a1 - a0 + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU) - std::f64::consts::PI;
        assert!((h.app.cam.roll - settled.roll - turn).abs() < 1e-6, "the model follows the finger around the ring");
        assert!(h.app.cam.back().dist(settled.back()) < 1e-9, "rolling never moves the viewing direction");
        assert_eq!((h.app.cam.target, h.app.cam.distance), (settled.target, settled.distance));
        assert!(h.app.viewport.axis_drag.is_none());
        assert!(!h.app.viewport.axis_drag_roll);
    }
}
