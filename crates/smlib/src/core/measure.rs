//! The Measure and Check families on solidcore: what `capi/src/smlib_measure.cpp`,
//! `smlib_brep_solid_count` (`smlib_ops.cpp`), `smlib_brep_is_manifold_solid`
//! (`smlib_flat_api.cpp`) and `smlib_brep_check` (`smlib_check.cpp`) do on the C++ kernel, here
//! composed from solidcore's ports of the same SMLib routines.
//!
//! The bounds, the manifold and solid-count tests, point classification, the edge/face gap and
//! the check, mass properties and area are solidcore-ops' ([`solidcore_ops::Body`]). [`check`] fails with "not supported yet" while a test it reaches is unported in
//! solidcore.

use solidcore::topology::brep::p1 as bl;
use solidcore_ops::PointLocation;
use solidcore_ops::check::ValidityScope;

use super::call::{ops_as, ops_on};
use super::{Body, with_body};
use crate::check::{CheckLevel, CheckReport};
use crate::measure::{MassProperties, PointClass};
use crate::{Error, Result};

const OP: &str = "smlib_measure";

/// The accuracy the C API passes on: finite, clamped to [1e-8, 1e-1].
pub(super) fn accuracy(relative_accuracy: f64) -> Result<f64> {
    if !relative_accuracy.is_finite() {
        return Err(Error::Argument);
    }
    Ok(relative_accuracy.clamp(1e-8, 1e-1))
}

fn finite(v: &[f64]) -> bool {
    v.iter().all(|x| x.is_finite())
}

/// `smlib_brep_mass_properties`: the mass properties at unit density, summed over the body's
/// material regions, with the area of its sheet faces counted; a body with no material gives
/// its faces' area and their centroid, no volume and no inertia. solidcore-ops' estimate
/// ([`Body::mass_properties`]) at the C API's accuracy, finite and clamped to [1e-8, 1e-1].
pub(crate) fn mass_properties(body: &Body, relative_accuracy: f64) -> Result<MassProperties> {
    let accuracy = accuracy(relative_accuracy)?;
    let m = ops_as("mass_properties", &[("brep", body)], measure_error, || body.mass_properties(accuracy))?;
    Ok(MassProperties { area: m.area, volume: m.volume, centroid: m.centroid, moments: m.moments, products: m.products })
}

/// `smlib_brep_area`: every face counted once ([`Body::area`]).
pub(crate) fn area(body: &Body, relative_accuracy: f64) -> Result<f64> {
    let accuracy = accuracy(relative_accuracy)?;
    ops_as("area", &[("brep", body)], measure_error, || body.area(accuracy))
}

/// solidcore-ops' refusal of a measure as the C API's: a body it cannot measure (no mass, no
/// area, a result that is not finite) is the input rejected, in the kernel's words; the rest as
/// any solidcore-ops call's.
fn measure_error(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    match e {
        solidcore_ops::Error::Refused { operation, reason } => Error::Rejected { operation, reason },
        e => super::call::error(call, e, detail),
    }
}

/// `smlib_brep_bounds`: `SmBrep::CalculateTightBoundingBox` ([`Body::bounds`]); no box (not
/// finite, or its corners out of order) is the capi's rejected input.
pub(crate) fn bounds(body: &Body) -> Result<([f64; 3], [f64; 3])> {
    ops_as(
        "bounds",
        &[("brep", body)],
        |call, e, detail| match e {
            solidcore_ops::Error::Status(solidcore_ops::error::INVALID_INPUT) => Error::Rejected { operation: OP, reason: "no box" },
            e => super::call::error(call, e, detail),
        },
        || body.bounds(),
    )
}

/// `smlib_brep_edge_face_distance`: SMLib's own edge/face gap (`SmEdgeuse::CalcMaxEdgeFaceGap`'s
/// `SmCrvSrfGapFunction` with 20 samples, its largest sample: [`Body::edge_face_largest_gap`]).
/// Indices in the body's edge and face order.
pub(crate) fn edge_face_distance(body: &Body, edge: usize, face: usize) -> Result<f64> {
    ops_on("edge_face_distance", &[("brep", body)], || {
        let order = body.own_order();
        body.edge_face_largest_gap(&order, edge, face)
    })
}

/// A proved upper bound on the distance from the record curve `curve` (a spline or rational
/// spline) over its parameters `interval` to the surface of face `face` (the body's face
/// order): [`solidcore_ops::curve_gap::curve_face_gap_bound`]. `None` when the
/// kernel does not enclose the gap or there is no such face.
pub(crate) fn curve_face_gap_bound(
    body: &Body,
    curve: &solidcore_record::brep::Curve,
    interval: [f64; 2],
    face: usize,
    floor: f64,
) -> Result<Option<f64>> {
    // a circle or ellipse is the kernel's exact rational spline, over its interval there
    let (curve, interval) = match solidcore_ops::curve_gap::GapCurve::of_conic(curve, interval).map_err(|e| Error::Solidcore(e.to_string()))? {
        Some(found) => found,
        None => (solidcore_ops::curve_gap::GapCurve::new(curve).ok_or(Error::Argument)?, interval),
    };
    with_body("curve_face_gap_bound", body, |k, b| {
        let Some(&f) = bl::faces(b, None)?.get(face) else {
            return Ok(None);
        };
        solidcore_ops::curve_gap::curve_face_gap_bound(k, b, f, &curve, interval, floor)
    })
}

/// `smlib_brep_classify_point`: `SmBrep::Point3DClassify` at the body's own zone tolerance
/// (`SmTol::GetZoneTol3d` of the body), with boundary intersections, so a point within that
/// tolerance of a vertex, an edge or a face is on the boundary and never decided by a ray; else
/// the region it lies in, outside when that region is void (`IsVoid() == TRUE`; `UNSURE` is
/// material, as the capi compares with `TRUE`). The classification is the modelling layer's
/// (`solidcore_ops::measure::locate_points`), which runs on a private copy when a solve needs one:
/// upstream's `const` method may add UV trim curves to the body.
pub(crate) fn classify_point(body: &Body, p: [f64; 3]) -> Result<PointClass> {
    if !finite(&p) {
        return Err(Error::Argument);
    }
    classify_points(body, &[p])?.pop().unwrap_or(Err(Error::Status(SM_ERR)))
}

/// [`classify_point`] for each of `points`, through one classification of the body
/// (`locate_points`): the body's private copy and its search structures are made once for all of
/// them, not once a point (on a large body they cost far more than a point's own solve). Each
/// point's class, or its own error.
pub(crate) fn classify_points(body: &Body, points: &[[f64; 3]]) -> Result<Vec<Result<PointClass>>> {
    if points.iter().all(|p| !finite(p)) {
        return Ok(points.iter().map(|_| Err(Error::Argument)).collect());
    }
    let located = ops_on("classify_points", &[("brep", body)], || body.locate_points(points))?;
    Ok(located.into_iter().map(|l| point_class(l.map_err(|e| super::call::error("classify_point", e, None)))).collect())
}

/// [`classify_point`]'s class of a point the modelling layer placed. A region the kernel is unsure
/// of is material (the capi's `IsVoid() == TRUE` test), where `solidcore_ops`' own
/// `Body::classify_point` refuses it.
fn point_class(l: Result<PointLocation>) -> Result<PointClass> {
    match l? {
        PointLocation::OnBoundary => Ok(PointClass::OnBoundary),
        PointLocation::Outside => Ok(PointClass::Outside),
        PointLocation::Inside | PointLocation::Unsure { .. } => Ok(PointClass::Inside),
    }
}

/// `SM_ERR`.
const SM_ERR: i32 = 1001;

/// `smlib_brep_is_manifold_solid`: `SmApiBrepIsManifoldSolid`.
pub(crate) fn is_manifold_solid(body: &Body) -> Result<bool> {
    ops_on("is_manifold_solid", &[("brep", body)], || body.is_manifold_solid())
}

/// `smlib_brep_solid_count`: the solids of `SmApiBrepMaterialCensus` (its voids are dropped).
pub(crate) fn solid_count(body: &Body) -> Result<usize> {
    ops_on("solid_count", &[("brep", body)], || body.solid_count())
}

/// `smlib_brep_check` (`smlib_check.cpp`): `SmBrep::ValidatePointers` at `level`, then, when it
/// failed nothing, `SmBrep::AssertValid` at `level` over the whole topology graph, with every
/// failed test as one line of the report and the capi's own lines for a failed status without a
/// report. solidcore-ops' [`Body::check_scoped`] runs the same two calls and builds the same lines.
///
/// The geometry owners are named as the body's naming scheme names them: `check_brep` reads a
/// `NamingScheme::Caller` body's keys from where its maker recorded them (solidcore#1585: an
/// array import through `make_topology_from_data`, `api_brep_copy`); where it recorded none, the
/// arena identities the binders name such an entity by. A `NamingScheme::Unknown` body would be
/// checked under arena identities no maker uses: every edge would fail `SmBrep` test 61 on the
/// owner of its curve.
///
/// A test solidcore has not ported yet (its `SolidError::Unported`), or a body whose geometry
/// owners' identities the check cannot know, fails the check with "not supported yet": the check
/// never reports a body valid on fewer tests than the C++ kernel runs.
pub(crate) fn check(body: &Body, level: CheckLevel) -> Result<CheckReport> {
    check_with(body, level, false)
}

/// [`check`], stopped by the token over this thread's work when `stoppable` (a body the caller
/// consumes); a body others read too is never stopped part way (solidcore#3247).
pub(crate) fn check_with(body: &Body, level: CheckLevel, stoppable: bool) -> Result<CheckReport> {
    check_scoped(body, level, stoppable, ValidityScope::Body)
}

/// [`check_with`] holding the tolerance pass to `scope`: [`ValidityScope::Stitched`] for a body a
/// stitch produced.
pub(crate) fn check_scoped(body: &Body, level: CheckLevel, stoppable: bool, scope: ValidityScope) -> Result<CheckReport> {
    let lv = match level {
        CheckLevel::Basic => solidcore_ops::check::CheckLevel::Basic,
        CheckLevel::Geometry => solidcore_ops::check::CheckLevel::Geometry,
        CheckLevel::All => solidcore_ops::check::CheckLevel::All,
    };
    let checked = || ops_as("check", &[("brep", body)], refused, || body.check_scoped(lv, scope));
    // a body others read too is checked whatever the token says (solidcore#3247)
    let c = if stoppable { checked()? } else { crate::stop::unstoppable(checked)? };
    Ok(CheckReport { failures: c.failures, text: c.text })
}

/// The check's refusal as the capi words it: a test solidcore has not ported, or a body whose
/// geometry owners' identities the check cannot know, is "not supported yet".
fn refused(call: &'static str, e: solidcore_ops::Error, detail: Option<String>) -> Error {
    match e {
        solidcore_ops::Error::Unported(upstream) => {
            Error::Solidcore(format!("smlib_brep_check: {upstream}: not supported yet: solidcore has not ported it"))
        }
        // solidcore-ops' one refusal of a body it cannot check: the naming scheme it cannot know
        solidcore_ops::Error::Unsupported(_) => Error::Solidcore(
            "smlib_brep_check: not supported yet: the body names its geometry owners by its maker's keys (NamingScheme::Unknown), which the check cannot know"
                .into(),
        ),
        e => super::call::error(call, e, detail),
    }
}
