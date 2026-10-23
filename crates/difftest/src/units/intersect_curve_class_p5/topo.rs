//! B-rep cases of the part-5 runner: `sm_InsertSolutions` against a real edge and
//! `IntersectVertices` against real vertices (shim
//! `harness/cpp/shim/intersect_curve_class_p5_topo.cpp`, layouts in
//! `solidcore_ffi::units::intersect_curve_class_p5`).
//!
//! The C++ side builds a B-rep of unit `topology.edgeuse_p1`'s block (here a box, a face,
//! points and lines, or a wire network), applies the case's edits (stored tolerances,
//! vertex points, UV trim curves) and, with edge mates, builds it a second time and
//! relates the edges through an `SmTopologyIntersector`. The Rust side imports both
//! exports (unit `topology.edgeuse_p1`'s importer) and runs
//! [`insert_solutions_mut`] / [`CurveClassification::intersect_vertices_mut`] on them,
//! with the classification curve and the B-reps owned by the adapter, so the edits
//! upstream makes in place (an edge-end vertex's tolerance, the classification curve
//! through a surf/surf/surf point, the deleted UV trim curves of the vertex's edges and
//! their mates) are made and compared.
//!
//! Geometry is resolved on the imported B-rep and passed to both sides as numbers: the
//! line of an edge case crosses the edge at a fraction of its interval (plus an
//! offset), and the curve of a vertex case is given in a frame at a vertex (along the
//! intersection of the two classification planes through it). Both sides compare the
//! status, the whole classification, the solutions, the classification curve and the
//! state of every vertex tolerance and edge-use UV trim curve.
//!
//! Large-gap vertex/face gaps are computed on the imported B-rep before the call (C++
//! computes them lazily during it); generated large-gap cases have no classification
//! surfaces, so no UV trim curve changes in between.

use serde::{Deserialize, Serialize};

use solidcore::curve::curve::p2::{CurveKind, BSPLINE_CURVE_TYPE, LINE_TYPE};
use solidcore::curve::curve::Curve;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::solution_array::{Solution, SolutionType};
use solidcore::geom::tol::zone_tol3d;
use solidcore::geom::vector3d::Vec3;
use solidcore::intersect::curve_class::p2::ResetOptions;
use solidcore::intersect::curve_class::p5::{insert_solutions_mut, ClassGeometryMut};
use solidcore::intersect::curve_class::CurveClassification;
use solidcore::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassification,
};
use solidcore::kernel::Kernel;
use solidcore::math::Point3;
use solidcore::nurbs::math::{NOW, NOZ};
use solidcore::surface::surface::Surface;
use solidcore::surface::volume::Volume;
use solidcore::topology::{Brep, EdgeId, FaceId, VertexId};
use solidcore::{EntityRef, SolidError};
use solidcore_ffi::units::intersect_curve_class_p5::{self as ffi, TopoOp};

use super::super::topology_edgeuse_p1::{
    brep_block_of, bspline_of, import_with_types, path_of, push_curve, push_surface, sel,
    surface_of, BrepIn, BssIn, CrvIn, SrfIn,
};
use crate::{HarnessError, Output, Rng};

fn harness(msg: impl Into<String>) -> HarnessError {
    HarnessError::new(msg)
}

fn hx(e: impl std::fmt::Display) -> HarnessError {
    harness(e.to_string())
}

fn zone() -> f64 {
    1e-6
}
fn none_tol() -> f64 {
    -1.0
}
fn yes() -> bool {
    true
}

// ---- inputs -----------------------------------------------------------------------------

/// A B-rep edit (see the FFI docs): `kind` 0 face, 1 edge, 2 vertex tolerance; 3 the
/// face's UV trim curves; 4, 5, 6 a vertex point's x, y, z moved by `value`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct Edit {
    kind: u8,
    sel: i64,
    #[serde(default)]
    value: f64,
}

/// A B-rep case.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub(super) struct TopoIn {
    brep: BrepIn,
    #[serde(default)]
    edits: Vec<Edit>,
    /// Edge selector pairs related by the topology intersector (first B-rep, second);
    /// a pair reusing an edge already related is skipped.
    #[serde(default)]
    mates: Vec<[i64; 2]>,
    #[serde(flatten)]
    op: TopoOpIn,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "topo_op", rename_all = "snake_case")]
enum TopoOpIn {
    EdgeInsert(EdgeInsertIn),
    Vertices(VerticesIn),
}

/// `sm_InsertSolutions` of a line crossing an edge.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct EdgeInsertIn {
    edge: i64,
    /// The crossing point: this fraction of the edge interval.
    at: f64,
    /// The line's direction and half length.
    dir: [f64; 3],
    half: f64,
    /// The line's midpoint is the crossing point plus this.
    #[serde(default)]
    off: [f64; 3],
    #[serde(default = "zone")]
    tol: f64,
    #[serde(default)]
    ok_set: bool,
    #[serde(default)]
    ignore: bool,
    #[serde(default = "none_tol")]
    opt_tol: f64,
    /// Normals of the classification planes through the crossing point.
    #[serde(default)]
    srf: Option<[[f64; 3]; 2]>,
    /// `[type, s0, e0, s1, e1, g0, g1]`: the line parameters, fractions of the edge
    /// interval and solution values of both ends.
    sols: Vec<[f64; 7]>,
}

/// The classification surfaces of a vertex case: planes (or bilinear B-spline patches)
/// through the anchor vertex with these normals.
#[derive(Debug, Clone, Serialize, Deserialize)]
struct SrfPair {
    n1: [f64; 3],
    n2: [f64; 3],
    #[serde(default)]
    bspline: [bool; 2],
}

/// `IntersectVertices`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[allow(clippy::struct_excessive_bools)] // independent native flags, varied separately
struct VerticesIn {
    face: i64,
    /// Vertex selectors (modulo the vertex count); empty: every vertex of the face.
    #[serde(default)]
    verts: Vec<i64>,
    /// The vertex the curve's frame is at: a selector into the case's vertices.
    anchor: i64,
    #[serde(default)]
    big: bool,
    #[serde(default = "yes")]
    uv_ok: bool,
    #[serde(default = "zone")]
    tol: f64,
    #[serde(default)]
    cc_face: bool,
    /// Degree, control points `[a, b, c]` (the point anchor + a d + b e + c f of the
    /// frame `d = n1 x n2`, `e = n1`, `f = d x e`, unit; the axes without surfaces) and
    /// the knot vector.
    p: usize,
    cpts: Vec<[f64; 3]>,
    knots: Vec<f64>,
    #[serde(default)]
    srf: Option<SrfPair>,
    #[serde(default)]
    uv1: bool,
    #[serde(default)]
    uv2: bool,
}

// ---- the model ----------------------------------------------------------------------------

/// The handles of a case.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum T {
    Curve,
    Uv1,
    Uv2,
    S1,
    S2,
    Ti,
    Topo(u8, EntityRef),
}

impl ClassObject for T {
    fn class_kind(&self) -> ClassObjectKind {
        match self {
            Self::Curve | Self::Uv1 | Self::Uv2 => ClassObjectKind::Curve,
            Self::S1 | Self::S2 => ClassObjectKind::Surface,
            Self::Ti => ClassObjectKind::Other,
            Self::Topo(_, e) => match e {
                EntityRef::Vertex(_) => ClassObjectKind::Vertex,
                EntityRef::Edge(_) => ClassObjectKind::Edge,
                EntityRef::Face(_) => ClassObjectKind::Face,
                EntityRef::Loop(_) => ClassObjectKind::Loop,
                EntityRef::Edgeuse(_) => ClassObjectKind::Edgeuse,
                EntityRef::Vertexuse(_) => ClassObjectKind::Vertexuse,
                EntityRef::Faceuse(_) => ClassObjectKind::Faceuse,
                EntityRef::Loopuse(_) => ClassObjectKind::Loopuse,
                EntityRef::Region(_) => ClassObjectKind::Region,
                _ => ClassObjectKind::Other,
            },
        }
    }
}

/// The geometry and B-reps behind the handles; owned, so the port can edit them.
struct World {
    breps: Vec<Brep>,
    curve: Curve,
    uv: [Option<Curve>; 2],
    srf: [Option<Surface>; 2],
    /// Related edges (first B-rep, second).
    mates: Vec<(EdgeId, EdgeId)>,
    /// Vertex/face gaps of the case's vertices (first B-rep).
    gaps: Vec<(VertexId, f64)>,
}

impl World {
    fn brep(&self, b: u8) -> Option<&Brep> {
        self.breps.get(usize::from(b))
    }
}

impl ClassGeometry<T> for World {
    fn curve(&self, o: T) -> Option<&Curve> {
        match o {
            T::Curve => Some(&self.curve),
            T::Uv1 => self.uv[0].as_ref(),
            T::Uv2 => self.uv[1].as_ref(),
            _ => None,
        }
    }
    fn surface(&self, o: T) -> Option<&Surface> {
        match o {
            T::S1 => self.srf[0].as_ref(),
            T::S2 => self.srf[1].as_ref(),
            _ => None,
        }
    }
    fn volume(&self, _o: T) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: T) -> Option<Point3> {
        None
    }
    fn classification_vertex_point(&self, o: T) -> Option<Point3> {
        match o {
            T::Topo(b, EntityRef::Vertex(v)) => self.brep(b)?.vertex(v).ok().map(|v| v.point),
            _ => None,
        }
    }
    fn topology_zone_tol_3d(&self, o: T) -> Option<f64> {
        match o {
            T::Topo(b, EntityRef::Vertex(v)) => self.brep(b)?.vertex(v).ok().map(|v| v.tolerance),
            T::Topo(b, EntityRef::Edge(e)) => self.brep(b)?.edge(e).ok().map(|e| e.tolerance),
            T::Topo(b, EntityRef::Face(f)) => self.brep(b)?.face(f).ok().map(|f| f.tolerance),
            _ => None,
        }
    }
    fn vertex_face_gap(&self, vertex: T, _surface: &Surface, _face: Option<T>) -> Option<f64> {
        match vertex {
            T::Topo(0, EntityRef::Vertex(v)) => {
                self.gaps.iter().find(|(w, _)| *w == v).map(|(_, g)| *g)
            }
            _ => None,
        }
    }
    fn topology_entity(&self, o: T) -> Option<(&Brep, EntityRef)> {
        match o {
            T::Topo(b, e) => Some((self.brep(b)?, e)),
            _ => None,
        }
    }
    fn topology_handle(&self, entity: EntityRef) -> Option<T> {
        Some(T::Topo(0, entity))
    }
    fn topology_handle_in(&self, brep: &Brep, entity: EntityRef) -> Option<T> {
        let b = self.breps.iter().position(|x| std::ptr::eq(x, brep))?;
        Some(T::Topo(u8::try_from(b).ok()?, entity))
    }
    fn topology_intersector_mate(&self, ti: T, entity: T) -> Option<Option<T>> {
        if ti != T::Ti {
            return None;
        }
        // GetOtherMate (first B-rep to second), else GetBrepMate (second to first)
        Some(match entity {
            T::Topo(0, EntityRef::Edge(e)) => self
                .mates
                .iter()
                .find(|(a, _)| *a == e)
                .map(|(_, b)| T::Topo(1, EntityRef::Edge(*b))),
            T::Topo(1, EntityRef::Edge(e)) => self
                .mates
                .iter()
                .find(|(_, b)| *b == e)
                .map(|(a, _)| T::Topo(0, EntityRef::Edge(*a))),
            _ => None,
        })
    }
}

impl ClassGeometryMut<T> for World {
    fn curve_mut(&mut self, o: T) -> Option<&mut Curve> {
        match o {
            T::Curve => Some(&mut self.curve),
            _ => None,
        }
    }
    fn topology_entity_mut(&mut self, o: T) -> Option<(&mut Brep, EntityRef)> {
        match o {
            T::Topo(b, e) => Some((self.breps.get_mut(usize::from(b))?, e)),
            _ => None,
        }
    }
}

// ---- resolution -------------------------------------------------------------------------

/// A case resolved against the imported B-reps: the C++ input and the Rust world.
struct Resolved {
    path: String,
    /// B-rep block, edits and mates.
    prefix: Vec<f64>,
    breps: Vec<Brep>,
    /// The related edges (first B-rep, second).
    mates: Vec<(EdgeId, EdgeId)>,
    /// Whether the import holds the geometry exactly.
    exact: bool,
}

/// Builds and imports the case's B-reps; `Err(Output)` when one cannot be built (the
/// same outcome on both sides).
fn resolve(i: &TopoIn) -> Result<Result<Resolved, Output>, HarnessError> {
    let path = path_of(Some(&i.brep))?;
    let block = brep_block_of(Some(&i.brep), false, false)?;
    let plain = crate::export_memo::export("topology_edgeuse_p1", &[&path], &block, || {
        solidcore_ffi::units::topology_edgeuse_p1::eval(
            solidcore_ffi::units::topology_edgeuse_p1::Op::Export,
            &path,
            &block,
        )
    })
    .map_err(|e| harness(format!("c++ export: {e}")))?;
    let Some(counts) = solidcore_ffi::units::topology_edgeuse_p1::counts(&plain) else {
        return Ok(Err(Output::err(format!(
            "brep_build_status_{}",
            plain.get(1).copied().unwrap_or(0.0)
        ))));
    };
    let n_e = counts[7];
    let mut prefix = block;
    prefix.push(i.edits.len() as f64);
    for e in &i.edits {
        if e.sel < 0 {
            return Err(harness("negative edit selector"));
        }
        prefix.extend([f64::from(e.kind.min(6)), e.sel as f64, e.value]);
    }
    let mut mates: Vec<(i64, i64)> = Vec::new();
    for &[a, b] in &i.mates {
        let (a, b) = (sel(a, n_e), sel(b, n_e));
        if a < 0 || b < 0 || mates.iter().any(|&(x, y)| x == a || y == b) {
            continue;
        }
        mates.push((a, b));
    }
    prefix.push(mates.len() as f64);
    for &(a, b) in &mates {
        prefix.extend([a as f64, b as f64]);
    }
    let mut breps = Vec::new();
    let mut exact = true;
    for which in 0..if mates.is_empty() { 1 } else { 2 } {
        let mut input = prefix.clone();
        input.push(f64::from(which));
        let ex = crate::export_memo::export("intersect_curve_class_p5_topo", &[&path], &input, || {
            ffi::topo(TopoOp::Export, &path, &input)
        })
        .map_err(|e| harness(format!("c++ export: {e}")))?;
        if ex.first() != Some(&0.0) {
            return Ok(Err(Output::err(format!(
                "brep_build_status_{}_{}",
                ex.first().copied().unwrap_or(0.0),
                ex.get(1).copied().unwrap_or(0.0)
            ))));
        }
        let (brep, types) = import_with_types(&ex)?;
        exact &= (0..brep.faces().len()).all(|f| types.inexact_surface(f).is_none());
        exact &= types
            .edge_curve_types
            .iter()
            .all(|&t| t < 0 || t == BSPLINE_CURVE_TYPE || t == LINE_TYPE);
        breps.push(brep);
    }
    let mates = mates
        .iter()
        .map(|&(a, b)| {
            Ok((
                EdgeId::from_index(u32::try_from(a).map_err(hx)?),
                EdgeId::from_index(u32::try_from(b).map_err(hx)?),
            ))
        })
        .collect::<Result<Vec<_>, HarnessError>>()?;
    Ok(Ok(Resolved {
        path,
        prefix,
        breps,
        mates,
        exact,
    }))
}

fn edge_id(b: &Brep, s: i64) -> Result<EdgeId, HarnessError> {
    let e = sel(s, b.edges().len());
    u32::try_from(e)
        .map(EdgeId::from_index)
        .map_err(|_| harness("no edge"))
}

fn face_id(b: &Brep, s: i64) -> Result<FaceId, HarnessError> {
    let f = sel(s, b.faces().len());
    u32::try_from(f)
        .map(FaceId::from_index)
        .map_err(|_| harness("no face"))
}

fn vertex_id(b: &Brep, s: i64) -> Result<VertexId, HarnessError> {
    let v = sel(s, b.vertices().len());
    u32::try_from(v)
        .map(VertexId::from_index)
        .map_err(|_| harness("no vertex"))
}

fn v3(a: [f64; 3]) -> Vec3 {
    Vec3::new(a[0], a[1], a[2])
}

fn unit(v: Vec3) -> Result<Vec3, HarnessError> {
    let l = v.length();
    if l.is_finite() && l > 1e-12 {
        Ok(v / l)
    } else {
        Err(harness("degenerate direction"))
    }
}

/// A plane, or a bilinear B-spline patch of side 2 centred at `o`, normal to `n`.
fn class_surface(o: Vec3, n: Vec3, bspline: bool) -> Result<SrfIn, HarnessError> {
    if !bspline {
        return Ok(SrfIn::Plane {
            o: [o.x, o.y, o.z],
            n: [n.x, n.y, n.z],
        });
    }
    let n = unit(n)?;
    let a = if n.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    let u = unit(n.cross(a))?;
    let v = n.cross(u);
    let corner = |s: f64, t: f64| {
        let p = o + u * s + v * t;
        [p.x, p.y, p.z, NOW]
    };
    Ok(SrfIn::Bspline(BssIn {
        oob: false,
        form: 0,
        net: vec![
            vec![corner(-1.0, -1.0), corner(-1.0, 1.0)],
            vec![corner(1.0, -1.0), corner(1.0, 1.0)],
        ],
        p: 1,
        q: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
        v: vec![0.0, 0.0, 1.0, 1.0],
    }))
}

/// A 2D UV guess curve (a degree-1 segment).
fn uv_curve(a: [f64; 2], b: [f64; 2]) -> CrvIn {
    CrvIn {
        dim: 2,
        cpts: vec![[a[0], a[1], NOZ, NOW], [b[0], b[1], NOZ, NOW]],
        p: 1,
        u: vec![0.0, 0.0, 1.0, 1.0],
    }
}

/// The resolved op: its C++ arguments and what the Rust side needs.
enum Op {
    EdgeInsert {
        args: Vec<f64>,
        edge: EdgeId,
        p: Vec3,
        q: Vec3,
        srf: Option<[SrfIn; 2]>,
        sols: Vec<[f64; 7]>,
    },
    Vertices {
        args: Vec<f64>,
        face: FaceId,
        verts: Vec<VertexId>,
        curve: CrvIn,
        srf: Option<[SrfIn; 2]>,
        uv: [Option<CrvIn>; 2],
    },
}

fn op_args(k: &Kernel, r: &Resolved, op: &TopoOpIn) -> Result<Op, HarnessError> {
    let b = r.breps.first().ok_or_else(|| harness("no B-rep"))?;
    match op {
        TopoOpIn::EdgeInsert(e) => {
            let edge = edge_id(b, e.edge)?;
            let ivl = solidcore::topology::edge::interval(b, edge).map_err(hx)?;
            let curve = solidcore::topology::edge::curve(b, edge)
                .map_err(hx)?
                .ok_or_else(|| harness("edge without curve"))?;
            let param = |f: f64| ivl.min() + f * (ivl.max() - ivl.min());
            let x = curve.evaluate_point(k, param(e.at)).map_err(hx)?;
            let d = unit(v3(e.dir))?;
            let m = x + v3(e.off);
            let (p, q) = (m - d * e.half, m + d * e.half);
            let srf = match e.srf {
                Some([n1, n2]) => Some([
                    class_surface(x, v3(n1), false)?,
                    class_surface(x, v3(n2), false)?,
                ]),
                None => None,
            };
            let sols: Vec<[f64; 7]> = e
                .sols
                .iter()
                .map(|&[t, s0, e0, s1, e1, g0, g1]| [t, s0, param(e0), s1, param(e1), g0, g1])
                .collect();
            let mut args = vec![
                f64::from(edge.index()),
                f64::from(u8::from(e.ok_set)),
                f64::from(u8::from(e.ignore)),
                e.tol,
            ];
            args.extend([p.x, p.y, p.z, q.x, q.y, q.z, e.opt_tol]);
            args.push(f64::from(u8::from(srf.is_some())));
            if let Some([s1, s2]) = &srf {
                push_surface(&mut args, s1)?;
                push_surface(&mut args, s2)?;
            }
            args.push(sols.len() as f64);
            for s in &sols {
                args.extend(s);
            }
            Ok(Op::EdgeInsert {
                args,
                edge,
                p,
                q,
                srf,
                sols,
            })
        }
        TopoOpIn::Vertices(v) => {
            let face = face_id(b, v.face)?;
            let verts = if v.verts.is_empty() {
                let mut out = Vec::new();
                for i in 0..b.vertices().len() {
                    let id = VertexId::from_index(u32::try_from(i).map_err(hx)?);
                    if solidcore::topology::vertex::vertexuse_of_face(b, id, Some(face))
                        .map_err(hx)?
                        .is_some()
                    {
                        out.push(id);
                    }
                }
                out
            } else {
                v.verts
                    .iter()
                    .map(|&s| vertex_id(b, s))
                    .collect::<Result<Vec<_>, _>>()?
            };
            let anchor = usize::try_from(sel(v.anchor, verts.len()))
                .ok()
                .and_then(|i| verts.get(i).copied())
                .ok_or_else(|| harness("no vertex to anchor the curve at"))?;
            let a = b.vertex(anchor).map_err(hx)?.point;
            let (d, e, f) = match &v.srf {
                Some(s) => {
                    let d = unit(v3(s.n1).cross(v3(s.n2)))?;
                    let e = unit(v3(s.n1))?;
                    (d, e, d.cross(e))
                }
                None => (
                    Vec3::new(1.0, 0.0, 0.0),
                    Vec3::new(0.0, 1.0, 0.0),
                    Vec3::new(0.0, 0.0, 1.0),
                ),
            };
            let curve = CrvIn {
                dim: 3,
                cpts: v
                    .cpts
                    .iter()
                    .map(|&[s, t, w]| {
                        let p = a + d * s + e * t + f * w;
                        [p.x, p.y, p.z, NOW]
                    })
                    .collect(),
                p: v.p,
                u: v.knots.clone(),
            };
            let srf = match &v.srf {
                Some(s) => Some([
                    class_surface(a, v3(s.n1), s.bspline[0])?,
                    class_surface(a, v3(s.n2), s.bspline[1])?,
                ]),
                None => None,
            };
            let uv = [
                v.uv1.then(|| uv_curve([0.0, 0.0], [1.0, 1.0])),
                v.uv2.then(|| uv_curve([1.0, 0.0], [0.0, 1.0])),
            ];
            let flag = |x: bool| f64::from(u8::from(x));
            let mut args = vec![
                f64::from(face.index()),
                flag(v.big),
                flag(v.uv_ok),
                v.tol,
                flag(v.cc_face),
            ];
            push_curve(&mut args, &curve)?;
            args.push(flag(srf.is_some()));
            if let Some([s1, s2]) = &srf {
                push_surface(&mut args, s1)?;
                push_surface(&mut args, s2)?;
            }
            for c in &uv {
                args.push(flag(c.is_some()));
                if let Some(c) = c {
                    push_curve(&mut args, c)?;
                }
            }
            args.push(verts.len() as f64);
            args.extend(verts.iter().map(|v| f64::from(v.index())));
            Ok(Op::Vertices {
                args,
                face,
                verts,
                curve,
                srf,
                uv,
            })
        }
    }
}

// ---- the C++ side -----------------------------------------------------------------------

pub(super) fn cpp(i: &TopoIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let r = match resolve(i)? {
        Ok(r) => r,
        Err(o) => return Ok(o),
    };
    if !r.exact {
        return Ok(Output::err("inexact_geometry"));
    }
    let (code, args) = match op_args(&k, &r, &i.op)? {
        Op::EdgeInsert { args, .. } => (TopoOp::EdgeInsert, args),
        Op::Vertices { args, .. } => (TopoOp::Vertices, args),
    };
    let mut input = r.prefix.clone();
    input.extend(&args);
    let o = ffi::topo(code, &r.path, &input).map_err(|e| harness(format!("c++: {e}")))?;
    match o.first() {
        Some(&0.0) => Ok(Output::new().values("out", o.get(1..).unwrap_or(&[]).to_vec())),
        Some(&-3.0) => Ok(Output::rejected(
            "upstream would dereference a null edge curve, end vertex, surface owner or vertex use",
        )),
        Some(x) => Err(harness(format!("c++ setup failed after export ({x})"))),
        None => Err(harness("empty c++ output")),
    }
}

/// Validates a case's C++ input (for the generator).
pub(super) fn valid(i: &TopoIn) -> bool {
    let k = Kernel::default();
    matches!(cpp_checked(&k, i), Ok(true))
}

fn cpp_checked(k: &Kernel, i: &TopoIn) -> Result<bool, HarnessError> {
    let Ok(r) = resolve(i)? else {
        return Ok(false);
    };
    if !r.exact {
        return Ok(false);
    }
    let (code, args) = match op_args(k, &r, &i.op)? {
        Op::EdgeInsert { args, .. } => (TopoOp::EdgeInsert, args),
        Op::Vertices { args, .. } => (TopoOp::Vertices, args),
    };
    let mut input = r.prefix.clone();
    input.extend(&args);
    Ok(ffi::topo(code, &r.path, &input).is_ok_and(|o| o.first() == Some(&0.0)))
}

// ---- the Rust side ----------------------------------------------------------------------

fn status(e: &SolidError) -> f64 {
    match e {
        SolidError::Upstream { code, .. } => f64::from(*code),
        _ => -9.0,
    }
}

fn put_object(o: &mut Vec<f64>, h: Option<T>) {
    let topo = |kind: f64, b: u8, i: u32| [kind, f64::from(b), f64::from(i)];
    o.extend(match h {
        None => [0.0, -1.0, -1.0],
        Some(T::Curve) => [4.0, -1.0, -1.0],
        Some(T::S1) => [5.0, -1.0, -1.0],
        Some(T::S2) => [6.0, -1.0, -1.0],
        Some(T::Topo(b, EntityRef::Face(f))) => topo(1.0, b, f.index()),
        Some(T::Topo(b, EntityRef::Edge(e))) => topo(2.0, b, e.index()),
        Some(T::Topo(b, EntityRef::Vertex(v))) => topo(3.0, b, v.index()),
        Some(_) => [-3.0, -1.0, -1.0],
    });
}

fn put_pc(o: &mut Vec<f64>, pc: &PointClassification<T>) {
    o.push(f64::from(pc.point_class().code()));
    put_object(o, pc.object());
    let uv = pc.uv_param();
    let uvw = pc.uvw_param();
    let sss = pc.sss_point();
    o.extend([
        pc.deviation(),
        pc.t_param(),
        uv.x,
        uv.y,
        uvw.x,
        uvw.y,
        uvw.z,
        f64::from(u8::from(pc.are_parameters_set())),
        f64::from(u8::from(pc.sss_point_flag())),
        sss.x,
        sss.y,
        sss.z,
        pc.pre_snap_param(),
        pc.src_zone_tol_3d(),
    ]);
}

fn put_cc(o: &mut Vec<f64>, cc: &CurveClassification<T>) {
    o.extend([cc.size() as f64, cc.interval().min(), cc.interval().max()]);
    for ivl in cc.intervals() {
        o.extend([ivl.min(), ivl.max()]);
        for pc in [&ivl.start, &ivl.mid, &ivl.end] {
            put_pc(o, pc);
        }
    }
}

/// The shim's `put_curve` block of a B-spline curve (`sc_edgeuse_graph.h`).
fn put_curve(o: &mut Vec<f64>, c: &Curve) {
    let CurveKind::BSpline(b) = c.kind() else {
        o.push(-2.0);
        return;
    };
    o.push(c.type_code() as f64);
    o.push(b.dim() as f64);
    match b.gw_nurb() {
        Some(nurb) => {
            o.push(1.0);
            o.push(nurb.n() as f64);
            o.push(nurb.degree() as f64);
            o.push(nurb.m() as f64);
            for p in nurb.cpts().iter().take(nurb.n() + 1) {
                o.extend([p.x, p.y, p.z, p.w]);
            }
            o.extend(nurb.knots().iter().take(nurb.m() + 1));
        }
        None => o.push(0.0),
    }
}

fn put_topology_state(o: &mut Vec<f64>, w: &World) {
    for b in &w.breps {
        o.extend(b.vertices().iter().map(|(_, v)| v.tolerance));
        o.extend(
            b.edgeuses()
                .iter()
                .map(|(_, eu)| f64::from(u8::from(eu.uv_trim_curve.is_some()))),
        );
    }
}

/// `SmCurveClassification(curve, interval, uv_curve, SmZoneTol3d(tol))`: `ReSet` with
/// those arguments.
fn classification(
    k: &Kernel,
    interval: Extent1d,
    uv_curve: Option<T>,
    tol: f64,
) -> Result<CurveClassification<T>, SolidError> {
    let mut cc = CurveClassification::new(k);
    cc.reset(
        k,
        &ResetOptions {
            curve: Some(T::Curve),
            interval: Some(interval),
            uv_curve,
            zone_tol_3d: zone_tol3d(k, tol),
            ..ResetOptions::default()
        },
    )?;
    Ok(cc)
}

pub(super) fn rust(i: &TopoIn) -> Result<Output, HarnessError> {
    let k = Kernel::default();
    let r = match resolve(i)? {
        Ok(r) => r,
        Err(o) => return Ok(o),
    };
    if !r.exact {
        return Ok(Output::err("inexact_geometry"));
    }
    let op = op_args(&k, &r, &i.op)?;
    let mates = r.mates.clone();
    let mut o = Vec::new();
    match (op, &i.op) {
        (
            Op::EdgeInsert {
                edge, p, q, srf, sols, ..
            },
            TopoOpIn::EdgeInsert(e),
        ) => {
            let Ok(line) = solidcore::nurbs::b_spline_curve::BSplineCurve::create_line_segment(3, p, q)
            else {
                return Err(harness("line segment failed in Rust only"));
            };
            let curve = Curve::from(line);
            let interval = curve.natural_interval();
            let srf = match &srf {
                Some([a, b]) => [Some(surface_of(a)?), Some(surface_of(b)?)],
                None => [None, None],
            };
            let mut w = World {
                breps: r.breps,
                curve,
                uv: [None, None],
                srf,
                mates,
                gaps: Vec::new(),
            };
            let before = snapshot(&w);
            let target = T::Topo(0, EntityRef::Edge(edge));
            let mut cc = match classification(&k, interval, None, e.tol) {
                Ok(cc) => cc,
                Err(err) => return Ok(rust_err(&err)),
            };
            cc.set_ignore_tangency(e.ignore);
            cc.set_ok_to_set_tolerance(e.ok_set);
            if e.srf.is_some() {
                cc.set_intersect_data(Some(T::S1), Some(T::S2), None, true, None);
            }
            let mut solutions = Vec::new();
            for &[t, s0, e0, s1, e1, g0, g1] in &sols {
                let mut s = Solution::new();
                s.solution_type = SolutionType::from_code(t as i32)
                    .ok_or_else(|| harness("bad solution type"))?;
                s.num_objects = 2;
                s.num_variables = 2;
                if let Some(slot) = s.objects.get_mut(0) {
                    *slot = Some(T::Curve);
                }
                if let Some(slot) = s.objects.get_mut(1) {
                    *slot = Some(target);
                }
                for (end, a, b, g) in [(&mut s.start, s0, e0, g0), (&mut s.end, s1, e1, g1)] {
                    *end.param_mut(0).map_err(hx)? = a;
                    *end.param_mut(1).map_err(hx)? = b;
                    end.solution_value = g;
                }
                solutions.push(s);
            }
            let opt = (e.opt_tol >= 0.0).then_some(e.opt_tol);
            let st = match insert_solutions_mut(&k, &mut w, &mut cc, &mut solutions, target, opt) {
                Ok(()) => 0.0,
                Err(err) => {
                    if let Some(d) = Output::unported(&err) {
                        return Ok(d);
                    }
                    status(&err)
                }
            };
            o.push(st);
            put_cc(&mut o, &cc);
            for s in &solutions {
                o.extend([
                    f64::from(s.solution_type.code()),
                    s.start.param(0).map_err(hx)?,
                    s.start.param(1).map_err(hx)?,
                    s.end.param(0).map_err(hx)?,
                    s.end.param(1).map_err(hx)?,
                    s.start.solution_value,
                    s.end.solution_value,
                ]);
            }
            put_topology_state(&mut o, &w);
            let after = snapshot(&w);
            Ok(diagnostics(Output::new().values("out", o), &before, &after, &cc))
        }
        (
            Op::Vertices {
                face,
                verts,
                curve,
                srf,
                uv,
                ..
            },
            TopoOpIn::Vertices(v),
        ) => {
            let curve = Curve::from(bspline_of(&curve)?);
            let interval = curve.natural_interval();
            let srf = match &srf {
                Some([a, b]) => [Some(surface_of(a)?), Some(surface_of(b)?)],
                None => [None, None],
            };
            let uv = [
                uv[0].as_ref().map(bspline_of).transpose()?.map(Curve::from),
                uv[1].as_ref().map(bspline_of).transpose()?.map(Curve::from),
            ];
            let mut breps = r.breps;
            // the face surface, a copy keeping its owner (the B-reps are edited)
            let surface = {
                let b = breps.first().ok_or_else(|| harness("no B-rep"))?;
                let s = b
                    .face(face)
                    .map_err(hx)?
                    .surface
                    .as_ref()
                    .ok_or_else(|| harness("face without surface"))?;
                let mut c = s.clone();
                c.set_owner(s.owner());
                c
            };
            let mut gaps = Vec::new();
            if v.big {
                let b = breps.first_mut().ok_or_else(|| harness("no B-rep"))?;
                for &vx in &verts {
                    let Some(vu) =
                        solidcore::topology::vertex::vertexuse_of_face(b, vx, Some(face)).map_err(hx)?
                    else {
                        continue;
                    };
                    let gap = match solidcore::topology::vertexuse::vertex_face_gap(&k, b, vu, false) {
                        Ok(g) => g.map_or(0.0, |g| g.gap_3d),
                        Err(err) => return Ok(rust_err(&err)),
                    };
                    gaps.push((vx, gap));
                }
            }
            let ti = (!mates.is_empty()).then_some(T::Ti);
            let mut w = World {
                breps,
                curve,
                uv,
                srf,
                mates,
                gaps,
            };
            let before = snapshot(&w);
            let mut cc = match classification(&k, interval, v.uv1.then_some(T::Uv1), v.tol) {
                Ok(cc) => cc,
                Err(err) => return Ok(rust_err(&err)),
            };
            if v.srf.is_some() {
                cc.set_intersect_data(Some(T::S1), Some(T::S2), v.uv2.then_some(T::Uv2), true, ti);
            }
            if v.cc_face {
                cc.set_face(Some(T::Topo(0, EntityRef::Face(face))));
            }
            let handles: Vec<T> = verts
                .iter()
                .map(|&v| T::Topo(0, EntityRef::Vertex(v)))
                .collect();
            let st = match cc.intersect_vertices_mut(&k, &mut w, &handles, &surface, v.big, v.uv_ok) {
                Ok(()) => 0.0,
                Err(err) => {
                    if let Some(d) = Output::unported(&err) {
                        return Ok(d);
                    }
                    status(&err)
                }
            };
            o.extend([st, f64::from(u8::from(cc.is_curve_current()))]);
            put_curve(&mut o, &w.curve);
            put_cc(&mut o, &cc);
            put_topology_state(&mut o, &w);
            let after = snapshot(&w);
            Ok(diagnostics(Output::new().values("out", o), &before, &after, &cc))
        }
        _ => Err(harness("op mismatch")),
    }
}

/// The state [`diagnostics`] compares: vertex tolerances and UV trim curve presence per
/// B-rep, and the classification curve block.
struct Snapshot {
    tols: Vec<Vec<f64>>,
    uv: Vec<Vec<bool>>,
    curve: Vec<f64>,
}

fn snapshot(w: &World) -> Snapshot {
    let mut curve = Vec::new();
    put_curve(&mut curve, &w.curve);
    Snapshot {
        tols: w
            .breps
            .iter()
            .map(|b| b.vertices().iter().map(|(_, v)| v.tolerance).collect())
            .collect(),
        uv: w
            .breps
            .iter()
            .map(|b| {
                b.edgeuses()
                    .iter()
                    .map(|(_, eu)| eu.uv_trim_curve.is_some())
                    .collect()
            })
            .collect(),
        curve,
    }
}

/// What a case exercised (reported, never compared): vertex tolerances raised, edge-use
/// UV trim curves deleted (in the second B-rep: through a mate), the classification
/// curve edited, and the objects of the resulting point classifications.
fn diagnostics(o: Output, before: &Snapshot, after: &Snapshot, cc: &CurveClassification<T>) -> Output {
    let raised: usize = before
        .tols
        .iter()
        .zip(&after.tols)
        .map(|(a, b)| a.iter().zip(b).filter(|(x, y)| x != y).count())
        .sum();
    let deleted: Vec<usize> = before
        .uv
        .iter()
        .zip(&after.uv)
        .map(|(a, b)| a.iter().zip(b).filter(|(x, y)| **x && !**y).count())
        .collect();
    let edited = before.curve.len() != after.curve.len()
        || before
            .curve
            .iter()
            .zip(&after.curve)
            .any(|(a, b)| a.to_bits() != b.to_bits());
    let (mut vertex, mut edge, mut far) = (0, 0, 0);
    for ivl in cc.intervals() {
        for pc in [&ivl.start, &ivl.mid, &ivl.end] {
            match pc.object() {
                Some(T::Topo(b, EntityRef::Vertex(v))) => {
                    vertex += 1;
                    // inserted beyond the source + vertex tolerance: a large-gap vertex
                    let tol = after
                        .tols
                        .get(usize::from(b))
                        .and_then(|t| t.get(v.index() as usize))
                        .copied()
                        .unwrap_or(0.0);
                    far += i64::from(pc.deviation() > pc.src_zone_tol_3d() + tol);
                }
                Some(T::Topo(_, EntityRef::Edge(_))) => edge += 1,
                _ => {}
            }
        }
    }
    let n = |x: usize| i64::try_from(x).unwrap_or(i64::MAX);
    o.diagnostic("vertex_tol_raised", n(raised))
        .diagnostic("uv_trims_deleted", n(deleted.first().copied().unwrap_or(0)))
        .diagnostic("mate_uv_trims_deleted", n(deleted.get(1).copied().unwrap_or(0)))
        .diagnostic("curve_edited", i64::from(edited))
        .diagnostic("curve_not_current", i64::from(!cc.is_curve_current()))
        .diagnostic("vertex_classes", vertex)
        .diagnostic("edge_classes", edge)
        .diagnostic("large_gap_vertex_classes", far)
}

fn rust_err(e: &SolidError) -> Output {
    Output::unported(e).unwrap_or_else(|| Output::err(format!("rust_error: {e}")))
}

// ---- generation -------------------------------------------------------------------------

fn gen_box(rng: &mut Rng) -> BrepIn {
    let o = if rng.chance(0.5) {
        [0.0, 0.0, 0.0]
    } else {
        [rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0), rng.uniform(-2.0, 2.0)]
    };
    let s = *rng.choose(&[1.0, 2.0, 0.5, 10.0]).unwrap_or(&1.0);
    BrepIn::Box {
        origin: o,
        size: [s * rng.uniform(0.5, 2.0), s * rng.uniform(0.5, 2.0), s * rng.uniform(0.5, 2.0)],
    }
}

fn gen_dir(rng: &mut Rng) -> [f64; 3] {
    loop {
        let d = [rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0), rng.uniform(-1.0, 1.0)];
        if d.iter().map(|x| x * x).sum::<f64>() > 0.05 {
            return d;
        }
    }
}

fn gen_edge_insert(rng: &mut Rng) -> TopoIn {
    let at = *rng
        .choose(&[0.0, 1.0, 0.5, 1e-9, 1.0 - 1e-9, 1e-6, 0.999_999, 0.25])
        .unwrap_or(&0.5);
    let at = if rng.chance(0.2) { rng.unit() } else { at };
    // with classification planes through the crossing point (the line in both), the
    // refinement onto surf/surf/surf points finds no gaps when the solution is exact
    let coplanar = rng.chance(0.25);
    let mut dir = gen_dir(rng);
    let srf = if coplanar {
        let n1 = gen_dir(rng);
        let l2: f64 = n1.iter().map(|x| x * x).sum();
        let dot: f64 = n1.iter().zip(&dir).map(|(a, b)| a * b).sum();
        for (d, n) in dir.iter_mut().zip(&n1) {
            *d -= dot / l2 * n;
        }
        if dir.iter().map(|x| x * x).sum::<f64>() < 1e-6 {
            dir = [n1[1], -n1[0], 0.0];
        }
        let n2 = [
            n1[1] * dir[2] - n1[2] * dir[1],
            n1[2] * dir[0] - n1[0] * dir[2],
            n1[0] * dir[1] - n1[1] * dir[0],
        ];
        Some([n1, n2])
    } else {
        None
    };
    let off_scale = if coplanar {
        0.0
    } else {
        *rng.choose(&[0.0, 0.0, 1e-9, 1e-7, 1e-6, 5e-6, 1e-4])
            .unwrap_or(&0.0)
    };
    let off = [
        rng.uniform(-1.0, 1.0) * off_scale,
        rng.uniform(-1.0, 1.0) * off_scale,
        rng.uniform(-1.0, 1.0) * off_scale,
    ];
    let gap = (off[0] * off[0] + off[1] * off[1] + off[2] * off[2]).sqrt();
    let mut sols = Vec::new();
    for _ in 0..=if coplanar { 0 } else { rng.below(2) } {
        let s = 0.5 + if !coplanar && rng.chance(0.3) { rng.uniform(-1e-3, 1e-3) } else { 0.0 };
        let e = at + if !coplanar && rng.chance(0.3) { rng.uniform(-1e-6, 1e-6) } else { 0.0 };
        if !coplanar && rng.chance(0.25) {
            let ds = *rng.choose(&[1e-12, 1e-8, 0.01, 0.2]).unwrap_or(&0.01);
            sols.push([2.0, s - ds, e, s + ds, e, gap, gap * rng.uniform(0.5, 1.5)]);
        } else {
            sols.push([1.0, s, e, s, e, gap, gap]);
        }
    }
    let mut edits = Vec::new();
    if rng.chance(0.5) {
        // edge tolerances above the vertices': a vertex snap raises the vertex's
        let value = *rng.choose(&[1e-5, 1e-4]).unwrap_or(&1e-5);
        for e in 0..12 {
            edits.push(Edit {
                kind: 1,
                sel: e,
                value,
            });
        }
    }
    if rng.chance(0.3) {
        let value = *rng.choose(&[1e-7, 1e-6, 1e-5]).unwrap_or(&1e-6);
        for v in 0..8 {
            edits.push(Edit {
                kind: 2,
                sel: v,
                value,
            });
        }
    }
    TopoIn {
        brep: gen_box(rng),
        edits,
        mates: Vec::new(),
        op: TopoOpIn::EdgeInsert(EdgeInsertIn {
            edge: rng.below(64) as i64,
            at,
            dir,
            half: *rng.choose(&[0.1, 1.0, 5.0]).unwrap_or(&1.0),
            off,
            tol: *rng.choose(&[1e-6, 1e-6, 1e-5, 0.0, 1e-3]).unwrap_or(&1e-6),
            ok_set: rng.chance(0.6),
            ignore: rng.chance(0.3),
            opt_tol: if coplanar || rng.chance(0.7) {
                -1.0
            } else {
                rng.unit() * 1e-4
            },
            srf,
            sols,
        }),
    }
}

/// A vertex case on a box: classification planes through a corner (two of its face
/// normals, sometimes tilted) and a curve along their intersection near it.
fn gen_vertices(rng: &mut Rng) -> TopoIn {
    let anchor = rng.below(4) as i64;
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let srf = if rng.chance(0.75) {
        let i = rng.below(3) as usize;
        let j = (i + 1 + rng.below(2) as usize) % 3;
        let tilt = |rng: &mut Rng, a: [f64; 3]| -> [f64; 3] {
            if rng.chance(0.3) {
                [a[0] + rng.uniform(-0.3, 0.3), a[1] + rng.uniform(-0.3, 0.3), a[2]]
            } else {
                a
            }
        };
        let axis = |k: usize| axes.get(k).copied().unwrap_or([0.0, 0.0, 1.0]);
        let n1 = tilt(rng, axis(i));
        let n2 = tilt(rng, axis(j));
        Some(SrfPair {
            n1,
            n2,
            bspline: [rng.chance(0.3), rng.chance(0.3)],
        })
    } else {
        None
    };
    // the curve: along d, offset by `off` in e and f
    // (an unowned classification surface's zone tolerance is the system one, so the
    // curve is edited for drop distances from about 1.1e-5 up to the vertex tolerance)
    let off_scale = *rng
        .choose(&[
            0.0, 1e-9, 5e-8, 2e-7, 1e-6, 5e-6, 2e-5, 5e-5, 5e-5, 2e-4, 2e-4, 1e-3,
        ])
        .unwrap_or(&1e-6);
    let (oe, of) = (
        rng.uniform(-1.0, 1.0) * off_scale,
        rng.uniform(-1.0, 1.0) * off_scale,
    );
    let len = *rng.choose(&[0.5, 1.0, 3.0]).unwrap_or(&1.0);
    let (p, cpts, knots) = if rng.chance(0.25) {
        // a cubic loop crossing itself near the anchor (two drop points near one knot)
        let s = len;
        let raw = [[0.0, 0.0], [1.0, 1.0], [0.0, 1.0], [1.0, 0.0]];
        // x(t) = 3t(1-t)^2 + t^3 = 1/2 at the crossing t (bisection), y = 3t(1-t)
        let (mut lo, mut hi) = (0.0f64, 0.5f64);
        for _ in 0..60 {
            let t = 0.5 * (lo + hi);
            let x = 3.0 * t * (1.0 - t) * (1.0 - t) + t * t * t;
            if x < 0.5 {
                lo = t;
            } else {
                hi = t;
            }
        }
        let t = 0.5 * (lo + hi);
        let (cx, cy) = (0.5, 3.0 * t * (1.0 - t));
        let cpts = raw
            .iter()
            .map(|&[x, y]| [(x - cx) * s, oe + (y - cy) * s, of])
            .collect::<Vec<_>>();
        (3, cpts, vec![0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0])
    } else if srf.is_some() && rng.chance(0.6) {
        // a piecewise C0 cubic along d, as the surface intersector makes (only such a
        // curve is edited to pass through a surf/surf/surf point)
        let spans = 1 + rng.below(3) as usize;
        let mut cuts: Vec<f64> = (0..spans - 1).map(|_| rng.uniform(0.1, 0.9)).collect();
        cuts.sort_by(f64::total_cmp);
        let mut knots = vec![0.0; 4];
        for &c in &cuts {
            knots.extend([c, c, c]);
        }
        knots.extend([1.0; 4]);
        let n = 3 * spans;
        let cpts = (0..=n)
            .map(|i| [-len + 2.0 * len * i as f64 / n as f64, oe, of])
            .collect::<Vec<_>>();
        (3, cpts, knots)
    } else {
        let p = 1 + rng.below(3) as usize;
        let n = p + rng.below(4) as usize;
        let cpts = (0..=n)
            .map(|i| {
                let a = -len + 2.0 * len * i as f64 / n as f64;
                [a, oe, of]
            })
            .collect::<Vec<_>>();
        let mut knots = vec![0.0; p + 1];
        let inner = n - p;
        let mut cuts: Vec<f64> = (0..inner).map(|_| rng.uniform(0.05, 0.95)).collect();
        cuts.sort_by(f64::total_cmp);
        knots.extend(cuts);
        knots.extend(vec![1.0; p + 1]);
        (p, cpts, knots)
    };
    let mut edits = Vec::new();
    if rng.chance(if srf.is_some() { 0.8 } else { 0.5 }) {
        // every vertex tolerance (the anchor is one of them)
        let value = *rng.choose(&[1e-5, 1e-4, 1e-3]).unwrap_or(&1e-4);
        for v in 0..8 {
            edits.push(Edit {
                kind: 2,
                sel: v,
                value,
            });
        }
    }
    if rng.chance(0.3) {
        edits.push(Edit {
            kind: 3,
            sel: rng.below(6) as i64,
            value: 0.0,
        });
    }
    let big = srf.is_none() && rng.chance(0.5);
    if big && rng.chance(0.7) {
        // a vertex moved off its face: face tolerances above the move keep its
        // projection local (a failed local projection builds the face's trimmed surface
        // cache in C++, whose edge validation raises the vertex tolerance; see
        // plan/findings.md)
        for f in 0..6 {
            edits.push(Edit {
                kind: 0,
                sel: f,
                value: 1e-3,
            });
        }
        edits.push(Edit {
            kind: 4 + rng.below(3) as u8,
            sel: rng.below(8) as i64,
            value: *rng.choose(&[1e-5, -1e-5, 1e-4, 2e-6]).unwrap_or(&1e-5),
        });
    }
    let mates = if srf.is_some() && rng.chance(0.6) {
        // every edge mated (to itself, or shifted), or a few random pairs
        if rng.chance(0.6) {
            let shift = if rng.chance(0.7) { 0 } else { rng.below(12) as i64 };
            (0..12).map(|e| [e, (e + shift) % 12]).collect()
        } else {
            (0..2 + rng.below(4))
                .map(|_| [rng.below(12) as i64, rng.below(12) as i64])
                .collect()
        }
    } else {
        Vec::new()
    };
    TopoIn {
        brep: gen_box(rng),
        edits,
        mates,
        op: TopoOpIn::Vertices(VerticesIn {
            face: rng.below(6) as i64,
            verts: if rng.chance(0.3) {
                vec![rng.below(8) as i64]
            } else {
                Vec::new()
            },
            anchor,
            big,
            uv_ok: true,
            tol: *rng.choose(&[1e-6, 1e-6, 1e-5, 0.0]).unwrap_or(&1e-6),
            cc_face: rng.chance(0.3),
            p,
            cpts,
            knots,
            srf,
            uv1: rng.chance(0.3),
            uv2: rng.chance(0.3),
        }),
    }
}

/// A generated B-rep case whose C++ input is valid, or `None`.
pub(super) fn generate(rng: &mut Rng) -> Option<TopoIn> {
    for _ in 0..32 {
        let i = if rng.chance(0.5) {
            gen_edge_insert(rng)
        } else {
            gen_vertices(rng)
        };
        if valid(&i) {
            return Some(i);
        }
    }
    None
}
