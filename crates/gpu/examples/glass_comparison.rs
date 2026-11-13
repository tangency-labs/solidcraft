//! Original, headless CPU/GPU Glass evidence. This is not an application or image-quality acceptance test.
//! Run with `--features solidcraft-render/raytrace-cpu`; output directory must not exist.
use serde_json::json;
use solidcore_record::Vec3;
use solidcraft_geom::MeshGeom;
use solidcraft_geom::{Aabb3, Mesh, Vec3Geom};
use solidcraft_render::{
    ProductMesh, ProductScene,
    glass_model::{Dispersion, GlassModel, ThinFilm},
    job::{
        AreaLight, AreaLights, CausticOptions, CpuBudget, CpuHost, CpuPathOptions, CpuProgressiveOptions, DielectricBoundary, DielectricOptions,
        DirectionalLightOptions, DirectionalLights, ImageHost, ImageRequest, Integrator, JobId, JobOptions, OutputOptions, Phase,
        PunctualLightOptions, PunctualLights, RaytraceEnvironmentOptions, RaytraceOptions, RectangleLightOptions, SpotConeOptions, StudioLighting,
    },
    transmission::{Transmission, subsurface::Subsurface},
};
use solidcraft_viewport::{
    Camera,
    surface::{OpaqueSurface, unit_direction},
};
use std::{error::Error, fs, path::Path, sync::Arc, time::Duration};

type Result<T> = std::result::Result<T, Box<dyn Error>>;
const CASES: &[&str] = &[
    "clear",
    "tinted",
    "hollow",
    "nested",
    "sheet",
    "rough",
    "dispersion",
    "film",
    "rough_sheet",
    "caustics",
    "rough_lights",
    "rough_sheet_lights",
    "rough_caustics",
    "scattering_caustics",
];
const ENVIRONMENT: [f64; 3] = [0.7, 0.8, 1.0];
const LIGHT: AreaLight = AreaLight { center: [-22.0, -30.0, 28.0], u: [9.0, 0.0, 0.0], v: [0.0, 0.0, -13.0], radiance: [4.0, 4.0, 4.0] };

fn quad(mesh: &mut Mesh, vertices: [u32; 4]) {
    let [a, b, c, d] = vertices;
    mesh.triangles.extend([[a, b, c], [a, c, d]]);
    mesh.tri_face.extend([0, 0]);
}

fn rectangle(points: [Vec3; 4]) -> Mesh {
    let mut mesh = Mesh { positions: points.to_vec(), ..Default::default() };
    quad(&mut mesh, [0, 1, 2, 3]);
    mesh
}

fn box_mesh(half: Vec3) -> Mesh {
    let mut mesh = Mesh::default();
    for z in [-half.z, half.z] {
        mesh.positions.extend([
            Vec3::new(-half.x, -half.y, z),
            Vec3::new(half.x, -half.y, z),
            Vec3::new(half.x, half.y, z),
            Vec3::new(-half.x, half.y, z),
        ]);
    }
    quad(&mut mesh, [0, 3, 2, 1]);
    quad(&mut mesh, [4, 5, 6, 7]);
    for i in 0..4 {
        let next = (i + 1) % 4;
        quad(&mut mesh, [i, next, next + 4, i + 4]);
    }
    mesh
}

fn cup_mesh() -> Mesh {
    // One connected, outward shell with an open air cavity and a solid bottom.
    // Shared coordinates are authored once; no welded or repaired seam is needed.
    let mut mesh = Mesh::default();
    for (half, z) in [(10.0, -10.0), (10.0, 10.0), (7.0, 10.0), (7.0, -7.0)] {
        mesh.positions.extend([Vec3::new(-half, -half, z), Vec3::new(half, -half, z), Vec3::new(half, half, z), Vec3::new(-half, half, z)]);
    }
    quad(&mut mesh, [0, 3, 2, 1]);
    quad(&mut mesh, [12, 13, 14, 15]);
    for i in 0..4 {
        let next = (i + 1) % 4;
        quad(&mut mesh, [i, next, next + 4, i + 4]);
        quad(&mut mesh, [i + 4, next + 4, next + 8, i + 8]);
        quad(&mut mesh, [next + 12, i + 12, i + 8, next + 8]);
    }
    mesh
}

fn prism_mesh() -> Mesh {
    let mut mesh = Mesh {
        positions: vec![
            Vec3::new(-10.0, -8.0, -10.0),
            Vec3::new(10.0, -8.0, -10.0),
            Vec3::new(-10.0, 8.0, -10.0),
            Vec3::new(-10.0, -8.0, 10.0),
            Vec3::new(10.0, -8.0, 10.0),
            Vec3::new(-10.0, 8.0, 10.0),
        ],
        triangles: vec![[0, 2, 1], [3, 4, 5]],
        tri_face: vec![0; 2],
        ..Default::default()
    };
    for face in [[0, 1, 4, 3], [1, 2, 5, 4], [2, 0, 3, 5]] {
        quad(&mut mesh, face);
    }
    mesh
}

fn sphere_mesh() -> Mesh {
    use std::f64::consts::{PI, TAU};
    const SEGMENTS: u32 = 32;
    const RINGS: u32 = 15;
    let mut mesh = Mesh { positions: vec![Vec3::new(0.0, 0.0, -10.0)], ..Default::default() };
    for ring in 1..=RINGS {
        let latitude = -PI * 0.5 + PI * f64::from(ring) / f64::from(RINGS + 1);
        for sector in 0..SEGMENTS {
            let angle = TAU * f64::from(sector) / f64::from(SEGMENTS);
            mesh.positions.push(Vec3::new(10.0 * latitude.cos() * angle.cos(), 10.0 * latitude.cos() * angle.sin(), 10.0 * latitude.sin()));
        }
    }
    let top = 1 + RINGS * SEGMENTS;
    mesh.positions.push(Vec3::new(0.0, 0.0, 10.0));
    for sector in 0..SEGMENTS {
        let next = (sector + 1) % SEGMENTS;
        mesh.triangles.push([0, 1 + next, 1 + sector]);
        mesh.triangles.push([top, 1 + (RINGS - 1) * SEGMENTS + sector, 1 + (RINGS - 1) * SEGMENTS + next]);
    }
    for ring in 0..RINGS - 1 {
        for sector in 0..SEGMENTS {
            let next = (sector + 1) % SEGMENTS;
            let a = 1 + ring * SEGMENTS + sector;
            let b = 1 + ring * SEGMENTS + next;
            quad(&mut mesh, [a, b, b + SEGMENTS, a + SEGMENTS]);
        }
    }
    mesh.tri_face.resize(mesh.triangles.len(), 0);
    mesh
}

fn flat_mesh(source: Mesh) -> Result<Mesh> {
    let count = source.triangles.len().checked_mul(3).ok_or("fixture vertex count overflow")?;
    let mut mesh = Mesh::default();
    mesh.positions.try_reserve_exact(count)?;
    mesh.normals.try_reserve_exact(count)?;
    mesh.triangles.try_reserve_exact(source.triangles.len())?;
    mesh.tri_face.try_reserve_exact(source.triangles.len())?;
    for (face, &[a, b, c]) in source.triangles.iter().enumerate() {
        let point = |index: u32| source.positions.get(index as usize).copied().ok_or("fixture vertex index is invalid");
        let [a, b, c] = [point(a)?, point(b)?, point(c)?];
        let normal = unit_direction((b - a).cross(c - a))?;
        let begin = u32::try_from(mesh.positions.len())?;
        mesh.positions.extend([a, b, c]);
        mesh.normals.extend([normal; 3]);
        mesh.triangles.push([begin, begin + 1, begin + 2]);
        mesh.tri_face.push(source.tri_face.get(face).copied().ok_or("fixture face ID is missing")?);
    }
    Ok(mesh)
}

fn body(name: &str, mesh: Mesh, color: Vec3, ior: f64, transmission: Option<Transmission>) -> Result<ProductMesh> {
    Ok(ProductMesh {
        name: name.into(),
        mesh: Arc::new(flat_mesh(mesh)?),
        color: [255, 255, 255],
        opacity: 1.0,
        surface: Some(OpaqueSurface::new(color, 0.0, 0.45, ior)?),
        transmission,
        faces: Vec::new(),
    })
}

fn scene(case: &str) -> Result<ProductScene> {
    let mut meshes = Vec::new();
    let sigma = if ["tinted", "hollow", "nested", "sheet"].contains(&case) { Vec3::new(0.065, 0.012, 0.003) } else { Vec3::ZERO };
    let sheet = ["sheet", "rough_sheet", "rough_sheet_lights"].contains(&case).then_some(3.0);
    let mut transmission = Transmission::new(sigma, sheet)?;
    let model = match case {
        "rough" | "rough_sheet" | "rough_lights" | "rough_sheet_lights" => Some(GlassModel { roughness: 0.35, ..Default::default() }),
        "rough_caustics" => Some(GlassModel { roughness: 0.2, ..Default::default() }),
        "dispersion" => Some(GlassModel { dispersion: Some(Dispersion { abbe_number: 15.0 }), ..Default::default() }),
        "film" => Some(GlassModel { thin_film: Some(ThinFilm { ior: 1.35, thickness_nm: 400.0 }), ..Default::default() }),
        _ => None,
    };
    if let Some(model) = model {
        transmission = transmission.with_shadow_transmission(false).with_model(model)?;
    }
    if case == "scattering_caustics" {
        transmission = transmission.with_subsurface(Subsurface { scattering_per_mm: 0.06, anisotropy: 0.35 })?;
    }
    let optical_mesh = match case {
        "hollow" => cup_mesh(),
        "dispersion" => prism_mesh(),
        "caustics" | "rough_caustics" => sphere_mesh(),
        "sheet" | "rough_sheet" | "rough_sheet_lights" => {
            rectangle([Vec3::new(-10.0, -1.0, -10.0), Vec3::new(10.0, -1.0, -10.0), Vec3::new(10.0, -1.0, 10.0), Vec3::new(-10.0, -1.0, 10.0)])
        }
        _ => box_mesh(Vec3::new(10.0, 8.0, 10.0)),
    };
    meshes.push(body("Original Glass boundary", optical_mesh, Vec3::ZERO, 1.5, Some(transmission))?);
    if case == "nested" {
        meshes.push(body(
            "Original enclosed air bubble",
            box_mesh(Vec3::new(6.0, 4.0, 6.0)),
            Vec3::ZERO,
            1.0,
            Some(Transmission::new(Vec3::ZERO, None)?),
        )?);
    }
    for row in 0..6 {
        for column in 0..8 {
            let x = (f64::from(column) - 4.0) * 8.0;
            let z = (f64::from(row) - 3.0) * 8.0;
            let color = if (row + column) % 2 == 0 { Vec3::new(0.055, 0.08, 0.13) } else { Vec3::new(0.8, 0.65, 0.38) };
            let mesh =
                rectangle([Vec3::new(x, 18.0, z), Vec3::new(x + 8.0, 18.0, z), Vec3::new(x + 8.0, 18.0, z + 8.0), Vec3::new(x, 18.0, z + 8.0)]);
            meshes.push(body(&format!("Original checker {column} {row}"), mesh, color, 1.5, None)?);
        }
    }
    let floor = if ["caustics", "rough_caustics"].contains(&case) { -15.0 } else { -12.0 };
    meshes.push(body(
        "Original opaque floor",
        rectangle([Vec3::new(-45.0, -35.0, floor), Vec3::new(45.0, -35.0, floor), Vec3::new(45.0, 35.0, floor), Vec3::new(-45.0, 35.0, floor)]),
        Vec3::new(0.33, 0.37, 0.43),
        1.5,
        None,
    )?);
    let bounds = meshes.iter().fold(Aabb3::EMPTY, |bounds, body| bounds.union(&body.mesh.bounds()));
    Ok(ProductScene { meshes, bounds })
}

fn camera() -> Camera {
    Camera { target: Vec3::ZERO, yaw: -0.23, pitch: 0.25, distance: 82.0, fov: 0.65, ..Default::default() }
}

fn caustics(case: &str) -> Option<CausticOptions> {
    ["caustics", "rough_caustics", "scattering_caustics"].contains(&case).then_some(CausticOptions {
        photons: 65536,
        radius_mm: if case == "scattering_caustics" { 2.0 } else { 0.75 },
        ..Default::default()
    })
}
fn hard_only(case: &str) -> bool {
    ["rough_lights", "rough_sheet_lights", "rough_caustics", "scattering_caustics"].contains(&case)
}
fn environment(case: &str) -> [f64; 3] {
    if hard_only(case) { [0.0; 3] } else { ENVIRONMENT }
}
fn area_lights(case: &str) -> Vec<AreaLight> {
    if hard_only(case) { Vec::new() } else { vec![LIGHT] }
}
fn directional(case: &str) -> Vec<DirectionalLightOptions> {
    if ["rough_lights", "rough_sheet_lights"].contains(&case) {
        vec![DirectionalLightOptions { direction: [0.15, -1.0, 0.25], color: [0.5, 0.7, 1.0], intensity: 3.0 }]
    } else if caustics(case).is_some() {
        vec![DirectionalLightOptions { direction: [0.2, -0.3, 1.0], color: [1.0; 3], intensity: 4.0 }]
    } else {
        Vec::new()
    }
}
fn punctual(case: &str) -> Vec<PunctualLightOptions> {
    if !["rough_lights", "rough_sheet_lights"].contains(&case) {
        return Vec::new();
    }
    vec![
        PunctualLightOptions {
            position_mm: [-12.0, -30.0, 15.0],
            color: [1.0, 0.3, 0.1],
            intensity: 2.0,
            reference_distance_mm: 30.0,
            minimum_distance_mm: 0.1,
            cone: None,
        },
        PunctualLightOptions {
            position_mm: [20.0, -25.0, 18.0],
            color: [0.2, 1.0, 0.3],
            intensity: 2.0,
            reference_distance_mm: 30.0,
            minimum_distance_mm: 0.1,
            cone: Some(SpotConeOptions { emitted_axis: [-20.0, 25.0, -18.0], inner_half_angle_radians: 0.25, outer_half_angle_radians: 0.65 }),
        },
    ]
}
fn request(scene: &ProductScene, cpu: bool, case: &str, samples: u32, width: u32) -> Result<ImageRequest> {
    let raytrace = if cpu {
        let mut dielectrics = Vec::new();
        for (body_index, body) in scene.meshes.iter().enumerate() {
            if let Some(optics) = body.transmission {
                let surface = body.surface.ok_or("Glass body lacks its optical IOR")?;
                let sigma = optics.extinction();
                dielectrics.push(DielectricOptions {
                    model: optics.model(),
                    body_index,
                    body_name: body.name.clone(),
                    boundary: if optics.thin_wall_thickness_mm().is_some() { DielectricBoundary::ThinSheet } else { DielectricBoundary::ClosedMesh },
                    ior: surface.dielectric_ior(),
                    absorption_per_mm: [sigma.x, sigma.y, sigma.z],
                    thickness_mm: optics.thin_wall_thickness_mm().unwrap_or(0.0),
                    shadow_transmission: optics.shadow_transmission(),
                    subsurface: optics.subsurface(),
                });
            }
        }
        let areas = area_lights(case);
        Some(RaytraceOptions {
            caustics: caustics(case),
            budget: CpuBudget::Studio,
            progressive: Some(CpuProgressiveOptions { samples_per_pixel: samples, seed: 81 }),
            path: Some(CpuPathOptions { max_depth: 16 }),
            dielectrics,
            environment: Some(RaytraceEnvironmentOptions { color: environment(case), intensity: 1.0 }),
            lights: Some(directional(case)),
            punctual_lights: Some(punctual(case)),
            area_lights: Some(
                areas
                    .iter()
                    .map(|light| RectangleLightOptions {
                        center_mm: light.center,
                        edge_u_mm: light.u.map(|v| v * 2.0),
                        edge_v_mm: light.v.map(|v| v * 2.0),
                        color: [1.0; 3],
                        intensity: 4.0,
                    })
                    .collect(),
            ),
            area_samples: (!areas.is_empty()).then_some(4),
            output: Some(Box::new(OutputOptions { linear_exr: true, ..Default::default() })),
            ..Default::default()
        })
    } else {
        None
    };
    let mut camera = camera();
    if ["caustics", "rough_caustics"].contains(&case) {
        camera.pitch = 0.65;
        camera.target.z = -4.0;
    }
    Ok(ImageRequest {
        camera,
        width,
        height: width * 3 / 4,
        background: [228, 235, 245],
        transparent_background: false,
        integrator: if cpu { Integrator::Raytrace } else { Integrator::Raster },
        raytrace,
        studio_weights: None,
        environment: None,
        textures: None,
    })
}

struct Evidence<'a> {
    samples: u32,
    width: u32,
    out: &'a Path,
}

fn render(host: &ImageHost, source: ProductScene, cpu: bool, case: &str, evidence: &Evidence<'_>, serial: u64) -> Result<()> {
    let Evidence { samples, width, out } = *evidence;
    let backend = if cpu { "cpu" } else { "gpu" };
    let request = request(&source, cpu, case, samples, width)?;
    let options = if cpu {
        JobOptions::default()
    } else {
        JobOptions {
            caustics: caustics(case),
            samples,
            seed: 81,
            max_depth: 16,
            nested_media: case == "nested",
            linear_exr: true,
            lighting: Some(StudioLighting {
                directional_lights: DirectionalLights::new(&directional(case))?,
                punctual_lights: PunctualLights::new(&punctual(case))?,
                environment: environment(case),
                area_lights: AreaLights::new(&area_lights(case))?,
                ..Default::default()
            }),
            ..Default::default()
        }
    };
    let descriptor = host.descriptor_for_request(&request)?;
    let recorded_request = serde_json::to_value(&request)?;
    let recorded_options = serde_json::to_value(options)?;
    let geometry: Vec<_> = source
        .meshes
        .iter()
        .map(|body| {
            json!({
                "name":body.name,"positions":body.mesh.positions,"normals":body.mesh.normals,
                "triangles":body.mesh.triangles,"tri_face":body.mesh.tri_face,"preview_color":body.color,"opacity":body.opacity,
                "surface":body.surface.map(|surface|json!({
                    "base_color_linear":surface.base_color(),"metallic":surface.metallic(),"roughness":surface.roughness(),
                    "ior":surface.dielectric_ior(),"clear_coat_present":surface.clear_coat().is_some()
                })),
                "transmission":body.transmission.map(|optics|json!({
                    "absorption_per_mm":optics.extinction(),"thin_wall_thickness_mm":optics.thin_wall_thickness_mm(),
                    "shadow_transmission":optics.shadow_transmission(),"subsurface":optics.subsurface(),"model":optics.model()
                }))
            })
        })
        .collect();
    let mut source = source;
    if cpu {
        // CPU optics were lowered into RaytraceOptions above. Its capture scene
        // carries opaque geometry; scene-level Transmission belongs to GPU.
        for body in &mut source.meshes {
            body.transmission = None;
            for face in &mut body.faces {
                face.transmission = None;
            }
        }
    }
    let handle = host.submit_with_options(JobId { session: 20261104, serial, generation: 1 }, 1, source, request, options)?;
    let status = match handle.wait_timeout(Duration::from_secs(600)) {
        Ok(status) => status,
        Err(error) => {
            let _ = handle.cancel();
            return Err(error.into());
        }
    };
    if status.progress.phase != Phase::Completed {
        return Err(format!("{backend} {case} failed: {:?}", status.error).into());
    }
    let frame = handle.frame()?.ok_or("completed render has no frame")?;
    let png = image::load_from_memory(frame.png())?.to_rgba8();
    if png.dimensions() != (width, width * 3 / 4) || !png.pixels().any(|pixel| pixel.0 != [228, 235, 245, 255]) {
        return Err("render did not produce the original Glass fixture".into());
    }
    let exr = frame.linear_exr().ok_or("completed render has no raw EXR master")?;
    fs::write(out.join(format!("{backend}-{case}.png")), frame.png())?;
    fs::write(out.join(format!("{backend}-{case}.exr")), exr)?;
    let receipt = json!({
        "status":"PASS", "case":case,"backend":backend,"executor":descriptor,"request":recorded_request,"options":recorded_options,
        "frame":frame.info,"linear_master":frame.linear_master_info(),"original_geometry":geometry,
        "qualification":"headless completed image and retained same-frame raw master only",
        "application_ui_tested":false,"saved_design_tested":false,"image_quality_acceptance":false,
        "pixel_identity_between_backends_expected":false,
        "sampling_difference":"CPU uses four samples per enabled area connection; GPU uses its native estimator. Camera and branch sample streams differ."
    });
    fs::write(out.join(format!("{backend}-{case}.json")), serde_json::to_vec_pretty(&receipt)?)?;
    println!("Completed {backend} {case}: {width}x{}, {samples} samples/pixel", width * 3 / 4);
    Ok(())
}

fn main() -> Result<()> {
    let mut args = std::env::args().skip(1);
    let out = args.next().ok_or("usage: glass_comparison NEW_OUTPUT_DIRECTORY [cpu|gpu|both] [SPP=32] [CASE=all] [WIDTH=256]")?;
    let backend = args.next().unwrap_or_else(|| "both".into());
    let samples = args.next().map_or(Ok(32), |v| v.parse::<u32>())?;
    let case = args.next().unwrap_or_else(|| "all".into());
    let width = args.next().map_or(Ok(256), |v| v.parse::<u32>())?;
    if args.next().is_some() || !["cpu", "gpu", "both"].contains(&backend.as_str()) || !(1..=64).contains(&samples) || !(64..=1024).contains(&width) {
        return Err("expected cpu|gpu|both, 1..64 SPP and width 64..1024".into());
    }
    if case != "all" && !CASES.contains(&case.as_str()) {
        return Err(format!("case must be one of {} or all", CASES.join("|")).into());
    }
    let out = Path::new(&out);
    fs::create_dir(out)?;
    let evidence = Evidence { samples, width, out };
    let cpu = (backend != "gpu").then(CpuHost::for_platform).transpose()?;
    let gpu = (backend != "cpu").then(solidcraft_gpu::path::host).transpose()?;
    for (index, selected) in CASES.iter().copied().enumerate().filter(|(_, selected)| case == "all" || case == *selected) {
        let serial = u64::try_from(index)? + 1;
        if let Some(host) = &cpu {
            render(host, scene(selected)?, true, selected, &evidence, serial)?;
        }
        if let Some(host) = &gpu {
            render(host, scene(selected)?, false, selected, &evidence, serial)?;
        }
    }
    Ok(())
}
