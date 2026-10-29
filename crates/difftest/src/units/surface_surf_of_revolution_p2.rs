//! Runner for unit `surface.surf_of_revolution_p2`: `SmSurfOfRevolution` part 2
//! (`AdjustSTEPUVDomain`, `STEPInversion`, `CreateOffsetSurface`,
//! `GlobalSurfaceIntersect`, `TrimWithDomain` and the static
//! `IsNurbSurfaceSurfOfRevolution`; `CreateCanonical` and `MakeNurb` are built by the
//! surface block of unit `surface.surf_of_revolution_p1`, which co-ports them).
//!
//! C++: `solidcore_ffi::units::surface_surf_of_revolution_p2` (upstream
//! `source/SMLib/src/SmSurfOfRevolution.cpp`). Rust:
//! `solidcore::surface::surf_of_revolution::p2`.
//!
//! A case is `surf` (the surface block of the `p1` runner) plus `op` and its
//! parameters, or the static `is_nurb` op on a B-spline (or revolution) surface. Both
//! sides produce the same flat record (layout in the FFI module docs), split into
//! `build` (the construction status), `other_build` (the other surface's, for
//! `gssi`), `status` and `out`. Statuses are `0` (`SM_SUCCESS`) or the `SmStatus`; `-1`
//! on the Rust side for an error upstream cannot produce.
//!
//! Deferred cases: `STEPInversion` of a non-planar generator (`SmBSplineSurface::
//! STEPInversion` needs `SmSurface::LocalPointSolve` / `GlobalPointSolve`,
//! `surface.surface_p5` / `surface.surface_p1`), `GlobalSurfaceIntersect` where the
//! plane intersection (`surface.surface_p1`, `surface.plane_p2`) or the general
//! intersector (`SmAdvSurfaceIntersector`, `intersect.adv_surface_intersector`) is
//! needed, and the conversions without exact polar conversion (`surface.surface_p5`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore_ffi::units::surface_surf_of_revolution_p2 as ffi;

use super::surface_surf_of_revolution_p1::{
    add, cross3, ffi_surf, flag, frame, gen_generator, gen_surf, normalize, put_curve, put_state,
    rust_surf, unit_vec, v3, GenIn, InlineCrv, SurfIn,
};
use super::surface_surf_of_revolution_p3::{crossing_surf, other_surf, scale_surf, sub_dom};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 900;

/// `surface.surf_of_revolution_p2` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// The `SmStatus` of a Rust error; `1007` (`SM_ERR_INVALID_INPUT`) for `InvalidInput`
/// (where upstream returns it), `-1` for one upstream cannot return.
fn status(e: &solidcore::SolidError) -> f64 {
    match e {
        solidcore::SolidError::Upstream { code, .. } => f64::from(*code),
        solidcore::SolidError::InvalidInput { .. } | solidcore::SolidError::PortInvariant { .. } => 1007.0,
        _ => -1.0,
    }
}

// ---- inputs -----------------------------------------------------------------------

/// An inline B-spline surface: control net as stored (rows in u), degrees, knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct InlineSrf {
    net: Vec<Vec<[f64; 4]>>,
    p: usize,
    q: usize,
    u: Vec<f64>,
    v: Vec<f64>,
}

/// The test surface of `is_nurb`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum TestIn {
    BSpline(InlineSrf),
    Revolution { surf: Box<SurfIn> },
}

/// The other operand of `gssi`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum OtherIn {
    /// Another surface of revolution; `None` is the same surface.
    Revolution {
        #[serde(default)]
        surf: Option<Box<SurfIn>>,
    },
    Plane {
        frame: [[f64; 3]; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    Build {},
    Adjust {
        dom: [f64; 4],
    },
    StepInv {
        /// `None`: the surface's STEP domain.
        #[serde(default)]
        dom: Option<[f64; 4]>,
        p: [f64; 3],
        tol: f64,
        #[serde(default)]
        guess: Option<[f64; 2]>,
    },
    Offset {
        dist: f64,
        tol: f64,
    },
    Gssi {
        other: OtherIn,
        #[serde(default)]
        dom: Option<[f64; 4]>,
        #[serde(default)]
        other_dom: Option<[f64; 4]>,
        #[serde(default)]
        tol: Option<f64>,
    },
    Trim {
        /// `None`: the natural domain.
        #[serde(default)]
        dom: Option<[f64; 4]>,
    },
    IsNurb {
        test: TestIn,
        scale: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    surf: Option<SurfIn>,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ side -------------------------------------------------------------------------

fn ffi_dom(d: Option<[f64; 4]>) -> ffi::Dom {
    d.map_or(ffi::Dom::Natural, ffi::Dom::Explicit)
}

fn ffi_data(s: &InlineSrf) -> solidcore_ffi::units::surface_surf_of_revolution_p1::SurfaceData {
    solidcore_ffi::units::surface_surf_of_revolution_p1::SurfaceData {
        net: s.net.clone(),
        p: s.p,
        q: s.q,
        u: s.u.clone(),
        v: s.v.clone(),
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let flat = if let OpIn::IsNurb { test, scale } = &i.op {
        let t = match test {
            TestIn::BSpline(s) => ffi::Test::BSpline(ffi_data(s)),
            TestIn::Revolution { surf } => ffi::Test::Revolution(Box::new(ffi_surf(surf)?)),
        };
        ffi::is_nurb_surface_surf_of_revolution(&t, *scale)
    } else {
        let s = i.surf.as_ref().ok_or_else(|| harness("missing surf"))?;
        let op = match &i.op {
            OpIn::Build {} => ffi::Op::Build,
            OpIn::Adjust { dom } => ffi::Op::Adjust { dom: *dom },
            OpIn::StepInv { dom, p, tol, guess } => ffi::Op::StepInv {
                dom: ffi_dom(*dom),
                point: *p,
                tol: *tol,
                guess: *guess,
            },
            OpIn::Offset { dist, tol } => ffi::Op::Offset {
                dist: *dist,
                tol: *tol,
            },
            OpIn::Gssi {
                other,
                dom,
                other_dom,
                tol,
            } => ffi::Op::Gssi {
                other: match other {
                    OtherIn::Revolution { surf } => ffi::Other::Revolution(match surf {
                        Some(o) => Some(Box::new(ffi_surf(o)?)),
                        None => None,
                    }),
                    OtherIn::Plane { frame } => ffi::Other::Plane { frame: *frame },
                },
                dom: ffi_dom(*dom),
                other_dom: ffi_dom(*other_dom),
                tol: *tol,
            },
            OpIn::Trim { dom } => ffi::Op::Trim {
                dom: ffi_dom(*dom),
            },
            OpIn::IsNurb { .. } => return Err(harness("unreachable")),
        };
        ffi::surface_op(&ffi_surf(s)?, &op)
    }
    .map_err(|e| harness(e.to_string()))?;
    to_output(i, &flat)
}

/// Whether the record starts with the construction status of a surface block.
fn has_build(op: &OpIn) -> bool {
    match op {
        OpIn::IsNurb { test, .. } => matches!(test, TestIn::Revolution { .. }),
        _ => true,
    }
}

/// Splits a record into `build`, `other_build`, `status` and `out`.
fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut rest = flat;
    if has_build(&i.op) {
        let (b, r) = rest
            .split_first()
            .ok_or_else(|| harness("empty record"))?;
        out = out.values("build", vec![*b]);
        rest = r;
        if *b != 0.0 {
            if !rest.is_empty() {
                return Err(harness("record continues after a failed build"));
            }
            return Ok(out);
        }
    }
    if matches!(i.op, OpIn::Gssi { .. }) {
        let (b, r) = rest
            .split_first()
            .ok_or_else(|| harness("record has no other build status"))?;
        out = out.values("other_build", vec![*b]);
        rest = r;
        if *b != 0.0 {
            if !rest.is_empty() {
                return Err(harness("record continues after a failed other build"));
            }
            return Ok(out);
        }
    }
    if matches!(i.op, OpIn::Build {}) {
        return Ok(out.values("out", rest.to_vec()));
    }
    let (s, r) = rest
        .split_first()
        .ok_or_else(|| harness("record has no op status"))?;
    out = out.values("status", vec![*s]);
    Ok(out.values("out", r.to_vec()))
}

// ---- Rust side ------------------------------------------------------------------------

fn dom_of(d: Option<[f64; 4]>, natural: Extent2d) -> Extent2d {
    d.map_or(natural, |[a, b, c, e]| Extent2d::from_bounds(a, b, c, e))
}

/// Pushes `0` and the state of `s`.
fn ok_state(k: &Kernel, o: &mut Vec<f64>, s: &SurfOfRevolution) -> solidcore::Result<()> {
    o.push(0.0);
    put_state(k, o, s)
}

fn nurb_surface(s: &InlineSrf) -> Result<BSplineSurface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|r| r.iter().map(|&[x, y, z, w]| CPoint::new(x, y, z, w)).collect())
        .collect();
    let sur = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface input: {e}")))?;
    Ok(BSplineSurface::from_nurb(sur, false))
}

/// Runs the op on the Rust surface, appending its status and outputs (for `gssi`, the
/// other surface's construction status first).
fn rust_op(
    k: &Kernel,
    s: &SurfOfRevolution,
    op: &OpIn,
    o: &mut Vec<f64>,
) -> Result<solidcore::Result<()>, HarnessError> {
    let r: solidcore::Result<()> = match op {
        OpIn::Build {} => put_state(k, o, s),
        OpIn::Adjust { dom } => {
            let mut c = s.clone();
            let [a, b, cc, d] = *dom;
            c.adjust_step_uv_domain(k, &Extent2d::from_bounds(a, b, cc, d))
                .and_then(|()| ok_state(k, o, &c))
        }
        OpIn::StepInv { dom, p, tol, guess } => {
            let d = dom_of(*dom, s.step_uv_domain());
            s.step_inversion(k, &d, v3(*p), *tol, guess.map(|[x, y]| Point2::new(x, y)))
                .map(|(uv, loc)| o.extend([0.0, uv.x, uv.y, f64::from(loc as i32)]))
        }
        OpIn::Offset { dist, tol } => s
            .create_offset_surface(k, *dist, *tol)
            .and_then(|off| ok_state(k, o, &off)),
        OpIn::Gssi {
            other,
            dom,
            other_dom,
            tol,
        } => {
            let me = Surface::from(s.clone());
            let other_owned = match other {
                OtherIn::Revolution { surf: None } => None,
                OtherIn::Revolution { surf: Some(os) } => match rust_surf(k, os)? {
                    Err(e) if e.is_unported() => return Ok(Err(e)),
                    Err(e) => {
                        o.push(status(&e));
                        return Ok(Ok(()));
                    }
                    Ok(b) => Some(Surface::from(b)),
                },
                OtherIn::Plane { frame } => {
                    let [origin, x, y] = *frame;
                    let f = Axis2Placement::new(v3(origin), v3(x), v3(y));
                    match Plane::create_canonical(&f) {
                        Err(e) if e.is_unported() => return Ok(Err(e)),
                        Err(e) => {
                            o.push(status(&e));
                            return Ok(Ok(()));
                        }
                        Ok(p) => Some(Surface::from(p)),
                    }
                }
            };
            // the same surface (by address) when no other is given
            let other_surface = other_owned.as_ref().unwrap_or(&me);
            o.push(0.0);
            me.global_surface_intersect(
                k,
                &dom_of(*dom, s.base().natural_uv_domain()),
                other_surface,
                &dom_of(*other_dom, other_surface.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?),
                [true, true],
                *tol,
                None,
            )
            .map(|x| {
                o.extend([0.0, 0.0, x.curves_3d.len() as f64]);
                for c in &x.curves_3d {
                    put_curve(o, Some(c));
                }
                o.push(x.curve_types.len() as f64);
                o.extend(x.curve_types.iter().map(|t| f64::from(*t as u8)));
                o.push(x.deviations.len() as f64);
                o.extend_from_slice(&x.deviations);
                let null1 = x.surface1_uv_curves.iter().filter(|c| c.is_none()).count();
                let null2 = x.surface2_uv_curves.iter().filter(|c| c.is_none()).count();
                o.extend([
                    x.surface1_uv_curves.len() as f64,
                    x.surface2_uv_curves.len() as f64,
                    null1 as f64,
                    null2 as f64,
                ]);
            })
        }
        OpIn::Trim { dom } => {
            let mut c = s.clone();
            let d = dom_of(*dom, c.base().natural_uv_domain());
            c.trim_with_domain(k, &d).and_then(|()| ok_state(k, o, &c))
        }
        OpIn::IsNurb { .. } => return Err(harness("unreachable")),
    };
    Ok(r)
}

fn rust_is_nurb(
    k: &Kernel,
    test: &TestIn,
    scale: f64,
    o: &mut Vec<f64>,
) -> Result<solidcore::Result<()>, HarnessError> {
    let surface = match test {
        TestIn::BSpline(s) => Surface::from(nurb_surface(s)?),
        TestIn::Revolution { surf } => match rust_surf(k, surf)? {
            Err(e) => {
                if !e.is_unported() {
                    o.push(status(&e));
                    return Ok(Ok(()));
                }
                return Ok(Err(e));
            }
            Ok(b) => {
                o.push(0.0);
                Surface::from(b)
            }
        },
    };
    Ok(
        SurfOfRevolution::is_nurb_surface_surf_of_revolution(k, &surface, scale).and_then(
            |found| {
                o.push(flag(found.is_some()));
                match &found {
                    Some(r) => put_state(k, o, r),
                    None => Ok(()),
                }
            },
        ),
    )
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut o = Vec::new();
    if let OpIn::IsNurb { test, scale } = &i.op {
        let at = o.len();
        if let Err(e) = rust_is_nurb(&k, test, *scale, &mut o)? {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.truncate(at + usize::from(has_build(&i.op)));
            o.push(status(&e));
        }
        return to_output(i, &o);
    }
    let s = i.surf.as_ref().ok_or_else(|| harness("missing surf"))?;
    match rust_surf(&k, s)? {
        Err(e) => {
            if let Some(out) = Output::unported(&e) {
                return Ok(out);
            }
            o.push(status(&e));
        }
        Ok(surf) => {
            o.push(0.0);
            let at = o.len();
            if let Err(e) = rust_op(&k, &surf, &i.op, &mut o)? {
                if let Some(out) = Output::unported(&e) {
                    return Ok(out);
                }
                // keep the other surface's construction status of an intersection
                let keep = if matches!(i.op, OpIn::Gssi { .. }) {
                    at + 1
                } else {
                    at
                };
                o.truncate(keep);
                o.push(status(&e));
            }
        }
    }
    to_output(i, &o)
}

// ---- generation -----------------------------------------------------------------------

/// A clamped knot vector for `n + 1` control points of degree `p` on `[a, b]`.
fn knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let inner = n - p;
    let mut ts: Vec<f64> = (0..inner)
        .map(|i| (i as f64 + 1.0) / (inner as f64 + 1.0))
        .collect();
    if rng.chance(0.4) {
        for t in &mut ts {
            *t += rng.uniform(-0.2, 0.2) / (inner as f64 + 1.0);
        }
    }
    let mut u = vec![a; p + 1];
    u.extend(ts.iter().map(|t| a + t * (b - a)));
    u.extend(vec![b; p + 1]);
    u
}

/// The NURB of a revolved generator, or a random patch.
fn gen_test(rng: &mut Rng) -> Option<TestIn> {
    let srf = if rng.chance(0.8) {
        let f = frame(rng);
        let planar = rng.chance(0.85);
        let GenIn::BSpline(c) = gen_generator(rng, &f, planar) else {
            return None;
        };
        let InlineCrv {
            cpts,
            weights,
            p,
            u,
        } = c;
        let pw = match &weights {
            None => cpts.iter().map(|&[x, y, z]| CPoint::new(x, y, z, NOW)).collect(),
            Some(w) => cpts
                .iter()
                .zip(w)
                .map(|(&[x, y, z], &w)| CPoint::new(x * w, y * w, z * w, w))
                .collect(),
        };
        let cur = NurbCurve::new(pw, p, u).ok()?;
        let gen = BSplineCurve::from_nurb(cur, 3, false);
        let (o, _, _, z) = f;
        let ang = if rng.chance(0.5) { 360.0 } else { rng.uniform(20.0, 359.0) };
        let s = BSplineSurface::create_surf_of_revolution(
            &Kernel::default(),
            &gen,
            v3(o),
            v3(z),
            ang,
            rng.chance(0.4),
            None,
        )
        .ok()?;
        let sur = s.gw_nurb()?;
        let ix = sur.indices();
        InlineSrf {
            net: sur
                .rows()
                .map(|r| r.iter().map(|p| [p.x, p.y, p.z, p.w]).collect())
                .collect(),
            p: ix.p,
            q: ix.q,
            u: sur.u_knots().to_vec(),
            v: sur.v_knots().to_vec(),
        }
    } else {
        let p = 1 + rng.below(3) as usize;
        let q = 1 + rng.below(3) as usize;
        let n = p + rng.below(3) as usize;
        let m = q + rng.below(3) as usize;
        let net = (0..=n)
            .map(|i| {
                (0..=m)
                    .map(|j| {
                        let [dx, dy, dz] = rng.point3(-0.3, 0.3);
                        [i as f64 + dx, j as f64 + dy, dz, NOW]
                    })
                    .collect()
            })
            .collect();
        InlineSrf {
            net,
            p,
            q,
            u: knots(rng, n, p, 0.0, 1.0),
            v: knots(rng, m, q, 0.0, 1.0),
        }
    };
    Some(TestIn::BSpline(srf))
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let op_kind = rng.below(20);
    if (17..=19).contains(&op_kind) {
        let test = if rng.chance(0.2) {
            TestIn::Revolution {
                surf: Box::new(gen_surf(rng)),
            }
        } else {
            gen_test(rng)?
        };
        return Some(Inputs {
            surf: None,
            op: OpIn::IsNurb {
                test,
                scale: *rng.choose(&[1.0, 1.0, 1.0, 10.0, 0.1, 1000.0])?,
            },
        });
    }
    // most ops mostly get planar generators (the others only test the early exits)
    let want_planar = op_kind >= 4 && rng.chance(0.85);
    let mut s = gen_surf(rng);
    for _ in 0..6 {
        let planar = rust_surf(&k, &s)
            .ok()
            .and_then(Result::ok)
            .is_some_and(|b| b.planar_generator());
        if planar || !want_planar {
            break;
        }
        s = gen_surf(rng);
    }
    if rng.chance(0.1) {
        let f = if rng.chance(0.5) { 1e-3 } else { 1e3 };
        scale_surf(&mut s, f);
    }
    // the Rust surface sizes the parameters (both sides get the same numbers)
    let built = rust_surf(&k, &s).ok()?.ok();
    let nd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0), |b| {
            b.base().natural_uv_domain()
        });
    let sd = built
        .as_ref()
        .map_or(Extent2d::from_bounds(0.0, 0.0, 360.0, 1.0), |b| {
            b.step_uv_domain()
        });
    let size = built
        .as_ref()
        .and_then(|b| b.evaluate_step_point(&k, sd.evaluate(0.5, 0.5)).ok())
        .map_or(1.0, |p| 1.0 + p.length());
    let op = match op_kind {
        0 => OpIn::Build {},
        1..=3 => {
            // a new STEP domain inside (or equal to) the current one
            let same = rng.chance(0.15);
            let (a, b) = if same {
                (sd.evaluate(0.0, 0.0), sd.evaluate(1.0, 1.0))
            } else {
                (
                    sd.evaluate(rng.uniform(0.0, 0.4), rng.uniform(0.0, 0.4)),
                    sd.evaluate(rng.uniform(0.6, 1.0), rng.uniform(0.6, 1.0)),
                )
            };
            OpIn::Adjust {
                dom: [a.x, a.y, b.x, b.y],
            }
        }
        4..=7 => {
            let b = built.as_ref()?;
            let p = match rng.below(5) {
                // on (or very near) the surface
                0..=2 => {
                    let uv = Point2::new(
                        sd.min().x + rng.unit() * (sd.max().x - sd.min().x),
                        sd.min().y + rng.unit() * (sd.max().y - sd.min().y),
                    );
                    let p = b.evaluate_step_point(&k, uv).ok()?;
                    let d = rng.point3(-1.0, 1.0);
                    let e = if rng.chance(0.6) { 0.0 } else { 1e-9 * size };
                    [p.x + e * d[0], p.y + e * d[1], p.z + e * d[2]]
                }
                // on the axis
                3 => {
                    let o = b.position().origin();
                    let z = b.position().z_axis();
                    let t = rng.uniform(-2.0, 2.0) * size;
                    [o.x + t * z.x, o.y + t * z.y, o.z + t * z.z]
                }
                _ => {
                    let p = rng.point3(-5.0, 5.0);
                    [p[0] * size, p[1] * size, p[2] * size]
                }
            };
            let guess = rng.chance(0.3).then(|| {
                let g = sd.evaluate(rng.unit(), rng.unit());
                [g.x, g.y]
            });
            OpIn::StepInv {
                dom: sub_dom(rng, &sd),
                p,
                tol: *rng.choose(&[1e-9, 1e-7, 1e-5, 1e-3])? * size,
                guess,
            }
        }
        8..=10 => OpIn::Offset {
            dist: rng.uniform(-0.5, 0.5) * size,
            tol: *rng.choose(&[1e-6, 1e-5, 1e-4])? * size,
        },
        11..=13 => {
            let b = built.as_ref()?;
            let other = match rng.below(10) {
                0..=2 => OtherIn::Revolution {
                    surf: Some(Box::new(other_surf(rng, b, true))),
                },
                3..=4 => OtherIn::Revolution {
                    surf: Some(Box::new(crossing_surf(rng, b, &k)?)),
                },
                5 => OtherIn::Revolution {
                    surf: Some(Box::new(other_surf(rng, b, false))),
                },
                6 => OtherIn::Revolution { surf: None },
                _ => {
                    let o = b.position().origin();
                    let z = b.position().z_axis();
                    let arr = |v: solidcore::geom::vector3d::Vec3| [v.x, v.y, v.z];
                    let t = unit_vec(rng);
                    let (x, y) = match rng.below(3) {
                        0 => (arr(z), normalize(cross3(arr(z), t))),
                        1 => {
                            let x = normalize(cross3(arr(z), t));
                            (x, cross3(arr(z), x))
                        }
                        _ => (unit_vec(rng), normalize(cross3(t, unit_vec(rng)))),
                    };
                    OtherIn::Plane {
                        frame: [add(arr(o), arr(z), rng.uniform(-1.0, 1.0)), x, y],
                    }
                }
            };
            let other_dom = match &other {
                OtherIn::Revolution { surf: Some(o) } => rust_surf(&k, o)
                    .ok()
                    .and_then(Result::ok)
                    .and_then(|os| sub_dom(rng, &os.base().natural_uv_domain())),
                _ => None,
            };
            OpIn::Gssi {
                other,
                dom: sub_dom(rng, &nd),
                other_dom,
                tol: if rng.chance(0.15) {
                    None
                } else {
                    Some(*rng.choose(&[1e-7, 1e-6, 1e-4, 1e-3])? * size)
                },
            }
        }
        _ => {
            // 14..=16: a trim to a sub-domain (or the whole domain)
            let dom = if rng.chance(0.15) { None } else { sub_dom(rng, &nd) };
            OpIn::Trim { dom }
        }
    };
    Some(Inputs { surf: Some(s), op })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surf_of_revolution_p2"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..8 {
            if let Some(i) = gen_case(rng) {
                return serde_json::to_value(i).ok();
            }
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
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn op_name(op: &OpIn) -> &'static str {
        match op {
            OpIn::Build {} => "build",
            OpIn::Adjust { .. } => "adjust",
            OpIn::StepInv { .. } => "step_inv",
            OpIn::Offset { .. } => "offset",
            OpIn::Gssi { .. } => "gssi",
            OpIn::Trim { .. } => "trim",
            OpIn::IsNurb { .. } => "is_nurb",
        }
    }

    /// The generators reach a successful run of every op on the Rust side.
    #[test]
    fn generated_cases_cover_every_op() {
        let mut tally: BTreeMap<(String, String), usize> = BTreeMap::new();
        for seed in 0..900u64 {
            let mut rng = Rng::new(0x5eed_2000 + seed);
            let Some(i) = gen_case(&mut rng) else {
                continue;
            };
            let out = rust(&i).expect("harness");
            let kind = if let Some(d) = &out.deferred {
                format!("deferred:{}", d.symbol)
            } else {
                let st = out.values.get("status").and_then(|v| v.first()).copied();
                format!("status:{st:?}")
            };
            *tally.entry((op_name(&i.op).to_string(), kind)).or_default() += 1;
        }
        for (k, v) in &tally {
            eprintln!("{k:?} {v}");
        }
        for op in ["build", "adjust", "step_inv", "offset", "gssi", "trim", "is_nurb"] {
            let ok = tally
                .iter()
                .any(|((o, k), _)| o == op && (k.starts_with("status:Some(0.0)") || op == "build"));
            assert!(ok, "{op} never succeeds");
        }
    }
}
