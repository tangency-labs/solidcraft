//! The blend corner's Ball topology: three rounds of one radius at a vertex of three
//! planes, each round's rails crossing its neighbours' on their common faces, each round
//! ending on its own section, and a fourth face (the ball's sphere) closing the corner.
//!
//! Upstream: none (pinned SMLib has no Ball). The fork's
//! `source/SMLib/src/SmBlendCornerBall.cpp` (tangency-labs/smlib 79d4763, unchanged to
//! 5e8c9c1; smlib#79 step 4c).
//!
//! | Rust | Fork |
//! |------|------|
//! | `FilletCorner::make_ball_topology` (crate) | `SmBlendCorner::MakeBallTopology` |
//!
//! Deviations: the solvers of the three rounds are resolved by the caller
//! (`GetFilletSolverOfEdgeuse` of each ring edge use, else of its mate), and passed in
//! [`BallTopologyEnv`]. Opt-in: `Kernel::fixes.fillet_blend_corner` (WR-697). There is no
//! recursion.

use super::{new_corner_vertex, OP_NX2};
use crate::blend::blend_corner::ball::BallCorner;
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletBrep, FilletCornerId, FilletEdgeType, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::error::{EntityRef, Result, SolidError};
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
    /// The loaded solver of each of the corner's three ring edge uses (or its mate),
    /// in ring order.
    pub(crate) solvers: [&'a FilletSolver; 3],
}

impl FilletCorner {
    /// The fork's `SmBlendCorner::MakeBallTopology` (WR-697, 79d4763): vertex `i` where
    /// the rails of rounds `i - 1` and `i` cross on their common face `i`
    /// (`SM_FV_RAIL_X_RAIL`, the ball's contact with that face), and edge `i` from vertex
    /// `i` to vertex `i + 1`, round `i`'s end section (`SM_FE_CROSS_SECTION`). The corner
    /// is blending and not a bevel.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a corner that is not a Ball of three ring edge uses;
    /// [`SolidError::null_pointer`] where the fork dereferences missing topology (`NER`);
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
        let nul = || SolidError::null_pointer(OP_NX2);
        let all = self.all_eus.clone().ok_or_else(nul)?;
        if !self
            .blend
            .is_some_and(crate::blend::blend_corner::BlendCorner::is_ball)
            || all.len() != 3
        {
            return Err(SolidError::sm_err(OP_NX2));
        }
        self.ball = Some(BallCorner::default());
        // sm_BallRingFaces: face i between edge uses i - 1 and i
        let mut faces = Vec::with_capacity(3);
        let mut geoms = Vec::with_capacity(3);
        for ii in 0..3 {
            let prev = all.get((ii + 2) % 3).copied();
            let this = all.get(ii).copied();
            faces.push(face_by_two_edgeuses(target, prev, this, None)?);
        }
        for (fs, face) in solvers.iter().zip(&faces) {
            geoms.push(fs.first_fillet_geom()?);
            face.ok_or_else(nul)?;
        }

        // the contacts: rounds i - 1 and i's rails on face i
        for ii in 0..3 {
            let face = faces.get(ii).copied().flatten().ok_or_else(nul)?;
            let fv = new_corner_vertex(k, pseudo, corner)?;
            {
                let v = pseudo.fillet_vertex_mut(fv)?;
                v.set_point_class(PointClassType::Face, Some(EntityRef::Face(face)));
                v.set_fillet_vertex_type(FilletVertexType::RailXRail);
            }
            let prev = (ii + 2) % 3;
            for round in [prev, ii] {
                let (Some(&g), Some(&fs)) = (geoms.get(round), solvers.get(round)) else {
                    return Err(nul());
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
        for ii in 0..3 {
            let (Some(&from), Some(&to), Some(&g)) = (
                self.vertices.get(ii),
                self.vertices.get((ii + 1) % 3),
                geoms.get(ii),
            ) else {
                return Err(nul());
            };
            let e = pseudo.make_fillet_edge(Some(from), Some(to), Some(corner))?;
            pseudo
                .fillet_edge_mut(e)?
                .set_fillet_edge_type(FilletEdgeType::CrossSection);
            let primary = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(nul)?;
            pseudo.set_edgeuse_fillet_geom(primary, Some(g))?;
            self.edges.push(e);
        }
        Ok(())
    }
}
