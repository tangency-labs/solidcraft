//! Runner for unit `surface.trim_srf_cache`: the trimmed surface cache `SmTrimSrfCache`
//! (constructor state and type queries, `PointTest` with point testing off, `ImplantEdgeuse`
//! on hand-built decomposition trees, and `BuildTree` / `PointTest` / `PointClassify` on the
//! cache of a face).
//!
//! C++: `solidcore_ffi::units::surface_trim_srf_cache` (upstream
//! `source/SMLib/src/SmTrimSrfCache.cpp`; trees are built by hand, edge uses are bare
//! `SmEdgeuse` objects holding their UV trim curve, faces come from
//! `SmBrep::CreateFaceFromSurface`). Rust: `solidcore::surface::trim_srf_cache`.
//!
//! A case is a surface `srf` (`oob`, `form` and an inline or `.sms` surface) and an `op` with
//! its parameters. Both sides produce the flat record documented in the ffi module; the runner
//! reports its first entry as the `status` (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for
//! a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce) and the rest as `out`.
//!
//! Face cases independently construct the same naturally trimmed B-spline face as
//! the shim, then use the shared `BrepCacheFace` adapter for cache construction and
//! classification. Missing downstream services remain explicit deferrals. Every
//! run has at least [`MIN_GENERATED`] generated cases; generated inputs the ffi
//! wrapper rejects are redrawn.
//!
//! Deviations: the runner-local SMS reader validates counts below the exclusive
//! `2^usize::BITS` bound, checks dimension and payload arithmetic and available
//! tokens before constructing the net, and grows storage incrementally. Valid
//! point order, weights, knots and eager float parsing are unchanged.

#[path = "fixtures/trim_srf_cache_sms.rs"]
mod sms;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
};
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::FaceContainment;
use solidcore::surface::surface_cache::{BezierAux2d, BezierPatch, SurfaceCacheOptions};
use solidcore::surface::trim_srf_cache::TrimSrfCache;
use solidcore::SolidError;
use solidcore_ffi::units::surface_trim_srf_cache::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

#[path = "surface_trim_srf_cache/constructed_face.rs"]
mod constructed_face;

#[path = "surface_trim_srf_cache/native_face.rs"]
mod native_face;

#[path = "surface_trim_srf_cache/poly_edges.rs"]
mod poly_edges;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `surface.trim_srf_cache` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The handle of the cache surface in trees (edge uses are `0..`).
const SURFACE_HANDLE: i64 = -7;

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

/// A 2D curve: control points `[x, y, w]` as stored (`w` = `NOW` when not rational),
/// degree, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Crv2In {
    cpts: Vec<[f64; 3]>,
    p: usize,
    u: Vec<f64>,
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
    dom: [f64; 4],
}

/// One `ImplantEdgeuse` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CallIn {
    crv: Crv2In,
    #[serde(default)]
    subdiv: bool,
    /// Node to implant into, `-1` = all nodes meeting the curve box.
    node: i64,
}

/// A face point: `PointTest` with `tol` when `test_tol`, `PointClassify` with `tol`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PtIn {
    uv: [f64; 2],
    #[serde(default)]
    test_tol: bool,
    tol: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    New {
        /// `dChordHeightTolerance dAngleTolDeg dAspectRatio3D dMaxSideLength3D
        /// dMinSideLength3D dMinSideLengthRatioUV`.
        opts: [f64; 6],
        kinds: Vec<i64>,
    },
    PointTest {
        enabled: bool,
        modified: bool,
        uv: [f64; 2],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        tol: Option<f64>,
    },
    ImplantEu {
        dom: [f64; 4],
        factor: f64,
        tree: Vec<NodeIn>,
        /// Initial face extent (`None` = `SmExtent2d()`).
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fe: Option<[f64; 4]>,
        calls: Vec<CallIn>,
    },
    Face {
        build: bool,
        points: Vec<PtIn>,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            Self::New { .. } => Op::New,
            Self::PointTest { .. } => Op::PointTest,
            Self::ImplantEu { .. } => Op::ImplantEu,
            Self::Face { .. } => Op::Face,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    srf: BssIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- file readers -------------------------------------------------------------------

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let s = sms::parse_sms(&text, path)?;
    Ok(InlineSrf {
        net: s.net,
        p: s.p,
        q: s.q,
        u: s.u,
        v: s.v,
    })
}

fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
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

// ---- C++ side -----------------------------------------------------------------------

fn push_srf(o: &mut Vec<f64>, b: &BssIn) -> Result<(), HarnessError> {
    o.push(flag(b.oob));
    o.push(f64::from(b.form));
    let s = srf(&b.srf)?;
    let (n, m) = shape(&s)?;
    o.extend([n as f64, m as f64, s.p as f64, s.q as f64]);
    let ku =
        s.u.len()
            .checked_sub(1)
            .ok_or_else(|| harness("empty u knots"))?;
    let kv =
        s.v.len()
            .checked_sub(1)
            .ok_or_else(|| harness("empty v knots"))?;
    o.extend([ku as f64, kv as f64]);
    for row in &s.net {
        for p in row {
            o.extend(p);
        }
    }
    o.extend(&s.u);
    o.extend(&s.v);
    Ok(())
}

fn push_crv(o: &mut Vec<f64>, c: &Crv2In) {
    let n = c.cpts.len().saturating_sub(1);
    o.extend([
        2.0,
        n as f64,
        c.p as f64,
        (c.u.len().saturating_sub(1)) as f64,
    ]);
    for &[x, y, w] in &c.cpts {
        o.extend([x, y, NOZ, w]);
    }
    o.extend(&c.u);
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
        ]);
        o.extend(n.dom);
    }
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    push_srf(&mut o, &i.srf)?;
    match &i.op {
        OpIn::New { opts, kinds } => {
            o.extend(opts);
            o.push(kinds.len() as f64);
            o.extend(kinds.iter().map(|&k| k as f64));
        }
        OpIn::PointTest {
            enabled,
            modified,
            uv,
            tol,
        } => {
            o.extend([flag(*enabled), flag(*modified), uv[0], uv[1]]);
            o.extend([flag(tol.is_some()), tol.unwrap_or(0.0)]);
        }
        OpIn::ImplantEu {
            dom,
            factor,
            tree,
            fe,
            calls,
        } => {
            o.extend(dom);
            o.push(*factor);
            push_tree(&mut o, tree);
            o.push(flag(fe.is_none()));
            o.extend(fe.unwrap_or([0.0; 4]));
            o.push(calls.len() as f64);
            for c in calls {
                push_crv(&mut o, &c.crv);
                o.extend([flag(c.subdiv), c.node as f64]);
            }
        }
        OpIn::Face { build, points } => {
            o.push(flag(*build));
            o.push(points.len() as f64);
            for p in points {
                o.extend([p.uv[0], p.uv[1], flag(p.test_tol), p.tol]);
            }
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

fn curve_of(c: &Crv2In) -> Result<BSplineCurve, HarnessError> {
    let pw = c
        .cpts
        .iter()
        .map(|&[x, y, w]| CPoint::new(x, y, NOZ, w))
        .collect();
    let cur =
        NlCurve::new(pw, c.p, c.u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
    Ok(BSplineCurve::from_nurb(cur, 2, false))
}

fn class_of(v: u8) -> Result<NodeClass, HarnessError> {
    match v {
        0 => Ok(NodeClass::Unknown),
        1 => Ok(NodeClass::Inside),
        2 => Ok(NodeClass::Outside),
        3 => Ok(NodeClass::OnBoundary),
        _ => Err(harness(format!("bad node class {v}"))),
    }
}

fn class_code(c: NodeClass) -> f64 {
    match c {
        NodeClass::Unknown => 0.0,
        NodeClass::Inside => 1.0,
        NodeClass::Outside => 2.0,
        NodeClass::OnBoundary => 3.0,
    }
}

fn split_of(v: u8) -> SplitDir {
    match v {
        0 => SplitDir::U,
        1 => SplitDir::V,
        _ => SplitDir::Unknown,
    }
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn link(i: i64) -> Option<NodeId> {
    usize::try_from(i).ok().map(NodeId)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn push_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn containment_code(c: FaceContainment) -> f64 {
    match c {
        FaceContainment::NotDone => 0.0,
        FaceContainment::Done => 1.0,
        FaceContainment::UnderConstruction => 2.0,
    }
}

/// A trimmed cache of `s` (options: chord, angle, aspect, max, min, ratio).
fn cache_of(s: &BSplineSurface, opts: [f64; 6]) -> TrimSrfCache<i64> {
    let opts = SurfaceCacheOptions {
        chord_height_tolerance: opts[0],
        ang_tol_rad: opts[1],
        aspect_ratio_3d: opts[2],
        max_side_length_3d: opts[3],
        min_side_length_3d: opts[4],
        min_side_length_ratio_uv: opts[5],
    };
    let mut c = TrimSrfCache::new(Surface::from(s.copy()), opts);
    c.base_mut().set_surface_handle(Some(SURFACE_HANDLE));
    // the halves of split subdivision surfaces (any handles but SURFACE_HANDLE)
    c.base_mut()
        .set_subdivision_handles(Some(|n| 1000 + n as i64));
    c
}

const DEFAULT_OPTS: [f64; 6] = [0.0, 0.0, 0.0, 0.0, 0.0, 0.001];

/// Builds the decomposition tree of `t` in `cache` (node 0 the top).
fn build_tree(cache: &mut TrimSrfCache<i64>, t: &[NodeIn]) -> Result<(), HarnessError> {
    let mut tree: Tree<i64> = Tree::new(Some(TreeOwner::TrimSrfCache));
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
                face: None,
                surface: Some(SURFACE_HANDLE),
                slot: Some(slot),
            }),
            geom_type: NodeGeomType::Default,
            ..TreeNode::new()
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    *cache.base_mut().patches_mut() = patches;
    *cache.base_mut().auxs_mut() = auxs;
    cache.base_mut().set_tree(Some(tree));
    Ok(())
}

/// The Rust record (status first); `Err` for a Rust error to report as is.
type Run = Result<Vec<f64>, SolidError>;

#[allow(clippy::too_many_lines)]
fn rust_op(i: &Inputs) -> Result<Run, HarnessError> {
    let mut k = Kernel::default();
    let bss = bss_of(&i.srf)?;
    let mut o = vec![0.0];
    let run: Run = match &i.op {
        OpIn::New { opts, kinds } => {
            let c = cache_of(&bss, *opts);
            let b = c.base();
            o.push(c.type_code() as f64);
            o.extend(kinds.iter().map(|&t| flag(c.is_kind_of(t))));
            o.push(flag(b.face_was_modified));
            o.push(flag(b.process_boundary_curves));
            o.push(flag(b.point_test_enabled));
            o.push(flag(b.have_t_surface_cache));
            o.push(containment_code(b.face_containment_done));
            push_dom(&mut o, &b.uv_domain());
            o.push(b.trim_subdivision_factor);
            let p = b.options();
            o.extend([
                p.chord_height_tolerance,
                p.ang_tol_rad,
                p.aspect_ratio_3d,
                p.max_side_length_3d,
                p.min_side_length_3d,
                p.min_side_length_ratio_uv,
            ]);
            o.push(flag(b.tree().is_none()));
            Ok(o)
        }
        OpIn::PointTest {
            enabled,
            modified,
            uv,
            tol,
        } => {
            let mut c = cache_of(&bss, DEFAULT_OPTS);
            c.base_mut().point_test_enabled = *enabled;
            c.base_mut().face_was_modified = *modified;
            c.point_test(&k, Point2::new(uv[0], uv[1]), *tol)
                .map(|(ok, obj)| {
                    o.push(flag(ok));
                    o.push(obj.map_or(-1.0, |_| -2.0));
                    o
                })
        }
        OpIn::ImplantEu {
            dom,
            factor,
            tree,
            fe,
            calls,
        } => {
            let mut c = cache_of(&bss, DEFAULT_OPTS);
            c.base_mut().set_uv_domain(dom_of(*dom));
            c.base_mut().trim_subdivision_factor = *factor;
            c.base_mut().point_test_enabled = false;
            build_tree(&mut c, tree)?;
            let mut ext = fe.map_or_else(Extent2d::new, dom_of);
            let mut err = None;
            for (idx, call) in calls.iter().enumerate() {
                let crv = curve_of(&call.crv)?;
                match c.implant_edgeuse(
                    &k,
                    idx as i64,
                    Some(&crv),
                    call.subdiv,
                    &mut ext,
                    link(call.node),
                ) {
                    Ok(()) => o.push(0.0),
                    Err(e) if e.is_unported() => {
                        err = Some(e);
                        break;
                    }
                    Err(e) => {
                        o.push(status(&e));
                        break;
                    }
                }
            }
            if let Some(e) = err {
                Err(e)
            } else {
                push_dom(&mut o, &ext);
                // every node of the tree (the input nodes, then the subdivisions made)
                let t = c.base().tree().ok_or_else(|| harness("no tree"))?;
                o.push(t.nodes().len() as f64);
                for id in 0..t.nodes().len() {
                    let n = t.node(NodeId(id)).map_err(|e| harness(e.to_string()))?;
                    let NodeData::Surface(a) = n.data else {
                        return Err(harness("node lost its surface data"));
                    };
                    o.push(flag(a.is_marked));
                    o.push(class_code(a.node_class));
                    let aux = c
                        .base()
                        .aux(NodeId(id))
                        .map_err(|e| harness(e.to_string()))?;
                    o.push(aux.edgeuse_list.len() as f64);
                    o.extend(aux.edgeuse_list.iter().map(|&e| e as f64));
                }
                Ok(o)
            }
        }
        OpIn::Face { build, points } => constructed_face::run(&mut k, bss, *build, points),
    };
    Ok(run)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    Ok(match rust_op(i)? {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

// ---- generated cases ----------------------------------------------------------------

fn gen_knots(rng: &mut Rng, p: usize, n: usize, ends: (f64, f64)) -> Vec<f64> {
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
    let (a, b) = ends;
    let mut k = vec![a; p + 1];
    k.extend(inner.iter().map(|t| a + t * (b - a)));
    k.extend(vec![b; p + 1]);
    k
}

fn gen_ends(rng: &mut Rng) -> (f64, f64) {
    if rng.chance(0.5) {
        (0.0, 1.0)
    } else {
        let a = rng.uniform(-2.0, 2.0);
        (a, a + rng.uniform(0.5, 3.0))
    }
}

/// A surface: a bumpy grid, a plane, a cylinder-like sheet, or one with a collapsed
/// (degenerate) side.
fn gen_srf(rng: &mut Rng, max_deg: usize) -> InlineSrf {
    let p = 1 + rng.below(max_deg as u64) as usize;
    let q = 1 + rng.below(max_deg as u64) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let kind = rng.below(5);
    let amp = if kind == 1 {
        0.0
    } else {
        rng.uniform(0.0, 1.0)
    };
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
                        1 => z = 0.0,
                        2 => {
                            let t = i as f64 / n as f64 * 2.5;
                            x = t.cos() * 3.0;
                            z = t.sin() * 3.0;
                            y = j as f64;
                        }
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
    let (ue, ve) = (gen_ends(rng), gen_ends(rng));
    InlineSrf {
        net,
        p,
        q,
        u: gen_knots(rng, p, n, ue),
        v: gen_knots(rng, q, m, ve),
    }
}

fn gen_bss(rng: &mut Rng, s: InlineSrf) -> BssIn {
    BssIn {
        oob: rng.chance(0.1),
        form: if rng.chance(0.85) {
            11
        } else {
            rng.below(10) as u8
        },
        srf: SrfIn::Inline(s),
    }
}

fn domain_of_srf(s: &InlineSrf) -> [f64; 4] {
    let u0 = s.u.first().copied().unwrap_or(0.0);
    let u1 = s.u.last().copied().unwrap_or(1.0);
    let v0 = s.v.first().copied().unwrap_or(0.0);
    let v1 = s.v.last().copied().unwrap_or(1.0);
    [u0, v0, u1, v1]
}

/// A cache domain inside `d`: away from the natural boundaries (no pole sides) unless
/// `touch`.
fn gen_cache_dom(rng: &mut Rng, d: [f64; 4], touch: bool) -> [f64; 4] {
    let (lu, lv) = (d[2] - d[0], d[3] - d[1]);
    let mut m = [
        rng.uniform(0.02, 0.3),
        rng.uniform(0.02, 0.3),
        rng.uniform(0.02, 0.3),
        rng.uniform(0.02, 0.3),
    ];
    if touch {
        if let Some(s) = m.get_mut(rng.below(4) as usize) {
            *s = 0.0;
        }
    }
    if rng.chance(0.15) {
        // a long thin domain (node growth clamping)
        m[1] = 0.45;
        m[3] = 0.45 + rng.uniform(0.0, 0.04);
    }
    [
        d[0] + m[0] * lu,
        d[1] + m[1] * lv,
        d[2] - m[2] * lu,
        d[3] - m[3] * lv,
    ]
}

/// A random subdivision tree over `dom`: binary splits of random leaves.
fn gen_tree(rng: &mut Rng, dom: [f64; 4]) -> Vec<NodeIn> {
    let mut t = vec![NodeIn {
        parent: -1,
        c1: -1,
        c2: -1,
        aux: 3,
        split: 2,
        marked: false,
        class: 0,
        dom,
    }];
    let splits = rng.below(12) as usize;
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
        let f = if rng.chance(0.7) {
            0.5
        } else {
            rng.uniform(0.2, 0.8)
        };
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
                dom: dd,
            });
        }
    }
    // pre-classify some leaves (PropagateToParents meets mixed states)
    for n in &mut t {
        if n.c1 >= 0 {
            continue;
        }
        let (class, marked) = match rng.below(10) {
            0 => (1, true),
            1 => (2, true),
            2 => (3, true),
            3 => (1, false),
            _ => (0, false),
        };
        n.class = class;
        n.marked = marked;
    }
    t
}

/// A 2D curve in (or around) `dom`: a line, a polyline, or a curved B-spline.
fn gen_curve(rng: &mut Rng, dom: [f64; 4]) -> Crv2In {
    let (lu, lv) = (dom[2] - dom[0], dom[3] - dom[1]);
    let pt = |rng: &mut Rng| {
        [
            dom[0] + rng.uniform(-0.2, 1.2) * lu,
            dom[1] + rng.uniform(-0.2, 1.2) * lv,
        ]
    };
    let kind = rng.below(5);
    let rational = kind >= 3 && rng.chance(0.3);
    let (p, pts): (usize, Vec<[f64; 2]>) = match kind {
        // a straight segment
        0 => (1, vec![pt(rng), pt(rng)]),
        // an iso line (axis aligned)
        1 => {
            let a = pt(rng);
            let mut b = pt(rng);
            if rng.chance(0.5) {
                b[0] = a[0];
            } else {
                b[1] = a[1];
            }
            (1, vec![a, b])
        }
        // a degree 1 polyline
        2 => (1, (0..3 + rng.below(3)).map(|_| pt(rng)).collect()),
        // a curved spline
        _ => {
            let p = 2 + rng.below(2) as usize;
            let n = p + rng.below(3) as usize;
            (p, (0..=n).map(|_| pt(rng)).collect())
        }
    };
    let n = pts.len() - 1;
    let cpts = pts
        .iter()
        .map(|&[x, y]| {
            if rational {
                let w = rng.uniform(0.5, 2.0);
                [x * w, y * w, w]
            } else {
                [x, y, NOW]
            }
        })
        .collect();
    let ends = if rng.chance(0.7) {
        (0.0, 1.0)
    } else {
        gen_ends(rng)
    };
    Crv2In {
        cpts,
        p,
        u: gen_knots(rng, p, n, ends),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let s = gen_srf(rng, 3);
    let d = domain_of_srf(&s);
    let b = gen_bss(rng, s);
    let op = match rng.below(10) {
        0 => OpIn::New {
            opts: [
                if rng.chance(0.5) {
                    0.0
                } else {
                    rng.uniform(0.0, 0.1)
                },
                if rng.chance(0.5) {
                    0.0
                } else {
                    rng.uniform(0.0, 30.0)
                },
                if rng.chance(0.7) {
                    0.0
                } else {
                    rng.uniform(1.0, 10.0)
                },
                if rng.chance(0.7) {
                    0.0
                } else {
                    rng.uniform(0.1, 10.0)
                },
                if rng.chance(0.7) {
                    0.0
                } else {
                    rng.uniform(0.0, 0.1)
                },
                if rng.chance(0.5) {
                    0.001
                } else {
                    rng.uniform(0.0, 0.1)
                },
            ],
            kinds: {
                let pool = [15120, 15100, 15150, 15000, 11001, 16002, 15011, 0];
                (0..=rng.below(5))
                    .map(|_| *rng.choose(&pool).unwrap_or(&15120))
                    .collect()
            },
        },
        1 => {
            let (enabled, modified) = match rng.below(3) {
                0 => (false, false),
                1 => (false, true),
                _ => (true, true),
            };
            OpIn::PointTest {
                enabled,
                modified,
                uv: [
                    rng.uniform(d[0] - 0.5, d[2] + 0.5),
                    rng.uniform(d[1] - 0.5, d[3] + 0.5),
                ],
                tol: rng.chance(0.5).then(|| rng.uniform(1e-6, 1e-2)),
            }
        }
        2 => {
            let build = rng.chance(0.6);
            OpIn::Face {
                build,
                points: (0..rng.below(3) + u64::from(!build))
                    .map(|_| PtIn {
                        uv: [
                            rng.uniform(d[0] - 0.1, d[2] + 0.1),
                            rng.uniform(d[1] - 0.1, d[3] + 0.1),
                        ],
                        test_tol: rng.chance(0.5),
                        tol: rng.uniform(1e-6, 1e-3),
                    })
                    .collect(),
            }
        }
        _ => {
            let touch = rng.chance(0.08);
            let dom = gen_cache_dom(rng, d, touch);
            let tree = gen_tree(rng, dom);
            let nn = tree.len() as u64;
            let calls = (0..=rng.below(4))
                .map(|_| {
                    let node = if rng.chance(0.06) {
                        -1
                    } else {
                        rng.below(nn) as i64
                    };
                    CallIn {
                        crv: gen_curve(rng, dom),
                        subdiv: rng.chance(0.08),
                        node,
                    }
                })
                .collect();
            OpIn::ImplantEu {
                dom,
                factor: if rng.chance(0.5) {
                    20.0
                } else {
                    rng.uniform(1.0, 60.0)
                },
                tree,
                fe: rng.chance(0.4).then(|| {
                    let a = gen_cache_dom(rng, d, false);
                    if rng.chance(0.2) {
                        // a negative area extent is reset to the curve box
                        [a[2], a[1], a[0], a[3]]
                    } else {
                        a
                    }
                }),
                calls,
            }
        }
    };
    Inputs { srf: b, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.trim_srf_cache"
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
        let mut cases = default_cases(self, &ctx)?;
        cases.extend(poly_edges::cases()?);
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        if let Some(input) = case.inputs.get("native_face") {
            return native_face::run_cpp(input);
        }
        if let Some(input) = case.inputs.get("poly_edges") {
            return poly_edges::run_cpp(input);
        }
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        if let Some(input) = case.inputs.get("native_face") {
            return native_face::run_rust(input);
        }
        if let Some(input) = case.inputs.get("poly_edges") {
            return poly_edges::run_rust(input);
        }
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach every op with successful outcomes on the Rust side.
    #[test]
    fn generated_cases_cover_ops() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..2000u64 {
            let mut rng = Rng::new(0x5eed_1000 + seed);
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
            if let (OpIn::ImplantEu { calls, .. }, None) = (&i.op, &out.deferred) {
                // count the implanted edge uses per curve kind (line / curved)
                let o = out.values.get("out").cloned().unwrap_or_default();
                let mut at = calls.len() + 4;
                let count = o.get(at).copied().unwrap_or(0.0) as usize;
                at += 1;
                for _ in 0..count {
                    let k = o.get(at + 2).copied().unwrap_or(0.0) as usize;
                    for j in 0..k {
                        let id = o.get(at + 3 + j).copied().unwrap_or(-1.0) as usize;
                        let curved = calls.get(id).is_some_and(|c| c.crv.p > 1);
                        let key = if curved {
                            "implant:curved"
                        } else {
                            "implant:line"
                        };
                        *tally.entry(("ImplantEu".into(), key.into())).or_default() += 1;
                    }
                    at += 3 + k;
                }
            }
            *tally.entry((format!("{op:?}"), kind)).or_default() += 1;
        }
        for key in ["implant:curved", "implant:line"] {
            assert!(
                tally
                    .get(&("ImplantEu".to_string(), key.to_string()))
                    .copied()
                    .unwrap_or(0)
                    > 20,
                "too few {key}"
            );
        }
        for (k, v) in &tally {
            println!("{k:?}: {v}");
        }
        for op in ["New", "PointTest", "ImplantEu", "Face"] {
            assert!(
                tally.keys().any(|(n, k)| n == op && k == "status:0"),
                "{op} never succeeds"
            );
        }
    }
}
