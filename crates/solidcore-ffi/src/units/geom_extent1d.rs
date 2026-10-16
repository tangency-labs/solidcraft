//! FFI wrappers for unit `geom.extent1d`.
//!
//! Shim: `harness/cpp/shim/geom_extent1d.cpp`.
//! Upstream: `source/SMLib/src/SmExtent1d.cpp` (`SmExtent1d`),
//! `source/SMLib/src/SmExtentNd.cpp` (`SmExtentNd`).
//!
//! | Rust   | shim                  | upstream                                       |
//! |--------|-----------------------|------------------------------------------------|
//! | [`op`] | `sc_geom_extent1d_op` | `SmExtent1d` / `SmExtentNd` methods, per [`Op`] |
//! | [`accepts`] | (input validation only) | |
//!
//! Every operation takes a flat `f64` input (intervals are `[min, max]`, stored as
//! given) and returns a flat `f64` output; the layouts are documented on [`Op`] and
//! in the shim.

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_extent1d_op(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// Operation selector of `sc_geom_extent1d_op` (`E`, `F`: intervals of 2 values).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmExtent1d(a, b)`, `SmExtent1d(a)`, `SmExtent1d()`: in `a, b` -> 3 intervals.
    Constructors = 0,
    /// Accessors and value queries: in `E, t, tol` -> 11 floats, 13 integers.
    Queries = 1,
    /// One modifier (`which` 0..=10, see the shim): in `E, which, x, y` -> status, E.
    Modify = 2,
    /// `Union`, `Intersect`, `AreDisjoint`, `AreEqual` (both), `IsContainedBy`,
    /// `operator==`: in `E, F, tol` -> union, intersection, 7 integers.
    Binary = 3,
    /// `ApproximateUnbounded` with and without the expanded extent: in
    /// `E, center, half` -> approx, expanded (`Init` if rejected), approx.
    ApproximateUnbounded = 4,
    /// `PeriodicWrap`, `ContainsPeriodicValue`, `ClampPeriodicValue`,
    /// `IsValueOnPeriodicBoundary`, `IsClosed`: in `E, v, period, tol` -> 6 floats,
    /// 4 integers.
    PeriodicValue = 5,
    /// `IntersectPeriodic`: in `E, F, period, dont_cross` -> status, count, intervals.
    IntersectPeriodic = 6,
    /// `AreDisjointPeriodic`: in `E, F, period, tol` -> flag.
    AreDisjointPeriodic = 7,
    /// `SmExtentNd::ClampVector`: in `dim, has_periodic, has_counts, n_prior_clamped,
    /// n_prior_counts`, then `dim` intervals, `dim` values, `dim` periodic flags (if
    /// any), the prior clamped values, the prior counts -> flag, clamped, counts.
    ClampVector = 8,
}

impl Op {
    /// Number of input values (`None`: variable, `ClampVector`).
    #[must_use]
    pub fn input_size(self) -> Option<usize> {
        Some(match self {
            Op::Constructors => 2,
            Op::Queries | Op::ApproximateUnbounded => 4,
            Op::Modify | Op::Binary | Op::PeriodicValue => 5,
            Op::IntersectPeriodic | Op::AreDisjointPeriodic => 6,
            Op::ClampVector => return None,
        })
    }
}

/// Largest number of period steps a `PeriodicWrap` call may take in the C++ (it
/// loops one period at a time).
const MAX_WRAP_STEPS: f64 = 1e7;

/// True when upstream `PeriodicWrap(v)` on `[lo, hi]` (stored as given) finishes
/// within [`MAX_WRAP_STEPS`] steps.
fn wrap_bounded(lo: f64, hi: f64, v: f64) -> bool {
    let p = hi - lo;
    if p.is_nan() || p <= 1.0e-12 || v + p == v || v.is_nan() {
        return true;
    }
    ((v - lo).abs() / p) <= MAX_WRAP_STEPS && ((v - hi).abs() / p) <= MAX_WRAP_STEPS
}

/// `PeriodicWrap` bound of `ContainsPeriodicValue(v, period)` on `[lo, hi]`: it wraps
/// on `SmExtent1d(lo, lo + period)` (swapped if inverted).
fn periodic_bounded(lo: f64, period: f64, v: f64) -> bool {
    let hi = lo + period;
    if lo > hi {
        wrap_bounded(hi, lo, v)
    } else {
        wrap_bounded(lo, hi, v)
    }
}

fn get(input: &[f64], i: usize) -> f64 {
    input.get(i).copied().unwrap_or(f64::NAN)
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let bad = |m: String| Err(FfiError::InvalidArgument(m));
    if let Some(n) = op.input_size() {
        if input.len() != n {
            return bad(format!("{op:?}: {} inputs, expected {n}", input.len()));
        }
    }
    let g = |i| get(input, i);
    match op {
        Op::Modify => {
            let w = g(2);
            if !(0.0..=10.0).contains(&w) || w.fract() != 0.0 {
                return bad(format!("Modify: which = {w}"));
            }
        }
        Op::PeriodicValue => {
            let (lo, hi, v, period) = (g(0), g(1), g(2), g(3));
            if !wrap_bounded(lo, hi, v) || !periodic_bounded(lo, period, v) {
                return bad("PeriodicValue: PeriodicWrap would loop too long".into());
            }
        }
        Op::IntersectPeriodic | Op::AreDisjointPeriodic => {
            let (e0, e1, f0, f1, period) = (g(0), g(1), g(2), g(3), g(4));
            let ok = [f0, f1].iter().all(|&v| periodic_bounded(e0, period, v))
                && [e0, e1].iter().all(|&v| periodic_bounded(f0, period, v));
            if !ok {
                return bad(format!("{op:?}: PeriodicWrap would loop too long"));
            }
        }
        Op::ClampVector => validate_clamp(input)?,
        _ => {}
    }
    Ok(())
}

fn count(v: f64, what: &str) -> Result<usize, FfiError> {
    if v >= 0.0 && v.fract() == 0.0 && v <= 1e6 {
        Ok(v as usize)
    } else {
        Err(FfiError::InvalidArgument(format!(
            "ClampVector: {what} = {v}"
        )))
    }
}

fn validate_clamp(input: &[f64]) -> Result<(), FfiError> {
    let bad = |m: &str| Err(FfiError::InvalidArgument(format!("ClampVector: {m}")));
    if input.len() < 5 {
        return bad("short header");
    }
    let dim = count(get(input, 0), "dim")?;
    let has_per = get(input, 1) != 0.0;
    let has_cnt = get(input, 2) != 0.0;
    let n_pc = count(get(input, 3), "prior clamped")?;
    let n_pn = count(get(input, 4), "prior counts")?;
    if !has_cnt && n_pn != 0 {
        return bad("prior counts without counts");
    }
    let total = 5 + 3 * dim + if has_per { dim } else { 0 } + n_pc + n_pn;
    if input.len() != total {
        return bad("length mismatch");
    }
    let ext = |i: usize| (get(input, 5 + 2 * i), get(input, 6 + 2 * i));
    let val = |i: usize| get(input, 5 + 2 * dim + i);
    let per = |i: usize| has_per && get(input, 5 + 3 * dim + i) != 0.0;
    for i in 0..dim {
        let (lo, hi) = ext(i);
        if per(i) && !wrap_bounded(lo, hi, val(i)) {
            return bad("PeriodicWrap would loop too long");
        }
    }
    let counts_start = 5 + 3 * dim + if has_per { dim } else { 0 } + n_pc;
    for i in 0..n_pn {
        let c = get(input, counts_start + i);
        let ok = c >= 0.0 && c.fract() == 0.0 && c <= 1e15;
        if !ok {
            return bad("prior count not a small non-negative integer");
        }
    }
    Ok(())
}

/// True when [`op`] would accept `input` (sizes, `PeriodicWrap` loop bounds).
#[must_use]
pub fn accepts(op: Op, input: &[f64]) -> bool {
    validate(op, input).is_ok()
}

/// Runs `op` on `input` in upstream C++ and returns its flat output.
pub fn op(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    let n_in = i64::try_from(input.len()).map_err(|_| FfiError::InvalidArgument("input".into()))?;
    call_with_buffer(64, |buf: &mut [f64], len| {
        // SAFETY: `input` is valid for `n_in` reads (the shim reads at most its op's
        // input size, validated above and again in the shim); `buf` is valid for
        // `buf.len()` writes; `len` is a valid writable i64.
        unsafe {
            sys::sc_geom_extent1d_op(
                op as i32,
                input.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
