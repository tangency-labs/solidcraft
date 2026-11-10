//! Bounded comparisons for continuous-source receiver ownership and one split.
//! All cases are original public fixtures. Raw outputs and timings are retained;
//! seed scatter is not an independent reference for finite-radius transport bias.
use super::{Result, SeedSeries, command_output, execute_render, request, scene};
use serde_json::{Value, json};
use solidcore_record::Vec3;
use solidcraft_render::{
    ProductScene,
    glass_model::{Dispersion, GlassModel},
    job::{ImageRequest, RaytraceEnvironmentOptions, RectangleLightOptions},
    sampling::Sampler,
};
use solidcraft_viewport::surface::OpaqueSurface;
use std::{fs, path::Path};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Source {
    Rectangle,
    Environment,
    Directional,
}

impl Source {
    fn name(self) -> &'static str {
        match self {
            Self::Rectangle => "rectangle",
            Self::Environment => "environment",
            Self::Directional => "directional",
        }
    }
}

#[derive(Clone, Copy)]
struct Case {
    name: &'static str,
    source: Source,
    roughness: f64,
    metallic: f64,
    dispersive: bool,
}

const RECEIVER_CASES: &[Case] = &[
    Case { name: "area-diffuse-control", source: Source::Rectangle, roughness: 0.65, metallic: 0.0, dispersive: false },
    Case { name: "area-glossy", source: Source::Rectangle, roughness: 0.32, metallic: 0.0, dispersive: false },
    Case { name: "area-polished", source: Source::Rectangle, roughness: 0.06, metallic: 1.0, dispersive: false },
    Case { name: "environment-glossy", source: Source::Environment, roughness: 0.32, metallic: 0.0, dispersive: false },
    Case { name: "environment-polished", source: Source::Environment, roughness: 0.06, metallic: 1.0, dispersive: false },
    Case { name: "hard-source-control", source: Source::Directional, roughness: 0.32, metallic: 0.0, dispersive: false },
];

const SPLIT_CASES: &[Case] = &[
    Case { name: "area-diffuse", source: Source::Rectangle, roughness: 0.5, metallic: 0.0, dispersive: false },
    Case { name: "area-glossy", source: Source::Rectangle, roughness: 0.32, metallic: 1.0, dispersive: false },
    Case { name: "environment-polished", source: Source::Environment, roughness: 0.06, metallic: 1.0, dispersive: false },
    Case { name: "dispersive-area", source: Source::Rectangle, roughness: 0.32, metallic: 1.0, dispersive: true },
];

#[derive(Clone, Copy)]
enum Receiver {
    Gather,
    Continue,
    CameraReference,
}

#[derive(Clone, Copy)]
struct Variant {
    name: &'static str,
    sampler: Sampler,
    strata: u32,
    split: bool,
    receiver: Receiver,
}

impl Variant {
    fn lanes(self) -> u32 {
        if self.split { 2 } else { self.strata }
    }
}

const RECEIVER_VARIANTS: &[Variant] = &[
    Variant { name: "gather", sampler: Sampler::Independent, strata: 1, split: false, receiver: Receiver::Gather },
    Variant { name: "continue-glossy", sampler: Sampler::Independent, strata: 1, split: false, receiver: Receiver::Continue },
    Variant { name: "camera-reference", sampler: Sampler::Independent, strata: 1, split: false, receiver: Receiver::CameraReference },
];

const SPLIT_VARIANTS: &[Variant] = &[
    Variant { name: "independent", sampler: Sampler::Independent, strata: 1, split: false, receiver: Receiver::CameraReference },
    Variant { name: "sobol", sampler: Sampler::ScrambledSobol, strata: 1, split: false, receiver: Receiver::CameraReference },
    Variant { name: "strata2", sampler: Sampler::Independent, strata: 2, split: false, receiver: Receiver::CameraReference },
    Variant { name: "strata4", sampler: Sampler::Independent, strata: 4, split: false, receiver: Receiver::CameraReference },
    Variant { name: "split", sampler: Sampler::Independent, strata: 1, split: true, receiver: Receiver::CameraReference },
    Variant { name: "sobol-split", sampler: Sampler::ScrambledSobol, strata: 1, split: true, receiver: Receiver::CameraReference },
];

const GLOSSY_THRESHOLD: f64 = 0.35;

pub(super) fn is_profile(profile: &str) -> bool {
    matches!(profile, "receiver-smoke" | "receiver-quality" | "split-smoke" | "split-quality")
}

fn fixture(case: Case) -> Result<ProductScene> {
    let mut source = scene(false)?;
    let receiver = source.meshes.get_mut(1).ok_or("strategy fixture has no receiver")?;
    receiver.name = format!("Original {} receiver", case.name);
    receiver.surface = Some(OpaqueSurface::new(Vec3::new(0.8, 0.8, 0.8), case.metallic, case.roughness, 1.5)?);
    let glass = source.meshes.first_mut().ok_or("strategy fixture has no glass")?;
    let optics = glass.transmission.ok_or("strategy fixture has no transmission")?.with_shadow_transmission(false);
    glass.transmission = Some(if case.dispersive {
        optics.with_model(GlassModel { dispersion: Some(Dispersion { abbe_number: 30.0 }), ..Default::default() })?
    } else {
        optics
    });
    Ok(source)
}

fn options(source: &ProductScene, case: Case, variant: Variant, size: u32, samples: u32, photons: u32, seed: u32) -> Result<ImageRequest> {
    let mut image = request(source, size, samples, photons, seed, false)?;
    let options = image.raytrace.as_mut().ok_or("strategy options disappeared")?;
    options.radiance_components = false;
    options.sampler = variant.sampler;
    options.area_samples = (case.source == Source::Rectangle).then_some(1);
    let path = options.path.as_mut().ok_or("strategy needs the CPU path integrator")?;
    path.transmission_samples = variant.strata;
    path.split_first_interface = variant.split;
    path.russian_roulette = false;
    let caustics = options.caustics.as_mut().ok_or("strategy photon controls disappeared")?;
    caustics.refresh_per_pass = false;
    caustics.probe_pixel = None;
    caustics.glossy_continuation_max_roughness = match variant.receiver {
        Receiver::Continue => Some(GLOSSY_THRESHOLD),
        Receiver::Gather | Receiver::CameraReference => None,
    };
    caustics.area_lights = case.source == Source::Rectangle && !matches!(variant.receiver, Receiver::CameraReference);
    caustics.environment = case.source == Source::Environment && !matches!(variant.receiver, Receiver::CameraReference);
    match case.source {
        Source::Rectangle => {
            options.lights = Some(Vec::new());
            options.area_lights = Some(vec![RectangleLightOptions {
                center_mm: [-1.0, 0.0, 8.0],
                edge_u_mm: [3.0, 0.0, 0.0],
                edge_v_mm: [0.0, -3.0, 0.0],
                color: [1.0, 0.95, 0.85],
                intensity: 8.0,
            }]);
        }
        Source::Environment => {
            options.lights = Some(Vec::new());
            options.environment = Some(RaytraceEnvironmentOptions { color: [0.8, 0.9, 1.0], intensity: 2.0 });
        }
        Source::Directional => {}
    }
    Ok(image)
}

fn measure(
    out: &Path,
    case: Case,
    variant: Variant,
    size: u32,
    samples: u32,
    photons: u32,
    seed: u32,
    split_comparison: bool,
) -> Result<(Value, Vec<[f64; 3]>)> {
    let source = fixture(case)?;
    let image = options(&source, case, variant, size, samples, photons, seed)?;
    let name = format!("{}-{}-spp{samples:04}-seed{seed:02}", case.name, variant.name);
    execute_render(
        out,
        &name,
        source,
        image,
        json!({
            "case":case.name,"variant":variant.name,"phase":"measurement","seed":seed,"samples":samples,
            "source":case.source.name(),"receiver_roughness":case.roughness,"receiver_metallic":case.metallic,"receiver_ior":1.5,
            "dispersive":case.dispersive,"sampler":variant.sampler,"fresnel_strata":variant.strata,
            "split_first_interface":variant.split,"production_lanes":variant.lanes(),
            "requested_camera_lane_evaluations_per_pixel":u64::from(samples)*u64::from(variant.lanes()),
            "glossy_continuation_max_roughness":match variant.receiver {Receiver::Continue=>Some(GLOSSY_THRESHOLD),_=>None},
            "continuous_source_photons_requested":case.source!=Source::Directional && !matches!(variant.receiver,Receiver::CameraReference),
            "receiver_continuation_selected":matches!(variant.receiver,Receiver::Continue),
            "fixed_radius":true,"photon_maps_reused_within_seed":true,
            "independent_camera_and_map_seeds_between_replicates":true,
            "emitted_paths_is_configured_upper_bound":true,
            "photon_budget_note":"Configured path bound includes misses; the camera reference has no enabled photon source in these continuous-light-only fixtures.",
            "comparison":if split_comparison {"matched_requested_camera_lane_evaluations"} else {"matched_camera_observations"}
        }),
    )
}

fn exact_pair(left: &[[f64; 3]], right: &[[f64; 3]]) -> Result<Value> {
    if left.len() != right.len() || left.is_empty() {
        return Err("paired strategy images have incompatible extents".into());
    }
    let mut different_pixels = 0u64;
    let mut different_components = 0u64;
    let mut max_absolute_difference = 0.0f64;
    for (a, b) in left.iter().zip(right) {
        let mut different = false;
        for (a, b) in a.iter().zip(b) {
            if !a.is_finite() || !b.is_finite() || *a < 0.0 || *b < 0.0 {
                return Err("paired strategy image contains invalid radiance".into());
            }
            if a != b {
                different = true;
                different_components += 1;
                max_absolute_difference = max_absolute_difference.max((a - b).abs());
            }
        }
        different_pixels += u64::from(different);
    }
    Ok(json!({
        "pixels":left.len(),"different_pixels":different_pixels,"different_rgb_components":different_components,
        "max_absolute_rgb_difference":max_absolute_difference,"exact":different_components==0,
        "comparison_domain":"Decoded f32 raw-master RGB, represented exactly as f64; this check is not a density-bias reference."
    }))
}

fn add_ratios(groups: &mut [Value]) -> Result<()> {
    let first = groups.first().ok_or("strategy groups are empty")?;
    let baseline_seconds = first["mean_elapsed_seconds"].as_f64().ok_or("strategy baseline timing missing")?;
    let baseline_variance = first["mean_pixel_luminance_variance_across_independent_seeds"].as_f64().ok_or("strategy baseline variance missing")?;
    let baseline_proxy = first["luminance_variance_times_mean_seconds"].as_f64().ok_or("strategy baseline proxy missing")?;
    for group in groups {
        let seconds = group["mean_elapsed_seconds"].as_f64().ok_or("strategy timing missing")?;
        let variance = group["mean_pixel_luminance_variance_across_independent_seeds"].as_f64().ok_or("strategy variance missing")?;
        let proxy = group["luminance_variance_times_mean_seconds"].as_f64().ok_or("strategy proxy missing")?;
        group["elapsed_ratio_to_baseline"] = json!(seconds / baseline_seconds);
        group["luminance_variance_ratio_to_baseline"] = json!((baseline_variance > 0.0).then(|| variance / baseline_variance));
        group["variance_time_ratio_to_baseline"] = json!((baseline_proxy > 0.0).then(|| proxy / baseline_proxy));
        group["within_factor_1_25_of_baseline_elapsed"] = json!((0.8..=1.25).contains(&(seconds / baseline_seconds)));
    }
    Ok(())
}

pub(super) fn run(out: &Path, profile: &str) -> Result<()> {
    if !is_profile(profile) {
        return Err("unknown strategy comparison profile".into());
    }
    let split_comparison = profile.starts_with("split-");
    let quality = profile.ends_with("-quality");
    let size = if quality { 32u32 } else { 16 };
    let seeds = if quality { 8u32 } else { 2 };
    let camera_budget = if quality { 32u32 } else { 4 };
    let photons = if split_comparison {
        1
    } else if quality {
        4096
    } else {
        512
    };
    let cases = if split_comparison { SPLIT_CASES } else { RECEIVER_CASES };
    let pixels = usize::try_from(size.checked_mul(size).ok_or("strategy pixel extent overflow")?)?;
    let mut records = Vec::new();
    let mut groups = Vec::new();
    let mut pair_checks = Vec::new();
    for (case_index, case) in cases.iter().copied().enumerate() {
        let negative_control = case.source == Source::Directional || case.roughness > GLOSSY_THRESHOLD;
        let variants = if split_comparison {
            SPLIT_VARIANTS
        } else if negative_control {
            RECEIVER_VARIANTS.get(..2).ok_or("negative-control variants disappeared")?
        } else {
            RECEIVER_VARIANTS
        };
        let mut series: Vec<_> = variants.iter().map(|_| SeedSeries::new(pixels)).collect();
        for seed in 0..seeds {
            let mut paired_images = vec![None; variants.len()];
            for position in 0..variants.len() {
                let index = (position + usize::try_from(seed)? + case_index) % variants.len();
                let variant = *variants.get(index).ok_or("strategy variant index escaped")?;
                let samples = if split_comparison { camera_budget / variant.lanes() } else { camera_budget };
                if samples == 0 || (split_comparison && samples * variant.lanes() != camera_budget) {
                    return Err("strategy camera work does not form complete lane groups".into());
                }
                let (record, image) = measure(out, case, variant, size, samples, photons, seed, split_comparison)?;
                let seconds = record["elapsed_seconds"].as_f64().ok_or("strategy elapsed time missing")?;
                series.get_mut(index).ok_or("strategy statistics index escaped")?.observe(&image, seconds)?;
                *paired_images.get_mut(index).ok_or("strategy image index escaped")? = Some(image);
                records.push(record);
                fs::write(out.join("runs.json"), serde_json::to_vec_pretty(&records)?)?;
            }
            if !split_comparison {
                let reference_index = if negative_control { 0 } else { 2 };
                let continued = paired_images.get(1).and_then(Option::as_deref).ok_or("continued image missing")?;
                let reference = paired_images.get(reference_index).and_then(Option::as_deref).ok_or("comparison image missing")?;
                let mut check = exact_pair(continued, reference)?;
                check["case"] = json!(case.name);
                check["seed"] = json!(seed);
                check["pair"] =
                    json!(if negative_control { "continue-glossy versus unchanged gather" } else { "continue-glossy versus camera reference" });
                pair_checks.push(check);
                fs::write(out.join("pair-checks.json"), serde_json::to_vec_pretty(&pair_checks)?)?;
            }
        }
        let mut case_groups = Vec::new();
        for (variant, observed) in variants.iter().copied().zip(&series) {
            let samples = if split_comparison { camera_budget / variant.lanes() } else { camera_budget };
            let mut group = observed.record_group(case.name, variant.name, samples, variant.lanes())?;
            group["production_lanes"] = json!(variant.lanes());
            group["fresnel_strata"] = json!(variant.strata);
            group["split_first_interface"] = json!(variant.split);
            group["sampler"] = json!(variant.sampler);
            group["source"] = json!(case.source.name());
            group["receiver_roughness"] = json!(case.roughness);
            group["receiver_metallic"] = json!(case.metallic);
            group["receiver_ior"] = json!(1.5);
            case_groups.push(group);
        }
        add_ratios(&mut case_groups)?;
        groups.extend(case_groups);
        fs::write(out.join("groups.json"), serde_json::to_vec_pretty(&groups)?)?;
    }
    let pair_checks_passed = pair_checks.iter().all(|check| check["exact"] == true);
    let report = json!({
        "schema_version":1,"profile":profile,"dimensions":[size,size],"seeds":seeds,"threads":1,
        "camera_budget":camera_budget,"camera_budget_unit":if split_comparison {"requested_lane_evaluations_per_pixel"} else {"complete_observations_per_pixel"},
        "configured_photons_per_fixed_map":photons,"glossy_threshold":if split_comparison {None} else {Some(GLOSSY_THRESHOLD)},
        "equal_time_claim":false,"timing_calibration":false,"all_pair_checks_passed":pair_checks_passed,
        "pair_check_count":pair_checks.len(),"case_count":cases.len(),"group_count":groups.len(),"render_count":records.len(),
        "rustc":command_output("rustc",&["--version"]),"git_head":command_output("git",&["rev-parse","HEAD"]),
        "working_tree_status":command_output("git",&["status","--short"]),
        "scope":if split_comparison {
            "CPU first-interface splitting versus one/two/four strata at matched requested lane counts in four continuous-light scenes."
        } else {
            "CPU continuous-source receiver ownership in area/environment scenes, with diffuse and hard-source negative controls."
        },
        "limitations":[
            "Independent-seed scatter is not an independent reference for finite-radius bias or missing path families.",
            "The camera reference disables continuous-source photon ownership while retaining the same geometric-normal policy; it tests reciprocal path accounting, not independent physical accuracy.",
            "All budgets are fixed before production radiance is observed. Actual elapsed times, including setup and output encoding, are retained.",
            "Variance times elapsed time is a descriptive efficiency proxy; there is no exact equal-time or calibrated confidence claim.",
            "Requested lane counts bound path launches; zero branches and paths without an eligible split may do different work.",
            "Two or eight independent randomizations do not establish rare-event tails, GPU throughput or universal noise reduction.",
            "Sum of pixel luminance is a sum over this image, not a physical scene-energy integral.",
            "The finite source, mesh geometry, fixed radius and finite transport-depth domain remain part of every result."
        ],
        "groups":groups,"runs":records,"pair_checks":pair_checks
    });
    fs::write(out.join("report.json"), serde_json::to_vec_pretty(&report)?)?;
    println!(
        "{}",
        serde_json::to_string_pretty(&json!({
            "output":out,"profile":profile,"groups":groups.len(),"renders":records.len(),
            "pair_checks":pair_checks.len(),"all_pair_checks_passed":pair_checks_passed
        }))?
    );
    if !pair_checks_passed {
        return Err("receiver ownership comparison failed an exact raw-pixel accounting check; retained results require inspection".into());
    }
    Ok(())
}
