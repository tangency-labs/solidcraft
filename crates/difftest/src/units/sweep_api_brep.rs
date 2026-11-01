//! Runner for unit `sweep.api_brep`: the `SM_API` B-rep sweep entry points.
//!
//! C++: `solidcore_ffi::units::sweep_api_brep` (upstream
//! `source/SM_API/src/SmApiBrep.cpp`). Rust: `solidcore::sweep::api_brep`.
//!
//! A case is one API call (`op` plus its arguments). Curve arguments are built the same
//! way on both sides: the `SM_API` curve constructors (`SmApiCreateCurve`,
//! `SmApiCreateArc`, `SmApiCreateCircle`, `SmApiCreateLineSegment`, ported in
//! `primitive.api_curves`), or a 3D `SmHermiteCurve` (a curve without `Copy`). Faces
//! and edges come from `SmApiCreatePlanarCircle` B-reps on both sides (fixture op
//! `from_box_face`: one face of an `SmApiCreateBox` B-rep, plus the result's volume). Both sides
//! produce the flat record documented in the FFI module, split into keys: `status`
//! (`0` = `SM_SUCCESS`, else the `SmStatus`: `1001` `SM_ERR`, `1007`
//! `SM_ERR_INVALID_INPUT`; `-1` on the Rust side for an error upstream cannot produce),
//! then on success `has`, `counts` (faces, edges, vertices) and, for the curve sweeps,
//! `faces` (start, side, end).
//!
//! A call runs on the Rust side until the next unported callee; a case that reaches one
//! is deferred. A successful call is compared by its status, presence and counts, so
//! both the input validation and curve copies (`NULL` curves, curves without `Copy`,
//! empty inputs, a missing path, a non-positive pipe radius, a cap selector out of
//! range) and the sweep results are covered. Every run has at least [`MIN_GENERATED`]
//! generated cases.
//!
//! Linear fixtures may opt into `observe_geometry` (default false). The additive
//! record contains actual reachable vertices, edge intervals and samples at
//! 0.17/0.37/0.83, followed by manifold and volume queries (accuracy 1e-6).
//! Evaluation/query statuses are retained; no healing or extra stitching is done.
//! Observation decoding rejects truncated records without unchecked indexing.
//! Legacy records and generated inputs remain unchanged.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::error::EntityRef;
use solidcore::geom::attribute::{Attribute, Behavior};
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
        #[serde(default, skip_serializing_if = "std::ops::Not::not")]
        observe_geometry: bool,
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
        /// Tag every disc face and edge first (this behaviour) and compare the
        /// attribute sets of the result and of the discs.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attributed: Option<u8>,
    },
    FromEdges {
        circles: Vec<(P3, f64)>,
        sweep: SweepIn,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        attributed: Option<u8>,
    },
    /// Fixtures only: face `face` of `SmApiCreateBox(origin, size)`, then the volume.
    FromBoxFace {
        origin: P3,
        size: P3,
        face: u64,
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
            ..
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
        Inputs::FromFaces {
            circles,
            sweep,
            attributed,
        } => Call::FromFaces {
            circles: circles.clone(),
            args: sweep_args(sweep),
            attributed: *attributed,
        },
        Inputs::FromEdges {
            circles,
            sweep,
            attributed,
        } => Call::FromEdges {
            circles: circles.clone(),
            args: sweep_args(sweep),
            attributed: *attributed,
        },
        Inputs::FromBoxFace {
            origin,
            size,
            face,
            sweep,
        } => Call::FromBoxFace {
            origin: *origin,
            size: *size,
            face: *face,
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
    let call = call_of(i);
    let flat = if observes(i) {
        ffi::observe_linear(&call)
    } else {
        ffi::run(&call)
    }
    .map_err(|e| harness(e.to_string()))?;
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
            Inputs::CurveSweep { .. }
                | Inputs::FromFaces { .. }
                | Inputs::FromEdges { .. }
                | Inputs::FromBoxFace { .. }
        ) {
            out = out.values("faces", take(flat, &mut at, 3)?.to_vec());
        }
        if matches!(i, Inputs::FromBoxFace { .. }) && has.first().copied() == Some(1.0) {
            let [volume_status, volume] = take(flat, &mut at, 2)? else {
                return Err(harness("missing volume record"));
            };
            out = out
                .int("volume_status", *volume_status as i64)
                .scalar("volume", *volume);
        }
    }
    if attributed(i).is_some() {
        out = out.values("attributes", flat.get(at..).unwrap_or_default().to_vec());
        at = flat.len();
    }
    if observes(i) && out.values.get("has").and_then(|v| v.first()) == Some(&1.0) {
        let counts = out
            .values
            .get("counts")
            .ok_or_else(|| harness("missing counts"))?;
        let count = |index: usize| -> Result<usize, HarnessError> {
            let value = *counts.get(index).ok_or_else(|| harness("missing count"))?;
            if value.is_finite() && (0.0..=4096.0).contains(&value) && value.fract() == 0.0 {
                Ok(value as usize)
            } else {
                Err(harness("invalid observation count"))
            }
        };
        let (nv, ne) = (count(2)?, count(1)?);
        let observation = flat
            .get(at..)
            .ok_or_else(|| harness("missing observation record"))?;
        out = decode_observation(out, observation, nv, ne)?;
        at = flat.len();
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

/// `SmApiCreatePlanarCircle` B-reps of the circles; an unported construction defers.
fn discs(k: &mut Kernel, circles: &[(P3, f64)]) -> solidcore::Result<Vec<Brep>> {
    circles
        .iter()
        .map(|(c, r)| solidcore::primitive::api_primitives::create_planar_circle(k, pt(c), *r))
        .collect()
}

/// Every face of the disc B-reps, with its B-rep.
fn disc_faces(discs: &[Brep]) -> solidcore::Result<Vec<(&Brep, FaceId)>> {
    let mut faces = Vec::new();
    for b in discs {
        for f in solidcore::topology::brep::p1::faces(b, None)? {
            faces.push((b, f));
        }
    }
    Ok(faces)
}

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
        profile_at_path_start: false,
    })
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The result of a successful sweep call: a bare B-rep, or a curve sweep's B-rep with
/// its start, side and end faces.
enum Swept {
    /// The B-rep-returning entry points.
    Brep(Brep),
    /// `SmApiCreateCurveSweep` and the face / edge variants.
    CurveSweep(api::CurveSweep),
}

/// The record of a successful call (layout in the FFI module docs): the status, the
/// B-rep's presence and its face, edge and vertex counts, plus the curve sweep's start,
/// side and end face counts.
fn success(s: &Swept) -> Output {
    let (brep, faces) = match s {
        Swept::Brep(b) => (b, None),
        Swept::CurveSweep(c) => (
            &c.brep,
            Some([
                c.start_faces.len() as f64,
                c.side_faces.len() as f64,
                c.end_faces.len() as f64,
            ]),
        ),
    };
    let mut out = Output::new()
        .scalar("status", 0.0)
        .scalar("has", 1.0)
        .values(
            "counts",
            [
                brep.face_list().len() as f64,
                brep.edge_list().len() as f64,
                brep.vertex_list().len() as f64,
            ],
        );
    if let Some(f) = faces {
        out = out.values("faces", f);
    }
    out
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    // an input the FFI refuses is a harness error on both sides: the curve sweeps'
    // path polygonization does not return upstream for a huge curved path (HANG-5 /
    // HANG-43, nightly fuzz00293), so the Rust port does not run it either
    if let Inputs::CurveSweep { sweep, .. }
    | Inputs::FromFaces { sweep, .. }
    | Inputs::FromEdges { sweep, .. }
    | Inputs::FromBoxFace { sweep, .. } = i
    {
        ffi::check_path_extent(&spec(&sweep.path)).map_err(|e| harness(e.to_string()))?;
    }
    // nor more repetitions than the FFI runs (nightly fuzz00132: 2^31 linear repetitions
    // ran past the fuzz timeout on the Rust side alone)
    if let Inputs::LinearReps { reps, .. } | Inputs::RotationalReps { reps, .. } = i {
        ffi::check_repetitions(*reps).map_err(|e| harness(e.to_string()))?;
    }
    let mut k = Kernel::default();
    let v = |p: &P3| Vec3::new(p[0], p[1], p[2]);
    // A successful call is compared by its status, presence and counts; a path that
    // reaches an unported callee is deferred.
    let res: solidcore::Result<Swept> = match i {
        Inputs::Linear {
            curves: cs,
            dir,
            dist,
            cap,
            ..
        } => {
            let cs = curves(&k, cs)?;
            api::create_linear_sweep(&mut k, &refs(&cs), v(dir), *dist, *cap).map(Swept::Brep)
        }
        Inputs::Rotational {
            curves: cs,
            base,
            axis,
            angle_deg,
            cap,
        } => {
            let cs = curves(&k, cs)?;
            api::create_rotational_sweep(&mut k, &refs(&cs), pt(base), v(axis), *angle_deg, *cap)
                .map(Swept::Brep)
        }
        Inputs::Draft {
            curves: cs,
            height,
            angle_deg,
            corner,
            caps,
        } => {
            let cs = curves(&k, cs)?;
            api::create_draft_sweep(&mut k, &refs(&cs), *height, *angle_deg, *corner, *caps)
                .map(Swept::Brep)
        }
        Inputs::Pipe { radius, path, cap } => {
            let p = curve(&k, path)?;
            api::create_pipe_sweep(&mut k, *radius, p.as_ref(), *cap).map(Swept::Brep)
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
                &mut k,
                &non_null(&pc)?,
                &non_null(&pp)?,
                *move_profile,
                *caps,
            )
            .map(Swept::Brep)
        }
        Inputs::CurveSweep { profile, sweep } => {
            let pc = curves(&k, profile)?;
            let path = curve(&k, &sweep.path)?;
            let sref = curve(&k, &sweep.scale_reference)?;
            let scl = curve(&k, &sweep.scale_curve)?;
            let opts = options_of(sweep.options);
            api::create_curve_sweep(
                &mut k,
                &refs(&pc),
                path.as_ref(),
                sref.as_ref(),
                scl.as_ref(),
                opts.as_ref(),
            )
            .map(Swept::CurveSweep)
        }
        Inputs::FromFaces {
            circles,
            sweep,
            attributed: Some(behavior),
        }
        | Inputs::FromEdges {
            circles,
            sweep,
            attributed: Some(behavior),
        } => return rust_attributed(&mut k, i, circles, sweep, *behavior),
        Inputs::FromFaces { circles, sweep, .. } | Inputs::FromEdges { circles, sweep, .. } => {
            let path = curve(&k, &sweep.path)?;
            let sref = curve(&k, &sweep.scale_reference)?;
            let scl = curve(&k, &sweep.scale_curve)?;
            let opts = options_of(sweep.options);
            // The faces of `SmApiCreatePlanarCircle` B-reps, as the C++ side.
            let from_faces = matches!(i, Inputs::FromFaces { .. });
            (|| -> solidcore::Result<Swept> {
                let discs = discs(&mut k, circles)?;
                let faces = disc_faces(&discs)?;
                if from_faces {
                    api::create_curve_sweep_from_faces(
                        &mut k,
                        &faces,
                        path.as_ref(),
                        sref.as_ref(),
                        scl.as_ref(),
                        opts.as_ref(),
                    )
                    .map(Swept::CurveSweep)
                } else {
                    let mut edges: Vec<(&Brep, EdgeId)> = Vec::new();
                    for (b, f) in &faces {
                        let fe = solidcore::topology::face::p4::edges(b, *f, None, false, None)?;
                        edges.extend(fe.into_iter().map(|e| (*b, e)));
                    }
                    api::create_curve_sweep_from_edges(
                        &mut k,
                        &edges,
                        path.as_ref(),
                        sref.as_ref(),
                        scl.as_ref(),
                        opts.as_ref(),
                    )
                    .map(Swept::CurveSweep)
                }
            })()
        }
        Inputs::FromBoxFace {
            origin,
            size,
            face,
            sweep,
        } => {
            let path = curve(&k, &sweep.path)?;
            let sref = curve(&k, &sweep.scale_reference)?;
            let scl = curve(&k, &sweep.scale_curve)?;
            let opts = options_of(sweep.options);
            let bx = built(
                solidcore::primitive::api_primitives::create_box(
                    &mut k,
                    pt(origin),
                    size[0],
                    size[1],
                    size[2],
                ),
                "box",
            )?;
            let faces = built(solidcore::topology::brep::p1::faces(&bx, None), "box faces")?;
            let f = usize::try_from(*face)
                .ok()
                .and_then(|i| faces.get(i).copied())
                .ok_or_else(|| harness("box face index out of range"))?;
            api::create_curve_sweep_from_faces(
                &mut k,
                &[(&bx, f)],
                path.as_ref(),
                sref.as_ref(),
                scl.as_ref(),
                opts.as_ref(),
            )
            .map(Swept::CurveSweep)
        }
        Inputs::Taper {
            curves: cs,
            height,
            angle_deg,
            fillet,
            caps,
        } => {
            let cs = curves(&k, cs)?;
            api::create_taper_extrude(&mut k, &refs(&cs), *height, *angle_deg, *caps, *fillet)
                .map(Swept::Brep)
        }
        Inputs::LinearReps {
            curves: cs,
            dir,
            dist,
            reps,
            cap,
        } => {
            let cs = curves(&k, cs)?;
            api::create_linear_sweep_with_repetitions(
                &mut k,
                &refs(&cs),
                v(dir),
                *dist,
                *reps,
                *cap,
            )
            .map(Swept::Brep)
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
                &mut k,
                &refs(&cs),
                pt(base),
                v(axis),
                *angle_deg,
                *reps,
                *cap,
            )
            .map(Swept::Brep)
        }
    };
    match res {
        Ok(mut s) => {
            let out = success(&s);
            if let (Inputs::FromBoxFace { .. }, Swept::CurveSweep(c)) = (i, &mut s) {
                return Ok(match box_volume(&k, &mut c.brep) {
                    Ok((st, v)) => out.int("volume_status", st).scalar("volume", v),
                    Err(e) => Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())),
                });
            }
            if !observes(i) {
                return Ok(out);
            }
            let body = match &mut s {
                Swept::Brep(b) => b,
                Swept::CurveSweep(c) => &mut c.brep,
            };
            let nv = solidcore::topology::brep::p1::vertices(body, None)
                .map_err(|e| harness(e.to_string()))?
                .len();
            let ne = solidcore::topology::brep::p1::edges(body, None)
                .map_err(|e| harness(e.to_string()))?
                .len();
            match rust_observation(&k, body) {
                Ok(flat) => decode_observation(out, &flat, nv, ne),
                Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string()))),
            }
        }
        Err(e) => match Output::unported(&e) {
            Some(out) => Ok(out),
            None => to_output(i, &[status(&e)]),
        },
    }
}

/// The behaviour of an attributed curve sweep's tags.
fn attributed(i: &Inputs) -> Option<u8> {
    match i {
        Inputs::FromFaces { attributed, .. } | Inputs::FromEdges { attributed, .. } => *attributed,
        _ => None,
    }
}

/// The shim's `tag_disc`: every face and edge of disc `disc` gets its own long attribute.
fn tag_disc(b: &mut Brep, disc: u64, behavior: Behavior) -> solidcore::Result<()> {
    use solidcore::topology::brep::p1;
    let faces = p1::faces(b, None)?;
    let edges = p1::edges(b, None)?;
    let tagged = faces
        .into_iter()
        .enumerate()
        .map(|(i, f)| (EntityRef::Face(f), 0, i))
        .chain(
            edges
                .into_iter()
                .enumerate()
                .map(|(i, e)| (EntityRef::Edge(e), 1, i)),
        );
    for (entity, kind, index) in tagged {
        let id = ffi::ATTRIBUTE_BASE + 1000 * disc + 500 * kind + index as u64;
        b.add_attribute(entity, Attribute::long(id, id as i64, behavior))?;
    }
    Ok(())
}

/// The shim's `put_attribute_rows`: the sorted face rows then edge rows of `b`.
fn put_attribute_rows(o: &mut Vec<f64>, b: &Brep) -> solidcore::Result<()> {
    use solidcore::topology::brep::p1;
    let row = |e: EntityRef| -> Vec<f64> {
        let mut entries: Vec<[f64; 4]> = b
            .entity_attributes(e)
            .into_iter()
            .map(|(_, a)| {
                [
                    a.attribute_id() as f64,
                    f64::from(a.behavior() as u8),
                    a.long_value().map_or(0.0, |v| v as f64),
                    a.users().len() as f64,
                ]
            })
            .collect();
        entries.sort_by(|x, y| total(x, y));
        let mut r = vec![entries.len() as f64];
        r.extend(entries.into_iter().flatten());
        r
    };
    let faces: Vec<EntityRef> = p1::faces(b, None)?.into_iter().map(EntityRef::Face).collect();
    let edges: Vec<EntityRef> = p1::edges(b, None)?.into_iter().map(EntityRef::Edge).collect();
    for list in [faces, edges] {
        let mut rows: Vec<Vec<f64>> = list.into_iter().map(row).collect();
        rows.sort_by(|x, y| total(x, y));
        o.push(rows.len() as f64);
        o.extend(rows.into_iter().flatten());
    }
    Ok(())
}

/// Lexicographic `total_cmp` order (the shim's `std::sort` of value vectors).
fn total(x: &[f64], y: &[f64]) -> core::cmp::Ordering {
    x.iter()
        .zip(y)
        .map(|(p, q)| p.total_cmp(q))
        .find(|o| o.is_ne())
        .unwrap_or_else(|| x.len().cmp(&y.len()))
}

/// An attributed `FromFaces` / `FromEdges`: the discs tagged as the shim tags them, the
/// sweep, then the result's and the discs' attribute rows.
fn rust_attributed(
    k: &mut Kernel,
    i: &Inputs,
    circles: &[(P3, f64)],
    sweep: &SweepIn,
    behavior: u8,
) -> Result<Output, HarnessError> {
    let behavior = Behavior::from_code(i32::from(behavior))
        .map_err(|e| harness(format!("behavior: {e}")))?;
    let path = curve(k, &sweep.path)?;
    let sref = curve(k, &sweep.scale_reference)?;
    let scl = curve(k, &sweep.scale_curve)?;
    let opts = options_of(sweep.options);
    let from_faces = matches!(i, Inputs::FromFaces { .. });
    let mut discs = built(discs(k, circles), "discs")?;
    for (d, b) in discs.iter_mut().enumerate() {
        built(tag_disc(b, d as u64, behavior), "tag")?;
    }
    let result = (|| -> solidcore::Result<api::CurveSweep> {
        let faces = disc_faces(&discs)?;
        if from_faces {
            api::create_curve_sweep_from_faces(
                k,
                &faces,
                path.as_ref(),
                sref.as_ref(),
                scl.as_ref(),
                opts.as_ref(),
            )
        } else {
            let mut edges: Vec<(&Brep, EdgeId)> = Vec::new();
            for (b, f) in &faces {
                let fe = solidcore::topology::face::p4::edges(b, *f, None, false, None)?;
                edges.extend(fe.into_iter().map(|e| (*b, e)));
            }
            api::create_curve_sweep_from_edges(
                k,
                &edges,
                path.as_ref(),
                sref.as_ref(),
                scl.as_ref(),
                opts.as_ref(),
            )
        }
    })();
    let mut rows = Vec::new();
    let out = match &result {
        Ok(c) => {
            built(put_attribute_rows(&mut rows, &c.brep), "attributes")?;
            success(&Swept::CurveSweep(c.clone()))
        }
        Err(e) => match Output::unported(e) {
            Some(out) => return Ok(out),
            None => Output::new().values("status", vec![status(e)]),
        },
    };
    for b in &discs {
        built(put_attribute_rows(&mut rows, b), "attributes")?;
    }
    Ok(out.values("attributes", rows))
}

fn observes(i: &Inputs) -> bool {
    matches!(
        i,
        Inputs::Linear {
            observe_geometry: true,
            ..
        }
    )
}

fn decode_observation(
    mut out: Output,
    flat: &[f64],
    nv: usize,
    ne: usize,
) -> Result<Output, HarnessError> {
    if nv > 4096 || ne > 4096 {
        return Err(harness("observation too large"));
    }
    let mut at = 0;
    out = out
        .values("linear_vertices", take(flat, &mut at, 3 * nv)?.to_vec())
        .values(
            "linear_edge_intervals",
            take(flat, &mut at, 2 * ne)?.to_vec(),
        );
    let mut statuses = Vec::new();
    let mut samples = Vec::new();
    for _ in 0..3 * ne {
        let [evaluation_status] = take(flat, &mut at, 1)? else {
            return Err(harness("missing evaluation status"));
        };
        statuses.push(*evaluation_status as i64);
        samples.extend_from_slice(take(flat, &mut at, 3)?);
    }
    let [manifold_status, manifold, volume_status, volume] = take(flat, &mut at, 4)? else {
        return Err(harness("missing query record"));
    };
    out = out
        .ints("linear_evaluation_status", statuses)
        .values("linear_edge_samples", samples)
        .ints(
            "linear_manifold",
            vec![*manifold_status as i64, *manifold as i64],
        )
        .int("linear_volume_status", *volume_status as i64)
        .scalar("linear_volume", *volume);
    if at != flat.len() {
        return Err(harness("trailing observation values"));
    }
    Ok(out)
}

/// `SmApiBrepComputeVolume(body, 1e-6)`: its status and, on success, the volume.
fn box_volume(k: &Kernel, body: &mut Brep) -> solidcore::Result<(i64, f64)> {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    match brep_volume(k, body, 1e-6, &attrs) {
        Ok(v) => Ok((0, v)),
        Err(e) if e.is_unported() => Err(e),
        Err(e) => Ok((status(&e) as i64, 0.0)),
    }
}

fn rust_observation(k: &Kernel, body: &mut Brep) -> solidcore::Result<Vec<f64>> {
    use solidcore::geom::attribute::{AttributeStore, ObjectKey};
    use solidcore::mass::integrator::properties::{brep_volume, PropertyAttributes};
    use solidcore::topology::brep::p1;
    let vertices = p1::vertices(body, None)?;
    let edges = p1::edges(body, None)?;
    if vertices.len() > 4096 || edges.len() > 4096 {
        return Err(SolidError::invalid_input(
            "harness::linear_observation",
            "observation too large",
        ));
    }
    let mut flat = Vec::new();
    for id in vertices {
        let p = body.vertex(id)?.point;
        flat.extend([p.x, p.y, p.z]);
    }
    for &id in &edges {
        let e = body.edge(id)?;
        flat.extend([e.interval.min(), e.interval.max()]);
    }
    for id in edges {
        let e = body.edge(id)?;
        for fraction in [0.17, 0.37, 0.83] {
            let result = match &e.curve {
                Some(c) => c.evaluate_point(k, e.interval.evaluate(fraction)),
                None => Err(SolidError::sm_err("harness::linear_observation")),
            };
            match result {
                Ok(p) => flat.extend([0.0, p.x, p.y, p.z]),
                Err(e) if e.is_unported() => return Err(e),
                Err(e) => flat.extend([status(&e), 0.0, 0.0, 0.0]),
            }
        }
    }
    match solidcore::query::api_queries::brep_is_manifold_solid(body) {
        Ok(m) => flat.extend([0.0, f64::from(m)]),
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => flat.extend([status(&e), 0.0]),
    }
    let store = AttributeStore::default();
    let attrs = PropertyAttributes {
        store: &store,
        key_of: &|_| ObjectKey(0),
    };
    match brep_volume(k, body, 1e-6, &attrs) {
        Ok(v) => flat.extend([0.0, v]),
        Err(e) if e.is_unported() => return Err(e),
        Err(e) => flat.extend([status(&e), 0.0]),
    }
    Ok(flat)
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
        0 => vec![CrvIn::Circle {
            center: off,
            radius: r,
        }],
        1 => vec![CrvIn::AnalyticCircle {
            center: off,
            radius: r,
        }],
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

/// Whether a profile holds a full circle. The generator sweeps one over a positive
/// distance only (at c9c5979 a negative distance never returned, HANG-13), which keeps
/// the generated inputs unchanged; the fixture `hang13-negative-full-circle.json`
/// covers negative distances.
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
    let angle = *rng
        .choose(&[90.0, 180.0, 270.0, 360.0, 45.0])
        .unwrap_or(&90.0);
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
    let options = rng.chance(0.6).then(|| [0; 5].map(|_| rng.chance(0.4)));
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
                observe_geometry: false,
                dist: if has_full_circle(&curves) {
                    dist.abs()
                } else {
                    dist
                },
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
                Inputs::FromFaces {
                    circles,
                    sweep,
                    attributed: None,
                }
            } else {
                Inputs::FromEdges {
                    circles,
                    sweep,
                    attributed: None,
                }
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

    /// The nightly fuzz00293 input (run 41040730716, HANG-43): a disc swept along a
    /// four-point path whose third point is at `z`.
    fn from_faces(z: f64) -> Inputs {
        serde_json::from_value(serde_json::json!({
            "op": "from_faces",
            "circles": [[[0.0, 0.0, 0.0], 2.388303630595588]],
            "sweep": {
                "options": null,
                "path": {"kind": "curve", "points": [
                    [0.0, 0.0, 0.0],
                    [2.8458631679264377, 0.0, 5.636683100796663],
                    [3.0752528636317265, 0.0, z],
                    [0.4772692587692055, 0.0, 16.91004930238999]
                ]},
                "scale_curve": {"kind": "line", "start": [2.4410083596487024, 0.0, 0.0],
                                "end": [4.882016719297405, 0.0, 12.205041798243512]},
                "scale_reference": {"kind": "curve", "points": [
                    [0.0, 0.0, 0.0], [0.0, 0.0, 4.140555904167603],
                    [0.0, 0.0, 8.281111808335206], [0.0, 0.0, 12.42166771250281]
                ]}
            }
        }))
        .unwrap()
    }

    /// HANG-5 / HANG-43: the C++ path polygonization never returns for a path point of
    /// `1e300`; both sides refuse it with the same message. The path at `1e7` (inside
    /// the bound) still runs on both sides and agrees.
    #[test]
    fn huge_curve_sweep_path_is_refused_on_both_sides() {
        let i = from_faces(1e300);
        let c = cpp(&i).unwrap_err().to_string();
        let r = rust(&i).unwrap_err().to_string();
        assert!(c.contains("HANG-5"), "{c}");
        assert_eq!(c, r);

        let i = from_faces(1e7);
        let (c, r) = (cpp(&i).unwrap(), rust(&i).unwrap());
        assert!(r.deferred.is_none(), "{:?}", r.deferred);
        let d = crate::compare(&c, &r, Runner.abs_floors());
        assert!(d.mismatches.is_empty(), "{:?}", d.mismatches);
    }

    /// The nightly fuzz00132 input (run 41190675541): two circles swept `reps` times,
    /// linearly or about the Z axis.
    fn repeated(op: &str, reps: u64) -> Inputs {
        let circle = |x: f64| {
            serde_json::json!({"kind": "arc", "center": [x, 0.0, 0.0], "radius": 1.0,
                               "start_deg": 0.0, "end_deg": 360.0})
        };
        let mut v = serde_json::json!({
            "op": op, "curves": [circle(0.0), circle(5.0)], "reps": reps, "cap": true
        });
        if op == "linear_reps" {
            let d = std::f64::consts::FRAC_1_SQRT_2;
            v["dir"] = serde_json::json!([d, 0.0, d]);
            v["dist"] = serde_json::json!(1.0);
        } else {
            v["base"] = serde_json::json!([0.0, -3.0, 0.0]);
            v["axis"] = serde_json::json!([1.0, 0.0, 0.0]);
            v["angle_deg"] = serde_json::json!(10.0);
        }
        serde_json::from_value(v).unwrap()
    }

    /// The FFI runs at most `2^20` repetitions; the Rust side refuses the same counts
    /// with the same message instead of running them (fuzz00132: `2^31` linear
    /// repetitions took the Rust side past the fuzz timeout).
    #[test]
    fn repetitions_past_the_ffi_bound_are_refused_on_both_sides() {
        for op in ["linear_reps", "rotational_reps"] {
            for reps in [ffi::MAX_REPETITIONS + 1, 1 << 31] {
                let i = repeated(op, reps);
                let c = cpp(&i).unwrap_err().to_string();
                let r = rust(&i).unwrap_err().to_string();
                assert!(c.contains("more than 2^20 repetitions"), "{c}");
                assert_eq!(c, r, "{op} {reps}");
            }
            let i = repeated(op, 3);
            let (c, r) = (cpp(&i).unwrap(), rust(&i).unwrap());
            assert!(r.deferred.is_none(), "{:?}", r.deferred);
            let d = crate::compare(&c, &r, Runner.abs_floors());
            assert!(d.mismatches.is_empty(), "{:?}", d.mismatches);
        }
        assert!(ffi::check_repetitions(ffi::MAX_REPETITIONS).is_ok());
    }

    /// Golden stream executed against the pre-observer runner at 11595407bedc.
    /// Includes each next RNG draw, not only serialized case identities.
    #[test]
    fn legacy_generator_serialization_and_rng_stream() {
        let mut hash = 0xcbf29ce484222325_u64;
        for seed in 0..400_u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let serialized = serde_json::to_string(&gen_case(&mut rng)).expect("serialize");
            assert!(!serialized.contains("observe_geometry"));
            let record = format!("{serialized}\t{}", rng.next_u64());
            for byte in record.bytes() {
                hash = (hash ^ u64::from(byte)).wrapping_mul(0x100000001b3);
            }
        }
        assert_eq!(hash, 0xa1d1_fd86_67ec_73fa);
    }

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
        }
        // Valid inputs must reach a successful kernel call or an unported callee.
        for op in [
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
            let reached_kernel = tally
                .keys()
                .any(|(o, k)| o == op && (k == "status:0" || k.starts_with("deferred:")));
            assert!(reached_kernel, "{op} never reaches the kernel");
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

    #[test]
    fn linear_geometry_native_and_analytic_controls() {
        let data: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/sweep.api_brep/linear_geometry.json"
        ))
        .expect("fixture");
        let mut observations = Vec::new();
        for case in data["cases"].as_array().expect("cases") {
            let i: Inputs = serde_json::from_value(case["inputs"].clone()).expect("input");
            let distance = case["inputs"]["dist"].as_f64().expect("distance");
            let native = cpp(&i).expect("native");
            let observed = rust(&i).expect("rust");
            assert!(crate::compare::compare(&native, &observed, &[]).pass());
            assert_eq!(observed.values["status"], vec![0.0]);
            assert_eq!(observed.values["has"], vec![1.0]);
            assert_eq!(observed.values["counts"], vec![6.0, 12.0, 8.0]);
            assert_eq!(observed.ints["linear_manifold"], vec![0, 1]);
            assert_eq!(observed.ints["linear_volume_status"], vec![0]);
            assert_eq!(observed.ints["linear_evaluation_status"], vec![0; 36]);
            assert!(
                (observed.values["linear_volume"][0] - 6.0 * distance).abs()
                    <= 1e-9 * 6.0 * distance
            );
            let mut expected = Vec::new();
            for x in [2.0, 4.0] {
                for y in [3.0, 6.0] {
                    for z in [5.0, 5.0 + distance] {
                        expected.push([x, y, z]);
                    }
                }
            }
            let mut actual = observed.values["linear_vertices"]
                .as_chunks::<3>()
                .0
                .to_vec();
            actual.sort_by(|a, b| a.partial_cmp(b).expect("finite"));
            assert_eq!(actual, expected);
            // Closed-form prism edges, independent of either kernel's graph/order.
            let mut expected_edges = Vec::new();
            for (index, a) in expected.iter().enumerate() {
                for b in expected.iter().skip(index + 1) {
                    if a.iter().zip(b).filter(|(a, b)| a != b).count() == 1 {
                        expected_edges.push((*a, *b));
                    }
                }
            }
            assert_eq!(expected_edges.len(), 12);
            for output in [&native, &observed] {
                let mut seen = vec![false; 12];
                for samples in output.values["linear_edge_samples"].as_chunks::<9>().0 {
                    let index = expected_edges
                        .iter()
                        .position(|&(a, b)| {
                            [(a, b), (b, a)].iter().any(|(a, b)| {
                                [0.17, 0.37, 0.83].iter().enumerate().all(|(j, &t)| {
                                    (0..3).all(|axis| {
                                        let value = a[axis] + t * (b[axis] - a[axis]);
                                        let actual = samples[3 * j + axis];
                                        (actual - value).abs()
                                            <= 1e-9 * actual.abs().max(value.abs())
                                    })
                                })
                            })
                        })
                        .expect("samples lie on a true prism edge at the requested fractions");
                    assert!(!seen[index], "duplicate edge");
                    seen[index] = true;
                }
                assert!(seen.into_iter().all(|seen| seen));
            }
            for key in ["linear_vertices", "linear_edge_samples", "linear_volume"] {
                let mut changed = observed.clone();
                changed.values.get_mut(key).expect("key")[0] *= 1.0 + 1e-8;
                assert!(
                    !crate::compare::compare(&native, &changed, &[]).pass(),
                    "{key}"
                );
            }
            let mut changed = observed.clone();
            changed.ints.get_mut("linear_manifold").expect("manifold")[1] ^= 1;
            assert!(!crate::compare::compare(&native, &changed, &[]).pass());
            let mut legacy_input = i.clone();
            if let Inputs::Linear {
                observe_geometry, ..
            } = &mut legacy_input
            {
                *observe_geometry = false;
            }
            let legacy = rust(&legacy_input).expect("legacy");
            let legacy_native = cpp(&legacy_input).expect("legacy native");
            assert!(crate::compare::compare(&legacy_native, &legacy, &[]).pass());
            let legacy_flat = ffi::run(&call_of(&legacy_input)).expect("legacy record");
            let observed_flat = ffi::observe_linear(&call_of(&i)).expect("observed record");
            assert_eq!(
                legacy_flat.iter().map(|v| v.to_bits()).collect::<Vec<_>>(),
                observed_flat[..legacy_flat.len()]
                    .iter()
                    .map(|v| v.to_bits())
                    .collect::<Vec<_>>()
            );
            for (key, value) in &legacy.values {
                assert_eq!(observed.values.get(key), Some(value));
                assert_eq!(native.values.get(key), legacy_native.values.get(key));
            }
            eprintln!(
                "linear distance={distance}: native counts={:?} manifold={:?} volume={:?}; 36 edge evaluations; four mutations detected; legacy prefix bit-identical",
                native.values["counts"],
                native.ints["linear_manifold"],
                native.values["linear_volume"]
            );
            observations.push(observed);
        }
        assert_eq!(observations.len(), 2);
        assert_eq!(
            observations[0].values["counts"],
            observations[1].values["counts"]
        );
        for key in ["linear_vertices", "linear_edge_samples", "linear_volume"] {
            assert_ne!(observations[0].values[key], observations[1].values[key]);
        }
    }
}
