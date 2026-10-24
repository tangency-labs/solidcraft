//! The test model in Preferences > Navigation > 3D mouse: a small boat that moves with the 3D mouse
//! exactly as the view does, so what each setting on that page does can be felt on it. It is a
//! tessellation of a CC0 model (`assets/models/test_boat.bin`, made by
//! `assets/models/make_test_boat.py`; see ATTRIBUTION.md).
//!
//! The boat is drawn as one indexed mesh: the vertices are projected once with a [`Camera`] of its
//! own and shaded smoothly from their normals by one light that follows the camera, coloured from the
//! theme; the triangles facing away are dropped and the rest sorted far to near. The camera takes the same per-frame motion the viewport's
//! camera takes (`navigation::device_camera`), so calibration, reassigned axes, invert, speed and
//! the zoom direction all show. Pan and zoom are held so the boat never leaves its box.

use std::sync::OnceLock;

use egui::{Pos2, Rect, pos2};
use solidcraft_engine::geom::Vec3;
use solidcraft_engine::render::Camera;

use crate::space_mouse::DeviceMotion;
use crate::theme::Tokens;

const DATA: &[u8] = include_bytes!("../../../assets/models/test_boat.bin");

/// The model: triangles in millimetres about its centre. Each vertex has its own normal (smooth
/// where the surface is, split at creases); each triangle has the face normal and centre that the
/// culling and sorting use.
pub(crate) struct Model {
    pub vertices: Vec<Vec3>,
    pub vertex_normals: Vec<Vec3>,
    pub triangles: Vec<[u16; 3]>,
    pub normals: Vec<Vec3>,
    pub centroids: Vec<Vec3>,
    /// The furthest a vertex is from the centre.
    pub radius: f64,
}

/// Bytes of one vertex in the file: its position (i16 x 3) and its normal (i8 x 3).
const VERTEX_BYTES: usize = 9;

/// Reads the file: u32 vertex count, u32 triangle count, f32 centre (3) and half size, then per
/// vertex i16 x 3 and i8 x 3, then the triangles as u16 x 3. `None` for anything that does not add up.
pub(crate) fn parse(data: &[u8]) -> Option<Model> {
    let word = |at: usize| data.get(at..at + 4).map(|b| [b[0], b[1], b[2], b[3]]);
    let (vertex_count, triangle_count) = (u32::from_le_bytes(word(0)?) as usize, u32::from_le_bytes(word(4)?) as usize);
    let half = f64::from(f32::from_le_bytes(word(20)?));
    let (vertices_at, triangles_at) = (24, 24usize.checked_add(vertex_count.checked_mul(VERTEX_BYTES)?)?);
    if data.len() != triangles_at.checked_add(triangle_count.checked_mul(6)?)?
        || !half.is_finite()
        || half <= 0.0
        || vertex_count == 0
        || vertex_count > usize::from(u16::MAX) + 1
    {
        return None;
    }
    let short = |at: usize| data.get(at..at + 2).map(|b| i16::from_le_bytes([b[0], b[1]]));
    // Centred on the origin: the file's own centre is where the model sat, not where it is drawn.
    let vertices: Vec<Vec3> = (0..vertex_count)
        .map(|i| {
            let at = vertices_at + VERTEX_BYTES * i;
            Some(Vec3::new(f64::from(short(at)?), f64::from(short(at + 2)?), f64::from(short(at + 4)?)) * (half / 32767.0))
        })
        .collect::<Option<_>>()?;
    let vertex_normals: Vec<Vec3> = (0..vertex_count)
        .map(|i| {
            let at = vertices_at + VERTEX_BYTES * i + 6;
            let byte = |k: usize| data.get(at + k).map(|&b| f64::from(i8::from_le_bytes([b])));
            Some(Vec3::new(byte(0)?, byte(1)?, byte(2)?).normalized().unwrap_or(Vec3::Z))
        })
        .collect::<Option<_>>()?;
    let triangles: Vec<[u16; 3]> = (0..triangle_count)
        .map(|i| {
            let at = triangles_at + 6 * i;
            let index = |k: usize| data.get(at + 2 * k..at + 2 * k + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
            Some([index(0)?, index(1)?, index(2)?]).filter(|t| t.iter().all(|&v| usize::from(v) < vertex_count))
        })
        .collect::<Option<_>>()?;
    let corner = |t: &[u16; 3], k: usize| vertices[usize::from(t[k])];
    let normals =
        triangles.iter().map(|t| (corner(t, 1) - corner(t, 0)).cross(corner(t, 2) - corner(t, 0)).normalized().unwrap_or(Vec3::Z)).collect();
    let centroids = triangles.iter().map(|t| (corner(t, 0) + corner(t, 1) + corner(t, 2)) * (1.0 / 3.0)).collect();
    let radius = vertices.iter().map(|v| v.len()).fold(0.0, f64::max);
    Some(Model { vertices, vertex_normals, triangles, normals, centroids, radius })
}

/// The built-in model, read once.
pub(crate) fn model() -> Option<&'static Model> {
    static MODEL: OnceLock<Option<Model>> = OnceLock::new();
    MODEL.get_or_init(|| parse(DATA)).as_ref()
}

/// Vertical field of view of the boat's camera.
const FOV: f64 = 0.6;
/// How near and far the boat's camera may be: nearest still shows the whole boat in the box.
const NEAREST: f64 = 3.0;
const FARTHEST: f64 = 12.0;

/// The boat's view: a camera on the boat and when it was last drawn, so a page opened again starts
/// afresh.
#[derive(Clone, Copy, Debug)]
pub struct Boat {
    pub cam: Camera,
    pub seen: f64,
}

impl Boat {
    pub fn new(radius: f64) -> Self {
        Self { cam: Camera { distance: radius * 6.0, fov: FOV, ..Camera::default() }, seen: 0.0 }
    }
}

/// `motion` for `seconds` on the boat's camera in a box `aspect` wide for its height of `height`
/// points, with pan and zoom held so a sphere of `radius` stays inside it.
pub(crate) fn step(
    cam: &mut Camera,
    motion: crate::space_mouse::Motion,
    seconds: f64,
    height: f64,
    radius: f64,
    aspect: f64,
    reverse_zoom: bool,
    free_orbit: bool,
) {
    crate::navigation::device_camera(cam, motion, seconds, height, true, reverse_zoom, free_orbit);
    cam.fov = FOV;
    // Zoom within limits: the nearest still shows the whole boat in the shorter side of the box.
    let shorter = aspect.min(1.0);
    cam.distance = cam.distance.clamp(radius * NEAREST / shorter.max(0.2), radius * FARTHEST);
    // Pan so the boat's centre stays where the whole boat is still inside: within the box less its radius.
    let half = cam.distance * (FOV / 2.0).tan();
    let room = (half * aspect.min(1.0) - radius).max(0.0);
    let off = cam.target.len();
    if off > room && off > 0.0 {
        cam.target = cam.target * (room / off);
    }
    if !cam.is_valid() {
        *cam = Boat::new(radius).cam;
    }
}

/// The boat for one frame: every vertex projected once with its shade (0 to 1, from its own normal,
/// so surfaces are smooth), and the triangles facing the eye as indices, far to near.
pub(crate) struct Frame {
    pub points: Vec<Pos2>,
    pub shades: Vec<f32>,
    pub indices: Vec<u32>,
}

/// Projects `model` through `cam` into `rect`. Pure, for the drawing and for tests.
pub(crate) fn project(model: &Model, cam: &Camera, rect: Rect) -> Frame {
    let aspect = f64::from(rect.width()) / f64::from(rect.height().max(1.0));
    let vp = cam.view_proj(aspect, model.radius * 2.0);
    let (right, up, back) = cam.basis();
    let light = (right * -0.4 + up * 0.6 + back * 0.7).normalized().unwrap_or(back);
    let (width, height) = (f64::from(rect.width()), f64::from(rect.height()));
    let (mut points, mut shades) = (Vec::with_capacity(model.vertices.len()), Vec::with_capacity(model.vertices.len()));
    let mut behind = vec![false; model.vertices.len()];
    for (i, (vertex, normal)) in model.vertices.iter().zip(&model.vertex_normals).enumerate() {
        let c = vp.apply(*vertex);
        behind[i] = c[3] <= 1e-9;
        let w = if behind[i] { 1.0 } else { c[3] };
        points.push(pos2(rect.left() + ((c[0] / w + 1.0) * 0.5 * width) as f32, rect.top() + ((1.0 - c[1] / w) * 0.5 * height) as f32));
        shades.push(normal.dot(light).max(0.0) as f32);
    }
    let eye = cam.eye();
    let mut visible: Vec<(f64, usize)> = (0..model.triangles.len())
        .filter(|&i| model.normals[i].dot(eye - model.centroids[i]) > 0.0 && !model.triangles[i].iter().any(|&v| behind[usize::from(v)]))
        .map(|i| ((eye - model.centroids[i]).len(), i))
        .collect();
    visible.sort_by(|a, b| b.0.total_cmp(&a.0));
    let indices = visible.into_iter().flat_map(|(_, i)| model.triangles[i].map(u32::from)).collect();
    Frame { points, shades, indices }
}

/// Draws the boat in a box of `size` and moves it by this frame's motion. `boat` is the page's state.
pub(crate) fn show(
    ui: &mut egui::Ui,
    boat: &mut Option<Boat>,
    motion: Option<DeviceMotion>,
    reverse_zoom: bool,
    free_orbit: bool,
    size: egui::Vec2,
) -> Option<Rect> {
    let model = model()?;
    let t = Tokens::get();
    let now = ui.input(|i| i.time);
    let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
    // A page opened again (not drawn for a moment) starts with the boat back.
    let mut state = boat.filter(|b| now - b.seen < 0.5).unwrap_or_else(|| Boat::new(model.radius));
    state.seen = now;
    if let Some(motion) = motion {
        let aspect = f64::from(rect.width()) / f64::from(rect.height().max(1.0));
        step(&mut state.cam, motion.motion, motion.seconds, f64::from(rect.height()), model.radius, aspect, reverse_zoom, free_orbit);
    }
    let painter = ui.painter_at(rect);
    painter.rect(rect, t.radius_sm, t.dialog_body, egui::Stroke::new(1.0, t.dialog_rule), egui::StrokeKind::Inside);
    let (dark, lit) = (t.border, t.text);
    let frame = project(model, &state.cam, rect.shrink(1.0));
    let vertices = frame
        .points
        .iter()
        .zip(&frame.shades)
        .map(|(&pos, &shade)| egui::epaint::Vertex { pos, uv: egui::epaint::WHITE_UV, color: dark.lerp_to_gamma(lit, 0.25 + 0.75 * shade) })
        .collect();
    let mesh = egui::Mesh { indices: frame.indices, vertices, texture_id: egui::TextureId::default() };
    painter.add(egui::Shape::mesh(mesh));
    *boat = Some(state);
    Some(rect)
}

/// The boat back where it began.
pub(crate) fn reset(boat: &mut Option<Boat>) {
    *boat = None;
}

#[cfg(test)]
#[path = "test_boat_tests.rs"]
mod tests;
