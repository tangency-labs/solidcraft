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
const VIEW_SIZE: usize = 208;
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
                wgpu::BindGroupLayoutEntry {
                    binding: 5,
                    visibility: wgpu::ShaderStages::COMPUTE,
                    ty: wgpu::BindingType::Texture {
                        sample_type: wgpu::TextureSampleType::Float { filterable: false },
                        view_dimension: wgpu::TextureViewDimension::D2,
                        multisampled: false,
                    },
                    count: None,
                },
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

/// First indirect triangle entry index for each `(draw, level)`, in O(D * L) time.
fn tri_entry_table(draw_levels: &[usize], tri_entries: &[(usize, usize, u32, u32, i32)]) -> Vec<Vec<Option<usize>>> {
    let mut table: Vec<Vec<Option<usize>>> = draw_levels.iter().map(|&len| vec![None; len]).collect();
    for (entry, &(draw, level, ..)) in tri_entries.iter().enumerate() {
        if let Some(slot) = table.get_mut(draw).and_then(|levels| levels.get_mut(level)) {
            slot.get_or_insert(entry);
        }
    }
    table
}

/// Indirect edge `(entry, level)` pairs grouped by edge part in ascending entry order, in
/// O(D + E) time.
fn edge_entry_table(part_count: usize, edge_entries: &[(usize, usize, u32, u32)]) -> Vec<Vec<(usize, usize)>> {
    let mut table: Vec<Vec<(usize, usize)>> = vec![Vec::new(); part_count];
    for (n, &(part, level, ..)) in edge_entries.iter().enumerate() {
        if let Some(entries) = table.get_mut(part) {
            entries.push((n, level));
        }
    }
    table
}

/// One scene's instanced placements, culled on the GPU.
#[derive(Clone)]
pub(super) struct GpuCull {
    kernel: Arc<Kernel>,
    bind: wgpu::BindGroup,
    /// The scene, arguments, placements and picks buffers, to bind again with another depth
    /// pyramid.
    buffers: [wgpu::Buffer; 4],
    /// Bound in place of a depth pyramid while there is none.
    placeholder: wgpu::TextureView,
    /// The pyramid bound ([`super::hiz::Hiz::generation`]; 0: the placeholder).
    bound_hiz: u64,
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
    culled_for: Option<([[f32; 4]; 4], [f32; 2], bool, Option<u32>)>,
    /// The device's workgroups per dispatch dimension.
    max_groups: u32,
    readback: wgpu::Buffer,
    state: Arc<AtomicU8>,
    /// The last cull's statistics are not copied yet: the readback was busy.
    copy_pending: bool,
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
        let draw_levels: Vec<usize> = draws.iter().map(|d| d.levels.len()).collect();
        let tri_table = tri_entry_table(&draw_levels, &tri_entries);
        let edge_part_count = if edges.is_some() { draws.iter().filter(|d| d.edges.is_some()).count() } else { 0 };
        let edge_table = edge_entry_table(edge_part_count, &edge_entries);
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
                let entry = tri_table[i][k]?;
                levels.extend([l.error.to_bits(), u32::try_from(entry).ok()?, 0, 0]);
            }
            parts.extend([first, u32::try_from(d.levels.len()).ok()?, u32::from(here), 0]);
            let first = u32::try_from(levels.len() / LEVEL_WORDS).ok()?;
            let mut count = 0u32;
            if let (Some(e), true) = (&d.edges, edges.is_some()) {
                if let Some(entries) = edge_table.get(edge_part) {
                    for &(n, level) in entries {
                        let tolerance = e.levels.get(level).map_or(f32::INFINITY, |(_, t)| *t);
                        levels.extend([tolerance.to_bits(), u32::try_from(n).ok()?, 0, 0]);
                        count += 1;
                    }
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
        let placeholder = device
            .create_texture(&wgpu::TextureDescriptor {
                label: Some("sc_cull_no_hiz"),
                size: wgpu::Extent3d { width: 1, height: 1, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::R32Float,
                usage: wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            })
            .create_view(&Default::default());
        let buffers = [scene_buffer.clone(), args_buffer.clone(), out.clone(), picks.clone()];
        let bind = Self::bind_with(device, &kernel, &view, &buffers, &placeholder);
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
            buffers,
            placeholder,
            bound_hiz: 0,
            culled_for: None,
            max_groups: limits.max_compute_workgroups_per_dimension.max(1),
            readback,
            state: Arc::new(AtomicU8::new(IDLE)),
            copy_pending: false,
        })
    }

    /// The bind group over `view`, the buffers `b` and the depth pyramid `hiz`.
    fn bind_with(device: &wgpu::Device, kernel: &Kernel, view: &wgpu::Buffer, b: &[wgpu::Buffer; 4], hiz: &wgpu::TextureView) -> wgpu::BindGroup {
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("sc_cull"),
            layout: &kernel.layout,
            entries: &[
                wgpu::BindGroupEntry { binding: 0, resource: view.as_entire_binding() },
                wgpu::BindGroupEntry { binding: 1, resource: b[0].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 2, resource: b[1].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 3, resource: b[2].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 4, resource: b[3].as_entire_binding() },
                wgpu::BindGroupEntry { binding: 5, resource: wgpu::BindingResource::TextureView(hiz) },
            ],
        })
    }

    /// The pipelines, for the next scene's cull.
    pub(super) fn kernel(&self) -> Arc<Kernel> {
        self.kernel.clone()
    }

    /// Whether a depth pyramid is bound ([`GpuCull::run`] with `hidden`): a frame drawn straight
    /// into the window's pass binds the placeholder instead.
    #[cfg(test)]
    pub(super) fn pyramid_bound(&self) -> bool {
        self.bound_hiz != 0
    }

    /// Whether the triangles of the scene's instanced draw `draw` are culled here (else on the
    /// CPU: see-through, drawn back to front).
    pub(super) fn culls(&self, draw: &InstancedDraw) -> bool {
        draw.look == InstanceLook::Opaque
    }

    /// Cull for the view `vp` of `size` pixels, in `encoder` (before the frame's passes):
    /// again only when the view changed or `cpu_changed` (the see-through placements the CPU
    /// culled, in `draws`' `shown`). With `hidden` (the last frame's depth pyramid, and whether
    /// the depth buffer is 1 at the near plane), also leaves out the placements it hides. Takes
    /// the statistics of an earlier cull that have come back into `indirect` and `edges`.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn run(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        vp: &[[f32; 4]; 4],
        size: [f32; 2],
        hidden: Option<(&super::hiz::Hiz, bool)>,
        cpu_changed: bool,
        draws: &[InstancedDraw],
        indirect: &mut indirect::Indirect,
        edges: Option<&mut edges::Edges>,
        hover: Option<u32>,
    ) {
        self.collect(indirect, edges);
        // Against a pyramid, each frame (it changes with every frame drawn); without one, again
        // only for another view, or after a cull that left out hidden placements.
        let key = (*vp, size, hidden.is_some(), hover);
        if !cpu_changed && hidden.is_none() && self.culled_for == Some(key) {
            // A cull whose statistics could not be copied while the readback was busy: copy them
            // now that it is free (a cull below would overwrite its arguments).
            if self.copy_pending {
                self.copy_stats(encoder);
            }
            return;
        }
        self.culled_for = Some(key);
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
        let generation = hidden.map_or(0, |(h, _)| h.generation);
        if generation != self.bound_hiz {
            self.bind = Self::bind_with(device, &self.kernel, &self.view, &self.buffers, hidden.map_or(&self.placeholder, |(h, _)| &h.view));
            self.bound_hiz = generation;
        }
        // The view the pyramid was drawn with, its frame, levels and flags.
        let pyramid = hidden.and_then(|(h, reversed)| h.vp.map(|vp| (h, vp, reversed)));
        for column in pyramid.map_or(*vp, |(_, vp, _)| vp) {
            view.extend(column.iter().flat_map(|x| x.to_le_bytes()));
        }
        let words = pyramid.map_or([1, 1, 1, 0], |(h, _, reversed)| [h.frame[0], h.frame[1], h.levels, 1 | if reversed { 2 } else { 0 }]);
        view.extend(words.iter().flat_map(|w| w.to_le_bytes()));
        // x: the whole-body hovered placement's index in the scene, y: 1 when there is one.
        let hover_words = hover.map_or([0; 4], |index| [index, 1, 0, 0]);
        view.extend(hover_words.iter().flat_map(|w| w.to_le_bytes()));
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
        self.copy_stats(encoder);
    }

    /// Copy the arguments for the statistics now, or remember to when the last readback frees the
    /// buffer (a cull's arguments change only with the next cull).
    fn copy_stats(&mut self, encoder: &mut wgpu::CommandEncoder) {
        if self.state.load(Ordering::Acquire) == IDLE {
            encoder.copy_buffer_to_buffer(&self.args, 0, &self.readback, 0, self.args_reset.len() as u64);
            self.state.store(COPIED, Ordering::Release);
            self.copy_pending = false;
        } else {
            self.copy_pending = true;
        }
    }

    /// Move the statistics' readback along without a cull ([`GpuCull::collect`]), then copy the
    /// last cull's arguments if it could not when they were made ([`GpuCull::copy_stats`]): a
    /// frame that reuses the picture still returns the counts its last cull drew.
    pub(super) fn advance_stats(&mut self, encoder: &mut wgpu::CommandEncoder, indirect: &mut indirect::Indirect, edges: Option<&mut edges::Edges>) {
        self.collect(indirect, edges);
        if self.copy_pending {
            self.copy_stats(encoder);
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
        // The margin has a CPU mirror with tests ([`hidden_by_depth`]): the two cannot drift.
        assert!(super::SHADER.contains("near > far + max(shift, 1e-6)"), "the cull's margin");
        assert!(super::SHADER.contains("var shift = 0.0;"), "the placement's own depth change");
    }

    /// The rule `hidden()` ends with, on the CPU: a placement is left out only when it lies
    /// farther than everything the pyramid holds under it by more than its own depth change since
    /// that frame (`shift`), and never by less than the depth buffer's own resolution. Here so
    /// the rule has a test: the shader itself cannot be run on a CPU, and the GPU tests are CI's.
    fn hidden_by_depth(near: f32, far: f32, shift: f32) -> bool {
        near > far + shift.max(1e-6)
    }

    /// A placement's own pixels are part of the pyramid, so its own depth change must never be
    /// enough to leave it out: that was the popping (near geometry, strongest while orbiting).
    #[test]
    fn the_margin_covers_a_placements_own_depth_change() {
        // It gained 0.02 of depth as the camera receded; the pyramid's farthest under it is its
        // own back, 0.02 nearer: it is not behind itself.
        assert!(!hidden_by_depth(0.30, 0.28, 0.02));
        // Well behind what the last frame drew there, by far more than it moved: left out.
        assert!(hidden_by_depth(0.80, 0.20, 0.01));
        // Within the depth buffer's own resolution: the same surface, not one behind another.
        assert!(!hidden_by_depth(0.500_000_5, 0.5, 0.0));
    }

    /// The O(D * L) triangle and edge lookup tables built in `GpuCull::new` match the previous
    /// linear `.position(...)` and `.enumerate().filter(...)` lookups identically across arbitrary
    /// entry orderings, missing-edge draws, and varying level counts.
    #[test]
    fn entry_tables_match_linear_lookup() {
        use super::{edge_entry_table, tri_entry_table};
        let draw_levels = [3usize, 1, 4, 2, 3];
        // Shuffled order as `Indirect::new` groups entries by (look, flat, closed).
        let tri_entries: Vec<(usize, usize, u32, u32, i32)> = vec![
            (2, 0, 12, 0, 0),
            (0, 1, 6, 12, 4),
            (4, 2, 3, 18, 8),
            (1, 0, 9, 21, 12),
            (0, 0, 18, 30, 16),
            (2, 3, 3, 48, 20),
            (3, 1, 6, 51, 24),
            (4, 0, 15, 57, 28),
            (0, 2, 3, 72, 32),
            (2, 1, 9, 75, 36),
            (3, 0, 12, 84, 40),
            (4, 1, 9, 96, 44),
            (2, 2, 6, 105, 48),
            // Duplicate (draw, level) must resolve to the first entry just like `.position(...)`.
            (0, 1, 6, 111, 52),
        ];
        let tri_table = tri_entry_table(&draw_levels, &tri_entries);
        for (i, &len) in draw_levels.iter().enumerate() {
            assert_eq!(tri_table[i].len(), len);
            for (k, &got) in tri_table[i].iter().enumerate() {
                let linear = tri_entries.iter().position(|e| e.0 == i && e.1 == k);
                assert_eq!(got, linear, "draw {i}, level {k}");
            }
        }

        let part_count = 4usize;
        let edge_entries = vec![(2, 0, 0, 10), (0, 0, 10, 8), (0, 1, 18, 4), (2, 1, 22, 5), (2, 2, 27, 2), (3, 0, 29, 12)];
        let edge_table = edge_entry_table(part_count, &edge_entries);
        assert_eq!(edge_table.len(), part_count);
        for (part, got) in edge_table.iter().enumerate() {
            let linear: Vec<(usize, usize)> = edge_entries.iter().enumerate().filter(|(_, x)| x.0 == part).map(|(n, x)| (n, x.1)).collect();
            assert_eq!(*got, linear, "edge part {part}");
        }
    }

    /// The GPU culls a scene's placements as the CPU does: every level of every part draws the
    /// same placements (in any order), triangles and edges alike, for a view with placements out
    /// of it on every side, behind the eye, and near and far enough for each level. Both mark a
    /// whole-body hover's one placement alike.
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
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX, None, None);
        cpu.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX, None, None);
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
        // A whole-body hover flags its one placement's palette word in what each cull writes,
        // on the GPU and on the CPU alike, and the flag goes when the hover does.
        let row = cpu.instanced[0].shown.as_ref().and_then(|s| s.first()).map(|(i, _)| *i).expect("a placement in view");
        let rows: Arc<std::collections::HashMap<String, (u32, u32)>> = Arc::new([("Hovered".to_owned(), (0, row))].into_iter().collect());
        let flagged = |b: &super::super::Batches| -> Vec<u32> {
            let out = read(&b.indirect.as_ref().unwrap().instances().unwrap());
            (0..out.len() / INSTANCE_SIZE)
                .map(|k| word(&out, k * INSTANCE_SIZE / 4 + 12))
                .filter(|w| w & crate::view::HOVER_FLAG != 0)
                .map(|w| w & !crate::view::HOVER_FLAG)
                .collect()
        };
        for b in [&mut gpu, &mut cpu] {
            b.instance_rows = rows.clone();
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            b.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX, None, Some("Hovered"));
            queue.submit([encoder.finish()]);
        }
        // Placement `row` of the first part is in palette entry `row` (see `scene`).
        assert_eq!(flagged(&gpu), vec![row], "the GPU cull flags the hovered placement alone");
        // The CPU writes its placements from the draws' own (its buffer is not read back here).
        let cpu_flagged = |b: &super::super::Batches| -> Vec<(usize, usize)> {
            b.instanced
                .iter()
                .enumerate()
                .flat_map(|(d, x)| x.placements.as_chunks::<INSTANCE_SIZE>().0.iter().enumerate().map(move |(k, p)| (d, k, p[51])))
                .filter(|(_, _, high)| high & 0x80 != 0)
                .map(|(d, k, _)| (d, k))
                .collect()
        };
        assert_eq!(cpu_flagged(&cpu), vec![(0, row as usize)], "the CPU cull flags the hovered placement alone");
        assert!(cpu.instanced[0].shown.as_ref().is_some_and(|s| s.iter().any(|(i, _)| *i == row)), "and writes it");
        assert!(cpu_flagged(&gpu).is_empty(), "the GPU's flag is in its output only");
        for b in [&mut gpu, &mut cpu] {
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            b.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX, None, None);
            queue.submit([encoder.finish()]);
        }
        assert!(flagged(&gpu).is_empty() && cpu_flagged(&cpu).is_empty(), "no flag once nothing is hovered");
        // The cull reads the placements where the scene holds them, not where a motion puts one:
        // a frame that moves a body is culled on the CPU, and the next that moves nothing on the
        // GPU again, from the buffers the cull took back.
        let shift = [[1.0, 0.0, 0.0, 30.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &Motions::new([(1, shift)]), crate::view::EDGE_PX, None, None);
        assert!(gpu.gpu_cull.is_none(), "a frame with motions is culled on the CPU");
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &motions, crate::view::EDGE_PX, None, None);
        assert!(gpu.gpu_cull.is_some(), "a frame without motions is culled on the GPU again");
        queue.submit([encoder.finish()]);
    }

    /// Cull `batches` with the GPU cull for the view `vp` of `size` pixels, with or without the
    /// last frame's depth pyramid, and read back how many placements it kept and the words of the
    /// first placement it kept.
    fn culled(
        batches: &mut super::super::Batches,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        vp: &[[f32; 4]; 4],
        size: [f32; 2],
        hidden: Option<(&super::super::hiz::Hiz, bool)>,
    ) -> (u32, Vec<u8>) {
        use super::super::INSTANCE_SIZE;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        batches.cull(device, queue, &mut encoder, vp, size, &super::super::Motions::default(), crate::view::EDGE_PX, hidden, None);
        queue.submit([encoder.finish()]);
        let Some(g) = batches.gpu_cull.as_ref() else { panic!("the GPU culls the scene") };
        let out = batches.indirect.as_ref().and_then(|i| i.instances()).expect("placements");
        let staging = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: g.args.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let placed = device.create_buffer(&wgpu::BufferDescriptor {
            label: None,
            size: out.size(),
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        encoder.copy_buffer_to_buffer(&g.args, 0, &staging, 0, g.args.size());
        encoder.copy_buffer_to_buffer(&out, 0, &placed, 0, out.size());
        queue.submit([encoder.finish()]);
        staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        placed.slice(..).map_async(wgpu::MapMode::Read, |_| {});
        device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
        let bytes = staging.slice(..).get_mapped_range().unwrap().to_vec();
        let outs = placed.slice(..).get_mapped_range().unwrap().to_vec();
        let word = |b: &[u8], i: usize| u32::from_le_bytes(b[i * 4..i * 4 + 4].try_into().unwrap());
        let entries = batches.indirect.as_ref().unwrap().entry_list().len();
        let mut total = 0u32;
        let mut survivor = Vec::new();
        for n in 0..entries {
            let count = word(&bytes, n * 5 + 1);
            total += count;
            if count == 1 {
                let first = word(&bytes, n * 5 + 4) as usize;
                survivor = outs[first * INSTANCE_SIZE..(first + 1) * INSTANCE_SIZE].to_vec();
            }
        }
        (total, survivor)
    }

    /// A pipeline drawing a whole viewport at depth 0.9, writing no colour: a depth a placement
    /// behind can be tested against.
    fn plane_pipeline(device: &wgpu::Device) -> wgpu::RenderPipeline {
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: None,
            source: wgpu::ShaderSource::Wgsl(
                r#"
@vertex
fn vs(@builtin(vertex_index) i: u32) -> @builtin(position) vec4<f32> {
    let x = f32((i << 1u) & 2u);
    let y = f32(i & 2u);
    return vec4<f32>(x * 2.0 - 1.0, 1.0 - y * 2.0, 0.0, 1.0);
}
@fragment
fn fs() -> @builtin(frag_depth) f32 {
    return 0.9;
}
"#
                .into(),
            ),
        });
        device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
            label: Some("sc_test_plane"),
            layout: None,
            vertex: wgpu::VertexState { module: &module, entry_point: Some("vs"), buffers: &[], compilation_options: Default::default() },
            fragment: Some(wgpu::FragmentState { module: &module, entry_point: Some("fs"), targets: &[], compilation_options: Default::default() }),
            primitive: wgpu::PrimitiveState::default(),
            depth_stencil: Some(wgpu::DepthStencilState {
                format: wgpu::TextureFormat::Depth32Float,
                depth_write_enabled: Some(true),
                depth_compare: Some(wgpu::CompareFunction::Always),
                stencil: wgpu::StencilState::default(),
                bias: wgpu::DepthBiasState::default(),
            }),
            multisample: wgpu::MultisampleState::default(),
            multiview_mask: None,
            cache: None,
        })
    }

    /// While the view moves, a placement lying wholly behind the depth the last frame drew is
    /// left out, and one in front of it is drawn; without the last frame's depth both are.
    #[test]
    fn a_placement_behind_the_last_frames_depth_is_left_out_while_the_view_moves() {
        cull_against_the_last_frames_depth(false);
    }

    /// The same on a reversed depth buffer — a `Depth32Float` one, which is what the app draws
    /// into on Metal, so `depth_format` picks `Depth32FloatStencil8` there. The pyramid stores
    /// the view the frame was drawn with (the turned one) and the cull converts its depths back
    /// with the same flag. Stored unturned, it compared `1 - z` with `z`: every placement in the
    /// near half of the depth range was left out, and popped in and out as the pyramid changed.
    ///
    /// GPU, CI only: the GPU tests hang on the machine this was written on.
    #[test]
    fn a_reversed_depth_pyramid_hides_the_same_placement() {
        cull_against_the_last_frames_depth(true);
    }

    /// [`a_placement_behind_the_last_frames_depth_is_left_out_while_the_view_moves`] with
    /// `reversed` saying which way the depth buffer runs.
    fn cull_against_the_last_frames_depth(reversed: bool) {
        use super::super::{
            Batches, GpuScene, GpuTarget, InstanceLook, InstancedMesh, PaletteIndex, Renderer, SceneSlot, SharedTris, gpu_view_proj_turned, hiz,
        };
        use solidcore_record::Vec3;
        use std::sync::{Arc, Mutex};
        let Ok((_, device, queue)) = crate::open_headless(crate::test_env::policy()) else { return };
        if !super::supported(&device) {
            return;
        }
        let Some(kernel) = hiz::Kernel::new(&device, 1) else { return };
        let target = GpuTarget { format: wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false, edges: false };
        let r = Renderer::new(&device, target, None);
        let mesh = solidcore_tess::Mesh {
            positions: vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]],
            tri_face: vec![0; 4],
            ..Default::default()
        };
        // One part placed in front of a plane 100 mm from the eye (z from -30 to -20) and behind
        // it (z from -210 to -200), both in the middle of the view.
        let mut part = InstancedMesh {
            key: 1,
            tris: Arc::new(SharedTris::new(&mesh)),
            instances: Vec::new(),
            look: InstanceLook::Opaque,
            levels: Vec::new(),
            closed: true,
            edges: None,
        };
        for z in [-30.0, -210.0] {
            part.place([[1.0, 0.0, 0.0, -5.0], [0.0, 1.0, 0.0, -5.0], [0.0, 0.0, 1.0, z]], PaletteIndex(0));
        }
        let mut batches = Batches::default();
        let slot: SceneSlot = Arc::new(Mutex::new(Some(GpuScene { instanced: vec![part], ..GpuScene::default() })));
        batches.take(&device, &queue, &r.uniform, &r.uniform_bgl, 1, &slot, None, false).unwrap();
        // Perspective down -z from the origin: a point at depth z (< -1) is at 1 + 1 / z.
        let vp = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, -1.0], [0.0, 0.0, -1.0, 0.0]];
        let size = [64.0, 48.0];
        // The last frame's depth: the plane at z = -100 everywhere.
        let depth = device.create_texture(&wgpu::TextureDescriptor {
            label: None,
            size: wgpu::Extent3d { width: 64, height: 48, depth_or_array_layers: 1 },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Depth32Float,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
            view_formats: &[],
        });
        let depth_view = depth.create_view(&Default::default());
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        drop(encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: None,
            color_attachments: &[],
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: &depth_view,
                depth_ops: Some(wgpu::Operations {
                    // The plane at z = -100 is at 1 - 1/100 in the plain convention; a reversed
                    // buffer holds it the other way round.
                    load: wgpu::LoadOp::Clear(if reversed { 1.0 / 100.0 } else { 1.0 - 1.0 / 100.0 }),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            timestamp_writes: None,
            occlusion_query_set: None,
            multiview_mask: None,
        }));
        let mut pyramid = hiz::Hiz::new(&device, &kernel, &depth_view, [64, 48], 1);
        // The renderer stores the view the frame was drawn with; the cull is given the unturned
        // one. Building here with the matrix it culls with is what hid the bug from this test.
        let drawn_with = gpu_view_proj_turned(vp, None, reversed);
        pyramid.build(&queue, &kernel, &mut encoder, drawn_with, reversed, 1);
        queue.submit([encoder.finish()]);
        assert_eq!(culled(&mut batches, &device, &queue, &vp, size, None).0, 2, "a still view draws both");
        let (count, survivor) = culled(&mut batches, &device, &queue, &vp, size, Some((&pyramid, reversed)));
        assert_eq!(count, 1, "moving: the one behind the plane is left out");
        // The near placement's rows, [row][column]: the part at z -30, the one in front of the plane.
        let near: Vec<u8> =
            [[1.0f32, 0.0, 0.0, -5.0], [0.0, 1.0, 0.0, -5.0], [0.0, 0.0, 1.0, -30.0]].iter().flatten().flat_map(|v| v.to_le_bytes()).collect();
        assert_eq!(&survivor[..near.len()], &near[..], "the near placement survives, not the rear one");
        assert_eq!(culled(&mut batches, &device, &queue, &vp, size, None).0, 2, "still again: both");
    }

    /// An odd frame keeps its last row and column in the depth pyramid: a placement whose box
    /// reaches the frame's edge, where the depth the last frame drew is clear, is drawn, though
    /// the rest of the box lies behind that depth. A level that halved the level-0 texels without
    /// rounding them up would have dropped the edge texels, and the placement with them.
    #[test]
    fn a_placement_reaching_an_odd_frames_clear_edge_is_not_left_out() {
        use super::super::{Batches, GpuScene, GpuTarget, InstanceLook, InstancedMesh, PaletteIndex, Renderer, SceneSlot, SharedTris, hiz};
        use solidcore_record::Vec3;
        use std::sync::{Arc, Mutex};
        let Ok((_, device, queue)) = crate::open_headless(crate::test_env::policy()) else { return };
        if !super::supported(&device) {
            return;
        }
        let Some(kernel) = hiz::Kernel::new(&device, 1) else { return };
        let target = GpuTarget { format: wgpu::TextureFormat::Rgba8Unorm, depth: None, samples: 1, tess: false, oit: false, edges: false };
        let r = Renderer::new(&device, target, None);
        let mesh = solidcore_tess::Mesh {
            positions: vec![Vec3::ZERO, Vec3::new(10.0, 0.0, 0.0), Vec3::new(0.0, 10.0, 0.0), Vec3::new(0.0, 0.0, 10.0)],
            normals: vec![Vec3::Z; 4],
            triangles: vec![[0, 1, 2], [0, 1, 3], [0, 2, 3], [1, 2, 3]],
            tri_face: vec![0; 4],
            ..Default::default()
        };
        // 9 × 5 pixels: level 0 is 8 × 4 texels, its last columns and rows padding that repeats
        // the frame's edge. The plane, at depth 0.9 (10 mm from the eye), covers all but the last
        // column and row; a part behind it reaches one of them, and its box's rest is drawn over.
        let size = [9.0, 5.0];
        let vp = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, -1.0], [0.0, 0.0, -1.0, 0.0]];
        let plane = plane_pipeline(&device);
        let cases = [
            // Across pixels 5 to 8.7: 16.3 mm of x per millimetre of the part's 0 to 10 box,
            // from x = 23.3 mm. The plane covers x 0 to 8 and y 0 to 4, so the last column is clear.
            ("the clear last column", [8.0, 4.0], [[16.3, 0.0, 0.0, 23.3], [0.0, 1.0, 0.0, -5.0], [0.0, 0.0, 1.0, -210.0]]),
            // Across rows 3 to 4.1: y' = 8.6 y - 126 for the part's 0 to 10 box. The plane covers
            // every column and y 0 to 4, so the last row is clear.
            ("the clear last row", [9.0, 4.0], [[14.0, 0.0, 0.0, -70.0], [0.0, 8.6, 0.0, -126.0], [0.0, 0.0, 1.0, -210.0]]),
        ];
        for (name, [covered_w, covered_h], rows) in cases {
            let depth = device.create_texture(&wgpu::TextureDescriptor {
                label: None,
                size: wgpu::Extent3d { width: 9, height: 5, depth_or_array_layers: 1 },
                mip_level_count: 1,
                sample_count: 1,
                dimension: wgpu::TextureDimension::D2,
                format: wgpu::TextureFormat::Depth32Float,
                usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
                view_formats: &[],
            });
            let depth_view = depth.create_view(&Default::default());
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            {
                let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: None,
                    color_attachments: &[],
                    depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                        view: &depth_view,
                        depth_ops: Some(wgpu::Operations { load: wgpu::LoadOp::Clear(1.0), store: wgpu::StoreOp::Store }),
                        stencil_ops: None,
                    }),
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                });
                pass.set_viewport(0.0, 0.0, covered_w, covered_h, 0.0, 1.0);
                pass.set_pipeline(&plane);
                pass.draw(0..3, 0..1);
            }
            let mut part = InstancedMesh {
                key: 1,
                tris: Arc::new(SharedTris::new(&mesh)),
                instances: Vec::new(),
                look: InstanceLook::Opaque,
                levels: Vec::new(),
                closed: true,
                edges: None,
            };
            part.place(rows, PaletteIndex(0));
            let mut batches = Batches::default();
            let slot: SceneSlot = Arc::new(Mutex::new(Some(GpuScene { instanced: vec![part], ..GpuScene::default() })));
            batches.take(&device, &queue, &r.uniform, &r.uniform_bgl, 1, &slot, None, false).unwrap();
            let mut pyramid = hiz::Hiz::new(&device, &kernel, &depth_view, [9, 5], 1);
            pyramid.build(&queue, &kernel, &mut encoder, vp, false, 1);
            queue.submit([encoder.finish()]);
            assert_eq!(culled(&mut batches, &device, &queue, &vp, size, None).0, 1, "{name}: a still view draws it");
            assert_eq!(culled(&mut batches, &device, &queue, &vp, size, Some((&pyramid, false))).0, 1, "{name} keeps the placement drawn");
        }
    }

    /// Falling back from GPU culling to CPU culling for a frame with motions keeps the triangles'
    /// indirect draw arguments separate from the edges' draw arguments: `Edges::write` must not
    /// overwrite `Indirect`'s shared GPU-cull argument buffer at offset 0.
    #[test]
    fn cpu_fallback_during_motion_keeps_triangle_and_edge_args_separate() {
        use super::super::{
            Batches, GpuScene, GpuTarget, InstanceLook, InstancedMesh, Motions, PaletteIndex, Renderer, SceneSlot, SharedEdges, SharedTris,
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
        let scene = || {
            let mut parts = Vec::new();
            for key in [1u64, 2u64] {
                let mut m = InstancedMesh {
                    key,
                    tris: Arc::new(SharedTris::new(&mesh)),
                    instances: Vec::new(),
                    look: InstanceLook::Opaque,
                    levels: Vec::new(),
                    closed: true,
                    edges: Some(Arc::new(SharedEdges::new(&mesh, |_| true, Vec::new()))),
                };
                let rows = [[1.0, 0.0, 0.0, key as f32 * 20.0 - 30.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, -60.0]];
                m.place_edged(rows, PaletteIndex(key as u32), PaletteIndex(1), PaletteIndex(2));
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
        let vp = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, -1.0, -1.0], [0.0, 0.0, -1.0, 0.0]];
        let size = [800.0, 600.0];
        let rest = Motions::default();
        // Frame 1: at rest, GPU cull takes over both `indirect` and `edges` with a shared args buffer.
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &rest, crate::view::EDGE_PX, None, None);
        cpu.cull(&device, &queue, &mut encoder, &vp, size, &rest, crate::view::EDGE_PX, None, None);
        queue.submit([encoder.finish()]);
        assert!(gpu.gpu_cull.is_some(), "GPU cull takes over at rest");
        // Frame 2: dragging a joint brings motions, dropping GPU cull and falling back to CPU culling.
        let shift = [[1.0, 0.0, 0.0, 5.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
        let moving = Motions::new([(1, shift)]);
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &moving, crate::view::EDGE_PX, None, None);
        cpu.cull(&device, &queue, &mut encoder, &vp, size, &moving, crate::view::EDGE_PX, None, None);
        queue.submit([encoder.finish()]);
        assert!(gpu.gpu_cull.is_none(), "motion frame falls back to CPU culling");
        let read = |buffer: &wgpu::Buffer, len: u64| -> Vec<u8> {
            let staging = device.create_buffer(&wgpu::BufferDescriptor {
                label: None,
                size: len,
                usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            });
            let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
            encoder.copy_buffer_to_buffer(buffer, 0, &staging, 0, len);
            queue.submit([encoder.finish()]);
            staging.slice(..).map_async(wgpu::MapMode::Read, |_| {});
            device.poll(wgpu::PollType::wait_indefinitely()).unwrap();
            staging.slice(..).get_mapped_range().unwrap().to_vec()
        };
        let tri_bytes = (gpu.indirect.as_ref().unwrap().entry_list().len() * super::TRI_ARGS_WORDS * 4) as u64;
        let edge_bytes = (gpu.edges.as_ref().unwrap().entry_list().len() * super::EDGE_ARGS_WORDS * 4) as u64;
        let gpu_tri = read(&gpu.indirect.as_ref().unwrap().args().unwrap(), tri_bytes);
        let cpu_tri = read(&cpu.indirect.as_ref().unwrap().args().unwrap(), tri_bytes);
        let gpu_edge = read(&gpu.edges.as_ref().unwrap().args().unwrap(), edge_bytes);
        let cpu_edge = read(&cpu.edges.as_ref().unwrap().args().unwrap(), edge_bytes);
        assert_eq!(gpu_tri, cpu_tri, "triangle indirect args must match CPU cull without being overwritten by edges");
        assert_eq!(gpu_edge, cpu_edge, "edge indirect args must match CPU cull");
        // If either argument buffer was refused on an earlier frame, the next frame retries even
        // when the shown placements did not change.
        gpu.indirect.as_mut().unwrap().clear_args();
        gpu.edges.as_mut().unwrap().clear_args();
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor::default());
        gpu.cull(&device, &queue, &mut encoder, &vp, size, &moving, crate::view::EDGE_PX, None, None);
        queue.submit([encoder.finish()]);
        let retried_tri = read(&gpu.indirect.as_ref().unwrap().args().unwrap(), tri_bytes);
        let retried_edge = read(&gpu.edges.as_ref().unwrap().args().unwrap(), edge_bytes);
        assert_eq!(retried_tri, cpu_tri, "triangle indirect args are retried when missing");
        assert_eq!(retried_edge, cpu_edge, "edge indirect args are retried when missing");
    }
}
