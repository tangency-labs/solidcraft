//! Runner for unit `surface.surface_cache_p1`: the surface cache members owned by the unit
//! (bounding planes of a patch, the Bezier approximation and size tolerances of sub-patches,
//! patch corners, the alternate curve caches, the top of the decomposition tree, leaf
//! classification, knot continuity, iso boundary curves, lazily built polar boxes, simple
//! queries) and the file-static helpers of `SmSurfaceCache.cpp`.
//!
//! C++: `solidcore_ffi::units::surface_surface_cache_p1` (upstream
//! `source/SMLib/src/SmSurfaceCache.cpp`; the file-static helpers are verbatim copies in the
//! shim, decomposition trees are built by hand). Rust: `solidcore::surface::surface_cache::p1`.
//!
//! A case is an optional surface `srf` (`oob`, `form` and an inline or `.sms` surface) and an
//! `op` with its parameters. Both sides produce the flat record documented in the ffi module;
//! the runner reports its first entry as the `status` (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as the `out` values.
//!
//! Deferred cases: the paths into unported units (`SmSurface::IsSingularity` in every
//! Bezier approximation, `SmSurface::EvaluateNormal` for collinear boundary samples and the
//! polar boxes of low degree surfaces, `SmSurface::GetFace`). Every run has at least
//! [`MIN_GENERATED`] generated cases; generated inputs the ffi wrapper rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::p2::CurveKind;
use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::polar_box::{PolarBox, PolarBoxState};
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::nurbs_srf::allocate_nurb_surface;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::surface_cache::p1::{
    self as sc, AltCacheKey, AltSrfCache, AltSrfCacheType, BezierAux2d, BezierPatch,
    PatchBoundaryBoundingPlanes, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_cache_p1::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.surface_cache_p1` runner.
pub struct Runner;

pub(crate) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The handle of the cache surface in trees (faces are `0..`).
const SURFACE_HANDLE: i64 = -7;

// ---- inputs -----------------------------------------------------------------------

/// An inline surface: control net as stored (`[x, y, z, w]`, rows in u), degrees,
/// knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct InlineSrf {
    pub(crate) net: Vec<Vec<[f64; 4]>>,
    pub(crate) p: usize,
    pub(crate) q: usize,
    pub(crate) u: Vec<f64>,
    pub(crate) v: Vec<f64>,
}

/// A surface input.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum SrfIn {
    /// An upstream `.sms` file (absolute path after fixture expansion).
    File { sms: String },
    /// Inline data.
    Inline(InlineSrf),
}

/// A B-spline surface with its flags.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct BssIn {
    #[serde(default)]
    pub(crate) oob: bool,
    #[serde(default = "default_form")]
    pub(crate) form: u8,
    pub(crate) srf: SrfIn,
}

pub(crate) fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

/// An inline curve: dimension, control points as stored, degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct CrvIn {
    pub(crate) dim: u64,
    pub(crate) cpts: Vec<[f64; 4]>,
    pub(crate) p: usize,
    pub(crate) u: Vec<f64>,
}

/// Alternate cache user data (`None` = null pointer).
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub(crate) struct KeyIn {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) vec: Option<[f64; 3]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) b: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) l: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) d: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub(crate) p: Option<i64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct AltIn {
    ty: u8,
    approx: f64,
    angle: f64,
    #[serde(default)]
    key: KeyIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    c3: Option<Vec<CrvIn>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    cuv: Option<Vec<CrvIn>>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueryIn {
    ty: u8,
    approx: f64,
    angle: f64,
    #[serde(default)]
    key: KeyIn,
    w3: bool,
    wuv: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct FirstGenIn {
    bbox: [[f64; 3]; 2],
    dom: [f64; 4],
}

/// A subdivision tree node.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeIn {
    parent: i64,
    c1: i64,
    c2: i64,
    aux: u8,
    split: u8,
    #[serde(default)]
    marked: bool,
    #[serde(default)]
    class: u8,
    #[serde(default = "no_face")]
    face: i64,
    dom: [f64; 4],
}

fn no_face() -> i64 {
    -1
}

/// One subdivision program step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "p", rename_all = "snake_case")]
enum ProgIn {
    Find { class: u8, stack: Vec<usize> },
    Region { node: usize },
    Mark,
    Set {
        node: usize,
        marked: bool,
        class: u8,
        face: i64,
    },
    Dump,
    FirstPatch { node: usize },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Plane3 {
        p: [[f64; 3]; 3],
        tol: f64,
    },
    Halfspace {
        base: [f64; 3],
        normal: [f64; 3],
    },
    BPlanes {
        dom: [f64; 4],
        index: usize,
    },
    BezApprox {
        dom: [f64; 4],
    },
    SizeCorners {
        c: [[f64; 3]; 4],
    },
    SizeSurf {
        dom: [f64; 4],
    },
    Corners {
        dom: [f64; 4],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bez: Option<InlineSrf>,
    },
    Alt {
        alts: Vec<AltIn>,
        queries: Vec<QueryIn>,
    },
    TreeTop {
        nu: usize,
        nv: usize,
        nodes: Vec<FirstGenIn>,
    },
    Subdiv {
        tree: Vec<NodeIn>,
        prog: Vec<ProgIn>,
    },
    Cont {
        u: Vec<u8>,
        v: Vec<u8>,
        dom: [f64; 4],
    },
    Iso {
        cache: [f64; 4],
        calls: Vec<([f64; 4], f64)>,
    },
    Polar {
        tree: Vec<NodeIn>,
        nodes: Vec<usize>,
    },
    Misc {
        dom: [f64; 4],
        test: bool,
        uv: [f64; 2],
    },
    Face,
    Cull {
        v: Vec<f64>,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            Self::Plane3 { .. } => Op::Plane3,
            Self::Halfspace { .. } => Op::Halfspace,
            Self::BPlanes { .. } => Op::BPlanes,
            Self::BezApprox { .. } => Op::BezApprox,
            Self::SizeCorners { .. } => Op::SizeCorners,
            Self::SizeSurf { .. } => Op::SizeSurf,
            Self::Corners { .. } => Op::Corners,
            Self::Alt { .. } => Op::Alt,
            Self::TreeTop { .. } => Op::TreeTop,
            Self::Subdiv { .. } => Op::Subdiv,
            Self::Cont { .. } => Op::Cont,
            Self::Iso { .. } => Op::Iso,
            Self::Polar { .. } => Op::Polar,
            Self::Misc { .. } => Op::Misc,
            Self::Face => Op::Face,
            Self::Cull { .. } => Op::Cull,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    srf: Option<BssIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- file readers -------------------------------------------------------------------

fn numbers(path: &str) -> Result<Vec<f64>, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    text.split_whitespace()
        .map(|t| t.parse::<f64>().map_err(|e| harness(format!("{path}: {e}"))))
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
pub(crate) fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let v = numbers(path)?;
    let mut it = v.into_iter();
    let mut num = || it.next().ok_or_else(|| harness(format!("{path}: truncated")));
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
    let u = (0..n + p + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    let v = (0..m + q + 2).map(|_| num()).collect::<Result<Vec<_>, _>>()?;
    Ok(InlineSrf { net, p, q, u, v })
}

pub(crate) fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

pub(crate) fn shape(s: &InlineSrf) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

pub(crate) fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

// ---- C++ side -----------------------------------------------------------------------

pub(crate) fn push_srf(o: &mut Vec<f64>, s: &InlineSrf) -> Result<(), HarnessError> {
    let (n, m) = shape(s)?;
    o.extend(
        [n, m, s.p, s.q, s.u.len().saturating_sub(1), s.v.len().saturating_sub(1)]
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

pub(crate) fn push_bss(o: &mut Vec<f64>, b: &BssIn) -> Result<(), HarnessError> {
    o.push(flag(b.oob));
    o.push(f64::from(b.form));
    push_srf(o, &srf(&b.srf)?)
}

pub(crate) fn push_crv(o: &mut Vec<f64>, c: &CrvIn) {
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

pub(crate) fn push_key(o: &mut Vec<f64>, k: &KeyIn) {
    o.push(flag(k.vec.is_some()));
    o.extend(k.vec.unwrap_or([0.0; 3]));
    o.push(flag(k.b.is_some()));
    o.push(flag(k.b.unwrap_or(false)));
    o.push(flag(k.l.is_some()));
    o.push(k.l.unwrap_or(0) as f64);
    o.push(flag(k.d.is_some()));
    o.push(k.d.unwrap_or(0.0));
    o.push(flag(k.p.is_some()));
    o.push(k.p.unwrap_or(-1) as f64);
}

pub(crate) fn push_curves(o: &mut Vec<f64>, c: Option<&Vec<CrvIn>>) {
    match c {
        None => o.push(-1.0),
        Some(v) => {
            o.push(v.len() as f64);
            for c in v {
                push_crv(o, c);
            }
        }
    }
}

fn push_tree(o: &mut Vec<f64>, t: &[NodeIn]) {
    o.push(t.len() as f64);
    for n in t {
        o.extend([
            n.parent as f64,
            n.c1 as f64,
            n.c2 as f64,
            f64::from(n.aux),
            f64::from(n.split),
            flag(n.marked),
            f64::from(n.class),
            n.face as f64,
        ]);
        o.extend_from_slice(&n.dom);
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = i.op.ffi();
    if !matches!(op, Op::Plane3 | Op::Cull) {
        let b = i.srf.as_ref().ok_or_else(|| harness("missing surface"))?;
        push_bss(&mut o, b)?;
    }
    match &i.op {
        OpIn::Plane3 { p, tol } => {
            for v in p {
                o.extend_from_slice(v);
            }
            o.push(*tol);
        }
        OpIn::Halfspace { base, normal } => {
            o.extend_from_slice(base);
            o.extend_from_slice(normal);
        }
        OpIn::BPlanes { dom, index } => {
            o.extend_from_slice(dom);
            o.push(*index as f64);
        }
        OpIn::BezApprox { dom } | OpIn::SizeSurf { dom } => o.extend_from_slice(dom),
        OpIn::SizeCorners { c } => {
            for v in c {
                o.extend_from_slice(v);
            }
        }
        OpIn::Corners { dom, bez } => {
            o.extend_from_slice(dom);
            o.push(flag(bez.is_some()));
            if let Some(b) = bez {
                push_srf(&mut o, b)?;
            }
        }
        OpIn::Alt { alts, queries } => {
            o.push(alts.len() as f64);
            for a in alts {
                o.extend([f64::from(a.ty), a.approx, a.angle]);
                push_key(&mut o, &a.key);
                push_curves(&mut o, a.c3.as_ref());
                push_curves(&mut o, a.cuv.as_ref());
            }
            o.push(queries.len() as f64);
            for q in queries {
                o.extend([f64::from(q.ty), q.approx, q.angle]);
                push_key(&mut o, &q.key);
                o.push(flag(q.w3));
                o.push(flag(q.wuv));
            }
        }
        OpIn::TreeTop { nu, nv, nodes } => {
            o.push(*nu as f64);
            o.push(*nv as f64);
            for n in nodes {
                o.extend_from_slice(&n.bbox[0]);
                o.extend_from_slice(&n.bbox[1]);
                o.extend_from_slice(&n.dom);
            }
        }
        OpIn::Subdiv { tree, prog } => {
            push_tree(&mut o, tree);
            o.push(prog.len() as f64);
            for p in prog {
                match p {
                    ProgIn::Find { class, stack } => {
                        o.extend([0.0, f64::from(*class), stack.len() as f64]);
                        o.extend(stack.iter().map(|&s| s as f64));
                    }
                    ProgIn::Region { node } => o.extend([1.0, *node as f64]),
                    ProgIn::Mark => o.push(2.0),
                    ProgIn::Set {
                        node,
                        marked,
                        class,
                        face,
                    } => o.extend([
                        3.0,
                        *node as f64,
                        flag(*marked),
                        f64::from(*class),
                        *face as f64,
                    ]),
                    ProgIn::Dump => o.push(4.0),
                    ProgIn::FirstPatch { node } => o.extend([5.0, *node as f64]),
                }
            }
        }
        OpIn::Cont { u, v, dom } => {
            o.push(u.len() as f64);
            o.extend(u.iter().map(|&c| f64::from(c)));
            o.push(v.len() as f64);
            o.extend(v.iter().map(|&c| f64::from(c)));
            o.extend_from_slice(dom);
        }
        OpIn::Iso { cache, calls } => {
            o.extend_from_slice(cache);
            o.push(calls.len() as f64);
            for (d, tol) in calls {
                o.extend_from_slice(d);
                o.push(*tol);
            }
        }
        OpIn::Polar { tree, nodes } => {
            push_tree(&mut o, tree);
            o.push(nodes.len() as f64);
            o.extend(nodes.iter().map(|&n| n as f64));
        }
        OpIn::Misc { dom, test, uv } => {
            o.extend_from_slice(dom);
            o.push(flag(*test));
            o.extend_from_slice(uv);
        }
        OpIn::Face => {}
        OpIn::Cull { v } => {
            o.push(v.len() as f64);
            o.extend_from_slice(v);
        }
    }
    Ok((op, o))
}

fn to_output(flat: &[f64]) -> Output {
    let (status, rest) = flat.split_first().map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new().values("status", vec![status]).values("out", rest.to_vec())
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    let flat = ffi::eval(op, &input).map_err(|e| harness(e.to_string()))?;
    Ok(to_output(&flat))
}

// ---- Rust side ----------------------------------------------------------------------

pub(crate) fn surface_of(s: &InlineSrf) -> Result<NlSurface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
        .collect();
    NlSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))
}

pub(crate) fn bss_of(b: &BssIn) -> Result<BSplineSurface, HarnessError> {
    let mut s = BSplineSurface::from_nurb(surface_of(&srf(&b.srf)?)?, false);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
}

pub(crate) fn curve_of(c: &CrvIn) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect();
    let cur = Curve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, c.dim, false))
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

fn class_of(v: u8) -> Result<NodeClass, HarnessError> {
    Ok(match v {
        0 => NodeClass::Unknown,
        1 => NodeClass::Inside,
        2 => NodeClass::Outside,
        3 => NodeClass::OnBoundary,
        _ => return Err(harness(format!("bad class {v}"))),
    })
}

fn split_of(v: u8) -> SplitDir {
    match v {
        0 => SplitDir::U,
        1 => SplitDir::V,
        _ => SplitDir::Unknown,
    }
}

fn split_code(s: SplitDir) -> f64 {
    match s {
        SplitDir::U => 0.0,
        SplitDir::V => 1.0,
        SplitDir::Unknown => 2.0,
    }
}

pub(crate) fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

pub(crate) fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn box_of(b: [[f64; 3]; 2]) -> Extent3d {
    let mut e = Extent3d::new();
    e.add_point(v3(b[0]));
    e.add_point(v3(b[1]));
    e
}

fn link(i: i64) -> Option<NodeId> {
    usize::try_from(i).ok().map(NodeId)
}

fn obj(i: i64) -> Option<i64> {
    (i >= 0).then_some(i)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
pub(crate) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

pub(crate) fn push3(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn push2(o: &mut Vec<f64>, v: Vec2) {
    o.extend([v.x, v.y]);
}

pub(crate) fn push_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

pub(crate) fn push_box(o: &mut Vec<f64>, b: &Extent3d) {
    push3(o, b.min());
    push3(o, b.max());
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

fn dump_curve_list(o: &mut Vec<f64>, l: Option<&Vec<SmCurve>>) -> Result<(), HarnessError> {
    let Some(l) = l else {
        o.push(0.0);
        return Ok(());
    };
    o.push(l.len() as f64);
    for c in l {
        match c.kind() {
            CurveKind::BSpline(b) => dump_curve(o, b),
            _ => return Err(harness("copied curve is not a B-spline")),
        }
    }
    Ok(())
}

fn dump_polar(o: &mut Vec<f64>, b: &PolarBox) {
    o.push(f64::from(b.state().code()));
    if b.state() == PolarBoxState::Uninitialized {
        return;
    }
    for v in b.basis() {
        push3(o, *v);
    }
    push_dom(o, &b.domain());
}

pub(crate) fn key_of(k: &KeyIn) -> AltCacheKey {
    AltCacheKey {
        vec: k.vec.map(v3),
        boolean: k.b,
        long: k.l,
        double: k.d,
        pointer: k.p.and_then(|p| u64::try_from(p).ok()),
    }
}

pub(crate) fn alt_type(t: u8) -> AltSrfCacheType {
    if t == 0 {
        AltSrfCacheType::Silhouette
    } else {
        AltSrfCacheType::TrimmedSilhouette
    }
}

pub(crate) fn curves_of(c: Option<&Vec<CrvIn>>) -> Result<Option<Vec<SmCurve>>, HarnessError> {
    c.map(|v| {
        v.iter()
            .map(|c| curve_of(c).map(SmCurve::from))
            .collect::<Result<Vec<_>, _>>()
    })
    .transpose()
}

/// The outcome of a Rust op: the record, or an operation error (its status ends the
/// record unless the op already wrote it).
type Run = Result<Vec<f64>, SolidError>;

/// A cache of `s` over its natural domain.
fn cache_of(s: &BSplineSurface, dom: Option<Extent2d>) -> SurfaceCache<i64> {
    let surface = Surface::from(s.copy());
    let d = dom.unwrap_or_else(|| surface.natural_uv_domain());
    let mut c = SurfaceCache::new(surface, d, SurfaceCacheOptions::default());
    c.set_surface_handle(Some(SURFACE_HANDLE));
    c
}

/// Builds the decomposition tree of `t` in `cache` (node 0 the top).
fn build_tree(cache: &mut SurfaceCache<i64>, t: &[NodeIn]) -> Result<(), HarnessError> {
    let mut tree: Tree<i64> = Tree::new(Some(TreeOwner::SurfaceCache));
    let mut patches = Vec::new();
    let mut auxs = Vec::new();
    for (k, n) in t.iter().enumerate() {
        let aux_type = AuxDataType::from_code(i32::from(n.aux))
            .ok_or_else(|| harness(format!("bad aux type {}", n.aux)))?;
        let data = BezierAux2d {
            uv_domain: dom_of(n.dom),
            owning_tree_node: Some(NodeId(k)),
            ..BezierAux2d::default()
        };
        let slot = if aux_type == AuxDataType::BezierSurface {
            patches.push(BezierPatch {
                aux: data,
                ..BezierPatch::default()
            });
            patches.len() - 1
        } else {
            auxs.push(data);
            auxs.len() - 1
        };
        tree.add_node(TreeNode {
            parent: link(n.parent),
            child1: link(n.c1),
            child2: link(n.c2),
            aux_type,
            data: NodeData::Surface(SurfaceAux {
                split_dir: split_of(n.split),
                is_marked: n.marked,
                node_class: class_of(n.class)?,
                face: obj(n.face),
                surface: Some(SURFACE_HANDLE),
                slot: Some(slot),
            }),
            geom_type: NodeGeomType::Default,
            ..TreeNode::new()
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    *cache.patches_mut() = patches;
    *cache.auxs_mut() = auxs;
    cache.set_tree(Some(tree));
    Ok(())
}

fn tree_of(cache: &SurfaceCache<i64>) -> Result<&Tree<i64>, HarnessError> {
    cache.tree().ok_or_else(|| harness("no tree"))
}

fn idx(n: Option<NodeId>) -> f64 {
    n.map_or(-1.0, |n| n.0 as f64)
}

#[allow(clippy::too_many_lines)]
fn rust_op(i: &Inputs) -> Result<Run, HarnessError> {
    let k = Kernel::default();
    let bss = i.srf.as_ref().map(bss_of).transpose()?;
    let need = || bss.as_ref().ok_or_else(|| harness("missing surface"));
    let mut o = vec![0.0];
    let run: Run = match &i.op {
        OpIn::Plane3 { p, tol } => {
            sc::compute_plane_from_3_points(v3(p[0]), v3(p[1]), v3(p[2]), *tol).map(|r| {
                o.push(flag(r.plane_exists));
                o.push(flag(r.collinear));
                if let (true, Some(b), Some(n)) = (r.plane_exists, r.base, r.normal) {
                    push3(&mut o, b);
                    push3(&mut o, n);
                }
                o
            })
        }
        OpIn::Cull { v } => {
            let mut a = v.clone();
            sc::cull_array(&mut a);
            o.push(a.len() as f64);
            o.extend(a);
            Ok(o)
        }
        OpIn::Halfspace { base, normal } => {
            sc::check_bounding_plane_separating_halfspace(need()?, v3(*base), v3(*normal)).map(
                |(sep, inv)| {
                    o.push(flag(sep));
                    o.push(flag(inv));
                    o
                },
            )
        }
        OpIn::BPlanes { dom, index } => {
            let surface = Surface::from(need()?.copy());
            let mut bp = PatchBoundaryBoundingPlanes::new(dom_of(*dom));
            o.clear();
            match bp.create_boundary_curve_for_index(&k, &surface, *index) {
                Ok(c) => {
                    o.push(0.0);
                    dump_curve(&mut o, &c);
                }
                Err(e) if e.is_unported() => return Ok(Err(e)),
                Err(e) => o.push(status(&e)),
            }
            for i in 0..4 {
                let (a, b) = bp.boundary_uv_end_points(i);
                push2(&mut o, a);
                push2(&mut o, b);
            }
            match bp.compute_bounding_planes(&k, &surface) {
                Ok(()) => {
                    o.push(0.0);
                    o.push(flag(bp.are_planes_computed()));
                    for i in 0..4 {
                        o.push(flag(bp.is_boundary_degenerate(i)));
                        o.push(flag(bp.is_boundary_planar(i)));
                        if let Some((b, n, s, t)) = bp.plane(i) {
                            push3(&mut o, b);
                            push3(&mut o, n);
                            o.push(t);
                            for p in s {
                                push3(&mut o, p);
                            }
                        }
                    }
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => {
                    o.push(status(&e));
                    Ok(o)
                }
            }
        }
        OpIn::BezApprox { dom } => {
            let surface = Surface::from(need()?.copy());
            let mut bez =
                allocate_nurb_surface(3, 3, 3, 3, 7, 7).map_err(|e| harness(e.to_string()))?;
            sc::compute_bezier_approximation(&surface, &dom_of(*dom), &mut bez).map(|()| {
                for row in bez.rows() {
                    for p in row {
                        o.extend([p.x, p.y, p.z, p.w]);
                    }
                }
                o.extend_from_slice(bez.u_knots());
                o.extend_from_slice(bez.v_knots());
                o
            })
        }
        OpIn::SizeCorners { c } => {
            let s = SurfaceCache::<i64>::size_tolerances_from_corners(
                v3(c[0]),
                v3(c[1]),
                v3(c[2]),
                v3(c[3]),
            );
            push2(&mut o, s.aspect_ratio);
            push2(&mut o, s.max_side_length_3d);
            push2(&mut o, s.min_side_length_3d);
            Ok(o)
        }
        OpIn::SizeSurf { dom } => {
            let surface = Surface::from(need()?.copy());
            SurfaceCache::<i64>::size_tolerances_from_surface(&k, &surface, &dom_of(*dom)).map(
                |s| {
                    push2(&mut o, s.aspect_ratio);
                    push2(&mut o, s.max_side_length_3d);
                    push2(&mut o, s.min_side_length_3d);
                    o
                },
            )
        }
        OpIn::Corners { dom, bez } => {
            let cache = cache_of(need()?, None);
            let nurb = bez.as_ref().map(surface_of).transpose()?;
            cache.corners(nurb.as_ref(), &dom_of(*dom)).map(|c| {
                for p in [c.u0v0, c.u1v0, c.u1v1, c.u0v1] {
                    push3(&mut o, p);
                }
                push_dom(
                    &mut o,
                    &c.domain
                        .unwrap_or_else(|| Extent2d::from_bounds(0.0, 0.0, 0.0, 0.0)),
                );
                o
            })
        }
        OpIn::Alt { alts, queries } => {
            let mut cache = cache_of(need()?, None);
            for a in alts {
                cache.alt_caches_mut().push(AltSrfCache::new(
                    curves_of(a.c3.as_ref())?,
                    curves_of(a.cuv.as_ref())?,
                    alt_type(a.ty),
                    a.approx,
                    a.angle,
                    &key_of(&a.key),
                ));
            }
            o.clear();
            o.push(0.0);
            for q in queries {
                let key = key_of(&q.key);
                for a in cache.alt_caches() {
                    o.push(flag(a.match_cache(alt_type(q.ty), q.approx, q.angle, &key)));
                    let (copies, st) = a.copy_of_curves_partial(q.w3, q.wuv);
                    o.push(match &st {
                        Ok(()) => 0.0,
                        Err(e) => status(e),
                    });
                    o.push(copies.curves_3d.as_ref().map_or(0.0, |v| v.len() as f64));
                    o.push(copies.curves_uv.as_ref().map_or(0.0, |v| v.len() as f64));
                }
                match cache.has_cached_curves(alt_type(q.ty), q.approx, q.angle, &key, q.w3, q.wuv) {
                    Ok(Some(c)) => {
                        o.push(1.0);
                        dump_curve_list(&mut o, c.curves_3d.as_ref())?;
                        dump_curve_list(&mut o, c.curves_uv.as_ref())?;
                    }
                    Ok(None) => o.extend([0.0, 0.0, 0.0]),
                    Err(e) => return Ok(Err(e)),
                }
            }
            Ok(o)
        }
        OpIn::TreeTop { nu, nv, nodes } => {
            let mut cache = cache_of(need()?, None);
            let mut tree: Tree<i64> = Tree::new(Some(TreeOwner::SurfaceCache));
            let mut first = Vec::new();
            for (k, n) in nodes.iter().enumerate() {
                let id = tree.add_node(TreeNode {
                    parent: None,
                    child1: None,
                    child2: None,
                    bbox: box_of(n.bbox),
                    aux_type: AuxDataType::BezierSurface,
                    data: NodeData::Surface(SurfaceAux {
                        split_dir: SplitDir::Unknown,
                        is_marked: false,
                        node_class: NodeClass::Unknown,
                        face: None,
                        surface: Some(SURFACE_HANDLE),
                        slot: Some(k),
                    }),
                    geom_type: NodeGeomType::Default,
                });
                cache.patches_mut().push(BezierPatch {
                    aux: BezierAux2d {
                        uv_domain: dom_of(n.dom),
                        owning_tree_node: Some(id),
                        ..BezierAux2d::default()
                    },
                    ..BezierPatch::default()
                });
                first.push(id);
            }
            cache.set_tree(Some(tree));
            match cache.build_tree_top(&first, *nu, *nv) {
                Ok(()) => {}
                Err(e) => return Ok(Err(e)),
            }
            let tree = tree_of(&cache)?;
            o.push(idx(tree.top_node()));
            o.push(tree.nodes().len() as f64);
            for (k, n) in tree.nodes().iter().enumerate() {
                o.extend([idx(n.parent), idx(n.child1), idx(n.child2)]);
                o.push(f64::from(n.aux_type.code()));
                o.push(f64::from(n.geom_type.code()));
                let NodeData::Surface(a) = n.data else {
                    return Err(harness("node without surface data"));
                };
                o.push(split_code(a.split_dir));
                o.push(flag(a.is_marked));
                o.push(f64::from(a.node_class.code()));
                o.push(flag(a.surface.is_some()));
                let aux = match cache.aux(NodeId(k)) {
                    Ok(x) => x,
                    Err(e) => return Ok(Err(e)),
                };
                o.push(idx(aux.owning_tree_node));
                push_box(&mut o, &n.bbox);
                push_dom(&mut o, aux.uv_domain());
            }
            o.push(cache.patch_count() as f64);
            for i in 0..*nu {
                for j in 0..*nv {
                    let found = match cache.patch_at(i, j, *nu, *nv) {
                        Ok(p) => cache
                            .patches()
                            .iter()
                            .position(|x| std::ptr::eq(x, p))
                            .map_or(-1.0, |x| x as f64),
                        Err(_) => -1.0,
                    };
                    o.push(found);
                }
            }
            Ok(o)
        }
        OpIn::Subdiv { tree, prog } => {
            let mut cache = cache_of(need()?, None);
            build_tree(&mut cache, tree)?;
            for p in prog {
                match p {
                    ProgIn::Find { class, stack } => {
                        let mut st: Vec<NodeId> = stack.iter().map(|&s| NodeId(s)).collect();
                        match cache.find_unmarked_leaf_node(class_of(*class)?, &mut st) {
                            Ok(f) => o.push(idx(f)),
                            Err(e) => return Err(harness(format!("find: {e}"))),
                        }
                    }
                    ProgIn::Region { node } => match cache.connected_region(NodeId(*node)) {
                        Ok(r) => {
                            o.push(r.nodes.len() as f64);
                            o.extend(r.nodes.iter().map(|n| n.0 as f64));
                            o.push(idx(r.classified_neighbor));
                        }
                        Err(e) => return Err(harness(format!("region: {e}"))),
                    },
                    ProgIn::Mark => match cache.mark_face_containment(&k) {
                        Ok(()) => o.push(0.0),
                        Err(e) if e.is_unported() => return Ok(Err(e)),
                        Err(e) => o.push(status(&e)),
                    },
                    ProgIn::Set {
                        node,
                        marked,
                        class,
                        face,
                    } => {
                        let t = cache.tree_mut().ok_or_else(|| harness("no tree"))?;
                        let n = t.node_mut(NodeId(*node)).map_err(|e| harness(e.to_string()))?;
                        if let NodeData::Surface(a) = &mut n.data {
                            a.is_marked = *marked;
                            a.node_class = class_of(*class)?;
                            a.face = obj(*face);
                        }
                    }
                    ProgIn::FirstPatch { node } => {
                        let own = match cache.surface().kind() {
                            SurfaceKind::BSpline(b) => b.gw_nurb(),
                            SurfaceKind::CurveBounded(_)
                            | SurfaceKind::Plane(_)
                            | SurfaceKind::SurfOfRevolution(_)
                            | SurfaceKind::Step(_)
                            | SurfaceKind::SrfInVolume(_) => None,
                        };
                        match cache.first_bezier_patch(NodeId(*node)) {
                            Ok(None) => o.push(0.0),
                            Ok(Some(p)) => {
                                o.push(if own.is_some_and(|q| std::ptr::eq(p, q)) { 1.0 } else { 2.0 });
                            }
                            Err(e) => return Err(harness(format!("first patch: {e}"))),
                        }
                    }
                    ProgIn::Dump => {
                        for n in tree_of(&cache)?.nodes() {
                            if let NodeData::Surface(a) = n.data {
                                o.push(flag(a.is_marked));
                                o.push(f64::from(a.node_class.code()));
                                o.push(a.face.map_or(-1.0, |f| f as f64));
                            }
                        }
                    }
                }
            }
            // the C++ record has no status: drop the leading 0
            o.remove(0);
            Ok(o)
        }
        OpIn::Cont { u, v, dom } => {
            let mut cache = cache_of(need()?, None);
            let uc = u.iter().map(|&c| continuity_of(c)).collect::<Result<Vec<_>, _>>()?;
            let vc = v.iter().map(|&c| continuity_of(c)).collect::<Result<Vec<_>, _>>()?;
            cache.set_continuities(uc, vc);
            cache.surface_continuity(&dom_of(*dom)).map(|c| {
                o.push(f64::from(c as i32));
                o
            })
        }
        OpIn::Iso { cache: cd, calls } => {
            let mut cache = cache_of(need()?, Some(dom_of(*cd)));
            for (d, tol) in calls {
                match cache.iso_boundary_curves(&k, &dom_of(*d), *tol) {
                    Ok(r) => {
                        o.push(0.0);
                        o.push(flag(r.cached));
                        for c in &r.curves {
                            dump_curve(&mut o, c);
                        }
                    }
                    Err(e) if e.is_unported() => return Ok(Err(e)),
                    Err(e) => o.push(status(&e)),
                }
                o.push(cache.iso_curve_tolerance());
            }
            Ok(o)
        }
        OpIn::Polar { tree, nodes } => {
            let mut cache = cache_of(need()?, None);
            build_tree(&mut cache, tree)?;
            for &n in nodes {
                match cache.polar_box(NodeId(n)) {
                    Ok(b) => dump_polar(&mut o, &b),
                    Err(e) => return Ok(Err(e)),
                }
            }
            for k in 0..tree.len() {
                let a = cache.aux(NodeId(k)).map_err(|e| harness(e.to_string()))?;
                o.push(f64::from(a.polar_box_stored().state().code()));
            }
            Ok(o)
        }
        OpIn::Misc { dom, test, uv } => {
            let mut cache = cache_of(need()?, Some(dom_of(*dom)));
            cache.point_test_enabled = *test;
            o.push(cache.type_code() as f64);
            for t in [15100, 11001, 15001, 15120] {
                o.push(flag(cache.is_kind_of(t)));
            }
            o.push(flag(cache.process_boundary_curves()));
            o.push(cache.patch_count() as f64);
            o.push(0.0);
            o.push(flag(cache.point_test(Point2::new(uv[0], uv[1]))));
            let st = |r: solidcore::Result<()>| r.map_or_else(|e| status(&e), |()| 0.0);
            o.push(st(cache
                .point_classify(Point2::new(uv[0], uv[1]), 0.0)
                .map(|_| ())));
            o.push(st(cache.implant_edgeuse(0, true)));
            o.push(st(cache.implant_poly_edge(0, NodeId(0))));
            push_dom(&mut o, &cache.uv_domain());
            o.push(1.0);
            o.push(flag(cache.patch_boundary_bounding_planes().are_planes_computed()));
            Ok(o)
        }
        OpIn::Face => {
            let cache = cache_of(need()?, None);
            cache.face().map(|f| {
                o.push(flag(f.is_some()));
                o
            })
        }
    };
    Ok(run)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let run = rust_op(i)?;
    let is_subdiv = matches!(i.op, OpIn::Subdiv { .. });
    Ok(match run {
        Ok(o) if is_subdiv => Output::new()
            .values("status", vec![0.0])
            .values("out", o),
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

/// The C++ record of a subdivision program has no status entry.
fn cpp_output(i: &Inputs) -> Result<Output, HarnessError> {
    if matches!(i.op, OpIn::Subdiv { .. }) {
        let (op, input) = cpp_input(i)?;
        let flat = ffi::eval(op, &input).map_err(|e| harness(e.to_string()))?;
        return Ok(Output::new()
            .values("status", vec![0.0])
            .values("out", flat));
    }
    cpp(i)
}

// ---- generated cases ----------------------------------------------------------------

pub(crate) fn gen_knots(rng: &mut Rng, p: usize, n: usize) -> Vec<f64> {
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

/// A surface: a bumpy grid, a plane, a cylinder-like sheet, or one with a collapsed
/// (degenerate) side.
pub(crate) fn gen_srf(rng: &mut Rng, max_deg: usize) -> InlineSrf {
    let p = 1 + rng.below(max_deg as u64) as usize;
    let q = 1 + rng.below(max_deg as u64) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let kind = rng.below(5);
    let amp = if kind == 1 { 0.0 } else { rng.uniform(0.0, 1.0) };
    let rational = rng.chance(0.15);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (mut x, mut y, mut z) = (
                        i as f64 + rng.uniform(-0.2, 0.2),
                        j as f64 + rng.uniform(-0.2, 0.2),
                        amp * rng.uniform(-1.0, 1.0),
                    );
                    match kind {
                        // planar
                        1 => z = 0.0,
                        // cylinder-like: v straight lines, u bent
                        2 => {
                            let t = i as f64 / n as f64 * 2.5;
                            x = t.cos() * 3.0;
                            z = t.sin() * 3.0;
                            y = j as f64;
                        }
                        // u = 0 side collapsed to a point
                        3 if i == 0 => {
                            x = 0.0;
                            y = 0.0;
                            z = 0.0;
                        }
                        _ => {}
                    }
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

pub(crate) fn gen_bss(rng: &mut Rng, s: InlineSrf) -> BssIn {
    BssIn {
        oob: rng.chance(0.1),
        form: if rng.chance(0.85) { 11 } else { rng.below(10) as u8 },
        srf: SrfIn::Inline(s),
    }
}

pub(crate) fn gen_curve(rng: &mut Rng) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(3) as usize;
    let cpts = (0..=n)
        .map(|i| [i as f64, rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), NOW])
        .collect();
    CrvIn {
        dim: 3,
        cpts,
        p,
        u: gen_knots(rng, p, n),
    }
}

pub(crate) fn domain_of_srf(s: &InlineSrf) -> [f64; 4] {
    let u0 = s.u.first().copied().unwrap_or(0.0);
    let u1 = s.u.last().copied().unwrap_or(1.0);
    let v0 = s.v.first().copied().unwrap_or(0.0);
    let v1 = s.v.last().copied().unwrap_or(1.0);
    [u0, v0, u1, v1]
}

/// A sub-domain of `d`: the whole, a knot-aligned or a random piece.
pub(crate) fn gen_sub_dom(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    if rng.chance(0.3) {
        return d;
    }
    let mut pick = |a: f64, b: f64| {
        let s = rng.uniform(0.0, 0.6);
        let e = rng.uniform(s + 0.2, 1.0);
        (a + s * (b - a), a + e * (b - a))
    };
    let (a, b) = pick(d[0], d[2]);
    let (c, e) = pick(d[1], d[3]);
    [a, c, b, e]
}

pub(crate) fn gvec(rng: &mut Rng, s: f64) -> [f64; 3] {
    [
        rng.uniform(-s, s),
        rng.uniform(-s, s),
        rng.uniform(-s, s),
    ]
}

pub(crate) fn gen_key(rng: &mut Rng) -> KeyIn {
    KeyIn {
        vec: rng.chance(0.4).then(|| gvec(rng, 2.0)),
        b: rng.chance(0.4).then(|| rng.chance(0.5)),
        l: rng.chance(0.4).then(|| rng.range_i64(-2, 3)),
        d: rng.chance(0.4).then(|| rng.uniform(-1.0, 1.0)),
        p: rng.chance(0.3).then(|| rng.range_i64(0, 3)),
    }
}

/// A key equal to or slightly off `k` (or with values left out).
pub(crate) fn perturb_key(rng: &mut Rng, k: &KeyIn) -> KeyIn {
    let mut q = k.clone();
    match rng.below(6) {
        0 => q.vec = q.vec.map(|v| [v[0] + rng.uniform(-1e-12, 1e-12), v[1], v[2]]),
        1 => q.b = None,
        2 => q.l = q.l.map(|l| l + 1),
        3 => q.d = q.d.map(|d| d + rng.uniform(-2e-12, 2e-12)),
        4 => q.p = Some(rng.range_i64(0, 3)),
        _ => {}
    }
    q
}

/// A random subdivision tree over `dom`: binary splits of random leaves.
fn gen_tree(rng: &mut Rng, dom: [f64; 4], polar: bool) -> Vec<NodeIn> {
    let mut t = vec![NodeIn {
        parent: -1,
        c1: -1,
        c2: -1,
        aux: 3,
        split: 2,
        marked: false,
        class: 0,
        face: -1,
        dom,
    }];
    let splits = rng.below(if polar { 6 } else { 14 }) as usize;
    for _ in 0..splits {
        let leaves: Vec<usize> = t
            .iter()
            .enumerate()
            .filter(|(_, n)| n.c1 < 0)
            .map(|(k, _)| k)
            .collect();
        let Some(&l) = rng.choose(&leaves) else {
            break;
        };
        let n = t.len() as i64;
        let aux = *rng.choose(&[3u8, 3, 4, 6]).unwrap_or(&3);
        let split = u8::from(rng.chance(0.5));
        let Some(node) = t.get_mut(l) else {
            break;
        };
        let d = node.dom;
        node.c1 = n;
        node.c2 = n + 1;
        node.split = split;
        node.aux = aux;
        let f = if rng.chance(0.7) { 0.5 } else { rng.uniform(0.2, 0.8) };
        let (d1, d2) = if split == 0 {
            let m = d[0] + f * (d[2] - d[0]);
            ([d[0], d[1], m, d[3]], [m, d[1], d[2], d[3]])
        } else {
            let m = d[1] + f * (d[3] - d[1]);
            ([d[0], d[1], d[2], m], [d[0], m, d[2], d[3]])
        };
        for dd in [d1, d2] {
            t.push(NodeIn {
                parent: l as i64,
                c1: -1,
                c2: -1,
                aux: if rng.chance(0.8) { 3 } else { 6 },
                split: 2,
                marked: false,
                class: 0,
                face: -1,
                dom: dd,
            });
        }
    }
    // classify some leaves
    for n in &mut t {
        if n.c1 >= 0 || polar {
            continue;
        }
        let r = rng.below(10);
        let (class, marked) = match r {
            0 | 1 => (1, true),
            2 | 3 => (2, true),
            4 => (3, true),
            5 => (1, false),
            _ => (0, false),
        };
        n.class = class;
        n.marked = marked;
        if class == 1 && rng.chance(0.7) {
            n.face = rng.range_i64(0, 5);
        }
    }
    t
}

fn gen_prog(rng: &mut Rng, t: &[NodeIn]) -> Vec<ProgIn> {
    let n = t.len();
    let mut prog = Vec::new();
    for _ in 0..=rng.below(4) {
        let p = match rng.below(6) {
            0 => ProgIn::Find {
                class: rng.below(4) as u8,
                stack: if rng.chance(0.6) {
                    vec![0]
                } else {
                    (0..=rng.below(3)).map(|_| rng.below(n as u64) as usize).collect()
                },
            },
            1 | 2 => ProgIn::Region {
                node: rng.below(n as u64) as usize,
            },
            3 => {
                let class = rng.below(4) as u8;
                ProgIn::Set {
                    node: rng.below(n as u64) as usize,
                    marked: rng.chance(0.5),
                    class,
                    face: if class == 1 { rng.range_i64(-1, 4) } else { -1 },
                }
            }
            4 => ProgIn::Dump,
            5 if rng.chance(0.5) => ProgIn::FirstPatch {
                node: rng.below(n as u64) as usize,
            },
            _ => ProgIn::Mark,
        };
        prog.push(p);
    }
    if rng.chance(0.6) {
        prog.push(ProgIn::Mark);
    }
    prog.push(ProgIn::Dump);
    prog
}

/// A grid of first generation nodes over `dom` with boxes from the surface's net.
fn gen_tree_top(rng: &mut Rng, dom: [f64; 4]) -> OpIn {
    let nu = 1 + rng.below(6) as usize;
    let nv = 1 + rng.below(6) as usize;
    let mut nodes = Vec::new();
    for i in 0..nu {
        for j in 0..nv {
            let u0 = dom[0] + (dom[2] - dom[0]) * i as f64 / nu as f64;
            let u1 = dom[0] + (dom[2] - dom[0]) * (i + 1) as f64 / nu as f64;
            let v0 = dom[1] + (dom[3] - dom[1]) * j as f64 / nv as f64;
            let v1 = dom[1] + (dom[3] - dom[1]) * (j + 1) as f64 / nv as f64;
            let a = [
                i as f64 + rng.uniform(-0.1, 0.1),
                j as f64 * rng.uniform(0.5, 2.0),
                rng.uniform(-1.0, 0.0),
            ];
            let b = [
                a[0] + rng.uniform(0.5, 1.5),
                a[1] + rng.uniform(0.2, 2.0),
                rng.uniform(0.0, 1.0),
            ];
            nodes.push(FirstGenIn {
                bbox: [a, b],
                dom: [u0, v0, u1, v1],
            });
        }
    }
    OpIn::TreeTop { nu, nv, nodes }
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let max_deg = if rng.chance(0.3) { 2 } else { 4 };
    let s = gen_srf(rng, max_deg);
    let d = domain_of_srf(&s);
    let b = gen_bss(rng, s.clone());
    let op = match rng.below(20) {
        0 => {
            let a = gvec(rng, 3.0);
            let p = match rng.below(4) {
                // collinear
                0 => {
                    let dir = gvec(rng, 1.0);
                    let t1 = rng.uniform(0.5, 2.0);
                    let t2 = rng.uniform(2.5, 4.0);
                    [
                        a,
                        [a[0] + t1 * dir[0], a[1] + t1 * dir[1], a[2] + t1 * dir[2]],
                        [a[0] + t2 * dir[0], a[1] + t2 * dir[1], a[2] + t2 * dir[2]],
                    ]
                }
                // short edge
                1 => [a, [a[0] + 1e-6, a[1], a[2]], gvec(rng, 3.0)],
                _ => [a, gvec(rng, 3.0), gvec(rng, 3.0)],
            };
            OpIn::Plane3 {
                p,
                tol: *rng.choose(&[0.0001, 0.0, 0.01]).unwrap_or(&0.0001),
            }
        }
        1 => OpIn::Halfspace {
            base: gvec(rng, 2.0),
            normal: if rng.chance(0.5) {
                [0.0, 0.0, if rng.chance(0.5) { 1.0 } else { -1.0 }]
            } else {
                gvec(rng, 1.0)
            },
        },
        2..=4 => OpIn::BPlanes {
            dom: gen_sub_dom(rng, d),
            index: rng.below(4) as usize,
        },
        5 => OpIn::BezApprox {
            dom: gen_sub_dom(rng, d),
        },
        6 => OpIn::SizeCorners {
            c: if rng.chance(0.15) {
                [[1.0, 1.0, 1.0]; 4]
            } else {
                [gvec(rng, 3.0), gvec(rng, 3.0), gvec(rng, 3.0), gvec(rng, 3.0)]
            },
        },
        7 => OpIn::SizeSurf {
            dom: gen_sub_dom(rng, d),
        },
        8 => OpIn::Corners {
            dom: gen_sub_dom(rng, d),
            bez: rng.chance(0.5).then(|| gen_srf(rng, 3)),
        },
        9 | 10 => {
            let na = rng.below(6) as usize;
            let alts: Vec<AltIn> = (0..na)
                .map(|_| AltIn {
                    ty: rng.below(2) as u8,
                    approx: *rng.choose(&[0.01, 0.001, 0.1]).unwrap_or(&0.01),
                    angle: *rng.choose(&[0.1, 0.2]).unwrap_or(&0.1),
                    key: gen_key(rng),
                    c3: rng
                        .chance(0.7)
                        .then(|| (0..rng.below(3)).map(|_| gen_curve(rng)).collect()),
                    cuv: rng
                        .chance(0.5)
                        .then(|| (0..rng.below(3)).map(|_| gen_curve(rng)).collect()),
                })
                .collect();
            let queries = (0..=rng.below(4))
                .map(|_| {
                    let base = rng.choose(&alts).cloned();
                    match base {
                        Some(a) if rng.chance(0.7) => QueryIn {
                            ty: if rng.chance(0.9) { a.ty } else { 1 - a.ty },
                            approx: a.approx + if rng.chance(0.8) { 0.0 } else { 1e-9 },
                            angle: a.angle,
                            key: perturb_key(rng, &a.key),
                            w3: rng.chance(0.7),
                            wuv: rng.chance(0.5),
                        },
                        _ => QueryIn {
                            ty: rng.below(2) as u8,
                            approx: 0.01,
                            angle: 0.1,
                            key: gen_key(rng),
                            w3: rng.chance(0.5),
                            wuv: rng.chance(0.5),
                        },
                    }
                })
                .collect();
            OpIn::Alt { alts, queries }
        }
        11 => gen_tree_top(rng, d),
        12..=14 | 18 => {
            let sub = gen_sub_dom(rng, d);
            let tree = gen_tree(rng, sub, false);
            let prog = gen_prog(rng, &tree);
            OpIn::Subdiv { tree, prog }
        }
        15 => {
            let ud = 1 + s.u.windows(2).filter(|w| w.first() != w.last()).count();
            let vd = 1 + s.v.windows(2).filter(|w| w.first() != w.last()).count();
            let nu = rng.below(ud as u64 + 1) as usize;
            let nv = rng.below(vd as u64 + 1) as usize;
            OpIn::Cont {
                u: (0..nu).map(|_| rng.below(14) as u8).collect(),
                v: (0..nv).map(|_| rng.below(14) as u8).collect(),
                dom: gen_sub_dom(rng, d),
            }
        }
        16 => {
            let cache = gen_sub_dom(rng, d);
            let calls = (0..=rng.below(4))
                .map(|_| {
                    let dd = if rng.chance(0.6) { cache } else { gen_sub_dom(rng, d) };
                    (dd, *rng.choose(&[0.001, 0.01, 0.0005]).unwrap_or(&0.001))
                })
                .collect();
            OpIn::Iso { cache, calls }
        }
        17 => {
            let sub = gen_sub_dom(rng, d);
            let tree = gen_tree(rng, sub, true);
            let n = tree.len();
            let nodes = (0..=rng.below(4)).map(|_| rng.below(n as u64) as usize).collect();
            OpIn::Polar { tree, nodes }
        }
        _ => match rng.below(3) {
            0 => OpIn::Cull {
                v: (0..rng.below(10)).map(|i| i as f64 * 1.5).collect(),
            },
            1 => OpIn::Face,
            _ => OpIn::Misc {
                dom: gen_sub_dom(rng, d),
                test: rng.chance(0.5),
                uv: [rng.uniform(d[0] - 0.5, d[2] + 0.5), rng.uniform(d[1] - 0.5, d[3] + 0.5)],
            },
        },
    };
    let srf = (!matches!(op, OpIn::Plane3 { .. } | OpIn::Cull { .. })).then_some(b);
    Inputs { srf, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_cache_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let i = gen_case(rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
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
        default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp_output(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach the ops with successful outcomes on the Rust side (the polar
    /// boxes, the Bezier approximation and `GetFace` are always deferred until
    /// `SmSurface::EvaluateNormal`, `IsSingularity` and `GetFace` are ported).
    #[test]
    fn generated_cases_cover_ops() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..3000u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let i = gen_case(&mut rng);
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_err() {
                continue;
            }
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.unit)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{}", st.unwrap_or(f64::NAN))
            };
            *tally.entry((format!("{op:?}"), kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            println!("{k:?}: {v}");
        }
        for op in [
            "Plane3", "Halfspace", "BPlanes", "SizeCorners", "SizeSurf", "Corners", "Alt",
            "TreeTop", "Subdiv", "Cont", "Iso", "Misc", "Cull",
        ] {
            assert!(
                tally.keys().any(|(n, k)| n == op && k == "status:0"),
                "{op} never succeeds"
            );
        }
    }
}
