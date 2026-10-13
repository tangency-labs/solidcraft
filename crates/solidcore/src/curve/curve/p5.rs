//! `SmCurve`, part 5 (so far only functions co-ported ahead of the unit): the
//! base-class `IsLine`, sampling a curve for a straight, constant-speed line, with its
//! dispatch over the curve kinds, the linearity test `IsLinear`, and building the
//! curve cache of `CacheMakeOrValidate`.
//!
//! Upstream: `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! | Rust                               | Upstream                                    |
//! |------------------------------------|---------------------------------------------|
//! | [`is_line_base`]                   | `SmCurve::IsLine` (the base-class body)     |
//! | [`Curve::is_line`]                 | `IsLine` (virtual: base, B-spline and line) |
//! | [`Curve::is_linear`], [`Linearity`] | `SmCurve::IsLinear`                        |
//! | [`make_curve_cache`]               | `SmCurve::CacheMakeOrValidate` (building a new cache) |
//!
//! Deviations:
//! - **Co-port.** Unit `curve.curve_p5` owns these functions. Unit
//!   `curve.curve_cache` ported `IsLine` ahead of the unit because every curve
//!   tessellation reaches it (through `SmBSplineCurve::CalculateBoundingBox` and
//!   `SmBSplineCurve::IsLine`). It is written for the only kind that reaches the base
//!   class so far, [`BSplineCurve`] (whose `IsLine` override defers to it). Unit
//!   `curve.curve_p3` ported `IsLinear` (non-virtual) because
//!   `SmCurve::ComputeCrvsNormal` calls it on every input curve. Unit `curve.curve_p1`
//!   ported the [`Curve`] dispatch of `IsLine` (`SmCurve::TessellateByBisection` calls
//!   it on every curve; the base body is written once more over [`Curve`] for the
//!   kinds without an override) and the new-cache branch of `CacheMakeOrValidate`
//!   ([`make_curve_cache`]: `SmCurve::GlobalPointSolve` gets the curve's cache through
//!   `SmCacheMgr::GetOrCreateObjectCache`). The rest of the unit is not ported yet;
//!   its porter should route [`crate::geom::a_object::get_or_create_object_cache`]'s
//!   `CacheMaker::Curve` through [`make_curve_cache`].
//! - **Caches.** Upstream keeps the cache on the curve and reuses it until the curve
//!   changes; [`make_curve_cache`] builds a fresh one for the caller. The build is
//!   deterministic, so a cache built per call holds the same spans and tree.
//!   `cpOldCache` (an existing cache) is the caller's to reuse; the `SM_OC_CURVE`
//!   type check and the context check have no counterpart (a [`Curve`] always has
//!   the kernel's context).
//! - **Out-params.** `rLinePoint` / `rLineVector` are returned with the flag as
//!   `Some((point, vector))`; `IsLinear`'s optional outputs are the fields of
//!   [`Linearity`] (`want_deviation` stands for a non-`NULL` `pOptActualDeviation`).
//! - **Ignored statuses.** `GetEnds` and the sample evaluations ignore their status
//!   upstream; a failed evaluation keeps the previous sample (initially
//!   [`Vec3::UNDEF`]), except [`SolidError::Unported`](crate::SolidError), which is
//!   propagated.

use crate::curve::curve::p2::{Curve, CurveKind, BSPLINE_CURVE_TYPE};
use crate::curve::curve_cache::{CurveCache, CurveCacheOptions};
use crate::error::{ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::geom_utility::p1::line_point_distance;
use crate::geom::vector3d::{Point3, Vec3};
use crate::kernel::Kernel;
use crate::math::{EFF_ZERO, EFF_ZERO_SQ};
use crate::nurbs::b_spline_curve::BSplineCurve;

/// `SmCurve::IsLine` for `curve`: the start point and the start-to-end vector when
/// `num_samples` (at least 3) evenly spaced samples step along the chord by equal
/// vectors (within `tol`) with tangents parallel to it (1 degree); `None` for a
/// curve shorter than `tol` or one that is not such a line. A non-rational degree 1
/// curve with two control points is a line.
///
/// # Errors
///
/// `SM_ERR` without a NURB; [`SolidError::Unported`] from the evaluations.
pub fn is_line_base(
    curve: &BSplineCurve,
    num_samples: u64,
    tol: f64,
) -> Result<Option<(Point3, Vec3)>> {
    let num_samples = num_samples.max(3);

    // locals
    let domain: Extent1d = curve.natural_interval();
    // (GetEnds has no status; it only fails without a NURB here)
    let ends = curve.ends()?;
    let (start_pt, end_pt) = (ends.start, ends.end);

    let base_vec = end_pt - start_pt;
    if base_vec.length_squared() < tol * tol {
        return Ok(None);
    }

    // A quick check:
    if curve.degree()? == 1 && curve.number_control_points()? == 2 && !curve.is_rational()? {
        return Ok(Some((start_pt, base_vec)));
    }

    let num_spans = (num_samples - 1) as f64;

    // standard step vector: total base vector divided by number of spans
    let standard_vec = base_vec / num_spans;

    let mut pt1 = start_pt;
    let mut pt_vec = [Vec3::UNDEF; 2];
    for ii in 1..num_samples {
        let pt0 = pt1;

        let t = if ii == num_samples - 1 {
            domain.max()
        } else {
            domain.evaluate(ii as f64 / num_spans)
        };

        // exact tangent values (status ignored)
        match curve.evaluate(t, 1, false, false) {
            Ok(v) => {
                if let Some(&p) = v.first() {
                    pt_vec[0] = p;
                }
                if let Some(&d) = v.get(1) {
                    pt_vec[1] = d;
                }
            }
            Err(e @ SolidError::Unported { .. }) => return Err(e),
            Err(_) => {}
        }
        pt1 = pt_vec[0];
        let test_vec = pt1 - pt0;
        let error = test_vec - standard_vec;
        if error.length_squared() > tol * tol {
            return Ok(None);
        }

        // Check angle.
        if !pt_vec[1].is_parallel_to(base_vec, 1.0) {
            return Ok(None);
        }
    }

    // Looks like a line.
    Ok(Some((start_pt, base_vec)))
}

/// `SmCurve::IsLine` (the base-class body) for any [`Curve`]: as [`is_line_base`],
/// through the curve's virtual queries.
///
/// # Errors
///
/// Those of the degree, control point count and rationality queries;
/// [`SolidError::Unported`] from the evaluations.
fn curve_is_line_base(curve: &Curve, num_samples: u64, tol: f64) -> Result<Option<(Point3, Vec3)>> {
    let num_samples = num_samples.max(3);

    // locals
    let domain = curve.natural_interval();
    // (GetEnds has no status upstream)
    let (start_pt, end_pt) = match ignored(curve.ends())? {
        Some(e) => (e.start, e.end),
        None => (Vec3::UNDEF, Vec3::UNDEF),
    };

    let base_vec = end_pt - start_pt;
    if base_vec.length_squared() < tol * tol {
        return Ok(None);
    }

    // A quick check:
    if curve.degree()? == 1 && curve.number_control_points()? == 2 && !curve.is_rational()? {
        return Ok(Some((start_pt, base_vec)));
    }

    let num_spans = (num_samples - 1) as f64;

    // standard step vector: total base vector divided by number of spans
    let standard_vec = base_vec / num_spans;

    let mut pt1 = start_pt;
    let mut pt_vec = [Vec3::UNDEF; 2];
    for ii in 1..num_samples {
        let pt0 = pt1;

        let t = if ii == num_samples - 1 {
            domain.max()
        } else {
            domain.evaluate(ii as f64 / num_spans)
        };

        // exact tangent values (status ignored)
        if let Some(v) = ignored(curve.evaluate(t, 1, false, false))? {
            for (d, s) in pt_vec.iter_mut().zip(v) {
                *d = s;
            }
        }
        pt1 = pt_vec[0];
        let test_vec = pt1 - pt0;
        let error = test_vec - standard_vec;
        if error.length_squared() > tol * tol {
            return Ok(None);
        }

        // Check angle.
        if !pt_vec[1].is_parallel_to(base_vec, 1.0) {
            return Ok(None);
        }
    }

    // Looks like a line.
    Ok(Some((start_pt, base_vec)))
}

/// `SmCurve::CacheMakeOrValidate` building a new cache: the curve's [`CurveCache`]
/// over its natural interval, a full tree with chord height tolerance
/// `ApproximateLength(5) / min(250, 30 * spans)`, angle tolerance 15 degrees,
/// propagated parent data and minimum parametric ratio `0.0001`, tessellated.
/// `handle` is the curve's handle in the caller's object space (the tree's owner
/// object).
///
/// # Errors
///
/// Those of the length and knot count queries and of [`CurveCache::tessellate`]
/// (upstream then has no cache).
pub fn make_curve_cache<O: Copy + PartialEq>(
    k: &Kernel,
    curve: &Curve,
    handle: Option<O>,
) -> Result<CurveCache<O>> {
    // cast Object to SmCurve and get its interval
    let ivl = curve.natural_interval();

    // pick dChordHeightTol and dAngularTol tessellation control parameter values

    // get an Idea of the curve's size.
    let length = curve.approximate_length(&ivl, 5, false)?.0;
    // (int arithmetic: GetNumberNaturalKnots() - 1, then 30 * NumberOfSpans)
    let number_of_spans = curve.number_natural_knots()? as i64 - 1;
    let chord_factor = 250.min(30 * number_of_spans) as f64; // 0.004 of length may be too tight

    // set chordHeight tolerance based on curve size
    let chord_height_tol = length / chord_factor;

    // set angular tolerance
    let angular_tol_deg = 15.0; // 15 degrees might be ok

    // construct new CurveCache Object
    let mut cache = CurveCache::new(
        curve.clone(),
        ivl,
        CurveCacheOptions {
            make_full_tree: true,
            chord_height_tolerance: chord_height_tol,
            angle_tol_deg: angular_tol_deg,
            off_axis_tolerance: 0.0,
            min_segments: 0,
            compute_aux_tree_data: true,
            max_3d_distance: 0.0,
            min_parametric_ratio: 0.0001,
        },
    );
    cache.set_curve_handle(handle);

    // build the CruveCache's curve subdivision tree
    cache.tessellate(k)?;
    Ok(cache)
}

impl Curve {
    /// `IsLine(lNumSamples, dTol, ..)`: the line point and vector when the curve is a
    /// straight, constant-speed line ([`BSplineCurve::is_line`] for B-splines and
    /// hyperbolas, always for a line, else the base body sampling `num_samples`
    /// points as [`is_line_base`]).
    ///
    /// # Errors
    ///
    /// Those of the overrides and of the curve queries; [`SolidError::Unported`] from
    /// the evaluations.
    pub fn is_line(&self, num_samples: u64, tol: f64) -> Result<Option<(Point3, Vec3)>> {
        match self.kind() {
            CurveKind::BSpline(c) => c.is_line(num_samples, tol),
            CurveKind::Hyperbola(c) => c.base().is_line(num_samples, tol),
            CurveKind::Line(l) => Ok(Some(l.is_line())),
            CurveKind::Hermite(_)
            | CurveKind::Conic(_)
            | CurveKind::Projected(_)
            | CurveKind::Iso(_)
            | CurveKind::Offset(_)
            | CurveKind::CrvInVolume(_) => curve_is_line_base(self, num_samples, tol),
        }
    }
}

/// Result of [`Curve::is_linear`] (`SmCurve::IsLinear`).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Linearity {
    /// The return value: the curve lies on a line (within the tolerance).
    pub linear: bool,
    /// `*pOptLinePoint`: a point on the line, written only when [`Self::linear`]
    /// ([`Vec3::UNDEF`] otherwise).
    pub line_point: Point3,
    /// `*pOptLineTangent`: the unit line direction, written only when
    /// [`Self::linear`] ([`Vec3::UNDEF`] otherwise).
    pub line_tangent: Vec3,
    /// `*pOptActualDeviation`: the largest sample distance from the line (`0` unless
    /// the deviation was asked for and the samples were reached).
    pub deviation: f64,
}

impl Curve {
    /// `SmCurve::IsLinear(dTol, ..)`: whether the curve lies on a line within `tol`.
    /// A B-spline with a NURB and two control points is linear when its start tangent
    /// is longer than `tol` (and `SM_EFF_ZERO`). Otherwise a base line from the start
    /// point (to the end point, or to the 0.38 / 0.62 samples of a closed curve) is
    /// checked at 9 more samples: each must lie within `tol` of the line and, unless
    /// the deviation is asked for, have a tangent parallel to it (1 degree; zero
    /// tangents are skipped). With `want_deviation` the samples only accumulate the
    /// largest distance and the curve is linear when it is below `tol`.
    ///
    /// # Errors
    ///
    /// Those of the control point count; [`SolidError::Unported`] from the
    /// evaluations (their other failures are ignored, as upstream).
    pub fn is_linear(&self, tol: f64, want_deviation: bool) -> Result<Linearity> {
        let mut out = Linearity {
            linear: false,
            line_point: Vec3::UNDEF,
            line_tangent: Vec3::UNDEF,
            deviation: 0.0,
        };

        // Special case check: a B-Spline with only two points.
        if self.is_kind_of(BSPLINE_CURVE_TYPE) {
            let has_nurb = match self.kind() {
                CurveKind::BSpline(c) => c.gw_nurb().is_some(),
                CurveKind::Line(c) => c.base().gw_nurb().is_some(),
                CurveKind::Hyperbola(c) => c.base().gw_nurb().is_some(),
                _ => false,
            };
            // don't force building Nurb
            if has_nurb && self.number_control_points()? == 2 {
                let mut pv = [Vec3::UNDEF; 2];
                // exact Tangent values (status ignored)
                if let Some(v) =
                    ignored(self.evaluate(self.natural_interval().min(), 1, false, false))?
                {
                    for (d, s) in pv.iter_mut().zip(v) {
                        *d = s;
                    }
                }

                let len = pv[1].length();
                if len > tol && len > EFF_ZERO {
                    out.linear = true;
                    out.line_point = pv[0];
                    out.line_tangent = pv[1] / len;
                }
                // else: zero-length line.
                return Ok(out);
            }
        }

        let domain = self.natural_interval();

        // Get a basis: a line that (might) contain the whole curve.
        // First try vector from start to end, and if that doesn't work
        // (closed curve), try a couple of interior points.
        let (start_pt, end_pt) = match ignored(self.ends())? {
            Some(e) => (e.start, e.end),
            None => (Vec3::UNDEF, Vec3::UNDEF),
        };
        let mut pt = Vec3::UNDEF;
        let mut base_vec = end_pt - start_pt;
        if base_vec.length_squared() < tol * tol {
            if let Some(p) = ignored(self.evaluate_point(domain.evaluate(0.38)))? {
                pt = p;
            }
            base_vec = pt - start_pt;
            if base_vec.length_squared() < tol * tol {
                if let Some(p) = ignored(self.evaluate_point(domain.evaluate(0.62)))? {
                    pt = p;
                }
                base_vec = pt - start_pt;
                if base_vec.length_squared() < tol * tol {
                    // Four coincident points: must be degenerate.
                    return Ok(out);
                }
            }
        }

        // We have a basis.  Unitize it.
        let Ok(base_vec) = base_vec.unitize() else {
            // Again, must be degenerate w.r.t. SM_EFF_ZERO.
            return Ok(out);
        };

        // Sample at several points looking for off-basis points.
        // Note, as in IsLine(), we also have to check angles. (Helix counterexample.)
        let mut pt_vec = [Vec3::UNDEF; 2];
        let num_samples: u64 = 10;
        let num_spans = (num_samples - 1) as f64; // for convenience

        for ii in 1..num_samples {
            let frac = ii as f64 / num_spans;
            let t = domain.evaluate(frac);
            // nonZeroTangent values (status ignored)
            if let Some(v) = ignored(self.evaluate(t, 1, false, true))? {
                for (d, s) in pt_vec.iter_mut().zip(v) {
                    *d = s;
                }
            }
            let (dist, _) = line_point_distance(start_pt, base_vec, pt_vec[0]);

            if want_deviation {
                if dist > out.deviation {
                    out.deviation = dist;
                }
            } else {
                if dist > tol {
                    return Ok(out);
                }

                // Don't return False because of a single zero-length derivative. [B424]
                // (IsParallelTo() returns False for a zero vector.)
                if pt_vec[1].length_squared() <= EFF_ZERO_SQ {
                    // The criterion used in SmParallelTo()
                    continue;
                }

                // Check angle.
                if !pt_vec[1].is_parallel_to(base_vec, 1.0) {
                    return Ok(out);
                }
            }
        } // end for each sample point.

        // Either it's all within tol, or we're calculating pOptActualDeviation.
        let ret = !want_deviation || out.deviation < tol;
        if ret {
            out.linear = true;
            out.line_point = start_pt;
            out.line_tangent = base_vec;
        }
        Ok(out)
    }
}
