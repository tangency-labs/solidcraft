//! `SmCurve`, part 5 (so far only the line test co-ported by unit
//! `curve.curve_cache`): the base-class `IsLine`, sampling a curve for a straight,
//! constant-speed line.
//!
//! Upstream: `source/SMLib/src/SmCurve.cpp`, `source/SMLib/inc/SmCurve.h`.
//!
//! | Rust                 | Upstream                                    |
//! |----------------------|---------------------------------------------|
//! | [`is_line_base`]     | `SmCurve::IsLine` (the base-class body)     |
//!
//! Deviations:
//! - **Co-port.** Unit `curve.curve_p5` owns this function; unit `curve.curve_cache`
//!   ported it ahead of the unit because every curve tessellation reaches it (through
//!   `SmBSplineCurve::CalculateBoundingBox` and `SmBSplineCurve::IsLine`). It is
//!   written for the only kind that reaches the base class so far,
//!   [`BSplineCurve`] (whose `IsLine` override defers to it); the rest of the unit
//!   (the [`crate::curve::curve::Curve`] dispatch included) is not ported yet.
//! - **Out-params.** `rLinePoint` / `rLineVector` are returned with the flag as
//!   `Some((point, vector))`.
//! - **Ignored statuses.** `GetEnds` and the sample evaluations ignore their status
//!   upstream; a failed evaluation keeps the previous sample (initially
//!   [`Vec3::UNDEF`]), except [`SolidError::Unported`](crate::SolidError), which is
//!   propagated.

use crate::error::{Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::vector3d::{Point3, Vec3};
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
