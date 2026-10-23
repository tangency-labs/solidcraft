//! Differential tests of the five p3 surface-point solver entry points.
//!
//! Native construction and generated surfaces reuse p1/p2: actual planes, rational
//! and non-rational B-splines, surfaces of revolution, curve-bounded and STEP surfaces.
//! The wire result compares statuses, found flags, reasons, iterations, UV, distance,
//! object identity and restoration of the out-of-bounds flag. No absolute floor.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::global_solver::SolutionRequested;
use solidcore::geom::solution_array::{ObjectKind, Solution, SolutionObject};
use solidcore::kernel::Kernel;
use solidcore::math::EFF_ZERO;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::surface::surface::p1::StepSurface;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::surface_point_solve::SurfPtSolveStepper;
use solidcore_ffi::units::surface_surface_point_solve_p3::{self as ffi, Op};

use super::surface_surf_of_revolution_p1::{
    ffi_surf as rev_ffi, gen_surf as gen_rev, rust_surf as rev_rust, SurfIn as RevIn,
};
use super::surface_surface_cache_p1::{dom_of, flag, gen_sub_dom, harness, v3, BssIn, SrfIn};
use super::surface_surface_cache_p3::{push_surf, surf_domain, surf_size, SurfIn};
use super::surface_surface_point_solve_p1::{
    gen_surface, gen_test, gen_uv, gen_uv_any, p2, solver_op, wrapped_of, SOLVERS,
    SURFACE_SMS_POINTS,
};
use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The p3 differential runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum Geometry {
    Rev { rev: RevIn },
    Base(SurfIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default)]
    wrap: bool,
    surf: Geometry,
    dom: [f64; 4],
    solver: u8,
    test: [f64; 3],
    tol: f64,
    #[serde(flatten)]
    op: Operation,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Operation {
    Global {
        target: Option<f64>,
        request: u8,
    },
    Local {
        guess: [f64; 2],
    },
    Stepper {
        mode: u8,
        uv: [f64; 2],
        tight: f64,
        next: [f64; 2],
        iteration: i32,
        hu: i32,
        hv: i32,
    },
}

impl Operation {
    fn code(&self) -> Op {
        match self {
            Self::Global { .. } => Op::Global,
            Self::Local { .. } => Op::Local,
            Self::Stepper { mode: 0, .. } => Op::Boundary,
            Self::Stepper { mode: 1, .. } => Op::Next,
            Self::Stepper { .. } => Op::Crease,
        }
    }
}

fn tail(i: &Inputs) -> Vec<f64> {
    let mut out = i.dom.to_vec();
    out.push(f64::from(i.solver));
    out.extend(i.test);
    out.push(i.tol);
    match i.op {
        Operation::Global { target, request } => {
            out.extend([
                flag(target.is_some()),
                target.unwrap_or(0.0),
                f64::from(request),
            ]);
        }
        Operation::Local { guess } => out.extend(guess),
        Operation::Stepper {
            uv,
            tight,
            next,
            iteration,
            hu,
            hv,
            ..
        } => {
            out.extend(uv);
            out.push(tight);
            out.extend(next);
            out.extend([f64::from(iteration), f64::from(hu), f64::from(hv)]);
        }
    }
    out
}

fn record(flat: &[f64]) -> Output {
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
    match &i.surf {
        Geometry::Base(surface) => {
            let mut input = vec![flag(i.wrap)];
            push_surf(&mut input, surface)?;
            input.extend(tail(i));
            ffi::validate(i.op.code(), &input).map_err(|e| harness(e.to_string()))
        }
        Geometry::Rev { .. } => Ok(()),
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let rest = tail(i);
    let result = match &i.surf {
        Geometry::Base(surface) => {
            let mut input = vec![flag(i.wrap)];
            push_surf(&mut input, surface)?;
            input.extend(rest);
            ffi::eval(i.op.code(), &input)
        }
        Geometry::Rev { rev } => ffi::eval_rev(i.op.code(), i.wrap, &rev_ffi(rev)?, &rest),
    }
    .map_err(|e| harness(e.to_string()))?;
    Ok(record(&result))
}

#[derive(Debug, Clone, Copy, PartialEq)]
struct Object(Extent2d);

impl SolutionObject for Object {
    fn kind(&self) -> ObjectKind {
        ObjectKind::Surface
    }
    fn natural_uv_domain(&self) -> Option<Extent2d> {
        Some(self.0)
    }
}

fn geometry(k: &Kernel, i: &Inputs) -> Result<Surface, HarnessError> {
    match &i.surf {
        Geometry::Base(s) => wrapped_of(i.wrap, s),
        Geometry::Rev { rev } => {
            let surface = Surface::from(rev_rust(k, rev)?.map_err(|e| harness(e.to_string()))?);
            Ok(if i.wrap {
                Surface::from(StepSurface::new(surface, true).map_err(|e| HarnessError::new(e.to_string()))?)
            } else {
                surface
            })
        }
    }
}

fn put_solution(
    out: &mut Vec<f64>,
    solution: &Solution<Object>,
    object: Object,
) -> solidcore::Result<()> {
    out.extend([
        f64::from(solution.solution_type.code()),
        solution.start.param(0)?,
        solution.start.param(1)?,
        solution.start.solution_value,
        solution.num_objects as f64,
        solution.num_variables as f64,
        flag(solution.objects.first().is_some_and(|v| *v == Some(object))),
    ]);
    Ok(())
}

fn put_state(out: &mut Vec<f64>, stepper: &SurfPtSolveStepper<'_>) {
    let uv = stepper.uv();
    out.extend([
        uv.x,
        uv.y,
        f64::from(stepper.bound_hits_u()),
        f64::from(stepper.bound_hits_v()),
        flag(stepper.nr_step_suspect()),
        f64::from(stepper.iter_count()),
    ]);
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let surface = geometry(&k, i)?;
    let domain = dom_of(i.dom);
    let operation = solver_op(i.solver)?;
    let object = Object(surface.natural_uv_domain());
    let oob = |s: &Surface| match s.kind() {
        SurfaceKind::BSpline(b) => Some(b.is_out_of_bounds_enabled()),
        SurfaceKind::Plane(p) => Some(p.base().is_out_of_bounds_enabled()),
        SurfaceKind::SurfOfRevolution(r) => Some(r.base().is_out_of_bounds_enabled()),
        _ => None,
    };
    let before = oob(&surface);
    let run = || -> solidcore::Result<Vec<f64>> {
        let mut out = vec![0.0];
        match i.op {
            Operation::Global { target, request } => {
                let requested = SolutionRequested::from_code(i32::from(request)).ok_or(
                    solidcore::SolidError::invalid_input(
                        "difftest::surface_point_solve_p3",
                        "bad request",
                    ),
                )?;
                let result = match surface.global_point_solve_1(
                    &k,
                    &domain,
                    operation,
                    v3(i.test),
                    i.tol,
                    target,
                    requested,
                    Some(object),
                ) {
                    Ok(result) => result,
                    Err(error) if error.is_unported() => return Err(error),
                    Err(error) => {
                        // The global solver clears its output before finding a rough
                        // guess; an ordinary failure retains the empty solution array.
                        return Ok(vec![
                            super::surface_surface_cache_p1::status(&error),
                            0.0,
                            flag(oob(&surface) == before),
                        ]);
                    }
                };
                out.push(result.len() as f64);
                for solution in result.as_slice() {
                    put_solution(&mut out, solution, object)?;
                }
                out.push(flag(oob(&surface) == before));
            }
            Operation::Local { guess } => {
                let result = surface.local_point_solve_1(
                    &k,
                    &domain,
                    operation,
                    v3(i.test),
                    p2(guess),
                    Some(object),
                )?;
                out.extend([
                    flag(result.found),
                    f64::from(result.reason.code()),
                    f64::from(result.iterations),
                ]);
                put_solution(&mut out, &result.solution, object)?;
                out.push(flag(oob(&surface) == before));
            }
            Operation::Stepper {
                mode,
                uv,
                tight,
                next,
                iteration,
                hu,
                hv,
            } => {
                let mut previous = SurfPtSolveStepper::new(
                    &k,
                    &surface,
                    &domain,
                    v3(i.test),
                    p2(uv),
                    tight,
                    i.tol,
                    operation,
                )?;
                previous.set_iter_count(iteration);
                for (which, hits) in [(SurfParamType::U, hu), (SurfParamType::V, hv)] {
                    for _ in 0..hits.unsigned_abs() {
                        previous.hit_bound(which, hits.signum());
                    }
                }
                let mut stepper = SurfPtSolveStepper::copy_of(&k, &previous);
                stepper.set_uv(p2(next));
                match mode {
                    0 => out.push(flag(stepper.find_boundary_solution(&k)?)),
                    1 => {
                        let result = stepper.find_next_step(&k, &mut previous)?;
                        if let Some(status) = out.first_mut() {
                            *status = f64::from(result.status);
                        }
                        out.push(f64::from(result.reason.code()));
                    }
                    _ => {
                        let (processed, found) = stepper.check_crease(&mut previous);
                        out.extend([flag(processed), flag(found)]);
                    }
                }
                put_state(&mut out, &stepper);
            }
        }
        Ok(out)
    };
    Ok(match run() {
        Ok(out) => record(&out),
        Err(e) => Output::unported(&e)
            .unwrap_or_else(|| record(&[super::surface_surface_cache_p1::status(&e)])),
    })
}

fn candidate(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let wrap = rng.chance(0.12);
    let (surf, natural, size) = if rng.chance(0.15) {
        let rev = gen_rev(rng);
        let surface = Surface::from(rev_rust(&k, &rev).ok()?.ok()?);
        let domain = surface.natural_uv_domain();
        (
            Geometry::Rev { rev },
            [
                domain.min().x,
                domain.min().y,
                domain.max().x,
                domain.max().y,
            ],
            1.0,
        )
    } else {
        let surface = gen_surface(rng);
        let natural = surf_domain(&surface).ok()?;
        let size = surf_size(&surface).ok()?;
        (Geometry::Base(surface), natural, size)
    };
    let domain = natural;
    let mut case = Inputs {
        wrap,
        surf,
        dom: domain,
        solver: *SOLVERS.get(rng.below(5) as usize)?,
        test: [0.0; 3],
        tol: size.max(1e-12) * 1e-6,
        op: Operation::Local {
            guess: gen_uv_any(rng, domain),
        },
    };
    let surface = geometry(&k, &case).ok()?;
    let d = surface.natural_uv_domain();
    let natural = [d.min().x, d.min().y, d.max().x, d.max().y];
    let domain = if rng.chance(0.4) {
        gen_sub_dom(rng, natural)
    } else {
        natural
    };
    case.dom = domain;
    case.test = gen_test(&k, rng, &surface, domain, size);
    case.op = match rng.below(12) {
        0..=2 => Operation::Global {
            target: if rng.chance(0.5) {
                Some(rng.uniform(-1.0, 2.0))
            } else {
                None
            },
            request: rng.below(4) as u8,
        },
        3..=6 => Operation::Local {
            guess: gen_uv_any(rng, domain),
        },
        n => {
            let uv = gen_uv(rng, domain);
            Operation::Stepper {
                mode: if n == 11 { 2 } else { u8::from(n >= 9) },
                uv,
                next: if rng.chance(0.8) {
                    uv
                } else {
                    gen_uv(rng, domain)
                },
                tight: EFF_ZERO * (1.0 + v3(case.test).max_dimension()),
                iteration: i32::from(!rng.chance(0.5)),
                hu: 0,
                hv: 0,
            }
        }
    };
    Some(case)
}

// Generate only structurally valid inputs, before either engine is run. Never
// inspect solver outputs here. Fully discontinuous knot sequences are outside the
// shared native surface reader's contract; retry them as in the p1/p2 generators.
fn generated(rng: &mut Rng) -> Option<Inputs> {
    for _ in 0..64 {
        let Some(input) = candidate(rng) else {
            continue;
        };
        let rest = tail(&input);
        let valid = match &input.surf {
            Geometry::Base(surface) => {
                let mut flat = vec![flag(input.wrap)];
                if push_surf(&mut flat, surface).is_err() {
                    continue;
                }
                flat.extend(rest);
                ffi::validate(input.op.code(), &flat)
            }
            Geometry::Rev { rev } => {
                ffi::validate_rev(input.op.code(), input.wrap, &rev_ffi(rev).ok()?, &rest)
            }
        };
        if valid.is_ok() {
            return Some(input);
        }
    }
    None
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_point_solve_p3"
    }
    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i = case.parse()?;
        validate(&i)?;
        rust(&i)
    }
    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(generated(rng)?).ok()
    }
    fn cases(&self, context: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut cases = default_cases(self, context)?;
        for index in 0..1200_u64 {
            let seed = 0x5350_5350_3300_0001 + index;
            let mut rng = Rng::new(seed);
            if let Some(input) = generated(&mut rng) {
                cases.push(Case {
                    unit: self.id().to_owned(),
                    name: format!("matrix_{index:04}"),
                    source: CaseSource::Generated { seed },
                    inputs: serde_json::to_value(input).map_err(|e| harness(e.to_string()))?,
                });
            }
        }
        let surf = Geometry::Base(SurfIn::Bspline(BssIn {
            oob: false,
            form: super::surface_surface_cache_p1::default_form(),
            srf: SrfIn::File {
                sms: context
                    .upstream_test_file("Surface.sms")?
                    .to_string_lossy()
                    .into_owned(),
            },
        }));
        let input = Inputs {
            wrap: false,
            surf,
            dom: [0.0, 0.0, 1.0, 1.0],
            solver: 3,
            test: [0.0; 3],
            tol: 1e-6,
            op: Operation::Global {
                target: Some(0.03),
                request: 0,
            },
        };
        let surface = geometry(&Kernel::default(), &input)?;
        let domain = surface.natural_uv_domain();
        for (index, point) in SURFACE_SMS_POINTS.into_iter().enumerate() {
            let mut input = input.clone();
            input.dom = [
                domain.min().x,
                domain.min().y,
                domain.max().x,
                domain.max().y,
            ];
            input.test = point;
            cases.push(Case {
                unit: self.id().to_owned(),
                name: format!("upstream_closest_{index:02}"),
                source: CaseSource::Inline,
                inputs: serde_json::to_value(input).map_err(|e| harness(e.to_string()))?,
            });
        }
        Ok(cases)
    }
}
