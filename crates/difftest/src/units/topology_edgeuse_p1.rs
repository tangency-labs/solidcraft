//! Runner for unit `topology.edgeuse_p1`. C++: `solidcore_ffi::units::topology_edgeuse_p1`.
//! Rust: `solidcore::topology::edgeuse::p1`.
//!
//! **B-rep ops.** The C++ side builds a real `SmBrep` per call: read from an upstream
//! `.smb` test file (optionally rebuilding its UV trim curves), a wire edge from a
//! B-spline curve (`CreateWireEdgeFromCurve`), a face from a B-spline surface or a plane
//! (`CreateFaceFromSurface`), or the wire network of `prog_test` `my_test_topology`. There
//! is no Rust B-rep reader or builder yet (units `topology.brep_*`), so the Rust side
//! imports the graph the shim exports in traversal order (raw members: list owners,
//! list order, loop neighbours, orientations, UV trim curves, cached gap states,
//! tolerances, face UV domains, geometry as B-splines; an `SmPlane` face surface as the
//! exact [`Plane`] (origin, axes, scales, analytic domain), an `SmSurfOfRevolution` face
//! surface as the exact surface of revolution (its database record, read by the ported
//! `ReadFromDB`: [`crate::util::read_exact_surface`]), an `SmLine` edge curve as
//! the exact [`Line`]) into a [`Brep`] and runs the ported functions on it; their
//! results are compared as traversal indices, and the gaps the gap ops compute by
//! value. A case that evaluates a face surface or edge curve the import holds only as
//! its B-spline part (an analytic subclass: cone, sphere, torus, extrusion,
//! circle, ...) is deferred ([`inexact_geometry`]): the gap calculations and the 3D
//! drop branch of `GetStartPointUVLoopGap`. The skeleton fields no ported function
//! reads (`Loop::edges`, `Shell::faces`, `Face::loops`) are filled minimally.
//!
//! Index arguments are selectors taken modulo the entity count (so generated cases
//! need no counts); `-1` selects null.
//!
//! **Geometry ops** (`SmFindStepOffENFO::Evaluate`, `sm_CalcDistAndDev`,
//! `sm_AreSeamCurves`, verbatim copies in the shim) run on B-spline surfaces and planes
//! (real `SmPlane`s) and B-spline curves.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::p2::{BSPLINE_CURVE_TYPE, CRV_ON_SURF_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::solution_array::{ObjectKind, Solution};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::math::UNDEF_DOUBLE;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::DropCurveFail;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::edgeuse::p1 as eu1;
use solidcore::topology::shell::{ShellMembers, ShellType};
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId,
    Loop, LoopId, Loopuse, LoopuseId, LoopuseStart, ObjectHandle, Region, RegionId, Shell,
    ShellId, Vertex, VertexId, Vertexuse, VertexuseId, VertexuseOwner,
};
use solidcore::{EntityRef, SolidError};
use solidcore::geom::attribute::ObjectKey;
use solidcore_ffi::units::topology_edgeuse_p1::{self as ffi, Op};

use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
pub struct Runner;

pub(crate) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// `SmTopoTypes.h` codes.
const SHELL_TYPE: i64 = 16013;
const LOOPUSE_TYPE: i64 = 16004;
const EDGEUSE_TYPE: i64 = 16005;
const VERTEXUSE_TYPE: i64 = 16006;

/// The type codes `IsKindOf` is asked about (as the shim's `KIND_TYPES`).
const KIND_TYPES: [i64; 9] = [16005, 16022, 16001, 16010, 12060, 11001, 16013, 16004, 16999];

/// "Upstream would dereference null" in an output slot.
const NULL_DEREF: i64 = -2;

// ---- inputs -----------------------------------------------------------------------------

/// A B-spline curve: `dim`, control points `[x y z w]` (`w = NOW` non-rational, `z =
/// NOZ` 2D), degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CrvIn {
    pub(crate) dim: u64,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// A B-spline surface.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BssIn {
    #[serde(default)]
    pub(crate) oob: bool,
    #[serde(default)]
    pub(crate) form: u8,
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SrfIn {
    Bspline(BssIn),
    Plane { o: [f64; 3], n: [f64; 3] },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub(crate) enum BrepIn {
    /// An upstream test file, relative to `upstream/TestFiles/pt_TestFiles`.
    File {
        file: String,
        #[serde(default)]
        rebuild: bool,
    },
    Wire { curve: CrvIn, t0: f64, t1: f64 },
    Face { surface: SrfIn, dom: [f64; 4] },
    /// A face as [`BrepIn::Face`] whose edges are `SmCrvOnSurf`s of the face's (B-spline)
    /// surface over their UV trim curves: the `CrvOnSurf` path of `CreateUVTrimCurve`.
    CrvOnSurfFace { surface: SrfIn, dom: [f64; 4] },
    Network,
    Vertices { points: Vec<[f64; 3]> },
    /// An optional face and `networks` copies of the wire network in one B-rep, so
    /// face-use shells and wire shells coexist (native constructors only).
    Mixed {
        #[serde(default)]
        face: Option<FaceIn>,
        networks: usize,
    },
    /// The closed box solid of `SmApiCreateBox` (planes and lines; fixtures only).
    Box { origin: [f64; 3], size: [f64; 3] },
    /// Isolated vertices `[x, y, z, tol]` and wire line segments `[x0, y0, z0, x1, y1,
    /// z1, tol]`, each set to its tolerance (fixtures and other units' generators).
    Points {
        vertices: Vec<[f64; 4]>,
        lines: Vec<[f64; 7]>,
    },
}

/// The face of a [`BrepIn::Mixed`] B-rep (as [`BrepIn::Face`]).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct FaceIn {
    surface: SrfIn,
    dom: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    EuQuery { eu: i64, vertex: i64 },
    Connected { eu: i64, ty: i64, index: i64 },
    FuQuery { fu: i64, tgt_only: bool, has_loops: bool, loops: Vec<i64> },
    Collect { fu: i64, threshold: Option<f64> },
    UvLoopGap { eu: i64 },
    /// `kind`: 0 NULL, 1 the attached curve, 2 a new curve, 3 `DeleteUVTrimCurve`, 4
    /// `SetMemberUVTrimCurve` with a new curve.
    SetUv {
        eu: i64,
        kind: u8,
        curve: Option<CrvIn>,
        dmax: f64,
        delete: bool,
        fail: Option<i64>,
    },
    /// `which`: 0 `GetMaxEdgeFaceGap`, 1 `GetEdgeCCWEdgeGap`, 2 `CalcMaxEdgeFaceGap`, 3
    /// `CalcEdgeCCWEdgeGap`.
    Gaps { eu: i64, which: u8, flag: bool, want_out: bool },
    SetOrient { eu: i64, orient: u8 },
    StepOff { surface: SrfIn, dist: f64, p: [f64; 3], n: [f64; 3], x: [f64; 2], jac: bool },
    DistDev { surface: SrfIn, p: [f64; 3], uv: [f64; 2], value: f64, obj: u8, want_dev: bool },
    Seam { curves: Vec<CrvIn>, dom: [f64; 2], called: bool },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    brep: Option<BrepIn>,
    #[serde(default)]
    validate_only: bool,
    #[serde(default)]
    init_gaps: bool,
    #[serde(flatten)]
    op: OpIn,
}

// ---- encoding ---------------------------------------------------------------------------

pub(crate) fn push_curve(v: &mut Vec<f64>, c: &CrvIn) -> Result<(), HarnessError> {
    let n = c.cpts.len().checked_sub(1).ok_or_else(|| harness("curve without points"))?;
    let m = c.u.len().checked_sub(1).ok_or_else(|| harness("curve without knots"))?;
    v.extend([c.dim as f64, n as f64, c.p as f64, m as f64]);
    for p in &c.cpts {
        v.extend(p);
    }
    v.extend(&c.u);
    Ok(())
}

pub(crate) fn push_surface(v: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Plane { o, n } => {
            v.push(1.0);
            v.extend(o);
            v.extend(n);
        }
        SrfIn::Bspline(b) => {
            let n = b.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
            let m = b
                .net
                .first()
                .and_then(|r| r.len().checked_sub(1))
                .ok_or_else(|| harness("empty net row"))?;
            if b.net.iter().any(|r| r.len() != m + 1) {
                return Err(harness("ragged net"));
            }
            if b.u.is_empty() || b.v.is_empty() {
                return Err(harness("empty knot vector"));
            }
            v.extend([
                0.0,
                f64::from(u8::from(b.oob)),
                f64::from(b.form),
                n as f64,
                m as f64,
                b.p as f64,
                b.q as f64,
                b.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("empty u knots"))? as f64,
                b.v.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("empty v knots"))? as f64,
            ]);
            for row in &b.net {
                for p in row {
                    v.extend(p);
                }
            }
            v.extend(&b.u);
            v.extend(&b.v);
        }
    }
    Ok(())
}

pub(crate) fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// The `.smb` path of a B-rep input ("" when it is not a file).
pub(crate) fn path_of(b: Option<&BrepIn>) -> Result<String, HarnessError> {
    match b {
        Some(BrepIn::File { file, .. }) => {
            let p = crate::fixtures::upstream_test_file(&crate::case::repo_root(), file)?;
            Ok(p.to_string_lossy().into_owned())
        }
        _ => Ok(String::new()),
    }
}

/// The B-rep block.
fn brep_block(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    brep_block_of(i.brep.as_ref(), i.validate_only, i.init_gaps)
}

/// The B-rep block of `brep` (shared with unit `topology.edgeuse_p2`).
pub(crate) fn brep_block_of(
    brep: Option<&BrepIn>,
    validate_only: bool,
    init_gaps: bool,
) -> Result<Vec<f64>, HarnessError> {
    let b = brep.ok_or_else(|| harness("B-rep op without brep"))?;
    let mut v = Vec::new();
    match b {
        BrepIn::File { rebuild, .. } => {
            v.push(0.0);
            v.push(flag(*rebuild));
        }
        BrepIn::Wire { curve, t0, t1 } => {
            v.push(1.0);
            push_curve(&mut v, curve)?;
            v.extend([*t0, *t1]);
        }
        BrepIn::Face { surface, dom } => {
            v.push(2.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::CrvOnSurfFace { surface, dom } => {
            v.push(6.0);
            push_surface(&mut v, surface)?;
            v.extend(dom);
        }
        BrepIn::Network => v.push(3.0),
        BrepIn::Vertices { points } => {
            v.extend([4.0, points.len() as f64]);
            for point in points { v.extend(point); }
        }
        BrepIn::Mixed { face, networks } => {
            v.extend([5.0, flag(face.is_some())]);
            if let Some(face) = face {
                push_surface(&mut v, &face.surface)?;
                v.extend(face.dom);
            }
            v.push(*networks as f64);
        }
        BrepIn::Box { origin, size } => {
            v.push(7.0);
            v.extend(origin);
            v.extend(size);
        }
        BrepIn::Points { vertices, lines } => {
            v.extend([8.0, vertices.len() as f64]);
            for p in vertices {
                v.extend(p);
            }
            v.push(lines.len() as f64);
            for l in lines {
                v.extend(l);
            }
        }
    }
    v.push(flag(validate_only));
    v.push(flag(init_gaps));
    Ok(v)
}

/// Resolves a selector modulo `n` (`-1` and an empty kind select null).
pub(crate) fn sel(s: i64, n: usize) -> i64 {
    if s < 0 || n == 0 {
        -1
    } else {
        (s as u64 % n as u64) as i64
    }
}

/// The selectors of an op resolved against the B-rep's counts; `None` when the op
/// needs an edge use or face use and the B-rep has none.
fn resolve(op: &OpIn, c: [usize; 10]) -> Option<OpIn> {
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    let needs_fu = matches!(op, OpIn::FuQuery { .. } | OpIn::Collect { .. });
    if (needs_fu && n_fu == 0) || (!needs_fu && is_brep_op(op) && n_eu == 0) {
        return None;
    }
    let eu = |s: i64| sel(s, n_eu).max(0);
    Some(match op.clone() {
        OpIn::EuQuery { eu: e, vertex } => OpIn::EuQuery {
            eu: eu(e),
            vertex: sel(vertex, n_v),
        },
        OpIn::Connected { eu: e, ty, index } => {
            let n = match ty {
                16012 => n_r,
                16013 => n_s,
                16022 => n_fu,
                16002 => n_f,
                16004 => n_lu,
                16003 => n_l,
                16005 => n_eu,
                16016 => n_e,
                16006 => n_vu,
                16021 => n_v,
                _ => 0,
            };
            OpIn::Connected {
                eu: eu(e),
                ty,
                index: if n == 0 { index.min(-1) } else { sel(index, n) },
            }
        }
        OpIn::FuQuery {
            fu,
            tgt_only,
            has_loops,
            loops,
        } => OpIn::FuQuery {
            fu: sel(fu, n_fu).max(0),
            tgt_only,
            has_loops,
            loops: if n_l == 0 {
                Vec::new()
            } else {
                loops.iter().map(|&l| sel(l, n_l).max(0)).collect()
            },
        },
        OpIn::Collect { fu, threshold } => OpIn::Collect {
            fu: sel(fu, n_fu).max(0),
            threshold,
        },
        OpIn::UvLoopGap { eu: e } => OpIn::UvLoopGap { eu: eu(e) },
        OpIn::SetUv {
            eu: e,
            kind,
            curve,
            dmax,
            delete,
            fail,
        } => OpIn::SetUv {
            eu: eu(e),
            kind,
            curve,
            dmax,
            delete,
            fail,
        },
        OpIn::Gaps {
            eu: e,
            which,
            flag,
            want_out,
        } => OpIn::Gaps {
            eu: eu(e),
            which,
            flag,
            want_out,
        },
        OpIn::SetOrient { eu: e, orient } => OpIn::SetOrient { eu: eu(e), orient },
        other => other,
    })
}

/// The output when the B-rep has no entity of the kind an op needs.
fn no_entity() -> Output {
    Output::err("no_entity_of_that_kind")
}

/// The op arguments after the B-rep block (resolved selectors).
fn op_args(op: &OpIn) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    let code = match op {
        OpIn::EuQuery { eu, vertex } => {
            v.extend([*eu as f64, *vertex as f64]);
            Op::EuQuery
        }
        OpIn::Connected { eu, ty, index } => {
            v.extend([*eu as f64, *ty as f64, *index as f64]);
            Op::Connected
        }
        OpIn::FuQuery {
            fu,
            tgt_only,
            has_loops,
            loops,
        } => {
            v.extend([*fu as f64, flag(*tgt_only), flag(*has_loops), loops.len() as f64]);
            v.extend(loops.iter().map(|&l| l as f64));
            Op::FuQuery
        }
        OpIn::Collect { fu, threshold } => {
            v.extend([*fu as f64, flag(threshold.is_some()), threshold.unwrap_or(0.0)]);
            Op::Collect
        }
        OpIn::UvLoopGap { eu } => {
            v.push(*eu as f64);
            Op::UvLoopGap
        }
        OpIn::SetUv {
            eu,
            kind,
            curve,
            dmax,
            delete,
            fail,
        } => {
            v.extend([*eu as f64, f64::from(*kind)]);
            if *kind == 2 || *kind == 4 {
                push_curve(&mut v, curve.as_ref().ok_or_else(|| harness("set_uv: no curve"))?)?;
            }
            v.extend([*dmax, flag(*delete), flag(fail.is_some()), fail.unwrap_or(0) as f64]);
            Op::SetUv
        }
        OpIn::Gaps {
            eu,
            which,
            flag: f,
            want_out,
        } => {
            v.extend([*eu as f64, f64::from(*which), flag(*f), flag(*want_out)]);
            Op::Gaps
        }
        OpIn::SetOrient { eu, orient } => {
            v.extend([*eu as f64, f64::from(*orient)]);
            Op::SetOrient
        }
        OpIn::StepOff {
            surface,
            dist,
            p,
            n,
            x,
            jac,
        } => {
            push_surface(&mut v, surface)?;
            v.push(*dist);
            v.extend(p);
            v.extend(n);
            v.extend(x);
            v.push(flag(*jac));
            Op::StepOff
        }
        OpIn::DistDev {
            surface,
            p,
            uv,
            value,
            obj,
            want_dev,
        } => {
            push_surface(&mut v, surface)?;
            v.extend(p);
            v.extend(uv);
            v.push(*value);
            v.push(f64::from(*obj));
            v.push(flag(*want_dev));
            Op::DistDev
        }
        OpIn::Seam {
            curves,
            dom,
            called,
        } => {
            v.push(curves.len() as f64);
            for c in curves {
                push_curve(&mut v, c)?;
            }
            v.extend(dom);
            v.push(flag(*called));
            Op::Seam
        }
    };
    Ok((code, v))
}

fn is_brep_op(op: &OpIn) -> bool {
    !matches!(op, OpIn::StepOff { .. } | OpIn::DistDev { .. } | OpIn::Seam { .. })
}

/// Reuse the native B-rep fixture encoding without duplicating its geometry
/// constructors or input validation. Shell cases supply a B-rep description.
pub(crate) fn fixture_input(value: Value) -> Result<(String, Vec<f64>), HarnessError> {
    let brep: BrepIn = serde_json::from_value(value)
        .map_err(|e| harness(format!("topology fixture: {e}")))?;
    let inputs = Inputs { brep: Some(brep), validate_only: false, init_gaps: false,
        op: OpIn::SetOrient { eu: 0, orient: 0 } };
    Ok((path_of(inputs.brep.as_ref())?, brep_block(&inputs)?))
}

/// The C++ export of the case's B-rep.
fn export(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let path = path_of(i.brep.as_ref())?;
    let block = brep_block(i)?;
    crate::export_memo::export("topology_edgeuse_p1", &[&path], &block, || {
        ffi::eval(Op::Export, &path, &block)
    })
    .map_err(|e| harness(format!("c++ export: {e}")))
}

/// A build failure as an operation error.
fn build_failure(export: &[f64]) -> Output {
    Output::err(format!("brep_build_status_{}", export.get(1).copied().unwrap_or(0.0)))
}

fn ints(v: &[f64]) -> Vec<i64> {
    v.iter().map(|&x| x as i64).collect()
}

// ---- C++ side ---------------------------------------------------------------------------

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if !is_brep_op(&i.op) {
        let (op, args) = op_args(&i.op)?;
        let out = ffi::eval(op, "", &args).map_err(|e| harness(format!("c++: {e}")))?;
        return Ok(geom_output(&i.op, &out));
    }
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let Some(op) = resolve(&i.op, c) else {
        return Ok(no_entity());
    };
    let (code, args) = op_args(&op)?;
    let mut input = brep_block(i)?;
    input.extend(args);
    let path = path_of(i.brep.as_ref())?;
    let out = ffi::eval(code, &path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    let body = out.get(1..).ok_or_else(|| harness("c++: empty output"))?;
    Ok(brep_output(&op, body))
}

/// The C++ output of a geometry op, as the Rust side builds it.
fn geom_output(op: &OpIn, out: &[f64]) -> Output {
    match op {
        OpIn::StepOff { .. } => {
            let st = out.first().copied().unwrap_or(-1.0);
            Output::new()
                .int("failed", i64::from(st != 0.0))
                .int("found", out.get(1).copied().unwrap_or(-1.0) as i64)
                .values("fj", out.get(2..).unwrap_or(&[]).to_vec())
        }
        OpIn::DistDev { .. } => Output::new().values("dist_dev", out.to_vec()),
        _ => Output::new().ints("seam", ints(out)),
    }
}

/// The C++ output of a B-rep op (after the build status), as the Rust side builds it.
fn brep_output(op: &OpIn, body: &[f64]) -> Output {
    match op {
        OpIn::EuQuery { .. } => {
            let mut q = ints(body);
            let tol = if q.len() > 15 { q.remove(15) as f64 } else { 0.0 };
            let tol = body.get(15).copied().unwrap_or(tol);
            Output::new().ints("q", q).values("tol", vec![tol])
        }
        OpIn::FuQuery { .. } => {
            let head = 6 + KIND_TYPES.len();
            let q = ints(body.get(..head).unwrap_or(body));
            let mut lists = Vec::new();
            let mut at = head;
            while let Some(&n) = body.get(at) {
                let n = n as usize;
                lists.push(ints(body.get(at + 1..at + 1 + n).unwrap_or(&[])));
                at += 1 + n;
            }
            Output::new()
                .ints("q", q)
                .topology(crate::output::Topology::new().adjacency("lists", lists))
        }
        OpIn::Collect { .. } => {
            let st = body.first().copied().unwrap_or(-1.0) as i64;
            let list = ints(body.get(2..).unwrap_or(&[]));
            Output::new().int("status", st).ints("list", list)
        }
        OpIn::UvLoopGap { .. } => Output::new().values("gap", body.to_vec()),
        OpIn::SetUv { .. } | OpIn::Gaps { .. } => Output::new().values("state", body.to_vec()),
        _ => Output::new().ints("q", ints(body)),
    }
}

// ---- import -----------------------------------------------------------------------------

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = self
            .v
            .get(self.at)
            .copied()
            .ok_or_else(|| harness("export too short"))?;
        self.at += 1;
        Ok(x)
    }
    fn idx(&mut self) -> Result<i64, HarnessError> {
        Ok(self.num()? as i64)
    }
    fn usize(&mut self) -> Result<usize, HarnessError> {
        usize::try_from(self.idx()?).map_err(|_| harness("negative count"))
    }
    fn list(&mut self) -> Result<Vec<i64>, HarnessError> {
        let n = self.usize()?;
        (0..n).map(|_| self.idx()).collect()
    }
}

fn opt<T>(i: i64, f: impl Fn(u32) -> T) -> Result<Option<T>, HarnessError> {
    match i {
        -1 => Ok(None),
        x if x >= 0 => Ok(Some(f(u32::try_from(x).map_err(|_| harness("index overflow"))?))),
        x => Err(harness(format!("unregistered pointer ({x}) in export"))),
    }
}

fn orient_of(code: i64) -> Result<OrientType, HarnessError> {
    match code {
        0 => Ok(OrientType::Unknown),
        1 => Ok(OrientType::Same),
        2 => Ok(OrientType::Opposite),
        x => Err(harness(format!("bad orientation {x}"))),
    }
}

fn orient_code(o: OrientType) -> i64 {
    match o {
        OrientType::Unknown => 0,
        OrientType::Same => 1,
        OrientType::Opposite => 2,
    }
}

/// An exported curve block (`None` for `-1`). A B-spline or line is rebuilt exactly;
/// another `SmBSplineCurve` kind is an error for a UV trim curve (`uv`) and its B-spline
/// part for an edge curve (edge curves are only evaluated on deferred paths).
fn read_curve(rd: &mut Rd<'_>, uv: bool) -> Result<Option<SmCurve>, HarnessError> {
    let ty = rd.num()?;
    if ty == -1.0 {
        return Ok(None);
    }
    if ty < 0.0 {
        return Err(harness("export: a curve is not an SmBSplineCurve"));
    }
    let ty = ty as i64;
    if ty == CRV_ON_SURF_TYPE {
        // an SmCrvOnSurf: the UV curve block, then the (B-spline) base surface block
        let uv_curve = read_curve(rd, true)?
            .ok_or_else(|| harness("export: CrvOnSurf without UV curve"))?;
        let surface = read_surface(rd)?
            .ok_or_else(|| harness("export: CrvOnSurf without surface"))?;
        let SurfaceKind::BSpline(base) = surface.kind() else {
            return Err(harness("export: CrvOnSurf base is not a B-spline surface"));
        };
        let k = Kernel::default();
        let cos = CrvOnSurf::new(&k, uv_curve, base.clone(), None, 1)
            .map_err(|e| harness(format!("export CrvOnSurf: {e}")))?;
        return Ok(Some(SmCurve::from(cos)));
    }
    let dim = rd.num()? as u64;
    let has_nurb = rd.num()? != 0.0;
    let base = if has_nurb {
        let n = rd.usize()?;
        let p = rd.usize()?;
        let m = rd.usize()?;
        let mut pw = Vec::new();
        for _ in 0..=n {
            pw.push(CPoint::new(rd.num()?, rd.num()?, rd.num()?, rd.num()?));
        }
        let mut u = Vec::new();
        for _ in 0..=m {
            u.push(rd.num()?);
        }
        let cur = Curve::new(pw, p, u).map_err(|e| harness(format!("export curve: {e}")))?;
        BSplineCurve::from_nurb(cur, dim, false)
    } else {
        BSplineCurve::new(dim)
    };
    if ty == LINE_TYPE {
        let point = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let vector = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let a = rd.num()?;
        let b = rd.num()?;
        let scale = rd.num()?;
        let inside_out = rd.num()? != 0.0;
        let dom = Extent1d::from_raw(a, b);
        return Ok(Some(SmCurve::from(Line::from_parts(
            base, point, vector, dom, scale, inside_out,
        ))));
    }
    if ty != BSPLINE_CURVE_TYPE && uv {
        return Err(harness(format!("export: UV trim curve of type {ty}")));
    }
    Ok(Some(SmCurve::from(base)))
}

fn read_surface(rd: &mut Rd<'_>) -> Result<Option<Surface>, HarnessError> {
    let kind = rd.num()?;
    if kind == -1.0 {
        return Ok(None);
    }
    if kind == 1.0 {
        // an SmPlane: origin, axes, scales, analytic domain (exact)
        let o = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let x = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let y = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let sc = solidcore::geom::vector2d::Vec2::new(rd.num()?, rd.num()?);
        let d = Extent2d::from_bounds(rd.num()?, rd.num()?, rd.num()?, rd.num()?);
        return Ok(Some(Surface::from(Plane::new(o, x, y, sc, &d))));
    }
    if kind != 0.0 {
        return Err(harness("export: a surface is neither a plane nor a B-spline"));
    }
    let oob = rd.num()? != 0.0;
    let form = rd.num()?;
    let n = rd.usize()?;
    let m = rd.usize()?;
    let p = rd.usize()?;
    let q = rd.usize()?;
    let r = rd.usize()?;
    let s = rd.usize()?;
    let mut net = Vec::new();
    for _ in 0..=n {
        let mut row = Vec::new();
        for _ in 0..=m {
            row.push(CPoint::new(rd.num()?, rd.num()?, rd.num()?, rd.num()?));
        }
        net.push(row);
    }
    let u = (0..=r).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..=s).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
    let nl = NlSurface::new(net, p, q, u, v).map_err(|e| harness(format!("export surface: {e}")))?;
    let mut b = BSplineSurface::from_nurb(nl, false);
    *b.out_of_bounds_enabled_mut() = oob;
    let form = BSplineSurfaceForm::from_code(form as u64).ok_or_else(|| harness("bad surface form"))?;
    b.set_b_spline_surface_form(form);
    Ok(Some(Surface::from(b)))
}

/// What the import cannot hold exactly: the upstream type codes of the face surfaces
/// and edge curves (`-1` none), by export index. A surface other than an
/// `SmBSplineSurface`, an `SmPlane` or an exactly exported analytic kind (an
/// `SmSurfOfRevolution`, see [`crate::util::read_exact_surface`]) is imported as its
/// B-spline part, and an edge curve other than an `SmBSplineCurve` or `SmLine` likewise.
#[derive(Debug, Clone, Default)]
pub(crate) struct ImportTypes {
    /// `GetSurface()->GetType()` of each face.
    pub(crate) face_surface_types: Vec<i64>,
    /// Whether each face's Rust surface is the upstream kind (or the face has none).
    pub(crate) face_exact: Vec<bool>,
    /// `GetCurve()->GetType()` of each edge.
    pub(crate) edge_curve_types: Vec<i64>,
}

impl ImportTypes {
    /// The upstream surface type code of face `f` when the import holds that surface
    /// only as its B-spline part (`None`: imported exactly, no surface, or no face `f`).
    pub(crate) fn inexact_surface(&self, f: usize) -> Option<i64> {
        let t = *self.face_surface_types.get(f)?;
        (t >= 0 && !self.face_exact.get(f).copied().unwrap_or(false)).then_some(t)
    }
}

/// The upstream class of an `SmBSplineSurface` subclass type code, for deferral symbols.
pub(crate) const fn surface_class_name(t: i64) -> &'static str {
    match t {
        15012 => "SmCone",
        15013 => "SmCylinder",
        15014 => "SmSphere",
        15015 => "SmTorus",
        15030 => "SmSurfOfExtrusion",
        15020 => "SmSurfOfRevolution",
        _ => "analytic",
    }
}

/// A face's surface block: an exact analytic head (`2 ...`,
/// [`crate::util::read_exact_surface`]) and the regular block after it, or the regular
/// block alone. Returns the surface and whether the export carried it exactly (the
/// regular block of a B-spline surface or a plane is exact by itself).
fn read_face_surface(rd: &mut Rd<'_>) -> Result<(Option<Surface>, bool), HarnessError> {
    if rd.v.get(rd.at) != Some(&2.0) {
        return Ok((read_surface(rd)?, false));
    }
    rd.at += 1;
    let exact = crate::util::read_exact_surface(&mut || rd.num())?;
    let fallback = read_surface(rd)?;
    match exact {
        Some(s) => {
            crate::util::check_exact_base(&s, fallback.as_ref())?;
            Ok((Some(s), true))
        }
        None => Ok((fallback, false)),
    }
}

/// Builds the Rust B-rep from the shim's export (see the module docs).
pub(crate) fn import(ex: &[f64]) -> Result<Brep, HarnessError> {
    import_with_types(ex).map(|(b, _)| b)
}

/// [`import`], with the type codes of the geometry it imported.
pub(crate) fn import_with_types(ex: &[f64]) -> Result<(Brep, ImportTypes), HarnessError> {
    let mut extras = ImportTypes::default();
    let c = ffi::counts(ex).ok_or_else(|| harness("import of a failed build"))?;
    let [n_r, n_s, n_fu, n_f, n_lu, n_l, n_eu, n_e, n_vu, n_v] = c;
    let mut rd = Rd { v: ex, at: 1 };
    let validate_only = rd.num()? != 0.0;
    rd.at = 12;

    let mut regions = Vec::new();
    for _ in 0..n_r {
        let in_brep = rd.num()? != 0.0;
        let shells = rd.list()?.into_iter()
            .map(|x| opt(x, ShellId::from_index)?.ok_or_else(|| harness("null shell")))
            .collect::<Result<Vec<_>, _>>()?;
        regions.push(Region { in_brep, shells, ..Region::default() });
    }
    let mut shells = Vec::new();
    for _ in 0..n_s {
        let region = opt(rd.idx()?, RegionId::from_index)?;
        let shell_type = ShellType::from_code(rd.idx()?);
        let faceuses = rd.list()?.into_iter()
            .map(|x| opt(x, FaceuseId::from_index)?.ok_or_else(|| harness("null face use")))
            .collect::<Result<Vec<_>, _>>()?;
        let members = if faceuses.is_empty() { ShellMembers::Empty }
            else { ShellMembers::Faceuses(faceuses) };
        shells.push(Shell { faces: Vec::new(), region, shell_type, members });
    }
    let mut faceuses = Vec::new();
    for _ in 0..n_fu {
        let shell = opt(rd.idx()?, ShellId::from_index)?;
        let face = opt(rd.idx()?, FaceId::from_index)?;
        let mate = opt(rd.idx()?, FaceuseId::from_index)?;
        let orientation = orient_of(rd.idx()?)?;
        let loopuses = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, LoopuseId::from_index)?.ok_or_else(|| harness("null loop use")))
            .collect::<Result<Vec<_>, _>>()?;
        faceuses.push(Faceuse {
            shell,
            face,
            mate,
            orientation,
            loopuses,
            sibling_links: None,
        });
    }
    let mut surfaces = Vec::new();
    for _ in 0..n_f {
        let (surface, by_record) = read_face_surface(&mut rd)?;
        let (u0, v0, u1, v1) = (rd.num()?, rd.num()?, rd.num()?, rd.num()?);
        let uv_domain = Extent2d::from_bounds(u0, v0, u1, v1);
        let tolerance = rd.num()?;
        let ty = rd.idx()?;
        extras.face_surface_types.push(ty);
        extras
            .face_exact
            .push(by_record || ty < 0 || EXACT_SURFACES.contains(&ty));
        surfaces.push((surface, uv_domain, tolerance));
    }
    let mut loopuses = Vec::new();
    for _ in 0..n_lu {
        let faceuse = opt(rd.idx()?, FaceuseId::from_index)?;
        let loop_ = opt(rd.idx()?, LoopId::from_index)?;
        let mate = opt(rd.idx()?, LoopuseId::from_index)?;
        let ty = rd.idx()?;
        let start = rd.idx()?;
        let start = match ty {
            EDGEUSE_TYPE => LoopuseStart::Edgeuse(opt(start, EdgeuseId::from_index)?),
            VERTEXUSE_TYPE => LoopuseStart::Vertexuse(opt(start, VertexuseId::from_index)?),
            _ => LoopuseStart::Unknown,
        };
        let orientation = orient_of(rd.idx()?)?;
        loopuses.push(Loopuse {
            faceuse,
            loop_,
            mate,
            start,
            orientation,
        });
    }
    let mut loop_lus = Vec::new();
    for _ in 0..n_l {
        loop_lus.push(opt(rd.idx()?, LoopuseId::from_index)?);
    }
    let mut edgeuses = Vec::new();
    for _ in 0..n_eu {
        let edge = opt(rd.idx()?, EdgeId::from_index)?;
        let ty = rd.idx()?;
        let sorlu = rd.idx()?;
        let owner = match ty {
            LOOPUSE_TYPE => EdgeuseOwner::Loopuse(opt(sorlu, LoopuseId::from_index)?),
            SHELL_TYPE => EdgeuseOwner::Shell(opt(sorlu, ShellId::from_index)?),
            _ => EdgeuseOwner::Unknown,
        };
        let cw = opt(rd.idx()?, EdgeuseId::from_index)?;
        let ccw = opt(rd.idx()?, EdgeuseId::from_index)?;
        let vertexuse = opt(rd.idx()?, VertexuseId::from_index)?;
        let orientation = orient_of(rd.idx()?)?;
        let uv_trim_curve = read_curve(&mut rd, true)?;
        rd.idx()?; // the curve's owner: the holding edge use
        let has_fail = rd.num()? != 0.0;
        let fail_status = rd.idx()?;
        let fail_brep = rd.num()? != 0.0;
        let ef_init = rd.num()? != 0.0;
        let ccw_init = rd.num()? != 0.0;
        let mut r = Edgeuse {
            edge,
            owner,
            ccw,
            cw,
            vertexuse,
            orientation,
            uv_trim_curve,
            ..Edgeuse::default()
        };
        let me = EdgeuseId::from_index(u32::try_from(edgeuses.len()).map_err(|_| harness("overflow"))?);
        if has_fail {
            let f = DropCurveFail::<ObjectHandle> {
                fail_status: i32::try_from(fail_status).map_err(|_| harness("fail status"))?,
                brep: fail_brep.then_some(ObjectHandle {
                    kind: ObjectKind::Other,
                    key: ObjectKey(0),
                }),
                ..DropCurveFail::default()
            };
            r.drop_curve_fail = Some(Box::new(f));
        }
        if ef_init {
            // the gap value itself is never compared here
            r.max_edge_face_gap.set_edgeuse(Some(me));
        }
        if ccw_init {
            r.edge_ccw_edge_gap.edgeuse0 = Some(me);
            r.edge_ccw_edge_gap.edgeuse1 = ccw.or(Some(me));
        }
        edgeuses.push(r);
    }
    let mut edges = Vec::new();
    for _ in 0..n_e {
        let tolerance = rd.num()?;
        let a = rd.num()?;
        let b = rd.num()?;
        let list = rd
            .list()?
            .into_iter()
            .map(|x| opt(x, EdgeuseId::from_index)?.ok_or_else(|| harness("null edge use")))
            .collect::<Result<Vec<_>, _>>()?;
        extras.edge_curve_types.push(rd.v.get(rd.at).map_or(-1, |&t| t as i64));
        let curve = read_curve(&mut rd, false)?;
        let interval = Extent1d::from_raw(a, b);
        edges.push((tolerance, interval, list, curve));
    }
    let mut vertexuses = Vec::new();
    for _ in 0..n_vu {
        let vertex = opt(rd.idx()?, VertexId::from_index)?;
        let ty = rd.idx()?;
        let own = rd.idx()?;
        let owner = match ty {
            EDGEUSE_TYPE => VertexuseOwner::Edgeuse(opt(own, EdgeuseId::from_index)?),
            LOOPUSE_TYPE => VertexuseOwner::Loopuse(opt(own, LoopuseId::from_index)?),
            SHELL_TYPE => VertexuseOwner::Shell(opt(own, ShellId::from_index)?),
            _ => VertexuseOwner::Unknown,
        };
        let init = rd.num()? != 0.0;
        let mut r = Vertexuse {
            vertex,
            owner,
            ..Vertexuse::default()
        };
        if init {
            r.vertex_face_gap.vertexuse = Some(VertexuseId::from_index(
                u32::try_from(vertexuses.len()).map_err(|_| harness("overflow"))?,
            ));
        }
        vertexuses.push(r);
    }
    let mut vertices = Vec::new();
    for _ in 0..n_v {
        let point = Vec3::new(rd.num()?, rd.num()?, rd.num()?);
        let tolerance = rd.num()?;
        vertices.push(Vertex { point, tolerance, ..Vertex::default() });
    }
    if rd.idx()? != 3 {
        return Err(harness("export: unsupported shell extension version"));
    }
    let infinite_region = opt(rd.idx()?, RegionId::from_index)?;
    for region in &mut regions {
        region.is_void = i32::try_from(rd.idx()?)
            .map_err(|_| harness("export: void flag outside native int range"))?;
    }
    for shell in &mut shells {
        let head = rd.idx()?;
        if shell.is_wire_shell() {
            shell.members = opt(head, EdgeuseId::from_index)?
                .map_or(ShellMembers::Empty, ShellMembers::Edgeuse);
        } else if shell.is_vertex_shell() {
            shell.members = opt(head, VertexuseId::from_index)?
                .map_or(ShellMembers::Empty, ShellMembers::Vertexuse);
        } else if head != -1 {
            return Err(harness("export: unexpected shell head"));
        }
    }
    for vertex in &mut vertices {
        vertex.vertexuses = rd.list()?.into_iter()
            .map(|v| opt(v, VertexuseId::from_index)?.ok_or_else(|| harness("null vertex use")))
            .collect::<Result<Vec<_>, _>>()?;
    }
    for faceuse in &mut faceuses {
        faceuse.sibling_links = Some((opt(rd.idx()?, FaceuseId::from_index)?,
            opt(rd.idx()?, FaceuseId::from_index)?));
    }
    let brep_tolerance = rd.num()?;
    if rd.at != ex.len() {
        return Err(harness("export: trailing data"));
    }

    // assemble (skeleton fields filled minimally)
    let mut b = Brep::new();
    b.set_validate_only(validate_only);
    b.set_infinite_region_record(infinite_region);
    b.set_tolerance(brep_tolerance);
    for r in regions {
        b.add_region_record(r).map_err(|e| harness(e.to_string()))?;
    }
    let face_shell = |f: usize| -> ShellId {
        faceuses
            .iter()
            .find(|fu| fu.face == Some(FaceId::from_index(f as u32)) && fu.shell.is_some())
            .and_then(|fu| fu.shell)
            .unwrap_or(ShellId::from_index(0))
    };
    let face_shells: Vec<ShellId> = (0..n_f).map(face_shell).collect();
    for (i, mut s) in shells.into_iter().enumerate() {
        s.faces = face_shells
            .iter()
            .enumerate()
            .filter(|(_, sh)| sh.index() as usize == i)
            .map(|(f, _)| FaceId::from_index(f as u32))
            .collect();
        b.add_shell_record(s).map_err(|e| harness(e.to_string()))?;
    }
    for (f, (surface, uv_domain, tolerance)) in surfaces.into_iter().enumerate() {
        let shell = face_shells.get(f).copied().unwrap_or(ShellId::from_index(0));
        // m_pFU, which the export does not carry: the primary face use is the face's
        // upward one (SM_OT_SAME, SmFace::GetUpwardFaceuse)
        let faceuse = faceuses
            .iter()
            .position(|fu| {
                fu.face == Some(FaceId::from_index(f as u32))
                    && fu.orientation == OrientType::Same
            })
            .map(|i| FaceuseId::from_index(i as u32));
        b.add_face_record(Face {
            surface,
            uv_domain,
            tolerance,
            faceuse,
            ..Face::new(shell)
        })
        .map_err(|e| harness(e.to_string()))?;
    }
    for lu in &loop_lus {
        let face = lu
            .and_then(|x| loopuses.get(x.index() as usize))
            .and_then(|l| l.faceuse)
            .and_then(|fu| faceuses.get(fu.index() as usize))
            .and_then(|fu| fu.face)
            .unwrap_or(FaceId::from_index(0));
        b.add_loop_record(Loop {
            face,
            edges: Vec::new(),
            loopuse: *lu,
        })
        .map_err(|e| harness(e.to_string()))?;
    }
    for (tolerance, interval, list, curve) in edges {
        let vertex_of = |eu: Option<&EdgeuseId>| -> Option<VertexId> {
            let vu = edgeuses.get(eu?.index() as usize)?.vertexuse?;
            vertexuses.get(vu.index() as usize)?.vertex
        };
        let v0 = vertex_of(list.first()).unwrap_or(VertexId::from_index(0));
        let v1 = vertex_of(list.get(1)).unwrap_or(v0);
        b.add_edge_record(Edge {
            vertices: [v0, v1],
            edgeuses: list,
            interval,
            tolerance,
            curve,
        })
        .map_err(|e| harness(e.to_string()))?;
    }
    for v in vertices {
        b.add_vertex_record(v).map_err(|e| harness(e.to_string()))?;
    }
    for fu in faceuses {
        b.add_faceuse_record(fu).map_err(|e| harness(e.to_string()))?;
    }
    for lu in loopuses {
        b.add_loopuse_record(lu).map_err(|e| harness(e.to_string()))?;
    }
    for eu in edgeuses {
        b.add_edgeuse_record(eu).map_err(|e| harness(e.to_string()))?;
    }
    for vu in vertexuses {
        b.add_vertexuse_record(vu).map_err(|e| harness(e.to_string()))?;
    }
    // the vertex use lists are not exported: each vertex lists its uses in arena order
    let uses: Vec<(VertexuseId, Option<VertexId>)> =
        b.vertexuses().iter().map(|(id, r)| (id, r.vertex)).collect();
    for (id, v) in uses {
        if let Some(v) = v {
            b.vertex_mut(v).map_err(|e| harness(e.to_string()))?.vertexuses.push(id);
        }
    }
    Ok((b, extras))
}

// ---- Rust side --------------------------------------------------------------------------

/// The value of a query slot: an id index, `-1` for none, [`NULL_DEREF`] where
/// upstream dereferences null; deferral and missing ids propagate.
fn slot<T>(r: Result<T, SolidError>, f: impl FnOnce(T) -> i64) -> Result<i64, SolidError> {
    match r {
        Ok(v) => Ok(f(v)),
        Err(SolidError::InvalidInput { .. }) => Ok(NULL_DEREF),
        Err(e) => Err(e),
    }
}

fn id<I: Copy>(o: Option<I>, f: impl Fn(I) -> u32) -> i64 {
    o.map_or(-1, |x| i64::from(f(x)))
}

fn rust_err(e: &SolidError) -> Result<Output, HarnessError> {
    match Output::unported(e) {
        Some(o) => Ok(o),
        None => Err(harness(format!("rust: {e}"))),
    }
}

fn run_rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    if !is_brep_op(&i.op) {
        return match run_rust_geom(k, &i.op) {
            Ok(o) => Ok(o),
            Err(Fail::Solid(e)) => rust_err(&e),
            Err(Fail::Harness(h)) => Err(h),
        };
    }
    let ex = export(i)?;
    let Some(c) = ffi::counts(&ex) else {
        return Ok(build_failure(&ex));
    };
    let (mut b, types) = import_with_types(&ex)?;
    let Some(op) = resolve(&i.op, c) else {
        return Ok(no_entity());
    };
    if let Some(o) = inexact_geometry(&b, &types, &op) {
        return Ok(o);
    }
    match run_rust_brep(k, &mut b, &op) {
        Ok(o) => Ok(o),
        Err(Fail::Solid(e)) => rust_err(&e),
        Err(Fail::Harness(h)) => Err(h),
    }
}

/// `SmBSplineSurface_TYPE`, `SmPlane_TYPE`; `SmBSplineCurve_TYPE`, `SmLine_TYPE`.
const EXACT_SURFACES: [i64; 2] = [15002, 15011];
const EXACT_CURVES: [i64; 2] = [14033, 14010];

/// The deferral of a case that evaluates a face surface or edge curve the import holds
/// only as its B-spline part (an analytic `SmBSplineSurface` / `SmBSplineCurve`
/// subclass): the gap calculations and the 3D-drop branch of `GetStartPointUVLoopGap`.
fn inexact_geometry(b: &Brep, types: &ImportTypes, op: &OpIn) -> Option<Output> {
    let surface_of = |e: EdgeuseId| {
        eu1::face(b, e)
            .ok()
            .flatten()
            .and_then(|f| types.inexact_surface(f.index() as usize))
            .unwrap_or(-1)
    };
    let curve_of = |e: Option<EdgeuseId>| {
        e.and_then(|e| eu1::edge(b, e).ok().flatten())
            .and_then(|x| types.edge_curve_types.get(x.index() as usize).copied())
            .unwrap_or(-1)
    };
    let (e, surface, other) = match op {
        OpIn::Gaps { eu, which, .. } => {
            let e = eid(*eu);
            if !eu1::is_loop_edgeuse(b, e).unwrap_or(false) {
                return None;
            }
            if *which == 0 || *which == 2 {
                (e, true, None)
            } else {
                (e, false, eu1::ccw_edgeuse(b, e).ok().flatten())
            }
        }
        OpIn::UvLoopGap { eu } => {
            let e = eid(*eu);
            let r = b.edgeuse(e).ok()?;
            let cw = r.cw?;
            let both = eu1::uv_trim_curve(b, e).ok().and_then(|x| x.0).is_some()
                && eu1::uv_trim_curve(b, cw).ok().and_then(|x| x.0).is_some();
            if both {
                return None;
            }
            (e, true, Some(cw))
        }
        _ => return None,
    };
    let st = if surface { surface_of(e) } else { -1 };
    if st >= 0 && !EXACT_SURFACES.contains(&st) {
        let (unit, symbol) = match st {
            // the DISPATCH GAP of `Surface::read_from_db` (issue #76) belongs to
            // `surface.surface_p2`, not to the analytic surface units (all green)
            15012 => ("surface.surface_p2", "SurfaceKind::Cone (SmCone dispatch)"),
            15013 => ("surface.surface_p2", "SurfaceKind::Cone (SmCylinder dispatch)"),
            15014 => ("surface.surface_p2", "SurfaceKind::Sphere (SmSphere dispatch)"),
            15015 => ("surface.surface_p2", "SurfaceKind::Torus (SmTorus dispatch)"),
            15030 => ("surface.surf_of_extrusion_p2", "SurfaceKind::SurfOfExtrusion"),
            _ => ("topology.brep_p10", "SmBrep::MakeTopologyFromData"),
        };
        return Some(Output::deferred(unit, symbol));
    }
    for t in [curve_of(Some(e)), curve_of(other)] {
        if t >= 0 && !EXACT_CURVES.contains(&t) {
            return Some(Output::deferred(
                "topology.brep_p10",
                "SmBrep::MakeTopologyFromData",
            ));
        }
    }
    None
}

enum Fail {
    Solid(SolidError),
    Harness(HarnessError),
}

impl From<SolidError> for Fail {
    fn from(e: SolidError) -> Self {
        Self::Solid(e)
    }
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Self::Harness(e)
    }
}

fn eid(i: i64) -> EdgeuseId {
    EdgeuseId::from_index(i as u32)
}

/// The state of an edge use after `SetUVTrimCurve`, as the shim's `put_eu_state`.
fn eu_state(k: &Kernel, b: &Brep, eu: Option<EdgeuseId>, out: &mut Vec<f64>) -> Result<(), SolidError> {
    let Some(eu) = eu else {
        out.push(-1.0);
        return Ok(());
    };
    out.push(1.0);
    let r = b.edgeuse(eu)?;
    match r.uv_trim_curve.as_ref() {
        None => out.push(-1.0),
        Some(c) => {
            out.push(1.0);
            let ivl = c.natural_interval();
            out.extend([ivl.min(), ivl.max()]);
            for t in [ivl.min(), ivl.mid(), ivl.max()] {
                let p = c.evaluate_point(k, t).unwrap_or(Vec3::UNDEF);
                out.extend([p.x, p.y, p.z]);
            }
        }
    }
    out.push(if r.uv_trim_curve.is_some() { f64::from(eu.index()) } else { -1.0 });
    match r.drop_curve_fail.as_deref() {
        Some(f) => out.extend([1.0, f64::from(f.fail_status), flag(f.brep.is_some())]),
        None => out.extend([0.0, 0.0, 0.0]),
    }
    match r.vertexuse {
        Some(vu) => out.push(flag(b.vertexuse(vu)?.vertex_face_gap.is_init())),
        None => out.push(-1.0),
    }
    Ok(())
}

fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        SolidError::InvalidInput { .. } => NULL_DEREF as f64,
        _ => -9.0,
    }
}

fn run_rust_brep(k: &Kernel, b: &mut Brep, op: &OpIn) -> Result<Output, Fail> {
    match op {
        OpIn::EuQuery { eu, vertex } => {
            let e = eid(*eu);
            let v = (*vertex >= 0).then(|| VertexId::from_index(*vertex as u32));
            let mut q = vec![
                id(eu1::edge(b, e)?, EdgeId::index),
                orient_code(eu1::orientation(b, e)?),
            ];
            q.push(id(eu1::cw_edgeuse(b, e)?, EdgeuseId::index));
            q.push(id(eu1::ccw_edgeuse(b, e)?, EdgeuseId::index));
            q.push(i64::from(eu1::has_edge_face_gap(b, e)?));
            q.push(i64::from(eu1::has_edge_ccw_edge_gap(b, e)?));
            q.push(i64::from(eu1::is_max_edge_face_gap_init(b, e)?));
            q.push(i64::from(eu1::is_loop_edgeuse(b, e)?));
            q.push(i64::from(eu1::is_shell_edgeuse(b, e)?));
            q.push(id(eu1::vertexuse(b, e)?, VertexuseId::index));
            q.push(id(eu1::loopuse(b, e)?, LoopuseId::index));
            q.push(id(eu1::faceuse(b, e)?, FaceuseId::index));
            q.push(slot(eu1::face(b, e), |x| id(x, FaceId::index))?);
            q.push(id(eu1::shell(b, e)?, ShellId::index));
            q.push(slot(eu1::brep(b, e), |x| i64::from(x.is_some()))?);
            let tol = eu1::tolerance(b, e)?;
            q.push(slot(eu1::mate(b, e), |x| id(x, EdgeuseId::index))?);
            q.push(slot(eu1::radial(b, e), |x| id(x, EdgeuseId::index))?);
            q.push(slot(eu1::is_strut(b, e), i64::from)?);
            let uv = eu1::uv_trim_curve(b, e).map(|(c, o)| (c.is_some(), o));
            q.push(slot(uv.clone(), |(c, _)| i64::from(c))?);
            q.push(slot(uv, |(_, o)| id(o, EdgeuseId::index))?);
            let fail = eu1::drop_curve_fail(b, e).map(|(f, o)| (f.is_some(), o));
            q.push(slot(fail.clone(), |(f, _)| i64::from(f))?);
            q.push(slot(fail, |(_, o)| id(o, EdgeuseId::index))?);
            q.push(i64::from(eu1::uv_trim_curve_pointer(b, e)?.is_some()));
            q.push(slot(eu1::corner_mate_edgeuse(b, e, v), |x| id(x, EdgeuseId::index))?);
            q.push(eu1::TYPE);
            q.extend(KIND_TYPES.iter().map(|&t| i64::from(eu1::is_kind_of(t))));
            Ok(Output::new().ints("q", q).values("tol", vec![tol]))
        }
        OpIn::Connected { eu, ty, index } => {
            let t = target(*ty, *index);
            Ok(Output::new().ints("q", vec![i64::from(eu1::is_connected_to(b, eid(*eu), t)?)]))
        }
        OpIn::FuQuery {
            fu,
            tgt_only,
            has_loops,
            loops,
        } => {
            let f = FaceuseId::from_index(*fu as u32);
            let mut q = vec![
                id(eu1::faceuse_shell(b, f)?, ShellId::index),
                id(eu1::faceuse_region(b, f)?, RegionId::index),
                id(eu1::faceuse_face(b, f)?, FaceId::index),
                id(eu1::faceuse_mate(b, f)?, FaceuseId::index),
                orient_code(eu1::faceuse_orientation(b, f)?),
                eu1::FACEUSE_TYPE,
            ];
            q.extend(KIND_TYPES.iter().map(|&t| i64::from(eu1::faceuse_is_kind_of(t))));
            let ls: Vec<LoopId> = loops.iter().map(|&l| LoopId::from_index(l as u32)).collect();
            let tl = has_loops.then_some(ls.as_slice());
            let lists = vec![
                eu1::faceuse_loopuses(b, f, *tgt_only, tl)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
                eu1::faceuse_edgeuses(b, f, *tgt_only, tl)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
                eu1::faceuse_neighbors(b, f, false)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
                eu1::faceuse_neighbors(b, f, true)?
                    .into_iter()
                    .map(|x| i64::from(x.index()))
                    .collect(),
            ];
            Ok(Output::new()
                .ints("q", q)
                .topology(crate::output::Topology::new().adjacency("lists", lists)))
        }
        OpIn::Collect { fu, threshold } => {
            let mut list = Vec::new();
            eu1::faceuse_collect_adjacent(&Kernel::default(), b, FaceuseId::from_index(*fu as u32), &mut list, *threshold)?;
            Ok(Output::new()
                .int("status", 0)
                .ints("list", list.into_iter().map(|x| i64::from(x.index())).collect::<Vec<_>>()))
        }
        OpIn::UvLoopGap { eu } => Ok(Output::new().values("gap", vec![eu1::start_point_uv_loop_gap(&Kernel::default(), b, eid(*eu))?])),
        OpIn::SetUv {
            eu,
            kind,
            curve,
            dmax,
            delete,
            fail,
        } => {
            let e = eid(*eu);
            let mate = match eu1::mate(b, e) {
                Ok(m) => m,
                Err(SolidError::InvalidInput { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF as f64]))
                }
                Err(x) => return Err(x.into()),
            };
            let mut arg_fail = fail.map(|s| DropCurveFail::<ObjectHandle> {
                fail_status: s as i32,
                ..DropCurveFail::default()
            });
            let new_curve = match curve {
                Some(c) => Some(SmCurve::from(bspline_of(c)?)),
                None => None,
            };
            let r = match kind {
                3 => eu1::delete_uv_trim_curve(b, e),
                4 => {
                    let c = new_curve.ok_or_else(|| harness("set_uv: no curve"))?;
                    eu1::set_member_uv_trim_curve(b, e, c, arg_fail.clone()).map(|_| ())
                }
                _ => {
                    let arg = match kind {
                        0 => eu1::UvTrimCurveArg::Null,
                        1 => eu1::UvTrimCurveArg::Current,
                        _ => eu1::UvTrimCurveArg::New(Box::new(
                            new_curve.ok_or_else(|| harness("set_uv: no curve"))?,
                        )),
                    };
                    eu1::set_uv_trim_curve(b, e, arg, *dmax, *delete, arg_fail.as_mut()).map(|_| ())
                }
            };
            match r {
                Ok(()) => {}
                Err(SolidError::InvalidInput { .. }) => {
                    return Ok(Output::new().values("state", vec![NULL_DEREF as f64]))
                }
                Err(x) => return Err(x.into()),
            }
            let mut out = vec![0.0];
            eu_state(k, b, Some(e), &mut out)?;
            eu_state(k, b, mate, &mut out)?;
            out.push(match eu1::edge(b, e)? {
                Some(x) => b.edge(x)?.tolerance,
                None => -1.0,
            });
            out.push(flag(*kind != 4 && arg_fail.as_ref().is_some_and(|f| f.brep.is_some())));
            Ok(Output::new().values("state", out))
        }
        OpIn::Gaps {
            eu,
            which,
            flag: f,
            want_out,
        } => {
            let e = eid(*eu);
            let mut out = Vec::new();
            let fg0 = b.edgeuse(e)?.max_edge_face_gap.is_init();
            let cg0 = b.edgeuse(e)?.edge_ccw_edge_gap.is_init();
            let mut vals: Vec<f64> = Vec::new();
            match which {
                0 => out.push(flag(eu1::max_edge_face_gap(&Kernel::default(), b, e, *f)?.is_some())),
                1 => out.push(flag(eu1::edge_ccw_edge_gap(&Kernel::default(), b, e, *f)?.is_some())),
                2 => match eu1::calc_max_edge_face_gap(&Kernel::default(), b, e, *f) {
                    Ok(g) => {
                        out.extend([0.0, flag(*want_out && g.is_init())]);
                        if *want_out && g.is_init() {
                            vals.extend([g.gap_3d, g.edge_t, g.face_uv.x, g.face_uv.y]);
                        }
                    }
                    Err(x) if x.is_unported() => return Err(x.into()),
                    Err(x) => out.extend([status_of(&x), 0.0]),
                },
                _ => match eu1::calc_edge_ccw_edge_gap(&Kernel::default(), b, e, *want_out, *f) {
                    Ok(g) => {
                        out.extend([0.0, flag(g.is_some_and(|g| g.is_init()))]);
                        if let Some(g) = g.filter(solidcore::heal::gap::EdgeEdgeGap::is_init) {
                            vals.extend([g.gap_3d, g.edge_t0, g.edge_t1]);
                        }
                    }
                    Err(x) if x.is_unported() => return Err(x.into()),
                    Err(x) => out.extend([status_of(&x), 0.0]),
                },
            }
            let is_loop = eu1::is_loop_edgeuse(b, e)?;
            let fcomp = is_loop && ((*which == 0 && (*f || !fg0)) || (*which == 2 && *f));
            let ccomp = is_loop && ((*which == 1 && (*f || !cg0)) || (*which == 3 && *f));
            {
                let r = b.edgeuse(e)?;
                let g = r.max_edge_face_gap;
                if fcomp && g.is_init() {
                    vals.extend([g.gap_3d, g.edge_t, g.face_uv.x, g.face_uv.y]);
                }
                let g = r.edge_ccw_edge_gap;
                if ccomp && g.is_init() {
                    vals.extend([g.gap_3d, g.edge_t0, g.edge_t1]);
                }
            }
            let r = b.edgeuse(e)?;
            out.push(flag(r.max_edge_face_gap.is_init()));
            out.push(flag(r.edge_ccw_edge_gap.is_init()));
            let mate = eu1::mate(b, e).ok().flatten();
            out.push(match mate {
                Some(m) => flag(b.edgeuse(m)?.max_edge_face_gap.is_init()),
                None => -1.0,
            });
            out.extend(vals);
            Ok(Output::new().values("state", out))
        }
        OpIn::SetOrient { eu, orient } => {
            let e = eid(*eu);
            eu1::set_orientation(b, e, orient_of(i64::from(*orient))?)?;
            let q = vec![
                orient_code(eu1::orientation(b, e)?),
                slot(eu1::mate(b, e), |x| id(x, EdgeuseId::index))?,
                slot(eu1::radial(b, e), |x| id(x, EdgeuseId::index))?,
            ];
            Ok(Output::new().ints("q", q))
        }
        _ => Err(harness("not a B-rep op").into()),
    }
}

/// The `IsConnectedTo` target of a type code and index (`None` for null and unknown
/// codes, which upstream also answers `TRUE` for... via the null check).
fn target(ty: i64, index: i64) -> Option<EntityRef> {
    let i = u32::try_from(index).ok()?;
    Some(match ty {
        16012 => EntityRef::Region(RegionId::from_index(i)),
        16013 => EntityRef::Shell(ShellId::from_index(i)),
        16022 => EntityRef::Faceuse(FaceuseId::from_index(i)),
        16002 => EntityRef::Face(FaceId::from_index(i)),
        16004 => EntityRef::Loopuse(LoopuseId::from_index(i)),
        16003 => EntityRef::Loop(LoopId::from_index(i)),
        16005 => EntityRef::Edgeuse(EdgeuseId::from_index(i)),
        16016 => EntityRef::Edge(EdgeId::from_index(i)),
        16006 => EntityRef::Vertexuse(VertexuseId::from_index(i)),
        16021 => EntityRef::Vertex(VertexId::from_index(i)),
        _ => return None,
    })
}

pub(crate) fn bspline_of(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c.cpts.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, c.dim, false))
}

pub(crate) fn surface_of(s: &SrfIn) -> Result<Surface, HarnessError> {
    match s {
        SrfIn::Plane { o, n } => Ok(Surface::from(Plane::from_normal(
            Vec3::from_array(*o),
            Vec3::from_array(*n),
        ))),
        SrfIn::Bspline(bs) => {
            let net = bs
                .net
                .iter()
                .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
                .collect();
            let nl = NlSurface::new(net, bs.p, bs.q, bs.u.clone(), bs.v.clone())
                .map_err(|e| harness(format!("surface input: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = bs.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(bs.form))
                .ok_or_else(|| harness(format!("bad form {}", bs.form)))?;
            s.set_b_spline_surface_form(form);
            Ok(Surface::from(s))
        }
    }
}

/// A solution object handle of `sm_CalcDistAndDev` cases.
#[derive(Debug, Clone, Copy, PartialEq)]
struct Obj(ObjectKind);

impl solidcore::geom::solution_array::SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        self.0
    }
}

fn run_rust_geom(k: &Kernel, op: &OpIn) -> Result<Output, Fail> {
    match op {
        OpIn::StepOff {
            surface,
            dist,
            p,
            n,
            x,
            jac,
        } => {
            let s = surface_of(surface)?;
            let mut e = eu1::FindStepOff::new(k, *dist, Vec3::from_array(*p), Vec3::from_array(*n), &s);
            let mut f = [UNDEF_DOUBLE; 2];
            let mut m = Matrix::new(2, 2);
            for r in 0..2 {
                for c in 0..2 {
                    m.set_at(r, c, UNDEF_DOUBLE)?;
                }
            }
            let res = e.evaluate(x, &mut f, if *jac { Some(&mut m) } else { None });
            let failed = match res {
                Ok(_) => 0,
                Err(x) if x.is_unported() => return Err(x.into()),
                Err(_) => 1,
            };
            let mut fj = f.to_vec();
            for r in 0..2 {
                for c in 0..2 {
                    fj.push(m.at(r, c)?);
                }
            }
            Ok(Output::new().int("failed", failed).int("found", 0).values("fj", fj))
        }
        OpIn::DistDev {
            surface,
            p,
            uv,
            value,
            obj,
            want_dev,
        } => {
            let s = surface_of(surface)?;
            let mut sol = Solution::<Obj>::new();
            sol.start.parameters[0] = uv[0];
            sol.start.parameters[1] = uv[1];
            sol.start.solution_value = *value;
            sol.objects[0] = match obj {
                1 => Some(Obj(ObjectKind::Surface)),
                2 => Some(Obj(ObjectKind::Curve)),
                _ => None,
            };
            let (dist, dev) =
                eu1::calc_dist_and_dev(k, Vec3::from_array(*p), &sol, |_| Some(&s), *want_dev)?;
            Ok(Output::new().values("dist_dev", vec![dist, dev.unwrap_or(UNDEF_DOUBLE)]))
        }
        OpIn::Seam {
            curves,
            dom,
            called,
        } => {
            let cs = curves
                .iter()
                .map(|c| bspline_of(c).map(SmCurve::from))
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<&SmCurve> = cs.iter().collect();
            let d = Extent1d::from_min_max(dom[0], dom[1]);
            Ok(Output::new().ints("seam", vec![i64::from(eu1::are_seam_curves(&refs, &d, *called))]))
        }
        _ => Err(harness("not a geometry op").into()),
    }
}

// ---- generation -------------------------------------------------------------------------

/// The upstream `.smb` files used (all B-spline geometry).
/// (`SmallBox.smb` and `Platform.smb` crash upstream's `SmBrep::ReadFromFile`; see
/// `plan/findings.md`.)
pub(crate) const FILES: [&str; 15] = [
    "Mouse.smb",
    "Endcap.smb",
    "box_with_large_fillet.smb",
    "cone.smb",
    "disk.smb",
    "ellips_ts2.smb",
    "plane_circ.smb",
    "plane_square.smb",
    "sphere.smb",
    "surf_extrude.smb",
    "surf_rev.smb",
    "surf_sphere150.smb",
    "torus.smb",
    "toruswvy.smb",
    "vase_rev.smb",
];

pub(crate) fn clamped_knots(rng: &mut Rng, p: usize, n: usize, a: f64, b: f64) -> Vec<f64> {
    let inner = n - p;
    let mut ks: Vec<f64> = (0..inner).map(|_| a + (b - a) * rng.uniform(0.05, 0.95)).collect();
    ks.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(ks);
    u.extend(vec![b; p + 1]);
    u
}

pub(crate) fn gen_curve(rng: &mut Rng, dim: u64, scale: f64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.2);
    let cpts = (0..=n)
        .map(|i| {
            let w = if rational { rng.uniform(0.5, 2.0) } else { NOW };
            let k = if rational { w } else { 1.0 };
            let x = (i as f64 + rng.uniform(-0.3, 0.3)) * scale;
            let y = rng.uniform(-1.0, 1.0) * scale;
            let z = if dim == 2 { NOZ } else { rng.uniform(-1.0, 1.0) * scale * k };
            [x * k, y * k, z, w]
        })
        .collect();
    let (a, b) = if rng.chance(0.5) { (0.0, 1.0) } else { (rng.uniform(-5.0, 0.0), rng.uniform(1.0, 6.0)) };
    CrvIn {
        dim,
        cpts,
        p,
        u: clamped_knots(rng, p, n, a, b),
    }
}

pub(crate) fn gen_surface(rng: &mut Rng) -> SrfIn {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    if rng.chance(0.25) {
        let o = rng.point3(-2.0, 2.0).map(|c| c * scale);
        let mut n = rng.point3(-1.0, 1.0);
        if n.iter().all(|c| c.abs() < 0.1) {
            n = [0.0, 0.0, 1.0];
        }
        return SrfIn::Plane { o, n };
    }
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let pinch = rng.chance(0.15);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (x, y) = if pinch && i == 0 { (0.0, 0.0) } else { (i as f64, j as f64) };
                    [x * scale, y * scale, rng.uniform(-0.5, 0.5) * scale, NOW]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: rng.chance(0.2),
        form: 0,
        net,
        p,
        q,
        u: clamped_knots(rng, p, n, 0.0, 1.0),
        v: clamped_knots(rng, q, m, 0.0, 1.0),
    })
}

pub(crate) fn domain(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        SrfIn::Bspline(b) => [
            b.u.first().copied().unwrap_or(0.0),
            b.v.first().copied().unwrap_or(0.0),
            b.u.last().copied().unwrap_or(1.0),
            b.v.last().copied().unwrap_or(1.0),
        ],
    }
}

pub(crate) fn gen_brep(rng: &mut Rng) -> BrepIn {
    match rng.below(10) {
        0..=5 => BrepIn::File {
            file: (*rng.choose(&FILES).unwrap_or(&"SmallBox.smb")).to_string(),
            rebuild: rng.chance(0.25),
        },
        6 => {
            let c = gen_curve(rng, 3, 1.0);
            let (a, b) = (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0));
            let (t0, t1) = if rng.chance(0.5) {
                (a, b)
            } else {
                (a + (b - a) * rng.uniform(0.0, 0.3), b - (b - a) * rng.uniform(0.0, 0.3))
            };
            BrepIn::Wire { curve: c, t0, t1 }
        }
        7 | 8 => {
            let s = gen_surface(rng);
            let [u0, v0, u1, v1] = domain(&s);
            let dom = if rng.chance(0.5) {
                [u0, v0, u1, v1]
            } else {
                let (du, dv) = (u1 - u0, v1 - v0);
                [
                    u0 + du * rng.uniform(0.0, 0.3),
                    v0 + dv * rng.uniform(0.0, 0.3),
                    u1 - du * rng.uniform(0.0, 0.3),
                    v1 - dv * rng.uniform(0.0, 0.3),
                ]
            };
            BrepIn::Face { surface: s, dom }
        }
        _ => BrepIn::Network,
    }
}

fn gen_sel(rng: &mut Rng) -> i64 {
    rng.below(1 << 20) as i64
}

fn gen_uv(rng: &mut Rng, s: &SrfIn) -> [f64; 2] {
    let [u0, v0, u1, v1] = domain(s);
    let pick = |rng: &mut Rng, a: f64, b: f64| match rng.below(8) {
        0 => a,
        1 => b,
        2 => 0.5 * (a + b),
        3 => a - (b - a) * rng.uniform(0.0, 0.5),
        _ => a + (b - a) * rng.unit(),
    };
    [pick(rng, u0, u1), pick(rng, v0, v1)]
}

fn gen_op(rng: &mut Rng) -> OpIn {
    let types = [16012, 16013, 16022, 16002, 16004, 16003, 16005, 16016, 16006, 16021, 16011, 7];
    match rng.below(24) {
        0..=4 => OpIn::EuQuery {
            eu: gen_sel(rng),
            vertex: if rng.chance(0.15) { -1 } else { gen_sel(rng) },
        },
        5..=7 => OpIn::Connected {
            eu: gen_sel(rng),
            ty: *rng.choose(&types).unwrap_or(&16005),
            index: if rng.chance(0.1) { -1 } else { gen_sel(rng) },
        },
        8..=10 => OpIn::FuQuery {
            fu: gen_sel(rng),
            tgt_only: rng.chance(0.5),
            has_loops: rng.chance(0.5),
            loops: (0..rng.below(4)).map(|_| gen_sel(rng)).collect(),
        },
        11 => OpIn::Collect {
            fu: gen_sel(rng),
            threshold: rng.chance(0.2).then(|| rng.uniform(0.0, 1.0)),
        },
        12..=13 => OpIn::UvLoopGap { eu: gen_sel(rng) },
        14..=16 => {
            let kind = rng.below(5) as u8;
            OpIn::SetUv {
                eu: gen_sel(rng),
                kind,
                curve: (kind == 2 || kind == 4).then(|| gen_curve(rng, 2, 1.0)),
                dmax: *rng.choose(&[0.0, 0.0, 1e-9, 1e-6, 1e-3, 1.0]).unwrap_or(&0.0),
                delete: rng.chance(0.5),
                fail: rng.chance(0.4).then(|| rng.range_i64(0, 1100)),
            }
        }
        17 => OpIn::Gaps {
            eu: gen_sel(rng),
            which: rng.below(4) as u8,
            flag: rng.chance(0.5),
            want_out: rng.chance(0.5),
        },
        18 => OpIn::SetOrient {
            eu: gen_sel(rng),
            orient: rng.below(3) as u8,
        },
        19..=20 => {
            let s = gen_surface(rng);
            let x = gen_uv(rng, &s);
            OpIn::StepOff {
                dist: *rng.choose(&[0.0, 1e-6, 0.1, 1.0, 10.0]).unwrap_or(&0.1),
                p: rng.point3(-3.0, 3.0),
                n: if rng.chance(0.1) { [0.0; 3] } else { rng.point3(-1.0, 1.0) },
                x,
                jac: rng.chance(0.7),
                surface: s,
            }
        }
        21..=22 => {
            let s = gen_surface(rng);
            let uv = gen_uv(rng, &s);
            OpIn::DistDev {
                p: rng.point3(-3.0, 3.0),
                uv,
                value: rng.uniform(0.0, 2.0),
                obj: rng.below(3) as u8,
                want_dev: rng.chance(0.8),
                surface: s,
            }
        }
        _ => {
            let n = *rng.choose(&[0usize, 1, 2, 2, 2, 3]).unwrap_or(&2);
            let mut curves: Vec<CrvIn> = (0..n).map(|_| gen_curve(rng, 2, 1.0)).collect();
            if n == 2 && rng.chance(0.5) {
                // Copy curve 0's knots only onto a curve of the same degree, or the
                // result has an end multiplicity above degree + 1.
                let (first, first_p) =
                    curves.first().map(|c| (c.u.clone(), c.p)).unwrap_or_default();
                if let Some(c) = curves.get_mut(1) {
                    if c.u.len() == first.len() && c.p == first_p {
                        c.u = first;
                    }
                }
            }
            let (a, b) = curves.first().map_or((0.0, 1.0), |c| {
                (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0))
            });
            let slop = rng.uniform(0.0, 0.02) * (b - a);
            OpIn::Seam {
                curves,
                dom: if rng.chance(0.6) { [a - slop, b + slop] } else { [a, a + 0.5 * (b - a)] },
                called: rng.chance(0.3),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.edgeuse_p1"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse::<Inputs>()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        run_rust(k, &case.parse::<Inputs>()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let op = gen_op(rng);
        let brep = is_brep_op(&op).then(|| gen_brep(rng));
        let i = Inputs {
            brep,
            validate_only: rng.chance(0.15),
            init_gaps: rng.chance(0.2),
            op,
        };
        serde_json::to_value(i).ok()
    }
}
