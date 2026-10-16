//! Runner for unit `nlib.spiral`: the cubic spiral approximation
//! (`N_CrvApproxSpiral`) and the NURBS volume allocate / copy helpers
//! (`sm_AllocateNurbVolume`, `sm_AllocateAndCopyNurbVolume`).
//!
//! C++: `solidcore_ffi::units::nlib_spiral` (upstream `source/SMLib/src/NL_Spiral.cpp`,
//! `source/SMLib/src/SmNurbsVol.cpp`). Rust: `solidcore::nurbs::spiral`.
//!
//! Each case runs one *op* (`inputs.op`). Both sides produce the same flat record
//! (see the ffi module), split into keys. `spiral`: `status` (`0` ok, else the
//! `NL_*_ERR` number; `-1` on the Rust side for a non-NLib error, which never
//! matches), then on success `num_points`, `tol`, `dims = [n, p, m]`, `cpts`
//! (x y z w) and `knots`. `volume`: for the fresh allocation and for the copy,
//! `<v>_dims = [l, m, n, p, q, r, ku, kv, kw]`, `<v>_knots` and `<v>_cpts`.
//!
//! Upstream crashes on the input errors it detects before initialising its local
//! stacks (`plan/findings.md`), so those inputs are neither generated nor in the
//! fixtures; the Rust unit tests cover them.

// Harness code: the record parsers index slices they have length-checked; a panic
// would fail the case (runner panics are caught).
#![allow(clippy::indexing_slicing)]

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::spiral::{self as sp, Hand, SpiralSpec, Volume};
use solidcore::{Result as SolidResult, SolidError};
use solidcore_ffi::units::nlib_spiral as ffi;
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.spiral` runner.
pub struct Runner;

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Spiral {
        height: f64,
        rad0: f64,
        rad1: f64,
        turns: f64,
        rorl: i64,
        num_points: i64,
        tol: f64,
    },
    Volume {
        cp_hi: [usize; 3],
        degree: [usize; 3],
        knot_hi: [usize; 3],
        knots: Vec<f64>,
        cpts: Vec<[f64; 4]>,
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

fn ints(v: &[f64]) -> Vec<i64> {
    v.iter().map(|x| *x as i64).collect()
}

fn volume_keys(
    mut out: Output,
    tag: &str,
    flat: &[f64],
    at: &mut usize,
) -> Result<Output, HarnessError> {
    let d = take(flat, at, 9)?;
    let cp: usize = d[..3]
        .iter()
        .map(|x| count(*x).map(|c| c + 1))
        .product::<Result<usize, _>>()?;
    let kn: usize = d[6..]
        .iter()
        .map(|x| count(*x).map(|c| c + 1))
        .sum::<Result<usize, _>>()?;
    out = out.ints(&format!("{tag}_dims"), ints(d));
    out = out.values(&format!("{tag}_knots"), take(flat, at, kn)?.to_vec());
    out = out.values(&format!("{tag}_cpts"), take(flat, at, 4 * cp)?.to_vec());
    Ok(out)
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0;
    let mut out = Output::new();
    match i {
        Inputs::Spiral { .. } => {
            let st = take(flat, &mut at, 1)?[0];
            out = out.int("status", st as i64);
            if st == 0.0 {
                out = out.int("num_points", take(flat, &mut at, 1)?[0] as i64);
                out = out.scalar("tol", take(flat, &mut at, 1)?[0]);
                let d = take(flat, &mut at, 3)?;
                let (n, m) = (count(d[0])?, count(d[2])?);
                out = out.ints("dims", ints(d));
                out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
                out = out.values("knots", take(flat, &mut at, m + 1)?.to_vec());
            }
        }
        Inputs::Volume { .. } => {
            out = volume_keys(out, "fresh", flat, &mut at)?;
            out = volume_keys(out, "copy", flat, &mut at)?;
        }
    }
    if at == flat.len() {
        Ok(out)
    } else {
        Err(harness(format!(
            "record has {} extra values",
            flat.len() - at
        )))
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn cpp_flat(i: &Inputs) -> Result<Vec<f64>, FfiError> {
    match i {
        Inputs::Spiral {
            height,
            rad0,
            rad1,
            turns,
            rorl,
            num_points,
            tol,
        } => ffi::approx_spiral(*height, *rad0, *rad1, *turns, *rorl, *num_points, *tol),
        Inputs::Volume {
            cp_hi,
            degree,
            knot_hi,
            knots,
            cpts,
        } => ffi::volume_copy(*cp_hi, *degree, *knot_hi, knots, cpts),
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

fn push_volume(o: &mut Vec<f64>, v: &Volume) {
    o.extend(v.cp_hi().iter().map(|&x| x as f64));
    o.extend(v.degrees().iter().map(|&x| x as f64));
    o.extend((0..3).map(|d| v.knots(d).len().saturating_sub(1) as f64));
    for d in 0..3 {
        o.extend_from_slice(v.knots(d));
    }
    for c in v.cpts() {
        o.extend_from_slice(&c.to_array());
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    match i {
        Inputs::Spiral {
            height,
            rad0,
            rad1,
            turns,
            rorl,
            num_points,
            tol,
        } => {
            let r = sp::approx_spiral(&SpiralSpec {
                height: *height,
                rad0: *rad0,
                rad1: *rad1,
                num_turns: *turns,
                hand: Hand::from_flag(*rorl),
                num_points: *num_points,
                tol: *tol,
            });
            let mut o = vec![status(&r)];
            if let Ok(s) = &r {
                o.push(s.num_points as f64);
                o.push(s.tol);
                let c = &s.curve;
                let (n, m) = c.array_sizes();
                o.extend([n as f64, c.degree() as f64, m as f64]);
                for pw in c.cpts() {
                    o.extend_from_slice(&pw.to_array());
                }
                o.extend_from_slice(c.knots());
            }
            Ok(o)
        }
        Inputs::Volume {
            cp_hi,
            degree,
            knot_hi,
            knots,
            cpts,
        } => {
            let mut v = sp::allocate_nurb_volume(*cp_hi, *degree, *knot_hi)
                .map_err(|e| harness(format!("allocate: {e}")))?;
            let mut o = Vec::new();
            push_volume(&mut o, &v);
            let mut src = knots.iter();
            for d in 0..3 {
                for k in v.knots_mut(d) {
                    *k = *src.next().ok_or_else(|| harness("too few knots"))?;
                }
            }
            if v.cpts().len() != cpts.len() {
                return Err(harness("wrong control point count"));
            }
            for (c, x) in v.cpts_mut().iter_mut().zip(cpts) {
                *c = solidcore::nurbs::math::CPoint::new(x[0], x[1], x[2], x[3]);
            }
            let c = sp::copy_nurb_volume(&v);
            push_volume(&mut o, &c);
            Ok(o)
        }
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    to_output(i, &rust_flat(i)?)
}

// ---- generator --------------------------------------------------------------------

fn log_uniform(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    (lo.ln() + (hi.ln() - lo.ln()) * rng.unit()).exp()
}

fn gen_radius(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 0.0,
        1 => *rng
            .choose(&[5e-8, -5e-8, 1e-7, -1e-7, 2e-7])
            .unwrap_or(&0.0),
        2 => log_uniform(rng, 1e-7, 1e-5),
        _ => log_uniform(rng, 1e-4, 1e3),
    }
}

fn gen_spiral(rng: &mut Rng) -> Inputs {
    // Upstream crashes on its early input errors (see the ffi module): redraw.
    loop {
        let i = gen_spiral_any(rng);
        if let Inputs::Spiral {
            height,
            rad0,
            rad1,
            turns,
            ..
        } = i
        {
            if !ffi::hits_early_input_error(height, rad0, rad1, turns) {
                return i;
            }
        }
    }
}

fn gen_spiral_any(rng: &mut Rng) -> Inputs {
    let height = match rng.below(20) {
        0 => *rng.choose(&[0.0, -1.0, 5e-8, 1e-7]).unwrap_or(&0.0),
        _ => log_uniform(rng, 1e-4, 1e4),
    };
    let turns = match rng.below(20) {
        0 => *rng
            .choose(&[0.0, -1.0, 5e-8, 1e-7, 1e-4, 0.00125, 124.25, 125.0])
            .unwrap_or(&1.0),
        1 => log_uniform(rng, 1e-7, 0.00124), // zero spans: fit input error
        2 | 3 => log_uniform(rng, 1e-3, 150.0),
        _ => log_uniform(rng, 0.05, 20.0),
    };
    let num_points = match rng.below(10) {
        0 => rng.range_i64(-3, 2),
        1 | 2 => rng.range_i64(2, 300),
        _ => 0,
    };
    let tol = match rng.below(10) {
        0 => *rng.choose(&[0.0, -1.0, 1e-9, 1e-6, 1e3]).unwrap_or(&0.0),
        _ => log_uniform(rng, 1e-8, 1.0),
    };
    Inputs::Spiral {
        height,
        rad0: gen_radius(rng),
        rad1: gen_radius(rng),
        turns,
        rorl: *rng.choose(&[1, 1, 1, 0, 0, -1, 2]).unwrap_or(&1),
        num_points,
        tol,
    }
}

fn gen_volume(rng: &mut Rng) -> Inputs {
    let cp_hi = [0, 0, 0].map(|_: usize| rng.below(5) as usize);
    let degree = [0, 0, 0].map(|_: usize| rng.below(4) as usize);
    let knot_hi = [0, 0, 0].map(|_: usize| rng.below(9) as usize);
    let n_knots: usize = knot_hi.iter().map(|k| k + 1).sum();
    let n_cpts: usize = cp_hi.iter().map(|k| k + 1).product();
    let knots = (0..n_knots).map(|_| rng.uniform(-5.0, 5.0)).collect();
    let cpts = (0..n_cpts)
        .map(|_| {
            let p = rng.point3(-100.0, 100.0);
            [p[0], p[1], p[2], rng.uniform(0.1, 3.0)]
        })
        .collect();
    Inputs::Volume {
        cp_hi,
        degree,
        knot_hi,
        knots,
        cpts,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.1) {
        gen_volume(rng)
    } else {
        gen_spiral(rng)
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.spiral"
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

    /// The generator covers both ops, successes and every input error.
    #[test]
    fn generator_covers_success_and_errors() {
        let mut rng = Rng::new(11);
        let (mut ok, mut err, mut vol) = (0, 0, 0);
        for _ in 0..500 {
            let i = gen_case(&mut rng);
            let flat = rust_flat(&i).unwrap();
            match i {
                Inputs::Volume { .. } => vol += 1,
                Inputs::Spiral { .. } if flat[0] == 0.0 => ok += 1,
                Inputs::Spiral { .. } => err += 1,
            }
        }
        assert!(ok > 100 && err > 10 && vol > 20, "{ok} {err} {vol}");
    }
}
