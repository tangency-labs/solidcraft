//! Borrowed target/pseudo geometry for the executive's attached rail classifications.

use crate::blend::fillet_classification::{FilletBody, FilletClassObject};
use crate::curve::curve::Curve;
use crate::error::EntityRef;
use crate::geom::object::ObjectRef;
use crate::intersect::point_class::ClassGeometry;
use crate::math::Point3;
use crate::query::face::p2::CurveOnClassifyObjects;
use crate::surface::surface::Surface;
use crate::surface::volume::Volume;
use crate::topology::{Brep, BrepId, EdgeId, EdgeuseId, FaceId, LoopId, VertexId, VertexuseId};

/// Each body is borrowed from the same live session during the classification loan.
/// No indexed/legacy handles are accepted by this production provider.
pub(super) struct Objects<'a> {
    pub target: &'a Brep,
    pub pseudo: &'a Brep,
}

impl Objects<'_> {
    fn body(&self, body: FilletBody) -> &Brep {
        match body {
            FilletBody::Target => self.target,
            FilletBody::Pseudo => self.pseudo,
        }
    }

    fn body_of(&self, brep: &Brep) -> Option<FilletBody> {
        if std::ptr::eq(brep, self.target) {
            Some(FilletBody::Target)
        } else if std::ptr::eq(brep, self.pseudo) {
            Some(FilletBody::Pseudo)
        } else {
            None
        }
    }
}

impl ClassGeometry<FilletClassObject> for Objects<'_> {
    fn curve(&self, object: FilletClassObject) -> Option<&Curve> {
        match object {
            FilletClassObject::Curve(body, edge) => self.body(body).edge(edge).ok()?.curve.as_ref(),
            FilletClassObject::TrimCurve(body, eu) => {
                self.body(body).edgeuse(eu).ok()?.uv_trim_curve.as_ref()
            }
            _ => None,
        }
    }
    fn surface(&self, object: FilletClassObject) -> Option<&Surface> {
        let FilletClassObject::Surface(body, face) = object else {
            return None;
        };
        self.body(body).face(face).ok()?.surface.as_ref()
    }
    fn volume(&self, _: FilletClassObject) -> Option<&Volume> {
        None
    }
    fn point(&self, object: FilletClassObject) -> Option<Point3> {
        let FilletClassObject::Topology(body, EntityRef::Vertex(vertex)) = object else {
            return None;
        };
        Some(self.body(body).vertex(vertex).ok()?.point)
    }
    fn classification_vertex_point(&self, object: FilletClassObject) -> Option<Point3> {
        self.point(object)
    }
    fn topology_zone_tol_3d(&self, object: FilletClassObject) -> Option<f64> {
        let FilletClassObject::Topology(body, entity) = object else {
            return None;
        };
        let brep = self.body(body);
        match entity {
            EntityRef::Vertex(id) => Some(brep.vertex(id).ok()?.tolerance),
            EntityRef::Edge(id) => Some(brep.edge(id).ok()?.tolerance),
            EntityRef::Face(id) => Some(brep.face(id).ok()?.tolerance),
            EntityRef::Edgeuse(id) => Some(brep.edge(brep.edgeuse(id).ok()?.edge?).ok()?.tolerance),
            _ => None,
        }
    }
    fn topology_entity(&self, object: FilletClassObject) -> Option<(&Brep, EntityRef)> {
        let FilletClassObject::Topology(body, entity) = object else {
            return None;
        };
        Some((self.body(body), entity))
    }
    fn topology_handle(&self, entity: EntityRef) -> Option<FilletClassObject> {
        Some(FilletClassObject::Topology(FilletBody::Target, entity))
    }
    fn topology_handle_in(&self, brep: &Brep, entity: EntityRef) -> Option<FilletClassObject> {
        Some(FilletClassObject::Topology(self.body_of(brep)?, entity))
    }
    fn brep_id(&self, brep: &Brep) -> Option<BrepId> {
        self.body_of(brep)?;
        // These are the actual distinct serials issued by the kernel constructor,
        // not local topology indices or synthesized offsets between body arenas.
        Some(BrepId::from_index(
            u32::try_from(brep.notify_serial()?).ok()?,
        ))
    }
    fn owner_entity(&self, owner: ObjectRef) -> Option<(&Brep, EntityRef)> {
        for brep in [self.target, self.pseudo] {
            let Some(serial) = brep.notify_serial() else {
                continue;
            };
            let Some(entity) = crate::topology::brep::p3::topology_entity(serial, owner.key) else {
                continue;
            };
            if crate::topology::brep::p3::topology_ref(serial, entity) == owner {
                return Some((brep, entity));
            }
        }
        None
    }
}

impl CurveOnClassifyObjects<FilletClassObject> for Objects<'_> {
    fn face(&self, id: FaceId) -> FilletClassObject {
        FilletClassObject::Topology(FilletBody::Target, EntityRef::Face(id))
    }
    fn vertex(&self, id: VertexId) -> FilletClassObject {
        FilletClassObject::Topology(FilletBody::Target, EntityRef::Vertex(id))
    }
    fn edge(&self, id: EdgeId) -> FilletClassObject {
        FilletClassObject::Topology(FilletBody::Target, EntityRef::Edge(id))
    }
    fn vertexuse(&self, id: VertexuseId) -> FilletClassObject {
        FilletClassObject::Topology(FilletBody::Target, EntityRef::Vertexuse(id))
    }
    fn edgeuse(&self, id: EdgeuseId) -> FilletClassObject {
        FilletClassObject::Topology(FilletBody::Target, EntityRef::Edgeuse(id))
    }
    fn edge_curve(&self, id: EdgeId) -> FilletClassObject {
        FilletClassObject::Curve(FilletBody::Target, id)
    }
    fn trim_curve(&self, id: EdgeuseId) -> FilletClassObject {
        FilletClassObject::TrimCurve(FilletBody::Target, id)
    }
    fn face_surface(&self, id: FaceId) -> Option<FilletClassObject> {
        self.target.face(id).ok()?.surface.as_ref()?;
        Some(FilletClassObject::Surface(FilletBody::Target, id))
    }
    fn loop_id(&self, object: FilletClassObject) -> Option<LoopId> {
        match object {
            FilletClassObject::Topology(FilletBody::Target, EntityRef::Loop(id)) => Some(id),
            _ => None,
        }
    }
}

/// Live retained corner resolution; geometry borrows exist only inside `classify`.
pub(super) struct Context<'a> {
    pub corners: &'a [crate::blend::fillet_corner::p3::FilletCorner],
}

impl crate::blend::fillet_solver::p5::RailClassificationContext for Context<'_> {
    fn rail_curve(&self, edge: EdgeId) -> FilletClassObject {
        FilletClassObject::Curve(FilletBody::Pseudo, edge)
    }
    fn rail_mate_uv_curve(&self, pseudo: &Brep, edgeuse: EdgeuseId) -> Option<FilletClassObject> {
        pseudo.edgeuse(edgeuse).ok()?.uv_trim_curve.as_ref()?;
        Some(FilletClassObject::TrimCurve(FilletBody::Pseudo, edgeuse))
    }
    fn classify(
        &mut self,
        k: &crate::kernel::Kernel,
        target: &mut Brep,
        pseudo: &Brep,
        face: FaceId,
        cc: &mut crate::intersect::curve_class::p1::CurveClassification<FilletClassObject>,
    ) -> crate::error::Result<()> {
        let mut cache = crate::query::face::p2::CurveOnClassifyCache::default();
        let result = {
            let objects = Objects { target, pseudo };
            crate::query::face::p2::curve_on_classify_cached(
                k, target, face, &objects, true, cc, true, false, true, &mut cache,
            )
        };
        // Native const queries retain their lazy gap/UV edits, including mutations
        // made before a failed classification. Use the shared query synchronization.
        cache.apply_uv_trim_updates(target)?;
        result.map(|_| ())
    }
    fn corner_type(
        &self,
        id: crate::blend::fillet_geom::p1::FilletCornerId,
    ) -> crate::error::Result<crate::blend::fillet_geom::p1::FilletCornerType> {
        self.corners
            .get(id.0 as usize)
            .map(crate::blend::fillet_corner::p3::FilletCorner::corner_type)
            .ok_or_else(|| super::invalid("rollover corner is not live in this session"))
    }
}

/// Issued pseudo-B-rep identity for new recovery curves. Notification routing is
/// deliberately unavailable until the recovery API supplies the actual curve key.
pub(super) struct Ownership {
    pub serial: u64,
    pub brep: ObjectRef,
}
impl crate::blend::fillet_solver::p1::RailOwnership for Ownership {
    fn edge_owner(&self, edge: EdgeId) -> crate::error::Result<(ObjectRef, Option<ObjectRef>)> {
        Ok((
            crate::topology::brep::p3::topology_ref(self.serial, EntityRef::Edge(edge)),
            Some(self.brep),
        ))
    }
    fn owner_changed(
        &mut self,
        _: crate::curve::curve::p2::OwnerChange,
    ) -> crate::error::Result<()> {
        // UNPORTED(blend.fillet_solver_p5): ProcessRollOver retained curve notification context.
        Err(crate::error::SolidError::unported(
            "SmFilletSolver::ProcessRollOver (actual curve notification context)",
            "blend.fillet_solver_p5",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::kernel::Kernel;
    use crate::primitive::api_primitives::create_box;

    #[test]
    fn equal_local_ids_keep_actual_body_and_geometry_identity() {
        let mut k = Kernel::default();
        let target = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 20.0, 30.0).unwrap();
        let pseudo = create_box(&mut k, Point3::new(100.0, 200.0, 300.0), 4.0, 5.0, 6.0).unwrap();
        let target_edge = target.edges().ids().next().unwrap();
        let pseudo_edge = pseudo.edges().ids().next().unwrap();
        assert_eq!(target_edge, pseudo_edge);
        let objects = Objects {
            target: &target,
            pseudo: &pseudo,
        };
        let source = FilletClassObject::Curve(FilletBody::Pseudo, pseudo_edge);
        let boundary = objects.edge_curve(target_edge);
        assert_ne!(source, boundary);
        assert!(std::ptr::eq(
            objects.curve(source).unwrap(),
            pseudo.edge(pseudo_edge).unwrap().curve.as_ref().unwrap()
        ));
        assert!(std::ptr::eq(
            objects.curve(boundary).unwrap(),
            target.edge(target_edge).unwrap().curve.as_ref().unwrap()
        ));
        assert!(!std::ptr::eq(
            objects.curve(source).unwrap(),
            objects.curve(boundary).unwrap()
        ));
        assert!(objects
            .curve(FilletClassObject::Legacy(EntityRef::Edge(target_edge)))
            .is_none());
        let target_entity = EntityRef::Edge(target_edge);
        let scoped = objects.topology_handle_in(&pseudo, target_entity).unwrap();
        assert_eq!(
            scoped,
            FilletClassObject::Topology(FilletBody::Pseudo, target_entity)
        );
        assert!(std::ptr::eq(
            objects.topology_entity(scoped).unwrap().0,
            &raw const pseudo
        ));
        assert_ne!(objects.brep_id(&target), objects.brep_id(&pseudo));
        let actual_owner = target
            .edge(target_edge)
            .unwrap()
            .curve
            .as_ref()
            .unwrap()
            .owner()
            .unwrap();
        // Primitive construction retains its supplied producer key. This provider
        // does not infer a canonical body identity from that opaque key.
        assert!(objects.owner_entity(actual_owner).is_none());
        let canonical =
            crate::topology::brep::p3::topology_ref(target.notify_serial().unwrap(), target_entity);
        let (body, entity) = objects.owner_entity(canonical).unwrap();
        assert!(std::ptr::eq(body, &raw const target));
        assert_eq!(entity, target_entity);
        let foreign = create_box(&mut k, Point3::new(-10.0, 0.0, 0.0), 1.0, 1.0, 1.0).unwrap();
        let foreign_edge = foreign.edges().ids().next().unwrap();
        let foreign_owner = foreign
            .edge(foreign_edge)
            .unwrap()
            .curve
            .as_ref()
            .unwrap()
            .owner()
            .unwrap();
        assert!(objects.owner_entity(foreign_owner).is_none());
        let foreign_canonical = crate::topology::brep::p3::topology_ref(
            foreign.notify_serial().unwrap(),
            EntityRef::Edge(foreign_edge),
        );
        assert!(objects.owner_entity(foreign_canonical).is_none());
        assert!(objects
            .topology_handle_in(&foreign, EntityRef::Edge(foreign_edge))
            .is_none());
    }
}
