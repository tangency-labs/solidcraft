//! Window-local preparation and linear recombination of exclusive components.
//! No full component image crosses the CPU/GPU boundary during reconstruction.
use super::{Context, Result, Task, Window, checked, dispatch, scopes};

pub(super) const EXTRA_BYTES_PER_PIXEL: u64 = 64;

pub(super) struct Pipelines {
    prepare: wgpu::ComputePipeline,
    prepare_layout: wgpu::BindGroupLayout,
    add: wgpu::ComputePipeline,
    add_layout: wgpu::BindGroupLayout,
}
impl Pipelines {
    fn new(context: &Context) -> Result<Self> {
        let error_scopes = scopes(&context.device);
        let (prepare, prepare_layout) = pipeline(context, "component prepare", include_str!("denoise_component_prepare.wgsl"), "prepare", 7, 4);
        let (add, add_layout) = pipeline(context, "component sum", include_str!("denoise_component_sum.wgsl"), "add", 4, 3);
        checked(error_scopes)?;
        Ok(Self { prepare, prepare_layout, add, add_layout })
    }
    fn shared<'a>(context: &Context, parent: &'a super::Pipelines) -> Result<&'a Self> {
        if let Some(pipelines) = parent.components.get() {
            return Ok(pipelines);
        }
        let built = Self::new(context)?;
        Ok(parent.components.get_or_init(|| built))
    }
}
fn pipeline(
    context: &Context,
    label: &str,
    source: &str,
    entry: &str,
    bindings: u32,
    first_write: u32,
) -> (wgpu::ComputePipeline, wgpu::BindGroupLayout) {
    let device = &context.device;
    let entries: Vec<_> = (0..bindings)
        .map(|binding| wgpu::BindGroupLayoutEntry {
            binding,
            visibility: wgpu::ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: if binding == 0 {
                    wgpu::BufferBindingType::Uniform
                } else {
                    wgpu::BufferBindingType::Storage { read_only: binding < first_write }
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some(label), entries: &entries });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor { label: Some(label), source: wgpu::ShaderSource::Wgsl(source.into()) });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module: &shader,
        entry_point: Some(entry),
        compilation_options: Default::default(),
        cache: None,
    });
    (pipeline, layout)
}
fn bind(device: &wgpu::Device, layout: &wgpu::BindGroupLayout, label: &str, buffers: &[&wgpu::Buffer]) -> wgpu::BindGroup {
    let entries: Vec<_> = buffers
        .iter()
        .enumerate()
        .map(|(binding, buffer)| wgpu::BindGroupEntry { binding: binding as u32, resource: buffer.as_entire_binding() })
        .collect();
    device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some(label), layout, entries: &entries })
}

pub(super) struct Inputs<'a> {
    pub uniform: &'a wgpu::Buffer,
    pub beauty: &'a wgpu::Buffer,
    pub preview: Option<&'a wgpu::Buffer>,
    pub guides: &'a wgpu::Buffer,
    pub output: &'a wgpu::Buffer,
    pub temporary: &'a wgpu::Buffer,
    pub uncertainty: &'a wgpu::Buffer,
}

pub(super) struct Resources {
    input: wgpu::Buffer,
    moments: wgpu::Buffer,
    counts: wgpu::Buffer,
    pub sum: wgpu::Buffer,
    prepare: wgpu::ComputePipeline,
    prepare_bind: wgpu::BindGroup,
    preview_bind: Option<wgpu::BindGroup>,
    pub filter_bind: wgpu::BindGroup,
    add: wgpu::ComputePipeline,
    add_binds: [wgpu::BindGroup; 2],
}
impl Resources {
    pub(super) fn new(context: &Context, parent: &super::Pipelines, inputs: Inputs<'_>, size: u64) -> Result<Self> {
        let pipelines = Pipelines::shared(context, parent)?;
        let device = &context.device;
        let error_scopes = scopes(device);
        let buffer = |label| {
            device.create_buffer(&wgpu::BufferDescriptor {
                label: Some(label),
                size,
                usage: wgpu::BufferUsages::STORAGE | wgpu::BufferUsages::COPY_SRC | wgpu::BufferUsages::COPY_DST,
                mapped_at_creation: false,
            })
        };
        let input = buffer("component normalized input");
        let moments = buffer("component raw moments window");
        let counts = buffer("component observation counts window");
        let sum = buffer("component linear sum window");
        let prepare_bind = bind(
            device,
            &pipelines.prepare_layout,
            "component preparation",
            &[inputs.uniform, &moments, &counts, inputs.beauty, &input, inputs.uncertainty, &sum],
        );
        let preview_bind = inputs.preview.map(|preview| {
            bind(
                device,
                &pipelines.prepare_layout,
                "component preview preparation",
                &[inputs.uniform, &moments, &counts, preview, &input, inputs.uncertainty, &sum],
            )
        });
        let filter_bind =
            bind(device, &parent.filter_layout, "component filtering", &[inputs.uniform, inputs.guides, &input, inputs.output, inputs.uncertainty]);
        let add_binds = [inputs.output, inputs.temporary]
            .map(|filtered| bind(device, &pipelines.add_layout, "component recombination", &[inputs.uniform, filtered, &counts, &sum]));
        checked(error_scopes)?;
        Ok(Self {
            input,
            moments,
            counts,
            sum,
            prepare: pipelines.prepare.clone(),
            prepare_bind,
            preview_bind,
            filter_bind,
            add: pipelines.add.clone(),
            add_binds,
        })
    }

    pub(super) fn prepare(&self, task: &Task, uniform: &wgpu::Buffer, window: Window, component: u32, preview: bool) -> Result<()> {
        if component >= 6 || !task.options.radiance_components {
            return Err(super::super::gpu("component filter has no complete captured partition"));
        }
        task.cancel.check()?;
        let base = if task.options.adaptive.is_some() { 3 } else { 1 };
        let submission = super::super::checked_creation(&task.context.device, || {
            let mut encoder =
                task.context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("component film window gather") });
            if component == 0 {
                task.copy_film_range(&mut encoder, window.start, window.pixels, base, &self.counts, 0)?;
            }
            task.copy_film_range(&mut encoder, window.start, window.pixels, base + 3 + component, &self.input, 0)?;
            task.copy_film_range(&mut encoder, window.start, window.pixels, base + 9 + component, &self.moments, 0)?;
            Ok(task.context.queue.submit([encoder.finish()]))
        })?;
        task.context.wait(submission, super::TIMEOUT)?;
        let bind = if preview {
            self.preview_bind.as_ref().ok_or_else(|| super::super::gpu("component adaptive preview input is missing"))?
        } else {
            &self.prepare_bind
        };
        self.dispatch_window(task, uniform, window, component, task.normalization_samples(), &self.prepare, bind)
    }

    pub(super) fn add(&self, task: &Task, uniform: &wgpu::Buffer, window: Window, component: u32) -> Result<()> {
        let index = usize::from(task.options.denoise_settings.passes == 2);
        let bind = self.add_binds.get(index).ok_or_else(|| super::super::gpu("component sum source is unavailable"))?;
        self.dispatch_window(task, uniform, window, component, 1, &self.add, bind)
    }

    #[allow(clippy::too_many_arguments)]
    fn dispatch_window(
        &self,
        task: &Task,
        uniform: &wgpu::Buffer,
        window: Window,
        component: u32,
        samples: u32,
        pipeline: &wgpu::ComputePipeline,
        bind: &wgpu::BindGroup,
    ) -> Result<()> {
        let mut budget = super::super::dispatch::Budget::default();
        let mut offset = 0;
        while offset < window.pixels {
            task.cancel.check()?;
            let count = budget.count(window.pixels - offset);
            let mut bytes = [0_u8; 48];
            for (destination, word) in bytes.as_chunks_mut::<4>().0.iter_mut().zip([
                task.request.width,
                task.request.height,
                0,
                offset,
                count,
                samples,
                0,
                0,
                window.start,
                window.pixels,
                0,
                component,
            ]) {
                destination.copy_from_slice(&word.to_le_bytes());
            }
            task.context.queue.write_buffer(uniform, 0, &bytes);
            let started = std::time::Instant::now();
            dispatch(&task.context, pipeline, bind, count)?;
            budget.observe(count, started.elapsed());
            offset += count;
        }
        task.cancel.check()
    }
}
