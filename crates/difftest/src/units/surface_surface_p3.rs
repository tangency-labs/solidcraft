//! Runner for unit `surface.surface_p3`: `SmSurface` part 3 -- `EvaluateGeometric`,
//! `EvaluateNormalSection`, `GetSingularities`, `CoverCoincidentSurface`,
//! `smsurf_CheckShapeMatch`, `LocalPlaneSurfaceIntersect`, `LocalSurfaceSolve`,
//! `LocalSurfaceSurfaceIntersect` and the Newton equation sets `SmFindPSNormalENFO` and
//! `SmSSSIntersectENFO` (`SmSurfaceEval::EvalNormal` and `smsurf_IsPointOnNormal`, also owned
//! by the unit, were co-ported and are compared by the runner of unit `surface.surface_p4`).
//!
//! C++: `solidcore_ffi::units::surface_surface_p3` (upstream `source/SMLib/src/SmSurface.cpp`;
//! the file-local classes are verbatim copies). Rust: `solidcore::surface::surface::p3`.
//!
//! A case is one `op` on a `surface` (the input forms of unit `surface.surface_p7`). Both
//! sides produce the flat record documented in the ffi module (status first); the runner
//! reports it as the `out` values. A Rust error maps to its `SmStatus` (`1007` for
//! `InvalidInput`, `-1` for one upstream cannot return); `Unported` defers the case.
//!
//! Deferred cases: those that evaluate a curve-bounded surface (`SmSurface::Evaluate`), a
//! cover of a plane's drop (`SmSurface::GlobalPointSolve`) and a cover whose seam check needs
//! `SmSurface::EvaluateSTEP`.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::local_solve_nd::EvalNFunctions;
use solidcore::geom::matrix::Matrix;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::surface::surface::p2::GeometricProperties;
use solidcore::geom::global_solver::SolverOperation;
use solidcore::surface::surface::p1::{FindPsNormalEnfo, SssIntersectEnfo};
use solidcore::surface::surface::p3::check_shape_match;
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::surface_surface_p3::{self as ffi, Op};

use super::surface_surface_p7::{
    b, dom_of, gen_boundary_uv, gen_dom, gen_plane, gen_point_near, gen_surface, gen_uv, harness,
    p2, push_srf, push_vecs, pv, side, sop_of, st, surface_of, v3, Fail, SrfIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2500;

/// `surface.surface_p3` runner.
pub struct Runner;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    EvalGeometric {
        uv: [f64; 2],
        fu: bool,
        fv: bool,
    },
    NormalSection {
        uv: [f64; 2],
        fu: bool,
        fv: bool,
        dir: [f64; 3],
    },
    Singularities {
        pts: bool,
        nrm: bool,
    },
    Cover {
        other: SrfIn,
        tol: f64,
        dom: Option<[f64; 4]>,
    },
    ShapeMatch {
        other: SrfIn,
        uv0: [f64; 2],
        uv1: [f64; 2],
        tol: f64,
        ang: f64,
    },
    FindPsNormal {
        p: [f64; 3],
        sq_tol: f64,
        pts: Vec<Vec<f64>>,
    },
    SssIntersect {
        s2: SrfIn,
        s3: SrfIn,
        pts: Vec<Vec<f64>>,
    },
    LocalPlane {
        other: SrfIn,
        dom1: [f64; 4],
        dom2: [f64; 4],
        tol: f64,
        g1: [f64; 2],
        g2: [f64; 2],
        po: [f64; 3],
        pn: [f64; 3],
    },
    LocalSolve {
        other: SrfIn,
        dom1: [f64; 4],
        dom2: [f64; 4],
        sop: u8,
        vecs: Vec<[f64; 3]>,
        g1: [f64; 2],
        g2: [f64; 2],
    },
    LocalSss {
        s2: SrfIn,
        s3: SrfIn,
        doms: [[f64; 4]; 3],
        g1: [f64; 2],
        g2: [f64; 2],
        g3: [f64; 2],
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

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut v = Vec::new();
    push_srf(&mut v, &i.surface)?;
    let op = match &i.op {
        OpIn::EvalGeometric { uv, fu, fv } => {
            v.extend_from_slice(uv);
            v.extend([b(*fu), b(*fv)]);
            Op::EvalGeometric
        }
        OpIn::NormalSection { uv, fu, fv, dir } => {
            v.extend_from_slice(uv);
            v.extend([b(*fu), b(*fv)]);
            v.extend_from_slice(dir);
            Op::NormalSection
        }
        OpIn::Singularities { pts, nrm } => {
            v.extend([b(*pts), b(*nrm)]);
            Op::Singularities
        }
        OpIn::Cover { other, tol, dom } => {
            push_srf(&mut v, other)?;
            v.push(*tol);
            v.push(b(dom.is_some()));
            v.extend_from_slice(&dom.unwrap_or([0.0, 0.0, 1.0, 1.0]));
            Op::Cover
        }
        OpIn::ShapeMatch { other, uv0, uv1, tol, ang } => {
            push_srf(&mut v, other)?;
            v.extend_from_slice(uv0);
            v.extend_from_slice(uv1);
            v.extend([*tol, *ang]);
            Op::ShapeMatch
        }
        OpIn::FindPsNormal { p, sq_tol, pts } => {
            v.extend_from_slice(p);
            v.push(*sq_tol);
            push_pts(&mut v, pts);
            Op::FindPsNormal
        }
        OpIn::SssIntersect { s2, s3, pts } => {
            push_srf(&mut v, s2)?;
            push_srf(&mut v, s3)?;
            push_pts(&mut v, pts);
            Op::SssIntersect
        }
        OpIn::LocalPlane { other, dom1, dom2, tol, g1, g2, po, pn } => {
            push_srf(&mut v, other)?;
            v.extend_from_slice(dom1);
            v.extend_from_slice(dom2);
            v.push(*tol);
            v.extend_from_slice(g1);
            v.extend_from_slice(g2);
            v.extend_from_slice(po);
            v.extend_from_slice(pn);
            Op::LocalPlane
        }
        OpIn::LocalSolve { other, dom1, dom2, sop, vecs, g1, g2 } => {
            push_srf(&mut v, other)?;
            v.extend_from_slice(dom1);
            v.extend_from_slice(dom2);
            v.push(f64::from(*sop));
            push_vecs(&mut v, vecs);
            v.extend_from_slice(g1);
            v.extend_from_slice(g2);
            Op::LocalSolve
        }
        OpIn::LocalSss { s2, s3, doms, g1, g2, g3 } => {
            push_srf(&mut v, s2)?;
            push_srf(&mut v, s3)?;
            for d in doms {
                v.extend_from_slice(d);
            }
            v.extend_from_slice(g1);
            v.extend_from_slice(g2);
            v.extend_from_slice(g3);
            Op::LocalSss
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

/// The upper half of the evaluation matrix of `s` at `uv` (exact tangents), with the unit
/// normal in `[2][2]` (as the shim builds it).
fn eval_matrix(k: &Kernel, s: &Surface, uv: [f64; 2]) -> Result<Result<[[Vec3; 3]; 3], f64>, Fail> {
    let flags = EvaluateFlags {
        u_side: Side::Left,
        v_side: Side::Left,
        only_upper_half: true,
        non_zero_tangents: false,
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
        match stat(s.evaluate_normal(k, p2(uv), Side::Left, Side::Left))? {
            Ok(n) => n,
            Err(c) => return Ok(Err(c)),
        }
    } else {
        n.unitize_or_z()
    };
    Ok(Ok(pvv))
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

fn push_geometric(o: &mut Vec<f64>, g: &GeometricProperties) {
    o.extend([
        g.gaussian_curvature,
        g.normal_curvature,
        g.principle_curvature1,
        g.principle_curvature2,
    ]);
    pv(o, g.efg_of_first_fund_form);
    pv(o, g.lmn_of_second_fund_form);
    pv(o, g.principle_curvature_vector1);
    pv(o, g.principle_curvature_vector2);
}

fn vecs3(v: &[[f64; 3]]) -> Vec<Vec3> {
    v.iter().map(|&x| v3(x)).collect()
}

/// `found [value u0 v0 ..]` of a local solve.
fn push_found(o: &mut Vec<f64>, found: Option<(f64, Vec<f64>)>) {
    o.push(b(found.is_some()));
    if let Some((value, params)) = found {
        o.push(value);
        o.extend(params);
    }
}

fn rust_run(k: &Kernel, i: &Inputs) -> Result<Vec<f64>, Fail> {
    let mut s = surface_of(&i.surface)?;
    let mut o = Vec::new();
    match &i.op {
        OpIn::EvalGeometric { uv, fu, fv } => {
            match stat(s.evaluate_geometric(k, p2(*uv), side(*fu), side(*fv)))? {
                Ok(g) => {
                    o.push(0.0);
                    push_geometric(&mut o, &g);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::NormalSection { uv, fu, fv, dir } => {
            match stat(s.evaluate_normal_section(k, p2(*uv), side(*fu), side(*fv), v3(*dir)))? {
                Ok(sec) => {
                    o.push(0.0);
                    pv(&mut o, sec.tangent);
                    o.push(sec.curvature);
                    pv(&mut o, sec.normal);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Singularities { pts, nrm } => {
            let k = Kernel::new(&KernelOptions::default());
            match stat(s.get_singularities(&k, *pts, *nrm))? {
                Ok(sing) => {
                    o.extend([0.0, f64::from(sing.singular), f64::from(sing.approx_singular)]);
                    for p in sing.pole_points.iter().flatten() {
                        pv(&mut o, *p);
                    }
                    for n in sing.pole_normals.iter().flatten() {
                        pv(&mut o, *n);
                    }
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::Cover { other, tol, dom } => {
            let other = surface_of(other)?;
            let d = dom.map(dom_of);
            let k = Kernel::new(&KernelOptions::default());
            match stat(s.cover_coincident_surface(&k, &other, *tol, d.as_ref()))? {
                Ok(r) => {
                    o.extend([0.0, b(r.success), r.max_dist]);
                    if let Some(c) = r.covered_step_domain.filter(|_| r.success) {
                        o.extend([c.u_min(), c.v_min(), c.u_max(), c.v_max()]);
                    }
                    let d = s.step_uv_domain();
                    o.extend([d.u_min(), d.v_min(), d.u_max(), d.v_max()]);
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::ShapeMatch { other, uv0, uv1, tol, ang } => {
            let s2 = surface_of(other)?;
            let mut p0 = match eval_matrix(k, &s, *uv0)? {
                Ok(m) => m,
                Err(c) => return Ok(vec![c]),
            };
            let mut p1 = match eval_matrix(k, &s2, *uv1)? {
                Ok(m) => m,
                Err(c) => return Ok(vec![c]),
            };
            match stat(check_shape_match(&mut p0, &mut p1, *tol, *ang))? {
                Ok(m) => o.extend([0.0, b(m)]),
                Err(c) => o.push(c),
            }
        }
        OpIn::FindPsNormal { p, sq_tol, pts } => {
            run_enfo(&mut FindPsNormalEnfo::new(k, v3(*p), &s, *sq_tol, SolverOperation::Minimize), 2, pts, &mut o)?;
        }
        OpIn::SssIntersect { s2, s3, pts } => {
            let (s2, s3) = (surface_of(s2)?, surface_of(s3)?);
            run_enfo(&mut SssIntersectEnfo::new(k, &s, &s2, &s3, 0.0), 6, pts, &mut o)?;
        }
        OpIn::LocalPlane { other, dom1, dom2, tol, g1, g2, po, pn } => {
            let s2 = surface_of(other)?;
            match stat(s.local_plane_surface_intersect(
                k, &dom_of(*dom1),
                &s2,
                &dom_of(*dom2),
                *tol,
                p2(*g1),
                p2(*g2),
                v3(*po),
                v3(*pn),
            ))? {
                Ok(sol) => {
                    o.push(0.0);
                    push_found(
                        &mut o,
                        sol.map(|r| (r.distance, vec![r.uv.x, r.uv.y, r.other_uv.x, r.other_uv.y])),
                    );
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::LocalSolve { other, dom1, dom2, sop, vecs, g1, g2 } => {
            let s2 = surface_of(other)?;
            let vs = vecs3(vecs);
            match stat(s.local_surface_solve(
                k, &dom_of(*dom1),
                &s2,
                &dom_of(*dom2),
                sop_of(*sop)?,
                &vs,
                p2(*g1),
                p2(*g2),
            ))? {
                Ok(sol) => {
                    o.push(0.0);
                    push_found(
                        &mut o,
                        sol.map(|r| (r.distance, vec![r.uv.x, r.uv.y, r.other_uv.x, r.other_uv.y])),
                    );
                }
                Err(c) => o.push(c),
            }
        }
        OpIn::LocalSss { s2, s3, doms, g1, g2, g3 } => {
            let (s2, s3) = (surface_of(s2)?, surface_of(s3)?);
            match stat(s.local_surface_surface_intersect(
                k, &dom_of(doms[0]),
                &s2,
                &dom_of(doms[1]),
                &s3,
                &dom_of(doms[2]),
                p2(*g1),
                p2(*g2),
                p2(*g3),
            ))? {
                Ok(sol) => {
                    o.push(0.0);
                    push_found(
                        &mut o,
                        sol.map(|r| {
                            (
                                r.distance,
                                vec![r.uv1.x, r.uv1.y, r.uv2.x, r.uv2.y, r.uv3.x, r.uv3.y],
                            )
                        }),
                    );
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

/// A surface of revolution of a line about an axis: the line's first end lies on the axis
/// a third of the time (a pole, for `GetSingularities`), the out-of-bounds flag is random.
fn gen_revolution(rng: &mut Rng) -> SrfIn {
    let mut axis = rng.point3(-1.0, 1.0);
    if axis.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
        axis = [0.0, 0.0, 1.0];
    }
    let p = rng.point3(-2.0, 2.0);
    let r = [0.3, 0.5, 0.8];
    let perp = [
        axis[1] * r[2] - axis[2] * r[1],
        axis[2] * r[0] - axis[0] * r[2],
        axis[0] * r[1] - axis[1] * r[0],
    ];
    let at = |h: f64, w: f64| {
        [
            p[0] + axis[0] * h + perp[0] * w,
            p[1] + axis[1] * h + perp[1] * w,
            p[2] + axis[2] * h + perp[2] * w,
        ]
    };
    let w0 = if rng.chance(0.33) { 0.0 } else { rng.uniform(0.2, 2.0) };
    SrfIn::Revolution {
        a: at(rng.uniform(-2.0, 2.0), w0),
        b: at(rng.uniform(-2.0, 2.0), rng.uniform(0.2, 2.0)),
        p,
        axis,
        swap: rng.chance(0.3),
        oob: rng.chance(0.5),
    }
}

/// [`gen_surface`] with some surfaces of revolution.
fn gen_surface3(rng: &mut Rng) -> SrfIn {
    if rng.chance(0.15) {
        gen_revolution(rng)
    } else {
        gen_surface(rng)
    }
}

/// A domain to solve in: the surface's own sub-domain, or a small window of a plane's
/// unbounded one.
fn gen_solve_dom(rng: &mut Rng, s: &SrfIn) -> [f64; 4] {
    if matches!(s, SrfIn::Plane { .. } | SrfIn::Revolution { .. }) {
        let (a, bb) = (rng.uniform(-4.0, 0.0), rng.uniform(-4.0, 0.0));
        return [a, bb, a + rng.uniform(0.5, 6.0), bb + rng.uniform(0.5, 6.0)];
    }
    gen_dom(rng, s)
}

/// A parameter pair inside `dom`.
fn uv_in(rng: &mut Rng, dom: [f64; 4]) -> [f64; 2] {
    [rng.uniform(dom[0], dom[2]), rng.uniform(dom[1], dom[3])]
}

/// A parameter pair near `uv` (a shift of at most a few percent of the domain).
fn uv_near(rng: &mut Rng, uv: [f64; 2], dom: [f64; 4]) -> [f64; 2] {
    let sh = *rng.choose(&[0.0, 1e-9, 1e-4, 0.01, 0.05]).unwrap_or(&0.01);
    [
        (uv[0] + sh * (dom[2] - dom[0]) * rng.uniform(-1.0, 1.0)).clamp(dom[0], dom[2]),
        (uv[1] + sh * (dom[3] - dom[1]) * rng.uniform(-1.0, 1.0)).clamp(dom[1], dom[3]),
    ]
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

fn gen_signed_vec(rng: &mut Rng) -> Vec<[f64; 3]> {
    let mut v = rng.point3(-1.0, 1.0);
    if v.iter().map(|c| c * c).sum::<f64>() < 1e-3 {
        v = [0.0, 0.0, 1.0];
    }
    vec![v]
}

/// The single vector of [`gen_signed_vec`].
fn gen_signed_dir(rng: &mut Rng) -> [f64; 3] {
    gen_signed_vec(rng).first().copied().unwrap_or([0.0, 0.0, 1.0])
}

/// The other surface of a two-surface case: the surface again (a coincident pair), a plane
/// (which meets most surfaces) or another random surface.
fn gen_other(rng: &mut Rng, s: &SrfIn) -> (SrfIn, bool) {
    match rng.below(10) {
        0..=2 => (s.clone(), true),
        3..=6 => (gen_plane(rng), false),
        _ => (gen_surface3(rng), false),
    }
}

/// A guess for the other surface of a two-surface case.
fn gen_guess2(rng: &mut Rng, same: bool, other: &SrfIn, dom2: [f64; 4], g1: [f64; 2]) -> [f64; 2] {
    if same {
        uv_near(rng, g1, dom2)
    } else {
        let _ = other;
        uv_in(rng, dom2)
    }
}

fn gen_op(k: &Kernel, rng: &mut Rng, s: &SrfIn) -> OpIn {
    let is_bspline = matches!(s, SrfIn::Bspline(_) | SrfIn::Sms(_));
    match rng.below(50) {
        0..=5 => OpIn::EvalGeometric {
            uv: gen_boundary_uv(rng, s),
            fu: rng.chance(0.7),
            fv: rng.chance(0.7),
        },
        6..=10 => OpIn::NormalSection {
            uv: gen_boundary_uv(rng, s),
            fu: rng.chance(0.7),
            fv: rng.chance(0.7),
            dir: gen_dir(rng),
        },
        11..=14 => OpIn::Singularities {
            pts: rng.chance(0.7),
            nrm: rng.chance(0.7),
        },
        15..=19 if is_bspline || matches!(s, SrfIn::Revolution { .. }) => {
            let (other, same) = gen_other(rng, s);
            let tol = *rng.choose(&[1e-9, 1e-7, 1e-5, 1e-3]).unwrap_or(&1e-5);
            let dom = if rng.chance(0.5) { Some(gen_dom(rng, &other)) } else { None };
            let _ = same;
            OpIn::Cover { other, tol, dom }
        }
        20..=24 => {
            let (other, same) = gen_other(rng, s);
            let uv0 = gen_uv(rng, s);
            let uv1 = if same && rng.chance(0.7) { uv0 } else { gen_uv(rng, &other) };
            OpIn::ShapeMatch {
                other,
                uv0,
                uv1,
                tol: *rng.choose(&[1e-9, 1e-6, 1e-3, 0.5]).unwrap_or(&1e-6),
                ang: *rng.choose(&[1e-9, 1e-6, 1e-3, 0.1]).unwrap_or(&1e-3),
            }
        }
        25..=28 => {
            let count = if rng.chance(0.4) { 8 + rng.below(4) as usize } else { 1 + rng.below(4) as usize };
            let target = gen_uv(rng, s);
            let pts = (0..count)
                .map(|_| {
                    let sh = *rng.choose(&[0.0, 1e-6, 1e-3, 0.05, 0.3]).unwrap_or(&0.0);
                    let g = gen_uv(rng, s);
                    vec![
                        target[0] + sh * (g[0] - target[0]),
                        target[1] + sh * (g[1] - target[1]),
                    ]
                })
                .collect();
            OpIn::FindPsNormal {
                p: gen_point_near(k, rng, s),
                sq_tol: *rng.choose(&[1e-24, 1e-18, 1e-12, 1e-6]).unwrap_or(&1e-12),
                pts,
            }
        }
        29..=32 => {
            let (s2, same2) = gen_other(rng, s);
            let s3 = if rng.chance(0.2) { s.clone() } else { gen_plane(rng) };
            let count = 1 + rng.below(3) as usize;
            let pts = (0..count)
                .map(|_| {
                    let a = gen_uv(rng, s);
                    let c = if same2 {
                        uv_near(rng, a, [a[0] - 1.0, a[1] - 1.0, a[0] + 1.0, a[1] + 1.0])
                    } else {
                        gen_uv(rng, &s2)
                    };
                    let d = gen_uv(rng, &s3);
                    vec![a[0], a[1], c[0], c[1], d[0], d[1]]
                })
                .collect();
            OpIn::SssIntersect { s2, s3, pts }
        }
        33..=38 => {
            let (other, same) = gen_other(rng, s);
            let dom1 = gen_solve_dom(rng, s);
            let dom2 = if same { dom1 } else { gen_solve_dom(rng, &other) };
            let g1 = uv_in(rng, dom1);
            let g2 = gen_guess2(rng, same, &other, dom2, g1);
            // a plane through the surface point at the guess
            let po = gen_point_near(k, rng, s);
            OpIn::LocalPlane {
                other,
                dom1,
                dom2,
                tol: *rng.choose(&[1e-9, 1e-6, 1e-4, 1e-2]).unwrap_or(&1e-6),
                g1,
                g2,
                po,
                // a zero normal exercises Unitize -> (0, 0, 1)
                pn: if rng.chance(0.12) { [0.0; 3] } else { gen_signed_dir(rng) },
            }
        }
        39..=44 => {
            let (other, same) = gen_other(rng, s);
            let dom1 = gen_solve_dom(rng, s);
            let dom2 = if same { dom1 } else { gen_solve_dom(rng, &other) };
            let sop = *rng.choose(&[3u8, 4, 5, 5, 9]).unwrap_or(&5);
            let g1 = uv_in(rng, dom1);
            let g2 = gen_guess2(rng, same, &other, dom2, g1);
            let vecs = if sop == 9 { gen_signed_vec(rng) } else { Vec::new() };
            OpIn::LocalSolve { other, dom1, dom2, sop, vecs, g1, g2 }
        }
        _ => {
            let s2 = gen_plane(rng);
            let s3 = if rng.chance(0.15) { s.clone() } else { gen_plane(rng) };
            let doms = [gen_solve_dom(rng, s), gen_solve_dom(rng, &s2), gen_solve_dom(rng, &s3)];
            OpIn::LocalSss {
                g1: uv_in(rng, doms[0]),
                g2: uv_in(rng, doms[1]),
                g3: uv_in(rng, doms[2]),
                s2,
                s3,
                doms,
            }
        }
    }
}

fn gen_case(k: &Kernel, rng: &mut Rng) -> Inputs {
    let surface = gen_surface3(rng);
    let op = gen_op(k, rng, &surface);
    Inputs { surface, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p3"
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

    /// Prints generated cases `P3_CASES` (comma separated indices) with both outputs (a
    /// debugging aid: `P3_CASES=146,168 cargo test --profile difftest -p difftest --features
    /// cpp --lib p3_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs P3_CASES"]
    fn p3_dump_case() {
let k = &Kernel::default();
        for idx in std::env::var("P3_CASES").unwrap().split(',') {
            let i: usize = idx.parse().unwrap();
            let mut rng = Rng::new(CaseContext::seed_for("surface.surface_p3", i));
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
        for _ in 0..1500 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let i: Inputs = serde_json::from_value(v).unwrap();
            let name = serde_json::to_value(&i.op).unwrap()["op"].as_str().unwrap().to_string();
            let e = stats.entry(name).or_default();
            e[0] += 1;
            match rust_run(k, &i) {
                Ok(_) => e[1] += 1,
                Err(_) => e[2] += 1,
            }
        }
        for (k, v) in &stats {
            eprintln!("{k}: total {} ran {} deferred {}", v[0], v[1], v[2]);
        }
        assert!(stats.len() >= 10, "{}", stats.len());
    }
}
