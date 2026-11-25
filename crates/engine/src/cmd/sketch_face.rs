//! Exact planar face capture for sketch attachment. World labels are lookup keys only.
use crate::params::{bad, vec3};
use crate::{Result, Session};
use serde_json::Value;
use solidcore_record::Plane;
use solidcraft_doc::{EmbossFace, PlaneRef, apply_point, apply_vector, mat_inverse, mat_mul};
use solidcraft_geom::prelude::*;

pub(in crate::cmd) fn is_face(p: &Value) -> bool {
    p.get("plane").and_then(|plane| plane.get("face")).is_some()
}

/// What a pick's face `index` means to [`capture_with`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(in crate::cmd) enum IndexMode {
    /// The index names a face of the body as it is where the sketch attaches: a point on another
    /// face is refused, never attached to silently.
    Exact,
    /// The index was read off the finished design, where a feature after the sketch may have
    /// numbered the faces differently (Redefine Sketch Plane): when no face of the body has that
    /// index at the picked point, the point alone names the face on that body.
    MayBeStale,
}

pub(in crate::cmd) fn capture(s: &Session, value: &Value, cmd: &str) -> Result<PlaneRef> {
    capture_with(s, value, cmd, IndexMode::Exact)
}

/// [`capture`], with how the pick's face index is read.
pub(in crate::cmd) fn capture_with(s: &Session, value: &Value, cmd: &str, index: IndexMode) -> Result<PlaneRef> {
    capture_attempt(s, value, cmd, false, index)
}

/// Internal face-profile compatibility only; never exposed as a command parameter.
pub(in crate::cmd) fn capture_internal(s: &Session, value: &Value, cmd: &str) -> Result<PlaneRef> {
    capture_attempt(s, value, cmd, true, IndexMode::Exact)
}

/// A planar face pick `{face, fixed?, body?, index?, path?}` as [`capture`] reads it before it
/// looks at the design.
pub(crate) struct FacePickForm<'a> {
    /// The picked point, in world coordinates.
    pub point: solidcore_record::Vec3,
    /// Keep the plane as it is now instead of following the face.
    pub fixed: bool,
    /// The exact pick: the world body and the face's index, given together.
    pub body: Option<&'a str>,
    pub index: Option<usize>,
    /// The occurrence path of the body's placement.
    pub path: Option<Vec<u64>>,
}

/// What [`capture`] reads `value` as, or its message for the first key it refuses, in the order
/// `face`, `fixed`, `body`, `index`, the two together, then `path`.
pub(crate) fn face_pick_form(value: &Value) -> std::result::Result<FacePickForm<'_>, &'static str> {
    let point = value
        .get("face")
        .and_then(vec3)
        .filter(|p| p.x.is_finite() && p.y.is_finite() && p.z.is_finite())
        .ok_or("face point must be three finite world coordinates")?;
    let fixed = match value.get("fixed") {
        None => false,
        Some(Value::Bool(fixed)) => *fixed,
        _ => return Err("fixed must be a boolean"),
    };
    let body = match value.get("body") {
        None => None,
        Some(Value::String(body)) if !body.is_empty() => Some(body.as_str()),
        _ => return Err("body must be the picked world body name"),
    };
    let index = match value.get("index") {
        None => None,
        Some(value) => Some(value.as_u64().and_then(|index| usize::try_from(index).ok()).ok_or("index must be a nonnegative face index")?),
    };
    if body.is_some() != index.is_some() {
        return Err("an exact face pick requires both body and index");
    }
    let path = match value.get("path") {
        None => None,
        Some(value) => Some(
            value
                .as_array()
                .filter(|path| path.len() <= 64)
                .ok_or("path must contain at most64 occurrence ids")?
                .iter()
                .map(|id| id.as_u64().filter(|id| *id != 0).ok_or("path ids must be nonzero integers"))
                .collect::<std::result::Result<Vec<_>, _>>()?,
        ),
    };
    Ok(FacePickForm { point, fixed, body, index, path })
}

/// The message when no face holds the picked point (and, given, carries the picked index).
const NO_SUCH_FACE: &str = "the selected planar face no longer exists; choose another face or an explicit fixed plane";

/// No face holds the picked point (or carries the picked index): the one refusal a stale face
/// index is retried after, told apart from every other by its type. Holds the refusal.
struct NoSuchFace(crate::EngineError);

fn capture_attempt(s: &Session, value: &Value, cmd: &str, internal: bool, index: IndexMode) -> Result<PlaneRef> {
    let NoSuchFace(refused) = match capture_impl(s, value, cmd, internal, true)? {
        Ok(plane) => return Ok(plane),
        Err(none) => none,
    };
    // A face index counts faces of the body as the picker showed it, which can be the finished
    // design, while a sketch attaches to the design before itself (Redefine Sketch Plane): a
    // feature after the sketch can add or remove faces, so the same face has another index here.
    // Only a caller that says so retries by the point on the same body, and the retry's own
    // refusal is the answer (two faces holding the point are refused as ambiguous).
    if index == IndexMode::MayBeStale && value.get("index").is_some() {
        return capture_impl(s, value, cmd, internal, false)?.map_err(|NoSuchFace(error)| error);
    }
    Err(refused)
}

fn capture_impl(s: &Session, value: &Value, cmd: &str, internal: bool, use_index: bool) -> Result<std::result::Result<PlaneRef, NoSuchFace>> {
    let FacePickForm { point, fixed, body: requested_body, index: requested_index, path: requested_path } =
        face_pick_form(value).map_err(|why| bad(cmd, why))?;
    let requested_index = requested_index.filter(|_| use_index);
    let state = s.model.state();
    let doc = s.placed_document();
    let author_path = s.active_occurrence_path.clone().unwrap_or_else(|| doc.first_path(s.active_component));
    let author_frame = EmbossFace::component_frame(&doc, s.active_component, &author_path)?;
    let author_inverse = mat_inverse(&author_frame).ok_or_else(|| bad(cmd, "invalid sketch author frame"))?;
    let selected_path = requested_path
        .clone()
        .or_else(|| requested_body.and_then(|name| state.body(name)).map(|body| doc.first_path(doc.model_body_component(body))));
    let placements = if let Some(path) = selected_path {
        let component = path
            .last()
            .and_then(|id| doc.occurrences.iter().find(|occurrence| occurrence.id == *id))
            .map(|occurrence| occurrence.component)
            .unwrap_or(0);
        let frame = EmbossFace::component_frame(&doc, component, &path)?;
        vec![(component, path, frame)]
    } else {
        crate::cmd::component::properties::placements(&doc, cmd)?
    };
    let mut by_component: std::collections::BTreeMap<u64, Vec<_>> = std::collections::BTreeMap::new();
    for body in &state.bodies {
        by_component.entry(doc.model_body_component(body)).or_default().push(body);
    }
    let mut chosen = None;
    // A face holds the picked point but has no exact plane: a curved face (or a flat one the kernel does not know as a plane), not a missing one.
    let mut curved = false;
    // The first body whose faces the kernel could not be asked, for when no other body has the face.
    let mut unread: Option<String> = None;
    let mut work = state.bodies.len().saturating_add(doc.occurrences.len());
    for (component, path, placement) in placements {
        let Some(bodies) = by_component.get(&component) else {
            continue;
        };
        for body in bodies {
            if let Some(source) = requested_body.and_then(|name| state.body(name))
                && source.name != body.name
            {
                continue;
            }
            work = work.saturating_add(1);
            if let Some(requested) = requested_body {
                work = work.saturating_add(doc.occurrences.len()).saturating_add(state.bodies.len()).saturating_add(path.len());
                if work > 2_000_000 {
                    return Err(bad(cmd, "face capture work limit exceeded; supply exact path"));
                }
                if requested != doc.world_body_name(&state, &body.name, component, &path) {
                    continue;
                }
            }
            let frame = doc.model_body_offset(body).map(|offset| mat_mul(&placement, offset)).unwrap_or(placement);
            let inverse = mat_inverse(&frame).ok_or_else(|| bad(cmd, "invalid selected face frame"))?;
            let local_point = apply_point(&inverse, point);
            let tolerance = (body.body.size() * 1e-4).max(1e-5);
            // The cost of asking a body for its faces grows with their number (a large body of
            // B-spline faces is the slow case), so each body is charged by its faces.
            work = work.saturating_add(body.body.face_count());
            if work > 2_000_000 {
                return Err(bad(cmd, "face capture work limit exceeded; use an exact body and face index"));
            }
            // A face of a sheet metal body attaches as every body's does, by the model's name for
            // the face, in the frame the sheet gives it (a panel's: its flat pattern's, see
            // `ModelBody::sketch_frame`). The faces holding the point are the body's own exact
            // faces: each is within the model tolerance of the point by its distance to the
            // trimmed face (its edges and rims, never display triangles), so nothing is meshed to
            // find them. A body with no exact faces (a mesh body) holds none; a body whose faces
            // the kernel cannot be asked is passed over, so it does not stop a face of another
            // body from being captured.
            let holding = match body.body.faces_within(local_point, tolerance) {
                Ok(holding) => holding,
                Err(why) => {
                    unread.get_or_insert_with(|| why.to_string());
                    continue;
                }
            };
            work = work.saturating_add(holding.len());
            for (index, _) in holding {
                if requested_index.is_some_and(|requested| requested != index) {
                    continue;
                }
                let Some(exact_plane) = (if fixed {
                    body.body.planar_face(index).or_else(|| body.body.face_plane(index))
                } else {
                    body.body.face_plane(index).or_else(|| body.body.planar_face(index))
                }) else {
                    curved = true;
                    continue;
                };
                let normal = exact_plane.normal();
                let on_plane = exact_plane.origin;
                if chosen.is_some() {
                    return Err(bad(cmd, "face point is ambiguous; pick an exact body and face index"));
                }
                let relative = mat_mul(&author_inverse, &frame);
                let normal = apply_vector(&relative, normal).normalized().ok_or_else(|| bad(cmd, "invalid planar face normal"))?;
                let on_plane = apply_point(&relative, on_plane);
                // The frame the body gives the face (a sheet's panel: its flat pattern's), else one
                // through the face.
                let picked = match body.sketch_frame(index).filter(|_| !fixed) {
                    Some(own) => Plane::new(apply_point(&relative, own.origin), apply_vector(&relative, own.x), apply_vector(&relative, own.y)),
                    None => Plane::from_normal(normal * on_plane.dot(normal), normal),
                }
                .ok_or_else(|| bad(cmd, "invalid selected planar face"))?;
                chosen = Some((body, index, path.clone(), picked));
            }
        }
    }
    let Some((body, index, path, plane)) = chosen else {
        if curved {
            return Err(bad(cmd, "the selected face is curved or is not an exact plane; choose a flat face or an explicit fixed plane"));
        }
        if let Some(why) = unread {
            return Err(bad(cmd, format!("face capture: {why}")));
        }
        return Ok(Err(NoSuchFace(bad(cmd, NO_SUCH_FACE))));
    };
    if fixed {
        return Ok(Ok(PlaneRef::Custom { plane }));
    }
    let at = apply_point(&author_inverse, point);
    let name = solidcraft_doc::naming::face_names(body).get(index).cloned();
    let anchor = match doc.capture_face_anchor(body, index, &plane, &path, s.active_component, &author_path) {
        Ok(anchor) => Some(Box::new(anchor)),
        Err(solidcraft_doc::DocError::Invalid(message))
            if internal
                && matches!(
                    message.as_str(),
                    "this face has no exact planar source; choose a fixed plane instead"
                        | "this face has no supported construction certificate; choose a fixed plane instead"
                ) =>
        {
            if path.iter().any(|id| s.pending_moves.contains_key(id) && !author_path.contains(id)) {
                return Err(bad(cmd, "capture the source occurrence position before generating a legacy face profile across components"));
            }
            None
        }
        Err(solidcraft_doc::DocError::Invalid(message))
            if matches!(
                message.as_str(),
                "this face has no exact planar source; choose a fixed plane instead"
                    | "this face has no supported construction certificate; choose a fixed plane instead"
            ) =>
        {
            return Err(bad(
                cmd,
                "this face can't keep an attachment: its history can't tell it apart from other faces (an edit reshaped it, \
                 or more than one face could claim its name). \
                 Pick another face, or use a fixed plane here (fixed:true), which won't follow later changes to the face",
            ));
        }
        Err(error) => return Err(bad(cmd, format!("{error}; choose a supported exact face or explicitly use fixed:true"))),
    };
    Ok(Ok(PlaneRef::Face { plane, at, name, anchor }))
}

/// The sole durable face attachment beneath an offset/angle wrapper.
pub(in crate::cmd) fn anchor(mut plane: &PlaneRef) -> Option<&solidcraft_doc::FaceAnchor> {
    loop {
        match plane {
            PlaneRef::Offset { base, .. } | PlaneRef::ThroughPoint { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => {
                plane = base
            }
            PlaneRef::Face { anchor, .. } => return anchor.as_deref(),
            _ => return None,
        }
    }
}

pub(in crate::cmd) fn anchor_mut(mut plane: &mut PlaneRef) -> Option<&mut solidcraft_doc::FaceAnchor> {
    loop {
        match plane {
            PlaneRef::Offset { base, .. } | PlaneRef::ThroughPoint { base, .. } | PlaneRef::AtAngle { base, .. } | PlaneRef::Flipped { base, .. } => {
                plane = base
            }
            PlaneRef::Face { anchor, .. } => return anchor.as_deref_mut(),
            _ => return None,
        }
    }
}

pub(in crate::cmd) fn capture_positions(s: &mut Session, plane: &PlaneRef) -> Result<()> {
    let mut involved = std::collections::BTreeSet::new();
    if let Some(anchor) = anchor(plane) {
        involved.extend(anchor.path.iter().copied());
        involved.extend(anchor.author_path.iter().copied());
    } else {
        involved.extend(s.active_occurrence_path.clone().unwrap_or_else(|| s.doc.first_path(s.active_component)));
    }
    let changes: Vec<_> = involved.into_iter().filter_map(|id| s.pending_moves.get(&id).copied().map(|matrix| (id, matrix))).collect();
    for (id, matrix) in &changes {
        let occurrence = s
            .doc_mut()
            .occurrences
            .iter_mut()
            .find(|occurrence| occurrence.id == *id)
            .ok_or_else(|| bad("sketch attachment", "pending author/source path disappeared"))?;
        occurrence.transform = *matrix;
    }
    for (id, _) in changes {
        s.pending_moves.remove(&id);
    }
    Ok(())
}

/// The component and placement (occurrence path, empty for the design) whose body a face pick
/// `value` lies on: the pick's `path`, else its `body`'s first placement, else the exact
/// attachment `captured` from it. `None` when none of them names one (a sheet metal panel
/// face picked by its point alone, or a fixed plane picked by its point alone).
pub(in crate::cmd) fn pick_owner(s: &Session, value: &Value, captured: &PlaneRef, cmd: &str) -> Result<Option<(u64, Vec<u64>)>> {
    let form = face_pick_form(value).map_err(|why| bad(cmd, why))?;
    if let Some(path) = form.path {
        if path.is_empty() {
            return Ok(Some((0, path)));
        }
        let component = s.occurrence_placement(&path)?.component;
        return Ok(Some((component, path)));
    }
    if let Some(body) = form.body {
        let state = s.model.state();
        let doc = s.placed_document();
        return Ok(state.body(body).map(|body| doc.model_body_component(body)).map(|component| (component, doc.first_path(component))));
    }
    Ok(anchor(captured).map(|anchor| (anchor.component, anchor.path.clone())))
}

#[cfg(test)]
#[path = "sketch_face_tests.rs"]
mod tests;
