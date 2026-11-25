//! Untrim and extend: the C API's `smlib_untrim_face` and `smlib_extend_sheet`.
//!
//! Untrim is solidcore-ops' ([`solidcore_ops::untrim::untrim_face`]): the face and the holes are
//! picked here as the capi's `smlib_untrim.cpp` picked them (the face nearest the point, the
//! inner loop nearest each hole point), and ops restores the face towards its surface.
//!
//! Extend is solidcore-ops' too ([`solidcore_ops::extend::extend_sheet`]): the edge is picked
//! here as `smlib_extend.cpp` picked it (the nearest to the point by the kernel's closest point,
//! within the tolerance), and ops continues the sheet past the side that edge runs along: its
//! surface extended exactly by the distance (a plane face that is its plane's whole box grows its
//! plane, a trimmed one its trim), given back its analytic kind, the new face and each of its
//! sides taking the attributes of the face and side they continue.

use solidcore::query::api_queries::face_internal_point;

use super::Body;
use super::blend::refused;
use super::call::ops_as;
use crate::trim::Continuation;
use crate::{Error, Result};

/// `MAX_HOLE_POINTS`.
const MAX_HOLE_POINTS: usize = 10_000;

/// The index of the least of `distances` within `tol`, the first of equal ones; an entry that
/// is `None` or not finite is passed over (`face_at` and the nearest hole, as the capi picked).
pub(super) fn nearest_within(distances: impl IntoIterator<Item = Option<f64>>, tol: f64) -> Option<usize> {
    let (mut best, mut d0) = (None, f64::INFINITY);
    for (i, d) in distances.into_iter().enumerate() {
        if let Some(d) = d.filter(|d| d.is_finite())
            && d < d0
        {
            d0 = d;
            best = Some(i);
        }
    }
    best.filter(|_| d0 <= tol)
}

/// `smlib_untrim_face`: the face of `body` nearest `point` (within `tol`) restored towards its
/// surface: the holes nearest `holes` (all of them for `None`) taken away and, with `outer`, its
/// outer boundary back on the surface's parameter box. The face alone, or with `in_place` the
/// whole body with it replaced ([`solidcore_ops::untrim::untrim_face`]).
///
/// The picks are the capi's: the face nearest `point` as it is trimmed
/// ([`solidcore_ops::Body::face_distances_within`]), and per hole point the inner loop of that
/// face whose nearest edge is nearest it ([`solidcore_ops::Body::face_loops`],
/// [`solidcore_ops::Body::edge_closest_point`]), each within `tol`, the first of equal ones.
pub(crate) fn untrim_face(body: &Body, point: [f64; 3], tol: f64, holes: Option<&[[f64; 3]]>, outer: bool, in_place: bool) -> Result<Body> {
    let n_holes = holes.map(<[_]>::len);
    if !point.iter().all(|x| x.is_finite())
        || !tol.is_finite()
        || !(tol > 0.0)
        || n_holes.is_some_and(|n| n > MAX_HOLE_POINTS)
        || holes.is_some_and(|h| !h.iter().flatten().all(|x| x.is_finite()))
        || (n_holes == Some(0) && !outer)
    {
        return Err(Error::Argument);
    }
    if body.kernel_left_unfinished() {
        return Err(Error::Internal("smlib_untrim_face: the body was left unfinished by an earlier panic".to_owned()));
    }
    const CALL: &str = "smlib_untrim_face";
    // A pick that chose nothing is the call's refusal, its body left in the dump folder as a
    // failed call leaves it.
    let missed = || {
        let e = Error::NoMatch { point: 0 };
        super::dump::failed_ops(CALL, &e.to_string(), &[("brep", body)]);
        e
    };
    let order = body.own_order();
    let rows = ops_as(CALL, &[("brep", body)], refused, || body.face_distances_within(&order, &[point], tol))?;
    let face = nearest_within(rows.iter().map(|row| row.first().copied().flatten()), tol).ok_or_else(missed)?;
    // The holes taken away: all of them, or the one nearest each point, named by an edge of it.
    let mut along: Vec<usize> = Vec::new();
    if let Some(points) = holes {
        let loops = ops_as(CALL, &[("brep", body)], refused, || body.face_loops(&order, face))?;
        let inner = loops.get(1..).unwrap_or_default();
        let mut taken: Vec<usize> = Vec::new();
        for &p in points {
            let mut distances = Vec::with_capacity(inner.len());
            for edges in inner {
                let mut d = f64::INFINITY;
                for &e in edges {
                    let hit = ops_as(CALL, &[("brep", body)], refused, || body.edge_closest_point(&order, e, p))?;
                    if let Some(hit) = hit.filter(|h| h.distance.is_finite()) {
                        d = d.min(hit.distance);
                    }
                }
                distances.push(Some(d));
            }
            let l = nearest_within(distances, tol).ok_or_else(missed)?;
            if !taken.contains(&l) {
                taken.push(l);
                along.extend(inner.get(l).and_then(|edges| edges.first()));
            }
        }
    }
    let which = if holes.is_some() { solidcore_ops::untrim::Holes::Along(&along) } else { solidcore_ops::untrim::Holes::All };
    ops_as(CALL, &[("brep", body)], refused, || solidcore_ops::untrim::untrim_face(body, &order, face, which, outer, in_place))
}

/// `smlib_extend_sheet`: the sheet `sheet` continued `distance` past the side of its surface's
/// box that its edge nearest `point` (within `tol`) runs along, as `how` says: solidcore-ops'
/// Extend ([`solidcore_ops::extend::extend_sheet`]), the face and each side carrying the
/// attributes of those they continue. The edge is picked as the capi picked it: the nearest by
/// the kernel's closest point, the first of equal ones, within `tol`. A sheet ops cannot extend is
/// [`Error::Unsupported`]; an extension that cannot be made `distance` long across the side
/// (within 1e-9 of it), or one below the side's resolution, is [`Error::NotExact`], as before.
pub(crate) fn extend_sheet(sheet: &Body, point: [f64; 3], tol: f64, distance: f64, how: Continuation) -> Result<Body> {
    const CALL: &str = "smlib_extend_sheet";
    if !point.iter().all(|x| x.is_finite()) || !tol.is_finite() || !(tol > 0.0) || !distance.is_finite() || !(distance > 0.0) {
        return Err(Error::Argument);
    }
    let order = sheet.own_order();
    let mut distances = Vec::with_capacity(order.edges.len());
    for e in 0..order.edges.len() {
        let at = ops_as(CALL, &[("sheet", sheet)], extend_error, || sheet.edge_closest_point(&order, e, point))?;
        distances.push(at.map(|c| c.distance));
    }
    let edge = nearest_within(distances, tol).ok_or(Error::NoMatch { point: 0 })?;
    let how = match how {
        Continuation::Natural => solidcore_ops::extend::Continuation::Natural,
        Continuation::Tangent => solidcore_ops::extend::Continuation::Tangent,
        Continuation::Curvature => solidcore_ops::extend::Continuation::Curvature,
    };
    ops_as(CALL, &[("sheet", sheet)], extend_error, || solidcore_ops::extend::extend_sheet(sheet, &order, edge, distance, how))
}

/// solidcore-ops' refusal of an Extend as the capi words it: a sheet it cannot extend is
/// [`Error::Unsupported`]; a distance it cannot meet exactly, or one too short for the side to
/// carry, is [`Error::NotExact`]; the rest as solidcore-ops' calls are worded ([`refused`]).
fn extend_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    use solidcore_ops::Error as Ops;
    match e {
        Ops::Argument => Error::Argument,
        Ops::Unsupported(_) => Error::Unsupported,
        Ops::NotExact | Ops::TooFar { .. } | Ops::NoEffect => Error::NotExact,
        e => refused(call, e, detail),
    }
}

/// A point inside each face, in the body's face order (`smlib_brep_face_points`:
/// `SmFace::CalculateAnInternalPoint`).
pub(crate) fn face_points(body: &Body) -> Result<Vec<[f64; 3]>> {
    super::with_body("smlib_brep_face_points", body, |k, b| {
        let faces = b.face_list().to_vec();
        faces.into_iter().map(|f| face_internal_point(k, b, f).map(|p| [p.x, p.y, p.z])).collect()
    })
}
