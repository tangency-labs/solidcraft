//! The viewport's drawing: shaded triangles and screen-space wide lines with a depth buffer,
//! the section cap, surface analysis, reference images and supersampling, as render pipelines on
//! any wgpu device. The desktop and web viewport draws through it (an `egui_wgpu` paint callback
//! hands it the device and the pass); headless pictures draw through it offscreen
//! ([`crate::capture`]). Geometry is uploaded when a scene changes; each frame only the camera
//! uniform changes. A frame that would draw the same picture as the last is blitted from that
//! picture's texture instead, so a still view costs nothing to show again. Where the device runs
//! compute shaders, opaque bodies are drawn straight from their display plans: the plan is
//! evaluated on the GPU into the vertex buffer the pass draws (see [`PlannedBody`]), with nothing
//! built or read back on the CPU.

use std::sync::{Arc, Mutex};

use solidcraft_geom::Vec3;

use crate::pbr;
use wgpu::util::DeviceExt;

mod ao;
mod clouds;
pub use clouds::{CloudFilter, CloudStyle, ColorMode, DOT_SIZE, Dot, FILTERS, GpuCloud, cloud_dot};
#[cfg(not(target_arch = "wasm32"))]
mod msaa_check;
mod oit;
#[cfg(not(target_arch = "wasm32"))]
pub use msaa_check::MsaaCheck;

/// Triangle vertex: position (3 × f32), normal (3 × f32), palette index (u32).
pub const TRI_SIZE: usize = 28;
pub const PBR_SIZE: usize = pbr::VERTEX_SIZE;
/// Line instance: a, b (3 × f32 each), palette index (u32), width in pixels (f32).
pub const LINE_SIZE: usize = 32;
/// Placement of an [`InstancedMesh`]: the rows of its rigid move (3 × 4 f32; a point's x, y and z
/// are each row's dot product with (p, 1)), then its palette index (u32).
pub const INSTANCE_SIZE: usize = 52;

/// An index into a scene's [`Palette`]. Deliberately not convertible from a colour: every
/// producer must go through a palette, and the compiler should say so.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
#[repr(transparent)]
pub struct PaletteIndex(pub u32);

/// How many palette entries fit on one texel row: the shader reads `x = i & 2047`,
/// `y = i >> 11`. A power of two keeps the lookup a shift and a mask.
pub const PALETTE_WIDTH: u32 = 2048;
/// Most palette entries a scene may carry (2048 × 512): the palette is an `R32Uint` texture
/// (`downlevel_defaults`/WebGL2 have no storage buffers in the vertex stage).
pub const MAX_PALETTE: usize = 1 << 20;

/// sRGB RGBA bytes, looked up in the shader by [`PaletteIndex`].
///
/// The first [`Palette::stable_len`] entries are stable for a scene: cached block bytes keep
/// referring to them, so appearance edits only change their values. Entries after that are
/// interned per build for ephemeral geometry (grids, highlights, sketch, previews).
#[derive(Clone, Debug, Default)]
pub struct Palette {
    entries: Vec<[u8; 4]>,
    lookup: std::collections::HashMap<[u8; 4], u32>,
    stable_len: usize,
}

impl Palette {
    /// A palette whose entries are all stable (the model scene's body/face slots).
    pub fn with_stable(entries: Vec<[u8; 4]>) -> Palette {
        Palette { stable_len: entries.len(), entries, lookup: std::collections::HashMap::new() }
    }

    /// Where the stable entries end and the per-build interned ones begin.
    pub fn stable_len(&self) -> usize {
        self.stable_len
    }

    pub fn entries(&self) -> &[[u8; 4]] {
        &self.entries
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// The colour at `i`, if the palette has it.
    pub fn get(&self, i: PaletteIndex) -> Option<[u8; 4]> {
        self.entries.get(i.0 as usize).copied()
    }

    /// Set an entry's colour; `false` when the index is out of range. Stable entries are updated
    /// in place (what appearance and theme edits do); interned entries keep their lookup current.
    pub fn set(&mut self, i: PaletteIndex, rgba: [u8; 4]) -> bool {
        let at = i.0 as usize;
        let ephemeral = at >= self.stable_len;
        let Some(slot) = self.entries.get_mut(at) else { return false };
        if ephemeral {
            self.lookup.remove(slot);
        }
        *slot = rgba;
        if ephemeral {
            self.lookup.insert(rgba, i.0);
        }
        true
    }

    /// A colour's index in the per-build section, appended when it is new. At [`MAX_PALETTE`]
    /// the first entry stands in: far past what any real scene interns, and never a panic.
    pub fn intern(&mut self, rgba: [u8; 4]) -> PaletteIndex {
        if let Some(&i) = self.lookup.get(&rgba) {
            return PaletteIndex(i);
        }
        if self.entries.len() >= MAX_PALETTE {
            return PaletteIndex(0);
        }
        let i = u32::try_from(self.entries.len()).unwrap_or(0);
        self.entries.push(rgba);
        self.lookup.insert(rgba, i);
        PaletteIndex(i)
    }

    /// The palette as the shader's `R32Uint` texels (rgba bytes packed little-endian).
    pub fn texels(&self) -> Vec<u32> {
        self.entries.iter().map(|[r, g, b, a]| u32::from_le_bytes([*r, *g, *b, *a])).collect()
    }
}

/// The width slot of a lifted line ([`GpuScene::lifted_line`]): the top bit set (a plain line's
/// width is a positive `f32`, whose top bit is clear), the width in eighths of a pixel (7 bits,
/// up to 15.9 px), and the two faces' normals in octahedral form (6 bits a component). The
/// shader reads the slot as `u32` so the bits arrive as they are.
pub fn lifted_width(width: f32, normals: [[f32; 3]; 2]) -> u32 {
    let w = if width.is_finite() { (width * 8.0).round().clamp(1.0, 127.0) as u32 } else { 8 };
    let [a, b] = normals.map(octahedral6);
    0x8000_0000 | (w << 24) | (b << 12) | a
}

/// A unit normal in octahedral form, 6 bits a component (12 bits); a zero or non-finite
/// normal is 0, which reads back as "unknown".
fn octahedral6(n: [f32; 3]) -> u32 {
    let l1 = n[0].abs() + n[1].abs() + n[2].abs();
    if !(l1.is_finite() && l1 > 1e-6) {
        return 0;
    }
    let (mut x, mut y) = (n[0] / l1, n[1] / l1);
    if n[2] < 0.0 {
        let (ox, oy) = (x, y);
        x = (1.0 - oy.abs()) * ox.signum();
        y = (1.0 - ox.abs()) * oy.signum();
    }
    // 1..=63 (0 kept for "unknown"): -1 → 1, 1 → 63.
    let q = |v: f32| ((v.clamp(-1.0, 1.0) + 1.0) * 31.0).round() as u32 + 1;
    (q(y).min(63) << 6) | q(x).min(63)
}
/// The shader's `U`: the view-projection matrix, 10 `vec4`s (the tenth carries the ambient
/// occlusion radius), the 8 analysis colours, their flags, the depth direction and the two
/// whole-body highlight colours, the product lights, and the sky (`uniform_bytes`).
const UNIFORM_SIZE: u64 = 416 + 16 + 21 * 64 + 16;

/// A body drawn from its display plan, evaluated on the GPU: the plan, and a palette index per
/// face (the plan's face numbering; faces past the end take the last index).
#[derive(Clone)]
pub struct PlannedBody {
    pub plan: Arc<solidcraft_geom::tess::TessPlan>,
    pub face_colors: Vec<PaletteIndex>,
    /// How the whole body is drawn: opaque with the model, or faded or see-through blended
    /// order-independently (only on a renderer that does that: [`GpuTarget::oit`]).
    pub class: TriClass,
}

/// What the viewport's last frame drew, counted as it was drawn (the performance overlay, #2548).
/// Triangles and line segments count every pass that draws them, so a section's stencil passes
/// count once each; that is the work the GPU did.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct DrawStats {
    pub draw_calls: u32,
    pub triangles: u64,
    pub line_segments: u64,
    /// The frame was drawn supersampled and filtered down.
    pub supersampled: bool,
    /// The device refused the memory for a frame (#7296): the view is drawn at a lower
    /// resolution, or with its see-through surfaces sorted.
    pub lowered: bool,
}

/// What the last scene change uploaded to the GPU, in bytes: what an edit costs beyond the CPU
/// work of building the scene. Counted per scene ([`Renderer::uploads`]).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct UploadStats {
    /// Triangle vertices, index lists and lines: of the blocks and of the scene's own lists, and
    /// the triangles shared by instanced meshes.
    pub geometry: u64,
    /// Placements of instanced meshes.
    pub placements: u64,
    /// Palette rows written.
    pub palette: u64,
    /// Planned bodies' per-vertex palette indices.
    pub planned: u64,
    /// Vertices of planned bodies evaluated again on the GPU (chunks whose plans changed).
    pub evaluated: u64,
}

/// The renderer's last [`DrawStats`], shared with the UI that shows them.
pub type DrawStatsHandle = Arc<Mutex<DrawStats>>;

/// Counts a frame's draw calls as they are made (shared by the drawing closures, hence the cells).
#[derive(Default)]
pub(crate) struct Tally {
    calls: std::cell::Cell<u32>,
    triangles: std::cell::Cell<u64>,
    segments: std::cell::Cell<u64>,
}

impl Tally {
    /// A draw call of `vertices` triangle corners (or indices), `instances` times.
    fn triangles(&self, vertices: u32, instances: u32) {
        self.call();
        self.triangles.set(self.triangles.get().saturating_add(u64::from(vertices / 3) * u64::from(instances)));
    }

    /// A draw call of `n` line segments.
    fn segments(&self, n: u32) {
        self.call();
        self.segments.set(self.segments.get().saturating_add(u64::from(n)));
    }

    /// A draw call that counts neither (a full-screen pass).
    fn call(&self) {
        self.calls.set(self.calls.get().saturating_add(1));
    }

    fn stats(&self) -> DrawStats {
        DrawStats {
            draw_calls: self.calls.get(),
            triangles: self.triangles.get(),
            line_segments: self.segments.get(),
            supersampled: false,
            lowered: false,
        }
    }
}

/// A quick hasher for welding (the keys are coordinates' bits, from the design, not chosen to
/// collide): the standard one made welding a 2 M-triangle scan take half a second.
#[derive(Default)]
struct WeldHasher(u64);

impl std::hash::Hasher for WeldHasher {
    fn finish(&self) -> u64 {
        self.0
    }

    fn write(&mut self, bytes: &[u8]) {
        for b in bytes {
            self.write_u32(u32::from(*b));
        }
    }

    fn write_u32(&mut self, x: u32) {
        self.0 = (self.0.rotate_left(5) ^ u64::from(x)).wrapping_mul(0x51_7c_c1_b7_27_22_0a_95);
    }
}

/// A vertex of [`SharedTris`]: position and normal (3 × f32 each).
pub const SHARED_SIZE: usize = 24;

/// A mesh's triangles as an [`InstancedMesh`] draws them: each vertex once, shared by the
/// triangles that meet there, and the triangles as indices into them, so the GPU transforms each
/// vertex once rather than once per triangle corner.
#[derive(Default)]
pub struct SharedTris {
    /// [`SHARED_SIZE`] bytes per vertex, in the part's own frame.
    pub vertices: Vec<u8>,
    /// Three per triangle.
    pub indices: Vec<u32>,
    /// Every triangle is flat (its corners share one normal, as an STL's facets do): vertices
    /// are shared by position alone, and each fragment takes its triangle's own normal.
    pub flat: bool,
    /// The box around the vertices (min, max), in the part's own frame: a placement whose box
    /// is out of the view is not drawn.
    pub bounds: [[f32; 3]; 2],
}

impl SharedTris {
    /// `mesh`'s triangles with their vertices shared: by position and normal, or by position
    /// alone when every triangle is flat.
    pub fn new(mesh: &solidcraft_geom::Mesh) -> SharedTris {
        let corner = |i: u32| Some((mesh.positions.get(i as usize)?.to_f32(), mesh.normals.get(i as usize)?.to_f32()));
        let bits = |v: [f32; 3]| v.map(|x| (x + 0.0).to_bits());
        let flat = mesh.triangles.iter().all(|t| {
            let [a, b, c] = t.map(|i| corner(i).map(|(_, n)| bits(n)));
            a == b && b == c
        });
        let mut out = SharedTris { flat, ..Default::default() };
        out.indices.reserve_exact(mesh.triangles.len() * 3);
        let mut seen: std::collections::HashMap<[u32; 6], u32, std::hash::BuildHasherDefault<WeldHasher>> =
            std::collections::HashMap::with_capacity_and_hasher(mesh.positions.len() / 2, Default::default());
        for t in &mesh.triangles {
            let [Some(a), Some(b), Some(c)] = t.map(corner) else { continue };
            for (p, n) in [a, b, c] {
                let [x, y, z] = bits(p);
                let [a, b, c] = if flat { [0; 3] } else { bits(n) };
                let next = u32::try_from(out.vertices.len() / SHARED_SIZE).unwrap_or(u32::MAX);
                let index = *seen.entry([x, y, z, a, b, c]).or_insert_with(|| {
                    for v in p.iter().chain(&n) {
                        out.vertices.extend_from_slice(&v.to_le_bytes());
                    }
                    next
                });
                out.indices.push(index);
            }
        }
        out.vertices.shrink_to_fit();
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for v in 0..out.vertices.len() / SHARED_SIZE {
            let (p, _) = out.vertex(v as u32);
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
        }
        out.bounds = [lo, hi];
        out
    }

    /// A simplified copy of a mesh ([`solidcraft_geom::simplify::Level`]): its vertices already
    /// shared, its triangles lit flat (each fragment with its triangle's own normal).
    pub fn from_level(level: &solidcraft_geom::simplify::Level) -> SharedTris {
        let mut out = SharedTris { flat: true, ..Default::default() };
        out.vertices.reserve_exact(level.positions.len() * SHARED_SIZE);
        let mut lo = [f32::INFINITY; 3];
        let mut hi = [f32::NEG_INFINITY; 3];
        for p in &level.positions {
            let p = p.to_f32();
            for k in 0..3 {
                lo[k] = lo[k].min(p[k]);
                hi[k] = hi[k].max(p[k]);
            }
            for v in p.iter().chain(&[0.0f32; 3]) {
                out.vertices.extend_from_slice(&v.to_le_bytes());
            }
        }
        let n = u32::try_from(level.positions.len()).unwrap_or(u32::MAX);
        out.indices = level.triangles.iter().filter(|t| t.iter().all(|i| *i < n)).flatten().copied().collect();
        out.bounds = [lo, hi];
        out
    }

    /// Vertex `i`: position and normal.
    fn vertex(&self, i: u32) -> ([f32; 3], [f32; 3]) {
        let at = i as usize * SHARED_SIZE;
        let f = |k: usize| self.vertices.get(at + 4 * k..at + 4 * k + 4).and_then(|x| x.try_into().ok()).map_or(0.0, f32::from_le_bytes);
        ([f(0), f(1), f(2)], [f(3), f(4), f(5)])
    }

    /// Each triangle's corners (position, and the normal they are lit with: a flat triangle's
    /// own), in the part's frame.
    pub fn corners(&self) -> impl Iterator<Item = [([f32; 3], [f32; 3]); 3]> + '_ {
        self.indices.as_chunks::<3>().0.iter().map(move |t| {
            let c = t.map(|i| self.vertex(i));
            if !self.flat {
                return c;
            }
            let [a, b, d] = c.map(|(p, _)| p);
            let (u, v) = ([b[0] - a[0], b[1] - a[1], b[2] - a[2]], [d[0] - a[0], d[1] - a[1], d[2] - a[2]]);
            let n = [u[1] * v[2] - u[2] * v[1], u[2] * v[0] - u[0] * v[2], u[0] * v[1] - u[1] * v[0]];
            let len = (n[0] * n[0] + n[1] * n[1] + n[2] * n[2]).sqrt();
            let n = if len > 0.0 { n.map(|x| x / len) } else { [0.0; 3] };
            c.map(|(p, _)| (p, n))
        })
    }

    /// The triangles unshared ([`SHARED_SIZE`] bytes per corner, three per triangle): for a mesh
    /// too large for one buffer.
    fn unshared(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(self.indices.len() * SHARED_SIZE);
        for t in self.corners() {
            for (p, n) in t {
                for v in p.iter().chain(&n) {
                    out.extend_from_slice(&v.to_le_bytes());
                }
            }
        }
        out
    }
}

/// How an [`InstancedMesh`]'s placements are drawn: opaque, or see-through (their colour's alpha)
/// as faded context (another component active) or as glass (an appearance's opacity).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum InstanceLook {
    #[default]
    Opaque,
    /// With the translucent triangles ([`GpuScene::trans`]): no depth written.
    Faded,
    /// With the see-through bodies ([`GpuScene::glass`]): far sides, then near sides.
    Glass,
}

/// Triangles drawn at several places: a part placed by many occurrences keeps one copy of its
/// triangles on the GPU, drawn once per placement, however often it is placed.
#[derive(Clone)]
pub struct InstancedMesh {
    /// Names exactly these triangles: while it comes back with the same triangles, their buffers
    /// are kept and only the placements are uploaded again.
    pub key: u64,
    /// The opaque triangles, in the part's own frame.
    pub tris: Arc<SharedTris>,
    /// The placements, [`INSTANCE_SIZE`] bytes each ([`InstancedMesh::place`]).
    pub instances: Vec<u8>,
    /// How its placements are drawn (see-through ones are drawn back to front).
    pub look: InstanceLook,
    /// Coarser copies of the triangles, finest first, each with how far it strays from them (mm,
    /// in the part's frame): a placement small on screen is drawn from the coarsest whose error
    /// stays under [`LOD_PX`] there ([`Batches::cull`]).
    pub levels: Vec<(Arc<SharedTris>, f32)>,
}

impl InstancedMesh {
    /// Add a placement: `rows` of its rigid move (see [`INSTANCE_SIZE`]) and its palette index.
    pub fn place(&mut self, rows: [[f32; 4]; 3], color: PaletteIndex) {
        for v in rows.iter().flatten() {
            self.instances.extend_from_slice(&v.to_le_bytes());
        }
        self.instances.extend_from_slice(&color.0.to_le_bytes());
    }

    /// Its triangles at every placement, as the GPU draws them, appended to `out` (laid out as
    /// [`GpuScene::tris`]).
    pub fn expand_into(&self, out: &mut Vec<u8>) {
        let f = |b: &[u8], i: usize| b.get(i..i + 4).and_then(|x| x.try_into().ok()).map_or(0.0, f32::from_le_bytes);
        for inst in self.instances.as_chunks::<INSTANCE_SIZE>().0 {
            let row = |r: usize| [f(inst, 16 * r), f(inst, 16 * r + 4), f(inst, 16 * r + 8), f(inst, 16 * r + 12)];
            let rows = [row(0), row(1), row(2)];
            let index = inst.get(48..52).and_then(|b| b.try_into().ok()).map_or(0, u32::from_le_bytes);
            for (p, n) in self.tris.corners().flatten() {
                let moved = rows.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3]);
                let turned = rows.map(|r| r[0] * n[0] + r[1] * n[1] + r[2] * n[2]);
                for x in moved.iter().chain(&turned) {
                    out.extend_from_slice(&x.to_le_bytes());
                }
                out.extend_from_slice(&index.to_le_bytes());
            }
        }
    }

    /// How many placements it is drawn at.
    pub fn count(&self) -> usize {
        self.instances.len() / INSTANCE_SIZE
    }
}

/// CPU-side geometry waiting to be uploaded. The model scene changes with the design; the
/// highlight scene (hover, selection, the origin widget) changes often and is small.
#[derive(Default, Clone)]
pub struct GpuScene {
    /// The colours every vertex, line and instance index into.
    pub palette: Arc<Palette>,
    /// Opaque triangles. A zero normal draws the colour flat (unlit).
    pub tris: Vec<u8>,
    /// Opaque product PBR vertices; linear base color and explicit optical parameters.
    pub pbr: Vec<u8>,
    /// Translucent triangles (alpha from the colour), drawn after the opaque ones without
    /// writing depth.
    pub trans: Vec<u8>,
    /// Ground-plane shadow: alpha-blended after opaque geometry, without writing depth.
    /// It does not require order-independent transparency.
    pub shadow: Vec<u8>,
    /// See-through bodies and faces (appearance opacity below 1), closed and outward-wound:
    /// their far sides are drawn first, then their near sides, so they read like glass.
    pub glass: Vec<u8>,
    /// Depth-tested lines.
    pub lines: Vec<u8>,
    /// Lines drawn only where the model hides them (hidden edges).
    pub hidden: Vec<u8>,
    /// Lines drawn over everything (active sketch, highlights).
    pub overlay: Vec<u8>,
    /// Translucent triangles pushed slightly back in depth, so they only show where nothing
    /// coincides with them (removed material in a preview).
    pub ghost: Vec<u8>,
    /// Translucent triangles drawn through everything (a cut's tool body).
    pub xray: Vec<u8>,
    /// Lines drawn first, under everything (the ground grid: the model hides it, it never
    /// hides the model).
    pub under: Vec<u8>,
    /// Reference planes, axes and sketches: depth-tested translucent triangles and lines,
    /// left whole by a section (#1229, #8617).
    pub guide_trans: Vec<u8>,
    pub guide_lines: Vec<u8>,
    /// Opaque bodies drawn from their plans on the GPU, like `tris`.
    pub planned: Vec<PlannedBody>,
    /// Opaque bodies drawn as placements of shared triangles, like `tris`.
    pub instanced: Vec<InstancedMesh>,
    /// Whole hovered bodies the model draws from their plans on the GPU: lit again over the
    /// model in the hover colour from the model's own evaluated triangles, so a whole assembly
    /// under the pointer costs a draw per body, not a copy of every triangle on the CPU.
    pub hover_planned: Vec<Arc<solidcraft_geom::tess::TessPlan>>,
    /// Whole selected bodies, as `hover_planned`, in the selection colour.
    pub selected_planned: Vec<Arc<solidcraft_geom::tess::TessPlan>>,
    /// Whole hovered bodies the model builds on the CPU (no display plan): their triangles in
    /// the model scene's own blocks, drawn again tinted.
    pub hover_blocks: Vec<BlockTris>,
    /// Whole selected bodies, as `hover_blocks`, in the selection colour.
    pub selected_blocks: Vec<BlockTris>,
    /// Whole highlighted bodies the model builds in its blocks but does not draw opaque (faded
    /// context, see-through): every one of their vertices drawn again, in this scene's palette,
    /// whose entries for their faces hold the highlight colour. No triangle is copied.
    pub recoloured: Vec<BlockTris>,
    /// Whole hovered bodies the model draws as placements of shared triangles: those meshes with
    /// the body's highlight colour on each placement.
    pub hover_instanced: Vec<InstancedMesh>,
    /// Whole selected bodies, as `hover_instanced`, in the selection colour.
    pub selected_instanced: Vec<InstancedMesh>,
    /// The model's bodies, a few at a time, drawn before the loose geometry above (`tris`,
    /// `trans`, `lines` and `hidden` of each block, in order, then the scene's own). A block
    /// that comes back unchanged keeps its buffers: a design changing one body at a time (an
    /// agent adding bodies over MCP) uploads only the blocks that changed, not the whole model.
    pub blocks: Vec<SceneBlock>,
}

impl GpuScene {
    /// A scene whose palette starts with `palette`'s stable entries (the model scene).
    pub fn with_palette(palette: Palette) -> GpuScene {
        GpuScene { palette: Arc::new(palette), ..Default::default() }
    }

    /// Intern an ephemeral colour (grid, highlight, sketch, preview) into this scene's palette.
    pub fn color(&mut self, rgba: [u8; 4]) -> PaletteIndex {
        Arc::make_mut(&mut self.palette).intern(rgba)
    }

    /// The blocks' geometry moved into the scene's own lists, in drawing order (for tests that
    /// count what the scene holds).
    pub fn merged(mut self) -> GpuScene {
        let palette = self.palette.clone();
        let mut out = GpuScene { glass: std::mem::take(&mut self.glass), palette: palette.clone(), ..Default::default() };
        for block in std::mem::take(&mut self.blocks) {
            append_runs(&block.bytes.vertices, &block.bytes.opaque, &mut out.tris);
            append_runs(&block.bytes.vertices, &block.bytes.glass, &mut out.glass);
            append_runs(&block.bytes.vertices, &block.bytes.faded, &mut out.trans);
            out.lines.extend_from_slice(&block.bytes.lines);
            out.hidden.extend_from_slice(&block.bytes.hidden);
        }
        for m in std::mem::take(&mut self.instanced) {
            match m.look {
                InstanceLook::Opaque => m.expand_into(&mut out.tris),
                InstanceLook::Faded => m.expand_into(&mut out.trans),
                InstanceLook::Glass => m.expand_into(&mut out.glass),
            }
        }
        out.tris.append(&mut self.tris);
        out.trans.append(&mut self.trans);
        out.lines.append(&mut self.lines);
        out.hidden.append(&mut self.hidden);
        let palette = std::mem::replace(&mut self.palette, palette);
        GpuScene { palette, tris: out.tris, trans: out.trans, glass: out.glass, lines: out.lines, hidden: out.hidden, ..self }
    }
}

/// A class's triangles as `TRI_SIZE` vertex records: its runs of the block's vertices, in order.
fn append_runs(vertices: &[u8], runs: &[std::ops::Range<u32>], out: &mut Vec<u8>) {
    for r in runs {
        let (a, b) = (r.start as usize * TRI_SIZE, r.end as usize * TRI_SIZE);
        if let Some(v) = vertices.get(a..b) {
            out.extend_from_slice(v);
        }
    }
}

/// The part of `runs` (sorted, not overlapping) inside `within`.
pub fn runs_within(runs: &[std::ops::Range<u32>], within: std::ops::Range<u32>) -> Vec<std::ops::Range<u32>> {
    let first = runs.partition_point(|r| r.end <= within.start);
    runs[first..]
        .iter()
        .take_while(|r| r.start < within.end)
        .map(|r| r.start.max(within.start)..r.end.min(within.end))
        .filter(|r| !r.is_empty())
        .collect()
}

/// A run of the model's bodies in the scene: their triangles and edges, and a key that names
/// exactly this content (the viewport keeps the bytes while the key comes back).
#[derive(Clone)]
pub struct SceneBlock {
    pub key: u64,
    pub bytes: Arc<BlockBytes>,
}

/// A whole highlighted body the model builds on the CPU: its triangles in a model scene block.
#[derive(Clone, Debug)]
pub struct BlockTris {
    /// The block's key ([`SceneBlock::key`]).
    pub block: u64,
    /// The body's opaque triangles: runs of the block's vertices (a block too large for one
    /// vertex buffer draws from copies instead; its highlights are then skipped).
    pub tris: Vec<std::ops::Range<u32>>,
    /// All of the body's vertices in the block's canonical vertex stream.
    pub vertices: std::ops::Range<u32>,
}

/// How a built triangle is drawn: opaque, see-through (glass), or faded context.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum TriClass {
    Opaque,
    Glass,
    Faded,
}

/// A block's geometry: one canonical triangle vertex stream (`TRI_SIZE` bytes a vertex, three a
/// triangle) and, per draw class, the runs of that stream the class draws. A colour or opacity
/// edit changes no vertex; an opacity class change changes only the runs, which are drawn
/// straight from the vertex buffer (no index buffer to upload).
#[derive(Default, Clone)]
pub struct BlockBytes {
    pub vertices: Arc<Vec<u8>>,
    /// Each body's share of `vertices` and the box around it: a body out of the view is left
    /// out of the draws ([`Batches::cull`]). Vertices outside every span are always drawn.
    pub bodies: Arc<Vec<BodySpan>>,
    pub opaque: Arc<Vec<std::ops::Range<u32>>>,
    pub glass: Arc<Vec<std::ops::Range<u32>>>,
    pub faded: Arc<Vec<std::ops::Range<u32>>>,
    pub lines: Arc<Vec<u8>>,
    pub hidden: Arc<Vec<u8>>,
}

impl BlockBytes {
    /// The runs of `class`.
    pub fn runs(&self, class: TriClass) -> &Arc<Vec<std::ops::Range<u32>>> {
        match class {
            TriClass::Opaque => &self.opaque,
            TriClass::Glass => &self.glass,
            TriClass::Faded => &self.faded,
        }
    }
}

/// A body's vertices in a block and the box around them.
#[derive(Clone, Debug, PartialEq)]
pub struct BodySpan {
    pub vertices: std::ops::Range<u32>,
    pub bounds: [[f32; 3]; 2],
}

/// `runs` without the vertices in `hidden` (both sorted and disjoint).
fn runs_without(runs: &[std::ops::Range<u32>], hidden: &[std::ops::Range<u32>]) -> Vec<std::ops::Range<u32>> {
    let mut out = Vec::with_capacity(runs.len());
    let mut h = hidden.iter().peekable();
    for r in runs {
        let mut start = r.start;
        while let Some(x) = h.peek() {
            if x.end <= start {
                h.next();
                continue;
            }
            if x.start >= r.end {
                break;
            }
            if x.start > start {
                out.push(start..x.start);
            }
            start = start.max(x.end);
            if x.end >= r.end {
                break;
            }
            h.next();
        }
        if start < r.end {
            out.push(start..r.end);
        }
    }
    out
}

/// Add a triangle starting at vertex `at` to `runs`, extending the last run when it ends there.
pub fn extend_runs(runs: &mut Vec<std::ops::Range<u32>>, at: u32) {
    let end = at.saturating_add(3);
    match runs.last_mut() {
        Some(last) if last.end == at => last.end = end,
        _ => runs.push(at..end),
    }
}

/// Builds a [`BlockBytes`], writing each triangle's vertices once and filing it by class.
#[derive(Default)]
pub struct BlockBuilder {
    vertices: Vec<u8>,
    opaque: Vec<std::ops::Range<u32>>,
    glass: Vec<std::ops::Range<u32>>,
    faded: Vec<std::ops::Range<u32>>,
    lines: Vec<u8>,
    hidden: Vec<u8>,
    bodies: Vec<BodySpan>,
    /// The body being written: its first vertex and the box so far.
    open: Option<(u32, [[f32; 3]; 2])>,
}

impl BlockBuilder {
    /// The triangles from here to [`BlockBuilder::end_body`] are one body's (its box is kept, so
    /// it can be left out of a view it is not in).
    pub fn begin_body(&mut self) {
        let at = u32::try_from(self.vertices.len() / TRI_SIZE).unwrap_or(u32::MAX);
        self.open = Some((at, [[f32::INFINITY; 3], [f32::NEG_INFINITY; 3]]));
    }

    /// End the body [`BlockBuilder::begin_body`] began.
    pub fn end_body(&mut self) {
        let Some((start, bounds)) = self.open.take() else { return };
        let end = u32::try_from(self.vertices.len() / TRI_SIZE).unwrap_or(u32::MAX);
        if end > start && bounds.iter().flatten().all(|x| x.is_finite()) {
            self.bodies.push(BodySpan { vertices: start..end, bounds });
        }
    }

    /// Room for `triangles` more triangles, so a block of large bodies is written without
    /// reallocating (and copying) its vertex stream as it grows.
    pub fn reserve_triangles(&mut self, triangles: usize) {
        let _ = self.vertices.try_reserve_exact(triangles.saturating_mul(3 * TRI_SIZE));
    }

    /// Add a triangle (three positions and normals, one palette index) under `class`.
    pub fn triangle(&mut self, p: [[f32; 3]; 3], n: [[f32; 3]; 3], c: PaletteIndex, class: TriClass) {
        let at = u32::try_from(self.vertices.len() / TRI_SIZE).unwrap_or(u32::MAX);
        let mut record = [0u8; 3 * TRI_SIZE];
        for ((out, p), n) in record.as_chunks_mut::<TRI_SIZE>().0.iter_mut().zip(p).zip(n) {
            let words = [p[0].to_bits(), p[1].to_bits(), p[2].to_bits(), n[0].to_bits(), n[1].to_bits(), n[2].to_bits(), c.0];
            for (w, slot) in words.iter().zip(out.as_chunks_mut::<4>().0) {
                *slot = w.to_le_bytes();
            }
        }
        self.vertices.extend_from_slice(&record);
        if let Some((_, [lo, hi])) = &mut self.open {
            for v in p {
                for k in 0..3 {
                    lo[k] = lo[k].min(v[k]);
                    hi[k] = hi[k].max(v[k]);
                }
            }
        }
        let runs = match class {
            TriClass::Opaque => &mut self.opaque,
            TriClass::Glass => &mut self.glass,
            TriClass::Faded => &mut self.faded,
        };
        extend_runs(runs, at);
    }

    /// The runs of `class` that fall within `vertices` (a body's share of the block).
    pub fn runs_within(&self, class: TriClass, vertices: std::ops::Range<u32>) -> Vec<std::ops::Range<u32>> {
        let runs = match class {
            TriClass::Opaque => &self.opaque,
            TriClass::Glass => &self.glass,
            TriClass::Faded => &self.faded,
        };
        runs_within(runs, vertices)
    }

    /// A depth-tested line, or one only shown where the model hides it.
    pub fn line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32) {
        self.lines.extend_from_slice(&line_record(a, b, c, width.to_bits()));
    }

    pub fn hidden_line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32) {
        self.hidden.extend_from_slice(&line_record(a, b, c, width.to_bits()));
    }

    /// A vertex marker: a square `size` pixels across at `p`, the same size at every zoom. It is a
    /// line from `p` to itself (the line shader caps each end by half its width), lifted toward
    /// the eye by half its size so a face through the vertex seen at a slant does not bury half of
    /// it; `hidden` draws it only where the model hides it, as a hidden edge.
    pub fn marker(&mut self, p: [f32; 3], c: PaletteIndex, size: f32, hidden: bool) {
        let out = if hidden { &mut self.hidden } else { &mut self.lines };
        out.extend_from_slice(&line_record(p, p, c, lifted_width(size, [[0.0; 3]; 2])));
    }

    /// How many vertices the block holds so far.
    pub fn vertices_len(&self) -> usize {
        self.vertices.len() / TRI_SIZE
    }

    /// The block, frozen for the scene and the GPU cache.
    pub fn freeze(self) -> Arc<BlockBytes> {
        Arc::new(BlockBytes {
            vertices: Arc::new(self.vertices),
            bodies: Arc::new(self.bodies),
            opaque: Arc::new(self.opaque),
            glass: Arc::new(self.glass),
            faded: Arc::new(self.faded),
            lines: Arc::new(self.lines),
            hidden: Arc::new(self.hidden),
        })
    }
}

impl GpuScene {
    pub fn pbr_tri(&mut self, p: [f32; 3], n: [f32; 3], surface: solidcraft_render::surface::OpaqueSurface) {
        let base = surface.base_color().to_f32();
        let parameters = [surface.metallic() as f32, surface.roughness() as f32, surface.dielectric_ior() as f32];
        for value in p.into_iter().chain(n).chain(base).chain(parameters) {
            self.pbr.extend_from_slice(&value.to_le_bytes());
        }
    }
    pub fn tri(&mut self, p: [f32; 3], n: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&n) {
            self.tris.extend_from_slice(&v.to_le_bytes());
        }
        self.tris.extend_from_slice(&c.0.to_le_bytes());
    }
    /// A vertex of the ground-plane shadow (see [`GpuScene::shadow`]).
    pub fn shadow_tri(&mut self, p: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&[0.0; 3]) {
            self.shadow.extend_from_slice(&v.to_le_bytes());
        }
        self.shadow.extend_from_slice(&c.0.to_le_bytes());
    }
    pub fn trans_tri(&mut self, p: [f32; 3], n: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&n) {
            self.trans.extend_from_slice(&v.to_le_bytes());
        }
        self.trans.extend_from_slice(&c.0.to_le_bytes());
    }
    /// A line shown only where the model hides it.
    pub fn hidden_line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32) {
        self.hidden.extend_from_slice(&line_record(a, b, c, width.to_bits()));
    }

    /// Room for `lines` more lines and `hidden` more hidden lines, so a highlight of many edges
    /// is written without growing its buffers line by line.
    pub fn reserve_lines(&mut self, lines: usize, hidden: usize) {
        let _ = self.lines.try_reserve(lines.saturating_mul(LINE_SIZE));
        let _ = self.hidden.try_reserve(hidden.saturating_mul(LINE_SIZE));
    }
    pub fn glass_tri(&mut self, p: [f32; 3], n: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&n) {
            self.glass.extend_from_slice(&v.to_le_bytes());
        }
        self.glass.extend_from_slice(&c.0.to_le_bytes());
    }
    pub fn ghost_tri(&mut self, p: [f32; 3], n: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&n) {
            self.ghost.extend_from_slice(&v.to_le_bytes());
        }
        self.ghost.extend_from_slice(&c.0.to_le_bytes());
    }
    pub fn xray_tri(&mut self, p: [f32; 3], n: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&n) {
            self.xray.extend_from_slice(&v.to_le_bytes());
        }
        self.xray.extend_from_slice(&c.0.to_le_bytes());
    }
    /// A line under everything (see [`GpuScene::under`]).
    pub fn under_line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32) {
        for v in a.iter().chain(&b) {
            self.under.extend_from_slice(&v.to_le_bytes());
        }
        self.under.extend_from_slice(&c.0.to_le_bytes());
        self.under.extend_from_slice(&width.to_le_bytes());
    }
    /// A translucent triangle of a plane a section does not cut (see [`GpuScene::guide_trans`]).
    pub fn guide_tri(&mut self, p: [f32; 3], c: PaletteIndex) {
        for v in p.iter().chain(&[0.0; 3]) {
            self.guide_trans.extend_from_slice(&v.to_le_bytes());
        }
        self.guide_trans.extend_from_slice(&c.0.to_le_bytes());
    }
    /// A depth-tested line a section does not cut (see [`GpuScene::guide_lines`]).
    pub fn guide_line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32) {
        for v in a.iter().chain(&b) {
            self.guide_lines.extend_from_slice(&v.to_le_bytes());
        }
        self.guide_lines.extend_from_slice(&c.0.to_le_bytes());
        self.guide_lines.extend_from_slice(&width.to_le_bytes());
    }
    /// A thick highlight along an edge (a selected or hovered edge with its halo), lit like a
    /// thin cylinder around it: its depth is pulled toward the eye by its half width times how
    /// steeply the faces beside it are seen (`normals`: theirs along the segment; zero when
    /// unknown), so a face seen at a grazing angle no longer buries part of the band (it showed as
    /// a thin wedge instead of a line). See [`lifted_width`] for how this travels.
    pub fn lifted_line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32, normals: [[f32; 3]; 2]) {
        self.lifted_line_word(a, b, c, lifted_width(width, normals));
    }
    /// [`GpuScene::lifted_line`] with its width word already worked out ([`lifted_width`]), for
    /// many segments that share it.
    pub fn lifted_line_word(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, word: u32) {
        self.lines.extend_from_slice(&line_record(a, b, c, word));
    }
    pub fn line(&mut self, a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: f32, on_top: bool) {
        let out = if on_top { &mut self.overlay } else { &mut self.lines };
        out.extend_from_slice(&line_record(a, b, c, width.to_bits()));
    }
}

/// One line's [`LINE_SIZE`] bytes: its ends, its palette index and its width word (an `f32`'s
/// bits, or [`lifted_width`]'s).
fn line_record(a: [f32; 3], b: [f32; 3], c: PaletteIndex, width: u32) -> [u8; LINE_SIZE] {
    let mut out = [0u8; LINE_SIZE];
    let words = a.into_iter().chain(b).map(f32::to_bits).chain([c.0, width]);
    for (chunk, word) in out.as_chunks_mut::<4>().0.iter_mut().zip(words) {
        *chunk = word.to_le_bytes();
    }
    out
}

pub type SceneSlot = Arc<Mutex<Option<GpuScene>>>;

/// Where the product preview's geometry upload reports a GPU error it caught (out of memory, a
/// buffer the device refused): the preview shows it instead of drawing.
pub type UploadError = Arc<Mutex<Option<String>>>;

/// A reference image (canvas or decal): a textured quad, depth-tested against the model.
#[derive(Clone)]
pub struct GpuImage {
    pub id: u64,
    /// Changes when the pixels change (the texture is re-uploaded).
    pub version: u64,
    pub size: [u32; 2],
    /// Straight RGBA8, `size[0] * size[1] * 4` bytes.
    pub rgba: Arc<Vec<u8>>,
    /// Bottom-left, bottom-right, top-right, top-left of the image.
    pub corners: [[f32; 3]; 4],
    pub opacity: f32,
}

/// Image vertex: position (3 × f32), uv (2 × f32), opacity (f32).
const IMG_SIZE: usize = 24;

/// Ambient occlusion samples per shaded pixel in a still picture (the shader's most).
pub const AO_SAMPLES: u32 = 32;

/// Ambient occlusion samples per shaded pixel while the view moves (#7165): a quarter of the
/// still picture's, on the same turned pattern, so orbiting costs less and shows no copies of
/// nearby parts (#4167).
pub const AO_SAMPLES_MOVING: u32 = 8;

/// Per-frame parameters.
pub struct Frame {
    /// Bounded unshadowed product-light preview. None retains the fixed studio.
    pub product_lights: Option<Vec<[f32; 16]>>,
    /// Product preview uses its own batches, retaining the Design viewport's uploads.
    pub product: bool,
    /// Product preview only: receives a GPU error raised by its geometry upload.
    pub upload_error: Option<UploadError>,
    pub key: u64,
    pub slot: SceneSlot,
    pub sketch_key: u64,
    pub sketch_slot: SceneSlot,
    /// The highlight scene and its key.
    pub hl_key: u64,
    pub hl_slot: SceneSlot,
    /// The live preview scene and its key.
    pub pv_key: u64,
    pub pv_slot: SceneSlot,
    /// Preview reselect: skip drawing the preview while Primary rolls it back.
    pub pv_hide: bool,
    /// Column-major view-projection.
    pub view_proj: [[f32; 4]; 4],
    /// Direction toward the eye (for lighting).
    pub back: [f32; 3],
    /// Viewport size in physical pixels.
    pub size_px: [f32; 2],
    /// Section cut: (normal, d); fragments with normal·p > d are hidden. A zero normal: none.
    pub clip: [f32; 4],
    /// Colour of the inside of cut bodies (straight sRGB, 0..1); alpha 1 hatches it, 0 leaves it plain in the body's colour.
    pub cap: [f32; 4],
    /// Canvases and decals.
    pub images: Vec<GpuImage>,
    /// Point clouds.
    pub clouds: Vec<GpuCloud>,
    /// Surface analysis: (mode: 0 none, 1 zebra, 2 draft, 3 curvature, 4 environment,
    /// 5 accessibility; parameter; scale; size) and the pull / access direction.
    pub analysis: [f32; 8],
    /// The analysis colours and the stripe flag ([`analysis_colors`]).
    pub analysis_colors: [f32; 36],
    /// Accessibility: the depth map seen from the access direction.
    pub access: Option<AccessMap>,
    /// Draw silhouette outlines around the model (visual styles with edges).
    pub outline: bool,
    /// Eye position and 1 for a perspective camera (0: orthographic), for the section cap's depth
    /// and the depth bias.
    pub eye: [f32; 4],
    /// World-space occlusion radius; zero skips the depth prepass and all AO sampling.
    pub ao_radius: f32,
    /// Occlusion samples per shaded pixel: [`AO_SAMPLES`] for a still picture,
    /// [`AO_SAMPLES_MOVING`] while the view moves (#7165).
    pub ao_samples: u32,
    /// Supersampling (#1288): draw the frame this many times the viewport's pixels in each
    /// direction and filter it down (1: off). Under 1 the frame is drawn smaller and filtered up
    /// (the product preview past its pixel budget, #1464).
    pub ssaa: f32,
    /// Whole-body hover highlight: added to the body's own colour before lighting (sRGB 0..1).
    pub hover_tint: [f32; 3],
    /// Whole-body selection highlight: a selected body's colour before lighting (sRGB 0..1).
    pub selection_tint: [f32; 3],
    /// The world's up direction and 1 to shade in linear light with a sky fill that follows it
    /// (#7974); 0 shades as before.
    pub sky: [f32; 4],
}

/// Heights of the model seen from a direction (an orthographic depth map): a point is reachable
/// from that direction when nothing lies above it.
#[derive(Clone)]
pub struct AccessMap {
    pub version: u64,
    pub size: u32,
    /// Row-major heights along the direction (−∞ where nothing is).
    pub depth: Arc<Vec<f32>>,
    /// Map axes: pixel = (p·u + u.w, p·v + v.w) × scale.
    pub u: [f32; 4],
    pub v: [f32; 4],
    pub scale: f32,
}

/// The accessibility depth map's resolution (pixels per side).
pub const ACCESS_SIZE: u32 = 1024;

impl AccessMap {
    /// The depth map of `meshes` seen along `dir` (`version`: what the GPU uploads it by):
    /// rasterise the triangles seen along `dir` (orthographic), keeping the highest point per pixel.
    pub fn build(meshes: &[&solidcraft_geom::Mesh], dir: Vec3, version: u64) -> Option<AccessMap> {
        let d = dir.normalized()?;
        let u = d.cross(if d.x.abs() < 0.9 { Vec3::X } else { Vec3::Y }).normalized()?;
        let v = d.cross(u);
        let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
        for m in meshes {
            for p in &m.positions {
                let (a, b) = (p.dot(u), p.dot(v));
                lo = [lo[0].min(a), lo[1].min(b)];
                hi = [hi[0].max(a), hi[1].max(b)];
            }
        }
        let ext = (hi[0] - lo[0]).max(hi[1] - lo[1]);
        if !(ext.is_finite() && ext > 1e-9) {
            return None;
        }
        let n = ACCESS_SIZE as usize;
        let scale = (ACCESS_SIZE - 2) as f64 / ext;
        // One pixel of margin on each side.
        let (ou, ov) = (-lo[0] + 1.0 / scale, -lo[1] + 1.0 / scale);
        let mut depth = vec![f32::NEG_INFINITY; n * n];
        for m in meshes {
            for t in &m.triangles {
                let q: Vec<(f64, f64, f64)> = t
                    .iter()
                    .filter_map(|i| m.positions.get(*i as usize))
                    .map(|p| ((p.dot(u) + ou) * scale, (p.dot(v) + ov) * scale, p.dot(d)))
                    .collect();
                let [a, b, c] = q[..] else { continue };
                let area = (b.0 - a.0) * (c.1 - a.1) - (b.1 - a.1) * (c.0 - a.0);
                let (x0, x1) = (a.0.min(b.0).min(c.0).floor().max(0.0) as usize, (a.0.max(b.0).max(c.0).ceil() as usize).min(n - 1));
                let (y0, y1) = (a.1.min(b.1).min(c.1).floor().max(0.0) as usize, (a.1.max(b.1).max(c.1).ceil() as usize).min(n - 1));
                // Edge-on triangles: their vertices still count.
                if area.abs() < 1e-12 {
                    for p in [a, b, c] {
                        let (x, y) = (p.0 as usize, p.1 as usize);
                        if let Some(z) = depth.get_mut(y * n + x) {
                            *z = z.max(p.2 as f32);
                        }
                    }
                    continue;
                }
                for y in y0..=y1 {
                    for x in x0..=x1 {
                        let (px, py) = (x as f64 + 0.5, y as f64 + 0.5);
                        let w0 = ((b.0 - px) * (c.1 - py) - (b.1 - py) * (c.0 - px)) / area;
                        let w1 = ((c.0 - px) * (a.1 - py) - (c.1 - py) * (a.0 - px)) / area;
                        let w2 = 1.0 - w0 - w1;
                        if w0 < -1e-9 || w1 < -1e-9 || w2 < -1e-9 {
                            continue;
                        }
                        let z = (w0 * a.2 + w1 * b.2 + w2 * c.2) as f32;
                        if let Some(cell) = depth.get_mut(y * n + x) {
                            *cell = cell.max(z);
                        }
                    }
                }
            }
        }
        Some(AccessMap {
            version,
            size: ACCESS_SIZE,
            depth: Arc::new(depth),
            u: [u.x as f32, u.y as f32, u.z as f32, ou as f32],
            v: [v.x as f32, v.y as f32, v.z as f32, ov as f32],
            scale: scale as f32,
        })
    }
}

/// The shader's surface analysis uniform (see [`Frame::analysis`]) for `mode`; Accessibility
/// reads its depth map's scale and size from `access`.
pub fn analysis_uniform(mode: Option<solidcraft_render::AnalysisMode>, access: Option<&AccessMap>) -> [f32; 8] {
    use solidcraft_render::AnalysisMode as A;
    match mode {
        Some(A::Zebra { stripes }) => [1.0, stripes as f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        Some(A::Draft { pull, angle }) => [2.0, angle as f32, 0.0, 0.0, pull.x as f32, pull.y as f32, pull.z as f32, 0.0],
        Some(A::Curvature { radius }) => [3.0, radius as f32, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        Some(A::Environment) => [4.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0],
        Some(A::Access { dir }) => {
            let (scale, size, eps) = access.map(|m| (m.scale, m.size as f32, 1.5 / m.scale.max(1e-9))).unwrap_or((0.0, 0.0, 0.0));
            [5.0, eps, scale, size, dir.x as f32, dir.y as f32, dir.z as f32, 0.0]
        }
        None => [0.0; 8],
    }
}

/// Vertices (or line instances) on the GPU, in as many buffers as the device's buffer size
/// limit needs: a large import's triangles can exceed one buffer (#883).
#[derive(Clone)]
struct Batch {
    /// Each buffer and its vertex (instance) count.
    parts: Vec<(wgpu::Buffer, u32)>,
    /// Bytes per buffer (all but the last are full).
    chunk: usize,
}

/// A chunk of planned bodies on the GPU: their evaluated mesh and a colour per vertex.
struct PlannedDraw {
    mesh: Arc<crate::GpuMesh>,
    colors: wgpu::Buffer,
    /// Its bodies' class: a chunk holds bodies of one class.
    class: TriClass,
    /// The address of each plan in the chunk, in `mesh.parts` order: a whole-body highlight
    /// finds the evaluated triangles of the body it lights.
    plans: Vec<usize>,
}

impl PlannedDraw {
    /// The part drawn from the plan at `address`, if this chunk holds it.
    fn part_of(&self, address: usize) -> Option<&crate::Part> {
        self.plans.iter().position(|p| *p == address).and_then(|i| self.mesh.parts.get(i))
    }
}

/// An [`InstancedMesh`]'s triangles on the GPU (kept while its key comes back).
enum SharedGpu {
    /// Shared vertices and their indices.
    Indexed { vertices: wgpu::Buffer, indices: wgpu::Buffer, count: u32 },
    /// Too large for one buffer: unshared, in as many buffers as it takes.
    Unshared(Batch),
}

impl SharedGpu {
    fn new(device: &wgpu::Device, tris: &SharedTris) -> Option<SharedGpu> {
        let limit = device.limits().max_buffer_size;
        let index_bytes = (tris.indices.len() * 4) as u64;
        if tris.vertices.is_empty() || tris.indices.is_empty() {
            return None;
        }
        if tris.vertices.len() as u64 > limit || index_bytes > limit {
            return upload(device, "sc_instanced_unshared", &tris.unshared(), SHARED_SIZE).map(SharedGpu::Unshared);
        }
        let buffer = |label: &str, contents: &[u8], usage: wgpu::BufferUsages| buffer_init(device, label, contents, usage);
        let indices: Vec<u8> = tris.indices.iter().flat_map(|i| i.to_le_bytes()).collect();
        Some(SharedGpu::Indexed {
            vertices: buffer("sc_instanced_vertices", &tris.vertices, wgpu::BufferUsages::VERTEX)?,
            indices: buffer("sc_instanced_indices", &indices, wgpu::BufferUsages::INDEX)?,
            count: u32::try_from(tris.indices.len()).unwrap_or(u32::MAX),
        })
    }
}

/// How far (pixels) a level of detail may stray from the full triangles where a placement is
/// drawn from it.
pub const LOD_PX: f32 = 0.75;

/// Pixels per millimetre where a placement (`rows`, see [`INSTANCE_SIZE`]) of a box (`bounds`)
/// is nearest the eye, along whichever of its axes shows largest, in the view `vp` (columns) of
/// `size` pixels. Infinite when a corner of it is at or behind the eye, or for a box that is not
/// finite: such a placement is drawn in full.
pub fn placement_scale(rows: &[[f32; 4]; 3], bounds: &[[f32; 3]; 2], vp: &[[f32; 4]; 4], size: [f32; 2]) -> f32 {
    let [lo, hi] = bounds;
    if !lo.iter().chain(hi).all(|x| x.is_finite()) {
        return f32::INFINITY;
    }
    let clip = |p: [f32; 3]| -> [f32; 4] {
        let w = rows.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3]);
        std::array::from_fn(|k| vp[0][k] * w[0] + vp[1][k] * w[1] + vp[2][k] * w[2] + vp[3][k])
    };
    let corners: [[f32; 3]; 8] = std::array::from_fn(|i| {
        [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }]
    });
    let mut nearest = (f32::INFINITY, corners[0]);
    for c in corners {
        let w = clip(c)[3];
        if !(w > 1e-6) {
            return f32::INFINITY;
        }
        if w < nearest.0 {
            nearest = (w, c);
        }
    }
    let screen = |c: [f32; 4]| [c[0] / c[3] * size[0] * 0.5, c[1] / c[3] * size[1] * 0.5];
    let (_, c) = nearest;
    let at = screen(clip(c));
    let mut scale = 0.0f32;
    for k in 0..3 {
        // A step of a hundredth of the box (at least a micrometre) along each axis.
        let step = ((hi[k] - lo[k]) * 0.01).max(1e-3);
        let mut q = c;
        q[k] += step;
        let qc = clip(q);
        if !(qc[3] > 1e-6) {
            return f32::INFINITY;
        }
        let s = screen(qc);
        scale = scale.max(((s[0] - at[0]).powi(2) + (s[1] - at[1]).powi(2)).sqrt() / step);
    }
    if scale.is_finite() { scale } else { f32::INFINITY }
}

/// The coarsest level whose error (mm; level 0, the full triangles, has none) stays under
/// [`LOD_PX`] at `scale` pixels per millimetre.
pub fn level_for(errors: &[f32], scale: f32) -> usize {
    errors.iter().rposition(|e| *e * scale <= LOD_PX).unwrap_or(0)
}

/// Whether a box (`bounds`, min and max) moved by a placement (`rows`, see [`INSTANCE_SIZE`])
/// may be seen in the view `vp` (the view-projection, columns, depth 0 near and 1 far): `false`
/// only when all its corners lie beyond the same side of the view (left, right, top, bottom,
/// before the near plane or past the far one). Tested on the clip-space corners, so it holds for
/// corners behind the eye too.
pub fn placed_box_in_view(rows: &[[f32; 4]; 3], bounds: &[[f32; 3]; 2], vp: &[[f32; 4]; 4]) -> bool {
    let [lo, hi] = bounds;
    if !lo.iter().chain(hi).all(|x| x.is_finite()) {
        return true;
    }
    // Per plane, whether every corner so far is outside it: x > w, -x > w, y > w, -y > w, z < 0, z > w.
    let mut out = [true; 6];
    for i in 0..8 {
        let p = [if i & 1 == 0 { lo[0] } else { hi[0] }, if i & 2 == 0 { lo[1] } else { hi[1] }, if i & 4 == 0 { lo[2] } else { hi[2] }];
        let w = rows.map(|r| r[0] * p[0] + r[1] * p[1] + r[2] * p[2] + r[3]);
        let c: [f32; 4] = std::array::from_fn(|k| vp[0][k] * w[0] + vp[1][k] * w[1] + vp[2][k] * w[2] + vp[3][k]);
        let outside = [c[0] > c[3], -c[0] > c[3], c[1] > c[3], -c[1] > c[3], c[2] < 0.0, c[2] > c[3]];
        for (o, x) in out.iter_mut().zip(outside) {
            *o &= x;
        }
    }
    !out.iter().any(|o| *o)
}

/// One level of an [`InstancedDraw`]: its triangles and the placements drawn from it.
struct LevelDraw {
    tris: Arc<SharedGpu>,
    flat: bool,
    /// How far it strays from the full triangles (mm; 0 for them).
    error: f32,
    /// The placements drawn from it, first `count` of them ([`Batches::cull`]).
    instances: wgpu::Buffer,
    count: u32,
}

/// An [`InstancedMesh`] on the GPU: its triangles and coarser levels (shared with the next scene
/// while they come back), and the placements in view, each drawn from its level.
struct InstancedDraw {
    look: InstanceLook,
    /// The full triangles first, then coarser ones.
    levels: Vec<LevelDraw>,
    bounds: [[f32; 3]; 2],
    /// Every placement ([`INSTANCE_SIZE`] bytes each), as the scene has them.
    placements: Vec<u8>,
    /// Which placements the levels hold now, with their level (none drawn yet: `None`).
    shown: Option<Vec<(u32, u8)>>,
}

impl InstancedDraw {
    /// Draw every placement in view, from its level, with `smooth` or `flat` as the level's
    /// triangles are lit.
    pub(crate) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, smooth: &wgpu::RenderPipeline, flat: &wgpu::RenderPipeline, tally: &Tally) {
        for level in &self.levels {
            pass.set_pipeline(if level.flat { flat } else { smooth });
            level.draw(pass, tally);
        }
    }
}

impl LevelDraw {
    /// Draw its placements (in vertex buffer 1).
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, tally: &Tally) {
        if self.count == 0 {
            return;
        }
        pass.set_vertex_buffer(1, self.instances.slice(..));
        match &*self.tris {
            SharedGpu::Indexed { vertices, indices, count } => {
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..self.count);
                tally.triangles(*count, self.count);
            }
            SharedGpu::Unshared(batch) => {
                for (buffer, count) in &batch.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..self.count);
                    tally.triangles(*count, self.count);
                }
            }
        }
    }
}

/// Most runs a class draws one by one; a class broken into more (faces of their own opacity all
/// over a body) is drawn through an index buffer instead, in one call.
const MAX_RUNS: usize = 64;

/// One class's triangles on the GPU: runs of the block's vertex buffer (no buffer of their own),
/// an index buffer for a fragmented class, or copies of its triangles when the block is too large
/// for one vertex buffer.
#[derive(Clone)]
enum ClassGpu {
    Runs(Arc<Vec<std::ops::Range<u32>>>),
    Indexed { indices: wgpu::Buffer, count: u32 },
    Unindexed(Batch),
}

/// A scene block on the GPU, with the bytes it was uploaded from. Each artifact is kept while
/// its `Arc` comes back, so an opacity class change uploads nothing for a class drawn by runs.
struct BlockGpu {
    bytes: Arc<BlockBytes>,
    vertices: Option<wgpu::Buffer>,
    opaque: Option<ClassGpu>,
    glass: Option<ClassGpu>,
    faded: Option<ClassGpu>,
    lines: Option<Batch>,
    hidden: Option<Batch>,
    /// The runs of each class (opaque, glass, faded) without the bodies out of the view, while
    /// some are ([`Batches::cull`]); `None`: every body is drawn.
    culled: Option<[Vec<std::ops::Range<u32>>; 3]>,
}

fn class_gpu(
    device: &wgpu::Device,
    fits: bool,
    vertices: &[u8],
    runs: &Arc<Vec<std::ops::Range<u32>>>,
    old: Option<(&ClassGpu, &Arc<Vec<std::ops::Range<u32>>>)>,
    same_vertices: bool,
    uploaded: &mut u64,
) -> Option<ClassGpu> {
    if runs.is_empty() {
        return None;
    }
    let same_runs = old.is_some_and(|(_, old_runs)| Arc::ptr_eq(old_runs, runs));
    if !fits {
        if let (Some((ClassGpu::Unindexed(batch), _)), true) = (old, same_vertices && same_runs) {
            return Some(ClassGpu::Unindexed(batch.clone()));
        }
        let mut bytes = Vec::new();
        append_runs(vertices, runs, &mut bytes);
        *uploaded += bytes.len() as u64;
        return upload(device, "sc_block_class", &bytes, TRI_SIZE).map(ClassGpu::Unindexed);
    }
    if runs.len() <= MAX_RUNS {
        return Some(ClassGpu::Runs(runs.clone()));
    }
    if let (Some((ClassGpu::Indexed { indices, count }, _)), true) = (old, same_runs) {
        return Some(ClassGpu::Indexed { indices: indices.clone(), count: *count });
    }
    let bytes: Vec<u8> = runs.iter().flat_map(|r| r.clone()).flat_map(|i| i.to_le_bytes()).collect();
    *uploaded += bytes.len() as u64;
    let count = u32::try_from(bytes.len() / 4).unwrap_or(u32::MAX);
    let indices = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some("sc_block_indices"),
        contents: &bytes,
        usage: wgpu::BufferUsages::INDEX,
    });
    Some(ClassGpu::Indexed { indices, count })
}

impl BlockGpu {
    fn update(device: &wgpu::Device, old: Option<BlockGpu>, bytes: Arc<BlockBytes>, uploaded: &mut u64) -> BlockGpu {
        let fits = bytes.vertices.len() as u64 <= device.limits().max_buffer_size;
        let same_vertices = old.as_ref().is_some_and(|o| Arc::ptr_eq(&o.bytes.vertices, &bytes.vertices));
        let vertices = if !fits || bytes.vertices.is_empty() {
            None
        } else if same_vertices {
            old.as_ref().and_then(|o| o.vertices.clone())
        } else {
            *uploaded += bytes.vertices.len() as u64;
            Some(device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
                label: Some("sc_block_vertices"),
                contents: &bytes.vertices,
                usage: wgpu::BufferUsages::VERTEX,
            }))
        };
        let mut class = |class: TriClass| {
            let old_pair = old.as_ref().and_then(|o| o.class(class).map(|c| (c, o.bytes.runs(class))));
            class_gpu(device, fits, &bytes.vertices, bytes.runs(class), old_pair, same_vertices, uploaded)
        };
        let (opaque, glass, faded) = (class(TriClass::Opaque), class(TriClass::Glass), class(TriClass::Faded));
        let keep = |old: Option<&BlockGpu>, pick: fn(&BlockBytes) -> &Arc<Vec<u8>>, old_lines: fn(&BlockGpu) -> Option<&Batch>| {
            old.filter(|o| Arc::ptr_eq(pick(&o.bytes), pick(&bytes))).and_then(old_lines).cloned()
        };
        let mut fresh = |data: &[u8], label: &str| {
            *uploaded += data.len() as u64;
            upload(device, label, data, LINE_SIZE)
        };
        let lines = keep(old.as_ref(), |b| &b.lines, |b| b.lines.as_ref()).or_else(|| fresh(&bytes.lines, "sc_block_lines"));
        let hidden = keep(old.as_ref(), |b| &b.hidden, |b| b.hidden.as_ref()).or_else(|| fresh(&bytes.hidden, "sc_block_hidden"));
        BlockGpu { bytes, vertices, opaque, glass, faded, lines, hidden, culled: None }
    }

    fn class(&self, class: TriClass) -> Option<&ClassGpu> {
        match class {
            TriClass::Opaque => self.opaque.as_ref(),
            TriClass::Glass => self.glass.as_ref(),
            TriClass::Faded => self.faded.as_ref(),
        }
    }

    /// Draw `class`'s triangles with the pipeline already set (`tally`: what is counted).
    fn draw(&self, pass: &mut wgpu::RenderPass<'_>, class: TriClass, tally: &Tally) {
        match self.class(class) {
            Some(ClassGpu::Runs(runs)) => {
                let Some(vertices) = &self.vertices else { return };
                pass.set_vertex_buffer(0, vertices.slice(..));
                let runs = self.culled.as_ref().map_or(&runs[..], |c| &c[class as usize][..]);
                for r in runs.iter() {
                    pass.draw(r.clone(), 0..1);
                    tally.triangles(r.len() as u32, 1);
                }
            }
            Some(ClassGpu::Indexed { indices, count }) => {
                let Some(vertices) = &self.vertices else { return };
                pass.set_vertex_buffer(0, vertices.slice(..));
                pass.set_index_buffer(indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..*count, 0, 0..1);
                tally.triangles(*count, 1);
            }
            Some(ClassGpu::Unindexed(batch)) => {
                for (buffer, count) in &batch.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                    tally.triangles(*count, 1);
                }
            }
            None => {}
        }
    }
}

/// The group 0 bind of the scene that owns the data being drawn: its palette (and the shared
/// uniform), or the one-entry stand-in for draws that read no colours.
fn scene_bind<'a>(scene: &'a Batches, fallback: &'a wgpu::BindGroup) -> &'a wgpu::BindGroup {
    scene.palette.as_ref().map_or(fallback, |p| &p.bind)
}

/// The blocks' buffers, then the scene's own, as one batch (the blocks' buffers are shared,
/// not copied).
fn joined<'a>(blocks: impl Iterator<Item = &'a Option<Batch>>, own: Option<Batch>) -> Option<Batch> {
    let mut parts: Vec<(wgpu::Buffer, u32)> = blocks.flatten().flat_map(|b| b.parts.iter().cloned()).collect();
    let chunk = own.as_ref().map(|b| b.chunk).unwrap_or(0);
    parts.extend(own.into_iter().flat_map(|b| b.parts));
    (!parts.is_empty()).then_some(Batch { parts, chunk })
}

/// A scene's palette on the GPU: an `R32Uint` texture, [`PALETTE_WIDTH`] entries to a row.
struct PaletteGpu {
    palette: Arc<Palette>,
    /// Held for the bind group, which views it.
    _texture: wgpu::Texture,
    bind: wgpu::BindGroup,
    rows: u32,
    entries: Vec<[u8; 4]>,
}

fn palette_texture(device: &wgpu::Device, rows: u32) -> wgpu::Texture {
    device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_palette"),
        size: wgpu::Extent3d { width: PALETTE_WIDTH, height: rows.max(1), depth_or_array_layers: 1 },
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Uint,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    })
}

/// Rows `first..last` of `entries` as the texture's texels (zero past the end).
fn palette_bytes(entries: &[[u8; 4]], first: u32, last: u32) -> Vec<u8> {
    let width = PALETTE_WIDTH as usize;
    let mut out = Vec::with_capacity((last - first) as usize * width * 4);
    for row in first..last {
        for col in 0..PALETTE_WIDTH {
            let i = row as usize * width + col as usize;
            let texel = entries.get(i).map_or(0, |[r, g, b, a]| u32::from_le_bytes([*r, *g, *b, *a]));
            out.extend_from_slice(&texel.to_le_bytes());
        }
    }
    out
}

fn palette_rows(entries: &[[u8; 4]]) -> u32 {
    u32::try_from(entries.len().div_ceil(PALETTE_WIDTH as usize)).unwrap_or(1).max(1)
}

impl PaletteGpu {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniform: &wgpu::Buffer,
        bgl: &wgpu::BindGroupLayout,
        palette: Arc<Palette>,
        written: &mut u64,
    ) -> PaletteGpu {
        let entries = palette.entries().to_vec();
        let rows = palette_rows(&entries);
        let texture = palette_texture(device, rows);
        let bytes = palette_bytes(&entries, 0, rows);
        *written += bytes.len() as u64;
        queue.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &texture, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PALETTE_WIDTH * 4), rows_per_image: Some(rows) },
            wgpu::Extent3d { width: PALETTE_WIDTH, height: rows, depth_or_array_layers: 1 },
        );
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_palette"),
            layout: bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&view) },
            ],
        });
        PaletteGpu { palette, _texture: texture, bind, rows, entries }
    }

    /// Bring the texture up to `palette`, writing only the rows whose entries changed (or the
    /// whole texture when it must grow). The bind group is rebuilt only then.
    fn update(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniform: &wgpu::Buffer,
        bgl: &wgpu::BindGroupLayout,
        palette: Arc<Palette>,
        written: &mut u64,
    ) {
        if Arc::ptr_eq(&self.palette, &palette) {
            return;
        }
        let entries = palette.entries().to_vec();
        let rows = palette_rows(&entries);
        if rows != self.rows {
            *self = PaletteGpu::new(device, queue, uniform, bgl, palette, written);
            return;
        }
        let width = PALETTE_WIDTH as usize;
        let same = |row: u32| {
            let base = row as usize * width;
            (0..width).all(|c| self.entries.get(base + c).copied().unwrap_or_default() == entries.get(base + c).copied().unwrap_or_default())
        };
        let mut row = 0u32;
        while row < rows {
            if same(row) {
                row += 1;
                continue;
            }
            let first = row;
            while row < rows && !same(row) {
                row += 1;
            }
            let bytes = palette_bytes(&entries, first, row);
            *written += bytes.len() as u64;
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self._texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x: 0, y: first, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &bytes,
                wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(PALETTE_WIDTH * 4), rows_per_image: Some(row - first) },
                wgpu::Extent3d { width: PALETTE_WIDTH, height: row - first, depth_or_array_layers: 1 },
            );
        }
        self.entries = entries;
        self.palette = palette;
    }
}

/// Planned bodies end a GPU chunk one time in this many (by their plan's address), and a chunk
/// holds at most [`MAX_CHUNK_BODIES`]: a scene of a thousand bodies takes a few dozen dispatches
/// once, and a changed body re-evaluates a few dozen bodies, not all of them.
const CHUNK_SPLIT: u64 = 32;
const MAX_CHUNK_BODIES: usize = 128;

/// A block's see-through triangles as the global glass stream took them ([`Batches::glass_from`]).
struct GlassSource {
    key: u64,
    vertices: Arc<Vec<u8>>,
    glass: Arc<Vec<std::ops::Range<u32>>>,
}

#[derive(Default)]
struct Batches {
    /// What the last scene taken uploaded.
    uploads: UploadStats,
    /// The blocks' see-through triangles the global glass stream was assembled from (block key,
    /// vertices and glass runs, held so their addresses cannot be reused), when it holds nothing
    /// else: an edit that changes neither leaves the sorted stream as it is.
    glass_from: Option<Vec<GlassSource>>,
    /// The scene's palette texture and its group 0 bind (uniform + palette).
    palette: Option<PaletteGpu>,
    /// The scene blocks on the GPU, by key, and their drawing order.
    blocks: std::collections::HashMap<u64, BlockGpu>,
    block_order: Vec<u64>,
    key: Option<u64>,
    tris: Option<Batch>,
    pbr: Option<Batch>,
    trans: Option<Batch>,
    shadow: Option<Batch>,
    glass: Option<Batch>,
    /// The glass triangles as built, and the view direction they were last sorted for.
    glass_cpu: Vec<u8>,
    glass_sorted_for: Option<[f32; 3]>,
    /// The view the blocks' bodies were last culled for ([`Batches::cull`]).
    blocks_culled_for: Option<[[f32; 4]; 4]>,
    lines: Option<Batch>,
    hidden: Option<Batch>,
    overlays: Option<Batch>,
    ghost: Option<Batch>,
    xray: Option<Batch>,
    under: Option<Batch>,
    guide_trans: Option<Batch>,
    guide_lines: Option<Batch>,
    planned: Vec<PlannedDraw>,
    instanced: Vec<InstancedDraw>,
    /// Whole highlighted bodies redrawn from `planned` (the scene's own plans are not used).
    hover_planned: Vec<Arc<solidcraft_geom::tess::TessPlan>>,
    selected_planned: Vec<Arc<solidcraft_geom::tess::TessPlan>>,
    /// Whole highlighted bodies redrawn from the scene's blocks, by block key and vertex range.
    hover_blocks: Vec<BlockTris>,
    selected_blocks: Vec<BlockTris>,
    /// Whole faded or see-through bodies redrawn from the model's blocks in this scene's palette.
    recoloured: Vec<BlockTris>,
    /// Whole highlighted bodies drawn as placements of the scene's shared triangles.
    hover_instanced: Vec<InstancedDraw>,
    selected_instanced: Vec<InstancedDraw>,
    /// The instanced meshes' triangles (and their levels) on the GPU, by the address of the
    /// triangles they were uploaded from (held, so it stays theirs).
    instanced_tris: std::collections::HashMap<usize, (Arc<SharedTris>, Arc<SharedGpu>)>,
    /// Evaluated chunks by their plans' addresses, holding the plans so the addresses stay
    /// theirs: a chunk whose plans are unchanged is not evaluated again when the scene is rebuilt.
    resident: std::collections::HashMap<Vec<usize>, (Vec<Arc<solidcraft_geom::tess::TessPlan>>, Arc<crate::GpuMesh>)>,
}

impl Batches {
    /// Upload a new scene. Planned bodies not yet on the GPU are evaluated there: their
    /// dispatches go into `encoder`, ahead of this frame's render pass.
    fn take(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        uniform: &wgpu::Buffer,
        bgl: &wgpu::BindGroupLayout,
        key: u64,
        slot: &SceneSlot,
        tess: Option<(&crate::TessGpu, &mut wgpu::CommandEncoder)>,
        oit_available: bool,
    ) {
        if self.key == Some(key) {
            return;
        }
        if let Some(sc) = slot.lock().ok().and_then(|mut s| s.take()) {
            self.uploads = UploadStats::default();
            let palette = sc.palette.clone();
            let mut written = 0;
            match self.palette.take() {
                Some(mut p) => {
                    p.update(device, queue, uniform, bgl, palette, &mut written);
                    self.palette = Some(p);
                }
                None => self.palette = Some(PaletteGpu::new(device, queue, uniform, bgl, palette, &mut written)),
            }
            self.uploads.palette = written;
            self.planned = match tess {
                Some((tess, encoder)) => self.planned(device, tess, encoder, &sc.planned),
                None => Vec::new(),
            };
            self.instanced = self.instanced(device, &sc.instanced);
            self.hover_planned = sc.hover_planned;
            self.selected_planned = sc.selected_planned;
            self.hover_blocks = sc.hover_blocks;
            self.selected_blocks = sc.selected_blocks;
            self.recoloured = sc.recoloured;
            self.hover_instanced = self.instanced(device, &sc.hover_instanced);
            self.selected_instanced = self.instanced(device, &sc.selected_instanced);
            let mut kept = std::mem::take(&mut self.blocks);
            let mut order = Vec::with_capacity(sc.blocks.len());
            let mut geometry = 0;
            for block in &sc.blocks {
                let old = kept.remove(&block.key);
                self.blocks.insert(block.key, BlockGpu::update(device, old, block.bytes.clone(), &mut geometry));
                order.push(block.key);
            }
            self.block_order = order;
            let mut own = |label: &str, data: &[u8], stride: usize| {
                geometry += data.len() as u64;
                upload(device, label, data, stride)
            };
            self.tris = own("sc_tris", &sc.tris, TRI_SIZE);
            self.pbr = own("sc_pbr", &sc.pbr, PBR_SIZE);
            self.trans = own("sc_trans", &sc.trans, TRI_SIZE);
            self.shadow = own("sc_shadow", &sc.shadow, TRI_SIZE);
            let own_lines = own("sc_lines", &sc.lines, LINE_SIZE);
            let own_hidden = own("sc_hidden", &sc.hidden, LINE_SIZE);
            self.overlays = own("sc_overlay", &sc.overlay, LINE_SIZE);
            self.ghost = own("sc_ghost", &sc.ghost, TRI_SIZE);
            self.xray = own("sc_xray", &sc.xray, TRI_SIZE);
            self.under = own("sc_under", &sc.under, LINE_SIZE);
            self.guide_trans = own("sc_guide_trans", &sc.guide_trans, TRI_SIZE);
            self.guide_lines = own("sc_guide_lines", &sc.guide_lines, LINE_SIZE);
            let blocks: Vec<&BlockGpu> = self.block_order.iter().filter_map(|k| self.blocks.get(k)).collect();
            self.lines = joined(blocks.iter().map(|b| &b.lines), own_lines);
            self.hidden = joined(blocks.iter().map(|b| &b.hidden), own_hidden);
            // Without order-independent transparency one global glass stream is sorted per view,
            // so cross-body blend order is kept; the blocks' glass triangles join it here. With
            // OIT the blocks draw their own runs and nothing is sorted. A stream made of the same
            // blocks' glass as before (a colour or opacity edit, a selection) is kept as sorted.
            let from: Vec<GlassSource> = if oit_available {
                Vec::new()
            } else {
                sc.blocks
                    .iter()
                    .filter(|b| !b.bytes.glass.is_empty())
                    .map(|b| GlassSource { key: b.key, vertices: b.bytes.vertices.clone(), glass: b.bytes.glass.clone() })
                    .collect()
            };
            let same_glass = sc.glass.is_empty()
                && self.glass_from.as_ref().is_some_and(|was| {
                    was.len() == from.len()
                        && was
                            .iter()
                            .zip(&from)
                            .all(|(a, b)| a.key == b.key && Arc::ptr_eq(&a.vertices, &b.vertices) && Arc::ptr_eq(&a.glass, &b.glass))
                });
            if !same_glass {
                self.glass_cpu.clear();
                self.glass_cpu.extend_from_slice(&sc.glass);
                if !oit_available {
                    for block in &sc.blocks {
                        append_runs(&block.bytes.vertices, &block.bytes.glass, &mut self.glass_cpu);
                    }
                }
                geometry += self.glass_cpu.len() as u64;
                self.glass = upload_with(device, "sc_glass", &self.glass_cpu, TRI_SIZE, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST);
                self.glass_sorted_for = None;
                self.glass_from = sc.glass.is_empty().then_some(from);
            }
            self.uploads.geometry += geometry;
            self.blocks_culled_for = None;
            self.key = Some(key);
        }
    }

    /// The instanced meshes on the GPU: triangles uploaded once per key (kept while the key comes
    /// back with the same bytes), placements every time.
    fn instanced(&mut self, device: &wgpu::Device, meshes: &[InstancedMesh]) -> Vec<InstancedDraw> {
        let mut kept = std::mem::take(&mut self.instanced_tris);
        let mut out = Vec::with_capacity(meshes.len());
        for m in meshes {
            let count = u32::try_from(m.count()).unwrap_or(u32::MAX);
            if count == 0 {
                continue;
            }
            let placements = m.instances.get(..count as usize * INSTANCE_SIZE).unwrap_or_default().to_vec();
            let mut levels = Vec::with_capacity(1 + m.levels.len());
            for (i, (tris, error)) in std::iter::once((&m.tris, 0.0f32)).chain(m.levels.iter().map(|(t, e)| (t, *e))).enumerate() {
                let key = Arc::as_ptr(tris) as usize;
                let gpu = match kept.remove(&key).or_else(|| self.instanced_tris.remove(&key)).filter(|(b, _)| Arc::ptr_eq(b, tris)) {
                    Some(t) => t,
                    None => match SharedGpu::new(device, tris) {
                        Some(g) => {
                            self.uploads.geometry += (tris.vertices.len() + tris.indices.len() * 4) as u64;
                            (tris.clone(), Arc::new(g))
                        }
                        None => continue,
                    },
                };
                // Until the first cull, every placement is drawn in full.
                let Some(instances) = buffer_init(device, "sc_instances", &placements, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST)
                else {
                    continue;
                };
                self.uploads.placements += placements.len() as u64;
                let count = if i == 0 { count } else { 0 };
                levels.push(LevelDraw { tris: gpu.1.clone(), flat: tris.flat, error, instances, count });
                self.instanced_tris.insert(key, gpu);
            }
            if !levels.is_empty() {
                out.push(InstancedDraw { look: m.look, levels, bounds: m.tris.bounds, placements, shown: None });
            }
        }
        out
    }

    /// Leave the blocks' bodies that are out of the view `vp` out of their draws: each class's
    /// runs without those bodies' vertices. Worked out again only when the view changes; a block
    /// with every body in view draws its runs as they are.
    fn cull_blocks(&mut self, vp: &[[f32; 4]; 4]) {
        if self.blocks_culled_for.as_ref() == Some(vp) {
            return;
        }
        self.blocks_culled_for = Some(*vp);
        const ROWS: [[f32; 4]; 3] = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        for block in self.blocks.values_mut() {
            let hidden: Vec<std::ops::Range<u32>> =
                block.bytes.bodies.iter().filter(|b| !placed_box_in_view(&ROWS, &b.bounds, vp)).map(|b| b.vertices.clone()).collect();
            block.culled = (!hidden.is_empty())
                .then(|| [TriClass::Opaque, TriClass::Glass, TriClass::Faded].map(|class| runs_without(block.bytes.runs(class), &hidden)));
        }
    }

    /// Keep only the placements of the instanced meshes that may be seen in the view `vp` (see
    /// [`placed_box_in_view`]): a part placed many times, most of it out of the view, draws
    /// only what is in it. The placements are written again only when that set changes.
    /// Each placement's level of detail is chosen here too: the coarsest whose error stays under
    /// [`LOD_PX`] where it is drawn ([`placement_scale`], [`level_for`]), in a view of `size`
    /// pixels.
    fn cull(&mut self, queue: &wgpu::Queue, vp: &[[f32; 4]; 4], size: [f32; 2]) {
        self.cull_blocks(vp);
        let f = |b: &[u8], i: usize| b.get(i..i + 4).and_then(|x| x.try_into().ok()).map_or(0.0, f32::from_le_bytes);
        for d in &mut self.instanced {
            let errors: Vec<f32> = d.levels.iter().map(|l| l.error).collect();
            let mut shown: Vec<(u32, u8)> = d
                .placements
                .as_chunks::<INSTANCE_SIZE>()
                .0
                .iter()
                .enumerate()
                .filter_map(|(i, p)| {
                    let rows = [0, 1, 2].map(|r| [f(&p[..], 16 * r), f(&p[..], 16 * r + 4), f(&p[..], 16 * r + 8), f(&p[..], 16 * r + 12)]);
                    placed_box_in_view(&rows, &d.bounds, vp).then(|| {
                        let level = if errors.len() > 1 { level_for(&errors, placement_scale(&rows, &d.bounds, vp, size)) } else { 0 };
                        (i as u32, u8::try_from(level).unwrap_or(0))
                    })
                })
                .collect();
            // See-through placements back to front (by their box's centre), so each blends over
            // what lies behind it.
            if d.look != InstanceLook::Opaque {
                let depth = |i: u32| {
                    let p = d.placements.get(i as usize * INSTANCE_SIZE..(i as usize + 1) * INSTANCE_SIZE).unwrap_or_default();
                    let rows = [0, 1, 2].map(|r| [f(p, 16 * r), f(p, 16 * r + 4), f(p, 16 * r + 8), f(p, 16 * r + 12)]);
                    let [lo, hi] = d.bounds;
                    let c = [0, 1, 2].map(|k| (lo[k] + hi[k]) * 0.5);
                    let w = rows.map(|r| r[0] * c[0] + r[1] * c[1] + r[2] * c[2] + r[3]);
                    let clip: [f32; 4] = std::array::from_fn(|k| vp[0][k] * w[0] + vp[1][k] * w[1] + vp[2][k] * w[2] + vp[3][k]);
                    if clip[3].abs() > 1e-12 { clip[2] / clip[3] } else { f32::INFINITY }
                };
                shown.sort_by(|a, b| depth(b.0).total_cmp(&depth(a.0)).then(a.0.cmp(&b.0)));
            }
            if d.shown.as_ref() == Some(&shown) {
                continue;
            }
            for (k, level) in d.levels.iter_mut().enumerate() {
                let bytes: Vec<u8> = shown
                    .iter()
                    .filter(|(_, l)| usize::from(*l) == k)
                    .filter_map(|(i, _)| d.placements.get(*i as usize * INSTANCE_SIZE..(*i as usize + 1) * INSTANCE_SIZE))
                    .flatten()
                    .copied()
                    .collect();
                if !bytes.is_empty() {
                    queue.write_buffer(&level.instances, 0, &bytes);
                }
                level.count = u32::try_from(bytes.len() / INSTANCE_SIZE).unwrap_or(u32::MAX);
            }
            d.shown = Some(shown);
        }
    }

    /// The planned bodies on the GPU: in chunks of at most `MAX_BATCH` vertices (one for any
    /// ordinary scene), each uploaded and evaluated in one dispatch and drawn in one call. A
    /// chunk whose plans are unchanged is not evaluated again; colours are rebuilt each time.
    fn planned(
        &mut self,
        device: &wgpu::Device,
        tess: &crate::TessGpu,
        encoder: &mut wgpu::CommandEncoder,
        bodies: &[PlannedBody],
    ) -> Vec<PlannedDraw> {
        // Chunks end where a body's own plan says (or when full), so a body whose plan changes
        // (an edit, or a refinement as the view zooms in, #1320) re-evaluates only its own
        // chunk, not the whole scene.
        let mut chunks: Vec<Vec<&PlannedBody>> = Vec::new();
        let mut filled = 0usize;
        let mut open = false;
        for b in bodies {
            let n = b.plan.vertex_count();
            match chunks.last_mut() {
                Some(c) if open && filled + n <= crate::MAX_BATCH && c.first().is_some_and(|f| f.class == b.class) => c.push(b),
                _ => {
                    chunks.push(vec![b]);
                    filled = 0;
                }
            }
            filled += n;
            let mut h = std::collections::hash_map::DefaultHasher::new();
            std::hash::Hash::hash(&(Arc::as_ptr(&b.plan) as usize), &mut h);
            open = !std::hash::Hasher::finish(&h).is_multiple_of(CHUNK_SPLIT) && chunks.last().is_some_and(|c| c.len() < MAX_CHUNK_BODIES);
        }
        let key = |c: &[&PlannedBody]| c.iter().map(|b| Arc::as_ptr(&b.plan) as usize).collect::<Vec<usize>>();
        self.resident.retain(|k, _| chunks.iter().any(|c| key(c) == *k));
        let mut out = Vec::with_capacity(chunks.len());
        for chunk in &chunks {
            let mesh = match self.resident.get(&key(chunk)) {
                Some((_, m)) => m.clone(),
                None => {
                    let plans: Vec<&solidcraft_geom::tess::TessPlan> = chunk.iter().map(|b| &*b.plan).collect();
                    match tess.prepare_many(&plans) {
                        Ok(m) => {
                            self.uploads.evaluated += plans.iter().map(|p| p.vertex_count() as u64).sum::<u64>();
                            tess.record(encoder, &m);
                            let m = Arc::new(m);
                            self.resident.insert(key(chunk), (chunk.iter().map(|b| b.plan.clone()).collect(), m.clone()));
                            m
                        }
                        Err(e) => {
                            log::warn!("bodies are not drawn on the GPU: {e}");
                            continue;
                        }
                    }
                }
            };
            let mut bytes = Vec::with_capacity(mesh.vertex_face.len() * 4);
            for (part, b) in mesh.parts.iter().zip(chunk) {
                let last = b.face_colors.last().copied().unwrap_or(PaletteIndex(0));
                let faces = mesh.vertex_face.get(part.vertices.start as usize..part.vertices.end as usize).unwrap_or_default();
                for f in faces {
                    bytes.extend_from_slice(&b.face_colors.get(*f as usize).copied().unwrap_or(last).0.to_le_bytes());
                }
            }
            self.uploads.planned += bytes.len() as u64;
            if let Some((colors, _)) = upload(device, "sc_planned_colors", &bytes, 4).and_then(|c| c.parts.into_iter().next()) {
                let class = chunk.first().map_or(TriClass::Opaque, |b| b.class);
                out.push(PlannedDraw { mesh, colors, class, plans: key(chunk) });
            }
        }
        out
    }
}

/// The section's pipelines (see `Renderer::tri_cut`). The stencil's low bits ([`WINDING`]) count
/// the faces behind the cut on each pixel, its high bit ([`NOT_CAP`]) marks where no cap shows.
struct Cut {
    /// No colour or depth: counts the faces behind the cut, +1 per back face and -1 per front
    /// face, so the count is not 0 where the cut runs through a body, whatever the faces' order.
    wind: wgpu::RenderPipeline,
    /// Draws the faces and marks the stencil by their true depth: [`NOT_CAP`] where a front face
    /// is the visible surface. Unpushed: `FACE_BIAS`'s slope part would let a front face win a
    /// band along its edge with a back face seen through the cut, and the cap would skip it (#6328).
    mark: wgpu::RenderPipeline,
    /// Where the cut runs through a body but a front face won the marking: the back faces again,
    /// pulled toward the eye (`fs_solid_touch`), and marked as the cap where one wins. That front
    /// face is another body's lying on the far wall (a shaft in its bore), faceted another way,
    /// which won in streaks (#9407).
    touch: wgpu::RenderPipeline,
    /// The faces' depth again, pushed back by `FACE_BIAS` as unsectioned faces are (after
    /// `Renderer::section_reset`), so their edges draw over them (#1434). No colour, no stencil.
    push: wgpu::RenderPipeline,
    /// The cap again, only where marked, at the section plane's depth: depth only, as the mark
    /// drew the cap's colour (#6728).
    cap: wgpu::RenderPipeline,
}

/// The stencil bits of [`Cut::wind`]'s count (kept modulo 128).
const WINDING: u32 = 0x7f;
/// The stencil bit [`Cut::mark`] sets where a front face is the visible surface (no cap there).
const NOT_CAP: u32 = 0x80;

/// The viewport's pipelines and the scenes uploaded to one device, for one kind of target.
pub struct Renderer {
    /// What the last frame drew ([`Renderer::stats`]).
    stats: DrawStatsHandle,
    /// What the frame `prepare` drew offscreen (supersampled, or for see-through surfaces),
    /// published when `paint` copies it into the view.
    ss_stats: DrawStats,
    /// The AO depth prepass resources (a 1 × 1 stand-in while ambient occlusion is off).
    ao: ao::Resources,
    tri: wgpu::RenderPipeline,
    /// `tri` and `tri_cut` for planned bodies (positions and normals from the GPU evaluation,
    /// colours per vertex, indexed).
    tri_planned: wgpu::RenderPipeline,
    tri_cut_planned: Option<Cut>,
    /// `tri` and `tri_cut` for instanced meshes (shared triangles, a move and colour per
    /// placement).
    tri_instanced: wgpu::RenderPipeline,
    tri_cut_instanced: Option<Cut>,
    /// `tri_instanced` and `tri_cut_instanced` for flat triangles.
    tri_instanced_flat: wgpu::RenderPipeline,
    /// `glass_far`, `glass_near` and `trans` for instanced meshes, smooth and flat.
    glass_far_instanced: (wgpu::RenderPipeline, wgpu::RenderPipeline),
    glass_near_instanced: (wgpu::RenderPipeline, wgpu::RenderPipeline),
    trans_instanced: (wgpu::RenderPipeline, wgpu::RenderPipeline),
    tri_cut_instanced_flat: Option<Cut>,
    /// The tessellation compute pipeline, where the device runs compute shaders.
    tess: Option<crate::TessGpu>,
    /// The tessellation compute pipeline while it compiles beside the window ([`TessLater`]):
    /// `tess` once it is ready.
    #[cfg(not(target_arch = "wasm32"))]
    tess_later: Option<TessLater>,
    /// While a section is shown (when the depth buffer has a stencil), in place of `tri`: the
    /// faces marking where the cap shows, their depth pushed back, then the cap again at the
    /// section plane's depth, so it hides the inside ([`Cut`]).
    tri_cut: Option<Cut>,
    /// Highlight triangles over the model (depth test ≤, no depth write).
    tri_hl: wgpu::RenderPipeline,
    /// Whole highlighted bodies, redrawn from the model's planned chunks in the hover and the
    /// selection colour (depth test ≤, no depth write).
    tri_hl_hover: wgpu::RenderPipeline,
    tri_hl_selected: wgpu::RenderPipeline,
    /// The same for bodies the model builds on the CPU, from its blocks' triangle buffers.
    tri_hl_hover_built: wgpu::RenderPipeline,
    tri_hl_selected_built: wgpu::RenderPipeline,
    /// The same for bodies drawn as placements of shared triangles (the placement's colour is
    /// the highlight's), smooth and flat.
    tri_hl_instanced: wgpu::RenderPipeline,
    tri_hl_instanced_flat: wgpu::RenderPipeline,
    pbr: wgpu::RenderPipeline,
    trans: wgpu::RenderPipeline,
    /// Glass: far sides (front faces culled), then near sides (back faces culled).
    glass_far: wgpu::RenderPipeline,
    glass_near: wgpu::RenderPipeline,
    ghost: wgpu::RenderPipeline,
    xray: wgpu::RenderPipeline,
    line: wgpu::RenderPipeline,
    /// Lines behind the model (depth test >).
    line_hidden: wgpu::RenderPipeline,
    overlay: wgpu::RenderPipeline,
    /// Reference planes and axes: as `trans` and `line`, but not cut by a section.
    guide_trans: wgpu::RenderPipeline,
    guide_line: wgpu::RenderPipeline,
    uniform: wgpu::Buffer,
    /// The layout of group 0 (the uniform and a palette), for per-scene bind groups.
    uniform_bgl: wgpu::BindGroupLayout,
    /// Group 0 with a one-entry palette: draws that do not read colours (images, AO, depth).
    bind: wgpu::BindGroup,
    linear_out: bool,
    model: Batches,
    product: Batches,
    empty: Batches,
    sketch: Batches,
    highlight: Batches,
    preview: Batches,
    image: wgpu::RenderPipeline,
    image_bgl: wgpu::BindGroupLayout,
    sampler: wgpu::Sampler,
    /// Textures by image id: (version, bind group).
    textures: std::collections::HashMap<u64, (u64, wgpu::BindGroup)>,
    /// This frame's quads: (image id, vertices).
    quads: Vec<(u64, Batch)>,
    cloud_pipes: clouds::CloudPipes,
    clouds: clouds::CloudDraws,
    access_bgl: wgpu::BindGroupLayout,
    /// The accessibility depth map (a 1 × 1 stand-in when there is none) and its version.
    access_bind: wgpu::BindGroup,
    access_version: u64,
    /// The target the pipelines draw into (format, depth, samples): a supersampled frame is
    /// drawn into textures of the same kind.
    target: GpuTarget,
    /// The pass the picture is shown in (egui's): its samples and depth. Equal to [`Self::target`]
    /// unless the scene is drawn offscreen and blitted ([`Self::window_differs`]).
    window: GpuTarget,
    /// The device's largest 2D texture side (#869).
    max_texture: u32,
    /// Supersampling (#1288): the filter that draws the supersampled frame into the viewport.
    ss_blit: wgpu::RenderPipeline,
    /// The same filter into an offscreen texture of its own (no depth, one sample): the picture
    /// [`Renderer::cached`] keeps for still frames.
    ss_filter: wgpu::RenderPipeline,
    /// The frame drawn offscreen at the viewport's own size (for the see-through surfaces' order-
    /// independent transparency), copied as it is.
    copy_blit: wgpu::RenderPipeline,
    /// The offscreen frame is not at the viewport's size: filtered ([`Renderer::ss_blit`]).
    ss_scaled: bool,
    /// Order-independent transparency for the model's see-through surfaces (`None` where the
    /// device cannot blend into float targets: they are sorted instead), and its targets.
    oit: Option<oit::Pipelines>,
    oit_targets: Option<oit::Targets>,
    ss_bgl: wgpu::BindGroupLayout,
    /// The supersampled frame's textures, kept while its size stays.
    ss: Option<Supersampled>,
    /// This frame was drawn supersampled (in `prepare`): `paint` only filters it down.
    ss_ready: bool,
    /// What the picture in `ss` shows ([`frame_key`]): a frame whose key matches is blitted, not
    /// drawn again.
    ss_key: Option<u64>,
    /// The filtered picture at the viewport's own size, made on the first still frame and blitted
    /// as it is until the content changes: a still view does not pay for the filter every frame.
    cached: Option<CachedPicture>,
    /// The device refused the cached picture's texture: still frames are filtered every time.
    no_cache: bool,
    /// Most memory (bytes) an offscreen frame may take ([`frame_bytes`]): [`SS_BUDGET`], halved
    /// below each frame the device refused (out of video memory, #7296).
    frame_budget: u64,
    /// The device refused memory for a frame or its see-through targets ([`DrawStats::lowered`]).
    lowered: bool,
    /// Reversed depth drawn into the window's pass: egui clears its depth to 1, which is the near
    /// plane then, so the viewport first sets its rectangle to the far plane (0).
    depth_reset: Option<wgpu::RenderPipeline>,
    /// The scene target's depth back to the far plane between the section's marking and push
    /// passes ([`Cut`]); stencil and colour untouched.
    section_reset: Option<wgpu::RenderPipeline>,
    /// The scene target's stencil to 0 before the section's passes count faces in it ([`Cut::wind`]);
    /// depth and colour untouched.
    section_clear: Option<wgpu::RenderPipeline>,
    /// The new resolve textures [`clear_resolve_targets`] has cleared (tests count them).
    #[cfg(test)]
    resolve_clears: u32,
}

/// A triangle over the whole viewport at the far plane's depth (`FAR`), writing no colour:
/// [`depth_reset_pipeline`].
const DEPTH_RESET_SHADER: &str = r#"
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, FAR, 1.0);
}
@fragment
fn fs() -> @location(0) vec4<f32> {
    return vec4<f32>(0.0);
}
"#;

/// A supersampled frame: the scene drawn at a multiple of the viewport's pixels.
struct Supersampled {
    size: [u32; 2],
    /// Drawn into (multisampled with the viewport's MSAA).
    color: wgpu::TextureView,
    /// The resolved colour (when `color` is multisampled).
    resolve: Option<wgpu::TextureView>,
    depth: Option<wgpu::TextureView>,
    /// The single-sampled colour, for the filter.
    bind: wgpu::BindGroup,
}

/// A frame filtered down to the viewport's own pixels, for a still view to blit as it is.
struct CachedPicture {
    /// Kept for the bind group, which samples it.
    _view: wgpu::TextureView,
    bind: wgpu::BindGroup,
}

/// Most memory (bytes) a supersampled frame's colour and depth may take.
const SS_BUDGET: u64 = 384 << 20;

/// The least [`Renderer::frame_budget`] falls to after refused frames: about 256 × 256 pixels at
/// 4× MSAA.
const MIN_FRAME_BUDGET: u64 = 2 << 20;

/// How many smaller frames one frame asks for after the device refuses one.
const FRAME_TRIES: usize = 8;

/// The size (px) to draw a `size` viewport supersampled `factor` times in each direction:
/// within the device's largest texture side (#869) and the memory budget (at `samples` per pixel,
/// four bytes of colour and four of depth each). `None` when that leaves under 1.25×: the frame
/// is drawn as usual. A `factor` under 1 draws it that much smaller, to be filtered up.
pub fn supersampled_size(size: [f32; 2], factor: f32, max_side: u32, samples: u32) -> Option<[u32; 2]> {
    supersampled_within(size, factor, max_side, samples, SS_BUDGET)
}

/// [`supersampled_size`] within `budget` bytes instead of [`SS_BUDGET`].
fn supersampled_within(size: [f32; 2], factor: f32, max_side: u32, samples: u32, budget: u64) -> Option<[u32; 2]> {
    let [w, h] = size;
    if w >= 1.0 && h >= 1.0 && factor > 0.0 && factor < 1.0 {
        return Some([((w * factor).floor() as u32).clamp(1, max_side.max(1)), ((h * factor).floor() as u32).clamp(1, max_side.max(1))]);
    }
    if !(w >= 1.0 && h >= 1.0 && factor.is_finite() && factor > 1.0) {
        return None;
    }
    let side = max_side as f32;
    let bytes_per_px = 8.0 * samples.max(1) as f32;
    let by_memory = (budget as f32 / (w * h * bytes_per_px)).sqrt();
    let f = factor.min(side / w).min(side / h).min(by_memory);
    (f >= 1.25).then(|| [((w * f).floor() as u32).clamp(1, max_side), ((h * f).floor() as u32).clamp(1, max_side)])
}

/// The memory (bytes) a frame of `size` pixels at `samples` per pixel takes by the measure of
/// the budgets: four bytes of colour and four of depth per sample.
fn frame_bytes(size: [u32; 2], samples: u32) -> u64 {
    u64::from(size[0]) * u64::from(size[1]) * 8 * u64::from(samples.max(1))
}

/// `size` made smaller, keeping its shape, until its frame ([`frame_bytes`]) fits `budget`: a
/// device that refused a frame of the viewport's own size draws it smaller, filtered up (#7296).
/// At least 1 × 1.
fn fitted_size(size: [u32; 2], samples: u32, budget: u64) -> [u32; 2] {
    let bytes = frame_bytes(size, samples);
    if bytes <= budget {
        return size;
    }
    let f = (budget as f64 / bytes as f64).sqrt();
    size.map(|side| ((f64::from(side) * f).floor() as u32).max(1))
}

/// `make`'s value, or the error the device raised while it ran. A texture the device has no
/// memory for (#7296) would otherwise reach the uncaptured-error handler (a panic by default)
/// and be drawn into, invalid, every frame. A browser answers later and is not waited for (no
/// error is assumed).
fn allocated<T>(device: &wgpu::Device, make: impl FnOnce() -> T) -> Result<T, wgpu::Error> {
    let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
    let value = make();
    let mut failed = None;
    for scope in scopes.into_iter().rev() {
        if let Some(e) = crate::popped(scope.pop()) {
            failed.get_or_insert(e);
        }
    }
    failed.map_or(Ok(value), Err)
}

/// The filter: each viewport pixel the mean of the supersampled pixels under it (four bilinear
/// taps a quarter of its footprint from its centre: exactly the 2 × 2 box at 2×).
const SS_SHADER: &str = r#"
@group(0) @binding(0) var src: texture_2d<f32>;
@group(0) @binding(1) var smp: sampler;
struct O { @builtin(position) pos: vec4<f32>, @location(0) uv: vec2<f32> };
@vertex
fn vs(@builtin(vertex_index) i: u32) -> O {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    var o: O;
    o.pos = vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
    o.uv = vec2<f32>(x, y);
    return o;
}
/// The frame as it is (drawn at the viewport's own size).
@fragment
fn fs_copy(i: O) -> @location(0) vec4<f32> {
    return textureSampleLevel(src, smp, i.uv, 0.0);
}
@fragment
fn fs(i: O) -> @location(0) vec4<f32> {
    let d = vec2<f32>(dpdx(i.uv.x), dpdy(i.uv.y)) * 0.25;
    let a = textureSampleLevel(src, smp, i.uv + vec2<f32>(-d.x, -d.y), 0.0);
    let b = textureSampleLevel(src, smp, i.uv + vec2<f32>(d.x, -d.y), 0.0);
    let c = textureSampleLevel(src, smp, i.uv + vec2<f32>(-d.x, d.y), 0.0);
    let e = textureSampleLevel(src, smp, i.uv + vec2<f32>(d.x, d.y), 0.0);
    return (a + b + c + e) * 0.25;
}
"#;

fn access_texture(device: &wgpu::Device, queue: Option<&wgpu::Queue>, bgl: &wgpu::BindGroupLayout, size: u32, depth: &[f32]) -> wgpu::BindGroup {
    let extent = wgpu::Extent3d { width: size.max(1), height: size.max(1), depth_or_array_layers: 1 };
    let tex = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("sc_access"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::R32Float,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[],
    });
    if let Some(q) = queue
        && depth.len() == (size as usize) * (size as usize)
    {
        let bytes: Vec<u8> = depth.iter().flat_map(|x| x.to_le_bytes()).collect();
        q.write_texture(
            wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
            &bytes,
            wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * size), rows_per_image: Some(size) },
            extent,
        );
    }
    let view = tex.create_view(&Default::default());
    device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("sc_access"),
        layout: bgl,
        entries: &[wgpu::BindGroupEntry { binding: 2, resource: wgpu::BindingResource::TextureView(&view) }],
    })
}

pub const SHADER: &str = include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets/shaders/viewport.wgsl"));

fn tri_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32];
    wgpu::VertexBufferLayout { array_stride: TRI_SIZE as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

/// A planned body's vertices as the GPU evaluation writes them: the position, then the normal
/// at [`crate::NORMAL_OFFSET`] (each padded to four floats).
fn pbr_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Float32x3, 3 => Float32x3];
    wgpu::VertexBufferLayout { array_stride: PBR_SIZE as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

fn planned_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 2] = [
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: 0, shader_location: 0 },
        wgpu::VertexAttribute { format: wgpu::VertexFormat::Float32x3, offset: crate::NORMAL_OFFSET, shader_location: 1 },
    ];
    wgpu::VertexBufferLayout { array_stride: crate::VERTEX_STRIDE, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

/// An instanced mesh's vertices ([`SHARED_SIZE`]).
fn shared_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 2] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3];
    wgpu::VertexBufferLayout { array_stride: SHARED_SIZE as u64, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

/// An instanced mesh's placements ([`INSTANCE_SIZE`]), one per instance.
fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Uint32];
    wgpu::VertexBufferLayout { array_stride: INSTANCE_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
}

/// Where an opaque triangle pipeline reads its vertices from.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum TriSource {
    /// Built on the CPU ([`tri_layout`]).
    Built,
    /// Evaluated from display plans on the GPU ([`planned_layout`], [`color_layout`]), indexed.
    Planned,
    /// Shared vertices ([`shared_layout`]) drawn at each placement ([`instance_layout`]).
    Instanced,
    /// [`TriSource::Instanced`] for flat triangles: each fragment takes its triangle's normal.
    InstancedFlat,
}

impl TriSource {
    /// The vertex shader entry point.
    fn entry(self) -> &'static str {
        match self {
            TriSource::Instanced => "vs_tri_inst",
            TriSource::InstancedFlat => "vs_tri_inst_flat",
            TriSource::Built | TriSource::Planned => "vs_tri",
        }
    }

    /// The fragment shader entry point standing for `fs` (written for [`TriSource::Built`]).
    fn fragment(self, fs: &str) -> String {
        match self {
            TriSource::InstancedFlat => format!("{fs}_flat"),
            _ => fs.to_string(),
        }
    }
}

/// A planned body's colours, one per vertex.
fn color_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 1] = wgpu::vertex_attr_array![2 => Uint32];
    wgpu::VertexBufferLayout { array_stride: 4, step_mode: wgpu::VertexStepMode::Vertex, attributes: &A }
}

fn line_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 4] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x3, 2 => Uint32, 3 => Uint32];
    wgpu::VertexBufferLayout { array_stride: LINE_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
}

/// The target configuration: colour format, depth format and MSAA sample count of the app.
#[derive(Clone, Copy, Debug)]
pub struct GpuTarget {
    pub format: wgpu::TextureFormat,
    pub depth: Option<wgpu::TextureFormat>,
    pub samples: u32,
    /// The device evaluates display plans (compute shaders): bodies may be drawn from them.
    pub tess: bool,
    /// The renderer blends see-through surfaces order-independently: faded and see-through
    /// bodies may be drawn from their plans too (they need no sorting on the CPU). Set by the
    /// renderer ([`Renderer::target`]); asking for it does not make it so.
    pub oit: bool,
}

impl GpuTarget {
    /// Depth runs from 1 at the near plane to 0 at the far one: see [`reversed_depth`].
    pub fn reversed_depth(&self) -> bool {
        reversed_depth(self.depth)
    }
}

/// Is depth reversed (1 at the near plane, 0 at the far one) in a buffer of `format`? It is with a
/// 32-bit float buffer: a float's steps grow with its value, so with depth falling away from the
/// eye they follow the distance, about 2⁻²⁴ of it everywhere, instead of piling up at the near
/// plane (#1434: a face 0.3 mm in front of another speckled at 10 m). Other formats keep 0 near.
pub fn reversed_depth(format: Option<wgpu::TextureFormat>) -> bool {
    matches!(format, Some(wgpu::TextureFormat::Depth32Float | wgpu::TextureFormat::Depth32FloatStencil8))
}

/// The depth test `compare`, written for depth growing away from the eye, for a buffer whose depth
/// may run the other way ([`reversed_depth`]). Every pass drawn with the viewport's depth takes
/// its tests from here, so it agrees with the viewport whichever way depth runs.
pub fn depth_compare(reversed: bool, compare: wgpu::CompareFunction) -> wgpu::CompareFunction {
    use wgpu::CompareFunction::{Greater, GreaterEqual, Less, LessEqual};
    match compare {
        Less if reversed => Greater,
        LessEqual if reversed => GreaterEqual,
        Greater if reversed => Less,
        GreaterEqual if reversed => LessEqual,
        c => c,
    }
}

/// A depth bias written for depth growing away from the eye (negative: toward it), turned round
/// for reversed depth.
pub fn depth_bias(reversed: bool, b: wgpu::DepthBiasState) -> wgpu::DepthBiasState {
    if reversed { wgpu::DepthBiasState { constant: -b.constant, slope_scale: -b.slope_scale, clamp: -b.clamp } } else { b }
}

/// The depth of the far plane, which a depth buffer is cleared to.
pub fn far_depth(reversed: bool) -> f32 {
    if reversed { 0.0 } else { 1.0 }
}

/// The view-projection the GPU draws with: `vp` (depth 0 near, 1 far, as the CPU uses it), with
/// depth turned round (z' = w - z) for reversed depth. Columns, as `Mat4::to_f32` gives them.
pub fn gpu_view_proj(vp: [[f32; 4]; 4], reversed: bool) -> [[f32; 4]; 4] {
    if !reversed {
        return vp;
    }
    vp.map(|c| [c[0], c[1], c[3] - c[2], c[3]])
}

impl Renderer {
    /// The pipelines for target `t` on `device`; `tess` evaluates planned bodies on the GPU. The
    /// frame is drawn into `t` and shown in a pass of the same kind (one target).
    pub fn new(device: &wgpu::Device, t: GpuTarget, tess: Option<crate::TessGpu>) -> Self {
        Self::with_window_target(device, t, t, tess)
    }

    /// The pipelines for a frame drawn into `t` (the scene target) and shown in a pass of `window`'s
    /// kind. When the two differ (a 1× window pass with no depth, say, and a multisampled scene
    /// target), the scene is always drawn offscreen and blitted into the window's pass.
    pub fn with_window_target(device: &wgpu::Device, t: GpuTarget, window: GpuTarget, tess: Option<crate::TessGpu>) -> Self {
        let source = format!("{SHADER}\n{}", pbr::SOURCE);
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_viewport"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_viewport"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::VERTEX_FRAGMENT,
                    ty: wgpu::BindingType::Buffer { ty: wgpu::BufferBindingType::Uniform, has_dynamic_offset: false, min_binding_size: None },
                    count: None,
                },
                // The palette: a texture (not a storage buffer), so WebGL2 can draw it.
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Uint,
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
            ],
        });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_viewport"),
            bind_group_layouts: &[Some(&bgl)],
            immediate_size: 0,
        });
        let uniform = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_viewport_u"),
            size: UNIFORM_SIZE,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let dummy_palette = palette_texture(device, 1);
        let dummy_view = dummy_palette.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_viewport"),
            layout: &bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: uniform.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::TextureView(&dummy_view) },
            ],
        });
        let reversed = t.reversed_depth();
        let depth = |compare: wgpu::CompareFunction, write: bool| -> Option<wgpu::DepthStencilState> {
            t.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(write),
                depth_compare: Some(depth_compare(reversed, compare)),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        // The pass the picture is shown in, which may have no depth and one sample.
        let window_depth = |compare: wgpu::CompareFunction| -> Option<wgpu::DepthStencilState> {
            window.depth.map(|format| wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(false),
                depth_compare: Some(depth_compare(window.reversed_depth(), compare)),
                stencil: Default::default(),
                bias: Default::default(),
            })
        };
        // Opaque faces pushed back by their own slope (FACE_BIAS); ghosts twice as far.
        let pushed = |compare: wgpu::CompareFunction, write: bool, bias: wgpu::DepthBiasState| -> Option<wgpu::DepthStencilState> {
            depth(compare, write).map(|d| wgpu::DepthStencilState { bias: depth_bias(reversed, bias), ..d })
        };
        let pipeline = |label: &str,
                        vs: &str,
                        fs: &str,
                        buf: wgpu::VertexBufferLayout<'static>,
                        ds: Option<wgpu::DepthStencilState>,
                        blend: Option<wgpu::BlendState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState { module: &module, entry_point: Some(vs), buffers: &[Some(buf)], compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: ds,
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(fs),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let alpha = Some(wgpu::BlendState::ALPHA_BLENDING);
        let glass = |label: &str, cull: wgpu::Face| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_tri"),
                    buffers: &[Some(tri_layout())],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: Some(cull), ..Default::default() },
                depth_stencil: depth(wgpu::CompareFunction::LessEqual, false),
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_tri"),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: alpha, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        // See-through instanced meshes: blended, depth tested, not written; `cull` for glass.
        let placed_see_through = [Some(shared_layout()), Some(instance_layout())];
        let see_through = |label: &str, source: TriSource, cull: Option<wgpu::Face>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(source.entry()),
                    buffers: &placed_see_through,
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: cull, ..Default::default() },
                depth_stencil: depth(wgpu::CompareFunction::LessEqual, false),
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(&source.fragment("fs_tri")),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: alpha, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let image_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_image"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let image_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_image"),
            bind_group_layouts: &[Some(&bgl), Some(&image_bgl)],
            immediate_size: 0,
        });
        const IA: [wgpu::VertexAttribute; 3] = wgpu::vertex_attr_array![0 => Float32x3, 1 => Float32x2, 2 => Float32];
        let image = || {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_image"),
                layout: Some(&image_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some("vs_img"),
                    buffers: &[Some(wgpu::VertexBufferLayout {
                        array_stride: IMG_SIZE as u64,
                        step_mode: wgpu::VertexStepMode::Vertex,
                        attributes: &IA,
                    })],
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil: depth(wgpu::CompareFunction::LessEqual, false),
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some("fs_img"),
                    targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: alpha, write_mask: wgpu::ColorWrites::ALL })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let access_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_access"),
            entries: &[wgpu::BindGroupLayoutEntry {
                binding: 2,
                visibility: wgpu::ShaderStages::FRAGMENT,
                ty: wgpu::BindingType::Texture {
                    sample_type: wgpu::TextureSampleType::Float { filterable: false },
                    view_dimension: wgpu::TextureViewDimension::D2,
                    multisampled: false,
                },
                count: None,
            }],
        });
        let ao_layout = ao::Resources::layout(device);
        let ao = || ao::Resources::new(device, &ao_layout, &bgl, &module, reversed);
        let solid_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_solid"),
            bind_group_layouts: &[Some(&bgl), Some(&access_bgl), Some(&ao_layout)],
            immediate_size: 0,
        });
        let built = [Some(tri_layout())];
        let evaluated = [Some(planned_layout()), Some(color_layout())];
        let placed = [Some(shared_layout()), Some(instance_layout())];
        // `colour`: whether the pipeline writes colour (the cap's depth pass writes none).
        let solid_writes = |label: &str, fs: &str, cull: Option<wgpu::Face>, ds: Option<wgpu::DepthStencilState>, source: TriSource, colour: bool| {
            let fs = source.fragment(fs);
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some(label),
                layout: Some(&solid_layout),
                vertex: wgpu::VertexState {
                    module: &module,
                    entry_point: Some(source.entry()),
                    buffers: match source {
                        TriSource::Built => &built[..],
                        TriSource::Planned => &evaluated[..],
                        TriSource::Instanced | TriSource::InstancedFlat => &placed[..],
                    },
                    compilation_options: Default::default(),
                },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, cull_mode: cull, ..Default::default() },
                depth_stencil: ds,
                multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &module,
                    entry_point: Some(&fs),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: t.format,
                        blend: None,
                        write_mask: if colour { wgpu::ColorWrites::ALL } else { wgpu::ColorWrites::empty() },
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let solid = |label: &str, fs: &str, cull: Option<wgpu::Face>, ds: Option<wgpu::DepthStencilState>, source: TriSource| {
            solid_writes(label, fs, cull, ds, source, true)
        };
        // While a section is shown, the depth states of the stencil marking and of the cap (see
        // `Renderer::tri_cut`), where the depth buffer has a stencil.
        let cut_states = t.depth.filter(|f| f.has_stencil_aspect()).map(|format| {
            use wgpu::StencilOperation::{DecrementWrap, IncrementWrap, Keep, Replace, Zero};
            let face = |pass_op| wgpu::StencilFaceState { compare: wgpu::CompareFunction::Always, fail_op: Keep, depth_fail_op: Keep, pass_op };
            // First the faces behind the cut are counted, back +1 and front -1, whatever their depth.
            let wind = wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(false),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState { front: face(DecrementWrap), back: face(IncrementWrap), read_mask: 0xff, write_mask: WINDING },
                bias: Default::default(),
            };
            // The visible surface, by its true depth, marks the stencil: `NOT_CAP` where it is a
            // front face (the reference), cleared where it is a back face (the cap).
            let mark = wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(depth_compare(reversed, wgpu::CompareFunction::Less)),
                stencil: wgpu::StencilState { front: face(Replace), back: face(Zero), read_mask: 0xff, write_mask: NOT_CAP },
                bias: Default::default(),
            };
            // Where a front face won but the count says the cut runs through a body (`NOT_CAP` set
            // and a count: above the reference `NOT_CAP`), a back face pulled in front of it is the
            // cap after all.
            let through = wgpu::StencilFaceState { compare: wgpu::CompareFunction::Less, ..face(Zero) };
            let touch = wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(depth_compare(reversed, wgpu::CompareFunction::LessEqual)),
                stencil: wgpu::StencilState { front: through, back: through, read_mask: 0xff, write_mask: NOT_CAP },
                bias: Default::default(),
            };
            // Then the cap's back faces again, only there (`NOT_CAP` clear, the reference 0), at the
            // section plane's depth.
            let only = wgpu::StencilFaceState { compare: wgpu::CompareFunction::Equal, ..face(Keep) };
            let cap = wgpu::DepthStencilState {
                format,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState { front: only, back: only, read_mask: NOT_CAP, write_mask: 0 },
                bias: Default::default(),
            };
            (wind, mark, touch, cap)
        });
        // `Cut::wind`, `Cut::mark` and `Cut::push` use `tri`'s fragment shader; `Cut::touch` and
        // `Cut::cap` have their own.
        let marks = |source: TriSource| {
            cut_states.as_ref().map(|(wind, mark, _, _)| {
                (
                    solid_writes("sc_tris_wind", "fs_solid", None, Some(wind.clone()), source, false),
                    solid("sc_tris_mark", "fs_solid", None, Some(mark.clone()), source),
                    solid_writes("sc_tris_push", "fs_solid", None, pushed(wgpu::CompareFunction::Less, true, FACE_BIAS), source, false),
                )
            })
        };
        // Depth only: every back face behind a marked pixel draws here, so writing colour let the
        // last one drawn paint the cut, other bodies behind it included (their colours and
        // outlines showed through it, #6328). The marking pass already drew the nearest back
        // face's colour.
        let cap = |source: TriSource| {
            cut_states.as_ref().map(|(_, _, touch, cap)| {
                (
                    solid("sc_tris_touch", "fs_solid_touch", Some(wgpu::Face::Front), Some(touch.clone()), source),
                    solid_writes("sc_tris_cap", "fs_solid_cut", Some(wgpu::Face::Front), Some(cap.clone()), source, false),
                )
            })
        };
        type Marks = (wgpu::RenderPipeline, wgpu::RenderPipeline, wgpu::RenderPipeline);
        let cut = |marks: Option<Marks>, cap: Option<(wgpu::RenderPipeline, wgpu::RenderPipeline)>| {
            marks.zip(cap).map(|((wind, mark, push), (touch, cap))| Cut { wind, mark, touch, push, cap })
        };
        let access_bind = access_texture(device, None, &access_bgl, 1, &[]);
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("sc_image"),
            mag_filter: wgpu::FilterMode::Linear,
            min_filter: wgpu::FilterMode::Linear,
            ..Default::default()
        });
        let ss_module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_ssaa"), source: wgpu::ShaderSource::Wgsl(SS_SHADER.into()) });
        let ss_bgl = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_ssaa"),
            entries: &[
                wgpu::BindGroupLayoutEntry {
                    binding: 0,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: true },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
                wgpu::BindGroupLayoutEntry {
                    binding: 1,
                    visibility: wgpu::ShaderStages::FRAGMENT,
                    ty: wgpu::BindingType::Sampler(wgpu::SamplerBindingType::Filtering),
                    count: None,
                },
            ],
        });
        let ss_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_ssaa"),
            bind_group_layouts: &[Some(&ss_bgl)],
            immediate_size: 0,
        });
        // The frame is premultiplied (drawn over transparent black), so it goes over the
        // viewport's background premultiplied; the viewport's depth is left as it is.
        let blit = |entry: &str, samples: u32, depth_stencil: Option<wgpu::DepthStencilState>| {
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("sc_ssaa"),
                layout: Some(&ss_layout),
                vertex: wgpu::VertexState { module: &ss_module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
                primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
                depth_stencil,
                multisample: wgpu::MultisampleState { count: samples, mask: !0, alpha_to_coverage_enabled: false },
                fragment: Some(wgpu::FragmentState {
                    module: &ss_module,
                    entry_point: Some(entry),
                    targets: &[Some(wgpu::ColorTargetState {
                        format: t.format,
                        blend: Some(wgpu::BlendState::PREMULTIPLIED_ALPHA_BLENDING),
                        write_mask: wgpu::ColorWrites::ALL,
                    })],
                    compilation_options: Default::default(),
                }),
                multiview_mask: None,
                cache: None,
            })
        };
        let pass_depth = window_depth(wgpu::CompareFunction::Always);
        let blits = || {
            (
                blit("fs", window.samples.max(1), pass_depth.clone()),
                blit("fs_copy", window.samples.max(1), pass_depth.clone()),
                // Into the cached picture's own texture: no depth, one sample.
                blit("fs", 1, None),
            )
        };
        let oit = || oit::Pipelines::new(device, &module, &layout, t.format, t.depth, t.samples);
        let resets = || {
            (
                window.reversed_depth().then(|| depth_reset_pipeline(device, window)).flatten(),
                t.depth.filter(|f| f.has_stencil_aspect()).and_then(|_| depth_reset_pipeline(device, t)),
                t.depth.filter(|f| f.has_stencil_aspect()).and_then(|_| stencil_clear_pipeline(device, t)),
            )
        };
        let opaque = || pushed(wgpu::CompareFunction::Less, true, FACE_BIAS);
        // One job per fragment shader, the costly part: wgpu compiles a shader for the first
        // pipeline that uses it and reuses it for the next, so each is still compiled once. A
        // vertex shader two jobs share may be compiled by both; those are small.
        let tris = || {
            (
                solid("sc_tris", "fs_solid", None, opaque(), TriSource::Built),
                marks(TriSource::Built),
                solid("sc_tris_planned", "fs_solid", None, opaque(), TriSource::Planned),
                marks(TriSource::Planned),
                solid("sc_tris_instanced", "fs_solid", None, opaque(), TriSource::Instanced),
                marks(TriSource::Instanced),
            )
        };
        let caps = || (cap(TriSource::Built), cap(TriSource::Planned), cap(TriSource::Instanced));
        let flat = || (solid("sc_tris_instanced_flat", "fs_solid", None, opaque(), TriSource::InstancedFlat), marks(TriSource::InstancedFlat));
        let flat_cap = || cap(TriSource::InstancedFlat);
        let hover = || {
            (
                solid("sc_tris_hl_hover", "fs_hl_hover", None, highlight_depth(t.depth), TriSource::Planned),
                solid("sc_tris_hl_hover_built", "fs_hl_hover", None, highlight_depth(t.depth), TriSource::Built),
            )
        };
        let selected = || {
            (
                solid("sc_tris_hl_selected", "fs_hl_selected", None, highlight_depth(t.depth), TriSource::Planned),
                solid("sc_tris_hl_selected_built", "fs_hl_selected", None, highlight_depth(t.depth), TriSource::Built),
            )
        };
        // `glass_far_instanced`, `glass_near_instanced` and `trans_instanced`, smooth or flat.
        let see_through_instanced = |source: TriSource, suffix: &str| {
            (
                see_through(&format!("sc_glass_far_instanced{suffix}"), source, Some(wgpu::Face::Front)),
                see_through(&format!("sc_glass_near_instanced{suffix}"), source, Some(wgpu::Face::Back)),
                see_through(&format!("sc_trans_instanced{suffix}"), source, None),
            )
        };
        let see_through_tris = || {
            (
                pipeline("sc_tris_hl", "vs_tri", "fs_tri", tri_layout(), highlight_depth(t.depth), alpha),
                pipeline("sc_trans", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
                glass("sc_glass_far", wgpu::Face::Front),
                glass("sc_glass_near", wgpu::Face::Back),
                pipeline("sc_ghost", "vs_tri", "fs_tri", tri_layout(), pushed(wgpu::CompareFunction::LessEqual, false, GHOST_BIAS), alpha),
                pipeline("sc_xray", "vs_tri", "fs_tri", tri_layout(), depth(wgpu::CompareFunction::Always, false), alpha),
                see_through_instanced(TriSource::Instanced, ""),
                solid("sc_tris_hl_instanced", "fs_tri", None, highlight_depth(t.depth), TriSource::Instanced),
            )
        };
        let see_through_flat = || {
            (
                see_through_instanced(TriSource::InstancedFlat, "_flat"),
                solid("sc_tris_hl_instanced_flat", "fs_tri", None, highlight_depth(t.depth), TriSource::InstancedFlat),
            )
        };
        let pbr = || pipeline("sc_pbr", "vs_pbr", "fs_pbr", pbr_layout(), opaque(), None);
        let lines = || {
            (
                pipeline("sc_lines", "vs_line", "fs_line", line_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
                pipeline("sc_lines_hidden", "vs_line", "fs_line", line_layout(), depth(wgpu::CompareFunction::Greater, false), alpha),
            )
        };
        let lines_all = || {
            (
                pipeline("sc_overlay", "vs_line", "fs_line_all", line_layout(), depth(wgpu::CompareFunction::Always, false), alpha),
                pipeline("sc_guide_lines", "vs_line", "fs_line_all", line_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha),
            )
        };
        let cloud_pipes = || clouds::CloudPipes::new(device, &module, &bgl, t, depth(wgpu::CompareFunction::Less, true));
        let guide_trans = || pipeline("sc_guide_trans", "vs_tri", "fs_tri_all", tri_layout(), depth(wgpu::CompareFunction::LessEqual, false), alpha);
        // Each job on a thread of its own where the device allows it: D3D12 compiles every shader
        // as its pipeline is made, which took seconds one after another (#4860).
        crate::concurrent::scope(device, |jobs| {
            let tris = jobs.spawn(tris);
            let caps = jobs.spawn(caps);
            let flat = jobs.spawn(flat);
            let flat_cap = jobs.spawn(flat_cap);
            let hover = jobs.spawn(hover);
            let selected = jobs.spawn(selected);
            let see_through_tris = jobs.spawn(see_through_tris);
            let see_through_flat = jobs.spawn(see_through_flat);
            let pbr = jobs.spawn(pbr);
            let lines = jobs.spawn(lines);
            let lines_all = jobs.spawn(lines_all);
            let guide_trans = jobs.spawn(guide_trans);
            let cloud_pipes = jobs.spawn(cloud_pipes);
            let image = jobs.spawn(image);
            let ao = jobs.spawn(ao);
            let oit = jobs.spawn(oit);
            let blits = jobs.spawn(blits);
            let resets = jobs.spawn(resets);
            let (tri, tri_marks, tri_planned, planned_marks, tri_instanced, instanced_marks) = tris.join();
            let (tri_cap, planned_cap, instanced_cap) = caps.join();
            let (tri_instanced_flat, flat_marks) = flat.join();
            let (tri_hl_hover, tri_hl_hover_built) = hover.join();
            let (tri_hl_selected, tri_hl_selected_built) = selected.join();
            let (tri_hl, trans, glass_far, glass_near, ghost, xray, (glass_far_smooth, glass_near_smooth, trans_smooth), tri_hl_instanced) =
                see_through_tris.join();
            let ((glass_far_flat, glass_near_flat, trans_flat), tri_hl_instanced_flat) = see_through_flat.join();
            let (line, line_hidden) = lines.join();
            let (overlay, guide_line) = lines_all.join();
            let (ss_blit, copy_blit, ss_filter) = blits.join();
            let (depth_reset, section_reset, section_clear) = resets.join();
            let oit = oit.join();
            Renderer {
                ao: ao.join(),
                depth_reset,
                section_reset,
                section_clear,
                #[cfg(test)]
                resolve_clears: 0,
                target: GpuTarget { oit: oit.is_some(), ..t },
                window,
                max_texture: device.limits().max_texture_dimension_2d,
                ss_blit,
                ss_filter,
                copy_blit,
                ss_scaled: false,
                oit,
                oit_targets: None,
                ss_bgl,
                ss: None,
                ss_ready: false,
                stats: DrawStatsHandle::default(),
                ss_stats: DrawStats::default(),
                ss_key: None,
                cached: None,
                no_cache: false,
                frame_budget: SS_BUDGET,
                lowered: false,
                image: image.join(),
                image_bgl,
                sampler,
                textures: Default::default(),
                quads: Vec::new(),
                cloud_pipes: cloud_pipes.join(),
                clouds: Default::default(),
                tri,
                tri_cut: cut(tri_marks, tri_cap),
                tri_planned,
                tri_cut_planned: cut(planned_marks, planned_cap),
                tri_instanced,
                tri_cut_instanced: cut(instanced_marks, instanced_cap),
                tri_instanced_flat,
                tri_cut_instanced_flat: cut(flat_marks, flat_cap.join()),
                glass_far_instanced: (glass_far_smooth, glass_far_flat),
                glass_near_instanced: (glass_near_smooth, glass_near_flat),
                trans_instanced: (trans_smooth, trans_flat),
                tess,
                #[cfg(not(target_arch = "wasm32"))]
                tess_later: None,
                access_bgl,
                access_bind,
                access_version: 0,
                tri_hl,
                tri_hl_hover,
                tri_hl_selected,
                tri_hl_hover_built,
                tri_hl_selected_built,
                tri_hl_instanced,
                tri_hl_instanced_flat,
                pbr: pbr.join(),
                trans,
                glass_far,
                glass_near,
                ghost,
                xray,
                line,
                line_hidden,
                overlay,
                guide_trans: guide_trans.join(),
                guide_line,
                uniform,
                uniform_bgl: bgl.clone(),
                bind,
                linear_out: t.format.is_srgb(),
                model: Batches::default(),
                product: Batches::default(),
                empty: Batches::default(),
                sketch: Batches::default(),
                highlight: Batches::default(),
                preview: Batches::default(),
            }
        })
    }
}

/// Depth state of faces lit over the model (hover, selection): tested, not written, and pulled a
/// little toward the eye. They lie on the model's own faces, but the GPU evaluates a body's plan
/// in f32 while the highlight's copy of the same triangles is evaluated on the CPU, so the two
/// depths differ by rounding and an unbiased equal test lights the face in speckles and wedges.
/// The bias is a few depth steps plus the face's own slope: far less than any real gap.
fn highlight_depth(format: Option<wgpu::TextureFormat>) -> Option<wgpu::DepthStencilState> {
    let reversed = reversed_depth(format);
    format.map(|format| wgpu::DepthStencilState {
        format,
        depth_write_enabled: Some(false),
        depth_compare: Some(depth_compare(reversed, wgpu::CompareFunction::LessEqual)),
        stencil: Default::default(),
        bias: depth_bias(reversed, HIGHLIGHT_BIAS),
    })
}

/// See [`highlight_depth`]. Negative: toward the eye (depth grows away from it).
const HIGHLIGHT_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState { constant: -8, slope_scale: -1.0, clamp: 0.0 };

/// Opaque faces are drawn pushed back from the eye by a few depth steps and by their own depth
/// slope over [`FACE_SLOPE`] pixels, so whatever lies on a face (its edges, an origin plane, a
/// sketch, a decal) is drawn over it at any angle, and an edge behind it stays hidden however
/// large the scene (#1434, #1739). The CPU renderer has the same rule (`raster.rs`, #983).
/// Lines used to be pulled toward the eye by a share of the depth range instead, which on a large
/// design reached tens of millimetres. Positive: away from the eye.
pub const FACE_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState { constant: FACE_STEPS, slope_scale: FACE_SLOPE, clamp: 0.0 };
/// The depth steps of [`FACE_BIAS`]: f32 rounding between a face and what lies on it (the GPU
/// evaluates planned bodies in f32; edges and overlays come from the CPU).
pub const FACE_STEPS: i32 = 8;
/// The pixels of [`FACE_BIAS`]: half a 2 px line, so edges (1.2 px) draw whole on a steep face.
/// It is also how far behind a face an edge may show, in that face's own depth per pixel: at 10 m
/// a pixel is about 9 mm, and a face turned 25° gains 4 mm of depth across it, so a larger value
/// let a washer 12 mm behind a plate show through it (#1434). A thicker line (a selected edge,
/// 3.5 px) may lose its outer edge on one side where it lies on a steep face. The section cap,
/// which writes its own depth, adds the same in the shader.
pub const FACE_SLOPE: f32 = 1.0;
/// The removed material of a preview, drawn translucent: behind the faces it shares with the new
/// shape, so twice [`FACE_BIAS`].
const GHOST_BIAS: wgpu::DepthBiasState = wgpu::DepthBiasState { constant: 2 * FACE_STEPS, slope_scale: 2.0 * FACE_SLOPE, clamp: 0.0 };

/// A pipeline that sets the depth of the viewport in a pass of `t`'s kind back to the far plane,
/// leaving colour and stencil as they are. None without a depth buffer.
fn depth_reset_pipeline(device: &wgpu::Device, t: GpuTarget) -> Option<wgpu::RenderPipeline> {
    viewport_fill_pipeline(device, t, "sc_depth_reset", true, Default::default())
}

/// A pipeline that sets the stencil of the viewport in a pass of `t`'s kind to 0, leaving colour
/// and depth as they are. None without a depth buffer.
fn stencil_clear_pipeline(device: &wgpu::Device, t: GpuTarget) -> Option<wgpu::RenderPipeline> {
    let zero = wgpu::StencilFaceState {
        compare: wgpu::CompareFunction::Always,
        fail_op: wgpu::StencilOperation::Keep,
        depth_fail_op: wgpu::StencilOperation::Keep,
        pass_op: wgpu::StencilOperation::Zero,
    };
    viewport_fill_pipeline(device, t, "sc_stencil_clear", false, wgpu::StencilState { front: zero, back: zero, read_mask: 0xff, write_mask: 0xff })
}

/// A triangle over the whole viewport in a pass of `t`'s kind, writing no colour: the far plane's
/// depth where `depth`, and `stencil`'s operations.
fn viewport_fill_pipeline(
    device: &wgpu::Device,
    t: GpuTarget,
    label: &str,
    depth: bool,
    stencil: wgpu::StencilState,
) -> Option<wgpu::RenderPipeline> {
    let format = t.depth?;
    let far = if t.reversed_depth() { "0.0" } else { "1.0" };
    let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(DEPTH_RESET_SHADER.replace("FAR", far).into()),
    });
    let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor { label: Some(label), bind_group_layouts: &[], immediate_size: 0 });
    Some(device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
        label: Some(label),
        layout: Some(&layout),
        vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
        primitive: wgpu::PrimitiveState { topology: wgpu::PrimitiveTopology::TriangleList, ..Default::default() },
        depth_stencil: Some(wgpu::DepthStencilState {
            format,
            depth_write_enabled: Some(depth),
            depth_compare: Some(wgpu::CompareFunction::Always),
            stencil,
            bias: Default::default(),
        }),
        multisample: wgpu::MultisampleState { count: t.samples.max(1), mask: !0, alpha_to_coverage_enabled: false },
        fragment: Some(wgpu::FragmentState {
            module: &module,
            entry_point: Some("fs"),
            targets: &[Some(wgpu::ColorTargetState { format: t.format, blend: None, write_mask: wgpu::ColorWrites::empty() })],
            compilation_options: Default::default(),
        }),
        multiview_mask: None,
        cache: None,
    }))
}

fn upload(device: &wgpu::Device, label: &str, bytes: &[u8], stride: usize) -> Option<Batch> {
    upload_with(device, label, bytes, stride, wgpu::BufferUsages::VERTEX)
}

fn upload_with(device: &wgpu::Device, label: &str, bytes: &[u8], stride: usize, usage: wgpu::BufferUsages) -> Option<Batch> {
    if bytes.len() < stride || stride == 0 {
        return None;
    }
    let chunk = buffer_chunk(stride, device.limits().max_buffer_size);
    let parts = bytes
        .chunks(chunk)
        .filter(|part| part.len() >= stride)
        .map(|part| Some((buffer_init(device, label, part, usage)?, u32::try_from(part.len() / stride).unwrap_or(0))))
        .collect::<Option<Vec<_>>>()?;
    Some(Batch { parts, chunk })
}

/// As `create_buffer_init`, without its panic when the device refused the buffer (an allocation
/// error caught by an error scope): `None` instead, so the upload fails.
fn buffer_init(device: &wgpu::Device, label: &str, contents: &[u8], usage: wgpu::BufferUsages) -> Option<wgpu::Buffer> {
    let len = contents.len() as u64;
    let size = len.div_ceil(wgpu::COPY_BUFFER_ALIGNMENT) * wgpu::COPY_BUFFER_ALIGNMENT;
    let buffer = device.create_buffer(&wgpu::BufferDescriptor { label: Some(label), size, usage, mapped_at_creation: true });
    match buffer.get_mapped_range_mut(..len) {
        Ok(mut mapped) => mapped.copy_from_slice(contents),
        Err(error) => {
            log::error!("geometry upload failed: {error}");
            return None;
        }
    }
    buffer.unmap();
    Some(buffer)
}

/// Bytes per GPU buffer for vertices of `stride` bytes: within the device's limit and a
/// whole number of triangles and line pairs, so no primitive straddles two buffers.
fn buffer_chunk(stride: usize, max_buffer_size: u64) -> usize {
    let unit = stride.max(1) * 6;
    let max = usize::try_from(max_buffer_size).unwrap_or(usize::MAX).min(1 << 30);
    (max / unit).max(1) * unit
}

/// Triangles (3 vertices of `TRI_SIZE` bytes each, position first) ordered far to near along
/// the view direction `back` (toward the eye), so blended glass draws back to front.
pub fn sort_back_to_front(bytes: &[u8], back: [f32; 3]) -> Vec<u8> {
    let tri = TRI_SIZE * 3;
    let depth = |t: &[u8]| -> f32 {
        let f = |o: usize| t.get(o..o + 4).and_then(|b| b.try_into().ok()).map_or(0.0, f32::from_le_bytes);
        (0..3).map(|v| (0..3).map(|k| f(v * TRI_SIZE + k * 4) * back[k]).sum::<f32>()).sum::<f32>()
    };
    let mut tris: Vec<(f32, &[u8])> = bytes.chunks_exact(tri).map(|t| (depth(t), t)).collect();
    tris.sort_by(|a, b| a.0.total_cmp(&b.0));
    tris.into_iter().flat_map(|(_, t)| t.iter().copied()).collect()
}

impl Batches {
    /// Whether it has see-through surfaces (glass or faded, instanced or not).
    fn see_through(&self) -> bool {
        self.glass.is_some()
            || self.trans.is_some()
            || self.planned.iter().any(|d| d.class != TriClass::Opaque)
            || self.instanced.iter().any(|d| d.look != InstanceLook::Opaque)
            || self.block_order.iter().any(|k| self.blocks.get(k).is_some_and(|b| b.glass.is_some() || b.faded.is_some()))
    }

    /// Re-sort the glass for a new view direction (when it turned more than about 2 degrees).
    fn sort_glass(&mut self, queue: &wgpu::Queue, back: [f32; 3]) {
        let (Some(g), false) = (&self.glass, self.glass_cpu.is_empty()) else { return };
        let turned = self.glass_sorted_for.is_none_or(|s| s[0] * back[0] + s[1] * back[1] + s[2] * back[2] < 0.9994);
        if turned {
            let sorted = sort_back_to_front(&self.glass_cpu, back);
            for ((buffer, _), bytes) in g.parts.iter().zip(sorted.chunks(g.chunk.max(1))) {
                queue.write_buffer(buffer, 0, bytes);
            }
            self.glass_sorted_for = Some(back);
        }
    }
}

/// The tessellation pipeline (`shader`: its source), where display plans are evaluated: on a
/// device that runs compute shaders with the storage buffers the evaluation binds (not WebGL2)
/// and builds the pipeline. Elsewhere `None`, and bodies are built on the CPU: a device whose
/// shader compiler rejects the shader (D3D12's FXC once did) only loses GPU evaluation.
pub fn tessellation(adapter: &wgpu::Adapter, device: &wgpu::Device, queue: &wgpu::Queue, shader: &str) -> Option<crate::TessGpu> {
    let compute = adapter.get_downlevel_capabilities().flags.contains(wgpu::DownlevelFlags::COMPUTE_SHADERS)
        && device.limits().max_storage_buffers_per_shader_stage >= 4;
    if !compute {
        return None;
    }
    let info = adapter.get_info();
    if hangs_on_gpu_tessellation(info.backend, &info.name) || std::env::var_os("SOLIDCRAFT_CPU_TESS").is_some() {
        log::warn!("bodies are built on the CPU on {} ({:?})", info.name, info.backend);
        return None;
    }
    match crate::TessGpu::with_shader(info.name.clone(), device.clone(), queue.clone(), shader) {
        Ok(t) => Some(t),
        Err(e) => {
            log::warn!("bodies are built on the CPU on {} ({:?}): {e}", info.name, info.backend);
            None
        }
    }
}

/// The plan evaluation ([`tessellation`]) compiled on a thread of its own, so the window does not
/// wait for it: D3D12 compiles the evaluation shader in seconds, most of the viewport's start
/// (#4860). Bodies are built on the CPU until [`TessLater::ready`] holds it, and stay so where it
/// never does (a device that cannot build it), as they are where [`tessellation`] gives `None`.
#[cfg(not(target_arch = "wasm32"))]
pub struct TessLater {
    ready: Arc<std::sync::OnceLock<crate::TessGpu>>,
    /// Joined when dropped, so the app does not exit in the middle of the driver's compile.
    worker: Option<std::thread::JoinHandle<()>>,
}

#[cfg(not(target_arch = "wasm32"))]
impl TessLater {
    /// Start compiling [`tessellation`]'s pipeline (`shader`) for `device`.
    pub fn start(adapter: &wgpu::Adapter, device: &wgpu::Device, queue: &wgpu::Queue, shader: &'static str) -> TessLater {
        let ready = Arc::new(std::sync::OnceLock::new());
        let threaded = crate::concurrent::threads_allowed(device);
        let (adapter, device, queue, slot) = (adapter.clone(), device.clone(), queue.clone(), ready.clone());
        let compile = move || {
            if let Some(tess) = tessellation(&adapter, &device, &queue, shader) {
                let _ = slot.set(tess);
            }
        };
        // Where the device may not make pipelines on two threads at once (DXC, OpenGL), or no
        // thread starts, it compiles now, as before.
        let worker = threaded.then(|| std::thread::Builder::new().name("solidcraft tessellation".into()).spawn(compile.clone()).ok()).flatten();
        if worker.is_none() {
            compile();
        }
        TessLater { ready, worker }
    }

    /// The evaluation, there once it has compiled.
    pub fn ready(&self) -> Arc<std::sync::OnceLock<crate::TessGpu>> {
        self.ready.clone()
    }

    /// Wait for the compile to end.
    #[cfg(test)]
    fn wait(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

#[cfg(not(target_arch = "wasm32"))]
impl Drop for TessLater {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.take() {
            let _ = worker.join();
        }
    }
}

/// A device whose driver hangs on the evaluation shader, so bodies are built on the CPU there:
/// the AMD FirePro D300/D500/D700 (Mac Pro 2013, Tahiti) under Metal, where macOS resets the GPU
/// and the whole desktop freezes. `SOLIDCRAFT_CPU_TESS=1` forces CPU evaluation on any device.
pub fn hangs_on_gpu_tessellation(backend: wgpu::Backend, name: &str) -> bool {
    backend == wgpu::Backend::Metal && name.contains("FirePro")
}

/// Whether [`Renderer::prepare`] may blit the picture it drew last instead of drawing the frame
/// again: the content is the same and that picture is a texture frame (the direct path has none
/// to blit).
fn can_reuse(ss_ready: bool, cached: Option<u64>, key: u64) -> bool {
    ss_ready && cached == Some(key)
}

/// Everything the drawn picture depends on: the scene keys and camera, the viewport's size, the
/// section, images, analysis, accessibility map, ambient occlusion and the theme's hover and
/// selection tints. The same key draws the same picture (pipelines and shaders are fixed for the
/// renderer).
pub fn frame_key(cb: &Frame) -> u64 {
    use std::hash::Hash as _;
    let mut h = std::collections::hash_map::DefaultHasher::new();
    cb.product.hash(&mut h);
    cb.product_lights.as_ref().map(|lights| lights.iter().take(21).map(|light| light.map(f32::to_bits)).collect::<Vec<_>>()).hash(&mut h);
    cb.key.hash(&mut h);
    cb.sketch_key.hash(&mut h);
    cb.hl_key.hash(&mut h);
    cb.pv_key.hash(&mut h);
    cb.pv_hide.hash(&mut h);
    cb.view_proj.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
    cb.back.map(f32::to_bits).hash(&mut h);
    cb.eye.map(f32::to_bits).hash(&mut h);
    cb.size_px.map(f32::to_bits).hash(&mut h);
    cb.clip.map(f32::to_bits).hash(&mut h);
    cb.cap.map(f32::to_bits).hash(&mut h);
    cb.analysis.map(f32::to_bits).hash(&mut h);
    cb.analysis_colors.map(f32::to_bits).hash(&mut h);
    // The theme's hover and selection tints: a theme change draws the frame again, so the
    // picture cached under the previous theme's colours is not shown (#5802).
    cb.hover_tint.map(f32::to_bits).hash(&mut h);
    cb.selection_tint.map(f32::to_bits).hash(&mut h);
    cb.sky.map(f32::to_bits).hash(&mut h);
    cb.outline.hash(&mut h);
    cb.ao_radius.to_bits().hash(&mut h);
    cb.ao_samples.hash(&mut h);
    cb.ssaa.to_bits().hash(&mut h);
    for img in &cb.images {
        img.id.hash(&mut h);
        img.version.hash(&mut h);
        img.size.hash(&mut h);
        img.corners.iter().flatten().for_each(|v| v.to_bits().hash(&mut h));
        img.opacity.to_bits().hash(&mut h);
    }
    clouds::hash_clouds(&cb.clouds, &mut h);
    match &cb.access {
        Some(a) => {
            true.hash(&mut h);
            a.version.hash(&mut h);
            a.size.hash(&mut h);
            a.u.map(f32::to_bits).hash(&mut h);
            a.v.map(f32::to_bits).hash(&mut h);
            a.scale.to_bits().hash(&mut h);
        }
        None => false.hash(&mut h),
    }
    std::hash::Hasher::finish(&h)
}

impl Renderer {
    /// The scene can't be drawn straight into the window's pass (a different sample count, depth
    /// or format): it is drawn offscreen and blitted.
    fn window_differs(&self) -> bool {
        self.target.samples != self.window.samples || self.target.depth != self.window.depth || self.target.format != self.window.format
    }

    /// Before the frame's render pass: upload the scenes that changed (planned bodies not yet on
    /// the GPU are evaluated by dispatches recorded into `encoder`), the uniform, the images and
    /// the accessibility map, and draw the frame supersampled when it asks for that. A frame whose
    /// content is the same as the picture drawn last is not drawn again: that picture is blitted
    /// instead, so pointer moves and UI animations over a still view cost no scene work.
    pub fn prepare(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, encoder: &mut wgpu::CommandEncoder, frame: &Frame) {
        let res = self;
        let cb = frame;
        let key = frame_key(cb);
        if can_reuse(res.ss_ready && res.ss.is_some(), res.ss_key, key) {
            // A still frame shows the picture as it is; the first one filters it down once.
            let filter = res.ss_scaled && res.cached.is_none();
            if filter {
                res.cache_picture(device, encoder, cb.size_px);
            }
            // The scene isn't drawn again: `paint`'s blit (and that filter) is all this frame draws.
            res.ss_stats = DrawStats { draw_calls: u32::from(filter), ..DrawStats::default() };
            return;
        }
        res.cached = None;
        if cb.product {
            // A large product scene can exceed what the device will allocate. Its upload runs under
            // its own error scopes, so the failure reaches the preview instead of the device's
            // uncaptured-error handler, and nothing half-uploaded is drawn.
            let scopes =
                [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
            res.product.take(device, queue, &res.uniform, &res.uniform_bgl, cb.key, &cb.slot, None, false);
            let mut failed = None;
            for scope in scopes.into_iter().rev() {
                if let Some(e) = crate::popped(scope.pop()) {
                    failed.get_or_insert(e.to_string());
                }
            }
            if let Some(error) = failed {
                log::error!("product preview upload failed: {error}");
                res.product = Batches { key: Some(cb.key), ..Batches::default() };
                if let Some(Ok(mut sink)) = cb.upload_error.as_ref().map(|e| e.lock()) {
                    *sink = Some(error);
                }
            }
            res.product.sort_glass(queue, cb.back);
            res.highlight.take(device, queue, &res.uniform, &res.uniform_bgl, cb.hl_key, &cb.hl_slot, None, false);
            queue.write_buffer(&res.uniform, 0, &uniform_bytes(cb, cb.size_px[0], cb.size_px[1], res.linear_out, res.target.reversed_depth()));
            // The product picture goes through the window's pass too: offscreen when the targets
            // differ or it is drawn smaller, else straight into the window's pass.
            res.ss_ready = (res.window_differs() || cb.ssaa != 1.0) && res.supersample(device, encoder, cb, false);
            res.ss_key = res.ss_ready.then_some(key);
            return;
        }
        res.take_ready_tessellation();
        res.model.take(
            device,
            queue,
            &res.uniform,
            &res.uniform_bgl,
            cb.key,
            &cb.slot,
            res.tess.as_ref().map(|t| (t, &mut *encoder)),
            res.oit.is_some(),
        );
        res.sketch.take(device, queue, &res.uniform, &res.uniform_bgl, cb.sketch_key, &cb.sketch_slot, None, res.oit.is_some());
        res.highlight.take(device, queue, &res.uniform, &res.uniform_bgl, cb.hl_key, &cb.hl_slot, None, res.oit.is_some());
        res.preview.take(device, queue, &res.uniform, &res.uniform_bgl, cb.pv_key, &cb.pv_slot, None, res.oit.is_some());
        // See-through surfaces are drawn without sorting where the device can (order-independent
        // transparency, in an offscreen frame); else their triangles are sorted for the view.
        let see_through = res.oit.is_some() && res.model.see_through();
        if !see_through {
            res.model.sort_glass(queue, cb.back);
        }
        res.model.cull(queue, &cb.view_proj, cb.size_px);
        queue.write_buffer(&res.uniform, 0, &uniform_bytes(cb, cb.size_px[0], cb.size_px[1], res.linear_out, res.target.reversed_depth()));
        if let Some(a) = &cb.access
            && a.version != res.access_version
        {
            res.access_bind = access_texture(device, Some(queue), &res.access_bgl, a.size, &a.depth);
            res.access_version = a.version;
        }
        let ao_binds = [scene_bind(&res.model, &res.bind), scene_bind(&res.preview, &res.bind)];
        res.ao.prepare(device, encoder, ao_binds, cb.ao_radius, cb.size_px, res.target.reversed_depth(), [&res.model, &res.preview]);
        // Images: upload new or changed textures, drop removed ones, rebuild the quads.
        res.textures.retain(|id, _| cb.images.iter().any(|i| i.id == *id));
        res.quads.clear();
        for img in &cb.images {
            let [w, h] = img.size;
            if w == 0 || h == 0 || img.rgba.len() != (w as usize) * (h as usize) * 4 {
                continue;
            }
            if res.textures.get(&img.id).is_none_or(|t| t.0 != img.version) {
                let size = wgpu::Extent3d { width: w, height: h, depth_or_array_layers: 1 };
                let tex = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("sc_image"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo { texture: &tex, mip_level: 0, origin: wgpu::Origin3d::ZERO, aspect: wgpu::TextureAspect::All },
                    &img.rgba,
                    wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(4 * w), rows_per_image: Some(h) },
                    size,
                );
                let view = tex.create_view(&Default::default());
                let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("sc_image"),
                    layout: &res.image_bgl,
                    entries: &[
                        wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                        wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&res.sampler) },
                    ],
                });
                res.textures.insert(img.id, (img.version, bind));
            }
            let c = img.corners;
            let uv = [[0.0_f32, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
            let mut bytes = Vec::with_capacity(6 * IMG_SIZE);
            for k in [0, 1, 2, 0, 2, 3] {
                for v in c[k].iter().chain(&uv[k]).chain(std::iter::once(&img.opacity)) {
                    bytes.extend_from_slice(&v.to_le_bytes());
                }
            }
            if let Some(b) = upload(device, "sc_image_quad", &bytes, IMG_SIZE) {
                res.quads.push((img.id, b));
            }
        }
        res.clouds.prepare(device, queue, &res.cloud_pipes, &cb.clouds);
        res.ss_ready = (cb.ssaa != 1.0 || see_through || res.window_differs()) && res.supersample(device, encoder, cb, see_through);
        res.ss_key = res.ss_ready.then_some(key);
    }

    /// Draw the frame into `pass` within `viewport` (x, y, width, height in physical pixels):
    /// filtered down from the supersampled frame [`Renderer::prepare`] drew, or drawn directly.
    pub fn paint(&self, frame: &Frame, pass: &mut wgpu::RenderPass<'static>, viewport: [f32; 4]) {
        let [x, y, w, h] = viewport;
        if !(w > 0.0 && h > 0.0) {
            return;
        }
        pass.set_viewport(x, y, w, h, 0.0, 1.0);
        if self.ss_ready
            && let Some(ss) = &self.ss
        {
            // A still view's picture is already at the viewport's size: blit it as it is.
            if let Some(cached) = &self.cached {
                pass.set_pipeline(&self.copy_blit);
                pass.set_bind_group(0, &cached.bind, &[]);
            } else {
                pass.set_pipeline(if self.ss_scaled { &self.ss_blit } else { &self.copy_blit });
                pass.set_bind_group(0, &ss.bind, &[]);
            }
            pass.draw(0..3, 0..1);
            self.publish(DrawStats { draw_calls: self.ss_stats.draw_calls + 1, supersampled: self.ss_scaled, ..self.ss_stats });
            return;
        }
        // The pipelines draw into the scene's kind of target, not the window's pass: with no
        // offscreen frame (the device had no memory for one, #7296) nothing can be drawn.
        if self.window_differs() {
            self.publish(DrawStats::default());
            return;
        }
        let tally = Tally::default();
        if let Some(reset) = &self.depth_reset {
            pass.set_pipeline(reset);
            pass.draw(0..3, 0..1);
            tally.call();
        }
        self.draw(frame, pass, Phase::All, &tally);
        self.publish(tally.stats());
    }

    /// What the last frame drew; the handle stays current as later frames are drawn.
    pub fn stats(&self) -> DrawStatsHandle {
        self.stats.clone()
    }

    /// What the model scene's last change uploaded ([`UploadStats`]).
    pub fn uploads(&self) -> UploadStats {
        self.model.uploads
    }

    fn publish(&self, drawn: DrawStats) {
        if let Ok(mut s) = self.stats.lock() {
            *s = DrawStats { lowered: self.lowered, ..drawn };
        }
    }

    /// The target the pipelines draw into.
    pub fn target(&self) -> GpuTarget {
        self.target
    }

    /// Evaluate planned bodies with `later`'s pipeline once it has compiled (#4860).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn tessellate_later(&mut self, later: TessLater) {
        self.tess_later = Some(later);
    }

    /// [`Renderer::tess`] from [`Renderer::tess_later`] once that has compiled.
    fn take_ready_tessellation(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(tess) = self.tess_later.as_ref().and_then(|later| later.ready.get()) {
            self.tess = Some(tess.clone());
            self.tess_later = None;
        }
    }
}

impl Renderer {
    /// Draw the frame's scenes into `pass` (its viewport already set), counting what it draws.
    fn draw(&self, cb: &Frame, pass: &mut wgpu::RenderPass<'static>, phase: Phase, tally: &Tally) {
        let res = self;
        pass.set_bind_group(0, &res.bind, &[]);
        let tris = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, b: &Option<Batch>| {
            if let Some(b) = b {
                pass.set_pipeline(p);
                for (buffer, count) in &b.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                    tally.triangles(*count, 1);
                }
            }
        };
        let lines = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, b: &Option<Batch>| {
            if let Some(b) = b {
                pass.set_pipeline(p);
                for (buffer, count) in &b.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..6, 0..*count);
                    tally.segments(*count);
                }
            }
        };
        let (m, sk, h, pv) = if cb.product {
            (&res.product, &res.empty, &res.highlight, &res.empty)
        } else if cb.pv_hide {
            (&res.model, &res.sketch, &res.highlight, &res.empty)
        } else {
            (&res.model, &res.sketch, &res.highlight, &res.preview)
        };
        // A block's triangles of one class, drawn from its canonical vertex buffer.
        let block_class = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, class: TriClass| {
            for key in &m.block_order {
                let Some(block) = m.blocks.get(key) else { continue };
                if block.class(class).is_some() {
                    pass.set_pipeline(p);
                    block.draw(pass, class, tally);
                }
            }
        };
        if phase == Phase::AfterSeeThrough {
            pass.set_bind_group(1, &res.access_bind, &[]);
            pass.set_bind_group(2, &res.ao.bind, &[]);
            Self::draw_over(res, pass, sk, h, pv, m, &tris, &lines);
            return;
        }
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        lines(pass, &res.overlay, &m.under);
        pass.set_bind_group(0, scene_bind(sk, &res.bind), &[]);
        lines(pass, &res.overlay, &sk.under);
        pass.set_bind_group(1, &res.access_bind, &[]);
        pass.set_bind_group(2, &res.ao.bind, &[]);
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        let planned = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline| {
            for d in m.planned.iter().filter(|d| d.class == TriClass::Opaque) {
                pass.set_pipeline(p);
                pass.set_vertex_buffer(0, d.mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, d.colors.slice(..));
                pass.set_index_buffer(d.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..d.mesh.index_count, 0, 0..1);
                tally.triangles(d.mesh.index_count, 1);
            }
        };
        // Instanced meshes, with the pipeline for smooth and for flat triangles.
        let instanced = |pass: &mut wgpu::RenderPass<'static>, smooth: &wgpu::RenderPipeline, flat: &wgpu::RenderPipeline| {
            for d in m.instanced.iter().filter(|d| d.look == InstanceLook::Opaque) {
                d.draw(pass, smooth, flat, tally);
            }
        };
        // Whole highlighted bodies: the model's own evaluated plan triangles, drawn again in a
        // highlight colour (the pipeline adds or replaces the vertex colour).
        let tinted = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, plans: &[Arc<solidcraft_geom::tess::TessPlan>]| {
            for plan in plans {
                let address = Arc::as_ptr(plan) as usize;
                for d in &m.planned {
                    let Some(part) = d.part_of(address) else { continue };
                    pass.set_pipeline(p);
                    pass.set_vertex_buffer(0, d.mesh.vertices.slice(..));
                    pass.set_vertex_buffer(1, d.colors.slice(..));
                    pass.set_index_buffer(d.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                    pass.draw_indexed(part.indices.clone(), 0, 0..1);
                }
            }
        };
        // Whole highlighted bodies the model builds on the CPU: their runs of a block's vertex
        // buffer (a block too large for one vertex buffer has its highlight skipped).
        let tinted_blocks = |pass: &mut wgpu::RenderPass<'static>, p: &wgpu::RenderPipeline, refs: &[BlockTris]| {
            for r in refs {
                let Some(block) = m.blocks.get(&r.block) else { continue };
                let Some(vertices) = &block.vertices else { continue };
                pass.set_pipeline(p);
                pass.set_vertex_buffer(0, vertices.slice(..));
                for run in &r.tris {
                    pass.draw(run.clone(), 0..1);
                    tally.triangles(run.len() as u32, 1);
                }
            }
        };
        // Whole highlighted bodies the model draws as placements of shared triangles: the same
        // placements, with the highlight colour on each.
        let tinted_instanced =
            |pass: &mut wgpu::RenderPass<'static>, smooth: &wgpu::RenderPipeline, flat: &wgpu::RenderPipeline, list: &[InstancedDraw]| {
                for d in list.iter().filter(|d| d.look == InstanceLook::Opaque) {
                    d.draw(pass, smooth, flat, tally);
                }
            };
        let sectioned = cb.clip[..3].iter().map(|c| c * c).sum::<f32>() > 0.25;
        match (
            res.tri_cut.as_ref().filter(|_| sectioned),
            res.tri_cut_planned.as_ref(),
            res.tri_cut_instanced.as_ref(),
            res.tri_cut_instanced_flat.as_ref(),
            res.section_reset.as_ref().zip(res.section_clear.as_ref()),
        ) {
            (Some(cut), Some(cut_planned), Some(cut_instanced), Some(cut_flat), Some((reset, clear))) => {
                // The model's own triangles, blocks, plans and instances with its palette, then the
                // preview's with its own.
                let all = |pass: &mut wgpu::RenderPass<'static>, pick: fn(&Cut) -> &wgpu::RenderPipeline| {
                    pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
                    tris(pass, pick(cut), &m.tris);
                    block_class(pass, pick(cut), TriClass::Opaque);
                    planned(pass, pick(cut_planned));
                    instanced(pass, pick(cut_instanced), pick(cut_flat));
                    pass.set_bind_group(0, scene_bind(pv, &res.bind), &[]);
                    tris(pass, pick(cut), &pv.tris);
                };
                // The stencil starts at 0 (the window's pass keeps it from frame to frame), counts
                // the faces behind the cut, then marks where no cap shows (`Cut`).
                pass.set_pipeline(clear);
                pass.draw(0..3, 0..1);
                tally.call();
                all(pass, |c| &c.wind);
                pass.set_stencil_reference(NOT_CAP);
                all(pass, |c| &c.mark);
                all(pass, |c| &c.touch);
                pass.set_pipeline(reset);
                pass.draw(0..3, 0..1);
                tally.call();
                all(pass, |c| &c.push);
                pass.set_stencil_reference(0);
                all(pass, |c| &c.cap);
            }
            _ => {
                pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
                tris(pass, &res.tri, &m.tris);
                block_class(pass, &res.tri, TriClass::Opaque);
                planned(pass, &res.tri_planned);
                instanced(pass, &res.tri_instanced, &res.tri_instanced_flat);
                pass.set_bind_group(0, scene_bind(pv, &res.bind), &[]);
                tris(pass, &res.tri, &pv.tris);
            }
        }
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        tris(pass, &res.pbr, &m.pbr);
        if !cb.product {
            res.clouds.paint(pass, &res.cloud_pipes, tally);
        }
        // Whole-body tints first, so a face or an edge highlighted over the body (in `h.tris`
        // and `h.lines`) keeps its own colour, as it did when the body's triangles were built
        // into the same list before them.
        // The model's own data (blocks, plans, instanced meshes) uses the model palette; the
        // highlight scene's own geometry uses the highlight palette.
        tinted(pass, &res.tri_hl_hover, &h.hover_planned);
        tinted(pass, &res.tri_hl_selected, &h.selected_planned);
        tinted_blocks(pass, &res.tri_hl_hover_built, &h.hover_blocks);
        tinted_blocks(pass, &res.tri_hl_selected_built, &h.selected_blocks);
        pass.set_bind_group(0, scene_bind(h, &res.bind), &[]);
        tinted_instanced(pass, &res.tri_hl_instanced, &res.tri_hl_instanced_flat, &h.hover_instanced);
        tinted_instanced(pass, &res.tri_hl_instanced, &res.tri_hl_instanced_flat, &h.selected_instanced);
        // Whole faded or see-through bodies: the model's block vertices, in this palette.
        for r in &h.recoloured {
            let Some(block) = m.blocks.get(&r.block) else { continue };
            let Some(vertices) = &block.vertices else { continue };
            pass.set_pipeline(&res.tri_hl);
            pass.set_vertex_buffer(0, vertices.slice(..));
            pass.draw(r.vertices.clone(), 0..1);
            tally.triangles(r.vertices.len() as u32, 1);
        }
        // The highlight's own feedback triangles: its blocks' (kept while it is unchanged), then
        // the loose ones.
        for key in &h.block_order {
            if let Some(block) = h.blocks.get(key).filter(|b| b.class(TriClass::Opaque).is_some()) {
                pass.set_pipeline(&res.tri_hl);
                block.draw(pass, TriClass::Opaque, tally);
            }
        }
        tris(pass, &res.tri_hl, &h.tris);
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        lines(pass, &res.line, &m.lines);
        lines(pass, &res.guide_line, &m.guide_lines);
        pass.set_bind_group(0, scene_bind(sk, &res.bind), &[]);
        lines(pass, &res.line, &sk.lines);
        lines(pass, &res.guide_line, &sk.guide_lines);
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        lines(pass, &res.line_hidden, &m.hidden);
        pass.set_bind_group(0, scene_bind(h, &res.bind), &[]);
        lines(pass, &res.line_hidden, &h.hidden);
        lines(pass, &res.line, &h.lines);
        lines(pass, &res.guide_line, &h.guide_lines);
        pass.set_bind_group(0, scene_bind(pv, &res.bind), &[]);
        lines(pass, &res.line, &pv.lines);
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        for (id, q) in res.quads.iter().filter(|_| !cb.product) {
            if let Some((_, bind)) = res.textures.get(id) {
                pass.set_pipeline(&res.image);
                pass.set_bind_group(1, bind, &[]);
                for (buffer, count) in &q.parts {
                    pass.set_vertex_buffer(0, buffer.slice(..));
                    pass.draw(0..*count, 0..1);
                    tally.triangles(*count, 1);
                }
            }
        }
        pass.set_bind_group(0, scene_bind(pv, &res.bind), &[]);
        tris(pass, &res.ghost, &pv.ghost);
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        tris(pass, &res.ghost, &m.ghost);
        tris(pass, &res.trans, &m.shadow);
        if phase == Phase::BeforeSeeThrough {
            return;
        }
        // See-through instanced meshes (placements back to front) with the rest of their kind.
        let see_through =
            |pass: &mut wgpu::RenderPass<'static>, look: InstanceLook, (smooth, flat): &(wgpu::RenderPipeline, wgpu::RenderPipeline)| {
                for d in m.instanced.iter().filter(|d| d.look == look) {
                    d.draw(pass, smooth, flat, tally);
                }
            };
        tris(pass, &res.glass_far, &m.glass);
        see_through(pass, InstanceLook::Glass, &res.glass_far_instanced);
        tris(pass, &res.glass_near, &m.glass);
        see_through(pass, InstanceLook::Glass, &res.glass_near_instanced);
        tris(pass, &res.trans, &m.trans);
        block_class(pass, &res.trans, TriClass::Faded);
        see_through(pass, InstanceLook::Faded, &res.trans_instanced);
        Self::draw_over(res, pass, sk, h, pv, m, &tris, &lines);
    }

    /// What is drawn over the model's see-through surfaces.
    #[allow(clippy::too_many_arguments)]
    fn draw_over(
        res: &Renderer,
        pass: &mut wgpu::RenderPass<'static>,
        sk: &Batches,
        h: &Batches,
        pv: &Batches,
        m: &Batches,
        tris: &dyn Fn(&mut wgpu::RenderPass<'static>, &wgpu::RenderPipeline, &Option<Batch>),
        lines: &dyn Fn(&mut wgpu::RenderPass<'static>, &wgpu::RenderPipeline, &Option<Batch>),
    ) {
        pass.set_bind_group(0, scene_bind(sk, &res.bind), &[]);
        tris(pass, &res.trans, &sk.trans);
        tris(pass, &res.guide_trans, &sk.guide_trans);
        pass.set_bind_group(0, scene_bind(h, &res.bind), &[]);
        tris(pass, &res.trans, &h.trans);
        tris(pass, &res.guide_trans, &h.guide_trans);
        tris(pass, &res.xray, &h.xray);
        pass.set_bind_group(0, scene_bind(pv, &res.bind), &[]);
        tris(pass, &res.xray, &pv.xray);
        pass.set_bind_group(0, scene_bind(m, &res.bind), &[]);
        lines(pass, &res.overlay, &m.overlays);
        pass.set_bind_group(0, scene_bind(sk, &res.bind), &[]);
        lines(pass, &res.overlay, &sk.overlays);
        pass.set_bind_group(0, scene_bind(h, &res.bind), &[]);
        lines(pass, &res.overlay, &h.overlays);
    }

    /// The model's see-through surfaces into the order-independent transparency's targets.
    fn draw_see_through(&self, pass: &mut wgpu::RenderPass<'static>, p: &oit::Pipelines, tally: &Tally) {
        let m = &self.model;
        pass.set_bind_group(0, scene_bind(m, &self.bind), &[]);
        pass.set_pipeline(&p.built);
        for batch in [&m.glass, &m.trans].into_iter().flatten() {
            for (buffer, count) in &batch.parts {
                pass.set_vertex_buffer(0, buffer.slice(..));
                pass.draw(0..*count, 0..1);
                tally.triangles(*count, 1);
            }
        }
        // The blocks' see-through triangles, straight from their vertex buffers (nothing sorted).
        for class in [TriClass::Glass, TriClass::Faded] {
            for key in &m.block_order {
                if let Some(block) = m.blocks.get(key) {
                    block.draw(pass, class, tally);
                }
            }
        }
        // Faded and see-through bodies drawn from their plans.
        let mut planned = m.planned.iter().filter(|d| d.class != TriClass::Opaque).peekable();
        if planned.peek().is_some() {
            pass.set_pipeline(&p.planned);
            for d in planned {
                pass.set_vertex_buffer(0, d.mesh.vertices.slice(..));
                pass.set_vertex_buffer(1, d.colors.slice(..));
                pass.set_index_buffer(d.mesh.indices.slice(..), wgpu::IndexFormat::Uint32);
                pass.draw_indexed(0..d.mesh.index_count, 0, 0..1);
                tally.triangles(d.mesh.index_count, 1);
            }
        }
        for d in m.instanced.iter().filter(|d| d.look != InstanceLook::Opaque) {
            d.draw(pass, &p.instanced, &p.instanced_flat, tally);
        }
    }
}

impl Renderer {
    /// Filter the supersampled picture down to the viewport's own pixels once, into the texture
    /// [`Renderer::cached`] keeps: still frames then blit it as it is instead of filtering it every
    /// time.
    fn cache_picture(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, size_px: [f32; 2]) {
        let Some(ss) = &self.ss else { return };
        let [w, h] = size_px;
        if !(w >= 1.0 && h >= 1.0) || self.no_cache {
            return;
        }
        let max = self.max_texture.max(1);
        let size = [(w.round() as u32).clamp(1, max), (h.round() as u32).clamp(1, max)];
        let made = allocated(device, || {
            device.create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_ssaa_cached"),
                size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: self.target.format,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
        });
        let texture = match made {
            Ok(texture) => texture,
            Err(e) => {
                log::warn!("still frames are filtered every time: the GPU refused a {} × {} picture: {e}", size[0], size[1]);
                self.no_cache = true;
                self.lowered = true;
                return;
            }
        };
        let view = texture.create_view(&Default::default());
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_ssaa_cached"),
            layout: &self.ss_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(&view) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        {
            let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                label: Some("sc_ssaa_cached"),
                color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                    view: &view,
                    depth_slice: None,
                    resolve_target: None,
                    ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                })],
                depth_stencil_attachment: None,
                timestamp_writes: None,
                occlusion_query_set: None,
                multiview_mask: None,
            });
            pass.set_pipeline(&self.ss_filter);
            pass.set_bind_group(0, &ss.bind, &[]);
            pass.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            pass.draw(0..3, 0..1);
        }
        self.cached = Some(CachedPicture { _view: view, bind });
    }

    /// Draw the frame supersampled (`cb.ssaa` times the viewport's pixels each way, within the
    /// device's texture limit and the memory budget) into textures of the viewport's kind.
    /// `false` when it cannot be (too small a gain, a zero-sized viewport): drawn as usual.
    fn supersample(&mut self, device: &wgpu::Device, encoder: &mut wgpu::CommandEncoder, cb: &Frame, see_through: bool) -> bool {
        // Supersampled when asked (and worth it); else, for order-independent transparency, at
        // the viewport's own size.
        let max = self.max_texture.max(1);
        let own = || {
            let [w, h] = cb.size_px;
            (w >= 1.0 && h >= 1.0).then(|| [(w.round() as u32).clamp(1, max), (h.round() as u32).clamp(1, max)])
        };
        // See-through surfaces (order-independent transparency) and a window pass of another kind
        // (samples, depth) need a frame of their own, even at the viewport's own size.
        let needs_own = see_through || self.window_differs();
        let samples = self.target.samples;
        let mut drawn = None;
        for _ in 0..FRAME_TRIES {
            let budget = self.frame_budget;
            let scaled = if cb.ssaa != 1.0 { supersampled_within(cb.size_px, cb.ssaa, self.max_texture, samples, budget) } else { None };
            // After refused frames, one of the viewport's own size may not fit either: it is drawn
            // smaller and filtered up.
            let frame = scaled.map(|s| (s, true)).or_else(|| {
                needs_own.then(own).flatten().map(|s| {
                    let fitted = fitted_size(s, samples, budget);
                    (fitted, fitted != s)
                })
            });
            let Some((size, filtered)) = frame else {
                self.ss = None;
                return false;
            };
            if self.ss.as_ref().is_some_and(|ss| ss.size == size) {
                drawn = Some((size, filtered));
                break;
            }
            // The old frame goes before the new one is asked for.
            self.ss = None;
            match allocated(device, || self.supersampled(device, size)) {
                Ok(ss) => {
                    let _cleared = clear_resolve_targets(encoder, ss.resolve.as_ref());
                    #[cfg(test)]
                    {
                        self.resolve_clears += _cleared;
                    }
                    self.ss = Some(ss);
                    drawn = Some((size, filtered));
                    break;
                }
                Err(e) => {
                    let before = self.frame_budget;
                    self.refused(size, &e);
                    // Already at the least budget: no smaller frame to ask for.
                    if self.frame_budget == before {
                        break;
                    }
                }
            }
        }
        let Some((size, filtered)) = drawn else { return false };
        self.ss_scaled = filtered;
        if see_through && self.oit_targets.as_ref().is_none_or(|t| t.size != size) {
            self.oit_targets = None;
            match self.oit.as_ref().map(|p| allocated(device, || p.targets(device, size, samples))) {
                Some(Ok(targets)) => {
                    let _cleared = clear_resolve_targets(encoder, targets.resolves());
                    #[cfg(test)]
                    {
                        self.resolve_clears += _cleared;
                    }
                    self.oit_targets = Some(targets);
                }
                Some(Err(e)) => {
                    log::warn!("see-through surfaces are sorted: the GPU refused their {} × {} targets: {e}", size[0], size[1]);
                    self.oit = None;
                    self.lowered = true;
                }
                None => {}
            }
        }
        let Some(ss) = &self.ss else { return false };
        let depth_format = self.target.depth;
        let stencil = depth_format.is_some_and(|f| f.has_stencil_aspect());
        // The depth is kept for the passes after the first when there are see-through surfaces.
        let keep = if see_through { wgpu::StoreOp::Store } else { wgpu::StoreOp::Discard };
        let pass = |encoder: &mut wgpu::CommandEncoder, first: bool| {
            let load = |clear| if first { wgpu::LoadOp::Clear(clear) } else { wgpu::LoadOp::Load };
            encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("sc_ssaa"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &ss.color,
                        depth_slice: None,
                        resolve_target: ss.resolve.as_ref(),
                        ops: wgpu::Operations { load: load(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: ss.depth.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations { load: load_depth(first, far_depth(self.target.reversed_depth())), store: keep }),
                        stencil_ops: stencil.then_some(wgpu::Operations { load: load_depth(first, 0), store: keep }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime()
        };
        let tally = Tally::default();
        let (Some(p), Some(t), true) = (&self.oit, &self.oit_targets, see_through) else {
            let mut first = pass(encoder, true);
            first.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            self.draw(cb, &mut first, Phase::All, &tally);
            drop(first);
            self.ss_stats = tally.stats();
            return true;
        };
        {
            let mut first = pass(encoder, true);
            first.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            self.draw(cb, &mut first, Phase::BeforeSeeThrough, &tally);
        }
        p.accumulate(encoder, t, ss.depth.as_ref(), stencil, |pass| self.draw_see_through(pass, p, &tally));
        p.composite(encoder, t, &ss.color, ss.resolve.as_ref());
        tally.call();
        {
            let mut over = pass(encoder, false);
            over.set_viewport(0.0, 0.0, size[0] as f32, size[1] as f32, 0.0, 1.0);
            self.draw(cb, &mut over, Phase::AfterSeeThrough, &tally);
        }
        self.ss_stats = tally.stats();
        true
    }

    /// The device refused a frame of `size` pixels (out of video memory, #7296): the next one
    /// asked for takes at most half its memory.
    fn refused(&mut self, size: [u32; 2], e: &wgpu::Error) {
        self.frame_budget = (self.frame_budget.min(frame_bytes(size, self.target.samples)) / 2).max(MIN_FRAME_BUDGET);
        self.lowered = true;
        log::warn!("the view is drawn at a lower resolution: the GPU refused a {} × {} frame: {e}", size[0], size[1]);
    }

    /// Textures for a supersampled frame of `size` pixels.
    fn supersampled(&self, device: &wgpu::Device, size: [u32; 2]) -> Supersampled {
        let t = self.target;
        let texture = |label: &str, format: wgpu::TextureFormat, samples: u32, usage: wgpu::TextureUsages| {
            device
                .create_texture(&wgpu::TextureDescriptor {
                    label: Some(label),
                    size: wgpu::Extent3d { width: size[0], height: size[1], depth_or_array_layers: 1 },
                    mip_level_count: 1,
                    sample_count: samples,
                    dimension: wgpu::TextureDimension::D2,
                    format,
                    usage,
                    view_formats: &[],
                })
                .create_view(&Default::default())
        };
        let samples = t.samples.max(1);
        let sampled = wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING;
        let (color, resolve) = if samples > 1 {
            (texture("sc_ssaa_ms", t.format, samples, wgpu::TextureUsages::RENDER_ATTACHMENT), Some(texture("sc_ssaa", t.format, 1, sampled)))
        } else {
            (texture("sc_ssaa", t.format, 1, sampled), None)
        };
        let depth = t.depth.map(|f| texture("sc_ssaa_depth", f, samples, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_ssaa"),
            layout: &self.ss_bgl,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: wgpu::BindingResource::TextureView(resolve.as_ref().unwrap_or(&color)) },
                wgpu::BindGroupEntry { binding: 1, resource: wgpu::BindingResource::Sampler(&self.sampler) },
            ],
        });
        Supersampled { size, color, resolve, depth, bind }
    }
}

/// Clear new textures that only a multisample resolve writes (the frame's resolved colour, the
/// see-through surfaces' resolved sums), once, before their first resolve; how many it cleared.
/// Direct3D 12 requires a render target placed in reused memory to be cleared, discarded or
/// copied over before anything else uses it. A resolve is none of these, but wgpu counts it as
/// the texture's first write and never clears it. Some AMD Radeon drivers then kept that memory's
/// old compression state, and the multisampled scene showed as nothing at all (no grid, origin
/// planes or bodies), a flat colour or speckles (#6828, #7157, #6613, #7052). At 1x the scene is
/// drawn into a texture its own pass clears, which is why 1x anti-aliasing showed it.
fn clear_resolve_targets<'a>(encoder: &mut wgpu::CommandEncoder, views: impl IntoIterator<Item = &'a wgpu::TextureView>) -> u32 {
    let mut cleared = 0;
    for view in views {
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("sc_resolve_clear"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT), store: wgpu::StoreOp::Store },
            })],
            ..Default::default()
        });
        cleared += 1;
    }
    cleared
}

/// Which part of a frame [`Renderer::draw`] draws.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Phase {
    /// All of it, the model's see-through surfaces sorted.
    All,
    /// Up to the model's see-through surfaces, which order-independent transparency draws.
    BeforeSeeThrough,
    /// What is drawn over them (sketch fills, highlights, X-ray, overlays).
    AfterSeeThrough,
}

/// A depth (or stencil) attachment's load: cleared to `clear` for the first pass, else kept.
fn load_depth<T>(first: bool, clear: T) -> wgpu::LoadOp<T> {
    if first { wgpu::LoadOp::Clear(clear) } else { wgpu::LoadOp::Load }
}

/// Uniform bytes for a frame.
pub fn uniform_bytes(cb: &Frame, w: f32, h: f32, linear: bool, reversed: bool) -> Vec<u8> {
    let mut v: Vec<f32> = gpu_view_proj(cb.view_proj, reversed).iter().flatten().copied().collect();
    v.extend([cb.back[0], cb.back[1], cb.back[2], if linear { 1.0 } else { 0.0 }]);
    v.extend([w.max(1.0), h.max(1.0), 0.0, if cb.outline { 1.0 } else { 0.0 }]);
    v.extend(cb.clip);
    v.extend(cb.cap);
    v.extend(cb.analysis);
    match &cb.access {
        Some(a) => {
            v.extend(a.u);
            v.extend(a.v);
        }
        None => v.extend([0.0; 8]),
    }
    v.extend(cb.eye);
    v.extend([cb.ao_radius, cb.ao_samples.clamp(1, AO_SAMPLES) as f32, 0.0, 0.0]);
    v.extend(cb.analysis_colors);
    v.extend([if reversed { 1.0 } else { 0.0 }, FACE_STEPS as f32, FACE_SLOPE, 0.0]);
    v.extend([cb.hover_tint[0], cb.hover_tint[1], cb.hover_tint[2], 0.0]);
    v.extend([cb.selection_tint[0], cb.selection_tint[1], cb.selection_tint[2], 0.0]);
    v.extend([
        if cb.product_lights.is_some() { 1.0 } else { 0.0 },
        cb.product_lights.as_ref().map_or(0, |lights| lights.len().min(21)) as f32,
        0.0,
        0.0,
    ]);
    for index in 0..21 {
        let light = cb.product_lights.as_ref().and_then(|lights| lights.get(index)).copied().unwrap_or([0.0; 16]);
        v.extend(light.map(|value| if value.is_finite() { value } else { 0.0 }));
    }
    v.extend(cb.sky.map(|value| if value.is_finite() { value } else { 0.0 }));
    v.iter().flat_map(|x| x.to_le_bytes()).collect()
}

/// The shader's analysis colours (`ana_c`: draft positive, between, negative; reachable,
/// blocked; curvature flat, middle, tight; alpha 1) and flags (`ana_f.x`: 1 to stripe the bad
/// faces).
pub fn analysis_colors(c: &solidcraft_render::AnalysisColors) -> [f32; 36] {
    let mut v = [0.0; 36];
    let colors = c.draft.iter().chain(&c.access).chain(&c.curvature).flat_map(|rgb| [rgb[0], rgb[1], rgb[2], 1.0]);
    for (slot, x) in v.iter_mut().zip(colors) {
        *slot = x;
    }
    v[32] = if c.hatch { 1.0 } else { 0.0 };
    v
}

#[cfg(test)]
mod tests {
    /// A placed box is kept while any of it may be in view, and left out only when it lies
    /// wholly past one side of the view: in an orthographic view and a perspective one, behind
    /// the eye too.
    #[test]
    fn placed_boxes_out_of_view_are_left_out() {
        use super::placed_box_in_view;
        let unit = [[0.0f32, 0.0, 0.0], [1.0, 1.0, 1.0]];
        let at = |x: f32, y: f32, z: f32| [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]];
        // Orthographic: x and y from -10 to 10 on screen, depth z from 0 (near) to 1 (far) over
        // -100..100 (columns).
        let ortho = [[0.1, 0.0, 0.0, 0.0], [0.0, 0.1, 0.0, 0.0], [0.0, 0.0, 0.005, 0.0], [0.0, 0.0, 0.5, 1.0]];
        assert!(placed_box_in_view(&at(0.0, 0.0, 0.0), &unit, &ortho));
        assert!(placed_box_in_view(&at(9.5, 9.5, 0.0), &unit, &ortho), "partly in view");
        for (x, y, z) in [(11.0, 0.0, 0.0), (-12.0, 0.0, 0.0), (0.0, 11.0, 0.0), (0.0, -12.0, 0.0), (0.0, 0.0, -120.0), (0.0, 0.0, 120.0)] {
            assert!(!placed_box_in_view(&at(x, y, z), &unit, &ortho), "{x} {y} {z}");
        }
        // Turned a quarter about z, the box at x 10.5 reaches back into view (x from 9.5).
        let turned = [[0.0, -1.0, 0.0, 10.5], [1.0, 0.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        assert!(placed_box_in_view(&turned, &unit, &ortho));
        // Perspective, looking down -z from the origin (w = -z, 90° field of view, near 1).
        let persp = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, -1.0], [0.0, 0.0, -1.0, 0.0]];
        assert!(placed_box_in_view(&at(-0.5, -0.5, -10.0), &unit, &persp));
        assert!(!placed_box_in_view(&at(-0.5, -0.5, 10.0), &unit, &persp), "behind the eye");
        assert!(!placed_box_in_view(&at(30.0, -0.5, -10.0), &unit, &persp), "beside the view");
        assert!(placed_box_in_view(&at(-0.5, -0.5, -20.0), &[[0.0, 0.0, 0.0], [1.0, 1.0, 21.0]], &persp), "from in front of the eye to behind it");
        // A box that is not finite is drawn (never culled by mistake).
        assert!(placed_box_in_view(&at(30.0, 0.0, -10.0), &[[f32::NAN; 3], [1.0; 3]], &persp));
    }

    /// A placement's pixels per millimetre: in an orthographic view the same anywhere, along the
    /// axis that shows largest (a turned box too); in perspective, larger nearer the eye, and
    /// infinite (drawn in full) with a corner behind it. The level chosen is the coarsest whose
    /// error stays under `LOD_PX` there.
    #[test]
    fn levels_of_detail_follow_the_size_on_screen() {
        use super::{LOD_PX, level_for, placement_scale};
        let unit = [[0.0f32, 0.0, 0.0], [1.0, 1.0, 1.0]];
        let at = |x: f32, y: f32, z: f32| [[1.0, 0.0, 0.0, x], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]];
        // Orthographic over x, y in -10..10 on a 200 × 200 view: 10 px a millimetre.
        let ortho = [[0.1, 0.0, 0.0, 0.0], [0.0, 0.1, 0.0, 0.0], [0.0, 0.0, 0.005, 0.0], [0.0, 0.0, 0.5, 1.0]];
        let size = [200.0, 200.0];
        assert!((placement_scale(&at(0.0, 0.0, 0.0), &unit, &ortho, size) - 10.0).abs() < 1e-3);
        assert!((placement_scale(&at(5.0, -3.0, 0.0), &unit, &ortho, size) - 10.0).abs() < 1e-3);
        let (c, s) = (0.6f32.cos(), 0.6f32.sin());
        let turned = [[c, -s, 0.0, 0.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        assert!((placement_scale(&turned, &unit, &ortho, size) - 10.0).abs() < 1e-2);
        // Perspective looking down -z from the origin (w = -z), 90° over 200 px.
        let persp = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, -1.0], [0.0, 0.0, -1.0, 0.0]];
        let near = placement_scale(&at(0.0, 0.0, -10.0), &unit, &persp, size);
        let far = placement_scale(&at(0.0, 0.0, -100.0), &unit, &persp, size);
        assert!((near - 100.0 / 9.0).abs() < 0.2, "nearest corner at depth 9: {near}");
        assert!(far < near / 9.0 && far > 0.0, "{far} vs {near}");
        assert!(placement_scale(&at(0.0, 0.0, 0.5), &unit, &persp, size).is_infinite(), "a corner behind the eye");
        assert!(placement_scale(&at(0.0, 0.0, -10.0), &[[f32::NAN; 3], [1.0; 3]], &persp, size).is_infinite());
        // The coarsest level within LOD_PX: errors 0 (full), 0.05, 0.2, 1.0 mm.
        let errors = [0.0, 0.05, 0.2, 1.0];
        assert_eq!(level_for(&errors, f32::INFINITY), 0);
        assert_eq!(level_for(&errors, 100.0), 0, "5 px for the first level");
        assert_eq!(level_for(&errors, 10.0), 1);
        assert_eq!(level_for(&errors, LOD_PX / 0.2), 2);
        assert_eq!(level_for(&errors, 0.5), 3);
        assert_eq!(level_for(&[0.0], 0.001), 0);
    }

    /// Shared vertices: a flat mesh's by position alone (an STL's facets: one normal per
    /// triangle), a smooth mesh's by position and normal; corners that differ in either stay
    /// apart. The corners come back as they went in, a flat triangle's lit with its own normal.
    #[test]
    fn shared_tris_share_each_vertex_once() {
        use super::{SHARED_SIZE, SharedTris};
        use solidcraft_geom::{Mesh, Vec3};
        let p = [Vec3::new(0.0, 0.0, 0.0), Vec3::new(1.0, 0.0, 0.0), Vec3::new(1.0, 1.0, 0.0), Vec3::new(0.0, 1.0, 0.0)];
        // Two triangles of a square, each corner its own vertex (as an STL is read).
        let mut flat = Mesh::default();
        for t in [[0, 1, 2], [0, 2, 3]] {
            let base = flat.positions.len() as u32;
            flat.positions.extend(t.map(|i| p[i]));
            flat.normals.extend([Vec3::Z; 3]);
            flat.triangles.push([base, base + 1, base + 2]);
        }
        let shared = SharedTris::new(&flat);
        assert!(shared.flat);
        assert_eq!(shared.vertices.len() / SHARED_SIZE, 4, "the diagonal's ends are shared");
        assert_eq!(shared.indices.len(), 6);
        let corners: Vec<_> = shared.corners().flatten().collect();
        for (k, (q, n)) in corners.iter().enumerate() {
            assert_eq!(*q, flat.positions[k].to_f32());
            assert!((n[2] - 1.0).abs() < 1e-6, "{n:?}");
        }
        // Smooth: the same corners with different normals stay apart.
        let mut smooth = flat.clone();
        smooth.normals = vec![Vec3::Z, Vec3::Z, Vec3::new(0.0, 0.6, 0.8), Vec3::Z, Vec3::X, Vec3::new(0.0, 0.6, 0.8)];
        let shared = SharedTris::new(&smooth);
        assert!(!shared.flat);
        assert_eq!(shared.vertices.len() / SHARED_SIZE, 5, "corner 0 differs in normal only");
        let back: Vec<_> = shared.corners().flatten().map(|(_, n)| n).collect();
        assert_eq!(back, smooth.normals.iter().map(|n| n.to_f32()).collect::<Vec<_>>());
        // Indices past the vertices: the triangle is left out, nothing panics.
        smooth.triangles.push([0, 1, 99]);
        assert_eq!(SharedTris::new(&smooth).indices.len(), 6);
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn rejected_geometry_allocation_returns_without_panicking() {
        let Ok((_, device, _)) = crate::open_headless() else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let scope = device.push_error_scope(wgpu::ErrorFilter::Validation);
        // Invalid mapping usage provides a deterministic failed allocation without exhausting memory.
        let result =
            super::upload_with(&device, "rejected geometry", &[0; 288], super::PBR_SIZE, wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::VERTEX);
        assert!(result.is_none());
        assert!(pollster::block_on(scope.pop()).is_some());
    }

    /// Only the FirePro under Metal skips GPU evaluation; the same card under another backend
    /// and other Metal GPUs keep it.
    #[test]
    fn firepro_under_metal_builds_bodies_on_the_cpu() {
        assert!(super::hangs_on_gpu_tessellation(wgpu::Backend::Metal, "AMD FirePro D500"));
        assert!(super::hangs_on_gpu_tessellation(wgpu::Backend::Metal, "AMD FirePro D700"));
        assert!(!super::hangs_on_gpu_tessellation(wgpu::Backend::Metal, "Apple M2"));
        assert!(!super::hangs_on_gpu_tessellation(wgpu::Backend::Metal, "AMD Radeon Pro 560X"));
        assert!(!super::hangs_on_gpu_tessellation(wgpu::Backend::Vulkan, "AMD FirePro W5100"));
    }

    /// The plan evaluation compiled beside the window reaches the renderer once it is ready; one
    /// that does not compile never does, and bodies stay built on the CPU (#4860).
    #[test]
    fn a_renderer_takes_the_plan_evaluation_once_it_has_compiled() {
        let Ok((adapter, device, queue)) = crate::open_headless() else {
            eprintln!("skipped: no GPU adapter");
            return;
        };
        let depth = crate::depth_format(&device, true);
        let target = super::GpuTarget { format: wgpu::TextureFormat::Rgba8Unorm, depth: Some(depth), samples: 1, tess: false, oit: false };
        let compiles = super::tessellation(&adapter, &device, &queue, crate::SHADER).is_some();
        for (shader, expected) in [(crate::SHADER, compiles), ("not a shader", false)] {
            let mut later = super::TessLater::start(&adapter, &device, &queue, shader);
            later.wait();
            let mut renderer = super::Renderer::new(&device, target, None);
            renderer.take_ready_tessellation();
            assert!(renderer.tess.is_none(), "no evaluation before one is handed over");
            renderer.tessellate_later(later);
            renderer.take_ready_tessellation();
            assert_eq!(renderer.tess.is_some(), expected, "shader {shader:.12}");
        }
    }

    /// The viewport shader parses and validates (a bad shader would only fail at run time).
    #[test]
    fn shader_is_valid() {
        let module = naga::front::wgsl::parse_str(super::SHADER).unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
    }

    /// The ambient occlusion sample count rides in the uniform's `ao.y`, beside the radius, kept
    /// within the 1 to 32 samples the shader draws (#7165).
    #[test]
    fn ambient_occlusion_samples_follow_the_radius_in_the_uniform() {
        let samples_sent = |samples: u32| {
            let mut f = cached_frame();
            f.ao_radius = 123.5;
            f.ao_samples = samples;
            let bytes = super::uniform_bytes(&f, 64.0, 64.0, false, false);
            let v: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            let radius = v.iter().position(|&x| x == 123.5).unwrap();
            v[radius + 1]
        };
        assert_eq!(samples_sent(super::AO_SAMPLES), 32.0);
        assert_eq!(samples_sent(super::AO_SAMPLES_MOVING), 8.0);
        assert_eq!(samples_sent(0), 1.0, "at least one sample");
        assert_eq!(samples_sent(1000), 32.0, "at most the shader's 32");
    }

    /// The sky (#7974) is the uniform's last `vec4`: the world's up direction and the switch, with
    /// anything not finite sent as zero (shaded as before).
    #[test]
    fn the_sky_rides_at_the_end_of_the_uniform() {
        let last = |sky: [f32; 4]| {
            let mut f = cached_frame();
            f.sky = sky;
            let bytes = super::uniform_bytes(&f, 64.0, 64.0, false, false);
            assert_eq!(bytes.len() as u64, super::UNIFORM_SIZE);
            let v: Vec<f32> = bytes.as_chunks::<4>().0.iter().map(|c| f32::from_le_bytes(*c)).collect();
            v[v.len() - 4..].to_vec()
        };
        assert_eq!(last([0.0, 0.0, 1.0, 1.0]), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(last([0.0; 4]), [0.0; 4]);
        assert_eq!(last([f32::NAN, 1.0, f32::INFINITY, 1.0]), [0.0, 1.0, 0.0, 1.0]);
        // Turning it on draws the frame again.
        let mut f = cached_frame();
        let off = super::frame_key(&f);
        f.sky = [0.0, 0.0, 1.0, 1.0];
        assert_ne!(super::frame_key(&f), off);
    }

    #[test]
    fn render_lights_preview_shader_uniform_and_frame_cache_include_light_state() {
        let source = format!("{}\n{}", super::SHADER, crate::pbr::SOURCE);
        let module = naga::front::wgsl::parse_str(&source).unwrap_or_else(|error| panic!("{}", error.emit_to_string(&source)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
        let uniform = module.types.iter().find(|(_, ty)| ty.name.as_deref() == Some("U")).unwrap().1;
        let naga::TypeInner::Struct { span, .. } = &uniform.inner else { panic!("U is a uniform struct") };
        assert_eq!(u64::from(*span), super::UNIFORM_SIZE);
        let mut frame = cached_frame();
        let previous = super::frame_key(&frame);
        frame.product_lights = Some(vec![[0.0; 16]]);
        assert_ne!(super::frame_key(&frame), previous);
        assert_eq!(super::uniform_bytes(&frame, 800.0, 600.0, false, false).len() as u64, super::UNIFORM_SIZE);
        let previous = super::frame_key(&frame);
        frame.product_lights = Some(Vec::new());
        assert_ne!(super::frame_key(&frame), previous);
    }

    /// A frame with a fixed picture, and a key that names everything it shows.
    fn cached_frame() -> super::Frame {
        use std::sync::{Arc, Mutex};
        let slot = || Arc::new(Mutex::new(None));
        super::Frame {
            product_lights: None,
            product: false,
            upload_error: None,
            key: 1,
            slot: slot(),
            sketch_key: 2,
            sketch_slot: slot(),
            hl_key: 3,
            hl_slot: slot(),
            pv_key: 4,
            pv_slot: slot(),
            pv_hide: false,
            view_proj: [[0.0; 4]; 4],
            back: [0.0; 3],
            size_px: [800.0, 600.0],
            clip: [0.0; 4],
            cap: [0.0; 4],
            images: Vec::new(),
            clouds: Vec::new(),
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            access: None,
            hover_tint: [0.0; 3],
            selection_tint: [0.0; 3],
            outline: false,
            eye: [0.0; 4],
            ao_radius: 0.0,
            ao_samples: crate::view::AO_SAMPLES,
            sky: [0.0; 4],
            ssaa: 1.0,
        }
    }

    fn image(version: u64) -> super::GpuImage {
        super::GpuImage { id: 1, version, size: [2, 2], rgba: std::sync::Arc::new(vec![0; 16]), corners: [[0.0; 3]; 4], opacity: 1.0 }
    }

    fn access(version: u64) -> super::AccessMap {
        super::AccessMap { version, size: 2, depth: std::sync::Arc::new(vec![0.0; 4]), u: [1.0, 0.0, 0.0, 0.0], v: [0.0, 1.0, 0.0, 0.0], scale: 1.0 }
    }

    /// A picture is drawn again when anything in it changes; only an identical frame reuses it.
    #[test]
    fn a_frame_is_redrawn_when_anything_it_shows_changes() {
        let key = |change: fn(&mut super::Frame)| {
            let mut f = cached_frame();
            change(&mut f);
            super::frame_key(&f)
        };
        let base = key(|_| {});
        assert_eq!(base, key(|_| {}), "the same frame draws the same picture");
        for (what, changed) in [
            ("product", key(|f| f.product = true)),
            ("model", key(|f| f.key += 1)),
            ("sketch", key(|f| f.sketch_key += 1)),
            ("highlight", key(|f| f.hl_key += 1)),
            ("preview", key(|f| f.pv_key += 1)),
            ("preview hidden", key(|f| f.pv_hide = true)),
            ("camera", key(|f| f.view_proj[0][0] += 1.0)),
            ("lighting", key(|f| f.back[0] += 1.0)),
            ("eye", key(|f| f.eye[0] += 1.0)),
            ("size", key(|f| f.size_px[0] += 1.0)),
            ("section", key(|f| f.clip[0] = 1.0)),
            ("section cap", key(|f| f.cap[0] = 0.5)),
            ("analysis", key(|f| f.analysis[0] = 1.0)),
            ("analysis colours", key(|f| f.analysis_colors[0] = 1.0)),
            // The theme's hover and selection tints: a theme change draws the frame again, so
            // the picture cached under the previous theme's colours is not shown (#5802).
            ("hover tint", key(|f| f.hover_tint = [0.1, 0.2, 0.3])),
            ("selection tint", key(|f| f.selection_tint = [0.4, 0.5, 0.6])),
            ("outline", key(|f| f.outline = true)),
            ("ambient occlusion", key(|f| f.ao_radius = 1.0)),
            // The settled view's full count replaces the moving view's picture (#7165).
            ("ambient occlusion samples", key(|f| f.ao_samples = super::AO_SAMPLES_MOVING)),
            ("supersampling", key(|f| f.ssaa = 2.0)),
            ("image", key(|f| f.images.push(image(1)))),
            ("image pixels", key(|f| f.images.push(image(2)))),
            (
                "image opacity",
                key(|f| {
                    f.images.push(image(1));
                    if let Some(i) = f.images.first_mut() {
                        i.opacity = 0.5;
                    }
                }),
            ),
            ("accessibility", key(|f| f.access = Some(access(1)))),
            ("accessibility map", key(|f| f.access = Some(access(2)))),
        ] {
            assert_ne!(base, changed, "{what} must draw again");
        }
        // The comparison a frame makes: only a frame drawn into a texture has a picture to blit,
        // and only the picture with the same content is shown again.
        assert!(super::can_reuse(true, Some(base), base));
        assert!(!super::can_reuse(true, Some(base), base + 1));
        assert!(!super::can_reuse(false, Some(base), base));
        assert!(!super::can_reuse(true, None, base));
    }

    /// Derivatives and implicit-lod sampling run only in uniform control flow: browsers' WGSL
    /// compilers (Tint) reject them after a branch, and the whole viewport shader then fails,
    /// a black screen on the web (naga accepts it). Every fragment entry point makes such calls
    /// (directly or through a helper) before its first branch or return.
    #[test]
    fn derivatives_only_in_uniform_control_flow() {
        // Without comments (their words are not code).
        let src: String = super::SHADER.lines().map(|l| l.split("//").next().unwrap_or("")).collect::<Vec<_>>().join("\n");
        let src = src.as_str();
        let derivs = ["dpdx(", "dpdy(", "fwidth(", "textureSample("];
        // Functions: name -> body.
        let mut fns: Vec<(String, String, bool)> = Vec::new();
        let mut rest = src;
        let mut fragment = false;
        while let Some(at) = rest.find("fn ") {
            let before = &rest[..at];
            if before.contains("@fragment") {
                fragment = true;
            }
            let Some(open) = rest[at..].find('{') else { break };
            let name = rest[at + 3..].split('(').next().unwrap_or("").trim().to_string();
            let start = at + open;
            let mut depth = 0;
            let mut end = start;
            for (k, ch) in rest[start..].char_indices() {
                match ch {
                    '{' => depth += 1,
                    '}' => {
                        depth -= 1;
                        if depth == 0 {
                            end = start + k;
                            break;
                        }
                    }
                    _ => {}
                }
            }
            fns.push((name, rest[start..=end].to_string(), fragment));
            fragment = false;
            rest = &rest[end + 1..];
        }
        // Helpers that take derivatives (directly or through another helper).
        let mut uses: Vec<String> = Vec::new();
        for _ in 0..8 {
            for (n, body, _) in &fns {
                if !uses.contains(n) && (derivs.iter().any(|d| body.contains(d)) || uses.iter().any(|u| body.contains(&format!("{u}(")))) {
                    uses.push(n.clone());
                }
            }
        }
        for (n, body, frag) in &fns {
            let calls: Vec<usize> = derivs
                .iter()
                .map(|d| d.to_string())
                .chain(uses.iter().map(|u| format!("{u}(")))
                .flat_map(|c| body.match_indices(&c).map(|(i, _)| i).collect::<Vec<_>>())
                .collect();
            if calls.is_empty() {
                continue;
            }
            if !*frag {
                // A helper: its own derivatives must come before any branch or return in it.
                let first = ["if (", "return", "discard"].iter().filter_map(|k| body.find(k)).min().unwrap_or(usize::MAX);
                for d in derivs {
                    if let Some(i) = body.find(d) {
                        assert!(i < first, "{n}: {d} after a branch");
                    }
                }
                continue;
            }
            let first = ["if (", "return", "discard"].iter().filter_map(|k| body.find(k)).min().unwrap_or(usize::MAX);
            for c in calls {
                assert!(c < first, "fragment {n}: a derivative after a branch or return: {}", &body[c..(c + 40).min(body.len())]);
            }
        }
    }
}

#[cfg(test)]
mod glass_tests {
    use super::*;

    #[test]
    fn ground_shadow_does_not_require_order_independent_transparency() {
        let batch = || Some(Batch { parts: Vec::new(), chunk: TRI_SIZE * 6 });
        let mut batches = Batches { shadow: batch(), ..Default::default() };
        assert!(!batches.see_through());
        batches.glass = batch();
        assert!(batches.see_through(), "glass still requires transparency with a shadow");
        batches.glass = None;
        batches.trans = batch();
        assert!(batches.see_through(), "faded bodies still require transparency with a shadow");
    }

    #[test]
    fn ground_shadow_blends_and_stays_behind_opaque_geometry() {
        let mut scene = GpuScene::default();
        let square = [[-5.0, -5.0, -5.0], [5.0, -5.0, -5.0], [5.0, 5.0, -5.0], [-5.0, -5.0, -5.0], [5.0, 5.0, -5.0], [-5.0, 5.0, -5.0]];
        let shadow_color = scene.color([0, 0, 0, 128]);
        for p in square {
            scene.shadow_tri(p, shadow_color);
        }
        for samples in [1, 4] {
            let Some(shadow) = middle_pixel_with_samples(scene.clone(), true, samples) else {
                eprintln!("skipping: no GPU adapter");
                return;
            };
            assert!(shadow[..3].iter().all(|c| (125..=129).contains(c)), "{samples}× alpha-blended shadow: {shadow:?}");
            let mut occluded = scene.clone();
            let green = occluded.color([0, 255, 0, 255]);
            for mut p in square {
                p[2] = 5.0;
                occluded.tri(p, [0.0; 3], green);
            }
            assert_eq!(middle_pixel_with_samples(occluded, true, samples), Some([0, 255, 0, 255]), "opaque geometry hides the ground shadow");
        }
    }

    /// A large import's triangles once went into one buffer past the device's limit (524 MB
    /// against 256 MB) and the app crashed (#883): they are split into whole triangles.
    #[test]
    fn large_scenes_are_split_into_buffers_within_the_device_limit() {
        let limit = 256u64 << 20;
        for stride in [TRI_SIZE, LINE_SIZE, 4] {
            let chunk = buffer_chunk(stride, limit);
            assert!(chunk as u64 <= limit && chunk > limit as usize / 2);
            assert_eq!(chunk % (stride * 3), 0, "whole triangles");
            assert_eq!(chunk % (stride * 2), 0, "whole line pairs");
        }
        let scene = 524_434_680usize;
        assert_eq!(scene.div_ceil(buffer_chunk(TRI_SIZE, limit)), 2);
        assert!(buffer_chunk(TRI_SIZE, 1) >= TRI_SIZE * 6, "a tiny limit still holds one unit");
    }

    /// Bodies out of the view are cut out of a class's runs, wherever they fall in them: inside
    /// a run, over its ends, across two runs, or not at all.
    #[test]
    fn runs_without_hidden_bodies() {
        let r = |v: &[(u32, u32)]| v.iter().map(|&(a, b)| a..b).collect::<Vec<_>>();
        let cut = |runs: &[(u32, u32)], hidden: &[(u32, u32)]| runs_without(&r(runs), &r(hidden));
        assert_eq!(cut(&[(0, 30)], &[(9, 18)]), r(&[(0, 9), (18, 30)]));
        assert_eq!(cut(&[(0, 9), (12, 30)], &[(6, 15)]), r(&[(0, 6), (15, 30)]));
        assert_eq!(cut(&[(0, 9), (12, 30)], &[(0, 9), (27, 30)]), r(&[(12, 27)]));
        assert_eq!(cut(&[(3, 6)], &[(0, 3), (6, 9)]), r(&[(3, 6)]));
        assert_eq!(cut(&[(0, 30)], &[(0, 30)]), r(&[]));
        assert_eq!(cut(&[(0, 12), (15, 24)], &[]), r(&[(0, 12), (15, 24)]));
    }

    /// A line is written as one record: its ends, its palette index and its width word, each
    /// four little-endian bytes, as the shader reads them; a lifted line's word is
    /// [`lifted_width`]'s, whether worked out per segment or once for many.
    #[test]
    fn lines_are_written_as_one_record() {
        let (a, b, c): ([f32; 3], [f32; 3], _) = ([1.0, -2.0, 3.5], [4.0, 5.0, -6.25], PaletteIndex(7));
        let mut expected = Vec::new();
        for v in a.iter().chain(&b) {
            expected.extend_from_slice(&v.to_le_bytes());
        }
        expected.extend_from_slice(&7u32.to_le_bytes());
        expected.extend_from_slice(&1.5f32.to_le_bytes());
        let mut sc = GpuScene::default();
        sc.line(a, b, c, 1.5, false);
        sc.hidden_line(a, b, c, 1.5);
        assert_eq!(sc.lines, expected);
        assert_eq!(sc.hidden, expected);
        let normals = [[0.0, 0.0, 1.0], [1.0, 0.0, 0.0]];
        let (mut per_segment, mut shared) = (GpuScene::default(), GpuScene::default());
        per_segment.lifted_line(a, b, c, 2.0, normals);
        shared.lifted_line_word(a, b, c, lifted_width(2.0, normals));
        assert_eq!(per_segment.lines, shared.lines);
        assert_eq!(per_segment.lines.get(28..32), Some(&lifted_width(2.0, normals).to_le_bytes()[..]));
        let mut block = BlockBuilder::default();
        block.line(a, b, c, 1.5);
        assert_eq!(*block.freeze().lines, expected);
    }

    /// A vertex marker (#7834) is a lifted line from the vertex to itself, among the block's
    /// lines or, hidden, among its hidden lines.
    #[test]
    fn a_marker_is_a_lifted_line_to_itself() {
        let (p, c) = ([1.0f32, -2.0, 3.5], PaletteIndex(4));
        let mut block = BlockBuilder::default();
        block.marker(p, c, 4.0, false);
        block.marker(p, c, 4.0, true);
        let bytes = block.freeze();
        let mut sc = GpuScene::default();
        sc.lifted_line(p, p, c, 4.0, [[0.0; 3]; 2]);
        assert_eq!(*bytes.lines, sc.lines);
        assert_eq!(*bytes.hidden, sc.lines);
    }

    /// A marker shows as a square of its size at its vertex, over a face through the vertex even
    /// where the face rises toward the eye beside it (lifted, as a highlight band), and not
    /// through a face in front of it; a hidden marker shows only there. Skipped without a GPU.
    #[test]
    fn a_marker_shows_over_a_slanted_face_and_hides_behind_one() {
        let face = |sc: &mut GpuScene, slope: f32, z: f32, x0: f32| {
            let grey = sc.color([128, 128, 128, 255]);
            // A plane through (x0, 0, z), rising toward the eye (+z) along +x.
            let at = |x: f32, y: f32| [x, y, z + slope * (x - x0)];
            let n = [-slope, 0.0, 1.0];
            for t in [[at(-9.0, -9.0), at(9.0, -9.0), at(9.0, 9.0)], [at(-9.0, -9.0), at(9.0, 9.0), at(-9.0, 9.0)]] {
                for p in t {
                    sc.tri(p, n, grey);
                }
            }
        };
        // The middle pixel's centre is 0.16 mm right of the view's centre (0.31 mm a pixel): a
        // marker 0.3 mm left of it still covers that pixel, where the face stands 0.69 mm nearer
        // the eye than the vertex.
        let x0 = -0.3;
        let marker = |hidden: bool| {
            let mut b = BlockBuilder::default();
            b.marker([x0, 0.0, 0.0], PaletteIndex(0), 4.0, hidden);
            SceneBlock { key: u64::from(hidden), bytes: b.freeze() }
        };
        let scene = |slope: f32, z: f32, hidden: bool| {
            let mut sc = GpuScene::with_palette(Palette::with_stable(vec![[255, 0, 0, 255]]));
            face(&mut sc, slope, z, x0);
            sc.blocks.push(marker(hidden));
            sc
        };
        let red = [255, 0, 0, 255];
        let Some(slanted) = middle_pixel(scene(1.5, 0.0, false), false) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        assert_eq!(slanted, red, "on a slanted face through the vertex");
        assert_ne!(middle_pixel(scene(0.0, 5.0, false), false), Some(red), "behind a face in front");
        assert_eq!(middle_pixel(scene(0.0, 5.0, true), false), Some(red), "a hidden marker behind a face in front");
        assert_ne!(middle_pixel(scene(1.5, 0.0, true), false), Some(red), "no hidden marker where it is seen");
    }

    /// Without order-independent transparency, the sorted glass stream is kept while the blocks'
    /// see-through triangles stay the same: a colour edit (a new palette) uploads palette rows
    /// and no geometry; a class change rebuilds the stream.
    #[test]
    fn a_colour_edit_keeps_the_sorted_glass_stream() {
        let Ok((_, device, queue)) = crate::open_headless() else { return };
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r =
            Renderer::new(&device, GpuTarget { format, depth: Some(crate::depth_format(&device, true)), samples: 1, tess: false, oit: false }, None);
        let mut palette = Palette::with_stable(vec![[200, 0, 0, 128]]);
        let mut b = BlockBuilder::default();
        b.triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [[0.0, 0.0, 1.0]; 3], PaletteIndex(0), TriClass::Glass);
        let bytes = b.freeze();
        let scene = |palette: &Palette, bytes: &Arc<BlockBytes>| GpuScene {
            palette: Arc::new(palette.clone()),
            blocks: vec![SceneBlock { key: 7, bytes: bytes.clone() }],
            ..GpuScene::default()
        };
        let take = |r: &mut Renderer, key: u64, sc: GpuScene| {
            let slot: SceneSlot = Arc::new(Mutex::new(Some(sc)));
            r.model.take(&device, &queue, &r.uniform, &r.uniform_bgl, key, &slot, None, false);
            r.model.uploads
        };
        let first = take(&mut r, 1, scene(&palette, &bytes));
        assert_eq!(first.geometry, (2 * 3 * TRI_SIZE) as u64, "the block's vertices and the sorted stream: {first:?}");
        palette.set(PaletteIndex(0), [0, 0, 200, 128]);
        let colour = take(&mut r, 2, scene(&palette, &bytes));
        assert_eq!(colour.geometry, 0, "a colour edit keeps the stream: {colour:?}");
        assert!(colour.palette > 0, "and writes the palette: {colour:?}");
        let mut b = BlockBuilder::default();
        b.triangle([[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]], [[0.0, 0.0, 1.0]; 3], PaletteIndex(0), TriClass::Opaque);
        let opaque = take(&mut r, 3, scene(&palette, &b.freeze()));
        assert!(opaque.geometry > 0, "new block geometry is uploaded: {opaque:?}");
        let back = take(&mut r, 4, scene(&palette, &bytes));
        assert!(back.geometry >= (3 * TRI_SIZE) as u64, "the stream is rebuilt when the see-through set changes: {back:?}");
    }

    /// The middle pixel of `scene` drawn looking down -Z (x, y in -10..10) into a 64 × 64 frame,
    /// with order-independent transparency (`oit`) or with the see-through triangles sorted.
    /// `None` without a GPU adapter.
    fn middle_pixel(scene: GpuScene, oit: bool) -> Option<[u8; 4]> {
        middle_pixel_with_samples(scene, oit, 1)
    }

    fn middle_pixel_with_samples(scene: GpuScene, oit: bool, samples: u32) -> Option<[u8; 4]> {
        const SIZE: u32 = 64;
        let (_, device, queue) = crate::open_headless().ok()?;
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = Renderer::new(&device, GpuTarget { format, depth: Some(depth), samples, tess: false, oit: false }, None);
        assert!(r.oit.is_some(), "this device does order-independent transparency");
        if !oit {
            r.oit = None;
        }
        let none = || Arc::new(Mutex::new(None));
        let frame = Frame {
            product_lights: None,
            product: false,
            upload_error: None,
            key: 1,
            slot: Arc::new(Mutex::new(Some(scene))),
            sketch_key: 0,
            sketch_slot: none(),
            hl_key: 0,
            hl_slot: none(),
            pv_key: 0,
            pv_slot: none(),
            pv_hide: false,
            view_proj: [[0.1, 0.0, 0.0, 0.0], [0.0, 0.1, 0.0, 0.0], [0.0, 0.0, -0.01, 0.0], [0.0, 0.0, 0.5, 1.0]],
            back: [0.0, 0.0, 1.0],
            size_px: [SIZE as f32; 2],
            clip: [0.0; 4],
            cap: [0.0; 4],
            images: Vec::new(),
            clouds: Vec::new(),
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            access: None,
            outline: false,
            eye: [0.0, 0.0, 100.0, 0.0],
            ao_radius: 0.0,
            ao_samples: crate::view::AO_SAMPLES,
            sky: [0.0; 4],
            ssaa: 1.0,
            hover_tint: [0.0; 3],
            selection_tint: [0.0; 3],
        };
        let texture = |format, usage, sample_count| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC, 1);
        let msaa = (samples > 1).then(|| texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT, samples).create_view(&Default::default()));
        let depth_tex = texture(depth, wgpu::TextureUsages::RENDER_ATTACHMENT, samples);
        let (color_view, depth_view) = (color.create_view(&Default::default()), depth_tex.create_view(&Default::default()));
        let mut encoder = device.create_command_encoder(&Default::default());
        r.prepare(&device, &queue, &mut encoder, &frame);
        if !r.model.see_through() {
            assert!(r.ss.is_none(), "opaque geometry and the ground shadow use the direct path");
        }
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: msaa.as_ref().unwrap_or(&color_view),
                        depth_slice: None,
                        resolve_target: msaa.as_ref().map(|_| &color_view),
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(far_depth(reversed_depth(Some(depth)))),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: depth
                            .has_stencil_aspect()
                            .then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            r.paint(&frame, &mut pass, [0.0, 0.0, SIZE as f32, SIZE as f32]);
        }
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE * 4), rows_per_image: Some(SIZE) },
            },
            wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok()?;
        let bytes = read.slice(..).get_mapped_range().ok()?.to_vec();
        let at = ((SIZE / 2 * SIZE + SIZE / 2) * 4) as usize;
        bytes.get(at..at + 4).and_then(|p| p.try_into().ok())
    }

    /// Render `frame` through `r` into a fresh 64 × 64 target and read the pixels back.
    fn render_frame(r: &mut Renderer, device: &wgpu::Device, queue: &wgpu::Queue, frame: &Frame, with_depth: bool) -> Vec<u8> {
        const SIZE: u32 = 64;
        let depth = crate::depth_format(device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let texture = |format, usage| {
            device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format,
                usage,
                view_formats: &[],
            })
        };
        let color = texture(format, wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::COPY_SRC);
        let color_view = color.create_view(&Default::default());
        let depth_tex = with_depth.then(|| texture(depth, wgpu::TextureUsages::RENDER_ATTACHMENT));
        let depth_view = depth_tex.as_ref().map(|t| t.create_view(&Default::default()));
        let mut encoder = device.create_command_encoder(&Default::default());
        r.prepare(device, queue, &mut encoder, frame);
        {
            let mut pass = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view: &color_view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations { load: wgpu::LoadOp::Clear(wgpu::Color::WHITE), store: wgpu::StoreOp::Store },
                    })],
                    depth_stencil_attachment: depth_view.as_ref().map(|view| wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(far_depth(reversed_depth(Some(depth)))),
                            store: wgpu::StoreOp::Store,
                        }),
                        stencil_ops: depth
                            .has_stencil_aspect()
                            .then_some(wgpu::Operations { load: wgpu::LoadOp::Clear(0), store: wgpu::StoreOp::Store }),
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            r.paint(frame, &mut pass, [0.0, 0.0, SIZE as f32, SIZE as f32]);
        }
        let read = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: u64::from(SIZE * SIZE * 4),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        encoder.copy_texture_to_buffer(
            color.as_image_copy(),
            wgpu::TexelCopyBufferInfo {
                buffer: &read,
                layout: wgpu::TexelCopyBufferLayout { offset: 0, bytes_per_row: Some(SIZE * 4), rows_per_image: Some(SIZE) },
            },
            wgpu::Extent3d { width: SIZE, height: SIZE, depth_or_array_layers: 1 },
        );
        queue.submit([encoder.finish()]);
        read.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).ok();
        read.slice(..).get_mapped_range().map(|b| b.to_vec()).unwrap_or_default()
    }

    /// A frame showing `scene` at a fixed view, `ssaa` times the viewport's pixels each way.
    fn test_frame(scene: GpuScene, ssaa: f32) -> Frame {
        const SIZE: u32 = 64;
        let none = || Arc::new(Mutex::new(None));
        Frame {
            product_lights: None,
            product: false,
            upload_error: None,
            key: 1,
            slot: Arc::new(Mutex::new(Some(scene))),
            sketch_key: 0,
            sketch_slot: none(),
            hl_key: 0,
            hl_slot: none(),
            pv_key: 0,
            pv_slot: none(),
            pv_hide: false,
            view_proj: [[0.1, 0.0, 0.0, 0.0], [0.0, 0.1, 0.0, 0.0], [0.0, 0.0, -0.01, 0.0], [0.0, 0.0, 0.5, 1.0]],
            back: [0.0, 0.0, 1.0],
            size_px: [SIZE as f32; 2],
            clip: [0.0; 4],
            cap: [0.0; 4],
            images: Vec::new(),
            clouds: Vec::new(),
            analysis: [0.0; 8],
            analysis_colors: [0.0; 36],
            access: None,
            hover_tint: [0.0; 3],
            selection_tint: [0.0; 3],
            outline: false,
            eye: [0.0, 0.0, 100.0, 0.0],
            ao_radius: 0.0,
            ao_samples: crate::view::AO_SAMPLES,
            sky: [0.0; 4],
            ssaa,
        }
    }

    /// A frame of opaque triangles at the fixed view.
    fn test_scene() -> GpuScene {
        let mut scene = GpuScene::default();
        let red = scene.color([255, 0, 0, 255]);
        scene.tri([-5.0, -5.0, 0.0], [0.0, 0.0, 1.0], red);
        scene.tri([5.0, -5.0, 0.0], [0.0, 0.0, 1.0], red);
        scene.tri([0.0, 5.0, 0.0], [0.0, 0.0, 1.0], red);
        scene
    }

    #[test]
    fn a_cached_cloud_picture_follows_its_style_without_uploading_again() {
        let Some((_, device, queue)) = crate::open_headless().ok() else { return };
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut renderer = Renderer::new(&device, GpuTarget { format, depth: Some(depth), samples: 1, tess: false, oit: false }, None);
        let mut frame = test_frame(GpuScene::default(), 2.0);
        // A dot labelled 2 (red in the palette) and a dot filtered by 5 (a range) and the label
        // 2 (a set).
        let mut dots = Vec::new();
        cloud_dot(&mut dots, &Dot { at: [5.0, 5.0, 0.0], rgb: [0, 0, 255], value: 2.0, has: 16, ..Default::default() });
        let filtered = Dot { at: [-3.0, -3.0, 0.0], value: 1.0, filtered: [5.0, 2.0, 0.0, 0.0], rgb: [0, 255, 0], has: 0b10011 };
        cloud_dot(&mut dots, &filtered);
        let style = CloudStyle { mode: ColorMode::Own, range: [0.0, 1.0], filters: Default::default(), point_size: 20.0 };
        frame.clouds.push(GpuCloud { id: 1, version: 1, dots: Arc::new(dots), style });
        let has = |img: &[u8], f: &dyn Fn(&[u8; 4]) -> bool| img.as_chunks::<4>().0.iter().any(f);
        let green = |p: &[u8; 4]| p[1] > 120 && p[0] < 30 && p[2] < 30;
        let own = render_frame(&mut renderer, &device, &queue, &frame, true);
        assert!(has(&own, &|p| p[2] > 200 && p[0] < 30), "a dot in its own colour");
        assert!(has(&own, &green), "the other in its own colour");
        // Values from -1: bit 3 is the label 2, bit 2 the label 1.
        frame.clouds[0].style.filters[1] = CloudFilter::Values { offset: -1, mask: 0b1000 };
        assert!(has(&render_frame(&mut renderer, &device, &queue, &frame, true), &green), "label 2 kept");
        frame.clouds[0].style.filters[1] = CloudFilter::Values { offset: -1, mask: 0b0100 };
        assert!(!has(&render_frame(&mut renderer, &device, &queue, &frame, true), &green), "label 2 left out");
        frame.clouds[0].style.filters[1] = CloudFilter::Off;
        frame.clouds[0].style.filters[0] = CloudFilter::Range([0.0, 4.0]);
        frame.clouds[0].style.mode = ColorMode::Palette;
        let filtered = render_frame(&mut renderer, &device, &queue, &frame, true);
        assert!(!has(&filtered, &green), "its 5 lies outside the range");
        assert!(has(&filtered, &|p| p[0] > 200 && p[1] < 90 && p[2] < 90), "the first, without that value, in the palette's red");
        frame.clouds.clear();
        let hidden = render_frame(&mut renderer, &device, &queue, &frame, true);
        assert!(!has(&hidden, &|p| p[0] > 200 && p[1] < 90 && p[2] < 90), "hiding the cloud removes it");
    }

    /// A still view shows the picture it drew last: a reused frame (whose filtered picture is
    /// cached once) draws the same pixels as the frame before it, and changing the camera draws
    /// again rather than keeping the cached picture.
    #[test]
    fn a_still_frame_blits_the_cached_picture() {
        let Some((_, device, queue)) = crate::open_headless().ok() else { return };
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = Renderer::new(&device, GpuTarget { format, depth: Some(depth), samples: 1, tess: false, oit: false }, None);
        // Supersampled: the picture is drawn into a texture, so it can be cached.
        let mut frame = test_frame(test_scene(), 2.0);
        let first = render_frame(&mut r, &device, &queue, &frame, true);
        assert!(r.ss_ready && r.ss_scaled, "the first frame is drawn supersampled");
        let again = render_frame(&mut r, &device, &queue, &frame, true);
        assert!(r.cached.is_some(), "the still frame's filtered picture is cached");
        // The cached picture goes through one more 8-bit texture than the direct filter.
        assert!(first.iter().zip(&again).all(|(a, b)| a.abs_diff(*b) <= 1), "a reused frame draws the same picture");
        // A moved camera is a new picture: the cache is dropped and the scene drawn again.
        frame.view_proj[3][0] += 0.5;
        let moved = render_frame(&mut r, &device, &queue, &frame, true);
        assert!(r.cached.is_none(), "a changed view cannot show the cached picture");
        assert_ne!(first, moved, "the changed view is drawn");
    }

    /// A window pass of another kind (one sample, no depth) shows the same picture: the scene is
    /// drawn offscreen with its own depth and MSAA, then blitted into the window's pass.
    #[test]
    fn a_split_window_target_blits_the_offscreen_scene() {
        let Some((_, device, queue)) = crate::open_headless().ok() else { return };
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let scene_target = GpuTarget { format, depth: Some(depth), samples: 1, tess: false, oit: false };
        let window_target = GpuTarget { format, depth: None, samples: 1, tess: false, oit: false };
        let mut direct = Renderer::new(&device, scene_target, None);
        let mut split = Renderer::with_window_target(&device, scene_target, window_target, None);
        // Each renderer takes the scene out of its slot: a frame of its own for each.
        let drawn = render_frame(&mut direct, &device, &queue, &test_frame(test_scene(), 1.0), true);
        let shown = render_frame(&mut split, &device, &queue, &test_frame(test_scene(), 1.0), false);
        assert!(split.ss_ready, "the split frame is drawn offscreen");
        let max = drawn.iter().zip(&shown).map(|(a, b)| a.abs_diff(*b)).max().unwrap_or(0);
        assert!(max <= 1, "the blitted picture matches the direct one (max difference {max})");
    }

    /// A GT 740 had no video memory left for the viewport's frame, and the app closed with "wgpu
    /// error: Out of Memory" (#7296). A frame the device refuses is drawn smaller instead, and the
    /// device's refusals are reported. A texture past the device's largest side stands in for
    /// one past its memory: the renderer is told the device takes larger textures than it does,
    /// and the device refuses them the same way. An error that escaped would panic here (the
    /// test device keeps wgpu's default handler).
    #[test]
    fn a_frame_the_device_refuses_is_drawn_smaller() {
        let Some((_, device, queue)) = crate::open_headless().ok() else { return };
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let scene_target = GpuTarget { format, depth: Some(depth), samples: 1, tess: false, oit: false };
        let window_target = GpuTarget { format, depth: None, samples: 1, tess: false, oit: false };
        let limit = device.limits().max_texture_dimension_2d;
        let red = |pixels: &[u8]| pixels.get((32 * 64 + 32) * 4..(32 * 64 + 32) * 4 + 4).is_some_and(|p| p[0] > 200 && p[1] < 60 && p[2] < 60);
        // Supersampled 4× (a wide, flat viewport) and at the viewport's own size: each frame is
        // refused at first, so drawn smaller, and the view still shows the model.
        for (width, ssaa) in [(limit / 2 + 64, 4.0), (limit * 2, 1.0)] {
            let mut r = Renderer::with_window_target(&device, scene_target, window_target, None);
            r.max_texture = limit.saturating_mul(4);
            let mut frame = test_frame(test_scene(), ssaa);
            frame.size_px = [width as f32, 64.0];
            let shown = render_frame(&mut r, &device, &queue, &frame, false);
            let size = r.ss.as_ref().map(|ss| ss.size);
            assert!(r.ss_ready && size.is_some_and(|[w, _]| w <= limit), "{width} px at {ssaa}×: drawn within the device ({size:?})");
            assert!(red(&shown), "{width} px at {ssaa}×: the view shows the model");
            let stats = r.stats().lock().map(|s| *s).unwrap_or_default();
            assert!(stats.lowered, "{width} px at {ssaa}×: the lower quality is reported");
            // The next frame asks for no more than the device gave.
            let budget = r.frame_budget;
            frame.view_proj[3][0] += 0.01;
            assert!(
                red(&render_frame(&mut r, &device, &queue, &frame, false)) && r.frame_budget == budget && r.ss.as_ref().map(|ss| ss.size) == size
            );
        }
    }

    /// A multisampled scene is resolved into textures nothing else writes. Direct3D 12 needs such a
    /// texture cleared before its first use, and on some AMD Radeon cards an uncleared one showed
    /// the scene as nothing at all: no grid, origin planes or bodies (#6828, #7157). Each new
    /// resolve texture is cleared once when it is made (not every frame), and the picture shows.
    #[test]
    fn new_resolve_targets_are_cleared_before_their_first_resolve() {
        let Some((adapter, device, queue)) = crate::open_headless().ok() else { return };
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        if ![format, depth].iter().all(|f| adapter.get_texture_format_features(*f).flags.sample_count_supported(4)) {
            return;
        }
        let window = GpuTarget { format, depth: None, samples: 1, tess: false, oit: false };
        let mut r = Renderer::with_window_target(&device, GpuTarget { depth: Some(depth), samples: 4, ..window }, window, None);
        let middle = |pixels: &[u8]| -> [i32; 4] {
            let at = (32 * 64 + 32) * 4;
            std::array::from_fn(|i| pixels.get(at + i).map_or(-1, |v| i32::from(*v)))
        };
        let mut frame = test_frame(test_scene(), 1.0);
        let shown = middle(&render_frame(&mut r, &device, &queue, &frame, false));
        assert!(shown[0] > 200 && shown[1] < 60, "the multisampled scene shows its red triangle: {shown:?}");
        assert_eq!(r.resolve_clears, 1, "the new frame's resolved colour is cleared first");
        frame.view_proj[3][0] += 0.25;
        render_frame(&mut r, &device, &queue, &frame, false);
        assert_eq!(r.resolve_clears, 1, "a frame of the same size keeps its textures and clears nothing");
        // Supersampled: a larger frame, so new textures, cleared again.
        render_frame(&mut r, &device, &queue, &test_frame(test_scene(), 2.0), false);
        assert_eq!(r.resolve_clears, 2, "the supersampled frame's new resolved colour is cleared first");
        // See-through surfaces: the order-independent transparency's resolved sums too.
        if r.oit.is_some() {
            // A new scene key, so the renderer takes the see-through scene instead of keeping its own.
            let glass = Frame { key: 2, ..test_frame(two_layers([220, 40, 40, 128], [40, 40, 220, 128], true), 1.0) };
            render_frame(&mut r, &device, &queue, &glass, false);
            assert!(r.model.see_through() && r.oit_targets.is_some(), "the see-through frame is drawn with order-independent transparency");
            assert_eq!(r.resolve_clears, 5, "the frame's colour and both see-through sums are cleared first");
        }
        // One sample: the scene's own pass clears the texture it draws into, and nothing is resolved.
        let mut single = Renderer::with_window_target(&device, GpuTarget { depth: Some(depth), samples: 1, ..window }, window, None);
        render_frame(&mut single, &device, &queue, &test_frame(test_scene(), 1.0), false);
        assert!(single.ss_ready, "the 1x scene is drawn offscreen too");
        assert_eq!(single.resolve_clears, 0, "at one sample no texture is resolved into");
    }

    /// The Render workspace's preview once painted the modelling viewport's last frame instead of
    /// its own (#5119): after a frame with see-through surfaces, drawn offscreen for
    /// order-independent transparency, it stayed frozen on that frame whatever the camera did.
    #[test]
    fn product_preview_never_paints_the_last_modelling_frame() {
        let Some((_, device, queue)) = crate::open_headless().ok() else { return };
        let depth = crate::depth_format(&device, true);
        let format = wgpu::TextureFormat::Rgba8Unorm;
        let mut r = Renderer::new(&device, GpuTarget { format, depth: Some(depth), samples: 1, tess: false, oit: false }, None);
        let middle = |pixels: &[u8]| -> [i32; 4] {
            let at = (32 * 64 + 32) * 4;
            std::array::from_fn(|i| pixels.get(at + i).map_or(-1, |v| i32::from(*v)))
        };
        let red = [220, 40, 40, 128];
        let model = middle(&render_frame(&mut r, &device, &queue, &test_frame(two_layers(red, red, true), 1.0), true));
        assert!(r.ss_ready, "the see-through modelling frame is drawn offscreen");
        assert!(model[0] > model[2] + 60, "the modelling frame shows its red layers: {model:?}");
        let mut product = GpuScene::default();
        let blue = product.color([40, 80, 220, 255]);
        for p in [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]] {
            product.tri([p[0], p[1], 0.0], [0.0, 0.0, 1.0], blue);
        }
        let mut frame = test_frame(product, 1.0);
        frame.product = true;
        let preview = middle(&render_frame(&mut r, &device, &queue, &frame, true));
        assert!(preview[2] > preview[0] + 60, "the preview shows its own blue square: {preview:?}");
    }

    /// The startup check that picks the viewport's anti-aliasing: it passes where the device
    /// draws the multisampled view, and a blank 1x resolve is detected. A machine with no
    /// software adapter has nothing to check, so the check is skipped there instead of failing
    /// the build (`SOLIDCRAFT_REQUIRE_GPU=1` makes a missing adapter a failure, as in CI).
    #[test]
    fn startup_msaa_check_passes_and_detects_a_blank_resolve() {
        let (adapter, device, queue) = match crate::open_headless() {
            Ok(d) => d,
            Err(e @ crate::GpuError::NoAdapter(_)) if std::env::var_os("SOLIDCRAFT_REQUIRE_GPU").is_none() => {
                eprintln!("skipping: {e}");
                return;
            }
            Err(e) => panic!("the startup check needs a GPU: {e}"),
        };
        let depth = crate::depth_format(&device, true);
        for format in [wgpu::TextureFormat::Rgba8Unorm, wgpu::TextureFormat::Bgra8Unorm, wgpu::TextureFormat::Bgra8UnormSrgb] {
            let window = GpuTarget { format, depth: None, samples: 1, tess: false, oit: false };
            for samples in [1, 4] {
                if ![format, depth].iter().all(|f| adapter.get_texture_format_features(*f).flags.sample_count_supported(samples)) {
                    continue;
                }
                let target = GpuTarget { depth: Some(depth), samples, ..window };
                let mut renderer = Renderer::with_window_target(&device, target, window, None);
                let start = std::time::Instant::now();
                assert_eq!(renderer.check_msaa(&device, &queue).unwrap(), super::MsaaCheck::Passed);
                eprintln!("startup check: {} {format:?} {samples}x: {:?}", adapter.get_info().name, start.elapsed());
                assert!(
                    renderer.model.key.is_none() && renderer.highlight.key.is_none() && renderer.ss.is_none(),
                    "probe uploads must not replace the first real frame"
                );
                assert_eq!(renderer.target().samples, samples);
                assert_eq!(renderer.check_msaa_with(&device, &queue, 1).unwrap(), super::MsaaCheck::FailedAtOne);
                assert_eq!(renderer.target().samples, samples, "a blank 1x result must preserve the selected count");
                assert_eq!(
                    renderer.check_msaa_with(&device, &queue, 4).unwrap(),
                    if samples > 1 { super::MsaaCheck::Fallback } else { super::MsaaCheck::Passed }
                );
                assert_eq!(renderer.target().samples, 1);
                assert_eq!(renderer.check_msaa(&device, &queue).unwrap(), super::MsaaCheck::Passed);
            }
        }
    }

    /// Two see-through squares, one at z = 5 (near) and one at z = -5, in `near` and `far`
    /// colours, given near first or far first.
    fn two_layers(near: [u8; 4], far: [u8; 4], near_first: bool) -> GpuScene {
        let mut sc = GpuScene::default();
        let mut layers = [(5.0f32, near), (-5.0, far)];
        if !near_first {
            layers.reverse();
        }
        for (z, c) in layers {
            let c = sc.color(c);
            for p in [[-5.0, -5.0], [5.0, -5.0], [5.0, 5.0], [-5.0, -5.0], [5.0, 5.0], [-5.0, 5.0]] {
                sc.glass_tri([p[0], p[1], z], [0.0, 0.0, 1.0], c);
            }
        }
        sc
    }

    /// Order-independent transparency: the same see-through layers give the same picture in
    /// whichever order they come, the same as sorting them where they share a colour, and close to
    /// it where they do not (the weighted average of their colours, by how much they cover).
    /// Skipped without a GPU adapter.
    #[test]
    fn see_through_layers_blend_in_any_order() {
        let (blue, red) = ([40, 80, 220, 128], [220, 40, 40, 128]);
        let Some(either) = middle_pixel(two_layers(blue, red, true), true) else {
            eprintln!("skipping: no GPU adapter");
            return;
        };
        let other = middle_pixel(two_layers(blue, red, false), true).unwrap();
        assert!(either.iter().zip(other).all(|(a, b)| a.abs_diff(b) <= 1), "{either:?} vs {other:?}");
        let near = |a: [u8; 4], b: [u8; 4], by: u8| a.iter().zip(b).all(|(x, y)| x.abs_diff(y) <= by);
        let same = middle_pixel(two_layers(blue, blue, false), true).unwrap();
        let sorted = middle_pixel(two_layers(blue, blue, false), false).unwrap();
        assert!(near(same, sorted, 3), "one colour: {same:?} vs sorted {sorted:?}");
        let sorted = middle_pixel(two_layers(blue, red, false), false).unwrap();
        assert!(near(either, sorted, 48), "two colours: {either:?} vs sorted {sorted:?}");
        assert!(either[0] > 100 && either[2] > 100, "both colours show: {either:?}");
    }

    #[test]
    fn glass_is_drawn_far_to_near() {
        // Two triangles at z = 5 and z = -5, given near first; looking down from +Z.
        let mut sc = GpuScene::default();
        let red = sc.color([255, 0, 0, 100]);
        for z in [5.0f32, -5.0] {
            for p in [[0.0, 0.0, z], [1.0, 0.0, z], [0.0, 1.0, z]] {
                sc.glass_tri(p, [0.0, 0.0, 1.0], red);
            }
        }
        let sorted = sort_back_to_front(&sc.glass, [0.0, 0.0, 1.0]);
        let z0 = f32::from_le_bytes(sorted[8..12].try_into().unwrap());
        assert_eq!(z0, -5.0, "the far triangle first");
        assert_eq!(sorted.len(), sc.glass.len());
        // Seen from below, the order flips.
        let sorted = sort_back_to_front(&sc.glass, [0.0, 0.0, -1.0]);
        assert_eq!(f32::from_le_bytes(sorted[8..12].try_into().unwrap()), 5.0);
    }
}
