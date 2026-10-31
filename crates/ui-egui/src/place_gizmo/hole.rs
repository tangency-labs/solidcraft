//! Hole: a marker on each Single hole placed on a face drags it across the model, snapping as a
//! pick does (sketch points on the face, the face's middle and its round edges' centres when Create
//! Hole › Snap to face and edge centres is on), and a click on a marker takes the hole out, as the
//! hole's own handle from #2912 did. Behind Create Hole › Move holes by clicking or dragging, not the
//! shared Placement handles setting. No ring: a hole has nothing to turn. Holes on sketch points
//! (Multiple) have no markers: their place is the point's.
//!
//! Reference edges (#37) place the first hole: with one picked, its marker has two arms along the
//! edge and drags the hole along it at its distance; with two, the marker has its four arms again
//! and a drag sets both distances. Any other hole in the dialog moves anywhere.

use solidcraft_engine::Sel;
use solidcraft_engine::geom::Vec3;

use super::{Freedom, Place, Spec, circle, face_pick, len, vec3_of};
use crate::SolveApp;
use crate::dialogs::{Dialog, Kind};
use crate::viewport::Hit;

pub(super) static SPEC: Spec = Spec {
    name: "hole",
    serves: |app, d| app.preferences.hole_moves && matches!(&d.kind, Kind::Hole { opts, .. } if !opts.multiple),
    snap_commands: &[],
    move_tip: "Drag to move the hole; click to remove it",
    many: true,
    places,
    move_to,
    click: Some(|app, d, key| crate::hole_place::remove_place(d, &app.session, key)),
    footprint,
    freedom: |_, d, key| crate::hole_refs::freedom(d, key),
    turn: None,
};

/// The holes placed so far, each at its face place with the face's outward normal. A hole being
/// edited whose face is gone keeps its place and the `direction` the edit restored, as a Rest's
/// frame does, so it still has a marker.
fn places(app: &SolveApp, d: &Dialog) -> Vec<Place> {
    let s = &app.session;
    crate::hole_place::places(s, d)
        .into_iter()
        .filter_map(|(key, base)| {
            let Some(Sel::Face { body, index, .. }) = d.inputs.first()?.items.get(key) else { return None };
            let n = crate::dialogs::planar_face(s, body, *index)
                .map(|(_, n)| n)
                .or_else(|| d.extra.get("direction").and_then(vec3_of))
                .and_then(|n| n.normalized())?;
            Some(Place { key, base, n })
        })
        .collect()
}

/// The hole at `key` moved to the face place a click with `hits` would take (snapped as the
/// pick is), unless that face already has another hole; held by a reference edge, as far along
/// the edge as the pick is.
fn move_to(app: &SolveApp, d: &mut Dialog, hits: &[Hit], key: usize) -> bool {
    let s = &app.session;
    let Some(to) = face_pick(d, s, hits) else { return false };
    match crate::hole_refs::freedom(d, key) {
        Freedom::Plane => crate::hole_place::move_place(d, s, key, to),
        Freedom::Line(_) => match to {
            Sel::Face { point, .. } => crate::hole_refs::drag_to(s, d, key, point),
            _ => false,
        },
    }
}

/// The hole's mouth on the face: the counterbore's or countersink's diameter, else the bore's.
fn footprint(app: &SolveApp, d: &Dialog, base: Vec3, n: Vec3) -> Vec<Vec3> {
    let Kind::Hole { diameter, kind, cb_diameter, cs_diameter, .. } = &d.kind else { return Vec::new() };
    let mouth = match kind {
        1 => cb_diameter,
        2 => cs_diameter,
        _ => diameter,
    };
    circle(base, n, len(app, mouth) * 0.5)
}

#[cfg(test)]
#[path = "hole_tests.rs"]
mod tests;
