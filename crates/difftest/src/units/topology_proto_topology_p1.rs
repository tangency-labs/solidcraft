//! Runner for unit `topology.proto_topology_p1`: the owning-topology lists, assemblies and
//! copy map (`SmOwningTopology`, `SmAssembly`, `SmAssemblyInstance`, `SmCopyBrepMap`), the
//! proto topology of `SmProtoTopology.cpp` (definitions, proto vertices, edges, faces, the
//! manager and the static helpers) and the API transformations of `SmApiGeneral.cpp`.
//!
//! C++: `solidcore_ffi::units::topology_proto_topology_p1` (shim
//! `harness/cpp/shim/topology_proto_topology_p1.cpp`). Rust:
//! `solidcore::topology::proto_topology::p1`.
//!
//! A case is one of three scripts (documented in the ffi module): a **list** script (a pool
//! of owning objects, assemblies and instance slots, one copy map, and list ops), a
//! **proto** script (a pool of curves, planes, B-spline surfaces and bare topology objects,
//! a manager, and proto ops given as `[code, args...]` arrays exactly as the shim reads
//! them) or a **transform** script (one object and one API call). Both sides produce the
//! same flat record, reported as `out`. The Rust side interprets the script against the
//! port, applying the shim's run-time refusals (an op that is undefined upstream ends the
//! record with `-2`) to its own state, so a state divergence shows up as a mismatch.
//!
//! The proto-script engine (shim ops, interpreter, pool) is shared with unit
//! `topology.proto_topology_p2`, whose ops (codes 91 to 108) its own runner
//! (`super::topology_proto_topology_p2`) generates and interprets; this runner never
//! generates them.
//!
//! A Rust error maps to its `SmStatus` (`-1` for one upstream cannot return, e.g. a
//! dereference the shim should have refused); an `Unported` error anywhere defers the case.
//! The B-rep walks (`ProtoTopology`) of the bare stand-ins answer as upstream's do: no
//! uses, no B-rep, vertices at the origin. The vertex, edge and face stand-ins are also
//! records of one B-rep ([`TopoRecords`]), so point classifications on them reach the
//! ported topology members (`SmVertex::GetPoint`, `SmEdge::GetCurve`,
//! `SmFace::GetSurface`, the zone tolerances) through the [`ClassGeometry`] hooks.
//! Deferred: `SmBrep::Transform`, the manager's fallback tolerance and what needs a real
//! B-rep (`SmBrep::GetVertices`, `Point3DClassify`, UV domains from trim curves).
//! `ProtoBrep::brep_squeeze_edge` (`SmBrep::SqueezeEdge`) defers under
//! `harness:proto-pool-brep-edit`: the pool has no B-rep it can edit, so that is a gap
//! of this harness, not of a plan unit (no case reaches it).

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};

use solidcore::curve::composite_curve::p1::CompositeCurve;
use solidcore::curve::crv_on_surf::CrvOnSurf;
use solidcore::curve::curve::p2::Curve;
use solidcore::curve::ellipse::{Ellipse, EllipseOptions};
use solidcore::curve::hermite_curve::HermiteCurve;
use solidcore::curve::line::{BoundedLineOptions, Line};
use solidcore::geom::a_object::CacheStore;
use solidcore::geom::attribute::{Attribute, AttributeStore, Behavior, ObjectKey};
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::object::ObjectRef;
use solidcore::geom::tol::{xsect_tol3d, zone_tol3d};
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::{CurveClassification, CurveInterval};
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassType, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::{Point2, Point3, EFF_ZERO};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface::BSplineSurface;
use solidcore::nurbs::math::CPoint;
use solidcore::nurbs::nurbs_crv::allocate_nurb_curve;
use solidcore::nurbs::srf_basic::Surface as NurbSurface;
use solidcore::query::poly::{PolyBrep, PolyNotifyEnv, PolyObjectKind};
use solidcore::surface::plane::Plane;
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::proto_topology::p1::api_transform::{
    self as api, PolyBrepTarget, PolyBrepTransform, TransformObject,
};
use solidcore::topology::proto_topology::p1::assembly::{
    self as asm_mod, Assembly, AssemblyInstance,
};
use solidcore::topology::proto_topology::p1::owning_topology as own_mod;
use solidcore::topology::proto_topology::p1::proto::{self as proto_mod};
use solidcore::topology::proto_topology::p1::{
    destroy_topology, CopyBrepMap, EdgeDefId, EdgeDefinition, OwningRecords, OwningTopology,
    ProtoEdgeId, ProtoFaceId, ProtoGeometry, ProtoTopology, ProtoTopologyManager, ProtoVertexId,
    VertexDefId, VertexDefinition,
};
use solidcore::topology::proto_topology::p2::ProtoBrep;
use solidcore::topology::topology::{Topology, TopologyRecords};
use solidcore::topology::edge as topo_edge;
use solidcore::topology::face::p2 as topo_face;
use solidcore::topology::vertex as topo_vertex;
use solidcore::topology::{Brep, Edge, Face, Vertex, VertexId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::topology_proto_topology_p1::{
    self as ffi, kind, list_op as lop, pool as pk, proto_op as p, proto_op_p2 as q, slot, tobj,
    PROBE_TYPES,
};

use crate::case::{CaseContext, CaseSource};
use crate::runner::default_cases;
use crate::{Case, HarnessError, Output, Rng, UnitDiff};

/// Generated cases per run, whatever `--seeds` says (it can only raise this).
pub const MIN_GENERATED: usize = 1500;

/// `topology.proto_topology_p1` runner.
pub struct Runner;

pub(super) fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

pub(super) const NOZ: f64 = f64::MAX;
pub(super) const NOW: f64 = -f64::MAX;
/// `SM_USE_DEFAULT`.
pub(super) const USE_DEFAULT: f64 = 9876543.21;
/// `SC_ERR_ARG`.
pub(super) const SC_ERR_ARG: i32 = -1002;
/// `SM_SUCCESS`.
pub(super) const SM_SUCCESS: f64 = 1000.0;
/// The status an op refused by the shim writes.
pub(super) const SKIP: f64 = ffi::SKIPPED;

// ---- inputs -----------------------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct BSplineIn {
    pub(super) dim: u64,
    #[serde(default)]
    pub(super) oob: bool,
    pub(super) cpts: Vec<[f64; 4]>,
    pub(super) p: usize,
    pub(super) u: Vec<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct HermiteIn {
    pub(super) dim: u64,
    pub(super) p1: [f64; 3],
    pub(super) d1: [f64; 3],
    pub(super) p2: [f64; 3],
    pub(super) d2: [f64; 3],
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct LineIn {
    pub(super) p: [f64; 3],
    pub(super) v: [f64; 3],
    pub(super) domain: [f64; 2],
    #[serde(default = "one")]
    pub(super) scale: f64,
    #[serde(default)]
    pub(super) inside_out: bool,
}

fn one() -> f64 {
    1.0
}

/// An `SmEllipse` or, without `ry`, an `SmCircle`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct ConicIn {
    pub(super) center: [f64; 3],
    pub(super) x: [f64; 3],
    pub(super) y: [f64; 3],
    pub(super) domain: [f64; 2],
    pub(super) rx: f64,
    pub(super) ry: Option<f64>,
    #[serde(default)]
    pub(super) inside_out: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum CurveIn {
    Bspline(BSplineIn),
    Hermite(HermiteIn),
    Line(LineIn),
    Conic(ConicIn),
}

/// A B-spline surface: control net `net[i][j]` (`x y z w`, `w = NOW` non-rational).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct SurfIn {
    #[serde(default)]
    pub(super) oob: bool,
    pub(super) net: Vec<Vec<[f64; 4]>>,
    pub(super) p: usize,
    pub(super) q: usize,
    pub(super) u: Vec<f64>,
    pub(super) v: Vec<f64>,
}

/// A proto-script pool object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum ObjIn {
    Curve(CurveIn),
    Plane {
        o: [f64; 3],
        n: [f64; 3],
    },
    Bsurf(SurfIn),
    Vertex,
    /// A vertex with a point and a stored zone tolerance.
    VertexAt {
        p: [f64; 3],
        tol: f64,
    },
    /// A bare edge on pool curve `c`.
    Edge(usize),
    /// A bare face on pool surface `s`.
    Face(usize),
    Region,
    Loop,
    /// A bare edge on pool curve `c` with interval `[a, b]` and zone tolerance `tol`.
    EdgeIvl {
        c: usize,
        a: f64,
        b: f64,
        tol: f64,
    },
    /// A bare vertex with zone tolerance `tol`.
    VertexTol(f64),
}

/// A transform-script object.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TObjIn {
    Curve {
        owned: bool,
        c: CurveIn,
    },
    Plane {
        owned: bool,
        o: [f64; 3],
        n: [f64; 3],
    },
    Bsurf {
        owned: bool,
        s: SurfIn,
    },
    Brep,
    /// An empty `SmPolyBrep`.
    Poly,
    /// An `SmPolyBrep` with poly vertices `x y z tol` (1 to 4).
    PolyPts {
        pts: Vec<[f64; 4]>,
    },
    Other,
    Null,
    /// An `SmCompositeCurve` of the curves.
    Composite {
        owned: bool,
        cs: Vec<CurveIn>,
    },
    /// An `SmCrvOnSurf`: the UV line from `a` to `b` on a B-spline surface.
    CrvOnSurf {
        owned: bool,
        s: SurfIn,
        a: [f64; 2],
        b: [f64; 2],
    },
}

/// A transform call.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub(super) enum TOpIn {
    Transform {
        o: [f64; 3],
        x: [f64; 3],
        y: [f64; 3],
    },
    Scale {
        s: [f64; 3],
    },
    ScaleByPt {
        r: [f64; 3],
        s: [f64; 3],
    },
    Translate {
        v: [f64; 3],
    },
    Rotate {
        r: [f64; 3],
        a: [f64; 3],
        deg: f64,
    },
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "script", rename_all = "snake_case")]
pub(super) enum Script {
    /// Slot kinds (0 owning, 1 assembly, 2 instance slot) and `[op, a, b, c, d]` ops.
    List { slots: Vec<i64>, ops: Vec<[i64; 5]> },
    /// Pool, manager parameters and `[code, args...]` ops.
    Proto {
        objects: Vec<ObjIn>,
        approx_tol: f64,
        capture: f64,
        override_tol: f64,
        ops: Vec<Vec<f64>>,
    },
    /// One object and one API call.
    Transform { obj: TObjIn, op: TOpIn },
}

// ---- encoding for the C++ side ------------------------------------------------------------

pub(super) fn b(x: bool) -> f64 {
    f64::from(u8::from(x))
}

fn curve_block(v: &mut Vec<f64>, c: &CurveIn) {
    match c {
        CurveIn::Bspline(bs) => {
            let n = bs.cpts.len().saturating_sub(1);
            let m = bs.u.len().saturating_sub(1);
            v.extend([0.0, bs.dim as f64, b(bs.oob)]);
            v.extend([n as f64, bs.p as f64, m as f64]);
            for c in &bs.cpts {
                v.extend_from_slice(c);
            }
            v.extend_from_slice(&bs.u);
        }
        CurveIn::Hermite(h) => {
            v.extend([1.0, h.dim as f64]);
            for p in [h.p1, h.d1, h.p2, h.d2] {
                v.extend_from_slice(&p);
            }
        }
        CurveIn::Conic(c) => {
            v.extend([if c.ry.is_some() { 3.0 } else { 4.0 }, 3.0]);
            v.extend_from_slice(&c.center);
            v.extend_from_slice(&c.x);
            v.extend_from_slice(&c.y);
            v.extend(c.domain);
            v.push(c.rx);
            if let Some(ry) = c.ry {
                v.push(ry);
            }
            v.push(b(c.inside_out));
        }
        CurveIn::Line(l) => {
            v.extend([2.0, 3.0]);
            v.extend_from_slice(&l.p);
            v.extend_from_slice(&l.v);
            v.extend([l.domain[0], l.domain[1], l.scale, b(l.inside_out)]);
        }
    }
}

fn surface_block(v: &mut Vec<f64>, s: &SurfIn) {
    let n = s.net.len().saturating_sub(1);
    let m = s.net.first().map_or(0, Vec::len).saturating_sub(1);
    v.push(b(s.oob));
    v.extend([
        n as f64,
        m as f64,
        s.p as f64,
        s.q as f64,
        s.u.len().saturating_sub(1) as f64,
        s.v.len().saturating_sub(1) as f64,
    ]);
    for row in &s.net {
        for c in row {
            v.extend_from_slice(c);
        }
    }
    v.extend_from_slice(&s.u);
    v.extend_from_slice(&s.v);
}

pub(super) fn cpp_input(s: &Script) -> Vec<f64> {
    let mut v = Vec::new();
    match s {
        Script::List { slots, ops } => {
            v.extend([kind::LIST as f64, slots.len() as f64]);
            v.extend(slots.iter().map(|&k| k as f64));
            v.push(ops.len() as f64);
            for o in ops {
                v.extend(o.iter().map(|&x| x as f64));
            }
        }
        Script::Proto {
            objects,
            approx_tol,
            capture,
            override_tol,
            ops,
        } => {
            v.extend([kind::PROTO as f64, objects.len() as f64]);
            for o in objects {
                match o {
                    ObjIn::Curve(c) => {
                        v.push(pk::CURVE as f64);
                        curve_block(&mut v, c);
                    }
                    ObjIn::Plane { o, n } => {
                        v.push(pk::PLANE as f64);
                        v.extend_from_slice(o);
                        v.extend_from_slice(n);
                    }
                    ObjIn::Bsurf(sf) => {
                        v.push(pk::BSURF as f64);
                        surface_block(&mut v, sf);
                    }
                    ObjIn::Vertex => v.push(pk::VERTEX as f64),
                    ObjIn::VertexAt { p, tol } => {
                        v.push(pk::VERTEX_AT as f64);
                        v.extend_from_slice(p);
                        v.push(*tol);
                    }
                    ObjIn::Edge(c) => v.extend([pk::EDGE as f64, *c as f64]),
                    ObjIn::Face(sf) => v.extend([pk::FACE as f64, *sf as f64]),
                    ObjIn::Region => v.push(pk::REGION as f64),
                    ObjIn::Loop => v.push(pk::LOOP as f64),
                    ObjIn::EdgeIvl { c, a, b: bb, tol } => {
                        v.extend([pk::EDGE_IVL as f64, *c as f64, *a, *bb, *tol]);
                    }
                    ObjIn::VertexTol(tol) => v.extend([pk::VERTEX_TOL as f64, *tol]),
                }
            }
            v.extend([*approx_tol, *capture, *override_tol, ops.len() as f64]);
            for o in ops {
                v.extend_from_slice(o);
            }
        }
        Script::Transform { obj, op } => {
            v.push(kind::TRANSFORM as f64);
            match obj {
                TObjIn::Curve { owned, c } => {
                    v.extend([tobj::CURVE as f64, b(*owned)]);
                    curve_block(&mut v, c);
                }
                TObjIn::Plane { owned, o, n } => {
                    v.extend([tobj::PLANE as f64, b(*owned)]);
                    v.extend_from_slice(o);
                    v.extend_from_slice(n);
                }
                TObjIn::Bsurf { owned, s } => {
                    v.extend([tobj::BSURF as f64, b(*owned)]);
                    surface_block(&mut v, s);
                }
                TObjIn::Brep => v.push(tobj::BREP as f64),
                TObjIn::Poly => v.push(tobj::POLY as f64),
                TObjIn::PolyPts { pts } => {
                    v.extend([tobj::POLY_PTS as f64, pts.len() as f64]);
                    for q in pts {
                        v.extend_from_slice(q);
                    }
                }
                TObjIn::Other => v.push(tobj::OTHER as f64),
                TObjIn::Null => v.push(tobj::NULL as f64),
                TObjIn::Composite { owned, cs } => {
                    v.extend([tobj::COMPOSITE as f64, b(*owned), cs.len() as f64]);
                    for c in cs {
                        curve_block(&mut v, c);
                    }
                }
                TObjIn::CrvOnSurf { owned, s, a, b: bb } => {
                    v.extend([tobj::CRV_ON_SURF as f64, b(*owned)]);
                    surface_block(&mut v, s);
                    v.extend([a[0], a[1], bb[0], bb[1]]);
                }
            }
            match op {
                TOpIn::Transform { o, x, y } => {
                    v.push(0.0);
                    for q in [o, x, y] {
                        v.extend_from_slice(q);
                    }
                }
                TOpIn::Scale { s } => {
                    v.push(1.0);
                    v.extend_from_slice(s);
                }
                TOpIn::ScaleByPt { r, s } => {
                    v.push(2.0);
                    v.extend_from_slice(r);
                    v.extend_from_slice(s);
                }
                TOpIn::Translate { v: t } => {
                    v.push(3.0);
                    v.extend_from_slice(t);
                }
                TOpIn::Rotate { r, a, deg } => {
                    v.push(4.0);
                    v.extend_from_slice(r);
                    v.extend_from_slice(a);
                    v.push(*deg);
                }
            }
        }
    }
    v
}

pub(super) fn cpp(s: &Script) -> Result<Output, HarnessError> {
    let out = ffi::run(&cpp_input(s)).map_err(|e| harness(format!("c++: {e}")))?;
    Ok(Output::new().values("out", out))
}

// ---- geometry -----------------------------------------------------------------------------

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

pub(super) fn build_curve(c: &CurveIn) -> Result<Curve, HarnessError> {
    match c {
        CurveIn::Bspline(bs) => {
            let n = bs
                .cpts
                .len()
                .checked_sub(1)
                .ok_or_else(|| harness("no points"))?;
            let m =
                bs.u.len()
                    .checked_sub(1)
                    .ok_or_else(|| harness("no knots"))?;
            let mut cur =
                allocate_nurb_curve(n, bs.p, m).map_err(|e| harness(format!("curve: {e}")))?;
            for (d, s) in cur.cpts_mut().iter_mut().zip(&bs.cpts) {
                d.x = s[0];
                d.y = s[1];
                d.z = s[2];
                d.w = s[3];
            }
            cur.knots_mut().copy_from_slice(&bs.u);
            let mut c = BSplineCurve::from_nurb(cur, bs.dim, false);
            c.set_out_of_bounds_enabled(bs.oob);
            Ok(Curve::from(c))
        }
        CurveIn::Hermite(h) => Ok(Curve::from(HermiteCurve::new(
            v3(h.p1),
            v3(h.d1),
            v3(h.p2),
            v3(h.d2),
            h.dim,
        ))),
        CurveIn::Line(l) => {
            let opts = BoundedLineOptions {
                scale: l.scale,
                inside_out: l.inside_out,
                ..BoundedLineOptions::default()
            };
            Ok(Curve::from(Line::bounded(
                v3(l.p),
                v3(l.v),
                &Extent1d::from_min_max(l.domain[0], l.domain[1]),
                &opts,
            )))
        }
        CurveIn::Conic(c) => {
            let k = Kernel::default();
            let opts = EllipseOptions {
                dim: 3,
                nurb: None,
                inside_out: c.inside_out,
            };
            let domain = Extent1d::from_min_max(c.domain[0], c.domain[1]);
            let e = match c.ry {
                Some(ry) => {
                    Ellipse::new(&k, v3(c.center), v3(c.x), v3(c.y), domain, c.rx, ry, &opts)
                }
                None => {
                    Ellipse::new_circle(&k, v3(c.center), v3(c.x), v3(c.y), domain, c.rx, &opts)
                }
            }
            .map_err(|e| harness(format!("conic: {e}")))?;
            Ok(Curve::from(e))
        }
    }
}

fn build_surface(s: &SurfIn) -> Result<Surface, HarnessError> {
    let net = s
        .net
        .iter()
        .map(|row| {
            row.iter()
                .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                .collect()
        })
        .collect();
    let sur = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
        .map_err(|e| harness(format!("surface: {e}")))?;
    let mut bs = BSplineSurface::from_nurb(sur, false);
    *bs.out_of_bounds_enabled_mut() = s.oob;
    Ok(Surface::from(bs))
}

/// The `SmStatus` of a Rust error; `-1` for one upstream cannot return.
pub(super) fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -1.0,
    }
}

/// A Rust failure: deferred for `Unported`, a harness error otherwise.
pub(super) enum Fail {
    Deferred(Box<Output>),
    Harness(HarnessError),
}

impl From<HarnessError> for Fail {
    fn from(e: HarnessError) -> Self {
        Fail::Harness(e)
    }
}

/// `Ok(v)` or the status; `Unported` defers the case.
pub(super) fn st<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Ok(v) => Ok(Ok(v)),
        Err(e) => match Output::unported(&e) {
            Some(o) => Err(Fail::Deferred(Box::new(o))),
            None => Ok(Err(status(&e))),
        },
    }
}

/// The status marking a refusal: undefined behaviour upstream (the shim refuses those inputs).
const REFUSED: f64 = f64::MIN;

/// `st` for the unit `topology.proto_topology_p3` ops: an [`SolidError::InvalidInput`] (a
/// dereference of a null or deleted object, a read of an uninitialised value upstream) is a
/// refusal, as the shim refuses those inputs.
fn st3<T>(r: solidcore::Result<T>) -> Result<Result<T, f64>, Fail> {
    match r {
        Err(SolidError::InvalidInput { .. }) => Ok(Err(REFUSED)),
        r => st(r),
    }
}

/// The status of a call (0 on success).
fn st0<T>(r: solidcore::Result<T>) -> Result<f64, Fail> {
    Ok(match st(r)? {
        Ok(_) => 0.0,
        Err(s) => s,
    })
}

pub(super) fn put_vec(o: &mut Vec<f64>, v: Vec3) {
    o.extend([v.x, v.y, v.z]);
}

pub(super) fn put_curve(k: &Kernel, o: &mut Vec<f64>, c: &Curve) {
    let ivl = c.natural_interval();
    o.extend([ivl.min(), ivl.max()]);
    for i in 0..=4 {
        match c.evaluate_point(k, ivl.evaluate(f64::from(i) / 4.0)) {
            Ok(p) => {
                o.push(0.0);
                put_vec(o, p);
            }
            Err(e) => o.push(status(&e)),
        }
    }
}

fn put_surface(k: &Kernel, o: &mut Vec<f64>, s: &Surface) {
    let d = s.natural_uv_domain();
    o.extend([d.u_min(), d.u_max(), d.v_min(), d.v_max()]);
    for i in 0..=2 {
        for j in 0..=2 {
            let uv = Point2::new(
                d.u_min() + (d.u_max() - d.u_min()) * (f64::from(i) / 2.0),
                d.v_min() + (d.v_max() - d.v_min()) * (f64::from(j) / 2.0),
            );
            match s.evaluate_point(k, uv) {
                Ok(p) => {
                    o.push(0.0);
                    put_vec(o, p);
                }
                Err(e) => o.push(status(&e)),
            }
        }
    }
}

fn put_string(o: &mut Vec<f64>, s: &str) {
    o.push(s.len() as f64);
    o.extend(s.bytes().map(f64::from));
}

fn put_type(o: &mut Vec<f64>, ty: i64, is_kind_of: fn(i64) -> bool, ts: &str, cs: &str) {
    o.push(ty as f64);
    for t in PROBE_TYPES {
        o.push(b(is_kind_of(t)));
    }
    put_string(o, ts);
    put_string(o, cs);
}

// ---- LIST scripts ---------------------------------------------------------------------------

/// One list-script object: its topology record, its list data and its kind.
struct Slot {
    kind: i64,
    rec: Option<Topology>,
    list: OwningTopology,
    alive: bool,
    assembly: Option<Assembly>,
    instance: Option<AssemblyInstance>,
}

struct ListStore {
    slots: Vec<Slot>,
}

impl TopologyRecords for ListStore {
    fn topology(&self, key: ObjectKey) -> Option<&Topology> {
        self.slots.get(usize::try_from(key.0).ok()?)?.rec.as_ref()
    }
    fn topology_mut(&mut self, key: ObjectKey) -> Option<&mut Topology> {
        self.slots
            .get_mut(usize::try_from(key.0).ok()?)?
            .rec
            .as_mut()
    }
}

impl OwningRecords for ListStore {
    fn owning(&self, key: ObjectKey) -> Option<OwningTopology> {
        let s = self.slots.get(usize::try_from(key.0).ok()?)?;
        s.rec.as_ref().map(|_| s.list)
    }
    fn set_owning(&mut self, key: ObjectKey, owning: OwningTopology) -> Option<()> {
        let s = self.slots.get_mut(usize::try_from(key.0).ok()?)?;
        s.rec.as_ref()?;
        s.list = owning;
        Some(())
    }
}

fn key(i: i64) -> ObjectKey {
    ObjectKey(i as u64)
}

impl ListStore {
    fn live(&self, i: i64) -> bool {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.slots.get(i))
            .is_some_and(|s| s.rec.is_some() && s.alive)
    }
    fn obj(j: i64) -> Option<ObjectKey> {
        (j != -1).then(|| key(j))
    }
    fn bad(&self, j: i64, null_ok: bool) -> bool {
        if j == -1 {
            !null_ok
        } else {
            !self.live(j)
        }
    }
    fn kind(&self, i: i64) -> i64 {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.slots.get(i))
            .map_or(-1, |s| s.kind)
    }
    fn id(&self, k: Option<ObjectKey>) -> f64 {
        match k {
            None => -1.0,
            Some(k) => match usize::try_from(k.0).ok().and_then(|i| self.slots.get(i)) {
                Some(s) if s.rec.is_some() => {
                    if s.alive {
                        k.0 as f64
                    } else {
                        -3.0
                    }
                }
                _ => -2.0,
            },
        }
    }
    fn is_live_key(&self, k: ObjectKey) -> bool {
        self.id(Some(k)) >= 0.0
    }
    fn rec(&self, k: ObjectKey) -> Topology {
        self.topology(k).copied().unwrap_or_default()
    }
    fn list(&self, i: i64) -> OwningTopology {
        usize::try_from(i)
            .ok()
            .and_then(|i| self.slots.get(i))
            .map(|s| s.list)
            .unwrap_or_default()
    }
    fn unlinked(&self, k: ObjectKey) -> bool {
        let r = self.rec(k);
        r.next().is_none() && r.last().is_none() && r.owner().is_none()
    }
    fn walk_terminates(&self, i: i64, null_ok: bool) -> bool {
        let Some(h) = self.list(i).list() else {
            return true;
        };
        let mut p = h;
        for _ in 0..=self.slots.len() + 1 {
            match self.rec(p).next() {
                None => return null_ok,
                Some(n) => p = n,
            }
            if p == h {
                return true;
            }
        }
        false
    }
    fn invariant(&self) -> bool {
        for (i, s) in self.slots.iter().enumerate() {
            if !self.live(i as i64) {
                continue;
            }
            let Some(r) = s.rec else { continue };
            let (n, l, w) = (r.next(), r.last(), r.owner());
            if n.is_none() != l.is_none() {
                return false;
            }
            let bad = |x: Option<ObjectKey>| x.is_some_and(|x| !self.is_live_key(x));
            if bad(n) || bad(l) || bad(w) {
                return false;
            }
            let me = Some(key(i as i64));
            if let Some(n) = n {
                if self.rec(n).last() != me {
                    return false;
                }
            }
            if let Some(l) = l {
                if self.rec(l).next() != me {
                    return false;
                }
            }
            if bad(s.list.list()) {
                return false;
            }
        }
        true
    }
}

#[allow(clippy::too_many_lines)]
fn run_list(slots: &[i64], ops: &[[i64; 5]]) -> Result<Vec<f64>, Fail> {
    let mut o = Vec::new();
    let mut st_ = ListStore { slots: Vec::new() };
    for &k in slots {
        let (rec, assembly) = match k {
            slot::OWNING => (Some(Topology::new()), None),
            slot::ASSEMBLY => (Some(Topology::new()), Some(Assembly::new(Some("assembly")))),
            slot::INSTANCE => (None, None),
            _ => return Err(harness("slot kind").into()),
        };
        st_.slots.push(Slot {
            kind: k,
            rec,
            list: OwningTopology::new(),
            alive: true,
            assembly,
            instance: None,
        });
    }
    let mut map: CopyBrepMap<ObjectKey> = CopyBrepMap::new();
    // the shim's record of what was set (its refusals of GetAt)
    let mut entries: BTreeMap<ObjectKey, ObjectKey> = BTreeMap::new();
    let mut attrs = AttributeStore::new();

    for &[op, a, bb, c, d] in ops {
        o.push(op as f64);
        let mut out: Vec<f64> = Vec::new();
        let mut status = 0.0;
        let mut skip = false;
        let s = &mut st_;
        match op {
            lop::PRE_INSERT | lop::POST_INSERT => {
                if s.bad(a, false) || s.bad(bb, false) {
                    skip = true;
                } else {
                    let head = s.list(a).list();
                    if !s.unlinked(key(bb)) || head.is_some_and(|h| s.rec(h).last().is_none()) {
                        skip = true;
                    } else {
                        let mut l = s.list(a);
                        let r = if op == lop::PRE_INSERT {
                            l.pre_insert(key(a), s, key(bb))
                        } else {
                            l.post_insert(key(a), s, key(bb))
                        };
                        let _ = s.set_owning(key(a), l);
                        status = st0(r)?;
                    }
                }
            }
            lop::INSERT_AFTER => {
                if s.bad(a, false)
                    || s.bad(bb, true)
                    || s.bad(c, true)
                    || ListStore::obj(bb).is_some_and(|x| !s.unlinked(x))
                {
                    skip = true;
                } else {
                    let mut l = s.list(a);
                    let (x, y) = (ListStore::obj(bb), ListStore::obj(c));
                    let r = l.insert_after(s, x, y);
                    let _ = s.set_owning(key(a), l);
                    status = st0(r)?;
                }
            }
            lop::REMOVE => {
                if s.bad(a, false) || s.bad(bb, false) {
                    skip = true;
                } else {
                    let mut l = s.list(a);
                    let r = l.remove(key(a), s, key(bb));
                    let _ = s.set_owning(key(a), l);
                    status = st0(r)?;
                }
            }
            lop::IS_IN_LIST => {
                if s.bad(a, false) || s.bad(bb, true) || !s.walk_terminates(a, true) {
                    skip = true;
                } else {
                    match st(s.list(a).is_in_list(s, ListStore::obj(bb)))? {
                        Ok((f, idx)) => out.extend([b(f), idx as f64]),
                        Err(e) => out.extend([e, -1.0]),
                    }
                }
            }
            lop::GET_ALL => {
                if s.bad(a, false)
                    || s.bad(c, true)
                    || ((bb != -1 || c != -1) && !s.walk_terminates(a, true))
                {
                    skip = true;
                } else {
                    let attr = (bb != -1).then_some(bb as u64);
                    let l = s.list(a);
                    status = st0(l.get_all(s, &attrs, attr, ListStore::obj(c)))?;
                    match st(l.get_all_ignoring_status(s, &attrs, attr, ListStore::obj(c)))? {
                        Ok(items) => {
                            out.push(items.len() as f64);
                            out.extend(items.iter().map(|&x| s.id(Some(x))));
                        }
                        Err(e) => out.push(e),
                    }
                }
            }
            lop::UPDATE_SIZE => {
                if s.bad(a, false) {
                    skip = true;
                } else {
                    let h = s.list(a).list();
                    if h.is_some_and(|h| s.rec(h).next().is_some()) && !s.walk_terminates(a, false)
                    {
                        skip = true;
                    } else {
                        let mut l = s.list(a);
                        let r = l.update_list_size(s);
                        let _ = s.set_owning(key(a), l);
                        status = st0(r)?;
                    }
                }
            }
            lop::REVERSE => {
                if s.bad(a, false) || s.list(a).list().is_none() || !s.walk_terminates(a, false) {
                    skip = true;
                } else {
                    let l = s.list(a);
                    status = st0(l.reverse_list(s))?;
                }
            }
            lop::SET_LIST => {
                if s.bad(a, false) || s.bad(bb, true) {
                    skip = true;
                } else {
                    let mut l = s.list(a);
                    l.set_list(ListStore::obj(bb));
                    let _ = s.set_owning(key(a), l);
                }
            }
            lop::DESTROY => {
                if s.bad(a, false) {
                    skip = true;
                } else {
                    let k = s.kind(a);
                    if k == slot::ASSEMBLY && !s.walk_terminates(a, true) {
                        skip = true;
                    } else {
                        let r = match k {
                            slot::OWNING => {
                                let mut l = s.list(a);
                                l.destroy();
                                let _ = s.set_owning(key(a), l);
                                destroy_topology(s, key(a))
                            }
                            slot::ASSEMBLY => {
                                let x = s.slots.get_mut(a as usize).and_then(|x| x.assembly.take());
                                match x {
                                    Some(x) => x.destroy(s, key(a)),
                                    None => Err(SolidError::invalid_input("runner", "no assembly")),
                                }
                            }
                            _ => {
                                let x = s.slots.get_mut(a as usize).and_then(|x| x.instance.take());
                                match x {
                                    Some(x) => x.destroy(s, key(a)),
                                    None => Err(SolidError::invalid_input("runner", "no instance")),
                                }
                            }
                        };
                        status = st0(r)?;
                        if let Some(x) = s.slots.get_mut(a as usize) {
                            x.alive = false;
                        }
                    }
                }
            }
            lop::ASM_INSTANCES => {
                if s.bad(a, false) || s.kind(a) != slot::ASSEMBLY || !s.walk_terminates(a, true) {
                    skip = true;
                } else {
                    match st(Assembly::assembly_instances(s, key(a)))? {
                        Ok(items) => {
                            out.push(items.len() as f64);
                            out.extend(items.iter().map(|&x| s.id(Some(x))));
                        }
                        Err(e) => out.push(e),
                    }
                }
            }
            lop::ASM_REMOVE_INSTANCE => {
                if s.bad(a, false)
                    || s.bad(bb, false)
                    || s.kind(a) != slot::ASSEMBLY
                    || s.kind(bb) != slot::INSTANCE
                {
                    skip = true;
                } else {
                    status = st0(Assembly::remove_instance(s, key(a), key(bb)))?;
                }
            }
            lop::INST_NEW => {
                let empty = usize::try_from(a)
                    .ok()
                    .and_then(|i| s.slots.get(i))
                    .is_some_and(|x| x.kind == slot::INSTANCE && x.rec.is_none());
                if !empty
                    || s.bad(bb, false)
                    || s.kind(bb) != slot::ASSEMBLY
                    || s.bad(c, true)
                    || (c != -1 && s.kind(c) == slot::OWNING)
                    || s.list(bb).list().is_some_and(|h| s.rec(h).last().is_none())
                {
                    skip = true;
                } else {
                    if let Some(x) = s.slots.get_mut(a as usize) {
                        x.rec = Some(Topology::new());
                        x.list = OwningTopology::new();
                    }
                    let comp = ListStore::obj(c);
                    let r = AssemblyInstance::new(
                        s,
                        key(a),
                        Some("instance"),
                        key(bb),
                        comp,
                        Axis2Placement::default(),
                        Vec3::new(1.0, 1.0, 1.0),
                        d != 0,
                    );
                    match st(r)? {
                        Ok(inst) => {
                            out.push(s.id(inst.component()));
                            out.push(b(inst.mirrored()));
                            if let Some(x) = s.slots.get_mut(a as usize) {
                                x.instance = Some(inst);
                            }
                        }
                        Err(e) => status = e,
                    }
                }
            }
            lop::INST_PARENT => {
                if s.bad(a, false) || s.kind(a) != slot::INSTANCE {
                    skip = true;
                } else {
                    match st(AssemblyInstance::parent_assembly(s, key(a)))? {
                        Ok(p) => out.push(s.id(p)),
                        Err(e) => status = e,
                    }
                }
            }
            lop::MAP_SET => {
                if s.bad(a, false) || s.bad(bb, false) {
                    skip = true;
                } else {
                    map.set_at(key(a), key(bb));
                    entries.insert(key(a), key(bb));
                }
            }
            lop::MAP_GET => {
                if s.bad(a, true)
                    || (a != -1 && entries.get(&key(a)).is_none_or(|&g| !s.is_live_key(g)))
                {
                    skip = true;
                } else {
                    match st(map.get_at(ListStore::obj(a)))? {
                        Ok(g) => out.push(s.id(g)),
                        Err(e) => status = e,
                    }
                }
            }
            lop::MAP_BREPS => {
                if s.bad(a, true) || s.bad(bb, true) {
                    skip = true;
                } else {
                    map.set_breps(ListStore::obj(a), ListStore::obj(bb));
                }
            }
            lop::ADD_ATTR => {
                if s.bad(a, false) || !(0..=1000).contains(&bb) {
                    skip = true;
                } else {
                    let h = attrs
                        .insert(Attribute::new(bb as u64, Behavior::StandaloneCopy))
                        .map_err(|e| harness(format!("attribute: {e}")))?;
                    let _ = attrs.add_attribute(key(a), h);
                }
            }
            lop::TYPE => {
                if s.bad(a, false) {
                    skip = true;
                } else {
                    match s.kind(a) {
                        slot::OWNING => put_type(
                            &mut out,
                            own_mod::TYPE,
                            own_mod::is_kind_of,
                            own_mod::TYPE_STRING,
                            own_mod::CLASS_STRING,
                        ),
                        slot::ASSEMBLY => put_type(
                            &mut out,
                            asm_mod::ASSEMBLY_TYPE,
                            asm_mod::assembly_is_kind_of,
                            asm_mod::ASSEMBLY_TYPE_STRING,
                            asm_mod::ASSEMBLY_CLASS_STRING,
                        ),
                        _ => put_type(
                            &mut out,
                            asm_mod::ASSEMBLY_INSTANCE_TYPE,
                            asm_mod::assembly_instance_is_kind_of,
                            asm_mod::ASSEMBLY_INSTANCE_TYPE_STRING,
                            asm_mod::ASSEMBLY_INSTANCE_CLASS_STRING,
                        ),
                    }
                }
            }
            lop::GET_SIZE => {
                if s.bad(a, false) {
                    skip = true;
                } else {
                    let l = s.list(a);
                    out.push(l.size() as f64);
                    out.push(s.id(l.list()));
                }
            }
            _ => return Err(harness("list op").into()),
        }
        if !skip && !st_.invariant() {
            skip = true;
        }
        if skip {
            o.push(SKIP);
            return Ok(o);
        }
        o.push(status);
        o.push(out.len() as f64);
        o.extend(out);
    }
    for i in 0..st_.slots.len() {
        if !st_.live(i as i64) {
            o.push(0.0);
            continue;
        }
        let r = st_.rec(key(i as i64));
        let l = st_.list(i as i64);
        o.push(1.0);
        o.extend([
            st_.id(r.owner()),
            st_.id(r.next()),
            st_.id(r.last()),
            st_.id(l.list()),
            l.size() as f64,
        ]);
    }
    o.push(st_.id(map.orig_brep()));
    o.push(st_.id(map.copy_brep()));
    Ok(o)
}

// ---- PROTO scripts --------------------------------------------------------------------------

/// A pool handle: index and kind.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) struct H(pub(super) usize, pub(super) ClassObjectKind);

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        self.1
    }
}

pub(super) enum PObj {
    Curve(Curve),
    Surface(Surface),
    /// A bare edge: its curve's pool index, interval and zone tolerance.
    Edge {
        curve: usize,
        ivl: Extent1d,
        tol: f64,
    },
    /// A bare vertex or face: its zone tolerance.
    Tol(f64),
    /// A vertex with a point and a zone tolerance.
    VertexPt(Point3, f64),
    Bare,
}

pub(super) struct Pool {
    pub(super) objs: Vec<PObj>,
    pub(super) kinds: Vec<i64>,
    pub(super) alive: Vec<bool>,
    /// The vertex, edge and face stand-ins as records of one B-rep ([`TopoRecords`]).
    pub(super) topo: TopoRecords,
}

/// The shim's vertex, edge and face stand-ins as records of one B-rep, so that topology
/// handles resolve through the [`ClassGeometry`] hooks to the ported topology members
/// (as `intersect.curve_class_p1`'s `BareTopology`). The shim builds default-constructed
/// `SmVertex` / `SmEdge` / `SmFace` objects with only the point, interval, tolerance,
/// `m_pCurve` / `m_pSurface` set, never joined to a B-rep; the records are in that state:
/// no uses, an edge sharing its pool curve (kept in step by [`Pool::sync_topo`]), a face
/// with its surface and nothing else (its shell is a placeholder no list names).
#[derive(Default)]
pub(super) struct TopoRecords {
    brep: Brep,
    /// Per pool object: its entity (vertex, edge and face stand-ins).
    ents: Vec<Option<EntityRef>>,
    /// Per pool object: for an edge, the pool index of its curve.
    edge_curve: Vec<Option<usize>>,
}

impl TopoRecords {
    /// Appends the record of pool object `obj` (kind `kind`; `surface`: a face's surface).
    fn push(
        &mut self,
        obj: &PObj,
        kind: i64,
        curve: Option<&Curve>,
        surface: Option<&Surface>,
    ) -> Result<(), HarnessError> {
        let e = |e: SolidError| harness(e.to_string());
        let mut edge_curve = None;
        let ent = match (kind, obj) {
            (pk::VERTEX, PObj::Tol(tol)) => Some(EntityRef::Vertex(
                self.brep
                    .add_vertex_record(Vertex {
                        tolerance: *tol,
                        ..Vertex::default()
                    })
                    .map_err(e)?,
            )),
            (pk::VERTEX, PObj::VertexPt(p, tol)) => Some(EntityRef::Vertex(
                self.brep
                    .add_vertex_record(Vertex {
                        point: *p,
                        tolerance: *tol,
                        ..Vertex::default()
                    })
                    .map_err(e)?,
            )),
            (pk::EDGE, PObj::Edge { curve: c, ivl, tol }) => {
                edge_curve = Some(*c);
                Some(EntityRef::Edge(
                    self.brep
                        .add_edge_record(Edge {
                            // never read: an edge's vertices are those of its (absent)
                            // edge uses
                            vertices: [VertexId::from_index(u32::MAX); 2],
                            edgeuses: Vec::new(),
                            interval: *ivl,
                            tolerance: *tol,
                            curve: curve.cloned(),
                        })
                        .map_err(e)?,
                ))
            }
            (pk::FACE, PObj::Tol(tol)) => {
                let shell = self.brep.add_shell().map_err(e)?;
                let mut face = Face::new(shell);
                face.surface = surface.cloned();
                face.tolerance = *tol;
                Some(EntityRef::Face(self.brep.add_face_record(face).map_err(e)?))
            }
            _ => None,
        };
        self.ents.push(ent);
        self.edge_curve.push(edge_curve);
        Ok(())
    }

    fn ent(&self, i: usize) -> Option<EntityRef> {
        self.ents.get(i).copied().flatten()
    }
}

/// The pool holds proto-topology objects, not a B-rep arena: it cannot glue. The glue
/// op skips every script whose vertices would be glued (the `p::GLUE` op), so this
/// refusal is never reached by a compared case.
impl solidcore::topology::proto_topology::p3::BrepGlue<H> for Pool {
    fn glue_vertices(
        &mut self,
        _k: &mut Kernel,
        _brep: H,
        _v1: H,
        _v2: H,
        _opt_wire_edges_in_brep: Option<&[H]>,
    ) -> solidcore::Result<()> {
        Err(SolidError::invalid_input(
            "difftest::topology_proto_topology_p1::Pool",
            "the pool has no B-rep arena to glue in",
        ))
    }
    fn delete_edge(&mut self, _k: &mut Kernel, _brep: H, _edge: H) -> solidcore::Result<()> {
        Err(SolidError::invalid_input(
            "difftest::topology_proto_topology_p1::Pool",
            "the pool has no B-rep arena to delete in",
        ))
    }
}

impl ClassGeometry<H> for Pool {
    fn curve(&self, o: H) -> Option<&Curve> {
        if !self.alive.get(o.0).copied().unwrap_or(false) {
            return None;
        }
        match self.objs.get(o.0) {
            Some(PObj::Curve(c)) => Some(c),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match self.objs.get(o.0) {
            Some(PObj::Surface(s)) => Some(s),
            _ => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: H) -> Option<Vec3> {
        None
    }
    fn classification_vertex_point(&self, o: H) -> Option<Vec3> {
        match self.topo_entity(o)? {
            EntityRef::Vertex(v) => topo_vertex::point(&self.topo.brep, v).ok(),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        let b = &self.topo.brep;
        match self.topo_entity(o)? {
            EntityRef::Vertex(v) => topo_vertex::tolerance(b, v).ok(),
            EntityRef::Edge(e) => topo_edge::tolerance(b, e).ok(),
            EntityRef::Face(f) => topo_face::tolerance(b, f).ok(),
            _ => None,
        }
    }
    fn topology_entity(&self, o: H) -> Option<(&Brep, EntityRef)> {
        self.topo_entity(o).map(|e| (&self.topo.brep, e))
    }
    fn topology_handle(&self, e: EntityRef) -> Option<H> {
        let i = self.topo.ents.iter().position(|t| *t == Some(e))?;
        let h = H(i, class_kind_of(self.kinds.get(i).copied().unwrap_or(-1)));
        self.live_h(h).then_some(h)
    }
}

impl Pool {
    /// The entity of live topology stand-in `o`.
    fn topo_entity(&self, o: H) -> Option<EntityRef> {
        if !self.live_h(o) {
            return None;
        }
        self.topo.ent(o.0)
    }

    /// Brings the edge records' curves in step with the pool curves they share (an op
    /// may have edited a pool curve).
    pub(super) fn sync_topo(&mut self) {
        for j in 0..self.topo.ents.len() {
            let (Some(Some(EntityRef::Edge(e))), Some(Some(c))) =
                (self.topo.ents.get(j), self.topo.edge_curve.get(j))
            else {
                continue;
            };
            let curve = match self.objs.get(*c) {
                Some(PObj::Curve(x)) => Some(x.clone()),
                _ => None,
            };
            if let Ok(r) = self.topo.brep.edge_mut(*e) {
                r.curve = curve;
            }
        }
    }
}

impl ProtoGeometry<H> for Pool {
    fn curve_mut(&mut self, o: H) -> Option<&mut Curve> {
        if !self.alive.get(o.0).copied().unwrap_or(false) {
            return None;
        }
        match self.objs.get_mut(o.0) {
            Some(PObj::Curve(c)) => Some(c),
            _ => None,
        }
    }
}

impl Pool {
    /// The point of a live vertex made with one (`VertexAt`, the p3 ops); the p3 guards
    /// only run paths whose vertices have one (the others are refused).
    fn point_of(&self, vertex: H) -> Option<Point3> {
        match self.objs.get(vertex.0) {
            Some(PObj::VertexPt(p, _)) if self.live_h(vertex) => Some(*p),
            _ => None,
        }
    }
}

/// The bare stand-ins' topology: no vertex or edge uses, no B-rep (upstream walks the
/// empty use lists; `SmEdge::GetBrep` reports `SE(SM_ERR)` and returns `NULL`); a bare
/// vertex sits at the origin (`SmVertex()` sets `m_vPoint(0,0,0)`). An edge's start vertex
/// dereferences its missing primary edge use: the shim refuses that (`connect_crashes`).
impl ProtoTopology<H> for Pool {
    fn vertex_use_faces(&self, _vertex: H) -> solidcore::Result<Vec<Option<H>>> {
        Ok(Vec::new())
    }
    fn edge_use_faces(&self, _edge: H) -> solidcore::Result<Vec<Option<H>>> {
        Ok(Vec::new())
    }
    fn edge_start_vertex(&self, _edge: H) -> solidcore::Result<Option<H>> {
        Err(SolidError::invalid_input(
            "difftest::pool",
            "bare edge has no primary edge use",
        ))
    }
    fn edge_other_vertex(&self, _edge: H, _vertex: Option<H>) -> solidcore::Result<Option<H>> {
        Ok(None)
    }
    // A vertex made with a point (`VertexAt`, the p3 ops) sits there.
    fn vertex_point(&self, vertex: H) -> solidcore::Result<Vec3> {
        Ok(self.point_of(vertex).unwrap_or(Vec3::new(0.0, 0.0, 0.0)))
    }
    fn edge_brep(&self, _edge: H) -> solidcore::Result<Option<ObjectKey>> {
        Ok(None)
    }
    fn edge_faces(&self, _edge: H) -> solidcore::Result<Vec<H>> {
        Ok(Vec::new())
    }
}

/// The bare stand-ins' members (upstream `SmEdge::GetInterval`, `GetCurve`, `GetTolerance`
/// of the stand-in objects the shim makes), and new curves appended to the pool.
impl ProtoBrep<H> for Pool {
    fn brep_squeeze_edge(
        &mut self,
        _k: &mut Kernel,
        _brep: H,
        _edge: H,
        _survivor: H,
    ) -> solidcore::Result<()> {
        // the pool's bare stand-ins have no B-rep to edit
        Err(SolidError::unported(
            "SmBrep::SqueezeEdge (bare stand-ins without a B-rep)",
            "harness:proto-pool-brep-edit",
        ))
    }
    fn edge_interval(&self, edge: H) -> Option<Extent1d> {
        match self.objs.get(edge.0) {
            Some(PObj::Edge { ivl, .. }) if self.live_h(edge) => Some(*ivl),
            _ => None,
        }
    }
    fn edge_curve(&self, edge: H) -> Option<Option<H>> {
        match self.objs.get(edge.0) {
            Some(PObj::Edge { curve, .. }) if self.live_h(edge) => {
                Some(Some(H(*curve, ClassObjectKind::Curve)))
            }
            _ => None,
        }
    }
    fn topology_tolerance(&self, topo: H) -> Option<f64> {
        match self.objs.get(topo.0) {
            Some(PObj::Edge { tol, .. } | PObj::Tol(tol) | PObj::VertexPt(_, tol))
                if self.live_h(topo) =>
            {
                Some(*tol)
            }
            _ => None,
        }
    }
    // The bare faces have no UV domain the port reads: `UpdateFaces` defers before
    // (`SmFace::CalculateUVDomainFromUVTrimCurves`, unit topology.face_p1).
    fn face_uv_domain(&self, _face: H) -> Option<Extent2d> {
        None
    }
    fn set_face_uv_domain(&mut self, _face: H, _domain: Extent2d) -> Option<()> {
        None
    }
    fn face_surface(&self, _face: H) -> Option<Option<H>> {
        None
    }
    // The bare stand-ins have no edge uses.
    fn edge_edgeuse_of_face(&self, _edge: H, _face: H) -> solidcore::Result<Option<H>> {
        Ok(None)
    }
    fn edge_is_connected_to_face(&self, _edge: H, _face: H) -> solidcore::Result<bool> {
        Ok(false)
    }
    fn insert_curve(&mut self, curve: Curve) -> Option<H> {
        self.objs.push(PObj::Curve(curve));
        self.kinds.push(pk::CURVE);
        self.alive.push(true);
        self.topo.ents.push(None);
        self.topo.edge_curve.push(None);
        Some(H(self.objs.len() - 1, ClassObjectKind::Curve))
    }
    fn vertex_common_edges(&self, v1: H, v2: H) -> solidcore::Result<Vec<H>> {
        // vertices with points have no vertex uses: no edges
        if self.point_of(v1).is_some() && self.point_of(v2).is_some() {
            return Ok(Vec::new());
        }
        Err(SolidError::invalid_input(
            "difftest::proto_p3::vertex_common_edges",
            "stand-in vertices have no edges",
        ))
    }
    // The stand-ins have no B-rep faces to run `query::face::p2::curve_on_classify` on,
    // so a group with definitions is deferred (the C++ shim cannot build bare faces
    // either; see the p3 resolve guard).
    fn face_curve_on_classify(
        &self,
        _k: &Kernel,
        _face: H,
        _do_point_classify: bool,
        _cc: &mut CurveClassification<H>,
    ) -> solidcore::Result<bool> {
        Err(SolidError::unported(
            "SmFace::CurveOnClassify",
            "query.face_p2",
        ))
    }

    fn brep_point_3d_classify(
        &mut self,
        _k: &Kernel,
        _brep: H,
        _point: Point3,
        _tolerance: f64,
        _boundary_intersections: bool,
    ) -> solidcore::Result<PointClassification<H>> {
        // The pool's handles name no B-rep (the native side classifies against real
        // ones), so the classification is a harness limitation.
        Err(SolidError::unported(
            "SmBrep::Point3DClassify (the proto topology pool holds no B-rep)",
            "harness:proto-topology-pool",
        ))
    }
}

fn class_kind_of(k: i64) -> ClassObjectKind {
    match k {
        pk::CURVE => ClassObjectKind::Curve,
        pk::PLANE | pk::BSURF => ClassObjectKind::Surface,
        pk::VERTEX => ClassObjectKind::Vertex,
        pk::EDGE => ClassObjectKind::Edge,
        pk::FACE => ClassObjectKind::Face,
        pk::REGION => ClassObjectKind::Region,
        pk::LOOP => ClassObjectKind::Loop,
        _ => ClassObjectKind::Other,
    }
}

impl Pool {
    pub(super) fn valid(&self, i: i64) -> bool {
        usize::try_from(i).is_ok_and(|i| self.alive.get(i).copied().unwrap_or(false))
    }
    pub(super) fn is(&self, i: i64, k: i64) -> bool {
        self.valid(i) && self.kinds.get(i as usize) == Some(&k)
    }
    pub(super) fn is_curve(&self, i: i64) -> bool {
        self.is(i, pk::CURVE)
    }
    pub(super) fn is_topo(&self, i: i64) -> bool {
        self.valid(i)
            && matches!(
                self.kinds.get(i as usize),
                Some(&(pk::VERTEX | pk::EDGE | pk::FACE | pk::REGION | pk::LOOP))
            )
    }
    pub(super) fn h(&self, i: i64) -> H {
        H(
            i as usize,
            class_kind_of(self.kinds.get(i as usize).copied().unwrap_or(-1)),
        )
    }
    pub(super) fn id(&self, h: Option<H>) -> f64 {
        match h {
            None => -1.0,
            Some(h) => {
                if self.alive.get(h.0).copied().unwrap_or(false) {
                    h.0 as f64
                } else {
                    -3.0
                }
            }
        }
    }
    pub(super) fn live_h(&self, h: H) -> bool {
        self.alive.get(h.0).copied().unwrap_or(false)
    }
    pub(super) fn kind_of(&self, h: Option<H>) -> i64 {
        h.and_then(|h| self.kinds.get(h.0).copied()).unwrap_or(-1)
    }
    pub(super) fn kill(&mut self, h: H) {
        if let Some(a) = self.alive.get_mut(h.0) {
            *a = false;
        }
    }
}

/// `sm_AreConnected` walks the bare stand-ins safely except an edge's start vertex.
pub(super) fn connect_crashes(pool: &Pool, a: Option<H>, bb: Option<H>) -> bool {
    if a == bb {
        return false;
    }
    let (ka, kb) = (pool.kind_of(a), pool.kind_of(bb));
    (ka == pk::VERTEX && kb == pk::EDGE) || (ka == pk::EDGE && kb == pk::VERTEX)
}

/// A reader over one op's numbers (the shim's `Rd`).
pub(super) struct Rd<'a> {
    pub(super) v: &'a [f64],
    pub(super) at: usize,
    pub(super) bad: bool,
}

impl Rd<'_> {
    pub(super) fn num(&mut self) -> f64 {
        if let Some(&x) = self.v.get(self.at) {
            self.at += 1;
            x
        } else {
            self.bad = true;
            0.0
        }
    }
    pub(super) fn idx(&mut self) -> i64 {
        self.num() as i64
    }
    pub(super) fn flag(&mut self) -> bool {
        self.num() != 0.0
    }
    pub(super) fn vec(&mut self) -> Vec3 {
        let x = self.num();
        let y = self.num();
        let z = self.num();
        Vec3::new(x, y, z)
    }
}

pub(super) struct ProtoRun {
    pub(super) k: Kernel,
    pub(super) pool: Pool,
    pub(super) mgr: ProtoTopologyManager<H>,
    pub(super) mgr_alive: bool,
    pub(super) n_vd: u32,
    pub(super) n_ed: u32,
    pub(super) n_pv: u32,
    pub(super) n_pe: u32,
    pub(super) n_pf: u32,
}

/// The ids of proto objects (`-1` null, `-3` deleted).
impl ProtoRun {
    pub(super) fn vd(&self, j: i64) -> Option<VertexDefId> {
        let i = u32::try_from(j).ok().filter(|&i| i < self.n_vd)?;
        let id = VertexDefId::from_index(i);
        self.mgr.vertex_def(id).map(|_| id)
    }
    pub(super) fn ed(&self, j: i64) -> Option<EdgeDefId> {
        let i = u32::try_from(j).ok().filter(|&i| i < self.n_ed)?;
        let id = EdgeDefId::from_index(i);
        self.mgr.edge_def(id).map(|_| id)
    }
    pub(super) fn pv(&self, j: i64) -> Option<ProtoVertexId> {
        let i = u32::try_from(j).ok().filter(|&i| i < self.n_pv)?;
        let id = ProtoVertexId::from_index(i);
        self.mgr.proto_vertex(id).map(|_| id)
    }
    pub(super) fn pe(&self, j: i64) -> Option<ProtoEdgeId> {
        let i = u32::try_from(j).ok().filter(|&i| i < self.n_pe)?;
        let id = ProtoEdgeId::from_index(i);
        self.mgr.proto_edge(id).map(|_| id)
    }
    pub(super) fn pf(&self, j: i64) -> Option<ProtoFaceId> {
        let i = u32::try_from(j).ok().filter(|&i| i < self.n_pf)?;
        let id = ProtoFaceId::from_index(i);
        self.mgr.proto_face(id).map(|_| id)
    }
    pub(super) fn vd_id(&self, x: Option<VertexDefId>) -> f64 {
        x.map_or(-1.0, |x| {
            if self.mgr.vertex_def(x).is_some() {
                f64::from(x.index())
            } else {
                -3.0
            }
        })
    }
    pub(super) fn ed_id(&self, x: Option<EdgeDefId>) -> f64 {
        x.map_or(-1.0, |x| {
            if self.mgr.edge_def(x).is_some() {
                f64::from(x.index())
            } else {
                -3.0
            }
        })
    }
    pub(super) fn pv_id(&self, x: Option<ProtoVertexId>) -> f64 {
        x.map_or(-1.0, |x| {
            if self.mgr.proto_vertex(x).is_some() {
                f64::from(x.index())
            } else {
                -3.0
            }
        })
    }
    pub(super) fn pe_id(&self, x: Option<ProtoEdgeId>) -> f64 {
        x.map_or(-1.0, |x| {
            if self.mgr.proto_edge(x).is_some() {
                f64::from(x.index())
            } else {
                -3.0
            }
        })
    }
    pub(super) fn pf_id(&self, x: Option<ProtoFaceId>) -> f64 {
        x.map_or(-1.0, |x| {
            if self.mgr.proto_face(x).is_some() {
                f64::from(x.index())
            } else {
                -3.0
            }
        })
    }

    /// A proto vertex id or null (`ok` false for an id that is not live).
    pub(super) fn pv_or_null(&self, j: i64, ok: &mut bool) -> Option<ProtoVertexId> {
        if j == -1 {
            return None;
        }
        let r = self.pv(j);
        if r.is_none() {
            *ok = false;
        }
        r
    }
    pub(super) fn curve_or_null(&self, j: i64, ok: &mut bool) -> Option<H> {
        if j == -1 {
            return None;
        }
        if !self.pool.is_curve(j) {
            *ok = false;
            return None;
        }
        Some(self.pool.h(j))
    }
    pub(super) fn topo(&self, j: i64, ok: &mut bool) -> Option<H> {
        if !self.pool.is_topo(j) {
            *ok = false;
            return None;
        }
        Some(self.pool.h(j))
    }
    pub(super) fn curve_ok(&self, c: Option<H>) -> bool {
        c.is_none_or(|c| self.pool.live_h(c) && self.pool.kinds.get(c.0) == Some(&pk::CURVE))
    }

    pub(super) fn invariant(&self) -> bool {
        let m = &self.mgr;
        if self.mgr_alive
            && (m
                .proto_vertices()
                .iter()
                .any(|&x| m.proto_vertex(x).is_none())
                || m.proto_edges().iter().any(|&x| m.proto_edge(x).is_none())
                || m.proto_faces().iter().any(|&x| m.proto_face(x).is_none()))
        {
            return false;
        }
        for i in 0..self.n_vd {
            let Some(v) = m.vertex_def(VertexDefId::from_index(i)) else {
                continue;
            };
            if v.proto_vertex()
                .is_some_and(|x| m.proto_vertex(x).is_none())
            {
                return false;
            }
            for w in 1..=2 {
                if v.brep_point_classif(w)
                    .object()
                    .is_some_and(|ob| !self.pool.live_h(ob))
                {
                    return false;
                }
            }
        }
        for i in 0..self.n_ed {
            let Some(e) = m.edge_def(EdgeDefId::from_index(i)) else {
                continue;
            };
            if e.proto_edge().is_some_and(|x| m.proto_edge(x).is_none()) {
                return false;
            }
            if !self.curve_ok(e.xsect_curve_3d())
                || !self.curve_ok(e.xsect_uv_curve(1))
                || !self.curve_ok(e.xsect_uv_curve(2))
            {
                return false;
            }
        }
        for i in 0..self.n_pv {
            let Some(v) = m.proto_vertex(ProtoVertexId::from_index(i)) else {
                continue;
            };
            if v.vertex_definitions()
                .iter()
                .any(|&x| m.vertex_def(x).is_none())
                || v.proto_edges().iter().any(|&x| m.proto_edge(x).is_none())
            {
                return false;
            }
        }
        for i in 0..self.n_pe {
            let Some(e) = m.proto_edge(ProtoEdgeId::from_index(i)) else {
                continue;
            };
            let dead_pv = |x: Option<ProtoVertexId>| x.is_some_and(|x| m.proto_vertex(x).is_none());
            if dead_pv(e.start_proto_vertex()) || dead_pv(e.end_proto_vertex()) {
                return false;
            }
            if e.edge_definitions()
                .iter()
                .any(|&x| m.edge_def(x).is_none())
                || e.overlapping_pes()
                    .iter()
                    .any(|&x| m.proto_edge(x).is_none())
                || e.coinc_proto_faces()
                    .iter()
                    .any(|&x| m.proto_face(x).is_none())
            {
                return false;
            }
        }
        for i in 0..self.n_pf {
            let Some(f) = m.proto_face(ProtoFaceId::from_index(i)) else {
                continue;
            };
            if f.coinc_proto_edges()
                .iter()
                .any(|&x| m.proto_edge(x).is_none())
            {
                return false;
            }
        }
        true
    }

    /// Every curve of every live edge definition is live and held by one slot only.
    fn ed_curves_unique(&self) -> bool {
        let mut seen: Vec<H> = Vec::new();
        for i in 0..self.n_ed {
            let Some(d) = self.mgr.edge_def(EdgeDefId::from_index(i)) else {
                continue;
            };
            for c in [d.xsect_curve_3d(), d.xsect_uv_curve(1), d.xsect_uv_curve(2)]
                .into_iter()
                .flatten()
            {
                if !self.pool.live_h(c) || seen.contains(&c) {
                    return false;
                }
                seen.push(c);
            }
        }
        true
    }

    /// What `SmProtoEdge::Split` needs to run on stand-ins.
    fn pe_split_ok(&self, e: ProtoEdgeId) -> bool {
        let Some(x) = self.mgr.proto_edge(e) else {
            return false;
        };
        if x.start_proto_vertex().is_none() || x.end_proto_vertex().is_none() {
            return false;
        }
        x.edge_definitions().iter().all(|&d| {
            self.mgr
                .edge_def(d)
                .is_some_and(|d| d.xsect_curve_3d().is_some() && d.proto_edge().is_some())
        })
    }

    /// The proto edge and both its end vertices are listed in the manager.
    fn listed_with_ends(&self, e: ProtoEdgeId) -> bool {
        let m = &self.mgr;
        m.proto_edges().contains(&e)
            && m.proto_edge(e).is_some_and(|x| {
                [x.start_proto_vertex(), x.end_proto_vertex()]
                    .iter()
                    .all(|v| v.is_some_and(|v| m.proto_vertices().contains(&v)))
            })
    }

    /// Creating a proto vertex reads an uninitialised position when `sm_FindPositionFromPCs`
    /// fails for two classifications that can be imprinted: such inputs are refused (the
    /// shim's `position_ok`).
    fn position_ok(
        &self,
        a: &PointClassification<H>,
        c: &PointClassification<H>,
    ) -> Result<bool, Fail> {
        if !(proto_mod::can_imprint(a.point_class()) && proto_mod::can_imprint(c.point_class())) {
            return Ok(true);
        }
        let (mut x, mut y) = (*a, *c);
        Ok(st(proto_mod::find_position_from_pcs(
            &self.k, &self.pool, &mut x, &mut y,
        ))?
        .is_ok())
    }

    /// Kills the curves an op deleted; their ids, sorted.
    fn kill_curves(&mut self, deleted: Vec<H>) -> Vec<f64> {
        let mut ids: Vec<f64> = deleted.iter().map(|&c| self.pool.id(Some(c))).collect();
        ids.sort_by(f64::total_cmp);
        for c in deleted {
            self.pool.kill(c);
        }
        ids
    }

    /// `class obj tol flags t u v gap`; `None` when the object is not of the class's kind.
    pub(super) fn read_pc(&self, rd: &mut Rd<'_>) -> Option<PointClassification<H>> {
        let cls = rd.idx();
        let ob = rd.idx();
        let tol = rd.num();
        let flags = rd.idx();
        let t = rd.num();
        let u = rd.num();
        let v = rd.num();
        let g = rd.num();
        let pool = &self.pool;
        let ok = match cls {
            100 | 110 => ob == -1,
            101 => pool.is(ob, pk::REGION),
            102 => pool.is(ob, pk::FACE),
            103 => pool.is(ob, pk::EDGE),
            104 => pool.is(ob, pk::VERTEX),
            106 => pool.is(ob, pk::CURVE),
            107 => pool.is(ob, pk::PLANE) || pool.is(ob, pk::BSURF),
            _ => false,
        };
        if !ok || rd.bad {
            return None;
        }
        let class = PointClassType::from_code(cls as i32)?;
        let mut pc = PointClassification::new(zone_tol3d(&self.k, tol));
        pc.set_class_object(class, (ob != -1).then(|| pool.h(ob)));
        if flags & 1 != 0 {
            pc.set_t_param(t, false);
        }
        if flags & 2 != 0 {
            pc.set_uv_param(Point2::new(u, v));
        }
        if flags & 4 != 0 {
            pc.set_gap_3d(g);
        }
        Some(pc)
    }

    pub(super) fn put_pc(&self, o: &mut Vec<f64>, pc: &PointClassification<H>) {
        o.push(f64::from(pc.point_class().code()));
        o.push(self.pool.id(pc.object()));
        o.push(pc.t_param());
        let uv = pc.uv_param();
        o.extend([uv.x, uv.y]);
        o.push(b(pc.are_parameters_set()));
        o.push(pc.deviation());
        o.push(pc.src_zone_tol_3d());
    }

    /// The curves an edge definition's destructor deletes.
    pub(super) fn ed_deletes(e: &EdgeDefinition<H>, curves: &mut Vec<H>) {
        if e.src_topo_type(1) == PointClassType::Edge {
            curves.extend(e.xsect_curve_3d());
            curves.extend(e.xsect_uv_curve(1));
        }
        if e.src_topo_type(2) == PointClassType::Edge {
            curves.extend(e.xsect_uv_curve(2));
        }
    }

    pub(super) fn put_state(&self, o: &mut Vec<f64>) {
        let m = &self.mgr;
        let k = &self.k;
        o.push(b(self.mgr_alive));
        if self.mgr_alive {
            o.push(m.tolerance(k));
            o.push(m.capture_distance());
            o.push(m.proto_vertices().len() as f64);
            o.extend(m.proto_vertices().iter().map(|&x| self.pv_id(Some(x))));
            o.push(m.proto_edges().len() as f64);
            o.extend(m.proto_edges().iter().map(|&x| self.pe_id(Some(x))));
            o.push(m.proto_faces().len() as f64);
            o.extend(m.proto_faces().iter().map(|&x| self.pf_id(Some(x))));
        }
        for i in 0..self.n_vd {
            let Some(v) = m.vertex_def(VertexDefId::from_index(i)) else {
                o.push(0.0);
                continue;
            };
            o.push(1.0);
            self.put_pc(o, v.brep_point_classif(1));
            self.put_pc(o, v.brep_point_classif(2));
            put_vec(o, v.position());
            o.push(v.tolerance(k));
            o.push(v.gap());
            o.push(self.pv_id(v.proto_vertex()));
            o.push(self.pool.id(v.final_vertex(1)));
            o.push(self.pool.id(v.final_vertex(2)));
            o.push(self.pool.id(v.src_topo(1)));
            o.push(self.pool.id(v.src_topo(2)));
            o.push(f64::from(v.src_topo_type(1).code()));
            o.push(f64::from(v.src_topo_type(2).code()));
        }
        for i in 0..self.n_ed {
            let Some(e) = m.edge_def(EdgeDefId::from_index(i)) else {
                o.push(0.0);
                continue;
            };
            o.push(1.0);
            o.push(self.pool.id(e.xsect_curve_3d()));
            o.push(self.pool.id(e.xsect_uv_curve(1)));
            o.push(self.pool.id(e.xsect_uv_curve(2)));
            o.extend([e.xsect_interval().min(), e.xsect_interval().max()]);
            o.push(self.pool.id(e.src_topo(1)));
            o.push(self.pool.id(e.src_topo(2)));
            o.push(f64::from(e.src_topo_type(1).code()));
            o.push(f64::from(e.src_topo_type(2).code()));
            o.push(self.pe_id(e.proto_edge()));
            o.push(e.tolerance(k));
            o.push(self.pool.id(e.final_edge(1)));
            o.push(self.pool.id(e.final_edge(2)));
        }
        for i in 0..self.n_pv {
            let Some(v) = m.proto_vertex(ProtoVertexId::from_index(i)) else {
                o.push(0.0);
                continue;
            };
            o.push(1.0);
            put_vec(o, v.position());
            o.push(v.tolerance(k));
            o.push(v.vertex_definitions().len() as f64);
            o.extend(v.vertex_definitions().iter().map(|&x| self.vd_id(Some(x))));
            o.push(v.proto_edges().len() as f64);
            o.extend(v.proto_edges().iter().map(|&x| self.pe_id(Some(x))));
        }
        for i in 0..self.n_pe {
            let Some(e) = m.proto_edge(ProtoEdgeId::from_index(i)) else {
                o.push(0.0);
                continue;
            };
            o.push(1.0);
            o.extend([e.xsect_interval().min(), e.xsect_interval().max()]);
            o.push(self.pv_id(e.start_proto_vertex()));
            o.push(self.pv_id(e.end_proto_vertex()));
            o.push(e.tolerance(k));
            o.push(e.edge_definitions().len() as f64);
            o.extend(e.edge_definitions().iter().map(|&x| self.ed_id(Some(x))));
            o.push(e.overlapping_pes().len() as f64);
            o.extend(e.overlapping_pes().iter().map(|&x| self.pe_id(Some(x))));
            o.push(e.coinc_proto_faces().len() as f64);
            o.extend(e.coinc_proto_faces().iter().map(|&x| self.pf_id(Some(x))));
        }
        for i in 0..self.n_pf {
            let Some(f) = m.proto_face(ProtoFaceId::from_index(i)) else {
                o.push(0.0);
                continue;
            };
            o.push(1.0);
            o.push(self.pool.id(f.src_topo(1)));
            o.push(self.pool.id(f.src_topo(2)));
            o.push(f64::from(f.src_topo_type(1).code()));
            o.push(f.coinc_proto_edges().len() as f64);
            o.extend(f.coinc_proto_edges().iter().map(|&x| self.pe_id(Some(x))));
        }
        for (i, obj) in self.pool.objs.iter().enumerate() {
            if self.pool.kinds.get(i) != Some(&pk::CURVE) {
                continue;
            }
            let alive = self.pool.alive.get(i).copied().unwrap_or(false);
            o.push(b(alive));
            if alive {
                if let PObj::Curve(c) = obj {
                    put_curve(k, o, c);
                }
            }
        }
    }
}

pub(super) fn unique_live<T: PartialEq + Copy>(v: &[T], live: impl Fn(T) -> bool) -> bool {
    for (i, &x) in v.iter().enumerate() {
        if !live(x) || v.iter().take(i).any(|&y| y == x) {
            return false;
        }
    }
    true
}

fn put_eval(out: &mut Vec<f64>, r: Result<(Option<Vec3>, Option<Vec3>), f64>, mode: i64) {
    match r {
        Ok((pp, tt)) => {
            out.push(0.0);
            if mode & 1 != 0 {
                put_vec(out, pp.unwrap_or(Vec3::UNDEF));
            }
            if mode & 2 != 0 {
                put_vec(out, tt.unwrap_or(Vec3::UNDEF));
            }
        }
        Err(s) => out.push(s),
    }
}

/// Between: whether `(v1, v2)` are the ends of a proto edge (either way round).
pub(super) fn between(
    v1: Option<ProtoVertexId>,
    v2: Option<ProtoVertexId>,
    s: Option<ProtoVertexId>,
    e: Option<ProtoVertexId>,
) -> bool {
    (v1 == s && v2 == e) || (v1 == e && v2 == s)
}

#[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
fn run_proto(
    objects: &[ObjIn],
    approx_tol: f64,
    capture: f64,
    override_tol: f64,
    ops: &[Vec<f64>],
) -> Result<Vec<f64>, Fail> {
    let k = Kernel::default();
    let mut pool = Pool {
        objs: Vec::new(),
        kinds: Vec::new(),
        alive: Vec::new(),
        topo: TopoRecords::default(),
    };
    for ob in objects {
        let (obj, kd) = match ob {
            ObjIn::Curve(c) => (PObj::Curve(build_curve(c)?), pk::CURVE),
            ObjIn::Plane { o, n } => (
                PObj::Surface(Surface::from(Plane::from_normal(v3(*o), v3(*n)))),
                pk::PLANE,
            ),
            ObjIn::Bsurf(s) => (PObj::Surface(build_surface(s)?), pk::BSURF),
            ObjIn::Vertex => (PObj::Tol(0.0), pk::VERTEX),
            ObjIn::VertexTol(tol) => (PObj::Tol(zone_tol3d(&k, *tol)), pk::VERTEX),
            ObjIn::VertexAt { p, tol } => {
                (PObj::VertexPt(v3(*p), zone_tol3d(&k, *tol)), pk::VERTEX)
            }
            ObjIn::Edge(c) => {
                if !pool.is_curve(*c as i64) {
                    return Err(harness("edge curve").into());
                }
                (
                    PObj::Edge {
                        curve: *c,
                        ivl: Extent1d::new(),
                        tol: 0.0,
                    },
                    pk::EDGE,
                )
            }
            ObjIn::EdgeIvl { c, a, b: bb, tol } => {
                if !pool.is_curve(*c as i64) {
                    return Err(harness("edge curve").into());
                }
                (
                    PObj::Edge {
                        curve: *c,
                        ivl: Extent1d::from_min_max(*a, *bb),
                        tol: zone_tol3d(&k, *tol),
                    },
                    pk::EDGE,
                )
            }
            ObjIn::Face(s) => {
                if !pool.is(*s as i64, pk::PLANE) && !pool.is(*s as i64, pk::BSURF) {
                    return Err(harness("face surface").into());
                }
                (PObj::Tol(0.0), pk::FACE)
            }
            ObjIn::Region => (PObj::Bare, pk::REGION),
            ObjIn::Loop => (PObj::Bare, pk::LOOP),
        };
        // the curve of an edge, the surface of a face (earlier pool objects)
        let curve = match ob {
            ObjIn::Edge(c) | ObjIn::EdgeIvl { c, .. } => match pool.objs.get(*c) {
                Some(PObj::Curve(x)) => Some(x),
                _ => None,
            },
            _ => None,
        };
        let surface = match ob {
            ObjIn::Face(sf) => match pool.objs.get(*sf) {
                Some(PObj::Surface(x)) => Some(x),
                _ => None,
            },
            _ => None,
        };
        pool.topo.push(&obj, kd, curve, surface)?;
        pool.objs.push(obj);
        pool.kinds.push(kd);
        pool.alive.push(true);
    }
    let brep1 = H(100_000, ClassObjectKind::Other);
    let brep2 = H(100_001, ClassObjectKind::Other);
    let ti = H(100_002, ClassObjectKind::Other);
    let ti_tol = approx_tol;
    // two empty B-reps, as the shim's
    let (b1, b2) = (Brep::new(), Brep::new());
    let mgr = match st(ProtoTopologyManager::new(
        &k,
        (brep1, &b1),
        (brep2, &b2),
        ti,
        ti_tol,
        capture,
        override_tol,
    ))? {
        Ok(m) => m,
        Err(s) => return Err(harness(format!("manager: status {s}")).into()),
    };
    let mut r = ProtoRun {
        k,
        pool,
        mgr,
        mgr_alive: true,
        n_vd: 0,
        n_ed: 0,
        n_pv: 0,
        n_pe: 0,
        n_pf: 0,
    };
    let mut o = Vec::new();

    for opv in ops {
        r.pool.sync_topo();
        let mut rd = Rd {
            v: opv,
            at: 0,
            bad: false,
        };
        let op = rd.idx();
        o.push(op as f64);
        let mut out: Vec<f64> = Vec::new();
        let mut status = 0.0;
        let mut skip = !r.mgr_alive;
        let mut ok = true;
        let k = &r.k;
        match op {
            p::PV_NEW => {
                let pt = rd.vec();
                let tol = rd.num();
                if !skip {
                    match st(r.mgr.new_proto_vertex(pt, tol))? {
                        Ok(id) => {
                            r.n_pv += 1;
                            out.push(f64::from(id.index()));
                        }
                        Err(e) => status = e,
                    }
                }
            }
            p::PV_NEW_PCS => {
                let pc1 = r.read_pc(&mut rd);
                let pc2 = r.read_pc(&mut rd);
                let pt = rd.vec();
                let tol = rd.num();
                match (pc1, pc2) {
                    (Some(pc1), Some(pc2)) if !skip => {
                        let res = r
                            .mgr
                            .new_proto_vertex_with_pcs(k, &r.pool, &pc1, &pc2, pt, tol);
                        match st(res)? {
                            Ok(id) => {
                                r.n_pv += 1;
                                r.n_vd += 1;
                                out.push(f64::from(id.index()));
                                let vd = r
                                    .mgr
                                    .proto_vertex(id)
                                    .and_then(|v| v.vertex_definitions().first().copied());
                                out.push(r.vd_id(vd));
                            }
                            Err(e) => status = e,
                        }
                    }
                    _ => skip = true,
                }
            }
            p::VD_NEW => {
                let pc1 = r.read_pc(&mut rd);
                let pc2 = r.read_pc(&mut rd);
                let pt = rd.vec();
                let tol = rd.num();
                let gap = rd.num();
                let owner = r.pv_or_null(rd.idx(), &mut ok);
                match (pc1, pc2) {
                    (Some(pc1), Some(pc2)) if !skip && ok => {
                        let vd = VertexDefinition::new(k, &r.pool, &pc1, &pc2, pt, tol, gap, owner);
                        match st(vd)? {
                            Ok(vd) => match st(r.mgr.insert_vertex_definition(vd))? {
                                Ok(id) => {
                                    r.n_vd += 1;
                                    out.push(f64::from(id.index()));
                                }
                                Err(e) => status = e,
                            },
                            Err(e) => status = e,
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_NEW => {
                let s = r.pv_or_null(rd.idx(), &mut ok);
                let e = r.pv_or_null(rd.idx(), &mut ok);
                let a = rd.num();
                let bb = rd.num();
                if skip || !ok {
                    skip = true;
                } else {
                    match st(r.mgr.new_proto_edge(k, s, e, Extent1d::from_min_max(a, bb)))? {
                        Ok(id) => {
                            r.n_pe += 1;
                            out.push(f64::from(id.index()));
                        }
                        Err(e) => status = e,
                    }
                }
            }
            p::PE_COPY => {
                let src = r.pe(rd.idx());
                match src {
                    Some(src) if !skip => match st(r.mgr.copy_proto_edge(src))? {
                        Ok(id) => {
                            r.n_pe += 1;
                            out.push(f64::from(id.index()));
                        }
                        Err(e) => status = e,
                    },
                    _ => skip = true,
                }
            }
            p::ED_NEW => {
                let c3 = r.curve_or_null(rd.idx(), &mut ok);
                let a = rd.num();
                let bb = rd.num();
                let s1 = r.topo(rd.idx(), &mut ok);
                let s2 = r.topo(rd.idx(), &mut ok);
                let tol = rd.num();
                let u1 = r.curve_or_null(rd.idx(), &mut ok);
                let u2 = r.curve_or_null(rd.idx(), &mut ok);
                let pej = rd.idx();
                if pej != -1 && r.pe(pej).is_none() {
                    ok = false;
                }
                match (s1, s2) {
                    (Some(s1), Some(s2)) if !skip && ok => {
                        let ed = EdgeDefinition::new(
                            c3,
                            Extent1d::from_min_max(a, bb),
                            s1,
                            s2,
                            tol,
                            u1,
                            u2,
                            r.pe(pej),
                        );
                        match st(r.mgr.insert_edge_definition(ed))? {
                            Ok(id) => {
                                r.n_ed += 1;
                                out.push(f64::from(id.index()));
                            }
                            Err(e) => status = e,
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_COPY => {
                let src = r.ed(rd.idx());
                match src {
                    Some(src) if !skip => match st(r.mgr.copy_edge_definition(src))? {
                        Ok(id) => {
                            r.n_ed += 1;
                            out.push(f64::from(id.index()));
                        }
                        Err(e) => status = e,
                    },
                    _ => skip = true,
                }
            }
            p::PF_NEW | p::PF_CREATE => {
                let f1 = rd.idx();
                let f2 = rd.idx();
                let n = rd.idx();
                let mut arr = Vec::new();
                let mut kk = 0;
                while kk < n && kk < 16 && !rd.bad {
                    let j = rd.idx();
                    match r.pe(j) {
                        Some(x) => arr.push(x),
                        None => ok = false,
                    }
                    kk += 1;
                }
                if (f1 != -1 && !r.pool.is(f1, pk::FACE))
                    || (f2 != -1 && !r.pool.is(f2, pk::FACE))
                    || !(0..=16).contains(&n)
                {
                    ok = false;
                }
                if skip || !ok {
                    skip = true;
                } else {
                    let a = (f1 != -1).then(|| r.pool.h(f1));
                    let bb = (f2 != -1).then(|| r.pool.h(f2));
                    let res = if op == p::PF_NEW {
                        r.mgr.new_proto_face(a, bb, &arr)
                    } else {
                        r.mgr.create_proto_face(a, bb, &arr)
                    };
                    match st(res)? {
                        Ok(id) => {
                            r.n_pf += 1;
                            out.push(f64::from(id.index()));
                        }
                        Err(e) => status = e,
                    }
                }
            }
            p::MGR_ADD_PV => match r.pv(rd.idx()) {
                Some(v) if !skip => r.mgr.add_proto_vertex(v),
                _ => skip = true,
            },
            p::MGR_ADD_PE => match r.pe(rd.idx()) {
                Some(e) if !skip => r.mgr.add_proto_edge(e),
                _ => skip = true,
            },
            p::MGR_ADD_PF => match r.pf(rd.idx()) {
                Some(f) if !skip => r.mgr.add_proto_face(f),
                _ => skip = true,
            },
            p::MGR_RESET => {
                if !skip {
                    r.mgr.reset();
                }
            }
            p::PV_ADD_PE => {
                let v = r.pv(rd.idx());
                let e = r.pe(rd.idx());
                match (v, e) {
                    (Some(v), Some(e)) if !skip => {
                        if let Some(x) = r.mgr.proto_vertex_mut(v) {
                            x.add_proto_edge(e);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PV_ADD_VD => {
                let v = r.pv(rd.idx());
                let d = r.vd(rd.idx());
                match (v, d) {
                    (Some(v), Some(d)) if !skip => status = st0(r.mgr.add_vertex_definition(v, d))?,
                    _ => skip = true,
                }
            }
            p::PE_ADD_ED => {
                let e = r.pe(rd.idx());
                let d = r.ed(rd.idx());
                match (e, d) {
                    (Some(e), Some(d)) if !skip => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            x.edge_definitions_mut().push(d);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_ADD_OVERLAP => {
                let e = r.pe(rd.idx());
                let e2 = r.pe(rd.idx());
                match (e, e2) {
                    (Some(e), Some(e2)) if !skip => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            x.add_overlapping_pe(e2);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_APPEND_OVERLAPS => {
                let e = r.pe(rd.idx());
                let n = rd.idx();
                let mut arr = Vec::new();
                let mut kk = 0;
                while kk < n && kk < 16 && !rd.bad {
                    match r.pe(rd.idx()) {
                        Some(x) => arr.push(x),
                        None => ok = false,
                    }
                    kk += 1;
                }
                match e {
                    Some(e) if !skip && ok && (0..=16).contains(&n) => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            x.append_overlapping_pes(&arr);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_CLEAR_EDS => match r.pe(rd.idx()) {
                Some(e) if !skip => {
                    if let Some(x) = r.mgr.proto_edge_mut(e) {
                        x.clear_edge_definitions();
                    }
                }
                _ => skip = true,
            },
            p::PV_CLEAR_VDS => match r.pv(rd.idx()) {
                Some(v) if !skip => {
                    if let Some(x) = r.mgr.proto_vertex_mut(v) {
                        x.clear_vertex_definitions();
                    }
                }
                _ => skip = true,
            },
            p::PE_SET_START | p::PE_SET_END => {
                let e = r.pe(rd.idx());
                let v = r.pv_or_null(rd.idx(), &mut ok);
                match e {
                    Some(e) if !skip && ok => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            if op == p::PE_SET_START {
                                x.set_start_proto_vertex(v);
                            } else {
                                x.set_end_proto_vertex(v);
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_SET_IVL => {
                let e = r.pe(rd.idx());
                let a = rd.num();
                let bb = rd.num();
                match e {
                    Some(e) if !skip => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            x.set_xsect_interval(Extent1d::from_min_max(a, bb));
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_REPLACE_PV => {
                let e = r.pe(rd.idx());
                let a = r.pv_or_null(rd.idx(), &mut ok);
                let bb = r.pv_or_null(rd.idx(), &mut ok);
                match e {
                    Some(e) if !skip && ok => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            x.replace_proto_vertex(a, bb);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_REMOVE_ED => {
                let e = r.pe(rd.idx());
                let d = r.ed(rd.idx());
                match (e, d) {
                    (Some(e), Some(d)) if !skip => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            status = st0(x.remove_edge_definition(d))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_REMOVE_PV => {
                let e = r.pe(rd.idx());
                let v = r.pv_or_null(rd.idx(), &mut ok);
                match e {
                    Some(e) if !skip && ok => {
                        if let Some(x) = r.mgr.proto_edge_mut(e) {
                            status = st0(x.remove_proto_vertex(v))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PV_REMOVE_VD => {
                let v = r.pv(rd.idx());
                let d = r.vd(rd.idx());
                match (v, d) {
                    (Some(v), Some(d)) if !skip => {
                        if let Some(x) = r.mgr.proto_vertex_mut(v) {
                            status = st0(x.remove_vertex_definition(d))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PV_REMOVE_PE => {
                let v = r.pv(rd.idx());
                let e = r.pe(rd.idx());
                match (v, e) {
                    (Some(v), Some(e)) if !skip => {
                        if let Some(x) = r.mgr.proto_vertex_mut(v) {
                            status = st0(x.remove_proto_edge(e))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_RELATE_PF => {
                let e = r.pe(rd.idx());
                let f = r.pf(rd.idx());
                match (e, f) {
                    (Some(e), Some(f)) if !skip => status = st0(r.mgr.relate_coincident_pf(e, f))?,
                    _ => skip = true,
                }
            }
            p::PF_SET_SRC => {
                let f = r.pf(rd.idx());
                let w = rd.idx();
                let j = rd.idx();
                if j != -1 && !r.pool.is_topo(j) {
                    ok = false;
                }
                match f {
                    Some(f) if !skip && ok => {
                        let h = (j != -1).then(|| r.pool.h(j));
                        if let Some(x) = r.mgr.proto_face_mut(f) {
                            x.set_src_topo(w as i32, PointClassType::Face, h);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::VD_SET_SRC => {
                let d = r.vd(rd.idx());
                let w = rd.idx();
                let cls = rd.idx();
                let j = rd.idx();
                ok = match cls {
                    100 | 110 => j == -1,
                    101 => r.pool.is(j, pk::REGION),
                    102 => r.pool.is(j, pk::FACE),
                    103 => r.pool.is(j, pk::EDGE),
                    104 => r.pool.is(j, pk::VERTEX),
                    _ => false,
                };
                let class = PointClassType::from_code(cls as i32);
                match (d, class) {
                    (Some(d), Some(class)) if !skip && ok => {
                        let h = (j != -1).then(|| r.pool.h(j));
                        if let Some(x) = r.mgr.vertex_def_mut(d) {
                            status = st0(x.set_src_topo(w as i32, class, h))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::VD_SET_FINAL => {
                let d = r.vd(rd.idx());
                let w = rd.idx();
                let j = rd.idx();
                if j != -1 && !r.pool.is(j, pk::VERTEX) {
                    ok = false;
                }
                match d {
                    Some(d) if !skip && ok => {
                        let h = (j != -1).then(|| r.pool.h(j));
                        if let Some(x) = r.mgr.vertex_def_mut(d) {
                            x.set_final_vertex(w as i32, h);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::VD_SET_POS => {
                let d = r.vd(rd.idx());
                let pt = rd.vec();
                match d {
                    Some(d) if !skip => {
                        if let Some(x) = r.mgr.vertex_def_mut(d) {
                            x.set_position(pt);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::VD_SET_PV => {
                let d = r.vd(rd.idx());
                let v = r.pv_or_null(rd.idx(), &mut ok);
                match d {
                    Some(d) if !skip && ok => {
                        if let Some(x) = r.mgr.vertex_def_mut(d) {
                            x.set_proto_vertex(v);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_C3 => {
                let d = r.ed(rd.idx());
                let c = r.curve_or_null(rd.idx(), &mut ok);
                match d {
                    Some(d) if !skip && ok => {
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            x.set_xsect_curve_3d(c);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_UV => {
                let d = r.ed(rd.idx());
                let w = rd.idx();
                let c = r.curve_or_null(rd.idx(), &mut ok);
                match d {
                    Some(d) if !skip && ok => {
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            x.set_xsect_curve_uv(w as i32, c);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_PE => {
                let d = r.ed(rd.idx());
                let j = rd.idx();
                if j != -1 && r.pe(j).is_none() {
                    ok = false;
                }
                match d {
                    Some(d) if !skip && ok => {
                        let pe = r.pe(j);
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            x.set_proto_edge(pe);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_IVL => {
                let d = r.ed(rd.idx());
                let a = rd.num();
                let bb = rd.num();
                match d {
                    Some(d) if !skip => {
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            x.set_xsect_interval(Extent1d::from_min_max(a, bb));
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_SRC => {
                let d = r.ed(rd.idx());
                let w = rd.idx();
                let t = r.topo(rd.idx(), &mut ok);
                match (d, t) {
                    (Some(d), Some(t)) if !skip && ok => {
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            x.set_src_topo(w as i32, PointClassType::Unknown, t);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_FINAL => {
                let d = r.ed(rd.idx());
                let w = rd.idx();
                let j = rd.idx();
                if j != -1 && !r.pool.is(j, pk::EDGE) {
                    ok = false;
                }
                match d {
                    Some(d) if !skip && ok => {
                        let h = (j != -1).then(|| r.pool.h(j));
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            x.set_final_edge(if w == 1 { 1 } else { 2 }, h);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::MGR_REMOVE_VD => {
                let d = r.vd(rd.idx());
                let flag = rd.flag();
                let owner = d
                    .and_then(|d| r.mgr.vertex_def(d))
                    .and_then(VertexDefinition::proto_vertex);
                match (d, owner) {
                    (Some(d), Some(owner)) if !skip => {
                        let mut left: Vec<VertexDefId> = r
                            .mgr
                            .proto_vertex(owner)
                            .map(|v| v.vertex_definitions().to_vec())
                            .unwrap_or_default();
                        if let Some(i) = left.iter().position(|&x| x == d) {
                            left.remove(i);
                        }
                        let dies = flag && left.is_empty();
                        if dies {
                            let pes = r
                                .mgr
                                .proto_vertex(owner)
                                .map(|v| v.proto_edges().to_vec())
                                .unwrap_or_default();
                            if pes.iter().any(|&x| r.mgr.proto_edge(x).is_none()) {
                                skip = true;
                            }
                        }
                        if !skip {
                            status = st0(r.mgr.remove_vertex_definition(d, flag))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::MGR_REMOVE_PV => match r.pv(rd.idx()) {
                Some(v) if !skip => status = st0(r.mgr.remove_proto_vertex(v))?,
                _ => skip = true,
            },
            p::MGR_REMOVE_PE => match r.pe(rd.idx()) {
                Some(e) if !skip => status = st0(r.mgr.remove_proto_edge(e))?,
                _ => skip = true,
            },
            p::MGR_SET_TOL => {
                let t = rd.num();
                if !skip {
                    let z = zone_tol3d(k, t);
                    r.mgr.set_tolerance(z);
                }
            }
            p::MGR_SET_CAPTURE => {
                let t = rd.num();
                if !skip {
                    r.mgr.set_capture_distance(t);
                }
            }
            p::PV_COMBINE => {
                let v = r.pv(rd.idx());
                let w = r.pv(rd.idx());
                match (v, w) {
                    (Some(v), Some(w)) if !skip => status = st0(r.mgr.combine_proto_vertex(v, w))?,
                    _ => skip = true,
                }
            }
            p::ED_DESTRUCT => match r.ed(rd.idx()) {
                Some(d) if !skip => {
                    let e = r.mgr.edge_def(d).cloned();
                    let pre: Vec<H> = e
                        .iter()
                        .flat_map(|e| {
                            [e.xsect_curve_3d(), e.xsect_uv_curve(1), e.xsect_uv_curve(2)]
                        })
                        .flatten()
                        .collect();
                    if unique_live(&pre, |c| r.pool.live_h(c)) {
                        let deleted = r
                            .mgr
                            .edge_def_mut(d)
                            .map(EdgeDefinition::destruct)
                            .unwrap_or_default();
                        out.push(deleted.len() as f64);
                        out.extend(deleted.iter().map(|&c| r.pool.id(Some(c))));
                        for c in deleted {
                            r.pool.kill(c);
                        }
                    } else {
                        skip = true;
                    }
                }
                _ => skip = true,
            },
            p::DELETE_ED => match r.ed(rd.idx()) {
                Some(d) if !skip => {
                    let mut pre = Vec::new();
                    if let Some(e) = r.mgr.edge_def(d) {
                        ProtoRun::ed_deletes(e, &mut pre);
                    }
                    if unique_live(&pre, |c| r.pool.live_h(c)) {
                        let deleted = r.mgr.delete_edge_definition(d);
                        out.push(deleted.len() as f64);
                        out.extend(deleted.iter().map(|&c| r.pool.id(Some(c))));
                        for c in deleted {
                            r.pool.kill(c);
                        }
                    } else {
                        skip = true;
                    }
                }
                _ => skip = true,
            },
            p::DELETE_VD => match r.vd(rd.idx()) {
                Some(d) if !skip => r.mgr.delete_vertex_definition(d),
                _ => skip = true,
            },
            p::DELETE_PE => match r.pe(rd.idx()) {
                Some(e) if !skip => {
                    let eds = r
                        .mgr
                        .proto_edge(e)
                        .map(|x| x.edge_definitions().to_vec())
                        .unwrap_or_default();
                    let mut curves = Vec::new();
                    for &d in &eds {
                        if let Some(x) = r.mgr.edge_def(d) {
                            ProtoRun::ed_deletes(x, &mut curves);
                        }
                    }
                    if !unique_live(&eds, |x| r.mgr.edge_def(x).is_some())
                        || !unique_live(&curves, |c| r.pool.live_h(c))
                    {
                        skip = true;
                    } else {
                        let deleted = r.mgr.delete_proto_edge_object(e);
                        out.push(deleted.len() as f64);
                        out.extend(deleted.iter().map(|&c| r.pool.id(Some(c))));
                        for c in deleted {
                            r.pool.kill(c);
                        }
                    }
                }
                _ => skip = true,
            },
            p::DELETE_PV => match r.pv(rd.idx()) {
                Some(v) if !skip => {
                    let vds = r
                        .mgr
                        .proto_vertex(v)
                        .map(|x| x.vertex_definitions().to_vec())
                        .unwrap_or_default();
                    if unique_live(&vds, |x| r.mgr.vertex_def(x).is_some()) {
                        r.mgr.delete_proto_vertex_object(v);
                    } else {
                        skip = true;
                    }
                }
                _ => skip = true,
            },
            p::MGR_DESTROY => {
                if !skip {
                    let m = &r.mgr;
                    let pfs = m.proto_faces().to_vec();
                    let pes = m.proto_edges().to_vec();
                    let pvs = m.proto_vertices().to_vec();
                    let mut eds = Vec::new();
                    let mut curves = Vec::new();
                    for &e in &pes {
                        if let Some(x) = m.proto_edge(e) {
                            for &d in x.edge_definitions() {
                                eds.push(d);
                                if let Some(y) = m.edge_def(d) {
                                    ProtoRun::ed_deletes(y, &mut curves);
                                }
                            }
                        }
                    }
                    let mut vds = Vec::new();
                    for &v in &pvs {
                        if let Some(x) = m.proto_vertex(v) {
                            vds.extend_from_slice(x.vertex_definitions());
                        }
                    }
                    if !unique_live(&pfs, |x| m.proto_face(x).is_some())
                        || !unique_live(&pes, |x| m.proto_edge(x).is_some())
                        || !unique_live(&pvs, |x| m.proto_vertex(x).is_some())
                        || !unique_live(&eds, |x| m.edge_def(x).is_some())
                        || !unique_live(&curves, |c| r.pool.live_h(c))
                        || !unique_live(&vds, |x| m.vertex_def(x).is_some())
                    {
                        skip = true;
                    } else {
                        // the proto vertices' edge lists after the proto edges unlist
                        // themselves: a deleted proto edge left there is used after free
                        let mut lists: BTreeMap<u32, Vec<ProtoEdgeId>> = BTreeMap::new();
                        for i in 0..r.n_pv {
                            if let Some(x) = m.proto_vertex(ProtoVertexId::from_index(i)) {
                                lists.insert(i, x.proto_edges().to_vec());
                            }
                        }
                        let mut unlist = |v: ProtoVertexId, e: ProtoEdgeId| {
                            if let Some(l) = lists.get_mut(&v.index()) {
                                if let Some(pos) = l.iter().position(|&y| y == e) {
                                    l.remove(pos);
                                }
                            }
                        };
                        for &e in &pes {
                            if let Some(x) = m.proto_edge(e) {
                                if let Some(sv) = x.start_proto_vertex() {
                                    unlist(sv, e);
                                }
                                if let Some(ev) = x.end_proto_vertex() {
                                    if x.end_proto_vertex() != x.start_proto_vertex() {
                                        unlist(ev, e);
                                    }
                                }
                            }
                        }
                        for &v in &pvs {
                            if lists
                                .get(&v.index())
                                .is_some_and(|l| l.iter().any(|e| pes.contains(e)))
                            {
                                skip = true;
                            }
                        }
                        if !skip {
                            let deleted = r.mgr.destroy();
                            r.mgr_alive = false;
                            out.push(deleted.len() as f64);
                            out.extend(deleted.iter().map(|&c| r.pool.id(Some(c))));
                            for c in deleted {
                                r.pool.kill(c);
                            }
                        }
                    }
                }
            }
            p::VD_RESOLVE => {
                let d = r.vd(rd.idx());
                let has_pv = d
                    .and_then(|d| r.mgr.vertex_def(d))
                    .and_then(VertexDefinition::proto_vertex);
                match (d, has_pv) {
                    (Some(d), Some(_)) if !skip => {
                        let res = r.mgr.resolve_vertex_definition(k, &r.pool, d);
                        let sres = match st(res)? {
                            Ok(s) => s,
                            Err(e) => {
                                status = e;
                                0
                            }
                        };
                        out.push(f64::from(sres));
                        let pos = r
                            .mgr
                            .vertex_def(d)
                            .map_or(Vec3::UNDEF, VertexDefinition::position);
                        put_vec(&mut out, pos);
                    }
                    _ => skip = true,
                }
            }
            p::VD_SET_POINT_PARAMS => match r.vd(rd.idx()) {
                Some(d) if !skip => {
                    if let Some(x) = r.mgr.vertex_def_mut(d) {
                        status = st0(x.set_point_parameters(k, &r.pool))?;
                    }
                }
                _ => skip = true,
            },
            p::VD_NUM_PES => {
                let d = r.vd(rd.idx());
                let has_pv = d
                    .and_then(|d| r.mgr.vertex_def(d))
                    .and_then(VertexDefinition::proto_vertex);
                match (d, has_pv) {
                    (Some(d), Some(_)) if !skip => match st(r.mgr.vertex_def_num_proto_edges(d))? {
                        Ok(n) => out.push(n as f64),
                        Err(e) => status = e,
                    },
                    _ => skip = true,
                }
            }
            p::ED_EVAL | p::ED_EVAL_MID => {
                let d = r.ed(rd.idx());
                let t = if op == p::ED_EVAL { rd.num() } else { 0.0 };
                let mode = rd.idx();
                let e = d.and_then(|d| r.mgr.edge_def(d));
                match e {
                    Some(e)
                        if !skip
                            && (0..=3).contains(&mode)
                            && (mode == 0 || e.xsect_curve_3d().is_some()) =>
                    {
                        let res = if op == p::ED_EVAL {
                            e.evaluate(k, &r.pool, t, mode & 1 != 0, mode & 2 != 0)
                        } else {
                            e.evaluate_mid(k, &r.pool, mode & 1 != 0, mode & 2 != 0)
                        };
                        put_eval(&mut out, st(res)?, mode);
                    }
                    _ => skip = true,
                }
            }
            p::ED_END_POS => {
                let d = r.ed(rd.idx());
                let at_start = rd.flag();
                let e = d.and_then(|d| r.mgr.edge_def(d));
                match e {
                    Some(e) if !skip && e.xsect_curve_3d().is_some() => {
                        match st(e.end_position(k, &r.pool, at_start))? {
                            Ok(pt) => put_vec(&mut out, pt),
                            Err(s) => {
                                status = s;
                                put_vec(&mut out, Vec3::UNDEF);
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_END_POS_PV => {
                let d = r.ed(rd.idx());
                let v = r.pv_or_null(rd.idx(), &mut ok);
                let e = d.and_then(|d| r.mgr.edge_def(d));
                let pe = e
                    .and_then(EdgeDefinition::proto_edge)
                    .and_then(|x| r.mgr.proto_edge(x));
                match (d, e, pe) {
                    (Some(d), Some(e), Some(pe)) if !skip && ok => {
                        if (v == pe.start_proto_vertex() || v == pe.end_proto_vertex())
                            && e.xsect_curve_3d().is_none()
                        {
                            skip = true;
                        } else {
                            match st(r.mgr.edge_def_end_position_at(k, &r.pool, d, v))? {
                                Ok(pt) => put_vec(&mut out, pt),
                                Err(s) => status = s,
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_REVERSE => match r.ed(rd.idx()) {
                Some(d) if !skip => {
                    if let Some(x) = r.mgr.edge_def_mut(d) {
                        status = st0(x.reverse_orientation(k, &mut r.pool))?;
                    }
                }
                _ => skip = true,
            },
            p::ED_REPARAM => {
                let d = r.ed(rd.idx());
                let a = rd.num();
                let bb = rd.num();
                let rev = rd.flag();
                let has_c = d
                    .and_then(|d| r.mgr.edge_def(d))
                    .is_some_and(|e| e.xsect_curve_3d().is_some());
                match d {
                    // a zero-length domain is refused (HANG-12)
                    Some(d) if !skip && has_c && a != bb => {
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            let dom = Extent1d::from_min_max(a, bb);
                            status = st0(x.reparameterize(k, &mut r.pool, &dom, rev))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SET_IVL_MINMAX => {
                let d = r.ed(rd.idx());
                let a = rd.num();
                let bb = rd.num();
                match d {
                    Some(d) if !skip => {
                        if let Some(x) = r.mgr.edge_def_mut(d) {
                            status = st0(x.set_xsect_interval_min_max(k, &mut r.pool, a, bb))?;
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_IS_SAME | p::PE_IS_SAME => {
                let j = rd.idx();
                let v1 = r.pv_or_null(rd.idx(), &mut ok);
                let v2 = r.pv_or_null(rd.idx(), &mut ok);
                let pc1 = r.read_pc(&mut rd);
                let pc2 = r.read_pc(&mut rd);
                let c = r.curve_or_null(rd.idx(), &mut ok);
                let a = rd.num();
                let bb = rd.num();
                match (pc1, pc2) {
                    (Some(pc1), Some(pc2)) if !skip && ok => {
                        let dom = Extent1d::from_min_max(a, bb);
                        let mut defs = Vec::new();
                        if op == p::ED_IS_SAME {
                            match r.ed(j) {
                                Some(d) => defs.push(d),
                                None => skip = true,
                            }
                        } else {
                            match r.pe(j).and_then(|e| r.mgr.proto_edge(e)) {
                                Some(e) => {
                                    if between(v1, v2, e.start_proto_vertex(), e.end_proto_vertex())
                                    {
                                        defs.extend_from_slice(e.edge_definitions());
                                    }
                                }
                                None => skip = true,
                            }
                        }
                        for &d in &defs {
                            let Some(e) = r.mgr.edge_def(d) else {
                                skip = true;
                                continue;
                            };
                            match e.proto_edge().and_then(|x| r.mgr.proto_edge(x)) {
                                None => skip = true,
                                Some(pe) => {
                                    let bt = between(
                                        v1,
                                        v2,
                                        pe.start_proto_vertex(),
                                        pe.end_proto_vertex(),
                                    );
                                    if bt
                                        && e.xsect_curve_3d() != c
                                        && (e.xsect_curve_3d().is_none() || c.is_none())
                                    {
                                        skip = true;
                                    }
                                }
                            }
                        }
                        if !skip {
                            let res = if op == p::ED_IS_SAME {
                                r.mgr.is_same_edge_definition(
                                    k,
                                    &r.pool,
                                    EdgeDefId::from_index(j as u32),
                                    v1,
                                    v2,
                                    &pc1,
                                    &pc2,
                                    c,
                                    &dom,
                                )
                            } else {
                                r.mgr.is_same_proto_edge(
                                    k,
                                    &r.pool,
                                    ProtoEdgeId::from_index(j as u32),
                                    v1,
                                    v2,
                                    &pc1,
                                    &pc2,
                                    c,
                                    &dom,
                                )
                            };
                            match st(res)? {
                                Ok((same, ident)) => out.extend([b(same), b(ident)]),
                                Err(s) => {
                                    status = s;
                                    out.extend([0.0, 0.0]);
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_IS_INTERIOR => {
                let d = r.ed(rd.idx());
                let pt = rd.vec();
                let tol = rd.num();
                let at_start = rd.flag();
                let e = d.and_then(|d| r.mgr.edge_def(d));
                match e {
                    Some(e) if !skip && e.xsect_curve_3d().is_some() => {
                        let xt = xsect_tol3d(k, tol);
                        match st(e.is_interior_point(k, &r.pool, pt, xt, at_start))? {
                            Ok((f, prm)) => out.extend([b(f), prm]),
                            Err(s) => {
                                status = s;
                                out.extend([0.0, 0.0]);
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_START_END_PV => {
                let d = r.ed(rd.idx());
                let has_pe = d
                    .and_then(|d| r.mgr.edge_def(d))
                    .and_then(EdgeDefinition::proto_edge)
                    .is_some();
                match d {
                    Some(d) if !skip && has_pe => {
                        let s = st(r.mgr.edge_def_start_proto_vertex(d))?;
                        let e = st(r.mgr.edge_def_end_proto_vertex(d))?;
                        match (s, e) {
                            (Ok(s), Ok(e)) => out.extend([r.pv_id(s), r.pv_id(e)]),
                            (Err(x), _) | (_, Err(x)) => status = x,
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_END_POS | p::PE_END_POS_PV => {
                let e = r.pe(rd.idx());
                let mut at_start = false;
                let mut v = None;
                if op == p::PE_END_POS {
                    at_start = rd.flag();
                } else {
                    v = r.pv_or_null(rd.idx(), &mut ok);
                }
                match e.and_then(|x| r.mgr.proto_edge(x).map(|y| (x, y))) {
                    Some((eid, ee)) if !skip && ok => {
                        let mut reached = true;
                        if op == p::PE_END_POS_PV {
                            if v == ee.start_proto_vertex() {
                                at_start = true;
                            } else if v == ee.end_proto_vertex() {
                                at_start = false;
                            } else {
                                reached = false;
                            }
                        }
                        if reached {
                            if ee.edge_definitions().is_empty() {
                                let end = if at_start {
                                    ee.start_proto_vertex()
                                } else {
                                    ee.end_proto_vertex()
                                };
                                if end.is_none() {
                                    skip = true;
                                }
                            } else if ee.edge_definitions().iter().any(|&d| {
                                r.mgr
                                    .edge_def(d)
                                    .is_none_or(|x| x.xsect_curve_3d().is_none())
                            }) {
                                skip = true;
                            }
                        }
                        if !skip {
                            let res = if op == p::PE_END_POS {
                                r.mgr.proto_edge_end_position(k, &r.pool, eid, at_start)
                            } else {
                                r.mgr.proto_edge_end_position_at(k, &r.pool, eid, v)
                            };
                            match st(res)? {
                                Ok(pt) => put_vec(&mut out, pt),
                                Err(s) => status = s,
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_EVAL | p::PE_EVAL_MID => {
                let e = r.pe(rd.idx());
                let t = if op == p::PE_EVAL { rd.num() } else { 0.0 };
                let mode = rd.idx();
                match e.and_then(|x| r.mgr.proto_edge(x).map(|y| (x, y))) {
                    Some((eid, ee)) if !skip && (0..=3).contains(&mode) => {
                        if mode != 0
                            && ee.edge_definitions().iter().any(|&d| {
                                r.mgr
                                    .edge_def(d)
                                    .is_none_or(|x| x.xsect_curve_3d().is_none())
                            })
                        {
                            skip = true;
                        } else {
                            let res = if op == p::PE_EVAL {
                                r.mgr.proto_edge_evaluate(k, &r.pool, eid, t, mode & 1 != 0, mode & 2 != 0)
                            } else {
                                r.mgr.proto_edge_evaluate_mid(k, &r.pool, eid, mode & 1 != 0, mode & 2 != 0)
                            };
                            put_eval(&mut out, st(res)?, mode);
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PE_GROUP => match r.pe(rd.idx()) {
                Some(e) if !skip => match st(r.mgr.overlapping_pe_group(e))? {
                    Ok(g) => {
                        out.push(g.len() as f64);
                        out.extend(g.iter().map(|&x| r.pe_id(Some(x))));
                    }
                    Err(s) => status = s,
                },
                _ => skip = true,
            },
            p::PV_FIND_MATCH => {
                let v = r.pv(rd.idx());
                let a = rd.idx();
                let bb = rd.idx();
                if (a != -1 && !r.pool.valid(a)) || (bb != -1 && !r.pool.valid(bb)) {
                    ok = false;
                }
                match v {
                    Some(v) if !skip && ok => {
                        let t1 = (a != -1).then(|| r.pool.h(a));
                        let t2 = (bb != -1).then(|| r.pool.h(bb));
                        match st(r.mgr.find_matching_vertex_definition(v, t1, t2))? {
                            Ok(x) => out.push(r.vd_id(x)),
                            Err(s) => status = s,
                        }
                    }
                    _ => skip = true,
                }
            }
            p::PV_CLOSEST => {
                let v = r.pv(rd.idx());
                let pt = rd.vec();
                match v {
                    Some(v) if !skip => match st(r.mgr.closest_vertex_definition(v, pt))? {
                        Ok(x) => out.push(r.vd_id(x)),
                        Err(s) => status = s,
                    },
                    _ => skip = true,
                }
            }
            p::PV_FINAL_VTX => {
                let v = r.pv(rd.idx());
                let w = rd.idx();
                let d = r.ed(rd.idx());
                let at_start = rd.flag();
                match (v, d) {
                    (Some(v), Some(d)) if !skip => {
                        let vv = r.mgr.proto_vertex(v);
                        let e = r.mgr.edge_def(d);
                        if let (Some(vv), Some(e)) = (vv, e) {
                            if !vv.vertex_definitions().is_empty() {
                                if e.xsect_curve_3d().is_none() {
                                    skip = true;
                                }
                                for &x in vv.vertex_definitions() {
                                    let fv =
                                        r.mgr.vertex_def(x).and_then(|y| y.final_vertex(w as i32));
                                    if fv.is_some()
                                        && connect_crashes(&r.pool, e.src_topo(w as i32), fv)
                                    {
                                        skip = true;
                                    }
                                }
                            }
                        }
                        if !skip {
                            match st(r.mgr.proto_vertex_final_vertex(k, &r.pool, v, w as i32, d, at_start))? {
                                Ok(x) => out.push(r.pool.id(x)),
                                Err(s) => status = s,
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::MGR_FIND_PV => {
                let pt = rd.vec();
                if !skip {
                    match st(r.mgr.find_proto_vertex(pt))? {
                        Ok((v, dist)) => out.extend([r.pv_id(v), dist]),
                        Err(s) => status = s,
                    }
                }
            }
            p::MGR_FIND_ED_FROM_EDGE => {
                let j = rd.idx();
                let w = rd.idx();
                if j != -1 && !r.pool.is(j, pk::EDGE) {
                    ok = false;
                }
                if skip || !ok {
                    skip = true;
                } else {
                    let h = (j != -1).then(|| r.pool.h(j));
                    match st(r.mgr.find_edge_definition_from_edge(h, w as i32))? {
                        Ok(x) => out.push(r.ed_id(x)),
                        Err(s) => status = s,
                    }
                }
            }
            p::MGR_ALL_VDS => {
                if !skip {
                    match st(r.mgr.all_vertex_definitions())? {
                        Ok(a) => {
                            out.push(a.len() as f64);
                            out.extend(a.iter().map(|&x| r.vd_id(Some(x))));
                        }
                        Err(s) => status = s,
                    }
                }
            }
            p::MGR_ALL_EDS => {
                if !skip {
                    match st(r.mgr.all_edge_definitions())? {
                        Ok(a) => {
                            out.push(a.len() as f64);
                            out.extend(a.iter().map(|&x| r.ed_id(Some(x))));
                        }
                        Err(s) => status = s,
                    }
                }
            }
            p::MGR_CHECK_OVERLAP | p::ARE_OVERLAPPING => {
                let cc_curve = r.curve_or_null(rd.idx(), &mut ok);
                let cc_tol = rd.num();
                let a = rd.num();
                let bb = rd.num();
                let m1 = r.read_pc(&mut rd);
                let mut m2 = None;
                let mut d = None;
                if op == p::MGR_CHECK_OVERLAP {
                    m2 = r.read_pc(&mut rd);
                    if m2.is_none() {
                        ok = false;
                    }
                } else {
                    d = r.ed(rd.idx());
                }
                let d_ok = d
                    .and_then(|d| r.mgr.edge_def(d))
                    .is_some_and(|e| e.xsect_curve_3d().is_some());
                match m1 {
                    Some(m1)
                        if !skip
                            && ok
                            && cc_curve.is_some()
                            && (op != p::ARE_OVERLAPPING || d_ok) =>
                    {
                        let mut cc = CurveClassification::new(k);
                        cc.set_curve(cc_curve, false);
                        cc.set_src_zone_tol_3d(zone_tol3d(k, cc_tol));
                        let mut ci1 = CurveInterval::with_interval(
                            k,
                            &Extent1d::from_min_max(a, bb),
                            Some(&cc),
                        );
                        ci1.mid = m1;
                        if op == p::ARE_OVERLAPPING {
                            let e = d.and_then(|d| r.mgr.edge_def(d));
                            match e {
                                Some(e) => {
                                    match st(proto_mod::are_overlapping(k, &r.pool, &cc, &ci1, e))?
                                    {
                                        Ok(f) => out.push(b(f)),
                                        Err(s) => status = s,
                                    }
                                }
                                None => skip = true,
                            }
                        } else if let Some(m2) = m2 {
                            let mut ci2 = CurveInterval::with_interval(
                                k,
                                &Extent1d::from_min_max(a, bb),
                                None,
                            );
                            ci2.mid = m2;
                            if m1.edge_object().is_some() || m2.edge_object().is_some() {
                                if let Ok(all) = r.mgr.all_edge_definitions() {
                                    let topo_of = |o: Option<H>| {
                                        o.filter(|h| {
                                            matches!(
                                                h.1,
                                                ClassObjectKind::Vertex
                                                    | ClassObjectKind::Edge
                                                    | ClassObjectKind::Face
                                                    | ClassObjectKind::Loop
                                                    | ClassObjectKind::Region
                                            )
                                        })
                                    };
                                    for x in all {
                                        if let Some(e) = r.mgr.edge_def(x) {
                                            if e.src_topo(1) == topo_of(m1.object())
                                                && e.src_topo(2) == topo_of(m2.object())
                                                && e.xsect_curve_3d().is_none()
                                            {
                                                skip = true;
                                            }
                                        }
                                    }
                                }
                            }
                            if !skip {
                                match st(r
                                    .mgr
                                    .check_overlapping_proto_edges(k, &r.pool, &cc, &ci1, &ci2))?
                                {
                                    Ok(res) => {
                                        out.push(res.len() as f64);
                                        out.extend(res.iter().map(|&x| r.pe_id(x)));
                                    }
                                    Err(s) => {
                                        status = s;
                                        out.push(0.0);
                                    }
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::MGR_CAN_COMBINE => {
                let d1 = r.ed(rd.idx());
                let d2 = r.ed(rd.idx());
                match (d1, d2) {
                    (Some(d1), Some(d2)) if !skip => {
                        let (e1, e2) = (r.mgr.edge_def(d1), r.mgr.edge_def(d2));
                        if let (Some(e1), Some(e2)) = (e1, e2) {
                            let same = e1.src_topo(1) == e2.src_topo(1)
                                && e1.src_topo(2) == e2.src_topo(2);
                            if !same
                                && (connect_crashes(&r.pool, e1.src_topo(1), e2.src_topo(1))
                                    || connect_crashes(&r.pool, e1.src_topo(2), e2.src_topo(2)))
                            {
                                skip = true;
                            }
                        }
                        if !skip {
                            match st(r.mgr.can_combine_edge_definitions(k, &r.pool, d1, d2))? {
                                Ok(c) => out.push(c as f64),
                                Err(s) => {
                                    status = s;
                                    out.push(99.0);
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::MGR_REMOVE_NOEDGE_PV => {
                let e = r.pe(rd.idx());
                let v = r.pv(rd.idx());
                match (e, v) {
                    (Some(e), Some(v)) if !skip => {
                        if let Some(ee) = r.mgr.proto_edge(e) {
                            if ee.start_proto_vertex() != Some(v)
                                && ee.end_proto_vertex() != Some(v)
                                && ee.edge_definitions().first().is_some_and(|&d| {
                                    r.mgr
                                        .edge_def(d)
                                        .is_none_or(|x| x.xsect_curve_3d().is_none())
                                })
                            {
                                skip = true;
                            }
                        }
                        if !skip {
                            let res = r
                                .mgr
                                .remove_coincident_no_edge_proto_vertex(k, &r.pool, e, v);
                            match st(res)? {
                                Ok(f) => out.push(b(f)),
                                Err(s) => {
                                    status = s;
                                    out.push(0.0);
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::WHICH_PV => {
                let e = r.pe(rd.idx());
                let t = rd.num();
                let sv = r.pv_or_null(rd.idx(), &mut ok);
                match e.and_then(|x| r.mgr.proto_edge(x).map(|y| (x, y))) {
                    Some((eid, ee)) if !skip && ok => {
                        if sv.is_none() {
                            if ee.edge_definitions().iter().any(|&d| {
                                r.mgr
                                    .edge_def(d)
                                    .is_none_or(|x| x.xsect_curve_3d().is_none())
                            }) {
                                skip = true;
                            }
                            if ee.edge_definitions().is_empty()
                                || ee.start_proto_vertex().is_none()
                                || ee.end_proto_vertex().is_none()
                            {
                                skip = true;
                            }
                        }
                        if !skip {
                            match st(r.mgr.which_pv_to_combine(k, &r.pool, eid, t, sv))? {
                                Ok(x) => out.push(r.pv_id(x)),
                                Err(s) => status = s,
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::FIND_POS => {
                let pc1 = r.read_pc(&mut rd);
                let pc2 = r.read_pc(&mut rd);
                match (pc1, pc2) {
                    (Some(mut pc1), Some(mut pc2)) if !skip => {
                        let (pt, dev) = match st(proto_mod::find_position_from_pcs(
                            k, &r.pool, &mut pc1, &mut pc2,
                        ))? {
                            Ok(x) => (x.position, x.deviation),
                            Err(s) => {
                                status = s;
                                (Vec3::new(0.0, 0.0, 0.0), -1.0)
                            }
                        };
                        put_vec(&mut out, pt);
                        out.push(dev);
                        r.put_pc(&mut out, &pc1);
                        r.put_pc(&mut out, &pc2);
                    }
                    _ => skip = true,
                }
            }
            p::SAME_CURVE => {
                let c1 = r.curve_or_null(rd.idx(), &mut ok);
                let a1 = rd.num();
                let b1 = rd.num();
                let c2 = r.curve_or_null(rd.idx(), &mut ok);
                let a2 = rd.num();
                let b2 = rd.num();
                let tol = rd.num();
                if skip || !ok || (c1 != c2 && (c1.is_none() || c2.is_none())) {
                    skip = true;
                } else {
                    let i1 = Extent1d::from_min_max(a1, b1);
                    let i2 = Extent1d::from_min_max(a2, b2);
                    match st(proto_mod::are_same_curve(k, &r.pool, c1, &i1, c2, &i2, tol))? {
                        Ok(f) => out.push(b(f)),
                        Err(s) => status = s,
                    }
                }
            }
            p::IS_INTERIOR => {
                let c = r.curve_or_null(rd.idx(), &mut ok);
                let a = rd.num();
                let bb = rd.num();
                let t = rd.num();
                let tol = rd.num();
                if skip || !ok || c.is_none() {
                    skip = true;
                } else {
                    let dom = Extent1d::from_min_max(a, bb);
                    match st(proto_mod::is_interior(k, &r.pool, c, &dom, t, zone_tol3d(k, tol)))? {
                        Ok(f) => out.push(b(f)),
                        Err(s) => status = s,
                    }
                }
            }
            p::ARE_CONNECTED => {
                let a = rd.idx();
                let bb = rd.idx();
                if (a != -1 && !r.pool.is_topo(a)) || (bb != -1 && !r.pool.is_topo(bb)) {
                    ok = false;
                }
                let t1 = (a != -1 && ok).then(|| r.pool.h(a));
                let t2 = (bb != -1 && ok).then(|| r.pool.h(bb));
                if skip || !ok || connect_crashes(&r.pool, t1, t2) {
                    skip = true;
                } else {
                    match st(proto_mod::are_connected(&r.pool, t1, t2))? {
                        Ok(f) => out.push(b(f)),
                        Err(s) => status = s,
                    }
                }
            }
            p::TOPO_DIM => {
                let t = r.topo(rd.idx(), &mut ok);
                match t {
                    Some(t) if !skip && ok => out.push(f64::from(proto_mod::topo_dimension(t))),
                    _ => skip = true,
                }
            }
            p::CAN_IMPRINT => {
                let c = rd.idx();
                match PointClassType::from_code(c as i32) {
                    Some(t) if !skip && (100..=110).contains(&c) => {
                        out.push(b(proto_mod::can_imprint(t)));
                    }
                    _ => skip = true,
                }
            }
            p::MGR_TYPE => {
                if !skip {
                    put_type(
                        &mut out,
                        proto_mod::MANAGER_TYPE,
                        proto_mod::manager_is_kind_of,
                        proto_mod::MANAGER_TYPE_STRING,
                        proto_mod::MANAGER_CLASS_STRING,
                    );
                }
            }
            p::MGR_GET => {
                if !skip {
                    out.push(b(r.mgr.which_brep(1) == brep1));
                    out.push(b(r.mgr.which_brep(2) == brep2));
                    out.push(r.mgr.tolerance(k));
                    out.push(r.mgr.capture_distance());
                }
            }
            p::FOC_PV => {
                let pc1 = r.read_pc(&mut rd);
                let pc2 = r.read_pc(&mut rd);
                let pos_ok = match (&pc1, &pc2) {
                    (Some(a), Some(c)) => r.position_ok(a, c)?,
                    _ => true,
                };
                match (pc1, pc2) {
                    (Some(mut a), Some(mut c)) if !skip && pos_ok => {
                        let called = st3(r
                            .mgr
                            .find_or_create_proto_vertex_pcs(k, &r.pool, &mut a, &mut c))?;
                        super::topology_proto_topology_p2::sync_counts(&mut r);
                        match called {
                            Ok((pv, created)) => {
                                out.push(r.pv_id(pv));
                                out.push(b(created));
                            }
                            Err(s) => {
                                status = s;
                                out.push(-1.0);
                                out.push(0.0);
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::FOC_PE => {
                let cc_curve = r.curve_or_null(rd.idx(), &mut ok);
                let cc_tol = rd.num();
                let a = rd.num();
                let bb = rd.num();
                let pcs: Vec<Option<PointClassification<H>>> =
                    (0..6).map(|_| r.read_pc(&mut rd)).collect();
                let mut pos_ok = true;
                for i in 0..3 {
                    if let (Some(a), Some(c)) = (
                        pcs.get(i).and_then(Option::as_ref),
                        pcs.get(i + 3).and_then(Option::as_ref),
                    ) {
                        pos_ok &= r.position_ok(a, c)?;
                    }
                }
                if skip
                    || !ok
                    || cc_curve.is_none()
                    || a > bb
                    || pcs.iter().any(Option::is_none)
                    || pcs
                        .iter()
                        .flatten()
                        .all(|pc| proto_mod::can_imprint(pc.point_class()))
                    || !pos_ok
                {
                    skip = true;
                } else {
                    let p: Vec<PointClassification<H>> = pcs.into_iter().flatten().collect();
                    let Ok::<[PointClassification<H>; 6], _>([p0, p1, p2, p3, p4, p5]) =
                        p.try_into()
                    else {
                        return Err(harness("six classifications").into());
                    };
                    let mut cc = CurveClassification::new(k);
                    cc.set_curve(cc_curve, false);
                    cc.set_src_zone_tol_3d(zone_tol3d(k, cc_tol));
                    let ivl = Extent1d::from_min_max(a, bb);
                    let mut ci1 = CurveInterval::with_interval(k, &ivl, Some(&cc));
                    ci1.start = p0;
                    ci1.mid = p1;
                    ci1.end = p2;
                    let mut ci2 = CurveInterval::with_interval(k, &ivl, None);
                    ci2.start = p3;
                    ci2.mid = p4;
                    ci2.end = p5;
                    if ci1.mid.edge_object().is_some() || ci2.mid.edge_object().is_some() {
                        if let Ok(all) = r.mgr.all_edge_definitions() {
                            let topo_of = |o: Option<H>| {
                                o.filter(|h| {
                                    matches!(
                                        h.1,
                                        ClassObjectKind::Vertex
                                            | ClassObjectKind::Edge
                                            | ClassObjectKind::Face
                                            | ClassObjectKind::Loop
                                            | ClassObjectKind::Region
                                    )
                                })
                            };
                            for x in all {
                                if let Some(e) = r.mgr.edge_def(x) {
                                    if e.src_topo(1) == topo_of(ci1.mid.object())
                                        && e.src_topo(2) == topo_of(ci2.mid.object())
                                        && e.xsect_curve_3d().is_none()
                                    {
                                        skip = true;
                                    }
                                }
                            }
                        }
                    }
                    if !skip {
                        let called = st3(r.mgr.find_or_create_proto_edge(
                            k,
                            &mut r.pool,
                            approx_tol,
                            &cc,
                            &mut ci1,
                            &cc,
                            &mut ci2,
                        ))?;
                        super::topology_proto_topology_p2::sync_counts(&mut r);
                        match called {
                            Ok((pe, created)) => {
                                out.push(r.pe_id(pe));
                                out.push(b(created));
                            }
                            Err(s) => {
                                status = s;
                                out.push(-1.0);
                                out.push(0.0);
                            }
                        }
                    }
                }
            }
            p::PE_SPLIT => {
                let e = r.pe(rd.idx());
                let t = rd.num();
                match e {
                    Some(e) if !skip => {
                        let interior = r
                            .mgr
                            .proto_edge(e)
                            .is_some_and(|x| x.xsect_interval().contains_value(t, -EFF_ZERO));
                        if interior && (!r.pe_split_ok(e) || !r.ed_curves_unique()) {
                            skip = true;
                        } else {
                            let called = st3(r.mgr.proto_edge_split(k, &mut r.pool, e, t))?;
                            super::topology_proto_topology_p2::sync_counts(&mut r);
                            match called {
                                Ok((pv, pe)) => {
                                    out.push(r.pv_id(pv));
                                    out.push(r.pe_id(pe));
                                }
                                Err(s) => {
                                    status = s;
                                    out.push(-1.0);
                                    out.push(-1.0);
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::ED_SPLIT => {
                let d = r.ed(rd.idx());
                let t = rd.num();
                let mid = r.pv_or_null(rd.idx(), &mut ok);
                let top = r.pe(rd.idx());
                match (d, mid, top) {
                    (Some(d), Some(mid), Some(top)) if !skip && ok => {
                        let interior = r
                            .mgr
                            .edge_def(d)
                            .is_some_and(|x| x.xsect_interval().contains_value(t, -EFF_ZERO));
                        let pe_live = r
                            .mgr
                            .edge_def(d)
                            .and_then(EdgeDefinition::proto_edge)
                            .is_some_and(|pe| r.mgr.proto_edge(pe).is_some());
                        let has_curve = r
                            .mgr
                            .edge_def(d)
                            .is_some_and(|x| x.xsect_curve_3d().is_some());
                        if interior && (!has_curve || !pe_live || !r.ed_curves_unique()) {
                            skip = true;
                        } else {
                            let called = st3(r.mgr.edge_definition_split(
                                k,
                                &mut r.pool,
                                d,
                                t,
                                Some(mid),
                                Some(top),
                            ))?;
                            super::topology_proto_topology_p2::sync_counts(&mut r);
                            match called {
                                Ok((vd, ed)) => {
                                    out.push(r.vd_id(vd));
                                    out.push(r.ed_id(ed));
                                }
                                Err(s) => {
                                    status = s;
                                    out.push(-1.0);
                                    out.push(-1.0);
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::SPLIT_INT => {
                let e1 = r.pe(rd.idx());
                let e2 = r.pe(rd.idx());
                match (e1, e2) {
                    (Some(e1), Some(e2)) if !skip => {
                        let guard = match (r.mgr.proto_edge(e1), r.mgr.proto_edge(e2)) {
                            (Some(a), Some(c)) => {
                                let shared = a.start_proto_vertex() == c.start_proto_vertex()
                                    || a.start_proto_vertex() == c.end_proto_vertex()
                                    || a.end_proto_vertex() == c.start_proto_vertex()
                                    || a.end_proto_vertex() == c.end_proto_vertex();
                                !shared
                                    && a.num_edge_definitions() >= 1
                                    && c.num_edge_definitions() >= 1
                                    && !a.overlapping_pes().contains(&e2)
                                    && (!r.pe_split_ok(e1)
                                        || !r.pe_split_ok(e2)
                                        || !r.ed_curves_unique()
                                        || !r.listed_with_ends(e1)
                                        || !r.listed_with_ends(e2))
                            }
                            _ => true,
                        };
                        if guard {
                            skip = true;
                        } else {
                            let called =
                                st3(r.mgr.split_intersecting_proto_edges(k, &mut r.pool, e1, e2))?;
                            super::topology_proto_topology_p2::sync_counts(&mut r);
                            match called {
                                Ok(deleted) => {
                                    let ids = r.kill_curves(deleted);
                                    out.push(ids.len() as f64);
                                    out.extend(ids);
                                }
                                Err(s) => {
                                    status = s;
                                    out.push(0.0);
                                }
                            }
                        }
                    }
                    _ => skip = true,
                }
            }
            p::GLUE => {
                if !skip {
                    let m = &r.mgr;
                    for &pv in m.proto_vertices() {
                        let vds = m
                            .proto_vertex(pv)
                            .map(|x| x.vertex_definitions().to_vec())
                            .unwrap_or_default();
                        for &vd in vds.iter().skip(1) {
                            let first = vds.first().and_then(|&f| m.vertex_def(f));
                            let this = m.vertex_def(vd);
                            if let (Some(f), Some(t)) = (first, this) {
                                // only the paths that do not glue run: a pair of final vertices
                                // must be one vertex or two vertices with points that are far
                                // apart (a bare stand-in has no usable point; a glue needs a B-rep)
                                if let (Some(a1), Some(b1), Some(a2), Some(b2)) = (
                                    f.final_vertex(1),
                                    t.final_vertex(1),
                                    f.final_vertex(2),
                                    t.final_vertex(2),
                                ) {
                                    for (x, y) in [(a1, b1), (a2, b2)] {
                                        match (r.pool.point_of(x), r.pool.point_of(y)) {
                                            (Some(px), Some(py)) => {
                                                // `dist > xs` is the no-glue test of the port
                                                let xs = match (
                                                    r.pool.topology_tolerance(x),
                                                    r.pool.topology_tolerance(y),
                                                ) {
                                                    (Some(tx), Some(ty)) => tx + ty,
                                                    _ => f64::INFINITY,
                                                };
                                                if x != y && px.distance(py) <= xs {
                                                    skip = true;
                                                }
                                            }
                                            _ => skip = true,
                                        }
                                    }
                                }
                            }
                        }
                    }
                    if !skip {
                        // (the shim's intersector relates nothing: GetOtherMate is always null)
                        match st3(r.mgr.glue_coincident_brep_vertices(
                            &mut r.k,
                            &mut r.pool,
                            &mut solidcore::topology::proto_topology::p3::NoMates,
                        ))? {
                            Ok(deleted) => {
                                let ids = r.kill_curves(deleted);
                                out.push(ids.len() as f64);
                                out.extend(ids);
                            }
                            Err(s) => {
                                status = s;
                                out.push(0.0);
                            }
                        }
                    }
                }
            }
            p::RESOLVE => {
                // a proto edge listed twice in the manager crashes the C++ (UB-132)
                if r.mgr
                    .proto_edges()
                    .iter()
                    .enumerate()
                    .any(|(i, e)| r.mgr.proto_edges().iter().skip(i + 1).any(|f| f == e))
                {
                    skip = true;
                }
                if !skip {
                    let m = &r.mgr;
                    let mut grp: Vec<ProtoEdgeId> = Vec::new();
                    for i in 0..r.n_pe {
                        let id = ProtoEdgeId::from_index(i);
                        if let Some(e) = m.proto_edge(id) {
                            if !e.overlapping_pes().is_empty() {
                                grp.push(id);
                                grp.extend(e.overlapping_pes().iter().copied());
                            }
                        }
                    }
                    for &e in &grp {
                        let live_pv = |pv: Option<ProtoVertexId>| {
                            pv.is_some_and(|v| m.proto_vertex(v).is_some())
                        };
                        match m.proto_edge(e) {
                            Some(x)
                                if live_pv(x.start_proto_vertex())
                                    && live_pv(x.end_proto_vertex()) =>
                            {
                                if !r.listed_with_ends(e) {
                                    skip = true;
                                    continue;
                                }
                                // upstream crashes (reads a deleted edge) unless every overlap relation is
                                // symmetric, no overlap list repeats an edge and each edge is in its own group
                                let ov = x.overlapping_pes();
                                let mut group: Vec<ProtoEdgeId> = Vec::new();
                                for (a, &o) in ov.iter().enumerate() {
                                    if ov.iter().skip(a + 1).any(|&q| q == o) {
                                        skip = true;
                                    }
                                    match m.proto_edge(o) {
                                        Some(oe) if oe.overlapping_pes().contains(&e) => {
                                            group.push(o);
                                        }
                                        _ => skip = true,
                                    }
                                }
                                let mut g = 0;
                                while g < group.len() && !skip {
                                    match group.get(g).and_then(|&ge| m.proto_edge(ge)) {
                                        Some(og) => {
                                            for &c in og.overlapping_pes() {
                                                if m.proto_edge(c).is_none() {
                                                    skip = true;
                                                } else if !group.contains(&c) {
                                                    group.push(c);
                                                }
                                            }
                                        }
                                        None => skip = true,
                                    }
                                    g += 1;
                                }
                                if skip || !group.contains(&e) {
                                    skip = true;
                                    continue;
                                }
                                // a vertex left without edges but with definitions is deleted
                                for v in [x.start_proto_vertex(), x.end_proto_vertex()]
                                    .into_iter()
                                    .flatten()
                                {
                                    if let Some(pv) = m.proto_vertex(v) {
                                        let left =
                                            pv.proto_edges().iter().any(|pe| !grp.contains(pe));
                                        if !left && pv.num_vertex_definitions() > 0 {
                                            skip = true;
                                        }
                                    }
                                }
                            }
                            _ => skip = true,
                        }
                    }
                    for i in 0..r.n_ed {
                        if let Some(d) = m.edge_def(EdgeDefId::from_index(i)) {
                            for w in [1, 2] {
                                let t = d.src_topo(w);
                                if t.is_none() || r.pool.kind_of(t) == pk::EDGE {
                                    skip = true;
                                }
                            }
                        }
                    }
                    for i in 0..r.n_pf {
                        if let Some(f) = m.proto_face(ProtoFaceId::from_index(i)) {
                            if f.coinc_proto_edges()
                                .iter()
                                .any(|&e| m.proto_edge(e).is_none())
                            {
                                skip = true;
                            }
                        }
                    }
                    if skip || !r.ed_curves_unique() {
                        skip = true;
                    } else {
                        match st3(r.mgr.resolve_overlapping_proto_edges(
                            k,
                            &mut r.pool,
                            approx_tol,
                        ))? {
                            Ok(deleted) => {
                                let ids = r.kill_curves(deleted);
                                out.push(ids.len() as f64);
                                out.extend(ids);
                            }
                            Err(s) => {
                                status = s;
                                out.push(0.0);
                            }
                        }
                    }
                }
            }
            op if (q::FIRST..=q::LAST).contains(&op) => {
                super::topology_proto_topology_p2::run_op(
                    &mut r,
                    &mut rd,
                    op,
                    &mut out,
                    &mut status,
                    &mut skip,
                )?;
            }
            _ => return Err(harness("proto op").into()),
        }
        if rd.bad {
            return Err(harness("op arguments").into());
        }
        if status == REFUSED {
            skip = true;
        }
        if !skip && !r.invariant() {
            skip = true;
        }
        if skip {
            o.push(SKIP);
            return Ok(o);
        }
        o.push(status);
        o.push(out.len() as f64);
        o.extend(out);
    }
    r.put_state(&mut o);
    Ok(o)
}

// ---- TRANSFORM scripts ----------------------------------------------------------------------

/// The owner the shim gives an owned object (any object: only its presence matters).
const ANCHOR: ObjectRef = ObjectRef {
    type_code: Line::TYPE,
    key: ObjectKey(1),
};

fn run_transform(obj: &TObjIn, op: &TOpIn) -> Result<Vec<f64>, Fail> {
    let mut k = Kernel::default();
    let mut curve = None;
    let mut surface = None;
    // a curve kind the API refuses (no state is reported for it)
    let mut special: Option<Curve> = None;
    let mut poly: Option<PolyBrep> = None;
    match obj {
        TObjIn::Poly => poly = Some(PolyBrep::new(&k, 1.0e-5)),
        TObjIn::PolyPts { pts } => {
            let mut b = PolyBrep::new(&k, 1.0e-5);
            for q in pts {
                b.add_vertex(&k, Vec3::new(q[0], q[1], q[2]), q[3])
                    .map_err(|e| harness(format!("poly vertex: {e}")))?;
            }
            poly = Some(b);
        }
        TObjIn::Curve { owned, c } => {
            let mut c = build_curve(c)?;
            if *owned {
                let _ = c.set_owner(Some(ANCHOR), None);
            }
            curve = Some(c);
        }
        TObjIn::Plane { owned, o, n } => {
            let mut s = Surface::from(Plane::from_normal(v3(*o), v3(*n)));
            if *owned {
                let _ = s.set_owner(Some(ANCHOR));
            }
            surface = Some(s);
        }
        TObjIn::Bsurf { owned, s } => {
            let mut s = build_surface(s)?;
            if *owned {
                let _ = s.set_owner(Some(ANCHOR));
            }
            surface = Some(s);
        }
        TObjIn::Composite { owned, cs } => {
            let curves = cs.iter().map(build_curve).collect::<Result<Vec<_>, _>>()?;
            let c = CompositeCurve::with_curves(3, curves, false, None, None)
                .map_err(|e| harness(format!("composite: {e}")))?;
            let mut c = Curve::from(c);
            if *owned {
                let _ = c.set_owner(Some(ANCHOR), None);
            }
            special = Some(c);
        }
        TObjIn::CrvOnSurf { owned, s, a, b: bb } => {
            let net = s
                .net
                .iter()
                .map(|row| {
                    row.iter()
                        .map(|c| CPoint::new(c[0], c[1], c[2], c[3]))
                        .collect()
                })
                .collect();
            let sur = NurbSurface::new(net, s.p, s.q, s.u.clone(), s.v.clone())
                .map_err(|e| harness(format!("surface: {e}")))?;
            let mut bs = BSplineSurface::from_nurb(sur, false);
            *bs.out_of_bounds_enabled_mut() = s.oob;
            let c = CrvOnSurf::from_uv_line(
                &k,
                Point2::new(a[0], a[1]),
                Point2::new(bb[0], bb[1]),
                bs,
                None,
                0,
            )
            .map_err(|e| harness(format!("crv on surf: {e}")))?;
            let mut c = Curve::from(c);
            if *owned {
                let _ = c.set_owner(Some(ANCHOR), None);
            }
            special = Some(c);
        }
        _ => {}
    }
    let mut records: BTreeMap<ObjectKey, Topology> = BTreeMap::new();
    let mut attributes = AttributeStore::new();
    let mut caches: CacheStore<()> = CacheStore::new();
    let mut env = PolyNotifyEnv {
        records: &mut records,
        attributes: &mut attributes,
        caches: &mut caches,
        context: None,
        this: ObjectRef {
            type_code: PolyObjectKind::Brep.type_code(),
            key: ObjectKey(0),
        },
    };
    let mut poly_target = poly.as_mut().map(|b| PolyBrepTarget {
        brep: b,
        env: &mut env,
    });
    let mut brep = Brep::new();
    let target = match obj {
        TObjIn::Composite { .. } | TObjIn::CrvOnSurf { .. } => {
            special.as_mut().map(TransformObject::Curve)
        }
        TObjIn::Curve { .. } => curve.as_mut().map(TransformObject::Curve),
        TObjIn::Plane { .. } | TObjIn::Bsurf { .. } => {
            surface.as_mut().map(TransformObject::Surface)
        }
        TObjIn::Brep => Some(TransformObject::Brep(&mut brep)),
        TObjIn::Poly | TObjIn::PolyPts { .. } => poly_target
            .as_mut()
            .map(|t| TransformObject::PolyBrep(t as &mut dyn PolyBrepTransform)),
        TObjIn::Other => Some(TransformObject::Other),
        TObjIn::Null => None,
    };
    let res = match op {
        TOpIn::Transform { o, x, y } => {
            let pl = Axis2Placement::new(v3(*o), v3(*x), v3(*y));
            api::api_transform(&mut k, target, &pl)
        }
        TOpIn::Scale { s } => api::api_scale(&mut k, target, v3(*s)),
        TOpIn::ScaleByPt { r, s } => api::api_scale_by_pt(&mut k, target, v3(*r), v3(*s)),
        TOpIn::Translate { v } => api::api_translate(&mut k, target, v3(*v)),
        TOpIn::Rotate { r, a, deg } => api::api_rotate(&mut k, target, v3(*r), v3(*a), *deg),
    };
    let s = match res {
        Ok(()) => SM_SUCCESS,
        Err(e) => match Output::unported(&e) {
            Some(d) => return Err(Fail::Deferred(Box::new(d))),
            None => match e {
                SolidError::InvalidInput { .. } => 1007.0,
                e => status(&e),
            },
        },
    };
    let mut o = vec![s];
    if let Some(c) = &curve {
        put_curve(&k, &mut o, c);
    }
    if let Some(sf) = &surface {
        put_surface(&k, &mut o, sf);
    }
    if let Some(b) = &poly {
        // the poly B-rep's tolerance, then each vertex's point and tolerance
        o.push(b.tolerance());
        let vs = b.poly_vertices();
        o.push(vs.len() as f64);
        for v in vs {
            let q = b
                .vertex_point(v)
                .map_err(|e| harness(format!("poly vertex: {e}")))?;
            let t = b
                .vertex_tolerance(v)
                .map_err(|e| harness(format!("poly vertex: {e}")))?;
            o.extend([q.x, q.y, q.z, t]);
        }
    }
    Ok(o)
}

pub(super) fn rust_run(s: &Script) -> Result<Vec<f64>, Fail> {
    match s {
        Script::List { slots, ops } => run_list(slots, ops),
        Script::Proto {
            objects,
            approx_tol,
            capture,
            override_tol,
            ops,
        } => run_proto(objects, *approx_tol, *capture, *override_tol, ops),
        Script::Transform { obj, op } => run_transform(obj, op),
    }
}

pub(super) fn rust(s: &Script) -> Result<Output, HarnessError> {
    match rust_run(s) {
        Ok(v) => Ok(Output::new().values("out", v)),
        Err(Fail::Deferred(d)) => Ok(*d),
        Err(Fail::Harness(e)) => Err(e),
    }
}

// ---- generation -----------------------------------------------------------------------------

pub(super) fn rp(rng: &mut Rng, s: f64) -> [f64; 3] {
    rng.point3(-s, s)
}

/// A random clamped B-spline (sometimes rational or 2D).
fn gen_bspline(rng: &mut Rng, scale: f64) -> BSplineIn {
    let p = 1 + rng.below(4) as usize;
    let n = p + rng.below(5) as usize;
    let rational = rng.chance(0.25);
    let two_d = rng.chance(0.08);
    let a = if rng.chance(0.5) {
        0.0
    } else {
        rng.uniform(-5.0, 5.0)
    };
    let len = if rng.chance(0.5) {
        1.0
    } else {
        rng.uniform(0.1, 10.0)
    };
    let mut u = vec![a; p + 1];
    let mut ks: Vec<f64> = (0..n - p)
        .map(|_| a + len * rng.uniform(0.05, 0.95))
        .collect();
    ks.sort_by(f64::total_cmp);
    u.extend(ks);
    u.extend(vec![a + len; p + 1]);
    let cpts = (0..=n)
        .map(|i| {
            let [x, y, z] = rp(rng, scale);
            let x = x + i as f64 * scale * 0.3;
            let w = if rational { rng.uniform(0.3, 3.0) } else { NOW };
            let z = if two_d { NOZ } else { z };
            if rational {
                [x * w, y * w, if two_d { z } else { z * w }, w]
            } else {
                [x, y, z, w]
            }
        })
        .collect();
    BSplineIn {
        dim: if two_d { 2 } else { 3 },
        oob: rng.chance(0.1),
        cpts,
        p,
        u,
    }
}

pub(super) fn gen_curve(rng: &mut Rng, scale: f64) -> CurveIn {
    let sc = |x: [f64; 3]| [x[0] * scale, x[1] * scale, x[2] * scale];
    match rng.below(10) {
        0..=2 => {
            let d0 = rng.uniform(-5.0, 5.0);
            let mut v = rp(rng, 3.0);
            if v.iter().all(|c| c.abs() < 1e-3) {
                v = [1.0, 0.0, 0.0];
            }
            CurveIn::Line(LineIn {
                p: sc(rp(rng, 10.0)),
                v: sc(v),
                domain: [d0, d0 + rng.uniform(0.5, 10.0)],
                scale: *rng.choose(&[1.0, 1.0, 2.0, 0.5]).unwrap_or(&1.0),
                inside_out: rng.chance(0.2),
            })
        }
        3..=6 => CurveIn::Bspline(gen_bspline(rng, 5.0 * scale)),
        7..=8 => {
            let d0 = rng.uniform(-180.0, 180.0);
            let mut y = rp(rng, 1.0);
            if y.iter().map(|c| c * c).sum::<f64>() < 1e-2 {
                y = [0.0, 1.0, 0.0];
            }
            CurveIn::Conic(ConicIn {
                center: sc(rp(rng, 5.0)),
                x: [1.0, 0.0, 0.0],
                y,
                domain: [d0, d0 + rng.uniform(10.0, 360.0)],
                rx: scale * rng.uniform(0.5, 10.0),
                ry: rng.chance(0.5).then(|| scale * rng.uniform(0.5, 10.0)),
                inside_out: rng.chance(0.2),
            })
        }
        _ => CurveIn::Hermite(HermiteIn {
            dim: 3,
            p1: sc(rp(rng, 5.0)),
            d1: sc(rp(rng, 5.0)),
            p2: sc(rp(rng, 5.0)),
            d2: sc(rp(rng, 5.0)),
        }),
    }
}

pub(super) fn gen_surface(rng: &mut Rng, scale: f64) -> SurfIn {
    let p = 1 + rng.below(3) as usize;
    let q = 1 + rng.below(3) as usize;
    let n = p + rng.below(2) as usize;
    let m = q + rng.below(2) as usize;
    let knots = |rng: &mut Rng, deg: usize, cnt: usize| {
        let a = if rng.chance(0.6) {
            0.0
        } else {
            rng.uniform(-2.0, 2.0)
        };
        let l = if rng.chance(0.6) {
            1.0
        } else {
            rng.uniform(0.5, 5.0)
        };
        let mut k = vec![a; deg + 1];
        let mut inner: Vec<f64> = (0..cnt - deg)
            .map(|_| a + l * rng.uniform(0.1, 0.9))
            .collect();
        inner.sort_by(f64::total_cmp);
        k.extend(inner);
        k.extend(vec![a + l; deg + 1]);
        k
    };
    let u = knots(rng, p, n);
    let v = knots(rng, q, m);
    let rational = rng.chance(0.2);
    let bump = rng.uniform(0.0, 2.0);
    let net = (0..=n)
        .map(|i| {
            (0..=m)
                .map(|j| {
                    let e = rp(rng, bump);
                    let pt = [
                        scale * (i as f64 * 3.0 + e[0]),
                        scale * (j as f64 * 3.0 + e[1]),
                        scale * e[2],
                    ];
                    if rational {
                        let w = rng.uniform(0.5, 2.0);
                        [pt[0] * w, pt[1] * w, pt[2] * w, w]
                    } else {
                        [pt[0], pt[1], pt[2], NOW]
                    }
                })
                .collect()
        })
        .collect();
    SurfIn {
        oob: rng.chance(0.2),
        net,
        p,
        q,
        u,
        v,
    }
}

pub(super) fn dir(rng: &mut Rng) -> [f64; 3] {
    let d = rp(rng, 1.0);
    let l = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
    if l < 0.1 {
        [0.0, 0.0, 1.0]
    } else {
        [d[0] / l, d[1] / l, d[2] / l]
    }
}

pub(super) fn gen_scale(rng: &mut Rng) -> f64 {
    match rng.below(10) {
        0 => 1e-4,
        1 => 1e4,
        2 => rng.uniform(0.01, 0.1),
        3 => rng.uniform(10.0, 100.0),
        _ => 1.0,
    }
}

pub(super) fn gen_tol(rng: &mut Rng, scale: f64) -> f64 {
    match rng.below(10) {
        0 => USE_DEFAULT,
        1 => 0.0,
        2 => -1e-3,
        3 => 1e-8 * scale,
        4 => rng.uniform(1e-6, 1e-1) * scale,
        5 => 1.0 * scale,
        _ => 1e-5 * scale,
    }
}

/// List scripts: lists built by inserts, then random list, assembly and map ops.
fn gen_list(rng: &mut Rng) -> Script {
    let n = 2 + rng.below(8) as i64;
    let slots: Vec<i64> = (0..n)
        .map(|_| match rng.below(10) {
            0..=4 => slot::OWNING,
            5..=6 => slot::ASSEMBLY,
            _ => slot::INSTANCE,
        })
        .collect();
    let of = |k: i64, eq: bool| -> Vec<i64> {
        (0..n)
            .filter(|&i| (slots.get(i as usize) == Some(&k)) == eq)
            .collect()
    };
    let owners = of(slot::INSTANCE, false);
    let asms = of(slot::ASSEMBLY, true);
    let insts = of(slot::INSTANCE, true);
    let all: Vec<i64> = (0..n).collect();
    let pick = |rng: &mut Rng, v: &[i64]| -> i64 {
        if rng.chance(0.05) {
            -1
        } else {
            rng.choose(v).copied().unwrap_or(-1)
        }
    };
    let mut ops = Vec::new();
    // instances into assemblies first
    for &i in &insts {
        if let Some(&a) = rng.choose(&asms) {
            let comp = if rng.chance(0.5) {
                pick(rng, &asms)
            } else {
                -1
            };
            ops.push([lop::INST_NEW, i, a, comp, i64::from(rng.chance(0.3))]);
        }
    }
    let n_ops = 4 + rng.below(30);
    for _ in 0..n_ops {
        let a = pick(rng, &owners);
        let x = pick(rng, &all);
        let c = pick(rng, &all);
        let op = match rng.below(40) {
            0..=7 => [lop::POST_INSERT, a, x, 0, 0],
            8..=10 => [lop::PRE_INSERT, a, x, 0, 0],
            11..=12 => [lop::INSERT_AFTER, a, x, c, 0],
            13..=15 => [lop::REMOVE, a, x, 0, 0],
            16..=17 => [lop::IS_IN_LIST, a, x, 0, 0],
            18..=20 => {
                let attr = if rng.chance(0.5) {
                    -1
                } else {
                    rng.below(4) as i64
                };
                let tgt = if rng.chance(0.6) { -1 } else { x };
                [lop::GET_ALL, a, attr, tgt, 0]
            }
            21 => [lop::UPDATE_SIZE, a, 0, 0, 0],
            22 => [lop::REVERSE, a, 0, 0, 0],
            23 => [lop::SET_LIST, a, x, 0, 0],
            24 => [lop::DESTROY, x, 0, 0, 0],
            25 => [lop::ASM_INSTANCES, pick(rng, &asms), 0, 0, 0],
            26 => [
                lop::ASM_REMOVE_INSTANCE,
                pick(rng, &asms),
                pick(rng, &insts),
                0,
                0,
            ],
            27 => [lop::INST_PARENT, pick(rng, &insts), 0, 0, 0],
            28..=29 => [lop::MAP_SET, x, c, 0, 0],
            30..=31 => [lop::MAP_GET, x, 0, 0, 0],
            32 => [lop::MAP_BREPS, x, c, 0, 0],
            33..=34 => [lop::ADD_ATTR, x, rng.below(4) as i64, 0, 0],
            35..=36 => [lop::TYPE, x, 0, 0, 0],
            _ => [lop::GET_SIZE, a, 0, 0, 0],
        };
        ops.push(op);
    }
    Script::List { slots, ops }
}

/// Generator state for proto scripts: the pool layout and the creation counts.
struct PGen {
    curves: Vec<i64>,
    surfs: Vec<i64>,
    vertices: Vec<i64>,
    edges: Vec<i64>,
    faces: Vec<i64>,
    others: Vec<i64>,
    /// Curves no op refers to, each given to one edge definition that is deleted.
    scratch: Vec<i64>,
    n_vd: i64,
    n_ed: i64,
    n_pv: i64,
    n_pe: i64,
    n_pf: i64,
    scale: f64,
    /// Parameter domains of the pool curves (for sensible parameters).
    domains: BTreeMap<i64, [f64; 2]>,
}

impl PGen {
    fn id(rng: &mut Rng, n: i64) -> f64 {
        if n == 0 || rng.chance(0.04) {
            if rng.chance(0.5) {
                -1.0
            } else {
                n as f64
            }
        } else {
            rng.below(n as u64) as f64
        }
    }
    fn of(rng: &mut Rng, v: &[i64]) -> f64 {
        rng.choose(v).map_or(-1.0, |&x| x as f64)
    }
    fn topo(&self, rng: &mut Rng) -> f64 {
        let mut all = self.vertices.clone();
        all.extend(&self.edges);
        all.extend(&self.faces);
        all.extend(&self.others);
        all.extend(&self.others);
        all.extend(&self.others);
        Self::of(rng, &all)
    }
    fn curve(&self, rng: &mut Rng) -> f64 {
        Self::of(rng, &self.curves)
    }
    fn param(&self, rng: &mut Rng, c: f64) -> f64 {
        let d = self.domains.get(&(c as i64)).copied().unwrap_or([0.0, 1.0]);
        let t = rng.uniform(-0.1, 1.1);
        d[0] + (d[1] - d[0]) * t
    }
    fn sub_domain(&self, rng: &mut Rng, c: f64) -> [f64; 2] {
        let d = self.domains.get(&(c as i64)).copied().unwrap_or([0.0, 1.0]);
        if rng.chance(0.4) {
            return d;
        }
        let a = rng.uniform(0.0, 0.6);
        let bb = rng.uniform(a, 1.0);
        [d[0] + (d[1] - d[0]) * a, d[0] + (d[1] - d[0]) * bb]
    }
    fn point(&self, rng: &mut Rng) -> [f64; 3] {
        let s = self.scale;
        let p = rp(rng, 10.0);
        [p[0] * s, p[1] * s, p[2] * s]
    }
    /// A point classification on a curve or surface (comparable), sometimes on topology.
    fn pc(&self, rng: &mut Rng, topo_ok: bool) -> Vec<f64> {
        let tol = gen_tol(rng, self.scale);
        let gap = if rng.chance(0.3) {
            0.0
        } else {
            rng.uniform(0.0, 0.1) * self.scale
        };
        let r = rng.below(20);
        if r < 9 && !self.curves.is_empty() {
            let c = self.curve(rng);
            let flags =
                if rng.chance(0.85) { 1.0 } else { 0.0 } + if rng.chance(0.5) { 4.0 } else { 0.0 };
            return vec![106.0, c, tol, flags, self.param(rng, c), 0.0, 0.0, gap];
        }
        if r < 13 && !self.surfs.is_empty() {
            let s = Self::of(rng, &self.surfs);
            let flags =
                if rng.chance(0.96) { 2.0 } else { 0.0 } + if rng.chance(0.5) { 4.0 } else { 0.0 };
            return vec![
                107.0,
                s,
                tol,
                flags,
                0.0,
                rng.uniform(-0.2, 1.2),
                rng.uniform(-0.2, 1.2),
                gap,
            ];
        }
        if r < 15 {
            return vec![
                *rng.choose(&[100.0, 110.0]).unwrap_or(&100.0),
                -1.0,
                tol,
                0.0,
                0.0,
                0.0,
                0.0,
                gap,
            ];
        }
        if topo_ok {
            let (cls, set) = match rng.below(4) {
                0 => (101.0, self.others.clone()),
                1 => (102.0, self.faces.clone()),
                2 => (103.0, self.edges.clone()),
                _ => (104.0, self.vertices.clone()),
            };
            let ob = Self::of(rng, &set);
            if ob >= 0.0 {
                return vec![
                    cls,
                    ob,
                    tol,
                    f64::from(rng.below(8) as u32),
                    rng.unit(),
                    rng.unit(),
                    rng.unit(),
                    gap,
                ];
            }
        }
        vec![100.0, -1.0, tol, 0.0, 0.0, 0.0, 0.0, gap]
    }
}

#[allow(clippy::too_many_lines)]
fn gen_proto(rng: &mut Rng, p3: bool) -> Script {
    let scale = gen_scale(rng);
    let mut objects = Vec::new();
    let mut g = PGen {
        curves: Vec::new(),
        surfs: Vec::new(),
        vertices: Vec::new(),
        edges: Vec::new(),
        faces: Vec::new(),
        others: Vec::new(),
        scratch: Vec::new(),
        n_vd: 0,
        n_ed: 0,
        n_pv: 0,
        n_pe: 0,
        n_pf: 0,
        scale,
        domains: BTreeMap::new(),
    };
    for _ in 0..2 + rng.below(4) {
        let c = gen_curve(rng, scale);
        if let Ok(cv) = build_curve(&c) {
            let d = cv.natural_interval();
            g.domains.insert(objects.len() as i64, [d.min(), d.max()]);
        }
        g.curves.push(objects.len() as i64);
        objects.push(ObjIn::Curve(c));
    }
    // a near-duplicate of the first curve (overlaps, same-curve checks)
    if rng.chance(0.6) {
        if let Some(ObjIn::Curve(c)) = objects.first().cloned() {
            g.domains.insert(
                objects.len() as i64,
                g.domains.get(&0).copied().unwrap_or([0.0, 1.0]),
            );
            g.curves.push(objects.len() as i64);
            objects.push(ObjIn::Curve(c));
        }
    }
    let plane = objects.len() as i64;
    objects.push(ObjIn::Plane {
        o: g.point(rng),
        n: dir(rng),
    });
    g.surfs.push(plane);
    if rng.chance(0.5) {
        g.surfs.push(objects.len() as i64);
        objects.push(ObjIn::Bsurf(gen_surface(rng, scale)));
    }
    for i in 0..2 {
        g.vertices.push(objects.len() as i64);
        objects.push(ObjIn::Vertex);
        if p3 && rng.chance(0.7) {
            // a vertex with a point (Glue compares the near / coincident paths), apart from the others
            let q = g.point(rng);
            if let Some(slot) = objects.last_mut() {
                *slot = ObjIn::VertexAt {
                    p: [
                        q[0] + 5.0 * (f64::from(i) + 1.0) * g.scale.max(1.0),
                        q[1],
                        q[2],
                    ],
                    tol: gen_tol(rng, g.scale),
                };
            }
        }
    }
    if p3 {
        // two more vertices with points, so a proto vertex can hold final vertices the glue compares
        // (one vertex, or two vertices far apart; near distinct vertices would glue and are refused)
        for _ in 0..2 {
            g.vertices.push(objects.len() as i64);
            let q = g.point(rng);
            objects.push(ObjIn::VertexAt {
                p: q,
                tol: gen_tol(rng, g.scale),
            });
        }
    }
    // edges on their own curves (never edited or deleted by the ops)
    for _ in 0..2 {
        let c = objects.len();
        objects.push(ObjIn::Curve(gen_curve(rng, scale)));
        g.edges.push(objects.len() as i64);
        objects.push(ObjIn::Edge(c));
    }
    g.faces.push(objects.len() as i64);
    objects.push(ObjIn::Face(plane as usize));
    for _ in 0..2 {
        g.others.push(objects.len() as i64);
        objects.push(if rng.chance(0.5) {
            ObjIn::Region
        } else {
            ObjIn::Loop
        });
    }
    for _ in 0..4 {
        let c = gen_curve(rng, scale);
        if let Ok(cv) = build_curve(&c) {
            let d = cv.natural_interval();
            g.domains.insert(objects.len() as i64, [d.min(), d.max()]);
        }
        g.scratch.push(objects.len() as i64);
        objects.push(ObjIn::Curve(c));
    }
    let approx_tol = match rng.below(10) {
        0 => 1e-3 * scale,
        1 => 0.1 * scale,
        _ => 1e-5 * scale.max(1.0),
    };
    let capture = match rng.below(4) {
        0 => 0.0,
        1 => 1e-3 * scale,
        _ => rng.uniform(1e-4, 1.0) * scale,
    };
    let override_tol = rng.uniform(-1.0, 1.0);

    let mut ops: Vec<Vec<f64>> = Vec::new();
    // build: proto vertices, proto edges, edge definitions, lists
    let n_pv = 2 + rng.below(4);
    for _ in 0..n_pv {
        if rng.chance(0.75) {
            let mut v = vec![p::PV_NEW as f64];
            v.extend(g.point(rng));
            v.push(gen_tol(rng, scale));
            ops.push(v);
            g.n_pv += 1;
        } else {
            let mut v = vec![p::PV_NEW_PCS as f64];
            v.extend(g.pc(rng, false));
            v.extend(g.pc(rng, false));
            v.extend(g.point(rng));
            v.push(gen_tol(rng, scale));
            ops.push(v);
            g.n_pv += 1;
            g.n_vd += 1;
        }
        if rng.chance(0.8) {
            ops.push(vec![p::MGR_ADD_PV as f64, (g.n_pv - 1) as f64]);
        }
    }
    let n_pe = 1 + rng.below(4);
    for _ in 0..n_pe {
        let s = PGen::id(rng, g.n_pv);
        let e = PGen::id(rng, g.n_pv);
        let c = g.curve(rng);
        let d = g.sub_domain(rng, c);
        ops.push(vec![p::PE_NEW as f64, s, e, d[0], d[1]]);
        g.n_pe += 1;
        let pe = (g.n_pe - 1) as f64;
        if rng.chance(0.8) {
            ops.push(vec![p::MGR_ADD_PE as f64, pe]);
        }
        if s >= 0.0 && s < g.n_pv as f64 {
            ops.push(vec![p::PV_ADD_PE as f64, s, pe]);
        }
        if e >= 0.0 && e < g.n_pv as f64 && e != s {
            ops.push(vec![p::PV_ADD_PE as f64, e, pe]);
        }
        for _ in 0..=rng.below(2) {
            let c3 = if rng.chance(0.9) { c } else { g.curve(rng) };
            let dd = if rng.chance(0.7) {
                d
            } else {
                g.sub_domain(rng, c3)
            };
            let s1 = g.topo(rng);
            let s2 = g.topo(rng);
            let uv1 = if rng.chance(0.2) { g.curve(rng) } else { -1.0 };
            let uv2 = if rng.chance(0.2) { g.curve(rng) } else { -1.0 };
            ops.push(vec![
                p::ED_NEW as f64,
                c3,
                dd[0],
                dd[1],
                s1,
                s2,
                gen_tol(rng, scale),
                uv1,
                uv2,
                pe,
            ]);
            g.n_ed += 1;
            ops.push(vec![p::PE_ADD_ED as f64, pe, (g.n_ed - 1) as f64]);
        }
    }
    // vertex definitions at the proto vertices
    for _ in 0..rng.below(3) {
        let mut v = vec![p::VD_NEW as f64];
        v.extend(g.pc(rng, false));
        v.extend(g.pc(rng, false));
        v.extend(g.point(rng));
        v.push(gen_tol(rng, scale));
        v.push(if rng.chance(0.5) {
            0.0
        } else {
            rng.uniform(0.0, 0.1) * scale
        });
        let owner = PGen::id(rng, g.n_pv);
        v.push(owner);
        ops.push(v);
        g.n_vd += 1;
        if owner >= 0.0 && owner < g.n_pv as f64 {
            ops.push(vec![p::PV_ADD_VD as f64, owner, (g.n_vd - 1) as f64]);
        }
    }
    // a coincident proto face
    if rng.chance(0.5) && g.n_pe > 0 {
        let f = PGen::of(rng, &g.faces);
        ops.push(vec![
            p::PF_CREATE as f64,
            f,
            -1.0,
            1.0,
            PGen::id(rng, g.n_pe),
        ]);
        g.n_pf += 1;
    }
    let n_ops = 6 + rng.below(30);
    for _ in 0..n_ops {
        let code = if p3 && rng.chance(0.5) {
            120 + rng.below(7) as i64
        } else {
            rng.below(91) as i64
        };
        // ops on kinds not made yet are mostly replaced (a refusal ends the script)
        let need = |c: i64| -> i64 {
            match c {
                p::PE_COPY
                | p::MGR_ADD_PE
                | p::PE_CLEAR_EDS
                | p::MGR_REMOVE_PE
                | p::PE_GROUP
                | p::PE_ADD_OVERLAP
                | p::PE_APPEND_OVERLAPS
                | p::PE_SET_START
                | p::PE_SET_END
                | p::PE_SET_IVL
                | p::PE_REPLACE_PV
                | p::PE_END_POS
                | p::PE_END_POS_PV
                | p::PE_EVAL
                | p::PE_EVAL_MID
                | p::PE_IS_SAME
                | p::WHICH_PV
                | p::MGR_REMOVE_NOEDGE_PV
                | p::PE_SPLIT
                | p::SPLIT_INT => g.n_pe,
                p::MGR_ADD_PF | p::PE_RELATE_PF | p::PF_SET_SRC => g.n_pf.min(g.n_pe.max(1)),
                p::PV_ADD_VD
                | p::PV_REMOVE_VD
                | p::VD_SET_SRC
                | p::VD_SET_FINAL
                | p::VD_SET_POS
                | p::VD_SET_PV
                | p::MGR_REMOVE_VD
                | p::VD_RESOLVE
                | p::VD_SET_POINT_PARAMS
                | p::VD_NUM_PES => g.n_vd,
                p::MGR_ADD_PV
                | p::PV_CLEAR_VDS
                | p::MGR_REMOVE_PV
                | p::PV_COMBINE
                | p::PV_FIND_MATCH
                | p::PV_CLOSEST
                | p::PV_FINAL_VTX
                | p::PV_ADD_PE
                | p::PV_REMOVE_PE => g.n_pv,
                p::ED_COPY
                | p::ED_SET_C3
                | p::ED_SET_UV
                | p::ED_SET_PE
                | p::ED_SET_IVL
                | p::ED_SET_IVL_MINMAX
                | p::ED_SET_SRC
                | p::ED_SET_FINAL
                | p::ED_REVERSE
                | p::ED_START_END_PV
                | p::ED_EVAL
                | p::ED_EVAL_MID
                | p::ED_END_POS
                | p::ED_END_POS_PV
                | p::ED_REPARAM
                | p::ED_IS_SAME
                | p::ED_IS_INTERIOR
                | p::MGR_CAN_COMBINE
                | p::ARE_OVERLAPPING
                | p::PE_ADD_ED
                | p::PE_REMOVE_ED
                | p::ED_SPLIT => g.n_ed,
                _ => 1,
            }
        };
        if need(code) == 0 && rng.chance(0.9) {
            continue;
        }
        // deletions of objects nothing else refers to (otherwise the shim refuses them)
        if matches!(
            code,
            p::ED_DESTRUCT | p::DELETE_ED | p::DELETE_PE | p::DELETE_PV | p::DELETE_VD
        ) && rng.chance(0.8)
        {
            match code {
                p::ED_DESTRUCT | p::DELETE_ED | p::DELETE_PE => {
                    let Some(c) = g.scratch.pop() else { continue };
                    let d = g.domains.get(&c).copied().unwrap_or([0.0, 1.0]);
                    let uv = if rng.chance(0.3) {
                        g.scratch.pop().map_or(-1.0, |x| x as f64)
                    } else {
                        -1.0
                    };
                    let pe = if code == p::DELETE_PE {
                        ops.push(vec![p::PE_NEW as f64, -1.0, -1.0, d[0], d[1]]);
                        g.n_pe += 1;
                        (g.n_pe - 1) as f64
                    } else {
                        -1.0
                    };
                    let s1 = if rng.chance(0.6) {
                        PGen::of(rng, &g.edges)
                    } else {
                        g.topo(rng)
                    };
                    let s2 = if rng.chance(0.4) {
                        PGen::of(rng, &g.edges)
                    } else {
                        g.topo(rng)
                    };
                    ops.push(vec![
                        p::ED_NEW as f64,
                        c as f64,
                        d[0],
                        d[1],
                        s1,
                        s2,
                        gen_tol(rng, scale),
                        uv,
                        -1.0,
                        pe,
                    ]);
                    g.n_ed += 1;
                    if code == p::DELETE_PE {
                        ops.push(vec![p::PE_ADD_ED as f64, pe, (g.n_ed - 1) as f64]);
                        ops.push(vec![p::DELETE_PE as f64, pe]);
                    } else {
                        ops.push(vec![code as f64, (g.n_ed - 1) as f64]);
                    }
                }
                p::DELETE_PV => {
                    let mut v = vec![p::PV_NEW_PCS as f64];
                    v.extend(g.pc(rng, false));
                    v.extend(g.pc(rng, false));
                    v.extend(g.point(rng));
                    v.push(gen_tol(rng, scale));
                    ops.push(v);
                    g.n_pv += 1;
                    g.n_vd += 1;
                    ops.push(vec![p::DELETE_PV as f64, (g.n_pv - 1) as f64]);
                }
                _ => {
                    let mut v = vec![p::VD_NEW as f64];
                    v.extend(g.pc(rng, false));
                    v.extend(g.pc(rng, false));
                    v.extend(g.point(rng));
                    v.extend([gen_tol(rng, scale), 0.0, -1.0]);
                    ops.push(v);
                    g.n_vd += 1;
                    ops.push(vec![p::DELETE_VD as f64, (g.n_vd - 1) as f64]);
                }
            }
            continue;
        }
        let mut v = vec![code as f64];
        let idv = |rng: &mut Rng, n: i64| PGen::id(rng, n);
        match code {
            p::PV_NEW => {
                v.extend(g.point(rng));
                v.push(gen_tol(rng, scale));
                g.n_pv += 1;
            }
            p::PV_NEW_PCS => {
                v.extend(g.pc(rng, true));
                v.extend(g.pc(rng, true));
                v.extend(g.point(rng));
                v.push(gen_tol(rng, scale));
                g.n_pv += 1;
                g.n_vd += 1;
            }
            p::VD_NEW => {
                v.extend(g.pc(rng, true));
                v.extend(g.pc(rng, true));
                v.extend(g.point(rng));
                v.push(gen_tol(rng, scale));
                v.push(if rng.chance(0.5) {
                    0.0
                } else {
                    rng.uniform(-0.1, 0.1) * scale
                });
                v.push(idv(rng, g.n_pv));
                g.n_vd += 1;
            }
            p::PE_NEW => {
                let c = g.curve(rng);
                let d = g.sub_domain(rng, c);
                v.extend([idv(rng, g.n_pv), idv(rng, g.n_pv), d[0], d[1]]);
                g.n_pe += 1;
            }
            p::PE_COPY => {
                v.push(idv(rng, g.n_pe));
                g.n_pe += 1;
            }
            p::ED_NEW => {
                let c = if rng.chance(0.9) { g.curve(rng) } else { -1.0 };
                let d = g.sub_domain(rng, c);
                v.extend([c, d[0], d[1], g.topo(rng), g.topo(rng), gen_tol(rng, scale)]);
                v.push(if rng.chance(0.2) { g.curve(rng) } else { -1.0 });
                v.push(if rng.chance(0.2) { g.curve(rng) } else { -1.0 });
                v.push(if rng.chance(0.8) {
                    idv(rng, g.n_pe)
                } else {
                    -1.0
                });
                g.n_ed += 1;
            }
            p::ED_COPY => {
                v.push(idv(rng, g.n_ed));
                g.n_ed += 1;
            }
            p::PF_NEW | p::PF_CREATE => {
                let f = |rng: &mut Rng| {
                    if rng.chance(0.7) {
                        PGen::of(rng, &g.faces)
                    } else {
                        -1.0
                    }
                };
                v.extend([f(rng), f(rng)]);
                let n = rng.below(3);
                v.push(n as f64);
                for _ in 0..n {
                    v.push(idv(rng, g.n_pe));
                }
                g.n_pf += 1;
            }
            p::MGR_ADD_PV | p::PV_CLEAR_VDS | p::MGR_REMOVE_PV | p::DELETE_PV => {
                v.push(idv(rng, g.n_pv));
            }
            p::MGR_ADD_PE | p::PE_CLEAR_EDS | p::MGR_REMOVE_PE | p::DELETE_PE | p::PE_GROUP => {
                v.push(idv(rng, g.n_pe));
            }
            p::MGR_ADD_PF => v.push(idv(rng, g.n_pf)),
            p::MGR_RESET
            | p::MGR_ALL_VDS
            | p::MGR_ALL_EDS
            | p::MGR_TYPE
            | p::MGR_GET
            | p::GLUE
            | p::RESOLVE => {}
            p::MGR_DESTROY => {
                if rng.chance(0.7) {
                    continue;
                }
            }
            p::PV_ADD_PE | p::PV_REMOVE_PE => v.extend([idv(rng, g.n_pv), idv(rng, g.n_pe)]),
            p::PV_ADD_VD | p::PV_REMOVE_VD => v.extend([idv(rng, g.n_pv), idv(rng, g.n_vd)]),
            p::PE_ADD_ED | p::PE_REMOVE_ED => v.extend([idv(rng, g.n_pe), idv(rng, g.n_ed)]),
            p::PE_ADD_OVERLAP | p::SPLIT_INT => v.extend([idv(rng, g.n_pe), idv(rng, g.n_pe)]),
            p::PE_APPEND_OVERLAPS => {
                v.push(idv(rng, g.n_pe));
                let n = rng.below(3);
                v.push(n as f64);
                for _ in 0..n {
                    v.push(idv(rng, g.n_pe));
                }
            }
            p::PE_SET_START | p::PE_SET_END | p::PE_REMOVE_PV | p::MGR_REMOVE_NOEDGE_PV => {
                v.extend([idv(rng, g.n_pe), idv(rng, g.n_pv)]);
            }
            p::PE_SET_IVL => {
                let c = g.curve(rng);
                let d = g.sub_domain(rng, c);
                v.extend([idv(rng, g.n_pe), d[0], d[1]]);
            }
            p::PE_REPLACE_PV => v.extend([idv(rng, g.n_pe), idv(rng, g.n_pv), idv(rng, g.n_pv)]),
            p::PE_RELATE_PF => v.extend([idv(rng, g.n_pe), idv(rng, g.n_pf)]),
            p::PF_SET_SRC => v.extend([
                idv(rng, g.n_pf),
                f64::from(1 + rng.below(2) as u32),
                g.topo(rng),
            ]),
            p::VD_SET_SRC => {
                let (cls, set) = match rng.below(5) {
                    0 => (101.0, g.others.clone()),
                    1 => (102.0, g.faces.clone()),
                    2 => (103.0, g.edges.clone()),
                    3 => (104.0, g.vertices.clone()),
                    _ => (100.0, Vec::new()),
                };
                let ob = if set.is_empty() {
                    -1.0
                } else {
                    PGen::of(rng, &set)
                };
                v.extend([idv(rng, g.n_vd), f64::from(rng.below(4) as u32), cls, ob]);
            }
            p::VD_SET_FINAL => {
                let ob = if rng.chance(0.2) {
                    -1.0
                } else {
                    PGen::of(rng, &g.vertices)
                };
                v.extend([idv(rng, g.n_vd), f64::from(1 + rng.below(2) as u32), ob]);
            }
            p::VD_SET_POS => {
                v.push(idv(rng, g.n_vd));
                v.extend(g.point(rng));
            }
            p::VD_SET_PV => v.extend([idv(rng, g.n_vd), idv(rng, g.n_pv)]),
            p::ED_SET_C3 => {
                let c = if rng.chance(0.9) { g.curve(rng) } else { -1.0 };
                v.extend([idv(rng, g.n_ed), c]);
            }
            p::ED_SET_UV => {
                let c = if rng.chance(0.6) { g.curve(rng) } else { -1.0 };
                v.extend([idv(rng, g.n_ed), f64::from(1 + rng.below(2) as u32), c]);
            }
            p::ED_SET_PE => v.extend([idv(rng, g.n_ed), idv(rng, g.n_pe)]),
            p::ED_SET_IVL | p::ED_SET_IVL_MINMAX => {
                let c = g.curve(rng);
                let mut d = g.sub_domain(rng, c);
                if code == p::ED_SET_IVL_MINMAX && rng.chance(0.3) {
                    d.swap(0, 1);
                }
                v.extend([idv(rng, g.n_ed), d[0], d[1]]);
            }
            p::ED_SET_SRC => v.extend([
                idv(rng, g.n_ed),
                f64::from(1 + rng.below(2) as u32),
                g.topo(rng),
            ]),
            p::ED_SET_FINAL => {
                let ob = if rng.chance(0.3) {
                    -1.0
                } else {
                    PGen::of(rng, &g.edges)
                };
                v.extend([idv(rng, g.n_ed), f64::from(1 + rng.below(2) as u32), ob]);
            }
            p::MGR_REMOVE_VD => v.extend([idv(rng, g.n_vd), b(rng.chance(0.5))]),
            p::MGR_SET_TOL => v.push(gen_tol(rng, scale)),
            p::MGR_SET_CAPTURE => v.push(rng.uniform(0.0, 1.0) * scale),
            p::PV_COMBINE => v.extend([idv(rng, g.n_pv), idv(rng, g.n_pv)]),
            p::ED_DESTRUCT | p::DELETE_ED | p::ED_REVERSE | p::ED_START_END_PV => {
                v.push(idv(rng, g.n_ed));
            }
            p::DELETE_VD | p::VD_RESOLVE | p::VD_SET_POINT_PARAMS | p::VD_NUM_PES => {
                v.push(idv(rng, g.n_vd));
            }
            p::ED_EVAL => {
                let c = g.curve(rng);
                v.extend([idv(rng, g.n_ed), g.param(rng, c), rng.below(4) as f64]);
            }
            p::ED_EVAL_MID => v.extend([idv(rng, g.n_ed), rng.below(4) as f64]),
            p::ED_END_POS => v.extend([idv(rng, g.n_ed), b(rng.chance(0.5))]),
            p::ED_END_POS_PV => v.extend([idv(rng, g.n_ed), idv(rng, g.n_pv)]),
            p::ED_REPARAM => {
                let a = rng.uniform(-3.0, 3.0);
                let len = if rng.chance(0.03) {
                    0.0
                } else {
                    rng.uniform(0.1, 10.0)
                };
                v.extend([idv(rng, g.n_ed), a, a + len, b(rng.chance(0.3))]);
            }
            p::ED_IS_SAME | p::PE_IS_SAME => {
                v.push(if code == p::ED_IS_SAME {
                    idv(rng, g.n_ed)
                } else {
                    idv(rng, g.n_pe)
                });
                v.extend([idv(rng, g.n_pv), idv(rng, g.n_pv)]);
                v.extend(g.pc(rng, true));
                v.extend(g.pc(rng, true));
                let c = g.curve(rng);
                let d = g.sub_domain(rng, c);
                v.extend([c, d[0], d[1]]);
            }
            p::ED_IS_INTERIOR => {
                v.push(idv(rng, g.n_ed));
                v.extend(g.point(rng));
                v.extend([gen_tol(rng, scale), b(rng.chance(0.5))]);
            }
            p::PE_END_POS => v.extend([idv(rng, g.n_pe), b(rng.chance(0.5))]),
            p::PE_END_POS_PV => v.extend([idv(rng, g.n_pe), idv(rng, g.n_pv)]),
            p::PE_EVAL => {
                let c = g.curve(rng);
                v.extend([idv(rng, g.n_pe), g.param(rng, c), rng.below(4) as f64]);
            }
            p::PE_EVAL_MID => v.extend([idv(rng, g.n_pe), rng.below(4) as f64]),
            p::PV_FIND_MATCH => {
                let n = objects.len() as u64;
                let o1 = if rng.chance(0.2) {
                    -1.0
                } else {
                    rng.below(n) as f64
                };
                let o2 = if rng.chance(0.2) {
                    -1.0
                } else {
                    rng.below(n) as f64
                };
                v.extend([idv(rng, g.n_pv), o1, o2]);
            }
            p::PV_CLOSEST => {
                v.push(idv(rng, g.n_pv));
                v.extend(g.point(rng));
            }
            p::PV_FINAL_VTX => v.extend([
                idv(rng, g.n_pv),
                f64::from(1 + rng.below(2) as u32),
                idv(rng, g.n_ed),
                b(rng.chance(0.5)),
            ]),
            p::MGR_FIND_PV => v.extend(g.point(rng)),
            p::MGR_FIND_ED_FROM_EDGE => {
                let e = if rng.chance(0.2) {
                    -1.0
                } else {
                    PGen::of(rng, &g.edges)
                };
                v.extend([e, f64::from(1 + rng.below(2) as u32)]);
            }
            p::MGR_CHECK_OVERLAP | p::ARE_OVERLAPPING => {
                let c = g.curve(rng);
                let d = g.sub_domain(rng, c);
                v.extend([c, gen_tol(rng, scale), d[0], d[1]]);
                // midpoints on edges reach the overlap test
                let mid = |rng: &mut Rng, g: &PGen| -> Vec<f64> {
                    if rng.chance(0.6) {
                        let e = PGen::of(rng, &g.edges);
                        vec![103.0, e, gen_tol(rng, g.scale), 0.0, 0.0, 0.0, 0.0, 0.0]
                    } else {
                        g.pc(rng, true)
                    }
                };
                v.extend(mid(rng, &g));
                if code == p::MGR_CHECK_OVERLAP {
                    v.extend(mid(rng, &g));
                } else {
                    v.push(idv(rng, g.n_ed));
                }
            }
            p::MGR_CAN_COMBINE => v.extend([idv(rng, g.n_ed), idv(rng, g.n_ed)]),
            p::WHICH_PV => {
                let c = g.curve(rng);
                let sv = if rng.chance(0.7) {
                    idv(rng, g.n_pv)
                } else {
                    -1.0
                };
                v.extend([idv(rng, g.n_pe), g.param(rng, c), sv]);
            }
            p::FIND_POS | p::FOC_PV => {
                v.extend(g.pc(rng, true));
                v.extend(g.pc(rng, true));
            }
            p::SAME_CURVE => {
                let c1 = g.curve(rng);
                let c2 = if rng.chance(0.3) { c1 } else { g.curve(rng) };
                let d1 = g.sub_domain(rng, c1);
                let d2 = if rng.chance(0.5) {
                    d1
                } else {
                    g.sub_domain(rng, c2)
                };
                v.extend([
                    c1,
                    d1[0],
                    d1[1],
                    c2,
                    d2[0],
                    d2[1],
                    rng.uniform(1e-6, 1.0) * scale,
                ]);
            }
            p::IS_INTERIOR => {
                let c = g.curve(rng);
                let d = g.sub_domain(rng, c);
                v.extend([c, d[0], d[1], g.param(rng, c), gen_tol(rng, scale)]);
            }
            p::ARE_CONNECTED => {
                let a = if rng.chance(0.1) { -1.0 } else { g.topo(rng) };
                let bb = if rng.chance(0.3) { a } else { g.topo(rng) };
                v.extend([a, bb]);
            }
            p::TOPO_DIM => v.push(g.topo(rng)),
            p::CAN_IMPRINT => v.push(f64::from(100 + rng.below(11) as u32)),
            p::FOC_PE => {
                let c = g.curve(rng);
                let d = g.sub_domain(rng, c);
                v.extend([c, gen_tol(rng, scale), d[0], d[1]]);
                for _ in 0..6 {
                    if rng.chance(0.3) {
                        let e = PGen::of(rng, &g.edges);
                        v.extend([103.0, e, gen_tol(rng, scale), 0.0, 0.0, 0.0, 0.0, 0.0]);
                    } else {
                        v.extend(g.pc(rng, true));
                    }
                }
            }
            p::PE_SPLIT => {
                let c = g.curve(rng);
                v.extend([idv(rng, g.n_pe), g.param(rng, c)]);
            }
            p::ED_SPLIT => {
                let c = g.curve(rng);
                v.extend([
                    idv(rng, g.n_ed),
                    g.param(rng, c),
                    idv(rng, g.n_pv),
                    idv(rng, g.n_pe),
                ]);
            }
            _ => continue,
        }
        ops.push(v);
    }
    Script::Proto {
        objects,
        approx_tol,
        capture,
        override_tol,
        ops,
    }
}

fn gen_transform(rng: &mut Rng) -> Script {
    let scale = gen_scale(rng);
    let owned = rng.chance(0.15);
    let obj = match rng.below(20) {
        0..=8 => TObjIn::Curve {
            owned,
            c: gen_curve(rng, scale),
        },
        9..=11 => TObjIn::Plane {
            owned,
            o: rp(rng, 5.0 * scale),
            n: dir(rng),
        },
        12..=15 => TObjIn::Bsurf {
            owned,
            s: gen_surface(rng, scale),
        },
        16 => TObjIn::Brep,
        17 => {
            if rng.chance(0.2) {
                TObjIn::Poly
            } else {
                let n = 1 + rng.below(4);
                let pts = (0..n)
                    .map(|_| {
                        let q = rp(rng, 5.0 * scale);
                        [q[0], q[1], q[2], 1e-5 * scale * rng.uniform(0.5, 2.0)]
                    })
                    .collect();
                TObjIn::PolyPts { pts }
            }
        }
        18 => match rng.below(3) {
            0 => TObjIn::Other,
            1 => TObjIn::Composite {
                owned,
                cs: (0..=rng.below(3)).map(|_| gen_curve(rng, scale)).collect(),
            },
            _ => TObjIn::CrvOnSurf {
                owned,
                s: gen_surface(rng, scale),
                a: [rng.unit(), rng.unit()],
                b: [rng.unit(), rng.unit()],
            },
        },
        _ => TObjIn::Null,
    };
    let sv = |rng: &mut Rng| -> [f64; 3] {
        match rng.below(6) {
            0 => {
                let s = rng.uniform(0.2, 5.0);
                [s, s, s]
            }
            1 => [
                rng.uniform(-3.0, 3.0),
                rng.uniform(-3.0, 3.0),
                rng.uniform(-3.0, 3.0),
            ],
            2 => [1e-12, 1.0, 1.0],
            3 => [2.0, 2.0, 2.0 * (1.0 + 1e-13)],
            4 => [-1.0, -1.0, -1.0],
            _ => [
                rng.uniform(0.1, 4.0),
                rng.uniform(0.1, 4.0),
                rng.uniform(0.1, 4.0),
            ],
        }
    };
    let op = match rng.below(5) {
        0 => {
            let x = dir(rng);
            let y = if rng.chance(0.05) { x } else { dir(rng) };
            TOpIn::Transform {
                o: rp(rng, 5.0 * scale),
                x,
                y,
            }
        }
        1 => TOpIn::Scale { s: sv(rng) },
        2 => TOpIn::ScaleByPt {
            r: rp(rng, 5.0 * scale),
            s: sv(rng),
        },
        3 => TOpIn::Translate {
            v: rp(rng, 10.0 * scale),
        },
        _ => TOpIn::Rotate {
            r: rp(rng, 5.0 * scale),
            a: if rng.chance(0.05) {
                [0.0; 3]
            } else {
                rp(rng, 2.0)
            },
            deg: rng.uniform(-720.0, 720.0),
        },
    };
    Script::Transform { obj, op }
}

fn gen_script(rng: &mut Rng) -> Script {
    match rng.below(20) {
        0..=4 => gen_list(rng),
        5..=16 => gen_proto(rng, false),
        _ => gen_transform(rng),
    }
}

/// Whether the shim accepts the script (it rejects what is malformed).
pub(super) fn shim_accepts(s: &Script) -> bool {
    match ffi::run(&cpp_input(s)) {
        Ok(_) => true,
        Err(solidcore_ffi::FfiError::Status(c)) if c == SC_ERR_ARG => false,
        Err(_) => false,
    }
}

// ---- fixtures built in code -----------------------------------------------------------------

pub(super) fn line(p0: [f64; 3], v: [f64; 3], d: [f64; 2]) -> CurveIn {
    CurveIn::Line(LineIn {
        p: p0,
        v,
        domain: d,
        scale: 1.0,
        inside_out: false,
    })
}

pub(super) fn pcv(
    class: f64,
    ob: f64,
    tol: f64,
    flags: f64,
    t: f64,
    u: f64,
    v: f64,
    gap: f64,
) -> Vec<f64> {
    vec![class, ob, tol, flags, t, u, v, gap]
}

pub(super) fn opv(code: i64, rest: &[f64]) -> Vec<f64> {
    let mut v = vec![code as f64];
    v.extend_from_slice(rest);
    v
}

pub(super) fn cat(parts: &[&[f64]]) -> Vec<f64> {
    parts.iter().flat_map(|x| x.iter().copied()).collect()
}

/// Edge cases: list walks, assembly destruction, the proto object life cycle with every
/// query, huge and tiny scales, degenerate scales and axes of the transforms.
#[allow(clippy::too_many_lines)]
fn edge_cases() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    // lists: insert, reverse, remove the head, the last one, a member not owned
    let ops = vec![
        [lop::POST_INSERT, 0, 1, 0, 0],
        [lop::POST_INSERT, 0, 2, 0, 0],
        [lop::PRE_INSERT, 0, 3, 0, 0],
        [lop::GET_SIZE, 0, 0, 0, 0],
        [lop::GET_ALL, 0, -1, -1, 0],
        [lop::IS_IN_LIST, 0, 2, 0, 0],
        [lop::IS_IN_LIST, 0, 4, 0, 0],
        [lop::REVERSE, 0, 0, 0, 0],
        [lop::GET_ALL, 0, -1, -1, 0],
        [lop::ADD_ATTR, 2, 7, 0, 0],
        [lop::GET_ALL, 0, 7, -1, 0],
        [lop::GET_ALL, 0, -1, 2, 0],
        [lop::GET_ALL, 0, 7, 3, 0],
        [lop::REMOVE, 0, 4, 0, 0],
        [lop::REMOVE, 0, 3, 0, 0],
        [lop::INSERT_AFTER, 0, 3, 1, 0],
        [lop::INSERT_AFTER, 0, -1, 1, 0],
        [lop::UPDATE_SIZE, 0, 0, 0, 0],
        [lop::REMOVE, 0, 1, 0, 0],
        [lop::REMOVE, 0, 2, 0, 0],
        [lop::REMOVE, 0, 3, 0, 0],
        [lop::GET_SIZE, 0, 0, 0, 0],
        [lop::SET_LIST, 4, 4, 0, 0],
        [lop::UPDATE_SIZE, 4, 0, 0, 0],
        [lop::TYPE, 0, 0, 0, 0],
        [lop::MAP_SET, 1, 2, 0, 0],
        [lop::MAP_SET, 1, 3, 0, 0],
        [lop::MAP_GET, 1, 0, 0, 0],
        [lop::MAP_GET, -1, 0, 0, 0],
        [lop::MAP_BREPS, 0, 4, 0, 0],
        [lop::MAP_GET, 2, 0, 0, 0],
    ];
    out.push((
        "edge_lists".to_string(),
        json!(Script::List {
            slots: vec![0, 0, 0, 0, 0],
            ops
        }),
    ));
    // assemblies: instances, parent, remove, destructors in both orders
    let ops = vec![
        [lop::INST_NEW, 2, 0, 1, 1],
        [lop::INST_NEW, 3, 0, -1, 0],
        [lop::INST_NEW, 4, 1, 0, 0],
        [lop::ASM_INSTANCES, 0, 0, 0, 0],
        [lop::INST_PARENT, 3, 0, 0, 0],
        [lop::TYPE, 0, 0, 0, 0],
        [lop::TYPE, 2, 0, 0, 0],
        [lop::ASM_REMOVE_INSTANCE, 0, 4, 0, 0],
        [lop::ASM_REMOVE_INSTANCE, 0, 2, 0, 0],
        [lop::INST_PARENT, 2, 0, 0, 0],
        [lop::DESTROY, 3, 0, 0, 0],
        [lop::ASM_INSTANCES, 0, 0, 0, 0],
        [lop::DESTROY, 4, 0, 0, 0],
        [lop::DESTROY, 1, 0, 0, 0],
        [lop::POST_INSERT, 0, 2, 0, 0],
        [lop::DESTROY, 0, 0, 0, 0],
        [lop::GET_SIZE, 2, 0, 0, 0],
    ];
    out.push((
        "edge_assemblies".to_string(),
        json!(Script::List {
            slots: vec![1, 1, 2, 2, 2],
            ops
        }),
    ));
    // proto: the life cycle on lines and a circle at three scales
    for (name, s) in [("unit", 1.0), ("huge", 1e5), ("tiny", 1e-5)] {
        let objects = vec![
            ObjIn::Curve(line([0.0, 0.0, 0.0], [s, 0.0, 0.0], [0.0, 1.0])),
            ObjIn::Curve(line([0.0, 1e-7 * s, 0.0], [s, 0.0, 0.0], [0.0, 1.0])),
            ObjIn::Curve(CurveIn::Conic(ConicIn {
                center: [0.0, 0.0, 0.0],
                x: [1.0, 0.0, 0.0],
                y: [0.0, 1.0, 0.0],
                domain: [0.0, 90.0],
                rx: s,
                ry: None,
                inside_out: false,
            })),
            ObjIn::Plane {
                o: [0.0, 0.0, 0.0],
                n: [0.0, 0.0, 1.0],
            },
            ObjIn::Vertex,
            ObjIn::Curve(line([0.0, 0.0, 0.0], [0.0, s, 0.0], [0.0, 1.0])),
            ObjIn::Edge(5),
            ObjIn::Face(3),
            ObjIn::Region,
            ObjIn::Loop,
        ];
        let t = 1e-5 * s;
        let pc_c0 = pcv(106.0, 0.0, t, 1.0, 0.25, 0.0, 0.0, 0.0);
        let pc_s = pcv(107.0, 3.0, t, 2.0, 0.0, 0.25 * s, 0.0, 0.0);
        let pc_c2 = pcv(106.0, 2.0, t, 5.0, 45.0, 0.0, 0.0, 1e-6 * s);
        let pc_u = pcv(100.0, -1.0, t, 0.0, 0.0, 0.0, 0.0, 0.0);
        let pc_e = pcv(103.0, 6.0, t, 0.0, 0.0, 0.0, 0.0, 0.0);
        let ops = vec![
            opv(p::MGR_GET, &[]),
            opv(p::MGR_TYPE, &[]),
            opv(p::PV_NEW, &[0.0, 0.0, 0.0, t]),
            opv(p::PV_NEW, &[s, 0.0, 0.0, 2.0 * t]),
            opv(
                p::PV_NEW_PCS,
                &cat(&[&pc_c0, &pc_s, &[0.25 * s, 0.0, 0.0, t]]),
            ),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::MGR_ADD_PV, &[1.0]),
            opv(p::MGR_ADD_PV, &[2.0]),
            opv(
                p::VD_NEW,
                &cat(&[&pc_c2, &pc_s, &[0.0, s, 0.0, t, 0.0, 2.0]]),
            ),
            opv(p::PV_ADD_VD, &[2.0, 1.0]),
            opv(p::VD_RESOLVE, &[0.0]),
            opv(p::VD_RESOLVE, &[1.0]),
            opv(p::VD_NUM_PES, &[0.0]),
            opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]),
            opv(p::MGR_ADD_PE, &[0.0]),
            opv(p::PV_ADD_PE, &[0.0, 0.0]),
            opv(p::PV_ADD_PE, &[1.0, 0.0]),
            opv(p::ED_NEW, &[0.0, 0.0, 1.0, 8.0, 9.0, t, -1.0, -1.0, 0.0]),
            opv(p::ED_NEW, &[1.0, 0.0, 1.0, 8.0, 7.0, t, -1.0, -1.0, 0.0]),
            opv(p::PE_ADD_ED, &[0.0, 0.0]),
            opv(p::PE_ADD_ED, &[0.0, 1.0]),
            opv(p::MGR_ALL_EDS, &[]),
            opv(p::MGR_ALL_VDS, &[]),
            opv(p::ED_EVAL, &[0.0, 0.5, 3.0]),
            opv(p::ED_EVAL_MID, &[1.0, 1.0]),
            opv(p::ED_EVAL, &[0.0, 0.5, 0.0]),
            opv(p::ED_END_POS, &[0.0, 1.0]),
            opv(p::ED_END_POS_PV, &[0.0, 1.0]),
            opv(p::ED_END_POS_PV, &[0.0, 2.0]),
            opv(p::ED_START_END_PV, &[1.0]),
            opv(p::PE_END_POS, &[0.0, 0.0]),
            opv(p::PE_END_POS_PV, &[0.0, 0.0]),
            opv(p::PE_EVAL, &[0.0, 0.25, 3.0]),
            opv(p::PE_EVAL_MID, &[0.0, 2.0]),
            opv(p::ED_IS_INTERIOR, &[0.0, 0.5 * s, 0.0, 0.0, t, 1.0]),
            opv(p::ED_IS_INTERIOR, &[0.0, 0.1 * s, 0.0, 0.0, t, 0.0]),
            opv(p::ED_IS_INTERIOR, &[0.0, 0.0, 0.0, 0.0, t, 1.0]),
            opv(p::SAME_CURVE, &[0.0, 0.0, 1.0, 1.0, 0.0, 1.0, t]),
            opv(p::SAME_CURVE, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, t]),
            opv(p::SAME_CURVE, &[0.0, 0.0, 1.0, 2.0, 0.0, 90.0, t]),
            opv(p::IS_INTERIOR, &[2.0, 0.0, 90.0, 45.0, t]),
            opv(p::IS_INTERIOR, &[2.0, 0.0, 90.0, 0.0, t]),
            opv(p::IS_INTERIOR, &[2.0, 0.0, 90.0, 120.0, t]),
            opv(p::MGR_SET_CAPTURE, &[1e-3 * s]),
            opv(
                p::ED_IS_SAME,
                &cat(&[&[0.0, 0.0, 1.0], &pc_u, &pc_u, &[1.0, 0.0, 1.0]]),
            ),
            opv(
                p::PE_IS_SAME,
                &cat(&[&[0.0, 1.0, 0.0], &pc_u, &pc_u, &[0.0, 0.0, 1.0]]),
            ),
            opv(p::MGR_CAN_COMBINE, &[0.0, 1.0]),
            opv(p::MGR_CAN_COMBINE, &[0.0, 0.0]),
            opv(
                p::ARE_OVERLAPPING,
                &cat(&[&[1.0, t, 0.25, 0.75], &pc_e, &[0.0]]),
            ),
            opv(
                p::MGR_CHECK_OVERLAP,
                &cat(&[&[1.0, t, 0.25, 0.75], &pc_e, &pc_u]),
            ),
            opv(p::FIND_POS, &cat(&[&pc_c0, &pc_s])),
            opv(p::FIND_POS, &cat(&[&pc_c0, &pc_c2])),
            opv(p::FIND_POS, &cat(&[&pc_u, &pc_u])),
            opv(p::TOPO_DIM, &[4.0]),
            opv(p::TOPO_DIM, &[8.0]),
            opv(p::CAN_IMPRINT, &[101.0]),
            opv(p::CAN_IMPRINT, &[106.0]),
            opv(p::ARE_CONNECTED, &[8.0, 9.0]),
            opv(p::ARE_CONNECTED, &[8.0, 8.0]),
            opv(p::MGR_FIND_ED_FROM_EDGE, &[6.0, 1.0]),
            opv(p::ED_SET_FINAL, &[1.0, 2.0, 6.0]),
            opv(p::MGR_FIND_ED_FROM_EDGE, &[6.0, 2.0]),
            opv(p::PV_FIND_MATCH, &[2.0, 0.0, 3.0]),
            opv(p::PV_FIND_MATCH, &[2.0, 4.0, 3.0]),
            opv(p::PV_CLOSEST, &[2.0, 0.0, s, 0.0]),
            opv(p::PE_ADD_OVERLAP, &[0.0, 0.0]),
            opv(p::PE_GROUP, &[0.0]),
            opv(p::PF_CREATE, &[7.0, -1.0, 1.0, 0.0]),
            opv(p::PE_RELATE_PF, &[0.0, 0.0]),
            opv(p::PE_RELATE_PF, &[0.0, 0.0]),
            opv(p::WHICH_PV, &[0.0, 0.5, 1.0]),
            opv(p::MGR_REMOVE_NOEDGE_PV, &[0.0, 1.0]),
            opv(p::ED_REPARAM, &[0.0, 0.0, 2.0, 1.0]),
            opv(p::ED_SET_IVL_MINMAX, &[1.0, 1.0, 0.0]),
            opv(p::ED_REVERSE, &[1.0]),
            opv(p::ED_COPY, &[0.0]),
            opv(p::PE_COPY, &[0.0]),
            opv(p::PV_COMBINE, &[0.0, 2.0]),
            opv(p::MGR_REMOVE_VD, &[1.0, 1.0]),
            opv(p::MGR_REMOVE_PE, &[0.0]),
            opv(p::MGR_RESET, &[]),
            opv(p::ED_DESTRUCT, &[2.0]),
            opv(p::DELETE_ED, &[2.0]),
        ];
        out.push((
            format!("edge_proto_{name}"),
            json!(Script::Proto {
                objects,
                approx_tol: 1e-5 * s.max(1.0),
                capture: 1e-4 * s,
                override_tol: -1.0,
                ops
            }),
        ));
    }
    // the manager's destructor with listed objects, then the leftovers
    let objects = vec![
        ObjIn::Curve(line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0])),
        ObjIn::Curve(line([0.0, 0.0, 0.0], [1.0, 1.0, 0.0], [0.0, 1.0])),
        ObjIn::Vertex,
        ObjIn::Curve(line([0.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0])),
        ObjIn::Edge(3),
        ObjIn::Region,
    ];
    let ops = vec![
        opv(p::PV_NEW, &[0.0, 0.0, 0.0, 1e-5]),
        opv(p::PV_NEW, &[1.0, 0.0, 0.0, 1e-5]),
        opv(p::PV_NEW, &[5.0, 0.0, 0.0, 1e-5]),
        opv(p::MGR_ADD_PV, &[0.0]),
        opv(p::MGR_ADD_PV, &[1.0]),
        opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]),
        opv(p::PE_NEW, &[0.0, 2.0, 0.0, 1.0]),
        opv(p::MGR_ADD_PE, &[0.0]),
        opv(p::PV_ADD_PE, &[0.0, 0.0]),
        opv(p::PV_ADD_PE, &[1.0, 0.0]),
        opv(p::ED_NEW, &[0.0, 0.0, 1.0, 4.0, 5.0, 1e-5, -1.0, -1.0, 0.0]),
        opv(p::ED_NEW, &[1.0, 0.0, 1.0, 5.0, 4.0, 1e-5, -1.0, -1.0, 1.0]),
        opv(p::PE_ADD_ED, &[0.0, 0.0]),
        opv(p::PE_ADD_ED, &[1.0, 1.0]),
        opv(p::PF_NEW, &[-1.0, -1.0, 1.0, 1.0]),
        opv(p::MGR_REMOVE_NOEDGE_PV, &[1.0, 2.0]),
        opv(p::MGR_DESTROY, &[]),
    ];
    out.push((
        "edge_proto_destroy".to_string(),
        json!(Script::Proto {
            objects,
            approx_tol: 1e-5,
            capture: 1e-3,
            override_tol: 0.0,
            ops
        }),
    ));
    // an approximation tolerance below `SM_EFF_ZERO` (zero, negative, tiny) falls back
    // to the two empty B-reps' tolerances and edge sizes; `SM_EFF_ZERO` itself does not
    for (name, approx_tol) in [
        ("zero", 0.0),
        ("negative", -1e-3),
        ("minus_one", -1.0),
        ("tiny", 1e-13),
        ("eff_zero", 1e-12),
    ] {
        let objects = vec![ObjIn::Curve(line(
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [0.0, 1.0],
        ))];
        let ops = vec![
            opv(p::PV_NEW, &[0.0, 0.0, 0.0, 1e-5]),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::MGR_FIND_PV, &[3e-4, 0.0, 0.0]),
        ];
        out.push((
            format!("edge_proto_fallback_tol_{name}"),
            json!(Script::Proto {
                objects,
                approx_tol,
                capture: 1e-3,
                override_tol: 0.5,
                ops
            }),
        ));
    }
    // transforms: every API call on a line, a B-spline, a circle and a plane
    let bs = CurveIn::Bspline(BSplineIn {
        dim: 3,
        oob: false,
        cpts: vec![
            [0.0, 0.0, 0.0, NOW],
            [1.0, 2.0, 0.0, NOW],
            [3.0, -1.0, 1.0, NOW],
            [4.0, 0.0, 0.0, NOW],
        ],
        p: 3,
        u: vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0],
    });
    let circle = CurveIn::Conic(ConicIn {
        center: [1.0, 2.0, 3.0],
        x: [1.0, 0.0, 0.0],
        y: [0.0, 1.0, 0.0],
        domain: [0.0, 270.0],
        rx: 2.0,
        ry: None,
        inside_out: false,
    });
    let calls = [
        (
            "transform",
            TOpIn::Transform {
                o: [1.0, 2.0, 3.0],
                x: [0.0, 1.0, 0.0],
                y: [-1.0, 0.0, 0.0],
            },
        ),
        ("scale_uniform", TOpIn::Scale { s: [2.0, 2.0, 2.0] }),
        ("scale_nonuniform", TOpIn::Scale { s: [2.0, 3.0, 0.5] }),
        ("scale_zero", TOpIn::Scale { s: [0.0, 1.0, 1.0] }),
        (
            "scale_negative",
            TOpIn::Scale {
                s: [-1.0, -1.0, -1.0],
            },
        ),
        (
            "scale_by_pt",
            TOpIn::ScaleByPt {
                r: [1.0, 1.0, 1.0],
                s: [3.0, 3.0, 3.0],
            },
        ),
        (
            "translate",
            TOpIn::Translate {
                v: [1e6, -1e-6, 0.0],
            },
        ),
        (
            "rotate",
            TOpIn::Rotate {
                r: [1.0, 0.0, 0.0],
                a: [0.0, 0.0, 5.0],
                deg: 90.0,
            },
        ),
        (
            "rotate_zero_axis",
            TOpIn::Rotate {
                r: [1.0, 0.0, 0.0],
                a: [0.0, 0.0, 0.0],
                deg: 90.0,
            },
        ),
    ];
    for (oname, obj) in [
        (
            "line",
            TObjIn::Curve {
                owned: false,
                c: line([1.0, 1.0, 1.0], [1.0, 2.0, 3.0], [0.0, 2.0]),
            },
        ),
        (
            "bspline",
            TObjIn::Curve {
                owned: false,
                c: bs,
            },
        ),
        (
            "circle",
            TObjIn::Curve {
                owned: false,
                c: circle,
            },
        ),
        (
            "owned_line",
            TObjIn::Curve {
                owned: true,
                c: line([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0]),
            },
        ),
        (
            "plane",
            TObjIn::Plane {
                owned: false,
                o: [1.0, 2.0, 3.0],
                n: [0.0, 0.0, 1.0],
            },
        ),
        ("other", TObjIn::Other),
        ("null", TObjIn::Null),
        ("poly_empty", TObjIn::Poly),
        (
            "poly",
            TObjIn::PolyPts {
                pts: vec![
                    [0.0, 0.0, 0.0, 1e-5],
                    [1.0, 2.0, 3.0, 2e-5],
                    [-1.0, 0.5, 0.25, 1e-6],
                ],
            },
        ),
        (
            "poly_huge",
            TObjIn::PolyPts {
                pts: vec![[1e7, -3e6, 2e5, 1e-1], [-2e6, 4e7, 1e6, 3e-2]],
            },
        ),
        (
            "poly_tiny",
            TObjIn::PolyPts {
                pts: vec![[1e-7, -3e-6, 2e-8, 1e-12], [0.0, 0.0, 0.0, 0.0]],
            },
        ),
        (
            "composite",
            TObjIn::Composite {
                owned: false,
                cs: vec![
                    line([0.0; 3], [1.0, 0.0, 0.0], [0.0, 1.0]),
                    line([1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0]),
                ],
            },
        ),
        (
            "crv_on_surf",
            TObjIn::CrvOnSurf {
                owned: false,
                s: SurfIn {
                    oob: false,
                    net: vec![
                        vec![[0.0, 0.0, 0.0, NOW], [0.0, 1.0, 0.0, NOW]],
                        vec![[1.0, 0.0, 0.0, NOW], [1.0, 1.0, 0.5, NOW]],
                    ],
                    p: 1,
                    q: 1,
                    u: vec![0.0, 0.0, 1.0, 1.0],
                    v: vec![0.0, 0.0, 1.0, 1.0],
                },
                a: [0.1, 0.2],
                b: [0.9, 0.7],
            },
        ),
    ] {
        for (cname, call) in &calls {
            out.push((
                format!("edge_transform_{oname}_{cname}"),
                json!(Script::Transform {
                    obj: obj.clone(),
                    op: call.clone()
                }),
            ));
        }
    }
    out
}

/// Edge cases of unit `topology.proto_topology_p3`.
#[allow(clippy::too_many_lines)]
pub fn edge_cases_p3() -> Vec<(String, Value)> {
    let mut out = Vec::new();
    // topology.proto_topology_p3: groups of overlapping proto edges, intersecting proto
    // edges, vertices to glue and the imprintable classifications
    {
        let objects = vec![
            ObjIn::Curve(line([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0])),
            ObjIn::Curve(line([0.5, -0.5, 0.0], [0.0, 1.0, 0.0], [0.0, 1.0])),
            ObjIn::Curve(line([5.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0])),
            ObjIn::Plane {
                o: [0.0, 0.0, 0.0],
                n: [0.0, 0.0, 1.0],
            },
            ObjIn::Face(3),
            ObjIn::Face(3),
            ObjIn::Vertex,
            ObjIn::Vertex,
            ObjIn::Region,
        ];
        let t = 1e-5;
        let pc_u = pcv(100.0, -1.0, t, 0.0, 0.0, 0.0, 0.0, 0.0);
        let pc_s = pcv(107.0, 3.0, t, 2.0, 0.0, 0.25, 0.25, 0.0);
        let ops = vec![
            opv(p::FOC_PV, &cat(&[&pc_u, &pc_s])),
            opv(p::FOC_PV, &cat(&[&pc_s, &pc_u])),
            opv(p::PV_NEW, &[0.0, 0.0, 0.0, t]),
            opv(p::PV_NEW, &[1.0, 0.0, 0.0, t]),
            opv(p::PV_NEW, &[0.5, -0.5, 0.0, t]),
            opv(p::PV_NEW, &[0.5, 0.5, 0.0, t]),
            opv(p::PV_NEW, &[5.0, 0.0, 0.0, t]),
            opv(p::PV_NEW, &[6.0, 0.0, 0.0, t]),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(p::MGR_ADD_PV, &[1.0]),
            opv(p::MGR_ADD_PV, &[2.0]),
            opv(p::MGR_ADD_PV, &[3.0]),
            opv(p::MGR_ADD_PV, &[4.0]),
            opv(p::MGR_ADD_PV, &[5.0]),
            opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]),
            opv(p::PE_NEW, &[2.0, 3.0, 0.0, 1.0]),
            opv(p::PE_NEW, &[4.0, 5.0, 0.0, 1.0]),
            opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]),
            opv(p::MGR_ADD_PE, &[0.0]),
            opv(p::MGR_ADD_PE, &[1.0]),
            opv(p::MGR_ADD_PE, &[2.0]),
            opv(p::MGR_ADD_PE, &[3.0]),
            opv(p::PV_ADD_PE, &[0.0, 0.0]),
            opv(p::PV_ADD_PE, &[1.0, 0.0]),
            opv(p::PV_ADD_PE, &[2.0, 1.0]),
            opv(p::PV_ADD_PE, &[3.0, 1.0]),
            opv(p::ED_NEW, &[0.0, 0.0, 1.0, 4.0, 4.0, t, -1.0, -1.0, 0.0]),
            opv(p::ED_NEW, &[1.0, 0.0, 1.0, 4.0, 4.0, t, -1.0, -1.0, 1.0]),
            opv(p::ED_NEW, &[2.0, 0.0, 1.0, 4.0, 4.0, t, -1.0, -1.0, 2.0]),
            opv(p::PE_ADD_ED, &[0.0, 0.0]),
            opv(p::PE_ADD_ED, &[1.0, 1.0]),
            opv(p::PE_ADD_ED, &[2.0, 2.0]),
            // not interior, near an end, parallel, no definitions: nothing is split
            opv(p::PE_SPLIT, &[0.0, 0.0]),
            opv(p::PE_SPLIT, &[0.0, 2.0]),
            opv(p::SPLIT_INT, &[0.0, 2.0]),
            opv(p::SPLIT_INT, &[0.0, 3.0]),
            opv(p::ED_SPLIT, &[0.0, 7.0, 0.0, 1.0]),
            opv(
                p::FOC_PE,
                &cat(&[
                    &[0.0, t, 0.0, 1.0],
                    &pc_u,
                    &pc_u,
                    &pc_u,
                    &pc_u,
                    &pc_u,
                    &pc_u,
                ]),
            ),
            opv(
                p::FOC_PE,
                &cat(&[
                    &[0.0, t, 0.0, 1.0],
                    &pc_s,
                    &pc_u,
                    &pc_u,
                    &pc_u,
                    &pc_u,
                    &pc_u,
                ]),
            ),
            opv(p::GLUE, &[]),
            opv(p::RESOLVE, &[]),
            // a group of two proto edges without definitions is removed
            opv(p::PE_NEW, &[4.0, 5.0, 0.0, 1.0]),
            opv(p::PE_NEW, &[4.0, 5.0, 0.0, 1.0]),
            opv(p::MGR_ADD_PE, &[4.0]),
            opv(p::MGR_ADD_PE, &[5.0]),
            opv(p::PV_ADD_PE, &[4.0, 4.0]),
            opv(p::PV_ADD_PE, &[5.0, 4.0]),
            opv(p::PV_ADD_PE, &[4.0, 5.0]),
            opv(p::PV_ADD_PE, &[5.0, 5.0]),
            opv(p::PE_ADD_OVERLAP, &[4.0, 5.0]),
            opv(p::PE_ADD_OVERLAP, &[5.0, 4.0]),
            opv(p::RESOLVE, &[]),
            opv(p::MGR_ALL_EDS, &[]),
        ];
        // crossing at (0.5, 0): interior to both (the splits need the unported proto vertex
        // creation, so these cases are deferred)
        for (name, tail) in [
            ("intersect", opv(p::SPLIT_INT, &[0.0, 1.0])),
            ("edge_definition", opv(p::ED_SPLIT, &[0.0, 0.5, 0.0, 1.0])),
            ("proto_edge", opv(p::PE_SPLIT, &[0.0, 0.5])),
        ] {
            let mut ops = ops.clone();
            ops.push(tail);
            out.push((
                format!("edge_proto_p3_split_{name}"),
                json!(Script::Proto {
                    objects: objects.clone(),
                    approx_tol: 1e-5,
                    capture: 1e-4,
                    override_tol: -1.0,
                    ops
                }),
            ));
        }
        // rejected inputs (the C++ crashes on them: a one-way overlap leaves an edge outside its
        // own group, a repeated overlap processes a deleted edge): both sides must skip RESOLVE
        // (and a proto edge listed twice in the manager: `AddProtoEdge` does not de-duplicate and
        // `RemoveProtoEdge` removes the first copy only, UB-132)
        for (name, overlaps, dup) in [
            ("one_way_overlap", vec![(0.0, 1.0)], None),
            (
                "repeated_overlap",
                vec![(0.0, 1.0), (0.0, 1.0), (1.0, 0.0)],
                None,
            ),
            (
                "duplicate_manager_entry_0",
                vec![(0.0, 1.0), (1.0, 0.0)],
                Some(0.0),
            ),
            (
                "duplicate_manager_entry_1",
                vec![(0.0, 1.0), (1.0, 0.0)],
                Some(1.0),
            ),
        ] {
            let mut ops = vec![
                opv(p::PV_NEW, &[0.0, 0.0, 0.0, t]),
                opv(p::PV_NEW, &[1.0, 0.0, 0.0, t]),
                opv(p::MGR_ADD_PV, &[0.0]),
                opv(p::MGR_ADD_PV, &[1.0]),
                opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]),
                opv(p::PE_NEW, &[0.0, 1.0, 0.0, 1.0]),
                opv(p::MGR_ADD_PE, &[0.0]),
                opv(p::MGR_ADD_PE, &[1.0]),
                opv(p::PV_ADD_PE, &[0.0, 0.0]),
                opv(p::PV_ADD_PE, &[1.0, 0.0]),
                opv(p::PV_ADD_PE, &[0.0, 1.0]),
                opv(p::PV_ADD_PE, &[1.0, 1.0]),
            ];
            if let Some(d) = dup {
                ops.push(opv(p::MGR_ADD_PE, &[d]));
            }
            for (x, y) in overlaps {
                ops.push(opv(p::PE_ADD_OVERLAP, &[x, y]));
            }
            ops.push(opv(p::RESOLVE, &[]));
            ops.push(opv(p::MGR_ALL_EDS, &[]));
            out.push((
                format!("edge_proto_p3_rejected_{name}"),
                json!(Script::Proto {
                    objects: objects.clone(),
                    approx_tol: 1e-5,
                    capture: 1e-4,
                    override_tol: -1.0,
                    ops
                }),
            ));
        }
        out.push((
            "edge_proto_p3".to_string(),
            json!(Script::Proto {
                objects,
                approx_tol: 1e-5,
                capture: 1e-4,
                override_tol: -1.0,
                ops
            }),
        ));
    }
    // GlueCoincidentBrepVertices on final vertices with points: the same vertex, vertices far
    // apart (nothing is glued; vertices within their combined zone tolerance would glue and are
    // refused by the guard)
    for (name, finals) in [
        ("same_vertex", [0.0, 0.0, 0.0, 0.0]),
        ("far_vertices", [0.0, 0.0, 1.0, 1.0]),
        ("one_side_far", [0.0, 0.0, 0.0, 1.0]),
        ("near_vertices", [1.0, 1.0, 2.0, 2.0]),
        // 5e-4 apart with tolerance 1e-5: farther than the combined tolerance, nothing is glued
        ("apart_beyond_tol", [1.0, 1.0, 3.0, 3.0]),
    ] {
        let t = 1e-5;
        let objects = vec![
            ObjIn::VertexAt {
                p: [0.0, 0.0, 0.0],
                tol: t,
            },
            ObjIn::VertexAt {
                p: [5.0, 0.0, 0.0],
                tol: t,
            },
            ObjIn::VertexAt {
                p: [5.0, 0.0, 1e-6],
                tol: t,
            },
            ObjIn::VertexAt {
                p: [5.0005, 0.0, 0.0],
                tol: t,
            },
        ];
        let pc_u = pcv(100.0, -1.0, t, 0.0, 0.0, 0.0, 0.0, 0.0);
        let mut ops = vec![
            opv(p::PV_NEW, &[0.0, 0.0, 0.0, t]),
            opv(p::MGR_ADD_PV, &[0.0]),
            opv(
                p::VD_NEW,
                &cat(&[&pc_u, &pc_u, &[0.0, 0.0, 0.0, t, 0.0, 0.0]]),
            ),
            opv(
                p::VD_NEW,
                &cat(&[&pc_u, &pc_u, &[0.0, 0.0, 0.0, t, 0.0, 0.0]]),
            ),
            opv(p::PV_ADD_VD, &[0.0, 0.0]),
            opv(p::PV_ADD_VD, &[0.0, 1.0]),
            opv(p::VD_SET_FINAL, &[0.0, 1.0, finals[0]]),
            opv(p::VD_SET_FINAL, &[0.0, 2.0, finals[1]]),
            opv(p::VD_SET_FINAL, &[1.0, 1.0, finals[2]]),
            opv(p::VD_SET_FINAL, &[1.0, 2.0, finals[3]]),
        ];
        ops.push(opv(p::GLUE, &[]));
        out.push((
            format!("edge_proto_p3_glue_{name}"),
            json!(Script::Proto {
                objects,
                approx_tol: 1e-5,
                capture: 1e-4,
                override_tol: -1.0,
                ops
            }),
        ));
    }
    out
}

/// A generated script for unit `topology.proto_topology_p3`: a proto script rich in its ops.
pub fn generate_p3(rng: &mut Rng) -> Option<Value> {
    for _ in 0..64 {
        let s = gen_proto(rng, true);
        if ffi::validate(&cpp_input(&s)).is_err() || !shim_accepts(&s) {
            continue;
        }
        return serde_json::to_value(s).ok();
    }
    None
}

/// Runs a case on the C++ side.
pub fn run_cpp_case(case: &Case) -> Result<Output, HarnessError> {
    cpp(&case.parse()?)
}

/// Runs a case on the Rust side.
pub fn run_rust_case(case: &Case) -> Result<Output, HarnessError> {
    rust(&case.parse()?)
}

impl UnitDiff for Runner {
    fn id(&self) -> &'static str {
        "topology.proto_topology_p1"
    }

    fn generate(&self, rng: &mut Rng) -> Option<Value> {
        for _ in 0..64 {
            let s = gen_script(rng);
            if ffi::validate(&cpp_input(&s)).is_err() || !shim_accepts(&s) {
                continue;
            }
            return serde_json::to_value(s).ok();
        }
        serde_json::to_value(Script::Transform {
            obj: TObjIn::Null,
            op: TOpIn::Translate { v: [1.0, 0.0, 0.0] },
        })
        .ok()
    }

    fn cases(&self, ctx: &CaseContext) -> Result<Vec<Case>, HarnessError> {
        let ctx = CaseContext {
            seeds: ctx.seeds.max(MIN_GENERATED),
            ..ctx.clone()
        };
        let mut cases = default_cases(self, &ctx)?;
        for (name, inputs) in edge_cases() {
            cases.push(Case {
                unit: self.id().into(),
                name,
                source: CaseSource::Inline,
                inputs,
            });
        }
        Ok(cases)
    }

    fn run_cpp(&self, case: &Case) -> Result<Output, HarnessError> {
        cpp(&case.parse()?)
    }

    fn run_rust(&self, case: &Case) -> Result<Output, HarnessError> {
        rust(&case.parse()?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Generated scripts are accepted by the ffi checks, and most of them are comparable
    /// (not deferred) and get past their first few ops.
    #[test]
    fn generated_scripts_are_mostly_comparable() {
        let mut rng = Rng::new(17);
        let (mut total, mut deferred) = (0, 0);
        for _ in 0..300 {
            let Some(v) = Runner.generate(&mut rng) else {
                continue;
            };
            let s: Script = serde_json::from_value(v).unwrap();
            total += 1;
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
        }
        assert!(total > 250, "{total}");
        assert!(deferred * 3 < total, "{deferred} of {total} deferred");
    }
}

#[cfg(test)]
mod dump {
    use super::*;

    /// Prints generated case `PT_CASE` as JSON with both outputs (a debugging aid:
    /// `PT_CASE=6 cargo test -p difftest --features cpp --lib proto_dump_case -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid; needs PT_CASE"]
    fn proto_dump_case() {
        let i: usize = std::env::var("PT_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p1", i));
        let v = Runner.generate(&mut rng).unwrap();
        println!("{}", serde_json::to_string(&v).unwrap());
        let s: Script = serde_json::from_value(v).unwrap();
        println!("cpp  {:?}", ffi::run(&cpp_input(&s)));
        match rust_run(&s) {
            Ok(o) => println!("rust {o:?}"),
            Err(Fail::Deferred(_)) => println!("rust deferred"),
            Err(Fail::Harness(e)) => println!("rust harness {e}"),
        }
    }

    /// Runs every generated case on both sides, printing each index first (a debugging
    /// aid for hangs: `cargo test -p difftest --features cpp --lib proto_progress --
    /// --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid"]
    fn proto_progress() {
        for i in 0..MIN_GENERATED {
            eprint!("{i} gen ");
            let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p1", i));
            let v = Runner.generate(&mut rng).unwrap();
            let s: Script = serde_json::from_value(v).unwrap();
            eprint!("cpp ");
            let _ = ffi::run(&cpp_input(&s));
            eprint!("rust ");
            let _ = rust_run(&s);
            eprintln!("ok");
        }
    }

    /// Prints each candidate script of generated case `PT_CASE` before the shim runs it
    /// (a debugging aid for shim hangs).
    #[test]
    #[ignore = "debugging aid; needs PT_CASE"]
    fn proto_candidates() {
        let i: usize = std::env::var("PT_CASE").unwrap().parse().unwrap();
        let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p1", i));
        for _ in 0..64 {
            let s = gen_script(&mut rng);
            if ffi::validate(&cpp_input(&s)).is_err() {
                continue;
            }
            eprintln!("{}", serde_json::to_string(&s).unwrap());
            if shim_accepts(&s) {
                break;
            }
        }
    }

    /// Runs the script in file `PT_FILE` (JSON), truncated to `PT_NOPS` proto ops, on both
    /// sides and prints the records (a debugging aid).
    #[test]
    #[ignore = "debugging aid; needs PT_FILE"]
    fn proto_run_file() {
        let f = std::env::var("PT_FILE").unwrap();
        let mut s: Script = serde_json::from_str(&std::fs::read_to_string(f).unwrap()).unwrap();
        if let (Script::Proto { ops, .. }, Ok(n)) = (&mut s, std::env::var("PT_NOPS")) {
            ops.truncate(n.parse().unwrap());
        }
        if std::env::var("PT_RUST_ONLY").is_err() {
            eprintln!("cpp  {:?}", ffi::run(&cpp_input(&s)));
        }
        match rust_run(&s) {
            Ok(o) => eprintln!("rust {o:?}"),
            Err(Fail::Deferred(d)) => eprintln!("rust deferred {d:?}"),
            Err(Fail::Harness(e)) => eprintln!("rust harness {e}"),
        }
    }

    /// Prints what fraction of generated proto ops are refused and which op codes end
    /// scripts (`cargo test -p difftest --features cpp --lib proto_coverage -- --ignored --nocapture`).
    #[test]
    #[ignore = "debugging aid"]
    fn proto_coverage() {
        let mut ran: BTreeMap<i64, usize> = BTreeMap::new();
        let mut skipped: BTreeMap<i64, usize> = BTreeMap::new();
        let mut deferred = 0;
        for i in 0..MIN_GENERATED {
            let mut rng = Rng::new(CaseContext::seed_for("topology.proto_topology_p1", i));
            let s: Script = serde_json::from_value(Runner.generate(&mut rng).unwrap()).unwrap();
            let Script::Proto { ops, .. } = &s else {
                continue;
            };
            let Ok(out) = ffi::run(&cpp_input(&s)) else {
                continue;
            };
            if matches!(rust_run(&s), Err(Fail::Deferred(_))) {
                deferred += 1;
            }
            // walk the record: op status n outputs
            let mut at = 0;
            for o in ops {
                let code = o[0] as i64;
                if at + 1 >= out.len() {
                    break;
                }
                let stt = out[at + 1];
                if stt == SKIP {
                    *skipped.entry(code).or_default() += 1;
                    break;
                }
                *ran.entry(code).or_default() += 1;
                let n = out[at + 2] as usize;
                at += 3 + n;
            }
        }
        println!("deferred proto scripts: {deferred}");
        for c in 0..=p::LAST {
            println!(
                "{c:3} ran {:5} skipped {:5}",
                ran.get(&c).unwrap_or(&0),
                skipped.get(&c).unwrap_or(&0)
            );
        }
    }
}
