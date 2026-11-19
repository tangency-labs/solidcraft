//! How model commands take the current selection (#5501): edges for Fillet and Chamfer, faces
//! for Shell and Draft, one face for Press Pull, sketch profiles for Extrude and Revolve, a plane
//! or a face for Create Sketch. Each gives the parameters the selection fills in; these commands
//! open a dialog, which takes the selection and asks for the rest (a radius, a distance). Menus
//! offer a command for the selection only when its hook here gives parameters
//! ([`super::CommandSpec::for_selection`]).

use serde_json::{Value, json};

use crate::Sel;
use crate::Session;

/// Most entities a model command takes from the selection.
const MOST: usize = 500;

fn xyz(p: &solidcore_record::Vec3) -> Value {
    json!([p.x, p.y, p.z])
}

/// The selection when it is between 1 and [`MOST`] items, no sketch is being edited, and
/// `pick` reads every item: the picks, or `None`.
fn every<T>(s: &Session, pick: impl Fn(&Sel) -> Option<T>) -> Option<Vec<T>> {
    if s.active_sketch.is_some() || !(1..=MOST).contains(&s.selection.len()) {
        return None;
    }
    s.selection.iter().map(pick).collect()
}

/// One body's picks: (body, point) for each, all on the same body.
fn one_body(picks: Vec<(String, Value)>) -> Option<(String, Vec<Value>)> {
    let body = picks.first()?.0.clone();
    picks.iter().all(|(b, _)| *b == body).then(|| (body, picks.into_iter().map(|(_, p)| p).collect()))
}

fn edges(s: &Session) -> Option<Value> {
    let picks = every(s, |sel| match sel {
        Sel::Edge { body, point, .. } => Some((body.clone(), xyz(point))),
        _ => None,
    })?;
    let (body, edges) = one_body(picks)?;
    Some(json!({"body": body, "edges": edges}))
}

fn faces(s: &Session) -> Option<Value> {
    let picks = every(s, |sel| match sel {
        Sel::Face { body, point, .. } => Some((body.clone(), xyz(point))),
        _ => None,
    })?;
    let (body, faces) = one_body(picks)?;
    Some(json!({"body": body, "faces": faces}))
}

/// Edges of one body: Fillet and Chamfer.
pub(super) fn blend_edges(s: &Session) -> Option<Value> {
    edges(s)
}

/// Faces of one body: Shell and Draft.
pub(super) fn body_faces(s: &Session) -> Option<Value> {
    faces(s)
}

/// One face: Press Pull.
pub(super) fn one_face(s: &Session) -> Option<Value> {
    let [Sel::Face { body, point, .. }] = s.selection.as_slice() else { return None };
    s.active_sketch.is_none().then(|| json!({"face": xyz(point), "face_body": body}))
}

/// Profiles of one sketch: Extrude and Revolve.
pub(super) fn profiles(s: &Session) -> Option<Value> {
    let picks = every(s, |sel| match sel {
        Sel::Profile { sketch, index } => Some((*sketch, *index)),
        _ => None,
    })?;
    let sketch = picks.first()?.0;
    picks.iter().all(|(k, _)| *k == sketch).then(|| json!({"sketch": sketch, "profiles": picks.iter().map(|(_, i)| i).collect::<Vec<_>>()}))
}

/// A plane, or a face to sketch on: Create Sketch.
pub(super) fn sketch_plane(s: &Session) -> Option<Value> {
    if s.active_sketch.is_some() {
        return None;
    }
    match s.selection.as_slice() {
        [Sel::Plane { name }] => Some(json!({"plane": name})),
        [Sel::Face { body, index, point }] => Some(json!({"plane": {"face": xyz(point), "body": body, "index": index}})),
        _ => None,
    }
}
