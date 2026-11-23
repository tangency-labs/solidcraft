//! Runner for unit `intersect.surface_intersector_p5`: `SmSurfaceIntersector::DoIntersection`
//! (with `GrazingBoxProcess`).
//!
//! Ported from upstream `source/SMLib/src/SmSurfaceIntersector.cpp` (an
//! `SmAdvSurfaceIntersector`, as `SmSurface::GlobalSurfaceIntersect` builds it, on
//! `SmPlane` / `SmBSplineSurface` / `SmCurveBoundedSurface` objects); checked against its
//! golden. Rust: `solidcore::intersect::surface_intersector::p5`.
//!
//! A case is two surfaces (B may be A itself), their domains, the edge flags, the optional
//! tolerances and optionally a hand-built decomposition tree per surface cache. With trees,
//! the Rust side runs `do_intersection_with_caches` on caches holding them (upstream's
//! nested `SmSurface::GlobalCurveIntersect` fetches the same cache; the port hands it the
//! intersector's cache, so the boundary curves are solved on the hand tree); without, it
//! builds the caches (`do_intersection`). The record is `status` and, on success, the curves
//! (values `rec`; statuses `0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust
//! `InvalidInput`, `-1` for a Rust error upstream cannot return).
//!
//! Generated scenes, at scales `1e-3` to `1e4`, with and without edges and tolerances:
//! planes apart, corner to corner (touching or not, the grazing box), sharing an edge
//! (coplanar or folded: the boundary plane graze), parallel, nearly parallel and crossing,
//! a B-spline against a plane or a B-spline, a surface against itself, and curve-bounded
//! surfaces over planes; trees of depth 0 to 3 sampled from the surfaces.
//!
//! Deferred cases: plane boundary iso curves (`SmPlane::CreateIsoParametricCurve` and
//! `SmBezierPatch::GetPolarBox`, `surface.surface_cache_p1`); traced points that need
//! `SmSurfaceIntersector::ComputePointValues` (`intersect.surface_intersector_p2`); curves
//! coincident with the other surface (`SmSurfaceDropCurve::DoTrace`,
//! `intersect.surface_drop_curve`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::polar_box::PolarBox;
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::geom::solution_array::{ObjectKind, SolutionObject};
use solidcore::geom::tree::{
    AuxDataType, NodeData, NodeGeomType, NodeId, SurfaceAux, Tree, TreeNode, TreeOwner,
};
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::adv_surface_intersector::AdvSurfaceIntersector;
use solidcore::intersect::surface_intersector::p5::do_intersection;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::SurfaceIntersection;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::p1::{
    BezierAux2d, BezierPatch, SurfaceCache, SurfaceCacheOptions,
};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, CaseSource, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `intersect.surface_intersector_p5` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The surface handle type of the caches (`SmSurface*`).
#[derive(Debug, Clone, Copy, PartialEq, Default)]
struct H(i64);

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
}

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum SurfaceIn {
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        domain: [f64; 4],
    },
    BSpline {
        p: usize,
        q: usize,
        /// Rows (u index) of `[x, y, z, w]` (homogeneous); `w` = `null` when non-rational.
        net: Vec<Vec<[Option<f64>; 4]>>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    /// An `SmCurveBoundedSurface` (implicit outer, no boundary curves) over a plane or
    /// B-spline basis.
    CurveBounded { basis: Box<SurfaceIn> },
}

/// Basis vectors and `[min, max]` intervals of a pseudo box.
type PseudoIn = ([[f64; 3]; 3], [[f64; 2]; 3]);

#[derive(Debug, Clone, Serialize, Deserialize)]
struct NodeIn {
    parent: i64,
    c1: i64,
    c2: i64,
    aux: u8,
    bbox: [f64; 6],
    dom: [f64; 4],
    polar: Vec<[f64; 3]>,
    /// Basis vectors and `[min, max]` intervals (patches only).
    pseudo: Option<PseudoIn>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    a: SurfaceIn,
    /// `None`: B is A itself.
    b: Option<SurfaceIn>,
    dom1: [f64; 4],
    dom2: [f64; 4],
    edges: [bool; 2],
    tol: Option<f64>,
    ang: Option<f64>,
    /// Tree of A and (unless B is A) of B; `None`: the caches build their own.
    trees: Option<(Vec<NodeIn>, Option<Vec<NodeIn>>)>,
    /// Per surface (A, B): keep the B-spline knots exactly as given; Rust builds that
    /// surface over a borrowed NURB, whose knots `SmBSplineSurface(gw_SURFACE*, TRUE, ..)`
    /// leaves as they are.
    #[serde(default, skip_serializing_if = "no_raw_knots")]
    raw_knots: [bool; 2],
}

#[expect(
    clippy::trivially_copy_pass_by_ref,
    reason = "serde's skip_serializing_if passes the field by reference"
)]
fn no_raw_knots(r: &[bool; 2]) -> bool {
    !r[0] && !r[1]
}

fn cpt4(c: &[Option<f64>; 4]) -> [f64; 4] {
    [
        c[0].unwrap_or(f64::NAN),
        c[1].unwrap_or(f64::NAN),
        c[2].unwrap_or(f64::NAN),
        c[3].unwrap_or(NOW),
    ]
}

// ---- Rust side ----------------------------------------------------------------------------

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
        _ => -1.0,
    }
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn ext(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn rust_surface(s: &SurfaceIn) -> Result<Surface, HarnessError> {
    rust_surface_with(s, false)
}

fn rust_surface_with(s: &SurfaceIn, raw_knots: bool) -> Result<Surface, HarnessError> {
    match s {
        SurfaceIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Ok(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &ext(*domain),
        ))),
        SurfaceIn::BSpline { p, q, net, u, v } => {
            let net = net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|c| {
                            let [x, y, z, w] = cpt4(c);
                            CPoint::new(x, y, z, w)
                        })
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Ok(Surface::from(BSplineSurface::from_nurb(sur, raw_knots)))
        }
        SurfaceIn::CurveBounded { basis } => Ok(Surface::from(CurveBoundedSurface::new(
            rust_surface_with(basis, raw_knots)?,
            Vec::new(),
            Vec::new(),
            true,
        ).map_err(|e| HarnessError::new(e.to_string()))?)),
    }
}

fn link(i: i64) -> Option<NodeId> {
    usize::try_from(i).ok().map(NodeId)
}

/// The cache of `surface` over its natural domain with the hand-built tree `t`.
fn rust_cache(surface: Surface, t: &[NodeIn], handle: H) -> Result<SurfaceCache<H>, HarnessError> {
    // (runner helper without a kernel in scope)
    let d = surface.natural_uv_domain_if_built();
    let mut cache = SurfaceCache::new(surface, d, SurfaceCacheOptions::default());
    cache.set_surface_handle(Some(handle));
    let mut tree: Tree<H> = Tree::new(Some(TreeOwner::SurfaceCache));
    let mut patches = Vec::new();
    let mut auxs = Vec::new();
    for (k, n) in t.iter().enumerate() {
        let aux_type = AuxDataType::from_code(i32::from(n.aux))
            .ok_or_else(|| harness(format!("bad aux type {}", n.aux)))?;
        let mut polar_box = PolarBox::new();
        for v in &n.polar {
            polar_box.add_vector(v3(*v), None);
        }
        let data = BezierAux2d {
            uv_domain: ext(n.dom),
            polar_box,
            owning_tree_node: Some(NodeId(k)),
            ..BezierAux2d::default()
        };
        let slot = if aux_type == AuxDataType::BezierSurface {
            let (basis, ivls) = n.pseudo.ok_or_else(|| harness("patch without pseudo box"))?;
            let mut pseudo_box = PseudoBox::new();
            pseudo_box
                .set_basis(v3(basis[0]), v3(basis[1]), v3(basis[2]))
                .map_err(|e| harness(format!("pseudo box: {e}")))?;
            let iv = |i: [f64; 2]| Extent1d::from_min_max(i[0], i[1]);
            pseudo_box.set_intervals(iv(ivls[0]), iv(ivls[1]), iv(ivls[2]));
            patches.push(BezierPatch {
                aux: data,
                pseudo_box,
                ..BezierPatch::default()
            });
            patches.len() - 1
        } else {
            auxs.push(data);
            auxs.len() - 1
        };
        let [x0, y0, z0, x1, y1, z1] = n.bbox;
        tree.add_node(TreeNode {
            parent: link(n.parent),
            child1: link(n.c1),
            child2: link(n.c2),
            bbox: Extent3d::from_points(Vec3::new(x0, y0, z0), Vec3::new(x1, y1, z1)),
            aux_type,
            data: NodeData::Surface(SurfaceAux {
                surface: Some(handle),
                slot: Some(slot),
                ..SurfaceAux::default()
            }),
            geom_type: NodeGeomType::Default,
        });
    }
    tree.set_top_node(Some(NodeId(0)));
    *cache.patches_mut() = patches;
    *cache.auxs_mut() = auxs;
    cache.set_tree(Some(tree));
    Ok(cache)
}

/// `type dim knot_type form has [n p m cpts(x y z w) knots] ivl[2]`.
fn put_bspline(o: &mut Vec<f64>, c: &Curve) -> Result<(), HarnessError> {
    o.extend([c.type_code() as f64, c.dim() as f64]);
    // the shim casts to `SmBSplineCurve`: the B-spline part of the line of a plane's iso
    // curve or of the circle of a sweep's
    let bs = match c.kind() {
        CurveKind::BSpline(bs) => bs,
        CurveKind::Line(l) => l.base(),
        CurveKind::Ellipse(e) => e.base(),
        _ => return Err(harness("output curve is not a B-spline kind")),
    };
    o.extend([
        f64::from(bs.knot_type() as i32),
        f64::from(bs.form() as i32),
    ]);
    match bs.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
        }
    }
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    Ok(())
}

fn put_opt(o: &mut Vec<f64>, c: Option<&Curve>) -> Result<(), HarnessError> {
    match c {
        None => {
            o.push(0.0);
            Ok(())
        }
        Some(c) => {
            o.push(1.0);
            put_bspline(o, c)
        }
    }
}

/// `closest uv1[2] uv2[2] separate trace_dir ang_tol approx_tol tol3d op closed ncurve
/// nstart nthrough`.
fn put_state(o: &mut Vec<f64>, si: &AdvSurfaceIntersector<'_, '_, H>) {
    let uvs = si.closest_uvs();
    o.extend([
        si.closest_value(),
        uvs[0].x,
        uvs[0].y,
        uvs[1].x,
        uvs[1].y,
        if si.separate_gauss_maps() { 1.0 } else { 0.0 },
        si.base.curve_trace_direction,
        si.base.this_ang_tol_rad,
        si.base.this_approx_tol_3d,
        si.solver.tolerance_3d,
        f64::from(si.solver.operation as i32),
        if si.base.curve_is_closed { 1.0 } else { 0.0 },
        si.base.curve_points().len() as f64,
        si.base.start_points().len() as f64,
        si.base.through_points().len() as f64,
    ]);
}

/// The record of `r`, with the intersector state `state` after the status.
fn record(
    r: solidcore::Result<SurfaceIntersection>,
    state: &[f64],
) -> Result<Output, HarnessError> {
    let x = match r {
        Ok(x) => x,
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return Ok(d);
            }
            let mut o = vec![status(&e)];
            o.extend_from_slice(state);
            return Ok(Output::new().values("rec", o));
        }
    };
    let n = x.curves_3d.len();
    if x.surface1_uv_curves.len() != n
        || x.surface2_uv_curves.len() != n
        || x.curve_types.len() != n
        || x.deviations.len() != n
    {
        return Err(harness("output arrays of different lengths"));
    }
    let mut o = vec![0.0];
    o.extend_from_slice(state);
    o.push(n as f64);
    for k in 0..n {
        let (Some(c3), Some(u1), Some(u2), Some(t), Some(d)) = (
            x.curves_3d.get(k),
            x.surface1_uv_curves.get(k),
            x.surface2_uv_curves.get(k),
            x.curve_types.get(k),
            x.deviations.get(k),
        ) else {
            return Err(harness("output index"));
        };
        put_bspline(&mut o, c3)?;
        put_opt(&mut o, u1.as_ref())?;
        put_opt(&mut o, u2.as_ref())?;
        o.push(f64::from(*t as i32));
        o.push(*d);
    }
    Ok(Output::new().values("rec", o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let a = rust_surface_with(&i.a, i.raw_knots[0])?;
    let b_owned = match &i.b {
        Some(b) => Some(rust_surface_with(b, i.raw_knots[1])?),
        None => None,
    };
    let b = b_owned.as_ref().unwrap_or(&a);
    let (dom1, dom2) = (ext(i.dom1), ext(i.dom2));
    let k = Kernel::default();
    let Some((ta, tb)) = &i.trees else {
        return record(
            do_intersection(&k, &a, &dom1, b, &dom2, i.edges, i.tol, i.ang),
            &[],
        );
    };
    let mut caches = vec![rust_cache(a.clone(), ta, H(0))?];
    match (&i.b, tb) {
        (None, None) => {}
        (Some(_), Some(tb)) => caches.push(rust_cache(b.clone(), tb, H(1))?),
        _ => return Err(harness("one tree per distinct surface")),
    }
    let mut ssi: AdvSurfaceIntersector<'_, '_, H> =
        match AdvSurfaceIntersector::new(&k, &a, &dom1, b, &dom2, false) {
            Ok(s) => s,
            Err(e) => return record(Err(e), &[]),
        };
    ssi.base.set_use_surface_edges(i.edges[0], i.edges[1]);
    let r = ssi.do_intersection_with_caches(&k, &mut caches, i.tol, i.ang);
    let mut state = Vec::new();
    put_state(&mut state, &ssi);
    record(r, &state)
}

// ---- generation -----------------------------------------------------------------------------

type V = [f64; 3];

fn add(a: V, c: V) -> V {
    [a[0] + c[0], a[1] + c[1], a[2] + c[2]]
}

fn mul(s: f64, a: V) -> V {
    [s * a[0], s * a[1], s * a[2]]
}

fn dot(a: V, c: V) -> f64 {
    a[0] * c[0] + a[1] * c[1] + a[2] * c[2]
}

fn cross(a: V, c: V) -> V {
    [
        a[1] * c[2] - a[2] * c[1],
        a[2] * c[0] - a[0] * c[2],
        a[0] * c[1] - a[1] * c[0],
    ]
}

fn norm(a: V) -> V {
    let l = dot(a, a).sqrt();
    if l > 0.0 {
        mul(1.0 / l, a)
    } else {
        [0.0, 0.0, 1.0]
    }
}

fn unit_vec(rng: &mut Rng) -> V {
    loop {
        let v = rng.point3(-1.0, 1.0);
        let l = dot(v, v).sqrt();
        if l > 0.2 {
            return mul(1.0 / l, v);
        }
    }
}

/// A random orthonormal frame `[x, y, x × y]`.
fn frame(rng: &mut Rng) -> [V; 3] {
    let x = unit_vec(rng);
    let y = norm(cross(cross(x, unit_vec(rng)), x));
    let y = if dot(y, y) > 0.5 {
        y
    } else {
        norm(cross(x, [0.3, 0.1, 1.0]))
    };
    [x, y, cross(x, y)]
}

/// A plane through `o` with axes `x`, `y` over `[0, s]^2`.
fn plane_at(o: V, x: V, y: V, s: f64) -> SurfaceIn {
    SurfaceIn::Plane {
        o,
        x,
        y,
        scale: [1.0, 1.0],
        domain: [0.0, 0.0, s, s],
    }
}

/// The bilinear B-spline over `[0, s]^2` of a plane (its iso curves are B-splines; the
/// surface cache does not make a plane's iso curves yet).
fn bilinear(p: &SurfaceIn) -> SurfaceIn {
    let SurfaceIn::Plane {
        o, x, y, domain, ..
    } = p
    else {
        return p.clone();
    };
    let [u0, v0, u1, v1] = *domain;
    let at = |u: f64, v: f64| {
        let q = add(*o, add(mul(u, *x), mul(v, *y)));
        [Some(q[0]), Some(q[1]), Some(q[2]), None]
    };
    SurfaceIn::BSpline {
        p: 1,
        q: 1,
        net: vec![vec![at(u0, v0), at(u0, v1)], vec![at(u1, v0), at(u1, v1)]],
        u: vec![u0, u0, u1, u1],
        v: vec![v0, v0, v1, v1],
    }
}

fn gen_bspline(rng: &mut Rng, s: f64, origin: V, axes: [V; 3], bump: f64) -> SurfaceIn {
    let p = 1 + rng.below(2) as usize;
    let q = 1 + rng.below(2) as usize;
    let nu = p + 1 + rng.below(2) as usize;
    let nv = q + 1 + rng.below(2) as usize;
    let rational = rng.chance(0.15);
    let [ax, ay, az] = axes;
    let mut net = Vec::new();
    for i in 0..nu {
        let mut row = Vec::new();
        for j in 0..nv {
            let pt = add(
                origin,
                add(
                    add(
                        mul(s * i as f64 / (nu - 1) as f64, ax),
                        mul(s * j as f64 / (nv - 1) as f64, ay),
                    ),
                    mul(s * rng.uniform(-bump, bump), az),
                ),
            );
            let w = rational.then(|| rng.uniform(0.5, 2.0));
            let ww = w.unwrap_or(1.0);
            row.push([Some(pt[0] * ww), Some(pt[1] * ww), Some(pt[2] * ww), w]);
        }
        net.push(row);
    }
    let knots = |n: usize, p: usize| {
        let mut k = vec![0.0; p + 1];
        for i in 0..(n - p - 1) {
            k.push((i as f64 + 1.0) / (n - p) as f64);
        }
        k.extend(vec![1.0; p + 1]);
        k
    };
    SurfaceIn::BSpline {
        p,
        q,
        net,
        u: knots(nu, p),
        v: knots(nv, q),
    }
}

fn natural(s: &SurfaceIn) -> [f64; 4] {
    match s {
        SurfaceIn::Plane { domain, .. } => *domain,
        SurfaceIn::BSpline { u, v, .. } => [
            u.first().copied().unwrap_or(0.0),
            v.first().copied().unwrap_or(0.0),
            u.last().copied().unwrap_or(1.0),
            v.last().copied().unwrap_or(1.0),
        ],
        SurfaceIn::CurveBounded { basis } => natural(basis),
    }
}

fn gen_domain(rng: &mut Rng, nat: [f64; 4]) -> [f64; 4] {
    let [a, c, e, d] = nat;
    let (w, h) = (e - a, d - c);
    match rng.below(8) {
        0 => [a + 0.1 * w, c + 0.2 * h, e - 0.3 * w, d - 0.1 * h],
        1 => [a + 0.5 * w, c, e, d],
        2 => [a - 0.2 * w, c - 0.1 * h, e + 0.2 * w, d],
        _ => nat,
    }
}

/// Samples `s` on a 3 x 3 grid of `d`.
fn samples(k: &Kernel, s: &Surface, d: [f64; 4]) -> Vec<V> {
    let mut v = Vec::new();
    for i in 0..3 {
        for j in 0..3 {
            let uv = Point2::new(
                d[0] + (d[2] - d[0]) * f64::from(i) / 2.0,
                d[1] + (d[3] - d[1]) * f64::from(j) / 2.0,
            );
            if let Ok(p) = s.evaluate_point(k, uv) {
                v.push([p.x, p.y, p.z]);
            }
        }
    }
    v
}

/// Approximate normals of `s` over `d` (finite differences).
fn normals(s: &Surface, d: [f64; 4], count: usize) -> Vec<V> {
    let mut out = Vec::new();
    let h = 1e-4 * ((d[2] - d[0]).abs() + (d[3] - d[1]).abs()).max(1e-9);
    for k in 0..count {
        let t = (k as f64 + 0.5) / count as f64;
        let u = d[0] + (d[2] - d[0]) * t;
        let v = d[1] + (d[3] - d[1]) * (1.0 - t);
        let ev = |u: f64, v: f64| s.evaluate_point(&Kernel::default(), Point2::new(u, v)).ok();
        if let (Some(p), Some(pu), Some(pv)) = (ev(u, v), ev(u + h, v), ev(u, v + h)) {
            let n = cross(
                [pu.x - p.x, pu.y - p.y, pu.z - p.z],
                [pv.x - p.x, pv.y - p.y, pv.z - p.z],
            );
            if dot(n, n) > 1e-30 {
                out.push(norm(n));
            }
        }
    }
    if out.is_empty() {
        out.push([0.0, 0.0, 1.0]);
    }
    out
}

fn bounds(pts: &[V], basis: &[V; 3], grow: f64) -> [[f64; 2]; 3] {
    let mut r = [[f64::MAX, -f64::MAX]; 3];
    for p in pts {
        for (iv, bv) in r.iter_mut().zip(basis) {
            let x = dot(*p, *bv);
            iv[0] = iv[0].min(x);
            iv[1] = iv[1].max(x);
        }
    }
    for iv in &mut r {
        if iv[0] > iv[1] {
            *iv = [0.0, 0.0];
        }
        iv[0] -= grow;
        iv[1] += grow;
    }
    r
}

struct TreeGen<'a> {
    s: &'a Surface,
    nodes: Vec<NodeIn>,
    frame: [V; 3],
    /// Box growth relative to the sample size.
    grow: f64,
}

impl TreeGen<'_> {
    /// Adds the node of domain `d` under `parent` and its subtree; returns its index.
    fn add(&mut self, rng: &mut Rng, parent: i64, d: [f64; 4], depth: u32, aux: u8) -> i64 {
        let k = self.nodes.len();
        let pts = samples(&Kernel::default(), self.s, d);
        let ident = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
        let size = pts.iter().map(|p| dot(*p, *p).sqrt()).fold(0.0, f64::max);
        let grow = self.grow * (1e-3 + size);
        let bb = bounds(&pts, &ident, grow);
        let basis = if rng.chance(0.5) { ident } else { self.frame };
        let pseudo = (aux == 3).then(|| (basis, bounds(&pts, &basis, grow)));
        let polar = normals(self.s, d, 1 + rng.below(3) as usize);
        self.nodes.push(NodeIn {
            parent,
            c1: -1,
            c2: -1,
            aux,
            bbox: [bb[0][0], bb[1][0], bb[2][0], bb[0][1], bb[1][1], bb[2][1]],
            dom: d,
            polar,
            pseudo,
        });
        if depth > 0 {
            let split_u = rng.chance(0.5);
            let t = rng.uniform(0.3, 0.7);
            let (da, db) = if split_u {
                let m = d[0] + (d[2] - d[0]) * t;
                ([d[0], d[1], m, d[3]], [m, d[1], d[2], d[3]])
            } else {
                let m = d[1] + (d[3] - d[1]) * t;
                ([d[0], d[1], d[2], m], [d[0], m, d[2], d[3]])
            };
            let c1 = self.add(rng, k as i64, da, depth - 1, 3);
            let c2 = self.add(rng, k as i64, db, depth - 1, 3);
            if let Some(n) = self.nodes.get_mut(k) {
                n.c1 = c1;
                n.c2 = c2;
            }
        } else if aux != 3 {
            // a leaf must be a patch
            if let Some(n) = self.nodes.get_mut(k) {
                n.aux = 3;
                n.pseudo = Some((ident, bb));
            }
        }
        k as i64
    }
}

fn gen_tree(rng: &mut Rng, s: &Surface, nat: [f64; 4], depth: u32, grow: f64) -> Vec<NodeIn> {
    let mut g = TreeGen {
        s,
        nodes: Vec::new(),
        frame: frame(rng),
        grow,
    };
    let top_aux = if depth > 0 && rng.chance(0.6) { 4 } else { 3 };
    g.add(rng, -1, nat, depth, top_aux);
    g.nodes
}

/// A scale: mostly unit-ish, sometimes tiny or huge.
fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => rng.uniform(0.5, 2.0) * 1e-3,
        1 => rng.uniform(0.5, 2.0) * 1e4,
        2 => rng.uniform(0.5, 2.0) * 1e2,
        _ => rng.uniform(0.5, 4.0),
    }
}

/// The scene kinds of the generator.
#[derive(Clone, Copy)]
enum Scene {
    Apart,
    Corner,
    SharedEdge,
    Parallel,
    NearParallel,
    Crossing,
    BSplinePlane,
    BSplines,
    SelfIntersect,
}

#[allow(clippy::too_many_lines)] // one generator
fn gen_scene(rng: &mut Rng, scene: Scene, s: f64) -> (SurfaceIn, Option<SurfaceIn>) {
    let o = rng.point3(-s, s);
    let [x, y, z] = frame(rng);
    match scene {
        Scene::Apart => {
            let d = add(o, mul(s * rng.uniform(1.5, 4.0), unit_vec(rng)));
            let [x2, y2, _] = frame(rng);
            (plane_at(o, x, y, s), Some(plane_at(d, x2, y2, s)))
        }
        Scene::Corner => {
            // B starts at A's far corner (or a little away from it) and leaves A
            let corner = add(o, add(mul(s, x), mul(s, y)));
            let gap = match rng.below(4) {
                0 => 0.0,
                1 => s * 1e-6,
                2 => s * rng.uniform(1e-4, 2e-3),
                _ => s * rng.uniform(0.01, 0.1),
            };
            let away = norm(add(add(x, y), mul(rng.uniform(-1.0, 1.0), z)));
            let start = add(corner, mul(gap, away));
            // B's axes: pointing away from A (outside its corner sector), or back over it
            let (bx, by) = if rng.chance(0.7) {
                let bx = norm(add(away, mul(rng.uniform(-0.3, 0.3), z)));
                let by = norm(cross(cross(bx, add(z, mul(0.3, x))), bx));
                (bx, by)
            } else {
                let bx = norm(add(mul(-1.0, x), mul(rng.uniform(-0.5, 0.5), z)));
                let by = norm(cross(cross(bx, add(z, y)), bx));
                (bx, by)
            };
            let by = if dot(by, by) > 0.5 { by } else { z };
            let bs = s * rng.uniform(0.3, 1.5);
            if rng.chance(0.5) {
                // the corner of B at its origin
                (plane_at(o, x, y, s), Some(plane_at(start, bx, by, bs)))
            } else {
                // B's far corner at the start point
                let bo = add(start, mul(-bs, add(bx, by)));
                (plane_at(o, x, y, s), Some(plane_at(bo, bx, by, bs)))
            }
        }
        Scene::SharedEdge => {
            // B shares A's edge u = s: coplanar beyond it, folded up or down, or overlapping
            let edge_o = add(o, mul(s, x));
            let fold = match rng.below(4) {
                0 => 0.0,
                1 => rng.uniform(-1.5, 1.5),
                2 => std::f64::consts::FRAC_PI_2,
                _ => rng.uniform(2.0, 3.0),
            };
            let bx = add(mul(fold.cos(), x), mul(fold.sin(), z));
            let off = if rng.chance(0.3) {
                s * rng.uniform(-1e-3, 1e-3)
            } else {
                0.0
            };
            let bo = add(edge_o, mul(off, x));
            if rng.chance(0.3) {
                // B's v axis reversed (its edge at v = s)
                let bo2 = add(bo, mul(s, y));
                (plane_at(o, x, y, s), Some(plane_at(bo2, bx, mul(-1.0, y), s)))
            } else {
                (plane_at(o, x, y, s), Some(plane_at(bo, bx, y, s)))
            }
        }
        Scene::Parallel => {
            let gap = match rng.below(3) {
                0 => 0.0,
                1 => s * rng.uniform(1e-6, 1e-3),
                _ => s * rng.uniform(0.01, 0.5),
            };
            let shift = mul(s * rng.uniform(-0.5, 0.5), x);
            let o2 = add(add(o, mul(gap, z)), shift);
            let flip = rng.chance(0.3);
            let (x2, y2) = if flip { (y, x) } else { (x, y) };
            (plane_at(o, x, y, s), Some(plane_at(o2, x2, y2, s)))
        }
        Scene::NearParallel => {
            let ang = rng.uniform(-4.5, 4.5).to_radians();
            let y2 = add(mul(ang.cos(), y), mul(ang.sin(), z));
            let o2 = add(o, mul(s * rng.uniform(-0.3, 0.3), z));
            (plane_at(o, x, y, s), Some(plane_at(o2, x, y2, s)))
        }
        Scene::Crossing => {
            let [x2, y2, _] = frame(rng);
            let c = add(o, mul(0.5 * s, add(x, y)));
            let o2 = add(c, mul(-0.5 * s, add(x2, y2)));
            (plane_at(o, x, y, s), Some(plane_at(o2, x2, y2, s)))
        }
        Scene::BSplinePlane => {
            let bs = gen_bspline(rng, s, o, [x, y, z], 0.3);
            let lift = s * rng.uniform(-0.6, 0.6);
            let p = plane_at(add(o, mul(lift, z)), x, y, s);
            if rng.chance(0.5) {
                (bs, Some(p))
            } else {
                (p, Some(bs))
            }
        }
        Scene::BSplines => {
            let b1 = gen_bspline(rng, s, o, [x, y, z], 0.3);
            let off = mul(s * rng.uniform(0.05, 2.0), unit_vec(rng));
            let f2 = frame(rng);
            let b2 = gen_bspline(rng, s, add(o, off), f2, 0.3);
            (b1, Some(b2))
        }
        Scene::SelfIntersect => {
            if rng.chance(0.5) {
                (gen_bspline(rng, s, o, [x, y, z], 0.3), None)
            } else {
                (plane_at(o, x, y, s), None)
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let s = gen_scale(rng);
    let scene = match rng.below(20) {
        0 | 1 => Scene::Apart,
        2..=5 => Scene::Corner,
        6..=8 => Scene::SharedEdge,
        9 | 10 => Scene::Parallel,
        11..=13 => Scene::NearParallel,
        14 | 15 => Scene::Crossing,
        16 => Scene::BSplinePlane,
        17 => Scene::BSplines,
        _ => Scene::SelfIntersect,
    };
    let (mut a, mut b) = gen_scene(rng, scene, s);
    // planes as bilinear B-splines, mostly
    if rng.chance(0.75) {
        a = bilinear(&a);
    }
    if rng.chance(0.75) {
        b = b.map(|x| bilinear(&x));
    }
    // sometimes curve-bounded surfaces over the planes (upstream has no surface cache for
    // them, so they get no hand-built trees)
    let mut curve_bounded = false;
    if rng.chance(0.06) && matches!(a, SurfaceIn::Plane { .. }) {
        a = SurfaceIn::CurveBounded { basis: Box::new(a) };
        curve_bounded = true;
    }
    if rng.chance(0.06) && matches!(b, Some(SurfaceIn::Plane { .. })) {
        b = b.map(|x| SurfaceIn::CurveBounded { basis: Box::new(x) });
        curve_bounded = true;
    }
    let ra = rust_surface(&a).ok()?;
    let rb = match &b {
        Some(x) => Some(rust_surface(x).ok()?),
        None => None,
    };
    let na = natural(&a);
    let nb = natural(b.as_ref().unwrap_or(&a));
    let dom1 = gen_domain(rng, na);
    let dom2 = gen_domain(rng, nb);
    let edges = if rng.chance(0.5) {
        [false, false]
    } else {
        [rng.chance(0.7), rng.chance(0.7)]
    };
    let tol = match rng.below(6) {
        0..=2 => None,
        3 => Some(s * rng.uniform(1e-4, 1e-2)),
        4 => Some(s * 1e-6),
        _ => Some(s * rng.uniform(0.01, 0.2)),
    };
    let ang = match rng.below(4) {
        0 => Some(rng.uniform(0.05, 0.6)),
        _ => None,
    };
    let trees = if curve_bounded || rng.chance(0.08) {
        None
    } else {
        let depth_a = rng.below(4) as u32;
        let depth_b = if rng.chance(0.7) {
            depth_a
        } else {
            rng.below(4) as u32
        };
        // thin boxes (grow 1e-6) make corner and edge contacts a grazing box
        let grow = if matches!(scene, Scene::Corner | Scene::SharedEdge) && rng.chance(0.6) {
            1e-6
        } else {
            *rng.choose(&[1e-3, 1e-3, 1e-6, 1e-2]).unwrap_or(&1e-3)
        };
        let ta = gen_tree(rng, &ra, na, depth_a, grow);
        let tb = rb.as_ref().map(|r| gen_tree(rng, r, nb, depth_b, grow));
        Some((ta, tb))
    };
    Some(Inputs {
        a,
        b,
        dom1,
        dom2,
        edges,
        tol,
        ang,
        trees,
        raw_knots: [false; 2],
    })
}

// ---- named edge cases -----------------------------------------------------------------------

/// Unit-square planes in fixed configurations, each with single-patch trees.
fn edge_cases() -> Vec<(String, Value)> {
    let x = [1.0, 0.0, 0.0];
    let y = [0.0, 1.0, 0.0];
    let z = [0.0, 0.0, 1.0];
    let o = [0.0, 0.0, 0.0];
    let a = plane_at(o, x, y, 1.0);
    let configs: Vec<(&str, SurfaceIn)> = vec![
        ("apart", plane_at([0.0, 0.0, 5.0], x, y, 1.0)),
        // touching at A's corner (1,1,0), leaving it
        ("corner_touch_out", plane_at([1.0, 1.0, 0.0], norm([1.0, 1.0, 0.0]), z, 1.0)),
        // B's corner sector folds back over A
        ("corner_touch_back", plane_at([1.0, 1.0, 0.0], [-1.0, 0.0, 0.0], z, 1.0)),
        // corner to corner, coplanar
        ("corner_coplanar", plane_at([1.0, 1.0, 0.0], x, y, 1.0)),
        ("edge_coplanar", plane_at([1.0, 0.0, 0.0], x, y, 1.0)),
        ("edge_fold_up", plane_at([1.0, 0.0, 0.0], z, y, 1.0)),
        ("edge_fold_45", plane_at([1.0, 0.0, 0.0], norm([1.0, 0.0, 1.0]), y, 1.0)),
        ("parallel_gap", plane_at([0.0, 0.0, 0.5], x, y, 1.0)),
        ("coincident", plane_at(o, x, y, 1.0)),
        ("coincident_swapped", plane_at(o, y, x, 1.0)),
        ("near_parallel_2deg", plane_at([0.0, 0.0, 0.01], x, norm([0.0, 1.0, 0.035]), 1.0)),
        ("crossing", plane_at([0.5, 0.0, -0.5], y, z, 1.0)),
        ("crossing_tiny", plane_at([0.5e-3, 0.0, -0.5e-3], y, z, 1e-3)),
    ];
    let mut out = Vec::new();
    for (name, b) in configs {
        for (form, sa, sb) in [
            ("plane", a.clone(), b.clone()),
            ("bilinear", bilinear(&a), bilinear(&b)),
        ] {
            let (Ok(ra), Ok(rb)) = (rust_surface(&sa), rust_surface(&sb)) else {
                continue;
            };
            let mut rng = Rng::new(0x5eed_0000 + out.len() as u64);
            for (tag, edges, trees, grow) in [
                ("noedges", [false, false], true, 1e-3),
                ("edges", [true, true], true, 1e-3),
                ("thin_noedges", [false, false], true, 1e-6),
                ("thin_edges", [true, true], true, 1e-6),
                ("cachetree", [true, true], false, 1e-3),
            ] {
                let ta = gen_tree(&mut rng, &ra, natural(&sa), 0, grow);
                let tb = gen_tree(&mut rng, &rb, natural(&sb), 0, grow);
                let i = Inputs {
                    a: sa.clone(),
                    b: Some(sb.clone()),
                    dom1: natural(&sa),
                    dom2: natural(&sb),
                    edges,
                    tol: None,
                    ang: None,
                    trees: trees.then_some((ta, Some(tb))),
                    raw_knots: [false; 2],
                };
                if let Ok(v) = serde_json::to_value(&i) {
                    out.push((format!("edge_{name}_{form}_{tag}"), v));
                }
            }
        }
    }
    let Ok(ra) = rust_surface(&a) else {
        return out;
    };
    // a surface against itself
    let ta = {
        let mut rng = Rng::new(0x5eed_1000);
        gen_tree(&mut rng, &ra, [0.0, 0.0, 1.0, 1.0], 2, 1e-3)
    };
    let i = Inputs {
        a,
        b: None,
        dom1: [0.0, 0.0, 1.0, 1.0],
        dom2: [0.0, 0.0, 1.0, 1.0],
        edges: [true, true],
        tol: Some(1e-3),
        ang: Some(0.2),
        trees: Some((ta, None)),
        raw_knots: [false; 2],
    };
    if let Ok(v) = serde_json::to_value(&i) {
        out.push(("edge_self_plane".into(), v));
    }
    out
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.surface_intersector_p5"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            if let Some(i) = gen_case(rng) {
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
        let mut v = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            v.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(v)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

/// The certificate of a touching point that `DoIntersection` adds for a start point
/// that traced nothing (WR-51): `r` is that one touching curve, its UV points lie in
/// the domains, the surfaces evaluated there are `r`'s deviation apart and closer than
/// `tol`, and its 3D point lies midway between them.
#[cfg(test)]
pub(crate) fn certify_restored_touching_point(
    k: &Kernel,
    s: [&Surface; 2],
    d: [&Extent2d; 2],
    tol: f64,
    r: &SurfaceIntersection,
) {
    use solidcore::surface::surface::p2::TsectCurveType;
    assert_eq!(r.curves_3d.len(), 1, "{r:?}");
    assert_eq!(r.curve_types, [TsectCurveType::Touching]);
    let c3 = &r.curves_3d[0];
    let p = c3.evaluate_point(k, 0.0).unwrap();
    assert_eq!(p, c3.evaluate_point(k, 1.0).unwrap(), "a degenerate curve");
    let uv = |c: &Option<Curve>| c.as_ref().unwrap().evaluate_point(k, 0.0).unwrap();
    let (uv1, uv2) = (uv(&r.surface1_uv_curves[0]), uv(&r.surface2_uv_curves[0]));
    let (uv1, uv2) = (Point2::new(uv1.x, uv1.y), Point2::new(uv2.x, uv2.y));
    assert!(d[0].contains_point(uv1, 0.0) && d[1].contains_point(uv2, 0.0));
    let p1 = s[0].evaluate_point(k, uv1).unwrap();
    let p2 = s[1].evaluate_point(k, uv2).unwrap();
    let dev = r.deviations[0];
    // the intersector evaluated its points through other paths (the boundary curve,
    // the surface caches): their rounding, far below any tolerance here
    let slack = 1e-9 * (p1.length() + p2.length() + 1.0);
    assert!((p1.distance(p2) - dev).abs() <= slack, "{} vs {dev}", p1.distance(p2));
    assert!(dev < tol, "deviation {dev} not below the tolerance {tol}");
    assert!(p1.distance(p2) < tol, "surface points not within the tolerance {tol}");
    assert!(p.distance((p1 + p2) / 2.0) <= slack + 1e-9 * dev, "{p:?} {p1:?} {p2:?}");
}

/// Replays the tube certificate (`certify::tube`) on an answer (its curves, their UV
/// curves and types) whose curve count the default tolerance from both surfaces moved
/// (tangency-labs/solidcraft#17720). Each of `chains` (the answer's curve indices, in order
/// along it) is one certified component, and the components are pairwise disjoint, so the
/// true intersection has at least `chains.len()` of them. The crossing curves in no chain
/// are `open`: each ends inside both domains at no other curve's end, since an arc of its
/// component lies in no curve (known, tangency-labs/solidcraft#18488), and they prove
/// nothing either way.
#[cfg(test)]
pub(crate) fn assert_certified_components(
    k: &Kernel,
    s: [&Surface; 2],
    d: [&Extent2d; 2],
    curves: &[Curve],
    uv: [&[Option<Curve>]; 2],
    types: &[solidcore::surface::surface::p2::TsectCurveType],
    chains: &[&[usize]],
    open: &[usize],
) {
    use crate::certify::tube::{self, AnswerCurve, System};
    use solidcore::surface::surface::p2::TsectCurveType;
    let crossing: Vec<usize> = (0..curves.len())
        .filter(|&i| types[i] == TsectCurveType::Crossing)
        .collect();
    let curves: Vec<AnswerCurve<'_>> = crossing
        .iter()
        .map(|&i| AnswerCurve {
            curve: &curves[i],
            uv: [uv[0][i].as_ref(), uv[1][i].as_ref()],
        })
        .collect();
    let sys = System::new(k, s, d);
    let mut found = Vec::new();
    let mut failed = Vec::new();
    for c in tube::components(k, &sys, &curves) {
        match c {
            Ok(c) => found.push(c),
            Err(e) => failed.push(e),
        }
    }
    let got: Vec<Vec<usize>> = found
        .iter()
        .map(|c| c.curves.iter().map(|&j| crossing[j]).collect())
        .collect();
    assert_eq!(got, chains, "the certified components; not certified: {failed:?}");
    for (i, a) in found.iter().enumerate() {
        for b in &found[i + 1..] {
            assert!(tube::disjoint(&a.tube, &b.tube), "components {:?} and {:?} meet", a.curves, b.curves);
        }
    }
    let mut rest: Vec<usize> = crossing
        .iter()
        .copied()
        .filter(|i| !chains.iter().any(|c| c.contains(i)))
        .collect();
    rest.sort_unstable();
    assert_eq!(rest, open, "the curves in no component: {failed:?}");
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcore::curve::curve_cache::{BezierAux1d, BezierSpan};
    use solidcore::geom::polar_box::PolarBoxState;
    use solidcore::geom::tree::CurveSpanRef;
    use solidcore::surface::surface::p2::cs_are_perpen_disjoint_in;

    #[test]
    fn csi_polar_cones_keep_and_reject_with_stored_and_lazy_boxes() {
        let k = Kernel::default();
        let surface =
            rust_surface(&plane_at([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 1.0)).unwrap();
        let node = NodeIn {
            parent: -1,
            c1: -1,
            c2: -1,
            aux: 3,
            bbox: [0.0, 0.0, 0.0, 1.0, 1.0, 0.0],
            dom: [0.0, 0.0, 1.0, 1.0],
            polar: vec![],
            pseudo: Some((
                [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]],
                [[0.0, 1.0], [0.0, 1.0], [0.0, 0.0]],
            )),
        };
        // The lazy path must evaluate the real owning surface, not substitute an
        // empty cone. The read-only helper must leave that cache uninitialised.
        for stored in [false, true] {
            let mut node = node.clone();
            if stored {
                node.polar.push([0.0, 0.0, 1.0]);
            }
            let cache = rust_cache(surface.clone(), &[node], H(0)).unwrap();
            let id = NodeId(0);
            let before = cache.aux(id).unwrap().polar_box;
            assert_eq!(before.state() == PolarBoxState::Uninitialized, !stored);
            let mut normal = PolarBox::new();
            normal.add_vector(Vec3::new(0.0, 0.0, 1.0), None);
            assert_eq!(cache.polar_box_unstored(&k, id).unwrap(), normal);

            for (name, ends, rejected) in [
                // Displacement is outside the surface-normal cone. The chord
                // is perpendicular to it, so the chord test cannot mask this.
                ("polar_reject", [[10.0, 0.0, 0.0], [10.0, 1.0, 0.0]], true),
                // Normal cone overlaps, but no chord direction is perpendicular
                // to the displacement cone (ConesAreDisjointPerpen).
                ("chord_reject", [[0.5, 0.5, 10.0], [0.5, 0.5, 11.0]], true),
                // Normal cone overlaps and the chord can be perpendicular to
                // the displacement: the cone tests cannot reject this branch.
                ("keep", [[0.0, 0.5, 10.0], [1.0, 0.5, 10.0]], false),
                // Overlapping boxes give a wide cone and must not reject.
                ("wide_keep", [[0.5, 0.5, -1.0], [0.5, 0.5, 1.0]], false),
            ] {
                let ends = ends.map(v3);
                let [start, end] = ends;
                let mut curve_tree = Tree::<H>::new(Some(TreeOwner::CurveCache));
                curve_tree.set_curve_data(
                    vec![BezierSpan {
                        aux: BezierAux1d {
                            ends,
                            ..BezierAux1d::default()
                        },
                        ..BezierSpan::default()
                    }],
                    vec![],
                    None,
                );
                let curve_node = TreeNode {
                    bbox: Extent3d::from_points(start, end),
                    aux_type: AuxDataType::BezierCurve,
                    data: NodeData::Curve(CurveSpanRef::Span(0)),
                    ..TreeNode::new()
                };
                let surface_node = cache.tree().unwrap().node(id).unwrap();
                assert_eq!(
                    cs_are_perpen_disjoint_in(
                        &k,
                        &curve_tree,
                        &curve_node,
                        surface_node,
                        &cache,
                        id,
                    )
                    .unwrap(),
                    rejected,
                    "{name}, stored={stored}"
                );
                assert_eq!(cache.aux(id).unwrap().polar_box, before);
            }
        }
    }

    /// WR-51: the touching points `DoIntersection` adds for a start point that traced
    /// nothing, in the five cases where the boundary search drops one: gen0236 (two
    /// plane patches 9.9 apart at their corners, kept since upstream already reported
    /// it) and the four that upstream left with no answer. Each is certified on both
    /// surfaces and within the tolerance.
    #[test]
    fn wr51_restored_touching_points_are_certified_contacts() {
        use solidcore::intersect::surface_intersector::p1::SurfaceIntersector;
        let ctx = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let names = ["gen0133", "gen0236", "gen0247", "gen0351", "gen0840"];
        let mut seen = 0;
        for case in Runner.cases(&ctx).unwrap() {
            if !names.contains(&case.name.as_str()) {
                continue;
            }
            seen += 1;
            let i: Inputs = case.parse().unwrap();
            let a = rust_surface_with(&i.a, i.raw_knots[0]).unwrap();
            let b_owned = i.b.as_ref().map(|b| rust_surface_with(b, i.raw_knots[1]).unwrap());
            let b = b_owned.as_ref().unwrap_or(&a);
            let (dom1, dom2) = (ext(i.dom1), ext(i.dom2));
            let k = Kernel::default();
            let (r, tol) = match &i.trees {
                None => {
                    let si = SurfaceIntersector::new(&k, &a, &dom1, b, &dom2, false).unwrap();
                    let tol = i.tol.unwrap_or(si.this_approx_tol_3d);
                    let r = do_intersection(&k, &a, &dom1, b, &dom2, i.edges, i.tol, i.ang);
                    (r.unwrap(), tol)
                }
                Some((ta, tb)) => {
                    let mut caches = vec![rust_cache(a.clone(), ta, H(0)).unwrap()];
                    if let Some(tb) = tb {
                        caches.push(rust_cache(b.clone(), tb, H(1)).unwrap());
                    }
                    let mut ssi: AdvSurfaceIntersector<'_, '_, H> =
                        AdvSurfaceIntersector::new(&k, &a, &dom1, b, &dom2, false).unwrap();
                    ssi.base.set_use_surface_edges(i.edges[0], i.edges[1]);
                    let r = ssi.do_intersection_with_caches(&k, &mut caches, i.tol, i.ang);
                    (r.unwrap(), ssi.base.this_approx_tol_3d)
                }
            };
            certify_restored_touching_point(&k, [&a, b], [&dom1, &dom2], tol, &r);
        }
        assert_eq!(seen, names.len());
    }

    #[test]
    fn csi_former_polar_deferrals_record_values() {
        // Exact original seeds from main CI 39940775733; no generator changes or
        // replacement inputs. Reverting the CSI call must restore the deferrals.
        for index in [393, 1277] {
            let seed = CaseContext::seed_for(Runner.id(), index);
            let inputs = Runner.generate(&mut Rng::new(seed)).unwrap();
            let case = Case {
                unit: Runner.id().into(),
                name: format!("gen{index:04}"),
                source: CaseSource::Generated { seed },
                inputs,
            };
            let rust = Runner.run_rust(&case).unwrap();
            assert!(rust.deferred.is_none(), "{case:?}");
            assert!(rust.error.is_none(), "{case:?}");
            let rec = rust.values.get("rec").unwrap();
            assert_eq!(rec.first(), Some(&0.0), "{}", case.name);
            assert!(rec.len() > 15, "{}: {} values", case.name, rec.len());
            assert!(rec.iter().skip(1).any(|v| *v != 0.0), "{}", case.name);
        }
    }

    /// gen1277 (WR-455's contact move, tangency-labs/solidcraft#17404): main stopped its one
    /// curve, along A's `v = 0` boundary, at `u = 0.0537` and found nothing along `v = 1`;
    /// the new row runs that curve to `u = 0.625` and adds one along `v = 1` over
    /// `[0, 0.875]`, tolerance 0.5635. PROVEN, per new curve: its 3D curve stays within the
    /// tolerance of A's iso curve at that `v` along its UV curve's `u` range, and of B's iso
    /// curve at the `v` its UV curve on B keeps, over that curve's `u` range
    /// (`curves_stay_within`, certified, no samples; A and B build those iso curves
    /// exactly, `max_gap_3d = 0`), so every point of it is a point of the tolerant
    /// intersection. Main's curve, A's `v = 0` over `u in [0, 0.0537]` with its B side on
    /// B's `v = 0` over `[0, 0.0773]`, lies on the first one's UV curves (main's record).
    /// Class: closer (main left out tolerant intersection the new row certifies).
    ///
    /// Not this PR (MEASURED, the same on main): curve 2's UV curve on B is the drop of
    /// its 3D curve onto B (`drop_and_trim_curve`, unchanged here, gives main the same
    /// line from the same call), a line along B's `v` max edge to `u = 2.846`, where the
    /// nearest point of that edge to the curve's end is near `u = 2.527`; B at that UV end
    /// is 0.600 from the 3D curve's end, beyond the tolerance. The path for B therefore
    /// ends near 2.527, not at the UV curve's end.
    #[test]
    fn wr455_gen1277_new_curves_are_certified_tolerant_intersections() {
        use solidcore::curve::curve::p8::curves_stay_within;
        use solidcore::nurbs::b_spline_surface::SurfParamType;
        let seed = CaseContext::seed_for(Runner.id(), 1277);
        let i: Inputs = serde_json::from_value(Runner.generate(&mut Rng::new(seed)).unwrap())
            .unwrap();
        let k = Kernel::default();
        let a = rust_surface_with(&i.a, i.raw_knots[0]).unwrap();
        let b = rust_surface_with(i.b.as_ref().unwrap(), i.raw_knots[1]).unwrap();
        let (dom1, dom2) = (ext(i.dom1), ext(i.dom2));
        let tol = i.tol.unwrap();
        assert_eq!(tol, 0.563_509_128_660_695);
        // as the runner: on caches holding the case's hand-built trees
        let (ta, tb) = i.trees.as_ref().unwrap();
        let mut caches = vec![
            rust_cache(a.clone(), ta, H(0)).unwrap(),
            rust_cache(b.clone(), tb.as_ref().unwrap(), H(1)).unwrap(),
        ];
        let mut ssi: AdvSurfaceIntersector<'_, '_, H> =
            AdvSurfaceIntersector::new(&k, &a, &dom1, &b, &dom2, false).unwrap();
        ssi.base.set_use_surface_edges(i.edges[0], i.edges[1]);
        let r = ssi.do_intersection_with_caches(&k, &mut caches, i.tol, i.ang).unwrap();
        assert_eq!(r.curves_3d.len(), 2);
        // per curve: A's iso v, its u range, B's iso v, the B parameters the path from the
        // curve's start to its end runs between (curve 1: its UV curve's ends on B; curve 2:
        // from its UV curve's start to near the nearest point of B's edge to the curve's end,
        // see above: any end with a certified node only picks the path)
        let rows = [
            (0.0, [0.0, 0.625], 0.0, [0.0, 2.194_154_392_942_427]),
            (1.0, [0.0, 0.875], 3.281_850_727_176_471, [0.0, 2.527]),
        ];
        for (j, (va, ua, vb, ub)) in rows.into_iter().enumerate() {
            let c3 = &r.curves_3d[j];
            let ivl = c3.natural_interval();
            assert_eq!([ivl.min(), ivl.max()], ua, "curve {j}: the 3D curve's interval");
            assert!(r.deviations[j] < tol);
            let uv = |c: &Option<Curve>, t: f64| {
                let p = c.as_ref().unwrap().evaluate_point(&k, t).unwrap();
                [p.x, p.y]
            };
            // the UV curves run along A's iso `va` over `ua` and B's iso `vb`
            let e1 = [uv(&r.surface1_uv_curves[j], ua[0]), uv(&r.surface1_uv_curves[j], ua[1])];
            let e2 = [uv(&r.surface2_uv_curves[j], ua[0]), uv(&r.surface2_uv_curves[j], ua[1])];
            assert!(e1.iter().all(|p| (p[1] - va).abs() <= 1e-15), "curve {j}: {e1:?}");
            assert!((e1[0][0] - ua[0]).abs() <= 1e-15 && (e1[1][0] - ua[1]).abs() <= 1e-15);
            assert!(e2.iter().all(|p| (p[1] - vb).abs() <= 1e-15), "curve {j}: {e2:?}");
            assert!(e2[0][0].abs() <= 1e-15, "curve {j}: {e2:?}");
            for (s, v, u) in [(&a, va, ua), (&b, vb, ub)] {
                let iso = s
                    .create_iso_parametric_curve(&k, SurfParamType::V, v, 0.0, None, false)
                    .unwrap();
                assert_eq!(iso.max_gap_3d, 0.0, "curve {j}: an exact iso curve");
                assert!(
                    curves_stay_within(&k, c3, &iso.curve, [ua[0], u[0]], [ua[1], u[1]], tol)
                        .unwrap(),
                    "curve {j}: not certified within {tol} of the iso curve v = {v}"
                );
            }
        }
    }


    /// tangency-labs/solidcraft#17720: the rows whose curve count the default from both
    /// surfaces moved, replayed through the tube certificate: each curve is one
    /// component.
    #[test]
    fn moved_count_rows_are_certified_components() {
        for name in [
            "edge_crossing_tiny_bilinear_noedges",
            "edge_crossing_tiny_bilinear_thin_noedges",
            "edge_crossing_tiny_plane_noedges",
            "edge_crossing_tiny_plane_thin_noedges",
        ] {
            let case = crate::case::regression_case(&Runner, name);
            let i: Inputs = case.parse().unwrap();
            let a = rust_surface_with(&i.a, i.raw_knots[0]).unwrap();
            let b_owned = i.b.as_ref().map(|b| rust_surface_with(b, i.raw_knots[1]).unwrap());
            let b = b_owned.as_ref().unwrap_or(&a);
            let (dom1, dom2) = (ext(i.dom1), ext(i.dom2));
            let k = Kernel::default();
            let r = match &i.trees {
                None => do_intersection(&k, &a, &dom1, b, &dom2, i.edges, i.tol, i.ang).unwrap(),
                Some((ta, tb)) => {
                    let mut caches = vec![rust_cache(a.clone(), ta, H(0)).unwrap()];
                    if let Some(tb) = tb {
                        caches.push(rust_cache(b.clone(), tb, H(1)).unwrap());
                    }
                    let mut ssi: AdvSurfaceIntersector<'_, '_, H> =
                        AdvSurfaceIntersector::new(&k, &a, &dom1, b, &dom2, false).unwrap();
                    ssi.base.set_use_surface_edges(i.edges[0], i.edges[1]);
                    ssi.do_intersection_with_caches(&k, &mut caches, i.tol, i.ang).unwrap()
                }
            };
            assert_certified_components(
                &k,
                [&a, b],
                [&dom1, &dom2],
                &r.curves_3d,
                [&r.surface1_uv_curves, &r.surface2_uv_curves],
                &r.curve_types,
                &[&[0]],
                &[],
            );
        }
    }
}
