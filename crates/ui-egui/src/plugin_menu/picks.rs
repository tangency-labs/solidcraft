//! Carry a pick through an edit only when its body and the picked topology still have the same
//! identity. The option query then measures the current face with current coordinates.

use std::{collections::BTreeMap, sync::Arc};

use solidcraft_engine::{
    Sel, Session,
    doc::{self, Document, ModelBody, ModelState, naming},
    kernel,
};

pub(super) struct Snapshot {
    document: Arc<Document>,
    pub(super) revision: Option<u64>,
    geometry: Option<Geometry>,
}

struct Geometry {
    world: Arc<ModelState>,
    /// A display name can pass to a different occurrence after an assembly edit. A duplicated
    /// name identifies no single placement. A design without occurrences has only the root.
    placements: Option<BTreeMap<String, Option<Vec<u64>>>>,
}

impl Snapshot {
    /// An empty viewport picker needs a date before it has any geometry to retain.
    pub(super) fn watch(session: &Session) -> Self {
        Self { document: session.doc.clone(), revision: Some(session.revision), geometry: None }
    }

    pub(super) fn capture(session: &Session, computed: bool) -> Self {
        let geometry = computed.then(|| Geometry::capture(session)).flatten();
        Self { document: session.doc.clone(), revision: computed.then_some(session.revision), geometry }
    }

    pub(super) fn current(&self, session: &Session) -> bool {
        Arc::ptr_eq(&self.document, &session.doc) && self.revision.is_none_or(|revision| revision == session.revision)
    }

    pub(super) fn rebind(&self, new: &Self, pick: &Sel) -> Option<Sel> {
        let name = match pick {
            Sel::Body { name } => name,
            Sel::Face { body, .. } | Sel::Edge { body, .. } | Sel::Vertex { body, .. } => body,
            _ => return None,
        };
        let (old, old_path) = self.geometry.as_ref()?.body(name)?;
        let (body, path) = new.geometry.as_ref()?.body(name)?;
        if old.feature != body.feature || old.definition_name() != body.definition_name() || old.component != body.component || old_path != path {
            return None;
        }
        if matches!(pick, Sel::Body { .. }) || (old.body.same(&body.body) && old.source_transform == body.source_transform) {
            return Some(pick.clone());
        }
        let Sel::Face { index, point, .. } = pick else { return None };
        let names = naming::face_names(old);
        let name_on_face = names.get(*index)?;
        if !naming::face_name_exact(old, *index) || unique_face(&names, name_on_face) != Some(*index) {
            return None;
        }
        let index = unique_face(&naming::face_names(body), name_on_face)?;
        if !naming::face_name_exact(body, index) {
            return None;
        }
        // Follow the placement first, then project the original hit onto the resized surface.
        // The kernel checks the trim: a hit outside the remaining face asks for a new pick.
        let local = doc::apply_point(&doc::mat_inverse(&old.source_transform)?, *point);
        let world = doc::apply_point(&body.source_transform, local);
        let projected = kernel::face_surface(&body.body, index).and_then(|surface| surface.nearest(world)).unwrap_or(world);
        let point = body.body.point_lies_on_face(index, projected, 1e-6).ok()??;
        Some(Sel::Face { body: name.clone(), index, point })
    }
}

fn unique_face(names: &[String], wanted: &str) -> Option<usize> {
    let mut matches = names.iter().enumerate().filter(|(_, name)| name.as_str() == wanted);
    let (index, _) = matches.next()?;
    matches.next().is_none().then_some(index)
}

impl Geometry {
    fn capture(session: &Session) -> Option<Self> {
        if session.model.is_stale() {
            return None;
        }
        let world = session.world_state().ok()?;
        let placements = (!session.doc.occurrences.is_empty()).then(|| {
            let mut paths = BTreeMap::new();
            for (name, _, _, path, _) in session.doc.body_frames_with_source(&session.model.state()) {
                paths.entry(name).and_modify(|entry| *entry = None).or_insert(Some(path));
            }
            paths
        });
        Some(Self { world, placements })
    }

    fn body(&self, name: &str) -> Option<(&ModelBody, &[u64])> {
        let mut found = self.world.bodies.iter().filter(|b| b.name == name);
        let body = found.next()?;
        let path = match &self.placements {
            None => &[][..],
            Some(paths) => paths.get(name)?.as_deref()?,
        };
        found.next().is_none().then_some((body, path))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use solidcore_record::Vec3;
    use solidcraft_engine::geom::Vec3Geom;

    fn blank(session: &mut Session, name: &str, x: f64) {
        session.execute("solid.cylinder", &json!({"body_name":name,"operation":"new","base":[x,0,0],"height":12,"radius":"blankRadius"})).unwrap();
    }

    fn wall(session: &Session, name: &str, x: f64) -> Sel {
        let world = session.world_state().unwrap();
        let body = world.body(name).unwrap();
        let index =
            (0..body.body.face_count()).find(|i| matches!(kernel::face_surface(&body.body, *i), Some(kernel::FaceSurface::Cylinder { .. }))).unwrap();
        let point = Vec3::new(x, 0., 6.);
        assert!(body.body.point_lies_on_face(index, point, 1e-6).unwrap().is_some());
        Sel::Face { body: name.into(), index, point }
    }

    #[test]
    fn a_resized_cylinder_keeps_its_picked_wall_at_a_current_point() {
        let mut session = Session::default();
        session.execute("parameters.add", &json!({"name":"blankRadius","expression":"5 mm","unit":"mm"})).unwrap();
        blank(&mut session, "Blank", 0.);
        let pick = wall(&session, "Blank", 5.);
        let before = Snapshot::capture(&session, true);
        session.execute("parameters.change", &json!({"name":"blankRadius","expression":"6 mm"})).unwrap();
        let after = Snapshot::capture(&session, true);
        let Sel::Face { body, index, point } = before.rebind(&after, &pick).unwrap() else { panic!("expected the same wall") };
        assert_eq!(body, "Blank");
        assert!(point.dist(Vec3::new(6., 0., 6.)) < 1e-6);
        assert!(session.world_state().unwrap().body("Blank").unwrap().body.point_lies_on_face(index, point, 1e-6).unwrap().is_some());
    }

    #[test]
    fn replacing_a_blank_with_an_identical_one_keeps_only_the_other_pick() {
        let mut session = Session::default();
        session.execute("parameters.add", &json!({"name":"blankRadius","expression":"5 mm","unit":"mm"})).unwrap();
        blank(&mut session, "Pinion", 0.);
        blank(&mut session, "Ring", 20.);
        let pinion = wall(&session, "Pinion", 5.);
        let original_ring = wall(&session, "Ring", 25.);
        let original = Snapshot::capture(&session, true);
        let feature = session.doc.features.last().unwrap().id;
        session.execute("timeline.delete", &json!({"feature":feature})).unwrap();
        let removed = Snapshot::capture(&session, true);
        assert!(original.rebind(&removed, &original_ring).is_none());
        blank(&mut session, "Ring", 20.);
        let replacement = Snapshot::capture(&session, true);
        assert_eq!(original.rebind(&replacement, &pinion), Some(pinion));
        assert!(original.rebind(&replacement, &original_ring).is_none());
        assert!(original.rebind(&replacement, &Sel::Body { name: "Ring".into() }).is_none());
    }
}
