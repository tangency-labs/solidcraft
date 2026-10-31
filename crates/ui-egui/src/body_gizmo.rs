//! The custom view gizmo: the body chosen in [`crate::view_gizmo`] drawn in place of the navigation
//! cube, made the way the cube is: the same size, each face shaded by how squarely it meets the
//! eye. Its texture is the cube's face and border colours, the hovered face in the cube's hover
//! colour and accent (themed), or the body's own appearance with the view's edge colour, the
//! hovered face lit as the view lights a face under the pointer (modeled); the cube's orbit and roll
//! arrows and its Home button go around it when the `navigator_arrows` and `navigator_home`
//! preferences say so. While the Custom View Navigator panel is open it shows the panel's choices.
//! A click on a face looks straight at it; a drag orbits.
//!
//! A concave body cannot be drawn back to front as the cube's six faces are, so the faces go
//! through a small depth-buffered raster (drawn on the CPU, shown as a texture): hidden faces
//! stay hidden and each sample knows its triangle, so hover and click name the face actually
//! seen. Edges stay crisp vector lines, cut where the raster says something nearer covers them.
use super::*;
use crate::prefs::NavigatorTexture;
use crate::view_gizmo::Shape;
use solidcraft_engine::geom::Mesh;
use std::sync::{Arc, Mutex};

/// The body's bounding sphere, in the navigation cube's design units: the cube's own (half
/// side 40, so its corners lie 40√3 from the centre).
const RADIUS: f32 = 69.28;
/// Samples per pixel along each side: smooth silhouettes.
const SS: usize = 2;
/// Largest raster side in pixels (a guard against hostile UI scales).
const MAX_PIXELS: usize = 512;
/// No triangle.
const NONE: u32 = u32::MAX;

/// Centre, radius (points) of the disc the body fills, and the cube's design scale.
fn disc(app: &SolveApp, rect: Rect) -> (Pos2, f32, f32) {
    let scale = axis_rect(app, rect).width() / navigation_cube::DESIGN_SIZE;
    (axis_center(app, rect), scale * RADIUS, scale)
}

/// A press anywhere on the body's disc orbits, as a press on the cube does.
pub(super) fn drag_origin(app: &SolveApp, rect: Rect, p: Pos2) -> bool {
    let (c, r, _) = disc(app, rect);
    c.distance(p) <= r
}

/// The orthographic view of a body along a camera, its bounding sphere filling `side` samples.
struct View {
    centre: Vec3,
    radius: f64,
    half: f64,
    k: f64,
    basis: (Vec3, Vec3, Vec3),
}

impl View {
    fn new(mesh: &Mesh, cam: &Camera, side: usize) -> Option<View> {
        let bounds = mesh.bounds();
        if bounds.is_empty() || !cam.is_valid() {
            return None;
        }
        let radius = (bounds.diagonal() * 0.5).max(1e-9);
        let half = side as f64 * 0.5;
        Some(View { centre: bounds.center(), radius, half, k: (half - 1.0).max(0.5) / radius, basis: cam.basis() })
    }
    /// Sample x and y (y down) and depth toward the eye.
    fn project(&self, v: Vec3) -> (f64, f64, f64) {
        let (r, u, b) = self.basis;
        let d = v - self.centre;
        (self.half + d.dot(r) * self.k, self.half - d.dot(u) * self.k, d.dot(b))
    }
}

/// The body seen along the camera: per sample, the nearest triangle, its depth and its shade;
/// and the visible pieces of the body's edges.
struct Raster {
    /// Samples per side.
    side: usize,
    depth: Vec<f64>,
    /// Triangle index per sample ([`NONE`]: empty).
    tri: Vec<u32>,
    shade: Vec<f32>,
    /// Visible edge pieces: the edge's index and its ends, in samples.
    edges: Vec<(usize, [f64; 2], [f64; 2])>,
}

impl Raster {
    /// The triangle seen at sample (`x`, `y`).
    fn at(&self, x: usize, y: usize) -> Option<usize> {
        if x >= self.side || y >= self.side {
            return None;
        }
        self.tri.get(y * self.side + x).filter(|t| **t != NONE).map(|t| *t as usize)
    }
    /// Nothing nearer than depth `z` at sample (`x`, `y`), within `bias`.
    fn clear(&self, x: f64, y: f64, z: f64, bias: f64) -> bool {
        let limit = (self.side - 1) as f64;
        let (x, y) = (x.floor().clamp(0.0, limit), y.floor().clamp(0.0, limit));
        self.depth.get(y as usize * self.side + x as usize).is_none_or(|d| z + bias >= *d)
    }
}

/// Orthographic view of `mesh` along `cam`, its bounding sphere filling `side` samples.
fn raster(mesh: &Mesh, cam: &Camera, side: usize) -> Raster {
    let side = side.clamp(1, MAX_PIXELS * SS);
    let n = side * side;
    let mut out = Raster { side, depth: vec![f64::NEG_INFINITY; n], tri: vec![NONE; n], shade: vec![0.0; n], edges: Vec::new() };
    let Some(view) = View::new(mesh, cam, side) else { return out };
    let back = view.basis.2;
    // The cube's shading: a face square to the eye is the full face colour, an edge-on one 72 %.
    let shade = |n: Vec3| 0.72 + 0.28 * n.dot(back).clamp(0.0, 1.0) as f32;
    let limit = (side - 1) as f64;
    for (i, t) in mesh.triangles.iter().enumerate() {
        let Some(p) = mesh.tri(t) else { continue };
        let Some(normal) = (p[1] - p[0]).cross(p[2] - p[0]).normalized() else { continue };
        // A closed body shows only the triangles facing the eye.
        if normal.dot(back) <= 0.0 {
            continue;
        }
        let lit: [f32; 3] = std::array::from_fn(|j| {
            shade(t.get(j).and_then(|v| mesh.normals.get(*v as usize)).copied().filter(|v| v.dot(normal) > 0.0).unwrap_or(normal))
        });
        let [a, b, c] = p.map(|v| view.project(v));
        let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
        if !area.is_finite() || area.abs() < 1e-12 {
            continue;
        }
        let x0 = a.0.min(b.0).min(c.0).floor().clamp(0.0, limit) as usize;
        let x1 = a.0.max(b.0).max(c.0).ceil().clamp(0.0, limit) as usize;
        let y0 = a.1.min(b.1).min(c.1).floor().clamp(0.0, limit) as usize;
        let y1 = a.1.max(b.1).max(c.1).ceil().clamp(0.0, limit) as usize;
        let edge = |p: (f64, f64, f64), q: (f64, f64, f64), x: f64, y: f64| ((q.0 - p.0) * (y - p.1) - (q.1 - p.1) * (x - p.0)) / area;
        for y in y0..=y1 {
            for x in x0..=x1 {
                let (sx, sy) = (x as f64 + 0.5, y as f64 + 0.5);
                let (w0, w1, w2) = (edge(b, c, sx, sy), edge(c, a, sx, sy), edge(a, b, sx, sy));
                if w0 < 0.0 || w1 < 0.0 || w2 < 0.0 {
                    continue;
                }
                let z = w0 * a.2 + w1 * b.2 + w2 * c.2;
                let s = y * side + x;
                if out.depth.get(s).is_some_and(|d| z > *d) {
                    out.depth[s] = z;
                    out.tri[s] = u32::try_from(i).unwrap_or(NONE);
                    out.shade[s] = w0 as f32 * lit[0] + w1 as f32 * lit[1] + w2 as f32 * lit[2];
                }
            }
        }
    }
    out.edges = visible_edges(mesh, &view, &out);
    out
}

/// The pieces of the body's edges nothing nearer covers, stepping one sample at a time.
fn visible_edges(mesh: &Mesh, view: &View, r: &Raster) -> Vec<(usize, [f64; 2], [f64; 2])> {
    let bias = view.radius * 0.02;
    let mut out = Vec::new();
    for (e, line) in mesh.edges.iter().enumerate() {
        for w in line.windows(2) {
            let (p, q) = (view.project(w[0]), view.project(w[1]));
            let steps = (q.0 - p.0).abs().max((q.1 - p.1).abs()).ceil();
            if !steps.is_finite() {
                continue;
            }
            let steps = (steps as usize).clamp(1, 4 * r.side);
            let at = |s: usize| {
                let f = s as f64 / steps as f64;
                (p.0 + (q.0 - p.0) * f, p.1 + (q.1 - p.1) * f, p.2 + (q.2 - p.2) * f)
            };
            let mut run: Option<[f64; 2]> = None;
            for s in 0..steps {
                // A step is seen when its middle is.
                let (a, b) = (at(s), at(s + 1));
                let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5, (a.2 + b.2) * 0.5);
                if r.clear(mid.0, mid.1, mid.2, bias) {
                    run.get_or_insert([a.0, a.1]);
                    if s + 1 == steps
                        && let Some(start) = run.take()
                    {
                        out.push((e, start, [b.0, b.1]));
                    }
                } else if let Some(start) = run.take() {
                    out.push((e, start, [a.0, a.1]));
                }
            }
        }
    }
    out
}

/// What the faces are painted with: the hovered face, the texture, and the colours used.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    hovered: Option<u32>,
    texture: NavigatorTexture,
    active: bool,
    /// Cube face, cube hover, the theme's body colour and the view's background.
    colours: [Color32; 4],
    /// How much lighter the view draws a face under the pointer (grey levels; modeled).
    lift: u8,
}

/// The hovered face's outline: the cube's accent (themed), or the view's mouse-over edge colour
/// (modeled).
fn hover_outline(texture: NavigatorTexture, cube: &Tokens, view: &Tokens) -> Color32 {
    match texture {
        NavigatorTexture::Themed => cube.accent,
        NavigatorTexture::Modeled => view.hover_edge,
    }
}

/// The colour face `face` is painted before shading.
fn face_colour(shape: &Shape, face: Option<u32>, look: &Look) -> Color32 {
    let [cube_face, hover, body, background] = look.colours;
    let hovered = face.is_some() && face == look.hovered;
    match look.texture {
        // As on the cube, the hovered target is one flat colour.
        NavigatorTexture::Themed if hovered => hover,
        NavigatorTexture::Themed => cube_face,
        NavigatorTexture::Modeled => {
            let own = shape.colour_of(face).map_or(body, |[r, g, b]| Color32::from_rgb(r, g, b));
            if hovered {
                // As the view shows a face under the pointer: its own colour, a little lighter.
                let k = look.lift;
                return Color32::from_rgb(own.r().saturating_add(k), own.g().saturating_add(k), own.b().saturating_add(k));
            }
            // Idle, it fades into the view as the cube does.
            if look.active { own } else { own.lerp_to_gamma(background, 0.55) }
        }
    }
}

/// The raster's pixels: each face's colour shaded, the hovered face flat, nothing (transparent)
/// around the body.
fn compose(r: &Raster, shape: &Shape, look: &Look) -> egui::ColorImage {
    let mesh = &shape.mesh;
    let px = r.side / SS;
    let dim = |c: Color32, s: f32| Color32::from_rgb((f32::from(c.r()) * s) as u8, (f32::from(c.g()) * s) as u8, (f32::from(c.b()) * s) as u8);
    let mut colours: std::collections::HashMap<Option<u32>, Color32> = std::collections::HashMap::new();
    let mut pixels = Vec::with_capacity(px * px);
    for y in 0..px {
        for x in 0..px {
            let mut sum = [0.0_f32; 4];
            for sy in 0..SS {
                for sx in 0..SS {
                    let i = (y * SS + sy) * r.side + x * SS + sx;
                    let Some(&tri) = r.tri.get(i).filter(|t| **t != NONE) else { continue };
                    let face = mesh.tri_face.get(tri as usize).copied();
                    let base = *colours.entry(face).or_insert_with(|| face_colour(shape, face, look));
                    let flat = face.is_some() && face == look.hovered && look.texture == NavigatorTexture::Themed;
                    let colour = if flat { base } else { dim(base, r.shade.get(i).copied().unwrap_or(1.0)) };
                    for (acc, v) in sum.iter_mut().zip([colour.r(), colour.g(), colour.b(), 255]) {
                        *acc += f32::from(v);
                    }
                }
            }
            let n = (SS * SS) as f32;
            // Premultiplied: the average of the covered samples, each fully opaque.
            pixels.push(Color32::from_rgba_premultiplied((sum[0] / n) as u8, (sum[1] / n) as u8, (sum[2] / n) as u8, (sum[3] / n) as u8));
        }
    }
    egui::ColorImage::new([px, px], pixels)
}

/// Per face seen, the sample nearest the middle of its samples: where automation finds it.
fn face_samples(r: &Raster, mesh: &Mesh) -> Vec<(u32, usize)> {
    use std::collections::BTreeMap;
    let face = |t: &u32| mesh.tri_face.get(*t as usize).copied();
    let xy = |i: usize| ((i % r.side) as f64, (i / r.side) as f64);
    let mut sums: BTreeMap<u32, (f64, f64, f64)> = BTreeMap::new();
    for (i, t) in r.tri.iter().enumerate() {
        if let Some(f) = face(t) {
            let (x, y) = xy(i);
            let e = sums.entry(f).or_default();
            *e = (e.0 + x, e.1 + y, e.2 + 1.0);
        }
    }
    let mut best: BTreeMap<u32, (f64, usize)> = BTreeMap::new();
    for (i, t) in r.tri.iter().enumerate() {
        let Some(f) = face(t) else { continue };
        let Some((sx, sy, n)) = sums.get(&f) else { continue };
        let (x, y) = xy(i);
        let d = (x - sx / n).powi(2) + (y - sy / n).powi(2);
        let e = best.entry(f).or_insert((f64::INFINITY, i));
        if d < e.0 {
            *e = (d, i);
        }
    }
    best.into_iter().map(|(f, (_, i))| (f, i)).collect()
}

/// The last raster and its texture, kept while the view, body and size stay the same.
struct Cache {
    shape: u64,
    basis: (Vec3, Vec3, Vec3),
    side: usize,
    raster: Raster,
    faces: Vec<(u32, usize)>,
    look: Option<Look>,
    texture: Option<egui::TextureHandle>,
}

/// One cache per egui context, kept in its memory.
type Shared = Arc<Mutex<Option<Cache>>>;

/// Draw the custom navigator as `shown` says (the panel's choices while it is open).
pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui, rect: Rect, shown: crate::view_gizmo::Shown) {
    let Some(shape) = shown.shape else { return navigation_cube::show(app, ui, rect) };
    let outer = axis_rect(app, rect);
    if !outer.is_positive() {
        app.viewport.axis_drag = None;
        crate::scenario::publish_count("axis_view_highlights", 0.0);
        return;
    }
    crate::scenario::publish_rect("axis_view:outer", outer);
    let (c, radius, scale) = disc(app, rect);
    let orbiting = axis_view_orbit(app, ui, rect);
    let active = orbiting || ui.rect_contains_pointer(outer);
    let t = Tokens::get().navigation_cube(active);
    crate::scenario::publish_count("navigation_cube:active", f64::from(u8::from(active)));
    let ppp = ui.ctx().pixels_per_point();
    let px = ((2.0 * radius * ppp).ceil().max(1.0) as usize).min(MAX_PIXELS);
    let side = px * SS;
    let image = Rect::from_center_size(c, vec2(px as f32, px as f32) / ppp);
    let step = image.width() / side as f32;
    let hover = ui.input(|i| i.pointer.hover_pos()).filter(|p| ui.is_enabled() && ui.rect_contains_pointer(rect) && !crate::shell::covered(app, *p));
    let mesh = &shape.mesh;
    let key = (shape.id, app.cam.basis());
    let cam = app.cam;
    let texture = shown.texture;
    // The model's own look: its body colour and its edge colour, as the view draws them.
    let view = Tokens::view();
    let to_screen = |p: [f64; 2]| image.min + vec2(p[0] as f32 * step, p[1] as f32 * step);
    let painter = ui.painter_at(outer);
    let shared: Shared = ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Shared>(egui::Id::new("body_gizmo_cache")).clone());
    let (hovered, handles) = shared
        .lock()
        .ok()
        .and_then(|mut cell| {
            if !cell.as_ref().is_some_and(|k| (k.shape, k.basis) == key && k.side == side) {
                let texture = cell.take().and_then(|k| k.texture);
                let raster = raster(mesh, &cam, side);
                let faces = face_samples(&raster, mesh);
                *cell = Some(Cache { shape: key.0, basis: key.1, side, raster, faces, look: None, texture });
            }
            let cache = cell.as_mut()?;
            let tri = hover.filter(|_| !orbiting).and_then(|p| {
                let s = (p - image.min) / step;
                (s.x >= 0.0 && s.y >= 0.0).then(|| cache.raster.at(s.x as usize, s.y as usize)).flatten()
            });
            let face = tri.and_then(|t| mesh.tri_face.get(t).copied());
            let hover_outline = hover_outline(texture, &t, &view);
            let look = Look {
                hovered: face,
                texture,
                active,
                colours: [t.cube_face, t.cube_hover, view.body, view.viewport_top],
                lift: view.hover_face_lift,
            };
            if cache.look != Some(look) || cache.texture.is_none() {
                let img = compose(&cache.raster, &shape, &look);
                match cache.texture.as_mut() {
                    Some(tex) => tex.set(img, egui::TextureOptions::LINEAR),
                    None => cache.texture = Some(ui.ctx().load_texture("body_gizmo", img, egui::TextureOptions::LINEAR)),
                }
                cache.look = Some(look);
            }
            if let Some(tex) = &cache.texture {
                painter.image(tex.id(), image, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
            }
            // The body's edges in the cube's border colour; the hovered face's in the accent, on top.
            let of_hovered = |e: usize| face.is_some_and(|f| mesh.edge_faces.get(e).is_some_and(|fs| fs.contains(&f)));
            for lit in [false, true] {
                for (_, a, b) in cache.raster.edges.iter().filter(|(e, _, _)| of_hovered(*e) == lit) {
                    let edge = match texture {
                        NavigatorTexture::Themed => t.cube_border,
                        // Idle, the model's edges fade into the view with its faces.
                        NavigatorTexture::Modeled if active => view.body_edge,
                        NavigatorTexture::Modeled => view.body_edge.lerp_to_gamma(view.viewport_top, 0.55),
                    };
                    painter.line_segment([to_screen(*a), to_screen(*b)], Stroke::new(1.0, if lit { hover_outline } else { edge }));
                }
            }
            let w = cache.raster.side;
            let handles: Vec<(u32, Pos2)> = cache.faces.iter().map(|(f, i)| (*f, to_screen([(i % w) as f64 + 0.5, (i / w) as f64 + 0.5]))).collect();
            Some((tri, handles))
        })
        .unwrap_or((None, Vec::new()));
    for (face, at) in &handles {
        crate::scenario::publish_handle(&format!("body_gizmo:face:{face}"), *at);
    }
    crate::scenario::publish_count("body_gizmo:faces", handles.len() as f64);
    crate::scenario::publish_count("axis_view_highlights", f64::from(u8::from(hovered.is_some())));
    if let Some(tri) = hovered {
        crate::scenario::publish_count("body_gizmo:hovered", f64::from(mesh.tri_face.get(tri).copied().unwrap_or_default()));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui.input(|i| i.pointer.primary_clicked())
            && let Some(normal) = shape.face_normal(tri)
        {
            navigation_cube::navigate(app, None, Some(app.cam.to_frame(normal)));
        }
    }
    if shown.arrows {
        navigation_cube::arrows(app, ui, &painter, c, scale, active, false, t);
    }
    if shown.home {
        navigation_cube::home(app, ui, &painter, c, scale, active, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_engine::render::{StandardView, UpAxis};

    fn boxed() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10})).unwrap();
        h.frames(2);
        h
    }

    fn body_name(h: &crate::scenario::Harness) -> String {
        h.app.session.world_state().bodies[0].name.clone()
    }

    fn custom(h: &mut crate::scenario::Harness) {
        h.call("ui.prefs", json!({"navigator_style": "custom"}));
        let name = body_name(h);
        h.app.run("view.navigator", json!({"body": name})).unwrap();
        h.frames(2);
    }

    #[test]
    fn raster_hides_back_faces_and_sees_the_top_from_above() {
        let h = boxed();
        let shape = Shape::of_body(&h.app.session, &body_name(&h)).unwrap();
        let mesh = &shape.mesh;
        for up in [UpAxis::Z, UpAxis::Y] {
            let mut cam = Camera { up, ..Default::default() };
            cam.set_view(StandardView::Top);
            let r = raster(mesh, &cam, 64);
            let tri = r.at(32, 32).expect("the box covers the middle");
            let p = mesh.tri(&mesh.triangles[tri]).unwrap();
            let n = (p[1] - p[0]).cross(p[2] - p[0]).normalized().unwrap();
            assert!(n.dot(cam.back()) > 0.99, "{up:?}: the face seen from above faces the eye: {n:?}");
            assert!(r.at(0, 0).is_none(), "the corner of the raster is empty");
        }
        // Isometric: three faces show, never more, and nine of the box's twelve edges.
        let r = raster(mesh, &Camera::default(), 128);
        let faces: std::collections::BTreeSet<u32> = r.tri.iter().filter_map(|t| mesh.tri_face.get(*t as usize).copied()).collect();
        assert_eq!(faces.len(), 3, "{faces:?}");
        let edges: std::collections::BTreeSet<usize> = r.edges.iter().map(|(e, _, _)| *e).collect();
        assert_eq!(edges.len(), 9, "the three hidden edges stay hidden: {edges:?}");
    }

    #[test]
    fn hostile_raster_sizes_and_cameras_never_panic() {
        let h = boxed();
        let shape = Shape::of_body(&h.app.session, &body_name(&h)).unwrap();
        let look = Look { hovered: Some(0), texture: NavigatorTexture::Modeled, active: false, colours: [Color32::WHITE; 4], lift: u8::MAX };
        for side in [0, 1, 2, 3, usize::MAX] {
            let r = raster(&shape.mesh, &Camera::default(), side);
            assert!(r.side >= 1 && r.side <= MAX_PIXELS * SS);
            let _ = compose(&r, &shape, &look);
        }
        let bad = Camera { distance: f64::NAN, ..Default::default() };
        assert!(raster(&shape.mesh, &bad, 16).tri.iter().all(|t| *t == NONE));
        let empty = raster(&Mesh::default(), &Camera::default(), 16);
        assert!(empty.tri.iter().all(|t| *t == NONE) && empty.edges.is_empty());
    }

    #[test]
    fn hovering_a_face_highlights_it_and_a_click_looks_straight_at_it() {
        let mut h = boxed();
        custom(&mut h);
        let design = h.app.session.doc.to_json();
        let mut seen = 0;
        for face in 0..6_u32 {
            h.call("ui.view", json!({"view": "iso"}));
            let Some(p) = crate::scenario::handle_at(&format!("body_gizmo:face:{face}")) else { continue };
            seen += 1;
            h.call("ui.move", json!({"x": p.x, "y": p.y}));
            assert_eq!(crate::scenario::count_of("axis_view_highlights"), Some(1.0));
            assert_eq!(crate::scenario::count_of("body_gizmo:hovered"), Some(f64::from(face)));
            h.call("ui.click", json!({"x": p.x, "y": p.y}));
            let (_, n) = crate::dialogs::planar_face(&h.app.session, &body_name(&h), face as usize).expect("box faces are planar");
            let back = h.app.cam.back();
            assert!(back.dist(n) < 1e-6, "face {face}: the view looks along its normal {n:?}, not {back:?}");
        }
        assert_eq!(seen, 3, "three faces of the box show in the isometric view");
        assert_eq!(h.app.session.doc.to_json(), design, "looking at a face never edits the design");
        assert!(h.app.session.selection.is_empty());
    }

    #[test]
    fn the_rotation_arrows_and_home_follow_their_settings_and_turn_the_view() {
        let mut h = boxed();
        custom(&mut h);
        assert!(crate::scenario::handle_at("navigation_cube:control:left").is_some(), "arrows by default");
        assert!(crate::scenario::handle_at("navigation_cube:control:home").is_some(), "Home by default");
        h.app.run("view.navigator", json!({"arrows": false, "home": false})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("navigation_cube:control:left").is_none(), "arrows off");
        assert!(crate::scenario::handle_at("navigation_cube:control:home").is_none(), "Home off");
        h.app.run("view.navigator", json!({"arrows": true, "home": true})).unwrap();
        h.frames(2);
        let p = crate::scenario::handle_at("navigation_cube:control:clockwise").expect("the roll arrows are there");
        let camera = h.app.cam;
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!((h.app.cam.roll - camera.roll - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        let p = crate::scenario::handle_at("navigation_cube:control:home").expect("Home is there");
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!(h.app.cam.back().dist(Camera::default().back()) < 1e-6, "Home turns to the home view");
        assert!(crate::scenario::handle_at("navigation_cube:control:fit").is_none(), "Fit stays the cube's");
    }

    #[test]
    fn modeled_texture_paints_the_body_in_its_own_colour_and_themed_in_the_cubes() {
        let mut h = boxed();
        let name = body_name(&h);
        let mut shape = Shape::of_body(&h.app.session, &name).unwrap();
        shape.colour = Some([200, 30, 40]);
        let r = raster(&shape.mesh, &Camera::default(), 64);
        let t = Tokens::light();
        let look = |texture| Look {
            hovered: None,
            texture,
            active: true,
            colours: [t.cube_face, t.cube_hover, t.body, t.viewport_top],
            lift: t.hover_face_lift,
        };
        assert_ne!(t.body_edge, t.cube_border, "the two textures' edges can be told apart");
        // A hovered face: the cube's hover colours when themed, the view's mouse-over when modeled.
        assert_eq!(hover_outline(NavigatorTexture::Themed, &t, &t), t.accent);
        assert_eq!(hover_outline(NavigatorTexture::Modeled, &t, &t), t.hover_edge);
        assert!(t.hover_face_lift > 0);
        // The pixel over the middle of one face seen, hovered.
        let (face, sample) = face_samples(&r, &shape.mesh)[0];
        let at = (sample / r.side / SS) * (r.side / SS) + sample % r.side / SS;
        let modeled = look(NavigatorTexture::Modeled);
        let (plain, lit) = (compose(&r, &shape, &modeled).pixels[at], compose(&r, &shape, &Look { hovered: Some(face), ..modeled }).pixels[at]);
        assert!(
            lit.r() > plain.r() && lit.g() > plain.g() && lit.b() > plain.b(),
            "the hovered face is lifted as the view lifts it: {plain:?} → {lit:?}"
        );
        assert_ne!(lit, t.sel_face, "not the selection colour");
        let middle = |img: &egui::ColorImage| img.pixels[img.size[1] / 2 * img.size[0] + img.size[0] / 2];
        let red = middle(&compose(&r, &shape, &look(NavigatorTexture::Modeled)));
        assert!(red.r() > red.g() * 3 && red.r() > red.b() * 3, "the body's red: {red:?}");
        let themed = middle(&compose(&r, &shape, &look(NavigatorTexture::Themed)));
        let shade = f32::from(themed.r()) / f32::from(t.cube_face.r().max(1));
        assert!((0.7..=1.01).contains(&shade), "the cube's face colour, shaded: {themed:?}");
        // The gizmo draws without a frame failing in both textures.
        custom(&mut h);
        for texture in ["modeled", "themed"] {
            h.app.run("view.navigator", json!({"texture": texture})).unwrap();
            h.frames(2);
            assert!(crate::scenario::count_of("body_gizmo:faces").is_some_and(|n| n >= 3.0), "{texture}");
        }
    }

    #[test]
    fn custom_without_a_body_draws_the_cube() {
        let mut h = boxed();
        h.call("ui.prefs", json!({"navigator_style": "custom"}));
        h.frames(2);
        assert!(crate::scenario::handle_at("navigation_cube:front").is_some() || crate::scenario::handle_at("navigation_cube:top").is_some());
        assert_eq!(crate::scenario::count_of("body_gizmo:faces"), None);
    }
}
