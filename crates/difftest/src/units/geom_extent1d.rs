//! Runner for unit `geom.extent1d`: 1D intervals and N-dimensional interval boxes.
//!
//! C++: `solidcore_ffi::units::geom_extent1d` (upstream `source/SMLib/src/SmExtent1d.cpp`,
//! `source/SMLib/src/SmExtentNd.cpp`). Rust: `solidcore::geom::extent1d`.
//!
//! | op (`inputs.op`)        | Rust                                           | upstream |
//! |-------------------------|------------------------------------------------|----------|
//! | `constructors`          | `from_min_max`, `from_value`, `new`            | `SmExtent1d(a, b)`, `SmExtent1d(a)`, `SmExtent1d()` |
//! | `queries`               | accessors, `clamp_value`, `snap_value`, `evaluate`, `distance_from`, `is_value_on_boundary`, `inversion`, `t_left_eval`, `is_*`, `boundary_type`, `contains_value`, `classify_point` | `Get*`, `ClampValue`, `SnapValue`, `Evaluate`, `DistanceFrom`, `IsValueOnBoundary`, `Inversion`, `GetTLeftEval`, `AssertDefined`, `IsInit`, `HasNegativeLength`, `IsDegenerate`, `IsBounded`, `ContainsValue`, `ClassifyPoint` |
//! | `modify`                | `add_value`, `set_min_max`, `set_min`, `set_max`, `set_unbounded`, `negate`, `translate`, `expand_absolute`, `expand_relative`, `scale`, `init` | `AddValue`, `SetMinMax`, `SetMin`, `SetMax`, `SetUnbounded`, `Negate`, `Translate`, `ExpandAbsolute`, `ExpandRelative`, `Scale`, `Init` |
//! | `binary`                | `union`, `intersect`, `are_disjoint`, `are_equal`, `are_equal_bounds`, `is_contained_by`, `are_same` | `Union`, `Intersect`, `AreDisjoint`, `AreEqual` (both), `IsContainedBy`, `operator==` |
//! | `approximate_unbounded` | `approximate_unbounded_expanded`, `approximate_unbounded` | `ApproximateUnbounded` |
//! | `periodic_value`        | `periodic_wrap`, `contains_periodic_value`, `clamp_periodic_value`, `is_value_on_periodic_boundary`, `is_closed` | `PeriodicWrap`, `ContainsPeriodicValue`, `ClampPeriodicValue`, `IsValueOnPeriodicBoundary`, `IsClosed` |
//! | `intersect_periodic`    | `intersect_periodic`                           | `IntersectPeriodic` |
//! | `disjoint_periodic`     | `are_disjoint_periodic`                        | `AreDisjointPeriodic` |
//! | `clamp_vector`          | `ExtentNd::new`, `get_mut`, `clamp_vector`     | `SmExtentNd(ULONG)`, `operator[]`, `ClampVector` |
//!
//! Upstream statuses are compared as the int `status` (0 = `SM_SUCCESS`, else the
//! `SM_ERR_*` value; Rust `InvalidInput` is 1007, `Upstream` its code). A failed
//! `Intersect` reports upstream's `Init()` result interval. `Inversion`'s undefined
//! parameter is `SM_UNDEF_DOUBLE`. Every run has at least [`MIN_GENERATED`] generated
//! cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::{Extent1d, ExtentNd};
use solidcore::math::{
    BIG_DOUBLE, EFF_ZERO, EFF_ZERO_DEG, INFINITE_PARAMETER, INFINITE_PARAMETER_SQRT, TWO_PI,
    UNDEF_DOUBLE,
};
use solidcore::SolidError;
use solidcore_ffi::units::geom_extent1d::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.extent1d` runner.
pub struct Runner;

type E2 = [f64; 2];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    Constructors {
        a: f64,
        b: f64,
    },
    Queries {
        e: E2,
        t: f64,
        tol: f64,
    },
    Modify {
        e: E2,
        which: u8,
        x: f64,
        #[serde(default)]
        y: f64,
    },
    Binary {
        e: E2,
        f: E2,
        tol: f64,
    },
    ApproximateUnbounded {
        e: E2,
        center: f64,
        half: f64,
    },
    PeriodicValue {
        e: E2,
        v: f64,
        period: f64,
        tol: f64,
    },
    IntersectPeriodic {
        e: E2,
        f: E2,
        period: f64,
        dont_cross: bool,
    },
    DisjointPeriodic {
        e: E2,
        f: E2,
        period: f64,
        tol: f64,
    },
    ClampVector {
        exts: Vec<E2>,
        values: Vec<f64>,
        #[serde(default)]
        periodic: Option<Vec<bool>>,
        #[serde(default)]
        prior_clamped: Vec<f64>,
        #[serde(default)]
        prior_counts: Option<Vec<u64>>,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

fn ext(e: E2) -> Extent1d {
    Extent1d::from_raw(e[0], e[1])
}

fn e2(e: &Extent1d) -> E2 {
    [e.min(), e.max()]
}

/// Status value of a Rust error (upstream `SmStatus`).
fn status_of(e: &SolidError) -> Result<f64, HarnessError> {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => Ok(1007.0),
        SolidError::Upstream { code, .. } => Ok(f64::from(*code)),
        other => Err(harness(format!("unexpected rust error: {other}"))),
    }
}

fn status<T>(r: &solidcore::Result<T>) -> Result<f64, HarnessError> {
    match r {
        Ok(_) => Ok(0.0),
        Err(e) => status_of(e),
    }
}

// ---- flat layouts ------------------------------------------------------------------

/// Takes `n` values off the front of `rest`.
fn take<'a>(rest: &mut &'a [f64], n: usize, flat: &[f64]) -> Result<&'a [f64], HarnessError> {
    let Some((head, tail)) = rest.split_at_checked(n) else {
        return Err(harness(format!("result too short: {flat:?}")));
    };
    *rest = tail;
    Ok(head)
}

fn ints(v: &[f64]) -> Vec<i64> {
    v.iter().map(|&x| x as i64).collect()
}

/// Splits a flat result (same layout on both sides) into keyed outputs.
fn split(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut rest = flat;
    let r = &mut rest;
    let out = match i {
        Inputs::Constructors { .. } => Output::new().values("extents", take(r, 6, flat)?.to_vec()),
        Inputs::Queries { .. } => Output::new()
            .values("values", take(r, 11, flat)?.to_vec())
            .ints("flags", ints(take(r, 13, flat)?)),
        Inputs::Modify { .. } => Output::new()
            .ints("status", ints(take(r, 1, flat)?))
            .values("extent", take(r, 2, flat)?.to_vec()),
        Inputs::Binary { .. } => Output::new()
            .values("union", take(r, 2, flat)?.to_vec())
            .values("intersect", take(r, 2, flat)?.to_vec())
            .ints("flags", ints(take(r, 7, flat)?)),
        Inputs::ApproximateUnbounded { .. } => Output::new()
            .values("approx", take(r, 2, flat)?.to_vec())
            .values("expanded", take(r, 2, flat)?.to_vec())
            .values("approx_plain", take(r, 2, flat)?.to_vec()),
        Inputs::PeriodicValue { .. } => Output::new()
            .values("values", take(r, 6, flat)?.to_vec())
            .ints("flags", ints(take(r, 4, flat)?)),
        Inputs::IntersectPeriodic { .. } => {
            let head = ints(take(r, 2, flat)?);
            let n = usize::try_from(head.get(1).copied().unwrap_or(0))
                .map_err(|_| harness("negative count"))?;
            Output::new()
                .ints("status_count", head)
                .values("intervals", take(r, 2 * n, flat)?.to_vec())
        }
        Inputs::DisjointPeriodic { .. } => Output::new().ints("disjoint", ints(take(r, 1, flat)?)),
        Inputs::ClampVector {
            exts, prior_counts, ..
        } => {
            let o = Output::new()
                .ints("flag", ints(take(r, 1, flat)?))
                .values("clamped", take(r, exts.len(), flat)?.to_vec());
            if prior_counts.is_some() {
                o.ints("counts", ints(take(r, exts.len(), flat)?))
            } else {
                o
            }
        }
    };
    if !rest.is_empty() {
        return Err(harness(format!("result too long: {flat:?}")));
    }
    Ok(out)
}

/// Flat input for the C++ op.
fn flat_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut v = Vec::new();
    let op = match i {
        Inputs::Constructors { a, b } => {
            v.extend([*a, *b]);
            Op::Constructors
        }
        Inputs::Queries { e, t, tol } => {
            v.extend(e);
            v.extend([*t, *tol]);
            Op::Queries
        }
        Inputs::Modify { e, which, x, y } => {
            v.extend(e);
            v.extend([f64::from(*which), *x, *y]);
            Op::Modify
        }
        Inputs::Binary { e, f, tol } => {
            v.extend(e);
            v.extend(f);
            v.push(*tol);
            Op::Binary
        }
        Inputs::ApproximateUnbounded { e, center, half } => {
            v.extend(e);
            v.extend([*center, *half]);
            Op::ApproximateUnbounded
        }
        Inputs::PeriodicValue {
            e,
            v: x,
            period,
            tol,
        } => {
            v.extend(e);
            v.extend([*x, *period, *tol]);
            Op::PeriodicValue
        }
        Inputs::IntersectPeriodic {
            e,
            f,
            period,
            dont_cross,
        } => {
            v.extend(e);
            v.extend(f);
            v.extend([*period, b(*dont_cross)]);
            Op::IntersectPeriodic
        }
        Inputs::DisjointPeriodic { e, f, period, tol } => {
            v.extend(e);
            v.extend(f);
            v.extend([*period, *tol]);
            Op::AreDisjointPeriodic
        }
        Inputs::ClampVector {
            exts,
            values,
            periodic,
            prior_clamped,
            prior_counts,
        } => {
            v.extend([
                exts.len() as f64,
                b(periodic.is_some()),
                b(prior_counts.is_some()),
                prior_clamped.len() as f64,
                prior_counts.as_ref().map_or(0, Vec::len) as f64,
            ]);
            v.extend(exts.iter().flatten());
            v.extend(values);
            if let Some(p) = periodic {
                v.extend(p.iter().map(|&x| b(x)));
            }
            v.extend(prior_clamped);
            if let Some(c) = prior_counts {
                v.extend(c.iter().map(|&x| x as f64));
            }
            Op::ClampVector
        }
    };
    (op, v)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = flat_input(i);
    match ffi::op(op, &input) {
        Ok(flat) => split(i, &flat),
        Err(e @ FfiError::Status(_)) => Err(harness(format!("shim failure: {e}"))),
        Err(FfiError::InvalidArgument(m)) => Err(harness(format!("ffi rejected: {m}"))),
    }
}

fn rust_flat(i: &Inputs) -> Result<Vec<f64>, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::Constructors { a, b } => {
            o.extend(e2(&Extent1d::from_min_max(*a, *b)));
            o.extend(e2(&Extent1d::from_value(*a)));
            o.extend(e2(&Extent1d::new()));
        }
        Inputs::Queries { e, t, tol } => {
            let x = ext(*e);
            let (t, tol) = (*t, *tol);
            let (on, other) = x.is_value_on_boundary(t, tol);
            let inv = x.inversion(t);
            o.extend([
                x.min(),
                x.max(),
                x.mid(),
                x.length(),
                x.max_dimension(),
                x.clamp_value(t),
                x.snap_value(t, tol),
                x.evaluate(t),
                x.distance_from(t),
                other,
                inv.parameter.unwrap_or(UNDEF_DOUBLE),
                b(x.t_left_eval(t)),
                b(x.is_defined()),
                b(x.is_init()),
                b(x.has_negative_length()),
                b(x.is_degenerate(tol)),
                b(x.is_bounded()),
                f64::from(x.boundary_type().code()),
                b(x.is_bounded()),
                b(x.contains_value(t, tol)),
                f64::from(x.classify_point(t, tol).code()),
                b(on),
                b(on),
                if inv.is_success() { 0.0 } else { 1001.0 },
            ]);
        }
        Inputs::Modify { e, which, x, y } => {
            let mut z = ext(*e);
            let (x, y) = (*x, *y);
            let r: solidcore::Result<()> = match which {
                0 => {
                    z.add_value(x);
                    Ok(())
                }
                1 => z.set_min_max(x, y),
                2 => z.set_min(x),
                3 => z.set_max(x),
                4 => {
                    z.set_unbounded();
                    Ok(())
                }
                5 => {
                    z.negate();
                    Ok(())
                }
                6 => {
                    z.translate(x);
                    Ok(())
                }
                7 => {
                    z.expand_absolute(x);
                    Ok(())
                }
                8 => {
                    z.expand_relative(x);
                    Ok(())
                }
                9 => {
                    z.scale(x);
                    Ok(())
                }
                10 => {
                    z.init();
                    Ok(())
                }
                w => return Err(harness(format!("which = {w}"))),
            };
            o.push(status(&r)?);
            o.extend(e2(&z));
        }
        Inputs::Binary { e, f, tol } => {
            let (x, y, tol) = (ext(*e), ext(*f), *tol);
            o.extend(e2(&x.union(&y)));
            let r = x.intersect(&y);
            o.extend(e2(r.as_ref().unwrap_or(&Extent1d::new())));
            o.extend([
                status(&r)?,
                b(x.are_disjoint(&y, tol)),
                b(x.are_disjoint(&y, 0.0)),
                b(x.are_equal(&y, tol)),
                b(x.are_equal_bounds(y.min(), y.max(), tol)),
                b(x.is_contained_by(&y, tol)),
                b(x.are_same(&y)),
            ]);
        }
        Inputs::ApproximateUnbounded { e, center, half } => {
            let x = ext(*e);
            let (a, xp) = x.approximate_unbounded_expanded(*center, *half);
            o.extend(e2(&a));
            o.extend(e2(&xp.unwrap_or_default()));
            o.extend(e2(&x.approximate_unbounded(*center, *half)));
        }
        Inputs::PeriodicValue { e, v, period, tol } => {
            let x = ext(*e);
            let (v, period, tol) = (*v, *period, *tol);
            let c = x.contains_periodic_value(v, period, tol);
            let (on, dist) = x.is_value_on_periodic_boundary(v, period, tol);
            o.extend([
                x.periodic_wrap(v),
                c.periodic_value,
                c.dist_to_min,
                c.dist_to_max,
                x.clamp_periodic_value(v, period, tol),
                dist,
                b(c.contained),
                b(c.contained),
                b(on),
                b(x.is_closed(period)),
            ]);
        }
        Inputs::IntersectPeriodic {
            e,
            f,
            period,
            dont_cross,
        } => {
            let r = ext(*e).intersect_periodic(&ext(*f), *period, *dont_cross);
            o.push(status(&r)?);
            let list = match r {
                Ok(l) => l,
                Err(_) => vec![Extent1d::new()],
            };
            o.push(list.len() as f64);
            for x in &list {
                o.extend(e2(x));
            }
        }
        Inputs::DisjointPeriodic { e, f, period, tol } => {
            o.push(b(ext(*e).are_disjoint_periodic(&ext(*f), *period, *tol)));
        }
        Inputs::ClampVector {
            exts,
            values,
            periodic,
            prior_clamped,
            prior_counts,
        } => {
            let mut nd = ExtentNd::new(exts.len());
            for (k, x) in exts.iter().enumerate() {
                if let Some(slot) = nd.get_mut(k) {
                    *slot = ext(*x);
                }
            }
            let mut clamped = prior_clamped.clone();
            let mut counts = prior_counts.clone();
            let r = nd
                .clamp_vector(values, periodic.as_deref(), &mut clamped, counts.as_mut())
                .map_err(|e| harness(format!("clamp_vector: {e}")))?;
            o.push(b(r));
            o.extend(&clamped);
            if let Some(c) = counts {
                o.extend(c.iter().map(|&x| x as f64));
            }
        }
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    split(i, &rust_flat(i)?)
}

// ---- generator -------------------------------------------------------------------

/// Upstream's unbounded markers.
const SENTINELS: [f64; 4] = [
    INFINITE_PARAMETER,
    -INFINITE_PARAMETER,
    INFINITE_PARAMETER_SQRT,
    -INFINITE_PARAMETER_SQRT,
];

/// Offsets that straddle upstream's tolerances.
const NUDGES: [f64; 10] = [
    1e-13, 1e-12, 2e-12, 1e-9, 1e-6, 9.9e-5, 1e-4, 1.01e-4, 0.5, 0.0,
];

/// A value: mostly ordinary, sometimes integral (exact ties), tiny, or huge.
fn value(rng: &mut Rng, scale: f64) -> f64 {
    let r = rng.unit();
    if r < 0.55 {
        rng.uniform(-scale, scale)
    } else if r < 0.85 {
        rng.range_i64(-6, 6) as f64
    } else if r < 0.92 {
        rng.uniform(-1e-9, 1e-9)
    } else {
        *rng.choose(&[0.0, -0.0, 360.0, 90.0, 1e6, -1e6])
            .unwrap_or(&0.0)
    }
}

/// An interval: usually ordered, sometimes inverted, degenerate, `Init`, tiny, or
/// with unbounded ends.
fn interval(rng: &mut Rng, scale: f64) -> E2 {
    let r = rng.unit();
    if r < 0.04 {
        return [BIG_DOUBLE, -BIG_DOUBLE];
    }
    let a = value(rng, scale);
    if r < 0.1 {
        return [a, a];
    }
    if r < 0.15 {
        let d = *rng.choose(&NUDGES).unwrap_or(&0.0);
        return [a, a + d];
    }
    let mut c = value(rng, scale);
    let (mut lo, mut hi) = (a, c);
    if r < 0.9 && lo > hi {
        core::mem::swap(&mut lo, &mut hi);
    }
    if rng.chance(0.15) {
        if rng.chance(0.5) {
            lo = -INFINITE_PARAMETER;
        }
        if rng.chance(0.5) {
            hi = INFINITE_PARAMETER;
        }
        if rng.chance(0.2) {
            c = *rng.choose(&SENTINELS).unwrap_or(&INFINITE_PARAMETER);
            if rng.chance(0.5) {
                lo = c;
            } else {
                hi = c;
            }
        }
    }
    [lo, hi]
}

/// A value related to `e`: an end, an interior point, slightly off an end, or random.
fn value_near(rng: &mut Rng, e: &E2, scale: f64) -> f64 {
    let [lo, hi] = *e;
    let nudge = |rng: &mut Rng| {
        let d = *rng.choose(&NUDGES).unwrap_or(&0.0);
        if rng.chance(0.5) {
            d
        } else {
            -d
        }
    };
    match rng.below(7) {
        0 => value(rng, scale),
        1 => lo,
        2 => hi,
        3 => lo + rng.unit() * (hi - lo),
        4 => lo + nudge(rng),
        5 => hi + nudge(rng),
        _ => (lo + hi) / 2.0 + nudge(rng),
    }
}

fn tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[
        0.0,
        0.0,
        EFF_ZERO,
        1e-9,
        1e-6,
        EFF_ZERO_DEG,
        0.01,
        0.5,
        -1e-6,
        -1.0,
    ])
    .unwrap_or(&0.0)
}

/// A period: a common domain length, `e`'s length (closed), nearly it, 0 or random.
fn period(rng: &mut Rng, e: &E2, scale: f64) -> f64 {
    let len = e[1] - e[0];
    match rng.below(8) {
        0 => 0.0,
        1 => *rng
            .choose(&[360.0, TWO_PI, 1.0, 2.0, 4.0, 5.0, 10.0])
            .unwrap_or(&1.0),
        2 | 3 => len,
        4 => len + *rng.choose(&NUDGES).unwrap_or(&0.0),
        5 => len * rng.uniform(1.0, 3.0),
        6 => -len,
        _ => rng.uniform(0.0, 2.0 * scale),
    }
}

/// A value in a periodic setting: near `e` shifted by a few periods.
fn periodic_value(rng: &mut Rng, e: &E2, p: f64, scale: f64) -> f64 {
    let base = value_near(rng, e, scale);
    if rng.chance(0.6) && p.is_finite() {
        base + rng.range_i64(-4, 4) as f64 * p
    } else {
        base
    }
}

fn gen_once(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 10.0, 360.0, 1e-3, 1e4]).unwrap_or(&1.0);
    let e = interval(rng, scale);
    match rng.below(20) {
        0 => Inputs::Constructors {
            a: value(rng, scale),
            b: if rng.chance(0.2) {
                e[0]
            } else {
                value(rng, scale)
            },
        },
        1..=3 => Inputs::Queries {
            t: if rng.chance(0.2) {
                *rng.choose(&[0.0, 1.0, 0.5, 0.25, 1.0 / 3.0, -0.5, 1.5])
                    .unwrap_or(&0.0)
            } else {
                value_near(rng, &e, scale)
            },
            e,
            tol: tol(rng),
        },
        4..=6 => {
            let which = rng.below(11) as u8;
            Inputs::Modify {
                x: match which {
                    8 | 9 => rng.uniform(-2.0, 2.0),
                    _ => value_near(rng, &e, scale),
                },
                y: value_near(rng, &e, scale),
                e,
                which,
            }
        }
        7 | 8 => {
            let f = if rng.chance(0.5) {
                let a = value_near(rng, &e, scale);
                let c = value_near(rng, &e, scale);
                [a.min(c), a.max(c)]
            } else {
                interval(rng, scale)
            };
            Inputs::Binary {
                e,
                f,
                tol: tol(rng),
            }
        }
        9 => Inputs::ApproximateUnbounded {
            e,
            center: value(rng, scale),
            half: *rng
                .choose(&[33.0, 1.0, 10.0, 0.0, -5.0, 1e-3])
                .unwrap_or(&33.0),
        },
        10..=13 => {
            let p = period(rng, &e, scale);
            Inputs::PeriodicValue {
                v: periodic_value(rng, &e, p, scale),
                e,
                period: p,
                tol: tol(rng),
            }
        }
        14..=16 => {
            let p = period(rng, &e, scale);
            let f = periodic_interval(rng, &e, p, scale);
            Inputs::IntersectPeriodic {
                e,
                f,
                period: p,
                dont_cross: rng.chance(0.5),
            }
        }
        17 => {
            let p = period(rng, &e, scale);
            let f = periodic_interval(rng, &e, p, scale);
            Inputs::DisjointPeriodic {
                e,
                f,
                period: p,
                tol: tol(rng),
            }
        }
        _ => clamp_vector(rng, scale),
    }
}

/// A second interval of a periodic space: shifted copies, wrapping or degenerate.
fn periodic_interval(rng: &mut Rng, e: &E2, p: f64, scale: f64) -> E2 {
    match rng.below(4) {
        0 => interval(rng, scale),
        1 => {
            let a = periodic_value(rng, e, p, scale);
            [a, a]
        }
        2 => {
            let a = periodic_value(rng, e, p, scale);
            [a, a + p]
        }
        _ => {
            let a = periodic_value(rng, e, p, scale);
            let c = periodic_value(rng, e, p, scale);
            [a.min(c), a.max(c)]
        }
    }
}

fn clamp_vector(rng: &mut Rng, scale: f64) -> Inputs {
    let dim = if rng.chance(0.15) {
        rng.range_i64(10, 14) as usize
    } else {
        rng.below(5) as usize
    };
    let exts: Vec<E2> = (0..dim).map(|_| interval(rng, scale)).collect();
    let values = exts
        .iter()
        .map(|x| {
            let len = x[1] - x[0];
            periodic_value(rng, x, len, scale)
        })
        .collect();
    let periodic = rng
        .chance(0.7)
        .then(|| (0..dim).map(|_| rng.chance(0.4)).collect());
    let n_prior = rng.below(dim as u64 + 3) as usize;
    let prior_clamped = (0..n_prior).map(|_| value(rng, scale)).collect();
    let prior_counts = rng.chance(0.6).then(|| {
        let n = rng.below(dim as u64 + 3) as usize;
        (0..n).map(|_| rng.below(4)).collect()
    });
    Inputs::ClampVector {
        exts,
        values,
        periodic,
        prior_clamped,
        prior_counts,
    }
}

/// A generated case the C++ wrapper accepts (`PeriodicWrap` loop bounds).
fn gen_inputs(rng: &mut Rng) -> Inputs {
    for _ in 0..50 {
        let i = gen_once(rng);
        let (op, input) = flat_input(&i);
        if ffi::accepts(op, &input) {
            return i;
        }
    }
    Inputs::Constructors { a: 1.0, b: 0.0 }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.extent1d"
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        serde_json::to_value(gen_inputs(rng)).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}
