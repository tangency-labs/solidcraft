//! Instanced meshes drawn by indirect draws: every shared mesh and level of detail in one vertex
//! and one index buffer (the arena), every placement in view in one instance buffer, and one
//! draw's arguments per mesh and level in one indirect buffer, ordered so that each look a pass
//! draws (with each pipeline: smooth or flat triangles, closed or open bodies) is one run of them.
//! A pass then draws a look's placements with one `multi_draw_indexed_indirect`, not one call per
//! mesh and level: a large assembly placed many times took thousands of calls a frame.
//!
//! Only on a device whose indirect draws take a first instance
//! ([`wgpu::Features::INDIRECT_FIRST_INSTANCE`]); elsewhere, and for a mesh too large for one
//! buffer, each mesh and level is drawn on its own as before ([`super::InstancedDraw::draw`]).

use std::collections::HashMap;
use std::sync::Arc;

use super::{INSTANCE_SIZE, InstanceLook, InstancedDraw, SharedTris, Tally};

/// Bytes of one draw's arguments ([`wgpu::util::DrawIndexedIndirectArgs`]).
const ARGS_SIZE: u64 = 20;

/// Whether `device` draws instanced meshes indirectly.
pub(super) fn supported(device: &wgpu::Device) -> bool {
    device.features().contains(wgpu::Features::INDIRECT_FIRST_INSTANCE)
}

/// Where a mesh's triangles lie in the arena.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Span {
    first_index: u32,
    count: u32,
    base_vertex: i32,
}

/// One indirect entry: level `level` of instanced draw `draw`, at `span` in the arena.
#[derive(Clone, Copy, Debug)]
struct Entry {
    draw: usize,
    level: usize,
    span: Span,
}

/// The entries one pass draws with one pipeline: `len` of them from `first`.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Group {
    look: InstanceLook,
    flat: bool,
    closed: bool,
    first: u32,
    len: u32,
}

/// The arena: the meshes' vertices and indices, and where each mesh is (by its address, the
/// meshes held so their addresses stay theirs). Its buffers have room past what they hold: a
/// mesh a later scene adds (a part opened, a level of detail made) is written into the room,
/// and the meshes already there are not uploaded again. Every arena made over the same buffers
/// takes room from one shared mark, so two never write to the same bytes.
pub(super) struct Arena {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    /// The buffers' sizes (bytes: vertices, indices).
    room: [u64; 2],
    /// Bytes of each buffer taken by any arena over them (the rest is free).
    taken: Arc<std::sync::Mutex<[u64; 2]>>,
    spans: HashMap<usize, (Arc<SharedTris>, Span)>,
}

/// Bytes of a mesh in the arena: vertices, indices.
fn mesh_bytes(m: &SharedTris) -> [u64; 2] {
    [m.vertices.len() as u64, m.indices.len() as u64 * 4]
}

impl Arena {
    /// An arena holding `meshes` (each once; a mesh with no triangles, or one too large for a
    /// buffer, is left to be drawn on its own), with the bytes it uploaded. `before`'s buffers
    /// are kept when the meshes it lacks fit in their room and the meshes it held but no longer
    /// draws take no more than the meshes drawn (they are let go when the buffers are made
    /// again). New buffers have room for half as many again. Meshes beyond what one buffer
    /// holds are left out (drawn on their own). `None` when there is nothing to hold or the
    /// device refused the buffers.
    pub(super) fn holding(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        meshes: &[Arc<SharedTris>],
        before: Option<&Arc<Arena>>,
    ) -> Option<(Arc<Arena>, u64)> {
        let limit = device.limits().max_buffer_size;
        let mut seen = std::collections::HashSet::with_capacity(meshes.len());
        let mut wanted: Vec<&Arc<SharedTris>> = Vec::with_capacity(meshes.len());
        let mut need = [0u64; 2];
        for m in meshes {
            let [v, i] = mesh_bytes(m);
            if v == 0 || i == 0 || !seen.insert(Arc::as_ptr(m) as usize) {
                continue;
            }
            if need[0] + v > limit || need[1] + i > limit {
                continue;
            }
            need = [need[0] + v, need[1] + i];
            wanted.push(m);
        }
        if wanted.is_empty() {
            return None;
        }
        if let Some(before) = before {
            let lacking: Vec<&Arc<SharedTris>> = wanted.iter().copied().filter(|m| before.span(m).is_none()).collect();
            if lacking.is_empty() && before.spans.len() == wanted.len() {
                return Some((before.clone(), 0));
            }
            if let Some(grown) = before.with(queue, &wanted, &lacking, need) {
                return Some(grown);
            }
        }
        Arena::new(device, &wanted, need, limit)
    }

    /// `before`'s buffers holding `wanted`, the `lacking` ones written into their room; `None`
    /// when they do not fit there or the buffers would hold more no longer drawn than drawn.
    fn with(&self, queue: &wgpu::Queue, wanted: &[&Arc<SharedTris>], lacking: &[&Arc<SharedTris>], need: [u64; 2]) -> Option<(Arc<Arena>, u64)> {
        let add = lacking.iter().fold([0u64; 2], |a, m| {
            let [v, i] = mesh_bytes(m);
            [a[0] + v, a[1] + i]
        });
        let mut taken = self.taken.lock().ok()?;
        let at = *taken;
        let after = [at[0] + add[0], at[1] + add[1]];
        if after[0] > self.room[0] || after[1] > self.room[1] || after[0] > 2 * need[0] || after[1] > 2 * need[1] {
            return None;
        }
        let first_index = u32::try_from(at[1] / 4).ok()?;
        let base_vertex = i32::try_from(at[0] / super::SHARED_SIZE as u64).ok()?;
        let (vertices, indices, placed) = pack(lacking, first_index, base_vertex)?;
        if !vertices.is_empty() {
            queue.write_buffer(&self.vertices, at[0], &vertices);
            queue.write_buffer(&self.indices, at[1], &indices);
        }
        *taken = after;
        drop(taken);
        let mut spans = HashMap::with_capacity(wanted.len());
        for m in wanted {
            let key = Arc::as_ptr(m) as usize;
            if let Some(span) = self.span(m).or_else(|| placed.get(&key).copied()) {
                spans.insert(key, (Arc::clone(m), span));
            }
        }
        let arena = Arena { vertices: self.vertices.clone(), indices: self.indices.clone(), room: self.room, taken: self.taken.clone(), spans };
        Some((Arc::new(arena), add[0] + add[1]))
    }

    /// New buffers holding `wanted` (`need` bytes), with room for half as many again (within
    /// `limit`).
    fn new(device: &wgpu::Device, wanted: &[&Arc<SharedTris>], need: [u64; 2], limit: u64) -> Option<(Arc<Arena>, u64)> {
        let (mut vertices, mut indices, placed) = pack(wanted, 0, 0)?;
        let room = need.map(|n| (n.saturating_add(n / 2)).min(limit).max(n).next_multiple_of(wgpu::COPY_BUFFER_ALIGNMENT));
        vertices.resize(usize::try_from(room[0]).ok()?, 0);
        indices.resize(usize::try_from(room[1]).ok()?, 0);
        let spans =
            wanted.iter().filter_map(|m| placed.get(&(Arc::as_ptr(m) as usize)).map(|s| (Arc::as_ptr(m) as usize, (Arc::clone(m), *s)))).collect();
        let arena = Arena {
            vertices: super::buffer_init(device, "sc_indirect_vertices", &vertices, wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST)?,
            indices: super::buffer_init(device, "sc_indirect_indices", &indices, wgpu::BufferUsages::INDEX | wgpu::BufferUsages::COPY_DST)?,
            room,
            taken: Arc::new(std::sync::Mutex::new(need)),
            spans,
        };
        Some((Arc::new(arena), need[0] + need[1]))
    }

    fn span(&self, mesh: &Arc<SharedTris>) -> Option<Span> {
        self.spans.get(&(Arc::as_ptr(mesh) as usize)).filter(|(m, _)| Arc::ptr_eq(m, mesh)).map(|(_, s)| *s)
    }

    /// Where `mesh` is drawn from, when the arena holds it: its buffers, and its span.
    pub(super) fn place(&self, mesh: &Arc<SharedTris>) -> Option<super::SharedGpu> {
        let span = self.span(mesh)?;
        Some(super::SharedGpu::InArena {
            vertices: self.vertices.clone(),
            indices: self.indices.clone(),
            first_index: span.first_index,
            count: span.count,
            base_vertex: span.base_vertex,
        })
    }
}

/// `meshes` one after another (vertices, indices) and each one's span, the first starting at
/// `first_index` and `base_vertex`.
fn pack(meshes: &[&Arc<SharedTris>], first_index: u32, base_vertex: i32) -> Option<(Vec<u8>, Vec<u8>, HashMap<usize, Span>)> {
    let (mut vertices, mut indices) = (Vec::new(), Vec::new());
    let mut spans = HashMap::with_capacity(meshes.len());
    for m in meshes {
        let span = Span {
            first_index: first_index.checked_add(u32::try_from(indices.len() / 4).ok()?)?,
            count: u32::try_from(m.indices.len()).ok()?,
            base_vertex: base_vertex.checked_add(i32::try_from(vertices.len() / super::SHARED_SIZE).ok()?)?,
        };
        vertices.extend_from_slice(&m.vertices);
        indices.extend(m.indices.iter().flat_map(|i| i.to_le_bytes()));
        spans.insert(Arc::as_ptr(m) as usize, span);
    }
    Some((vertices, indices, spans))
}

/// The instanced draws of a scene drawn indirectly.
#[derive(Clone)]
pub(super) struct Indirect {
    arena: Arc<Arena>,
    entries: Vec<Entry>,
    groups: Vec<Group>,
    args: Option<wgpu::Buffer>,
    instances: Option<(wgpu::Buffer, u64)>,
    /// Each group's triangles drawn by the last [`Indirect::write`] (the draw statistics).
    triangles: Vec<u64>,
    /// Which of the scene's instanced draws are drawn here (the others on their own).
    batched: Vec<bool>,
    /// The GPU culls the placements ([`super::gpu_cull`]): `args` and `instances` are its, and
    /// `triangles` are what it last reported, so a group is drawn even when they say none.
    on_gpu: bool,
}

impl Indirect {
    /// The scene's instanced `draws` drawn indirectly from `arena`: those whose every level it
    /// holds ([`Arena::holding`]). `None` when none can be.
    pub(super) fn new(device: &wgpu::Device, draws: &[InstancedDraw], arena: Arc<Arena>) -> Option<Indirect> {
        let mut batched = vec![false; draws.len()];
        for (i, d) in draws.iter().enumerate() {
            batched[i] = !d.levels.is_empty() && d.levels.iter().all(|l| arena.span(&l.src).is_some());
        }
        if !batched.contains(&true) {
            return None;
        }
        // Entries grouped by what a pass draws them with; within a group, in the scene's order
        // (see-through placements are sorted back to front within each mesh, as before).
        let key = |d: &InstancedDraw, flat: bool| (look_order(d.look), flat, d.closed);
        let mut entries = Vec::new();
        for (i, d) in draws.iter().enumerate().filter(|(i, _)| batched[*i]) {
            for (k, l) in d.levels.iter().enumerate() {
                entries.push((key(d, l.flat), Entry { draw: i, level: k, span: arena.span(&l.src)? }));
            }
        }
        entries.sort_by_key(|(k, _)| *k);
        let mut groups: Vec<Group> = Vec::new();
        for (n, ((_, flat, closed), e)) in entries.iter().enumerate() {
            let look = draws.get(e.draw)?.look;
            match groups.last_mut() {
                Some(g) if g.look == look && g.flat == *flat && g.closed == *closed => g.len += 1,
                _ => groups.push(Group { look, flat: *flat, closed: *closed, first: u32::try_from(n).ok()?, len: 1 }),
            }
        }
        let entries: Vec<Entry> = entries.into_iter().map(|(_, e)| e).collect();
        // Until the first cull (and in a scene never culled: a moving layer, the product
        // preview), every placement is drawn in full, as each mesh's own draws were.
        let mut out = Indirect { arena, entries, groups, args: None, instances: None, triangles: Vec::new(), batched, on_gpu: false };
        let (instances, args) = out.contents(draws);
        out.args = Some(super::buffer_init(device, "sc_indirect_args", &args, Self::args_usage())?);
        if !instances.is_empty() {
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
            out.instances = Some((super::buffer_init(device, "sc_indirect_instances", &instances, usage)?, instances.len() as u64));
        }
        Some(out)
    }

    /// Each entry: its draw, level, index count, first index and base vertex, in the order of the
    /// arguments (grouped by look, opaque first).
    pub(super) fn entry_list(&self) -> Vec<(usize, usize, u32, u32, i32)> {
        self.entries.iter().map(|e| (e.draw, e.level, e.span.count, e.span.first_index, e.span.base_vertex)).collect()
    }

    /// Draw from the arguments (from 0) and placements a GPU cull fills ([`super::gpu_cull`]).
    pub(super) fn cull_on_gpu(&mut self, args: wgpu::Buffer, instances: wgpu::Buffer, size: u64) {
        self.args = Some(args);
        self.instances = Some((instances, size));
        self.on_gpu = true;
    }

    /// Whether a GPU cull last filled its arguments and placements, rather than the CPU.
    pub(super) fn culls_on_gpu(&self) -> bool {
        self.on_gpu
    }

    fn args_usage() -> wgpu::BufferUsages {
        let base = wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST;
        if cfg!(test) { base | wgpu::BufferUsages::COPY_SRC } else { base }
    }

    /// Whether the draw-argument buffer is missing (for example after a refused allocation) and
    /// needs another [`Self::write`] even when the shown placements did not change.
    pub(super) fn args_missing(&self) -> bool {
        self.args.is_none()
    }

    /// The buffer the draw arguments are drawn from (for tests).
    #[cfg(test)]
    pub(super) fn args(&self) -> Option<wgpu::Buffer> {
        self.args.clone()
    }

    /// Drop the argument buffer to simulate a refused allocation (for tests).
    #[cfg(test)]
    pub(super) fn clear_args(&mut self) {
        self.args = None;
    }

    /// The buffer the placements are drawn from.
    pub(super) fn instances(&self) -> Option<wgpu::Buffer> {
        self.instances.as_ref().map(|(b, _)| b.clone())
    }

    /// The placements each entry drew (a GPU cull's, read back): each group's triangles.
    pub(super) fn counted(&mut self, counts: &[u32]) {
        self.triangles = vec![0; self.groups.len()];
        for (n, (e, count)) in self.entries.iter().zip(counts).enumerate() {
            let n = u32::try_from(n).unwrap_or(u32::MAX);
            if let Some(g) = self.groups.iter().position(|g| (g.first..g.first + g.len).contains(&n))
                && let Some(t) = self.triangles.get_mut(g)
            {
                *t += u64::from(e.span.count / 3) * u64::from(*count);
            }
        }
    }

    /// Whether the scene's instanced draw `i` is drawn here.
    pub(super) fn batches(&self, i: usize) -> bool {
        self.batched.get(i).copied().unwrap_or(false)
    }

    /// The placements in view (`draws`' `shown`, each with its level; every placement in full
    /// before the first cull) and each entry's arguments: an entry draws its level's placements
    /// from where they start in the one instance buffer. Counts each group's triangles.
    fn contents(&mut self, draws: &[InstancedDraw]) -> (Vec<u8>, Vec<u8>) {
        let mut instances: Vec<u8> = Vec::new();
        let mut args: Vec<u8> = Vec::with_capacity(self.entries.len() * ARGS_SIZE as usize);
        self.triangles = vec![0; self.groups.len()];
        for (n, e) in self.entries.iter().enumerate() {
            let first = u32::try_from(instances.len() / INSTANCE_SIZE).unwrap_or(u32::MAX);
            if let Some(d) = draws.get(e.draw) {
                match &d.shown {
                    Some(shown) => {
                        let level = u8::try_from(e.level).unwrap_or(u8::MAX);
                        for (i, _) in shown.iter().filter(|(_, l)| *l == level) {
                            if let Some(p) = d.placements.get(*i as usize * INSTANCE_SIZE..(*i as usize + 1) * INSTANCE_SIZE) {
                                instances.extend_from_slice(p);
                            }
                        }
                    }
                    None if e.level == 0 => instances.extend_from_slice(&d.placements),
                    None => {}
                }
            }
            let count = u32::try_from(instances.len() / INSTANCE_SIZE).unwrap_or(u32::MAX).saturating_sub(first);
            for word in [e.span.count, count, e.span.first_index] {
                args.extend_from_slice(&word.to_le_bytes());
            }
            args.extend_from_slice(&e.span.base_vertex.to_le_bytes());
            args.extend_from_slice(&first.to_le_bytes());
            let n = u32::try_from(n).unwrap_or(u32::MAX);
            if let Some(g) = self.groups.iter().position(|g| (g.first..g.first + g.len).contains(&n))
                && let Some(t) = self.triangles.get_mut(g)
            {
                *t += u64::from(e.span.count / 3) * u64::from(count);
            }
        }
        (instances, args)
    }

    /// Write the placements in view and each entry's arguments ([`Indirect::contents`]): the CPU
    /// culled this frame, so it draws from these again (`on_gpu` off).
    pub(super) fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draws: &[InstancedDraw]) {
        let was_on_gpu = self.on_gpu;
        self.on_gpu = false;
        let (instances, args) = self.contents(draws);
        if was_on_gpu || self.args.is_none() {
            // Leaving a GPU cull (or retrying after a refused allocation): allocate a private
            // argument buffer instead of writing into the shared GPU-cull buffer.
            self.args = crate::buffer_init(device, "sc_indirect_args", &args, Self::args_usage()).ok();
        } else if let Some(buffer) = &self.args {
            queue.write_buffer(buffer, 0, &args);
        }
        if instances.is_empty() {
            if was_on_gpu {
                self.instances = None;
            }
            return;
        }
        let need = instances.len() as u64;
        if was_on_gpu || self.instances.as_ref().is_none_or(|(_, size)| *size < need) {
            // Room to grow, so a camera move that brings a few more placements into view does
            // not make a buffer each frame.
            let size = need.saturating_mul(3) / 2;
            let buffer = device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("sc_indirect_instances"),
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

    /// Draw the placements of look `look` (only closed bodies' when `closed_only`), one call per
    /// pipeline: `smooth` or `flat` as the triangles are lit.
    pub(super) fn draw(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        look: InstanceLook,
        closed_only: bool,
        smooth: &wgpu::RenderPipeline,
        flat: &wgpu::RenderPipeline,
        tally: &Tally,
    ) {
        let (Some((instances, _)), Some(args)) = (&self.instances, &self.args) else { return };
        let mut bound = false;
        for (g, triangles) in self.groups.iter().zip(&self.triangles).filter(|(g, _)| g.look == look && (g.closed || !closed_only)) {
            if *triangles == 0 && !self.on_gpu {
                continue;
            }
            if !bound {
                pass.set_vertex_buffer(0, self.arena.vertices.slice(..));
                pass.set_vertex_buffer(1, instances.slice(..));
                pass.set_index_buffer(self.arena.indices.slice(..), wgpu::IndexFormat::Uint32);
                bound = true;
            }
            pass.set_pipeline(if g.flat { flat } else { smooth });
            pass.multi_draw_indexed_indirect(args, u64::from(g.first) * ARGS_SIZE, g.len);
            tally.call();
            tally.add_triangles(*triangles);
        }
    }
}

/// The order looks are grouped in.
fn look_order(look: InstanceLook) -> u8 {
    match look {
        InstanceLook::Opaque => 0,
        InstanceLook::Pbr => 1,
        InstanceLook::Faded => 2,
        InstanceLook::Glass => 3,
    }
}
