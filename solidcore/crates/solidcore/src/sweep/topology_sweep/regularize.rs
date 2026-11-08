//! Opt-in regularization of whole-body translational sweep results.
//!
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SMLib/src/SmFace.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | `regularize_body` | composition of `IsDegenerate`, `SqueezeFace`, `DeleteEdge` |
//!
//! Deviations: not an upstream sweep step; enabled only by
//! `Kernel::fixes.translation_sweep_regularize`. No new recursion. Only faces
//! recorded as swept higher are squeezed, using one quarter of the body tolerance.
//! Isolated open-edge inner slit loops have no area and are deleted by Euler edits.

use std::collections::BTreeSet;

use super::access::SweepAccess;
use super::{SweepEntity, TopologySweep};
use crate::error::Result;
use crate::kernel::Kernel;
use crate::topology::brep::{p1, p4, p5};
use crate::topology::edge::NotifyEnv;
use crate::topology::{edge, edgeuse::p1 as eu, face, loopuse, Brep, BrepId, FaceId};

impl TopologySweep {
    pub(super) fn regularize_translation(
        &mut self,
        access: &mut impl SweepAccess,
        body: BrepId,
    ) -> Result<()> {
        let faces: BTreeSet<_> = self
            .to_higher
            .values()
            .filter_map(|entity| match entity {
                Some(SweepEntity::Face(id, face)) if *id == body => Some(*face),
                _ => None,
            })
            .collect();
        let result = access.with_brep_mut(body, |kernel, brep, _, env| {
            regularize_body(kernel, brep, &faces, env)
        });
        // Euler edits can leave partial output on error. Never expose a mapping
        // to a deleted result, including after a failed cleanup.
        let brep = access.brep(body)?;
        let live = |entity: &Option<SweepEntity>| match entity {
            Some(SweepEntity::Face(id, face)) if *id == body => brep.face(*face).is_ok(),
            Some(SweepEntity::Edge(id, edge)) if *id == body => brep.edge(*edge).is_ok(),
            Some(SweepEntity::Vertex(id, vertex)) if *id == body => brep.vertex(*vertex).is_ok(),
            Some(SweepEntity::Region(id, region)) if *id == body => brep.region(*region).is_ok(),
            _ => true,
        };
        for map in [&mut self.to_same, &mut self.to_higher] {
            for value in map.values_mut() {
                if !live(value) {
                    *value = None;
                }
            }
        }
        self.from_same.retain(|key, _| live(key));
        self.from_lower.retain(|key, _| live(key));
        result
    }
}

fn regularize_body(
    kernel: &mut Kernel,
    brep: &mut Brep,
    swept_faces: &BTreeSet<FaceId>,
    env: &mut NotifyEnv<'_>,
) -> Result<()> {
    let was_editing = brep.editing_enabled();
    brep.set_editing_enabled(true);
    let result = (|| {
        let tolerance = brep.tolerance();
        for &face in swept_faces {
            if brep.face(face).is_err() {
                continue;
            }
            if face::p3::is_degenerate(kernel, brep, face, true, Some(tolerance / 4.0))? {
                p5::squeeze_face_in(
                    kernel,
                    brep,
                    true,
                    face,
                    Some(tolerance),
                    &mut p5::SqueezedFace::default(),
                    env,
                )?;
            }
        }
        let mut slit_edges = BTreeSet::new();
        for face in p1::faces(brep, None)? {
            for loop_id in face::p2::loops(brep, face, false, None)?
                .into_iter()
                .skip(1)
            {
                let uses = loopuse::loop_edgeuses(brep, loop_id)?;
                let [first, second] = uses.as_slice() else {
                    continue;
                };
                let Some(edge) = eu::edge(brep, *first)? else {
                    continue;
                };
                if eu::edge(brep, *second)? == Some(edge)
                    && eu::orientation(brep, *first)? != eu::orientation(brep, *second)?
                    && !edge::is_closed(brep, edge)?
                    && brep.edge(edge)?.edgeuses.len() == 4
                    && edge::faces(brep, edge, None)?.iter().all(|id| *id == face)
                {
                    slit_edges.insert(edge);
                }
            }
        }
        for edge in slit_edges {
            p4::delete_edge(
                kernel,
                brep,
                &mut p4::DeleteEnv::from_notify(env),
                edge,
                None,
            )?;
        }
        Ok(())
    })();
    brep.set_editing_enabled(was_editing);
    brep.refresh_cached_topology();
    result
}
