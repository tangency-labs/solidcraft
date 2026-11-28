//! A drawing of the visible bodies and shown sketches from a supplied viewport camera. No
//! screen pixels, grid, selection highlights or tessellation diagonals are exported. The lines
//! come from [`crate::mesh_lines`]; this command chooses the bodies and writes the DXF.

#[path = "view_dxf_arcs.rs"]
mod arcs;
#[path = "view_dxf_sketch.rs"]
pub(super) mod sketch;

use std::collections::BTreeMap;
use std::sync::Arc;

use serde_json::Value;
use solidcore_record::{Plane, Vec2, Vec3};
use solidcraft_doc::{ModelBody, ModelState};
use solidcraft_geom::PlaneGeom;
use solidcraft_io::{DxfFile, DxfLayerId, DxfLinetype, DxfShape};
use solidcraft_viewport::Camera;

use super::CommandSpec;
use crate::mesh_lines::{self, Refused, Stage, View};
use crate::params::{bad, str_};
use crate::schema::silhouette::{DxfOutput, DxfProjection, DxfSectionPlane, DxfUnits, ExportViewDxfParams, ViewDxfResult, ViewDxfStats};
use crate::schema::{TypedContract, output_schema_for};
use crate::{Result, Session};

const CMD: &str = "file.export_view_dxf";
const CONTRACT: TypedContract<ExportViewDxfParams> = TypedContract::new();
pub static COMMANDS: &[CommandSpec] = &[CommandSpec::new(CMD, "Export Current View as DXF", crate::checked!("file.export_view_dxf", export))
    .icon("export")
    .noundo()
    .contract(&CONTRACT)
    .output(output_schema_for::<ViewDxfResult>)
    .params("camera: Camera (current view); hidden?: omit|dashed (default omit); tolerance?: 0.0001..1 mm (curve chord tolerance, default 0.01); curves?: lines|arcs (lines, default, writes the short lines the view is built from; arcs writes visible circles and arcs as CIRCLE and ARC); zero?: target|origin (where the drawing's 0,0 is: the camera target, default, or the model origin as seen from the camera); section?: [{origin, normal}] (up to 6 planes the view is cut with, the normal side left out, the cut faces' outlines drawn; default the session's Section Analysis, [] draws bodies whole); path?: .dxf file (else returns dxf text). Visible placed bodies and shown sketches' profile curves (no construction geometry, points, dimensions or constraints); 2D LINE/LWPOLYLINE entities on VISIBLE/HIDDEN layers (sketch curves on SKETCH), mm; finished sketches are hidden behind bodies, the active sketch is drawn on top; perspective projected onto the camera target plane; no viewport crop or overlays.")];

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
    let at_origin = match p.get("zero") {
        None => false,
        Some(Value::String(v)) if v == "target" => false,
        Some(Value::String(v)) if v == "origin" => true,
        _ => return Err(bad(CMD, "zero must be target or origin")),
    };
    let as_arcs = match p.get("curves") {
        None => false,
        Some(Value::String(v)) if v == "lines" => false,
        Some(Value::String(v)) if v == "arcs" => true,
        _ => return Err(bad(CMD, "curves must be lines or arcs")),
    };
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
    let planes = section_planes(s, p)?;
    let started = web_time::Instant::now();
    Stage::new("dxf_meshes", 1).check().map_err(|e| bad(CMD, refused(e)))?;
    let (world, uncut_meshes) = sectioned(s.world_state()?, &planes, |b| s.body_visible(&b.name))?;
    let meshes = mesh_lines::prepare(s, &world, CMD, tolerance, mesh_lines::Ask::default(), |b| s.body_visible(&b.name), refused)?;
    let prepare_ms = started.elapsed().as_secs_f64() * 1000.;
    // The sketch runs (up to 4,000,000 points) go once the lines are made, not through writing.
    let view = View::from_camera(&cam);
    let mut drawing = {
        let sketch = sketch::parts(s, tolerance).map_err(|e| bad(CMD, refused(e)))?;
        mesh_lines::drawing(&meshes, &sketch, &view).map_err(|e| bad(CMD, refused(e)))?
    };
    // A sketch under a body's outline is drawn once, as the body's line (#19507). Both curves are
    // sampled at `tolerance`, so they can lie that far apart from each other twice over.
    let reach = 2. * tolerance + drawing.eps;
    drawing.sketch = mesh_lines::sketch_off_lines(std::mem::take(&mut drawing.sketch), &drawing.lines, reach);
    if at_origin {
        // The drawing is centred on the camera target; move it so the model origin is 0,0.
        let zero = view.world_origin().map_err(|e| bad(CMD, refused(e)))?;
        for l in &mut drawing.lines {
            l.a -= zero;
            l.b -= zero;
        }
        for path in &mut drawing.sketch {
            for pt in &mut path.points {
                *pt -= zero;
            }
        }
    }
    let writing = Stage::new("dxf_write", 1);
    writing.check().map_err(|e| bad(CMD, refused(e)))?;
    let sketch_points = drawing
        .sketch
        .iter()
        .try_fold(0usize, |total, path| total.checked_add(path.points.len()).ok_or_else(|| bad(CMD, "view sketch-point count overflow")))?;
    let mut file = if as_arcs {
        DxfFile::for_current_view_arc_export(drawing.lines.len(), drawing.sketch.len(), sketch_points)?
    } else {
        DxfFile::for_current_view_export(drawing.lines.len(), drawing.sketch.len(), sketch_points)?
    };
    // Validate the original source counts before fitting: recognition must not bypass the
    // combined source-entity cap. Keep only sparse recognized sketch shapes, not a copy of
    // every ordinary line/path, and give short arcs their writer's actual extent budget.
    let planned = if as_arcs {
        let plan = PlannedViewArcs::sketch(&drawing.sketch, tolerance, &writing)?;
        file =
            DxfFile::for_current_view_arc_export_with_sketch_arcs(drawing.lines.len(), drawing.sketch.len(), sketch_points, &plan.arc_source_points)?;
        plan
    } else {
        PlannedViewArcs::default()
    };
    let visible_layer = file.declare_layer("VISIBLE", 7, DxfLinetype::Continuous, None)?;
    let hidden_layer = if dashed { Some(file.declare_layer("HIDDEN", 8, DxfLinetype::Hidden, None)?) } else { None };
    let sketch_layer =
        if drawing.sketch.iter().any(|path| !path.hidden) { Some(file.declare_layer("SKETCH", 5, DxfLinetype::Continuous, None)?) } else { None };
    let mut visible = 0;
    let mut hidden = 0;
    let mut visible_lines = Vec::new();
    let mut hidden_lines = Vec::new();
    for line in drawing.lines {
        if line.hidden {
            if !dashed {
                continue;
            }
            hidden += 1;
            hidden_lines.push((line.a, line.b));
        } else {
            visible += 1;
            visible_lines.push((line.a, line.b));
        }
    }
    let hidden_paths: Vec<DxfShape> = connect(hidden_lines).into_iter().map(|path| DxfShape::Polyline { points: path.points }).collect();
    // The entity order is part of the file's bytes: with arcs recognised, the hidden lines come first.
    if as_arcs {
        push_all(&mut file, hidden_layer, hidden_paths)?;
        push_all(&mut file, Some(visible_layer), arcs::recognise(visible_lines, tolerance))?;
    } else {
        push_all(&mut file, Some(visible_layer), visible_lines.into_iter().map(|(a, b)| DxfShape::Line(a, b)).collect())?;
        push_all(&mut file, hidden_layer, hidden_paths)?;
    }
    let sketch = sketch_entities(drawing.sketch, planned.sketch_shapes, dashed, &mut file, hidden_layer, sketch_layer)?;
    let text = solidcraft_io::write_dxf_file(&file)?;
    writing.check().map_err(|e| bad(CMD, refused(e)))?;
    let stats = ViewDxfStats {
        bodies: meshes.placements.len(),
        shared_meshes: meshes.meshes.len(),
        triangles: meshes.triangle_count(),
        mesh_bytes: meshes.bytes,
        worker_limit: mesh_lines::WORKERS,
        segments: drawing.segments,
        occlusion_work: drawing.work,
        prepare_ms,
        projection_ms: drawing.projection_ms,
        occlusion_ms: drawing.occlusion_ms,
    };
    let output = if let Some(path) = path {
        solidcraft_io::vfs::check(path).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        solidcraft_io::write_atomic(std::path::Path::new(path), text.as_bytes(), false).map_err(|e| crate::EngineError::Other(e.to_string()))?;
        DxfOutput::File { path: path.to_string() }
    } else {
        DxfOutput::Text { dxf: text }
    };
    let reply = ViewDxfResult {
        visible,
        hidden,
        sketch,
        units: DxfUnits::Millimetres,
        tolerance,
        projection: if cam.fov > 0.0 { DxfProjection::Perspective } else { DxfProjection::Orthographic },
        stats,
        output,
        param_warnings: match uncut_meshes {
            0 => Vec::new(),
            1 => vec!["1 mesh body is drawn uncut: a mesh has no faces to cut".into()],
            n => vec![format!("{n} mesh bodies are drawn uncut: a mesh has no faces to cut")],
        },
    };
    serde_json::to_value(reply).map_err(|e| bad(CMD, e.to_string()))
}

/// The planes the view is cut with (#22913): `section`'s, else the session's Section Analysis.
/// Each is turned to face the side that stays, which [`ModelBody::section`] keeps: what lies on a
/// section plane's normal side is hidden, as in the viewport.
fn section_planes(s: &Session, p: &Value) -> Result<Vec<Plane>> {
    let given: Vec<(Vec3, Vec3)> = match p.get("section") {
        None => s.section.iter().map(|plane| (plane.origin, plane.normal)).collect(),
        Some(v) => {
            let planes: Vec<DxfSectionPlane> =
                serde_json::from_value(v.clone()).map_err(|_| bad(CMD, "section must be a list of planes, each with an origin and a normal"))?;
            if planes.len() > solidcraft_viewport::MAX_SECTION_PLANES {
                return Err(bad(CMD, format!("a section cuts with at most {} planes", solidcraft_viewport::MAX_SECTION_PLANES)));
            }
            planes.into_iter().map(|plane| (Vec3::from(plane.origin), Vec3::from(plane.normal))).collect()
        }
    };
    given
        .into_iter()
        .map(|(origin, normal)| {
            let usable = origin.is_finite() && normal.is_finite() && [normal.x, normal.y, normal.z].iter().any(|c| c.abs() > 1e-12);
            usable
                .then(|| Plane::from_normal(origin, -normal))
                .flatten()
                .ok_or_else(|| bad(CMD, "a section plane needs a finite origin and a normal that isn't zero"))
        })
        .collect()
}

/// `world` with each body `shown` picks cut by every one of `planes`, the side each faces kept
/// ([`ModelBody::section`], which remembers the cut), and a body with nothing left left out; and how
/// many shown mesh bodies are drawn whole, having no faces to cut. Without planes, `world` as it is.
fn sectioned(world: Arc<ModelState>, planes: &[Plane], shown: impl Fn(&ModelBody) -> bool) -> Result<(Arc<ModelState>, usize)> {
    if planes.is_empty() {
        return Ok((world, 0));
    }
    let mut state = ModelState::clone(&world);
    let (mut keep, mut meshes, mut cut, mut left) = (Vec::with_capacity(state.bodies.len()), 0, 0, 0);
    for body in &mut state.bodies {
        if !shown(body) || body.body.is_mesh() {
            meshes += usize::from(shown(body));
            keep.push(true);
            continue;
        }
        cut += 1;
        let mut kept = Some(body.clone());
        for plane in planes {
            let Some(whole) = kept else { break };
            kept = whole.section(plane).map_err(|e| bad(CMD, format!("the section can't cut {}: {e}", body.name)))?;
        }
        keep.push(kept.is_some());
        if let Some(kept) = kept {
            left += 1;
            *body = kept;
        }
    }
    if cut > 0 && left == 0 && meshes == 0 {
        return Err(bad(CMD, "the section cuts away every visible body: move or flip its planes"));
    }
    let mut kept = keep.into_iter();
    state.bodies.retain(|_| kept.next().unwrap_or(true));
    Ok((Arc::new(state), meshes))
}

/// What a refusal of the view's lines says here, with what to do about it in a DXF's terms.
fn refused(e: Refused) -> String {
    match e {
        Refused::Cancelled => "DXF export cancelled".into(),
        Refused::Work | Refused::MeshBuffers(_) | Refused::MeshTriangles(_) | Refused::ViewNodes(_) | Refused::SketchPoints(_) => {
            format!("{e}: hide bodies or sketches, or increase tolerance")
        }
        Refused::EyePlane => format!("{e}: zoom out before exporting"),
        Refused::Invalid(_) => e.to_string(),
    }
}

/// `shapes` on `layer`, which must be declared when there are any.
fn push_all(file: &mut DxfFile<DxfShape>, layer: Option<DxfLayerId>, shapes: Vec<DxfShape>) -> Result<()> {
    if shapes.is_empty() {
        return Ok(());
    }
    let layer = layer.ok_or_else(|| bad(CMD, "missing hidden DXF layer"))?;
    for shape in shapes {
        file.push(layer, shape, None)?;
    }
    Ok(())
}

/// Sparse, per-visible-path recognition. A failed or mixed recognition keeps the original
/// path unchanged; lines from separate curves are never joined into a new sketch entity.
#[derive(Default)]
struct PlannedViewArcs {
    sketch_shapes: Vec<(usize, DxfShape)>,
    arc_source_points: Vec<usize>,
}

impl PlannedViewArcs {
    fn sketch(paths: &[mesh_lines::Path], tolerance: f64, writing: &Stage) -> Result<Self> {
        let mut plan = Self::default();
        for (index, path) in paths.iter().enumerate() {
            writing.check().map_err(|e| bad(CMD, refused(e)))?;
            // Two points are already exactly the line that the old producer writes.
            if path.hidden || path.points.len() <= 2 {
                continue;
            }
            let shape = arcs::fit(&path.points, path.closed, tolerance);
            writing.check().map_err(|e| bad(CMD, refused(e)))?;
            let Some(shape) = shape else { continue };
            plan.sketch_shapes.try_reserve(1).map_err(|_| bad(CMD, "cannot allocate recognized sketch shapes"))?;
            if matches!(shape, DxfShape::Arc { .. }) {
                plan.arc_source_points.try_reserve(1).map_err(|_| bad(CMD, "cannot allocate sketch arc source counts"))?;
                plan.arc_source_points.push(path.points.len());
            }
            plan.sketch_shapes.push((index, shape));
        }
        Ok(plan)
    }
}

/// Sketch runs as entities: visible ones on SKETCH, hidden ones dashed on HIDDEN or omitted.
/// Returns the number of SKETCH entities.
fn sketch_entities(
    paths: Vec<mesh_lines::Path>,
    planned_shapes: Vec<(usize, DxfShape)>,
    dashed: bool,
    file: &mut DxfFile<DxfShape>,
    hidden_layer: Option<DxfLayerId>,
    sketch_layer: Option<DxfLayerId>,
) -> Result<usize> {
    let mut count = 0;
    let mut planned = planned_shapes.into_iter().peekable();
    for (index, path) in paths.into_iter().enumerate().filter(|(_, p)| dashed || !p.hidden) {
        let layer = (if path.hidden { hidden_layer } else { sketch_layer }).ok_or_else(|| bad(CMD, "missing sketch DXF layer"))?;
        count += usize::from(!path.hidden);
        let shape = match planned.next_if(|(planned_index, _)| *planned_index == index) {
            Some((_, shape)) => shape,
            None => match <[Vec2; 2]>::try_from(path.points) {
                Ok([a, b]) => DxfShape::Line(a, b),
                Err(points) => DxfShape::Polyline { points },
            },
        };
        file.push(layer, shape, None)?;
    }
    if planned.next().is_some() {
        return Err(bad(CMD, "unconsumed planned sketch geometry"));
    }
    Ok(count)
}

/// Keep the hidden curves connected: restarting a dash on every short chord would make a
/// small circular hole look solid. Degree-two joins continue; junctions end the polyline.
fn connect(lines: Vec<(Vec2, Vec2)>) -> Vec<mesh_lines::Path> {
    let mut result = Vec::new();
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
        result.push(mesh_lines::Path { points, closed: false, hidden: true });
    }
    result
}

#[cfg(test)]
#[path = "view_dxf_tests.rs"]
mod tests;
