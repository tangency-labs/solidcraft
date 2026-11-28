//! Fillets and chamfers: the C API's entry points (`smlib_fillet`, `smlib_chamfer`, `smlib_blend`,
//! `smlib_blend_dropping`, `smlib_blend_setback`, `smlib_blend_setback_open`,
//! `smlib_fillet_variable`, `smlib_chamfer_sides`, the fork's `capi/src/smlib_ops.cpp`) on
//! solidcore-ops' blends ([`solidcore_ops::blend`]).
//!
//! As there, every operation works on a copy of its body and picks edges and corners by point
//! ([`solidcore_ops::pick`]); the blend itself, its corners, the overflow that goes first and the
//! refusals are ops' ([`solidcore_ops::blend_with`], [`solidcore_ops::chamfer_sides_facing`],
//! [`solidcore_ops::fillet_variable_from`]). What stays here is what the entry points check of
//! their own arguments, and ops' refusals as SolidCraft's errors ([`refused`]).
//!
//! The blend executive splits variable-radius faces at corners in their law. An unported kernel
//! operation maps to [`Error::Unsupported`].
//!
//! The picks by point, the statuses and the copy below ([`pick`], [`status`], [`on_copy`]) are
//! what the operation families still on the kernel crate share (tangency-labs/solidcore#3304).

use solidcore::SolidError;
use solidcore::blend::fillet_corner::p3::MIXED_CORNER_RAILS_CROSS;
use solidcore::error::nurbs::NurbsFailure;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::topology::Brep;
use solidcore::topology::brep::p3::brep_copy;

use super::Body;
use crate::{Blend, ChamferSecond, Error, Hole, HoleEdge, MAX_POINTS, Result, Turn};

/// Why solidcore refuses the 3x2 mixed-convexity corner whose rails cross
/// ([`MIXED_CORNER_RAILS_CROSS`]): it builds no corner for it yet.
pub(crate) const MIXED_CORNER_UNSUPPORTED: &str = "a fillet corner where a convex and a concave round leave the vertex along one \
tangent, so that their contact lines cross before the corner (https://github.com/tangency-labs/solidcore/issues/2256)";
/// The capi's statuses as SolidCraft errors: an operation's own result.
type Out<T> = std::result::Result<T, Error>;

/// An SMLib status as the capi passes it on (`status`, smlib_shim.h): solidcore's upstream
/// status codes as they are; its refusals of code it has not ported as not supported; a refusal
/// or a blend failure solidcore-ops types as that ([`Error::Refused`]: an offset refused with
/// its reason, a blend failed with the kernel's statement of why); a rejected input with solidcore's
/// reason ([`Error::Rejected`]); the rest as solidcore's text.
pub(super) fn status(e: SolidError) -> Error {
    if let Some(failed) = solidcore_ops::blend::failure(&e) {
        return Error::Refused(failed);
    }
    match e {
        SolidError::Upstream { code, .. } => Error::Status(code),
        e if stands_for_status(&e) => Error::Status(e.sm_status().unwrap_or(solidcore::error::SM_ERR)),
        SolidError::InvalidInput { operation, reason } => Error::Rejected { operation, reason },
        SolidError::Unsupported { reason, .. } if reason == MIXED_CORNER_RAILS_CROSS => Error::Unported(MIXED_CORNER_UNSUPPORTED),
        e if is_boolean_failure(&e) => boolean_status(e),
        e if e.is_unported() => Error::Unsupported,
        e => match solidcore_ops::offset::refusal(&e) {
            Some(r @ solidcore_ops::Error::OffsetRefused { .. }) => Error::Refused(r),
            _ => Error::Solidcore(e.to_string()),
        },
    }
}

/// The kernel failures SolidCraft still passes on as the status they stand for ([`Error::Status`]),
/// because solidcore-ops gives them no type of their own yet: each replaced a status its routine
/// returned (`Upstream` before solidcore described it), whether or not it names an entity. A
/// B-spline method's (`NurbsFailure::BSpline`); an intersection or classification, primitive
/// (extrusion, revolution, skin, taper), mass-property, healing, geometry-utility, query,
/// topology or surface routine's; and a sweep, curve or offset routine's that replaced a status
/// (a railed loft's, a length bracket's and an offset's refusals replaced none and pass on
/// otherwise). A blend routine's is solidcore-ops' blend failure ([`status`]). `tests::status_sites_only_shrink` counts the sites that make a status, a set that only shrinks.
fn stands_for_status(e: &SolidError) -> bool {
    matches!(e.nurbs_failure(), Some(NurbsFailure::BSpline(_)))
        || e.intersect_failure().is_some()
        || e.primitive_failure().is_some()
        || e.mass_failure().is_some()
        || e.heal_failure().is_some()
        || e.geom_failure().is_some()
        || e.sweep_failure().is_some_and(|w| w.replaces_upstream_status())
        || e.query_failure().is_some()
        || e.topology_failure().is_some()
        || e.curve_failure().is_some_and(|c| c.replaces_upstream_status())
        || e.surface_failure().is_some()
        || e.offset_failure().is_some_and(|o| o.replaces_upstream_status())
}

/// A failure of the Boolean engine (which slice, sweep and the other users of the merge engine
/// run), which solidcore names in its own terms as a [`SolidError::Failed`] from a
/// `boolean::` stage where it once returned [`SolidError::Upstream`].
fn is_boolean_failure(e: &SolidError) -> bool {
    matches!(e, SolidError::Failed(f) if f.operation.starts_with("boolean::"))
}

/// The status of a Boolean failure as the capi passes it on: the status the failure stands for
/// ([`Error::Status`]), as the `Upstream` it replaced gave. A stage that failed because a call is
/// not ported is not that status ([`SolidError::is_status_answer`]): it is as unported code is,
/// not supported.
fn boolean_status(e: SolidError) -> Error {
    match e.sm_status() {
        Some(code) if e.is_status_answer() => Error::Status(code),
        _ if e.root_cause().is_unported() => Error::Unsupported,
        _ => Error::Solidcore(e.to_string()),
    }
}

/// `size_ok`.
fn size_ok(x: f64) -> bool {
    x.is_finite() && x > 0.0 && x <= 1e6
}

/// `tolerance_ok`.
pub(super) fn tolerance_ok(t: f64) -> bool {
    t.is_finite() && t > 0.0 && t < 1e6
}

/// `points_ok`.
pub(super) fn points_ok(points: &[[f64; 3]], allow_none: bool) -> bool {
    points.len() <= MAX_POINTS && (allow_none || !points.is_empty()) && points.iter().flatten().all(|x| x.is_finite())
}

pub(super) fn p3(p: [f64; 3]) -> Point3 {
    Vec3::new(p[0], p[1], p[2])
}

/// Why a pick failed: `SMLIB_ERR_PICK_NONE` or `SMLIB_ERR_PICK_AMBIGUOUS`, and the point.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum Miss {
    None(usize),
    Ambiguous(usize),
}

/// `smlib_pick::pick`: the entity nearest each point, each once, with the index of the point that
/// first picked it. A point picks none when the nearest is farther than `tol`, and is ambiguous
/// when the second nearest is within `tol` too and nearer than the first by at most `model_tol`.
/// An entity whose distance query returns a status other than success is skipped, as the capi
/// skips it ([`super::answer`]); any other solidcore error fails the pick.
pub(super) fn pick<T: Copy + PartialEq>(
    all: &[T],
    points: &[[f64; 3]],
    tol: f64,
    model_tol: f64,
    mut dist: impl FnMut(T, Point3) -> solidcore::Result<Option<f64>>,
) -> solidcore::Result<std::result::Result<(Vec<T>, Vec<usize>), Miss>> {
    let (mut picked, mut first) = (Vec::new(), Vec::new());
    for (i, p) in points.iter().enumerate() {
        let p = p3(*p);
        let (mut best, mut d0, mut d1) = (None, f64::INFINITY, f64::INFINITY);
        for &x in all {
            let Some(d) = dist(x, p)? else { continue };
            if !d.is_finite() {
                continue;
            }
            if d < d0 {
                d1 = d0;
                d0 = d;
                best = Some(x);
            } else if d < d1 {
                d1 = d;
            }
        }
        let Some(best) = best.filter(|_| d0 <= tol) else {
            return Ok(Err(Miss::None(i)));
        };
        if d1 <= tol && d1 - d0 <= model_tol {
            return Ok(Err(Miss::Ambiguous(i)));
        }
        if !picked.contains(&best) {
            picked.push(best);
            first.push(i);
        }
    }
    Ok(Ok((picked, first)))
}

/// An edge pick's failure as `pick_error` (ops.rs) reads it.
pub(super) fn edge_miss(m: Miss) -> Error {
    match m {
        Miss::None(point) => Error::NoMatch { point },
        Miss::Ambiguous(point) => Error::Ambiguous { point },
    }
}

/// A corner pick's failure as `setback_error` (ops.rs) reads it: the capi writes the corner
/// point's index to `out_failed_corner` only, so the status comes back bare.
fn corner_miss(m: Miss) -> Error {
    match m {
        Miss::None(_) => Error::Status(ERR_PICK_NONE),
        Miss::Ambiguous(_) => Error::Status(ERR_PICK_AMBIGUOUS),
    }
}

/// The capi's statuses for a pick that is near nothing, and for one equally near to two.
const ERR_PICK_NONE: i32 = -1101;
const ERR_PICK_AMBIGUOUS: i32 = -1102;

/// Runs `f`, a step of the entry point `call` on solidcore-ops, for `body`, the entry point's
/// input ([`super::call::ops_as`]: the session's fixes, the stop token, the profiler span, the
/// call running now, and `body` left in the dump folder should it fail), its refusal as
/// SolidCraft's error ([`blend_refused`]).
fn ops<T>(call: &'static str, body: &Body, f: impl FnOnce() -> solidcore_ops::Result<T>) -> Result<T> {
    super::call::ops_as(call, &[("brep", body)], blend_refused, f)
}

/// A refusal of a blend's own step as [`refused`] words it, except that what the kernel cannot
/// build yet keeps the kernel's reason ([`Error::Unported`]) instead of the bare
/// [`Error::Unsupported`], which the other callers of [`refused`] word in their own terms.
fn blend_refused(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    match e {
        solidcore_ops::Error::Unsupported(why) if why != MIXED_CORNER_RAILS_CROSS => Error::Unported(why),
        e => refused(call, e, detail),
    }
}

/// `e`, a refusal of the entry point `call` on `body` that is this crate's own (a pick that
/// chose nothing): while a dump folder is set, `body` is left
/// there, as for a refusal of solidcore-ops'.
fn failed(call: &'static str, body: &Body, e: Error) -> Error {
    super::dump::failed_ops(call, &e.to_string(), &[("brep", body)]);
    e
}

/// A refusal of a solidcore-ops blend as SolidCraft's error: the refusals SolidCraft words itself
/// by their fields as they are (a blend too large with the largest size that builds, a setback
/// too short, a swept turn not built, a corner no blend handles), the kernel's statuses as the
/// status, the blend refusals and failures ops types carried as they are ([`Error::Refused`]),
/// and every other error as any solidcore-ops call's ([`super::call::error`]).
pub(super) fn refused(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    use solidcore_ops::Error as Ops;
    match e {
        Ops::Status(code) => Error::Status(code),
        // the kernel's refusal of its input (`SM_ERR_INVALID_INPUT`), with its reason
        Ops::Refused { operation, reason } => Error::Rejected { operation, reason },
        Ops::NoRoom | Ops::SetbackTooLong { .. } => Error::NoRoom,
        Ops::TooLarge { size, kind, at, face, why } => Error::TooLarge { size, kind, at, face, why },
        Ops::SetbackTooShort { least, at, builds, .. } => Error::SetbackTooShort { least, at, builds },
        Ops::CornerUnsupported { why, at } => Error::CornerUnsupported { why, at },
        Ops::FaceClosesToPoint { at } => Error::FaceClosesToPoint { at },
        Ops::CornerRefused { corner } => Error::CornerRefused { corner },
        Ops::TurnRefused { room, why, at, .. } => Error::TurnRefused { room, why, at },
        Ops::NoEffect => Error::NoEffect,
        Ops::Unsupported(why) if why == MIXED_CORNER_RAILS_CROSS => Error::Unported(MIXED_CORNER_UNSUPPORTED),
        Ops::Unsupported(_) | Ops::Unported(_) => Error::Unsupported,
        e @ (Ops::BlendFailed { .. }
        | Ops::BlendNotCertified { .. }
        | Ops::EdgeNotBlendable { .. }
        | Ops::ContinuesInto { .. }
        | Ops::RoundTooSmall { .. }
        | Ops::ChamferAngleTooLarge { .. }
        | Ops::RoundsCross { .. }) => Error::Refused(e),
        e => super::call::error(call, e, detail),
    }
}

/// A pick by points as the operations take it: the entities picked, each once in the order its
/// first point picked it, with that point's index, and per point its entity's place among them.
struct Picks {
    entities: Vec<usize>,
    first: Vec<usize>,
    place: Vec<usize>,
}

/// The entities `each` point picked (their indices in the body's list), each once.
fn each_once(each: &[usize]) -> Picks {
    let mut picks = Picks { entities: Vec::new(), first: Vec::new(), place: Vec::with_capacity(each.len()) };
    for (i, &x) in each.iter().enumerate() {
        let at = match picks.entities.iter().position(|&e| e == x) {
            Some(at) => at,
            None => {
                picks.entities.push(x);
                picks.first.push(i);
                picks.entities.len() - 1
            }
        };
        picks.place.push(at);
    }
    picks
}

/// A pick's miss as [`Miss`].
fn miss(m: solidcore_ops::pick::Miss) -> Miss {
    match m {
        solidcore_ops::pick::Miss::None(point) => Miss::None(point),
        solidcore_ops::pick::Miss::Ambiguous(point) => Miss::Ambiguous(point),
    }
}

/// `pick_edges` on solidcore-ops: the edges of `body` nearest `points`, by their indices in its
/// edge list.
fn picked_edges(call: &'static str, body: &Body, points: &[[f64; 3]], tolerance: f64) -> Result<Picks> {
    match ops(call, body, || solidcore_ops::pick::edges(body, points, tolerance))? {
        Ok(each) => Ok(each_once(&each)),
        Err(m) => Err(failed(call, body, edge_miss(miss(m)))),
    }
}

/// A copy of `body` for an operation to build on (`SmApiBrepCopy`): solidcore-ops' operations
/// change the body they are given.
fn copy(call: &'static str, body: &Body) -> Result<Body> {
    ops(call, body, || body.copy())
}

/// Runs `f` on a copy of `body` (`brep_copy`).
pub(super) fn on_copy(call: &'static str, body: &Body, f: impl FnOnce(&mut Kernel, Brep) -> solidcore::Result<Out<Brep>>) -> Result<Body> {
    if body.kernel_left_unfinished() {
        return Err(Error::Internal(format!("{call}: the body was left unfinished by an earlier panic")));
    }
    super::run_on(call, &[("brep", body)], |k| {
        let c = brep_copy(k, &body.kernel())?;
        f(k, c)
    })?
    .map(Body::from_kernel)
}

/// What [`blend`] does with the picked corners' patches.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum Corners {
    /// Filled, as SMLib builds them (`smlib_blend`, `smlib_blend_setback`).
    Filled,
    /// Removed, the holes listed (`smlib_blend_setback_open`).
    Open,
}

/// [`blend_holes`] without its holes.
#[allow(clippy::too_many_arguments)] // the capi's blend_call
pub(crate) fn blend(
    body: &Body,
    kind: Blend,
    points: &[[f64; 3]],
    sizes: &[f64],
    tolerance: f64,
    corners: &[[f64; 3]],
    setbacks: &[f64],
    open: Corners,
) -> Result<Body> {
    blend_holes(body, kind, points, sizes, tolerance, corners, setbacks, open).map(|(b, _)| b)
}

/// `smlib_blend_dropping`: [`blend`] with one size per point, no corners, and per point whether it
/// picked an edge that a larger round, overflowing its faces, removed whole.
pub(crate) fn blend_dropping(body: &Body, kind: Blend, turn: Turn, points: &[[f64; 3]], sizes: &[f64], tolerance: f64) -> Result<(Body, Vec<bool>)> {
    let mut dropped = vec![false; points.len()];
    let (b, _) = blend_call(body, kind, turn, points, sizes, tolerance, &[], &[], Corners::Filled, &mut dropped)?;
    Ok((b, dropped))
}

/// `blend_call` (as `smlib_fillet`, `smlib_chamfer`, `smlib_blend`, `smlib_blend_setback` and
/// `smlib_blend_setback_open` call it): `sizes` one per point, or one for all; `corners` and
/// `setbacks` the corner setbacks. With [`Corners::Open`], the holes, one per point of
/// `corners` (none otherwise).
#[allow(clippy::too_many_arguments)] // the capi's blend_call
pub(crate) fn blend_holes(
    body: &Body,
    kind: Blend,
    points: &[[f64; 3]],
    sizes: &[f64],
    tolerance: f64,
    corners: &[[f64; 3]],
    setbacks: &[f64],
    open: Corners,
) -> Result<(Body, Vec<Hole>)> {
    blend_call(body, kind, Turn::Mitre, points, sizes, tolerance, corners, setbacks, open, &mut vec![false; points.len()])
}

/// `blend_call`: [`blend_holes`], with per point in `dropped` whether it picked an edge an
/// overflowing round removed whole, and the rounds' turns at side edges as `turn` says (a swept
/// turn is for [`Blend::Round`] only).
///
/// The blend is solidcore-ops' ([`solidcore_ops::blend_with`]), on a copy of the body: the edges
/// and corners the points pick ([`solidcore_ops::pick`]) by their indices in the copy's lists.
/// An edge picked twice takes its first point's size, and a vertex picked twice its first
/// point's setback.
#[allow(clippy::too_many_arguments)] // the capi's blend_call
fn blend_call(
    body: &Body,
    kind: Blend,
    turn: Turn,
    points: &[[f64; 3]],
    sizes: &[f64],
    tolerance: f64,
    corners: &[[f64; 3]],
    setbacks: &[f64],
    open: Corners,
    dropped: &mut [bool],
) -> Result<(Body, Vec<Hole>)> {
    let one_size = sizes.len() == 1;
    if !points_ok(points, false)
        || !tolerance_ok(tolerance)
        || !(one_size || sizes.len() == points.len())
        || !points_ok(corners, true)
        || setbacks.len() != corners.len()
        || (turn == Turn::Swept && kind != Blend::Round)
    {
        return Err(Error::Argument);
    }
    if !sizes.iter().all(|&s| size_ok(s)) || !setbacks.iter().all(|&s| size_ok(s)) {
        return Err(Error::Argument);
    }
    // A chamfer's corners are bevelled, which builds no setback.
    if !corners.is_empty() && kind == Blend::Chamfer {
        return Err(Error::Unsupported);
    }
    const CALL: &str = "blend_call";
    let w = copy(CALL, body)?;
    let picked = picked_edges(CALL, &w, points, tolerance)?;
    let each: Vec<f64> = picked.first.iter().map(|&i| sizes.get(if one_size { 0 } else { i }).copied().unwrap_or(0.0)).collect();
    // Each corner point picks a vertex; a vertex picked twice takes the first point's setback.
    let at = match ops(CALL, &w, || solidcore_ops::pick::vertices(&w, corners, tolerance))? {
        Ok(each) => each_once(&each),
        Err(m) => return Err(failed(CALL, &w, corner_miss(miss(m)))),
    };
    let back: Vec<solidcore_ops::SetBack> = at
        .entities
        .iter()
        .zip(&at.first)
        .map(|(&vertex, &j)| solidcore_ops::SetBack { vertex, distance: setbacks.get(j).copied().unwrap_or(0.0) })
        .collect();
    let kind = match kind {
        Blend::Round => solidcore_ops::Blend::Round,
        Blend::Curvature => solidcore_ops::Blend::Curvature,
        Blend::Chord => solidcore_ops::Blend::Chord,
        Blend::Chamfer => solidcore_ops::Blend::Chamfer,
    };
    let options = solidcore_ops::BlendOptions {
        turn: match turn {
            Turn::Mitre => solidcore_ops::Turn::Mitre,
            Turn::Swept => solidcore_ops::Turn::Swept,
            Turn::Auto => solidcore_ops::Turn::Auto,
        },
        ..solidcore_ops::BlendOptions::default()
    };
    let corners_made = match open {
        Corners::Filled => solidcore_ops::Corners::Filled,
        Corners::Open => solidcore_ops::Corners::Open,
    };
    let blended =
        ops(CALL, &w, || solidcore_ops::blend_with(&w, kind, &picked.entities, &each, &back, corners_made, options)).map_err(|e| match e {
            // A corner SMLib does not set back: the first of the caller's points that picked it.
            Error::CornerRefused { corner } => at.first.get(corner).map_or(Error::Unsupported, |&corner| Error::CornerRefused { corner }),
            e => e,
        })?;
    // Every point that picked an edge an overflowing round removed whole.
    for (flag, &place) in dropped.iter_mut().zip(&picked.place) {
        *flag = blended.dropped.get(place).copied().unwrap_or(false);
    }
    // Each corner opened: the edges round its hole in the result's orders, once per corner point
    // (a vertex picked twice gives its hole twice).
    let mut holes = Vec::new();
    if open == Corners::Open {
        for &place in &at.place {
            let Some(hole) = blended.holes.get(place) else { return Err(Error::Argument) };
            holes.push(Hole { edges: hole.edges.iter().map(|h| HoleEdge { edge: h.edge, face: h.face, forward: h.forward }).collect() });
        }
    }
    Ok((w, holes))
}

/// `smlib_chamfer_sides`: the edges nearest `points` chamfered `distance` back along the face
/// whose outward normal leans most along `first`, and by `second` on the other, with bevelled
/// corners ([`solidcore_ops::chamfer_sides_facing`], on a copy of the body).
pub(crate) fn chamfer_sides(body: &Body, points: &[[f64; 3]], tolerance: f64, distance: f64, second: ChamferSecond, first: [f64; 3]) -> Result<Body> {
    let second_ok = match second {
        ChamferSecond::Angle(a) => a.is_finite() && a > 1e-6 && a < std::f64::consts::PI - 1e-6,
        ChamferSecond::Distance(d) => size_ok(d),
    };
    if !points_ok(points, false) || !tolerance_ok(tolerance) || !size_ok(distance) || !second_ok || !first.iter().all(|x| x.is_finite()) {
        return Err(Error::Argument);
    }
    if first.iter().all(|&x| x == 0.0) {
        return Err(Error::Argument);
    }
    const CALL: &str = "smlib_chamfer_sides";
    let w = copy(CALL, body)?;
    let picked = picked_edges(CALL, &w, points, tolerance)?;
    let second = match second {
        ChamferSecond::Distance(d) => solidcore_ops::ChamferSecond::Distance(d),
        ChamferSecond::Angle(a) => solidcore_ops::ChamferSecond::Angle(a),
    };
    ops(CALL, &w, || solidcore_ops::chamfer_sides_facing(&w, &picked.entities, distance, second, first))?;
    Ok(w)
}

/// `law_ok`: a clamped B-spline of `degree` 1 to 3 from 0 to 1, 2 to 100 control radii, each
/// finite, at least 0 and at most 1e6.
fn law_ok(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    let n = radii.len();
    if !(1..=3).contains(&degree) || !(2..=100).contains(&n) || n <= degree || knots.len() != n + degree + 1 {
        return false;
    }
    if !knots.iter().chain(radii).all(|x| x.is_finite()) {
        return false;
    }
    for (i, &k) in knots.iter().enumerate() {
        let (low_end, high_end) = (i <= degree, i >= n);
        if (low_end && k != 0.0) || (high_end && k != 1.0) || (i > 0 && knots.get(i - 1).is_some_and(|&p| k < p)) {
            return false;
        }
    }
    radii.iter().all(|&r| (0.0..=1e6).contains(&r))
}

/// `law_pinches`: whether the law's radius is zero somewhere inside (0, 1).
fn law_pinches(degree: usize, knots: &[f64], radii: &[f64]) -> bool {
    let n = radii.len() as i64;
    let p = degree as i64;
    let all_zero = |from: i64, to: i64| -> bool {
        let lo = from.max(0);
        let mut j = lo;
        while j <= to && j < n {
            if radii.get(j as usize).is_some_and(|&r| r != 0.0) {
                return false;
            }
            j += 1;
        }
        from <= to
    };
    let kn = |i: i64| knots.get(i as usize).copied().unwrap_or(f64::NAN);
    let n_knots = n + p + 1;
    let mut i = p;
    while i + 1 < n_knots && i < n {
        if kn(i) < kn(i + 1) && all_zero(i - p, i) {
            return true;
        }
        i += 1;
    }
    let mut s = p + 1;
    while s < n {
        let mut m = 1;
        while s + m < n && kn(s + m) == kn(s) {
            m += 1;
        }
        if all_zero(s + m - 1 - p, s - 1) {
            return true;
        }
        s += m;
    }
    false
}

/// `smlib_fillet_variable`: the one edge nearest `point` rounded with the radius law of
/// `degree`, `knots` and `radii`, 0 at the end nearest `start`
/// ([`solidcore_ops::fillet_variable_from`], on a copy of the body).
pub(crate) fn fillet_variable(
    body: &Body,
    point: [f64; 3],
    tolerance: f64,
    start: [f64; 3],
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<Body> {
    if !points_ok(&[point], false) || !start.iter().all(|x| x.is_finite()) || !tolerance_ok(tolerance) || !law_ok(degree, knots, radii) {
        return Err(Error::Argument);
    }
    if law_pinches(degree, knots, radii) {
        return Err(Error::Unsupported);
    }
    const CALL: &str = "smlib_fillet_variable";
    let w = copy(CALL, body)?;
    let picked = picked_edges(CALL, &w, &[point], tolerance)?;
    let [e] = picked.entities[..] else { return Err(Error::NoMatch { point: 0 }) };
    // The round is the union of its balls (smlib#78, solidcore#2262): each ball touches it along
    // the circle shifted -r r'/|c'| from its great circle. A law whose balls fold over one
    // another is refused, [`Error::NoRoom`].
    ops(CALL, &w, || solidcore_ops::fillet_variable_from(&w, e, start, degree, knots, radii))?;
    Ok(w)
}

/// The edges nearest `points`, one tangent chain, rounded with one radius law of `degree`,
/// `knots` and `radii` along the whole chain, 0 at the chain's end nearest `start`
/// ([`solidcore_ops::fillet_variable_chain_from`], on a copy of the body).
pub(crate) fn fillet_variable_chain(
    body: &Body,
    points: &[[f64; 3]],
    tolerance: f64,
    start: [f64; 3],
    degree: usize,
    knots: &[f64],
    radii: &[f64],
) -> Result<Body> {
    if !points_ok(points, false) || !start.iter().all(|x| x.is_finite()) || !tolerance_ok(tolerance) || !law_ok(degree, knots, radii) {
        return Err(Error::Argument);
    }
    if law_pinches(degree, knots, radii) {
        return Err(Error::Unsupported);
    }
    const CALL: &str = "smlib_fillet_variable_chain";
    let w = copy(CALL, body)?;
    let picked = picked_edges(CALL, &w, points, tolerance)?;
    ops(CALL, &w, || solidcore_ops::fillet_variable_chain_from(&w, &picked.entities, start, degree, knots, radii))?;
    Ok(w)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A B-spline method's failure passes on as the status it stands for, as the `Upstream` it
    /// replaced did; a cancellation or unported call a B-spline method passes through is not one.
    #[test]
    fn b_spline_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::nurbs::BSplineFailure;
        let op = "nurbs::b_spline_surface::p2::BSplineSurface::split_at";
        let failed = |b| Failure::new(op, NurbsFailure::BSpline(b)).into_error();
        assert!(matches!(status(failed(BSplineFailure::NoNurbsForm)), Error::Status(1001)));
        assert!(matches!(status(failed(BSplineFailure::RoutineFailed)), Error::Status(1001)));
        assert!(matches!(status(failed(BSplineFailure::Missing("the NURBS form"))), Error::Status(1006)));
        assert!(matches!(status(failed(BSplineFailure::ZeroAxis)), Error::Status(1007)));
        assert!(matches!(status(SolidError::unported("SmX", "x")), Error::Unsupported));
    }

    /// A sweep routine's failure passes on as the status it replaced, as the `Upstream` it replaced
    /// did; a railed loft's refusal, which replaced no status, keeps passing on as solidcore's text.
    #[test]
    fn sweep_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::sweep::SweepFailure;
        let sweep = |w| Failure::new("sweep::topology_sweep", w).into_error();
        assert!(matches!(status(sweep(SweepFailure::FaceAlreadySwept)), Error::Status(1001)));
        assert!(matches!(status(sweep(SweepFailure::NullTarget)), Error::Status(1006)));
        assert!(matches!(status(SolidError::sm_err("CopyFaces")), Error::Status(1001)));
        let cap = SweepFailure::RailPastStationCap { asked: 1e-6, reached: 1e-3, wires: 64 };
        assert!(matches!(status(sweep(cap)), Error::Solidcore(_)));
    }

    /// A curve routine's failure passes on as the status it replaced, as the `Upstream` it replaced
    /// did; a length bracket's refusal, which replaced no status, keeps passing on as solidcore's
    /// text.
    #[test]
    fn curve_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::curve::CurveFailure;
        let curve = |c| Failure::new("curve::curve::p2::Curve::create_by_split_curve", c).into_error();
        assert!(matches!(status(curve(CurveFailure::Failed("the split parameter is at the curve's end"))), Error::Status(1001)));
        assert!(matches!(status(curve(CurveFailure::Missing("the curve's cache"))), Error::Status(1006)));
        assert!(matches!(status(curve(CurveFailure::ZeroDirection("the auxiliary direction"))), Error::Status(1007)));
        let floor = CurveFailure::LengthBelowFloor { asked: 1e-12, floor: 1e-10, pieces: 8 };
        assert!(matches!(status(curve(floor)), Error::Solidcore(_)));
        let cap = CurveFailure::LengthPastPieceCap { asked: 1e-9, reached: 1e-6, pieces: 4096 };
        assert!(matches!(status(curve(cap)), Error::Solidcore(_)));
    }

    #[test]
    fn laws_are_checked_as_the_capi_checks_them() {
        assert!(law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[1.0, 2.0]));
        assert!(!law_ok(1, &[0.0, 0.0, 1.0], &[1.0, 2.0]));
        assert!(!law_ok(1, &[0.0, 0.5, 1.0, 1.0], &[1.0, 2.0]));
        assert!(!law_ok(1, &[0.0, 0.0, 1.0, 1.0], &[-1.0, 2.0]));
        assert!(!law_ok(4, &[0.0; 7], &[1.0, 2.0]));
        assert!(law_pinches(1, &[0.0, 0.0, 0.5, 1.0, 1.0], &[1.0, 0.0, 1.0]));
        assert!(!law_pinches(1, &[0.0, 0.0, 1.0, 1.0], &[0.0, 1.0]));
    }

    #[test]
    fn picks_follow_the_capis_rule() {
        let all = [0.0f64, 1.0, 3.0];
        let d = |x: f64, p: Point3| Ok(Some((x - p.x).abs()));
        assert_eq!(pick(&all, &[[0.9, 0.0, 0.0]], 0.5, 1e-6, d).unwrap(), Ok((vec![1.0], vec![0])));
        assert_eq!(pick(&all, &[[2.0, 0.0, 0.0]], 0.5, 1e-6, d).unwrap(), Err(Miss::None(0)));
        assert_eq!(pick(&all, &[[1.0, 0.0, 0.0], [0.5, 0.0, 0.0]], 1.0, 1e-6, d).unwrap(), Err(Miss::Ambiguous(1)));
        // The same entity twice is picked once, by its first point.
        assert_eq!(pick(&all, &[[3.1, 0.0, 0.0], [2.9, 0.0, 0.0]], 0.5, 1e-6, d).unwrap(), Ok((vec![3.0], vec![0])));
    }

    /// A query's failure passes on as the status it replaced, as the `Upstream` it replaced did.
    #[test]
    fn query_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::query::QueryFailure;
        let query = |q| Failure::new("query::edge", q).into_error();
        assert!(matches!(status(query(QueryFailure::Failed("the region has no shells"))), Error::Status(1001)));
        assert!(matches!(status(query(QueryFailure::Missing("the face's surface"))), Error::Status(1006)));
        assert!(matches!(status(query(QueryFailure::InvalidValue("a vector of zero length has no direction"))), Error::Status(1007)));
    }

    /// A topology routine's failure passes on as the status it replaced, as the `Upstream` it
    /// replaced did, whether or not it names an entity.
    #[test]
    fn topology_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::topology::TopologyFailure;
        let topology = |t| Failure::new("topology::edge", t).into_error();
        assert!(matches!(status(topology(TopologyFailure::Failed("the loop does not close"))), Error::Status(1001)));
        assert!(matches!(status(topology(TopologyFailure::Missing("the edge's curve"))), Error::Status(1006)));
        assert!(matches!(status(topology(TopologyFailure::InvalidValue("a zero normal"))), Error::Status(1007)));
        assert!(matches!(status(topology(TopologyFailure::Inconsistent("two outer loops"))), Error::Status(1005)));
        assert!(matches!(status(topology(TopologyFailure::DropFailed { status: 1010 })), Error::Status(1010)));
        let at = Failure::new("topology::vertexuse", TopologyFailure::Failed("x"))
            .at(solidcore::EntityRef::Vertex(solidcore::topology::VertexId::from_index(2)))
            .into_error();
        assert!(matches!(status(at), Error::Status(1001)));
    }

    /// A surface routine's failure passes on as the status it replaced, as the `Upstream` it
    /// replaced did.
    #[test]
    fn surface_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::surface::SurfaceFailure;
        let surface = |s| Failure::new("surface::surface::p2::Surface::step_inversion", s).into_error();
        assert!(matches!(status(surface(SurfaceFailure::Failed("the point is not on the generator"))), Error::Status(1001)));
        assert!(matches!(status(surface(SurfaceFailure::Missing("the surface's cache"))), Error::Status(1006)));
        assert!(matches!(status(surface(SurfaceFailure::InvalidValue("the line direction is the zero vector"))), Error::Status(1007)));
        assert!(matches!(status(surface(SurfaceFailure::OutsideDomain("the point is outside the sweep"))), Error::Status(1020)));
        assert!(matches!(status(surface(SurfaceFailure::OffSurface("the point is off the surface"))), Error::Status(1021)));
    }

    /// SMLib's rejection of its input reaches SolidCraft with solidcore's reason, not as the bare
    /// `SM_ERR_INVALID_INPUT` (#18570: an Offset Face that the enclosure check rejected; #18568: a
    /// shell whose input had a loop gap), and so does solidcore-ops' typed refusal of its input.
    #[test]
    fn rejected_input_says_why() {
        let e = status(SolidError::invalid_input("mass::enclosure", "a face whose sides are not one material region and one void"));
        assert_eq!(e, Error::Rejected { operation: "mass::enclosure", reason: "a face whose sides are not one material region and one void" });
        assert_eq!(e.to_string(), "SMLib rejected the input: a face whose sides are not one material region and one void (mass::enclosure)");
        let ops = refused("shell", solidcore_ops::Error::Refused { operation: "topology::brep::p7", reason: "loop gap larger than tolerance" }, None);
        assert_eq!(ops.to_string(), "SMLib rejected the input: loop gap larger than tolerance (topology::brep::p7)");
    }

    /// An offset routine's failure passes on as the status it replaced, as the `Upstream` it
    /// replaced did, whether or not it names a face; an offset's refusal does not pass on as a
    /// status.
    #[test]
    fn offset_failures_pass_on_as_their_status() {
        use solidcore::error::Failure;
        use solidcore::error::offset::OffsetFailure;
        let offset = |o| Failure::new("offset::offset_executive::crossing_loops", o).into_error();
        assert!(matches!(status(offset(OffsetFailure::Failed("x"))), Error::Status(1001)));
        assert!(matches!(status(offset(OffsetFailure::Missing("the face's surface"))), Error::Status(1006)));
        assert!(matches!(status(offset(OffsetFailure::InvalidValue("x"))), Error::Status(1007)));
        assert!(matches!(status(offset(OffsetFailure::Inconsistent("x"))), Error::Status(1005)));
        let at = Failure::new("offset::draft", OffsetFailure::InvalidValue("the face has no surface or no outer face use"))
            .at(solidcore::EntityRef::Face(solidcore::topology::FaceId::from_index(2)))
            .into_error();
        assert!(matches!(status(at), Error::Status(1007)));
        let folds = offset(OffsetFailure::Folds { face: solidcore::topology::FaceId::from_index(1), distance: 2.0, largest: 1.0 });
        assert!(!matches!(status(folds), Error::Status(_)));
    }

    /// An offset's refusal that stands for no status reaches SolidCraft as ops' typed refusal
    /// ([`Error::Refused`]), its reason and sizes as data, not as solidcore's text (#15032); the
    /// refusals with kinds of their own keep their errors.
    #[test]
    fn offset_refusals_pass_on_as_ops_refusals() {
        use solidcore::error::Failure;
        use solidcore::error::offset::{LoopCrossing, OffsetFailure};
        use solidcore_ops::{Error as Ops, OffsetRefusal};
        let face = solidcore::topology::FaceId::from_index(3);
        let offset = |o| Failure::new("offset::offset_executive::crossing_loops", o).into_error();
        let refused = |why| Error::Refused(Ops::OffsetRefused { face: None, why });
        assert_eq!(
            status(offset(OffsetFailure::Collapses { face, distance: -2.0, radius: 1.5 })),
            refused(OffsetRefusal::Collapses { distance: -2.0, radius: 1.5 })
        );
        assert_eq!(status(offset(OffsetFailure::NotCertified { face, distance: -1.0 })), refused(OffsetRefusal::NotCertified { distance: -1.0 }));
        let crossing = LoopCrossing::ThreeAtOnePoint;
        let e = status(offset(OffsetFailure::LoopsCross { face: Some(face), crossing }));
        assert_eq!(e, refused(OffsetRefusal::LoopsCross { crossing }));
        assert_eq!(
            e.to_string(),
            "the offset is refused: its offset's loops cross in a way the offset cannot resolve: three loops cross at one point"
        );
        assert_eq!(status(offset(OffsetFailure::SeamOpened { face })), refused(OffsetRefusal::SeamOpened));
        // A fold is the offset module's own error (`Error::WallFolds`), never ops' general refusal.
        assert!(!matches!(status(offset(OffsetFailure::Folds { face, distance: 2.0, largest: 1.0 })), Error::Refused(_) | Error::Status(_)));
    }

    /// A blend routine's failure passes on as the status it replaced, as the `Upstream` it
    /// replaced did, whether or not it names an edge, and keeps solidcore's reason for it
    /// ([`Error::BlendFailed`]): a bare status names no cause the user or a report can act on.
    #[test]
    fn blend_failures_pass_on_as_their_status_with_their_reason() {
        use solidcore::error::Failure;
        use solidcore::error::blend::BlendFailure;
        let blend = |b| Failure::new("blend::feature_executive", b).into_error();
        // the status and the reason the failure carries, and its class (the kernel's own)
        let carried = |e: Error| match e {
            Error::Refused(solidcore_ops::Error::BlendFailed { class, status, phrase }) => (status, phrase, class),
            e => panic!("not a typed blend failure: {e:?}"),
        };
        let x = |status: i32, phrase: &str| (status, phrase.to_owned());
        let cases = [
            (BlendFailure::Failed("x"), x(1001, "x")),
            (BlendFailure::Missing("the edge use's edge"), x(1006, "the edge use's edge is missing")),
            (BlendFailure::InvalidValue("x"), x(1007, "x")),
            (BlendFailure::Inconsistent("x"), x(1005, "a consistency check failed: x")),
            (BlendFailure::BadIntersections("x"), x(1027, "x")),
            (BlendFailure::NotConverged("x"), x(1022, "x")),
            (BlendFailure::PassedOn { what: "x", status: 1020 }, x(1020, "x")),
        ];
        for (failure, (want_status, want_phrase)) in cases {
            let (status, phrase, class) = carried(status(blend(failure)));
            assert_eq!((status, phrase.as_str(), class), (want_status, want_phrase.as_str(), failure.class()), "{failure:?}");
        }
        let at = Failure::new("blend::fillet_executive::p3", BlendFailure::Failed("x"))
            .at(solidcore::EntityRef::Edge(solidcore::topology::EdgeId::from_index(2)))
            .into_error();
        assert_eq!(carried(status(at)).0, 1001);
        // the trim of a blend whose faces leak past its rails, as the message says it
        let leak = blend(BlendFailure::Failed("the faces the rails enclose reach past the rails"));
        assert_eq!(status(leak).to_string(), "the blend failed (status 1001): the faces the rails enclose reach past the rails");
    }
}
