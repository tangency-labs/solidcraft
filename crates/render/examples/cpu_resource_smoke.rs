//! Bounded native acceptance probe for explicit CPU profiles. This is not a speed
//! benchmark or an application/UI test. Writes only to a new caller-selected folder.
use serde_json::json;
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_render::job::{CpuBudget, CpuHost, DirectionalLightOptions, ImageRequest, Integrator, JobId, JobOptions, Phase, RaytraceOptions};
use solidcraft_render::{Camera, ProductMesh, ProductScene, StandardView, surface::OpaqueSurface};
use std::{error::Error, fs, path::PathBuf, sync::Arc, time::Duration};

fn scene() -> Result<ProductScene, Box<dyn Error>> {
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::new(-20.0, -15.0, 0.0), Vec3::new(20.0, -15.0, 0.0), Vec3::new(20.0, 15.0, 0.0), Vec3::new(-20.0, 15.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    });
    Ok(ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![ProductMesh {
            name: "Original CPU admission plate".into(),
            mesh,
            color: [92, 150, 210],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.1, 0.3, 0.6), 0.0, 0.5, 1.5)?),
            transmission: None,
            faces: Vec::new(),
        }],
    })
}
fn request(width: u32, height: u32, budget: CpuBudget) -> ImageRequest {
    let mut camera = Camera { distance: 65.0, ..Default::default() };
    camera.set_view(StandardView::Top);
    ImageRequest {
        environment: None,
        textures: None,
        integrator: Integrator::Raytrace,
        raytrace: Some(RaytraceOptions {
            budget,
            lights: Some(vec![DirectionalLightOptions { direction: [0.0, 0.0, 1.0], color: [1.0; 3], intensity: 2.0 }]),
            shadows: false,
            ..Default::default()
        }),
        camera,
        width,
        height,
        background: [236, 240, 245],
        transparent_background: false,
    }
}
fn dense_scene() -> Result<ProductScene, Box<dyn Error>> {
    let side = 500u32;
    let vertices = (side + 1) * (side + 1);
    let mut positions = Vec::new();
    positions.try_reserve_exact(vertices as usize)?;
    for y in 0..=side {
        for x in 0..=side {
            positions.push(Vec3::new(-20.0 + 40.0 * f64::from(x) / f64::from(side), -15.0 + 30.0 * f64::from(y) / f64::from(side), 0.0));
        }
    }
    let mut triangles = Vec::new();
    triangles.try_reserve_exact((2 * side * side) as usize)?;
    for y in 0..side {
        for x in 0..side {
            let a = y * (side + 1) + x;
            let b = a + 1;
            let c = a + side + 2;
            let d = a + side + 1;
            triangles.push([a, b, c]);
            triangles.push([a, c, d]);
        }
    }
    let mesh =
        Arc::new(Mesh { positions, normals: vec![Vec3::Z; vertices as usize], tri_face: vec![0; triangles.len()], triangles, ..Default::default() });
    let mut result = scene()?;
    result.bounds = mesh.bounds();
    let body = result.meshes.first_mut().ok_or("missing fixture body")?;
    body.mesh = mesh;
    body.name = "Original 500000-triangle grid".into();
    Ok(result)
}
fn body_scene() -> Result<ProductScene, Box<dyn Error>> {
    let mut result = scene()?;
    let template = result.meshes.first().cloned().ok_or("missing fixture body")?;
    result.meshes.clear();
    result.meshes.try_reserve_exact(20_000)?;
    for y in 0..100 {
        for x in 0..200 {
            let (x, y) = (f64::from(x) - 99.5, f64::from(y) - 49.5);
            let mesh = Arc::new(Mesh {
                positions: vec![
                    Vec3::new(x - 0.4, y - 0.4, 0.0),
                    Vec3::new(x + 0.4, y - 0.4, 0.0),
                    Vec3::new(x + 0.4, y + 0.4, 0.0),
                    Vec3::new(x - 0.4, y + 0.4, 0.0),
                ],
                normals: vec![Vec3::Z; 4],
                triangles: vec![[0, 1, 2], [0, 2, 3]],
                tri_face: vec![0, 0],
                ..Default::default()
            });
            let mut body = template.clone();
            body.name = format!("Original tile {x} {y}");
            body.mesh = mesh;
            result.meshes.push(body);
        }
    }
    result.bounds = solidcraft_geom::Aabb3 { min: Vec3::new(-100.0, -50.0, 0.0), max: Vec3::new(100.0, 50.0, 0.0) };
    Ok(result)
}

fn main() -> Result<(), Box<dyn Error>> {
    let out = std::env::args_os().nth(1).map(PathBuf::from).ok_or("usage: cpu_resource_smoke NEW_OUTPUT_DIRECTORY [4k|8k|dense|bodies]")?;
    fs::create_dir(&out)?;
    let host = CpuHost::for_platform()?;
    let case = std::env::args().nth(2).unwrap_or_else(|| "4k".into());
    let (width, height, budget, source) = match case.as_str() {
        "4k" => (3840, 2160, CpuBudget::Studio, scene()?),
        "8k" => (7680, 4320, CpuBudget::Workstation, scene()?),
        "dense" => (256, 256, CpuBudget::Studio, dense_scene()?),
        "bodies" => (512, 256, CpuBudget::Studio, body_scene()?),
        _ => return Err("case must be 4k, 8k, dense or bodies".into()),
    };
    let compact = request(width, height, CpuBudget::Compact);
    if host.admit(&source, &compact, JobOptions::default()).is_ok() {
        return Err("legacy Compact incorrectly admitted a deliberately oversized request".into());
    }
    let mut studio = request(width, height, budget);
    if case == "bodies" {
        studio.camera.fit(&source.bounds);
    }
    let admission = host.admit(&source, &studio, JobOptions::default())?;
    let id = JobId { session: 20261022, serial: 1, generation: 1 };
    let handle = host.submit(id, 1, source.clone(), studio)?;
    let status = match handle.wait_timeout(Duration::from_secs(300)) {
        Ok(status) => status,
        Err(error) => {
            let _ = handle.cancel();
            return Err(error.into());
        }
    };
    if status.progress.phase != Phase::Completed {
        return Err(format!("CPU render failed: {:?}", status.error).into());
    }
    let frame = handle.frame()?.ok_or("completed CPU result has no frame")?;
    let image = image::load_from_memory(frame.png())?.to_rgba8();
    if image.dimensions() != (width, height) || !image.pixels().any(|pixel| pixel.0 != [236, 240, 245, 255]) {
        return Err("image does not contain the original analytic fixture".into());
    }
    fs::write(out.join(format!("cpu-{case}.png")), frame.png())?;
    let frame_info = serde_json::to_value(&frame.info)?;
    drop(frame);
    drop(handle);
    // Native cleanup is observed through reservations; no capacity is claimed merely
    // from a dimensions constant or a downscaled display texture.
    for _ in 0..100 {
        if host.admitted_bytes() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if host.admitted_bytes() != 0 {
        return Err("completed CPU frame reservation was not released".into());
    }

    // Synthetic cardinality probe only, not a heavy-assembly render benchmark.
    let admission_scene = scene()?;
    let template = admission_scene.meshes.first().ok_or("missing original plate")?;
    let mut bodies = Vec::new();
    bodies.try_reserve_exact(20_000)?;
    for i in 0..20_000 {
        let mut body = template.clone();
        body.name = format!("Admission body {i}");
        bodies.push(body);
    }
    let many = ProductScene { bounds: admission_scene.bounds, meshes: bodies };
    if host.admit(&many, &request(16, 16, CpuBudget::Compact), JobOptions::default()).is_ok() {
        return Err("Compact accepted too many bodies".into());
    }
    let bodies_admission = host.admit(&many, &request(16, 16, CpuBudget::Studio), JobOptions::default())?;
    let record = json!({"status":"PASS","scope":"native CPU selected-case execution plus separate bounded synthetic body admission","case":case,"rendered_bodies":source.meshes.len(),"rendered_triangles":admission.triangles,
        "not_a_performance_benchmark":true,"application_ui_tested":false,"frame":frame_info,
        "image_admission":admission,"twenty_thousand_bodies_admission":bodies_admission,
        "retained_pool_bytes_after_drop":host.admitted_bytes(),"compact_refused_both_explicit_large_requests":true});
    fs::write(out.join("receipt.json"), serde_json::to_vec_pretty(&record)?)?;
    println!("{}", serde_json::to_string_pretty(&record)?);
    Ok(())
}
