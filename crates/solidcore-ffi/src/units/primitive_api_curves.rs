//! FFI wrappers for unit `primitive.api_curves`. Shim:
//! `harness/cpp/shim/primitive_api_curves.cpp`. Upstream:
//! `source/SM_API/src/SmApiCurves.cpp` (`SmApiCreateLineSegment`, `SmApiCreateLine`,
//! `SmApiCreateCircle` (both overloads), `SmApiCreateEllipse`, `SmApiCreateArc`,
//! `SmApiCreateHelix`, `SmApiCreateRectangle`, `SmApiCreateRegularPolygon`,
//! `SmApiCreateCurve`, `SmApiCreateCanonicalCurve`, and `SmApiRemoveCurveKnots` on a
//! curve one of them made).
//!
//! Program layout (all `f64`): `op args...`, with `op` the [`Call`] discriminant:
//!
//! | op | args |
//! |----|------|
//! | 1 line segment | `start(3) end(3)` |
//! | 2 line | `start(3) direction(3)` |
//! | 3 B-spline circle | `center(3) radius` |
//! | 4 analytic circle | `center(3) radius` |
//! | 5 ellipse | `origin(3) radius1 radius2` |
//! | 6 arc | `center(3) radius start_deg end_deg` |
//! | 7 helix | `origin(3) height r0 r1 turns right_handed tol` |
//! | 8 rectangle | `center(3) length width` |
//! | 9 regular polygon | `center(3) sides radius` |
//! | 10 cubic curve | `n pts(3n)` |
//! | 11 canonical curve | `n pts(3n) nk knots(nk) mults(nk) has_w nw w(nw) degree form` |
//! | 12 remove knots | `op args...` of one of ops 1-7, 10, 11 |
//!
//! Output: the API status (`0` = `SM_SUCCESS`, else the `SmStatus`), then on success
//! the created curve: a B-spline is `type dim knot_type form has_nurb [n p m Pw(4(n+1))
//! U(m+1) natural_interval(2)]`; a line is `point(3) vector(3) step_interval(2) scale
//! inside_out` then its B-spline part; a circle / ellipse is `type origin(3) x(3) y(3)
//! step_interval(2) rx ry inside_out polar_possible nk knots na angles nx exact` then
//! its B-spline part; rectangle and polygon give `count` then each line. Under op 12
//! the creation status is followed, on success, by `SmApiRemoveCurveKnots`'s status and
//! then the curve's state after it.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_primitive_api_curves_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Most points of a curve.
pub const MAX_POINTS: usize = 4096;
/// Most sides of a polygon.
pub const MAX_SIDES: u64 = 4096;
/// Largest knot multiplicity.
pub const MAX_MULT: usize = 64;

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiCreateLineSegment(start, end)`.
    LineSegment {
        /// Start point.
        start: [f64; 3],
        /// End point.
        end: [f64; 3],
    },
    /// `SmApiCreateLine(start, direction)`.
    Line {
        /// Start point.
        start: [f64; 3],
        /// Direction.
        direction: [f64; 3],
    },
    /// `SmApiCreateCircle(center, radius, SmBSplineCurve*&)`.
    Circle {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateCircle(center, radius, SmCircle*&)`.
    AnalyticCircle {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateEllipse(origin, radius1, radius2)`.
    Ellipse {
        /// Origin.
        origin: [f64; 3],
        /// X radius.
        radius1: f64,
        /// Y radius.
        radius2: f64,
    },
    /// `SmApiCreateArc(center, radius, start_deg, end_deg)`.
    Arc {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
        /// Start angle (degrees).
        start_deg: f64,
        /// End angle (degrees).
        end_deg: f64,
    },
    /// `SmApiCreateHelix(...)`.
    Helix {
        /// Base point.
        origin: [f64; 3],
        /// Height.
        height: f64,
        /// Start radius.
        radius_start: f64,
        /// End radius.
        radius_end: f64,
        /// Turns.
        num_turns: f64,
        /// Right handed.
        right_handed: bool,
        /// Tolerance.
        tolerance: f64,
    },
    /// `SmApiCreateRectangle(center, length, width)`.
    Rectangle {
        /// Center.
        center: [f64; 3],
        /// Length (X).
        length: f64,
        /// Width (Y).
        width: f64,
    },
    /// `SmApiCreateRegularPolygon(center, sides, radius)`.
    Polygon {
        /// Center.
        center: [f64; 3],
        /// Number of sides (at most [`MAX_SIDES`]).
        sides: u64,
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateCurve(points)`.
    Curve {
        /// Control points (at most [`MAX_POINTS`]).
        points: Vec<[f64; 3]>,
    },
    /// `SmApiCreateCanonicalCurve(points, knots, mults, weights, degree, form)`.
    Canonical {
        /// Control points (1 to [`MAX_POINTS`]).
        points: Vec<[f64; 3]>,
        /// Unique knots (finite: a NaN or infinite knot corrupts upstream's heap,
        /// UB-238).
        knots: Vec<f64>,
        /// Multiplicity per knot (same length as `knots`, each at most [`MAX_MULT`]).
        mults: Vec<usize>,
        /// Optional weights (upstream rejects a count other than one per point).
        weights: Option<Vec<f64>>,
        /// Degree (0 to 64).
        degree: usize,
        /// `SmBSplineCurveForm` code (0 to 6).
        form: u32,
    },
    /// `SmApiRemoveCurveKnots` on the curve of one call (not a rectangle, polygon or
    /// another removal).
    RemoveKnots(Box<Call>),
}

fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::RemoveKnots(inner) => {
            if matches!(
                **inner,
                Call::Rectangle { .. } | Call::Polygon { .. } | Call::RemoveKnots(_)
            ) {
                return Err(bad("knot removal of a segment array or a removal"));
            }
            o.push(12.0);
            o.extend(encode(inner)?);
        }
        Call::LineSegment { start, end } => {
            o.push(1.0);
            o.extend_from_slice(start);
            o.extend_from_slice(end);
        }
        Call::Line { start, direction } => {
            o.push(2.0);
            o.extend_from_slice(start);
            o.extend_from_slice(direction);
        }
        Call::Circle { center, radius } => {
            o.push(3.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        Call::AnalyticCircle { center, radius } => {
            o.push(4.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        Call::Ellipse {
            origin,
            radius1,
            radius2,
        } => {
            o.push(5.0);
            o.extend_from_slice(origin);
            o.extend([*radius1, *radius2]);
        }
        Call::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => {
            o.push(6.0);
            o.extend_from_slice(center);
            o.extend([*radius, *start_deg, *end_deg]);
        }
        Call::Helix {
            origin,
            height,
            radius_start,
            radius_end,
            num_turns,
            right_handed,
            tolerance,
        } => {
            o.push(7.0);
            o.extend_from_slice(origin);
            o.extend([
                *height,
                *radius_start,
                *radius_end,
                *num_turns,
                if *right_handed { 1.0 } else { 0.0 },
                *tolerance,
            ]);
        }
        Call::Rectangle {
            center,
            length,
            width,
        } => {
            o.push(8.0);
            o.extend_from_slice(center);
            o.extend([*length, *width]);
        }
        Call::Polygon {
            center,
            sides,
            radius,
        } => {
            if *sides > MAX_SIDES {
                return Err(bad("too many sides"));
            }
            o.push(9.0);
            o.extend_from_slice(center);
            o.extend([*sides as f64, *radius]);
        }
        Call::Curve { points } => {
            if points.len() > MAX_POINTS {
                return Err(bad("too many points"));
            }
            o.push(10.0);
            o.push(points.len() as f64);
            o.extend(points.iter().flatten());
        }
        Call::Canonical {
            points,
            knots,
            mults,
            weights,
            degree,
            form,
        } => {
            if points.len() > MAX_POINTS || knots.len() > MAX_POINTS {
                return Err(bad("too many points or knots"));
            }
            if mults.len() != knots.len() || mults.iter().any(|&m| m > MAX_MULT) {
                return Err(bad("multiplicities"));
            }
            // UB-238: a NaN knot makes CreateCanonical write past a heap block (glibc
            // aborts later, in N_EndNurbs's free or the next realloc)
            if knots.iter().any(|k| !k.is_finite()) {
                return Err(bad("non-finite knot"));
            }
            if *degree > 64 || *form > 6 {
                return Err(bad("degree or form"));
            }
            if weights.as_ref().is_some_and(|w| w.len() > MAX_POINTS) {
                return Err(bad("too many weights"));
            }
            // Mirrors the shim: upstream reads these without checking (after its degree
            // and weight count checks).
            let total: usize = mults.iter().sum();
            let weights_ok = weights.as_ref().is_none_or(|w| w.len() == points.len());
            if (1..=32).contains(degree)
                && weights_ok
                && (points.is_empty() || total < degree + 1 || total < points.len() + degree + 1)
            {
                return Err(bad("too few knots or points"));
            }
            o.push(11.0);
            o.push(points.len() as f64);
            o.extend(points.iter().flatten());
            o.push(knots.len() as f64);
            o.extend_from_slice(knots);
            o.extend(mults.iter().map(|&m| m as f64));
            match weights {
                Some(w) => {
                    o.extend([1.0, w.len() as f64]);
                    o.extend_from_slice(w);
                }
                None => o.extend([0.0, 0.0]),
            }
            o.extend([*degree as f64, f64::from(*form)]);
        }
    }
    Ok(o)
}

/// Runs one API call; returns the record described in the module docs.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim would reject; the shim's status
/// otherwise.
pub fn run(call: &Call) -> Result<Vec<f64>, FfiError> {
    let prog = encode(call)?;
    let n_in = i64::try_from(prog.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(256, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_primitive_api_curves_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// UB-238 (`--fuzz 30000` fuzz21932, fuzz21945): a NaN knot is refused before the
    /// shim runs.
    #[test]
    fn canonical_refuses_a_non_finite_knot() {
        for bad_knot in [f64::NAN, f64::INFINITY] {
            let call = Call::Canonical {
                points: vec![[0.0, 0.0, 0.0], [1.0, 0.0, 0.0]],
                knots: vec![0.0, bad_knot],
                mults: vec![2, 2],
                weights: None,
                degree: 1,
                form: 0,
            };
            let error = encode(&call).unwrap_err().to_string();
            assert!(error.contains("non-finite knot"), "{error}");
        }
    }
}
