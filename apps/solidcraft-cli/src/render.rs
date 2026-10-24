//! Complete headless image-job workflow through the same engine commands as the UI/MCP.
use serde_json::{Value, json};
use solidcraft_engine::{
    Session,
    doc::render_settings::{RenderJob, RenderSetup},
    render::job::{CpuHost, JobId, Phase, ToneMap},
};

/// Only explicit runtime CLI flags are stored here. A typed setup owns these
/// values even when a supplied flag happens to match the saved value.
#[derive(Default)]
struct RuntimeOptions {
    progressive: bool,
    transparent: bool,
    samples: Option<u32>,
    max_depth: Option<u32>,
    seed: Option<u32>,
    lighting: Option<String>,
    tone_map: Option<ToneMap>,
    environment: Option<String>,
}

impl RuntimeOptions {
    fn explicit_flag(&self) -> Option<&'static str> {
        [
            (self.progressive, "--progressive"),
            (self.transparent, "--transparent"),
            (self.samples.is_some(), "--samples"),
            (self.max_depth.is_some(), "--max-depth"),
            (self.seed.is_some(), "--seed"),
            (self.lighting.is_some(), "--lighting"),
            (self.tone_map.is_some(), "--tone-map"),
            (self.environment.is_some(), "--environment"),
        ]
        .into_iter()
        .find_map(|(present, flag)| present.then_some(flag))
    }

    fn request(&self, session: &Session, settings: RenderSetup) -> Result<(Value, bool), String> {
        if let Some(job) = &settings.job {
            if let Some(flag) = self.explicit_flag() {
                return Err(format!("{flag} cannot accompany a typed saved render setup; change the saved job options instead"));
            }
            settings.validate().map_err(|e| e.to_string())?;
            session.doc.render.validate_resources(&settings).map_err(|e| e.to_string())?;
            let needs_gpu = matches!(job, RenderJob::GpuPath { .. });
            return Ok((json!({"settings":settings}), needs_gpu));
        }
        // Legacy setups and unnamed requests retain their runtime wire contract.
        settings.validate().map_err(|e| e.to_string())?;
        let mut params = json!({"settings":settings,"transparent_background":self.transparent,
            "progressive":self.progressive,"samples":self.samples,"max_depth":self.max_depth,"seed":self.seed});
        if let Some(path) = &self.lighting {
            let text = solidcraft_engine::io::vfs::read_to_string(path, 65536).map_err(|e| format!("could not read lighting: {e}"))?;
            let lighting: solidcraft_engine::render::job::StudioLighting = serde_json::from_str(&text).map_err(|e| e.to_string())?;
            params["lighting"] = json!(lighting);
        }
        if let Some(tone_map) = self.tone_map {
            params["tone_map"] = json!(tone_map);
        }
        if let Some(path) = &self.environment {
            let text = solidcraft_engine::io::vfs::read_to_string(path, 65536).map_err(|e| format!("could not read environment: {e}"))?;
            params["environment"] = serde_json::from_str(&text).map_err(|e| e.to_string())?;
        }
        Ok((params, self.progressive))
    }
}

pub fn run(args: &[String]) -> Result<(), String> {
    let input = args.first().filter(|p| !p.starts_with('-')).ok_or("render requires an input design or script")?;
    let mut out = None;
    let mut setup = None;
    let mut width = None;
    let mut height = None;
    let mut exposure = None;
    let mut runtime = RuntimeOptions::default();
    let mut overwrite = false;
    let mut timeout = 120u64;
    let mut i = 1;
    while let Some(arg) = args.get(i) {
        match arg.as_str() {
            "--progressive" => runtime.progressive = true,
            "--transparent" => runtime.transparent = true,
            "--overwrite" => overwrite = true,
            "--out" | "--setup" | "--width" | "--height" | "--timeout" | "--samples" | "--max-depth" | "--seed" | "--exposure" | "--lighting"
            | "--tone-map" | "--environment" => {
                i += 1;
                let value = args.get(i).ok_or_else(|| format!("missing value for {arg}"))?;
                match arg.as_str() {
                    "--out" => out = Some(value.clone()),
                    "--setup" => setup = Some(value.clone()),
                    "--width" => width = Some(value.parse::<u32>().map_err(|_| "invalid width")?),
                    "--height" => height = Some(value.parse::<u32>().map_err(|_| "invalid height")?),
                    "--samples" => runtime.samples = Some(value.parse::<u32>().map_err(|_| "invalid samples")?),
                    "--max-depth" => runtime.max_depth = Some(value.parse::<u32>().map_err(|_| "invalid depth")?),
                    "--seed" => runtime.seed = Some(value.parse::<u32>().map_err(|_| "invalid seed")?),
                    "--exposure" => exposure = Some(value.parse::<f64>().map_err(|_| "invalid exposure")?),
                    // Defer file reads until a saved job has rejected conflicting controls.
                    "--environment" => runtime.environment = Some(value.clone()),
                    "--lighting" => runtime.lighting = Some(value.clone()),
                    "--tone-map" => {
                        runtime.tone_map = Some(match value.as_str() {
                            "reinhard" => ToneMap::Reinhard,
                            "aces_fitted" => ToneMap::AcesFitted,
                            _ => return Err("tone-map must be reinhard or aces_fitted".into()),
                        })
                    }
                    _ => timeout = value.parse::<u64>().map_err(|_| "invalid timeout")?,
                }
            }
            _ => return Err(format!("unknown render option: {arg}")),
        }
        i += 1;
    }
    if !(1..=3600).contains(&timeout) {
        return Err("render timeout must be 1..3600 seconds".into());
    }
    let out = out.ok_or("render requires --out FILE.png")?;
    let mut session = crate::load(input)?;
    let mut settings = if let Some(name) = setup.as_deref() {
        session.doc.render.setups.get(name).cloned().ok_or("no saved render setup with that name")?
    } else {
        RenderSetup::default()
    };
    if let Some(width) = width {
        settings.width = width;
    }
    if let Some(height) = height {
        settings.height = height;
    }
    if let Some(exposure) = exposure {
        settings.exposure_ev = exposure;
    }
    // Resolve saved intent before opening a GPU or explicitly settling geometry.
    // Width/height/exposure only change this validated inline settings clone.
    let (mut params, needs_gpu) = runtime.request(&session, settings)?;
    session.set_render_job_host(Some(CpuHost::for_platform().map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    if needs_gpu {
        session.register_render_job_host(solidcraft_gpu::path::host().map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    }
    session.execute("render.scene", &json!({})).map_err(|e| e.to_string())?;
    if setup.is_none() {
        let camera = if let Some(camera) = session.camera {
            camera
        } else {
            let mut camera = RenderSetup::default().camera;
            camera.fit(&solidcraft_engine::product_scene::extract(&session).map_err(|e| e.to_string())?.bounds);
            camera
        };
        params["settings"]["camera"] = json!(camera);
    }
    let preflight = session.execute("render.preflight", &params).map_err(|e| e.to_string())?;
    let started = session.execute("render.start", &params).map_err(|e| e.to_string())?;
    let id: JobId = serde_json::from_value(started["job"].clone()).map_err(|e| e.to_string())?;
    let handle = session.render_job_handle(id).map_err(|e| e.to_string())?;
    let status = match handle.wait_timeout(std::time::Duration::from_secs(timeout)) {
        Ok(status) => status,
        Err(e) => {
            let _ = session.execute("render.cancel", &json!({"job":id}));
            return Err(e.to_string());
        }
    };
    if status.progress.phase != Phase::Completed {
        return Err(status.error.map_or_else(|| "render cancelled".into(), |e| e.to_string()));
    }
    let exported = session.execute("render.export", &json!({"job":id,"path":out,"overwrite":overwrite})).map_err(|e| e.to_string())?;
    println!("{}", crate::pretty(&json!({"preflight":preflight,"export":exported})));
    Ok(())
}
