//! The live session's `DoFilleting` / `DoFillet` drivers and the post-merge tail.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp` (`DoFillet` 1568-1593,
//! `DoFilleting` 2917-3648).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletSession::do_fillet`] | `SmFilletExecutive::DoFillet` |
//! | [`FilletSession::do_filleting`] | `SmFilletExecutive::DoFilleting` (the fork's wrapper of `e418750` round `DoFilletingSteps`) |
//! | `do_filleting_steps` (private) | the fork's `SmFilletExecutive::DoFilletingSteps` (upstream's `DoFilleting` body) |
//! | `join_blended_vertex_fans` (private) | the fork's `SmFilletExecutive::JoinBlendedVertexFans` (`e418750`, WR-665) |
//! | `FinishAccess` (private) | the tail after `MergeBrep` (3532-3644) on the live bodies |
//!
//! Deviations: the stage order, flags and the tail itself are the shared generic
//! [`FilletExecutive::finish_merged_fillet`]; this file only supplies the live bodies.
//! The original-trim mark lock encloses the tail, as upstream's `sMarkLocks` scope.
//! `delete m_pFilletBrep` clears the session attributes and notification records of
//! every entity of the intermediate body before `~SmBrep`. `RemoveTopoEdges` and
//! `BooleanPostProcess` notify through the session's records and attributes, so their
//! face and edge merges carry the dying objects' attributes to the survivors
//! (`SM_NO_MERGE_IN_BREP`); afterwards the attributes of target edges they deleted are
//! cleared from the session store. `do_fillet` returns the filleting's status, as
//! upstream: whether a solid stayed one is decided on the stitched body by the three
//! public edge-list wrappers (`fillet_edges`, `fillet_edges_per_edge`,
//! `variable_radius_fillet` in `api`) and by the offset's put-back of its blends
//! (`OffsetFacesExecutive::reblend`), WR-212.
//! After the in-place merge (also on error) the geometry bound to the target's
//! edges, edge uses and faces under the session's kernel identities is rebound to the
//! target's naming, as the piecewise merge does (owners are pointers upstream;
//! tangency-labs/solidcore#423).
//! `do_filleting` ends with the fork's `SplitFilletFacesAtRadiusCorners`, which the fork
//! runs in `DoFillet` only, so a session driven step by step splits a round at its radius
//! law's corners too.
//! A failing `CreateFilletBrep` (or one that makes no faces) that noted no error notes
//! `SM_FILERR_EDGE_PROBLEM` with what failed, and the caller's error info gets it (not
//! upstream, #11360: upstream leaves code 0 and the default comment there).
//! No public API entry point reaches `DoPiecewiseMerge` or a live-session
//! `IntersectInsertRelate` (global merge): `SetFilletParameters` resets both flags;
//! [`FilletSession::set_do_piecewise_merge`] selects the piecewise merge after it.
//! No recursion.

use super::original_trim::OriginalSurfaceDeletion;
use super::{invalid, FilletBody, FilletObject, FilletSession, SessionObjects};
use crate::blend::fillet_cliff_overflow::Refusal;
use crate::blend::fillet_executive::p3::{
    ErrorInfo, FilletExecutive, FilletFinishAccess, StitchAttempt, StitchSettings,
};
use crate::blend::fillet_geom::p1::FilletErrorType;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::object::NotifyOperation;
use crate::geom::tol::approx_tol3d;
use crate::kernel::Kernel;
use crate::surface::procedural_blend::NetScope;
use crate::topology::brep::{p1, p3};
use crate::topology::{BrepId, EdgeId};
use std::collections::BTreeSet;

const OP: &str = "blend::fillet_executive::session::finish";

/// The operation, status and entity of a failure the executive turns into a
/// [`SolidError::FilletRefused`] when a size builds (WR-696): one that answers with an
/// upstream status, by its status rather than its variant, so a typed failure that replaced
/// an [`SolidError::Upstream`] is refused alike (#13790). Failures that stand for no
/// upstream status are passed on as they are.
fn refusable(error: &SolidError) -> Option<(&'static str, i32, Option<EntityRef>)> {
    error
        .upstream_status()
        .map(|code| (error.operation(), code, error.entity()))
}

/// `FILEXEC_ERR` (`SM_ERR`): the executive's generic failure status, as the phrase says.
fn failure(what: &'static str) -> SolidError {
    SolidError::blend_failed(OP, what)
}

/// Ordinary statuses become `None`; deferrals propagate.
fn ignore<T>(result: Result<T>) -> Result<Option<T>> {
    match result {
        Ok(value) => Ok(Some(value)),
        Err(error) if error.is_unported_or_cancelled() => Err(error),
        Err(_) => Ok(None),
    }
}

fn target_edge(object: FilletObject) -> Result<EdgeId> {
    match object {
        FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge)) => Ok(edge),
        _ => Err(invalid("object is not an actual target edge")),
    }
}

/// The topology entity of an intersector mate (an intermediate or target object).
const fn mate_entity(object: FilletObject) -> Option<EntityRef> {
    match object {
        FilletObject::Topology(_, entity) | FilletObject::IntermediateTopology(entity) => {
            Some(entity)
        }
        _ => None,
    }
}

pub(super) struct FinishAccess<'a, 'target>(pub(super) &'a mut SessionObjects<'target>);

impl FinishAccess<'_, '_> {
    fn require_target(brep: FilletObject) -> Result<()> {
        if brep == FilletObject::Body(FilletBody::Target) {
            Ok(())
        } else {
            Err(invalid("finish body is not the actual target"))
        }
    }

    /// `body` on the target with a notification environment over the session's records
    /// and attributes under the target's issued identities.
    fn with_target_env<T>(
        &mut self,
        k: &mut Kernel,
        body: impl FnOnce(
            &mut Kernel,
            &mut crate::topology::Brep,
            &mut crate::topology::edge::NotifyEnv<'_>,
        ) -> Result<T>,
    ) -> Result<T> {
        let serial = self.0.target_serial;
        let key_of = |entity| p3::topology_ref(serial, entity);
        let SessionObjects {
            target,
            notification_records,
            attributes,
            ..
        } = &mut *self.0;
        k.work_counters.add_target_record_seeding();
        p3::seed_records(notification_records, target, &key_of);
        crate::geom::context::Context::with_notification_context(k, |k, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: notification_records,
                attributes,
                context: Some(context),
                key_of: &key_of,
            };
            body(k, target, &mut env)
        })
        .and_then(|result| result)
    }

    /// `ClearAttributes` of target edges an edit deleted.
    fn clear_deleted_edges(&mut self, before: &[EdgeId]) -> Result<()> {
        let serial = self.0.target_serial;
        for &edge in before {
            if self.0.target.edge(edge).is_err() {
                let key = p3::topology_ref(serial, EntityRef::Edge(edge)).key;
                self.0.attributes.clear_attributes(key)?;
            }
        }
        Ok(())
    }
}

impl FilletFinishAccess<FilletObject> for FinishAccess<'_, '_> {
    fn finish_delete_brep(&mut self, k: &mut Kernel, brep: FilletObject) -> Result<()> {
        if brep != FilletObject::IntermediateBrep {
            return Err(invalid("deleted body is not the actual intermediate body"));
        }
        let mut body = self
            .0
            .intermediate
            .take()
            .ok_or_else(|| invalid("intermediate body is not allocated"))?;
        let serial = body
            .notify_serial()
            .ok_or_else(|| invalid("intermediate body has no kernel identity"))?;
        // ~SmAObject: every remaining object leaves its attributes' user lists.
        let mut entities = Vec::new();
        entities.extend(body.faces().iter().map(|(x, _)| EntityRef::Face(x)));
        entities.extend(body.loops().iter().map(|(x, _)| EntityRef::Loop(x)));
        entities.extend(body.edges().iter().map(|(x, _)| EntityRef::Edge(x)));
        entities.extend(body.edgeuses().iter().map(|(x, _)| EntityRef::Edgeuse(x)));
        entities.extend(body.vertices().iter().map(|(x, _)| EntityRef::Vertex(x)));
        entities.extend(
            body.vertexuses()
                .iter()
                .map(|(x, _)| EntityRef::Vertexuse(x)),
        );
        for entity in entities {
            let key = p3::topology_ref(serial, entity).key;
            self.0.attributes.clear_attributes(key)?;
            self.0.notification_records.remove(&key);
        }
        crate::topology::brep::p4::destroy(k, &mut body)
    }
    fn finish_tolerance(&self, _k: &Kernel, brep: FilletObject) -> Result<f64> {
        Self::require_target(brep)?;
        Ok(self.0.target.tolerance())
    }
    fn finish_do_stitching(
        &mut self,
        k: &mut Kernel,
        brep: FilletObject,
        tolerance: f64,
        settings: StitchSettings,
    ) -> Result<StitchAttempt> {
        Self::require_target(brep)?;
        let serial = self.0.target_serial;
        let key_of = |entity| p3::topology_ref(serial, entity);
        let keys = |object| match object {
            crate::topology::brep_cache::CacheObject::Brep => {
                key_of(EntityRef::Brep(BrepId::from_index(0)))
            }
            crate::topology::brep_cache::CacheObject::Other(owner) => owner,
            object => crate::topology::brep::p2::caller_cache_key(&key_of, object),
        };
        let owner_of = |_| None;
        let identity = crate::query::brep::BrepIdentity {
            brep_id: BrepId::from_index(
                u32::try_from(serial)
                    .map_err(|_| invalid("target body identity exceeds query domain"))?,
            ),
            keys: &keys,
            owner_of: &owner_of,
        };
        let SessionObjects {
            target,
            notification_records,
            attributes,
            ..
        } = &mut *self.0;
        if k.context.issued_brep_serial(target.notify_identity_slot()) != Some(serial) {
            return Err(invalid(
                "stitch body belongs to another notification context",
            ));
        }
        let result = crate::geom::context::Context::with_notification_context(k, |k, context| {
            k.work_counters.add_target_record_seeding();
            p3::seed_records(notification_records, target, &key_of);
            let mut env = crate::topology::edge::NotifyEnv {
                records: notification_records,
                attributes,
                context: Some(context),
                key_of: &key_of,
            };
            let mut edits = crate::heal::stitch::p1::BorrowedStitchEdits {
                env: &mut env,
                curve_identity: crate::topology::brep::p9::CurveIdentity::Actual(
                    &crate::topology::brep::p9::issue_curve_identity,
                ),
            };
            // SmStitch sStitch(sCallBack, dStitchTol3d): base callback, default flags.
            let mut callback = crate::heal::stitch::p1::DefaultStitchCallback::new();
            let mut stitch = crate::heal::stitch::p1::Stitch::new(
                &mut callback,
                tolerance,
                settings.do_region_nesting,
                settings.validate_result,
            );
            stitch.query_identity = Some(identity);
            stitch.edits = Some(&mut edits);
            stitch.ignore_problems = settings.ignore_problems;
            stitch.fast_edge_compare = settings.fast_edge_compare;
            stitch.squeeze_small_edges = settings.squeeze_small_edges;
            stitch.split_edges_with_vertices = settings.split_edges_with_vertices;
            stitch.making_manifold_solid = settings.making_manifold_solid;
            stitch.remove_laminar_slivers = settings.remove_laminar_slivers;
            stitch.do_stitching(
                k,
                target,
                None,
                None,
                crate::heal::stitch::p2::MinGapRequest::default(),
            )
        })?;
        match result {
            Ok(done) => Ok(StitchAttempt {
                status: None,
                stitched: done.stitched_edges,
                lamina: done.lamina_edges,
                max_vertex_gap: done.max_vertex_gap,
                max_edge_gap: done.max_edge_gap,
            }),
            Err(error) if error.is_unported_or_cancelled() => Err(error),
            Err(error) => Ok(StitchAttempt {
                status: Some(error),
                stitched: 0,
                lamina: 0,
                max_vertex_gap: 0.0,
                max_edge_gap: 0.0,
            }),
        }
    }
    fn finish_attribute_users(&self, attribute: FilletObject) -> Result<Vec<FilletObject>> {
        let FilletObject::Attribute(handle) = attribute else {
            return Err(invalid("merge tag is not an actual attribute"));
        };
        let serial = self.0.target_serial;
        let edges = p1::edges(self.0.target, None)?;
        self.0
            .attributes
            .users(handle)?
            .into_iter()
            .map(|user| {
                edges
                    .iter()
                    .find(|edge| p3::topology_ref(serial, EntityRef::Edge(**edge)).key == user)
                    .map(|edge| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(*edge)))
                    .ok_or_else(|| invalid("merge tag user is not an actual target edge"))
            })
            .collect()
    }
    fn finish_remove_topo_edges(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<FilletObject>,
    ) -> Result<()> {
        let edges = executive
            .topo_edges
            .iter()
            .map(|edge| edge.map(target_edge).transpose())
            .collect::<Result<Vec<_>>>()?;
        let before = p1::edges(self.0.target, None)?;
        // As the cleanup below: the merges notify through the session's store.
        let result = self.with_target_env(k, |k, target, env| {
            crate::blend::fillet_executive::p1::remove_topo_edges_with_notifications(
                k,
                target,
                &edges,
                Some(env),
            )
        });
        let cleared = self.clear_deleted_edges(&before);
        result.and(cleared)
    }
    fn finish_boolean_post_process(
        &mut self,
        k: &mut Kernel,
        executive: &mut FilletExecutive<FilletObject>,
        check_all_edges: bool,
    ) -> Result<()> {
        let mut relations = crate::boolean::merge::p1::CleanupRelations::default();
        for edge in executive.intersection_edges.iter().flatten() {
            relations.edges.push(target_edge(*edge)?);
        }
        for vertex in executive.intersection_vertices.iter().flatten() {
            let FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(vertex)) = *vertex
            else {
                return Err(invalid(
                    "intersection vertex is not an actual target vertex",
                ));
            };
            relations.vertices.push(vertex);
        }
        // `m_vTI.GetOtherMate` of the target's edges and vertices (the FALSE edge and
        // vertex filters and `GetCommonVertices`): only whether a mate exists is read, so
        // each mate is recorded under its own operand-local id.
        for edge in p1::edges(self.0.target, None)? {
            let key = FilletObject::Topology(FilletBody::Target, EntityRef::Edge(edge));
            if let Some(EntityRef::Edge(other)) = executive
                .intersector
                .other_mate(Some(key))
                .and_then(mate_entity)
            {
                relations.mates.edges.record(edge, other)?;
            }
        }
        for vertex in p1::vertices(self.0.target, None)? {
            let key = FilletObject::Topology(FilletBody::Target, EntityRef::Vertex(vertex));
            if let Some(EntityRef::Vertex(other)) = executive
                .intersector
                .other_mate(Some(key))
                .and_then(mate_entity)
            {
                relations.mates.vertices.record(vertex, other)?;
            }
        }
        if let Some((edges, faces)) = &self.0.cleanup_kept {
            // An input edge is kept by its id while it is still the whole input edge, with
            // the ends it had. A split one is not: which of its pieces keeps the id follows
            // the edge's direction, so a part and its mirror image would keep different
            // pieces (#15768). Its pieces stay where they lie between two input faces.
            let live: BTreeSet<EdgeId> = p1::edges(self.0.target, None)?.into_iter().collect();
            for (&edge, ends) in edges {
                if live.contains(&edge)
                    && crate::topology::edge::vertices(self.0.target, edge, None)? == *ends
                {
                    relations.kept_edges.insert(edge);
                }
            }
            relations.kept_faces.clone_from(faces);
        }
        let before = p1::edges(self.0.target, None)?;
        // The cleanup's face and edge merges notify through the session's store, so the
        // survivors keep the attributes of the objects merged into them.
        let result = self.with_target_env(k, |k, target, env| {
            crate::boolean::merge::p1::boolean_post_process_with_notifications(
                k,
                target,
                &relations,
                check_all_edges,
                Some(env),
            )
        });
        let cleared = self.clear_deleted_edges(&before);
        result.and(cleared)
    }
    fn finish_faces(&self, brep: FilletObject) -> Result<Vec<FilletObject>> {
        Self::require_target(brep)?;
        Ok(p1::faces(self.0.target, None)?
            .into_iter()
            .map(|face| FilletObject::Topology(FilletBody::Target, EntityRef::Face(face)))
            .collect())
    }
    fn finish_notify_face(
        &mut self,
        k: &mut Kernel,
        face: FilletObject,
        operation: NotifyOperation,
        data1: Option<FilletObject>,
        data2: Option<FilletObject>,
        data3: Option<FilletObject>,
    ) -> Result<()> {
        let FilletObject::Topology(FilletBody::Target, EntityRef::Face(id)) = face else {
            return Err(invalid("notified object is not an actual target face"));
        };
        let serial = self.0.target_serial;
        let key_of = |entity| p3::topology_ref(serial, entity);
        let data =
            |object: Option<FilletObject>| -> Result<Option<crate::geom::object::ObjectRef>> {
                match object {
                    None => Ok(None),
                    Some(FilletObject::Topology(FilletBody::Target, entity)) => {
                        Ok(Some(key_of(entity)))
                    }
                    Some(_) => Err(invalid("notification datum is not actual target topology")),
                }
            };
        let data = [data(data1)?, data(data2)?, data(data3)?];
        let SessionObjects {
            target,
            notification_records,
            attributes,
            ..
        } = &mut *self.0;
        k.work_counters.add_target_record_seeding();
        p3::seed_records(notification_records, target, &key_of);
        crate::geom::context::Context::with_notification_context(k, |_, context| {
            let mut env = crate::topology::edge::NotifyEnv {
                records: notification_records,
                attributes,
                context: Some(context),
                key_of: &key_of,
            };
            crate::topology::face::p3::notify(target, id, &mut env, None, operation, data)
        })?
    }
    /// (not upstream) The records are seeded once for all the faces, not once a face as
    /// [`Self::finish_notify_face`] does: a `PreEdit` notification makes and removes no
    /// entity, so the records are the same, but each seeding walks the whole target, and
    /// a walk a face made a large part's fillet spend most of its time here. Each face
    /// still gets its own notification context, as before.
    fn finish_pre_edit_faces(&mut self, k: &mut Kernel, brep: FilletObject) -> Result<()> {
        Self::require_target(brep)?;
        let serial = self.0.target_serial;
        let key_of = |entity| p3::topology_ref(serial, entity);
        let SessionObjects {
            target,
            notification_records,
            attributes,
            ..
        } = &mut *self.0;
        let faces = p1::faces(target, None)?;
        k.work_counters.add_target_record_seeding();
        p3::seed_records(notification_records, target, &key_of);
        for id in faces {
            let this = key_of(EntityRef::Face(id));
            // SmTypes.h:659: SM_NO_GET_BREP(face) expands to a null object.
            ignore(
                crate::geom::context::Context::with_notification_context(k, |_, context| {
                    let mut env = crate::topology::edge::NotifyEnv {
                        records: &mut *notification_records,
                        attributes: &mut *attributes,
                        context: Some(context),
                        key_of: &key_of,
                    };
                    crate::topology::face::p3::notify(
                        target,
                        id,
                        &mut env,
                        None,
                        NotifyOperation::PreEdit,
                        [Some(this), None, None],
                    )
                })
                .and_then(|notified| notified),
            )?;
        }
        Ok(())
    }
    fn finish_orient_trimmed_surfaces(
        &mut self,
        k: &mut Kernel,
        brep: FilletObject,
        outward: bool,
        maybe_not_solid: &mut bool,
        just_unify: bool,
    ) -> Result<()> {
        Self::require_target(brep)?;
        let serial = self.0.target_serial;
        let key_of = |entity| p3::topology_ref(serial, entity);
        let SessionObjects {
            target,
            notification_records,
            attributes,
            ..
        } = &mut *self.0;
        k.work_counters.add_target_record_seeding();
        p3::seed_records(notification_records, target, &key_of);
        let oriented =
            crate::geom::context::Context::with_notification_context(k, |k, context| {
                let mut env = crate::topology::edge::NotifyEnv {
                    records: notification_records,
                    attributes,
                    context: Some(context),
                    key_of: &key_of,
                };
                let mut marks = crate::topology::topology_traverser::Marks::default();
                p3::orient_trimmed_surfaces(
                    k, target, &mut env, &mut marks, outward, just_unify, None,
                )
            })??;
        *maybe_not_solid = oriented.maybe_not_closed_solid;
        Ok(())
    }
}

impl FilletSession<'_> {
    /// `DoFillet`: refuse uninitialized parameters, create the corners, then
    /// [`Self::do_filleting`], as the generic [`FilletExecutive::do_fillet`].
    ///
    /// # Errors
    /// The first stage error, with the executive's error info recorded as upstream.
    /// With `fixes.fillet_vertex_error_diagnostics`, a missing-corner note from the
    /// corner stage is kept in [`FilletSession::error_info`] across the later failure;
    /// the returned error is still the later stage's own status.
    ///
    /// Not upstream (#2510): the fillet is built to the target's approximation tolerance
    /// (`SmTol::GetApproxTol3d`, its solvers' and its merge's), so a face it rolls on that
    /// is a blend by its definition evaluates as its certified net wherever the net's
    /// bound is within that tolerance ([`NetScope::within`]); a step that asks for more
    /// (a station of a definition) takes S.
    pub fn do_fillet(&mut self, k: &mut Kernel) -> Result<()> {
        crate::profile::profile_scope!(k, "blend::session::do_fillet");
        let tolerance = approx_tol3d(k, 0.5 * self.objects.target.tolerance());
        NetScope::within_mut(k, tolerance, |k| self.do_fillet_within(k))?;
        // Measured outside the net scope, as every later reader of the body (the full check
        // among them) evaluates the round's surfaces.
        self.raise_fillet_tolerances_to_measured_gaps(k)
    }

    /// A round is approximated as finely as its edge is known (`SetFilletParameters`), and its
    /// new edges and vertices are made at that approximation tolerance. They can lie farther
    /// from their faces than that (a face the round meets may itself only be fitted, and the
    /// round's own surface is evaluated from its net outside the fillet), and a stored
    /// tolerance must bound the entity's measured gap to its faces: on the fillet's faces
    /// (those the target did not have) each edge's and vertex's tolerance is grown to its
    /// largest measured gap, never shrunk.
    fn raise_fillet_tolerances_to_measured_gaps(&mut self, k: &Kernel) -> Result<()> {
        let faces_before = std::mem::take(&mut self.objects.faces_before);
        let target = &mut *self.objects.target;
        let mut edges = std::collections::BTreeSet::new();
        let mut vertices = std::collections::BTreeSet::new();
        for f in p1::faces(target, None)? {
            if faces_before.contains(&f) {
                continue;
            }
            for e in crate::topology::face::p4::edges(target, f, None, false, None)? {
                if edges.insert(e) {
                    vertices.extend(crate::topology::edge::start_vertex(target, e)?);
                    vertices.extend(crate::topology::edge::end_vertex(target, e)?);
                }
            }
        }
        for e in edges {
            let mut gap = None::<f64>;
            for eu in target.edge(e)?.edgeuses.clone() {
                if let Some(g) =
                    crate::topology::edgeuse::p1::max_edge_face_gap(k, target, eu, true)?
                {
                    if g.gap_3d.is_finite() {
                        gap = Some(gap.map_or(g.gap_3d, |m| m.max(g.gap_3d)));
                    }
                }
            }
            if let Some(gap) = gap {
                crate::topology::edge::set_tolerance(target, e, gap, true, true)?;
            }
        }
        for v in vertices {
            let mut gap = None::<f64>;
            for vu in crate::topology::vertex::vertexuses(target, v)? {
                if let Some(g) = crate::topology::vertexuse::vertex_face_gap(k, target, vu, true)? {
                    if g.gap_3d.is_finite() {
                        gap = Some(gap.map_or(g.gap_3d, |m| m.max(g.gap_3d)));
                    }
                }
            }
            if let Some(gap) = gap {
                // a vertex's tolerance is the width of its zone: a gap within half of it
                // (`ValidateTolerances`, which grows a bad one the same way)
                let zone = crate::geom::tol::zone_tol3d(k, 2.0 * gap);
                crate::topology::vertex::set_tolerance(target, v, zone, true, true)?;
            }
        }
        Ok(())
    }

    /// The body of [`Self::do_fillet`], in its tolerance's [`NetScope`].
    fn do_fillet_within(&mut self, k: &mut Kernel) -> Result<()> {
        if !self.executive.initialized {
            // The generic NoteError overload changes exactly these two members.
            self.executive.error.code = 1; // SM_FILERR_EXEC_UNINITIALIZED
            self.executive.error.comment = "ERROR: fillet parameters not set up.".into();
            return Err(invalid("fillet parameters not set up"));
        }
        self.create_corners(k, None)?;
        // The opt-in missing-corner note is the reason the later stage fails; keep it
        // through that later error so `error_info()` reports the unsupported vertex,
        // while the returned failure stays the later stage's own status.
        let missing_corner_note = (k.fixes.fillet_vertex_error_diagnostics
            && self.executive.error.code == FilletErrorType::CornerUnsupported as i32)
            .then(|| self.executive.error.clone());
        let (result, split_and_logged) = self.filleting_split_and_logged(k, None);
        if result.is_err() {
            if let Some(note) = missing_corner_note {
                self.executive.error = note;
            }
        }
        // Not upstream (WR-696): a refusal with a size that builds carries it on the error,
        // its status unchanged.
        if let Some((operation, code, entity)) = result.as_ref().err().and_then(refusable) {
            if let Some(fit) = self.fit_size() {
                return Err(SolidError::FilletRefused {
                    operation,
                    code,
                    entity,
                    reason: self.executive.error.no_room,
                    fit,
                });
            }
        }
        result?;
        split_and_logged
    }

    /// `DoFilleting`: build the intermediate fillet body, shrink its main faces,
    /// insert its intersection topology, then (with trimming) select and delete the
    /// original faces, tag and merge the fillet faces and run the post-merge tail
    /// under one original-trim mark scope. The Boolean flag and target editing are
    /// restored on every result.
    ///
    /// # Errors
    /// The first stage error, with all earlier mutations retained.
    ///
    /// With `fixes.fillet_corner_per_vertex_fan` (WR-665, the fork's
    /// tangency-labs/smlib@e418750) the vertices the fillet parameters split by fans are
    /// joined again afterwards where the blends left more than one at a split point,
    /// whether the blends built or not (`JoinBlendedVertexFans`); the filleting's own
    /// error comes first.
    ///
    /// A round whose radius law has a corner is split there (the fork's
    /// `SplitFilletFacesAtRadiusCorners`, which its `DoFillet` runs), so a session filleted
    /// step by step, its corners set between [`Self::create_corners`] and this, builds the
    /// law's corners as [`Self::do_fillet`] does.
    pub fn do_filleting(
        &mut self,
        k: &mut Kernel,
        error_out: Option<&mut ErrorInfo<FilletObject>>,
    ) -> Result<()> {
        let (status, split_and_logged) = self.filleting_split_and_logged(k, error_out);
        status.and(split_and_logged)
    }

    /// The filleting's status, then the split at the radius law's corners and the blend
    /// face log read after it: the filleting's own error is kept apart for
    /// [`Self::do_fillet`].
    fn filleting_split_and_logged(
        &mut self,
        k: &mut Kernel,
        error_out: Option<&mut ErrorInfo<FilletObject>>,
    ) -> (Result<()>, Result<()>) {
        let status = self.do_filleting_unlogged(k, error_out);
        // The fork's SplitFilletFacesAtRadiusCorners (6eecf04): a fillet surface turns where
        // its radius has a corner; the face is split there into faces that are smooth, with
        // an edge along the turn (and rebuilt where the law dips or peaks, 7babe17).
        let split = if status.is_ok() {
            self.split_fillet_faces_at_radius_corners(k)
        } else {
            Ok(())
        };
        // (not upstream) the blend face log is read after the split, whose faces replace
        // the round's
        let logged = self.take_blend_faces(k, status.is_ok() && split.is_ok());
        (status, split.and(logged))
    }

    /// [`Self::do_filleting`]'s filleting, without the split at the radius law's corners
    /// or reading the blend face log.
    fn do_filleting_unlogged(
        &mut self,
        k: &mut Kernel,
        error_out: Option<&mut ErrorInfo<FilletObject>>,
    ) -> Result<()> {
        let status = self.do_filleting_steps(k, error_out);
        // a refusal on an edge or a corner, as data
        let status = self.typed_filleting(k, status);
        let joined = self.join_blended_vertex_fans();
        match status {
            Ok(()) => joined,
            Err(e) => Err(e),
        }
    }

    /// `JoinBlendedVertexFans` (the fork's, tangency-labs/smlib@e418750, WR-665): the
    /// inverse of the fan split after the blends: the target's vertices still at a split
    /// point (within their own tolerance of it), joined into the first of them
    /// (`SmBrep::JoinVertexFans`). Where the blends took the point away none is left.
    fn join_blended_vertex_fans(&mut self) -> Result<()> {
        if self.executive.fan_split_points.is_empty() {
            return Ok(());
        }
        let target = &mut *self.objects.target;
        let mut vertices: Vec<Option<_>> =
            p1::vertices(target, None)?.into_iter().map(Some).collect();
        let points = self.executive.fan_split_points.clone();
        for at in points {
            let mut first = None;
            for slot in &mut vertices {
                let Some(v) = *slot else {
                    continue;
                };
                let record = target.vertex(v)?;
                if record.point.distance(at) > record.tolerance {
                    continue;
                }
                let Some(survivor) = first else {
                    first = Some(v);
                    continue;
                };
                crate::topology::brep::vertex_fans::join_vertex_fans(target, survivor, v)?;
                *slot = None;
            }
        }
        self.executive.fan_split_points.clear();
        Ok(())
    }

    /// Notes `SM_FILERR_EDGE_PROBLEM` with `comment` where nothing is noted yet (not upstream,
    /// #11360): upstream's `DoFilleting` returns `FILEXEC_ERR` after a failing
    /// `CreateFilletBrep` whatever the step noted, and a step that fails without noting (the
    /// rollover's `SM_ERR_NULL_POINTER`) left the caller a failure with code 0 and no comment.
    fn note_unnoted(&mut self, comment: &str) {
        if self.executive.error.code == FilletErrorType::Ok as i32 {
            self.executive.error.code = FilletErrorType::EdgeProblem as i32;
            self.executive.error.comment = comment.into();
        }
    }

    /// `DoFilletingSteps` (the fork's name for upstream's `DoFilleting` body): see
    /// [`Self::do_filleting`].
    fn do_filleting_steps(
        &mut self,
        k: &mut Kernel,
        mut error_out: Option<&mut ErrorInfo<FilletObject>>,
    ) -> Result<()> {
        crate::profile::profile_scope!(k, "blend::session::filleting");
        self.sync_self_intersection_handler();
        self.objects.faces_before = p1::faces(self.objects.target, None)?.into_iter().collect();
        // solidcore-native (WR-697 addendum; the fork refuses this only in its capi): a
        // setback set on a made corner (SetSetBackDist) that builds none (neither the
        // Setback nor an N x N corner) would be dropped silently, so it is refused, as
        // `create_corners` refuses one asked with SetSetBackFill at such a corner
        if self
            .objects
            .corners
            .iter()
            .any(crate::blend::blend_corner::setback::asked_setback_not_built)
        {
            self.note_refusal(Refusal {
                code: FilletErrorType::CornerUnsupported,
                reason: None,
                comment: crate::blend::blend_corner::setback::SETBACK_NOT_BUILT,
            });
            return Err(failure("an asked setback is not built"));
        }
        // (not the fork, #2933) a corner at a vertex marked set back is classified now, where
        // the setback set on it is first read: the Setback where it is one
        self.set_back_marked_corners(k)?;
        // not the fork's (#3145): setbacks that overlap on an edge, before the trace
        if let Some((vertex, refusal)) = crate::blend::blend_corner::setback::overlapping_setbacks(
            k,
            &*self.objects.target,
            &self.objects.corners,
            &self.objects.set_back_fills,
        )? {
            super::note_setback_refusal(
                &mut self.executive,
                &*self.objects.target,
                Some(vertex),
                &refusal,
            );
            self.objects.setback_refusal = refusal.refusal;
            return Err(super::setback_refused(
                "blend::fillet_executive::session::overlapping_setbacks",
                &refusal,
            ));
        }
        // WR-459: the clean-up after filleting leaves the target's own edges and faces.
        self.objects.cleanup_kept = if k.fixes.fillet_cleanup_keeps_target_faces {
            let mut edges = std::collections::BTreeMap::new();
            for edge in p1::edges(self.objects.target, None)? {
                let ends = crate::topology::edge::vertices(self.objects.target, edge, None)?;
                edges.insert(edge, ends);
            }
            Some((
                edges,
                p1::faces(self.objects.target, None)?.into_iter().collect(),
            ))
        } else {
            None
        };
        self.with_filleting_context(k, |session, k| {
            let made = session.create_fillet_brep(k, error_out.as_deref_mut());
            if let Err(error) = &made {
                // (not upstream, #11360) a failure never leaves the error info unset: a step
                // that failed without noting why (a rollover's missing classification) notes
                // its own status
                session.note_unnoted(&format!("Fillet Surface Computation Failure: {error}"));
            }
            if ignore(made)?.is_none() {
                if let Some(output) = error_out.as_deref_mut() {
                    output.assign(&session.executive.error);
                }
                return Err(failure("the fillet body is not built"));
            }
            let faces = session
                .objects
                .intermediate
                .as_ref()
                .map(|body| p1::faces(body, None))
                .transpose()?
                .map_or(0, |faces| faces.len());
            if faces < 1 {
                session
                    .note_unnoted("Fillet Surface Computation Failure: no fillet faces were made");
                if let Some(output) = error_out {
                    output.assign(&session.executive.error);
                }
                return Err(failure("the fillet body has no faces"));
            }
            session.shrink_fillet_faces(k)?;
            // The fork's NoteRadiusCornerSurfaces (6eecf04): the fillet surfaces that turn at
            // a radius corner, whose faces DoFillet splits there once they are in the target
            session.note_radius_corner_surfaces(k)?;
            // The fork's SetMarkCornerPatches (60af574): mark the set-back corners'
            // patch faces, which the merge copies into the target with their marks.
            session.mark_corner_patches()?;
            // (not upstream) with `Kernel::blend_face_log` on: mark every face made, with
            // what made it, read back from the target after filleting
            session.mark_blend_faces(k)?;
            let editing = session.objects.target.editing_enabled();
            session.objects.target.set_editing_enabled(true);
            // The fork's (WR-703, `eb883ad`): a feature standing inside a round's band is kept:
            // the round is trimmed round it in place and sewn in instead of merged.
            // (not with an open G1 setback corner: the fork's ef57570)
            let open_setback = session.has_open_setback();
            if !session.executive.cliff_overflow
                && !session.executive.open_ends.iter().any(|end| end.section)
                && !open_setback
            {
                let kept = session.keep_features_in_bands(k);
                if !matches!(kept, Ok(false)) {
                    session.objects.target.set_editing_enabled(editing);
                    if let Some(output) = error_out {
                        output.assign(&session.executive.error);
                    }
                    return kept.map(drop);
                }
            }
            // The fork's (WR-631, WR-638): a cliff overflow closes the fillet faces into the
            // region the blends remove and subtracts it from the target, instead of merging
            // the fillet faces in; the error a failed subtraction noted stands.
            // WR-674: a round left open at its tangent-continuation ends is built the same
            // way, by subtraction
            // WR-695 (the fork's TrimAtCreases, smlib#79 step 6): with
            // `fillet_overflow_trim_at_creases` a cliff overflow is trimmed at its creases in
            // place instead; a WR-674 section end is still subtracted, unless
            // `fillet_section_end_trim_at_creases` (WR-706) trims it too
            // The fork's ef57570 (smlib#125): so is an open G1 setback
            // corner's hole made (no patches: its spring curves are imprinted on its faces
            // with the sheet's boundary)
            let sectioned = session.executive.open_ends.iter().any(|end| end.section);
            let trimmed = open_setback
                || if sectioned {
                    k.fixes.fillet_section_end_trim_at_creases
                        && k.fixes.fillet_overflow_trim_at_creases
                        && k.fixes.fillet_round_ends_at_tangent_continuation
                } else {
                    session.executive.cliff_overflow && k.fixes.fillet_overflow_trim_at_creases
                };
            if trimmed {
                let result = session.trim_at_creases(k);
                session.objects.target.set_editing_enabled(editing);
                if let Some(output) = error_out {
                    output.assign(&session.executive.error);
                }
                return result;
            }
            if session.executive.cliff_overflow
                || session.executive.open_ends.iter().any(|end| end.section)
            {
                let result = session.subtract_removal_region(k);
                session.objects.target.set_editing_enabled(editing);
                if let Some(output) = error_out {
                    output.assign(&session.executive.error);
                }
                return result;
            }
            let result = session.merge_fillet(k);
            session.objects.target.set_editing_enabled(editing);
            result?;
            if let Some(output) = error_out {
                output.assign(&session.executive.error);
            }
            Ok(())
        })
    }

    fn merge_fillet(&mut self, k: &mut Kernel) -> Result<()> {
        crate::profile::profile_scope!(k, "blend::session::merge");
        self.executive
            .intersector
            .set_primary_brep(Some(FilletObject::Body(FilletBody::Target)));
        self.executive
            .intersector
            .set_other_brep(Some(FilletObject::IntermediateBrep));
        if self.executive.options.piecewise_merge {
            // DoPiecewiseMerge deletes m_pFilletBrep inside its mark scope
            return self.do_piecewise_merge(k);
        }
        // the keys a caller-named target recorded before the merge name its entities
        // afterwards (`Brep::owner_identity`)
        let recorded = self.objects.target.geometry_owner_keys().clone();
        let result = self.merge_fillet_in_place(k);
        // (not upstream: owners are pointers there) the insertion names the geometry
        // it binds to the target's edges and faces by the session's kernel identity
        // of the entity; rebind it to the same entity's identity in the target's
        // naming, as the piecewise merge does, so the curve owner test of
        // `SmEdge::ValidateGeometry` (`SmBrep` test #61) finds its edge
        // (tangency-labs/solidcore#423). Also after an error.
        let naming = self.objects.target.naming();
        let serial = self.objects.target_serial;
        let session_owner = |e| p3::topology_ref(serial, e);
        let restored =
            super::piecewise::normalize_session_owners(self.objects.target, session_owner, |e| {
                super::piecewise::naming_owner(naming, recorded.get(&e).copied(), e)
                    .unwrap_or_else(|| session_owner(e))
            });
        result.and(restored)
    }

    /// The in-place (non-piecewise) body of [`Self::merge_fillet`].
    fn merge_fillet_in_place(&mut self, k: &mut Kernel) -> Result<()> {
        // SetFilletBrepEdgeMap, then the geometry and corner insertion.
        self.insert_fillet_intersection_topology(k)?;
        // Upstream cbi_MAP_TOPO_EDGES=0: do not collect mapped intersections.
        if self.executive.options.global_merge {
            // SmTol::GetApproxTol3d(m_pTargetBrep), then m_vTI.IntersectInsertRelate()
            // between the target and the intermediate body.
            let tolerance = crate::geom::tol::brep_approx_tol_3d(
                k,
                &*self.objects.target,
                crate::geom::tol::SmallTopoType::NormalSize,
            );
            self.executive.intersector.set_this_approx_tol_3d(tolerance);
            self.with_insertion_geometry(k, |k, ti, geometry| {
                ti.intersect_insert_relate(k, geometry, FilletObject::Intersector)
                    .map(|_| ())
            })?;
        }
        if self.executive.options.trimming {
            self.with_original_trim_marks(k, |session, k, locks, serial| {
                let mut deleted = OriginalSurfaceDeletion::default();
                session.select_and_delete_with_marks(k, locks, serial, &mut deleted)?;
                session.tag_and_merge_retained(k)?;
                session.finish_merge(k)
            })?;
        }
        // The final spine-edge scan only emits SM_DBG_WARN in this release build.
        Ok(())
    }

    /// The post-merge tail on the live bodies, then the destruction of the temporary
    /// tags (upstream `SmObjsDelete`, on every result).
    fn finish_merge(&mut self, k: &mut Kernel) -> Result<()> {
        let handles = self
            .objects
            .merge_attributes
            .take()
            .ok_or_else(|| invalid("fillet faces have not been merged"))?;
        let tags: Vec<_> = handles
            .iter()
            .copied()
            .map(FilletObject::Attribute)
            .collect();
        let result = self.executive.finish_merged_fillet(
            k,
            &mut FinishAccess(&mut self.objects),
            FilletObject::Body(FilletBody::Target),
            FilletObject::IntermediateBrep,
            &tags,
        );
        let cleanup = super::original_merge::destroy_tags(&mut self.objects, &tags);
        result.and(cleanup)
    }
}

#[cfg(test)]
mod stitch_identity_tests {
    use super::*;
    use crate::blend::fillet_executive::p3::{FaceConstructionAccess, FaceStitchAccess};
    use crate::math::Point3;

    #[test]
    fn both_stitch_sites_reject_a_foreign_notification_domain() {
        let mut owner = Kernel::default();
        let mut target = crate::primitive::api_primitives::create_box(
            &mut owner,
            Point3::ZERO,
            10.0,
            20.0,
            30.0,
        )
        .unwrap();
        let mut session = FilletSession::new(&mut owner, &mut target).unwrap();
        session.objects.faces_new_brep(&mut owner).unwrap();
        let before_records = session.objects.notification_records.len();
        let before_key = session.objects.target.key();
        let mut foreign = Kernel::default();
        let other =
            crate::primitive::api_primitives::create_box(&mut foreign, Point3::ZERO, 1.0, 2.0, 3.0)
                .unwrap();
        assert_eq!(
            other.notify_serial(),
            session.objects.target.notify_serial()
        );
        let tolerance = session.objects.target.tolerance();
        let target = FilletObject::Body(FilletBody::Target);
        assert!(FinishAccess(&mut session.objects)
            .finish_do_stitching(&mut foreign, target, tolerance, StitchSettings::default(),)
            .is_err());
        assert!(session
            .objects
            .stitch_body(&mut foreign, FilletObject::IntermediateBrep, tolerance,)
            .is_err());
        assert_eq!(session.objects.notification_records.len(), before_records);
        assert_eq!(session.objects.target.key(), before_key);
        assert!(session.objects.intermediate.is_some());
        assert_eq!(
            owner
                .context
                .issued_brep_serial(session.objects.target.notify_identity_slot()),
            Some(session.objects.target_serial)
        );
    }
}

#[cfg(test)]
mod pre_edit_tests {
    use super::*;
    use crate::math::Point3;

    /// The tail of a fillet notifies every face of the target `PreEdit`, which resets its
    /// rectangular-trim flag to unsure. It seeded the session's records before each face, a
    /// walk over every entity of the target each time, so the tail cost faces × entities: on
    /// a part of a few thousand faces most of a fillet's time. The records are seeded once,
    /// and every face is still notified.
    #[test]
    fn the_pre_edit_tail_seeds_the_records_once_and_notifies_every_face() {
        let mut k = Kernel::default();
        let mut target =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 10.0, 20.0, 30.0)
                .unwrap();
        let faces = p1::faces(&target, None).unwrap();
        assert_eq!(faces.len(), 6);
        for &f in &faces {
            target.face_mut(f).unwrap().rectangular_trim = 1;
        }
        let mut session = FilletSession::new(&mut k, &mut target).unwrap();
        k.work_counters.reset();
        FinishAccess(&mut session.objects)
            .finish_pre_edit_faces(&mut k, FilletObject::Body(FilletBody::Target))
            .unwrap();
        assert_eq!(k.work_counters.target_record_seedings(), 1);
        for &f in &faces {
            assert_eq!(
                session.objects.target.face(f).unwrap().rectangular_trim,
                crate::topology::brep_data::UNSURE,
                "{f:?}"
            );
        }
        // Not the target: refused, as each face's notification refused it.
        assert!(FinishAccess(&mut session.objects)
            .finish_pre_edit_faces(&mut k, FilletObject::IntermediateBrep)
            .is_err());
    }
}

/// A ball that does not fit a face it rolls on (WR-462, tangency-labs/solidcore#1172; since the
/// retirement of `fillet_refuse_ball_not_fitting`, judged by `BallIsRegular`, WR-696): a
/// 60 x 40 x 20 box, its top edge at x = 60 rounded r3, then the round's end arc on the face
/// y = 40 with the two edges tangent to it (the top edge at y = 40, the vertical edge at
/// x = 60, y = 40): the ball rolls on the r3 round, so one of radius over 3 does not fit it.
/// (The box's earlier case, an r5 round on the top edge at y = 40 then the r3 round into
/// it, rounded the seam the old corner made between the two rounds; the blend corner ends
/// the r3 round at its smooth vertex instead, so that seam is gone, #2400.)
#[cfg(test)]
mod ball_fit_tests {
    use super::*;
    use crate::blend::fillet_geom::p1::FilletErrorType;
    use crate::error::blend::BlendFailure;
    use crate::geom::global_solver::SolverOperation;
    use crate::math::Point3;
    use crate::topology::{edge, Brep};

    const NO_ROOM: i32 = FilletErrorType::NoRoom as i32;

    fn edge_near(k: &Kernel, b: &Brep, point: [f64; 3]) -> EdgeId {
        let point = Point3::new(point[0], point[1], point[2]);
        let mut nearest = (f64::MAX, None);
        for id in p1::edges(b, None).unwrap() {
            let Some(curve) = edge::curve(b, id).unwrap() else {
                continue;
            };
            let interval = edge::interval(b, id).unwrap();
            let d = curve
                .drop_point(
                    k,
                    &interval,
                    point,
                    None,
                    1e-6,
                    None,
                    SolverOperation::Minimize,
                )
                .unwrap();
            if d.success && d.distance < nearest.0 {
                nearest = (d.distance, Some(id));
            }
        }
        assert!(nearest.0 < 2e-3, "no edge near {point:?}");
        nearest.1.unwrap()
    }

    /// The executive's error code after the round of `radius` on the r3 round's end, and its
    /// result.
    fn end_round(radius: f64) -> (i32, Result<()>) {
        let mut k = Kernel::default();
        let mut body =
            crate::primitive::api_primitives::create_box(&mut k, Point3::ZERO, 60.0, 40.0, 20.0)
                .unwrap();
        let e = edge_near(&k, &body, [60.0, 20.0, 20.0]);
        crate::blend::fillet_executive::api::fillet_edges(&mut k, &mut body, &[e], 3.0, 1, 1, 1.0)
            .unwrap();
        // the r3 round's end arc on y = 40 (about (57, 40, 17)) and the edges tangent to it
        let h = 3.0 * core::f64::consts::FRAC_1_SQRT_2;
        let edges: Vec<_> = [
            [57.0 + h, 40.0, 17.0 + h],
            [30.0, 40.0, 20.0],
            [60.0, 40.0, 8.0],
        ]
        .into_iter()
        .map(|p| edge_near(&k, &body, p))
        .collect();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        session
            .prepare_constant_radius_edges(&mut k, &edges, radius, 1, 1, 1.0)
            .unwrap();
        let result = session.do_fillet(&mut k);
        (session.executive.error.code, result)
    }

    #[test]
    fn a_ball_that_does_not_fit_is_no_room_before_the_corners() {
        for radius in [3.1, 7.0] {
            let (code, result) = end_round(radius);
            eprintln!("r{radius}: code {code} {result:?}");
            let Err(SolidError::FilletRefused { reason, fit, .. }) = result else {
                panic!("r{radius}: {result:?}");
            };
            assert_eq!(
                reason,
                Some(crate::blend::fillet_executive::NoRoomReason::BallDoesNotFit),
                "r{radius}"
            );
            assert!(fit.size > 0.0 && fit.size <= 3.0, "r{radius}: {}", fit.size);
            assert_eq!(code, NO_ROOM, "r{radius}: NoRoom");
        }
    }

    #[test]
    fn a_ball_that_fits_is_not_refused() {
        let (code, result) = end_round(2.0);
        result.unwrap();
        assert_ne!(code, NO_ROOM);
    }

    /// The executive's generic failure (`FILEXEC_ERR`) is a typed failure with the status,
    /// operation and (no) entity of the bare status it replaced (#13790).
    #[test]
    fn the_generic_failure_is_typed_with_its_status() {
        let e = failure("the fillet body is not built");
        assert_eq!(
            e.blend_failure(),
            Some(&BlendFailure::Failed("the fillet body is not built"))
        );
        assert_eq!(e.upstream_status(), Some(1001));
        assert_eq!(e.operation(), OP);
        assert_eq!(e.entity(), None);
        assert_eq!(refusable(&e), Some((OP, 1001, None)));
    }

    /// #13790: the executive's decision on a bare `Upstream` (the WR-696 refusal with a fit
    /// size) treats a typed failure that replaced the `Upstream` alike, and leaves typed
    /// failures that never stood for a status as they are.
    #[test]
    fn typed_failures_that_replaced_an_upstream_status_are_refused_alike() {
        use crate::error::replaced_upstream_samples::{native_failures, replaced_upstream};
        use crate::topology::EdgeId;
        let edge = Some(EntityRef::Edge(EdgeId::from_index(3)));
        // the refusal with a fit size
        let upstream = SolidError::Upstream {
            operation: "intersect::x",
            code: 1001,
            entity: edge,
        };
        assert_eq!(refusable(&upstream), Some(("intersect::x", 1001, edge)));
        for typed in replaced_upstream("intersect::x", 1001, edge) {
            assert_eq!(refusable(&typed), refusable(&upstream), "{typed:?}");
        }
        for native in native_failures("intersect::x", 1001) {
            assert_eq!(refusable(&native), None, "{native:?}");
        }
    }
}
