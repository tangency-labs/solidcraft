//! Runner for unit `primitive.api_primitives`: the `SM_API` primitive constructors.
//!
//! C++: `solidcore_ffi::units::primitive_api_primitives` (upstream
//! `source/SM_API/src/SmApiPrimitives.cpp`). Rust: `solidcore::primitive::api_primitives`.
//!
//! A case is one API call: `op`, its `args` in the API's order (see the ffi module; floats
//! may be `"NaN"`, `"inf"`, `"-inf"`) and, for `swung`, the `xy` and `xz` arcs. Both sides
//! produce the record documented in the ffi module: the status (`0` = `SM_SUCCESS`, else
//! the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust error upstream cannot
//! produce), then on success `has` and the surface or B-rep record, reported as the values
//! `rec`.
//!
//! Surface overloads compare their parameterizations. Successful B-reps additionally
//! compare reachable vertices, sampled curves/surfaces, tolerances and UV trim counts.
//! A pyramid whose plane fit reaches UB-119 is rejected (see [`ub_119`]).

use serde::{Deserialize, Deserializer, Serialize, Serializer};
use serde_json::Value;

use solidcore::curve::curve::Curve as SmCurve;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::{Point3, Vec3};
use solidcore::kernel::Kernel;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::primitive::api_curves;
use solidcore::primitive::api_primitives as api;
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::torus::Torus;
use solidcore::topology::Brep;
use solidcore::SolidError;
use solidcore_ffi::units::primitive_api_primitives::{self as ffi, Arc, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

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
}

impl OpName {
    const fn ffi(self) -> Op {
        match self {
            Self::PlaneSurface => Op::PlaneSurface,
            Self::CylinderSurface => Op::CylinderSurface,
            Self::ConeSurface => Op::ConeSurface,
            Self::TorusSurface => Op::TorusSurface,
            Self::PlaneBrep => Op::PlaneBrep,
            Self::PlanarCircle => Op::PlanarCircle,
            Self::Box => Op::Box,
            Self::Sphere => Op::Sphere,
            Self::Cylinder => Op::Cylinder,
            Self::Cone => Op::Cone,
            Self::Torus => Op::Torus,
            Self::Pyramid => Op::Pyramid,
            Self::CylindricalBox => Op::CylindricalBox,
            Self::PartialSphere => Op::PartialSphere,
            Self::SphereNoPole => Op::SphereNoPole,
            Self::PartialCone => Op::PartialCone,
            Self::ConeEllipticEnds => Op::ConeEllipticEnds,
            Self::PartialCylinder => Op::PartialCylinder,
            Self::PartialTorus => Op::PartialTorus,
            Self::Swung => Op::Swung,
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

impl ArcIn {
    const fn ffi(self) -> Arc {
        Arc {
            center: self.center,
            radius: self.radius,
            start_deg: self.start_deg,
            end_deg: self.end_deg,
        }
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

// ---- C++ -------------------------------------------------------------------------------

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    if i.op == OpName::Pyramid && ub_119(i)? {
        return Ok(Output::rejected(
            "UB-119: SmTrimmingTools::FindOuterLoops reads past its arrays for a zero-area \
             loop of the pyramid's plane fit; not run",
        ));
    }
    let args: Vec<f64> = i.args.iter().map(|f| f.0).collect();
    let xy: Vec<Arc> = i.xy.iter().map(|a| a.ffi()).collect();
    let xz: Vec<Arc> = i.xz.iter().map(|a| a.ffi()).collect();
    let arcs = (i.op == OpName::Swung).then_some((xy.as_slice(), xz.as_slice()));
    match ffi::run(i.op.ffi(), &args, arcs) {
        Ok(rec) => Ok(Output::new().values("rec", rec)),
        Err(e) => Err(HarnessError::new(e.to_string())),
    }
}

/// A pyramid face whose plane fit hits `FindOuterLoops`' zero-area loop (a base far
/// from the origin relative to its size): upstream reads past its arrays there
/// (`plan/upstream-issues.md` UB-119), so the input is undefined in C++ (AGENTS.md
/// section 4, step 5.6) and [`cpp`] rejects it. Decided from the inputs and the Rust
/// side only: the port's `find_outer_loops` returns `InvalidInput` ("index out of
/// range") exactly where upstream's count translation reads past `sOutLoops`.
fn ub_119(i: &Inputs) -> Result<bool, HarnessError> {
    let r = api::create_pyramid(&mut Kernel::default(), i.point(0)?, i.arg(3)?, i.arg(4)?);
    Ok(matches!(
        r,
        Err(SolidError::InvalidInput {
            operation: "topology::trimming_tools",
            reason: "index out of range",
        })
    ))
}

/// The ffi's guard accepts the case.
fn accepted(i: &Inputs) -> bool {
    let args: Vec<f64> = i.args.iter().map(|f| f.0).collect();
    let xy: Vec<Arc> = i.xy.iter().map(|a| a.ffi()).collect();
    let xz: Vec<Arc> = i.xz.iter().map(|a| a.ffi()).collect();
    let arcs = (i.op == OpName::Swung).then_some((xy.as_slice(), xz.as_slice()));
    ffi::guard(i.op.ffi(), &args, arcs).is_ok()
}

// ---- Rust ------------------------------------------------------------------------------

/// The upstream status of a Rust error.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => 1007.0,
        SolidError::Upstream { code, .. } => f64::from(*code),
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
    use solidcore::topology::brep::p1;
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
    if i.op != OpName::Swung && i.args.len() != i.op.ffi().arity() {
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
        _ => {
            if rng.chance(0.5) {
                mk(
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
                )
            } else {
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
                }
            }
        }
    }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "primitive.api_primitives"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // Redraw the (rare) inputs the ffi fences off (upstream crashes or hangs there).
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
