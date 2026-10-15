//! Runner for unit `sweep.api_brep`: the `SM_API` B-rep sweep entry points.
//!
//! C++: `solidcore_ffi::units::sweep_api_brep` (upstream
//! `source/SM_API/src/SmApiBrep.cpp`). Rust: `solidcore::sweep::api_brep`.
//!
//! A case is one API call (`op` plus its arguments). Curve arguments are built the same
//! way on both sides: the `SM_API` curve constructors (`SmApiCreateCurve`,
//! `SmApiCreateArc`, `SmApiCreateCircle`, `SmApiCreateLineSegment`, ported in
//! `primitive.api_curves`), or a 3D `SmHermiteCurve` (a curve without `Copy`). Faces
//! and edges come from `SmApiCreatePlanarCircle` B-reps on the C++ side; the Rust side
//! cannot build those yet and passes placeholder ids (only their count is read before
//! the unported kernel call). Both sides produce the flat record documented in the FFI
//! module, split into keys: `status` (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001`
//! `SM_ERR`, `1007` `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream
//! cannot produce), then on success `has`, `counts` (faces, edges, vertices) and, for
//! the curve sweeps, `faces` (start, side, end).
//!
//! Every call that reaches `SmPrimitiveCreation` is deferred on the Rust side
//! (`primitive.primitive_creation_p2` / `_p3`); the input validation and curve copies
//! before it (`NULL` curves, curves without `Copy`, empty inputs, a missing path, a
//! non-positive pipe radius, a cap selector out of range) are compared. Every run has at
//! least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::primitive::api_curves;
use solidcore::primitive::primitive_creation::SweepOptions;
use solidcore::sweep::api_brep as api;
use solidcore::topology::{Brep, EdgeId, FaceId};
use solidcore::SolidError;
use solidcore_ffi::units::sweep_api_brep::{self as ffi, Call, CurveSpec, SweepArgs};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `sweep.api_brep` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -------------------------------------------------------------------------

type P3 = [f64; 3];

/// A curve argument.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CrvIn {
    /// `NULL`.
    Null,
    /// `SmApiCreateCurve`.
    Curve { points: Vec<P3> },
    /// `SmApiCreateArc`.
    Arc {
        center: P3,
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
    /// `SmApiCreateCircle` (B-spline).
    Circle { center: P3, radius: f64 },
    /// `SmApiCreateLineSegment`.
    Line { start: P3, end: P3 },
    /// `SmApiCreateCircle` (analytic `SmCircle`).
    AnalyticCircle { center: P3, radius: f64 },
    /// `SmHermiteCurve(p1, v1, p2, v2)`.
    Hermite { p1: P3, v1: P3, p2: P3, v2: P3 },
}

/// The path, scale curves and options of a curve sweep.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SweepIn {
    path: CrvIn,
    #[serde(default = "null")]
    scale_reference: CrvIn,
    #[serde(default = "null")]
    scale_curve: CrvIn,
    /// `[cap, translational, move_center, orient_perpendicular, move_to_profile]`;
    /// absent: `NULL` options.
    #[serde(default)]
    options: Option<[bool; 5]>,
}

fn null() -> CrvIn {
    CrvIn::Null
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Linear {
        curves: Vec<CrvIn>,
        dir: P3,
        dist: f64,
        cap: bool,
    },
    Rotational {
        curves: Vec<CrvIn>,
        base: P3,
        axis: P3,
        angle_deg: f64,
        cap: bool,
    },
    Draft {
        curves: Vec<CrvIn>,
        height: f64,
        angle_deg: f64,
        corner: bool,
        caps: i32,
    },
    Pipe {
        radius: f64,
        path: CrvIn,
        cap: bool,
    },
    PlanarPath {
        profile: Vec<CrvIn>,
        path: Vec<CrvIn>,
        move_profile: bool,
        caps: i32,
    },
    CurveSweep {
        profile: Vec<CrvIn>,
        sweep: SweepIn,
    },
    FromFaces {
        circles: Vec<(P3, f64)>,
        sweep: SweepIn,
    },
    FromEdges {
        circles: Vec<(P3, f64)>,
        sweep: SweepIn,
    },
    Taper {
        curves: Vec<CrvIn>,
        height: f64,
        angle_deg: f64,
        fillet: bool,
        caps: i32,
    },
    LinearReps {
        curves: Vec<CrvIn>,
        dir: P3,
        dist: f64,
        reps: u64,
        cap: bool,
    },
    RotationalReps {
        curves: Vec<CrvIn>,
        base: P3,
        axis: P3,
        angle_deg: f64,
        reps: u64,
        cap: bool,
    },
}

// ---- C++ side -----------------------------------------------------------------------

fn spec(c: &CrvIn) -> CurveSpec {
    match c {
        CrvIn::Null => CurveSpec::Null,
        CrvIn::Curve { points } => CurveSpec::ApiCurve {
            points: points.clone(),
        },
        CrvIn::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => CurveSpec::ApiArc {
            center: *center,
            radius: *radius,
            start_deg: *start_deg,
            end_deg: *end_deg,
        },
        CrvIn::Circle { center, radius } => CurveSpec::ApiCircle {
            center: *center,
            radius: *radius,
        },
        CrvIn::Line { start, end } => CurveSpec::LineSegment {
            start: *start,
            end: *end,
        },
        CrvIn::AnalyticCircle { center, radius } => CurveSpec::AnalyticCircle {
            center: *center,
            radius: *radius,
        },
        CrvIn::Hermite { p1, v1, p2, v2 } => CurveSpec::Hermite {
            p1: *p1,
            v1: *v1,
            p2: *p2,
            v2: *v2,
        },
    }
}

fn specs(cs: &[CrvIn]) -> Vec<CurveSpec> {
    cs.iter().map(spec).collect()
}

fn sweep_args(s: &SweepIn) -> SweepArgs {
    SweepArgs {
        path: spec(&s.path),
        scale_reference: spec(&s.scale_reference),
        scale_curve: spec(&s.scale_curve),
        options: s.options,
    }
}

fn call_of(i: &Inputs) -> Call {
    match i {
        Inputs::Linear {
            curves,
            dir,
            dist,
            cap,
        } => Call::Linear {
            curves: specs(curves),
            dir: *dir,
            dist: *dist,
            cap: *cap,
        },
        Inputs::Rotational {
            curves,
            base,
            axis,
            angle_deg,
            cap,
        } => Call::Rotational {
            curves: specs(curves),
            base: *base,
            axis: *axis,
            angle_deg: *angle_deg,
            cap: *cap,
        },
        Inputs::Draft {
            curves,
            height,
            angle_deg,
            corner,
            caps,
        } => Call::Draft {
            curves: specs(curves),
            height: *height,
            angle_deg: *angle_deg,
            corner: *corner,
            caps: *caps,
        },
        Inputs::Pipe { radius, path, cap } => Call::Pipe {
            radius: *radius,
            path: spec(path),
            cap: *cap,
        },
        Inputs::PlanarPath {
            profile,
            path,
            move_profile,
            caps,
        } => Call::PlanarPath {
            profile: specs(profile),
            path: specs(path),
            move_profile: *move_profile,
            caps: *caps,
        },
        Inputs::CurveSweep { profile, sweep } => Call::CurveSweep {
            profile: specs(profile),
            args: sweep_args(sweep),
        },
        Inputs::FromFaces { circles, sweep } => Call::FromFaces {
            circles: circles.clone(),
            args: sweep_args(sweep),
        },
        Inputs::FromEdges { circles, sweep } => Call::FromEdges {
            circles: circles.clone(),
            args: sweep_args(sweep),
        },
        Inputs::Taper {
            curves,
            height,
            angle_deg,
            fillet,
            caps,
        } => Call::Taper {
            curves: specs(curves),
            height: *height,
            angle_deg: *angle_deg,
            fillet: *fillet,
            caps: *caps,
        },
        Inputs::LinearReps {
            curves,
            dir,
            dist,
            reps,
            cap,
        } => Call::LinearReps {
            curves: specs(curves),
            dir: *dir,
            dist: *dist,
            reps: *reps,
            cap: *cap,
        },
        Inputs::RotationalReps {
            curves,
            base,
            axis,
            angle_deg,
            reps,
            cap,
        } => Call::RotationalReps {
            curves: specs(curves),
            base: *base,
            axis: *axis,
            angle_deg: *angle_deg,
            reps: *reps,
            cap: *cap,
        },
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let flat = ffi::run(&call_of(i)).map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

// ---- output splitting ---------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = at
        .checked_add(k)
        .and_then(|end| flat.get(*at..end))
        .ok_or_else(|| harness(format!("record too short ({} values)", flat.len())))?;
    *at += k;
    Ok(part)
}

/// Splits a flat record into keys (layout in the FFI module docs).
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0;
    let status = take(flat, &mut at, 1)?.to_vec();
    let mut out = Output::new().values("status", status.clone());
    if status.first().copied() == Some(0.0) {
        let has = take(flat, &mut at, 1)?.to_vec();
        out = out.values("has", has.clone());
        if has.first().copied() == Some(1.0) {
            out = out.values("counts", take(flat, &mut at, 3)?.to_vec());
        }
        if matches!(
            i,
            Inputs::CurveSweep { .. } | Inputs::FromFaces { .. } | Inputs::FromEdges { .. }
        ) {
            out = out.values("faces", take(flat, &mut at, 3)?.to_vec());
        }
    }
    if at != flat.len() {
        return Err(harness(format!(
            "record has {} trailing values",
            flat.len() - at
        )));
    }
    Ok(out)
}

// ---- Rust side ----------------------------------------------------------------------

fn pt(p: &P3) -> Point3 {
    Point3::new(p[0], p[1], p[2])
}

fn built<T>(r: solidcore::Result<T>, what: &str) -> Result<T, HarnessError> {
    r.map_err(|e| harness(format!("{what}: {e}")))
}

/// The curve of a curve argument (`None` for `NULL`).
fn curve(k: &Kernel, c: &CrvIn) -> Result<Option<SmCurve>, HarnessError> {
    Ok(Some(match c {
        CrvIn::Null => return Ok(None),
        CrvIn::Curve { points } => {
            let pts: Vec<Point3> = points.iter().map(pt).collect();
            SmCurve::from(built(api_curves::create_curve(&pts), "api curve")?)
        }
        CrvIn::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => SmCurve::from(built(
            api_curves::create_arc(pt(center), *radius, *start_deg, *end_deg),
            "arc",
        )?),
        CrvIn::Circle { center, radius } => SmCurve::from(built(
            api_curves::create_circle(pt(center), *radius),
            "circle",
        )?),
        CrvIn::Line { start, end } => SmCurve::from(built(
            api_curves::create_line_segment(pt(start), pt(end)),
            "line",
        )?),
        CrvIn::AnalyticCircle { center, radius } => SmCurve::from(built(
            api_curves::create_analytic_circle(k, pt(center), *radius),
            "analytic circle",
        )?),
        CrvIn::Hermite { p1, v1, p2, v2 } => {
            SmCurve::from(HermiteCurve::new(pt(p1), pt(v1), pt(p2), pt(v2), 3))
        }
    }))
}

fn curves(k: &Kernel, cs: &[CrvIn]) -> Result<Vec<Option<SmCurve>>, HarnessError> {
    cs.iter().map(|c| curve(k, c)).collect()
}

fn refs(cs: &[Option<SmCurve>]) -> Vec<Option<&SmCurve>> {
    cs.iter().map(Option::as_ref).collect()
}

fn non_null(cs: &[Option<SmCurve>]) -> Result<Vec<&SmCurve>, HarnessError> {
    cs.iter()
        .map(|c| c.as_ref().ok_or_else(|| harness("NULL curve")))
        .collect()
}

fn options_of(o: Option<[bool; 5]>) -> Option<SweepOptions> {
    o.map(|[a, b, c, d, e]| SweepOptions {
        cap_ends: a,
        translational_sweep: b,
        move_profile_center_to_path: c,
        orient_profile_perpendicular_to_path: d,
        move_to_profile: e,
    })
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let v = |p: &P3| Vec3::new(p[0], p[1], p[2]);
    let b = Brep::default();
    // `Ok(())` only on success, which the stubbed entry points cannot reach yet.
    let res: solidcore::Result<()> = match i {
        Inputs::Linear {
            curves: cs,
            dir,
            dist,
            cap,
        } => {
            let cs = curves(&k, cs)?;
            api::create_linear_sweep(&k, &refs(&cs), v(dir), *dist, *cap).map(drop)
        }
        Inputs::Rotational {
            curves: cs,
            base,
            axis,
            angle_deg,
            cap,
        } => {
            let cs = curves(&k, cs)?;
            api::create_rotational_sweep(&k, &refs(&cs), pt(base), v(axis), *angle_deg, *cap)
                .map(drop)
        }
        Inputs::Draft {
            curves: cs,
            height,
            angle_deg,
            corner,
            caps,
        } => {
            let cs = curves(&k, cs)?;
            api::create_draft_sweep(&k, &refs(&cs), *height, *angle_deg, *corner, *caps).map(drop)
        }
        Inputs::Pipe { radius, path, cap } => {
            let p = curve(&k, path)?;
            api::create_pipe_sweep(&k, *radius, p.as_ref(), *cap).map(drop)
        }
        Inputs::PlanarPath {
            profile,
            path,
            move_profile,
            caps,
        } => {
            let pc = curves(&k, profile)?;
            let pp = curves(&k, path)?;
            api::create_sweep_along_planar_path(
                &k,
                &non_null(&pc)?,
                &non_null(&pp)?,
                *move_profile,
                *caps,
            )
            .map(drop)
        }
        Inputs::CurveSweep { profile, sweep } => {
            let pc = curves(&k, profile)?;
            let path = curve(&k, &sweep.path)?;
            let sref = curve(&k, &sweep.scale_reference)?;
            let scl = curve(&k, &sweep.scale_curve)?;
            let opts = options_of(sweep.options);
            api::create_curve_sweep(
                &k,
                &refs(&pc),
                path.as_ref(),
                sref.as_ref(),
                scl.as_ref(),
                opts.as_ref(),
            )
            .map(drop)
        }
        Inputs::FromFaces { circles, sweep } | Inputs::FromEdges { circles, sweep } => {
            let path = curve(&k, &sweep.path)?;
            let sref = curve(&k, &sweep.scale_reference)?;
            let scl = curve(&k, &sweep.scale_curve)?;
            let opts = options_of(sweep.options);
            // Placeholder ids, one per planar circle: `SmApiCreatePlanarCircle` cannot be
            // built in Rust yet and only the count is read before the kernel call.
            let n = u32::try_from(circles.len()).map_err(|_| harness("too many circles"))?;
            if matches!(i, Inputs::FromFaces { .. }) {
                let faces: Vec<FaceId> = (0..n).map(FaceId::from_index).collect();
                api::create_curve_sweep_from_faces(
                    &k,
                    &b,
                    &faces,
                    path.as_ref(),
                    sref.as_ref(),
                    scl.as_ref(),
                    opts.as_ref(),
                )
                .map(drop)
            } else {
                let edges: Vec<EdgeId> = (0..n).map(EdgeId::from_index).collect();
                api::create_curve_sweep_from_edges(
                    &k,
                    &b,
                    &edges,
                    path.as_ref(),
                    sref.as_ref(),
                    scl.as_ref(),
                    opts.as_ref(),
                )
                .map(drop)
            }
        }
        Inputs::Taper {
            curves: cs,
            height,
            angle_deg,
            fillet,
            caps,
        } => {
            let cs = curves(&k, cs)?;
            api::create_taper_extrude(&k, &refs(&cs), *height, *angle_deg, *caps, *fillet)
                .map(drop)
        }
        Inputs::LinearReps {
            curves: cs,
            dir,
            dist,
            reps,
            cap,
        } => {
            let cs = curves(&k, cs)?;
            api::create_linear_sweep_with_repetitions(&k, &refs(&cs), v(dir), *dist, *reps, *cap)
                .map(drop)
        }
        Inputs::RotationalReps {
            curves: cs,
            base,
            axis,
            angle_deg,
            reps,
            cap,
        } => {
            let cs = curves(&k, cs)?;
            api::create_rotational_sweep_with_repetitions(
                &k,
                &refs(&cs),
                pt(base),
                v(axis),
                *angle_deg,
                *reps,
                *cap,
            )
            .map(drop)
        }
    };
    match res {
        Ok(()) => Err(harness("rust: a stubbed sweep cannot succeed yet")),
        Err(e) => match Output::unported(&e) {
            Some(out) => Ok(out),
            None => to_output(i, &[status(&e)]),
        },
    }
}

// ---- generators ---------------------------------------------------------------------

/// A length scale: mostly moderate, sometimes tiny or huge.
fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 10f64.powi(-(rng.range_i64(3, 6) as i32)),
        1 => 10f64.powi(rng.range_i64(3, 6) as i32),
        _ => rng.uniform(0.2, 20.0),
    }
}

fn scaled(p: P3, s: f64, off: P3) -> P3 {
    [p[0] * s + off[0], p[1] * s + off[1], p[2] * s + off[2]]
}

/// A planar closed profile in the plane `z = off[2]`: a circle (B-spline or
/// analytic), a rectangle of line segments, or two half-circle arcs.
fn gen_profile(rng: &mut Rng, s: f64, off: P3) -> Vec<CrvIn> {
    let r = s * rng.uniform(0.5, 2.0);
    match rng.below(4) {
        0 => vec![CrvIn::Circle { center: off, radius: r }],
        1 => vec![CrvIn::AnalyticCircle { center: off, radius: r }],
        2 => {
            let (w, h) = (r, r * rng.uniform(0.3, 2.0));
            let c = [[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, h, 0.0], [0.0, h, 0.0]]
                .map(|p| scaled(p, 1.0, off));
c.iter()
                .zip(c.iter().cycle().skip(1))
                .map(|(&start, &end)| CrvIn::Line { start, end })
                .collect()
        }
        _ => vec![
            CrvIn::Arc {
                center: off,
                radius: r,
                start_deg: 0.0,
                end_deg: 180.0,
            },
            CrvIn::Arc {
                center: off,
                radius: r,
                start_deg: 180.0,
                end_deg: 360.0,
            },
        ],
    }
}

fn gen_hermite(rng: &mut Rng, s: f64) -> CrvIn {
    CrvIn::Hermite {
        p1: scaled(rng.point3(-1.0, 1.0), s, [0.0; 3]),
        v1: rng.point3(-1.0, 1.0),
        p2: scaled(rng.point3(-1.0, 1.0), s, [0.0; 3]),
        v2: rng.point3(-1.0, 1.0),
    }
}

/// Spoils a curve list: a `NULL` or a curve without `Copy` at a random position.
fn spoil(rng: &mut Rng, cs: &mut Vec<CrvIn>, s: f64) {
    let at = rng.below(cs.len() as u64 + 1) as usize;
    let bad = if rng.chance(0.5) {
        CrvIn::Null
    } else {
        gen_hermite(rng, s)
    };
    cs.insert(at, bad);
    if rng.chance(0.3) {
        let at = rng.below(cs.len() as u64 + 1) as usize;
        let other = if rng.chance(0.5) {
            CrvIn::Null
        } else {
            gen_hermite(rng, s)
        };
        cs.insert(at, other);
    }
}

/// A straight or gently curved B-spline path from the origin along +z.
fn gen_path(rng: &mut Rng, s: f64) -> CrvIn {
    let len = s * rng.uniform(2.0, 10.0);
    if rng.chance(0.3) {
        return CrvIn::Line {
            start: [0.0; 3],
            end: [0.0, 0.0, len],
        };
    }
    let bend = if rng.chance(0.5) { 0.0 } else { len * 0.2 };
    let points = (0..4)
        .map(|i| {
            let t = f64::from(i) / 3.0;
            [bend * (t * 3.0).sin(), 0.0, len * t]
        })
        .collect();
    CrvIn::Curve { points }
}

fn gen_dir(rng: &mut Rng) -> P3 {
    if rng.chance(0.7) {
        [0.0, 0.0, 1.0]
    } else {
        let mut d = rng.point3(-1.0, 1.0);
        d[2] = d[2].abs() + 0.5;
        d
    }
}

fn gen_caps(rng: &mut Rng) -> i32 {
    if rng.chance(0.85) {
        rng.range_i64(0, 3) as i32
    } else {
        *rng.choose(&[-1, 4, 5, -16, 16]).unwrap_or(&4)
    }
}

/// A profile for a consuming sweep: valid (deferred), empty, or spoiled (compared).
fn gen_consumed(rng: &mut Rng, s: f64, off: P3, empty_ok: bool) -> Vec<CrvIn> {
    let mut cs = gen_profile(rng, s, off);
    match rng.below(10) {
        0..=2 => cs,
        3 if empty_ok => Vec::new(),
        _ => {
            spoil(rng, &mut cs, s);
            cs
        }
    }
}

/// Whether a profile holds a full circle (the FFI rejects sweeping one over a
/// negative distance: upstream never returns).
fn has_full_circle(cs: &[CrvIn]) -> bool {
    cs.iter().any(|c| match c {
        CrvIn::Circle { .. } | CrvIn::AnalyticCircle { .. } => true,
        CrvIn::Arc {
            start_deg, end_deg, ..
        } => (end_deg - start_deg).abs() >= 360.0,
        _ => false,
    })
}

/// A closed profile in the xy plane, off the x axis, for a revolution about x.
fn gen_rotational(rng: &mut Rng, s: f64) -> (Vec<CrvIn>, P3, P3, f64) {
    let off = [0.0, s * rng.uniform(1.5, 4.0), 0.0];
    let curves = gen_consumed(rng, s, off, false);
    let angle = *rng.choose(&[90.0, 180.0, 270.0, 360.0, 45.0]).unwrap_or(&90.0);
    (curves, [0.0; 3], [1.0, 0.0, 0.0], angle)
}

fn gen_sweep_in(rng: &mut Rng, s: f64) -> SweepIn {
    let path = if rng.chance(0.2) {
        CrvIn::Null
    } else {
        gen_path(rng, s)
    };
    let (scale_reference, scale_curve) = if rng.chance(0.15) {
        let len = s * 5.0;
        (
            if rng.chance(0.5) {
                CrvIn::Null
            } else {
                gen_path(rng, s)
            },
            CrvIn::Line {
                start: [s, 0.0, 0.0],
                end: [2.0 * s, 0.0, len],
            },
        )
    } else {
        (CrvIn::Null, CrvIn::Null)
    };
    let options = rng
        .chance(0.6)
        .then(|| [0; 5].map(|_| rng.chance(0.4)));
    SweepIn {
        path,
        scale_reference,
        scale_curve,
        options,
    }
}

/// A case at a random scale. The kernel (`SmPrimitiveCreation`) is slow or crashes on
/// tiny and huge profiles and every case reaching it is deferred, so a case at an
/// extreme scale that passes the API's validation is redrawn at a moderate scale:
/// extreme scales are kept for the compared (rejected) cases.
fn gen_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let i = gen_case_at(rng, s);
    let extreme = !(0.2..=20.0).contains(&s);
    if extreme && rust(&i).is_ok_and(|o| o.deferred.is_some()) {
        let s = rng.uniform(0.2, 20.0);
        return gen_case_at(rng, s);
    }
    i
}

fn gen_case_at(rng: &mut Rng, s: f64) -> Inputs {
    let off = scaled(rng.point3(-1.0, 1.0), s, [0.0; 3]);
    match rng.below(11) {
        0 => {
            let curves = gen_consumed(rng, s, off, false);
            let dist = s * rng.uniform(-5.0, 10.0);
            Inputs::Linear {
                dist: if has_full_circle(&curves) { dist.abs() } else { dist },
                curves,
                dir: gen_dir(rng),
                cap: rng.chance(0.7),
            }
        }
        1 => {
            let (curves, base, axis, angle_deg) = gen_rotational(rng, s);
            Inputs::Rotational {
                curves,
                base,
                axis,
                angle_deg,
                cap: rng.chance(0.5),
            }
        }
        2 => Inputs::Draft {
            curves: gen_consumed(rng, s.max(1e-3), off, false),
            height: s * rng.uniform(-5.0, 10.0),
            angle_deg: rng.uniform(-20.0, 20.0),
            corner: rng.chance(0.5),
            caps: gen_caps(rng),
        },
        3 => Inputs::Pipe {
            radius: match rng.below(6) {
                0 => 0.0,
                1 => -s,
                2 => -0.0,
                _ => s * rng.uniform(0.05, 0.5),
            },
            path: if rng.chance(0.25) {
                CrvIn::Null
            } else {
                gen_path(rng, s)
            },
            cap: rng.chance(0.5),
        },
        4 => {
            let empty = rng.below(4);
            let profile = if empty == 0 {
                Vec::new()
            } else {
                gen_profile(rng, s, [0.0; 3])
            };
            let path = if empty == 1 {
                Vec::new()
            } else {
                vec![CrvIn::Line {
                    start: [0.0; 3],
                    end: [0.0, 0.0, s * rng.uniform(2.0, 10.0)],
                }]
            };
            Inputs::PlanarPath {
                profile,
                path,
                move_profile: rng.chance(0.5),
                caps: if rng.chance(0.6) {
                    *rng.choose(&[-1, 4, 7, -3]).unwrap_or(&4)
                } else {
                    rng.range_i64(0, 3) as i32
                },
            }
        }
        5 => {
            let mut profile = if rng.chance(0.2) {
                Vec::new()
            } else {
                gen_profile(rng, s, [0.0; 3])
            };
            if !profile.is_empty() && rng.chance(0.2) {
                spoil(rng, &mut profile, s);
            }
            Inputs::CurveSweep {
                profile,
                sweep: gen_sweep_in(rng, s),
            }
        }
        6 | 7 => {
            let n = rng.below(3) as usize;
            let circles = (0..n)
                .map(|i| ([0.0, 0.0, 3.0 * s * i as f64], s * rng.uniform(0.5, 2.0)))
                .collect();
            let sweep = gen_sweep_in(rng, s);
            if rng.chance(0.5) {
                Inputs::FromFaces { circles, sweep }
            } else {
                Inputs::FromEdges { circles, sweep }
            }
        }
        8 => Inputs::Taper {
            curves: gen_consumed(rng, s.max(1e-3), off, true),
            height: s * rng.uniform(-5.0, 10.0),
            angle_deg: rng.uniform(-20.0, 20.0),
            fillet: rng.chance(0.5),
            caps: gen_caps(rng),
        },
        9 => Inputs::LinearReps {
            curves: gen_consumed(rng, s, off, true),
            dir: gen_dir(rng),
            dist: s * rng.uniform(0.5, 10.0),
            reps: rng.below(4),
            cap: rng.chance(0.5),
        },
        _ => {
            let (mut curves, base, axis, angle_deg) = gen_rotational(rng, s);
            if rng.chance(0.2) {
                curves.clear();
            }
            Inputs::RotationalReps {
                curves,
                base,
                axis,
                angle_deg: angle_deg / 4.0,
                reps: rng.below(4),
                cap: rng.chance(0.5),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "sweep.api_brep"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach every op, and most cases are compared, not deferred.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        let n = MIN_GENERATED as u64;
        for seed in 0..n {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let op = serde_json::to_value(&i).expect("json")["op"]
                .as_str()
                .expect("op")
                .to_string();
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((op, kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        for op in [
            "linear",
            "rotational",
            "draft",
            "pipe",
            "planar_path",
            "curve_sweep",
            "from_faces",
            "from_edges",
            "taper",
            "linear_reps",
            "rotational_reps",
        ] {
            let failed = tally
                .keys()
                .any(|(o, k)| o == op && k.starts_with("status:"));
            assert!(failed, "{op} never compared");
            let deferred = tally
                .keys()
                .any(|(o, k)| o == op && k.starts_with("deferred:"));
            assert!(deferred, "{op} never reaches the kernel");
        }
        let compared: usize = tally
            .iter()
            .filter(|((_, k), _)| k.starts_with("status:"))
            .map(|(_, v)| v)
            .sum();
        assert!(
            compared * 2 >= MIN_GENERATED,
            "only {compared} compared cases"
        );
    }
}
