//! Runner for unit `surface.surface_approximate`: `SmSurface::ApproximateSurface` on real
//! B-spline surfaces, planes, cylinders, cones, spheres, tori and surfaces of revolution,
//! bare or inside up to three offset, curve bounded, STEP and volume (rigid transform)
//! surfaces, so every `GetRootSurface` override is reached.
//!
//! C++: `solidcore_ffi::units::surface_surface_approximate` (shim
//! `harness/cpp/shim/surface_surface_approximate.cpp`, which documents the block).
//! Rust: [`solidcore::surface::surface::Surface::approximate_surface_ignoring_status`].
//!
//! A case is the shim's input block (`tol level just_copy n_wraps wraps.. kind` and the
//! kind's construction data, as `surface.surface_assert_valid`'s); both sides build their
//! own surface from it. The output is the status, the achieved tolerance and, when a
//! surface comes back, its type and its `SmBSplineSurface` part (degrees, control point
//! counts, knots with multiplicities, every control point and weight). Generated cases
//! cover every kind with and without wraps (offsets at positive, negative, zero and
//! nested distances and past a radius of curvature; curve bounded, STEP and volume
//! surfaces, alone and around or inside offsets), copies of B-spline kinds
//! (`bJustCopyBSplines`), starters of degree above 3 (the single-patch starter), simple
//! knots (the patch index check), loose, tight and default tolerances, subdivision
//! levels 0 to 4, at huge and tiny scales. Fixtures: `fixtures/surface.surface_approximate/`
//! (issue #1441's offset quarter cylinder among them).

// The generator indexes the nets it has just built (as other runners do).
#![allow(clippy::indexing_slicing)]

use std::borrow::Cow;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::approx_tol3d;
use solidcore::geom::transform::Transform;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, ControlPointForm};
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::cone::Cone;
use solidcore::surface::curve_bounded_surface::CurveBoundedSurface;
use solidcore::surface::offset_surface::OffsetSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::srf_in_volume::SrfInVolume;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{StepSurface, Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore_ffi::units::surface_surface_approximate as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `SM_USE_DEFAULT`.
const USE_DEFAULT: f64 = 9_876_543.21;

/// `surface.surface_approximate` runner.
pub struct Runner;

/// The shim's input block.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    block: Vec<f64>,
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn output(flat: Vec<f64>) -> Output {
    Output::new().values("out", flat)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    // each case in a fresh child: a native crash fails that case, not the unit run
    match solidcore_ffi::isolate::fresh_process(|| ffi::eval(&i.block)) {
        Ok(v) => Ok(output(v)),
        Err(solidcore_ffi::FfiError::InvalidArgument(r)) => Ok(Output::rejected(r)),
        Err(e) => Err(herr(e)),
    }
}

// ---- Rust side ----------------------------------------------------------------------

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, HarnessError> {
        let x = *self.v.get(self.at).ok_or_else(|| herr("short block"))?;
        self.at += 1;
        Ok(x)
    }
    fn int(&mut self) -> Result<usize, HarnessError> {
        Ok(self.num()? as usize)
    }
    fn flag(&mut self) -> Result<bool, HarnessError> {
        Ok(self.num()? != 0.0)
    }
    fn vec(&mut self) -> Result<Vec3, HarnessError> {
        Ok(Vec3::new(self.num()?, self.num()?, self.num()?))
    }
    fn dom(&mut self) -> Result<Extent2d, HarnessError> {
        Ok(Extent2d::from_bounds(
            self.num()?,
            self.num()?,
            self.num()?,
            self.num()?,
        ))
    }
    fn point(&mut self, rational: bool) -> Result<CPoint, HarnessError> {
        let (x, y, z, w) = (self.num()?, self.num()?, self.num()?, self.num()?);
        Ok(CPoint::new(x, y, z, if rational { w } else { NOW }))
    }
}

/// The surface of `kind` (the builders of `surface.surface_assert_valid`'s runner).
fn build(k: &Kernel, rd: &mut Rd<'_>, kind: usize) -> Result<Surface, HarnessError> {
    if kind == 0 {
        let (p, q, nu, nv) = (rd.int()?, rd.int()?, rd.int()?, rd.int()?);
        let rational = rd.flag()?;
        let ku = (0..=nu + p).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
        let kv = (0..=nv + q).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
        let mut net = Vec::new();
        for _ in 0..nu {
            net.push((0..nv).map(|_| rd.point(rational)).collect::<Result<Vec<_>, _>>()?);
        }
        let nurb = NlSurface::new(net, p, q, ku, kv).map_err(herr)?;
        return Ok(Surface::from(BSplineSurface::from_nurb(nurb, false)));
    }
    let o = rd.vec()?;
    let x = rd.vec()?;
    let y = rd.vec()?;
    Ok(match kind {
        1 => {
            let s = Vec2::new(rd.num()?, rd.num()?);
            let d = rd.dom()?;
            Surface::from(Plane::new(o, x, y, s, &d))
        }
        2 => {
            let (r, s0, s1, h) = (rd.num()?, rd.num()?, rd.num()?, rd.num()?);
            let (swap, inside, mngc) = (rd.flag()?, rd.flag()?, rd.flag()?);
            Surface::from(Cone::new_cylinder_bounded(
                o, x, y, r, s0, s1, h, swap, inside, mngc,
            ))
        }
        3 => {
            let (bot, top, s0, s1, h) = (rd.num()?, rd.num()?, rd.num()?, rd.num()?, rd.num()?);
            let (swap, inside, mngc) = (rd.flag()?, rd.flag()?, rd.flag()?);
            Surface::from(Cone::new_bounded(
                o, x, y, bot, top, s0, s1, h, swap, inside, mngc,
            ))
        }
        4 => {
            let d = rd.dom()?;
            let r = rd.num()?;
            let (swap_uv, inside_out) = (rd.flag()?, rd.flag()?);
            let opts = SphereOptions {
                swap_uv,
                inside_out,
                circle_nurb: None,
            };
            Surface::from(Sphere::new(k, o, x, y, d, r, opts).map_err(herr)?)
        }
        5 => {
            let d = rd.dom()?;
            let (major, minor) = (rd.num()?, rd.num()?);
            let (swap_uv, inside_out) = (rd.flag()?, rd.flag()?);
            let opts = TorusOptions {
                swap_uv,
                inside_out,
                circle_nurb: None,
            };
            Surface::from(Torus::new(k, o, x, y, d, major, minor, opts).map_err(herr)?)
        }
        _ => {
            let d = rd.dom()?;
            let swap = rd.flag()?;
            let (p, n) = (rd.int()?, rd.int()?);
            let rational = rd.flag()?;
            let pts = (0..n).map(|_| rd.point(rational)).collect::<Result<Vec<_>, _>>()?;
            let knots = (0..=n + p).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?;
            let gen = Curve::from(BSplineCurve::from_nurb(
                NlCurve::new(pts, p, knots).map_err(herr)?,
                3,
                false,
            ));
            Surface::from(SurfOfRevolution::new(k, gen, o, x, y, d, swap).map_err(herr)?)
        }
    })
}

/// The `SmStatus` of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// The `SmBSplineSurface` part of a result (`SM_CAST_PTR` of the returned pointer: a
/// copy keeps its kind).
fn b_spline_part<'a>(
    k: &Kernel,
    s: &'a Surface,
) -> Result<Cow<'a, BSplineSurface>, SolidError> {
    Ok(match s.kind_with_nurb(k)? {
        SurfaceKind::BSpline(b) => Cow::Borrowed(b),
        SurfaceKind::Plane(p) => p.base_with_nurb()?,
        SurfaceKind::Cone(c) => Cow::Borrowed(c.rev().base()),
        SurfaceKind::Sphere(c) => Cow::Borrowed(c.base().base()),
        SurfaceKind::Torus(c) => Cow::Borrowed(c.base().base()),
        SurfaceKind::SurfOfRevolution(c) => Cow::Borrowed(c.base()),
        _ => return Err(SolidError::sm_err("difftest::surface_approximate::b_spline_part")),
    })
}

/// The shim's `ap_put` record of `s`.
fn put(v: &mut Vec<f64>, s: &BSplineSurface) {
    let nu = s.number_control_points(SurfParamType::U);
    let nv = s.number_control_points(SurfParamType::V);
    v.extend([
        s.degree(SurfParamType::U) as f64,
        s.degree(SurfParamType::V) as f64,
        nu as f64,
        nv as f64,
    ]);
    for dir in [SurfParamType::U, SurfParamType::V] {
        match s.knots(dir, None) {
            Ok((knots, mults)) => {
                v.push(knots.len() as f64);
                v.extend(knots);
                v.extend(mults.iter().map(|&m| m as f64));
            }
            Err(_) => v.push(-1.0),
        }
    }
    for i in 0..nu {
        for j in 0..nv {
            match s.control_point(ControlPointForm::EuclideanRational, i, j) {
                Ok((p, w)) => v.extend([p.x, p.y, p.z, w]),
                Err(_) => v.extend([0.0, 0.0, 0.0, -1.0]),
            }
        }
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut rd = Rd {
        v: &i.block,
        at: 0,
    };
    let tol = rd.num()?;
    let level = rd.int()?;
    let just_copy = rd.flag()?;
    let n_wrap = rd.int()?;
    let mut wraps = Vec::new();
    for _ in 0..n_wrap {
        let t = rd.int()?;
        let m = match t {
            0 | 1 => 1,
            3 => 12,
            _ => 0,
        };
        wraps.push((t, (0..m).map(|_| rd.num()).collect::<Result<Vec<_>, _>>()?));
    }
    let kind = rd.int()?;
    let mut surface = build(&k, &mut rd, kind)?;
    for (t, d) in wraps {
        let at = |i: usize| d.get(i).copied().ok_or_else(|| herr("short wrap"));
        surface = match t {
            0 => Surface::from(OffsetSurface::new(&k, at(0)?, surface, true, 0).map_err(herr)?),
            1 => Surface::from(
                CurveBoundedSurface::new(surface, Vec::new(), Vec::new(), at(0)? != 0.0)
                    .map_err(herr)?,
            ),
            2 => Surface::from(StepSurface::new(surface, true).map_err(herr)?),
            _ => {
                let v = |i: usize| -> Result<Vec3, HarnessError> {
                    Ok(Vec3::new(at(i)?, at(i + 1)?, at(i + 2)?))
                };
                let t = Transform::new(v(0)?, v(3)?, v(6)?, Some(v(9)?));
                let vol = Volume::with_maps(VolumeKind::Transform(t), None, None).map_err(herr)?;
                Surface::from(SrfInVolume::new(&k, surface, false, vol, 3, 0).map_err(herr)?)
            }
        };
    }
    let r = match surface.approximate_surface_ignoring_status(
        &k,
        approx_tol3d(&k, tol),
        level,
        just_copy,
    ) {
        Ok(r) => r,
        Err(e) => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
        }
    };
    let mut v = vec![
        r.failure.as_ref().map_or(0.0, status),
        r.achieved_tol,
        f64::from(u8::from(r.surface.is_some())),
    ];
    if let Some(s) = &r.surface {
        v.push(s.type_code() as f64);
        match b_spline_part(&k, s) {
            Ok(b) => put(&mut v, &b),
            Err(e) => {
                return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
            }
        }
    }
    Ok(output(v))
}

// ---- generation ---------------------------------------------------------------------

fn scale(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 10f64.powi(rng.range_i64(-6, -2) as i32),
        1 => 10f64.powi(rng.range_i64(2, 6) as i32),
        _ => rng.uniform(0.5, 4.0),
    }
}

/// A unit-ish frame: the identity, a rotation about z or y, or a slightly skewed one.
fn frame(rng: &mut Rng, b: &mut Vec<f64>, s: f64) {
    let o = [rng.uniform(-2.0, 2.0) * s, rng.uniform(-2.0, 2.0) * s, rng.uniform(-2.0, 2.0) * s];
    let (x, y) = match rng.below(4) {
        0 => ([1.0, 0.0, 0.0], [0.0, 1.0, 0.0]),
        1 => {
            let a = rng.uniform(0.0, 6.0);
            ([a.cos(), a.sin(), 0.0], [-a.sin(), a.cos(), 0.0])
        }
        2 => {
            let a = rng.uniform(0.0, 6.0);
            ([a.cos(), 0.0, a.sin()], [0.0, 1.0, 0.0])
        }
        _ => ([1.0, 0.0, 0.0], [rng.uniform(-0.3, 0.3), 1.0, rng.uniform(-0.2, 0.2)]),
    };
    b.extend(o);
    b.extend(x);
    b.extend(y);
}

fn flag(rng: &mut Rng, p: f64) -> f64 {
    f64::from(u8::from(rng.chance(p)))
}

fn angles(rng: &mut Rng) -> (f64, f64) {
    match rng.below(5) {
        0 => (0.0, 360.0),
        1 => (-360.0, 0.0),
        2 => {
            let a = rng.uniform(-300.0, 200.0);
            (a, a + rng.uniform(1.0, 160.0))
        }
        3 => (0.0, 90.0),
        _ => (0.0, rng.uniform(10.0, 359.0)),
    }
}

/// A clamped knot vector for `n` control points of degree `p`: simple interior knots,
/// or interior knots of full multiplicity `p` (piecewise Bezier).
fn clamped(rng: &mut Rng, n: usize, p: usize) -> Vec<f64> {
    let interior = n - p - 1;
    let mut mid: Vec<f64> = if interior > 0 && interior.is_multiple_of(p) && rng.chance(0.5) {
        let spans = interior / p;
        (0..spans)
            .flat_map(|i| std::iter::repeat_n((i + 1) as f64 / (spans + 1) as f64, p))
            .collect()
    } else {
        (0..interior)
            .map(|i| (i + 1) as f64 / (interior + 1) as f64)
            .collect()
    };
    mid.sort_by(f64::total_cmp);
    let mut k = vec![0.0; p + 1];
    k.extend(mid);
    k.extend(vec![1.0; p + 1]);
    k
}

fn gen_bspline(rng: &mut Rng, b: &mut Vec<f64>, s: f64) {
    let deg = |rng: &mut Rng| match rng.below(8) {
        0 => 4,
        1 => 5,
        _ => 1 + rng.below(3) as usize,
    };
    let p = deg(rng);
    let q = deg(rng);
    let nu = p + 1 + [0, p, 2 * p, 1, 2][rng.below(5) as usize];
    let nv = q + 1 + [0, q, 2 * q, 1, 2][rng.below(5) as usize];
    let rational = rng.chance(0.25);
    let ku = clamped(rng, nu, p);
    let kv = clamped(rng, nv, q);
    let bump = [0.0, 0.3, 1.0][rng.below(3) as usize];
    let pole = rng.chance(0.1);
    b.extend([p as f64, q as f64, nu as f64, nv as f64, f64::from(u8::from(rational))]);
    b.extend(ku);
    b.extend(kv);
    for i in 0..nu {
        for j in 0..nv {
            let u = i as f64 / (nu - 1) as f64;
            let v = j as f64 / (nv - 1) as f64;
            let w = if rational { rng.uniform(0.5, 2.0) } else { 1.0 };
            let pt = if pole && i == 0 {
                // a collapsed boundary row
                [s, 0.5 * s, 0.0]
            } else {
                [(u + 1.0) * s, (v + 0.5) * s, bump * rng.uniform(-1.0, 1.0) * s]
            };
            if rational {
                b.extend([pt[0] * w, pt[1] * w, pt[2] * w, w]);
            } else {
                b.extend([pt[0], pt[1], pt[2], 1.0]);
            }
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let kind = rng.below(7) as usize;
    let s = scale(rng);
    let tol = match rng.below(8) {
        0 => USE_DEFAULT,
        1 => 1e-6 * s,
        2 => 0.0,
        3 => -1e-3 * s, // fabs
        4 => 0.1 * s,
        _ => 1e-3 * s,
    };
    let level = match rng.below(6) {
        0 => 0,
        1 => 1,
        2 => 4,
        _ => 3,
    };
    let mut b = vec![tol, f64::from(level), flag(rng, 0.15)];
    let n_wrap = match rng.below(10) {
        0..=2 => 0,
        3 => 2,
        4 => 3,
        _ => 1,
    };
    b.push(f64::from(n_wrap));
    for _ in 0..n_wrap {
        match rng.below(10) {
            0 => b.extend([1.0, flag(rng, 0.5)]), // curve bounded
            1 => b.push(2.0),                     // STEP
            2 => {
                // in a volume: a rigid motion
                let a = rng.uniform(0.0, 6.0);
                let t = [rng.uniform(-1.0, 1.0) * s, rng.uniform(-1.0, 1.0) * s, 0.5 * s];
                b.push(3.0);
                b.extend(t);
                b.extend([a.cos(), a.sin(), 0.0, -a.sin(), a.cos(), 0.0, 0.0, 0.0, 1.0]);
            }
            _ => {
                b.push(0.0);
                b.push(match rng.below(8) {
                    0 => 0.0,
                    1 => -rng.uniform(0.05, 0.5) * s,
                    2 => rng.uniform(1.0, 3.0) * s, // past a radius of curvature
                    _ => rng.uniform(0.05, 0.5) * s,
                });
            }
        }
    }
    b.push(kind as f64);
    if kind == 0 {
        gen_bspline(rng, &mut b, s);
        return Inputs { block: b };
    }
    frame(rng, &mut b, s);
    match kind {
        1 => {
            let sx = if rng.chance(0.2) { -1.0 } else { rng.uniform(0.2, 3.0) };
            let sy = rng.uniform(0.2, 3.0);
            let u0 = rng.uniform(-2.0, 2.0) * s;
            let v0 = rng.uniform(-2.0, 2.0) * s;
            let du = rng.uniform(0.1, 3.0) * s;
            let dv = rng.uniform(0.1, 3.0) * s;
            b.extend([sx, sy, u0, v0, u0 + du, v0 + dv]);
        }
        2 => {
            let (a0, a1) = angles(rng);
            b.extend([s * rng.uniform(0.2, 2.0), a0, a1, s * rng.uniform(0.1, 3.0)]);
            b.extend([flag(rng, 0.3), flag(rng, 0.3), flag(rng, 0.3)]);
        }
        3 => {
            let (a0, a1) = angles(rng);
            let (bot, top) = match rng.below(4) {
                0 => (0.0, rng.uniform(0.5, 2.0)),
                1 => (rng.uniform(0.5, 2.0), 0.0),
                _ => (rng.uniform(0.2, 2.0), rng.uniform(0.2, 2.0)),
            };
            b.extend([bot * s, top * s, a0, a1, s * rng.uniform(0.1, 3.0)]);
            b.extend([flag(rng, 0.3), flag(rng, 0.3), flag(rng, 0.3)]);
        }
        4 => {
            let (a0, a1) = angles(rng);
            let (v0, v1) = match rng.below(4) {
                0 => (-90.0, 90.0),
                1 => (-90.0, rng.uniform(-80.0, 89.0)),
                2 => (rng.uniform(-89.0, 80.0), 90.0),
                _ => {
                    let v = rng.uniform(-85.0, 60.0);
                    (v, v + rng.uniform(1.0, 85.0 - v.max(-85.0)).min(89.0 - v))
                }
            };
            b.extend([a0, v0, a1, v1, s * rng.uniform(0.2, 3.0)]);
            b.extend([flag(rng, 0.3), flag(rng, 0.3)]);
        }
        5 => {
            let (a0, a1) = angles(rng);
            let (v0, v1) = angles(rng);
            let major = s * rng.uniform(0.5, 3.0);
            let minor = match rng.below(4) {
                0 => major * rng.uniform(1.1, 2.0), // spindle
                _ => major * rng.uniform(0.05, 0.9),
            };
            b.extend([a0, v0, a1, v1, major, minor]);
            b.extend([flag(rng, 0.3), flag(rng, 0.3)]);
        }
        _ => {
            let (a0, a1) = angles(rng);
            let p = 1 + rng.below(4) as usize;
            let n = p + 1 + rng.below(3) as usize;
            let interior = n - p - 1;
            let mut knots = vec![0.0; p + 1];
            knots.extend((0..interior).map(|i| (i + 1) as f64 / (interior + 1) as f64));
            knots.extend(vec![1.0; p + 1]);
            b.extend([a0, 0.0, a1, 1.0, flag(rng, 0.3)]);
            let rational = rng.chance(0.2);
            b.extend([p as f64, n as f64, f64::from(u8::from(rational))]);
            for i in 0..n {
                let t = i as f64 / (n - 1) as f64;
                let (r, z) = (s * rng.uniform(0.3, 2.0), s * (t + rng.uniform(0.0, 0.1)));
                let w = if rational { rng.uniform(0.5, 2.0) } else { 1.0 };
                b.extend([r * w, 0.0, z * w, w]);
            }
            b.extend(knots);
        }
    }
    Inputs { block: b }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "surface.surface_approximate"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_case(rng)).ok()
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
