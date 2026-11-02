//! The blend corner's Swept turn topology: two rounds of one radius that turn at a sharp
//! straight side edge end on their own sections, and a third face, the end section
//! revolved about the side edge, closes the corner.
//!
//! Upstream: none (pinned SMLib has no Swept turn). The fork's
//! `source/SMLib/src/SmBlendCorner.cpp` (tangency-labs/smlib e647dac, unchanged to
//! 5e8c9c1; smlib#79 step 4b).
//!
//! | Rust | Fork |
//! |------|------|
//! | `FilletCorner::make_swept_topology` (crate) | `SmBlendCorner::MakeSweptTopology` |
//!
//! Deviations: the side edge use (`m_pSideEU`) is kept in
//! [`FilletCorner::swept`](crate::blend::fillet_corner::p3::FilletCorner::swept). The
//! solvers of the two rounds are resolved by the caller (`GetFilletSolverOfEdgeuse`).
//! Opt-in: `Kernel::fixes.fillet_blend_corner` (WR-697) with the session's swept turns
//! (`FilletSession::set_swept_turns`). There is no recursion.

use super::{find_side_edgeuse, new_corner_vertex, radial, Nx2TopologyEnv, OP_NX2};
use crate::blend::blend_corner::swept::SweptTurn;
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletCornerId, FilletEdgeType, FilletVertexType,
};
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as eu1;

impl FilletCorner {
    /// The fork's `SmBlendCorner::MakeSweptTopology` (WR-697): the Swept turn's side
    /// vertex S (both rounds' rails on their other faces meet the side edge there), the
    /// plate vertices A and B (S's mates on each round's rail on the common face) and the
    /// corner edges S-A and B-S (the rounds' end sections, `SM_FE_CROSS_SECTION`) and A-B
    /// (where the corner's face meets the common face, `SM_FE_PRECOMPUTED` on it). The
    /// vertices are S, A, B in that order.
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a corner that is not a Swept turn of two solver edge uses among
    /// three, or a round whose side rail is its common-face rail;
    /// [`SolidError::null_pointer`] where the fork dereferences missing topology (`NER`);
    /// those of the solver and pseudo-topology operations.
    pub(crate) fn make_swept_topology(&mut self, env: Nx2TopologyEnv<'_>) -> Result<()> {
        fn lookup(corner: FilletCornerId, this: &FilletCorner) -> CornerLookup<'_> {
            CornerLookup {
                this: Some((Some(corner), this)),
                ..Default::default()
            }
        }
        let Nx2TopologyEnv {
            k,
            target,
            pseudo,
            corner,
            solvers,
        } = env;
        let nul = || SolidError::null_pointer(OP_NX2);
        let all = self.all_eus.clone().ok_or_else(nul)?;
        let (eu_a, eu_b) = match self.solver_eus.as_deref() {
            Some(&[a, b])
                if all.len() == 3
                    && self
                        .blend
                        .is_some_and(crate::blend::blend_corner::BlendCorner::is_swept) =>
            {
                (a, b)
            }
            _ => return Err(SolidError::sm_err(OP_NX2)),
        };
        let mut ring = None;
        for &eu in &all {
            if eu != eu_a && eu != eu_b {
                ring = Some(eu);
            }
        }
        let ring = ring.ok_or_else(nul)?;
        let side_eu = find_side_edgeuse(target, eu_a, ring)?.ok_or_else(nul)?;
        self.swept = Some(SweptTurn::new(side_eu));
        let common =
            face_by_two_edgeuses(target, Some(eu_a), Some(eu_b), Some(side_eu))?.ok_or_else(nul)?;

        let side_eus = [side_eu, radial(target, side_eu)?];
        let mut geoms = Vec::with_capacity(2);
        let mut side_rails = Vec::with_capacity(2);
        let mut face_rails = Vec::with_capacity(2);
        for (fs, side) in solvers.iter().zip(side_eus) {
            geoms.push(fs.first_fillet_geom()?);
            let s = fs.find_index_of_rail_x_side_edgeuse(target, side)?;
            let f = fs.find_index_of_rail_on_face(target, Some(common))?;
            if s == f {
                return Err(SolidError::sm_err(OP_NX2));
            }
            side_rails.push(s);
            face_rails.push(f);
        }

        // S, on the side edge, at both rounds' rails on their other faces
        let s = new_corner_vertex(k, pseudo, corner)?;
        {
            let fv = pseudo.fillet_vertex_mut(s)?;
            fv.set_point_class(PointClassType::Edgeuse, Some(EntityRef::Edgeuse(side_eu)));
            fv.set_fillet_vertex_type(FilletVertexType::RailXEdgeuse);
        }
        self.vertices.push(s);
        for ((&g, &rail), &fs) in geoms.iter().zip(&side_rails).zip(&solvers) {
            pseudo.make_rail_edgeuse_on(
                g,
                rail,
                Some(s),
                OrientType::Unknown,
                None,
                lookup(corner, self),
                fs,
                target,
            )?;
        }

        // A and B, on each round's rail on the common face, straight across its end
        // section from S: S's mates, which SmFilletVertex::CalcRailIntEdgeuse places with
        // S, each on its own round
        let mut plates = Vec::with_capacity(2);
        for (ii, ((&g, &rail), &fs)) in geoms.iter().zip(&face_rails).zip(&solvers).enumerate() {
            let plate = new_corner_vertex(k, pseudo, corner)?;
            {
                let fv = pseudo.fillet_vertex_mut(plate)?;
                fv.set_point_class(PointClassType::Face, Some(EntityRef::Face(common)));
                fv.set_fillet_vertex_type(FilletVertexType::Mate);
                fv.set_mate(0, Some(s));
            }
            pseudo.fillet_vertex_mut(s)?.set_mate(ii, Some(plate));
            self.vertices.push(plate);
            plates.push(plate);
            pseudo.make_rail_edgeuse_on(
                g,
                rail,
                Some(plate),
                OrientType::Unknown,
                None,
                lookup(corner, self),
                fs,
                target,
            )?;
        }

        // each round ends on its own section; the corner's face is a patch of its own
        self.blending = true;
        self.bevel = false;

        // the corner's edges in its face's loop order: S-A, A-B, B-S
        let (&[a, b], &[geom_a, geom_b]) = (plates.as_slice(), geoms.as_slice()) else {
            return Err(nul());
        };
        for (from, to, own) in [(s, a, Some(geom_a)), (a, b, None), (b, s, Some(geom_b))] {
            let e = pseudo.make_fillet_edge(Some(from), Some(to), Some(corner))?;
            let primary = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(nul)?;
            pseudo.set_edgeuse_fillet_geom(primary, own)?;
            let edge = pseudo.fillet_edge_mut(e)?;
            if own.is_some() {
                edge.set_fillet_edge_type(FilletEdgeType::CrossSection);
            } else {
                edge.set_fillet_edge_type(FilletEdgeType::Precomputed);
                edge.set_original_face(Some(common));
            }
            self.edges.push(e);
        }
        Ok(())
    }
}
