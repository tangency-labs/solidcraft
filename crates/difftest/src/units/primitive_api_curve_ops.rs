//! Runner for unit `primitive.api_curve_ops`: the `SM_API` curve utilities
//! (`SmApiCreateCurveInterpPoints`, `SmApiOffsetCurve`, `SmApiEvaluateContinuity`,
//! `SmApiMakeCurvesCompatible`, `SmApiOrderCurves`).
//!
//! C++: `solidcore_ffi::units::primitive_api_curve_ops` (upstream
//! `source/SM_API/src/SmApiCurves.cpp`). Rust: `solidcore::primitive::api_curve_ops`.
//!
//! A case is one API call (`op` plus its arguments; floats may be `"NaN"`, `"inf"`,
//! `"-inf"`). Input curves are made by the `SM_API` curve constructors on each side
//! (`primitive.api_curves`); a constructor failure records `-2` alone. Both sides
//! produce the record documented in the ffi module: the status (`0` = `SM_SUCCESS`,
//! else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error
//! upstream cannot produce), then the result, reported as the values `rec`. A
//! continuity parameter given as `relative` is the curve's natural interval evaluated
//! there (`SmExtent1d::Evaluate`), on each side from its own curve. A case whose Rust
//! side reaches an unported unit is deferred.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::crv_fit::p1::ParamMethod;
use solidcore::primitive::api_curve_ops as api;
use solidcore::primitive::api_curves;
use solidcore::SolidError;
use solidcore_ffi::units::primitive_api_curve_ops::{self as ffi, Call, InCurve};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 600;

/// `primitive.api_curve_ops` runner.
pub struct Runner;

// ---- inputs -------------------------------------------------------------------------

/// A float that may be written `"NaN"`, `"inf"` or `"-inf"` in JSON.
#[derive(Debug, Clone, Copy, PartialEq)]
struct F(f64);

impl Serialize for F {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        if self.0.is_nan() {
            s.serialize_str("NaN")
        } else if self.0 == f64::INFINITY {
            s.serialize_str("inf")
        } else if self.0 == f64::NEG_INFINITY {
            s.serialize_str("-inf")
        } else {
            s.serialize_f64(self.0)
        }
    }
}

impl<'de> Deserialize<'de> for F {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        match Value::deserialize(d)? {
            Value::Number(n) => n
                .as_f64()
                .map(F)
                .ok_or_else(|| serde::de::Error::custom("bad number")),
            Value::String(s) if s == "NaN" => Ok(F(f64::NAN)),
            Value::String(s) if s == "inf" => Ok(F(f64::INFINITY)),
            Value::String(s) if s == "-inf" => Ok(F(f64::NEG_INFINITY)),
            other => Err(serde::de::Error::custom(format!("bad float {other}"))),
        }
    }
}

type P3 = [F; 3];

/// One input curve, made by an `SM_API` constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Crv {
    Curve { points: Vec<P3> },
    Circle { center: P3, radius: F },
    Arc { center: P3, radius: F, start_deg: F, end_deg: F },
    Segment { start: P3, end: P3 },
    AnalyticCircle { center: P3, radius: F },
    Rectangle { center: P3, length: F, width: F },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum In {
    Interp {
        points: Vec<P3>,
        /// `SmCurveParameterizationType` code: 0 uniform, 1 chord length, 2 centripetal.
        #[serde(default)]
        parameterization: u32,
    },
    Offset {
        curve: Crv,
        distance: F,
    },
    Continuity {
        curve: Crv,
        parameter: F,
        /// `parameter` is relative to the curve's natural interval.
        #[serde(default)]
        relative: bool,
    },
    Compatible {
        curves: Vec<Crv>,
    },
    Order {
        curves: Vec<Crv>,
    },
}

fn a3(p: &P3) -> [f64; 3] {
    [p[0].0, p[1].0, p[2].0]
}

fn v3(p: &P3) -> Vec3 {
    Vec3::from_array(a3(p))
}

fn in_curve(c: &Crv) -> InCurve {
    match c {
        Crv::Curve { points } => InCurve::Curve {
            points: points.iter().map(a3).collect(),
        },
        Crv::Circle { center, radius } => InCurve::Circle {
            center: a3(center),
            radius: radius.0,
        },
        Crv::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => InCurve::Arc {
            center: a3(center),
            radius: radius.0,
            start_deg: start_deg.0,
            end_deg: end_deg.0,
        },
        Crv::Segment { start, end } => InCurve::Segment {
            start: a3(start),
            end: a3(end),
        },
        Crv::AnalyticCircle { center, radius } => InCurve::AnalyticCircle {
            center: a3(center),
            radius: radius.0,
        },
        Crv::Rectangle {
            center,
            length,
            width,
        } => InCurve::Rectangle {
            center: a3(center),
            length: length.0,
            width: width.0,
        },
    }
}

fn to_call(i: &In) -> Call {
    match i {
        In::Interp {
            points,
            parameterization,
        } => Call::Interp {
            points: points.iter().map(a3).collect(),
            parameterization: *parameterization,
        },
        In::Offset { curve, distance } => Call::Offset {
            curve: in_curve(curve),
            distance: distance.0,
        },
        In::Continuity {
            curve,
            parameter,
            relative,
        } => Call::Continuity {
            curve: in_curve(curve),
            parameter: parameter.0,
            relative: *relative,
        },
        In::Compatible { curves } => Call::Compatible {
            curves: curves.iter().map(in_curve).collect(),
        },
        In::Order { curves } => Call::Order {
            curves: curves.iter().map(in_curve).collect(),
        },
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn cpp(i: &In) -> Result<Output, HarnessError> {
    match ffi::run(&to_call(i)) {
        Ok(v) => Ok(Output::new().values("rec", v)),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

// ---- Rust side --------------------------------------------------------------------

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn put_ivl(o: &mut Vec<f64>, e: &Extent1d) {
    o.extend([e.min(), e.max()]);
}

/// `put_bspline` of the shim; `type_code` and `natural` are the virtual
/// `GetType` / `GetNaturalInterval` of the object.
fn put_bspline(o: &mut Vec<f64>, type_code: i64, c: &BSplineCurve, natural: &Extent1d) {
    o.extend([
        type_code as f64,
        c.dim() as f64,
        f64::from(c.knot_type() as i32),
        f64::from(c.form() as i32),
    ]);
    match c.gw_nurb() {
        None => o.push(0.0),
        Some(cur) => {
            o.extend([1.0, cur.n() as f64, cur.degree() as f64, cur.m() as f64]);
            for p in cur.cpts() {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend_from_slice(cur.knots());
            put_ivl(o, natural);
        }
    }
}

/// [`put_bspline`] of a curve of an `SmBSplineCurve` class.
fn put_curve(o: &mut Vec<f64>, c: &Curve) -> Result<(), HarnessError> {
    let base = match c.kind() {
        CurveKind::BSpline(b) => b,
        CurveKind::Line(l) => l.base(),
        CurveKind::Ellipse(e) => e.base(),
        _ => return Err(HarnessError::new("result is not an SmBSplineCurve class")),
    };
    put_bspline(o, c.type_code(), base, &c.natural_interval());
    Ok(())
}

/// How making an input curve went.
enum Made {
    /// Made (appended).
    Yes,
    /// The constructor failed: the `-2` record.
    Failed,
    /// The constructor reached an unported unit.
    Deferred(Box<Output>),
}

impl Made {
    fn of<T>(r: solidcore::Result<T>, push: impl FnOnce(T)) -> Self {
        match r {
            Ok(v) => {
                push(v);
                Self::Yes
            }
            Err(e) => match Output::unported(&e) {
                Some(d) => Self::Deferred(Box::new(d)),
                None => Self::Failed,
            },
        }
    }
}

/// The curves `c` makes, appended to `out`.
fn build(k: &Kernel, c: &Crv, out: &mut Vec<Curve>) -> Made {
    let made: solidcore::Result<Vec<Curve>> = match c {
        Crv::Curve { points } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            api_curves::create_curve(&pts).map(|b| vec![Curve::from(b)])
        }
        Crv::Circle { center, radius } => {
            api_curves::create_circle(v3(center), radius.0).map(|b| vec![Curve::from(b)])
        }
        Crv::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => api_curves::create_arc(v3(center), radius.0, start_deg.0, end_deg.0)
            .map(|b| vec![Curve::from(b)]),
        Crv::Segment { start, end } => {
            api_curves::create_line_segment(v3(start), v3(end)).map(|l| vec![Curve::from(l)])
        }
        Crv::AnalyticCircle { center, radius } => {
            api_curves::create_analytic_circle(k, v3(center), radius.0)
                .map(|e| vec![Curve::from(e)])
        }
        Crv::Rectangle {
            center,
            length,
            width,
        } => api_curves::create_rectangle(v3(center), length.0, width.0)
            .map(|v| v.into_iter().map(Curve::from).collect()),
    };
    Made::of(made, |v| out.extend(v))
}

/// The B-spline curve `c` makes (kinds curve, circle, arc), as [`build`].
fn build_plain(c: &Crv, out: &mut Vec<BSplineCurve>) -> Made {
    let made = match c {
        Crv::Curve { points } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            api_curves::create_curve(&pts)
        }
        Crv::Circle { center, radius } => api_curves::create_circle(v3(center), radius.0),
        Crv::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => api_curves::create_arc(v3(center), radius.0, start_deg.0, end_deg.0),
        _ => return Made::Failed,
    };
    Made::of(made, |b| out.push(b))
}

fn record(o: Vec<f64>) -> Output {
    Output::new().values("rec", o)
}

fn failed(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| record(vec![status(e)]))
}

fn param_method(code: u32) -> Option<ParamMethod> {
    Some(match code {
        0 => ParamMethod::Uniform,
        1 => ParamMethod::ChordLength,
        2 => ParamMethod::Centripetal,
        _ => return None,
    })
}

fn rust(i: &In) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    // the one curve of an offset or continuity case, else the case's record
    let one = |c: &Crv| -> Result<Result<Curve, Box<Output>>, HarnessError> {
        let mut v = Vec::new();
        match build(&k, c, &mut v) {
            Made::Deferred(d) => Ok(Err(d)),
            Made::Failed => Ok(Err(Box::new(record(vec![-2.0])))),
            Made::Yes => v
                .into_iter()
                .next()
                .map(Ok)
                .ok_or_else(|| HarnessError::new("no curve built")),
        }
    };
    Ok(match i {
        In::Interp {
            points,
            parameterization,
        } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            let par = param_method(*parameterization)
                .ok_or_else(|| HarnessError::new("bad parameterization"))?;
            match api::create_curve_interp_points(&pts, par) {
                Ok(c) => {
                    let mut o = vec![0.0];
                    put_bspline(&mut o, BSplineCurve::TYPE, &c, &c.natural_interval());
                    record(o)
                }
                Err(e) => failed(&e),
            }
        }
        In::Offset { curve, distance } => {
            let c = match one(curve)? {
                Ok(c) => c,
                Err(out) => return Ok(*out),
            };
            match api::offset_curve(&k, &c, distance.0) {
                Ok(r) => {
                    let mut o = vec![0.0];
                    put_curve(&mut o, &r)?;
                    record(o)
                }
                Err(e) => failed(&e),
            }
        }
        In::Continuity {
            curve,
            parameter,
            relative,
        } => {
            let c = match one(curve)? {
                Ok(c) => c,
                Err(out) => return Ok(*out),
            };
            let t = if *relative {
                c.natural_interval().evaluate(parameter.0)
            } else {
                parameter.0
            };
            match api::evaluate_continuity(&k, &c, t) {
                Ok(ct) => record(vec![0.0, f64::from(ct as i32)]),
                Err(e) => failed(&e),
            }
        }
        In::Compatible { curves } => {
            let mut v = Vec::new();
            for c in curves {
                match build_plain(c, &mut v) {
                    Made::Deferred(d) => return Ok(*d),
                    Made::Failed => return Ok(record(vec![-2.0])),
                    Made::Yes => {}
                }
            }
            match api::make_curves_compatible(&mut v) {
                Ok(()) => {
                    let mut o = vec![0.0, v.len() as f64];
                    for c in &v {
                        put_bspline(&mut o, BSplineCurve::TYPE, c, &c.natural_interval());
                    }
                    record(o)
                }
                Err(e) => failed(&e),
            }
        }
        In::Order { curves } => {
            let mut v = Vec::new();
            for c in curves {
                match build(&k, c, &mut v) {
                    Made::Deferred(d) => return Ok(*d),
                    Made::Failed => return Ok(record(vec![-2.0])),
                    Made::Yes => {}
                }
            }
            match api::order_curves(&k, &v) {
                Ok(order) => {
                    let mut o = vec![0.0, order.len() as f64];
                    o.extend(order.iter().map(|&i| i as f64));
                    record(o)
                }
                Err(e) => failed(&e),
            }
        }
    })
}

// ---- generation -------------------------------------------------------------------

fn p3(p: [f64; 3]) -> P3 {
    [F(p[0]), F(p[1]), F(p[2])]
}

/// A "nasty" float: NaN, infinities, zero, negatives, tiny or huge values.
fn nasty(rng: &mut Rng) -> f64 {
    let choices = [
        f64::NAN,
        f64::INFINITY,
        0.0,
        -1.0,
        1.0e-12,
        1.0e-7,
        1.0e12,
    ];
    rng.choose(&choices).copied().unwrap_or(0.0)
}

/// Mostly a moderate value in `[lo, hi]`, sometimes a nasty one.
fn val(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    if rng.chance(0.04) {
        nasty(rng)
    } else {
        rng.uniform(lo, hi)
    }
}

fn pt(rng: &mut Rng) -> P3 {
    if rng.chance(0.02) {
        let mut p = rng.point3(-50.0, 50.0);
        if let Some(c) = p.get_mut(rng.below(3) as usize) {
            *c = nasty(rng);
        }
        p3(p)
    } else {
        p3(rng.point3(-50.0, 50.0))
    }
}

/// A planar point (z = `z`) in a box.
fn flat(rng: &mut Rng, z: f64) -> P3 {
    p3([rng.uniform(-30.0, 30.0), rng.uniform(-30.0, 30.0), z])
}

/// One curve of the kinds up to `max` (0 curve, 1 circle, 2 arc, 3 segment, 4 analytic
/// circle), planar at `z` most of the time.
fn gen_curve(rng: &mut Rng, max: u64) -> Crv {
    let z = if rng.chance(0.8) { 0.0 } else { rng.uniform(-5.0, 5.0) };
    match rng.below(max + 1) {
        0 => {
            let n = 4 + rng.below(6) as usize;
            Crv::Curve {
                points: (0..n)
                    .map(|_| if rng.chance(0.85) { flat(rng, z) } else { pt(rng) })
                    .collect(),
            }
        }
        1 => Crv::Circle {
            center: flat(rng, z),
            radius: F(val(rng, 0.5, 30.0)),
        },
        2 => Crv::Arc {
            center: flat(rng, z),
            radius: F(val(rng, 0.5, 30.0)),
            start_deg: F(rng.uniform(-180.0, 180.0)),
            end_deg: F(rng.uniform(-180.0, 360.0)),
        },
        3 => Crv::Segment {
            start: flat(rng, z),
            end: if rng.chance(0.85) { flat(rng, z) } else { pt(rng) },
        },
        _ => Crv::AnalyticCircle {
            center: flat(rng, z),
            radius: F(val(rng, 0.5, 30.0)),
        },
    }
}

/// The segments of a closed polygon through `corners`, shuffled and some reversed.
fn polygon_loop(rng: &mut Rng, corners: &[[f64; 3]]) -> Vec<Crv> {
    let n = corners.len();
    let mut segs: Vec<Crv> = (0..n)
        .map(|i| {
            let a = corners.get(i).copied().unwrap_or([0.0; 3]);
            let b = corners.get((i + 1) % n).copied().unwrap_or([0.0; 3]);
            if rng.chance(0.3) {
                Crv::Segment {
                    start: p3(b),
                    end: p3(a),
                }
            } else {
                Crv::Segment {
                    start: p3(a),
                    end: p3(b),
                }
            }
        })
        .collect();
    // Fisher-Yates with the case's generator
    for i in (1..segs.len()).rev() {
        let j = rng.below(i as u64 + 1) as usize;
        segs.swap(i, j);
    }
    segs
}

fn gen_case(rng: &mut Rng) -> In {
    match rng.below(5) {
        0 => {
            let n = 2 + rng.below(9) as usize;
            let mut points: Vec<P3> = (0..n).map(|_| pt(rng)).collect();
            if n > 2 && rng.chance(0.1) {
                // coincident neighbours
                if let Some(&a) = points.first() {
                    if let Some(b) = points.get_mut(1) {
                        *b = a;
                    }
                }
            }
            In::Interp {
                points,
                parameterization: rng.below(3) as u32,
            }
        }
        1 => In::Offset {
            curve: gen_curve(rng, 4),
            distance: F(if rng.chance(0.1) {
                nasty(rng)
            } else {
                rng.uniform(-5.0, 5.0)
            }),
        },
        2 => {
            let relative = rng.chance(0.9);
            let parameter = if relative {
                *rng.choose(&[0.0, 0.25, 0.5, 0.75, 1.0, -0.1, 1.1])
                    .unwrap_or(&0.5)
            } else {
                rng.uniform(-1.0, 2.0)
            };
            In::Continuity {
                curve: gen_curve(rng, 4),
                parameter: F(if rng.chance(0.03) { f64::NAN } else { parameter }),
                relative,
            }
        }
        3 => {
            let n = rng.below(5) as usize;
            In::Compatible {
                curves: (0..n).map(|_| gen_curve(rng, 2)).collect(),
            }
        }
        _ => {
            let curves = match rng.below(5) {
                0 => vec![Crv::Rectangle {
                    center: flat(rng, 0.0),
                    length: F(val(rng, 0.5, 40.0)),
                    width: F(val(rng, 0.5, 40.0)),
                }],
                1 | 2 => {
                    let n = 3 + rng.below(4) as usize;
                    let c = [rng.uniform(-10.0, 10.0), rng.uniform(-10.0, 10.0)];
                    let r = rng.uniform(1.0, 20.0);
                    let corners: Vec<[f64; 3]> = (0..n)
                        .map(|i| {
                            let a = core::f64::consts::TAU * i as f64 / n as f64;
                            [c[0] + r * a.cos(), c[1] + r * a.sin(), 0.0]
                        })
                        .collect();
                    polygon_loop(rng, &corners)
                }
                3 => (0..2 + rng.below(3))
                    .map(|_| Crv::Circle {
                        center: flat(rng, 0.0),
                        radius: F(rng.uniform(0.5, 10.0)),
                    })
                    .collect(),
                _ => (0..rng.below(5)).map(|_| gen_curve(rng, 4)).collect(),
            };
            In::Order { curves }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.api_curve_ops"
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
