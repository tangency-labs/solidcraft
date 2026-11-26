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
use crate::blend::blend_refusal::{chains_among, setback_below_resolution, BlendRefusal};
use crate::blend::chamfer_side::widest_wedge_bound;
use crate::blend::fillet_executive::p2::FilletParametersAccess;
use crate::blend::fillet_solver::p5::is_knife_edge;
use crate::surface::procedural_blend::chamfer::{ChamferDefinition, ChamferSize};
use crate::surface::surface::SurfaceKind;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, EdgeId, EdgeuseId};
use crate::{EntityRef, Kernel, Result, SolidError};

/// The operation path of [`FilletSession::prepare_per_edge_two_offsets`]'s errors.
const OP: &str = "blend::fillet_executive::session::prepare_per_edge_two_offsets";

/// The operation path of [`FilletSession::prepare_chamfers_along`]'s errors (and so of
/// [`FilletSession::prepare_chamfers`]'s).
const OP_ALONG: &str = "blend::fillet_executive::session::prepare_chamfers_along";

/// A reviewed defect of the preparation at `op`: a list entry, edge use or definition it
/// checked or built itself before reading it, `why` saying why it is there. Status `SM_ERR_INVALID_INPUT`, as
/// the phrase cause it replaces had.
fn defect(op: &'static str, why: &'static str) -> SolidError {
    crate::error::Failure::new(op, crate::error::blend::BlendFailure::Defect { why })
        .with_status(crate::error::SM_ERR_INVALID_INPUT)
        .into_error()
}

/// The caller's chamfer size for edge `id` is not one it can take.
fn size_invalid(id: EdgeId) -> SolidError {
    SolidError::blend_at(
        OP_ALONG,
        crate::error::blend::BlendFailure::ChamferSizeInvalid,
        EntityRef::Edge(id),
    )
}

/// The caller's lists for `edges` edges: none empty where a value is needed (`lengths`
/// each with whether it may be empty), else one value or one per edge.
fn check_lists(op: &'static str, edges: usize, lengths: &[(usize, bool)]) -> Result<()> {
    if edges == 0 {
        return Err(SolidError::blend(
            op,
            crate::error::blend::BlendFailure::NoEdgesGiven,
        ));
    }
    for &(given, may_be_empty) in lengths {
        if !(given == edges || given == 1 || (may_be_empty && given == 0)) {
            return Err(SolidError::blend(
                op,
                crate::error::blend::BlendFailure::PerEdgeListLength { edges, given },
            ));
        }
    }
    Ok(())
}

/// Why a list entry is there: the caller's lists are checked one value or one per edge
/// ([`check_lists`]) before any is read, and the run-on edges added take one each.
const LISTS_CHECKED: &str = "the lists are checked one value or one per edge before any entry is read, and each run-on edge added takes one";

/// Why an edge's per-edge entry is there: the preparation pushes one per edge in order.
const ONE_PER_EDGE: &str =
    "the preparation pushes one wedge, measure and chain entry per edge, in the edges' order";

/// Why an edge's radial edge use is there: [`FilletSession::blendable_edgeuse`] has just
/// returned its blend edge use, which it refuses for an edge without two faces (a lamina).
const RADIAL_OF_BLENDABLE: &str =
    "the edge's blend edge use was just found, which needs the edge to lie between two faces";

/// The support a chamfer's first size is along: one of its edge's two faces, as the
/// caller named it, or support 1 or 2 of the edge's blend edge use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Along {
    Support { second: bool },
    Face(crate::topology::FaceId),
}

/// The tangency of a solver these prepare, radians: 10 degrees.
const TANGENCY: f64 = 10.0 * (crate::math::PI / 180.0);

/// How many of a chain's edge tolerances the smallest setback keeps at its widest station: a
/// bound that is open, as a refusal's fit size is (`docs/generic-blend.md`, §3.5).
const FOLD_MARGIN: f64 = 1000.0;

fn value<T: Copy>(op: &'static str, values: &[T], index: usize) -> Result<T> {
    values
        .get(if values.len() == 1 { 0 } else { index })
        .copied()
        .ok_or_else(|| defect(op, LISTS_CHECKED))
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
            .error(OP_ALONG, crate::error::SM_ERR_INVALID_INPUT)
        }
        _ => size_invalid(id),
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
/// its own and reports the conflicting planar wedges. Without a planar reference it is the
/// narrowest of the chain, whose offsets are the smallest, so
/// the setbacks are nowhere much past the sizes asked; unequal sizes can fold at it
/// ([`unfolded_wedge`]).
fn chain_wedge(
    wedges: &[f64],
    between_planes: &[bool],
    index: usize,
    alike: impl Fn(usize) -> Result<bool>,
) -> Result<(f64, bool)> {
    let own = wedges
        .get(index)
        .copied()
        .ok_or_else(|| defect(OP_ALONG, ONE_PER_EDGE))?;
    if between_planes.get(index).copied().unwrap_or(false) {
        return Ok((own, false));
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
                Some(a) if (a - w).abs() > room => return Ok((own, true)),
                Some(_) => {}
                None => anchor = Some(w),
            }
        }
    }
    Ok((anchor.unwrap_or(narrowest), false))
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
    /// `SetFilletParameters` (including disabling global merge), and splits the filleted
    /// edges' vertices by fans as it does. Solvers use four times the edge tolerance for
    /// approximation, 20 degrees for angle and 10 degrees for tangency. A shared
    /// generator is retained before loading; per-edge generators are retained
    /// after loading each solver. Initialization is set only when all loads
    /// succeed. Set execution switches after preparation, as for the existing
    /// per-edge preparation helpers.
    ///
    /// # Errors
    /// [`BlendFailure::NoEdgesGiven`](crate::error::blend::BlendFailure::NoEdgesGiven),
    /// [`BlendFailure::PerEdgeListLength`](crate::error::blend::BlendFailure::PerEdgeListLength)
    /// for a list neither one entry nor one per edge,
    /// [`BlendFailure::EdgeNotInTarget`](crate::error::blend::BlendFailure::EdgeNotInTarget)
    /// for a dead edge, and those of [`Self::blendable_edgeuse`] for an edge without a blend
    /// edge use. Constructor and loader errors propagate, leaving
    /// prior successful loads in the session, as in `SetFilletParameters`.
    pub fn prepare_per_edge_two_offsets(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        first: &[f64],
        second: &[f64],
        generators: &[FilletSurfaceGenerator],
    ) -> Result<()> {
        self.prepare_two_offsets(k, edges, first, second, generators, &[])
    }

    fn prepare_two_offsets(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        first: &[f64],
        second: &[f64],
        generators: &[FilletSurfaceGenerator],
        chamfers: &[Option<ChamferDefinition>],
    ) -> Result<()> {
        check_lists(
            OP,
            edges.len(),
            &[
                (first.len(), false),
                (second.len(), false),
                (generators.len(), false),
                (chamfers.len(), true),
            ],
        )?;
        let live_edges = crate::topology::brep::edges(self.target(), None)?;
        if let Some(index) = edges.iter().position(|id| !live_edges.contains(id)) {
            return Err(SolidError::blend(
                OP,
                crate::error::blend::BlendFailure::EdgeNotInTarget { index },
            ));
        }

        self.executive.initialized = false;
        self.executive.reset_parameter_state();
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

        let shared = if generators.len() == 1 {
            Some(self.add_surface_generator(value(OP, generators, 0)?)?)
        } else {
            None
        };
        for (index, &id) in edges.iter().enumerate() {
            let edgeuse = self.blendable_edgeuse(k, id)?;
            let approx_tol = 4.0 * edge::tolerance(self.target(), id)?;
            let chamfer = if chamfers.is_empty() {
                None
            } else {
                value(OP, chamfers, index)?
            };
            let r1 = value(OP, first, index)?;
            let angle = 20.0 * (crate::math::PI / 180.0);
            let solver = if let Some(c) = chamfer.filter(ChamferDefinition::is_setbacks) {
                // a chamfer of setbacks held at every station (#13710): its own solver, its
                // setbacks along this edge use's face and its radial's
                self.new_setback_solver(k, edgeuse, c.setbacks(), approx_tol, angle, TANGENCY)?
            } else if chamfer
                .and_then(ChamferDefinition::constant_setback)
                .is_some()
            {
                // The constant-distance solver seeds a radius of chord / sqrt(2).
                // Use the radius from this edge's reference wedge as the seed.
                self.new_constant_distance_solver(
                    k,
                    edgeuse,
                    r1 * std::f64::consts::SQRT_2,
                    approx_tol,
                    angle,
                    TANGENCY,
                )?
            } else {
                self.new_constant_radius_solver_with_second(
                    k,
                    edgeuse,
                    r1,
                    value(OP, second, index)?,
                    approx_tol,
                    angle,
                    TANGENCY,
                )?
            };
            if chamfer.is_some() {
                self.objects.solver_mut(solver)?.base_mut().chamfer = chamfer;
            }
            self.load_solver(k, solver)?;
            let generator = match shared {
                Some(generator) => generator,
                None => self.add_surface_generator(value(OP, generators, index)?)?,
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
    /// other. If the planar wedges require different offsets, a symmetric chamfer on
    /// the intervening curved faces keeps its setback at every station. The linear
    /// section joins its rails and the solver keeps the sizes: the
    /// chamfer's surface is its definition, the
    /// chord of order 0, and keeps them too. A distance is along the edge's blend edge
    /// use's face (support 1) or its radial mate's (support 2); an angle is to support 1.
    /// With fixed offsets on a curved face, the sizes hold where the wedge was
    /// measured ([`crate::surface::procedural_blend::chamfer`]). Not upstream (solidcore's, #2486).
    ///
    /// # Errors
    ///
    /// Those of [`Self::prepare_per_edge_two_offsets`], at
    /// `blend::fillet_executive::session::prepare_chamfers_along`;
    /// [`BlendFailure::ChamferSizeInvalid`](crate::error::blend::BlendFailure::ChamferSizeInvalid)
    /// at the edge for faces that do not make a wedge there (tangent, or folded back other
    /// than a knife edge chamfered at equal distances, which is the chord between the contacts
    /// of the ball of radius the distance), sizes or setbacks that are not finite and
    /// positive, or two distances that fold on it (one at most the other's `cos(wedge)`);
    /// [`BlendRefusal::ChamferAngleTooLarge`] for an angle past the wedge's room;
    /// [`BlendFailure::SizeBelowResolution`](crate::error::blend::BlendFailure::SizeBelowResolution)
    /// at the edge for a setback (a distance, or one computed from an angle or a chord at
    /// the wedge) below eight times the body's tolerance, along either face.
    pub fn prepare_chamfers(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        sizes: &[ChamferSize],
    ) -> Result<()> {
        self.prepare_chamfers_along(k, edges, sizes, &[])
            .map(|_| ())
    }

    /// `edges`, `sizes` and the support each edge's first size is along, with the edges the
    /// chamfers run on into across seams' vertices (`super::runs_on_across_seams`): each
    /// takes its picked edge's size along the same face, so a chamfer never ends at a seam
    /// (#2864). Where `along` is empty a picked edge's size is along its support 1, and a
    /// run-on edge's along its support 1 or, where its blend edge use lies on the picked
    /// edge's other face, its support 2.
    fn chamfers_run_on(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        sizes: &[ChamferSize],
        along: &[crate::topology::FaceId],
    ) -> Result<(Vec<EdgeId>, Vec<ChamferSize>, Vec<Along>)> {
        let run_ons = super::runs_on_across_seams(
            k,
            self.objects.target,
            edges,
            crate::math::rad_to_deg(TANGENCY),
        )?;
        let (mut edges, mut sizes) = (edges.to_vec(), sizes.to_vec());
        let mut supports: Vec<Along> = if along.is_empty() {
            vec![Along::Support { second: false }; edges.len()]
        } else {
            along.iter().map(|&face| Along::Face(face)).collect()
        };
        for run_on in run_ons {
            edges.push(run_on.edge);
            if sizes.len() != 1 {
                sizes.push(value(OP_ALONG, &sizes, run_on.source)?);
            }
            supports.push(match value(OP_ALONG, &supports, run_on.source)? {
                Along::Face(face) => Along::Face(face),
                Along::Support { second } => Along::Support {
                    second: second != run_on.flipped,
                },
            });
        }
        Ok((edges, sizes, supports))
    }

    /// [`Self::prepare_chamfers`], with each edge's first size along the face `along` names
    /// (empty: support 1 for every edge; else one face per edge, either face of its edge),
    /// and the offsets each edge's solver was given, `[support 1, support 2]` per edge (those
    /// of `edges` in order, then those of the edges they run on into across seams' vertices,
    /// `chamfers_run_on`). A
    /// size along support 2 is kept as it was asked when it is two distances (swapped onto
    /// the supports); a distance and an angle to support 2 is kept as its two setbacks at the
    /// wedge, as no size names an angle to support 2. Not upstream (solidcore's).
    ///
    /// # Errors
    ///
    /// Those of [`Self::prepare_chamfers`];
    /// [`BlendFailure::PerEdgeListLength`](crate::error::blend::BlendFailure::PerEdgeListLength)
    /// for `along` neither empty nor one face per edge, and
    /// [`BlendFailure::ChamferFaceNotOfEdge`](crate::error::blend::BlendFailure::ChamferFaceNotOfEdge)
    /// at the edge for a face that is neither of its edge's.
    pub fn prepare_chamfers_along(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        sizes: &[ChamferSize],
        along: &[crate::topology::FaceId],
    ) -> Result<Vec<[f64; 2]>> {
        check_lists(OP_ALONG, edges.len(), &[(sizes.len(), false)])?;
        if !(along.is_empty() || along.len() == edges.len()) {
            return Err(SolidError::blend(
                OP_ALONG,
                crate::error::blend::BlendFailure::PerEdgeListLength {
                    edges: edges.len(),
                    given: along.len(),
                },
            ));
        }
        let (edges, sizes, along) = self.chamfers_run_on(k, edges, sizes, along)?;
        let (edges, sizes, along) = (edges.as_slice(), sizes.as_slice(), along.as_slice());
        let mut wedges = Vec::new();
        let mut between_planes = Vec::new();
        let mut knife = Vec::new();
        let knife_tol = k.tolerances.ang_tol_deg * (crate::math::PI / 180.0);
        for &id in edges {
            let u0 = self.blendable_edgeuse(k, id)?;
            let target: &mut crate::Brep = &mut *self.objects.target;
            let u1 =
                eu1::radial(target, u0)?.ok_or_else(|| defect(OP_ALONG, RADIAL_OF_BLENDABLE))?;
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
                .map_err(|_| {
                    defect(
                        OP_ALONG,
                        "the edge's blend edge use was just found, so the edge is live",
                    )
                })?
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
                measured.push((0.0, false));
                continue;
            }
            let size = value(OP_ALONG, sizes, index)?;
            let alike = |other: usize| -> Result<bool> {
                Ok(chains.get(index) == chains.get(other)
                    && !knife.get(other).copied().unwrap_or(false)
                    && value(OP_ALONG, sizes, other)? == size)
            };
            let (mut wedge, conflicting_planes) =
                chain_wedge(&wedges, &between_planes, index, alike)?;
            let mut on_chain = Vec::new();
            let mut uniform = true;
            for other in 0..edges.len() {
                if alike(other)? {
                    on_chain.push(other);
                } else if chains.get(index) == chains.get(other) {
                    // A differently sized chamfer or a knife edge still has a fixed
                    // offset at a Join. Keep the existing compatibility check there.
                    uniform = false;
                }
            }
            let planes = |i: &usize| between_planes.get(*i).copied().unwrap_or(false);
            let setbacks = ChamferDefinition::new(size, wedge).map(|c| c.setbacks());
            if let (false, Ok([d1, d2])) = (on_chain.iter().any(planes), setbacks) {
                if d1 != d2 {
                    let mut tolerance = 0.0_f64;
                    for &other in &on_chain {
                        let id = edges.get(other).copied().ok_or_else(|| {
                            defect(OP_ALONG, "the chain's members are indices of the edges")
                        })?;
                        tolerance = tolerance.max(edge::tolerance(target, id)?);
                    }
                    // the bound's gap moves the fold test's numerator by at most `|d_2 - d_1
                    // cos a| <= d_1 + d_2` times it: by one tolerance at most, a thousandth of
                    // the margin
                    let gap = tolerance / (d1 + d2);
                    let mut opens = wedge;
                    for &other in &on_chain {
                        let id = edges.get(other).copied().ok_or_else(|| {
                            defect(OP_ALONG, "the chain's members are indices of the edges")
                        })?;
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
            // Different planar wedges cannot share an offset while retaining the requested
            // setback. Between these anchors a curved edge solves the offset at each station
            // so it meets the planar rails at their common wedge, even on a shifted side seam.
            let stationwise = conflicting_planes
                && uniform
                && matches!(size, ChamferSize::Distances([a, b]) if a == b);
            measured.push((wedge, stationwise));
        }
        let mut chamfers = Vec::new();
        let mut knives = Vec::new();
        let mut first = Vec::new();
        let mut second = Vec::new();
        for (index, &id) in edges.iter().enumerate() {
            let u0 = self.blendable_edgeuse(k, id)?;
            let target: &crate::Brep = &*self.objects.target;
            let u1 =
                eu1::radial(target, u0)?.ok_or_else(|| defect(OP_ALONG, RADIAL_OF_BLENDABLE))?;
            // the support the first size is along: 1 unless `along` names support 2
            let on_second = match value(OP_ALONG, along, index)? {
                Along::Support { second } => second,
                Along::Face(face) => {
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
                        return Err(SolidError::blend_at(
                            OP_ALONG,
                            crate::error::blend::BlendFailure::ChamferFaceNotOfEdge,
                            EntityRef::Edge(id),
                        ));
                    }
                }
            };
            let size = value(OP_ALONG, sizes, index)?;
            // a knife edge has no wedge: equal distances are the chord between the contacts of
            // the ball of that radius, kept as the linear section on those offsets; other
            // sizes have no such chord
            if knife.get(index).copied().unwrap_or(false) {
                let ChamferSize::Distances([d, d2]) = value(OP_ALONG, sizes, index)? else {
                    return Err(size_invalid(id));
                };
                if !(d == d2 && d.is_finite() && d > 0.0) {
                    return Err(size_invalid(id));
                }
                setback_below_resolution(OP_ALONG, id, [d, d2], self.objects.target.tolerance())?;
                first.push(d);
                second.push(d);
                chamfers.push(None);
                knives.push(id);
                continue;
            }
            let (wedge, stationwise) = measured
                .get(index)
                .copied()
                .ok_or_else(|| defect(OP_ALONG, ONE_PER_EDGE))?;
            let asked =
                ChamferDefinition::new(size, wedge).map_err(|_| refused_size(id, size, wedge))?;
            // a size whose setbacks at this wedge are not both positive and finite (an angle
            // so small its second setback underflows to 0, a setback that overflows) is not
            // one the edge can take, whichever face it is along: refused before the swap, so
            // both supports name it alike and the swapped size below always checks
            let [a, b] = asked.setbacks();
            if !(a.is_finite() && a > 0.0 && b.is_finite() && b > 0.0) {
                return Err(size_invalid(id));
            }
            // a setback is how far a rail lies from the edge along its face, whether it was
            // given as a distance or computed from an angle or a chord: one below the
            // smallest size a fillet builds (an angle so small its second setback is far
            // below the body's tolerance) puts the rail on the edge within tolerance, a
            // sliver, refused as such a distance is; before the swap, so either face the
            // first size is along names it alike
            setback_below_resolution(
                OP_ALONG,
                id,
                asked.setbacks(),
                self.objects.target.tolerance(),
            )?;
            let chamfer = if on_second {
                let size = match asked.size {
                    ChamferSize::Distances(_) | ChamferSize::DistanceAngle { .. } => {
                        ChamferSize::Distances([b, a])
                    }
                    // a chord's setbacks are equal: the same along either face
                    chord @ ChamferSize::Chord(_) => chord,
                };
                ChamferDefinition::new(size, wedge)
                    .map_err(|_| {
                        defect(
                            OP_ALONG,
                            "the setbacks were checked positive and finite just above, and the wedge was accepted",
                        )
                    })?
            } else {
                asked
            };
            let [r1, r2] = chamfer.offsets().map_err(|_| size_invalid(id))?;
            first.push(r1);
            second.push(r2);
            chamfers.push(Some(if stationwise {
                chamfer.at_every_station()?
            } else {
                chamfer
            }));
        }
        let offsets: Vec<[f64; 2]> = first.iter().zip(&second).map(|(&a, &b)| [a, b]).collect();
        self.prepare_two_offsets(
            k,
            edges,
            &first,
            &second,
            &[FilletSurfaceGenerator::Linear],
            &chamfers,
        )?;
        self.objects.knife_chamfers.extend(knives);
        Ok(offsets)
    }
}

impl FilletSession<'_> {
    /// Test-only, not upstream (#13710): chamfers of setbacks held at every station on
    /// `edges`, `distances[i]` the setbacks of edge i, the first along `along[i]` (one of the
    /// edge's faces) and the second along its other face. Nothing routes a command's chamfer
    /// to a chamfer of setbacks yet; this requests one through the executive as
    /// [`Self::prepare_chamfers_along`] requests today's, without its wedges, chains, run-ons
    /// or knife edges.
    #[cfg(test)]
    pub(crate) fn prepare_setback_chamfers(
        &mut self,
        k: &Kernel,
        edges: &[EdgeId],
        distances: &[[f64; 2]],
        along: &[crate::topology::FaceId],
    ) -> Result<()> {
        let mut first = Vec::new();
        let mut second = Vec::new();
        let mut chamfers = Vec::new();
        for (index, &id) in edges.iter().enumerate() {
            let u0 = self.blendable_edgeuse(k, id)?;
            let target: &crate::Brep = &*self.objects.target;
            let face0 = match eu1::faceuse(target, u0)? {
                Some(fu) => target.faceuse(fu)?.face,
                None => None,
            };
            let [a, b] = value(OP_ALONG, distances, index)?;
            let [d1, d2] = if face0 == Some(value(OP_ALONG, along, index)?) {
                [a, b]
            } else {
                [b, a]
            };
            first.push(d1);
            second.push(d2);
            chamfers.push(Some(ChamferDefinition::of_setbacks(
                ChamferSize::Distances([d1, d2]),
            )?));
        }
        self.prepare_two_offsets(
            k,
            edges,
            &first,
            &second,
            &[FilletSurfaceGenerator::Linear],
            &chamfers,
        )
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used)]
mod tests {
    use super::*;

    /// The preparations' refusals of their values are typed, with the status of the bare
    /// `SM_ERR_INVALID_INPUT` they replaced (#13790).
    #[test]
    fn refused_values_are_typed_invalid_values() {
        use crate::error::blend::BlendFailure;
        let mut k = Kernel::default();
        let mut body = crate::topology::brep::p3::new_brep(
            &mut k,
            crate::geom::tol::USE_DEFAULT,
            crate::geom::tol::USE_DEFAULT,
        )
        .unwrap();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        let dead = EdgeId::from_index(7);
        let cases = [
            (
                session.prepare_per_edge_two_offsets(&k, &[], &[1.0], &[1.0], &[]),
                BlendFailure::NoEdgesGiven,
            ),
            (
                session.prepare_per_edge_two_offsets(
                    &k,
                    &[dead],
                    &[1.0],
                    &[1.0],
                    &[FilletSurfaceGenerator::Linear],
                ),
                BlendFailure::EdgeNotInTarget { index: 0 },
            ),
            (
                session
                    .prepare_chamfers_along(&k, &[], &[ChamferSize::Distances([1.0; 2])], &[])
                    .map(|_| ()),
                BlendFailure::NoEdgesGiven,
            ),
            (
                session.prepare_per_edge_two_offsets(
                    &k,
                    &[dead, dead, dead],
                    &[1.0],
                    &[1.0, 2.0],
                    &[FilletSurfaceGenerator::Linear],
                ),
                BlendFailure::PerEdgeListLength { edges: 3, given: 2 },
            ),
        ];
        for (result, cause) in cases {
            let e = result.unwrap_err();
            assert_eq!(e.blend_failure(), Some(&cause), "{e:?}");
            assert_eq!(e.kind(), crate::error::ErrorKind::InvalidInput, "{e:?}");
            assert_eq!(
                e.upstream_status(),
                Some(crate::error::SM_ERR_INVALID_INPUT)
            );
        }
    }

    /// A chamfer size an edge cannot take, and a face for the first size that is not one of
    /// the edge's, are the caller's input: named causes at the edge, status
    /// `SM_ERR_INVALID_INPUT` (#19824).
    #[test]
    fn a_bad_chamfer_size_or_face_is_named_at_its_edge() {
        use crate::error::blend::BlendFailure;
        let mut k = Kernel::default();
        let mut body = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        let e = crate::topology::brep::p1::edges(&body, None).unwrap()[0];
        let of_edge = edge::faces(&body, e, None).unwrap();
        let other = crate::topology::brep::p1::faces(&body, None)
            .unwrap()
            .into_iter()
            .find(|f| !of_edge.contains(f))
            .unwrap();
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        let cases = [
            (
                session.prepare_chamfers(&k, &[e], &[ChamferSize::Distances([-1.0, 0.1])]),
                BlendFailure::ChamferSizeInvalid,
            ),
            (
                session
                    .prepare_chamfers_along(&k, &[e], &[ChamferSize::Distances([0.1; 2])], &[other])
                    .map(|_| ()),
                BlendFailure::ChamferFaceNotOfEdge,
            ),
        ];
        for (result, cause) in cases {
            let e_err = result.unwrap_err();
            assert_eq!(e_err.blend_failure(), Some(&cause), "{e_err:?}");
            assert_eq!(
                e_err.kind(),
                crate::error::ErrorKind::InvalidInput,
                "{e_err:?}"
            );
            assert_eq!(e_err.entity(), Some(EntityRef::Edge(e)), "{e_err:?}");
            assert_eq!(e_err.operation(), OP_ALONG, "{e_err:?}");
            assert_eq!(
                e_err.upstream_status(),
                Some(crate::error::SM_ERR_INVALID_INPUT)
            );
        }
    }

    /// A distance and an angle so small that the second setback underflows to 0 is a size
    /// the edge cannot take along either of its faces: the same named cause at the edge
    /// whichever face the first size is along, never a defect on the swap to support 2
    /// (review of #20927).
    #[test]
    fn a_size_whose_setback_underflows_is_refused_alike_along_either_face() {
        use crate::error::blend::BlendFailure;
        let size = ChamferSize::DistanceAngle {
            distance: 0.001,
            angle: 5e-324,
        };
        let mut k = Kernel::default();
        let mut body = crate::primitive::api_primitives::create_box(
            &mut k,
            crate::math::Point3::new(0.0, 0.0, 0.0),
            1.0,
            1.0,
            1.0,
        )
        .unwrap();
        let e = crate::topology::brep::p1::edges(&body, None).unwrap()[0];
        let of_edge = edge::faces(&body, e, None).unwrap();
        assert_eq!(of_edge.len(), 2);
        let mut session = FilletSession::new(&mut k, &mut body).unwrap();
        for face in of_edge {
            let err = session
                .prepare_chamfers_along(&k, &[e], &[size], &[face])
                .unwrap_err();
            assert_eq!(
                err.blend_failure(),
                Some(&BlendFailure::ChamferSizeInvalid),
                "along {face:?}: {err:?}"
            );
            assert_eq!(err.kind(), crate::error::ErrorKind::InvalidInput, "{err:?}");
            assert_eq!(err.entity(), Some(EntityRef::Edge(e)), "{err:?}");
            assert_eq!(err.operation(), OP_ALONG, "{err:?}");
            assert_eq!(
                err.upstream_status(),
                Some(crate::error::SM_ERR_INVALID_INPUT)
            );
        }
    }

    #[test]
    fn a_chain_is_measured_on_its_edges_between_planes_else_at_its_narrowest() {
        let all = |_: usize| Ok(true);
        // none between planes: the narrowest, whichever edge asks
        let w = [1.0, 1.5, 1.2];
        for i in 0..3 {
            assert_eq!(chain_wedge(&w, &[false; 3], i, all).unwrap(), (1.0, false));
        }
        // one between planes: its wedge on the others, narrower or wider than their own
        let planes = [false, false, true];
        assert_eq!(chain_wedge(&w, &planes, 0, all).unwrap(), (1.2, false));
        assert_eq!(chain_wedge(&w, &planes, 1, all).unwrap(), (1.2, false));
        assert_eq!(chain_wedge(&w, &planes, 2, all).unwrap(), (1.2, false));
        // two between planes of different wedges: no one pair, each keeps its own
        let planes = [false, true, true];
        assert_eq!(chain_wedge(&w, &planes, 0, all).unwrap(), (1.0, true));
        assert_eq!(chain_wedge(&w, &planes, 1, all).unwrap(), (1.5, false));
        assert_eq!(chain_wedge(&w, &planes, 2, all).unwrap(), (1.2, false));
        // an edge of another chain or size does not count
        let alone = |other: usize| Ok(other != 0);
        assert_eq!(
            chain_wedge(&w, &[false; 3], 1, alone).unwrap(),
            (1.2, false)
        );
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

    /// The unit box with its corner at (1, 1) cut off from (1, 0.5) to (0.5, 1), from z = 0
    /// to 1, and its edge that is either the box's square edge on x = 1, y = 0 (a wedge of 90
    /// degrees) or the cut's obtuse edge on x = 1, y = 0.5 (135 degrees).
    fn cut_box(k: &mut Kernel, obtuse: bool) -> (crate::Brep, EdgeId) {
        use crate::curve::curve::Curve;
        let pts = [(0.0, 0.0), (1.0, 0.0), (1.0, 0.5), (0.5, 1.0), (0.0, 1.0)];
        let curves: Vec<Curve> = (0..pts.len())
            .map(|i| {
                let (a, b) = (pts[i], pts[(i + 1) % pts.len()]);
                Curve::from(
                    crate::primitive::api_curves::create_line_segment(
                        crate::math::Vec3::new(a.0, a.1, 0.0),
                        crate::math::Vec3::new(b.0, b.1, 0.0),
                    )
                    .unwrap(),
                )
            })
            .collect();
        let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
        let body =
            crate::sweep::api_brep::create_taper_extrude(k, &refs, 1.0, 0.0, 3, false).unwrap();
        let at = crate::math::Point3::new(1.0, if obtuse { 0.5 } else { 0.0 }, 0.5);
        let e = body
            .edge_list()
            .iter()
            .copied()
            .find(|&e| {
                crate::query::api_queries::edge_closest_point(k, &body, e, at)
                    .unwrap()
                    .closest
                    .distance
                    < 1e-9
            })
            .unwrap();
        (body, e)
    }

    /// #21004: a distance and an angle so small that the second setback, `distance sin(angle)
    /// / sin(wedge + angle)`, is far below the body's tolerance (0.001 and 1e-300: about
    /// 1.4e-303 on the obtuse 135-degree edge, 1e-303 on the square one) is refused as a
    /// distance that small is, `SizeBelowResolution` at the edge, alike along either face;
    /// the same distance at an angle of 22.5 degrees (setback 0.001 on the obtuse edge,
    /// 0.0004 on the square one) still prepares.
    #[test]
    fn a_setback_an_angle_gives_below_resolution_is_refused_alike_along_either_face() {
        use crate::error::blend::BlendFailure;
        let sliver = ChamferSize::DistanceAngle {
            distance: 0.001,
            angle: 1e-300,
        };
        let good = ChamferSize::DistanceAngle {
            distance: 0.001,
            angle: crate::math::PI / 8.0,
        };
        for obtuse in [true, false] {
            let mut k = Kernel::default();
            let (mut body, e) = cut_box(&mut k, obtuse);
            let tolerance = body.tolerance();
            let of_edge = edge::faces(&body, e, None).unwrap();
            assert_eq!(of_edge.len(), 2);
            for face in of_edge {
                let err = FilletSession::new(&mut k, &mut body)
                    .unwrap()
                    .prepare_chamfers_along(&k, &[e], &[sliver], &[face])
                    .unwrap_err();
                let Some(&BlendFailure::SizeBelowResolution { size, tolerance: t }) =
                    err.blend_failure()
                else {
                    panic!("obtuse {obtuse}, along {face:?}: {err:?}");
                };
                // the second setback, not the distance asked: positive, finite, a sliver
                assert!(size > 0.0 && size < 1e-302, "{size}");
                assert_eq!(t, tolerance);
                assert_eq!(err.kind(), crate::error::ErrorKind::InvalidInput, "{err:?}");
                assert_eq!(err.entity(), Some(EntityRef::Edge(e)), "{err:?}");
                assert_eq!(
                    err.operation(),
                    "blend::fillet_executive::session::prepare_chamfers_along"
                );
                assert_eq!(
                    err.upstream_status(),
                    Some(crate::error::SM_ERR_INVALID_INPUT)
                );
                FilletSession::new(&mut k, &mut body)
                    .unwrap()
                    .prepare_chamfers_along(&k, &[e], &[good], &[face])
                    .unwrap();
            }
        }
    }

    /// Chamfers of setbacks held at every station (#13710), through their solver's trace.
    mod setbacks {
        use super::*;
        use crate::geom::extent3d::Extent3d;
        use crate::math::{Point3, Vec3, PI};
        use crate::surface::surface::SurfaceKind;
        use crate::topology::{Brep, FaceId};

        /// Every fix on, as ops runs.
        fn fixed_kernel() -> Kernel {
            let mut k = Kernel::default();
            for &name in crate::kernel::UpstreamFixes::NAMES {
                *k.fixes.flag_mut(name).unwrap() = true;
            }
            k
        }

        fn face_box(k: &Kernel, b: &Brep, f: FaceId) -> Extent3d {
            crate::query::api_queries::face_bounding_box(k, b, f, true).unwrap()
        }

        /// The faces of `b` whose tight boxes `keep` takes.
        fn face_holding(k: &Kernel, b: &Brep, keep: impl Fn(&Extent3d) -> bool) -> Vec<FaceId> {
            crate::topology::brep::p1::faces(b, None)
                .unwrap()
                .into_iter()
                .filter(|&f| keep(&face_box(k, b, f)))
                .collect()
        }

        /// `edges` of `body` chamfered by setbacks held at every station, `distances[i]` the first
        /// along `along[i]`: the session's result, every corner bevelled.
        fn chamfer_setbacks(
            k: &mut Kernel,
            body: &mut Brep,
            edges: &[EdgeId],
            distances: &[[f64; 2]],
            along: &[FaceId],
        ) -> Result<()> {
            let mut s = FilletSession::new(k, body)?;
            s.prepare_setback_chamfers(k, edges, distances, along)?;
            s.create_corners(k, None)?;
            for corner in s.fillet_corners()? {
                s.set_corner_bevel(corner, true)?;
            }
            s.do_filleting(k, None)
        }

        /// The faces of `b` that are chamfers of setbacks by their definition, each with its
        /// certified bound (re-certified).
        fn setback_faces(k: &Kernel, b: &Brep) -> Vec<(FaceId, f64)> {
            let mut out = Vec::new();
            for f in crate::topology::brep::p1::faces(b, None).unwrap() {
                let Some(surface) = b.face(f).unwrap().surface.as_ref() else {
                    continue;
                };
                if let SurfaceKind::ProceduralBlend(pb) = surface.kind() {
                    if pb.definition().chamfer.is_some_and(|c| c.is_setbacks()) {
                        out.push((f, pb.recertify(k).unwrap()));
                    }
                }
            }
            out
        }

        /// `b` is a closed manifold solid.
        fn closed(b: &Brep) -> bool {
            crate::topology::brep::p2::is_manifold_solid(b).unwrap()
        }

        fn volume(k: &Kernel, b: &mut Brep) -> f64 {
            crate::mass::integrator::properties::brep_volume_default(k, b, 1e-9).unwrap()
        }

        /// Points along the interior of edge `e` (fractions 0.1 to 0.9 of its interval).
        fn along_edge(k: &Kernel, b: &Brep, e: EdgeId) -> Vec<Point3> {
            let ivl = edge::interval(b, e).unwrap();
            let curve = edge::curve(b, e).unwrap().unwrap();
            (1..10)
                .map(|i| {
                    curve
                        .evaluate_point(k, ivl.evaluate(f64::from(i) / 10.0))
                        .unwrap()
                })
                .collect()
        }

        /// The top rim of half a cylinder of radius 10 and height 20 (from 0 to 180 degrees about
        /// its axis, an open arc between the flat face's corners) chamfered by setbacks 1 along
        /// the top and 0.5 down the wall, through the setback solver's trace: in closed form the
        /// chamfer is the cone between the arcs of radius 9 at z = 20 and radius 10 at z = 19.5,
        /// its rails those arcs (each foot a straight 1 or 0.5 from the rim, square to it), its
        /// ends on the flat face (a plane through the axis, the station planes' own there), and
        /// the solid loses the triangle of legs 1 and 0.5 turned half a turn about the axis, `pi
        /// (10 - 1/3) (1/2)(1)(0.5)` (Pappus: the triangle's centroid at radius 10 - 1/3).
        #[test]
        fn a_half_cylinder_rim_chamfered_by_setbacks_is_the_closed_form_cone() {
            let (r, h, d) = (10.0, 20.0, [1.0, 0.5]);
            let mut k = fixed_kernel();
            let mut b = crate::primitive::api_primitives::create_partial_cylinder(
                &mut k,
                Point3::new(0.0, 0.0, 0.0),
                r,
                h,
                0.0,
                180.0,
            )
            .unwrap();
            assert!(closed(&b));
            let v0 = volume(&k, &mut b);
            let flat = face_holding(&k, &b, |bx| {
                bx.min().y.abs() < 1e-6 && bx.max().y.abs() < 1e-6
            });
            // (the half turn's cut: two coplanar faces in y = 0, from the axis to either end
            // of the rim)
            assert_eq!(flat.len(), 2);
            let rim: Vec<EdgeId> = crate::topology::brep::p1::edges(&b, None)
                .unwrap()
                .into_iter()
                .filter(|&e| {
                    along_edge(&k, &b, e)
                        .iter()
                        .all(|p| (p.z - h).abs() < 1e-9 && p.y > 1e-6)
                })
                .collect();
            assert_eq!(rim.len(), 1, "{rim:?}");
            let top = face_holding(&k, &b, |bx| {
                (bx.min().z - h).abs() < 1e-9 && (bx.max().z - h).abs() < 1e-9
            });
            assert_eq!(top.len(), 1);
            chamfer_setbacks(&mut k, &mut b, &rim, &[d], &top).unwrap();
            assert!(closed(&b));
            // (the flat faces as the chamfered body numbers them)
            let flat = face_holding(&k, &b, |bx| {
                bx.min().y.abs() < 1e-6 && bx.max().y.abs() < 1e-6
            });
            assert_eq!(flat.len(), 2);
            let faces = setback_faces(&k, &b);
            assert_eq!(faces.len(), 1, "one chamfer of setbacks");
            let (chamfer, bound) = faces[0];
            let mut rails = 0;
            for e in crate::topology::face::p4::edges(&b, chamfer, None, false, None).unwrap() {
                // (its ends lie on the flat faces)
                if edge::faces(&b, e, None)
                    .unwrap()
                    .iter()
                    .any(|f| flat.contains(f))
                {
                    continue;
                }
                let tol = edge::tolerance(&b, e).unwrap() + bound + 1e-9;
                for p in along_edge(&k, &b, e) {
                    let radius = (p.x * p.x + p.y * p.y).sqrt();
                    if (p.z - h).abs() < 1e-3 {
                        assert!((radius - (r - d[0])).abs() <= tol, "top rail {p:?}");
                    } else {
                        assert!(
                            (radius - r).abs() <= tol && (p.z - (h - d[1])).abs() <= tol,
                            "wall rail {p:?}"
                        );
                    }
                    rails += 1;
                }
            }
            assert!(rails >= 18, "{rails}");
            // the chord at the half turn's middle (theta = 90 degrees) runs from (r, z) =
            // (9, 20) to (10, 19.5), on the line r = 9 + 2 (20 - z): its midpoint (0, 9.5,
            // 19.75) lies on the chamfer, so the surface's nearest point to it is within the
            // bound, and on that line, the residual in r within sqrt(5) times the bound (the
            // line's normal in (r, z) is (1, 2) / sqrt(5))
            let mid = Point3::new(0.0, 9.5, 19.75);
            let surface = b.face(chamfer).unwrap().surface.clone().unwrap();
            let domain = surface.natural_uv_domain_with_nurb(&k).unwrap();
            let dropped = surface
                .drop_point::<crate::surface::surface::p4::NoObject>(
                    &k,
                    mid,
                    &domain,
                    &crate::surface::surface::p5::DropPointOptions::default(),
                )
                .unwrap();
            assert!(dropped.success);
            let on = surface.evaluate_point(&k, dropped.result_uv).unwrap();
            assert!(
                on.distance(mid) <= bound + 1e-9,
                "{on:?} off the chord's middle"
            );
            let radius = (on.x * on.x + on.y * on.y).sqrt();
            assert!(
                (radius - (r - d[0] + 2.0 * (h - on.z))).abs() <= 5.0_f64.sqrt() * bound + 1e-9,
                "{on:?} off the cone's ruling"
            );
            let removed = PI * (r - d[0] / 3.0) * 0.5 * d[0] * d[1];
            let cone_area = PI * (r - 0.5 * d[0]) * (d[0] * d[0] + d[1] * d[1]).sqrt();
            let v = volume(&k, &mut b);
            assert!(
                (v - (v0 - removed)).abs() <= bound * cone_area + 1e-8 * v0,
                "{v} vs {} (bound {bound})",
                v0 - removed
            );
        }

        /// zhuangzard's enclosure front (tangency-labs/solidcraft#13710), in x (width), y
        /// (height) and z (depth, the front at z = 0): the side profile (depth, height) [0, 0],
        /// [60, 0], [60, 40], [20, 40], [0, 15] swept 120 across, meeting the 100 x 40 front
        /// outline whose top corners are rounds of 10 swept along the depth. The slanted face
        /// runs from height 15 at the front to 40 at depth 20; the edges it shares with the top,
        /// the two rounds and the two sides are one tangent chain.
        fn enclosure_front(k: &mut Kernel) -> Brep {
            use crate::curve::curve::Curve;
            use crate::primitive::api_curves::{create_arc, create_line_segment};
            use crate::sweep::api_brep::create_linear_sweep;
            let line = |a: [f64; 3], b: [f64; 3]| {
                Curve::from(
                    create_line_segment(
                        Point3::new(a[0], a[1], a[2]),
                        Point3::new(b[0], b[1], b[2]),
                    )
                    .unwrap(),
                )
            };
            // the side profile in the plane x = -60: (depth, height) -> (x, height, depth)
            let side = [
                [0.0, 0.0],
                [60.0, 0.0],
                [60.0, 40.0],
                [20.0, 40.0],
                [0.0, 15.0],
            ];
            let curves: Vec<Curve> = (0..side.len())
                .map(|i| {
                    let (a, b) = (side[i], side[(i + 1) % side.len()]);
                    line([-60.0, a[1], a[0]], [-60.0, b[1], b[0]])
                })
                .collect();
            let refs: Vec<Option<&Curve>> = curves.iter().map(Some).collect();
            let profile =
                create_linear_sweep(k, &refs, Vec3::new(1.0, 0.0, 0.0), 120.0, true).unwrap();
            // the front outline in the plane z = -10, its top corners rounds of 10
            let z = -10.0;
            let outline = [
                line([-50.0, 0.0, z], [50.0, 0.0, z]),
                line([50.0, 0.0, z], [50.0, 30.0, z]),
                Curve::from(create_arc(Point3::new(40.0, 30.0, z), 10.0, 0.0, 90.0).unwrap()),
                line([40.0, 40.0, z], [-40.0, 40.0, z]),
                Curve::from(create_arc(Point3::new(-40.0, 30.0, z), 10.0, 90.0, 180.0).unwrap()),
                line([-50.0, 30.0, z], [-50.0, 0.0, z]),
            ];
            let refs: Vec<Option<&Curve>> = outline.iter().map(Some).collect();
            let front =
                create_linear_sweep(k, &refs, Vec3::new(0.0, 0.0, 1.0), 80.0, true).unwrap();
            crate::boolean::api_brep::boolean_intersection(k, profile, front).unwrap()
        }

        /// Each round of the enclosure front's chain alone (a quarter of the cylinder of radius
        /// 10 about (+-40, 30) along the depth, cut by the slanted plane: its edge an arc of an
        /// ellipse) chamfered by setbacks 0.8 and 0.8: the circle of radius 0.8 about the edge
        /// point, in the plane square to the edge, crosses the cylinder twice, once on the
        /// round's face (the slanted plane's inner side, where (p - (0, 15, 0)) . (0, 20, -25)
        /// < 0, as at the face's point (-40, 40, 30): -250) and once past the slant in the
        /// material the cut removed. A station split seeded at its ends' mean uv found the
        /// second (at s = 0.25 of the round's edge, foot 1 at z 18.67 against 20.2 beside it)
        /// and the trace never closed its span. Every rail point on the round lies on the
        /// face's side, to the face's certified bound and the rail's tolerance; the solid is
        /// closed, with chamfers of setbacks (the round's, and its run-on past its smooth end
        /// onto the side's edge).
        #[test]
        fn a_round_of_the_enclosure_front_chamfered_alone_keeps_its_feet_on_its_face() {
            let d = [0.8, 0.8];
            let n = Vec3::new(0.0, 20.0, -25.0).unitize().unwrap();
            let o = Point3::new(0.0, 15.0, 0.0);
            for side in [-1.0, 1.0] {
                let mut k = fixed_kernel();
                let mut b = enclosure_front(&mut k);
                let slant = face_holding(&k, &b, |bx| {
                    bx.min().z.abs() < 1e-6
                        && (bx.max().z - 20.0).abs() < 1e-6
                        && (bx.min().y - 15.0).abs() < 1e-6
                        && (bx.max().y - 40.0).abs() < 1e-6
                })[0];
                let round: Vec<EdgeId> =
                    crate::topology::face::p4::edges(&b, slant, None, false, None)
                        .unwrap()
                        .into_iter()
                        .filter(|&e| {
                            along_edge(&k, &b, e)
                                .iter()
                                .all(|p| p.x * side > 40.0 && p.y > 30.0)
                        })
                        .collect();
                assert_eq!(round.len(), 1, "{side}");
                chamfer_setbacks(&mut k, &mut b, &round, &[d], &[slant]).unwrap();
                assert!(closed(&b), "{side}: not closed");
                let faces = setback_faces(&k, &b);
                assert!(!faces.is_empty(), "{side}");
                let mut on_round = 0;
                for &(f, bound) in &faces {
                    for e in crate::topology::face::p4::edges(&b, f, None, false, None).unwrap() {
                        let tol = edge::tolerance(&b, e).unwrap() + bound + 1e-9;
                        for p in along_edge(&k, &b, e) {
                            let r = ((p.x - 40.0 * side).powi(2) + (p.y - 30.0).powi(2)).sqrt();
                            if (r - 10.0).abs() > tol || (p - o).dot(n).abs() <= tol {
                                continue;
                            }
                            assert!((p - o).dot(n) <= tol, "{side}: {p:?} past the slant");
                            on_round += 1;
                        }
                    }
                }
                assert!(on_round >= 9, "{side}: {on_round}");
            }
        }

        /// The station of the enclosure front's round at x > 40 (the cylinder `(x − 40)² +
        /// (y − 30)² = 100` cut by the slanted plane `z = 0.8 (y − 15)`), a quarter of the way
        /// along its edge, seeded at the setback circle's other crossing of each support (the
        /// mirror root, on the far side of the edge), with setbacks 0.8 and 0.8.
        ///
        /// The feet in closed form. The edge is the ellipse `C(θ) = (40 + 10 cos θ, 30 + 10 sin
        /// θ, 12 + 8 sin θ)`, `C′ = (−10 sin θ, 10 cos θ, 8 cos θ)`. A point of the cylinder is
        /// `S = (40 + 10 cos φ, 30 + 10 sin φ, z)`; with `δ = φ − θ` and `Δz = z − C_z`,
        /// `(S − C) · C′ = 100 sin δ + 8 cos θ Δz`, so square to the edge `Δz = −12.5 sin δ / cos
        /// θ`, and `|S − C|² = 200 (1 − cos δ) + 156.25 sin² δ / cos² θ = d²`: with `x = cos δ`,
        /// `a = 156.25 / cos² θ`, `a x² + 200 x + (d² − 200 − a) = 0`, whose root near 1 gives
        /// two feet `δ = ±acos x`, mirror images through the edge. The round's own has `z > 0.8
        /// (y − 15)` (the face's side of the slanted plane, where `20 (y − 15) − 25 z < 0`). On
        /// the slanted plane the foot is `C ± d u`, `u` the unit of `n × C′` (n its normal
        /// `(0, 20, −25)`): the plane's own runs into the wedge, towards the cylinder's axis,
        /// `u · (cos θ, sin θ, 0) < 0`; the other runs off the face beyond the edge.
        ///
        /// Seeded at the mirror feet, Newton stays on them; the station must decide the branch
        /// (each foot on the wedge's side of the other support) and re-seed, ending on the
        /// round's own feet, never answering the mirror (a trace span that then never passes
        /// its test, to the 100000-point limit).
        #[test]
        fn a_round_station_seeded_at_its_mirror_feet_ends_on_the_round() {
            use crate::blend::fillet_custom_solver::surface_global_point_solve;
            use crate::blend::fillet_geom::p1::{FilletBrep, FilletSolverId};
            use crate::blend::fillet_standard_solver::p1::SolverRails;
            use crate::blend::fillet_standard_solver::setback::SetbackFs;
            use crate::geom::vector2d::Point2;
            use crate::surface::surface::Surface;
            let d = [0.8, 0.8];
            let mut k = fixed_kernel();
            let mut b = enclosure_front(&mut k);
            let slant = face_holding(&k, &b, |bx| {
                bx.min().z.abs() < 1e-6
                    && (bx.max().z - 20.0).abs() < 1e-6
                    && (bx.min().y - 15.0).abs() < 1e-6
                    && (bx.max().y - 40.0).abs() < 1e-6
            })[0];
            let round: Vec<EdgeId> = crate::topology::face::p4::edges(&b, slant, None, false, None)
                .unwrap()
                .into_iter()
                .filter(|&e| {
                    along_edge(&k, &b, e)
                        .iter()
                        .all(|p| p.x > 40.0 && p.y > 30.0)
                })
                .collect();
            assert_eq!(round.len(), 1);
            // rail 0 on the slanted plane: the edge use on its face
            let eu = b
                .edge(round[0])
                .unwrap()
                .edgeuses
                .iter()
                .copied()
                .find(|&u| {
                    let fu = eu1::faceuse(&b, u).unwrap().unwrap();
                    b.faceuse(fu).unwrap().face == Some(slant)
                })
                .unwrap();
            let mut pseudo = FilletBrep::new();
            let mut fs = SetbackFs::from_edgeuse(
                &k,
                &mut b,
                &mut pseudo,
                FilletSolverId(0),
                1e-5,
                0.1,
                0.01,
                eu,
                d,
            )
            .unwrap();
            let domains = fs.trace_domains(&k).unwrap();
            let s = fs.edge_interval.evaluate(0.25);
            let jet = fs.jet(&k, s).unwrap();
            let c = jet.point;
            let theta = (c.y - 30.0).atan2(c.x - 40.0);
            let (ct, st) = (theta.cos(), theta.sin());
            // the edge point is on the ellipse
            let ellipse = Point3::new(40.0 + 10.0 * ct, 30.0 + 10.0 * st, 12.0 + 8.0 * st);
            assert!(
                c.distance(ellipse) < 1e-9,
                "{c:?} off the ellipse {ellipse:?}"
            );
            let a = 156.25 / (ct * ct);
            let x = (-200.0 + (200.0f64.powi(2) - 4.0 * a * (d[1] * d[1] - 200.0 - a)).sqrt())
                / (2.0 * a);
            let delta = x.acos();
            let on_cylinder = |dl: f64| {
                let phi = theta + dl;
                Point3::new(
                    40.0 + 10.0 * phi.cos(),
                    30.0 + 10.0 * phi.sin(),
                    c.z - 12.5 * dl.sin() / ct,
                )
            };
            let face_side = |p: Point3| p.z > 0.8 * (p.y - 15.0);
            let [mut round_foot, mut round_mirror] = [on_cylinder(delta), on_cylinder(-delta)];
            if !face_side(round_foot) {
                std::mem::swap(&mut round_foot, &mut round_mirror);
            }
            assert!(face_side(round_foot) && !face_side(round_mirror));
            let tau = Vec3::new(-10.0 * st, 10.0 * ct, 8.0 * ct);
            let u = Vec3::new(0.0, 20.0, -25.0).cross(tau).unitize().unwrap();
            let radial = Vec3::new(ct, st, 0.0);
            let u = if u.dot(radial) < 0.0 { u } else { -u };
            let want = [c + u * d[0], round_foot];
            let mirror = [c - u * d[0], round_mirror];
            let guesses = [0, 1].map(|rail| {
                let support =
                    Surface::from(fs.curve_based.solver.rail(rail).unwrap().surface.clone());
                let uv: Point2 = surface_global_point_solve(
                    &k,
                    &support,
                    None,
                    &domains[rail],
                    mirror[rail],
                    1e-20,
                )
                .unwrap()[0];
                assert!(
                    support
                        .evaluate_point(&k, uv)
                        .unwrap()
                        .distance(mirror[rail])
                        < 1e-9,
                    "rail {rail}: the mirror foot is on its support"
                );
                uv
            });
            let station = fs
                .station(&k, s, guesses, &domains)
                .unwrap()
                .expect("a station");
            // the same feet given (never re-solved) are refused as beyond the edge, foot 0
            // first
            let err = fs
                .station_through(&k, s, guesses, &domains)
                .expect_err("the mirror feet are refused");
            assert!(
                matches!(
                    err.blend_failure(),
                    Some(&crate::error::blend::BlendFailure::Solver(
                        crate::error::blend::SolverFailure::SetbackFootMirrored { rail: 0, .. }
                    ))
                ),
                "{err:?}"
            );
            for rail in 0..2 {
                let p = station.feet[rail].point;
                assert!(
                    p.distance(want[rail]) < 1e-9,
                    "rail {rail}: {p:?}, not the round's {:?} (mirror {:?})",
                    want[rail],
                    mirror[rail]
                );
            }
        }

        /// The enclosure front's slanted chain chamfered by equal setbacks 0.02, small enough that
        /// no Join waits for a roll-over: at the side vertices V = (+-50, 30, 12) the round's foot at
        /// V lies off the cylinder face only by the cylinder's sag over the foot's drop below the
        /// seam, (0.02 * 0.8 / sqrt(1.64))^2 / (2 * 10) = 7.8e-6, within the solvers' 3D tolerance
        /// (4e-5), so the Join keeps its ends where the rails cross the seam, as before
        /// (`join::setback_seam_sides` decides nothing, the Join is not deferred). The rails' seam
        /// crossings then meet within the tolerance: the solid is closed with one chamfer of
        /// setbacks per edge, five (no rail runs off its face, so no roll-over piece).
        #[test]
        fn the_enclosure_front_chain_with_setbacks_below_the_seams_sag_is_joined_on_the_seams() {
            let d = [0.02, 0.02];
            let mut k = fixed_kernel();
            let mut b = enclosure_front(&mut k);
            let slant = face_holding(&k, &b, |bx| {
                bx.min().z.abs() < 1e-6
                    && (bx.max().z - 20.0).abs() < 1e-6
                    && (bx.min().y - 15.0).abs() < 1e-6
                    && (bx.max().y - 40.0).abs() < 1e-6
            })[0];
            let front_face = face_holding(&k, &b, |bx| {
                bx.min().z.abs() < 1e-6 && bx.max().z.abs() < 1e-6
            });
            assert_eq!(front_face.len(), 1);
            let chain: Vec<EdgeId> = crate::topology::face::p4::edges(&b, slant, None, false, None)
                .unwrap()
                .into_iter()
                .filter(|&e| !edge::faces(&b, e, None).unwrap().contains(&front_face[0]))
                .collect();
            assert_eq!(chain.len(), 5);
            chamfer_setbacks(&mut k, &mut b, &chain, &[d; 5], &[slant; 5]).unwrap();
            assert!(closed(&b));
            assert_eq!(setback_faces(&k, &b).len(), 5);
        }
    }
}
