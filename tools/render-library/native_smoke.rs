//! Native acceptance probe linked only to geom/render/gpu, not the CAD kernel or UI.
//! The Python runner verifies original source hashes and PNG output receipts.
#![forbid(unsafe_code)]
use solidcraft_geom::{Mesh, Vec3};
use solidcraft_render::job::textures::{BaseColorMode, PlanarFrame, RuntimeTextures, TextureBinding, TextureMap};
use solidcraft_render::job::{ImageHost, ImageRequest, Integrator, JobId, JobOptions, Phase, ToneMap};
use solidcraft_render::lighting::{HdrSource, environment::EnvironmentSource};
use solidcraft_render::texture_input::MapRole;
use solidcraft_render::{Camera, ProductMesh, ProductScene, surface::OpaqueSurface};
use std::{
    error::Error,
    fs,
    io::Write,
    path::{Path, PathBuf},
    sync::Arc,
    time::Duration,
};

struct Logger;
impl log::Log for Logger {
    fn enabled(&self, metadata: &log::Metadata<'_>) -> bool {
        metadata.level() <= log::Level::Info
    }
    fn log(&self, record: &log::Record<'_>) {
        if self.enabled(record.metadata()) {
            eprintln!("[{} {}] {}", record.level().as_str().to_lowercase(), record.target(), record.args());
        }
    }
    fn flush(&self) {}
}
static LOGGER: Logger = Logger;
fn scene() -> Result<ProductScene, Box<dyn Error>> {
    let mesh = Arc::new(Mesh {
        positions: vec![Vec3::new(-50.0, -50.0, 0.0), Vec3::new(50.0, -50.0, 0.0), Vec3::new(50.0, 50.0, 0.0), Vec3::new(-50.0, 50.0, 0.0)],
        normals: vec![Vec3::Z; 4],
        triangles: vec![[0, 1, 2], [0, 2, 3]],
        tri_face: vec![0, 0],
        ..Default::default()
    });
    Ok(ProductScene {
        bounds: mesh.bounds(),
        meshes: vec![ProductMesh {
            name: "Original authored-surface coupon".into(),
            mesh,
            color: [220, 40, 20],
            opacity: 1.0,
            surface: Some(OpaqueSurface::new(Vec3::new(0.7, 0.02, 0.01), 0.0, 0.8, 1.5)?),
            transmission: None,
            faces: Vec::new(),
        }],
    })
}
fn map(root: &Path, name: &str, role: MapRole) -> Result<TextureMap, Box<dyn Error>> {
    let file = root.join("assets/render/starter/materials/exports/SURF-001").join(format!("{name}.png"));
    Ok(TextureMap::new(fs::read(file)?, role)?)
}
fn request(root: &Path) -> Result<ImageRequest, Box<dyn Error>> {
    let hdr = HdrSource::new(fs::read(root.join("assets/render/starter/environments/exports/neutral-side-softbox.hdr"))?)?;
    let environment = EnvironmentSource::from_hdr(hdr, std::f64::consts::PI, 1.0, false)?;
    Ok(ImageRequest {
        camera: Camera { distance: 220.0, ..Default::default() },
        width: 320,
        height: 240,
        background: [236, 240, 245],
        transparent_background: false,
        integrator: Integrator::Raster,
        raytrace: None,
        environment: Some(Arc::new(environment)),
        textures: None,
    })
}
fn render(host: &ImageHost, root: &Path, out: &Path, label: &str, serial: u64, mode: BaseColorMode, scalars: bool) -> Result<String, Box<dyn Error>> {
    let scene = scene()?;
    let mut request = request(root)?;
    let binding = TextureBinding {
        body_index: 0,
        body_name: "Original authored-surface coupon".into(),
        frame: PlanarFrame { origin: [0.0; 3], u: [100.0, 0.0, 0.0], v: [0.0, 100.0, 0.0] },
        normal_strength: 1.0,
        base_color: Some(map(root, "base_color", MapRole::BaseColor)?),
        normal: Some(map(root, "normal_open_gl", MapRole::NormalOpenGl)?),
        roughness: if scalars { Some(map(root, "roughness", MapRole::Roughness)?) } else { None },
        metallic: if scalars { Some(map(root, "metallic", MapRole::Metallic)?) } else { None },
        base_color_mode: mode,
        flip_v: true,
    };
    request.textures = Some(Arc::new(RuntimeTextures::new(vec![binding])?));
    let options = JobOptions { samples: 64, max_depth: 6, seed: 82, tone_map: ToneMap::AcesFitted, ..Default::default() };
    let admission = host.admit(&scene, &request, options)?;
    let handle = host.submit_with_options(JobId { session: 20261022, serial, generation: 1 }, 1, scene, request, options)?;
    let status = match handle.wait_timeout(Duration::from_secs(300)) {
        Ok(status) => status,
        Err(error) => {
            let _ = handle.cancel();
            return Err(error.into());
        }
    };
    if status.progress.phase != Phase::Completed {
        return Err(format!("GPU did not complete: {:?}", status.error).into());
    }
    let frame = handle.frame()?.ok_or("GPU completion has no retained frame")?;
    if frame.info.executor.as_str() != "gpu-path" || (frame.info.width, frame.info.height) != (320, 240) {
        return Err("wrong GPU executor or output dimensions".into());
    }
    let mut file = fs::OpenOptions::new().write(true).create_new(true).open(out.join(format!("{label}.png")))?;
    file.write_all(frame.png())?;
    file.sync_all()?;
    println!(
        "FRAME|{label}|{}|{}|{}|{}|{}|{}",
        frame.info.png_sha256,
        frame.info.png_bytes,
        frame.info.width,
        frame.info.height,
        frame.info.executor.as_str(),
        admission.estimated_total_bytes
    );
    let hash = frame.info.png_sha256.clone();
    drop(frame);
    drop(handle);
    Ok(hash)
}
fn main() -> Result<(), Box<dyn Error>> {
    log::set_logger(&LOGGER).map_err(|e| format!("logger: {e}"))?;
    log::set_max_level(log::LevelFilter::Info);
    let mut args = std::env::args_os().skip(1);
    let root = args.next().map(PathBuf::from).ok_or("repository path required")?;
    let out = args.next().map(PathBuf::from).ok_or("new output path required")?;
    let host = solidcraft_gpu::path::host()?;
    let full = render(&host, &root, &out, "authored-four-maps", 1, BaseColorMode::Replace, true)?;
    let legacy = render(&host, &root, &out, "legacy-color-modulation", 2, BaseColorMode::Modulate, true)?;
    let scalar_control = render(&host, &root, &out, "captured-scalar-control", 3, BaseColorMode::Replace, false)?;
    if full == legacy || full == scalar_control {
        return Err("authored surface and control renders must differ".into());
    }
    for _ in 0..100 {
        if host.admitted_bytes() == 0 {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if host.admitted_bytes() != 0 {
        return Err("GPU retained reservations were not released".into());
    }
    println!("POOL|0");
    println!("STATUS|PASS");
    Ok(())
}
