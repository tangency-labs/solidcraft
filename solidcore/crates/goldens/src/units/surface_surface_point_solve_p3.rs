//! Runner for unit `surface.surface_point_solve_p3`: the five p3 surface-point solver
//! entry points. Ported from upstream `source/SMLib/src/SmSurfacePointSolve.cpp`; checked
//! against its golden.
//!
//! Generated surfaces reuse p1/p2: actual planes, rational
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

use super::surface_surf_of_revolution_p1::{
    gen_surf as gen_rev, rust_surf as rev_rust, SurfIn as RevIn,
};
use super::surface_surface_cache_p1::{
    dom_of, fin, flag, gen_sub_dom, harness, v3, BssIn, SrfIn, LIMITS,
};
use super::surface_surface_cache_p3::{sized_surface, surf_domain, surf_size, SurfIn};
use super::surface_surface_point_solve_p1::{
    gen_surface, gen_test, gen_uv, gen_uv_any, p2, solver_op, wrapped_of, OPS, SOLVERS,
    SURFACE_SMS_POINTS,
};
use crate::case::{CaseContext, CaseSource};
use crate::cases::contract;
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

fn record(flat: &[f64]) -> Output {
    let (status, rest) = flat
        .split_first()
        .map_or((f64::NAN, &[][..]), |(s, r)| (*s, r));
    Output::new()
        .values("status", vec![status])
        .values("out", rest.to_vec())
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
                Surface::from(
                    StepSurface::new(surface, true)
                        .map_err(|e| HarnessError::new(e.to_string()))?,
                )
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
    // the object key is read from a second build of the geometry: a query on the
    // solved surface would make and keep a lazy plane's NURB, which upstream does not
    let object = Object(
        geometry(&k, i)?
            .natural_uv_domain_with_nurb(&k)
            .map_err(|e| HarnessError::new(e.to_string()))?,
    );
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
        // (runner helper that does not return Result)
        let domain = surface.natural_uv_domain_if_built();
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
    // (runner helper that does not return Result)
    let d = surface.natural_uv_domain_if_built();
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

// ---- input contract -----------------------------------------------------------------

/// The surface domain of a case's geometry, if it keeps the contract: a plane with finite
/// data, axes whose cross product is finite and non-zero, non-zero scales and a finite
/// ordered domain (no cache-building limits: this solver builds no tree); a B-spline or
/// curve bounded surface as `surface_cache_p3` takes it; a surface of revolution that
/// builds with a finite, strictly ordered natural domain (of the wrapped surface when
/// wrapped).
fn geometry_domain(i: &Inputs) -> Option<[f64; 4]> {
    match &i.surf {
        Geometry::Base(SurfIn::Plane(p)) => {
            let finite3 = |v: &[f64; 3]| v.iter().all(|c| c.is_finite());
            let [x0, x1, x2] = p.x;
            let [y0, y1, y2] = p.y;
            let cross = [x1 * y2 - x2 * y1, x2 * y0 - x0 * y2, x0 * y1 - x1 * y0];
            let ok = finite3(&p.o)
                && finite3(&p.x)
                && finite3(&p.y)
                && cross.iter().any(|&c| c != 0.0)
                && cross.iter().all(|c| c.is_finite())
                && p.scale.iter().all(|&c| c.is_finite() && c != 0.0)
                && contract::domain(&p.dom, LIMITS.max_coord);
            ok.then_some(p.dom)
        }
        Geometry::Base(s) => sized_surface(s).map(|(d, _, _)| d),
        Geometry::Rev { .. } => {
            let k = Kernel::default();
            let nd = geometry(&k, i).ok()?.natural_uv_domain_with_nurb(&k).ok()?;
            let d = [nd.min().x, nd.min().y, nd.max().x, nd.max().y];
            (d.iter().all(|v| v.is_finite()) && d[0] < d[2] && d[1] < d[3]).then_some(d)
        }
    }
}

/// Whether a case keeps the input contract ported from the reference's wrapper: a
/// [`geometry_domain`], a solver domain of non-degenerate sides inside it, a supported
/// solver operation, finite values, a global request up to 3, and stepper iterations up to
/// 35 with integer boundary counters up to 64 in magnitude.
fn keeps_contract(i: &Inputs) -> bool {
    let Some(d) = geometry_domain(i) else {
        return false;
    };
    let base = matches!(i.surf, Geometry::Base(_));
    let finite = |v: f64| if base { v.is_finite() } else { fin(v) };
    let problem = contract::domain_in(&i.dom, &d, true, LIMITS.max_coord)
        && OPS.contains(&i.solver)
        && i.test.iter().all(|&c| finite(c))
        && finite(i.tol);
    problem
        && match &i.op {
            Operation::Global { target, request } => fin(target.unwrap_or(0.0)) && *request <= 3,
            Operation::Local { guess } => guess.iter().all(|&c| fin(c)),
            Operation::Stepper {
                uv,
                tight,
                next,
                iteration,
                hu,
                hv,
                ..
            } => {
                uv.iter().chain(next).all(|&c| fin(c))
                    && fin(*tight)
                    && (0..=35).contains(iteration)
                    && hu.unsigned_abs() <= 64
                    && hv.unsigned_abs() <= 64
            }
        }
}

// Generate inputs before the engine is run; never inspect solver outputs here.
fn generated(rng: &mut Rng) -> Option<Inputs> {
    (0..64).find_map(|_| candidate(rng).filter(keeps_contract))
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_point_solve_p3"
    }
    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
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
        // (runner helper without a kernel in scope)
        let domain = surface.natural_uv_domain_if_built();
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

/// Witness challenges of the optima the point solves report, shared by this unit's and the
/// `surface.surface_point_solve_p2` runner's tests.
///
/// A solve claims an optimum when its operation ranks points: minimise (the nearest point
/// to the test point), maximise (the farthest) and the 3D signed directed maximise (the
/// farthest along the direction the test point carries). Its objective at `uv`, larger is
/// better, is the distance, the negated distance, or the surface point dotted with the unit
/// direction; for the directed maximise only differences matter (the solver minimises along
/// the negated direction from a corner of the surface's box). Intersect and normalise
/// rank nothing: they claim a point on the surface, or a normal projection.
///
/// What a witness can and cannot show: a witness is an in-domain point whose objective
/// beats the reported optimum's by more than the solve's tolerance. One such point refutes
/// the claimed optimum. No witness search proves an optimum: a solve that survives the
/// challenge may still be wrong between the points tried.
#[cfg(test)]
pub(crate) mod witness {
    use solidcore::geom::global_solver::SolverOperation;
    use solidcore::kernel::Kernel;
    use solidcore::surface::surface::Surface;

    use super::{p2, v3};
    use crate::golden::KnownFailing;

    /// The issue a refuted maximise is listed under. The kernel's
    /// `Surface::global_point_solve` sends a maximise to the older solver
    /// (`global_point_solve_0`), so these answers come from a path no kernel caller takes
    /// for that operation.
    pub(crate) const MAXIMUM_ISSUE: &str = "tangency-labs/solidcraft#13684";
    /// The issue a refuted minimise or directed maximise is listed under: the router sends
    /// these operations to this solver, so the kernel's callers reach these answers.
    pub(crate) const REACHED_ISSUE: &str = "tangency-labs/solidcraft#20483";

    /// Points per side of the global grid, less one.
    const GRID: u32 = 200;

    /// Whether `op` ranks points, so that its answer claims an optimum.
    pub(crate) fn ranks(op: SolverOperation) -> bool {
        matches!(
            op,
            SolverOperation::Minimize
                | SolverOperation::Maximize
                | SolverOperation::Signed3dDirectedMaximize
        )
    }

    /// The objective `op` optimises at `uv`, larger is better (see the module doc).
    pub(crate) fn objective(
        k: &Kernel,
        surface: &Surface,
        op: SolverOperation,
        test: [f64; 3],
        uv: [f64; 2],
    ) -> f64 {
        let at = surface.evaluate_point(k, p2(uv)).unwrap();
        match op {
            SolverOperation::Maximize => at.distance(v3(test)),
            SolverOperation::Minimize => -at.distance(v3(test)),
            SolverOperation::Signed3dDirectedMaximize => {
                let d = v3(test);
                at.dot(d) / d.length()
            }
            other => panic!("{other:?} ranks no points"),
        }
    }

    /// One reported optimum and its best witness.
    #[derive(Debug)]
    pub(crate) struct Claim {
        pub(crate) name: String,
        pub(crate) op: SolverOperation,
        /// The kind of solve, for the report (`global request <code>`).
        pub(crate) solve: String,
        /// The record's value at its parameters (a distance; the directed maximise's
        /// error norm).
        pub(crate) value: f64,
        /// The objective at the reported parameters, and those parameters.
        pub(crate) reported: (f64, [f64; 2]),
        /// The best witness's objective, and where.
        pub(crate) witness: (f64, [f64; 2]),
        pub(crate) tol: f64,
    }

    impl Claim {
        /// Whether the witness beats the reported optimum by more than the tolerance.
        pub(crate) fn refuted(&self) -> bool {
            self.witness.0 > self.reported.0 + self.tol
        }

        /// The issue the claim is listed under when it is refuted.
        pub(crate) fn issue(&self) -> &'static str {
            if self.op == SolverOperation::Maximize {
                MAXIMUM_ISSUE
            } else {
                REACHED_ISSUE
            }
        }
    }

    /// The record's value must be the surface's at the record's parameters: the distance
    /// to the test point (the directed maximise's value is an error norm from a point the
    /// solver picks, not checked).
    fn check_value(
        k: &Kernel,
        surface: &Surface,
        name: &str,
        op: SolverOperation,
        test: [f64; 3],
        uv: [f64; 2],
        value: f64,
    ) {
        if op != SolverOperation::Signed3dDirectedMaximize {
            let actual = objective(k, surface, op, test, uv).abs();
            assert!(
                (actual - value).abs() < 1e-11 * actual.max(1.0),
                "{name}: the surface at {uv:?} is {actual}, the record says {value}"
            );
        }
    }

    /// A global solve's claim: its best solution (every solution is checked against the
    /// surface) against the best of a 201 by 201 grid over `dom`, corners and edges
    /// included.
    #[allow(clippy::too_many_arguments)]
    pub(crate) fn global(
        k: &Kernel,
        surface: &Surface,
        name: &str,
        op: SolverOperation,
        solve: String,
        test: [f64; 3],
        dom: [f64; 4],
        tol: f64,
        solutions: &[([f64; 2], f64)],
    ) -> Claim {
        assert!(!solutions.is_empty(), "{name}: no solution");
        let mut reported = (f64::NEG_INFINITY, [0.0; 2]);
        let mut value = f64::NAN;
        for &(uv, v) in solutions {
            check_value(k, surface, name, op, test, uv, v);
            let f = objective(k, surface, op, test, uv);
            if f > reported.0 {
                reported = (f, uv);
                value = v;
            }
        }
        let mut witness = (f64::NEG_INFINITY, [0.0; 2]);
        for iu in 0..=GRID {
            for iv in 0..=GRID {
                let uv = [
                    dom[0] + (dom[2] - dom[0]) * f64::from(iu) / f64::from(GRID),
                    dom[1] + (dom[3] - dom[1]) * f64::from(iv) / f64::from(GRID),
                ];
                let f = objective(k, surface, op, test, uv);
                if f > witness.0 {
                    witness = (f, uv);
                }
            }
        }
        Claim {
            name: name.to_owned(),
            op,
            solve,
            value,
            reported,
            witness,
            tol,
        }
    }

    /// Whether a known-failing reason lists its case under `issue`.
    fn listed(why: &str, issue: &str) -> bool {
        why.strip_prefix(issue)
            .is_some_and(|rest| rest.starts_with(':'))
    }

    /// Every refuted claim must be listed known failing under its class's issue
    /// ([`Claim::issue`]), and every case listed under either issue must be a refuted
    /// claim (so a fix shows). Prints the claims and refutations per operation.
    pub(crate) fn check(unit: &str, claims: &[Claim], known: &KnownFailing) {
        let mut unexplained = Vec::new();
        for c in claims.iter().filter(|c| c.refuted()) {
            eprintln!(
                "{unit} {} {:?} {}: value {:?}, objective {:?} at {:?}; witness {:?} at {:?} \
                 (better by {:e}, tol {:e})",
                c.name,
                c.op,
                c.solve,
                c.value,
                c.reported.0,
                c.reported.1,
                c.witness.0,
                c.witness.1,
                c.witness.0 - c.reported.0,
                c.tol
            );
            if !known.get(&c.name).is_some_and(|why| listed(why, c.issue())) {
                unexplained.push(format!("{} ({})", c.name, c.issue()));
            }
        }
        let stale: Vec<&String> = known
            .iter()
            .filter(|(_, why)| listed(why, MAXIMUM_ISSUE) || listed(why, REACHED_ISSUE))
            .map(|(name, _)| name)
            .filter(|name| !claims.iter().any(|c| &c.name == *name && c.refuted()))
            .collect();
        eprintln!(
            "{unit}: {} claimed optima, {} refuted",
            claims.len(),
            claims.iter().filter(|c| c.refuted()).count()
        );
        for op in [
            SolverOperation::Minimize,
            SolverOperation::Maximize,
            SolverOperation::Signed3dDirectedMaximize,
        ] {
            let of_op = claims.iter().filter(|c| c.op == op);
            eprintln!(
                "{unit}: {op:?}: {} claimed, {} refuted",
                of_op.clone().count(),
                of_op.filter(|c| c.refuted()).count()
            );
        }
        assert!(
            unexplained.is_empty(),
            "optima refuted but not listed known failing: {unexplained:?}"
        );
        assert!(
            stale.is_empty(),
            "listed known failing but no witness refutes them (fixed?): {stale:?}"
        );
    }
}

#[cfg(test)]
mod review_geometry_tests {
    use super::*;
    use solidcore::geom::global_solver::SolverOperation;

    /// Every global solve of the unit that claims an optimum is challenged by a grid of
    /// in-domain witnesses over its domain ([`witness`]: one witness refutes an optimum,
    /// none proves one).
    ///
    /// Which cases claim one, from their inputs and status: a global solve
    /// ([`Operation::Global`]) of an operation that [`witness::ranks`] points, that
    /// succeeded and returned a solution. Every request type claims the same optimum:
    /// `SurfPtGlobalSolver` finds one candidate and returns it with its across-seam
    /// partners, so a nodes, all or find-ambiguities request is answered like a single
    /// one (#20438); a target distance only bounds that answer out. Local solves and
    /// single steps are not challenged here: what a local solve's answer promises for
    /// each termination reason is not settled.
    ///
    /// A successful solve with no solution claims nothing and is counted, not challenged.
    /// Every global solve that succeeds records whether the surface's out-of-bounds
    /// setting was restored; that must hold for each one, refuted or not (a refuted case
    /// has no golden line to pin it).
    ///
    /// A witness better by more than the case's `tol` (the solve's distance tolerance)
    /// refutes the solve, and the case must be listed known failing under
    /// [`witness::MAXIMUM_ISSUE`] (maximise) or [`witness::REACHED_ISSUE`] (minimise,
    /// directed maximise); a case listed under either must still be refuted.
    #[test]
    fn review_every_claimed_optimum_survives_its_witnesses() {
        let k = Kernel::default();
        let known =
            crate::golden::known_failing(&crate::golden::default_dir(), Runner.id()).unwrap();
        let context = CaseContext {
            root: crate::case::repo_root(),
            seeds: 16,
        };
        let mut claims = Vec::new();
        let mut empty = std::collections::BTreeMap::<String, usize>::new();
        for case in Runner.cases(&context).unwrap() {
            let name = case.name.as_str();
            let input: Inputs = case.parse().unwrap();
            let op = solver_op(input.solver).unwrap();
            let Operation::Global { request, .. } = input.op else {
                continue;
            };
            if !witness::ranks(op) {
                continue;
            }
            let output = rust(&input).unwrap();
            if output.deferred.is_some() || output.values["status"] != [0.0] {
                continue;
            }
            let values = &output.values["out"];
            let n = values[0] as usize;
            // (the record's last value: the out-of-bounds setting was restored)
            assert_eq!(values.get(1 + 7 * n), Some(&1.0), "{name}: {values:?}");
            if n == 0 {
                *empty.entry(format!("{op:?}")).or_insert(0) += 1;
                continue;
            }
            let solutions: Vec<([f64; 2], f64)> = (0..n)
                .map(|j| ([values[2 + 7 * j], values[3 + 7 * j]], values[4 + 7 * j]))
                .collect();
            let surface = geometry(&k, &input).unwrap();
            claims.push(witness::global(
                &k,
                &surface,
                name,
                op,
                format!("global request {request}"),
                input.test,
                input.dom,
                input.tol,
                &solutions,
            ));
        }
        eprintln!(
            "{}: succeeded with no solution, by operation: {empty:?}",
            Runner.id()
        );
        witness::check(Runner.id(), &claims, &known);
    }

    /// WR-57: a maximize stepping into a singular point keeps the farthest iso curve
    /// drop, and the guess where every drop is proven nearer, reported unconverged
    /// (nothing shows the guess is a maximum); the nearest drop ended these local
    /// maximize solves nearer the test point than the main branch's. `matrix_0105` keeps
    /// main's `UnableToConverge` (code 5) at main's point; the others end at least as
    /// far as main's.
    #[test]
    fn wr57_maximize_keeps_the_farthest_singular_answer() {
        let k = Kernel::default();
        for (name, main, unconverged) in [
            ("matrix_0105", 21_900.553_430_444_63, true),
            ("matrix_0822", 0.592_092_503_846_087, false),
            ("matrix_0957", 1.163_218_054_077_855_2, false),
        ] {
            let case = crate::case::regression_case(&Runner, name);
            let input: Inputs = case.parse().unwrap();
            assert_eq!(solver_op(input.solver).unwrap(), SolverOperation::Maximize);
            assert!(matches!(input.op, Operation::Local { .. }), "{name}");
            let surface = geometry(&k, &input).unwrap();
            let output = rust(&input).unwrap();
            let values = &output.values["out"];
            let reported = values[6];
            let actual = surface
                .evaluate_point(&k, p2([values[4], values[5]]))
                .unwrap()
                .distance(v3(input.test));
            assert!(
                (actual - reported).abs() <= 1e-12 * actual,
                "{name}: {actual} {reported}"
            );
            assert!(reported >= main, "{name}: {reported} < {main}");
            if unconverged {
                assert_eq!(values[1], 5.0, "{name}: {values:?}");
                assert!(
                    (reported - main).abs() <= 1e-12 * main,
                    "{name}: {reported} {main}"
                );
            }
        }
    }

    /// The local solves WR-57's retirement moved (#17856): each case's name, main's row
    /// (3740d2254) and the new row (`values[1]` the reason, `[4..6]` the answer's `u`, `v`
    /// and distance).
    #[rustfmt::skip]
    const WR57_MOVED: &[(&str, &[f64], &[f64])] = &[
        ("matrix_0511", &[1.0, 1.0, 0.0, 1.0, -1.5818805874435007, -1.1520818153828836, 3.133991971692754e-15, 1.0, 0.0, 1.0, 1.0], &[1.0, 1.0, 0.0, 1.0, -1.5818805874435011, -1.152081815382878, 0.0, 1.0, 0.0, 1.0, 1.0]),
        ("matrix_0698", &[1.0, 1.0, 0.0, 1.0, 1.2086437884418642, 0.05049691621437583, 4.244835760994505, 1.0, 0.0, 1.0, 1.0], &[1.0, 1.0, 0.0, 1.0, 1.2086437884418642, 0.9662401036934778, 1.7281261817905709, 1.0, 0.0, 1.0, 1.0]),
        ("matrix_0822", &[1.0, 1.0, 0.0, 1.0, 0.4127548407697886, 0.8200607287797311, 0.592092503846087, 1.0, 0.0, 1.0, 1.0], &[1.0, 5.0, 0.0, 1.0, 0.2402072156240318, 0.0, 0.6947377622543364, 1.0, 0.0, 1.0, 1.0]),
        ("matrix_0957", &[1.0, 1.0, 0.0, 1.0, 2.198184284773183, 0.5320965165959111, 1.1632180540778552, 1.0, 0.0, 1.0, 1.0], &[1.0, 5.0, 0.0, 1.0, 0.5706589560298241, 0.053094881626908896, 3.4366983956367245, 1.0, 0.0, 1.0, 1.0]),
        ("matrix_1074", &[1.0, 1.0, 0.0, 1.0, -0.4281683613170542, -0.01020393315696179, 7.943795879082694e-07, 1.0, 0.0, 1.0, 1.0], &[1.0, 1.0, 0.0, 1.0, -0.42816836132035424, -0.010203933142704728, 7.943795876004761e-07, 1.0, 0.0, 1.0, 1.0]),
        ("review106_wrapped_cone_1_solver_4", &[1.0, 1.0, 0.0, 1.0, 5.729577951308233e-08, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0], &[1.0, 1.0, 0.0, 1.0, 5.7295779513082324e-08, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0]),
    ];

    /// WR-57 (#17856): every local solve that moved from main is replayed (the row agrees
    /// with the golden: the code checked is the code that blessed) and certified with
    /// enclosed distances from the test point (the evaluation's rounding
    /// [`crate::certify::evaluation_slack_at`], grown past the knots, and the distance's own
    /// charged): a minimize's or
    /// intersection's new answer is not proven farther than main's, a maximize's not proven nearer (each
    /// proven better where the enclosures part), and another operation's is main's point
    /// to the rounding (the enclosures meet). A surface without a NURBS form (the
    /// evaluation unbounded) must answer main's parameters to an ulp each.
    #[test]
    fn wr57_moved_local_solves_are_not_worse() {
        let k = Kernel::default();
        let mut proven_better = 0usize;
        for &(name, main, new) in WR57_MOVED {
            let case = crate::case::regression_case(&Runner, name);
            let input: Inputs = case.parse().unwrap();
            let row = &rust(&input).unwrap().values["out"];
            assert!(
                row.len() == new.len()
                    && row
                        .iter()
                        .zip(new)
                        .all(|(a, b)| (a - b).abs() <= crate::REL_TOL * a.abs().max(b.abs())),
                "{name}: the code gives {row:?}, not the new row"
            );
            assert!(matches!(input.op, Operation::Local { .. }), "{name}");
            let surface = geometry(&k, &input).unwrap();
            let nurbs = solidcore::blend::fillet_section::station::as_b_spline(
                &k,
                crate::certify::measured(&surface),
            )
            .unwrap()
            .is_some();
            if !nurbs {
                // (no NURBS form to bound the evaluation by: the answer must be main's
                // parameters to an ulp each, the same point)
                let ulp = |a: f64, b: f64| a == b || a.next_up() == b || a.next_down() == b;
                assert!(
                    ulp(main[4], new[4]) && ulp(main[5], new[5]),
                    "{name}: main {main:?}, new {new:?}"
                );
                eprintln!("{name}: main's parameters to an ulp, main {main:?} new {new:?}");
                continue;
            }
            let at = |r: &[f64]| {
                let uv = p2([r[4], r[5]]);
                let q = surface.evaluate_point(&k, uv).unwrap();
                let d = solidcore::geom::certified::distance(q, v3(input.test));
                let slack = crate::certify::evaluation_slack_at(&k, &surface, uv)?;
                Some((d.value - d.bound - slack, d.value + d.bound + slack))
            };
            let (Some(m), Some(n)) = (at(main), at(new)) else {
                panic!("{name}: past a rational net's knots, no enclosure (main {main:?}, new {new:?})");
            };
            match solver_op(input.solver).unwrap() {
                SolverOperation::Maximize => {
                    assert!(
                        n.1 >= m.0,
                        "{name}: new {n:?} proven nearer than main {m:?}"
                    );
                    proven_better += usize::from(n.0 > m.1);
                }
                // (an intersection with the point minimizes its distance to zero)
                SolverOperation::Minimize | SolverOperation::Intersect => {
                    assert!(
                        n.0 <= m.1,
                        "{name}: new {n:?} proven farther than main {m:?}"
                    );
                    proven_better += usize::from(n.1 < m.0);
                }
                op => {
                    // (another operation's answer is not ranked by its distance: the
                    // moved row must be main's point to the rounding)
                    assert!(
                        n.0 <= m.1 && m.0 <= n.1,
                        "{name} ({op:?}): new {n:?}, main {m:?}"
                    );
                }
            }
            eprintln!(
                "{name} ({:?}): main {m:?} new {n:?}",
                solver_op(input.solver).unwrap()
            );
        }
        eprintln!("{proven_better} answers proven better than main's");
        assert_eq!(proven_better, 4, "the certified count moved");
    }

    /// The global solves the target distance bounded out (#20438): each named case's solve
    /// now answers no solution, where before the candidate beyond the target was returned.
    #[rustfmt::skip]
    const TARGET_BOUNDED: &[&str] = &[
        "matrix_0006", "matrix_0034", "matrix_0236", "matrix_0251", "matrix_0281",
        "matrix_0301", "matrix_0334", "matrix_0452", "matrix_0471", "matrix_0531",
        "matrix_0562", "matrix_0611", "matrix_0690", "matrix_0702", "matrix_0729",
        "matrix_0897", "matrix_0898", "matrix_0901", "matrix_1009", "matrix_1087",
        "matrix_1096", "matrix_1098", "matrix_1155", "plane_scale_1.0_solver_3_30",
        "plane_scale_100000000.0_solver_3_55",
    ];

    /// #20438: every solve the target bounded out is replayed (it answers no solution) and
    /// certified empty, as the tree solve culls (`AddSortedSolution`): each is a minimize
    /// (the only operation the global solve bounds) whose surface is proven farther than
    /// `|target| + tol` over the whole solve domain ([`crate::certify::domain_lower_bound`]).
    /// A case not proven is listed by name and fails the test.
    #[test]
    fn target_bounded_solves_are_certified_empty() {
        let k = Kernel::default();
        let mut unproven = Vec::new();
        for &name in TARGET_BOUNDED {
            let case = crate::case::regression_case(&Runner, name);
            let input: Inputs = case.parse().unwrap();
            let Operation::Global {
                target: Some(target),
                ..
            } = input.op
            else {
                panic!("{name}: not a global solve with a target");
            };
            let output = rust(&input).unwrap();
            assert_eq!(output.values["status"], [0.0], "{name}");
            assert_eq!(
                output.values["out"],
                [0.0, 1.0],
                "{name}: not answered empty"
            );
            let surface = geometry(&k, &input).unwrap();
            let dom = dom_of(input.dom);
            let p = v3(input.test);
            let op = solver_op(input.solver).unwrap();
            assert_eq!(
                op,
                SolverOperation::Minimize,
                "{name}: only a minimize is bounded"
            );
            let want = target.abs() + input.tol;
            let bound = crate::certify::domain_lower_bound(&k, &surface, &dom, p, want);
            let proved = bound.is_some_and(|lo| lo > want);
            eprintln!(
                "{name} ({op:?}): target {target:e} tol {:e} bound {bound:?} proved {proved}",
                input.tol
            );
            if !proved {
                unproven.push(name);
            }
        }
        assert!(unproven.is_empty(), "not certified empty: {unproven:?}");
    }
}
