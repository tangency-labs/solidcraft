//! FFI wrappers for unit `primitive.api_trim_ops`. Shim:
//! `harness/cpp/shim/primitive_api_trim_ops.cpp`. Upstream:
//! `source/SM_API/src/SmApiTrimmedSurfaces.cpp` (`SmApiTrimSurfaceWith3dCurves`,
//! `SmApiTrimProjectParallel`, `SmApiCreatePlanarFaces`) and
//! `source/SM_API/src/SmApiBrep.cpp` (`SmApiProjectAndTrim`).
//!
//! Program layout (all `f64`): `op args...`, with `op` the [`Call`] discriminant:
//!
//! | op | args |
//! |----|------|
//! | 1 trim with 3D curves | `surface nloops counts(nloops) curves` |
//! | 2 project parallel | `object curve direction(3) reference(3) trim_type again [..]` |
//! | 3 planar faces | `curves` |
//! | 4 project and trim | `box curve direction(3) reference(3)` |
//!
//! A `surface` is `kind args` ([`InSurface`]): `0 rows columns n pts(3n)`
//! (`SmApiCreateSurface`) or `1 corners(4x3)` (`SmApiCreateSurfaceFromCornerPoints`). A
//! `box` is `origin(3) length width height fillet [radius]` (`SmApiCreateBox`, then with
//! `fillet` = 1 `SmApiCircularFillet(radius)`); an `object` is `0 surface` or `1 box`.
//! `curves` is `count` then each curve, `kind args` ([`InCurve`]): `0 n pts(3n)`
//! (`SmApiCreateCurve`), `1 center(3) radius` (`SmApiCreateCircle`, B-spline), `2
//! center(3) radius start_deg end_deg` (`SmApiCreateArc`), `3 start(3) end(3)`
//! (`SmApiCreateLineSegment`), `4 center(3) radius` (`SmApiCreateCircle`, `SmCircle`), `5
//! center(3) length width` (`SmApiCreateRectangle`, its four segments). The projected
//! `curve` is one of kinds 0 to 4 (an `SmBSplineCurve` class). `trim_type` is `0` keep
//! point, `1` delete point, `2` split.
//!
//! Output: `-2` when an input's constructor fails; else the API status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`), then on success `has` (the B-rep is not `NULL`)
//! and the B-rep's `faces edges vertices` counts and
//! `harness/cpp/shim/support/primitive_record.h`'s record. With `again` = 1 a project
//! parallel call carries a second `curve direction(3) reference(3) trim_type` and records
//! the first call's status, then on success the record of the second call, which trims
//! the first call's result.
//!
//! [`run`] refuses a trim-with-3D-curves surface with a coordinate above
//! [`MAX_TRIM_SURFACE_COORD`] (HANG-29: building the trimmed face's UV trim curves never
//! returns upstream; nightly fuzz00254).

use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_primitive_api_trim_ops_run(
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
/// Most curve specs or loops of one call.
pub const MAX_ITEMS: usize = 64;
/// Largest coordinate magnitude of the surface `SmApiTrimSurfaceWith3dCurves` trims.
///
/// HANG-29: `SmTrimmingTools::CreateUVTrimCurves` -> `SmEdgeuse::CreateUVTrimCurve` ->
/// `SmSurfaceDropCurve::FindBoundaryStartPoints` -> `SmCurve::GlobalCoincidenceChecker`
/// splits the face boundary forever once squared distances overflow. On the nightly
/// fuzz00254 net (4 x 4, one control point lifted to `z = -10`), a control point of
/// `y = 1e154` returns in 6 s and `1e155`, `1e200`, `1e300` give no result in 15 s; the
/// same `1e300` on the flat net returns at once, and `1e150` takes 0.1 s. The bound is
/// that of the other HANG-29 guards (`topology_edgeuse_p2::MAX_BLOCK_MAGNITUDE`).
pub const MAX_TRIM_SURFACE_COORD: f64 = 1.0e150;

/// Refuses a trimmed surface with a coordinate above [`MAX_TRIM_SURFACE_COORD`].
fn check_trim_surface(s: &InSurface) -> Result<(), FfiError> {
    let huge = |p: &[f64; 3]| p.iter().any(|v| v.abs() > MAX_TRIM_SURFACE_COORD);
    let any_huge = match s {
        InSurface::Net { points, .. } => points.iter().any(huge),
        InSurface::Corners { corners } => corners.iter().any(huge),
    };
    if any_huge {
        return Err(bad("trimmed surface coordinate above 1e150 (HANG-29)"));
    }
    Ok(())
}

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
}

/// One input curve spec, made by an `SM_API` constructor.
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
    /// `SmApiCreateRectangle(center, length, width)`: four segments.
    Rectangle {
        /// Center.
        center: [f64; 3],
        /// Length along x.
        length: f64,
        /// Width along y.
        width: f64,
    },
}

/// The box `SmApiCreateBox(origin, length, width, height)`, optionally filleted.
#[derive(Debug, Clone, PartialEq)]
pub struct InBox {
    /// Lower left corner.
    pub origin: [f64; 3],
    /// Size along x.
    pub length: f64,
    /// Size along y.
    pub width: f64,
    /// Size along z.
    pub height: f64,
    /// `SmApiCircularFillet(box, radius)` after the box is made.
    pub fillet: Option<f64>,
}

/// The object `SmApiTrimProjectParallel` trims.
#[derive(Debug, Clone, PartialEq)]
pub enum InObject {
    /// A standalone surface.
    Surface(InSurface),
    /// A box B-rep.
    Box(InBox),
}

/// A second `SmApiTrimProjectParallel` on the first one's result.
#[derive(Debug, Clone, PartialEq)]
pub struct Reproject {
    /// The projected curve (any kind but a rectangle).
    pub curve: InCurve,
    /// Projection direction.
    pub direction: [f64; 3],
    /// Reference point.
    pub reference: [f64; 3],
    /// `0` keep point, `1` delete point, `2` split.
    pub trim_type: u8,
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiTrimSurfaceWith3dCurves(surface, loops, curves, rpResult)`.
    Trim3d {
        /// The surface.
        surface: InSurface,
        /// Curves per loop (each at most 4096).
        loops: Vec<u64>,
        /// The curve specs, in order (a rectangle adds four curves).
        curves: Vec<InCurve>,
    },
    /// `SmApiTrimProjectParallel(object, curve, direction, reference, trim_type, rpResult)`.
    ProjectParallel {
        /// The object.
        object: InObject,
        /// The projected curve (any kind but a rectangle).
        curve: InCurve,
        /// Projection direction.
        direction: [f64; 3],
        /// Reference point.
        reference: [f64; 3],
        /// `0` keep point, `1` delete point, `2` split.
        trim_type: u8,
        /// A second call on the result.
        again: Option<Reproject>,
    },
    /// `SmApiCreatePlanarFaces(curves, rpResult)`.
    PlanarFaces {
        /// The curve specs (a rectangle adds four curves).
        curves: Vec<InCurve>,
    },
    /// `SmApiProjectAndTrim(box, curve, direction, reference)`.
    ProjectAndTrim {
        /// The box trimmed in place.
        brep: InBox,
        /// The projected curve (any kind but a rectangle).
        curve: InCurve,
        /// Projection direction.
        direction: [f64; 3],
        /// Reference point.
        reference: [f64; 3],
    },
}

fn put_surface(o: &mut Vec<f64>, s: &InSurface) -> Result<(), FfiError> {
    match s {
        InSurface::Net {
            rows,
            columns,
            points,
        } => {
            if *rows > MAX_COUNT || *columns > MAX_COUNT || points.len() > MAX_POINTS {
                return Err(bad("net too large"));
            }
            // a product that wraps would make upstream build knot vectors of the wrapped sizes
            if rows.checked_mul(*columns).is_none() {
                return Err(bad("rows times columns overflows"));
            }
            o.extend([0.0, *rows as f64, *columns as f64, points.len() as f64]);
            o.extend(points.iter().flatten());
        }
        InSurface::Corners { corners } => {
            o.push(1.0);
            o.extend(corners.iter().flatten());
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
        InCurve::AnalyticCircle { center, radius } => {
            o.push(4.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        InCurve::Rectangle {
            center,
            length,
            width,
        } => {
            o.push(5.0);
            o.extend_from_slice(center);
            o.extend([*length, *width]);
        }
    }
    Ok(())
}

fn put_curves(o: &mut Vec<f64>, v: &[InCurve]) -> Result<(), FfiError> {
    if v.len() > MAX_ITEMS {
        return Err(bad("too many curves"));
    }
    o.push(v.len() as f64);
    for c in v {
        put_curve(o, c)?;
    }
    Ok(())
}

/// The projected curve: one `SmBSplineCurve*` (not the four of a rectangle).
fn put_projection_curve(o: &mut Vec<f64>, c: &InCurve) -> Result<(), FfiError> {
    if matches!(c, InCurve::Rectangle { .. }) {
        return Err(bad("the projected curve is one curve, not a rectangle"));
    }
    put_curve(o, c)
}

fn put_box(o: &mut Vec<f64>, b: &InBox) {
    o.extend_from_slice(&b.origin);
    o.extend([b.length, b.width, b.height]);
    match b.fillet {
        None => o.push(0.0),
        Some(r) => o.extend([1.0, r]),
    }
}

fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Trim3d {
            surface,
            loops,
            curves,
        } => {
            if loops.len() > MAX_ITEMS || loops.iter().any(|&n| n > MAX_POINTS as u64) {
                return Err(bad("too many loops"));
            }
            // loop counts the shim does not support are refused
            let made: u64 = curves
                .iter()
                .map(|c| match c {
                    InCurve::Rectangle { .. } => 4,
                    _ => 1,
                })
                .sum();
            if loops.iter().sum::<u64>() > made {
                return Err(bad("unsupported loop-count input"));
            }
            check_trim_surface(surface)?;
            o.push(1.0);
            put_surface(&mut o, surface)?;
            o.push(loops.len() as f64);
            o.extend(loops.iter().map(|&n| n as f64));
            put_curves(&mut o, curves)?;
        }
        Call::ProjectParallel {
            object,
            curve,
            direction,
            reference,
            trim_type,
            again,
        } => {
            if *trim_type > 2 || again.as_ref().is_some_and(|a| a.trim_type > 2) {
                return Err(bad("trim type"));
            }
            o.push(2.0);
            match object {
                InObject::Surface(s) => {
                    o.push(0.0);
                    put_surface(&mut o, s)?;
                }
                InObject::Box(b) => {
                    o.push(1.0);
                    put_box(&mut o, b);
                }
            }
            put_projection_curve(&mut o, curve)?;
            o.extend_from_slice(direction);
            o.extend_from_slice(reference);
            o.push(f64::from(*trim_type));
            match again {
                None => o.push(0.0),
                Some(a) => {
                    o.push(1.0);
                    put_projection_curve(&mut o, &a.curve)?;
                    o.extend_from_slice(&a.direction);
                    o.extend_from_slice(&a.reference);
                    o.push(f64::from(a.trim_type));
                }
            }
        }
        Call::PlanarFaces { curves } => {
            o.push(3.0);
            put_curves(&mut o, curves)?;
        }
        Call::ProjectAndTrim {
            brep,
            curve,
            direction,
            reference,
        } => {
            o.push(4.0);
            put_box(&mut o, brep);
            put_projection_curve(&mut o, curve)?;
            o.extend_from_slice(direction);
            o.extend_from_slice(reference);
        }
    }
    Ok(o)
}

/// The input checks of [`run`] alone, without a native call (the difftest runner's
/// Rust side refuses what the C++ side refuses).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`run`] refuses.
pub fn validate(call: &Call) -> Result<(), FfiError> {
    encode(call).map(|_| ())
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
    call_with_buffer(4096, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_primitive_api_trim_ops_run(
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
    fn ffi_refuses_overrun() {
        let surface = InSurface::Corners {
            corners: [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.0]],
        };
        let rectangle = InCurve::Rectangle {
            center: [0.5, 0.5, 0.0],
            length: 0.5,
            width: 0.5,
        };
        let call = |loops: Vec<u64>| Call::Trim3d {
            surface: surface.clone(),
            loops,
            curves: vec![rectangle.clone()],
        };
        assert!(encode(&call(vec![4])).is_ok());
        assert!(encode(&call(vec![3])).is_ok());
        assert!(encode(&call(vec![4, 1])).is_err());
    }

    /// HANG-29 (nightly fuzz00254): a trimmed surface coordinate above 1e150 is
    /// refused; the same net at the bound, and a huge trimming curve, are not.
    #[test]
    fn huge_trimmed_surface_is_refused() {
        let net = |y: f64| InSurface::Net {
            rows: 2,
            columns: 2,
            points: vec![[0.0, 0.0, 0.0], [0.0, 1.0, -10.0], [1.0, y, 0.0], [1.0, 1.0, 0.0]],
        };
        let circle = |radius: f64| InCurve::Circle {
            center: [0.5, 0.5, 0.0],
            radius,
        };
        let call = |surface: InSurface, radius: f64| Call::Trim3d {
            surface,
            loops: vec![1],
            curves: vec![circle(radius)],
        };
        for y in [1e155, -1e300, f64::MAX] {
            let e = encode(&call(net(y), 0.25)).unwrap_err();
            assert_eq!(
                e.to_string(),
                "invalid argument: trimmed surface coordinate above 1e150 (HANG-29)"
            );
        }
        let corners = InSurface::Corners {
            corners: [[0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [1.0, 1.0, -1e300]],
        };
        assert!(encode(&call(corners, 0.25)).is_err());
        assert!(encode(&call(net(1e150), 0.25)).is_ok());
        assert!(encode(&call(net(0.0), 1e300)).is_ok());
    }

    #[test]
    fn a_rectangle_is_not_a_projection_curve() {
        let call = Call::ProjectAndTrim {
            brep: InBox {
                origin: [0.0; 3],
                length: 1.0,
                width: 1.0,
                height: 1.0,
                fillet: None,
            },
            curve: InCurve::Rectangle {
                center: [0.0; 3],
                length: 1.0,
                width: 1.0,
            },
            direction: [0.0, 0.0, 1.0],
            reference: [0.0; 3],
        };
        assert!(encode(&call).is_err());
    }
}
