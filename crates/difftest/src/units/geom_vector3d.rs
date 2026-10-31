//! Runner for unit `geom.vector3d`: 3D vector arithmetic, angles, projections,
//! frames, unit-vector derivatives and the `SmMath.cpp` helpers.
//!
//! C++: `solidcore_ffi::units::geom_vector3d` (upstream `SmVector3d`,
//! `source/SMLib/inc/SmVector3d.h`, `source/SMLib/src/SmVector3d.cpp`, and `sm_sqrt`,
//! `sm_cbrt`, `sm_acos`, `sm_asin` of `source/SMLib/src/SmMath.cpp`).
//! Rust: `solidcore::geom::vector3d`.
//!
//! Every case is one operation `op` on vectors `a`..`e`, scalar `s`, index `idx` and
//! `flag`; the result is `r` (booleans as `1.0`/`0.0`). Errors are normalised to
//! `invalid_input` (`SM_ERR_INVALID_INPUT`) or `status_<code>` on both sides.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::vector2d::Vec2;
use solidcore::geom::vector3d::{self as v3, Vec3};
use solidcore::math::{EFF_ZERO, EFF_ZERO_SQ, PI, UNDEF_DOUBLE};
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::SolidError;
use solidcore_ffi::units::geom_vector3d::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 6000;

use solidcore::error::SM_ERR_INVALID_INPUT;
/// Upstream `SM_ERR`.
const SM_ERR: f64 = 1001.0;

/// `geom.vector3d` runner.
pub struct Runner;

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 59] = [
    ("length_squared", Op::LengthSquared),
    ("length", Op::Length),
    ("dot", Op::Dot),
    ("cross", Op::Cross),
    ("neg", Op::Neg),
    ("add", Op::Add),
    ("sub", Op::Sub),
    ("mul", Op::Mul),
    ("lmul", Op::LMul),
    ("div", Op::Div),
    ("unitize", Op::Unitize),
    ("unitize_or_z", Op::UnitizeOrZ),
    ("triple_product", Op::TripleProduct),
    ("normal_in_plane", Op::NormalInPlane),
    ("max_dimension", Op::MaxDimension),
    ("min_dimension", Op::MinDimension),
    ("parallel", Op::Parallel),
    ("perpendicular", Op::Perpendicular),
    ("project_to_plane", Op::ProjectToPlane),
    ("project_point_to_plane", Op::ProjectPointToPlane),
    ("rotate_project_to_plane", Op::RotateProjectToPlane),
    ("rotate_project_point_to_plane", Op::RotateProjectPointToPlane),
    ("perspective_project_to_plane", Op::PerspectiveProjectToPlane),
    (
        "perspective_project_point_to_plane",
        Op::PerspectiveProjectPointToPlane,
    ),
    ("distance", Op::Distance),
    ("distance_squared", Op::DistanceSquared),
    ("closer_than", Op::CloserThan),
    ("multiply", Op::Multiply),
    ("derivative_of_cross", Op::DerivativeOfCross),
    ("solve_two_linear", Op::SolveTwoLinear),
    ("index", Op::Index),
    ("index_set", Op::IndexSet),
    ("eq", Op::Eq),
    ("add_assign", Op::AddAssign),
    ("sub_assign", Op::SubAssign),
    ("mul_assign", Op::MulAssign),
    ("div_assign", Op::DivAssign),
    ("inside_sector", Op::InsideSector),
    ("unit_ortho", Op::UnitOrtho),
    ("rotate_about_axis", Op::RotateAboutAxis),
    ("unitized_derivative", Op::UnitizedDerivative),
    ("unitized_derivative2", Op::UnitizedDerivative2),
    ("unitized_derivative3", Op::UnitizedDerivative3),
    ("angle", Op::Angle),
    ("ccw_angle", Op::CcwAngle),
    ("colinear", Op::Colinear),
    ("is_initialized", Op::IsInitialized),
    ("is_undef", Op::IsUndef),
    ("is_zero", Op::IsZero),
    ("is_2d", Op::Is2d),
    ("from_nl", Op::FromNl),
    ("between", Op::Between),
    ("from_vec2", Op::FromVec2),
    ("copy", Op::Copy),
    ("uninitialized", Op::Uninitialized),
    ("sqrt", Op::Sqrt),
    ("cbrt", Op::Cbrt),
    ("acos", Op::Acos),
    ("asin", Op::Asin),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: [f64; 3],
    #[serde(default)]
    b: [f64; 3],
    #[serde(default)]
    c: [f64; 3],
    #[serde(default)]
    d: [f64; 3],
    #[serde(default)]
    e: [f64; 3],
    #[serde(default)]
    s: f64,
    #[serde(default)]
    idx: i32,
    #[serde(default)]
    flag: bool,
}

fn lookup(name: &str) -> Result<Op, HarnessError> {
    OPS.iter()
        .find(|(n, _)| *n == name)
        .map(|(_, op)| *op)
        .ok_or_else(|| HarnessError::new(format!("unknown op {name}")))
}

fn normalise(e: &SolidError) -> String {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => "invalid_input".into(),
        SolidError::Upstream { code, .. } => format!("status_{code}"),
        other => format!("rust_error: {other}"),
    }
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    match ffi::op(op, [i.a, i.b, i.c, i.d, i.e], i.s, i.idx, i.flag) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(FfiError::Status(SM_ERR_INVALID_INPUT)) => Ok(Output::err("invalid_input")),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

fn flat(vs: &[Vec3]) -> Vec<f64> {
    vs.iter().flat_map(|v| v.to_array()).collect()
}

/// `[status, vectors...]` for `UnitizedDerivative2`/`3`.
fn with_status<const N: usize>(r: Result<[Vec3; N], [Vec3; N]>) -> Vec<f64> {
    let (st, vs) = match r {
        Ok(vs) => (0.0, vs),
        Err(vs) => (SM_ERR, vs),
    };
    let mut out = vec![st];
    out.extend(flat(&vs));
    out
}

// `a * s` and `s * a` are different impls (`Mul<f64> for Vec3`, `Mul<Vec3> for f64`).
#[allow(clippy::match_same_arms, clippy::too_many_lines)]
fn rust_values(op: Op, i: &Inputs) -> solidcore::Result<Vec<f64>> {
    let a = Vec3::from_array(i.a);
    let b = Vec3::from(i.b);
    let c = Vec3::from(i.c);
    let d = Vec3::from(i.d);
    let e = Vec3::from(i.e);
    let s = i.s;
    let r3 = |v: Vec3| Ok(v.to_array().to_vec());
    let r1 = |x: f64| Ok(vec![x]);
    let idx = usize::try_from(i.idx).unwrap_or(usize::MAX);
    match op {
        Op::LengthSquared => r1(a.length_squared()),
        Op::Length => r1(a.length()),
        Op::Dot => r1(a.dot(b)),
        Op::Cross => r3(a.cross(b)),
        Op::Neg => r3(-a),
        Op::Add => r3(a + b),
        Op::Sub => r3(a - b),
        Op::Mul => r3(a * s),
        Op::LMul => r3(s * a),
        Op::Div => r3(a / s),
        Op::Unitize => r3(a.unitize()?),
        Op::UnitizeOrZ => r3(a.unitize_or_z()),
        Op::TripleProduct => r1(a.triple_product(b, c)),
        Op::NormalInPlane => r3(a.normal_in_plane(b)),
        Op::MaxDimension => r1(a.max_dimension()),
        Op::MinDimension => r1(a.min_dimension()),
        Op::Parallel => {
            let (p, anti) = a.parallel_sense(b, s);
            Ok(vec![flag(p), anti.map_or(-1.0, flag)])
        }
        Op::Perpendicular => r1(flag(a.is_perpendicular_to(b, s))),
        Op::ProjectToPlane => r3(a.project_to_plane(b)),
        Op::ProjectPointToPlane => r3(a.project_point_to_plane(b, c)),
        Op::RotateProjectToPlane => r3(a.rotate_project_to_plane(b, c)),
        Op::RotateProjectPointToPlane => r3(a.rotate_project_point_to_plane(b, c, d)),
        Op::PerspectiveProjectToPlane => r3(a.perspective_project_to_plane(b, c, d, e)),
        Op::PerspectiveProjectPointToPlane => r3(a.perspective_project_point_to_plane(b, c, d)),
        Op::Distance => r1(a.distance(b)),
        Op::DistanceSquared => r1(a.distance_squared(b)),
        Op::CloserThan => r1(flag(a.closer_than(s, b))),
        Op::Multiply => r3(a.multiply(b)),
        Op::DerivativeOfCross => r3(a.derivative_of_cross_product(b, c, d)),
        Op::SolveTwoLinear => {
            let p = a.solve_two_linear_equations(b)?;
            Ok(vec![p.x, p.y])
        }
        Op::Index => r1(a.component(idx)),
        Op::IndexSet => {
            let mut r = a;
            *r.component_mut(idx)? = s;
            r3(r)
        }
        Op::Eq => r1(flag(a.approx_eq(b))),
        Op::AddAssign => {
            let mut r = a;
            r += b;
            r3(r)
        }
        Op::SubAssign => {
            let mut r = a;
            r -= b;
            r3(r)
        }
        Op::MulAssign => {
            let mut r = a;
            r *= s;
            r3(r)
        }
        Op::DivAssign => {
            let mut r = a;
            r /= s;
            r3(r)
        }
        Op::InsideSector => r1(flag(a.is_inside_sector(b, c, d))),
        Op::UnitOrtho => Ok(flat(&a.make_unit_ortho_vectors(i.flag.then_some(b))?)),
        Op::RotateAboutAxis => r3(a.rotate_vec_about_axis(b, s)),
        Op::UnitizedDerivative => r3(a.unitized_derivative(b)),
        Op::UnitizedDerivative2 => Ok(with_status(a.unitized_derivative2(b, c))),
        Op::UnitizedDerivative3 => Ok(with_status(a.unitized_derivative3(b, c, d))),
        Op::Angle => r1(a.angle_between(b)?),
        Op::CcwAngle => r1(a.ccw_angle_between(b, c)?),
        Op::Colinear => r1(flag(a.is_colinear_with(b, c, d))),
        Op::IsInitialized => r1(flag(a.is_initialized())),
        Op::IsUndef => r1(flag(a.is_undef())),
        Op::IsZero => r1(flag(a.is_zero(s))),
        Op::Is2d => r1(flag(a.is_2d())),
        Op::FromNl => r3(Vec3::from_nl([i.a[0], i.a[1], i.a[2], s])),
        Op::Between => r3(Vec3::between(a, b)),
        Op::FromVec2 => r3(Vec3::from(Vec2::new(i.a[0], i.a[1]))),
        Op::Copy => {
            let mut c5 = a;
            let mut c6 = Vec3::new(1.0, 2.0, 3.0);
            core::mem::swap(&mut c6, &mut c5);
            r3(c6)
        }
        Op::Uninitialized => {
            let mut r = a;
            r.set_uninitialized();
            r3(r)
        }
        Op::Sqrt => r1(v3::sqrt(s)),
        Op::Cbrt => r1(v3::cbrt(s)),
        Op::Acos => r1(v3::acos(s)),
        Op::Asin => r1(v3::asin(s)),
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&i.op)?;
    match rust_values(op, i) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))),
    }
}

// ---- generation ---------------------------------------------------------------

/// A coordinate: mostly ordinary values, sometimes zero, tiny, huge, integers or
/// `SM_UNDEF_DOUBLE`.
fn coord(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.55 {
        rng.uniform(-10.0, 10.0)
    } else if r < 0.65 {
        0.0
    } else if r < 0.72 {
        rng.range_i64(-5, 5) as f64
    } else if r < 0.80 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(6, 14) as i32))
    } else if r < 0.86 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(rng.range_i64(3, 12) as i32)
    } else if r < 0.88 {
        UNDEF_DOUBLE
    } else {
        rng.uniform(-1.0, 1.0)
    }
}

fn unit(rng: &mut Rng) -> [f64; 3] {
    let z = rng.uniform(-1.0, 1.0);
    let t = rng.uniform(0.0, 2.0 * PI);
    let r = (1.0 - z * z).sqrt();
    [r * t.cos(), r * t.sin(), z]
}

fn scale(v: [f64; 3], k: f64) -> [f64; 3] {
    [v[0] * k, v[1] * k, v[2] * k]
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn axis_vector(rng: &mut Rng) -> [f64; 3] {
    let mut v = [0.0; 3];
    if let Some(slot) = v.get_mut(rng.below(3) as usize) {
        *slot = if rng.chance(0.5) { 1.0 } else { -1.0 } * rng.uniform(0.1, 5.0);
    }
    v
}

fn vector(rng: &mut Rng) -> [f64; 3] {
    let r = rng.unit();
    if r < 0.10 {
        // Unit or nearly-unit vector.
        let e = if rng.chance(0.5) {
            0.0
        } else {
            rng.uniform(-1.0, 1.0) * 1e-12
        };
        scale(unit(rng), 1.0 + e)
    } else if r < 0.15 {
        // Length near EFF_ZERO (the unitize / angle / derivative thresholds).
        scale(unit(rng), EFF_ZERO * rng.uniform(0.5, 1.5))
    } else if r < 0.22 {
        axis_vector(rng)
    } else {
        [coord(rng), coord(rng), coord(rng)]
    }
}

/// A vector related to `a`: parallel, anti-parallel, nearly so, perpendicular, close
/// (for `==` / `CloserThan`) or independent.
fn partner(rng: &mut Rng, a: [f64; 3]) -> [f64; 3] {
    let r = rng.unit();
    let k = rng.uniform(-3.0, 3.0);
    if r < 0.12 {
        scale(a, k)
    } else if r < 0.30 {
        // Nearly (anti)parallel: exercises the asin branch and tolerance edges.
        let eps = 10f64.powi(-(rng.range_i64(1, 16) as i32)) * rng.uniform(0.5, 2.0);
        let len = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
        let p = cross(a, unit(rng));
        let plen = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if plen > 0.0 {
            scale(add(a, scale(p, eps * len / plen)), k)
        } else {
            scale(a, k)
        }
    } else if r < 0.42 {
        let p = cross(a, unit(rng));
        let eps = if rng.chance(0.5) {
            0.0
        } else {
            rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(4, 14) as i32))
        };
        add(scale(p, k), scale(a, eps))
    } else if r < 0.54 {
        let eps = 10f64.powi(-(rng.range_i64(8, 16) as i32));
        [
            a[0] + rng.uniform(-1.0, 1.0) * eps,
            a[1] + rng.uniform(-1.0, 1.0) * eps,
            a[2] + rng.uniform(-1.0, 1.0) * eps,
        ]
    } else {
        vector(rng)
    }
}

/// An angle tolerance in degrees.
fn ang_tol_deg(rng: &mut Rng) -> f64 {
    match rng.below(4) {
        0 => 1e-4,
        1 => 10f64.powi(-(rng.range_i64(0, 12) as i32)),
        2 => rng.uniform(0.0, 90.0),
        _ => 0.000001,
    }
}

fn scalar(rng: &mut Rng, op: Op, a: [f64; 3], b: [f64; 3]) -> f64 {
    match op {
        Op::Parallel | Op::Perpendicular => ang_tol_deg(rng),
        Op::RotateAboutAxis => match rng.below(4) {
            0 => f64::from(rng.range_i64(-8, 8) as i32) * PI / 4.0,
            _ => rng.uniform(-10.0, 10.0),
        },
        Op::CloserThan => {
            let dd = (a[0] - b[0]) * (a[0] - b[0])
                + (a[1] - b[1]) * (a[1] - b[1])
                + (a[2] - b[2]) * (a[2] - b[2]);
            let d = dd.sqrt();
            match rng.below(4) {
                0 => d,
                1 => d * (1.0 + rng.uniform(-1e-12, 1e-12)),
                2 => (a[0] - b[0]).abs().max((a[1] - b[1]).abs()),
                _ => rng.uniform(-1.0, 10.0),
            }
        }
        Op::Div | Op::DivAssign => match rng.below(5) {
            0 => 0.0,
            1 => {
                let m = a[0].abs().max(a[1].abs()).max(a[2].abs());
                m * EFF_ZERO * rng.uniform(0.5, 1.5)
            }
            _ => coord(rng),
        },
        Op::IsZero => match rng.below(3) {
            0 => EFF_ZERO,
            1 => (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt() * rng.uniform(0.9, 1.1),
            _ => coord(rng).abs(),
        },
        Op::FromNl => match rng.below(4) {
            0 => NOW,
            1 => 1.0,
            _ => coord(rng),
        },
        Op::Sqrt => match rng.below(6) {
            0 => -EFF_ZERO_SQ * rng.uniform(0.5, 1.5),
            1 => 0.0,
            2 => -0.0,
            3 => -rng.uniform(0.0, 10.0),
            _ => rng.uniform(0.0, 100.0) * 10f64.powi(rng.range_i64(-20, 20) as i32),
        },
        Op::Cbrt => coord(rng) * 10f64.powi(rng.range_i64(-5, 5) as i32),
        Op::Acos | Op::Asin => match rng.below(6) {
            0 => 1.0,
            1 => -1.0,
            2 => rng.uniform(-1.5, 1.5),
            3 => {
                (if rng.chance(0.5) { 1.0 } else { -1.0 })
                    * (1.0 - 10f64.powi(-(rng.range_i64(1, 16) as i32)))
            }
            _ => rng.uniform(-1.0, 1.0),
        },
        _ => coord(rng),
    }
}

/// Vectors `b`..`e` for an operation, given `a`.
fn operands(rng: &mut Rng, op: Op, a: [f64; 3]) -> [[f64; 3]; 4] {
    let mut v = [
        partner(rng, a),
        vector(rng),
        vector(rng),
        vector(rng),
    ];
    match op {
        Op::ProjectToPlane | Op::RotateProjectToPlane => {
            v[0] = unit(rng);
            v[1] = if rng.chance(0.7) {
                let p = cross(v[0], unit(rng));
                let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
                if l > 0.0 {
                    scale(p, 1.0 / l)
                } else {
                    unit(rng)
                }
            } else {
                unit(rng)
            };
        }
        Op::ProjectPointToPlane | Op::RotateProjectPointToPlane => {
            v[1] = unit(rng);
            v[2] = unit(rng);
        }
        Op::PerspectiveProjectToPlane => {
            v[2] = unit(rng);
        }
        Op::PerspectiveProjectPointToPlane => {
            v[1] = unit(rng);
        }
        // Coplanar vectors (sometimes) in a random plane: sector bounds and binormal,
        // or the start and end vectors of a CCW angle.
        Op::InsideSector | Op::CcwAngle if rng.chance(0.7) => {
            let n = unit(rng);
            let u = cross(n, unit(rng));
            let w = cross(n, u);
            let mut inplane = || {
                let t = if rng.chance(0.2) {
                    f64::from(rng.range_i64(-4, 4) as i32) * PI / 2.0
                } else {
                    rng.uniform(-PI, PI)
                };
                let k = rng.uniform(0.1, 3.0);
                add(scale(u, k * t.cos()), scale(w, k * t.sin()))
            };
            v = [inplane(), inplane(), inplane(), v[3]];
        }
        Op::Colinear if rng.chance(0.6) => {
            let k = rng.uniform(-3.0, 3.0);
            v[1] = vector(rng);
            v[2] = add(v[1], scale(a, k));
            v[0] = partner(rng, a);
        }
        _ => {}
    }
    v
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx_op = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS.get(idx_op).copied().unwrap_or(("length", Op::Length));
    let mut a = vector(rng);
    let [b, c, d, e] = operands(rng, op, a);
    if op == Op::InsideSector && rng.chance(0.7) {
        // a = a combination of the bounds (inside or outside).
        let (k1, k2) = (rng.uniform(-1.0, 2.0), rng.uniform(-1.0, 2.0));
        a = add(scale(b, k1), scale(d, k2));
    }
    // NLib's 2D marker, set after the operands (which square `a`) are drawn.
    if (op == Op::Is2d && rng.chance(0.5)) || (op == Op::FromNl && rng.chance(0.3)) {
        a[2] = NOZ;
    }
    let s = scalar(rng, op, a, b);
    let idx = match op {
        Op::Index => rng.below(6) as i32,
        _ => rng.below(3) as i32,
    };
    Inputs {
        op: name.to_string(),
        a,
        b,
        c,
        d,
        e,
        s,
        idx,
        flag: rng.chance(0.6),
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.vector3d"
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
