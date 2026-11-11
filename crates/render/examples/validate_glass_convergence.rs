//! Reproducible public-safe glass convergence measurements.
//! The output contains raw masters, display images and independent-seed statistics.
//! A high-sample run with this estimator is not an independent unbiased reference.
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_geom::{Aabb3, Mesh, MeshGeom};
use solidcraft_render::{
    ProductMesh, ProductScene,
    job::{
        CancelToken, CausticOptions, CpuBudget, CpuPathOptions, CpuProgressiveOptions, DielectricBoundary, DielectricOptions,
        DirectionalLightOptions, ImageRequest, Integrator, OutputOptions, RaytraceEnvironmentOptions, RaytraceOptions,
    },
    raytrace::image::{ImagePoll, ImageTask},
    transmission::Transmission,
};
use solidcraft_viewport::{Camera, StandardView, surface::OpaqueSurface};
use std::{
    error::Error,
    fs,
    path::{Path, PathBuf},
    sync::Arc,
    time::Instant,
};

#[path = "glass_evaluation/strategies.rs"]
mod strategies;

type Result<T> = std::result::Result<T, Box<dyn Error>>;

fn glass_sphere() -> Mesh {
    let rows = 16u32;
    let columns = 32u32;
    let center = Vec3::new(0.0, 0.0, 3.0);
    let mut positions = vec![center + Vec3::Z * 2.0];
    let mut normals = vec![Vec3::Z];
    for ring in 1..rows {
        let theta = f64::from(ring) * std::f64::consts::PI / f64::from(rows);
        for column in 0..columns {
            let phi = f64::from(column) * std::f64::consts::TAU / f64::from(columns);
            let normal = Vec3::new(theta.sin() * phi.cos(), theta.sin() * phi.sin(), theta.cos());
            positions.push(center + normal * 2.0);
            normals.push(normal);
        }
    }
    let bottom = 1 + (rows - 1) * columns;
    positions.push(center - Vec3::Z * 2.0);
    normals.push(-Vec3::Z);
    let mut triangles = Vec::new();
    for column in 0..columns {
        triangles.push([0, 1 + column, 1 + (column + 1) % columns]);
        for ring in 0..rows - 2 {
            let a = 1 + ring * columns + column;
            let b = 1 + ring * columns + (column + 1) % columns;
            triangles.extend([[a, a + columns, b], [b, a + columns, b + columns]]);
        }
        triangles.push([1 + (rows - 2) * columns + column, bottom, 1 + (rows - 2) * columns + (column + 1) % columns]);
    }
    Mesh { positions, normals, tri_face: vec![0; triangles.len()], triangles, ..Default::default() }
}

fn scene(polished: bool) -> Result<ProductScene> {
    let glass = ProductMesh {
        name: "Original convergence glass sphere".into(),
        mesh: Arc::new(glass_sphere()),
        color: [255; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::ZERO, 0.0, 0.1, 1.5)?),
        transmission: Some(Transmission::new(Vec3::ZERO, None)?),
        faces: Vec::new(),
    };
    let floor = Mesh {
        positions: vec![Vec3::new(-8.0, -8.0, 0.0), Vec3::new(8.0, -8.0, 0.0), Vec3::new(8.0, 8.0, 0.0), Vec3::new(-8.0, 8.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    };
    let receiver = ProductMesh {
        name: if polished { "Original polished receiver" } else { "Original diffuse receiver" }.into(),
        mesh: Arc::new(floor),
        color: [235; 3],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(Vec3::new(0.8, 0.8, 0.8), if polished { 1.0 } else { 0.0 }, if polished { 0.045 } else { 0.5 }, 1.0)?),
        transmission: None,
        faces: Vec::new(),
    };
    let meshes = vec![glass, receiver];
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    Ok(ProductScene { meshes, bounds })
}

fn request(scene: &ProductScene, size: u32, samples: u32, photons: u32, seed: u32, progressive: bool) -> Result<ImageRequest> {
    let mut dielectrics = Vec::new();
    for (body_index, body) in scene.meshes.iter().enumerate() {
        if let Some(optics) = body.transmission {
            let surface = body.surface.ok_or("benchmark optical body has no IOR")?;
            let absorption = optics.extinction();
            dielectrics.push(DielectricOptions {
                body_index,
                body_name: body.name.clone(),
                boundary: if optics.thin_wall_thickness_mm().is_some() { DielectricBoundary::ThinSheet } else { DielectricBoundary::ClosedMesh },
                ior: surface.dielectric_ior(),
                absorption_per_mm: [absorption.x, absorption.y, absorption.z],
                thickness_mm: optics.thin_wall_thickness_mm().unwrap_or(0.0),
                model: optics.model(),
                subsurface: optics.subsurface(),
                shadow_transmission: optics.shadow_transmission(),
            });
        }
    }
    let options = RaytraceOptions {
        threads: 1,
        budget: CpuBudget::Studio,
        path: Some(CpuPathOptions { max_depth: 16, ..Default::default() }),
        progressive: Some(CpuProgressiveOptions { samples_per_pixel: samples, seed: 0x1234_0000 + seed }),
        radiance_components: true,
        dielectrics,
        lights: Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]),
        environment: Some(RaytraceEnvironmentOptions { color: [0.0; 3], intensity: 0.0 }),
        shadows: true,
        caustics: Some(CausticOptions {
            photons,
            radius_mm: 1.0,
            max_depth: 16,
            seed: 0x4567_0000 + seed,
            guided_emission: true,
            refresh_per_pass: true,
            progressive_radius: progressive,
            // The optical axis on the floor projects near (0.5, 0.56) in this camera.
            probe_pixel: Some([size / 2, 9 * size / 16]),
            ..Default::default()
        }),
        output: Some(Box::new(OutputOptions { linear_exr: true, ..Default::default() })),
        ..Default::default()
    };
    let mut camera = Camera { target: Vec3::new(0.0, 0.0, 1.0), distance: 18.0, fov: 0.65, ..Default::default() };
    camera.set_view(StandardView::Front);
    camera.pitch = 0.72;
    camera.yaw = 0.25;
    Ok(ImageRequest {
        studio_weights: None,
        environment: None,
        textures: None,
        integrator: Integrator::Raytrace,
        raytrace: Some(options),
        camera,
        width: size,
        height: size,
        background: [18, 22, 28],
        transparent_background: false,
    })
}

fn word(bytes: &[u8], at: usize) -> Result<u32> {
    let value = bytes.get(at..at + 4).and_then(|v| v.try_into().ok()).ok_or("truncated native EXR word")?;
    Ok(u32::from_le_bytes(value))
}
fn terminated<'a>(bytes: &'a [u8], cursor: &mut usize) -> Result<&'a [u8]> {
    let tail = bytes.get(*cursor..).ok_or("truncated native EXR header")?;
    let length = tail.iter().position(|value| *value == 0).ok_or("unterminated native EXR attribute")?;
    let value = tail.get(..length).ok_or("truncated native EXR name")?;
    *cursor = cursor.checked_add(length + 1).ok_or("native EXR header overflow")?;
    Ok(value)
}

/// Read this renderer's documented uncompressed FLOAT A/B/G/R master layout.
/// This is a small validation reader, not an importer for arbitrary OpenEXR files.
fn pixels(bytes: &[u8], size: u32) -> Result<Vec<[f64; 3]>> {
    if word(bytes, 0)? != 20_000_630 || word(bytes, 4)? != 2 {
        return Err("native EXR is not the expected single-part scanline file".into());
    }
    let mut cursor = 8usize;
    loop {
        let name = terminated(bytes, &mut cursor)?;
        if name.is_empty() {
            break;
        }
        let _kind = terminated(bytes, &mut cursor)?;
        let length = word(bytes, cursor)? as usize;
        cursor = cursor.checked_add(4).and_then(|v| v.checked_add(length)).filter(|v| *v <= bytes.len()).ok_or("native EXR attribute overflow")?;
    }
    let mut result = vec![[0.0; 3]; (size * size) as usize];
    for row in 0..size as usize {
        let table = cursor + row * 8;
        let offset_bytes = bytes.get(table..table + 8).and_then(|v| v.try_into().ok()).ok_or("native EXR offset table truncated")?;
        let offset = usize::try_from(u64::from_le_bytes(offset_bytes))?;
        let y = word(bytes, offset)? as usize;
        if y >= size as usize || word(bytes, offset + 4)? != size * 16 {
            return Err("native EXR row extent mismatch".into());
        }
        for x in 0..size as usize {
            let at = offset + 8;
            let mut rgb = [0.0; 3];
            for (channel, source) in rgb.iter_mut().zip([3usize, 2, 1]) {
                *channel = f64::from(f32::from_bits(word(bytes, at + (source * size as usize + x) * 4)?));
                if !channel.is_finite() || *channel < 0.0 {
                    return Err("nonfinite or negative native raw radiance".into());
                }
            }
            *result.get_mut(y * size as usize + x).ok_or("native EXR pixel out of range")? = rgb;
        }
    }
    Ok(result)
}
fn luminance(rgb: [f64; 3]) -> f64 {
    rgb[0] * 0.2126 + rgb[1] * 0.7152 + rgb[2] * 0.0722
}
fn quantile(sorted: &[f64], q: f64) -> Result<f64> {
    let at = ((sorted.len().saturating_sub(1)) as f64 * q).round() as usize;
    sorted.get(at).copied().ok_or_else(|| "empty convergence statistics".into())
}
fn statistics(image: &[[f64; 3]]) -> Result<Value> {
    let mut values: Vec<_> = image.iter().copied().map(luminance).collect();
    values.sort_by(f64::total_cmp);
    let median = quantile(&values, 0.5)?;
    let outlier_threshold = (median * 100.0).max(10.0);
    Ok(json!({
        "radiance_sum": values.iter().sum::<f64>(), "mean_radiance": values.iter().sum::<f64>() / values.len() as f64,
        "p50": median, "p90": quantile(&values, 0.90)?, "p99": quantile(&values, 0.99)?, "maximum": quantile(&values, 1.0)?,
        "outlier_threshold": outlier_threshold, "pixels_above_threshold": values.iter().filter(|v| **v > outlier_threshold).count(),
        "outlier_count_is_not_a_brightness_clamp": true
    }))
}

fn execute_render(out: &Path, name: &str, mut source: ProductScene, request: ImageRequest, labels: Value) -> Result<(Value, Vec<[f64; 3]>)> {
    if request.width != request.height {
        return Err("benchmark expects a square trace grid".into());
    }
    let size = request.width;
    let options = request.raytrace.clone().ok_or("benchmark options disappeared")?;
    let photon_options = options.caustics.ok_or("benchmark photon options disappeared")?;
    let camera_samples = options.progressive.ok_or("benchmark production options disappeared")?.samples_per_pixel;
    let last_epoch = if photon_options.refresh_per_pass { camera_samples.checked_sub(1).ok_or("benchmark has no camera samples")? } else { 0 };
    let last_radii_mm = photon_options.radii_mm(last_epoch)?;
    let requested_maps = if photon_options.refresh_per_pass { camera_samples } else { 1 };
    let settings = serde_json::to_value(&options)?;
    // CPU optics are captured from the authored fixture in the recorded request.
    // The CPU capture meshes themselves carry opaque geometry, as in glass_comparison.
    for body in &mut source.meshes {
        body.transmission = None;
    }
    // Record the conservative preflight estimate separately from process RSS.
    // ImageTask still performs its ordinary setup inside the timed interval.
    let admission = solidcraft_render::raytrace::image::admit(&source, &request, &options)?;
    let started = Instant::now();
    let mut task = ImageTask::new(source, request, CancelToken::default(), options)?;
    let png = loop {
        match task.step_with_previews(16_384)? {
            ImagePoll::Complete(png) => break png,
            ImagePoll::Pending(_) | ImagePoll::Preview { .. } => {}
        }
    };
    let elapsed = started.elapsed().as_secs_f64();
    let raw = task.take_linear_exr().ok_or("completed convergence render has no raw master")?;
    let image = pixels(&raw, size)?;
    fs::write(out.join(format!("{name}.png")), png)?;
    fs::write(out.join(format!("{name}.exr")), raw)?;
    let mut record = json!({
        "name":name, "photons_per_map":photon_options.photons,
        "emitted_paths":u64::from(requested_maps)*u64::from(photon_options.photons),
        "elapsed_seconds":elapsed, "settings":settings,
        "admission":admission, "admission_is_measured_peak_rss":false,
        "pixel_statistics":statistics(&image)?, "photon_probe":task.photon_diagnostics(),
        "components":task.radiance_report(), "sampling":task.sampling_report(), "last_radii_mm":last_radii_mm
    });
    let extra = labels.as_object().ok_or("benchmark labels must be an object")?;
    record.as_object_mut().ok_or("benchmark record disappeared")?.extend(extra.clone());
    fs::write(out.join(format!("{name}.json")), serde_json::to_vec_pretty(&record)?)?;
    eprintln!("completed {name} in {elapsed:.3}s");
    Ok((record, image))
}

fn render(out: &Path, size: u32, samples: u32, photons: u32, seed: u32, progressive: bool, polished: bool) -> Result<(Value, Vec<[f64; 3]>)> {
    let case = if polished { "polished" } else { "diffuse" };
    let mode = if progressive { "progressive" } else { "fixed" };
    let name = format!("{case}-{mode}-spp{samples:03}-seed{seed:02}");
    let source = scene(polished)?;
    let request = request(&source, size, samples, photons, seed, progressive)?;
    execute_render(out, &name, source, request, json!({"case":case,"mode":mode,"seed":seed,"samples":samples}))
}

#[derive(Clone, Copy)]
struct SamplingVariant {
    name: &'static str,
    sampler: solidcraft_render::sampling::Sampler,
    strata: u32,
    roulette: bool,
}

const SAMPLING_VARIANTS: [SamplingVariant; 7] = [
    SamplingVariant { name: "independent", sampler: solidcraft_render::sampling::Sampler::Independent, strata: 1, roulette: false },
    SamplingVariant { name: "sobol", sampler: solidcraft_render::sampling::Sampler::ScrambledSobol, strata: 1, roulette: false },
    SamplingVariant { name: "strata2", sampler: solidcraft_render::sampling::Sampler::Independent, strata: 2, roulette: false },
    SamplingVariant { name: "strata4", sampler: solidcraft_render::sampling::Sampler::Independent, strata: 4, roulette: false },
    SamplingVariant { name: "sobol-strata4", sampler: solidcraft_render::sampling::Sampler::ScrambledSobol, strata: 4, roulette: false },
    SamplingVariant { name: "roulette", sampler: solidcraft_render::sampling::Sampler::Independent, strata: 1, roulette: true },
    SamplingVariant { name: "sobol-strata4-roulette", sampler: solidcraft_render::sampling::Sampler::ScrambledSobol, strata: 4, roulette: true },
];

#[derive(Clone, Copy)]
struct SamplingScene {
    name: &'static str,
    polished: bool,
    rough: bool,
    dispersive: bool,
}

const SAMPLING_SCENES: [SamplingScene; 4] = [
    SamplingScene { name: "clear-diffuse", polished: false, rough: false, dispersive: false },
    SamplingScene { name: "clear-polished", polished: true, rough: false, dispersive: false },
    SamplingScene { name: "rough-diffuse", polished: false, rough: true, dispersive: false },
    SamplingScene { name: "dispersive-diffuse", polished: false, rough: false, dispersive: true },
];

fn sampling_run(
    out: &Path,
    case: SamplingScene,
    variant: SamplingVariant,
    size: u32,
    samples: u32,
    photons: u32,
    seed: u32,
    calibration: bool,
) -> Result<(Value, Vec<[f64; 3]>)> {
    let mut source = scene(case.polished)?;
    if case.rough || case.dispersive {
        let glass = source.meshes.first_mut().ok_or("sampling fixture has no glass")?;
        let optics = glass.transmission.ok_or("sampling fixture has no optical material")?;
        glass.transmission = Some(optics.with_shadow_transmission(false).with_model(solidcraft_render::glass_model::GlassModel {
            roughness: if case.rough { 0.2 } else { 0.0 },
            dispersion: case.dispersive.then_some(solidcraft_render::glass_model::Dispersion { abbe_number: 30.0 }),
            ..Default::default()
        })?);
    }
    let mut request = request(&source, size, samples, photons, seed, false)?;
    let options = request.raytrace.as_mut().ok_or("sampling request options disappeared")?;
    options.sampler = variant.sampler;
    options.radiance_components = false;
    let path = options.path.as_mut().ok_or("sampling request has no CPU path integrator")?;
    path.transmission_samples = variant.strata;
    path.russian_roulette = variant.roulette;
    let caustics = options.caustics.as_mut().ok_or("sampling request has no photons")?;
    caustics.probe_pixel = None;
    let phase = if calibration { "calibration" } else { "measurement" };
    let name = format!("{}-{}-{phase}-spp{samples:04}-seed{seed:08x}", case.name, variant.name);
    execute_render(
        out,
        &name,
        source,
        request,
        json!({
            "case":case.name, "variant":variant.name, "phase":phase, "seed":seed, "samples":samples,
            "sampler":variant.sampler, "fresnel_strata":variant.strata, "roulette":variant.roulette,
            "fixed_radius":true, "independent_fresh_maps":true
        }),
    )
}

struct SeedSeries {
    count: u32,
    mean: Vec<[f64; 3]>,
    m2: Vec<[f64; 3]>,
    luma_mean: Vec<f64>,
    luma_m2: Vec<f64>,
    seconds: Vec<f64>,
    integrated: Vec<f64>,
}

impl SeedSeries {
    fn new(pixels: usize) -> Self {
        Self {
            count: 0,
            mean: vec![[0.0; 3]; pixels],
            m2: vec![[0.0; 3]; pixels],
            luma_mean: vec![0.0; pixels],
            luma_m2: vec![0.0; pixels],
            seconds: Vec::new(),
            integrated: Vec::new(),
        }
    }

    fn observe(&mut self, image: &[[f64; 3]], seconds: f64) -> Result<()> {
        if image.len() != self.mean.len() || !seconds.is_finite() || seconds <= 0.0 {
            return Err("sampling measurement has an invalid extent or elapsed time".into());
        }
        self.count += 1;
        let n = f64::from(self.count);
        let mut sum = 0.0;
        for (index, rgb) in image.iter().copied().enumerate() {
            for (axis, value) in rgb.into_iter().enumerate() {
                let delta = value - self.mean[index][axis];
                self.mean[index][axis] += delta / n;
                self.m2[index][axis] += delta * (value - self.mean[index][axis]);
            }
            let value = luminance(rgb);
            let delta = value - self.luma_mean[index];
            self.luma_mean[index] += delta / n;
            self.luma_m2[index] += delta * (value - self.luma_mean[index]);
            sum += value;
        }
        self.seconds.push(seconds);
        self.integrated.push(sum);
        Ok(())
    }

    fn record(&self, case: &str, variant: SamplingVariant, samples: u32) -> Result<Value> {
        let mut record = self.record_group(case, variant.name, samples, variant.strata)?;
        record["fresnel_strata"] = json!(variant.strata);
        Ok(record)
    }

    fn record_group(&self, case: &str, variant: &str, samples: u32, production_lanes: u32) -> Result<Value> {
        if self.count < 2 {
            return Err("sampling comparison needs independent seeds".into());
        }
        let pixels = self.mean.len() as f64;
        let variance = self.luma_m2.iter().sum::<f64>() / f64::from(self.count - 1) / pixels;
        let rgb_variance = self.m2.iter().flat_map(|pixel| pixel.iter()).sum::<f64>() / f64::from(self.count - 1) / pixels / 3.0;
        let seconds = self.seconds.iter().sum::<f64>() / f64::from(self.count);
        Ok(json!({
            "case":case,"variant":variant,"samples":samples,"seeds":self.count,
            "requested_camera_lane_evaluations_per_pixel":u64::from(samples)*u64::from(production_lanes),
            "mean_pixel_luminance_variance_across_independent_seeds":variance,
            "mean_rgb_channel_variance_across_independent_seeds":rgb_variance,
            "elapsed_seconds_by_seed":self.seconds,"mean_elapsed_seconds":seconds,
            "luminance_variance_times_mean_seconds":variance*seconds,"rgb_variance_times_mean_seconds":rgb_variance*seconds,
            "integrated_luminance_by_seed":self.integrated
        }))
    }
}

/// Timings alone select production sample counts. Calibration uses disjoint
/// camera/map seeds; calibration radiance never enters budgets or measurements.
fn sampling_comparison(out: &Path, quality: bool) -> Result<()> {
    let profile = if quality { "sampling-quality" } else { "sampling-smoke" };
    let size = if quality { 32 } else { 16 };
    let seeds = if quality { 8 } else { 2 };
    let photons = if quality { 2048 } else { 512 };
    let target_seconds = if quality { Some(4.0) } else { None };
    let mut calibration_records = Vec::new();
    let mut calibrations = Vec::new();
    let mut records = Vec::new();
    let mut groups = Vec::new();
    for (case_index, case) in SAMPLING_SCENES.into_iter().enumerate() {
        let mut budgets = [4u32; SAMPLING_VARIANTS.len()];
        if let Some(target) = target_seconds {
            for (index, variant) in SAMPLING_VARIANTS.into_iter().enumerate() {
                let mut times = [0.0; 2];
                for (step, samples) in [2u32, 8].into_iter().enumerate() {
                    let seed = 0x0100_0000 + (case_index as u32) * 256 + (index as u32) * 2 + step as u32;
                    let (record, _) = sampling_run(out, case, variant, size, samples, photons, seed, true)?;
                    times[step] = record["elapsed_seconds"].as_f64().ok_or("calibration time missing")?;
                    calibration_records.push(record);
                    fs::write(out.join("calibration.json"), serde_json::to_vec_pretty(&calibration_records)?)?;
                }
                let slope = (times[1] - times[0]) / 6.0;
                if !slope.is_finite() || slope <= 0.0 {
                    return Err("nonmonotonic timing calibration: repeat on a stable machine; no equal-time claim is available".into());
                }
                let setup = (times[0] - 2.0 * slope).max(0.0);
                budgets[index] = ((target - setup).max(0.0) / slope).floor().clamp(2.0, 512.0) as u32;
                calibrations.push(json!({
                    "case":case.name,"variant":variant.name,"seconds_at_2_and_8_samples":times,
                    "seconds_per_sample":slope,"nonnegative_setup_seconds":setup,"target_seconds":target,
                    "selected_samples":budgets[index],"predicted_seconds":setup+slope*f64::from(budgets[index]),
                    "at_lower_budget_bound":budgets[index]==2,"at_upper_budget_bound":budgets[index]==512
                }));
                fs::write(out.join("calibration-models.json"), serde_json::to_vec_pretty(&calibrations)?)?;
            }
        }
        let mut series: Vec<_> = SAMPLING_VARIANTS.iter().map(|_| SeedSeries::new((size * size) as usize)).collect();
        // Rotate ordering across seeds so one option does not always run first.
        // All budgets are fixed before any production radiance is inspected.
        for seed in 0..seeds {
            for position in 0..SAMPLING_VARIANTS.len() {
                let index = (position + seed as usize) % SAMPLING_VARIANTS.len();
                let variant = SAMPLING_VARIANTS[index];
                let (record, image) = sampling_run(out, case, variant, size, budgets[index], photons, seed, false)?;
                let seconds = record["elapsed_seconds"].as_f64().ok_or("measurement time missing")?;
                series[index].observe(&image, seconds)?;
                records.push(record);
                fs::write(out.join("runs.json"), serde_json::to_vec_pretty(&records)?)?;
            }
        }
        let mut case_groups = Vec::new();
        for (index, variant) in SAMPLING_VARIANTS.into_iter().enumerate() {
            case_groups.push(series[index].record(case.name, variant, budgets[index])?);
        }
        let baseline_seconds = case_groups[0]["mean_elapsed_seconds"].as_f64().ok_or("baseline time missing")?;
        let baseline_efficiency = case_groups[0]["luminance_variance_times_mean_seconds"].as_f64().ok_or("baseline variance missing")?;
        for group in &mut case_groups {
            let seconds = group["mean_elapsed_seconds"].as_f64().ok_or("group time missing")?;
            let ratio = seconds / baseline_seconds;
            let efficiency = group["luminance_variance_times_mean_seconds"].as_f64().ok_or("group variance missing")?;
            group["elapsed_ratio_to_independent"] = json!(ratio);
            group["within_factor_1_25_of_baseline_elapsed"] = json!((0.8..=1.25).contains(&ratio));
            group["elapsed_ratio_to_target"] = json!(target_seconds.map(|target| seconds / target));
            group["variance_time_ratio_to_independent"] = json!((baseline_efficiency > 0.0).then_some(efficiency / baseline_efficiency));
        }
        groups.extend(case_groups);
        fs::write(out.join("groups.json"), serde_json::to_vec_pretty(&groups)?)?;
    }
    let report = json!({
        "profile":profile,"dimensions":[size,size],"seeds":seeds,"threads":1,"photons_per_refreshed_map":photons,
        "target_seconds_per_render":target_seconds,"calibration_sample_counts":if quality {vec![2,8]} else {vec![]},
        "budget_bounds":if quality {[2,512]} else {[4,4]},
        "rustc":command_output("rustc",&["--version"]),"git_head":command_output("git",&["rev-parse","HEAD"]),
        "working_tree_status":command_output("git",&["status","--short"]),
        "scope":"CPU sampling comparison in four original glass scenes with seven configurations and independent camera/map seeds",
        "limitations":[
            "Seed variance measures repeatability, not photon-radius bias or error against an independent reference.",
            "Variance multiplied by elapsed time is an efficiency proxy; it is not a calibrated confidence bound.",
            if quality {"Each configuration has its own timing-calibrated fixed budget; actual elapsed times and mismatches remain reported."}
            else {"The smoke profile uses four camera observations per configuration without timing calibration."},
            "Setup and output encoding are timed. Shared-machine load and CI-profile optimization limit throughput interpretation.",
            "Two or eight randomizations do not establish rare-event tails or universal quality improvement.",
            "The same finite-depth transport domain and fixed photon radius are used in every configuration.",
            "Camera/map randomizations are independent between replicates and paired by replicate across configurations.",
            "This experiment does not compare deterministic branch splitting or qualify GPU throughput."
        ],
        "calibration_models":calibrations,"calibration_runs":calibration_records,"groups":groups,"runs":records
    });
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&json!({"output":out,"profile":profile,"groups":groups}))?);
    Ok(())
}

fn command_output(program: &str, args: &[&str]) -> Option<String> {
    std::process::Command::new(program)
        .args(args)
        .output()
        .ok()
        .filter(|out| out.status.success())
        .and_then(|out| String::from_utf8(out.stdout).ok())
        .map(|s| s.trim().to_owned())
}

fn main() -> Result<()> {
    let out = std::env::args_os()
        .nth(1)
        .map(PathBuf::from)
        .ok_or("usage: validate_glass_convergence NEW_OUTPUT_DIRECTORY [smoke|quality|sampling-smoke|sampling-quality|receiver-smoke|receiver-quality|split-smoke|split-quality]")?;
    let profile = std::env::args().nth(2).unwrap_or_else(|| "smoke".into());
    if strategies::is_profile(&profile) {
        fs::create_dir(&out)?;
        return strategies::run(&out, &profile);
    }
    if matches!(profile.as_str(), "sampling-smoke" | "sampling-quality") {
        fs::create_dir(&out)?;
        return sampling_comparison(&out, profile == "sampling-quality");
    }
    let (size, seeds, budgets, photons) =
        match profile.as_str() {
            "smoke" => (16, 2, vec![4, 16], 1024),
            "quality" => (64, 4, vec![8, 16, 64], 16384),
            _ => return Err(
                "profile must be smoke, quality, sampling-smoke, sampling-quality, receiver-smoke, receiver-quality, split-smoke or split-quality"
                    .into(),
            ),
        };
    fs::create_dir(&out)?;
    let mut records = Vec::new();
    let mut groups = Vec::new();
    for polished in [false, true] {
        for samples in budgets.iter().copied() {
            for progressive in [false, true] {
                let mut means = vec![0.0; (size * size) as usize];
                let mut m2 = means.clone();
                let mut elapsed = 0.0;
                let mut integrated = Vec::new();
                for seed in 0..seeds {
                    let (record, image) = render(&out, size, samples, photons, seed, progressive, polished)?;
                    elapsed += record.get("elapsed_seconds").and_then(Value::as_f64).ok_or("missing render time")?;
                    let mut sum = 0.0;
                    for ((mean, m2), rgb) in means.iter_mut().zip(m2.iter_mut()).zip(image) {
                        let value = luminance(rgb);
                        let delta = value - *mean;
                        *mean += delta / f64::from(seed + 1);
                        *m2 += delta * (value - *mean);
                        sum += value;
                    }
                    integrated.push(sum);
                    records.push(record);
                    fs::write(out.join("runs.json"), serde_json::to_vec_pretty(&records)?)?;
                }
                let variance = m2.iter().sum::<f64>() / f64::from(seeds - 1) / means.len() as f64;
                groups.push(json!({
                    "case":if polished {"polished"} else {"diffuse"}, "mode":if progressive {"progressive"} else {"fixed"},
                    "samples":samples,"seeds":seeds,"mean_pixel_variance_across_independent_seeds":variance,
                    "elapsed_seconds_total":elapsed,"mean_elapsed_seconds":elapsed/f64::from(seeds),
                    "variance_times_mean_seconds":variance*elapsed/f64::from(seeds),"integrated_radiance_by_seed":integrated
                }));
            }
        }
    }
    let report = json!({
        "profile":profile,"dimensions":[size,size],"threads":1,"architecture":std::env::consts::ARCH,"os":std::env::consts::OS,
        "rustc":command_output("rustc",&["--version"]),"git_head":command_output("git",&["rev-parse","HEAD"]),
        "modified_files":command_output("git",&["diff","--name-only"]),
        "working_tree_status":command_output("git",&["status","--short"]),
        "scope":"Synthetic glass sphere over diffuse and polished receivers; independent camera and photon seeds; raw linear statistics",
        "limitations":[
            "No independently converged image reference is provided; seed variance is not reconstruction bias.",
            "The source sphere is an explicitly triangulated fixture; geometric faceting is retained.",
            "Timings include setup and output encoding; shared-machine load is not controlled by this example.",
            "Progressive radii can increase single-map variance and do not guarantee finite-budget improvement.",
            "Probe reports distinguish receiver-local and camera-weighted contributions."
        ],
        "groups":groups,"runs":records
    });
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!("{}", serde_json::to_string_pretty(&json!({"output":out,"groups":groups,"profile":profile}))?);
    Ok(())
}
