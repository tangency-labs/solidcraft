//! Sketches and the component that owns them: a face-attached sketch moved to another component
//! keeps its place on the face, and `component.move_sketches.to_parts` (#10218) moves sketches
//! to the part whose face they are drawn on.
use serde_json::{Value, json};
use solidcraft_doc::{EmbossFace, FeatureKind, PlaneRef, apply_point, apply_vector, mat_inverse, mat_mul};
use solidcraft_geom::Plane;

use crate::params::{bad, name_or_id_list};
use crate::schema::component::{MoveSketchesToPartsResult, SketchKeptInPlace, SketchMovedToPart};
use crate::{Result, Session};

/// The face a sketch's plane is built on, beneath an offset, flip or angle.
fn face_mut(mut plane: &mut PlaneRef) -> Option<&mut PlaneRef> {
    for _ in 0..64 {
        match plane {
            PlaneRef::Offset { base, .. } | PlaneRef::Flipped { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::ThroughPoint { base, .. } => {
                plane = base
            }
            PlaneRef::Face { .. } => return Some(plane),
            _ => return None,
        }
    }
    None
}

/// Make the face-attached sketch `id`, about to belong to `to`, authored in a placement of `to`:
/// the placement its face is in when `to` owns the face, the design for the design, else `to`'s
/// first. The picked point, the repair plane and an angle's axis are carried into that placement,
/// so the sketch stays where it is on the face. Other sketches keep their own coordinates.
pub(super) fn reauthor(s: &mut Session, id: u64, to: u64, cmd: &str) -> Result<()> {
    let Some(feature) = s.doc.feature(id) else { return Ok(()) };
    let from = feature.component;
    let FeatureKind::Sketch { plane, .. } = &feature.kind else { return Ok(()) };
    let Some(anchor) = solidcraft_doc::face_anchor::plane_face_anchor(plane) else { return Ok(()) };
    if from == to {
        return Ok(());
    }
    let doc = s.placed_document();
    let path = if to == anchor.component {
        anchor.path.clone()
    } else if to == 0 {
        Vec::new()
    } else {
        doc.first_path(to)
    };
    let new_frame = EmbossFace::component_frame(&doc, to, &path)?;
    let inverse = mat_inverse(&new_frame).ok_or_else(|| bad(cmd, "the component's placement can't be inverted"))?;
    // A sketch whose old placement no longer resolves only gets the new one: its face anchor
    // places it, and the picked point and repair plane are a guide.
    let relative = anchor.author_frame(&doc, from).ok().map(|old| mat_mul(&inverse, &old));
    let Some(feature) = s.doc_mut().feature_mut(id) else { return Ok(()) };
    let FeatureKind::Sketch { plane, .. } = &mut feature.kind else { return Ok(()) };
    if let (Some(relative), PlaneRef::AtAngle { axis_origin, axis_dir, .. }) = (relative, &mut *plane) {
        *axis_origin = apply_point(&relative, *axis_origin);
        *axis_dir = apply_vector(&relative, *axis_dir);
    }
    let Some(PlaneRef::Face { plane: picked, at, anchor: Some(anchor), .. }) = face_mut(plane) else { return Ok(()) };
    anchor.author_path = path;
    if let Some(relative) = relative {
        *at = apply_point(&relative, *at);
        if let Some(moved) = Plane::new(apply_point(&relative, picked.origin), apply_vector(&relative, picked.x), apply_vector(&relative, picked.y)) {
            *picked = moved;
        }
    }
    Ok(())
}

/// Why a sketch stays where it is (a code and its text), or the component whose face it is on.
fn part_of(s: &Session, owner: u64, plane: &PlaneRef) -> std::result::Result<u64, (&'static str, &'static str)> {
    let mut base = plane;
    for _ in 0..64 {
        match base {
            PlaneRef::Offset { base: inner, .. }
            | PlaneRef::Flipped { base: inner, .. }
            | PlaneRef::AtAngle { base: inner, .. }
            | PlaneRef::ThroughPoint { base: inner, .. } => base = inner,
            _ => break,
        }
    }
    let PlaneRef::Face { anchor, .. } = base else { return Err(("not_on_face", "not on a face")) };
    let Some(anchor) = anchor else {
        return Err(("no_exact_attachment", "its face has no exact attachment (a sheet metal panel, or picked before exact attachments)"));
    };
    if anchor.component == 0 {
        return Err(("design_body", "on a body of the design itself, not of a part"));
    }
    if s.doc.components.iter().all(|component| component.id != anchor.component) {
        return Err(("part_missing", "its face's part is no longer in the design"));
    }
    if anchor.component == owner {
        return Err(("already_in_part", "already in its part"));
    }
    Ok(anchor.component)
}

/// `component.move_sketches.to_parts`: each sketch given (every sketch when none are) whose plane
/// is a face of a part's body goes to that part, as `component.move_sketches` moves it. One undo
/// step; the reply lists what moved and what stayed, and why.
pub(super) fn to_parts(s: &mut Session, p: &Value) -> Result<Value> {
    let cmd = "component.move_sketches.to_parts";
    let ids: Vec<u64> = if p.get("sketches").is_some() {
        let keys = name_or_id_list(cmd, p, "sketches")?;
        let mut ids = Vec::with_capacity(keys.len().min(10_000));
        for key in &keys {
            match s.doc.find_feature(key) {
                Some(f) if matches!(f.kind, FeatureKind::Sketch { .. }) => {
                    if !ids.contains(&f.id) {
                        ids.push(f.id);
                    }
                }
                _ => return Err(bad(cmd, format!("no sketch `{key}`"))),
            }
        }
        ids
    } else {
        s.doc.features.iter().filter(|f| matches!(f.kind, FeatureKind::Sketch { .. })).map(|f| f.id).collect()
    };
    let mut result = MoveSketchesToPartsResult::default();
    let mut moves = Vec::new();
    for id in ids {
        let Some(f) = s.doc.feature(id) else { continue };
        let FeatureKind::Sketch { plane, .. } = &f.kind else { continue };
        match part_of(s, f.component, plane) {
            Ok(component) => {
                let part = s.doc.components.iter().find(|c| c.id == component).map(|c| c.name.clone()).unwrap_or_default();
                result.sketches.push(SketchMovedToPart { sketch: id, name: f.name.clone(), component, part });
                moves.push((id, component));
            }
            Err((code, reason)) => {
                result.kept.push(SketchKeptInPlace { sketch: id, name: f.name.clone(), code: code.to_string(), reason: reason.to_string() })
            }
        }
    }
    for (id, component) in moves {
        super::move_sketches(s, &json!({"sketches": [id.to_string()], "component": component}))?;
    }
    (result.moved, result.stayed) = (result.sketches.len(), result.kept.len());
    serde_json::to_value(result).map_err(|e| bad(cmd, e.to_string()))
}
