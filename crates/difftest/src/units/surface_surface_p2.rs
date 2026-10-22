//! Runner for unit `surface.surface_p2`: the `SmSurface` base class (the virtual interface
//! over a base `SmSurface()` and B-spline surfaces, type queries, the static limits),
//! `SmSurfaceEval`, `SmDerivSurfDefinition`, `SmDropCurveFail`, the `smsurf_*` numerics,
//! the file-static helpers of `SmSurface.cpp` and `SmLSIGlobalSolver`'s box test.
//!
//! C++: `solidcore_ffi::units::surface_surface_p2` (upstream `source/SMLib/src/SmSurface.cpp`,
//! called through `SmSurface*` so the virtual dispatch is upstream's; the file-static
//! helpers are verbatim copies in the shim). Rust: `solidcore::surface::surface::p2`.
//!
//! A case is an optional `this` (`{"base": true}` for the base class, else `oob`, `form`
//! and an inline or `.sms` surface) and an `op` with its parameters. Both sides produce
//! the flat record documented in the ffi module; the runner reports its first entry as the
//! `status` (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`,
//! `-1` for a Rust error upstream cannot produce) and the rest as the `out` values. The
//! Rust side runs the base-class defaults (`base_*`) for `this.base`.
//!
//! Deferred cases: the paths into unported units (`CopyAndAddAnalytics` and
//! `GetMaxAnalyticDomain` of B-splines, `GlobalSurfaceIntersect`, `CreatePlaneWith3DCurves`,
//! curve property analyses). Every run has at least [`MIN_GENERATED`] generated cases;
//! generated inputs the ffi wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve as SmCurve, CurveKind};
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::a_object;
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::{Branch, GlobalSolver, SolverOperation};
use solidcore::geom::object::OBJECT_TYPE;
use solidcore::geom::solution_array::{ObjectKind, SolutionArray, SolutionObject};
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::tree::{AuxDataType, Tree, TreeNode};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, SurfParamType};
use solidcore::nurbs::b_spline_surface_n_lib::p2::{EvaluateFlags, OrientType};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::surface::p2::{
    self as sp2, base_convert_uv, base_degree, base_natural_uv_domain, base_number_control_points,
    cc_local_solve, compute_surface_point_type, convert_to_parameter_delta, cs_are_perpen_disjoint,
    derivative_limit, drop_vectors, evaluate_geometric, intersect_and_split,
    intersect_curve_and_split, is_even, lift_second_derivative, lift_third_derivative, map_uv,
    merge_double_arrays, normal_limit, scale_step_angular_derivatives, set_up_breaks, BlendEndType,
    CreateUvTrimCurvePathType, CrossSectionValues, CurvatureType, DerivSurfDefinition,
    DropCurveFail, DropCurveFailArgs, LsiGlobalSolver, Surface, SurfaceEval, SurfaceSideType,
    TangencyType,
};
use solidcore::topology::database_io::{DatabaseReader, FileType};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_p2::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.surface_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// A solution object handle (the solutions name no objects here).
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct Handle;

impl SolutionObject for Handle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }
}

/// The handle of the curve `sm_IntersectAndSplit` intersects: upstream its solutions
/// name the curve, whose natural interval `IdenticalSolutions` reads.
#[derive(Debug, Clone, Copy, PartialEq)]
struct CurveHandle(Extent1d);

impl SolutionObject for CurveHandle {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Curve
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        Some(self.0)
    }
}

// ---- inputs -----------------------------------------------------------------------

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// A B-spline surface with its flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    srf: SrfIn,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

/// The surface a member op runs on.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct This {
    /// The base class `SmSurface()`.
    #[serde(default)]
    base: bool,
    #[serde(default, flatten, skip_serializing_if = "Option::is_none")]
    bss: Option<BssIn>,
}

/// An inline curve: dimension, control points as stored, degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrvIn {
    dim: u64,
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Eval {
        uv: [f64; 2],
        hu: usize,
        hv: usize,
        u_left: bool,
        v_left: bool,
        upper: bool,
        nzt: bool,
    },
    EvalPt {
        uv: [f64; 2],
    },
    Queries,
    Knots {
        dir: u8,
        ivl: Option<[f64; 2]>,
    },
    Parallel {
        v: [f64; 3],
    },
    Reverse {
        dir: u8,
    },
    Reparam {
        dom: [f64; 4],
    },
    Swap,
    Transform {
        origin: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: Option<[f64; 3]>,
    },
    Trim {
        dom: [f64; 4],
    },
    Split {
        t: f64,
        dir: u8,
    },
    StepInv {
        dom: [f64; 4],
        p: [f64; 3],
        tol: f64,
        guess: Option<[f64; 2]>,
    },
    Extend {
        dist: f64,
        cont: u8,
    },
    ExtendDir {
        dir: u8,
        dist: f64,
        cont: u8,
    },
    AdjustStep {
        dom: [f64; 4],
    },
    MaxAnal,
    Copy,
    CopyAnal,
    DropFast {
        dom: [f64; 4],
        p: [f64; 3],
    },
    ConvertUv {
        uv: [f64; 2],
    },
    SEval {
        ctor: u8,
        uv: [f64; 2],
        dir: [f64; 3],
        /// `[kind, a, b, c]`: 0 `SetUV(point)`, 1 `SetUV(u, v)`, 2 `SetSectDir`, 3 evaluate.
        steps: Vec<[f64; 4]>,
    },
    Gssi {
        dom: [f64; 4],
        other: BssIn,
        other_dom: [f64; 4],
        /// `pOptApproxTol3d` (`None`: upstream's default).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tol: Option<f64>,
        /// Translates both surfaces first (`prog_test` `my_test_one_ssi`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        translate: Option<[f64; 3]>,
    },
    Lift2 {
        v: Vec<[f64; 3]>,
    },
    Lift3 {
        v: Vec<[f64; 3]>,
    },
    ScaleStep {
        hu: usize,
        hv: usize,
        u_ang: bool,
        v_ang: bool,
        d: Vec<[f64; 3]>,
    },
    Geom {
        e: Vec<[f64; 3]>,
    },
    PointType {
        n: [f64; 3],
        duu: [f64; 3],
        dvv: [f64; 3],
        want_uu: bool,
        want_vv: bool,
        side_uu: u8,
    },
    DropVecs {
        du: [f64; 3],
        dv: [f64; 3],
        v: Vec<[f64; 3]>,
    },
    ParamDelta {
        t: [f64; 3],
        d: [f64; 3],
    },
    Merge {
        a: Vec<f64>,
        b: Vec<f64>,
    },
    MapUv {
        uv: [f64; 2],
        orient: u8,
        dir: [f64; 2],
    },
    IsEven {
        i: i32,
    },
    Breaks {
        ivl: [f64; 2],
        b: Vec<f64>,
    },
    Limits {
        p: [f64; 3],
    },
    DerivAssert {
        samples: u64,
        tt: u8,
        dir_angles: [f64; 2],
        tref: bool,
        tan: [f64; 2],
        angles: [f64; 2],
        vector: [f64; 3],
        ct: u8,
        bet: u8,
        radii: [f64; 2],
        cref: bool,
        flip: bool,
        with_list: bool,
    },
    Cross {
        sets: CrossSets,
    },
    CrossSkin {
        sets: CrossSets,
        params: Vec<f64>,
        rail1: CrvIn,
        rail2: Option<CrvIn>,
        tol: f64,
    },
    Dcf {
        status: i32,
        from: u64,
        path: u8,
        ctx: bool,
        dom: [f64; 4],
        ivl: [f64; 2],
        tols: [f64; 3],
        flags: [u8; 5],
        max_drop: f64,
        closed: [u8; 2],
        closure: u8,
        sing: [u64; 2],
        poles: Vec<[f64; 3]>,
        normals: Vec<[f64; 3]>,
        poly: Vec<[f64; 3]>,
        knots: Vec<f64>,
        curves: Vec<CrvIn>,
    },
    CcLocal {
        c1: CrvIn,
        i1: [f64; 2],
        k1: [u8; 2],
        c2: CrvIn,
        i2: [f64; 2],
        k2: [u8; 2],
        sop: u8,
        tol: f64,
    },
    IsectSplit {
        c: CrvIn,
        ivl: [f64; 2],
        pp: [f64; 3],
        pn: [f64; 3],
        s: Vec<f64>,
    },
    IsectCurveSplit {
        c: CrvIn,
        ivl: [f64; 2],
        lp: [f64; 3],
        lv: [f64; 3],
        livl: [f64; 2],
        zero: f64,
        s: Vec<f64>,
    },
    Lsi {
        bbox: [[f64; 3]; 2],
        lp: [f64; 3],
        lv: [f64; 3],
        ivl: Option<[f64; 2]>,
        ray: bool,
        tol: f64,
    },
    CsPerpen {
        caux: u8,
        cbox: [[f64; 3]; 2],
        saux: u8,
        sbox: [[f64; 3]; 2],
    },
    Plane3d {
        curves: Vec<CrvIn>,
        tol: f64,
    },
    ReadDb {
        ty: i64,
        binary: bool,
        version: u64,
        db: DbIn,
    },
}

/// The optional arrays of [`CrossSectionValues`]: start positions and first
/// derivatives, then `[start 2nd, start 3rd, end pos, end 1st, end 2nd, end 3rd]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CrossSets {
    pos: Vec<[f64; 3]>,
    d1: Vec<[f64; 3]>,
    opt: [Option<Vec<[f64; 3]>>; 6],
}

/// A database stream: a file, or text.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum DbIn {
    File { file: String },
    Text { text: String },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Eval { .. } => Op::Eval,
            OpIn::EvalPt { .. } => Op::EvalPt,
            OpIn::Queries => Op::Queries,
            OpIn::Knots { .. } => Op::Knots,
            OpIn::Parallel { .. } => Op::Parallel,
            OpIn::Reverse { .. } => Op::Reverse,
            OpIn::Reparam { .. } => Op::Reparam,
            OpIn::Swap => Op::Swap,
            OpIn::Transform { .. } => Op::Transform,
            OpIn::Trim { .. } => Op::Trim,
            OpIn::Split { .. } => Op::Split,
            OpIn::StepInv { .. } => Op::StepInv,
            OpIn::Extend { .. } => Op::Extend,
            OpIn::ExtendDir { .. } => Op::ExtendDir,
            OpIn::AdjustStep { .. } => Op::AdjustStep,
            OpIn::MaxAnal => Op::MaxAnal,
            OpIn::Copy => Op::Copy,
            OpIn::CopyAnal => Op::CopyAnal,
            OpIn::DropFast { .. } => Op::DropFast,
            OpIn::ConvertUv { .. } => Op::ConvertUv,
            OpIn::SEval { .. } => Op::SEval,
            OpIn::Gssi { .. } => Op::Gssi,
            OpIn::Lift2 { .. } => Op::Lift2,
            OpIn::Lift3 { .. } => Op::Lift3,
            OpIn::ScaleStep { .. } => Op::ScaleStep,
            OpIn::Geom { .. } => Op::Geom,
            OpIn::PointType { .. } => Op::PointType,
            OpIn::DropVecs { .. } => Op::DropVecs,
            OpIn::ParamDelta { .. } => Op::ParamDelta,
            OpIn::Merge { .. } => Op::Merge,
            OpIn::MapUv { .. } => Op::MapUv,
            OpIn::IsEven { .. } => Op::IsEven,
            OpIn::Breaks { .. } => Op::Breaks,
            OpIn::Limits { .. } => Op::Limits,
            OpIn::DerivAssert { .. } => Op::DerivAssert,
            OpIn::Cross { .. } => Op::Cross,
            OpIn::CrossSkin { .. } => Op::CrossSkin,
            OpIn::Dcf { .. } => Op::Dcf,
            OpIn::CcLocal { .. } => Op::CcLocal,
            OpIn::IsectSplit { .. } => Op::IsectSplit,
            OpIn::IsectCurveSplit { .. } => Op::IsectCurveSplit,
            OpIn::Lsi { .. } => Op::Lsi,
            OpIn::CsPerpen { .. } => Op::CsPerpen,
            OpIn::Plane3d { .. } => Op::Plane3d,
            OpIn::ReadDb { .. } => Op::ReadDb,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    this: Option<This>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- file readers -------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| {
            t.parse::<f64>()
                .map_err(|e| harness(format!("{path}: {e}")))
        })
        .collect()
}

fn int(path: &str, v: f64) -> Result<usize, HarnessError> {
    if v.fract() != 0.0 || v < 0.0 {
        return Err(harness(format!("{path}: bad count {v}")));
    }
    Ok(v as usize)
}

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || {
        it.next()
            .ok_or_else(|| harness(format!("{path}: truncated")))
    };
    let n = int(path, num()?)?;
    let m = int(path, num()?)?;
    let p = int(path, num()?)?;
    let q = int(path, num()?)?;
    let rat = num()? != 0.0;
    let mut net = Vec::with_capacity(n + 1);
    for _ in 0..=n {
        let mut row = Vec::with_capacity(m + 1);
        for _ in 0..=m {
            let (x, y, z) = (num()?, num()?, num()?);
            let w = if rat { num()? } else { NOW };
            row.push([x, y, z, w]);
        }
        net.push(row);
    }
    let u = (0..n + p + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2)
        .map(|_| num())
        .collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

fn db_bytes(db: &DbIn) -> Result<Vec<u8>, HarnessError> {
    match db {
        DbIn::File { file } => std::fs::read(file).map_err(|e| harness(format!("{file}: {e}"))),
        DbIn::Text { text } => Ok(text.clone().into_bytes()),
    }
}

fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
    let n = s
        .net
        .len()
        .checked_sub(1)
        .ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols
        .checked_sub(1)
        .ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

/// Whether `CreateExtendedSurface` would retry a failed `N_SrfExtendByDist`: upstream
/// extends in place (`surP == surQ`), and a failed attempt leaves the surface on freshly
/// allocated, partly written `NL_STACKS` storage that the halved retry then reads
/// (undefined; its outcome depends on earlier calls in the process, see
/// `plan/findings.md`). Such cases are not generated. The side sequence is the one of
/// `create_extended_surface_dispatched` (singular and closed sides skipped); any failure
/// of the emulation counts as a retry.
fn extension_retries_in_place(i: &Inputs) -> bool {
    let (dir, dist, cont) = match &i.op {
        OpIn::Extend { dist, cont } => (SurfParamType::Both, *dist, *cont),
        OpIn::ExtendDir { dir, dist, cont } => match param_of(*dir) {
            Ok(d) => (d, *dist, *cont),
            Err(_) => return false,
        },
        _ => return false,
    };
    let Some(b) = i
        .this
        .as_ref()
        .filter(|t| !t.base)
        .and_then(|t| t.bss.as_ref())
    else {
        return false;
    };
    let Ok(bs) = bss_of(b) else {
        return false;
    };
    !matches!(extension_needs_no_retry(&bs, dir, dist, cont), Ok(true))
}

/// The first attempts of [`extension_retries_in_place`]: `Ok(true)` when every side
/// extends at the full distance.
fn extension_needs_no_retry(
    bs: &BSplineSurface,
    dir: SurfParamType,
    dist: f64,
    cont: u8,
) -> solidcore::Result<bool> {
    use solidcore::nurbs::crv_adv::p1::CurveEnd;
    use solidcore::nurbs::srf_adv::p1::extend_by_dist;
    use solidcore::nurbs::srf_adv::p2::SrfContinuity;
    use solidcore::nurbs::srf_basic::Direction;
    use SurfParamType as P;

    let Some(nurb) = bs.gw_nurb() else {
        return Ok(true);
    };
    let k = Kernel::default();
    let s = Surface::from(bs.clone_exact());
    let dom = bs.natural_uv_domain();
    let gflg = match continuity_of(cont) {
        Ok(ContinuityType::G1R) => SrfContinuity::G1R,
        Ok(ContinuityType::G1G2) => SrfContinuity::G2,
        Ok(ContinuityType::CInfinity) => SrfContinuity::Cmax,
        _ => SrfContinuity::G1,
    };
    let u_dir = matches!(dir, P::U | P::UMin | P::UMax);
    let v_dir = matches!(dir, P::V | P::VMin | P::VMax);
    let neither = dir == P::Neither;
    let mut skip_umin = neither || v_dir || dir == P::UMax;
    let mut skip_umax = neither || v_dir || dir == P::UMin;
    let mut skip_vmin = neither || u_dir || dir == P::VMax;
    let mut skip_vmax = neither || u_dir || dir == P::VMin;
    let sing = s.is_singularity(&k, dom.min(), solidcore::math::EFF_ZERO, false)?;
    if sing.singular {
        if sing.direction == P::U {
            skip_vmin = true;
        } else {
            skip_umin = true;
        }
    }
    let sing = s.is_singularity(&k, dom.max(), solidcore::math::EFF_ZERO, false)?;
    if sing.singular {
        if sing.direction == P::U {
            skip_vmax = true;
        } else {
            skip_umax = true;
        }
    }
    let mut cur = nurb.clone();
    let mut sides = Vec::new();
    if !v_dir && !s.is_closed(&k, &dom, P::U, None, false)?.0 {
        sides.extend([
            (Direction::U, CurveEnd::Start, skip_umin),
            (Direction::U, CurveEnd::End, skip_umax),
        ]);
    }
    if !u_dir && !s.is_closed(&k, &dom, P::V, None, false)?.0 {
        sides.extend([
            (Direction::V, CurveEnd::Start, skip_vmin),
            (Direction::V, CurveEnd::End, skip_vmax),
        ]);
    }
    for (d, e, skipped) in sides {
        if skipped {
            continue;
        }
        match extend_by_dist(&cur, dist, d, e, gflg) {
            Ok(x) => cur = x,
            Err(_) => return Ok(false),
        }
    }
    Ok(true)
}

// ---- C++ side -----------------------------------------------------------------------

fn push_srf(o: &mut Vec<f64>, s: &InlineSrf) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend(
        [
            n,
            m,
            s.p,
            s.q,
            s.u.len().saturating_sub(1),
            s.v.len().saturating_sub(1),
        ]
        .map(|x| x as f64),
    );
    for row in &s.net {
        for p in row {
            o.extend_from_slice(p);
        }
    }
    o.extend_from_slice(&s.u);
    o.extend_from_slice(&s.v);
    Ok(())
}

fn push_bss(o: &mut Vec<f64>, b: &BssIn) -> Result<(), HarnessError> {
    o.push(flag(b.oob));
    o.push(f64::from(b.form));
    push_srf(o, &srf(&b.srf)?)
}

fn push_crv(o: &mut Vec<f64>, c: &CrvIn) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([
        c.dim as f64,
        n as f64,
        c.p as f64,
        c.u.len().saturating_sub(1) as f64,
    ]);
    for p in &c.cpts {
        o.extend_from_slice(p);
    }
    o.extend_from_slice(&c.u);
}

fn push_vecs(o: &mut Vec<f64>, v: &[[f64; 3]]) {
    o.push(v.len() as f64);
    for x in v {
        o.extend_from_slice(x);
    }
}

fn push_vals(o: &mut Vec<f64>, v: &[f64]) {
    o.push(v.len() as f64);
    o.extend_from_slice(v);
}

fn push_cross(o: &mut Vec<f64>, s: &CrossSets) {
    o.push(s.pos.len() as f64);
    let mut flags = 0u32;
    for (b, x) in s.opt.iter().enumerate() {
        if x.is_some() {
            flags |= 1 << b;
        }
    }
    o.push(f64::from(flags));
    for x in &s.pos {
        o.extend_from_slice(x);
    }
    push_vecs(o, &s.d1);
    for x in s.opt.iter().flatten() {
        push_vecs(o, x);
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    match &i.this {
        None => o.push(0.0),
        Some(t) if t.base => o.push(1.0),
        Some(t) => {
            o.push(2.0);
            let b = t
                .bss
                .as_ref()
                .ok_or_else(|| harness("this: missing surface"))?;
            push_bss(&mut o, b)?;
        }
    }
    match &i.op {
        OpIn::Eval {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            nzt,
        } => {
            o.extend_from_slice(uv);
            o.extend([
                *hu as f64,
                *hv as f64,
                flag(*u_left),
                flag(*v_left),
                flag(*upper),
                flag(*nzt),
            ]);
        }
        OpIn::EvalPt { uv } | OpIn::ConvertUv { uv } => o.extend_from_slice(uv),
        OpIn::Queries | OpIn::Swap | OpIn::MaxAnal | OpIn::Copy | OpIn::CopyAnal => {}
        OpIn::Knots { dir, ivl } => {
            o.push(f64::from(*dir));
            o.push(flag(ivl.is_some()));
            o.extend_from_slice(&ivl.unwrap_or([0.0, 1.0]));
        }
        OpIn::Parallel { v } => o.extend_from_slice(v),
        OpIn::Reverse { dir } => o.push(f64::from(*dir)),
        OpIn::Reparam { dom } | OpIn::Trim { dom } | OpIn::AdjustStep { dom } => {
            o.extend_from_slice(dom);
        }
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.push(flag(scale.is_some()));
            o.extend_from_slice(&scale.unwrap_or([1.0, 1.0, 1.0]));
        }
        OpIn::Split { t, dir } => o.extend([*t, f64::from(*dir)]),
        OpIn::StepInv { dom, p, tol, guess } => {
            o.extend_from_slice(dom);
            o.extend_from_slice(p);
            o.push(*tol);
            o.push(flag(guess.is_some()));
            o.extend_from_slice(&guess.unwrap_or([0.0, 0.0]));
        }
        OpIn::Extend { dist, cont } => o.extend([*dist, f64::from(*cont)]),
        OpIn::ExtendDir { dir, dist, cont } => {
            o.extend([f64::from(*dir), *dist, f64::from(*cont)]);
        }
        OpIn::DropFast { dom, p } => {
            o.extend_from_slice(dom);
            o.extend_from_slice(p);
        }
        OpIn::SEval {
            ctor,
            uv,
            dir,
            steps,
        } => {
            o.push(f64::from(*ctor));
            o.extend_from_slice(uv);
            o.extend_from_slice(dir);
            o.push(steps.len() as f64);
            for s in steps {
                o.extend_from_slice(s);
            }
        }
        OpIn::Gssi {
            dom,
            other,
            other_dom,
            tol,
            translate,
        } => {
            o.extend_from_slice(dom);
            push_bss(&mut o, other)?;
            o.extend_from_slice(other_dom);
            o.extend([flag(tol.is_some()), tol.unwrap_or(0.0)]);
            o.push(flag(translate.is_some()));
            o.extend_from_slice(&translate.unwrap_or([0.0; 3]));
        }
        OpIn::Lift2 { v } | OpIn::Lift3 { v } | OpIn::Geom { e: v } => {
            for x in v {
                o.extend_from_slice(x);
            }
        }
        OpIn::ScaleStep {
            hu,
            hv,
            u_ang,
            v_ang,
            d,
        } => {
            o.extend([*hu as f64, *hv as f64, flag(*u_ang), flag(*v_ang)]);
            for x in d {
                o.extend_from_slice(x);
            }
        }
        OpIn::PointType {
            n,
            duu,
            dvv,
            want_uu,
            want_vv,
            side_uu,
        } => {
            o.extend_from_slice(n);
            o.extend_from_slice(duu);
            o.extend_from_slice(dvv);
            o.extend([flag(*want_uu), flag(*want_vv), f64::from(*side_uu)]);
        }
        OpIn::DropVecs { du, dv, v } => {
            o.extend_from_slice(du);
            o.extend_from_slice(dv);
            push_vecs(&mut o, v);
        }
        OpIn::ParamDelta { t, d } => {
            o.extend_from_slice(t);
            o.extend_from_slice(d);
        }
        OpIn::Merge { a, b } => {
            push_vals(&mut o, a);
            push_vals(&mut o, b);
        }
        OpIn::MapUv { uv, orient, dir } => {
            o.extend_from_slice(uv);
            o.push(f64::from(*orient));
            o.extend_from_slice(dir);
        }
        OpIn::IsEven { i } => o.push(f64::from(*i)),
        OpIn::Breaks { ivl, b } => {
            o.extend_from_slice(ivl);
            push_vals(&mut o, b);
        }
        OpIn::Limits { p } => o.extend_from_slice(p),
        OpIn::DerivAssert {
            samples,
            tt,
            dir_angles,
            tref,
            tan,
            angles,
            vector,
            ct,
            bet,
            radii,
            cref,
            flip,
            with_list,
        } => {
            o.extend([*samples as f64, f64::from(*tt)]);
            o.extend_from_slice(dir_angles);
            o.push(flag(*tref));
            o.extend_from_slice(tan);
            o.extend_from_slice(angles);
            o.extend_from_slice(vector);
            o.extend([f64::from(*ct), f64::from(*bet)]);
            o.extend_from_slice(radii);
            o.extend([flag(*cref), flag(*flip), flag(*with_list)]);
        }
        OpIn::Cross { sets } => push_cross(&mut o, sets),
        OpIn::CrossSkin {
            sets,
            params,
            rail1,
            rail2,
            tol,
        } => {
            push_cross(&mut o, sets);
            push_vals(&mut o, params);
            push_crv(&mut o, rail1);
            o.push(flag(rail2.is_some()));
            if let Some(r) = rail2 {
                push_crv(&mut o, r);
            }
            o.push(*tol);
        }
        OpIn::Dcf {
            status,
            from,
            path,
            ctx,
            dom,
            ivl,
            tols,
            flags,
            max_drop,
            closed,
            closure,
            sing,
            poles,
            normals,
            poly,
            knots,
            curves,
        } => {
            o.extend([
                f64::from(*status),
                *from as f64,
                f64::from(*path),
                flag(*ctx),
            ]);
            o.extend_from_slice(dom);
            o.extend_from_slice(ivl);
            o.extend_from_slice(tols);
            o.extend(flags.map(f64::from));
            o.push(*max_drop);
            o.extend(closed.map(f64::from));
            o.push(f64::from(*closure));
            o.extend(sing.map(|x| x as f64));
            push_vecs(&mut o, poles);
            push_vecs(&mut o, normals);
            push_vecs(&mut o, poly);
            push_vals(&mut o, knots);
            o.push(curves.len() as f64);
            for c in curves {
                push_crv(&mut o, c);
            }
        }
        OpIn::CcLocal {
            c1,
            i1,
            k1,
            c2,
            i2,
            k2,
            sop,
            tol,
        } => {
            push_crv(&mut o, c1);
            o.extend_from_slice(i1);
            o.extend(k1.map(f64::from));
            push_crv(&mut o, c2);
            o.extend_from_slice(i2);
            o.extend(k2.map(f64::from));
            o.extend([f64::from(*sop), *tol]);
        }
        OpIn::IsectSplit { c, ivl, pp, pn, s } => {
            push_crv(&mut o, c);
            o.extend_from_slice(ivl);
            o.extend_from_slice(pp);
            o.extend_from_slice(pn);
            push_vals(&mut o, s);
        }
        OpIn::IsectCurveSplit {
            c,
            ivl,
            lp,
            lv,
            livl,
            zero,
            s,
        } => {
            push_crv(&mut o, c);
            o.extend_from_slice(ivl);
            o.extend_from_slice(lp);
            o.extend_from_slice(lv);
            o.extend_from_slice(livl);
            o.push(*zero);
            push_vals(&mut o, s);
        }
        OpIn::Lsi {
            bbox,
            lp,
            lv,
            ivl,
            ray,
            tol,
        } => {
            o.extend_from_slice(&bbox[0]);
            o.extend_from_slice(&bbox[1]);
            o.extend_from_slice(lp);
            o.extend_from_slice(lv);
            o.push(flag(ivl.is_some()));
            o.extend_from_slice(&ivl.unwrap_or([0.0, 1.0]));
            o.extend([flag(*ray), *tol]);
        }
        OpIn::CsPerpen {
            caux,
            cbox,
            saux,
            sbox,
        } => {
            o.push(f64::from(*caux));
            o.extend_from_slice(&cbox[0]);
            o.extend_from_slice(&cbox[1]);
            o.push(f64::from(*saux));
            o.extend_from_slice(&sbox[0]);
            o.extend_from_slice(&sbox[1]);
        }
        OpIn::Plane3d { curves, tol } => {
            o.push(curves.len() as f64);
            for c in curves {
                push_crv(&mut o, c);
            }
            o.push(*tol);
        }
        OpIn::ReadDb {
            ty,
            binary,
            version,
            db,
        } => {
            let bytes = db_bytes(db)?;
            o.extend([
                *ty as f64,
                flag(*binary),
                *version as f64,
                bytes.len() as f64,
            ]);
            o.extend(bytes.iter().map(|&b| f64::from(b)));
        }
    }
    Ok((i.op.ffi(), o))
}

fn to_output(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("out", rest.to_vec())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    let flat = ffi::eval(op, &input).map_err(|e| harness(e.to_string()))?;
    Ok(to_output(&flat))
}

// ---- Rust side ----------------------------------------------------------------------

fn surface_of(s: &InlineSrf) -> Result<NlSurface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| {
            r.iter()
                .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                .collect()
        })
        .collect();
    NlSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))
}

fn bss_of(b: &BssIn) -> Result<BSplineSurface, HarnessError> {
    let mut s = BSplineSurface::from_nurb(surface_of(&srf(&b.srf)?)?, false);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

fn curve_of(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, c.dim, false))
}

fn param_of(v: u8) -> Result<SurfParamType, HarnessError> {
    SurfParamType::from_code(u64::from(v)).ok_or_else(|| harness(format!("bad param {v}")))
}

fn continuity_of(v: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match v {
        0 => C::Undefined,
        1 => C::Discontinuous,
        2 => C::C0,
        3 => C::G1,
        4 => C::G1R,
        5 => C::G1G2,
        6 => C::G1G2G3,
        7 => C::C1,
        8 => C::C1G2,
        9 => C::C1G2G3,
        10 => C::C1C2,
        11 => C::C1C2G3,
        12 => C::C1C2C3,
        13 => C::CInfinity,
        _ => return Err(harness(format!("bad continuity {v}"))),
    })
}

fn orient_of(v: u8) -> Result<OrientType, HarnessError> {
    Ok(match v {
        0 => OrientType::Unknown,
        1 => OrientType::Same,
        2 => OrientType::Opposite,
        _ => return Err(harness(format!("bad orient {v}"))),
    })
}

fn aux_of(v: u8) -> Result<AuxDataType, HarnessError> {
    Ok(match v {
        0 => AuxDataType::None,
        1 => AuxDataType::BezierCurve,
        2 => AuxDataType::CurveData,
        3 => AuxDataType::BezierSurface,
        4 => AuxDataType::BezierSurfaceHead,
        5 => AuxDataType::SurfaceData,
        6 => AuxDataType::AuxData,
        7 => AuxDataType::CubeAuxData,
        8 => AuxDataType::ObjectList,
        _ => return Err(harness(format!("bad aux type {v}"))),
    })
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn p2(p: [f64; 2]) -> Point2 {
    Point2::new(p[0], p[1])
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn box_of(b: [[f64; 3]; 2]) -> Extent3d {
    let mut e = Extent3d::new();
    e.add_point(v3(b[0]));
    e.add_point(v3(b[1]));
    e
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn push2(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

fn push_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn push_str(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn dump_bss(o: &mut Vec<f64>, s: &BSplineSurface) {
    o.extend([
        1.0,
        flag(s.gw_nurb().is_some()),
        flag(s.is_out_of_bounds_enabled()),
        flag(s.is_nurb_borrowed()),
        f64::from(s.knot_type() as i32),
        f64::from(s.form() as i32),
    ]);
    if let Some(sur) = s.gw_nurb() {
        let ix = sur.indices();
        o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
        for row in sur.rows() {
            for p in row {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        o.extend_from_slice(sur.u_knots());
        o.extend_from_slice(sur.v_knots());
    }
}

/// `put_state`: `0` for the base class, else the B-spline state (of a plane's B-spline
/// part).
fn dump_state(o: &mut Vec<f64>, s: Option<&Surface>) {
    match s.map(Surface::kind) {
        Some(sp2::SurfaceKind::BSpline(b)) => dump_bss(o, b),
        // an SmPlane is an SmBSplineSurface: its B-spline part
        Some(sp2::SurfaceKind::Plane(p)) => dump_bss(o, p.base()),
        // an SmSurfOfRevolution is an SmBSplineSurface: its NURB
        Some(sp2::SurfaceKind::SurfOfRevolution(r)) => dump_bss(o, r.base()),
        Some(sp2::SurfaceKind::SurfOfExtrusion(r)) => dump_bss(o, r.base()),
        Some(sp2::SurfaceKind::Cone(c)) => dump_bss(o, c.rev().base()),
        Some(sp2::SurfaceKind::Sphere(s)) => dump_bss(o, s.base().base()),
        Some(sp2::SurfaceKind::Torus(s)) => dump_bss(o, s.base().base()),
        // `put_state` of a surface that is not a B-spline
        Some(
            sp2::SurfaceKind::CurveBounded(_)
            | sp2::SurfaceKind::Step(_)
            | sp2::SurfaceKind::OffsetSurface(_)
            | sp2::SurfaceKind::SrfInVolume(_),
        )
        | None => o.push(0.0),
    }
}

/// `GlobalSurfaceIntersect`'s outputs: `n`, `same` (the arrays agree in length) and per
/// curve the 3D curve and the two UV curves (each its type code, `0` for none, then
/// [`dump_curve`] for a B-spline), the curve type and the deviation.
fn put_intersection(o: &mut Vec<f64>, r: &sp2::SurfaceIntersection) {
    let n = r.curves_3d.len();
    o.push(n as f64);
    let same = r.surface1_uv_curves.len() == n
        && r.surface2_uv_curves.len() == n
        && r.curve_types.len() == n
        && r.deviations.len() == n;
    o.push(flag(same));
    if !same {
        return;
    }
    let put = |o: &mut Vec<f64>, c: Option<&SmCurve>| match c {
        None => o.push(0.0),
        Some(c) => {
            o.push(c.type_code() as f64);
            if let CurveKind::BSpline(b) = c.kind() {
                dump_curve(o, b);
            }
        }
    };
    for (((c3, (u1, u2)), t), d) in r
        .curves_3d
        .iter()
        .zip(r.surface1_uv_curves.iter().zip(&r.surface2_uv_curves))
        .zip(&r.curve_types)
        .zip(&r.deviations)
    {
        put(o, Some(c3));
        put(o, u1.as_ref());
        put(o, u2.as_ref());
        o.push(f64::from(*t as i32));
        o.push(*d);
    }
}

fn dump_curve(o: &mut Vec<f64>, c: &BSplineCurve) {
    o.push(c.dim() as f64);
    o.push(flag(c.gw_nurb().is_some()));
    if let Some(cur) = c.gw_nurb() {
        o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
        for p in cur.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.extend_from_slice(cur.knots());
    }
}

/// The outcome of a Rust op: the record, or an operation error (its status ends the
/// record unless the op already wrote it).
type Run = Result<Vec<f64>, SolidError>;

/// Runs a fallible op whose record on failure is only its status.
fn record(f: impl FnOnce(&mut Vec<f64>) -> solidcore::Result<()>) -> Run {
    let mut o = vec![0.0];
    f(&mut o)?;
    Ok(o)
}

fn this_surface(t: &This) -> Result<Option<Surface>, HarnessError> {
    if t.base {
        return Ok(None);
    }
    let b = t
        .bss
        .as_ref()
        .ok_or_else(|| harness("this: missing surface"))?;
    Ok(Some(Surface::from(bss_of(b)?)))
}

fn base_err() -> SolidError {
    SolidError::sm_err("difftest: base class default")
}

/// An edit: status then the surface state (whatever the status).
fn edit(s: &mut Option<Surface>, f: impl FnOnce(&mut Surface) -> solidcore::Result<()>) -> Run {
    let r = match s.as_mut() {
        Some(x) => f(x),
        None => Err(base_err()),
    };
    let mut o = vec![match &r {
        Ok(()) => 0.0,
        Err(e) if e.is_unported() => return Err(r.err().unwrap_or_else(base_err)),
        Err(e) => status(e),
    }];
    dump_state(&mut o, s.as_ref());
    Ok(o)
}

fn rust_surface_op(k: &Kernel, op: &OpIn, t: &This) -> Result<Run, HarnessError> {
    let mut s = this_surface(t)?;
    let base = s.is_none();
    let run = match op {
        OpIn::Eval {
            uv,
            hu,
            hv,
            u_left,
            v_left,
            upper,
            nzt,
        } => record(|o| {
            let flags = EvaluateFlags {
                u_side: side(*u_left),
                v_side: side(*v_left),
                only_upper_half: *upper,
                non_zero_tangents: *nzt,
            };
            let d = match &s {
                Some(x) => x.evaluate(k, p2(*uv), *hu, *hv, flags)?,
                None => sp2::base_evaluate()?,
            };
            for i in 0..=*hu {
                for j in 0..=*hv {
                    if !*upper || i + j <= *hu {
                        let v = d
                            .get(i)
                            .and_then(|r| r.get(j))
                            .copied()
                            .ok_or_else(|| SolidError::sm_err("difftest: short matrix"))?;
                        push3(o, v);
                    }
                }
            }
            Ok(())
        }),
        OpIn::EvalPt { uv } => record(|o| {
            let p = match &s {
                Some(x) => x.evaluate_point(k, p2(*uv))?,
                None => sp2::base_evaluate_point()?,
            };
            push3(o, p);
            Ok(())
        }),
        OpIn::Queries => record(|o| {
            let types = [
                sp2::TYPE,
                sp2::B_SPLINE_SURFACE_TYPE,
                sp2::PLANE_TYPE,
                a_object::TYPE,
                OBJECT_TYPE,
                solidcore::curve::curve::p2::TYPE,
                sp2::CONE_TYPE,
                sp2::TORUS_TYPE,
            ];
            if let Some(x) = &s {
                {
                    push_dom(o, &x.natural_uv_domain());
                    push_dom(o, &x.step_uv_domain());
                    o.extend([
                        x.degree(SurfParamType::U) as f64,
                        x.degree(SurfParamType::V) as f64,
                        x.number_control_points(SurfParamType::U) as f64,
                        x.number_control_points(SurfParamType::V) as f64,
                        flag(x.is_bilinear()),
                        x.type_code() as f64,
                    ]);
                    push_str(o, x.type_string());
                    push_str(o, x.class_string());
                    for ty in types {
                        o.push(flag(x.is_kind_of(ty)?));
                    }
                }
            } else {
                {
                    push_dom(o, &base_natural_uv_domain());
                    push_dom(o, &base_natural_uv_domain());
                    o.extend([
                        base_degree() as f64,
                        base_degree() as f64,
                        base_number_control_points() as f64,
                        base_number_control_points() as f64,
                        flag(sp2::base_is_bilinear()),
                        sp2::TYPE as f64,
                    ]);
                    push_str(o, sp2::TYPE_STRING);
                    push_str(o, sp2::CLASS_STRING);
                    for ty in types {
                        o.push(flag(sp2::is_kind_of(ty)));
                    }
                }
            }
            Ok(())
        }),
        OpIn::Knots { dir, ivl } => {
            let dir = param_of(*dir)?;
            record(|o| {
                let ivl = ivl.map(|i| Extent1d::from_min_max(i[0], i[1]));
                let (k, m) = match &s {
                    Some(x) => x.knots(dir, ivl.as_ref())?,
                    None => sp2::base_knots()?,
                };
                o.push(k.len() as f64);
                o.extend_from_slice(&k);
                o.push(m.len() as f64);
                o.extend(m.iter().map(|&x| x as f64));
                Ok(())
            })
        }
        OpIn::Parallel { v } => record(|o| {
            let r = match &s {
                Some(x) => x.is_parallel_to_vector(v3(*v))?,
                None => sp2::base_is_parallel_to_vector(),
            };
            o.push(flag(r));
            Ok(())
        }),
        OpIn::Reverse { dir } => {
            let dir = param_of(*dir)?;
            edit(&mut s, |x| x.reverse(&Kernel::default(), dir))
        }
        OpIn::Reparam { dom } => edit(&mut s, |x| {
            x.reparameterize(&Kernel::default(), &dom_of(*dom))
        }),
        OpIn::Swap => edit(&mut s, |x| x.swap_uv(&Kernel::default())),
        OpIn::Transform {
            origin,
            x,
            y,
            scale,
        } => {
            let ax = Axis2Placement::new(v3(*origin), v3(*x), v3(*y));
            edit(&mut s, |srf| {
                srf.transform(&Kernel::default(), &ax, scale.map(v3))
            })
        }
        OpIn::Trim { dom } => {
            let mut d = dom_of(*dom);
            let r = match s.as_mut() {
                Some(x) => x.trim_with_domain(&Kernel::default(), &mut d),
                None => Err(base_err()),
            };
            match r {
                Err(e) if e.is_unported() => Err(e),
                r => {
                    let mut o = vec![r.as_ref().map_or_else(status, |()| 0.0)];
                    push_dom(&mut o, &d);
                    dump_state(&mut o, s.as_ref());
                    Ok(o)
                }
            }
        }
        OpIn::Split { t, dir } => {
            let dir = param_of(*dir)?;
            record(|o| {
                let (l, r) = match &s {
                    Some(x) => x.split_at(&Kernel::default(), *t, dir)?,
                    None => sp2::base_split_at()?,
                };
                dump_state(o, Some(&l));
                dump_state(o, Some(&r));
                Ok(())
            })
        }
        OpIn::StepInv { dom, p, tol, guess } => record(|o| {
            let (uv, loc) = match &s {
                Some(x) => x.step_inversion(
                    &Kernel::default(),
                    &dom_of(*dom),
                    v3(*p),
                    *tol,
                    guess.map(p2),
                )?,
                None => sp2::base_step_inversion()?,
            };
            push2(o, uv);
            o.push(f64::from(loc as i32));
            Ok(())
        }),
        OpIn::Extend { dist, cont } => {
            let cont = continuity_of(*cont)?;
            record(|o| {
                let e = match &s {
                    Some(x) => x.create_extended_surface(k, *dist, cont)?,
                    None => sp2::base_create_extended_surface()?,
                };
                dump_state(o, Some(&e));
                Ok(())
            })
        }
        OpIn::ExtendDir { dir, dist, cont } => {
            let cont = continuity_of(*cont)?;
            let dir = param_of(*dir)?;
            record(|o| {
                let e = match &s {
                    Some(x) => x.create_extended_surface_dir(k, dir, *dist, cont)?,
                    None => sp2::base_create_extended_surface()?,
                };
                dump_state(o, Some(&e));
                Ok(())
            })
        }
        OpIn::AdjustStep { dom } => edit(&mut s, |x| {
            x.adjust_step_uv_domain(&Kernel::default(), &dom_of(*dom))
        }),
        OpIn::MaxAnal => record(|o| {
            let d = match &s {
                Some(x) => x.max_analytic_domain(k)?,
                None => sp2::base_max_analytic_domain(),
            };
            push_dom(o, &d);
            Ok(())
        }),
        OpIn::Copy | OpIn::CopyAnal => {
            let anal = matches!(op, OpIn::CopyAnal);
            record(|o| {
                let c = match &s {
                    Some(x) if anal => x.copy_and_add_analytics(k)?,
                    Some(x) => x.copy(k)?,
                    None if anal => sp2::base_copy_and_add_analytics()?,
                    None => sp2::base_copy()?,
                };
                o.push(c.type_code() as f64);
                dump_state(o, Some(&c));
                Ok(())
            })
        }
        OpIn::DropFast { dom, p } => record(|_| {
            match &s {
                Some(x) => x.drop_point_fast::<Handle>(
                    &Kernel::default(),
                    &dom_of(*dom),
                    SolverOperation::Minimize,
                    v3(*p),
                    None,
                    1.0e-6,
                    None,
                    solidcore::geom::global_solver::SolutionRequested::All,
                )?,
                None => sp2::base_drop_point_fast::<Handle>()?,
            };
            Ok(())
        }),
        OpIn::ConvertUv { uv } => record(|o| {
            let uv = p2(*uv);
            let (a, b) = match &s {
                Some(x) => (
                    x.convert_uv_from_step_to_nurbs(&Kernel::default(), uv)?,
                    x.convert_uv_from_nurbs_to_step(&Kernel::default(), uv)?,
                ),
                None => (base_convert_uv(uv), base_convert_uv(uv)),
            };
            push2(o, a);
            push2(o, b);
            Ok(())
        }),
        OpIn::SEval {
            ctor,
            uv,
            dir,
            steps,
        } => {
            if base {
                return Err(harness("surface evaluator on the base class"));
            }
            record(|o| {
                let srf = s.as_ref();
                let mut e = match ctor {
                    0 => SurfaceEval::new(k, srf),
                    1 => SurfaceEval::with_uv(k, srf, uv[0], uv[1]),
                    _ => SurfaceEval::with_uv_and_sect_dir(k, srf, uv[0], uv[1], v3(*dir)),
                };
                let put = |o: &mut Vec<f64>, e: &SurfaceEval<'_>| {
                    o.extend(e.flags().map(flag));
                    push2(o, e.uv());
                };
                put(o, &e);
                for st in steps {
                    match st[0] as u8 {
                        0 => e.set_uv(Point2::new(st[1], st[2])),
                        1 => e.set_uv_coords(st[1], st[2]),
                        2 => e.set_sect_dir(Vec3::new(st[1], st[2], st[3])),
                        _ => match e.eval() {
                            Ok(()) => {
                                o.push(0.0);
                                let pvv = e.values();
                                for (i, row) in pvv.iter().enumerate() {
                                    for (j, v) in row.iter().enumerate() {
                                        if i + j <= 2 {
                                            push3(o, *v);
                                        }
                                    }
                                }
                            }
                            Err(err) if err.is_unported() => return Err(err),
                            Err(err) => o.push(status(&err)),
                        },
                    }
                    put(o, &e);
                }
                Ok(())
            })
        }
        OpIn::Gssi {
            dom,
            other,
            other_dom,
            tol,
            translate,
        } => {
            let mut other = Surface::from(bss_of(other)?);
            let mut this = s.clone();
            record(|o| {
                let k = Kernel::default();
                let x = this.as_mut().ok_or_else(base_err)?;
                if let Some(t) = translate {
                    let mut ax = Axis2Placement::default();
                    ax.translate(v3(*t));
                    x.transform(&k, &ax, None)?;
                    other.transform(&k, &ax, None)?;
                }
                let r = x.global_surface_intersect(
                    &k,
                    &dom_of(*dom),
                    &other,
                    &dom_of(*other_dom),
                    [true, true],
                    *tol,
                    None,
                )?;
                put_intersection(o, &r);
                Ok(())
            })
        }
        _ => return Err(harness("not a surface op")),
    };
    Ok(run)
}

fn cross_values(s: &CrossSets) -> (Vec<Vec3>, Vec<Vec3>, [Option<Vec<Vec3>>; 6]) {
    let conv = |v: &Vec<[f64; 3]>| v.iter().copied().map(v3).collect::<Vec<_>>();
    (
        conv(&s.pos),
        conv(&s.d1),
        s.opt.clone().map(|x| x.as_ref().map(conv)),
    )
}

fn rust_static_op(op: &OpIn) -> Result<Run, HarnessError> {
    let k = Kernel::default();
    let run = match op {
        OpIn::Lift2 { v } => {
            let a: Vec<Vec3> = v.iter().copied().map(v3).collect();
            let g = |i: usize| a.get(i).copied().ok_or_else(|| harness("lift2: 7 vectors"));
            let r = lift_second_derivative(g(0)?, g(1)?, g(2)?, g(3)?, g(4)?, g(5)?, g(6)?);
            Ok(vec![0.0, r.x, r.y, r.z])
        }
        OpIn::Lift3 { v } => {
            let a: Vec<Vec3> = v.iter().copied().map(v3).collect();
            let g = |i: usize| {
                a.get(i)
                    .copied()
                    .ok_or_else(|| harness("lift3: 12 vectors"))
            };
            let r = lift_third_derivative(
                g(0)?,
                g(1)?,
                g(2)?,
                g(3)?,
                g(4)?,
                g(5)?,
                g(6)?,
                g(7)?,
                g(8)?,
                g(9)?,
                g(10)?,
                g(11)?,
            );
            Ok(vec![0.0, r.x, r.y, r.z])
        }
        OpIn::ScaleStep {
            hu,
            hv,
            u_ang,
            v_ang,
            d,
        } => record(|o| {
            let mut a: Vec<Vec3> = d.iter().copied().map(v3).collect();
            scale_step_angular_derivatives(&mut a, *hu, *hv, *u_ang, *v_ang)?;
            for x in a {
                push3(o, x);
            }
            Ok(())
        }),
        OpIn::Geom { e } => {
            let mut m = [[Vec3::UNDEF; 3]; 3];
            for (idx, x) in e.iter().enumerate() {
                if let Some(slot) = m.get_mut(idx / 3).and_then(|r| r.get_mut(idx % 3)) {
                    *slot = v3(*x);
                }
            }
            record(|o| {
                let g = evaluate_geometric(&mut m)?;
                o.extend([
                    g.gaussian_curvature,
                    g.normal_curvature,
                    g.principle_curvature1,
                    g.principle_curvature2,
                ]);
                push3(o, g.efg_of_first_fund_form);
                push3(o, g.lmn_of_second_fund_form);
                push3(o, g.principle_curvature_vector1);
                push3(o, g.principle_curvature_vector2);
                push3(o, m[1][0]);
                push3(o, m[0][1]);
                Ok(())
            })
        }
        OpIn::PointType {
            n,
            duu,
            dvv,
            want_uu,
            want_vv,
            side_uu,
        } => {
            let mut su = SurfaceSideType::from_code(u64::from(*side_uu))
                .ok_or_else(|| harness("bad side"))?;
            let t = compute_surface_point_type(
                v3(*n),
                v3(*duu),
                v3(*dvv),
                if *want_uu { Some(&mut su) } else { None },
                *want_vv,
            );
            Ok(vec![0.0, f64::from(t as i32), f64::from(su as i32)])
        }
        OpIn::DropVecs { du, dv, v } => record(|o| {
            let vs: Vec<Vec3> = v.iter().copied().map(v3).collect();
            for x in drop_vectors(v3(*du), v3(*dv), &vs)? {
                push2(o, x);
            }
            Ok(())
        }),
        OpIn::ParamDelta { t, d } => Ok(vec![0.0, convert_to_parameter_delta(v3(*t), v3(*d))]),
        OpIn::Merge { a, b } => {
            let r = merge_double_arrays(a, b);
            let mut o = vec![0.0, r.len() as f64];
            o.extend(r);
            Ok(o)
        }
        OpIn::MapUv { uv, orient, dir } => {
            let r = map_uv(uv[0], uv[1], orient_of(*orient)?, dir[0], dir[1]);
            Ok(vec![0.0, r.x, r.y])
        }
        OpIn::IsEven { i } => Ok(vec![0.0, flag(is_even(*i))]),
        OpIn::Breaks { ivl, b } => record(|o| {
            let mut b = b.clone();
            set_up_breaks(&mut b, &Extent1d::from_min_max(ivl[0], ivl[1]))?;
            o.push(b.len() as f64);
            o.extend(b);
            Ok(())
        }),
        OpIn::Limits { p } => Ok(vec![0.0, derivative_limit(v3(*p)), normal_limit()]),
        OpIn::DerivAssert {
            samples,
            tt,
            dir_angles,
            tref,
            tan,
            angles,
            vector,
            ct,
            bet,
            radii,
            cref,
            flip,
            with_list,
        } => {
            let dummy = SmCurve::from(BSplineCurve::new(3));
            let d = DerivSurfDefinition {
                num_samples: *samples,
                tangency_type: TangencyType::from_code(u64::from(*tt))
                    .ok_or_else(|| harness("bad tangency"))?,
                start_deriv_dir_angle_deg: dir_angles[0],
                end_deriv_dir_angle_deg: dir_angles[1],
                tangent_reference: tref.then_some(&dummy),
                start_tan_length: tan[0],
                end_tan_scale: tan[1],
                start_angle_deg: angles[0],
                end_angle_deg: angles[1],
                vector: v3(*vector),
                curvature_type: CurvatureType::from_code(u64::from(*ct))
                    .ok_or_else(|| harness("bad curvature type"))?,
                blend_end_type: BlendEndType::from_code(u64::from(*bet))
                    .ok_or_else(|| harness("bad blend end"))?,
                start_radius: radii[0],
                end_rad_scale: radii[1],
                curvature_reference: cref.then_some(&dummy),
                flip_curvature: *flip,
            };
            record(|o| {
                let mut list = AssertArray::new();
                let ok = d.assert_valid(with_list.then_some(&mut list), ObjectKey(1))?;
                o.push(flag(ok));
                o.push(list.len() as f64);
                for r in list.reports() {
                    o.extend([r.test_index as f64, r.tol3d, r.value]);
                }
                o.push(flag(d.is_from_surface_boundary_curve()));
                o.push(d.num_derivs() as f64);
                o.push(d.class_type() as f64);
                push_str(o, d.class_string());
                let def = DerivSurfDefinition::default();
                o.extend([
                    def.num_samples as f64,
                    f64::from(def.tangency_type as i32),
                    f64::from(def.curvature_type as i32),
                    f64::from(def.blend_end_type as i32),
                    def.start_tan_length,
                    def.end_tan_scale,
                ]);
                push3(o, def.vector);
                Ok(())
            })
        }
        OpIn::Cross { sets } => {
            let (pos, d1, opt) = cross_values(sets);
            let v = CrossSectionValues {
                start_pos: &pos,
                start_1st_derivs: &d1,
                start_2nd_derivs: opt[0].as_deref(),
                start_3rd_derivs: opt[1].as_deref(),
                end_pos: opt[2].as_deref(),
                end_1st_derivs: opt[3].as_deref(),
                end_2nd_derivs: opt[4].as_deref(),
                end_3rd_derivs: opt[5].as_deref(),
            };
            record(|o| {
                let secs = DerivSurfDefinition::default().create_cross_section_curves(&v)?;
                o.push(secs.len() as f64);
                for c in &secs {
                    dump_curve(o, c);
                }
                Ok(())
            })
        }
        OpIn::CrossSkin {
            sets,
            params,
            rail1,
            rail2,
            tol,
        } => {
            let (pos, d1, opt) = cross_values(sets);
            let v = CrossSectionValues {
                start_pos: &pos,
                start_1st_derivs: &d1,
                start_2nd_derivs: opt[0].as_deref(),
                start_3rd_derivs: opt[1].as_deref(),
                end_pos: opt[2].as_deref(),
                end_1st_derivs: opt[3].as_deref(),
                end_2nd_derivs: opt[4].as_deref(),
                end_3rd_derivs: opt[5].as_deref(),
            };
            let mut r1 = curve_of(rail1)?;
            let mut r2 = rail2.as_ref().map(curve_of).transpose()?;
            let d = DerivSurfDefinition::default();
            match d.create_cross_section_curves(&v) {
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status(&e)]),
                Ok(secs) => {
                    match d.skin_deriv_surface(&k, &secs, params, &mut r1, r2.as_mut(), *tol) {
                        Err(e) if e.is_unported() => Err(e),
                        Err(e) => Ok(vec![0.0, status(&e)]),
                        Ok(skin) => {
                            let mut o = vec![0.0, 0.0];
                            dump_bss(&mut o, &skin);
                            Ok(o)
                        }
                    }
                }
            }
        }
        OpIn::Dcf {
            status: st,
            from,
            path,
            ctx,
            dom,
            ivl,
            tols,
            flags,
            max_drop,
            closed,
            closure,
            sing,
            poles,
            normals,
            poly,
            knots,
            curves,
        } => {
            let poles: Vec<Vec3> = poles.iter().copied().map(v3).collect();
            let normals: Vec<Vec3> = normals.iter().copied().map(v3).collect();
            let poly: Vec<Vec3> = poly.iter().copied().map(v3).collect();
            let curves = curves.iter().map(curve_of).collect::<Result<Vec<_>, _>>()?;
            let mut f: DropCurveFail<Handle> = DropCurveFail::default();
            let mut o = vec![0.0, flag(f.is_un_init())];
            let args = DropCurveFailArgs {
                fail_status: *st,
                from_call: *from,
                create_path_type: CreateUvTrimCurvePathType::from_code(u64::from(*path))
                    .ok_or_else(|| harness("bad path type"))?,
                has_context: *ctx,
                uv_domain: dom_of(*dom),
                surface: None,
                curve: None,
                interval: Extent1d::from_min_max(ivl[0], ivl[1]),
                approx_tol_3d: approx_tol3d(&k, tols[0]),
                max_drop_to_surf_arg: tols[1],
                max_approx_dev: tols[2],
                uv_curves_arg: None,
                keep_all_drop_curves: i32::from(flags[0]),
                degen_curve: i32::from(flags[1]),
                degen_surf_u: i32::from(flags[2]),
                degen_surf_v: i32::from(flags[3]),
                drop_domain_is_contained: i32::from(flags[4]),
                max_drop_to_surf: *max_drop,
                closed_u: i32::from(closed[0]),
                closed_v: i32::from(closed[1]),
                srf_closure: param_of(*closure)?,
                srf_pole_points: Some(&poles),
                srf_pole_normals: Some(&normals),
                singularities: sing[0],
                approx_singularities: sing[1],
                solutions: None,
                ctrl_poly: Some(&poly),
                knots: Some(knots),
                uv_curves: Some(&curves),
            };
            f.set(&args);
            let dump = |o: &mut Vec<f64>, g: &DropCurveFail<Handle>| {
                o.extend([
                    f64::from(g.fail_status),
                    g.from_call as f64,
                    f64::from(g.create_path_type as i32),
                    flag(g.has_context),
                ]);
                push_dom(o, &g.uv_domain);
                o.extend([
                    g.interval.min(),
                    g.interval.max(),
                    g.approx_tol_3d,
                    g.max_drop_to_surf_arg,
                    g.max_approx_dev,
                    f64::from(g.keep_all_drop_curves),
                    f64::from(g.degen_curve),
                    f64::from(g.degen_surf_u),
                    f64::from(g.degen_surf_v),
                    f64::from(g.drop_domain_is_contained),
                    g.max_drop_to_surf,
                    g.solutions.len() as f64,
                    g.uv_curves.len() as f64,
                ]);
                for c in &g.uv_curves {
                    dump_curve(o, c);
                }
                o.extend([
                    f64::from(g.closed_u),
                    f64::from(g.closed_v),
                    f64::from(g.srf_closure as i32),
                    g.srf_pole_points.len() as f64,
                ]);
                for p in &g.srf_pole_points {
                    push3(o, *p);
                }
                o.push(g.srf_pole_normals.len() as f64);
                for p in &g.srf_pole_normals {
                    push3(o, *p);
                }
                o.extend([g.singularities as f64, g.approx_singularities as f64]);
                o.push(g.ctrl_poly.len() as f64);
                for p in &g.ctrl_poly {
                    push3(o, *p);
                }
                o.push(g.knots.len() as f64);
                o.extend_from_slice(&g.knots);
            };
            o.push(flag(f.is_un_init()));
            dump(&mut o, &f);
            let mut g = DropCurveFail::default();
            g.assign(&f);
            dump(&mut o, &g);
            f.un_init();
            o.push(flag(f.is_un_init()));
            dump(&mut o, &f);
            Ok(o)
        }
        OpIn::CcLocal {
            c1,
            i1,
            k1,
            c2,
            i2,
            k2,
            sop,
            tol,
        } => {
            let a = SmCurve::from(curve_of(c1)?);
            let b = SmCurve::from(curve_of(c2)?);
            let sop = SolverOperation::from_code(i32::from(*sop))
                .ok_or_else(|| harness("bad solver op"))?;
            let ka = [continuity_of(k1[0])?, continuity_of(k1[1])?];
            let kb = [continuity_of(k2[0])?, continuity_of(k2[1])?];
            record(|o| {
                let sols: SolutionArray<Handle> = cc_local_solve(
                    &k,
                    &a,
                    &Extent1d::from_min_max(i1[0], i1[1]),
                    ka,
                    &b,
                    &Extent1d::from_min_max(i2[0], i2[1]),
                    kb,
                    sop,
                    *tol,
                )?;
                o.push(sols.len() as f64);
                for s in sols.as_slice() {
                    o.extend([
                        s.num_variables as f64,
                        f64::from(s.solution_type as i32),
                        s.start.solution_value,
                        s.start.parameters[0],
                        s.start.parameters[1],
                    ]);
                }
                Ok(())
            })
        }
        OpIn::IsectSplit { c, ivl, pp, pn, s } => {
            let c = SmCurve::from(curve_of(c)?);
            record(|o| {
                let mut s = s.clone();
                intersect_and_split(
                    &k,
                    &c,
                    &Extent1d::from_min_max(ivl[0], ivl[1]),
                    v3(*pp),
                    v3(*pn),
                    Some(CurveHandle(c.natural_interval())),
                    &mut s,
                )?;
                o.push(s.len() as f64);
                o.extend(s);
                Ok(())
            })
        }
        OpIn::IsectCurveSplit {
            c,
            ivl,
            lp,
            lv,
            livl,
            zero,
            s,
        } => {
            let c = SmCurve::from(curve_of(c)?);
            let livl = Extent1d::from_min_max(livl[0], livl[1]);
            let line = Line::bounded(
                v3(*lp),
                v3(*lv),
                &livl,
                &BoundedLineOptions {
                    scale: 1.0,
                    dim: 2,
                    ..BoundedLineOptions::default()
                },
            );
            record(|o| {
                let mut s = s.clone();
                intersect_curve_and_split::<Handle>(
                    &k,
                    &c,
                    &Extent1d::from_min_max(ivl[0], ivl[1]),
                    &line,
                    &livl,
                    *zero,
                    &mut s,
                )?;
                o.push(s.len() as f64);
                o.extend(s);
                Ok(())
            })
        }
        OpIn::Lsi {
            bbox,
            lp,
            lv,
            ivl,
            ray,
            tol,
        } => {
            let bx = box_of(*bbox);
            let tree: Tree<Handle> = Tree::new_spatial(&bx);
            let top = *tree
                .all_tree_nodes()
                .first()
                .ok_or_else(|| harness("empty tree"))?;
            let solver = LsiGlobalSolver {
                base: GlobalSolver::with_tree(&tree, 2),
                surface: None,
                uv_domain: Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
                line_point: v3(*lp),
                line_vector: v3(*lv),
                line_interval: ivl.map(|i| Extent1d::from_min_max(i[0], i[1])),
                ray_fire: *ray,
                distance_tolerance: *tol,
                surface_cache: None,
                face_queries: None,
                face_cache: None,
                kernel: &k,
            };
            let mut branch: Branch = Default::default();
            if let Some(slot) = branch.first_mut() {
                *slot = Some(top);
            }
            record(|o| {
                o.push(flag(solver.may_contain_answers(&branch)?));
                Ok(())
            })
        }
        OpIn::CsPerpen {
            caux,
            cbox,
            saux,
            sbox,
        } => {
            let tree: Tree<Handle> = Tree::new_spatial(&box_of(*cbox));
            let base_node = *tree.nodes().first().ok_or_else(|| harness("empty tree"))?;
            let cn = TreeNode {
                aux_type: aux_of(*caux)?,
                bbox: box_of(*cbox),
                ..base_node
            };
            let sn = TreeNode {
                aux_type: aux_of(*saux)?,
                bbox: box_of(*sbox),
                ..base_node
            };
            record(|o| {
                o.push(flag(cs_are_perpen_disjoint(&tree, &cn, &sn)?));
                Ok(())
            })
        }
        OpIn::Plane3d { curves, tol } => {
            let cs = curves
                .iter()
                .map(|c| curve_of(c).map(SmCurve::from))
                .collect::<Result<Vec<_>, _>>()?;
            record(|_| {
                Surface::create_plane_with_3d_curves(&k, &cs, *tol)?;
                Ok(())
            })
        }
        OpIn::ReadDb {
            ty,
            binary,
            version,
            db,
        } => {
            let bytes = db_bytes(db)?;
            let mut r = DatabaseReader::new();
            r.open_for_read(
                bytes,
                if *binary {
                    FileType::Binary
                } else {
                    FileType::Ascii
                },
            );
            record(|o| {
                let s = Surface::read_from_db(&Kernel::default(), *ty, &mut r, *version)?;
                dump_state(o, Some(&s));
                Ok(())
            })
        }
        _ => return Err(harness("not a static op")),
    };
    Ok(run)
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    let run = match &i.this {
        Some(t) => rust_surface_op(k, &i.op, t)?,
        None => rust_static_op(&i.op)?,
    };
    Ok(match run {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

// ---- generated cases ----------------------------------------------------------------

fn gen_knots(rng: &mut Rng, p: usize, n: usize) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    if rng.chance(0.2) && interior >= 2 {
        if let Some(&a) = inner.first() {
            if let Some(b) = inner.get_mut(1) {
                *b = a;
            }
        }
    }
    inner.sort_by(f64::total_cmp);
    let (a, b) = if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-2.0, 2.0);
        (a, a + rng.uniform(0.5, 3.0))
    };
    let mut k = vec![a; p + 1];
    k.extend(inner.iter().map(|t| a + t * (b - a)));
    k.extend(vec![b; p + 1]);
    k
}

fn gen_srf(rng: &mut Rng, max_deg: usize) -> InlineSrf {
    let p = 1 + rng.below(max_deg as u64) as usize;
    let q = 1 + rng.below(max_deg as u64) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let amp = rng.uniform(0.0, 1.0);
    let rational = rng.chance(0.2);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let x = i as f64 + rng.uniform(-0.2, 0.2);
                    let y = j as f64 + rng.uniform(-0.2, 0.2);
                    let z = amp * rng.uniform(-1.0, 1.0);
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [x * w, y * w, z * w, w]
                    } else {
                        [x, y, z, NOW]
                    }
                })
                .collect()
        })
        .collect();
    InlineSrf {
        net,
        p,
        q,
        u: gen_knots(rng, p, n),
        v: gen_knots(rng, q, m),
    }
}

fn gen_bss(rng: &mut Rng) -> BssIn {
    BssIn {
        oob: rng.chance(0.1),
        form: if rng.chance(0.85) {
            11
        } else {
            rng.below(10) as u8
        },
        srf: SrfIn::Inline(gen_srf(rng, 3)),
    }
}

fn gen_curve(rng: &mut Rng, dim: u64) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let rational = rng.chance(0.15);
    let cpts = (0..=n)
        .map(|i| {
            let x = i as f64 + rng.uniform(-0.3, 0.3);
            let y = rng.uniform(-1.0, 1.0);
            let z = if dim == 2 {
                0.0
            } else {
                rng.uniform(-1.0, 1.0)
            };
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, z * w, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect();
    CrvIn {
        dim,
        cpts,
        p,
        u: gen_knots(rng, p, n),
    }
}

fn knot_range(u: &[f64]) -> (f64, f64) {
    (
        u.first().copied().unwrap_or(0.0),
        u.last().copied().unwrap_or(1.0),
    )
}

fn sub_ivl(rng: &mut Rng, (a, b): (f64, f64)) -> [f64; 2] {
    if rng.chance(0.4) {
        return [a, b];
    }
    let x = a + rng.uniform(0.0, 0.4) * (b - a);
    let y = b - rng.uniform(0.0, 0.4) * (b - a);
    [x, y]
}

fn gvec(rng: &mut Rng, s: f64) -> [f64; 3] {
    let p = rng.point3(-s, s);
    if rng.chance(0.05) {
        [0.0, 0.0, 0.0]
    } else {
        p
    }
}

fn domain_of_srf(s: &InlineSrf) -> [f64; 4] {
    let (u0, u1) = knot_range(&s.u);
    let (v0, v1) = knot_range(&s.v);
    [u0, v0, u1, v1]
}

fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let pick = |rng: &mut Rng, a: f64, b: f64| {
        if rng.chance(0.15) {
            if rng.chance(0.5) {
                a
            } else {
                b
            }
        } else {
            rng.uniform(a, b)
        }
    };
    [pick(rng, d[0], d[2]), pick(rng, d[1], d[3])]
}

fn gen_sub_dom(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    let [u0, v0, u1, v1] = d;
    let du = u1 - u0;
    let dv = v1 - v0;
    [
        u0 + rng.uniform(0.0, 0.3) * du,
        v0 + rng.uniform(0.0, 0.3) * dv,
        u1 - rng.uniform(0.0, 0.3) * du,
        v1 - rng.uniform(0.0, 0.3) * dv,
    ]
}

fn gen_surface_case(rng: &mut Rng) -> Inputs {
    let base = rng.chance(0.12);
    let bss = gen_bss(rng);
    let d = match &bss.srf {
        SrfIn::Inline(s) => domain_of_srf(s),
        SrfIn::File { .. } => [0.0, 0.0, 1.0, 1.0],
    };
    let this = This {
        base,
        bss: (!base).then_some(bss),
    };
    let op = match rng.below(22) {
        0 | 1 => {
            let upper = rng.chance(0.3);
            let hu = rng.below(4) as usize;
            OpIn::Eval {
                uv: gen_uv(rng, d),
                hu,
                hv: if upper { hu } else { rng.below(4) as usize },
                u_left: rng.chance(0.5),
                v_left: rng.chance(0.5),
                upper,
                nzt: rng.chance(0.7),
            }
        }
        2 => OpIn::EvalPt { uv: gen_uv(rng, d) },
        4 => OpIn::Knots {
            dir: rng.below(2) as u8,
            ivl: rng.chance(0.3).then(|| {
                let s = gen_sub_dom(rng, d);
                [s[0], s[2]]
            }),
        },
        5 => OpIn::Parallel { v: gvec(rng, 1.0) },
        6 => OpIn::Reverse {
            dir: rng.below(2) as u8,
        },
        7 => OpIn::Reparam {
            dom: gen_sub_dom(rng, [-1.0, -1.0, 3.0, 3.0]),
        },
        8 => OpIn::Swap,
        9 => OpIn::Transform {
            origin: rng.point3(-2.0, 2.0),
            x: [1.0, rng.uniform(-0.5, 0.5), 0.0],
            y: [rng.uniform(-0.5, 0.5), 1.0, rng.uniform(-0.5, 0.5)],
            scale: rng.chance(0.4).then(|| {
                let s = rng.uniform(0.5, 2.0);
                if rng.chance(0.5) {
                    [s, s, s]
                } else {
                    [s, rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)]
                }
            }),
        },
        10 => OpIn::Trim {
            dom: gen_sub_dom(rng, d),
        },
        11 => {
            let dir = rng.below(2) as u8;
            let (a, b) = if dir == 0 { (d[0], d[2]) } else { (d[1], d[3]) };
            OpIn::Split {
                t: a + rng.uniform(0.1, 0.9) * (b - a),
                dir,
            }
        }
        12 => OpIn::StepInv {
            dom: d,
            p: rng.point3(-1.0, 3.0),
            tol: 1.0e-6,
            guess: rng.chance(0.3).then(|| gen_uv(rng, d)),
        },
        13 => OpIn::Extend {
            dist: rng.uniform(0.05, 0.5),
            cont: *rng.choose(&[2u8, 3, 4, 5, 13]).unwrap_or(&3),
        },
        14 => OpIn::ExtendDir {
            dir: *rng.choose(&[0u8, 1, 3, 5, 6, 7, 8]).unwrap_or(&0),
            dist: rng.uniform(0.05, 0.5),
            cont: *rng.choose(&[2u8, 3, 4, 5, 13]).unwrap_or(&3),
        },
        15 => OpIn::AdjustStep {
            dom: gen_sub_dom(rng, d),
        },
        16 => OpIn::MaxAnal,
        17 => {
            if rng.chance(0.7) {
                OpIn::Copy
            } else {
                OpIn::CopyAnal
            }
        }
        18 => OpIn::DropFast {
            dom: d,
            p: rng.point3(-1.0, 2.0),
        },
        19 => OpIn::ConvertUv { uv: gen_uv(rng, d) },
        20 if !base => {
            let k = 1 + rng.below(5) as usize;
            let uv0 = gen_uv(rng, d);
            let steps = (0..k)
                .map(|_| {
                    let kind = rng.below(5).min(3);
                    let uv = if rng.chance(0.3) { uv0 } else { gen_uv(rng, d) };
                    let c = rng.uniform(-1.0, 1.0);
                    [kind as f64, uv[0], uv[1], c]
                })
                .collect();
            OpIn::SEval {
                ctor: rng.below(3) as u8,
                uv: uv0,
                dir: gvec(rng, 1.0),
                steps,
            }
        }
        _ => OpIn::Queries,
    };
    Inputs {
        this: Some(this),
        op,
    }
}

fn gen_static_case(rng: &mut Rng) -> Inputs {
    let op = match rng.below(24) {
        0 => OpIn::Lift2 {
            v: (0..7).map(|_| gvec(rng, 2.0)).collect(),
        },
        1 => OpIn::Lift3 {
            v: (0..12).map(|_| gvec(rng, 2.0)).collect(),
        },
        2 => {
            let hu = rng.below(4) as usize;
            let hv = rng.below(4) as usize;
            OpIn::ScaleStep {
                hu,
                hv,
                u_ang: rng.chance(0.5),
                v_ang: rng.chance(0.5),
                d: (0..(hu + 1) * (hv + 1)).map(|_| gvec(rng, 3.0)).collect(),
            }
        }
        3..=5 => {
            let mut e: [[f64; 3]; 9] = std::array::from_fn(|_| gvec(rng, 2.0));
            match rng.below(5) {
                0 => {
                    // umbilic-ish: sphere point
                    e = [[0.0, 0.0, 1.0]; 9];
                    e[1] = [0.0, 1.0, 0.0];
                    e[3] = [1.0, 0.0, 0.0];
                    e[2] = [0.0, 0.0, -1.0];
                    e[6] = [0.0, 0.0, -1.0];
                    e[4] = [0.0, 0.0, 0.0];
                }
                1 => {
                    // singular: zero Du, normal given
                    e[3] = [0.0, 0.0, 0.0];
                    e[8] = gvec(rng, 1.0);
                }
                2 => {
                    // planar
                    e[2] = [rng.uniform(-1.0, 1.0), 0.0, 0.0];
                    e[4] = [0.0, rng.uniform(-1.0, 1.0), 0.0];
                    e[6] = [0.0, 0.0, 0.0];
                    e[1] = [0.0, 1.0, 0.0];
                    e[3] = [1.0, 0.0, 0.0];
                }
                _ => {}
            }
            OpIn::Geom { e: e.to_vec() }
        }
        6 => {
            let want_uu = rng.chance(0.7);
            OpIn::PointType {
                n: gvec(rng, 1.0),
                duu: gvec(rng, 1.0),
                dvv: if rng.chance(0.2) {
                    [0.0, 0.0, 0.0]
                } else {
                    gvec(rng, 1.0)
                },
                want_uu,
                want_vv: want_uu && rng.chance(0.5),
                side_uu: rng.below(3) as u8,
            }
        }
        7 | 8 => {
            let du = gvec(rng, 2.0);
            let dv = if rng.chance(0.15) {
                let s = rng.uniform(-2.0, 2.0);
                [du[0] * s, du[1] * s, du[2] * s]
            } else if rng.chance(0.1) {
                [1e-7, 0.0, 0.0]
            } else {
                gvec(rng, 2.0)
            };
            OpIn::DropVecs {
                du,
                dv,
                v: (0..rng.below(5)).map(|_| gvec(rng, 2.0)).collect(),
            }
        }
        9 => OpIn::ParamDelta {
            t: gvec(rng, 2.0),
            d: gvec(rng, 2.0),
        },
        10 => {
            let gen = |rng: &mut Rng| {
                let mut a: Vec<f64> = (0..rng.below(6)).map(|_| rng.uniform(0.0, 1.0)).collect();
                if rng.chance(0.3) {
                    a.push(0.5);
                }
                a.sort_by(f64::total_cmp);
                a
            };
            OpIn::Merge {
                a: gen(rng),
                b: gen(rng),
            }
        }
        11 => OpIn::MapUv {
            uv: [rng.unit(), rng.unit()],
            orient: rng.below(3) as u8,
            dir: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
        },
        12 => OpIn::IsEven {
            i: rng.range_i64(-50, 50) as i32,
        },
        13 => {
            let mut b: Vec<f64> = (0..=rng.below(6)).map(|_| rng.uniform(-0.5, 1.5)).collect();
            b.sort_by(f64::total_cmp);
            OpIn::Breaks { ivl: [0.0, 1.0], b }
        }
        14 => OpIn::Limits {
            p: rng.point3(-100.0, 100.0),
        },
        15 | 16 => {
            let ang = |rng: &mut Rng| {
                if rng.chance(0.2) {
                    rng.uniform(-500.0, 500.0)
                } else {
                    rng.uniform(-360.0, 360.0)
                }
            };
            let len = |rng: &mut Rng| {
                if rng.chance(0.15) {
                    -rng.unit()
                } else {
                    rng.uniform(0.0, 2.0)
                }
            };
            OpIn::DerivAssert {
                samples: if rng.chance(0.1) {
                    0
                } else {
                    1 + rng.below(200)
                },
                tt: rng.below(9) as u8,
                dir_angles: [ang(rng), ang(rng)],
                tref: rng.chance(0.3),
                tan: [len(rng), len(rng)],
                angles: [ang(rng), ang(rng)],
                vector: if rng.chance(0.2) {
                    [1.23456e-61; 3]
                } else {
                    gvec(rng, 1.0)
                },
                ct: rng.below(5) as u8,
                bet: rng.below(4) as u8,
                radii: [len(rng), len(rng)],
                cref: rng.chance(0.4),
                flip: rng.chance(0.5),
                with_list: rng.chance(0.8),
            }
        }
        17 | 18 => {
            let n = 1 + rng.below(4) as usize;
            let vs = |rng: &mut Rng, k: usize| (0..k).map(|_| gvec(rng, 1.0)).collect::<Vec<_>>();
            let order = rng.below(3); // 0: 1st, 1: 2nd, 2: 3rd derivatives
            let with_end = rng.chance(0.5);
            let bad = rng.chance(0.1);
            let mut opt: [Option<Vec<[f64; 3]>>; 6] = Default::default();
            if order >= 1 {
                opt[0] = Some(vs(rng, n));
            }
            if order >= 2 {
                opt[1] = Some(vs(rng, n));
            }
            if with_end {
                opt[2] = Some(vs(rng, n));
                opt[3] = Some(vs(rng, n));
                if order >= 1 {
                    opt[4] = Some(vs(rng, n));
                }
                if order >= 2 {
                    opt[5] = Some(vs(rng, n));
                }
            }
            if bad {
                let b = rng.below(6) as usize;
                if let Some(slot) = opt.get_mut(b) {
                    *slot = if slot.is_some() {
                        None
                    } else {
                        Some(vs(rng, n))
                    };
                }
            }
            let pos: Vec<[f64; 3]> = (0..n)
                .map(|i| [i as f64, rng.uniform(-0.2, 0.2), rng.uniform(-0.2, 0.2)])
                .collect();
            let d1 = if rng.chance(0.05) {
                vs(rng, n + 1)
            } else {
                vs(rng, n)
            };
            let sets = CrossSets { pos, d1, opt };
            if rng.chance(0.5) {
                OpIn::Cross { sets }
            } else {
                // rail through the start points
                let cpts = sets
                    .pos
                    .iter()
                    .map(|p| [p[0], p[1], p[2], NOW])
                    .collect::<Vec<_>>();
                let rp = (sets.pos.len().saturating_sub(1)).clamp(1, 3);
                let (rail1, params) = if sets.pos.len() >= 2 {
                    let rail = CrvIn {
                        dim: 3,
                        u: gen_knots(rng, rp, sets.pos.len() - 1),
                        p: rp,
                        cpts,
                    };
                    let (a, b) = knot_range(&rail.u);
                    let k = sets.pos.len();
                    let params = (0..k)
                        .map(|i| a + (b - a) * i as f64 / (k - 1) as f64)
                        .collect();
                    (rail, params)
                } else {
                    (gen_curve(rng, 3), vec![0.0])
                };
                OpIn::CrossSkin {
                    sets,
                    params,
                    rail1,
                    rail2: None,
                    tol: if rng.chance(0.7) { 0.0 } else { 1e-3 },
                }
            }
        }
        19 => {
            let c1 = gen_curve(rng, 3);
            let c2 = gen_curve(rng, 3);
            let i1 = sub_ivl(rng, knot_range(&c1.u));
            let i2 = sub_ivl(rng, knot_range(&c2.u));
            OpIn::CcLocal {
                c1,
                i1,
                k1: [rng.below(4) as u8, rng.below(4) as u8],
                c2,
                i2,
                k2: [rng.below(4) as u8, rng.below(4) as u8],
                sop: if rng.chance(0.7) { 3 } else { 4 },
                tol: 1e-6,
            }
        }
        20 => {
            let c = gen_curve(rng, 2);
            let ivl = sub_ivl(rng, knot_range(&c.u));
            let mut s: Vec<f64> = (0..rng.below(3)).map(|_| rng.uniform(0.0, 4.0)).collect();
            s.sort_by(f64::total_cmp);
            if rng.chance(0.5) {
                OpIn::IsectSplit {
                    c,
                    ivl,
                    pp: [rng.uniform(0.0, 3.0), 0.0, 0.0],
                    pn: [1.0, rng.uniform(-0.3, 0.3), 0.0],
                    s,
                }
            } else {
                OpIn::IsectCurveSplit {
                    c,
                    ivl,
                    lp: [rng.uniform(0.0, 3.0), -2.0, 0.0],
                    lv: [rng.uniform(-0.3, 0.3), 1.0, 0.0],
                    livl: [0.0, 4.0],
                    zero: 1e-9,
                    s,
                }
            }
        }
        21 => {
            let a = rng.point3(-1.0, 1.0);
            let b = rng.point3(-1.0, 1.0);
            OpIn::Lsi {
                bbox: [a, b],
                lp: rng.point3(-3.0, 3.0),
                lv: {
                    let v = rng.point3(-1.0, 1.0);
                    if v.iter().map(|c| c * c).sum::<f64>() < 1e-3 {
                        [0.0, 0.0, 1.0]
                    } else {
                        v
                    }
                },
                ivl: rng.chance(0.4).then(|| {
                    let x = rng.uniform(-3.0, 1.0);
                    [x, x + rng.uniform(0.0, 3.0)]
                }),
                ray: rng.chance(0.3),
                tol: if rng.chance(0.5) { 0.0 } else { 1e-3 },
            }
        }
        22 => {
            let s0 = rng.point3(-1.0, 1.0);
            let s1 = rng.point3(-1.0, 1.0);
            let near = rng.chance(0.6);
            let c0 = if near {
                rng.point3(-1.0, 1.0)
            } else {
                rng.point3(3.0, 5.0)
            };
            let c1 = [c0[0] + 0.5, c0[1] + 0.5, c0[2] + 0.5];
            OpIn::CsPerpen {
                caux: rng.below(9) as u8,
                cbox: [c0, c1],
                saux: rng.below(9) as u8,
                sbox: [s0, s1],
            }
        }
        _ => {
            let n = rng.below(3) as usize;
            let cr = |rng: &mut Rng| gen_curve(rng, 2);
            OpIn::Dcf {
                status: *rng.choose(&[1001, 1003, 1007, 0]).unwrap_or(&1001),
                from: rng.below(4),
                path: rng.below(25) as u8,
                ctx: rng.chance(0.5),
                dom: [0.0, 0.0, rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)],
                ivl: [0.0, rng.uniform(0.5, 2.0)],
                tols: [
                    if rng.chance(0.2) { -0.001 } else { 0.001 },
                    rng.unit(),
                    rng.unit(),
                ],
                flags: [0, 1, 2, 0, 1].map(|f: u8| if rng.chance(0.5) { f } else { 2 }),
                max_drop: rng.unit(),
                closed: [rng.below(3) as u8, rng.below(3) as u8],
                closure: rng.below(10) as u8,
                sing: [rng.below(16), rng.below(16)],
                poles: (0..n).map(|_| rng.point3(-1.0, 1.0)).collect(),
                normals: (0..n).map(|_| rng.point3(-1.0, 1.0)).collect(),
                poly: (0..rng.below(4)).map(|_| rng.point3(-1.0, 1.0)).collect(),
                knots: (0..rng.below(4)).map(|_| rng.unit()).collect(),
                curves: (0..rng.below(3)).map(|_| cr(rng)).collect(),
            }
        }
    };
    Inputs { this: None, op }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.5) {
        gen_surface_case(rng)
    } else {
        gen_static_case(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if extension_retries_in_place(&i) {
                continue;
            }
            if ffi::validate(op, &input).is_ok() {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        cases.extend(super::surface_cone_p1::common_cases(&ctx)?);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if let Some(value) = case.inputs.get("common_cone") {
            return super::surface_cone_p1::common_cpp(
                value.clone(),
                case.inputs.get("common_db"),
                case.inputs
                    .get("common_lazy")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                case.inputs
                    .get("common_subdomain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                case.inputs
                    .get("common_queries")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            );
        }
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let k = &Kernel::default();
        if let Some(value) = case.inputs.get("common_cone") {
            return super::surface_cone_p1::common_rust(
                value.clone(),
                case.inputs.get("common_db"),
                case.inputs
                    .get("common_lazy")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                case.inputs
                    .get("common_subdomain")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
                case.inputs
                    .get("common_queries")
                    .and_then(Value::as_bool)
                    .unwrap_or(false),
            );
        }
        rust(k, &case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach the ops with successful outcomes on the Rust side.
    #[test]
    fn generated_cases_cover_ops() {
        let k = &Kernel::default();
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..2000u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_err() {
                continue;
            }
            let base = i.this.as_ref().is_some_and(|t| t.base);
            let name = format!("{op:?}{}", if base { " base" } else { "" });
            let out = rust(k, &i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.unit)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((name, kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            println!("{k:?}: {v}");
        }
        for op in [
            "Eval",
            "EvalPt",
            "Queries",
            "Knots",
            "Reverse",
            "Swap",
            "Transform",
            "Trim",
            "Split",
            "Copy",
            "SEval",
            "Geom",
            "DropVecs",
            "Merge",
            "Breaks",
            "DerivAssert",
            "Cross",
            "CrossSkin",
            "Dcf",
            "Lsi",
            "CsPerpen",
            "PointType",
            "ScaleStep",
        ] {
            assert!(
                tally.keys().any(|(n, k)| n == op && k == "status:0"),
                "{op} never succeeds"
            );
        }
    }
}
