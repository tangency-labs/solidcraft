//! Runner for unit `surface.surface_p8`: `SmSurface` part 8 (the knots below a continuity,
//! the coincidence check, the blend surface, the global line and curve intersections, the
//! local line intersection, the point inversion near a singularity and
//! `SmSPGlobalSolver::LocalSolve`).
//!
//! C++: `solidcore_ffi::units::surface_surface_p8` (upstream `source/SMLib/src/SmSurface.cpp`,
//! called through `SmSurface*` on B-spline surfaces, planes and surfaces of revolution).
//! Rust: `solidcore::surface::surface::p8`.
//!
//! A case is one `op` on a `surface` (`{"bspline": {oob, form, net, p, q, u, v}}` with control
//! points as stored `[x, y, z, w]`, rows in u; `{"plane": {o, n}}`; or `{"revolution": {a, b, p,
//! axis, swap}}`). Both sides produce the flat record documented in the ffi module (status
//! first); the runner reports it as the `out` values. A Rust error maps to its `SmStatus`
//! (`1007` for `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the
//! case.
//!
//! The statics `sm_ExtractIsoCurve` and `sm_IntersectSrfBndryCrv` are not reachable from C++
//! (their callers are unported); the Rust unit tests cover them.

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
use solidcore_ffi::units::surface_surface_p8::{self as ffi, Op};

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

// ---- C++ side -----------------------------------------------------------------------

fn push_srf(o: &mut Vec<f64>, s: &SrfIn) -> Result<(), HarnessError> {
    match s {
        SrfIn::Revolution {
            a,
            b: bb,
            p,
            axis,
            swap,
        } => {
            o.push(3.0);
            o.extend_from_slice(a);
            o.extend_from_slice(bb);
            o.extend_from_slice(p);
            o.extend_from_slice(axis);
            o.push(b(*swap));
        }
        SrfIn::Plane { o: org, n } => {
            o.push(1.0);
            o.extend_from_slice(org);
            o.extend_from_slice(n);
        }
        SrfIn::Bspline(bs) => {
            let (n, m) = shape(bs)?;
            o.extend([0.0, b(bs.oob), f64::from(bs.form)]);
            o.extend(
                [
                    n,
                    m,
                    bs.p,
                    bs.q,
                    bs.u.len().saturating_sub(1),
                    bs.v.len().saturating_sub(1),
                ]
                .map(|x| x as f64),
            );
            for row in &bs.net {
                for p in row {
                    o.extend_from_slice(p);
                }
            }
            o.extend_from_slice(&bs.u);
            o.extend_from_slice(&bs.v);
        }
    }
    Ok(())
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    push_srf(&mut v, &i.surface)?;
    let op = match &i.op {
        OpIn::Knots { cont, at } => {
            v.extend([f64::from(*cont), b(*at)]);
            Op::Knots
        }
        OpIn::Coinc {
            same,
            other,
            tol,
            whole,
        } => {
            v.push(b(*same));
            push_srf(&mut v, other)?;
            v.extend([*tol, b(*whole)]);
            Op::Coinc
        }
        OpIn::Line {
            dom,
            p,
            v: vec,
            ivl,
            ray,
            tol,
        } => {
            v.extend(dom);
            v.extend(p);
            v.extend(vec);
            v.push(b(ivl.is_some()));
            v.extend(ivl.unwrap_or([0.0, 0.0]));
            v.extend([b(*ray), *tol]);
            Op::Line
        }
        OpIn::Curve {
            dom,
            curve,
            ivl,
            tol,
        } => {
            v.extend(dom);
            push_crv(&mut v, curve);
            v.extend(ivl);
            v.push(*tol);
            Op::Curve
        }
        OpIn::LocalLine {
            dom,
            p,
            v: vec,
            ivl,
            dtol,
            atol,
            uv,
        } => {
            v.extend(dom);
            v.extend(p);
            v.extend(vec);
            v.extend(ivl);
            v.extend([*dtol, *atol]);
            v.extend(uv);
            Op::LocalLine
        }
        OpIn::Invert { p, dom, uv, tol } => {
            v.extend(p);
            v.extend(dom);
            v.extend(uv);
            v.push(*tol);
            Op::Invert
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
            push_crv(&mut v, curve1);
            v.extend(ivl1);
            v.push(b(*right));
            push_srf(&mut v, other)?;
            push_crv(&mut v, curve2);
            v.extend(ivl2);
            v.extend([b(*left), *tol]);
            Op::Blend
        }
        OpIn::SpLocal {
            dom,
            p,
            sop,
            tol,
            best_sq,
        } => {
            v.extend(dom);
            v.extend(p);
            v.extend([f64::from(*sop), *tol, *best_sq]);
            Op::SpLocal
        }
    };
    Ok((op, v))
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i)?;
    let out = ffi::eval(op, &input).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- Rust side ----------------------------------------------------------------------

/// The surface, `None` when its construction fails (the shim's `-3`).
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
/// natural domain).
#[derive(Debug, Clone, Copy, PartialEq)]
enum Obj {
    Curve([f64; 2]),
    Surface([f64; 4]),
}

impl SolutionObject for Obj {
    fn kind(&self) -> ObjectKind {
        match self {
            Obj::Curve(_) => ObjectKind::Curve,
            Obj::Surface(_) => ObjectKind::Surface,
        }
    }

    fn natural_interval(&self) -> Option<Extent1d> {
        match self {
            Obj::Curve(i) => Some(ivl_of(*i)),
            Obj::Surface(_) => None,
        }
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        match self {
            Obj::Surface(d) => Some(dom_of(*d)),
            Obj::Curve(_) => None,
        }
    }
}

fn surface_obj(s: &Surface) -> Obj {
    let d = s.natural_uv_domain();
    Obj::Surface([d.min().x, d.min().y, d.max().x, d.max().y])
}

fn curve_obj(c: &SmCurve) -> Obj {
    let i = c.natural_interval();
    Obj::Curve([i.min(), i.max()])
}

/// The shim's solution records (objects `-1` NULL, `0` the curve, `1` the surface).
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
                    o.extend([0.0, u.len() as f64]);
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
                Ok(c) => o.extend([0.0, b(c.are_coincident), c.max_distance]),
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
            let (co, so) = (curve_obj(&c), surface_obj(&s));
            match st(s.global_curve_intersect::<Obj>(
                &k,
                [co, so],
                &dom_of(*dom),
                &c,
                &ivl_of(*ivl),
                *tol,
            ))? {
                Ok(sols) => {
                    o.push(0.0);
                    put_solutions(&mut o, &sols, Some(co), so);
                }
                Err(c) => o.push(c),
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
                Ok(r) => o.extend([0.0, f64::from(r.reason as i32), r.uv.x, r.uv.y]),
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
                    let nd = blend.natural_uv_domain();
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
                s.natural_uv_domain(),
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
            let Ok((op, input)) = cpp_input(&i) else {
                continue;
            };
            if ffi::validate(op, &input).is_err() {
                continue;
            }
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

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse()?;
        // Run only what the FFI accepts, as the C++ side: its guards refuse inputs
        // upstream (and the faithful port) never finish on (fuzz Rust-side hangs).
        let (code, input) = cpp_input(&i)?;
        ffi::validate(code, &input).map_err(|e| HarnessError::new(format!("c++: {e}")))?;
        rust(&i)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

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

fn shape(s: &BssIn) -> Result<(usize, usize), HarnessError> {
    let n = s.net.len().checked_sub(1).ok_or_else(|| harness("empty net"))?;
    let cols = s.net.first().map_or(0, Vec::len);
    let m = cols.checked_sub(1).ok_or_else(|| harness("empty net row"))?;
    if s.net.iter().any(|r| r.len() != cols) {
        return Err(harness("net rows differ in length"));
    }
    Ok((n, m))
}

fn push_crv(o: &mut Vec<f64>, c: &CrvIn) {
    match c {
        CrvIn::Line { a, b } => {
            o.push(1.0);
            o.extend_from_slice(a);
            o.extend_from_slice(b);
        }
        CrvIn::Bspline { dim, cpts, p, u } => {
            let n = cpts.len().saturating_sub(1);
            let m = u.len().saturating_sub(1);
            o.extend([0.0, *dim as f64, n as f64, *p as f64, m as f64]);
            for x in cpts {
                o.extend_from_slice(x);
            }
            o.extend_from_slice(u);
        }
    }
}

fn dom_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_points(Point2::new(d[0], d[1]), Point2::new(d[2], d[3]))
}

fn ivl_of(d: [f64; 2]) -> Extent1d {
    Extent1d::from_min_max(d[0], d[1])
}

/// The curve, `None` when its construction fails (the shim's `-3`).
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
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
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
