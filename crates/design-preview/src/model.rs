//! A design's saved display meshes (`cache/preview.json` and `cache/preview.bin`), ready to draw,
//! and the orbit camera a file browser turns them with.
//!
//! A save with the display cache keeps each body's evaluated display mesh
//! (docs/design-container.md, "Provisional saved display"). This reads it without the kernel:
//! the manifest's body slices, each an `SCMESH01` (positions and triangles) or `SCMESH02` (also a
//! normal per triangle corner) blob, bounded as the writer bounds them. The result is one flat
//! list of triangle corners centred on the model, which a GPU draws in a single call. The mesh is
//! for looking at only: nothing here measures, picks or edits.

use std::path::Path;

use serde::Deserialize;

use crate::{open, part};

/// The display cache's format tag (`PREVIEW_VERSION` in crates/io/src/design.rs).
const FORMAT: &str = "solidcraft-preview/1";
const MAX_MANIFEST: u64 = 64 << 10;
/// The writer's bounds: bytes, bodies and triangles of the whole cache.
const MAX_DATA: u64 = 8 << 20;
const MAX_BODIES: usize = 1024;
const MAX_TRIANGLES: usize = 100_000;
/// The largest coordinate a mesh blob may hold, in millimetres (docs/design-container.md).
const MAX_COORDINATE: f64 = 1e9;
/// A body without a colour of its own is drawn in this neutral blue-grey.
const DEFAULT_COLOR: [f32; 3] = [0.62, 0.66, 0.72];

/// One triangle corner as a GPU reads it: three tightly packed `f32` triples (MSL `packed_float3`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Vertex {
    /// Millimetres from the model's centre.
    pub position: [f32; 3],
    pub normal: [f32; 3],
    /// Linear RGB, 0 to 1.
    pub color: [f32; 3],
}

/// The saved display meshes of a design, centred on the middle of their bounding box.
#[derive(Debug, Clone, PartialEq)]
pub struct Model {
    /// Three corners per triangle.
    pub vertices: Vec<Vertex>,
    /// Half the bounding box's diagonal, in millimetres (never zero).
    pub radius: f32,
}

/// The saved display meshes of the design at `path`, if it keeps a readable display cache.
pub fn model(path: &Path) -> Option<Model> {
    let mut archive = open(path)?;
    let manifest = part(&mut archive, "cache/preview.json", MAX_MANIFEST)?;
    let data = part(&mut archive, "cache/preview.bin", MAX_DATA)?;
    decode(&manifest, &data)
}

/// The model the display cache's `manifest` and `data` describe.
pub fn decode(manifest: &[u8], data: &[u8]) -> Option<Model> {
    let manifest: Manifest = serde_json::from_slice(manifest).ok()?;
    // The build that wrote it doesn't matter here: an older mesh still shows the design's shape.
    if manifest.format != FORMAT || manifest.meshes.is_empty() || manifest.meshes.len() > MAX_BODIES {
        return None;
    }
    let mut bodies = Vec::with_capacity(manifest.meshes.len());
    let mut end = 0_usize;
    let mut triangles = 0_usize;
    for entry in &manifest.meshes {
        if entry.offset != end {
            return None;
        }
        end = end.checked_add(entry.length)?;
        let mesh = Mesh::decode(data.get(entry.offset..end)?)?;
        triangles = triangles.checked_add(mesh.triangles.len() / 3)?;
        if triangles > MAX_TRIANGLES {
            return None;
        }
        let color = entry.color.filter(|c| c.iter().all(|v| (0.0..=1.0).contains(v))).unwrap_or(DEFAULT_COLOR);
        bodies.push((mesh, color));
    }
    let (mut low, mut high) = ([f64::MAX; 3], [f64::MIN; 3]);
    for (mesh, _) in &bodies {
        for p in mesh.positions.as_chunks::<3>().0 {
            for axis in 0..3 {
                low[axis] = low[axis].min(p[axis]);
                high[axis] = high[axis].max(p[axis]);
            }
        }
    }
    if triangles == 0 {
        return None;
    }
    let center: [f64; 3] = std::array::from_fn(|axis| (low[axis] + high[axis]) * 0.5);
    let radius = (0..3).map(|axis| (high[axis] - low[axis]) * 0.5).map(|h| h * h).sum::<f64>().sqrt().max(1e-3);
    let mut vertices = Vec::with_capacity(triangles * 3);
    for (mesh, color) in &bodies {
        mesh.corners(center, *color, &mut vertices);
    }
    Some(Model { vertices, radius: radius as f32 })
}

/// One body's mesh blob, checked as the app's reader checks it.
struct Mesh {
    positions: Vec<f64>,
    triangles: Vec<u32>,
    /// One normal per triangle corner, or none.
    normals: Vec<f64>,
}

impl Mesh {
    fn decode(bytes: &[u8]) -> Option<Mesh> {
        let shaded = match bytes.get(..8)? {
            b"SCMESH01" => false,
            b"SCMESH02" => true,
            _ => return None,
        };
        let count = |at: usize| bytes.get(at..at + 8).and_then(|b| b.try_into().ok()).map(u64::from_le_bytes).and_then(|n| usize::try_from(n).ok());
        let (positions, triangles) = (count(8)?, count(16)?);
        let normals = if shaded { count(24)? } else { 0 };
        let header = if shaded { 32 } else { 24 };
        let middle = positions.checked_mul(8)?.checked_add(header)?;
        let normal_start = triangles.checked_mul(4)?.checked_add(middle)?;
        let total = normals.checked_mul(8)?.checked_add(normal_start)?;
        if total != bytes.len()
            || !positions.is_multiple_of(3)
            || !triangles.is_multiple_of(3)
            || (normals != 0 && normals != triangles.checked_mul(3)?)
        {
            return None;
        }
        let f64s = |range: std::ops::Range<usize>| -> Option<Vec<f64>> {
            Some(bytes.get(range)?.as_chunks::<8>().0.iter().map(|&b| f64::from_le_bytes(b)).collect())
        };
        let mesh = Mesh {
            positions: f64s(header..middle)?,
            triangles: bytes.get(middle..normal_start)?.as_chunks::<4>().0.iter().map(|&b| u32::from_le_bytes(b)).collect(),
            normals: f64s(normal_start..total)?,
        };
        let vertex_count = mesh.positions.len() / 3;
        let fine = mesh.positions.iter().all(|v| v.is_finite() && v.abs() <= MAX_COORDINATE)
            && mesh.triangles.iter().all(|&i| (i as usize) < vertex_count)
            && mesh.normals.iter().all(|v| v.is_finite());
        fine.then_some(mesh)
    }

    /// Push this mesh's triangle corners, moved by `-center`, onto `out`.
    fn corners(&self, center: [f64; 3], color: [f32; 3], out: &mut Vec<Vertex>) {
        let at = |i: u32| -> [f64; 3] {
            let i = i as usize * 3;
            std::array::from_fn(|axis| self.positions.get(i + axis).copied().unwrap_or(0.0))
        };
        for (t, corners) in self.triangles.as_chunks::<3>().0.iter().enumerate() {
            let [a, b, c] = [at(corners[0]), at(corners[1]), at(corners[2])];
            let face = unit(cross(sub(b, a), sub(c, a))).unwrap_or([0.0, 0.0, 1.0]);
            for (k, p) in [a, b, c].into_iter().enumerate() {
                let n = t * 9 + k * 3;
                let normal = self.normals.get(n..n + 3).and_then(|n| unit([n[0], n[1], n[2]])).unwrap_or(face);
                out.push(Vertex { position: std::array::from_fn(|axis| (p[axis] - center[axis]) as f32), normal: normal.map(|v| v as f32), color });
            }
        }
    }
}

fn sub(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] - b[0], a[1] - b[1], a[2] - b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}

fn unit(v: [f64; 3]) -> Option<[f64; 3]> {
    let length = v[0].hypot(v[1]).hypot(v[2]);
    (length.is_finite() && length > 1e-12).then(|| v.map(|x| x / length))
}

/// The display cache's manifest (`encode_preview` in crates/io/src/design.rs); the rest is skipped.
#[derive(Deserialize)]
struct Manifest {
    #[serde(default)]
    format: String,
    #[serde(default)]
    meshes: Vec<Entry>,
}

#[derive(Deserialize)]
struct Entry {
    offset: usize,
    length: usize,
    #[serde(default)]
    color: Option<[f32; 3]>,
}

/// How a file browser looks at a model: turned with the pointer as a ball under it (an arcball),
/// panned across the view and zoomed, with an orthographic projection as the app's default camera.
/// It starts at the app's isometric Home view, Z up.
///
/// A drag turns the model about every axis: from the middle of the view it tips the model the way
/// the pointer goes, near the edge it turns it about the line of sight. The turn depends only on
/// where the drag started and where the pointer is now, so going back to the start of a drag
/// brings the model back to where it was. Panning shifts the point at the middle of the view in
/// model space, so zooming and orbiting afterwards stay centred on the region the user panned to.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Orbit {
    /// The view's right, up and back (towards the eye) directions in model space, orthonormal.
    pub axes: [[f32; 3]; 3],
    /// The point in model space at the middle of the view, in units of the model's radius.
    pub target: [f32; 3],
    /// 1 fits the model in the view; larger is closer.
    pub zoom: f32,
}

/// Where a drag started: the view then, and the point on the ball under the pointer.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Grab {
    axes: [[f32; 3]; 3],
    from: [f32; 3],
}

/// Where a pan drag started: the target then, and the pointer's place in the view.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pan {
    target: [f32; 3],
    from: [f32; 2],
}

impl Default for Orbit {
    /// The isometric view, as the saved picture and the app's Home view.
    fn default() -> Self {
        let (yaw, pitch) = (-std::f32::consts::FRAC_PI_4, 0.615_479_7_f32);
        let (sy, cy) = yaw.sin_cos();
        let (sp, cp) = pitch.sin_cos();
        let back = [-sy * cp, -cy * cp, sp];
        let right = [cy, -sy, 0.0];
        Orbit { axes: [right, ball::cross(back, right), back], target: [0.0; 3], zoom: 1.0 }
    }
}

/// The arcball's geometry, in single precision as the GPU takes it.
mod ball {
    pub fn cross(a: [f32; 3], b: [f32; 3]) -> [f32; 3] {
        [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
    }

    pub fn dot(a: [f32; 3], b: [f32; 3]) -> f32 {
        a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
    }

    /// `v` scaled to length 1, or `None` when it has no direction.
    pub fn unit(v: [f32; 3]) -> Option<[f32; 3]> {
        let length = dot(v, v).sqrt();
        (length.is_finite() && length > 1e-6).then(|| v.map(|c| c / length))
    }

    /// The point on the ball under view point `x`, `y` (points from the bottom left) of a view
    /// `width` by `height`, as a unit direction in the view's frame (right, up, towards the eye).
    /// The ball fills the shorter side; outside it a sheet that meets it smoothly takes over
    /// (Bell's trackball), so a drag around the edge turns about the line of sight.
    pub fn point(x: f32, y: f32, width: f32, height: f32) -> Option<[f32; 3]> {
        let size = width.min(height);
        if !(size.is_finite() && size > 0.0 && x.is_finite() && y.is_finite()) {
            return None;
        }
        let (px, py) = ((2.0 * x - width) / size, (2.0 * y - height) / size);
        let d = px * px + py * py;
        let pz = if d <= 0.5 { (1.0 - d).sqrt() } else { 0.5 / d.sqrt() };
        unit([px, py, pz])
    }
}

/// The matrices a vertex shader needs, column-major: `clip` takes a model point to Metal's clip
/// space (depth 0 to 1, nearer is smaller), `view` turns a normal into the view's frame. The
/// layout is the shader's (two `float4x4`).
#[repr(C)]
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Matrices {
    pub clip: [f32; 16],
    pub view: [f32; 16],
}

/// How much more the model turns than the ball under the pointer would.
const TURN_GAIN: f32 = 2.0;
/// How far `target` may move from the model's middle, in units of the model's radius.
const MAX_PAN: f32 = 2.0;

impl Orbit {
    /// Start a drag at view point `x`, `y` (points from the bottom left) of a view `width` by
    /// `height`. `None` for a point or size that isn't one.
    pub fn grab(&self, x: f32, y: f32, width: f32, height: f32) -> Option<Grab> {
        Some(Grab { axes: self.axes, from: ball::point(x, y, width, height)? })
    }

    /// Turn the model so the ball point the drag started on lies under the pointer at `x`, `y`.
    pub fn drag_to(&mut self, grab: &Grab, x: f32, y: f32, width: f32, height: f32) {
        let Some(to) = ball::point(x, y, width, height) else { return };
        let Some(axis) = ball::unit(ball::cross(grab.from, to)) else {
            // Back at the start (or exactly opposite, which the sheet never reaches).
            self.axes = grab.axes;
            return;
        };
        // Twice the ball's own angle, so a drag across the view turns the model a full half
        // turn: the ball alone feels sluggish beside the app's viewport.
        let angle = TURN_GAIN * ball::dot(grab.from, to).clamp(-1.0, 1.0).acos();
        // The turn in the view's frame (Rodrigues), applied to the view's axes at the start:
        // each row of the result is a combination of the starting rows.
        let (s, c) = angle.sin_cos();
        let turn: [[f32; 3]; 3] = std::array::from_fn(|i| {
            std::array::from_fn(|j| {
                let identity = if i == j { c } else { 0.0 };
                let skew = match (i, j) {
                    (0, 1) => -axis[2],
                    (0, 2) => axis[1],
                    (1, 0) => axis[2],
                    (1, 2) => -axis[0],
                    (2, 0) => -axis[1],
                    (2, 1) => axis[0],
                    _ => 0.0,
                };
                identity + s * skew + (1.0 - c) * axis[i] * axis[j]
            })
        });
        // A model point's view coordinates are its dot products with the axes; turning those by
        // `turn` makes new axis k the sum over i of turn[k][i] times old axis i.
        let rows: [[f32; 3]; 3] = std::array::from_fn(|k| std::array::from_fn(|j| (0..3).map(|i| turn[k][i] * grab.axes[i][j]).sum()));
        // Square them up again so rounding never skews the view.
        let Some(back) = ball::unit(rows[2]) else { return };
        let Some(right) = ball::unit(std::array::from_fn(|i| rows[0][i] - back[i] * ball::dot(rows[0], back))) else { return };
        self.axes = [right, ball::cross(back, right), back];
    }

    /// Start panning at view point `x`, `y` (points from the bottom left) of a view `width` by
    /// `height`. `None` for a point or size that isn't one.
    pub fn pan_start(&self, x: f32, y: f32, width: f32, height: f32) -> Option<Pan> {
        let valid_size = width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0;
        if !(valid_size && x.is_finite() && y.is_finite() && self.target.iter().all(|v| v.is_finite())) {
            return None;
        }
        Some(Pan { target: self.target, from: [x, y] })
    }

    /// Shift the view so the point under the pointer at the start of `pan` stays under `x`, `y`.
    pub fn pan_to(&mut self, pan: &Pan, x: f32, y: f32, width: f32, height: f32) {
        let valid_size = width.is_finite() && height.is_finite() && width > 0.0 && height > 0.0;
        if !(valid_size && x.is_finite() && y.is_finite() && self.zoom.is_finite() && self.zoom > 0.0) {
            return;
        }
        // The visible span along the shorter side is 2 * 1.1 / zoom model radii.
        let scale = 2.2 / (self.zoom * width.min(height));
        if !scale.is_finite() {
            return;
        }
        let (dx, dy) = (x - pan.from[0], y - pan.from[1]);
        let [right, up, _] = self.axes;
        let raw: [f32; 3] = std::array::from_fn(|i| pan.target[i] - (right[i] * dx + up[i] * dy) * scale);
        let length = ball::dot(raw, raw).sqrt();
        if !length.is_finite() {
            return;
        }
        self.target = if length > MAX_PAN { raw.map(|v| v * (MAX_PAN / length)) } else { raw };
    }

    /// Zoom by `factor` (above 1 is closer), within 1/20 and 50 times the fitted view.
    pub fn zoom_by(&mut self, factor: f32) {
        if factor.is_finite() && factor > 0.0 {
            self.zoom = (self.zoom * factor).clamp(0.05, 50.0);
        }
    }

    /// Return to the starting isometric view, centred and fitted.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// The matrices for a model of `radius` in a view `aspect` (width / height) wide.
    pub fn matrices(&self, radius: f32, aspect: f32) -> Matrices {
        let aspect = if aspect.is_finite() && aspect > 0.0 { aspect } else { 1.0 };
        let radius = if radius.is_finite() && radius > 0.0 { radius } else { 1.0 };
        let zoom = if self.zoom.is_finite() && self.zoom > 0.0 { self.zoom } else { 1.0 };
        let target = if self.target.iter().all(|v| v.is_finite()) { self.target } else { [0.0; 3] };
        let [right, up, back] = self.axes;
        // Half the visible height: the model's sphere fits the shorter side, with a margin.
        let half = radius * 1.1 / (zoom * aspect.min(1.0));
        let (x, y) = (1.0 / (half * aspect), 1.0 / half);
        let depth = 1.0 / (radius * 2.2);
        let rows = [right.map(|v| v * x), up.map(|v| v * y), back.map(|v| -v * depth)];
        let mut clip = [0.0; 16];
        let mut view = [0.0; 16];
        for (row, (scaled, axis)) in rows.iter().zip([right, up, back]).enumerate() {
            for col in 0..3 {
                clip[col * 4 + row] = scaled[col];
                view[col * 4 + row] = axis[col];
            }
        }
        // Keep depth centred on the model's origin so panning never clips against near or far.
        clip[12] = -ball::dot(rows[0], target) * radius;
        clip[13] = -ball::dot(rows[1], target) * radius;
        clip[14] = 0.5;
        clip[15] = 1.0;
        view[15] = 1.0;
        Matrices { clip, view }
    }
}

#[cfg(test)]
#[path = "model_tests.rs"]
mod tests;
