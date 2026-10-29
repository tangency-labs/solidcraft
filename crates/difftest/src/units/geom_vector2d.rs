//! Runner for unit `geom.vector2d`: 2D vector arithmetic, lengths, angles.
//!
//! C++: `solidcore_ffi::units::geom_vector2d` (upstream `SmVector2d`,
//! `source/SMLib/inc/SmVector2d.h`, `source/SMLib/src/SmVector2d.cpp`).
//! Rust: `solidcore::geom::vector2d::Vec2`.
//!
//! Every case is one operation `op` on vectors `a`, `b`, scalar `s` and index `idx`;
//! the result is `r` (1 or 2 floats; booleans as `1.0`/`0.0`). Errors are normalised
//! to `invalid_input` (`SM_ERR_INVALID_INPUT`) or `status_<code>` on both sides.
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::vector2d::Vec2;
use solidcore::math::{Vec3, EFF_ZERO, PI, UNDEF_DOUBLE};
use solidcore::SolidError;
use solidcore_ffi::units::geom_vector2d::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 4000;

/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i32 = 1007;

/// `geom.vector2d` runner.
pub struct Runner;

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 30] = [
    ("length_squared", Op::LengthSquared),
    ("length", Op::Length),
    ("dot", Op::Dot),
    ("neg", Op::Neg),
    ("add", Op::Add),
    ("sub", Op::Sub),
    ("mul", Op::Mul),
    ("lmul", Op::LMul),
    ("div", Op::Div),
    ("unitize", Op::Unitize),
    ("distance", Op::Distance),
    ("distance_squared", Op::DistanceSquared),
    ("closer_than", Op::CloserThan),
    ("max_dimension", Op::MaxDimension),
    ("swap_xy", Op::SwapXy),
    ("rotate", Op::Rotate),
    ("eq", Op::Eq),
    ("ne", Op::Ne),
    ("index", Op::Index),
    ("index_set", Op::IndexSet),
    ("add_assign", Op::AddAssign),
    ("sub_assign", Op::SubAssign),
    ("mul_assign", Op::MulAssign),
    ("angle", Op::Angle),
    ("ccw_angle", Op::CcwAngle),
    ("is_zero", Op::IsZero),
    ("is_initialized", Op::IsInitialized),
    ("from_vec3", Op::FromVec3),
    ("uninitialized", Op::Uninitialized),
    ("copy", Op::Copy),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: [f64; 2],
    #[serde(default)]
    b: [f64; 2],
    #[serde(default)]
    s: f64,
    #[serde(default)]
    idx: i32,
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
    match ffi::op(op, i.a, i.b, i.s, i.idx) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(FfiError::Status(SM_ERR_INVALID_INPUT)) => Ok(Output::err("invalid_input")),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

// `a * s` and `s * a` are different impls (`Mul<f64> for Vec2`, `Mul<Vec2> for f64`).
#[allow(clippy::match_same_arms)]
fn rust_values(op: Op, i: &Inputs) -> solidcore::Result<Vec<f64>> {
    let a = Vec2::from_array(i.a);
    let b = Vec2::from(i.b);
    let s = i.s;
    let v2 = |v: Vec2| Ok(v.to_array().to_vec());
    let v1 = |x: f64| Ok(vec![x]);
    match op {
        Op::LengthSquared => v1(a.length_squared()),
        Op::Length => v1(a.length()),
        Op::Dot => v1(a.dot(b)),
        Op::Neg => v2(-a),
        Op::Add => v2(a + b),
        Op::Sub => v2(a - b),
        Op::Mul => v2(a * s),
        Op::LMul => v2(s * a),
        Op::Div => v2(a / s),
        Op::Unitize => v2(a.unitize()?),
        Op::Distance => v1(a.distance(b)),
        Op::DistanceSquared => v1(a.distance_squared(b)),
        Op::CloserThan => v1(flag(a.closer_than(s, b))),
        Op::MaxDimension => v1(a.max_dimension()),
        Op::SwapXy => v2(a.swap_xy()),
        Op::Rotate => v2(a.rotate(s)),
        Op::Eq => v1(flag(a.approx_eq(b))),
        Op::Ne => v1(flag(a.approx_ne(b))),
        Op::Index => v1(a.component(usize::try_from(i.idx).unwrap_or(usize::MAX))?),
        Op::IndexSet => {
            let mut r = a;
            *r.component_mut(usize::try_from(i.idx).unwrap_or(usize::MAX))? = s;
            v2(r)
        }
        Op::AddAssign => {
            let mut r = a;
            r += b;
            v2(r)
        }
        Op::SubAssign => {
            let mut r = a;
            r -= b;
            v2(r)
        }
        Op::MulAssign => {
            let mut r = a;
            r *= s;
            v2(r)
        }
        Op::Angle => v1(a.angle_between(b)?),
        Op::CcwAngle => v1(a.ccw_angle_between(b)?),
        Op::IsZero => v1(flag(a.is_zero(s))),
        Op::IsInitialized => v1(flag(a.is_initialized())),
        Op::FromVec3 => v2(Vec2::from_xy(Vec3::new(i.a[0], i.a[1], s))),
        Op::Uninitialized => {
            let mut r = a;
            r.set_uninitialized();
            v2(r)
        }
        Op::Copy => {
            let c = a;
            v2(Vec2::new(c.x, c.y))
        }
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
    } else if r < 0.87 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(rng.range_i64(3, 12) as i32)
    } else if r < 0.90 {
        UNDEF_DOUBLE
    } else {
        rng.uniform(-1.0, 1.0)
    }
}

fn vector(rng: &mut Rng) -> [f64; 2] {
    let r = rng.unit();
    if r < 0.08 {
        // Unit or nearly-unit vector.
        let t = rng.uniform(0.0, 2.0 * PI);
        let e = if rng.chance(0.5) {
            0.0
        } else {
            rng.uniform(-1.0, 1.0) * 1e-12
        };
        [t.cos() * (1.0 + e), t.sin()]
    } else if r < 0.12 {
        // Squared length near EFF_ZERO_SQ (the unitize / angle thresholds).
        let t = rng.uniform(0.0, 2.0 * PI);
        let len = EFF_ZERO * rng.uniform(0.5, 1.5);
        [t.cos() * len, t.sin() * len]
    } else {
        [coord(rng), coord(rng)]
    }
}

/// `b` related to `a`: parallel, anti-parallel, nearly so, perpendicular, close
/// (for `==`/`!=`/`CloserThan`) or independent.
fn partner(rng: &mut Rng, a: [f64; 2]) -> [f64; 2] {
    let r = rng.unit();
    let k = rng.uniform(-3.0, 3.0);
    if r < 0.12 {
        [a[0] * k, a[1] * k]
    } else if r < 0.30 {
        // Nearly (anti)parallel: exercises the asin branch and CCW snapping.
        let eps = rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(1, 16) as i32));
        let (c, s) = (eps.cos(), eps.sin());
        [k * (c * a[0] - s * a[1]), k * (s * a[0] + c * a[1])]
    } else if r < 0.38 {
        [-a[1] * k, a[0] * k]
    } else if r < 0.52 {
        let eps = 10f64.powi(-(rng.range_i64(8, 16) as i32));
        [
            a[0] + rng.uniform(-1.0, 1.0) * eps,
            a[1] + rng.uniform(-1.0, 1.0) * eps,
        ]
    } else {
        vector(rng)
    }
}

fn scalar(rng: &mut Rng, op: Op, a: [f64; 2], b: [f64; 2]) -> f64 {
    match op {
        Op::Rotate => match rng.below(4) {
            0 => f64::from(rng.range_i64(-8, 8) as i32) * PI / 4.0,
            _ => rng.uniform(-10.0, 10.0),
        },
        Op::CloserThan => {
            let d = ((a[0] - b[0]) * (a[0] - b[0]) + (a[1] - b[1]) * (a[1] - b[1])).sqrt();
            match rng.below(4) {
                0 => d,
                1 => d * (1.0 + rng.uniform(-1e-12, 1e-12)),
                2 => (a[0] - b[0]).abs().max((a[1] - b[1]).abs()),
                _ => rng.uniform(-1.0, 10.0),
            }
        }
        Op::Div => match rng.below(5) {
            0 => 0.0,
            1 => {
                let m = a[0].abs().max(a[1].abs());
                m * EFF_ZERO * rng.uniform(0.5, 1.5)
            }
            _ => coord(rng),
        },
        Op::IsZero => match rng.below(3) {
            0 => EFF_ZERO,
            1 => (a[0] * a[0] + a[1] * a[1]).sqrt() * rng.uniform(0.9, 1.1),
            _ => coord(rng).abs(),
        },
        _ => coord(rng),
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx_op = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS.get(idx_op).copied().unwrap_or(("length", Op::Length));
    let a = vector(rng);
    let b = partner(rng, a);
    let s = scalar(rng, op, a, b);
    Inputs {
        op: name.to_string(),
        a,
        b,
        s,
        idx: rng.below(2) as i32,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.vector2d"
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
