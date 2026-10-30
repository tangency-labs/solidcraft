//! Runner for unit `primitive.api_trim_ops`: the `SM_API` trimmed-surface functions
//! (`SmApiTrimSurfaceWith3dCurves`, `SmApiTrimProjectParallel`, `SmApiCreatePlanarFaces`,
//! `SmApiProjectAndTrim`).
//!
//! C++: `solidcore_ffi::units::primitive_api_trim_ops` (upstream
//! `source/SM_API/src/SmApiTrimmedSurfaces.cpp`, `SmApiBrep.cpp`). Rust:
//! `solidcore::primitive::api_trim_ops`.
//!
//! A case is one API call (`op` plus its arguments; floats may be `"NaN"`, `"inf"`,
//! `"-inf"`). Input surfaces, curves and boxes are made by the `SM_API` constructors on
//! each side; a constructor failure records `-2` alone. Both sides produce the record
//! documented in the ffi module: the status (`0` = `SM_SUCCESS`, else the `SmStatus`;
//! `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot produce),
//! then the B-rep's counts and `support/primitive_record.h` record, reported as the
//! values `rec`. A case whose Rust side reaches an unported unit is deferred.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use solidcore::blend::fillet_executive::api::circular_fillet;
use solidcore::curve::curve::Curve;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::primitive::api_curves;
use solidcore::primitive::api_primitives;
use solidcore::primitive::api_surface_ops;
use solidcore::primitive::api_trim_ops as api;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p10::TrimType;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::FfiError;
use solidcore_ffi::units::primitive_api_trim_ops::{
    self as ffi, Call, InBox, InCurve, InObject, InSurface, Reproject,
};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 160;

/// `primitive.api_trim_ops` runner.
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
}

/// One input curve spec, made by an `SM_API` constructor.
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

/// `SmApiCreateBox(origin, length, width, height)`, then with `fillet`
/// `SmApiCircularFillet(radius)`.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Bx {
    origin: P3,
    length: F,
    width: F,
    height: F,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    fillet: Option<F>,
}

/// The object `SmApiTrimProjectParallel` trims.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Obj {
    Surface(Srf),
    Box(Bx),
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum Tt {
    KeepPoint,
    DeletePoint,
    Split,
}

/// A second `SmApiTrimProjectParallel` on the first one's result.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct Again {
    curve: Crv,
    direction: P3,
    reference: P3,
    trim_type: Tt,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "op")]
enum In {
    Trim3d {
        surface: Srf,
        loops: Vec<u64>,
        curves: Vec<Crv>,
    },
    ProjectParallel {
        object: Obj,
        curve: Crv,
        direction: P3,
        reference: P3,
        trim_type: Tt,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        again: Option<Again>,
    },
    PlanarFaces {
        curves: Vec<Crv>,
    },
    ProjectAndTrim {
        #[serde(rename = "box")]
        brep: Bx,
        curve: Crv,
        direction: P3,
        reference: P3,
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

fn in_box(b: &Bx) -> InBox {
    InBox {
        origin: a3(&b.origin),
        length: b.length.0,
        width: b.width.0,
        height: b.height.0,
        fillet: b.fillet.map(|r| r.0),
    }
}

const fn tt_code(t: Tt) -> u8 {
    match t {
        Tt::KeepPoint => 0,
        Tt::DeletePoint => 1,
        Tt::Split => 2,
    }
}

fn to_call(i: &In) -> Call {
    match i {
        In::Trim3d {
            surface,
            loops,
            curves,
        } => Call::Trim3d {
            surface: in_surface(surface),
            loops: loops.clone(),
            curves: curves.iter().map(in_curve).collect(),
        },
        In::ProjectParallel {
            object,
            curve,
            direction,
            reference,
            trim_type,
            again,
        } => Call::ProjectParallel {
            object: match object {
                Obj::Surface(s) => InObject::Surface(in_surface(s)),
                Obj::Box(b) => InObject::Box(in_box(b)),
            },
            curve: in_curve(curve),
            direction: a3(direction),
            reference: a3(reference),
            trim_type: tt_code(*trim_type),
            again: again.as_ref().map(|a| Reproject {
                curve: in_curve(&a.curve),
                direction: a3(&a.direction),
                reference: a3(&a.reference),
                trim_type: tt_code(a.trim_type),
            }),
        },
        In::PlanarFaces { curves } => Call::PlanarFaces {
            curves: curves.iter().map(in_curve).collect(),
        },
        In::ProjectAndTrim {
            brep,
            curve,
            direction,
            reference,
        } => Call::ProjectAndTrim {
            brep: in_box(brep),
            curve: in_curve(curve),
            direction: a3(direction),
            reference: a3(reference),
        },
    }
}

// ---- C++ side ---------------------------------------------------------------------

fn cpp(i: &In) -> Result<Output, HarnessError> {
    match ffi::run(&to_call(i)) {
        Ok(v) => Ok(Output::new().values("rec", v)),
        // an input the ffi guard refuses
        Err(e @ FfiError::InvalidArgument(_)) => Ok(Output::rejected(format!("ffi guard: {e}"))),
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

fn surface_of(s: &Srf) -> Result<Surface, Box<Output>> {
    let made = match s {
        Srf::Net {
            rows,
            columns,
            points,
        } => {
            let pts: Vec<Vec3> = points.iter().map(v3).collect();
            api_surface_ops::create_surface(&pts, *rows, *columns)
        }
        Srf::Corners { corners } => {
            let [a, b, c, d] = corners.each_ref().map(v3);
            api_surface_ops::create_surface_from_corner_points(a, b, c, d)
        }
    };
    made.map(Surface::from).map_err(|e| build_failed(&e))
}

/// The curves one spec makes, appended to `out` (the shim's `read_curve`).
fn curves_of(k: &Kernel, c: &Crv, out: &mut Vec<Curve>) -> Result<(), Box<Output>> {
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
    match made {
        Ok(v) => {
            out.extend(v);
            Ok(())
        }
        Err(e) => Err(build_failed(&e)),
    }
}

fn all_curves(k: &Kernel, specs: &[Crv]) -> Result<Vec<Curve>, Box<Output>> {
    let mut out = Vec::new();
    for c in specs {
        curves_of(k, c, &mut out)?;
    }
    Ok(out)
}

/// The projected curve (any kind but a rectangle, which the ffi refuses; the shim's
/// `read_projection_curve`).
fn projection_curve_of(k: &Kernel, c: &Crv) -> Result<Curve, Box<Output>> {
    let mut one = Vec::new();
    curves_of(k, c, &mut one)?;
    match (matches!(c, Crv::Rectangle { .. }), one.pop()) {
        (false, Some(curve)) => Ok(curve),
        _ => Err(Box::new(record(vec![-1.0]))),
    }
}

/// The shim's `read_box`: `SmApiCreateBox`, then with `fillet` `SmApiCircularFillet`.
fn box_of(k: &mut Kernel, b: &Bx) -> Result<Brep, Box<Output>> {
    let (l, w, h) = (b.length.0, b.width.0, b.height.0);
    let mut brep =
        api_primitives::create_box(k, v3(&b.origin), l, w, h).map_err(|e| build_failed(&e))?;
    if let Some(r) = b.fillet {
        circular_fillet(k, &mut brep, r.0).map_err(|e| build_failed(&e))?;
    }
    Ok(brep)
}

/// The shim's `put_brep` for a successful call: `0 1 faces edges vertices` then the
/// `primitive_record` of the B-rep.
fn brep_out(k: &Kernel, b: &Brep) -> Result<Output, HarnessError> {
    let n = |r: solidcore::Result<usize>| r.map_or(-1.0, |n| n as f64);
    let mut o = vec![
        0.0,
        1.0,
        n(p1::faces(b, None).map(|v| v.len())),
        n(p1::edges(b, None).map(|v| v.len())),
        n(p1::vertices(b, None).map(|v| v.len())),
    ];
    match crate::util::primitive::append(k, b, &mut o) {
        Ok(()) => Ok(record(o)),
        Err(e) => Output::unported(&e)
            .ok_or_else(|| HarnessError::new(format!("B-rep record: {e}"))),
    }
}

fn finish(k: &Kernel, r: solidcore::Result<Brep>) -> Result<Output, HarnessError> {
    match r {
        Ok(b) => brep_out(k, &b),
        Err(e) => Ok(failed(&e)),
    }
}

const fn trim_type(t: Tt) -> TrimType {
    match t {
        Tt::KeepPoint => TrimType::KeepPoint,
        Tt::DeletePoint => TrimType::DeletePoint,
        Tt::Split => TrimType::Split,
    }
}

/// Unwraps an input, or returns its record from `rust`.
macro_rules! input {
    ($e:expr) => {
        match $e {
            Ok(v) => v,
            Err(out) => return Ok(*out),
        }
    };
}

fn rust(i: &In) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    match i {
        In::Trim3d {
            surface,
            loops,
            curves,
        } => {
            let s = input!(surface_of(surface));
            let c = input!(all_curves(&k, curves));
            let counts: Vec<usize> = loops
                .iter()
                .map(|&n| usize::try_from(n).map_err(|_| HarnessError::new("loop count")))
                .collect::<Result<_, _>>()?;
            let r = api::trim_surface_with_3d_curves(&mut k, s, &counts, c);
            finish(&k, r)
        }
        In::ProjectParallel {
            object,
            curve,
            direction,
            reference,
            trim_type: t,
            again,
        } => {
            // (the shim reads every input before the first call)
            let object = match object {
                Obj::Surface(s) => api::TrimObject::Surface(Box::new(input!(surface_of(s)))),
                Obj::Box(b) => api::TrimObject::Brep(Box::new(input!(box_of(&mut k, b)))),
            };
            let c = input!(projection_curve_of(&k, curve));
            let c2 = match again {
                Some(a) => Some(input!(projection_curve_of(&k, &a.curve))),
                None => None,
            };
            let r = api::trim_project_parallel(
                &mut k,
                object,
                &c,
                v3(direction),
                v3(reference),
                trim_type(*t),
            );
            match (again, c2) {
                (Some(a), Some(c2)) => match r {
                    Ok(b) => {
                        let r2 = api::trim_project_parallel(
                            &mut k,
                            api::TrimObject::Brep(Box::new(b)),
                            &c2,
                            v3(&a.direction),
                            v3(&a.reference),
                            trim_type(a.trim_type),
                        );
                        // the first call's status, then the second call's record
                        let mut second = finish(&k, r2)?;
                        if let Some(rec) = second.values.get_mut("rec") {
                            rec.insert(0, 0.0);
                        }
                        Ok(second)
                    }
                    Err(e) => Ok(failed(&e)),
                },
                _ => finish(&k, r),
            }
        }
        In::PlanarFaces { curves } => {
            let c = input!(all_curves(&k, curves));
            let r = api::create_planar_faces(&mut k, &c);
            finish(&k, r)
        }
        In::ProjectAndTrim {
            brep,
            curve,
            direction,
            reference,
        } => {
            let mut b = input!(box_of(&mut k, brep));
            let c = input!(projection_curve_of(&k, curve));
            let r = api::project_and_trim(&mut k, &mut b, &c, v3(direction), v3(reference));
            finish(&k, r.map(|()| b))
        }
    }
}

// ---- generation -------------------------------------------------------------------

fn p3(p: [f64; 3]) -> P3 {
    [F(p[0]), F(p[1]), F(p[2])]
}

/// A corner surface over `[0, s]^2`, flat at `z = 0` or (with `tilted`) maybe tilted.
fn gen_corners(rng: &mut Rng, s: f64, tilted: bool) -> Srf {
    let tilt = if !tilted || rng.chance(0.5) { 0.0 } else { rng.uniform(-0.2, 0.2) };
    let z = |x: f64, y: f64| tilt * (x - y);
    Srf::Corners {
        corners: [
            p3([0.0, 0.0, z(0.0, 0.0)]),
            p3([s, 0.0, z(s, 0.0)]),
            p3([0.0, s, z(0.0, s)]),
            p3([s, s, z(s, s)]),
        ],
    }
}

/// A closed polygon of segments through `pts` (counterclockwise unless reversed).
fn polygon(pts: &[[f64; 3]]) -> Vec<Crv> {
    let mut v = Vec::new();
    for (i, a) in pts.iter().enumerate() {
        let b = pts.get((i + 1) % pts.len()).copied().unwrap_or(*a);
        v.push(Crv::Segment {
            start: p3(*a),
            end: p3(b),
        });
    }
    v
}

/// A convex polygon of `n` corners around `(cx, cy)` at height `z`.
fn ngon(rng: &mut Rng, cx: f64, cy: f64, r: f64, z: f64, n: u64) -> Vec<[f64; 3]> {
    let phase = rng.uniform(0.0, 1.0);
    (0..n)
        .map(|i| {
            let a = (i as f64 + phase) * std::f64::consts::TAU / n as f64;
            [cx + r * a.cos(), cy + r * a.sin(), z]
        })
        .collect()
}

fn gen_trim3d(rng: &mut Rng) -> In {
    let s = rng.uniform(8.0, 30.0);
    let surface = gen_corners(rng, s, false);
    let c = s / 2.0;
    let mut curves = Vec::new();
    let mut loops = Vec::new();
    // outer loop
    let n = 3 + rng.below(4);
    let mut outer = ngon(rng, c, c, s * 0.45, 0.0, n);
    if rng.chance(0.15) {
        outer.reverse();
    }
    curves.extend(polygon(&outer));
    loops.push(n);
    // inner loops
    if rng.chance(0.6) {
        let r = rng.uniform(0.05, 0.15) * s;
        let reversed = rng.chance(0.8);
        let (start_deg, end_deg) = if reversed { (360.0, 0.0) } else { (0.0, 360.0) };
        curves.push(if rng.chance(0.5) {
            Crv::Circle {
                center: p3([c, c, 0.0]),
                radius: F(r),
            }
        } else {
            Crv::Arc {
                center: p3([c, c, 0.0]),
                radius: F(r),
                start_deg: F(start_deg),
                end_deg: F(end_deg),
            }
        });
        loops.push(1);
    }
    if rng.chance(0.1) {
        // a loop that counts fewer curves than it was given
        if let Some(last) = loops.last_mut().filter(|n| **n > 1) {
            *last -= 1;
        }
    }
    In::Trim3d {
        surface,
        loops,
        curves,
    }
}

fn gen_planar(rng: &mut Rng) -> In {
    let z = rng.uniform(-10.0, 10.0);
    let mut curves = Vec::new();
    match rng.below(4) {
        0 => curves.push(Crv::Rectangle {
            center: p3([rng.uniform(-5.0, 5.0), rng.uniform(-5.0, 5.0), z]),
            length: F(rng.uniform(0.5, 20.0)),
            width: F(rng.uniform(0.5, 20.0)),
        }),
        1 => {
            let n = 3 + rng.below(5);
            let r = rng.uniform(1.0, 10.0);
            let pts = ngon(rng, 0.0, 0.0, r, z, n);
            curves.extend(polygon(&pts));
        }
        2 => curves.push(if rng.chance(0.5) {
            Crv::AnalyticCircle {
                center: p3([0.0, 0.0, z]),
                radius: F(rng.uniform(0.5, 10.0)),
            }
        } else {
            Crv::Circle {
                center: p3([0.0, 0.0, z]),
                radius: F(rng.uniform(0.5, 10.0)),
            }
        }),
        _ => {
            // a polygon with a hole
            let r = rng.uniform(4.0, 10.0);
            let n = 3 + rng.below(4);
            let pts = ngon(rng, 0.0, 0.0, r, z, n);
            curves.extend(polygon(&pts));
            curves.push(Crv::Circle {
                center: p3([0.0, 0.0, z]),
                radius: F(r * rng.uniform(0.1, 0.4)),
            });
        }
    }
    if rng.chance(0.1) {
        // an open loop
        curves.pop();
    }
    if rng.chance(0.05) {
        // off the plane
        curves.push(Crv::Segment {
            start: p3([0.0, 0.0, z + 1.0]),
            end: p3([1.0, 0.0, z + 2.0]),
        });
    }
    In::PlanarFaces { curves }
}

/// A projection curve crossing `[ox, ox + sx] x [oy, oy + sy]` above height `h`: any
/// `SmBSplineCurve` class (B-spline, circle, arc, line segment, analytic circle).
fn gen_projection_curve(rng: &mut Rng, ox: f64, oy: f64, sx: f64, sy: f64, h: f64) -> Crv {
    let at = |x: f64, y: f64| p3([ox + x, oy + y, h]);
    match rng.below(5) {
        0 => {
            let y0 = rng.uniform(0.2, 0.8) * sy;
            let n = 2 + rng.below(4) as usize;
            Crv::Curve {
                points: (0..n)
                    .map(|i| {
                        let t = i as f64 / (n - 1) as f64;
                        at(-0.5 * sx + t * 2.0 * sx, y0 + rng.uniform(-0.2, 0.2) * sy)
                    })
                    .collect(),
            }
        }
        1 => Crv::Circle {
            center: at(sx * rng.uniform(0.3, 0.7), sy * rng.uniform(0.3, 0.7)),
            radius: F(sx.min(sy) * rng.uniform(0.1, 0.3)),
        },
        2 => Crv::Arc {
            center: at(sx * rng.uniform(0.3, 0.7), -0.2 * sy),
            radius: F(sy * rng.uniform(0.5, 0.9)),
            start_deg: F(rng.uniform(0.0, 40.0)),
            end_deg: F(rng.uniform(140.0, 180.0)),
        },
        3 => {
            let (y0, y1) = (rng.uniform(0.1, 0.9) * sy, rng.uniform(0.1, 0.9) * sy);
            Crv::Segment {
                start: at(-0.5 * sx, y0),
                end: at(1.5 * sx, y1),
            }
        }
        _ => Crv::AnalyticCircle {
            center: at(sx * rng.uniform(0.3, 0.7), sy * rng.uniform(0.3, 0.7)),
            radius: F(sx.min(sy) * rng.uniform(0.1, 0.3)),
        },
    }
}

fn gen_direction(rng: &mut Rng) -> P3 {
    if rng.chance(0.05) {
        return p3([0.0, 0.0, 0.0]);
    }
    let len = if rng.chance(0.3) { rng.uniform(0.1, 5.0) } else { 1.0 };
    p3([rng.uniform(-0.1, 0.1), rng.uniform(-0.1, 0.1), -len])
}

fn gen_box(rng: &mut Rng) -> Bx {
    Bx {
        origin: p3([rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0)]),
        length: F(rng.uniform(2.0, 15.0)),
        width: F(rng.uniform(2.0, 15.0)),
        height: F(rng.uniform(2.0, 15.0)),
        fillet: rng.chance(0.15).then(|| F(rng.uniform(0.2, 0.8))),
    }
}

fn gen_case(rng: &mut Rng) -> In {
    match rng.below(4) {
        0 => gen_trim3d(rng),
        1 => gen_planar(rng),
        2 => {
            let (object, sx, sy, top, base) = if rng.chance(0.5) {
                let s = rng.uniform(5.0, 30.0);
                (Obj::Surface(gen_corners(rng, s, true)), s, s, 0.0, [0.0, 0.0, 0.0])
            } else {
                let b = gen_box(rng);
                let (o, l, w, h) = (a3(&b.origin), b.length.0, b.width.0, b.height.0);
                (Obj::Box(b), l, w, o[2] + h, o)
            };
            let curve = gen_projection_curve(rng, base[0], base[1], sx, sy, top + 5.0);
            let trim_type = match rng.below(3) {
                0 => Tt::KeepPoint,
                1 => Tt::DeletePoint,
                _ => Tt::Split,
            };
            In::ProjectParallel {
                object,
                curve,
                direction: gen_direction(rng),
                reference: p3([base[0] + 0.5 * sx, base[1] + 0.05 * sy, top]),
                trim_type,
                again: None,
            }
        }
        _ => {
            let b = gen_box(rng);
            let (o, l, w, h) = (a3(&b.origin), b.length.0, b.width.0, b.height.0);
            let curve = gen_projection_curve(rng, o[0], o[1], l, w, o[2] + h + 5.0);
            In::ProjectAndTrim {
                brep: b,
                curve,
                direction: gen_direction(rng),
                reference: p3([o[0] + 0.5 * l, o[1] + 0.05 * w, o[2] + h]),
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.api_trim_ops"
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
