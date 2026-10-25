//! Topology-solver geometry access over the retained insertion objects.
//!
//! Cache trees need a body-qualified storage authority, not raw arena IDs. Their
//! existing unsupported default remains until that authority is supplied.

use super::{
    invalid, ClassGeometry, Curve, EntityRef, FilletObject, InsertionGeometry, Result, Surface,
};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::object::ShapeMember;
use crate::intersect::topology_solver::p1::TopologyObjects;
use crate::math::Point3;
use crate::topology::{Brep, FaceId};
use crate::SolidError;

impl TopologyObjects<FilletObject> for InsertionGeometry<'_> {
    fn curve(&self, object: FilletObject) -> Result<&Curve> {
        ClassGeometry::curve(self, object).ok_or_else(|| invalid("not an actual Curve handle"))
    }
    fn analytic_curve_handle(&self, _curve: FilletObject) -> Option<FilletObject> {
        // No independent replacement Curve is installed in the operation's store.
        None
    }
    fn vertex_point(&self, vertex: FilletObject) -> Result<Point3> {
        ClassGeometry::point(self, vertex).ok_or_else(|| invalid("not an actual Vertex handle"))
    }
    fn vertex_tolerance(&self, vertex: FilletObject) -> Result<f64> {
        let (role, EntityRef::Vertex(vertex)) =
            Self::topology(vertex).ok_or_else(|| invalid("unknown Vertex role"))?
        else {
            return Err(invalid("handle is not a Vertex"));
        };
        crate::topology::vertex::tolerance(self.body(role), vertex)
    }
    fn curve_edge(&self, object: FilletObject) -> Result<Option<FilletObject>> {
        let curve = TopologyObjects::curve(self, object)?;
        let Some(owner) = curve.owner() else {
            return Ok(None);
        };
        match self.owner_binding(owner) {
            Some((role, entity @ EntityRef::Edge(_))) => Ok(Some(Self::handle(role, entity))),
            _ => Err(SolidError::unported(
                "SmCurve::GetEdge (insertion owner chain)",
                "blend.fillet_executive_p1",
            )),
        }
    }
    fn edge_curve(&self, edge: FilletObject) -> Result<Option<FilletObject>> {
        InsertionGeometry::edge_curve(self, edge)
            .ok_or_else(|| invalid("not an actual Edge handle"))
    }
    fn edge_interval(&self, edge: FilletObject) -> Result<Extent1d> {
        InsertionGeometry::edge_interval(self, edge)
            .ok_or_else(|| invalid("not an actual Edge interval"))
    }
    fn edge_tolerance(&self, edge: FilletObject) -> Result<f64> {
        let (role, EntityRef::Edge(edge)) =
            Self::topology(edge).ok_or_else(|| invalid("unknown Edge role"))?
        else {
            return Err(invalid("handle is not an Edge"));
        };
        crate::topology::edge::tolerance(self.body(role), edge)
    }
    fn surface_face(&self, surface: FilletObject) -> Result<Option<FilletObject>> {
        if let FilletObject::Surface(binding) = surface {
            return InsertionGeometry::surface_face(self, binding);
        }
        let surface = TopologyObjects::surface(self, surface)?;
        let Some(owner) = surface.owner() else {
            return Ok(None);
        };
        match self.owner_binding(owner) {
            Some((role, entity @ EntityRef::Face(_))) => Ok(Some(Self::handle(role, entity))),
            _ => Err(SolidError::unported(
                "SmSurface::GetFace (insertion owner chain)",
                "blend.fillet_executive_p1",
            )),
        }
    }
    fn face_surface(&self, face: FilletObject) -> Result<FilletObject> {
        InsertionGeometry::face_surface(self, face)
            .ok_or_else(|| invalid("not an actual Face handle"))?
            .ok_or_else(|| SolidError::null_pointer(super::OP))
    }
    fn surface(&self, object: FilletObject) -> Result<&Surface> {
        if let FilletObject::Surface(binding) = object {
            return self.bound_surface(binding);
        }
        ClassGeometry::surface(self, object).ok_or_else(|| invalid("not an actual Surface handle"))
    }
    fn face_uv_domain(&self, face: FilletObject) -> Result<Extent2d> {
        InsertionGeometry::face_uv_domain(self, face)
            .ok_or_else(|| invalid("not an actual Face domain"))
    }
    fn face_tolerance(&self, face: FilletObject) -> Result<f64> {
        let (body, face) = TopologyObjects::face_entity(self, face)
            .ok_or_else(|| invalid("not an actual Face handle"))?;
        crate::topology::face::p2::tolerance(body, face)
    }
    fn face_entity(&self, face: FilletObject) -> Option<(&Brep, FaceId)> {
        let (role, EntityRef::Face(face)) = Self::topology(face)? else {
            return None;
        };
        let body = self.body(role);
        body.face(face).ok()?;
        Some((body, face))
    }
    fn surface_owner(&self, object: FilletObject) -> Result<Option<FilletObject>> {
        let surface = TopologyObjects::surface(self, object)?;
        let Some(owner) = surface.owner() else {
            return Ok(None);
        };
        self.owner_binding(owner)
            .map(|(role, entity)| Some(Self::handle(role, entity)))
            .ok_or_else(|| {
                SolidError::unported(
                    "SmSurface::GetOwner (insertion owner chain)",
                    "blend.fillet_executive_p1",
                )
            })
    }
    fn object_zone_tol_3d(&self, k: &crate::Kernel, object: FilletObject) -> Result<f64> {
        if let Some((role, EntityRef::Brep(_))) = Self::topology(object) {
            return Ok(self.body(role).tolerance());
        }
        crate::intersect::point_class::object_zone_tol_3d(k, self, Some(object))
    }
    fn shape_member(&self, _object: FilletObject) -> ShapeMember {
        // ShapeMember has no body role. No body-qualified tree is installed yet.
        ShapeMember::Other
    }
}
