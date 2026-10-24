use crate::{Result, Session, params::bad, render_jobs::job_error};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::render_settings::RenderSetup;
use solidcraft_render::{
    RenderOperation, RenderRequest,
    job::{ImageRequest, JobId},
};

#[derive(Default, Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Start {
    setup: Option<String>,
    settings: Option<RenderSetup>,
    transparent_background: bool,
    replace: bool,
}

fn prepare(s: &Session, params: &Value, command: &str) -> Result<(Start, solidcraft_render::ProductScene, ImageRequest, Value)> {
    let input: Start = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.setup.is_some() && input.settings.is_some() {
        return Err(bad(command, "choose either setup or settings"));
    }
    let scene = crate::product_scene::extract(s)?;
    let settings = if let Some(name) = &input.setup {
        s.doc.render.setups.get(name).cloned().ok_or_else(|| bad(command, "no saved render setup with that name"))?
    } else if let Some(settings) = &input.settings {
        settings.clone()
    } else {
        let mut settings = RenderSetup::default();
        if let Some(camera) = s.camera {
            settings.camera = camera;
        } else {
            settings.camera.fit(&scene.bounds);
        }
        settings
    };
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    // The CPU image executor has no exposure or environment controls yet. A setup saved with them
    // (the render workspace and other backends use them) still renders here, and the reply says
    // which of them this render leaves out (#5089).
    let ignored = ignored_controls(&settings);
    let host = s.render_job_host().ok_or_else(|| bad(command, "no image host is installed"))?;
    if !host.is_available() {
        return Err(bad(command, "image host is unavailable"));
    }
    let resolved = solidcraft_render::resolve_request(
        &[s.render_backend()],
        &RenderRequest {
            backend: Some(settings.backend.clone()),
            mode: settings.mode,
            operation: RenderOperation::Image,
            width: u64::from(settings.width),
            height: u64::from(settings.height),
            format: Some(settings.format),
            features: scene.required_features(),
            policy: Default::default(),
        },
    )
    .map_err(|e| bad(command, e.to_string()))?;
    if resolved.executor.as_str() != "cpu-product" {
        return Err(bad(command, "resolved executor is not installed as an image job host"));
    }
    let request = ImageRequest {
        camera: settings.camera,
        width: settings.width,
        height: settings.height,
        background: settings.background,
        transparent_background: input.transparent_background,
    };
    let admission = solidcraft_render::job::admit(&scene, &request).map_err(job_error)?;
    let reply = json!({"resolved":resolved,"admission":admission,"request":request,"scene_generation":s.revision,"host":host.kind(),"transparency":"sorted_alpha_approximation","refraction":false,"ignored_controls":ignored});
    Ok((input, scene, request, reply))
}

/// The setup's controls the CPU image executor doesn't apply: those not at their neutral value.
fn ignored_controls(settings: &RenderSetup) -> Vec<&'static str> {
    let mut ignored = Vec::new();
    if settings.exposure_ev != 0.0 {
        ignored.push("exposure_ev");
    }
    if settings.environment_rotation != 0.0 {
        ignored.push("environment_rotation");
    }
    if settings.environment_intensity != 1.0 {
        ignored.push("environment_intensity");
    }
    ignored
}

pub(super) fn preflight(s: &mut Session, params: &Value) -> Result<Value> {
    prepare(s, params, "render.preflight").map(|(_, _, _, reply)| reply)
}

pub(super) fn start(s: &mut Session, params: &Value) -> Result<Value> {
    let (input, scene, request, reply) = prepare(s, params, "render.start")?;
    let status = s.render_jobs.start(s.revision, scene, request, input.replace).map_err(job_error)?;
    let mut status = json!(status);
    if let (Some(out), Some(ignored)) = (status.as_object_mut(), reply.get("ignored_controls")) {
        out.insert("ignored_controls".into(), ignored.clone());
    }
    Ok(status)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Job {
    job: JobId,
}

pub(super) fn status(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Job = serde_json::from_value(params.clone()).map_err(|e| bad("render.status", e.to_string()))?;
    Ok(json!(s.render_jobs.status(input.job).map_err(job_error)?))
}

pub(super) fn cancel(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Job = serde_json::from_value(params.clone()).map_err(|e| bad("render.cancel", e.to_string()))?;
    let status = s.render_jobs.status(input.job).map_err(job_error)?;
    if status.progress.phase.terminal() {
        return Ok(json!(status));
    }
    Ok(json!(s.render_jobs.handle(input.job).map_err(job_error)?.cancel().map_err(job_error)?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Export {
    job: JobId,
    path: String,
    #[serde(default)]
    overwrite: bool,
}

pub(super) fn export(s: &mut Session, params: &Value) -> Result<Value> {
    let input: Export = serde_json::from_value(params.clone()).map_err(|e| bad("render.export", e.to_string()))?;
    let path = std::path::Path::new(&input.path);
    if input.path.len() > 4096 || input.path.chars().any(char::is_control) || !path.extension().is_some_and(|x| x.eq_ignore_ascii_case("png")) {
        return Err(bad("render.export", "a bounded PNG destination path is required"));
    }
    // Atomic native writers are also used for the app's own files and do not
    // apply a request's path scope. Check the caller's destination explicitly.
    solidcraft_io::vfs::check(&input.path).map_err(|e| bad("render.export", e.to_string()))?;
    let frame = s.render_jobs.frame(input.job).map_err(job_error)?;
    // Replace atomically without reading an arbitrarily large previous target into memory.
    // Design saves retain their independent backup policy.
    let result = if input.overwrite {
        solidcraft_io::write_atomic(path, frame.png(), false)
    } else {
        solidcraft_io::safe_write::create_atomic(path, frame.png())
    };
    result.map_err(|e| bad("render.export", format!("could not export PNG: {e}")))?;
    Ok(json!({"job":input.job,"path":input.path,"bytes":frame.png().len(),"frame":frame.info}))
}
