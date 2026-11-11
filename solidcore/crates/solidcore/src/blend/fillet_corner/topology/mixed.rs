//! The blend corner's Mixed topology: a convex and a concave blend continuing each other
//! G1 across a tangent seam, meeting at their rails' crossing X on their common face, the
//! corner two faces either side of the inflection X-V.
//!
//! Upstream: none (pinned SMLib builds this corner with `SmFillet3x2MixedConvexityCorner`).
//! The fork's `source/SMLib/src/SmBlendCornerMixed.cpp` (tangency-labs/smlib d443f10;
//! smlib#82).
//!
//! | Rust | Fork |
//! |------|------|
//! | `FilletCorner::make_mixed_topology` (crate) | `SmBlendCorner::MakeMixedTopology` |
//!
//! Deviations: the ring (`sm_MixedRing`) read here is kept on the corner
//! ([`MixedCorner`]) for the later steps, with the blends' solvers' identities (their
//! first geometries' `GetFilletSolver()`; WR-697). There is no recursion.

use super::{new_corner_vertex, Nx2TopologyEnv, OP_NX2};
use crate::blend::blend_corner::mixed::{mixed_ring, MixedCorner};
use crate::blend::fillet_corner::p3::FilletCorner;
use crate::blend::fillet_geom::p1::{
    CornerLookup, FilletCornerId, FilletEdgeType, FilletVertexType,
};
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::error::{EntityRef, Result, SolidError};
use crate::intersect::point_class::PointClassType;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::EdgeuseId;

impl FilletCorner {
    /// The fork's `SmBlendCorner::MakeMixedTopology` (WR-697, 6fd0411): vertices X (the
    /// rails' crossing on T), A (B1's rail end on F1), P (on the seam), V, Q (on F2), B
    /// (B2's rail end on F2); edges X-A (B1's trimline), A-P (on F1), P-V (the seam), V-X
    /// (the inflection, in T), V-Q, Q-B (on F2), X-B (B2's trimline), every one
    /// precomputed. The blends run on past the vertex: the corner is neither blending nor
    /// a bevel. Q is on the edge beyond the seam, where the ring has one (#2953).
    ///
    /// # Errors
    ///
    /// `SM_ERR` for a corner that is not a Mixed one, or whose blends' rails on T and on
    /// their walls are one; [`SolidError::null_pointer`] where the fork dereferences
    /// missing topology (`NER`); those of the solver and pseudo-topology operations.
    pub(crate) fn make_mixed_topology(&mut self, env: Nx2TopologyEnv<'_>) -> Result<()> {
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
        let err = || SolidError::sm_err(OP_NX2);
        let solver_eus = self.solver_eus.clone().ok_or_else(nul)?;
        let all = self.all_eus.clone().ok_or_else(nul)?;
        // GetFilletSolverOfEdgeuse of the ring's blended edge uses: the solvers of the
        // corner's first and second filleted edge uses
        let of = |eu: EdgeuseId| -> Option<&FilletSolver> {
            solver_eus
                .iter()
                .position(|&s| s == eu)
                .and_then(|i| solvers.get(i).copied())
        };
        let (ring, fs) = mixed_ring(k, target, &solver_eus, &all, &of)?.ok_or_else(err)?;
        let geoms = [fs[0].first_fillet_geom()?, fs[1].first_fillet_geom()?];
        let ids = [
            pseudo.geom(geoms[0])?.fillet_solver().ok_or_else(nul)?,
            pseudo.geom(geoms[1])?.fillet_solver().ok_or_else(nul)?,
        ];
        let on_t = [
            fs[0].find_index_of_rail_on_face(target, Some(ring.t))?,
            fs[1].find_index_of_rail_on_face(target, Some(ring.t))?,
        ];
        let on_wall = [
            fs[0].find_index_of_rail_on_face(target, Some(ring.f1))?,
            fs[1].find_index_of_rail_on_face(target, Some(ring.f2))?,
        ];
        if on_t[0] == on_wall[0] || on_t[1] == on_wall[1] {
            return Err(err());
        }
        self.mixed = Some(MixedCorner::new(ring, ids));

        // the blends run on past the vertex (no blending corner): their rails on the wall
        // then cross the seam's line, at P and Q
        self.blending = false;
        self.bevel = false;

        // vertices: X, A, P, V, Q, B
        let mut vs = Vec::with_capacity(6);
        for _ in 0..6 {
            let fv = new_corner_vertex(k, pseudo, corner)?;
            self.vertices.push(fv);
            vs.push(fv);
        }
        let &[x, a, p, v, q, b] = vs.as_slice() else {
            return Err(err());
        };
        let vertex = self.vertex.ok_or_else(nul)?;
        {
            let fv = pseudo.fillet_vertex_mut(x)?;
            fv.set_point_class(PointClassType::Face, Some(EntityRef::Face(ring.t)));
            fv.set_fillet_vertex_type(FilletVertexType::RailXRail);
        }
        pseudo
            .fillet_vertex_mut(a)?
            .set_point_class(PointClassType::Face, Some(EntityRef::Face(ring.f1)));
        pseudo
            .fillet_vertex_mut(p)?
            .set_point_class(PointClassType::Edgeuse, Some(EntityRef::Edgeuse(ring.seam)));
        {
            let fv = pseudo.fillet_vertex_mut(v)?;
            fv.set_point_class(PointClassType::Vertex, Some(EntityRef::Vertex(vertex)));
            fv.set_fillet_vertex_type(FilletVertexType::OnVertex);
        }
        let (q_class, q_on) = match ring.beyond {
            Some(eu) => (PointClassType::Edgeuse, EntityRef::Edgeuse(eu)),
            None => (PointClassType::Face, EntityRef::Face(ring.f2)),
        };
        pseudo
            .fillet_vertex_mut(q)?
            .set_point_class(q_class, Some(q_on));
        pseudo
            .fillet_vertex_mut(b)?
            .set_point_class(PointClassType::Face, Some(EntityRef::Face(ring.f2)));
        let [g0, g1] = geoms;
        let [f0, f1] = fs;
        for (geom, solver, rail, fv) in [
            (g0, f0, on_t[0], x),
            (g1, f1, on_t[1], x),
            (g0, f0, on_wall[0], a),
            (g1, f1, on_wall[1], b),
        ] {
            pseudo.make_rail_edgeuse_on(
                geom,
                rail,
                Some(fv),
                OrientType::Unknown,
                None,
                lookup(corner, self),
                solver,
                target,
            )?;
        }

        // edges: X-A, A-P, P-V, V-X, V-Q, Q-B, X-B (each trimline from X, as its face's iso
        // runs)
        let ends = [(x, a), (a, p), (p, v), (v, x), (v, q), (q, b), (x, b)];
        let on = [
            None,
            Some(ring.f1),
            Some(ring.f1),
            None,
            Some(ring.f2),
            Some(ring.f2),
            None,
        ];
        for (ii, ((from, to), face)) in ends.into_iter().zip(on).enumerate() {
            let e = pseudo.make_fillet_edge(Some(from), Some(to), Some(corner))?;
            {
                let edge = pseudo.fillet_edge_mut(e)?;
                edge.set_fillet_edge_type(FilletEdgeType::Precomputed);
                if face.is_some() {
                    edge.set_original_face(face);
                }
            }
            if ii == 0 || ii == 6 {
                let primary = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(nul)?;
                pseudo.set_edgeuse_fillet_geom(primary, Some(if ii == 6 { g1 } else { g0 }))?;
            }
            self.edges.push(e);
        }
        Ok(())
    }
}
