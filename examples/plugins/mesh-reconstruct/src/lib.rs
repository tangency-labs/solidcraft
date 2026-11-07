//! Separately compiled command plugin using the reconstruction implementation from PR #8061.
//! The host receives an ordinary embedded STEP Import, never a plugin feature or executable.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use serde::Deserialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use solidcraft_geom::Vec3;

#[cfg(target_arch = "wasm32")]
mod component;

pub const ALGORITHM: &str = "solidcraft-mesh-reconstruction-8061-v7";
/// Exact build-time SDK revision pinned by the standalone Cargo manifest.
pub const SDK_REV: &str = "aab1df183a7b4ca7c3d9e4dd5b52a6b8a2c64e4f";
/// Bump when reconstruction face enumeration changes, even for equal geometry.
pub const FACE_ORDER_VERSION: &str = "reconstruction-face-order-1";
pub const MAX_JSON_BYTES: usize = 1024 * 1024;

/// The exact command interface imported by the command-plugin WIT world.
/// This abstraction also lets native tests use the real engine without a Wasm runtime.
pub trait Host {
    fn run(&mut self, command: &str, params: Value) -> Result<Value, String>;
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Params {
    body: String,
    #[serde(default = "tolerance")]
    tolerance: f64,
    name: Option<String>,
}
fn tolerance() -> f64 {
    0.05
}

#[derive(Deserialize)]
struct MeshReply {
    body: String,
    component: u64,
    frame: String,
    positions: Vec<[f64; 3]>,
    triangles: Vec<[u32; 3]>,
}

fn plain(s: &str) -> bool {
    !s.trim().is_empty() && s.chars().count() <= 128 && !s.chars().any(char::is_control)
}

/// Recognize and reconstruct fully before asking the host to change the design.
/// The source data remains in history; plugin.run supplies atomic undo/cancellation.
pub fn execute(host: &mut impl Host, entry: &str, params: Value) -> Result<Value, String> {
    if entry != "convert" {
        return Err("unknown entry".into());
    }
    let params: Params = serde_json::from_value(params).map_err(|e| format!("invalid parameters: {e}"))?;
    if !plain(&params.body) || params.name.as_deref().is_some_and(|s| !plain(s) || s == params.body.as_str()) {
        return Err("body and optional name must be 1..128 plain characters; the new name must differ from the input".into());
    }
    if !params.tolerance.is_finite() || !(1e-6..=1.).contains(&params.tolerance) {
        return Err("tolerance must be between 0.000001 and 1 mm".into());
    }
    let reply = host.run("model.mesh", json!({"body": params.body, "tolerance":params.tolerance}))?;
    let mesh: MeshReply = serde_json::from_value(reply).map_err(|e| format!("invalid mesh reply: {e}"))?;
    if mesh.body != params.body || mesh.frame != "component-local-mm" {
        return Err("host mesh identity or coordinate frame does not match".into());
    }
    if mesh.positions.is_empty() || mesh.positions.len() > 8192 || mesh.triangles.is_empty() || mesh.triangles.len() > 16384 {
        return Err("mesh exceeds the plugin transfer budget or is empty".into());
    }
    if mesh.positions.iter().flatten().any(|v| !v.is_finite() || v.abs() > 100_000.)
        || mesh.triangles.iter().flatten().any(|i| *i as usize >= mesh.positions.len())
    {
        return Err("mesh has non-finite, out-of-range coordinates or invalid vertex indices".into());
    }
    // Source identity, numeric parameters and algorithm version all bind the stored STEP.
    // Any future picks/all_surfaces parameters must be included before they are exposed.
    let mut hash = Sha256::new();
    hash.update(b"solidcraft-mesh-source-identity-v2\0");
    for identity in [ALGORITHM, SDK_REV, FACE_ORDER_VERSION, mesh.body.as_str()] {
        hash.update((identity.len() as u64).to_le_bytes());
        hash.update(identity.as_bytes());
    }
    hash.update(mesh.component.to_le_bytes());
    hash.update(params.tolerance.to_le_bytes());
    hash.update((mesh.positions.len() as u64).to_le_bytes());
    for p in &mesh.positions {
        for v in p {
            hash.update(v.to_le_bytes());
        }
    }
    hash.update((mesh.triangles.len() as u64).to_le_bytes());
    for t in &mesh.triangles {
        for i in t {
            hash.update(i.to_le_bytes());
        }
    }
    let fingerprint = format!("{:x}", hash.finalize());
    let positions: Vec<_> = mesh.positions.iter().map(|p| Vec3::new(p[0], p[1], p[2])).collect();
    let input = solidcraft_kernel::mesh_body(&positions, &mesh.triangles).map_err(|e| e.to_string())?;
    let solid = solidcraft_kernel::convert_prismatic_mesh(&input, params.tolerance).map_err(|e| e.to_string())?;
    // The SDK certifies this exact returned body, including the full measure,
    // before returning it. Repeating validity here remeshes an unchanged solid.
    if solid.is_mesh() {
        return Err("reconstruction did not produce an analytic solid".into());
    }
    let step = solidcraft_kernel::step_export(&[&solid], &format!("{ALGORITHM}:{fingerprint}")).map_err(|e| e.to_string())?;
    let name = params.name.unwrap_or_else(|| format!("{} Solid", params.body));
    let command = json!({"feature": {
        "type":"import", "name":name, "body_names":[name], "component":mesh.component,
        "file":format!("mesh-reconstruct-{fingerprint}.step"), "step":step
    }});
    if serde_json::to_vec(&command).map_err(|e| e.to_string())?.len() > MAX_JSON_BYTES {
        return Err("reconstructed STEP exceeds the host's 1 MiB command budget".into());
    }
    let adopted = host.run("timeline.add", command)?;
    if let Some(error) = adopted.get("error").filter(|v| !v.is_null()) {
        return Err(format!("host rejected reconstruction: {error}"));
    }
    // The original MeshImport data remains in document history. Retire only its
    // current body after the solid has been created, so default STEP export no
    // longer includes a mesh. The host adopts both writes atomically.
    let retired = host.run(
        "timeline.add",
        json!({"feature": {
            "type":"remove", "name":format!("Replace {}", params.body),
            "bodies":[params.body], "component":mesh.component
        }}),
    )?;
    if let Some(error) = retired.get("error").filter(|v| !v.is_null()) {
        return Err(format!("host rejected source retirement: {error}"));
    }
    Ok(json!({"feature":adopted.get("feature"),"name":adopted.get("name"),"source_body":params.body,
        "source_fingerprint":fingerprint,"algorithm":ALGORITHM,"sdk_revision":SDK_REV,"face_order_version":FACE_ORDER_VERSION,"rebuild":"recorded-step","source_retained":true,"source_storage":"timeline-input","source_body_replaced":true}))
}

#[cfg(test)]
mod tests;
