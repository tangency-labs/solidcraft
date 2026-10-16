//! Runner for unit `geom.axis2_placement`: right-handed coordinate systems used as
//! rotate-and-translate transformations.
//!
//! C++: `solidcore_ffi::units::geom_axis2_placement` (upstream `SmAxis2Placement`,
//! `source/SMLib/inc/SmAxis2Placement.h`, `source/SMLib/src/SmAxis2Placement.cpp`).
//! Rust: `solidcore::geom::axis2_placement`.
//!
//! Every case builds placement `a` (and `b` for binary operations) by a construction
//! `kind` from six vectors `v` and a scalar `d`, then runs one operation `op` with
//! vectors `w` and scalars `s`, `t`. The result `r` is
//! `[status_a, A origin, A x, A y, A z, (status_b, B ...,) op values...]` with
//! booleans as `1.0`/`0.0` and setter statuses as their upstream codes
//! (`SM_ERR_INVALID_INPUT` = 1007).
//! Every run uses at least [`MIN_GENERATED`] generated inputs (more with `--seeds`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::vector3d::Vec3;
use solidcore::math::{EFF_ZERO, PI, UNDEF_DOUBLE};
use solidcore::SolidError;
use solidcore_ffi::units::geom_axis2_placement::{self as ffi, Kind, Op, Placement};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::flag;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 4000;

/// Upstream `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: i32 = 1007;

/// `geom.axis2_placement` runner.
pub struct Runner;

/// Construction names used in `inputs.a.kind` / `inputs.b.kind`.
const KINDS: [(&str, Kind); 8] = [
    ("identity", Kind::Identity),
    ("orient", Kind::Orient),
    ("canonical", Kind::Canonical),
    ("step", Kind::Step),
    ("plane", Kind::Plane),
    ("from_to", Kind::FromTo),
    ("copy", Kind::Copy),
    ("rotated", Kind::Rotated),
];

/// Operation names used in `inputs.op`.
const OPS: [(&str, Op); 17] = [
    ("state", Op::State),
    ("matrix", Op::Matrix),
    ("mirror", Op::Mirror),
    ("translate", Op::Translate),
    ("rotate", Op::Rotate),
    ("rotate_at", Op::RotateAt),
    ("transform_point", Op::TransformPoint),
    ("transform_vector", Op::TransformVector),
    ("transform_a2p", Op::TransformA2p),
    ("invert", Op::Invert),
    ("eq", Op::Eq),
    ("eq_self", Op::EqSelf),
    ("is_identity", Op::IsIdentity),
    ("coaxial", Op::Coaxial),
    ("chain", Op::Chain),
    ("init", Op::Init),
    ("transform_self", Op::TransformSelf),
];

#[derive(Debug, Clone, Serialize, Deserialize)]
struct PlacementIn {
    kind: String,
    #[serde(default)]
    v: [[f64; 3]; 6],
    #[serde(default)]
    d: f64,
}

impl Default for PlacementIn {
    fn default() -> Self {
        Self {
            kind: "identity".into(),
            v: [[0.0; 3]; 6],
            d: 0.0,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: String,
    #[serde(default)]
    a: PlacementIn,
    #[serde(default)]
    b: PlacementIn,
    #[serde(default)]
    w: [[f64; 3]; 3],
    #[serde(default)]
    s: f64,
    #[serde(default)]
    t: f64,
}

fn lookup<T: Copy>(table: &[(&str, T)], name: &str) -> Result<T, HarnessError> {
    table
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, x)| *x)
        .ok_or_else(|| HarnessError::new(format!("unknown name {name}")))
}

fn status_code(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => f64::from(SM_ERR_INVALID_INPUT),
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn normalise(e: &SolidError) -> String {
    format!("rust_error: {e}")
}

fn ffi_placement(p: &PlacementIn) -> Result<Placement, HarnessError> {
    Ok(Placement {
        kind: lookup(&KINDS, &p.kind)?,
        v: p.v,
        d: p.d,
    })
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let op = lookup(&OPS, &i.op)?;
    let a = ffi_placement(&i.a)?;
    let b = ffi_placement(&i.b)?;
    match ffi::op(op, &a, &b, i.w, i.s, i.t) {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(FfiError::Status(s)) if s > 0 => Ok(Output::err(format!("status_{s}"))),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// Build a placement as the shim does: `(status, placement)`.
fn build(p: &PlacementIn) -> Result<(f64, Axis2Placement), HarnessError> {
    let kind = lookup(&KINDS, &p.kind)?;
    let v = p.v.map(Vec3::from);
    let [v0, v1, v2, v3, v4, v5] = v;
    let mut st = 0.0;
    let a = match kind {
        Kind::Identity => Axis2Placement::default(),
        // The copy constructor is `Copy`.
        Kind::Orient | Kind::Copy => Axis2Placement::new(v0, v1, v2),
        Kind::Canonical => {
            let mut q = Axis2Placement::IDENTITY;
            if let Err(e) = q.set_canonical(v0, v1, v2) {
                st = status_code(&e);
            }
            q
        }
        Kind::Step => {
            let mut q = Axis2Placement::IDENTITY;
            if let Err(e) = q.set_step_canonical(v0, v1, v2) {
                st = status_code(&e);
            }
            q
        }
        Kind::Plane => Axis2Placement::from_plane(v0, p.d),
        Kind::FromTo => Axis2Placement::from_to(
            &Axis2Placement::new(v0, v1, v2),
            &Axis2Placement::new(v3, v4, v5),
        ),
        Kind::Rotated => {
            let mut q = Axis2Placement::new(v0, v1, v2);
            q.rotate_about_axis_at_point(p.d, v3, v4);
            q
        }
    };
    Ok((st, a))
}

fn put_v(out: &mut Vec<f64>, v: Vec3) {
    out.extend(v.to_array());
}

fn put_state(out: &mut Vec<f64>, p: &Axis2Placement) {
    put_v(out, p.origin());
    put_v(out, p.x_axis());
    put_v(out, p.y_axis());
    put_v(out, p.z_axis());
}

fn rust_values(i: &Inputs) -> Result<solidcore::Result<Vec<f64>>, HarnessError> {
    let op = lookup(&OPS, &i.op)?;
    let (st_a, mut a) = build(&i.a)?;
    let mut out = vec![st_a];
    put_state(&mut out, &a);
    let [w0, w1, w2] = i.w.map(Vec3::from);
    let build_b = |out: &mut Vec<f64>| -> Result<Axis2Placement, HarnessError> {
        let (st_b, b) = build(&i.b)?;
        out.push(st_b);
        put_state(out, &b);
        Ok(b)
    };
    match op {
        Op::State => {
            put_v(&mut out, a.origin());
            put_v(&mut out, a.x_axis());
            put_v(&mut out, a.y_axis());
            let (o, x, y) = a.canonical();
            put_v(&mut out, o);
            put_v(&mut out, x);
            put_v(&mut out, y);
        }
        Op::Matrix => out.extend(a.matrix().iter().flatten()),
        Op::Mirror => out.extend(a.mirror_matrix().iter().flatten()),
        Op::Translate => {
            a.translate(w0);
            put_state(&mut out, &a);
        }
        Op::Rotate => {
            a.rotate_about_axis(i.s, w0);
            put_state(&mut out, &a);
        }
        Op::RotateAt => {
            a.rotate_about_axis_at_point(i.s, w0, w1);
            put_state(&mut out, &a);
        }
        Op::TransformPoint => put_v(&mut out, a.transform_point(w0)),
        Op::TransformVector => put_v(&mut out, a.transform_vector(w0)),
        Op::TransformA2p => {
            let b = build_b(&mut out)?;
            put_state(&mut out, &a.transform_axis2_placement(&b));
        }
        Op::Invert => put_state(&mut out, &a.invert()),
        Op::Eq => {
            let b = build_b(&mut out)?;
            out.push(flag(a.approx_eq(&b)));
        }
        Op::EqSelf => out.push(flag(a.approx_eq(&a))),
        Op::IsIdentity => out.push(flag(a.is_identity(i.s))),
        Op::Coaxial => {
            let b = build_b(&mut out)?;
            out.push(flag(a.are_coaxial(&b, i.s, i.t)));
        }
        Op::Chain => {
            a.rotate_about_axis(i.s, w0);
            a.translate(w1);
            a.rotate_about_axis(i.t, w2);
            put_state(&mut out, &a);
        }
        Op::Init => {
            a.init();
            put_state(&mut out, &a);
        }
        Op::TransformSelf => {
            a = a.transform_axis2_placement(&a);
            put_state(&mut out, &a);
        }
    }
    Ok(Ok(out))
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    match rust_values(i)? {
        Ok(v) => Ok(Output::new().values("r", v)),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(normalise(&e)))),
    }
}

// ---- generation ---------------------------------------------------------------

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

fn coord(rng: &mut Rng) -> f64 {
    let r = rng.unit();
    if r < 0.6 {
        rng.uniform(-10.0, 10.0)
    } else if r < 0.7 {
        0.0
    } else if r < 0.8 {
        rng.range_i64(-5, 5) as f64
    } else if r < 0.9 {
        rng.uniform(-1.0, 1.0) * 10f64.powi(rng.range_i64(2, 9) as i32)
    } else {
        rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(3, 12) as i32))
    }
}

fn point(rng: &mut Rng) -> [f64; 3] {
    [coord(rng), coord(rng), coord(rng)]
}

/// A direction: unit, nearly unit (around the `SM_EFF_ZERO/100` unit test), scaled,
/// axis-aligned, tiny (around the `SM_EFF_ZERO_SQ` unitize limit), zero or undefined.
fn direction(rng: &mut Rng) -> [f64; 3] {
    let r = rng.unit();
    if r < 0.35 {
        unit(rng)
    } else if r < 0.45 {
        let e = rng.uniform(-1.0, 1.0) * 10f64.powi(-(rng.range_i64(8, 16) as i32));
        scale(unit(rng), 1.0 + e)
    } else if r < 0.6 {
        scale(unit(rng), rng.uniform(0.01, 100.0))
    } else if r < 0.72 {
        let mut v = [0.0; 3];
        if let Some(slot) = v.get_mut(rng.below(3) as usize) {
            *slot = if rng.chance(0.5) { 1.0 } else { -1.0 } * rng.uniform(0.5, 3.0);
        }
        v
    } else if r < 0.76 {
        scale(unit(rng), EFF_ZERO * rng.uniform(0.3, 3.0))
    } else if r < 0.79 {
        [0.0; 3]
    } else if r < 0.80 {
        [UNDEF_DOUBLE; 3]
    } else {
        point(rng)
    }
}

/// A Y axis for `x`: perpendicular, nearly perpendicular (around the `SM_EFF_ZERO`
/// dot test), parallel, anti-parallel or independent.
fn partner(rng: &mut Rng, x: [f64; 3]) -> [f64; 3] {
    let r = rng.unit();
    let p = cross(x, unit(rng));
    let plen = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    if r < 0.35 && plen > 0.0 {
        let k = if rng.chance(0.5) {
            1.0 / plen
        } else {
            rng.uniform(0.1, 5.0) / plen
        };
        scale(p, k)
    } else if r < 0.55 && plen > 0.0 {
        let eps = 10f64.powi(-(rng.range_i64(6, 16) as i32)) * rng.uniform(-3.0, 3.0);
        add(scale(p, 1.0 / plen), scale(x, eps))
    } else if r < 0.62 {
        scale(x, rng.uniform(-3.0, 3.0))
    } else {
        direction(rng)
    }
}

fn angle(rng: &mut Rng) -> f64 {
    match rng.below(5) {
        0 => f64::from(rng.range_i64(-8, 8) as i32) * PI / 4.0,
        1 => rng.uniform(-1e-6, 1e-6),
        2 => rng.uniform(-100.0, 100.0),
        _ => rng.uniform(-PI, PI),
    }
}

fn gen_placement(rng: &mut Rng) -> PlacementIn {
    let idx = rng.below(KINDS.len() as u64) as usize;
    let (name, kind) = KINDS.get(idx).copied().unwrap_or(("identity", Kind::Identity));
    let x = direction(rng);
    let y = partner(rng, x);
    let tx = direction(rng);
    let ty = partner(rng, tx);
    let mut v = [point(rng), x, y, point(rng), tx, ty];
    let mut d = coord(rng);
    match kind {
        Kind::Rotated => {
            v[4] = if rng.chance(0.8) {
                unit(rng)
            } else {
                direction(rng)
            };
            d = angle(rng);
        }
        Kind::Plane if rng.chance(0.2) => v[0] = [0.0; 3],
        _ => {}
    }
    PlacementIn {
        kind: name.to_string(),
        v,
        d,
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let idx = rng.below(OPS.len() as u64) as usize;
    let (name, op) = OPS.get(idx).copied().unwrap_or(("state", Op::State));
    let a = gen_placement(rng);
    let mut b = gen_placement(rng);
    if matches!(op, Op::Eq) && rng.chance(0.5) {
        // Same construction, perturbed slightly (or not at all).
        b = a.clone();
        let eps = if rng.chance(0.3) {
            0.0
        } else {
            10f64.powi(-(rng.range_i64(8, 16) as i32))
        };
        let (vi, ci) = (rng.below(6) as usize, rng.below(3) as usize);
        if let Some(c) = b.v.get_mut(vi).and_then(|v| v.get_mut(ci)) {
            *c += eps * rng.uniform(-1.0, 1.0);
        }
    }
    let w = [direction(rng), direction(rng), direction(rng)];
    let (s, t) = match op {
        Op::IsIdentity => (
            match rng.below(3) {
                0 => EFF_ZERO,
                1 => 10f64.powi(-(rng.range_i64(1, 15) as i32)),
                _ => rng.uniform(0.0, 2.0),
            },
            0.0,
        ),
        Op::Coaxial => (rng.uniform(0.0, 1e-3), rng.uniform(0.0, 10.0)),
        _ => (angle(rng), angle(rng)),
    };
    Inputs {
        op: name.to_string(),
        a,
        b,
        w,
        s,
        t,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "geom.axis2_placement"
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
