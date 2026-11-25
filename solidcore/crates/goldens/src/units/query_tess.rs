//! Runner for unit `query.tess`: the tessellation surface cache `SmTessSrfCache` (constructor,
//! type queries and getters, `TestAgainstTolerances` with plain and view based drivers on
//! hand-built nodes, `BuildTreeWithSubdivision`, `ImplantUVSegment` and `MarkFaceContainment`
//! on hand-built trees with a poly B-rep boundary, the whole `BuildTree`), the tessellation
//! drivers and `SmPolyToEUVUAttr`.
//!
//! Ported from upstream `source/SMLib/src/SmTess.cpp`; checked against its golden (the face
//! is a bare `SmFace` holding the surface without owning it, the poly B-rep one face of
//! closed loops in the uv plane, trees are built by hand). Rust: `solidcore::query::tess`.
//!
//! A case has an `op` and (except `attr`) a `setup`: a surface, the cache tolerances, a
//! driver, an optional poly B-rep, an optional tree and the cache state. The Rust side
//! produces the sections: `status` (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce), `res`, `res_i`, `state`, `dom`, `nodes` and `node_geom`. Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::attribute::{AttributeValue, Behavior, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
};
use solidcore::kernel::Kernel;
use solidcore::math::{Point3, Vec2, Vec3};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::query::poly::{PolyBrep, PolyEdgeId};
use solidcore::query::tess::{
    self, SurfaceTessDriver, SurfaceTessDriverOptions, TessHandle, TessSrfCache,
    ViewBasedTessDriverOptions,
};
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::FaceContainment;
use solidcore::surface::surface_cache::{BezierAux2d, SurfaceCacheOptions};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::cases::contract;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// The face key of the cache (any label; the C++ side compares the pointer).
const FACE_KEY: ObjectKey = ObjectKey(77);

/// `query.tess` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
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

/// A bounded plane `SmPlane(o, x, y, scale, dom)` (an `SmBSplineSurface`).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaneIn {
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    scale: [f64; 2],
    dom: [f64; 4],
}

/// The tessellation driver: `kind` 0 none, 1 plain, 2 view based.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct DriverIn {
    kind: u8,
    #[serde(default)]
    view: [f64; 3],
    #[serde(default)]
    sil_ch: f64,
    #[serde(default = "default_sil_ang")]
    sil_ang: f64,
    /// `ch ang max_edge min_edge ratio aspect`.
    opts: [f64; 6],
}

fn default_sil_ang() -> f64 {
    5.0
}

/// A closed poly loop: points `[x, y, z, vertex tol]`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct LoopIn {
    etol: f64,
    pts: Vec<[f64; 4]>,
}

/// The poly B-rep: one face of closed loops.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PolyIn {
    brep_tol: f64,
    face_tol: f64,
    /// Original face label, `0` = none.
    label: u64,
    loops: Vec<LoopIn>,
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

/// The cache state before the op.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StateIn {
    point_test: bool,
    #[serde(default)]
    have_t: bool,
    #[serde(default)]
    containment: u8,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    dom: Option<[f64; 4]>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Setup {
    /// The B-spline surface (ignored when `plane` is set).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    srf: Option<BssIn>,
    /// A plane surface instead of `srf`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    plane: Option<PlaneIn>,
    /// `ch ang max_edge min_edge ratio aspect` (constructor order).
    opts: [f64; 6],
    driver: DriverIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    poly: Option<PolyIn>,
    #[serde(default)]
    tree: Vec<NodeIn>,
    state: StateIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    New {
        kinds: Vec<i64>,
    },
    Test {
        nodes: Vec<usize>,
    },
    BuildSub,
    Implant {
        edges: Vec<usize>,
    },
    Mark,
    Build,
    Attr {
        id: u64,
        behavior: u8,
        eu: u64,
        vu: u64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(flatten)]
    op: OpIn,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    setup: Option<Setup>,
}

// ---- file readers -------------------------------------------------------------------

#[path = "fixtures/query_tess_sms.rs"]
mod query_tess_sms;

/// Reads an upstream `.sms` text file: `n m`, `p q`, rational flag, `(n+1)(m+1)`
/// points (`x y z`, or `wx wy wz w` when rational, v index fastest), `n+p+2` u knots,
/// `m+q+2` v knots.
fn read_sms(path: &str) -> Result<InlineSrf, HarnessError> {
    let text = std::fs::read_to_string(path).map_err(|e| harness(format!("{path}: {e}")))?;
    let parsed = query_tess_sms::parse(&text).map_err(|e| harness(format!("{path}: {e}")))?;
    Ok(InlineSrf {
        net: parsed.net,
        p: parsed.p,
        q: parsed.q,
        u: parsed.u,
        v: parsed.v,
    })
}

fn srf(s: &SrfIn) -> Result<InlineSrf, HarnessError> {
    match s {
        SrfIn::File { sms } => read_sms(sms),
        SrfIn::Inline(i) => Ok(i.clone()),
    }
}

// ---- input contract -----------------------------------------------------------------

/// The largest control point index of a surface direction.
const MAX_INDEX: usize = 24;
/// The largest surface degree.
const MAX_DEGREE: usize = 5;
/// The largest magnitude of a coordinate, knot or scalar.
const MAX_COORD: f64 = 1e6;
/// The most tree nodes.
const MAX_NODES: usize = 256;
/// The most loops of a poly B-rep.
const MAX_LOOPS: usize = 8;
/// The most points of a poly B-rep loop.
const MAX_LOOP_POINTS: usize = 64;
/// The largest id, label or index the wrapper passes on.
const MAX_ID: u64 = 1_000_000;

fn fin(v: f64) -> bool {
    contract::finite(v, MAX_COORD)
}

fn non_negative(v: f64) -> bool {
    fin(v) && v >= 0.0
}

/// The domain of a B-spline surface within the size bounds, with a full net of finite
/// points of one rationality (weights in `[1e-3, 1e3]`) and clamped knots within range.
fn bspline_surface(b: &BssIn) -> Option<[f64; 4]> {
    let s = srf(&b.srf).ok()?;
    let n = s.net.len().checked_sub(1)?;
    let m = s.net.first()?.len().checked_sub(1)?;
    let (r, q_) = (s.u.len().checked_sub(1)?, s.v.len().checked_sub(1)?);
    let w0 = s.net.first()?.first()?[3];
    let ok = b.form <= 11
        && s.net.iter().all(|row| row.len() == m + 1)
        && n <= MAX_INDEX
        && m <= MAX_INDEX
        && s.p <= MAX_DEGREE
        && s.q <= MAX_DEGREE
        && r <= MAX_INDEX + MAX_DEGREE + 1
        && q_ <= MAX_INDEX + MAX_DEGREE + 1
        && (1..=n).contains(&s.p)
        && (1..=m).contains(&s.q)
        && r == n + s.p + 1
        && q_ == m + s.q + 1
        && s.net.iter().flatten().all(|&[x, y, z, w]| {
            [x, y, z].iter().all(|&c| fin(c))
                && (w == NOW) == (w0 == NOW)
                && (w == NOW || (1e-3..=1e3).contains(&w))
        })
        && s.u.iter().chain(&s.v).all(|&k| fin(k));
    let (u0, u1) = contract::clamped(&s.u, s.p)?;
    let (v0, v1) = contract::clamped(&s.v, s.q)?;
    ok.then_some([u0, v0, u1, v1])
}

/// An ordered finite domain inside `outer` (and, when `strict`, not smaller than `1e-6` of
/// it in either direction).
fn domain_in(d: [f64; 4], outer: [f64; 4], strict: bool) -> bool {
    let o = outer;
    d.iter().all(|&v| fin(v))
        && d[0] <= d[2]
        && d[1] <= d[3]
        && d[0] >= o[0]
        && d[1] >= o[1]
        && d[2] <= o[2]
        && d[3] <= o[3]
        && !(strict && (d[2] - d[0] < 1e-6 * (o[2] - o[0]) || d[3] - d[1] < 1e-6 * (o[3] - o[1])))
}

/// The edge count of a poly B-rep of non-negative tolerances, a label up to [`MAX_ID`] and
/// one to [`MAX_LOOPS`] loops of three to [`MAX_LOOP_POINTS`] finite points with
/// non-negative tolerances.
fn poly_edges(p: &PolyIn) -> Option<usize> {
    let ok = non_negative(p.brep_tol)
        && non_negative(p.face_tol)
        && p.label <= MAX_ID
        && (1..=MAX_LOOPS).contains(&p.loops.len())
        && p.loops.iter().all(|l| {
            non_negative(l.etol)
                && (3..=MAX_LOOP_POINTS).contains(&l.pts.len())
                && l.pts
                    .iter()
                    .all(|&[x, y, z, t]| fin(x) && fin(y) && fin(z) && non_negative(t))
        });
    ok.then(|| p.loops.iter().map(|l| l.pts.len()).sum())
}

/// Whether a tree keeps the contract: at most [`MAX_NODES`] nodes with links inside the
/// tree, aux types 4 or 6, split, class and domain bounds, node 0 the root, and every node
/// reached once from it through pairs of distinct children that point back.
fn tree_ok(tree: &[NodeIn], dom: [f64; 4]) -> bool {
    let n = tree.len();
    if n > MAX_NODES {
        return false;
    }
    if n == 0 {
        return true;
    }
    let link = |v: i64| (-1..n as i64).contains(&v);
    let nodes = tree.iter().all(|t| {
        link(t.parent)
            && link(t.c1)
            && link(t.c2)
            && matches!(t.aux, 4 | 6)
            && t.split <= 2
            && t.class <= 3
            && domain_in(t.dom, dom, false)
    });
    if !nodes || tree.first().map(|t| t.parent) != Some(-1) {
        return false;
    }
    let mut seen = vec![false; n];
    let mut stack = vec![0usize];
    while let Some(k) = stack.pop() {
        let (Some(s), Some(t)) = (seen.get_mut(k), tree.get(k)) else {
            return false;
        };
        if *s {
            return false;
        }
        *s = true;
        match (t.c1, t.c2) {
            (-1, -1) => {}
            (a, b) if a >= 0 && b >= 0 && a != b => {
                for c in [a as usize, b as usize] {
                    if tree.get(c).map(|l| l.parent) != Some(k as i64) {
                        return false;
                    }
                    stack.push(c);
                }
            }
            _ => return false,
        }
    }
    seen.iter().all(|&s| s)
}

/// Whether a case keeps the input contract ported from the reference's wrapper: the
/// attribute op's id and behavior bounds; otherwise a setup with a plane
/// ([`contract::plane`]) or a B-spline surface ([`bspline_surface`]), finite options, a
/// driver kind up to 2, a poly B-rep as [`poly_edges`] takes it, a tree as [`tree_ok`]
/// takes it, the state's containment and domain, and each op's needs (a driver where
/// upstream reads it unchecked, a tree, or none for a subdivision build, a poly B-rep, node
/// and edge indices in range).
fn keeps_contract(i: &Inputs) -> bool {
    if let OpIn::Attr {
        id,
        behavior,
        eu,
        vu,
    } = &i.op
    {
        return *id <= MAX_ID && *behavior <= 4 && *eu <= MAX_ID && *vu <= MAX_ID;
    }
    let Some(s) = &i.setup else {
        return false;
    };
    let dom = match (&s.plane, &s.srf) {
        (Some(p), _) => {
            contract::plane(&p.o, &p.x, &p.y, p.scale, p.dom, MAX_COORD).then_some(p.dom)
        }
        (None, Some(b)) => bspline_surface(b),
        (None, None) => None,
    };
    let Some(dom) = dom else {
        return false;
    };
    let d = &s.driver;
    let edges = s.poly.as_ref().map(poly_edges);
    let n = s.tree.len();
    let setup = s.opts.iter().all(|&v| fin(v))
        && d.kind <= 2
        && d.view
            .iter()
            .chain([&d.sil_ch, &d.sil_ang])
            .chain(&d.opts)
            .all(|&v| fin(v))
        && edges.is_none_or(|e| e.is_some())
        && tree_ok(&s.tree, dom)
        && s.state.containment <= 2
        && s.state.dom.is_none_or(|sd| domain_in(sd, dom, true));
    let edges = edges.flatten();
    setup
        && match &i.op {
            OpIn::New { kinds } => {
                kinds.len() <= 16 && kinds.iter().all(|&k| (0..=MAX_ID as i64).contains(&k))
            }
            OpIn::Test { nodes } => {
                d.kind != 0 && n > 0 && nodes.len() <= 8 && nodes.iter().all(|&k| k < n)
            }
            OpIn::BuildSub => d.kind != 0 && n == 0,
            OpIn::Implant { edges: es } => {
                edges.is_some_and(|e| n > 0 && es.len() <= 16 && es.iter().all(|&k| k < e))
            }
            OpIn::Mark => edges.is_some() && n > 0,
            OpIn::Build => d.kind != 0,
            OpIn::Attr { .. } => true,
        }
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

fn class_of(v: u8) -> Result<NodeClass, HarnessError> {
    match v {
        0 => Ok(NodeClass::Unknown),
        1 => Ok(NodeClass::Inside),
        2 => Ok(NodeClass::Outside),
        3 => Ok(NodeClass::OnBoundary),
        _ => Err(harness(format!("bad node class {v}"))),
    }
}

fn split_of(v: u8) -> SplitDir {
    match v {
        0 => SplitDir::U,
        1 => SplitDir::V,
        _ => SplitDir::Unknown,
    }
}

fn split_code(s: SplitDir) -> i64 {
    match s {
        SplitDir::U => 0,
        SplitDir::V => 1,
        SplitDir::Unknown => 2,
    }
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn link(i: i64) -> Option<NodeId> {
    usize::try_from(i).ok().map(NodeId)
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
fn status(e: &SolidError) -> i64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007,
        e => solidcore::error::upstream_status(e).map_or(-1, i64::from),
    }
}

fn containment_code(c: FaceContainment) -> i64 {
    match c {
        FaceContainment::NotDone => 0,
        FaceContainment::Done => 1,
        FaceContainment::UnderConstruction => 2,
    }
}

fn containment_of(v: u8) -> FaceContainment {
    match v {
        1 => FaceContainment::Done,
        2 => FaceContainment::UnderConstruction,
        _ => FaceContainment::NotDone,
    }
}

fn driver_of(d: &DriverIn) -> Option<SurfaceTessDriver> {
    let base = SurfaceTessDriverOptions {
        chord_height: d.opts[0],
        ang_tol_deg: d.opts[1],
        max_edge_length_3d: d.opts[2],
        min_edge_length_3d: d.opts[3],
        min_edge_length_ratio_uv: d.opts[4],
        max_aspect_ratio: d.opts[5],
    };
    match d.kind {
        1 => Some(SurfaceTessDriver::new(base)),
        2 => Some(SurfaceTessDriver::view_based(
            Vec3::new(d.view[0], d.view[1], d.view[2]),
            ViewBasedTessDriverOptions {
                silhouette_chord_height: d.sil_ch,
                silhouette_angle_tolerance_deg: d.sil_ang,
                base,
            },
        )),
        _ => None,
    }
}

fn hs(e: &SolidError) -> HarnessError {
    harness(format!("poly construction: {e}"))
}

/// The poly B-rep and its edges in creation order.
fn poly_of(k: &Kernel, p: &PolyIn) -> Result<(PolyBrep, Vec<PolyEdgeId>), HarnessError> {
    let mut b = PolyBrep::new(k, p.brep_tol);
    let r = b.add_region().map_err(|e| hs(&e))?;
    let sh = b.add_shell(r).map_err(|e| hs(&e))?;
    let f = b.add_face(k, sh, p.face_tol, None).map_err(|e| hs(&e))?;
    let label = (p.label > 0).then_some(ObjectKey(p.label));
    b.face_set_original_face(f, label, false, None)
        .map_err(|e| hs(&e))?;
    let mut edges = Vec::new();
    for l in &p.loops {
        let mut vs = Vec::new();
        for pt in &l.pts {
            let v = b
                .add_vertex(k, Point3::new(pt[0], pt[1], pt[2]), pt[3])
                .map_err(|e| hs(&e))?;
            vs.push((v, Point3::new(pt[0], pt[1], pt[2])));
        }
        let lp = b.start_poly_edge_loop(f).map_err(|e| hs(&e))?;
        // each point to the next, the last back to the first
        let next = vs.iter().cycle().skip(1);
        for (&(v0, p0), &(_, p1)) in vs.iter().zip(next) {
            let e = b
                .add_poly_edge(k, lp, l.etol, p0, p1, v0)
                .map_err(|e| hs(&e))?;
            edges.push(e);
        }
        b.finish_poly_edge_loop(lp).map_err(|e| hs(&e))?;
    }
    Ok((b, edges))
}

/// Builds the hand-made tree `t` in `cache` (node 0 the top).
fn put_tree(cache: &mut TessSrfCache, t: &[NodeIn]) -> Result<(), HarnessError> {
    if t.is_empty() {
        return Ok(());
    }
    let mut tree: Tree<TessHandle> = Tree::new(Some(TreeOwner::TessSrfCache));
    let mut auxs = Vec::new();
    for (k, n) in t.iter().enumerate() {
        let aux_type = AuxDataType::from_code(i32::from(n.aux))
            .ok_or_else(|| harness(format!("bad aux type {}", n.aux)))?;
        auxs.push(BezierAux2d {
            uv_domain: dom_of(n.dom),
            owning_tree_node: Some(NodeId(k)),
            ..BezierAux2d::default()
        });
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
                surface: Some(TessHandle::CacheSurface),
                slot: Some(k),
            }),
            geom_type: NodeGeomType::Default,
            // (calloc'ed node manager memory: an all-zero box)
            bbox: solidcore::geom::extent3d::Extent3d::from_bounds(0.0, 0.0, 0.0, 0.0, 0.0, 0.0),
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    *cache.base_mut().auxs_mut() = auxs;
    cache.base_mut().set_tree(Some(tree));
    Ok(())
}

fn class_code(c: NodeClass) -> i64 {
    i64::from(c.code())
}

fn face_label(h: Option<TessHandle>) -> i64 {
    match h {
        Some(TessHandle::Object(k)) => k.0 as i64,
        _ => 0,
    }
}

/// The `state`, `dom`, `nodes` and `node_geom` sections.
fn dump(c: &mut TessSrfCache, edges: &[PolyEdgeId], out: Output) -> Result<Output, HarnessError> {
    let original = match c.ts_poly_brep() {
        None => -1,
        Some(pb) => {
            let f = pb
                .first_poly_face()
                .map_err(|e| harness(e.to_string()))?
                .ok_or_else(|| harness("no poly face"))?;
            pb.face_original_face(f)
                .map_err(|e| harness(e.to_string()))?
                .map_or(0, |k| k.0 as i64)
        }
    };
    let subs: Vec<TessHandle> = c
        .base_mut()
        .subdivision_surfaces_mut()
        .iter()
        .map(|(h, _)| *h)
        .collect();
    let b = c.base();
    let state = vec![
        i64::from(u8::from(b.point_test_enabled)),
        i64::from(u8::from(b.have_t_surface_cache)),
        containment_code(b.face_containment_done),
        i64::from(u8::from(b.process_boundary_curves)),
        i64::from(u8::from(b.switch_to_fast_subdivision)),
        subs.len() as i64,
        original,
    ];
    let d = b.uv_domain();
    let dom = vec![
        d.min().x,
        d.min().y,
        d.max().x,
        d.max().y,
        b.options().chord_height_tolerance,
        b.options().ang_tol_rad,
    ];
    let mut ni: Vec<i64> = Vec::new();
    let mut nf: Vec<f64> = Vec::new();
    if let Some(t) = b.tree() {
        let ix = |n: Option<NodeId>| n.map_or(-1, |n| n.0 as i64);
        ni.push(t.nodes().len() as i64);
        ni.push(ix(t.top_node()));
        for id in 0..t.nodes().len() {
            let n = t.node(NodeId(id)).map_err(|e| harness(e.to_string()))?;
            let NodeData::Surface(a) = n.data else {
                return Err(harness("node without surface data"));
            };
            let aux = b.aux(NodeId(id)).map_err(|e| harness(e.to_string()))?;
            ni.extend([
                ix(n.parent),
                ix(n.child1),
                ix(n.child2),
                i64::from(n.aux_type.code()),
                split_code(a.split_dir),
                i64::from(u8::from(a.is_marked)),
                class_code(a.node_class),
                face_label(a.face),
            ]);
            let surf = match a.surface {
                None => -1,
                Some(TessHandle::CacheSurface) => -3,
                Some(h) => subs.iter().position(|&s| s == h).map_or(-2, |p| p as i64),
            };
            ni.push(surf);
            ni.push(i64::from(u8::from(aux.chord_height_satisfied)));
            ni.push(i64::from(u8::from(aux.angle_tol_satisfied)));
            ni.push(aux.poly_edge_list.len() as i64);
            for h in &aux.poly_edge_list {
                let k = match h {
                    TessHandle::PolyEdge(e) => {
                        edges.iter().position(|x| x == e).map_or(-2, |p| p as i64)
                    }
                    _ => -2,
                };
                ni.push(k);
            }
            let ud = aux.uv_domain;
            nf.extend([ud.min().x, ud.min().y, ud.max().x, ud.max().y]);
            let bb = n.bbox;
            nf.extend([
                bb.min().x,
                bb.min().y,
                bb.min().z,
                bb.max().x,
                bb.max().y,
                bb.max().z,
            ]);
        }
    }
    Ok(out
        .ints("state", state)
        .values("dom", dom)
        .ints("nodes", ni)
        .values("node_geom", nf))
}

fn attr_rust(id: u64, behavior: u8, eu: u64, vu: u64) -> Result<Output, HarnessError> {
    let b = Behavior::from_code(i32::from(behavior)).map_err(|e| harness(e.to_string()))?;
    let mut a = tess::poly_to_euvu_attribute(id, b);
    let ptrs = |a: &solidcore::geom::attribute::Attribute| match a.value() {
        AttributeValue::PolyToEuVu { edgeuse, vertexuse } => (
            edgeuse.map_or(0, |k| k.0 as i64),
            vertexuse.map_or(0, |k| k.0 as i64),
        ),
        _ => (-9, -9),
    };
    let (e0, v0) = ptrs(&a);
    let mut r = vec![e0, v0];
    *a.value_mut() = AttributeValue::PolyToEuVu {
        edgeuse: (eu > 0).then_some(ObjectKey(eu)),
        vertexuse: (vu > 0).then_some(ObjectKey(vu)),
    };
    let cp = a.make_copy();
    for x in [&a, &cp] {
        let (e, v) = ptrs(x);
        r.extend([
            x.attribute_id() as i64,
            i64::from(x.behavior().code()),
            x.type_code(),
            e,
            v,
        ]);
    }
    Ok(Output::new().ints("res_i", r))
}

#[allow(clippy::too_many_lines)]
fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    if let OpIn::Attr {
        id,
        behavior,
        eu,
        vu,
    } = &i.op
    {
        return attr_rust(*id, *behavior, *eu, *vu);
    }
    let s = i.setup.as_ref().ok_or_else(|| harness("missing setup"))?;
    let k = Kernel::default();
    // the cache surface, and the B-spline (part) handed to BuildTreeWithSubdivision
    let (surface, bss) = if let Some(p) = &s.plane {
        let v3 = |a: [f64; 3]| Vec3::new(a[0], a[1], a[2]);
        let plane = Plane::new(
            v3(p.o),
            v3(p.x),
            v3(p.y),
            Vec2::new(p.scale[0], p.scale[1]),
            &dom_of(p.dom),
        );
        // (upstream's GetNaturalUVDomain and copy constructor make the plane's NURB)
        let bss = plane
            .base_with_nurb()
            .map_err(|e| harness(format!("plane NURB: {e}")))?
            .into_owned();
        (Surface::from(plane), bss)
    } else {
        let bss = bss_of(s.srf.as_ref().ok_or_else(|| harness("missing surface"))?)?;
        (Surface::from(bss.copy()), bss)
    };
    let (poly, edges) = match &s.poly {
        None => (None, Vec::new()),
        Some(p) => {
            let (b, e) = poly_of(&k, p)?;
            (Some(b), e)
        }
    };
    let o = s.opts;
    let opts = SurfaceCacheOptions {
        chord_height_tolerance: o[0],
        ang_tol_rad: o[1],
        max_side_length_3d: o[2],
        min_side_length_3d: o[3],
        min_side_length_ratio_uv: o[4],
        aspect_ratio_3d: o[5],
    };
    let mut c = TessSrfCache::new(FACE_KEY, surface, poly, driver_of(&s.driver), opts);
    put_tree(&mut c, &s.tree)?;
    {
        let b = c.base_mut();
        b.point_test_enabled = s.state.point_test;
        b.have_t_surface_cache = s.state.have_t;
        b.face_containment_done = containment_of(s.state.containment);
        if let Some(d) = s.state.dom {
            b.set_uv_domain(dom_of(d));
        }
    }

    let mut st: Vec<i64> = Vec::new();
    let mut res: Vec<f64> = Vec::new();
    let mut fail: Option<SolidError> = None;
    let mut record = |r: solidcore::Result<()>, st: &mut Vec<i64>| -> bool {
        match r {
            Ok(()) => {
                st.push(0);
                true
            }
            Err(e) if e.is_unported() => {
                fail = Some(e);
                false
            }
            Err(e) => {
                st.push(status(&e));
                false
            }
        }
    };
    match &i.op {
        OpIn::New { kinds } => {
            res.push(c.type_code() as f64);
            res.extend(kinds.iter().map(|&t| flag(c.is_kind_of(t))));
            res.push(flag(c.face() == FACE_KEY));
            res.push(flag(c.ts_poly_brep().is_some() == s.poly.is_some()));
            let b = c.base();
            res.push(flag(b.face_was_modified));
            let p = b.options();
            res.extend([
                p.aspect_ratio_3d,
                p.max_side_length_3d,
                p.min_side_length_3d,
                p.min_side_length_ratio_uv,
                b.trim_subdivision_factor,
            ]);
            if let Some(d) = c.driver() {
                res.push(d.type_code() as f64);
                res.push(flag(d.eval_based_tessellation));
                let q = d.options;
                res.extend([
                    q.chord_height,
                    q.ang_tol_deg,
                    q.max_aspect_ratio,
                    q.max_edge_length_3d,
                    q.min_edge_length_3d,
                    q.min_edge_length_ratio_uv,
                ]);
                if let (Some(v), Some(ch), Some(ang)) = (
                    d.view_vector(),
                    d.silhouette_chord_height(),
                    d.silhouette_angle_tolerance(),
                ) {
                    res.extend([v.x, v.y, v.z, ch, ang]);
                }
            }
            st.push(0);
        }
        OpIn::Test { nodes } => {
            for &n in nodes {
                match c.test_against_tolerances(&k, NodeId(n)) {
                    Ok(t) => {
                        st.push(0);
                        res.push(flag(t.needs_subdivision));
                        res.push(
                            if t.direction == solidcore::nurbs::b_spline_surface::SurfParamType::U {
                                0.0
                            } else {
                                1.0
                            },
                        );
                    }
                    Err(e) => {
                        let _ = record(Err(e), &mut st);
                        break;
                    }
                }
            }
        }
        OpIn::BuildSub => {
            let r = c.build_tree_with_subdivision(&k, &bss);
            record(r, &mut st);
        }
        OpIn::Implant { edges: es } => {
            for &e in es {
                let id = *edges.get(e).ok_or_else(|| harness("bad edge"))?;
                if !record(c.implant_uv_segment(&k, id), &mut st) {
                    break;
                }
            }
        }
        OpIn::Mark => {
            let r = c.mark_face_containment(&k);
            record(r, &mut st);
        }
        OpIn::Build => {
            let r = c.build_tree(&k);
            record(r, &mut st);
        }
        OpIn::Attr { .. } => {}
    }
    if let Some(e) = fail {
        return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
    }
    let out = Output::new().ints("status", st).values("res", res);
    dump(&mut c, &edges, out)
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

/// A surface: a bumpy grid, a plane, a cylinder-like sheet, one with a collapsed side,
/// a dome; `scale` multiplies the coordinates.
fn gen_srf(rng: &mut Rng, max_deg: usize, scale: f64) -> InlineSrf {
    let p = 1 + rng.below(max_deg as u64) as usize;
    let q = 1 + rng.below(max_deg as u64) as usize;
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let kind = rng.below(6);
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
                        4 => {
                            // a dome: normals turn through the view direction
                            let (a, b) = (i as f64 / n as f64 - 0.5, j as f64 / m as f64 - 0.5);
                            z = 2.0 * (1.0 - 2.0 * (a * a + b * b));
                        }
                        _ => {}
                    }
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [x * scale * w, y * scale * w, z * scale * w, w]
                    } else {
                        [x * scale, y * scale, z * scale, NOW]
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

/// A bounded plane (unit or skew axes, scaled) of coordinates about `scale`.
fn gen_plane(rng: &mut Rng, scale: f64) -> PlaneIn {
    let v = |rng: &mut Rng| {
        [
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        ]
    };
    let x = if rng.chance(0.5) {
        [1.0, 0.0, 0.0]
    } else {
        v(rng)
    }
    .map(|c| c * scale);
    let y = if rng.chance(0.5) {
        [0.0, 1.0, 0.0]
    } else {
        v(rng)
    }
    .map(|c| c * scale);
    let o = v(rng).map(|c| c * 5.0 * scale);
    let (a, b) = (rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0));
    PlaneIn {
        o,
        x,
        y,
        scale: [rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)],
        dom: [a, b, a + rng.uniform(0.1, 4.0), b + rng.uniform(0.1, 4.0)],
    }
}

fn domain_of_srf(s: &InlineSrf) -> [f64; 4] {
    let u0 = s.u.first().copied().unwrap_or(0.0);
    let u1 = s.u.last().copied().unwrap_or(1.0);
    let v0 = s.v.first().copied().unwrap_or(0.0);
    let v1 = s.v.last().copied().unwrap_or(1.0);
    [u0, v0, u1, v1]
}

/// A random subdivision tree over `dom` (binary splits of random leaves), with
/// `SM_AD_AUX_DATA` leaves and some pre-classified ones.
fn gen_tree(rng: &mut Rng, dom: [f64; 4], max_splits: u64, classify: bool) -> Vec<NodeIn> {
    let mut t = vec![NodeIn {
        parent: -1,
        c1: -1,
        c2: -1,
        aux: 6,
        split: 0,
        marked: false,
        class: 0,
        dom,
    }];
    let splits = rng.below(max_splits + 1) as usize;
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
        let aux = if rng.chance(0.7) { 6u8 } else { 4 };
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
                aux: 6,
                split: 0,
                marked: false,
                class: 0,
                dom: dd,
            });
        }
    }
    if classify {
        for n in &mut t {
            if n.c1 >= 0 {
                continue;
            }
            let (class, marked) = match rng.below(12) {
                0 => (1, true),
                1 => (2, true),
                2 | 3 => (3, false),
                4 => (3, true),
                5 => (1, false),
                _ => (0, false),
            };
            n.class = class;
            n.marked = marked;
        }
    }
    t
}

/// A closed polygon inside (or reaching out of) `dom`: a rectangle, a star or a
/// rotated triangle, optionally with a hole.
fn gen_poly(rng: &mut Rng, dom: [f64; 4]) -> PolyIn {
    let (lu, lv) = (dom[2] - dom[0], dom[3] - dom[1]);
    let c = [
        dom[0] + rng.uniform(0.3, 0.7) * lu,
        dom[1] + rng.uniform(0.3, 0.7) * lv,
    ];
    let reach = if rng.chance(0.1) { 0.7 } else { 0.4 };
    let tol = if rng.chance(0.5) {
        1e-6
    } else {
        rng.uniform(1e-7, 1e-3)
    };
    let ring = |rng: &mut Rng, scale: f64, k: usize, star: bool, rev: bool| -> LoopIn {
        let phase = rng.uniform(0.0, 1.0);
        let mut pts: Vec<[f64; 4]> = (0..k)
            .map(|j| {
                let a = (j as f64 + phase) / k as f64 * std::f64::consts::TAU;
                let r = if star && j % 2 == 1 { 0.5 } else { 1.0 } * scale;
                [c[0] + r * a.cos() * lu, c[1] + r * a.sin() * lv, 0.0, tol]
            })
            .collect();
        if rev {
            pts.reverse();
        }
        LoopIn { etol: tol, pts }
    };
    let shape = rng.below(3);
    let outer = match shape {
        0 => ring(rng, reach, 4, false, false),
        1 => ring(rng, reach, 10, true, false),
        _ => {
            let k = 3 + rng.below(6) as usize;
            ring(rng, reach, k, false, false)
        }
    };
    let mut loops = vec![outer];
    if rng.chance(0.3) {
        loops.push(ring(rng, reach * 0.3, 4, false, true));
    }
    PolyIn {
        brep_tol: tol,
        face_tol: tol,
        label: if rng.chance(0.85) { 5 } else { 0 },
        loops,
    }
}

fn gen_driver(rng: &mut Rng, size: f64) -> DriverIn {
    let kind = if rng.chance(0.5) { 1 } else { 2 };
    let view = match rng.below(4) {
        0 => [0.0, 0.0, 1.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0, 1.0, 0.0],
        _ => [
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        ],
    };
    DriverIn {
        kind,
        view,
        sil_ch: if rng.chance(0.4) {
            0.0
        } else {
            rng.uniform(0.002, 0.05) * size
        },
        sil_ang: match rng.below(3) {
            0 => 5.0,
            // (stored as the radian tolerance: small values bite)
            1 => rng.uniform(0.01, 0.3),
            _ => rng.uniform(0.0, 30.0),
        },
        opts: [0.0, 25.0, 0.0, 0.0, 0.001, 0.0],
    }
}

/// Cache tolerances `ch ang max_edge min_edge ratio aspect` for a surface of size
/// `size`; coarse enough to keep trees small.
fn gen_opts(rng: &mut Rng, size: f64) -> [f64; 6] {
    [
        if rng.chance(0.2) {
            0.0
        } else {
            rng.uniform(0.005, 0.1) * size
        },
        if rng.chance(0.2) {
            0.0
        } else {
            rng.uniform(0.2, 0.8)
        },
        if rng.chance(0.8) {
            0.0
        } else {
            rng.uniform(0.3, 2.0) * size
        },
        if rng.chance(0.8) {
            0.0
        } else {
            rng.uniform(0.05, 0.3) * size
        },
        if rng.chance(0.6) {
            0.001
        } else {
            rng.uniform(0.01, 0.2)
        },
        if rng.chance(0.85) {
            0.0
        } else {
            rng.uniform(1.5, 10.0)
        },
    ]
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Inputs {
    let scale = match rng.below(10) {
        0 => 1e-3,
        1 => 1e3,
        _ => 1.0,
    };
    let s = gen_srf(rng, 3, scale);
    let mut d = domain_of_srf(&s);
    let plane = rng.chance(0.12).then(|| {
        let p = gen_plane(rng, scale);
        d = p.dom;
        p
    });
    let srf = BssIn {
        oob: rng.chance(0.1),
        form: if rng.chance(0.85) {
            11
        } else {
            rng.below(10) as u8
        },
        srf: SrfIn::Inline(s),
    };
    let size = 3.0 * scale;
    let mut opts = gen_opts(rng, size);
    if opts[0] == 0.0 && opts[1] == 0.0 {
        opts[1] = 0.5;
    }
    let driver = gen_driver(rng, size);
    let state = StateIn {
        point_test: rng.chance(0.5),
        have_t: false,
        containment: 0,
        dom: None,
    };
    let base = Setup {
        srf: Some(srf),
        plane,
        opts,
        driver,
        poly: None,
        tree: Vec::new(),
        state,
    };
    match rng.below(12) {
        0 => {
            let pool = [15150, 15120, 15100, 15000, 15152, 15154, 16002, 0];
            let mut setup = base;
            if rng.chance(0.2) {
                setup.driver.kind = 0;
            }
            if rng.chance(0.3) {
                setup.poly = Some(gen_poly(rng, d));
            }
            Inputs {
                op: OpIn::New {
                    kinds: (0..=rng.below(5))
                        .map(|_| *rng.choose(&pool).unwrap_or(&15150))
                        .collect(),
                },
                setup: Some(setup),
            }
        }
        1 => Inputs {
            op: OpIn::Attr {
                id: rng.below(2000),
                behavior: rng.below(5) as u8,
                eu: if rng.chance(0.3) {
                    0
                } else {
                    1 + rng.below(1000)
                },
                vu: if rng.chance(0.3) {
                    0
                } else {
                    1 + rng.below(1000)
                },
            },
            setup: None,
        },
        2 | 3 => {
            let mut setup = base;
            if rng.chance(0.5) {
                // coarse tolerances: nodes pass the surface tests, then the silhouette test
                setup.opts[0] = rng.uniform(0.3, 1.0) * size;
                setup.opts[1] = rng.uniform(1.0, 2.5);
                setup.driver.kind = 2;
            }
            setup.tree = gen_tree(rng, d, 6, false);
            let n = setup.tree.len() as u64;
            let leaves: Vec<usize> = (0..setup.tree.len())
                .filter(|&k| setup.tree.get(k).is_some_and(|t| t.c1 < 0))
                .collect();
            let nodes = (0..=rng.below(3))
                .map(|_| {
                    if rng.chance(0.85) {
                        rng.choose(&leaves).copied().unwrap_or(0)
                    } else {
                        rng.below(n) as usize
                    }
                })
                .collect();
            Inputs {
                op: OpIn::Test { nodes },
                setup: Some(setup),
            }
        }
        4 | 5 => Inputs {
            op: OpIn::BuildSub,
            setup: Some(base),
        },
        6 | 7 => {
            let mut setup = base;
            let poly = gen_poly(rng, d);
            let ne: usize = poly.loops.iter().map(|l| l.pts.len()).sum();
            setup.poly = Some(poly);
            setup.tree = gen_tree(rng, d, 14, false);
            if rng.chance(0.3) {
                let (lu, lv) = (d[2] - d[0], d[3] - d[1]);
                setup.state.dom = Some([
                    d[0] + rng.uniform(0.0, 0.2) * lu,
                    d[1] + rng.uniform(0.0, 0.2) * lv,
                    d[2] - rng.uniform(0.0, 0.2) * lu,
                    d[3] - rng.uniform(0.0, 0.2) * lv,
                ]);
            }
            let edges = (0..ne).filter(|_| rng.chance(0.8)).collect();
            Inputs {
                op: OpIn::Implant { edges },
                setup: Some(setup),
            }
        }
        8 | 9 => {
            let mut setup = base;
            setup.poly = Some(gen_poly(rng, d));
            setup.tree = gen_tree(rng, d, 14, true);
            Inputs {
                op: OpIn::Mark,
                setup: Some(setup),
            }
        }
        _ => {
            let mut setup = base;
            if rng.chance(0.85) {
                setup.poly = Some(gen_poly(rng, d));
            }
            Inputs {
                op: OpIn::Build,
                setup: Some(setup),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.tess"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = (0..64).map(|_| gen_case(rng)).find(keeps_contract)?;
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse()?;
        rust(&i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The kind of an [`OpIn`].
    #[derive(Debug, Clone, Copy, PartialEq, Eq)]
    enum Op {
        New,
        Test,
        BuildSub,
        Implant,
        Mark,
        Build,
        Attr,
    }

    impl OpIn {
        fn kind(&self) -> Op {
            match self {
                Self::New { .. } => Op::New,
                Self::Test { .. } => Op::Test,
                Self::BuildSub => Op::BuildSub,
                Self::Implant { .. } => Op::Implant,
                Self::Mark => Op::Mark,
                Self::Build => Op::Build,
                Self::Attr { .. } => Op::Attr,
            }
        }
    }

    #[test]
    fn cyclic_trees_fail_softly() {
        // fuzz00241 / fuzz00353 (`tests/fuzz_corpus/query.tess.json`): a tree node
        // whose child is itself. The FFI refuses the tree, so the runner never hands it
        // to the port; run directly, `mark_face_containment` used to descend the cycle
        // forever and now returns an error status.
        let corpus: Value =
            serde_json::from_str(include_str!("../../tests/fuzz_corpus/query.tess.json")).unwrap();
        for name in ["fuzz00241_rust_hang", "fuzz00353_rust_hang"] {
            let case = corpus["cases"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == name)
                .unwrap();
            let input: Inputs = serde_json::from_value(case["inputs"].clone()).unwrap();
            let out = rust(&input).unwrap();
            assert_ne!(out.ints["status"], vec![0], "{name}: {out:?}");
        }
    }

    /// The generators reach every op with successful outcomes on the Rust side, real
    /// subdivision (split subdivision surfaces), silhouette nodes and classified nodes.
    #[test]
    fn generated_cases_cover_ops() {
        let mut tally: BTreeMap<String, usize> = BTreeMap::new();
        for seed in 0..MIN_GENERATED as u64 {
            let mut rng = Rng::new(0x7e55_0000 + seed);
            let i = gen_case(&mut rng);
            if !keeps_contract(&i) {
                continue;
            }
            let op = i.op.kind();
            let out = rust(&i).expect("harness");
            if let Some(d) = &out.deferred {
                *tally
                    .entry(format!("{op:?}:deferred:{}", d.unit))
                    .or_default() += 1;
                continue;
            }
            let st = out.ints.get("status").cloned().unwrap_or_default();
            *tally
                .entry(format!("{op:?}:status:{:?}", st.last()))
                .or_default() += 1;
            let nodes = out.ints.get("nodes").cloned().unwrap_or_default();
            let n = nodes.first().copied().unwrap_or(0);
            if n > 8 {
                *tally.entry(format!("{op:?}:big_tree")).or_default() += 1;
            }
            let res = out.values.get("res").cloned().unwrap_or_default();
            if op == Op::Test && res.first() == Some(&1.0) {
                *tally.entry("Test:needs".into()).or_default() += 1;
            }
            // the silhouette retest changes the outcome of a view based driver
            if let (Some(setup), Op::Test | Op::BuildSub) = (&i.setup, op) {
                if setup.driver.kind == 2 {
                    let mut plain = i.clone();
                    if let Some(s) = plain.setup.as_mut() {
                        s.driver.kind = 1;
                    }
                    let other = rust(&plain).expect("harness");
                    if other.values != out.values || other.ints != out.ints {
                        *tally.entry(format!("{op:?}:silhouette")).or_default() += 1;
                    }
                }
            }
            // classes present after mark / build
            let mut at = 2usize;
            for _ in 0..n {
                let class = nodes.get(at + 6).copied().unwrap_or(-1);
                let surf = nodes.get(at + 8).copied().unwrap_or(-1);
                let k = nodes.get(at + 11).copied().unwrap_or(0) as usize;
                *tally.entry(format!("{op:?}:class:{class}")).or_default() += 1;
                if surf >= 0 {
                    *tally.entry(format!("{op:?}:subdivision_node")).or_default() += 1;
                }
                at += 12 + k;
            }
        }
        for (k, v) in &tally {
            println!("{k}: {v}");
        }
        for key in [
            "New:status:Some(0)",
            "Test:status:Some(0)",
            "Test:needs",
            "BuildSub:status:Some(0)",
            "BuildSub:big_tree",
            "BuildSub:subdivision_node",
            "BuildSub:silhouette",
            "Test:silhouette",
            "Implant:status:Some(0)",
            "Mark:status:Some(0)",
            "Mark:class:1",
            "Mark:class:2",
            "Build:status:Some(0)",
            "Build:class:1",
            "Build:class:2",
            "Build:class:3",
        ] {
            assert!(tally.get(key).copied().unwrap_or(0) > 0, "never: {key}");
        }
    }
}
