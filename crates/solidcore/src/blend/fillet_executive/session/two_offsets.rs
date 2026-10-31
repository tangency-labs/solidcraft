//! Per-edge two-offset preparation using the session's existing solver lifecycle.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`,
//! `source/SMLib/src/SmFilletStandardSolver.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletSession::prepare_per_edge_two_offsets`] | `SetFilletParameters` setup with `SmConstantRadiusFS::pdOptSecondRadius` |
//!
//! Deviations: explicit generator descriptors replace cross-section type codes.
//! List sizes and live edges are checked before changing session state. No recursion.

use super::{FilletBody, FilletObject, FilletSession, FilletSurfaceGenerator};
use crate::blend::fillet_executive::p2::FilletParametersAccess;
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
}
