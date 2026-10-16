//! Runner for unit `nlib.crv_fit_p3`: curve fitting part 3 (interpolation with two
//! end derivatives, the cubic spline, first derivatives at every point, end tangents
//! with suggested knots, closed data with end conditions, higher end derivatives).
//!
//! C++: `solidcore_ffi::units::nlib_crv_fit_p3` (upstream
//! `source/SMLib/src/NL_CrvFit.cpp`). Rust: `solidcore::nurbs::crv_fit::p3`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record
//! (see the ffi module), split into keys: `status` (`0` ok, else the `NL_*_ERR`
//! number; `-1` on the Rust side for a non-NLib error, which never matches); for
//! `first_derivs` the derivatives after the call (`d`); for `knots_and_tangents` the
//! tangents after the call (`ts`, `te`) and the knot vector after the call
//! (`knt_count`, `knt`); then on success the curve: `dims = [n, p, m]`, `cpts`
//! (x y z w) and `knots`.
//!
//! Points are `[x, y, z]`. Every run has at least [`MIN_GENERATED`] generated cases.

// Harness code: the input builders index their own freshly sized arrays; a panic
// would fail the case (runner panics are caught).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::math::Vec3;
use solidcore::nurbs::crv_basic::Curve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::nurbs::crv_fit::p3::{self as fit, EndTangents, ParamSource, VectorKind};
use solidcore::nurbs::math::CPoint;
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_crv_fit_p3::{self as ffi, ExistingCurve, Tangents};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_fit_p3` runner.
pub struct Runner;

/// An existing output curve (zero control points).
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CurIn {
    n: usize,
    p: usize,
    knots: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    EndDerivs {
        pts: Vec<[f64; 3]>,
        p: usize,
        ds: [f64; 3],
        de: [f64; 3],
        par: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        cur: Option<CurIn>,
    },
    CubicSpline {
        pts: Vec<[f64; 3]>,
        ds: [f64; 3],
        de: [f64; 3],
        par: i32,
    },
    FirstDerivs {
        pts: Vec<[f64; 3]>,
        d: Vec<[f64; 3]>,
        p: usize,
        vec: i32,
        par: i32,
    },
    KnotsAndTangents {
        pts: Vec<[f64; 3]>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        u: Option<Vec<f64>>,
        tan: i32,
        ts: [f64; 3],
        te: [f64; 3],
        der: i32,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        knots: Option<Vec<f64>>,
        per: f64,
        p: usize,
    },
    ClosedConditions {
        pts: Vec<[f64; 3]>,
        p: usize,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        d: Option<Vec<[f64; 3]>>,
        k: usize,
        par: i32,
    },
    HighDerivs {
        pts: Vec<[f64; 3]>,
        p: usize,
        #[serde(default)]
        ds: Vec<[f64; 3]>,
        #[serde(default)]
        de: Vec<[f64; 3]>,
        par: i32,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- output -----------------------------------------------------------------------

fn count(x: f64) -> Result<usize, HarnessError> {
    if x >= 0.0 && x.fract() == 0.0 && x < 1e9 {
        Ok(x as usize)
    } else {
        Err(harness(format!("bad count {x}")))
    }
}

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let s = flat
        .get(*at..*at + k)
        .ok_or_else(|| harness("record too short"))?;
    *at += k;
    Ok(s)
}

fn int(x: f64) -> i64 {
    x as i64
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0;
    let st = take(flat, &mut at, 1)?[0];
    let mut out = Output::new().int("status", int(st));
    match i {
        Inputs::FirstDerivs { pts, .. } => {
            out = out.values("d", take(flat, &mut at, 3 * pts.len())?.to_vec());
        }
        Inputs::KnotsAndTangents { .. } => {
            out = out.values("ts", take(flat, &mut at, 3)?.to_vec());
            out = out.values("te", take(flat, &mut at, 3)?.to_vec());
            let c = count(take(flat, &mut at, 1)?[0])?;
            out = out.int("knt_count", c as i64);
            out = out.values("knt", take(flat, &mut at, c)?.to_vec());
        }
        _ => {}
    }
    if st == 0.0 {
        let d = take(flat, &mut at, 3)?;
        let (n, m) = (count(d[0])?, count(d[2])?);
        out = out.ints("dims", d.iter().map(|x| int(*x)).collect::<Vec<_>>());
        out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
        out = out.values("knots", take(flat, &mut at, m + 1)?.to_vec());
    }
    if at == flat.len() {
        Ok(out)
    } else {
        Err(harness(format!("record has {} extra values", flat.len() - at)))
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn cpp_flat(i: &Inputs) -> Result<Vec<f64>, FfiError> {
    match i {
        Inputs::EndDerivs {
            pts,
            p,
            ds,
            de,
            par,
            cur,
        } => {
            let c = cur.as_ref().map(|c| ExistingCurve {
                n: c.n,
                p: c.p,
                knots: &c.knots,
            });
            ffi::end_derivs(pts, *p, *ds, *de, *par, c.as_ref())
        }
        Inputs::CubicSpline { pts, ds, de, par } => ffi::cubic_spline(pts, *ds, *de, *par),
        Inputs::FirstDerivs {
            pts,
            d,
            p,
            vec,
            par,
        } => ffi::first_derivs(pts, d, *p, *vec, *par),
        Inputs::KnotsAndTangents {
            pts,
            u,
            tan,
            ts,
            te,
            der,
            knots,
            per,
            p,
        } => ffi::knots_and_tangents(&Tangents {
            pts,
            u: u.as_deref(),
            tan: *tan,
            ts: *ts,
            te: *te,
            der: *der,
            knots: knots.as_deref(),
            per: *per,
            p: *p,
        }),
        Inputs::ClosedConditions { pts, p, d, k, par } => {
            ffi::closed_conditions(pts, *p, d.as_deref(), *k, *par)
        }
        Inputs::HighDerivs {
            pts,
            p,
            ds,
            de,
            par,
        } => ffi::high_derivs(pts, *p, ds, de, *par),
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    match cpp_flat(i) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
        Err(e) => Err(harness(format!("shim failure: {e}"))),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// `0` for `Ok`, the `NL_*_ERR` number for an NLib error, `-1` otherwise.
fn status<T>(r: &SolidResult<T>) -> f64 {
    match r {
        Ok(_) => 0.0,
        Err(SolidError::Nlib { code, .. }) => f64::from(code.code()),
        Err(_) => -1.0,
    }
}

fn push_curve(o: &mut Vec<f64>, c: &Curve) {
    let (n, m) = c.array_sizes();
    o.extend([n as f64, c.degree() as f64, m as f64]);
    for pw in c.cpts().iter().take(n + 1) {
        o.extend_from_slice(&pw.to_array());
    }
    o.extend(c.knots().iter().take(m + 1));
}

fn curve_record(r: &SolidResult<Curve>) -> Vec<f64> {
    let mut o = vec![status(r)];
    if let Ok(c) = r {
        push_curve(&mut o, c);
    }
    o
}

fn vs(v: &[[f64; 3]]) -> Vec<Vec3> {
    v.iter().copied().map(v3).collect()
}

fn a3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn par_method(par: i32) -> Result<ParamMethod, HarnessError> {
    match par {
        1 => Ok(ParamMethod::Uniform),
        2 => Ok(ParamMethod::ChordLength),
        3 => Ok(ParamMethod::Centripetal),
        _ => Err(harness(format!("par {par}"))),
    }
}

fn kind(v: i32) -> Result<VectorKind, HarnessError> {
    match v {
        1 => Ok(VectorKind::Tangent),
        2 => Ok(VectorKind::Derivative),
        _ => Err(harness(format!("vector kind {v}"))),
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    Ok(match i {
        Inputs::EndDerivs {
            pts,
            p,
            ds,
            de,
            par,
            cur,
        } => {
            let src = if *par == 4 {
                ParamSource::Inherited
            } else {
                ParamSource::Method(par_method(*par)?)
            };
            let mut slot = match cur {
                Some(c) => Some(
                    Curve::new(vec![CPoint::new(0.0, 0.0, 0.0, 0.0); c.n + 1], c.p, c.knots.clone())
                        .map_err(|e| harness(format!("existing curve: {e}")))?,
                ),
                None => None,
            };
            let r = fit::interp_end_derivs(&vs(pts), *p, v3(*ds), v3(*de), src, &mut slot);
            let mut o = vec![status(&r)];
            if r.is_ok() {
                let c = slot.ok_or_else(|| harness("no curve"))?;
                push_curve(&mut o, &c);
            }
            o
        }
        Inputs::CubicSpline { pts, ds, de, par } => curve_record(&fit::cubic_spline_interp(
            &vs(pts),
            v3(*ds),
            v3(*de),
            par_method(*par)?,
        )),
        Inputs::FirstDerivs {
            pts,
            d,
            p,
            vec,
            par,
        } => {
            let mut dv = vs(d);
            let r = fit::interp_first_derivs(&vs(pts), &mut dv, *p, kind(*vec)?, par_method(*par)?);
            let mut o = vec![status(&r)];
            for v in &dv {
                o.extend(a3(*v));
            }
            if let Ok(c) = &r {
                push_curve(&mut o, c);
            }
            o
        }
        Inputs::KnotsAndTangents {
            pts,
            u,
            tan,
            ts,
            te,
            der,
            knots,
            per,
            p,
        } => {
            let mut tsv = v3(*ts);
            let mut tev = v3(*te);
            let mut knt = knots.clone();
            let tangents = match tan {
                0 => EndTangents::None,
                1 => EndTangents::Start(&mut tsv),
                2 => EndTangents::End(&mut tev),
                3 => EndTangents::Both(&mut tsv, &mut tev),
                4 => EndTangents::Same(&mut tsv),
                t => return Err(harness(format!("tan {t}"))),
            };
            let r = fit::interp_knots_and_tangents(
                &vs(pts),
                u.as_deref(),
                tangents,
                kind(*der)?,
                &mut knt,
                *per,
                *p,
            );
            let mut o = vec![status(&r)];
            o.extend(a3(tsv));
            o.extend(a3(tev));
            match &knt {
                Some(k) => {
                    o.push(k.len() as f64);
                    o.extend(k);
                }
                None => o.push(0.0),
            }
            if let Ok(c) = &r {
                push_curve(&mut o, c);
            }
            o
        }
        Inputs::ClosedConditions { pts, p, d, k, par } => {
            let dv = d.as_ref().map(|d| vs(d));
            curve_record(&fit::approx_closed_conditions(
                &vs(pts),
                *p,
                dv.as_deref(),
                *k,
                par_method(*par)?,
            ))
        }
        Inputs::HighDerivs {
            pts,
            p,
            ds,
            de,
            par,
        } => curve_record(&fit::interp_high_derivs(
            &vs(pts),
            *p,
            &vs(ds),
            &vs(de),
            par_method(*par)?,
        )),
    })
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generation -------------------------------------------------------------------

fn below(rng: &mut Rng, n: usize) -> usize {
    if n == 0 {
        0
    } else {
        rng.below(n as u64) as usize
    }
}

fn coord(rng: &mut Rng) -> f64 {
    let c = rng.uniform(-10.0, 10.0);
    if rng.chance(0.1) {
        c.round()
    } else {
        c
    }
}

/// A random walk of `cnt` points with distinct consecutive points.
fn gen_pts(rng: &mut Rng, cnt: usize) -> Vec<[f64; 3]> {
    let mut pts: Vec<[f64; 3]> = Vec::with_capacity(cnt);
    let mut cur = [coord(rng), coord(rng), coord(rng)];
    let flat = rng.chance(0.2);
    if flat {
        cur[2] = 0.0;
    }
    let scale = if rng.chance(0.1) {
        rng.uniform(0.001, 0.1)
    } else {
        1.0
    };
    for i in 0..cnt {
        if i > 0 {
            loop {
                let step = [
                    rng.uniform(-3.0, 3.0) * scale,
                    rng.uniform(-3.0, 3.0) * scale,
                    if flat {
                        0.0
                    } else {
                        rng.uniform(-3.0, 3.0) * scale
                    },
                ];
                if step.iter().any(|s| s.abs() > 1e-3 * scale) {
                    for (c, s) in cur.iter_mut().zip(step) {
                        *c += s;
                    }
                    break;
                }
            }
        }
        pts.push(cur);
    }
    pts
}

fn gen_vec(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

fn gen_par(rng: &mut Rng) -> i32 {
    1 + below(rng, 3) as i32
}

/// `(zero ds, zero de)`, estimated ones only with at least four points.
fn gen_end(rng: &mut Rng, cnt: usize) -> ([f64; 3], [f64; 3]) {
    let est = cnt >= 4;
    let ds = if est && rng.chance(0.25) {
        [0.0; 3]
    } else {
        gen_vec(rng)
    };
    let de = if est && rng.chance(0.25) {
        [0.0; 3]
    } else {
        gen_vec(rng)
    };
    (ds, de)
}

/// Strictly increasing interior knots in `(lo, hi)`, clamped with `p + 1` end knots.
fn gen_clamped(rng: &mut Rng, lo: f64, hi: f64, p: usize, interior: usize) -> Vec<f64> {
    let mut v: Vec<f64> = (0..interior).map(|_| rng.uniform(lo, hi)).collect();
    v.sort_by(f64::total_cmp);
    v.dedup();
    v.retain(|x| *x > lo && *x < hi);
    let mut k = vec![lo; p + 1];
    k.extend(v);
    k.extend(vec![hi; p + 1]);
    k
}

fn gen_end_derivs(rng: &mut Rng) -> Inputs {
    let cnt = 2 + below(rng, 14);
    let kk = cnt - 1;
    let p = if rng.chance(0.05) {
        [0, 1, kk + 3][below(rng, 3)]
    } else {
        2 + below(rng, (kk + 1).min(5))
    };
    let (ds, de) = gen_end(rng, cnt);
    let mut par = gen_par(rng);
    let cur = if rng.chance(0.2) {
        let n = kk + 2;
        if rng.chance(0.15) {
            // Too small an existing curve.
            Some(CurIn {
                n: n - 1,
                p,
                knots: vec![0.0; n + p.max(1)],
            })
        } else {
            if rng.chance(0.6) {
                par = 4;
            }
            // Clamped with simple interior knots, exactly the size upstream sets.
            let knots = if (2..=n).contains(&p) {
                let ni = n - p;
                let h = 1.0 / (ni + 1) as f64;
                let mut k = vec![0.0; p + 1];
                k.extend((1..=ni).map(|i| i as f64 * h + rng.uniform(-0.4, 0.4) * h));
                k.extend(vec![1.0; p + 1]);
                k
            } else {
                // p out of range (rejected first): any sorted knots of that count.
                (0..n + p + 2).map(|i| i as f64 / (n + p + 1) as f64).collect()
            };
            Some(CurIn { n, p, knots })
        }
    } else {
        None
    };
    Inputs::EndDerivs {
        pts: gen_pts(rng, cnt),
        p,
        ds,
        de,
        par,
        cur,
    }
}

fn gen_tangents(rng: &mut Rng) -> Inputs {
    let cnt = 2 + below(rng, 14);
    let k = cnt - 1;
    let p = if rng.chance(0.05) {
        [1, k + 3][below(rng, 2)]
    } else {
        2 + below(rng, 4)
    };
    let tan = below(rng, 5) as i32;
    let der = 1 + below(rng, 2) as i32;
    let per = match below(rng, 10) {
        0 => 0.0,
        1 => 1.0,
        2 if rng.chance(0.3) => {
            if rng.chance(0.5) {
                -0.5
            } else {
                1.5
            }
        }
        _ => rng.unit(),
    };
    let (lo, hi) = if rng.chance(0.3) {
        let lo = rng.uniform(-2.0, 2.0);
        (lo, lo + rng.uniform(0.5, 3.0))
    } else {
        (0.0, 1.0)
    };
    let knots = if rng.chance(0.55) {
        let pt = 1 + below(rng, 4);
        let ni = below(rng, 2 * cnt + 2);
        let mut kv = gen_clamped(rng, lo, hi, pt, ni);
        match below(rng, 30) {
            0 => kv = vec![lo; 4],
            1 => kv.insert(0, lo),
            _ => {}
        }
        Some(kv)
    } else {
        None
    };
    let u = if rng.chance(0.3) {
        let (a, b) = if knots.is_some() { (lo, hi) } else { (0.0, 1.0) };
        let mut u: Vec<f64> = (0..cnt)
            .map(|i| a + (b - a) * (i as f64 / k as f64))
            .collect();
        u[k] = b;
        if rng.chance(0.5) && cnt > 2 {
            // Jitter the interior, keeping the order.
            let h = (b - a) / k as f64;
            for v in &mut u[1..k] {
                *v += rng.uniform(-0.3, 0.3) * h;
            }
        }
        if rng.chance(0.05) {
            u[0] -= 0.25;
        }
        Some(u)
    } else {
        None
    };
    let mut ts = gen_vec(rng);
    if rng.chance(0.03) {
        ts = [0.0; 3];
    }
    Inputs::KnotsAndTangents {
        pts: gen_pts(rng, cnt),
        u,
        tan,
        ts,
        te: gen_vec(rng),
        der,
        knots,
        per,
        p,
    }
}

fn gen_closed(rng: &mut Rng) -> Inputs {
    let cnt = 3 + below(rng, 14);
    let mut pts = gen_pts(rng, cnt - 1);
    pts.push(pts[0]);
    let n = cnt - 1;
    let mut p = 1 + below(rng, n.min(6));
    let k = if rng.chance(0.05) {
        p
    } else {
        below(rng, p.min(5))
    };
    let d = if rng.chance(0.4) {
        Some((0..k).map(|_| gen_vec(rng)).collect::<Vec<_>>())
    } else {
        // The local interpolant needs p <= nl.
        let (ni, pi) = (n as i64, p as i64);
        let nm = ni / 2;
        let nl = ni - nm.max(ni - pi - 1) + nm.min(pi + 1);
        p = p.min(nl as usize).max(1);
        None
    };
    Inputs::ClosedConditions {
        pts,
        p,
        d,
        k,
        par: gen_par(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match below(rng, 12) {
        0 | 1 => gen_end_derivs(rng),
        2 | 3 => {
            let cnt = 1 + below(rng, 15);
            let (ds, de) = gen_end(rng, cnt);
            Inputs::CubicSpline {
                pts: gen_pts(rng, cnt),
                ds,
                de,
                par: gen_par(rng),
            }
        }
        4 | 5 => {
            let cnt = 2 + below(rng, 12);
            let p = if rng.chance(0.05) {
                [1, 4][below(rng, 2)]
            } else {
                2 + below(rng, 2)
            };
            let mut d: Vec<[f64; 3]> = (0..cnt).map(|_| gen_vec(rng)).collect();
            if rng.chance(0.03) {
                d[below(rng, cnt)] = [0.0; 3];
            }
            Inputs::FirstDerivs {
                pts: gen_pts(rng, cnt),
                d,
                p,
                vec: 1 + below(rng, 2) as i32,
                par: gen_par(rng),
            }
        }
        6..=8 => gen_tangents(rng),
        9 => gen_closed(rng),
        _ => {
            let cnt = 1 + below(rng, 14);
            let p = 1 + below(rng, 7);
            let (kk, ll) = if rng.chance(0.05) {
                (p, below(rng, p))
            } else {
                (below(rng, p), below(rng, p))
            };
            Inputs::HighDerivs {
                pts: gen_pts(rng, cnt),
                p,
                ds: (0..kk).map(|_| gen_vec(rng)).collect(),
                de: (0..ll).map(|_| gen_vec(rng)).collect(),
                par: gen_par(rng),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_fit_p3"
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn op_name(i: &Inputs) -> &'static str {
        match i {
            Inputs::EndDerivs { .. } => "end_derivs",
            Inputs::CubicSpline { .. } => "cubic_spline",
            Inputs::FirstDerivs { .. } => "first_derivs",
            Inputs::KnotsAndTangents { .. } => "knots_and_tangents",
            Inputs::ClosedConditions { .. } => "closed_conditions",
            Inputs::HighDerivs { .. } => "high_derivs",
        }
    }

    /// Every op is generated with both successful and failing (status) runs.
    #[test]
    fn generator_covers_success_and_errors() {
        let mut rng = Rng::new(7);
        let mut seen: BTreeMap<(&str, i64), usize> = BTreeMap::new();
        for _ in 0..3000 {
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            *seen.entry((op_name(&i), flat[0] as i64)).or_default() += 1;
        }
        eprintln!("{seen:?}");
        for op in [
            "end_derivs",
            "cubic_spline",
            "first_derivs",
            "knots_and_tangents",
            "closed_conditions",
            "high_derivs",
        ] {
            assert!(seen.get(&(op, 0)).copied().unwrap_or(0) > 20, "{op}: few successes");
            assert!(
                seen.iter().any(|((o, s), _)| *o == op && *s > 0),
                "{op}: no upstream error status"
            );
            assert!(!seen.contains_key(&(op, -1)), "{op}: non-NLib Rust error");
        }
    }
}
