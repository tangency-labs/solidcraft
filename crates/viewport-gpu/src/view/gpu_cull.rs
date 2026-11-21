//! The instanced placements in view, and the level of detail each is drawn from, chosen on the
//! GPU each frame (`cull.wgsl`) in place of [`super::Batches::cull`]'s walk over every placement:
//! a large assembly has tens of thousands, and the walk, and writing what it chose to the
//! instance buffers, took tens of milliseconds of the window thread a frame.
//!
//! The placements of every instanced draw are uploaded once per scene. Each frame a compute pass
//! picks, for every placement in view, its triangles' and its edges' level (as
//! [`super::PlacedBox`] does on the CPU), counts each level's placements in its indirect draw
//! arguments, turns the counts into where each level's placements start, and copies the
//! placements there. The indirect draws ([`indirect`], [`edges`]) then draw from those buffers.
//!
//! See-through looks are drawn back to front, so their placements are still culled and sorted
//! on the CPU and written at the end of the triangles' half of the buffer. The draw statistics
//! (triangles and segments drawn) come back from the GPU a frame or two later. A frame that moves
//! bodies is culled on the CPU too: the shader reads the placements the scene holds, not where
//! their motions put them, and the cull is made again for the next frame that moves nothing.
//!
//! Only on a device that draws indirectly and binds four storage buffers to a compute shader
//! (not WebGL2); elsewhere the CPU culls as before.

use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use super::{EDGE_PX, INSTANCE_SIZE, InstanceLook, InstancedDraw, LOD_PX, edges, indirect};

const SHADER: &str = include_str!("cull.wgsl");
/// Words of a placement ([`INSTANCE_SIZE`] bytes).
const PLACEMENT_WORDS: usize = INSTANCE_SIZE / 4;
/// Words of a placement as the shader reads it: the placement, then its part.
const SCENE_PLACEMENT_WORDS: usize = PLACEMENT_WORDS + 1;
const PART_WORDS: usize = 16;
const LEVEL_WORDS: usize = 4;
/// Words of one draw's arguments: indexed triangles, and edges.
const TRI_ARGS_WORDS: usize = 5;
const EDGE_ARGS_WORDS: usize = 4;
/// Bytes of the shader's `View` uniform.
const VIEW_SIZE: usize = 112;
/// Placements a workgroup of `pick` and `place` takes.
const GROUP: u32 = 64;

/// The cull's pipelines on one device, made once and kept from scene to scene.
pub(super) struct Kernel {
    layout: wgpu::BindGroupLayout,
    pick: wgpu::ComputePipeline,
    scan: wgpu::ComputePipeline,
    place: wgpu::ComputePipeline,
}

/// Whether `device` culls instanced placements on the GPU.
pub(super) fn supported(device: &wgpu::Device) -> bool {
    let limits = device.limits();
    indirect::supported(device)
        && limits.max_storage_buffers_per_shader_stage >= 4
        && limits.max_compute_invocations_per_workgroup >= 256
        && limits.max_compute_workgroup_size_x >= 256
}

impl Kernel {
    /// The pipelines on `device`; `None` (logged) where it cannot build them, and the CPU culls.
    pub(super) fn new(device: &wgpu::Device) -> Option<Kernel> {
        if !supported(device) {
            return None;
        }
        let scopes = [wgpu::ErrorFilter::OutOfMemory, wgpu::ErrorFilter::Internal, wgpu::ErrorFilter::Validation].map(|f| device.push_error_scope(f));
        let module =
            device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("sc_cull"), source: wgpu::ShaderSource::Wgsl(SHADER.into()) });
        let entry = |binding: u32, ty: wgpu::BufferBindingType| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer { ty, has_dynamic_offset: false, min_binding_size: None },
            count: None,
        };
        let storage = |read_only| wgpu::BufferBindingType::Storage { read_only };
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("sc_cull"),
            entries: &[
                entry(0, wgpu::BufferBindingType::Uniform),
                entry(1, storage(true)),
                entry(2, storage(false)),
                entry(3, storage(false)),
                entry(4, storage(false)),
            ],
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("sc_cull"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point: &str| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some("sc_cull"),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: Default::default(),
                cache: None,
            })
        };
        let kernel = Kernel { pick: pipeline("pick"), scan: pipeline("scan"), place: pipeline("place"), layout };
        let mut failed = None;
        for scope in scopes.into_iter().rev() {
            if let Some(e) = crate::popped(scope.pop()) {
                failed.get_or_insert(e.to_string());
            }
        }
        if let Some(e) = failed {
            log::warn!("instanced placements are culled on the CPU: {e}");
            return None;
        }
        Some(kernel)
    }
}

/// Where the draw statistics are on their way back: nothing asked, copied in a frame not yet
/// submitted, being mapped, mapped.
const IDLE: u8 = 0;
const COPIED: u8 = 1;
const MAPPING: u8 = 2;
const MAPPED: u8 = 3;

/// One scene's instanced placements, culled on the GPU.
#[derive(Clone)]
pub(super) struct GpuCull {
    kernel: Arc<Kernel>,
    bind: wgpu::BindGroup,
    view: wgpu::Buffer,
    args: wgpu::Buffer,
    /// The arguments as each frame starts: every count the GPU makes at 0. The see-through
    /// entries' counts and starts are filled in on the CPU.
    args_reset: Vec<u8>,
    /// The triangle entries culled on the CPU: (entry, draw, level), in entry order.
    cpu_entries: Vec<(usize, usize, usize)>,
    /// Where (placements) the see-through placements start in the triangles' half of the buffer.
    cpu_from: u32,
    /// Index counts of the triangle entries and segment counts of the edge entries (statistics).
    tri_indices: Vec<u32>,
    edge_segments: Vec<u32>,
    /// The shader's `View` after its matrix and size.
    words: [u32; 8],
    /// The view the placements were last culled for.
    culled_for: Option<([[f32; 4]; 4], [f32; 2])>,
    /// Whether the last cull's counts are still to be copied back (the readback was busy).
    counts_pending: bool,
    /// The device's workgroups per dispatch dimension.
    max_groups: u32,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
}

impl GpuCull {
    /// The cull of a scene's instanced `draws`, drawn by `indirect` (and their edges by `edges`),
    /// which from now on draw from the buffers it fills. `None` where the scene does not suit it
    /// (some draw is not drawn indirectly, or the buffers would be larger than the device binds):
    /// the CPU culls it.
    pub(super) fn new(
        device: &wgpu::Device,
        kernel: Arc<Kernel>,
        draws: &[InstancedDraw],
        indirect: &mut indirect::Indirect,
        edges: Option<&mut edges::Edges>,
    ) -> Option<GpuCull> {
        let n: usize = draws.iter().map(|d| d.placements.len() / INSTANCE_SIZE).sum();
        if n == 0 || (0..draws.len()).any(|i| !indirect.batches(i)) {
            return None;
        }
        let tri_entries = indirect.entry_list();
        // The opaque looks' entries come first ([`indirect`] groups them by look): they are
        // culled here, the rest on the CPU.
        let opaque = |draw: usize| draws.get(draw).is_some_and(|d| d.look == InstanceLook::Opaque);
        let gpu_tri = tri_entries.iter().take_while(|e| opaque(e.0)).count();
        if tri_entries.iter().skip(gpu_tri).any(|e| opaque(e.0)) {
            return None;
        }
        let edge_entries = edges.as_deref().map(|e| e.entry_list()).unwrap_or_default();
        // Each draw's part, levels and the entries that draw them.
        let mut parts: Vec<u32> = Vec::with_capacity(draws.len() * PART_WORDS);
        let mut levels: Vec<u32> = Vec::new();
        let mut edge_part = 0usize;
        for (i, d) in draws.iter().enumerate() {
            let [lo, hi] = d.bounds;
            let finite = lo.iter().chain(&hi).all(|x| x.is_finite());
            parts.extend(lo.map(f32::to_bits));
            parts.push(u32::from(!finite));
            parts.extend(hi.map(f32::to_bits));
            parts.push(0);
            let first = u32::try_from(levels.len() / LEVEL_WORDS).ok()?;
            let here = d.look == InstanceLook::Opaque;
            for (k, l) in d.levels.iter().enumerate() {
                let entry = tri_entries.iter().position(|e| e.0 == i && e.1 == k)?;
                levels.extend([l.error.to_bits(), u32::try_from(entry).ok()?, 0, 0]);
            }
            parts.extend([first, u32::try_from(d.levels.len()).ok()?, u32::from(here), 0]);
            let first = u32::try_from(levels.len() / LEVEL_WORDS).ok()?;
            let mut count = 0u32;
            if let (Some(e), true) = (&d.edges, edges.is_some()) {
                for (n, entry) in edge_entries.iter().enumerate().filter(|(_, x)| x.0 == edge_part) {
                    let tolerance = e.levels.get(entry.1).map_or(f32::INFINITY, |(_, t)| *t);
                    levels.extend([tolerance.to_bits(), u32::try_from(n).ok()?, 0, 0]);
                    count += 1;
                }
                edge_part += 1;
            }
            parts.extend([first, count, u32::from(count > 0), 0]);
        }
        let mut scene: Vec<u32> = Vec::with_capacity(n * SCENE_PLACEMENT_WORDS + parts.len() + levels.len());
        for (i, d) in draws.iter().enumerate() {
            for p in d.placements.as_chunks::<INSTANCE_SIZE>().0 {
                scene.extend(p.as_chunks::<4>().0.iter().map(|w| u32::from_le_bytes(*w)));
                scene.push(u32::try_from(i).ok()?);
            }
        }
        let parts_at = u32::try_from(scene.len()).ok()?;
        scene.extend(parts);
        let levels_at = u32::try_from(scene.len()).ok()?;
        scene.extend(levels);

        // The arguments: the triangles' (each entry's placements from its start, every count 0),
        // then the edges'.
        let mut args: Vec<u32> = Vec::with_capacity(tri_entries.len() * TRI_ARGS_WORDS + edge_entries.len() * EDGE_ARGS_WORDS);
        for (_, _, count, first_index, base_vertex) in &tri_entries {
            args.extend([*count, 0, *first_index, base_vertex.cast_unsigned(), 0]);
        }
        let edge_args_at = u32::try_from(args.len()).ok()?;
        for (_, _, first, count) in &edge_entries {
            args.extend([count.saturating_mul(edges::VERTICES), 0, first.saturating_mul(edges::VERTICES), 0]);
        }
        // The see-through placements go at the end of the triangles' half.
        let cpu_entries: Vec<(usize, usize, usize)> = tri_entries.iter().enumerate().skip(gpu_tri).map(|(e, x)| (e, x.0, x.1)).collect();
        // A draw's levels need not be adjacent (entries are grouped by flat shading too).
        let mut cpu_draws: Vec<usize> = cpu_entries.iter().map(|e| e.1).collect();
        cpu_draws.sort_unstable();
        cpu_draws.dedup();
        let cpu_placements: usize = cpu_draws.iter().filter_map(|i| draws.get(*i)).map(|d| d.placements.len() / INSTANCE_SIZE).sum();
        let cpu_from = u32::try_from(n - cpu_placements).ok()?;

        let bytes = |words: &[u32]| -> Vec<u8> { words.iter().flat_map(|w| w.to_le_bytes()).collect() };
        let half = u64::try_from(n * INSTANCE_SIZE).ok()?;
        let limits = device.limits();
        let largest = [scene.len() as u64 * 4, args.len() as u64 * 4, half * 2, n as u64 * 16];
        if largest.iter().any(|b| *b > limits.max_storage_buffer_binding_size || *b > limits.max_buffer_size) {
            return None;
        }
        let groups = u32::try_from(n).ok()?.div_ceil(GROUP);
        if groups > limits.max_compute_workgroups_per_dimension.saturating_mul(limits.max_compute_workgroups_per_dimension) {
            return None;
        }
        let storage = wgpu::BufferUsages::STORAGE;
        let scene_buffer = super::buffer_init(device, "sc_cull_scene", &bytes(&scene), storage)?;
        let args_reset = bytes(&args);
        // A buffer or bind group the device refuses leaves the scene to the CPU cull, its own
        // buffers untouched.
        let scopes = crate::error_scopes(device);
        let args_buffer = super::buffer_init(
            device,
            "sc_cull_args",
            &args_reset,
            storage | wgpu::BufferUsages::INDIRECT | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
        )?;
        let out = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_cull_placements"),
            size: half * 2,
            usage: storage | wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::COPY_DST | wgpu::BufferUsages::COPY_SRC,
            mapped_at_creation: false,
        });
        let picks = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_cull_picks"),
            size: n as u64 * 16,
            usage: storage,
            mapped_at_creation: false,
        });
        let view = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_cull_view"),
            size: VIEW_SIZE as u64,
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let readback = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("sc_cull_readback"),
            size: args_reset.len() as u64,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_cull"),
            layout: &kernel.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: scene_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: args_buffer.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: out.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: picks.as_entire_binding() },
            ],
        });
        if let Some(reason) = crate::caught(scopes) {
            log::warn!("instanced placements are culled on the CPU: {reason}");
            return None;
        }
        let edge_out_at = u32::try_from(n * PLACEMENT_WORDS).ok()?;
        indirect.cull_on_gpu(args_buffer.clone(), out.clone(), half);
        if let Some(e) = edges {
            e.cull_on_gpu(args_buffer.clone(), u64::from(edge_args_at) * 4, out.clone(), half);
        }
        let n = u32::try_from(n).ok()?;
        let words = [n, parts_at, levels_at, u32::try_from(gpu_tri).ok()?, u32::try_from(edge_entries.len()).ok()?, edge_args_at, edge_out_at, 0];
        Some(GpuCull {
            kernel,
            bind,
            view,
            args: args_buffer,
            args_reset,
            cpu_entries,
            cpu_from,
            tri_indices: tri_entries.iter().map(|e| e.2).collect(),
            edge_segments: edge_entries.iter().map(|e| e.3).collect(),
            words,
            culled_for: None,
            counts_pending: false,
            max_groups: limits.max_compute_workgroups_per_dimension.max(1),
            readback,
            state: Arc::new(AtomicU8::new(IDLE)),
        })
    }

    /// The pipelines, for the next scene's cull.
    pub(super) fn kernel(&self) -> Arc<Kernel> {
        self.kernel.clone()
    }

    /// Whether the triangles of the scene's instanced draw `draw` are culled here (else on the
    /// CPU: see-through, drawn back to front).
    pub(super) fn culls(&self, draw: &InstancedDraw) -> bool {
        draw.look == InstanceLook::Opaque
    }

    /// Cull for the view `vp` of `size` pixels, in `encoder` (before the frame's passes):
    /// again only when the view changed or `cpu_changed` (the see-through placements the CPU
    /// culled, in `draws`' `shown`). Takes the statistics of an earlier cull that have come
    /// back into `indirect` and `edges`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run(
        &mut self,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        vp: &[[f32; 4]; 4],
        size: [f32; 2],
        cpu_changed: bool,
        draws: &[InstancedDraw],
        indirect: &mut indirect::Indirect,
        edges: Option<&mut edges::Edges>,
    ) {
        self.collect(indirect, edges);
        if !cpu_changed && self.culled_for == Some((*vp, size)) {
            self.copy_counts(encoder);
            return;
        }
        self.culled_for = Some((*vp, size));
        // The see-through placements, as the CPU culled and sorted them, from `cpu_from`.
        let mut args = self.args_reset.clone();
        let mut placements: Vec<u8> = Vec::new();
        for (entry, draw, level) in &self.cpu_entries {
            let first = self.cpu_from.saturating_add(u32::try_from(placements.len() / INSTANCE_SIZE).unwrap_or(u32::MAX));
            if let Some(d) = draws.get(*draw)
                && let Some(shown) = &d.shown
            {
                let level = u8::try_from(*level).unwrap_or(u8::MAX);
                for (i, _) in shown.iter().filter(|(_, l)| *l == level) {
                    if let Some(p) = d.placements.get(*i as usize * INSTANCE_SIZE..(*i as usize + 1) * INSTANCE_SIZE) {
                        placements.extend_from_slice(p);
                    }
                }
            }
            let count = self.cpu_from.saturating_add(u32::try_from(placements.len() / INSTANCE_SIZE).unwrap_or(u32::MAX)).saturating_sub(first);
            let at = entry * TRI_ARGS_WORDS * 4;
            if let Some(words) = args.get_mut(at..at + TRI_ARGS_WORDS * 4) {
                words[4..8].copy_from_slice(&count.to_le_bytes());
                words[16..20].copy_from_slice(&first.to_le_bytes());
            }
        }
        queue.write_buffer(&self.args, 0, &args);
        if !placements.is_empty() {
            // `out` is the buffer indirect draws from; its triangles' half starts at 0.
            if let Some(out) = self.out_buffer(indirect) {
                queue.write_buffer(&out, u64::from(self.cpu_from) * INSTANCE_SIZE as u64, &placements);
            }
        }
        let mut view = Vec::with_capacity(VIEW_SIZE);
        for column in vp {
            view.extend(column.iter().flat_map(|x| x.to_le_bytes()));
        }
        for x in [size[0], size[1], LOD_PX, EDGE_PX] {
            view.extend(x.to_le_bytes());
        }
        view.extend(self.words.iter().flat_map(|w| w.to_le_bytes()));
        queue.write_buffer(&self.view, 0, &view);

        let n = self.words[0];
        let groups = n.div_ceil(GROUP);
        let x = groups.clamp(1, self.max_groups);
        let y = groups.div_ceil(x).max(1);
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("sc_cull"), timestamp_writes: None });
            pass.set_bind_group(0, &self.bind, &[]);
            pass.set_pipeline(&self.kernel.pick);
            pass.dispatch_workgroups(x, y, 1);
            pass.set_pipeline(&self.kernel.scan);
            pass.dispatch_workgroups(1, 1, 1);
            pass.set_pipeline(&self.kernel.place);
            pass.dispatch_workgroups(x, y, 1);
        }
        self.counts_pending = true;
        self.copy_counts(encoder);
    }

    /// Copy the last cull's counts back for the statistics, in `encoder`, once the readback is
    /// free: a cull while it was busy is copied in a later frame, even if the view stays put.
    fn copy_counts(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.counts_pending && self.state.load(Ordering::Acquire) == IDLE {
            encoder.copy_buffer_to_buffer(&self.args, 0, &self.readback, 0, self.args_reset.len() as u64);
            self.state.store(COPIED, Ordering::Release);
            self.counts_pending = false;
        }
    }

    /// The buffer the triangles are drawn from (the one this cull fills).
    fn out_buffer(&self, indirect: &indirect::Indirect) -> Option<wgpu::Buffer> {
        indirect.instances()
    }

    /// Move the statistics' readback along: map what an earlier frame copied (that frame has
    /// been submitted since), and read what is mapped into `indirect`'s and `edges`' counts.
    fn collect(&mut self, indirect: &mut indirect::Indirect, edges: Option<&mut edges::Edges>) {
        match self.state.load(Ordering::Acquire) {
            COPIED => {
                self.state.store(MAPPING, Ordering::Release);
                let state = self.state.clone();
                self.readback.slice(..).map_async(wgpu::MapMode::Read, move |r| {
                    state.store(if r.is_ok() { MAPPED } else { IDLE }, Ordering::Release);
                });
            }
            MAPPED => {
                let words: Vec<u32> = match self.readback.slice(..).get_mapped_range() {
                    Ok(range) => range.as_chunks::<4>().0.iter().map(|w| u32::from_le_bytes(*w)).collect(),
                    Err(_) => Vec::new(),
                };
                self.readback.unmap();
                self.state.store(IDLE, Ordering::Release);
                let tri: Vec<u32> = (0..self.tri_indices.len()).map(|e| words.get(e * TRI_ARGS_WORDS + 1).copied().unwrap_or(0)).collect();
                indirect.counted(&tri);
                let at = self.words[5] as usize;
                if let Some(edges) = edges {
                    let counts: Vec<u32> =
                        (0..self.edge_segments.len()).map(|e| words.get(at + e * EDGE_ARGS_WORDS + 1).copied().unwrap_or(0)).collect();
                    edges.counted(&counts);
                }
            }
            _ => {}
        }
    }
}

#[cfg(test)]
mod tests {
    /// The cull shader parses and validates (a bad shader would only fail at run time, and the
    /// CPU would quietly cull instead).
    #[test]
    fn the_cull_shader_is_valid() {
        let module = naga::front::wgsl::parse_str(super::SHADER).unwrap_or_else(|e| panic!("{}", e.emit_to_string(super::SHADER)));
        naga::valid::Validator::new(naga::valid::ValidationFlags::all(), naga::valid::Capabilities::empty()).validate(&module).unwrap();
        for entry in ["pick", "scan", "place"] {
            assert!(module.entry_points.iter().any(|e| e.name == entry), "{entry}");
        }
    }

    /// The GPU culls a scene's placements as the CPU does: every level of every part draws the
    /// same placements (in any order), triangles and edges alike, for a view with placements out
    /// of it on every side, behind the eye, and near and far enough for each level.
    #[test]
    fn the_gpu_culls_the_placements_the_cpu_would() {
        use super::super::{
            Batches, GpuScene, GpuTarget, INSTANCE_SIZE, InstanceLook, InstancedMesh, Motions, PaletteIndex, Renderer, SceneSlot, SharedEdges,
            SharedTris,
        };
        use solidcore_record::Vec3;
        use std::sync::{Arc, Mutex};
        let (_, device, queue) = match crate::open_headless(crate::test_env::policy()) {
            Ok(opened) => opened,
            Err(error) if crate::test_env::skip_unless_required(crate::test_env::required(), &error) => return,
            Err(error) => panic!("required GPU unavailable: {error}"),
        };
        if !super::supported(&device) {
            return;
        }
        let target = GpuTarget { format: wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false, edges: true };
        let r = Renderer::new(&device, target, None);
        let mesh = solidcore_tess::Mesh {
            positions: vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]],
            tri_face: vec![0; 4],
            edges: vec![vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0)], vec![Vec3::ZERO, Vec3::new(0.0, 0.0, 10.0)]],
            edge_faces: vec![vec![0], vec![0]],
            ..Default::default()
        };
        let coarse = vec![vec![[0.0, 0.0, 0.0], [10.0, 0.0, 0.0]]];
        let scene = || {
            let mut parts = Vec::new();
            for (key, look) in [(1, InstanceLook::Opaque), (2, InstanceLook::Glass)] {
                let mut m = InstancedMesh {
                    key,
                    tris: Arc::new(SharedTris::new(&mesh)),
                    instances: Vec::new(),
                    look,
                    levels: vec![(Arc::new(SharedTris::new(&mesh)), 0.05), (Arc::new(SharedTris::new(&mesh)), 1.0)],
                    closed: true,
                    edges: Some(Arc::new(SharedEdges::new(&mesh, |_| true, vec![(0.05, coarse.clone()), (1.0, coarse.clone())]))),
                };
                for i in 0..400u32 {
                    let (x, y) = ((i % 20) as f32 * 40.0 - 400.0, (i / 20) as f32 * 40.0 - 400.0);
                    let z = -20.0 - (i % 7) as f32 * 300.0 + if i % 31 == 0 { 400.0 } else { 0.0 };
                    let rows = [[1.0, 0.0, 0.0, x + key as f32], [0.0, 1.0, 0.0, y], [0.0, 0.0, 1.0, z]];
                    m.place_edged(rows, PaletteIndex(i), PaletteIndex(1), PaletteIndex(2));
                }
                parts.push(m);
            }
            GpuScene { instanced: parts, ..GpuScene::default() }
        };
        let take = |cpu: bool| {
            let mut b = Batches { cull_kernel_failed: cpu, ..Batches::default() };
            let slot: SceneSlot = Arc::new(Mutex::new(Some(scene())));
            b.take(&device, &queue, &r.uniform, &r.uniform_bgl, 1, &slot, None, false).unwrap();
            b
        };
        let (mut gpu, mut cpu) = (take(false), take(true));
        // Perspective down -z from the origin, 90 degrees across.
        let vp = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, -1.0], [0.0, 0.0, -1.0, 0.0]];
        let size = [800.0, 600.0];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        let motions = Motions::default();
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX);
        cpu.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX);
        queue.submit([encoder.finish()]);
        assert!(cpu.gpu_cull.is_none());
        let Some(g) = gpu.gpu_cull.as_ref() else { panic!("the GPU culls the scene") };
        let read = |buffer: &wgpu::Buffer| -> Vec<u8> {
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: buffer.size(),
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, buffer.size());
            queue.submit([encoder.finish()]);
            staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            staging.slice(..).get_mapped_range().unwrap().to_vec()
        };
        let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        let args = read(&g.args);
        let out = read(&gpu.indirect.as_ref().unwrap().instances().unwrap());
        let placements = |at: usize, first: u32, count: u32| -> Vec<Vec<u8>> {
            let mut v: Vec<Vec<u8>> =
                (first..first + count).map(|k| out[at + k as usize * INSTANCE_SIZE..at + (k as usize + 1) * INSTANCE_SIZE].to_vec()).collect();
            v.sort();
            v
        };
        let shown = |draw: usize, level: usize, edges: bool| -> Vec<Vec<u8>> {
            let d = &cpu.instanced[draw];
            let list = if edges { d.edges_shown.as_ref() } else { d.shown.as_ref() }.expect("culled on the CPU");
            let mut v: Vec<Vec<u8>> = list
                .iter()
                .filter(|(_, l)| usize::from(*l) == level)
                .map(|(i, _)| d.placements[*i as usize * INSTANCE_SIZE..(*i as usize + 1) * INSTANCE_SIZE].to_vec())
                .collect();
            v.sort();
            v
        };
        let mut levels_drawn = [0usize; 3];
        for (n, (draw, level, ..)) in gpu.indirect.as_ref().unwrap().entry_list().into_iter().enumerate() {
            let (count, first) = (word(&args, n * 5 + 1), word(&args, n * 5 + 4));
            let want = shown(draw, level, false);
            assert_eq!(placements(0, first, count), want, "the triangles of part {draw}, level {level}");
            levels_drawn[level] += usize::from(count > 0);
        }
        let edge_args_at = g.words[5] as usize;
        let half = out.len() / 2;
        for (n, (part, level, ..)) in gpu.edges.as_ref().unwrap().entry_list().into_iter().enumerate() {
            let (count, first) = (word(&args, edge_args_at + n * 4 + 1), word(&args, edge_args_at + n * 4 + 3));
            assert_eq!(placements(half, first, count), shown(part, level, true), "the edges of part {part}, level {level}");
            levels_drawn[level] += usize::from(count > 0);
        }
        assert!(levels_drawn.iter().all(|n| *n > 0), "every level drawn: {levels_drawn:?}");
        // The cull reads the placements where the scene holds them, not where a motion puts one:
        // a frame that moves a body is culled on the CPU, and the next that moves nothing on the
        // GPU again, from the buffers the cull took back.
        let shift = [[1.0, 0.0, 0.0, 30.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &Motions::new([(1, shift)]), crate::view::EDGE_PX);
        assert!(gpu.gpu_cull.is_none(), "a frame with motions is culled on the CPU");
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX);
        assert!(gpu.gpu_cull.is_some(), "a frame without motions is culled on the GPU again");
        queue.submit([encoder.finish()]);
    }
}
