//! The blend corner's Ball topology: `n >= 3` rounds of one radius at a vertex of `n`
//! faces, each round's rails crossing its neighbours' on their common faces, each round
//! ending on its own section, and the ball's sphere closing the corner.
//!
//! Upstream: none (pinned SMLib has no Ball). The fork's
//! `source/SMLib/src/SmBlendCornerBall.cpp` (tangency-labs/smlib 79d4763, smlib#79 step 4c;
//! `n` rounds 278bf77, smlib#125).
//!
//! | Rust | Fork |
//! |------|------|
//! | `FilletCorner::make_ball_topology` (crate) | `SmBlendCorner::MakeBallTopology` |
//!
//! Deviations: the solvers of the rounds are resolved by the caller
//! (`GetFilletSolverOfEdgeuse` of each ring edge use, else of its mate), and passed in
//! [`BallTopologyEnv`] (WR-697). There is no recursion.

use super::{defect, missing_defect, new_corner_vertex, NO_PRIMARY_EU, OP_NX2};
use crate::blend::blend_corner::ball::BallCorner;
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletEdgeType, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::error::{EntityRef, Result};
use crate::intersect::point_class::PointClassType;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::Brep;

/// What the Ball's topology operates on besides the corner itself.
pub(crate) struct BallTopologyEnv<'a> {
    /// The executive's kernel.
    pub(crate) k: &'a Kernel,
    /// The target whose edge uses the corner stores.
    pub(crate) target: &'a mut Brep,
    /// The executive's actual pseudo-B-rep.
    pub(crate) pseudo: &'a mut FilletBrep,
    /// This corner's identity in the executive's corner store.
    pub(crate) corner: FilletCornerId,
    /// The loaded solver of each of the corner's `n` ring edge uses (or its mate),
    /// in ring order.
    pub(crate) solvers: Vec<&'a FilletSolver>,
}

/// Why a missing face of the ball corner's ring is the kernel's fault.
const NO_RING_FACE: &str = "two edge uses next to each other in the ball corner's ring share \
     no face, though the ball's classification makes a ball only of a ring with a face between \
     each two";

impl FilletCorner {
    /// The fork's `SmBlendCorner::MakeBallTopology` (WR-697, 79d4763): vertex `i` where
    /// the rails of rounds `i - 1` and `i` cross on their common face `i`
    /// (`SM_FV_RAIL_X_RAIL`, the ball's contact with that face), and edge `i` from vertex
    /// `i` to vertex `i + 1`, round `i`'s end section (`SM_FE_CROSS_SECTION`). The corner
    /// is blending and not a bevel.
    ///
    /// # Errors
    ///
    /// Reviewed `Defect`s: `SM_ERR` for a corner that is not a Ball of three or more ring
    /// edge uses (one solver each); `SM_ERR_NULL_POINTER` where the fork dereferences
    /// missing topology (`NER`);
    /// those of the solver and pseudo-topology operations.
    pub(crate) fn make_ball_topology(&mut self, env: BallTopologyEnv<'_>) -> Result<()> {
        fn lookup(corner: FilletCornerId, this: &FilletCorner) -> CornerLookup<'_> {
            CornerLookup {
                this: Some((Some(corner), this)),
                ..Default::default()
            }
        }
        let BallTopologyEnv {
            k,
            target,
            pseudo,
            corner,
            solvers,
        } = env;
        let all = self.all_eus.clone().ok_or_else(|| {
            missing_defect(
                OP_NX2,
                "the ball corner has no ring of edge uses, though the corner's create walks \
                 the ring before the corner is classified a ball",
            )
        })?;
        let n = all.len();
        if !self
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_ball)
            || n < 3
            || solvers.len() != n
        {
            return Err(defect(
                OP_NX2,
                "the corner is not a ball corner of a round on each of its three or more edges, \
                 though its classification makes a ball only of three or more rounded edges \
                 and the caller passes one solver per edge",
            ));
        }
        self.ball = Some(BallCorner::default());
        // sm_BallRingFaces: face i between edge uses i - 1 and i
        let mut faces = Vec::with_capacity(n);
        let mut geoms = Vec::with_capacity(n);
        for ii in 0..n {
            let prev = all.get((ii + n - 1) % n).copied();
            let this = all.get(ii).copied();
            faces.push(face_by_two_edgeuses(target, prev, this, None)?);
        }
        for (fs, face) in solvers.iter().zip(&faces) {
            geoms.push(fs.first_fillet_geom()?);
            face.ok_or_else(|| missing_defect(OP_NX2, NO_RING_FACE))?;
        }

        // the contacts: rounds i - 1 and i's rails on face i
        for ii in 0..n {
            let face = faces
                .get(ii)
                .copied()
                .flatten()
                .ok_or_else(|| missing_defect(OP_NX2, NO_RING_FACE))?;
            let fv = new_corner_vertex(k, pseudo, corner)?;
            {
                let v = pseudo.fillet_vertex_mut(fv)?;
                v.set_point_class(PointClassType::Face, Some(EntityRef::Face(face)));
                v.set_fillet_vertex_type(FilletVertexType::RailXRail);
            }
            let prev = (ii + n - 1) % n;
            for round in [prev, ii] {
                let (Some(&g), Some(&fs)) = (geoms.get(round), solvers.get(round)) else {
                    return Err(missing_defect(
                        OP_NX2,
                        "a round of the ball corner has no geometry or solver, though the loop \
                         above takes one geometry from each of the corner's solvers, one per edge",
                    ));
                };
                let rail = fs.find_index_of_rail_on_face(target, Some(face))?;
                pseudo.make_rail_edgeuse_on(
                    g,
                    rail,
                    Some(fv),
                    OrientType::Unknown,
                    None,
                    lookup(corner, self),
                    fs,
                    target,
                )?;
            }
            self.vertices.push(fv);
        }

        // each round ends on its section through the ball's centre; the corner's face is a
        // patch of its own
        self.blending = true;
        self.bevel = false;
        for ii in 0..n {
            let (Some(&from), Some(&to), Some(&g)) = (
                self.vertices.get(ii),
                self.vertices.get((ii + 1) % n),
                geoms.get(ii),
            ) else {
                return Err(missing_defect(
                    OP_NX2,
                    "the ball corner has no contact vertex or round geometry at an index, though \
                     the loops above make one contact and take one geometry per edge",
                ));
            };
            let e = pseudo.make_fillet_edge(Some(from), Some(to), Some(corner))?;
            pseudo
                .fillet_edge_mut(e)?
                .set_fillet_edge_type(FilletEdgeType::CrossSection);
            let primary = eu1::primary_edgeuse(pseudo.brep(), e)?
                .ok_or_else(|| missing_defect(OP_NX2, NO_PRIMARY_EU))?;
            pseudo.set_edgeuse_fillet_geom(primary, Some(g))?;
            self.edges.push(e);
        }
        Ok(())
    }
}
