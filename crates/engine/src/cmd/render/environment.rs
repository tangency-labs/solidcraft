//! Runtime environment inputs. Captures bytes through the caller's scoped VFS.
use crate::{Result, params::bad};
use serde::Deserialize;
use solidcraft_doc::render_resources::{HdrId, HdrResources};
use solidcraft_render::lighting::{
    MAX_HDR_BYTES,
    environment::{EnvironmentResource, EnvironmentSource, SkyGradient},
};
use std::sync::Arc;

pub(super) const CONTRACT: crate::schema::TypedContract<crate::schema::render::EnvironmentParams> = crate::schema::TypedContract::new();

pub(super) fn set(s: &mut crate::Session, params: &serde_json::Value) -> Result<serde_json::Value> {
    use solidcraft_doc::render_settings::{RenderJob, SkyEnvironment};
    let command = "render.environment.set";
    let input: crate::schema::render::EnvironmentParams = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.rotation_deg.is_none() && input.intensity.is_none() {
        return Err(bad(command, "rotation_deg or intensity is required"));
    }
    let rotation = input
        .rotation_deg
        .map(|degrees| {
            if !degrees.is_finite() || degrees.abs() > 1e6 {
                Err(bad(command, "rotation must be finite degrees within +/-1000000"))
            } else {
                Ok(((degrees + 180.0).rem_euclid(360.0) - 180.0).to_radians())
            }
        })
        .transpose()?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    let (angle, intensity, maximum) = match &mut setup.job {
        Some(RenderJob::CpuRaytrace { options }) => {
            if let Some(hdr) = &mut options.hdr {
                (Some(&mut hdr.rotation_rad), &mut hdr.intensity, 64.0)
            } else if let Some(environment) = &mut options.environment {
                (None, &mut environment.intensity, 64.0)
            } else {
                return Err(bad(command, "choose an environment before editing it"));
            }
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            if let Some(hdr) = &mut options.hdr {
                (Some(&mut hdr.rotation_rad), &mut hdr.intensity, 100.0)
            } else if let Some(SkyEnvironment::Sky { rotation_rad, intensity, .. }) = &mut options.environment {
                (Some(rotation_rad), intensity, 100.0)
            } else {
                return Err(bad(command, "choose an environment before editing it"));
            }
        }
        _ => return Err(bad(command, "environment editing requires a typed raytrace setup")),
    };
    if let Some(value) = input.intensity {
        if !value.is_finite() || !(0.0..=maximum).contains(&value) {
            return Err(bad(command, format!("intensity must be finite and within 0..{maximum}")));
        }
        *intensity = value;
    }
    if let Some(value) = rotation {
        *angle.ok_or_else(|| bad(command, "a constant environment has no rotation"))? = value;
    }
    super::set_settings(s, &serde_json::json!({"name":input.name,"set":setup}))
}
fn unit() -> f64 {
    1.0
}
#[derive(Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub(super) enum Input {
    BuiltinHdr {
        id: String,
        #[serde(default)]
        rotation_rad: f64,
        #[serde(default = "unit")]
        intensity: f64,
    },
    Hdr {
        path: String,
        #[serde(default)]
        expected_sha256: Option<HdrId>,
        #[serde(default)]
        rotation_rad: f64,
        #[serde(default = "unit")]
        intensity: f64,
        #[serde(default)]
        assume_linear_srgb: bool,
    },
    EmbeddedHdr {
        resource: HdrId,
        #[serde(default)]
        rotation_rad: f64,
        #[serde(default = "unit")]
        intensity: f64,
        #[serde(default)]
        assume_linear_srgb: bool,
    },
    Sky {
        #[serde(default)]
        gradient: SkyGradient,
        #[serde(default)]
        rotation_rad: f64,
        #[serde(default = "unit")]
        intensity: f64,
    },
}
impl Input {
    pub(super) fn capture(&self, resources: &HdrResources, command: &str) -> Result<EnvironmentResource> {
        let source = match self {
            Self::BuiltinHdr { id, rotation_rad, intensity } => {
                let resource = super::library::hdr_resource(id, command)?;
                EnvironmentSource::from_hdr(resource.source(), super::library::hdr_rotation(*rotation_rad, command)?, *intensity, false)
            }
            Self::Hdr { path, expected_sha256, rotation_rad, intensity, assume_linear_srgb } => {
                if path.is_empty() || path.len() > 4096 || path.chars().any(char::is_control) {
                    return Err(bad(command, "bounded HDR path required"));
                }
                let bytes = solidcraft_io::vfs::read(path, MAX_HDR_BYTES).map_err(|e| bad(command, format!("could not capture HDR: {e}")))?;
                if expected_sha256.is_some_and(|id| id != HdrId::of(&bytes)) {
                    return Err(bad(command, "HDR file changed since selection; inspect and select it again"));
                }
                EnvironmentSource::hdr(bytes, *rotation_rad, *intensity, *assume_linear_srgb)
            }
            Self::EmbeddedHdr { resource, rotation_rad, intensity, assume_linear_srgb } => {
                let source = resources.required(resource).map_err(|e| bad(command, e.to_string()))?;
                EnvironmentSource::from_hdr(source.source(), *rotation_rad, *intensity, *assume_linear_srgb)
            }
            Self::Sky { gradient, rotation_rad, intensity } => EnvironmentSource::sky(*gradient, *rotation_rad, *intensity),
        }
        .map_err(|e| bad(command, e.to_string()))?;
        Ok(Arc::new(source))
    }
}
