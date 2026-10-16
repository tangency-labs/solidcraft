//! Runner for unit `geom.extent2d`: axis-aligned 2D extents (parameter domains).
//!
//! C++: `solidcore_ffi::units::geom_extent2d` (upstream `source/SMLib/src/SmExtent2d.cpp`).
//! Rust: `solidcore::geom::extent2d`.
//!
//! | op (`inputs.op`)      | Rust                                               | upstream |
//! |-----------------------|----------------------------------------------------|----------|
//! | `from_points`         | `Extent2d::from_points`                            | `SmExtent2d(min, max)` |
//! | `from_point`          | `Extent2d::from_point`                             | `SmExtent2d(p)` |
//! | `new`                 | `Extent2d::new`, `is_init`, `has_negative_area`    | `SmExtent2d()` |
//! | `from_extent3d`       | `Extent2d::from_extent3d`                          | `SmExtent2d(const SmExtent3d&)` |
//! | `add_points`          | `add_point`                                        | `AddPoint2d` |
//! | `set_min_max`         | `set_min_max`                                      | `SetMinMax(p, p)` |
//! | `set_min_max_coords`  | `set_min_max_coords`                               | `SetMinMax(x4)` |
//! | `queries`             | accessors, lengths, intervals, `is_*`, `boundary_types` | `Get*`, `*Length`, `Is*` |
//! | `evaluate`            | `evaluate`                                         | `Evaluate` |
//! | `union`               | `union`                                            | `Union` |
//! | `intersect`           | `intersect`                                        | `Intersect` |
//! | `compare`             | `are_disjoint`, `are_equal`, `is_contained_by`, `are_same`, `max_boundary_dist` | `AreDisjoint`, `AreEqual`, `IsContainedBy`, `operator==`, `GetMaxBoundaryDist2d` |
//! | `expand_relative`     | `expand_relative`                                  | `ExpandRelative` |
//! | `expand_absolute`     | `expand_absolute`                                  | `ExpandAbsolute(double)` |
//! | `expand_absolute_vec` | `expand_absolute_vec`                              | `ExpandAbsolute(SmVector2d)` |
//! | `transpose`           | `transpose`                                        | `Transpose` |
//! | `point_ops`           | `contains_point(_dist)`, `classify_point`, `is_point_on_boundary`, `point_boundaries`, `clamp_point`, `u/v_left_eval` | `ContainsPoint2d`, `ClassifyPoint2d`, `IsPoint2dOnBoundary`, `GetPoint2dBoundaries`, `ClampPoint2d`, `GetU/VLeftEval` |
//! | `contains_periodic`   | `contains_periodic_point`                          | `ContainsPeriodicPoint2d` |
//! | `inversion`           | `inversion`                                        | `Inversion` |
//! | `clip_line`           | `clip_line`                                        | `ClipLine2d` |
//! | `intersect_line`      | `intersect_with_infinite_line`                     | `IntersectWithInfiniteLine` |
//! | `map_to_domain`       | `point_map_to_domain`                              | `Point2dMapToDomain` |
//! | `intervals`           | `u/v_interval`, `set_u/v_interval`                 | `GetU/VInterval`, `SetU/VInterval` |
//! | `scaled_zero`         | `ScaledZero for Extent2d`                          | `SmTol::GetScaledZero(const SmExtent2d&)` |
//!
//! Upstream statuses are compared as the int `status` (0 = `SM_SUCCESS`, else the
//! `SM_ERR_*` value; Rust `InvalidInput` is 1007). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tol::ScaledZero;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::math::{EFF_ZERO, INFINITE_PARAMETER, INFINITE_PARAMETER_SQRT};
use solidcore::SolidError;
use solidcore_ffi::units::geom_extent2d::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.extent2d` runner.
pub struct Runner;

type P2 = [f64; 2];
type E4 = [f64; 4];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    FromPoints {
        a: P2,
        b: P2,
    },
    FromPoint {
        p: P2,
    },
    New {},
    FromExtent3d {
        x: [f64; 6],
    },
    AddPoints {
        e: E4,
        pts: Vec<P2>,
    },
    SetMinMax {
        e: E4,
        min: P2,
        max: P2,
    },
    SetMinMaxCoords {
        e: E4,
        c: E4,
    },
    Queries {
        e: E4,
        tol: f64,
    },
    Evaluate {
        e: E4,
        n: P2,
    },
    Union {
        e: E4,
        f: E4,
    },
    Intersect {
        e: E4,
        f: E4,
    },
    Compare {
        e: E4,
        f: E4,
        tol: f64,
    },
    ExpandRelative {
        e: E4,
        factor: f64,
    },
    ExpandAbsolute {
        e: E4,
        amount: f64,
    },
    ExpandAbsoluteVec {
        e: E4,
        d: P2,
    },
    Transpose {
        e: E4,
    },
    PointOps {
        e: E4,
        p: P2,
        tol: f64,
        tol_v: Option<f64>,
    },
    ContainsPeriodic {
        e: E4,
        p: P2,
        u_period: f64,
        v_period: f64,
        tol: f64,
    },
    Inversion {
        e: E4,
        p: P2,
        tol: f64,
    },
    ClipLine {
        e: E4,
        p: P2,
        v: P2,
        t: f64,
    },
    IntersectLine {
        e: E4,
        p: P2,
        v: P2,
        tol_u: Option<f64>,
        tol_v: Option<f64>,
    },
    MapToDomain {
        e: E4,
        p: P2,
        f: E4,
        switch: bool,
    },
    Intervals {
        e: E4,
        u: P2,
        v: P2,
    },
    ScaledZero {
        e: E4,
    },
}

/// Kind of an output slot.
#[derive(Clone, Copy)]
enum K {
    /// Floats.
    F,
    /// Integers (status, flags, enum codes, bit sets).
    I,
}

/// Output layout per op: `(key, length, kind)`, filled in order from the flat
/// result; a result may stop early (status-only error results).
fn layout(i: &Inputs) -> &'static [(&'static str, usize, K)] {
    const EXT: &[(&str, usize, K)] = &[("extent", 4, K::F)];
    const ST_EXT: &[(&str, usize, K)] = &[("status", 1, K::I), ("extent", 4, K::F)];
    const ST_PT: &[(&str, usize, K)] = &[("status", 1, K::I), ("point", 2, K::F)];
    match i {
        Inputs::FromPoints { .. }
        | Inputs::FromPoint { .. }
        | Inputs::FromExtent3d { .. }
        | Inputs::AddPoints { .. }
        | Inputs::Union { .. }
        | Inputs::ExpandRelative { .. }
        | Inputs::ExpandAbsolute { .. }
        | Inputs::ExpandAbsoluteVec { .. }
        | Inputs::Transpose { .. } => EXT,
        Inputs::New {} => &[("extent", 4, K::F), ("flags", 2, K::I)],
        Inputs::SetMinMax { .. } | Inputs::SetMinMaxCoords { .. } | Inputs::Intersect { .. } => {
            ST_EXT
        }
        Inputs::Queries { .. } => &[
            ("mid", 2, K::F),
            ("uv_min_max", 4, K::F),
            ("max_dimension", 1, K::F),
            ("size", 2, K::F),
            ("xy_length", 2, K::F),
            ("intervals", 4, K::F),
            ("init_negarea_degenerate_bounded", 4, K::I),
            ("boundary_types", 2, K::I),
            ("bounded", 1, K::I),
        ],
        Inputs::Evaluate { .. } => &[("point", 2, K::F)],
        Inputs::Compare { .. } => &[
            ("disjoint_equal_contained_same", 4, K::I),
            ("max_boundary_dist", 1, K::F),
        ],
        Inputs::PointOps { .. } => &[
            ("contains", 2, K::I),
            ("outside_dist", 1, K::F),
            ("classify", 2, K::I),
            ("on_boundary", 1, K::I),
            ("binormal", 2, K::F),
            ("on_boundary_bits", 2, K::I),
            ("clamp", 2, K::F),
            ("left_eval", 2, K::I),
        ],
        Inputs::ContainsPeriodic { .. } => &[
            ("contained", 1, K::I),
            ("periodic_point", 2, K::F),
            ("contained_again", 1, K::I),
        ],
        Inputs::Inversion { .. } | Inputs::MapToDomain { .. } => ST_PT,
        Inputs::ClipLine { .. } => &[("t", 1, K::F)],
        Inputs::IntersectLine { .. } => &[("status_found", 2, K::I), ("interval", 2, K::F)],
        Inputs::Intervals { .. } => &[
            ("intervals", 4, K::F),
            ("statuses", 2, K::I),
            ("extent", 4, K::F),
        ],
        Inputs::ScaledZero { .. } => &[("scaled_zero", 1, K::F)],
    }
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

/// Splits a flat result into keyed outputs following [`layout`].
fn split(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut out = Output::new();
    let mut rest = flat;
    for &(key, n, kind) in layout(i) {
        if rest.is_empty() {
            break;
        }
        let Some((head, tail)) = rest.split_at_checked(n) else {
            return Err(harness(format!("result too short for `{key}`: {flat:?}")));
        };
        out = match kind {
            K::F => out.values(key, head.to_vec()),
            K::I => out.ints(key, head.iter().map(|&v| v as i64).collect::<Vec<_>>()),
        };
        rest = tail;
    }
    if !rest.is_empty() {
        return Err(harness(format!("result too long: {flat:?}")));
    }
    Ok(out)
}

fn ext4(e: &Extent2d) -> E4 {
    let (a, b) = (e.min(), e.max());
    [a.x, a.y, b.x, b.y]
}

fn pt(p: P2) -> Point2 {
    Vec2::new(p[0], p[1])
}

fn ext(e: E4) -> Extent2d {
    Extent2d::from_bounds(e[0], e[1], e[2], e[3])
}

fn b(v: bool) -> f64 {
    if v {
        1.0
    } else {
        0.0
    }
}

/// Status value of a Rust error (upstream `SmStatus`).
fn status_of(e: &SolidError) -> Result<f64, HarnessError> {
    match e {
        SolidError::InvalidInput { .. } => Ok(1007.0),
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

/// Pushes `has, value` for an optional tolerance.
fn opt(v: &mut Vec<f64>, o: Option<f64>) {
    v.push(b(o.is_some()));
    v.push(o.unwrap_or(0.0));
}

/// Flat input for the C++ op.
fn flat_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut v = Vec::new();
    let op = match i {
        Inputs::FromPoints { a, b } => {
            v.extend(a);
            v.extend(b);
            Op::FromPoints
        }
        Inputs::FromPoint { p } => {
            v.extend(p);
            Op::FromPoint
        }
        Inputs::New {} => Op::New,
        Inputs::FromExtent3d { x } => {
            v.extend(x);
            Op::FromExtent3d
        }
        Inputs::AddPoints { e, pts } => {
            v.extend(e);
            v.extend(pts.iter().flatten());
            Op::AddPoints
        }
        Inputs::SetMinMax { e, min, max } => {
            v.extend(e);
            v.extend(min);
            v.extend(max);
            Op::SetMinMax
        }
        Inputs::SetMinMaxCoords { e, c } => {
            v.extend(e);
            v.extend(c);
            Op::SetMinMaxCoords
        }
        Inputs::Queries { e, tol } => {
            v.extend(e);
            v.push(*tol);
            Op::Queries
        }
        Inputs::Evaluate { e, n } => {
            v.extend(e);
            v.extend(n);
            Op::Evaluate
        }
        Inputs::Union { e, f } => {
            v.extend(e);
            v.extend(f);
            Op::Union
        }
        Inputs::Intersect { e, f } => {
            v.extend(e);
            v.extend(f);
            Op::Intersect
        }
        Inputs::Compare { e, f, tol } => {
            v.extend(e);
            v.extend(f);
            v.push(*tol);
            Op::Compare
        }
        Inputs::ExpandRelative { e, factor } => {
            v.extend(e);
            v.push(*factor);
            Op::ExpandRelative
        }
        Inputs::ExpandAbsolute { e, amount } => {
            v.extend(e);
            v.push(*amount);
            Op::ExpandAbsolute
        }
        Inputs::ExpandAbsoluteVec { e, d } => {
            v.extend(e);
            v.extend(d);
            Op::ExpandAbsoluteVec
        }
        Inputs::Transpose { e } => {
            v.extend(e);
            Op::Transpose
        }
        Inputs::PointOps { e, p, tol, tol_v } => {
            v.extend(e);
            v.extend(p);
            v.push(*tol);
            opt(&mut v, *tol_v);
            Op::PointOps
        }
        Inputs::ContainsPeriodic {
            e,
            p,
            u_period,
            v_period,
            tol,
        } => {
            v.extend(e);
            v.extend(p);
            v.extend([*u_period, *v_period, *tol]);
            Op::ContainsPeriodic
        }
        Inputs::Inversion { e, p, tol } => {
            v.extend(e);
            v.extend(p);
            v.push(*tol);
            Op::Inversion
        }
        Inputs::ClipLine { e, p, v: d, t } => {
            v.extend(e);
            v.extend(p);
            v.extend(d);
            v.push(*t);
            Op::ClipLine
        }
        Inputs::IntersectLine {
            e,
            p,
            v: d,
            tol_u,
            tol_v,
        } => {
            v.extend(e);
            v.extend(p);
            v.extend(d);
            opt(&mut v, *tol_u);
            opt(&mut v, *tol_v);
            Op::IntersectLine
        }
        Inputs::MapToDomain { e, p, f, switch } => {
            v.extend(e);
            v.extend(p);
            v.extend(f);
            v.push(b(*switch));
            Op::MapToDomain
        }
        Inputs::Intervals { e, u, v: w } => {
            v.extend(e);
            v.extend(u);
            v.extend(w);
            Op::Intervals
        }
        Inputs::ScaledZero { e } => {
            v.extend(e);
            Op::ScaledZero
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
        Inputs::FromPoints { a, b } => o.extend(ext4(&Extent2d::from_points(pt(*a), pt(*b)))),
        Inputs::FromPoint { p } => o.extend(ext4(&Extent2d::from_point(pt(*p)))),
        Inputs::New {} => {
            let e = Extent2d::new();
            o.extend(ext4(&e));
            o.push(b(e.is_init()));
            o.push(b(e.has_negative_area()));
        }
        Inputs::FromExtent3d { x } => {
            let x3 = Extent3d::from_bounds(x[0], x[1], x[2], x[3], x[4], x[5]);
            o.extend(ext4(&Extent2d::from(x3)));
        }
        Inputs::AddPoints { e, pts } => {
            let mut x = ext(*e);
            for p in pts {
                x.add_point(pt(*p));
            }
            o.extend(ext4(&x));
        }
        Inputs::SetMinMax { e, min, max } => {
            let mut x = ext(*e);
            let r = x.set_min_max(pt(*min), pt(*max));
            o.push(status(&r)?);
            o.extend(ext4(&x));
        }
        Inputs::SetMinMaxCoords { e, c } => {
            let mut x = ext(*e);
            let r = x.set_min_max_coords(c[0], c[1], c[2], c[3]);
            o.push(status(&r)?);
            o.extend(ext4(&x));
        }
        Inputs::Queries { e, tol } => {
            let x = ext(*e);
            o.extend(x.mid().to_array());
            o.extend([x.u_min(), x.u_max(), x.v_min(), x.v_max()]);
            o.push(x.max_dimension());
            o.extend(x.size().to_array());
            o.extend([x.x_length(), x.y_length()]);
            for ivl in [x.u_interval(), x.v_interval()] {
                o.extend([ivl.min(), ivl.max()]);
            }
            o.extend([
                b(x.is_init()),
                b(x.has_negative_area()),
                b(x.is_degenerate(*tol)),
                b(x.is_bounded()),
            ]);
            o.extend(x.boundary_types().map(|t| f64::from(t.code())));
            o.push(b(x.is_bounded()));
        }
        Inputs::Evaluate { e, n } => o.extend(ext(*e).evaluate(n[0], n[1]).to_array()),
        Inputs::Union { e, f } => o.extend(ext4(&ext(*e).union(&ext(*f)))),
        Inputs::Intersect { e, f } => {
            let r = ext(*e).intersect(&ext(*f));
            o.push(status(&r)?);
            if let Ok(x) = r {
                o.extend(ext4(&x));
            }
        }
        Inputs::Compare { e, f, tol } => {
            let (x, y) = (ext(*e), ext(*f));
            o.extend([
                b(x.are_disjoint(&y, *tol)),
                b(x.are_equal(&y, *tol)),
                b(x.is_contained_by(&y, *tol)),
                b(x.are_same(&y)),
                x.max_boundary_dist(&y),
            ]);
        }
        Inputs::ExpandRelative { e, factor } => {
            o.extend(ext4(ext(*e).expand_relative(*factor)));
        }
        Inputs::ExpandAbsolute { e, amount } => {
            o.extend(ext4(ext(*e).expand_absolute(*amount)));
        }
        Inputs::ExpandAbsoluteVec { e, d } => {
            o.extend(ext4(ext(*e).expand_absolute_vec(pt(*d))));
        }
        Inputs::Transpose { e } => {
            let mut x = ext(*e);
            x.transpose();
            o.extend(ext4(&x));
        }
        Inputs::PointOps { e, p, tol, tol_v } => {
            let (x, q) = (ext(*e), pt(*p));
            let (inside, dist) = x.contains_point_dist(q, *tol);
            o.extend([b(x.contains_point(q, *tol)), b(inside), dist]);
            let (cu, cv) = x.classify_point(q, *tol, *tol_v);
            o.extend([f64::from(cu.code()), f64::from(cv.code())]);
            let (on, bn) = x.is_point_on_boundary(q, *tol);
            o.push(b(on));
            o.extend(bn.to_array());
            o.push(b(on));
            o.push(f64::from(x.point_boundaries(q, *tol)));
            o.extend(x.clamp_point(q).to_array());
            o.extend([b(x.u_left_eval(q.x)), b(x.v_left_eval(q.y))]);
        }
        Inputs::ContainsPeriodic {
            e,
            p,
            u_period,
            v_period,
            tol,
        } => {
            let (c, per) = ext(*e).contains_periodic_point(pt(*p), *u_period, *v_period, *tol);
            o.push(b(c));
            o.extend(per.to_array());
            o.push(b(c));
        }
        Inputs::Inversion { e, p, tol } => {
            let r = ext(*e).inversion(pt(*p), *tol);
            o.push(status(&r)?);
            if let Ok(q) = r {
                o.extend(q.to_array());
            }
        }
        Inputs::ClipLine { e, p, v, t } => o.push(ext(*e).clip_line(pt(*p), pt(*v), *t)),
        Inputs::IntersectLine {
            e,
            p,
            v,
            tol_u,
            tol_v,
        } => {
            let r = ext(*e).intersect_with_infinite_line(pt(*p), pt(*v), *tol_u, *tol_v);
            o.push(0.0);
            o.push(b(r.is_some()));
            if let Some(ivl) = r {
                o.extend([ivl.min(), ivl.max()]);
            }
        }
        Inputs::MapToDomain { e, p, f, switch } => {
            let r = ext(*e).point_map_to_domain(pt(*p), &ext(*f), *switch);
            o.push(status(&r)?);
            if let Ok(q) = r {
                o.extend(q.to_array());
            }
        }
        Inputs::Intervals { e, u, v } => {
            let mut x = ext(*e);
            for ivl in [x.u_interval(), x.v_interval()] {
                o.extend([ivl.min(), ivl.max()]);
            }
            x.set_u_interval(&Extent1d::from_min_max(u[0], u[1]));
            x.set_v_interval(&Extent1d::from_min_max(v[0], v[1]));
            o.extend([0.0, 0.0]);
            o.extend(ext4(&x));
        }
        Inputs::ScaledZero { e } => o.push(ext(*e).scaled_zero()),
    }
    Ok(o)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    split(i, &rust_flat(i)?)
}

// ---- generator -------------------------------------------------------------------

/// A coordinate: mostly ordinary, sometimes integral (exact ties), huge, tiny, or
/// one of upstream's sentinel values.
fn coord(rng: &mut Rng, scale: f64) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.uniform(-scale, scale)
    } else if r < 0.85 {
        rng.range_i64(-4, 4) as f64
    } else if r < 0.92 {
        rng.uniform(-1e-9, 1e-9)
    } else {
        *rng.choose(&[0.0, -0.0, 1e6, -1e6, 1e15, -1e15])
            .unwrap_or(&0.0)
    }
}

fn point(rng: &mut Rng, scale: f64) -> P2 {
    [coord(rng, scale), coord(rng, scale)]
}

const SENTINELS: [f64; 4] = [
    INFINITE_PARAMETER,
    -INFINITE_PARAMETER,
    INFINITE_PARAMETER_SQRT,
    -INFINITE_PARAMETER_SQRT,
];

/// An extent: usually ordered, sometimes inverted, point sized, line shaped,
/// `Init`, or with unbounded ends.
fn extent(rng: &mut Rng, scale: f64) -> E4 {
    let r = rng.unit();
    if r < 0.04 {
        return ext4(&Extent2d::new());
    }
    let a = point(rng, scale);
    if r < 0.1 {
        return join(a, a);
    }
    let (mut lo, mut hi) = (a, point(rng, scale));
    if r < 0.9 {
        for (l, h) in lo.iter_mut().zip(hi.iter_mut()) {
            if *l > *h {
                core::mem::swap(l, h);
            }
        }
    }
    if rng.chance(0.08) {
        let k = rng.below(2) as usize;
        if let (Some(l), Some(h)) = (lo.get(k).copied(), hi.get_mut(k)) {
            *h = l;
        }
    }
    if rng.chance(0.15) {
        for (l, h) in lo.iter_mut().zip(hi.iter_mut()) {
            if rng.chance(0.4) {
                *l = -INFINITE_PARAMETER;
            }
            if rng.chance(0.4) {
                *h = INFINITE_PARAMETER;
            }
        }
        if rng.chance(0.15) {
            let k = rng.below(4) as usize;
            let v = *rng.choose(&SENTINELS).unwrap_or(&INFINITE_PARAMETER);
            if let Some(c) = lo.iter_mut().chain(hi.iter_mut()).nth(k) {
                *c = v;
            }
        }
    }
    join(lo, hi)
}

fn join(lo: P2, hi: P2) -> E4 {
    [lo[0], lo[1], hi[0], hi[1]]
}

const OFFSETS: [f64; 7] = [1e-13, 1e-12, 2e-12, 1e-10, 1e-6, 1e-3, 0.5];

/// A point related to `e`: a corner, a side point, slightly off a side, or random.
fn point_near(rng: &mut Rng, e: &E4, scale: f64) -> P2 {
    if rng.chance(0.25) {
        return point(rng, scale);
    }
    let lo = [e[0], e[1]];
    let hi = [e[2], e[3]];
    let mut p = [0.0; 2];
    for ((c, l), h) in p.iter_mut().zip(lo).zip(hi) {
        *c = match rng.below(6) {
            0 => l,
            1 => h,
            2 | 3 => l + rng.uniform(0.0, 1.0) * (h - l),
            4 => l - *rng.choose(&OFFSETS).unwrap_or(&0.0) * rng.uniform(-1.0, 1.0),
            _ => h + *rng.choose(&OFFSETS).unwrap_or(&0.0) * rng.uniform(-1.0, 1.0),
        };
    }
    p
}

fn tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[
        0.0, 0.0, EFF_ZERO, 1e-12, 1e-9, 1e-6, 1.2e-8, 0.01, 0.2, 0.5, -1e-6,
    ])
    .unwrap_or(&0.0)
}

fn direction(rng: &mut Rng) -> P2 {
    let mut v = [0.0; 2];
    for c in &mut v {
        *c = match rng.below(6) {
            0 => 0.0,
            1 => rng.uniform(-1e-12, 1e-12),
            2 => *rng.choose(&[1.0, -1.0, 2e-12, -2e-12]).unwrap_or(&1.0),
            _ => rng.uniform(-3.0, 3.0),
        };
    }
    if rng.chance(0.03) {
        v = [0.0; 2];
    }
    v
}

/// A normalised coordinate for `Evaluate`.
fn norm(rng: &mut Rng) -> f64 {
    if rng.chance(0.3) {
        rng.unit()
    } else {
        *rng.choose(&[0.0, 1.0, 0.5, 0.25, 1.0 / 3.0, 0.1, 0.9])
            .unwrap_or(&0.0)
    }
}

/// A period for `ContainsPeriodicPoint2d`: often 0 (not periodic) or the interval
/// length (closed), sometimes larger.
///
/// Upstream `SmExtent1d::PeriodicWrap` steps one period at a time, so a period that
/// is tiny relative to the point's distance from the interval would loop for ages
/// (or forever); such periods (and non-positive ones) become 0.
fn period(rng: &mut Rng, len: f64, dist: f64) -> f64 {
    let p = match rng.below(5) {
        0 | 1 => 0.0,
        2 => len,
        3 => len * rng.uniform(1.0, 3.0),
        _ => *rng
            .choose(&[360.0, core::f64::consts::TAU, 1.0, 4.0])
            .unwrap_or(&1.0),
    };
    if p > 1e-6 && dist.abs() / p < 1e4 {
        p
    } else {
        0.0
    }
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 10.0, 1e-3, 1e4]).unwrap_or(&1.0);
    let e = extent(rng, scale);
    match rng.below(26) {
        0 => Inputs::FromPoints {
            a: point(rng, scale),
            b: point(rng, scale),
        },
        1 => Inputs::FromPoint {
            p: point(rng, scale),
        },
        2 => Inputs::FromExtent3d {
            x: [e[0], e[1], coord(rng, scale), e[2], e[3], coord(rng, scale)],
        },
        3 => {
            let n = rng.below(6) as usize;
            let pts = (0..n).map(|_| point_near(rng, &e, scale)).collect();
            Inputs::AddPoints { e, pts }
        }
        4 => Inputs::SetMinMax {
            e,
            min: point(rng, scale),
            max: point(rng, scale),
        },
        5 => {
            let c = extent(rng, scale);
            Inputs::SetMinMaxCoords { e, c }
        }
        6 => Inputs::Queries { e, tol: tol(rng) },
        7 => Inputs::Evaluate {
            e,
            n: [norm(rng), norm(rng)],
        },
        8 => Inputs::Union {
            e,
            f: extent(rng, scale),
        },
        9 => Inputs::Intersect {
            e,
            f: extent(rng, scale),
        },
        10 => {
            let f = if rng.chance(0.3) {
                let mut f = e;
                for c in &mut f {
                    *c += *rng
                        .choose(&[0.0, 1e-13, -1e-12, 1e-9, 0.05])
                        .unwrap_or(&0.0);
                }
                f
            } else {
                extent(rng, scale)
            };
            Inputs::Compare {
                e,
                f,
                tol: tol(rng),
            }
        }
        11 => Inputs::ExpandRelative {
            e,
            factor: rng.uniform(-1.0, 2.0),
        },
        12 => Inputs::ExpandAbsolute {
            e,
            amount: rng.uniform(-1.0, 1.0) * scale,
        },
        13 => Inputs::ExpandAbsoluteVec {
            e,
            d: [
                rng.uniform(-1.0, 1.0) * scale,
                rng.uniform(-1.0, 1.0) * scale,
            ],
        },
        14 => Inputs::Transpose { e },
        15..=17 => Inputs::PointOps {
            p: point_near(rng, &e, scale),
            e,
            tol: tol(rng),
            tol_v: rng.chance(0.4).then(|| tol(rng)),
        },
        18 => {
            let p = point_near(rng, &e, scale);
            let ul = e[2] - e[0];
            let vl = e[3] - e[1];
            let p = [
                p[0] + *rng.choose(&[0.0, 1.0, -1.0, 2.0]).unwrap_or(&0.0) * ul,
                p[1] + *rng.choose(&[0.0, 1.0, -1.0, 3.0]).unwrap_or(&0.0) * vl,
            ];
            Inputs::ContainsPeriodic {
                e,
                u_period: period(rng, ul, p[0] - e[0]),
                v_period: period(rng, vl, p[1] - e[1]),
                p,
                tol: tol(rng),
            }
        }
        19 => Inputs::Inversion {
            p: point_near(rng, &e, scale),
            e,
            tol: tol(rng),
        },
        20 => {
            let p = point_near(rng, &e, scale);
            let v = if rng.chance(0.4) {
                let q = point_near(rng, &e, scale);
                [q[0] - p[0], q[1] - p[1]]
            } else {
                direction(rng)
            };
            Inputs::ClipLine {
                e,
                p,
                v,
                t: *rng
                    .choose(&[1.0, 4.0, -1.0, 0.5, 0.0, 100.0])
                    .unwrap_or(&1.0),
            }
        }
        21 | 22 => {
            let p = point_near(rng, &e, scale);
            let v = if rng.chance(0.4) {
                let q = point_near(rng, &e, scale);
                [q[0] - p[0], q[1] - p[1]]
            } else {
                direction(rng)
            };
            Inputs::IntersectLine {
                e,
                p,
                v,
                tol_u: rng.chance(0.5).then(|| tol(rng)),
                tol_v: rng.chance(0.3).then(|| tol(rng)),
            }
        }
        23 => Inputs::MapToDomain {
            p: point_near(rng, &e, scale),
            e,
            f: extent(rng, scale),
            switch: rng.chance(0.5),
        },
        24 => {
            let mut ivl = || {
                let a = coord(rng, scale);
                let b = if rng.chance(0.2) {
                    a
                } else {
                    coord(rng, scale)
                };
                [a, b]
            };
            let u = ivl();
            let v = ivl();
            Inputs::Intervals { e, u, v }
        }
        _ => Inputs::ScaledZero { e },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.extent2d"
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
