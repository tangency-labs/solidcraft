//! A drawing of the visible bodies and shown sketches from a supplied viewport camera. No
//! screen pixels, grid, selection highlights or tessellation diagonals are exported.

#[path = "view_dxf_meshes.rs"]
mod meshes;
#[path = "view_dxf_progress.rs"]
mod progress;
#[path = "view_dxf_projection.rs"]
mod projection;
#[path = "view_dxf_sketch.rs"]
pub(super) mod sketch;

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_io::{DxfEntity, DxfLayer, DxfLinetype, DxfShape};
use solidcraft_render::Camera;

use super::CommandSpec;
use crate::params::{bad, str_};
use crate::{Result, Session};

const CMD: &str = "file.export_view_dxf";
pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Export Current View as DXF", export)
    .icon("export")
    .noundo()
    .params("camera: Camera (current view); hidden?: omit|dashed (default omit); tolerance?: 0.0001..1 mm (curve chord tolerance, default 0.01); path?: .dxf file (else returns dxf text). Visible placed bodies and shown sketches' profile curves (no construction geometry, points, dimensions or constraints); 2D LINE/LWPOLYLINE entities on VISIBLE/HIDDEN layers (sketch curves on SKETCH), mm; finished sketches are hidden behind bodies, the active sketch is drawn on top; perspective projected onto the camera target plane; no viewport crop or overlays.")];

fn export(s: &mut Session, p: &Value) -> Result<Value> {
    let cam: Camera =
        serde_json::from_value(p.get("camera").cloned().ok_or_else(|| bad(CMD, "camera is required"))?).map_err(|_| bad(CMD, "invalid camera"))?;
    if !cam.is_valid()
        || !(0.0..std::f64::consts::PI).contains(&cam.fov)
        || cam.pitch.abs() > std::f64::consts::FRAC_PI_2
        || cam.yaw.abs() > 1e6
        || cam.roll.abs() > 1e6
    {
        return Err(bad(CMD, "invalid camera"));
    }
    let dashed = match p.get("hidden") {
        None => false,
        Some(Value::String(v)) if v == "omit" => false,
        Some(Value::String(v)) if v == "dashed" => true,
        _ => return Err(bad(CMD, "hidden must be omit or dashed")),
    };
    if !sketch::exportable(s) {
        return Err(bad(CMD, "no visible bodies or sketches to export"));
    }
    let tolerance = match p.get("tolerance") {
        None => 0.01,
        Some(v) => v.as_f64().filter(|t| (0.0001..=1.0).contains(t)).ok_or_else(|| bad(CMD, "tolerance must be 0.0001..1 mm"))?,
    };
    let path = match p.get("path") {
        None => None,
        Some(_) => Some(str_(p, "path").filter(|v| !v.trim().is_empty() && v.len() < 4096).ok_or_else(|| bad(CMD, "invalid path"))?),
    };
    if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
    }
    let started = std::time::Instant::now();
    progress::Stage::new("dxf_meshes", 1).check().map_err(|e| bad(CMD, e))?;
    let meshes = meshes::prepare(s, tolerance)?;
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    let drawing = projection::drawing(&meshes, &sketch::parts(s, tolerance).map_err(|e| bad(CMD, e))?, &cam).map_err(|e| bad(CMD, e))?;
    let writing = progress::Stage::new("dxf_write", 1);
    writing.check().map_err(|e| bad(CMD, e))?;
    let mut visible = 0;
    let mut hidden = 0;
    let mut entities = Vec::new();
    for line in drawing.lines {
        if line.hidden && !dashed {
            continue;
        }
        let layer = if line.hidden {
            hidden += 1;
            "HIDDEN"
        } else {
            visible += 1;
            "VISIBLE"
        };
        entities.push(DxfEntity { layer: layer.into(), shape: DxfShape::Line(line.a, line.b) });
    }
    let mut entities = projection::connect(entities);
    let sketch = sketch_entities(drawing.sketch, dashed, &mut entities);
    let mut layers = vec![DxfLayer::new("VISIBLE", 7)];
    if dashed {
        layers.push(DxfLayer { linetype: DxfLinetype::Hidden, ..DxfLayer::new("HIDDEN", 8) });
    }
    if sketch > 0 {
        layers.push(DxfLayer::new("SKETCH", 5));
    }
    let text = solidcraft_io::write_dxf_drawing(&layers, &entities);
    writing.check().map_err(|e| bad(CMD, e))?;
    let mut result = json!({"visible":visible,"hidden":hidden,"sketch":sketch,"units":"mm","tolerance":tolerance,"projection":if cam.fov > 0.0 {"perspective"} else {"orthographic"}});
    result["stats"] = json!({"bodies":meshes.placements.len(),"shared_meshes":meshes.meshes.len(),"triangles":meshes.triangles,
        "mesh_bytes":meshes.bytes,"worker_limit":meshes::WORKERS,"segments":drawing.segments,"occlusion_work":drawing.work,
        "prepare_ms":prepare_ms,"projection_ms":drawing.projection_ms,"occlusion_ms":drawing.occlusion_ms});
    if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(std::path::Path::new(path), text.as_bytes(), false).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        result["path"] = json!(path);
    } else {
        result["dxf"] = json!(text);
    }
    Ok(result)
}

/// Sketch runs as entities: visible ones on SKETCH, hidden ones dashed on HIDDEN or omitted.
/// Returns the number of SKETCH entities.
fn sketch_entities(paths: Vec<projection::Path>, dashed: bool, entities: &mut Vec<DxfEntity>) -> usize {
    let mut count = 0;
    for path in paths.into_iter().filter(|p| dashed || !p.hidden) {
        let layer = if path.hidden { "HIDDEN" } else { "SKETCH" };
        count += usize::from(!path.hidden);
        let shape = match <[Vec2; 2]>::try_from(path.points) {
            Ok([a, b]) => DxfShape::Line(a, b),
            Err(points) => DxfShape::Polyline { points },
        };
        entities.push(DxfEntity { layer: layer.into(), shape });
    }
    count
}

#[cfg(test)]
#[path = "view_dxf_tests.rs"]
mod tests;
