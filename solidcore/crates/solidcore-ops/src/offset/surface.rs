//! Offset surfaces: faces of a body copied out at a signed distance along their normals, as a
//! new body. Each face goes onto its surface's offset on its own, the kernel's offset of the face
//! as a sheet ([`super::offset_sheet`]); the offsets are then joined and stitched where they meet
//! within the caller's tolerance, so neighbours moved to the same place along their shared edge
//! (meeting smoothly) keep sharing it, and across a sharp edge they come apart. Faces that close
//! up make a solid. An offset the kernel cannot make (one that would fold a face over) is its
//! refusal, never a fitted surface. Ops sequences: every face, offset and stitch is the
//! kernel's.

use solidcore::geom::tol::USE_DEFAULT;
use solidcore::topology::brep::p2::{copy_source_ref, CopyNotifyState};
use solidcore::topology::brep::p3::{brep_copy_mapped, new_brep};
use solidcore::topology::brep::p7::copy_faces_from;
use solidcore::topology::loopuse::notify_ref;
use solidcore::topology::{Brep, FaceId};
use solidcore::EntityRef;

use super::{offset_sheet, MAX_DISTANCE};
use crate::body::Body;
use crate::error::status;
use crate::order::{self, Order};
use crate::session::run_on;
use crate::split::copies_of;
use crate::stitch::stitch;
use crate::{Error, Result};

/// Record faces `faces` of `body` (through `order`, each once) offset by `distance` along their
/// outward normals (negative: against them) into one new body, joined where their offsets'
/// edges lie within `tolerance` of each other; with whether the result closes into a solid
/// ([`stitch`]'s). A distance of zero copies the faces. `body` is unchanged; the faces and edges
/// of the result carry what their sources carry (an application's provenance,
/// [`crate::history::recorded_into`]).
///
/// # Errors
/// [`Error::Argument`] for no face, an index past the faces or repeated, a distance not finite
/// or past [`MAX_DISTANCE`], a tolerance not finite and positive; the kernel's refusals (an
/// offset that would fold a face, [`Error::Empty`] for one that leaves nothing); those of
/// [`stitch`].
pub fn offset_surface(
    body: &Body,
    order: &Order,
    faces: &[usize],
    distance: f64,
    tolerance: f64,
) -> Result<(Body, bool)> {
    if faces.is_empty()
        || !(distance.is_finite() && distance.abs() <= MAX_DISTANCE)
        || !(tolerance.is_finite() && tolerance > 0.0)
    {
        return Err(Error::Argument);
    }
    let mut picked: Vec<FaceId> = Vec::new();
    for &i in faces {
        let f = order::face(order, i)?;
        if picked.contains(&f) {
            return Err(Error::Argument);
        }
        picked.push(f);
    }
    let mut sheets = Vec::new();
    for f in picked {
        let sheet = face_sheet(body, f)?;
        if distance != 0.0 {
            offset_sheet(&sheet, distance)?;
        }
        sheets.push(sheet);
    }
    stitch(joined(&sheets)?, tolerance)
}

/// Face `f` of `body` copied out as a sheet of its own, with what it and its edges carry.
fn face_sheet(body: &Body, f: FaceId) -> Result<Body> {
    run_on("offset_surface", &[("brep", body)], |k| {
        let (mut whole, src) = {
            let g = body.lock();
            if !g.face_list().contains(&f) {
                return Ok(Err(Error::Argument));
            }
            let (whole, map) = brep_copy_mapped(k, &g)?;
            let face = |e| match e {
                EntityRef::Face(x) => Some(x),
                _ => None,
            };
            let Some([src]) = copies_of(&map, &[f], EntityRef::Face, face)
                .and_then(|v| <[FaceId; 1]>::try_from(v).ok())
            else {
                return Ok(Err(Error::Argument));
            };
            (whole, src)
        };
        let mut w = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        w.set_tolerance(whole.tolerance());
        match copied_into(k, &mut whole, &[src], &mut w) {
            Ok(1) => {}
            Ok(_) => return Ok(Err(Error::Empty)),
            Err(e) => return Ok(Err(status(e))),
        }
        Ok(Ok(Body::new(w)))
    })?
}

/// The faces of `sheets` copied into one body, each sheet's topology kept.
fn joined(sheets: &[Body]) -> Result<Body> {
    let inputs: Vec<(&'static str, &Body)> = sheets.iter().map(|s| ("sheet", s)).collect();
    run_on("offset_surface", &inputs, |k| {
        let mut dest = new_brep(k, USE_DEFAULT, USE_DEFAULT)?;
        for s in sheets {
            let mut g = s.lock();
            let faces = g.face_list().to_vec();
            if let Err(e) = copied_into(k, &mut g, &faces, &mut dest) {
                return Ok(Err(status(e)));
            }
        }
        Ok(Ok(Body::new(dest)))
    })?
}

/// `faces` of `src` copied into `dest` through the merge, with both bodies' attributes in the
/// copy's notifications so each copy carries what its source carries; how many faces it made.
fn copied_into(
    k: &mut solidcore::Kernel,
    src: &mut Brep,
    faces: &[FaceId],
    dest: &mut Brep,
) -> solidcore::Result<usize> {
    let mut state = CopyNotifyState::with_attributes_of(src, dest)?;
    let this_face = |x: FaceId| notify_ref(EntityRef::Face(x));
    let copied = copy_faces_from(
        k,
        src,
        faces,
        dest,
        true,
        &copy_source_ref,
        &this_face,
        &mut state.env(None),
    );
    state.restore_attributes(src, dest)?;
    let (made, _) = copied?;
    dest.refresh_cached_topology();
    Ok(made.len())
}
