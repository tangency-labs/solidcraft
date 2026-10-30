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
//! `RelateToMany` / `RemoveAll` / `RemovePair` steps on objects identified by index and reports, as
//! integers `r`, every step's `SmStatus` (`SM_SUCCESS` = 1000; `RemoveAll` reports 0) and then every query (`GetSecond`,
//! `GetSeconds`, `GetFirsts`, `GetAllFirsts`, `GetAllSeconds`). Upstream returns the sets in
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
//! A B-rep case (`op = "brep"`) runs the `SmBrep` overloads of `SmTol`
//! (`SetModelSizeEstimate` when `set_est`, `SetLargeSmallSizeRatio` when `set_ratio`,
//! then `GetModelSizeEstimate`, `GetLargeSmallSizeRatio`, `GetZoneTol3d`,
//! `GetApproxTol3d`, `GetXSectTol3d`, `CalcFaceZoneTol3d`, `CalcEdgeZoneTol3d` /
//! `CalcVertexZoneTol3d` with `a = [gap1, gap2, max]` (`flag`: pass `max`) and
//! `GetScaledZero`) on the `query.brep` fixture B-rep `brep` (a box solid or points and
//! wire segments), moved into a fresh context, with the system tolerances from `model`
//! and `ratio`; the Rust side imports the B-rep's export and uses a fresh kernel with
//! the same system tolerances. The result is `r` (10 values, the first 0 without
//! `set_est`). They are generated under their own seeds (`brep` cases).
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
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::spiral::allocate_nurb_volume;
use solidcore::surface::b_spline_volume::BSplineVolume;
use solidcore::surface::volume::{Volume, VolumeKind};
use solidcore::SolidError;
use solidcore_ffi::units::geom_tol::{self as ffi, Op, OpArgs, RelationStep};

use super::intersect_surface_drop_lib_p2::{
    add3, ffi_curve, gen_knots, gen_scale, pick, rust_curve, scale3, unit_dir, CurveIn,
};
use super::query_brep::{gen_box_solid, gen_points_brep};
use super::query_face_p1::own_model;
use super::topology_edgeuse_p1::fixture_input;
use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// Generated B-rep cases per run, whatever `--seeds` says.
pub const BREP_GENERATED: usize = 160;

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

/// Name of the B-rep pseudo-op (see the module docs).
const BREP: &str = "brep";

/// Name of the volume pseudo-op: `GetScaledZero(const SmVolume&)` on the supplied
/// eight Cartesian poles, k index fastest, with degree 1, knots 0011 and unit weights.
const VOLUME: &str = "volume";

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
    RemovePair(i64, i64),
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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    poles: Option<[[f64; 3]; 8]>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    brep: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    set_est: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    set_ratio: Option<f64>,
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
            Step::RemovePair(f, s) => RelationStep::RemovePair(*f, *s),
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
    if i.op == BREP {
        return cpp_brep(i);
    }
    if i.op == VOLUME {
        let p = volume_ffi_poles(i)?;
        return match ffi::volume_scaled_zero(&p) {
            Ok(v) => Ok(Output::new().values("scaled_zero_of_volume", v)),
            Err(e) => Err(HarnessError::new(e.to_string())),
        };
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

fn volume_operands(i: &Inputs) -> Result<&[[f64; 3]; 8], HarnessError> {
    i.poles
        .as_ref()
        .ok_or_else(|| HarnessError::new("volume case without poles"))
}

fn volume_ffi_poles(i: &Inputs) -> Result<[f64; 24], HarnessError> {
    let mut flat = [0.0; 24];
    for (dst, src) in flat.iter_mut().zip(volume_operands(i)?.iter().flatten()) {
        *dst = *src;
    }
    Ok(flat)
}

fn rust_volume(i: &Inputs) -> Result<Volume, HarnessError> {
    let poles = volume_operands(i)?;
    let mut nurb = allocate_nurb_volume([1; 3], [1; 3], [3; 3])
        .map_err(|e| HarnessError::new(e.to_string()))?;
    for dir in 0..3 {
        nurb.knots_mut(dir).copy_from_slice(&[0.0, 0.0, 1.0, 1.0]);
    }
    for (index, &[x, y, z]) in poles.iter().enumerate() {
        let dst = nurb
            .cpt_mut(index / 4, (index / 2) % 2, index % 2)
            .ok_or_else(|| HarnessError::new("volume control point missing"))?;
        *dst = CPoint::new(x, y, z, 1.0);
    }
    Ok(Volume::new(VolumeKind::BSpline(BSplineVolume::new(nurb))))
}

fn rust_volume_case(i: &Inputs) -> Result<Output, HarnessError> {
    let volume = rust_volume(i)?;
    let k = kernel(i.model, i.ratio);
    match tol::scaled_zero_of_volume(&k, &volume) {
        Ok(v) => Ok(Output::new().values("scaled_zero_of_volume", vec![v])),
        Err(e) => {
            Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
        }
    }
}

// ---- Rust side ----------------------------------------------------------------------

fn brep_args(i: &Inputs) -> ffi::BrepTolArgs {
    ffi::BrepTolArgs {
        set_estimate: i.set_est,
        set_ratio: i.set_ratio,
        small: i.small,
        gap1: arg(i, 0),
        gap2: arg(i, 1),
        max: i.flag.then(|| arg(i, 2)),
    }
}

fn brep_value(i: &Inputs) -> Result<&Value, HarnessError> {
    i.brep
        .as_ref()
        .ok_or_else(|| HarnessError::new("brep case without brep"))
}

fn cpp_brep(i: &Inputs) -> Result<Output, HarnessError> {
    let (path, block) = fixture_input(brep_value(i)?.clone())?;
    match ffi::brep_tols(&path, &block, i.model, i.ratio, &brep_args(i)) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

fn rust_brep(i: &Inputs) -> Result<Output, HarnessError> {
    // the Rust-built B-rep (no C++ export reaches the Rust side)
    let m = match own_model(brep_value(i)?)? {
        Ok(m) => m,
        Err(o) => return Ok(o),
    };
    let mut b = m.brep;
    let mut k = kernel(i.model, i.ratio);
    let small = SmallTopoType::from_code(i.small)
        .ok_or_else(|| HarnessError::new(format!("small {}", i.small)))?;
    let a = brep_args(i);
    let z = match a.set_estimate {
        Some(est) => tol::set_brep_model_size_estimate(&mut k, &mut b, est),
        None => 0.0,
    };
    if let Some(ratio) = a.set_ratio {
        tol::set_brep_large_small_size_ratio(&mut k, &mut b, ratio);
    }
    let mut r = vec![
        z,
        tol::brep_model_size_estimate(&k, &b),
        tol::brep_large_small_size_ratio(&k, &b),
        tol::brep_zone_tol_3d(&k, &b, small),
        tol::brep_approx_tol_3d(&k, &b, small),
        tol::brep_xsect_tol_3d(&k, &b, small),
        tol::brep_calc_face_zone_tol_3d(&k, &b),
        tol::brep_calc_edge_zone_tol_3d(&k, a.gap1, a.gap2, &b, a.max),
        tol::brep_calc_vertex_zone_tol_3d(&k, a.gap1, a.gap2, &b, a.max),
    ];
    match tol::scaled_zero_of_brep(&k, &b) {
        Ok(v) => r.push(v),
        Err(e) => {
            return Ok(Output::unported(&e)
                .unwrap_or_else(|| Output::err(format!("rust_error: {e}"))))
        }
    }
    Ok(Output::new().values("r", r))
}

/// A generated B-rep case (see the module docs).
fn brep_case(rng: &mut Rng) -> Inputs {
    let brep = if rng.chance(0.6) {
        gen_box_solid(rng)
    } else {
        gen_points_brep(rng, false)
    };
    let est = |rng: &mut Rng| -> f64 {
        const E: [f64; 12] = [
            tol::USE_DEFAULT, 0.00005, 0.003, 0.5, 0.7, 50.0, 500.0, 2000.0, 50000.0, 6e6, 0.0,
            -3.0,
        ];
        if rng.chance(0.7) {
            *rng.choose(&E).unwrap_or(&50.0)
        } else {
            10f64.powf(rng.uniform(-6.0, 8.0))
        }
    };
    let ratio = |rng: &mut Rng| -> f64 {
        *rng.choose(&[tol::USE_DEFAULT, 1.0, 10.0, 1000.0, 1e5, 0.0])
            .unwrap_or(&1000.0)
    };
    let gap = |rng: &mut Rng| -> f64 {
        if rng.chance(0.2) {
            0.0
        } else {
            10f64.powf(rng.uniform(-8.0, 0.0))
        }
    };
    let model = if rng.chance(0.5) { tol::USE_DEFAULT } else { est(rng) };
    let sys_ratio = ratio(rng);
    let set_est = rng.chance(0.75).then(|| est(rng));
    let set_ratio = rng.chance(0.4).then(|| ratio(rng));
    let a = vec![gap(rng), gap(rng), gap(rng)];
    Inputs {
        op: BREP.to_string(),
        model,
        ratio: sys_ratio,
        a,
        flag: rng.chance(0.5),
        small: rng.below(5) as i32,
        objs: Vec::new(),
        card: 0,
        n1: 0,
        n2: 0,
        steps: Vec::new(),
        curve: None,
        poles: None,
        brep: serde_json::to_value(brep).ok(),
        set_est,
        set_ratio,
    }
}

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
            Step::RemovePair(f, s) => status(&rel.remove_pair(*f, *s)),
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
    for s in 0..i.n2 {
        push_list(&mut out, rel.firsts(s));
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
    if i.op == VOLUME {
        return rust_volume_case(i);
    }
    if i.op == BREP {
        return rust_brep(i);
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
        poles: None,
        brep: None,
        set_est: None,
        set_ratio: None,
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
        poles: None,
        brep: None,
        set_est: None,
        set_ratio: None,
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
        poles: None,
        brep: None,
        set_est: None,
        set_ratio: None,
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
        let gen_ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &gen_ctx)?;
        // the B-rep cases under their own names and seeds, so that each `gen` case
        // stays the one the baseline has
        let brep_ctx = CaseContext {
            seeds: BREP_GENERATED,
            ..ctx.clone()
        };
        for mut c in brep_ctx.generated_cases("geom.tol:brep", |rng| {
            serde_json::to_value(brep_case(rng)).ok()
        }) {
            c.unit = self.id().to_string();
            c.name = c.name.replacen("gen", "brep", 1);
            v.push(c);
        }
        Ok(v)
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
    use crate::case::CaseSource;

    fn volume_fixtures() -> Vec<Case> {
        let file: Value = serde_json::from_str(include_str!(
            "../../../../fixtures/geom.tol/bspline-volume-scaled-zero.json"
        ))
        .unwrap();
        file["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|entry| Case {
                unit: "geom.tol".into(),
                name: entry["name"].as_str().unwrap().into(),
                source: CaseSource::Inline,
                inputs: entry["inputs"].clone(),
            })
            .collect()
    }

    fn volume_scalar(case: &Case) -> f64 {
        let out = Runner.run_rust(case).unwrap();
        assert!(out.error.is_none(), "{}: {:?}", case.name, out.error);
        assert!(out.deferred.is_none(), "{}: {:?}", case.name, out.deferred);
        assert!(out.rejected.is_none());
        assert!(out.ints.is_empty());
        assert_eq!(out.values.len(), 1);
        let values = &out.values["scaled_zero_of_volume"];
        assert_eq!(values.len(), 1);
        assert!(values[0].is_finite() && values[0] > 0.0);
        values[0]
    }

    #[test]
    fn volume_fixtures_return_positive_geometric_scaled_zeros() {
        let cases = volume_fixtures();
        let expected_factors = [2.0, 32.0, 9.0, 13.0, 3.0, 10.0];
        assert_eq!(cases.len(), expected_factors.len());
        for (case, factor) in cases.iter().zip(expected_factors) {
            let input: Inputs = case.parse().unwrap();
            let max_abs = volume_operands(&input)
                .unwrap()
                .iter()
                .flatten()
                .fold(0.0_f64, |max, x| max.max(x.abs()));
            assert_eq!(1.0 + max_abs, factor, "{}", case.name);
            let expected = EFF_ZERO * (1.0 + max_abs);
            let actual = volume_scalar(case);
            assert!(
                (actual - expected).abs() <= 1e-12 * expected.abs(),
                "{}: {actual} != {expected}",
                case.name
            );
        }
    }

    #[test]
    fn changing_only_max_corner_changes_volume_scaled_zero() {
        let cases = volume_fixtures();
        let before = cases
            .iter()
            .find(|c| c.name == "volume_max_corner_before")
            .unwrap();
        let after = cases
            .iter()
            .find(|c| c.name == "volume_max_corner_after")
            .unwrap();
        let a: Inputs = before.parse().unwrap();
        let b: Inputs = after.parse().unwrap();
        let a = volume_operands(&a).unwrap();
        let b = volume_operands(&b).unwrap();
        let changes = a
            .iter()
            .flatten()
            .zip(b.iter().flatten())
            .filter(|(a, b)| a != b)
            .count();
        assert_eq!(changes, 1);
        assert_eq!(a[7][2], 2.0);
        assert_eq!(b[7][2], 9.0);
        assert!(volume_scalar(after) > volume_scalar(before));
    }

    #[test]
    fn parsed_poles_reach_both_backends_in_k_fastest_order() {
        // Distinct coordinates expose swapped axes, omitted poles and constant cubes.
        let poles: [[f64; 3]; 8] = std::array::from_fn(|n| {
            [n as f64 + 0.25, -(n as f64) - 10.5, n as f64 + 100.75]
        });
        let case = Case {
            unit: "geom.tol".into(),
            name: "forward_all_poles".into(),
            source: CaseSource::Inline,
            inputs: serde_json::json!({"op": "volume", "poles": poles}),
        };
        let input: Inputs = case.parse().unwrap();
        let flat = volume_ffi_poles(&input).unwrap();
        assert_eq!(
            flat.to_vec(),
            poles.into_iter().flatten().collect::<Vec<_>>()
        );
        let volume = rust_volume(&input).unwrap();
        let VolumeKind::BSpline(bspline) = volume.kind() else {
            panic!("expected B-spline volume");
        };
        let nurb = bspline.nurb();
        assert_eq!(nurb.cp_hi(), [1; 3]);
        assert_eq!(nurb.degrees(), [1; 3]);
        for dir in 0..3 {
            assert_eq!(nurb.knots(dir), &[0.0, 0.0, 1.0, 1.0]);
        }
        for (index, [x, y, z]) in poles.into_iter().enumerate() {
            let point = nurb.cpt(index / 4, (index / 2) % 2, index % 2).unwrap();
            assert_eq!([point.x, point.y, point.z, point.w], [x, y, z, 1.0]);
        }
    }

    #[test]
    fn volume_requires_explicit_poles() {
        let input: Inputs = serde_json::from_value(serde_json::json!({"op": "volume"})).unwrap();
        assert!(volume_ffi_poles(&input).is_err());
        assert!(rust(&input).is_err());
        assert!(cpp(&input).is_err());
    }

    #[test]
    fn legacy_generated_cases_omit_optional_poles_without_consuming_rng() {
        for seed in [0, 1, 42, u64::MAX, CaseContext::seed_for("geom.tol", 0)] {
            let mut direct_rng = Rng::new(seed);
            let mut runner_rng = Rng::new(seed);
            for _ in 0..32 {
                let input = gen_case(&mut direct_rng);
                assert!(input.poles.is_none());
                let value = serde_json::to_value(&input).unwrap();
                assert!(value.get("poles").is_none());
                assert_eq!(Runner.generate(&mut runner_rng).unwrap(), value);
                let parsed: Inputs = serde_json::from_value(value.clone()).unwrap();
                assert_eq!(serde_json::to_value(parsed).unwrap(), value);
            }
            assert_eq!(direct_rng.next_u64(), runner_rng.next_u64());
        }
    }
}
