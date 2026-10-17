//! Runner for unit `query.loop`: `SmLoop::ContainsUVPoint` (with the co-ported
//! `SmLoop::GetUVCornerBoundaries` and `sm_CalcQuadNum`).
//!
//! C++: `solidcore_ffi::units::query_loop` (upstream `source/SMLib/src/SmLoop.cpp`) on a
//! one-face B-rep built with `SmBrepConstructor`: real `SmPlane`, `SmBSplineSurface` and
//! `SmSurfOfRevolution` faces, `SmLine`, `SmCircle` and `SmBSplineCurve` edges, explicit
//! 2D B-spline UV trim curves and explicit face / edge zone tolerances. Rust:
//! `solidcore::query::r#loop::contains_uv_point` on a one-face `Brep` built from the same
//! data (the loop walks the B-rep, which it may edit where it creates UV trim curves).
//! The arena surfaces carry no face owner; the face's zone tolerance is used as upstream's
//! `GetZoneTol3d(pSurface)` of the face's own surface.
//!
//! A case is the face (surface, tolerance), its edges (curve, interval, tolerance), the
//! loop's edge uses in loop order (edge, orientation, UV trim curve) and the point.
//! Outputs: `containment` (`SmPointObjectContainmentType`), `on_edge` (edge index, `-1`
//! none), `edge_param` and `orientation` (`SmOrientType`). A surface that cannot be built
//! is the error `surface` on both sides.
//!
//! Deferred cases: a point on a side of the surface's natural domain with a loop end on
//! a side too (`SmSurface::GetSingularities`, `surface.surface_p3`), and the global
//! point solves of `DropPoint` on kinds whose solver is not ported yet.

use serde::{Deserialize, Serialize};
use serde_json::Value;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::line::Line;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector2d::{Point2, Vec2};
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::b_spline_surface_n_lib::p2::OrientType;
use solidcore::nurbs::crv_basic::Curve as NurbCurve;
use solidcore::nurbs::math::{CPoint, NOW};
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::query::r#loop::contains_uv_point;
use solidcore::surface::plane::Plane;
use solidcore::surface::surf_of_revolution::SurfOfRevolution;
use solidcore::surface::surface::Surface;
use solidcore::topology::{
    Brep, Edge, EdgeId, Edgeuse, EdgeuseId, EdgeuseOwner, Face, FaceId, Faceuse, FaceuseId, Loop,
    LoopId, Loopuse, LoopuseId, LoopuseStart, Region, RegionId, Shell, ShellId, Vertex, VertexId,
};
use solidcore::SolidError;
use solidcore_ffi::units::query_loop as ffi;

use crate::case::CaseContext;
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 3000;

/// `query.loop` runner.
pub struct Runner;

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

// ---- inputs -----------------------------------------------------------------------

/// A NURB curve: control points `[x, y, z, w]` (`w = NL_NOW` when not rational; `z`
/// is ignored for a UV trim curve), degree and knots.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct NurbIn {
    cpts: Vec<[f64; 4]>,
    p: usize,
    u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum CurveIn {
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
    },
    Bspline(NurbIn),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum GenIn {
    Line {
        a: [f64; 3],
        b: [f64; 3],
    },
    Circle {
        c: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        d: [f64; 2],
        r: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SurfIn {
    Plane {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
        #[serde(default = "unit_scale")]
        scale: [f64; 2],
        /// `[umin, vmin, umax, vmax]`.
        domain: [f64; 4],
    },
    Bspline {
        net: Vec<Vec<[f64; 4]>>,
        p: usize,
        q: usize,
        u: Vec<f64>,
        v: Vec<f64>,
    },
    Revolution {
        gen: GenIn,
        point: [f64; 3],
        axis: [f64; 3],
        #[serde(default)]
        swap: bool,
    },
}

fn unit_scale() -> [f64; 2] {
    [1.0, 1.0]
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeIn {
    curve: CurveIn,
    interval: [f64; 2],
    tol: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeuseIn {
    edge: usize,
    same: bool,
    trim: NurbIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct Inputs {
    face_tol: f64,
    #[serde(default)]
    vertex_loop: bool,
    uv: [f64; 2],
    #[serde(default)]
    tol3d: f64,
    surface: SurfIn,
    edges: Vec<EdgeIn>,
    edgeuses: Vec<EdgeuseIn>,
}

// ---- C++ ----------------------------------------------------------------------------

fn ffi_nurb(n: &NurbIn) -> ffi::Nurb {
    ffi::Nurb {
        cpts: n.cpts.clone(),
        p: n.p,
        knots: n.u.clone(),
    }
}

fn ffi_case(i: &Inputs) -> ffi::Case {
    let surface = match &i.surface {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => ffi::Surf::Plane {
            o: *o,
            x: *x,
            y: *y,
            scale: *scale,
            domain: *domain,
        },
        SurfIn::Bspline { net, p, q, u, v } => ffi::Surf::BSpline {
            net: net.clone(),
            p: *p,
            q: *q,
            u: u.clone(),
            v: v.clone(),
        },
        SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
        } => ffi::Surf::Revolution {
            gen: match gen {
                GenIn::Line { a, b } => ffi::Gen::Line { a: *a, b: *b },
                GenIn::Circle { c, x, y, d, r } => ffi::Gen::Circle {
                    c: *c,
                    x: *x,
                    y: *y,
                    d: *d,
                    r: *r,
                },
            },
            point: *point,
            axis: *axis,
            swap: *swap,
        },
    };
    ffi::Case {
        face_tol: i.face_tol,
        vertex_loop: i.vertex_loop,
        uv: i.uv,
        tol3d: i.tol3d,
        surface,
        edges: i
            .edges
            .iter()
            .map(|e| ffi::Edge {
                curve: match &e.curve {
                    CurveIn::Line { a, b } => ffi::Curve3::Line { a: *a, b: *b },
                    CurveIn::Circle { c, x, y, d, r } => ffi::Curve3::Circle {
                        c: *c,
                        x: *x,
                        y: *y,
                        d: *d,
                        r: *r,
                    },
                    CurveIn::Bspline(n) => ffi::Curve3::BSpline(ffi_nurb(n)),
                },
                interval: e.interval,
                tol: e.tol,
            })
            .collect(),
        edgeuses: i
            .edgeuses
            .iter()
            .map(|eu| ffi::Edgeuse {
                edge: eu.edge,
                same: eu.same,
                trim: ffi_nurb(&eu.trim),
            })
            .collect(),
    }
}

fn answer(containment: i64, on_edge: i64, edge_param: f64, orientation: i64) -> Output {
    Output::new()
        .int("containment", containment)
        .int("on_edge", on_edge)
        .scalar("edge_param", edge_param)
        .int("orientation", orientation)
}

fn cpp(i: &Inputs) -> Result<Output, HarnessError> {
    let a = ffi::contains(&ffi_case(i)).map_err(|e| harness(format!("C++: {e}")))?;
    match a.build {
        0 => Ok(answer(
            i64::from(a.containment),
            a.on_edge,
            a.edge_param,
            i64::from(a.orientation),
        )),
        1 => Ok(Output::err("surface")),
        b => Err(harness(format!("C++ face build failed at step {b}"))),
    }
}

// ---- Rust ---------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

fn rust_nurb(n: &NurbIn, dim: u64) -> Result<Curve, HarnessError> {
    let pw = n
        .cpts
        .iter()
        .map(|c| CPoint::new(c[0], c[1], if dim == 2 { 0.0 } else { c[2] }, c[3]))
        .collect();
    let cur = NurbCurve::new(pw, n.p, n.u.clone()).map_err(|e| harness(format!("curve: {e}")))?;
    Ok(Curve::from(BSplineCurve::from_nurb(cur, dim, false)))
}

fn rust_circle(
    k: &Kernel,
    c: [f64; 3],
    x: [f64; 3],
    y: [f64; 3],
    d: [f64; 2],
    r: f64,
) -> Result<Curve, HarnessError> {
    let e = Ellipse::new_circle(
        k,
        v3(c),
        v3(x),
        v3(y),
        Extent1d::from_min_max(d[0], d[1]),
        r,
        &EllipseOptions::default(),
    )
    .map_err(|e| harness(format!("circle: {e}")))?;
    Ok(Curve::from(e))
}

fn rust_curve(k: &Kernel, c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Line { a, b } => Ok(Curve::from(Line::from_points(v3(*a), v3(*b), 3))),
        CurveIn::Circle { c, x, y, d, r } => rust_circle(k, *c, *x, *y, *d, *r),
        CurveIn::Bspline(n) => rust_nurb(n, 3),
    }
}

/// The surface; `Ok(None)` when its construction fails.
fn rust_surface(k: &Kernel, s: &SurfIn) -> Result<Option<Surface>, HarnessError> {
    Ok(match s {
        SurfIn::Plane {
            o,
            x,
            y,
            scale,
            domain,
        } => Some(Surface::from(Plane::new(
            v3(*o),
            v3(*x),
            v3(*y),
            Vec2::new(scale[0], scale[1]),
            &Extent2d::from_bounds(domain[0], domain[1], domain[2], domain[3]),
        ))),
        SurfIn::Bspline { net, p, q, u, v } => {
            let net = net
                .iter()
                .map(|r| r.iter().map(|c| CPoint::new(c[0], c[1], c[2], c[3])).collect())
                .collect();
            let sur = NurbSurface::new(net, *p, *q, u.clone(), v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            Some(Surface::from(BSplineSurface::from_nurb(sur, false)))
        }
        SurfIn::Revolution {
            gen,
            point,
            axis,
            swap,
        } => {
            let g = match gen {
                GenIn::Line { a, b } => {
                    let mut l = Line::from_points(v3(*a), v3(*b), 3);
                    l.make_nurb()
                        .map_err(|e| harness(format!("line nurb: {e}")))?;
                    Curve::from(l)
                }
                GenIn::Circle { c, x, y, d, r } => rust_circle(k, *c, *x, *y, *d, *r)?,
            };
            SurfOfRevolution::create_canonical(k, g, v3(*point), v3(*axis), *swap)
                .ok()
                .map(Surface::from)
        }
    })
}

fn orient_code(o: OrientType) -> i64 {
    match o {
        OrientType::Unknown => 0,
        OrientType::Same => 1,
        OrientType::Opposite => 2,
    }
}

/// The one-face B-rep of a case, as the shim's `SmBrepConstructor` realises it: the
/// face (surface owned by the face, zone tolerance), its loop (a vertex loop or the
/// edge uses in the requested order, closed through the CCW links), the edges and
/// every edge use's own UV trim curve.
fn rust_brep(
    k: &Kernel,
    i: &Inputs,
    surface: Surface,
    curves: Vec<Curve>,
    trims: Vec<Curve>,
) -> Result<Brep, HarnessError> {
    let err = |e: SolidError| harness(format!("brep: {e}"));
    let lu = LoopuseId::from_index(0);
    let n = u32::try_from(i.edgeuses.len()).map_err(|_| harness("edge uses"))?;
    let uv_domain = surface.natural_uv_domain();
    let mut b = Brep::new();
    b.add_region_record(Region {
        in_brep: true,
        ..Region::default()
    })
    .map_err(err)?;
    b.add_shell_record(Shell {
        faces: vec![FaceId::from_index(0)],
        region: Some(RegionId::from_index(0)),
        ..Shell::default()
    })
    .map_err(err)?;
    b.add_face_record(Face {
        loops: vec![LoopId::from_index(0)],
        surface: Some(surface),
        faceuse: Some(FaceuseId::from_index(0)),
        uv_domain,
        tolerance: zone_tol3d(k, i.face_tol),
        ..Face::new(ShellId::from_index(0))
    })
    .map_err(err)?;
    b.add_loop_record(Loop {
        face: FaceId::from_index(0),
        edges: Vec::new(),
        loopuse: Some(lu),
    })
    .map_err(err)?;
    b.add_faceuse_record(Faceuse {
        shell: Some(ShellId::from_index(0)),
        face: Some(FaceId::from_index(0)),
        mate: None,
        orientation: OrientType::Same,
        loopuses: vec![lu],
        ..Faceuse::default()
    })
    .map_err(err)?;
    b.add_loopuse_record(Loopuse {
        faceuse: Some(FaceuseId::from_index(0)),
        loop_: Some(LoopId::from_index(0)),
        mate: None,
        start: if i.vertex_loop {
            LoopuseStart::Vertexuse(None)
        } else {
            LoopuseStart::Edgeuse(Some(EdgeuseId::from_index(0)))
        },
        orientation: OrientType::Same,
    })
    .map_err(err)?;
    b.add_vertex_record(Vertex::default()).map_err(err)?;
    for (ei, (e, curve)) in i.edges.iter().zip(curves).enumerate() {
        let ei = u32::try_from(ei).map_err(|_| harness("edges"))?;
        let edgeuses = i
            .edgeuses
            .iter()
            .enumerate()
            .filter(|(_, eu)| eu.edge == ei as usize)
            .map(|(j, _)| EdgeuseId::from_index(j as u32))
            .collect();
        b.add_edge_record(Edge {
            vertices: [VertexId::from_index(0), VertexId::from_index(0)],
            edgeuses,
            interval: Extent1d::from_min_max(e.interval[0], e.interval[1]),
            // SetTolerance takes an SmZoneTol3d
            tolerance: zone_tol3d(k, e.tol),
            curve: Some(curve),
        })
        .map_err(err)?;
    }
    if i.vertex_loop {
        return Ok(b);
    }
    for (j, (eu, trim)) in i.edgeuses.iter().zip(trims).enumerate() {
        let j = u32::try_from(j).map_err(|_| harness("edge uses"))?;
        b.add_edgeuse_record(Edgeuse {
            edge: Some(EdgeId::from_index(
                u32::try_from(eu.edge).map_err(|_| harness("edge"))?,
            )),
            owner: EdgeuseOwner::Loopuse(Some(lu)),
            ccw: Some(EdgeuseId::from_index((j + 1) % n)),
            cw: Some(EdgeuseId::from_index((j + n - 1) % n)),
            orientation: if eu.same {
                OrientType::Same
            } else {
                OrientType::Opposite
            },
            uv_trim_curve: Some(trim),
            ..Edgeuse::default()
        })
        .map_err(err)?;
    }
    Ok(b)
}

fn rust(i: &Inputs) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let Some(surface) = rust_surface(&k, &i.surface)? else {
        return Ok(Output::err("surface"));
    };
    let curves = i
        .edges
        .iter()
        .map(|e| rust_curve(&k, &e.curve))
        .collect::<Result<Vec<_>, _>>()?;
    for eu in &i.edgeuses {
        if eu.edge >= i.edges.len() {
            return Err(harness("edge index"));
        }
    }
    let trims = i
        .edgeuses
        .iter()
        .map(|eu| rust_nurb(&eu.trim, 2))
        .collect::<Result<Vec<_>, _>>()?;
    let mut b = rust_brep(&k, i, surface, curves, trims)?;
    match contains_uv_point(
        &k,
        &mut b,
        LoopId::from_index(0),
        None,
        Point2::new(i.uv[0], i.uv[1]),
        i.tol3d,
    ) {
        Ok(r) => Ok(answer(
            i64::from(r.containment.code()),
            r.on_edge.map_or(-1, |e| i64::from(e.index())),
            r.edge_param,
            orient_code(r.orientation),
        )),
        Err(e) => Ok(Output::unported(&e).unwrap_or_else(|| Output::err(format!("rust: {e}")))),
    }
}

// ---- generation ---------------------------------------------------------------------

type P2 = [f64; 2];

fn sub2(a: P2, b: P2) -> P2 {
    [a[0] - b[0], a[1] - b[1]]
}

fn lerp2(a: P2, b: P2, t: f64) -> P2 {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]
}

fn unit3(rng: &mut Rng) -> [f64; 3] {
    loop {
        let p = rng.point3(-1.0, 1.0);
        let l = (p[0] * p[0] + p[1] * p[1] + p[2] * p[2]).sqrt();
        if l > 0.2 && l <= 1.0 {
            return [p[0] / l, p[1] / l, p[2] / l];
        }
    }
}

fn cross3(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

fn norm3(a: [f64; 3]) -> [f64; 3] {
    let l = (a[0] * a[0] + a[1] * a[1] + a[2] * a[2]).sqrt();
    [a[0] / l, a[1] / l, a[2] / l]
}

fn scale3(a: [f64; 3], s: f64) -> [f64; 3] {
    [a[0] * s, a[1] * s, a[2] * s]
}

/// Clamped knots on `[a, b]` for `n + 1` points of degree `p`, random interior knots.
fn knots(rng: &mut Rng, n: usize, p: usize, a: f64, b: f64) -> Vec<f64> {
    let mut inner: Vec<f64> = (0..n - p).map(|_| rng.uniform(0.05, 0.95)).collect();
    inner.sort_by(f64::total_cmp);
    let mut u = vec![a; p + 1];
    u.extend(inner.iter().map(|t| a + (b - a) * t));
    u.extend(std::iter::repeat_n(b, p + 1));
    u
}

fn gen_surface(rng: &mut Rng, gs: f64) -> SurfIn {
    match rng.below(10) {
        0..=4 => {
            let o = scale3(rng.point3(-5.0, 5.0), gs);
            let x = unit3(rng);
            let y = norm3(cross3(unit3(rng), x));
            let x = scale3(x, gs);
            let y = scale3(y, gs);
            let us = *rng.choose(&[1.0, 1.0, 1.0, 1e-3, 1e3]).unwrap_or(&1.0);
            let u0 = rng.uniform(-10.0, 0.0) * us;
            let v0 = rng.uniform(-10.0, 0.0) * us;
            let scale = if rng.chance(0.7) {
                [1.0, 1.0]
            } else {
                [rng.uniform(0.5, 2.0), rng.uniform(0.5, 2.0)]
            };
            SurfIn::Plane {
                o,
                x,
                y,
                scale,
                domain: [
                    u0,
                    v0,
                    u0 + rng.uniform(2.0, 20.0) * us,
                    v0 + rng.uniform(2.0, 20.0) * us,
                ],
            }
        }
        5..=7 => {
            let p = 1 + rng.below(3) as usize;
            let q = 1 + rng.below(3) as usize;
            let n = p + rng.below(3) as usize;
            let m = q + rng.below(3) as usize;
            let rational = rng.chance(0.3);
            let (lx, ly) = (rng.uniform(2.0, 8.0), rng.uniform(2.0, 8.0));
            let net = (0..=n)
                .map(|i| {
                    (0..=m)
                        .map(|j| {
                            let x = (i as f64 / n as f64 + rng.uniform(-0.05, 0.05)) * lx * gs;
                            let y = (j as f64 / m as f64 + rng.uniform(-0.05, 0.05)) * ly * gs;
                            let z = rng.uniform(-1.0, 1.0) * gs;
                            if rational {
                                let w = rng.uniform(0.5, 2.0);
                                [x * w, y * w, z * w, w]
                            } else {
                                [x, y, z, NOW]
                            }
                        })
                        .collect()
                })
                .collect();
            let (a, b) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                let a = rng.uniform(-3.0, 3.0);
                (a, a + rng.uniform(0.5, 6.0))
            };
            let (c, d) = if rng.chance(0.5) {
                (0.0, 1.0)
            } else {
                let c = rng.uniform(-3.0, 3.0);
                (c, c + rng.uniform(0.5, 6.0))
            };
            SurfIn::Bspline {
                net,
                p,
                q,
                u: knots(rng, n, p, a, b),
                v: knots(rng, m, q, c, d),
            }
        }
        _ => {
            let r = rng.uniform(0.5, 3.0) * gs;
            let gen = match rng.below(3) {
                // a cylinder
                0 => GenIn::Line {
                    a: [r, 0.0, rng.uniform(-2.0, 0.0) * gs],
                    b: [r, 0.0, rng.uniform(0.5, 3.0) * gs],
                },
                // a cone
                1 => GenIn::Line {
                    a: [r, 0.0, rng.uniform(-2.0, 0.0) * gs],
                    b: [r * rng.uniform(0.2, 2.0), 0.0, rng.uniform(0.5, 3.0) * gs],
                },
                // a torus-like band or a sphere (the arc reaches the axis: poles)
                _ => {
                    let sphere = rng.chance(0.4);
                    let (cx, d) = if sphere {
                        (0.0, [-90.0, 90.0])
                    } else {
                        let d0 = rng.uniform(-150.0, 0.0);
                        (r * 2.0, [d0, d0 + rng.uniform(60.0, 150.0)])
                    };
                    GenIn::Circle {
                        c: [cx, 0.0, 0.0],
                        x: [1.0, 0.0, 0.0],
                        y: [0.0, 0.0, 1.0],
                        d,
                        r,
                    }
                }
            };
            SurfIn::Revolution {
                gen,
                point: [0.0, 0.0, 0.0],
                axis: [0.0, 0.0, 1.0],
                swap: rng.chance(0.3),
            }
        }
    }
}

/// A trim curve shape between two UV points, control points in traversal order
/// (`[u, v, w]`, `w = NOW` when not rational), degree and whether it is a circular arc.
fn gen_shape(rng: &mut Rng, a: P2, b: P2) -> (Vec<[f64; 3]>, usize, bool) {
    let d = sub2(b, a);
    let nrm = [-d[1], d[0]];
    match rng.below(10) {
        // a straight segment
        0..=4 => (vec![[a[0], a[1], NOW], [b[0], b[1], NOW]], 1, false),
        // a straight polyline (collinear control points)
        5 => {
            let k = 2 + rng.below(3) as usize;
            let mut ts: Vec<f64> = (0..k).map(|_| rng.uniform(0.1, 0.9)).collect();
            ts.sort_by(f64::total_cmp);
            let mut c = vec![[a[0], a[1], NOW]];
            c.extend(ts.iter().map(|t| {
                let p = lerp2(a, b, *t);
                [p[0], p[1], NOW]
            }));
            c.push([b[0], b[1], NOW]);
            (c, 1, false)
        }
        // a circular arc: rational quadratic
        6 => {
            let h = rng.uniform(-0.4, 0.4);
            let m = lerp2(a, b, 0.5);
            let mid = [m[0] + nrm[0] * h, m[1] + nrm[1] * h];
            // weight cos(half angle) of the arc through a, b with tangent apex `mid`
            let half = (2.0 * h).atan();
            let w = half.cos();
            (
                vec![
                    [a[0], a[1], 1.0],
                    [mid[0] * w, mid[1] * w, w],
                    [b[0], b[1], 1.0],
                ],
                2,
                true,
            )
        }
        // a bulged or wiggly B-spline (sometimes with a cusp-like turnaround)
        _ => {
            let p = 2 + rng.below(2) as usize;
            let k = p - 1 + rng.below(3) as usize;
            let cusp = rng.chance(0.2);
            let mut c = vec![[a[0], a[1], NOW]];
            for i in 0..k {
                let t = (i + 1) as f64 / (k + 1) as f64;
                let t = if cusp && i == k / 2 { t + 0.6 } else { t };
                let h = rng.uniform(-0.3, 0.3);
                let q = lerp2(a, b, t);
                c.push([q[0] + nrm[0] * h, q[1] + nrm[1] * h, NOW]);
            }
            c.push([b[0], b[1], NOW]);
            (c, p, false)
        }
    }
}

/// Evaluates a UV trim point on the surface (Rust; the 3D data only has to be the
/// same numbers on both sides).
fn srf_pt(s: &Surface, uv: P2) -> Option<[f64; 3]> {
    let p = s.evaluate_point(Point2::new(uv[0], uv[1])).ok()?;
    (p.x.is_finite() && p.y.is_finite() && p.z.is_finite()).then_some([p.x, p.y, p.z])
}

struct Built {
    edges: Vec<EdgeIn>,
    edgeuses: Vec<EdgeuseIn>,
}

/// One loop segment `a -> b`: an edge and its edge use (orientation random).
fn push_segment(
    rng: &mut Rng,
    k: &Kernel,
    s: &Surface,
    plane: bool,
    out: &mut Built,
    a: P2,
    b: P2,
    tol: f64,
) -> Option<()> {
    let (mut cpts, p, arc) = gen_shape(rng, a, b);
    let same = rng.chance(0.6);
    if !same {
        cpts.reverse();
    }
    let n = cpts.len() - 1;
    let linear2 = p == 1 && n == 1;

    if linear2 && rng.chance(0.15) {
        // a trim (and edge curve) extended beyond both ends, the edge interval its
        // sub-range from `a` to `b`
        let e = rng.uniform(0.05, 0.5);
        let (c0, c1) = (*cpts.first()?, *cpts.get(1)?);
        let (s0, s1) = ([c0[0], c0[1]], [c1[0], c1[1]]);
        let p0 = lerp2(s0, s1, -e);
        let p1 = lerp2(s0, s1, 1.0 + e);
        let t0 = rng.uniform(-2.0, 2.0);
        let t1 = t0 + rng.uniform(0.5, 4.0);
        let a3 = srf_pt(s, p0)?;
        let b3 = srf_pt(s, p1)?;
        let iv = [
            t0 + (t1 - t0) * e / (1.0 + 2.0 * e),
            t0 + (t1 - t0) * (1.0 + e) / (1.0 + 2.0 * e),
        ];
        out.edges.push(EdgeIn {
            curve: CurveIn::Bspline(NurbIn {
                cpts: vec![[a3[0], a3[1], a3[2], NOW], [b3[0], b3[1], b3[2], NOW]],
                p: 1,
                u: vec![t0, t0, t1, t1],
            }),
            interval: iv,
            tol,
        });
        out.edgeuses.push(EdgeuseIn {
            edge: out.edges.len() - 1,
            same,
            trim: NurbIn {
                cpts: vec![[p0[0], p0[1], 0.0, NOW], [p1[0], p1[1], 0.0, NOW]],
                p: 1,
                u: vec![t0, t0, t1, t1],
            },
        });
        return Some(());
    }

    // the edge curve and its parameter domain
    let (curve, dom) = if arc && plane && rng.chance(0.7) {
        // a true circle through the arc's 3D end points (centre from the UV arc)
        let (s0, m, s1) = (*cpts.first()?, *cpts.get(1)?, *cpts.get(2)?);
        let (pa, pb) = ([s0[0], s0[1]], [s1[0], s1[1]]);
        let apex = [m[0] / m[2], m[1] / m[2]];
        let mid = lerp2(pa, pb, 0.5);
        let half = sub2(pb, pa);
        let c = 0.5 * (half[0] * half[0] + half[1] * half[1]).sqrt();
        let alpha = m[2].clamp(-1.0, 1.0).acos();
        let to_apex = sub2(apex, mid);
        let la = (to_apex[0] * to_apex[0] + to_apex[1] * to_apex[1]).sqrt();
        if alpha < 1e-3 || la <= 0.0 || c <= 0.0 {
            return None;
        }
        let r = c / alpha.sin();
        let centre = [
            mid[0] - to_apex[0] / la * r * alpha.cos(),
            mid[1] - to_apex[1] / la * r * alpha.cos(),
        ];
        let c3 = srf_pt(s, centre)?;
        let a3 = srf_pt(s, pa)?;
        let b3 = srf_pt(s, pb)?;
        let xa = [a3[0] - c3[0], a3[1] - c3[1], a3[2] - c3[2]];
        let xb = [b3[0] - c3[0], b3[1] - c3[1], b3[2] - c3[2]];
        let rad = (xa[0] * xa[0] + xa[1] * xa[1] + xa[2] * xa[2]).sqrt();
        let nz = cross3(xa, xb);
        if rad <= 0.0 || (nz[0] * nz[0] + nz[1] * nz[1] + nz[2] * nz[2]).sqrt() <= 1e-9 * rad * rad {
            return None;
        }
        let x = norm3(xa);
        let y = norm3(cross3(norm3(nz), x));
        let ci = CurveIn::Circle {
            c: c3,
            x,
            y,
            d: [0.0, (2.0 * alpha).to_degrees()],
            r: rad,
        };
        let d = rust_curve(k, &ci).ok()?.natural_interval();
        (ci, [d.min(), d.max()])
    } else if linear2 && rng.chance(0.6) {
        let (c0, c1) = (*cpts.first()?, *cpts.get(1)?);
        let a3 = srf_pt(s, [c0[0], c0[1]])?;
        let b3 = srf_pt(s, [c1[0], c1[1]])?;
        let c = CurveIn::Line { a: a3, b: b3 };
        let d = rust_curve(k, &c).ok()?.natural_interval();
        (c, [d.min(), d.max()])
    } else {
        let (t0, t1) = if rng.chance(0.5) {
            (0.0, 1.0)
        } else {
            let t0 = rng.uniform(-5.0, 5.0);
            (t0, t0 + rng.uniform(0.1, 10.0))
        };
        // the 3D control points: the surface at the UV control points (exact on a
        // plane, a close approximation elsewhere)
        let mut c3 = Vec::new();
        for c in &cpts {
            let (u, v, w) = if c[2] == NOW {
                (c[0], c[1], NOW)
            } else {
                (c[0] / c[2], c[1] / c[2], c[2])
            };
            let q = srf_pt(s, [u, v])?;
            c3.push(if w == NOW {
                [q[0], q[1], q[2], NOW]
            } else {
                [q[0] * w, q[1] * w, q[2] * w, w]
            });
        }
        let u = knots(rng, n, p, t0, t1);
        (CurveIn::Bspline(NurbIn { cpts: c3, p, u }), [t0, t1])
    };
    if dom[1] - dom[0] <= 1e-12 {
        return None;
    }
    let u = knots(rng, n, p, dom[0], dom[1]);
    let trim = NurbIn {
        cpts: cpts.iter().map(|c| [c[0], c[1], 0.0, c[2]]).collect(),
        p,
        u,
    };
    out.edges.push(EdgeIn {
        curve,
        interval: dom,
        tol,
    });
    out.edgeuses.push(EdgeuseIn {
        edge: out.edges.len() - 1,
        same,
        trim,
    });
    Some(())
}

fn gen_tol(rng: &mut Rng, gs: f64) -> f64 {
    *rng.choose(&[1e-6, 1e-6, 1e-4, 1e-3, 1e-8, 1e-10]).unwrap_or(&1e-6) * gs
}

/// The point to classify: random, on the loop, at or near a vertex, on a domain side.
fn gen_point(rng: &mut Rng, verts: &[P2], built: &Built, dom: &Extent2d) -> P2 {
    let (lo, hi) = (dom.min(), dom.max());
    let mut bmin = [f64::MAX, f64::MAX];
    let mut bmax = [f64::MIN, f64::MIN];
    for v in verts {
        bmin = [bmin[0].min(v[0]), bmin[1].min(v[1])];
        bmax = [bmax[0].max(v[0]), bmax[1].max(v[1])];
    }
    let span = [bmax[0] - bmin[0], bmax[1] - bmin[1]];
    let clamp = |p: P2| [p[0].clamp(lo.x, hi.x), p[1].clamp(lo.y, hi.y)];
    let on_trim = |rng: &mut Rng| -> Option<P2> {
        let eu = rng.choose(&built.edgeuses)?;
        let c = rust_nurb(&eu.trim, 2).ok()?;
        let d = c.natural_interval();
        let t = d.evaluate(rng.unit());
        let p = c.evaluate_point(t).ok()?;
        Some([p.x, p.y])
    };
    let pick = rng.below(100);
    let p = match pick {
        0..=39 => [
            rng.uniform(bmin[0] - 0.2 * span[0], bmax[0] + 0.2 * span[0]),
            rng.uniform(bmin[1] - 0.2 * span[1], bmax[1] + 0.2 * span[1]),
        ],
        40..=59 => on_trim(rng).unwrap_or(bmin),
        60..=69 => *rng.choose(verts).unwrap_or(&bmin),
        70..=84 => {
            let q = on_trim(rng).unwrap_or(bmin);
            let s = span[0].max(span[1]);
            let e = *rng
                .choose(&[1e-13, 1e-11, 1e-9, 1e-8, 1e-7, 1e-6, 1e-5, 1e-3])
                .unwrap_or(&1e-9)
                * s;
            [q[0] + rng.uniform(-e, e), q[1] + rng.uniform(-e, e)]
        }
        85..=89 => {
            // same u (or v) as a vertex: the quadrant bit-noise case
            let v = *rng.choose(verts).unwrap_or(&bmin);
            if rng.chance(0.5) {
                [v[0], rng.uniform(bmin[1], bmax[1])]
            } else {
                [rng.uniform(bmin[0], bmax[0]), v[1]]
            }
        }
        90..=94 => {
            // on a side of the surface domain
            match rng.below(4) {
                0 => [lo.x, rng.uniform(lo.y, hi.y)],
                1 => [hi.x, rng.uniform(lo.y, hi.y)],
                2 => [rng.uniform(lo.x, hi.x), lo.y],
                _ => [rng.uniform(lo.x, hi.x), hi.y],
            }
        }
        _ => lerp2(bmin, bmax, 0.5),
    };
    clamp(p)
}

fn gen_case(rng: &mut Rng) -> Option<Inputs> {
    let k = Kernel::default();
    let gs = *rng.choose(&[1.0, 1.0, 1.0, 1.0, 1e-3, 1e3]).unwrap_or(&1.0);
    let surface_in = gen_surface(rng, gs);
    let surface = rust_surface(&k, &surface_in).ok()??;
    let plane = matches!(surface_in, SurfIn::Plane { .. });
    let dom = surface.natural_uv_domain();
    let (lo, hi) = (dom.min(), dom.max());
    let (w, h) = (hi.x - lo.x, hi.y - lo.y);
    if !(w.is_finite() && h.is_finite() && w > 0.0 && h > 0.0) {
        return None;
    }
    let at = |s: f64, t: f64| [lo.x + w * s, lo.y + h * t];
    let face_tol = gen_tol(rng, gs);
    let mut built = Built {
        edges: Vec::new(),
        edgeuses: Vec::new(),
    };
    let mut verts: Vec<P2> = Vec::new();
    let mut vertex_loop = false;
    let revolution = matches!(surface_in, SurfIn::Revolution { .. });
    match rng.below(100) {
        // a vertex loop
        0..=3 => {
            vertex_loop = true;
            verts.push(at(0.5, 0.5));
        }
        // the whole domain
        4..=15 => {
            verts = vec![at(0.0, 0.0), at(1.0, 0.0), at(1.0, 1.0), at(0.0, 1.0)];
        }
        // a band around a surface of revolution with its seam edge used twice
        16..=27 if revolution => {
            let (t0, t1) = (rng.uniform(0.05, 0.4), rng.uniform(0.6, 0.95));
            let (uw, vw) = if rng.chance(0.5) {
                (true, [t0, t1])
            } else {
                (false, [t0, t1])
            };
            // rectangle full in one direction, partial in the other
            let q = |s: f64, t: f64| if uw { at(s, t) } else { at(t, s) };
            let c = [q(0.0, vw[0]), q(1.0, vw[0]), q(1.0, vw[1]), q(0.0, vw[1])];
            verts = c.to_vec();
            let tol = gen_tol(rng, gs);
            // bottom, seam (c1 -> c2), top, seam reversed (c3 -> c0, the same edge)
            push_segment(rng, &k, &surface, plane, &mut built, c[0], c[1], tol)?;
            let a3 = srf_pt(&surface, c[1])?;
            let b3 = srf_pt(&surface, c[2])?;
            let seam = CurveIn::Line { a: a3, b: b3 };
            let d = rust_curve(&k, &seam).ok()?.natural_interval();
            let dom1 = [d.min(), d.max()];
            if dom1[1] - dom1[0] <= 1e-12 {
                return None;
            }
            let line = |a: P2, b: P2| NurbIn {
                cpts: vec![[a[0], a[1], 0.0, NOW], [b[0], b[1], 0.0, NOW]],
                p: 1,
                u: vec![dom1[0], dom1[0], dom1[1], dom1[1]],
            };
            built.edges.push(EdgeIn {
                curve: seam,
                interval: dom1,
                tol,
            });
            let seam_idx = built.edges.len() - 1;
            built.edgeuses.push(EdgeuseIn {
                edge: seam_idx,
                same: true,
                trim: line(c[1], c[2]),
            });
            push_segment(rng, &k, &surface, plane, &mut built, c[2], c[3], tol)?;
            built.edgeuses.push(EdgeuseIn {
                edge: seam_idx,
                same: false,
                trim: line(c[0], c[3]),
            });
        }
        // a star-shaped polygon inside the domain (sometimes touching a side)
        _ => {
            let c = at(rng.uniform(0.3, 0.7), rng.uniform(0.3, 0.7));
            let nv = 3 + rng.below(6) as usize;
            let mut angs: Vec<f64> = (0..nv)
                .map(|_| rng.uniform(0.0, std::f64::consts::TAU))
                .collect();
            angs.sort_by(f64::total_cmp);
            for a in &angs {
                let r = rng.uniform(0.08, 0.28);
                verts.push([c[0] + w * r * a.cos(), c[1] + h * r * a.sin()]);
            }
            if nv % 2 == 1 && rng.chance(0.12) {
                // a star polygon {nv/2}: it winds twice around its centre
                let star: Vec<P2> = (0..nv)
                    .map(|i| verts.get((2 * i) % nv).copied())
                    .collect::<Option<_>>()?;
                verts = star;
            }
            if rng.chance(0.1) {
                // pull one vertex onto the lower u side
                if let Some(v) = verts.first_mut() {
                    v[0] = lo.x;
                }
            }
        }
    }
    if !vertex_loop && built.edgeuses.is_empty() {
        if rng.chance(0.35) {
            verts.reverse(); // clockwise
        }
        let tol = gen_tol(rng, gs);
        for i in 0..verts.len() {
            let a = *verts.get(i)?;
            let b = *verts.get((i + 1) % verts.len())?;
            let t = if rng.chance(0.3) { gen_tol(rng, gs) } else { tol };
            push_segment(rng, &k, &surface, plane, &mut built, a, b, t)?;
        }
        // rotate the start of the loop
        let r = rng.below(built.edgeuses.len() as u64) as usize;
        built.edgeuses.rotate_left(r);
        if built.edgeuses.len() > 3 && rng.chance(0.12) {
            // an open loop (as at a pole): the gap is walked as a straight step
            let r = 1 + rng.below(built.edgeuses.len() as u64 - 1) as usize;
            built.edgeuses.remove(r);
        }
    }
    let uv = gen_point(rng, &verts, &built, &dom);
    Some(Inputs {
        face_tol,
        vertex_loop,
        uv,
        tol3d: gen_tol(rng, gs),
        surface: surface_in,
        edges: built.edges,
        edgeuses: built.edgeuses,
    })
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "query.loop"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let Some(i) = gen_case(rng) else { continue };
            // keep cases the C++ side builds as requested (the shim rejects a point
            // the surface cannot evaluate and a loop the constructor realises
            // differently)
            match ffi::contains(&ffi_case(&i)) {
                Ok(a) if a.build == 0 => return serde_json::to_value(i).ok(),
                _ => {}
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

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}
