//! FFI wrappers for unit `sweep.api_brep`.
//!
//! Shim: `harness/cpp/shim/sweep_api_brep.cpp` (`sc_sweep_api_brep_run`).
//! Upstream: `source/SM_API/src/SmApiBrep.cpp` (`SmApiCreateLinearSweep`,
//! `SmApiCreateRotationalSweep`, `SmApiCreateDraftSweep`, `SmApiCreatePipeSweep`,
//! `SmApiCreateSweepAlongPlanarPath`, `SmApiCreateCurveSweep`,
//! `SmApiCreateCurveSweepFromFaces`, `SmApiCreateCurveSweepFromEdges`,
//! `SmApiCreateTaperExtrude`, `SmApiCreateLinearSweepWithRepetitions`,
//! `SmApiCreateRotationalSweepWithRepetitions`).
//!
//! A [`Call`] is encoded as `op args...`. A curve block is `kind` then:
//!
//! | [`CurveSpec`]      | `kind` | data                   | upstream construction                          |
//! |--------------------|--------|------------------------|------------------------------------------------|
//! | `Null`             | 0      |                        | `NULL`                                         |
//! | `ApiCurve`         | 2      | `np`, `np` x `x y z`   | `SmApiCreateCurve`                             |
//! | `ApiArc`           | 3      | `C[3] r a0 a1`         | `SmApiCreateArc`                               |
//! | `ApiCircle`        | 4      | `C[3] r`               | `SmApiCreateCircle` (`SmBSplineCurve*&`)       |
//! | `LineSegment`      | 5      | `S[3] E[3]`            | `SmApiCreateLineSegment` (an `SmLine`)         |
//! | `AnalyticCircle`   | 6      | `C[3] r`               | `SmApiCreateCircle` (`SmCircle*&`)             |
//! | `Hermite`          | 7      | `P1[3] V1[3] P2[3] V2[3]` | `SmHermiteCurve(P1, V1, P2, V2)` (3D)       |
//!
//! A curve list is `k`, then `k` curve blocks. The sweep arguments of the curve sweeps
//! are `path scale_reference scale has_options cap translational move_center
//! orient_perpendicular move_to_profile` (three curve blocks and six flags; without
//! `has_options` upstream gets `NULL` options).
//!
//! | [`Call`]           | op | args                                          |
//! |--------------------|----|-----------------------------------------------|
//! | `Linear`           | 1  | curves `D[3] dist cap`                        |
//! | `Rotational`       | 2  | curves `B[3] A[3] angle cap`                  |
//! | `Draft`            | 3  | curves `height angle corner caps`             |
//! | `Pipe`             | 4  | `radius` path `cap`                           |
//! | `PlanarPath`       | 5  | profile path `move caps`                      |
//! | `CurveSweep`       | 6  | profile sweep-args                            |
//! | `FromFaces`        | 7  | `nf`, `nf` x `C[3] r` planar circles, sweep-args |
//! | `FromEdges`        | 8  | as `FromFaces` (the edges of those faces)     |
//! | `Taper`            | 9  | curves `height angle fillet caps`             |
//! | `LinearReps`       | 10 | curves `D[3] dist reps cap`                   |
//! | `RotationalReps`   | 11 | curves `B[3] A[3] angle reps cap`             |
//!
//! The output is the API status (`0` = `SM_SUCCESS`, else the `SmStatus`), then on
//! success `has` and, when there is a B-rep, its face, edge and vertex counts; the
//! curve sweeps then add the start, side and end face counts.
//!
//! [`run`] accepts only what upstream handles without undefined behaviour or a hang:
//! finite data; `SmApiCreateCurve` with 4 to 512 points and positive arc / circle
//! radii (a failing constructor is `SC_ERR_ARG`); at most 64 curves per list; a
//! B-spline kind (not `Hermite`) where upstream takes `SmBSplineCurve*`; no `Null`
//! curve in the lists of `PlanarPath`, which upstream dereferences unchecked; at most 16
//! positive-radius planar circles; a cap selector in `-16..=16` and at most `2^20`
//! repetitions (shim bounds). Five kernel defects (`plan/upstream-issues.md`) are
//! fenced off (each only where the kernel is reached: a list holding a `Null` or
//! `Hermite` curve fails in `SmApiCopyCurvesForConsumingSweep` before it):
//! - `Draft` with no curves (unlike `Taper`, the API does not reject it) crashes in
//!   `SmPrimitiveCreation::CreateTaperExtrude`;
//! - `Draft` / `Taper` crash on a profile curve smaller than about `2 * SM_ZONE_TOL_3D`
//!   (a line of length `1.2e-5`, a circle of radius `1.7e-6`): curves whose extent
//!   (line length, arc / circle diameter, the bounding-box diagonal of the
//!   `SmApiCreateCurve` points) is below [`MIN_TAPER_EXTENT`] are rejected;
//! - `Linear` / `LinearReps` with a negative distance and a full circle (B-spline or
//!   analytic circle, or an arc spanning 360 degrees or more) never return (not
//!   rejected for a capped `LinearReps` with other than one repetition, which
//!   `CreateLinearSweep` refuses with `SM_ERR` first);
//! - `PlanarPath` with a `Hermite` curve crashes: `CreateSweepAlongPlanarPath` uses the
//!   `NULL` result of the profile `Copy` and casts the path curves to `SmBSplineCurve*`
//!   unchecked;
//! - `CurveSweep` whose profile holds no B-spline curve (only `Null` / `Hermite`
//!   entries, which `CreateCurveSweep` skips) crashes on `sNurbs[0]` unless the options
//!   move the profile center to the path.

use crate::{call_with_buffer, FfiError};

/// Most curves in one curve list.
pub const MAX_CURVES: usize = 64;
/// Most points of an `SmApiCreateCurve` curve.
pub const MAX_POINTS: usize = 512;
/// Most planar circle faces of `FromFaces` / `FromEdges`.
pub const MAX_DISCS: usize = 16;
/// Most repetitions of `LinearReps` / `RotationalReps`.
pub const MAX_REPETITIONS: u64 = 1 << 20;
/// Smallest profile curve extent accepted by `Draft` / `Taper` (`2 * SM_ZONE_TOL_3D`).
pub const MIN_TAPER_EXTENT: f64 = 2.0e-5;

mod sys {
    extern "C" {
        pub fn sc_sweep_api_brep_run(
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
    /// `SmHermiteCurve(p1, v1, p2, v2)`: a 3D Hermite cubic (no `Copy`).
    Hermite {
        /// Start point.
        p1: [f64; 3],
        /// Start tangent.
        v1: [f64; 3],
        /// End point.
        p2: [f64; 3],
        /// End tangent.
        v2: [f64; 3],
    },
}

/// The trailing arguments of the curve sweeps.
#[derive(Debug, Clone, PartialEq)]
pub struct SweepArgs {
    /// `pPathCurve`.
    pub path: CurveSpec,
    /// `pScaleReference`.
    pub scale_reference: CurveSpec,
    /// `pScaleCurve`.
    pub scale_curve: CurveSpec,
    /// `pOptions` flags (`cap`, `translational`, `move_center`,
    /// `orient_perpendicular`, `move_to_profile`); `None` passes `NULL`.
    pub options: Option<[bool; 5]>,
}

/// One API call.
#[derive(Debug, Clone, PartialEq)]
pub enum Call {
    /// `SmApiCreateLinearSweep(curves, dir, dist, cap)`.
    Linear {
        /// Curves to sweep.
        curves: Vec<CurveSpec>,
        /// Sweep direction.
        dir: [f64; 3],
        /// Sweep distance.
        dist: f64,
        /// Cap the ends.
        cap: bool,
    },
    /// `SmApiCreateRotationalSweep(curves, base, axis, angle, cap)`.
    Rotational {
        /// Curves to sweep.
        curves: Vec<CurveSpec>,
        /// Axis base point.
        base: [f64; 3],
        /// Axis direction.
        axis: [f64; 3],
        /// Angle (degrees).
        angle_deg: f64,
        /// Cap the ends.
        cap: bool,
    },
    /// `SmApiCreateDraftSweep(curves, height, angle, corner, caps)`.
    Draft {
        /// Curves to sweep.
        curves: Vec<CurveSpec>,
        /// Height.
        height: f64,
        /// Draft angle (degrees).
        angle_deg: f64,
        /// `bCornerType` (fillet corners).
        corner: bool,
        /// `iCapEnds`.
        caps: i32,
    },
    /// `SmApiCreatePipeSweep(radius, path, cap)`.
    Pipe {
        /// Pipe radius.
        radius: f64,
        /// Path curve (may be `Null`).
        path: CurveSpec,
        /// Cap the ends.
        cap: bool,
    },
    /// `SmApiCreateSweepAlongPlanarPath(profile, path, move, caps)`.
    PlanarPath {
        /// Profile curves.
        profile: Vec<CurveSpec>,
        /// Path curves.
        path: Vec<CurveSpec>,
        /// `bMoveProfile`.
        move_profile: bool,
        /// `iCapEnds`.
        caps: i32,
    },
    /// `SmApiCreateCurveSweep(profile, path, scale_reference, scale, options, ...)`.
    CurveSweep {
        /// Profile curves (may hold `Null`).
        profile: Vec<CurveSpec>,
        /// Path, scale curves and options.
        args: SweepArgs,
    },
    /// `SmApiCreateCurveSweepFromFaces` on the faces of `SmApiCreatePlanarCircle` breps.
    FromFaces {
        /// Planar circles `(center, radius)`.
        circles: Vec<([f64; 3], f64)>,
        /// Path, scale curves and options.
        args: SweepArgs,
    },
    /// `SmApiCreateCurveSweepFromEdges` on the edges of those faces.
    FromEdges {
        /// Planar circles `(center, radius)`.
        circles: Vec<([f64; 3], f64)>,
        /// Path, scale curves and options.
        args: SweepArgs,
    },
    /// `SmApiCreateTaperExtrude(curves, height, angle, caps, fillet)`.
    Taper {
        /// Curves to sweep.
        curves: Vec<CurveSpec>,
        /// Height.
        height: f64,
        /// Draft angle (degrees).
        angle_deg: f64,
        /// `bFilletCorner`.
        fillet: bool,
        /// `iEndCaps`.
        caps: i32,
    },
    /// `SmApiCreateLinearSweepWithRepetitions(curves, dir, dist, reps, cap)`.
    LinearReps {
        /// Curves to sweep.
        curves: Vec<CurveSpec>,
        /// Sweep direction.
        dir: [f64; 3],
        /// Sweep distance.
        dist: f64,
        /// Repetitions.
        reps: u64,
        /// Cap the ends.
        cap: bool,
    },
    /// `SmApiCreateRotationalSweepWithRepetitions(curves, base, axis, angle, reps, cap)`.
    RotationalReps {
        /// Curves to sweep.
        curves: Vec<CurveSpec>,
        /// Axis base point.
        base: [f64; 3],
        /// Axis direction.
        axis: [f64; 3],
        /// Angle (degrees).
        angle_deg: f64,
        /// Repetitions.
        reps: u64,
        /// Cap the ends.
        cap: bool,
    },
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn check_curve(c: &CurveSpec, bspline: bool, nullable: bool) -> Result<(), FfiError> {
    match c {
        CurveSpec::Null => {
            if !nullable {
                return Err(bad("NULL curve where upstream dereferences it"));
            }
        }
        CurveSpec::ApiCurve { points } => {
            if points.len() < 4 || points.len() > MAX_POINTS {
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
        }
        CurveSpec::LineSegment { start, end } => {
            finite(start, "line")?;
            finite(end, "line")?;
        }
        CurveSpec::Hermite { p1, v1, p2, v2 } => {
            for v in [p1, v1, p2, v2] {
                finite(v, "hermite")?;
            }
            if bspline {
                return Err(bad("hermite curve where upstream takes an SmBSplineCurve"));
            }
        }
    }
    Ok(())
}

fn push_curve(o: &mut Vec<f64>, c: &CurveSpec) {
    match c {
        CurveSpec::Null => o.push(0.0),
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
        CurveSpec::Hermite { p1, v1, p2, v2 } => {
            o.push(7.0);
            for v in [p1, v1, p2, v2] {
                o.extend_from_slice(v);
            }
        }
    }
}

/// Checks and encodes a curve list (`nullable`: `Null` entries allowed).
fn push_curves(
    o: &mut Vec<f64>,
    curves: &[CurveSpec],
    bspline: bool,
    nullable: bool,
    non_empty: bool,
) -> Result<(), FfiError> {
    if curves.len() > MAX_CURVES {
        return Err(bad("more than 64 curves"));
    }
    if non_empty && curves.is_empty() {
        return Err(bad("empty curve list (upstream crash)"));
    }
    for c in curves {
        check_curve(c, bspline, nullable)?;
    }
    o.push(curves.len() as f64);
    for c in curves {
        push_curve(o, c);
    }
    Ok(())
}

fn push_sweep_args(o: &mut Vec<f64>, a: &SweepArgs) -> Result<(), FfiError> {
    for c in [&a.path, &a.scale_reference, &a.scale_curve] {
        check_curve(c, true, true)?;
    }
    for c in [&a.path, &a.scale_reference, &a.scale_curve] {
        push_curve(o, c);
    }
    o.push(flag(a.options.is_some()));
    o.extend(a.options.unwrap_or_default().map(flag));
    Ok(())
}

fn push_discs(o: &mut Vec<f64>, circles: &[([f64; 3], f64)]) -> Result<(), FfiError> {
    if circles.len() > MAX_DISCS {
        return Err(bad("more than 16 planar circles"));
    }
    o.push(circles.len() as f64);
    for (center, radius) in circles {
        finite(center, "planar circle")?;
        finite(&[*radius], "planar circle")?;
        if *radius <= 0.0 {
            return Err(bad("planar circle: radius must be positive"));
        }
        o.extend_from_slice(center);
        o.push(*radius);
    }
    Ok(())
}

fn dist3(a: &[f64; 3], b: &[f64; 3]) -> f64 {
    let d = [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt()
}

/// The size of a curve's definition (see the module docs); `None` for `Null` and
/// `Hermite` (which fail before the kernel).
fn extent(c: &CurveSpec) -> Option<f64> {
    match c {
        CurveSpec::Null | CurveSpec::Hermite { .. } => None,
        CurveSpec::ApiCurve { points } => {
            let mut lo = [f64::INFINITY; 3];
            let mut hi = [f64::NEG_INFINITY; 3];
            for p in points {
                for ((l, h), &x) in lo.iter_mut().zip(hi.iter_mut()).zip(p) {
                    *l = l.min(x);
                    *h = h.max(x);
                }
            }
            Some(dist3(&lo, &hi))
        }
        CurveSpec::ApiArc { radius, .. }
        | CurveSpec::ApiCircle { radius, .. }
        | CurveSpec::AnalyticCircle { radius, .. } => Some(2.0 * radius),
        CurveSpec::LineSegment { start, end } => Some(dist3(start, end)),
    }
}

/// Whether `SmApiCopyCurvesForConsumingSweep` copies every curve, so the call reaches
/// the kernel (a `Null` curve or one without `Copy` fails first).
fn copies_ok(curves: &[CurveSpec]) -> bool {
    !curves
        .iter()
        .any(|c| matches!(c, CurveSpec::Null | CurveSpec::Hermite { .. }))
}

/// Whether a curve reaches `CreateCurveSweep` as a profile curve (an
/// `SmBSplineCurve`; `NULL` and the Hermite curve are skipped).
fn is_bspline(c: &CurveSpec) -> bool {
    !matches!(c, CurveSpec::Null | CurveSpec::Hermite { .. })
}

/// Rejects a curve-sweep profile without a B-spline curve, on which `CreateCurveSweep`
/// reads `sNurbs[0]` of an empty list, unless the profile center is moved to the path.
fn check_curve_sweep_profile(profile: &[CurveSpec], a: &SweepArgs) -> Result<(), FfiError> {
    let move_center = a.options.is_some_and(|f| f[2]);
    let reaches_kernel = !profile.is_empty() && !matches!(a.path, CurveSpec::Null);
    if reaches_kernel && !move_center && !profile.iter().any(is_bspline) {
        return Err(bad(
            "curve sweep: profile without a B-spline curve (upstream crash)",
        ));
    }
    Ok(())
}

/// Rejects the tiny profile curves `CreateTaperExtrude` crashes on.
fn check_taper_curves(curves: &[CurveSpec]) -> Result<(), FfiError> {
    if copies_ok(curves)
        && curves
            .iter()
            .filter_map(extent)
            .any(|e| e < MIN_TAPER_EXTENT)
    {
        return Err(bad(
            "draft / taper: profile curve below 2e-5 (upstream crash)",
        ));
    }
    Ok(())
}

/// Rejects the negative-distance sweeps of a full circle that never return.
/// `refused`: `CreateLinearSweep` refuses the call before sweeping (capped with
/// other than one repetition).
fn check_linear(curves: &[CurveSpec], dist: f64, refused: bool) -> Result<(), FfiError> {
    let full_circle = |c: &CurveSpec| match c {
        CurveSpec::ApiCircle { .. } | CurveSpec::AnalyticCircle { .. } => true,
        CurveSpec::ApiArc {
            start_deg, end_deg, ..
        } => (end_deg - start_deg).abs() >= 360.0,
        _ => false,
    };
    if dist < 0.0 && !refused && copies_ok(curves) && curves.iter().any(full_circle) {
        return Err(bad(
            "linear sweep: negative distance with a full circle (upstream hang)",
        ));
    }
    Ok(())
}

fn caps_of(caps: i32) -> Result<f64, FfiError> {
    if (-16..=16).contains(&caps) {
        Ok(f64::from(caps))
    } else {
        Err(bad("cap selector outside -16..=16"))
    }
}

fn reps_of(reps: u64) -> Result<f64, FfiError> {
    if reps <= MAX_REPETITIONS {
        Ok(reps as f64)
    } else {
        Err(bad("more than 2^20 repetitions"))
    }
}

/// Validates `call` and encodes it as the shim's program.
fn encode(call: &Call) -> Result<Vec<f64>, FfiError> {
    let mut o = Vec::new();
    match call {
        Call::Linear {
            curves,
            dir,
            dist,
            cap,
        }
        | Call::LinearReps {
            curves,
            dir,
            dist,
            cap,
            ..
        } => {
            let reps = matches!(call, Call::LinearReps { .. });
            finite(dir, "sweep direction")?;
            finite(&[*dist], "sweep distance")?;
            let refused = match call {
                Call::LinearReps { reps, cap, .. } => *cap && *reps != 1,
                _ => false,
            };
            check_linear(curves, *dist, refused)?;
            o.push(if reps { 10.0 } else { 1.0 });
            push_curves(&mut o, curves, false, true, false)?;
            o.extend_from_slice(dir);
            o.push(*dist);
            if let Call::LinearReps { reps, .. } = call {
                o.push(reps_of(*reps)?);
            }
            o.push(flag(*cap));
        }
        Call::Rotational {
            curves,
            base,
            axis,
            angle_deg,
            cap,
        }
        | Call::RotationalReps {
            curves,
            base,
            axis,
            angle_deg,
            cap,
            ..
        } => {
            let reps = matches!(call, Call::RotationalReps { .. });
            finite(base, "axis base")?;
            finite(axis, "axis")?;
            finite(&[*angle_deg], "angle")?;
            o.push(if reps { 11.0 } else { 2.0 });
            push_curves(&mut o, curves, false, true, false)?;
            o.extend_from_slice(base);
            o.extend_from_slice(axis);
            o.push(*angle_deg);
            if let Call::RotationalReps { reps, .. } = call {
                o.push(reps_of(*reps)?);
            }
            o.push(flag(*cap));
        }
        Call::Draft {
            curves,
            height,
            angle_deg,
            corner,
            caps,
        }
        | Call::Taper {
            curves,
            height,
            angle_deg,
            fillet: corner,
            caps,
        } => {
            let draft = matches!(call, Call::Draft { .. });
            finite(&[*height, *angle_deg], "height / angle")?;
            check_taper_curves(curves)?;
            o.push(if draft { 3.0 } else { 9.0 });
            push_curves(&mut o, curves, false, true, draft)?;
            o.extend([*height, *angle_deg, flag(*corner), caps_of(*caps)?]);
        }
        Call::Pipe { radius, path, cap } => {
            finite(&[*radius], "pipe radius")?;
            check_curve(path, true, true)?;
            o.extend([4.0, *radius]);
            push_curve(&mut o, path);
            o.push(flag(*cap));
        }
        Call::PlanarPath {
            profile,
            path,
            move_profile,
            caps,
        } => {
            o.push(5.0);
            push_curves(&mut o, profile, true, false, false)?;
            push_curves(&mut o, path, true, false, false)?;
            o.extend([flag(*move_profile), caps_of(*caps)?]);
        }
        Call::CurveSweep { profile, args } => {
            check_curve_sweep_profile(profile, args)?;
            o.push(6.0);
            push_curves(&mut o, profile, false, true, false)?;
            push_sweep_args(&mut o, args)?;
        }
        Call::FromFaces { circles, args } | Call::FromEdges { circles, args } => {
            o.push(if matches!(call, Call::FromFaces { .. }) {
                7.0
            } else {
                8.0
            });
            push_discs(&mut o, circles)?;
            push_sweep_args(&mut o, args)?;
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
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `prog` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len`
        // is a valid i64; nothing is retained.
        unsafe {
            sys::sc_sweep_api_brep_run(
                prog.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
