//! Not upstream: a face of two edges whose offset is empty, taken out of the offset and its
//! two neighbours' offsets mated across it.
//!
//! Upstream: none (solidcore-native).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`OffsetExecutive::eliminate_empty_lenses`] | none |
//!
//! **The event.** A face P bounded by one loop of two edges (a lens: a notch's ceiling where it
//! meets a curved wall, a sliver between two faces), each edge between P and a neighbour (N1 and
//! N2, two faces), meets the neighbours at its two vertices, where only P, N1 and N2 meet. Its
//! offset P' is bounded by the two curves N1' ∩ P' and N2' ∩ P', which meet at the offsets of
//! the two vertices, the points of P', N1' and N2'. As the distance grows the lens's offset can
//! shrink to nothing: the two curves come together, touch and part, and the three offsets no
//! longer meet. P is eliminated by the offset, one of the changes of topology a shell meets
//! (Forsyth 1995): past it P has no offset face, and N1' and N2' meet each other directly
//! where P was, so the edges N1 ∩ N2 at the lens's two vertices continue into one another along
//! N1' ∩ N2'.
//!
//! **How it is decided.** Each offset edge's trim (`TrimExtendedEdge`) intersects its curve
//! with the extended offsets of the faces beside its ends. An edge of the lens whose curve meets
//! no neighbour's offset at either end has only the drops of its vertices for ends
//! (`TrimmedEdge::unreached`); where both edges of the lens are such, neither curve reaches the
//! third offset at either vertex, so the curves do not meet within the faces' extensions, and
//! the region they bound on P' is empty (it closes only at their meetings: the lens's offset has
//! no corners). The decision rests on those intersections, the trim's own, over extensions as
//! long as the trim's.
//!
//! **What is built.** The lens's offset faces are deleted with their relations, and the offsets
//! of its two edges on N1' and N2' are mated across it (`MateEdges` of the two neighbours'
//! edge uses, as WR-346 mates across a collapsed round): both take the curve N1' ∩ N2' from
//! where the offset of the edge N1 ∩ N2 at one of the lens's vertices ends to where the one at
//! the other vertex does. Those edges lie between the curve's own two faces, so their surfaces
//! cut nothing of it; the trim takes their mated ends instead (`TrimExtendedEdge`'s
//! `between_faces`). The result has a vertex of two edges at each: N1 ∩ N2, its continuation
//! across the lens, and N1 ∩ N2 again (or the same edge, where one edge N1 ∩ N2 joins the
//! lens's two vertices), on one curve.
//!
//! Deviations: none (no upstream counterpart). No recursion.
//!
//! Reference: M. Forsyth, "Shelling and offsetting bodies", Proc. Third ACM Symposium on Solid
//! Modeling and Applications (1995) 373–381.

use std::collections::BTreeSet;

use super::p1::{forget_seconds, OffsetExecutive};
use super::p2::OffsetNotifications;
use crate::error::{Result, SolidError};
use crate::geom::context::MarkType;
use crate::kernel::Kernel;
use crate::topology::brep::p1 as bp1;
use crate::topology::brep::p3::{delete_face_with_body_identity, DeleteFaceOptions};
use crate::topology::edge::{self, NotificationStores, NotifyState};
use crate::topology::edgeuse::p1 as eu;
use crate::topology::face::{p2 as face2, p4 as face4};
use crate::topology::{vertex, Brep, EdgeId, EdgeuseId, FaceId};

const OP: &str = "offset::offset_executive::empty_lens";

/// A face of `original` bounded by one loop of two edges, each shared with one other face
/// (two faces in all), whose two vertices are where only the three faces meet: its two edges
/// and its two neighbours. `None` for any other face.
fn lens(original: &Brep, p: FaceId) -> Result<Option<[(EdgeId, FaceId); 2]>> {
    let Some(fu) = face2::upward_faceuse(original, p)? else {
        return Ok(None);
    };
    let loopuses = original.faceuse(fu)?.loopuses.clone();
    let [lu] = loopuses.as_slice() else {
        return Ok(None);
    };
    let uses = eu::loopuse_edgeuses(original, *lu)?;
    let [u1, u2] = uses.as_slice() else {
        return Ok(None);
    };
    let mut sides = Vec::new();
    for &u in [u1, u2] {
        let e = eu::edge(original, u)?.ok_or_else(|| missing("edge use without edge"))?;
        if edge::start_vertex(original, e)? == edge::end_vertex(original, e)? {
            return Ok(None);
        }
        let faces = edge::faces(original, e, None)?;
        let [a, b] = faces.as_slice() else {
            return Ok(None);
        };
        let n = if *a == p { *b } else { *a };
        if n == p || (*a != p && *b != p) {
            return Ok(None);
        }
        sides.push((e, n));
    }
    let [(e1, n1), (e2, n2)] = sides.as_slice() else {
        return Ok(None);
    };
    if e1 == e2 || n1 == n2 {
        return Ok(None);
    }
    for v in [
        edge::start_vertex(original, *e1)?,
        edge::end_vertex(original, *e1)?,
    ] {
        let v = v.ok_or_else(|| missing("edge without vertex"))?;
        let mut at = vertex::faces(original, v)?;
        at.sort();
        at.dedup();
        let mut want = vec![p, *n1, *n2];
        want.sort();
        if at != want {
            return Ok(None);
        }
    }
    Ok(Some([(*e1, *n1), (*e2, *n2)]))
}

fn missing(why: &'static str) -> SolidError {
    SolidError::invalid_input(OP, why)
}

impl OffsetExecutive {
    /// Not upstream: takes out of the offset every face of two edges whose offset is empty
    /// (see the module notes) and mates its neighbours' offsets across it. Run once every
    /// edge is mated, before the loop gaps are closed.
    ///
    /// # Errors
    ///
    /// Those of the mating across the lens, the topology queries and `DeleteFace`.
    pub(super) fn eliminate_empty_lenses(
        &mut self,
        kernel: &mut Kernel,
        original: &mut Brep,
        mark_type: MarkType,
        marked: &BTreeSet<EdgeuseId>,
        notifications: &mut OffsetNotifications<'_>,
    ) -> Result<()> {
        if self.unreached_edgeuses.is_empty() {
            return Ok(());
        }
        let mut gone: BTreeSet<FaceId> = BTreeSet::new();
        let mut gone_list: Vec<FaceId> = Vec::new();
        for p in bp1::faces(original, None)? {
            if self.shell_faces().contains(&p) || self.is_collapsed_face(p) {
                continue;
            }
            let Some([(e1, n1), (e2, n2)]) = lens(original, p)? else {
                continue;
            };
            // the neighbours both move (an open face's offset is the face itself)
            if [n1, n2]
                .iter()
                .any(|n| self.shell_faces().contains(n) || self.is_collapsed_face(*n))
            {
                continue;
            }
            let reached = |e: EdgeId| -> Result<bool> {
                Ok(edge::edgeuses(original, e, None, None)?
                    .iter()
                    .all(|u| !self.unreached_edgeuses.contains(u)))
            };
            if reached(e1)? || reached(e2)? {
                continue;
            }
            // the neighbours' edge uses whose offsets were mated, with those offsets
            let offset_on = |e: EdgeId, n: FaceId| -> Result<Option<(EdgeuseId, EdgeuseId)>> {
                for u in edge::edgeuses(original, e, None, None)? {
                    if eu::face(original, u)? != Some(n) {
                        continue;
                    }
                    if let Some(off) = self.offset_edgeuse(u) {
                        return Ok(Some((u, off)));
                    }
                }
                Ok(None)
            };
            let (Some((u1, off1)), Some((u2, off2))) = (offset_on(e1, n1)?, offset_on(e2, n2)?)
            else {
                continue;
            };
            self.eliminated_lenses.insert(p);
            let mut stores = NotificationStores {
                records: &mut *notifications.records,
                attributes: &mut *notifications.attributes,
                key_of: notifications.original_key_of,
            };
            self.mate_edges_across_with_marked(
                kernel,
                original,
                u1,
                u2,
                off1,
                off2,
                mark_type,
                marked,
                &mut stores,
                notifications.result_key_of,
            )?;
            let (same, opposite) = face2::faceuses(original, p)?;
            for fu in [same, opposite].into_iter().flatten() {
                for f in self.faceuse_to_faces.seconds(fu) {
                    if gone.insert(f) {
                        gone_list.push(f);
                    }
                }
            }
        }
        if gone_list.is_empty() {
            return Ok(());
        }
        let mut offset = self
            .take_offset_brep()
            .ok_or_else(|| missing("no offset B-rep"))?;
        let r = delete_offset_faces(kernel, &mut offset, &gone_list, notifications);
        self.set_offset_brep(offset);
        let gone_eus = r?;
        forget_seconds(&mut self.edgeuse_to_edgeuses, &gone_eus, None)?;
        self.forget_faces(&gone)
    }
}

/// Deletes the offset faces `faces` of `b`, with their edges and vertices, returning the
/// edge uses that went with them.
fn delete_offset_faces(
    kernel: &mut Kernel,
    b: &mut Brep,
    faces: &[FaceId],
    notifications: &mut OffsetNotifications<'_>,
) -> Result<BTreeSet<EdgeuseId>> {
    let mut gone_eus: BTreeSet<EdgeuseId> = BTreeSet::new();
    let body = notifications.result_brep;
    for &face in faces {
        for e in face4::edges(b, face, None, false, None)? {
            gone_eus.extend(edge::edgeuses(b, e, None, None)?);
        }
        let mut stores = NotificationStores {
            records: &mut *notifications.records,
            attributes: &mut *notifications.attributes,
            key_of: notifications.result_key_of,
        };
        NotifyState::from_kernel_stores(&mut stores).with_kernel_env(kernel, |kernel, env| {
            delete_face_with_body_identity(
                kernel,
                b,
                env,
                face,
                DeleteFaceOptions {
                    delete_edges_vertices: true,
                    do_region_nesting: true,
                    region_to_keep: None,
                },
                body,
            )
        })?;
    }
    Ok(gone_eus)
}
