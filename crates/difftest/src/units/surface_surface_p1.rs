//! Runner for unit `surface.surface_p1`: the `SmSurface` members of the unit (evaluation
//! wrappers, degeneracy, periodicity and boundary tests, domain conversions, sizes, the
//! surface cache maker and the base `CalculateBoundingBox` / `GetDomainWithContinuity`),
//! `SmSTEPSurface`, and the file-local helpers of `SmSurface.cpp` the unit owns.
//!
//! C++: `solidcore_ffi::units::surface_surface_p1` (virtual members called through
//! `SmSurface*`, base bodies called qualified; the file-local helpers are copies in the
//! shim). Rust: `solidcore::surface::surface::p1`.
//!
//! A case is a surface `surf` (a B-spline, a plane or a curve-bounded surface), wrapped in
//! a STEP surface when `wrap`, and an `op` with its parameters. Both sides produce the flat
//! record documented in the ffi module; the runner reports its first entry as the `status`
//! (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a
//! Rust error upstream cannot produce) and the rest as the `out` values.
//!
//! `Sil` runs `CreateSilhouetteCurves` on an unwrapped B-spline or plane (translated first
//! by `Transform` when asked) and records every 3D and UV curve in full; the
//! `upstream_sil_adv.json` fixtures are the `prog_test` `my_test_sil_adv` faces
//! (`harness/fixturegen/sil_adv.py`), and `silNNNN` cases come from their own seed stream.
//!
//! `DomCont` makes the surface's cache first (UB-83: upstream reads a null cache) and
//! then calls `GetDomainWithContinuity`; `history` says what happened to that cache before
//! the call (made during a Boolean operation, so without tree and with empty continuity
//! arrays; then its tree built; or reused during a Boolean operation). The `domhistNNNN`
//! cases (their own seed stream) cover those histories, half of them at points off the
//! surface domain, where the end-knot discontinuities clamp the domain only once the
//! cache's tree has been built (`plan/findings.md`, "surface cache history").
//!
//! Deferred cases: the paths into unported units (every surface cache build needs
//! `SmSurface::CalculateContinuities`, `surface.surface_p6`: `Cache`, `BBox` of
//! curve-bounded and STEP surfaces, `DomCont` of every kind but STEP surfaces; the
//! iso curves of `ApproximateSize` over kinds other than B-splines).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::{Curve as SmCurve, CurveKind};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::geom_utility::p2::ContinuityType;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_surface::SurfParamType;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::nurbs::knots::Side;
use solidcore::surface::surface::p1::{
    base_is_degenerate_curve, base_is_degenerate_point, crossed_seam_or_pole,
    evaluate_normal_section, find_uv_curve_ivl_in_domain, get_surface_cache, make_surface_cache,
    make_trim_srf_cache, FindSingularParamWithGivenTangentDir, StepSurface,
};
use solidcore::surface::surface::p2::IsoCurveLengthIntegrand;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::surface_cache::p1::SurfaceCache;
use solidcore::error::SM_ERR;
use solidcore::SolidError;
use solidcore_ffi::units::surface_surface_p1::{self as ffi, Op, MAX_DIR_DERIV};

use super::surface_surface_cache_p1::{
    curve_of, dom_of, flag, gen_knots, gen_sub_dom, gvec, harness, push3, push_box, push_crv,
    push_dom, status, v3, CrvIn,
};
use super::surface_surface_cache_p3::{gen_surf, push_surf, surf_domain, surf_size, surface_in, SurfIn};
use super::surface_surface_p2::Handle;
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 800;

/// `surface.surface_p1` runner.
pub struct Runner;

/// The surface handle of the caches.
const SURFACE_HANDLE: i64 = -7;

// ---- inputs -----------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Eval {
        uv: [f64; 2],
        ul: bool,
        vl: bool,
    },
    Periodic {
        dom: [f64; 4],
        v: bool,
    },
    DegenPoint {
        tol: f64,
        dom: Option<[f64; 4]>,
    },
    DegenCurve {
        ang: f64,
        dom: Option<[f64; 4]>,
        tol: f64,
    },
    Flat {
        uv: [f64; 2],
        ang: f64,
    },
    OnBoundary {
        dom: [f64; 4],
        uv: [f64; 2],
        tol: Option<f64>,
    },
    InDomain {
        dom: [f64; 4],
        uv: [f64; 2],
        tol: Option<f64>,
    },
    Misc {
        other_wrap: bool,
        other: SurfIn,
    },
    Snap {
        uv: [f64; 2],
        tol: f64,
    },
    NSect {
        e: Vec<[f64; 3]>,
        dir: [f64; 3],
    },
    ParamDir {
        uv: [f64; 2],
        vec: [f64; 3],
    },
    DirDerivs {
        uv: [f64; 2],
        dir: [f64; 2],
        h: usize,
        ul: bool,
        vl: bool,
    },
    Convert {
        dom: [f64; 4],
    },
    Size {
        dom: Option<[f64; 4]>,
    },
    FindIvl {
        curve: CrvIn,
        ivl: [f64; 2],
        param: f64,
        dom: [f64; 4],
    },
    Sing {
        uv: [f64; 2],
        dir: [f64; 3],
        v: bool,
        tol: f64,
        s: f64,
    },
    Seam {
        prev: [f64; 2],
        curr: [f64; 2],
        dom: [f64; 4],
    },
    IsoLen {
        u: f64,
        t: f64,
    },
    Step {
        v: bool,
        ivl: Option<[f64; 2]>,
    },
    Cache {
        trim: bool,
    },
    BBox {
        dom: [f64; 4],
        normal: bool,
        pseudo: bool,
        expand: bool,
        /// Only for unwrapped B-splines and planes (upstream recurses on the others,
        /// UB-84).
        #[serde(default)]
        polar: bool,
    },
    DomCont {
        cont: u8,
        uv: [f64; 2],
        vec: [f64; 2],
        /// What happened to the surface's cache before the call (the shim's
        /// `op_dom_cont`): `0` made outside a Boolean operation, `1` made while doing
        /// one (no tree), `2` as `1` then its tree built, `3` as `0` with the call made
        /// while doing a Boolean operation.
        #[serde(default)]
        history: u8,
    },
    /// `CreateSilhouetteCurves` on the (unwrapped) surface, translated first by
    /// `translate` (`Transform`) when given.
    Sil {
        dom: [f64; 4],
        eye: [f64; 3],
        perspective: bool,
        tol: Option<f64>,
        ang: Option<f64>,
        #[serde(default)]
        translate: Option<[f64; 3]>,
    },
}

impl OpIn {
    fn ffi(&self) -> Op {
        match self {
            OpIn::Eval { .. } => Op::Eval,
            OpIn::Periodic { .. } => Op::Periodic,
            OpIn::DegenPoint { .. } => Op::DegenPoint,
            OpIn::DegenCurve { .. } => Op::DegenCurve,
            OpIn::Flat { .. } => Op::Flat,
            OpIn::OnBoundary { .. } => Op::OnBoundary,
            OpIn::InDomain { .. } => Op::InDomain,
            OpIn::Misc { .. } => Op::Misc,
            OpIn::Snap { .. } => Op::Snap,
            OpIn::NSect { .. } => Op::NSect,
            OpIn::ParamDir { .. } => Op::ParamDir,
            OpIn::DirDerivs { .. } => Op::DirDerivs,
            OpIn::Convert { .. } => Op::Convert,
            OpIn::Size { .. } => Op::Size,
            OpIn::FindIvl { .. } => Op::FindIvl,
            OpIn::Sing { .. } => Op::Sing,
            OpIn::Seam { .. } => Op::Seam,
            OpIn::IsoLen { .. } => Op::IsoLen,
            OpIn::Step { .. } => Op::Step,
            OpIn::Cache { .. } => Op::Cache,
            OpIn::BBox { .. } => Op::BBox,
            OpIn::DomCont { .. } => Op::DomCont,
            OpIn::Sil { .. } => Op::Sil,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    /// Wrap the surface in an `SmSTEPSurface` (unused by `Step`, `NSect`, `FindIvl`).
    #[serde(default)]
    wrap: bool,
    /// The surface (unused by `NSect`, `FindIvl`).
    surf: SurfIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -----------------------------------------------------------------------

fn push_wrapped(o: &mut Vec<f64>, wrap: bool, s: &SurfIn) -> Result<(), HarnessError> {
    o.push(flag(wrap));
    push_surf(o, s)
}

fn push_opt_dom(o: &mut Vec<f64>, d: Option<[f64; 4]>) {
    o.push(flag(d.is_some()));
    o.extend(d.unwrap_or([0.0, 0.0, 1.0, 1.0]));
}

fn push_opt_tol(o: &mut Vec<f64>, t: Option<f64>) {
    o.push(flag(t.is_some()));
    o.push(t.unwrap_or(0.0));
}

fn cpp_input(i: &Inputs) -> Result<(Op, Vec<f64>), HarnessError> {
    let mut o = Vec::new();
    let op = i.op.ffi();
    match &i.op {
        OpIn::NSect { .. } | OpIn::FindIvl { .. } | OpIn::Step { .. } => {}
        OpIn::Sil { .. } => {
            if i.wrap {
                return Err(harness("sil: the surface is not wrapped"));
            }
            push_surf(&mut o, &i.surf)?;
        }
        _ => push_wrapped(&mut o, i.wrap, &i.surf)?,
    }
    match &i.op {
        OpIn::Eval { uv, ul, vl } => {
            o.extend(uv);
            o.extend([flag(*ul), flag(*vl)]);
        }
        OpIn::Periodic { dom, v } => {
            o.extend(dom);
            o.push(flag(*v));
        }
        OpIn::DegenPoint { tol, dom } => {
            o.push(*tol);
            push_opt_dom(&mut o, *dom);
        }
        OpIn::DegenCurve { ang, dom, tol } => {
            o.push(*ang);
            push_opt_dom(&mut o, *dom);
            o.push(*tol);
        }
        OpIn::Flat { uv, ang } => {
            o.extend(uv);
            o.push(*ang);
        }
        OpIn::OnBoundary { dom, uv, tol } | OpIn::InDomain { dom, uv, tol } => {
            o.extend(dom);
            o.extend(uv);
            push_opt_tol(&mut o, *tol);
        }
        OpIn::Misc { other_wrap, other } => push_wrapped(&mut o, *other_wrap, other)?,
        OpIn::Snap { uv, tol } => {
            o.extend(uv);
            o.push(*tol);
        }
        OpIn::NSect { e, dir } => {
            if e.len() != 9 {
                return Err(harness("nsect: need 9 vectors"));
            }
            for v in e {
                o.extend(v);
            }
            o.extend(dir);
        }
        OpIn::ParamDir { uv, vec } => {
            o.extend(uv);
            o.extend(vec);
        }
        OpIn::DirDerivs { uv, dir, h, ul, vl } => {
            o.extend(uv);
            o.extend(dir);
            o.push(*h as f64);
            o.extend([flag(*ul), flag(*vl)]);
        }
        OpIn::Convert { dom } => o.extend(dom),
        OpIn::Size { dom } => push_opt_dom(&mut o, *dom),
        OpIn::FindIvl {
            curve,
            ivl,
            param,
            dom,
        } => {
            push_crv(&mut o, curve);
            o.extend(ivl);
            o.push(*param);
            o.extend(dom);
        }
        OpIn::Sing { uv, dir, v, tol, s } => {
            o.extend(uv);
            o.extend(dir);
            o.extend([flag(*v), *tol, *s]);
        }
        OpIn::Seam { prev, curr, dom } => {
            o.extend(prev);
            o.extend(curr);
            o.extend(dom);
        }
        OpIn::IsoLen { u, t } => o.extend([*u, *t]),
        OpIn::Step { v, ivl } => {
            push_surf(&mut o, &i.surf)?;
            o.push(flag(*v));
            o.push(flag(ivl.is_some()));
            o.extend(ivl.unwrap_or([0.0, 0.0]));
        }
        OpIn::Cache { trim } => o.push(flag(*trim)),
        OpIn::BBox {
            dom,
            normal,
            pseudo,
            expand,
            polar,
        } => {
            o.extend(dom);
            o.extend([flag(*normal), flag(*pseudo), flag(*expand), flag(*polar)]);
        }
        OpIn::DomCont {
            cont,
            uv,
            vec,
            history,
        } => {
            o.push(f64::from(*cont));
            o.extend(uv);
            o.extend(vec);
            o.push(f64::from(*history));
        }
        OpIn::Sil {
            dom,
            eye,
            perspective,
            tol,
            ang,
            translate,
        } => {
            o.extend(dom);
            o.push(flag(*perspective));
            o.extend(eye);
            push_opt_tol(&mut o, *tol);
            push_opt_tol(&mut o, *ang);
            o.push(flag(translate.is_some()));
            o.extend(translate.unwrap_or([0.0; 3]));
        }
    }
    Ok((op, o))
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

fn wrapped_of(wrap: bool, s: &SurfIn) -> Result<Surface, HarnessError> {
    let s = surface_in(s)?;
    Ok(if wrap {
        Surface::from(StepSurface::new(s, true).map_err(|e| HarnessError::new(e.to_string()))?)
    } else {
        s
    })
}

fn p2(uv: [f64; 2]) -> Point2 {
    Point2::new(uv[0], uv[1])
}

fn dir_of(v: bool) -> SurfParamType {
    if v {
        SurfParamType::V
    } else {
        SurfParamType::U
    }
}

fn side(left: bool) -> Side {
    if left {
        Side::Left
    } else {
        Side::Right
    }
}

fn continuity(code: u8) -> Result<ContinuityType, HarnessError> {
    use ContinuityType as C;
    Ok(match code {
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
        _ => return Err(harness(format!("bad continuity {code}"))),
    })
}

/// `D[i][j]` of an evaluation matrix (zero when not filled).
fn at(m: &[Vec<Vec3>], i: usize, j: usize) -> Vec3 {
    m.get(i)
        .and_then(|r| r.get(j))
        .copied()
        .unwrap_or(Vec3::ZERO)
}

/// The upper half of a `2 x 2` evaluation, as the shim's `put_upper`.
fn push_upper(o: &mut Vec<f64>, m: &[Vec<Vec3>]) {
    for i in 0..=2 {
        for j in 0..=(2 - i) {
            push3(o, at(m, i, j));
        }
    }
}

/// A sub-step result: `status [values]`, or the unported error.
fn sub<T>(
    o: &mut Vec<f64>,
    r: solidcore::Result<T>,
    put: impl FnOnce(&mut Vec<f64>, T),
) -> Result<(), SolidError> {
    match r {
        Ok(v) => {
            o.push(0.0);
            put(o, v);
            Ok(())
        }
        Err(e) if e.is_unported() => Err(e),
        Err(e) => {
            o.push(status(&e));
            Ok(())
        }
    }
}

/// `0 flags..` from boolean results (an error ends the record with its status).
fn flags(r: solidcore::Result<Vec<bool>>) -> Result<Vec<f64>, SolidError> {
    match r {
        Ok(v) => {
            let mut o = vec![0.0];
            o.extend(v.into_iter().map(flag));
            Ok(o)
        }
        Err(e) if e.is_unported() => Err(e),
        Err(e) => Ok(vec![status(&e)]),
    }
}

/// The outcome of a Rust op: the record, or an unported error.
type Run = Result<Vec<f64>, SolidError>;

fn top_box<O: Copy + PartialEq>(o: &mut Vec<f64>, c: &SurfaceCache<O>) {
    if let Some(t) = c.tree() {
        if let Some(n) = t.top_node().and_then(|id| t.node(id).ok()) {
            push_box(o, &n.bbox);
        }
    }
}

#[allow(clippy::too_many_lines)]
fn rust_op(i: &Inputs) -> Result<Run, HarnessError> {
    let k = Kernel::default();
    let run = || wrapped_of(i.wrap, &i.surf);
    Ok(match &i.op {
        OpIn::Eval { uv, ul, vl } => {
            let s = run()?;
            let uv = p2(*uv);
            let fl = EvaluateFlags {
                u_side: side(*ul),
                v_side: side(*vl),
                only_upper_half: true,
                non_zero_tangents: true,
            };
            let mut o = Vec::new();
            let r = (|| -> Result<(), SolidError> {
                sub(&mut o, s.evaluate_1st_derivatives(&k, uv, *ul, *vl), |o, (p, du, dv)| {
                    push3(o, p);
                    push3(o, du);
                    push3(o, dv);
                })?;
                sub(&mut o, s.evaluate_2nd_derivatives(&k, uv, *ul, *vl), |o, d| {
                    for v in d {
                        push3(o, v);
                    }
                })?;
                sub(&mut o, s.evaluate_step(&k, uv, 2, 2, fl), |o, m| push_upper(o, &m))?;
                sub(&mut o, s.evaluate_step_point(&k, uv), push3)?;
                sub(&mut o, s.evaluate_simple(&k, uv, 2, 2, fl), |o, m| push_upper(o, &m))
            })();
            r.map(|()| o)
        }
        OpIn::Periodic { dom, v } => {
            let s = run()?;
            flags(s.is_periodic(&k, &dom_of(*dom), dir_of(*v)).map(|b| vec![b]))
        }
        OpIn::DegenPoint { tol, dom } => {
            let s = run()?;
            let d = dom.map(dom_of);
            flags((|| {
                Ok(vec![
                    base_is_degenerate_point(&k, &s, *tol, d.as_ref())?,
                    s.is_degenerate_point(&k, *tol, d.as_ref())?,
                ])
            })())
        }
        OpIn::DegenCurve { ang, dom, tol } => {
            let s = run()?;
            let d = dom.map(dom_of);
            flags((|| {
                Ok(vec![
                    base_is_degenerate_curve(&k, &s, *ang, d.as_ref(), *tol)?,
                    s.is_degenerate_curve(&k, *ang, d.as_ref(), *tol)?,
                ])
            })())
        }
        OpIn::Flat { uv, ang } => {
            let s = run()?;
            flags(s.is_flat_corner(&k, p2(*uv), *ang).map(|b| vec![b]))
        }
        OpIn::OnBoundary { dom, uv, tol } => {
            let s = run()?;
            match s.is_on_boundary(&k, &dom_of(*dom), p2(*uv), *tol) {
                Ok((on, side, ou, ov)) => Ok(vec![
                    0.0,
                    flag(on),
                    f64::from(side as u8),
                    ou.unwrap_or(0.0),
                    ov.unwrap_or(0.0),
                ]),
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status(&e)]),
            }
        }
        OpIn::InDomain { dom, uv, tol } => {
            let s = run()?;
            flags(s.is_in_domain(&k, &dom_of(*dom), p2(*uv), *tol).map(|b| vec![b]))
        }
        OpIn::Misc { other_wrap, other } => {
            let a = run()?;
            let b = wrapped_of(*other_wrap, other)?;
            match (a.approx_eq(&k, &b), a.is_analytic(), a.face()) {
                (Ok(eq), Ok(an), Ok(face)) => Ok(vec![
                    0.0,
                    flag(eq),
                    flag(a.base_eq(&b)),
                    flag(an),
                    flag(face.is_none()),
                    a.type_code() as f64,
                ]),
                (Err(e), _, _) | (_, Err(e), _) | (_, _, Err(e)) => {
                    if e.is_unported() {
                        Err(e)
                    } else {
                        Ok(vec![status(&e)])
                    }
                }
            }
        }
        OpIn::Snap { uv, tol } => {
            let s = run()?;
            let mut o = Vec::new();
            sub(&mut o, s.snap_to_knots(p2(*uv), *tol), |o, p| {
                o.extend([p.x, p.y]);
            })
            .map(|()| o)
        }
        OpIn::NSect { e, dir } => {
            let mut m = [[Vec3::ZERO; 3]; 3];
            for (k, v) in e.iter().enumerate() {
                if let Some(slot) = m.get_mut(k / 3).and_then(|r| r.get_mut(k % 3)) {
                    *slot = v3(*v);
                }
            }
            match evaluate_normal_section(&mut m, v3(*dir)) {
                Ok((t, curv)) => {
                    let mut o = vec![0.0];
                    push3(&mut o, t);
                    o.push(curv);
                    for row in m {
                        for v in row {
                            push3(&mut o, v);
                        }
                    }
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status(&e)]),
            }
        }
        OpIn::ParamDir { uv, vec } => {
            let s = run()?;
            let mut o = Vec::new();
            sub(&mut o, s.compute_param_direction(&k, p2(*uv), v3(*vec)), |o, d| {
                o.extend([d.x, d.y]);
            })
            .map(|()| o)
        }
        OpIn::DirDerivs { uv, dir, h, ul, vl } => {
            if *h > MAX_DIR_DERIV {
                return Err(harness("derivative order too high"));
            }
            let s = run()?;
            let mut o = Vec::new();
            let r = s.evaluate_directional_derivs(
                &k, p2(*uv),
                Vec2::new(dir[0], dir[1]),
                *h,
                *ul,
                *vl,
            );
            let n = *h + 1;
            sub(&mut o, r, |o, d| {
                for v in d.into_iter().take(n) {
                    push3(o, v);
                }
            })
            .map(|()| o)
        }
        OpIn::Convert { dom } => {
            let s = run()?;
            let d = dom_of(*dom);
            let mut o = vec![0.0];
            (|| -> Result<(), SolidError> {
                sub(&mut o, s.convert_domain_from_step_to_nurbs(&k, &d), |o, x| {
                    push_dom(o, &x);
                })?;
                sub(&mut o, s.convert_domain_from_nurbs_to_step(&k, &d), |o, x| {
                    push_dom(o, &x);
                })
            })()
            .map(|()| o)
        }
        OpIn::Size { dom } => {
            let s = run()?;
            let d = dom.map(dom_of);
            let mut o = Vec::new();
            sub(&mut o, s.approximate_size(&k, d.as_ref()), |o, v| {
                o.extend([v.x, v.y]);
            })
            .map(|()| o)
        }
        OpIn::FindIvl {
            curve,
            ivl,
            param,
            dom,
        } => {
            let c = SmCurve::from(curve_of(curve)?);
            let mut o = Vec::new();
            let r = find_uv_curve_ivl_in_domain::<Handle>(
                &k,
                &c,
                &Extent1d::from_min_max(ivl[0], ivl[1]),
                *param,
                &dom_of(*dom),
            );
            sub(&mut o, r, |o, x| o.extend([x.min(), x.max()])).map(|()| o)
        }
        OpIn::Sing { uv, dir, v, tol, s: t } => {
            let s = run()?;
            let f = FindSingularParamWithGivenTangentDir::new(&k, &s, p2(*uv), v3(*dir), dir_of(*v), *tol);
            let mut o = Vec::new();
            sub(&mut o, f.evaluate(*t), |o, (a, b, found)| {
                o.extend([a, b, flag(found)]);
            })
            .map(|()| o)
        }
        OpIn::Seam { prev, curr, dom } => {
            let s = run()?;
            Ok(vec![
                0.0,
                flag(crossed_seam_or_pole(&s, p2(*prev), p2(*curr), &dom_of(*dom))),
            ])
        }
        OpIn::IsoLen { u, t } => {
            let s = run()?;
            let mut o = Vec::new();
            sub(&mut o, IsoCurveLengthIntegrand::new(&s, *u).evaluate(&k, *t), |o, v| {
                o.push(v);
            })
            .map(|()| o)
        }
        OpIn::Step { v, ivl } => {
            let s = StepSurface::new(surface_in(&i.surf)?, true).map_err(|e| HarnessError::new(e.to_string()))?;
            let mut o = vec![0.0];
            push_dom(&mut o, &s.natural_uv_domain());
            match s.domain_with_continuity() {
                Ok(d) => push_dom(&mut o, &d),
                Err(e) => return Ok(Err(e)),
            }
            let iv = ivl.map(|[a, b]| Extent1d::from_raw(a, b));
            sub(&mut o, s.knots(dir_of(*v), iv.as_ref()), |o, (knots, mults)| {
                o.push(knots.len() as f64);
                o.extend(&knots);
                o.extend(mults.iter().map(|m| *m as f64));
            })
            .map(|()| o)
        }
        OpIn::Cache { trim } => {
            let s = run()?;
            let mut o = Vec::new();
            if *trim {
                sub(&mut o, make_trim_srf_cache(&k, &s, Some(SURFACE_HANDLE)), |o, c| {
                    top_box(o, c.base());
                })
                .map(|()| o)
            } else {
                sub(&mut o, make_surface_cache(&k, &s, Some(SURFACE_HANDLE)), |o, c| {
                    top_box(o, &c);
                })
                .map(|()| o)
            }
        }
        OpIn::BBox {
            dom,
            normal,
            pseudo,
            expand,
            polar,
        } => {
            let s = run()?;
            let mut o = Vec::new();
            let r = s.calculate_bounding_box(&k, &dom_of(*dom), [*normal, *pseudo, *polar], *expand);
            sub(&mut o, r, |o, b| {
                if let Some(n) = b.normal_box.filter(|_| *normal) {
                    push_box(o, &n);
                }
                if let Some(p) = b.pseudo_box.filter(|_| *pseudo) {
                    for i in 0..3 {
                        push3(o, p.basis_vector(i).unwrap_or(Vec3::ZERO));
                    }
                    for i in 0..3 {
                        let iv = p.interval(i).unwrap_or_default();
                        o.extend([iv.min(), iv.max()]);
                    }
                }
                if let Some(pl) = b.polar_box.as_ref().filter(|_| *polar) {
                    o.push(f64::from(pl.state().code()));
                    for v in pl.basis() {
                        push3(o, *v);
                    }
                    push_dom(o, &pl.domain());
                }
            })
            .map(|()| o)
        }
        OpIn::Sil {
            dom,
            eye,
            perspective,
            tol,
            ang,
            translate,
        } => {
            let mut s = surface_in(&i.surf)?;
            if let Some(t) = translate {
                let mut ax = Axis2Placement::default();
                ax.translate(v3(*t));
                match s.transform(&k, &ax, None) {
                    Ok(()) => {}
                    Err(e) if e.is_unported() => return Ok(Err(e)),
                    Err(e) => return Ok(Ok(vec![status(&e)])),
                }
            }
            match s.create_silhouette_curves(&k, &dom_of(*dom), v3(*eye), *perspective, *tol, *ang) {
                Ok((c3, cuv)) => {
                    let mut o = vec![0.0, c3.len() as f64, cuv.len() as f64];
                    for c in c3.iter().chain(&cuv) {
                        push_sil_curve(&mut o, c);
                    }
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status(&e)]),
            }
        }
        OpIn::DomCont {
            cont,
            uv,
            vec,
            history,
        } => {
            let s = run()?;
            let c = continuity(*cont)?;
            let mut k = Kernel::default();
            if *history != 0 && !matches!(s.kind(), SurfaceKind::Step(_)) {
                // the shim's cache history: the surface's cache, made while doing a
                // Boolean operation for 1 and 2 (no tree), its tree then built for 2
                k.context.set_doing_boolean(matches!(history, 1 | 2));
                let made = get_surface_cache::<()>(&k, &s, None);
                k.context.set_doing_boolean(false);
                let built = made.and_then(|mut cache| {
                    if *history == 2 {
                        cache.get_tree(&k)?;
                    }
                    Ok(())
                });
                match built {
                    Ok(()) => {}
                    Err(e) if e.is_unported() => return Ok(Err(e)),
                    Err(_) => return Ok(Ok(vec![f64::from(SM_ERR)])),
                }
            }
            k.context.set_doing_boolean(*history == 3);
            match s.domain_with_continuity(&k, c, p2(*uv), Vec2::new(vec[0], vec[1])) {
                Ok(d) => {
                    let mut o = vec![0.0];
                    push_dom(&mut o, &d);
                    Ok(o)
                }
                Err(e) if e.is_unported() => Err(e),
                Err(e) => Ok(vec![status(&e)]),
            }
        }
    })
}

/// A silhouette curve as the shim's `put_sil_curve`: its type code, then for a B-spline
/// `dim hasNurb [n p m cpts(x y z w) knots]`.
fn push_sil_curve(o: &mut Vec<f64>, c: &SmCurve) {
    o.push(c.type_code() as f64);
    let CurveKind::BSpline(b) = c.kind() else {
        return;
    };
    o.push(b.dim() as f64);
    o.push(flag(b.gw_nurb().is_some()));
    if let Some(cur) = b.gw_nurb() {
        o.extend([cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
        for p in cur.cpts() {
            o.extend([p.x, p.y, p.z, p.w]);
        }
        o.extend_from_slice(cur.knots());
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    Ok(match rust_op(i)? {
        Ok(o) => to_output(&o),
        Err(e) => Output::unported(&e).unwrap_or_else(|| to_output(&[status(&e)])),
    })
}

// ---- generated cases ----------------------------------------------------------------

/// A point of `d`: a corner, a point on a side, or inside.
fn gen_uv(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let pick = |rng: &mut Rng, a: f64, b: f64| -> f64 {
        match rng.below(6) {
            0 => a,
            1 => b,
            _ => a + (b - a) * rng.unit(),
        }
    };
    [pick(rng, d[0], d[2]), pick(rng, d[1], d[3])]
}

/// A point in `d` or, half the time, up to 20 % of the side outside it.
fn gen_uv_near(rng: &mut Rng, d: [f64; 4]) -> [f64; 2] {
    let [u, v] = gen_uv(rng, d);
    if rng.chance(0.5) {
        return [u, v];
    }
    let off = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        let w = (hi - lo) * 10f64.powf(rng.uniform(-6.0, -0.7));
        if rng.chance(0.5) {
            lo - w
        } else {
            hi + w
        }
    };
    match rng.below(3) {
        0 => [off(rng, d[0], d[2]), v],
        1 => [u, off(rng, d[1], d[3])],
        _ => [off(rng, d[0], d[2]), off(rng, d[1], d[3])],
    }
}

fn gen_opt_dom(rng: &mut Rng, d: [f64; 4]) -> Option<[f64; 4]> {
    rng.chance(0.6).then(|| gen_sub_dom(rng, d))
}

/// A tolerance relative to the surface size: zero, tiny, moderate or large.
fn gen_tol(rng: &mut Rng, size: f64) -> f64 {
    match rng.below(5) {
        0 => 1e-12,
        1 => size * 1e-7,
        2 => size * rng.uniform(1e-6, 1e-3),
        3 => size * rng.uniform(1e-3, 0.3),
        _ => 1e-5,
    }
}

/// A 2D curve through the domain `d` (and a bit beyond it).
fn gen_uv_curve(rng: &mut Rng, d: [f64; 4]) -> CrvIn {
    let p = 1 + rng.below(3) as usize;
    let n = p + rng.below(4) as usize;
    let (w, h) = (d[2] - d[0], d[3] - d[1]);
    let cpts = (0..=n)
        .map(|i| {
            let t = i as f64 / n as f64;
            [
                d[0] + w * (-0.3 + 1.6 * t + rng.uniform(-0.1, 0.1)),
                d[1] + h * rng.uniform(-0.3, 1.3),
                0.0,
                1.0,
            ]
        })
        .collect();
    CrvIn {
        dim: 2,
        cpts,
        p,
        u: gen_knots(rng, p, n),
    }
}

#[allow(clippy::too_many_lines)]
fn gen_case(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surf(rng);
    let wrap = rng.chance(0.3);
    let d = surf_domain(&surf)?;
    let size = surf_size(&surf)?;
    // B-splines that clamp outside their domain and planes evaluate anywhere
    let outside_ok = match &surf {
        SurfIn::Bspline(b) => !b.oob,
        SurfIn::Plane(_) => true,
        SurfIn::CurveBounded(_) => false,
    };
    // upstream's base CalculateBoundingBox recurses on a polar box (UB-84)
    let polar_ok = !wrap && !matches!(surf, SurfIn::CurveBounded(_));
    let op = match rng.below(46) {
        0..=5 => OpIn::Eval {
            uv: gen_uv(rng, d),
            ul: rng.chance(0.5),
            vl: rng.chance(0.5),
        },
        6..=7 => OpIn::Periodic {
            dom: if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) },
            v: rng.chance(0.5),
        },
        8..=9 => OpIn::DegenPoint {
            tol: gen_tol(rng, size),
            dom: gen_opt_dom(rng, d),
        },
        10..=11 => OpIn::DegenCurve {
            ang: match rng.below(3) {
                0 => 1e-3,
                1 => rng.uniform(0.01, 5.0),
                _ => rng.uniform(5.0, 60.0),
            },
            dom: gen_opt_dom(rng, d),
            tol: gen_tol(rng, size),
        },
        12..=13 => OpIn::Flat {
            uv: gen_uv(rng, d),
            ang: rng.uniform(0.01, 30.0),
        },
        14..=15 => {
            let dom = if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) };
            OpIn::OnBoundary {
                dom,
                uv: if outside_ok { gen_uv_near(rng, dom) } else { gen_uv(rng, dom) },
                tol: rng.chance(0.6).then(|| gen_tol(rng, size)),
            }
        }
        16..=17 => {
            let dom = if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) };
            OpIn::InDomain {
                dom,
                uv: if outside_ok { gen_uv_near(rng, d) } else { gen_uv(rng, d) },
                tol: rng.chance(0.6).then(|| gen_tol(rng, size)),
            }
        }
        18..=19 => OpIn::Misc {
            other_wrap: rng.chance(0.3),
            other: if rng.chance(0.3) {
                surf.clone()
            } else {
                gen_surf(rng)
            },
        },
        20..=21 => OpIn::Snap {
            uv: gen_uv(rng, d),
            tol: match rng.below(3) {
                0 => 0.0,
                1 => 1e-9,
                _ => rng.uniform(0.0, 0.2) * (d[2] - d[0]).max(d[3] - d[1]),
            },
        },
        22..=23 => OpIn::NSect {
            e: (0..9)
                .map(|k| if k == 0 || rng.chance(0.85) { gvec(rng, 2.0) } else { [0.0; 3] })
                .collect(),
            dir: gvec(rng, 1.0),
        },
        24..=25 => OpIn::ParamDir {
            uv: gen_uv(rng, d),
            vec: gvec(rng, 1.0),
        },
        26..=27 => OpIn::DirDerivs {
            uv: gen_uv(rng, d),
            dir: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
            h: rng.below(4) as usize,
            ul: rng.chance(0.5),
            vl: rng.chance(0.5),
        },
        28..=29 => OpIn::Convert {
            dom: gen_sub_dom(rng, d),
        },
        30..=31 => OpIn::Size {
            dom: gen_opt_dom(rng, d),
        },
        32..=33 => {
            let curve = gen_uv_curve(rng, d);
            let (a, b) = (
                curve.u.first().copied().unwrap_or(0.0),
                curve.u.last().copied().unwrap_or(1.0),
            );
            let (lo, hi) = if rng.chance(0.5) {
                (a, b)
            } else {
                let x = a + (b - a) * rng.uniform(0.0, 0.4);
                (x, x + (b - x) * rng.uniform(0.3, 1.0))
            };
            OpIn::FindIvl {
                curve,
                ivl: [lo, hi],
                param: lo + (hi - lo) * rng.unit(),
                dom: if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) },
            }
        }
        34..=35 => {
            let v = rng.chance(0.5);
            let (a, b) = if v { (d[1], d[3]) } else { (d[0], d[2]) };
            OpIn::Sing {
                uv: gen_uv(rng, d),
                dir: gvec(rng, 1.0),
                v,
                tol: gen_tol(rng, size),
                s: a + (b - a) * rng.unit(),
            }
        }
        36..=37 => OpIn::Seam {
            prev: gen_uv(rng, d),
            curr: gen_uv(rng, d),
            dom: if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) },
        },
        38..=39 => {
            let [u, t] = gen_uv(rng, d);
            OpIn::IsoLen { u, t }
        }
        40..=41 => {
            let v = rng.chance(0.5);
            let (a, b) = if v { (d[1], d[3]) } else { (d[0], d[2]) };
            OpIn::Step {
                v,
                ivl: rng.chance(0.5).then(|| {
                    // sometimes just inside a domain end (ContainsValue has no tolerance)
                    let near = |rng: &mut Rng, x: f64| x + (b - a) * 10f64.powf(rng.uniform(-14.0, -11.0));
                    match rng.below(5) {
                        0 => [near(rng, a), a + (b - a) * rng.uniform(0.2, 0.8)],
                        1 => [a + (b - a) * rng.uniform(0.2, 0.8), b - (near(rng, a) - a)],
                        _ => {
                            let x = a + (b - a) * rng.uniform(-0.2, 0.6);
                            [x, x + (b - a) * rng.uniform(0.0, 0.8)]
                        }
                    }
                }),
            }
        }
        42 => OpIn::Cache {
            trim: rng.chance(0.5),
        },
        43..=44 => {
            let pseudo = rng.chance(0.5);
            let polar = polar_ok && rng.chance(0.6);
            OpIn::BBox {
                dom: if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) },
                normal: (!pseudo && !polar) || rng.chance(0.5),
                pseudo,
                expand: rng.chance(0.5),
                polar,
            }
        }
        _ => OpIn::DomCont {
            cont: rng.below(14) as u8,
            uv: gen_uv(rng, d),
            vec: [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)],
            history: 0,
        },
    };
    Ok(Inputs { wrap, surf, op })
}

/// Generated `Sil` cases (their own seed stream, `surface.surface_p1:sil`).
pub const SIL_CASES: usize = 64;

/// A `Sil` case: a B-spline or plane over its domain or a sub-domain; a parallel view
/// (axis-aligned or random) or an eye point near or far from the surface; default, fine,
/// coarse or near-limit tolerances; sometimes translated.
fn gen_sil(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surf(rng);
    if matches!(surf, SurfIn::CurveBounded(_)) {
        return Err(harness("sil: a B-spline or a plane"));
    }
    let d = surf_domain(&surf)?;
    let size = surf_size(&surf)?;
    // tiny surfaces split their surface cache past MAX_SUBDIVISION_NODES on the Rust side
    // (an InvalidInput where upstream subdivides on; plan/findings.md, surface.surface_cache_p3)
    if size < 1e-2 {
        return Err(harness("sil: surface too small for the Rust subdivision bound"));
    }
    let dom = if rng.chance(0.5) { d } else { gen_sub_dom(rng, d) };
    let perspective = rng.chance(0.4);
    let eye = if perspective {
        let r = size * if rng.chance(0.7) { rng.uniform(2.0, 10.0) } else { rng.uniform(1.05, 1.5) };
        let v = gvec(rng, 1.0);
        let n = (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt().max(1e-3);
        [v[0] / n * r, v[1] / n * r, v[2] / n * r]
    } else {
        match rng.below(6) {
            0 => [1.0, 0.0, 0.0],
            1 => [0.0, 1.0, 0.0],
            2 => [0.0, 0.0, 1.0],
            3 => [1.0, 0.0, 1e-7],
            _ => gvec(rng, 1.0),
        }
    };
    let tol = match rng.below(5) {
        0 => None,
        1 => Some(size * 2e-6),
        2 => Some(size * rng.uniform(1e-4, 1e-2)),
        3 => Some(size * rng.uniform(0.05, 0.5)),
        _ => Some(1e-3),
    };
    let ang = match rng.below(5) {
        0 => None,
        1 => Some(20f64.to_radians()),
        2 => Some(rng.uniform(1e-3, 0.1)),
        3 => Some(core::f64::consts::PI),
        _ => Some(rng.uniform(0.1, 1.0)),
    };
    let translate = rng.chance(0.3).then(|| gvec(rng, size * 10.0));
    Ok(Inputs {
        wrap: false,
        surf,
        op: OpIn::Sil {
            dom,
            eye,
            perspective,
            tol,
            ang,
            translate,
        },
    })
}

/// Generated `DomCont` cases with a cache history (their own seed stream,
/// `surface.surface_p1:domhist`).
pub const DOM_HIST_CASES: usize = 96;

/// A `DomCont` case after a cache history (`1` to `3`, see [`OpIn::DomCont`]): half the
/// points in the domain, half off it (up to 90 % of a side out in u, v or both), where
/// the end-knot discontinuities clamp the domain only when the cache's continuity
/// arrays are filled.
fn gen_dom_hist(rng: &mut Rng) -> Result<Inputs, HarnessError> {
    let surf = gen_surf(rng);
    // tiny surfaces split their surface cache past MAX_SUBDIVISION_NODES on the Rust side
    // (an InvalidInput where upstream subdivides on; plan/findings.md, surface.surface_cache_p3)
    if surf_size(&surf)? < 1e-2 {
        return Err(harness("domhist: surface too small for the Rust subdivision bound"));
    }
    let wrap = rng.chance(0.1);
    let d = surf_domain(&surf)?;
    let [u, v] = gen_uv(rng, d);
    let out = |rng: &mut Rng, lo: f64, hi: f64| -> f64 {
        let w = (hi - lo) * rng.uniform(1e-3, 0.9);
        if rng.chance(0.5) {
            lo - w
        } else {
            hi + w
        }
    };
    let uv = match rng.below(6) {
        0..=2 => [u, v],
        3 => [out(rng, d[0], d[2]), v],
        4 => [u, out(rng, d[1], d[3])],
        _ => [out(rng, d[0], d[2]), out(rng, d[1], d[3])],
    };
    let component = |rng: &mut Rng| {
        if rng.chance(0.15) {
            0.0
        } else {
            rng.uniform(-1.0, 1.0)
        }
    };
    let vec = [component(rng), component(rng)];
    Ok(Inputs {
        wrap,
        surf,
        op: OpIn::DomCont {
            cont: rng.below(14) as u8,
            uv,
            vec,
            history: 1 + rng.below(3) as u8,
        },
    })
}

fn generate_dom_hist(rng: &mut Rng) -> Option<Value> {
    for _ in 0..64 {
        let Ok(i) = gen_dom_hist(rng) else {
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

fn generate_sil(rng: &mut Rng) -> Option<Value> {
    for _ in 0..64 {
        let Ok(i) = gen_sil(rng) else {
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

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_p1"
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
        let main = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &main)?;
        // CreateSilhouetteCurves cases: their own seed stream (`<unit>:sil`), so the other
        // ops' generated cases are unchanged; named `silNNNN`.
        let sil = CaseContext {
            seeds: ctx.seeds.max(SIL_CASES),
            ..ctx.clone()
        };
        for mut c in sil.generated_cases("surface.surface_p1:sil", generate_sil) {
            c.name = c.name.replacen("gen", "sil", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        // GetDomainWithContinuity after a cache history: their own seed stream
        // (`<unit>:domhist`); named `domhistNNNN`.
        let hist = CaseContext {
            seeds: ctx.seeds.max(DOM_HIST_CASES),
            ..ctx.clone()
        };
        for mut c in hist.generated_cases("surface.surface_p1:domhist", generate_dom_hist) {
            c.name = c.name.replacen("gen", "domhist", 1);
            c.unit = self.id().to_string();
            v.push(c);
        }
        Ok(v)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
