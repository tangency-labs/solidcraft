//! FFI wrappers for unit `primitive.api_surface_ops`. Shim:
//! `harness/cpp/shim/primitive_api_surface_ops.cpp`. Upstream:
//! `source/SM_API/src/SmApiSurfaces.cpp` (`SmApiCreateSurface`,
//! `SmApiCreateSurfaceFromCornerPoints`, `SmApiCreateSurfaceFromOrderedPoints`,
//! `SmApiCreateOffsetSurface`) and
//! `source/SM_API/src/SmApiCurves.cpp` (`SmApiDropCurveToSrf`, `SmApiLiftUVCurveFromSrf`).
//!
//! Program layout (all `f64`): `op args...`, with `op` the [`Call`] discriminant:
//!
//! | op | args |
//! |----|------|
//! | 1 net surface | `rows columns n pts(3n)` |
//! | 2 corner surface | `corners(4x3)` |
//! | 3 offset | `surface distance` |
//! | 4 drop | `curve surface lift` |
//! | 5 lift | `uv_curve surface` |
//! | 6 ordered-points surface | `rows columns n pts(3n)` |
//!
//! A `surface` is `kind args` ([`InSurface`]): `0 rows columns n pts(3n)`
//! (`SmApiCreateSurface`), `1 corners(4x3)` (`SmApiCreateSurfaceFromCornerPoints`), `2
//! center(3) radius` (the first face's surface of `SmApiCreateSphere`) or `3 center(3) major
//! minor` (`SmApiCreateTorus`, surface overload) or `4 rows columns n pts(3n)`
//! (`SmApiCreateSurfaceFromOrderedPoints`). A
//! `curve` is `kind args` ([`InCurve`]): `0 n pts(3n)` (`SmApiCreateCurve`), `1 center(3)
//! radius` (`SmApiCreateCircle`), `2 center(3) radius start_deg end_deg`
//! (`SmApiCreateArc`), `3 start(3) end(3)` (`SmApiCreateLineSegment`).
//!
//! Output: `-2` when an input's constructor fails; else the API status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then on success:
//! - ops 1, 2 and 6: `type`, the B-spline surface `has_nurb knot_type form [n m p q r s
//!   Pw(4(n+1)(m+1)) U(r+1) V(s+1)]`, then the samples;
//! - op 3: the samples of the offset surface;
//! - op 4: `count` then each UV curve, then with `lift` `count` then each 3D curve;
//! - op 5: `count` (1) then the 3D curve.
//!
//! The samples of a surface are `type domain(4)` then, at the domain's `(0, .5, 1)` grid
//! (u outer), `status point(3)` (the point on success). A curve is the B-spline `type
//! dim knot_type form has_nurb [n p m Pw(4(n+1)) U(m+1) natural_interval(2)]`.

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_primitive_api_surface_ops_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Most points of a net or curve.
pub const MAX_POINTS: usize = 4096;
/// Largest row or column count.
pub const MAX_COUNT: u64 = 1 << 40;

/// One input surface, made by an `SM_API` constructor.
#[derive(Debug, Clone, PartialEq)]
pub enum InSurface {
    /// `SmApiCreateSurface(points, rows, columns)`.
    Net {
        /// Row count (at most [`MAX_COUNT`]).
        rows: u64,
        /// Column count (at most [`MAX_COUNT`]).
        columns: u64,
        /// Control points, row by row (at most [`MAX_POINTS`]).
        points: Vec<[f64; 3]>,
    },
    /// `SmApiCreateSurfaceFromCornerPoints(umin_vmin, umax_vmin, umin_vmax, umax_vmax)`.
    Corners {
        /// The corners in that order.
        corners: [[f64; 3]; 4],
    },
    /// The first face's surface of `SmApiCreateSphere(center, radius)` (owned by the face).
    SphereFace {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateTorus(center, major, minor, SmSurface*&)`.
    Torus {
        /// Center.
        center: [f64; 3],
        /// Major radius.
        major: f64,
        /// Minor radius.
        minor: f64,
    },
    /// `SmApiCreateSurfaceFromOrderedPoints(points, rows, columns)`.
    Ordered {
        /// Row count (at most [`MAX_COUNT`]).
        rows: u64,
        /// Column count (at most [`MAX_COUNT`]).
        columns: u64,
        /// The grid points, row by row (at most [`MAX_POINTS`]).
        points: Vec<[f64; 3]>,
    },
}

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
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiCreateSurface(points, rows, columns, rpResult)`.
    Net {
        /// The net.
        rows: u64,
        /// Column count.
        columns: u64,
        /// Control points.
        points: Vec<[f64; 3]>,
    },
    /// `SmApiCreateSurfaceFromCornerPoints(corners..., rpResult)`.
    Corners {
        /// The corners (`umin_vmin`, `umax_vmin`, `umin_vmax`, `umax_vmax`).
        corners: [[f64; 3]; 4],
    },
    /// `SmApiCreateSurfaceFromOrderedPoints(points, rows, columns, rpResult)`.
    Ordered {
        /// Row count.
        rows: u64,
        /// Column count.
        columns: u64,
        /// The grid points, row by row.
        points: Vec<[f64; 3]>,
    },
    /// `SmApiCreateOffsetSurface(surface, distance, rpResult)`.
    Offset {
        /// The surface.
        surface: InSurface,
        /// Offset distance.
        distance: f64,
    },
    /// `SmApiDropCurveToSrf(curve, surface, &uv, lift ? &c3d : NULL)`.
    Drop {
        /// The curve.
        curve: InCurve,
        /// The surface.
        surface: InSurface,
        /// Whether the 3D curves are asked for.
        lift: bool,
    },
    /// `SmApiLiftUVCurveFromSrf(uv_curve, surface, &c3d)`.
    Lift {
        /// The UV curve (x, y are u, v).
        uv_curve: InCurve,
        /// The surface.
        surface: InSurface,
    },
}

/// A net or grid `rows columns n pts(3n)`.
fn put_net(o: &mut Vec<f64>, rows: u64, columns: u64, points: &[[f64; 3]]) -> Result<(), FfiError> {
    if rows > MAX_COUNT || columns > MAX_COUNT || points.len() > MAX_POINTS {
        return Err(bad("net too large"));
    }
    // inputs undefined in C++ are refused (held record)
    if rows.checked_mul(columns).is_none() && rows.wrapping_mul(columns) == points.len() as u64 {
        return Err(bad("unsupported grid size"));
    }
    if (rows == 0 || columns == 0) && rows.max(columns) > MAX_POINTS as u64 {
        return Err(bad("unsupported grid size"));
    }
    o.extend([rows as f64, columns as f64, points.len() as f64]);
    o.extend(points.iter().flatten());
    Ok(())
}

fn put_surface(o: &mut Vec<f64>, s: &InSurface) -> Result<(), FfiError> {
    match s {
        InSurface::Net {
            rows,
            columns,
            points,
        } => {
            o.push(0.0);
            put_net(o, *rows, *columns, points)?;
        }
        InSurface::Corners { corners } => {
            o.push(1.0);
            o.extend(corners.iter().flatten());
        }
        InSurface::SphereFace { center, radius } => {
            o.push(2.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        InSurface::Torus {
            center,
            major,
            minor,
        } => {
            o.push(3.0);
            o.extend_from_slice(center);
            o.extend([*major, *minor]);
        }
        InSurface::Ordered {
            rows,
            columns,
            points,
        } => {
            o.push(4.0);
            put_net(o, *rows, *columns, points)?;
        }
    }
    Ok(())
}

fn put_curve(o: &mut Vec<f64>, c: &InCurve) -> Result<(), FfiError> {
    match c {
        InCurve::Curve { points } => {
            if points.len() > MAX_POINTS {
                return Err(bad("too many points"));
            }
            o.extend([0.0, points.len() as f64]);
            o.extend(points.iter().flatten());
        }
        InCurve::Circle { center, radius } => {
            o.push(1.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        InCurve::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => {
            o.push(2.0);
            o.extend_from_slice(center);
            o.extend([*radius, *start_deg, *end_deg]);
        }
        InCurve::Segment { start, end } => {
            o.push(3.0);
            o.extend_from_slice(start);
            o.extend_from_slice(end);
        }
    }
    Ok(())
}

fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Net {
            rows,
            columns,
            points,
        } => {
            o.push(1.0);
            put_net(&mut o, *rows, *columns, points)?;
        }
        Call::Corners { corners } => {
            o.push(2.0);
            o.extend(corners.iter().flatten());
        }
        Call::Ordered {
            rows,
            columns,
            points,
        } => {
            o.push(6.0);
            put_net(&mut o, *rows, *columns, points)?;
        }
        Call::Offset { surface, distance } => {
            o.push(3.0);
            put_surface(&mut o, surface)?;
            o.push(*distance);
        }
        Call::Drop {
            curve,
            surface,
            lift,
        } => {
            o.push(4.0);
            put_curve(&mut o, curve)?;
            put_surface(&mut o, surface)?;
            o.push(if *lift { 1.0 } else { 0.0 });
        }
        Call::Lift { uv_curve, surface } => {
            o.push(5.0);
            put_curve(&mut o, uv_curve)?;
            put_surface(&mut o, surface)?;
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
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_primitive_api_surface_ops_run(
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
    fn ffi_refuses_unsupported_grid_sizes() {
        let net = |rows, columns| Call::Net {
            rows,
            columns,
            points: Vec::new(),
        };
        let ordered = |rows, columns| Call::Ordered {
            rows,
            columns,
            points: Vec::new(),
        };
        for call in [net, ordered] {
            assert!(encode(&call(MAX_COUNT, MAX_COUNT)).is_err());
            assert!(encode(&call(MAX_COUNT, MAX_COUNT - 1)).is_ok());
            assert!(encode(&call(MAX_COUNT, 0)).is_err());
            assert!(encode(&call(0, MAX_COUNT)).is_err());
            assert!(encode(&call(7, 0)).is_ok());
            assert!(encode(&call(0, 0)).is_ok());
        }
    }
}
