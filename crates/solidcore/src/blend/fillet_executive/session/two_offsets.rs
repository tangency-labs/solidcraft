//! Per-edge two-offset preparation using the session's existing solver lifecycle.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`,
//! `source/SMLib/src/SmFilletStandardSolver.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletSession::prepare_per_edge_two_offsets`] | `SetFilletParameters` setup with `SmConstantRadiusFS::pdOptSecondRadius` |
//! | [`FilletSession::prepare_chamfers`] | not upstream (#2486): chamfers by their sizes, kept in their definitions |
//!
//! Deviations: explicit generator descriptors replace cross-section type codes.
//! List sizes and live edges are checked before changing session state. No recursion.

use super::{FilletBody, FilletObject, FilletSession, FilletSurfaceGenerator};
use crate::blend::fillet_executive::p2::FilletParametersAccess;
use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use crate::topology::{edge, EdgeId};
use crate::{EntityRef, Kernel, Result, SolidError};

fn invalid() -> SolidError {
    SolidError::Upstream {
        operation: "blend::fillet_executive::session::prepare_per_edge_two_offsets",
        code: crate::error::SM_ERR_INVALID_INPUT,
        entity: None,
    }
}

fn value<T: Copy>(values: &[T], index: usize) -> Result<T> {
    values
        .get(if values.len() == 1 { 0 } else { index })
        .copied()
        .ok_or_else(invalid)
}

impl FilletSession<'_> {
    /// Prepare constant-radius solvers with independent offsets for their first
    /// and second support surfaces. Use [`FilletSurfaceGenerator::Linear`] for
    /// two-distance chamfers. Each list has either one entry for all edges or one
    /// entry per edge. The offsets follow [`Self::blend_edgeuse`]'s surface order.
    ///
    /// Resets the same execution switches and preparation bookkeeping as
    /// `SetFilletParameters` (including disabling global merge), and honors
    /// `fixes.fillet_corner_per_vertex_fan`. Solvers use four times the edge tolerance for
    /// approximation, 20 degrees for angle and 10 degrees for tangency. A shared
    /// generator is retained before loading; per-edge generators are retained
    /// after loading each solver. Initialization is set only when all loads
    /// succeed. Set execution switches after preparation, as for the existing
    /// per-edge preparation helpers.
    ///
    /// # Errors
    /// Native invalid input for no edges, mismatched lists, a dead edge or an edge
    /// without a blend edge use. Constructor and loader errors propagate, leaving
    /// prior successful loads in the session, as in `SetFilletParameters`.
    pub fn prepare_per_edge_two_offsets(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        first: &[f64],
        second: &[f64],
        generators: &[FilletSurfaceGenerator],
    ) -> Result<()> {
        if edges.is_empty()
            || [first.len(), second.len(), generators.len()]
                .into_iter()
                .any(|len| len != 1 && len != edges.len())
        {
            return Err(invalid());
        }
        let live_edges = crate::topology::brep::edges(self.target(), None)?;
        if edges.iter().any(|id| !live_edges.contains(id)) {
            return Err(invalid());
        }

        self.executive.initialized = false;
        self.executive.reset_parameter_state();
        if k.fixes.fillet_corner_per_vertex_fan {
            let objects: Vec<_> = edges
                .iter()
                .map(|&id| FilletObject::Topology(FilletBody::Target, EntityRef::Edge(id)))
                .collect();
            let points = self.objects.parameters_split_blended_vertex_fans(
                k,
                FilletObject::Body(FilletBody::Target),
                &objects,
            )?;
            self.executive.fan_split_points.extend(points);
        }

        let shared = if generators.len() == 1 {
            Some(self.add_surface_generator(value(generators, 0)?)?)
        } else {
            None
        };
        for (index, &id) in edges.iter().enumerate() {
            let edgeuse = self.blend_edgeuse(k, id)?.ok_or_else(invalid)?;
            let approx_tol = 4.0 * edge::tolerance(self.target(), id)?;
            let solver = self.new_constant_radius_solver_with_second(
                k,
                edgeuse,
                value(first, index)?,
                value(second, index)?,
                approx_tol,
                20.0 * (crate::math::PI / 180.0),
                10.0 * (crate::math::PI / 180.0),
            )?;
            self.load_solver(k, solver)?;
            let generator = match shared {
                Some(generator) => generator,
                None => self.add_surface_generator(value(generators, index)?)?,
            };
            self.set_solver_generator(solver, generator)?;
        }
        self.executive.initialized = true;
        Ok(())
    }

    /// Chamfers `edges` by their sizes (`sizes`: one for all edges or one per edge): two
    /// distances, a distance and an angle, or a chord. Each edge's sizes are converted to
    /// its two offsets at the wedge between its faces at the edge's middle (the angle
    /// between the directions into them there, [`ChamferDefinition::offsets`]), then
    /// prepared as [`Self::prepare_per_edge_two_offsets`] with the linear section, and the
    /// solver keeps the sizes: with `fixes.fillet_gn_section_generator` and
    /// `fixes.fillet_round_as_definition` the chamfer's surface is its definition, the
    /// chord of order 0, and keeps them too. A distance is along the edge's blend edge
    /// use's face (support 1) or its radial mate's (support 2); an angle is to support 1.
    /// Between planes the sizes hold all along the edge; on a curved face, where the wedge
    /// was measured. Not upstream (solidcore's, #2486).
    ///
    /// # Errors
    ///
    /// Those of [`Self::prepare_per_edge_two_offsets`]; native invalid input for an edge
    /// without a blend edge use or its mate, faces that do not make a wedge there (tangent
    /// or folded back), sizes that are not finite and positive, an angle past the wedge's
    /// room, or two distances that fold on it (one at most the other's `cos(wedge)`).
    pub fn prepare_chamfers(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        sizes: &[ChamferSize],
    ) -> Result<()> {
        use crate::topology::edgeuse::p1 as eu1;
        use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
        if edges.is_empty() || !(sizes.len() == 1 || sizes.len() == edges.len()) {
            return Err(invalid());
        }
        let mut chamfers = Vec::new();
        let mut first = Vec::new();
        let mut second = Vec::new();
        for (index, &id) in edges.iter().enumerate() {
            let u0 = self.blend_edgeuse(k, id)?.ok_or_else(invalid)?;
            let target: &mut crate::Brep = &mut *self.objects.target;
            let u1 = eu1::radial(target, u0)?.ok_or_else(invalid)?;
            let t = target
                .edge(id)
                .map_err(|_| invalid())?
                .interval
                .evaluate(0.5);
            let o = BinormalOptions::default();
            let b0 = evaluate_binormal(k, target, u0, t, false, &o)?
                .binormal
                .unitize()?;
            let b1 = evaluate_binormal(k, target, u1, t, false, &o)?
                .binormal
                .unitize()?;
            let wedge = b0.dot(b1).clamp(-1.0, 1.0).acos();
            let chamfer =
                ChamferDefinition::new(value(sizes, index)?, wedge).map_err(|_| invalid())?;
            let [r1, r2] = chamfer.offsets().map_err(|_| invalid())?;
            first.push(r1);
            second.push(r2);
            chamfers.push(chamfer);
        }
        let solvers_before = self.objects.solvers.len();
        self.prepare_per_edge_two_offsets(
            k,
            edges,
            &first,
            &second,
            &[FilletSurfaceGenerator::Linear],
        )?;
        // the solvers just made, one per edge in order: each keeps its edge's sizes
        let made: Vec<usize> = (solvers_before..self.objects.solvers.len())
            .filter(|&i| matches!(self.objects.solvers.get(i), Some(Some(_))))
            .collect();
        if made.len() != chamfers.len() {
            return Err(invalid());
        }
        for (i, chamfer) in made.into_iter().zip(chamfers) {
            if let Some(Some(solver)) = self.objects.solvers.get_mut(i) {
                solver.base_mut().chamfer = Some(chamfer);
            }
        }
        Ok(())
    }
}
