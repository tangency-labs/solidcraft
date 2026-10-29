//! FFI wrappers for unit `primitive.api_curve_ops`. Shim:
//! `harness/cpp/shim/primitive_api_curve_ops.cpp`. Upstream:
//! `source/SM_API/src/SmApiCurves.cpp` (`SmApiCreateCurveInterpPoints`,
//! `SmApiOffsetCurve`, `SmApiEvaluateContinuity`, `SmApiMakeCurvesCompatible`,
//! `SmApiOrderCurves`).
//!
//! Program layout (all `f64`): `op args...`, with `op` the [`Call`] discriminant:
//!
//! | op | args |
//! |----|------|
//! | 1 interpolation | `n pts(3n) parameterization` (`SM_CP_*`, 0 to 2) |
//! | 2 offset | `curve distance` |
//! | 3 continuity | `curve relative parameter` |
//! | 4 make compatible | `count curve...` (kinds 0 to 2) |
//! | 5 order | `count curve...` (kinds 0 to 5) |
//!
//! With `relative = 1` the continuity parameter is the curve's natural interval evaluated
//! there (`SmExtent1d::Evaluate`).
//!
//! A `curve` is `kind args`, made by an `SM_API` constructor ([`InCurve`]): `0 n pts(3n)`
//! (`SmApiCreateCurve`), `1 center(3) radius` (`SmApiCreateCircle`, B-spline),
//! `2 center(3) radius start_deg end_deg` (`SmApiCreateArc`), `3 start(3) end(3)`
//! (`SmApiCreateLineSegment`), `4 center(3) radius` (`SmApiCreateCircle`, `SmCircle`),
//! `5 center(3) length width` (`SmApiCreateRectangle`, its 4 segments).
//!
//! Output: `-2` when an input curve's constructor fails; else the API status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then on success: a B-spline `type dim knot_type
//! form has_nurb [n p m Pw(4(n+1)) U(m+1) natural_interval(2)]` (interpolation, offset);
//! the `SmContinuityType` code (continuity); `count` then each curve as a B-spline (make
//! compatible); `count` then each curve's input index (order).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_primitive_api_curve_ops_run(
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
/// Most curves of a list.
pub const MAX_CURVES: usize = 64;

/// One input curve, made by an `SM_API` constructor.
#[derive(Debug, Clone, PartialEq)]
pub enum InCurve {
    /// `SmApiCreateCurve(points)`.
    Curve {
        /// Control points (at most [`MAX_POINTS`]).
        points: Vec<[f64; 3]>,
    },
    /// `SmApiCreateCircle(center, radius, SmBSplineCurve*&)`.
    Circle {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
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
    /// `SmApiCreateLineSegment(start, end)`.
    Segment {
        /// Start point.
        start: [f64; 3],
        /// End point.
        end: [f64; 3],
    },
    /// `SmApiCreateCircle(center, radius, SmCircle*&)`.
    AnalyticCircle {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateRectangle(center, length, width)`: its 4 segments (lists only).
    Rectangle {
        /// Center.
        center: [f64; 3],
        /// Length (X).
        length: f64,
        /// Width (Y).
        width: f64,
    },
}

impl InCurve {
    /// The shim's kind code.
    const fn kind(&self) -> u32 {
        match self {
            Self::Curve { .. } => 0,
            Self::Circle { .. } => 1,
            Self::Arc { .. } => 2,
            Self::Segment { .. } => 3,
            Self::AnalyticCircle { .. } => 4,
            Self::Rectangle { .. } => 5,
        }
    }
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiCreateCurveInterpPoints(points, rpResult, parameterization)`.
    Interp {
        /// Points (at most [`MAX_POINTS`]).
        points: Vec<[f64; 3]>,
        /// `SmCurveParameterizationType` code (0 to 2).
        parameterization: u32,
    },
    /// `SmApiOffsetCurve(curve, distance, rpResult)`.
    Offset {
        /// The curve (not a rectangle).
        curve: InCurve,
        /// Offset distance.
        distance: f64,
    },
    /// `SmApiEvaluateContinuity(curve, parameter, rType)`.
    Continuity {
        /// The curve (not a rectangle).
        curve: InCurve,
        /// Parameter (relative to the natural interval with `relative`).
        parameter: f64,
        /// `parameter` is relative to the curve's natural interval.
        relative: bool,
    },
    /// `SmApiMakeCurvesCompatible(curves)`.
    Compatible {
        /// The curves (B-spline kinds: curve, circle, arc).
        curves: Vec<InCurve>,
    },
    /// `SmApiOrderCurves(curves)`.
    Order {
        /// The curves (any kind; a rectangle adds its 4 segments).
        curves: Vec<InCurve>,
    },
}

fn put_curve(o: &mut Vec<f64>, c: &InCurve, max_kind: u32) -> Result<(), FfiError> {
    if c.kind() > max_kind {
        return Err(bad("curve kind not allowed here"));
    }
    o.push(f64::from(c.kind()));
    match c {
        InCurve::Curve { points } => {
            if points.len() > MAX_POINTS {
                return Err(bad("too many points"));
            }
            o.push(points.len() as f64);
            o.extend(points.iter().flatten());
        }
        InCurve::Circle { center, radius } | InCurve::AnalyticCircle { center, radius } => {
            o.extend_from_slice(center);
            o.push(*radius);
        }
        InCurve::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => {
            o.extend_from_slice(center);
            o.extend([*radius, *start_deg, *end_deg]);
        }
        InCurve::Segment { start, end } => {
            o.extend_from_slice(start);
            o.extend_from_slice(end);
        }
        InCurve::Rectangle {
            center,
            length,
            width,
        } => {
            o.extend_from_slice(center);
            o.extend([*length, *width]);
        }
    }
    Ok(())
}

fn put_list(o: &mut Vec<f64>, curves: &[InCurve], max_kind: u32) -> Result<(), FfiError> {
    if curves.len() > MAX_CURVES {
        return Err(bad("too many curves"));
    }
    o.push(curves.len() as f64);
    for c in curves {
        put_curve(o, c, max_kind)?;
    }
    Ok(())
}

fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Interp {
            points,
            parameterization,
        } => {
            if points.len() > MAX_POINTS {
                return Err(bad("too many points"));
            }
            if *parameterization > 2 {
                return Err(bad("parameterization"));
            }
            o.push(1.0);
            o.push(points.len() as f64);
            o.extend(points.iter().flatten());
            o.push(f64::from(*parameterization));
        }
        Call::Offset { curve, distance } => {
            o.push(2.0);
            put_curve(&mut o, curve, 4)?;
            o.push(*distance);
        }
        Call::Continuity {
            curve,
            parameter,
            relative,
        } => {
            o.push(3.0);
            put_curve(&mut o, curve, 4)?;
            o.extend([if *relative { 1.0 } else { 0.0 }, *parameter]);
        }
        Call::Compatible { curves } => {
            o.push(4.0);
            put_list(&mut o, curves, 2)?;
        }
        Call::Order { curves } => {
            o.push(5.0);
            put_list(&mut o, curves, 5)?;
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
            sys::sc_primitive_api_curve_ops_run(
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

    #[test]
    fn a_rectangle_is_refused_where_one_curve_is_expected() {
        let call = Call::Offset {
            curve: InCurve::Rectangle {
                center: [0.0; 3],
                length: 1.0,
                width: 1.0,
            },
            distance: 1.0,
        };
        assert!(encode(&call).is_err());
    }
}
