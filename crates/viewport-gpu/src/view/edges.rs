//! Model edges of parts placed many times, drawn from each part's shared edges
//! ([`super::InstancedMesh::edges`]): every part's segment records (and their coarser levels)
//! in one storage buffer, the placements in view in one instance buffer (each with the level its
//! edges are drawn from), and one draw's arguments per part and level in one indirect buffer.
//! The model's instanced edges are then drawn with one `multi_draw_indirect` (and its hidden
//! edges with another), where they were each body's lines in the scene's blocks, a call per
//! block and never culled: thousands of calls a frame for a large assembly placed many times.
//!
//! The vertex shader (`vs_edge_inst`, [`SHADER`]) pulls segment `vertex_index / 6` from the
//! storage buffer and moves it by the instance's placement, so a segment is stored once per
//! part, not once per placement. Only on a device with storage buffers in its vertex shaders
//! and indirect draws that start at an instance ([`supported`]).

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use super::{EDGE_SEG_SIZE, INSTANCE_SIZE, InstancedDraw, SharedEdges, Tally};

/// The instanced edge vertex shaders, appended to the viewport's (they draw with its
/// `line_vertex` and `fs_line`).
pub(super) const SHADER: &str = r#"
struct EdgeSeg {
    a: vec3<f32>,
    flags: u32,
    b: vec3<f32>,
    edge: u32,
};

@group(3) @binding(0) var<storage, read> edge_segs: array<EdgeSeg>;

fn edge_moved(r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, p: vec3<f32>) -> vec3<f32> {
    return vec3<f32>(dot(r0.xyz, p) + r0.w, dot(r1.xyz, p) + r1.w, dot(r2.xyz, p) + r2.w);
}

/// Segment `vertex_index / 6` of the part's shared edges at the instance's placement, in its
/// edge colour (`EDGE_WIDTH` pixels wide).
@vertex
fn vs_edge_inst(@builtin(vertex_index) vi: u32, @location(3) r0: vec4<f32>, @location(4) r1: vec4<f32>, @location(5) r2: vec4<f32>, @location(7) edge: u32, @location(8) hidden: u32, @location(9) slot: u32) -> LOut {
    let s = edge_segs[vi / 6u];
    return line_vertex(vi % 6u, edge_moved(r0, r1, r2, s.a), edge_moved(r0, r1, r2, s.b), edge, bitcast<u32>(1.3), edge_motion(slot, edge));
}

/// The motion an instance's edges are drawn moved by: its own slot, else the one the palette entry
/// `c` they are drawn in names (only read while something moves).
fn edge_motion(slot: u32, c: u32) -> u32 {
    if (u.screen.z < 0.5) { return 0u; }
    return placement_motion(slot, palette_entry(c));
}

/// A lit segment (a whole hovered or selected body's edges, drawn again over the model) in
/// `style`: the palette index from bit 8, a raised line at bit 7, the width in eighths of a pixel
/// below it (`lit_line_style`, as `vs_line_lit` reads it).
fn edge_lit(vi: u32, r0: vec4<f32>, r1: vec4<f32>, r2: vec4<f32>, style: u32, slot: u32) -> LOut {
    let s = edge_segs[vi / 6u];
    let eighths = style & 127u;
    var w = bitcast<u32>(f32(eighths) / 8.0);
    if ((style & 128u) != 0u) { w = 0x80000000u | (eighths << 24u); }
    return line_vertex(vi % 6u, edge_moved(r0, r1, r2, s.a), edge_moved(r0, r1, r2, s.b), style >> 8u, w, edge_motion(slot, style >> 8u));
}

/// A lit segment in the instance's first style (the halo, or the hidden edges' style).
@vertex
fn vs_edge_inst_lit_a(@builtin(vertex_index) vi: u32, @location(3) r0: vec4<f32>, @location(4) r1: vec4<f32>, @location(5) r2: vec4<f32>, @location(7) a: u32, @location(8) b: u32, @location(9) slot: u32) -> LOut {
    return edge_lit(vi, r0, r1, r2, a, slot);
}

/// A lit segment in the instance's second style (the line over the halo).
@vertex
fn vs_edge_inst_lit_b(@builtin(vertex_index) vi: u32, @location(3) r0: vec4<f32>, @location(4) r1: vec4<f32>, @location(5) r2: vec4<f32>, @location(7) a: u32, @location(8) b: u32, @location(9) slot: u32) -> LOut {
    return edge_lit(vi, r0, r1, r2, b, slot);
}

/// The same segment as a hidden edge, in its hidden edge colour (`HIDDEN_EDGE_WIDTH` wide).
@vertex
fn vs_edge_inst_hidden(@builtin(vertex_index) vi: u32, @location(3) r0: vec4<f32>, @location(4) r1: vec4<f32>, @location(5) r2: vec4<f32>, @location(7) edge: u32, @location(8) hidden: u32, @location(9) slot: u32) -> LOut {
    let s = edge_segs[vi / 6u];
    return line_vertex(vi % 6u, edge_moved(r0, r1, r2, s.a), edge_moved(r0, r1, r2, s.b), hidden, bitcast<u32>(1.0), edge_motion(slot, hidden));
}
"#;

/// Vertices drawn per segment (two triangles).
const VERTICES: u32 = 6;
/// Bytes of one draw's arguments ([`wgpu::util::DrawIndirectArgs`]).
const ARGS_SIZE: u64 = 16;

/// Whether `device` draws instanced edges this way: storage buffers in vertex shaders (WebGL2
/// has none) and indirect draws that start at an instance.
pub(super) fn supported(device: &wgpu::Device) -> bool {
    device.limits().max_storage_buffers_per_shader_stage > 0 && super::indirect::supported(device)
}

/// The placements' layout for the edge shaders: their rows, the edge palette indices and the
/// motion slot ([`INSTANCE_SIZE`]).
pub(super) fn instance_layout() -> wgpu::VertexBufferLayout<'static> {
    const A: [wgpu::VertexAttribute; 7] =
        wgpu::vertex_attr_array![3 => Float32x4, 4 => Float32x4, 5 => Float32x4, 6 => Uint32, 7 => Uint32, 8 => Uint32, 9 => Uint32];
    wgpu::VertexBufferLayout { array_stride: INSTANCE_SIZE as u64, step_mode: wgpu::VertexStepMode::Instance, attributes: &A }
}

/// The bind group layout of the segment buffer (group 3).
pub(super) fn bind_layout(device: &wgpu::Device) -> wgpu::BindGroupLayout {
    device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some("sc_edge_segments"),
        entries: &[wgpu::BindGroupLayoutEntry {
            binding: 0,
            visibility: wgpu::ShaderStages::VERTEX,
            ty: wgpu::BindingType::Buffer {
                ty: wgpu::BufferBindingType::Storage { read_only: true },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        }],
    })
}

/// Every part's segments in one storage buffer: where each level of each part's edges starts
/// (segments) and how many it has, by the address of its edges (held, so it stays theirs). The
/// buffer has room to grow: a scene that adds parts writes only theirs after the others (the
/// parts of a design being opened came a few at a time, and each scene uploaded every part's
/// edges again), and it is made anew only when they do not fit or it would hold more edges no
/// longer drawn than drawn.
struct Arena {
    buffer: wgpu::Buffer,
    bind: wgpu::BindGroup,
    /// Bytes the buffer has room for.
    room: u64,
    /// Bytes written into the buffer, by this arena and every one grown from it: shared, so two
    /// arenas over the same buffer never write the same bytes.
    taken: Arc<Mutex<u64>>,
    spans: HashMap<usize, (Arc<SharedEdges>, Vec<(u32, u32)>)>,
}

/// The bytes of all `e`'s levels.
fn edge_bytes(e: &SharedEdges) -> u64 {
    e.levels.iter().map(|(r, _)| r.len() as u64).sum()
}

impl Arena {
    /// An arena holding every part's edges in `parts`: `before`'s when it holds them all, else
    /// `before`'s buffer with the parts it lacks written into its room, else a new one. With the
    /// bytes uploaded.
    fn holding(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        parts: &[Arc<SharedEdges>],
        before: Option<&Arc<Arena>>,
    ) -> Option<(Arc<Arena>, u64)> {
        let mut seen = std::collections::HashSet::with_capacity(parts.len());
        let wanted: Vec<&Arc<SharedEdges>> = parts.iter().filter(|e| seen.insert(Arc::as_ptr(e) as usize)).collect();
        let need: u64 = wanted.iter().map(|e| edge_bytes(e)).sum();
        if let Some(before) = before {
            let lacking: Vec<&Arc<SharedEdges>> = wanted.iter().copied().filter(|e| before.levels(e).is_none()).collect();
            if lacking.is_empty() {
                return Some((before.clone(), 0));
            }
            if let Some(grown) = before.with(queue, &wanted, &lacking, need) {
                return Some(grown);
            }
        }
        Arena::new(device, queue, layout, &wanted, need)
    }

    /// The largest buffer `device` binds as storage, in whole segments.
    fn limit(device: &wgpu::Device) -> u64 {
        let limit = device.limits().max_storage_buffer_binding_size.min(device.limits().max_buffer_size);
        // Vertex indices count six per segment: they must fit a u32.
        let segments = (limit / EDGE_SEG_SIZE as u64).min(u64::from(u32::MAX / VERTICES));
        segments * EDGE_SEG_SIZE as u64
    }

    /// A new buffer with room for half as much again as `wanted` (`need` bytes), holding them.
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        wanted: &[&Arc<SharedEdges>],
        need: u64,
    ) -> Option<(Arc<Arena>, u64)> {
        let limit = Arena::limit(device);
        if need == 0 || need > limit {
            return None;
        }
        let room = (need + need / 2).min(limit).next_multiple_of(EDGE_SEG_SIZE as u64).min(limit);
        // A buffer or bind group the device refuses fails the scene's upload, which keeps the
        // previous picture and tries the scene again ([`super::refused`]).
        let scopes = crate::error_scopes(device);
        let buffer = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_edge_segments"),
            size: room,
            usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_edge_segments"),
            layout,
            entries: &[wgpu::BindGroupEntry { binding: 0, resource: buffer.as_entire_binding() }],
        });
        if let Some(reason) = crate::caught(scopes) {
            super::refused(reason);
            return None;
        }
        let empty = Arena { buffer, bind, room, taken: Arc::new(Mutex::new(0)), spans: HashMap::new() };
        empty.with(queue, wanted, wanted, need)
    }

    /// This arena's buffer holding `wanted` (`need` bytes), the `lacking` ones written into its
    /// room; `None` when they do not fit there or the buffer would hold more no longer drawn
    /// than drawn.
    fn with(&self, queue: &wgpu::Queue, wanted: &[&Arc<SharedEdges>], lacking: &[&Arc<SharedEdges>], need: u64) -> Option<(Arc<Arena>, u64)> {
        let add: u64 = lacking.iter().map(|e| edge_bytes(e)).sum();
        let mut taken = self.taken.lock().ok()?;
        let at = *taken;
        let after = at + add;
        if after > self.room || after > 2 * need {
            return None;
        }
        let mut bytes = Vec::with_capacity(usize::try_from(add).ok()?);
        let mut placed = HashMap::with_capacity(lacking.len());
        for e in lacking {
            let mut levels = Vec::with_capacity(e.levels.len());
            for (records, _) in &e.levels {
                let first = u32::try_from((at + bytes.len() as u64) / EDGE_SEG_SIZE as u64).ok()?;
                levels.push((first, u32::try_from(records.len() / EDGE_SEG_SIZE).ok()?));
                bytes.extend_from_slice(records);
            }
            placed.insert(Arc::as_ptr(e) as usize, ((*e).clone(), levels));
        }
        if !bytes.is_empty() {
            queue.write_buffer(&self.buffer, at, &bytes);
        }
        *taken = after;
        drop(taken);
        let mut spans = HashMap::with_capacity(wanted.len());
        for e in wanted {
            let key = Arc::as_ptr(e) as usize;
            let span = placed.remove(&key).or_else(|| self.spans.get(&key).filter(|(k, _)| Arc::ptr_eq(k, e)).cloned())?;
            spans.insert(key, span);
        }
        let grown = Arena { buffer: self.buffer.clone(), bind: self.bind.clone(), room: self.room, taken: self.taken.clone(), spans };
        Some((Arc::new(grown), add))
    }

    fn levels(&self, e: &Arc<SharedEdges>) -> Option<&[(u32, u32)]> {
        self.spans.get(&(Arc::as_ptr(e) as usize)).filter(|(k, _)| Arc::ptr_eq(k, e)).map(|(_, l)| l.as_slice())
    }
}

/// A part's edges as drawn at its placements: the placements ([`INSTANCE_SIZE`] bytes each), and
/// which of them are drawn from which level (`None`: all, at full detail).
pub(super) struct Part<'a> {
    pub edges: &'a Arc<SharedEdges>,
    pub placements: &'a [u8],
    pub shown: Option<&'a [(u32, u8)]>,
}

impl<'a> Part<'a> {
    /// The parts of a scene's instanced draws that have edges, in the draws' order.
    pub(super) fn of_draws(draws: &'a [InstancedDraw]) -> Vec<Part<'a>> {
        draws.iter().filter_map(|d| Some(Part { edges: d.edges.as_ref()?, placements: &d.placements, shown: d.edges_shown.as_deref() })).collect()
    }

    /// The parts of instanced meshes that have edges, every placement at full detail (a
    /// highlight's lit edges).
    pub(super) fn of_meshes(meshes: &'a [super::InstancedMesh]) -> Vec<Part<'a>> {
        meshes.iter().filter_map(|m| Some(Part { edges: m.edges.as_ref()?, placements: &m.instances, shown: None })).collect()
    }
}

/// One indirect entry: level `level` of part `draw`'s edges, `count` segments from `first`.
#[derive(Clone, Copy, Debug)]
struct Entry {
    draw: usize,
    level: usize,
    first: u32,
    count: u32,
}

/// The instanced edges of a scene.
#[derive(Clone)]
pub(super) struct Edges {
    arena: Arc<Arena>,
    entries: Vec<Entry>,
    args: Option<wgpu::Buffer>,
    instances: Option<(wgpu::Buffer, u64)>,
    /// Segments the entries draw now (the draw statistics).
    segments: u64,
}

impl Edges {
    /// The edges of the scene's instanced `draws` that have them, in `before`'s segment buffer
    /// (only the parts it lacks are uploaded: none for a scene rebuilt for a colour or visibility
    /// change), with the bytes uploaded. `None` when no draw has edges or they cannot be
    /// uploaded.
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layout: &wgpu::BindGroupLayout,
        parts: &[Part<'_>],
        before: Option<&Edges>,
    ) -> Option<(Edges, u64)> {
        if parts.is_empty() {
            return None;
        }
        let shared: Vec<Arc<SharedEdges>> = parts.iter().map(|p| p.edges.clone()).collect();
        let (arena, uploaded) = Arena::holding(device, queue, layout, &shared, before.map(|b| &b.arena))?;
        let mut entries = Vec::new();
        for (i, p) in parts.iter().enumerate() {
            let e = p.edges;
            for (level, (first, count)) in arena.levels(e)?.iter().enumerate() {
                entries.push(Entry { draw: i, level, first: *first, count: *count });
            }
        }
        let mut out = Edges { arena, entries, args: None, instances: None, segments: 0 };
        let (instances, args) = out.contents(parts);
        out.args = Some(super::buffer_init(device, "sc_edge_args", &args, wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST)?);
        if !instances.is_empty() {
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
            out.instances = Some((super::buffer_init(device, "sc_edge_instances", &instances, usage)?, instances.len() as u64));
        }
        Some((out, uploaded))
    }

    /// The placements in view by edge level (`draws`' `edges_shown`; every placement at full
    /// detail before the first cull) and each entry's arguments. Counts the segments drawn.
    fn contents(&mut self, parts: &[Part<'_>]) -> (Vec<u8>, Vec<u8>) {
        let mut instances: Vec<u8> = Vec::new();
        let mut args: Vec<u8> = Vec::with_capacity(self.entries.len() * ARGS_SIZE as usize);
        self.segments = 0;
        for e in &self.entries {
            let first = u32::try_from(instances.len() / INSTANCE_SIZE).unwrap_or(u32::MAX);
            if let Some(d) = parts.get(e.draw) {
                match d.shown {
                    Some(shown) => {
                        let level = u8::try_from(e.level).unwrap_or(u8::MAX);
                        for (i, _) in shown.iter().filter(|(_, l)| *l == level) {
                            if let Some(p) = d.placements.get(*i as usize * INSTANCE_SIZE..(*i as usize + 1) * INSTANCE_SIZE) {
                                instances.extend_from_slice(p);
                            }
                        }
                    }
                    None if e.level == 0 => instances.extend_from_slice(d.placements),
                    None => {}
                }
            }
            let count = u32::try_from(instances.len() / INSTANCE_SIZE).unwrap_or(u32::MAX).saturating_sub(first);
            for word in [e.count.saturating_mul(VERTICES), count, e.first.saturating_mul(VERTICES), first] {
                args.extend_from_slice(&word.to_le_bytes());
            }
            self.segments += u64::from(e.count) * u64::from(count);
        }
        (instances, args)
    }

    /// Write the placements in view and each entry's arguments ([`Edges::contents`]).
    pub(super) fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, parts: &[Part<'_>]) {
        let (instances, args) = self.contents(parts);
        if let Some(buffer) = &self.args {
            queue.write_buffer(buffer, 0, &args);
        }
        if instances.is_empty() {
            return;
        }
        let need = instances.len() as u64;
        if self.instances.as_ref().is_none_or(|(_, size)| *size < need) {
            let size = need.saturating_mul(3) / 2;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sc_edge_instances"),
                size,
                usage: wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            self.instances = Some((buffer, size));
        }
        if let Some((buffer, _)) = &self.instances {
            queue.write_buffer(buffer, 0, &instances);
        }
    }

    /// Draw every entry with `pipeline` (the edges, or the hidden edges), in one call.
    pub(super) fn draw(&self, pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline, tally: &Tally) {
        if self.draw_uncounted(pass, pipeline) {
            tally.segments(u32::try_from(self.segments).unwrap_or(u32::MAX));
        }
    }

    /// Draw the edges again without counting them (their depth, [`Pipelines::depth`]); `false`
    /// when there are none.
    pub(super) fn draw_uncounted(&self, pass: &mut wgpu::RenderPass<'_>, pipeline: &wgpu::RenderPipeline) -> bool {
        let (Some((instances, _)), Some(args)) = (&self.instances, &self.args) else { return false };
        if self.segments == 0 {
            return false;
        }
        pass.set_pipeline(pipeline);
        pass.set_bind_group(3, &self.arena.bind, &[]);
        pass.set_vertex_buffer(0, instances.slice(..));
        pass.multi_draw_indirect(args, 0, u32::try_from(self.entries.len()).unwrap_or(u32::MAX));
        true
    }
}

/// The instanced edge pipelines.
pub(super) struct Pipelines {
    /// The model's edges, and its hidden edges (where the model hides them).
    pub visible: wgpu::RenderPipeline,
    pub hidden: wgpu::RenderPipeline,
    /// Lit edges in the instance's first style (halos) and second (the lines over them), and
    /// lit hidden edges in the first.
    pub lit_a: wgpu::RenderPipeline,
    pub lit_b: wgpu::RenderPipeline,
    pub lit_hidden: wgpu::RenderPipeline,
    /// The edges' depth only, written before the see-through faces as the blocks' lines' is
    /// (`line_depth`).
    pub depth: wgpu::RenderPipeline,
}

/// The edge level each placement in view is drawn from: the coarsest whose deviation stays under
/// `px` pixels ([`super::EDGE_PX`], or [`super::EDGE_PX_MOVING`] while the view moves) at `scale`, the
/// most pixels a millimetre of the placement covers where it is drawn
/// ([`placement_stretch`](super::placement_stretch)).
pub(super) fn level_for(e: &SharedEdges, scale: f32, px: f32) -> u8 {
    if e.levels.len() < 2 {
        return 0;
    }
    let level = e.levels.iter().rposition(|(_, tolerance)| *tolerance * scale <= px).unwrap_or(0);
    u8::try_from(level).unwrap_or(0)
}
