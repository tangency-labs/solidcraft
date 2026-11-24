//! Runner for unit `sweep.rotational_sweep_geometry`. Ported from upstream
//! `source/SMLib/src/SmRotationalSweepGeometry.cpp`; checked against its golden. Rust:
//! `solidcore::sweep::rotational_sweep_geometry`.
//!
//! A case is a sweep (`base`, `axis`, `angle` in degrees, `eps`, `analytics` =
//! `SetMakeAnalytics`, `brep_tol` = the B-rep tolerance when positive) and one `op`.
//! Edges are wire edges of a real `SmBrep` (`MakeShellVertex` + `MakeWireEdgeVertex`,
//! or `MakeWireEdge` from the vertex to itself for a closed edge) on a real curve
//! (`SmBSplineCurve`, B-spline line, `SmLine`, `SmCircle`, `SmEllipse`,
//! `SmHermiteCurve`) over `t`, same or opposite to the curve, with its vertices at the
//! curve ends plus offsets and given edge / vertex tolerances; vertices are shell
//! vertices, faces `CreateFaceFromSurface` on an `SmBSplineSurface` or finite `SmPlane`.
//! The runner builds these as B-rep records and reports the facts they reproduce
//! (B-rep tolerance, which vertex is the start / primary one, closed, points,
//! tolerances) first.
//!
//! The flat record (`out`) holds statuses (`0` =
//! `SM_SUCCESS`, else the `SmStatus`; `1007` for a Rust `InvalidInput`, `-1` for a Rust
//! error upstream cannot produce), curves and surfaces as type code, natural interval /
//! domain and evaluations at fixed fractions of it, tolerances, the trimming loop's
//! curve indices and orientations, the surface normal.
//!
//! Deferred cases: `CopyAndAddAnalytics` of a non-planar B-spline surface
//! (`SmCone::IsNurbSurfaceCone`, unit `surface.cone_p2`).

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::Line;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface as NlSurface;
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::sweep::rotational_sweep_geometry::RotationalSweepGeometry;
use solidcore::topology::{
    edge, Brep, Edge, EdgeId, Edgeuse, EdgeuseOwner, Face, Region, Shell, Vertex, VertexId,
    Vertexuse, VertexuseOwner,
};
use solidcore::SolidError;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::util::{flag, v3};
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The unit runner.
pub struct Runner;

/// Generated cases at least (the default `--seeds` is far too few for the op mix).
const MIN_GENERATED: usize = 400;
/// Samples per curve and per surface direction (as the shim).
const N_CURVE_SAMPLES: usize = 7;
const N_SURF_SAMPLES: usize = 5;
/// `SM_ERR_INVALID_INPUT`.
const SM_ERR_INVALID_INPUT: f64 = 1007.0;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs ---------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
struct SweepIn {
    base: [f64; 3],
    axis: [f64; 3],
    angle: f64,
    eps: f64,
    analytics: bool,
    brep_tol: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    /// A 3D B-spline: control points as stored `[x, y, z, w]` (`w = -f64::MAX` for
    /// non-rational), degree, knots.
    Nurb {
        p: usize,
        cpts: Vec<[f64; 4]>,
        knots: Vec<f64>,
    },
    /// `SmBSplineCurve::CreateLineSegment`.
    Bline { a: [f64; 3], b: [f64; 3] },
    /// `SmLine::CreateLineSegment`, optionally reparameterised.
    Line {
        a: [f64; 3],
        b: [f64; 3],
        ivl: Option<[f64; 2]>,
    },
    /// `SmCircle` (domain in degrees).
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
        io: bool,
    },
    /// `SmEllipse` (domain in degrees).
    Ellipse {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        rx: f64,
        ry: f64,
        io: bool,
    },
    /// `SmHermiteCurve`.
    Hermite {
        p1: [f64; 3],
        d1: [f64; 3],
        p2: [f64; 3],
        d2: [f64; 3],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeIn {
    curve: CurveIn,
    t: [f64; 2],
    opposite: bool,
    closed: bool,
    off0: [f64; 3],
    off1: [f64; 3],
    etol: f64,
    vtol: [f64; 2],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    /// A B-spline surface: net rows in u, points as stored `[x, y, z, w]`.
    Nurb {
        p: usize,
        q: usize,
        net: Vec<Vec<[f64; 4]>>,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    /// A finite plane `o + u * su * X + v * sv * Y` over `dom = [u0, v0, u1, v1]`.
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        scale: [f64; 2],
        dom: [f64; 4],
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "op", rename_all = "snake_case")]
enum OpIn {
    IsClosed {
        repeats: u64,
    },
    StartCs {
        pt: [f64; 3],
    },
    EndCs {
        pt: [f64; 3],
    },
    Transform,
    Copy,
    CheckCurve {
        curve: CurveIn,
        t: [f64; 2],
        tol: f64,
        pt: [f64; 3],
        vec: [f64; 3],
        amount: f64,
    },
    VertexHigher {
        pt: [f64; 3],
        vtol: f64,
    },
    VertexSame {
        pt: [f64; 3],
        vtol: f64,
    },
    EdgeHigher {
        edge: EdgeIn,
    },
    EdgeSame {
        edge: EdgeIn,
    },
    FaceSame {
        surface: SurfIn,
        ftol: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    sweep: SweepIn,
    #[serde(flatten)]
    op: OpIn,
}

// ---- C++ ------------------------------------------------------------------------------

// ---- Rust -----------------------------------------------------------------------------

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } | SolidError::PortInvariant { .. } => SM_ERR_INVALID_INPUT,
        e if solidcore::error::upstream_status(e).is_some() => {
            f64::from(solidcore::error::upstream_status(e).unwrap_or(-1))
        }
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, a harness problem, or the status record.
enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Unported` anywhere defers the case.
fn defer(e: &SolidError) -> Result<(), Fail> {
    match Output::unported(e) {
        Some(o) => Err(Fail::Deferred(Box::new(o))),
        None => Ok(()),
    }
}

fn rust_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    let bad = |e: SolidError| harness(format!("curve: {e}"));
    Ok(match c {
        CurveIn::Nurb { p, cpts, knots } => {
            let n = cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m = knots
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no knots"))?;
            let mut cur = allocate_nurb_curve(n, *p, m).map_err(bad)?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(knots);
            Curve::from(BSplineCurve::from_nurb(cur, 3, false))
        }
        CurveIn::Bline { a, b } => {
            Curve::from(BSplineCurve::create_line_segment(3, v3(*a), v3(*b)).map_err(bad)?)
        }
        CurveIn::Line { a, b, ivl } => {
            let ivl = ivl.map(|[t0, t1]| Extent1d::from_min_max(t0, t1));
            Curve::from(Line::create_line_segment(3, v3(*a), v3(*b), ivl.as_ref()).map_err(bad)?)
        }
        CurveIn::Circle { c, x, y, d, r, io } => {
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: *io,
            };
            Curve::from(
                Ellipse::new_circle(
                    k,
                    v3(*c),
                    v3(*x),
                    v3(*y),
                    Extent1d::from_min_max(d[0], d[1]),
                    *r,
                    &opts,
                )
                .map_err(bad)?,
            )
        }
        CurveIn::Ellipse {
            c,
            x,
            y,
            d,
            rx,
            ry,
            io,
        } => {
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: *io,
            };
            Curve::from(
                Ellipse::new(
                    k,
                    v3(*c),
                    v3(*x),
                    v3(*y),
                    Extent1d::from_min_max(d[0], d[1]),
                    *rx,
                    *ry,
                    &opts,
                )
                .map_err(bad)?,
            )
        }
        CurveIn::Hermite { p1, d1, p2, d2 } => {
            Curve::from(HermiteCurve::new(v3(*p1), v3(*d1), v3(*p2), v3(*d2), 3))
        }
    })
}

fn rust_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    Ok(match s {
        SurfIn::Nurb { p, q, net, u, v } => {
            let net: Vec<Vec<CPoint>> = net
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                        .collect()
                })
                .collect();
            let nl = NlSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Surface::from(BSplineSurface::from_nurb(nl, false))
        }
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            dom,
        } => Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &Extent2d::from_bounds(dom[0], dom[1], dom[2], dom[3]),
        )),
    })
}

/// A B-rep with the sweep's tolerance (`SmBrep()`, then `SetTolerance(brep_tol)` when
/// positive), a region and a shell.
fn new_brep(k: &Kernel, s: &SweepIn) -> Result<(Brep, solidcore::topology::ShellId), HarnessError> {
    let hb = |e: SolidError| harness(format!("brep: {e}"));
    let mut b = Brep::new();
    if s.brep_tol > 0.0 {
        b.set_tolerance(zone_tol3d(k, s.brep_tol));
    }
    let r = b
        .add_region_record(Region {
            in_brep: true,
            ..Region::default()
        })
        .map_err(hb)?;
    let shell = b
        .add_shell_record(Shell {
            faces: Vec::new(),
            region: Some(r),
            ..Shell::default()
        })
        .map_err(hb)?;
    Ok((b, shell))
}

/// The wire edge of `e` as `MakeShellVertex` + `MakeWireEdgeVertex` (or `MakeWireEdge`
/// to the same vertex) link it: two shell edge uses, the primary one at the first
/// vertex with the edge's orientation, its mate at the other vertex. Returns the edge
/// and the first vertex.
fn rust_edge(
    k: &Kernel,
    b: &mut Brep,
    shell: solidcore::topology::ShellId,
    e: &EdgeIn,
) -> Result<(EdgeId, VertexId), HarnessError> {
    let hb = |e: SolidError| harness(format!("edge: {e}"));
    let curve = rust_curve(k, &e.curve)?;
    let [t0, t1] = e.t;
    let (ta, tb) = if e.opposite { (t1, t0) } else { (t0, t1) };
    let pa = curve.evaluate_point(k, ta).map_err(hb)? + v3(e.off0);
    let pb = curve.evaluate_point(k, tb).map_err(hb)? + v3(e.off1);
    let v0 = b
        .add_vertex_record(Vertex {
            point: pa,
            tolerance: zone_tol3d(k, e.vtol[0]),
            ..Vertex::default()
        })
        .map_err(hb)?;
    let v1 = if e.closed {
        v0
    } else {
        b.add_vertex_record(Vertex {
            point: pb,
            tolerance: zone_tol3d(k, e.vtol[1]),
            ..Vertex::default()
        })
        .map_err(hb)?
    };
    let edge_id = b.add_edge_record(Edge::new([v0, v1])).map_err(hb)?;
    let (o0, o1) = if e.opposite {
        (OrientType::Opposite, OrientType::Same)
    } else {
        (OrientType::Same, OrientType::Opposite)
    };
    let mut uses = Vec::new();
    for (v, o) in [(v0, o0), (v1, o1)] {
        let eu = b
            .add_edgeuse_record(Edgeuse {
                edge: Some(edge_id),
                owner: EdgeuseOwner::Shell(Some(shell)),
                orientation: o,
                ..Edgeuse::default()
            })
            .map_err(hb)?;
        let vu = b
            .add_vertexuse_record(Vertexuse {
                vertex: Some(v),
                owner: VertexuseOwner::Edgeuse(Some(eu)),
                ..Vertexuse::default()
            })
            .map_err(hb)?;
        b.edgeuse_mut(eu).map_err(hb)?.vertexuse = Some(vu);
        uses.push(eu);
    }
    let out = b.edge_mut(edge_id).map_err(hb)?;
    out.edgeuses = uses;
    out.interval = Extent1d::from_min_max(t0, t1);
    out.tolerance = zone_tol3d(k, e.etol);
    out.curve = Some(curve);
    Ok((edge_id, v0))
}

fn put3(o: &mut Vec<f64>, p: Vec3) {
    o.extend([p.x, p.y, p.z]);
}

fn put_frame(o: &mut Vec<f64>, a: &Axis2Placement) {
    put3(o, a.origin());
    put3(o, a.x_axis());
    put3(o, a.y_axis());
}

/// Evaluates `t` as the shim's `nat.Evaluate(f)`.
fn ivl_eval(a: f64, b: f64, f: f64) -> f64 {
    Extent1d::from_min_max(a, b).evaluate(f)
}

/// As the shim's `put_curve`.
fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: &Curve, uv: bool) -> Result<(), Fail> {
    o.push(c.type_code() as f64);
    let nat = c.natural_interval();
    o.push(nat.min());
    o.push(nat.max());
    for i in 0..N_CURVE_SAMPLES {
        let t = ivl_eval(
            nat.min(),
            nat.max(),
            i as f64 / (N_CURVE_SAMPLES - 1) as f64,
        );
        let (st, p) = match c.evaluate_point(k, t) {
            Ok(p) => (0.0, p),
            Err(e) => {
                defer(&e)?;
                (status(&e), Vec3::ZERO)
            }
        };
        o.extend([st, p.x, p.y]);
        if !uv {
            o.push(p.z);
        }
    }
    Ok(())
}

/// As the shim's `put_surface`.
fn put_surface(k: &Kernel, o: &mut Vec<f64>, s: &Surface) -> Result<(), Fail> {
    o.push(s.type_code() as f64);
    let d = s.natural_uv_domain_with_nurb(k).map_err(|e| HarnessError::new(e.to_string()))?;
    let (mn, mx) = (d.min(), d.max());
    o.extend([mn.x, mn.y, mx.x, mx.y]);
    for i in 0..N_SURF_SAMPLES {
        for j in 0..N_SURF_SAMPLES {
            let fu = i as f64 / (N_SURF_SAMPLES - 1) as f64;
            let fv = j as f64 / (N_SURF_SAMPLES - 1) as f64;
            let uv = Point2::new(mn.x + fu * (mx.x - mn.x), mn.y + fv * (mx.y - mn.y));
            let (st, p) = match s.evaluate_point(k, uv) {
                Ok(p) => (0.0, p),
                Err(e) => {
                    defer(&e)?;
                    (status(&e), Vec3::ZERO)
                }
            };
            o.push(st);
            put3(o, p);
        }
    }
    Ok(())
}

/// As the shim's `put_fixture`.
fn put_fixture(o: &mut Vec<f64>, b: &Brep, e: EdgeId, first: VertexId) -> Result<(), Fail> {
    let hb = |e: SolidError| Fail::Harness(harness(format!("fixture: {e}")));
    let sv = edge::start_vertex(b, e)
        .map_err(hb)?
        .ok_or_else(|| harness("no start vertex"))?;
    let ov = edge::other_vertex(b, e, Some(sv))
        .map_err(hb)?
        .ok_or_else(|| harness("no other vertex"))?;
    o.push(b.tolerance());
    o.push(flag(sv == first));
    o.push(flag(edge::vertex(b, e).map_err(hb)? == Some(first)));
    o.push(flag(edge::is_closed(b, e).map_err(hb)?));
    let vs = b.vertex(sv).map_err(hb)?;
    let vo = b.vertex(ov).map_err(hb)?;
    put3(o, vs.point);
    put3(o, vo.point);
    o.push(vs.tolerance);
    o.push(vo.tolerance);
    o.push(edge::tolerance(b, e).map_err(hb)?);
    Ok(())
}

fn orient_code(o: OrientType) -> f64 {
    match o {
        OrientType::Unknown => 0.0,
        OrientType::Same => 1.0,
        OrientType::Opposite => 2.0,
    }
}

fn idx(i: Option<usize>) -> f64 {
    i.map_or(-1.0, |i| i as f64)
}

fn run_rust_op(i: &Inputs, o: &mut Vec<f64>) -> Result<(), Fail> {
    let k = Kernel::default();
    let s = &i.sweep;
    let mut g = RotationalSweepGeometry::new(v3(s.base), v3(s.axis), s.angle, s.eps);
    g.base.set_make_analytics(s.analytics);

    match &i.op {
        OpIn::IsClosed { repeats } => match g.is_sweep_closed(*repeats) {
            Ok(c) => o.extend([0.0, flag(c)]),
            Err(e) => {
                defer(&e)?;
                o.extend([status(&e), 0.0]);
            }
        },
        OpIn::StartCs { pt } | OpIn::EndCs { pt } => {
            let r = if matches!(i.op, OpIn::StartCs { .. }) {
                g.start_coord_system(v3(*pt))
            } else {
                g.end_coord_system(v3(*pt))
            };
            match r {
                Ok(cs) => {
                    o.push(0.0);
                    put_frame(o, &cs);
                }
                Err(e) => {
                    defer(&e)?;
                    o.push(status(&e));
                }
            }
        }
        OpIn::Transform => {
            o.push(0.0);
            put_frame(o, &g.sweep_transform());
        }
        OpIn::Copy => {
            let c = g.copy();
            o.push(0.0);
            put3(o, c.base_point());
            put3(o, c.axis());
            o.extend([c.angle(), c.eps(), flag(c.base.make_analytics)]);
        }
        OpIn::CheckCurve {
            curve,
            t,
            tol,
            pt,
            vec,
            amount,
        } => {
            let c = rust_curve(&k, curve)?;
            match g.check_curve_sweep(
                &k,
                &c,
                &Extent1d::from_min_max(t[0], t[1]),
                *tol,
                v3(*pt),
                v3(*vec),
                *amount,
            ) {
                Ok(st) => o.extend([0.0, f64::from(st.code())]),
                Err(e) => {
                    defer(&e)?;
                    o.push(status(&e));
                }
            }
        }
        OpIn::VertexHigher { pt, vtol } | OpIn::VertexSame { pt, vtol } => {
            let (mut b, _) = new_brep(&k, s)?;
            let v = b
                .add_vertex_record(Vertex {
                    point: v3(*pt),
                    tolerance: zone_tol3d(&k, *vtol),
                    ..Vertex::default()
                })
                .map_err(|e| harness(format!("vertex: {e}")))?;
            o.push(zone_tol3d(&k, *vtol));
            if matches!(i.op, OpIn::VertexHigher { .. }) {
                match g.vertex_sweep_higher(&b, v) {
                    Ok(r) => {
                        o.push(0.0);
                        o.push(flag(r.is_some()));
                        if let Some((c, tol)) = r {
                            o.push(tol);
                            put_curve(&k, o, &c, false)?;
                        }
                    }
                    Err(e) => {
                        defer(&e)?;
                        o.push(status(&e));
                    }
                }
            } else {
                match g.vertex_sweep_same(&b, v) {
                    Ok((p, tol)) => {
                        o.push(0.0);
                        put3(o, p);
                        o.push(tol);
                    }
                    Err(e) => {
                        defer(&e)?;
                        o.push(status(&e));
                    }
                }
            }
        }
        OpIn::EdgeSame { edge: ein } => {
            let (mut b, shell) = new_brep(&k, s)?;
            let (e, first) = rust_edge(&k, &mut b, shell, ein)?;
            put_fixture(o, &b, e, first)?;
            match g.edge_sweep_same(&k, &b, e) {
                Ok((c, tol)) => {
                    o.extend([0.0, 1.0, tol]);
                    put_curve(&k, o, &c, false)?;
                }
                Err(err) => {
                    defer(&err)?;
                    o.push(status(&err));
                }
            }
        }
        OpIn::EdgeHigher { edge: ein } => {
            let (mut b, shell) = new_brep(&k, s)?;
            let (e, first) = rust_edge(&k, &mut b, shell, ein)?;
            put_fixture(o, &b, e, first)?;
            match g.edge_sweep_higher(&k, &b, e) {
                Ok(r) => {
                    o.push(0.0);
                    o.push(flag(r.surface.is_some()));
                    if let Some(srf) = &r.surface {
                        put_surface(&k, o, srf)?;
                    }
                    o.extend([
                        r.face_tol,
                        r.start_curve_tol,
                        r.edge_tol,
                        r.start_vert_edge_tol,
                        r.end_vert_edge_tol,
                    ]);
                    put3(o, r.normal.unwrap_or(Vec3::ZERO));
                    o.extend([
                        idx(r.start_curve),
                        idx(r.far_end_curve),
                        idx(r.start_vert_curve),
                        idx(r.end_vert_curve),
                    ]);
                    o.push(r.trim_curves.len() as f64);
                    for c in &r.trim_curves {
                        put_curve(&k, o, c, false)?;
                    }
                    o.push(r.uv_trim_curves.len() as f64);
                    for c in &r.uv_trim_curves {
                        put_curve(&k, o, &Curve::from(c.clone()), true)?;
                    }
                    o.push(r.orients.len() as f64);
                    o.extend(r.orients.iter().map(|x| orient_code(*x)));
                }
                Err(err) => {
                    defer(&err)?;
                    o.push(status(&err));
                }
            }
        }
        OpIn::FaceSame { surface, ftol } => {
            let (mut b, shell) = new_brep(&k, s)?;
            let srf = rust_surface(surface)?;
            let dom = srf.natural_uv_domain_with_nurb(&k).map_err(|e| HarnessError::new(e.to_string()))?;
            let mut face = Face::new(shell);
            face.surface = Some(srf);
            face.uv_domain = dom;
            face.tolerance = zone_tol3d(&k, *ftol);
            let f = b
                .add_face_record(face)
                .map_err(|e| harness(format!("face: {e}")))?;
            o.push(zone_tol3d(&k, *ftol));
            match g.face_sweep_same(&k, &b, f) {
                Ok(r) => {
                    o.push(0.0);
                    o.push(flag(r.is_some()));
                    if let Some((srf, tol)) = r {
                        o.push(tol);
                        put_surface(&k, o, &srf)?;
                    }
                }
                Err(e) => {
                    defer(&e)?;
                    o.push(status(&e));
                }
            }
        }
    }
    Ok(())
}

fn run_rust(i: &Inputs) -> Result<Output, HarnessError> {
    let mut o = Vec::new();
    match run_rust_op(i, &mut o) {
        Ok(()) => Ok(Output::new().values("out", o)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(h)) => Err(h),
    }
}

// ---- generation -----------------------------------------------------------------------

fn pick<T: Copy>(rng: &mut Rng, items: &[T], default: T) -> T {
    rng.choose(items).copied().unwrap_or(default)
}

fn add(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[0] + b[0], a[1] + b[1], a[2] + b[2]]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

fn norm(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    if l == 0.0 {
        [0.0, 0.0, 1.0]
    } else {
        scale3(a, 1.0 / l)
    }
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The local frame of a sweep axis: the point of the axis, its unit direction and two
/// unit vectors perpendicular to it.
struct Frame {
    o: [f64; 3],
    d: [f64; 3],
    u: [f64; 3],
    w: [f64; 3],
}

impl Frame {
    fn new(o: [f64; 3], axis: [f64; 3]) -> Self {
        let d = norm(axis);
        let r = if d[0].abs() < 0.9 {
            [1.0, 0.0, 0.0]
        } else {
            [0.0, 1.0, 0.0]
        };
        let u = norm(cross(d, r));
        let w = cross(d, u);
        Self { o, d, u, w }
    }

    /// The point at radius `r`, angle `th` (radians) and height `h`.
    fn at(&self, r: f64, th: f64, h: f64) -> [f64; 3] {
        let radial = add(scale3(self.u, r * th.cos()), scale3(self.w, r * th.sin()));
        add(add(self.o, radial), scale3(self.d, h))
    }

    /// The radial direction at angle `th`.
    fn radial(&self, th: f64) -> [f64; 3] {
        add(scale3(self.u, th.cos()), scale3(self.w, th.sin()))
    }
}

fn gen_sweep(rng: &mut Rng, scale: f64) -> SweepIn {
    let base = if rng.chance(0.3) {
        [0.0; 3]
    } else {
        scale3(rng.point3(-10.0, 10.0), scale)
    };
    let axis = match rng.below(6) {
        0 => [0.0, 0.0, 1.0],
        1 => [0.0, -1.0, 0.0],
        2 => [1.0, 0.0, 0.0],
        3 => scale3(norm(rng.point3(-1.0, 1.0)), rng.uniform(0.01, 5.0)),
        4 => [-1.0, 1.0, 0.0],
        _ => rng.point3(-1.0, 1.0),
    };
    let angle = if rng.chance(0.7) {
        pick(
            rng,
            &[
                30.0, 36.0, 45.0, 90.0, 180.0, 270.0, 360.0, 359.9999, 360.0, 0.5, 1e-3,
            ],
            90.0,
        )
    } else {
        rng.uniform(1.0, 360.0)
    };
    let eps = pick(rng, &[1e-6, 1e-6, 1e-5, 1e-3, 0.0, 1e-9, 0.05], 1e-6) * scale.max(1e-3);
    SweepIn {
        base,
        axis,
        angle,
        eps,
        analytics: rng.chance(0.2),
        brep_tol: if rng.chance(0.75) {
            0.0
        } else {
            pick(rng, &[1e-4, 1e-8, 1e-3], 1e-4)
        },
    }
}

fn clamped_knots(rng: &mut Rng, p: usize, n: usize, a: f64, b: f64) -> Vec<f64> {
    let mut inner: Vec<f64> = (0..n.saturating_sub(p))
        .map(|_| rng.uniform(a + 0.05 * (b - a), b - 0.05 * (b - a)))
        .collect();
    inner.sort_by(f64::total_cmp);
    let mut k = vec![a; p + 1];
    k.extend(inner);
    k.extend(vec![b; p + 1]);
    k
}

/// A curve near the sweep axis of `f`, its parameter range and whether it is closed.
fn gen_curve(rng: &mut Rng, f: &Frame, scale: f64) -> (CurveIn, [f64; 2], bool) {
    let th = rng.uniform(0.0, std::f64::consts::TAU);
    let s = scale;
    match rng.below(20) {
        // a planar segment through two (r, h) points: cone, cylinder, annulus, from or
        // on the axis, crossing it
        0..=6 => {
            let (r1, h1, r2, h2) = match rng.below(9) {
                0 => (0.0, 0.0, rng.uniform(0.5, 3.0), 0.0), // from axis, perpendicular
                1 => (rng.uniform(0.5, 2.0), 0.0, rng.uniform(2.5, 4.0), 0.0), // annulus
                2 => {
                    let r = rng.uniform(0.5, 3.0);
                    (r, 0.0, r, rng.uniform(0.5, 3.0)) // cylinder
                }
                3 => (0.0, 0.0, 0.0, rng.uniform(0.5, 3.0)), // on the axis
                4 => (
                    -rng.uniform(0.5, 2.0),
                    0.0,
                    rng.uniform(0.5, 2.0),
                    rng.uniform(0.0, 1.0),
                ), // crossing
                5 => (0.0, 0.0, rng.uniform(0.5, 3.0), rng.uniform(0.5, 3.0)), // cone apex on axis
                6 => (rng.uniform(1e-7, 1e-5), 0.0, rng.uniform(0.5, 3.0), 0.0), // near the axis
                7 => (
                    rng.uniform(0.5, 3.0),
                    0.0,
                    rng.uniform(0.5, 3.0),
                    rng.uniform(1e-7, 1e-5),
                ), // almost perpendicular
                _ => (
                    rng.uniform(0.2, 3.0),
                    rng.uniform(-2.0, 2.0),
                    rng.uniform(0.2, 3.0),
                    rng.uniform(-2.0, 2.0),
                ),
            };
            let a = f.at(r1 * s, th, h1 * s);
            let b = f.at(r2 * s, th, h2 * s);
            match rng.below(4) {
                0 | 1 => (CurveIn::Bline { a, b }, [0.0, 1.0], false),
                2 => (CurveIn::Line { a, b, ivl: None }, [0.0, 1.0], false),
                _ => {
                    let t0 = rng.uniform(-5.0, 5.0);
                    let t1 = t0 + rng.uniform(0.5, 10.0);
                    (
                        CurveIn::Line {
                            a,
                            b,
                            ivl: Some([t0, t1]),
                        },
                        [t0, t1],
                        false,
                    )
                }
            }
        }
        // a skew segment (hyperboloid)
        7 | 8 => {
            let a = f.at(rng.uniform(0.5, 2.0) * s, th, rng.uniform(-1.0, 1.0) * s);
            let b = f.at(
                rng.uniform(0.5, 2.0) * s,
                th + rng.uniform(0.2, 1.5),
                rng.uniform(-1.0, 1.0) * s,
            );
            (CurveIn::Bline { a, b }, [0.0, 1.0], false)
        }
        // a circular arc in a plane containing the axis: sphere (centre on the axis),
        // torus, or a full circle
        9..=13 => {
            let on_axis = rng.chance(0.4);
            let c_r = if on_axis { 0.0 } else { rng.uniform(1.5, 4.0) };
            let c = f.at(c_r * s, th, rng.uniform(-1.0, 1.0) * s);
            let r = if on_axis {
                rng.uniform(0.5, 2.0)
            } else {
                rng.uniform(0.2, c_r - 0.1)
            } * s;
            let x = f.radial(th);
            let y = f.d;
            let (d, closed) = if on_axis {
                match rng.below(3) {
                    0 => ([-90.0, 90.0], false),
                    1 => ([-60.0, 45.0], false),
                    _ => ([0.0, 90.0], false),
                }
            } else if rng.chance(0.3) {
                ([0.0, 360.0], true)
            } else {
                let d0 = rng.uniform(-180.0, 90.0);
                ([d0, d0 + rng.uniform(10.0, 200.0)], false)
            };
            let io = rng.chance(0.2);
            if rng.chance(0.75) {
                (CurveIn::Circle { c, x, y, d, r, io }, d, closed)
            } else {
                let ry = r * rng.uniform(0.3, 0.9);
                (
                    CurveIn::Ellipse {
                        c,
                        x,
                        y,
                        d,
                        rx: r,
                        ry,
                        io,
                    },
                    d,
                    closed,
                )
            }
        }
        // a circle in a plane perpendicular to the axis (a surface along its length)
        14 => {
            let c = f.at(rng.uniform(0.0, 1.0) * s, th, rng.uniform(-1.0, 1.0) * s);
            let d0 = rng.uniform(0.0, 90.0);
            let d = [d0, d0 + rng.uniform(20.0, 120.0)];
            (
                CurveIn::Circle {
                    c,
                    x: f.u,
                    y: f.w,
                    d,
                    r: rng.uniform(0.5, 2.0) * s,
                    io: false,
                },
                d,
                false,
            )
        }
        // a B-spline near the axis (planar or not)
        15..=18 => {
            let p = 1 + rng.below(3) as usize;
            let n = p + rng.below(4) as usize;
            let planar = rng.chance(0.5);
            let rational = rng.chance(0.3);
            let from_axis = rng.chance(0.2);
            let cpts = (0..=n)
                .map(|i| {
                    let r = if from_axis && i == 0 {
                        0.0
                    } else {
                        rng.uniform(0.3, 3.0)
                    };
                    let t = if planar {
                        th
                    } else {
                        th + rng.uniform(-0.8, 0.8)
                    };
                    let q = f.at(r * s, t, (i as f64 + rng.uniform(-0.3, 0.3)) * s);
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [q[0] * w, q[1] * w, q[2] * w, w]
                    } else {
                        [q[0], q[1], q[2], NOW]
                    }
                })
                .collect();
            let (a, b) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                let a = rng.uniform(-3.0, 3.0);
                (a, a + rng.uniform(0.5, 4.0))
            };
            let knots = clamped_knots(rng, p, n, a, b);
            (CurveIn::Nurb { p, cpts, knots }, [a, b], false)
        }
        // a Hermite curve (not a B-spline: SM_ERR)
        _ => {
            let p1 = f.at(s, th, 0.0);
            let p2 = f.at(2.0 * s, th, s);
            (
                CurveIn::Hermite {
                    p1,
                    d1: scale3(f.d, s),
                    p2,
                    d2: scale3(f.radial(th), s),
                },
                [0.0, 1.0],
                false,
            )
        }
    }
}

/// The natural interval of a generated curve (`range` when it cannot be built).
fn natural_range(c: &CurveIn, range: [f64; 2]) -> [f64; 2] {
    rust_curve(&Kernel::default(), c).map_or(range, |c| {
        let n = c.natural_interval();
        [n.min(), n.max()]
    })
}

fn gen_edge(rng: &mut Rng, f: &Frame, scale: f64, eps: f64) -> EdgeIn {
    let (curve, range, closed) = gen_curve(rng, f, scale);
    let [a, b] = natural_range(&curve, range);
    let t = if closed || rng.chance(0.6) {
        [a, b]
    } else {
        let t0 = a + (b - a) * rng.uniform(0.0, 0.4);
        let t1 = b - (b - a) * rng.uniform(0.0, 0.4);
        [t0, t1]
    };
    let off = |rng: &mut Rng| -> [f64; 3] {
        if rng.chance(0.85) {
            [0.0; 3]
        } else {
            scale3(
                norm(rng.point3(-1.0, 1.0)),
                pick(rng, &[1e-9, 1e-7, 0.5], 1e-7) * eps.max(1e-6),
            )
        }
    };
    let tols = [1e-6, 1e-6, 1e-5, 1e-3, 0.0, 1e-8];
    EdgeIn {
        curve,
        t,
        opposite: rng.chance(0.3),
        closed,
        off0: off(rng),
        off1: if closed { [0.0; 3] } else { off(rng) },
        etol: pick(rng, &tols, 1e-6) * scale.max(1e-3),
        vtol: [
            pick(rng, &tols, 1e-6) * scale.max(1e-3),
            pick(rng, &tols, 1e-6) * scale.max(1e-3),
        ],
    }
}

fn gen_surface(rng: &mut Rng, scale: f64) -> SurfIn {
    if rng.chance(0.5) {
        let x = rng.point3(-1.0, 1.0);
        let mut y = rng.point3(-1.0, 1.0);
        if cross(x, y).iter().all(|c| c.abs() < 1e-3) {
            y = cross(x, [0.3, 0.2, 0.9]);
        }
        let u0 = rng.uniform(-2.0, 1.0);
        let v0 = rng.uniform(-2.0, 1.0);
        SurfIn::Plane {
            o: scale3(rng.point3(-5.0, 5.0), scale),
            x,
            y,
            scale: [rng.uniform(0.5, 2.0) * scale, rng.uniform(0.5, 2.0) * scale],
            dom: [
                u0,
                v0,
                u0 + rng.uniform(0.5, 3.0),
                v0 + rng.uniform(0.5, 3.0),
            ],
        }
    } else {
        let p = 1 + rng.below(2) as usize;
        let q = 1 + rng.below(2) as usize;
        let n = p + rng.below(2) as usize;
        let m = q + rng.below(2) as usize;
        let planar = rng.chance(0.4);
        let net = (0..=n)
            .map(|i| {
                (0..=m)
                    .map(|j| {
                        let z = if planar { 0.0 } else { rng.uniform(-1.0, 1.0) };
                        [
                            (i as f64 + rng.uniform(-0.2, 0.2)) * scale,
                            (j as f64 + rng.uniform(-0.2, 0.2)) * scale,
                            z * scale,
                            NOW,
                        ]
                    })
                    .collect()
            })
            .collect();
        SurfIn::Nurb {
            p,
            q,
            net,
            u: clamped_knots(rng, p, n, 0.0, 1.0),
            v: clamped_knots(rng, q, m, 0.0, 1.0),
        }
    }
}

fn gen_case(rng: &mut Rng) -> Inputs {
    let scale = pick(rng, &[1.0, 1.0, 1.0, 1e-3, 1e3, 1e5], 1.0);
    let sweep = gen_sweep(rng, scale);
    let f = Frame::new(sweep.base, sweep.axis);
    let near_axis = |rng: &mut Rng, f: &Frame| -> [f64; 3] {
        let r = pick(rng, &[0.0, 1e-9, 1e-7, 1e-5, 1.0, 3.0], 1.0) * scale;
        f.at(r, rng.uniform(0.0, 6.0), rng.uniform(-2.0, 2.0) * scale)
    };
    let op = match rng.below(100) {
        0..=39 => OpIn::EdgeHigher {
            edge: gen_edge(rng, &f, scale, sweep.eps),
        },
        40..=54 => OpIn::EdgeSame {
            edge: gen_edge(rng, &f, scale, sweep.eps),
        },
        55..=64 => {
            let (curve, range, _) = gen_curve(rng, &f, scale);
            let [a, b] = natural_range(&curve, range);
            let t = if rng.chance(0.6) {
                [a, b]
            } else {
                [a + 0.3 * (b - a), b - 0.2 * (b - a)]
            };
            OpIn::CheckCurve {
                curve,
                t,
                tol: pick(rng, &[1e-6, 1e-4, 1e-9, 0.0], 1e-6) * scale.max(1e-3),
                pt: sweep.base,
                vec: if sweep.axis.iter().all(|c| c.abs() < 1e-6) {
                    [0.0, 0.0, 1.0]
                } else {
                    sweep.axis
                },
                amount: sweep.angle,
            }
        }
        65..=72 => OpIn::VertexHigher {
            pt: near_axis(rng, &f),
            vtol: pick(rng, &[1e-6, 1e-3, 0.0, 1e-8], 1e-6) * scale.max(1e-3),
        },
        73..=79 => OpIn::VertexSame {
            pt: near_axis(rng, &f),
            vtol: pick(rng, &[1e-6, 1e-3, 0.0, 1.0], 1e-6) * scale.max(1e-3),
        },
        80..=87 => OpIn::FaceSame {
            surface: gen_surface(rng, scale),
            ftol: pick(rng, &[1e-6, 1e-3, 0.0], 1e-6),
        },
        88..=91 => OpIn::StartCs {
            pt: near_axis(rng, &f),
        },
        92..=95 => OpIn::EndCs {
            pt: near_axis(rng, &f),
        },
        96 => OpIn::IsClosed {
            repeats: rng.below(13),
        },
        97 => OpIn::Transform,
        _ => OpIn::Copy,
    };
    Inputs { sweep, op }
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "sweep.rotational_sweep_geometry"
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
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
}


/// Proof of the rows `revolve_analytic_profile_faces` moved (tangency-labs/solidcraft#14425):
/// a line or circle coplanar with the axis now sweeps a cylinder, cone, sphere or torus
/// parameterised by angle and meridian instead of a B-spline surface of revolution, so the
/// samples at fixed fractions of the domain sit elsewhere on the same surface. Each row is
/// checked against the closed form of the solid of revolution of its profile, in interval
/// arithmetic from the exact inputs, within the kernel's declared accuracy of an analytic
/// revolution face: `FIT_REL (1 + size)`, the largest distance by which it may miss the
/// B-spline surface it replaces (`size` the largest coordinate of the profile's ends and
/// the base point, as the kernel takes it).
#[cfg(test)]
mod revolve_analytic_rows {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::indexing_slicing
    )]
    use super::*;
    use crate::interval::{sin_cos_deg, Iv};
    use solidcore::geom::geom_utility::p1::line_point_distance;
    use solidcore::nurbs::b_spline_curve_n_lib::TrimMode;
    use solidcore::surface::surface::p2::{CONE_TYPE, CYLINDER_TYPE, SPHERE_TYPE, TORUS_TYPE};
    use solidcore::sweep::analytic_revolution::FIT_REL;

    /// The moved `EdgeHigher` rows.
    const ROWS: [&str; 9] = [
        "gen0143", "gen0273", "gen0293", "gen0301", "gen0306", "gen0310", "gen0342", "gen0367",
        "gen0384",
    ];

    /// Record layout of an `EdgeHigher` row that sweeps a surface.
    const FIXTURE: usize = 13;
    const SAMPLES: usize = FIXTURE + 7;
    const AFTER_SAMPLES: usize = SAMPLES + 4 * N_SURF_SAMPLES * N_SURF_SAMPLES;
    const NORMAL: usize = AFTER_SAMPLES + 5;
    const IDX: usize = NORMAL + 3;
    const TRIMS: usize = IDX + 4;

    /// A vector of enclosures.
    type V = [Iv; 3];

    fn iv(p: Vec3) -> V {
        [Iv::exact(p.x), Iv::exact(p.y), Iv::exact(p.z)]
    }

    fn sub(a: V, b: V) -> V {
        [a[0].sub(b[0]), a[1].sub(b[1]), a[2].sub(b[2])]
    }

    fn scale(a: V, s: Iv) -> V {
        [a[0].mul(s), a[1].mul(s), a[2].mul(s)]
    }

    fn dot(a: V, b: V) -> Iv {
        a[0].mul(b[0]).add(a[1].mul(b[1])).add(a[2].mul(b[2]))
    }

    fn cross(a: V, b: V) -> V {
        [
            a[1].mul(b[2]).sub(a[2].mul(b[1])),
            a[2].mul(b[0]).sub(a[0].mul(b[2])),
            a[0].mul(b[1]).sub(a[1].mul(b[0])),
        ]
    }

    fn norm(a: V) -> Iv {
        dot(a, a).sqrt()
    }

    fn sq(x: Iv) -> Iv {
        x.mul(x)
    }

    /// The enclosed distance between two recorded points.
    fn dist(p: Vec3, q: Vec3) -> Iv {
        norm(sub(iv(p), iv(q)))
    }

    /// The profile's carrier folded into the half-plane frame: `h` along the axis from
    /// the base point, `x` along `e0` (the plane through the axis that holds the profile).
    #[derive(Debug, Clone, Copy)]
    enum Carrier {
        /// The line through `(h, x)` points `p` and `q`.
        Line { p: [Iv; 2], q: [Iv; 2] },
        /// The circle about `c` of radius `r`.
        Circle { c: [Iv; 2], r: Iv },
    }

    /// A point's coordinates in the frame: along the axis, along `e0`, along `e1`, and
    /// its distance from the axis.
    #[derive(Debug, Clone, Copy)]
    struct Hxy {
        h: Iv,
        x: Iv,
        y: Iv,
        rho: Iv,
    }

    /// The exact frame of the revolution, enclosed: the base point, the axis `a`, the
    /// radial `e0` of the profile's plane (perpendicular to `a`) and `e1 = a x e0`, each
    /// with its length, so that the enclosures hold the coordinates in the orthonormal
    /// frame of the exact inputs.
    struct Revolution {
        base: V,
        a: V,
        la: Iv,
        e0: V,
        l0: Iv,
        e1: V,
        l1: Iv,
        sin_cos: (Iv, Iv),
        reflex: bool,
        carrier: Carrier,
        /// The largest distance of the profile from the plane of `a` and `e0`.
        off_plane: f64,
    }

    impl Revolution {
        fn new(s: &SweepIn, c: &CurveIn) -> Self {
            /// The profile's carrier in 3D: a line through two points, or a circle
            /// (centre, normal, radius).
            enum C3 {
                Line(V, V),
                Circle(V, V, f64),
            }
            let base = iv(v3(s.base));
            let a = iv(v3(s.axis));
            let la = norm(a);
            let rej = |p: V| {
                let w = sub(p, base);
                sub(w, scale(a, dot(w, a).div(dot(a, a))))
            };
            let point = |q: [f64; 4]| {
                if q[3] == NOW {
                    iv(Vec3::new(q[0], q[1], q[2]))
                } else {
                    let w = Iv::exact(q[3]);
                    [0, 1, 2].map(|i| Iv::exact(q[i]).div(w))
                }
            };
            let c3 = match c {
                CurveIn::Line { a, b, .. } | CurveIn::Bline { a, b } => {
                    C3::Line(iv(v3(*a)), iv(v3(*b)))
                }
                CurveIn::Nurb { p: 1, cpts, .. } if cpts.len() == 2 => {
                    C3::Line(point(cpts[0]), point(cpts[1]))
                }
                CurveIn::Circle { c, x, y, r, .. } => {
                    C3::Circle(iv(v3(*c)), cross(iv(v3(*x)), iv(v3(*y))), *r)
                }
                CurveIn::Ellipse { c, x, y, rx, ry, .. } if rx == ry => {
                    C3::Circle(iv(v3(*c)), cross(iv(v3(*x)), iv(v3(*y))), *rx)
                }
                other => panic!("no closed-form carrier for {other:?}"),
            };
            // e0: radial in the plane through the axis that holds the profile
            let e0 = match &c3 {
                C3::Line(p, q) => {
                    let (rp, rq) = (rej(*p), rej(*q));
                    if norm(rp).hi >= norm(rq).hi { rp } else { rq }
                }
                C3::Circle(_, n, _) => cross(*n, a),
            };
            let l0 = norm(e0);
            let e1 = cross(a, e0);
            let l1 = norm(e1);
            let (sin, cos) = sin_cos_deg(s.angle);
            let mut rev = Self {
                base,
                a,
                la,
                e0,
                l0,
                e1,
                l1,
                sin_cos: (sin, cos),
                reflex: s.angle > 180.0,
                carrier: Carrier::Circle {
                    c: [Iv::ZERO; 2],
                    r: Iv::ZERO,
                },
                off_plane: 0.0,
            };
            let (carrier, off_plane) = match c3 {
                C3::Line(p, q) => {
                    let (fp, fq) = (rev.frame(p), rev.frame(q));
                    // a segment is as far from the plane as its farther end
                    let off = fp.y.mag().max(fq.y.mag());
                    (
                        Carrier::Line {
                            p: [fp.h, fp.x],
                            q: [fq.h, fq.x],
                        },
                        off,
                    )
                }
                C3::Circle(c, n, r) => {
                    let fc = rev.frame(c);
                    // the circle's points are its centre plus `r` times a unit vector of
                    // its plane, whose component along `e1` is at most `|n x e1|/(|n||e1|)`
                    let tilt = norm(cross(n, e1)).div(norm(n).mul(l1));
                    let off = fc.y.add(Iv::exact(r).mul(tilt)).mag().max(fc.y.sub(Iv::exact(r).mul(tilt)).mag());
                    (
                        Carrier::Circle {
                            c: [fc.h, fc.x],
                            r: Iv::exact(r),
                        },
                        off,
                    )
                }
            };
            rev.carrier = carrier;
            rev.off_plane = off_plane;
            rev
        }

        /// The coordinates of the exact point enclosed by `p`.
        fn frame(&self, p: V) -> Hxy {
            let w = sub(p, self.base);
            let x = dot(w, self.e0).div(self.l0);
            let y = dot(w, self.e1).div(self.l1);
            Hxy {
                h: dot(w, self.a).div(self.la),
                x,
                y,
                rho: sq(x).add(sq(y)).sqrt(),
            }
        }

        fn at(&self, p: Vec3) -> Hxy {
            self.frame(iv(p))
        }

        /// The distance of `(h, w)` from the carrier in the half-plane.
        fn off_carrier(&self, h: Iv, w: Iv) -> Iv {
            match self.carrier {
                Carrier::Line { p, q } => {
                    let d = [q[0].sub(p[0]), q[1].sub(p[1])];
                    let cr = h.sub(p[0]).mul(d[1]).sub(w.sub(p[1]).mul(d[0]));
                    let len = sq(d[0]).add(sq(d[1])).sqrt();
                    Iv::exact(cr.mag()).div(len)
                }
                Carrier::Circle { c, r } => {
                    let dd = sq(h.sub(c[0])).add(sq(w.sub(c[1]))).sqrt();
                    Iv::exact(dd.sub(r).mag())
                }
            }
        }

        /// The side `sigma` (`1` the profile's half-plane, `-1` the opposite one) of the
        /// carrier `p` lies on within `tol`: `p` folds to `(h, sigma rho)`.
        fn side(&self, p: Hxy, tol: f64) -> Option<f64> {
            let plus = self.off_carrier(p.h, p.rho).hi;
            let minus = self.off_carrier(p.h, Iv::ZERO.sub(p.rho)).hi;
            let (sigma, d) = if plus <= minus { (1.0, plus) } else { (-1.0, minus) };
            (d <= tol).then_some(sigma)
        }

        /// The signed distances of `p` turned to side `sigma` from the start half-plane
        /// and the end half-plane, positive inside the swept sector.
        fn start_end_offsets(&self, p: Hxy, sigma: f64) -> (Iv, Iv) {
            let s = Iv::exact(sigma);
            let (x, y) = (p.x.mul(s), p.y.mul(s));
            let (sin, cos) = self.sin_cos;
            (y, sin.mul(x).sub(cos.mul(y)))
        }

        /// Whether `p` on side `sigma` lies within `tol` of the swept sector: a convex
        /// wedge up to a half turn, the union of the two half-planes past it.
        fn in_sector(&self, p: Hxy, sigma: f64, tol: f64) -> bool {
            let (fs, fe) = self.start_end_offsets(p, sigma);
            if self.reflex {
                fs.lo >= -tol || fe.lo >= -tol
            } else {
                (fs.lo >= -tol && fe.lo >= -tol) || p.rho.hi <= tol
            }
        }

        /// The turned meridian's unit tangent at `p` (on side `sigma`), in the half-plane
        /// of the radial `r` (which names the half-plane of a point on the axis).
        fn meridian_in(&self, p: Hxy, sigma: f64, r: V) -> V {
            let t = match self.carrier {
                Carrier::Line { p: a, q } => [q[0].sub(a[0]), q[1].sub(a[1])],
                Carrier::Circle { c, .. } => {
                    let w = p.rho.mul(Iv::exact(sigma));
                    [c[1].sub(w), p.h.sub(c[0])]
                }
            };
            let ahat = scale(self.a, Iv::exact(1.0).div(self.la));
            let rhat = scale(r, Iv::exact(1.0).div(norm(r)));
            let m = [0, 1, 2].map(|i| ahat[i].mul(t[0]).add(rhat[i].mul(t[1].mul(Iv::exact(sigma)))));
            scale(m, Iv::exact(1.0).div(norm(m)))
        }

        /// The radial of the recorded point `p` (perpendicular to the axis).
        fn radial(&self, p: Vec3) -> V {
            let w = sub(iv(p), self.base);
            sub(w, scale(self.a, dot(w, self.a).div(dot(self.a, self.a))))
        }
    }

    fn pt(rec: &[f64], at: usize) -> Vec3 {
        Vec3::new(rec[at], rec[at + 1], rec[at + 2])
    }

    #[test]
    fn every_moved_row_is_the_revolution_of_its_profile() {
        let k = Kernel::default();
        // the kernel's angular tolerance: vectors within it are parallel
        let ang = sin_cos_deg(k.tolerances.ang_tol_deg).0.hi;
        for name in ROWS {
            let i: Inputs = crate::case::regression_case(&Runner, name).parse().unwrap();
            let OpIn::EdgeHigher { edge } = &i.op else {
                panic!("{name}: an EdgeHigher row");
            };
            let out = run_rust(&i).unwrap();
            let rec = &out.values["out"];
            assert_eq!((rec[FIXTURE], rec[FIXTURE + 1]), (0.0, 1.0), "{name}: a surface");
            let (u0, v0, u1, v1) = (
                rec[FIXTURE + 3],
                rec[FIXTURE + 4],
                rec[FIXTURE + 5],
                rec[FIXTURE + 6],
            );
            let sample = |a: usize, b: usize| {
                let at = SAMPLES + 4 * (a * N_SURF_SAMPLES + b);
                assert_eq!(rec[at], 0.0, "{name}: sample status");
                pt(rec, at + 1)
            };
            // the kernel's declared accuracy for this profile, from the same floats
            // `edge_sweep_higher` hands `analytic_revolution`: the edge's curve trimmed to
            // its interval, and the larger of the sweep's and the B-rep's tolerance, capped
            // at `FIT_REL * (1 + size)`
            let curve = rust_curve(&k, &edge.curve).unwrap();
            let (mut fb, shell) = new_brep(&k, &i.sweep).unwrap();
            let (fe, _) = rust_edge(&k, &mut fb, shell, edge).unwrap();
            let mut trimmed = edge::curve(&fb, fe).unwrap().unwrap().copy(&k).unwrap();
            let mut ivl = edge::interval(&fb, fe).unwrap();
            trimmed.trim(&k, &mut ivl, TrimMode::Notify).unwrap();
            let natural = trimmed.ends(&k).unwrap();
            let base = v3(i.sweep.base);
            let size = natural
                .start
                .max_dimension()
                .max(natural.end.max_dimension())
                .max(base.max_dimension());
            let eps = if i.sweep.eps > fb.tolerance() { i.sweep.eps } else { fb.tolerance() };
            let tol = eps.min(FIT_REL * (1.0 + size));
            let rev = Revolution::new(&i.sweep, &edge.curve);
            assert!(rev.off_plane <= tol, "{name}: profile {} off its plane", rev.off_plane);

            // the surface's kind is the one `candidate` names for the profile: a line
            // whose ends are equally far from the axis (within `tol`) sweeps a cylinder,
            // another line a cone; an arc centred on the axis (within the larger of `tol`
            // and the caller's tolerance) a sphere, another arc a torus. The distances are
            // the kernel's own (`line_point_distance` on the same floats); the geometry of
            // the surface is proved by the samples below, whatever its kind.
            let axis = v3(i.sweep.axis);
            let code = rec[FIXTURE + 2] as i64;
            let want = match &edge.curve {
                CurveIn::Circle { c, .. } | CurveIn::Ellipse { c, .. } => {
                    let (centre_dist, _) = line_point_distance(base, axis, v3(*c));
                    if centre_dist <= tol.max(eps) { SPHERE_TYPE } else { TORUS_TYPE }
                }
                _ => {
                    let (sd, _) = line_point_distance(base, axis, natural.start);
                    let (ed, _) = line_point_distance(base, axis, natural.end);
                    if (sd - ed).abs() <= tol { CYLINDER_TYPE } else { CONE_TYPE }
                }
            };
            assert_eq!(code, want, "{name}: surface kind");

            // the five tolerances are the edge's and its vertices' (the fixture's), which
            // the analytic surface does not touch: the face, start curve and edge take the
            // edge's; each side takes its vertex's, or stays unset (-1) on a closed edge
            let (vtol, etol) = ([rec[10], rec[11]], rec[12]);
            assert_eq!(rec[AFTER_SAMPLES..AFTER_SAMPLES + 3], [etol; 3], "{name}: edge tols");
            for t in &rec[AFTER_SAMPLES + 3..NORMAL] {
                assert!(vtol.contains(t) || *t == -1.0, "{name}: vertex tol {t}");
            }

            // (a) the surface: every sample on the carrier turned into the sector
            for a in 0..N_SURF_SAMPLES {
                for b in 0..N_SURF_SAMPLES {
                    let p = rev.at(sample(a, b));
                    let sigma = rev
                        .side(p, tol)
                        .unwrap_or_else(|| panic!("{name}: sample {a},{b} {p:?} off carrier"));
                    assert!(rev.in_sector(p, sigma, tol), "{name}: sample {a},{b} outside sector");
                }
            }

            // the profile's ends, which the side trims sweep
            let ends = [
                curve.evaluate_point(&k, edge.t[0]).unwrap(),
                curve.evaluate_point(&k, edge.t[1]).unwrap(),
            ];

            // (b) the 3D trims: each on the surface and one boundary of it
            let n = rec[TRIMS] as usize;
            assert!(n > 0, "{name}: a swept face with no trims");
            let mut at = TRIMS + 1;
            let mut trims = Vec::new();
            for t in 0..n {
                let ps: Vec<Vec3> = (0..N_CURVE_SAMPLES)
                    .map(|j| {
                        let o = at + 3 + 4 * j;
                        assert_eq!(rec[o], 0.0, "{name}: trim {t} status");
                        pt(rec, o + 1)
                    })
                    .collect();
                at += 3 + 4 * N_CURVE_SAMPLES;
                let fs: Vec<Hxy> = ps.iter().map(|p| rev.at(*p)).collect();
                for f in &fs {
                    let s = rev
                        .side(*f, tol)
                        .unwrap_or_else(|| panic!("{name}: trim {t} {f:?} off carrier"));
                    assert!(rev.in_sector(*f, s, tol), "{name}: trim {t} outside sector");
                }
                // a profile (start or end half-plane) or the circle of one profile end
                let start = fs.iter().all(|f| f.y.mag() <= tol);
                let (sin, cos) = rev.sin_cos;
                let end = fs.iter().all(|f| sin.mul(f.x).sub(cos.mul(f.y)).mag() <= tol);
                let circle = (0..2).find(|&e| {
                    let fe = rev.at(ends[e]);
                    fs.iter().all(|f| f.h.sub(fe.h).mag() <= tol && f.rho.sub(fe.rho).mag() <= tol)
                });
                assert!(start || end || circle.is_some(), "{name}: trim {t} on no boundary");
                trims.push((ps, start, end, circle));
            }

            // the uv trims: lines of the domain's boundary
            let nuv = rec[at] as usize;
            at += 1;
            let mut uvs = Vec::new();
            for t in 0..nuv {
                let ps: Vec<[f64; 2]> = (0..N_CURVE_SAMPLES)
                    .map(|j| {
                        let o = at + 3 + 3 * j;
                        assert_eq!(rec[o], 0.0, "{name}: uv trim {t} status");
                        [rec[o + 1], rec[o + 2]]
                    })
                    .collect();
                at += 3 + 3 * N_CURVE_SAMPLES;
                let on = |f: &dyn Fn(&[f64; 2]) -> bool| ps.iter().all(f);
                assert!(
                    on(&|q| q[0] == u0) || on(&|q| q[0] == u1) || on(&|q| q[1] == v0) || on(&|q| q[1] == v1),
                    "{name}: uv trim {t} off the domain boundary: {ps:?}"
                );
                uvs.push(ps);
            }
            let norient = rec[at] as usize;
            let orients: Vec<f64> = rec[at + 1..at + 1 + norient].to_vec();
            assert_eq!(at + 1 + norient, rec.len(), "{name}: the record ends");
            assert_eq!(norient, n, "{name}: an orientation per trim");
            assert_eq!(nuv, n, "{name}: a uv line per trim");

            // the loop closes in 3D and in uv, and each uv end is the domain corner whose
            // surface sample is the 3D end
            let ends_of = |o: f64, len: usize| if o == 1.0 { (0, len - 1) } else { (len - 1, 0) };
            let index_of = |x: f64, lo: f64, hi: f64| {
                if x == lo {
                    0
                } else {
                    assert_eq!(x, hi, "{name}: a uv end off the domain's corners");
                    N_SURF_SAMPLES - 1
                }
            };
            let corner = |q: [f64; 2]| sample(index_of(q[0], u0, u1), index_of(q[1], v0, v1));
            for t in 0..n {
                assert!(orients[t] == 1.0 || orients[t] == 2.0, "{name}: orient {t}");
                let (_, last) = ends_of(orients[t], N_CURVE_SAMPLES);
                let next = (t + 1) % n;
                let (first, _) = ends_of(orients[next], N_CURVE_SAMPLES);
                // both ends are within `tol` of their uv corner (checked below), so they
                // meet within twice that
                let gap = dist(trims[t].0[last], trims[next].0[first]).hi;
                assert!(gap <= 2.0 * tol, "{name}: 3D loop open after trim {t} by {gap}");
                // in uv the loop closes too, except across a side dropped as degenerate:
                // a boundary line between two corners whose surface point is one (a cone's
                // apex, a sphere's pole)
                let (qa, qb) = (uvs[t][last], uvs[next][first]);
                let (same_u, same_v) = (qa[0] == qb[0], qa[1] == qb[1]);
                if !(same_u && same_v) {
                    let apex = corner(qa);
                    let collapsed = dist(apex, corner(qb)).hi;
                    assert!(
                        (same_u || same_v) && collapsed <= tol,
                        "{name}: uv loop open after trim {t} ({qa:?} to {qb:?}, {collapsed})"
                    );
                    // the whole boundary line is that one point
                    for j in 0..N_SURF_SAMPLES {
                        let p = if same_u {
                            sample(index_of(qa[0], u0, u1), j)
                        } else {
                            sample(j, index_of(qa[1], v0, v1))
                        };
                        assert!(dist(p, apex).hi <= tol, "{name}: side {t} not collapsed");
                    }
                }
                for j in [0, N_CURVE_SAMPLES - 1] {
                    let gap = dist(corner(uvs[t][j]), trims[t].0[j]).hi;
                    assert!(gap <= tol, "{name}: trim {t} end {j} not its uv corner by {gap}");
                }
            }

            // the curve indices name the right boundaries
            let index = |o: usize| (rec[IDX + o] >= 0.0).then(|| rec[IDX + o] as usize);
            if let Some(sc) = index(0) {
                assert!(trims[sc].1, "{name}: start curve in the start half-plane");
            }
            if let Some(fc) = index(1) {
                assert!(trims[fc].2, "{name}: far-end curve in the end half-plane");
            }
            let sv = pt(rec, 4);
            // (which end a vertex is: the nearer one)
            let near_end = |p: Vec3| usize::from((ends[0] - p).length() > (ends[1] - p).length());
            if let Some(c) = index(2) {
                assert_eq!(trims[c].3, Some(near_end(sv)), "{name}: start vertex circle");
            }
            if let Some(c) = index(3) {
                assert_eq!(trims[c].3, Some(1 - near_end(sv)), "{name}: end vertex circle");
            }

            // the normal: unit to rounding (each path divides the components by the
            // computed length: the squared length's three products and two sums, its
            // square root and the division put each component within a relative
            // d = 3.5 u / (1 - 3.5 u) of the exact quotient, u = eps/2, so the squared
            // length is within 2 d + d^2 < 7.1 u < 4 eps of 1; the test's own sum is
            // enclosed)
            let nrm = pt(rec, NORMAL);
            let n2 = dot(iv(nrm), iv(nrm));
            assert!(n2.sub(Iv::exact(1.0)).mag() <= 4.0 * f64::EPSILON, "{name}: unit normal {n2:?}");
            // perpendicular to the circle and to the meridian through the domain's minimum
            // corner within the kernel's angular tolerance, the kernel's own criterion for
            // two directions being perpendicular (as the entity tolerances are its
            // criterion for points being one). No tighter bound is derivable here: the
            // normal is evaluated through the surface's parameterisation (the sine and
            // cosine of the corner's parameters, whose accuracy Rust's `f64::sin` and
            // `f64::cos` do not specify), so any few-ulps allowance would be measured, not
            // derived; the samples' positions are held to the declared `tol` for the same
            // reason. At a corner on the axis (a cone
            // apex or a sphere pole) the normal is the limit along the corner's meridian:
            // take that meridian's half-plane from the neighbouring sample on it (the other
            // neighbour is the corner again)
            let p00 = sample(0, 0);
            let off_axis = |q: Vec3| rev.at(q).rho.lo > tol;
            let on = if off_axis(p00) {
                p00
            } else {
                [sample(0, 1), sample(1, 0)]
                    .into_iter()
                    .find(|&q| off_axis(q))
                    .unwrap_or_else(|| panic!("{name}: corner meridian collapsed"))
            };
            let r = rev.radial(on);
            let sigma = rev.side(rev.at(on), tol).unwrap();
            let circ = cross(rev.a, r);
            let along_circ = dot(iv(nrm), circ).div(norm(circ)).mag();
            assert!(along_circ <= ang, "{name}: normal off the circle's normal plane: {along_circ}");
            let mer = rev.meridian_in(rev.at(p00), sigma, r);
            let along_mer = dot(iv(nrm), mer).mag();
            assert!(along_mer <= ang, "{name}: normal along the meridian: {along_mer}");
            // on the side of S_u x S_v: the first grid cell's diagonals, (S11 - S00) x
            // (S01 - S10), are twice the cell's mean S_u x S_v (exact for a bilinear
            // cell) and stay nonzero when one of the corner's sides collapses to the axis
            let d1 = sub(iv(sample(1, 1)), iv(p00));
            let d2 = sub(iv(sample(0, 1)), iv(sample(1, 0)));
            let side = dot(iv(nrm), cross(d1, d2));
            assert!(side.lo > 0.0, "{name}: normal against S_u x S_v: {side:?}");
        }
    }
}
