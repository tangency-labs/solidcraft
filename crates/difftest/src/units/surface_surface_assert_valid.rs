//! Runner for unit `surface.surface_assert_valid`: the virtual `SmSurface::AssertValid`
//! on real B-spline surfaces, planes, cylinders, cones, spheres, tori and surfaces of
//! revolution.
//!
//! C++: `solidcore_ffi::units::surface_surface_assert_valid` (shim
//! `harness/cpp/shim/surface_surface_assert_valid.cpp`, which documents the block).
//! Rust: [`solidcore::surface::surface::Surface::assert_valid`].
//!
//! A case is the shim's input block (`kind level make` and the kind's construction
//! data); both sides build their own surface from it. The output is the check's result
//! and every report (all fields, texts included). Generated cases cover valid surfaces
//! of every kind at levels 0, 1, 2 and `SM_LEVEL_GIVEN`, and corrupted ones: knot
//! multiplicities above the degree, internal poles, degenerate control rows, collapsed
//! and nearly collapsed boundaries, folded (non-monotonic) nets, flat corners,
//! coincident boundaries, degenerate surfaces, rational weights, negative and zero
//! scales and radii, apexes inside cone domains, spindle tori, mismatched and
//! degenerate generators, at huge and tiny scales.

// The generator indexes the nets and knot vectors it has just built (as other runners do).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::geom::assert_array::{AssertArray, ObjectKey};
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::crv_basic::Curve as NlCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::{Sphere, SphereOptions};
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::surface::torus::{Torus, TorusOptions};
use solidcore_ffi::units::surface_surface_assert_valid as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `surface.surface_assert_valid` runner.
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
    match ffi::eval(&i.block) {
        Ok(v) => Ok(output(v)),
        Err(solidcore_ffi::FfiError::InvalidArgument(r)) => Ok(Output::rejected(r)),
        Err(solidcore_ffi::FfiError::Status(solidcore_ffi::SC_ERR_ARG)) => Ok(Output::rejected(
            "CreateIsoBoundaries fails: sm_AssertTestSurface6 reads uninitialised curves",
        )),
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

fn text(out: &mut Vec<f64>, s: Option<&str>) {
    out.push(s.map_or(0, str::len) as f64);
    if let Some(s) = s {
        out.extend(s.bytes().map(f64::from));
    }
}

fn id(k: Option<ObjectKey>) -> f64 {
    match k {
        None => 0.0,
        Some(ObjectKey(1)) => 1.0,
        Some(_) => 9.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let mut rd = Rd {
        v: &i.block,
        at: 0,
    };
    let kind = rd.int()?;
    let level = rd.int()?;
    let make = rd.flag()?;
    let surface = build(&k, &mut rd, kind)?;
    if make {
        // MakeNurb (status ignored)
        match surface.kind() {
            SurfaceKind::Plane(p) => {
                let _ = p.base_with_nurb();
            }
            SurfaceKind::BSpline(_) => {}
            _ => {
                if let Err(e) = surface.kind_with_nurb(&k) {
                    if let Some(o) = Output::unported(&e) {
                        return Ok(o);
                    }
                }
            }
        }
    }
    let mut reports = AssertArray::new();
    let level = u8::try_from(level).map_err(herr)?;
    let ok = match surface.assert_valid(&k, Some(&mut reports), level, ObjectKey(1)) {
        Ok(ok) => ok,
        Err(e) => {
            return Ok(Output::unported(&e).unwrap_or_else(|| Output::err(e.to_string())));
        }
    };
    let mut v = vec![f64::from(u8::from(ok)), reports.len() as f64];
    for r in reports.reports() {
        v.extend([
            r.reporting_type as f64,
            r.list_index as f64,
            r.test_index as f64,
            f64::from(r.assert_type.code()),
            f64::from(u8::from(r.ok)),
            id(r.reporter),
            id(r.owner),
            id(r.other),
            r.owner_type as f64,
            r.other_type as f64,
            r.tol3d,
            r.value,
            r.dist3d,
        ]);
        text(&mut v, Some(r.name));
        text(&mut v, r.message.as_deref());
        text(&mut v, r.message_for_file);
        text(&mut v, r.owner_type_string);
        text(&mut v, r.other_type_string);
    }
    v.push(f64::from(u8::from(reports.last_add())));
    Ok(output(v))
}

// ---- generation ---------------------------------------------------------------------

fn level(rng: &mut Rng) -> f64 {
    [0.0, 1.0, 2.0, 2.0, 4.0][rng.below(5) as usize]
}

fn scale(rng: &mut Rng) -> f64 {
    match rng.below(8) {
        0 => 10f64.powi(rng.range_i64(-6, -2) as i32),
        1 => 10f64.powi(rng.range_i64(2, 6) as i32),
        _ => rng.uniform(0.5, 4.0),
    }
}

/// A unit-ish frame: the identity, a random rotation-like frame, or a slightly skewed
/// (not orthogonal) one.
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

/// A clamped knot vector for `n` control points of degree `p` with interior knots in
/// `(0, 1)`, sometimes repeated.
fn clamped(rng: &mut Rng, n: usize, p: usize) -> Vec<f64> {
    let interior = n - p - 1;
    let mut mid: Vec<f64> = (0..interior)
        .map(|i| (i + 1) as f64 / (interior + 1) as f64)
        .collect();
    if interior >= 2 && rng.chance(0.3) {
        // repeated interior knots (possibly above the degree)
        let at = rng.below(interior as u64 - 1) as usize;
        let v = mid[at];
        // (at most the degree: above it the C++ evaluates a calloc'd iso curve, see
        // the FFI's `iso_extraction`)
        let reps = 1 + rng.below((interior - at).min(p) as u64) as usize;
        for m in mid.iter_mut().skip(at).take(reps) {
            *m = v;
        }
    }
    let mut k = vec![0.0; p + 1];
    k.extend(mid);
    k.extend(vec![1.0; p + 1]);
    k
}

#[allow(clippy::too_many_lines)]
fn gen_bspline(rng: &mut Rng, b: &mut Vec<f64>) {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let mut nu = p + 1 + rng.below(4) as usize;
    let mut nv = q + 1 + rng.below(4) as usize;
    let mode = rng.below(14);
    if mode == 1 {
        // a repeated interior knot needs room
        nu = nu.max(2 * p + 2);
    }
    if matches!(mode, 2 | 3 | 5) {
        nu = nu.max(3);
        nv = nv.max(3);
    }
    let rational = rng.chance(0.25);
    let s = scale(rng);
    let mut ku = clamped(rng, nu, p);
    let mut kv = clamped(rng, nv, q);
    if mode == 1 {
        // an interior knot repeated p times (a C0 crease; above the degree the case is
        // rejected), sometimes p + 1 times
        let reps = if rng.chance(0.2) { p + 1 } else { p };
        for kk in ku.iter_mut().skip(p + 1).take(reps) {
            *kk = 0.5;
        }
        ku.sort_by(f64::total_cmp);
        if rng.chance(0.5) && nv > 2 * q {
            for kk in kv.iter_mut().skip(q + 1).take(q) {
                *kk = 0.5;
            }
            kv.sort_by(f64::total_cmp);
        }
    }
    let bump = if rng.chance(0.5) { 0.0 } else { 0.3 };
    let mut pts: Vec<[f64; 4]> = Vec::new();
    for i in 0..nu {
        for j in 0..nv {
            let u = i as f64 / (nu - 1) as f64;
            let v = j as f64 / (nv - 1) as f64;
            let w = if rational { rng.uniform(0.5, 2.0) } else { 1.0 };
            pts.push([
                (u + 1.0) * s,
                (v + 0.5) * s,
                bump * rng.uniform(-1.0, 1.0) * s,
                w,
            ]);
        }
    }
    let at = |i: usize, j: usize| i * nv + j;
    match mode {
        2 => {
            // internal pole: an interior row all at one point
            let i = 1 + rng.below((nu - 2) as u64) as usize;
            let c = pts[at(i, 0)];
            for j in 0..nv {
                pts[at(i, j)] = [c[0], c[1], c[2], pts[at(i, j)][3]];
            }
        }
        3 => {
            // degenerate interior row at the origin
            let i = 1 + rng.below((nu - 2) as u64) as usize;
            for j in 0..nv {
                pts[at(i, j)] = [0.0, 0.0, 0.0, pts[at(i, j)][3]];
            }
        }
        4 => {
            // collapsed (or nearly collapsed) boundary row: a pole
            let eps = [0.0, 1e-9, 1e-7, 1e-5][rng.below(4) as usize] * s;
            let c = pts[at(0, 0)];
            for j in 0..nv {
                pts[at(0, j)] = [c[0] + eps * j as f64, c[1], c[2], pts[at(0, j)][3]];
            }
        }
        5 => {
            // folded net: a middle column turns back
            let j = 1 + rng.below((nv - 2) as u64) as usize;
            for i in 0..nu {
                pts[at(i, j)][1] = pts[at(i, j - 1)][1] - 0.7 * s;
            }
        }
        6 => {
            // degenerate to a point
            for pt in &mut pts {
                *pt = [s, s, s, pt[3]];
            }
        }
        7 => {
            // degenerate to a curve: every row the same
            for i in 1..nu {
                for j in 0..nv {
                    pts[at(i, j)] = pts[at(0, j)];
                }
            }
        }
        8 => {
            // flat corner: the first u and v legs are parallel
            let c = pts[at(0, 0)];
            pts[at(1, 0)] = [c[0] + 0.3 * s, c[1], c[2], pts[at(1, 0)][3]];
            pts[at(0, 1)] = [c[0] + 0.6 * s, c[1], c[2], pts[at(0, 1)][3]];
        }
        9 => {
            // closed in u: the last row repeats the first
            for j in 0..nv {
                pts[at(nu - 1, j)] = pts[at(0, j)];
            }
        }
        10 => {
            // opposite v boundaries partially coincident (planar strip folded on itself)
            for i in 0..nu {
                let a = pts[at(i, 0)];
                let last = at(i, nv - 1);
                pts[last] = [a[0], a[1], a[2] + if i < nu / 2 { 0.0 } else { 0.5 * s }, pts[last][3]];
            }
        }
        11 => {
            // tiny or huge perturbation of a valid net
            let e = [1e-13, 1e-9, 1e-6][rng.below(3) as usize] * s;
            for pt in &mut pts {
                pt[2] += rng.uniform(-e, e);
            }
        }
        _ => {}
    }
    b.extend([p as f64, q as f64, nu as f64, nv as f64, f64::from(u8::from(rational))]);
    b.extend(ku);
    b.extend(kv);
    for pt in pts {
        let w = pt[3];
        if rational {
            // homogeneous coordinates
            b.extend([pt[0] * w, pt[1] * w, pt[2] * w, w]);
        } else {
            b.extend(pt);
        }
    }
}

fn angles(rng: &mut Rng) -> (f64, f64) {
    match rng.below(5) {
        0 => (0.0, 360.0),
        1 => (-360.0, 0.0),
        2 => {
            let a = rng.uniform(-300.0, 200.0);
            (a, a + rng.uniform(1.0, 160.0))
        }
        3 => (-180.0, 180.0),
        _ => (0.0, rng.uniform(10.0, 359.0)),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let kind = rng.below(7) as usize;
    let mut b = vec![kind as f64, level(rng)];
    b.push(if kind == 0 { 0.0 } else { flag(rng, 0.5) });
    let s = scale(rng);
    if kind == 0 {
        gen_bspline(rng, &mut b);
        return Inputs { block: b };
    }
    frame(rng, &mut b, s);
    match kind {
        1 => {
            let sx = match rng.below(6) {
                0 => -1.0,
                1 => 0.0,
                _ => rng.uniform(0.2, 3.0),
            };
            let sy = match rng.below(6) {
                0 => -0.5,
                1 => 1e-13,
                _ => rng.uniform(0.2, 3.0),
            };
            let u0 = rng.uniform(-2.0, 2.0) * s;
            let v0 = rng.uniform(-2.0, 2.0) * s;
            let du = if rng.chance(0.1) { 0.0 } else { rng.uniform(0.1, 3.0) * s };
            let dv = if rng.chance(0.1) { 0.0 } else { rng.uniform(0.1, 3.0) * s };
            b.extend([sx, sy, u0, v0, u0 + du, v0 + dv]);
        }
        2 => {
            let (a0, a1) = angles(rng);
            b.extend([s * rng.uniform(0.2, 2.0), a0, a1, s * rng.uniform(0.1, 3.0)]);
            b.extend([flag(rng, 0.3), flag(rng, 0.3), flag(rng, 0.3)]);
        }
        3 => {
            let (a0, a1) = angles(rng);
            let (bot, top) = match rng.below(6) {
                0 => (0.0, rng.uniform(0.5, 2.0)),
                1 => (rng.uniform(0.5, 2.0), 0.0),
                2 => (-rng.uniform(0.2, 1.0), rng.uniform(0.5, 2.0)),
                3 => (rng.uniform(0.5, 2.0), -rng.uniform(0.2, 1.0)),
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
                1 => major,
                _ => major * rng.uniform(0.05, 0.9),
            };
            b.extend([a0, v0, a1, v1, major, minor]);
            b.extend([flag(rng, 0.3), flag(rng, 0.3)]);
        }
        _ => {
            let (a0, a1) = angles(rng);
            let p = 1 + rng.below(3) as usize;
            let n = p + 1 + rng.below(3) as usize;
            let knots = clamped(rng, n, p);
            let (t0, t1) = match rng.below(5) {
                0 => (0.2, 0.8), // mismatched generator domain
                _ => (0.0, 1.0),
            };
            b.extend([a0, t0, a1, t1, flag(rng, 0.3)]);
            let degenerate = rng.chance(0.12);
            let rational = rng.chance(0.2);
            b.extend([p as f64, n as f64, f64::from(u8::from(rational))]);
            for i in 0..n {
                let t = i as f64 / (n - 1) as f64;
                let (r, z) = if degenerate {
                    (s, s * 1e-9 * t)
                } else {
                    (s * rng.uniform(0.3, 2.0), s * (t + rng.uniform(0.0, 0.1)))
                };
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
        "surface.surface_assert_valid"
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
