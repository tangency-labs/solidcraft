//! Optional output filtering; never writes the Monte Carlo accumulation buffer.
use super::{Context, Result, SHADER, TIMEOUT, Task, checked, gpu, one_compile_at_a_time, placeholder_bind, scopes, warm_up};
use solidcraft_render::job::{CancelToken, DenoiseDiagnostics, DenoiseGuideClass, resolve_denoise_class};
use std::sync::atomic::{AtomicBool, Ordering};

pub(super) const GUIDE_BYTES_PER_PIXEL: u64 = 96; // two complete surface guides
pub(super) const BYTES_PER_PIXEL: u64 = GUIDE_BYTES_PER_PIXEL + 32; // two colour vec4s
const FILTER: &str = include_str!("denoise.wgsl");
pub(super) const GUIDES: &str = include_str!("denoise_guides.wgsl");
#[cfg(test)]
#[path = "denoise_tests.rs"]
mod tests;

// Shared by every denoised job on a device: the guide pipeline compiles the whole
// path shader, so it is built and warmed once rather than per job.
pub(super) struct Pipelines {
    guide: wgpu::ComputePipeline,
    filter: wgpu::ComputePipeline,
    classify: wgpu::ComputePipeline,
    filter_layout: wgpu::BindGroupLayout,
}

impl Pipelines {
    fn new(context: &Context) -> Result<Self> {
        let device = &context.device;
        let error_scopes = scopes(device);
        let guide_shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("denoise guides"),
            source: wgpu::ShaderSource::Wgsl(format!("{SHADER}\n{GUIDES}").into()),
        });
        let guide_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("denoise guides"),
            bind_group_layouts: &[Some(&context.layout)],
            immediate_size: 0,
        });
        let guide = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("denoise guides"),
            layout: Some(&guide_layout),
            module: &guide_shader,
            entry_point: Some("denoise_guides"),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &[("HAS_TEXTURES", 1.0)], ..Default::default() },
            cache: None,
        });
        let entries: Vec<_> = (0..4)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: binding != 3 } },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let filter_layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("denoise filter"), entries: &entries });
        let layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("denoise filter"),
            bind_group_layouts: &[Some(&filter_layout)],
            immediate_size: 0,
        });
        let shader = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("denoise filter"), source: wgpu::ShaderSource::Wgsl(FILTER.into()) });
        let filter = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("denoise filter"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("atrous"),
            compilation_options: Default::default(),
            cache: None,
        });
        let classify = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("denoise guide classes"),
            layout: Some(&layout),
            module: &shader,
            entry_point: Some("guide_classes"),
            compilation_options: Default::default(),
            cache: None,
        });
        checked(error_scopes)?;
        warm_up(device, &context.queue, &guide, &placeholder_bind(device, &context.layout, 7)?, &context.healthy)?;
        warm_up(device, &context.queue, &filter, &placeholder_bind(device, &filter_layout, 4)?, &context.healthy)?;
        warm_up(device, &context.queue, &classify, &placeholder_bind(device, &filter_layout, 4)?, &context.healthy)?;
        Ok(Self { guide, filter, classify, filter_layout })
    }

    // The guide program inlines the path tracer's BVH walks, so it compiles under the
    // process-wide lock of the path programs (#6706), never beside one of them. As for a scene
    // program, the lock is held until the pipelines are published: a job that waited for another
    // job's compile then uses its pipelines instead of compiling them again, and a job cancelled
    // while it waited returns before compiling. A compile that ran is kept for the next job.
    fn shared<'a>(context: &'a Context, cancel: &CancelToken) -> Result<&'a Self> {
        if let Some(pipelines) = context.denoise.get() {
            return Ok(pipelines);
        }
        cancel.check()?;
        let _compiling = one_compile_at_a_time();
        if let Some(pipelines) = context.denoise.get() {
            return Ok(pipelines);
        }
        cancel.check()?;
        let built = Self::new(context)?;
        Ok(context.denoise.get_or_init(|| built))
    }
}

pub(super) struct Resources {
    adaptive: Option<AdaptiveInput>,
    layout: Option<super::image_pages::Layout>,
    input: Option<wgpu::Buffer>,
    guides_ready: AtomicBool,
    guide_pipeline: wgpu::ComputePipeline,
    guide_bind: wgpu::BindGroup,
    filter_pipeline: wgpu::ComputePipeline,
    classify_pipeline: wgpu::ComputePipeline,
    filter_uniform: wgpu::Buffer,
    filter_binds: Vec<wgpu::BindGroup>,
    output: wgpu::Buffer,
    temporary: wgpu::Buffer,
    // Retained for diagnostic tests; bind groups also retain the guide allocation.
    _guides: wgpu::Buffer,
}

type Window = super::image_pages::Window;

fn full_window(task: &Task) -> Window {
    let pixels = task.request.width * task.request.height;
    Window { start: 0, pixels, core_start: 0, core_pixels: pixels }
}

impl Resources {
    #[cfg(test)]
    pub(super) fn new(
        context: &Context,
        path_uniform: &wgpu::Buffer,
        scene: [&wgpu::Buffer; 5],
        accumulation: &wgpu::Buffer,
        size: u64,
        adaptive_preview: bool,
    ) -> Result<Self> {
        Self::build(context, path_uniform, scene, accumulation, size, adaptive_preview, None, &CancelToken::default())
    }

    pub(super) fn new_for_layout(
        context: &Context,
        path_uniform: &wgpu::Buffer,
        scene: [&wgpu::Buffer; 5],
        accumulation: &wgpu::Buffer,
        adaptive_preview: bool,
        layout: super::image_pages::Layout,
        cancel: &CancelToken,
    ) -> Result<Self> {
        Self::build(context, path_uniform, scene, accumulation, u64::from(layout.max_window_pixels()) * 16, adaptive_preview, Some(layout), cancel)
    }

    fn build(
        context: &Context,
        path_uniform: &wgpu::Buffer,
        scene: [&wgpu::Buffer; 5],
        accumulation: &wgpu::Buffer,
        size: u64,
        adaptive_preview: bool,
        layout: Option<super::image_pages::Layout>,
        cancel: &CancelToken,
    ) -> Result<Self> {
        let pipelines = Pipelines::shared(context, cancel)?;
        let device = &context.device;
        let error_scopes = scopes(device);
        let buffer = |label, bytes| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size: bytes,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let input = layout.filter(|layout| layout.is_paged()).map(|layout| buffer("denoise gathered film window", size * u64::from(layout.planes)));
        let accumulation = input.as_ref().unwrap_or(accumulation);
        let guides = buffer("denoise first-hit guide window", size * (GUIDE_BYTES_PER_PIXEL / 16));
        let output = buffer("denoise filtered colour window", size);
        let temporary = buffer("denoise intermediate colour window", size);
        let [triangles, nodes, environment, textures, failures] = scene;
        let entries: Vec<_> = [path_uniform, triangles, nodes, &guides, environment, textures, failures]
            .into_iter()
            .enumerate()
            .map(|(binding, value)| wgpu::BindGroupEntry { binding: binding as u32, resource: value.as_entire_binding() })
            .collect();
        let guide_bind =
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("denoise guides"), layout: &context.layout, entries: &entries });
        let filter_uniform = match solidcraft_viewport_gpu::buffer_init(
            device,
            "denoise parameters",
            &[0; 48],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        ) {
            Ok(buffer) => buffer,
            Err(e) => {
                // Drain the scopes in order before returning.
                let _ = checked(error_scopes);
                return Err(gpu(e));
            }
        };
        let filter_binds = [(accumulation, &output), (&output, &temporary), (&temporary, &output)]
            .into_iter()
            .map(|(source, destination)| {
                let entries: Vec<_> = [&filter_uniform, &guides, source, destination]
                    .into_iter()
                    .enumerate()
                    .map(|(binding, value)| wgpu::BindGroupEntry { binding: binding as u32, resource: value.as_entire_binding() })
                    .collect();
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("denoise filter"),
                    layout: &pipelines.filter_layout,
                    entries: &entries,
                })
            })
            .collect();
        let adaptive =
            match adaptive_preview.then(|| AdaptiveInput::new(context, pipelines, accumulation, &guides, &filter_uniform, &output, size)).transpose()
            {
                Ok(adaptive) => adaptive,
                Err(e) => {
                    // Drain the scopes in order before returning.
                    let _ = checked(error_scopes);
                    return Err(e);
                }
            };
        checked(error_scopes)?;
        Ok(Self {
            adaptive,
            layout,
            input,
            guides_ready: AtomicBool::new(false),
            guide_pipeline: pipelines.guide.clone(),
            guide_bind,
            filter_pipeline: pipelines.filter.clone(),
            classify_pipeline: pipelines.classify.clone(),
            filter_uniform,
            filter_binds,
            output,
            temporary,
            _guides: guides,
        })
    }

    pub(super) fn output(&self, passes: u32) -> &wgpu::Buffer {
        if passes == 2 { &self.temporary } else { &self.output }
    }

    #[cfg(test)]
    pub(super) fn run(&self, task: &Task) -> Result<()> {
        self.prepare_window(task, full_window(task), false)?;
        self.run_filter(task, false)
    }

    fn prepare_window(&self, task: &Task, window: Window, preview: bool) -> Result<()> {
        if let Some(input) = &self.input {
            let context = &task.context;
            context.ensure_available()?;
            let submission = super::checked_creation(&context.device, || {
                let mut encoder =
                    context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("denoise film window gather") });
                let planes = if preview { task.page_layout.planes } else { 1 };
                for plane in 0..planes {
                    task.copy_film_range(&mut encoder, window.start, window.pixels, plane, input, u64::from(plane) * u64::from(window.pixels) * 16)?;
                }
                Ok(context.queue.submit([encoder.finish()]))
            })?;
            context.wait(submission, TIMEOUT)?;
        }
        // The immutable scene permits caching only when the entire guide image
        // is resident. A paged job reuses this allocation for every window.
        if self.input.is_some() || !self.guides_ready.load(Ordering::Relaxed) {
            self.generate_guides_window(task, window)?;
            self.guides_ready.store(true, Ordering::Relaxed);
        }
        if preview {
            self.adaptive.as_ref().ok_or_else(|| super::gpu("missing adaptive denoise preview input"))?.normalize_window(task, window)?;
        }
        task.cancel.check()
    }

    #[cfg(test)]
    fn generate_guides(&self, task: &Task) -> Result<()> {
        self.generate_guides_window(task, full_window(task))
    }

    fn generate_guides_window(&self, task: &Task, window: Window) -> Result<()> {
        // Camera dimensions, footprints and random keys use global pixels.
        // Only guide storage uses the current window's local index.
        let guide_flags = u32::from(task.photons.is_some()) | (u32::from(task.optical_resolve.is_some()) << 1);
        let mut budget = super::dispatch::Budget::default();
        let mut offset = 0;
        while offset < window.pixels {
            task.cancel.check()?;
            let count = budget.count(window.pixels - offset);
            let mut parameters = task.parameters(count)?;
            let global = window.start + offset;
            for (start, word) in
                [(12, global), (28, guide_flags), (super::PARAMETER_BYTES - 16, window.start), (super::PARAMETER_BYTES - 12, window.pixels)]
            {
                parameters
                    .get_mut(start..start + 4)
                    .ok_or_else(|| super::gpu("missing denoise window parameters"))?
                    .copy_from_slice(&word.to_le_bytes());
            }
            task.context.queue.write_buffer(&task.uniform, 0, &parameters);
            let started = std::time::Instant::now();
            dispatch(&task.context, &self.guide_pipeline, &self.guide_bind, count)?;
            budget.observe(count, started.elapsed());
            offset += count;
        }
        task.cancel.check()
    }

    /// Assemble only each core after filtering its complete dependency halo.
    /// The source film and every pixel/sample identity remain unchanged.
    pub(super) fn read_filtered(&self, task: &Task, preview: bool) -> Result<Vec<[f32; 4]>> {
        let Some(layout) = self.layout.filter(|layout| layout.is_paged()) else {
            let window = full_window(task);
            self.prepare_window(task, window, preview)?;
            self.run_filter_window(task, preview, window)?;
            return task.read_linear_buffer(self.output(task.options.denoise_settings.passes));
        };
        let mut values = super::scene::reserved((task.request.width * task.request.height) as usize)?;
        let source = self.output(task.options.denoise_settings.passes);
        for window in layout.windows() {
            self.prepare_window(task, window, preview)?;
            self.run_filter_window(task, preview, window)?;
            let bytes = u64::from(window.core_pixels) * 16;
            task.readback_data(
                bytes,
                |encoder| {
                    encoder.copy_buffer_to_buffer(source, u64::from(window.core_start - window.start) * 16, &task.readback, 0, bytes);
                    Ok(())
                },
                |data| {
                    for (index, pixel) in data.as_chunks::<16>().0.iter().enumerate() {
                        if index % 4096 == 0 {
                            task.cancel.check()?;
                        }
                        let mut value = [0.0; 4];
                        for (value, bytes) in value.iter_mut().zip(pixel.as_chunks::<4>().0) {
                            *value = f32::from_le_bytes(*bytes) / task.normalization_samples() as f32;
                            if !value.is_finite() || *value < 0.0 {
                                return Err(super::gpu("nonfinite or negative filtered path radiance"));
                            }
                        }
                        values.push(value);
                    }
                    Ok(())
                },
            )?;
        }
        if values.len() != (task.request.width * task.request.height) as usize {
            return Err(super::gpu("denoise windows do not cover the complete trace grid"));
        }
        task.validate_failures(None)?;
        Ok(values)
    }

    /// Each core contributes once; filter halos never enter the category counts
    /// or the conservative X2 category resolve.
    pub(super) fn diagnostics(&self, task: &Task) -> Result<(DenoiseDiagnostics, Vec<u8>)> {
        let output = &task.output_request;
        let capacity =
            usize::try_from(u64::from(output.width) * u64::from(output.height) * 4).map_err(|_| super::gpu("denoise mask exceeds address space"))?;
        let mut rgba = super::scene::reserved(capacity)?;
        let mut report = DenoiseDiagnostics::new(task.request.width, task.request.height);
        if let Some(layout) = self.layout.filter(|layout| layout.is_paged()) {
            for window in layout.windows() {
                self.diagnostic_window(task, window, &mut report, &mut rgba)?;
            }
        } else {
            self.diagnostic_window(task, full_window(task), &mut report, &mut rgba)?;
        }
        if rgba.len() != capacity {
            return Err(super::gpu("denoise diagnostic windows do not cover the final grid"));
        }
        task.cancel.check()?;
        Ok((report, rgba))
    }

    fn diagnostic_window(&self, task: &Task, window: Window, report: &mut DenoiseDiagnostics, rgba: &mut Vec<u8>) -> Result<()> {
        self.prepare_window(task, window, false)?;
        let bind = self.filter_binds.first().ok_or_else(|| super::gpu("missing diagnostic bind group"))?;
        let mut budget = super::dispatch::Budget::default();
        let mut offset = 0;
        while offset < window.pixels {
            task.cancel.check()?;
            let count = budget.count(window.pixels - offset);
            let mut bytes = Vec::with_capacity(48);
            for word in
                [task.request.width, task.request.height, 0, offset, count, task.normalization_samples(), 0, 0, window.start, window.pixels, 0, 0]
            {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            task.context.queue.write_buffer(&self.filter_uniform, 0, &bytes);
            let started = std::time::Instant::now();
            dispatch(&task.context, &self.classify_pipeline, bind, count)?;
            budget.observe(count, started.elapsed());
            offset += count;
        }
        let scale = task.options.supersampling as usize;
        let width = task.request.width as usize;
        let first_row = window.core_start as usize / width;
        let core_rows = window.core_pixels as usize / width;
        if !(1..=2).contains(&scale) || !first_row.is_multiple_of(scale) || !core_rows.is_multiple_of(scale) {
            return Err(super::gpu("diagnostic window does not own complete supersampling cells"));
        }
        let byte_count = u64::from(window.core_pixels) * 16;
        task.readback_data(
            byte_count,
            |encoder| {
                encoder.copy_buffer_to_buffer(&self.output, u64::from(window.core_start - window.start) * 16, &task.readback, 0, byte_count);
                Ok(())
            },
            |data| {
                for y in 0..core_rows / scale {
                    task.cancel.check()?;
                    for x in 0..task.output_request.width as usize {
                        let mut classes = [DenoiseGuideClass::Unavailable; 4];
                        for dy in 0..scale {
                            for dx in 0..scale {
                                let pixel = (y * scale + dy) * width + x * scale + dx;
                                let bytes: [u8; 4] = data
                                    .get(pixel * 16..pixel * 16 + 4)
                                    .and_then(|bytes| bytes.try_into().ok())
                                    .ok_or_else(|| super::gpu("missing denoise guide classification"))?;
                                let code = f32::from_le_bytes(bytes);
                                if !code.is_finite() || code < 0.0 || code.fract() != 0.0 {
                                    return Err(super::gpu("invalid denoise guide classification"));
                                }
                                let class =
                                    DenoiseGuideClass::from_code(code as u32).ok_or_else(|| super::gpu("unknown denoise guide classification"))?;
                                report.record(class);
                                *classes.get_mut(dy * scale + dx).ok_or_else(|| super::gpu("invalid diagnostic supersample scale"))? = class;
                            }
                        }
                        let class = resolve_denoise_class(classes.into_iter().take(scale * scale));
                        rgba.extend_from_slice(&class.color());
                    }
                }
                Ok(())
            },
        )
    }

    #[cfg(test)]
    fn run_filter(&self, task: &Task, preview: bool) -> Result<()> {
        self.run_filter_window(task, preview, full_window(task))
    }

    fn run_filter_window(&self, task: &Task, preview: bool, window: Window) -> Result<()> {
        let settings = task.options.denoise_settings;
        settings.validate()?;
        for (pass, bind) in self.filter_binds.iter().take(settings.passes as usize).enumerate() {
            let bind = if preview && pass == 0 {
                &self.adaptive.as_ref().ok_or_else(|| super::gpu("missing adaptive denoise preview input"))?.filter_bind
            } else {
                bind
            };
            let mut budget = super::dispatch::Budget::default();
            let mut offset = 0;
            while offset < window.pixels {
                task.cancel.check()?;
                let count = budget.count(window.pixels - offset);
                let mut bytes = Vec::with_capacity(48);
                for word in [
                    task.request.width,
                    task.request.height,
                    1 << pass,
                    offset,
                    count,
                    task.normalization_samples(),
                    (settings.strength as f32).to_bits(),
                    u32::from(settings.firefly_suppression),
                    window.start,
                    window.pixels,
                    u32::from(settings.optical_filter),
                    0,
                ] {
                    bytes.extend_from_slice(&word.to_le_bytes());
                }
                task.context.queue.write_buffer(&self.filter_uniform, 0, &bytes);
                let started = std::time::Instant::now();
                dispatch(&task.context, &self.filter_pipeline, bind, count)?;
                budget.observe(count, started.elapsed());
                offset += count;
            }
        }
        task.cancel.check()
    }
}

/// A distinct normalized input for adaptive display only. The final filter stays bound
/// to the original production accumulation, and this pass never writes that film.
struct AdaptiveInput {
    _pixels: wgpu::Buffer,
    uniform: wgpu::Buffer,
    pipeline: wgpu::ComputePipeline,
    bind: wgpu::BindGroup,
    filter_bind: wgpu::BindGroup,
}
impl AdaptiveInput {
    fn new(
        context: &Context,
        pipelines: &Pipelines,
        accumulation: &wgpu::Buffer,
        guides: &wgpu::Buffer,
        filter_uniform: &wgpu::Buffer,
        output: &wgpu::Buffer,
        size: u64,
    ) -> Result<Self> {
        let device = &context.device;
        let pixels = device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("adaptive preview denoise input"),
            size,
            usage: wgpu::BufferUsages::STORAGE,
            mapped_at_creation: false,
        });
        let uniform = solidcraft_viewport_gpu::buffer_init(
            device,
            "adaptive preview parameters",
            &[0; 32],
            wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        )
        .map_err(gpu)?;
        let entries: Vec<_> = (0..3)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 { wgpu::BufferBindingType::Uniform } else { wgpu::BufferBindingType::Storage { read_only: binding == 1 } },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout =
            device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("adaptive preview normalization"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("adaptive preview normalization"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("adaptive preview normalization"),
            source: wgpu::ShaderSource::Wgsl(include_str!("adaptive_preview.wgsl").into()),
        });
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("adaptive preview normalization"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: Default::default(),
            cache: None,
        });
        let entries: Vec<_> = [&uniform, accumulation, &pixels]
            .into_iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry { binding: binding as u32, resource: buffer.as_entire_binding() })
            .collect();
        let bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adaptive preview normalization"),
            layout: &layout,
            entries: &entries,
        });
        let entries: Vec<_> = [filter_uniform, guides, &pixels, output]
            .into_iter()
            .enumerate()
            .map(|(binding, buffer)| wgpu::BindGroupEntry { binding: binding as u32, resource: buffer.as_entire_binding() })
            .collect();
        let filter_bind = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("adaptive preview filter"),
            layout: &pipelines.filter_layout,
            entries: &entries,
        });
        Ok(Self { _pixels: pixels, uniform, pipeline, bind, filter_bind })
    }
    fn normalize_window(&self, task: &Task, window: Window) -> Result<()> {
        let options = task.options.adaptive.ok_or_else(|| super::gpu("missing adaptive preview options"))?;
        let pixels = window.pixels;
        let mut budget = super::dispatch::Budget::default();
        let mut offset = 0;
        while offset < pixels {
            task.cancel.check()?;
            let count = budget.count(pixels - offset);
            let mut bytes = Vec::with_capacity(32);
            for word in [task.request.width, window.pixels / task.request.width, task.sample, offset, count, options.pilot_samples, 0, 0] {
                bytes.extend_from_slice(&word.to_le_bytes());
            }
            task.context.queue.write_buffer(&self.uniform, 0, &bytes);
            let started = std::time::Instant::now();
            dispatch(&task.context, &self.pipeline, &self.bind, count)?;
            budget.observe(count, started.elapsed());
            offset += count;
        }
        task.cancel.check()
    }
}

fn dispatch(context: &Context, pipeline: &wgpu::ComputePipeline, bind: &wgpu::BindGroup, count: u32) -> Result<()> {
    context.ensure_available()?;
    let error_scopes = scopes(&context.device);
    let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("denoise tile") });
    {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("denoise tile"), timestamp_writes: None });
        pass.set_pipeline(pipeline);
        pass.set_bind_group(0, bind, &[]);
        pass.dispatch_workgroups(count.div_ceil(64), 1, 1);
    }
    let submission = context.queue.submit([encoder.finish()]);
    checked(error_scopes)?;
    context.wait(submission, TIMEOUT)?;
    Ok(())
}
