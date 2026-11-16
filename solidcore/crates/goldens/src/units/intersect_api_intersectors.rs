//! Differential coverage for `intersect.api_intersectors` (`SmApiIntersectCurves`,
//! `SmApiIntersectSurfaces`, `SmApiIntersectFaces`, `SmApiIntersectBreps`,
//! `SmApiIntersectCurveSurface`, `SmApiIntersectCurveFace`, `SmApiIntersectCurveBrep`).
//!
//! Ported from upstream `source/SM_API/src/SmApiIntersectors.cpp`; checked against its
//! golden. The Rust side builds the operands from the case: curves (`SmApiCreateCurve`,
//! `SmApiCreateLineSegment`, `SmApiCreateCircle`, `SmApiCreateArc`), surfaces
//! (`SmBSplineSurface::CreateCanonical` over a control net with clamped knots,
//! `SmApiCreateCylinder`, `SmApiCreateSurfaceRevolution`, `SmApiCreateRuledSurface`,
//! `SmApiCreateSurfaceFromCornerPoints`, and the `CreateCanonical` of the analytic planes,
//! cylinders, cones, spheres and surfaces of extrusion and revolution with an optional
//! `AdjustSTEPUVDomain`) and bodies (`SmApiCreateBox`, `SmApiCreateSphere`,
//! `SmApiCreateCylinder`, and a box with a hole by `SmApiBooleanDifference`), with the Rust
//! ports of those constructors. Pairs of analytic surfaces, and the curve/surface,
//! curve/face and curve/B-rep cases, are also generated from their own seeds (`ssi` and
//! `hit` cases), so the `gen` cases keep their inputs. The case then runs one API call and
//! records the status (`status`) and the record (`record`): the curve/curve parameters (and
//! points), or the count and samples of every 3D curve (type code, natural interval, five
//! evaluated points) and the points. Operands the Rust constructors refuse are `rejected`.

use serde::{Deserialize, Serialize};
use serde_json::Value;
use solidcore::curve::curve::Curve;
use solidcore::intersect::api_intersectors::{
    intersect_breps, intersect_curve_brep, intersect_curve_face, intersect_curve_surface,
    intersect_curves, intersect_faces, intersect_surfaces, BrepOperands, CurveHits, FaceBodies,
    Intersections,
};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent2d::Extent2d;
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::nurbs::b_spline_curve::KnotType;
use solidcore::nurbs::b_spline_surface::{
    BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData,
};
use solidcore::primitive::{api_curves, api_primitives};
use solidcore::surface::cone::Cone;
use solidcore::surface::plane::Plane;
use solidcore::surface::sphere::Sphere;
use solidcore::surface::surf_of_extrusion::SurfOfExtrusion;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep::p1 as brep1;
use solidcore::topology::Brep;
use solidcore::{Result as SolidResult, SolidError};

use crate::{Case, CaseContext, HarnessError, Output, Rng, UnitDiff};

/// The runner of the four `SM_API` intersectors.
pub struct Runner;

type P = [f64; 3];

/// A curve operand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    /// `SmApiCreateCurve(points)`.
    Points { points: Vec<P> },
    /// `SmApiCreateLineSegment(a, b)`.
    Segment { a: P, b: P },
    /// `SmApiCreateCircle(center, radius)` (`SmBSplineCurve` overload).
    Circle { center: P, radius: f64 },
    /// `SmApiCreateArc(center, radius, start, end)` (degrees).
    Arc {
        center: P,
        radius: f64,
        start: f64,
        end: f64,
    },
}

/// A surface operand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfaceIn {
    /// `SmBSplineSurface::CreateCanonical` of an `nu x nv` net (`points[i * nv + j]`),
    /// degrees `du`, `dv`, clamped knots `i / (n - d)`.
    Net {
        nu: usize,
        nv: usize,
        du: usize,
        dv: usize,
        points: Vec<P>,
    },
    /// `SmApiCreateCylinder(origin, radius, height)` (`SmSurface` overload).
    Cylinder { origin: P, radius: f64, height: f64 },
    /// `SmApiCreateSurfaceRevolution(SmApiCreateCurve(curve), origin, axis, angle)`.
    Revolution {
        curve: Vec<P>,
        origin: P,
        axis: P,
        angle: f64,
    },
    /// `SmApiCreateRuledSurface(SmApiCreateCurve(curve1), SmApiCreateCurve(curve2))`.
    Ruled { curve1: Vec<P>, curve2: Vec<P> },
    /// `SmApiCreateSurfaceFromCornerPoints(uminvmin, umaxvmin, uminvmax, umaxvmax)`.
    Corners { corners: [P; 4] },
    /// `SmApiCreateSurfaceFromOrderedPoints(points, rows, cols)`: the bicubic
    /// least-squares surface of a `rows x cols` grid (`points[i * cols + j]`).
    Ordered {
        rows: usize,
        cols: usize,
        points: Vec<P>,
    },
    /// An analytic surface's `CreateCanonical`, then `AdjustSTEPUVDomain(domain)` when
    /// given (`[umin, vmin, umax, vmax]`, STEP parameters).
    Analytic {
        shape: AnalyticIn,
        #[serde(default)]
        domain: Option<[f64; 4]>,
    },
}

/// An analytic surface (`frame`: `SmAxis2Placement::SetCanonical(origin, x, y)`).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum AnalyticIn {
    /// `SmPlane::CreateCanonical(frame)`.
    Plane { origin: P, x: P, y: P },
    /// `SmCylinder::CreateCanonical(frame, radius)` (infinite).
    Cylinder { origin: P, x: P, y: P, radius: f64 },
    /// `SmCone::CreateCanonical(frame, bot_radius, top_radius, height)` (`height` 0:
    /// infinite).
    Cone {
        origin: P,
        x: P,
        y: P,
        bot_radius: f64,
        top_radius: f64,
        height: f64,
    },
    /// `SmSphere::CreateCanonical(frame, radius)`.
    Sphere { origin: P, x: P, y: P, radius: f64 },
    /// `SmSurfOfExtrusion::CreateCanonical(SmApiCreateCurve(curve), vector)`.
    Extrusion { curve: Vec<P>, vector: P },
    /// `SmSurfOfRevolution::CreateCanonical(SmApiCreateCurve(curve), axis_point,
    /// axis_dir)`.
    Revolution {
        curve: Vec<P>,
        axis_point: P,
        axis_dir: P,
    },
}

/// A body operand.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum BodyIn {
    /// `SmApiCreateBox(origin, size)`.
    Box { origin: P, size: P },
    /// `SmApiCreateSphere(origin, radius)`.
    Sphere { origin: P, radius: f64 },
    /// `SmApiCreateCylinder(origin, radius, height)` (`SmBrep` overload).
    Cylinder { origin: P, radius: f64, height: f64 },
    /// `SmApiBooleanDifference(SmApiCreateBox(origin, size),
    /// SmApiCreateCylinder(hole_origin, hole_radius, hole_height))`.
    HoledBox {
        origin: P,
        size: P,
        hole_origin: P,
        hole_radius: f64,
        hole_height: f64,
    },
}

/// One API call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum Inputs {
    /// `SmApiIntersectCurves(curve1, curve2 or curve1, points?, t1, t2)`.
    Curves {
        curve1: CurveIn,
        /// `None`: `pCurve2 == pCurve1` (self-intersection).
        #[serde(default)]
        curve2: Option<CurveIn>,
        #[serde(default)]
        want_points: bool,
    },
    /// `SmApiIntersectSurfaces(surface1, surface2)`.
    Surfaces {
        surface1: SurfaceIn,
        surface2: SurfaceIn,
    },
    /// `SmApiIntersectFaces(face1 of body1, face2 of body2 (or of body1), points?)`.
    Faces {
        body1: BodyIn,
        /// `None`: both faces are of `body1`.
        #[serde(default)]
        body2: Option<BodyIn>,
        face1: usize,
        face2: usize,
        #[serde(default)]
        want_points: bool,
    },
    /// `SmApiIntersectBreps(body1, body2 or body1, points?)`.
    Breps {
        body1: BodyIn,
        /// `None`: `pBrep2 == pBrep1`.
        #[serde(default)]
        body2: Option<BodyIn>,
        #[serde(default)]
        want_points: bool,
    },
    /// `SmApiIntersectCurveSurface(curve, surface)`.
    CurveSurface { curve: CurveIn, surface: SurfaceIn },
    /// `SmApiIntersectCurveFace(curve, face of body)`.
    CurveFace {
        curve: CurveIn,
        body: BodyIn,
        face: usize,
    },
    /// `SmApiIntersectCurveBrep(curve, body)`.
    CurveBrep { curve: CurveIn, body: BodyIn },
}

fn herr(e: impl std::fmt::Display) -> HarnessError {
    HarnessError::new(e.to_string())
}

fn v(p: P) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

// ---- the shim's argument stream ----------------------------------------------------------

// ---- the Rust side ------------------------------------------------------------------------

fn points_of(points: &[P]) -> Vec<Vec3> {
    points.iter().map(|p| v(*p)).collect()
}

fn build_curve(c: &CurveIn) -> SolidResult<Curve> {
    Ok(match c {
        CurveIn::Points { points } => Curve::from(api_curves::create_curve(&points_of(points))?),
        CurveIn::Segment { a, b } => Curve::from(api_curves::create_line_segment(v(*a), v(*b))?),
        CurveIn::Circle { center, radius } => {
            Curve::from(api_curves::create_circle(v(*center), *radius)?)
        }
        CurveIn::Arc {
            center,
            radius,
            start,
            end,
        } => Curve::from(api_curves::create_arc(v(*center), *radius, *start, *end)?),
    })
}

/// The shim's `clamped_knots`.
fn clamped_knots(n: usize, d: usize) -> (Vec<f64>, Vec<usize>) {
    let m = n - d + 1;
    let knots = (0..m).map(|i| i as f64 / (m - 1) as f64).collect();
    let mults = (0..m)
        .map(|i| if i == 0 || i == m - 1 { d + 1 } else { 1 })
        .collect();
    (knots, mults)
}

fn build_surface(k: &mut Kernel, s: &SurfaceIn) -> SolidResult<Surface> {
    Ok(match s {
        SurfaceIn::Net {
            nu,
            nv,
            du,
            dv,
            points,
        } => {
            // the shim's guard (CreateCanonical itself would read past the net)
            if !(2..=16).contains(nu)
                || !(2..=16).contains(nv)
                || !(1..=3).contains(du)
                || !(1..=3).contains(dv)
                || du >= nu
                || dv >= nv
                || points.len() != nu * nv
            {
                return Err(SolidError::invalid_input(
                    "difftest::intersect_api_intersectors::build_surface",
                    "net",
                ));
            }
            let (uk, um) = clamped_knots(*nu, *du);
            let (vk, vm) = clamped_knots(*nv, *dv);
            Surface::from(BSplineSurface::create_canonical(&CanonicalSurfaceData {
                u_degree: *du,
                v_degree: *dv,
                control_points: &points_of(points),
                form: BSplineSurfaceForm::Unspecified,
                u_multiplicities: &um,
                v_multiplicities: &vm,
                u_knots: &uk,
                v_knots: &vk,
                knot_type: KnotType::Unspecified,
                weights: None,
                uv_domain: None,
            })?)
        }
        SurfaceIn::Cylinder {
            origin,
            radius,
            height,
        } => Surface::from(api_primitives::create_cylinder_surface(
            k,
            v(*origin),
            *radius,
            *height,
        )?),
        SurfaceIn::Revolution {
            curve,
            origin,
            axis,
            angle,
        } => {
            let gen = api_curves::create_curve(&points_of(curve))?;
            solidcore::sweep::api_surfaces::create_surface_revolution(
                k,
                Some(&gen),
                v(*origin),
                v(*axis),
                *angle,
            )?
        }
        SurfaceIn::Ruled { curve1, curve2 } => {
            let c1 = api_curves::create_curve(&points_of(curve1))?;
            let c2 = api_curves::create_curve(&points_of(curve2))?;
            Surface::from(solidcore::sweep::api_surfaces::create_ruled_surface(
                &c1, &c2,
            )?)
        }
        SurfaceIn::Corners { corners: [a, b, c, d] } => Surface::from(
            BSplineSurface::create_bilinear_surface(v(*a), v(*b), v(*c), v(*d))?,
        ),
        SurfaceIn::Ordered { rows, cols, points } => {
            // the shim's guard (its stream holds rows * cols points)
            if !(2..=16).contains(rows)
                || !(2..=16).contains(cols)
                || points.len() != rows * cols
            {
                return Err(SolidError::invalid_input(
                    "difftest::intersect_api_intersectors::build_surface",
                    "ordered grid",
                ));
            }
            Surface::from(
                solidcore::primitive::api_surface_ops::create_surface_from_ordered_points(
                    &points_of(points),
                    *rows as u64,
                    *cols as u64,
                )?,
            )
        }
        SurfaceIn::Analytic { shape, domain } => {
            let frame = |origin: &P, x: &P, y: &P| -> SolidResult<Axis2Placement> {
                let mut f = Axis2Placement::default();
                f.set_canonical(v(*origin), v(*x), v(*y))?;
                Ok(f)
            };
            let mut surface = match shape {
                AnalyticIn::Plane { origin, x, y } => {
                    Surface::from(Plane::create_canonical(&frame(origin, x, y)?)?)
                }
                AnalyticIn::Cylinder {
                    origin,
                    x,
                    y,
                    radius,
                } => Surface::from(Cone::create_canonical_cylinder(
                    k,
                    &frame(origin, x, y)?,
                    *radius,
                )?),
                AnalyticIn::Cone {
                    origin,
                    x,
                    y,
                    bot_radius,
                    top_radius,
                    height,
                } => Surface::from(Cone::create_canonical(
                    k,
                    &frame(origin, x, y)?,
                    *bot_radius,
                    *top_radius,
                    *height,
                    false,
                    false,
                )?),
                AnalyticIn::Sphere {
                    origin,
                    x,
                    y,
                    radius,
                } => Surface::from(Sphere::create_canonical(k, &frame(origin, x, y)?, *radius)?),
                AnalyticIn::Extrusion { curve, vector } => {
                    let gen = Curve::from(api_curves::create_curve(&points_of(curve))?);
                    Surface::from(SurfOfExtrusion::create_canonical(k, gen, v(*vector))?)
                }
                AnalyticIn::Revolution {
                    curve,
                    axis_point,
                    axis_dir,
                } => {
                    let gen = Curve::from(api_curves::create_curve(&points_of(curve))?);
                    Surface::from(SurfOfRevolution::create_canonical(
                        k,
                        gen,
                        v(*axis_point),
                        v(*axis_dir),
                        false,
                    )?)
                }
            };
            if let Some([u0, v0, u1, v1]) = domain {
                let mut d = Extent2d::default();
                d.set_min_max_coords(*u0, *v0, *u1, *v1)?;
                surface.adjust_step_uv_domain(k, &d)?;
            }
            surface
        }
    })
}

fn build_body(k: &mut Kernel, b: &BodyIn) -> SolidResult<Brep> {
    match b {
        BodyIn::Box { origin, size } => {
            api_primitives::create_box(k, v(*origin), size[0], size[1], size[2])
        }
        BodyIn::Sphere { origin, radius } => api_primitives::create_sphere(k, v(*origin), *radius),
        BodyIn::Cylinder {
            origin,
            radius,
            height,
        } => api_primitives::create_cylinder(k, v(*origin), *radius, *height),
        BodyIn::HoledBox {
            origin,
            size,
            hole_origin,
            hole_radius,
            hole_height,
        } => {
            let b = api_primitives::create_box(k, v(*origin), size[0], size[1], size[2])?;
            let hole =
                api_primitives::create_cylinder(k, v(*hole_origin), *hole_radius, *hole_height)?;
            solidcore::boolean::api_brep::boolean_difference(k, b, hole)
        }
    }
}

/// The status code of a failed call (as the shim records it: `SM_SUCCESS` is `1000`).
fn status(e: &SolidError) -> Option<f64> {
    match e {
        e if solidcore::error::upstream_status(e).is_some() => {
            solidcore::error::upstream_status(e).map(f64::from)
        }
        SolidError::InvalidInput { .. } => Some(1007.),
        _ => None,
    }
}

/// The shim's `put_curve`.
fn put_curve(k: &Kernel, out: &mut Vec<f64>, c: &Curve) {
    out.push(c.type_code() as f64);
    let d = c.natural_interval();
    out.extend([d.min(), d.max()]);
    for i in 0..5 {
        match c.evaluate_point(k, d.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => out.extend([0., p.x, p.y, p.z]),
            Err(_) => out.push(1.),
        }
    }
}

/// The shim's `put_intersections` after a successful call.
fn put_intersections(k: &Kernel, out: &mut Vec<f64>, r: &Intersections) {
    out.push(r.curves.len() as f64);
    for c in &r.curves {
        put_curve(k, out, c);
    }
    match &r.points {
        Some(points) => {
            out.push(points.len() as f64);
            for p in points {
                out.extend([p.x, p.y, p.z]);
            }
        }
        None => out.push(-1.),
    }
}

/// The shim's `put_intersections` of a curve/surface, curve/face or curve/B-rep call
/// that succeeded.
fn put_hits(k: &Kernel, out: &mut Vec<f64>, r: &CurveHits) {
    out.push(r.curves.len() as f64);
    for c in &r.curves {
        put_curve(k, out, c);
    }
    out.push(r.points.len() as f64);
    for p in &r.points {
        out.extend([p.x, p.y, p.z]);
    }
}

/// An operand the Rust constructors refuse: `rejected` (the native one refuses it too, or
/// the constructor's own unit shows the mismatch), unported deferred.
fn refused(e: &SolidError) -> Output {
    if let Some(out) = Output::unported(e) {
        return out;
    }
    Output::rejected(format!("rust operand build: {e}"))
}

fn output(raw: &[f64]) -> Result<Output, HarnessError> {
    let (&st, record) = raw.split_first().ok_or_else(|| herr("empty record"))?;
    Ok(Output::new()
        .scalar("status", st)
        .values("record", record.to_vec()))
}

fn rust_record(k: &mut Kernel, i: &Inputs) -> Result<Result<Vec<f64>, Output>, HarnessError> {
    macro_rules! build {
        ($e:expr) => {
            match $e {
                Ok(x) => x,
                Err(e) => return Ok(Err(refused(&e))),
            }
        };
    }
    let result: SolidResult<Vec<f64>>;
    // the record after the status of a failed call (the shim's layout)
    let failure_tail: Vec<f64>;
    match i {
        Inputs::Curves {
            curve1,
            curve2,
            want_points,
        } => {
            failure_tail = vec![0.];
            let c1 = build!(build_curve(curve1));
            let c2 = match curve2 {
                Some(c) => Some(build!(build_curve(c))),
                None => None,
            };
            result = intersect_curves(k, &c1, c2.as_ref().unwrap_or(&c1), *want_points).map(|r| {
                let mut out = vec![r.params1.len() as f64];
                for (j, (t1, t2)) in r.params1.iter().zip(&r.params2).enumerate() {
                    out.extend([*t1, *t2]);
                    if let Some(p) = r.points.as_ref().and_then(|p| p.get(j)) {
                        out.extend([p.x, p.y, p.z]);
                    }
                }
                out
            });
        }
        Inputs::Surfaces { surface1, surface2 } => {
            let s1 = build!(build_surface(k, surface1));
            let s2 = build!(build_surface(k, surface2));
            let k: &Kernel = k;
            // the curves are recorded also on a failed status (those a special-case
            // intersection left in the array, as the native side records them)
            let (curves, st) = intersect_surfaces(k, &s1, &s2);
            let mut out = vec![curves.len() as f64];
            for c in &curves {
                put_curve(k, &mut out, c);
            }
            failure_tail = out.clone();
            result = st.map(|()| out);
        }
        Inputs::Faces {
            body1,
            body2,
            face1,
            face2,
            want_points,
        } => {
            // SmApiIntersectFaces empties both arrays on failure
            failure_tail = vec![0., if *want_points { 0. } else { -1. }];
            let mut b1 = build!(build_body(k, body1));
            let mut b2 = match body2 {
                Some(b) => Some(build!(build_body(k, b))),
                None => None,
            };
            let f1 = brep1::faces(&b1, None).map_err(herr)?;
            let f2 = brep1::faces(b2.as_ref().unwrap_or(&b1), None).map_err(herr)?;
            let (Some(&f1), Some(&f2)) = (f1.get(*face1), f2.get(*face2)) else {
                return Ok(Err(Output::rejected("face index out of range")));
            };
            let bodies = match b2.as_mut() {
                Some(b2) => FaceBodies::Two(&mut b1, b2),
                None => FaceBodies::One(&mut b1),
            };
            let k: &Kernel = k;
            result = intersect_faces(k, bodies, f1, f2, *want_points).map(|r| {
                let mut out = Vec::new();
                put_intersections(k, &mut out, &r);
                out
            });
        }
        Inputs::Breps {
            body1,
            body2,
            want_points,
        } => {
            let mut b1 = build!(build_body(k, body1));
            let mut b2 = match body2 {
                Some(b) => Some(build!(build_body(k, b))),
                None => None,
            };
            let operands = match b2.as_mut() {
                Some(b2) => BrepOperands::Two(&mut b1, b2),
                None => BrepOperands::Same(&mut b1),
            };
            let k: &Kernel = k;
            // the outputs are recorded also on a failed status (the partial results
            // upstream leaves in the caller's arrays)
            let (r, st) = intersect_breps(k, operands, *want_points);
            let mut out = Vec::new();
            put_intersections(k, &mut out, &r);
            failure_tail = out.clone();
            result = st.map(|()| out);
        }
        Inputs::CurveSurface { curve, surface } => {
            // the call removes its own outputs on failure
            failure_tail = vec![0., 0.];
            let c = build!(build_curve(curve));
            let s = build!(build_surface(k, surface));
            let k: &Kernel = k;
            result = intersect_curve_surface(k, &c, &s).map(|r| {
                let mut out = Vec::new();
                put_hits(k, &mut out, &r);
                out
            });
        }
        Inputs::CurveFace { curve, body, face } => {
            failure_tail = vec![0., 0.];
            let c = build!(build_curve(curve));
            let mut b = build!(build_body(k, body));
            let faces = brep1::faces(&b, None).map_err(herr)?;
            let Some(&f) = faces.get(*face) else {
                return Ok(Err(Output::rejected("face index out of range")));
            };
            let k: &Kernel = k;
            result = intersect_curve_face(k, &c, &mut b, f).map(|r| {
                let mut out = Vec::new();
                put_hits(k, &mut out, &r);
                out
            });
        }
        Inputs::CurveBrep { curve, body } => {
            failure_tail = vec![0., 0.];
            let c = build!(build_curve(curve));
            let mut b = build!(build_body(k, body));
            let k: &Kernel = k;
            result = intersect_curve_brep(k, &c, &mut b).map(|r| {
                let mut out = Vec::new();
                put_hits(k, &mut out, &r);
                out
            });
        }
    }
    match result {
        Ok(record) => {
            // SM_SUCCESS
            let mut raw = vec![1000.];
            raw.extend(record);
            Ok(Ok(raw))
        }
        Err(e) if e.is_unported() => Ok(Err(Output::unported(&e).ok_or_else(|| herr(&e))?)),
        Err(e) => {
            let mut raw = vec![status(&e).ok_or_else(|| herr(&e))?];
            raw.extend(failure_tail);
            Ok(Ok(raw))
        }
    }
}

// ---- generated cases ----------------------------------------------------------------------

fn point(rng: &mut Rng, scale: f64) -> P {
    [
        rng.uniform(-1., 1.) * scale,
        rng.uniform(-1., 1.) * scale,
        rng.uniform(-1., 1.) * scale,
    ]
}

/// A cubic through `n` points along a wavy line across the origin's neighbourhood.
fn wavy(rng: &mut Rng, n: usize, scale: f64) -> Vec<P> {
    let dir = point(rng, 1.);
    let start = point(rng, scale);
    (0..n)
        .map(|j| {
            let t = j as f64 / (n - 1) as f64 * 2.0 - 1.0;
            let w = point(rng, 0.3 * scale);
            [
                -start[0] * t + dir[0] * scale * t + w[0],
                -start[1] * t + dir[1] * scale * t + w[1],
                -start[2] * t * 0.2 + w[2] * 0.3,
            ]
        })
        .collect()
}

fn gen_curve(rng: &mut Rng, scale: f64) -> CurveIn {
    match rng.below(4) {
        0 => {
            let n = 4 + rng.below(4) as usize;
            CurveIn::Points {
                points: wavy(rng, n, scale),
            }
        }
        1 => CurveIn::Segment {
            a: point(rng, scale),
            b: point(rng, scale),
        },
        2 => CurveIn::Circle {
            center: point(rng, 0.3 * scale),
            radius: rng.uniform(0.2, 1.0) * scale,
        },
        _ => {
            let start = rng.uniform(0., 300.);
            let center = point(rng, 0.3 * scale);
            let radius = rng.uniform(0.2, 1.0) * scale;
            let end = start + rng.uniform(20., 359.);
            // CreateCircleSegment takes angles in [-360, 360]: an arc ending past 360 is
            // drawn as the same arc one turn down (the draws are unchanged)
            let (start, end) = if end > 360. {
                (start - 360., end - 360.)
            } else {
                (start, end)
            };
            CurveIn::Arc {
                center,
                radius,
                start,
                end,
            }
        }
    }
}

/// The number of faces the shim's (and the port's) constructor gives `body`
/// (`GetFaces`): a drawn face index is taken modulo it, so it names a face of the body
/// (the draws are unchanged).
fn face_count(body: &BodyIn) -> usize {
    match body {
        BodyIn::Box { .. } => 6,
        BodyIn::Sphere { .. } => 1,
        BodyIn::Cylinder { .. } => 3,
        BodyIn::HoledBox { .. } => 7,
    }
}

fn gen_surface(rng: &mut Rng, scale: f64) -> SurfaceIn {
    match rng.below(4) {
        0 => {
            let (nu, nv) = (2 + rng.below(5) as usize, 2 + rng.below(5) as usize);
            let du = 1 + rng.below(3.min(nu as u64 - 1)) as usize;
            let dv = 1 + rng.below(3.min(nv as u64 - 1)) as usize;
            let tilt = point(rng, 0.5);
            let mut points = Vec::with_capacity(nu * nv);
            for i in 0..nu {
                for j in 0..nv {
                    let (x, y) = (
                        (i as f64 / (nu - 1) as f64 * 2.0 - 1.0) * scale,
                        (j as f64 / (nv - 1) as f64 * 2.0 - 1.0) * scale,
                    );
                    points.push([
                        x,
                        y,
                        tilt[0] * x + tilt[1] * y + rng.uniform(-0.3, 0.3) * scale,
                    ]);
                }
            }
            SurfaceIn::Net {
                nu,
                nv,
                du,
                dv,
                points,
            }
        }
        1 => SurfaceIn::Cylinder {
            origin: [
                rng.uniform(-0.3, 0.3) * scale,
                rng.uniform(-0.3, 0.3) * scale,
                -scale,
            ],
            radius: rng.uniform(0.3, 1.0) * scale,
            height: 2.0 * scale,
        },
        2 => SurfaceIn::Revolution {
            curve: (0..4)
                .map(|j| {
                    let t = f64::from(j) / 3.0;
                    [rng.uniform(0.3, 1.0) * scale, 0.0, (t * 2.0 - 1.0) * scale]
                })
                .collect(),
            origin: [0.0; 3],
            axis: [0.0, 0.0, 1.0],
            angle: match rng.below(3) {
                0 => 90.0,
                1 => 180.0,
                _ => 360.0,
            },
        },
        _ => SurfaceIn::Ruled {
            curve1: wavy(rng, 4, scale),
            curve2: wavy(rng, 4, scale),
        },
    }
}

fn gen_body(rng: &mut Rng, scale: f64) -> BodyIn {
    match rng.below(3) {
        0 => BodyIn::Box {
            origin: point(rng, 0.5 * scale),
            size: [
                rng.uniform(0.5, 1.5) * scale,
                rng.uniform(0.5, 1.5) * scale,
                rng.uniform(0.5, 1.5) * scale,
            ],
        },
        1 => BodyIn::Sphere {
            origin: point(rng, 0.5 * scale),
            radius: rng.uniform(0.4, 1.0) * scale,
        },
        _ => BodyIn::Cylinder {
            origin: point(rng, 0.5 * scale),
            radius: rng.uniform(0.3, 0.8) * scale,
            height: rng.uniform(0.5, 1.5) * scale,
        },
    }
}

fn unit(p: P) -> P {
    let n = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
    if n < 1e-3 {
        return [0., 0., 1.];
    }
    [p[0] / n, p[1] / n, p[2] / n]
}

/// A random orthonormal frame `(x, y)`; with `axis_aligned`, the canonical one.
fn gen_frame(rng: &mut Rng, axis_aligned: bool) -> (P, P) {
    if axis_aligned {
        return ([1., 0., 0.], [0., 1., 0.]);
    }
    let x = unit(point(rng, 1.));
    let b = point(rng, 1.);
    let d = b[0] * x[0] + b[1] * x[1] + b[2] * x[2];
    let y = unit([b[0] - d * x[0], b[1] - d * x[1], b[2] - d * x[2]]);
    (x, y)
}

/// A profile in the xz half-plane `x > 0` (a generator of revolution) or a planar curve
/// in xy (of extrusion).
fn gen_profile(rng: &mut Rng, scale: f64, in_xz: bool) -> Vec<P> {
    let n = 4 + rng.below(3) as usize;
    (0..n)
        .map(|j| {
            let t = (j as f64 / (n - 1) as f64 * 2.0 - 1.0) * scale;
            let r = rng.uniform(0.3, 1.0) * scale;
            if in_xz {
                [r, 0., t]
            } else {
                [t, rng.uniform(-0.5, 0.5) * scale, 0.]
            }
        })
        .collect()
}

/// An analytic surface (the special-case intersectors of `GlobalSurfaceIntersect`).
fn gen_analytic(rng: &mut Rng, scale: f64) -> SurfaceIn {
    let aligned = rng.chance(0.4);
    let (x, y) = gen_frame(rng, aligned);
    let origin = if aligned {
        [0., 0., rng.uniform(-0.5, 0.5) * scale]
    } else {
        point(rng, 0.5 * scale)
    };
    let (shape, domain) = match rng.below(6) {
        0 => (
            AnalyticIn::Plane { origin, x, y },
            rng.chance(0.6).then_some([-scale, -scale, scale, scale]),
        ),
        1 => (
            AnalyticIn::Cylinder {
                origin,
                x,
                y,
                radius: rng.uniform(0.2, 1.0) * scale,
            },
            Some([0., -scale, 360., scale]),
        ),
        2 => {
            let bot = rng.uniform(0.2, 1.0) * scale;
            let top = if rng.chance(0.3) {
                0.
            } else {
                rng.uniform(0.1, 1.0) * scale
            };
            (
                AnalyticIn::Cone {
                    origin,
                    x,
                    y,
                    bot_radius: bot,
                    top_radius: top,
                    height: rng.uniform(0.3, 1.5) * scale,
                },
                None,
            )
        }
        3 => (
            AnalyticIn::Sphere {
                origin,
                x,
                y,
                radius: rng.uniform(0.2, 1.0) * scale,
            },
            None,
        ),
        4 => (
            AnalyticIn::Extrusion {
                curve: gen_profile(rng, scale, false),
                vector: [0., 0., rng.uniform(0.5, 2.0) * scale],
            },
            None,
        ),
        _ => (
            AnalyticIn::Revolution {
                curve: gen_profile(rng, scale, true),
                axis_point: [0., 0., 0.],
                axis_dir: [0., 0., 1.],
            },
            None,
        ),
    };
    SurfaceIn::Analytic { shape, domain }
}

/// Two analytic surfaces: independent, or the second a copy of the first moved by a
/// tiny or a tolerance-sized offset (coincident, near-tangent and touching pairs).
fn gen_ssi(rng: &mut Rng) -> Option<Value> {
    let scale = 10f64.powi(rng.range_i64(-1, 2) as i32);
    let surface1 = gen_analytic(rng, scale);
    let surface2 = if rng.chance(0.3) {
        let mut s = surface1.clone();
        let shift = match rng.below(4) {
            0 => 1e-9,
            1 => 1e-7,
            2 => 1e-4,
            _ => 0.0,
        } * scale;
        if let SurfaceIn::Analytic {
            shape:
                AnalyticIn::Plane { origin, .. }
                | AnalyticIn::Cylinder { origin, .. }
                | AnalyticIn::Cone { origin, .. }
                | AnalyticIn::Sphere { origin, .. },
            ..
        } = &mut s
        {
            let d = point(rng, 1.);
            for (o, d) in origin.iter_mut().zip(d) {
                *o += shift * d;
            }
        }
        s
    } else {
        gen_analytic(rng, scale)
    };
    serde_json::to_value(Inputs::Surfaces { surface1, surface2 }).ok()
}

/// The axis-aligned box around `body` (exact for the boxes, the sphere and the
/// cylinder along z from its origin).
fn body_bounds(body: &BodyIn) -> (P, P) {
    match body {
        BodyIn::Box { origin, size } | BodyIn::HoledBox { origin, size, .. } => (
            *origin,
            [
                origin[0] + size[0],
                origin[1] + size[1],
                origin[2] + size[2],
            ],
        ),
        BodyIn::Sphere { origin, radius } => (
            [origin[0] - radius, origin[1] - radius, origin[2] - radius],
            [origin[0] + radius, origin[1] + radius, origin[2] + radius],
        ),
        BodyIn::Cylinder {
            origin,
            radius,
            height,
        } => (
            [origin[0] - radius, origin[1] - radius, origin[2]],
            [origin[0] + radius, origin[1] + radius, origin[2] + height],
        ),
    }
}

fn gen_hit_body(rng: &mut Rng, scale: f64) -> BodyIn {
    if !rng.chance(0.25) {
        return gen_body(rng, scale);
    }
    let origin = point(rng, 0.5 * scale);
    let size = [
        rng.uniform(0.8, 1.5) * scale,
        rng.uniform(0.8, 1.5) * scale,
        rng.uniform(0.8, 1.5) * scale,
    ];
    BodyIn::HoledBox {
        origin,
        size,
        hole_origin: [
            origin[0] + 0.5 * size[0],
            origin[1] + 0.5 * size[1],
            origin[2] - 0.1 * size[2],
        ],
        hole_radius: rng.uniform(0.1, 0.35) * size[0].min(size[1]),
        hole_height: 1.2 * size[2],
    }
}

/// A curve near `body`: a vertical line through its box, a segment across it, a segment
/// along a box edge, a horizontal circle (on a box's top or bottom face, or inside), or
/// any generated curve.
fn gen_body_curve(rng: &mut Rng, scale: f64, body: &BodyIn) -> CurveIn {
    let (lo, hi) = body_bounds(body);
    let ext = [hi[0] - lo[0], hi[1] - lo[1], hi[2] - lo[2]];
    let inside = |rng: &mut Rng, f: f64| -> P {
        let mut p = [0.0; 3];
        for (pi, (l, e)) in p.iter_mut().zip(lo.iter().zip(ext)) {
            *pi = l + e * rng.uniform(-f, 1.0 + f);
        }
        p
    };
    let is_box = matches!(body, BodyIn::Box { .. } | BodyIn::HoledBox { .. });
    match rng.below(5) {
        0 => {
            let p = inside(rng, 0.0);
            CurveIn::Segment {
                a: [p[0], p[1], lo[2] - 0.5 * ext[2]],
                b: [p[0], p[1], hi[2] + 0.5 * ext[2]],
            }
        }
        1 => CurveIn::Segment {
            a: inside(rng, 0.5),
            b: inside(rng, 0.5),
        },
        2 if is_box => {
            let (s, e) = (rng.uniform(0.1, 0.4), rng.uniform(0.6, 0.9));
            CurveIn::Segment {
                a: [lo[0] + s * ext[0], lo[1], lo[2]],
                b: [lo[0] + e * ext[0], lo[1], lo[2]],
            }
        }
        3 => {
            let z = match rng.below(3) {
                0 if is_box => hi[2],
                1 if is_box => lo[2],
                _ => lo[2] + rng.uniform(0.2, 0.8) * ext[2],
            };
            CurveIn::Circle {
                center: [lo[0] + 0.5 * ext[0], lo[1] + 0.5 * ext[1], z],
                radius: rng.uniform(0.15, 0.6) * ext[0].min(ext[1]),
            }
        }
        _ => gen_curve(rng, scale),
    }
}

/// A curve/surface, curve/face or curve/B-rep case.
fn gen_hit(rng: &mut Rng) -> Option<Value> {
    let scale = 10f64.powi(rng.range_i64(-1, 2) as i32);
    let i = match rng.below(3) {
        0 => {
            if rng.chance(0.4) {
                // a corner-point plane at height h (tilted or not), and a curve in it or
                // across it
                let h = rng.uniform(-0.5, 0.5) * scale;
                let tilt = if rng.chance(0.5) {
                    0.0
                } else {
                    rng.uniform(-0.5, 0.5) * scale
                };
                let surface = SurfaceIn::Corners {
                    corners: [
                        [-scale, -scale, h - tilt],
                        [scale, -scale, h - tilt],
                        [-scale, scale, h + tilt],
                        [scale, scale, h + tilt],
                    ],
                };
                let curve = match rng.below(3) {
                    0 => CurveIn::Circle {
                        center: [
                            rng.uniform(-0.3, 0.3) * scale,
                            rng.uniform(-0.3, 0.3) * scale,
                            h,
                        ],
                        radius: rng.uniform(0.2, 0.6) * scale,
                    },
                    1 => CurveIn::Segment {
                        a: [
                            rng.uniform(-1.5, 0.0) * scale,
                            rng.uniform(-1.5, 1.5) * scale,
                            h,
                        ],
                        b: [
                            rng.uniform(0.0, 1.5) * scale,
                            rng.uniform(-1.5, 1.5) * scale,
                            h,
                        ],
                    },
                    _ => gen_curve(rng, scale),
                };
                Inputs::CurveSurface { curve, surface }
            } else {
                let surface = if rng.chance(0.5) {
                    gen_analytic(rng, scale)
                } else {
                    gen_surface(rng, scale)
                };
                Inputs::CurveSurface {
                    curve: gen_curve(rng, scale),
                    surface,
                }
            }
        }
        1 => {
            let body = gen_hit_body(rng, scale);
            let curve = gen_body_curve(rng, scale, &body);
            let face = rng.below(7) as usize % face_count(&body);
            Inputs::CurveFace { curve, face, body }
        }
        _ => {
            let body = gen_hit_body(rng, scale);
            Inputs::CurveBrep {
                curve: gen_body_curve(rng, scale, &body),
                body,
            }
        }
    };
    serde_json::to_value(i).ok()
}

/// The unit id the `hit` cases' seeds derive from.
const HIT_SEEDS: &str = "intersect.api_intersectors.curve_hits";

/// The unit id the `ssi` cases' seeds derive from.
const SSI_SEEDS: &str = "intersect.api_intersectors.analytic_surfaces";

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "intersect.api_intersectors"
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        let i: Inputs = case.parse()?;
        let mut k = Kernel::default();
        match rust_record(&mut k, &i)? {
            Ok(raw) => output(&raw),
            Err(out) => Ok(out),
        }
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        let scale = 10f64.powi(rng.range_i64(-1, 2) as i32);
        let i = match rng.below(4) {
            0 => {
                let curve1 = gen_curve(rng, scale);
                let curve2 = (!rng.chance(0.15)).then(|| gen_curve(rng, scale));
                Inputs::Curves {
                    curve1,
                    curve2,
                    want_points: rng.chance(0.7),
                }
            }
            1 => Inputs::Surfaces {
                surface1: gen_surface(rng, scale),
                surface2: gen_surface(rng, scale),
            },
            2 => {
                let body1 = gen_body(rng, scale);
                let body2 = (!rng.chance(0.15)).then(|| gen_body(rng, scale));
                let face1 = rng.below(6) as usize % face_count(&body1);
                let face2 = rng.below(6) as usize % face_count(body2.as_ref().unwrap_or(&body1));
                Inputs::Faces {
                    body1,
                    body2,
                    face1,
                    face2,
                    want_points: rng.chance(0.7),
                }
            }
            _ => {
                let body1 = gen_body(rng, scale);
                let body2 = (!rng.chance(0.1)).then(|| gen_body(rng, scale));
                Inputs::Breps {
                    body1,
                    body2,
                    want_points: rng.chance(0.7),
                }
            }
        };
        serde_json::to_value(i).ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let mut ctx = ctx.clone();
        ctx.seeds = ctx.seeds.max(96);
        let mut cases = crate::runner::default_cases(self, &ctx)?;
        let mut ssi = ctx.clone();
        ssi.seeds = 64.max(ctx.seeds.saturating_sub(32));
        for mut case in ssi.generated_cases(SSI_SEEDS, gen_ssi) {
            case.unit = self.id().to_string();
            case.name = case.name.replacen("gen", "ssi", 1);
            cases.push(case);
        }
        let mut hits = ctx.clone();
        hits.seeds = 64.max(ctx.seeds.saturating_sub(32));
        for mut case in hits.generated_cases(HIT_SEEDS, gen_hit) {
            case.unit = self.id().to_string();
            case.name = case.name.replacen("gen", "hit", 1);
            cases.push(case);
        }
        Ok(cases)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The answer of a plane-first case of tangency-labs/solidcraft#17720.
    #[derive(Clone, Copy)]
    enum Crossing {
        /// Two open arcs, one each side of the xz plane, from the bottom rim to the top.
        TwoArcsRimToRim,
        /// One closed curve once around the axis.
        Loop,
        /// One open arc with both ends on the top rim, at these points (either order).
        ArcOnTopRim([[f64; 3]; 2]),
    }

    /// The four plane-first cases of tangency-labs/solidcraft#17720, with their answers. In
    /// each the plane crosses a revolution about the z axis whose profile rises strictly in z.
    /// - `near-axis-parallel`: the plane `x = 1/2 + 1e-6 z` (to rounding) meets every
    ///   parallel circle of the barrel (radius 1 to 1.375) twice, at `cos(theta) = (1/2 +
    ///   1e-6 z) / r`: two open arcs, `y > 0` and `y < 0`, each from the bottom rim to the top.
    /// - `oblique` and `ssi0043`: along every meridian the plane's signed distance is
    ///   negative at the bottom rim and positive at the top, and rises strictly (the plane's
    ///   slope over its tilt, 4/3 and 1.23, is above the profile's largest `|dr/dz|`, 0.71
    ///   and 0.52, from its control points): one point on each meridian, one closed curve
    ///   once around the axis.
    /// - `ssi0048`: the signed distance is negative all round the bottom rim (at most
    ///   -0.25) and changes sign twice round the top rim, at the two points below (the roots
    ///   of `a cos(theta) + b sin(theta) = c` on the circle of radius 0.832 at z = 1, theta
    ///   72.0 and 354.2 degrees). Counting sign changes along 3600 meridians at 4001 points
    ///   each finds one on each meridian between those two and none on the others: one open
    ///   arc between the two points, 282 degrees round the axis.
    const PLANE_FIRST: [(&str, usize, Crossing); 4] = [
        (
            "ssi-revolution-plane-near-axis-parallel-swapped",
            2,
            Crossing::TwoArcsRimToRim,
        ),
        ("ssi-revolution-plane-oblique-swapped", 1, Crossing::Loop),
        ("ssi0043", 1, Crossing::Loop),
        (
            "ssi0048",
            1,
            Crossing::ArcOnTopRim([
                [0.257_038_378_677_531, 0.791_107_148_764_212, 1.0],
                [0.827_586_313_590_561, -0.083_786_290_624_427, 1.0],
            ]),
        ),
    ];

    /// A plane-first case: the plane, the revolution, their natural domains, the plane's
    /// point and unit normal and the revolution's profile (in the xz plane).
    struct PlaneFirst {
        k: Kernel,
        plane: Surface,
        revolution: Surface,
        domains: [Extent2d; 2],
        origin: Vec3,
        normal: Vec3,
        profile: Curve,
    }

    fn plane_first(name: &str) -> PlaneFirst {
        use solidcore::surface::surface::SurfaceKind;
        let case = crate::case::regression_case(&Runner, name);
        let Inputs::Surfaces { surface1, surface2 } = case.parse().unwrap() else {
            panic!("{name}: expected a surface pair");
        };
        let SurfaceIn::Analytic {
            shape:
                AnalyticIn::Revolution {
                    curve,
                    axis_point,
                    axis_dir,
                },
            ..
        } = &surface2
        else {
            panic!("{name}: expected a revolution second");
        };
        assert_eq!(*axis_point, [0.0; 3], "{name}");
        assert_eq!(*axis_dir, [0.0, 0.0, 1.0], "{name}");
        let mut k = Kernel::default();
        let plane = build_surface(&mut k, &surface1).unwrap();
        let revolution = build_surface(&mut k, &surface2).unwrap();
        let profile = Curve::from(api_curves::create_curve(&points_of(curve)).unwrap());
        let SurfaceKind::Plane(p) = plane.kind() else {
            panic!("{name}: expected a plane first");
        };
        let (origin, normal, _) = p.is_planar();
        let domains = [
            plane.natural_uv_domain_with_nurb(&k).unwrap(),
            revolution.natural_uv_domain_with_nurb(&k).unwrap(),
        ];
        PlaneFirst {
            k,
            plane,
            revolution,
            domains,
            origin,
            normal: normal.unitize().unwrap(),
            profile,
        }
    }

    impl PlaneFirst {
        /// The default tolerances of the plane first and of the revolution first.
        fn tolerances(&self) -> (f64, f64) {
            use solidcore::intersect::surface_intersector::p1::SurfaceIntersector;
            let [d1, d2] = &self.domains;
            let (s1, s2) = (&self.plane, &self.revolution);
            (
                SurfaceIntersector::new(&self.k, s1, d1, s2, d2, false)
                    .unwrap()
                    .this_approx_tol_3d,
                SurfaceIntersector::new(&self.k, s2, d2, s1, d1, false)
                    .unwrap()
                    .this_approx_tol_3d,
            )
        }

        /// The plane-first intersection over the natural domains, at `tol` (`None`: the
        /// default).
        fn intersect(
            &self,
            tol: Option<f64>,
        ) -> (
            solidcore::surface::surface::SurfaceIntersection,
            SolidResult<()>,
        ) {
            let [d1, d2] = &self.domains;
            self.plane.global_surface_intersect_with_partial(
                &self.k,
                d1,
                &self.revolution,
                d2,
                [true, true],
                tol,
                None,
            )
        }

        fn off_plane(&self, p: Vec3) -> f64 {
            self.normal.dot(p - self.origin).abs()
        }

        /// A bound on the distance from `p` to the revolution: the distance, in `p`'s
        /// meridian half-plane, to the profile's point at `p`'s height (bisected; the
        /// profile's heights rise strictly; past an end, the end's point).
        fn off_revolution(&self, p: Vec3) -> f64 {
            let at = |t: f64| self.profile.evaluate_point(&self.k, t).unwrap();
            let (mut lo, mut hi) = (0.0, 1.0);
            for _ in 0..80 {
                let mid = 0.5 * (lo + hi);
                if at(mid).z < p.z {
                    lo = mid;
                } else {
                    hi = mid;
                }
            }
            let q = at(0.5 * (lo + hi));
            let rho = (p.x * p.x + p.y * p.y).sqrt();
            ((rho - q.x).powi(2) + (p.z - q.z).powi(2)).sqrt()
        }
    }

    /// Asserts that `r` is the answer `shape` of `name` (see [`PLANE_FIRST`]): `count`
    /// crossing curves, each within `tol` of both surfaces at 65 points of its interval, of
    /// that shape.
    fn assert_crossing_curves(
        c: &PlaneFirst,
        name: &str,
        r: &solidcore::surface::surface::SurfaceIntersection,
        (count, shape): (usize, Crossing),
        tol: f64,
    ) {
        use solidcore::surface::surface::TsectCurveType;
        use std::f64::consts::PI;
        assert_eq!(r.curves_3d.len(), count, "{name}: {:?}", r.curve_types);
        assert!(
            r.curve_types.iter().all(|t| *t == TsectCurveType::Crossing),
            "{name}: {:?}",
            r.curve_types
        );
        let bottom = c.profile.evaluate_point(&c.k, 0.0).unwrap().z;
        let top = c.profile.evaluate_point(&c.k, 1.0).unwrap().z;
        let mut sides = Vec::new();
        for curve in &r.curves_3d {
            let iv = curve.natural_interval();
            let pts: Vec<Vec3> = (0..=64)
                .map(|i| {
                    curve
                        .evaluate_point(&c.k, iv.evaluate(f64::from(i) / 64.0))
                        .unwrap()
                })
                .collect();
            for p in &pts {
                assert!(c.off_plane(*p) <= tol, "{name}: {p:?} off the plane");
                assert!(c.off_revolution(*p) <= tol, "{name}: {p:?} off the revolution");
            }
            let (first, last) = (pts[0], pts[64]);
            match shape {
                Crossing::Loop => {
                    assert!(first.distance(last) <= tol, "{name}: not closed");
                    let turn: f64 = pts
                        .windows(2)
                        .map(|w| {
                            let d = w[1].y.atan2(w[1].x) - w[0].y.atan2(w[0].x);
                            (d + PI).rem_euclid(2.0 * PI) - PI
                        })
                        .sum();
                    assert!(
                        (turn.abs() - 2.0 * PI).abs() < 0.1,
                        "{name}: turns {turn} about the axis"
                    );
                }
                Crossing::TwoArcsRimToRim => {
                    let (lo, hi) = (first.z.min(last.z), first.z.max(last.z));
                    assert!((lo - bottom).abs() <= tol, "{name}: starts at z = {lo}");
                    assert!((hi - top).abs() <= tol, "{name}: ends at z = {hi}");
                    let side = pts[32].y.signum();
                    assert!(pts.iter().all(|p| p.y.signum() == side), "{name}");
                    sides.push(side);
                }
                Crossing::ArcOnTopRim([a, b]) => {
                    let (a, b) = (Vec3::new(a[0], a[1], a[2]), Vec3::new(b[0], b[1], b[2]));
                    let ends = first.distance(a).max(last.distance(b));
                    let swapped = first.distance(b).max(last.distance(a));
                    assert!(
                        ends.min(swapped) <= tol,
                        "{name}: ends {first:?} {last:?}"
                    );
                }
            }
        }
        if matches!(shape, Crossing::TwoArcsRimToRim) {
            sides.sort_by(f64::total_cmp);
            assert_eq!(sides, [-1.0, 1.0], "{name}");
        }
    }

    /// WR-51, tangency-labs/solidcraft#17720: with the plane first, the intersection finds a
    /// lone touching point where the plane crosses the revolution. `intersect_surfaces`
    /// refuses that answer with its typed cause, naming the tolerance it ran at (the
    /// plane's, a thousandth of its 2.47e6 domain) and the revolution's own. At the
    /// revolution's tolerance the plane-first intersection finds the crossing curves of
    /// [`PLANE_FIRST`], and `intersect_surfaces_within` returns them.
    #[test]
    fn plane_first_pairs_refuse_touching_points_where_the_plane_crosses() {
        use solidcore::error::intersect::IntersectFailure;
        use solidcore::error::ErrorKind;
        use solidcore::intersect::api_intersectors::intersect_surfaces_within;
        for (name, count, shape) in PLANE_FIRST {
            let c = plane_first(name);
            let (tolerance, surface_tolerance) = c.tolerances();
            assert!(tolerance > 3000.0 && surface_tolerance < 0.03, "{name}");
            let (curves, status) = intersect_surfaces(&c.k, &c.plane, &c.revolution);
            assert!(curves.is_empty(), "{name}: {} curves", curves.len());
            let e = status.unwrap_err();
            assert_eq!(
                e.intersect_failure(),
                Some(&IntersectFailure::PointsWhereAPlaneCrosses {
                    tolerance,
                    surface_tolerance
                }),
                "{name}: {e}"
            );
            assert_eq!(e.kind(), ErrorKind::NotWithinTolerance, "{name}");

            let (r, status) = c.intersect(Some(surface_tolerance));
            status.unwrap();
            assert_crossing_curves(&c, name, &r, (count, shape), surface_tolerance);
            let (curves, status) =
                intersect_surfaces_within(&c.k, &c.plane, &c.revolution, Some(surface_tolerance));
            status.unwrap();
            assert_eq!(curves.len(), count, "{name}");
        }
    }

    /// The refusal needs the plane to cross: a plane tangent to the barrel of
    /// `ssi-revolution-plane-near-axis-parallel` at its widest circle (x = 1.375, the
    /// profile's largest radius at z = 0) leaves every point of the revolution on one side,
    /// so `intersect_surfaces` is not refused for it: it returns the one touching point.
    #[test]
    fn a_plane_tangent_to_the_revolution_is_not_refused() {
        let c = plane_first("ssi-revolution-plane-near-axis-parallel-swapped");
        let mut k = Kernel::default();
        let plane = build_surface(
            &mut k,
            &SurfaceIn::Analytic {
                shape: AnalyticIn::Plane {
                    origin: [1.375, 0.0, 0.0],
                    x: [0.0, 1.0, 0.0],
                    y: [0.0, 0.0, 1.0],
                },
                domain: None,
            },
        )
        .unwrap();
        let (curves, status) = intersect_surfaces(&k, &plane, &c.revolution);
        status.unwrap();
        assert_eq!(curves.len(), 1);
        let p = curves[0]
            .evaluate_point(&k, curves[0].natural_interval().evaluate(0.5))
            .unwrap();
        let (_, surface_tolerance) = c.tolerances();
        assert!(
            p.distance(Vec3::new(1.375, 0.0, 0.0)) <= surface_tolerance,
            "{p:?}"
        );
    }

    /// The refusal needs a point farther from the surfaces than the other surface's own
    /// tolerance: the plane `z = 4 - 1e-7` crosses the cone of `ssi-cone-plane-near-apex`
    /// (apex at z = 4, half-angle atan(1/2)) in a circle of radius 5e-8, far below the cone's
    /// tolerance 5.84e-3, so the touching point at its centre (deviation 0) is that circle
    /// at the cone's size, in either order, and is not refused.
    #[test]
    fn a_touching_point_within_the_surface_tolerance_is_not_refused() {
        for name in [
            "ssi-cone-plane-near-apex",
            "ssi-cone-plane-near-apex-swapped",
        ] {
            let case = crate::case::regression_case(&Runner, name);
            let Inputs::Surfaces { surface1, surface2 } = case.parse().unwrap() else {
                panic!("{name}: expected a surface pair");
            };
            let mut k = Kernel::default();
            let s1 = build_surface(&mut k, &surface1).unwrap();
            let s2 = build_surface(&mut k, &surface2).unwrap();
            let (curves, status) = intersect_surfaces(&k, &s1, &s2);
            status.unwrap_or_else(|e| panic!("{name}: {e}"));
            assert_eq!(curves.len(), 1, "{name}");
            let p = curves[0]
                .evaluate_point(&k, curves[0].natural_interval().evaluate(0.5))
                .unwrap();
            let radius = 2.0 * 1e-7 / 4.0;
            assert!(
                (p.z - 3.9999999).abs() <= 1e-12 && (p.x * p.x + p.y * p.y).sqrt() <= 2.0 * radius,
                "{name}: {p:?}"
            );
        }
    }

    /// A tolerance the caller gives, larger than the other surface's own, keeps a touching
    /// point within it: the crossing loop of `ssi-revolution-plane-oblique-swapped` (round
    /// the axis of a revolution about 2 across) is bigger than the revolution's tolerance
    /// 5.15e-3 but far smaller than the plane's default tolerance 3.49e3. Asked for at that
    /// tolerance, the plane-first intersection gives one touching point, deviation 0.2,
    /// past the revolution's tolerance and within the caller's: an answer at the size the
    /// caller asked for, which `intersect_surfaces_within` keeps. Without a tolerance from
    /// the caller the same point is refused
    /// (`plane_first_pairs_refuse_touching_points_where_the_plane_crosses`).
    #[test]
    fn a_touching_point_within_the_callers_tolerance_is_not_refused() {
        use solidcore::intersect::api_intersectors::intersect_surfaces_within;
        use solidcore::surface::surface::TsectCurveType;
        let name = "ssi-revolution-plane-oblique-swapped";
        let c = plane_first(name);
        let (t, surface_tolerance) = c.tolerances();

        // the premises: touching points only, one past the revolution's tolerance, none
        // past the caller's, and the loop is smaller than the caller's tolerance
        let (r, status) = c.intersect(Some(t));
        status.unwrap();
        assert!(
            !r.curve_types.is_empty()
                && r.curve_types.iter().all(|ty| *ty == TsectCurveType::Touching),
            "{name}: {:?} {:?}",
            r.curve_types,
            r.deviations
        );
        assert!(
            r.deviations.iter().any(|d| *d > surface_tolerance)
                && r.deviations.iter().all(|d| *d <= t),
            "{name}: {:?}",
            r.deviations
        );
        let (crossing, status) = c.intersect(Some(surface_tolerance));
        status.unwrap();
        assert_crossing_curves(&c, name, &crossing, (1, Crossing::Loop), surface_tolerance);
        let iv = crossing.curves_3d[0].natural_interval();
        let pts: Vec<Vec3> = (0..=64)
            .map(|i| {
                crossing.curves_3d[0]
                    .evaluate_point(&c.k, iv.evaluate(f64::from(i) / 64.0))
                    .unwrap()
            })
            .collect();
        let span = pts
            .iter()
            .flat_map(|p| pts.iter().map(move |q| p.distance(*q)))
            .fold(0.0, f64::max);
        assert!(surface_tolerance < span && span < t, "{name}: loop {span} across");

        let (curves, status) = intersect_surfaces_within(&c.k, &c.plane, &c.revolution, Some(t));
        status.unwrap();
        assert_eq!(curves.len(), r.curves_3d.len(), "{name}");
    }

    /// Known failing, tangency-labs/solidcraft#17720 (replaces
    /// `wr51_plane_first_pairs_report_their_touching_point`, which asserted the lone touching
    /// point): with the plane first and the default tolerance, the intersection should find
    /// the crossing curves of [`PLANE_FIRST`], within the revolution's own
    /// tolerance, as the revolution-first order does. Today it finds one touching point
    /// (deviation 0.500001, 0.2, 2.44 and 0.52), which `intersect_surfaces` refuses
    /// (`plane_first_pairs_refuse_touching_points_where_the_plane_crosses`).
    #[test]
    #[ignore = "known failing: plane first, the default tolerance finds a lone touching point, not the crossing curves (tangency-labs/solidcraft#17720)"]
    fn wr51_plane_first_pairs_report_their_crossing_curves() {
        for (name, count, shape) in PLANE_FIRST {
            let c = plane_first(name);
            let (_, surface_tolerance) = c.tolerances();
            let (r, status) = c.intersect(None);
            status.unwrap();
            assert_crossing_curves(&c, name, &r, (count, shape), surface_tolerance);
        }
    }

    #[test]
    fn face_counts_are_the_bodies_own() {
        let bodies = [
            BodyIn::Box {
                origin: [0.; 3],
                size: [10.; 3],
            },
            BodyIn::Sphere {
                origin: [0.; 3],
                radius: 2.,
            },
            BodyIn::Cylinder {
                origin: [0.; 3],
                radius: 1.,
                height: 3.,
            },
            BodyIn::HoledBox {
                origin: [0.; 3],
                size: [10.; 3],
                hole_origin: [5., 5., -1.],
                hole_radius: 2.,
                hole_height: 12.,
            },
        ];
        for body in &bodies {
            let mut k = Kernel::default();
            let b = build_body(&mut k, body).unwrap();
            assert_eq!(
                brep1::faces(&b, None).unwrap().len(),
                face_count(body),
                "{body:?}"
            );
        }
    }
}
