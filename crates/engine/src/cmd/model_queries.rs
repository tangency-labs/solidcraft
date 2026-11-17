//! Bounded exact model queries for command and plugin callers.
use serde_json::Value;
use solidcore_record::{
    Loop2, Region2, Seg2, Vec3,
    brep::{Curve, Loop, Shell, Surface},
};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{Body, KernelError, PathSeg, PlanarPiece};

use super::CommandSpec;
use crate::{
    Result, Session,
    params::bad,
    schema::{TypedContract, model_queries::*, output_schema_for},
};

pub static COMMANDS: &[CommandSpec] = &[
    CommandSpec::new("model.face_outlines", "Read Face Outlines", crate::checked!("model.face_outlines", face_outlines))
        .icon("inspect").noundo().read_only()
        .contract(&TypedContract::<FaceOutlinesParams>::new()).output(output_schema_for::<FaceOutlines>)
        .params("body; faces?: [{index}] from model.faces (refresh after edits); offset?: selected-list position, default 0; limit?: 1..64, default 64. Analytic planar face loops in model mm, lines/arcs/circles only; unsupported curves refuse. Omitted faces selects analytic planar faces. Reply stays below 900 KiB; next is the next selected-list position, null at end. No mesh or design edits."),
    CommandSpec::new("model.classify_point", "Classify Point Against Body", crate::checked!("model.classify_point", classify_point))
        .icon("inspect").noundo().read_only()
        .contract(&TypedContract::<ClassifyPointParams>::new()).output(output_schema_for::<ClassifiedPoint>)
        .params("body; point: [x,y,z], finite model mm. Exact B-rep classification: inside, outside (including cavities), or on the boundary within the body's kernel tolerance. Mesh bodies refuse; no design edits."),
];

const MAX_FACES: usize = 20_000;
const MAX_EDGES: usize = 4096;
const MAX_REPLY: usize = 900 * 1024;

fn classify_point(s: &mut Session, p: &Value) -> Result<Value> {
    const CMD: &str = "model.classify_point";
    let p: ClassifyPointParams = serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    if !p.point.iter().all(|x| x.is_finite()) {
        return Err(bad(CMD, "point must contain finite coordinates"));
    }
    let state = s.world_state()?;
    let body = state.body(&p.body).ok_or_else(|| bad(CMD, format!("no body `{}`", p.body)))?;
    if body.body.is_mesh() {
        return Err(bad(CMD, "a B-rep body is required; mesh bodies are not supported"));
    }
    let class = solidcraft_kernel::guard(CMD, || body.body.classify_point(Vec3::from(p.point)))?;
    let class = match class {
        solidcraft_kernel::PointClass::Inside => PointClass::Inside,
        solidcraft_kernel::PointClass::Outside => PointClass::Outside,
        solidcraft_kernel::PointClass::OnBoundary => PointClass::On,
    };
    serde_json::to_value(ClassifiedPoint { body: body.name.clone(), class }).map_err(|e| bad(CMD, e.to_string()))
}

fn analytic_plane(surface: &Surface) -> bool {
    matches!(surface, Surface::Plane { .. } | Surface::PlaneThrough { .. })
}

fn face_outlines(s: &mut Session, p: &Value) -> Result<Value> {
    face_outlines_with_budget(s, p, MAX_REPLY)
}

fn face_outlines_with_budget(s: &mut Session, p: &Value, max_reply: usize) -> Result<Value> {
    const CMD: &str = "model.face_outlines";
    let p: FaceOutlinesParams = serde_json::from_value(p.clone()).map_err(|e| bad(CMD, e.to_string()))?;
    if !(1..=64).contains(&p.limit) {
        return Err(bad(CMD, "limit must be 1..64"));
    }
    if p.faces.as_ref().is_some_and(|f| f.len() > 256) {
        return Err(bad(CMD, "faces accepts at most 256 indices"));
    }
    let state = s.world_state()?;
    let body = state.body(&p.body).ok_or_else(|| bad(CMD, format!("no body `{}`", p.body)))?;
    if body.body.is_mesh() {
        return Err(bad(CMD, "a B-rep body is required; mesh bodies are not supported"));
    }
    let record = body.body.record();
    if record.faces.len() > MAX_FACES {
        return Err(bad(CMD, "body exceeds the 20000-face query budget"));
    }
    let indices: Vec<usize> = match p.faces {
        Some(faces) => faces.into_iter().map(|f| f.index).collect(),
        None => record.faces.iter().enumerate().filter(|(_, f)| analytic_plane(&f.surface)).map(|(i, _)| i).collect(),
    };
    if indices.iter().any(|i| record.faces.get(*i).is_none()) {
        return Err(bad(CMD, "face index is out of range"));
    }
    if p.offset > indices.len() {
        return Err(bad(CMD, "offset is past the selected face list"));
    }
    let names = solidcraft_kernel::guard(CMD, || Ok(solidcraft_doc::naming::face_names(body)))?;
    let mut result = FaceOutlines { body: body.name.clone(), faces: Vec::new(), next: None };
    let mut position = p.offset;
    for &index in indices.iter().skip(p.offset).take(p.limit) {
        let mut face = solidcraft_kernel::guard(CMD, || outline(&body.body, index))?;
        face.name = names.get(index).cloned();
        face.exact = solidcraft_kernel::guard(CMD, || Ok(solidcraft_doc::naming::face_name_exact(body, index)))?;
        result.faces.push(face);
        result.next = (position + 1 < indices.len()).then_some(position + 1);
        let bytes = serde_json::to_vec(&result).map_err(|e| bad(CMD, e.to_string()))?;
        if bytes.len() > max_reply {
            result.faces.pop();
            if result.faces.is_empty() {
                return Err(bad(CMD, "one face exceeds the 900 KiB reply budget"));
            }
            break;
        }
        position += 1;
    }
    result.next = (position < indices.len()).then_some(position);
    let value = serde_json::to_value(result).map_err(|e| bad(CMD, e.to_string()))?;
    if serde_json::to_vec(&value).map_err(|e| bad(CMD, e.to_string()))?.len() > max_reply {
        return Err(bad(CMD, "reply exceeds the 900 KiB budget"));
    }
    Ok(value)
}

fn unsupported(reason: impl Into<String>) -> KernelError {
    KernelError::Invalid(format!("model.face_outlines: not supported: {}", reason.into()))
}

fn analytic_paths(edge: &solidcore_record::brep::Edge) -> solidcraft_kernel::Result<Vec<PathSeg>> {
    // A line label does not map a NURBS parameter interval onto its analytic line.
    // Refuse this representation rather than interpreting knots as distances along it.
    if matches!(edge.curve, Curve::Line { .. }) && edge.nurbs.is_some() {
        return Err(unsupported("line with a separate NURBS parameterization"));
    }
    let interval = solidcore_ops::label::edge_curve_interval(edge).ok_or_else(|| unsupported("edge interval cannot be read exactly"))?;
    solidcraft_kernel::curve_path(&edge.curve, interval)
}

fn outline(body: &Body, index: usize) -> solidcraft_kernel::Result<FaceOutline> {
    let record = body.record();
    let face = record.faces.get(index).ok_or_else(|| unsupported("face index is out of range"))?;
    if !analytic_plane(&face.surface) {
        return Err(unsupported(format!("face {index} is not an analytic plane")));
    }
    let plane = body.planar_face(index).ok_or_else(|| unsupported(format!("face {index} has no usable plane")))?;
    let forward = record
        .shells
        .iter()
        .filter_map(|s| match s {
            Shell::Faces { faces, .. } => Some(faces),
            _ => None,
        })
        .flatten()
        .find(|f| f.face as usize == index)
        .is_none_or(|f| f.forward);
    let mut outer = None;
    let mut holes = Vec::new();
    let mut edge_count = 0usize;
    for lp in &face.loops {
        let Loop::Edges { edges } = lp else {
            return Err(unsupported("vertex boundary loop"));
        };
        edge_count = edge_count.saturating_add(edges.len());
        if edge_count > MAX_EDGES {
            return Err(unsupported("face exceeds the 4096-edge budget"));
        }
        let mut segments = Vec::new();
        let mut planar = Vec::new();
        for edge_use in edges {
            let edge_index = edge_use.edge as usize;
            let edge = record.edges.get(edge_index).ok_or_else(|| unsupported("dangling edge"))?;
            if !matches!(edge.curve, Curve::Line { .. } | Curve::Circle { .. }) {
                return Err(unsupported(format!("face {index} edge {edge_index} is free-form; no certified flattening is supplied")));
            }
            // The projected analytic pieces are used only for the kernel's certified loop sign.
            // Output below comes from the declared 3D analytic curve, not a fitted polyline.
            let pieces = body
                .planar_edges(&plane, &[edge_index])
                .into_iter()
                .next()
                .flatten()
                .ok_or_else(|| unsupported("edge cannot be read in the face plane"))?;
            let mut edge_planar = Vec::new();
            for piece in pieces {
                match piece {
                    PlanarPiece::Seg(seg @ (Seg2::Line { .. } | Seg2::Arc { .. })) => edge_planar.push(seg),
                    _ => return Err(unsupported("non-analytic projected edge")),
                }
            }
            if !edge_use.forward {
                edge_planar = edge_planar.into_iter().rev().map(|s| s.reversed()).collect();
            }
            planar.extend(edge_planar);
            let mut paths = analytic_paths(edge)?;
            if !edge_use.forward {
                paths = paths.into_iter().rev().map(|s| s.reversed()).collect();
            }
            for path in paths {
                let segment = match path {
                    PathSeg::Line { a, b } => OutlineSegment::Line { a: a.into(), b: b.into() },
                    PathSeg::Arc { a, center, axis, angle } => {
                        let Curve::Circle { radius, .. } = &edge.curve else {
                            return Err(unsupported("arc has no declared radius"));
                        };
                        if edge.vertices.first() == edge.vertices.get(1) {
                            // Closed topology supplies the full turn even if mapped endpoints drift.
                            let circle =
                                solidcraft_kernel::edge_circle(body, edge_index).ok_or_else(|| unsupported("closed circle cannot be read"))?;
                            OutlineSegment::Circle {
                                center: center.into(),
                                radius: *radius,
                                axis: axis.into(),
                                start: a.into(),
                                sweep_rad: circle.angle,
                            }
                        } else {
                            OutlineSegment::Arc { center: center.into(), radius: *radius, axis: axis.into(), start: a.into(), sweep_rad: angle }
                        }
                    }
                    _ => return Err(unsupported("free-form edge path")),
                };
                segments.push(segment);
            }
        }
        let area = solidcore_ops::prism::region_area(&Region2 { outer: Loop2 { segs: planar }, holes: Vec::new() })
            .map_err(|e| unsupported(format!("loop orientation: {e}")))?;
        let positive = area.sign().ok_or_else(|| unsupported("loop orientation is not decided"))?;
        if positive == forward {
            if outer.replace(segments).is_some() {
                return Err(unsupported("more than one outer loop"));
            }
        } else {
            holes.push(segments);
        }
    }
    let outer = outer.ok_or_else(|| unsupported("face has no outer loop"))?;
    Ok(FaceOutline {
        index,
        name: None,
        exact: false,
        plane: OutlinePlane { origin: plane.origin.into(), normal: plane.normal().into() },
        loops: vec![OutlineRegion { outer, holes }],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn exact_model_queries_byte_budget_pages_without_skipping_faces() {
        let mut s = Session::default();
        s.execute("solid.box", &serde_json::json!({"length":40,"width":30,"height":10,"body_name":"Part"})).unwrap();
        let params = serde_json::json!({"body":"Part"});
        let all = face_outlines(&mut s, &params).unwrap();
        let first = face_outlines(&mut s, &serde_json::json!({"body":"Part","limit":1})).unwrap();
        let budget = serde_json::to_vec(&first).unwrap().len();
        let page = face_outlines_with_budget(&mut s, &params, budget).unwrap();
        assert_eq!(page, first);
        let rest = face_outlines_with_budget(&mut s, &serde_json::json!({"body":"Part","offset":page["next"]}), MAX_REPLY).unwrap();
        let mut faces = page["faces"].as_array().unwrap().clone();
        faces.extend(rest["faces"].as_array().unwrap().iter().cloned());
        assert_eq!(faces, *all["faces"].as_array().unwrap());
        assert!(rest["next"].is_null());
        let final_budget = serde_json::to_vec(&all).unwrap().len() - 1;
        let page = face_outlines_with_budget(&mut s, &params, final_budget).unwrap();
        assert_eq!(page["faces"].as_array().unwrap().len(), 5);
        assert_eq!(page["next"], 5);
        let error = face_outlines_with_budget(&mut s, &params, budget - 1).unwrap_err();
        assert!(error.to_string().contains("one face exceeds"), "{error}");
    }

    #[test]
    fn exact_model_queries_refuse_reparameterized_line_labels() {
        let edge = solidcore_record::brep::Edge {
            curve: Curve::Line { start: [0., 0., 0.], end: [1., 0., 0.] },
            nurbs: Some(Curve::Spline { knots: vec![10., 10., 20., 20.], points: vec![[0., 0., 0.], [1., 0., 0.]] }),
            interval: [10., 20.],
            vertices: [0, 1],
            tolerance: 1e-6,
            uses: Vec::new(),
        };
        let error = solidcraft_kernel::guard("outline test", || analytic_paths(&edge)).unwrap_err().to_string();
        assert!(error.contains("separate NURBS parameterization"), "{error}");
    }
}
