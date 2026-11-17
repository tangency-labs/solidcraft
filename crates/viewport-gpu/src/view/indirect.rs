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
/// meshes held so their addresses stay theirs).
struct Arena {
    vertices: wgpu::Buffer,
    indices: wgpu::Buffer,
    spans: HashMap<usize, (Arc<SharedTris>, Span)>,
}

impl Arena {
    /// `meshes` in one vertex and one index buffer; `None` when they do not fit.
    fn new(device: &wgpu::Device, meshes: &[Arc<SharedTris>]) -> Option<Arena> {
        let (mut vertices, mut indices) = (Vec::new(), Vec::new());
        let mut spans = HashMap::with_capacity(meshes.len());
        for m in meshes {
            let key = Arc::as_ptr(m) as usize;
            if spans.contains_key(&key) {
                continue;
            }
            let span = Span {
                first_index: u32::try_from(indices.len() / 4).ok()?,
                count: u32::try_from(m.indices.len()).ok()?,
                base_vertex: i32::try_from(vertices.len() / super::SHARED_SIZE).ok()?,
            };
            vertices.extend_from_slice(&m.vertices);
            indices.extend(m.indices.iter().flat_map(|i| i.to_le_bytes()));
            spans.insert(key, (m.clone(), span));
        }
        let limit = device.limits().max_buffer_size;
        if vertices.is_empty() || vertices.len() as u64 > limit || indices.len() as u64 > limit {
            return None;
        }
        Some(Arena {
            vertices: super::buffer_init(device, "sc_indirect_vertices", &vertices, wgpu::BufferUsages::VERTEX)?,
            indices: super::buffer_init(device, "sc_indirect_indices", &indices, wgpu::BufferUsages::INDEX)?,
            spans,
        })
    }

    fn span(&self, mesh: &Arc<SharedTris>) -> Option<Span> {
        self.spans.get(&(Arc::as_ptr(mesh) as usize)).filter(|(m, _)| Arc::ptr_eq(m, mesh)).map(|(_, s)| *s)
    }
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
}

impl Indirect {
    /// The scene's instanced `draws` drawn indirectly, taking `before`'s arena when it holds
    /// every mesh they draw (a scene rebuilt for a colour or visibility change uploads none).
    /// `None` when none can be.
    pub(super) fn new(device: &wgpu::Device, draws: &[InstancedDraw], before: Option<&Indirect>) -> Option<Indirect> {
        let mut batched = vec![false; draws.len()];
        let mut meshes = Vec::new();
        for (i, d) in draws.iter().enumerate() {
            if d.levels.iter().all(|l| matches!(*l.tris, super::SharedGpu::Indexed { .. })) && !d.levels.is_empty() {
                batched[i] = true;
                meshes.extend(d.levels.iter().map(|l| l.src.clone()));
            }
        }
        if meshes.is_empty() {
            return None;
        }
        let reused = before.map(|b| b.arena.clone()).filter(|a| meshes.iter().all(|m| a.span(m).is_some()));
        let arena = match reused {
            Some(arena) => arena,
            None => Arc::new(Arena::new(device, &meshes)?),
        };
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
        let mut out = Indirect { arena, entries, groups, args: None, instances: None, triangles: Vec::new(), batched };
        let (instances, args) = out.contents(draws);
        out.args = Some(super::buffer_init(device, "sc_indirect_args", &args, wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST)?);
        if !instances.is_empty() {
            let usage = wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST;
            out.instances = Some((super::buffer_init(device, "sc_indirect_instances", &instances, usage)?, instances.len() as u64));
        }
        Some(out)
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

    /// Write the placements in view and each entry's arguments ([`Indirect::contents`]).
    pub(super) fn write(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, draws: &[InstancedDraw]) {
        let (instances, args) = self.contents(draws);
        if let Some(buffer) = &self.args {
            queue.write_buffer(buffer, 0, &args);
        }
        if instances.is_empty() {
            return;
        }
        let need = instances.len() as u64;
        if self.instances.as_ref().is_none_or(|(_, size)| *size < need) {
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
            if *triangles == 0 {
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
