//! Arena access for the existing generic topology intersector used by merge p4.
//!
//! Upstream: `source/SMLib/src/SmTopologyIntersector.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `ArenaPair` (private) | operand access through the intersector's topology handles |
//!
//! Deviations: borrowed arenas replace pointers; generated curves have local handles.
//! This adapts the ported operations without copying their geometry algorithms.
//! No input-dependent recursion is introduced.

use super::notification_keys::NotificationKeys;
use crate::boolean::topology_intersector::p1 as ti;
use crate::boolean::topology_intersector::p1::IntersectorBrep;
use crate::boolean::topology_intersector::p2::IntersectorBrepP2;
use crate::curve::curve::Curve as SmCurve;
use crate::geom::a_object::BREP_TYPE;
use crate::geom::attribute::{AttributeStore, ObjectKey};
use crate::geom::context::Context;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::Extent3d;
use crate::geom::object::ShapeMember;
use crate::geom::solution_array::{ObjectKind, SolutionObject};
use crate::geom::tol::{update_object_tolerance, TopologyToUpdate};
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::point_class::{
    ClassGeometry, ClassObject, ClassObjectKind, PointClassification,
};
use crate::intersect::topology_solver::p1::TopologyObjects;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3};
use crate::nurbs::b_spline_curve::p2::BoundingBoxRequest;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::query::brep::{BrepIdentity, BrepQuery};
use crate::query::face::p2::{
    curve_on_classify, point_classify, CurveOnClassifyObjects, PointClassifyOptions,
};
use crate::surface::surface::Surface;
use crate::surface::volume::Volume;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p7;
use crate::topology::brep_cache::{arena_identity, BrepCache, CacheObject, CacheSource};
use crate::topology::edge as ed;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3 as eu3;
use crate::topology::face::p2 as fp2;
use crate::topology::face::p3 as fp3;
use crate::topology::face::p3::{FaceIntersectOptions, IntersectObject};
use crate::topology::proto_topology::p1::ProtoTopology;
use crate::topology::proto_topology::{ProtoBrep, ProtoGeometry};
use crate::topology::topology::Topology;
use crate::topology::vertex as vx;
use crate::topology::{Brep, BrepId, EdgeId, EdgeuseId, FaceId, LoopId, VertexId, VertexuseId};
use crate::{EntityRef, SolidError};
use std::collections::BTreeMap;
const K_VERTEX: u8 = 0;
const K_EDGE: u8 = 1;
const K_FACE: u8 = 2;
const K_EDGE_CURVE: u8 = 3;
const K_FACE_SURFACE: u8 = 4;
const K_CURVE: u8 = 5;
const K_BREP: u8 = 6;
const K_EXTRA: u8 = 7;
const K_EDGEUSE: u8 = 8;
const K_VERTEXUSE: u8 = 9;
const K_LOOP: u8 = 10;
const K_TI: u8 = 11;
const K_TRIM_CURVE: u8 = 12;
const K_NULL: u8 = 13;
const K_REGION: u8 = 14;
/// An object handle: the B-rep (`2` for the curve stores, `-1` for the intersector),
/// the kind and the index. Ordered by operand, class and arena index. A
/// curve handle also carries its curve's natural interval (what `SmSolutionArray`
/// reads through `SmCurve::GetNaturalInterval`); it takes no part in identity.
#[derive(Debug, Clone, Copy)]
pub(super) struct H {
    b: i8,
    kind: u8,
    i: u32,
    ivl: [f64; 2],
}

impl PartialEq for H {
    fn eq(&self, o: &Self) -> bool {
        (self.b, self.kind, self.i) == (o.b, o.kind, o.i)
    }
}

impl Eq for H {}

impl PartialOrd for H {
    fn partial_cmp(&self, o: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(o))
    }
}

impl Ord for H {
    fn cmp(&self, o: &Self) -> std::cmp::Ordering {
        (self.b, self.kind, self.i).cmp(&(o.b, o.kind, o.i))
    }
}

pub(super) const fn hd(b: i8, kind: u8, i: u32) -> H {
    H {
        b,
        kind,
        i,
        ivl: [f64::NAN, f64::NAN],
    }
}

/// A curve handle carrying the curve's natural interval.
fn curve_h(b: i8, kind: u8, i: u32, c: &SmCurve) -> H {
    let ivl = c.natural_interval();
    H {
        b,
        kind,
        i,
        ivl: [ivl.min(), ivl.max()],
    }
}

impl ClassObject for H {
    fn class_kind(&self) -> ClassObjectKind {
        match self.kind {
            K_VERTEX => ClassObjectKind::Vertex,
            K_EDGE => ClassObjectKind::Edge,
            K_FACE => ClassObjectKind::Face,
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_TRIM_CURVE => ClassObjectKind::Curve,
            K_FACE_SURFACE => ClassObjectKind::Surface,
            K_EDGEUSE => ClassObjectKind::Edgeuse,
            K_VERTEXUSE => ClassObjectKind::Vertexuse,
            K_LOOP => ClassObjectKind::Loop,
            K_REGION => ClassObjectKind::Region,
            _ => ClassObjectKind::Other,
        }
    }
}

impl SolutionObject for H {
    fn kind(&self) -> ObjectKind {
        match self.kind {
            K_VERTEX => ObjectKind::Vertex,
            K_EDGE => ObjectKind::Edge,
            K_FACE => ObjectKind::Face,
            K_EDGE_CURVE | K_CURVE | K_EXTRA | K_TRIM_CURVE => ObjectKind::Curve,
            K_FACE_SURFACE => ObjectKind::Surface,
            _ => ObjectKind::Other,
        }
    }
    fn natural_interval(&self) -> Option<Extent1d> {
        match self.kind {
            K_EDGE_CURVE | K_CURVE | K_EXTRA if self.ivl.first()?.is_finite() => Some(
                Extent1d::from_min_max(*self.ivl.first()?, *self.ivl.get(1)?),
            ),
            _ => None,
        }
    }
}

pub(super) const TI_HANDLE: H = hd(-1, K_TI, 0);

pub(super) const fn brep_h(b: i8) -> H {
    hd(b, K_BREP, b as u32)
}

#[derive(Debug)]
pub(super) struct ArenaPair<'a> {
    breps: [Option<&'a mut Brep>; 2],
    curves: &'a mut Vec<SmCurve>,
    extra: &'a mut Vec<SmCurve>,
    records: &'a mut BTreeMap<ObjectKey, Topology>,
    attributes: &'a mut AttributeStore,
    primary_context: &'a mut Context,
    other_context: Option<&'a mut Context>,
    keys: NotificationKeys<'a>,
    cache_kernels: Option<[Kernel; 2]>,
    caches: [std::cell::OnceCell<BrepCache<H>>; 2],
}
fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input("boolean::merge::p4::arena_pair", reason)
}
const fn vid(h: H) -> VertexId {
    VertexId::from_index(h.i)
}
const fn eid(h: H) -> EdgeId {
    EdgeId::from_index(h.i)
}
const fn fid(h: H) -> FaceId {
    FaceId::from_index(h.i)
}
impl ArenaPair<'_> {
    pub(super) fn new<'a>(
        primary: &'a mut Brep,
        other: Option<&'a mut Brep>,
        curves: &'a mut Vec<SmCurve>,
        extra: &'a mut Vec<SmCurve>,
        records: &'a mut BTreeMap<ObjectKey, Topology>,
        attributes: &'a mut AttributeStore,
        cache_kernels: Option<[Kernel; 2]>,
        primary_context: &'a mut Context,
        other_context: Option<&'a mut Context>,
        keys: NotificationKeys<'a>,
    ) -> ArenaPair<'a> {
        ArenaPair {
            breps: [Some(primary), other],
            curves,
            extra,
            records,
            attributes,
            primary_context,
            other_context,
            keys,
            cache_kernels,
            caches: [std::cell::OnceCell::new(), std::cell::OnceCell::new()],
        }
    }
    fn brep(&self, h: H) -> Option<&Brep> {
        self.breps.get(usize::try_from(h.b).ok()?)?.as_deref()
    }

    fn brep_mut(&mut self, h: H) -> Option<&mut Brep> {
        self.breps
            .get_mut(usize::try_from(h.b).ok()?)?
            .as_deref_mut()
    }

    fn brep_r(&self, h: H) -> crate::Result<&Brep> {
        self.brep(h)
            .ok_or_else(|| invalid("handle names an absent B-rep"))
    }

    fn brep_m(&mut self, h: H) -> crate::Result<&mut Brep> {
        self.brep_mut(h)
            .ok_or_else(|| invalid("handle names an absent B-rep"))
    }

    /// The edge's curve handle (`ProtoBrep::edge_curve`'s shape).
    #[allow(clippy::option_option)]
    fn edge_curve_h(&self, e: H) -> Option<Option<H>> {
        let b = self.brep(e)?;
        let edge = b.edge(eid(e)).ok()?;
        Some(
            edge.curve
                .as_ref()
                .map(|c| curve_h(e.b, K_EDGE_CURVE, e.i, c)),
        )
    }

    /// The face's surface handle (`ProtoBrep::face_surface`'s shape).
    #[allow(clippy::option_option)]
    fn face_surface_h(&self, f: H) -> Option<Option<H>> {
        let b = self.brep(f)?;
        let face = b.face(fid(f)).ok()?;
        Some(face.surface.as_ref().map(|_| hd(f.b, K_FACE_SURFACE, f.i)))
    }

    /// Gives a curve made by the port a handle in the curve store.
    fn register_curve(&mut self, curve: SmCurve) -> crate::Result<H> {
        self.insert_curve(curve)
            .ok_or_else(|| invalid("curve store is full"))
    }

    /// [`Two::register_curve`] for each curve present.
    fn register_curves(&mut self, curves: Vec<Option<SmCurve>>) -> crate::Result<Vec<Option<H>>> {
        curves
            .into_iter()
            .map(|c| c.map(|c| self.register_curve(c)).transpose())
            .collect()
    }

    fn entity_h(b: i8, e: EntityRef) -> Option<H> {
        match e {
            EntityRef::Vertex(v) => Some(hd(b, K_VERTEX, v.index())),
            EntityRef::Edge(x) => Some(hd(b, K_EDGE, x.index())),
            EntityRef::Face(f) => Some(hd(b, K_FACE, f.index())),
            EntityRef::Loop(l) => Some(hd(b, K_LOOP, l.index())),
            EntityRef::Edgeuse(eu) => Some(hd(b, K_EDGEUSE, eu.index())),
            EntityRef::Region(r) => Some(hd(b, K_REGION, r.index())),
            _ => None,
        }
    }
}
impl ClassGeometry<H> for ArenaPair<'_> {
    fn curve(&self, o: H) -> Option<&SmCurve> {
        match o.kind {
            K_EDGE_CURVE => self.brep(o)?.edge(eid(o)).ok()?.curve.as_ref(),
            K_CURVE => self.curves.get(o.i as usize),
            K_TRIM_CURVE => {
                eu1::uv_trim_curve(self.brep(o)?, EdgeuseId::from_index(o.i))
                    .ok()?
                    .0
            }
            K_EXTRA => self.extra.get(o.i as usize),
            _ => None,
        }
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        match o.kind {
            K_FACE_SURFACE => self.brep(o)?.face(fid(o)).ok()?.surface.as_ref(),
            _ => None,
        }
    }
    fn volume(&self, _o: H) -> Option<&Volume> {
        None
    }
    fn point(&self, _o: H) -> Option<Point3> {
        None
    }
    // Classification reads the actual vertex points and stored tolerances.
    fn classification_vertex_point(&self, o: H) -> Option<Point3> {
        if o.kind != K_VERTEX {
            return None;
        }
        vx::point(self.brep(o)?, vid(o)).ok()
    }
    fn topology_zone_tol_3d(&self, o: H) -> Option<f64> {
        match o.kind {
            K_VERTEX | K_EDGE | K_FACE => self.topology_tolerance(o),
            _ => None,
        }
    }
}

impl ProtoGeometry<H> for ArenaPair<'_> {
    fn curve_mut(&mut self, o: H) -> Option<&mut SmCurve> {
        match o.kind {
            K_EDGE_CURVE => self.brep_mut(o)?.edge_mut(eid(o)).ok()?.curve.as_mut(),
            K_CURVE => self.curves.get_mut(o.i as usize),
            K_EXTRA => self.extra.get_mut(o.i as usize),
            _ => None,
        }
    }
}

/// The face-side objects of `curve_on_classify` for one B-rep.
struct Objs<'a, 'b> {
    two: &'a ArenaPair<'b>,
    b: i8,
}

impl ClassGeometry<H> for Objs<'_, '_> {
    fn curve(&self, o: H) -> Option<&SmCurve> {
        ClassGeometry::curve(self.two, o)
    }
    fn surface(&self, o: H) -> Option<&Surface> {
        ClassGeometry::surface(self.two, o)
    }
    fn volume(&self, o: H) -> Option<&Volume> {
        ClassGeometry::volume(self.two, o)
    }
    fn point(&self, o: H) -> Option<Point3> {
        ClassGeometry::point(self.two, o)
    }
}

impl CurveOnClassifyObjects<H> for Objs<'_, '_> {
    fn face(&self, f: FaceId) -> H {
        hd(self.b, K_FACE, f.index())
    }
    fn vertex(&self, v: VertexId) -> H {
        hd(self.b, K_VERTEX, v.index())
    }
    fn edge(&self, e: EdgeId) -> H {
        hd(self.b, K_EDGE, e.index())
    }
    fn vertexuse(&self, vu: VertexuseId) -> H {
        hd(self.b, K_VERTEXUSE, vu.index())
    }
    fn edgeuse(&self, eu: EdgeuseId) -> H {
        hd(self.b, K_EDGEUSE, eu.index())
    }
    fn edge_curve(&self, e: EdgeId) -> H {
        hd(self.b, K_EDGE_CURVE, e.index())
    }
    fn trim_curve(&self, eu: EdgeuseId) -> H {
        hd(self.b, K_TRIM_CURVE, eu.index())
    }
    fn face_surface(&self, f: FaceId) -> Option<H> {
        self.two
            .face_surface_h(hd(self.b, K_FACE, f.index()))
            .flatten()
    }
    fn loop_id(&self, tgt_loop: H) -> Option<LoopId> {
        (tgt_loop.kind == K_LOOP && tgt_loop.b == self.b).then(|| LoopId::from_index(tgt_loop.i))
    }
}

impl ProtoBrep<H> for ArenaPair<'_> {
    fn edge_interval(&self, edge: H) -> Option<Extent1d> {
        ed::interval(self.brep(edge)?, eid(edge)).ok()
    }
    fn edge_curve(&self, edge: H) -> Option<Option<H>> {
        self.edge_curve_h(edge)
    }
    fn topology_tolerance(&self, topo: H) -> Option<f64> {
        let b = self.brep(topo)?;
        match topo.kind {
            K_VERTEX => vx::tolerance(b, vid(topo)).ok(),
            K_EDGE => ed::tolerance(b, eid(topo)).ok(),
            K_FACE => fp2::tolerance(b, fid(topo)).ok(),
            _ => None,
        }
    }
    fn face_uv_domain(&self, face: H) -> Option<Extent2d> {
        fp2::uv_domain(self.brep(face)?, fid(face)).ok()
    }
    fn set_face_uv_domain(&mut self, face: H, domain: Extent2d) -> Option<()> {
        fp2::set_uv_domain(self.brep_mut(face)?, fid(face), &domain).ok()
    }
    fn face_surface(&self, face: H) -> Option<Option<H>> {
        self.face_surface_h(face)
    }
    fn edge_edgeuse_of_face(&self, edge: H, face: H) -> crate::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        let face = (face.b == edge.b && face.kind == K_FACE).then(|| fid(face));
        Ok(ed::edgeuse_of_face(b, eid(edge), face)?.map(|eu| hd(edge.b, K_EDGEUSE, eu.index())))
    }
    fn edge_is_connected_to_face(&self, edge: H, face: H) -> crate::Result<bool> {
        let b = self.brep_r(edge)?;
        let face = (face.b == edge.b && face.kind == K_FACE).then(|| fid(face));
        ed::is_connected_to_face(b, eid(edge), face)
    }
    fn insert_curve(&mut self, curve: SmCurve) -> Option<H> {
        let i = u32::try_from(self.extra.len()).ok()?;
        let hh = curve_h(2, K_EXTRA, i, &curve);
        self.extra.push(curve);
        Some(hh)
    }
    fn vertex_common_edges(&self, v1: H, v2: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(v1)?;
        let other = (v2.b == v1.b && v2.kind == K_VERTEX).then(|| vid(v2));
        Ok(vx::common_edges(b, vid(v1), other, None)?
            .into_iter()
            .map(|e| hd(v1.b, K_EDGE, e.index()))
            .collect())
    }
    fn brep_point_3d_classify(
        &mut self,
        k: &Kernel,
        brep: H,
        point: Point3,
        tolerance: f64,
        boundary_intersections: bool,
    ) -> crate::Result<PointClassification<H>> {
        let operand = brep.b;
        let id = BrepId::from_index(u32::try_from(operand).map_err(|_| invalid("not an operand"))?);
        let pc = BrepQuery::new(BrepIdentity::arena(id)).point_3d_classify(
            k,
            self.brep_m(brep)?,
            point,
            tolerance,
            boundary_intersections,
        )?;
        pc.map_objects(&mut |e| {
            Self::entity_h(operand, e)
                .ok_or_else(|| invalid("classification object without a handle"))
        })
    }

    fn face_curve_on_classify(
        &self,
        k: &Kernel,
        face: H,
        do_point_classify: bool,
        cc: &mut CurveClassification<H>,
    ) -> crate::Result<bool> {
        let b = self.brep_r(face)?;
        let objs = Objs {
            two: self,
            b: face.b,
        };
        curve_on_classify(
            k,
            b,
            fid(face),
            &objs,
            do_point_classify,
            cc,
            false,
            false,
            true,
        )
    }

    /// `SmBrep::SqueezeEdge(edge, survivor)` of `brep`'s B-rep
    /// ([`crate::topology::brep::p3::squeeze_edge`]), notifying as the glue does.
    fn brep_squeeze_edge(
        &mut self,
        k: &mut Kernel,
        brep: H,
        edge: H,
        survivor: H,
    ) -> crate::Result<()> {
        if brep.kind != K_BREP || edge.b != brep.b || survivor.b != brep.b {
            return Err(invalid("edge or vertex is not in the B-rep"));
        }
        self.with_edit_env(brep, |b, env| {
            crate::topology::brep::p3::squeeze_edge(k, b, env, eid(edge), vid(survivor))
        })
    }
}

/// The B-rep walks over one operand's own entities (as `Brep`'s own impl, through the
/// handles); an unknown handle is an invalid-input error, as a null dereference upstream.
impl ProtoTopology<H> for ArenaPair<'_> {
    fn vertex_use_faces(&self, vertex: H) -> crate::Result<Vec<Option<H>>> {
        let b = self.brep_r(vertex)?;
        let mut out = Vec::new();
        for vu in vx::vertexuses(b, vid(vertex))? {
            let face = match crate::topology::vertexuse::faceuse(b, vu)? {
                Some(fu) => eu1::faceuse_face(b, fu)?,
                None => None,
            };
            out.push(face.map(|f| hd(vertex.b, K_FACE, f.index())));
        }
        Ok(out)
    }
    fn edge_use_faces(&self, edge: H) -> crate::Result<Vec<Option<H>>> {
        let b = self.brep_r(edge)?;
        let mut out = Vec::new();
        for eu in ed::edgeuses(b, eid(edge), None, None)? {
            out.push(eu1::face(b, eu)?.map(|f| hd(edge.b, K_FACE, f.index())));
        }
        Ok(out)
    }
    fn edge_start_vertex(&self, edge: H) -> crate::Result<Option<H>> {
        Ok(ed::start_vertex(self.brep_r(edge)?, eid(edge))?
            .map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn edge_other_vertex(&self, edge: H, vertex: Option<H>) -> crate::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        let v = vertex
            .filter(|v| v.b == edge.b && v.kind == K_VERTEX)
            .map(vid);
        Ok(ed::other_vertex(b, eid(edge), v)?.map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn vertex_point(&self, vertex: H) -> crate::Result<Point3> {
        vx::point(self.brep_r(vertex)?, vid(vertex))
    }
    fn edge_brep(&self, edge: H) -> crate::Result<Option<ObjectKey>> {
        Ok(eu1::edge_brep(self.brep_r(edge)?, eid(edge))?.map(Brep::key))
    }
    fn edge_faces(&self, edge: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(edge)?;
        Ok(ed::faces(b, eid(edge), None)?
            .into_iter()
            .map(|f| hd(edge.b, K_FACE, f.index()))
            .collect())
    }
}

impl IntersectorBrep<H> for ArenaPair<'_> {
    fn make_vertex_loop(&mut self, brep: H, face: H, point: Point3) -> crate::Result<(H, H)> {
        if brep.kind != K_BREP || face.kind != K_FACE || brep.b != face.b {
            return Err(invalid("vertex loop owner mismatch"));
        }
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("absent B-rep"))?;

        let mut env = crate::topology::edge::NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(if brep.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            key_of: &|e| self.keys.object(brep.b == 1, e),
        };
        let (loop_, vertex) = crate::topology::brep::p9::make_vertex_loop(
            b,
            fid(face),
            point,
            OrientType::Opposite,
            &mut env,
        )?;
        Ok((
            hd(brep.b, K_LOOP, loop_.index()),
            hd(brep.b, K_VERTEX, vertex.index()),
        ))
    }
    fn make_vertex_split_edge(
        &mut self,
        k: &mut Kernel,
        brep: H,
        edge: H,
        parameter: f64,
    ) -> crate::Result<(H, H, H)> {
        if brep.kind != K_BREP || edge.kind != K_EDGE || brep.b != edge.b {
            return Err(invalid("split edge owner mismatch"));
        }
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("absent B-rep"))?;

        let mut env = crate::topology::edge::NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(if brep.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            key_of: &|e| self.keys.object(brep.b == 1, e),
        };
        let out = crate::topology::brep::p9::make_vertex_split_edge(
            k,
            b,
            eid(edge),
            parameter,
            &mut env,
            &|e, kind| self.keys.curve(brep.b == 1, e, kind),
        )?;
        if !out.split {
            return Err(SolidError::sm_err("boolean::merge::p4::arena_pair::split"));
        }
        let [a, z] = out
            .edges
            .ok_or_else(|| invalid("successful split without edges"))?;
        let v = out
            .vertex
            .ok_or_else(|| invalid("successful split without vertex"))?;
        Ok((
            hd(brep.b, K_EDGE, a.index()),
            hd(brep.b, K_EDGE, z.index()),
            hd(brep.b, K_VERTEX, v.index()),
        ))
    }

    fn brep_edges(&self, brep: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(brep)?;
        Ok(bp1::edges(b, None)?
            .into_iter()
            .map(|e| hd(brep.b, K_EDGE, e.index()))
            .collect())
    }
    fn brep_vertices(&self, brep: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(brep)?;
        Ok(bp1::vertices(b, None)?
            .into_iter()
            .map(|v| hd(brep.b, K_VERTEX, v.index()))
            .collect())
    }
    fn face_calculate_bounding_box(
        &self,
        k: &Kernel,
        face: H,
        tight: bool,
    ) -> crate::Result<Extent3d> {
        let b = self.brep_r(face)?;
        crate::query::face::p1::calculate_bounding_box(k, b, fid(face), tight)
    }
    fn vertex_brep(&self, vertex: H) -> crate::Result<Option<H>> {
        let b = self.brep_r(vertex)?;
        Ok(vx::brep(b, vid(vertex))?.map(|_| brep_h(vertex.b)))
    }
    fn edge_brep_handle(&self, edge: H) -> crate::Result<Option<H>> {
        let b = self.brep_r(edge)?;
        Ok(ed::brep(b, eid(edge))?.map(|_| brep_h(edge.b)))
    }
    fn set_vertex_point(&mut self, _k: &Kernel, vertex: H, point: Point3) -> crate::Result<()> {
        let slot = usize::try_from(vertex.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("absent B-rep"))?;
        let keys = self.keys;
        let this = keys.object(vertex.b == 1, EntityRef::Vertex(vid(vertex)));
        vx::set_point(
            b,
            vid(vertex),
            point,
            self.records,
            self.attributes,
            Some(if vertex.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            this,
            &|vu| keys.object(vertex.b == 1, EntityRef::Vertexuse(vu)),
        )
    }
    fn set_topology_tolerance(
        &mut self,
        _k: &Kernel,
        topo: H,
        tolerance: f64,
        update_only_if_larger: bool,
        cascade_to_boundaries: bool,
    ) -> crate::Result<()> {
        let b = self.brep_m(topo)?;
        match topo.kind {
            K_VERTEX => vx::set_tolerance(
                b,
                vid(topo),
                tolerance,
                update_only_if_larger,
                cascade_to_boundaries,
            ),
            K_EDGE => ed::set_tolerance(
                b,
                eid(topo),
                tolerance,
                update_only_if_larger,
                cascade_to_boundaries,
            ),
            _ => Err(invalid("SetTolerance of a non-vertex, non-edge")),
        }
    }
    fn update_object_tolerance(
        &mut self,
        k: &Kernel,
        topo: H,
        this_gap: f64,
        this_tol: f64,
        check_gaps: bool,
    ) -> crate::Result<()> {
        let target = match topo.kind {
            K_VERTEX => TopologyToUpdate::Vertex(vid(topo)),
            K_EDGE => TopologyToUpdate::Edge(eid(topo)),
            K_FACE => TopologyToUpdate::Face(fid(topo)),
            _ => return Err(invalid("UpdateObjectTolerance of a non-topology object")),
        };
        let b = self.brep_m(topo)?;
        update_object_tolerance(k, b, Some(target), this_gap, this_tol, check_gaps)
    }
    fn vertex_is_shell_vertex(&self, vertex: H) -> crate::Result<bool> {
        vx::is_shell_vertex(self.brep_r(vertex)?, vid(vertex))
    }
    fn edge_is_wire(&self, edge: H) -> crate::Result<bool> {
        ed::is_wire(self.brep_r(edge)?, eid(edge))
    }
    fn edge_is_lamina(&self, edge: H) -> crate::Result<bool> {
        ed::is_lamina(self.brep_r(edge)?, eid(edge))
    }
    fn edge_vertex(&self, edge: H) -> crate::Result<Option<H>> {
        Ok(ed::vertex(self.brep_r(edge)?, eid(edge))?.map(|v| hd(edge.b, K_VERTEX, v.index())))
    }
    fn face_brep(&self, face: H) -> crate::Result<Option<H>> {
        if face.kind != K_FACE {
            return Err(invalid("not a face handle"));
        }
        let b = self.brep_r(face)?;
        Ok(crate::topology::face::p4::brep(b, fid(face))?.map(|_| brep_h(face.b)))
    }
    fn face_vertices(&self, face: H) -> crate::Result<Vec<H>> {
        if face.kind != K_FACE {
            return Err(invalid("not a face handle"));
        }
        let vs =
            crate::topology::face::p4::vertices(self.brep_r(face)?, fid(face), None, false, None)?;
        Ok(vs
            .into_iter()
            .map(|v| hd(face.b, K_VERTEX, v.index()))
            .collect())
    }
    fn face_edges(&self, face: H) -> crate::Result<Vec<H>> {
        if face.kind != K_FACE {
            return Err(invalid("not a face handle"));
        }
        let es =
            crate::topology::face::p4::edges(self.brep_r(face)?, fid(face), None, false, None)?;
        Ok(es
            .into_iter()
            .map(|e| hd(face.b, K_EDGE, e.index()))
            .collect())
    }
    fn edge_primary_edgeuse_orientation(&self, edge: H) -> crate::Result<Option<OrientType>> {
        let b = self.brep_r(edge)?;
        match eu1::primary_edgeuse(b, eid(edge))? {
            Some(eu) => Ok(Some(eu1::orientation(b, eu)?)),
            None => Ok(None),
        }
    }
    fn edge_bounding_box(&self, k: &Kernel, edge: H) -> crate::Result<Extent3d> {
        let b = self.brep_r(edge)?;
        let boxes = crate::query::edge::calculate_bounding_box(
            k,
            b,
            eid(edge),
            BoundingBoxRequest {
                normal: true,
                ..BoundingBoxRequest::default()
            },
            false,
        )?;
        boxes.normal.ok_or_else(|| invalid("no normal box"))
    }
    fn face_uv_domain_from_uv_trim_curves(
        &mut self,
        k: &Kernel,
        face: H,
    ) -> crate::Result<Extent2d> {
        if face.kind != K_FACE {
            return Err(invalid("not a face handle"));
        }
        let b = self.brep_m(face)?;
        Ok(
            crate::topology::face::p1::calculate_uv_domain_from_uv_trim_curves(
                k,
                b,
                fid(face),
                true,
                false,
                false,
            )?
            .domain,
        )
    }
    fn face_point_classify(
        &mut self,
        k: &Kernel,
        face: H,
        uv: Point2,
        pc: &mut PointClassification<H>,
    ) -> crate::Result<()> {
        // (mutable: the classification may create UV trim curves, as upstream)
        let b = self.brep_m(face)?;
        let mut pe: PointClassification<EntityRef> = PointClassification::new(pc.tolerance());
        let r = point_classify(
            k,
            b,
            fid(face),
            uv,
            &mut pe,
            PointClassifyOptions::default(),
        );
        // copy the classification back whatever the status (upstream's out-param)
        pc.set_class_object(
            pe.point_class(),
            pe.object().and_then(|e| Self::entity_h(face.b, e)),
        );
        pc.set_face(pe.face().and_then(|e| Self::entity_h(face.b, e)));
        pc.set_gap_3d(pe.deviation());
        pc.set_pre_snap_param(pe.pre_snap_param());
        if pe.are_parameters_set() {
            pc.set_t_param(pe.t_param(), false);
            pc.set_uv_param(pe.uv_param());
        } else {
            pc.unset_parameters();
        }
        r
    }
    fn face_intersect(
        &mut self,
        k: &Kernel,
        face1: H,
        face2: H,
        approx_tol_3d: f64,
        ang_tol_deg: f64,
        opt_coincidence_flag: Option<bool>,
    ) -> crate::Result<ti::FaceIntersection<H>> {
        if face1.kind != K_FACE || face2.kind != K_FACE {
            return Err(invalid("FaceIntersect needs two faces"));
        }
        if face1.b != face2.b {
            // UNPORTED(topology.face_p3): SmFace::FaceIntersect of faces of two B-reps --
            // justified: topology::face::p3::face_intersect takes one B-rep arena
            return Err(SolidError::unported(
                "SmFace::FaceIntersect (faces of two B-reps)",
                "topology.face_p3",
            ));
        }
        let options = FaceIntersectOptions {
            want_this_classes: true,
            want_other_classes: true,
            want_cc_curves: true,
            coincidence: opt_coincidence_flag,
            ..FaceIntersectOptions::default()
        };
        let out = fp3::face_intersect(
            k,
            self.brep_r(face1)?,
            fid(face1),
            fid(face2),
            approx_tol_3d,
            ang_tol_deg,
            &options,
        )?;
        let bi = face1.b;

        // every curve the call made gets a handle in the curve store
        let mut curves_3d = Vec::new();
        for c in out.curves_3d {
            curves_3d.push(self.register_curve(c)?);
        }
        let cc3d: Vec<Option<H>> = self.register_curves(
            out.cc_curves_3d
                .map(|v| v.into_iter().map(Some).collect())
                .unwrap_or_default(),
        )?;
        let uv1 = self.register_curves(out.cc_this_uv_curves.unwrap_or_default())?;
        let uv2 = self.register_curves(out.cc_other_uv_curves.unwrap_or_default())?;

        // the classifications name their objects by handle
        let mut map = |o: IntersectObject| -> crate::Result<H> {
            let bad = || invalid("classification object without a handle");
            match o {
                IntersectObject::Curve(i) => cc3d.get(i).copied().flatten().ok_or_else(bad),
                IntersectObject::UvCurve(i, false) => uv1.get(i).copied().flatten().ok_or_else(bad),
                IntersectObject::UvCurve(i, true) => uv2.get(i).copied().flatten().ok_or_else(bad),
                IntersectObject::Surface(f) => Ok(hd(bi, K_FACE_SURFACE, f.index())),
                IntersectObject::EdgeCurve(e) => self
                    .edge_curve_h(hd(bi, K_EDGE, e.index()))
                    .flatten()
                    .ok_or_else(bad),
                IntersectObject::Trim(eu) => Ok(hd(bi, K_TRIM_CURVE, eu.index())),
                IntersectObject::Topology(e) => match e {
                    EntityRef::Vertexuse(vu) => Ok(hd(bi, K_VERTEXUSE, vu.index())),
                    other => Self::entity_h(bi, other).ok_or_else(bad),
                },
            }
        };
        let mut cc1 = Vec::new();
        for c in out.this_classes.unwrap_or_default() {
            cc1.push(c.map_objects(&mut map)?);
        }
        let mut cc2 = Vec::new();
        for c in out.other_classes.unwrap_or_default() {
            cc2.push(c.map_objects(&mut map)?);
        }
        Ok(ti::FaceIntersection {
            curves_3d,
            cc1,
            cc2,
            cc_curves: [
                cc3d.into_iter().flatten().collect(),
                uv1.into_iter().flatten().collect(),
                uv2.into_iter().flatten().collect(),
            ],
        })
    }
    fn editing_enabled(&self, brep: H) -> crate::Result<bool> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        self.breps
            .get(slot)
            .and_then(Option::as_ref)
            .map(|b| b.editing_enabled())
            .ok_or_else(|| invalid("not a B-rep handle"))
    }
    fn brep_tolerance(&self, brep: H) -> crate::Result<f64> {
        Ok(self.brep_r(brep)?.tolerance())
    }
    fn set_editing_enabled(&mut self, brep: H, enabled: bool) -> crate::Result<bool> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("not a B-rep handle"))?;
        let old = b.editing_enabled();
        b.set_editing_enabled(enabled);
        Ok(old)
    }
    fn topology_objects(&self) -> &dyn TopologyObjects<H> {
        self
    }
    fn brep_arena(&self, brep: H) -> crate::Result<&Brep> {
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        self.brep_r(brep)
    }
    fn brep_glue_edges_general(
        &mut self,
        k: &mut Kernel,
        brep: H,
        surviving: Option<H>,
        relative_orientation: OrientType,
        edge_gap: f64,
        do_region_nesting: bool,
        to_delete: Option<H>,
        inside_faceuses: Option<&[H]>,
    ) -> crate::Result<()> {
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let edge = |h: Option<H>| -> crate::Result<Option<EdgeId>> {
            match h {
                None => Ok(None),
                Some(h) if h.kind == K_EDGE && h.b == brep.b => Ok(Some(eid(h))),
                Some(_) => Err(invalid("not an edge of this B-rep")),
            }
        };
        let (surviving, to_delete) = (edge(surviving)?, edge(to_delete)?);
        // (no face-use handles exist here: the only caller passes NULL)
        let inside: Option<&[crate::topology::FaceuseId]> = match inside_faceuses {
            None => None,
            Some([]) => Some(&[]),
            Some(_) => return Err(invalid("face-use handles are not modelled")),
        };
        let breps = &mut self.breps;
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;

        let mut notify = ed::NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(if brep.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            key_of: &|e| self.keys.object(brep.b == 1, e),
        };
        p7::glue_edges_general(
            k,
            b,
            surviving,
            relative_orientation,
            edge_gap,
            do_region_nesting,
            to_delete,
            inside,
            &mut notify,
        )
    }

    /// `SmBrep::DeleteFace(pFace)` with its defaults (`FALSE, TRUE, NULL, NULL`):
    /// [`crate::topology::brep::p3::delete_face`], notifying as the glue does.
    fn brep_delete_face(&mut self, k: &mut Kernel, brep: H, face: H) -> crate::Result<()> {
        if brep.kind != K_BREP || face.kind != K_FACE || face.b != brep.b {
            return Err(invalid("face is not in the B-rep"));
        }
        self.with_edit_env(brep, |b, env| {
            let mut marks = crate::topology::topology_traverser::Marks::default();
            crate::topology::brep::p3::delete_face(
                k,
                b,
                env,
                &mut marks,
                fid(face),
                crate::topology::brep::p3::DeleteFaceOptions::default(),
            )
            .map(drop)
        })
    }
}

impl ArenaPair<'_> {
    /// Runs the edit `f` of `brep`'s B-rep with the adapter's records and attributes,
    /// notifying through that B-rep's context under its notification keys (as
    /// `brep_glue_edges_general`).
    fn with_edit_env<R>(
        &mut self,
        brep: H,
        f: impl FnOnce(&mut Brep, &mut ed::NotifyEnv<'_>) -> crate::Result<R>,
    ) -> crate::Result<R> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let mut env = ed::NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(if brep.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            key_of: &|e| self.keys.object(brep.b == 1, e),
        };
        f(b, &mut env)
    }
}

impl TopologyObjects<H> for ArenaPair<'_> {
    /// The B-rep's cache (`SmBrep::GetBrepCache`), built from the B-rep on first use (the
    /// C++ topology solver builds it on demand) with this adapter's handles and the
    /// arena identities. Under caller notification keys it is not built yet (a dispatch
    /// gap of this adapter: no kernel, caller identities).
    fn brep_cache(&self, brep: H) -> crate::Result<&BrepCache<H>> {
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let cell = self.caches.get(slot).ok_or_else(|| invalid("bad B-rep"))?;
        if let Some(cache) = cell.get() {
            return Ok(cache);
        }
        let b = self
            .breps
            .get(slot)
            .and_then(Option::as_deref)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let bi = brep.b;
        let mut cache = BrepCache::new(BREP_TYPE)?;
        cache.build_trees(
            self.cache_kernels
                .as_ref()
                .ok_or_else(|| {
                    // DISPATCH GAP: SmBrep::CalculateBoundingBox (BuildTrees' first
                    // call) is ported (`query::brep::calculate_bounding_box`, query.brep
                    // partial), but under caller notification keys this adapter has no
                    // kernel to evaluate it with, and the cache would need the caller's
                    // geometry identities instead of `arena_identity` (the tree indexes
                    // only owned geometry).
                    SolidError::unported(
                        "SmBrep::GetBrepCache under caller notification keys",
                        "boolean.merge_p4",
                    )
                })?
                .get(slot)
                .ok_or_else(|| invalid("bad B-rep cache context"))?,
            CacheSource::Brep(b),
            |o| match o {
                CacheObject::Vertex(v) => hd(bi, K_VERTEX, v.index()),
                CacheObject::Edge(e) => hd(bi, K_EDGE, e.index()),
                CacheObject::Face(f) => hd(bi, K_FACE, f.index()),
                CacheObject::Curve(e) => b
                    .edge(e)
                    .ok()
                    .and_then(|x| x.curve.as_ref())
                    .map_or(hd(bi, K_EDGE_CURVE, e.index()), |c| {
                        curve_h(bi, K_EDGE_CURVE, e.index(), c)
                    }),
                CacheObject::Surface(f) => hd(bi, K_FACE_SURFACE, f.index()),
                CacheObject::Brep => brep_h(bi),
                _ => hd(bi, K_NULL, 0),
            },
            &arena_identity,
        )?;
        let _ = cell.set(cache);
        cell.get().ok_or_else(|| invalid("B-rep cache not stored"))
    }
    fn curve(&self, object: H) -> crate::Result<&SmCurve> {
        ClassGeometry::curve(self, object).ok_or_else(|| invalid("not a curve handle"))
    }
    fn analytic_curve_handle(&self, _curve: H) -> Option<H> {
        None
    }
    fn vertex_point(&self, vertex: H) -> crate::Result<Point3> {
        vx::point(self.brep_r(vertex)?, vid(vertex))
    }
    fn vertex_tolerance(&self, vertex: H) -> crate::Result<f64> {
        vx::tolerance(self.brep_r(vertex)?, vid(vertex))
    }
    fn curve_edge(&self, curve: H) -> crate::Result<Option<H>> {
        Ok((curve.kind == K_EDGE_CURVE).then_some(hd(curve.b, K_EDGE, curve.i)))
    }
    fn edge_curve(&self, edge: H) -> crate::Result<Option<H>> {
        self.edge_curve_h(edge)
            .ok_or_else(|| invalid("not an edge handle"))
    }
    fn edge_interval(&self, edge: H) -> crate::Result<Extent1d> {
        ed::interval(self.brep_r(edge)?, eid(edge))
    }
    fn surface_face(&self, surface: H) -> crate::Result<Option<H>> {
        Ok((surface.kind == K_FACE_SURFACE).then_some(hd(surface.b, K_FACE, surface.i)))
    }
    fn face_surface(&self, face: H) -> crate::Result<H> {
        self.face_surface_h(face)
            .flatten()
            .ok_or_else(|| SolidError::null_pointer("boolean::merge::p4::arena_pair::face_surface"))
    }
    fn surface(&self, surface: H) -> crate::Result<&Surface> {
        ClassGeometry::surface(self, surface).ok_or_else(|| invalid("not a surface handle"))
    }
    fn face_uv_domain(&self, face: H) -> crate::Result<Extent2d> {
        fp2::uv_domain(self.brep_r(face)?, fid(face))
    }
    fn face_tolerance(&self, face: H) -> crate::Result<f64> {
        fp2::tolerance(self.brep_r(face)?, fid(face))
    }
    fn surface_owner(&self, surface: H) -> crate::Result<Option<H>> {
        self.surface_face(surface)
    }
    fn shape_member(&self, _object: H) -> ShapeMember {
        ShapeMember::Other
    }
}

impl IntersectorBrepP2<H> for ArenaPair<'_> {
    fn brep_faces(&self, brep: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(brep)?;
        Ok(bp1::faces(b, None)?
            .into_iter()
            .map(|f| hd(brep.b, K_FACE, f.index()))
            .collect())
    }
    fn brep_combine_coincident_vertices(
        &mut self,
        k: &mut Kernel,
        brep: H,
        surviving: H,
        to_delete: H,
    ) -> crate::Result<Option<H>> {
        if brep.kind != K_BREP {
            return Err(invalid("not a B-rep handle"));
        }
        let vertex = |h: H| -> crate::Result<VertexId> {
            if h.kind == K_VERTEX && h.b == brep.b {
                Ok(vid(h))
            } else {
                Err(invalid("not a vertex of this B-rep"))
            }
        };
        let (surviving, to_delete) = (vertex(surviving)?, vertex(to_delete)?);
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let breps = &mut self.breps;
        let b = breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let mut notify = ed::NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(if brep.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            key_of: &|e| self.keys.object(brep.b == 1, e),
        };
        let mut deleted_edge = None;
        crate::topology::brep::p5::combine_with_notifications(
            k,
            b,
            surviving,
            to_delete,
            &mut deleted_edge,
            Some(&mut notify),
        )?;
        Ok(deleted_edge.map(|e| hd(brep.b, K_EDGE, e.index())))
    }
    fn edge_normalized_evaluate(&mut self, k: &Kernel, edge: H, t: f64) -> crate::Result<Point3> {
        if edge.kind != K_EDGE {
            return Err(invalid("not an edge handle"));
        }
        let b = self.brep_m(edge)?;
        let eu = eu1::primary_edgeuse(b, eid(edge))?
            .ok_or_else(|| invalid("edge without primary edge use"))?;
        // (pOptFirstDeriv NULL, bOKToMakeUVTrimCurve TRUE by default)
        Ok(eu3::normalized_evaluate(k, b, eu, t, false, false, true)?.point)
    }
    fn vertex_edges(&self, vertex: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(vertex)?;
        Ok(vx::edges(b, vid(vertex), None)?
            .into_iter()
            .map(|e| hd(vertex.b, K_EDGE, e.index()))
            .collect())
    }
    fn vertex_faces(&self, vertex: H) -> crate::Result<Vec<H>> {
        let b = self.brep_r(vertex)?;
        Ok(vx::faces(b, vid(vertex))?
            .into_iter()
            .map(|f| hd(vertex.b, K_FACE, f.index()))
            .collect())
    }
}

impl crate::topology::proto_topology::p3::BrepGlue<H> for ArenaPair<'_> {
    /// `SmBrep::GlueVertices` of `brep` (`topology::brep::p5::glue_with_notifications`),
    /// notifying through the adapter's records and attributes as
    /// [`IntersectorBrep::brep_glue_edges_general`].
    fn glue_vertices(
        &mut self,
        k: &mut Kernel,
        brep: H,
        v1: H,
        v2: H,
        opt_wire_edges_in_brep: Option<&[H]>,
    ) -> crate::Result<()> {
        if brep.kind != K_BREP
            || v1.kind != K_VERTEX
            || v2.kind != K_VERTEX
            || v1.b != brep.b
            || v2.b != brep.b
        {
            return Err(invalid("glue vertex owner mismatch"));
        }
        let wires = opt_wire_edges_in_brep
            .map(|edges| {
                edges
                    .iter()
                    .map(|&e| {
                        if e.kind == K_EDGE && e.b == brep.b {
                            Ok(eid(e))
                        } else {
                            Err(invalid("glue wire edge owner mismatch"))
                        }
                    })
                    .collect::<crate::Result<Vec<_>>>()
            })
            .transpose()?;
        let slot = usize::try_from(brep.b).map_err(|_| invalid("bad B-rep"))?;
        let b = self
            .breps
            .get_mut(slot)
            .and_then(Option::as_deref_mut)
            .ok_or_else(|| invalid("handle names an absent B-rep"))?;
        let mut notify = ed::NotifyEnv {
            records: self.records,
            attributes: self.attributes,
            context: Some(if brep.b == 1 {
                self.other_context
                    .as_deref_mut()
                    .unwrap_or(self.primary_context)
            } else {
                self.primary_context
            }),
            key_of: &|e| self.keys.object(brep.b == 1, e),
        };
        crate::topology::brep::p5::glue_with_notifications(
            k,
            b,
            Some(vid(v1)),
            Some(vid(v2)),
            wires.as_deref(),
            Some(&mut notify),
        )
    }
}

pub(super) fn encode(entity: EntityRef, other: bool) -> H {
    let (kind, i) = match entity {
        EntityRef::Face(f) => (K_FACE, f.index()),
        EntityRef::Edge(e) => (K_EDGE, e.index()),
        EntityRef::Vertex(v) => (K_VERTEX, v.index()),
        _ => (K_NULL, 0),
    };
    hd(i8::from(other), kind, i)
}
pub(super) fn decode(h: H) -> EntityRef {
    match h.kind {
        K_FACE => EntityRef::Face(fid(h)),
        K_EDGE => EntityRef::Edge(eid(h)),
        K_VERTEX => EntityRef::Vertex(vid(h)),
        _ => EntityRef::Brep(BrepId::from_index(h.i)),
    }
}

pub(super) fn surface_handle(other: bool, face: FaceId) -> H {
    hd(i8::from(other), K_FACE_SURFACE, face.index())
}

// A nonempty subset whose external identities match no surface must still
// exclude every actual surface. Operand 2 has no B-rep surfaces in this adapter.
pub(super) fn unmatched_surface() -> H {
    hd(2, K_FACE_SURFACE, 0)
}

pub(super) fn deleted_vertex(h: H) -> crate::Result<VertexId> {
    if h.b == 0 && h.kind == K_VERTEX {
        Ok(vid(h))
    } else {
        Err(invalid("deleted vertex is not a primary vertex"))
    }
}
pub(super) fn deleted_edge(h: H) -> crate::Result<EdgeId> {
    if h.b == 0 && h.kind == K_EDGE {
        Ok(eid(h))
    } else {
        Err(invalid("deleted edge is not a primary edge"))
    }
}
