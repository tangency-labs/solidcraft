//! Contracts and result types for the HDR environment commands in `cmd/render/resources.rs`
//! (#11761): check an HDR file, and copy it into a render setup. The handlers read these same
//! parameter types, so each contract is exactly what its handler takes; like the handlers, a
//! contract refuses unknown keys.
//!
//! `expected_sha256` is the doc crate's `HdrId`, which has no `JsonSchema` (the doc crate
//! doesn't depend on schemars), so its schema is written here by hand.

use schemars::{JsonSchema, Schema, SchemaGenerator, json_schema};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcraft_doc::render_resources::HdrId;

fn one() -> f64 {
    1.0
}

/// An HDR file's SHA-256, as `render.hdr.inspect` gives it: 64 lowercase hex digits.
fn digest_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "The file's SHA-256 as `render.hdr.inspect` gave it: 64 lowercase hex digits. A file whose bytes changed since is refused, so the saved setup uses the file the caller checked. Default: no check.",
        "type": ["string", "null"],
        "pattern": "^[0-9a-f]{64}$"
    })
}

/// The fields of a render setup, as `render.settings.get` gives them. A `set` patch may give up
/// to 13 of them; `patched_settings` reads the patched setup as a `RenderSetup`, which refuses any
/// other key.
const SETUP_FIELDS: &[&str] = &[
    "backend",
    "mode",
    "camera",
    "width",
    "height",
    "format",
    "exposure_ev",
    "environment_rotation",
    "environment_intensity",
    "background",
    "job",
    "surfaces",
    "optics",
    "stage",
];

/// Each setup field, without its own schema: `patched_settings` checks the values.
fn setup_properties() -> serde_json::Map<String, Value> {
    SETUP_FIELDS.iter().map(|field| (field.to_string(), Value::Object(serde_json::Map::new()))).collect()
}

/// `set`: changes to the setup, as `render.settings.set` takes them. `patched_settings` in
/// `cmd/render.rs` refuses anything but an object of at most 13 setup fields, with its own
/// message.
fn set_schema(_: &mut SchemaGenerator) -> Schema {
    json_schema!({
        "description": "Changes to the setup the HDR is saved into, as `render.settings.set` takes them: at most 13 of the setup's fields. The setup they make (a saved one keeps the fields left out) must have a `job` with method `cpu_raytrace` or `gpu_path`, without an `hdr` or an `environment`.",
        "type": "object",
        "properties": setup_properties(),
        "additionalProperties": false,
        "maxProperties": 13
    })
}

/// Read an HDR file and check it: its size in pixels, its byte count, its SHA-256 and the colour
/// primaries it declares. Nothing is saved, and the design doesn't change.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HdrInspectParams {
    /// The HDR file, in a folder the session may read: 1 to 4096 bytes, without control
    /// characters. At most 16 MiB.
    pub path: String,
}

/// Copy an HDR file's bytes into the design and save a render setup that lights the product with
/// them, in one undo step. The design keeps the bytes, so the setup still renders when the file
/// is gone. No render device is needed.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct HdrCaptureParams {
    /// The render setup, saved or new.
    pub name: String,
    #[schemars(schema_with = "set_schema")]
    pub set: Value,
    /// The HDR file, as `render.hdr.inspect` takes it.
    pub path: String,
    #[serde(default)]
    #[schemars(schema_with = "digest_schema")]
    pub expected_sha256: Option<HdrId>,
    /// The environment's turn about the up axis (radians): at most 1000000 in size. Default: 0.
    #[serde(default)]
    pub rotation_rad: f64,
    /// How bright the environment is, linear: 0 to 64 for the CPU raytracer, 0 to 100 for the GPU
    /// path tracer. Default: 1.
    #[serde(default = "one")]
    pub intensity: f64,
    /// The CPU raytracer's environment samples: 1, 4, 16 or 64. Default: 16. Refused for the GPU
    /// path tracer, even as `null`.
    #[serde(default)]
    pub samples: Option<u32>,
    /// The GPU path tracer only: treat a file that declares no primaries as linear sRGB. Default:
    /// false. Refused for the CPU raytracer, even as `null`; the CPU raytracer needs primaries the
    /// file declares.
    #[serde(default)]
    pub assume_linear_srgb: Option<bool>,
}

/// An HDR file as `render.hdr.inspect` reads it, and as `render.settings.capture_hdr` saves it.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HdrSourceInfo {
    /// Its SHA-256: 64 lowercase hex digits.
    pub sha256: String,
    /// Its size in pixels.
    pub width: u32,
    pub height: u32,
    /// The file's size in bytes.
    pub encoded_bytes: u64,
    /// The colour primaries the file declares (Rx, Ry, Gx, Gy, Bx, By, Wx, Wy); `null` when it
    /// declares none.
    pub primaries: Option<[f64; 8]>,
}

/// What `render.hdr.inspect` returns: the file's [`HdrSourceInfo`] at the top level.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HdrInspected {
    #[serde(flatten)]
    pub source: HdrSourceInfo,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// What `render.settings.capture_hdr` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct HdrCaptured {
    /// The render setup.
    pub name: String,
    /// The setup as saved, as the render settings commands give it, with its `hdr` set.
    pub setup: Value,
    /// The HDR the design now keeps.
    pub resource: HdrSourceInfo,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "render_hdr_tests.rs"]
mod tests;
