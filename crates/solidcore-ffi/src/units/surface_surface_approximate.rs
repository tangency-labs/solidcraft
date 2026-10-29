//! Safe wrapper for unit `surface.surface_approximate`: `SmSurface::ApproximateSurface`
//! on a real B-spline surface, plane, cylinder, cone, sphere, torus or surface of
//! revolution built from plain data, optionally inside up to three offset, curve bounded,
//! STEP or volume (rigid transform) surfaces (block layout in
//! `harness/cpp/shim/surface_surface_approximate.cpp`).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surface_approximate_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

/// Largest subdivision level accepted (each level can double the spans in each
/// direction, and every span is evaluated).
pub const MAX_LEVEL: usize = 4;

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(format!("surface_approximate: {what}"))
}

struct Rd<'a> {
    v: &'a [f64],
    at: usize,
}

impl Rd<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let x = *self.v.get(self.at).ok_or_else(|| bad("short block"))?;
        self.at += 1;
        if !x.is_finite() || x.abs() > 1e12 {
            return Err(bad("non-finite or huge value"));
        }
        Ok(x)
    }
    fn int(&mut self, lo: f64, hi: f64) -> Result<usize, FfiError> {
        let x = self.num()?;
        if x.fract() != 0.0 || x < lo || x > hi {
            return Err(bad("bad count"));
        }
        Ok(x as usize)
    }
    fn flag(&mut self) -> Result<bool, FfiError> {
        let x = self.num()?;
        if x != 0.0 && x != 1.0 {
            return Err(bad("bad flag"));
        }
        Ok(x == 1.0)
    }
    fn vec(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.num()?, self.num()?, self.num()?])
    }
    fn dom(&mut self) -> Result<[f64; 4], FfiError> {
        Ok([self.num()?, self.num()?, self.num()?, self.num()?])
    }
}

fn len(v: [f64; 3]) -> f64 {
    (v[0] * v[0] + v[1] * v[1] + v[2] * v[2]).sqrt()
}

/// Two axes long enough and far from parallel (the constructors unitize them).
fn axes(x: [f64; 3], y: [f64; 3]) -> Result<(), FfiError> {
    let c = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    if len(x) < 1e-6 || len(y) < 1e-6 || len(c) < 1e-3 * len(x) * len(y) {
        return Err(bad("degenerate or parallel axes"));
    }
    Ok(())
}

/// An angular interval the analytic constructors accept.
fn angular(d0: f64, d1: f64) -> Result<(), FfiError> {
    if d0 >= d1 || d0 < -360.0 || d1 > 360.0 || d1 - d0 > 360.0 {
        return Err(bad("bad angular domain"));
    }
    Ok(())
}

/// Non-decreasing knots of positive span, multiplicities at most the order.
fn knots(k: &[f64], p: usize) -> Result<(), FfiError> {
    if k.windows(2).any(|w| matches!(w, [a, b] if b < a)) || k.last() <= k.first() {
        return Err(bad("bad knots"));
    }
    let mut run = 1;
    for w in k.windows(2) {
        run = if matches!(w, [a, b] if a == b) {
            run + 1
        } else {
            1
        };
        if run > p + 1 {
            return Err(bad("knot multiplicity above the order"));
        }
    }
    Ok(())
}

/// `n` control points (`x y z w`), weights positive when rational.
fn points(rd: &mut Rd<'_>, n: usize, rational: bool) -> Result<(), FfiError> {
    for _ in 0..n {
        rd.vec()?;
        let w = rd.num()?;
        if rational && w <= 1e-6 {
            return Err(bad("weight not positive"));
        }
    }
    Ok(())
}

fn check(block: &[f64]) -> Result<(), FfiError> {
    let mut rd = Rd { v: block, at: 0 };
    rd.num()?; // tol
    rd.int(0.0, MAX_LEVEL as f64)?;
    rd.flag()?;
    let n_wrap = rd.int(0.0, 3.0)?;
    for _ in 0..n_wrap {
        match rd.int(0.0, 3.0)? {
            0 => {
                rd.num()?;
            }
            1 => {
                rd.flag()?;
            }
            2 => {}
            _ => {
                rd.vec()?;
                let x = rd.vec()?;
                let y = rd.vec()?;
                let z = rd.vec()?;
                axes(x, y)?;
                axes(y, z)?;
                axes(z, x)?;
            }
        }
    }
    let kind = rd.int(0.0, 6.0)?;
    if kind == 0 {
        let p = rd.int(1.0, 5.0)?;
        let q = rd.int(1.0, 5.0)?;
        let nu = rd.int(2.0, 32.0)?;
        let nv = rd.int(2.0, 32.0)?;
        if nu <= p || nv <= q {
            return Err(bad("too few control points for the degree"));
        }
        let rational = rd.flag()?;
        let ku: Vec<f64> = (0..=nu + p).map(|_| rd.num()).collect::<Result<_, _>>()?;
        let kv: Vec<f64> = (0..=nv + q).map(|_| rd.num()).collect::<Result<_, _>>()?;
        knots(&ku, p)?;
        knots(&kv, q)?;
        points(&mut rd, nu * nv, rational)?;
    } else {
        rd.vec()?;
        let x = rd.vec()?;
        let y = rd.vec()?;
        axes(x, y)?;
        match kind {
            1 => {
                let sx = rd.num()?;
                let sy = rd.num()?;
                let d = rd.dom()?;
                if sx.abs() < 1e-6 || sy.abs() < 1e-6 || d[0] >= d[2] || d[1] >= d[3] {
                    return Err(bad("degenerate plane scale or domain"));
                }
            }
            2 | 3 => {
                let (r0, r1) = if kind == 2 {
                    let r = rd.num()?;
                    (r, r)
                } else {
                    (rd.num()?, rd.num()?)
                };
                let s0 = rd.num()?;
                let s1 = rd.num()?;
                let h = rd.num()?;
                angular(s0, s1)?;
                if h <= 1e-6 || r0.abs().max(r1.abs()) < 1e-6 || (kind == 2 && r0 <= 1e-6) {
                    return Err(bad("degenerate height or radius"));
                }
                rd.flag()?;
                rd.flag()?;
                rd.flag()?;
            }
            4 => {
                let d = rd.dom()?;
                let r = rd.num()?;
                angular(d[0], d[2])?;
                if d[1] >= d[3] || d[1] < -90.0 || d[3] > 90.0 || r <= 1e-6 {
                    return Err(bad("sphere domain or radius"));
                }
                rd.flag()?;
                rd.flag()?;
            }
            5 => {
                let d = rd.dom()?;
                let major = rd.num()?;
                let minor = rd.num()?;
                angular(d[0], d[2])?;
                angular(d[1], d[3])?;
                if major <= 1e-6 || minor <= 1e-6 {
                    return Err(bad("torus radii"));
                }
                rd.flag()?;
                rd.flag()?;
            }
            _ => {
                let d = rd.dom()?;
                angular(d[0], d[2])?;
                if d[1] >= d[3] {
                    return Err(bad("revolution domain"));
                }
                rd.flag()?;
                let p = rd.int(1.0, 5.0)?;
                let n = rd.int(2.0, 32.0)?;
                if n <= p {
                    return Err(bad("too few generator points"));
                }
                let rational = rd.flag()?;
                points(&mut rd, n, rational)?;
                let k: Vec<f64> = (0..=n + p).map(|_| rd.num()).collect::<Result<_, _>>()?;
                knots(&k, p)?;
            }
        }
    }
    if rd.at != block.len() {
        return Err(bad("trailing values"));
    }
    Ok(())
}

/// Runs `SmSurface::ApproximateSurface(ctx, tol, achieved, level, result, just_copy)` on
/// the surface `block` describes and returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the constructors cannot take safely
/// (non-finite or huge values, degenerate axes, radii, heights, scales or domains,
/// angular domains outside the constructors' ranges, bad knots, non-positive weights,
/// degenerate transform axes, more than three wraps, a level above [`MAX_LEVEL`]); the
/// shim's status otherwise.
pub fn eval(block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check(block)?;
    call_with_buffer(1 << 16, |out, len| {
        // SAFETY: the validated block outlives the call; output and length pointers come
        // from call_with_buffer.
        unsafe {
            sys::sc_surface_surface_approximate_eval(
                block.as_ptr(),
                block.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
