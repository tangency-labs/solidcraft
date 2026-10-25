//! Corner insertion on the same retained intersector and bodies as geometry insertion.

use super::geometry::SurfaceBinding;
use super::live::{LiveCurveBinding, LiveGeometryInsertion};
use super::FaceMarker;
use crate::blend::fillet_corner::p1::CornerInsertionContext;
use crate::blend::fillet_geom::p1::{FilletBrep, FilletCornerId};
use crate::blend::fillet_geom::p4::IntersectionTopologyContext;
use crate::error::{Result, SolidError};
use crate::surface::surface::Surface;
use crate::topology::{edge, Brep, EdgeId, FaceId};

impl LiveGeometryInsertion<'_> {
    pub(super) fn insert_corner(
        &mut self,
        pseudo: &mut FilletBrep,
        target: &mut Brep,
        id: FilletCornerId,
    ) -> Result<()> {
        let corners = self.corners;
        let index =
            usize::try_from(id.0).map_err(|_| super::super::invalid("corner index overflow"))?;
        let corner = corners
            .get(index)
            .ok_or_else(|| super::super::invalid("actual insertion corner absent"))?;
        corner.insert_intersection_topology_with_context(pseudo, target, self)
    }
}

impl CornerInsertionContext for LiveGeometryInsertion<'_> {
    type CurveBinding = LiveCurveBinding;
    type SurfaceBinding = SurfaceBinding;
    type FaceMarker = FaceMarker;

    fn begin_corner(&mut self) -> Result<()> {
        // The actual executive intersector is already borrowed by this context.
        Ok(())
    }
    fn capture_managed_curve(&mut self, edge: EdgeId) -> Result<Option<LiveCurveBinding>> {
        LiveGeometryInsertion::capture_managed_curve(self, edge)
    }
    fn capture_pseudo_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        edge: EdgeId,
    ) -> Result<Option<LiveCurveBinding>> {
        LiveGeometryInsertion::capture_pseudo_curve(self, pseudo, edge)
    }
    fn attribute_edges(&self) -> Result<Vec<EdgeId>> {
        IntersectionTopologyContext::attribute_edges(self)
    }
    fn managed_edge_faces(&self, edge: EdgeId) -> Result<Vec<FaceId>> {
        edge::faces(self.intermediate, edge, None)
    }
    fn with_corner_face_scope<T>(
        &mut self,
        target: &mut Brep,
        face: FaceId,
        body: impl FnOnce(&mut Self, &mut Brep, &FaceMarker) -> Result<T>,
    ) -> Result<T> {
        self.marker_scope(target, face, 780, false, body)
    }
    fn merge_corner_curve(
        &mut self,
        pseudo: &mut FilletBrep,
        target: &mut Brep,
        face: FaceId,
        curve: &Option<LiveCurveBinding>,
        managed_edge: EdgeId,
    ) -> Result<()> {
        LiveGeometryInsertion::merge_corner_curve(
            self,
            pseudo,
            target,
            face,
            curve.as_ref(),
            managed_edge,
        )
    }
    fn update_corner_topology(
        &mut self,
        pseudo: &mut FilletBrep,
        target: &mut Brep,
        marker: &FaceMarker,
        face: FaceId,
    ) -> Result<()> {
        self.update_changes(pseudo, target, marker, face)
    }
    fn capture_managed_surface(&mut self, face: FaceId) -> Result<Option<SurfaceBinding>> {
        LiveGeometryInsertion::capture_managed_surface(self, face)
    }
    fn register_bound_intersection(
        &mut self,
        target: &mut Brep,
        face: FaceId,
        surface: &Option<SurfaceBinding>,
    ) -> Result<()> {
        LiveGeometryInsertion::register_bound_intersection(self, target, face, surface.as_ref())
    }
    fn register_blending_intersection(
        &mut self,
        _target: &mut Brep,
        _face: FaceId,
        _surface: &Surface,
    ) -> Result<()> {
        // UNPORTED(blend.fillet_executive_p1): actual retained corner-patch Surface identity for RegisterExistingSSI
        Err(SolidError::unported(
            "SmTopologyIntersector::RegisterExistingSSI (retained corner patch Surface)",
            "blend.fillet_executive_p1",
        ))
    }
}
