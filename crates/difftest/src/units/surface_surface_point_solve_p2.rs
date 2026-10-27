//! Runner for unit `surface.surface_point_solve_p2`: the global solver's guesses,
//! refinements, seam solutions and `Solve`, and the local stepper's step adjustment,
//! norms, convergence and boundary tests, steps and line searches
//! (`SmSurfacePointSolve.cpp`).
//!
//! C++: `solidcore_ffi::units::surface_surface_point_solve_p2` (the shim compiles the
//! upstream file, whose classes are file-local). Rust:
//! `solidcore::surface::surface_point_solve::p2`.
//!
//! A case is a problem as in the `surface.surface_point_solve_p1` runner (surface `surf`,
//! wrapped in a STEP surface when `wrap`; solver domain `dom`, operation `solver` as an
//! `SmSolverOperationType` code, test point `test`, tolerance `tol`) and an `op` with its
//! parameters. Both sides produce the flat record documented in the ffi module; the
//! runner reports its first entry as the `status` (`0` = `SM_SUCCESS`, else the
//! `SmStatus`; `-1` for a Rust error upstream cannot produce) and the rest as the `out`
//! values. Solutions are `n` then `u v value objects variables object0_is_surface` each.
//!
//! Cases: inline cases on the upstream `Surface.sms` with the test points of
//! `my_test_surface_closest_point` (`tests/prog_test/src/smSurfaceTest.cpp:4117`), and
//! generated cases (planes, B-splines incl. closed, rational, bilinear, zigzag and
//! huge / tiny ones, curve-bounded and STEP-wrapped surfaces; sub-domains; test points on,
//! near and far from the surface; stepper points inside, on and near the boundary), and
//! `rev_gen*` cases on surfaces of revolution (real `SmSurfOfRevolution`s, built as the
//! `surface.surf_of_revolution_p1` runner builds them, some STEP-wrapped; the problem
//! domain from the surface's natural domain as the shim reports it).
//!
//! Deferred cases: `SmSurface::LocalPointSolve` (`surface.surface_p5`; every `Solve` that
//! gets a rough guess), `SmSurface::IsOnSeam` (`surface.surface_p4`; `RefineGuess` on any
//! surface but a plane), `SmSurfaceEval::Normal` (`surface.surface_p4`; convergence at a
//! singularity), `SmSurface::InvertPointNearSingularity` (`surface.surface_p8`; a step
//! from a singularity), and the bounding box of a curve-bounded or STEP surface for the
//! directed operation (`surface.surface_p6`, as in p1).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolutionRequested;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionArray, SolutionObject};
use solidcore::kernel::Kernel;
use solidcore::surface::surface::p1::StepSurface;
use solidcore::surface::surface::Surface;
use solidcore::surface::surface_point_solve::p1::ErrorNormType;
use solidcore::surface::surface_point_solve::{SurfPtGlobalSolver, SurfPtSolveStepper};
use solidcore_ffi::units::surface_surface_point_solve_p2::{self as ffi, Op, MAX_GRID, MAX_HITS};

use super::surface_surf_of_revolution_p1::{
    ffi_surf as rev_ffi_surf, gen_surf as rev_gen_surf, rust_surf as rev_rust_surf,
    SurfIn as RevSurfIn,
};
use super::surface_surface_cache_p1::{
    dom_of, flag, gen_sub_dom, harness, srf, status, v3, BssIn, SrfIn,
};
use super::surface_surface_cache_p3::{push_surf, surf_domain, surf_size, surface_in, SurfIn};
use super::surface_surface_point_solve_p1::{
    gen_surface, gen_test, gen_uv, gen_uv_any, net_counts, p2, push2, solver_op, test_on_normal,
    wrapped_of, SOLVERS, SURFACE_SMS_POINTS,
};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 900;

/// Surface of revolution cases (see [`rev_cases`]).
const REV_CASES: usize = 240;

/// The seed of the first surface of revolution case.
const REV_SEED: u64 = 0x5245_5653_5032_0000;

/// `surface.surface_point_solve_p2` runner.
pub struct Runner;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Rough,
    Poly {
        iu: usize,
        iv: usize,
    },
    Refine {
        uv: [f64; 2],
    },
    Patch {
        sub: [f64; 4],
        nu: i32,
        nv: i32,
        uv: [f64; 2],
        best: f64,
    },
    Grid {
        uv: [f64; 2],
        iu: usize,
        iv: usize,
    },
    Seams {
        uv: [f64; 2],
        value: f64,
    },
    Solve,
    Adjust {
        uv0: [f64; 2],
        tight: f64,
        uv1: [f64; 2],
        hu: i32,
        hv: i32,
    },
    Norms {
        uv0: [f64; 2],
        tight: f64,
    },
    Conv {
        uv0: [f64; 2],
        tight: f64,
        ctol: f64,
    },
    Push {
        uv0: [f64; 2],
        tight: f64,
        ptol: f64,
    },
    Snap {
        uv0: [f64; 2],
        tight: f64,
        stol: f64,
    },
    Step {
        uv0: [f64; 2],
        tight: f64,
        #[serde(default)]
        plane: bool,
    },
    Line {
        uv0: [f64; 2],
        tight: f64,
        uv1: [f64; 2],
    },
    LineB {
        uv0: [f64; 2],
        tight: f64,
        uv1: [f64; 2],
        norm: u8,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Rough => Op::Rough,
            OpIn::Poly { .. } => Op::Poly,
            OpIn::Refine { .. } => Op::Refine,
            OpIn::Patch { .. } => Op::Patch,
            OpIn::Grid { .. } => Op::Grid,
            OpIn::Seams { .. } => Op::Seams,
            OpIn::Solve => Op::Solve,
            OpIn::Adjust { .. } => Op::Adjust,
            OpIn::Norms { .. } => Op::Norms,
            OpIn::Conv { .. } => Op::Conv,
            OpIn::Push { .. } => Op::Push,
            OpIn::Snap { .. } => Op::Snap,
            OpIn::Step { .. } => Op::Step,
            OpIn::Line { .. } => Op::Line,
            OpIn::LineB { .. } => Op::LineB,
        }
    }
}

/// The problem's surface: a surface of revolution (as the `surface.surf_of_revolution_p1`
/// runner builds it), or one of the `surface.surface_cache_p3` kinds.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AnySurf {
    Rev { rev: RevSurfIn },
    Base(SurfIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    /// Wrap the surface in an `SmSTEPSurface`.
    #[serde(default)]
    wrap: bool,
    surf: AnySurf,
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

/// The problem's surface block `W S` (not for a surface of revolution).
fn cpp_head(i: &Inputs, s: &SurfIn) -> Result<Vec<f64>, HarnessError> {
    let mut o = vec![flag(i.wrap)];
    push_surf(&mut o, s)?;
    Ok(o)
}

/// The problem after its surface block and the op's parameters.
fn cpp_rest(i: &Inputs) -> (Op, Vec<f64>) {
    let mut o = Vec::new();
    o.extend(i.dom);
    o.push(f64::from(i.solver));
    o.extend(i.test);
    o.push(i.tol);
    match &i.op {
        OpIn::Rough | OpIn::Solve => {}
        OpIn::Poly { iu, iv } => o.extend([*iu as f64, *iv as f64]),
        OpIn::Refine { uv } => o.extend(uv),
        OpIn::Patch {
            sub,
            nu,
            nv,
            uv,
            best,
        } => {
            o.extend(sub);
            o.extend([f64::from(*nu), f64::from(*nv)]);
            o.extend(uv);
            o.push(*best);
        }
        OpIn::Grid { uv, iu, iv } => {
            o.extend(uv);
            o.extend([*iu as f64, *iv as f64]);
        }
        OpIn::Seams { uv, value } => {
            o.extend(uv);
            o.push(*value);
        }
        OpIn::Adjust {
            uv0,
            tight,
            uv1,
            hu,
            hv,
        } => {
            o.extend(uv0);
            o.push(*tight);
            o.extend(uv1);
            o.extend([f64::from(*hu), f64::from(*hv)]);
        }
        OpIn::Norms { uv0, tight } => {
            o.extend(uv0);
            o.push(*tight);
        }
        OpIn::Conv {
            uv0,
            tight,
            ctol: t,
        }
        | OpIn::Push {
            uv0,
            tight,
            ptol: t,
        }
        | OpIn::Snap {
            uv0,
            tight,
            stol: t,
        } => {
            o.extend(uv0);
            o.extend([*tight, *t]);
        }
        OpIn::Step { uv0, tight, plane } => {
            o.extend(uv0);
            o.extend([*tight, flag(*plane)]);
        }
        OpIn::Line { uv0, tight, uv1 } => {
            o.extend(uv0);
            o.push(*tight);
            o.extend(uv1);
        }
        OpIn::LineB {
            uv0,
            tight,
            uv1,
            norm,
        } => {
            o.extend(uv0);
            o.push(*tight);
            o.extend(uv1);
            o.push(f64::from(*norm));
        }
    }
    (i.op.ffi(), o)
}

fn to_output(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("out", rest.to_vec())
}

/// The FFI's own validation of `i` (what [`cpp`] runs), applied to the Rust side too:
/// its guards refuse inputs upstream (and the faithful port) never finish on. A
/// surface of revolution is not checked here (its check builds the surface in C++).
fn validate(i: &Inputs) -> Result<(), HarnessError> {
    let (op, rest) = cpp_rest(i);
    match &i.surf {
        AnySurf::Rev { .. } => Ok(()),
        AnySurf::Base(s) => {
            let mut input = cpp_head(i, s)?;
            input.extend(rest);
            ffi::validate(op, &input).map_err(|e| harness(e.to_string()))
        }
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, rest) = cpp_rest(i);
    let flat = match &i.surf {
        AnySurf::Rev { rev } => ffi::eval_rev(op, i.wrap, &rev_ffi_surf(rev)?, &rest),
        AnySurf::Base(s) => {
            let mut input = cpp_head(i, s)?;
            input.extend(rest);
            ffi::eval(op, &input)
        }
    }
    .map_err(|e| harness(e.to_string()))?;
    Ok(to_output(&flat))
}

// ---- Rust side ----------------------------------------------------------------------

/// The solutions' handle for the case's surface: it reports the surface's natural
/// domain (`GetNaturalUVDomain` of the solution object, for `IsEqual`).
#[derive(Debug, Clone, Copy, PartialEq)]
struct SrfObj(Extent2d);

impl SolutionObject for SrfObj {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }

    fn natural_uv_domain(&self) -> Option<Extent2d> {
        Some(self.0)
    }
}

fn push_solutions(o: &mut Vec<f64>, sols: &SolutionArray<SrfObj>, obj: SrfObj) {
    o.push(sols.len() as f64);
    for s in sols.as_slice() {
        o.extend([
            s.start.parameters[0],
            s.start.parameters[1],
            s.start.solution_value,
            s.num_objects as f64,
            s.num_variables as f64,
            flag(s.objects[0] == Some(obj)),
        ]);
    }
}

fn norm_of(code: u8) -> ErrorNormType {
    match code {
        0 => ErrorNormType::SumAbs,
        1 => ErrorNormType::Sum,
        2 => ErrorNormType::F,
        3 => ErrorNormType::G,
        4 => ErrorNormType::FAbs,
        5 => ErrorNormType::GAbs,
        6 => ErrorNormType::Max,
        7 => ErrorNormType::MaxAbs,
        _ => ErrorNormType::Gap,
    }
}

const ALL_NORMS: [ErrorNormType; 9] = [
    ErrorNormType::SumAbs,
    ErrorNormType::Sum,
    ErrorNormType::F,
    ErrorNormType::G,
    ErrorNormType::FAbs,
    ErrorNormType::GAbs,
    ErrorNormType::Max,
    ErrorNormType::MaxAbs,
    ErrorNormType::Gap,
];

/// `UV BoundHitsU BoundHitsV NRStepSuspect IterCount` of a stepper.
fn push_state(o: &mut Vec<f64>, s: &SurfPtSolveStepper<'_>) {
    push2(o, s.uv());
    o.extend([
        f64::from(s.bound_hits_u()),
        f64::from(s.bound_hits_v()),
        flag(s.nr_step_suspect()),
        f64::from(s.iter_count()),
    ]);
}

/// `HitBound(which, sign)` `|n|` times.
fn hits(s: &mut SurfPtSolveStepper<'_>, which: solidcore::nurbs::b_spline_surface::SurfParamType, n: i32) {
    let sign = if n < 0 { -1 } else { 1 };
    for _ in 0..n.unsigned_abs() {
        s.hit_bound(which, sign);
    }
}

/// An error status record (`-1` for an error upstream cannot produce), or the deferral.
fn err_record(e: &solidcore::SolidError) -> Result<Vec<f64>, solidcore::SolidError> {
    if e.is_unported() {
        return Err(e.clone());
    }
    Ok(vec![status(e)])
}

#[allow(clippy::too_many_lines)]
fn rust_op(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    use solidcore::nurbs::b_spline_surface::SurfParamType;
    let k = Kernel::default();
    let surface: Surface = any_surface(&k, i.wrap, &i.surf)?;
    let dom = dom_of(i.dom);
    let op = solver_op(i.solver)?;
    let test = v3(i.test);
    let tol = i.tol;
    let obj = SrfObj(surface.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?);
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
            OpIn::Rough => {
                let mut g = global()?;
                match g.find_rough_guess(&k) {
                    Ok(r) => {
                        o.push(0.0);
                        push2(&mut o, r.uv);
                        o.extend([r.best_idx_u as f64, r.best_idx_v as f64]);
                    }
                    Err(e) => return err_record(&e),
                }
            }
            OpIn::Poly { iu, iv } => {
                let g = global()?;
                match g.find_polygon_guess(&k, (*iu, *iv)) {
                    Ok(r) => {
                        o.push(0.0);
                        push2(&mut o, r.uv);
                        o.extend([r.best_idx_u as f64, r.best_idx_v as f64, flag(r.suspicious)]);
                    }
                    Err(e) => return err_record(&e),
                }
            }
            OpIn::Refine { uv } => {
                let mut g = global()?;
                let mut uv = p2(*uv);
                match g.refine_guess(&Kernel::default(), &mut uv) {
                    Ok(()) => o.push(0.0),
                    Err(e) => o = err_record(&e)?,
                }
                push2(&mut o, uv);
            }
            OpIn::Patch {
                sub,
                nu,
                nv,
                uv,
                best,
            } => {
                let g = global()?;
                let mut uv = p2(*uv);
                let mut best = *best;
                g.refine_patch_by_grid(&k, &dom_of(*sub), *nu, *nv, &mut uv, &mut best)?;
                o.push(0.0);
                push2(&mut o, uv);
                o.push(best);
            }
            OpIn::Grid { uv, iu, iv } => {
                let g = global()?;
                let r = g.refine_guess_by_grid(&k, p2(*uv), *iu, *iv)?;
                o.push(0.0);
                push2(&mut o, r);
            }
            OpIn::Seams { uv, value } => {
                let mut g = global()?;
                let mut sol: Solution<SrfObj> = Solution::new();
                sol.num_variables = 2;
                sol.num_objects = 1;
                sol.objects[0] = Some(obj);
                sol.start.parameters[0] = uv[0];
                sol.start.parameters[1] = uv[1];
                sol.start.solution_value = *value;
                let tight = g.base.tight_tol();
                let sols = g.check_seams(&k, tight, &sol, Some(obj))?;
                o.push(0.0);
                push_solutions(&mut o, &sols, obj);
            }
            OpIn::Solve => {
                let mut g = global()?;
                match g.solve(&k, Some(obj)) {
                    Ok(sols) => {
                        o.push(0.0);
                        push_solutions(&mut o, &sols, obj);
                    }
                    Err(e) => return err_record(&e),
                }
            }
            OpIn::Adjust {
                uv0,
                tight,
                uv1,
                hu,
                hv,
            } => {
                let mut prev = stepper(*uv0, *tight)?;
                hits(&mut prev, SurfParamType::U, *hu);
                hits(&mut prev, SurfParamType::V, *hv);
                let mut next = SurfPtSolveStepper::copy_of(&k, &prev);
                next.set_uv(p2(*uv1));
                match next.adjust_step(&k, &prev) {
                    Ok((adjusted, scale)) => o.extend([0.0, flag(adjusted), scale]),
                    Err(e) => {
                        o = err_record(&e)?;
                        // upstream's outputs on SM_ERR
                        o.extend([flag(true), 0.0]);
                    }
                }
                push_state(&mut o, &next);
            }
            OpIn::Norms { uv0, tight } => {
                let mut s = stepper(*uv0, *tight)?;
                o.push(0.0);
                for n in ALL_NORMS {
                    o.push(s.error_norm(n)?);
                }
                let (b, which) = s.is_singular()?;
                o.extend([flag(b), f64::from(which as u8)]);
            }
            OpIn::Conv { uv0, tight, ctol } => {
                let mut s = stepper(*uv0, *tight)?;
                o.push(0.0);
                o.push(flag(s.converged(&k, *ctol)?));
                o.push(flag(s.converged_with(&k, true)?));
                o.push(flag(s.converged_with(&k, false)?));
            }
            OpIn::Push { uv0, tight, ptol } => {
                let mut s = stepper(*uv0, *tight)?;
                let mut c = SurfPtSolveStepper::copy_of(&k, &s);
                o.push(0.0);
                let (b, which) = s.pushing_boundary(*ptol)?;
                o.extend([flag(b), f64::from(which as u8)]);
                let (b, which) = c.converged_on_boundary(*ptol)?;
                o.extend([flag(b), f64::from(which as u8)]);
            }
            OpIn::Snap { uv0, tight, stol } => {
                let mut s = stepper(*uv0, *tight)?;
                let b = s.snap_to_boundaries(*stol)?;
                o.extend([0.0, flag(b)]);
                push_state(&mut o, &s);
            }
            OpIn::Step { uv0, tight, plane } => {
                let mut prev = stepper(*uv0, *tight)?;
                let mut next = SurfPtSolveStepper::copy_of(&k, &prev);
                let r = next.step(&k, &mut prev, *plane)?;
                o.extend([0.0, f64::from(r.code())]);
                push_state(&mut o, &next);
            }
            OpIn::Line { uv0, tight, uv1 } => {
                let mut prev = stepper(*uv0, *tight)?;
                let mut next = SurfPtSolveStepper::copy_of(&k, &prev);
                next.set_uv(p2(*uv1));
                match next.line_search(&k, &mut prev) {
                    Ok(()) => o.push(0.0),
                    Err(e) => o = err_record(&e)?,
                }
                push_state(&mut o, &next);
            }
            OpIn::LineB {
                uv0,
                tight,
                uv1,
                norm,
            } => {
                let mut prev = stepper(*uv0, *tight)?;
                let mut next = SurfPtSolveStepper::copy_of(&k, &prev);
                next.set_uv(p2(*uv1));
                match next.line_search_bracketted(&k, &mut prev, norm_of(*norm)) {
                    Ok(()) => o.push(0.0),
                    Err(e) => o = err_record(&e)?,
                }
                push_state(&mut o, &next);
            }
        }
        Ok(o)
    };
    Ok(run())
}

/// The problem's surface, wrapped in an `SmSTEPSurface` when `wrap`.
fn any_surface(k: &Kernel, wrap: bool, s: &AnySurf) -> Result<Surface, HarnessError> {
    match s {
        AnySurf::Base(s) => wrapped_of(wrap, s),
        AnySurf::Rev { rev } => {
            let r = rev_rust_surf(k, rev)?
                .map_err(|e| harness(format!("surface of revolution: {e}")))?;
            let r = Surface::from(r);
            Ok(if wrap {
                Surface::from(StepSurface::new(r, true).map_err(|e| HarnessError::new(e.to_string()))?)
            } else {
                r
            })
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

/// A point on the boundary of `d` (a side or a corner), or just inside or outside it.
fn gen_uv_edge(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let [mut u, mut v] = gen_uv(rng, d);
    let (w, h) = (d[2] - d[0], d[3] - d[1]);
    match rng.below(4) {
        0 => u = if rng.chance(0.5) { d[0] } else { d[2] },
        1 => v = if rng.chance(0.5) { d[1] } else { d[3] },
        2 => {
            // near a side, by a tiny to small amount
            let e = 10f64.powf(rng.uniform(-12.0, -2.0));
            u = if rng.chance(0.5) { d[0] + e * w } else { d[2] - e * w };
        }
        _ => {
            let e = 10f64.powf(rng.uniform(-12.0, -2.0));
            v = if rng.chance(0.5) { d[1] + e * h } else { d[3] - e * h };
        }
    }
    [u, v]
}

/// A point of `d` clipped into the surface domain `sd`.
fn clip(uv: [f64; 2], sd: [f64; 4]) -> [f64; 2] {
    [uv[0].clamp(sd[0], sd[2]), uv[1].clamp(sd[1], sd[3])]
}

#[allow(clippy::too_many_lines)]
fn gen_op(rng: &mut Rng, (nu, nv): (usize, usize), d: [f64; 4], sd: [f64; 4], size: f64) -> OpIn {
    let idx = |rng: &mut Rng, n: usize| -> usize {
        if rng.chance(0.3) {
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
        match rng.below(5) {
            0 => 1e-10,
            1 => size * 1e-6,
            2 => -size * 1e-6,
            3 => 0.0,
            _ => size * rng.uniform(1e-4, 0.1),
        }
    };
    // stepper points: inside, on or near the boundary of the solver domain
    let st_uv = |rng: &mut Rng| -> [f64; 2] {
        if rng.chance(0.5) {
            gen_uv(rng, sd)
        } else {
            clip(gen_uv_edge(rng, sd), d)
        }
    };
    let tight = |rng: &mut Rng| -> f64 {
        match rng.below(3) {
            0 => 1e-12,
            1 => size * 1e-10,
            _ => size * 1e-7,
        }
    };
    match rng.below(40) {
        0..=2 => OpIn::Rough,
        3..=5 => OpIn::Poly {
            iu: idx(rng, nu),
            iv: idx(rng, nv),
        },
        6..=7 => OpIn::Refine {
            uv: clip(gen_uv_any(rng, sd), d),
        },
        8..=10 => {
            let sub = if rng.chance(0.3) {
                sd
            } else {
                gen_sub_dom(rng, d)
            };
            let g = |rng: &mut Rng| -> i32 {
                if rng.chance(0.2) {
                    rng.range_i64(-2, 2) as i32
                } else {
                    rng.range_i64(2, MAX_GRID.min(9)) as i32
                }
            };
            OpIn::Patch {
                sub,
                nu: g(rng),
                nv: g(rng),
                uv: gen_uv(rng, sd),
                best: match rng.below(3) {
                    0 => 1e300,
                    1 => size * rng.uniform(0.0, 2.0),
                    _ => -size,
                },
            }
        }
        11..=13 => OpIn::Grid {
            uv: gen_uv(rng, sd),
            iu: idx(rng, nu),
            iv: idx(rng, nv),
        },
        14..=17 => OpIn::Seams {
            uv: if rng.chance(0.7) {
                clip(gen_uv_edge(rng, sd), d)
            } else {
                gen_uv(rng, sd)
            },
            value: size * rng.uniform(0.0, 1.0),
        },
        18 => OpIn::Solve,
        19..=21 => {
            let uv0 = st_uv(rng);
            let uv1 = match rng.below(4) {
                0 => uv0,
                1 => [uv0[0] + 1e-13, uv0[1]],
                _ => gen_uv_any(rng, sd),
            };
            OpIn::Adjust {
                uv0,
                tight: tight(rng),
                uv1,
                hu: rng.range_i64(-MAX_HITS, MAX_HITS) as i32,
                hv: rng.range_i64(-MAX_HITS, MAX_HITS) as i32,
            }
        }
        22..=24 => OpIn::Norms {
            uv0: st_uv(rng),
            tight: tight(rng),
        },
        25..=26 => OpIn::Conv {
            uv0: st_uv(rng),
            tight: tight(rng),
            ctol: small_tol(rng).abs().max(1e-12),
        },
        27..=29 => OpIn::Push {
            uv0: st_uv(rng),
            tight: tight(rng),
            ptol: small_tol(rng),
        },
        30..=31 => OpIn::Snap {
            uv0: st_uv(rng),
            tight: tight(rng),
            stol: {
                let w = (sd[2] - sd[0]).min(sd[3] - sd[1]);
                w * 10f64.powf(rng.uniform(-10.0, -1.0))
            },
        },
        32..=35 => OpIn::Step {
            uv0: st_uv(rng),
            tight: tight(rng),
            plane: rng.chance(0.5),
        },
        36..=37 => OpIn::Line {
            uv0: st_uv(rng),
            tight: tight(rng),
            uv1: gen_uv(rng, d),
        },
        _ => OpIn::LineB {
            uv0: st_uv(rng),
            tight: tight(rng),
            uv1: gen_uv(rng, d),
            norm: rng.below(9) as u8,
        },
    }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surface(rng);
    let wrap = rng.chance(0.15);
    let d = surf_domain(&surf)?;
    let size = surf_size(&surf)?;
    let sd = if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) };
    let solver = *rng.choose(&SOLVERS).unwrap_or(&3);
    let s = surface_in(&surf)?;
    let (op, test, tol) = gen_op_test(k, rng, &s, net_counts(&surf)?, d, sd, solver, size);
    let surf = AnySurf::Base(surf);
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

/// A surface of revolution case (the generator of the `surface.surf_of_revolution_p1`
/// runner; the problem as [`gen_case`]).
fn gen_rev_case(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    use solidcore::nurbs::b_spline_surface::SurfParamType;
    let k = Kernel::default();
    let rev = rev_gen_surf(rng);
    let wrap = rng.chance(0.15);
    let built = rev_rust_surf(&k, &rev)?.map_err(|e| harness(format!("not built: {e}")))?;
    let counts = (
        built.base().number_control_points(SurfParamType::U),
        built.base().number_control_points(SurfParamType::V),
    );
    let surf = AnySurf::Rev { rev };
    let s = any_surface(&k, wrap, &surf)?;
    let nd = s.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
    let d = [nd.min().x, nd.min().y, nd.max().x, nd.max().y];
    // the largest coordinate magnitude over a grid of surface points
    let mut size = 1e-12_f64;
    for i in 0..=4 {
        for j in 0..=4 {
            let uv = [
                d[0] + (d[2] - d[0]) * f64::from(i) / 4.0,
                d[1] + (d[3] - d[1]) * f64::from(j) / 4.0,
            ];
            if let Ok(p) = s.evaluate_point(&k, p2(uv)) {
                size = size.max(p.x.abs()).max(p.y.abs()).max(p.z.abs());
            }
        }
    }
    let sd = if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) };
    let solver = *rng.choose(&SOLVERS).unwrap_or(&3);
    let (op, test, tol) = gen_op_test(&k, rng, &s, counts, d, sd, solver, size);
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

/// The problem's tolerance.
fn gen_tol(rng: &mut Rng, size: f64) -> f64 {
    match rng.below(4) {
        0 => 1e-6,
        1 => size * 1e-8,
        2 => size * rng.uniform(1e-5, 1e-2),
        _ => 1e-10,
    }
}

/// The op, the test point and the tolerance of a case over `s` (surface domain `d`, solver domain
/// `sd`, control-net counts `counts`).
fn gen_op_test(k: &Kernel, 
    rng: &mut Rng,
    s: &Surface,
    counts: (usize, usize),
    d: [f64; 4],
    sd: [f64; 4],
    solver: u8,
    size: f64,
) -> (OpIn, [f64; 3], f64) {
    let test = if solver == 10 {
        match rng.below(4) {
            0 => [1.0, 0.0, 0.0],
            1 => [0.0, -1.0, 0.0],
            _ => super::surface_surface_cache_p1::gvec(rng, 1.0),
        }
    } else {
        gen_test(k, rng, s, sd, size)
    };
    let tol = gen_tol(rng, size);
    let op = gen_op(rng, counts, d, sd, size);
    let mut test = test;
    // converged / boundary tests: test points on the normal at the stepper point
    if solver != 10 && rng.chance(0.5) {
        if let OpIn::Conv { uv0, .. } | OpIn::Push { uv0, .. } | OpIn::Seams { uv: uv0, .. } = &op {
            test = test_on_normal(k, rng, s, *uv0, size);
        }
    }
    (op, test, tol)
}

// ---- inline cases from the upstream tests ----------------------------------------------------

fn upstream_cases(ctx: &CaseContext, id: &str) -> Result<Vec<Case>, HarnessError> {
    let path = ctx.upstream_test_file("Surface.sms")?;
    let sms = SrfIn::File {
        sms: path.to_string_lossy().into_owned(),
    };
    let inline = srf(&sms)?;
    let surf = SurfIn::Bspline(BssIn {
        oob: false,
        form: super::surface_surface_cache_p1::default_form(),
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
    let at = |fu: f64, fv: f64| [d[0] + (d[2] - d[0]) * fu, d[1] + (d[3] - d[1]) * fv];
    let mut out = Vec::new();
    for (pi, test) in SURFACE_SMS_POINTS.iter().enumerate() {
        let f = pi as f64 / 14.0;
        let ops = [
            OpIn::Rough,
            OpIn::Poly { iu: 0, iv: 0 },
            OpIn::Grid {
                uv: at(f, 1.0 - f),
                iu: pi % nu,
                iv: (pi * 3) % nv,
            },
            OpIn::Patch {
                sub: d,
                nu: 7,
                nv: 7,
                uv: mid,
                best: 1e300,
            },
            OpIn::Seams {
                uv: at(0.0, f),
                value: 0.0,
            },
            OpIn::Adjust {
                uv0: mid,
                tight: 1e-12,
                uv1: at(1.2, f),
                hu: 0,
                hv: 0,
            },
            OpIn::Norms {
                uv0: at(f, 0.5),
                tight: 1e-12,
            },
            OpIn::Conv {
                uv0: at(f, f),
                tight: 1e-12,
                ctol: 1e-6,
            },
            OpIn::Push {
                uv0: at(1.0, f),
                tight: 1e-12,
                ptol: 1e-9,
            },
            OpIn::Step {
                uv0: mid,
                tight: 1e-12,
                plane: false,
            },
            OpIn::Step {
                uv0: at(f, 0.25),
                tight: 1e-12,
                plane: true,
            },
            OpIn::Line {
                uv0: at(0.1, 0.1),
                tight: 1e-12,
                uv1: at(0.9, f),
            },
        ];
        for (oi, op) in ops.into_iter().enumerate() {
            let inputs = Inputs {
                wrap: false,
                surf: AnySurf::Base(surf.clone()),
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

/// Whether the shim accepts the case (for a surface of revolution this builds it in C++).
fn valid(i: &Inputs) -> bool {
    let (op, rest) = cpp_rest(i);
    match &i.surf {
        AnySurf::Rev { rev } => {
            rev_ffi_surf(rev).is_ok_and(|s| ffi::validate_rev(op, i.wrap, &s, &rest).is_ok())
        }
        AnySurf::Base(s) => cpp_head(i, s).is_ok_and(|mut input| {
            input.extend(rest);
            ffi::validate(op, &input).is_ok()
        }),
    }
}

/// The surface of revolution cases `rev_gen0000..`: case `i` from a generator seeded
/// with [`REV_SEED`]` + i` (the first valid of up to 64 draws).
fn rev_cases(id: &str) -> Result<Vec<Case>, HarnessError> {
    let mut out = Vec::new();
    for i in 0..REV_CASES {
        let mut rng = Rng::new(REV_SEED + i as u64);
        for _ in 0..64 {
            let Ok(c) = gen_rev_case(&mut rng) else {
                continue;
            };
            if valid(&c) {
                out.push(Case {
                    unit: id.to_string(),
                    name: format!("rev_gen{i:04}"),
                    source: CaseSource::Inline,
                    inputs: serde_json::to_value(c).map_err(|e| harness(e.to_string()))?,
                });
                break;
            }
        }
    }
    Ok(out)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_point_solve_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let k = &Kernel::default();
        for _ in 0..64 {
            let Ok(i) = gen_case(k, rng) else {
                continue;
            };
            if valid(&i) {
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
        v.extend(rev_cases(self.id())?);
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse()?;
        validate(&i)?;
        rust(&i)
    }
}

#[cfg(test)]
mod tests {
    use super::super::surface_surface_cache_p1::{default_form, InlineSrf};
    use super::super::surface_surface_point_solve_p1::gen_bilinear;
    use super::*;

    #[test]
    fn op_codes_round_trip() {
        let bil: InlineSrf = gen_bilinear(&mut Rng::new(1));
        let i = Inputs {
            wrap: false,
            surf: AnySurf::Base(SurfIn::Bspline(BssIn {
                oob: false,
                form: default_form(),
                srf: SrfIn::Inline(bil),
            })),
            dom: [0.0, 0.0, 1.0, 1.0],
            solver: 3,
            test: [0.5, 0.5, 1.0],
            tol: 1e-6,
            op: OpIn::Patch {
                sub: [0.0, 0.0, 1.0, 1.0],
                nu: MAX_GRID as i32,
                nv: 1,
                uv: [0.5, 0.5],
                best: 1e300,
            },
        };
        let (op, _) = cpp_rest(&i);
        assert_eq!(op, Op::Patch);
        assert!(valid(&i));
    }
}
