//! Explicit scoped capture into immutable document resources; no rendering or GPU required.

use std::sync::Arc;

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::{
    render_resources::{CpuHdrEnvironment, GpuHdrEnvironment, HdrId, HdrResource},
    render_settings::RenderJob,
};

use crate::{Result, Session, params::bad};

fn unit() -> f64 {
    1.0
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Inspect {
    path: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Capture {
    name: String,
    set: Value,
    path: String,
    #[serde(default)]
    expected_sha256: Option<HdrId>,
    #[serde(default)]
    rotation_rad: f64,
    #[serde(default = "unit")]
    intensity: f64,
    #[serde(default)]
    samples: Option<u32>,
    #[serde(default)]
    assume_linear_srgb: Option<bool>,
}

fn capture(path: &str, expected: Option<HdrId>, command: &str) -> Result<HdrResource> {
    if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
        return Err(bad(command, "HDR capture requires a bounded scoped path"));
    }
    let bytes = solidcraft_io::vfs::read(path, solidcraft_render::lighting::MAX_HDR_BYTES).map_err(|e| bad(command, e.to_string()))?;
    if expected.is_some_and(|id| id != HdrId::of(&bytes)) {
        return Err(bad(command, "HDR file changed since selection; inspect and select it again before saving"));
    }
    HdrResource::new(bytes).map_err(|e| bad(command, e.to_string()))
}

fn metadata(resource: &HdrResource) -> Value {
    let info = resource.info();
    json!({"sha256":resource.id(),"width":info.width,"height":info.height,"encoded_bytes":info.encoded_bytes,"primaries":info.primaries})
}

pub(super) fn inspect(_s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.hdr.inspect";
    let input: Inspect = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    capture(&input.path, None, command).map(|source| metadata(&source))
}

pub(super) fn capture_settings(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.settings.capture_hdr";
    let input: Capture = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    // Every numerical and method-dependent rejection happens before touching the source.
    match &setup.job {
        Some(RenderJob::CpuRaytrace { options }) => {
            if options.hdr.is_some() || options.environment.is_some() || params.get("assume_linear_srgb").is_some() {
                return Err(bad(command, "CPU HDR capture requires hdr/environment unset and declared primaries; assume_linear_srgb is GPU-only"));
            }
            CpuHdrEnvironment::check_controls(input.rotation_rad, input.intensity, input.samples.unwrap_or(16))
                .map_err(|e| bad(command, e.to_string()))?;
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            if options.hdr.is_some() || options.environment.is_some() || params.get("samples").is_some() {
                return Err(bad(command, "GPU HDR capture requires hdr/environment unset; environment samples are CPU-only"));
            }
            GpuHdrEnvironment::check_controls(input.rotation_rad, input.intensity).map_err(|e| bad(command, e.to_string()))?;
        }
        _ => return Err(bad(command, "HDR capture requires a typed cpu_raytrace or gpu_path setup")),
    }
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    let resource = capture(&input.path, input.expected_sha256, command)?;
    let info = metadata(&resource);
    let id = settings.resources.insert(resource).map_err(|e| bad(command, e.to_string()))?;
    match &mut setup.job {
        Some(RenderJob::CpuRaytrace { options }) => {
            options.hdr = Some(CpuHdrEnvironment {
                resource: id,
                rotation_rad: input.rotation_rad,
                intensity: input.intensity,
                samples: input.samples.unwrap_or(16),
            });
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            options.hdr = Some(GpuHdrEnvironment {
                resource: id,
                rotation_rad: input.rotation_rad,
                intensity: input.intensity,
                assume_linear_srgb: input.assume_linear_srgb.unwrap_or(false),
            });
        }
        _ => return Err(bad(command, "HDR capture requires a typed rendering method")),
    }
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"name":input.name,"setup":setup,"resource":info}))
}
