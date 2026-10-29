//! Reproducible evidence through the production command/job interfaces.
//! Usage: studio_evidence RECIPE.json RENDER-PARAMS.json OUTPUT.png
//! Writes the editable design, command timings and actual adapter/job metadata beside PNG.
//! Accepts runtime requests as well as inline or named saved render jobs.
use serde_json::{Value, json};
use solidcraft_engine::{
    Session,
    doc::render_settings::{RenderJob, RenderSetup},
    render::job::{CpuHost, ErrorCode, JobId, Phase},
};
use std::{
    io::Read,
    path::Path,
    time::{Duration, Instant},
};

fn read_json(path: &str) -> Result<Value, String> {
    let mut bytes = Vec::new();
    std::fs::File::open(path).map_err(|e| format!("{path}: {e}"))?.take(16_777_217).read_to_end(&mut bytes).map_err(|e| e.to_string())?;
    if bytes.len() > 16_777_216 {
        return Err("evidence input exceeds 16 MiB".into());
    }
    serde_json::from_slice(&bytes).map_err(|e| format!("{path}: {e}"))
}

// Only decide which optional host must be installed. The production command
// still validates and resolves the actual request without a second wire format.
fn needs_gpu(session: &Session, params: &Value) -> Result<bool, String> {
    let inline = params.get("settings").filter(|value| !value.is_null());
    let named = params.get("setup").filter(|value| !value.is_null());
    if inline.is_some() && named.is_some() {
        return Err("choose either setup or settings".into());
    }
    let inline_setup = inline
        .filter(|value| value.get("job").is_some_and(|job| !job.is_null()))
        .map(|value| serde_json::from_value::<RenderSetup>(value.clone()).map_err(|e| e.to_string()))
        .transpose()?;
    let setup = if let Some(name) = named.and_then(Value::as_str) {
        Some(session.doc.render.setups.get(name).ok_or("no saved render setup with that name")?)
    } else {
        inline_setup.as_ref()
    };
    if let Some(setup) = setup
        && let Some(job) = &setup.job
    {
        if params.as_object().is_some_and(|object| object.keys().any(|key| !matches!(key.as_str(), "setup" | "settings" | "replace"))) {
            return Err("saved render jobs cannot be mixed with runtime controls".into());
        }
        setup.validate().map_err(|e| e.to_string())?;
        return Ok(matches!(job, RenderJob::GpuPath { .. }));
    }
    Ok(params.get("progressive").and_then(Value::as_bool) == Some(true))
}

fn run() -> Result<(), String> {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [recipe_path, params_path, output_path] = args.as_slice() else {
        return Err("usage: studio_evidence RECIPE.json RENDER-PARAMS.json OUTPUT.png".into());
    };
    let output = Path::new(output_path);
    if !output.extension().is_some_and(|e| e.eq_ignore_ascii_case("png")) {
        return Err("output must have a .png extension".into());
    }
    let recipe = read_json(recipe_path)?;
    let params = read_json(params_path)?;
    let mut session = Session::default();
    eprintln!("Generating original CAD scene from {recipe_path}");
    let generation_start = Instant::now();
    let (commands, command_timings) = session.run_script_timed(&recipe, false).map_err(|e| e.to_string())?;
    let generation_seconds = generation_start.elapsed().as_secs_f64();
    let design_path = output.with_extension(solidcraft_engine::io::DESIGN_EXT);
    session.execute("file.save_as", &json!({"path":design_path})).map_err(|e| e.to_string())?;
    let scene = solidcraft_engine::product_scene::extract(&session).map_err(|e| e.to_string())?;
    let bodies: Vec<Value> = scene
        .meshes
        .iter()
        .map(|mesh| json!({"name":mesh.name,"vertices":mesh.mesh.positions.len(),"triangles":mesh.mesh.triangles.len()}))
        .collect();
    drop(scene);

    let cpu_host_start = Instant::now();
    session.set_render_job_host(Some(CpuHost::for_platform().map_err(|e| e.to_string())?)).map_err(|e| e.to_string())?;
    let cpu_host_seconds = cpu_host_start.elapsed().as_secs_f64();
    // Match runtime and saved-job dispatch. CPU evidence must not require a
    // Vulkan device, parse GPU policy or initialize a GPU host.
    let (adapter_report, gpu_host_seconds) = if needs_gpu(&session, &params)? {
        let gpu_host_start = Instant::now();
        let policy = solidcraft_gpu::select::Policy::from_env().map_err(|e| e.to_string())?;
        let (adapter, report) =
            solidcraft_gpu::select::pick_path(policy, |_| true).ok_or("no Vulkan or Metal adapter satisfies the requested GPU policy")?;
        session.register_render_job_host(solidcraft_gpu::path::host_on_adapter(&adapter).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
        (Some(report), Some(gpu_host_start.elapsed().as_secs_f64()))
    } else {
        (None, None)
    };
    let preflight = session.execute("render.preflight", &params).map_err(|e| e.to_string())?;
    let start = Instant::now();
    let started = session.execute("render.start", &params).map_err(|e| e.to_string())?;
    let executor = started
        .get("resolved")
        .and_then(|resolved| resolved.get("executor"))
        .and_then(Value::as_str)
        .ok_or("started job did not report the resolved image executor")?
        .to_owned();
    let id: JobId = serde_json::from_value(started.get("job").cloned().ok_or("missing job id")?).map_err(|e| e.to_string())?;
    let handle = session.render_job_handle(id).map_err(|e| e.to_string())?;
    eprintln!("Rendering through production {executor} executor");
    if let Some(adapter) = &adapter_report {
        eprintln!("GPU adapter: {adapter:?}");
    }
    let status = loop {
        match handle.wait_timeout(Duration::from_secs(10)) {
            Ok(status) => break status,
            Err(error) if error.code == ErrorCode::TimedOut && start.elapsed() < Duration::from_secs(3600) => {
                let status = handle.status().map_err(|e| e.to_string())?;
                eprintln!(
                    "Render progress: {:?} {}/{} after {:.1}s",
                    status.progress.phase,
                    status.progress.done,
                    status.progress.total,
                    start.elapsed().as_secs_f64()
                );
            }
            Err(error) => {
                let _ = handle.cancel();
                return Err(error.to_string());
            }
        }
    };
    let render_and_encode_seconds = start.elapsed().as_secs_f64();
    if status.progress.phase != Phase::Completed {
        return Err(status.error.map_or_else(|| "render did not complete".into(), |e| e.to_string()));
    }
    let exported = session.execute("render.export", &json!({"job":id,"path":output_path,"overwrite":true})).map_err(|e| e.to_string())?;
    let report = json!({
        "recipe":recipe_path,"render_params":params,"executor":executor,"adapter":adapter_report,
        "environment":{"SOLIDCRAFT_GPU":std::env::var("SOLIDCRAFT_GPU").ok(),"WGPU_BACKEND":std::env::var("WGPU_BACKEND").ok()},
        "cad_generation_seconds":generation_seconds,"cpu_host_setup_seconds":cpu_host_seconds,"gpu_host_setup_seconds":gpu_host_seconds,
        "render_and_encode_seconds":render_and_encode_seconds,
        "timing_scope":"wall time from render.start through completed PNG encoding; includes job preparation and readback, excludes CAD generation, preflight and initial CPU/GPU host setup; GPU setup timing includes adapter selection and pipelines when requested",
        "bodies":bodies,"commands":commands,"command_timings":command_timings,
        "preflight":preflight,"export":exported,"editable_design":design_path
    });
    std::fs::write(output.with_extension("json"), serde_json::to_vec_pretty(&report).map_err(|e| e.to_string())?).map_err(|e| e.to_string())?;
    println!("{}", serde_json::to_string_pretty(&report).map_err(|e| e.to_string())?);
    Ok(())
}

fn main() -> Result<(), String> {
    run()
}
