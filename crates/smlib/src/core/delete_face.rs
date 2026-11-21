use super::Body;
use super::blend::{finish, on_copy, points_ok, status, tolerance_ok};
use super::offset::{face_miss, pick_faces};
use crate::{Error, Result};

/// The solid with the fillet faces under `points` (each within `tolerance`) taken off, one after
/// another, each gap closed by the sharp edge its two tangent neighbours meet in. A face that is
/// not a four-sided fillet (no pair of opposite edges tangent to its neighbours, or a neighbour
/// that cannot be extended) is refused as solidcore refuses it.
pub(crate) fn delete_fillet_faces(body: &Body, points: &[[f64; 3]], tolerance: f64) -> Result<Body> {
    if !points_ok(points, false) || !tolerance_ok(tolerance) {
        return Err(Error::Argument);
    }
    on_copy("smlib_delete_fillet_faces", body, |k, mut b| {
        let mut faces = match pick_faces(k, &mut b, points, tolerance)? {
            Ok(fs) => fs,
            Err(m) => return Ok(Err(face_miss(m))),
        };
        let mut seen = std::collections::HashSet::new();
        faces.retain(|f| seen.insert(*f));
        match solidcore_ops::blend::delete_fillet_faces(k, &mut b, &faces) {
            Ok(true) => finish(k, b, true),
            Ok(false) => Ok(Err(Error::NotSolid)),
            Err(e) => Ok(Err(status(e))),
        }
    })
}
