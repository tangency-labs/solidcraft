//! Runner for unit `primitive.api_curves`: the `SM_API` curve constructors.
//!
//! C++: `solidcore_ffi::units::primitive_api_curves` (upstream
//! `source/SM_API/src/SmApiCurves.cpp`). Rust: `solidcore::primitive::api_curves`.
//!
//! A case is one API call (`op` plus its arguments; floats may be `"NaN"`, `"inf"`,
//! `"-inf"`). Both sides produce the record documented in the ffi module: the status
//! (`0` = `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1`
//! for a Rust error upstream cannot produce), then the created curve(s), reported as
//! the values `rec`. A case whose Rust side reaches an unported unit is deferred.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use solidcore::curve::ellipse::Ellipse;
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm};
use solidcore::primitive::api_curves as api;
use solidcore::SolidError;
use solidcore_ffi::units::primitive_api_curves::{self as ffi, Call};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `primitive.api_curves` runner.
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

fn yes() -> bool {
    true
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum In {
    LineSegment {
        start: P3,
        end: P3,
    },
    Line {
        start: P3,
        direction: P3,
    },
    Circle {
        center: P3,
        radius: F,
    },
    AnalyticCircle {
        center: P3,
        radius: F,
    },
    Ellipse {
        origin: P3,
        radius1: F,
        radius2: F,
    },
    Arc {
        center: P3,
        radius: F,
        start_deg: F,
        end_deg: F,
    },
    Helix {
        origin: P3,
        height: F,
        radius_start: F,
        radius_end: F,
        num_turns: F,
        #[serde(default = "yes")]
        right_handed: bool,
        tolerance: F,
    },
    Rectangle {
        center: P3,
        length: F,
        width: F,
    },
    Polygon {
        center: P3,
        sides: u64,
        radius: F,
    },
    Curve {
        points: Vec<P3>,
    },
    Canonical {
        points: Vec<P3>,
        knots: Vec<F>,
        mults: Vec<usize>,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        weights: Option<Vec<F>>,
        degree: usize,
        #[serde(default = "six")]
        form: u32,
    },
}

fn six() -> u32 {
    6
}

fn a3(p: &P3) -> [f64; 3] {
    [p[0].0, p[1].0, p[2].0]
}

fn v3(p: &P3) -> Vec3 {
    Vec3::from_array(a3(p))
}

fn fs(v: &[F]) -> Vec<f64> {
    v.iter().map(|x| x.0).collect()
}

fn to_call(i: &In) -> Call {
    match i {
        In::LineSegment { start, end } => Call::LineSegment {
            start: a3(start),
            end: a3(end),
        },
        In::Line { start, direction } => Call::Line {
            start: a3(start),
            direction: a3(direction),
        },
        In::Circle { center, radius } => Call::Circle {
            center: a3(center),
            radius: radius.0,
        },
        In::AnalyticCircle { center, radius } => Call::AnalyticCircle {
            center: a3(center),
            radius: radius.0,
        },
        In::Ellipse {
            origin,
            radius1,
            radius2,
        } => Call::Ellipse {
            origin: a3(origin),
            radius1: radius1.0,
            radius2: radius2.0,
        },
        In::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => Call::Arc {
            center: a3(center),
            radius: radius.0,
            start_deg: start_deg.0,
            end_deg: end_deg.0,
        },
        In::Helix {
            origin,
            height,
            radius_start,
            radius_end,
            num_turns,
            right_handed,
            tolerance,
        } => Call::Helix {
            origin: a3(origin),
            height: height.0,
            radius_start: radius_start.0,
            radius_end: radius_end.0,
            num_turns: num_turns.0,
            right_handed: *right_handed,
            tolerance: tolerance.0,
        },
        In::Rectangle {
            center,
            length,
            width,
        } => Call::Rectangle {
            center: a3(center),
            length: length.0,
            width: width.0,
        },
        In::Polygon {
            center,
            sides,
            radius,
        } => Call::Polygon {
            center: a3(center),
            sides: *sides,
            radius: radius.0,
        },
        In::Curve { points } => Call::Curve {
            points: points.iter().map(a3).collect(),
        },
        In::Canonical {
            points,
            knots,
            mults,
            weights,
            degree,
            form,
        } => Call::Canonical {
            points: points.iter().map(a3).collect(),
            knots: fs(knots),
            mults: mults.clone(),
            weights: weights.as_deref().map(fs),
            degree: *degree,
            form: *form,
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

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
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

fn put_line(o: &mut Vec<f64>, l: &Line) {
    put_vec(o, l.line_point());
    put_vec(o, l.line_vector());
    put_ivl(o, &l.step_interval());
    o.extend([l.line_scale(), b(l.inside_out())]);
    put_bspline(o, l.type_code(), l.base(), &l.natural_interval());
}

fn put_ellipse(o: &mut Vec<f64>, e: &Ellipse) {
    o.push(e.type_code() as f64);
    let pos = e.position();
    put_vec(o, pos.origin());
    put_vec(o, pos.x_axis());
    put_vec(o, pos.y_axis());
    put_ivl(o, &e.step_interval());
    o.extend([e.x_radius(), e.y_radius(), b(e.inside_out())]);
    let pc = e.polar_converter();
    o.push(b(pc.is_polar_conversion_possible()));
    o.push(pc.natural_knots().len() as f64);
    o.extend_from_slice(pc.natural_knots());
    o.push(pc.angles().len() as f64);
    o.extend_from_slice(pc.angles());
    o.push(pc.exact_conversion().len() as f64);
    o.extend(pc.exact_conversion().iter().map(|&x| b(x)));
    put_bspline(o, e.type_code(), e.base(), &e.base().natural_interval());
}

fn form_of(code: u32) -> Option<BSplineCurveForm> {
    Some(match code {
        0 => BSplineCurveForm::PolylineForm,
        1 => BSplineCurveForm::CircularArc,
        2 => BSplineCurveForm::EllipticArc,
        3 => BSplineCurveForm::ParabolicArc,
        4 => BSplineCurveForm::HyperbolicArc,
        5 => BSplineCurveForm::HelicalArc,
        6 => BSplineCurveForm::Unspecified,
        _ => return None,
    })
}

/// The record of a result: status, then the dump of the value.
fn rec<T>(r: solidcore::Result<T>, put: impl FnOnce(&mut Vec<f64>, &T)) -> Output {
    match r {
        Ok(v) => {
            let mut o = vec![0.0];
            put(&mut o, &v);
            Output::new().values("rec", o)
        }
        Err(e) => {
            if let Some(d) = Output::unported(&e) {
                return d;
            }
            Output::new().values("rec", vec![status(&e)])
        }
    }
}

fn put_plain(o: &mut Vec<f64>, c: &BSplineCurve) {
    put_bspline(o, BSplineCurve::TYPE, c, &c.natural_interval());
}

fn put_lines(o: &mut Vec<f64>, v: &Vec<Line>) {
    o.push(v.len() as f64);
    for l in v {
        put_line(o, l);
    }
}

fn rust(i: &In) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    Ok(match i {
        In::LineSegment { start, end } => {
            rec(api::create_line_segment(v3(start), v3(end)), put_line)
        }
        In::Line { start, direction } => rec(api::create_line(v3(start), v3(direction)), put_line),
        In::Circle { center, radius } => rec(api::create_circle(v3(center), radius.0), put_plain),
        In::AnalyticCircle { center, radius } => rec(
            api::create_analytic_circle(&k, v3(center), radius.0),
            put_ellipse,
        ),
        In::Ellipse {
            origin,
            radius1,
            radius2,
        } => rec(
            api::create_ellipse(&k, v3(origin), radius1.0, radius2.0),
            put_ellipse,
        ),
        In::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => rec(
            api::create_arc(v3(center), radius.0, start_deg.0, end_deg.0),
            put_plain,
        ),
        In::Helix {
            origin,
            height,
            radius_start,
            radius_end,
            num_turns,
            right_handed,
            tolerance,
        } => rec(
            api::create_helix(&api::HelixParams {
                origin: v3(origin),
                height: height.0,
                radius_start: radius_start.0,
                radius_end: radius_end.0,
                num_turns: num_turns.0,
                right_handed: *right_handed,
                tolerance: tolerance.0,
            }),
            put_plain,
        ),
        In::Rectangle {
            center,
            length,
            width,
        } => rec(
            api::create_rectangle(v3(center), length.0, width.0),
            put_lines,
        ),
        In::Polygon {
            center,
            sides,
            radius,
        } => rec(
            api::create_regular_polygon(v3(center), *sides, radius.0),
            put_lines,
        ),
        In::Curve { points } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            rec(api::create_curve(&pts), put_plain)
        }
        In::Canonical {
            points,
            knots,
            mults,
            weights,
            degree,
            form,
        } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            let kn = fs(knots);
            let w = weights.as_deref().map(fs);
            let form = form_of(*form).ok_or_else(|| HarnessError::new("bad form"))?;
            rec(
                api::create_canonical_curve(&api::CanonicalCurveParams {
                    points: &pts,
                    knots: &kn,
                    knot_multiplicities: mults,
                    weights: w.as_deref(),
                    degree: *degree,
                    form,
                }),
                put_plain,
            )
        }
    })
}

// ---- generation -------------------------------------------------------------------

fn f(x: f64) -> F {
    F(x)
}

fn p3(p: [f64; 3]) -> P3 {
    [F(p[0]), F(p[1]), F(p[2])]
}

/// A "nasty" float: NaN, infinities, zero, negatives, tiny or huge values.
fn nasty(rng: &mut Rng) -> f64 {
    let choices = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -1.0,
        1.0e-12,
        2.0e-12,
        1.0e-7,
        1.0e18,
        1.0e19,
        f64::MAX,
        5e-324,
    ];
    rng.choose(&choices).copied().unwrap_or(0.0)
}

/// Mostly a moderate value in `[lo, hi]`, sometimes a nasty one.
fn val(rng: &mut Rng, lo: f64, hi: f64) -> f64 {
    if rng.chance(0.08) {
        nasty(rng)
    } else {
        rng.uniform(lo, hi)
    }
}

fn pt(rng: &mut Rng) -> P3 {
    match rng.below(20) {
        0 => {
            let mut p = rng.point3(-50.0, 50.0);
            let i = rng.below(3) as usize;
            if let Some(c) = p.get_mut(i) {
                *c = nasty(rng);
            }
            p3(p)
        }
        // far from the origin: extents may collapse in double precision
        1 => p3(rng.point3(-1.0e16, 1.0e16)),
        _ => p3(rng.point3(-50.0, 50.0)),
    }
}

fn gen_case(rng: &mut Rng) -> In {
    match rng.below(11) {
        0 => {
            let start = pt(rng);
            let end = if rng.chance(0.1) { start } else { pt(rng) };
            In::LineSegment { start, end }
        }
        1 => {
            let direction = if rng.chance(0.1) {
                p3([0.0, 0.0, rng.uniform(-1e-12, 1e-12)])
            } else {
                pt(rng)
            };
            In::Line {
                start: pt(rng),
                direction,
            }
        }
        2 => In::Circle {
            center: pt(rng),
            radius: f(val(rng, 1e-3, 100.0)),
        },
        3 => In::AnalyticCircle {
            center: pt(rng),
            radius: f(val(rng, 1e-3, 100.0)),
        },
        4 => In::Ellipse {
            origin: pt(rng),
            radius1: f(val(rng, 1e-3, 100.0)),
            radius2: f(val(rng, 1e-3, 100.0)),
        },
        5 => In::Arc {
            center: pt(rng),
            radius: f(val(rng, 1e-3, 100.0)),
            start_deg: f(val(rng, -400.0, 400.0)),
            end_deg: f(val(rng, -400.0, 400.0)),
        },
        6 => {
            let r0 = val(rng, 0.1, 10.0);
            let r1 = if rng.chance(0.5) {
                r0
            } else {
                val(rng, 0.1, 10.0)
            };
            In::Helix {
                origin: pt(rng),
                height: f(val(rng, 0.1, 50.0)),
                radius_start: f(r0),
                radius_end: f(r1),
                num_turns: f(if rng.chance(0.05) {
                    rng.uniform(100.0, 130.0)
                } else {
                    val(rng, 0.05, 8.0)
                }),
                right_handed: rng.chance(0.5),
                tolerance: f(if rng.chance(0.2) {
                    rng.uniform(1e-6, 1e-5)
                } else {
                    val(rng, 1e-4, 1e-1)
                }),
            }
        }
        7 => In::Rectangle {
            center: pt(rng),
            length: f(val(rng, 1e-3, 100.0)),
            width: f(val(rng, 1e-3, 100.0)),
        },
        8 => In::Polygon {
            center: pt(rng),
            sides: rng.below(40),
            radius: f(val(rng, 1e-6, 100.0)),
        },
        9 => {
            let n = rng.below(12) as usize;
            In::Curve {
                points: (0..n).map(|_| pt(rng)).collect(),
            }
        }
        _ => {
            let degree = 1 + rng.below(5) as usize;
            let n = degree + 1 + rng.below(8) as usize;
            // clamped knot vector with n + degree + 1 entries
            let interior = n - degree - 1;
            let mut knots = vec![0.0];
            let mut mults = vec![degree + 1];
            let mut u = 0.0;
            let mut left = interior;
            while left > 0 {
                let m = (1 + rng.below(degree as u64) as usize).min(left);
                u += rng.uniform(0.05, 1.0);
                knots.push(u);
                mults.push(m);
                left -= m;
            }
            knots.push(u + rng.uniform(0.05, 1.0));
            mults.push(degree + 1);
            let weights = if rng.chance(0.5) {
                Some(
                    (0..n)
                        .map(|_| {
                            f(if rng.chance(0.05) {
                                -1.0
                            } else {
                                rng.uniform(0.2, 3.0)
                            })
                        })
                        .collect(),
                )
            } else {
                None
            };
            In::Canonical {
                points: (0..n).map(|_| p3(rng.point3(-20.0, 20.0))).collect(),
                knots: knots.into_iter().map(F).collect(),
                mults,
                weights,
                degree,
                form: rng.below(7) as u32,
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.api_curves"
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
