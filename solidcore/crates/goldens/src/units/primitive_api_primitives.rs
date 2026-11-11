//! Runner for unit `primitive.api_primitives`: the `SM_API` primitive constructors.
//!
//! Ported from upstream `source/SM_API/src/SmApiPrimitives.cpp`; checked against its
//! golden. Rust: `solidcore::primitive::api_primitives`.
//!
//! A case is one API call: `op`, its `args` in the API's order (floats may be `"NaN"`,
//! `"inf"`, `"-inf"`) and, for `swung`, the `xy` and `xz` arcs. The record is the status (`0` = `SM_SUCCESS`, else
//! the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce), then on success `has` and the surface or B-rep record, reported as the values
//! `rec`.
//!
//! Surface overloads compare their parameterizations. Successful B-reps additionally
//! compare reachable vertices, sampled curves/surfaces, tolerances and UV trim counts.
//! `blend_primitive` builds a box, calls `SmApiCreateBlendPrimitive` on the box's
//! edge/face list indices, and compares the API status, the mutated B-rep's record and
//! the blend face's own surface record. `offset_profile` builds its `profile`
//! curves (line segments, arcs, circles, B-spline lines and circle segments) and
//! compares `SmApiCreateOffsetProfile`'s B-rep record; its generated cases run on their
//! own seed stream (`offset0000`, ...: [`MIN_GENERATED_OFFSET`]), so each `gen` case stays
//! the one the baseline has.

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::assert_array::labels::FACE_TYPE;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurve, NurbCircleParam};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_curves;
use solidcore::primitive::api_primitives as api;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::SurfaceKind;
use solidcore::surface::torus::Torus;
use solidcore::topology::brep::p1;
use solidcore::topology::{Brep, EdgeId, FaceId};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Most profile curves of one `OffsetProfile` call.
const MAX_PROFILE_CURVES: usize = 16;

/// Largest `lOffsetType` passed on (1 left, 2 right, 3 both; others are refused by
/// `CreateOffsetsOfManyCurves` with `SM_ERR`).
const MAX_OFFSET_TYPE: u64 = 4;

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;
/// Generated `offset_profile` cases per run (their own seed stream).
pub const MIN_GENERATED_OFFSET: usize = 160;

/// Samples per STEP direction of the surface evaluation grid (as the shim).
const GRID: usize = 5;

/// `primitive.api_primitives` runner.
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

/// The entry point of a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
enum OpName {
    PlaneSurface,
    CylinderSurface,
    ConeSurface,
    TorusSurface,
    PlaneBrep,
    PlanarCircle,
    Box,
    Sphere,
    Cylinder,
    Cone,
    Torus,
    Pyramid,
    CylindricalBox,
    PartialSphere,
    SphereNoPole,
    PartialCone,
    ConeEllipticEnds,
    PartialCylinder,
    PartialTorus,
    Swung,
    BlendPrimitive,
    OffsetProfile,
}

impl OpName {
    /// The number of `f64` arguments of the call (for `Swung`: the scale only).
    const fn arity(self) -> usize {
        match self {
            Self::CylinderSurface
            | Self::ConeSurface
            | Self::TorusSurface
            | Self::Box
            | Self::Cone
            | Self::PartialSphere => 6,
            Self::PlaneSurface | Self::PlaneBrep | Self::Cylinder | Self::Torus | Self::Pyramid => {
                5
            }
            Self::PlanarCircle | Self::Sphere | Self::SphereNoPole | Self::OffsetProfile => 4,
            Self::CylindricalBox | Self::PartialCone | Self::ConeEllipticEnds => 8,
            Self::PartialCylinder | Self::PartialTorus => 7,
            Self::Swung => 1,
            Self::BlendPrimitive => 11,
        }
    }
}

/// An `SmApiCreateArc` curve.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct ArcIn {
    center: [f64; 3],
    radius: f64,
    start_deg: f64,
    end_deg: f64,
}

/// A profile curve of `offset_profile`.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    /// `SmApiCreateLineSegment`.
    Line { start: [f64; 3], end: [f64; 3] },
    /// `SmApiCreateArc`.
    Arc {
        center: [f64; 3],
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
    /// `SmApiCreateCircle` (`SmCircle*&`).
    Circle { center: [f64; 3], radius: f64 },
    /// `SmBSplineCurve::CreateLineSegment(ctx, 3, ..)`.
    BsplineLine { start: [f64; 3], end: [f64; 3] },
    /// `SmBSplineCurve::CreateCircleSegment(ctx, 3, frame, .., SM_CO_QUADRATIC)`.
    BsplineCircle {
        origin: [f64; 3],
        x_axis: [f64; 3],
        y_axis: [f64; 3],
        radius: f64,
        start_deg: f64,
        end_deg: f64,
    },
}

impl CurveIn {
    /// The curve the shim builds (a failing constructor is a harness error, as the
    /// shim's `SC_ERR_ARG`).
    fn build(self, k: &Kernel) -> Result<SmCurve, HarnessError> {
        let p = |v: [f64; 3]| Vec3::new(v[0], v[1], v[2]);
        let made = match self {
            Self::Line { start, end } => {
                api_curves::create_line_segment(p(start), p(end)).map(SmCurve::from)
            }
            Self::Arc {
                center,
                radius,
                start_deg,
                end_deg,
            } => api_curves::create_arc(p(center), radius, start_deg, end_deg).map(SmCurve::from),
            Self::Circle { center, radius } => {
                api_curves::create_analytic_circle(k, p(center), radius).map(SmCurve::from)
            }
            Self::BsplineLine { start, end } => {
                BSplineCurve::create_line_segment(3, p(start), p(end)).map(SmCurve::from)
            }
            Self::BsplineCircle {
                origin,
                x_axis,
                y_axis,
                radius,
                start_deg,
                end_deg,
            } => {
                let mut frame = Axis2Placement::default();
                frame
                    .set_canonical(p(origin), p(x_axis), p(y_axis))
                    .map_err(|e| HarnessError::new(format!("profile frame: {e}")))?;
                BSplineCurve::create_circle_segment(
                    3,
                    &frame,
                    radius,
                    start_deg,
                    end_deg,
                    NurbCircleParam::Quadratic,
                )
                .map(SmCurve::from)
            }
        };
        made.map_err(|e| HarnessError::new(format!("profile curve: {e}")))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    op: OpName,
    args: Vec<F>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    xy: Vec<ArcIn>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    xz: Vec<ArcIn>,
    /// The curves of `offset_profile`.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    profile: Vec<CurveIn>,
}

impl Inputs {
    fn arg(&self, i: usize) -> Result<f64, HarnessError> {
        self.args
            .get(i)
            .map(|f| f.0)
            .ok_or_else(|| HarnessError::new(format!("{:?}: missing argument {i}", self.op)))
    }

    fn point(&self, i: usize) -> Result<Point3, HarnessError> {
        Ok(Vec3::new(self.arg(i)?, self.arg(i + 1)?, self.arg(i + 2)?))
    }
}

// ---- Rust ------------------------------------------------------------------------------

/// The upstream status of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => 1007.0,
        e if solidcore::error::nurbs::upstream_status(e).is_some() => {
            f64::from(solidcore::error::nurbs::upstream_status(e).unwrap_or(-1))
        }
        _ => -1.0,
    }
}

fn b(x: bool) -> f64 {
    if x {
        1.0
    } else {
        0.0
    }
}

fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

fn put_dom(o: &mut Vec<f64>, d: &Extent2d) {
    o.extend([d.min().x, d.min().y, d.max().x, d.max().y]);
}

fn put_pos(o: &mut Vec<f64>, p: &Axis2Placement) {
    put_vec(o, p.origin());
    put_vec(o, p.x_axis());
    put_vec(o, p.y_axis());
    put_vec(o, p.z_axis());
}

fn put_surface_nurb(o: &mut Vec<f64>, s: Option<&NurbSurface>) {
    let Some(sur) = s else {
        o.push(0.0);
        return;
    };
    o.extend([
        1.0,
        sur.n() as f64,
        sur.m() as f64,
        sur.p() as f64,
        sur.q() as f64,
        sur.r() as f64,
        sur.s() as f64,
    ]);
    for row in sur.rows() {
        for c in row {
            o.extend([c.x, c.y, c.z, c.w]);
        }
    }
    o.extend_from_slice(sur.u_knots());
    o.extend_from_slice(sur.v_knots());
}

/// The grid parameter `i` of `GRID` over `[lo, hi]` (as the shim).
fn grid_at(lo: f64, hi: f64, i: usize) -> f64 {
    lo + (hi - lo) * (i as f64 / (GRID - 1) as f64)
}

/// The evaluation grid over the STEP domain `d` (only when finite).
fn put_grid(o: &mut Vec<f64>, d: &Extent2d, eval: impl Fn(Point2) -> solidcore::Result<Point3>) {
    let fin = d.min().x.is_finite()
        && d.min().y.is_finite()
        && d.max().x.is_finite()
        && d.max().y.is_finite();
    o.push(b(fin));
    if !fin {
        return;
    }
    for i in 0..GRID {
        for j in 0..GRID {
            let uv = Point2::new(
                grid_at(d.min().x, d.max().x, i),
                grid_at(d.min().y, d.max().y, j),
            );
            match eval(uv) {
                Ok(p) => {
                    o.push(0.0);
                    put_vec(o, p);
                }
                Err(e) => o.push(status(&e)),
            }
        }
    }
}

fn put_plane(o: &mut Vec<f64>, a: &Plane) {
    o.push(a.type_code() as f64);
    put_pos(o, a.position());
    let d = a.step_uv_domain();
    put_dom(o, &d);
    let sc = a.uv_scale();
    o.extend([sc.x, sc.y]);
    match a.base_with_nurb() {
        Ok(w) => put_surface_nurb(o, w.gw_nurb()),
        Err(_) => o.push(0.0),
    }
    put_grid(o, &d, |uv| Ok(a.evaluate_point(uv)));
}

fn put_cone(o: &mut Vec<f64>, c: &Cone) {
    o.push(c.type_code() as f64);
    put_pos(o, c.rev().position());
    let d = c.rev().step_uv_domain();
    put_dom(o, &d);
    o.extend([
        c.bot_radius(),
        c.top_radius(),
        b(c.inside_out()),
        b(c.rev().swap_uv()),
    ]);
    put_surface_nurb(o, c.rev().base().gw_nurb());
    put_grid(o, &d, |uv| c.evaluate_step_point(uv));
}

fn put_torus(o: &mut Vec<f64>, t: &Torus) {
    o.push(t.type_code() as f64);
    put_pos(o, t.base().position());
    let d = t.base().step_uv_domain();
    put_dom(o, &d);
    o.extend([
        t.major_radius(),
        t.minor_radius(),
        b(t.inside_out()),
        b(t.base().swap_uv()),
    ]);
    put_surface_nurb(o, t.base().base().gw_nurb());
    put_grid(o, &d, |uv| t.evaluate_step_point(uv));
}

/// The face, edge and vertex counts of the B-rep's lists (`GetNumFaces`, `GetNumEdges`,
/// `GetNumVertices`; arena records an edit unlinked are not in them).
fn put_brep(o: &mut Vec<f64>, brep: &Brep) {
    let n = |r: solidcore::Result<usize>| r.map_or(-1.0, |n| n as f64);
    o.extend([
        n(p1::faces(brep, None).map(|v| v.len())),
        n(p1::edges(brep, None).map(|v| v.len())),
        n(p1::vertices(brep, None).map(|v| v.len())),
    ]);
}

/// The record of a result, or the deferral of an unported call.
fn finish<T>(r: solidcore::Result<T>, put: impl Fn(&mut Vec<f64>, &T)) -> Output {
    match r {
        Ok(v) => {
            let mut o = vec![0.0, 1.0];
            put(&mut o, &v);
            Output::new().values("rec", o)
        }
        Err(e) => {
            Output::unported(&e).unwrap_or_else(|| Output::new().values("rec", vec![status(&e)]))
        }
    }
}

fn brep_out(r: solidcore::Result<Brep>) -> Output {
    brep_record(r, false)
}

fn brep_record(r: solidcore::Result<Brep>, rays: bool) -> Output {
    finish(
        r.and_then(|mut brep| {
            let mut record = Vec::new();
            put_brep(&mut record, &brep);
            let mut k = Kernel::default();
            crate::util::primitive::append(&k, &brep, &mut record)?;
            if rays {
                crate::util::primitive::append_pyramid_rays(&mut k, &mut brep, &mut record)?;
            }
            Ok(record)
        }),
        |out, record| out.extend(record),
    )
}

/// [`brep_out`] for the pyramid, with its rays. (A face whose plane fit hits
/// `FindOuterLoops`' zero-area loop, UB-119, is rejected by [`cpp`] before either side
/// runs; the port's `InvalidInput` there would report `1007`.)
fn pyramid_out(r: solidcore::Result<Brep>) -> Output {
    brep_record(r, true)
}

/// The status record of an error, then `trailing` (as much of the output as the C++
/// shim has already written). An unported call defers the case instead.
fn error_record(e: &SolidError, trailing: &[f64]) -> Output {
    Output::unported(e).unwrap_or_else(|| {
        let mut record = vec![status(e)];
        record.extend_from_slice(trailing);
        Output::new().values("rec", record)
    })
}

/// The blend face's record, mirroring the shim's `put_face_detail`: its kind,
/// tolerance, UV domain, loop count, then its surface (kind, NURB for a B-spline and a
/// 5 x 5 evaluation grid over the face's UV domain).
fn put_face_detail(o: &mut Vec<f64>, k: &Kernel, face: &solidcore::topology::Face) {
    o.push(FACE_TYPE as f64);
    o.push(face.tolerance);
    put_dom(o, &face.uv_domain);
    o.push(face.loops.len() as f64);
    let Some(surface) = face.surface.as_ref() else {
        o.push(0.0);
        return;
    };
    o.push(1.0);
    o.push(surface.type_code() as f64);
    let nurb = match surface.kind() {
        SurfaceKind::BSpline(s) => s.gw_nurb(),
        _ => None,
    };
    put_surface_nurb(o, nurb);
    put_grid(o, &face.uv_domain, |uv| surface.evaluate_point(k, uv));
}

/// A case's list index `n`, checked against the box's list (`ids`).
fn pick<T: Copy>(i: &Inputs, n: usize, ids: &[T], what: &str) -> Result<T, HarnessError> {
    let value = i.arg(n)?;
    if value < 0.0 || !value.is_finite() || value.fract() != 0.0 {
        return Err(HarnessError::new(format!("{what}: bad index")));
    }
    ids.get(value as usize)
        .copied()
        .ok_or_else(|| HarnessError::new(format!("{what}: index out of range")))
}

/// A `blend_primitive` case: build the box, then call `SmApiCreateBlendPrimitive` on the
/// edge/face indices of its `GetEdges`/`GetFaces` lists. On success the record is the
/// out face, the mutated B-rep (counts, geometry/trim record) and the blend face's own
/// record; a failing box reports only its status, a failing API call its status and
/// `has = 0`, as the shim writes them.
fn blend_out(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let mut brep =
        match api::create_box(&mut k, i.point(0)?, i.arg(3)?, i.arg(4)?, i.arg(5)?) {
            Ok(brep) => brep,
            Err(e) => return Ok(error_record(&e, &[])),
        };
    let edges = p1::edges(&brep, None)
        .map_err(|e| HarnessError::new(format!("blend_primitive edges: {e}")))?;
    let faces = p1::faces(&brep, None)
        .map_err(|e| HarnessError::new(format!("blend_primitive faces: {e}")))?;
    let edge1: EdgeId = pick(i, 6, &edges, "edge1")?;
    let face1: FaceId = pick(i, 7, &faces, "face1")?;
    let edge2: EdgeId = pick(i, 8, &edges, "edge2")?;
    let face2: FaceId = pick(i, 9, &faces, "face2")?;
    let same = match i.arg(10)? {
        0.0 => false,
        1.0 => true,
        _ => return Err(HarnessError::new("blend_primitive: direction flag")),
    };
    let blend = match api::create_blend_primitive(
        &mut k, &mut brep, edge1, face1, edge2, face2, same,
    ) {
        Ok(face) => face,
        Err(e) => return Ok(error_record(&e, &[0.0])),
    };
    let built: solidcore::Result<Vec<f64>> = (|| {
        let mut o = vec![if blend.is_some() { 1.0 } else { 0.0 }];
        o.extend([
            p1::faces(&brep, None)?.len() as f64,
            p1::edges(&brep, None)?.len() as f64,
            p1::vertices(&brep, None)?.len() as f64,
        ]);
        crate::util::primitive::append(&k, &brep, &mut o)?;
        if let Some(blend) = blend {
            put_face_detail(&mut o, &k, brep.face(blend)?);
        }
        Ok(o)
    })();
    Ok(match built {
        Ok(mut record) => {
            record.insert(0, 0.0);
            Output::new().values("rec", record)
        }
        Err(e) => error_record(&e, &[]),
    })
}

fn arcs(xs: &[ArcIn]) -> Result<Vec<SmCurve>, HarnessError> {
    xs.iter()
        .map(|a| {
            api_curves::create_arc(
                Vec3::new(a.center[0], a.center[1], a.center[2]),
                a.radius,
                a.start_deg,
                a.end_deg,
            )
            .map(SmCurve::from)
            .map_err(|e| HarnessError::new(format!("arc: {e}")))
        })
        .collect()
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut k = Kernel::default();
    let k = &mut k;
    let a = |n: usize| i.arg(n);
    if i.op != OpName::Swung && i.args.len() != i.op.arity() {
        return Err(HarnessError::new(format!(
            "{:?}: wrong argument count",
            i.op
        )));
    }
    Ok(match i.op {
        OpName::PlaneSurface => finish(
            api::create_plane_surface(i.point(0)?, a(3)?, a(4)?),
            put_plane,
        ),
        OpName::CylinderSurface => finish(
            api::create_cylinder_surface(k, i.point(0)?, a(3)?, a(4)?),
            put_cone,
        ),
        OpName::ConeSurface => finish(
            api::create_cone_surface(k, i.point(0)?, a(3)?, a(4)?, a(5)?),
            put_cone,
        ),
        OpName::TorusSurface => finish(
            api::create_torus_surface(k, i.point(0)?, a(3)?, a(4)?),
            put_torus,
        ),
        OpName::PlaneBrep => brep_out(api::create_plane(k, i.point(0)?, a(3)?, a(4)?)),
        OpName::PlanarCircle => brep_out(api::create_planar_circle(k, i.point(0)?, a(3)?)),
        OpName::Box => brep_out(api::create_box(k, i.point(0)?, a(3)?, a(4)?, a(5)?)),
        OpName::Sphere => brep_out(api::create_sphere(k, i.point(0)?, a(3)?)),
        OpName::Cylinder => brep_out(api::create_cylinder(k, i.point(0)?, a(3)?, a(4)?)),
        OpName::Cone => brep_out(api::create_cone(k, i.point(0)?, a(3)?, a(4)?, a(5)?)),
        OpName::Torus => brep_out(api::create_torus(k, i.point(0)?, a(3)?, a(4)?)),
        OpName::Pyramid => pyramid_out(api::create_pyramid(
            &mut Kernel::default(),
            i.point(0)?,
            a(3)?,
            a(4)?,
        )),
        OpName::CylindricalBox => brep_out(api::create_cylindrical_box(k, 
            i.point(0)?,
            a(3)?,
            a(4)?,
            a(5)?,
            a(6)?,
            a(7)?,
        )),
        OpName::PartialSphere => {
            brep_out(api::create_partial_sphere(k, i.point(0)?, a(3)?, a(4)?, a(5)?))
        }
        OpName::SphereNoPole => brep_out(api::create_sphere_no_pole(k, i.point(0)?, a(3)?)),
        OpName::PartialCone => brep_out(api::create_partial_cone(k, 
            i.point(0)?,
            a(3)?,
            a(4)?,
            a(5)?,
            a(6)?,
            a(7)?,
        )),
        OpName::ConeEllipticEnds => {
            let caps = a(7)?;
            if caps != 0.0 && caps != 1.0 {
                return Err(HarnessError::new("caps"));
            }
            brep_out(api::create_cone_elliptic_ends(k, &api::ConeEllipticEnds {
                front_radius: a(0)?,
                rear_radius: a(1)?,
                height: a(2)?,
                front_y_tilt_deg: a(3)?,
                rear_y_tilt_deg: a(4)?,
                front_x_tilt_deg: a(5)?,
                rear_x_tilt_deg: a(6)?,
                end_caps: caps != 0.0,
            }))
        }
        OpName::PartialCylinder => brep_out(api::create_partial_cylinder(k, 
            i.point(0)?,
            a(3)?,
            a(4)?,
            a(5)?,
            a(6)?,
        )),
        OpName::PartialTorus => brep_out(api::create_partial_torus(k, 
            i.point(0)?,
            a(3)?,
            a(4)?,
            a(5)?,
            a(6)?,
        )),
        OpName::Swung => {
            let xy = arcs(&i.xy)?;
            let xz = arcs(&i.xz)?;
            brep_out(api::create_swung_primitive(k, &xy, &xz, a(0)?))
        }
        OpName::BlendPrimitive => blend_out(i)?,
        OpName::OffsetProfile => {
            if i.profile.len() > MAX_PROFILE_CURVES {
                return Err(HarnessError::new("offset profile: too many curves"));
            }
            let curves = i
                .profile
                .iter()
                .map(|c| c.build(k))
                .collect::<Result<Vec<_>, _>>()?;
            let refs: Vec<Option<&SmCurve>> = curves.iter().map(Some).collect();
            let flag = |n: usize| -> Result<bool, HarnessError> {
                let v = a(n)?;
                if v != 0.0 && v != 1.0 {
                    return Err(HarnessError::new("offset profile: flag"));
                }
                Ok(v == 1.0)
            };
            let kind = a(1)?;
            if !(0.0..=MAX_OFFSET_TYPE as f64).contains(&kind) || kind.fract() != 0.0 {
                return Err(HarnessError::new("offset profile: offset type"));
            }
            brep_out(api::create_offset_profile(
                k,
                &refs,
                a(0)?,
                kind as u64,
                flag(2)?,
                flag(3)?,
            ))
        }
    })
}

// ---- generation -------------------------------------------------------------------

/// A "nasty" float: NaN, infinities, zero, negatives, tiny or huge values, and the
/// `SM_EFF_ZERO` and `SM_BIG_DOUBLE / 100` boundaries.
fn nasty(rng: &mut Rng) -> f64 {
    let choices = [
        f64::NAN,
        f64::INFINITY,
        f64::NEG_INFINITY,
        0.0,
        -0.0,
        -1.0,
        1.0e-12,
        1.0000001e-12,
        2.0e-12,
        -1.0e-12,
        1.0e-7,
        1.0e18,
        1.0000001e18,
        f64::MAX,
        5e-324,
    ];
    rng.choose(&choices).copied().unwrap_or(0.0)
}

/// A dimension: mostly moderate, else tiny, huge, negative or nasty.
fn dim(rng: &mut Rng) -> f64 {
    match rng.below(20) {
        0 | 1 => nasty(rng),
        2 => rng.uniform(1.0e-11, 1.0e-6),
        3 => rng.uniform(1.0e4, 1.0e9),
        4 => -rng.uniform(0.0, 10.0),
        _ => rng.uniform(0.05, 50.0),
    }
}

/// An origin: mostly moderate, sometimes far away or tiny, rarely nasty.
fn origin(rng: &mut Rng) -> [f64; 3] {
    match rng.below(20) {
        0 => [0.0, 0.0, 0.0],
        1 => rng.point3(-1.0e9, 1.0e9),
        2 => rng.point3(-1.0e-9, 1.0e-9),
        3 => {
            let mut p = rng.point3(-10.0, 10.0);
            let i = rng.below(3) as usize;
            if let Some(c) = p.get_mut(i) {
                *c = nasty(rng);
            }
            p
        }
        _ => rng.point3(-100.0, 100.0),
    }
}

/// An angle: mostly in `[-400, 400]`, sometimes at the validation boundaries.
fn angle(rng: &mut Rng) -> f64 {
    match rng.below(12) {
        0 => *rng
            .choose(&[-360.0, 360.0, 0.0, 1.0e-12, -360.0000001, 360.0000001, 90.0])
            .unwrap_or(&0.0),
        1 => nasty(rng),
        _ => rng.uniform(-400.0, 400.0),
    }
}

fn mk(op: OpName, args: Vec<f64>) -> Inputs {
    Inputs {
        op,
        args: args.into_iter().map(F).collect(),
        xy: Vec::new(),
        xz: Vec::new(),
        profile: Vec::new(),
    }
}

fn with_origin(rng: &mut Rng, rest: &[f64]) -> Vec<f64> {
    let mut v = origin(rng).to_vec();
    v.extend_from_slice(rest);
    v
}

/// A sane origin and positive dimension for the entry points without validation
/// (their kernel calls are deferred on the Rust side; extreme input there only
/// exercises the C++).
fn sane(rng: &mut Rng) -> (Vec<f64>, f64) {
    (rng.point3(-50.0, 50.0).to_vec(), rng.uniform(0.5, 20.0))
}

/// A `blend_primitive` case: a box and two boundary edge/face associations, given as
/// indices into that box's `GetEdges`/`GetFaces` lists. A corner case picks two edges of
/// one face; a cross case uses each edge's own face; the mixed case sometimes names a
/// face the edge does not bound, which `SmApiCreatePrimitive` rejects with
/// `SM_ERR_INVALID_INPUT` on both sides.
fn blend_case(rng: &mut Rng) -> Inputs {
    // The edges bounding each face, and each edge's two faces, of a box built at the
    // origin by `SmApiCreateBox` (its list order, as the B-rep record reports it).
    const ON_FACE: [[usize; 4]; 6] = [
        [2, 4, 7, 11],
        [0, 1, 3, 9],
        [2, 3, 6, 10],
        [0, 4, 5, 8],
        [6, 7, 8, 9],
        [1, 5, 10, 11],
    ];
    const INCIDENCE: [[usize; 2]; 12] = [
        [1, 3],
        [1, 5],
        [2, 0],
        [2, 1],
        [3, 0],
        [3, 5],
        [4, 2],
        [4, 0],
        [4, 3],
        [4, 1],
        [5, 2],
        [5, 0],
    ];
    let on_face = |f: usize, i: usize| {
        ON_FACE
            .get(f)
            .and_then(|r| r.get(i))
            .copied()
            .unwrap_or(0)
    };
    let own_face = |e: usize, i: usize| {
        INCIDENCE
            .get(e)
            .and_then(|r| r.get(i))
            .copied()
            .unwrap_or(0)
    };
    let other_face = |e: usize, k: usize| {
        let mut left = k;
        for f in 0..6 {
            let own = INCIDENCE.get(e).is_some_and(|r| r.contains(&f));
            if !own {
                if left == 0 {
                    return f;
                }
                left -= 1;
            }
        }
        0
    };
    let (e1, f1, e2, f2) = match rng.below(4) {
        0 => {
            let f = rng.below(6) as usize;
            let a = rng.below(4) as usize;
            let mut b = rng.below(3) as usize;
            if b >= a {
                b += 1;
            }
            (on_face(f, a), f, on_face(f, b), f)
        }
        1 => {
            let e1 = rng.below(12) as usize;
            let e2 = rng.below(12) as usize;
            (
                e1,
                own_face(e1, rng.below(2) as usize),
                e2,
                own_face(e2, rng.below(2) as usize),
            )
        }
        _ => {
            let e1 = rng.below(12) as usize;
            let e2 = rng.below(12) as usize;
            let f1 = if rng.chance(0.5) {
                own_face(e1, rng.below(2) as usize)
            } else {
                other_face(e1, rng.below(4) as usize)
            };
            let f2 = if rng.chance(0.5) {
                own_face(e2, rng.below(2) as usize)
            } else {
                other_face(e2, rng.below(4) as usize)
            };
            (e1, f1, e2, f2)
        }
    };
    let (l, w, h) = match rng.below(10) {
        0 => (1.0e-3, 1.0e-3, 1.0e-3),
        1 => (1.0e3, 1.0e3, 1.0e3),
        _ => (
            rng.uniform(0.05, 50.0),
            rng.uniform(0.05, 50.0),
            rng.uniform(0.05, 50.0),
        ),
    };
    let o = match rng.below(8) {
        0 => [0.0, 0.0, 0.0],
        1 => rng.point3(-1.0e3, 1.0e3),
        _ => rng.point3(-10.0, 10.0),
    };
    let mut v = o.to_vec();
    v.extend([
        l,
        w,
        h,
        e1 as f64,
        f1 as f64,
        e2 as f64,
        f2 as f64,
        if rng.chance(0.5) { 1.0 } else { 0.0 },
    ]);
    mk(OpName::BlendPrimitive, v)
}

fn gen_case(rng: &mut Rng) -> Inputs {
    match rng.below(40) {
        0..=5 => {
            let (l, w) = (dim(rng), dim(rng));
            mk(OpName::PlaneSurface, with_origin(rng, &[l, w]))
        }
        6..=11 => {
            let (r, h) = (dim(rng), dim(rng));
            mk(OpName::CylinderSurface, with_origin(rng, &[r, h, 0.0]))
        }
        12..=17 => {
            let rb = if rng.chance(0.15) { 0.0 } else { dim(rng) };
            let rt = if rng.chance(0.15) { 0.0 } else { dim(rng) };
            let h = dim(rng);
            mk(OpName::ConeSurface, with_origin(rng, &[rb, rt, h]))
        }
        18..=23 => {
            let (ra, ri) = (dim(rng), dim(rng));
            mk(OpName::TorusSurface, with_origin(rng, &[ra, ri, 0.0]))
        }
        24 | 25 => {
            let (l, w, h) = (dim(rng), dim(rng), dim(rng));
            mk(OpName::Box, with_origin(rng, &[l, w, h]))
        }
        26 => {
            let r = if rng.chance(0.5) { -dim(rng) } else { dim(rng) };
            mk(OpName::Sphere, with_origin(rng, &[r]))
        }
        27 => {
            let r = if rng.chance(0.5) { -dim(rng) } else { dim(rng) };
            mk(OpName::SphereNoPole, with_origin(rng, &[r]))
        }
        28 => {
            let (r, h) = (dim(rng), dim(rng));
            mk(OpName::Cylinder, with_origin(rng, &[r, h]))
        }
        29 => {
            let rb = if rng.chance(0.2) { 0.0 } else { dim(rng) };
            let rt = if rng.chance(0.2) { 0.0 } else { dim(rng) };
            let h = dim(rng);
            mk(OpName::Cone, with_origin(rng, &[rb, rt, h]))
        }
        30 | 31 => {
            let len = dim(rng);
            let ri = dim(rng);
            let ro = match rng.below(4) {
                0 => ri + 1.0e-12,
                1 => ri,
                _ => dim(rng) + if rng.chance(0.5) { ri } else { 0.0 },
            };
            let a0 = angle(rng);
            let a1 = match rng.below(4) {
                0 => a0 + 360.0,
                1 => a0 + 1.0e-12,
                _ => angle(rng),
            };
            mk(
                OpName::CylindricalBox,
                with_origin(rng, &[len, ri, ro, a0, a1]),
            )
        }
        32 | 33 => {
            // Pyramid: the base edges are made (and compared) before the deferral.
            let (l, h) = match rng.below(6) {
                0 => (nasty(rng), dim(rng)),
                1 => (dim(rng), nasty(rng)),
                2 => (rng.uniform(1.0e-12, 1.0e-8), dim(rng)),
                _ => (dim(rng), dim(rng)),
            };
            let o = if rng.chance(0.2) {
                rng.point3(1.0e8, 1.0e10).to_vec()
            } else {
                rng.point3(-50.0, 50.0).to_vec()
            };
            let mut v = o;
            v.extend([l, h]);
            mk(OpName::Pyramid, v)
        }
        34 => {
            let (mut o, r) = sane(rng);
            let a0 = rng.uniform(-90.0, 90.0);
            o.extend([r, a0, a0 + rng.uniform(10.0, 270.0)]);
            mk(OpName::PartialSphere, o)
        }
        35 => {
            let (mut o, r) = sane(rng);
            let a0 = rng.uniform(-90.0, 90.0);
            o.extend([
                r,
                rng.uniform(0.5, 20.0),
                rng.uniform(0.5, 20.0),
                a0,
                a0 + rng.uniform(10.0, 270.0),
            ]);
            mk(OpName::PartialCone, o)
        }
        36 => {
            let (mut o, r) = sane(rng);
            let a0 = rng.uniform(-90.0, 90.0);
            o.extend([r, rng.uniform(0.5, 20.0), a0, a0 + rng.uniform(10.0, 270.0)]);
            mk(OpName::PartialCylinder, o)
        }
        37 => {
            let (mut o, r) = sane(rng);
            let a0 = rng.uniform(-90.0, 90.0);
            o.extend([
                r + 5.0,
                rng.uniform(0.2, 4.0),
                a0,
                a0 + rng.uniform(10.0, 270.0),
            ]);
            mk(OpName::PartialTorus, o)
        }
        38 => {
            let (mut o, r) = sane(rng);
            match rng.below(3) {
                0 => {
                    o.push(r);
                    mk(OpName::PlanarCircle, o)
                }
                1 => {
                    o.extend([r, rng.uniform(0.5, 20.0)]);
                    mk(OpName::PlaneBrep, o)
                }
                _ => {
                    o.extend([r + 5.0, rng.uniform(0.2, 4.0)]);
                    mk(OpName::Torus, o)
                }
            }
        }
        _ => match rng.below(3) {
            0 => mk(
                OpName::ConeEllipticEnds,
                vec![
                    rng.uniform(1.0, 10.0),
                    rng.uniform(1.0, 10.0),
                    rng.uniform(1.0, 20.0),
                    rng.uniform(-30.0, 30.0),
                    rng.uniform(-30.0, 30.0),
                    rng.uniform(-30.0, 30.0),
                    rng.uniform(-30.0, 30.0),
                    b(rng.chance(0.5)),
                ],
            ),
            1 => {
                let arc = |rng: &mut Rng| ArcIn {
                    center: [0.0, 0.0, 0.0],
                    radius: rng.uniform(1.0, 10.0),
                    start_deg: 0.0,
                    end_deg: 180.0,
                };
                Inputs {
                    op: OpName::Swung,
                    args: vec![F(rng.uniform(0.5, 2.0))],
                    xy: vec![arc(rng)],
                    xz: vec![arc(rng)],
                    profile: Vec::new(),
                }
            }
            _ => blend_case(rng),
        },
    }
}

/// A closed planar profile at scale `s` around `c` in the XY plane: a rectangle of line
/// segments (either kind, either orientation, its sides in any order), a circle (either
/// kind), a slot of two lines and two arcs, or a rectangle with a circular hole.
fn gen_profile(rng: &mut Rng, s: f64, c: [f64; 3]) -> Vec<CurveIn> {
    let (w, h) = (s * rng.uniform(0.5, 2.0), s * rng.uniform(0.5, 2.0));
    let [x0, y0, z] = c;
    let bspline = rng.chance(0.4);
    let segment = |start: [f64; 3], end: [f64; 3]| {
        if bspline {
            CurveIn::BsplineLine { start, end }
        } else {
            CurveIn::Line { start, end }
        }
    };
    let rectangle = |rng: &mut Rng| {
        let corners = [
            [x0, y0, z],
            [x0 + w, y0, z],
            [x0 + w, y0 + h, z],
            [x0, y0 + h, z],
        ];
        let reverse = rng.chance(0.3);
        let mut sides: Vec<CurveIn> = (0..4)
            .map(|i| {
                let a = corners.get(i).copied().unwrap_or(c);
                let b = corners.get((i + 1) % 4).copied().unwrap_or(c);
                if reverse {
                    segment(b, a)
                } else {
                    segment(a, b)
                }
            })
            .collect();
        if rng.chance(0.3) {
            sides.rotate_left(1 + rng.below(3) as usize);
        }
        sides
    };
    let circle = |rng: &mut Rng, center: [f64; 3], radius: f64| {
        if rng.chance(0.5) {
            CurveIn::Circle { center, radius }
        } else {
            CurveIn::BsplineCircle {
                origin: center,
                x_axis: [1.0, 0.0, 0.0],
                y_axis: if rng.chance(0.5) {
                    [0.0, 1.0, 0.0]
                } else {
                    [0.0, -1.0, 0.0]
                },
                radius,
                start_deg: 0.0,
                end_deg: 360.0,
            }
        }
    };
    match rng.below(5) {
        0 | 1 => rectangle(rng),
        2 => vec![circle(rng, c, 0.5 * w)],
        3 => {
            // a slot: two parallel lines joined by half circles
            let r = 0.5 * h;
            vec![
                segment([x0, y0, z], [x0 + w, y0, z]),
                CurveIn::Arc {
                    center: [x0 + w, y0 + r, z],
                    radius: r,
                    start_deg: -90.0,
                    end_deg: 90.0,
                },
                segment([x0 + w, y0 + h, z], [x0, y0 + h, z]),
                CurveIn::Arc {
                    center: [x0, y0 + r, z],
                    radius: r,
                    start_deg: 90.0,
                    end_deg: 270.0,
                },
            ]
        }
        _ => {
            let mut v = rectangle(rng);
            let r = 0.2 * w.min(h);
            v.push(circle(rng, [x0 + 0.5 * w, y0 + 0.5 * h, z], r));
            v
        }
    }
}

/// An `offset_profile` case: a profile, a distance (sometimes negative, sometimes as large
/// as the profile), an offset type (mostly 1 to 3, sometimes 0 or 4) and the flags.
fn gen_offset_case(rng: &mut Rng) -> Inputs {
    let s = match rng.below(10) {
        0 => rng.uniform(0.01, 0.1),
        1 => rng.uniform(100.0, 1000.0),
        _ => rng.uniform(1.0, 20.0),
    };
    let c = [
        rng.uniform(-3.0 * s, 3.0 * s),
        rng.uniform(-3.0 * s, 3.0 * s),
        if rng.chance(0.7) {
            0.0
        } else {
            rng.uniform(-s, s)
        },
    ];
    let profile = gen_profile(rng, s, c);
    let mut distance = s * rng.uniform(0.02, 0.3);
    if rng.chance(0.2) {
        distance = -distance;
    }
    if rng.chance(0.05) {
        distance = s * rng.uniform(0.5, 2.0);
    }
    let kind = match rng.below(20) {
        0 => 0.0,
        1 => 4.0,
        _ => (1 + rng.below(3)) as f64,
    };
    let round = b(rng.chance(0.5));
    let shell = b(rng.chance(0.5));
    let mut i = mk(OpName::OffsetProfile, vec![distance, kind, round, shell]);
    i.profile = profile;
    i
}

// ---- input contract --------------------------------------------------------------------

/// Largest sphere radius magnitude a generated `sphere` case passes on.
const MAX_SPHERE_RADIUS: f64 = 6.0e9;
/// Smallest radius magnitude a generated `sphere_no_pole` case passes on.
const MIN_NO_POLE_RADIUS: f64 = 3.0e-4;
/// Largest radius magnitude a generated `sphere_no_pole` case passes on.
const MAX_NO_POLE_RADIUS: f64 = 3.0e8;
/// Largest coordinate or dimension magnitude of the entry points without validation,
/// and largest origin coordinate of `cylindrical_box`.
const MAX_KERNEL_COORD: f64 = 1.0e6;
/// Smallest inside-arc chord (`ri * sweep`, sweep in radians) of `cylindrical_box`.
const MIN_ARC_CHORD: f64 = 1.0e-4;
/// Smallest inside-arc sagitta (`ri * sweep^2 / 8`) of `cylindrical_box`.
const MIN_ARC_SAGITTA: f64 = 1.0e-8;
/// Smallest gap (degrees) between a `cylindrical_box` sweep and a full turn.
const MIN_FULL_TURN_GAP: f64 = 1.0e-8;
/// Largest edge/face index of `blend_primitive`.
const MAX_BLEND_ENTITY: usize = 64;
/// `SM_EFF_ZERO`.
const EFF_ZERO: f64 = 1.0e-12;

/// `SM_IS_VALID_DOUBLE`: not NaN, finite and within `SM_BIG_DOUBLE / 100`.
fn valid_double(a: f64) -> bool {
    (-1.0e18..=1.0e18).contains(&a)
}

/// Every value finite and at most [`MAX_KERNEL_COORD`] in magnitude.
fn bounded(xs: &[f64]) -> bool {
    xs.iter()
        .all(|x| x.is_finite() && x.abs() <= MAX_KERNEL_COORD)
}

/// The generator's input contract, ported from the upstream harness guard: past each
/// entry point's own validation (inputs it refuses itself always pass), the inputs on
/// which upstream `SmPrimitiveCreation` crashes or does not return are not generated,
/// so the generated cases exercise the working path (hostile inputs are fixtures).
/// - `plane_surface`: no NaN length or width;
/// - `torus_surface`: with both radii above `SM_EFF_ZERO`, a finite origin, finite
///   radii whose sum does not overflow and a minor radius not within `1e-8`
///   (relative) of `f64::MAX`;
/// - `sphere`: with a valid radius, a finite center and a radius magnitude at most
///   [`MAX_SPHERE_RADIUS`];
/// - `sphere_no_pole`: with a valid radius, a magnitude in
///   [`MIN_NO_POLE_RADIUS`]`..=`[`MAX_NO_POLE_RADIUS`];
/// - `cylindrical_box`: with valid arguments, an origin within [`MAX_KERNEL_COORD`],
///   an inside arc of chord at least [`MIN_ARC_CHORD`] and sagitta at least
///   [`MIN_ARC_SAGITTA`], and a sweep at least [`MIN_FULL_TURN_GAP`] degrees short of
///   a full turn;
/// - the entry points without validation and `swung` (its arcs too): every value
///   [`bounded`];
/// - `blend_primitive`: a [`bounded`] box, entity indices integers in
///   `0..=`[`MAX_BLEND_ENTITY`] and a `0`/`1` direction flag;
/// - `offset_profile`: a [`bounded`] distance, an integer offset type in
///   `0..=`[`MAX_OFFSET_TYPE`] and `0`/`1` flags.
fn accepted(i: &Inputs) -> bool {
    let a = |k: usize| i.args.get(k).map_or(f64::NAN, |f| f.0);
    let args: Vec<f64> = i.args.iter().map(|f| f.0).collect();
    let origin = [a(0), a(1), a(2)];
    match i.op {
        OpName::PlaneSurface => !(a(3).is_nan() || a(4).is_nan()),
        OpName::TorusSurface => {
            let (major, minor) = (a(3), a(4));
            !(major > EFF_ZERO
                && minor > EFF_ZERO
                && (!origin.iter().all(|x| x.is_finite())
                    || major == f64::INFINITY
                    || minor == f64::INFINITY
                    || (major + minor).is_infinite()
                    || minor > f64::MAX * (1.0 - 1.0e-8)))
        }
        OpName::Sphere => {
            let r = a(3);
            let valid = valid_double(r) && r.abs() > EFF_ZERO;
            !(valid && (!origin.iter().all(|x| x.is_finite()) || r.abs() > MAX_SPHERE_RADIUS))
        }
        OpName::SphereNoPole => {
            let r = a(3).abs();
            let valid = valid_double(a(3)) && r > EFF_ZERO;
            !valid || (MIN_NO_POLE_RADIUS..=MAX_NO_POLE_RADIUS).contains(&r)
        }
        OpName::CylindricalBox => {
            let (len, ri, ro, a0, a1) = (a(3), a(4), a(5), a(6), a(7));
            let positive = |v: f64| valid_double(v) && v > EFF_ZERO;
            let valid = positive(len)
                && positive(ri)
                && valid_double(ro)
                && ro > ri + EFF_ZERO
                && valid_double(a0)
                && valid_double(a1)
                && (-360.0..=360.0).contains(&a0)
                && a1 > a0 + EFF_ZERO
                && a1 - a0 <= 360.0;
            if !valid {
                return true;
            }
            let sweep = a1 - a0;
            let theta = sweep.to_radians();
            origin
                .iter()
                .all(|x| x.is_finite() && x.abs() <= MAX_KERNEL_COORD)
                && ri * theta >= MIN_ARC_CHORD
                && ri * theta * theta / 8.0 >= MIN_ARC_SAGITTA
                && 360.0 - sweep >= MIN_FULL_TURN_GAP
        }
        OpName::PlaneBrep
        | OpName::PlanarCircle
        | OpName::Torus
        | OpName::PartialSphere
        | OpName::PartialCone
        | OpName::ConeEllipticEnds
        | OpName::PartialCylinder
        | OpName::PartialTorus => bounded(&args),
        OpName::Swung => {
            bounded(&args)
                && i.xy.iter().chain(&i.xz).all(|c| {
                    bounded(&c.center) && bounded(&[c.radius, c.start_deg, c.end_deg])
                })
        }
        OpName::BlendPrimitive => {
            (0..6).all(|k| bounded(&[a(k)]))
                && (6..10).all(|k| {
                    let v = a(k);
                    v >= 0.0 && v <= MAX_BLEND_ENTITY as f64 && v == v.trunc()
                })
                && (a(10) == 0.0 || a(10) == 1.0)
        }
        OpName::OffsetProfile => {
            let flag = |v: f64| v == 0.0 || v == 1.0;
            let kind = a(1);
            bounded(&[a(0)])
                && kind >= 0.0
                && kind <= MAX_OFFSET_TYPE as f64
                && kind == kind.trunc()
                && flag(a(2))
                && flag(a(3))
        }
        OpName::CylinderSurface
        | OpName::ConeSurface
        | OpName::Box
        | OpName::Cylinder
        | OpName::Cone
        | OpName::Pyramid => true,
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.api_primitives"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // Redraw the (rare) inputs outside the contract, at most 64 times.
        let mut case = gen_case(rng);
        for _ in 0..64 {
            if accepted(&case) {
                break;
            }
            case = gen_case(rng);
        }
        serde_json::to_value(case).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let gen_ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut v = default_cases(self, &gen_ctx)?;
        // offset profiles under their own names and seeds, so that each `gen` case stays
        // the one the baseline has
        let offset_ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED_OFFSET),
            ..ctx.clone()
        };
        for mut c in offset_ctx.generated_cases("primitive.api_primitives:offset", |rng| {
            serde_json::to_value(gen_offset_case(rng)).ok()
        }) {
            c.unit = self.id().to_string();
            c.name = c.name.replacen("gen", "offset", 1);
            v.push(c);
        }
        Ok(v)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
