//! Exact planar face capture for sketch attachment. World labels are lookup keys only.
use crate::params::{bad, vec3};
use crate::{Result, Session};
use serde_json::Value;
use solidcraft_doc::{EmbossFace, PlaneRef, apply_point, apply_vector, mat_inverse, mat_mul};
use solidcraft_geom::Plane;

pub(in crate::cmd) fn is_face(p: &Value) -> bool {
    p.get("plane").and_then(|plane| plane.get("face")).is_some()
}

pub(in crate::cmd) fn capture(s: &Session, value: &Value, cmd: &str) -> Result<PlaneRef> {
    capture_impl(s, value, cmd, false)
}

/// Internal face-profile compatibility only; never exposed as a command parameter.
pub(in crate::cmd) fn capture_internal(s: &Session, value: &Value, cmd: &str) -> Result<PlaneRef> {
    capture_impl(s, value, cmd, true)
}

/// A planar face pick `{face, fixed?, body?, index?, path?}` as [`capture`] reads it before it
/// looks at the design.
pub(crate) struct FacePickForm<'a> {
    /// The picked point, in world coordinates.
    pub point: solidcraft_geom::Vec3,
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

fn capture_impl(s: &Session, value: &Value, cmd: &str, internal: bool) -> Result<PlaneRef> {
    let FacePickForm { point, fixed, body: requested_body, index: requested_index, path: requested_path } =
        face_pick_form(value).map_err(|why| bad(cmd, why))?;
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
    let mut sheet_face: Option<(String, Plane)> = None;
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
            // A flat face of a sheet metal body attaches to its panel (or a panel's rim): the
            // exact plane comes from the panel's flat region and fold, and the sketch keeps the
            // face's name to follow it.
            if !fixed
                && let Some(sheet) = state.sheets.iter().find(|sheet| sheet.body == body.name)
                && let Some((face, plane)) = sheet.panel_face_at(local_point, tolerance * 10.0)
            {
                if chosen.is_some() || sheet_face.is_some() {
                    return Err(bad(cmd, "face point is ambiguous; pick an exact body and face index"));
                }
                let relative = mat_mul(&author_inverse, &frame);
                let plane = Plane::new(apply_point(&relative, plane.origin), apply_vector(&relative, plane.x), apply_vector(&relative, plane.y))
                    .ok_or_else(|| bad(cmd, "invalid sheet face plane"))?;
                sheet_face = Some((solidcraft_doc::sheet::sheet_face_name(&body.name, &face), plane));
                continue;
            }
            let faces = body.body.faces(tolerance)?;
            let mesh = body.body.mesh(&solidcraft_kernel::MeshTolerance::chord(tolerance))?;
            // The triangles are grouped by face once, and a face's test reads only its own: the
            // work is the mesh plus one plane test per face, not the mesh once per face (a
            // modelled thread leaves thousands of faces, #6720).
            work = work.saturating_add(mesh.triangles.len()).saturating_add(faces.len());
            if work > 2_000_000 {
                return Err(bad(cmd, "face capture work limit exceeded; use an exact body and face index"));
            }
            let by_face = super::face_triangles(&mesh, faces.iter().map(|face| face.index.saturating_add(1)).max().unwrap_or(0));
            for face in faces {
                if requested_index.is_some_and(|index| index != face.index) {
                    continue;
                }
                let Some(exact_plane) = (if fixed {
                    body.body.planar_face(face.index).or_else(|| body.body.face_plane(face.index))
                } else {
                    body.body.face_plane(face.index).or_else(|| body.body.planar_face(face.index))
                }) else {
                    continue;
                };
                let normal = exact_plane.normal();
                if (local_point - face.centroid).dot(normal).abs() > tolerance * 10.0
                    || !super::face_contains(&mesh, by_face.get(face.index).map_or(&[][..], Vec::as_slice), local_point, tolerance * 10.0)
                {
                    continue;
                }
                if chosen.is_some() {
                    return Err(bad(cmd, "face point is ambiguous; pick an exact body and face index"));
                }
                let relative = mat_mul(&author_inverse, &frame);
                let normal = apply_vector(&relative, normal).normalized().ok_or_else(|| bad(cmd, "invalid planar face normal"))?;
                let centroid = apply_point(&relative, face.centroid);
                let picked = Plane::from_normal(normal * centroid.dot(normal), normal).ok_or_else(|| bad(cmd, "invalid selected planar face"))?;
                chosen = Some((body, face.index, path.clone(), picked));
            }
        }
    }
    if let Some((name, plane)) = sheet_face {
        if chosen.is_some() {
            return Err(bad(cmd, "face point is ambiguous; pick an exact body and face index"));
        }
        return Ok(PlaneRef::Face { plane, at: apply_point(&author_inverse, point), name: Some(name), anchor: None });
    }
    let (body, index, path, plane) =
        chosen.ok_or_else(|| bad(cmd, "the selected planar face no longer exists; choose another face or an explicit fixed plane"))?;
    if fixed {
        return Ok(PlaneRef::Custom { plane });
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
    Ok(PlaneRef::Face { plane, at, name, anchor })
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
