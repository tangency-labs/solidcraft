//! Runner for unit `geom.extent3d`: axis-aligned 3D extents (bounding boxes).
//!
//! C++: `solidcore_ffi::units::geom_extent3d` (upstream `source/SMLib/src/SmExtent3d.cpp`).
//! Rust: `solidcore::geom::extent3d`.
//!
//! | op (`inputs.op`)        | Rust                                             | upstream |
//! |-------------------------|--------------------------------------------------|----------|
//! | `from_points`           | `Extent3d::from_points`                          | `SmExtent3d(min, max)` |
//! | `from_point`            | `Extent3d::from_point`                           | `SmExtent3d(p)` |
//! | `new`                   | `Extent3d::new`, `is_init`, `has_negative_volume` | `SmExtent3d()` |
//! | `add_points`            | `add_point`                                      | `AddPoint3d` |
//! | `set_min_max`           | `set_min_max`                                    | `SetMinMax(p, p)` |
//! | `set_min_max_coords`    | `set_min_max_coords`                             | `SetMinMax(x6)` |
//! | `set_bound`             | `set_u_min`, `set_u_max`, `set_v_min`, `set_v_max` | `SetUMin` .. `SetVMax` |
//! | `queries`               | accessors, lengths, `is_*`, `boundary_types`, `any_bounds` | `Get*`, `*Length`, `Is*`, `AnyBounds`, `AssertDefined` |
//! | `evaluate`              | `evaluate`                                       | `Evaluate` |
//! | `union`                 | `union`                                          | `Union` |
//! | `intersect`             | `intersect`                                      | `Intersect` |
//! | `distances`             | `max/min_distance(_squared)`                     | `Maximum/MinimumDistance(Squared)` |
//! | `expand_relative`       | `expand_relative`                                | `ExpandRelative` |
//! | `expand_absolute`       | `expand_absolute`                                | `ExpandAbsolute` |
//! | `corners_edges_planes`  | `corners`, `edges`, `planes`                     | `GetCorners`, `GetEdges`, `GetPlanes` |
//! | `sphere_bound`          | `sphere_bound`                                   | `ComputeSphereBound` |
//! | `cone_bound_to`         | `cone_bound_to`                                  | `ComputeConeBoundTo` |
//! | `disjoint_contained`    | `are_disjoint`, `is_contained_by`                | `AreDisjoint`, `IsContainedBy` |
//! | `point_predicates`      | `contains_point`, `is_point_on_boundary`         | `ContainsPoint3d`, `IsPoint3dOnBoundary` |
//! | `contains_line_seg`     | `contains_line_seg`                              | `ContainsLineSeg3d` |
//! | `intersect_line`        | `intersect_line`                                 | `IntersectLine` |
//! | `set_unbounded`         | `set_unbounded`                                  | `SetUnbounded` |
//! | `approximate_unbounded` | `approximate_unbounded_expanded`                 | `ApproximateUnbounded` |
//! | `circumscribe`          | `Extent3d::from_pseudo_box`                      | `SmExtent3d(SmPseudoBox)`, `Circumscribe` |
//! | `intervals`             | `u/v/w_interval`, `set_u/v_interval`             | `GetU/V/WInterval`, `SetU/VInterval` |
//!
//! Upstream statuses are compared as the int `status` (0 = `SM_SUCCESS`, else the
//! `SM_ERR_*` value; Rust `InvalidInput` is 1007). Every run has at least
//! [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent3d::{Extent3d, LineHit};
use solidcore::geom::pseudo_box::PseudoBox;
use solidcore::math::{Point3, Vec3, INFINITE_PARAMETER, INFINITE_PARAMETER_SQRT};
use solidcore::SolidError;
use solidcore_ffi::units::geom_extent3d::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.extent3d` runner.
pub struct Runner;

type P3 = [f64; 3];
type E6 = [f64; 6];

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    FromPoints {
        a: P3,
        b: P3,
    },
    FromPoint {
        p: P3,
    },
    New {},
    AddPoints {
        e: E6,
        pts: Vec<P3>,
    },
    SetMinMax {
        e: E6,
        min: P3,
        max: P3,
    },
    SetMinMaxCoords {
        e: E6,
        c: E6,
    },
    SetBound {
        e: E6,
        which: u8,
        v: f64,
    },
    Queries {
        e: E6,
        tol: f64,
    },
    Evaluate {
        e: E6,
        n: P3,
    },
    Union {
        e: E6,
        f: E6,
    },
    Intersect {
        e: E6,
        f: E6,
    },
    Distances {
        e: E6,
        f: E6,
    },
    ExpandRelative {
        e: E6,
        factor: f64,
    },
    ExpandAbsolute {
        e: E6,
        amount: f64,
    },
    CornersEdgesPlanes {
        e: E6,
    },
    SphereBound {
        e: E6,
    },
    ConeBoundTo {
        e: E6,
        f: E6,
    },
    DisjointContained {
        e: E6,
        f: E6,
        tol: f64,
    },
    PointPredicates {
        e: E6,
        p: P3,
        tol: f64,
    },
    ContainsLineSeg {
        e: E6,
        p: P3,
        q: P3,
        tol: f64,
    },
    IntersectLine {
        e: E6,
        p: P3,
        v: P3,
    },
    SetUnbounded {
        e: E6,
    },
    ApproximateUnbounded {
        e: E6,
        center: Option<P3>,
        half_size: f64,
    },
    Circumscribe {
        basis: [P3; 3],
        intervals: [[f64; 2]; 3],
    },
    Intervals {
        e: E6,
        u: [f64; 2],
        v: [f64; 2],
    },
}

/// Kind of an output slot.
#[derive(Clone, Copy)]
enum K {
    /// Floats.
    F,
    /// Integers (status, flags, counts, enum codes).
    I,
}

/// Output layout per op: `(key, length, kind)`, filled in order from the flat
/// result; a result may stop early (status-only error results).
fn layout(i: &Inputs) -> &'static [(&'static str, usize, K)] {
    const EXT: &[(&str, usize, K)] = &[("extent", 6, K::F)];
    const ST_EXT: &[(&str, usize, K)] = &[("status", 1, K::I), ("extent", 6, K::F)];
    match i {
        Inputs::FromPoints { .. }
        | Inputs::FromPoint { .. }
        | Inputs::AddPoints { .. }
        | Inputs::Union { .. }
        | Inputs::ExpandRelative { .. }
        | Inputs::ExpandAbsolute { .. }
        | Inputs::SetUnbounded { .. }
        | Inputs::Circumscribe { .. } => EXT,
        Inputs::New {} => &[("extent", 6, K::F), ("flags", 2, K::I)],
        Inputs::SetMinMax { .. }
        | Inputs::SetMinMaxCoords { .. }
        | Inputs::SetBound { .. }
        | Inputs::Intersect { .. } => ST_EXT,
        Inputs::Queries { .. } => &[
            ("mid", 3, K::F),
            ("uvw_min_max", 6, K::F),
            ("min_max_at", 8, K::F),
            ("max_length", 1, K::F),
            ("max_dimension", 1, K::F),
            ("size", 3, K::F),
            ("xyz_length", 3, K::F),
            ("defined_negvol_init_bounded", 4, K::I),
            ("boundary_types", 3, K::I),
            ("any_bounds_point_sized", 2, K::I),
        ],
        Inputs::Evaluate { .. } => &[("point", 3, K::F)],
        Inputs::Distances { .. } => &[
            ("max_sq", 1, K::F),
            ("min_sq", 1, K::F),
            ("max", 1, K::F),
            ("min", 1, K::F),
        ],
        Inputs::CornersEdgesPlanes { .. } => &[
            ("corners", 24, K::F),
            ("edges", 72, K::F),
            ("plane_points", 18, K::F),
            ("plane_normals", 18, K::F),
        ],
        Inputs::SphereBound { .. } => &[("center", 3, K::F), ("radii", 2, K::F)],
        Inputs::ConeBoundTo { .. } => &[("status", 1, K::I), ("axis", 3, K::F), ("angle", 1, K::F)],
        Inputs::DisjointContained { .. } => &[("disjoint_contained", 2, K::I)],
        Inputs::PointPredicates { .. } => {
            &[("contains_on_boundary", 2, K::I), ("binormal", 3, K::F)]
        }
        Inputs::ContainsLineSeg { .. } => &[("contains", 1, K::I)],
        Inputs::IntersectLine { .. } => &[("status", 1, K::I), ("count", 1, K::I), ("t", 2, K::F)],
        Inputs::ApproximateUnbounded { .. } => &[("approx", 6, K::F), ("expanded", 6, K::F)],
        Inputs::Intervals { .. } => &[
            ("intervals", 6, K::F),
            ("statuses", 2, K::I),
            ("extent", 6, K::F),
        ],
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

fn ext6(e: &Extent3d) -> E6 {
    let (a, b) = (e.min(), e.max());
    [a.x, a.y, a.z, b.x, b.y, b.z]
}

fn pt(p: P3) -> Point3 {
    Vec3::new(p[0], p[1], p[2])
}

fn ext(e: E6) -> Extent3d {
    Extent3d::from_bounds(e[0], e[1], e[2], e[3], e[4], e[5])
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
        Inputs::SetBound { e, which, v: x } => {
            v.extend(e);
            v.push(f64::from(*which));
            v.push(*x);
            Op::SetBound
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
        Inputs::Union { e, f }
        | Inputs::Intersect { e, f }
        | Inputs::Distances { e, f }
        | Inputs::ConeBoundTo { e, f } => {
            v.extend(e);
            v.extend(f);
            match i {
                Inputs::Union { .. } => Op::Union,
                Inputs::Intersect { .. } => Op::Intersect,
                Inputs::Distances { .. } => Op::Distances,
                _ => Op::ConeBoundTo,
            }
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
        Inputs::CornersEdgesPlanes { e } => {
            v.extend(e);
            Op::CornersEdgesPlanes
        }
        Inputs::SphereBound { e } => {
            v.extend(e);
            Op::SphereBound
        }
        Inputs::DisjointContained { e, f, tol } => {
            v.extend(e);
            v.extend(f);
            v.push(*tol);
            Op::DisjointContained
        }
        Inputs::PointPredicates { e, p, tol } => {
            v.extend(e);
            v.extend(p);
            v.push(*tol);
            Op::PointPredicates
        }
        Inputs::ContainsLineSeg { e, p, q, tol } => {
            v.extend(e);
            v.extend(p);
            v.extend(q);
            v.push(*tol);
            Op::ContainsLineSeg
        }
        Inputs::IntersectLine { e, p, v: d } => {
            v.extend(e);
            v.extend(p);
            v.extend(d);
            Op::IntersectLine
        }
        Inputs::SetUnbounded { e } => {
            v.extend(e);
            Op::SetUnbounded
        }
        Inputs::ApproximateUnbounded {
            e,
            center,
            half_size,
        } => {
            v.extend(e);
            v.push(b(center.is_some()));
            v.extend(center.unwrap_or([0.0; 3]));
            v.push(*half_size);
            Op::ApproximateUnbounded
        }
        Inputs::Circumscribe { basis, intervals } => {
            v.extend(basis.iter().flatten());
            v.extend(intervals.iter().flatten());
            Op::Circumscribe
        }
        Inputs::Intervals { e, u, v: w } => {
            v.extend(e);
            v.extend(u);
            v.extend(w);
            Op::Intervals
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

fn rust_flat(i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    let mut o: Vec<f64> = Vec::new();
    match i {
        Inputs::FromPoints { a, b } => o.extend(ext6(&Extent3d::from_points(pt(*a), pt(*b)))),
        Inputs::FromPoint { p } => o.extend(ext6(&Extent3d::from_point(pt(*p)))),
        Inputs::New {} => {
            let e = Extent3d::new();
            o.extend(ext6(&e));
            o.push(b(e.is_init()));
            o.push(b(e.has_negative_volume()));
        }
        Inputs::AddPoints { e, pts } => {
            let mut x = ext(*e);
            for p in pts {
                x.add_point(pt(*p));
            }
            o.extend(ext6(&x));
        }
        Inputs::SetMinMax { e, min, max } => {
            let mut x = ext(*e);
            let r = x.set_min_max(pt(*min), pt(*max));
            o.push(status(&r)?);
            o.extend(ext6(&x));
        }
        Inputs::SetMinMaxCoords { e, c } => {
            let mut x = ext(*e);
            let r = x.set_min_max_coords(c[0], c[1], c[2], c[3], c[4], c[5]);
            o.push(status(&r)?);
            o.extend(ext6(&x));
        }
        Inputs::SetBound { e, which, v } => {
            let mut x = ext(*e);
            let r = match which {
                0 => x.set_u_min(*v),
                1 => x.set_u_max(*v),
                2 => x.set_v_min(*v),
                3 => x.set_v_max(*v),
                w => return Err(harness(format!("which = {w}"))),
            };
            o.push(status(&r)?);
            o.extend(ext6(&x));
        }
        Inputs::Queries { e, tol } => {
            let x = ext(*e);
            o.extend(x.mid().to_array());
            o.extend([
                x.u_min(),
                x.u_max(),
                x.v_min(),
                x.v_max(),
                x.w_min(),
                x.w_max(),
            ]);
            for k in 0..4 {
                o.push(x.min_at(k));
                o.push(x.max_at(k));
            }
            o.push(x.max_length());
            o.push(x.max_dimension());
            o.extend(x.size().to_array());
            o.extend([x.x_length(), x.y_length(), x.z_length()]);
            o.extend([
                b(x.is_defined()),
                b(x.has_negative_volume()),
                b(x.is_init()),
                b(x.is_bounded()),
            ]);
            o.extend(x.boundary_types().map(|t| f64::from(t.code())));
            o.push(b(x.any_bounds()));
            o.push(b(x.is_point_sized(*tol)));
        }
        Inputs::Evaluate { e, n } => o.extend(ext(*e).evaluate(n[0], n[1], n[2]).to_array()),
        Inputs::Union { e, f } => o.extend(ext6(&ext(*e).union(&ext(*f)))),
        Inputs::Intersect { e, f } => {
            let r = ext(*e).intersect(&ext(*f));
            o.push(status(&r)?);
            if let Ok(x) = r {
                o.extend(ext6(&x));
            }
        }
        Inputs::Distances { e, f } => {
            let (x, y) = (ext(*e), ext(*f));
            o.extend([
                x.max_distance_squared(&y),
                x.min_distance_squared(&y),
                x.max_distance(&y),
                x.min_distance(&y),
            ]);
        }
        Inputs::ExpandRelative { e, factor } => {
            o.extend(ext6(ext(*e).expand_relative(*factor)));
        }
        Inputs::ExpandAbsolute { e, amount } => {
            o.extend(ext6(ext(*e).expand_absolute(*amount)));
        }
        Inputs::CornersEdgesPlanes { e } => {
            let x = ext(*e);
            let (pp, pn) = x.planes();
            for p in x.corners().iter().chain(&x.edges()).chain(&pp).chain(&pn) {
                o.extend(p.to_array());
            }
        }
        Inputs::SphereBound { e } => {
            let s = ext(*e).sphere_bound();
            o.extend(s.center.to_array());
            o.extend([s.radius, s.min_radius]);
        }
        Inputs::ConeBoundTo { e, f } => {
            let r = ext(*e).cone_bound_to(&ext(*f));
            o.push(status(&r)?);
            if let Ok((v, a)) = r {
                o.extend(v.to_array());
                o.push(a);
            }
        }
        Inputs::DisjointContained { e, f, tol } => {
            let (x, y) = (ext(*e), ext(*f));
            o.extend([b(x.are_disjoint(&y, *tol)), b(x.is_contained_by(&y, *tol))]);
        }
        Inputs::PointPredicates { e, p, tol } => {
            let x = ext(*e);
            let (on, bn) = x.is_point_on_boundary(pt(*p), *tol);
            o.extend([b(x.contains_point(pt(*p), *tol)), b(on)]);
            o.extend(bn.to_array());
        }
        Inputs::ContainsLineSeg { e, p, q, tol } => {
            o.push(b(ext(*e).contains_line_seg(pt(*p), pt(*q), *tol)));
        }
        Inputs::IntersectLine { e, p, v } => {
            let r = ext(*e).intersect_line(pt(*p), pt(*v));
            o.push(status(&r)?);
            if let Ok(hit) = r {
                o.push(f64::from(hit.count()));
                match hit {
                    LineHit::Miss => o.extend([f64::NAN, f64::NAN]),
                    LineHit::Touch { t } => o.extend([t, t]),
                    LineHit::Through { enter, exit } => o.extend([enter, exit]),
                }
            }
        }
        Inputs::SetUnbounded { e } => {
            let mut x = ext(*e);
            x.set_unbounded();
            o.extend(ext6(&x));
        }
        Inputs::ApproximateUnbounded {
            e,
            center,
            half_size,
        } => {
            let (a, x) = ext(*e).approximate_unbounded_expanded(center.map(pt), *half_size);
            o.extend(ext6(&a));
            o.extend(ext6(&x.unwrap_or_default()));
        }
        Inputs::Circumscribe { basis, intervals } => {
            let ivl = |i: [f64; 2]| Extent1d::from_min_max(i[0], i[1]);
            let pb = PseudoBox::from_basis_intervals(basis.map(pt), intervals.map(ivl));
            o.extend(ext6(&Extent3d::from_pseudo_box(&pb)));
        }
        Inputs::Intervals { e, u, v } => {
            let mut x = ext(*e);
            for ivl in [x.u_interval(), x.v_interval(), x.w_interval()] {
                o.extend([ivl.min(), ivl.max()]);
            }
            x.set_u_interval(&Extent1d::from_min_max(u[0], u[1]));
            x.set_v_interval(&Extent1d::from_min_max(v[0], v[1]));
            o.extend([0.0, 0.0]);
            o.extend(ext6(&x));
        }
    }
    Ok(Ok(o))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_flat(i)? {
        Ok(flat) => split(i, &flat),
        Err(deferred) => Ok(deferred),
    }
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

fn point(rng: &mut Rng, scale: f64) -> P3 {
    [coord(rng, scale), coord(rng, scale), coord(rng, scale)]
}

const SENTINELS: [f64; 4] = [
    INFINITE_PARAMETER,
    -INFINITE_PARAMETER,
    INFINITE_PARAMETER_SQRT,
    -INFINITE_PARAMETER_SQRT,
];

/// An extent: usually ordered, sometimes inverted, point sized, `Init`, or with
/// unbounded ends.
fn extent(rng: &mut Rng, scale: f64) -> E6 {
    let r = rng.unit();
    if r < 0.05 {
        return ext6(&Extent3d::new());
    }
    let a = point(rng, scale);
    if r < 0.12 {
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
    if rng.chance(0.2) {
        for (l, h) in lo.iter_mut().zip(hi.iter_mut()) {
            if rng.chance(0.4) {
                *l = -INFINITE_PARAMETER;
            }
            if rng.chance(0.4) {
                *h = INFINITE_PARAMETER;
            }
        }
        if rng.chance(0.15) {
            let k = rng.below(6) as usize;
            let v = *rng.choose(&SENTINELS).unwrap_or(&INFINITE_PARAMETER);
            if let Some(c) = lo.iter_mut().chain(hi.iter_mut()).nth(k) {
                *c = v;
            }
        }
    }
    join(lo, hi)
}

fn join(lo: P3, hi: P3) -> E6 {
    [lo[0], lo[1], lo[2], hi[0], hi[1], hi[2]]
}

fn halves(e: &E6) -> (P3, P3) {
    ([e[0], e[1], e[2]], [e[3], e[4], e[5]])
}

/// A point related to `e`: a corner, a face point, slightly off a face, or random.
fn point_near(rng: &mut Rng, e: &E6, scale: f64) -> P3 {
    let r = rng.unit();
    if r < 0.3 {
        return point(rng, scale);
    }
    let (lo, hi) = halves(e);
    let mut p = [0.0; 3];
    for ((c, l), h) in p.iter_mut().zip(lo).zip(hi) {
        *c = match rng.below(5) {
            0 => l,
            1 => h,
            2 => l + rng.uniform(0.0, 1.0) * (h - l),
            3 => {
                l - *rng
                    .choose(&[1e-13, 1e-12, 2e-12, 1e-6, 0.5])
                    .unwrap_or(&0.0)
            }
            _ => {
                h + *rng
                    .choose(&[1e-13, 1e-12, 2e-12, 1e-6, 0.5])
                    .unwrap_or(&0.0)
            }
        };
    }
    p
}

fn tol(rng: &mut Rng) -> f64 {
    *rng.choose(&[0.0, 0.0, 1e-12, 1e-9, 1e-6, 0.01, 0.5, -1e-6])
        .unwrap_or(&0.0)
}

fn direction(rng: &mut Rng) -> P3 {
    let mut v = [0.0; 3];
    for c in &mut v {
        *c = match rng.below(6) {
            0 => 0.0,
            1 => rng.uniform(-1e-12, 1e-12),
            2 => *rng.choose(&[1.0, -1.0, 2e-12, -2e-12]).unwrap_or(&1.0),
            _ => rng.uniform(-3.0, 3.0),
        };
    }
    if rng.chance(0.03) {
        v = [0.0; 3];
    }
    v
}

fn gen_inputs(rng: &mut Rng) -> Inputs {
    let scale = *rng.choose(&[1.0, 10.0, 1e-3, 1e4]).unwrap_or(&1.0);
    let e = extent(rng, scale);
    match rng.below(25) {
        0 => Inputs::FromPoints {
            a: point(rng, scale),
            b: point(rng, scale),
        },
        1 => Inputs::FromPoint {
            p: point(rng, scale),
        },
        2 => {
            let n = rng.below(6) as usize;
            let pts = (0..n).map(|_| point_near(rng, &e, scale)).collect();
            Inputs::AddPoints { e, pts }
        }
        3 => Inputs::SetMinMax {
            e,
            min: point(rng, scale),
            max: point(rng, scale),
        },
        4 => {
            let f = extent(rng, scale);
            Inputs::SetMinMaxCoords { e, c: f }
        }
        5 => {
            let which = rng.below(4) as u8;
            let v = if rng.chance(0.5) {
                e.get(usize::from(which % 2) * 3 + usize::from(which / 2))
                    .copied()
                    .unwrap_or(0.0)
            } else {
                coord(rng, scale)
            };
            Inputs::SetBound { e, which, v }
        }
        6 | 7 => Inputs::Queries { e, tol: tol(rng) },
        8 => {
            let mut n = [0.0; 3];
            for c in &mut n {
                *c = *rng
                    .choose(&[0.0, 1.0, 0.5, 0.25, 1.0 / 3.0, -0.5, 1.5])
                    .unwrap_or(&0.0);
                if rng.chance(0.3) {
                    *c = rng.unit();
                }
            }
            Inputs::Evaluate { e, n }
        }
        9 => Inputs::Union {
            e,
            f: extent(rng, scale),
        },
        10 => Inputs::Intersect {
            e,
            f: extent(rng, scale),
        },
        11 => Inputs::Distances {
            e,
            f: extent(rng, scale),
        },
        12 => Inputs::ExpandRelative {
            e,
            factor: rng.uniform(-1.0, 2.0),
        },
        13 => Inputs::ExpandAbsolute {
            e,
            amount: rng.uniform(-1.0, 1.0) * scale,
        },
        14 => Inputs::CornersEdgesPlanes { e },
        15 => Inputs::SphereBound { e },
        16 => {
            let f = if rng.chance(0.2) {
                e
            } else {
                extent(rng, scale)
            };
            Inputs::ConeBoundTo { e, f }
        }
        17 => Inputs::DisjointContained {
            e,
            f: extent(rng, scale),
            tol: tol(rng),
        },
        18 => Inputs::PointPredicates {
            p: point_near(rng, &e, scale),
            e,
            tol: tol(rng),
        },
        19 => Inputs::ContainsLineSeg {
            p: point_near(rng, &e, scale),
            q: point_near(rng, &e, scale),
            e,
            tol: tol(rng),
        },
        20 | 21 => {
            let p = point_near(rng, &e, scale);
            let v = if rng.chance(0.4) {
                // Aim at a point of the box (often a corner or edge: grazing).
                let q = point_near(rng, &e, scale);
                [q[0] - p[0], q[1] - p[1], q[2] - p[2]]
            } else {
                direction(rng)
            };
            Inputs::IntersectLine { e, p, v }
        }
        22 => Inputs::ApproximateUnbounded {
            e,
            center: rng.chance(0.6).then(|| point(rng, scale)),
            half_size: *rng.choose(&[33.0, 1.0, 10.0, 0.0, -5.0]).unwrap_or(&33.0),
        },
        23 => {
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
        _ => Inputs::SetUnbounded { e },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.extent3d"
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
