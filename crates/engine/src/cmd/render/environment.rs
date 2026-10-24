//! Runtime environment inputs. Captures bytes through the caller's scoped VFS.
use crate::{Result, params::bad};
use serde::Deserialize;
use solidcraft_doc::render_resources::{HdrId, HdrResources};
use solidcraft_render::lighting::{
    MAX_HDR_BYTES,
    environment::{EnvironmentResource, EnvironmentSource, SkyGradient},
};
use std::sync::Arc;
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
