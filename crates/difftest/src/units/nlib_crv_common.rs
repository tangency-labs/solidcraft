//! Runner for unit `nlib.crv_common`: NURBS circles, circular arcs, ellipses and
//! elliptical arcs (quadratic, quartic, quintic) and circle weights.
//!
//! C++: `solidcore_ffi::units::nlib_crv_common` (upstream
//! `source/SMLib/src/NL_CrvCommon.cpp`). Rust: `solidcore::nurbs::crv_common`.
//!
//! Each case runs one routine (`inputs.op`). Both sides produce the same flat record
//! of doubles, split into keys by [`to_output`]: `status` (`0` ok, else the
//! `NL_*_ERR` number; `-1` on the Rust side for a non-NLib error, which never
//! matches), then on success a curve as `dims = [n, p, m]`, `cpts` (x y z w) and
//! `u`, or for `circ_weights` the weight count `count` and the weights `w`. Every run
//! has at least [`MIN_GENERATED`] generated cases.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::nurbs::crv_basic::{ConicType, Curve};
use solidcore::nurbs::crv_common::{self as cc, AngleType, Arc, EllipseArc};
use solidcore::SolidError;
use solidcore_ffi::units::nlib_crv_common::{self as ffi, Op};
use solidcore_ffi::FfiError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::v3;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 2000;

/// `nlib.crv_common` runner.
pub struct Runner;

/// Circle frame and angles shared by the circle ops.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct CircIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    r: f64,
    #[serde(rename = "as")]
    a_s: f64,
    ae: f64,
}

/// Ellipse frame, radii and angles.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EllIn {
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    r1: f64,
    r2: f64,
    #[serde(rename = "as")]
    a_s: f64,
    ae: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    QuadraticArcTol {
        #[serde(flatten)]
        arc: CircIn,
        at: f64,
    },
    QuarticArcTol {
        #[serde(flatten)]
        arc: CircIn,
        at: f64,
    },
    QuinticArcTol {
        #[serde(flatten)]
        arc: CircIn,
        at: f64,
    },
    CircArc {
        #[serde(flatten)]
        arc: CircIn,
        ctp: i32,
    },
    CircArcTol {
        #[serde(flatten)]
        arc: CircIn,
        at: f64,
        ctp: i32,
    },
    CircWeights {
        #[serde(rename = "as")]
        a_s: f64,
        ae: f64,
        ctp: i32,
    },
    EllipticalArcTol {
        #[serde(flatten)]
        ell: EllIn,
        at: f64,
        ctp: i32,
    },
    EllipticalArcAngTypeTol {
        #[serde(flatten)]
        ell: EllIn,
        at: f64,
        atp: i32,
        ctp: i32,
    },
}

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- records ----------------------------------------------------------------------

fn take<'a>(flat: &'a [f64], at: &mut usize, k: usize) -> Result<&'a [f64], HarnessError> {
    let part = at
        .checked_add(k)
        .and_then(|end| flat.get(*at..end))
        .ok_or_else(|| harness(format!("record too short ({} values)", flat.len())))?;
    *at += k;
    Ok(part)
}

fn count(x: f64) -> Result<usize, HarnessError> {
    if x.fract() != 0.0 || !(0.0..=1e6).contains(&x) {
        return Err(harness(format!("bad count {x} in record")));
    }
    Ok(x as usize)
}

fn to_output(i: &Inputs, flat: &[f64]) -> Result<Output, HarnessError> {
    let mut at = 0usize;
    let status = take(flat, &mut at, 1)?.to_vec();
    let ok = status == [0.0];
    let mut out = Output::new().values("status", status);
    if ok {
        if let Inputs::CircWeights { .. } = i {
            let k = take(flat, &mut at, 1)?.to_vec();
            let n = count(k.first().copied().unwrap_or(f64::NAN))?;
            out = out.values("count", k);
            out = out.values("w", take(flat, &mut at, n)?.to_vec());
        } else {
            let dims = take(flat, &mut at, 3)?;
            let n = count(dims.first().copied().unwrap_or(f64::NAN))?;
            let m = count(dims.get(2).copied().unwrap_or(f64::NAN))?;
            out = out.values("dims", dims.to_vec());
            out = out.values("cpts", take(flat, &mut at, 4 * (n + 1))?.to_vec());
            out = out.values("u", take(flat, &mut at, m + 1)?.to_vec());
        }
    }
    if at != flat.len() {
        return Err(harness(format!(
            "record has {} values, layout used {at}",
            flat.len()
        )));
    }
    Ok(out)
}

// ---- C++ side ---------------------------------------------------------------------

fn enc_circ(a: &CircIn, o: &mut Vec<f64>) {
    o.extend(a.c);
    o.extend(a.x);
    o.extend(a.y);
    o.extend([a.r, a.a_s, a.ae]);
}

fn enc_ell(e: &EllIn, o: &mut Vec<f64>) {
    o.extend(e.c);
    o.extend(e.x);
    o.extend(e.y);
    o.extend([e.r1, e.r2, e.a_s, e.ae]);
}

fn cpp_input(i: &Inputs) -> (Op, Vec<f64>) {
    let mut o = Vec::new();
    let op = match i {
        Inputs::QuadraticArcTol { arc, at } => {
            enc_circ(arc, &mut o);
            o.push(*at);
            Op::QuadraticArcTol
        }
        Inputs::QuarticArcTol { arc, at } => {
            enc_circ(arc, &mut o);
            o.push(*at);
            Op::QuarticArcTol
        }
        Inputs::QuinticArcTol { arc, at } => {
            enc_circ(arc, &mut o);
            o.push(*at);
            Op::QuinticArcTol
        }
        Inputs::CircArc { arc, ctp } => {
            enc_circ(arc, &mut o);
            o.push(f64::from(*ctp));
            Op::CircArc
        }
        Inputs::CircArcTol { arc, at, ctp } => {
            enc_circ(arc, &mut o);
            o.extend([*at, f64::from(*ctp)]);
            Op::CircArcTol
        }
        Inputs::CircWeights { a_s, ae, ctp } => {
            o.extend([*a_s, *ae, f64::from(*ctp)]);
            Op::CircWeights
        }
        Inputs::EllipticalArcTol { ell, at, ctp } => {
            enc_ell(ell, &mut o);
            o.extend([*at, f64::from(*ctp)]);
            Op::EllipticalArcTol
        }
        Inputs::EllipticalArcAngTypeTol { ell, at, atp, ctp } => {
            enc_ell(ell, &mut o);
            o.extend([*at, f64::from(*atp), f64::from(*ctp)]);
            Op::EllipticalArcAngTypeTol
        }
    };
    (op, o)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, input) = cpp_input(i);
    match ffi::eval(op, &input) {
        Ok(flat) => to_output(i, &flat),
        Err(FfiError::Status(s)) => Err(harness(format!("shim status {s}"))),
        Err(e) => Err(harness(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

fn arc(a: &CircIn) -> Arc {
    Arc {
        center: v3(a.c),
        x: v3(a.x),
        y: v3(a.y),
        r: a.r,
        start: a.a_s,
        end: a.ae,
    }
}

fn ell(e: &EllIn) -> EllipseArc {
    EllipseArc {
        center: v3(e.c),
        x: v3(e.x),
        y: v3(e.y),
        r1: e.r1,
        r2: e.r2,
        start: e.a_s,
        end: e.ae,
    }
}

fn conic(ctp: i32) -> Result<ConicType, HarnessError> {
    match ctp {
        2 => Ok(ConicType::Quadratic),
        4 => Ok(ConicType::Quartic),
        5 => Ok(ConicType::Quintic),
        _ => Err(harness(format!("bad conic type {ctp}"))),
    }
}

fn angle_type(atp: i32) -> Result<AngleType, HarnessError> {
    match atp {
        0 => Ok(AngleType::Circle),
        1 => Ok(AngleType::Ellipse),
        _ => Err(harness(format!("bad angle type {atp}"))),
    }
}

fn dump(o: &mut Vec<f64>, c: &Curve) {
    o.extend([c.n() as f64, c.degree() as f64, c.m() as f64]);
    for pw in c.cpts() {
        o.extend(pw.to_array());
    }
    o.extend_from_slice(c.knots());
}

/// Status of a Rust error: the NLib number, `-1` for anything else.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Nlib { code, .. } => f64::from(code.code()),
        _ => -1.0,
    }
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    // Flags first, so a bad flag is a harness error on both sides.
    let ctp = match i {
        Inputs::CircArc { ctp, .. }
        | Inputs::CircArcTol { ctp, .. }
        | Inputs::CircWeights { ctp, .. }
        | Inputs::EllipticalArcTol { ctp, .. }
        | Inputs::EllipticalArcAngTypeTol { ctp, .. } => conic(*ctp)?,
        _ => ConicType::Quadratic,
    };
    let atp = match i {
        Inputs::EllipticalArcAngTypeTol { atp, .. } => angle_type(*atp)?,
        _ => AngleType::Circle,
    };
    let mut o = vec![0.0];
    let res: Result<(), SolidError> = (|| {
        let cur = match i {
            Inputs::QuadraticArcTol { arc: a, at } => cc::quadratic_arc_tol(&arc(a), *at)?,
            Inputs::QuarticArcTol { arc: a, at } => cc::quartic_arc_tol(&arc(a), *at)?,
            Inputs::QuinticArcTol { arc: a, at } => cc::quintic_arc_tol(&arc(a), *at)?,
            Inputs::CircArc { arc: a, .. } => cc::circ_arc(&arc(a), ctp)?,
            Inputs::CircArcTol { arc: a, at, .. } => cc::circ_arc_tol(&arc(a), *at, ctp)?,
            Inputs::CircWeights { a_s, ae, .. } => {
                let w = cc::circ_weights(*a_s, *ae, ctp)?;
                o.push(w.len() as f64);
                o.extend(w);
                return Ok(());
            }
            Inputs::EllipticalArcTol { ell: e, at, .. } => {
                cc::elliptical_arc_tol(&ell(e), *at, ctp)?
            }
            Inputs::EllipticalArcAngTypeTol { ell: e, at, .. } => {
                cc::elliptical_arc_ang_type_tol(&ell(e), *at, atp, ctp)?
            }
        };
        dump(&mut o, &cur);
        Ok(())
    })();
    match res {
        Ok(()) => to_output(i, &o),
        Err(e) => match Output::unported(&e) {
            Some(d) => Ok(d),
            None => to_output(i, &[status(&e)]),
        },
    }
}

// ---- generated cases --------------------------------------------------------------

fn gen_angle(rng: &mut Rng) -> f64 {
    if rng.chance(0.3) {
        // Multiples of 15 degrees hit the segment-count and snapping boundaries.
        15.0 * (rng.below(49) as f64 - 24.0)
    } else if rng.chance(0.1) {
        rng.uniform(-2000.0, 2000.0)
    } else {
        rng.uniform(-360.0, 360.0)
    }
}

/// End angle: unrelated, equal (full circle), near a full turn or near a
/// segment-count boundary of the sweep.
fn gen_end(rng: &mut Rng, a_s: f64) -> f64 {
    match rng.below(8) {
        0 => a_s,
        1 => a_s + 360.0 + rng.uniform(-0.05, 0.05),
        2 => {
            let b = *rng.choose(&[91.0, 121.0, 181.0, 241.0, 271.0]).unwrap_or(&90.0);
            a_s + b + rng.uniform(-1e-3, 1e-3)
        }
        3 => a_s + rng.uniform(1e-5, 1.0),
        _ => gen_angle(rng),
    }
}

fn unit3(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.1 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// A frame: orthogonal axes (scaled), sometimes the world axes, rarely skewed.
fn gen_frame(rng: &mut Rng) -> ([f64; 3], [f64; 3], [f64; 3]) {
    let c = if rng.chance(0.2) {
        [0.0; 3]
    } else {
        rng.point3(-50.0, 50.0)
    };
    if rng.chance(0.25) {
        return (c, [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
    }
    let x = unit3(rng);
    let mut y = cross(unit3(rng), x);
    let l = (y[0] * y[0] + y[1] * y[1] + y[2] * y[2]).sqrt();
    if l < 1e-3 {
        return (c, [1.0, 0.0, 0.0], [0.0, 0.0, 1.0]);
    }
    y = [y[0] / l, y[1] / l, y[2] / l];
    if rng.chance(0.1) {
        // Skewed axes: upstream does not require orthogonality.
        let t = rng.uniform(-0.5, 0.5);
        y = [y[0] + t * x[0], y[1] + t * x[1], y[2] + t * x[2]];
    }
    let sx = rng.uniform(0.2, 5.0);
    let sy = rng.uniform(0.2, 5.0);
    (c, x.map(|v| v * sx), y.map(|v| v * sy))
}

fn gen_radius(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 => -rng.uniform(0.1, 10.0),
        1 => 1e-6,
        2 => 1e4,
        _ => rng.uniform(0.01, 100.0),
    }
}

fn gen_at(rng: &mut Rng) -> f64 {
    match rng.below(6) {
        0 => rng.uniform(0.0, 1.0),
        1 => 0.0,
        2 => -0.5,
        _ => 0.01,
    }
}

fn gen_circ(rng: &mut Rng) -> CircIn {
    let (c, x, y) = gen_frame(rng);
    let a_s = gen_angle(rng);
    let ae = gen_end(rng, a_s);
    CircIn {
        c,
        x,
        y,
        r: gen_radius(rng),
        a_s,
        ae,
    }
}

fn gen_ell(rng: &mut Rng) -> EllIn {
    let (c, x, y) = gen_frame(rng);
    let a_s = gen_angle(rng);
    let ae = gen_end(rng, a_s);
    let r1 = gen_radius(rng);
    let r2 = if rng.chance(0.1) {
        r1
    } else {
        gen_radius(rng)
    };
    let r2 = if r2 == 0.0 { 1.0 } else { r2 };
    EllIn {
        c,
        x,
        y,
        r1: if r1 == 0.0 { 1.0 } else { r1 },
        r2,
        a_s,
        ae,
    }
}

fn gen_ctp(rng: &mut Rng) -> i32 {
    *rng.choose(&[2, 4, 5]).unwrap_or(&2)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "nlib.crv_common"
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

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let i = match rng.below(8) {
            0 => Inputs::QuadraticArcTol {
                arc: gen_circ(rng),
                at: gen_at(rng),
            },
            1 => Inputs::QuarticArcTol {
                arc: gen_circ(rng),
                at: gen_at(rng),
            },
            2 => Inputs::QuinticArcTol {
                arc: gen_circ(rng),
                at: gen_at(rng),
            },
            3 => Inputs::CircArc {
                arc: gen_circ(rng),
                ctp: gen_ctp(rng),
            },
            4 => Inputs::CircArcTol {
                arc: gen_circ(rng),
                at: gen_at(rng),
                ctp: gen_ctp(rng),
            },
            5 => {
                let a_s = gen_angle(rng);
                Inputs::CircWeights {
                    a_s,
                    ae: gen_end(rng, a_s),
                    ctp: gen_ctp(rng),
                }
            }
            6 => Inputs::EllipticalArcTol {
                ell: gen_ell(rng),
                at: gen_at(rng),
                ctp: gen_ctp(rng),
            },
            _ => Inputs::EllipticalArcAngTypeTol {
                ell: gen_ell(rng),
                at: gen_at(rng),
                atp: i32::from(rng.chance(0.5)),
                ctp: gen_ctp(rng),
            },
        };
        serde_json::to_value(i).ok()
    }
}
