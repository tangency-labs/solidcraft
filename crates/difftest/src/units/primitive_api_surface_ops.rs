//! Runner for unit `primitive.api_surface_ops`: the `SM_API` surface utilities
//! (`SmApiCreateSurface`, `SmApiCreateSurfaceFromCornerPoints`, `SmApiCreateOffsetSurface`,
//! `SmApiDropCurveToSrf`, `SmApiLiftUVCurveFromSrf`).
//!
//! C++: `solidcore_ffi::units::primitive_api_surface_ops` (upstream
//! `source/SM_API/src/SmApiSurfaces.cpp`, `SmApiCurves.cpp`). Rust:
//! `solidcore::primitive::api_surface_ops`.
//!
//! A case is one API call (`op` plus its arguments; floats may be `"NaN"`, `"inf"`,
//! `"-inf"`). Input surfaces and curves are made by the `SM_API` constructors on each
//! side; a constructor failure records `-2` alone. Both sides produce the record
//! documented in the ffi module: the status (`0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce),
//! then the result, reported as the values `rec`. A case whose Rust side reaches an
//! unported unit is deferred.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use solidcore::curve::curve::{Curve, CurveKind};
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::primitive::api_curves;
use solidcore::primitive::api_primitives;
use solidcore::primitive::api_surface_ops as api;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1;
use solidcore::SolidError;
use solidcore_ffi::units::primitive_api_surface_ops::{self as ffi, Call, InCurve, InSurface};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 400;

/// `primitive.api_surface_ops` runner.
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

/// One input surface, made by an `SM_API` constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Srf {
    Net { rows: u64, columns: u64, points: Vec<P3> },
    Corners { corners: [P3; 4] },
    SphereFace { center: P3, radius: F },
    Torus { center: P3, major: F, minor: F },
}

/// One input curve, made by an `SM_API` constructor.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
enum Crv {
    Curve { points: Vec<P3> },
    Circle { center: P3, radius: F },
    Arc { center: P3, radius: F, start_deg: F, end_deg: F },
    Segment { start: P3, end: P3 },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum In {
    Net {
        rows: u64,
        columns: u64,
        points: Vec<P3>,
    },
    Corners {
        corners: [P3; 4],
    },
    Offset {
        surface: Srf,
        distance: F,
    },
    Drop {
        curve: Crv,
        surface: Srf,
        #[serde(default)]
        lift: bool,
    },
    Lift {
        uv_curve: Crv,
        surface: Srf,
    },
}

fn a3(p: &P3) -> [f64; 3] {
    [p[0].0, p[1].0, p[2].0]
}

fn v3(p: &P3) -> Vec3 {
    Vec3::from_array(a3(p))
}

fn in_surface(s: &Srf) -> InSurface {
    match s {
        Srf::Net {
            rows,
            columns,
            points,
        } => InSurface::Net {
            rows: *rows,
            columns: *columns,
            points: points.iter().map(a3).collect(),
        },
        Srf::Corners { corners } => InSurface::Corners {
            corners: corners.each_ref().map(a3),
        },
        Srf::SphereFace { center, radius } => InSurface::SphereFace {
            center: a3(center),
            radius: radius.0,
        },
        Srf::Torus {
            center,
            major,
            minor,
        } => InSurface::Torus {
            center: a3(center),
            major: major.0,
            minor: minor.0,
        },
    }
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
    }
}

fn to_call(i: &In) -> Call {
    match i {
        In::Net {
            rows,
            columns,
            points,
        } => Call::Net {
            rows: *rows,
            columns: *columns,
            points: points.iter().map(a3).collect(),
        },
        In::Corners { corners } => Call::Corners {
            corners: corners.each_ref().map(a3),
        },
        In::Offset { surface, distance } => Call::Offset {
            surface: in_surface(surface),
            distance: distance.0,
        },
        In::Drop {
            curve,
            surface,
            lift,
        } => Call::Drop {
            curve: in_curve(curve),
            surface: in_surface(surface),
            lift: *lift,
        },
        In::Lift { uv_curve, surface } => Call::Lift {
            uv_curve: in_curve(uv_curve),
            surface: in_surface(surface),
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

fn flag(b: bool) -> f64 {
    f64::from(u8::from(b))
}

fn put_ivl(o: &mut Vec<f64>, e: &Extent1d) {
    o.extend([e.min(), e.max()]);
}

fn put_dom(o: &mut Vec<f64>, d: &Extent2d) {
    let (lo, hi) = (d.min(), d.max());
    o.extend([lo.x, lo.y, hi.x, hi.y]);
}

/// The shim's `put_bspline_surface`.
fn put_bspline_surface(o: &mut Vec<f64>, s: &BSplineSurface) {
    o.extend([
        flag(s.gw_nurb().is_some()),
        f64::from(s.knot_type() as i32),
        f64::from(s.form() as i32),
    ]);
    if let Some(sur) = s.gw_nurb() {
        let ix = sur.indices();
        o.extend([ix.n, ix.m, ix.p, ix.q, ix.r, ix.s].map(|x| x as f64));
        for row in sur.rows() {
            for p in row {
                o.extend([p.x, p.y, p.z, p.w]);
            }
        }
        o.extend_from_slice(sur.u_knots());
        o.extend_from_slice(sur.v_knots());
    }
}

/// The shim's `put_surface_samples`: the virtual `GetType`, `GetNaturalUVDomain` and
/// `EvaluatePoint` of the surface.
fn put_surface_samples(k: &Kernel, o: &mut Vec<f64>, s: &Surface) -> Result<(), Box<Output>> {
    o.push(s.type_code() as f64);
    let dom = match s.natural_uv_domain_with_nurb(k) {
        Ok(d) => d,
        Err(e) => return Err(Box::new(failed(&e))),
    };
    put_dom(o, &dom);
    let f = [0.0, 0.5, 1.0];
    for fu in f {
        for fv in f {
            match s.evaluate_point(k, dom.evaluate(fu, fv)) {
                Ok(p) => o.extend([0.0, p.x, p.y, p.z]),
                Err(e) => {
                    if let Some(d) = Output::unported(&e) {
                        return Err(Box::new(d));
                    }
                    o.push(status(&e));
                }
            }
        }
    }
    Ok(())
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
    let natural = c.natural_interval();
    let tc = c.type_code();
    match c.kind() {
        CurveKind::BSpline(b) => put_bspline(o, tc, b, &natural),
        CurveKind::Line(l) => {
            let base = l
                .base_with_nurb()
                .map_err(|e| HarnessError::new(e.to_string()))?;
            put_bspline(o, tc, &base, &natural);
        }
        CurveKind::Ellipse(e) => put_bspline(o, tc, e.base(), &natural),
        CurveKind::Hyperbola(h) => put_bspline(o, tc, h.base(), &natural),
        CurveKind::Parabola(p) => put_bspline(o, tc, p.base(), &natural),
        _ => return Err(HarnessError::new("curve is not an SmBSplineCurve class")),
    }
    Ok(())
}

fn put_curves(o: &mut Vec<f64>, v: &[Curve]) -> Result<(), HarnessError> {
    o.push(v.len() as f64);
    for c in v {
        put_curve(o, c)?;
    }
    Ok(())
}

fn record(o: Vec<f64>) -> Output {
    Output::new().values("rec", o)
}

fn failed(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| record(vec![status(e)]))
}

/// An input's constructor failed (`-2`), or reached an unported unit.
fn build_failed(e: &SolidError) -> Box<Output> {
    Box::new(Output::unported(e).unwrap_or_else(|| record(vec![-2.0])))
}

/// The shim's `read_surface` (a sphere face's surface is a copy of the face's).
fn surface_of(k: &mut Kernel, s: &Srf) -> Result<Surface, Box<Output>> {
    let made = match s {
        Srf::Net {
            rows,
            columns,
            points,
        } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            api::create_surface(&pts, *rows, *columns)
        }
        Srf::Corners { corners } => {
            let [a, b, c, d] = corners.each_ref().map(v3);
            api::create_surface_from_corner_points(a, b, c, d)
        }
        Srf::SphereFace { center, radius } => {
            let b = api_primitives::create_sphere(k, v3(center), radius.0)
                .map_err(|e| build_failed(&e))?;
            let faces = p1::faces(&b, None).map_err(|e| build_failed(&e))?;
            let face = faces.first().ok_or_else(|| Box::new(record(vec![-2.0])))?;
            return b
                .face(*face)
                .ok()
                .and_then(|f| f.surface.clone())
                .ok_or_else(|| Box::new(record(vec![-2.0])));
        }
        Srf::Torus {
            center,
            major,
            minor,
        } => {
            return api_primitives::create_torus_surface(k, v3(center), major.0, minor.0)
                .map(Surface::from)
                .map_err(|e| build_failed(&e));
        }
    };
    made.map(Surface::from).map_err(|e| build_failed(&e))
}

fn curve_of(c: &Crv) -> Result<Curve, Box<Output>> {
    let made = match c {
        Crv::Curve { points } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            api_curves::create_curve(&pts).map(Curve::from)
        }
        Crv::Circle { center, radius } => {
            api_curves::create_circle(v3(center), radius.0).map(Curve::from)
        }
        Crv::Arc {
            center,
            radius,
            start_deg,
            end_deg,
        } => api_curves::create_arc(v3(center), radius.0, start_deg.0, end_deg.0)
            .map(Curve::from),
        Crv::Segment { start, end } => {
            api_curves::create_line_segment(v3(start), v3(end)).map(Curve::from)
        }
    };
    made.map_err(|e| build_failed(&e))
}

/// The record of a created B-spline surface (ops net and corners).
fn created(k: &Kernel, r: solidcore::Result<BSplineSurface>) -> Output {
    match r {
        Ok(b) => {
            let s = Surface::from(b);
            let mut o = vec![0.0, s.type_code() as f64];
            if let solidcore::surface::surface::SurfaceKind::BSpline(b) = s.kind() {
                put_bspline_surface(&mut o, b);
            }
            match put_surface_samples(k, &mut o, &s) {
                Ok(()) => record(o),
                Err(d) => *d,
            }
        }
        Err(e) => failed(&e),
    }
}

fn rust(i: &In) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    Ok(match i {
        In::Net {
            rows,
            columns,
            points,
        } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            created(&k, api::create_surface(&pts, *rows, *columns))
        }
        In::Corners { corners } => {
            let [a, b, c, d] = corners.each_ref().map(v3);
            created(&k, api::create_surface_from_corner_points(a, b, c, d))
        }
        In::Offset { surface, distance } => {
            let s = match surface_of(&mut k, surface) {
                Ok(s) => s,
                Err(out) => return Ok(*out),
            };
            match api::create_offset_surface(&k, &s, distance.0) {
                Ok(r) => {
                    let mut o = vec![0.0];
                    match put_surface_samples(&k, &mut o, &r) {
                        Ok(()) => record(o),
                        Err(d) => *d,
                    }
                }
                Err(e) => failed(&e),
            }
        }
        In::Drop {
            curve,
            surface,
            lift,
        } => {
            let c = match curve_of(curve) {
                Ok(c) => c,
                Err(out) => return Ok(*out),
            };
            let s = match surface_of(&mut k, surface) {
                Ok(s) => s,
                Err(out) => return Ok(*out),
            };
            match api::drop_curve_to_srf(&k, &c, &s, *lift) {
                Ok(d) => {
                    let mut o = vec![0.0];
                    put_curves(&mut o, &d.uv_curves)?;
                    if let Some(c3) = &d.curves_3d {
                        put_curves(&mut o, c3)?;
                    }
                    record(o)
                }
                Err(e) => failed(&e),
            }
        }
        In::Lift { uv_curve, surface } => {
            let c = match curve_of(uv_curve) {
                Ok(c) => c,
                Err(out) => return Ok(*out),
            };
            let s = match surface_of(&mut k, surface) {
                Ok(s) => s,
                Err(out) => return Ok(*out),
            };
            match api::lift_uv_curve_from_srf(&k, &c, &s) {
                Ok(c3) => {
                    let mut o = vec![0.0];
                    put_curves(&mut o, core::slice::from_ref(&c3))?;
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

/// A wavy control net over `[0, 10 rows] x [0, 10 cols]`.
fn net(rng: &mut Rng, rows: u64, columns: u64) -> Vec<P3> {
    let amp = rng.uniform(0.0, 4.0);
    let tilt = rng.uniform(-0.3, 0.3);
    let mut v = Vec::new();
    for i in 0..rows {
        for j in 0..columns {
            let (x, y) = (i as f64 * 10.0, j as f64 * 10.0);
            let z = tilt * x + amp * ((x * 0.07).sin() + (y * 0.05).cos()) + rng.uniform(-0.5, 0.5);
            v.push(p3([x, y, z]));
        }
    }
    v
}

fn gen_surface(rng: &mut Rng) -> Srf {
    if rng.chance(0.5) {
        let rows = 4 + rng.below(4);
        let columns = 4 + rng.below(4);
        Srf::Net {
            rows,
            columns,
            points: net(rng, rows, columns),
        }
    } else {
        let s = rng.uniform(5.0, 40.0);
        let z = [rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0), rng.uniform(-3.0, 3.0)];
        Srf::Corners {
            corners: [
                p3([0.0, 0.0, z[0]]),
                p3([s, 0.0, z[1]]),
                p3([0.0, s, z[2]]),
                p3([s, s, z[3]]),
            ],
        }
    }
}

/// A curve above the surfaces' footprint (x, y in `[lo, hi]`).
fn gen_curve(rng: &mut Rng, lo: f64, hi: f64, z: f64) -> Crv {
    match rng.below(4) {
        0 => {
            let n = 4 + rng.below(4) as usize;
            Crv::Curve {
                points: (0..n)
                    .map(|_| p3([rng.uniform(lo, hi), rng.uniform(lo, hi), z + rng.uniform(-1.0, 1.0)]))
                    .collect(),
            }
        }
        1 => Crv::Circle {
            center: p3([rng.uniform(lo, hi), rng.uniform(lo, hi), z]),
            radius: F(rng.uniform(0.1, (hi - lo) / 3.0)),
        },
        2 => Crv::Arc {
            center: p3([rng.uniform(lo, hi), rng.uniform(lo, hi), z]),
            radius: F(rng.uniform(0.1, (hi - lo) / 3.0)),
            start_deg: F(rng.uniform(-90.0, 90.0)),
            end_deg: F(rng.uniform(100.0, 270.0)),
        },
        _ => Crv::Segment {
            start: p3([rng.uniform(lo, hi), rng.uniform(lo, hi), z]),
            end: p3([rng.uniform(lo, hi), rng.uniform(lo, hi), z]),
        },
    }
}

fn gen_case(rng: &mut Rng) -> In {
    match rng.below(5) {
        0 => {
            let rows = rng.below(8);
            let columns = rng.below(8);
            let mut points = net(rng, rows, columns);
            if rng.chance(0.1) {
                points.pop();
            }
            In::Net {
                rows,
                columns,
                points,
            }
        }
        1 => match gen_surface(rng) {
            Srf::Corners { corners } => In::Corners { corners },
            // (gen_surface makes no sphere face or torus)
            Srf::Net { .. } | Srf::SphereFace { .. } | Srf::Torus { .. } => In::Corners {
                corners: [
                    p3(rng.point3(-20.0, 20.0)),
                    p3(rng.point3(-20.0, 20.0)),
                    p3(rng.point3(-20.0, 20.0)),
                    p3(rng.point3(-20.0, 20.0)),
                ],
            },
        },
        2 => match rng.below(5) {
            // an analytic sphere face or torus, offset inward past collapse or not
            0 => {
                let radius = rng.uniform(1.0, 20.0);
                In::Offset {
                    surface: Srf::SphereFace {
                        center: p3(rng.point3(-10.0, 10.0)),
                        radius: F(radius),
                    },
                    distance: F(radius * rng.uniform(-2.5, 1.0)),
                }
            }
            1 => {
                let minor = rng.uniform(0.5, 4.0);
                In::Offset {
                    surface: Srf::Torus {
                        center: p3(rng.point3(-10.0, 10.0)),
                        major: F(minor * rng.uniform(1.5, 6.0)),
                        minor: F(minor),
                    },
                    distance: F(minor * rng.uniform(-3.0, 1.0)),
                }
            }
            _ => In::Offset {
                surface: gen_surface(rng),
                distance: F(if rng.chance(0.05) { 0.0 } else { rng.uniform(-3.0, 3.0) }),
            },
        },
        3 => {
            let z = rng.uniform(-20.0, 20.0);
            In::Drop {
                curve: gen_curve(rng, 2.0, 28.0, z),
                surface: gen_surface(rng),
                lift: rng.chance(0.7),
            }
        }
        _ => In::Lift {
            uv_curve: gen_curve(rng, 0.05, 0.95, 0.0),
            surface: gen_surface(rng),
        },
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.api_surface_ops"
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
