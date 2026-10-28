//! Incremental product rasterization. Every phase yields between bounded work units.
//!
//! One rasteriser draws both the product image ([`ImageTask`]: the product camera, one tile, then
//! a PNG) and pictures ([`PictureTask`]: the product camera or an orthographic view frame, any
//! pixel windows, each handed back as straight-alpha sRGB rows). It prepares once (appearances,
//! projected triangles, glass sorted back to front) and then draws each tile.

use std::collections::BTreeMap;

use serde::Serialize;
use solidcraft_geom::pixel_grid::{MAX_SIDE_PX, PixelGrid};
use solidcraft_geom::{Plane, Vec2, Vec3};

use super::{CancelToken, ErrorCode, ImageRequest, JobError, Phase, Progress, Result};
use crate::{Camera, FaceAppearance, Mat4, ProductScene, RenderFormat, RenderMode, RenderOperation, RenderRequest};

#[cfg(test)]
#[path = "image_tests.rs"]
mod tests;

pub const MAX_TRIANGLES: u64 = 262_144;
pub const MAX_GEOMETRY_BYTES: u64 = 128 * 1024 * 1024;
pub const MAX_JOB_BYTES: u64 = 384 * 1024 * 1024;
pub const MAX_PIXEL_TESTS: u64 = 128_000_000;
const MAX_STEP: usize = 16_384;

#[derive(Clone, Copy, Debug, Serialize)]
pub struct Admission {
    pub triangles: u64,
    pub face_appearances: u64,
    pub geometry_bytes: u64,
    pub pixel_scratch_bytes: u64,
    pub preparation_bytes: u64,
    pub estimated_total_bytes: u64,
    pub encoded_limit: u64,
}

fn limit(message: &str) -> JobError {
    JobError::new(ErrorCode::LimitExceeded, message)
}
fn invalid(message: &str) -> JobError {
    JobError::new(ErrorCode::InvalidScene, message)
}
fn add_bytes(sum: &mut u64, count: usize, size: usize, maximum: u64) -> Result<()> {
    let bytes = (count as u64).checked_mul(size as u64).ok_or_else(|| limit("geometry size overflow"))?;
    *sum = sum.checked_add(bytes).ok_or_else(|| limit("geometry size overflow"))?;
    if *sum > maximum {
        return Err(limit(&format!("retained product geometry exceeds its {} MiB profile limit", maximum / (1024 * 1024))));
    }
    Ok(())
}

/// Conservative admission before allocating pixels or projected-work lists. Source Arcs are
/// counted even when another owner already holds them. Allocator and codec internals are estimates.
/// Includes the CPU raster work estimate and its 384 MiB per-job check.
pub fn admit(scene: &ProductScene, request: &ImageRequest) -> Result<Admission> {
    if request.environment.is_some() || request.textures.is_some() {
        return Err(JobError::new(ErrorCode::InvalidRequest, "captured GPU environments and textures require a registered image executor"));
    }
    match request.integrator {
        super::Integrator::Raster => {
            if request.raytrace.is_some() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "raytrace options require integrator: raytrace"));
            }
            admit_base(scene, request)
        }
        super::Integrator::Raytrace => {
            if scene.has_transmission() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "physical transmission requires the progressive GPU executor"));
            }
            #[cfg(feature = "raytrace-cpu")]
            {
                let defaults = super::RaytraceOptions::default();
                let options = request.raytrace.as_ref().unwrap_or(&defaults);
                options.validate()?;
                crate::raytrace::image::admit(scene, request, options)
            }
            #[cfg(not(feature = "raytrace-cpu"))]
            {
                Err(JobError::new(ErrorCode::HostUnavailable, "CPU raytracing is not compiled in; enable raytrace-cpu"))
            }
        }
    }
}

pub(crate) fn admit_base(scene: &ProductScene, request: &ImageRequest) -> Result<Admission> {
    admit_raster(admit_source(scene, request, MAX_TRIANGLES)?)
}

/// Common camera, dimension and source admission for registered executors, without any CPU
/// raster estimate: the returned admission counts retained source geometry only
/// (`pixel_scratch_bytes` and `preparation_bytes` are zero). Executors budget their own
/// preparation and pixel storage, apply their own job limit, and validate triangle contents
/// before uploading anything to a device.
pub fn admit_geometry(scene: &ProductScene, request: &ImageRequest, max_triangles: u64) -> Result<Admission> {
    Ok(admit_source(scene, request, max_triangles)?.admission)
}

/// A validated source with its geometry accounting, and the builtin raster scratch estimate
/// for the requested dimensions (used only by the CPU estimate).
struct Source {
    admission: Admission,
    bodies: u64,
    raster_scratch_bytes: u64,
}

fn admit_source(scene: &ProductScene, request: &ImageRequest, max_triangles: u64) -> Result<Source> {
    admit_source_with_budget(scene, request, max_triangles, super::CpuBudget::Compact)
}

/// Ray tracing has no raster projected-triangle lists. Keep a conservative
/// pixel/codec allowance, then let the ray tracer charge its actual BVH and film.
#[cfg(feature = "raytrace-cpu")]
pub(crate) fn admit_raytrace_source(scene: &ProductScene, request: &ImageRequest, budget: super::CpuBudget) -> Result<Admission> {
    budget.validate_execution()?;
    budget.validate_image(request.width, request.height, request.raytrace.as_ref().and_then(|o| o.progressive))?;
    let source = admit_source_with_budget(scene, request, budget.limits().max_triangles, budget)?;
    let mut a = source.admission;
    a.pixel_scratch_bytes = source.raster_scratch_bytes;
    a.preparation_bytes = 262_144;
    a.estimated_total_bytes = a
        .geometry_bytes
        .checked_add(a.pixel_scratch_bytes)
        .and_then(|n| n.checked_add(a.preparation_bytes))
        .ok_or_else(|| limit("CPU source memory overflow"))?;
    if a.estimated_total_bytes > budget.limits().max_job_bytes {
        return Err(limit("CPU source exceeds the selected resource profile"));
    }
    Ok(a)
}

fn admit_source_with_budget(scene: &ProductScene, request: &ImageRequest, max_triangles: u64, budget: super::CpuBudget) -> Result<Source> {
    let limits = budget.limits();
    let size = (request.camera.is_valid(), request.width, request.height);
    admit_source_with_limits(scene, size, max_triangles, limits.max_bodies, limits.max_geometry_bytes, budget.image_limits(false))
}

/// A registered executor may request a larger finite source envelope without
/// changing raster/legacy admission. Actual device allocations remain its job.
pub fn admit_geometry_with_limits(
    scene: &ProductScene,
    request: &ImageRequest,
    max_triangles: u64,
    max_bodies: usize,
    max_geometry_bytes: u64,
    image_limits: crate::RenderLimits,
) -> Result<Admission> {
    if max_triangles == 0
        || max_triangles > super::MAX_CPU_TRIANGLES as u64
        || max_bodies == 0
        || max_bodies > super::MAX_CPU_BODIES
        || max_geometry_bytes == 0
        || max_geometry_bytes > 4 * 1024 * 1024 * 1024
        || image_limits.max_dimension > 16_384
        || image_limits.max_pixels > 67_108_864
        || image_limits.max_scratch_bytes > super::NATIVE_CPU_POOL_BYTES
    {
        return Err(limit("registered source envelope exceeds implementation bounds"));
    }
    let size = (request.camera.is_valid(), request.width, request.height);
    Ok(admit_source_with_limits(scene, size, max_triangles, max_bodies, max_geometry_bytes, image_limits)?.admission)
}

/// `(valid, width, height)`: whether the view is valid, and the image (or tile) size whose pixel
/// scratch is estimated.
fn admit_source_with_limits(
    scene: &ProductScene,
    (valid, width, height): (bool, u32, u32),
    max_triangles: u64,
    max_bodies: usize,
    max_geometry_bytes: u64,
    image_limits: crate::RenderLimits,
) -> Result<Source> {
    if !valid {
        return Err(JobError::new(ErrorCode::InvalidRequest, "invalid image camera"));
    }
    if scene.meshes.len() > max_bodies {
        return Err(limit("too many product bodies"));
    }
    let mut backend = crate::builtin_backend();
    for executor in &mut backend.executors {
        executor.limits = image_limits;
    }
    let resolved = crate::resolve_request(
        &[backend],
        &RenderRequest {
            backend: None,
            mode: RenderMode::Preview,
            operation: RenderOperation::Image,
            width: u64::from(width),
            height: u64::from(height),
            format: Some(RenderFormat::Png),
            features: Vec::new(),
            policy: Default::default(),
        },
    )
    .map_err(|e| JobError::new(ErrorCode::InvalidRequest, e.to_string()))?;
    let (mut triangles, mut faces, mut geometry, mut polylines) = (0u64, 0u64, 0u64, 0u64);
    add_bytes(&mut geometry, scene.meshes.capacity(), std::mem::size_of::<crate::ProductMesh>(), max_geometry_bytes)?;
    for body in &scene.meshes {
        let m = &body.mesh;
        triangles = triangles.checked_add(m.triangles.len() as u64).ok_or_else(|| limit("triangle count overflow"))?;
        faces = faces.checked_add(body.faces.len() as u64).ok_or_else(|| limit("appearance count overflow"))?;
        polylines = polylines.checked_add(m.edges.len() as u64 + m.edge_faces.len() as u64).ok_or_else(|| limit("edge count overflow"))?;
        if triangles > max_triangles || faces > max_triangles || polylines > max_triangles {
            return Err(limit("product image exceeds its triangle/appearance/polyline admission cap"));
        }
        add_bytes(&mut geometry, m.positions.capacity(), std::mem::size_of::<Vec3>(), max_geometry_bytes)?;
        add_bytes(&mut geometry, m.normals.capacity(), std::mem::size_of::<Vec3>(), max_geometry_bytes)?;
        add_bytes(&mut geometry, m.triangles.capacity(), std::mem::size_of::<[u32; 3]>(), max_geometry_bytes)?;
        add_bytes(&mut geometry, m.tri_face.capacity(), std::mem::size_of::<u32>(), max_geometry_bytes)?;
        add_bytes(&mut geometry, body.faces.capacity(), std::mem::size_of::<FaceAppearance>(), max_geometry_bytes)?;
        add_bytes(&mut geometry, body.name.capacity(), 1, max_geometry_bytes)?;
        add_bytes(&mut geometry, m.edges.capacity(), std::mem::size_of::<Vec<Vec3>>(), max_geometry_bytes)?;
        add_bytes(&mut geometry, m.edge_faces.capacity(), std::mem::size_of::<Vec<u32>>(), max_geometry_bytes)?;
        for edge in &m.edges {
            add_bytes(&mut geometry, edge.capacity(), std::mem::size_of::<Vec3>(), max_geometry_bytes)?;
        }
        for edge in &m.edge_faces {
            add_bytes(&mut geometry, edge.capacity(), std::mem::size_of::<u32>(), max_geometry_bytes)?;
        }
        for count in [m.seams.capacity(), m.smooth_edges.capacity(), m.sharp_edges.capacity()] {
            add_bytes(&mut geometry, count, 1, max_geometry_bytes)?;
        }
        if body.surface.is_some() && body.transmission.is_none() && body.opacity != 1.0 {
            return Err(invalid("opaque PBR requires opacity 1"));
        }
        if !body.opacity.is_finite() || !(0.0..=1.0).contains(&body.opacity) {
            return Err(invalid("invalid body opacity"));
        }
    }
    let encoded_limit = u64::from(width) * u64::from(height) * 8 + 65_536;
    Ok(Source {
        admission: Admission {
            triangles,
            face_appearances: faces,
            geometry_bytes: geometry,
            pixel_scratch_bytes: 0,
            preparation_bytes: 0,
            estimated_total_bytes: geometry,
            encoded_limit,
        },
        bodies: scene.meshes.len() as u64,
        raster_scratch_bytes: resolved.estimated_scratch_bytes,
    })
}

/// The CPU raster scratch/preparation estimate and its 384 MiB per-job check.
fn admit_raster(source: Source) -> Result<Admission> {
    let mut a = source.admission;
    // Three bounded 16-byte lists, a conservative map-node allowance, and codec overhead.
    let preparation = a
        .triangles
        .checked_mul(48)
        .and_then(|n| a.face_appearances.checked_mul(160).and_then(|f| n.checked_add(f)))
        .and_then(|n| n.checked_add(source.bodies * 128 + 262_144))
        .ok_or_else(|| limit("work list size overflow"))?;
    let total = a
        .geometry_bytes
        .checked_add(preparation)
        .and_then(|n| n.checked_add(source.raster_scratch_bytes))
        .ok_or_else(|| limit("image memory overflow"))?;
    if total > MAX_JOB_BYTES {
        return Err(limit("product job exceeds its 384 MiB admission estimate"));
    }
    a.pixel_scratch_bytes = source.raster_scratch_bytes;
    a.preparation_bytes = preparation;
    a.estimated_total_bytes = total;
    Ok(a)
}

fn reserve<T>(count: usize) -> Result<Vec<T>> {
    let mut v = Vec::new();
    v.try_reserve_exact(count).map_err(|_| JobError::new(ErrorCode::AllocationFailed, "could not reserve render work memory"))?;
    Ok(v)
}

/// How a picture's pixels are sampled.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Supersample {
    /// One sample, at the pixel's centre.
    One,
    /// 2 × 2 samples, a quarter and three quarters of the way across the pixel, averaged in
    /// linear light (the product image's).
    #[default]
    Two,
}

impl Supersample {
    fn factor(self) -> usize {
        match self {
            Self::One => 1,
            Self::Two => 2,
        }
    }
}

/// What a body is in a picture.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BodyRole {
    /// Drawn in its own colours.
    #[default]
    Painted,
    /// Hides what lies behind it and draws nothing itself (a reference part that hides the model
    /// while only its lines are drawn).
    DepthOnly,
}

/// An orthographic view frame: a point `p` lies on paper at `basis.to_local(p) · scale + shift`
/// (millimetres, paper y up), seen from the side `basis`'s normal (x × y) points to.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrthoFrame {
    pub basis: Plane,
    /// Paper millimetres per model millimetre.
    pub scale: f64,
    /// Where the basis's own origin lies on paper.
    pub shift: Vec2,
}

impl OrthoFrame {
    /// Where `p` lies on paper.
    pub fn to_paper(&self, p: Vec3) -> Vec2 {
        self.basis.to_local(p) * self.scale + self.shift
    }
}

/// What a picture shows and how many pixels it has.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Projection {
    /// The product camera's view, `width × height` pixels: what [`ImageTask`] draws.
    Camera { camera: Camera, width: u32, height: u32 },
    /// An orthographic view frame, on the pixel grid of a paper rectangle.
    Ortho { frame: OrthoFrame, grid: PixelGrid },
}

impl Projection {
    fn size(&self) -> (u32, u32) {
        match self {
            Self::Camera { width, height, .. } => (*width, *height),
            Self::Ortho { grid, .. } => (grid.width, grid.height),
        }
    }

    /// The clip-space map onto a canvas of `ss` samples per pixel each way, and the eye.
    fn view(&self, scene: &ProductScene, ss: usize) -> Result<View> {
        let (width, height) = self.size();
        if width == 0 || height == 0 || width > MAX_SIDE_PX || height > MAX_SIDE_PX {
            return Err(limit(&format!("a picture is 1 to {MAX_SIDE_PX} px on a side")));
        }
        match *self {
            Self::Camera { camera, .. } => {
                if !camera.is_valid() {
                    return Err(JobError::new(ErrorCode::InvalidRequest, "invalid image camera"));
                }
                let radius = scene.bounds.diagonal().max(10.0) + scene.bounds.center().dist(camera.target);
                if !radius.is_finite() {
                    return Err(invalid("invalid product bounds"));
                }
                let (sw, sh) = (width as usize * ss, height as usize * ss);
                Ok(View {
                    vp: camera.view_proj(sw as f64 / sh as f64, radius),
                    back: camera.back(),
                    eye: (camera.fov > 0.0).then(|| camera.eye()),
                    reference: camera.target,
                })
            }
            Self::Ortho { frame, grid } => {
                let request = |message: &str| JobError::new(ErrorCode::InvalidRequest, message);
                solidcraft_geom::section::validate_plane(&frame.basis).map_err(|_| request("an orthographic frame needs an orthonormal basis"))?;
                if !frame.scale.is_finite() || frame.scale <= 0.0 || !frame.shift.is_finite() {
                    return Err(request("an orthographic frame needs a finite scale above 0 and a finite shift"));
                }
                if ![grid.x0, grid.y0, grid.pitch].iter().all(|v| v.is_finite()) || grid.pitch <= 0.0 {
                    return Err(request("a picture's pixel grid needs a finite position and pixel size"));
                }
                // Depth runs from the nearest corner of the design's box (0) to the farthest (1).
                let (o, n) = (frame.basis.origin, frame.basis.normal());
                let b = &scene.bounds;
                let (mut near, mut far) = (f64::INFINITY, f64::NEG_INFINITY);
                for corner in 0..8 {
                    let pick = |bit: usize, lo: f64, hi: f64| if corner & bit == 0 { lo } else { hi };
                    let p = Vec3::new(pick(1, b.min.x, b.max.x), pick(2, b.min.y, b.max.y), pick(4, b.min.z, b.max.z));
                    let depth = -(p - o).dot(n);
                    (near, far) = (near.min(depth), far.max(depth));
                }
                let range = if far - near > 1e-9 { far - near } else { 1.0 };
                let near = if near.is_finite() { near } else { 0.0 };
                if !range.is_finite() {
                    return Err(invalid("invalid product bounds"));
                }
                // Paper x and y onto -1..1 across the grid (y up, as clip space), depth onto 0..1.
                let (bx, by) = (frame.basis.x, frame.basis.y);
                let (kx, ky) = (2.0 * frame.scale / (f64::from(width) * grid.pitch), 2.0 * frame.scale / (f64::from(height) * grid.pitch));
                let cx = kx * -o.dot(bx) + 2.0 * (frame.shift.x - grid.x0) / (f64::from(width) * grid.pitch) - 1.0;
                let cy = ky * -o.dot(by) + 2.0 * (frame.shift.y - grid.y0) / (f64::from(height) * grid.pitch) - 1.0;
                let (bx, by, nv) = ([bx.x, bx.y, bx.z], [by.x, by.y, by.z], [n.x, n.y, n.z]);
                let column = |i: usize| [kx * bx[i], ky * by[i], -nv[i] / range, 0.0];
                let vp = Mat4([column(0), column(1), column(2), [cx, cy, (o.dot(n) - near) / range, 1.0]]);
                if vp.0.iter().flatten().any(|v| !v.is_finite()) {
                    return Err(request("an orthographic frame too large for its pixel grid"));
                }
                Ok(View { vp, back: n, eye: None, reference: o })
            }
        }
    }
}

/// A window of a picture's pixels: columns `x0..x1` and rows `y0..y1`, row 0 at the top.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PixelWindow {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}

impl PixelWindow {
    fn width(&self) -> usize {
        self.x1.saturating_sub(self.x0) as usize
    }
    fn height(&self) -> usize {
        self.y1.saturating_sub(self.y0) as usize
    }
}

/// At most this many tiles in one picture.
pub const MAX_TILES: usize = 65_536;

/// A picture of a product scene, drawn tile by tile.
#[derive(Clone, Debug, PartialEq)]
pub struct PictureRequest {
    pub projection: Projection,
    /// The windows drawn, in order: each is handed back as its rows ([`PicturePoll::Tile`]).
    pub tiles: Vec<PixelWindow>,
    pub supersample: Supersample,
    /// One role per body of the scene, in order; empty paints every body.
    pub roles: Vec<BodyRole>,
    /// `None`: a transparent background.
    pub background: Option<[u8; 3]>,
}

/// The checks a picture passes before anything is allocated, and what it costs: the prepared
/// lists as a product image's, the pixel scratch of its largest tile.
pub fn admit_picture(scene: &ProductScene, request: &PictureRequest) -> Result<Admission> {
    let (width, height) = request.projection.size();
    if request.tiles.is_empty() || request.tiles.len() > MAX_TILES {
        return Err(JobError::new(ErrorCode::InvalidRequest, "a picture is drawn in 1 to 65536 tiles"));
    }
    if request.tiles.iter().any(|t| t.x0 >= t.x1 || t.y0 >= t.y1 || t.x1 > width || t.y1 > height) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "a tile is an empty window or reaches outside its picture"));
    }
    if !request.roles.is_empty() && request.roles.len() != scene.meshes.len() {
        return Err(JobError::new(ErrorCode::InvalidRequest, "a picture gives every body a role, or none"));
    }
    let widest = request.tiles.iter().map(|t| t.x1 - t.x0).max().unwrap_or(1);
    let tallest = request.tiles.iter().map(|t| t.y1 - t.y0).max().unwrap_or(1);
    let budget = super::CpuBudget::Compact;
    let limits = budget.limits();
    admit_raster(admit_source_with_limits(
        scene,
        (true, widest, tallest),
        MAX_TRIANGLES,
        limits.max_bodies,
        limits.max_geometry_bytes,
        budget.image_limits(false),
    )?)
}

/// The map onto the canvas, and where the eye is.
struct View {
    vp: Mat4,
    /// Toward the eye (an orthographic view's direction).
    back: Vec3,
    /// A perspective view's eye.
    eye: Option<Vec3>,
    /// A point the glass's back-to-front depth is measured from.
    reference: Vec3,
}

#[derive(Clone, Copy, Default)]
struct Draw {
    body: u32,
    triangle: u32,
    depth: f64,
}

impl Draw {
    fn compare(&self, other: &Self) -> std::cmp::Ordering {
        self.depth.total_cmp(&other.depth).then_with(|| (self.body, self.triangle).cmp(&(other.body, other.triangle)))
    }
}

struct Triangle {
    points: [[f64; 3]; 3],
    world: [Vec3; 3],
    normals: [Vec3; 3],
    inv_w: [f64; 3],
    colour: [f64; 3],
    opacity: f64,
    surface: Option<crate::surface::OpaqueSurface>,
    /// Off for a depth-only body.
    paint: bool,
    centre_depth: f64,
    area: f64,
    /// Whole-canvas sample bounds `[x0, x1, y0, y1]` (inclusive), narrowed to the tile drawn.
    bounds: [usize; 4],
    x: usize,
    y: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Stage {
    Looks,
    Prepare,
    Sort,
    Clear,
    Opaque,
    Glass,
    Downsample,
    Encode,
    Done,
}

pub enum TaskPoll {
    Pending(Progress),
    Complete(Vec<u8>),
}

/// A step of a [`PictureTask`].
pub enum PicturePoll {
    Pending(Progress),
    /// One tile, in the order asked: its straight-alpha sRGB RGBA rows, top row first.
    Tile(PixelWindow, Vec<u8>),
    /// Every tile has been handed back.
    Complete,
}

/// The CPU rasteriser both the product image and pictures run: it prepares the scene once
/// (appearances, projected triangles, glass sorted back to front), then draws each tile on a
/// canvas of its own, sampling at the same whole-picture positions whichever tile a sample falls
/// in, so tiles put side by side are the picture drawn whole.
/// Transparency sorts triangle centres and blends in linear space; it is not refraction.
struct Raster {
    scene: ProductScene,
    admission: Admission,
    cancel: CancelToken,
    stage: Stage,
    view: View,
    ss: usize,
    sw: usize,
    sh: usize,
    roles: Vec<BodyRole>,
    background: Option<[u8; 3]>,
    tiles: Vec<PixelWindow>,
    tile: usize,
    looks: Vec<BTreeMap<u32, FaceAppearance>>,
    body: usize,
    item: usize,
    prepare_done: u64,
    opaque: Vec<Draw>,
    glass: Vec<Draw>,
    scratch: Vec<Draw>,
    sort_width: usize,
    sort_block: usize,
    sort_left: usize,
    sort_right: usize,
    sort_mid: usize,
    sort_end: usize,
    sort_dst: usize,
    pixels: Vec<[u8; 4]>,
    depth: Vec<f32>,
    pixel_tests: u64,
    max_pixel_tests: u64,
    pixel_done: u64,
    draw: usize,
    triangle: Option<Triangle>,
    output: Vec<u8>,
}

enum RasterPoll {
    Pending,
    Tile(PixelWindow, Vec<u8>),
}

impl Raster {
    #[allow(clippy::too_many_arguments)]
    fn new(
        scene: ProductScene,
        projection: &Projection,
        supersample: Supersample,
        roles: Vec<BodyRole>,
        background: Option<[u8; 3]>,
        tiles: Vec<PixelWindow>,
        admission: Admission,
        max_pixel_tests: u64,
        cancel: CancelToken,
    ) -> Result<Self> {
        let ss = supersample.factor();
        let view = projection.view(&scene, ss)?;
        let (width, height) = projection.size();
        let (sw, sh) = (width as usize * ss, height as usize * ss);
        let samples = tiles.iter().map(|t| t.width() * t.height()).max().unwrap_or(0) * ss * ss;
        let pixels = tiles.iter().map(|t| t.width() * t.height()).max().unwrap_or(0);
        let looks = reserve(scene.meshes.len())?;
        let (opaque, glass, scratch) =
            (reserve(admission.triangles as usize)?, reserve(admission.triangles as usize)?, reserve(admission.triangles as usize)?);
        Ok(Self {
            looks,
            opaque,
            glass,
            scratch,
            pixels: reserve(samples)?,
            depth: reserve(samples)?,
            output: reserve(pixels * 4)?,
            scene,
            admission,
            cancel,
            stage: Stage::Looks,
            view,
            ss,
            sw,
            sh,
            roles,
            background,
            tiles,
            tile: 0,
            body: 0,
            item: 0,
            prepare_done: 0,
            sort_width: 1,
            sort_block: 0,
            sort_left: 0,
            sort_right: 0,
            sort_mid: 0,
            sort_end: 0,
            sort_dst: 0,
            pixel_tests: 0,
            max_pixel_tests,
            pixel_done: 0,
            draw: 0,
            triangle: None,
        })
    }

    /// A sample nothing has been drawn on.
    fn clear(&self) -> [u8; 4] {
        self.background.map_or([0; 4], |[r, g, b]| [r, g, b, 255])
    }

    fn window(&self) -> Result<PixelWindow> {
        self.tiles.get(self.tile).copied().ok_or_else(|| invalid("missing picture tile"))
    }

    fn progress(&self) -> Progress {
        match self.stage {
            Stage::Looks | Stage::Prepare | Stage::Sort => Progress {
                phase: Phase::Preparing,
                done: self.prepare_done,
                total: self.admission.face_appearances + self.admission.triangles + self.scene.meshes.len() as u64 + self.glass.len() as u64 * 19,
            },
            Stage::Clear | Stage::Opaque | Stage::Glass | Stage::Downsample | Stage::Encode => {
                // In u64: a large picture's canvas outgrows a 32-bit usize.
                let samples = self.sw as u64 * self.sh as u64;
                let total = self.pixel_tests + samples + samples / (self.ss * self.ss) as u64;
                // Overlapping tiles can draw more than the picture once.
                Progress { phase: Phase::Running, done: self.pixel_done.min(total), total }
            }
            Stage::Done => Progress { phase: Phase::Completed, done: 1, total: 1 },
        }
    }

    /// At most `units` simple units, or up to the end of a tile.
    fn step(&mut self, units: usize) -> Result<RasterPoll> {
        for _ in 0..units.clamp(1, MAX_STEP) {
            self.cancel.check()?;
            match self.stage {
                Stage::Looks => self.look_step()?,
                Stage::Prepare => self.prepare_step()?,
                Stage::Sort => self.sort_step()?,
                Stage::Clear => self.clear_step()?,
                Stage::Opaque | Stage::Glass => self.draw_step()?,
                Stage::Downsample => {
                    if let Some(tile) = self.downsample_step()? {
                        return Ok(tile);
                    }
                }
                Stage::Encode | Stage::Done => return Err(JobError::new(ErrorCode::NotCompleted, "render result already consumed")),
            }
        }
        Ok(RasterPoll::Pending)
    }

    fn look_step(&mut self) -> Result<()> {
        let Some(body) = self.scene.meshes.get(self.body) else {
            self.body = 0;
            self.item = 0;
            self.stage = Stage::Prepare;
            return Ok(());
        };
        if self.looks.len() == self.body {
            self.looks.push(BTreeMap::new());
        }
        if let Some(face) = body.faces.get(self.item) {
            if !face.opacity.is_finite() || !(0.0..=1.0).contains(&face.opacity) {
                return Err(invalid("invalid face opacity"));
            }
            let looks = self.looks.get_mut(self.body).ok_or_else(|| invalid("missing body appearances"))?;
            // Match the document policy: the last assignment of a face wins.
            looks.insert(face.face, face.clone());
            self.item += 1;
        } else {
            self.body += 1;
            self.item = 0;
        }
        self.prepare_done += 1;
        Ok(())
    }

    fn project(&self, draw: Draw) -> Result<Option<Triangle>> {
        let body = self.scene.meshes.get(draw.body as usize).ok_or_else(|| invalid("missing product body"))?;
        let mesh = &body.mesh;
        let indices = mesh.triangles.get(draw.triangle as usize).ok_or_else(|| invalid("missing product triangle"))?;
        let vertices = indices.map(|i| mesh.positions.get(i as usize).copied());
        let normals = indices.map(|i| mesh.normals.get(i as usize).copied());
        let ([Some(a), Some(b), Some(c)], [Some(na), Some(nb), Some(nc)]) = (vertices, normals) else {
            return Err(invalid("invalid triangle index or missing normal"));
        };
        let world = [a, b, c];
        let normals = [na, nb, nc];
        if world.iter().chain(normals.iter()).any(|v| !v.is_finite()) {
            return Err(invalid("non-finite product triangle"));
        }
        let paint = self.roles.get(draw.body as usize).is_none_or(|role| *role == BodyRole::Painted);
        let look = self.looks.get(draw.body as usize).and_then(|looks| mesh.tri_face.get(draw.triangle as usize).and_then(|face| looks.get(face)));
        // Transmission is physical optical data, never a request to change CPU preview.
        // Keep the existing scalar sorted-alpha shading for glass previews.
        let (colour, opacity, surface) = if !paint {
            // A depth-only body is solid, whatever it is made of, and is never shaded.
            (body.color, 1.0, None)
        } else {
            look.map_or((body.color, body.opacity, body.surface.filter(|_| body.transmission.is_none())), |look| {
                (look.color, look.opacity, look.surface.filter(|_| look.transmission.is_none()))
            })
        };
        if surface.is_some() && opacity != 1.0 {
            return Err(invalid("opaque PBR requires opacity 1"));
        }
        let normals = if surface.is_some() {
            [crate::surface::unit_direction(na), crate::surface::unit_direction(nb), crate::surface::unit_direction(nc)]
                .map(|n| n.map_err(|e| invalid(&e.to_string())))
        } else {
            [Ok(normals[0]), Ok(normals[1]), Ok(normals[2])]
        };
        let [na, nb, nc] = normals;
        let normals = [na?, nb?, nc?];
        if opacity <= 0.0 {
            return Ok(None);
        }
        let clip = world.map(|p| self.view.vp.apply(p));
        if clip.iter().flatten().any(|v| !v.is_finite()) {
            return Err(invalid("non-finite product projection"));
        }
        if clip.iter().any(|p| p[3] <= 1e-9) {
            return Ok(None);
        }
        let inv_w = clip.map(|p| 1.0 / p[3]);
        let points = clip.map(|p| [(p[0] / p[3] + 1.0) * 0.5 * self.sw as f64, (1.0 - p[1] / p[3]) * 0.5 * self.sh as f64, p[2] / p[3]]);
        if points.iter().flatten().any(|v| !v.is_finite()) || points.iter().any(|p| !(p[2] as f32).is_finite()) {
            return Err(invalid("product projection exceeds pixel precision"));
        }
        let [a, b, c] = points;
        let (x0, x1) = (a[0].min(b[0]).min(c[0]).floor().max(0.0), a[0].max(b[0]).max(c[0]).ceil().min(self.sw as f64 - 1.0));
        let (y0, y1) = (a[1].min(b[1]).min(c[1]).floor().max(0.0), a[1].max(b[1]).max(c[1]).ceil().min(self.sh as f64 - 1.0));
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (b[1] - a[1]) * (c[0] - a[0]);
        if !(x0 <= x1 && y0 <= y1) || area.abs() < 1e-12 {
            return Ok(None);
        }
        if !area.is_finite() {
            return Err(invalid("invalid projected area"));
        }
        let centre_depth = (world[0] / 3.0 + world[1] / 3.0 + world[2] / 3.0 - self.view.reference).dot(self.view.back);
        if !centre_depth.is_finite() {
            return Err(invalid("invalid triangle depth"));
        }
        Ok(Some(Triangle {
            points,
            world,
            normals,
            inv_w,
            colour: colour.map(decode),
            opacity,
            surface,
            paint,
            centre_depth,
            area,
            bounds: [x0 as usize, x1 as usize, y0 as usize, y1 as usize],
            x: x0 as usize,
            y: y0 as usize,
        }))
    }

    fn prepare_step(&mut self) -> Result<()> {
        let Some(body) = self.scene.meshes.get(self.body) else {
            self.stage = if self.glass.len() > 1 { Stage::Sort } else { Stage::Clear };
            self.sort_block = 0;
            self.init_merge();
            return Ok(());
        };
        if self.item >= body.mesh.triangles.len() {
            self.body += 1;
            self.item = 0;
            return Ok(());
        }
        let mut draw = Draw { body: self.body as u32, triangle: self.item as u32, depth: 0.0 };
        if let Some(triangle) = self.project(draw)? {
            let [x0, x1, y0, y1] = triangle.bounds;
            let tests = ((x1 - x0 + 1) as u64).checked_mul((y1 - y0 + 1) as u64).ok_or_else(|| limit("pixel work overflow"))?;
            self.pixel_tests = self.pixel_tests.checked_add(tests).ok_or_else(|| limit("pixel work overflow"))?;
            if self.pixel_tests > self.max_pixel_tests {
                return Err(limit(&format!("projected product exceeds {} pixel tests", self.max_pixel_tests)));
            }
            draw.depth = triangle.centre_depth;
            if triangle.opacity >= 1.0 {
                self.opaque.push(draw);
            } else {
                self.glass.push(draw);
                self.scratch.push(draw);
            }
        }
        self.item += 1;
        self.prepare_done += 1;
        Ok(())
    }

    fn init_merge(&mut self) {
        let n = self.glass.len();
        self.sort_mid = (self.sort_block + self.sort_width).min(n);
        self.sort_end = (self.sort_block + 2 * self.sort_width).min(n);
        self.sort_left = self.sort_block;
        self.sort_right = self.sort_mid;
        self.sort_dst = self.sort_block;
    }

    fn sort_step(&mut self) -> Result<()> {
        if self.sort_dst >= self.sort_end {
            self.sort_block += 2 * self.sort_width;
            if self.sort_block >= self.glass.len() {
                std::mem::swap(&mut self.glass, &mut self.scratch);
                self.sort_width *= 2;
                self.sort_block = 0;
                if self.sort_width >= self.glass.len() {
                    self.stage = Stage::Clear;
                    return Ok(());
                }
            }
            self.init_merge();
        }
        let left = self.glass.get(self.sort_left).filter(|_| self.sort_left < self.sort_mid);
        let right = self.glass.get(self.sort_right).filter(|_| self.sort_right < self.sort_end);
        let take_left = match (left, right) {
            (Some(a), Some(b)) => a.compare(b).is_le(),
            (Some(_), None) => true,
            _ => false,
        };
        let source = if take_left {
            let i = self.sort_left;
            self.sort_left += 1;
            i
        } else {
            let i = self.sort_right;
            self.sort_right += 1;
            i
        };
        let draw = self.glass.get(source).copied().ok_or_else(|| invalid("invalid transparent sort source"))?;
        *self.scratch.get_mut(self.sort_dst).ok_or_else(|| invalid("invalid transparent sort destination"))? = draw;
        self.sort_dst += 1;
        self.prepare_done += 1;
        Ok(())
    }

    fn clear_step(&mut self) -> Result<()> {
        let window = self.window()?;
        if self.pixels.len() >= window.width() * window.height() * self.ss * self.ss {
            self.stage = Stage::Opaque;
            self.draw = 0;
            return Ok(());
        }
        self.pixels.push(self.clear());
        self.depth.push(f32::INFINITY);
        self.pixel_done += 1;
        Ok(())
    }

    fn draw_step(&mut self) -> Result<()> {
        let window = self.window()?;
        // The tile's canvas samples, in whole-canvas coordinates.
        let (cx0, cx1) = (window.x0 as usize * self.ss, window.x1 as usize * self.ss - 1);
        let (cy0, cy1) = (window.y0 as usize * self.ss, window.y1 as usize * self.ss - 1);
        if self.triangle.is_none() {
            let list = if self.stage == Stage::Opaque { &self.opaque } else { &self.glass };
            let Some(draw) = list.get(self.draw).copied() else {
                self.stage = if self.stage == Stage::Opaque { Stage::Glass } else { Stage::Downsample };
                self.draw = 0;
                return Ok(());
            };
            // Only the part of the triangle's bounds inside this tile.
            self.triangle = self.project(draw)?.and_then(|mut t| {
                let [x0, x1, y0, y1] = t.bounds;
                let bounds = [x0.max(cx0), x1.min(cx1), y0.max(cy0), y1.min(cy1)];
                (bounds[0] <= bounds[1] && bounds[2] <= bounds[3]).then(|| {
                    (t.bounds, t.x, t.y) = (bounds, bounds[0], bounds[2]);
                    t
                })
            });
            if self.triangle.is_none() {
                self.draw += 1;
            }
            return Ok(());
        }
        let clear = self.clear();
        let triangle = self.triangle.as_mut().ok_or_else(|| invalid("missing raster triangle"))?;
        let tile = (cx0, cy0, cx1 - cx0 + 1);
        raster_pixel(triangle, tile, self.view.back, self.view.eye, clear, &mut self.pixels, &mut self.depth)?;
        triangle.x += 1;
        if triangle.x > triangle.bounds[1] {
            triangle.x = triangle.bounds[0];
            triangle.y += 1;
        }
        if triangle.y > triangle.bounds[3] {
            self.triangle = None;
            self.draw += 1;
        }
        self.pixel_done += 1;
        Ok(())
    }

    /// One output pixel of the tile; the tile's rows once its last pixel is in.
    fn downsample_step(&mut self) -> Result<Option<RasterPoll>> {
        let window = self.window()?;
        let (width, ss) = (window.width(), self.ss);
        let index = self.output.len() / 4;
        if index >= width * window.height() {
            let rows = std::mem::take(&mut self.output);
            self.tile += 1;
            if self.tile < self.tiles.len() {
                self.output = reserve(rows.capacity())?;
                self.pixels.clear();
                self.depth.clear();
                self.stage = Stage::Clear;
            } else {
                // Pixels and prepared geometry are no longer needed.
                self.pixels = Vec::new();
                self.depth = Vec::new();
                self.opaque = Vec::new();
                self.glass = Vec::new();
                self.scratch = Vec::new();
                self.looks = Vec::new();
                self.scene.meshes.clear();
                self.stage = Stage::Done;
            }
            return Ok(Some(RasterPoll::Tile(window, rows)));
        }
        let (x, y) = (index % width, index / width);
        let (mut colour, mut alpha) = ([0.0; 3], 0.0);
        for dy in 0..ss {
            for dx in 0..ss {
                let p = self.pixels.get((y * ss + dy) * width * ss + x * ss + dx).ok_or_else(|| invalid("missing downsample pixel"))?;
                let a = f64::from(p[3]) / 255.0;
                for (sum, channel) in colour.iter_mut().zip(p.iter().take(3)) {
                    *sum += decode(*channel) * a;
                }
                alpha += a;
            }
        }
        let rgb = colour.map(|v| if alpha > 1e-12 { encode(v / alpha) } else { 0 });
        self.output.extend_from_slice(&[rgb[0], rgb[1], rgb[2], (alpha * 255.0 / (ss * ss) as f64).round() as u8]);
        self.pixel_done += 1;
        Ok(None)
    }
}

/// A picture drawn tile by tile ([`PictureRequest`]).
pub struct PictureTask {
    raster: Raster,
}

impl PictureTask {
    pub fn new(scene: ProductScene, request: PictureRequest, cancel: CancelToken) -> Result<Self> {
        cancel.check()?;
        let admission = admit_picture(&scene, &request)?;
        // A picture may be far larger than a product image: it may test each sample as often as
        // the largest product image may.
        let (width, height) = request.projection.size();
        let samples = u64::from(width) * u64::from(height) * (request.supersample.factor() as u64).pow(2);
        let max_tests = MAX_PIXEL_TESTS.max(samples.saturating_mul(8));
        let PictureRequest { projection, tiles, supersample, roles, background } = request;
        Ok(Self { raster: Raster::new(scene, &projection, supersample, roles, background, tiles, admission, max_tests, cancel)? })
    }

    pub fn progress(&self) -> Progress {
        self.raster.progress()
    }

    /// At most 16384 simple units, or up to the end of a tile.
    pub fn step(&mut self, units: usize) -> Result<PicturePoll> {
        if self.raster.stage == Stage::Done {
            return Ok(PicturePoll::Complete);
        }
        Ok(match self.raster.step(units)? {
            RasterPoll::Pending => PicturePoll::Pending(self.raster.progress()),
            RasterPoll::Tile(window, rows) => PicturePoll::Tile(window, rows),
        })
    }
}

/// CPU product renderer with cooperative preparation, sorting, raster and PNG encoding: a
/// [`Raster`] of one whole-picture tile from the product camera, then its PNG.
pub struct ImageTask {
    raster: Raster,
    width: u32,
    height: u32,
    output: Vec<u8>,
    encoder: Option<super::encoding::PngEncoding>,
}

impl ImageTask {
    pub fn new(scene: ProductScene, request: ImageRequest, cancel: CancelToken) -> Result<Self> {
        cancel.check()?;
        if request.integrator != super::Integrator::Raster {
            return Err(JobError::new(ErrorCode::InvalidRequest, "raster task cannot execute a raytrace request"));
        }
        let admission = admit(&scene, &request)?;
        let (width, height) = (request.width, request.height);
        let projection = Projection::Camera { camera: request.camera, width, height };
        let whole = vec![PixelWindow { x0: 0, y0: 0, x1: width, y1: height }];
        let background = (!request.transparent_background).then_some(request.background);
        let raster = Raster::new(scene, &projection, Supersample::Two, Vec::new(), background, whole, admission, MAX_PIXEL_TESTS, cancel)?;
        Ok(Self { raster, width, height, output: Vec::new(), encoder: None })
    }

    #[cfg(test)]
    fn stage(&self) -> Stage {
        self.raster.stage
    }

    pub fn progress(&self) -> Progress {
        match (&self.encoder, self.raster.stage) {
            (Some(encoder), Stage::Encode) => Progress { phase: Phase::Encoding, done: encoder.row() as u64, total: u64::from(self.height) },
            _ => self.raster.progress(),
        }
    }

    /// At most 16384 simple units; encoding additionally yields after at most four rows.
    pub fn step(&mut self, units: usize) -> Result<TaskPoll> {
        if let Some(encoder) = self.encoder.as_mut() {
            for _ in 0..units.clamp(1, 4) {
                self.raster.cancel.check()?;
                if let Some(png) = encoder.step(&self.output)? {
                    self.output = Vec::new();
                    self.encoder = None;
                    self.raster.stage = Stage::Done;
                    return Ok(TaskPoll::Complete(png));
                }
            }
            return Ok(TaskPoll::Pending(self.progress()));
        }
        if self.raster.stage == Stage::Done {
            return Err(JobError::new(ErrorCode::NotCompleted, "render result already consumed"));
        }
        if let RasterPoll::Tile(_, rows) = self.raster.step(units)? {
            self.output = rows;
            let limit = self.raster.admission.encoded_limit;
            self.encoder = Some(super::encoding::PngEncoding::new(self.width, self.height, limit, self.raster.cancel.clone())?);
            // The tile is the whole picture: the raster has handed it over and the PNG is written.
            self.raster.stage = Stage::Encode;
        }
        Ok(TaskPoll::Pending(self.progress()))
    }
}

/// Shades and stores one sample: the triangle's at its cursor `(t.x, t.y)`, in a tile whose
/// canvas starts at whole-canvas sample `(x0, y0)` and is `width` samples wide. A depth-only
/// triangle in front puts back `clear`, so it hides what was drawn behind it in either order.
#[allow(clippy::too_many_arguments)]
fn raster_pixel(
    t: &Triangle,
    (x0, y0, width): (usize, usize, usize),
    back: Vec3,
    eye: Option<Vec3>,
    clear: [u8; 4],
    pixels: &mut [[u8; 4]],
    depth: &mut [f32],
) -> Result<()> {
    let [a, b, c] = t.points;
    // The sample's position from its whole-canvas integer index, so it is the same number in
    // every tile and tiles side by side give the whole picture exactly.
    let (x, y) = (t.x as f64 + 0.5, t.y as f64 + 0.5);
    let w0 = ((b[0] - x) * (c[1] - y) - (b[1] - y) * (c[0] - x)) / t.area;
    let w1 = ((c[0] - x) * (a[1] - y) - (c[1] - y) * (a[0] - x)) / t.area;
    let w2 = 1.0 - w0 - w1;
    // A shared triangle edge belongs to exactly one raster triangle, including glass.
    for (w, mut p, mut q) in [(w0, b, c), (w1, c, a), (w2, a, b)] {
        if w < -1e-9 {
            return Ok(());
        }
        if w.abs() <= 1e-9 {
            if t.area < 0.0 {
                std::mem::swap(&mut p, &mut q);
            }
            let (dx, dy) = (q[0] - p[0], q[1] - p[1]);
            if !(dy < 0.0 || (dy == 0.0 && dx > 0.0)) {
                return Ok(());
            }
        }
    }
    let z = (w0 * a[2] + w1 * b[2] + w2 * c[2]) as f32;
    let index = (t.y - y0) * width + (t.x - x0);
    let old_depth = depth.get_mut(index).ok_or_else(|| invalid("missing pixel depth"))?;
    if z > *old_depth {
        return Ok(());
    }
    if !t.paint {
        *old_depth = z;
        *pixels.get_mut(index).ok_or_else(|| invalid("missing colour pixel"))? = clear;
        return Ok(());
    }
    let weights = [w0 * t.inv_w[0], w1 * t.inv_w[1], w2 * t.inv_w[2]];
    let sum: f64 = weights.iter().sum();
    if !sum.is_finite() || sum <= 1e-12 {
        return Ok(());
    }
    let n = t.normals.iter().zip(weights).fold(Vec3::ZERO, |v, (n, w)| v + *n * (w / sum)).normalized().unwrap_or(Vec3::Z);
    let colour = if let Some(surface) = t.surface {
        let world = t.world.iter().zip(weights).fold(Vec3::ZERO, |p, (v, w)| p + *v * (w / sum));
        let view = eye.map_or(back, |eye| eye - world);
        let radiance = crate::studio::shade(surface, n, view).map_err(|e| invalid(&e.to_string()))?;
        let mapped = crate::studio::tone_map(radiance).map_err(|e| invalid(&e.to_string()))?;
        [mapped.x, mapped.y, mapped.z]
    } else {
        crate::raster::shade(n, back, t.colour)
    };
    if colour.iter().any(|v| !v.is_finite()) {
        return Err(invalid("non-finite shaded colour"));
    }
    let pixel = pixels.get_mut(index).ok_or_else(|| invalid("missing colour pixel"))?;
    if t.opacity >= 1.0 {
        *old_depth = z;
        *pixel = [encode(colour[0]), encode(colour[1]), encode(colour[2]), 255];
    } else {
        let old_alpha = f64::from(pixel[3]) / 255.0;
        let alpha = t.opacity + old_alpha * (1.0 - t.opacity);
        for (out, src) in pixel.iter_mut().take(3).zip(colour) {
            *out = encode((src * t.opacity + decode(*out) * old_alpha * (1.0 - t.opacity)) / alpha);
        }
        pixel[3] = (alpha * 255.0).round() as u8;
    }
    Ok(())
}

fn decode(v: u8) -> f64 {
    let s = f64::from(v) / 255.0;
    if s <= 0.04045 { s / 12.92 } else { ((s + 0.055) / 1.055).powf(2.4) }
}
fn encode(v: f64) -> u8 {
    let l = v.clamp(0.0, 1.0);
    let s = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (s * 255.0).round() as u8
}
