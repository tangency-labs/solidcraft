//! Runner for unit `surface.surface_cache_p2`: `GetNthBoundingPlane`, `AddCurvesToCache`,
//! `TestAgainstTolerances`, `SubdivideByBlock` and the Bezier patch block builders
//! (`sm_CreateBezierPatchBlock`, `sm_CreateApproximateBezierPatchBlock`) of `SmSurfaceCache.cpp`.
//!
//! C++: `solidcore_ffi::units::surface_surface_cache_p2` (the file-static builders are verbatim
//! copies in the shim; subdivision nodes are built by hand). Rust:
//! `solidcore::surface::surface_cache::p2`.
//!
//! A case is a surface `surf` (a B-spline, a bounded plane or a curve-bounded surface over a
//! B-spline) and an `op` with its parameters. Both sides produce the flat record documented in
//! the ffi module; the runner reports its first entry as the `status` (`0` = `SM_SUCCESS`, else
//! the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce) and the rest as the `out` values.
//!
//! Deferred cases: the paths into unported units (`SmSurface::FindTessellationSplits` in every
//! block subdivision of a node with a surface, `SmSurface::IsSingularity` in the Bezier
//! approximation of B-splines, `SmSurface::CalculateBoundingBox` / `CreateIsoParametricCurve`
//! of planes and curve-bounded surfaces, `SmSurface::EvaluateNormal` in the bounding planes).
//! Every run has at least [`MIN_GENERATED`] generated cases; generated inputs the ffi wrapper
//! rejects are redrawn.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::tree::{
    AuxDataType, NodeClass, NodeData, NodeGeomType, NodeId, SplitDir, SurfaceAux, Tree, TreeNode,
    TreeOwner,
};
use solidcore::geom::vector2d::Vec2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::math::NOW;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::p1::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, BezierPatch, PatchBoundaryBoundingPlanes, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::surface::surface_cache::p2 as sc;
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_cache_p2::{self as ffi, Op};

use super::surface_surface_cache_p1::{
    alt_type, bss_of, curves_of, domain_of_srf, flag, gen_bss, gen_curve, gen_key, gen_srf,
    gen_sub_dom, gvec, harness, key_of, perturb_key, push3, push_box, push_bss, push_curves,
    push_dom, push_key, push_srf, srf, status, surface_of, v3, BssIn, CrvIn, InlineSrf, KeyIn,
    SrfIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.surface_cache_p2` runner.
pub struct Runner;

/// The handle of the cache surface in trees.
const SURFACE_HANDLE: i64 = -7;

// ---- inputs -----------------------------------------------------------------------

/// A bounded plane `SmPlane(o, x, y, scale, dom)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlaneIn {
    o: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    scale: [f64; 2],
    dom: [f64; 4],
}

/// A surface: a B-spline, a plane, or a curve-bounded surface (implicit outer boundary, no
/// boundary curves) over a B-spline.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SurfIn {
    Bspline(BssIn),
    Plane(PlaneIn),
    CurveBounded(BssIn),
    /// [`SurfIn::Bspline`] with its knots kept exactly as given: the shim writes them back
    /// over the NURB after `SmBSplineSurface(gw_SURFACE*, FALSE, ..)` has fixed them up
    /// (`sm_FixupKnotVector`); Rust builds it over a borrowed NURB, whose knots stay.
    RawBspline(BssIn),
    /// [`SurfIn::CurveBounded`] over a [`SurfIn::RawBspline`] basis.
    RawCurveBounded(BssIn),
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
    0.001
}

fn default_start() -> f64 {
    30.0
}

/// An `AddCurvesToCache` call.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct AddIn {
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

/// A `MatchCache` query of every cache.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct QueryIn {
    ty: u8,
    approx: f64,
    angle: f64,
    #[serde(default)]
    key: KeyIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Nth {
        dom: [f64; 4],
        idx: Vec<u8>,
    },
    Add {
        adds: Vec<AddIn>,
        #[serde(default)]
        queries: Vec<QueryIn>,
    },
    BezBlock,
    ApproxBlock,
    Test {
        cache_dom: [f64; 4],
        opts: OptsIn,
        aux: u8,
        #[serde(default)]
        children: bool,
        #[serde(default)]
        chsat: bool,
        #[serde(default)]
        angsat: bool,
        dom: [f64; 4],
        #[serde(default, skip_serializing_if = "Option::is_none")]
        bez: Option<SrfIn>,
        #[serde(default)]
        ivl: [[f64; 2]; 3],
    },
    SubdivBlock {
        opts: OptsIn,
        dom: [f64; 4],
        #[serde(default)]
        nosurf: bool,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Nth { .. } => Op::Nth,
            OpIn::Add { .. } => Op::Add,
            OpIn::BezBlock => Op::BezBlock,
            OpIn::ApproxBlock => Op::ApproxBlock,
            OpIn::Test { .. } => Op::Test,
            OpIn::SubdivBlock { .. } => Op::SubdivBlock,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surf: SurfIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -----------------------------------------------------------------------

fn push_surf(o: &mut Vec<f64>, s: &SurfIn) -> Result<(), HarnessError> {
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
        SurfIn::RawBspline(b) => {
            o.push(3.0);
            push_bss(o, b)
        }
        SurfIn::RawCurveBounded(b) => {
            o.push(4.0);
            push_bss(o, b)
        }
    }
}

fn push_opts(o: &mut Vec<f64>, p: &OptsIn) {
    o.extend([
        p.chord, p.ang, p.aspect, p.max_side, p.min_side, p.ratio, p.start,
    ]);
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    match &i.op {
        OpIn::BezBlock => {
            let SurfIn::Bspline(b) = &i.surf else {
                return Err(harness("bez_block needs a B-spline"));
            };
            push_bss(&mut o, b)?;
        }
        op => {
            push_surf(&mut o, &i.surf)?;
            match op {
                OpIn::BezBlock | OpIn::ApproxBlock => {}
                OpIn::Nth { dom, idx } => {
                    o.extend(dom);
                    o.push(idx.len() as f64);
                    o.extend(idx.iter().map(|&k| f64::from(k)));
                }
                OpIn::Add { adds, queries } => {
                    o.push(adds.len() as f64);
                    for a in adds {
                        o.extend([f64::from(a.ty), a.approx, a.angle]);
                        push_key(&mut o, &a.key);
                        push_curves(&mut o, a.c3.as_ref());
                        push_curves(&mut o, a.cuv.as_ref());
                    }
                    o.push(queries.len() as f64);
                    for q in queries {
                        o.extend([f64::from(q.ty), q.approx, q.angle]);
                        push_key(&mut o, &q.key);
                    }
                }
                OpIn::Test {
                    cache_dom,
                    opts,
                    aux,
                    children,
                    chsat,
                    angsat,
                    dom,
                    bez,
                    ivl,
                } => {
                    o.extend(cache_dom);
                    push_opts(&mut o, opts);
                    o.extend([
                        f64::from(*aux),
                        flag(*children),
                        flag(*chsat),
                        flag(*angsat),
                    ]);
                    o.extend(dom);
                    if *aux == 3 {
                        let b = bez.as_ref().ok_or_else(|| harness("aux 3 needs a Bezier"))?;
                        push_srf(&mut o, &srf(b)?)?;
                        for [lo, hi] in ivl {
                            o.extend([*lo, *hi]);
                        }
                    }
                }
                OpIn::SubdivBlock { opts, dom, nosurf } => {
                    push_opts(&mut o, opts);
                    o.extend(dom);
                    o.push(flag(*nosurf));
                }
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

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn surface_in(s: &SurfIn) -> Result<Surface, HarnessError> {
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
        SurfIn::RawBspline(b) => Surface::from(raw_bss_of(b)?),
        SurfIn::RawCurveBounded(b) => Surface::from(CurveBoundedSurface::new(
            Surface::from(raw_bss_of(b)?),
            Vec::new(),
            Vec::new(),
            true,
        ).map_err(|e| HarnessError::new(e.to_string()))?),
    })
}

/// [`bss_of`] over a borrowed NURB: the knots stay exactly as given.
fn raw_bss_of(b: &BssIn) -> Result<BSplineSurface, HarnessError> {
    let mut s = BSplineSurface::from_nurb(surface_of(&srf(&b.srf)?)?, true);
    *s.out_of_bounds_enabled_mut() = b.oob;
    let form = BSplineSurfaceForm::from_code(u64::from(b.form))
        .ok_or_else(|| harness(format!("bad form {}", b.form)))?;
    s.set_b_spline_surface_form(form);
    Ok(s)
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

fn cache_of(surface: Surface, dom: Extent2d, opts: &OptsIn) -> SurfaceCache<i64> {
    let mut c = SurfaceCache::new(surface, dom, options_of(opts));
    c.start_fast_subdivision_angle_deg = opts.start;
    c.set_surface_handle(Some(SURFACE_HANDLE));
    c
}

fn zero_box() -> Extent3d {
    Extent3d::from_bounds(0.0, 0.0, 0.0, 0.0, 0.0, 0.0)
}

/// A node with all-zero box (block memory) and surface data in `slot`.
fn node(aux_type: AuxDataType, surface: Option<i64>, slot: usize) -> TreeNode<i64> {
    TreeNode {
        bbox: zero_box(),
        aux_type,
        data: NodeData::Surface(SurfaceAux {
            split_dir: SplitDir::U,
            is_marked: false,
            node_class: NodeClass::Unknown,
            face: None,
            surface,
            slot: Some(slot),
        }),
        geom_type: NodeGeomType::Default,
        ..TreeNode::new()
    }
}

fn aux_data(dom: [f64; 4], owner: usize) -> BezierAux2d<i64> {
    BezierAux2d {
        uv_domain: dom_of(dom),
        owning_tree_node: Some(NodeId(owner)),
        ..BezierAux2d::default()
    }
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

/// Every node of the cache tree: `count` then `parent c1 c2 aux split D box`.
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
    }
    Ok(())
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

/// The outcome of a Rust op: the record, or an operation error (its status ends the
/// record unless the op already wrote it).
type Run = Result<Vec<f64>, SolidError>;

#[allow(clippy::too_many_lines)]
fn rust_op(i: &Inputs) -> Result<Run, HarnessError> {
    let k = Kernel::default();
    let surface = surface_in(&i.surf)?;
    match &i.op {
        OpIn::Nth { dom, idx } => {
            let mut bp = PatchBoundaryBoundingPlanes::new(dom_of(*dom));
            let mut o = vec![0.0];
            for &ix in idx {
                match bp.nth_bounding_plane(&k, &surface, usize::from(ix)) {
                    Ok(r) => {
                        o.extend([0.0, flag(r.plane_exists), flag(r.boundary_degenerate)]);
                        if let Some(p) = r.plane {
                            push3(&mut o, p.base_point);
                            push3(&mut o, p.normal);
                            for s in p.sample_points {
                                push3(&mut o, s);
                            }
                            o.push(p.tolerance);
                        }
                    }
                    Err(e) if e.is_unported() => return Ok(Err(e)),
                    Err(e) => {
                        o.push(status(&e));
                        o.extend([0.0, 0.0]);
                    }
                }
            }
            Ok(Ok(o))
        }
        OpIn::Add { adds, queries } => {
            let d = surface.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
            let mut cache = SurfaceCache::<i64>::new(surface, d, SurfaceCacheOptions::default());
            let mut o = vec![0.0];
            for a in adds {
                let r = cache.add_curves_to_cache(
                    alt_type(a.ty),
                    a.approx,
                    a.angle,
                    &key_of(&a.key),
                    curves_of(a.c3.as_ref())?,
                    curves_of(a.cuv.as_ref())?,
                );
                o.push(match r {
                    Ok(()) => 0.0,
                    Err(e) => status(&e),
                });
            }
            let caches = cache.alt_caches();
            o.push(caches.len() as f64);
            for c in caches {
                let (copies, st) = c.copy_of_curves_partial(true, true);
                if let Err(e) = &st {
                    if e.is_unported() {
                        return Ok(Err(e.clone()));
                    }
                }
                o.push(match &st {
                    Ok(()) => 0.0,
                    Err(e) => status(e),
                });
                o.push(copies.curves_3d.as_ref().map_or(0, Vec::len) as f64);
                o.push(copies.curves_uv.as_ref().map_or(0, Vec::len) as f64);
            }
            for q in queries {
                for c in caches {
                    o.push(flag(c.match_cache(
                        alt_type(q.ty),
                        q.approx,
                        q.angle,
                        &key_of(&q.key),
                    )));
                }
            }
            Ok(Ok(o))
        }
        OpIn::BezBlock => {
            let solidcore::surface::surface::SurfaceKind::BSpline(b) = surface.kind() else {
                return Err(harness("bez_block needs a B-spline"));
            };
            let nurb = b.gw_nurb().ok_or_else(|| harness("no NURB"))?;
            match sc::create_bezier_patch_block(nurb) {
                Ok(r) => {
                    let mut o = vec![
                        0.0,
                        r.num_u_spans as f64,
                        r.num_v_spans as f64,
                        r.beziers.len() as f64,
                    ];
                    for z in &r.beziers {
                        push_nurb(&mut o, z)?;
                    }
                    Ok(Ok(o))
                }
                Err(e) => Ok(Err(e)),
            }
        }
        OpIn::ApproxBlock => match sc::create_approximate_bezier_patch_block(&k, &surface) {
            Ok(r) => {
                let mut o = vec![0.0, r.num_u_spans as f64, r.num_v_spans as f64];
                o.push(r.u_splits.len() as f64);
                o.extend_from_slice(&r.u_splits);
                o.push(r.v_splits.len() as f64);
                o.extend_from_slice(&r.v_splits);
                o.push(r.beziers.len() as f64);
                for z in &r.beziers {
                    push_nurb(&mut o, z)?;
                }
                Ok(Ok(o))
            }
            Err(e) => Ok(Err(e)),
        },
        OpIn::Test {
            cache_dom,
            opts,
            aux,
            children,
            chsat,
            angsat,
            dom,
            bez,
            ivl,
        } => {
            let mut cache = cache_of(surface, dom_of(*cache_dom), opts);
            let aux_type = AuxDataType::from_code(i32::from(*aux))
                .ok_or_else(|| harness(format!("bad aux type {aux}")))?;
            let mut tree: Tree<i64> = Tree::new(Some(TreeOwner::SurfaceCache));
            let mut patches = Vec::new();
            let mut auxs = Vec::new();
            let mut data = aux_data(*dom, 0);
            data.chord_height_satisfied = *chsat;
            data.angle_tol_satisfied = *angsat;
            let slot = if aux_type == AuxDataType::BezierSurface {
                let b = bez.as_ref().ok_or_else(|| harness("aux 3 needs a Bezier"))?;
                let mut pb = PseudoBox::new();
                pb.set_intervals(
                    Extent1d::from_raw(ivl[0][0], ivl[0][1]),
                    Extent1d::from_raw(ivl[1][0], ivl[1][1]),
                    Extent1d::from_raw(ivl[2][0], ivl[2][1]),
                );
                patches.push(BezierPatch {
                    aux: data,
                    bezier: Some(surface_of(&srf(b)?)?),
                    pseudo_box: pb,
                    ..BezierPatch::default()
                });
                0
            } else {
                auxs.push(data);
                auxs.len() - 1
            };
            tree.add_node(node(aux_type, Some(SURFACE_HANDLE), slot));
            if *children {
                for c in [1usize, 2] {
                    auxs.push(aux_data(*dom, c));
                    let mut n = node(AuxDataType::AuxData, Some(SURFACE_HANDLE), auxs.len() - 1);
                    n.parent = Some(NodeId(0));
                    tree.add_node(n);
                }
                let top = tree
                    .node_mut(NodeId(0))
                    .map_err(|e| harness(e.to_string()))?;
                top.child1 = Some(NodeId(1));
                top.child2 = Some(NodeId(2));
            }
            tree.set_top_node(Some(NodeId(0)));
            *cache.patches_mut() = patches;
            *cache.auxs_mut() = auxs;
            cache.set_tree(Some(tree));
            let r = cache.test_against_tolerances(&k, NodeId(0));
            let mut o = Vec::new();
            match &r {
                Ok(t) => o.extend([
                    0.0,
                    flag(t.needs_subdivision),
                    f64::from(t.direction as u8),
                ]),
                Err(e) if e.is_unported() => return Ok(Err(e.clone())),
                Err(e) => o.push(status(e)),
            }
            let a = cache
                .aux(NodeId(0))
                .map_err(|e| harness(format!("aux: {e}")))?;
            o.push(flag(a.chord_height_satisfied));
            o.push(flag(a.angle_tol_satisfied));
            o.push(flag(cache.switch_to_fast_subdivision));
            if r.is_ok() {
                let tree = cache.tree().ok_or_else(|| harness("no tree"))?;
                let n = tree.node(NodeId(0)).map_err(|e| harness(e.to_string()))?;
                push_box(&mut o, &n.bbox);
            }
            Ok(Ok(o))
        }
        OpIn::SubdivBlock { opts, dom, nosurf } => {
            let d = surface.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
            let mut cache = cache_of(surface, d, opts);
            let mut tree: Tree<i64> = Tree::new(Some(TreeOwner::SurfaceCache));
            let handle = (!nosurf).then_some(SURFACE_HANDLE);
            tree.add_node(node(AuxDataType::AuxData, handle, 0));
            tree.set_top_node(Some(NodeId(0)));
            *cache.auxs_mut() = vec![aux_data(*dom, 0)];
            cache.set_tree(Some(tree));
            let r = cache.subdivide_by_block(&k, NodeId(0));
            let mut o = Vec::new();
            match r {
                Ok(()) => o.push(0.0),
                Err(e) if e.is_unported() => return Ok(Err(e)),
                Err(e) => o.push(status(&e)),
            }
            dump_tree(&mut o, &cache)?;
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

fn gen_bss_inline(rng: &mut Rng) -> BssIn {
    let s = gen_inline(rng);
    gen_bss(rng, s)
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

fn gen_surf(rng: &mut Rng) -> SurfIn {
    match rng.below(10) {
        0 | 1 => SurfIn::Plane(gen_plane(rng)),
        2 => SurfIn::CurveBounded(gen_bss_inline(rng)),
        _ => SurfIn::Bspline(gen_bss_inline(rng)),
    }
}

fn surf_domain(s: &SurfIn) -> Result<[f64; 4], HarnessError> {
    Ok(match s {
        SurfIn::Bspline(b)
        | SurfIn::CurveBounded(b)
        | SurfIn::RawBspline(b)
        | SurfIn::RawCurveBounded(b) => domain_of_srf(&srf(&b.srf)?),
        SurfIn::Plane(p) => p.dom,
    })
}

/// A tolerance: zero, tiny, moderate or large relative to `size`.
fn gen_tol(rng: &mut Rng, size: f64) -> f64 {
    match rng.below(6) {
        0 => 0.0,
        1 => size * 1e-6,
        2 => size * rng.uniform(1e-3, 1e-2),
        3 => size * rng.uniform(0.01, 0.2),
        4 => size * rng.uniform(0.2, 2.0),
        _ => size * rng.uniform(1e-4, 0.5),
    }
}

/// The largest coordinate magnitude of a surface (for tolerance scaling).
fn surf_size(s: &SurfIn) -> Result<f64, HarnessError> {
    Ok(match s {
        SurfIn::Bspline(b)
        | SurfIn::CurveBounded(b)
        | SurfIn::RawBspline(b)
        | SurfIn::RawCurveBounded(b) => srf(&b.srf)?
            .net
            .iter()
            .flatten()
            .map(|p| {
                let w = if p[3] == NOW { 1.0 } else { p[3] };
                (p[0] / w).abs().max((p[1] / w).abs()).max((p[2] / w).abs())
            })
            .fold(1e-12, f64::max),
        SurfIn::Plane(p) => (p.dom[2] - p.dom[0]).max(p.dom[3] - p.dom[1]) * 3.0,
    })
}

fn gen_opts(rng: &mut Rng, size: f64) -> OptsIn {
    OptsIn {
        chord: gen_tol(rng, size),
        ang: match rng.below(5) {
            0 => 0.0,
            1 => 1e-14,
            2 => rng.uniform(0.01, 0.2),
            3 => rng.uniform(0.2, 1.0),
            _ => rng.uniform(1.0, 3.0),
        },
        aspect: if rng.chance(0.4) {
            rng.uniform(0.5, 5.0)
        } else {
            0.0
        },
        max_side: if rng.chance(0.4) {
            gen_tol(rng, size)
        } else {
            0.0
        },
        min_side: if rng.chance(0.4) {
            gen_tol(rng, size)
        } else {
            0.0
        },
        ratio: match rng.below(4) {
            0 => 0.0,
            1 => 0.5,
            2 => rng.uniform(1e-4, 0.2),
            _ => 0.001,
        },
        start: match rng.below(3) {
            0 => 30.0,
            1 => rng.uniform(0.0, 90.0),
            _ => 600.0,
        },
    }
}

/// A Bezier patch for a test node: a decomposition patch shape over `dom`, a random
/// bicubic or a flat bilinear one, scaled like the surface.
fn gen_bez(rng: &mut Rng, size: f64, dom: [f64; 4]) -> InlineSrf {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let flat = rng.chance(0.3);
    let amp = if flat { 0.0 } else { rng.uniform(0.0, 0.5) };
    let net = (0..=p)
        .map(|i| {
            (0..=q)
                .map(|j| {
                    [
                        size * (i as f64 / p as f64),
                        size * (j as f64 / q as f64),
                        size * amp * rng.uniform(-1.0, 1.0),
                        NOW,
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

fn gen_test(rng: &mut Rng, surf: SurfIn) -> Result<Inputs, HarnessError> {
    let sd = surf_domain(&surf)?;
    let size = surf_size(&surf)?;
    let dom = if rng.chance(0.15) {
        // a sub-domain much smaller than the cache domain (minimum ratio tests)
        let w = (sd[2] - sd[0]) * rng.uniform(1e-4, 1e-2);
        let h = (sd[3] - sd[1]) * rng.uniform(1e-4, 1e-2);
        let a = sd[0] + (sd[2] - sd[0] - w) * rng.unit();
        let b = sd[1] + (sd[3] - sd[1] - h) * rng.unit();
        if rng.chance(0.5) {
            [a, sd[1], a + w, sd[3]]
        } else {
            [sd[0], b, sd[2], b + h]
        }
    } else {
        gen_sub_dom(rng, sd)
    };
    let aux = match rng.below(20) {
        0 => 4,
        1..=8 if matches!(surf, SurfIn::Bspline(_)) => 6,
        _ => 3,
    };
    let opts = gen_opts(rng, size);
    let h = match rng.below(4) {
        0 => opts.chord,
        1 => opts.chord * rng.uniform(0.5, 1.0),
        2 => opts.chord * rng.uniform(1.0, 3.0),
        _ => size * rng.uniform(0.0, 0.3),
    };
    // SM_AD_AUX_DATA nodes box their surface unless both tolerances are satisfied already
    let both = aux == 6 && rng.chance(0.5);
    Ok(Inputs {
        surf,
        op: OpIn::Test {
            cache_dom: if rng.chance(0.8) {
                sd
            } else {
                gen_sub_dom(rng, sd)
            },
            aux,
            children: rng.chance(0.08),
            chsat: both || rng.chance(0.2),
            angsat: both || rng.chance(0.2),
            dom,
            bez: (aux == 3).then(|| SrfIn::Inline(gen_bez(rng, size, dom))),
            ivl: [[0.0, size], [0.0, size], [0.0, h]],
            opts,
        },
    })
}

fn gen_add(rng: &mut Rng, surf: SurfIn) -> Inputs {
    let na = 1 + rng.below(8) as usize;
    let curves = |rng: &mut Rng| -> Option<Vec<CrvIn>> {
        if rng.chance(0.2) {
            None
        } else {
            Some((0..rng.below(3)).map(|_| gen_curve(rng)).collect())
        }
    };
    let adds: Vec<AddIn> = (0..na)
        .map(|_| AddIn {
            ty: rng.below(2) as u8,
            approx: rng.uniform(0.0, 0.1),
            angle: rng.uniform(0.0, 0.5),
            key: gen_key(rng),
            c3: curves(rng),
            cuv: curves(rng),
        })
        .collect();
    let nq = rng.below(5) as usize;
    let queries = (0..nq)
        // `adds` is never empty (na >= 1), so `get` always hits.
        .filter_map(|_| {
            let a = adds.get(rng.below(adds.len() as u64) as usize)?;
            Some(QueryIn {
                ty: if rng.chance(0.9) {
                    a.ty
                } else {
                    1 - a.ty.min(1)
                },
                approx: if rng.chance(0.8) {
                    a.approx
                } else {
                    a.approx + rng.uniform(-2e-12, 2e-12)
                },
                angle: a.angle,
                key: perturb_key(rng, &a.key),
            })
        })
        .collect();
    Inputs {
        surf,
        op: OpIn::Add { adds, queries },
    }
}

fn gen_case(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surf(rng);
    Ok(match rng.below(20) {
        0..=2 => {
            let sd = surf_domain(&surf)?;
            let idx = (0..=rng.below(6)).map(|_| rng.below(4) as u8).collect();
            Inputs {
                op: OpIn::Nth {
                    dom: gen_sub_dom(rng, sd),
                    idx,
                },
                surf,
            }
        }
        3 | 4 => gen_add(rng, surf),
        5..=7 => Inputs {
            surf: SurfIn::Bspline(gen_bss_inline(rng)),
            op: OpIn::BezBlock,
        },
        8 => Inputs {
            surf,
            op: OpIn::ApproxBlock,
        },
        9 => {
            let sd = surf_domain(&surf)?;
            let size = surf_size(&surf)?;
            let mut opts = gen_opts(rng, size);
            opts.chord = opts.chord.max(size * 1e-2);
            opts.ang = opts.ang.max(0.1);
            if opts.max_side > 0.0 {
                opts.max_side = opts.max_side.max(0.05).max(size * 0.05);
            }
            let nosurf = rng.chance(0.4) || matches!(surf, SurfIn::CurveBounded(_));
            Inputs {
                surf,
                op: OpIn::SubdivBlock {
                    opts,
                    dom: gen_sub_dom(rng, sd),
                    nosurf,
                },
            }
        }
        _ => gen_test(rng, surf)?,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_cache_p2"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The generators reach every op with compared (non-deferred) outcomes on the Rust
    /// side, except the approximate block of B-splines and the block subdivision of a node
    /// with a surface (deferred until `IsSingularity` / `FindTessellationSplits` are
    /// ported).
    #[test]
    fn generated_cases_cover_ops() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..3000u64 {
            let mut rng = Rng::new(0x5eed_0000 + seed);
            let Ok(i) = gen_case(&mut rng) else {
                continue;
            };
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
        for op in ["Nth", "Add", "BezBlock", "ApproxBlock", "Test"] {
            assert!(
                tally.keys().any(|(n, k)| n == op && k == "status:0"),
                "{op} never succeeds"
            );
        }
    }
}
