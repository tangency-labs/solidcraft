//! Optional output filtering; never writes the Monte Carlo accumulation buffer.
use super::{Context, Result, SHADER, TIMEOUT, Task, checked, placeholder_bind, scopes, warm_up};
use wgpu::util::DeviceExt;

pub(super) const BYTES_PER_PIXEL: u64 = 80; // three guide vec4s and two colour vec4s
const FILTER: &str = include_str!("denoise.wgsl");
const GUIDES: &str = include_str!("denoise_guides.wgsl");
#[cfg(test)]
#[path = "denoise_tests.rs"]
mod tests;

// Shared by every denoised job on a device: the guide pipeline compiles the whole
// path shader, so it is built and warmed once rather than per job.
pub(super) struct Pipelines {
    guide: wgpu::ComputePipeline,
    filter: wgpu::ComputePipeline,
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
            compilation_options: Default::default(),
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
        checked(error_scopes)?;
        warm_up(device, &context.queue, &guide, &placeholder_bind(device, &context.layout, 7)?, &context.healthy)?;
        warm_up(device, &context.queue, &filter, &placeholder_bind(device, &filter_layout, 4)?, &context.healthy)?;
        Ok(Self { guide, filter, filter_layout })
    }

    fn shared(context: &Context) -> Result<&Self> {
        if let Some(pipelines) = context.denoise.get() {
            return Ok(pipelines);
        }
        let built = Self::new(context)?;
        Ok(context.denoise.get_or_init(|| built))
    }
}

pub(super) struct Resources {
    guide_pipeline: wgpu::ComputePipeline,
    guide_bind: wgpu::BindGroup,
    filter_pipeline: wgpu::ComputePipeline,
    filter_uniform: wgpu::Buffer,
    filter_binds: Vec<wgpu::BindGroup>,
    output: wgpu::Buffer,
    // Retained for diagnostic tests; bind groups also retain the guide allocation.
    _guides: wgpu::Buffer,
}

impl Resources {
    pub(super) fn new(
        context: &Context,
        path_uniform: &wgpu::Buffer,
        scene: [&wgpu::Buffer; 5],
        accumulation: &wgpu::Buffer,
        size: u64,
    ) -> Result<Self> {
        let pipelines = Pipelines::shared(context)?;
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
        let guides = buffer("denoise first-hit guides", size * 3);
        let output = buffer("denoise filtered colour", size);
        let temporary = buffer("denoise intermediate colour", size);
        let [triangles, nodes, environment, textures, failures] = scene;
        let entries: Vec<_> = [path_uniform, triangles, nodes, &guides, environment, textures, failures]
            .into_iter()
            .enumerate()
            .map(|(binding, value)| wgpu::BindGroupEntry { binding: binding as u32, resource: value.as_entire_binding() })
            .collect();
        let guide_bind =
            device.create_bind_group(&wgpu::BindGroupDescriptor { label: Some("denoise guides"), layout: &context.layout, entries: &entries });
        let filter_uniform = device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
            label: Some("denoise parameters"),
            contents: &[0; 32],
            usage: wgpu::BufferUsages::UNIFORM | wgpu::BufferUsages::COPY_DST,
        });
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
        checked(error_scopes)?;
        Ok(Self {
            guide_pipeline: pipelines.guide.clone(),
            guide_bind,
            filter_pipeline: pipelines.filter.clone(),
            filter_uniform,
            filter_binds,
            output,
            _guides: guides,
        })
    }

    pub(super) fn output(&self) -> &wgpu::Buffer {
        &self.output
    }

    pub(super) fn run(&self, task: &Task) -> Result<()> {
        let pixels = task.request.width * task.request.height;
        let mut budget = super::dispatch::Budget::default();
        let mut offset = 0;
        while offset < pixels {
            task.cancel.check()?;
            let count = budget.count(pixels - offset);
            let mut parameters = task.parameters(count);
            if let Some(bytes) = parameters.get_mut(12..16) {
                bytes.copy_from_slice(&offset.to_le_bytes());
            }
            task.context.queue.write_buffer(&task.uniform, 0, &parameters);
            let started = std::time::Instant::now();
            dispatch(&task.context, &self.guide_pipeline, &self.guide_bind, count)?;
            budget.observe(count, started.elapsed());
            offset += count;
        }
        self.run_filter(task)
    }

    fn run_filter(&self, task: &Task) -> Result<()> {
        let pixels = task.request.width * task.request.height;
        for (pass, bind) in self.filter_binds.iter().enumerate() {
            let mut budget = super::dispatch::Budget::default();
            let mut offset = 0;
            while offset < pixels {
                task.cancel.check()?;
                let count = budget.count(pixels - offset);
                let mut bytes = Vec::with_capacity(32);
                for word in [task.request.width, task.request.height, 1 << pass, offset, count, task.normalization_samples(), 0, 0] {
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
