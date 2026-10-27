//! Runner for unit `surface.surface_cache_p3`: `FindUVNodes`, `SubdivideNode`,
//! `SubdivideToTolerances`, `BuildTreeBase`, `BuildTree` and `GetTree` of
//! `SmSurfaceCache.cpp`.
//!
//! C++: `solidcore_ffi::units::surface_surface_cache_p3` (start nodes built by hand, as
//! `BuildTreeBase` builds its first generation). Rust: `solidcore::surface::surface_cache::p3`.
//!
//! A case is a surface `surf`, the cache options and an `op`. Both sides produce the flat
//! record documented in the ffi module: the op status (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `1007` for a Rust `InvalidInput`) and, for the tree operations, every node of
//! the tree in allocation order with its links, kind, split direction, domain, box, class,
//! flags and surface, and for Bezier patch nodes their continuities, pseudo box and
//! construction Bezier.
//!
//! Deferred cases: the paths into unported units (`SmSurface::CalculateContinuities` in
//! every tree build, `SmSurface::GetSingularities` in pole searches, `SmSurface::
//! EvaluateNormal` in the polar boxes of the angle tolerance test and
//! `SmSurface::FindTessellationSplits` in block subdivisions, the last two through part 2).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
};
use solidcore::geom::vector2d::Vec2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::math::NOW;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, BezierPatch, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_cache_p3::{self as ffi, Op};

use super::surface_surface_cache_p1::{
    bss_of, domain_of_srf, flag, gen_bss, gen_srf, gen_sub_dom, gvec, harness, push3, push_box,
    push_bss, push_dom, push_srf, srf, status, surface_of, v3, BssIn, InlineSrf, SrfIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `surface.surface_cache_p3` runner.
pub struct Runner;

/// The handle of the cache surface in trees.
const SURFACE_HANDLE: i64 = -7;

// ---- inputs -----------------------------------------------------------------------

/// A bounded plane `SmPlane(o, x, y, scale, dom)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(crate) struct PlaneIn {
    pub(crate) o: [f64; 3],
    pub(crate) x: [f64; 3],
    pub(crate) y: [f64; 3],
    pub(crate) scale: [f64; 2],
    pub(crate) dom: [f64; 4],
}

/// A surface: a B-spline, a plane, or a curve-bounded surface (implicit outer boundary, no
/// boundary curves) over a B-spline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum SurfIn {
    Bspline(BssIn),
    Plane(PlaneIn),
    CurveBounded(BssIn),
}

/// The cache options.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct OptsIn {
    #[serde(default)]
    chord: f64,
    #[serde(default)]
    ang: f64,
    #[serde(default)]
    aspect: f64,
    #[serde(default)]
    max_side: f64,
    #[serde(default)]
    min_side: f64,
    #[serde(default = "default_ratio")]
    ratio: f64,
    #[serde(default = "default_start")]
    start: f64,
}

fn default_ratio() -> f64 {
    0.01
}

fn default_start() -> f64 {
    30.0
}

/// A start node, set as `BuildTreeBase` sets a first generation node.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StartIn {
    /// `SM_AD_BEZIER_SURFACE` (3) or `SM_AD_AUX_DATA` (6).
    aux: u8,
    dom: [f64; 4],
    #[serde(default)]
    bbox: [f64; 6],
    #[serde(default)]
    class: u8,
    #[serde(default)]
    chsat: bool,
    #[serde(default)]
    angsat: bool,
    /// The cache's surface is on the node (`mBA_pSurface`).
    #[serde(default)]
    surf: bool,
    /// The patch (aux 3): a single span over `dom`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    bez: Option<SrfIn>,
    #[serde(default = "unit_basis")]
    basis: [[f64; 3]; 3],
    #[serde(default)]
    ivl: [[f64; 2]; 3],
    #[serde(default)]
    uc: [u8; 2],
    #[serde(default)]
    vc: [u8; 2],
}

fn unit_basis() -> [[f64; 3]; 3] {
    [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]]
}

/// A `SubdivideNode` call of a `Find` script.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SplitIn {
    node: usize,
    #[serde(default)]
    v: bool,
    #[serde(default)]
    only: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    SubdivNode {
        start: StartIn,
        #[serde(default)]
        v: bool,
        #[serde(default)]
        only: bool,
    },
    SubdivTol {
        start: StartIn,
    },
    Find {
        start: StartIn,
        #[serde(default)]
        splits: Vec<SplitIn>,
        dom: [f64; 4],
        #[serde(default)]
        poles: bool,
    },
    Build,
    Get,
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::SubdivNode { .. } => Op::SubdivNode,
            OpIn::SubdivTol { .. } => Op::SubdivTol,
            OpIn::Find { .. } => Op::Find,
            OpIn::Build => Op::Build,
            OpIn::Get => Op::Get,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surf: SurfIn,
    opts: OptsIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -----------------------------------------------------------------------

pub(crate) fn push_surf(o: &mut Vec<f64>, s: &SurfIn) -> Result<(), HarnessError> {
    match s {
        SurfIn::Bspline(b) => {
            o.push(0.0);
            push_bss(o, b)
        }
        SurfIn::Plane(p) => {
            o.push(1.0);
            o.extend(p.o);
            o.extend(p.x);
            o.extend(p.y);
            o.extend(p.scale);
            o.extend(p.dom);
            Ok(())
        }
        SurfIn::CurveBounded(b) => {
            o.push(2.0);
            push_bss(o, b)
        }
    }
}

fn push_opts(o: &mut Vec<f64>, p: &OptsIn) {
    o.extend([
        p.chord, p.ang, p.aspect, p.max_side, p.min_side, p.ratio, p.start,
    ]);
}

fn push_start(o: &mut Vec<f64>, t: &StartIn) -> Result<(), HarnessError> {
    o.push(f64::from(t.aux));
    o.extend(t.dom);
    o.extend(t.bbox);
    o.push(f64::from(t.class));
    o.extend([flag(t.chsat), flag(t.angsat), flag(t.surf)]);
    if t.aux == 3 {
        let b = t.bez.as_ref().ok_or_else(|| harness("aux 3 needs a Bezier"))?;
        push_srf(o, &srf(b)?)?;
        for b in t.basis {
            o.extend(b);
        }
        for [lo, hi] in t.ivl {
            o.extend([lo, hi]);
        }
        o.extend(t.uc.map(f64::from));
        o.extend(t.vc.map(f64::from));
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    push_surf(&mut o, &i.surf)?;
    push_opts(&mut o, &i.opts);
    match &i.op {
        OpIn::SubdivNode { start, v, only } => {
            push_start(&mut o, start)?;
            o.extend([flag(*v), flag(*only)]);
        }
        OpIn::SubdivTol { start } => push_start(&mut o, start)?,
        OpIn::Find {
            start,
            splits,
            dom,
            poles,
        } => {
            push_start(&mut o, start)?;
            o.push(splits.len() as f64);
            for s in splits {
                o.extend([s.node as f64, flag(s.v), flag(s.only)]);
            }
            o.extend(dom);
            o.push(flag(*poles));
        }
        OpIn::Build | OpIn::Get => {}
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

pub(crate) fn surface_in(s: &SurfIn) -> Result<Surface, HarnessError> {
    Ok(match s {
        SurfIn::Bspline(b) => Surface::from(bss_of(b)?),
        SurfIn::Plane(p) => Surface::from(Plane::new(
            v3(p.o),
            v3(p.x),
            v3(p.y),
            Vec2::new(p.scale[0], p.scale[1]),
            &dom_of(p.dom),
        )),
        SurfIn::CurveBounded(b) => Surface::from(CurveBoundedSurface::new(
            Surface::from(bss_of(b)?),
            Vec::new(),
            Vec::new(),
            true,
        ).map_err(|e| HarnessError::new(e.to_string()))?),
    })
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn options_of(p: &OptsIn) -> SurfaceCacheOptions {
    SurfaceCacheOptions {
        chord_height_tolerance: p.chord,
        ang_tol_rad: p.ang,
        aspect_ratio_3d: p.aspect,
        max_side_length_3d: p.max_side,
        min_side_length_3d: p.min_side,
        min_side_length_ratio_uv: p.ratio,
    }
}

fn cache_of(surface: Surface, opts: &OptsIn) -> SurfaceCache<i64> {
    // (runner helper without a kernel in scope)
    let dom = surface.natural_uv_domain_if_built();
    let mut c = SurfaceCache::new(surface, dom, options_of(opts));
    c.start_fast_subdivision_angle_deg = opts.start;
    c.set_surface_handle(Some(SURFACE_HANDLE));
    // the halves of split subdivision surfaces (any handles but SURFACE_HANDLE)
    c.set_subdivision_handles(Some(|n| 1000 + n as i64));
    c
}

fn node_class(code: u8) -> Result<NodeClass, HarnessError> {
    Ok(match code {
        0 => NodeClass::Unknown,
        1 => NodeClass::Inside,
        2 => NodeClass::Outside,
        3 => NodeClass::OnBoundary,
        _ => return Err(harness(format!("bad node class {code}"))),
    })
}

fn continuity(code: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match code {
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
        _ => return Err(harness(format!("bad continuity {code}"))),
    })
}

/// A cache over `surf` whose tree holds the start node `t` (node 0, the top).
fn start_cache(i: &Inputs, t: &StartIn) -> Result<SurfaceCache<i64>, HarnessError> {
    let surface = surface_in(&i.surf)?;
    let mut cache = cache_of(surface, &i.opts);
    let aux_type = AuxDataType::from_code(i32::from(t.aux))
        .ok_or_else(|| harness(format!("bad aux type {}", t.aux)))?;
    let mut tree: Tree<i64> = Tree::new(Some(TreeOwner::SurfaceCache));
    tree.set_owner_surface(Some(SURFACE_HANDLE));
    let data = BezierAux2d {
        uv_domain: dom_of(t.dom),
        chord_height_satisfied: t.chsat,
        angle_tol_satisfied: t.angsat,
        owning_tree_node: Some(NodeId(0)),
        ..BezierAux2d::default()
    };
    if aux_type == AuxDataType::BezierSurface {
        let b = t.bez.as_ref().ok_or_else(|| harness("aux 3 needs a Bezier"))?;
        let mut pb = PseudoBox::new();
        pb.set_basis(v3(t.basis[0]), v3(t.basis[1]), v3(t.basis[2]))
            .map_err(|e| harness(format!("basis: {e}")))?;
        pb.set_intervals(
            Extent1d::from_raw(t.ivl[0][0], t.ivl[0][1]),
            Extent1d::from_raw(t.ivl[1][0], t.ivl[1][1]),
            Extent1d::from_raw(t.ivl[2][0], t.ivl[2][1]),
        );
        *cache.patches_mut() = vec![BezierPatch {
            aux: data,
            bezier: Some(surface_of(&srf(b)?)?),
            pseudo_box: pb,
            u_curve_conts: [continuity(t.uc[0])?, continuity(t.uc[1])?],
            v_curve_conts: [continuity(t.vc[0])?, continuity(t.vc[1])?],
        }];
    } else {
        *cache.auxs_mut() = vec![data];
    }
    let [a, b, c, d, e, f] = t.bbox;
    tree.add_node(TreeNode {
        bbox: Extent3d::from_bounds(a, b, c, d, e, f),
        aux_type,
        data: NodeData::Surface(SurfaceAux {
            split_dir: SplitDir::U,
            is_marked: false,
            node_class: node_class(t.class)?,
            face: None,
            surface: t.surf.then_some(SURFACE_HANDLE),
            slot: Some(0),
        }),
        geom_type: NodeGeomType::Default,
        ..TreeNode::new()
    });
    tree.set_top_node(Some(NodeId(0)));
    cache.set_tree(Some(tree));
    Ok(cache)
}

fn split_code(s: SplitDir) -> f64 {
    match s {
        SplitDir::U => 0.0,
        SplitDir::V => 1.0,
        SplitDir::Unknown => 2.0,
    }
}

fn idx(n: Option<NodeId>) -> f64 {
    n.map_or(-1.0, |n| n.0 as f64)
}

fn push_nurb(o: &mut Vec<f64>, s: &solidcore::nurbs::srf_basic::Surface) -> Result<(), HarnessError> {
    let (n, m, r, ss) = s.array_sizes();
    let (p, q) = s.degrees();
    o.extend([n, m, p, q, r, ss].map(|x| x as f64));
    for i in 0..=n {
        for j in 0..=m {
            let c = s.cpt(i, j).map_err(|e| harness(format!("cpt: {e}")))?;
            o.extend([c.x, c.y, c.z, c.w]);
        }
    }
    o.extend_from_slice(s.u_knots());
    o.extend_from_slice(s.v_knots());
    Ok(())
}

/// Every node of the cache tree, as the shim's `put_tree`.
fn dump_tree(o: &mut Vec<f64>, cache: &SurfaceCache<i64>) -> Result<(), HarnessError> {
    let tree = cache.tree().ok_or_else(|| harness("no tree"))?;
    o.push(tree.nodes().len() as f64);
    for (k, n) in tree.nodes().iter().enumerate() {
        o.extend([idx(n.parent), idx(n.child1), idx(n.child2)]);
        o.push(f64::from(n.aux_type.code()));
        let NodeData::Surface(a) = n.data else {
            return Err(harness("node without surface data"));
        };
        o.push(split_code(a.split_dir));
        let aux = cache
            .aux(NodeId(k))
            .map_err(|e| harness(format!("aux: {e}")))?;
        push_dom(o, aux.uv_domain());
        push_box(o, &n.bbox);
        o.push(f64::from(a.node_class.code()));
        o.extend([
            flag(aux.chord_height_satisfied),
            flag(aux.angle_tol_satisfied),
            flag(a.is_marked),
            flag(aux.is_tess_only),
        ]);
        o.push(match a.surface {
            None => 0.0,
            Some(SURFACE_HANDLE) => 1.0,
            Some(_) => 2.0,
        });
        if n.aux_type == AuxDataType::BezierSurface {
            let p = cache
                .patch(NodeId(k))
                .map_err(|e| harness(format!("patch: {e}")))?;
            o.extend(p.u_curve_conts.map(|c| f64::from(c as u8)));
            o.extend(p.v_curve_conts.map(|c| f64::from(c as u8)));
            for b in p.pseudo_box.basis() {
                push3(o, *b);
            }
            for i in p.pseudo_box.intervals() {
                o.extend([i.min(), i.max()]);
            }
            o.push(flag(p.bezier.is_some()));
            if let Some(b) = &p.bezier {
                push_nurb(o, b)?;
            }
        }
    }
    o.push(idx(tree.top_node()));
    Ok(())
}

/// The outcome of a Rust op: the record, or an unported error.
type Run = Result<Vec<f64>, SolidError>;

/// The status of `r` (or the unported error, which defers the case).
fn status_of(r: &solidcore::Result<()>) -> Result<f64, SolidError> {
    match r {
        Ok(()) => Ok(0.0),
        Err(e) if e.is_unported() => Err(e.clone()),
        Err(e) => Ok(status(e)),
    }
}

fn dir_of(v: bool) -> SurfParamType {
    if v {
        SurfParamType::V
    } else {
        SurfParamType::U
    }
}

fn rust_op(i: &Inputs) -> Result<Run, HarnessError> {
    let k = Kernel::default();
    match &i.op {
        OpIn::SubdivNode { start, v, only } => {
            let mut cache = start_cache(i, start)?;
            let r = cache.subdivide_node(&k, NodeId(0), dir_of(*v), *only);
            let st = match status_of(&r) {
                Ok(s) => s,
                Err(e) => return Ok(Err(e)),
            };
            let mut o = vec![st];
            dump_tree(&mut o, &cache)?;
            Ok(Ok(o))
        }
        OpIn::SubdivTol { start } => {
            let mut cache = start_cache(i, start)?;
            let r = cache.subdivide_to_tolerances(&k, NodeId(0), 0);
            let st = match status_of(&r) {
                Ok(s) => s,
                Err(e) => return Ok(Err(e)),
            };
            let mut o = vec![st, flag(cache.switch_to_fast_subdivision)];
            dump_tree(&mut o, &cache)?;
            Ok(Ok(o))
        }
        OpIn::Find {
            start,
            splits,
            dom,
            poles,
        } => {
            let mut cache = start_cache(i, start)?;
            let mut stats = Vec::new();
            for s in splits {
                let r = cache.subdivide_node(&k, NodeId(s.node), dir_of(s.v), s.only);
                match status_of(&r) {
                    Ok(st) => stats.push(st),
                    Err(e) => return Ok(Err(e)),
                }
            }
            let found = match cache.find_uv_nodes(&k, &dom_of(*dom), *poles) {
                Ok(f) => f,
                Err(e) if e.is_unported() => return Ok(Err(e)),
                Err(e) => {
                    let mut o = vec![status(&e), stats.len() as f64];
                    o.extend(&stats);
                    o.push(0.0);
                    dump_tree(&mut o, &cache)?;
                    return Ok(Ok(o));
                }
            };
            let mut o = vec![0.0, stats.len() as f64];
            o.extend(&stats);
            o.push(found.len() as f64);
            o.extend(found.iter().map(|n| n.0 as f64));
            dump_tree(&mut o, &cache)?;
            Ok(Ok(o))
        }
        OpIn::Build => {
            let mut cache = cache_of(surface_in(&i.surf)?, &i.opts);
            let r = cache.build_tree(&k, false);
            let st = match status_of(&r) {
                Ok(s) => s,
                Err(e) => return Ok(Err(e)),
            };
            let mut o = vec![st];
            if st == 0.0 {
                dump_tree(&mut o, &cache)?;
            }
            Ok(Ok(o))
        }
        OpIn::Get => {
            let mut cache = cache_of(surface_in(&i.surf)?, &i.opts);
            let r = cache.get_tree(&k).map(|_| ());
            let st = match status_of(&r) {
                Ok(s) => s,
                Err(e) => return Ok(Err(e)),
            };
            let got = st == 0.0;
            let mut o = vec![if got { 0.0 } else { 1.0 }];
            if got {
                dump_tree(&mut o, &cache)?;
            }
            Ok(Ok(o))
        }
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    Ok(match rust_op(i)? {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

// ---- generated cases ----------------------------------------------------------------

/// Scales the net coordinates of `s` by `f` (huge / tiny surfaces).
fn scaled(mut s: InlineSrf, f: f64) -> InlineSrf {
    for row in &mut s.net {
        for p in row.iter_mut() {
            p[0] *= f;
            p[1] *= f;
            p[2] *= f;
        }
    }
    s
}

fn gen_inline(rng: &mut Rng) -> InlineSrf {
    let s = gen_srf(rng, 3);
    match rng.below(10) {
        0 => scaled(s, 1e-4),
        1 => scaled(s, 1e4),
        _ => s,
    }
}

fn gen_plane(rng: &mut Rng) -> PlaneIn {
    let x = if rng.chance(0.5) {
        [1.0, 0.0, 0.0]
    } else {
        gvec(rng, 1.0)
    };
    let y = if rng.chance(0.5) {
        [0.0, 1.0, 0.0]
    } else {
        gvec(rng, 1.0)
    };
    let a = rng.uniform(-2.0, 2.0);
    let b = rng.uniform(-2.0, 2.0);
    PlaneIn {
        o: gvec(rng, 5.0),
        x,
        y,
        scale: [rng.uniform(0.2, 3.0), rng.uniform(0.2, 3.0)],
        dom: [a, b, a + rng.uniform(0.1, 4.0), b + rng.uniform(0.1, 4.0)],
    }
}

pub(crate) fn gen_surf(rng: &mut Rng) -> SurfIn {
    match rng.below(10) {
        0 | 1 => SurfIn::Plane(gen_plane(rng)),
        2 => {
            let s = gen_inline(rng);
            SurfIn::CurveBounded(gen_bss(rng, s))
        }
        _ => {
            let s = gen_inline(rng);
            SurfIn::Bspline(gen_bss(rng, s))
        }
    }
}

pub(crate) fn surf_domain(s: &SurfIn) -> Result<[f64; 4], HarnessError> {
    Ok(match s {
        SurfIn::Bspline(b) | SurfIn::CurveBounded(b) => domain_of_srf(&srf(&b.srf)?),
        SurfIn::Plane(p) => p.dom,
    })
}

/// The largest coordinate magnitude of a surface (as the ffi bounds it).
pub(crate) fn surf_size(s: &SurfIn) -> Result<f64, HarnessError> {
    Ok(match s {
        SurfIn::Bspline(b) | SurfIn::CurveBounded(b) => srf(&b.srf)?
            .net
            .iter()
            .flatten()
            .map(|p| p[0].abs().max(p[1].abs()).max(p[2].abs()))
            .fold(1e-12, f64::max),
        SurfIn::Plane(p) => {
            let o = p.o.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
            let reach = p.dom.iter().fold(0.0_f64, |a, v| a.max(v.abs()));
            let n = |v: [f64; 3]| (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
            (o + reach * (n(p.x) * p.scale[0] + n(p.y) * p.scale[1])).max(1e-12)
        }
    })
}

/// Options whose subdivision stays bounded for a surface of size `size` (see the ffi).
fn gen_opts(rng: &mut Rng, size: f64) -> OptsIn {
    let length = |rng: &mut Rng, lo: f64| -> f64 {
        match rng.below(4) {
            0 => 0.0,
            1 => size * lo,
            2 => size * rng.uniform(lo, 0.1),
            _ => size * rng.uniform(0.1, 1.0),
        }
    };
    OptsIn {
        chord: length(rng, 2e-3),
        // the angle test needs SmSurface::EvaluateNormal (deferred): mostly off
        ang: if rng.chance(0.2) {
            rng.uniform(0.05, 1.5)
        } else {
            0.0
        },
        aspect: if rng.chance(0.3) {
            rng.uniform(1.5, 6.0)
        } else {
            0.0
        },
        max_side: if rng.chance(0.4) {
            length(rng, 2e-2)
        } else {
            0.0
        },
        min_side: if rng.chance(0.3) {
            size * rng.uniform(0.0, 0.2)
        } else {
            0.0
        },
        ratio: match rng.below(4) {
            0 => 0.01,
            1 => 0.5,
            2 => rng.uniform(0.01, 0.2),
            _ => 0.05,
        },
        start: match rng.below(3) {
            0 => 30.0,
            1 => rng.uniform(0.0, 90.0),
            _ => 600.0,
        },
    }
}

/// A single span patch of degrees `(p, q)` over `dom`, scaled like the surface.
fn gen_bez(rng: &mut Rng, size: f64, dom: [f64; 4], p: usize, q: usize) -> InlineSrf {
    let flat = rng.chance(0.25);
    let amp = if flat { 0.0 } else { rng.uniform(0.0, 0.6) };
    let rational = rng.chance(0.15);
    let net = (0..=p)
        .map(|i| {
            (0..=q)
                .map(|j| {
                    [
                        size * (i as f64 / p as f64 + 0.1 * rng.uniform(-1.0, 1.0)),
                        size * (j as f64 / q as f64 + 0.1 * rng.uniform(-1.0, 1.0)),
                        size * amp * rng.uniform(-1.0, 1.0),
                        if rational { rng.uniform(0.5, 2.0) } else { NOW },
                    ]
                })
                .collect()
        })
        .collect();
    let mut u = vec![dom[0]; p + 1];
    u.extend(vec![dom[2]; p + 1]);
    let mut v = vec![dom[1]; q + 1];
    v.extend(vec![dom[3]; q + 1]);
    InlineSrf { net, p, q, u, v }
}

/// A random orthonormal basis (or the axes).
fn gen_basis(rng: &mut Rng) -> [[f64; 3]; 3] {
    if rng.chance(0.4) {
        return unit_basis();
    }
    let n = |v: [f64; 3]| {
        let l = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt();
        [v[0] / l, v[1] / l, v[2] / l]
    };
    let cross = |a: [f64; 3], b: [f64; 3]| {
        [
            a[1] * b[2] - a[2] * b[1],
            a[2] * b[0] - a[0] * b[2],
            a[0] * b[1] - a[1] * b[0],
        ]
    };
    loop {
        let a = gvec(rng, 1.0);
        let b = gvec(rng, 1.0);
        let c = cross(a, b);
        let l = (c[0] * c[0] + c[1] * c[1] + c[2] * c[2]).sqrt();
        if l > 0.3 {
            let e1 = n(a);
            let e3 = n(c);
            let e2 = cross(e3, e1);
            return [e1, e2, e3];
        }
    }
}

fn gen_start(rng: &mut Rng, surf: &SurfIn, aux6: bool) -> Result<StartIn, HarnessError> {
    let sd = surf_domain(surf)?;
    let size = surf_size(surf)?;
    // a B-spline cache (an SmPlane is an SmBSplineSurface)
    let bspline = matches!(surf, SurfIn::Bspline(_) | SurfIn::Plane(_));
    let dom = if rng.chance(0.3) { sd } else { gen_sub_dom(rng, sd) };
    let (p, q) = if bspline {
        (1 + rng.below(3) as usize, 1 + rng.below(3) as usize)
    } else {
        (3, 3)
    };
    let lo = gvec(rng, size);
    let ext = [
        rng.uniform(0.0, size),
        rng.uniform(0.0, size),
        rng.uniform(0.0, size),
    ];
    let bbox = [lo[0], lo[1], lo[2], lo[0] + ext[0], lo[1] + ext[1], lo[2] + ext[2]];
    let cont = |rng: &mut Rng| -> u8 { rng.below(14) as u8 };
    Ok(StartIn {
        aux: if aux6 { 6 } else { 3 },
        dom,
        bbox,
        class: match rng.below(10) {
            0 => 3,
            1 => 1,
            2 => 2,
            _ => 0,
        },
        chsat: rng.chance(0.2),
        angsat: rng.chance(0.2),
        surf: if aux6 {
            bspline && rng.chance(0.3)
        } else {
            rng.chance(0.15)
        },
        bez: (!aux6).then(|| SrfIn::Inline(gen_bez(rng, size, dom, p, q))),
        basis: gen_basis(rng),
        ivl: [
            [-size, size * rng.uniform(0.0, 2.0)],
            [0.0, size * rng.uniform(0.0, 2.0)],
            [-size * rng.uniform(0.0, 0.3), size * rng.uniform(0.0, 0.3)],
        ],
        uc: [cont(rng), cont(rng)],
        vc: [cont(rng), cont(rng)],
    })
}

/// A query domain: a sub-domain, a point, an edge strip or a domain outside.
fn gen_query(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    match rng.below(6) {
        0 => {
            let u = d[0] + (d[2] - d[0]) * rng.unit();
            let v = d[1] + (d[3] - d[1]) * rng.unit();
            [u, v, u, v]
        }
        1 => [d[0], d[1], d[0] + (d[2] - d[0]) * 1e-3, d[3]],
        2 => {
            let w = d[2] - d[0];
            [d[2] + w * 0.5, d[1], d[2] + w, d[3]]
        }
        3 => d,
        _ => gen_sub_dom(rng, d),
    }
}

fn gen_case(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surf(rng);
    let size = surf_size(&surf)?;
    let opts = gen_opts(rng, size);
    let op = match rng.below(20) {
        0..=7 => {
            let aux6 = rng.chance(0.3);
            OpIn::SubdivNode {
                start: gen_start(rng, &surf, aux6)?,
                v: rng.chance(0.5),
                only: rng.chance(0.2),
            }
        }
        8..=12 => {
            let aux6 = matches!(surf, SurfIn::Bspline(_) | SurfIn::Plane(_)) && rng.chance(0.1);
            let mut start = gen_start(rng, &surf, aux6)?;
            if aux6 {
                start.surf = true;
            }
            OpIn::SubdivTol { start }
        }
        13..=17 => {
            let aux6 = rng.chance(0.25);
            let start = gen_start(rng, &surf, aux6)?;
            let mut leaves = vec![0usize];
            let mut count = 1usize;
            let mut splits = Vec::new();
            for _ in 0..rng.below(8) {
                let at = rng.below(leaves.len() as u64) as usize;
                let node = leaves.swap_remove(at);
                splits.push(SplitIn {
                    node,
                    v: rng.chance(0.5),
                    only: rng.chance(0.2),
                });
                leaves.extend([count, count + 1]);
                count += 2;
            }
            let dom = gen_query(rng, start.dom);
            OpIn::Find {
                start,
                splits,
                dom,
                poles: rng.chance(0.15),
            }
        }
        18 => OpIn::Build,
        _ => OpIn::Get,
    };
    Ok(Inputs { surf, opts, op })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_cache_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Ok(i) = gen_case(rng) else {
                continue;
            };
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
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
