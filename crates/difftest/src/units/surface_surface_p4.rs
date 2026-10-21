//! Runner for unit `surface.surface_p4`: `SmSurface` part 4 -- `SmSurfaceEval::Normal` and
//! `EvalNormalSection`, `sm_PassSPConvergenceTest`, `CreateSeamCurve`, `IsOnSeam`,
//! `GlobalPointSolve`, `sm_ClassifyCurveSurfaceXSectPoint` and the Newton equation sets
//! `SmCSIntersectENFO`, `SmCSNormalizeENFO`, `SmSSPIntersectENFO`, `SmSSINormalizeENFO`
//! and `SmSSNormalizeENFO` -- with the co-ported `smsurf_EvaluateNormalSection`,
//! `smsurf_IsPointOnNormal` and `SmSurfaceEval::EvalNormal`.
//!
//! C++: `solidcore_ffi::units::surface_surface_p4` (upstream `source/SMLib/src/SmSurface.cpp`;
//! the file-static functions and file-local classes are verbatim copies). Rust:
//! `solidcore::surface::surface::p4`.
//!
//! A case is one `op` on a `surface` (the input forms of unit `surface.surface_p7`).
//! Both sides produce the flat record documented in the ffi module (status first); the
//! runner reports it as the `out` values. A Rust error maps to its `SmStatus` (`1007` for
//! `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the case.
//!
//! `StepInv` runs only on planes (`SmBSplineSurface::STEPInversion` dispatching to the
//! plane's virtual `IsSingularity` / `IsClosed`); a guess defers it (`LocalPointSolve`).
//!
//! Deferred cases: the global point solve of face-owned surfaces (`SmFace::PointClassify`)
//! and what needs the unported `surface.surface_p5` (`LocalPointSolve`, the base
//! `IsSingularity`), e.g. a drop point or a guessed inversion.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::nurbs::math::NOW;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::p2::SurfaceEval;
use solidcore::surface::surface::p1::{
    evaluate_normal_section, CsIntersectEnfo, CsNormalizeEnfo, SspIntersectEnfo, SsiNormalizeEnfo,
    SsNormalizeEnfo,
};
use solidcore::surface::surface::p4::{
    classify_curve_surface_xsect_point, is_point_on_normal, pass_sp_convergence_test, NoObject,
    NormalTestReason,
};
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::surface_surface_p4::{self as ffi, Op};

use super::surface_surface_p7::{
    b, curve_of, dom_of, gen_boundary_uv, gen_curve, gen_dom, gen_param, gen_plane, gen_point_near,
    gen_surface, gen_uv, harness, p2, param_of, push_crv, push_srf, push_vecs, pv,
    req_of, sop_of, st, surface_of, unit_scale, v3, CrvIn, Fail, SrfIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `surface.surface_p4` runner.
pub struct Runner;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    NormalEval { uv: [f64; 2] },
    SectionFn { uv: [f64; 2], dir: [f64; 3] },
    SectionEval { uv: [f64; 2], dir: [f64; 3] },
    OnNormal {
        dom: Option<[f64; 4]>,
        uv: [f64; 2],
        p: [f64; 3],
        deriv: Option<[f64; 3]>,
        tol: f64,
    },
    PassSp { uv: [f64; 2], p: [f64; 3], tol: f64 },
    Seam { param: u8, known: bool },
    IsOnSeam { uv: [f64; 2], tol: Option<f64> },
    Gps {
        dom: [f64; 4],
        sop: u8,
        p: [f64; 3],
        tol: f64,
        target: Option<f64>,
        req: u8,
    },
    CsIntersect { curve: CrvIn, pts: Vec<Vec<f64>> },
    CsNormalize {
        curve: CrvIn,
        sop: u8,
        vecs: Vec<[f64; 3]>,
        pts: Vec<Vec<f64>>,
    },
    SspIntersect {
        other: SrfIn,
        steepest: bool,
        plane: Option<([f64; 3], [f64; 3])>,
        pts: Vec<Vec<f64>>,
    },
    SsiNormalize {
        other: SrfIn,
        n: [f64; 3],
        d: f64,
        pts: Vec<Vec<f64>>,
    },
    SsNormalize {
        other: SrfIn,
        sop: u8,
        vecs: Vec<[f64; 3]>,
        pts: Vec<Vec<f64>>,
    },
    Classify {
        dom: [f64; 4],
        curve: CrvIn,
        ivl: [f64; 2],
        uv: [f64; 2],
        t: f64,
    },
    /// `STEPInversion` of a plane (the B-spline base's, with the plane's virtual
    /// `IsSingularity` / `IsClosed`).
    StepInv {
        dom: [f64; 4],
        p: [f64; 3],
        tol: f64,
        guess: Option<[f64; 2]>,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    surface: SrfIn,
    op: OpIn,
}

// ---- C++ side -----------------------------------------------------------------------

fn push_pts(o: &mut Vec<f64>, pts: &[Vec<f64>]) {
    o.push(pts.len() as f64);
    for p in pts {
        o.extend_from_slice(p);
    }
}

fn push_opt(o: &mut Vec<f64>, x: Option<f64>) {
    o.extend([b(x.is_some()), x.unwrap_or(0.0)]);
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    push_srf(&mut v, &i.surface)?;
    let op = match &i.op {
        OpIn::NormalEval { uv } => {
            v.extend_from_slice(uv);
            Op::NormalEval
        }
        OpIn::SectionFn { uv, dir } | OpIn::SectionEval { uv, dir } => {
            v.extend_from_slice(uv);
            v.extend_from_slice(dir);
            if matches!(i.op, OpIn::SectionFn { .. }) {
                Op::SectionFn
            } else {
                Op::SectionEval
            }
        }
        OpIn::OnNormal { dom, uv, p, deriv, tol } => {
            v.push(b(dom.is_some()));
            v.extend_from_slice(&dom.unwrap_or([0.0; 4]));
            v.extend_from_slice(uv);
            v.extend_from_slice(p);
            v.push(b(deriv.is_some()));
            v.extend_from_slice(&deriv.unwrap_or([0.0; 3]));
            v.push(*tol);
            Op::OnNormal
        }
        OpIn::PassSp { uv, p, tol } => {
            v.extend_from_slice(uv);
            v.extend_from_slice(p);
            v.push(*tol);
            Op::PassSp
        }
        OpIn::Seam { param, known } => {
            v.extend([f64::from(*param), b(*known)]);
            Op::Seam
        }
        OpIn::IsOnSeam { uv, tol } => {
            v.extend_from_slice(uv);
            v.extend([b(tol.is_some()), tol.unwrap_or(1.0)]);
            Op::IsOnSeam
        }
        OpIn::Gps { dom, sop, p, tol, target, req } => {
            v.extend_from_slice(dom);
            v.push(f64::from(*sop));
            v.extend_from_slice(p);
            v.push(*tol);
            push_opt(&mut v, *target);
            v.push(f64::from(*req));
            Op::Gps
        }
        OpIn::CsIntersect { curve, pts } => {
            push_crv(&mut v, curve);
            push_pts(&mut v, pts);
            Op::CsIntersect
        }
        OpIn::CsNormalize { curve, sop, vecs, pts } => {
            push_crv(&mut v, curve);
            v.push(f64::from(*sop));
            push_vecs(&mut v, vecs);
            push_pts(&mut v, pts);
            Op::CsNormalize
        }
        OpIn::SspIntersect { other, steepest, plane, pts } => {
            push_srf(&mut v, other)?;
            let (o, n) = plane.unwrap_or(([0.0; 3], [0.0, 0.0, 1.0]));
            v.extend([b(*steepest), b(plane.is_some())]);
            v.extend_from_slice(&o);
            v.extend_from_slice(&n);
            push_pts(&mut v, pts);
            Op::SspIntersect
        }
        OpIn::SsiNormalize { other, n, d, pts } => {
            push_srf(&mut v, other)?;
            v.extend_from_slice(n);
            v.push(*d);
            push_pts(&mut v, pts);
            Op::SsiNormalize
        }
        OpIn::SsNormalize { other, sop, vecs, pts } => {
            push_srf(&mut v, other)?;
            v.push(f64::from(*sop));
            push_vecs(&mut v, vecs);
            push_pts(&mut v, pts);
            Op::SsNormalize
        }
        OpIn::Classify { dom, curve, ivl, uv, t } => {
            v.extend_from_slice(dom);
            push_crv(&mut v, curve);
            v.extend_from_slice(ivl);
            v.extend_from_slice(uv);
            v.push(*t);
            Op::Classify
        }
        OpIn::StepInv { dom, p, tol, guess } => {
            v.extend_from_slice(dom);
            v.extend_from_slice(p);
            v.extend([*tol, b(guess.is_some())]);
            v.extend_from_slice(&guess.unwrap_or([0.0, 0.0]));
            Op::StepInv
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

/// `Ok(v)` or the status of the failure (`Unported` defers the case).
fn stat<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    st(r)
}

fn side_left() -> Side {
    Side::Left
}

/// The upper half of the evaluation matrix of `s` at `uv`, with the unit normal in `[2][2]`
/// (as the shim builds it for `smsurf_EvaluateNormalSection`).
fn eval_matrix(k: &Kernel, s: &Surface, uv: [f64; 2]) -> Result<Result<[[Vec3; 3]; 3], f64>, Fail> {
    let flags = EvaluateFlags {
        u_side: side_left(),
        v_side: side_left(),
        only_upper_half: true,
        non_zero_tangents: true,
    };
    let m = match stat(s.evaluate(k, p2(uv), 2, 2, flags))? {
        Ok(m) => m,
        Err(c) => return Ok(Err(c)),
    };
    let mut pvv = [[Vec3::UNDEF; 3]; 3];
    for (i, row) in pvv.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            if i + j <= 2 {
                *slot = m
                    .get(i)
                    .and_then(|r| r.get(j))
                    .copied()
                    .ok_or_else(|| harness("short evaluation"))?;
            }
        }
    }
    let n = pvv[1][0].cross(pvv[0][1]);
    pvv[2][2] = if n.length_squared() < 1e-24 {
        match stat(s.evaluate_normal(k, p2(uv), side_left(), side_left()))? {
            Ok(n) => n,
            Err(c) => return Ok(Err(c)),
        }
    } else {
        n.unitize_or_z()
    };
    Ok(Ok(pvv))
}

fn reason_code(r: NormalTestReason) -> f64 {
    match r {
        NormalTestReason::Unknown => 0.0,
        NormalTestReason::OutsideOfDomain => 1.0,
        NormalTestReason::BadSurfacePoint => 2.0,
        NormalTestReason::NotWithinTolerance => 3.0,
    }
}

/// Evaluates an equation set at each of `pts`, recording `status [found F J]` per point.
fn run_enfo<E: EvalNFunctions>(
    e: &mut E,
    dim: usize,
    pts: &[Vec<f64>],
    o: &mut Vec<f64>,
) -> Result<(), Fail> {
    o.push(0.0);
    for x in pts {
        let mut f = vec![0.0; dim];
        let mut jac = Matrix::new(dim, dim);
        match stat(e.evaluate(x, &mut f, Some(&mut jac)))? {
            Ok(found) => {
                o.extend([0.0, b(found)]);
                if found {
                    // upstream returns early: only the last function value is set
                    o.push(f.last().copied().unwrap_or(0.0));
                    continue;
                }
                o.extend_from_slice(&f);
                for r in 0..dim {
                    for c in 0..dim {
                        o.push(jac.at(r, c).map_err(|e| harness(format!("jacobian: {e}")))?);
                    }
                }
            }
            Err(c) => o.push(c),
        }
    }
    Ok(())
}

fn vecs3(v: &[[f64; 3]]) -> Vec<Vec3> {
    v.iter().map(|&x| v3(x)).collect()
}

/// `m_cpOptVectors`: NULL when there are no vectors.
fn opt_vecs(v: &[Vec3]) -> Option<&[Vec3]> {
    (!v.is_empty()).then_some(v)
}

fn rust_run(k: &Kernel, i: &Inputs) -> Result<Vec<f64>, Fail> {
    let s = surface_of(&i.surface)?;
    let mut o = Vec::new();
    match &i.op {
        OpIn::NormalEval { uv } => {
            let mut e = SurfaceEval::with_uv(k, Some(&s), uv[0], uv[1]);
            match stat(e.normal(k))? {
                Ok(n) => {
                    o.push(0.0);
                    pv(&mut o, n);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::SectionFn { uv, dir } => match eval_matrix(k, &s, *uv)? {
            Ok(mut pvv) => match stat(evaluate_normal_section(&mut pvv, v3(*dir)))? {
                Ok((tangent, curvature)) => {
                    o.push(0.0);
                    pv(&mut o, tangent);
                    o.push(curvature);
                }
                Err(c) => o.push(c),
            },
            Err(c) => o.push(c),
        },
        OpIn::SectionEval { uv, dir } => {
            let mut e = SurfaceEval::with_uv_and_sect_dir(k, Some(&s), uv[0], uv[1], v3(*dir));
            match stat(e.eval())? {
                Ok(()) => {
                    // upstream ignores the status of the normal and the section
                    let n = match stat(e.normal(k))? {
                        Ok(n) => n,
                        Err(_) => e.values()[2][2],
                    };
                    let _ = stat(e.eval_normal_section(k))?;
                    let (t, k) = e.stored_section();
                    o.push(0.0);
                    pv(&mut o, n);
                    pv(&mut o, t);
                    o.push(k);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::OnNormal { dom, uv, p, deriv, tol } => {
            let d = dom.map(dom_of);
            match stat(is_point_on_normal(k, &s, d.as_ref(), p2(*uv), v3(*p), deriv.map(v3), *tol))? {
                Ok(r) => {
                    o.extend([0.0, b(r.from_left_u), b(r.from_left_v), b(r.good_point)]);
                    o.push(reason_code(r.reason));
                    o.push(r.drop_to_surf);
                    o.push(if r.reason == NormalTestReason::BadSurfacePoint {
                        0.0
                    } else {
                        r.approx_dev
                    });
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::PassSp { uv, p, tol } => match stat(pass_sp_convergence_test(k, &s, p2(*uv), v3(*p), *tol))? {
            Ok(pass) => o.extend([0.0, b(pass)]),
            Err(c) => o.push(c),
        },
        OpIn::Seam { param, known } => {
            let k = Kernel::new(&KernelOptions::default());
            match stat(s.create_seam_curve(&k, param_of(*param)?, *known))? {
                Ok(seam) => {
                    o.push(0.0);
                    o.push(b(seam.is_some()));
                    if let Some(c) = seam {
                        let iv = c.natural_interval();
                        o.extend([iv.min(), iv.max()]);
                        for n in 0..5 {
                            let t = iv.min() + (iv.max() - iv.min()) * f64::from(n) / 4.0;
                            match stat(c.evaluate_point(&k, t))? {
                                Ok(p) => {
                                    o.push(0.0);
                                    pv(&mut o, p);
                                }
                                Err(code) => {
                                    o.push(code);
                                    pv(&mut o, Vec3::UNDEF);
                                }
                            }
                        }
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::IsOnSeam { uv, tol } => match stat(s.is_on_seam(&Kernel::new(&KernelOptions::default()), p2(*uv), *tol))? {
            Ok(info) => {
                o.extend([0.0, b(info.on_seam), f64::from(info.seam_dir as i32)]);
                o.push(info.cross_seam_uvs.len() as f64);
                for q in &info.cross_seam_uvs {
                    o.extend([q.x, q.y]);
                }
            }
            Err(c) => o.push(c),
        },
        OpIn::Gps { dom, sop, p, tol, target, req } => {
            let k = Kernel::new(&KernelOptions::default());
            match stat(s.global_point_solve::<NoObject>(
                &k,
                Some(NoObject),
                &dom_of(*dom),
                sop_of(*sop)?,
                v3(*p),
                *tol,
                *target,
                req_of(*req)?,
            ))? {
                Ok(sols) => {
                    o.extend([0.0, sols.len() as f64]);
                    for sol in sols.as_slice() {
                        o.extend([
                            sol.start.solution_value,
                            sol.start.parameters.first().copied().unwrap_or(0.0),
                            sol.start.parameters.get(1).copied().unwrap_or(0.0),
                        ]);
                    }
                }
                Err(c) => o.extend([c, 0.0]),
            }
        }
        OpIn::StepInv { dom, p, tol, guess } => {
            let k = Kernel::new(&KernelOptions::default());
            match stat(s.step_inversion(&k, &dom_of(*dom), v3(*p), *tol, guess.map(p2)))? {
                Ok((uv, loc)) => o.extend([0.0, uv.x, uv.y, f64::from(loc as i32)]),
                Err(c) => o.push(c),
            }
        }
        OpIn::CsIntersect { curve, pts } => {
            let c = curve_of(curve)?;
            run_enfo(&mut CsIntersectEnfo::new(k, &c, &s, 0.0), 3, pts, &mut o)?;
        }
        OpIn::CsNormalize { curve, sop, vecs, pts } => {
            let c = curve_of(curve)?;
            let op = sop_of(*sop)?;
            let dim = if op == SolverOperation::Signed3dDirectedMinimize { 4 } else { 3 };
            let vs = vecs3(vecs);
            run_enfo(&mut CsNormalizeEnfo::new(k, &c, &s, op, 0.0, opt_vecs(&vs), 0.0), dim, pts, &mut o)?;
        }
        OpIn::SspIntersect { other, steepest, plane, pts } => {
            let s2 = surface_of(other)?;
            let pl = plane.map(|(po, pn)| Plane::from_normal(v3(po), v3(pn)));
            run_enfo(&mut SspIntersectEnfo::new(k, &s, &s2, *steepest, pl.as_ref(), 0.0), 4, pts, &mut o)?;
        }
        OpIn::SsiNormalize { other, n, d, pts } => {
            let s2 = surface_of(other)?;
            run_enfo(&mut SsiNormalizeEnfo::new(k, &s, &s2, 0.0, v3(*n), *d), 4, pts, &mut o)?;
        }
        OpIn::SsNormalize { other, sop, vecs, pts } => {
            let s2 = surface_of(other)?;
            let op = sop_of(*sop)?;
            let dim = if op == SolverOperation::Signed3dDirectedMinimize { 5 } else { 4 };
            let vs = vecs3(vecs);
            run_enfo(&mut SsNormalizeEnfo::new(k, &s, &s2, op, opt_vecs(&vs), 0.0), dim, pts, &mut o)?;
        }
        OpIn::Classify { dom, curve, ivl, uv, t } => {
            let c = curve_of(curve)?;
            match stat(classify_curve_surface_xsect_point(
                k, &s,
                &dom_of(*dom),
                &c,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                p2(*uv),
                *t,
            ))? {
                Ok(r) => {
                    o.extend([0.0, b(r.found_guess), r.new_uv_guess.x, r.new_uv_guess.y, r.new_t_guess]);
                    o.push(f64::from(r.point_type as i32));
                }
                Err(c) => o.push(c),
            }
        }
    }
    Ok(o)
}

fn rust(k: &Kernel, i: &Inputs) -> Result<Output, HarnessError> {
    match rust_run(k, i) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(o)) => Ok(*o),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generators ---------------------------------------------------------------------

/// The curve range `[a, b]` of a curve input.
fn crange(c: &CrvIn) -> (f64, f64) {
    (c.u.first().copied().unwrap_or(0.0), c.u.last().copied().unwrap_or(1.0))
}

/// A quadratic Bezier through the surface point at `uv` (at `t = 0.5`) whose tangent lies in
/// the surface's tangent plane, with curvature `1 + delta` times that of the surface's
/// normal section: a (near) tangent intersection. Returns the curve and `t`.
fn gen_tangent_curve(k: &Kernel, rng: &mut Rng, srf: &SrfIn, uv: [f64; 2]) -> Option<(CrvIn, f64)> {
    let s = surface_of(srf).ok()?;
    let m = s
        .evaluate(
            k, p2(uv),
            2,
            2,
            EvaluateFlags {
                u_side: Side::Left,
                v_side: Side::Left,
                only_upper_half: true,
                non_zero_tangents: false,
            },
        )
        .ok()?;
    let at = |i: usize, j: usize| m.get(i).and_then(|r| r.get(j)).copied();
    let (p, du, dv) = (at(0, 0)?, at(1, 0)?, at(0, 1)?);
    let n = du.cross(dv);
    if n.length_squared() < 1e-12 {
        return None;
    }
    let n = n.unitize().ok()?;
    let (a, c) = (rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0));
    let mut t = du * a + dv * c;
    if t.length_squared() < 1e-12 {
        t = du;
    }
    // sometimes along a principal-ish direction: a coordinate direction
    if rng.chance(0.3) {
        t = if rng.chance(0.5) { du } else { dv };
    }
    let t = t.unitize().ok()?;
    // normal-section curvature along t
    let mut pvv = [[Vec3::UNDEF; 3]; 3];
    for (i, row) in pvv.iter_mut().enumerate() {
        for (j, slot) in row.iter_mut().enumerate() {
            if i + j <= 2 {
                *slot = at(i, j)?;
            }
        }
    }
    pvv[2][2] = n;
    let (_, sec_curvature) = evaluate_normal_section(&mut pvv, t).ok()?;
    let len = *rng.choose(&[0.05, 0.3, 1.0]).unwrap_or(&0.3);
    let delta = *rng.choose(&[0.0, 1e-3, -1e-3, 0.5, -0.5, 3.0, -2.0, -1.0]).unwrap_or(&0.0);
    let k = if rng.chance(0.2) { 0.0 } else { sec_curvature * (1.0 + delta) };
    // curvature of the bezier is 2|a|/L^2 along sign(a) N
    let amp = k * len * len / 2.0;
    // tilt the curve out of the tangent plane by a small angle: its osculating circle then
    // crosses the surface's section circle, which gives near-tangent intersections
    let deg = *rng.choose(&[0.0, 0.0, 0.5, 2.0, 6.0, 12.0, 17.0]).unwrap_or(&0.0_f64);
    let phi = deg * std::f64::consts::PI / 180.0;
    let tt = t * phi.cos() + n * phi.sin();
    let nn = n * phi.cos() - t * phi.sin();
    let q0 = p - tt * len + nn * amp;
    let q2 = p + tt * len + nn * amp;
    let q1 = p - nn * amp;
    let cpts = [q0, q1, q2].iter().map(|q| [q.x, q.y, q.z, NOW]).collect();
    Some((
        CrvIn {
            dim: 3,
            cpts,
            p: 2,
            u: vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        },
        0.5,
    ))
}

fn gen_curve_t(rng: &mut Rng, c: &CrvIn) -> f64 {
    let (a, b) = crange(c);
    gen_param(rng, a, b, Some(&c.u))
}

/// `count` parameter vectors `(t, u, v)`: random, or a sequence converging on a point.
fn gen_pts3(rng: &mut Rng, srf: &SrfIn, c: &CrvIn, tangent: Option<([f64; 2], f64)>) -> Vec<Vec<f64>> {
    let count = if rng.chance(0.4) { 11 + rng.below(4) as usize } else { 1 + rng.below(4) as usize };
    (0..count)
        .map(|_| match tangent {
            Some((uv, t)) if rng.chance(0.85) => {
                let sh = *rng.choose(&[0.0, 1e-6, 1e-3, 0.01, 0.05]).unwrap_or(&0.0);
                vec![t + sh * rng.uniform(-1.0, 1.0), uv[0] + sh * rng.uniform(-1.0, 1.0), uv[1] + sh * rng.uniform(-1.0, 1.0)]
            }
            _ => {
                let uv = gen_uv(rng, srf);
                vec![gen_curve_t(rng, c), uv[0], uv[1]]
            }
        })
        .collect()
}

fn gen_pts4(rng: &mut Rng, s1: &SrfIn, s2: &SrfIn, same: bool) -> Vec<Vec<f64>> {
    let count = 1 + rng.below(4) as usize;
    (0..count)
        .map(|_| {
            let a = gen_uv(rng, s1);
            let c = if same {
                let sh = *rng.choose(&[0.0, 1e-9, 1e-4, 0.05]).unwrap_or(&0.0);
                [a[0] + sh * rng.uniform(-1.0, 1.0), a[1] + sh * rng.uniform(-1.0, 1.0)]
            } else {
                gen_uv(rng, s2)
            };
            vec![a[0], a[1], c[0], c[1]]
        })
        .collect()
}

fn gen_unit_vec(rng: &mut Rng) -> [f64; 3] {
    let mut v = rng.point3(-1.0, 1.0);
    if v.iter().map(|c| c * c).sum::<f64>() < 1e-3 {
        v = [0.0, 0.0, 1.0];
    }
    v
}

fn gen_signed_vec(rng: &mut Rng) -> Vec<[f64; 3]> {
    vec![gen_unit_vec(rng)]
}

fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    let mut v = rng.point3(-2.0, 2.0);
    if rng.chance(0.1) {
        v = [0.0; 3];
    }
    if v.iter().map(|c| c * c).sum::<f64>() < 1e-9 && rng.chance(0.5) {
        v = [1.0, 0.0, 0.0];
    }
    v
}

/// A domain for the global point solve: the surface's own sub-domain, or a small window of
/// a plane's unbounded one.
fn gen_solve_dom(rng: &mut Rng, s: &SrfIn) -> [f64; 4] {
    if matches!(s, SrfIn::Plane { .. }) {
        let (a, b) = (rng.uniform(-4.0, 0.0), rng.uniform(-4.0, 0.0));
        return [a, b, a + rng.uniform(0.5, 6.0), b + rng.uniform(0.5, 6.0)];
    }
    gen_dom(rng, s)
}

fn gen_gps(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> OpIn {
    OpIn::Gps {
        dom: gen_solve_dom(rng, s),
        sop: *rng.choose(&[0u8, 3, 3, 4, 5]).unwrap_or(&3),
        p: gen_point_near(k, rng, s),
        tol: *rng.choose(&[1e-9, 1e-6, 1e-5, 1e-3]).unwrap_or(&1e-5),
        target: if rng.chance(0.2) { Some(rng.uniform(0.0, 3.0)) } else { None },
        req: rng.below(2) as u8,
    }
}

fn gen_op(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> OpIn {
    let is_bspline = matches!(s, SrfIn::Bspline(_) | SrfIn::Sms(_));
    match rng.below(46) {
        0..=2 => OpIn::NormalEval { uv: gen_boundary_uv(rng, s) },
        3..=5 => OpIn::SectionFn { uv: gen_uv(rng, s), dir: gen_dir(rng) },
        6..=8 => OpIn::SectionEval { uv: gen_uv(rng, s), dir: gen_dir(rng) },
        9..=12 => {
            let uv = gen_uv(rng, s);
            let p = if rng.chance(0.4) {
                // on the normal: surface point plus a normal offset
                surface_of(s).ok().and_then(|srf| {
                    let pt = srf.evaluate_point(k, p2(uv)).ok()?;
                    let n = srf.evaluate_normal(k, p2(uv), Side::Left, Side::Left).ok()?;
                    let d = *rng.choose(&[0.0, 0.3, 2.0, -1.5]).unwrap_or(&0.3);
                    let e = *rng.choose(&[0.0, 1e-9, 1e-4, 0.1]).unwrap_or(&0.0);
                    Some([pt.x + n.x * d + e, pt.y + n.y * d, pt.z + n.z * d])
                })
            } else {
                None
            }
            .unwrap_or_else(|| gen_point_near(k, rng, s));
            let dom = if rng.chance(0.5) { Some(gen_dom(rng, s)) } else { None };
            let tol = *rng.choose(&[1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-3);
            let deriv = if rng.chance(0.4) { Some(rng.point3(-1.0, 1.0)) } else { None };
            // a UV point outside a given domain now and then
            let uv = if dom.is_some() && rng.chance(0.2) { gen_boundary_uv(rng, s) } else { uv };
            OpIn::OnNormal { dom, uv, p, deriv, tol }
        }
        13..=15 => OpIn::PassSp {
            uv: gen_uv(rng, s),
            p: gen_point_near(k, rng, s),
            tol: *rng.choose(&[1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-3),
        },
        16..=18 if is_bspline => OpIn::Seam { param: rng.below(2) as u8, known: rng.chance(0.2) },
        19..=20 => OpIn::IsOnSeam {
            uv: gen_boundary_uv(rng, s),
            tol: if rng.chance(0.4) { Some(*rng.choose(&[1e-9, 1e-5, 1e-2]).unwrap_or(&1e-5)) } else { None },
        },
        21..=24 => gen_gps(k, rng, s),
        25..=29 => {
            let (curve, tangent) = gen_curve_for(k, rng, s);
            let pts = gen_pts3(rng, s, &curve, tangent);
            OpIn::CsIntersect { curve, pts }
        }
        30..=33 => {
            let (curve, tangent) = gen_curve_for(k, rng, s);
            let sop = *rng.choose(&[3u8, 4, 5, 5, 9]).unwrap_or(&5);
            let signed = sop == 9;
            let mut pts = gen_pts3(rng, s, &curve, tangent);
            pts.truncate(4);
            if signed {
                for p in &mut pts {
                    p.push(rng.uniform(-1.0, 1.0));
                }
            }
            let vecs = if signed { gen_signed_vec(rng) } else { Vec::new() };
            OpIn::CsNormalize { curve, sop, vecs, pts }
        }
        34..=36 => {
            let same = rng.chance(0.25);
            let other = if same { s.clone() } else { gen_surface(rng) };
            let steepest = rng.chance(0.5);
            let plane = if !steepest || rng.chance(0.2) {
                Some((rng.point3(-2.0, 2.0), gen_unit_vec(rng)))
            } else {
                None
            };
            let pts = gen_pts4(rng, s, &other, same);
            OpIn::SspIntersect { other, steepest, plane, pts }
        }
        37..=39 => {
            let same = rng.chance(0.25);
            // two different planes through a common point reach the small-gap branch,
            // which evaluates the second normal on the other surface
            let crossing = match s {
                SrfIn::Plane { o, .. } if !same && rng.chance(0.3) => {
                    let mut n = gen_unit_vec(rng);
                    if rng.chance(0.5) {
                        n = [0.0, 1.0, 0.0];
                    }
                    Some(SrfIn::Plane { o: *o, n })
                }
                _ => None,
            };
            let other = match &crossing {
                Some(p) => p.clone(),
                None if same => s.clone(),
                None => gen_surface(rng),
            };
            let pts = if crossing.is_some() {
                vec![vec![0.0; 4]]
            } else {
                gen_pts4(rng, s, &other, same)
            };
            OpIn::SsiNormalize { other, n: gen_unit_vec(rng), d: rng.uniform(-2.0, 2.0), pts }
        }
        40..=42 => {
            let same = rng.chance(0.25);
            let other = if same { s.clone() } else { gen_surface(rng) };
            let sop = *rng.choose(&[3u8, 4, 5, 5, 9]).unwrap_or(&5);
            let signed = sop == 9;
            let mut pts = gen_pts4(rng, s, &other, same);
            if signed {
                for p in &mut pts {
                    p.push(rng.uniform(-1.0, 1.0));
                }
            }
            let vecs = if signed { gen_signed_vec(rng) } else { Vec::new() };
            OpIn::SsNormalize { other, sop, vecs, pts }
        }
        _ => {
            let uv = gen_uv(rng, s);
            let (curve, t) = match gen_tangent_curve(k, rng, s, uv) {
                Some((c, t)) if rng.chance(0.85) => (c, t),
                _ => {
                    let c = gen_curve(rng);
                    let t = gen_curve_t(rng, &c);
                    (c, t)
                }
            };
            let (a, bb) = crange(&curve);
            let ivl = if rng.chance(0.6) { [a, bb] } else { [a + 0.2 * (bb - a), a + 0.8 * (bb - a)] };
            OpIn::Classify { dom: gen_dom(rng, s), curve, ivl, uv, t }
        }
    }
}

/// A curve for an equation-set case: tangent to the surface at some point half the time (with
/// that point, for generating the parameters), else random.
#[allow(clippy::type_complexity)]
fn gen_curve_for(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> (CrvIn, Option<([f64; 2], f64)>) {
    if rng.chance(0.5) {
        let uv = gen_uv(rng, s);
        if let Some((c, t)) = gen_tangent_curve(k, rng, s, uv) {
            return (c, Some((uv, t)));
        }
    }
    (gen_curve(rng), None)
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> Inputs {
    // the global point solve is only comparable on planes: give them their share
    if rng.chance(0.08) {
        let surface = gen_plane(rng);
        let mut op = gen_gps(k, rng, &surface);
        if let OpIn::Gps { sop, .. } = &mut op {
            *sop = *rng.choose(&[0u8, 3, 3, 3]).unwrap_or(&3);
        }
        return Inputs { surface, op };
    }
    // STEPInversion is only comparable on planes (the B-spline path needs the local solve)
    if rng.chance(0.06) {
        let surface = gen_plane(rng);
        let dom = gen_solve_dom(rng, &surface);
        let uv = [rng.uniform(dom[0], dom[2]), rng.uniform(dom[1], dom[3])];
        let on = surface_of(&surface)
            .ok()
            .and_then(|srf| srf.evaluate_point(k, p2(uv)).ok())
            .map(|q| [q.x, q.y, q.z]);
        let p = match on {
            Some(q) if rng.chance(0.8) => q,
            _ => gen_point_near(k, rng, &surface),
        };
        let op = OpIn::StepInv {
            dom,
            p,
            tol: *rng.choose(&[1e-9, 1e-6, 1e-3]).unwrap_or(&1e-6),
            guess: rng.chance(0.15).then(|| [rng.uniform(dom[0], dom[2]), rng.uniform(dom[1], dom[3])]),
        };
        return Inputs { surface, op };
    }
    let surface = gen_surface(rng);
    let op = gen_op(k, rng, &surface);
    Inputs { surface, op }
}

/// Whether the C++ global point solve can run on this input in reasonable time: only planes
/// and unit-scale surfaces.
fn gps_ok(i: &Inputs) -> bool {
    !matches!(i.op, OpIn::Gps { .. }) || matches!(i.surface, SrfIn::Plane { .. }) || unit_scale(&i.surface)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p4"
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
            if let Err(Fail::Harness(_)) = rust_run(k, &i) {
                continue;
            }
            if !gps_ok(&i) {
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
        let k = &Kernel::default();
        rust(k, &case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;


    /// Prints generated cases `P4_CASES` (comma separated indices) with both outputs (a
    /// debugging aid: `P4_CASES=146,168 cargo test --profile difftest -p difftest --features
    /// cpp --lib p4_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs P4_CASES"]
    fn p4_dump_case() {
        let k = &Kernel::default();
        for idx in std::env::var("P4_CASES").unwrap().split(',') {
            let i: usize = idx.parse().unwrap();
            let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p4", i));
            let v = Runner.generate(&mut rng).unwrap();
            let inp: Inputs = serde_json::from_value(v.clone()).unwrap();
            println!("CASE {i}: {}", serde_json::to_string(&v).unwrap());
            println!("  cpp : {:?}", cpp(&inp).map(|o| format!("{o:?}")));
            println!("  rust: {:?}", rust(k, &inp).map(|o| format!("{o:?}")));
        }
    }

    /// Generated cases cover every op, most of the comparable ones not deferred.
    #[test]
    fn generated_cases_are_diverse() {
        let k = &Kernel::default();
        let mut rng = Rng::new(7);
        let mut stats: BTreeMap<String, [usize; 3]> = BTreeMap::new();
        let mut classes: BTreeMap<(i64, i64), usize> = BTreeMap::new();
        for _ in 0..1500 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let i: Inputs = serde_json::from_value(v).unwrap();
            let name = serde_json::to_value(&i.op).unwrap()["op"].as_str().unwrap().to_string();
            let e = stats.entry(name).or_default();
            e[0] += 1;
            if let (OpIn::Classify { .. }, Ok(o)) = (&i.op, rust_run(k, &i)) {
                if o.len() == 6 {
                    *classes.entry((o[5] as i64, o[1] as i64)).or_default() += 1;
                }
            }
            match rust_run(k, &i) {
                Ok(_) => e[1] += 1,
                Err(_) => e[2] += 1,
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ran {} deferred {}", v[0], v[1], v[2]);
        }
        eprintln!("classify (type, found): {classes:?}");
        assert!(stats.len() >= 13, "{}", stats.len());
    }
}
