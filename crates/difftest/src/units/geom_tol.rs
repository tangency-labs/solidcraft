//! Runner for unit `geom.tol`: tolerance values and the `SmTol` tolerance policy, and
//! the `SmRelation` pair container.
//!
//! C++: `solidcore_ffi::units::geom_tol` (upstream `SmTol`, `SmTol3d` classes,
//! `SmRelation`; `source/SMLib/inc/SmTol.h`, `source/SMLib/src/SmTol.cpp`,
//! `source/SMLib/inc/SmRelation.h`). Rust: `solidcore::geom::tol`.
//!
//! A tolerance case is one operation `op` with the system tolerances set from
//! `model` (`SmTol::SetModelSizeEstimate`) and `ratio`
//! (`SmTol::SetLargeSmallSizeRatio`); the Rust kernel gets the same values. The
//! result is `r` (floats; booleans as `1.0`/`0.0`).
//!
//! A relation case (`op = "relation"`) runs a script of `RelatePair` /
//! `RelateToMany` / `RemoveAll` steps on objects identified by index and reports, as
//! integers `r`, every step's `SmStatus` (`SM_SUCCESS` = 1000; `RemoveAll` reports 0) and then every query (`GetSecond`,
//! `GetSeconds`, `GetAllFirsts`, `GetAllSeconds`). Upstream returns the sets in
//! hash (address) order, which is unspecified, so both sides report them sorted
//! (see `plan/findings.md`).
//!
//! A curve case (`op = "curve"`) runs the `SmCurve` overloads of `GetScaledZero`,
//! `MapTo1d(.., dParam, ..)` and `IsDegenerate(.., SmExtent1d, ..)` on a real line,
//! circle or B-spline curve (`curve`), with a parameter and an interval inside the
//! curve's range or overhanging it (the evaluators clamp or extend), a zero-length or an
//! inverted interval, tolerances around the intersection tolerance, and an optional
//! override; the result is `r = [scaled zero, tol1d, degenerate]`.
//!
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::extent3d::Extent3d;
use solidcore::geom::tol::{self, Cardinality, Relation, ScaledZero, SmallTopoType};
use solidcore::geom::vector2d::Vec2;
use solidcore::kernel::{Kernel, KernelOptions};
use solidcore::math::{Vec3, EFF_ZERO, PI};
use solidcore::nurbs::math::NOW;
use solidcore::SolidError;
use solidcore_ffi::units::geom_tol::{self as ffi, Op, OpArgs, RelationStep};

use super::intersect_surface_drop_lib_p2::{
    add3, ffi_curve, gen_knots, gen_scale, pick, rust_curve, scale3, unit_dir, CurveIn,
};
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `geom.tol` runner.
pub struct Runner;

/// Operation names used in `inputs.op` (plus `"relation"`).
const OPS: [(&str, Op); 25] = [
    ("size_estimate", Op::SizeEstimate),
    ("in_tol", Op::InTol),
    ("calc_face_zone", Op::CalcFaceZone),
    ("calc_edge_zone", Op::CalcEdgeZone),
    ("calc_vertex_zone", Op::CalcVertexZone),
    ("eff_zeros", Op::EffZeros),
    ("xsect_from_zones", Op::XSectFromZones),
    ("ang_tol_rad", Op::AngTolRad),
    ("model_size", Op::ModelSize),
    ("are_parallel", Op::AreParallel),
    ("are_perpendicular", Op::ArePerpendicular),
    ("is_zero_speed", Op::IsZeroSpeed),
    ("is_degenerate_vec3", Op::IsDegenerateVec3),
    ("gap_gain", Op::GapGain),
    ("zone_sys", Op::ZoneSys),
    ("zone_ctx", Op::ZoneCtx),
    ("approx_sys", Op::ApproxSys),
    ("approx_ctx", Op::ApproxCtx),
    ("xsect_sys", Op::XSectSys),
    ("xsect_ctx", Op::XSectCtx),
    ("scaled_zero_sys", Op::ScaledZeroSys),
    ("scaled_zero_n", Op::ScaledZeroN),
    ("tol_ctors", Op::TolCtors),
    ("tol_mul", Op::TolMul),
    ("map_to_1d", Op::MapTo1d),
];

/// Name of the relation pseudo-op.
const RELATION: &str = "relation";

/// Name of the curve pseudo-op: `GetScaledZero(const SmCurve&)`, `MapTo1d(.., dParam,
/// const SmCurve&)` and `IsDegenerate(const SmCurve*, SmExtent1d, ..)` on `curve`, with
/// `a = [param, tol, ivl0, ivl1, xsect]` (`flag`: pass `xsect`).
const CURVE: &str = "curve";

fn use_default() -> f64 {
    tol::USE_DEFAULT
}

/// An object of `scaled_zero_n`: `kind` 0 = double, 1 = `SmVector2d`,
/// 2 = `SmVector3d`, 3 = `SmExtent3d`, 4 = `SmExtent1d`, 5 = `SmExtent2d`; `d` holds its coordinates.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Obj {
    kind: i32,
    d: Vec<f64>,
}

/// A relation script step.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Step {
    Pair(i64, i64),
    Many(i64, Vec<i64>),
    RemoveAll,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default = "use_default")]
    model: f64,
    #[serde(default = "use_default")]
    ratio: f64,
    #[serde(default)]
    a: Vec<f64>,
    #[serde(default)]
    flag: bool,
    #[serde(default)]
    small: i32,
    #[serde(default)]
    objs: Vec<Obj>,
    #[serde(default)]
    card: i32,
    #[serde(default)]
    n1: i64,
    #[serde(default)]
    n2: i64,
    #[serde(default)]
    steps: Vec<Step>,
    #[serde(default)]
    curve: Option<CurveIn>,
}

fn lookup(name: &str) -> Result<Op, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn arg(i: &Inputs, k: usize) -> f64 {
    i.a.get(k).copied().unwrap_or(0.0)
}

fn coord(o: &Obj, k: usize) -> f64 {
    o.d.get(k).copied().unwrap_or(0.0)
}

// ---- C++ side ---------------------------------------------------------------------

fn cpp_tol(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    if i.a.len() > 8 || i.objs.len() > 4 {
        return Err(HarnessError::new("too many operands"));
    }
    let mut args = OpArgs {
        model_size: i.model,
        ratio: i.ratio,
        flag: i.flag,
        small: i.small,
        ..OpArgs::default()
    };
    for (dst, src) in args.a.iter_mut().zip(&i.a) {
        *dst = *src;
    }
    if op == Op::ScaledZeroN {
        args.small = i32::try_from(i.objs.len()).unwrap_or(0);
        for (j, o) in i.objs.iter().enumerate() {
            if let Some(k) = args.kinds.get_mut(j) {
                *k = o.kind;
            }
            for c in 0..6 {
                if let Some(dst) = args.objs.get_mut(j * 6 + c) {
                    *dst = coord(o, c);
                }
            }
        }
    }
    match ffi::op(op, &args) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

fn steps_ffi(i: &Inputs) -> Vec<RelationStep> {
    i.steps
        .iter()
        .map(|s| match s {
            Step::Pair(f, s) => RelationStep::Pair(*f, *s),
            Step::Many(f, ss) => RelationStep::Many(*f, ss.clone()),
            Step::RemoveAll => RelationStep::RemoveAll,
        })
        .collect()
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if i.op == RELATION {
        return match ffi::relation(i.card, i.n1, i.n2, &steps_ffi(i)) {
            Ok(v) => Ok(Output::new().ints("r", v)),
            Err(e) => Err(HarnessError::new(e.to_string())),
        };
    }
    if i.op == CURVE {
        return cpp_curve(i);
    }
    cpp_tol(i)
}

fn curve_operands(i: &Inputs) -> Result<(&CurveIn, [f64; 5]), HarnessError> {
    let c = i
        .curve
        .as_ref()
        .ok_or_else(|| HarnessError::new("curve case without curve"))?;
    let mut a = [0.0; 5];
    for (dst, src) in a.iter_mut().zip(&i.a) {
        *dst = *src;
    }
    Ok((c, a))
}

fn cpp_curve(i: &Inputs) -> Result<Output, HarnessError> {
    let (c, a) = curve_operands(i)?;
    let xsect = i.flag.then_some(a[4]);
    match ffi::curve_tols(&ffi_curve(c), a[0], a[1], [a[2], a[3]], xsect) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

// ---- Rust side ----------------------------------------------------------------------

/// The kernel `SmTol::SetModelSizeEstimate(model)` +
/// `SmTol::SetLargeSmallSizeRatio(ratio)` would leave behind.
fn kernel(model: f64, ratio: f64) -> Kernel {
    let model = if model == tol::USE_DEFAULT {
        tol::MODEL_SIZE_ESTIMATE
    } else {
        model
    };
    let ratio = if ratio == tol::USE_DEFAULT {
        tol::LARGE_SMALL_SIZE_RATIO
    } else {
        ratio
    };
    let zone = tol::size_estimate_to_zone_tol_3d(model);
    let mut o = KernelOptions::default();
    o.tolerances.model_size_estimate = model;
    o.tolerances.zone_tol_3d = zone;
    o.tolerances.xsect_tol_3d = zone * 2.0;
    o.tolerances.approx_tol_3d = zone / 2.0;
    o.tolerances.large_small_size_ratio = ratio;
    Kernel::new(&o)
}

fn object(o: &Obj) -> Box<dyn ScaledZero> {
    let c = |k| coord(o, k);
    match o.kind {
        1 => Box::new(Vec2::new(c(0), c(1))),
        2 => Box::new(Vec3::new(c(0), c(1), c(2))),
        3 => Box::new(Extent3d::from_bounds(c(0), c(1), c(2), c(3), c(4), c(5))),
        4 => Box::new(Extent1d::from_min_max(c(0), c(1))),
        5 => Box::new(Extent2d::from_bounds(c(0), c(1), c(2), c(3))),
        _ => Box::new(c(0)),
    }
}

fn scaled_zero_n(objs: &[Obj]) -> Result<Vec<f64>, HarnessError> {
    let o: Vec<Box<dyn ScaledZero>> = objs.iter().map(object).collect();
    match o.as_slice() {
        [a] => Ok(vec![
            a.scaled_zero(),
            tol::scaled_zero_sq_of(a.as_ref()),
            tol::scaled_zero_sqrt_of(a.as_ref()),
        ]),
        [a, b] => Ok(vec![
            tol::scaled_zero2(a.as_ref(), b.as_ref()),
            tol::scaled_zero_sq2(a.as_ref(), b.as_ref()),
            tol::scaled_zero_sqrt2(a.as_ref(), b.as_ref()),
        ]),
        [a, b, c] => Ok(vec![
            tol::scaled_zero3(a.as_ref(), b.as_ref(), c.as_ref()),
            tol::scaled_zero_sq3(a.as_ref(), b.as_ref(), c.as_ref()),
            tol::scaled_zero_sqrt3(a.as_ref(), b.as_ref(), c.as_ref()),
        ]),
        [a, b, c, d] => Ok(vec![
            tol::scaled_zero4(a.as_ref(), b.as_ref(), c.as_ref(), d.as_ref()),
            tol::scaled_zero_sq4(a.as_ref(), b.as_ref(), c.as_ref(), d.as_ref()),
            tol::scaled_zero_sqrt4(a.as_ref(), b.as_ref(), c.as_ref(), d.as_ref()),
        ]),
        _ => Err(HarnessError::new("scaled_zero_n needs 1..=4 objects")),
    }
}

fn rust_values(op: Op, i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let k = kernel(i.model, i.ratio);
    let a = |n| arg(i, n);
    // only the ops that take it parse `small` (the C++ side replaces it by the
    // object count for `scaled_zero_n`)
    let small = || {
        SmallTopoType::from_code(i.small)
            .ok_or_else(|| HarnessError::new(format!("small {}", i.small)))
    };
    let v1 = |x: f64| Ok(vec![x]);
    let vec3 = |n: usize| Vec3::new(a(n), a(n + 1), a(n + 2));
    Ok(match op {
        Op::SizeEstimate => v1(tol::size_estimate_to_zone_tol_3d(a(0))),
        Op::InTol => v1(flag(tol::in_tol(a(0), tol::tol3d(a(1))))),
        Op::CalcFaceZone => v1(tol::calc_face_zone_tol_3d(&k, None)),
        Op::CalcEdgeZone => v1(tol::calc_edge_zone_tol_3d(
            &k,
            a(0),
            a(1),
            i.flag.then(|| tol::zone_tol3d(&k, a(2))),
            None,
        )),
        Op::CalcVertexZone => v1(tol::calc_vertex_zone_tol_3d(
            &k,
            a(0),
            a(1),
            i.flag.then(|| tol::zone_tol3d(&k, a(2))),
            None,
        )),
        Op::EffZeros => Ok(vec![
            tol::eff_zero(),
            tol::eff_zero_sq(),
            tol::eff_zero_param(),
            tol::eff_zero_ang_rad(),
        ]),
        Op::XSectFromZones => v1(tol::xsect_tol_3d_from_zones(
            &k,
            tol::zone_tol3d(&k, a(0)),
            tol::zone_tol3d(&k, a(1)),
        )),
        Op::AngTolRad => v1(tol::ang_tol_rad()),
        Op::ModelSize => v1(tol::model_size_estimate(&k)),
        Op::AreParallel => Ok(vec![flag(tol::are_parallel(
            vec3(0),
            vec3(3),
            i.flag.then(|| a(6)),
        ))]),
        Op::ArePerpendicular => Ok(vec![flag(tol::are_perpendicular(
            vec3(0),
            vec3(3),
            i.flag.then(|| a(6)),
        ))]),
        Op::IsZeroSpeed => v1(flag(tol::is_zero_speed(
            a(0),
            a(1),
            i.flag.then(|| tol::tol3d(a(2))),
        ))),
        Op::IsDegenerateVec3 => v1(flag(tol::is_degenerate_vec3(
            &k,
            vec3(0),
            i.flag.then(|| tol::xsect_tol3d(&k, a(3))),
        ))),
        Op::GapGain => v1(tol::gap_gain(&k)),
        Op::ZoneSys => v1(tol::zone_tol_3d(&k, small()?)),
        Op::ZoneCtx => v1(tol::context_zone_tol_3d(&k, None, small()?)),
        Op::ApproxSys => v1(tol::approx_tol_3d(&k, small()?)),
        Op::ApproxCtx => v1(tol::context_approx_tol_3d(&k, None, small()?)),
        Op::XSectSys => v1(tol::xsect_tol_3d(&k, small()?)),
        Op::XSectCtx => v1(tol::context_xsect_tol_3d(&k, None, small()?)),
        Op::ScaledZeroSys => Ok(vec![
            tol::scaled_zero(),
            tol::scaled_zero_sq(),
            tol::scaled_zero_sqrt(),
        ]),
        Op::ScaledZeroN => Ok(scaled_zero_n(&i.objs)?),
        Op::TolCtors => {
            let d = a(0);
            let zone = tol::zone_tol3d(&k, d);
            Ok(vec![
                tol::tol3d(d),
                zone,
                tol::xsect_tol3d(&k, d),
                tol::approx_tol3d(&k, d),
                tol::tol3d(d),
                tol::tol3d(d),
                tol::tol3d(d),
                // operator=(double) of every SM_TOL_CALLS class
                tol::tol3d(d),
                tol::tol3d(d),
                tol::tol3d(d),
                tol::tol3d(d),
                tol::tol3d(d),
                tol::tol3d(d),
                // SmTol3d::operator=(const SmTol3d)
                zone,
            ])
        }
        Op::TolMul => v1(tol::tol_mul(tol::tol3d(a(0)), a(1))),
        Op::MapTo1d => v1(tol::map_to_1d(
            tol::tol3d(a(0)),
            vec3(1),
            &Extent1d::from_min_max(a(4), a(5)),
        )),
    })
}

/// Upstream `SM_SUCCESS`.
const SM_SUCCESS: i64 = 1000;

/// The upstream `SmStatus` of a relation call.
fn status(r: &solidcore::Result<()>) -> i64 {
    match r {
        Ok(()) => SM_SUCCESS,
        Err(SolidError::Upstream { code, .. }) => i64::from(*code),
        Err(_) => -1,
    }
}

fn rust_relation(i: &Inputs) -> Result<Output, HarnessError> {
    let card = Cardinality::from_code(i.card)
        .ok_or_else(|| HarnessError::new(format!("cardinality {}", i.card)))?;
    let mut rel: Relation<i64, i64> = Relation::new(card);
    let mut out = Vec::new();
    for step in &i.steps {
        out.push(match step {
            Step::Pair(f, s) => status(&rel.relate_pair(*f, *s)),
            Step::Many(f, ss) => status(&rel.relate_to_many(*f, ss)),
            Step::RemoveAll => {
                rel.remove_all();
                0
            }
        });
    }
    let push_list = |out: &mut Vec<i64>, v: Vec<i64>| {
        out.push(v.len() as i64);
        out.extend(v);
    };
    // the object counts are harness data: same bound as the FFI guard
    if !(0..=10_000).contains(&i.n1) || !(0..=10_000).contains(&i.n2) {
        return Err(HarnessError::new("object count"));
    }
    for f in 0..i.n1 {
        out.push(rel.second(f).unwrap_or(-1));
        push_list(&mut out, rel.seconds(f));
    }
    push_list(&mut out, rel.all_firsts());
    push_list(&mut out, rel.all_seconds());
    Ok(Output::new().ints("r", out))
}

fn rust_curve_case(i: &Inputs) -> Result<Output, HarnessError> {
    let (c, a) = curve_operands(i)?;
    let k = kernel(i.model, i.ratio);
    let curve = rust_curve(&k, c)?;
    let ivl = Extent1d::from_min_max(a[2], a[3]);
    let xsect = i.flag.then_some(tol::tol3d(a[4]));
    let r = (|| -> solidcore::Result<Vec<f64>> {
        Ok(vec![
            tol::scaled_zero_of_curve(&k, &curve)?,
            tol::map_to_1d_on_curve(&k, tol::tol3d(a[1]), a[0], &curve)?,
            flag(tol::is_degenerate_curve(&k, Some(&curve), &ivl, xsect)?),
        ])
    })();
    match r {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => {
            Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
        }
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    if i.op == RELATION {
        return rust_relation(i);
    }
    if i.op == CURVE {
        return rust_curve_case(i);
    }
    let op = lookup(&i.op)?;
    match rust_values(op, i)? {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => {
            Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
        }
    }
}

// ---- generation ---------------------------------------------------------------------

/// A model size estimate: table boundaries, the default sentinel, zero, negatives and
/// log-uniform values over many decades.
fn model_size(rng: &mut Rng) -> f64 {
    const EDGES: [f64; 10] = [
        0.00005, 0.005, 0.5, 500.0, 50000.0, 5e6, 50.0, 0.0, 1.0, 1e-5,
    ];
    let r = rng.unit();
    if r < 0.30 {
        tol::USE_DEFAULT
    } else if r < 0.50 {
        let e = rng.choose(&EDGES).copied().unwrap_or(50.0);
        if rng.chance(0.5) {
            e
        } else {
            e * (1.0 + rng.uniform(-1e-12, 1e-12))
        }
    } else {
        let v = 10f64.powf(rng.uniform(-9.0, 12.0));
        if rng.chance(0.1) {
            -v
        } else {
            v
        }
    }
}

fn ratio(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.5 {
        tol::USE_DEFAULT
    } else if r < 0.6 {
        -rng.uniform(0.0, 5000.0)
    } else if r < 0.65 {
        0.0
    } else {
        10f64.powf(rng.uniform(-3.0, 6.0))
    }
}

/// A tolerance-like scalar: zero, the sentinels, tolerance magnitudes, ordinary and
/// huge values, both signs.
fn scalar(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    let sign = if rng.chance(0.25) { -1.0 } else { 1.0 };
    if r < 0.08 {
        0.0
    } else if r < 0.14 {
        tol::USE_DEFAULT
    } else if r < 0.18 {
        tol::UNINIT_TOL
    } else if r < 0.50 {
        sign * 10f64.powf(rng.uniform(-14.0, -2.0))
    } else if r < 0.85 {
        sign * rng.uniform(0.0, 100.0)
    } else {
        sign * 10f64.powf(rng.uniform(2.0, 15.0))
    }
}

fn vector(rng: &mut Rng, len_scale: f64) -> [f64; 3] {
    let t = rng.uniform(0.0, 2.0 * PI);
    let p = rng.uniform(-PI / 2.0, PI / 2.0);
    let len = len_scale * rng.uniform(0.5, 1.5);
    [
        len * p.cos() * t.cos(),
        len * p.cos() * t.sin(),
        len * p.sin(),
    ]
}

fn obj(rng: &mut Rng) -> Obj {
    let kind = rng.below(5) as i32;
    let n = match kind {
        0 => 1,
        1 | 4 => 2,
        2 => 3,
        _ => 6,
    };
    let scale = 10f64.powf(rng.uniform(-3.0, 8.0));
    let mut d: Vec<f64> = (0..n).map(|_| rng.uniform(-1.0, 1.0) * scale).collect();
    if kind == 3 {
        for c in 0..3 {
            let lo = d.get(c).copied().unwrap_or(0.0);
            let hi = d.get(c + 3).copied().unwrap_or(0.0);
            if lo > hi {
                d.swap(c, c + 3);
            }
        }
    }
    Obj { kind, d }
}

fn relation_case(rng: &mut Rng) -> Inputs {
    let n1 = 1 + rng.below(6) as i64;
    let n2 = 1 + rng.below(6) as i64;
    let n_steps = 1 + rng.below(10);
    let steps = (0..n_steps)
        .map(|_| {
            let r = rng.unit();
            if r < 0.6 {
                Step::Pair(rng.range_i64(0, n1 - 1), rng.range_i64(0, n2 - 1))
            } else if r < 0.92 {
                let k = rng.below(4) as usize;
                Step::Many(
                    rng.range_i64(0, n1 - 1),
                    (0..k).map(|_| rng.range_i64(0, n2 - 1)).collect(),
                )
            } else {
                Step::RemoveAll
            }
        })
        .collect();
    Inputs {
        op: RELATION.into(),
        model: tol::USE_DEFAULT,
        ratio: tol::USE_DEFAULT,
        a: Vec::new(),
        flag: false,
        small: 0,
        objs: Vec::new(),
        card: rng.below(4) as i32,
        n1,
        n2,
        steps,
        curve: None,
    }
}

/// A curve case: a real line, circle or B-spline curve (or a point curve), a parameter
/// and interval inside or just outside its range (the evaluators clamp or extend), a
/// tolerance, and an intersection tolerance around the curve's length.
fn curve_case(rng: &mut Rng) -> Inputs {
    let s = gen_scale(rng);
    let xs = tol::xsect_tol_3d(&Kernel::default(), SmallTopoType::NormalSize);
    let org = scale3(unit_dir(rng), s * pick(rng, &[0.0, 1.0, 10.0], 0.0));
    let (curve, range) = match rng.below(5) {
        0 | 1 => {
            // a line; sometimes about as long as the intersection tolerance
            let len = if rng.chance(0.4) {
                xs * rng.uniform(0.2, 5.0)
            } else {
                s * pick(rng, &[1.0, 0.1, 10.0], 1.0)
            };
            let b = add3(org, scale3(unit_dir(rng), len));
            (CurveIn::Line { a: org, b }, [0.0, 1.0])
        }
        2 => {
            let t0 = pick(rng, &[0.0, -1.0, 2.0], 0.0);
            let t1 = t0 + pick(rng, &[1.0, 3.0, 6.0], 1.0);
            let r = if rng.chance(0.3) {
                xs * rng.uniform(0.2, 5.0)
            } else {
                s
            };
            (
                CurveIn::Circle {
                    c: org,
                    x: [1.0, 0.0, 0.0],
                    y: [0.0, 1.0, 0.0],
                    d: [t0, t1],
                    r,
                },
                [t0, t1],
            )
        }
        _ => {
            let n = 2 + rng.below(4) as usize;
            let deg = (n - 1).min(1 + rng.below(3) as usize);
            let spread = if rng.chance(0.3) {
                xs * rng.uniform(0.0, 3.0)
            } else {
                s
            };
            let cpts = (0..n)
                .map(|_| {
                    let q = add3(org, scale3(unit_dir(rng), spread * rng.uniform(0.0, 1.0)));
                    [q[0], q[1], q[2], NOW]
                })
                .collect();
            let t0 = pick(rng, &[0.0, -1.0, 2.0], 0.0);
            let t1 = t0 + pick(rng, &[1.0, 0.5, 10.0], 1.0);
            (
                CurveIn::Bspline {
                    p: deg,
                    cpts,
                    knots: gen_knots(rng, n, deg, t0, t1),
                },
                [t0, t1],
            )
        }
    };
    let w = range[1] - range[0];
    let at = |rng: &mut Rng, lo: f64, hi: f64| range[0] + rng.uniform(lo, hi) * w;
    let param = if rng.chance(0.15) {
        at(rng, -0.1, 1.1)
    } else {
        at(rng, 0.0, 1.0)
    };
    let (i0, i1) = match rng.below(6) {
        0 => (at(rng, 0.0, 0.4), at(rng, 0.6, 1.0)),
        1 => (at(rng, -0.1, 0.0), at(rng, 1.0, 1.1)),
        2 => {
            let x = at(rng, 0.0, 1.0);
            (x, x)
        }
        3 => (at(rng, 0.6, 1.0), at(rng, 0.0, 0.4)),
        _ => (range[0], range[1]),
    };
    let tol_v = xs * pick(rng, &[0.5, 1.0, 10.0, 1.0e-3], 1.0) * pick(rng, &[1.0, -1.0], 1.0);
    Inputs {
        op: CURVE.into(),
        model: tol::USE_DEFAULT,
        ratio: tol::USE_DEFAULT,
        a: vec![
            param,
            tol_v,
            i0,
            i1,
            xs * pick(rng, &[0.5, 1.0, 2.0, 1.0e3], 1.0),
        ],
        flag: rng.chance(0.4),
        small: 0,
        objs: Vec::new(),
        card: 0,
        n1: 0,
        n2: 0,
        steps: Vec::new(),
        curve: Some(curve),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    if rng.chance(0.12) {
        return relation_case(rng);
    }
    if rng.chance(0.12) {
        return curve_case(rng);
    }
    let idx = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS
        .get(idx)
        .copied()
        .unwrap_or(("size_estimate", Op::SizeEstimate));
    let model = model_size(rng);
    let ratio = ratio(rng);
    let zone = tol::size_estimate_to_zone_tol_3d(if model == tol::USE_DEFAULT {
        tol::MODEL_SIZE_ESTIMATE
    } else {
        model
    });
    let mut a: Vec<f64> = (0..8).map(|_| scalar(rng)).collect();
    let mut objs = Vec::new();
    let mut small = rng.below(5) as i32;
    match op {
        Op::SizeEstimate => {
            if rng.chance(0.6) {
                if let Some(x) = a.get_mut(0) {
                    *x = model_size(rng);
                }
            }
        }
        Op::InTol => {
            // |a0| close to the tolerance a1.
            if rng.chance(0.5) {
                let t = a.get(1).copied().unwrap_or(0.0).abs();
                let s = if rng.chance(0.5) { -1.0 } else { 1.0 };
                let e = match rng.below(3) {
                    0 => 0.0,
                    1 => 1e-15,
                    _ => -1e-15,
                };
                if let Some(x) = a.get_mut(0) {
                    *x = s * t * (1.0 + e);
                }
            }
        }
        Op::CalcEdgeZone | Op::CalcVertexZone => {
            for x in a.iter_mut().take(3) {
                if rng.chance(0.7) {
                    *x = zone * 10f64.powf(rng.uniform(-2.0, 2.0));
                }
            }
        }
        Op::AreParallel | Op::ArePerpendicular => {
            let sv = 10f64.powf(rng.uniform(-6.0, 3.0));
            let v = vector(rng, sv);
            let sw = 10f64.powf(rng.uniform(-6.0, 3.0));
            let w = vector(rng, sw);
            for (j, x) in v.iter().chain(w.iter()).enumerate() {
                if let Some(dst) = a.get_mut(j) {
                    *dst = *x;
                }
            }
        }
        Op::IsZeroSpeed => {
            if rng.chance(0.6) {
                let m = a.get(1).copied().unwrap_or(0.0);
                let z = EFF_ZERO * (1.0 + m.abs());
                if let Some(x) = a.get_mut(0) {
                    *x = z * rng.uniform(0.5, 1.5);
                }
            }
        }
        Op::IsDegenerateVec3 => {
            let sv = 2.0 * zone * 10f64.powf(rng.uniform(-1.0, 1.0));
            let v = vector(rng, sv);
            for (j, x) in v.iter().enumerate() {
                if let Some(dst) = a.get_mut(j) {
                    *dst = *x;
                }
            }
        }
        Op::MapTo1d => {
            // First derivative around the scaled zero (EFF_ZERO), or ordinary.
            let sv = if rng.chance(0.5) {
                EFF_ZERO * rng.uniform(0.3, 3.0)
            } else {
                10f64.powf(rng.uniform(-4.0, 3.0))
            };
            let v = vector(rng, sv);
            for (j, x) in v.iter().enumerate() {
                if let Some(dst) = a.get_mut(1 + j) {
                    *dst = if rng.chance(0.1) { 0.0 } else { *x };
                }
            }
        }
        Op::ScaledZeroN => {
            let n = 1 + rng.below(4) as usize;
            objs = (0..n).map(|_| obj(rng)).collect();
            small = 0;
        }
        _ => {}
    }
    Inputs {
        op: name.to_string(),
        model,
        ratio,
        a,
        flag: rng.chance(0.5),
        small,
        objs,
        card: 0,
        n1: 0,
        n2: 0,
        steps: Vec::new(),
        curve: None,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.tol"
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
