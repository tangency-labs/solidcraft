//! Runner for unit `topology.edgeuse_p2`. C++: `solidcore_ffi::units::topology_edgeuse_p2`.
//! Rust: `solidcore::topology::edgeuse::p2`.
//!
//! The B-reps are those of unit `topology.edgeuse_p1` (upstream `.smb` files, wire edges,
//! faces from B-spline surfaces and real `SmPlane`s, the wire network of `prog_test`
//! `my_test_topology`): the C++ side builds a real `SmBrep` per call, the Rust side
//! imports the graph the shim exports ([`super::topology_edgeuse_p1::import`]), and the
//! ported functions run on both. Edge use arguments are selectors taken modulo the edge
//! use count.
//!
//! `CheckAndSetUVTrimCurve` is given a UV curve built from the imported B-rep (so both
//! sides get the same curve): the selected edge use's current UV trim curve (its own or
//! its mate's) with every control point moved by `(du, dv)` face-domain lengths, or,
//! without one, the line between two points of the face's UV domain.
//!
//! The sector tests, the step-off binormal and `RebuildUVTrimCurve` reach
//! `SmEdgeuse::EvaluateBinormal` / `CreateUVTrimCurve` (unit `topology.edgeuse_p3`) on
//! every non-trivial path: those cases are deferred; their trivial paths (no edge,
//! lamina edges, zero samples, null dereferences) are compared.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::curve::CurveKind;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::geom::object::{NotifyOperation, ObjectRef};
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::NOW;
use solidcore::surface::surface::p2::DropCurveFail;
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::edgeuse::p2 as eu2;
use solidcore::topology::{Brep, EdgeuseId, ObjectHandle};
use solidcore::SolidError;
use solidcore_ffi::units::topology_edgeuse_p1 as ffi1;
use solidcore_ffi::units::topology_edgeuse_p2::{self as ffi, Op};

use super::topology_edgeuse_p1::{
    brep_block_of, gen_brep, gen_curve, harness, import_with_types, path_of, push_curve, sel,
    BrepIn, CrvIn, ImportTypes, FILES,
};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: f64 = -2.0;
/// The shim did not run the op (deferred on the Rust side).
const NOT_RUN: f64 = -3.0;

/// The UV curve given to `CheckAndSetUVTrimCurve`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "src", rename_all = "snake_case")]
enum UvSrc {
    /// The current UV trim curve moved by `(du, dv)` domain lengths; without one the
    /// line from `a` to `b` (fractions of the face's UV domain).
    Current { du: f64, dv: f64, a: [f64; 2], b: [f64; 2] },
    /// A given 2D curve.
    Given { curve: CrvIn },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Tangent { eu: i64, tol_deg: f64, coincidence: bool, ok_make: bool },
    Convex { eu: i64, n: u64, step_off: bool, tol_deg: f64 },
    StepOff {
        eu: i64,
        frac: f64,
        dist: f64,
        center: Option<[f64; 3]>,
        normal: Option<[f64; 3]>,
        ok_make: bool,
    },
    Rebuild { eu: i64 },
    CheckSet { eu: i64, uv: UvSrc, adjust: bool, fail: Option<i64> },
    Notify { eu: i64, operation: u8 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    brep: BrepIn,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

pub(super) fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

impl OpIn {
    fn eu(&self) -> i64 {
        match self {
            Self::Tangent { eu, .. }
            | Self::Convex { eu, .. }
            | Self::StepOff { eu, .. }
            | Self::Rebuild { eu }
            | Self::CheckSet { eu, .. }
            | Self::Notify { eu, .. } => *eu,
        }
    }
}

/// The case's B-rep: the C++ export, the imported Rust B-rep and the selected edge
/// use; `Err(output)` when the B-rep cannot be built or has no edge use.
pub(super) struct Setup {
    pub(super) path: String,
    pub(super) block: Vec<f64>,
    pub(super) brep: Brep,
    pub(super) eu: EdgeuseId,
    pub(super) types: ImportTypes,
}

fn setup(i: &Inputs) -> Result<Result<Setup, Output>, HarnessError> {
    let path = path_of(Some(&i.brep))?;
    let block = brep_block_of(Some(&i.brep), i.validate_only, i.init_gaps)?;
    let ex = ffi1::eval(ffi1::Op::Export, &path, &block)
        .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(c) = ffi1::counts(&ex) else {
        let st = ex.get(1).copied().unwrap_or(0.0);
        return Ok(Err(Output::err(format!("brep_build_status_{st}"))));
    };
    let n_eu = c.get(6).copied().unwrap_or(0);
    let s = sel(i.op.eu(), n_eu);
    if s < 0 {
        return Ok(Err(Output::err("no_entity_of_that_kind")));
    }
    let (brep, types) = import_with_types(&ex)?;
    let eu = EdgeuseId::from_index(u32::try_from(s).map_err(|_| harness("index"))?);
    Ok(Ok(Setup {
        path,
        block,
        brep,
        eu,
        types,
    }))
}

/// A 2D curve as a [`CrvIn`] (`None` for a non-B-spline or one without NURB data).
fn crv_in(c: &SmCurve) -> Option<CrvIn> {
    let CurveKind::BSpline(b) = c.kind() else {
        return None;
    };
    let nurb = b.gw_nurb()?;
    let cpts = nurb
        .cpts()
        .get(..=nurb.n())?
        .iter()
        .map(|q| [q.x, q.y, q.z, q.w])
        .collect();
    let u = nurb.knots().get(..=nurb.m())?.to_vec();
    Some(CrvIn {
        dim: b.dim(),
        cpts,
        p: nurb.degree(),
        u,
    })
}

/// The UV curve of a `CheckSet` case, from the imported B-rep.
fn uv_curve(b: &Brep, eu: EdgeuseId, src: &UvSrc) -> Option<CrvIn> {
    let (u0, v0, du_len, dv_len) = {
        let f = eu1::face(b, eu).ok().flatten();
        match f.and_then(|f| b.face(f).ok()) {
            Some(face) => {
                let d = face.uv_domain;
                (d.min().x, d.min().y, d.max().x - d.min().x, d.max().y - d.min().y)
            }
            None => (0.0, 0.0, 1.0, 1.0),
        }
    };
    match src {
        UvSrc::Given { curve } => Some(curve.clone()),
        UvSrc::Current { du, dv, a, b: bb } => {
            let current = eu1::uv_trim_curve(b, eu).ok().and_then(|(c, _)| c.and_then(crv_in));
            if let Some(mut c) = current {
                let (sx, sy) = (du * du_len, dv * dv_len);
                for q in &mut c.cpts {
                    let k = if q[3] == NOW { 1.0 } else { q[3] };
                    q[0] += sx * k;
                    q[1] += sy * k;
                }
                return Some(c);
            }
            let p0 = [u0 + a[0] * du_len, v0 + a[1] * dv_len];
            let p1 = [u0 + bb[0] * du_len, v0 + bb[1] * dv_len];
            if p0 == p1 || !(du_len > 0.0 && dv_len > 0.0) {
                return None;
            }
            Some(CrvIn {
                dim: 2,
                cpts: vec![[p0[0], p0[1], f64::MAX, NOW], [p1[0], p1[1], f64::MAX, NOW]],
                p: 1,
                u: vec![0.0, 0.0, 1.0, 1.0],
            })
        }
    }
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE` and
/// `SmCrvOnSurf_TYPE` (the import rebuilds a curve on a B-spline surface exactly).
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 3] = [14033, 14010, 14048];

/// The deferral of a `CheckSet` case whose face surface or edge curve the import holds
/// only as its B-spline part (an analytic `SmBSplineSurface` / `SmBSplineCurve`
/// subclass): the Rust `Surface` has no kind for it (the unit named, as in
/// `Surface::read_from_db`), or its exact import needs the B-rep reader
/// (`SmBrep::ReadFromFile`, unit `topology.brep_p7`).
pub(super) fn inexact_geometry(s: &Setup) -> Option<Output> {
    let b = &s.brep;
    let surface_type = eu1::face(b, s.eu)
        .ok()
        .flatten()
        .and_then(|f| s.types.face_surface_types.get(f.index() as usize).copied())
        .unwrap_or(-1);
    let curve_type = eu1::edge(b, s.eu)
        .ok()
        .flatten()
        .and_then(|e| s.types.edge_curve_types.get(e.index() as usize).copied())
        .unwrap_or(-1);
    if surface_type >= 0 && !EXACT_SURFACES.contains(&surface_type) {
        let (unit, symbol) = match surface_type {
            15012 | 15013 => ("surface.cone_p1", "SmCone (no SurfaceKind)"),
            15014 => ("surface.sphere", "SmSphere"),
            15015 => ("surface.torus", "SmTorus (no SurfaceKind)"),
            15030 => ("surface.surf_of_extrusion_p2", "SurfaceKind::SurfOfExtrusion"),
            _ => ("topology.brep_p7", "SmBrep::ReadFromFile"),
        };
        return Some(Output::deferred(unit, symbol));
    }
    if curve_type >= 0 && !EXACT_CURVES.contains(&curve_type) {
        return Some(Output::deferred("topology.brep_p7", "SmBrep::ReadFromFile"));
    }
    None
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let s = match setup(i)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    let mut input = s.block.clone();
    input.push(f64::from(s.eu.index()));
    let op = match &i.op {
        OpIn::Tangent {
            tol_deg,
            coincidence,
            ok_make,
            ..
        } => {
            input.extend([*tol_deg, flag(*coincidence), flag(*ok_make)]);
            Op::Tangent
        }
        OpIn::Convex {
            n,
            step_off,
            tol_deg,
            ..
        } => {
            input.extend([*n as f64, flag(*step_off), *tol_deg]);
            Op::Convex
        }
        OpIn::StepOff {
            frac,
            dist,
            center,
            normal,
            ok_make,
            ..
        } => {
            input.extend([*frac, *dist, flag(center.is_some())]);
            input.extend(center.unwrap_or([0.0; 3]));
            input.push(flag(normal.is_some()));
            input.extend(normal.unwrap_or([0.0; 3]));
            input.push(flag(*ok_make));
            Op::StepOff
        }
        OpIn::Rebuild { .. } => Op::Rebuild,
        OpIn::CheckSet {
            uv, adjust, fail, ..
        } => {
            let Some(c) = uv_curve(&s.brep, s.eu, uv) else {
                return Ok(Output::err("no_uv_curve"));
            };
            input.extend([flag(*adjust), flag(fail.is_some()), fail.unwrap_or(0) as f64]);
            push_curve(&mut input, &c)?;
            Op::CheckSet
        }
        OpIn::Notify { operation, .. } => {
            input.push(f64::from(*operation));
            Op::Notify
        }
    };
    let out = ffi::eval(op, &s.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(output_of(&i.op, body))
}

/// The output of an op body (after the build status), as both sides build it.
fn output_of(op: &OpIn, body: &[f64]) -> Output {
    let first = body.first().copied().unwrap_or(f64::NAN);
    if first == NULL_DEREF {
        return Output::err("null_deref");
    }
    if first == NOT_RUN {
        // only on paths the Rust side defers; compared, it fails the case
        return Output::err("not_run_by_cpp");
    }
    match op {
        OpIn::Tangent { .. } | OpIn::Convex { .. } => Output::new().int("result", first as i64),
        OpIn::CheckSet { .. } => Output::new()
            .int("failed", i64::from(first != 0.0))
            .values("state", body.get(1..).unwrap_or(&[]).to_vec()),
        _ => Output::new().values("state", body.to_vec()),
    }
}

// ---- Rust side --------------------------------------------------------------------------

/// The state of an edge use as the shim's `put_eu_state`.
pub(super) fn put_eu_state(o: &mut Vec<f64>, b: &Brep, eu: Option<EdgeuseId>) -> Result<(), SolidError> {
    let Some(eu) = eu else {
        o.push(-1.0);
        return Ok(());
    };
    o.push(1.0);
    let r = b.edgeuse(eu)?;
    match r.uv_trim_curve.as_ref() {
        None => o.push(-1.0),
        Some(c) => {
            o.push(1.0);
            let ivl = c.natural_interval();
            o.extend([ivl.min(), ivl.max()]);
            for t in [ivl.min(), ivl.mid(), ivl.max()] {
                let p = c
                    .evaluate_point(t)
                    .unwrap_or(solidcore::math::Vec3::UNDEF);
                o.extend([p.x, p.y, p.z]);
            }
        }
    }
    // the curve's owner is the edge use holding it
    o.push(if r.uv_trim_curve.is_some() {
        f64::from(eu.index())
    } else {
        -1.0
    });
    let fail = r.drop_curve_fail.as_deref();
    o.push(flag(fail.is_some()));
    o.push(fail.map_or(0.0, |f| f64::from(f.fail_status)));
    o.push(flag(fail.is_some_and(|f| f.brep.is_some())));
    o.push(match r.vertexuse {
        Some(vu) => flag(b.vertexuse(vu)?.vertex_face_gap.is_init()),
        None => -1.0,
    });
    Ok(())
}

fn gap_flag(b: &Brep, eu: Option<EdgeuseId>) -> Result<f64, SolidError> {
    Ok(match eu {
        Some(eu) => flag(b.edgeuse(eu)?.max_edge_face_gap.is_init()),
        None => -1.0,
    })
}

fn notify_op(code: u8) -> Result<NotifyOperation, HarnessError> {
    NotifyOperation::ALL
        .get(usize::from(code))
        .copied()
        .ok_or_else(|| harness(format!("bad notify operation {code}")))
}

/// The Rust outcome of an op body; `Err` carries a [`SolidError`].
fn run_rust_op(
    op: &OpIn,
    b: &mut Brep,
        eu: EdgeuseId,
) -> Result<Result<Vec<f64>, SolidError>, HarnessError> {
    let k = Kernel::default();
    Ok(match op {
        OpIn::Tangent {
            tol_deg,
            coincidence,
            ok_make,
            ..
        } => eu2::is_tangent_sector(&k, b, eu, *tol_deg, *coincidence, *ok_make)
            .map(|r| vec![flag(r)]),
        OpIn::Convex {
            n,
            step_off,
            tol_deg,
            ..
        } => eu2::is_convex_radial_sector(&Kernel::default(), b, eu, *n, *step_off, *tol_deg).map(|r| vec![flag(r)]),
        OpIn::StepOff {
            frac,
            dist,
            center,
            normal,
            ok_make,
            ..
        } => {
            let opts = eu2::StepOffOptions {
                center_point: center.map(solidcore::math::Vec3::from_array),
                plane_normal: normal.map(solidcore::math::Vec3::from_array),
                ok_to_make_uv_trim_curve: *ok_make,
            };
            let t = match eu1::edge(b, eu) {
                Ok(Some(e)) => b.edge(e).map(|r| r.interval.evaluate(*frac)),
                Ok(None) => Ok(*frac),
                Err(e) => Err(e),
            };
            // the out-params are compared whatever the status, like the shim's: the record
            // starts at the shim's zeros and keeps what a failing call wrote
            let mut out = eu2::BinormalEval {
                point: solidcore::math::Vec3::ZERO,
                binormal: solidcore::math::Vec3::ZERO,
                tangent: solidcore::math::Vec3::ZERO,
                face_normal: solidcore::math::Vec3::ZERO,
                uv: solidcore::math::Point2::new(0.0, 0.0),
            };
            let status = t.and_then(|t| {
                eu2::evaluate_binormal_step_off_into(
                    &Kernel::default(),
                    b,
                    eu,
                    t,
                    *dist,
                    &opts,
                    &mut out,
                )
            });
            let code = match status {
                Ok(()) => Ok(0.0),
                Err(SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
                Err(e) => Err(e),
            };
            code.map(|c| {
                let mut v = vec![c];
                for p in [out.point, out.binormal, out.tangent, out.face_normal] {
                    v.extend([p.x, p.y, p.z]);
                }
                v.extend([out.uv.x, out.uv.y]);
                v
            })
        }
        OpIn::Rebuild { .. } => {
            let status = match eu2::rebuild_uv_trim_curve(&k, b, eu) {
                Ok(()) => Ok(0.0),
                Err(SolidError::Upstream { code, .. }) => Ok(f64::from(code)),
                Err(e) => Err(e),
            };
            status.and_then(|s| {
                let mut v = vec![s];
                put_eu_state(&mut v, b, Some(eu))?;
                Ok(v)
            })
        }
        OpIn::CheckSet {
            uv, adjust, fail, ..
        } => {
            let Some(c) = uv_curve(b, eu, uv) else {
                return Err(harness("no_uv_curve reached the Rust side"));
            };
            let curve = SmCurve::from(super::topology_edgeuse_p1::bspline_of(&c)?);
            let mut rec = fail.map(|s| DropCurveFail::<ObjectHandle> {
                fail_status: i32::try_from(s).unwrap_or(0),
                ..DropCurveFail::default()
            });
            let mate = eu1::mate(b, eu);
            let r = eu2::check_and_set_uv_trim_curve(&k, b, eu, curve, *adjust, rec.as_mut());
            match (r, mate) {
                (Err(e), _) if e.is_unported() || matches!(e, SolidError::InvalidInput { .. }) => {
                    Err(e)
                }
                (_, Err(e)) => Err(e),
                (r, Ok(mate)) => {
                    let (failed, dmax, replaced) = match r {
                        Ok(o) => (0.0, o.max_dist_to_surf.unwrap_or(-1.0), flag(o.trim_replaced)),
                        Err(_) => (1.0, -1.0, 0.0),
                    };
                    let state = (|| -> Result<Vec<f64>, SolidError> {
                        let mut v = vec![failed, dmax, replaced];
                        put_eu_state(&mut v, b, Some(eu))?;
                        put_eu_state(&mut v, b, mate)?;
                        let tol = match eu1::edge(b, eu)? {
                            Some(e) => b.edge(e)?.tolerance,
                            None => -1.0,
                        };
                        v.push(tol);
                        v.push(flag(rec.as_ref().is_some_and(|f| f.brep.is_some())));
                        Ok(v)
                    })();
                    state
                }
            }
        }
        OpIn::Notify { operation, .. } => {
            let operation = notify_op(*operation)?;
            let mate = eu1::mate(b, eu).ok().flatten();
            let mut records = BTreeMap::new();
            let mut attributes = AttributeStore::default();
            let this = ObjectRef {
                type_code: eu1::TYPE,
                key: ObjectKey(u64::from(eu.index()) + 1),
            };
            eu2::notify(
                b,
                eu,
                &mut records,
                &mut attributes,
                None,
                operation,
                this,
                [None, None, None],
            )
            .and_then(|()| {
                let mut v = vec![0.0];
                put_eu_state(&mut v, b, Some(eu))?;
                put_eu_state(&mut v, b, mate)?;
                v.push(gap_flag(b, Some(eu))?);
                v.push(gap_flag(b, mate)?);
                Ok(v)
            })
        }
    })
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut s = match setup(i)? {
        Ok(s) => s,
        Err(o) => return Ok(o),
    };
    if let OpIn::CheckSet { uv, .. } = &i.op {
        if uv_curve(&s.brep, s.eu, uv).is_none() {
            return Ok(Output::err("no_uv_curve"));
        }
        if let Some(o) = inexact_geometry(&s) {
            return Ok(o);
        }
    }
    match run_rust_op(&i.op, &mut s.brep, s.eu)? {
        Ok(body) => Ok(output_of(&i.op, &body)),
        Err(e) => {
            if let Some(o) = Output::unported(&e) {
                return Ok(o);
            }
            match e {
                SolidError::InvalidInput { .. } => Ok(Output::err("null_deref")),
                e => Err(harness(format!("rust: {e}"))),
            }
        }
    }
}

// ---- generation -------------------------------------------------------------------------

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

fn gen_op(rng: &mut Rng) -> OpIn {
    match rng.below(20) {
        0..=2 => OpIn::Tangent {
            eu: gen_sel(rng),
            tol_deg: *rng.choose(&[0.0, 1.0, 5.0, 45.0, 89.0]).unwrap_or(&1.0),
            coincidence: rng.chance(0.3),
            ok_make: rng.chance(0.5),
        },
        3..=4 => OpIn::Convex {
            eu: gen_sel(rng),
            n: *rng.choose(&[0, 0, 1, 4, 10]).unwrap_or(&0),
            step_off: rng.chance(0.5),
            tol_deg: *rng.choose(&[0.0, 0.0, 2.0, 10.0]).unwrap_or(&0.0),
        },
        5 => OpIn::StepOff {
            eu: gen_sel(rng),
            frac: rng.uniform(-0.1, 1.1),
            dist: *rng.choose(&[0.0, 1e-12, 1e-3, 0.1, 1.0]).unwrap_or(&0.1),
            center: rng.chance(0.2).then(|| rng.point3(-2.0, 2.0)),
            normal: rng.chance(0.2).then(|| rng.point3(-1.0, 1.0)),
            ok_make: rng.chance(0.7),
        },
        6 => OpIn::Rebuild { eu: gen_sel(rng) },
        7..=14 => {
            let uv = if rng.chance(0.85) {
                let shift = *rng
                    .choose(&[0.0, 0.0, 1e-12, 1e-9, 1e-7, 1e-5, 1e-3, 0.05, 0.3])
                    .unwrap_or(&0.0);
                UvSrc::Current {
                    du: shift * rng.uniform(-1.0, 1.0),
                    dv: shift * rng.uniform(-1.0, 1.0),
                    a: [rng.unit(), rng.unit()],
                    b: [rng.unit(), rng.unit()],
                }
            } else {
                let mut c = gen_curve(rng, 2, 1.0);
                // keep it inside a unit domain's neighbourhood
                for q in &mut c.cpts {
                    let k = if q[3] == NOW { 1.0 } else { q[3] };
                    q[0] = (q[0] / k).rem_euclid(1.0) * k;
                    q[1] = (q[1] / k).rem_euclid(1.0) * k;
                }
                UvSrc::Given { curve: c }
            };
            OpIn::CheckSet {
                eu: gen_sel(rng),
                uv,
                adjust: rng.chance(0.3),
                fail: rng.chance(0.3).then(|| rng.range_i64(0, 1100)),
            }
        }
        _ => {
            let ops: Vec<u8> = (0..=16).filter(|&o| o != 14).collect();
            OpIn::Notify {
                eu: gen_sel(rng),
                operation: *rng.choose(&ops).unwrap_or(&1),
            }
        }
    }
}

/// B-reps of generated cases: mostly upstream files (their faces carry real UV trim
/// curves), plus the constructed kinds of `topology.edgeuse_p1`.
fn gen_brep_p2(rng: &mut Rng) -> BrepIn {
    if rng.chance(0.5) {
        BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"Mouse.smb")).to_string(),
            rebuild: rng.chance(0.25),
        }
    } else {
        gen_brep(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.edgeuse_p2"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = gen_op(rng);
        let i = Inputs {
            brep: gen_brep_p2(rng),
            validate_only: rng.chance(0.15),
            init_gaps: rng.chance(0.5),
            op,
        };
        serde_json::to_value(i).ok()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn inputs_round_trip() {
        let mut rng = Rng::new(7);
        for _ in 0..50 {
            let op = gen_op(&mut rng);
            let i = Inputs {
                brep: BrepIn::Network,
                validate_only: false,
                init_gaps: true,
                op,
            };
            let v = serde_json::to_value(&i).unwrap();
            let back: Inputs = serde_json::from_value(v).unwrap();
            assert_eq!(back.op.eu(), i.op.eu());
        }
    }
}
