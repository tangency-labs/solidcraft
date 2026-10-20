//! Runner for unit `nlib.iges`: IGES entity 126 / 128 import into clamped NLib
//! curves and surfaces (`N_Iges126Crv`, `N_Iges128Srf`), knot / bound validation
//! (`N_IgesValidate`), and the stand-alone clamping and knot-correction routines
//! (`N_IgesClampCrv`, `N_IgesCorrectCrvKnots`, `N_IgesClampSrf`,
//! `N_IgesCorrectSrfKnots`).
//!
//! C++: `solidcore_ffi::units::nlib_iges` (upstream `source/SMLib/src/NL_Iges.cpp`).
//! Rust: `solidcore::nurbs::iges`.
//!
//! Both sides produce the same flat record (status, then the resulting curve or
//! surface), split into keys by [`to_output`]: `status` (`0` ok, the `NL_*_ERR`
//! number, `1000` for an error flag without a number; `-1` on the Rust side for a
//! non-NLib error, which never matches), `dims` (`n p m` / `n m p q r s`), `cpts`
//! (`x y z w` per control point, row-major for nets), `u` and `v` knots.
//!
//! Generated cases cover random knot vectors with repeated knots (interior
//! multiplicities above the degree, end multiplicities above `p + 1`, junk end
//! knots), bounds on and between knots, rational / non-rational / 2D (`z = NOZ`)
//! data, and the error paths (degree, weight, knot / bound validation). Every run has
//! at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::iges;
use solidcore::nurbs::math::{CPoint, NOW, NOZ};
use solidcore::nurbs::srf_basic::Surface;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_iges::{self as ffi, SurfaceArgs};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.iges` runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    /// `N_Iges126Crv`.
    Crv126 {
        k: usize,
        m: usize,
        t: Vec<f64>,
        w: Vec<f64>,
        pts: Vec<[f64; 3]>,
        v: [f64; 2],
    },
    /// `N_IgesValidate` (`m = u.len() - 1`).
    Validate { u: Vec<f64>, p: usize, v: [f64; 2] },
    /// `N_Iges128Srf`; `w` and `pts` row-major `(k1+1) x (k2+1)`.
    Srf128 {
        k: [usize; 2],
        deg: [usize; 2],
        s: Vec<f64>,
        t: Vec<f64>,
        w: Vec<f64>,
        pts: Vec<[f64; 3]>,
        u: [f64; 2],
        v: [f64; 2],
    },
    /// `N_IgesClampCrv`.
    ClampCrv {
        p: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
        b: [f64; 2],
    },
    /// `N_IgesCorrectCrvKnots`.
    CorrectCrv {
        p: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
    /// `N_IgesClampSrf`; `net` row-major `size[0] x size[1]`.
    ClampSrf {
        deg: [usize; 2],
        size: [usize; 2],
        net: Vec<[f64; 4]>,
        u: Vec<f64>,
        v: Vec<f64>,
        ub: [f64; 2],
        vb: [f64; 2],
    },
    /// `N_IgesCorrectSrfKnots`.
    CorrectSrf {
        deg: [usize; 2],
        size: [usize; 2],
        net: Vec<[f64; 4]>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- records ------------------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let s = flat
        .get(*at..*at + k)
        .ok_or_else(|| harness("record too short"))?;
    *at += k;
    Ok(s)
}

fn count(x: f64) -> Result<usize, HarnessError> {
    if x < 0.0 || x.fract() != 0.0 {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

fn ints(v: &[f64]) -> Vec<i64> {
    v.iter().map(|&x| x as i64).collect()
}

/// Splits a record into keys.
fn to_output(surface: bool, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0;
    let status = take(flat, &mut at, 1)?;
    let mut out = Output::new().ints("status", ints(status));
    if status.first().copied() != Some(0.0) || at == flat.len() {
        if at != flat.len() {
            return Err(harness("trailing record data after an error"));
        }
        return Ok(out);
    }
    if surface {
        let dims = take(flat, &mut at, 6)?;
        let [n, m, _, _, r, s] = <[f64; 6]>::try_from(dims).map_err(|_| harness("dims"))?;
        let (n, m, r, s) = (count(n)?, count(m)?, count(r)?, count(s)?);
        out = out.ints("dims", ints(dims));
        out = out.values("cpts", take(flat, &mut at, 4 * (n + 1) * (m + 1))?.to_vec());
        out = out.values("u", take(flat, &mut at, r + 1)?.to_vec());
        out = out.values("v", take(flat, &mut at, s + 1)?.to_vec());
    } else {
        let dims = take(flat, &mut at, 3)?;
        let [n, _, m] = <[f64; 3]>::try_from(dims).map_err(|_| harness("dims"))?;
        let (n, m) = (count(n)?, count(m)?);
        out = out.ints("dims", ints(dims));
        out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
        out = out.values("u", take(flat, &mut at, m + 1)?.to_vec());
    }
    if at != flat.len() {
        return Err(harness("trailing record data"));
    }
    Ok(out)
}

fn is_surface(i: &Inputs) -> bool {
    matches!(
        i,
        Inputs::Srf128 { .. } | Inputs::ClampSrf { .. } | Inputs::CorrectSrf { .. }
    )
}

fn status_of(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        SolidError::Degenerate { .. } => 1000.0,
        _ => -1.0,
    }
}

fn dump_curve(o: &mut Vec<f64>, c: &Curve) {
    let (n, m) = c.array_sizes();
    o.extend([n as f64, c.degree() as f64, m as f64]);
    for pw in c.cpts() {
        o.extend(pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

fn dump_surface(o: &mut Vec<f64>, s: &Surface) {
    let i = s.indices();
    o.extend([i.n, i.m, i.p, i.q, i.r, i.s].map(|x| x as f64));
    for row in s.rows() {
        for pw in row {
            o.extend(pw.to_array());
        }
    }
    o.extend_from_slice(s.u_knots());
    o.extend_from_slice(s.v_knots());
}

fn record_curve(r: SolidResult<Curve>) -> Vec<f64> {
    match r {
        Ok(c) => {
            let mut o = vec![0.0];
            dump_curve(&mut o, &c);
            o
        }
        Err(e) => vec![status_of(&e)],
    }
}

fn record_surface(r: SolidResult<Surface>) -> Vec<f64> {
    match r {
        Ok(s) => {
            let mut o = vec![0.0];
            dump_surface(&mut o, &s);
            o
        }
        Err(e) => vec![status_of(&e)],
    }
}

// ---- the two sides --------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let rec = match i {
        Inputs::Crv126 { k, m, t, w, pts, v } => ffi::iges_126(*k, *m, t, w, pts, *v),
        Inputs::Validate { u, p, v } => ffi::validate(u, *p, *v),
        Inputs::Srf128 {
            k,
            deg,
            s,
            t,
            w,
            pts,
            u,
            v,
        } => ffi::iges_128(*k, *deg, s, t, w, pts, *u, *v),
        Inputs::ClampCrv { p, cpts, knots, b } => ffi::clamp_curve(*p, cpts, knots, *b),
        Inputs::CorrectCrv { p, cpts, knots } => ffi::correct_curve(*p, cpts, knots),
        Inputs::ClampSrf {
            deg,
            size,
            net,
            u,
            v,
            ub,
            vb,
        } => ffi::clamp_surface(
            &SurfaceArgs {
                deg: *deg,
                size: *size,
                net,
                u,
                v,
            },
            *ub,
            *vb,
        ),
        Inputs::CorrectSrf {
            deg,
            size,
            net,
            u,
            v,
        } => ffi::correct_surface(&SurfaceArgs {
            deg: *deg,
            size: *size,
            net,
            u,
            v,
        }),
    };
    match rec {
        Ok(flat) => to_output(is_surface(i), &flat),
        Err(e @ FfiError::InvalidArgument(_)) => Err(harness(e.to_string())),
        Err(e) => Err(harness(format!("shim failed: {e}"))),
    }
}

fn vec3(p: &[f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

fn cpoints(c: &[[f64; 4]]) -> Vec<CPoint> {
    c.iter()
        .map(|&[x, y, z, w]| CPoint::new(x, y, z, w))
        .collect()
}

fn net_rows(net: &[[f64; 4]], size: [usize; 2]) -> Result<Vec<Vec<CPoint>>, HarnessError> {
    if size[0].checked_mul(size[1]) != Some(net.len()) || size[1] == 0 {
        return Err(harness("net size mismatch"));
    }
    Ok(net.chunks(size[1]).map(cpoints).collect())
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let rec = match i {
        Inputs::Crv126 { k, m, t, w, pts, v } => {
            let pts: Vec<Vec3> = pts.iter().map(vec3).collect();
            record_curve(iges::iges_126_curve(&iges::Entity126 {
                k: *k,
                m: *m,
                t,
                w,
                pts: &pts,
                v: *v,
            }))
        }
        Inputs::Validate { u, p, v } => {
            let m = u.len().checked_sub(1).ok_or_else(|| harness("no knots"))?;
            match iges::validate(u, m, *p, *v) {
                Ok(()) => vec![0.0],
                Err(e) => vec![status_of(&e)],
            }
        }
        Inputs::Srf128 {
            k,
            deg,
            s,
            t,
            w,
            pts,
            u,
            v,
        } => {
            let cols = k[1].checked_add(1).ok_or_else(|| harness("128: bad k"))?;
            let rows = k[0].checked_add(1).ok_or_else(|| harness("128: bad k"))?;
            if rows.checked_mul(cols) != Some(w.len()) || pts.len() != w.len() {
                return Err(harness("128: net size mismatch"));
            }
            let w: Vec<Vec<f64>> = w.chunks(cols).map(<[f64]>::to_vec).collect();
            let pts: Vec<Vec<Vec3>> = pts
                .chunks(cols)
                .map(|r| r.iter().map(vec3).collect())
                .collect();
            record_surface(iges::iges_128_surface(&iges::Entity128 {
                k1: k[0],
                k2: k[1],
                m1: deg[0],
                m2: deg[1],
                s,
                t,
                w: &w,
                pts: &pts,
                u: *u,
                v: *v,
            }))
        }
        Inputs::ClampCrv { p, cpts, knots, b } => {
            let r = Curve::new(cpoints(cpts), *p, knots.clone()).and_then(|mut c| {
                iges::clamp_curve(&mut c, *b)?;
                Ok(c)
            });
            record_curve(r)
        }
        Inputs::CorrectCrv { p, cpts, knots } => {
            let r = Curve::new(cpoints(cpts), *p, knots.clone()).and_then(|mut c| {
                iges::correct_curve_knots(&mut c)?;
                Ok(c)
            });
            record_curve(r)
        }
        Inputs::ClampSrf {
            deg,
            size,
            net,
            u,
            v,
            ub,
            vb,
        } => {
            let rows = net_rows(net, *size)?;
            let r = Surface::new(rows, deg[0], deg[1], u.clone(), v.clone()).and_then(|mut s| {
                iges::clamp_surface(&mut s, *ub, *vb)?;
                Ok(s)
            });
            record_surface(r)
        }
        Inputs::CorrectSrf {
            deg,
            size,
            net,
            u,
            v,
        } => {
            let rows = net_rows(net, *size)?;
            let r = Surface::new(rows, deg[0], deg[1], u.clone(), v.clone()).and_then(|mut s| {
                iges::correct_surface_knots(&mut s)?;
                Ok(s)
            });
            record_surface(r)
        }
    };
    to_output(is_surface(i), &rec)
}

// ---- generation ------------------------------------------------------------------------

/// `u[i]`, or `0` outside.
fn kget(u: &[f64], i: usize) -> f64 {
    u.get(i).copied().unwrap_or(0.0)
}

/// `u[i] = x` when `i` is inside.
fn kset(u: &mut [f64], i: usize, x: f64) {
    if let Some(s) = u.get_mut(i) {
        *s = x;
    }
}

/// Random non-decreasing knots `U[0..=n+p+1]` with `U[p] < U[m-p]`: increments are
/// zero (repeated knots) with probability `zero`, integral (so bounds can hit knots)
/// or fractional otherwise.
fn gen_knots(rng: &mut Rng, n: usize, p: usize, zero: f64) -> Vec<f64> {
    let m = n + p + 1;
    let integral = rng.chance(0.5);
    let start = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    loop {
        let mut u = Vec::with_capacity(m + 1);
        let mut x = start;
        for _ in 0..=m {
            u.push(x);
            if !rng.chance(zero) {
                x += if integral {
                    rng.range_i64(1, 2) as f64
                } else {
                    rng.uniform(0.05, 1.5)
                };
            }
        }
        if kget(&u, p) < kget(&u, m - p) {
            return u;
        }
    }
}

/// Makes `u` clamped: `U[0..=p] = U[p]`, `U[m-p..=m] = U[m-p]` (optionally with an
/// extra repeated knot at either end). Keeps `U[p] < U[m-p]`.
fn clamp_ends(rng: &mut Rng, u: &mut [f64], p: usize) {
    let m = u.len() - 1;
    let (a, b) = (kget(u, p), kget(u, m - p));
    for i in 0..=p {
        kset(u, i, a);
    }
    for i in m - p..=m {
        kset(u, i, b);
    }
    // Extra end multiplicity (the "already clamped" path keeps it) when room is left.
    if m >= 2 * p + 3 {
        if rng.chance(0.25) && kget(u, p + 1) < b {
            kset(u, p + 1, a);
        }
        if rng.chance(0.25) && kget(u, m - p - 1) > a {
            kset(u, m - p - 1, b);
        }
    }
}

/// Bounds with `U[p] <= b0 < b1 <= U[m-p]`: at the ends, on interior knots, or between.
fn gen_bounds(rng: &mut Rng, u: &[f64], p: usize) -> [f64; 2] {
    let m = u.len() - 1;
    let (lo, hi) = (kget(u, p), kget(u, m - p));
    let inner: Vec<f64> = u.iter().skip(p).take(m + 1 - 2 * p).copied().collect();
    loop {
        let mut pick = |end: f64| match rng.below(3) {
            0 => end,
            1 => *rng.choose(&inner).unwrap_or(&end),
            _ => rng.uniform(lo, hi),
        };
        let a = pick(lo);
        let b = pick(hi);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        if a < b && lo <= a && b <= hi {
            return [a, b];
        }
    }
}

/// Junk end knots: `U[0]` below `U[1..=p]`, `U[m]` above `U[m-p..m]`.
fn junk_ends(rng: &mut Rng, u: &mut [f64], p: usize) {
    let m = u.len() - 1;
    let (a, b) = (kget(u, p), kget(u, m - p));
    if rng.chance(0.5) {
        for i in 1..=p {
            kset(u, i, a);
        }
        kset(u, 0, a - rng.uniform(0.1, 2.0));
    }
    if rng.chance(0.5) {
        for i in m - p..m {
            kset(u, i, b);
        }
        kset(u, m, b + rng.uniform(0.1, 2.0));
    }
}

fn gen_weights(rng: &mut Rng, k: usize) -> Vec<f64> {
    match rng.below(4) {
        0 => vec![1.0; k],
        1 => vec![2.5; k],
        _ => (0..k).map(|_| rng.uniform(0.2, 5.0)).collect(),
    }
}

/// Homogeneous control points (rational: weighted with a random weight).
fn gen_cpts(rng: &mut Rng, k: usize, two_d: bool) -> Vec<[f64; 4]> {
    let rat = rng.chance(0.5);
    (0..k)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            let z = if two_d { NOZ } else { z };
            if rat {
                let w = rng.uniform(0.2, 5.0);
                [w * x, w * y, if two_d { NOZ } else { w * z }, w]
            } else {
                [x, y, z, NOW]
            }
        })
        .collect()
}

fn gen_crv126(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(1, 5) as usize;
    let k = p + rng.below(8) as usize;
    let mut t = gen_knots(rng, k, p, 0.3);
    if rng.chance(0.3) {
        junk_ends(rng, &mut t, p);
    }
    let mut v = gen_bounds(rng, &t, p);
    let mut w = gen_weights(rng, k + 1);
    let two_d = rng.chance(0.2);
    let pts: Vec<[f64; 3]> = (0..=k)
        .map(|_| {
            let [x, y, z] = rng.point3(-10.0, 10.0);
            [x, y, if two_d { NOZ } else { z }]
        })
        .collect();
    let mut deg = p;
    // Error paths.
    match rng.below(20) {
        0 => deg = *rng.choose(&[0usize, 18, 25]).unwrap_or(&0),
        1 => {
            let i = rng.below(w.len() as u64) as usize;
            let bad = *rng.choose(&[1e-4, 150.0]).unwrap_or(&0.0);
            kset(&mut w, i, bad);
        }
        2 => v = [v[1], v[0]],
        3 => v[0] = kget(&t, p) - 1.0,
        4 => v[1] = kget(&t, t.len() - 1 - p) + 1.0,
        5 => {
            let i = rng.below(t.len() as u64 - 1) as usize;
            let (a, b) = (kget(&t, i), kget(&t, i + 1));
            kset(&mut t, i, b + 0.5);
            kset(&mut t, i + 1, a);
        }
        _ => {}
    }
    if deg != p {
        // Keep the array lengths consistent with the claimed degree.
        t.resize(k + deg + 2, t.last().copied().unwrap_or(0.0));
    }
    Inputs::Crv126 {
        k,
        m: deg,
        t,
        w,
        pts,
        v,
    }
}

fn gen_validate(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(0, 4) as usize;
    let n = p + rng.below(6) as usize;
    let mut u = gen_knots(rng, n, p, 0.3);
    let mut v = gen_bounds(rng, &u, p);
    match rng.below(6) {
        0 => v = [v[1], v[0]],
        1 => v = [v[0], v[0]],
        2 => v[0] -= 0.25,
        3 => v[1] += 0.25,
        4 => {
            let i = rng.below(u.len() as u64 - 1) as usize;
            let x = kget(&u, i) - 0.5;
            kset(&mut u, i + 1, x);
        }
        _ => {}
    }
    Inputs::Validate { u, p, v }
}

fn gen_srf128(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(1, 4) as usize;
    let q = rng.range_i64(1, 4) as usize;
    let k1 = p + rng.below(5) as usize;
    let k2 = q + rng.below(5) as usize;
    let mut s = gen_knots(rng, k1, p, 0.3);
    let mut t = gen_knots(rng, k2, q, 0.3);
    if rng.chance(0.3) {
        junk_ends(rng, &mut s, p);
    }
    if rng.chance(0.3) {
        junk_ends(rng, &mut t, q);
    }
    let mut u = gen_bounds(rng, &s, p);
    let v = gen_bounds(rng, &t, q);
    let np = (k1 + 1) * (k2 + 1);
    let mut w = gen_weights(rng, np);
    let pts: Vec<[f64; 3]> = (0..np).map(|_| rng.point3(-10.0, 10.0)).collect();
    let mut deg = [p, q];
    match rng.below(20) {
        0 => deg[0] = 0,
        1 => deg[1] = 18,
        2 => {
            let i = rng.below(np as u64) as usize;
            kset(&mut w, i, 1e-4);
        }
        3 => u = [u[1], u[0]],
        4 => {
            let x = t.last().copied().unwrap_or(0.0) + 1.0;
            kset(&mut t, 0, x);
        }
        _ => {}
    }
    s.resize(k1 + deg[0] + 2, s.last().copied().unwrap_or(0.0));
    t.resize(k2 + deg[1] + 2, t.last().copied().unwrap_or(0.0));
    Inputs::Srf128 {
        k: [k1, k2],
        deg,
        s,
        t,
        w,
        pts,
        u,
        v,
    }
}

fn gen_clamp_crv(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(1, 5) as usize;
    let n = p + rng.below(8) as usize;
    let mut knots = gen_knots(rng, n, p, 0.3);
    if rng.chance(0.3) {
        clamp_ends(rng, &mut knots, p);
    }
    if rng.chance(0.3) {
        junk_ends(rng, &mut knots, p);
    }
    let b = gen_bounds(rng, &knots, p);
    let two_d = rng.chance(0.2);
    let cpts = gen_cpts(rng, n + 1, two_d);
    Inputs::ClampCrv { p, cpts, knots, b }
}

fn gen_correct_crv(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(1, 5) as usize;
    let n = p + rng.below(9) as usize;
    let mut knots = gen_knots(rng, n, p, 0.45);
    clamp_ends(rng, &mut knots, p);
    let two_d = rng.chance(0.2);
    let cpts = gen_cpts(rng, n + 1, two_d);
    Inputs::CorrectCrv { p, cpts, knots }
}

fn gen_net(rng: &mut Rng, rows: usize, cols: usize) -> Vec<[f64; 4]> {
    gen_cpts(rng, rows * cols, false)
}

fn gen_clamp_srf(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(1, 4) as usize;
    let q = rng.range_i64(1, 4) as usize;
    let n = p + rng.below(5) as usize;
    let m = q + rng.below(5) as usize;
    let mut u = gen_knots(rng, n, p, 0.3);
    let mut v = gen_knots(rng, m, q, 0.3);
    for (kn, d) in [(&mut u, p), (&mut v, q)] {
        if rng.chance(0.3) {
            clamp_ends(rng, kn, d);
        }
        if rng.chance(0.3) {
            junk_ends(rng, kn, d);
        }
    }
    let ub = gen_bounds(rng, &u, p);
    let vb = gen_bounds(rng, &v, q);
    Inputs::ClampSrf {
        deg: [p, q],
        size: [n + 1, m + 1],
        net: gen_net(rng, n + 1, m + 1),
        u,
        v,
        ub,
        vb,
    }
}

fn gen_correct_srf(rng: &mut Rng) -> Inputs {
    let p = rng.range_i64(1, 4) as usize;
    let q = rng.range_i64(1, 4) as usize;
    let n = p + rng.below(6) as usize;
    let m = q + rng.below(6) as usize;
    let mut u = gen_knots(rng, n, p, 0.45);
    let mut v = gen_knots(rng, m, q, 0.45);
    clamp_ends(rng, &mut u, p);
    clamp_ends(rng, &mut v, q);
    Inputs::CorrectSrf {
        deg: [p, q],
        size: [n + 1, m + 1],
        net: gen_net(rng, n + 1, m + 1),
        u,
        v,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.iges"
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

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = match rng.below(7) {
            0 => gen_crv126(rng),
            1 => gen_validate(rng),
            2 => gen_srf128(rng),
            3 => gen_clamp_crv(rng),
            4 => gen_correct_crv(rng),
            5 => gen_clamp_srf(rng),
            _ => gen_correct_srf(rng),
        };
        serde_json::to_value(i).ok()
    }
}
