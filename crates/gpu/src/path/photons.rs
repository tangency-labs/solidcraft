//! Opt-in, prepaid GPU caustic photon storage and cancellable measured prepasses.
//! Legacy jobs retain the original six-storage-buffer shader/layout unchanged.
use super::*;
use solidcraft_render::job::{
    CausticOptions,
    photon_sources::{self as power, MAX_PHOTON_SOURCES, PhotonSourceDistribution},
};

const HEADER_WORDS: u32 = 16;
const RECORD_WORDS: u32 = 16;
const GUIDE_WORDS: u32 = 4;
const BOUNDS_WORDS: u32 = 6;
const SOURCE_WORDS: u32 = 1 + MAX_PHOTON_SOURCES as u32;
fn extra_words(options: CausticOptions) -> u32 {
    (if options.guided_emission || options.power_weighted_sources { GUIDE_WORDS } else { 0 })
        + if options.power_weighted_sources { SOURCE_WORDS } else { 0 }
}
const DISABLED: &str = include_str!("photons_disabled.wgsl");
const PHOTONS: &str = include_str!("photons.wgsl");

pub(super) fn heads(options: CausticOptions) -> Result<u32> {
    options.validate()?;
    options.photons.checked_mul(2).and_then(u32::checked_next_power_of_two).ok_or_else(|| gpu("caustic hash capacity overflow"))
}
pub(super) fn buffer_bytes(options: CausticOptions) -> Result<u64> {
    let words = u64::from(HEADER_WORDS + extra_words(options) + BOUNDS_WORDS)
        + u64::from(heads(options)?)
        + u64::from(options.photons) * u64::from(RECORD_WORDS);
    words.checked_mul(4).ok_or_else(|| gpu("caustic buffer size overflow"))
}
pub(super) fn admission_bytes(options: Option<CausticOptions>) -> Result<u64> {
    options.map_or(Ok(0), |options| {
        let upload_words = HEADER_WORDS + extra_words(options) + BOUNDS_WORDS;
        buffer_bytes(options)?.checked_add(u64::from(upload_words) * 4).ok_or_else(|| gpu("caustic header estimate overflow"))
    })
}

/// Host-side source order and enable predicates mirror photon_trace exactly.
/// The bounded CDF is uploaded once and retained across map refreshes.
pub(super) fn source_distribution(
    options: JobOptions,
    request: &ImageRequest,
    nodes: &[u8],
    guide: Option<[f32; 4]>,
    hdr_integral: f64,
) -> Result<Option<PhotonSourceDistribution>> {
    let Some(caustics) = options.caustics.filter(|c| c.power_weighted_sources) else { return Ok(None) };
    let mut bounds = [0.0; 6];
    for (value, index) in bounds.iter_mut().zip([0usize, 1, 2, 4, 5, 6]) {
        let bytes = nodes.get(index * 4..index * 4 + 4).and_then(|v| v.try_into().ok()).ok_or_else(|| gpu("photon power bounds missing"))?;
        *value = f64::from(f32::from_le_bytes(bytes));
    }
    let radius = ((bounds[3] - bounds[0]).powi(2) + (bounds[4] - bounds[1]).powi(2) + (bounds[5] - bounds[2]).powi(2)).sqrt() * 0.5;
    let radius = guide.filter(|g| caustics.guided_emission && g[3] > 0.0 && f64::from(g[3]) < radius).map_or(radius, |g| f64::from(g[3]));
    let studio = options.lighting.is_some() || request.environment.is_some();
    let lighting = options.lighting.unwrap_or_default();
    let mut powers = [0.0; MAX_PHOTON_SOURCES];
    let mut count = 0;
    let mut push = |value| -> Result<()> {
        *powers.get_mut(count).ok_or_else(|| gpu("photon source proposal capacity exceeded"))? = value;
        count += 1;
        Ok(())
    };
    if studio {
        for light in lighting.directional_lights.iter() {
            push(power::disk_power(radius, power::luminance(light.color.map(|c| c * light.intensity))?)?)?;
        }
        for light in lighting.punctual_lights.iter() {
            push(power::punctual_power(light)?)?;
        }
    } else {
        for (intensity, weight) in [3.0, 1.0, 1.5].into_iter().zip(request.studio_weights.unwrap_or([1.0; 3])) {
            push(power::disk_power(radius, intensity * weight)?)?;
        }
    }
    if caustics.area_lights && caustics.max_depth > 1 {
        for light in lighting.area_lights.iter() {
            push(power::rectangle_power(light.area(), light.radiance)?)?;
        }
    }
    let lit_environment =
        request.environment.as_ref().map_or(lighting.environment.into_iter().any(|c| (c as f32).to_bits() & 0x7fff_ffff != 0), |e| {
            (e.info().intensity as f32).to_bits() & 0x7fff_ffff != 0
        });
    if studio && lit_environment && caustics.environment && caustics.max_depth > 1 {
        let integral =
            if request.environment.is_some() { hdr_integral } else { 4.0 * std::f64::consts::PI * power::luminance(lighting.environment)? };
        push(power::disk_power(radius, integral)?)?;
    }
    if count == 0 { Ok(None) } else { PhotonSourceDistribution::new(&powers[..count]).map(Some) }
}

pub(super) fn validate_scene(scene: &ProductScene, options: JobOptions) -> Result<()> {
    let Some(caustics) = options.caustics else { return Ok(()) };
    caustics.validate()?;
    if options.volume.is_some_and(|volume| volume.active()) {
        return Err(JobError::new(ErrorCode::InvalidRequest, "caustic photons cannot combine with active external volume transport"));
    }
    let validate = |transmission: Option<solidcraft_render::transmission::Transmission>| -> Result<()> {
        if let Some(optics) = transmission
            && optics.shadow_transmission()
            && optics.thin_wall_thickness_mm().is_none()
        {
            return Err(JobError::new(
                ErrorCode::InvalidRequest,
                "caustic photons require disabled straight closed-glass shadow transmission to avoid counting the same direct light twice",
            ));
        }
        Ok(())
    };
    for body in &scene.meshes {
        validate(body.transmission)?;
        for face in &body.faces {
            validate(face.transmission)?;
        }
    }
    Ok(())
}

/// Photon origin classification currently consumes a rounded f32 point. A
/// punctual source's retained low part must not be able to cross a nearby closed
/// optical boundary and change that classification. Check once per source/body,
/// without an uncharged all-triangle scan in every emitted photon invocation.
pub(super) fn validate_punctual_origins(scene: &ProductScene, origin: Vec3, options: JobOptions, epsilon: f32, cancel: &CancelToken) -> Result<()> {
    if options.caustics.is_none() {
        return Ok(());
    }
    let closed =
        |optics: Option<solidcraft_render::transmission::Transmission>| optics.is_some_and(|optics| optics.thin_wall_thickness_mm().is_none());
    if let Some(lighting) = options.lighting {
        for light in lighting.punctual_lights.iter() {
            let parameters = punctual_parameters(light, origin);
            let high = parameters.first().ok_or_else(|| gpu("missing point-source origin"))?;
            let low = parameters.get(1).ok_or_else(|| gpu("missing point-source origin residual"))?;
            let point = Vec3::new(f64::from(high[0]), f64::from(high[1]), f64::from(high[2]));
            let residual = Vec3::new(f64::from(low[0]), f64::from(low[1]), f64::from(low[2]));
            let uncertainty = residual.len() + f64::from(epsilon);
            for body in &scene.meshes {
                if !closed(body.transmission) && !body.faces.iter().any(|face| closed(face.transmission)) {
                    continue;
                }
                for (ordinal, indices) in body.mesh.triangles.iter().enumerate() {
                    if ordinal % 256 == 0 {
                        cancel.check()?;
                    }
                    let mut points = [Vec3::ZERO; 3];
                    for (point, index) in points.iter_mut().zip(indices) {
                        let relative = *body.mesh.positions.get(*index as usize).ok_or_else(|| gpu("invalid photon optical vertex"))? - origin;
                        *point = Vec3::new(f64::from(relative.x as f32), f64::from(relative.y as f32), f64::from(relative.z as f32));
                    }
                    let bounds = points.into_iter().collect::<solidcraft_geom::Aabb3>();
                    if !bounds.inflated(uncertainty).contains(point) {
                        continue;
                    }
                    if let Ok(normal) = solidcraft_viewport::surface::unit_direction((points[1] - points[0]).cross(points[2] - points[0]))
                        && (point - points[0]).dot(normal).abs() <= uncertainty
                    {
                        return Err(JobError::new(
                            ErrorCode::InvalidScene,
                            "caustic point or spot origin is too close to a closed Glass boundary for unambiguous GPU medium classification",
                        ));
                    }
                }
            }
        }
    }
    cancel.check()
}

pub(super) struct Pipelines {
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) render: wgpu::ComputePipeline,
    clear: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    // The camera program with the indirect clamp compiled in, built by the first clamped job.
    pub(super) clamped_render: std::sync::OnceLock<wgpu::ComputePipeline>,
}
impl Pipelines {
    pub(super) fn shared(context: &Context) -> Result<&Self> {
        if let Some(pipelines) = context.photons.get() {
            return Ok(pipelines);
        }
        if context.device.limits().max_storage_buffers_per_shader_stage < 7 {
            return Err(JobError::new(
                ErrorCode::HostUnavailable,
                "caustics require seven GPU storage buffers; ordinary GPU images remain available",
            ));
        }
        let source = shader_source()?;
        let compiling = one_compile_at_a_time();
        let device = &context.device;
        let scopes = scopes(device);
        let module = device
            .create_shader_module(wgpu::ShaderModuleDescriptor { label: Some("gpu-path caustics"), source: wgpu::ShaderSource::Wgsl(source.into()) });
        let entries: Vec<_> = (0..8)
            .map(|binding| wgpu::BindGroupLayoutEntry {
                binding,
                visibility: wgpu::ShaderStages::COMPUTE,
                ty: wgpu::BindingType::Buffer {
                    ty: if binding == 0 {
                        wgpu::BufferBindingType::Uniform
                    } else {
                        wgpu::BufferBindingType::Storage { read_only: binding != 3 && binding != 6 && binding != 7 }
                    },
                    has_dynamic_offset: false,
                    min_binding_size: None,
                },
                count: None,
            })
            .collect();
        let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor { label: Some("gpu-path caustics"), entries: &entries });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu-path caustics"),
            bind_group_layouts: &[Some(&layout)],
            immediate_size: 0,
        });
        let pipeline = |entry_point| {
            device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
                label: Some(entry_point),
                layout: Some(&pipeline_layout),
                module: &module,
                entry_point: Some(entry_point),
                compilation_options: wgpu::PipelineCompilationOptions {
                    constants: &[("OPAQUE_SCENE", 0.0), ("HAS_TEXTURES", 1.0)],
                    ..Default::default()
                },
                cache: None,
            })
        };
        let built = Self {
            render: pipeline("main"),
            clear: pipeline("photon_clear"),
            emit: pipeline("photon_emit"),
            layout,
            clamped_render: std::sync::OnceLock::new(),
        };
        checked(scopes)?;
        drop(compiling);
        let dummy = placeholder_bind(device, &built.layout, 8)?;
        for pipeline in [&built.render, &built.clear, &built.emit] {
            warm_up(device, &context.queue, pipeline, &dummy, &context.healthy)?;
        }
        Ok(context.photons.get_or_init(|| built))
    }
    /// The camera program with the indirect clamp (JobOptions::clamp_indirect), on the same layout.
    fn clamped_render(&self, context: &Context) -> Result<&wgpu::ComputePipeline> {
        if let Some(pipeline) = self.clamped_render.get() {
            return Ok(pipeline);
        }
        let source = shader_source()?;
        let _compiling = one_compile_at_a_time();
        if let Some(pipeline) = self.clamped_render.get() {
            return Ok(pipeline);
        }
        let device = &context.device;
        let scopes = scopes(device);
        let module = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("gpu-path caustics clamped"),
            source: wgpu::ShaderSource::Wgsl(source.into()),
        });
        let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
            label: Some("gpu-path caustics clamped"),
            bind_group_layouts: &[Some(&self.layout)],
            immediate_size: 0,
        });
        let mut constants = vec![("OPAQUE_SCENE", 0.0), ("HAS_TEXTURES", 1.0)];
        constants.extend_from_slice(CLAMP_INDIRECT);
        let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
            label: Some("main"),
            layout: Some(&pipeline_layout),
            module: &module,
            entry_point: Some("main"),
            compilation_options: wgpu::PipelineCompilationOptions { constants: &constants, ..Default::default() },
            cache: None,
        });
        checked(scopes)?;
        warm_up(device, &context.queue, &pipeline, &placeholder_bind(device, &self.layout, 8)?, &context.healthy)?;
        let _ = self.clamped_render.set(pipeline);
        self.clamped_render.get().ok_or_else(|| gpu("clamped caustic pipeline publication failed"))
    }
}
pub(super) fn shader_source() -> Result<String> {
    let base = SHADER.strip_suffix(DISABLED).ok_or_else(|| gpu("legacy path shader has no photon-disabled suffix"))?;
    let mut source = String::new();
    source
        .try_reserve_exact(base.len() + PHOTONS.len())
        .map_err(|_| JobError::new(ErrorCode::AllocationFailed, "caustic shader source allocation failed"))?;
    source.push_str(base);
    source.push_str(PHOTONS);
    Ok(source)
}

pub(super) struct Resources {
    pub(super) buffer: wgpu::Buffer,
    pub(super) layout: wgpu::BindGroupLayout,
    pub(super) render: wgpu::ComputePipeline,
    clear: wgpu::ComputePipeline,
    emit: wgpu::ComputePipeline,
    photons: u32,
    head_count: u32,
    pass: u32,
    cleared: u32,
    emitted: u32,
    budget: dispatch::Budget,
}
impl Resources {
    pub(super) fn new(
        context: &Context,
        options: CausticOptions,
        nodes: &[u8],
        guide: Option<[f32; 4]>,
        bounds: Option<[f32; 6]>,
        sources: Option<&PhotonSourceDistribution>,
        clamp: bool,
    ) -> Result<Self> {
        options.validate()?;
        let head_count = heads(options)?;
        let word = |index: usize| -> Result<u32> {
            let begin = index.checked_mul(4).ok_or_else(|| gpu("caustic bounds offset overflow"))?;
            let bytes: [u8; 4] =
                nodes.get(begin..begin + 4).and_then(|bytes| bytes.try_into().ok()).ok_or_else(|| gpu("caustics require captured BVH bounds"))?;
            Ok(u32::from_le_bytes(bytes))
        };
        let mut header = [0u32; HEADER_WORDS as usize];
        header[0] = options.photons;
        header[1] = head_count;
        header[2] = (options.radius_mm as f32).to_bits();
        header[3] = u32::from(options.max_depth);
        header[4] = options.seed;
        for (at, index) in [(6, 0), (7, 1), (8, 2), (9, 4), (10, 5), (11, 6)] {
            let bits = word(index)?;
            let coordinate = f32::from_bits(bits);
            if !coordinate.is_finite() {
                return Err(JobError::new(ErrorCode::InvalidRequest, "caustics require finite captured BVH bounds"));
            }
            header[at] = bits;
        }
        for (lo, hi) in [(6, 9), (7, 10), (8, 11)] {
            let span = f64::from(f32::from_bits(header[hi])) - f64::from(f32::from_bits(header[lo]));
            if !(0.0..=1_048_576.0).contains(&(span / options.radius_mm)) {
                return Err(JobError::new(
                    ErrorCode::InvalidRequest,
                    "caustic grid exceeds 2^20 radius cells along an axis; increase the radius explicitly",
                ));
            }
        }
        header[12] = u32::from(options.area_lights);
        header[13] = u32::from(options.environment);
        let guide = guide.filter(|_| options.guided_emission);
        header[14] = u32::from(guide.is_some()) | (u32::from(sources.is_some()) << 1) | (u32::from(bounds.is_some()) << 2);
        if let Some([_, _, _, radius]) = guide {
            let diameter_squared = [(6, 9), (7, 10), (8, 11)]
                .into_iter()
                .map(|(lo, hi)| (f64::from(f32::from_bits(header[hi])) - f64::from(f32::from_bits(header[lo]))).powi(2))
                .sum::<f64>();
            log::info!("gpu-path guided photon emission optical radius {:.3} mm, scene radius {:.3} mm", radius, diameter_squared.sqrt() * 0.5);
        }
        let mut bytes = [0u8; HEADER_WORDS as usize * 4];
        for (word, output) in header.into_iter().zip(bytes.as_chunks_mut::<4>().0) {
            output.copy_from_slice(&word.to_le_bytes());
        }
        let pipelines = Pipelines::shared(context)?;
        let render = if clamp { pipelines.clamped_render(context)?.clone() } else { pipelines.render.clone() };
        let size = buffer_bytes(options)?;
        let buffer = checked_creation(&context.device, || {
            let buffer = context.device.create_buffer(&wgpu::BufferDescriptor {
                label: Some("caustic photons and spatial hash"),
                size,
                usage: wgpu::BufferUsages::STORAGE
                    | wgpu::BufferUsages::COPY_DST
                    | if cfg!(test) { wgpu::BufferUsages::COPY_SRC } else { wgpu::BufferUsages::empty() },
                mapped_at_creation: false,
            });
            context.queue.write_buffer(&buffer, 0, &bytes);
            if let Some(guide) = guide {
                let mut bytes = [0u8; GUIDE_WORDS as usize * 4];
                for (value, output) in guide.into_iter().zip(bytes.as_chunks_mut::<4>().0) {
                    output.copy_from_slice(&value.to_le_bytes());
                }
                context.queue.write_buffer(&buffer, size - u64::from(extra_words(options) + BOUNDS_WORDS) * 4, &bytes);
            }
            if let Some(sources) = sources {
                let mut words = [0u32; SOURCE_WORDS as usize];
                words[0] = sources.cumulative().len() as u32;
                for (target, value) in words[1..].iter_mut().zip(sources.cumulative()) {
                    *target = *value;
                }
                let mut packed = [0u8; SOURCE_WORDS as usize * 4];
                for (value, output) in words.into_iter().zip(packed.as_chunks_mut::<4>().0) {
                    output.copy_from_slice(&value.to_le_bytes());
                }
                context.queue.write_buffer(&buffer, size - u64::from(SOURCE_WORDS + BOUNDS_WORDS) * 4, &packed);
            }
            if let Some(bounds) = bounds {
                let mut bytes = [0u8; BOUNDS_WORDS as usize * 4];
                for (value, output) in bounds.into_iter().zip(bytes.as_chunks_mut::<4>().0) {
                    output.copy_from_slice(&value.to_le_bytes());
                }
                context.queue.write_buffer(&buffer, size - u64::from(BOUNDS_WORDS) * 4, &bytes);
            }
            Ok(buffer)
        })?;
        Ok(Self {
            buffer,
            layout: pipelines.layout.clone(),
            render,
            clear: pipelines.clear.clone(),
            emit: pipelines.emit.clone(),
            photons: options.photons,
            head_count,
            pass: 0,
            cleared: 0,
            emitted: 0,
            budget: Default::default(),
        })
    }
    /// Some native drivers specialize a pipeline at its first dispatch for the
    /// actual buffer bindings. The shared placeholder warm-up cannot establish
    /// that this job's admitted resources have completed that setup. Zero work
    /// warms every entry point under the existing setup timeout, without touching
    /// photon heads/records, camera film, or either production controller.
    pub(super) fn warm_up_actual(&self, context: &Context, uniform: &wgpu::Buffer, bind: &wgpu::BindGroup, cancel: &CancelToken) -> Result<()> {
        cancel.check()?;
        context.ensure_available()?;
        checked_creation(&context.device, || {
            context.queue.write_buffer(uniform, 0, &[0; PARAMETER_BYTES]);
            Ok(())
        })?;
        for pipeline in [&self.render, &self.clear, &self.emit] {
            cancel.check()?;
            warm_up(&context.device, &context.queue, pipeline, bind, &context.healthy)?;
            cancel.check()?;
        }
        Ok(())
    }
    pub(super) fn ready(&self) -> bool {
        self.emitted == self.photons
    }
    #[cfg(test)]
    pub(super) fn test_epoch(&self) -> u32 {
        self.pass
    }
    pub(super) fn ensure_pass(&mut self, pass: u32) -> Result<()> {
        if self.pass == pass {
            return Ok(());
        }
        if !self.ready() || self.pass.checked_add(1) != Some(pass) {
            return Err(gpu("caustic refresh requires the next complete camera pass"));
        }
        self.pass = pass;
        self.cleared = 0;
        self.emitted = 0;
        self.budget = Default::default();
        Ok(())
    }
    pub(super) fn count(&self) -> u32 {
        let remaining =
            if self.cleared < self.head_count + self.photons { self.head_count + self.photons - self.cleared } else { self.photons - self.emitted };
        self.budget.count(remaining)
    }
    pub(super) fn progress(&self) -> Progress {
        Progress { phase: Phase::Preparing, done: u64::from(self.cleared + self.emitted), total: u64::from(self.head_count + 2 * self.photons) }
    }
    pub(super) fn step(
        &mut self,
        context: &Context,
        uniform: &wgpu::Buffer,
        bind: &wgpu::BindGroup,
        parameters: &[u8],
        cancel: &CancelToken,
    ) -> Result<()> {
        cancel.check()?;
        context.ensure_available()?;
        if self.ready() {
            return Err(gpu("caustic prepass is already complete"));
        }
        let clearing = self.cleared < self.head_count + self.photons;
        let offset = if clearing { self.cleared } else { self.emitted };
        let count = self.count();
        let started = Instant::now();
        let scopes = scopes(&context.device);
        if clearing && offset == 0 && self.pass != 0 {
            context.queue.write_buffer(&self.buffer, 15 * 4, &self.pass.to_le_bytes());
        }
        context.queue.write_buffer(&self.buffer, 5 * 4, &offset.to_le_bytes());
        context.queue.write_buffer(uniform, 0, parameters);
        let mut encoder = context.device.create_command_encoder(&wgpu::CommandEncoderDescriptor { label: Some("caustic photon quantum") });
        {
            let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor { label: Some("caustic photon quantum"), timestamp_writes: None });
            pass.set_pipeline(if clearing { &self.clear } else { &self.emit });
            pass.set_bind_group(0, bind, &[]);
            pass.dispatch_workgroups(count.div_ceil(PATH_WORKGROUP_SIZE), 1, 1);
        }
        let submission = context.queue.submit([encoder.finish()]);
        checked(scopes)?;
        context.wait(submission, TIMEOUT)?;
        self.budget.observe(count, started.elapsed());
        if clearing {
            self.cleared += count;
            if self.cleared == self.head_count + self.photons {
                // Clearing is much cheaper than light paths; emission starts
                // with the original tiny measured controller, never its peak.
                self.budget = Default::default();
            }
        } else {
            self.emitted += count;
        }
        cancel.check()
    }
    #[cfg(test)]
    pub(super) fn maximum_quanta(&self) -> u32 {
        (self.head_count + 2 * self.photons).div_ceil(dispatch::MIN_PATHS) + 2
    }
    #[cfg(test)]
    pub(super) fn test_records(&self, context: &Context) -> Vec<[u32; RECORD_WORDS as usize]> {
        assert!(self.ready());
        let offset = u64::from(HEADER_WORDS + self.head_count) * 4;
        let bytes = u64::from(self.photons) * u64::from(RECORD_WORDS) * 4;
        let staging = context.device.create_buffer(&wgpu::BufferDescriptor {
            label: Some("test photon records"),
            size: bytes,
            usage: wgpu::BufferUsages::MAP_READ | wgpu::BufferUsages::COPY_DST,
            mapped_at_creation: false,
        });
        let mut encoder = context.device.create_command_encoder(&Default::default());
        encoder.copy_buffer_to_buffer(&self.buffer, offset, &staging, 0, bytes);
        let submission = context.queue.submit([encoder.finish()]);
        let slice = staging.slice(..);
        let (tx, rx) = std::sync::mpsc::channel();
        slice.map_async(wgpu::MapMode::Read, move |result| tx.send(result).unwrap());
        context.wait(submission, TIMEOUT).unwrap();
        rx.try_recv().unwrap().unwrap();
        let mapped = slice.get_mapped_range().unwrap();
        let records = mapped
            .as_chunks::<64>()
            .0
            .iter()
            .map(|record| {
                let mut words = std::array::from_fn(|i| u32::from_le_bytes(record[i * 4..i * 4 + 4].try_into().unwrap()));
                if words[11] == 0 {
                    return [0; RECORD_WORDS as usize];
                }
                // Atomic linked-list insertion order is independent of the
                // indexed emitted paths and may vary on the same driver.
                words[3] = 0;
                words
            })
            .collect();
        drop(mapped);
        staging.unmap();
        records
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn photon_allocation_counts_exact_header_hash_records_and_upload() {
        assert_eq!(admission_bytes(None).unwrap(), 0);
        for photons in [1, 17, 16384, 65536] {
            let options = CausticOptions { photons, ..Default::default() };
            let head_count = heads(options).unwrap();
            assert!(head_count.is_power_of_two() && head_count >= 2 * photons);
            assert_eq!(buffer_bytes(options).unwrap(), 88 + u64::from(head_count) * 4 + u64::from(photons) * 64);
            assert_eq!(admission_bytes(Some(options)).unwrap(), buffer_bytes(options).unwrap() + 88);
            let guided = CausticOptions { guided_emission: true, ..options };
            assert_eq!(buffer_bytes(guided).unwrap(), buffer_bytes(options).unwrap() + 16);
            assert_eq!(admission_bytes(Some(guided)).unwrap(), admission_bytes(Some(options)).unwrap() + 32);
        }
        assert!(buffer_bytes(CausticOptions { photons: u32::MAX, ..Default::default() }).is_err());
    }
    #[test]
    fn photon_shader_variant_preserves_the_legacy_six_buffer_source() {
        assert!(!SHADER.contains("@binding(7)"));
        let caustic = shader_source().unwrap();
        assert!(caustic.contains("@binding(7)"));
        assert!(caustic.contains("fn photon_emit") && caustic.contains("fn photon_clear"));
        naga::front::wgsl::parse_str(&caustic).unwrap();
    }
}
