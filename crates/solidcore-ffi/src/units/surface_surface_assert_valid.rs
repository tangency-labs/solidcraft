//! Safe wrapper for unit `surface.surface_assert_valid`: the pinned virtual
//! `SmSurface::AssertValid` on a real B-spline surface, plane, cylinder, cone, sphere,
//! torus, surface of revolution, surface of extrusion or curve-bounded surface built
//! from plain data (block
//! layout in `harness/cpp/shim/surface_surface_assert_valid.cpp`).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surface_assert_valid_eval(
            input: *const f64,
            n: i64,
            out: *mut f64,
            cap: i64,
            len: *mut i64,
        ) -> i32;
    }
}

fn bad(what: &str) -> FfiError {
    FfiError::InvalidArgument(format!("surface_assert_valid: {what}"))
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

/// Non-decreasing knots of positive span.
fn knots(k: &[f64]) -> Result<(), FfiError> {
    if k.windows(2).any(|w| matches!(w, [a, b] if b < a)) || k.last() <= k.first() {
        return Err(bad("bad knots"));
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

/// Knots clamped at both ends (multiplicity `degree + 1`), as the extrusion's generator
/// needs: its constructor evaluates the generator and builds its STEP interval.
fn clamped(k: &[f64], degree: usize) -> Result<(), FfiError> {
    let m = mults(k);
    if m.first() != Some(&(degree as i64 + 1)) || m.last() != Some(&(degree as i64 + 1)) {
        return Err(bad("generator knots not clamped"));
    }
    Ok(())
}

/// Control points spread over more than a point (the constructor's plane test of a
/// collapsed generator may fail and leave its result unset).
fn spread(block: &[f64], first: usize, n: usize, rational: bool) -> Result<(), FfiError> {
    let at = |i: usize, c: usize| block.get(first + 4 * i + c).copied().unwrap_or(0.0);
    // (homogeneous coordinates when rational)
    let w = |i: usize| if rational { at(i, 3) } else { 1.0 };
    let size = (1..n)
        .map(|i| {
            (0..3)
                .map(|c| (at(i, c) / w(i) - at(0, c) / w(0)).abs())
                .fold(0.0, f64::max)
        })
        .fold(0.0, f64::max);
    if size.is_nan() || size <= 1e-9 {
        return Err(bad("collapsed generator"));
    }
    Ok(())
}

/// Unique knots' multiplicities.
fn mults(k: &[f64]) -> Vec<i64> {
    let mut m: Vec<i64> = Vec::new();
    let mut prev: Option<f64> = None;
    for &x in k {
        if prev == Some(x) {
            if let Some(l) = m.last_mut() {
                *l += 1;
            }
        } else {
            m.push(1);
        }
        prev = Some(x);
    }
    m
}

/// `SmBSplineSurface::HasInternalPole` indexes the control net from the knot
/// multiplicities; interior knots of high multiplicity (or unclamped ends) send it
/// outside the net (undefined behaviour). Rejects every block whose index arithmetic
/// leaves the net, whether or not the early exits would reach the bad index.
fn internal_pole_indices(
    ku: &[f64],
    kv: &[f64],
    p: usize,
    q: usize,
    nu: usize,
    nv: usize,
) -> Result<(), FfiError> {
    let walk = |ka: &[i64], kb: &[i64], da: i64, db: i64, na: i64, nb: i64| -> bool {
        let Some(&m0) = ka.first() else { return false };
        let mut cnt_a = m0;
        for &ma in ka.iter().skip(1).take(ka.len().saturating_sub(2)) {
            cnt_a += ma;
            let rows = da - (ma - 1);
            let cp_a = cnt_a - rows - 1;
            let mut cnt_b = 0;
            for &mb in kb.iter().take(kb.len().saturating_sub(1)) {
                cnt_b += mb;
                let cp_b = (cnt_b - 1) - db;
                let lo_a = cp_a;
                let hi_a = if rows >= 1 { cp_a + rows - 1 } else { cp_a };
                if lo_a < 0 || hi_a >= na || cp_b < 0 || cp_b + db >= nb {
                    return false;
                }
            }
        }
        true
    };
    let (mu, mv) = (mults(ku), mults(kv));
    let (p, q, nu, nv) = (p as i64, q as i64, nu as i64, nv as i64);
    if !walk(&mu, &mv, p, q, nu, nv) || !walk(&mv, &mu, q, p, nv, nu) {
        return Err(bad("HasInternalPole indexes outside the control net"));
    }
    Ok(())
}

fn check(block: &[f64]) -> Result<(), FfiError> {
    let mut rd = Rd { v: block, at: 0 };
    let kind = rd.int(0.0, 8.0)?;
    let level = rd.int(0.0, 4.0)?;
    if level == 3 {
        return Err(bad("level"));
    }
    let make = rd.flag()?;
    if kind == 8 {
        rd.flag()?; // report list supplied
        rd.int(1.0, 8.0)?; // nesting is bounded in both builders
        rd.vec()?;
        let x = rd.vec()?;
        let y = rd.vec()?;
        axes(x, y)?;
        rd.num()?;
        rd.num()?;
        rd.dom()?;
        let n3d = rd.int(0.0, 8.0)?;
        let nuv = rd.int(0.0, 8.0)?;
        rd.flag()?;
        for _ in 0..n3d + nuv {
            rd.int(2.0, 3.0)?;
            rd.flag()?;
            rd.vec()?;
            rd.vec()?;
        }
    } else if kind == 0 {
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
        knots(&ku)?;
        knots(&kv)?;
        internal_pole_indices(&ku, &kv, p, q, nu, nv)?;
        points(&mut rd, nu * nv, rational)?;
    } else if kind == 7 {
        rd.vec()?;
        let v = rd.vec()?;
        let d = rd.dom()?;
        if d[0] > d[2] || d[1] > d[3] {
            return Err(bad("extrusion domain not ordered"));
        }
        // MakeNurb of a collapsed extrusion (zero vector or domain) is not exercised:
        // the check of its NURB is the B-spline unit's
        if make && (len(v) <= 1e-9 || d[2] - d[0] <= 1e-9 || d[3] - d[1] <= 1e-9) {
            return Err(bad("MakeNurb of a collapsed extrusion"));
        }
        rd.flag()?;
        let p = rd.int(1.0, 5.0)?;
        let n = rd.int(2.0, 32.0)?;
        if n <= p {
            return Err(bad("too few generator points"));
        }
        let rational = rd.flag()?;
        let first = rd.at;
        points(&mut rd, n, rational)?;
        spread(block, first, n, rational)?;
        let k: Vec<f64> = (0..=n + p).map(|_| rd.num()).collect::<Result<_, _>>()?;
        knots(&k)?;
        clamped(&k, p)?;
        rd.flag()?;
        rd.vec()?;
        rd.vec()?;
    } else {
        rd.vec()?;
        let x = rd.vec()?;
        let y = rd.vec()?;
        axes(x, y)?;
        match kind {
            1 => {
                rd.num()?;
                rd.num()?;
                rd.dom()?;
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
                // (a V interval past a pole is defined: sphere tests 4 and 5 report it)
                angular(d[1], d[3])?;
                if r <= 1e-6 {
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
                knots(&k)?;
            }
        }
    }
    if rd.at != block.len() {
        return Err(bad("trailing values"));
    }
    Ok(())
}

/// Runs `SmSurface::AssertValid(&reports, level, SM_NO_WALK)` on the surface `block`
/// describes and returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a block the constructors cannot take safely
/// (non-finite or huge values, degenerate axes, radii or heights, angular domains
/// outside the constructors' ranges, decreasing knots, non-positive weights, a level
/// other than 0, 1, 2 or `SM_LEVEL_GIVEN`); the shim's status otherwise.
pub fn eval(block: &[f64]) -> Result<Vec<f64>, FfiError> {
    check(block)?;
    call_with_buffer(4096, |out, len| {
        // SAFETY: the validated block outlives the call; output and length pointers come
        // from call_with_buffer.
        unsafe {
            sys::sc_surface_surface_assert_valid_eval(
                block.as_ptr(),
                block.len() as i64,
                out.as_mut_ptr(),
                out.len() as i64,
                &raw mut *len,
            )
        }
    })
}
