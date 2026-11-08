//! The rolling ball's local regularity along a blend (the blend design's §6): each face
//! encloses the ball, and every point of its section is progressive; where it is not,
//! the radius that works.
//!
//! Upstream: not in SMLib at the pinned commit. SolidCraft's fork (tangency-labs/smlib#79,
//! merged at 5e8c9c1) adds `SmBallStation` and `SmFilletSolver::JudgeBallAt` /
//! `BallIsRegular` (`SmFilletSolver.cpp`, `SmFilletSolver.h`) and the virtuals
//! `GetBallLaw`, `SetBallRadius` and `RollsABall` (`SmFilletStandardSolver.h/.cpp`) in
//! commits 410dfe1, 20ba5a9, 8c40818 and a34b5e8 (the last deletes the transversality check
//! 410dfe1 added, so it has no port). They run only with the opt-in
//! `Kernel::fixes.fillet_gn_section_generator`.
//!
//! | Rust                       | Upstream (fork)                                   |
//! |----------------------------|---------------------------------------------------|
//! | [`BallStation`]            | `SmBallStation`                                   |
//! | [`BallCheck`]              | `SmBallStation::SM_BS_*`                          |
//! | [`judge_ball_at`]          | `SmFilletSolver::JudgeBallAt`                     |
//! | [`ball_is_regular`]        | `SmFilletSolver::BallIsRegular`                   |
//! | [`ball_law`]               | `GetBallLaw` (base, `SmConstantRadiusFS`, `SmVariableRadiusFS`) |
//! | [`set_ball_radius`]        | `SetBallRadius` (base, `SmConstantRadiusFS`)      |
//! | [`rolls_a_ball`]           | `RollsABall` (base, `SmConstantRadiusFS`, `SmVariableRadiusFS`) |
//! | `face_jet_at` (private)    | `sm_FaceJetAt`                                    |
//! | `largest_curvature_toward_ball` (private) | `sm_LargestCurvatureTowardBall`    |
//! | `solve4` (private)         | `sm_Solve4`                                       |
//! | `ball_section_at` (private) | `sm_BallSectionAt`                               |
//! | `regular_at` (private)     | `sm_RegularAt`                                    |
//!
//! Deviations: the solver's virtuals are a match on the session's concrete solver
//! (`SmConstantRadiusAssistedFS` and `SmSurfaceSurfaceFS` inherit `SmConstantRadiusFS`'s;
//! `SmConstantDistanceFS` the base's). A variable-radius solver always holds its law in
//! the port, so it always rolls a ball (the fork's `m_rLaw != NULL`). The fork's
//! `EvaluatePoint` on the solver's offset keeps an extension it makes (a `const` cast);
//! the port's [`OffsetSurface::evaluate_point_extending`] keeps it the same way, so the
//! acting surface `GetExtendedBaseSurface` names is the fork's. Every loop is bounded by
//! fixed counts (32 steps, 40 bisections, 80 golden-section steps, 24 steps out, and
//! halvings down to a millionth of the radius); there is no recursion.

// the fork's NaN-safe tests (`!(a > b)`) are kept as written
#![allow(clippy::neg_cmp_op_on_partial_ord)]
use crate::blend::fillet_executive::session::SolverObject;
use crate::blend::fillet_solver::p1::{FilletSolver, FilletSolverOps, FilletSurfaceGeneratorType};
use crate::curve::curve::Curve;
use crate::error::{Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::geom::vector2d::Point2;
use crate::intersect::surface_intersector::p1::{SrfIndex, TsectPnt};
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use crate::nurbs::knots::Side;
use crate::surface::offset_surface::OffsetSurface;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::surface::surface::Surface;
use crate::topology::edgeuse::p1 as eu;
use crate::topology::{edge, Brep};

const OP: &str = "blend::fillet_executive::session::ball_regular";

/// `lSteps`: the sections along the edge.
const STEPS: usize = 32;

/// `lBisections`: the bisection (and half the golden-section) steps.
const BISECTIONS: usize = 40;

/// `smos_Max` of two doubles.
fn smos_max(a: f64, b: f64) -> f64 {
    if a > b {
        a
    } else {
        b
    }
}

/// `smos_Min` of two doubles.
fn smos_min(a: f64, b: f64) -> f64 {
    if a < b {
        a
    } else {
        b
    }
}

/// The first check of a station that fails (`SmBallStation::SM_BS_*`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BallCheck {
    /// `SM_BS_UNJUDGED`: not judged (a pole, a section not found, contacts that meet).
    #[default]
    Unjudged,
    /// `SM_BS_REGULAR`: every check passes.
    Regular,
    /// `SM_BS_NOT_ENCLOSED`: a face curves tighter than the ball (`1 - r k < 0`).
    NotEnclosed,
    /// `SM_BS_REGRESSIVE`: a point of the section is not progressive (`f_tt <= 0`).
    Regressive,
}

/// `SmBallStation`: the rolling ball's local regularity at a station of a blend.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BallStation {
    /// `m_lCheck`: the first check that fails, or [`BallCheck::Regular`].
    pub check: BallCheck,
    /// `m_dRadius`: the ball's radius there.
    pub radius: f64,
    /// `m_dEnclosed`: the least over the two faces of `1 - r k` (`BIG_DOUBLE`: not judged).
    pub enclosed: f64,
    /// `m_dProgress`: the least of `f_tt / |c'|^2` over the section's points.
    pub progress: f64,
    /// `m_dWorks`: the radius that works.
    pub works: f64,
    /// `m_dFits`: the least over the faces of `1 / k`.
    pub fits: f64,
    /// `m_bLaw`: the radius follows a law along the edge.
    pub law: bool,
    /// `m_bBall`: one ball rolls on both faces (`false`: unequal offsets, check 1 only).
    pub ball: bool,
}

impl Default for BallStation {
    fn default() -> Self {
        Self {
            check: BallCheck::Unjudged,
            radius: 0.0,
            enclosed: BIG_DOUBLE,
            progress: BIG_DOUBLE,
            works: 0.0,
            fits: BIG_DOUBLE,
            law: false,
            ball: true,
        }
    }
}

/// `GetBallLaw`'s outputs: whether one ball rolls on both faces, whether its radius
/// follows a law along the edge, `r, r', r''` and, on the edge, `C, C', C'', C'''`.
#[derive(Debug, Clone, Copy)]
pub struct BallLaw {
    /// `rbBall`.
    pub ball: bool,
    /// `rbOnEdge`.
    pub on_edge: bool,
    /// `adR`.
    pub r: [f64; 3],
    /// `asC`.
    pub c: [Vec3; 4],
}

/// The solver's base (`SmFilletSolver`) of a session solver.
pub(crate) fn ops_of(solver: &mut SolverObject) -> &mut dyn FilletSolverOps {
    match solver {
        SolverObject::ConstantRadius(s) => s.as_mut(),
        SolverObject::VariableRadius(s) => s.as_mut(),
        SolverObject::ConstantDistance(s) => s.as_mut(),
        SolverObject::SurfaceSurface(s) => s.as_mut(),
    }
}

/// `m_dFilletRadii` of a solver of `SmConstantRadiusFS`'s class.
fn constant_radii(solver: &SolverObject) -> Option<[f64; 2]> {
    match solver {
        SolverObject::ConstantRadius(s) => Some(s.fillet_radii),
        SolverObject::SurfaceSurface(s) => Some(s.base.fillet_radii),
        SolverObject::VariableRadius(_) | SolverObject::ConstantDistance(_) => None,
    }
}

/// `RollsABall(rbOnEdge)`: `(rolls, on_edge)`. `SmConstantRadiusFS`: one ball when both
/// radii are equal and positive; `SmVariableRadiusFS`: its law's ball, on the edge; the
/// base: none.
#[must_use]
pub fn rolls_a_ball(solver: &SolverObject) -> (bool, bool) {
    if let Some([r0, r1]) = constant_radii(solver) {
        return (r0 > 0.0 && (r1 - r0).abs() <= EFF_ZERO * (1.0 + r0), false);
    }
    match solver {
        SolverObject::VariableRadius(_) => (true, true),
        _ => (false, false),
    }
}

/// `GetBallLaw(crPnt, ...)`: the ball the solver rolls at `pnt`.
///
/// # Errors
///
/// Those of the law's and the base curve's evaluations.
pub fn ball_law(k: &Kernel, solver: &mut SolverObject, pnt: &TsectPnt) -> Result<BallLaw> {
    let mut law = BallLaw {
        ball: false,
        on_edge: false,
        r: [0.0; 3],
        c: [Vec3::UNDEF; 4],
    };
    if let Some([r0, r1]) = constant_radii(solver) {
        // SmConstantRadiusFS::GetBallLaw
        let d_r = r0;
        law.on_edge = false;
        law.ball = d_r > 0.0 && (r1 - d_r).abs() <= EFF_ZERO * (1.0 + d_r);
        law.r = [d_r, 0.0, 0.0];
        return Ok(law);
    }
    if let SolverObject::VariableRadius(vr) = solver {
        // SmVariableRadiusFS::GetBallLaw
        let t = pnt.user_doubles.first().copied().unwrap_or(0.0);
        law.r = vr.law.evaluate(t, &vr.law_interval, vr.law_orientation)?;
        let c = vr.curve_based.evaluate_base_curve(k, t, 3)?;
        for (dst, src) in law.c.iter_mut().zip(c) {
            *dst = src;
        }
        law.ball = true;
        law.on_edge = true;
    }
    Ok(law)
}

/// `SetBallRadius(dRadius)`: a constant ball's radius set to `radius` (both faces'
/// offsets); `false` where the solver has no constant ball.
///
/// # Errors
///
/// A missing offset surface ([`SolidError::null_pointer`]; the fork dereferences it).
pub fn set_ball_radius(solver: &mut SolverObject, radius: f64) -> Result<bool> {
    let (radii, base): (&mut [f64; 2], &mut FilletSolver) = match solver {
        SolverObject::ConstantRadius(s) => (&mut s.fillet_radii, &mut s.solver),
        SolverObject::SurfaceSurface(s) => (&mut s.base.fillet_radii, &mut s.base.solver),
        SolverObject::VariableRadius(_) | SolverObject::ConstantDistance(_) => return Ok(false),
    };
    *radii = [radius, radius];
    let orientations = base.orientations;
    for (rail, orientation) in orientations.into_iter().enumerate() {
        let slot = base
            .surface_mut(rail)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        slot.surface.set_offset_distance(radius * orientation);
    }
    Ok(true)
}

/// A face's surface, its normal's derivatives and its fundamental forms at a contact
/// (`sm_FaceJet`), the normal toward the ball.
#[derive(Debug, Clone, Copy)]
struct FaceJet {
    s: Vec3,
    su: Vec3,
    sv: Vec3,
    suu: Vec3,
    suv: Vec3,
    svv: Vec3,
    n: Vec3,
    nu: Vec3,
    nv: Vec3,
    nuu: Vec3,
    nuv: Vec3,
    nvv: Vec3,
    e: f64,
    f: f64,
    g: f64,
    l: f64,
    m: f64,
    nn: f64,
}

/// `D[i][j]` of an evaluation, `Vec3::UNDEF` where it is missing.
fn d_at(d: &[Vec<Vec3>], i: usize, j: usize) -> Vec3 {
    d.get(i)
        .and_then(|row| row.get(j))
        .copied()
        .unwrap_or(Vec3::UNDEF)
}

/// `sm_FaceJetAt`: the face's jet at `uv` with its normal toward `centre`; `None` at a
/// pole (`Su x Sv = 0`).
fn face_jet_at(k: &Kernel, surface: &Surface, uv: Point2, centre: Vec3) -> Result<Option<FaceJet>> {
    // Evaluate with three derivatives each way, upper half
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: false,
    };
    let d = surface.evaluate(k, uv, 3, 3, flags)?;
    let s = d_at(&d, 0, 0);
    let sv = d_at(&d, 0, 1);
    let svv = d_at(&d, 0, 2);
    let su = d_at(&d, 1, 0);
    let suv = d_at(&d, 1, 1);
    let suu = d_at(&d, 2, 0);
    let s_vvv = d_at(&d, 0, 3);
    let s_uvv = d_at(&d, 1, 2);
    let s_uuv = d_at(&d, 2, 1);
    let s_uuu = d_at(&d, 3, 0);

    let m = su.cross(sv);
    let dl = m.length();
    if !(dl > EFF_ZERO * su.length() * sv.length()) || !(dl > 0.0) {
        return Ok(None);
    }
    let mu = suu.cross(sv) + su.cross(suv);
    let mv = suv.cross(sv) + su.cross(svv);
    let muu = s_uuu.cross(sv) + 2.0 * suu.cross(suv) + su.cross(s_uuv);
    let muv = s_uuv.cross(sv) + suu.cross(svv) + su.cross(s_uvv);
    let mvv = s_uvv.cross(sv) + 2.0 * suv.cross(svv) + su.cross(s_vvv);

    let n = m / dl;
    let dlu = n.dot(mu);
    let dlv = n.dot(mv);
    let nu = (mu - n * dlu) / dl;
    let nv = (mv - n * dlv) / dl;
    let dluu = nu.dot(mu) + n.dot(muu);
    let dluv = nv.dot(mu) + n.dot(muv);
    let dlvv = nv.dot(mv) + n.dot(mvv);
    let nuu = (muu - nu * (2.0 * dlu) - n * dluu) / dl;
    let nuv = (muv - nu * dlv - nv * dlu - n * dluv) / dl;
    let nvv = (mvv - nv * (2.0 * dlv) - n * dlvv) / dl;

    let side = if n.dot(centre - s) < 0.0 { -1.0 } else { 1.0 };
    let n = n * side;
    Ok(Some(FaceJet {
        s,
        su,
        sv,
        suu,
        suv,
        svv,
        n,
        nu: nu * side,
        nv: nv * side,
        nuu: nuu * side,
        nuv: nuv * side,
        nvv: nvv * side,
        e: su.dot(su),
        f: su.dot(sv),
        g: sv.dot(sv),
        l: suu.dot(n),
        m: suv.dot(n),
        nn: svv.dot(n),
    }))
}

/// `sm_LargestCurvatureTowardBall`: the largest root `k` of
/// `(L - k E)(N - k G) - (M - k F)^2 = 0`.
fn largest_curvature_toward_ball(j: &FaceJet) -> f64 {
    let a = j.l * j.nn - j.m * j.m;
    let b = j.e * j.nn + j.g * j.l - 2.0 * j.f * j.m;
    let c = j.e * j.g - j.f * j.f;
    let disc = b * b - 4.0 * a * c;
    (b + (if disc > 0.0 { disc } else { 0.0 }).sqrt()) / (2.0 * c)
}

/// `sm_Solve4`: `A x = b` by Gaussian elimination with partial pivoting; `None` where `A`
/// is singular to working precision (its determinant over Hadamard's bound at most
/// `SM_EFF_ZERO`).
fn solve4(a: &[[f64; 4]; 4], b: &[f64; 4]) -> Option<[f64; 4]> {
    let mut hadamard = 1.0;
    for j in 0..4 {
        let mut col = 0.0;
        for row in a {
            let v = row.get(j).copied().unwrap_or(0.0);
            col += v * v;
        }
        hadamard *= f64::sqrt(col);
    }
    let mut mm = [[0.0_f64; 5]; 4];
    for ((dst, src), &bi) in mm.iter_mut().zip(a).zip(b) {
        for (d, s) in dst.iter_mut().zip(src) {
            *d = *s;
        }
        if let Some(last) = dst.get_mut(4) {
            *last = bi;
        }
    }
    let at = |mm: &[[f64; 5]; 4], i: usize, j: usize| -> f64 {
        mm.get(i).and_then(|r| r.get(j)).copied().unwrap_or(0.0)
    };
    let mut det = 1.0;
    for kk in 0..4 {
        let mut pivot = kk;
        for i in kk + 1..4 {
            if at(&mm, i, kk).abs() > at(&mm, pivot, kk).abs() {
                pivot = i;
            }
        }
        if pivot != kk {
            mm.swap(kk, pivot);
            det = -det;
        }
        let diag = at(&mm, kk, kk);
        det *= diag;
        if diag == 0.0 {
            break;
        }
        let pivot_row = mm.get(kk).copied().unwrap_or([0.0; 5]);
        for row in mm.iter_mut().skip(kk + 1) {
            let f = row.get(kk).copied().unwrap_or(0.0) / diag;
            for (j, v) in row.iter_mut().enumerate().skip(kk) {
                *v -= f * pivot_row.get(j).copied().unwrap_or(0.0);
            }
        }
    }
    let singular = !(hadamard > 0.0) || !(det.abs() > EFF_ZERO * hadamard);
    if singular {
        return None;
    }
    let mut x = [0.0_f64; 4];
    for i in (0..4).rev() {
        let mut d = at(&mm, i, 4);
        for j in i + 1..4 {
            d -= at(&mm, i, j) * x.get(j).copied().unwrap_or(0.0);
        }
        if let Some(xi) = x.get_mut(i) {
            *xi = d / at(&mm, i, i);
        }
    }
    Some(x)
}

/// `SmFilletSolver::JudgeBallAt`: the rolling ball's local regularity at the station
/// `pnt` (its contacts' uv on the solver's two faces and, for a law, its `t`).
///
/// # Errors
///
/// Those of the law's, the offsets' and the surfaces' evaluations; a missing offset
/// surface ([`SolidError::null_pointer`]).
pub fn judge_ball_at(k: &Kernel, solver: &mut SolverObject, pnt: &TsectPnt) -> Result<BallStation> {
    let mut station = BallStation::default();
    let law = ball_law(k, solver, pnt)?;
    let (ball, on_edge, ar, ac) = (law.ball, law.on_edge, law.r, law.c);
    let fsg_circular = solver
        .base()
        .fillet_surface_generator()
        .is_some_and(|g| g.fillet_surface_generator_type() == FilletSurfaceGeneratorType::Circular);
    let approx_tol = solver.base().this_approx_tol_3d();
    let ops = ops_of(solver);
    if ops.solver().surface(0)?.is_none() || ops.solver().surface(1)?.is_none() {
        return Err(SolidError::null_pointer(OP));
    }
    station.law = on_edge;
    station.ball = ball;
    let uvs = [pnt.uv_pos(SrfIndex::First), pnt.uv_pos(SrfIndex::Second)];

    // each face's offset point at the station's offsets: the ball's centre (both the same), or
    // where no ball rolls (unequal offsets) each face's own
    let radius_changes = ops.offset_radii_can_change()?;
    if radius_changes {
        ops.setup_offset_values(pnt, None)?;
    }
    let on = (|| -> Result<[Point3; 2]> {
        let mut on = [Point3::UNDEF; 2];
        for (rail, (dst, uv)) in on.iter_mut().zip(uvs).enumerate() {
            let slot = ops
                .solver_mut()
                .surface_mut(rail)?
                .ok_or_else(|| SolidError::null_pointer(OP))?;
            *dst = slot.surface.surface.evaluate_point_extending(k, uv)?;
        }
        Ok(on)
    })();
    if radius_changes {
        ops.solver_mut().reload_surface_offsets()?;
    }
    let on = on?;
    let c = on[0];

    // each face's surface as its offset evaluates it there: the base, or past the base's
    // domain its extension
    let mut jets: [Option<FaceJet>; 2] = [None, None];
    let mut offsets = [0.0_f64; 2];
    for rail in 0..2 {
        let slot = ops
            .solver()
            .surface(rail)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let offset: &OffsetSurface = &slot.surface.surface;
        let uv = uvs.get(rail).copied().unwrap_or(Point2::UNDEF);
        let base = offset.base_surface();
        let mut acting = base;
        if let Some(extended) = offset.extended_base_surface() {
            if !base.natural_uv_domain_with_nurb(k)?.contains_point(uv, 0.0)
                && extended
                    .natural_uv_domain_with_nurb(k)?
                    .contains_point(uv, 0.0)
            {
                acting = extended;
            }
        }
        // a ball's centre is the first face's offset point, for both faces
        let own = if ball {
            c
        } else {
            on.get(rail).copied().unwrap_or(Point3::UNDEF)
        };
        let Some(jet) = face_jet_at(k, acting, uv, own)? else {
            return Ok(station);
        };
        // a ball's radius is the solver's there (the law's, or its constant), for both faces
        let r0 = ar.first().copied().unwrap_or(0.0);
        if let (Some(dst), Some(o)) = (jets.get_mut(rail), offsets.get_mut(rail)) {
            *o = if ball && r0 > 0.0 {
                r0
            } else {
                (own - jet.s).length()
            };
            *dst = Some(jet);
        }
    }
    let [Some(j0), Some(j1)] = jets else {
        return Ok(station);
    };
    let jets = [j0, j1];
    let d_r = offsets[0];
    let chord = j1.s - j0.s;
    station.radius = if ball {
        d_r
    } else {
        smos_max(offsets[0], offsets[1])
    };
    if !(station.radius > EFF_ZERO) {
        return Ok(station);
    }

    // 1. each face encloses the ball: 1 - d k > 0 for its offset d
    let focal = 1.0e-6;
    station.enclosed = BIG_DOUBLE;
    for (jet, &offset) in jets.iter().zip(&offsets) {
        let kk = largest_curvature_toward_ball(jet);
        let margin = 1.0 - offset * kk;
        if margin < station.enclosed {
            station.enclosed = margin;
            if !ball {
                station.radius = offset;
            }
        }
        if kk > 0.0 && 1.0 / kk < station.fits {
            station.fits = 1.0 / kk;
        }
    }
    if station.enclosed < -focal {
        station.check = BallCheck::NotEnclosed;
        return Ok(station);
    }
    station.check = BallCheck::Regular;
    if !ball {
        return Ok(station); // no ball: its spine and section are not a rolling ball's
    }
    // a focal station, or contacts that meet, judge nothing further
    if station.enclosed <= focal || !(chord.length() > approx_tol) {
        return Ok(station);
    }

    // 2. the spine's tangent: O_i = s_i + r n_i
    let (rp, rpp) = (ar[1], ar[2]);
    let ou = [j0.su + j0.nu * d_r, j1.su + j1.nu * d_r];
    let ov = [j0.sv + j0.nv * d_r, j1.sv + j1.nv * d_r];
    let (w, a1) = if on_edge {
        (ac[1], ac[1].dot(ac[1]) + ac[2].dot(ac[0] - c))
    } else {
        let w0 = j0.n.cross(j1.n);
        let dw = w0.length();
        if !(dw > EFF_ZERO) {
            return Ok(station); // antipodal contacts: the section solve is singular here too
        }
        (w0 / dw, 1.0)
    };
    let mut a = [[0.0_f64; 4]; 4];
    let mut b = [0.0_f64; 4];
    for (i, (row, bi)) in a.iter_mut().zip(b.iter_mut()).take(3).enumerate() {
        *row = [
            ou[0].component(i),
            ov[0].component(i),
            -ou[1].component(i),
            -ov[1].component(i),
        ];
        *bi = -rp * (j0.n.component(i) - j1.n.component(i));
    }
    a[3] = [w.dot(ou[0]), w.dot(ov[0]), 0.0, 0.0];
    b[3] = a1 - rp * w.dot(j0.n);
    let x = solve4(&a, &b);
    let xs = x.unwrap_or([0.0; 4]);
    let cp = ou[0] * xs[0] + ov[0] * xs[1] + j0.n * rp;
    let cp2 = cp.length_squared();
    if x.is_none() || !(cp2 > 0.0) {
        return Ok(station); // the offsets tangent: no c'
    }

    // 3. c'': the equations differentiated twice, K_i the terms without u_i'', v_i''
    let uvp = [[xs[0], xs[1]], [xs[2], xs[3]]];
    let mut kv = [Vec3::UNDEF; 2];
    for ((dst, jet), [du, dv]) in kv.iter_mut().zip(&jets).zip(uvp) {
        let ouu = jet.suu + jet.nuu * d_r;
        let ouv = jet.suv + jet.nuv * d_r;
        let ovv = jet.svv + jet.nvv * d_r;
        *dst = ouu * (du * du)
            + ouv * (2.0 * du * dv)
            + ovv * (dv * dv)
            + (jet.nu * du + jet.nv * dv) * (2.0 * rp)
            + jet.n * rpp;
    }
    let mut a2 = 0.0;
    if on_edge {
        a2 = ac[3].dot(ac[0] - c) + 2.0 * ac[2].dot(ac[1] - cp) + ac[1].dot(ac[2]);
    }
    for (i, bi) in b.iter_mut().take(3).enumerate() {
        *bi = -(kv[0].component(i) - kv[1].component(i));
    }
    b[3] = a2 - w.dot(kv[0]);
    let Some(x) = solve4(&a, &b) else {
        return Ok(station);
    };
    let cpp = kv[0] + ou[0] * x[0] + ov[0] * x[1];

    // f_tt on the section: A0 - <p - c, c''>
    let a0 = cp2 - rp * rp - d_r * rpp;
    let mut least = BIG_DOUBLE;
    for jet in &jets {
        let end = a0 - (jet.s - c).dot(cpp);
        if end < least {
            least = end;
        }
    }
    if fsg_circular {
        // the touch circle: centre m on the centre line, through s1 and s2
        let mm = c + cp * ((j0.s - c).dot(cp) / cp2);
        let e1_raw = j0.s - mm;
        let rho = e1_raw.length();
        if rho > EFF_ZERO * d_r {
            let e1 = e1_raw / rho;
            let q = j1.s - mm;
            let e2_raw = q - e1 * q.dot(e1);
            let de2 = e2_raw.length();
            let e2 = if de2 > EFF_ZERO * d_r {
                e2_raw / de2
            } else {
                cp.cross(e1).unitize()?
            };
            let phi = f64::atan2(q.dot(e2), q.dot(e1));
            // the most of a cos(phi) + b sin(phi) over [0, phi]
            let da = e1.dot(cpp);
            let db = e2.dot(cpp);
            let mut most = da;
            let at_end = da * phi.cos() + db * phi.sin();
            if at_end > most {
                most = at_end;
            }
            let star = f64::atan2(db, da);
            if star > 0.0 && star < phi {
                most = f64::sqrt(da * da + db * db);
            }
            let arc = a0 - (mm - c).dot(cpp) - rho * most;
            if arc < least {
                least = arc;
            }
        }
    }
    station.progress = least / cp2;
    if !(station.progress > 0.0) {
        station.check = BallCheck::Regressive;
    }
    Ok(station)
}

/// `sm_BallSectionAt`: the solver's section in the plane square to its filleted edge at
/// the edge's `t`, on the faces' surfaces extended as the trace extends them; its guess
/// is the edge point dropped on each face, or where that finds none, `guess`'s contacts.
pub(crate) fn ball_section_at(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    edge_curve: &Curve,
    t: f64,
    guess: Option<&TsectPnt>,
) -> Result<Option<TsectPnt>> {
    let ops = ops_of(solver);
    let bases: [Surface; 2] = {
        let s0 = ops
            .solver()
            .surface(0)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        let s1 = ops
            .solver()
            .surface(1)?
            .ok_or_else(|| SolidError::null_pointer(OP))?;
        [
            s0.surface.base_surface().clone(),
            s1.surface.base_surface().clone(),
        ]
    };
    // the domains the trace solves on: the natural ones, extended as for the trace
    let mut domains: [Extent2d; 2] = [
        bases[0].natural_uv_domain_with_nurb(k)?,
        bases[1].natural_uv_domain_with_nurb(k)?,
    ];
    for (base, domain) in bases.iter().zip(domains.iter_mut()) {
        ops.setup_offset_extension(k, base, domain)?;
    }
    let pv = edge_curve.evaluate(k, t, 1, true, false)?;
    let (Some(&point), Some(&tangent)) = (pv.first(), pv.get(1)) else {
        return Ok(None);
    };
    if !(tangent.length() > EFF_ZERO) {
        return Ok(None);
    }
    let normal = tangent.unitize()?;
    let mut guesses = [Point2::UNDEF; 2];
    let mut dropped = true;
    for ((base, domain), g) in bases.iter().zip(&domains).zip(guesses.iter_mut()) {
        if !dropped {
            break;
        }
        let drop = base.drop_point::<NoObject>(k, point, domain, &DropPointOptions::default())?;
        dropped = drop.success;
        *g = drop.result_uv;
    }
    let mut found = false;
    let mut section = TsectPnt::new();
    if dropped {
        found = ops.point_on_plane_solve(
            k,
            Some(&mut *target),
            point,
            normal,
            &domains[0],
            &domains[1],
            guesses[0],
            guesses[1],
            &mut section,
        )?;
    }
    if !found {
        if let Some(g) = guess {
            section = TsectPnt::new();
            found = ops.point_on_plane_solve(
                k,
                Some(&mut *target),
                point,
                normal,
                &domains[0],
                &domains[1],
                g.uv_pos(SrfIndex::First),
                g.uv_pos(SrfIndex::Second),
                &mut section,
            )?;
        }
    }
    Ok(found.then_some(section))
}

/// The section near the next one (`rNear`, `rbNear` of the fork).
#[derive(Debug, Clone, Default)]
struct Near(Option<TsectPnt>);

/// `sm_RegularAt`: whether the solver's ball is regular at the section at the edge's `t`,
/// with the ball's radius `radius` for a constant ball (0: the solver's own), its
/// surfaces' offsets back at the solver's own after. `near` is the guess, then this
/// section if found.
fn regular_at(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
    edge_curve: &Curve,
    t: f64,
    radius: f64,
    near: &mut Near,
) -> Result<(bool, BallStation)> {
    let mut station = BallStation::default();
    let mut own = 0.0;
    if radius > 0.0 {
        let (rolls, on_edge) = rolls_a_ball(solver);
        if !rolls || on_edge {
            return Ok((true, station));
        }
        let any = TsectPnt::new();
        own = ball_law(k, solver, &any)?.r[0];
        if !set_ball_radius(solver, radius)? {
            return Ok((true, station));
        }
    }
    let mut found = false;
    let result = (|| -> Result<()> {
        if let Some(pnt) = ball_section_at(k, target, solver, edge_curve, t, near.0.as_ref())? {
            found = true;
            let judged = judge_ball_at(k, solver, &pnt);
            near.0 = Some(pnt);
            station = judged?;
        }
        Ok(())
    })();
    if radius > 0.0 {
        // the solver's own radius back, whatever the solve did
        let _ = set_ball_radius(solver, own);
    }
    result?;
    let regular = if radius > 0.0 && !found {
        false // no section at the trial radius: not one that works
    } else {
        !(station.check != BallCheck::Unjudged && station.check != BallCheck::Regular)
    };
    Ok((regular, station))
}

/// `SmFilletSolver::BallIsRegular`: whether this solver's rolling ball is regular along
/// its edge (judged at the middles of 32 equal steps of the edge's parameter, each
/// margin then minimized about its least section by golden section), and where it is
/// not, the station that fails with the radius that works (`works`): for a constant
/// ball its largest regular radius by bisection, least along the edge; for a law the
/// radius where it first stops being regular. A [`BallCheck::Regular`] station where
/// none fails.
///
/// `target` holds the solver's edge use.
///
/// # Errors
///
/// Those of the section solves and the evaluations; `SM_ERR_NULL_POINTER` for an edge
/// without a curve.
pub fn ball_is_regular(
    k: &Kernel,
    target: &mut Brep,
    solver: &mut SolverObject,
) -> Result<BallStation> {
    let mut first = BallStation {
        check: BallCheck::Regular,
        ..BallStation::default()
    };
    let base = solver.base();
    if base.surface(0)?.is_none() || base.surface(1)?.is_none() {
        return Ok(first);
    }
    let Some(edgeuse) = base.edgeuse(0)? else {
        return Ok(first);
    };
    let edge_id = eu::edge(target, edgeuse)?.ok_or_else(|| SolidError::null_pointer(OP))?;
    let curve = edge::evaluation_curve(target, edge_id)?
        .ok_or_else(|| SolidError::null_pointer(OP))?
        .clone();
    let ivl = edge::interval(target, edge_id)?;
    let closed = edge::is_closed(target, edge_id)?;
    // a solver that rolls no ball (unequal offsets) has only its offsets judged (check 1)
    let (rolls, _) = rolls_a_ball(solver);
    let mut sections: Vec<Near> = vec![Near::default(); STEPS];

    // a constant ball's largest regular radius at the section t (judged `at` at the ball's
    // own radius)
    let limit_at = |solver: &mut SolverObject,
                    target: &mut Brep,
                    t: f64,
                    at: &BallStation,
                    near: &mut Near|
     -> Result<f64> {
        let radius = at.radius;
        if at.check == BallCheck::Regular || at.check == BallCheck::Unjudged {
            return Ok(radius);
        }
        let (mut lo, mut hi) = (0.0, radius);
        let mut s_lo = BallStation::default();
        let mut tr = 0.5 * radius;
        while tr > 1.0e-6 * radius {
            let (ok, st) = regular_at(k, target, solver, &curve, t, tr, near)?;
            if ok {
                lo = tr;
                s_lo = st;
                break;
            }
            hi = tr;
            tr *= 0.5;
        }
        let mut i = 0;
        while i < BISECTIONS && lo > 0.0 {
            let mid = 0.5 * (lo + hi);
            let (ok, st) = regular_at(k, target, solver, &curve, t, mid, near)?;
            if ok {
                lo = mid;
                s_lo = st;
            } else {
                hi = mid;
            }
            i += 1;
        }
        if lo > 0.0 && at.check == BallCheck::NotEnclosed && s_lo.fits < BIG_DOUBLE {
            lo = s_lo.fits;
        }
        Ok(lo)
    };
    let mut worst_t = 0.0;
    let mut worst_near = Near::default();

    // the radius that works at a failing section t (judged `fail`), from a regular one at
    // `pass_t` below it (`pass`) for a law, the solves continuing from `near`
    let fail_at = |solver: &mut SolverObject,
                   target: &mut Brep,
                   first: &mut BallStation,
                   worst_t: &mut f64,
                   worst_near: &mut Near,
                   t: f64,
                   fail: &BallStation,
                   pass: bool,
                   pass_t: f64,
                   near: &Near|
     -> Result<()> {
        let mut near = near.clone();
        if fail.law {
            let mut onset = *fail;
            if pass {
                let (mut lo, mut hi) = (pass_t, t);
                for _ in 0..BISECTIONS {
                    let mid = 0.5 * (lo + hi);
                    let (ok, st) = regular_at(k, target, solver, &curve, mid, 0.0, &mut near)?;
                    if ok {
                        lo = mid;
                    } else {
                        hi = mid;
                        onset = st;
                    }
                }
            }
            if first.check == BallCheck::Regular {
                *first = onset;
                first.works = onset.radius;
            }
            return Ok(());
        }
        if !rolls {
            // unequal offsets: the offset that folds must stay below the face's radius of
            // curvature
            if fail.check == BallCheck::NotEnclosed
                && (first.check == BallCheck::Regular || fail.fits < first.works)
            {
                *first = *fail;
                first.works = fail.fits;
            }
            return Ok(());
        }
        let lo = limit_at(solver, target, t, fail, &mut near)?;
        if first.check == BallCheck::Regular || lo < first.works {
            *first = *fail;
            first.works = lo;
            *worst_t = t;
            *worst_near = near;
        }
        Ok(())
    };

    let refine = |solver: &mut SolverObject,
                  target: &mut Brep,
                  first: &mut BallStation,
                  worst_t: f64,
                  worst_near: &Near|
     -> Result<()> {
        // a constant ball: its limit's least along the edge, about the failing section
        if !rolls || first.law || !(first.works > 0.0) {
            return Ok(());
        }
        let length = ivl.length();
        let wrap = |t: f64| -> f64 {
            if !closed {
                return smos_max(ivl.min(), smos_min(ivl.max(), t));
            }
            let mut d = (t - ivl.min()) % length;
            if d < 0.0 {
                d += length;
            }
            ivl.min() + d
        };
        let limit = |solver: &mut SolverObject, target: &mut Brep, t: f64| -> Result<f64> {
            let mut at = worst_near.clone();
            let (_, station) = regular_at(k, target, solver, &curve, wrap(t), 0.0, &mut at)?;
            limit_at(solver, target, wrap(t), &station, &mut at)
        };
        let own = first.radius;
        let (mut da, mut db) = (worst_t, worst_t);
        for side in 0..2 {
            let mut step = length / (64.0 * STEPS as f64);
            for _ in 0..24 {
                let next = if side == 0 {
                    worst_t - step
                } else {
                    worst_t + step
                };
                let end = if side == 0 { &mut da } else { &mut db };
                if !closed && (next < ivl.min() || next > ivl.max()) {
                    *end = if side == 0 { ivl.min() } else { ivl.max() };
                    break;
                }
                *end = next;
                let lim = limit(solver, target, next)?;
                if !(lim < own) {
                    break;
                }
                step *= 2.0;
            }
        }
        let golden = 0.5 * (f64::sqrt(5.0) - 1.0);
        let mut x1 = db - golden * (db - da);
        let mut x2 = da + golden * (db - da);
        let mut f1 = limit(solver, target, x1)?;
        let mut f2 = limit(solver, target, x2)?;
        for _ in 0..2 * BISECTIONS {
            if f1 < f2 {
                db = x2;
                x2 = x1;
                f2 = f1;
                x1 = db - golden * (db - da);
                f1 = limit(solver, target, x1)?;
            } else {
                da = x1;
                x1 = x2;
                f1 = f2;
                x2 = da + golden * (db - da);
                f2 = limit(solver, target, x2)?;
            }
        }
        let least = smos_min(f1, f2);
        if least > 0.0 && least < first.works {
            first.works = least;
        }
        Ok(())
    };

    let mut ts: Vec<f64> = Vec::with_capacity(STEPS);
    let mut margins: [Vec<f64>; 2] = [Vec::with_capacity(STEPS), Vec::with_capacity(STEPS)];
    let mut passed = false;
    let mut passed_t = 0.0;
    let mut near = Near::default();
    for (j, section) in sections.iter_mut().enumerate() {
        let t = ivl.evaluate((j as f64 + 0.5) / STEPS as f64);
        let (regular, station) = regular_at(k, target, solver, &curve, t, 0.0, &mut near)?;
        *section = near.clone(); // this step's section, or the last one found before it
        ts.push(t);
        margins[0].push(station.enclosed);
        margins[1].push(station.progress);
        if regular {
            passed = true;
            passed_t = t;
            continue;
        }
        fail_at(
            solver,
            target,
            &mut first,
            &mut worst_t,
            &mut worst_near,
            t,
            &station,
            passed,
            passed_t,
            &near,
        )?;
        if station.law {
            return Ok(first); // a law: the first failing section along the edge
        }
    }
    if first.check != BallCheck::Regular {
        refine(solver, target, &mut first, worst_t, &worst_near)?;
        return Ok(first);
    }

    // every section regular: each margin's least between the sections about its least one
    for (m, marg) in margins.iter().enumerate() {
        let mut least_idx = 0;
        for j in 1..ts.len() {
            if marg.get(j) < marg.get(least_idx) {
                least_idx = j;
            }
        }
        let least_val = marg.get(least_idx).copied().unwrap_or(BIG_DOUBLE);
        if ts.is_empty() || !(least_val < BIG_DOUBLE) {
            continue;
        }
        let t_at = |j: usize| ts.get(j).copied().unwrap_or(0.0);
        let mut da = if least_idx > 0 {
            t_at(least_idx - 1)
        } else {
            ivl.min()
        };
        let mut db = if least_idx + 1 < ts.len() {
            t_at(least_idx + 1)
        } else {
            ivl.max()
        };
        let golden = 0.5 * (f64::sqrt(5.0) - 1.0);
        let mut near = sections.get(least_idx).cloned().unwrap_or_default();
        let mut margin =
            |solver: &mut SolverObject, target: &mut Brep, t: f64| -> Result<(f64, BallStation)> {
                let (regular, station) = regular_at(k, target, solver, &curve, t, 0.0, &mut near)?;
                let mut v = if m == 0 {
                    station.enclosed
                } else {
                    station.progress
                };
                if !regular {
                    v = -BIG_DOUBLE; // failing: as low as it gets
                }
                Ok((v, station))
            };
        let mut x1 = db - golden * (db - da);
        let mut x2 = da + golden * (db - da);
        let (mut f1, mut s1) = margin(solver, target, x1)?;
        let (mut f2, mut s2) = margin(solver, target, x2)?;
        let mut i = 0;
        while i < BISECTIONS && f1 > -BIG_DOUBLE && f2 > -BIG_DOUBLE {
            if f1 < f2 {
                db = x2;
                x2 = x1;
                f2 = f1;
                s2 = s1;
                x1 = db - golden * (db - da);
                (f1, s1) = margin(solver, target, x1)?;
            } else {
                da = x1;
                x1 = x2;
                f1 = f2;
                s1 = s2;
                x2 = da + golden * (db - da);
                (f2, s2) = margin(solver, target, x2)?;
            }
            i += 1;
        }
        let first_fails = f1 <= f2;
        let fail_t = if first_fails { x1 } else { x2 };
        let fail = if first_fails { s1 } else { s2 };
        if fail.check == BallCheck::Unjudged || fail.check == BallCheck::Regular {
            continue;
        }
        // a regular section below the failing one: the least section's, or the one before it
        let t_least = t_at(least_idx);
        let pass_t = if t_least < fail_t {
            t_least
        } else if least_idx > 0 {
            t_at(least_idx - 1)
        } else {
            ivl.min()
        };
        let pass = t_least < fail_t || least_idx > 0;
        let near_least = sections.get(least_idx).cloned().unwrap_or_default();
        fail_at(
            solver,
            target,
            &mut first,
            &mut worst_t,
            &mut worst_near,
            fail_t,
            &fail,
            pass,
            pass_t,
            &near_least,
        )?;
    }
    if first.check != BallCheck::Regular {
        refine(solver, target, &mut first, worst_t, &worst_near)?;
    }
    Ok(first)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn solve4_solves_and_flags_singular() {
        let a = [
            [2.0, 0.0, 0.0, 1.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 4.0, 0.0],
            [1.0, 0.0, 0.0, 5.0],
        ];
        let x = solve4(&a, &[3.0, 3.0, 4.0, 6.0]).unwrap();
        for v in x {
            assert!((v - 1.0).abs() < 1e-14);
        }
        let s = [
            [1.0, 2.0, 0.0, 0.0],
            [2.0, 4.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        assert!(solve4(&s, &[1.0; 4]).is_none());
    }
}
