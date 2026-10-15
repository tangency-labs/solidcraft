//! Runner for unit `surface.surface_point_solve_p1`: the problem data, global-solver
//! helpers and local-stepper pieces of `SmSurfacePointSolve.cpp` the unit owns.
//!
//! C++: `solidcore_ffi::units::surface_surface_point_solve_p1` (the shim compiles the
//! upstream file, whose classes are file-local). Rust:
//! `solidcore::surface::surface_point_solve::p1`.
//!
//! A case is a surface `surf` (a B-spline, a plane or a curve-bounded surface, wrapped in
//! a STEP surface when `wrap`), the problem (solver domain `dom`, operation `solver`
//! as an `SmSolverOperationType` code, test point `test`, tolerance `tol`) and an `op`
//! with its parameters. Both sides produce the flat record documented in the ffi
//! module; the runner reports its first entry as the `status` (`0` = `SM_SUCCESS`, else
//! the `SmStatus`; `1007` for an index past the control net, a Rust `InvalidInput`;
//! `-1` for a Rust error upstream cannot produce) and the rest as the `out` values.
//!
//! Cases: inline cases on the upstream `Surface.sms` with the test points of
//! `my_test_surface_closest_point` (`tests/prog_test/src/smSurfaceTest.cpp:4117`), and
//! generated cases (planes, B-splines incl. closed, rational, bilinear, collapsed and
//! huge / tiny ones, curve-bounded and STEP-wrapped surfaces; sub-domains; test points on,
//! near and far from the surface).
//!
//! Deferred cases: `SmSurface::IsOnSeam` and `SmSurfaceEval::Normal`
//! (`surface.surface_p4`), the base `CreateIsoParametricCurve` (`surface.surface_p5`),
//! and every surface cache build of a curve-bounded or STEP surface's bounding box
//! (`SmSurface::CalculateContinuities`, `surface.surface_p6`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::global_solver::{SolutionRequested, SolverOperation};
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Point3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::math::NOW;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::surface::surface::p1::StepSurface;
use solidcore::surface::surface::p2::SurfaceEval;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_point_solve::p1::{
    triangles_dist, well_behaved_control_net, SurfPtGlobalSolver, SurfPtSolveStepper,
};
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_point_solve_p1::{self as ffi, Op, MAX_HIT_STEPS};

use super::surface_surface_cache_p1::{
    default_form, dom_of, flag, gen_knots, gen_sub_dom, gvec, harness, push3, push_dom, srf, status, v3,
    BssIn, InlineSrf, SrfIn,
};
use super::surface_surface_cache_p3::{gen_surf, push_surf, surf_domain, surf_size, surface_in, SurfIn};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 900;

/// `surface.surface_point_solve_p1` runner.
pub struct Runner;

// ---- inputs -----------------------------------------------------------------------

/// One `HitBound` step: move to `uv`, then hit `v` (else u) towards `tob`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct HitIn {
    uv: [f64; 2],
    #[serde(default)]
    v: bool,
    tob: i32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Ctor,
    Closed,
    Func {
        uv: [f64; 2],
    },
    TriGuess {
        iu: usize,
        iv: usize,
    },
    Leg {
        iu: usize,
        iv: usize,
    },
    Stride {
        uv: [f64; 2],
    },
    SeamsUv {
        uv: [f64; 2],
    },
    SeamsCpts {
        iu: usize,
        iv: usize,
    },
    Crease,
    Local {
        uv: [f64; 2],
        ltol: f64,
    },
    TriDist {
        i: usize,
        j: usize,
    },
    Well {
        i: usize,
        j: usize,
    },
    Stepper {
        uv: [f64; 2],
        tight: f64,
        uv2: [f64; 2],
    },
    HitBound {
        uv: [f64; 2],
        steps: Vec<HitIn>,
    },
    CheckBounds {
        curr: [f64; 2],
        next: [f64; 2],
        bdom: [f64; 4],
    },
    DropIso {
        uv: [f64; 2],
        #[serde(default)]
        v: bool,
        crease: f64,
        param: f64,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Ctor => Op::Ctor,
            OpIn::Closed => Op::Closed,
            OpIn::Func { .. } => Op::Func,
            OpIn::TriGuess { .. } => Op::TriGuess,
            OpIn::Leg { .. } => Op::Leg,
            OpIn::Stride { .. } => Op::Stride,
            OpIn::SeamsUv { .. } => Op::SeamsUv,
            OpIn::SeamsCpts { .. } => Op::SeamsCpts,
            OpIn::Crease => Op::Crease,
            OpIn::Local { .. } => Op::Local,
            OpIn::TriDist { .. } => Op::TriDist,
            OpIn::Well { .. } => Op::Well,
            OpIn::Stepper { .. } => Op::Stepper,
            OpIn::HitBound { .. } => Op::HitBound,
            OpIn::CheckBounds { .. } => Op::CheckBounds,
            OpIn::DropIso { .. } => Op::DropIso,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    /// Wrap the surface in an `SmSTEPSurface`.
    #[serde(default)]
    wrap: bool,
    surf: SurfIn,
    /// The solver domain.
    dom: [f64; 4],
    /// The `SmSolverOperationType` code.
    solver: u8,
    test: [f64; 3],
    tol: f64,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -----------------------------------------------------------------------

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = vec![flag(i.wrap)];
    push_surf(&mut o, &i.surf)?;
    o.extend(i.dom);
    o.push(f64::from(i.solver));
    o.extend(i.test);
    o.push(i.tol);
    match &i.op {
        OpIn::Ctor | OpIn::Closed | OpIn::Crease => {}
        OpIn::Func { uv } | OpIn::Stride { uv } | OpIn::SeamsUv { uv } => o.extend(uv),
        OpIn::TriGuess { iu, iv } | OpIn::Leg { iu, iv } | OpIn::SeamsCpts { iu, iv } => {
            o.extend([*iu as f64, *iv as f64]);
        }
        OpIn::TriDist { i, j } | OpIn::Well { i, j } => o.extend([*i as f64, *j as f64]),
        OpIn::Local { uv, ltol } => {
            o.extend(uv);
            o.push(*ltol);
        }
        OpIn::Stepper { uv, tight, uv2 } => {
            o.extend(uv);
            o.push(*tight);
            o.extend(uv2);
        }
        OpIn::HitBound { uv, steps } => {
            o.extend(uv);
            o.push(steps.len() as f64);
            for s in steps {
                o.extend(s.uv);
                o.push(flag(s.v));
                o.push(f64::from(s.tob));
            }
        }
        OpIn::CheckBounds { curr, next, bdom } => {
            o.extend(curr);
            o.extend(next);
            o.extend(bdom);
        }
        OpIn::DropIso {
            uv,
            v,
            crease,
            param,
        } => {
            o.extend(uv);
            o.push(flag(*v));
            o.push(*crease);
            o.push(*param);
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

pub(crate) fn wrapped_of(wrap: bool, s: &SurfIn) -> Result<Surface, HarnessError> {
    let s = surface_in(s)?;
    Ok(if wrap {
        Surface::from(StepSurface::new(s, true))
    } else {
        s
    })
}

pub(crate) fn p2(uv: [f64; 2]) -> Point2 {
    Point2::new(uv[0], uv[1])
}

pub(crate) fn push2(o: &mut Vec<f64>, v: Point2) {
    o.extend([v.x, v.y]);
}

pub(crate) fn solver_op(code: u8) -> Result<SolverOperation, HarnessError> {
    SolverOperation::ALL
        .get(usize::from(code))
        .copied()
        .ok_or_else(|| harness(format!("bad solver operation {code}")))
}

/// `status has_bspl tight loose test dir` of the problem data.
fn push_base(o: &mut Vec<f64>, b: &solidcore::surface::surface_point_solve::SurfacePointSolver<'_>) {
    o.push(0.0);
    o.push(flag(b.b_spline().is_some()));
    o.push(b.tight_tol());
    o.push(b.loose_tol());
    push3(o, b.test_point());
    push3(o, b.dir_vec());
}

/// The control net of the solver's B-spline: `(nu, nv, points, rational)`.
type Net = (usize, usize, Vec<solidcore::nurbs::math::CPoint>, bool);

fn net_of(g: &SurfPtGlobalSolver<'_>) -> Option<Net> {
    let b = g.base.b_spline()?;
    let (nu, nv, pts) = b.control_points().ok()?;
    Some((nu, nv, pts, b.is_rational()))
}

#[allow(clippy::too_many_lines)]
fn rust_op(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let k = Kernel::default();
    let surface = wrapped_of(i.wrap, &i.surf)?;
    let dom = dom_of(i.dom);
    let op = solver_op(i.solver)?;
    let test = v3(i.test);
    let tol = i.tol;
    let global = || {
        SurfPtGlobalSolver::new(
            &k,
            &surface,
            &dom,
            op,
            test,
            tol,
            None,
            SolutionRequested::Single,
        )
    };
    let stepper = |uv: [f64; 2], tight: f64| {
        SurfPtSolveStepper::new(&k, &surface, &dom, test, p2(uv), tight, tol, op)
    };
    let run = || -> solidcore::Result<Vec<f64>> {
        let mut o = Vec::new();
        match &i.op {
            OpIn::Ctor => {
                let g = global()?;
                push_base(&mut o, &g.base);
            }
            OpIn::Closed => {
                let mut g = global()?;
                o.push(0.0);
                o.push(flag(g.closed_surface(SurfParamType::U)?));
                o.push(flag(g.closed_surface(SurfParamType::V)?));
                o.push(flag(g.closed_surface(SurfParamType::U)?));
                o.push(flag(g.closed_surface(SurfParamType::Both)?));
            }
            OpIn::Func { uv } => {
                let g = global()?;
                o.push(0.0);
                o.push(g.calc_func_val(p2(*uv))?);
            }
            OpIn::TriGuess { iu, iv } => {
                let g = global()?;
                let r = g.find_polygon_triangle_guess((*iu, *iv))?;
                o.push(0.0);
                push2(&mut o, r.uv);
                o.extend([r.best_idx_u as f64, r.best_idx_v as f64]);
                o.extend([r.best_dist, r.ave_cpt_dist, flag(r.suspicious)]);
            }
            OpIn::Leg { iu, iv } => {
                let g = global()?;
                let uv = g.refine_guess_polygon_leg(*iu, *iv)?;
                o.push(0.0);
                push2(&mut o, uv);
            }
            OpIn::Stride { uv } => {
                let g = global()?;
                let mut pt0 = SurfaceEval::new(Some(&surface));
                pt0.set_uv(p2(*uv));
                let r = g.check_bad_guess_stride(&mut pt0)?;
                o.push(0.0);
                o.push(flag(r.is_some()));
                if let Some((npt, f)) = r {
                    push2(&mut o, npt.uv());
                    o.push(f);
                }
            }
            OpIn::SeamsUv { uv } => {
                let mut g = global()?;
                let r = g.check_guess_for_seams(&Kernel::default(), p2(*uv))?;
                o.push(0.0);
                push2(&mut o, r);
            }
            OpIn::SeamsCpts { iu, iv } => {
                let g = global()?;
                let Some((nu, nv, pts, rational)) = net_of(&g) else {
                    return Err(SolidError::sm_err("difftest: no control net"));
                };
                let (a, b) = g.check_guess_for_seams_cpts(*iu, *iv, nu, nv, &pts, rational)?;
                o.extend([0.0, a as f64, b as f64]);
            }
            OpIn::Crease => {
                let g = global()?;
                let list = g.crease_subdomains()?;
                o.push(0.0);
                o.push(list.len() as f64);
                for d in &list {
                    push_dom(&mut o, d);
                }
            }
            OpIn::Local { uv, ltol } => {
                let g = global()?;
                o.push(0.0);
                let mut e = SurfaceEval::new(Some(&surface));
                e.set_uv(p2(*uv));
                o.push(flag(g.is_local_min(&mut e, *ltol)?));
                let mut e = SurfaceEval::new(Some(&surface));
                e.set_uv(p2(*uv));
                o.push(flag(g.is_local_max(&mut e, *ltol)?));
                let mut e = SurfaceEval::new(Some(&surface));
                e.set_uv(p2(*uv));
                let (b, which) = g.pushing_boundary(&mut e, *ltol)?;
                o.push(flag(b));
                o.push(f64::from(which as u8));
            }
            OpIn::TriDist { i: a, j } => {
                let g = global()?;
                let Some((nu, nv, pts, rational)) = net_of(&g) else {
                    return Err(SolidError::sm_err("difftest: no control net"));
                };
                let (d, u, v) = triangles_dist(g.base.test_point(), &pts, rational, *a, *j, nu, nv)?;
                o.extend([0.0, d, u, v]);
            }
            OpIn::Well { i: a, j } => {
                let g = global()?;
                let Some((nu, nv, pts, rational)) = net_of(&g) else {
                    return Err(SolidError::sm_err("difftest: no control net"));
                };
                let b = well_behaved_control_net(g.base.test_point(), &pts, rational, *a, *j, nu, nv)?;
                o.extend([0.0, flag(b)]);
            }
            OpIn::Stepper { uv, tight, uv2 } => {
                let mut s = stepper(*uv, *tight)?;
                push_base(&mut o, &s.base);
                o.push(s.max_step());
                let v = s.eval()?;
                push3(&mut o, s.gap()?);
                push3(&mut o, s.gap_u()?);
                push3(&mut o, s.gap_v()?);
                o.push(s.gap_length()?);
                o.extend([v.f_val, v.g_val, v.f_check, v.g_check, v.sum_check]);
                push3(&mut o, s.pos()?);
                push3(&mut o, s.su()?);
                push3(&mut o, s.sv()?);
                push3(&mut o, s.suu()?);
                push3(&mut o, s.suv()?);
                push3(&mut o, s.svv()?);
                let c = SurfPtSolveStepper::copy_of(&s);
                o.push(flag(c.is_set()));
                push2(&mut o, c.uv());
                let mut a = stepper(*uv2, *tight)?;
                a.assign(&s);
                o.push(flag(a.is_set()));
                o.push(a.gap_length()?);
                s.set_uv(p2(*uv2));
                o.push(flag(s.is_set()));
                s.clamp();
                push2(&mut o, s.uv());
            }
            OpIn::HitBound { uv, steps } => {
                let mut s = stepper(*uv, tol)?;
                o.push(0.0);
                for h in steps {
                    s.set_uv(p2(h.uv));
                    let which = if h.v { SurfParamType::V } else { SurfParamType::U };
                    s.hit_bound(which, h.tob);
                    o.push(f64::from(s.bound_hits_u()));
                    o.push(f64::from(s.bound_hits_v()));
                }
            }
            OpIn::CheckBounds { curr, next, bdom } => {
                // the stepper is constructed (and may fail) as upstream's
                let _ = stepper(*curr, tol)?;
                let r = SurfPtSolveStepper::check_bounds(p2(*curr), p2(*next), &dom_of(*bdom));
                o.push(0.0);
                o.push(flag(r.out_of_bounds));
                o.extend([f64::from(r.hits_u), f64::from(r.hits_v)]);
                push2(&mut o, r.clamped_uv.unwrap_or(Point2::new(0.0, 0.0)));
                push2(&mut o, r.clamped_uv_scaled.unwrap_or(Point2::new(0.0, 0.0)));
                o.push(r.step_scale);
            }
            OpIn::DropIso {
                uv,
                v,
                crease,
                param,
            } => {
                let mut s = stepper(*uv, tol)?;
                let which = if *v { SurfParamType::V } else { SurfParamType::U };
                let b = s.drop_to_iso_curve(&k, which, *crease, *param)?;
                o.push(0.0);
                o.push(flag(b));
                push2(&mut o, s.uv());
            }
        }
        Ok(o)
    };
    Ok(run())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    Ok(match rust_op(i)? {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

// ---- generated cases ----------------------------------------------------------------

/// The supported solver operations.
pub(crate) const SOLVERS: [u8; 5] = [0, 3, 4, 5, 10];

/// A point of `d`: a corner, a point on a side, or inside.
pub(crate) fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let pick = |rng: &mut Rng, a: f64, b: f64| -> f64 {
        match rng.below(6) {
            0 => a,
            1 => b,
            _ => a + (b - a) * rng.unit(),
        }
    };
    [pick(rng, d[0], d[2]), pick(rng, d[1], d[3])]
}

/// A point in `d` or, half the time, somewhat outside it.
pub(crate) fn gen_uv_any(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let [u, v] = gen_uv(rng, d);
    if rng.chance(0.5) {
        return [u, v];
    }
    let (w, h) = (d[2] - d[0], d[3] - d[1]);
    [
        u + w * rng.uniform(-0.9, 0.9),
        v + h * rng.uniform(-0.9, 0.9),
    ]
}

/// A test point for the surface: on it, slightly or well off it along its normal, or
/// anywhere around it.
pub(crate) fn gen_test(rng: &mut Rng, s: &Surface, d: [f64; 4], size: f64) -> [f64; 3] {
    let uv = gen_uv(rng, d);
    let Ok((p, su, sv)) = s.evaluate_1st_derivatives(p2(uv), false, false) else {
        return gvec(rng, size * 2.0);
    };
    let n = su.cross(sv).unitize_or_z();
    let off = match rng.below(6) {
        0 => 0.0,
        1 => size * 10f64.powf(rng.uniform(-12.0, -7.0)),
        2 => size * rng.uniform(-0.3, 0.3),
        3 => size * rng.uniform(-3.0, 3.0),
        4 => return gvec(rng, size * 1.5),
        _ => {
            // off the normal: a tangential offset too
            let t = su.unitize_or_z() * (size * rng.uniform(-0.5, 0.5));
            let q = p + n * (size * rng.uniform(-0.5, 0.5)) + t;
            return [q.x, q.y, q.z];
        }
    };
    let q: Point3 = p + n * off;
    [q.x, q.y, q.z]
}

/// A test point on the surface normal at `uv`: on the surface, or off it on either
/// side by a small to large distance.
pub(crate) fn test_on_normal(rng: &mut Rng, s: &Surface, uv: [f64; 2], size: f64) -> [f64; 3] {
    let Ok((p, su, sv)) = s.evaluate_1st_derivatives(p2(uv), false, false) else {
        return gvec(rng, size);
    };
    let n = su.cross(sv).unitize_or_z();
    let off = match rng.below(4) {
        0 => 0.0,
        1 => size * 10f64.powf(rng.uniform(-13.0, -6.0)),
        2 => size * rng.uniform(-1.0, 1.0),
        _ => size * rng.uniform(-20.0, 20.0),
    };
    let q = p + n * off;
    [q.x, q.y, q.z]
}

/// A closed B-spline: a full rational circle (or a closed polygon) in u swept in v.
pub(crate) fn gen_closed(rng: &mut Rng) -> InlineSrf {
    let r = rng.uniform(0.5, 5.0);
    let h = rng.uniform(0.5, 5.0);
    let v_rows = 2 + rng.below(2) as usize;
    let q = v_rows - 1;
    let v_knots: Vec<f64> = std::iter::repeat_n(0.0, q + 1)
        .chain(std::iter::repeat_n(1.0, q + 1))
        .collect();
    if rng.chance(0.5) {
        // rational circle: 9 points, degree 2
        let w = std::f64::consts::FRAC_1_SQRT_2;
        let pts: [(f64, f64, f64); 9] = [
            (1.0, 0.0, 1.0),
            (1.0, 1.0, w),
            (0.0, 1.0, 1.0),
            (-1.0, 1.0, w),
            (-1.0, 0.0, 1.0),
            (-1.0, -1.0, w),
            (0.0, -1.0, 1.0),
            (1.0, -1.0, w),
            (1.0, 0.0, 1.0),
        ];
        let net = pts
            .iter()
            .map(|&(x, y, wt)| {
                (0..v_rows)
                    .map(|j| {
                        let z = h * j as f64 / q as f64;
                        [x * r * wt, y * r * wt, z * wt, wt]
                    })
                    .collect()
            })
            .collect();
        InlineSrf {
            net,
            p: 2,
            q,
            u: vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0],
            v: v_knots,
        }
    } else {
        // closed polygon, degree 1 (non-rational)
        let k = 3 + rng.below(4) as usize;
        let net = (0..=k)
            .map(|i| {
                let t = std::f64::consts::TAU * (i % k) as f64 / k as f64;
                (0..v_rows)
                    .map(|j| {
                        let z = h * j as f64 / q as f64;
                        [r * t.cos(), r * t.sin(), z, NOW]
                    })
                    .collect()
            })
            .collect();
        let mut u = vec![0.0];
        u.extend((0..=k).map(|i| i as f64 / k as f64));
        u.push(1.0);
        InlineSrf {
            net,
            p: 1,
            q,
            u,
            v: v_knots,
        }
    }
}

/// A bilinear (2 x 2) surface, sometimes twisted.
pub(crate) fn gen_bilinear(rng: &mut Rng) -> InlineSrf {
    let tw = if rng.chance(0.5) { 0.0 } else { rng.uniform(-1.0, 1.0) };
    let net = vec![
        vec![[0.0, 0.0, 0.0, NOW], [0.0, 1.0, 0.0, NOW]],
        vec![[1.0, 0.0, 0.0, NOW], [1.0, 1.0, tw, NOW]],
    ];
    InlineSrf {
        net,
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// A badly behaved net: sharp zigzags and coincident interior points.
fn gen_zigzag(rng: &mut Rng) -> InlineSrf {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + 1 + rng.below(3) as usize;
    let m = q + 1 + rng.below(3) as usize;
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let mut x = i as f64;
                    let mut y = j as f64;
                    if rng.chance(0.3) {
                        // fold back in u or v
                        x -= rng.uniform(0.5, 2.5);
                    }
                    if rng.chance(0.3) {
                        y -= rng.uniform(0.5, 2.5);
                    }
                    if rng.chance(0.15) && i > 0 {
                        // coincident with the previous row
                        x = (i - 1) as f64;
                    }
                    [x, y, rng.uniform(-0.3, 0.3), NOW]
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

pub(crate) fn gen_surface(rng: &mut Rng) -> SurfIn {
    let bss = |s: InlineSrf| BssIn {
        oob: false,
        form: default_form(),
        srf: SrfIn::Inline(s),
    };
    match rng.below(14) {
        0 | 1 => SurfIn::Bspline(bss(gen_closed(rng))),
        2 => SurfIn::Bspline(bss(gen_bilinear(rng))),
        3 | 4 => SurfIn::Bspline(bss(gen_zigzag(rng))),
        _ => gen_surf(rng),
    }
}

/// The control-net counts of a surface input (a plane's net is 2 x 2).
pub(crate) fn net_counts(s: &SurfIn) -> Result<(usize, usize), HarnessError> {
    match s {
        SurfIn::Plane(_) => Ok((2, 2)),
        SurfIn::Bspline(b) | SurfIn::CurveBounded(b) => {
            let s = srf(&b.srf)?;
            Ok((s.net.len(), s.net.first().map_or(0, Vec::len)))
        }
    }
}

fn gen_op(rng: &mut Rng, s: &SurfIn, d: [f64; 4], sd: [f64; 4], size: f64) -> Result<OpIn, HarnessError> {
    let (nu, nv) = net_counts(s)?;
    let idx = |rng: &mut Rng, n: usize, edge: bool| -> usize {
        if edge && rng.chance(0.5) {
            if rng.chance(0.5) {
                0
            } else {
                n.saturating_sub(1)
            }
        } else {
            rng.below(n.max(1) as u64) as usize
        }
    };
    let small_tol = |rng: &mut Rng| -> f64 {
        match rng.below(4) {
            0 => 1e-10,
            1 => size * 1e-6,
            2 => -size * 1e-6,
            _ => size * rng.uniform(1e-4, 0.1),
        }
    };
    // more local-extremum and control-net cases than the others
    let pick = match rng.below(46) {
        40..=42 => 29,
        43..=45 => 23,
        x => x,
    };
    Ok(match pick {
        0..=1 => OpIn::Ctor,
        2..=3 => OpIn::Closed,
        4..=7 => OpIn::Func {
            uv: gen_uv_any(rng, d),
        },
        8..=11 => OpIn::TriGuess {
            iu: idx(rng, nu, false),
            iv: idx(rng, nv, false),
        },
        12..=14 => OpIn::Leg {
            iu: idx(rng, nu, true),
            iv: idx(rng, nv, true),
        },
        15..=17 => OpIn::Stride { uv: gen_uv(rng, sd) },
        18 => OpIn::SeamsUv { uv: gen_uv(rng, sd) },
        19..=21 => OpIn::SeamsCpts {
            iu: idx(rng, nu, true),
            iv: idx(rng, nv, true),
        },
        22 => OpIn::Crease,
        23..=26 => OpIn::Local {
            uv: gen_uv(rng, sd),
            ltol: small_tol(rng),
        },
        27..=28 => OpIn::TriDist {
            i: idx(rng, nu.saturating_sub(1), false),
            j: idx(rng, nv.saturating_sub(1), false),
        },
        29..=30 => OpIn::Well {
            i: idx(rng, nu, true),
            j: idx(rng, nv, true),
        },
        31..=33 => OpIn::Stepper {
            uv: gen_uv(rng, d),
            tight: small_tol(rng).abs(),
            uv2: gen_uv_any(rng, sd),
        },
        34..=35 => {
            let n = 1 + rng.below(12) as usize;
            let steps = (0..n.min(MAX_HIT_STEPS))
                .map(|_| HitIn {
                    uv: gen_uv_any(rng, sd),
                    v: rng.chance(0.5),
                    tob: rng.range_i64(-1, 1) as i32,
                })
                .collect();
            OpIn::HitBound {
                uv: gen_uv(rng, d),
                steps,
            }
        }
        36..=37 => {
            let bdom = if rng.chance(0.5) { sd } else { gen_sub_dom(rng, sd) };
            OpIn::CheckBounds {
                curr: gen_uv(rng, d),
                next: gen_uv_any(rng, bdom),
                bdom,
            }
        }
        _ => {
            let v = rng.chance(0.5);
            let (a, b) = if v { (d[1], d[3]) } else { (d[0], d[2]) };
            OpIn::DropIso {
                uv: gen_uv(rng, d),
                v,
                crease: a + (b - a) * rng.unit(),
                param: rng.uniform(-1.0, 2.0),
            }
        }
    })
}

fn gen_case(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surface(rng);
    let wrap = rng.chance(0.15);
    let d = surf_domain(&surf)?;
    let size = surf_size(&surf)?;
    let sd = if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) };
    let solver = *rng.choose(&SOLVERS).unwrap_or(&3);
    let s = surface_in(&surf)?;
    let test = if solver == 10 {
        match rng.below(4) {
            0 => [1.0, 0.0, 0.0],
            1 => [0.0, -1.0, 0.0],
            _ => gvec(rng, 1.0),
        }
    } else {
        gen_test(rng, &s, sd, size)
    };
    let tol = match rng.below(4) {
        0 => 1e-6,
        1 => size * 1e-8,
        2 => size * rng.uniform(1e-5, 1e-2),
        _ => 1e-10,
    };
    let mut op = gen_op(rng, &surf, d, sd, size)?;
    let mut test = test;
    if let OpIn::Local { uv, .. } = &mut op {
        if solver != 10 && rng.chance(0.7) {
            test = test_on_normal(rng, &s, *uv, size);
        }
    }
    Ok(Inputs {
        wrap,
        surf,
        dom: sd,
        solver,
        test,
        tol,
        op,
    })
}

// ---- inline cases from the upstream tests ----------------------------------------------------

/// The test points of `my_test_surface_closest_point` on `Surface.sms`
/// (`tests/prog_test/src/smSurfaceTest.cpp:4117`).
pub(crate) const SURFACE_SMS_POINTS: [[f64; 3]; 15] = [
    [-1.0616429349692744, -20.167126109553802, 20.020823739437560],
    [-1.1583434957685239, -19.953438722680168, 20.005921768970701],
    [-4.5941574701649142, -9.6528635542995485, 19.290949275641431],
    [-0.83908714000092477, -20.645564191951244, 20.054070584952139],
    [-0.22169184902399575, -21.752883811327614, 20.077084827504891],
    [0.097876793468328019, -21.633397717952313, 18.978461556867217],
    [-0.25257593806641893, -20.10880763638518, 19.018298541213202],
    [-0.32629818190650922, -19.760345509317247, 19.026748453308592],
    [-0.66892154856885222, -17.879224383359258, 19.072647185802911],
    [-0.83758573462226327, -20.648619822796935, 20.054270506974774],
    [-0.22211826515506564, -21.75240325273451, 20.077401022086111],
    [0.097878601273081345, -21.633405764037899, 18.978461275445223],
    [-0.25280760311075551, -20.107797610675622, 19.018327728721825],
    [-0.32651463102618838, -19.7592999915767, 19.026773809915298],
    [-0.40219453853645376, -19.384417128948424, 19.035877125510886],
];

fn upstream_cases(ctx: &CaseContext, id: &str) -> Result<Vec<Case>, HarnessError> {
    let path = ctx.upstream_test_file("Surface.sms")?;
    let sms = SrfIn::File {
        sms: path.to_string_lossy().into_owned(),
    };
    let inline = srf(&sms)?;
    let surf = SurfIn::Bspline(BssIn {
        oob: false,
        form: default_form(),
        srf: sms,
    });
    let d = [
        inline.u.first().copied().unwrap_or(0.0),
        inline.v.first().copied().unwrap_or(0.0),
        inline.u.last().copied().unwrap_or(1.0),
        inline.v.last().copied().unwrap_or(1.0),
    ];
    let (nu, nv) = (inline.net.len(), inline.net.first().map_or(0, Vec::len));
    let mid = [0.5 * (d[0] + d[2]), 0.5 * (d[1] + d[3])];
    let mut out = Vec::new();
    for (pi, test) in SURFACE_SMS_POINTS.iter().enumerate() {
        let ops = [
            OpIn::Ctor,
            OpIn::Closed,
            OpIn::TriGuess { iu: 0, iv: 0 },
            OpIn::Leg {
                iu: pi % nu,
                iv: (pi * 7) % nv,
            },
            OpIn::SeamsCpts {
                iu: if pi % 2 == 0 { 0 } else { nu - 1 },
                iv: pi % nv,
            },
            OpIn::Func { uv: mid },
            OpIn::Stride {
                uv: [d[0] + (d[2] - d[0]) * (pi as f64 / 14.0), d[1]],
            },
            OpIn::Local {
                uv: [d[2], d[1] + (d[3] - d[1]) * (pi as f64 / 14.0)],
                ltol: 1e-6,
            },
            OpIn::Stepper {
                uv: mid,
                tight: 1e-12,
                uv2: [d[2] + 1.0, d[1] - 1.0],
            },
        ];
        for (oi, op) in ops.into_iter().enumerate() {
            let inputs = Inputs {
                wrap: false,
                surf: surf.clone(),
                dom: d,
                solver: 3,
                test: *test,
                tol: 1e-6,
                op,
            };
            out.push(Case {
                unit: id.to_string(),
                name: format!("surface_sms_pt{:02}_op{oi}", pi + 1),
                source: CaseSource::Inline,
                inputs: serde_json::to_value(inputs).map_err(|e| harness(e.to_string()))?,
            });
        }
    }
    Ok(out)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_point_solve_p1"
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
        let mut v = upstream_cases(ctx, self.id())?;
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        v.extend(default_cases(self, &ctx)?);
        Ok(v)
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

    #[test]
    fn op_codes_round_trip() {
        let i = Inputs {
            wrap: false,
            surf: SurfIn::Bspline(BssIn {
                oob: false,
                form: default_form(),
                srf: SrfIn::Inline(gen_bilinear(&mut Rng::new(1))),
            }),
            dom: [0.0, 0.0, 1.0, 1.0],
            solver: 3,
            test: [0.5, 0.5, 1.0],
            tol: 1e-6,
            op: OpIn::Ctor,
        };
        let (op, input) = cpp_input(&i).unwrap();
        assert_eq!(op, Op::Ctor);
        assert!(ffi::validate(op, &input).is_ok());
    }
}
