//! A drawing of the visible bodies and shown sketches from a supplied viewport camera. No
//! screen pixels, grid, selection highlights or tessellation diagonals are exported. The lines
//! come from [`crate::mesh_lines`]; this command chooses the bodies and writes the DXF.

#[path = "view_dxf_sketch.rs"]
pub(super) mod sketch;

use std::collections::BTreeMap;

use serde_json::{Value, json};
use solidcraft_geom::Vec2;
use solidcraft_io::{DxfEntity, DxfLayer, DxfLinetype, DxfShape};
use solidcraft_render::Camera;

use super::CommandSpec;
use crate::mesh_lines::{self, Stage, View};
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
    Stage::new("dxf_meshes", 1).check().map_err(|e| bad(CMD, e))?;
    let meshes = mesh_lines::prepare(s, CMD, tolerance, |b| s.body_visible(&b.name))?;
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    // The sketch runs (up to 4,000,000 points) go once the lines are made, not through writing.
    let drawing = {
        let sketch = sketch::parts(s, tolerance).map_err(|e| bad(CMD, e))?;
        mesh_lines::drawing(&meshes, &sketch, &View::from_camera(&cam)).map_err(|e| bad(CMD, e))?
    };
    let writing = Stage::new("dxf_write", 1);
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
    let mut entities = connect(entities);
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
        "mesh_bytes":meshes.bytes,"worker_limit":mesh_lines::WORKERS,"segments":drawing.segments,"occlusion_work":drawing.work,
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
fn sketch_entities(paths: Vec<mesh_lines::Path>, dashed: bool, entities: &mut Vec<DxfEntity>) -> usize {
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

/// Keep the hidden curves connected: restarting a dash on every short chord would make a
/// small circular hole look solid. Degree-two joins continue; junctions end the polyline.
fn connect(entities: Vec<DxfEntity>) -> Vec<DxfEntity> {
    let mut result = Vec::new();
    let mut lines = Vec::new();
    for entity in entities {
        match entity {
            DxfEntity { layer, shape: DxfShape::Line(a, b) } if layer == "HIDDEN" => lines.push((a, b)),
            e => result.push(e),
        }
    }
    let extent = lines.iter().flat_map(|(a, b)| [a, b]).fold(1.0_f64, |s, p| s.max(p.x.abs()).max(p.y.abs()));
    let eps = extent * 1e-8;
    let key = |p: Vec2| [(p.x / eps).round() as i64, (p.y / eps).round() as i64];
    let mut nodes: BTreeMap<[i64; 2], Vec<usize>> = BTreeMap::new();
    for (i, (a, b)) in lines.iter().enumerate() {
        nodes.entry(key(*a)).or_default().push(i);
        nodes.entry(key(*b)).or_default().push(i);
    }
    let mut used = vec![false; lines.len()];
    for (i, (a, b)) in lines.iter().enumerate() {
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        if let Some(v) = used.get_mut(i) {
            *v = true;
        }
        let mut points = vec![*a, *b];
        for _ in 0..2 {
            while let Some(end) = points.last().copied() {
                let Some(neighbours) = nodes.get(&key(end)).filter(|n| n.len() == 2) else { break };
                let Some(next) = neighbours.iter().copied().find(|i| used.get(*i) == Some(&false)) else { break };
                let Some((a, b)) = lines.get(next) else { break };
                if let Some(v) = used.get_mut(next) {
                    *v = true;
                }
                points.push(if key(*a) == key(end) { *b } else { *a });
            }
            points.reverse();
        }
        result.push(DxfEntity { layer: "HIDDEN".into(), shape: DxfShape::Polyline { points } });
    }
    result
}

#[cfg(test)]
#[path = "view_dxf_tests.rs"]
mod tests;
