//! Healer target updates, ordered healing and coincidence, degeneracy and seam repairs.
//!
//! Upstream: `source/SMLib/src/SmHealData.cpp`, `source/SMLib/src/SmBrep.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`HealData::update_target_lists`] | `SmHealData::UpdateTargetLists(array overload)` |
//! | [`HealData::update_tracked_targets`] | `SmHealData::UpdateTargetLists(tracker overload)` |
//! | [`HealData::heal_targets`] | `SmHealData::HealTgts` |
//! | [`HealData::fix_coin_vertices`] | `SmHealData::Fix_CoinVertices` |
//! | [`HealData::fix_degen_faces`] | `SmHealData::Fix_DegenFaces` |
//! | [`HealData::fix_degen_edges`] | `SmHealData::Fix_DegenEdges` |
//! | [`HealData::fix_move_seam`] | `SmHealData::Fix_MoveSeam` |
//! | [`HealData::fix_split_edges_at_seam`] | `SmHealData::Fix_SplitEdgesAtSeam` |
//! | [`HealData::fix_split_face_at_seams`] | `SmHealData::Fix_SplitFaceAtSeams` |
//! | calls `topology::brep::p1::is_live_topology_member` (unit `topology.brep_p1`; it replaced the private co-port `live`) | `SmBrep::IsLiveTopologyMember` |
//!
//! Deviations: topology and props are arena ids as in [`super::p2`]. The B-rep and
//! kernel are explicit arguments. Tracker object keys are resolved by the caller,
//! because `SmTopology` notifications use caller-chosen keys. `None` add entries
//! represent null tracked pointers; stale additions are skipped by arena identity.
//! `heal_targets`, `fix_coin_vertices`, `fix_degen_faces` and `fix_degen_edges`
//! temporarily enable `Brep::editing_enabled` (`SmTemporaryChangeValue(m_bEditingEnabled,
//! TRUE)`) and restore it on every exit.
//! Mutating callees not yet ported return `Unported`, including when C++ ignores
//! their status; no successful mutation is simulated. Removed/added identities
//! for p5 edits are captured by the actual context callback (`capture_changes`).
//! Nonempty captures still require the owning notification adapter's key-to-id map;
//! they defer rather than guessing identities from arena indices. The ported p3/p4/face edits
//! (`SqueezeEdge`, `DeleteEdge`, `DeleteVertex`, `DeleteFace`, `CreateFaceInRegionFromSurface`, `MoveSeam`) run under a
//! real `SmTrackTopologyChanges` on a context of their own (`tracked_edit`), one per edit
//! (upstream keeps one per heal loop; the lists merge to the same result with `Changes::append`); the kernel
//! context's callbacks and identity domain are lent to that context, and the edits
//! name every object by its kernel-issued identity (`topology::brep::p3`'s
//! Deviations), which the tracker's lists are decoded by. It has no geometry-owner
//! metadata, so a geometry owner walk is a capture failure that leaves only the Chg
//! list short, which `Changes` does not read. Recursive target catch-up is limited to 32 nested updates (upstream
//! has no bound); temporary counters and healer state are restored on every exit.
//! Ordinary statuses from nested `HealTgts` are ignored as upstream, but unported
//! calls and invalid memory accesses propagate. Bare/SE mutation statuses are
//! ignored except `Unported`; failed calls with no output skip uses of that output.
//! `Fix_BadLoops` stays disabled. Opaque nonempty classification outputs (including
//! p7 `FindEdgeObjects`) require a typed resolver and are rejected if unresolved
//! (currently unreachable behind missing callees).
//! Partial lists from completed merge intervals survive ignored failures.
//! `Fix_SplitFaceAtSeams` updates the split-*edge* tried list, as upstream.

use crate::error::{ignored as ignore_soft, EntityRef, Result, SolidError, SM_ERR};
use crate::geom::attribute::ObjectKey;
use crate::geom::context::{MarkLock, ALL_MARKS};
use crate::heal::face_props::{HealerOpType, LoopObject};
use crate::kernel::Kernel;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p4;
use crate::topology::topology::TrackTopologyChanges;
use crate::topology::topology_traverser::{Entity, Marks};
use crate::topology::{edge, Brep, EdgeId, FaceId, VertexId};

use super::p2::{EdgeProps, FacePropsEntry, HealData, PropsStore, VertexProps};

use crate::curve::curve::Curve;
use crate::geom::object::ObjectRef;
use crate::geom::tol::xsect_tol_3d_from_zones;
use crate::heal::face_props::{
    FaceProps, PropStage, TriedLinkType, SS_UMAX, SS_UMIN, SS_VMAX, SS_VMIN,
};
use crate::intersect::curve_class::p1::CurveClassification;
use crate::intersect::point_class::ClassGeometry;
use crate::math::Point3;
use crate::nurbs::b_spline_surface::p1::SurfParamType;
use crate::surface::surface::Surface;
use crate::surface::volume::Volume;
use crate::topology::brep_cache::{arena_identity, CacheObject};
use crate::topology::edge::NotifyEnv;
use crate::topology::face::p1::{self as fp1, SeamProblem};
use crate::topology::face::p2 as face;
use crate::topology::{loopuse, vertex, RegionId};

const OP: &str = "heal::heal_data::p3";
/// Maximum number of nested `UpdateTargetLists` catch-up passes.
pub const MAX_TARGET_UPDATE_DEPTH: u64 = 32;

fn invalid(reason: &'static str) -> SolidError {
    SolidError::invalid_input(OP, reason)
}

// Upstream ignores operational statuses, never errors that represent a missing
// port or a dereference it could not survive.
fn ignore_status(r: Result<()>) -> Result<()> {
    match r {
        Ok(()) | Err(SolidError::Upstream { .. } | SolidError::Nlib { .. }) => Ok(()),
        Err(e) => Err(e),
    }
}

/// `SmTemporaryChangeValue<SmBoolean>(pBrep->m_bEditingEnabled, TRUE)`.
fn with_editing_enabled<T>(b: &mut Brep, op: impl FnOnce(&mut Brep) -> Result<T>) -> Result<T> {
    let saved = b.editing_enabled();
    b.set_editing_enabled(true);
    let r = op(b);
    b.set_editing_enabled(saved);
    r
}

/// Per-operation topology notifications, in the same order as the native tracker.
#[derive(Default)]
struct Changes {
    removed: Vec<EntityRef>,
    added: Vec<Option<EntityRef>>,
}

impl Changes {
    fn append(&mut self, other: Self) {
        // Native notification lists use AddUnique; a later removal also
        // cancels an earlier add. A survivor re-added by a merge goes last.
        for e in other.removed {
            if !self.removed.contains(&e) {
                self.removed.push(e);
            }
            self.added.retain(|x| *x != Some(e));
        }
        for e in other.added {
            if !self.added.contains(&e) {
                self.added.push(e);
            }
        }
    }
}

impl<O: LoopObject> HealData<O> {
    /// Applies a stopped topology tracker's remove/add lists. `resolve` maps its
    /// opaque keys to typed ids, including ids of removed objects; an unresolved
    /// remove entry cannot match a target, an unresolved add is skipped.
    /// Returns `SM_ERR` while that tracker is active.
    pub fn update_tracked_targets(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        tracker: &TrackTopologyChanges,
        resolve: impl Fn(ObjectKey) -> Option<EntityRef>,
        through: HealerOpType,
    ) -> Result<()> {
        if tracker.is_tracking(Some(&k.context)) {
            return Err(SolidError::Upstream {
                operation: OP,
                code: SM_ERR,
                entity: None,
            });
        }
        if let Some(error) = tracker.failure() {
            return Err(error);
        }
        let removed: Vec<_> = tracker
            .rm_list()
            .iter()
            .filter_map(|&x| resolve(x))
            .collect();
        let added: Vec<_> = tracker.add_list().iter().map(|&x| resolve(x)).collect();
        self.update_target_lists(k, b, &removed, &added, through)
    }

    /// Removes stale targets and their props from every problem list, adds live
    /// new targets and heals them through `min(through, done_heal_op)`.
    /// The temporary old counts, recursion count and stop/done state are restored
    /// even when nested healing returns an error.
    pub fn update_target_lists(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        removed: &[EntityRef],
        added: &[Option<EntityRef>],
        through: HealerOpType,
    ) -> Result<()> {
        // Resolve every stale props identity before removing any of them.
        let faces = self.face_props_for_tgts(removed, true);
        let edges = self.edge_props_for_tgts(removed, true);
        let vertices = self.vertex_props_for_tgts(removed, true);
        for id in faces {
            self.remove_face_props_from_lists(id)?;
            self.face_props.free(id);
        }
        for id in edges {
            self.remove_edge_props_from_lists(id)?;
            self.edge_props.free(id);
        }
        for id in vertices {
            self.remove_vertex_props_from_lists(id)?;
            self.vertex_props.free(id);
        }
        if added.is_empty() {
            return Ok(());
        }
        if self.recursion_count >= MAX_TARGET_UPDATE_DEPTH {
            return Err(invalid("target-update recursion exceeds 32 nested passes"));
        }
        let saved_counts = (self.old_face_cnt, self.old_edge_cnt, self.old_vertex_cnt);
        self.old_face_cnt = self.tgt_faces.len();
        self.old_edge_cnt = self.tgt_edges.len();
        self.old_vertex_cnt = self.tgt_vertices.len();
        for &e in added.iter().flatten() {
            if !bp1::is_live_topology_member(b, Some(e)) {
                continue;
            }
            match e {
                EntityRef::Face(f) => {
                    self.tgt_faces.push(f);
                    self.tgt_face_props
                        .push(self.face_props.alloc(FacePropsEntry::new()));
                }
                EntityRef::Edge(e) => {
                    self.tgt_edges.push(e);
                    self.tgt_edge_props
                        .push(self.edge_props.alloc(EdgeProps::new()));
                }
                EntityRef::Vertex(v) => {
                    self.tgt_vertices.push(v);
                    self.tgt_vertex_props
                        .push(self.vertex_props.alloc(VertexProps::new()));
                }
                _ => (),
            }
        }
        let saved_state = (self.stop_heal_op, self.done_heal_op);
        let through = if through.code() > self.done_heal_op.code() {
            self.done_heal_op
        } else {
            through
        };
        self.stop_heal_op = through;
        self.done_heal_op = HealerOpType::None;
        self.recursion_count += 1;
        let result = ignore_status(self.heal_targets(k, b, through, false));
        if self.recursion_count > 0 {
            self.recursion_count -= 1;
        }
        (self.stop_heal_op, self.done_heal_op) = saved_state;
        (self.old_face_cnt, self.old_edge_cnt, self.old_vertex_cnt) = saved_counts;
        result
    }

    /// Runs the native heal sequence through `healer_op`, resetting stop/done state
    /// on entry. Step 1 (`Fix_BackPointers`) is skipped upstream: requesting it
    /// still runs `Cache_EdgeProps` (step 2). Disabled `Fix_BadLoops` is skipped too.
    /// `make_uv_trim_curves` corresponds to the upstream default `false`.
    pub fn heal_targets(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        healer_op: HealerOpType,
        make_uv_trim_curves: bool,
    ) -> Result<()> {
        self.stop_heal_op = healer_op;
        self.done_heal_op = HealerOpType::None;
        with_editing_enabled(b, |b| {
            let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS)?;
            let mut marks = Marks::default();
            let result =
                self.heal_targets_with(k, b, &mut marks, &lock, healer_op, make_uv_trim_curves);
            lock.release(std::slice::from_mut(&mut k.context))?;
            result
        })
    }

    fn heal_targets_with(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        marks: &mut Marks,
        lock: &MarkLock,
        through: HealerOpType,
        make: bool,
    ) -> Result<()> {
        // The condition is Done < requested, not Step <= requested. Preserve the
        // gaps in the sequence (Fix_BackPointers and disabled Fix_BadLoops).
        macro_rules! step {
            ($op:ident, $call:expr) => {
                if self.done_heal_op.code() < through.code() {
                    $call?;
                    self.done_heal_op = HealerOpType::$op;
                }
            };
        }
        step!(
            CacheEdgeprops,
            self.cache_edge_props(k, b, HealerOpType::CacheEdgeprops)
        );
        step!(
            CacheVertexprops,
            self.cache_vertex_props(k, b, HealerOpType::CacheVertexprops)
        );
        step!(
            CacheFacepropsGaps,
            self.cache_face_props_gaps(k, b, HealerOpType::CacheFacepropsGaps)
        );
        step!(
            FixTolsizes,
            self.fix_tol_sizes(k, b, HealerOpType::FixTolsizes)
        );
        step!(
            CacheCoinVertices,
            self.cache_coin_vertices(k, b, HealerOpType::CacheCoinVertices)
        );
        step!(
            FixCoinVertices,
            self.fix_coin_vertices(k, b, HealerOpType::FixCoinVertices)
        );
        step!(
            CacheDegenFaces,
            self.cache_degen_faces(k, b, HealerOpType::CacheDegenFaces)
        );
        step!(
            FixDegenFaces,
            self.fix_degen_faces(k, b, HealerOpType::FixDegenFaces)
        );
        step!(
            FixDegenEdges,
            self.fix_degen_edges(k, b, HealerOpType::FixDegenEdges)
        );
        step!(
            CacheCoinEdges,
            Ok::<_, SolidError>(self.cache_coin_edges(HealerOpType::CacheCoinEdges))
        );
        step!(
            FixCoinEdges,
            self.fix_coin_edges(HealerOpType::FixCoinEdges)
        );
        step!(
            CacheMissedEdgexsects,
            Ok::<_, SolidError>(self.cache_missed_edge_xsects(HealerOpType::CacheMissedEdgexsects))
        );
        step!(
            FixMissedEdgexsects,
            self.fix_missed_edge_xsects(HealerOpType::FixMissedEdgexsects)
        );
        step!(
            FixUncontainedEdges,
            self.fix_uncontained_edges(HealerOpType::FixUncontainedEdges)
        );
        step!(FixBadgaps, {
            self.fix_bad_gaps(HealerOpType::FixBadgaps);
            Ok::<_, SolidError>(())
        });
        step!(
            CacheFaceprops2,
            self.cache_face_props_stage2(k, b, HealerOpType::CacheFaceprops2)
        );
        step!(
            FixMoveseam,
            self.fix_move_seam(k, b, marks, lock, HealerOpType::FixMoveseam)
        );
        step!(
            FixSplitedgeAtseam,
            self.fix_split_edges_at_seam(k, b, marks, lock, HealerOpType::FixSplitedgeAtseam)
        );
        step!(FixBadsheets, {
            // The heal path carries no attribute state at any level (no AttributeStore,
            // no SmTopology records), so SmSAGObject attribute propagation during
            // healing is not modelled anywhere in heal: the region split's
            // SmBrep::Notify runs on default region records and an empty store. Thread
            // an env from heal_targets once heal gains an AttributeStore
            // (plan/findings.md, heal.heal_data_p3).
            let mut state = crate::topology::brep::p2::RegionNotifyState::new(b);
            self.fix_bad_sheets(
                k,
                b,
                marks,
                &mut state.env(),
                lock,
                HealerOpType::FixBadsheets,
            )
        });
        step!(
            CacheFaceprops3,
            self.cache_face_props_stage3(k, b, HealerOpType::CacheFaceprops3)
        );
        step!(
            FixSplitfaceAtseams,
            self.fix_split_face_at_seams(k, b, HealerOpType::FixSplitfaceAtseams)
        );
        // Fix_BadLoops is disabled in pinned upstream.
        step!(
            MakeUvtrimcurves,
            self.make_uv_trim_curves(k, b, make, HealerOpType::MakeUvtrimcurves)
        );
        step!(
            FixInfiniteRegions,
            self.fix_infinite_region(k, b, HealerOpType::FixInfiniteRegions)
        );
        Ok(())
    }

    /// Merges the live coincident vertex pairs in snapshot order, rewriting later
    /// pairs from the removed props to the survivor after each tracked merge.
    pub fn fix_coin_vertices(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        _op: HealerOpType,
    ) -> Result<()> {
        with_editing_enabled(b, |b| {
            let mut pairs = self.bad_vertex_props_coin_vertices.base.prob_array.clone();
            let n = pairs.len() / 2; // upstream intentionally ignores an odd tail
            for i in 0..n {
                let (Some(&survivor), Some(&deleted)) = (pairs.get(2 * i), pairs.get(2 * i + 1))
                else {
                    return Err(invalid("coincident pair list changed"));
                };
                if survivor == deleted
                    || !self.tgt_vertex_props.contains(&survivor)
                    || !self.tgt_vertex_props.contains(&deleted)
                {
                    continue;
                }
                let a = self
                    .vertex_props
                    .props(survivor)?
                    .vertex
                    .ok_or_else(|| invalid("survivor props without vertex"))?;
                let d = self
                    .vertex_props
                    .props(deleted)?
                    .vertex
                    .ok_or_else(|| invalid("deleted props without vertex"))?;
                let changes = combine_vertices(k, b, a, d)?;
                self.update_target_lists(
                    k,
                    b,
                    &changes.removed,
                    &changes.added,
                    HealerOpType::FixCoinVertices,
                )?;
                for id in pairs.iter_mut().take(2 * n).skip(2 * (i + 1)) {
                    if *id == deleted {
                        *id = survivor;
                    }
                }
            }
            Ok(())
        })
    }

    /// Squeezes degenerate edges in reverse problem-list order, deleting closed
    /// single-vertex edges instead, then updates all target and problem lists.
    pub fn fix_degen_edges(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        _op: HealerOpType,
    ) -> Result<()> {
        let queue = self.bad_edge_props_degen_edges.prob_array.clone();
        if queue.is_empty() {
            return Ok(());
        }
        with_editing_enabled(b, |b| {
            let mut changes = Changes::default();
            for id in queue.into_iter().rev() {
                let p = self.edge_props.props(id)?;
                let e = p.edge.ok_or(SolidError::Upstream {
                    operation: OP,
                    code: SM_ERR,
                    entity: None,
                })?;
                if p.bad_degen_edge.is_false() {
                    continue;
                }
                let (a, d) = edge::start_end_vertices(b, e)?;
                let edits = if a == d {
                    delete_edge(k, b, e)?
                } else {
                    squeeze_edge(
                        k,
                        b,
                        e,
                        a.ok_or_else(|| invalid("edge without start vertex"))?,
                    )?
                };
                changes.append(edits);
            }
            self.update_target_lists(
                k,
                b,
                &changes.removed,
                &changes.added,
                HealerOpType::FixDegenEdges,
            )
        })
    }
}

// The context callback, not a mutation's nominal inputs/outputs, owns these lists
// (`SmTrackTopologyChanges`, then `UpdateTargetLists(GetRmList(), GetAddList(), ..)`).
// The edits run without a notification environment, so part 5 names `b`'s objects by
// their kernel identities (its plain entry points' `plain_edit`:
// `topology::brep::p3::topology_ref` with `b`'s serial in the kernel context's domain),
// which `topology::brep::p3::topology_entity` inverts with that serial. A key it did not
// issue is an error, never a guessed identity.
fn capture_changes<T>(
    k: &mut Kernel,
    b: &mut Brep,
    edit: impl FnOnce(&mut Kernel, &mut Brep) -> Result<T>,
) -> Result<(T, Changes)> {
    use crate::geom::context::NotifyCallback;
    use crate::topology::topology::TopologyChangeCallback;
    if k.context.sys_notify_callback().is_some() {
        // Native StartTracking preserves a preexisting callback, leaving this
        // tracker's own arrays empty. StopTracking preserves that callback too.
        // These are the actual empty native arrays, not guessed edit identities.
        let value = edit(k, b)?;
        return Ok((value, Changes::default()));
    }
    k.context
        .set_sys_notify_callback(Some(Box::new(TopologyChangeCallback::new(1))));
    let result = edit(k, b);
    let captured = k
        .context
        .sys_notify_callback()
        .and_then(NotifyCallback::as_any)
        .and_then(|a| a.downcast_ref::<TopologyChangeCallback>())
        .map(|callback| (callback.lists(), callback.failure()));
    k.context.set_sys_notify_callback(None);
    let value = result?;
    let (lists, failure) = captured.ok_or_else(|| invalid("edit replaced its topology tracker"))?;
    if let Some(error) = failure {
        return Err(error);
    }
    let serial = b.notify_serial();
    let decode = |key| {
        serial
            .and_then(|s| crate::topology::brep::p3::topology_entity(s, key))
            .ok_or_else(|| invalid("tracked object without a kernel identity of this B-rep"))
    };
    let removed = lists
        .rm_list
        .iter()
        .map(|&key| decode(key))
        .collect::<Result<Vec<_>>>()?;
    let added = lists
        .add_list
        .iter()
        .map(|&key| decode(key).map(Some))
        .collect::<Result<Vec<_>>>()?;
    Ok((value, Changes { removed, added }))
}

fn combine_vertices(k: &mut Kernel, b: &mut Brep, a: VertexId, d: VertexId) -> Result<Changes> {
    let ((), changes) = capture_changes(k, b, |k, b| {
        let mut deleted_edge = None;
        crate::topology::brep::p5::combine_coincident_vertices(k, b, a, d, &mut deleted_edge)
    })?;
    Ok(changes)
}

/// Tracker key of the edit trackers (one at a time).
const EDIT_TRACKER: u64 = 0x4845_414c; // "HEAL"

/// Runs the B-rep edit `edit` under a topology tracker, as the healer's loops do
/// (`SmTrackTopologyChanges sTopologyChanges(m_pBrep, TRUE)`, SmHealData.cpp:3341 and
/// the later loops), and returns its result with what the tracker recorded
/// (`SmTopologyChangeCallback::Execute`, `topology::topology`): the removed and the
/// added faces, edges and vertices. The edit also borrows the kernel (mark locks),
/// so the kernel context's notify callbacks are lent to a context of their own for
/// the edit's notifications and given back afterwards
/// (`topology::brep::p3::with_lent_context`); a callback already installed there (an
/// outer tracker) keeps the edit's notifications, and the tracker then records
/// nothing, as upstream. The healer keeps no notification identities of its own, so
/// `b`'s objects are named by their kernel identities
/// (`topology::brep::p3::kernel_named_edit`), and the tracker's lists are decoded with
/// `b`'s serial. Notification records and attributes are scratch: nothing in the
/// healer reads them. A failed edit still reports what was tracked before it failed
/// (callers that ignore its status keep those changes). Not nestable: a tracked edit
/// (or a `topology::brep::p3::with_kernel_callbacks`) inside `edit` is refused, before
/// anything changes.
fn tracked_edit<R>(
    k: &mut Kernel,
    b: &mut Brep,
    edit: impl FnOnce(&mut Kernel, &mut Brep, &mut NotifyEnv<'_>) -> Result<R>,
) -> (Result<R>, Changes) {
    use crate::topology::brep::p3 as brep_p3;
    let lent = brep_p3::with_lent_context(k, |k, ctx| {
        // (the tracker reads its B-rep only to tell it from null)
        let tracker = TrackTopologyChanges::new(
            Some(crate::topology::BrepId::from_index(0)),
            EDIT_TRACKER,
            true,
            Some(&mut *ctx),
        );
        let r = brep_p3::kernel_named_edit(k, ctx, b, edit);
        let serial = b.notify_serial();
        let edit_entity = |key: ObjectKey| {
            serial
                .and_then(|s| brep_p3::topology_entity(s, key))
                .filter(|e| {
                    matches!(
                        e,
                        EntityRef::Face(_) | EntityRef::Edge(_) | EntityRef::Vertex(_)
                    )
                })
        };
        let changes = Changes {
            removed: tracker
                .rm_list()
                .into_iter()
                .filter_map(edit_entity)
                .collect(),
            added: tracker.add_list().into_iter().map(edit_entity).collect(),
        };
        tracker.finish(Some(ctx));
        (r, changes)
    });
    lent.unwrap_or_else(|e| (Err(e), Changes::default()))
}
/// `SqueezeEdge(e, survivor)` under the edit tracker ([`tracked_edit`]).
fn squeeze_edge(k: &mut Kernel, b: &mut Brep, e: EdgeId, survivor: VertexId) -> Result<Changes> {
    let (r, changes) = tracked_edit(k, b, |k, b, env| {
        crate::topology::brep::p3::squeeze_edge(k, b, env, e, survivor)
    });
    r.map(|()| changes)
}
/// `SmBrep::DeleteEdge(edge)` (default `pOptSurfaceToKeep = NULL`) under the edit
/// tracker ([`tracked_edit`]).
fn delete_edge(k: &mut Kernel, b: &mut Brep, e: EdgeId) -> Result<Changes> {
    let (r, changes) = tracked_edit(k, b, |k, b, env| {
        p4::delete_edge(k, b, &mut p4::DeleteEnv::from_notify(env), e, None)
    });
    r.map(|()| changes)
}

impl<O: LoopObject> HealData<O> {
    /// Rebuilds closed surfaces imported as a single vertex loop, deletes open
    /// single-vertex faces and squeezes other degenerate faces, removing wire
    /// edges and the shell vertices they leave behind.
    pub fn fix_degen_faces(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        _op: HealerOpType,
    ) -> Result<()> {
        let queue = self.bad_face_props_degen_faces.prob_array.clone();
        if queue.is_empty() {
            return Ok(());
        }
        with_editing_enabled(b, |b| {
            let mut changes = Changes::default();
            for id in queue {
                if !self.tgt_face_props.contains(&id) {
                    continue;
                }
                let p = &self.face_props.props(id)?.props;
                if p.bad_degen_face.is_false() {
                    continue;
                }
                let Some(f) = p.face else {
                    continue; // IsLiveTopologyMember(NULL) is false upstream.
                };
                if !bp1::is_live_topology_member(b, Some(EntityRef::Face(f))) {
                    continue;
                }
                let mut edges = face_edges(b, f)?;
                let loops = face::loops(b, f, false, None)?;
                let vertex_loop = if let [l] = loops.as_slice() {
                    let lu = b
                        .loop_(*l)?
                        .loopuse
                        .ok_or_else(|| invalid("loop without loop use"))?;
                    loopuse::vertexuse(b, lu)?
                } else {
                    None
                };
                if let Some(vu) = vertex_loop {
                    let surface =
                        face::surface(b, f)?.ok_or_else(|| invalid("face without surface"))?;
                    let domain = surface.natural_uv_domain_with_nurb(k)?;
                    let zone = face::tolerance(b, f)?;
                    let tol = xsect_tol_3d_from_zones(k, zone, zone);
                    let (closed_u, _) =
                        surface.is_closed(k, &domain, SurfParamType::U, Some(tol), false)?;
                    let (closed_v, _) =
                        surface.is_closed(k, &domain, SurfParamType::V, Some(tol), false)?;
                    let poles = u64::from(surface.get_singularities(k, false, false)?.singular);
                    if (closed_u && closed_v)
                        || (closed_v && poles & SS_UMIN != 0 && poles & SS_UMAX != 0)
                        || (closed_u && poles & SS_VMIN != 0 && poles & SS_VMAX != 0)
                    {
                        let old_region = face_region(b, f)?;
                        let old_vertex = b
                            .vertexuse(vu)?
                            .vertex
                            .ok_or_else(|| invalid("vertex loop without vertex"))?;
                        let new_surface = ignore_soft(surface.copy_and_add_analytics(k))?;
                        let (deleted, edits) = delete_face(k, b, f, true, Some(old_region));
                        changes.append(edits);
                        let deleted = ignore_soft(deleted)?.unwrap_or_default();
                        let vertex_deleted = deleted.contains(&EntityRef::Vertex(old_vertex));
                        // A failed copy/create has no returned face to dereference.
                        let Some(new_surface) = new_surface else {
                            continue;
                        };
                        let (made, edits) = create_face_from_surface(k, b, old_region, new_surface);
                        changes.append(edits);
                        let Some(new_face) = ignore_soft(made)? else {
                            continue;
                        };
                        self.new_faces_from_fix_degen_faces.push(new_face);
                        if !vertex_deleted {
                            for v in face_vertices(b, new_face)? {
                                let tol = xsect_tol_3d_from_zones(
                                    k,
                                    vertex::tolerance(b, v)?,
                                    vertex::tolerance(b, old_vertex)?,
                                );
                                if (b.vertex(v)?.point - b.vertex(old_vertex)?.point).length() < tol
                                {
                                    changes.append(
                                        ignore_soft(glue_vertices(k, b, v, old_vertex))?
                                            .unwrap_or_default(),
                                    );
                                    break;
                                }
                            }
                        }
                    } else {
                        let (deleted, edits) = delete_face(k, b, f, false, None);
                        changes.append(edits);
                        ignore_soft(deleted)?;
                    }
                } else {
                    let (new_edges, deleted_edges, edits) =
                        ignore_soft(squeeze_face(k, b, f))?.unwrap_or_default();
                    changes.append(edits);
                    for e in new_edges {
                        if !edges.contains(&e) {
                            edges.push(e);
                        }
                    }
                    edges.retain(|e| !deleted_edges.contains(e));
                    let mut removed_duplicates = false;
                    let mut i = 0;
                    while let Some(&e) = edges.get(i) {
                        if edge::is_wire(b, e)? {
                            let a = edge::vertex(b, e)?
                                .ok_or_else(|| invalid("wire without vertex"))?;
                            let d = edge::other_vertex(b, e, Some(a))?
                                .ok_or_else(|| invalid("wire without other vertex"))?;
                            changes.append(ignore_soft(delete_edge(k, b, e))?.unwrap_or_default());
                            if a == d && vertex::is_shell_vertex(b, a)? {
                                changes.append(
                                    ignore_soft(delete_vertex(k, b, a))?.unwrap_or_default(),
                                );
                            } else {
                                if vertex::is_shell_vertex(b, a)? {
                                    changes.append(
                                        ignore_soft(delete_vertex(k, b, a))?.unwrap_or_default(),
                                    );
                                }
                                if vertex::is_shell_vertex(b, d)? {
                                    changes.append(
                                        ignore_soft(delete_vertex(k, b, d))?.unwrap_or_default(),
                                    );
                                }
                            }
                            if !removed_duplicates {
                                remove_duplicates_keep_last(&mut edges);
                                removed_duplicates = true;
                            }
                        }
                        i += 1;
                    }
                }
            }
            self.update_target_lists(
                k,
                b,
                &changes.removed,
                &changes.added,
                HealerOpType::FixDegenFaces,
            )
        })
    }

    /// Tries moving each seam-problem face once, in reverse problem-list order.
    /// A tail update selects problems directly from the new props instead.
    pub fn fix_move_seam(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        marks: &mut Marks,
        lock: &MarkLock,
        _op: HealerOpType,
    ) -> Result<()> {
        let mut queue = Vec::new();
        if self.old_face_cnt != 0 {
            for &id in self.tgt_face_props.iter().skip(self.old_face_cnt) {
                let p = &self.face_props.props(id)?.props;
                // SmBooleanUL tests nonzero (UNSURE is nonzero too).
                if seam_problem(p) {
                    queue.push(id);
                }
            }
        } else {
            queue.clone_from(&self.bad_face_props_missing_seam.prob_array);
            for &id in self
                .bad_face_props_crossed_seam
                .prob_array
                .iter()
                .chain(&self.bad_face_props_near_miss_seam.prob_array)
            {
                if !queue.contains(&id) {
                    queue.push(id);
                }
            }
        }
        if queue.is_empty() {
            return Ok(());
        }
        lock.new_mark(std::slice::from_mut(&mut k.context))?;
        let mut changes = Changes::default();
        for id in queue.into_iter().rev() {
            let p = &mut self.face_props.props_mut(id)?.props;
            let f = p.face.ok_or_else(|| invalid("seam props without face"))?;
            if marks.is_marked(k, Entity::Face(f), lock.mark_type()) {
                continue;
            }
            marks.mark(k, Entity::Face(f), lock.mark_type());
            // MoveSeam(TRUE /* move inside */, FALSE /* do not split */, props).
            let (res, edits) = move_seam(k, b, f, p, true, false);
            changes.append(edits);
            ignore_soft(res)?;
        }
        self.update_target_lists(
            k,
            b,
            &changes.removed,
            &changes.added,
            HealerOpType::FixMoveseam,
        )?;
        for &id in &self.tgt_face_props {
            let p = &self.face_props.props(id)?.props;
            let f = p.face.ok_or_else(|| invalid("seam props without face"))?;
            if marks.is_marked(k, Entity::Face(f), lock.mark_type()) {
                let tried = if seam_problem(p) {
                    TriedLinkType::NoFix
                } else {
                    TriedLinkType::Fixed
                };
                self.ran_face_props_move_seam
                    .add(&mut self.face_props, id, tried)?;
            }
        }
        Ok(())
    }

    /// Splits edges crossing U and then V seams. Native marking deliberately
    /// visits the last direction's `sThisSplitEdges`, not their combined list.
    pub fn fix_split_edges_at_seam(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        marks: &mut Marks,
        lock: &MarkLock,
        _op: HealerOpType,
    ) -> Result<()> {
        let queue = self.bad_face_props_crossed_seam.prob_array.clone();
        if queue.is_empty() {
            return Ok(());
        }
        lock.new_mark(std::slice::from_mut(&mut k.context))?;
        let mut changes = Changes::default();
        // Upstream keeps both statuses and the last direction's edges across faces.
        let mut merge_ok = [true, true];
        let mut this_split_edges = Vec::new();
        for id in queue.into_iter().rev() {
            if !self.tgt_face_props.contains(&id) {
                continue;
            }
            let p = &mut self.face_props.props_mut(id)?.props;
            let Some(f) = p.face else {
                continue; // IsLiveTopologyMember(NULL) is false upstream.
            };
            if !bp1::is_live_topology_member(b, Some(EntityRef::Face(f)))
                || p.bad_crossed_seam.is_false()
            {
                continue;
            }
            if classification_has_dead_topology(b, p.crv_class_u.as_ref())?
                || classification_has_dead_topology(b, p.crv_class_v.as_ref())?
            {
                refresh_stage2(k, b, p, self.done_heal_op)?;
                if p.bad_crossed_seam.is_false() {
                    continue;
                }
            }
            if has_u(p.bad_crossed_seam_dir) {
                this_split_edges = classified_edges(p.crv_class_u.as_ref())?;
            }
            if has_v(p.bad_crossed_seam_dir) {
                this_split_edges = classified_edges(p.crv_class_v.as_ref())?;
            }
            let mut rebuild = false;
            for &e in &this_split_edges {
                if marks.is_marked(k, Entity::Edge(e), lock.mark_type()) {
                    rebuild = true;
                }
                marks.mark(k, Entity::Edge(e), lock.mark_type());
            }
            if rebuild {
                refresh_stage2(k, b, p, self.done_heal_op)?;
            }
            let dirs = [
                (has_u(p.bad_crossed_seam_dir), true),
                (has_v(p.bad_crossed_seam_dir), false),
            ];
            for (run, u) in dirs {
                if !run {
                    continue;
                }
                let p = &mut self.face_props.props_mut(id)?.props;
                let class = if u {
                    p.crv_class_u.as_mut()
                } else {
                    p.crv_class_v.as_mut()
                }
                .ok_or_else(|| invalid("crossed seam without classification"))?;
                let (merged, success) =
                    match class.merge_intervals_with_partial(k, &NoGeometry, true) {
                        Ok(merged) => (merged, true),
                        Err(failure) => {
                            ignore_soft::<()>(Err(failure.error))?;
                            (*failure.merged, false)
                        }
                    };
                if let Some(status) = merge_ok.get_mut(usize::from(!u)) {
                    *status = success;
                }
                // Native retains lists appended before an ignored merge failure.
                // Opaque handles still require the topology adapter's resolver.
                let edits = merged_changes(b, &merged)?;
                self.new_edges_from_split_edges_at_seams.extend(
                    edits.added.iter().flatten().filter_map(|e| {
                        if let EntityRef::Edge(x) = e {
                            Some(*x)
                        } else {
                            None
                        }
                    }),
                );
                self.new_vertices_from_split_edges_at_seams.extend(
                    edits.added.iter().flatten().filter_map(|e| {
                        if let EntityRef::Vertex(x) = e {
                            Some(*x)
                        } else {
                            None
                        }
                    }),
                );
                changes.append(edits);
                self.ran_face_props_split_edges_at_seam.add(
                    &mut self.face_props,
                    id,
                    TriedLinkType::Ran,
                )?;
                // End the arena borrow before adding the tried record.
                // The next direction obtains the classification again below.
            }
            if merge_ok == [true, true] {
                let p = &mut self.face_props.props_mut(id)?.props;
                p.bad_crossed_seam_dir = SurfParamType::Neither;
                // Test the old flags before clearing the crossed-seam flag.
                p.been_through_split_edges_at_seam = if seam_problem(p) {
                    TriedLinkType::NoFix
                } else {
                    TriedLinkType::Fixed
                };
                p.bad_crossed_seam = crate::heal::face_props::Tri::False;
            }
        }
        self.update_target_lists(
            k,
            b,
            &changes.removed,
            &changes.added,
            HealerOpType::FixSplitedgeAtseam,
        )
    }

    /// Inserts missing seam edges and records the topology created by splitting.
    /// Native code updates targets through `FixSplitedgeAtseam` and records tried
    /// outcomes in the split-edge list; preserve both details.
    pub fn fix_split_face_at_seams(
        &mut self,
        k: &mut Kernel,
        b: &mut Brep,
        _op: HealerOpType,
    ) -> Result<()> {
        let queue = self.bad_face_props_missing_seam.prob_array.clone();
        if queue.is_empty() {
            return Ok(());
        }
        let mut changes = Changes::default();
        for id in queue.into_iter().rev() {
            if !self.tgt_face_props.contains(&id) {
                continue;
            }
            let p = &mut self.face_props.props_mut(id)?.props;
            let Some(f) = p.face else {
                continue; // IsLiveTopologyMember(NULL) is false upstream.
            };
            if !bp1::is_live_topology_member(b, Some(EntityRef::Face(f)))
                || p.bad_missing_seam.is_false()
            {
                continue;
            }
            if classification_has_dead_topology(b, p.crv_class_u.as_ref())?
                || classification_has_dead_topology(b, p.crv_class_v.as_ref())?
            {
                refresh_stage2(k, b, p, self.done_heal_op)?;
                if p.bad_missing_seam.is_false() {
                    continue;
                }
            }
            let (new_faces, edits) =
                ignore_soft(split_face_at_seam(k, b, f, p))?.unwrap_or_default();
            changes.append(edits);
            if new_faces.is_empty() {
                self.ran_face_props_split_edges_at_seam.add(
                    &mut self.face_props,
                    id,
                    TriedLinkType::NoFix,
                )?;
            } else {
                self.ran_face_props_split_edges_at_seam
                    .base
                    .inc_orig_prob_count();
            }
        }
        for e in changes.added.iter().flatten() {
            match *e {
                EntityRef::Face(x) => self.new_faces_from_split_face_at_seams.push(x),
                EntityRef::Edge(x) => self.new_edges_on_seams_from_split_face_at_seams.push(x),
                EntityRef::Vertex(x) => self.new_vertices_from_split_face_at_seams.push(x),
                _ => (),
            }
        }
        self.update_target_lists(
            k,
            b,
            &changes.removed,
            &changes.added,
            HealerOpType::FixSplitedgeAtseam,
        )
    }
}

fn remove_duplicates_keep_last<T: Copy + PartialEq>(items: &mut Vec<T>) {
    let mut unique = Vec::new();
    for &item in items.iter().rev() {
        if !unique.contains(&item) {
            unique.push(item);
        }
    }
    unique.reverse();
    *items = unique;
}

fn seam_problem<O: LoopObject>(p: &FaceProps<O>) -> bool {
    !p.bad_missing_seam.is_false()
        || !p.bad_crossed_seam.is_false()
        || !p.bad_near_miss_seam.is_false()
}
fn has_u(d: SurfParamType) -> bool {
    matches!(d, SurfParamType::U | SurfParamType::Both)
}
fn has_v(d: SurfParamType) -> bool {
    matches!(d, SurfParamType::V | SurfParamType::Both)
}
fn refresh_stage2<O: LoopObject>(
    k: &Kernel,
    b: &Brep,
    p: &mut FaceProps<O>,
    done: HealerOpType,
) -> Result<()> {
    p.reset(PropStage::Stage2);
    crate::error::ignore_status(p.set_props(
        k,
        b,
        PropStage::Stage2,
        p.face.ok_or_else(|| invalid("props without face"))?,
        p.face_index,
        done,
    ))
}
fn face_edges(b: &Brep, f: FaceId) -> Result<Vec<EdgeId>> {
    crate::topology::face::p4::edges(b, f, None, false, None)
}
fn face_vertices(b: &Brep, f: FaceId) -> Result<Vec<VertexId>> {
    crate::topology::face::p4::vertices(b, f, None, false, None)
}
fn face_region(b: &Brep, f: FaceId) -> Result<RegionId> {
    let fu = face::upward_faceuse(b, f)?.ok_or_else(|| invalid("face without upward use"))?;
    let s = b
        .faceuse(fu)?
        .shell
        .ok_or_else(|| invalid("face use without shell"))?;
    b.shell(s)?
        .region
        .ok_or_else(|| invalid("shell without region"))
}
/// `DeleteFace(f, TRUE, nest_regions, &deleted, keep)` under the edit tracker
/// ([`tracked_edit`]): the deleted edges and vertices (`pOptDeletedEdgesVertices`,
/// which `FixDegenFaces` reads as `sDelObjs`) or the status, and the tracked changes.
fn delete_face(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    nest_regions: bool,
    keep: Option<RegionId>,
) -> (Result<Vec<EntityRef>>, Changes) {
    let opts = crate::topology::brep::p3::DeleteFaceOptions {
        delete_edges_vertices: true,
        do_region_nesting: nest_regions,
        region_to_keep: keep,
    };
    let mut marks = Marks::default();
    tracked_edit(k, b, |k, b, env| {
        crate::topology::brep::p3::delete_face(k, b, env, &mut marks, f, opts)
    })
}
/// `CreateFaceInRegionFromSurface(region, surface, surface->GetNaturalUVDomain(), ..)`
/// (unit `topology.brep_p8`) under the edit tracker ([`tracked_edit`]): the new face or
/// the status, and the tracked changes (also those of a failed call). The face owns its surface under its edit identity.
fn create_face_from_surface(
    k: &mut Kernel,
    b: &mut Brep,
    r: RegionId,
    s: Surface,
) -> (Result<FaceId>, Changes) {
    let dom = s.natural_uv_domain();
    let mut marks = Marks::default();
    let (made, changes) = tracked_edit(k, b, |k, b, env| {
        let key_of = env.key_of;
        let this_face = move |f: FaceId| key_of(EntityRef::Face(f));
        let mut fenv = crate::topology::brep::p8::FaceBuildEnv {
            env,
            marks: &mut marks,
            this_face: &this_face,
        };
        crate::topology::brep::p8::create_face_in_region_from_surface(
            k,
            b,
            Some(r),
            Some(s),
            &dom,
            &mut fenv,
        )
    });
    (made.map(|m| m.face), changes)
}
fn glue_vertices(k: &mut Kernel, b: &mut Brep, a: VertexId, d: VertexId) -> Result<Changes> {
    let ((), changes) = capture_changes(k, b, |k, b| {
        crate::topology::brep::p5::glue_vertices(k, b, Some(a), Some(d), None)
    })?;
    Ok(changes)
}
fn squeeze_face(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
) -> Result<(Vec<EdgeId>, Vec<EdgeId>, Changes)> {
    let (output, changes) = capture_changes(k, b, |k, b| {
        let mut output = crate::topology::brep::p5::SqueezedFace::default();
        // The enclosing healer edit session enables editing upstream. SqueezeFace
        // receives NULL for its deleted-vertices output at this call site.
        crate::topology::brep::p5::squeeze_face(k, b, true, f, None, &mut output)?;
        Ok(output)
    })?;
    Ok((output.new_edges, output.deleted_edges, changes))
}

/// `SmBrep::DeleteVertex(vertex)` under the edit tracker ([`tracked_edit`]).
fn delete_vertex(k: &mut Kernel, b: &mut Brep, v: VertexId) -> Result<Changes> {
    let (r, changes) = tracked_edit(k, b, |k, b, env| {
        p4::delete_vertex(k, b, &mut p4::DeleteEnv::from_notify(env), v)
    });
    r.map(|()| changes)
}
/// The face as the owner of its surface (`this` of the face's members): the surface's
/// owner, or the face's arena identity when it has none.
fn face_ref(b: &Brep, f: FaceId) -> Result<ObjectRef> {
    Ok(b.face(f)?
        .surface
        .as_ref()
        .and_then(Surface::owner)
        .unwrap_or_else(|| arena_identity(CacheObject::Face(f))))
}

/// The seam problem of the face properties (`m_eBadCrossedSeam`, `m_eBadMissingSeam`,
/// `m_eBadNearMissSeam`).
fn props_seam_problem<O: LoopObject>(p: &FaceProps<O>) -> SeamProblem {
    SeamProblem {
        crossed: p.bad_crossed_seam_dir,
        missing: p.bad_missing_seam_dir,
        near_miss: p.bad_near_miss_seam_dir,
    }
}

/// Classification geometry that resolves nothing: the seam members read it only without
/// face properties, and the healer always passes them.
struct NoGeometry;

impl<O> ClassGeometry<O> for NoGeometry {
    fn curve(&self, _: O) -> Option<&Curve> {
        None
    }
    fn surface(&self, _: O) -> Option<&Surface> {
        None
    }
    fn volume(&self, _: O) -> Option<&Volume> {
        None
    }
    fn point(&self, _: O) -> Option<Point3> {
        None
    }
}

/// Runs `op` on the face properties' two classifications (`*m_pCrvClassU`,
/// `*m_pCrvClassV`, dereferenced unchecked upstream), lent out of `p` for the call.
fn with_classes<O: LoopObject, T>(
    p: &mut FaceProps<O>,
    op: impl FnOnce(
        &mut CurveClassification<O>,
        &mut CurveClassification<O>,
        &FaceProps<O>,
    ) -> Result<T>,
) -> Result<T> {
    let (Some(mut cu), Some(mut cv)) = (p.crv_class_u.take(), p.crv_class_v.take()) else {
        return Err(invalid("seam props without classifications"));
    };
    let r = op(&mut cu, &mut cv, p);
    p.crv_class_u = Some(cu);
    p.crv_class_v = Some(cv);
    r
}

/// `pThisFace->MoveSeam(eU, eV, *m_pCrvClassU, *m_pCrvClassV, m_eBadCrossedSeam,
/// m_eBadMissingSeam, m_eBadNearMissSeam, bOkToMoveToCenter, bOkToSplit, NULL, NULL, NULL,
/// pThisFaceProps)` under [`tracked_edit`]; upstream ignores the plan results (debug
/// output only) and keeps no new split topology, while `ReplaceSurface`'s
/// `SM_NO_POST_EDIT` marks the face removed and re-added in the tracker.
fn move_seam<O: LoopObject>(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    p: &mut FaceProps<O>,
    move_inside: bool,
    split: bool,
) -> (Result<()>, Changes) {
    let this = match face_ref(b, f) {
        Ok(r) => r,
        Err(e) => return (Err(e), Changes::default()),
    };
    let problem = props_seam_problem(p);
    let (res, changes) = tracked_edit(k, b, |k, b, env| {
        with_classes(p, |cu, cv, p| {
            fp1::move_seam_notified(
                k,
                b,
                Some(env),
                f,
                this,
                cu,
                cv,
                problem,
                fp1::MoveSeamOptions {
                    ok_to_move_to_center: move_inside,
                    ok_to_split: split,
                },
                Some(p),
                // (no new topology without split; a handle would be InvalidInput)
                &|_| None,
            )
        })
    });
    (res.map(|_| ()), changes)
}
/// Classification geometry that resolves topology handles to the healed B-rep and
/// nothing else: `HasDeadTopologyPointObject` only asks where a vertex, edge or face
/// point classification's object lives (`SmBrep::IsLiveTopologyMember`).
struct BrepTopology<'a>(&'a Brep);

impl<O: LoopObject> ClassGeometry<O> for BrepTopology<'_> {
    fn curve(&self, _: O) -> Option<&Curve> {
        None
    }
    fn surface(&self, _: O) -> Option<&Surface> {
        None
    }
    fn volume(&self, _: O) -> Option<&Volume> {
        None
    }
    fn point(&self, _: O) -> Option<Point3> {
        None
    }
    fn topology_entity(&self, o: O) -> Option<(&Brep, EntityRef)> {
        o.as_topology().map(|e| (self.0, e))
    }
    /// The healed B-rep is the only one the handles reach: a constant id (the first);
    /// any other B-rep has none.
    fn brep_id(&self, brep: &Brep) -> Option<crate::topology::BrepId> {
        core::ptr::eq(brep, self.0).then_some(crate::topology::BrepId::from_index(0))
    }
}

fn classification_has_dead_topology<O: LoopObject>(
    b: &Brep,
    class: Option<&CurveClassification<O>>,
) -> Result<bool> {
    match class {
        None => Ok(false),
        Some(class) => class.has_dead_topology_point_object(&BrepTopology(b), Some(b)),
    }
}
fn classified_edges<O: LoopObject>(class: Option<&CurveClassification<O>>) -> Result<Vec<EdgeId>> {
    let class = class.ok_or_else(|| invalid("crossed seam without classification"))?;
    let edges = class.find_edge_objects(&NoGeometry, None, 2)?;
    if edges.is_empty() {
        Ok(Vec::new())
    } else {
        Err(invalid(
            "classified edge handles require a typed B-rep resolver",
        ))
    }
}
fn merged_changes<O: LoopObject>(
    _b: &Brep,
    merged: &crate::intersect::curve_class::p5::MergedTopology<O>,
) -> Result<Changes> {
    if merged.faces.is_empty() && merged.edges.is_empty() && merged.vertices.is_empty() {
        return Ok(Changes::default());
    }
    // Classification handles have caller-chosen meaning (ClassObject). The
    // current MergeIntervals implementation cannot return newly created topology
    // before its missing classification callees are wired. Refuse unresolved
    // handles rather than inventing ids when that implementation is extended.
    Err(invalid(
        "merged topology handles require a typed B-rep resolver",
    ))
}
/// `pThisFace->SplitAtSeam(*m_pCrvClassU, *m_pCrvClassV, m_eBadCrossedSeam,
/// m_eBadMissingSeam, m_eBadNearMissSeam, &sThisNewFaces, NULL, NULL, pThisFaceProps)`:
/// the new faces, and the new faces, edges and vertices as added topology. Classification
/// handles have no B-rep meaning here (as in [`merged_changes`]): a merge that makes
/// topology is `InvalidInput` from the resolver, never an invented id.
fn split_face_at_seam<O: LoopObject>(
    k: &mut Kernel,
    b: &mut Brep,
    f: FaceId,
    p: &mut FaceProps<O>,
) -> Result<(Vec<FaceId>, Changes)> {
    let problem = props_seam_problem(p);
    let split = with_classes(p, |cu, cv, p| {
        fp1::split_at_seam(k, b, f, cu, cv, problem, Some(p), &|_| None)
    })?;
    let added = split
        .faces
        .iter()
        .map(|&x| EntityRef::Face(x))
        .chain(split.edges.iter().map(|&x| EntityRef::Edge(x)))
        .chain(split.vertices.iter().map(|&x| EntityRef::Vertex(x)))
        .map(Some)
        .collect();
    Ok((
        split.faces,
        Changes {
            removed: Vec::new(),
            added,
        },
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::heal::face_props::Tri;
    use crate::intersect::point_class::{ClassObject, ClassObjectKind};
    use crate::topology::LoopId;

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct L(LoopId);
    impl ClassObject for L {
        fn class_kind(&self) -> ClassObjectKind {
            ClassObjectKind::Loop
        }
    }
    impl LoopObject for L {
        fn from_loop(id: LoopId) -> Self {
            Self(id)
        }
    }

    /// A handle naming a vertex or a loop.
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum T {
        Vertex(VertexId),
        Loop(LoopId),
    }
    impl ClassObject for T {
        fn class_kind(&self) -> ClassObjectKind {
            match self {
                Self::Vertex(_) => ClassObjectKind::Vertex,
                Self::Loop(_) => ClassObjectKind::Loop,
            }
        }
    }
    impl LoopObject for T {
        fn from_loop(id: LoopId) -> Self {
            Self::Loop(id)
        }
        fn as_topology(&self) -> Option<EntityRef> {
            match self {
                Self::Vertex(v) => Some(EntityRef::Vertex(*v)),
                Self::Loop(l) => Some(EntityRef::Loop(*l)),
            }
        }
    }

    #[test]
    fn brep_topology_names_the_healed_brep() {
        // `PointClassification::GetBrep` of a vertex point reaches the healed
        // B-rep's (constant) id; the skipped B-rep gives none
        use crate::intersect::point_class::{PointClassType, PointClassification};
        let mut k = Kernel::default();
        let b = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Vec3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        let v = bp1::vertices(&b, None).unwrap()[0];
        let topology = BrepTopology(&b);
        let mut pc = PointClassification::<T>::new(1e-6);
        pc.set_class_object(PointClassType::Vertex, Some(T::Vertex(v)));
        let id = crate::topology::BrepId::from_index(0);
        assert_eq!(pc.brep(&topology, None).unwrap(), Some(id));
        assert_eq!(pc.brep(&topology, Some(id)).unwrap(), None);
        let other = b.clone();
        assert_eq!(ClassGeometry::<T>::brep_id(&topology, &other), None);
    }

    #[test]
    fn catch_up_restores_state_after_failed_callee() {
        let mut k = Kernel::default();
        let mut b = Brep::default();
        // a face whose tolerance SmBrep::SetTolerance raises (SmFace::SetTolerance):
        // B-rep and face tolerances below the estimate
        let f = b
            .add_face_record(crate::topology::Face::new(
                crate::topology::ShellId::from_index(0),
            ))
            .unwrap();
        b.face_mut(f).unwrap().tolerance = 0.0;
        b.set_tolerance(0.0);
        let mut h = HealData::<L>::new();
        h.old_face_cnt = 7;
        h.old_edge_cnt = 8;
        h.old_vertex_cnt = 9;
        h.stop_heal_op = HealerOpType::All;
        h.done_heal_op = HealerOpType::FixTolsizes;
        h.recursion_count = 3;
        // Even a null add entry enters native catch-up. Fix_TolSizes still sets the
        // B-rep and face tolerances, despite empty target arrays.
        h.update_target_lists(&mut k, &mut b, &[], &[None], HealerOpType::All)
            .unwrap();
        assert_eq!((b.tolerance(), b.face(f).unwrap().tolerance), (1e-5, 1e-5));
        assert_eq!(
            (h.old_face_cnt, h.old_edge_cnt, h.old_vertex_cnt),
            (7, 8, 9)
        );
        assert_eq!(
            (h.stop_heal_op, h.done_heal_op),
            (HealerOpType::All, HealerOpType::FixTolsizes)
        );
        assert_eq!(h.recursion_count, 3);
        // A catch-up through Fix_InfiniteRegions fails on this region-less B-rep and
        // restores the same state.
        h.done_heal_op = HealerOpType::FixInfiniteRegions;
        let e = h
            .update_target_lists(&mut k, &mut b, &[], &[None], HealerOpType::All)
            .unwrap_err();
        assert!(matches!(e, SolidError::InvalidInput { .. }));
        assert_eq!(
            (h.old_face_cnt, h.old_edge_cnt, h.old_vertex_cnt),
            (7, 8, 9)
        );
        assert_eq!(
            (h.stop_heal_op, h.done_heal_op),
            (HealerOpType::All, HealerOpType::FixInfiniteRegions)
        );
        assert_eq!(h.recursion_count, 3);
        // Both nested and outer mark locks are released on a failed pass.
        let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS).unwrap();
        assert_eq!(lock.mark_type(), crate::geom::context::MarkType::Mark);
        lock.release(std::slice::from_mut(&mut k.context)).unwrap();
    }

    #[test]
    fn recursion_limit_handles_extreme_counts_without_overflow() {
        for depth in [MAX_TARGET_UPDATE_DEPTH, u64::MAX] {
            let mut h = HealData::<L>::new();
            h.recursion_count = depth;
            let r = h.update_target_lists(
                &mut Kernel::default(),
                &mut Brep::default(),
                &[],
                &[None],
                HealerOpType::None,
            );
            assert!(matches!(r, Err(SolidError::InvalidInput { .. })));
            assert_eq!(h.recursion_count, depth);
        }
    }

    #[test]
    fn additions_skip_null_stale_and_unrelated_topology() {
        let mut k = Kernel::default();
        let mut b = Brep::default();
        let v = b
            .add_vertex_record(crate::topology::Vertex::default())
            .unwrap();
        let mut h = HealData::<L>::new();
        h.done_heal_op = HealerOpType::None;
        h.update_target_lists(
            &mut k,
            &mut b,
            &[],
            &[
                None,
                Some(EntityRef::Vertex(VertexId::from_index(u32::MAX))),
                Some(EntityRef::Vertex(v)),
            ],
            HealerOpType::All,
        )
        .unwrap();
        assert_eq!(h.tgt_vertices, vec![v]);
        assert_eq!(h.tgt_vertex_props.len(), 1);
        assert_eq!(
            h.vertex_props.get(h.tgt_vertex_props[0]).unwrap().vertex,
            None
        );
        assert_eq!(h.recursion_count, 0);
    }

    #[test]
    fn removal_frees_props_and_cleans_problem_lists() {
        let mut b = Brep::default();
        let v = b
            .add_vertex_record(crate::topology::Vertex::default())
            .unwrap();
        let mut h = HealData::<L>::with_targets(&[v], &[], &[]);
        let id = h.tgt_vertex_props[0];
        h.vertex_props.get_mut(id).unwrap().vertex = Some(v);
        h.bad_vertex_props_small_zone_tol_3d
            .add(&mut h.vertex_props, id, true)
            .unwrap();
        h.update_target_lists(
            &mut Kernel::default(),
            &mut b,
            &[EntityRef::Vertex(v)],
            &[],
            HealerOpType::All,
        )
        .unwrap();
        assert!(h.tgt_vertices.is_empty());
        assert!(h.tgt_vertex_props.is_empty());
        assert!(h.bad_vertex_props_small_zone_tol_3d.is_empty());
        assert!(h.vertex_props.get(id).is_none());
    }

    #[test]
    fn coincident_snapshot_skips_self_pairs_and_retired_props() {
        let mut h = HealData::<L>::new();
        let a = h.vertex_props.alloc(VertexProps::new());
        let d = h.vertex_props.alloc(VertexProps::new());
        h.tgt_vertex_props.push(a);
        h.bad_vertex_props_coin_vertices.base.prob_array = vec![a, a, a, d];
        h.fix_coin_vertices(
            &mut Kernel::default(),
            &mut Brep::default(),
            HealerOpType::FixCoinVertices,
        )
        .unwrap();
    }

    #[test]
    fn false_problem_flags_do_not_trigger_topology_mutations() {
        let mut b = Brep::default();
        let e = b
            .add_edge_record(crate::topology::Edge::new([VertexId::from_index(0); 2]))
            .unwrap();
        let mut h = HealData::<L>::new();
        let mut p = EdgeProps::new();
        p.edge = Some(e);
        p.bad_degen_edge = Tri::False;
        let id = h.edge_props.alloc(p);
        h.bad_edge_props_degen_edges.prob_array.push(id);
        h.fix_degen_edges(&mut Kernel::default(), &mut b, HealerOpType::FixDegenEdges)
            .unwrap();
    }
    #[test]
    fn wire_dedup_retains_last_occurrences_in_original_order() {
        let mut edges = vec![1, 2, 1, 3, 2, 4, 3];
        remove_duplicates_keep_last(&mut edges);
        assert_eq!(edges, [1, 2, 4, 3]);
    }

    #[test]
    fn null_faces_are_skipped_in_all_live_member_guarded_repairs() {
        let mut k = Kernel::default();
        let mut b = Brep::default();
        let mut h = HealData::<L>::new();
        let id = h.face_props.alloc(FacePropsEntry::new());
        h.tgt_face_props.push(id);
        h.bad_face_props_degen_faces
            .add(&mut h.face_props, id, true)
            .unwrap();
        h.bad_face_props_crossed_seam
            .add(&mut h.face_props, id, true)
            .unwrap();
        h.bad_face_props_missing_seam
            .add(&mut h.face_props, id, true)
            .unwrap();
        h.fix_degen_faces(&mut k, &mut b, HealerOpType::FixDegenFaces)
            .unwrap();
        let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS).unwrap();
        h.fix_split_edges_at_seam(
            &mut k,
            &mut b,
            &mut Marks::default(),
            &lock,
            HealerOpType::FixSplitedgeAtseam,
        )
        .unwrap();
        lock.release(std::slice::from_mut(&mut k.context)).unwrap();
        h.fix_split_face_at_seams(&mut k, &mut b, HealerOpType::FixSplitfaceAtseams)
            .unwrap();
        assert_eq!(
            h.face_props.props(id).unwrap().props.bad_crossed_seam,
            Tri::True
        );
    }

    #[test]
    fn neither_direction_still_updates_crossed_seam_flags_in_native_order() {
        let mut k = Kernel::default();
        let mut b = Brep::default();
        let f = b
            .add_face_record(crate::topology::Face::new(
                crate::topology::ShellId::from_index(0),
            ))
            .unwrap();
        let mut h = HealData::<L>::with_targets(&[], &[], &[f]);
        let id = h.tgt_face_props[0];
        let p = &mut h.face_props.props_mut(id).unwrap().props;
        p.face = Some(f);
        p.bad_crossed_seam_dir = SurfParamType::Neither;
        p.bad_missing_seam = Tri::False;
        p.bad_near_miss_seam = Tri::False;
        h.bad_face_props_crossed_seam
            .add(&mut h.face_props, id, true)
            .unwrap();
        let mut lock = MarkLock::new(std::slice::from_mut(&mut k.context), 0, ALL_MARKS).unwrap();
        h.fix_split_edges_at_seam(
            &mut k,
            &mut b,
            &mut Marks::default(),
            &lock,
            HealerOpType::FixSplitedgeAtseam,
        )
        .unwrap();
        lock.release(std::slice::from_mut(&mut k.context)).unwrap();
        let p = &h.face_props.props(id).unwrap().props;
        assert_eq!(p.bad_crossed_seam_dir, SurfParamType::Neither);
        assert_eq!(p.bad_crossed_seam, Tri::False);
        // FIXED is decided before clearing the old crossed-seam flag.
        assert_eq!(p.been_through_split_edges_at_seam, TriedLinkType::NoFix);
    }

    #[test]
    fn missing_edge_has_native_sm_err_status() {
        let mut h = HealData::<L>::new();
        let id = h.edge_props.alloc(EdgeProps::new());
        h.bad_edge_props_degen_edges.prob_array.push(id);
        assert!(matches!(
            h.fix_degen_edges(
                &mut Kernel::default(),
                &mut Brep::default(),
                HealerOpType::FixDegenEdges
            ),
            Err(SolidError::Upstream { code: SM_ERR, .. })
        ));
    }

    /// The wire chain A--B--C without curves (`topology::brep::p3`'s test fixture).
    fn wire_chain() -> (Brep, [VertexId; 3], [EdgeId; 2]) {
        crate::topology::brep::p3::test_support::wire_chain(false)
    }

    /// The healer's `SqueezeEdge` completes on an ordinary wire chain: its tracked
    /// edit seeds every object's `SmTopology` record, so `SM_NO_MERGE_IN_BREP` (which
    /// reads both vertices') succeeds, and the tracker reports the edit
    /// (`SmTopologyChangeCallback::Execute`).
    #[test]
    fn healer_squeeze_of_a_wire_chain_completes_under_its_tracker() {
        let (mut b, [a, bv, _], [e0, _]) = wire_chain();
        let mut k = Kernel::default();
        let changes = squeeze_edge(&mut k, &mut b, e0, a).unwrap();
        assert_eq!(
            changes.removed,
            vec![
                EntityRef::Edge(e0),
                EntityRef::Vertex(a),
                EntityRef::Vertex(bv)
            ]
        );
        assert_eq!(changes.added, vec![Some(EntityRef::Vertex(a))]);
        // the squeezed edge's uses and their vertex uses are gone; the deleted vertex's
        // other use (of the second edge) moved to the survivor
        assert!(b.edge(e0).unwrap().edgeuses.is_empty());
        assert_eq!(b.vertex(a).unwrap().vertexuses.len(), 1);
    }

    /// A tracked edit nested in a lent edit is refused before anything changes
    /// (`topology::brep::p3::with_lent_context`), and tracks nothing; once the outer
    /// lend is over, a tracked edit runs.
    #[test]
    fn nested_tracked_edits_are_refused() {
        use crate::topology::brep::p3 as brep_p3;
        let (mut b, [a, _, _], [e0, _]) = wire_chain();
        let mut k = Kernel::default();
        let r = brep_p3::with_kernel_callbacks(&mut k, &mut b, |k, b, _| {
            let (r, changes) =
                tracked_edit(k, b, |k, b, env| brep_p3::squeeze_edge(k, b, env, e0, a));
            assert!(changes.removed.is_empty() && changes.added.is_empty());
            r
        });
        assert!(matches!(r, Err(SolidError::InvalidInput { .. })), "{r:?}");
        assert!(!b.edge(e0).unwrap().edgeuses.is_empty());
        assert!(squeeze_edge(&mut k, &mut b, e0, a).is_ok());
    }

    /// The A--B--C chain of [`wire_chain`] with line curves and AB's uses reversed (its
    /// first edge use at B, so `GetVertex`, the vertex `FixDegenEdges` keeps, is B), a
    /// kernel with a recording user callback, and healers queueing AB and BC.
    #[allow(clippy::type_complexity)]
    fn two_healer_setup() -> (
        Brep,
        Kernel,
        std::sync::Arc<std::sync::Mutex<Vec<ObjectKey>>>,
        HealData<L>,
        HealData<L>,
    ) {
        use crate::curve::curve::Curve;
        use crate::curve::line::Line;
        use crate::geom::context::{NotifyCallback, GEOMETRY_KEY_BASE};
        use crate::geom::object::{NotifyOperation, ObjectRef};
        use crate::heal::face_props::Tri;
        use crate::heal::heal_data::p2::EdgeProps;
        use crate::topology::VertexuseId;
        use std::sync::{Arc, Mutex};

        #[derive(Debug)]
        struct Rec(Arc<Mutex<Vec<ObjectKey>>>);
        impl NotifyCallback for Rec {
            fn execute(
                &mut self,
                _operation: NotifyOperation,
                caller: ObjectRef,
                data1: Option<ObjectRef>,
                data2: Option<ObjectRef>,
                data3: Option<ObjectRef>,
            ) -> Result<()> {
                let mut seen = self.0.lock().unwrap();
                for x in [Some(caller), data1, data2, data3].into_iter().flatten() {
                    if x.key.0 >= GEOMETRY_KEY_BASE {
                        seen.push(x.key);
                    }
                }
                Ok(())
            }
        }
        fn queue(e: EdgeId) -> HealData<L> {
            let mut h = HealData::<L>::new();
            let mut p = EdgeProps::for_edge(e, u64::from(e.index()));
            p.bad_degen_edge = Tri::True;
            let id = h.edge_props.alloc(p);
            h.bad_edge_props_degen_edges.prob_array = vec![id];
            h
        }

        let (mut b, [a, bv, c], [e0, e1]) = wire_chain();
        // reverse AB: edge use 0 (SAME) at B, edge use 1 at A
        let (vu0, vu1) = (VertexuseId::from_index(0), VertexuseId::from_index(1));
        b.vertexuse_mut(vu0).unwrap().vertex = Some(bv);
        b.vertexuse_mut(vu1).unwrap().vertex = Some(a);
        b.vertex_mut(a).unwrap().vertexuses = vec![vu1];
        b.vertex_mut(bv).unwrap().vertexuses = vec![vu0, VertexuseId::from_index(2)];
        b.edge_mut(e0).unwrap().vertices = [bv, a];
        assert_eq!(edge::start_end_vertices(&b, e0).unwrap().0, Some(bv));
        for (e, p, q) in [(e0, bv, a), (e1, bv, c)] {
            let (pp, qq) = (b.vertex(p).unwrap().point, b.vertex(q).unwrap().point);
            b.edge_mut(e).unwrap().curve = Some(Curve::from(Line::from_points(pp, qq, 3)));
        }
        let mut k = Kernel::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        k.context
            .set_user_notify_callback(Some(Box::new(Rec(Arc::clone(&seen)))));
        (b, k, seen, queue(e0), queue(e1))
    }

    /// The two edits' curve keys, as the callback saw them.
    fn keys_of_two_edits(
        seen: &std::sync::Mutex<Vec<ObjectKey>>,
        first_len: usize,
    ) -> (Vec<ObjectKey>, Vec<ObjectKey>) {
        let all = seen.lock().unwrap().clone();
        (all[..first_len].to_vec(), all[first_len..].to_vec())
    }

    /// Curve identities outlive every healer: through the public `fix_degen_edges`, H1
    /// squeezing AB while keeping B (refused with `SM_ERR` after its removal
    /// notification, both curves still live), then a fresh H2 squeezing BC (keeping B,
    /// succeeds), notify the two curves under distinct keys.
    #[test]
    fn curve_identities_survive_a_fresh_healer() {
        let (mut b, mut k, seen, mut h1, mut h2) = two_healer_setup();
        let r1 = h1.fix_degen_edges(&mut k, &mut b, HealerOpType::FixDegenEdges);
        assert!(
            matches!(r1, Err(SolidError::Upstream { code: SM_ERR, .. })),
            "{r1:?}"
        );
        let first = seen.lock().unwrap().len();
        h2.fix_degen_edges(&mut k, &mut b, HealerOpType::FixDegenEdges)
            .unwrap();
        let (one, two) = keys_of_two_edits(&seen, first);
        assert!(!one.is_empty() && !two.is_empty());
        assert!(two.iter().all(|x| !one.contains(x)), "{one:?} {two:?}");
    }

    /// The same with a clone of H1 made before H1's edit (re-queued on BC): cloned
    /// healer state cannot restart the identities either.
    #[test]
    fn curve_identities_survive_a_cloned_healer() {
        let (mut b, mut k, seen, mut h1, h2) = two_healer_setup();
        let mut h1_clone = h1.clone();
        h1_clone.edge_props = h2.edge_props.clone();
        h1_clone.bad_edge_props_degen_edges = h2.bad_edge_props_degen_edges.clone();
        let r1 = h1.fix_degen_edges(&mut k, &mut b, HealerOpType::FixDegenEdges);
        assert!(
            matches!(r1, Err(SolidError::Upstream { code: SM_ERR, .. })),
            "{r1:?}"
        );
        let first = seen.lock().unwrap().len();
        h1_clone
            .fix_degen_edges(&mut k, &mut b, HealerOpType::FixDegenEdges)
            .unwrap();
        let (one, two) = keys_of_two_edits(&seen, first);
        assert!(!one.is_empty() && !two.is_empty());
        assert!(two.iter().all(|x| !one.contains(x)), "{one:?} {two:?}");
    }

    #[test]
    fn aliased_glue_preserves_a_preexisting_tracker_without_invented_changes() {
        use crate::geom::context::NotifyCallback;
        use crate::topology::topology::TopologyChangeCallback;
        let mut k = Kernel::default();
        let mut b = Brep::new();
        k.context
            .set_sys_notify_callback(Some(Box::new(TopologyChangeCallback::new(27))));
        // Native GlueVertices compares identities before using the pointer.
        let v = VertexId::from_index(0);
        let changes = glue_vertices(&mut k, &mut b, v, v).unwrap();
        assert!(changes.removed.is_empty() && changes.added.is_empty());
        let callback = k
            .context
            .sys_notify_callback()
            .and_then(NotifyCallback::as_any)
            .and_then(|a| a.downcast_ref::<TopologyChangeCallback>())
            .unwrap();
        assert_eq!(callback.tracker(), 27);
        assert!(callback.lists().notify_events.is_empty());
    }
}
