//! The custom view gizmo: the body chosen in [`crate::view_gizmo`] drawn in place of the signed
//! axes, each face shaded by how squarely it meets the eye. Its texture is the theme's navigator
//! face and border colours, the hovered face in the hover colour and accent (themed), or the
//! body's own appearance with the view's edge colour, the hovered face lit as the view lights a
//! face under the pointer (modeled); orbit and roll arrows and a Home button go around it when
//! the `navigator_arrows` and `navigator_home` preferences say so. While the Custom View Navigator
//! panel is open it shows the panel's choices.
//! A click on a face looks straight at it; a drag orbits.
//!
//! A concave body cannot be drawn back to front as a box's six faces are, so the faces go
//! through a small depth-buffered raster (drawn on the CPU, shown as a texture): hidden faces
//! stay hidden and each sample knows its triangle, so hover and click name the face actually
//! seen. Edges stay crisp vector lines, cut where the raster says something nearer covers them.
use super::*;
use crate::prefs::NavigatorTexture;
use crate::view_gizmo::Shape;
use solidcraft_engine::geom::Mesh;
use std::sync::{Arc, Mutex};

/// The body's bounding sphere, in the navigator's design units: the navigator's own (half
/// side 40, so its corners lie 40√3 from the centre).
const RADIUS: f32 = 69.28;
/// Samples per pixel along each side: smooth silhouettes.
const SS: usize = 2;
/// Largest raster side in pixels (a guard against hostile UI scales).
const MAX_PIXELS: usize = 512;
/// No triangle.
const NONE: u32 = u32::MAX;

/// Centre, radius (points) of the disc the body fills, and the navigator's design scale.
fn disc(app: &SolveApp, rect: Rect) -> (Pos2, f32, f32) {
    let scale = axis_rect(app, rect).width() / navigator_controls::DESIGN_SIZE;
    (axis_center(app, rect), scale * RADIUS, scale)
}

/// A press anywhere on the body's disc orbits, as a press on the axes does.
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
    /// Where the eye sees each of the six faces: the x and y sums over the samples whose
    /// nearest visible triangle points at the face (its normal within 60° of it), and how
    /// many samples voted. Zero votes: nothing of the body shows that face.
    face: [[f64; 2]; 6],
    face_n: [u32; 6],
    /// Which faces each sample shows, one bit per face, so a face's mean is kept as its name's
    /// anchor only where the face's own samples reach it: a face holds its mean, a ring's mean
    /// (a hole) or the gap between two patches does not.
    votes: Vec<u8>,
    /// The smallest box the body fits in, in the six faces' own frame, its six face centres as
    /// the eye sees them (samples): where a face the body gives no face to show carries its
    /// name — a torus's top is a ring whose mean is its hole. The six centres of one box are
    /// apart whatever the body's shape, so names anchored there never overlap, the way the
    /// removed cube's labels, one to a face, never did.
    cube: [(f64, f64); 6],
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

    /// Whether any sample within `reach` of (`x`, `y`) shows `face` (its bit in [`Raster::votes`]):
    /// the samples of a face reach their own mean; a ring's mean, in its hole, and the mean of
    /// two patches, in the gap between them, are reached by nothing.
    fn reaches(&self, face: usize, x: f64, y: f64, reach: usize) -> bool {
        let (cx, cy) = (x.round() as isize, y.round() as isize);
        let (reach, limit) = (reach as isize, self.side as isize);
        for py in cy - reach..=cy + reach {
            for px in cx - reach..=cx + reach {
                if (0..limit).contains(&px)
                    && (0..limit).contains(&py)
                    && self.votes.get(py as usize * self.side + px as usize).is_some_and(|v| (v >> face) & 1 == 1)
                {
                    return true;
                }
            }
        }
        false
    }
}

/// Orthographic view of `mesh` along `cam`, its bounding sphere filling `side` samples.
fn raster(mesh: &Mesh, cam: &Camera, side: usize) -> Raster {
    let side = side.clamp(1, MAX_PIXELS * SS);
    let n = side * side;
    let mut out = Raster {
        side,
        depth: vec![f64::NEG_INFINITY; n],
        tri: vec![NONE; n],
        shade: vec![0.0; n],
        edges: Vec::new(),
        face: [[0.0; 2]; 6],
        face_n: [0; 6],
        votes: Vec::new(),
        cube: [(side as f64 * 0.5, side as f64 * 0.5); 6],
    };
    let Some(view) = View::new(mesh, cam, side) else { return out };
    out.votes = vec![0; n];
    let back = view.basis.2;
    // The face shading: a face square to the eye is the full face colour, an edge-on one 72 %.
    let shade = |n: Vec3| 0.72 + 0.28 * n.dot(back).clamp(0.0, 1.0) as f32;
    let limit = (side - 1) as f64;
    let mut normals = vec![Vec3::ZERO; mesh.triangles.len()];
    for (i, t) in mesh.triangles.iter().enumerate() {
        let Some(p) = mesh.tri(t) else { continue };
        let Some(normal) = (p[1] - p[0]).cross(p[2] - p[0]).normalized() else { continue };
        // A closed body shows only the triangles facing the eye.
        if normal.dot(back) <= 0.0 {
            continue;
        }
        normals[i] = normal;
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
    // The invisible minimum cube: the smallest box the body fits in, in the six faces' own
    // frame (the up axis's, so a Y-up document's top face is its up face), each face's centre
    // as the eye sees it. A face the body gives no face to show is named there instead (see
    // [`Raster::cube`]).
    let frame = [cam.from_frame(Vec3::X), cam.from_frame(Vec3::Y), cam.from_frame(Vec3::Z)];
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for p in &mesh.positions {
        for a in 0..3 {
            let c = p.dot(frame[a]);
            lo[a] = lo[a].min(c);
            hi[a] = hi[a].max(c);
        }
    }
    for (i, (dir, ..)) in FACES.iter().enumerate() {
        let centre = Vec3::new((lo[0] + hi[0]) * 0.5, (lo[1] + hi[1]) * 0.5, (lo[2] + hi[2]) * 0.5);
        let along = Vec3::new(dir.x * (hi[0] - lo[0]), dir.y * (hi[1] - lo[1]), dir.z * (hi[2] - lo[2])) * 0.5;
        let (x, y, _) = view.project(cam.from_frame(centre + along));
        out.cube[i] = if x.is_finite() && y.is_finite() { (x, y) } else { (view.half, view.half) };
    }
    // Where each face's name belongs: the mean of the samples whose nearest triangle faces
    // the face, over the faces the eye faces — what the eye sees of that face, the cube's
    // face centre for a box — and which samples show which face, for the mean to be kept
    // only where the face's own samples reach it.
    let face_dirs: [Vec3; 6] = std::array::from_fn(|i| cam.from_frame(FACES[i].0));
    for y in 0..side {
        for x in 0..side {
            let s = y * side + x;
            let t = out.tri[s];
            if t == NONE {
                continue;
            }
            let n = normals[t as usize];
            for (i, w) in face_dirs.iter().enumerate() {
                if w.dot(back) > 1e-4 && n.dot(*w) > 0.5 {
                    out.face[i][0] += x as f64 + 0.5;
                    out.face[i][1] += y as f64 + 0.5;
                    out.face_n[i] += 1;
                    out.votes[s] |= 1 << i;
                }
            }
        }
    }
    out
}

/// The pieces of the body's edges nothing nearer covers, stepping one sample at a time. A piece
/// is kept only when the stretch of the edge seen with it (across the edge's segments) is at least
/// two samples long: shorter is a sliver where a hidden edge meets the silhouette.
fn visible_edges(mesh: &Mesh, view: &View, r: &Raster) -> Vec<(usize, [f64; 2], [f64; 2])> {
    let bias = view.radius * 0.02;
    let mut out = Vec::new();
    for (e, line) in mesh.edges.iter().enumerate() {
        // The stretch seen so far: its pieces and their length.
        let mut stretch: (Vec<(usize, [f64; 2], [f64; 2])>, f64) = (Vec::new(), 0.0);
        let mut end_stretch = |stretch: &mut (Vec<(usize, [f64; 2], [f64; 2])>, f64)| {
            let (pieces, length) = std::mem::take(stretch);
            if length >= 2.0 {
                out.extend(pieces);
            }
        };
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
            let piece = |stretch: &mut (Vec<(usize, [f64; 2], [f64; 2])>, f64), start: [f64; 2], end: [f64; 2]| {
                stretch.1 += (end[0] - start[0]).hypot(end[1] - start[1]);
                stretch.0.push((e, start, end));
            };
            for s in 0..steps {
                // A step is seen when its middle is.
                let (a, b) = (at(s), at(s + 1));
                let mid = ((a.0 + b.0) * 0.5, (a.1 + b.1) * 0.5, (a.2 + b.2) * 0.5);
                if r.clear(mid.0, mid.1, mid.2, bias) {
                    run.get_or_insert([a.0, a.1]);
                    // A stretch may go on into the next segment.
                    if s + 1 == steps
                        && let Some(start) = run.take()
                    {
                        piece(&mut stretch, start, [b.0, b.1]);
                    }
                } else {
                    if let Some(start) = run.take() {
                        piece(&mut stretch, start, [a.0, a.1]);
                    }
                    end_stretch(&mut stretch);
                }
            }
        }
        end_stretch(&mut stretch);
    }
    out
}

/// The six main faces: the frame direction each is named for, the two frame axes its name's text
/// is set in (as the cube's faces were labelled: the text's right × up is the face's direction, so
/// a name reads upright when the view looks straight at its face), its handle name and the
/// message id of its name. A name follows its direction, never the body's shape, so any body
/// carries all six.
const FACES: [(Vec3, Vec3, Vec3, &str, &str); 6] = [
    (Vec3::Z, Vec3::X, Vec3::Y, "top", "ui.top"),
    (Vec3::new(0.0, 0.0, -1.0), Vec3::X, Vec3::new(0.0, -1.0, 0.0), "bottom", "ui.bottom"),
    (Vec3::new(0.0, -1.0, 0.0), Vec3::X, Vec3::Z, "front", "ui.front"),
    (Vec3::Y, Vec3::new(-1.0, 0.0, 0.0), Vec3::Z, "back", "ui.back"),
    (Vec3::X, Vec3::Y, Vec3::Z, "right", "ui.right"),
    (Vec3::new(-1.0, 0.0, 0.0), Vec3::new(0.0, -1.0, 0.0), Vec3::Z, "left", "ui.left"),
];

/// One face's name as the camera poses it: its handle name, message id, the frame direction it
/// names, where on the navigator the name is drawn, how squarely the eye meets the face (a face
/// near edge-on fades, as the cube's labels did) and the two screen axes its text is warped into
/// (right and down), so it turns with the body as the cube's labels turned with the cube.
struct FaceName {
    name: &'static str,
    id: &'static str,
    dir: Vec3,
    at: Pos2,
    opacity: f32,
    axes: (egui::Vec2, egui::Vec2),
}

/// The names of the faces the camera faces, for the body `r` draws into `image`: each of the
/// six frame directions is posed by `cam`, and the ones toward the eye are named. A name sits
/// at the centre of what the eye sees of its face — the samples whose visible face points at
/// it — when that centre lies on the face's own samples, as a face's centre does: the way the
/// cube's name sat at its face's centre, and it turns with the view, set in its face's two
/// in-plane frame axes as the camera projects them (right and down), so it reads upright when
/// the view looks straight at the face and otherwise holds the face's angle, the way the
/// cube's labels turned with the cube. A face whose centre lies on nothing the eye sees — a
/// torus's top is a ring whose centre is its hole; a body of scattered patches leaves its
/// centre in a gap — is named at the centre of the face the invisible minimum cube gives it
/// instead, where the cube's own label sat: the six centres of one box are apart whatever the
/// body's shape, so the names never overlap, and a view straight along a face still meets its
/// name at the middle. A face near edge-on fades out, as the cube's labels did.
fn face_names(r: &Raster, cam: &Camera, image: Rect) -> Vec<FaceName> {
    let (right, up, back) = cam.basis();
    let step = image.width() / r.side as f32;
    // How far from its mean a face's own samples may sit for the mean to name the face: a
    // filled region holds its mean; a hole or a gap between patches is wider than this.
    let reach = (r.side / 32).max(6);
    let mut out = Vec::new();
    for (i, (dir, r_frame, u_frame, name, id)) in FACES.into_iter().enumerate() {
        let world = cam.from_frame(dir);
        let (sx, sy) = (world.dot(right), -world.dot(up));
        let depth = world.dot(back);
        // The name's text axes on screen (points per point): the face's two in-plane frame
        // axes as the camera projects them, right and down — the same warp the cube's
        // `face_text` drew its labels with, on the plane the name's face stands for.
        let (r_world, u_world) = (cam.from_frame(r_frame), cam.from_frame(u_frame));
        let axes = (vec2(r_world.dot(right) as f32, -(r_world.dot(up)) as f32), vec2(-(u_world.dot(right)) as f32, u_world.dot(up) as f32));
        if depth <= 1e-4 || !sx.is_finite() || !sy.is_finite() || !axes.0.is_finite() || !axes.1.is_finite() {
            continue;
        }
        // Where the name sits (#19303): at the invisible minimum cube's face centre for
        // the face while the face's own samples reach it and the body carries it — on the
        // body, with body beyond it along the face's screen direction. For any box-like
        // body — sharp, filleted, chamfered or rounded alike — that centre is the face's
        // own centre, at every view, exactly where the removed cube's label sat: v6 named
        // the face at the mean of its samples instead, and on a filleted or rounded body
        // the 60° vote cone admits the skirt around each flat face, whose near edge
        // projects wider than its far one at an angled view, so the mean sat off the
        // face's centre toward the eye-facing edge — the reported off-centre labels. The
        // mean is kept only where the body does not carry the cube centre (a ring's hole,
        // a gap between patches, a sliver the silhouette barely reaches) and the face's
        // own samples reach their mean — the centre of what the eye sees of such a face
        // — else the cube centre again, so the names stay apart whatever the body.
        let votes = r.face_n[i] as f64;
        let cube = image.min + vec2(r.cube[i].0 as f32 * step, r.cube[i].1 as f32 * step);
        let mean = (r.face[i][0] / votes, r.face[i][1] / votes);
        // Walking from the anchor along the face's screen direction, body samples continue
        // for at least 2 steps: the centre is on the face, not the rim's last sample (a
        // fully rounded box bulges enough that even its face centre's walk leaves quickly).
        let len = sx.hypot(sy).max(1e-9);
        let (dx, dy) = (sx / len, sy / len);
        let carried = |x: f64, y: f64| {
            let (mut on, mut px, mut py) = (0_u32, x, y);
            while on < r.side as u32 * 4 && r.at(px as usize, py as usize).is_some() {
                on += 1;
                px += dx;
                py += dy;
            }
            on >= 2
        };
        let at = if r.reaches(i, r.cube[i].0, r.cube[i].1, reach)
            && r.at(r.cube[i].0 as usize, r.cube[i].1 as usize).is_some()
            && carried(r.cube[i].0, r.cube[i].1)
        {
            cube
        } else if votes > 0.0 && r.reaches(i, mean.0, mean.1, reach) {
            image.min + vec2(mean.0 as f32 * step, mean.1 as f32 * step)
        } else {
            cube
        };
        // A face near edge-on fades the way the cube's did: gone by 0.18, full from 0.35.
        let opacity = ((depth - 0.18) / 0.17).clamp(0.0, 1.0) as f32;
        out.push(FaceName { name, id, dir, at, opacity, axes });
    }
    out
}

/// Private marker for the names' meshes: their UVs are font-atlas pixels until the frame is
/// complete, when the hook below turns them back into texture coordinates. Another label or
/// widget can grow the atlas after a mesh is made, which would invalidate earlier UVs.
const LABEL_PIXELS: egui::TextureId = egui::TextureId::User(u64::from_le_bytes(*b"navetext"));

struct LabelAtlas;

impl egui::plugin::Plugin for LabelAtlas {
    fn debug_name(&self) -> &'static str {
        "body gizmo face-name atlas"
    }

    fn output_hook(&mut self, ctx: &egui::Context, output: &mut egui::FullOutput) {
        let size = ctx.fonts(|f| f.font_image_size());
        let normalize = vec2(1.0 / size[0].max(1) as f32, 1.0 / size[1].max(1) as f32);
        for shape in &mut output.shapes {
            if let egui::Shape::Mesh(mesh) = &mut shape.shape
                && mesh.texture_id == LABEL_PIXELS
            {
                let mesh = Arc::make_mut(mesh);
                mesh.texture_id = egui::TextureId::default();
                for v in &mut mesh.vertices {
                    v.uv = (v.uv.to_vec2() * normalize).to_pos2();
                }
            }
        }
    }
}

/// Draw `galley` centred on `at`, warped into `axes` (right, down): the text is tessellated and
/// every vertex mapped from the galley's own frame into `at + right * x + down * y`, so the name
/// turns and foreshortens with the face it sits on. The removed cube printed its labels the same
/// way (`face_text` warped the font mesh into the face's projected axes).
fn warped_text(painter: &egui::Painter, at: Pos2, (right, down): (egui::Vec2, egui::Vec2), galley: Arc<egui::Galley>, fallback: Color32) {
    let centre = galley.rect.center();
    // Keep atlas-pixel UVs until the frame is complete: another label or widget can grow the
    // atlas after this mesh is created, invalidating early UVs.
    painter.ctx().add_plugin(LabelAtlas);
    let text = egui::Shape::Text(egui::epaint::TextShape::new(Pos2::ZERO, galley, fallback));
    let atlas = painter.ctx().fonts(|f| f.font_image_size());
    let primitives = painter.ctx().tessellate(vec![egui::epaint::ClippedShape::new(Rect::EVERYTHING, text)], painter.ctx().pixels_per_point());
    for primitive in primitives {
        if let egui::epaint::Primitive::Mesh(mut mesh) = primitive.primitive {
            mesh.texture_id = LABEL_PIXELS;
            for v in &mut mesh.vertices {
                v.uv = pos2(v.uv.x * atlas[0] as f32, v.uv.y * atlas[1] as f32);
                let local = v.pos - centre;
                v.pos = at + right * local.x + down * local.y;
            }
            painter.add(egui::Shape::mesh(mesh));
        }
    }
}

/// What the faces are painted with: the hovered face, the texture, and the colours used.
#[derive(Clone, Copy, PartialEq)]
struct Look {
    hovered: Option<u32>,
    texture: NavigatorTexture,
    active: bool,
    /// Face, hover, the theme's body colour and the view's background.
    colours: [Color32; 4],
    /// How much lighter the view draws a face under the pointer (grey levels; modeled).
    lift: u8,
}

/// The hovered face's outline: the navigator's accent (themed), or the view's mouse-over edge colour
/// (modeled).
fn hover_outline(texture: NavigatorTexture, look: &Tokens, view: &Tokens) -> Color32 {
    match texture {
        NavigatorTexture::Themed => look.accent,
        NavigatorTexture::Modeled => view.hover_edge,
    }
}

/// A colour of the body's raster: the design's own appearance, or a token, shaded or lit.
fn rgb(r: u8, g: u8, b: u8) -> Color32 {
    Color32::from_rgb(r, g, b)
}

/// The colour face `face` is painted before shading.
fn face_colour(shape: &Shape, face: Option<u32>, look: &Look) -> Color32 {
    let [navigator_face, hover, body, background] = look.colours;
    let hovered = face.is_some() && face == look.hovered;
    match look.texture {
        // As on the axes, the hovered target is one flat colour.
        NavigatorTexture::Themed if hovered => hover,
        NavigatorTexture::Themed => navigator_face,
        NavigatorTexture::Modeled => {
            let own = shape.colour_of(face).map_or(body, |[r, g, b]| rgb(r, g, b));
            if hovered {
                // As the view shows a face under the pointer: its own colour, a little lighter.
                let k = look.lift;
                return rgb(own.r().saturating_add(k), own.g().saturating_add(k), own.b().saturating_add(k));
            }
            // Idle, it fades into the view as the axes does.
            if look.active { own } else { own.lerp_to_gamma(background, 0.55) }
        }
    }
}

/// The raster's pixels: each face's colour shaded, the hovered face flat, nothing (transparent)
/// around the body.
fn compose(r: &Raster, shape: &Shape, look: &Look) -> egui::ColorImage {
    let mesh = &shape.mesh;
    let px = r.side / SS;
    let dim = |c: Color32, s: f32| rgb((f32::from(c.r()) * s) as u8, (f32::from(c.g()) * s) as u8, (f32::from(c.b()) * s) as u8);
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
    let Some(shape) = shown.shape else { return };
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
    let t = Tokens::get().navigator(active);
    crate::scenario::publish_count("navigator:active", f64::from(u8::from(active)));
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
    let (want_axes, want_faces) = (shown.axes, shown.faces);
    // The model's own look: its body colour and its edge colour, as the view draws them.
    let view = Tokens::view();
    let to_screen = |p: [f64; 2]| image.min + vec2(p[0] as f32 * step, p[1] as f32 * step);
    let painter = ui.painter_at(outer);
    let shared: Shared = ui.ctx().data_mut(|d| d.get_temp_mut_or_default::<Shared>(egui::Id::unique("body_gizmo_cache")).clone());
    let (hovered, handles, names) = shared
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
                colours: [t.navigator_face, t.navigator_hover, view.body, view.viewport_top],
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
            // The body's edges in the navigator's border colour; the hovered face's in the accent, on top.
            let of_hovered = |e: usize| face.is_some_and(|f| mesh.edge_faces.get(e).is_some_and(|fs| fs.contains(&f)));
            for lit in [false, true] {
                for (_, a, b) in cache.raster.edges.iter().filter(|(e, _, _)| of_hovered(*e) == lit) {
                    let edge = match texture {
                        NavigatorTexture::Themed => t.navigator_border,
                        // Idle, the model's edges fade into the view with its faces.
                        NavigatorTexture::Modeled if active => view.body_edge,
                        NavigatorTexture::Modeled => view.body_edge.lerp_to_gamma(view.viewport_top, 0.55),
                    };
                    painter.line_segment([to_screen(*a), to_screen(*b)], Stroke::new(1.0, if lit { hover_outline } else { edge }));
                }
            }
            let w = cache.raster.side;
            let handles: Vec<(u32, Pos2)> = cache.faces.iter().map(|(f, i)| (*f, to_screen([(i % w) as f64 + 0.5, (i / w) as f64 + 0.5]))).collect();
            let names = if want_faces { face_names(&cache.raster, &cam, image) } else { Vec::new() };
            Some((tri, handles, names))
        })
        .unwrap_or((None, Vec::new(), Vec::new()));
    for (face, at) in &handles {
        crate::scenario::publish_handle(&format!("body_gizmo:face:{face}"), *at);
    }
    crate::scenario::publish_count("body_gizmo:faces", handles.len() as f64);
    crate::scenario::publish_count("axis_view_highlights", f64::from(u8::from(hovered.is_some())));
    // The names of the faces facing the eye, drawn over the body: laid out once to place them,
    // the hovered one again in the accent, a face near edge-on fading out, each name warped into
    // its face's projected axes so it turns with the body the way the cube's labels turned with
    // the cube. A click looks from that face, the one camera path every navigator click takes,
    // and takes the click over the face under the name.
    let named: Vec<_> = names
        .into_iter()
        .map(|s| {
            let galley = painter.layout_no_wrap(
                solidcraft_ui_text::tr(s.id).to_owned(),
                FontId::proportional(t.font_caption),
                t.navigator_text.gamma_multiply(s.opacity),
            );
            let size = galley.size();
            // However wide a translation runs, the name stays inside the navigator's box.
            let (lo, hi) = (outer.min + size * 0.5, outer.max - size * 0.5);
            let at = pos2(s.at.x.clamp(lo.x.min(hi.x), hi.x.max(lo.x)), s.at.y.clamp(lo.y.min(hi.y), hi.y.max(lo.y)));
            (s, galley, Rect::from_center_size(at, size))
        })
        .collect();
    let hovered_name = if orbiting { None } else { hover.and_then(|p| named.iter().position(|(_, _, rect)| rect.contains(p))) };
    for (i, (s, galley, rect)) in named.iter().enumerate() {
        let galley = if hovered_name == Some(i) {
            painter.layout_no_wrap(solidcraft_ui_text::tr(s.id).to_owned(), FontId::proportional(t.font_caption), t.accent.gamma_multiply(s.opacity))
        } else {
            galley.clone()
        };
        crate::scenario::publish_handle(&format!("body_gizmo:name:{}", s.name), rect.center());
        warped_text(&painter, rect.center(), s.axes, galley, t.navigator_text);
    }
    crate::scenario::publish_count("body_gizmo:names", named.len() as f64);
    let mut name_clicked = None;
    if let Some((s, _, _)) = hovered_name.and_then(|i| named.get(i)) {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if ui.input(|i| i.pointer.primary_clicked()) {
            name_clicked = Some(s.dir);
        }
    }
    if let Some(tri) = hovered {
        crate::scenario::publish_count("body_gizmo:hovered", f64::from(mesh.tri_face.get(tri).copied().unwrap_or_default()));
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
        if name_clicked.is_none()
            && ui.input(|i| i.pointer.primary_clicked())
            && let Some(normal) = shape.face_normal(tri)
        {
            navigator_controls::navigate(app, None, Some(app.cam.to_frame(normal)));
        }
    }
    if let Some(dir) = name_clicked {
        navigator_controls::navigate(app, None, Some(dir));
    }
    if want_axes {
        // The world axes beside the body, as the view cube's triad was: X, Y and Z as the camera
        // poses them, in the axis colours, named at their tips.
        let origin = c + vec2(-60.0, 52.0) * scale;
        let (right, up, _) = cam.basis();
        for (d, colour, (letter, handle)) in [(Vec3::X, t.axis_x, ("X", "x")), (Vec3::Y, t.axis_y, ("Y", "y")), (Vec3::Z, t.axis_z, ("Z", "z"))] {
            let tip = origin + vec2((d.dot(right) * 17.0) as f32, -(d.dot(up) * 17.0) as f32) * scale;
            painter.line_segment([origin, tip], Stroke::new(1.5, colour));
            painter.text(tip, Align2::CENTER_CENTER, letter, FontId::proportional(t.font_triad), colour);
            crate::scenario::publish_handle(&format!("body_gizmo:axis:{handle}"), tip);
        }
    }
    if shown.arrows {
        navigator_controls::arrows(app, ui, &painter, c, scale, active, t);
    }
    if shown.home {
        navigator_controls::home(app, ui, &painter, c, scale, active, t);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_viewport::{StandardView, UpAxis};

    fn boxed() -> crate::scenario::Harness {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length": 40, "width": 20, "height": 10})).unwrap();
        h.frames(2);
        h
    }

    fn body_name(h: &crate::scenario::Harness) -> String {
        h.app.session.world_state().unwrap().bodies[0].name.clone()
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
        assert!(crate::scenario::handle_at("navigator:control:left").is_some(), "arrows by default");
        assert!(crate::scenario::handle_at("navigator:control:home").is_some(), "Home by default");
        h.app.run("view.navigator", json!({"arrows": false, "home": false})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("navigator:control:left").is_none(), "arrows off");
        assert!(crate::scenario::handle_at("navigator:control:home").is_none(), "Home off");
        h.app.run("view.navigator", json!({"arrows": true, "home": true})).unwrap();
        h.frames(2);
        let p = crate::scenario::handle_at("navigator:control:clockwise").expect("the roll arrows are there");
        let camera = h.app.cam;
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!((h.app.cam.roll - camera.roll - std::f64::consts::FRAC_PI_2).abs() < 1e-6);
        let p = crate::scenario::handle_at("navigator:control:home").expect("Home is there");
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        assert!(h.app.cam.back().dist(Camera::default().back()) < 1e-6, "Home turns to the home view");
        assert!(crate::scenario::handle_at("navigator:control:fit").is_none(), "Fit is not offered");
    }

    #[test]
    fn modeled_texture_paints_the_body_in_its_own_colour_and_themed_in_the_theme_colours() {
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
            colours: [t.navigator_face, t.navigator_hover, t.body, t.viewport_top],
            lift: t.hover_face_lift,
        };
        assert_ne!(t.body_edge, t.navigator_border, "the two textures' edges can be told apart");
        // A hovered face: the navigator's hover colours when themed, the view's mouse-over when modeled.
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
        let shade = f32::from(themed.r()) / f32::from(t.navigator_face.r().max(1));
        assert!((0.7..=1.01).contains(&shade), "the navigator's face colour, shaded: {themed:?}");
        // The gizmo draws without a frame failing in both textures.
        custom(&mut h);
        for texture in ["modeled", "themed"] {
            h.app.run("view.navigator", json!({"texture": texture})).unwrap();
            h.frames(2);
            assert!(crate::scenario::count_of("body_gizmo:faces").is_some_and(|n| n >= 3.0), "{texture}");
        }
    }

    #[test]
    fn custom_without_a_body_draws_the_signed_axes() {
        let mut h = boxed();
        h.call("ui.prefs", json!({"navigator_style": "custom"}));
        h.frames(2);
        assert!(crate::scenario::handle_at("axis_view:z").is_some());
        assert_eq!(crate::scenario::count_of("body_gizmo:faces"), None);
    }

    #[test]
    fn the_axes_and_face_names_follow_their_settings_and_turn_the_view() {
        let mut h = boxed();
        custom(&mut h);
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(2);
        // The iso view looks from the front-right-top corner: those three names show.
        for name in ["front", "right", "top"] {
            assert!(crate::scenario::handle_at(&format!("body_gizmo:name:{name}")).is_some(), "{name} faces the eye at the iso view");
        }
        for name in ["back", "left", "bottom"] {
            assert!(crate::scenario::handle_at(&format!("body_gizmo:name:{name}")).is_none(), "{name} faces away");
        }
        assert_eq!(crate::scenario::count_of("body_gizmo:names"), Some(3.0));
        for letter in ["x", "y", "z"] {
            assert!(crate::scenario::handle_at(&format!("body_gizmo:axis:{letter}")).is_some(), "the axes show by default");
        }
        // A name's click looks from its face, the same way a face's click does.
        let p = crate::scenario::handle_at("body_gizmo:name:front").expect("the front name is there");
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        h.frames(2);
        assert!(h.app.cam.back().dist(-Vec3::Y) < 1e-6, "the front view looks from -Y: {:?}", h.app.cam.back());
        // Both settings turn off with the command, and back on.
        h.app.run("view.navigator", json!({"axes": false, "faces": false})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("body_gizmo:name:front").is_none(), "the names are off");
        assert!(crate::scenario::handle_at("body_gizmo:axis:z").is_none(), "the axes are off");
        h.app.run("view.navigator", json!({"axes": true, "faces": true})).unwrap();
        h.frames(2);
        assert!(crate::scenario::handle_at("body_gizmo:name:front").is_some(), "the names are back");
        assert!(crate::scenario::handle_at("body_gizmo:axis:z").is_some(), "the axes are back");
    }

    #[test]
    fn face_names_sit_on_the_faces_the_eye_sees() {
        let h = boxed();
        let shape = Shape::of_body(&h.app.session, &body_name(&h)).unwrap();
        let mesh = &shape.mesh;
        let cam = Camera::default();
        let r = raster(mesh, &cam, 128);
        let image = Rect::from_min_size(Pos2::ZERO, vec2(r.side as f32, r.side as f32));
        let names = face_names(&r, &cam, image);
        let step = image.width() / r.side as f32;
        let (right, up, _) = cam.basis();
        assert_eq!(names.len(), 3, "the three faces toward the eye are named");
        for s in &names {
            assert!(s.opacity > 0.9, "the default view is well clear of edge-on: {}", s.opacity);
            // The name sits on the body, with body beyond it along its direction: on the face,
            // not at the rim's last sample, and not off the body.
            let at = (s.at - image.min) / step;
            let (x, y) = (at.x as usize, at.y as usize);
            assert!(r.at(x, y).is_some(), "{}'s name sits on the body", s.name);
            let world = cam.from_frame(s.dir);
            let (sx, sy) = (world.dot(right), -world.dot(up));
            let len = sx.hypot(sy);
            let (dx, dy) = ((sx / len) as f32, (sy / len) as f32);
            let (mut px, mut py) = (at.x as f64, at.y as f64);
            let mut on = 0_u32;
            while on < 4 * r.side as u32 && r.at(px as usize, py as usize).is_some() {
                on += 1;
                px += dx as f64;
                py += dy as f64;
            }
            assert!(on >= 3, "{}'s name has body beyond it, not the rim's last sample: {on}", s.name);
            // The box's own face centre is also the invisible minimum cube's for that face,
            // and v7 names the face there exactly (#19303: the mean could sit off-centre on
            // a filleted body; on the sharp box the two agree).
            let i = FACES.iter().position(|(_, _, _, n, _)| *n == s.name).expect("a face");
            let cube = image.min + vec2(r.cube[i].0 as f32 * step, r.cube[i].1 as f32 * step);
            assert!(s.at.distance(cube) < 1e-4, "{}'s name sits at the cube's face centre: {:?} vs {cube:?}", s.name, s.at);
        }
    }

    #[test]
    fn a_fillet_rounded_bodys_names_sit_at_the_faces_centres_at_angled_views() {
        // The bug (#19303): on a filleted or rounded body the mean of the face's samples
        // sits off the face's centre at angled views, pulled toward the edge that faces the
        // eye more squarely, so the name read off-centre on the face. The v7 rule names
        // every face the body lets reach its cube centre exactly there, so the name sits at
        // the face's centre whatever the view.
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.box", json!({"length": 40, "width": 40, "height": 40, "center": [0, 0, 0]})).unwrap();
        let name = h.app.session.world_state().unwrap().bodies[0].name.clone();
        // A fillet on the top perimeter: the rim is rounded, the four faces skirt it. One
        // point mid-way along each top edge (a point at a corner, where 3 edges meet, is
        // refused); tangent_chain is on by default, so the whole perimeter rounds.
        h.app.run("solid.fillet", json!({"edges": [[0, 20, 20], [20, 0, 20], [0, -20, 20], [-20, 0, 20]], "radius": 6.0, "body": name})).unwrap();
        h.frames(2);
        h.app.run("view.navigator", json!({"style": "custom", "body": name})).unwrap();
        let shape = Shape::of_body(&h.app.session, &name).unwrap();
        let mesh = &shape.mesh;
        for (yaw, pitch) in [(-35.0_f64.to_radians(), 15.0), (-60.0, 45.0), (-20.0, 25.0)] {
            let mut cam = Camera { up: UpAxis::Z, ..Default::default() };
            cam.yaw = yaw;
            cam.pitch = pitch;
            let r = raster(mesh, &cam, 128);
            let image = Rect::from_min_size(Pos2::ZERO, vec2(r.side as f32, r.side as f32));
            let names = face_names(&r, &cam, image);
            let step = image.width() / r.side as f32;
            assert!(!names.is_empty(), "some faces face the eye at yaw {yaw:?} pitch {pitch:?}");
            for s in &names {
                let i = FACES.iter().position(|(_, _, _, n, _)| *n == s.name).expect("a face");
                let cube = image.min + vec2(r.cube[i].0 as f32 * step, r.cube[i].1 as f32 * step);
                assert!(
                    s.at.distance(cube) < 1e-4,
                    "{}'s name sits at the face's centre, not off toward an edge (yaw {yaw:?}, pitch {pitch:?}): {:?} vs {cube:?}",
                    s.name,
                    s.at
                );
                let at = (s.at - image.min) / step;
                assert!(r.at(at.x as usize, at.y as usize).is_some(), "{}'s name sits on the body", s.name);
            }
        }
    }

    /// A torus mesh of ring `ring` and tube `tube`, wound outward as the engine's meshes are.
    fn torus(ring: f64, tube: f64) -> Mesh {
        let (major, minor) = (48, 16);
        let mut mesh = Mesh::default();
        for i in 0..major {
            let (c, s) = (std::f64::consts::TAU * i as f64 / major as f64).sin_cos();
            for j in 0..minor {
                let (u, v) = (std::f64::consts::TAU * j as f64 / minor as f64).sin_cos();
                mesh.positions.push(Vec3::new((ring + tube * u) * c, (ring + tube * u) * s, tube * v));
                mesh.normals.push(Vec3::new(u * c, u * s, v));
            }
        }
        let at = |i: usize, j: usize| ((i % major) * minor + j % minor) as u32;
        for i in 0..major {
            for j in 0..minor {
                mesh.triangles.push([at(i, j), at(i + 1, j), at(i + 1, j + 1)]);
                mesh.triangles.push([at(i, j), at(i + 1, j + 1), at(i, j + 1)]);
            }
        }
        mesh
    }

    #[test]
    fn a_ringed_bodys_names_sit_on_the_invisible_cubes_faces() {
        // A torus seen from the iso view shows its top as a ring whose mean is the hole, and
        // its front and right means fall between the outer band and the inner wall the eye
        // sees through the hole — the means gather where the names used to sit on one
        // another. The names go to the invisible minimum cube's face centres instead, apart
        // as the removed cube's labels were, one to a face.
        let mesh = torus(40.0, 8.0);
        let cam = Camera::default();
        let r = raster(&mesh, &cam, 128);
        let image = Rect::from_min_size(Pos2::ZERO, vec2(r.side as f32, r.side as f32));
        let names = face_names(&r, &cam, image);
        let step = image.width() / r.side as f32;
        assert_eq!(names.len(), 3, "the three faces toward the eye are named");
        let anchor = |name: &str| {
            let s = names.iter().find(|s| s.name == name).unwrap_or_else(|| panic!("{name} faces the eye"));
            let i = FACES.iter().position(|(_, _, _, n, _)| *n == name).expect("a face");
            let cube = image.min + vec2(r.cube[i].0 as f32 * step, r.cube[i].1 as f32 * step);
            assert!(s.at.distance(cube) < 1e-4, "{}'s name sits at the invisible cube's face centre, not at the ring's mean: {:?}", name, s.at);
            s.at
        };
        let (top, front, right) = (anchor("top"), anchor("front"), anchor("right"));
        for (a, b, names) in [(top, front, "top and front"), (top, right, "top and right"), (front, right, "front and right")] {
            assert!(a.distance(b) >= 25.0 * step, "the {names} names sit apart, not one on another");
        }
    }

    #[test]
    fn a_cylinders_face_names_sit_on_it_and_look_from_there() {
        let mut h = crate::scenario::Harness::new();
        h.app.preferences.look_at_anim = false;
        h.app.run("solid.cylinder", json!({"radius": 10, "height": 30})).unwrap();
        h.frames(2);
        let name = body_name(&h);
        h.app.run("view.navigator", json!({"style": "custom", "body": name})).unwrap();
        h.call("ui.view", json!({"view": "iso"}));
        h.frames(2);
        // A cylinder has no flat face on its right, but its curved face faces the eye there:
        // the name sits on what the eye sees of it, and still turns the view to look from it.
        let p = crate::scenario::handle_at("body_gizmo:name:right").expect("the right name is there");
        h.call("ui.click", json!({"x": p.x, "y": p.y}));
        h.frames(2);
        assert!(h.app.cam.back().dist(Vec3::X) < 1e-6, "the right view looks from +X: {:?}", h.app.cam.back());
    }

    #[test]
    fn face_names_are_set_in_the_faces_axes_and_turn_with_the_view() {
        let h = boxed();
        let shape = Shape::of_body(&h.app.session, &body_name(&h)).unwrap();
        let mesh = &shape.mesh;
        let image_of = |r: &Raster| Rect::from_min_size(Pos2::ZERO, vec2(r.side as f32, r.side as f32));
        let mut cam = Camera { up: UpAxis::Z, ..Default::default() };
        // Seen straight on, a name reads upright: its text axes are the screen's, as the cube's
        // label sat upright on the face it faced.
        cam.set_view(StandardView::Front);
        let r = raster(mesh, &cam, 128);
        let names = face_names(&r, &cam, image_of(&r));
        let front = names.iter().find(|s| s.name == "front").expect("the front faces the eye");
        assert!(front.axes.0.x > 0.99 && front.axes.0.y.abs() < 0.02, "front's text runs along the screen: {:?}", front.axes.0);
        assert!(front.axes.1.y > 0.99 && front.axes.1.x.abs() < 0.02, "front's text runs down the screen: {:?}", front.axes.1);
        // From above, the top name reads with the front edge at its bottom, as the cube's TOP
        // label did: its text runs up the screen toward the back edge.
        cam.set_view(StandardView::Top);
        let r = raster(mesh, &cam, 128);
        let names = face_names(&r, &cam, image_of(&r));
        let top = names.iter().find(|s| s.name == "top").expect("the top faces the eye");
        assert!(top.axes.0.x > 0.99 && top.axes.0.y.abs() < 0.02, "top's text runs along the screen: {:?}", top.axes.0);
        assert!(top.axes.1.y > 0.99 && top.axes.1.x.abs() < 0.02, "top's text runs down the screen, front edge low: {:?}", top.axes.1);
        // Roll the view a quarter turn: the name turns by the roll and keeps its angle to the
        // body, the way the cube's label stayed on its face.
        cam.set_view(StandardView::Front);
        cam.roll += std::f64::consts::FRAC_PI_4;
        let r = raster(mesh, &cam, 128);
        let names = face_names(&r, &cam, image_of(&r));
        let rolled = names.iter().find(|s| s.name == "front").expect("the front still faces the eye");
        let turn = (rolled.axes.0.y.atan2(rolled.axes.0.x) - front.axes.0.y.atan2(front.axes.0.x)).abs();
        assert!((turn - std::f32::consts::FRAC_PI_4).abs() < 0.02, "the name turned with the roll: {turn}");
    }
}
