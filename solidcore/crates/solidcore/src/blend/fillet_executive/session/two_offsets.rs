//! Per-edge two-offset preparation using the session's existing solver lifecycle.
//!
//! Upstream: `source/SMLib/src/SmFilletExecutive.cpp`,
//! `source/SMLib/src/SmFilletStandardSolver.cpp`.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`FilletSession::prepare_per_edge_two_offsets`] | `SetFilletParameters` setup with `SmConstantRadiusFS::pdOptSecondRadius` |
//! | [`FilletSession::prepare_chamfers`] | not upstream (#2486): chamfers by their sizes, kept in their definitions |
//! | [`FilletSession::prepare_chamfers_along`] | not upstream: the same, each first size along a face the caller names |
//!
//! Deviations: explicit generator descriptors replace cross-section type codes.
//! List sizes and live edges are checked before changing session state. A knife edge's
//! chamfer of equal distances has no wedge for its definition: it is the linear section on the
//! ball of radius the distance (#2793's rig cases). No recursion.

use super::{FilletBody, FilletObject, FilletSession, FilletSurfaceGenerator};
use crate::blend::blend_refusal::{chains_among, BlendRefusal};
use crate::blend::chamfer_side::widest_wedge_bound;
use crate::blend::fillet_executive::p2::FilletParametersAccess;
use crate::blend::fillet_solver::p5::is_knife_edge;
use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use crate::surface::surface::SurfaceKind;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, EdgeId, EdgeuseId};
use crate::{EntityRef, Kernel, Result, SolidError};

fn invalid() -> SolidError {
    SolidError::Upstream {
        operation: "blend::fillet_executive::session::prepare_per_edge_two_offsets",
        code: crate::error::SM_ERR_INVALID_INPUT,
        entity: None,
    }
}

/// The tangency of a solver these prepare, radians: 10 degrees.
const TANGENCY: f64 = 10.0 * (crate::math::PI / 180.0);

/// How many of a chain's edge tolerances the smallest setback keeps at its widest station: a
/// bound that is open, as a refusal's fit size is (`docs/generic-blend.md`, §3.5).
const FOLD_MARGIN: f64 = 1000.0;

fn value<T: Copy>(values: &[T], index: usize) -> Result<T> {
    values
        .get(if values.len() == 1 { 0 } else { index })
        .copied()
        .ok_or_else(invalid)
}

/// The refusal of `size` on edge `id` at `wedge`, which [`ChamferDefinition::new`] refused:
/// [`BlendRefusal::ChamferAngleTooLarge`] for a good angle past a good wedge's room, else
/// native invalid input.
fn refused_size(id: EdgeId, size: ChamferSize, wedge: f64) -> SolidError {
    match size {
        ChamferSize::DistanceAngle { angle, .. }
            if wedge > 0.0
                && wedge < crate::math::PI
                && angle > 0.0
                && angle.is_finite()
                && angle + wedge >= crate::math::PI =>
        {
            BlendRefusal::ChamferAngleTooLarge {
                edge: id,
                angle,
                largest: crate::math::PI - wedge,
            }
            .error(
                "blend::fillet_executive::session::prepare_chamfers_along",
                crate::error::SM_ERR_INVALID_INPUT,
            )
        }
        _ => invalid(),
    }
}

/// Whether edge use `u` bounds a face on a plane.
fn on_plane(b: &crate::Brep, u: EdgeuseId) -> Result<bool> {
    let Some(face) = eu1::face(b, u)? else {
        return Ok(false);
    };
    Ok(crate::topology::face::p2::surface(b, face)?
        .is_some_and(|s| matches!(s.kind(), SurfaceKind::Plane(_))))
}

/// The wedge edge `index`'s chamfer is measured at. `wedges` are the wedges at the edges'
/// middles, `between_planes` which edges lie between two planes, `alike` which edges are one
/// chamfer with it (its tangent chain's, of its size).
///
/// An edge between planes keeps its own wedge: it holds there all along, so the sizes do. On
/// any other edge the wedge is the chain's edges' between planes, so its offsets are theirs;
/// where those differ no one pair of offsets holds the sizes on them all, and the edge keeps
/// its own. Without one it is the narrowest of the chain, whose offsets are the smallest, so
/// the setbacks are nowhere much past the sizes asked; unequal sizes can fold at it
/// ([`unfolded_wedge`]).
fn chain_wedge(
    wedges: &[f64],
    between_planes: &[bool],
    index: usize,
    alike: impl Fn(usize) -> Result<bool>,
) -> Result<f64> {
    let own = wedges.get(index).copied().ok_or_else(invalid)?;
    if between_planes.get(index).copied().unwrap_or(false) {
        return Ok(own);
    }
    let mut anchor: Option<f64> = None;
    let mut narrowest = own;
    for (other, (&w, &planes)) in wedges.iter().zip(between_planes).enumerate() {
        if !alike(other)? {
            continue;
        }
        if w < narrowest {
            narrowest = w;
        }
        if planes {
            // the rounding of an angle measured from two unit vectors
            let room = 64.0 * f64::EPSILON / w.sin();
            match anchor {
                Some(a) if (a - w).abs() > room => return Ok(own),
                Some(_) => {}
                None => anchor = Some(w),
            }
        }
    }
    Ok(anchor.unwrap_or(narrowest))
}

/// The wedge a chamfer of setbacks `setbacks` is measured at, on a chain measured at `narrowest`
/// whose faces open to `widest`: `narrowest`, or the least wedge past it at which no station
/// folds. Measured at `a`, the setback along a face at wedge `w` is `(d_1 - d_2 cos a + (d_2 -
/// d_1 cos a) cos w) / (sin a sin w)` ([`ChamferDefinition::of_offsets`]), which falls as `w`
/// grows; `sin a <= 1`, so it is at least `margin` where the numerator is at least `margin sin
/// w`, a bound on `cos a` for each of the two faces. The numerator's form never folds equal
/// setbacks. The bound stops at `widest`, where the setbacks are as asked.
fn unfolded_wedge(narrowest: f64, setbacks: [f64; 2], widest: f64, margin: f64) -> f64 {
    let [d1, d2] = setbacks;
    let (sin_w, cos_w) = widest.sin_cos();
    let mut most = 1.0_f64;
    for (a, b) in [(d1, d2), (d2, d1)] {
        let across = b + a * cos_w;
        if across > 0.0 {
            most = most.min((a + b * cos_w - margin * sin_w) / across);
        }
    }
    // clamp: constant bounds, the lower below the upper (cannot panic)
    let least = most.clamp(-1.0, 1.0).acos();
    narrowest.max(least.min(widest))
}

/// The wedge between the faces of edge uses `u0`, `u1` at parameter `t` of their edge.
fn wedge_at(k: &Kernel, b: &mut crate::Brep, u0: EdgeuseId, u1: EdgeuseId, t: f64) -> Result<f64> {
    let o = BinormalOptions::default();
    let b0 = evaluate_binormal(k, b, u0, t, false, &o)?
        .binormal
        .unitize()?;
    let b1 = evaluate_binormal(k, b, u1, t, false, &o)?
        .binormal
        .unitize()?;
    // clamp: constant bounds, the lower below the upper (cannot panic)
    Ok(b0.dot(b1).clamp(-1.0, 1.0).acos())
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
            let edgeuse = self.blendable_edgeuse(k, id)?;
            let approx_tol = 4.0 * edge::tolerance(self.target(), id)?;
            let solver = self.new_constant_radius_solver_with_second(
                k,
                edgeuse,
                value(first, index)?,
                value(second, index)?,
                approx_tol,
                20.0 * (crate::math::PI / 180.0),
                TANGENCY,
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
    /// between the directions into them there, [`ChamferDefinition::offsets`]); the edges
    /// of one size on a tangent chain ([`chains_among`]) are one chamfer, measured at the
    /// wedge of its edges between planes, else at the narrowest of its wedges at which no
    /// station folds ([`unfolded_wedge`]), so their rails meet where they run on into each
    /// other. Then prepared as [`Self::prepare_per_edge_two_offsets`] with the linear
    /// section, and the solver keeps the sizes: with `fixes.fillet_gn_section_generator` and
    /// `fixes.fillet_round_as_definition` the chamfer's surface is its definition, the
    /// chord of order 0, and keeps them too. A distance is along the edge's blend edge
    /// use's face (support 1) or its radial mate's (support 2); an angle is to support 1.
    /// Between planes the sizes hold all along the edge; on a curved face, where the wedge
    /// was measured, and elsewhere along the edge they drift (the module's page of
    /// [`crate::surface::procedural_blend::chamfer`]). Not upstream (solidcore's, #2486).
    ///
    /// # Errors
    ///
    /// Those of [`Self::prepare_per_edge_two_offsets`]; native invalid input for an edge
    /// without a blend edge use or its mate, faces that do not make a wedge there (tangent,
    /// or folded back other than a knife edge chamfered at equal distances, which is the chord
    /// between the contacts of the ball of radius the distance), sizes that are not finite and
    /// positive, or two distances that fold on it (one at most the other's `cos(wedge)`);
    /// [`BlendRefusal::ChamferAngleTooLarge`] for an angle past the wedge's room.
    pub fn prepare_chamfers(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        sizes: &[ChamferSize],
    ) -> Result<()> {
        self.prepare_chamfers_along(k, edges, sizes, &[])
            .map(|_| ())
    }

    /// [`Self::prepare_chamfers`], with each edge's first size along the face `along` names
    /// (empty: support 1 for every edge; else one face per edge, either face of its edge),
    /// and the offsets each edge's solver was given, `[support 1, support 2]` per edge. A
    /// size along support 2 is kept as it was asked when it is two distances (swapped onto
    /// the supports); a distance and an angle to support 2 is kept as its two setbacks at the
    /// wedge, as no size names an angle to support 2. Not upstream (solidcore's).
    ///
    /// # Errors
    ///
    /// Those of [`Self::prepare_chamfers`]; native invalid input for `along` neither empty
    /// nor one face per edge, or a face that is neither of its edge's.
    pub fn prepare_chamfers_along(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        sizes: &[ChamferSize],
        along: &[crate::topology::FaceId],
    ) -> Result<Vec<[f64; 2]>> {
        if edges.is_empty()
            || !(sizes.len() == 1 || sizes.len() == edges.len())
            || !(along.is_empty() || along.len() == edges.len())
        {
            return Err(invalid());
        }
        let mut wedges = Vec::new();
        let mut between_planes = Vec::new();
        let mut knife = Vec::new();
        let knife_tol = k.tolerances.ang_tol_deg * (crate::math::PI / 180.0);
        for &id in edges {
            let u0 = self.blendable_edgeuse(k, id)?;
            let target: &mut crate::Brep = &mut *self.objects.target;
            let u1 = eu1::radial(target, u0)?.ok_or_else(invalid)?;
            // a knife edge has no wedge: none to measure, and none its chain could take
            let is_knife = is_knife_edge(k, target, id, knife_tol)?;
            knife.push(is_knife);
            if is_knife {
                wedges.push(0.0);
                between_planes.push(false);
                continue;
            }
            let t = target
                .edge(id)
                .map_err(|_| invalid())?
                .interval
                .evaluate(0.5);
            wedges.push(wedge_at(k, target, u0, u1, t)?);
            between_planes.push(on_plane(target, u0)? && on_plane(target, u1)?);
        }
        let target: &mut crate::Brep = &mut *self.objects.target;
        let chains = chains_among(k, target, edges, TANGENCY)?;
        // each edge's wedge: its chain's, past where unequal sizes fold (`unfolded_wedge`)
        let mut measured = Vec::new();
        let mut widest: Vec<Option<f64>> = vec![None; edges.len()];
        for index in 0..edges.len() {
            if knife.get(index).copied().unwrap_or(false) {
                measured.push(0.0);
                continue;
            }
            let size = value(sizes, index)?;
            let alike = |other: usize| -> Result<bool> {
                Ok(chains.get(index) == chains.get(other)
                    && !knife.get(other).copied().unwrap_or(false)
                    && value(sizes, other)? == size)
            };
            let mut wedge = chain_wedge(&wedges, &between_planes, index, alike)?;
            let mut on_chain = Vec::new();
            for other in 0..edges.len() {
                if alike(other)? {
                    on_chain.push(other);
                }
            }
            let planes = |i: &usize| between_planes.get(*i).copied().unwrap_or(false);
            let setbacks = ChamferDefinition::new(size, wedge).map(|c| c.setbacks());
            if let (false, Ok([d1, d2])) = (on_chain.iter().any(planes), setbacks) {
                if d1 != d2 {
                    let mut tolerance = 0.0_f64;
                    for &other in &on_chain {
                        let id = edges.get(other).copied().ok_or_else(invalid)?;
                        tolerance = tolerance.max(edge::tolerance(target, id)?);
                    }
                    // the bound's gap moves the fold test's numerator by at most `|d_2 - d_1
                    // cos a| <= d_1 + d_2` times it: by one tolerance at most, a thousandth of
                    // the margin
                    let gap = tolerance / (d1 + d2);
                    let mut opens = wedge;
                    for &other in &on_chain {
                        let id = edges.get(other).copied().ok_or_else(invalid)?;
                        let w = match widest.get(other).copied().flatten() {
                            Some(w) => w,
                            None => widest_wedge_bound(k, target, id, gap)?.wedge,
                        };
                        if let Some(slot) = widest.get_mut(other) {
                            *slot = Some(w);
                        }
                        opens = opens.max(w);
                    }
                    let margin = FOLD_MARGIN * tolerance;
                    wedge = unfolded_wedge(wedge, [d1, d2], opens, margin);
                }
            }
            measured.push(wedge);
        }
        let mut chamfers = Vec::new();
        let mut first = Vec::new();
        let mut second = Vec::new();
        for (index, &id) in edges.iter().enumerate() {
            let u0 = self.blendable_edgeuse(k, id)?;
            let target: &crate::Brep = &*self.objects.target;
            let u1 = eu1::radial(target, u0)?.ok_or_else(invalid)?;
            // the support the first size is along: 1 unless `along` names support 2's face
            let on_second = match along.get(index) {
                None => false,
                Some(&face) => {
                    let face_of = |u| -> Result<Option<crate::topology::FaceId>> {
                        Ok(match eu1::faceuse(target, u)? {
                            Some(fu) => target.faceuse(fu)?.face,
                            None => None,
                        })
                    };
                    if face_of(u0)? == Some(face) {
                        false
                    } else if face_of(u1)? == Some(face) {
                        true
                    } else {
                        return Err(invalid());
                    }
                }
            };
            let size = value(sizes, index)?;
            // a knife edge has no wedge: equal distances are the chord between the contacts of
            // the ball of that radius, kept as the linear section on those offsets; other
            // sizes have no such chord
            if knife.get(index).copied().unwrap_or(false) {
                let ChamferSize::Distances([d, d2]) = value(sizes, index)? else {
                    return Err(invalid());
                };
                if !(d == d2 && d.is_finite() && d > 0.0) {
                    return Err(invalid());
                }
                first.push(d);
                second.push(d);
                chamfers.push(None);
                continue;
            }
            let wedge = measured.get(index).copied().ok_or_else(invalid)?;
            let asked =
                ChamferDefinition::new(size, wedge).map_err(|_| refused_size(id, size, wedge))?;
            let chamfer = if on_second {
                let [a, b] = asked.setbacks();
                let size = match asked.size {
                    ChamferSize::Distances(_) | ChamferSize::DistanceAngle { .. } => {
                        ChamferSize::Distances([b, a])
                    }
                    // a chord's setbacks are equal: the same along either face
                    chord @ ChamferSize::Chord(_) => chord,
                };
                ChamferDefinition::new(size, wedge).map_err(|_| invalid())?
            } else {
                asked
            };
            let [r1, r2] = chamfer.offsets().map_err(|_| invalid())?;
            first.push(r1);
            second.push(r2);
            chamfers.push(Some(chamfer));
        }
        let offsets: Vec<[f64; 2]> = first.iter().zip(&second).map(|(&a, &b)| [a, b]).collect();
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
                solver.base_mut().chamfer = chamfer;
            }
        }
        Ok(offsets)
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    #[test]
    fn a_chain_is_measured_on_its_edges_between_planes_else_at_its_narrowest() {
        let all = |_: usize| Ok(true);
        // none between planes: the narrowest, whichever edge asks
        let w = [1.0, 1.5, 1.2];
        for i in 0..3 {
            assert_eq!(chain_wedge(&w, &[false; 3], i, all).unwrap(), 1.0);
        }
        // one between planes: its wedge on the others, narrower or wider than their own
        let planes = [false, false, true];
        assert_eq!(chain_wedge(&w, &planes, 0, all).unwrap(), 1.2);
        assert_eq!(chain_wedge(&w, &planes, 1, all).unwrap(), 1.2);
        assert_eq!(chain_wedge(&w, &planes, 2, all).unwrap(), 1.2);
        // two between planes of different wedges: no one pair, each keeps its own
        let planes = [false, true, true];
        assert_eq!(chain_wedge(&w, &planes, 0, all).unwrap(), 1.0);
        assert_eq!(chain_wedge(&w, &planes, 1, all).unwrap(), 1.5);
        assert_eq!(chain_wedge(&w, &planes, 2, all).unwrap(), 1.2);
        // an edge of another chain or size does not count
        let alone = |other: usize| Ok(other != 0);
        assert_eq!(chain_wedge(&w, &[false; 3], 1, alone).unwrap(), 1.2);
    }

    /// The setback along a face at wedge `w` of a chamfer measured at `a`.
    fn setback(a: f64, [d1, d2]: [f64; 2], w: f64) -> f64 {
        let chamfer = ChamferDefinition::new(ChamferSize::Distances([d1, d2]), a).unwrap();
        let [r1, r2] = chamfer.offsets().unwrap();
        (r2 + r1 * w.cos()) / w.sin()
    }

    #[test]
    fn equal_sizes_take_the_narrowest_and_unequal_ones_stop_short_of_the_fold() {
        let tilt = 0.5_f64.atan();
        let widest = 0.5 * crate::math::PI + tilt;
        // equal sizes do not fold: the narrowest, whatever the wedges
        assert_eq!(unfolded_wedge(1.0, [1.528; 2], widest, 0.01), 1.0);
        // the rim's 1 and 2 opening to 116.57 degrees: cos a = (1 + 2 cos w - m sin w)
        // / (2 + cos w), and the setback at the widest station is above the margin, not far
        let (s, c) = widest.sin_cos();
        let m = 0.01;
        let a = unfolded_wedge(1.37, [1.0, 2.0], widest, m);
        assert!((a.cos() - (1.0 + 2.0 * c - m * s) / (2.0 + c)).abs() < 1e-15);
        let least = setback(a, [1.0, 2.0], widest);
        assert!(least >= m && least < 1.01 * m, "{least}");
        // measured at the narrowest the same chamfer folds
        assert!(setback(1.37, [1.0, 2.0], widest) < 0.0);
        // already past the bound: the narrowest stands
        assert_eq!(unfolded_wedge(1.6, [1.0, 2.0], widest, m), 1.6);
    }
}
