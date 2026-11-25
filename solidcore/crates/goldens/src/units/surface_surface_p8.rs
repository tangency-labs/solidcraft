//! Runner for unit `surface.surface_p8`: `SmSurface` part 8 (the knots below a continuity,
//! the coincidence check, the blend surface, the global line and curve intersections, the
//! local line intersection, the point inversion near a singularity and
//! `SmSPGlobalSolver::LocalSolve`).
//!
//! Ported from upstream `source/SMLib/src/SmSurface.cpp` (on B-spline surfaces, planes and
//! surfaces of revolution); checked against its golden. Rust: `solidcore::surface::surface::p8`.
//!
//! A case is one `op` on a `surface` (`{"bspline": {oob, form, net, p, q, u, v}}` with control
//! points as stored `[x, y, z, w]`, rows in u; `{"plane": {o, n}}`; or `{"revolution": {a, b, p,
//! axis, swap}}`). The Rust side produces a flat record (status first); the runner reports it as the `out` values. A Rust error maps to its `SmStatus`
//! (`1007` for `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the
//! case.
//!
//! The statics `sm_ExtractIsoCurve` and `sm_IntersectSrfBndryCrv` are not reachable from a
//! case (their callers are unported); the Rust unit tests cover them.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::solution_array::{ObjectKind, SolutionArray, SolutionObject, SolutionType};
use solidcore::geom::tree::{NodeId, Tree};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm};
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::p7::SpGlobalSolver;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_cache::{SurfaceCache, SurfaceCacheOptions};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{p2, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `surface.surface_p8` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SrfIn {
    Bspline(BssIn),
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    /// `SmSurfOfRevolution::CreateCanonical` of the line `a`-`b` about the axis through
    /// `p` along `axis`.
    Revolution {
        a: [f64; 3],
        b: [f64; 3],
        p: [f64; 3],
        axis: [f64; 3],
        swap: bool,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Knots {
        cont: u8,
        at: bool,
    },
    Coinc {
        same: bool,
        other: SrfIn,
        tol: f64,
        whole: bool,
    },
    Line {
        dom: [f64; 4],
        p: [f64; 3],
        v: [f64; 3],
        ivl: Option<[f64; 2]>,
        ray: bool,
        tol: f64,
    },
    Curve {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        tol: f64,
    },
    LocalLine {
        dom: [f64; 4],
        p: [f64; 3],
        v: [f64; 3],
        ivl: [f64; 2],
        dtol: f64,
        atol: f64,
        uv: [f64; 2],
    },
    Invert {
        p: [f64; 3],
        dom: [f64; 4],
        uv: [f64; 2],
        tol: f64,
    },
    Blend {
        curve1: CrvIn,
        ivl1: [f64; 2],
        right: bool,
        other: SrfIn,
        curve2: CrvIn,
        ivl2: [f64; 2],
        left: bool,
        tol: f64,
    },
    SpLocal {
        dom: [f64; 4],
        p: [f64; 3],
        sop: u8,
        tol: f64,
        best_sq: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SrfIn,
    op: OpIn,
}

// ---- Rust side ----------------------------------------------------------------------

/// The surface, `None` when its construction fails (status `-3`).
fn surface_of(k: &Kernel, s: &SrfIn) -> Result<Option<Surface>, HarnessError> {
    Ok(match s {
        SrfIn::Revolution {
            a,
            b,
            p,
            axis,
            swap,
        } => {
            let mut gen = Line::from_points(v3(*a), v3(*b), 3);
            gen.make_nurb()
                .map_err(|e| harness(format!("line nurb: {e}")))?;
            SurfOfRevolution::create_canonical(k, SmCurve::from(gen), v3(*p), v3(*axis), *swap)
                .ok()
                .map(Surface::from)
        }
        SrfIn::Plane { o, n } => Some(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
        SrfIn::Bspline(bs) => {
            let net = bs
                .net
                .iter()
                .map(|r| {
                    r.iter()
                        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, bs.p, bs.q, bs.u.clone(), bs.v.clone())
                .map_err(|e| harness(format!("surface input: {e}")))?;
            let mut s = BSplineSurface::from_nurb(nl, false);
            *s.out_of_bounds_enabled_mut() = bs.oob;
            let form = BSplineSurfaceForm::from_code(u64::from(bs.form))
                .ok_or_else(|| harness(format!("bad form {}", bs.form)))?;
            s.set_b_spline_surface_form(form);
            Some(Surface::from(s))
        }
    })
}

/// A solution object: the op's curve (with its natural interval) or surface (with its
/// natural domain), or an opaque handle of either that cannot reach its geometry.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Obj {
    Curve([f64; 2]),
    Surface([f64; 4]),
    OpaqueCurve,
    OpaqueSurface,
}

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        match self {
            Obj::Curve(_) | Obj::OpaqueCurve => ObjectKind::Curve,
            Obj::Surface(_) | Obj::OpaqueSurface => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Obj::Curve(i) => Some(ivl_of(*i)),
            _ => None,
        }
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Obj::Surface(d) => Some(dom_of(*d)),
            _ => None,
        }
    }
}

fn surface_obj(s: &Surface) -> Obj {
    // (runner helper without a kernel in scope)
    let d = s.natural_uv_domain_if_built();
    Obj::Surface([d.min().x, d.min().y, d.max().x, d.max().y])
}

fn curve_obj(c: &SmCurve) -> Obj {
    let i = c.natural_interval();
    Obj::Curve([i.min(), i.max()])
}

/// The solution records (objects `-1` NULL, `0` the curve, `1` the surface).
fn put_solutions(o: &mut Vec<f64>, sols: &SolutionArray<Obj>, curve: Option<Obj>, surface: Obj) {
    o.push(sols.len() as f64);
    for sol in sols.as_slice() {
        o.extend([
            f64::from(sol.solution_type.code()),
            sol.num_variables as f64,
            sol.num_objects as f64,
            sol.start.solution_value,
        ]);
        let range = sol.solution_type == SolutionType::RangeOfValues;
        o.push(if range { sol.end.solution_value } else { 0.0 });
        o.extend(sol.start.parameters.iter().take(3));
        if range {
            o.extend(sol.end.parameters.iter().take(3));
        } else {
            o.extend([0.0; 3]);
        }
        for j in 0..2 {
            let code = match sol.objects.get(j).copied().flatten() {
                None => -1.0,
                Some(x) if Some(x) == curve => 0.0,
                Some(x) if x == surface => 1.0,
                Some(_) => -2.0,
            };
            o.push(code);
        }
    }
}

/// The first leaf of a tree (`GetFirstLeafNode`).
fn first_leaf(tree: &Tree<Obj>) -> Result<NodeId, HarnessError> {
    let mut id = tree.top_node().ok_or_else(|| harness("tree without top"))?;
    loop {
        let node = tree.node(id).map_err(|e| harness(e.to_string()))?;
        match node.child1 {
            Some(c) => id = c,
            None => return Ok(id),
        }
    }
}

#[allow(clippy::too_many_lines)]
fn rust_run(i: &Inputs) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut o = Vec::new();
    let Some(s) = surface_of(&k, &i.surface)? else {
        return Ok(vec![-3.0]);
    };
    match &i.op {
        OpIn::Knots { cont, at } => {
            match st(s.find_knots_by_max_continuity(&k, cont_of(*cont)?, *at))? {
                Ok((u, v)) => {
                    o.extend([crate::status::ok_slot(0.0), u.len() as f64]);
                    o.extend(&u);
                    o.push(v.len() as f64);
                    o.extend(&v);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Coinc {
            same,
            other,
            tol,
            whole,
        } => {
            let Some(s2) = surface_of(&k, other)? else {
                return Ok(vec![-3.0]);
            };
            let target = if *same { &s } else { &s2 };
            match st(s.coincidence_check(&k, target, *tol, *whole))? {
                Ok(c) => o.extend([crate::status::ok_slot(0.0), b(c.are_coincident), c.max_distance]),
                Err(c) => o.push(c),
            }
        }
        OpIn::Line {
            dom,
            p,
            v,
            ivl,
            ray,
            tol,
        } => {
            let surf = surface_obj(&s);
            let ivl = ivl.map(ivl_of);
            match st(s.global_line_intersect::<Obj>(
                &k,
                surf,
                &dom_of(*dom),
                v3(*p),
                v3(*v),
                ivl.as_ref(),
                *ray,
                *tol,
            ))? {
                Ok(sols) => {
                    o.push(0.0);
                    put_solutions(&mut o, &sols, None, surf);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Curve {
            dom,
            curve,
            ivl,
            tol,
        } => {
            let Some(c) = curve_of(curve)? else {
                return Ok(vec![-3.0]);
            };
            // Once with handles that know their domains, once with opaque handles (as a
            // B-rep's, which cannot reach their geometry): both must give the same result,
            // so a difference reports both runs.
            let mut runs = Vec::new();
            for (co, so) in [
                (curve_obj(&c), surface_obj(&s)),
                (Obj::OpaqueCurve, Obj::OpaqueSurface),
            ] {
                let mut r = Vec::new();
                match st(s.global_curve_intersect::<Obj>(
                    &k,
                    [co, so],
                    &dom_of(*dom),
                    &c,
                    &ivl_of(*ivl),
                    *tol,
                ))? {
                    Ok(sols) => {
                        r.push(0.0);
                        put_solutions(&mut r, &sols, Some(co), so);
                    }
                    Err(c) => r.push(c),
                }
                runs.push(r);
            }
            let opaque = runs.pop().unwrap_or_default();
            o = runs.pop().unwrap_or_default();
            if o != opaque {
                o.push(-9.0);
                o.extend(opaque);
            }
        }
        OpIn::LocalLine {
            dom,
            p,
            v,
            ivl,
            dtol,
            atol,
            uv,
        } => match st(s.local_line_intersect(&k, 
            &dom_of(*dom),
            v3(*p),
            v3(*v),
            &ivl_of(*ivl),
            *dtol,
            *atol,
            p2(*uv),
        ))? {
            Ok(r) => {
                o.extend([0.0, b(r.found)]);
                if r.found {
                    o.extend([r.uv.x, r.uv.y, r.t, r.deviation]);
                    pv(&mut o, r.point);
                    pv(&mut o, r.normal);
                }
            }
            Err(c) => o.push(c),
        },
        OpIn::Invert { p, dom, uv, tol } => {
            match st(s.invert_point_near_singularity(&k, v3(*p), &dom_of(*dom), p2(*uv), *tol))? {
                Ok(r) => o.extend([crate::status::ok_slot(0.0), f64::from(r.reason as i32), r.uv.x, r.uv.y]),
                Err(c) => o.push(c),
            }
        }
        OpIn::Blend {
            curve1,
            ivl1,
            right,
            other,
            curve2,
            ivl2,
            left,
            tol,
        } => {
            let Some(s2) = surface_of(&k, other)? else {
                return Ok(vec![-3.0]);
            };
            let (Some(c1), Some(c2)) = (curve_of(curve1)?, curve_of(curve2)?) else {
                return Ok(vec![-3.0]);
            };
            match st(s.create_blend_surface(
                &k,
                &c1,
                &ivl_of(*ivl1),
                *right,
                &s2,
                &c2,
                &ivl_of(*ivl2),
                *left,
                *tol,
                None,
            ))? {
                Ok(blend) => {
                    let nd = blend.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
                    o.extend([0.0, nd.min().x, nd.min().y, nd.max().x, nd.max().y]);
                    for a in 0..=4 {
                        for c in 0..=4 {
                            let uv = nd.evaluate(f64::from(a) / 4.0, f64::from(c) / 4.0);
                            match st(blend.evaluate_point(&k, uv))? {
                                Ok(p) => pv(&mut o, p),
                                Err(_) => o.extend([f64::NAN; 3]),
                            }
                        }
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::SpLocal {
            dom,
            p,
            sop,
            tol,
            best_sq,
        } => {
            let handle = surface_obj(&s);
            let mut cache = SurfaceCache::<Obj>::new(
                s.clone(),
                s.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?,
                SurfaceCacheOptions {
                    chord_height_tolerance: 0.01,
                    ..SurfaceCacheOptions::default()
                },
            );
            cache.set_surface_handle(Some(handle));
            if st(cache.build_tree(&k, false))?.is_err() {
                return Ok(vec![-3.0]);
            }
            let pt = v3(*p);
            let point_tree: Tree<Obj> = Tree::from_point(pt, 0.0, false);
            let pid = point_tree.top_node().ok_or_else(|| harness("point tree"))?;
            let stree = cache.tree().ok_or_else(|| harness("surface tree"))?;
            let leaf = first_leaf(stree)?;
            let mut solver =
                SpGlobalSolver::new(&k, &point_tree, stree, Some(cache.surface()), dom_of(*dom));
            solver.surface_cache = Some(&cache);
            solver.base.operation = sop_of(*sop)?;
            solver.base.tolerance_3d = *tol;
            solver.base.best_answer_so_far_sq = *best_sq;
            let branch = [Some(pid), Some(leaf), None];
            match st(solver.local_solve_branch(&branch))? {
                Ok(more) => {
                    o.extend([
                        0.0,
                        b(more),
                        solver.base.solutions.len() as f64,
                    ]);
                    for sol in solver.base.solutions.as_slice() {
                        o.extend([
                            f64::from(sol.solution_type.code()),
                            sol.num_variables as f64,
                            sol.start.solution_value,
                        ]);
                        o.extend(sol.start.parameters.iter().take(2));
                    }
                }
                Err(c) => o.push(c),
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_run(i) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

fn sop_of(v: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::from_code(i32::from(v)).ok_or_else(|| harness(format!("bad solver op {v}")))
}

// ---- generator --------------------------------------------------------------------

fn gen_surface(rng: &mut Rng) -> SrfIn {
    match rng.below(20) {
        0..=3 => gen_plane(rng),
        4..=5 => gen_revolution(rng),
        6..=7 => SrfIn::Bspline(gen_bss(rng, Shape::Pole)),
        8 => SrfIn::Bspline(gen_bss(rng, Shape::TwoPoles)),
        9..=10 => SrfIn::Bspline(gen_bss(rng, Shape::Crease)),
        11 => SrfIn::Bspline(gen_bss(rng, Shape::Herringbone)),
        12..=13 => SrfIn::Bspline(gen_bss(rng, Shape::Flat)),
        _ => SrfIn::Bspline(gen_bss(rng, Shape::Bumpy)),
    }
}

/// A point near the surface (on it at a random uv when its shape is known) and a direction.
fn gen_line(k: &Kernel, rng: &mut Rng, surface: &SrfIn) -> ([f64; 3], [f64; 3]) {
    let d = natural_domain(surface);
    let kk = Kernel::default();
    let target = surface_of(&kk, surface)
        .ok()
        .flatten()
        .and_then(|s| {
            let uv = Point2::new(
                d[0] + (d[2] - d[0]) * rng.uniform(0.0, 1.0),
                d[1] + (d[3] - d[1]) * rng.uniform(0.0, 1.0),
            );
            s.evaluate_point(k, uv).ok()
        })
        .map_or([0.5, 0.5, 0.0], |p| [p.x, p.y, p.z]);
    let dir = match rng.below(6) {
        0 => [0.0, 0.0, 1.0],
        1 => [1.0, 0.0, 0.0],
        2 => [0.0, 1.0, 0.0],
        _ => rng.point3(-1.0, 1.0),
    };
    let len = (dir[0] * dir[0] + dir[1] * dir[1] + dir[2] * dir[2]).sqrt();
    let dir = if len < 1e-3 { [0.0, 0.0, 1.0] } else { dir };
    let back = rng.uniform(-3.0, 3.0);
    let scale = rng.uniform(0.2, 5.0);
    let dir = [dir[0] * scale, dir[1] * scale, dir[2] * scale];
    let p = [
        target[0] - back * dir[0],
        target[1] - back * dir[1],
        target[2] - back * dir[2],
    ];
    (p, dir)
}

fn gen_tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[1e-6, 1e-6, 1e-5, 1e-4, 1e-3, 1e-8])
        .unwrap_or(&1e-6)
}

#[allow(clippy::too_many_lines)]
fn gen_case(k: &Kernel, rng: &mut Rng) -> Inputs {
    match rng.below(24) {
        0..=1 => Inputs {
            surface: gen_surface(rng),
            op: OpIn::Knots {
                cont: *rng.choose(&[1, 2, 3, 7, 10, 13]).unwrap_or(&7),
                at: rng.chance(0.4),
            },
        },
        2..=6 => {
            let surface = gen_surface(rng);
            // the other: the same or a perturbed copy, a flipped copy, a shifted copy,
            // a plane or something else
            let other = match rng.below(6) {
                0 | 1 => surface.clone(),
                2 => shifted(&surface, rng.uniform(1e-9, 1e-3)),
                3 => shifted(&surface, rng.uniform(0.1, 3.0)),
                4 => gen_plane(rng),
                _ => gen_surface(rng),
            };
            Inputs {
                surface,
                op: OpIn::Coinc {
                    same: rng.chance(0.1),
                    other,
                    tol: gen_tol(rng),
                    whole: rng.chance(0.6),
                },
            }
        }
        7..=11 => {
            let surface = gen_surface(rng);
            let dom = gen_subdomain(rng, natural_domain(&surface));
            let (p, v) = gen_line(k, rng, &surface);
            let ivl = rng.chance(0.5).then(|| {
                let a = rng.uniform(-6.0, 1.0);
                [a, a + rng.uniform(0.5, 12.0)]
            });
            Inputs {
                surface,
                op: OpIn::Line {
                    dom,
                    p,
                    v,
                    ivl,
                    ray: rng.chance(0.25),
                    tol: gen_tol(rng),
                },
            }
        }
        12..=15 => {
            let surface = gen_surface(rng);
            let dom = gen_subdomain(rng, natural_domain(&surface));
            let (curve, ivl) = if rng.chance(0.5) {
                let (p, v) = gen_line(k, rng, &surface);
                let t = rng.uniform(0.5, 4.0);
                (
                    CrvIn::Line {
                        a: p,
                        b: [p[0] + t * v[0], p[1] + t * v[1], p[2] + t * v[2]],
                    },
                    [0.0, t * (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()],
                )
            } else {
                gen_curve(rng, 1.0, true)
            };
            Inputs {
                surface,
                op: OpIn::Curve {
                    dom,
                    curve,
                    ivl,
                    tol: gen_tol(rng),
                },
            }
        }
        16..=18 => {
            let surface = gen_surface(rng);
            let dom = gen_subdomain(rng, natural_domain(&surface));
            let (p, v) = gen_line(k, rng, &surface);
            let nd = natural_domain(&surface);
            let uv = [
                nd[0] + (nd[2] - nd[0]) * rng.uniform(0.0, 1.0),
                nd[1] + (nd[3] - nd[1]) * rng.uniform(0.0, 1.0),
            ];
            Inputs {
                surface,
                op: OpIn::LocalLine {
                    dom,
                    p,
                    v,
                    ivl: *rng
                        .choose(&[[-10.0, 10.0], [0.0, 10.0], [-1.0, 1.0], [-100.0, 100.0]])
                        .unwrap_or(&[-10.0, 10.0]),
                    dtol: gen_tol(rng),
                    atol: *rng.choose(&[1e-9, 1e-10, 1e-8]).unwrap_or(&1e-9),
                    uv,
                },
            }
        }
        19..=20 => {
            // mostly poles: the point near a collapsed row or a cone apex
            let surface = match rng.below(3) {
                0 => SrfIn::Bspline(gen_bss(rng, Shape::Pole)),
                1 => SrfIn::Bspline(gen_bss(rng, Shape::TwoPoles)),
                _ => gen_revolution(rng),
            };
            let nd = natural_domain(&surface);
            let kk = Kernel::default();
            let uv = [
                nd[0] + (nd[2] - nd[0]) * *rng.choose(&[0.0, 0.01, 0.5, 1.0, 0.99]).unwrap_or(&0.01),
                nd[1] + (nd[3] - nd[1]) * rng.uniform(0.0, 1.0),
            ];
            let base = surface_of(&kk, &surface)
                .ok()
                .flatten()
                .and_then(|s| s.evaluate_point(k, Point2::new(uv[0], uv[1])).ok())
                .map_or([0.0; 3], |p| [p.x, p.y, p.z]);
            let off = rng.point3(-0.3, 0.3);
            let p = [base[0] + off[0], base[1] + off[1], base[2] + off[2]];
            let guess = [
                (uv[0] + (nd[2] - nd[0]) * rng.uniform(-0.02, 0.02)).clamp(nd[0], nd[2]),
                (uv[1] + (nd[3] - nd[1]) * rng.uniform(-0.1, 0.1)).clamp(nd[1], nd[3]),
            ];
            Inputs {
                surface,
                op: OpIn::Invert {
                    p,
                    dom: nd,
                    uv: guess,
                    tol: gen_tol(rng),
                },
            }
        }
        21 => {
            let surface = gen_surface(rng);
            let other = gen_surface(rng);
            let (c1, i1) = gen_curve(rng, 1.0, false);
            let (c2, i2) = gen_curve(rng, 1.0, false);
            Inputs {
                surface,
                op: OpIn::Blend {
                    curve1: c1,
                    ivl1: i1,
                    right: rng.chance(0.5),
                    other,
                    curve2: c2,
                    ivl2: i2,
                    left: rng.chance(0.5),
                    tol: *rng.choose(&[0.0, 1e-3, 1e-2]).unwrap_or(&0.0),
                },
            }
        }
        _ => {
            // a flat B-spline patch and a point near, inside or far from it
            let surface = SrfIn::Bspline(gen_bss(rng, Shape::Flat));
            let nd = natural_domain(&surface);
            let (p, _) = gen_line(k, rng, &surface);
            Inputs {
                surface,
                op: OpIn::SpLocal {
                    dom: gen_subdomain(rng, nd),
                    p,
                    sop: *rng.choose(&[0, 0, 3, 4]).unwrap_or(&0),
                    tol: gen_tol(rng),
                    best_sq: *rng.choose(&[1e20, 1e20, 1.0, 0.01]).unwrap_or(&1e20),
                },
            }
        }
    }
}

/// `s` moved by `d` in `z` (control points, plane origin, revolution points).
fn shifted(s: &SrfIn, d: f64) -> SrfIn {
    match s {
        SrfIn::Bspline(bs) => {
            let mut bs = bs.clone();
            for row in &mut bs.net {
                for c in row {
                    // w-scaled rational points move by d * w
                    let w = if c[3] == NOW { 1.0 } else { c[3] };
                    c[2] += d * w;
                }
            }
            SrfIn::Bspline(bs)
        }
        SrfIn::Plane { o, n } => SrfIn::Plane {
            o: [o[0], o[1], o[2] + d],
            n: *n,
        },
        SrfIn::Revolution {
            a,
            b,
            p,
            axis,
            swap,
        } => SrfIn::Revolution {
            a: [a[0], a[1], a[2] + d],
            b: [b[0], b[1], b[2] + d],
            p: [p[0], p[1], p[2] + d],
            axis: *axis,
            swap: *swap,
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p8"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let i = gen_case(k, rng);
            if let Err(Fail::Harness(_)) = rust_run(&i) {
                continue;
            }
            return serde_json::to_value(i).ok();
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

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    /// The drops `CreateBlendSurface`'s sampler makes along one boundary curve
    /// (`GetSampleValues`, 100 samples and the end): each curve point dropped onto the
    /// surface over its natural domain, from the previous foot (none for the first). Each
    /// foot is enclosed ([`crate::certify::nearest`]); returns the widest enclosure relative
    /// to its upper end.
    fn blend_drops(k: &Kernel, name: &str, surface: &Surface, curve: &SmCurve, ivl: &Extent1d) -> f64 {
        use solidcore::surface::surface::p4::NoObject;
        use solidcore::surface::surface::p5::DropPointOptions;
        let domain = surface.natural_uv_domain_with_nurb(k).expect("domain");
        let mut guess = None;
        let mut widest = 0.0_f64;
        for ii in 0..=100_u32 {
            let param = if ii < 100 { f64::from(ii) / 100.0 } else { 1.0 };
            let p = curve.evaluate_point(k, ivl.evaluate(param)).expect("curve point");
            let opts = DropPointOptions {
                guess_uv: guess,
                ..Default::default()
            };
            let d = surface.drop_point::<NoObject>(k, p, &domain, &opts).expect("drop");
            assert!(d.success, "{name} sample {ii}: the drop failed");
            let e = crate::certify::nearest(k, surface, None, p, d.result_uv);
            assert!(e.lo <= e.hi, "{name} sample {ii}: {e:?}");
            if e.width() > 1e-12 * e.hi.max(1.0) {
                // the drop without a guess (the global solve), for comparison
                let g = surface
                    .drop_point::<NoObject>(k, p, &domain, &DropPointOptions::default())
                    .expect("drop");
                let at = |uv| p.distance(surface.evaluate_point(k, uv).expect("point"));
                eprintln!(
                    "{name} sample {ii}: foot {:?} distance {:e} in [{:e}, {:e}]; global foot {:?} distance {:e}",
                    d.result_uv,
                    at(d.result_uv),
                    e.lo,
                    e.hi,
                    g.result_uv,
                    at(g.result_uv)
                );
            }
            widest = widest.max(e.width() / e.hi.max(f64::MIN_POSITIVE));
            guess = Some(d.result_uv);
        }
        widest
    }

    /// WR-56 (#17467, #17773): a blend surface between boundary curves of two surfaces,
    /// whose cross derivatives are sampled at the feet of the curves' points dropped onto
    /// their surfaces, each drop guessed from the one before. Every foot is certified the
    /// nearest point of its surface, to a width of 1e-9 of its distance
    /// (its upper end): the moved row `gen1959`'s 2 x 101 feet, and `gen0378`'s 101 on its first
    /// surface, where taking a guessed local result nearer than its guess kept local
    /// minima up to 0.19 farther than the nearest point (samples 63 to 81) until the
    /// global solve decided them. (`gen0378`'s second surface is not asserted: its row is
    /// main's, and one foot there, which the global solve also finds, is enclosed only to
    /// 5.3e-4.)
    #[test]
    fn wr56_blend_samples_drop_to_the_nearest_point() {
        for (name, sides) in [("gen0378", &[1][..]), ("gen1959", &[1, 2][..])] {
            let case = crate::case::regression_case(&Runner, name);
            let i: Inputs = case.parse().expect("inputs");
            let row = rust_run(&i).unwrap_or_else(|_| panic!("{name}: rust"));
            assert_eq!(row.first(), Some(&0.0), "{name}: status");
            let OpIn::Blend {
                curve1,
                ivl1,
                other,
                curve2,
                ivl2,
                ..
            } = &i.op
            else {
                panic!("{name}: not a blend case");
            };
            let k = Kernel::default();
            let s1 = surface_of(&k, &i.surface).expect("surface").expect("a surface");
            let s2 = surface_of(&k, other).expect("surface").expect("a surface");
            let c1 = curve_of(curve1).expect("curve").expect("a curve");
            let c2 = curve_of(curve2).expect("curve").expect("a curve");
            for (side, s, c, ivl) in [(1, &s1, &c1, ivl1), (2, &s2, &c2, ivl2)] {
                if !sides.contains(&side) {
                    continue;
                }
                let widest = blend_drops(&k, name, s, c, &ivl_of(*ivl));
                eprintln!("{name} surface {side}: widest relative enclosure {widest:e}");
                assert!(widest <= 1e-9, "{name} surface {side}: {widest:e}");
            }
        }
    }

    /// `InvertPointNearSingularity` near an edge that is not collapsed (`gen1191`: the
    /// guess is at u = 0.996, and the boundary u = 1 runs from 4.003 to 36085 away from
    /// the test point, tangency-labs/solidcraft#18052). The method assumes that boundary
    /// is a pole, and its drops land at best 1163.2 from the test point (v = 0.4624),
    /// nearer than the guess (1395.2) but not the closest point: S(1, 0.41725) is
    /// certified 9.399 away. The test point dropped onto the boundary finds a point
    /// proven nearer than the answer, so the boundary is not a pole and the inversion
    /// refuses (1001, main's status). Fails without the boundary check (the drop at
    /// v = 0.4624 is reported found).
    #[test]
    fn invert_near_singularity_refuses_a_drop_off_a_boundary_that_is_not_a_pole() {
        let case = crate::case::regression_case(&Runner, "gen1191");
        let i: Inputs = case.parse().expect("inputs");
        let OpIn::Invert { p, dom, uv, tol } = &i.op else {
            panic!("gen1191: not an invert case");
        };
        let k = Kernel::default();
        let s = surface_of(&k, &i.surface).expect("surface").expect("a surface");
        let r = s.invert_point_near_singularity(&k, v3(*p), &dom_of(*dom), p2(*uv), *tol);
        let e = r.expect_err("gen1191: a drop off a boundary that is not a pole is refused");
        assert_eq!(status(&e), 1001.0, "gen1191: {e}");
        // (the drop the search kept is proven farther than a point of that boundary)
        let near = wr57_distance(&k, &s, v3(*p), Point2::new(1.0, 0.41725)).expect("enclosed");
        let drop = wr57_distance(&k, &s, v3(*p), Point2::new(1.0, 0.462_430_875_879_150_77)).expect("enclosed");
        assert!(near.1 < 10.0 && drop.0 > 1000.0 && near.1 < drop.0, "gen1191: {near:?} {drop:?}");
        let row = rust_run(&i).unwrap_or_else(|_| panic!("gen1191: rust"));
        assert_eq!(row, [1001.0], "gen1191: row");
    }

    /// `InvertPointNearSingularity` refuses an answer proven farther than its guess
    /// (`gen0158`: the guess is 2.2566 from the test point, and the nearest drop is
    /// proven farther; main reports the drop at u = 0.1249, 9.1456 away, found). Status 1001, main's for a
    /// search that does not converge. Fails with the farther-than-the-guess refusal
    /// removed.
    #[test]
    fn invert_near_singularity_refuses_an_answer_farther_than_its_guess() {
        let case = crate::case::regression_case(&Runner, "gen0158");
        let i: Inputs = case.parse().expect("inputs");
        let OpIn::Invert { p, dom, uv, tol } = &i.op else {
            panic!("gen0158: not an invert case");
        };
        let k = Kernel::default();
        let s = surface_of(&k, &i.surface).expect("surface").expect("a surface");
        let r = s.invert_point_near_singularity(&k, v3(*p), &dom_of(*dom), p2(*uv), *tol);
        let e = r.expect_err("gen0158: an answer farther than the guess is refused");
        assert_eq!(status(&e), 1001.0, "gen0158: {e}");
        let row = rust_run(&i).unwrap_or_else(|_| panic!("gen0158: rust"));
        assert_eq!(row, [1001.0], "gen0158: row");
    }

    /// The rows of `InvertPointNearSingularity` WR-57's retirement moved (#17856): each
    /// case's name, main's row (3740d2254) and the new row (`[status, reason, u, v]`, or
    /// `[status]`).
    #[rustfmt::skip]
    const WR57_MOVED: &[(&str, &[f64], &[f64])] = &[
        ("gen0014", &[0.0, 1.0, 0.0, 0.3333333333333333], &[1001.0]),
        ("gen0047", &[0.0, 1.0, 0.9066324340674627, 0.5394037365288399], &[0.0, 1.0, 0.9066324340699695, 0.5394037496452493]),
        ("gen0053", &[1001.0], &[0.0, 1.0, -0.7956794078729252, 0.0]),
        ("gen0074", &[1001.0], &[0.0, 1.0, -0.07409559152995882, 0.7883623775104649]),
        ("gen0105", &[1001.0], &[0.0, 1.0, 0.5369190333955278, 0.0]),
        ("gen0158", &[0.0, 1.0, 0.12489462218132177, 0.9906255839782234], &[1001.0]),
        ("gen0172", &[1001.0], &[0.0, 1.0, 0.9375372909889319, 1.6784421551462283]),
        ("gen0181", &[0.0, 1.0, 0.0, 0.5555555555555556], &[1001.0]),
        ("gen0188", &[1001.0], &[0.0, 1.0, 1.0, 0.5538509292645661]),
        ("gen0231", &[0.0, 1.0, -0.9288773064566884, 0.4215134453827129], &[1001.0]),
        ("gen0263", &[0.0, 1.0, 4.9446034172910664, 0.19409115069965427], &[0.0, 1.0, 1.0, 0.19409115069965427]),
        ("gen0270", &[0.0, 1.0, 0.0, 0.7777777777777778], &[1001.0]),
        ("gen0305", &[0.0, 1.0, 0.0, 0.2761485602687741], &[0.0, 1.0, 0.2222222222222222, 0.27614856026877416]),
        ("gen0313", &[0.0, 1.0, 0.8490544991340391, 0.7096605998269282], &[1001.0]),
        ("gen0350", &[0.0, 1.0, 0.0, 1.0], &[1001.0]),
        ("gen0367", &[1001.0], &[0.0, 1.0, 0.6833358118884136, 1.0]),
        ("gen0386", &[0.0, 1.0, 0.9996021360682996, 0.6437453228585033], &[1001.0]),
        ("gen0424", &[0.0, 1.0, 0.007337507469351864, 0.3333333333333333], &[0.0, 1.0, 0.007337507469351885, 0.2969559706197508]),
        ("gen0455", &[0.0, 1.0, 0.015096173323598534, 0.1111111111111111], &[0.0, 1.0, 0.015096173323600607, 0.2222222222222222]),
        ("gen0480", &[0.0, 1.0, 0.4975101335583329, 0.4444444444444444], &[0.0, 1.0, 0.4975101335583329, 0.5555555555555556]),
        ("gen0522", &[0.0, 1.0, 0.9772760805607773, 0.2999881846062191], &[1001.0]),
        ("gen0560", &[0.0, 1.0, 0.0, 0.1111111111111111], &[1001.0]),
        ("gen0594", &[0.0, 1.0, 0.0, 0.1111111111111111], &[0.0, 1.0, 0.0, 0.0]),
        ("gen0598", &[1001.0], &[0.0, 1.0, 0.9741626315171246, -0.12830170427069731]),
        ("gen0623", &[0.0, 1.0, 0.9235918569506598, 4.198448818477761], &[0.0, 1.0, 0.92359185695066, 0.45930453789140524]),
        ("gen0625", &[0.0, 1.0, 11.505289406562873, 0.8706963713055447], &[0.0, 1.0, 1.0, 0.8706963713055447]),
        ("gen0677", &[0.0, 1.0, 16.622753413247768, 0.34604919278209584], &[0.0, 1.0, 1.0, 0.34604919278209584]),
        ("gen0725", &[0.0, 1.0, 1.0, 0.07810518260728934], &[0.0, 1.0, 0.9957032996868314, 0.07810518260728921]),
        ("gen0726", &[0.0, 1.0, 6.244028325018639, 0.8155595818260026], &[0.0, 1.0, 0.4921644857675187, 0.8155595818260023]),
        ("gen0775", &[0.0, 1.0, 0.08302362587299912, 2.037560255903], &[0.0, 1.0, 0.08302362587299913, 0.13584054917843388]),
        ("gen0781", &[0.0, 1.0, 10.934484812069021, 0.7176821150238701], &[0.0, 1.0, 1.0, 0.7176821150238701]),
        ("gen0816", &[0.0, 1.0, 2.5356010619296665, 1.0], &[0.0, 1.0, 1.3156639158049133, 0.3600495743950692]),
        ("gen0858", &[1001.0], &[0.0, 1.0, 0.011799220179244839, 0.0]),
        ("gen0887", &[0.0, 1.0, 0.1605075345008261, 7.396434227128571], &[0.0, 1.0, 0.1605075345008261, 1.0566334610183672]),
        ("gen0903", &[0.0, 1.0, 8.623782752222228, 0.2453309118051222], &[0.0, 1.0, 0.5077514536779808, 0.24533091180512226]),
        ("gen0962", &[1001.0], &[0.0, 1.0, 1.9594815675856745, 1.0]),
        ("gen0963", &[0.0, 1.0, 0.013105740496176483, 4.9591047726717585], &[0.0, 1.0, 0.013105740496176485, 0.0]),
        ("gen0974", &[0.0, 1.0, 0.0, 0.3333333333333333], &[1001.0]),
        ("gen1001", &[1001.0], &[0.0, 1.0, 0.9906494367092904, 1.841687344022744]),
        ("gen1023", &[1001.0], &[0.0, 1.0, -1.701240783902778, -0.5600289912404222]),
        ("gen1065", &[1001.0], &[0.0, 1.0, 0.26723201799703267, 1.0]),
        ("gen1071", &[0.0, 1.0, 0.011850340167537055, 0.2222222222222222], &[0.0, 1.0, 0.011850340167540507, 0.1111111111111111]),
        ("gen1123", &[0.0, 1.0, 0.024550142943931222, 0.5555555555555556], &[0.0, 1.0, 0.024550142943932173, 0.6666666666666666]),
        ("gen1128", &[0.0, 1.0, 0.0, 0.1111111111111111], &[1001.0]),
        ("gen1131", &[0.0, 1.0, 1.4112457664347887e-16, -1.0537632839133653], &[1001.0]),
        ("gen1138", &[0.0, 1.0, 7.135077433825912, 0.7257070461571425], &[0.0, 1.0, 0.5007015278013288, 0.7257070461571427]),
        ("gen1152", &[0.0, 1.0, 0.504635940702727, 0.2306637224781882], &[1001.0]),
        ("gen1178", &[0.0, 1.0, 2.314920805189961, -0.3381767038753387], &[1001.0]),
        ("gen1183", &[0.0, 1.0, 1.5573592017675013, 0.885123558981613], &[1001.0]),
        ("gen1246", &[1001.0], &[0.0, 1.0, -0.3446844117534995, 2.4351420286874634]),
        ("gen1257", &[0.0, 1.0, 4.166142441029816, 0.261747880000477], &[0.0, 1.0, 0.4878583181380995, 0.261747880000477]),
        ("gen1298", &[0.0, 1.0, 0.033334114380528836, 0.8888888888888888], &[0.0, 1.0, 0.03333411438052887, 0.6666666666666666]),
        ("gen1304", &[1001.0], &[0.0, 1.0, 1.1324709099809074, 0.899876532946227]),
        ("gen1400", &[1001.0], &[0.0, 1.0, 0.9016104159623767, 2.4381581600504143]),
        ("gen1414", &[1001.0], &[0.0, 1.0, -1.7939064783174012, 1.0]),
        ("gen1486", &[0.0, 1.0, 0.4444444444444444, 0.0], &[1001.0]),
        ("gen1510", &[1001.0], &[0.0, 1.0, 3.1179564298330127, 0.0]),
        ("gen1511", &[0.0, 1.0, 2.7462086273793243, 0.7716294691231871], &[1001.0]),
        ("gen1545", &[0.0, 1.0, 2.5113871811481134, 0.9166749287312247], &[0.0, 1.0, 0.0, 0.9166749287312247]),
        ("gen1562", &[0.0, 1.0, -1.2344133753969428, 1.1511373258295077], &[0.0, 1.0, 0.07660168143685353, 1.0682841038601312]),
        ("gen1573", &[1001.0], &[0.0, 1.0, 1.0, 1.7813197093476114]),
        ("gen1617", &[0.0, 1.0, 0.0, 0.1111111111111111], &[1001.0]),
        ("gen1625", &[1001.0], &[0.0, 1.0, 1.9603139438558477, 0.8143256774521873]),
        ("gen1645", &[1001.0], &[0.0, 1.0, 0.9763395118843511, 1.0]),
        ("gen1668", &[0.0, 1.0, 2.3437645526109203, 0.12712715782255418], &[0.0, 1.0, 2.3517922574205494, 0.23607017682643616]),
        ("gen1710", &[0.0, 1.0, 26.221650593892594, 0.2427790871502131], &[0.0, 1.0, 1.0, 0.24277908715021304]),
        ("gen1759", &[1001.0], &[0.0, 1.0, -1.1893621250558348, 1.0]),
        ("gen1767", &[0.0, 1.0, 14.592733309543917, 0.2075963621402525], &[0.0, 1.0, 1.0, 0.2075963621402525]),
        ("gen1873", &[0.0, 1.0, 7.292630543375605, 0.6107063669597693], &[0.0, 1.0, 0.9834814920662496, 0.6107063669597693]),
        ("gen1876", &[1001.0], &[0.0, 1.0, 0.0009527643812059326, 1.0]),
        ("gen1935", &[0.0, 1.0, 1.0, 0.0], &[1001.0]),
        ("gen1958", &[1001.0], &[0.0, 1.0, 0.005735801586641001, 0.0]),
        ("gen1988", &[1001.0], &[0.0, 1.0, 0.47640873711050946, 0.0]),
        ("gen1997", &[1001.0], &[0.0, 1.0, 0.007875936036191088, 0.26077138073868533]),
    ];

    /// The certified distance from `p` to `s`'s point at `uv`: `[lo, hi]`, the evaluation's
    /// rounding ([`crate::certify::evaluation_slack_at`], grown past the knots) and the
    /// distance's own ([`solidcore::geom::certified::distance`]) charged; `None` where the
    /// evaluation is not bounded (a rational net past its knots).
    fn wr57_distance(k: &Kernel, s: &Surface, p: Vec3, uv: Point2) -> Option<(f64, f64)> {
        crate::certify::distance(k, s, p, uv)
    }

    /// A lower bound on the distance from `p` to `s` over its natural domain
    /// ([`crate::certify::split_lower_bound`]).
    fn wr57_split_lower_bound(k: &Kernel, s: &Surface, p: Vec3, want: f64) -> f64 {
        crate::certify::split_lower_bound(k, s, p, want).expect("the lower bound")
    }

    /// The nearest of 2001 samples on each edge of `dom` to `p`, with its enclosed
    /// distance ([`wr57_distance`]); `None` where no sample is enclosed.
    fn wr57_nearest_edge_point(k: &Kernel, s: &Surface, p: Vec3, dom: &Extent2d) -> Option<((f64, f64), Point2)> {
        let (u0, v0, u1, v1) = (dom.min().x, dom.min().y, dom.max().x, dom.max().y);
        let mut best: Option<((f64, f64), Point2)> = None;
        for i in 0..=2000_u32 {
            let t = f64::from(i) / 2000.0;
            for q in [
                Point2::new(u0, v0 + (v1 - v0) * t),
                Point2::new(u1, v0 + (v1 - v0) * t),
                Point2::new(u0 + (u1 - u0) * t, v0),
                Point2::new(u0 + (u1 - u0) * t, v1),
            ] {
                if let Some(d) = wr57_distance(k, s, p, q) {
                    if best.is_none_or(|(b, _)| d.1 < b.1) {
                        best = Some((d, q));
                    }
                }
            }
        }
        best
    }

    /// WR-57 (#17856): every row of `InvertPointNearSingularity` that moved from main is
    /// replayed (the code checked is the code that blessed: the row agrees with the golden)
    /// and certified with enclosed distances from the test point:
    /// - answered in both: the new answer is not proven farther than main's (its lower end
    ///   at most main's upper end); the rows where the enclosures part (proven nearer) are
    ///   counted and the count asserted, the rest are ties within the rounding;
    /// - refused by main (1001), answered now: the new answer is not proven farther than
    ///   the guess, the acceptance rule of the search. That is not a proof that it is the
    ///   closest point (ARGUED): the surface's certified lower bound is printed per row, and
    ///   the rows where it is within `1e-6` relative of the answer's upper end (the answer
    ///   proven the closest point to that) are counted and the count asserted;
    /// - answered by main, refused now: main's answer is proven farther than the guess
    ///   (its lower end above the guess's upper end), so main returned an answer worse
    ///   than the starting point it was asked to improve.
    ///
    /// A row whose answer (or guess) lies past a rational net's knots has no enclosure
    /// ([`wr57_distance`]): it is listed as unproven, and the list asserted.
    #[test]
    fn wr57_moved_inversions_are_certified() {
        let k = Kernel::default();
        let (mut proven_nearer, mut ties, mut globally_nearest, mut argued) = (0usize, 0usize, 0usize, 0usize);
        let (mut refused_farther_than_guess, mut refused_off_a_pole) = (0usize, 0usize);
        let mut unproven: Vec<&str> = Vec::new();
        for &(name, main, new) in WR57_MOVED {
            let case = crate::case::regression_case(&Runner, name);
            let i: Inputs = case.parse().expect("inputs");
            let row = rust_run(&i).unwrap_or_else(|_| panic!("{name}: rust"));
            // the goldens' agreement (`crate::REL_TOL`): a bless keeps a row within it
            assert!(
                row.len() == new.len()
                    && row.iter().zip(new).all(|(a, b)| (a - b).abs() <= crate::REL_TOL * a.abs().max(b.abs())),
                "{name}: the code gives {row:?}, not the new row"
            );
            let OpIn::Invert { p, uv, dom, .. } = &i.op else {
                panic!("{name}: not an invert case");
            };
            let s = surface_of(&k, &i.surface).expect("surface").expect("a surface");
            let at = |r: &[f64]| wr57_distance(&k, &s, v3(*p), Point2::new(r[2], r[3]));
            let guess = wr57_distance(&k, &s, v3(*p), p2(*uv));
            match (main.len(), new.len()) {
                (4, 4) => {
                    let (Some(m), Some(n)) = (at(main), at(new)) else {
                        unproven.push(name);
                        eprintln!("{name}: unproven (past a rational net's knots), main {main:?} new {new:?}");
                        continue;
                    };
                    assert!(n.0 <= m.1, "{name}: new {n:?} proven farther than main {m:?}");
                    if n.1 < m.0 {
                        proven_nearer += 1;
                    } else {
                        ties += 1;
                    }
                    eprintln!("{name}: both answered, main {m:?} new {n:?}");
                }
                (1, 4) => {
                    let (Some(n), Some(guess)) = (at(new), guess) else {
                        unproven.push(name);
                        eprintln!("{name}: unproven (past a rational net's knots), new {new:?}");
                        continue;
                    };
                    assert!(n.0 <= guess.1, "{name}: new {n:?} proven farther than the guess {guess:?}");
                    let global = crate::certify::nearest(&k, &s, None, v3(*p), Point2::new(new[2], new[3]));
                    // (where the whole surface's bound stops short, the bound over a
                    // grid of boxes, each its own subdivision: the least of them)
                    let lo = if global.hi - global.lo <= 1e-6 * global.hi {
                        global.lo
                    } else {
                        wr57_split_lower_bound(&k, &s, v3(*p), global.hi)
                    };
                    if global.hi - lo <= 1e-6 * global.hi {
                        globally_nearest += 1;
                    } else {
                        argued += 1;
                    }
                    eprintln!("{name}: answered now, new {n:?} guess {guess:?} surface's lower bound {lo}");
                }
                (4, 1) => {
                    let (Some(m), Some(guess)) = (at(main), guess) else {
                        unproven.push(name);
                        eprintln!("{name}: unproven (past a rational net's knots), main {main:?}");
                        continue;
                    };
                    if m.0 > guess.1 {
                        refused_farther_than_guess += 1;
                        eprintln!("{name}: refused now, main {m:?} proven farther than the guess {guess:?}");
                    } else {
                        // (main's answer on a boundary that is not a pole: a point of the
                        // inversion domain's edges proven nearer, found by sampling them;
                        // a sampled point found nearer is a proof, its absence none)
                        let edge = wr57_nearest_edge_point(&k, &s, v3(*p), &dom_of(*dom));
                        assert!(
                            edge.is_some_and(|(e, _)| e.1 < m.0),
                            "{name}: main {m:?} proven neither farther than the guess {guess:?} nor than an edge point {edge:?}"
                        );
                        refused_off_a_pole += 1;
                        eprintln!("{name}: refused now, main {m:?} proven farther than the edge point {edge:?}");
                    }
                }
                _ => panic!("{name}: an unexpected transition {main:?} -> {new:?}"),
            }
        }
        eprintln!(
            "{proven_nearer} answers proven nearer than main's, {ties} ties; {globally_nearest} answers \
             now proven the closest point, {argued} not farther than the guess only (ARGUED); refused: \
             {refused_farther_than_guess} main proven farther than the guess, {refused_off_a_pole} than an edge \
             point; unproven {unproven:?}"
        );
        assert_eq!(
            (
                proven_nearer,
                ties,
                globally_nearest,
                argued,
                refused_farther_than_guess,
                refused_off_a_pole,
                unproven.as_slice()
            ),
            WR57_COUNTS,
            "the certified counts moved"
        );
    }

    /// [`wr57_moved_inversions_are_certified`]'s counts: proven nearer, ties, answered now
    /// and proven the closest point, answered now and not farther than the guess only, and
    /// the refused rows where main's answer is proven farther than the guess and where it is
    /// proven farther than a point of the inversion domain's edges (gen0270, whose answer
    /// lies on an edge that is not a pole), and the rows without an enclosure. The tie is
    /// gen1668 (the enclosures an ulp apart). gen0074's answer, 1.1e-4 from the domain's
    /// `u_min` in a narrow valley, is proven the closest point by the split bound
    /// ([`wr57_split_lower_bound`]: 0.2309564528332 against its 0.2309564528361; the whole
    /// surface's subdivision stops at 0.2260). gen1191 is main's refusal again (#18052).
    const WR57_COUNTS: (usize, usize, usize, usize, usize, usize, &[&str]) = (28, 1, 25, 0, 19, 1, &[]);

    /// Generated cases reach every op, and the comparable ones mostly compare values.
    #[test]
    fn generated_cases_are_diverse() {
        let mut stats: BTreeMap<String, [usize; 4]> = BTreeMap::new();
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p8", i));
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let i: Inputs = serde_json::from_value(v).unwrap();
            let name = serde_json::to_value(&i.op).unwrap()["op"]
                .as_str()
                .unwrap()
                .to_string();
            let e = stats.entry(name).or_default();
            e[0] += 1;
            match rust_run(&i) {
                Ok(o) if o.first() == Some(&0.0) => e[1] += 1,
                Ok(_) => e[2] += 1,
                Err(_) => e[3] += 1,
            }
        }
        eprintln!("{stats:?}");
        assert_eq!(stats.len(), 8);
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
struct BssIn {
    #[serde(default)]
    oob: bool,
    #[serde(default = "default_form")]
    form: u8,
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

fn default_form() -> u8 {
    BSplineSurfaceForm::Unspecified as u8
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum CrvIn {
    Bspline {
        dim: u64,
        cpts: Vec<[f64; 4]>,
        p: usize,
        u: Vec<f64>,
    },
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
}

fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_points(Point2::new(d[0], d[1]), Point2::new(d[2], d[3]))
}

fn ivl_of(d: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(d[0], d[1])
}

/// The curve, `None` when its construction fails (status `-3`).
fn curve_of(c: &CrvIn) -> Result<Option<SmCurve>, HarnessError> {
    Ok(match c {
        CrvIn::Line { a, b } => Line::create_line_segment(3, v3(*a), v3(*b), None)
            .ok()
            .map(SmCurve::from),
        CrvIn::Bspline { dim, cpts, p, u } => {
            let pw = cpts.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect();
            let cur =
                Curve::new(pw, *p, u.clone()).map_err(|e| harness(format!("curve input: {e}")))?;
            Some(SmCurve::from(BSplineCurve::from_nurb(cur, *dim, false)))
        }
    })
}

fn cont_of(v: u8) -> Result<ContinuityType, HarnessError> {
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

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    crate::status::kind_slot(e, legacy_status(e))
}

/// [`status`] as recorded before kinds.
fn legacy_status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, else a harness error.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(v)` or the status; `Unported` defers the case.
fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(Err(status(&e))),
        },
    }
}

fn pv(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn gen_knots(rng: &mut Rng, p: usize, n: usize, kink: bool) -> Vec<f64> {
    let interior = n - p;
    let mut inner: Vec<f64> = (0..interior).map(|_| rng.uniform(0.05, 0.95)).collect();
    inner.sort_by(f64::total_cmp);
    if kink && interior >= p && p >= 1 {
        // a knot of full multiplicity p (a C0 joint)
        let a = inner.first().copied().unwrap_or(0.5);
        for x in inner.iter_mut().take(p) {
            *x = a;
        }
        inner.sort_by(f64::total_cmp);
    } else if rng.chance(0.2) && interior >= 2 && p >= 2 {
        if let Some(&a) = inner.first() {
            if let Some(b) = inner.get_mut(1) {
                *b = a;
            }
        }
        inner.sort_by(f64::total_cmp);
    }
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

/// The surface shapes the generator makes.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Shape {
    Bumpy,
    Flat,
    /// The `u = min` row collapsed to one point (a pole).
    Pole,
    /// Both `u` end rows collapsed (a single span with poles at both ends).
    TwoPoles,
    /// A crease along an interior `u` knot (C0 in u, the iso curves kinked).
    Crease,
    /// A smooth surface with a C0 knot whose control points stay collinear (an iso-curve
    /// kink that the normals show smooth).
    Herringbone,
}

fn scale_of(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        // Not 1e-4: the surface cache's tolerances are absolute, and a surface a few 1e-4
        // across needs about 2^20 leaves per patch (HANG-?: 15 s in the C++, 120 s in the
        // port, for one `GlobalCurveIntersect`).
        0 => 1e-3,
        1 => 1e4,
        2..=5 => rng.uniform(0.1, 50.0),
        _ => 1.0,
    }
}

fn gen_bss(rng: &mut Rng, shape: Shape) -> BssIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let kink = matches!(shape, Shape::Crease | Shape::Herringbone);
    let n = if shape == Shape::TwoPoles {
        p
    } else if kink {
        2 * p + rng.below(2) as usize
    } else {
        p + rng.below(4) as usize
    };
    let m = q + rng.below(3) as usize;
    let amp = if shape == Shape::Flat {
        *rng.choose(&[0.0, 0.0, 1e-9, 1e-4]).unwrap_or(&0.0)
    } else if shape == Shape::Herringbone {
        0.0
    } else {
        rng.uniform(0.0, 1.0)
    };
    let rational = rng.chance(0.15);
    let scale = scale_of(rng);
    let mut net: Vec<Vec<[f64; 3]>> = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let jit = if shape == Shape::Herringbone { 0.0 } else { 0.2 };
                    let x = i as f64 + rng.uniform(-jit, jit);
                    let y = j as f64 + rng.uniform(-jit, jit);
                    let z = amp * rng.uniform(-1.0, 1.0);
                    [x * scale, y * scale, z * scale]
                })
                .collect()
        })
        .collect();
    match shape {
        Shape::Pole | Shape::TwoPoles => {
            let apex = [-scale, m as f64 * scale / 2.0, rng.uniform(-0.5, 0.5) * scale];
            if let Some(row) = net.first_mut() {
                for c in row.iter_mut() {
                    *c = apex;
                }
            }
            if shape == Shape::TwoPoles {
                let apex2 = [(n as f64 + 1.0) * scale, m as f64 * scale / 2.0, 0.0];
                if let Some(row) = net.last_mut() {
                    for c in row.iter_mut() {
                        *c = apex2;
                    }
                }
            }
        }
        Shape::Crease => {
            // lift the row at the C0 knot: a ridge
            if let Some(row) = net.get_mut(p) {
                for c in row.iter_mut() {
                    c[2] += scale * rng.uniform(0.5, 2.0);
                }
            }
        }
        Shape::Bumpy | Shape::Flat | Shape::Herringbone => {}
    }
    let net = net
        .into_iter()
        .map(|row| {
            row.into_iter()
                .map(|[x, y, z]| {
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
    BssIn {
        oob: rng.chance(0.05),
        form: if rng.chance(0.9) { 11 } else { rng.below(10) as u8 },
        net,
        p,
        q,
        u: gen_knots(rng, p, n, kink),
        v: gen_knots(rng, q, m, false),
    }
}

fn gen_plane(rng: &mut Rng) -> SrfIn {
    let mut n = rng.point3(-1.0, 1.0);
    if n.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
        n = [0.0, 0.0, 1.0];
    }
    if rng.chance(0.3) {
        n = *rng
            .choose(&[[0.0, 0.0, 1.0], [1.0, 0.0, 0.0], [0.0, -1.0, 0.0]])
            .unwrap_or(&[0.0, 0.0, 1.0]);
    }
    SrfIn::Plane {
        o: rng.point3(-5.0, 5.0),
        n,
    }
}

/// A revolution of a line coplanar with the axis (a cylinder or a cone, sometimes with its
/// apex on the axis).
fn gen_revolution(rng: &mut Rng) -> SrfIn {
    let axis = *rng
        .choose(&[[0.0, 0.0, 1.0], [0.0, 1.0, 0.0], [1.0, 1.0, 0.5]])
        .unwrap_or(&[0.0, 0.0, 1.0]);
    let p = rng.point3(-2.0, 2.0);
    // a radial direction perpendicular to the axis
    let radial = if axis[0] == 0.0 && axis[1] == 0.0 {
        [1.0, 0.0, 0.0]
    } else if axis[0] == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        [1.0, -1.0, 0.0]
    };
    // some touch the axis at one end (a cone apex: a collapsed row of the surface)
    let apex = rng.chance(0.2);
    let apex_first = rng.chance(0.5);
    let mut r0 = rng.uniform(0.5, 3.0);
    let mut r1 = if rng.chance(0.5) { r0 } else { rng.uniform(0.5, 3.0) };
    if apex {
        if apex_first {
            r0 = 0.0;
        } else {
            r1 = 0.0;
        }
    }
    let h0 = rng.uniform(-2.0, 0.0);
    let h1 = rng.uniform(0.5, 3.0);
    let at = |r: f64, h: f64| {
        [
            p[0] + r * radial[0] + h * axis[0],
            p[1] + r * radial[1] + h * axis[1],
            p[2] + r * radial[2] + h * axis[2],
        ]
    };
    SrfIn::Revolution {
        a: at(r0, h0),
        b: at(r1, h1),
        p,
        axis,
        swap: rng.chance(0.3),
    }
}


/// The natural domain of a generated surface (the unit square when unknown).
fn natural_domain(s: &SrfIn) -> [f64; 4] {
    match s {
        SrfIn::Bspline(b) => {
            let (u0, u1) = (b.u.first().copied().unwrap_or(0.0), b.u.last().copied().unwrap_or(1.0));
            let (v0, v1) = (b.v.first().copied().unwrap_or(0.0), b.v.last().copied().unwrap_or(1.0));
            [u0, v0, u1, v1]
        }
        SrfIn::Plane { .. } => [-1.0, -1.0, 1.0, 1.0],
        SrfIn::Revolution { .. } => [0.0, 0.0, 1.0, 1.0],
    }
}

/// A sub-domain of `d`: the whole, a random part, or a thin sliver.
fn gen_subdomain(rng: &mut Rng, d: [f64; 4]) -> [f64; 4] {
    let [u0, v0, u1, v1] = d;
    match rng.below(4) {
        0 => d,
        1 => {
            let a = u0 + (u1 - u0) * rng.uniform(0.0, 0.4);
            let b = u0 + (u1 - u0) * rng.uniform(0.6, 1.0);
            let c = v0 + (v1 - v0) * rng.uniform(0.0, 0.4);
            let e = v0 + (v1 - v0) * rng.uniform(0.6, 1.0);
            [a, c, b, e]
        }
        2 => {
            let a = u0 + (u1 - u0) * rng.uniform(0.0, 0.9);
            [a, v0, a + (u1 - u0) * 1e-7, v1]
        }
        _ => {
            let c = v0 + (v1 - v0) * rng.uniform(0.0, 0.5);
            [u0, c, u1, v1]
        }
    }
}

/// A curve; `lines` allows an `SmLine` (not as a blend rail: see the FFI guard).
fn gen_curve(rng: &mut Rng, scale: f64, lines: bool) -> (CrvIn, [f64; 2]) {
    if lines && rng.chance(0.3) {
        let a = rng.point3(-2.0 * scale, 2.0 * scale);
        let mut bb = rng.point3(-2.0 * scale, 2.0 * scale);
        if rng.chance(0.1) {
            bb = [a[0] + scale, a[1], a[2]];
        }
        let len = ((bb[0] - a[0]).powi(2) + (bb[1] - a[1]).powi(2) + (bb[2] - a[2]).powi(2)).sqrt();
        let hi = if rng.chance(0.5) { len } else { len * rng.uniform(0.2, 1.0) };
        return (CrvIn::Line { a, b: bb }, [0.0, hi.max(1e-3)]);
    }
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let degenerate = rng.chance(0.05);
    let base = rng.point3(-scale, scale);
    let cpts = (0..=n)
        .map(|_| {
            let q = if degenerate { base } else { rng.point3(-2.0 * scale, 2.0 * scale) };
            [q[0], q[1], q[2], NOW]
        })
        .collect();
    let u = gen_knots(rng, p, n, false);
    let (a, b) = (u.first().copied().unwrap_or(0.0), u.last().copied().unwrap_or(1.0));
    let ivl = if rng.chance(0.6) {
        [a, b]
    } else {
        let x = a + (b - a) * rng.uniform(0.0, 0.4);
        [x, x + (b - a) * rng.uniform(0.2, 0.6)]
    };
    (
        CrvIn::Bspline {
            dim: 3,
            cpts,
            p,
            u,
        },
        ivl,
    )
}
