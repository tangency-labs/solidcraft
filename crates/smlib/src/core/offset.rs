//! Shells, face offsets, thickening, sheet offsets and drafts: the C API's `smlib_shell`,
//! `smlib_offset_faces`, `smlib_thicken`, `smlib_draft` (the fork's `capi/src/smlib_ops.cpp`) and
//! `smlib_offset_sheet` (`capi/src/smlib_sheets.cpp`), each solidcore-ops' own
//! ([`solidcore_ops::offset`]) on a copy of its body, its faces picked by point
//! ([`solidcore_ops::pick::faces`]).
//!
//! The capi's argument checks stay here, so a call it refuses is refused before any kernel
//! work; solidcore-ops' refusals are worded as this crate's errors ([`offset_error`]). A shell
//! that fails without saying why names where a face's outline touches itself, where one does
//! (#18568).
//!
//! The kernel-level face pick ([`pick_faces`]) stays for the families that still run on the
//! kernel themselves (sweep, the trace's picks).

use solidcore::geom::object::Shape;
use solidcore::kernel::Kernel;
use solidcore::query::api_queries::face_closest_point_in_shape;
use solidcore::query::brep::{BrepIdentity, BrepQuery, LiveBrepObjects};
use solidcore::topology::brep::p1 as brep_p1;
use solidcore::topology::{Brep, BrepId, FaceId};

use super::blend::{Miss, pick, points_ok, tolerance_ok};
use super::call::ops_as;
use super::{Body, answer};
use crate::{Error, Result};

/// Application errors for ops' offset refusals: specialised wall answers keep
/// their fields; other typed refusals keep the kernel diagnostic alongside them.
pub(super) fn refused(error: solidcore_ops::Error, diagnostic: String) -> Error {
    use solidcore_ops::Error as Ops;
    // this crate words a named solid test as the bare NotSolid it always was
    if error.not_solid_test().is_some() {
        return Error::RefusedWithDiagnostic { refusal: Ops::NotSolid, diagnostic };
    }
    match error {
        Ops::WallsTouch => Error::WallsTouch,
        Ops::WallFolds { largest } => Error::WallFolds { largest },
        Ops::Unsupported(_) => Error::Unsupported,
        error @ Ops::OffsetRefused { why: solidcore_ops::OffsetRefusal::Phrase { .. }, .. } => Error::Refused(error),
        refusal => Error::RefusedWithDiagnostic { refusal, diagnostic },
    }
}

/// `std::fabs(x) > 1e6` and the rest of the capi's size checks: finite, not zero, at most 1e6.
fn distance_ok(x: f64) -> bool {
    x.is_finite() && x != 0.0 && x.abs() <= 1e6
}

/// The B-rep id every shape and face key in this module names the one body by.
const ONE: BrepId = BrepId::from_index(0);

/// The live view of `b` the closest-point solves read, under `identity`: `query`'s effective
/// identity ([`BrepQuery::effective_identity`]).
fn live<'a, 'i>(b: &'a mut Brep, identity: &'a BrepIdentity<'i>) -> solidcore::Result<LiveBrepObjects<'a, 'i>> {
    LiveBrepObjects::new(b, identity, None)
}

/// `pick_faces` (`face_distance`: `SmApiFaceClosestPoint`; a face whose query returns a status
/// other than success is skipped, [`answer`]).
pub(super) fn pick_faces(k: &mut Kernel, b: &mut Brep, points: &[[f64; 3]], tol: f64) -> solidcore::Result<std::result::Result<Vec<FaceId>, Miss>> {
    let all = brep_p1::faces(b, None)?;
    let model_tol = b.tolerance();
    let mut shapes = Vec::with_capacity(all.len());
    for &f in &all {
        shapes.push(answer(Shape::new(k, &(ONE, &*b), &[], &[(ONE, f)], &[], &[]))?);
    }
    let query = BrepQuery::of_brep(b);
    let identity = query.effective_identity();
    let objects = live(b, &identity)?;
    let k: &Kernel = k;
    let indices: Vec<usize> = (0..all.len()).collect();
    let picked = pick(&indices, points, tol, model_tol, |i, p| {
        let (Some(&f), Some(Some(shape))) = (all.get(i), shapes.get(i)) else { return Ok(None) };
        Ok(answer(face_closest_point_in_shape(k, shape, &objects, (ONE, f), p))?.map(|c| c.distance))
    })?;
    Ok(picked.map(|(p, _)| p.iter().filter_map(|&i| all.get(i).copied()).collect()))
}

/// A face pick's failure as `pick_error` (ops.rs) reads it.
pub(super) fn face_miss(m: Miss) -> Error {
    match m {
        Miss::None(point) => Error::NoMatch { point },
        Miss::Ambiguous(point) => Error::Ambiguous { point },
    }
}

/// solidcore-ops' refusal of an offset as this crate's error: the moves and walls that have
/// kinds of their own as those kinds, a kernel status as the kernel's words for it, a result
/// that is not a solid as [`Error::NotSolid`] (with the kernel's words where the kernel said
/// why), and the rest as [`refused`] words them.
fn offset_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    use solidcore_ops::Error as Ops;
    if e.not_solid_test().is_some() && detail.is_none() {
        return Error::NotSolid;
    }
    match e {
        Ops::Argument => Error::Argument,
        Ops::Empty => Error::Empty,
        Ops::NoRoom => Error::NoRoom,
        Ops::NoEffect => Error::NoEffect,
        Ops::NotExact => Error::NotExact,
        Ops::NotSolid => Error::NotSolid,
        Ops::Cancelled => Error::Cancelled(call),
        Ops::Internal(what) => Error::Internal(what),
        Ops::BlendsNotFollowed { exact, why } => Error::BlendsNotFollowed { exact, why },
        Ops::DraftCutsThrough { at, depth } => Error::DraftCutsThrough { at, depth },
        Ops::Status(code) => Error::Solidcore(detail.unwrap_or_else(|| format!("{call}: status {code}"))),
        e => refused(e, detail.unwrap_or_default()),
    }
}

/// [`offset_error`] for a draft: a face the kernel cannot draft is refused with what it is.
fn draft_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    match e {
        e @ solidcore_ops::Error::Unsupported(_) => Error::Refused(e),
        e => offset_error(call, e, detail),
    }
}

/// The faces of `body` nearest `points` (each within `tolerance`), each once, in the order its
/// first point picked it, as indices of the body's own list; a point that picks none, or is as
/// near two faces, refused ([`face_miss`]).
fn picked_faces(call: &'static str, body: &Body, points: &[[f64; 3]], tolerance: f64) -> Result<Vec<usize>> {
    let each = ops_as(call, &[("brep", body)], offset_error, || solidcore_ops::pick::faces(body, points, tolerance))?;
    let each = each.map_err(|m| {
        let e = face_miss(match m {
            solidcore_ops::pick::Miss::None(point) => Miss::None(point),
            solidcore_ops::pick::Miss::Ambiguous(point) => Miss::Ambiguous(point),
        });
        super::dump::failed_ops(call, &e.to_string(), &[("brep", body)]);
        e
    })?;
    let mut once: Vec<usize> = Vec::with_capacity(each.len());
    for f in each {
        if !once.contains(&f) {
            once.push(f);
        }
    }
    Ok(once)
}

/// A copy of `body` for an operation to work on (`SmApiBrepCopy`): solidcore-ops' offsets
/// change the body they are given.
fn working_copy(call: &'static str, body: &Body) -> Result<Body> {
    if body.kernel_left_unfinished() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    ops_as(call, &[("brep", body)], offset_error, || body.copy())
}

/// `smlib_shell`: the faces nearest `points` (each within `tolerance`) opened, the rest offset
/// by `thickness` (negative: inward) into a wall; none gives a closed hollow body. An inward
/// wall of a solid that leaves no cavity is [`Error::NoRoom`], refused before the shell.
pub(crate) fn shell(body: &Body, points: &[[f64; 3]], tolerance: f64, thickness: f64) -> Result<Body> {
    if !points_ok(points, true) || !tolerance_ok(tolerance) || !distance_ok(thickness) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_shell";
    let open = picked_faces(CALL, body, points, tolerance)?;
    let w = working_copy(CALL, body)?;
    let order = w.own_order();
    match ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::shell(&w, &order, &open, thickness)) {
        Ok(()) => Ok(w),
        // (#18568) said why where a face's outline touches itself: the copy was put back as it
        // came in, so its faces are the body's
        Err(e) if is_bare_failure(&e) => match ops_as(CALL, &[], offset_error, || w.outline_touch())? {
            Some(at) => Err(Error::OutlineTouches { at, detail: e.to_string() }),
            None => Err(e),
        },
        Err(e) => Err(e),
    }
}

/// (#18568) A failure that does not say why: a bare status, solidcore's text, a result that is
/// not a solid, or an offset refused as unsound.
fn is_bare_failure(e: &Error) -> bool {
    matches!(
        e,
        Error::Status(_) | Error::Solidcore(_) | Error::NotSolid | Error::Exception | Error::Refused(solidcore_ops::Error::OffsetRefused { .. })
    )
}

/// `smlib_offset_faces`: the faces nearest `points` (each within `tolerance`) of a solid moved
/// `distance` along their outward normals; faces moved in so far that the body leaves no
/// material behind them are [`Error::NoRoom`].
pub(crate) fn offset_faces(body: &Body, points: &[[f64; 3]], tolerance: f64, distance: f64) -> Result<Body> {
    if !points_ok(points, false) || !tolerance_ok(tolerance) || !distance_ok(distance) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_offset_faces";
    let picked = picked_faces(CALL, body, points, tolerance)?;
    let w = working_copy(CALL, body)?;
    let order = w.own_order();
    ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::offset_faces(&w, &order, &picked, distance))?;
    Ok(w)
}

/// `smlib_thicken`: a sheet thickened by `thickness` along its normals, or (symmetric) moved back
/// by half first; a closed solid is [`Error::Argument`].
pub(crate) fn thicken(body: &Body, thickness: f64, symmetric: bool) -> Result<Body> {
    if !distance_ok(thickness) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_thicken";
    let w = working_copy(CALL, body)?;
    ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::thicken(&w, thickness, symmetric))?;
    Ok(w)
}

/// `smlib_offset_sheet`: every face of a sheet moved onto its surface's offset by `distance`
/// along its normal; faces that close into a solid (a sphere's one face, a block's six) give the
/// offset solid (tangency-labs/smlib#17, which dropped the capi's refusal of a closed solid).
pub(crate) fn offset_sheet(body: &Body, distance: f64) -> Result<Body> {
    if !distance_ok(distance) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_offset_sheet";
    let w = working_copy(CALL, body)?;
    ops_as(CALL, &[("brep", body)], offset_error, || solidcore_ops::offset::offset_sheet(&w, distance))?;
    Ok(w)
}

/// A direction as `smlib_draft`'s `unit` checks it first, before any face is picked: finite and
/// of some length (solidcore-ops' [`solidcore_ops::unit_direction`]); a length the draft still
/// refuses (not longer than 1e-12) is solidcore-ops' check ([`solidcore_ops::offset::draft`]).
fn finite_direction(v: [f64; 3]) -> bool {
    solidcore_ops::unit_direction(v).is_some()
}

/// `smlib_draft`: the faces nearest `points` (each within `tolerance`) of a solid drafted by
/// `angle` radians about the neutral plane through `neutral_origin` with normal
/// `neutral_normal`, leaning in going along `pull` ([`solidcore_ops::offset::draft`]: a plane
/// turned about its line on the neutral plane, a cone or cylinder about the pull about its
/// circle on it); a face it cannot draft refused with what it is.
pub(crate) fn draft(
    body: &Body,
    points: &[[f64; 3]],
    tolerance: f64,
    neutral_origin: [f64; 3],
    neutral_normal: [f64; 3],
    pull: [f64; 3],
    angle: f64,
) -> Result<Body> {
    if !finite_direction(neutral_normal)
        || !finite_direction(pull)
        || !points_ok(points, false)
        || !tolerance_ok(tolerance)
        || !neutral_origin.iter().all(|x| x.is_finite() && x.abs() <= 1e9)
        || !angle.is_finite()
        || angle == 0.0
        || angle.abs() >= 1.5697963267948966
    {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_draft";
    if !super::is_manifold_solid(body)? {
        return Err(Error::Argument);
    }
    let picked = picked_faces(CALL, body, points, tolerance)?;
    let w = working_copy(CALL, body)?;
    let order = w.own_order();
    ops_as(CALL, &[("brep", body)], draft_error, || solidcore_ops::offset::draft(&w, &order, &picked, neutral_origin, neutral_normal, pull, angle))?;
    Ok(w)
}
