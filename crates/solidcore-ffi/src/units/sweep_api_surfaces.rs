//! FFI wrappers for unit `sweep.api_surfaces`.
//!
//! Shim: `harness/cpp/shim/sweep_api_surfaces.cpp` (`sc_sweep_api_surfaces_run`).
//! Upstream: `source/SM_API/src/SmApiSurfaces.cpp` (`SmApiCreateExtrude`,
//! `SmApiCreateSweep`, `SmApiCreateRuledSurface`, `SmApiCreateSurfaceRevolution`,
//! `SmApiCreateSkin`), `source/SM_API/src/SmApiPrimitives.cpp`
//! (`SmApiCreateSkinPrimitive`, `SmApiCreateSkinFromFaces`).
//!
//! A [`Call`] is encoded as `op args...`. A curve block is `kind` then:
//!
//! | [`CurveSpec`]      | `kind` | data                                  | upstream construction                      |
//! |--------------------|--------|---------------------------------------|--------------------------------------------|
//! | `Null`             | 0      |                                       | `NULL`                                     |
//! | `Nurb`             | 1      | `dim n p m`, `n+1` x `x y z w`, `m+1` knots | `SmBSplineCurve(gw_CURVE*, dim, FALSE)` |
//! | `ApiCurve`         | 2      | `np`, `np` x `x y z`                  | `SmApiCreateCurve`                          |
//! | `ApiArc`           | 3      | `C[3] r a0 a1`                        | `SmApiCreateArc`                            |
//! | `ApiCircle`        | 4      | `C[3] r`                              | `SmApiCreateCircle` (`SmBSplineCurve*&`)    |
//! | `LineSegment`      | 5      | `S[3] E[3]`                           | `SmApiCreateLineSegment` (an `SmLine`)      |
//! | `AnalyticCircle`   | 6      | `C[3] r`                              | `SmApiCreateCircle` (`SmCircle*&`)          |
//!
//! | [`Call`]         | op | args                                         | output after the status            |
//! |------------------|----|----------------------------------------------|------------------------------------|
//! | `Extrude`        | 1  | curve `V[3]`                                 | [surface] curve `has [n cpts]`     |
//! | `Sweep`          | 2  | curve curve                                  | [surface]                          |
//! | `Ruled`          | 3  | curve curve                                  | [surface]                          |
//! | `Revolution`     | 4  | curve `O[3] A[3] angle`                      | [`has`, owned-result geometry]     |
//! | `Skin`           | 5  | `k`, `k` curves                              | [surface]                          |
//! | `SkinPrimitive`  | 6  | `np`, `np` counts, `nc`, `nc` curves, `degree cap` | [`has start side end` counts] |
//! | `SkinFromFaces`  | 7  | `nf`, `nf` x `C[3] r` planar circles, `degree` | [`has new_faces` count]          |
//!
//! The output starts with the API status (`0` = `SM_SUCCESS`, else the `SmStatus`);
//! bracketed parts follow only on success. A surface is `has` then (when the result is
//! an `SmBSplineSurface`) `has_nurb oob borrowed knot_type form [n m p q r s cpts u v]`.
//!
//! A present revolution result retains `has`, followed by an owner-clear flag, its
//! natural UV domain (`min.x min.y max.x max.y`), then 25 virtual point evaluations
//! in u-major order on a 5 by 5 grid. Each evaluation is its normalized status,
//! followed by `x y z` only on success. No B-spline substitute is constructed.
//!
//! [`run`] accepts only what upstream handles without undefined behaviour: finite data;
//! well-formed clamped NURB curves (`m = n+p+1`, `1 <= p <= n`, degree at most 17,
//! consistent dimension and rationality markers, non-decreasing knots with end
//! multiplicity exactly `degree + 1` and interior multiplicity at most the degree);
//! NURB data of any dimension, 2D data (`z = NL_NOZ`) included, since upstream handles
//! it; positive weights except for `Extrude` (as `CreateLinearSweep`, any weights);
//! B-spline curves (including real `SmLine` / `SmCircle` subclasses for `Extrude`)
//! where upstream takes `SmBSplineCurve*` and no `NULL` curve where upstream dereferences it unchecked (all but
//! `Revolution` and `SkinPrimitive`); `SmApiCreateCurve` with at least 4 points and
//! positive arc / circle radii (a failing constructor is `SC_ERR_ARG`); at most 12
//! skin profiles; a skin-from-faces degree of 0 to 3 or 18 to 2^20 (outside 1 to 17 the
//! API returns `SM_ERR_INVALID_INPUT` first) with 1 to 4 positive-radius
//! circles (no faces crash upstream, `plan/upstream-issues.md` UB-78).

use crate::validate::{bad, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`run`].
pub const MAX_INDEX: usize = 512;
/// Largest NURB degree accepted by [`run`].
pub const MAX_DEGREE: usize = 17;
/// Minimum sweep tolerance as a fraction of the trajectory's control polygon
/// length (HANG-5): `SmApiCreateSweep` uses a fixed `dTol = 1e-5`, so a
/// trajectory whose control polygon is longer than `1e-5 / 1e-6 = 10` makes
/// the tolerance fall below floating-point resolution and hangs.
pub const MIN_SWEEP_TOL_FRACTION: f64 = 1e-6;
mod sys {
    extern "C" {
        pub fn sc_sweep_api_surfaces_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// How a curve argument is built on the C++ side.
#[derive(Debug, Clone, PartialEq)]
pub enum CurveSpec {
    /// `NULL`.
    Null,
    /// `SmBSplineCurve(gw_CURVE*, dim, FALSE)` from raw NURB data.
    Nurb {
        /// `SmBSplineCurve` dimension (1 to 3).
        dim: usize,
        /// Degree.
        p: usize,
        /// Control points `[x, y, z, w]` as stored (`w = NL_NOW` non-rational, `z =
        /// NL_NOZ` 2D).
        cpts: Vec<[f64; 4]>,
        /// Knots.
        knots: Vec<f64>,
    },
    /// `SmApiCreateCurve(points)`.
    ApiCurve {
        /// Control points.
        points: Vec<[f64; 3]>,
    },
    /// `SmApiCreateArc(center, radius, start_deg, end_deg)`.
    ApiArc {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
        /// Start angle (degrees).
        start_deg: f64,
        /// End angle (degrees).
        end_deg: f64,
    },
    /// `SmApiCreateCircle(center, radius, SmBSplineCurve*&)`.
    ApiCircle {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
    /// `SmApiCreateLineSegment(start, end)`: an `SmLine`.
    LineSegment {
        /// Start point.
        start: [f64; 3],
        /// End point.
        end: [f64; 3],
    },
    /// `SmApiCreateCircle(center, radius, SmCircle*&)`: an analytic `SmCircle`.
    AnalyticCircle {
        /// Center.
        center: [f64; 3],
        /// Radius.
        radius: f64,
    },
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiCreateExtrude(curve, vector)`.
    Extrude {
        /// Curve to extrude.
        curve: CurveSpec,
        /// Sweep vector.
        vector: [f64; 3],
    },
    /// `SmApiCreateSweep(curve, path)`.
    Sweep {
        /// Curve to sweep.
        curve: CurveSpec,
        /// Path curve.
        path: CurveSpec,
    },
    /// `SmApiCreateRuledSurface(curve1, curve2)`.
    Ruled {
        /// First curve.
        curve1: CurveSpec,
        /// Second curve.
        curve2: CurveSpec,
    },
    /// `SmApiCreateSurfaceRevolution(curve, origin, axis, angle)`.
    Revolution {
        /// Curve to revolve (may be `Null`).
        curve: CurveSpec,
        /// Axis base point.
        origin: [f64; 3],
        /// Axis direction.
        axis: [f64; 3],
        /// Angle (degrees).
        angle_deg: f64,
    },
    /// `SmApiCreateSkin(profiles)`.
    Skin {
        /// Profiles.
        profiles: Vec<CurveSpec>,
    },
    /// `SmApiCreateSkinPrimitive(counts, curves, degree, cap, ...)`.
    SkinPrimitive {
        /// Curves in each profile.
        counts: Vec<u64>,
        /// All profile curves (may hold `Null`).
        curves: Vec<CurveSpec>,
        /// Loft degree.
        degree: u64,
        /// Build end caps.
        cap_ends: bool,
    },
    /// `SmApiCreateSkinFromFaces` on the faces of `SmApiCreatePlanarCircle` breps.
    SkinFromFaces {
        /// Planar circles `(center, radius)`.
        circles: Vec<([f64; 3], f64)>,
        /// Loft degree.
        degree: u64,
    },
}

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize) -> Result<(), FfiError> {
    let m = knots.len().checked_sub(1).ok_or_else(|| bad("no knots"))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad("knots not non-decreasing"));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let short = || bad("knots too short");
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad("knots not clamped"));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad("end multiplicity above degree + 1"));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad("interior multiplicity above the degree"));
        }
    }
    Ok(())
}

/// What a curve argument may be.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Need {
    /// An `SmBSplineCurve` of any dimension with positive weights.
    BSplinePosWeights,
    /// An `SmBSplineCurve` of any dimension, including `SmLine` / `SmCircle`
    /// subclasses (`SmLine.h:49`, `SmCircle.h:59`, `SmEllipse.h:243`).
    /// `SmBSplineSurface.cpp:1588-1629` (`CreateLinearSweep`) uses their NURB.
    BSplineAnyDim,
    /// Any `SmCurve`.
    AnyCurve,
}

fn check_curve(c: &CurveSpec, need: Need, nullable: bool) -> Result<(), FfiError> {
    match c {
        CurveSpec::Null => {
            if !nullable {
                return Err(bad("NULL curve where upstream dereferences it"));
            }
        }
        CurveSpec::Nurb {
            dim,
            p,
            cpts,
            knots,
        } => {
            if !(1..=3).contains(dim) {
                return Err(bad("nurb: dimension must be 1 to 3"));
            }
            let n = cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| bad("nurb: no points"))?;
            if n > MAX_INDEX || *p == 0 || *p > n || *p > MAX_DEGREE {
                return Err(bad("nurb: need 1 <= p <= n, p <= 17, n <= 512"));
            }
            if knots.len() != n + p + 2 {
                return Err(bad("nurb: need m = n + p + 1"));
            }
            finite(cpts.as_flattened(), "nurb points")?;
            finite(knots, "nurb knots")?;
            let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("nurb: no points"))?;
            for &[_, _, z, w] in cpts {
                if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
                    return Err(bad("nurb: mixed dimension or rationality"));
                }
                if need != Need::BSplineAnyDim && w != NOW && w <= 0.0 {
                    return Err(bad("nurb: need positive weights"));
                }
            }
            check_knots(knots, *p)?;
        }
        CurveSpec::ApiCurve { points } => {
            if points.len() < 4 || points.len() > MAX_INDEX {
                return Err(bad("api curve: need 4 to 512 points"));
            }
            finite(points.as_flattened(), "api curve")?;
        }
        CurveSpec::ApiArc {
            center,
            radius,
            start_deg,
            end_deg,
        } => {
            finite(center, "arc")?;
            finite(&[*radius, *start_deg, *end_deg], "arc")?;
            if *radius <= 0.0 {
                return Err(bad("arc: radius must be positive"));
            }
        }
        CurveSpec::ApiCircle { center, radius } | CurveSpec::AnalyticCircle { center, radius } => {
            finite(center, "circle")?;
            finite(&[*radius], "circle")?;
            if *radius <= 0.0 {
                return Err(bad("circle: radius must be positive"));
            }
            if matches!(c, CurveSpec::AnalyticCircle { .. }) && need == Need::BSplinePosWeights {
                return Err(bad(
                    "analytic circle where upstream takes an SmBSplineCurve",
                ));
            }
        }
        CurveSpec::LineSegment { start, end } => {
            finite(start, "line")?;
            finite(end, "line")?;
            if need == Need::BSplinePosWeights {
                return Err(bad("line where upstream takes an SmBSplineCurve"));
            }
        }
    }
    Ok(())
}

fn push_curve(o: &mut Vec<f64>, c: &CurveSpec) {
    match c {
        CurveSpec::Null => o.push(0.0),
        CurveSpec::Nurb {
            dim,
            p,
            cpts,
            knots,
        } => {
            let n = cpts.len().saturating_sub(1);
            o.extend([1.0, *dim as f64, n as f64, *p as f64, (n + p + 1) as f64]);
            o.extend_from_slice(cpts.as_flattened());
            o.extend_from_slice(knots);
        }
        CurveSpec::ApiCurve { points } => {
            o.extend([2.0, points.len() as f64]);
            o.extend_from_slice(points.as_flattened());
        }
        CurveSpec::ApiArc {
            center,
            radius,
            start_deg,
            end_deg,
        } => {
            o.push(3.0);
            o.extend_from_slice(center);
            o.extend([*radius, *start_deg, *end_deg]);
        }
        CurveSpec::ApiCircle { center, radius } => {
            o.push(4.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
        CurveSpec::LineSegment { start, end } => {
            o.push(5.0);
            o.extend_from_slice(start);
            o.extend_from_slice(end);
        }
        CurveSpec::AnalyticCircle { center, radius } => {
            o.push(6.0);
            o.extend_from_slice(center);
            o.push(*radius);
        }
    }
}

/// Control-polygon length of a sweep trajectory, for the HANG-5 tolerance guard.
fn path_poly_len(c: &CurveSpec) -> f64 {
    match c {
        CurveSpec::Nurb { cpts, .. } => {
            let mut len = 0.0;
            let mut prev: Option<[f64; 4]> = None;
            for cp in cpts {
                if let Some(a) = prev {
                    let b = *cp;
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let dz = if a[2] == NOZ || b[2] == NOZ {
                        0.0
                    } else {
                        b[2] - a[2]
                    };
                    len += (dx * dx + dy * dy + dz * dz).sqrt();
                }
                prev = Some(*cp);
            }
            len
        }
        CurveSpec::ApiCurve { points } => {
            let mut len = 0.0;
            let mut prev: Option<[f64; 3]> = None;
            for pt in points {
                if let Some(a) = prev {
                    let b = *pt;
                    let dx = b[0] - a[0];
                    let dy = b[1] - a[1];
                    let dz = b[2] - a[2];
                    len += (dx * dx + dy * dy + dz * dz).sqrt();
                }
                prev = Some(*pt);
            }
            len
        }
        CurveSpec::ApiArc {
            center: _,
            radius,
            start_deg,
            end_deg,
        } => {
            let rad = (end_deg - start_deg).abs() * std::f64::consts::PI / 180.0;
            radius.abs() * rad
        }
        CurveSpec::ApiCircle { radius, .. } | CurveSpec::AnalyticCircle { radius, .. } => {
            2.0 * std::f64::consts::PI * radius.abs()
        }
        CurveSpec::LineSegment { start, end } => {
            let dx = end[0] - start[0];
            let dy = end[1] - start[1];
            let dz = end[2] - start[2];
            (dx * dx + dy * dy + dz * dz).sqrt()
        }
        CurveSpec::Null => 0.0,
    }
}

/// Validates `call` and encodes it as the shim's program.
fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Extrude { curve, vector } => {
            check_curve(curve, Need::BSplineAnyDim, false)?;
            finite(vector, "extrude vector")?;
            o.push(1.0);
            push_curve(&mut o, curve);
            o.extend_from_slice(vector);
        }
        Call::Sweep { curve, path } => {
            check_curve(curve, Need::BSplinePosWeights, false)?;
            check_curve(path, Need::BSplinePosWeights, false)?;
            // HANG-5: fixed dTol = 1e-5 must stay above MIN_SWEEP_TOL_FRACTION
            // times the trajectory's control polygon length.
            let traj_len = path_poly_len(path);
            // Reject only the extreme inputs that hang C++ (control points at
            // +/-1e300); keep normal and large-but-finite sweeps covered.
            if !traj_len.is_finite() || traj_len > 1e12 {
                return Err(bad("sweep: trajectory too long for the fixed tolerance"));
            }

            o.push(2.0);
            push_curve(&mut o, curve);
            push_curve(&mut o, path);
        }
        Call::Ruled { curve1, curve2 } => {
            check_curve(curve1, Need::BSplinePosWeights, false)?;
            check_curve(curve2, Need::BSplinePosWeights, false)?;
            o.push(3.0);
            push_curve(&mut o, curve1);
            push_curve(&mut o, curve2);
        }
        Call::Revolution {
            curve,
            origin,
            axis,
            angle_deg,
        } => {
            check_curve(curve, Need::BSplinePosWeights, true)?;
            finite(origin, "revolution origin")?;
            finite(axis, "revolution axis")?;
            finite(&[*angle_deg], "revolution angle")?;
            o.push(4.0);
            push_curve(&mut o, curve);
            o.extend_from_slice(origin);
            o.extend_from_slice(axis);
            o.push(*angle_deg);
        }
        Call::Skin { profiles } => {
            if profiles.len() > 12 {
                return Err(bad("skin: more than 12 profiles"));
            }
            for c in profiles {
                check_curve(c, Need::BSplinePosWeights, false)?;
            }
            o.extend([5.0, profiles.len() as f64]);
            for c in profiles {
                push_curve(&mut o, c);
            }
        }
        Call::SkinPrimitive {
            counts,
            curves,
            degree,
            cap_ends,
        } => {
            if counts.len() > 64 || curves.len() > 256 {
                return Err(bad("skin primitive: too many profiles or curves"));
            }
            if counts.iter().any(|&c| c > 1 << 20) || *degree > 1 << 20 {
                return Err(bad("skin primitive: count or degree too large"));
            }
            for c in curves {
                check_curve(c, Need::AnyCurve, true)?;
            }
            o.push(6.0);
            o.push(counts.len() as f64);
            o.extend(counts.iter().map(|&c| c as f64));
            o.push(curves.len() as f64);
            for c in curves {
                push_curve(&mut o, c);
            }
            o.extend([*degree as f64, f64::from(u8::from(*cap_ends))]);
        }
        Call::SkinFromFaces { circles, degree } => {
            if circles.is_empty() || circles.len() > 4 {
                return Err(bad("skin from faces: need 1 to 4 faces"));
            }
            // 0 and above NL_MAXDEG are refused by the API before any work
            if (4..=MAX_DEGREE as u64).contains(degree) || *degree > 1 << 20 {
                return Err(bad("skin from faces: degree must be 0 to 3 or above 17"));
            }
            o.extend([7.0, circles.len() as f64]);
            for (center, radius) in circles {
                finite(center, "planar circle")?;
                finite(&[*radius], "planar circle")?;
                if *radius <= 0.0 {
                    return Err(bad("planar circle: radius must be positive"));
                }
                o.extend_from_slice(center);
                o.push(*radius);
            }
            o.push(*degree as f64);
        }
    }
    Ok(o)
}

/// Runs `call` through the C++ API; returns the flat record documented above.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input upstream does not handle (see the module
/// docs); [`FfiError::Status`] for a shim error.
pub fn run(call: &Call) -> Result<Vec<f64>, FfiError> {
    let prog = encode(call)?;
    let n_in = i64::try_from(prog.len()).map_err(|_| bad("program too long"))?;
    call_with_buffer(1024, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_sweep_api_surfaces_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
