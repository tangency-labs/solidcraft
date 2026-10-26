//! Built-in authored resources and explicit runtime projections. The source catalogs
//! retain their original authoring status; unsupported concepts never become fake presets.
use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::{
    render_resources::{CpuHdrEnvironment, GpuHdrEnvironment, HdrId, HdrResource},
    render_settings::RenderJob,
};
use solidcraft_render::{
    job::{AreaLight, AreaLights, StudioLighting, Sun},
    texture_input::MapRole,
};

use crate::{Result, Session, params::bad};

const KINDS: [&str; 12] = [
    "materials",
    "lighting",
    "environments",
    "surfaces",
    "surface_manifest",
    "selene_appearances",
    "selene_shots",
    "hdr",
    "rigs",
    "scenes",
    "quality",
    "capabilities",
];
static CATALOGS: OnceLock<std::result::Result<BTreeMap<&'static str, Value>, String>> = OnceLock::new();

fn catalog(kind: &str, command: &str) -> Result<&'static [Value]> {
    if kind == "native_scenes" {
        static NATIVE: OnceLock<Value> = OnceLock::new();
        return NATIVE
            .get_or_init(super::studio_presets::entries)
            .as_array()
            .map(Vec::as_slice)
            .ok_or_else(|| bad(command, "native studio entries missing"));
    }
    let cached = CATALOGS.get_or_init(|| {
        let mut catalogs = BTreeMap::new();
        for kind in KINDS {
            let source = super::library_data::catalog(kind).ok_or_else(|| format!("missing built-in catalog {kind}"))?;
            let value: Value = serde_json::from_str(source).map_err(|e| format!("invalid built-in catalog {kind}: {e}"))?;
            if !value.get("items").is_some_and(Value::is_array) {
                return Err(format!("built-in catalog {kind} has no items"));
            }
            catalogs.insert(kind, value);
        }
        Ok(catalogs)
    });
    let catalogs = cached.as_ref().map_err(|e| bad(command, e.clone()))?;
    catalogs
        .get(kind)
        .and_then(|v| v.get("items"))
        .and_then(Value::as_array)
        .map(Vec::as_slice)
        .ok_or_else(|| bad(command, "unknown built-in catalog kind"))
}

fn bounded_id(id: &str, command: &str) -> Result<()> {
    if id.is_empty() || id.len() > 128 || !id.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_') {
        return Err(bad(command, "resource identifier must contain 1..128 ASCII letters, digits, hyphens or underscores"));
    }
    Ok(())
}

pub(super) fn item(kind: &str, id: &str, command: &str) -> Result<&'static Value> {
    bounded_id(id, command)?;
    catalog(kind, command)?
        .iter()
        .find(|v| v.get("id").and_then(Value::as_str) == Some(id))
        .ok_or_else(|| bad(command, format!("unknown {kind} resource {id}")))
}

fn projection(kind: &str, value: &Value) -> Value {
    match kind {
        "native_scenes" => json!({"status":"native_composition","executor":"gpu-path","variants":["review_then_apply"],
            "preserves":["camera","materials","geometry","engineering_properties"],"requires_subject_and_explicit_replacement_consent":true}),
        "surfaces" => {
            json!({"status":"implemented_pending_batch_validation","variants":if cfg!(feature="bundled-render-library") {vec!["uniform_opaque","gpu_planar_maps"]} else {vec!["uniform_opaque"]},
            "omitted":["height_displacement","true_anisotropic_brdf","manufacturing_geometry"],"maps_portability":"version_and_digest_pinned_builtin_setup_bindings"})
        }
        "selene_appearances" => json!({"status":"implemented_pending_batch_validation","variants":["uniform_opaque"],
            "omitted":value["baseline_projection"]["omitted_target_features"]}),
        "hdr" if !cfg!(feature = "bundled-render-library") => {
            json!({"status":"blocked","reason":"optional bundled-render-library pack is not included in this build","variants":[]})
        }
        "hdr" => json!({"status":"implemented_pending_batch_validation","variants":["embedded_hdr"],
            "author_to_native_rotation_offset_rad":std::f64::consts::PI,"geometry_included":false}),
        "rigs" => {
            let state = value["state"].as_str().unwrap_or("unknown");
            let cards = value["modifiers"].as_array().map_or(0, Vec::len);
            if state != "authored_numeric_unbound" {
                json!({"status":"blocked","reason":state,"source_state_preserved":true})
            } else {
                json!({"status":"implemented_pending_batch_validation","variants":if cards == 0 {vec!["complete"]} else {vec!["complete","emitters_only"]},
                    "passive_cards":cards,"requires_explicit_scale_and_placement":true,"executor":"gpu-path"})
            }
        }
        _ => json!({"status":"specification_only","reason":"requires capability and target binding; not an executable complete preset"}),
    }
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct Query {
    kind: String,
    query: String,
    offset: usize,
    limit: usize,
    availability: String,
}
impl Default for Query {
    fn default() -> Self {
        Self { kind: "surfaces".into(), query: String::new(), offset: 0, limit: 48, availability: "all".into() }
    }
}

pub(super) fn list(_s: &mut Session, params: &Value) -> Result<Value> {
    if params.get("kind").and_then(Value::as_str) == Some("physical_profiles") {
        return super::physical_library::list(params);
    }
    let command = "render.library.list";
    let input: Query = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.query.len() > 256 || input.query.chars().any(char::is_control) || input.limit == 0 || input.limit > 128 || input.offset > 4096 {
        return Err(bad(command, "query is bounded to 256 bytes, page size to 1..128 and offset to 4096"));
    }
    if !matches!(input.availability.as_str(), "all" | "usable" | "specification" | "blocked") {
        return Err(bad(command, "unknown availability filter"));
    }
    let query = input.query.to_lowercase();
    let matches = |v: &&Value| {
        let state = projection(&input.kind, v);
        let available = state["status"].as_str().unwrap_or("specification_only");
        let admitted = match input.availability.as_str() {
            "usable" => matches!(available, "implemented_pending_batch_validation" | "native_composition"),
            "blocked" => available == "blocked",
            "specification" => available == "specification_only",
            _ => true,
        };
        admitted
            && [
                v.get("id"),
                v.get("name"),
                v.get("family"),
                v.get("description"),
                v.get("domains"),
                v.get("process"),
                v.get("intended_use"),
                v.get("tags"),
            ]
            .into_iter()
            .flatten()
            .any(|field| field.to_string().to_lowercase().contains(&query))
    };
    let values = catalog(&input.kind, command)?;
    let count = values.iter().filter(matches).count();
    let items: Vec<_> = values.iter().filter(matches).skip(input.offset).take(input.limit).map(|v| {
        json!({"id":v["id"],"name":v["name"],"domains":v["domains"],"family":v["family"],"source_status":v.get("status").or_else(|| v.get("state")),"projection":projection(&input.kind,v)})
    }).collect();
    let end = input.offset.saturating_add(items.len());
    Ok(json!({"kind":input.kind,"total":count,"offset":input.offset,"next_offset":(end < count).then_some(end),"items":items}))
}

#[derive(Deserialize)]
#[serde(default, deny_unknown_fields)]
struct TargetQuery {
    query: String,
    offset: usize,
    limit: usize,
}
impl Default for TargetQuery {
    fn default() -> Self {
        Self { query: String::new(), offset: 0, limit: 128 }
    }
}
pub(super) fn targets(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.targets";
    let input: TargetQuery = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.query.len() > 256
        || input.query.chars().any(char::is_control)
        || input.limit == 0
        || input.limit > 128
        || input.offset > solidcraft_render::job::MAX_CPU_BODIES
    {
        return Err(bad(command, "bounded target search and page size 1..128 required"));
    }
    let query = input.query.to_lowercase();
    let scene = crate::product_scene::extract(s)?;
    let count = scene.meshes.iter().filter(|body| body.name.to_lowercase().contains(&query)).count();
    let items: Vec<_> = scene
        .meshes
        .iter()
        .enumerate()
        .filter(|(_, body)| body.name.to_lowercase().contains(&query))
        .skip(input.offset)
        .take(input.limit)
        .map(|(ordinal, body)| {
            json!({"ordinal":ordinal,"name":body.name,"color":body.color,"opacity":body.opacity,
            "pbr":body.surface.map(|v|json!({"metallic":v.metallic(),"roughness":v.roughness(),"dielectric_ior":v.dielectric_ior()}))})
        })
        .collect();
    let end = input.offset.saturating_add(items.len());
    Ok(json!({"items":items,"total":count,"offset":input.offset,"next_offset":(end<count).then_some(end),"revision":s.revision,
        "bounds":if scene.bounds.is_empty(){Value::Null}else{json!({"min":scene.bounds.min,"max":scene.bounds.max})}}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Identify {
    kind: String,
    id: String,
}

pub(super) fn describe(_s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.describe";
    let input: Identify = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.kind == "physical_profiles" {
        return super::physical_library::describe(&input.id);
    }
    let value = item(&input.kind, &input.id, command)?;
    let mut reply = json!({"kind":input.kind,"resource":value,"projection":projection(&input.kind,value),"source_manifest_unchanged":true});
    if matches!(input.kind.as_str(), "materials" | "surfaces") {
        reply["physical_references"] = super::physical_library::associations(&input.id)?;
    }
    Ok(reply)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Preview {
    id: String,
}
pub(super) fn preview(_s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.preview";
    let input: Preview = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    item("surfaces", &input.id, command)?;
    let bytes = asset_bytes(&format!("{}/base_color", input.id), 2 * 1024 * 1024, command)?;
    Ok(json!({"id":input.id,"mime":"image/png","sha256":HdrId::of(&bytes),"bytes":bytes,
        "fidelity":"authored_base_color_source_map_not_a_render","document_changed":false}))
}

/// The key is matched against generated constants, never joined to a filesystem path.
pub(super) fn require_bundled(command: &str) -> Result<()> {
    if !cfg!(feature = "bundled-render-library") {
        return Err(bad(command, "this build does not include the optional bundled-render-library texture/HDR pack"));
    }
    Ok(())
}

fn asset_bytes(key: &str, maximum: u64, command: &str) -> Result<Vec<u8>> {
    require_bundled(command)?;
    let (bytes, expected) = super::library_data::asset(key).ok_or_else(|| bad(command, "unknown built-in asset"))?;
    if bytes.len() as u64 > maximum || HdrId::of(bytes).to_string() != expected {
        return Err(bad(command, "built-in asset exceeds admission or does not match its published SHA-256"));
    }
    let mut owned = Vec::new();
    owned.try_reserve_exact(bytes.len()).map_err(|_| bad(command, "could not reserve built-in resource bytes"))?;
    owned.extend_from_slice(bytes);
    if owned.capacity() as u64 > maximum {
        return Err(bad(command, "built-in resource capacity exceeds admission"));
    }
    Ok(owned)
}

pub(super) fn texture_bytes(key: &str, role: MapRole, maximum: u64, command: &str) -> Result<Vec<u8>> {
    let (id, channel) = key.split_once('/').ok_or_else(|| bad(command, "built-in texture requires SURF-id/channel"))?;
    item("surfaces", id, command)?;
    let expected = match role {
        MapRole::BaseColor => "base_color",
        MapRole::NormalOpenGl => "normal_open_gl",
        MapRole::Roughness => "roughness",
        MapRole::Metallic => "metallic",
        _ => return Err(bad(command, "this authored channel has no runtime consumer")),
    };
    if channel != expected {
        return Err(bad(command, "built-in texture channel does not match its explicit map role"));
    }
    asset_bytes(key, maximum, command)
}

pub(super) fn owns_sun(id: &str, command: &str) -> Result<bool> {
    Ok(item("hdr", id, command)?["solar_disk"] == true)
}

pub(super) fn hdr_resource(id: &str, command: &str) -> Result<HdrResource> {
    item("hdr", id, command)?;
    HdrResource::new(asset_bytes(id, solidcraft_render::lighting::MAX_HDR_BYTES as u64, command)?).map_err(|e| bad(command, e.to_string()))
}

pub(super) fn hdr_rotation(author_rotation: f64, command: &str) -> Result<f64> {
    if !author_rotation.is_finite() || author_rotation.abs() > 1e6 {
        return Err(bad(command, "authored HDR rotation must be finite and within one million radians"));
    }
    Ok((author_rotation + std::f64::consts::PI).rem_euclid(std::f64::consts::TAU))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Appearance {
    id: String,
    variant: String,
    bodies: Vec<String>,
}

fn triple(value: &Value, command: &str) -> Result<[f64; 3]> {
    let v: [f64; 3] = serde_json::from_value(value.clone()).map_err(|e| bad(command, e.to_string()))?;
    if v.iter().any(|x| !x.is_finite()) {
        return Err(bad(command, "nonfinite authored vector"));
    }
    Ok(v)
}
fn number(value: &Value, command: &str) -> Result<f64> {
    value.as_f64().filter(|n| n.is_finite()).ok_or_else(|| bad(command, "missing finite authored value"))
}
pub(super) fn apply_appearance(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.appearance.apply";
    let input: Appearance = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.variant != "uniform_opaque" || input.bodies.is_empty() || input.bodies.len() > 256 {
        return Err(bad(command, "explicit uniform_opaque variant and 1..256 body targets are required"));
    }
    if input.bodies.iter().any(|name| name.is_empty() || name.len() > 1024 || name.chars().any(char::is_control)) {
        return Err(bad(command, "bounded exact body names required"));
    }
    let (value, mut look, omitted) = if input.id.starts_with("SURF-") {
        let value = item("surfaces", &input.id, command)?;
        let lo = triple(&value["base_linear_low"], command)?;
        let hi = triple(&value["base_linear_high"], command)?;
        let color = std::array::from_fn::<_, 3, _>(|i| crate::geom::srgb::encode8((lo[i] + hi[i]) * 0.5));
        (
            value,
            json!({"color":color,"opacity":1.0,"pbr":{"metallic":value["metallic"],"roughness":value["roughness"],"dielectric_ior":1.5}}),
            json!(["spatial_maps", "height_displacement", "true_anisotropic_brdf"]),
        )
    } else {
        let value = item("selene_appearances", &input.id, command)?;
        let p = &value["baseline_projection"]["runtime_parameters"];
        (value, json!({"color":p["color"],"opacity":p["opacity"],"pbr":p["pbr"]}), value["baseline_projection"]["omitted_target_features"].clone())
    };
    let name = value["name"].as_str().ok_or_else(|| bad(command, "authored appearance has no name"))?;
    look["name"] = json!(format!("{} {}", input.id, name).chars().take(128).collect::<String>());
    look["bodies"] = json!(input.bodies);
    let assigned = crate::cmd::inspect::appearance(s, &look)?;
    Ok(
        json!({"id":input.id,"variant":input.variant,"omitted":omitted,"appearance":look,"assignment":assigned,"engineering_properties_unchanged":true}),
    )
}

fn one() -> f64 {
    1.0
}
fn samples() -> u32 {
    16
}
fn empty_patch() -> Value {
    json!({})
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct EnvironmentApply {
    id: String,
    name: String,
    #[serde(default = "empty_patch")]
    set: Value,
    #[serde(default)]
    rotation_rad: f64,
    #[serde(default = "one")]
    intensity: f64,
    #[serde(default = "samples")]
    samples: u32,
    #[serde(default)]
    replace_environment: bool,
}

fn has_environment(setup: &solidcraft_doc::render_settings::RenderSetup) -> bool {
    match &setup.job {
        Some(RenderJob::CpuRaytrace { options }) => options.hdr.is_some() || options.environment.is_some(),
        Some(RenderJob::GpuPath { options, .. }) => options.hdr.is_some() || options.environment.is_some(),
        _ => false,
    }
}
pub(super) fn apply_environment(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.environment.apply";
    let input: EnvironmentApply = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    // Consent is checked against the existing authority, not merely a patch that
    // can replace the entire job object and thereby hide its previous environment.
    if !input.replace_environment && s.doc.render.setups.get(&input.name).is_some_and(has_environment) {
        return Err(bad(command, "existing environment requires replace_environment: true"));
    }
    let metadata = item("hdr", &input.id, command)?;
    let rotation = hdr_rotation(input.rotation_rad, command)?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    match &setup.job {
        Some(RenderJob::CpuRaytrace { options }) => {
            if !input.replace_environment && (options.hdr.is_some() || options.environment.is_some()) {
                return Err(bad(command, "existing environment requires replace_environment: true"));
            }
            CpuHdrEnvironment::check_controls(rotation, input.intensity, input.samples).map_err(|e| bad(command, e.to_string()))?;
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            if !input.replace_environment && (options.hdr.is_some() || options.environment.is_some()) {
                return Err(bad(command, "existing environment requires replace_environment: true"));
            }
            if metadata["solar_disk"] == true && options.lighting.as_ref().is_some_and(|l| l.sun.is_some()) {
                return Err(bad(command, "the selected HDR already owns the sun; remove the analytic sun explicitly"));
            }
            GpuHdrEnvironment::check_controls(rotation, input.intensity).map_err(|e| bad(command, e.to_string()))?;
        }
        _ => return Err(bad(command, "choose a typed CPU or GPU job before applying an HDR")),
    }
    let mut settings = (*s.doc.render).clone();
    let resource = hdr_resource(&input.id, command)?;
    let digest = resource.id();
    settings.resources.insert(resource).map_err(|e| bad(command, e.to_string()))?;
    match &mut setup.job {
        Some(RenderJob::CpuRaytrace { options }) => {
            options.environment = None;
            options.hdr = Some(CpuHdrEnvironment { resource: digest, rotation_rad: rotation, intensity: input.intensity, samples: input.samples });
        }
        Some(RenderJob::GpuPath { options, .. }) => {
            options.environment = None;
            options.hdr = Some(GpuHdrEnvironment { resource: digest, rotation_rad: rotation, intensity: input.intensity, assume_linear_srgb: false });
        }
        _ => return Err(bad(command, "typed rendering method disappeared")),
    }
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"id":input.id,"name":input.name,"setup":setup,"resource":digest,"author_rotation_rad":input.rotation_rad,
        "native_rotation_rad":rotation,"orientation_offset_rad":std::f64::consts::PI,"self_contained":true,"source_sha256":metadata["sha256"]}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct SurfacePrepare {
    id: String,
    body_index: usize,
    body_name: String,
    #[serde(default)]
    origin: Option<[f64; 3]>,
    #[serde(default = "one")]
    scale: f64,
    #[serde(default)]
    plane: Plane,
}
#[derive(Default, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Plane {
    #[default]
    Xy,
    Yz,
    Zx,
}

pub(super) fn prepare_surface(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.surface.prepare";
    let input: SurfacePrepare = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let value = item("surfaces", &input.id, command)?;
    if !input.scale.is_finite() || !(0.001..=1000.0).contains(&input.scale) {
        return Err(bad(command, "surface repeat scale must be finite in 0.001..1000"));
    }
    let tile: [f64; 2] = serde_json::from_value(value["tile_mm"].clone()).map_err(|e| bad(command, e.to_string()))?;
    let (u, v) = (tile[0] * input.scale, tile[1] * input.scale);
    let (u, v) = match input.plane {
        Plane::Xy => ([u, 0.0, 0.0], [0.0, v, 0.0]),
        Plane::Yz => ([0.0, u, 0.0], [0.0, 0.0, v]),
        Plane::Zx => ([0.0, 0.0, u], [v, 0.0, 0.0]),
    };
    let scene = crate::product_scene::extract(s)?;
    let body = scene
        .meshes
        .get(input.body_index)
        .filter(|b| b.name == input.body_name)
        .ok_or_else(|| bad(command, "body ordinal and exact name must match render.scene"))?;
    if body.opacity != 1.0 || body.transmission.is_some() || body.faces.iter().any(|f| f.opacity != 1.0 || f.transmission.is_some()) {
        return Err(bad(command, "authored planar maps require an opaque target"));
    }
    let center = body.mesh.bounds().center();
    let origin = input.origin.unwrap_or([center.x, center.y, center.z]);
    let frame = solidcraft_render::job::textures::PlanarFrame { origin, u, v };
    frame.validate().map_err(|e| bad(command, e.to_string()))?;
    let manifest = item("surface_manifest", &input.id, command)?;
    Ok(json!({"id":input.id,"revision":s.revision,"executor":"gpu-path","variant":"gpu_planar_maps",
        "texture":{"body_index":input.body_index,"body_name":input.body_name,"frame":frame,"normal_strength":1.0,"flip_v":true,"base_color_mode":"replace",
            "base_color_srgb":format!("builtin://{}/base_color",input.id),"normal_opengl":format!("builtin://{}/normal_open_gl",input.id),
            "roughness_linear":format!("builtin://{}/roughness",input.id),"metallic_linear":format!("builtin://{}/metallic",input.id)},
        "source_maps":manifest["maps"],"omitted":["height_displacement","true_anisotropic_brdf","manufacturing_geometry"],
        "dielectric_ior":"captured_body_value","persistence":"transient_request_binding; authored source bytes are built in"}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(super) struct RigInput {
    pub(super) id: String,
    pub(super) variant: String,
    pub(super) center_mm: [f64; 3],
    pub(super) subject_extent_mm: f64,
}

pub(super) fn rig_lighting(input: &RigInput, command: &str) -> Result<(StudioLighting, Value)> {
    let value = item("rigs", &input.id, command)?;
    if value["state"] != "authored_numeric_unbound" {
        return Err(bad(command, "rig requires unresolved calibration or an existing external sun"));
    }
    if input.center_mm.iter().any(|x| !x.is_finite() || x.abs() > 1e12)
        || !input.subject_extent_mm.is_finite()
        || !(0.001..=1e6).contains(&input.subject_extent_mm)
    {
        return Err(bad(command, "explicit bounded subject center and extent in millimetres required"));
    }
    let cards = value["modifiers"].as_array().map_or(0, Vec::len);
    if input.variant != "complete" && !(cards > 0 && input.variant == "emitters_only") {
        return Err(bad(command, "rig requires complete or an explicit card-omitting emitters_only variant"));
    }
    let scale = input.subject_extent_mm / 1000.0;
    let lights = value["lights"].as_array().ok_or_else(|| bad(command, "authored rig has no light array"))?;
    let mut areas = Vec::new();
    areas.try_reserve_exact(4).map_err(|_| bad(command, "could not reserve rig lights"))?;
    let mut sun = None;
    for light in lights {
        let radiance = triple(&light["emission"]["rgb"], command)?;
        match light["type"].as_str() {
            Some("rectangular_area") => {
                if areas.len() >= 4 || light["emission_sides"] != "front_only" {
                    return Err(bad(command, "rig exceeds the one-sided rectangular light contract"));
                }
                let position = triple(&light["position_mm"], command)?;
                let x = triple(&light["width_axis"], command)?;
                let y = triple(&light["height_axis"], command)?;
                let width = number(&light["width_mm"], command)? * scale * 0.5;
                let height = number(&light["height_mm"], command)? * scale * 0.5;
                areas.push(AreaLight {
                    center: std::array::from_fn(|i| input.center_mm[i] + position[i] * scale),
                    u: x.map(|v| v * width),
                    v: y.map(|v| v * height),
                    radiance,
                });
            }
            Some("uniform_distant_disk") => {
                if sun.is_some() {
                    return Err(bad(command, "more than one solar energy owner"));
                }
                sun = Some(Sun {
                    direction: triple(&light["incoming_photon_direction"], command)?.map(|v| -v),
                    angular_radius_rad: number(&light["angular_diameter_rad"], command)? * 0.5,
                    radiance,
                });
            }
            _ => return Err(bad(command, "authored light has no exact runtime adapter")),
        }
    }
    let lighting = StudioLighting { environment: [0.0; 3], sun, area_lights: AreaLights::new(&areas).map_err(|e| bad(command, e.to_string()))? };
    lighting.validate().map_err(|e| bad(command, e.to_string()))?;
    Ok((
        lighting,
        json!({"id":input.id,"variant":input.variant,"passive_cards_omitted":if input.variant=="emitters_only" {cards}else{0},"passive_cards_included":if input.variant=="complete" {cards}else{0},"scale":scale,
        "physical_units":"world millimetres; authored relative linear-sRGB radiance","reference_calibration_not_remeasured":true}),
    ))
}

pub(super) fn rig_stage(input: &RigInput, command: &str) -> Result<solidcraft_render::stage::Stage> {
    use solidcraft_render::stage::{Stage, StagePanel};
    if input.variant != "complete" {
        return Ok(Stage::default());
    }
    let source = item("rigs", &input.id, command)?;
    let modifiers = source["modifiers"].as_array().ok_or_else(|| bad(command, "rig modifier array missing"))?;
    if modifiers.len() > solidcraft_render::stage::MAX_STAGE_PANELS {
        return Err(bad(command, "too many rig cards"));
    }
    let scale = input.subject_extent_mm / 1000.0;
    let mut panels = Vec::new();
    panels.try_reserve_exact(modifiers.len()).map_err(|_| bad(command, "could not reserve rig cards"))?;
    for card in modifiers {
        if card["type"] != "diffuse_card"
            || card["material"]["model"] != "lambertian"
            || card["emission"] != 0
            || card["casts_shadow"] != true
            || card["two_sided"] != true
        {
            return Err(bad(command, "unsupported passive rig modifier; it cannot be silently omitted"));
        }
        let position = triple(&card["position_mm"], command)?;
        let u = triple(&card["width_axis"], command)?;
        let v = triple(&card["height_axis"], command)?;
        let width = number(&card["width_mm"], command)? * scale * 0.5;
        let height = number(&card["height_mm"], command)? * scale * 0.5;
        let id = card["id"].as_str().ok_or_else(|| bad(command, "rig card has no identity"))?;
        panels.push(StagePanel {
            id: format!("{}_{id}", input.id),
            center_mm: std::array::from_fn(|i| input.center_mm[i] + position[i] * scale),
            half_u_mm: u.map(|x| x * width),
            half_v_mm: v.map(|x| x * height),
            reflectance_linear: triple(&card["material"]["reflectance_linear_srgb"], command)?,
        });
    }
    Stage::new(panels).map_err(|e| bad(command, e.to_string()))
}
pub(super) fn rig_clearance(scene: &solidcraft_render::ProductScene, lighting: &StudioLighting, command: &str) -> Result<()> {
    for (i, light) in lighting.area_lights.iter().enumerate() {
        let panel = solidcraft_render::stage::StagePanel {
            id: format!("emitter_{i}"),
            center_mm: light.center,
            half_u_mm: light.u,
            half_v_mm: light.v,
            reflectance_linear: [0.; 3],
        };
        for body in &scene.meshes {
            if !panel.separated_from(body.mesh.bounds()).map_err(|e| bad(command, e.to_string()))? {
                return Err(bad(command, format!("rig emitter {i} intersects or touches {} bounds", body.name)));
            }
        }
    }
    Ok(())
}
pub(super) fn prepare_studio(_s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.studio.prepare";
    let input: RigInput = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let (lighting, projection) = rig_lighting(&input, command)?;
    let stage = rig_stage(&input, command)?;
    Ok(json!({"lighting":lighting,"stage":stage,"projection":projection,"requires_geometry_clearance_review":true,"document_changed":false}))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct StudioApply {
    rig: RigInput,
    name: String,
    #[serde(default = "empty_patch")]
    set: Value,
    replace_lighting_and_environment: bool,
    #[serde(default)]
    replace_stage: bool,
}
pub(super) fn apply_studio(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.library.studio.apply";
    let input: StudioApply = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if !input.replace_lighting_and_environment {
        return Err(bad(command, "confirm replacing lighting and environment explicitly to preserve a single energy owner"));
    }
    if !input.replace_stage && s.doc.render.setups.get(&input.name).is_some_and(|setup| !setup.stage.is_empty()) {
        return Err(bad(command, "existing stage requires replace_stage: true"));
    }
    let (lighting, projection) = rig_lighting(&input.rig, command)?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    let Some(RenderJob::GpuPath { options, .. }) = &mut setup.job else {
        return Err(bad(command, "authored area/sun rigs require an explicit GPU job"));
    };
    let stage = rig_stage(&input.rig, command)?;
    if !setup.stage.is_empty() && !input.replace_stage {
        return Err(bad(command, "existing stage requires replace_stage: true"));
    }
    setup.stage = stage;
    options.lighting = Some(lighting);
    options.environment = None;
    options.hdr = None;
    setup.validate().map_err(|e| bad(command, e.to_string()))?;
    let mut scene = crate::product_scene::extract(s)?;
    super::staging::apply(&mut scene, &setup, command)?;
    rig_clearance(&scene, &lighting, command)?;
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(input.name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(
        json!({"name":input.name,"setup":setup,"projection":projection,"clearance":"conservative product/stage AABB separation passed; not exact B-rep collision"}),
    )
}

#[cfg(test)]
#[path = "library_tests.rs"]
mod tests;
