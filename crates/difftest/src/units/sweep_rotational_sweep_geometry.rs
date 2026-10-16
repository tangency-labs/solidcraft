//! Runner for unit `sweep.rotational_sweep_geometry`. C++:
//! `solidcore_ffi::units::sweep_rotational_sweep_geometry` (upstream
//! `source/SMLib/src/SmRotationalSweepGeometry.cpp`). Rust:
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
//! The Rust side builds the same B-rep records; the shim reports the facts the records
//! must reproduce (B-rep tolerance, which vertex is the start / primary one, closed,
//! points, tolerances), which are compared first.
//!
//! Both sides produce the flat record of the shim (`out`): statuses (`0` =
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
use solidcore::nurbs::math::CPoint;
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
use solidcore_ffi::units::sweep_rotational_sweep_geometry::{self as ffi, Op};

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// The difftest runner.
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

fn flag(b: bool) -> f64 {
    if b {
        1.0
    } else {
        0.0
    }
}

fn push3(v: &mut Vec<f64>, p: [f64; 3]) {
    v.extend(p);
}

fn push_curve(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
        CurveIn::Nurb { p, cpts, knots } => {
            let n = cpts.len().saturating_sub(1);
            v.extend([0.0, n as f64, *p as f64, knots.len().saturating_sub(1) as f64]);
            for cp in cpts {
                v.extend(cp);
            }
            v.extend(knots);
        }
        CurveIn::Bline { a, b } => {
            v.push(1.0);
            push3(v, *a);
            push3(v, *b);
        }
        CurveIn::Line { a, b, ivl } => {
            v.push(2.0);
            push3(v, *a);
            push3(v, *b);
            let [t0, t1] = ivl.unwrap_or([0.0, 1.0]);
            v.extend([flag(ivl.is_some()), t0, t1]);
        }
        CurveIn::Circle { c, x, y, d, r, io } => {
            v.push(3.0);
            push3(v, *c);
            push3(v, *x);
            push3(v, *y);
            v.extend([d[0], d[1], *r, flag(*io)]);
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
            v.push(4.0);
            push3(v, *c);
            push3(v, *x);
            push3(v, *y);
            v.extend([d[0], d[1], *rx, *ry, flag(*io)]);
        }
        CurveIn::Hermite { p1, d1, p2, d2 } => {
            v.push(5.0);
            for p in [p1, d1, p2, d2] {
                push3(v, *p);
            }
        }
    }
}

fn push_surface(v: &mut Vec<f64>, s: &SurfIn) {
    match s {
        SurfIn::Nurb { p, q, net, u, v: kv } => {
            let n = net.len().saturating_sub(1);
            let m = net.first().map_or(0, Vec::len).saturating_sub(1);
            v.extend([
                0.0,
                n as f64,
                m as f64,
                *p as f64,
                *q as f64,
                u.len().saturating_sub(1) as f64,
                kv.len().saturating_sub(1) as f64,
            ]);
            for row in net {
                for cp in row {
                    v.extend(cp);
                }
            }
            v.extend(u);
            v.extend(kv);
        }
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            dom,
        } => {
            v.push(1.0);
            push3(v, *o);
            push3(v, *x);
            push3(v, *y);
            v.extend(scale);
            v.extend(dom);
        }
    }
}

fn push_edge(v: &mut Vec<f64>, e: &EdgeIn) {
    push_curve(v, &e.curve);
    v.extend([e.t[0], e.t[1], flag(e.opposite), flag(e.closed)]);
    push3(v, e.off0);
    push3(v, e.off1);
    v.extend([e.etol, e.vtol[0], e.vtol[1]]);
}

fn cpp_args(i: &Inputs) -> (Op, Vec<f64>) {
    let s = &i.sweep;
    let mut v = Vec::new();
    push3(&mut v, s.base);
    push3(&mut v, s.axis);
    v.extend([s.angle, s.eps, flag(s.analytics), s.brep_tol]);
    let op = match &i.op {
        OpIn::IsClosed { repeats } => {
            v.push(*repeats as f64);
            Op::IsClosed
        }
        OpIn::StartCs { pt } => {
            push3(&mut v, *pt);
            Op::StartCs
        }
        OpIn::EndCs { pt } => {
            push3(&mut v, *pt);
            Op::EndCs
        }
        OpIn::Transform => Op::Transform,
        OpIn::Copy => Op::Copy,
        OpIn::CheckCurve {
            curve,
            t,
            tol,
            pt,
            vec,
            amount,
        } => {
            push_curve(&mut v, curve);
            v.extend([t[0], t[1], *tol]);
            push3(&mut v, *pt);
            push3(&mut v, *vec);
            v.push(*amount);
            Op::CheckCurve
        }
        OpIn::VertexHigher { pt, vtol } => {
            push3(&mut v, *pt);
            v.push(*vtol);
            Op::VertexHigher
        }
        OpIn::VertexSame { pt, vtol } => {
            push3(&mut v, *pt);
            v.push(*vtol);
            Op::VertexSame
        }
        OpIn::EdgeHigher { edge } => {
            push_edge(&mut v, edge);
            Op::EdgeHigher
        }
        OpIn::EdgeSame { edge } => {
            push_edge(&mut v, edge);
            Op::EdgeSame
        }
        OpIn::FaceSame { surface, ftol } => {
            push_surface(&mut v, surface);
            v.push(*ftol);
            Op::FaceSame
        }
    };
    (op, v)
}

fn run_cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let (op, args) = cpp_args(i);
    match ffi::eval(op, &args) {
        Ok(out) => Ok(Output::new().values("out", out)),
        Err(e) => Err(harness(format!("C++ shim: {e}"))),
    }
}

// ---- Rust -----------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

/// The `SmStatus` of a Rust error; `1007` for `InvalidInput`, `-1` for one upstream
/// cannot return.
fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::InvalidInput { .. } => SM_ERR_INVALID_INPUT,
        SolidError::Upstream { code, .. } => f64::from(*code),
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
        CurveIn::Hermite { p1, d1, p2, d2 } => Curve::from(HermiteCurve::new(
            v3(*p1),
            v3(*d1),
            v3(*p2),
            v3(*d2),
            3,
        )),
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
    let r = b.add_region_record(Region { in_brep: true }).map_err(hb)?;
    let shell = b
        .add_shell_record(Shell {
            faces: Vec::new(),
            region: Some(r),
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
    let pa = curve.evaluate_point(ta).map_err(hb)? + v3(e.off0);
    let pb = curve.evaluate_point(tb).map_err(hb)? + v3(e.off1);
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
fn put_curve(o: &mut Vec<f64>, c: &Curve, uv: bool) -> Result<(), Fail> {
    o.push(c.type_code() as f64);
    let nat = c.natural_interval();
    o.push(nat.min());
    o.push(nat.max());
    for i in 0..N_CURVE_SAMPLES {
        let t = ivl_eval(nat.min(), nat.max(), i as f64 / (N_CURVE_SAMPLES - 1) as f64);
        let (st, p) = match c.evaluate_point(t) {
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
fn put_surface(o: &mut Vec<f64>, s: &Surface) -> Result<(), Fail> {
    o.push(s.type_code() as f64);
    let d = s.natural_uv_domain();
    let (mn, mx) = (d.min(), d.max());
    o.extend([mn.x, mn.y, mx.x, mx.y]);
    for i in 0..N_SURF_SAMPLES {
        for j in 0..N_SURF_SAMPLES {
            let fu = i as f64 / (N_SURF_SAMPLES - 1) as f64;
            let fv = j as f64 / (N_SURF_SAMPLES - 1) as f64;
            let uv = Point2::new(mn.x + fu * (mx.x - mn.x), mn.y + fv * (mx.y - mn.y));
            let (st, p) = match s.evaluate_point(uv) {
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
                            put_curve(o, &c, false)?;
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
                    put_curve(o, &c, false)?;
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
                        put_surface(o, srf)?;
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
                        put_curve(o, c, false)?;
                    }
                    o.push(r.uv_trim_curves.len() as f64);
                    for c in &r.uv_trim_curves {
                        put_curve(o, &Curve::from(c.clone()), true)?;
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
            let dom = srf.natural_uv_domain();
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
                        put_surface(o, &srf)?;
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

/// Non-rational weight marker.
const NOW: f64 = -f64::MAX;

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
            &[30.0, 36.0, 45.0, 90.0, 180.0, 270.0, 360.0, 359.9999, 360.0, 0.5, 1e-3],
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
                4 => (-rng.uniform(0.5, 2.0), 0.0, rng.uniform(0.5, 2.0), rng.uniform(0.0, 1.0)), // crossing
                5 => (0.0, 0.0, rng.uniform(0.5, 3.0), rng.uniform(0.5, 3.0)), // cone apex on axis
                6 => (rng.uniform(1e-7, 1e-5), 0.0, rng.uniform(0.5, 3.0), 0.0), // near the axis
                7 => (rng.uniform(0.5, 3.0), 0.0, rng.uniform(0.5, 3.0), rng.uniform(1e-7, 1e-5)), // almost perpendicular
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
                    let t = if planar { th } else { th + rng.uniform(-0.8, 0.8) };
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
            scale3(norm(rng.point3(-1.0, 1.0)), pick(rng, &[1e-9, 1e-7, 0.5], 1e-7) * eps.max(1e-6))
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
            dom: [u0, v0, u0 + rng.uniform(0.5, 3.0), v0 + rng.uniform(0.5, 3.0)],
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

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        run_cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        run_rust(&case.parse()?)
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        // keep only cases the shim accepts (the generator aims inside the guards)
        for _ in 0..16 {
            let i = gen_case(rng);
            let (op, args) = cpp_args(&i);
            if ffi::validate(op, &args).is_ok() {
                return serde_json::to_value(i).ok();
            }
        }
        None
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        default_cases(self, &ctx)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn generated_cases_validate() {
        let mut ok = 0;
        for seed in 0..300 {
            let mut rng = Rng::new(seed);
            let i = gen_case(&mut rng);
            let (op, args) = cpp_args(&i);
            if ffi::validate(op, &args).is_ok() {
                ok += 1;
            }
        }
        assert!(ok > 280, "only {ok} of 300 generated cases validate");
    }
}

