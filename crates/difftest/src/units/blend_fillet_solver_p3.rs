//! Runner for unit `blend.fillet_solver_p3`. C++: `solidcore_ffi::units::blend_fillet_solver_p3`.
//! Rust: `solidcore::blend::fillet_solver::p3`.
//!
//! `step` runs `SmFilletSolver::StepSolve` on both sides with the same deterministic solver
//! (the shim's `P3FS`, mirrored by [`HarnessSolver`]): the offset/offset intersection of a
//! floor and a wall meeting at an edge (B-spline surfaces with linear parametrisation,
//! optionally bumped), at angles between 20 and 160 degrees and scales from `1e-3` to
//! `1e3`, from a point on the fillet centre line (perturbed or not), stepping inside the
//! domains, across a domain boundary (clipped: the iso-curve/offset-surface intersection
//! for unknown and constant radius solvers, the clipped radius solve of a constant
//! distance solver, the constant-radius-assisted refinement, which defers at
//! `PointOnPlaneSolve`), from a boundary outwards, with zero, tiny and reversed steps,
//! extended domains (offsets allowed to extend), offsets on the wrong side (no
//! intersection), a failing `LoadInitialValues` and an extra unknown loaded into the
//! user doubles.
//!
//! The other routines of the unit (`GetOtherFilletCorner`, `sm_FindFilletTsectPnt`,
//! `SplitAndTrimFilletSurface`, `sm_RefineRailCurveClassification`, `sm_CalcRailXEdge`,
//! `ProcessFullRollOver`) reach unported callees (`blend.fillet_solver_p2`,
//! `blend.fillet_geom_p5`) on every path a native fixture can build without the
//! executive's pseudo B-rep; their comparable early exits are covered by kernel tests.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::blend::fillet_custom_solver::{
    InitialValues, JacobianLoad, OffsetSurface, SurfaceId,
};
use solidcore::blend::fillet_solver::p1::{
    FilletSolver, FilletSolverOps, FilletSolverType, OffsetSurfaceSlot,
};
use solidcore::blend::fillet_solver::p3::StepSolveOutcome;
use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use solidcore::kernel::Kernel;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::NOW;
use solidcore::surface::offset_surface::SS_UNKNOWN;
use solidcore::surface::surface::p1::evaluate_1st_derivatives;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::blend_fillet_solver_p3::{self as ffi, Op};

use super::topology_edgeuse_p1::{push_surface, surface_of, BssIn, SrfIn};
use crate::{HarnessError, Output, Rng, UnitDiff};

/// The runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// One `StepSolve` case.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct StepIn {
    /// `SmFilletSolverType` of the solver.
    ty: u8,
    tol: f64,
    radius: f64,
    orient: [f64; 2],
    distance: f64,
    #[serde(default)]
    extra: bool,
    #[serde(default)]
    init_fail: bool,
    #[serde(default)]
    allow_ext: bool,
    s0: SrfIn,
    s1: SrfIn,
    uv0: [f64; 2],
    uv1: [f64; 2],
    duv0: [f64; 2],
    duv1: [f64; 2],
    crv_pos: [f64; 3],
    crv_deriv: [f64; 3],
    step: f64,
    dom1: [f64; 4],
    dom2: [f64; 4],
    next_uv0: [f64; 2],
    next_uv1: [f64; 2],
    next_ud: [f64; 4],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Step(Box<StepIn>),
}

fn solver_type(ty: u8) -> Result<FilletSolverType, HarnessError> {
    Ok(match ty {
        0 => FilletSolverType::Unknown,
        1 => FilletSolverType::SurfSurf,
        2 => FilletSolverType::ConstRadius,
        3 => FilletSolverType::VariableRadius,
        4 => FilletSolverType::ConstDist,
        5 => FilletSolverType::CurveBased,
        6 => FilletSolverType::ConstRadiusAssisted,
        _ => return Err(harness(format!("bad solver type {ty}"))),
    })
}

/// The shim's `P3FS` (see its module docs).
struct HarnessSolver {
    base: FilletSolver,
    ty: FilletSolverType,
    radius: f64,
    distance: f64,
    extra: bool,
    init_fail: bool,
}

/// `GetSurface(i)->Evaluate1stDerivatives(uv, TRUE, TRUE, ..)` (on a copy, as the port).
fn offset_1st(
    k: &Kernel,
    fs: &FilletSolver,
    i: usize,
    uv: Point2,
) -> solidcore::Result<(Vec3, Vec3, Vec3)> {
    let mut s = fs
        .surfaces
        .get(i)
        .and_then(Option::as_ref)
        .ok_or(SolidError::sm_err("harness::offset_1st"))?
        .surface
        .clone();
    s.surface
        .with_evaluator(|e| evaluate_1st_derivatives(k, e, uv, Side::Left, Side::Left))
}

fn set_j(j: &mut Matrix, r: usize, c: usize, v: f64) {
    if r < j.num_rows() && c < j.num_columns() {
        let _ = j.set_at(r, c, v);
    }
}

impl FilletSolverOps for HarnessSolver {
    fn solver(&self) -> &FilletSolver {
        &self.base
    }

    fn solver_mut(&mut self) -> &mut FilletSolver {
        &mut self.base
    }

    fn solver_type(&self) -> solidcore::Result<FilletSolverType> {
        Ok(self.ty)
    }

    fn constant_distance(&self) -> solidcore::Result<f64> {
        Ok(self.distance)
    }

    fn jacobian_size(&self) -> solidcore::Result<usize> {
        Ok(if self.extra { 5 } else { 4 })
    }

    fn setup_offset_values(
        &mut self,
        _tsect: &TsectPnt,
        offset_dist: Option<&mut f64>,
    ) -> solidcore::Result<()> {
        for i in 0..2 {
            let o = self.base.orientations.get(i).copied().unwrap_or(1.0);
            if let Some(s) = self.base.surfaces.get_mut(i).and_then(Option::as_mut) {
                s.surface.set_offset_distance(self.radius * o);
            }
        }
        if let Some(d) = offset_dist {
            *d = self.radius;
        }
        Ok(())
    }

    fn load_initial_values(
        &mut self,
        _k: &Kernel,
        _brep: Option<&mut Brep>,
        _do_surf2_calcs: bool,
        _rail_index: usize,
        _surface1: &mut OffsetSurface,
        _surface1_index: usize,
        intervals: &mut ExtentNd,
        periodicities: &mut Vec<bool>,
        guess_t: &mut Vec<f64>,
    ) -> solidcore::Result<Option<InitialValues>> {
        if self.init_fail {
            return Err(SolidError::sm_err("harness::load_initial_values"));
        }
        if self.extra {
            if let Some(e) = intervals.get_mut(4) {
                *e = Extent1d::from_min_max(0.0, 1000.0);
            }
            periodicities.push(false);
            guess_t.push(self.radius);
        }
        Ok(None)
    }

    fn load_jacobian(
        &mut self,
        k: &Kernel,
        ne: &mut usize,
        np: &mut usize,
        load: &mut JacobianLoad<'_>,
        f: &mut [f64],
        mut jacobian: Option<&mut Matrix>,
    ) -> solidcore::Result<bool> {
        const OP: &str = "harness::load_jacobian";
        let x = load.x;
        let o1 = load.surf1_offset;
        if o1 + 1 >= x.len() || *np + 1 >= x.len() || *ne + 2 >= f.len() {
            return Err(SolidError::sm_err(OP));
        }
        let at = |i: usize| x.get(i).copied().ok_or(SolidError::sm_err(OP));
        let (p, pu, pv) = offset_1st(k, &self.base, 0, Point2::new(at(o1)?, at(o1 + 1)?))?;
        let (q, qu, qv) = offset_1st(k, &self.base, 1, Point2::new(at(*np)?, at(*np + 1)?))?;
        let d = p - q;
        let rows = [d.x, d.y, d.z];
        for (r, v) in rows.iter().enumerate() {
            if let Some(slot) = f.get_mut(*ne + r) {
                *slot = *v;
            }
        }
        if let Some(j) = jacobian.as_deref_mut() {
            if *ne + 2 < j.num_rows() && *np + 1 < j.num_columns() && o1 + 1 < j.num_columns() {
                let cols = [
                    ([pu.x, pu.y, pu.z], o1, 1.0),
                    ([pv.x, pv.y, pv.z], o1 + 1, 1.0),
                    ([qu.x, qu.y, qu.z], *np, -1.0),
                    ([qv.x, qv.y, qv.z], *np + 1, -1.0),
                ];
                for (vals, col, sign) in cols {
                    for (i, v) in vals.into_iter().enumerate() {
                        set_j(j, *ne + i, col, if sign < 0.0 { -v } else { v });
                    }
                }
            }
        }
        load.surf2_offset = *np;
        let mut big = d.x.abs();
        if d.y.abs() > big {
            big = d.y.abs();
        }
        if d.z.abs() > big {
            big = d.z.abs();
        }
        *ne += 3;
        *np += 2;
        if self.extra {
            if *np >= x.len() || *ne >= f.len() {
                return Err(SolidError::sm_err(OP));
            }
            let v = at(*np)? - self.radius;
            if let Some(slot) = f.get_mut(*ne) {
                *slot = v;
            }
            if let Some(j) = jacobian {
                set_j(j, *ne, *np, 1.0);
            }
            if v.abs() > big {
                big = v.abs();
            }
            *ne += 1;
            *np += 1;
        }
        Ok(big < 1.0e-12)
    }
}

fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn domain_of(d: [f64; 4]) -> Extent2d {
    Extent2d::from_bounds(d[0], d[1], d[2], d[3])
}

fn cpp_step(i: &StepIn) -> Result<Output, HarnessError> {
    let mut v = vec![
        f64::from(i.ty),
        i.tol,
        i.radius,
        i.orient[0],
        i.orient[1],
        i.distance,
        b(i.extra),
        b(i.init_fail),
        b(i.allow_ext),
    ];
    push_surface(&mut v, &i.s0)?;
    push_surface(&mut v, &i.s1)?;
    for a in [i.uv0, i.uv1, i.duv0, i.duv1] {
        v.extend(a);
    }
    v.extend(i.crv_pos);
    v.extend(i.crv_deriv);
    v.push(i.step);
    v.extend(i.dom1);
    v.extend(i.dom2);
    v.extend(i.next_uv0);
    v.extend(i.next_uv1);
    v.extend(i.next_ud);
    match ffi::eval(Op::Step, &v) {
        Ok(o) => Ok(Output::new().values("flat", o)),
        Err(e) => Err(harness(e.to_string())),
    }
}

fn rust_step(i: &StepIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let ty = solver_type(i.ty)?;
    let mut fs = HarnessSolver {
        base: FilletSolver::new(i.tol, 0.1, 0.01),
        ty,
        radius: i.radius,
        distance: i.distance,
        extra: i.extra,
        init_fail: i.init_fail,
    };
    fs.base.orientations = i.orient;
    for (rail, (srf, orient)) in [&i.s0, &i.s1].into_iter().zip(i.orient).enumerate() {
        let base = surface_of(srf)?;
        let d = i.radius * orient;
        let mut s = match OffsetSurface::new(&k, d, base, SurfaceId::Free(rail as u32), SS_UNKNOWN)
        {
            Ok(s) => s,
            Err(e) => {
                return Output::unported(&e).ok_or_else(|| harness(format!("offset surface: {e}")))
            }
        };
        if i.allow_ext {
            s.set_allow_extension(true);
        }
        fs.base
            .set_surface(
                rail,
                Some(OffsetSurfaceSlot {
                    key: rail as u64,
                    surface: s,
                }),
            )
            .map_err(|e| harness(e.to_string()))?;
    }
    let mut cur = TsectPnt::new();
    *cur.uv_pos_mut(SrfIndex::First) = Point2::new(i.uv0[0], i.uv0[1]);
    *cur.uv_pos_mut(SrfIndex::Second) = Point2::new(i.uv1[0], i.uv1[1]);
    *cur.uv_deriv_mut(SrfIndex::First) = Point2::new(i.duv0[0], i.duv0[1]);
    *cur.uv_deriv_mut(SrfIndex::Second) = Point2::new(i.duv1[0], i.duv1[1]);
    *cur.crv_pos_mut() = Vec3::from_array(i.crv_pos);
    *cur.crv_deriv_mut() = Vec3::from_array(i.crv_deriv);
    let mut next = TsectPnt::new();
    *next.uv_pos_mut(SrfIndex::First) = Point2::new(i.next_uv0[0], i.next_uv0[1]);
    *next.uv_pos_mut(SrfIndex::Second) = Point2::new(i.next_uv1[0], i.next_uv1[1]);
    next.user_doubles = i.next_ud;
    let mut step = i.step;
    let mut out = StepSolveOutcome::default();
    let r = FilletSolver::step_solve(
        &mut fs,
        &k,
        None,
        &cur,
        &mut step,
        &domain_of(i.dom1),
        &domain_of(i.dom2),
        &mut next,
        &mut out,
    );
    match r {
        Ok(()) => {
            let offset = |rail: usize| {
                fs.base
                    .surfaces
                    .get(rail)
                    .and_then(Option::as_ref)
                    .map_or(0.0, |s| s.surface.offset_distance())
            };
            let n0 = next.uv_pos(SrfIndex::First);
            let n1 = next.uv_pos(SrfIndex::Second);
            let mut o = vec![
                0.0,
                b(out.found_good_point),
                b(out.clipped),
                b(out.boundary_hit),
                step,
                n0.x,
                n0.y,
                n1.x,
                n1.y,
            ];
            o.extend(next.user_doubles);
            o.push(offset(0));
            o.push(offset(1));
            Ok(Output::new().values("flat", o))
        }
        // a failed solve keeps the flags it set (SmFilletIntersector reads them)
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| {
            Output::new().values(
                "flat",
                vec![
                    status(&e),
                    b(out.found_good_point),
                    b(out.clipped),
                    b(out.boundary_hit),
                ],
            )
        })),
    }
}

// ---- generator ------------------------------------------------------------------------

/// Clamped uniform knots for `n + 1` control points of degree `p` on `[0, 1]`.
fn knots(n: usize, p: usize) -> Vec<f64> {
    let spans = n + 1 - p;
    let mut u = vec![0.0; p + 1];
    for i in 1..spans {
        u.push(i as f64 / spans as f64);
    }
    u.extend(std::iter::repeat_n(1.0, p + 1));
    u
}

/// Greville abscissae of `knots` (degree `p`): control points there give `P(t) = t`.
fn greville(u: &[f64], n: usize, p: usize) -> Vec<f64> {
    (0..=n)
        .map(|i| u.iter().skip(i + 1).take(p).sum::<f64>() / p as f64)
        .collect()
}

/// `origin + s * du + t * dv` over `[0, 1]^2` as a B-spline surface of degrees `p`, `q`,
/// interior control points lifted by up to `bump` along `normal`.
#[allow(clippy::too_many_arguments)]
fn patch(
    rng: &mut Rng,
    origin: [f64; 3],
    du: [f64; 3],
    dv: [f64; 3],
    normal: [f64; 3],
    p: usize,
    q: usize,
    bump: f64,
) -> SrfIn {
    let n = p + rng.below(3) as usize;
    let m = q + rng.below(3) as usize;
    let u = knots(n, p);
    let v = knots(m, q);
    let gu = greville(&u, n, p);
    let gv = greville(&v, m, q);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let (s, t) = (
                        gu.get(i).copied().unwrap_or(0.0),
                        gv.get(j).copied().unwrap_or(0.0),
                    );
                    let lift = if i > 0 && i < n && j > 0 && j < m && bump != 0.0 {
                        rng.uniform(-bump, bump)
                    } else {
                        0.0
                    };
                    [
                        origin[0] + s * du[0] + t * dv[0] + lift * normal[0],
                        origin[1] + s * du[1] + t * dv[1] + lift * normal[1],
                        origin[2] + s * du[2] + t * dv[2] + lift * normal[2],
                        NOW,
                    ]
                })
                .collect()
        })
        .collect();
    SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net,
        p,
        q,
        u,
        v,
    })
}

/// A floor `z = 0` (`(u L, v L, 0)`) and a wall from the edge `x = z = 0` along
/// `w = (cos phi, 0, sin phi)` (`(0, u L, 0) + v L w`), offset towards the inside by
/// `r`: the centre line is `(r (1 + cos phi) / sin phi, y, r)`.
fn gen_step(rng: &mut Rng) -> StepIn {
    let scale = *rng.choose(&[1e-3, 1.0, 1.0, 1.0, 1.0, 1e3]).unwrap_or(&1.0);
    let l = 2.0 * scale;
    let phi = rng.uniform(20.0, 160.0).to_radians();
    let (sp, cp) = (phi.sin(), phi.cos());
    let r = l * rng.uniform(0.03, 0.15);
    let curved = rng.chance(0.4);
    let bump = if curved {
        l * rng.uniform(0.005, 0.03)
    } else {
        0.0
    };
    let deg = |rng: &mut Rng| 1 + rng.below(3) as usize;
    let (p0, q0, p1, q1) = (deg(rng), deg(rng), deg(rng), deg(rng));
    let s0 = patch(
        rng,
        [0.0; 3],
        [l, 0.0, 0.0],
        [0.0, l, 0.0],
        [0.0, 0.0, 1.0],
        p0,
        q0,
        bump,
    );
    let w = [cp, 0.0, sp];
    let n1 = [sp, 0.0, -cp];
    let s1 = patch(
        rng,
        [0.0; 3],
        [0.0, l, 0.0],
        [l * w[0], l * w[1], l * w[2]],
        n1,
        p1,
        q1,
        bump,
    );

    let xc = r * (1.0 + cp) / sp;
    let y0 = l * rng.uniform(0.1, 0.9);
    let rail1 = [xc - r * sp, y0, r + r * cp];
    let v1 = (rail1[0] * rail1[0] + rail1[2] * rail1[2]).sqrt() / l;
    let mut uv0 = [xc / l, y0 / l];
    let mut uv1 = [y0 / l, v1];
    let mut crv_pos = [xc, y0, r];
    if curved || rng.chance(0.3) {
        let e = 0.01 * rng.uniform(-1.0, 1.0);
        uv0[0] += e * uv0[0];
        uv1[1] -= e * uv1[1];
        crv_pos[0] += e * r;
    }
    let mut crv_deriv = [0.0, 1.0, 0.0];
    let mut duv0 = [0.0, 1.0 / l];
    let mut duv1 = [1.0 / l, 0.0];
    let mut step = l * rng.uniform(0.02, 0.2) * if rng.chance(0.3) { -1.0 } else { 1.0 };
    let mut dom1 = [0.0, 0.0, 1.0, 1.0];
    let mut dom2 = [0.0, 0.0, 1.0, 1.0];
    let mut allow_ext = false;
    match rng.below(10) {
        // across the far boundary of the floor (or of the wall, reversed directions)
        0 | 1 => step = l * (1.0 - y0 / l + rng.uniform(0.02, 0.3)),
        2 => step = -l * (y0 / l + rng.uniform(0.02, 0.3)),
        // a smaller domain clips the step
        3 => {
            let top = (y0 + 0.5 * step.abs()) / l;
            if step > 0.0 {
                dom1[3] = top.min(1.0);
            } else {
                dom2[0] = (y0 - 0.5 * step.abs()) / l;
            }
        }
        // starting on the boundary
        4 => {
            uv0[1] = 1.0;
            uv1[0] = 1.0;
            crv_pos[1] = l;
            step = step.abs() * if rng.chance(0.7) { 1.0 } else { -1.0 };
        }
        // zero, tiny and degenerate steps or tangents
        5 => match rng.below(4) {
            0 => step = 0.0,
            1 => step = 1e-14 * l,
            2 => duv0 = [0.0, 0.0],
            _ => crv_deriv = [0.0, 0.0, 0.0],
        },
        // extended domains with extendable offsets
        6 => {
            allow_ext = true;
            dom1 = [-0.5, -0.5, 1.5, 1.5];
            dom2 = [-0.5, -0.5, 1.5, 1.5];
            step = l * (1.0 - y0 / l + rng.uniform(0.05, 0.4));
        }
        // reversed curve direction
        7 => {
            crv_deriv = [0.0, -1.0, 0.0];
            duv0 = [0.0, -1.0 / l];
            duv1 = [-1.0 / l, 0.0];
        }
        _ => {}
    }
    let ty = *rng.choose(&[0u8, 2, 2, 4, 4, 6]).unwrap_or(&2);
    let orient = if rng.chance(0.1) {
        [-1.0, 1.0]
    } else {
        [1.0, 1.0]
    };
    // the rail chord of the exact fillet, perturbed for the clipped radius solve
    let chord = ((xc - rail1[0]).powi(2) + rail1[2].powi(2)).sqrt();
    let distance = chord * rng.uniform(0.9, 1.1);
    StepIn {
        ty,
        tol: 1e-6 * scale,
        radius: r,
        orient,
        distance,
        extra: rng.chance(0.2),
        init_fail: rng.chance(0.05),
        allow_ext,
        s0,
        s1,
        uv0,
        uv1,
        duv0,
        duv1,
        crv_pos,
        crv_deriv,
        step,
        dom1,
        dom2,
        next_uv0: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
        next_uv1: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
        next_ud: [
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
            rng.uniform(-1.0, 1.0),
        ],
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "blend.fillet_solver_p3"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(Inputs::Step(Box::new(gen_step(rng)))).ok()
    }

    fn cases(&self, ctx: &crate::case::CaseContext) -> Result<Vec<crate::Case>, HarnessError> {
        let ctx = crate::case::CaseContext {
            seeds: ctx.seeds.max(400),
            ..ctx.clone()
        };
        crate::runner::default_cases(self, &ctx)
    }

    fn run_cpp(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        match case.parse()? {
            Inputs::Step(i) => cpp_step(&i),
        }
    }

    fn run_rust(&self, case: &crate::Case) -> Result<Output, HarnessError> {
        match case.parse()? {
            Inputs::Step(i) => rust_step(&i),
        }
    }
}
