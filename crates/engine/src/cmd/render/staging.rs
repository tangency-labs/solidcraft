//! Explicit saved render-only ground. Does not add engineering bodies or material cards.
use crate::{Result, Session, params::bad};
use serde::Deserialize;
use serde_json::{Value, json};
use solidcraft_doc::render_settings::{RenderJob, RenderSetup};
use solidcraft_render::{
    ProductScene,
    job::CancelToken,
    stage::{Stage, StagePanel},
};
use std::sync::Arc;
fn patch() -> Value {
    json!({})
}
fn margin() -> f64 {
    0.25
}
fn gap() -> f64 {
    0.1
}
fn reflectance() -> [f64; 3] {
    [0.18; 3]
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Ground {
    name: String,
    #[serde(default = "patch")]
    set: Value,
    #[serde(default = "margin")]
    margin_fraction: f64,
    #[serde(default = "gap")]
    gap_mm: f64,
    #[serde(default = "reflectance")]
    reflectance_linear: [f64; 3],
    #[serde(default)]
    replace_stage: bool,
}
pub(super) fn apply(scene: &mut ProductScene, setup: &RenderSetup, command: &str) -> Result<()> {
    setup.stage.append_to(scene, &CancelToken::default()).map_err(|e| bad(command, e.to_string()))
}
fn save(s: &mut Session, name: String, setup: RenderSetup, command: &str) -> Result<Value> {
    let mut settings = (*s.doc.render).clone();
    settings.setups.insert(name.clone(), setup.clone());
    settings.validate().map_err(|e| bad(command, e.to_string()))?;
    if settings != *s.doc.render {
        s.doc_mut().render = Arc::new(settings);
    }
    Ok(json!({"name":name,"setup":setup,"engineering_geometry_unchanged":true,"stage_only":true,
        "clearance":"conservative product AABB separation; not a B-rep collision proof"}))
}
pub(super) fn ground(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.stage.ground";
    let input: Ground = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if !input.margin_fraction.is_finite()
        || !(0.0..=10.0).contains(&input.margin_fraction)
        || !input.gap_mm.is_finite()
        || !(1e-4..=1000.0).contains(&input.gap_mm)
    {
        return Err(bad(command, "ground margin must be 0..10 and explicit clearance gap 0.0001..1000 mm"));
    }
    if !input.replace_stage && s.doc.render.setups.get(&input.name).is_some_and(|setup| !setup.stage.is_empty()) {
        return Err(bad(command, "existing stage requires replace_stage: true"));
    }
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if !matches!(setup.job, Some(RenderJob::CpuRaytrace { .. } | RenderJob::GpuPath { .. })) {
        return Err(bad(command, "ground requires an explicit CPU raytrace or GPU path setup"));
    }
    if !setup.stage.is_empty() && !input.replace_stage {
        return Err(bad(command, "existing stage requires replace_stage: true"));
    }
    let mut scene = crate::product_scene::extract(s)?;
    if scene.bounds.is_empty() {
        return Err(bad(command, "ground fit requires a nonempty visible product"));
    }
    let bounds = scene.bounds;
    let size = bounds.max - bounds.min;
    let center = bounds.center();
    let extent = size.x.max(size.y).max(size.z).max(0.001);
    let u = (size.x * 0.5).max(0.001) + extent * input.margin_fraction;
    let v = (size.y * 0.5).max(0.001) + extent * input.margin_fraction;
    setup.stage = Stage::new(vec![StagePanel {
        id: "ground".into(),
        center_mm: [center.x, center.y, bounds.min.z - input.gap_mm],
        half_u_mm: [u, 0., 0.],
        half_v_mm: [0., v, 0.],
        reflectance_linear: input.reflectance_linear,
    }])
    .map_err(|e| bad(command, e.to_string()))?;
    apply(&mut scene, &setup, command)?;
    save(s, input.name, setup, command)
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Clear {
    name: String,
    #[serde(default = "patch")]
    set: Value,
}
pub(super) fn clear(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.stage.clear";
    let input: Clear = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    setup.stage = Stage::default();
    save(s, input.name, setup, command)
}

#[derive(Clone, Copy, Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
enum BackdropKind {
    FloorWall,
    Cyclorama,
}
#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct BackdropInput {
    name: String,
    #[serde(default = "patch")]
    set: Value,
    kind: BackdropKind,
    #[serde(default = "reflectance")]
    reflectance_linear: [f64; 3],
    #[serde(default)]
    replace_stage: bool,
    expected_revision: u64,
}
/// Original world-space stage derived from captured subject bounds and the chosen
/// camera azimuth. Twenty-four explicit planar strips approximate the round cove;
/// it is not an analytic smooth surface, terrain or engineering model operation.
pub(super) fn backdrop(s: &mut Session, params: &Value) -> Result<Value> {
    let command = "render.stage.backdrop";
    let input: BackdropInput = serde_json::from_value(params.clone()).map_err(|e| bad(command, e.to_string()))?;
    if input.expected_revision != s.revision {
        return Err(bad(command, "the design changed; review the stage again"));
    }
    if !input.replace_stage && s.doc.render.setups.get(&input.name).is_some_and(|p| !p.stage.is_empty()) {
        return Err(bad(command, "existing stage requires replace_stage: true"));
    }
    let mut setup = super::patched_settings(s, &input.name, &input.set, command)?;
    if !matches!(setup.job, Some(RenderJob::CpuRaytrace { .. } | RenderJob::GpuPath { .. })) {
        return Err(bad(command, "physical backgrounds require an explicit raytrace setup"));
    }
    if !input.replace_stage && !setup.stage.is_empty() {
        return Err(bad(command, "existing stage requires replace_stage: true"));
    }
    let mut scene = crate::product_scene::extract(s)?;
    if scene.bounds.is_empty() {
        return Err(bad(command, "physical background needs a nonempty visible subject"));
    }
    let bounds = scene.bounds;
    let c = bounds.center();
    let size = bounds.max - bounds.min;
    let extent = size.x.max(size.y).max(size.z);
    if !extent.is_finite() || !(0.001..=100_000.).contains(&extent) {
        return Err(bad(command, "subject extent must be 0.001..100000 mm"));
    }
    // Camera back projects toward the camera; the stage depth points away.
    let yaw = setup.camera.yaw;
    let depth = solidcraft_geom::Vec3::new(yaw.sin(), yaw.cos(), 0.);
    let horizontal = solidcraft_geom::Vec3::new(yaw.cos(), -yaw.sin(), 0.);
    let extent = extent.max(1.0); // minimum one-mm stage envelope, never scales the subject
    let radius = extent * 0.5;
    let near = -2.5 * extent;
    let start = extent; // farther than the subject's horizontal half diagonal
    let floor = bounds.min.z - (extent * 0.0001).max(0.0001);
    let wide = extent * 3.;
    let top = bounds.max.z + extent * 3.;
    let point = |d: f64, z: f64| [c.x + depth.x * d, c.y + depth.y * d, z];
    let edge = [horizontal.x * wide, horizontal.y * wide, 0.];
    let mut panels = Vec::new();
    panels.try_reserve_exact(26).map_err(|_| bad(command, "stage panel allocation failed"))?;
    panels.push(StagePanel {
        id: "backdrop_floor".into(),
        center_mm: point((near + start) * 0.5, floor),
        half_u_mm: edge,
        half_v_mm: [depth.x * (start - near) * 0.5, depth.y * (start - near) * 0.5, 0.],
        reflectance_linear: input.reflectance_linear,
    });
    let (wall_depth, wall_bottom) = if matches!(input.kind, BackdropKind::Cyclorama) {
        for i in 0..24 {
            let a = f64::from(i) * std::f64::consts::FRAC_PI_2 / 24.;
            let b = f64::from(i + 1) * std::f64::consts::FRAC_PI_2 / 24.;
            let (da, za) = (start + radius * a.sin(), floor + radius * (1. - a.cos()));
            let (db, zb) = (start + radius * b.sin(), floor + radius * (1. - b.cos()));
            panels.push(StagePanel {
                id: format!("backdrop_cove_{i}"),
                center_mm: point((da + db) * 0.5, (za + zb) * 0.5),
                half_u_mm: edge,
                half_v_mm: [depth.x * (db - da) * 0.5, depth.y * (db - da) * 0.5, (zb - za) * 0.5],
                reflectance_linear: input.reflectance_linear,
            });
        }
        (start + radius, floor + radius)
    } else {
        (start, floor)
    };
    panels.push(StagePanel {
        id: "backdrop_wall".into(),
        center_mm: point(wall_depth, (wall_bottom + top) * 0.5),
        half_u_mm: edge,
        half_v_mm: [0., 0., (top - wall_bottom) * 0.5],
        reflectance_linear: input.reflectance_linear,
    });
    setup.stage = Stage::new(panels).map_err(|e| bad(command, e.to_string()))?;
    apply(&mut scene, &setup, command)?;
    let mut result = save(s, input.name, setup, command)?;
    result["geometry_kind"] = json!(match input.kind {
        BackdropKind::FloorWall => "finite_floor_and_wall",
        BackdropKind::Cyclorama => "24_segment_polygonal_cyclorama",
    });
    result["light_transport"] =
        json!("nonemissive world-space stage; receives native lighting and casts reflections/shadows supported by the selected executor");
    result["camera_preserved"] = json!(true);
    result["limitations"] = json!([
        "Finite background, not an infinite floor or a terrain mesh.",
        "Cyclorama is an explicit 24-strip polygonal approximation, not a smooth CAD face.",
        "No change to HDR, light energy, subject optics, pose or engineering properties."
    ]);
    Ok(result)
}
