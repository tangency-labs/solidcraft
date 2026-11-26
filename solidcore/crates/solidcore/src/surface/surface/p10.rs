//! Line/surface leaf intersection and the combination of curve/surface intersection
//! solutions into unique points and coincident segments.
//!
//! Upstream: `source/SMLib/src/SmSurface.cpp`.
//!
//! | Rust                                             | Upstream                                    |
//! |--------------------------------------------------|---------------------------------------------|
//! | [`LsiGlobalSolver::local_solve_branch`]          | `SmLSIGlobalSolver::LocalSolve`             |
//! | [`CertifiedLsi`], [`CertifiedLsi::certified_pass`] | (not upstream)                            |
//! | [`global_curve_intersect_combine_solutions`], [`CombineContext`] | `sm_GlobalCurveIntersectCombineSolutions` (static) |
//!
//! Deviations:
//! - **Out-params.** `LocalSolve`'s `rbNeedsMoreSubdivision` is the return value
//!   (always false, as upstream). The combination edits the caller's
//!   [`SolutionArray`] in place, as upstream.
//! - **Patch data.** The leaf solve reads the node's `SmBezierPatch` through the
//!   solver's borrowed `surface_cache` (upstream casts `m_pData`); the node's tree must
//!   be that cache's tree. The surface evaluated (`sm_GetSurfaceToEvaluate`) and the
//!   parent surface (`sm_GetOriginalSurface`) are the cache's surface, named in the
//!   solutions by its handle. `GetPolarBox` builds a missing polar box without
//!   storing it ([`SurfaceCache::polar_box_unstored`]; the values are the same).
//!   `sm_IsPointValid` calls the virtual `PointTest` of the tree's cache: a face's
//!   trimmed cache answers through the solver's `face_cache`
//!   ([`crate::surface::surface::p8::FaceSolveCache::point_test`], a face
//!   classification), otherwise through [`super::p1::is_point_valid_with`].
//! - **Statuses passed on (WR-663).** Upstream ignores the statuses of
//!   `SmExtent3d::IntersectLine` and `SmPseudoBox::IntersectLine` in the leaf solve and
//!   of `sm_ClassifyCurveSurfaceXSectPoint`, `SmSurface::ClassifyIntersectingRangeSolution`
//!   and the `EvaluatePoint`s of the combination's point distance check; as the fork
//!   (tangency-labs/smlib 21dcf69) they are passed on, and a leaf whose domain or line
//!   interval cannot be trimmed to the one of interest has no answers.
//! - **Solution nodes.** The combination resolves `m_apNodes[0]` in
//!   [`CombineContext::curve_tree`] and `m_apNodes[1]` in the tree of
//!   [`CombineContext::surface_cache`]; a node id without its tree is an error (upstream
//!   holds pointers). The objects of the coincident range solutions it builds are
//!   [`CombineContext::curve_handle`] and [`CombineContext::surface_handle`] (upstream
//!   `&crCurve`, `&crSurface`).
//! - **Upstream quirks kept.** The combination classifies the first solution's point
//!   (not the current one) when two point solutions are too far apart; it tests the
//!   surface `u` of the current range start twice in the overlap test; the third
//!   interval span of its point/range culling is never filled (the `v` values go into
//!   the second span), so a point with valid nodes never culls; it resets a cull flag
//!   when the outer loop reaches that solution; the obsolete `while (!bDone2)` loop
//!   runs once (nothing clears the flag) and is a single pass here.
//! - **Contact (not upstream).** Where the combination keeps one of two point
//!   solutions close together, the point kept takes the
//!   [`crate::geom::solution_array::Contact::merged`] of both.
//! - **Debug only code** (`SM_DEBUG_CODE` drawing and dumps) and the assertions are not
//!   ported.
//! - **Bounds.** The leaf solve takes at most six guesses (upstream's fixed arrays). No
//!   recursion.
//! - **The certified pass (not upstream).** `LocalSolve` solves its guesses and decides
//!   from the patch's normal map whether another crossing can exist; a second crossing of
//!   the same leaf is lost where that map allows none (it is no proof) or the near-tangent
//!   prediction falls outside the leaf (a ray entering and leaving a torus near its crown
//!   within one leaf was given its exit as the first hit). The guesses run as upstream's,
//!   and their answers keep their numbers. For a surface whose net is the surface
//!   ([`Surface::certified_net`]) the solve runs through [`CertifiedLsi`], which keeps each
//!   leaf whose guesses ran; after the traversal each is searched by
//!   [`super::certified_line::isolate`] ([`CertifiedLsi::certified_pass`]), and every
//!   certified crossing that no solution of the whole solve already stands for is added.
//!   Nothing is removed, and no solution a guess found is solved again. The traversal also
//!   keeps the domain of each leaf and subtree it culls by the line's distance or
//!   parameter, which [`super::certified_line::enclose_strip`] encloses, and the pass
//!   returns the solve's side record ([`crate::geom::solution_array::SideRecord`]) from the
//!   components of what the leaves' passes and the strips kept, after the solve's drop
//!   ([`super::certified_line::drop_fenced`], [`super::certified_line::components`]);
//!   nothing reads it yet. A solve whose context keeps its operation's searches
//!   ([`super::p1::SurfaceSolveContext::line_passes`]) takes the one made on the same
//!   inputs instead of searching again ([`super::certified_line::LinePasses`]); its
//!   solutions are read as before.

use core::cell::RefCell;
use std::sync::Arc;

use super::certified_line;
use super::p1::{is_node_inside, is_node_valid, is_surface_cache_owner, surface_to_evaluate};
use super::p2::{original_surface, LsiGlobalSolver, Surface, TsectCurveType};
use super::p4;
use super::p6::{CurveIntersection, RangeTangentPoint};
use super::p9::implicit_iso;
use crate::curve::curve::p3::local_curve_intersect;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::surface::{SolverPart, SurfaceFailure};
use crate::error::{ignored, Result, SolidError};
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::extent3d::LineHit as BoxHit;
use crate::geom::geom_utility::p2::ContinuityType;
use crate::geom::global_solver::{Branch, GlobalSolve, SolverOperation};
use crate::geom::pseudo_box::LineHit as PseudoHit;
use crate::geom::solution_array::{
    SideRecord, Solution, SolutionArray, SolutionEnd, SolutionObject, SolutionType, SortKey,
};
use crate::geom::tol::{are_same, scaled_zero2};
use crate::geom::tree::{AuxDataType, NodeId, Tree};
use crate::geom::vector2d::{Point2, Vec2};
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::math::{max2, min2, BIG_DOUBLE, EFF_ZERO};
use crate::nurbs::b_spline_surface::SurfParamType;
use crate::nurbs::knots::Side;
use crate::surface::surface_cache::p1::{SolveModes, SurfaceCache};
use crate::topology::ObjectHandle;

const OP_LSI: &str = "surface::surface::p10::LsiGlobalSolver::local_solve_branch";
const OP_COMBINE: &str = "surface::surface::p10::global_curve_intersect_combine_solutions";

/// `SM_BIG_ULONG`: no solution index yet.
const NO_INDEX: usize = usize::MAX;

/// Upstream's guess arrays (`SmPoint2d sUVGuesses[6]`).
const MAX_GUESSES: usize = 6;

// ---- calls into other units -------------------------------------------------------------------

/// `SmSurface::LocalCurveIntersect` (`bFoundIntersection`, `rUVFound`, `rdTFound`,
/// `rdDeviation`), the status checked by the caller (`SER`).
#[allow(clippy::too_many_arguments)] // upstream's arguments
fn surface_local_curve_intersect(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    tol: f64,
    uv_guess: Point2,
    t_guess: f64,
) -> Result<Option<CurveIntersection>> {
    surface.local_curve_intersect(k, domain, curve, interval, tol, uv_guess, t_guess)
}

/// `sm_FilterSolution`: whether to keep a curve/surface solution (`rbKeepSolution`).
fn filter_solution<O: SolutionObject>(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    conts: [[ContinuityType; 2]; 3],
    solution: &Solution<O>,
) -> Result<bool> {
    let [u_conts, v_conts, t_conts] = conts;
    crate::surface::surface::p5::filter_solution(
        k, surface, domain, curve, interval, u_conts, v_conts, t_conts, solution,
    )
}

/// The outputs of `sm_ClassifyCurveSurfaceXSectPoint`.
struct XsectPointClass {
    /// `bFoundGuess`, `rsNewUVGuess`, `rdNewTGuess`.
    guess: Option<(Point2, f64)>,
    /// `rePointType`.
    point_type: TsectCurveType,
}

/// `sm_ClassifyCurveSurfaceXSectPoint` (`surface::surface::p4`); `edge_tolerance` is
/// the tolerance of the curve's owning edge, when the caller resolves it.
#[allow(clippy::too_many_arguments)]
fn classify_curve_surface_xsect_point(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    uv: Point2,
    t: f64,
    edge_tolerance: Option<f64>,
) -> Result<XsectPointClass> {
    let c = p4::classify_curve_surface_xsect_point_with(
        k,
        surface,
        domain,
        curve,
        interval,
        uv,
        t,
        edge_tolerance,
    )?;
    Ok(XsectPointClass {
        guess: c.found_guess.then_some((c.new_uv_guess, c.new_t_guess)),
        point_type: c.point_type,
    })
}

/// `SmSurface::LocalPointSolve` (`rbFoundAnswer`, `rSolution`).
fn surface_local_point_solve<O: Copy>(
    k: &Kernel,
    surface: &Surface,
    domain: &Extent2d,
    operation: SolverOperation,
    point: Point3,
    uv_guess: Point2,
) -> Result<(bool, Solution<O>)> {
    let r = surface.local_point_solve::<O>(k, domain, operation, point, uv_guess)?;
    Ok((r.found, r.solution))
}

/// The outputs of `SmSurface::DropPoint` used here: `rbSuccess` and `rdGap`.
struct DroppedPoint {
    success: bool,
    gap: f64,
}

/// `SmSurface::DropPoint` with a guess.
fn surface_drop_point(
    k: &Kernel,
    surface: &Surface,
    point: Point3,
    domain: &Extent2d,
    guess: Point2,
) -> Result<DroppedPoint> {
    let opts = crate::surface::surface::p5::DropPointOptions {
        guess_uv: Some(guess),
        ..Default::default()
    };
    let d = surface.drop_point::<p4::NoObject>(k, point, domain, &opts)?;
    Ok(DroppedPoint {
        success: d.success,
        gap: d.gap,
    })
}

/// `SmSurface::ClassifyIntersectingRangeSolution`: `bIsDegenerate` and, when it is,
/// the tangent point solution written to `rSolPoint`.
fn classify_intersecting_range_solution<O: SolutionObject>(
    k: &Kernel,
    surface: &Surface,
    curve: &Curve,
    solution: &Solution<O>,
) -> Result<(bool, Option<RangeTangentPoint>)> {
    surface.classify_intersecting_range_solution(k, curve, solution)
}

/// The outputs of `SmSurface::DropAndTrimCurve` used here.
struct DropAndTrim {
    /// `rdDistToSurface`.
    dist_to_surface: f64,
    /// `rUVCurves`.
    uv_curves: Vec<Curve>,
}

/// `SmSurface::DropAndTrimCurve(context, domain, curve, interval, tol, ..., TRUE,
/// TRUE)` (upstream requires an `SmBSplineCurve`).
fn drop_and_trim_curve<O: SolutionObject>(
    k: &Kernel,
    surface: &Surface,
    cache: Option<&SurfaceCache<O>>,
    domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    tol: f64,
) -> Result<DropAndTrim> {
    // the trace gets the surface's cache (`smsurf_GetSurfaceCache`): for a face's
    // surface, the face's cache the solve ran on
    let mut held = match cache {
        Some(c) if surface.face()?.is_some() => Some(c.map_handles(Ok)?),
        _ => None,
    };
    let r = surface.drop_and_trim_curve_in::<ObjectHandle, O>(
        k,
        held.as_mut(),
        domain,
        curve,
        interval,
        tol,
        true,
        true,
        None,
    )?;
    Ok(DropAndTrim {
        dist_to_surface: r.max_drop_to_surf,
        uv_curves: r.uv_curves,
    })
}

// ---- helpers -----------------------------------------------------------------------------------

/// `SM_IS_BETWEEN(c, a, b)`: `c` strictly inside the open interval between `a` and
/// `b` shrunk by their scaled zero.
fn is_between(c: f64, a: f64, b: f64) -> bool {
    let z = scaled_zero2(&a, &b);
    ((a + z) < c && c < (b - z)) || ((b + z) < c && c < (a - z))
}

/// `continuity <= SM_CT_C0`.
fn at_most_c0(c: ContinuityType) -> bool {
    (c as i32) <= ContinuityType::C0 as i32
}

/// Parameter `i` of a solution end (`m_adParameters[i]`).
fn p(end: &SolutionEnd, i: usize) -> Result<f64> {
    end.param(i)
}

/// Sets parameter `i` of a solution end.
fn set_p(end: &mut SolutionEnd, i: usize, v: f64) -> Result<()> {
    *end.param_mut(i)? = v;
    Ok(())
}

/// The count and parameters of an `IntersectLine` whose status upstream ignores (an
/// invalid input leaves the count at zero).
fn box_hit(hit: &Result<BoxHit>) -> Option<(f64, f64)> {
    match *hit {
        Ok(BoxHit::Touch { t }) => Some((t, t)),
        Ok(BoxHit::Through { enter, exit }) => Some((enter, exit)),
        Ok(BoxHit::Miss) | Err(_) => None,
    }
}

/// [`box_hit`] for a pseudo box.
fn pseudo_hit(hit: &Result<PseudoHit>) -> Option<(u32, f64, f64)> {
    match *hit {
        Ok(PseudoHit::Graze(t)) => Some((1, t, t)),
        Ok(PseudoHit::Segment(enter, exit)) => Some((2, enter, exit)),
        Ok(PseudoHit::Miss) | Err(_) => None,
    }
}

// ---- SmLSIGlobalSolver::LocalSolve -------------------------------------------------------------

/// A leaf whose guesses ran, kept for [`CertifiedLsi::certified_pass`]: what
/// [`LsiGlobalSolver::local_solve_branch`] had made of the leaf when it started its
/// guesses (not upstream).
#[derive(Debug, Clone, Copy, PartialEq)]
struct CertifiedLeaf<O> {
    owner: Option<crate::geom::tree::TreeOwner>,
    parent: O,
    domain: Extent2d,
    interval: Extent1d,
    conts: [[ContinuityType; 2]; 3],
    nodes: [Option<NodeId>; 2],
    /// Whether the leaf lies wholly inside the face
    /// ([`super::p1::is_node_inside`]).
    inside: bool,
}

/// What the traversal of a solve with the certified pass keeps (not upstream): the leaves
/// whose guesses ran, and the strips of the leaves and subtrees it culled by the line's
/// distance or parameter (a slab or sphere test of boxes, no proof), each the culled
/// domain met with the solve's, for [`certified_line::enclose_strip`]. A leaf outside the
/// face or the solve's domain is no cull here: no hit there counts.
#[derive(Debug, Clone, PartialEq)]
struct Kept<O> {
    leaves: Vec<CertifiedLeaf<O>>,
    /// The culled strips.
    strips: Vec<Extent2d>,
    /// Culls whose domain could not be placed (a subtree's node with no domain in the
    /// cache): nothing encloses them, and the whole line is undecided.
    unplaced: usize,
}

impl<O> Default for Kept<O> {
    fn default() -> Self {
        Self {
            leaves: Vec::new(),
            strips: Vec::new(),
            unplaced: 0,
        }
    }
}

/// Keeps the culled leaf `domain` (already met with the solve's) as a strip of `keep`
/// (when kept).
fn keep_strip<O>(keep: &mut Option<&mut Kept<O>>, domain: Extent2d) {
    if let Some(kept) = keep.as_deref_mut() {
        kept.strips.push(domain);
    }
}

/// Where a culled subtree's strip lies (not upstream).
enum CullStrip {
    /// Its node's domain met with the solve's.
    At(Extent2d),
    /// Outside the solve's domain, or a sliver of it, as a leaf there is: no cull.
    Outside,
    /// Its node has no domain in the cache.
    Unplaced,
}

/// Read-only state of one leaf solve.
struct LsiLeaf<'s, O> {
    cache: &'s SurfaceCache<O>,
    owner: Option<crate::geom::tree::TreeOwner>,
    parent: O,
    eval: &'s Surface,
    domain: Extent2d,
    line: Curve,
    interval: Extent1d,
    conts: [[ContinuityType; 2]; 3],
    nodes: [Option<NodeId>; 2],
}

impl<O: SolutionObject> LsiLeaf<'_, O> {
    /// A line/surface solution `[t, u, v]` of this leaf (upstream's field-by-field
    /// set up of `sSolution`).
    fn solution(&self, t: f64, uv: Point2, deviation: f64) -> Result<Solution<O>> {
        let mut solution = Solution::new();
        self.adopt(&mut solution);
        solution.num_variables = 3;
        set_p(&mut solution.start, 0, t)?;
        set_p(&mut solution.start, 1, uv.x)?;
        set_p(&mut solution.start, 2, uv.y)?;
        solution.start.solution_value = deviation;
        solution.solution_type = SolutionType::SingleValue;
        Ok(solution)
    }

    /// Names this leaf's objects (`NULL`, the parent surface) and nodes in `solution`.
    fn adopt(&self, solution: &mut Solution<O>) {
        solution.num_objects = 2;
        solution.objects[0] = None;
        solution.objects[1] = Some(self.parent);
        solution.nodes[0] = self.nodes[0];
        solution.nodes[1] = self.nodes[1];
    }
}

impl<'t, 's, O: SolutionObject + 't> LsiGlobalSolver<'t, 's, O> {
    /// `sm_IsPointValid` of a leaf of `cache` (tree owner kind `owner`): the face's
    /// trimmed cache answers with its point test (a face classification) when the
    /// solve runs on one, else the prepared query service. A failed native classification keeps the
    /// point (`SE(SM_ERR)`, return TRUE); [`SolidError::Unported`] propagates.
    fn is_point_valid(
        &mut self,
        k: &Kernel,
        owner: Option<crate::geom::tree::TreeOwner>,
        cache: &SurfaceCache<O>,
        uv: Point2,
    ) -> Result<bool> {
        match self.face_cache.as_deref_mut() {
            // keep all points when SurfaceNode has no trim boundaries
            Some(face_cache) if is_surface_cache_owner(owner) => Ok(
                super::p1::ignore_native_classification(face_cache.point_test(k, uv))?
                    .unwrap_or(true),
            ),
            _ => super::p1::is_point_valid_with(
                k,
                owner,
                cache,
                SolveModes::of(cache, self.modes),
                uv,
                None,
                self.face_queries,
            ),
        }
    }

    /// Filters a single valued solution of `surface` over `domain` and adds it when
    /// kept and inside the trim boundary (upstream's repeated `sm_FilterSolution` /
    /// `sm_IsPointValid` / `AddSortedSolution` block).
    fn keep_point(
        &mut self,
        k: &Kernel,
        leaf: &LsiLeaf<'s, O>,
        surface: &Surface,
        domain: &Extent2d,
        solution: &Solution<O>,
    ) -> Result<bool> {
        let keep = filter_solution(
            k,
            surface,
            domain,
            &leaf.line,
            &leaf.interval,
            leaf.conts,
            solution,
        )?;
        if keep {
            let uv = Point2::new(p(&solution.start, 1)?, p(&solution.start, 2)?);
            // for points within the surface's trim boundary
            if self.is_point_valid(k, leaf.owner, leaf.cache, uv)? {
                self.add_sorted_solution(solution, SortKey::ByAllParameters, true)?;
            }
        }
        Ok(keep)
    }

    /// `SmLSIGlobalSolver::LocalSolve`: intersects the line with the Bezier patch of
    /// the surface leaf node `branch[0]` (guess points refined by the surface's local
    /// curve intersection, then the line ends and the patch boundaries at
    /// discontinuities), adding the solutions `[line t, surface u, surface v]` kept to
    /// `base.solutions`. Returns the request for more subdivision (always false).
    ///
    /// # Errors
    ///
    /// `SM_ERR_NULL_POINTER` without `surface_cache` (or `kernel` for the boundary
    /// coincidence checks); `InvalidInput` when the node is not a patch node of that
    /// cache or the cache does not hold the tree's surface; those of the evaluations
    /// and solves; [`SolidError::Unported`] from the calls into units not ported yet.
    pub fn local_solve_branch(&mut self, k: &Kernel, branch: &Branch) -> Result<bool> {
        self.local_solve_branch_keeping(k, branch, None)
    }

    /// [`LsiGlobalSolver::local_solve_branch`], the leaf pushed to `keep` (when given)
    /// as its guesses start: the leaves [`CertifiedLsi::certified_pass`] searches; and the
    /// domain of a leaf culled by the line's distance or parameter kept there as a strip.
    #[allow(clippy::too_many_lines)]
    fn local_solve_branch_keeping(
        &mut self,
        k: &Kernel,
        branch: &Branch,
        mut keep: Option<&mut Kept<O>>,
    ) -> Result<bool> {
        // locals - apBranch[0] is SurfaceNode
        let (surface_id, surface_node) = self.base.branch_entry(branch, 0)?;
        let surface_node = *surface_node;
        let cache = self.surface_cache.ok_or_else(|| {
            SolidError::surface(
                OP_LSI,
                SurfaceFailure::SolverWithout {
                    part: SolverPart::SurfaceCache,
                },
            )
        })?;
        let patch = cache.patch(surface_id)?.clone();
        let tree = self.base.tree(0)?;
        let parent = original_surface(tree, &surface_node)?;
        if Some(parent) != cache.surface_handle() {
            return Err(SolidError::invalid_input(
                OP_LSI,
                "surface cache does not own the tree's surface",
            ));
        }
        let mut uv_domain = patch.aux.uv_domain;

        // Use the original surface (sm_GetSurfaceToEvaluate)
        let eval_handle = surface_to_evaluate(tree, &surface_node)?;
        if Some(eval_handle) != cache.surface_handle() {
            return Err(SolidError::invalid_input(
                OP_LSI,
                "surface cache does not own the surface to evaluate",
            ));
        }
        let srf_eval = cache.surface();

        // no work - This SurfaceNode is classified outside its face.
        if !is_node_valid(
            tree.owner(),
            cache,
            SolveModes::of(cache, self.modes),
            &surface_node,
        )? {
            return Ok(false);
        }
        // (not upstream) whether a component in it can straddle the face's border
        let inside = keep.is_some()
            && is_node_inside(
                tree.owner(),
                cache,
                SolveModes::of(cache, self.modes),
                &surface_node,
            )?;

        // no work - surfaceLeafDomain disjoint from intersectionDomain
        if uv_domain.are_disjoint(&self.uv_domain, 0.0) {
            return Ok(false);
        }

        // Set discontinuities: Mark Leaf boundaries discontinuous for line and surface
        let t_conts = [ContinuityType::Discontinuous; 2];
        let mut u_conts = patch.u_curve_conts;
        let mut v_conts = patch.v_curve_conts;

        // mark intersection boundary limits as discontinuous
        if self.uv_domain.min().x >= uv_domain.min().x {
            u_conts[0] = ContinuityType::Discontinuous;
        }
        if self.uv_domain.max().x <= uv_domain.max().x {
            u_conts[1] = ContinuityType::Discontinuous;
        }
        if self.uv_domain.min().y >= uv_domain.min().y {
            v_conts[0] = ContinuityType::Discontinuous;
        }
        if self.uv_domain.max().y <= uv_domain.max().y {
            v_conts[1] = ContinuityType::Discontinuous;
        }

        // as the fork, a branch outside the domain or interval of interest has no
        // answers and the box intersections pass on their statuses (WR-663,
        // tangency-labs/smlib 21dcf69)

        // when SurfaceLeafDomain is partially intersected by the IntersectionDomain
        if !uv_domain.is_contained_by(&self.uv_domain, EFF_ZERO) {
            // trim leafNode domain down to intersection domain
            uv_domain = match uv_domain.intersect(&self.uv_domain) {
                Ok(d) => d,
                Err(e) if !e.is_unported_or_cancelled() => return Ok(false),
                Err(e) => return Err(e),
            };

            // no work - remaining domain has a zero length dimension
            if are_same(uv_domain.max().x, uv_domain.min().x) {
                return Ok(false);
            }
            if are_same(uv_domain.max().y, uv_domain.min().y) {
                return Ok(false);
            }
        }

        // cull obviously nonIntersecting cases with BBox and pseudo box line intersections
        let mut pseudo_box = patch.pseudo_box;
        pseudo_box.expand_absolute(self.distance_tolerance);

        // BBox/line intersect
        let mut bbox = surface_node.bbox;
        bbox.expand_absolute(self.base.tolerance_3d);
        let Some((bbox_enter, bbox_exit)) =
            box_hit(&Ok(bbox.intersect_line(self.line_point, self.line_vector)?))
        else {
            // no work - no BBox intersection
            keep_strip(&mut keep, uv_domain);
            return Ok(false);
        };

        // PseudoBox/line intersect
        let Some((_, pseudo_enter, pseudo_exit)) = pseudo_hit(&Ok(
            pseudo_box.intersect_line(self.line_point, self.line_vector)?
        )) else {
            // no work - no Pseudo intersection
            keep_strip(&mut keep, uv_domain);
            return Ok(false);
        };

        // no work - BBox/Pseudo intersection segments don't overlap
        if max2(bbox_enter, pseudo_enter) > min2(bbox_exit, pseudo_exit) {
            keep_strip(&mut keep, uv_domain);
            return Ok(false);
        }

        // target line interval that must contain any intersection
        let mut bound_ivl =
            Extent1d::from_min_max(max2(bbox_enter, pseudo_enter), min2(bbox_exit, pseudo_exit));

        // expand by more than tolerance [B242]
        bound_ivl.expand_absolute(2.0 * self.base.tolerance_3d);

        // trim sBoundIvl to m_cpLineInterval (the interval of interest)
        if let Some(line_ivl) = &self.line_interval {
            bound_ivl = match bound_ivl.intersect(line_ivl) {
                Ok(i) => i,
                Err(e) if !e.is_unported_or_cancelled() => {
                    keep_strip(&mut keep, uv_domain);
                    return Ok(false);
                }
                Err(_) => Extent1d::new(),
            };
        }

        // when firing rays only look at positive line parameters
        if self.ray_fire && bound_ivl.max() <= 0.0 {
            // no work - ray casting and no positive intersection segment
            keep_strip(&mut keep, uv_domain);
            return Ok(false);
        }

        // Not upstream. A ray has no roots behind its start: only the forward part of the
        // interval is a candidate. Left whole, a leaf that a ray grazes (it crosses the
        // convex patch twice) seeded its solve from the middle of the interval, which can
        // settle on the root behind the start; that root is dropped below and the root
        // ahead was never looked for, so the ray missed a wall it leaves by.
        let mut clipped = false;
        if self.ray_fire {
            // (a band behind the start as wide as the one the interval is expanded by, so a
            // root on the surface the ray starts on is found where it was, and dropped below
            // where it was, rather than clamped to the end of the interval)
            let behind =
                EFF_ZERO * (1.0 + self.line_point.max_dimension()) + 2.0 * self.base.tolerance_3d;
            // A ray that starts on the surface has its root at the start, which the
            // classification of a point on a face leaves to the leaf solve as it was: the
            // interval is kept whole there.
            let on_surface = bound_ivl.min() < -behind && {
                let natural = srf_eval.natural_uv_domain_with_nurb(k)?;
                ignored(surface_local_point_solve::<O>(
                    k,
                    srf_eval,
                    &natural,
                    SolverOperation::Minimize,
                    self.line_point,
                    uv_domain.evaluate(0.5, 0.5),
                ))?
                .is_some_and(|(found, near)| {
                    found && near.start.solution_value < self.base.tolerance_3d
                })
            };
            if bound_ivl.min() < -behind && !on_surface {
                bound_ivl = Extent1d::from_min_max(-behind, bound_ivl.max());
                clipped = true;
            }
        }

        // no work - if given, m_cpLineInterval doesn't overlap sBoxIvl
        if let Some(line_ivl) = &self.line_interval {
            if line_ivl.are_disjoint(&bound_ivl, 0.0) {
                keep_strip(&mut keep, uv_domain);
                return Ok(false);
            }
        }

        // init a line interval - for a ray or for a given line interval
        let ivl = match &self.line_interval {
            Some(line_ivl) => line_ivl.intersect(&bound_ivl)?,
            None => bound_ivl,
        };

        // let guess = LineInterval/SurfaceDomain CenterPoints
        let mut first_t = ivl.evaluate(0.5);
        let first_uv_guess = uv_domain.evaluate(0.5, 0.5);
        let mut uv_guesses = Vec::new();
        let mut t_guesses = Vec::new();

        // Let SurfaceGuessPoint = Drop LinePoint down to SurfacePoint Tangent Plane
        let drop = |uv: Point2, t: f64| -> Result<Point2> {
            let point = self.line_point + t * self.line_vector;
            super::p7::drop_points_to_tangent_plane(
                k,
                srf_eval,
                uv,
                Side::Left,
                Side::Left,
                &[point],
            )?
            .first()
            .copied()
            .ok_or_else(|| {
                SolidError::surface(
                    OP_LSI,
                    SurfaceFailure::Defect {
                        why: "the tangent-plane drop returned no point, though it maps each \
                              point it is given and was given one",
                    },
                )
            })
        };
        let mut first_uv = drop(first_uv_guess, first_t)?;

        // When the updated SurfaceGuessPoint is outside the leafNodeIntersectionDomain
        let uv_eps = EFF_ZERO * (1.0 + uv_domain.size().max_dimension());
        if !uv_domain.contains_point(first_uv, uv_eps) {
            // move guess to SurfaceMidPoint and near LineInterval beginPoint.
            first_t = ivl.evaluate(0.1);
            first_uv = drop(uv_domain.evaluate(0.5, 0.5), first_t)?;

            // When SurfaceMid/LineStart Drop is also outside the leafNodeIntersectionDomain
            if !uv_domain.contains_point(first_uv, 0.0) {
                // move guess to SurfaceMidPoint and near LineInterval endPoint.
                first_t = ivl.evaluate(0.9);
                first_uv = drop(uv_domain.evaluate(0.5, 0.5), first_t)?;

                // when this 3rd point is also outside the leafNodeIntersectionDomain
                if !uv_domain.contains_point(first_uv, 0.0) {
                    // clamp the guess to the leafNodeIntersectionDomain
                    first_uv = uv_domain.clamp_point(first_uv);

                    // add in 3 more guesses: SurfLeafCenterPoint/(Interval Start, Mid, End)
                    for fraction in [0.5, 0.1, 0.9] {
                        uv_guesses.push(uv_domain.evaluate(0.5, 0.5));
                        t_guesses.push(ivl.evaluate(fraction));
                    }
                }
            }
        }
        uv_guesses.insert(0, first_uv);
        t_guesses.insert(0, first_t);

        // Make the line object (sTmpIvl = the bounded interval of interest).
        let line = Curve::from(Line::infinite(self.line_point, self.line_vector, 3, false));
        let leaf = LsiLeaf {
            cache,
            owner: tree.owner(),
            parent,
            eval: srf_eval,
            domain: uv_domain,
            line,
            interval: ivl,
            conts: [u_conts, v_conts, t_conts],
            nodes: [
                branch.first().copied().flatten(),
                branch.get(1).copied().flatten(),
            ],
        };

        // (not upstream) the certified pass searches this leaf after the traversal
        if let Some(keep) = keep {
            keep.leaves.push(CertifiedLeaf {
                owner: leaf.owner,
                parent: leaf.parent,
                domain: leaf.domain,
                interval: leaf.interval,
                conts: leaf.conts,
                nodes: leaf.nodes,
                inside,
            });
        }

        // for every Guess (more than 1 when LineGuess does not drop to SurfaceDomain)
        let mut i = 0;
        while let (Some(&uv_guess), Some(&t_guess)) = (uv_guesses.get(i), t_guesses.get(i)) {
            let index = i;
            i += 1;

            // pass the call along to local curve intersection
            let Some(hit) = surface_local_curve_intersect(
                k,
                leaf.eval,
                &leaf.domain,
                &leaf.line,
                &leaf.interval,
                self.base.tolerance_3d,
                uv_guess,
                t_guess,
            )?
            else {
                continue;
            };

            // process intersections
            let solution = leaf.solution(hit.t, hit.uv, hit.deviation)?;

            // skip negative parameter ray/surface intersections
            if self.ray_fire {
                let scaled_zero = EFF_ZERO * (1.0 + self.line_point.max_dimension());
                if hit.t < -scaled_zero {
                    continue;
                }
            }

            // keep exact solutions - check near solutions
            if !filter_solution(
                k,
                leaf.eval,
                &leaf.domain,
                &leaf.line,
                &leaf.interval,
                leaf.conts,
                &solution,
            )? {
                continue;
            }

            // for points within the surface's trim boundary
            if self.is_point_valid(k, leaf.owner, cache, hit.uv)? {
                self.add_sorted_solution(&solution, SortKey::ByAllParameters, true)?;
            }

            // Try to eliminate additional intersections with the patch's normal map.
            let line_vec = self.line_vector.unitize()?;
            if !cache
                .polar_box_unstored(k, surface_id)?
                .has_perpendicular_to_vector(line_vec)
            {
                return Ok(false);
            }

            // look for another intersection very close to this one (near tangent)
            if index < 1 {
                let class = classify_curve_surface_xsect_point(
                    k,
                    leaf.eval,
                    &leaf.domain,
                    &leaf.line,
                    &leaf.interval,
                    hit.uv,
                    hit.t,
                    None,
                );
                // status ignored upstream (SE)
                if let Some(XsectPointClass {
                    guess: Some((new_uv, new_t)),
                    ..
                }) = ignored(class)?
                {
                    if uv_guesses.len() >= MAX_GUESSES {
                        return Err(SolidError::invalid_input(
                            OP_LSI,
                            "intersection guess limit exceeded",
                        ));
                    }
                    uv_guesses.push(new_uv);
                    t_guesses.push(new_t);
                }
            }
        }

        // the extra work required at discontinuities of the line ends and patch boundaries
        let natural = cache.surface().natural_uv_domain_with_nurb(k)?;
        for j in 0..2 {
            // Handle curve discontinuities
            // (the clip's own end is no end of the line: a ray's root at its start is the
            // leaf solve's, as before the clip)
            if t_conts.get(j).copied().is_some_and(at_most_c0) && !(j == 0 && clipped) {
                let t = if j == 1 {
                    leaf.interval.max()
                } else {
                    leaf.interval.min()
                };
                let pnt = leaf.line.evaluate_point(k, t)?;
                if pseudo_box.distance_to_point(pnt) < EFF_ZERO {
                    // Get the big surface from the tree and cache
                    let surface = cache.surface();
                    let (found, mut solution) = surface_local_point_solve::<O>(
                        self.kernel,
                        surface,
                        &natural,
                        SolverOperation::Minimize,
                        pnt,
                        leaf.domain.evaluate(0.5, 0.5),
                    )?;
                    if found && solution.start.solution_value < self.base.tolerance_3d {
                        // turn Surf/Point solution into a line:[pt,vec]/Surf solution
                        leaf.adopt(&mut solution);
                        solution.num_variables = 3;
                        let (u, v) = (p(&solution.start, 0)?, p(&solution.start, 1)?);
                        set_p(&mut solution.start, 2, v)?;
                        set_p(&mut solution.start, 1, u)?;
                        set_p(&mut solution.start, 0, t)?;
                        solution.solution_type = SolutionType::SingleValue;
                        self.keep_point(k, &leaf, surface, &natural, &solution)?;
                    }
                }
            }

            // Handle surface boundary discontinuities
            if !SolveModes::of(cache, self.modes).process_boundary_curves {
                continue;
            }
            for dir in [SurfParamType::U, SurfParamType::V] {
                if self.boundary_solve(k, &leaf, &patch.pseudo_box, dir, j)? {
                    return Ok(false);
                }
            }
        }

        // all done
        Ok(false)
    }

    /// Where the strip of the subtree `branch` lies, pruned by its sphere bound (not
    /// upstream): its surface node's domain trimmed to the solve's as a leaf's is (kept whole
    /// within a rounding of it), outside it where a leaf there would have no work.
    fn branch_strip(&self, branch: &Branch) -> CullStrip {
        let (Some(cache), Some(id)) = (self.surface_cache, branch.first().copied().flatten())
        else {
            return CullStrip::Unplaced;
        };
        let Ok(aux) = cache.aux(id) else {
            return CullStrip::Unplaced;
        };
        let domain = aux.uv_domain;
        if domain.are_disjoint(&self.uv_domain, 0.0) {
            return CullStrip::Outside;
        }
        if domain.is_contained_by(&self.uv_domain, EFF_ZERO) {
            return CullStrip::At(domain);
        }
        match domain.intersect(&self.uv_domain) {
            Ok(d) if !are_same(d.max().x, d.min().x) && !are_same(d.max().y, d.min().y) => {
                CullStrip::At(d)
            }
            _ => CullStrip::Outside,
        }
    }

    /// Whether a solution of the solve so far stands for the certified `root`, whose
    /// line parameter is `t`: a point solution within `reach` (the tolerance) of it along
    /// the line, or in the `(u, v)` box the root is the only one in (a regular box holds
    /// one contact), or a range that holds it. The certified pass adds no second solution
    /// for one contact.
    fn stands_for(&self, root: &certified_line::CertifiedRoot, t: f64, reach: f64) -> Result<bool> {
        let [unique_u, unique_v] = root.unique;
        for solution in self.base.solutions.as_slice() {
            let start = p(&solution.start, 0)?;
            let held = if solution.solution_type == SolutionType::RangeOfValues {
                let end = p(&solution.end, 0)?;
                t >= min2(start, end) - reach && t <= max2(start, end) + reach
            } else {
                let (u, v) = (p(&solution.start, 1)?, p(&solution.start, 2)?);
                (t - start).abs() <= reach
                    || (u >= unique_u.lo
                        && u <= unique_u.hi
                        && v >= unique_v.lo
                        && v <= unique_v.hi)
            };
            if held {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// The crossings [`certified_line::isolate`] certifies on each leaf `kept` holds, added
    /// ([`CertifiedLsi::certified_pass`]), and the solve's side record built from the
    /// components of what the passes kept ([`certified_line::components`]). The search
    /// ([`certified_line::search`]) is taken from `passes` when the operation kept one on
    /// the same inputs, and kept there otherwise.
    fn add_certified_roots(
        &mut self,
        k: &Kernel,
        net: &certified_line::LineNet,
        kept: &Kept<O>,
        passes: Option<&RefCell<certified_line::LinePasses>>,
    ) -> Result<SideRecord> {
        // (without the cache no leaf was solved, and none is kept)
        let Some(cache) = self.surface_cache else {
            return Ok(SideRecord::NotRun);
        };
        let tol = self.base.tolerance_3d;
        let speed = self.line_vector.length();
        // (a line of no length has no parameter to tell two hits apart by)
        let moving = speed > 0.0;
        if !moving {
            return Ok(SideRecord::NotRun);
        }
        // the line's own interval, as `add_certified_root` bounds it: the solve's line
        // interval, and a ray's hits from just behind its start
        let behind = if self.ray_fire {
            -EFF_ZERO * (1.0 + self.line_point.max_dimension())
        } else {
            f64::NEG_INFINITY
        };
        let span = match &self.line_interval {
            Some(ivl) => [max2(ivl.min(), behind), ivl.max()],
            None => [behind, f64::INFINITY],
        };
        let line = certified_line::PassLine {
            point: self.line_point,
            vector: self.line_vector,
            span,
            solve_domain: self.uv_domain,
            tol,
        };
        // the search reads the net, the line, the leaves and the strips alone: one the
        // operation made on the same inputs is this one's (a borrow refused is no search
        // kept: this solve searches)
        let jobs: Vec<certified_line::SolvedLeaf> = kept
            .leaves
            .iter()
            .map(|job| certified_line::SolvedLeaf {
                domain: job.domain,
                interval: job.interval,
                inside: job.inside,
            })
            .collect();
        let key = passes.map(|_| certified_line::PassKey::of(net, &line, &jobs, &kept.strips));
        let found = match (passes, &key) {
            (Some(passes), Some(key)) => passes.try_borrow().ok().and_then(|p| p.found(key)),
            _ => None,
        };
        let search = if let Some(search) = found {
            k.work_counters.add_line_search_reused();
            search
        } else {
            let search = Arc::new(certified_line::search(
                net,
                &line,
                &jobs,
                &kept.strips,
                self.base.cancel.as_ref(),
                &k.work_counters,
            )?);
            if let (Some(passes), Some(key)) = (passes, key) {
                if let Ok(mut passes) = passes.try_borrow_mut() {
                    passes.keep(key, &search);
                }
            }
            search
        };
        // Only a leaf the pass finished adds crossings: one stopped at the limit may hold
        // a crossing the pass never reached, and adding one of two crossings it did reach
        // would turn the guesses' even count (a ray passing near a crown seam, both
        // crossings missed) into an odd one. Its answer stays the guesses'. (The solved
        // leaves come first in the search, in the order of `kept`.)
        for (job, found) in kept.leaves.iter().zip(&search.leaves) {
            if found.roots.is_empty() || found.stop != certified_line::Stop::Finished {
                continue;
            }
            let leaf = LsiLeaf {
                cache,
                owner: job.owner,
                parent: job.parent,
                eval: cache.surface(),
                domain: job.domain,
                line: Curve::from(Line::infinite(self.line_point, self.line_vector, 3, false)),
                interval: job.interval,
                conts: job.conts,
                nodes: job.nodes,
            };
            for root in &found.roots {
                // a root whose evaluation, filter or point test fails is not added: the
                // solve's answer without it is the guesses', as it was
                ignored(self.add_certified_root(k, &leaf, root, tol / speed))?;
            }
        }
        // (a record not run leaves the solutions the guesses' alone, unclassified)
        if certified_line::leaves_not_run(&search.leaves) {
            return Ok(SideRecord::NotRun);
        }
        let stood_for = self.type_solutions(net, &line, &search.components, kept.unplaced > 0)?;
        Ok(certified_line::side_record(
            &search.leaves,
            &search.components,
            &stood_for,
            kept.unplaced > 0,
            span,
        ))
    }

    /// Types every point and range solution of the solve from the typed `components`
    /// ([`certified_line::type_solutions`]; `unplaced`, a cull whose strip could not be
    /// placed). Returns, for each component, whether the solutions stand for it.
    fn type_solutions(
        &mut self,
        net: &certified_line::LineNet,
        line: &certified_line::PassLine,
        components: &certified_line::Components,
        unplaced: bool,
    ) -> Result<Vec<bool>> {
        use certified_line::SolutionAt;
        let mut at: Vec<SolutionAt> = Vec::new();
        for sol in self.base.solutions.as_slice() {
            at.push(match sol.solution_type {
                SolutionType::SingleValue => SolutionAt::Point([
                    sol.start.param(0)?,
                    sol.start.param(1)?,
                    sol.start.param(2)?,
                ]),
                SolutionType::RangeOfValues => SolutionAt::Range,
                _ => SolutionAt::Other,
            });
        }
        let (typed, stood) = certified_line::type_solutions(net, line, components, &at, unplaced);
        for (i, t) in typed.into_iter().enumerate() {
            let (Some((contact, unique, shared)), Some(sol)) = (t, self.base.solutions.get_mut(i))
            else {
                continue;
            };
            sol.contact = contact;
            sol.unique = unique.map(|[u, v]| [[u.lo, u.hi], [v.lo, v.hi]]);
            sol.shared_contact = shared;
        }
        Ok(stood)
    }

    /// One certified root of `leaf`, added as a guess's hit is, unless a solution of the
    /// solve already stands for it; `reach` is the tolerance as a stretch of the line's
    /// parameter.
    fn add_certified_root(
        &mut self,
        k: &Kernel,
        leaf: &LsiLeaf<'s, O>,
        root: &certified_line::CertifiedRoot,
        reach: f64,
    ) -> Result<()> {
        let [u, v, t] = root.point;
        // a contact is one solution: the guesses' where they found it
        if self.stands_for(root, t, reach)? {
            return Ok(());
        }
        // a root certified on the leaf's border may lie a rounding outside it; one
        // farther out is the next leaf's
        let uv = Point2::new(u, v);
        let uv_eps = EFF_ZERO * (1.0 + leaf.domain.size().max_dimension());
        if !leaf.domain.contains_point(uv, uv_eps) {
            return Ok(());
        }
        let uv = leaf.domain.clamp_point(uv);
        let on_line = self.line_point + t * self.line_vector;
        let deviation = leaf.eval.evaluate_point(k, uv)?.distance(on_line);
        let within_tolerance = deviation < self.base.tolerance_3d;
        if !within_tolerance {
            return Ok(());
        }
        let solution = leaf.solution(t, uv, deviation)?;

        // skip negative parameter ray/surface intersections
        if self.ray_fire {
            let scaled_zero = EFF_ZERO * (1.0 + self.line_point.max_dimension());
            if t < -scaled_zero {
                return Ok(());
            }
        }
        if !filter_solution(
            k,
            leaf.eval,
            &leaf.domain,
            &leaf.line,
            &leaf.interval,
            leaf.conts,
            &solution,
        )? {
            return Ok(());
        }
        // for points within the surface's trim boundary
        if self.is_point_valid(k, leaf.owner, leaf.cache, uv)? {
            self.add_sorted_solution(&solution, SortKey::ByAllParameters, true)?;
            k.work_counters.add_line_root();
        }
        Ok(())
    }

    /// The patch boundary of `leaf` at its `dir` minimum (`side == 0`) or maximum
    /// when its continuity there is at most C0: coincident ranges of the line along
    /// the boundary iso curve, else its crossings. True when upstream returns (a
    /// coincidence was found, or a point on a degenerate interval).
    fn boundary_solve(
        &mut self,
        k: &Kernel,
        leaf: &LsiLeaf<'s, O>,
        patch_box: &crate::geom::pseudo_box::PseudoBox,
        dir: SurfParamType,
        side: usize,
    ) -> Result<bool> {
        let [u_conts, v_conts, _] = leaf.conts;
        let is_u = dir == SurfParamType::U;
        let cont = if is_u { u_conts } else { v_conts };
        if !cont.get(side).copied().is_some_and(at_most_c0) {
            return Ok(false);
        }
        let domain = leaf.domain;
        let constant = match (is_u, side) {
            (true, 0) => domain.min().x,
            (true, _) => domain.max().x,
            (false, 0) => domain.min().y,
            (false, _) => domain.max().y,
        };
        let iso = implicit_iso(k, leaf.eval, dir, constant, side == 0)?;
        if iso.is_degenerate(k, EFF_ZERO, None)? {
            return Ok(false);
        }
        let iso_ivl = if is_u {
            Extent1d::from_min_max(domain.min().y, domain.max().y)
        } else {
            Extent1d::from_min_max(domain.min().x, domain.max().x)
        };
        let tol = self.base.tolerance_3d;

        // Check for coincidence with curves of low continuity or boundary curves
        let k = self.kernel;
        let found = leaf.line.simple_coincidence_checker::<O>(
            k,
            &leaf.interval,
            &iso,
            &iso_ivl,
            tol,
            None,
            Some(patch_box),
            [None, None],
        )?;
        let num_found = found.len();
        let mut found_coin = false;
        for mut solution in found {
            // turn TmpLine/isoCurve solution into a line:[pt,tan]/Surface solution
            solution.num_variables = 3;
            leaf.adopt(&mut solution);
            if is_u {
                let other = p(&solution.start, 1)?;
                set_p(&mut solution.start, 2, other)?;
                set_p(&mut solution.start, 1, constant)?;
            } else {
                set_p(&mut solution.start, 2, constant)?;
            }
            if solution.solution_type == SolutionType::RangeOfValues {
                found_coin = true;
                if is_u {
                    let other = p(&solution.end, 1)?;
                    set_p(&mut solution.end, 2, other)?;
                    set_p(&mut solution.end, 1, constant)?;
                } else {
                    set_p(&mut solution.end, 2, constant)?;
                }
                self.add_sorted_solution(&solution, SortKey::ByAllParameters, true)?;
            } else {
                // Single valued solutions
                let keep = self.keep_point(k, leaf, leaf.eval, &domain, &solution)?;
                // when working with degenerate intervals we're done (v boundary only)
                if keep
                    && !is_u
                    && (leaf.interval.length() < EFF_ZERO || iso_ivl.length() < EFF_ZERO)
                {
                    found_coin = true;
                }
            }
        }

        // If we found a coincidence we are done
        if found_coin {
            return Ok(true);
        }

        // when the point tester found nothing look for straight intersections
        if num_found == 0 {
            if let Some(hit) = local_curve_intersect(
                k,
                &iso,
                &iso_ivl,
                &leaf.line,
                &leaf.interval,
                tol,
                iso_ivl.evaluate(0.5),
                leaf.interval.evaluate(0.5),
            )? {
                let uv = if is_u {
                    Point2::new(constant, hit.my_param)
                } else {
                    Point2::new(hit.my_param, constant)
                };
                let solution = leaf.solution(hit.other_param, uv, hit.deviation)?;
                self.keep_point(k, leaf, leaf.eval, &domain, &solution)?;
            }
        }
        Ok(false)
    }
}

// ---- sm_GlobalCurveIntersectCombineSolutions ---------------------------------------------------

// ---- the certified pass (not upstream) ---------------------------------------------------------

/// The line/surface solver with the certified pass (module docs; not upstream): it
/// traverses as [`LsiGlobalSolver`] does, keeps each leaf whose guesses ran, and after the
/// traversal [`CertifiedLsi::certified_pass`] searches them. Without a net it is the
/// solver alone.
pub(crate) struct CertifiedLsi<'t, 's, O> {
    /// The solver the traversal runs.
    pub(crate) solver: LsiGlobalSolver<'t, 's, O>,
    /// The surface's exact net ([`certified_line::LineNet::of`]), `None` for a surface
    /// the pass does not run on.
    net: Option<certified_line::LineNet>,
    /// The leaves whose guesses ran, and the culls.
    kept: Kept<O>,
    /// The searches of the certified pass the operation keeps
    /// ([`certified_line::LinePasses`]); `None`: every solve searches.
    passes: Option<&'s RefCell<certified_line::LinePasses>>,
}

impl<'t, 's, O: SolutionObject + 't> CertifiedLsi<'t, 's, O> {
    /// `solver` with the certified pass on `net`, reusing the searches `passes` keeps.
    pub(crate) fn new(
        solver: LsiGlobalSolver<'t, 's, O>,
        net: Option<certified_line::LineNet>,
        passes: Option<&'s RefCell<certified_line::LinePasses>>,
    ) -> Self {
        Self {
            solver,
            net,
            kept: Kept::default(),
            passes,
        }
    }

    /// The certified pass (module docs): for every leaf whose guesses ran, the crossings
    /// [`certified_line::isolate`] certifies, each added as the guesses' hits are (the
    /// ray's negative parameters skipped, the filter, the trim boundary) unless a solution
    /// of the solve already stands for it. Run by the caller after the traversal, with the
    /// solve's solutions in the solver's `base.solutions`, so that a root some other
    /// leaf's guess found is never solved a second time. Returns the solve's side record
    /// (in the solver's scaled line parameter), [`SideRecord::NotRun`] without a net.
    ///
    /// # Errors
    ///
    /// A cancel, or an unported dependency of the evaluation, the filter or the point
    /// test.
    pub(crate) fn certified_pass(&mut self, k: &Kernel) -> Result<SideRecord> {
        let Some(net) = self.net.take() else {
            return Ok(SideRecord::NotRun);
        };
        let kept = std::mem::take(&mut self.kept);
        let added = self.solver.add_certified_roots(k, &net, &kept, self.passes);
        self.net = Some(net);
        added
    }
}

impl<'t, O: SolutionObject + 't> GlobalSolve<'t, O> for CertifiedLsi<'t, '_, O> {
    fn solver(&self) -> &crate::geom::global_solver::GlobalSolver<'t, O> {
        &self.solver.base
    }

    fn solver_mut(&mut self) -> &mut crate::geom::global_solver::GlobalSolver<'t, O> {
        &mut self.solver.base
    }

    fn branch_may_contain_answers(&mut self, branch: &Branch) -> Result<bool> {
        let may = self.solver.may_contain_answers(branch)?;
        // (a subtree pruned by its sphere bound is a cull: no proof; its strip is enclosed)
        if !may && self.net.is_some() {
            match self.solver.branch_strip(branch) {
                CullStrip::At(domain) => self.kept.strips.push(domain),
                CullStrip::Outside => {}
                CullStrip::Unplaced => self.kept.unplaced = self.kept.unplaced.saturating_add(1),
            }
        }
        Ok(may)
    }

    fn local_solve(&mut self, branch: &Branch) -> Result<bool> {
        let k = self.solver.kernel;
        let keep = self.net.is_some().then_some(&mut self.kept);
        self.solver.local_solve_branch_keeping(k, branch, keep)
    }

    fn identical_solutions(
        &self,
        sol1: &Solution<O>,
        sol2: &Solution<O>,
    ) -> Result<(
        crate::geom::global_solver::SolutionPairType,
        Option<crate::geom::global_solver::PreferredSolution>,
    )> {
        self.solver.identical_solutions(sol1, sol2)
    }
}

/// Where the combination finds the cache nodes and objects of the solutions (see the
/// module deviations).
#[derive(Debug, Clone, Copy)]
pub struct CombineContext<'a, O> {
    /// The curve cache tree `m_apNodes[0]` indexes.
    pub curve_tree: Option<&'a Tree<O>>,
    /// The surface cache whose tree `m_apNodes[1]` indexes.
    pub surface_cache: Option<&'a SurfaceCache<O>>,
    /// The curve's handle (upstream `&crCurve`).
    pub curve_handle: Option<O>,
    /// The surface's handle (upstream `&crSurface`).
    pub surface_handle: Option<O>,
    /// The tolerance of the curve's owning edge (`pCurveOwner->GetTolerance()` of
    /// `sm_ClassifyCurveSurfaceXSectPoint`), when the caller resolves it.
    pub edge_tolerance: Option<f64>,
}

impl<O> Default for CombineContext<'_, O> {
    fn default() -> Self {
        Self {
            curve_tree: None,
            surface_cache: None,
            curve_handle: None,
            surface_handle: None,
            edge_tolerance: None,
        }
    }
}

impl<O: SolutionObject> CombineContext<'_, O> {
    /// The curve tree node of a solution (`m_apNodes[0]`).
    fn curve_node(&self, id: NodeId) -> Result<&crate::geom::tree::TreeNode<O>> {
        self.curve_tree
            .ok_or(SolidError::invalid_input(
                OP_COMBINE,
                "solution curve node without a curve tree",
            ))?
            .node(id)
    }

    /// The surface cache tree node of a solution (`m_apNodes[1]`).
    fn surface_node(&self, id: NodeId) -> Result<&crate::geom::tree::TreeNode<O>> {
        self.surface_cache
            .ok_or(SolidError::invalid_input(
                OP_COMBINE,
                "solution surface node without a surface cache",
            ))?
            .tree()
            .ok_or(SolidError::invalid_input(
                OP_COMBINE,
                "surface cache has no tree",
            ))?
            .node(id)
    }

    /// `bNode1Valid && bNode2Valid`: the curve node is a span or curve data node and
    /// the surface node a patch, patch head or aux data node; then the curve node's
    /// interval and the surface node's domain.
    fn node_spans(&self, nodes: &[Option<NodeId>]) -> Result<Option<(Extent1d, Extent2d)>> {
        let mut valid1 = None;
        if let Some(id) = nodes.first().copied().flatten() {
            let n = self.curve_node(id)?;
            if matches!(
                n.aux_type,
                AuxDataType::BezierCurve | AuxDataType::CurveData
            ) {
                valid1 = Some(n);
            }
        }
        let mut valid2 = None;
        if let Some(id) = nodes.get(1).copied().flatten() {
            let n = self.surface_node(id)?;
            if matches!(
                n.aux_type,
                AuxDataType::BezierSurface | AuxDataType::BezierSurfaceHead | AuxDataType::AuxData
            ) {
                valid2 = Some(id);
            }
        }
        let (Some(n1), Some(id2)) = (valid1, valid2) else {
            return Ok(None);
        };
        let tree = self.curve_tree.ok_or_else(|| {
            SolidError::surface(
                OP_COMBINE,
                SurfaceFailure::SolverWithout {
                    part: SolverPart::CurveTree,
                },
            )
        })?;
        let cache = self.surface_cache.ok_or_else(|| {
            SolidError::surface(
                OP_COMBINE,
                SurfaceFailure::SolverWithout {
                    part: SolverPart::SurfaceCache,
                },
            )
        })?;
        Ok(Some((
            tree.curve_aux(n1)?.interval,
            cache.aux(id2)?.uv_domain,
        )))
    }
}

/// The point classification of the combination: whether `solution`'s start point is
/// classified `SM_TC_COINCIDENT` (status ignored upstream).
#[allow(clippy::too_many_arguments)]
fn is_coincident_point<O>(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    edge_tolerance: Option<f64>,
    solution: &Solution<O>,
) -> Result<bool> {
    let uv = Point2::new(p(&solution.start, 1)?, p(&solution.start, 2)?);
    let t = p(&solution.start, 0)?;
    // (passed on as the fork does, WR-663, tangency-labs/smlib 21dcf69)
    let class = classify_curve_surface_xsect_point(
        k,
        surface,
        uv_domain,
        curve,
        interval,
        uv,
        t,
        edge_tolerance,
    )?;
    Ok(class.point_type == TsectCurveType::Coincident)
}

/// Drops the curve points at `fractions` of the opening between two range solutions
/// to the surface: false when one does not drop or lands farther than `tol`.
#[allow(clippy::too_many_arguments)] // the opening and its surface
fn opening_drops(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    curve: &Curve,
    ivls: [&Extent1d; 3],
    tol: f64,
    count: u32,
    step: f64,
) -> Result<bool> {
    let [ivl1, ivl2, ivl3] = ivls;
    for jj in 0..count {
        let param = step * f64::from(jj + 1);
        let pt_on1 = curve.evaluate_point(k, ivl1.evaluate(param))?;
        let guess = Point2::new(ivl2.evaluate(param), ivl3.evaluate(param));
        let dropped = surface_drop_point(k, surface, pt_on1, uv_domain, guess)?;

        // don't combine solutions when SolutionGap MidPoint does not drop or is too far
        if !dropped.success || dropped.gap > tol {
            return Ok(false);
        }
    }
    Ok(true)
}

/// `sm_GlobalCurveIntersectCombineSolutions`: combines the curve/surface solutions
/// `[curve t, surface u, surface v]` (sorted by the curve parameter) into unique point
/// solutions and coincident segments, in place: merges touching, overlapping and
/// coincidently joined ranges and coincident points, drops short ranges next to
/// points, turns small or degenerate ranges into points, and when a coincidence is
/// found replaces the solutions by the curve's drop onto the surface.
///
/// # Errors
///
/// Those of the evaluations and approximate lengths, a solution node without its tree
/// (see [`CombineContext`]), and [`SolidError::Unported`] from the calls into units not
/// ported yet.
#[allow(clippy::too_many_arguments, clippy::too_many_lines)] // upstream's arguments
pub fn global_curve_intersect_combine_solutions<O: SolutionObject>(
    k: &Kernel,
    surface: &Surface,
    uv_domain: &Extent2d,
    curve: &Curve,
    interval: &Extent1d,
    distance_tolerance: f64,
    ctx: &CombineContext<'_, O>,
    solutions: &mut SolutionArray<O>,
) -> Result<()> {
    let tol = distance_tolerance;
    let mut found_coincidence = false;

    // Phase 1: combine pairs of solutions of the same type.
    let uv_size = surface.natural_uv_domain_with_nurb(k)?.size();
    let t_size = curve.natural_interval().length();

    // set up rSolutions array indexing for Point and interval solutions
    let mut last_interval = 0;
    let mut last_point = 0;
    if !solutions.is_empty() {
        let first = solutions.get_at(0)?;
        if first.solution_type == SolutionType::RangeOfValues {
            last_interval = 0;
            last_point = NO_INDEX;
        } else {
            last_interval = NO_INDEX;
            last_point = 0;
            if is_coincident_point(
                k,
                surface,
                uv_domain,
                curve,
                interval,
                ctx.edge_tolerance,
                &first,
            )? {
                found_coincidence = true;
            }
        }
    }

    // for every rSolution after the 1st
    let num = solutions.len();
    for ii in 1..num {
        // index of the last solution of the current solution's type
        let curr_was_range = solutions.get_at(ii)?.solution_type == SolutionType::RangeOfValues;
        let last_index = if curr_was_range {
            last_interval
        } else {
            last_point
        };

        // last solution and its end (rLastSol, pLastEnd)
        let last_slot = if last_index == NO_INDEX {
            0
        } else {
            last_index
        };
        let mut last_sol = solutions.get_at(last_slot)?;
        let last_end = if last_sol.solution_type == SolutionType::RangeOfValues {
            last_sol.end
        } else {
            last_sol.start
        };

        let mut do_combine = false;
        'decide: {
            // no previous solution of same type - keep solution
            if last_index == NO_INDEX {
                break 'decide;
            }

            // Look for other solutions starting where this one starts on the curve and
            // closer to the last end on the surface.
            let mut closest = ii;
            let scaled_zero = EFF_ZERO * (1.0 + p(&solutions.get_at(ii)?.start, 0)?.abs());
            for jj in (ii + 1)..num {
                let c = solutions.get_at(closest)?.start;
                let o = solutions.get_at(jj)?.start;
                if (p(&c, 0)? - p(&o, 0)?).abs() < scaled_zero {
                    let u_diff1 = (p(&c, 1)? - p(&last_end, 1)?).abs();
                    let v_diff1 = (p(&c, 2)? - p(&last_end, 2)?).abs();
                    let u_diff2 = (p(&o, 1)? - p(&last_end, 1)?).abs();
                    let v_diff2 = (p(&o, 2)? - p(&last_end, 2)?).abs();
                    if u_diff2 + v_diff2 < u_diff1 + v_diff1 {
                        closest = jj;
                    }
                } else {
                    // quit at the first worst solution
                    break;
                }
            }

            // when a closer OtherSolution was found - swap its position
            if closest != ii {
                solutions.swap(closest, ii)?;
            }
            let curr = solutions.get_at(ii)?;
            let curr_start = curr.start;

            // Don't allow jumping over seam
            let uv_diff = Vec2::new(
                (p(&curr_start, 1)? - p(&last_end, 1)?).abs(),
                (p(&curr_start, 2)? - p(&last_end, 2)?).abs(),
            );
            let t_diff = (p(&curr_start, 0)? - p(&last_end, 0)?).abs();
            if uv_diff.x > uv_size.x * 0.90 || uv_diff.y > uv_size.y * 0.90 {
                // don't connect solutions across Surface seams
                break 'decide;
            }

            // don't connect solutions across closed curve ends
            if t_diff > t_size * 0.90 {
                break 'decide;
            }

            // When Solution endPoints touch on either curve or surface - combine
            if are_same(p(&last_end, 0)?, p(&curr_start, 0)?)
                || (are_same(p(&last_end, 1)?, p(&curr_start, 1)?)
                    && are_same(p(&last_end, 2)?, p(&curr_start, 2)?))
            {
                do_combine = true;

                // for interval solutions - make some exceptions
                if curr.solution_type == SolutionType::RangeOfValues {
                    let start_dev1 = last_sol.start.solution_value;
                    let end_dev1 = last_sol.end.solution_value;

                    // don't combine when end of rLastSol is much better than start
                    if start_dev1 > EFF_ZERO && start_dev1 > end_dev1 * 100_000.0 {
                        do_combine = false;
                    }
                    let start_dev2 = curr.start.solution_value;
                    let end_dev2 = curr.end.solution_value;

                    // don't combine when start of CurrSol is much better than end
                    if end_dev2 > EFF_ZERO && end_dev2 > start_dev2 * 100_000.0 {
                        do_combine = false;
                    }

                    // If the smaller segment is less than 1/1000 of the other then
                    // just combine them anyhow.
                    let last_leng = max2(EFF_ZERO, p(&last_sol.end, 0)? - p(&last_sol.start, 0)?);
                    let curr_leng = max2(EFF_ZERO, p(&curr.end, 0)? - p(&curr.start, 0)?);
                    if last_leng > curr_leng * 1000.00 || curr_leng > last_leng * 1000.00 {
                        do_combine = true;
                    }
                }
                break 'decide;
            }

            // for point solutions only
            if curr.solution_type == SolutionType::SingleValue {
                // evaluate solution points (statuses ignored upstream; passed on as the
                // fork does, WR-663, tangency-labs/smlib 21dcf69)
                let pt1 = curve.evaluate_point(k, p(&curr_start, 0)?)?;
                let pt2 = curve.evaluate_point(k, p(&last_end, 0)?)?;
                let dist2 = pt1.distance_squared(pt2);

                // combine solutions when points are close enough together
                do_combine = dist2 < tol * tol;
                if do_combine {
                    break 'decide;
                }

                // see if points bound a coincident segment region (the first solution)
                let first = solutions.get_at(0)?;
                if is_coincident_point(
                    k,
                    surface,
                    uv_domain,
                    curve,
                    interval,
                    ctx.edge_tolerance,
                    &first,
                )? {
                    found_coincidence = true;
                }
            }

            // for range solutions only
            if curr.solution_type == SolutionType::RangeOfValues {
                // When SolutionIntervals overlap on both curve and surface - combine
                if p(&last_end, 0)? >= p(&curr_start, 0)? {
                    let u = p(&curr_start, 1)?;
                    let (u0, u1) = (p(&last_sol.start, 1)?, p(&last_sol.end, 1)?);
                    do_combine = is_between(u, u0, u1) && is_between(u, u0, u1);
                    break 'decide;
                }

                // curve parameter intervals from lastSol->End to nextSol->Start
                let mut ivl1 = Extent1d::new();
                let mut ivl2 = Extent1d::new();
                let mut ivl3 = Extent1d::new();
                ivl1.add_value(p(&last_end, 0)?);
                ivl1.add_value(p(&curr_start, 0)?);
                ivl2.add_value(p(&last_end, 1)?);
                ivl2.add_value(p(&curr_start, 1)?);
                ivl3.add_value(p(&last_end, 2)?);
                ivl3.add_value(p(&curr_start, 2)?);

                // drop two opening points [.33 .66] to the surface
                if !opening_drops(
                    k,
                    surface,
                    uv_domain,
                    curve,
                    [&ivl1, &ivl2, &ivl3],
                    tol,
                    2,
                    0.33,
                )? {
                    break 'decide;
                }

                // a small opening: assume the curves are coincident between solutions
                if ivl1.length() < t_size / 1000.0
                    && (ivl2.length() < uv_size.x / 1000.0 || ivl3.length() < uv_size.y / 1000.0)
                {
                    do_combine = true;
                    break 'decide;
                }

                // sample three more points [.25 .50 .75]
                if !opening_drops(
                    k,
                    surface,
                    uv_domain,
                    curve,
                    [&ivl1, &ivl2, &ivl3],
                    tol,
                    3,
                    0.25,
                )? {
                    break 'decide;
                }

                // the gap between two interval solutions is coincident
                do_combine = true;
            }
        }

        // continue here after deciding whether to combine current and last solutions
        let curr = solutions.get_at(ii)?;
        if do_combine {
            if curr.solution_type == SolutionType::RangeOfValues {
                // Expand rLastSol interval to cover rCurrSol interval
                if p(&last_sol.end, 0)? < p(&curr.end, 0)? {
                    last_sol.end = curr.end;
                    solutions.set_at(last_slot, &last_sol)?;
                }
            } else {
                // just use the better solution
                let mut kept = if curr.start.solution_value < last_end.solution_value {
                    curr
                } else {
                    last_sol
                };
                // (not upstream) the point kept stands for both
                kept.contact = last_sol.contact.merged(curr.contact);
                kept.shared_contact = None;
                solutions.set_at(last_slot, &kept)?;
            }
        } else {
            // Keep the current solution, packed into the array
            let next = if last_interval == NO_INDEX {
                last_point.saturating_add(1)
            } else if last_point == NO_INDEX {
                last_interval.saturating_add(1)
            } else {
                last_interval.max(last_point).saturating_add(1)
            };
            if curr_was_range {
                last_interval = next;
            } else {
                last_point = next;
            }
            if next != ii {
                solutions.set_at(next, &curr)?;
            }
        }
    }

    // Set rSolutions array Size after all combining has been done.
    if !solutions.is_empty() {
        let last = if last_interval == NO_INDEX {
            last_point
        } else if last_point == NO_INDEX {
            last_interval
        } else {
            last_interval.max(last_point)
        };
        // the packed indices never pass the array (shrinking only)
        if last >= solutions.len() {
            return Err(SolidError::surface(
                OP_COMBINE,
                SurfaceFailure::Defect {
                    why: "the combination packed a solution index past the end of the \
                          solutions, though packing only moves solutions toward the front",
                },
            ));
        }
        solutions.set_size(last + 1);
    }

    // Phase 2: remove coincident segments which include or are adjacent to point
    // intersections.
    let num = solutions.len();
    let mut cull = vec![false; num];
    let mut compress = false;
    for ii in 0..num {
        let inv_sol = solutions.get_at(ii)?;
        if let Some(c) = cull.get_mut(ii) {
            *c = false;
        }

        // skip point-solutions solutions
        if inv_sol.solution_type != SolutionType::RangeOfValues {
            continue;
        }
        let inv_start = inv_sol.start;
        let inv_end = inv_sol.end;
        let mut inv_span1 = Extent1d::new();
        inv_span1.add_value(p(&inv_start, 0)?);
        inv_span1.add_value(p(&inv_end, 0)?);
        let mut inv_span2 = Extent1d::new();
        inv_span2.add_value(p(&inv_start, 1)?);
        inv_span2.add_value(p(&inv_end, 1)?);
        // upstream adds the v values to the second span; the third stays Init()
        let inv_span3 = Extent1d::new();
        inv_span2.add_value(p(&inv_start, 2)?);
        inv_span2.add_value(p(&inv_end, 2)?);

        // search for point solutions within or adjacent to this interval solution
        for jj in 0..num {
            if ii == jj {
                continue;
            }
            let pt_sol = solutions.get_at(jj)?;

            // skip interval solutions
            if pt_sol.solution_type != SolutionType::SingleValue {
                continue;
            }
            let pt_start = pt_sol.start;

            // Get the cache subdivision intervals containing this point
            if let Some((pt_ivl1, dom)) = ctx.node_spans(&pt_sol.nodes)? {
                let pt_ivl2 = dom.u_interval();
                let pt_ivl3 = dom.v_interval();

                // overlap between IntervalSolution Span and PointSubDivision Span
                // (a disjoint intersection is Init())
                let xsect1 = pt_ivl1
                    .intersect(&inv_span1)
                    .unwrap_or_else(|_| Extent1d::new());
                let xsect2 = pt_ivl2
                    .intersect(&inv_span2)
                    .unwrap_or_else(|_| Extent1d::new());
                let xsect3 = pt_ivl3
                    .intersect(&inv_span3)
                    .unwrap_or_else(|_| Extent1d::new());
                if xsect1.is_init() || xsect2.is_init() || xsect3.is_init() {
                    continue;
                }
            }

            // Delete the range interval only if it's 'short': 1/1000 of the whole domain.
            let close_param = interval.length() / 1000.0;
            if inv_span1.length() < close_param {
                if let Some(c) = cull.get_mut(ii) {
                    *c = true;
                }
                compress = true;
            } else {
                // Range is long enough.  See about deleting the point solution.
                let pt_param1 = p(&pt_sol.start, 0)?;
                let delete_point = if pt_param1 < inv_span1.min() {
                    let dist = inv_span1.min() - pt_param1;
                    dist < close_param
                        && pt_sol.start.solution_value > 2.0 * inv_start.solution_value
                } else if pt_param1 > inv_span1.max() {
                    let dist = pt_param1 - inv_span1.max();
                    dist < close_param && pt_sol.start.solution_value > 2.0 * inv_end.solution_value
                } else {
                    // Point sol is inside the range sol: delete it.
                    true
                };
                if delete_point {
                    if let Some(c) = cull.get_mut(jj) {
                        *c = true;
                    }
                    compress = true;
                }
            }
            if cull.get(ii).copied().unwrap_or(false) {
                break;
            }

            // quit searching when point solutions get past interval solutions
            if p(&pt_start, 0)? > inv_span1.max()
                || p(&pt_start, 1)? > inv_span2.max()
                || p(&pt_start, 2)? > inv_span3.max()
            {
                break;
            }
        }
    }

    // compress the solutions array to remove all unwanted interval solutions
    if compress {
        let mut next = 0;
        for ii in 0..num {
            if cull.get(ii).copied().unwrap_or(false) {
                continue;
            }
            if ii != next {
                let s = solutions.get_at(ii)?;
                solutions.set_at(next, &s)?;
            }
            next += 1;
        }
        if !solutions.is_empty() {
            solutions.set_size(next);
        }
    }

    // Phase 3: turn small and degenerate ranges into point solutions (one pass).
    let mut length = BIG_DOUBLE;
    let mut is_degenerate = false;
    for ii in 0..solutions.len() {
        let mut curr = solutions.get_at(ii)?;
        if curr.solution_type != SolutionType::RangeOfValues {
            continue;
        }
        let dev_start = curr.start.solution_value;
        let dev_end = curr.end.solution_value;
        let mut ivl = Extent1d::from_value(p(&curr.start, 0)?);
        let mut reg = Extent2d::from_point(Point2::new(p(&curr.start, 1)?, p(&curr.start, 2)?));
        ivl.add_value(p(&curr.end, 0)?);
        reg.add_point(Point2::new(p(&curr.end, 1)?, p(&curr.end, 2)?));
        let ivl_length = ivl.length();
        let reg_length = reg.size().length();
        let pnt = curve.evaluate_point(k, p(&curr.start, 0)?)?;
        if length == BIG_DOUBLE {
            length = curve.approximate_length(k, interval, 5, false)?.0;
        }
        let seg_leng = curve.approximate_length(k, &ivl, 5, false)?.0;
        let scaled_zero = EFF_ZERO * (1.0 + pnt.max_dimension());
        let scaled_ps_tol = 100.0 * EFF_ZERO * (1.0 + interval.max_dimension());
        let scaled_ps_tol2 = 100.0 * EFF_ZERO * (1.0 + uv_domain.max_dimension());

        // keep nonZero intervals whose ends are exact intersections
        if dev_start < scaled_zero
            && dev_end < scaled_zero
            && ivl_length > scaled_ps_tol
            && reg_length > scaled_ps_tol2
        {
            continue;
        }

        let large_interval = seg_leng > length / 10.0;
        let small_interval = ivl_length < scaled_ps_tol || reg_length < scaled_ps_tol2;

        // for nonDegenerate intervals (status ignored upstream; passed on as the fork
        // does, WR-663, tangency-labs/smlib 21dcf69)
        if !small_interval {
            // (`ClassifyIntersectingRangeSolution` writes only the deviation and the
            // first three parameters of `rCurrSol.m_vStart`)
            let (degenerate, point) =
                classify_intersecting_range_solution(k, surface, curve, &curr)?;
            is_degenerate = degenerate;
            if let Some(pt) = point {
                curr.start.solution_value = pt.solution_value;
                for (slot, v) in curr.start.parameters.iter_mut().zip(pt.parameters) {
                    *slot = v;
                }
                solutions.set_at(ii, &curr)?;
            }
        }
        if large_interval && !is_degenerate {
            continue;
        }

        // Change very small and diverging intervals into a single pt solutions.
        if small_interval || is_degenerate {
            if curr.start.solution_value > curr.end.solution_value {
                curr.start = curr.end;
            }
            curr.solution_type = SolutionType::SingleValue;
            solutions.set_at(ii, &curr)?;
        }
    }

    // a NonDegenerate RangeOfValues solution means a coincidence
    for sol in solutions.as_slice() {
        if found_coincidence {
            break;
        }
        if sol.solution_type == SolutionType::RangeOfValues
            && p(&sol.end, 0)? - p(&sol.start, 0)? > 0.02 * interval.length()
        {
            found_coincidence = true;
        }
    }
    if found_coincidence {
        // keep every point solution not tangent with the surface
        let mut tmp = SolutionArray::new();
        for sol in solutions.as_slice() {
            if sol.solution_type == SolutionType::SingleValue {
                let pv = curve.evaluate(k, p(&sol.start, 0)?, 1, true, true)?;
                let uv = Point2::new(p(&sol.start, 1)?, p(&sol.start, 2)?);
                let normal = surface.evaluate_normal(k, uv, Side::Left, Side::Left)?;
                let tangent = pv.get(1).copied().ok_or_else(|| {
                    SolidError::surface(
                        OP_COMBINE,
                        SurfaceFailure::Defect {
                            why: "the curve's evaluation returned no first derivative, though \
                                  it returns the derivatives 0..=1 asked of it",
                        },
                    )
                })?;
                if !tangent.is_perpendicular_to(normal, 2.0) {
                    tmp.add(sol);
                }
            }
        }

        // replace the coincident parts by the curve's drop onto the surface
        let dropped = drop_and_trim_curve(
            k,
            surface,
            ctx.surface_cache,
            uv_domain,
            curve,
            interval,
            tol,
        )?;
        if dropped.dist_to_surface > tol {
            return Ok(());
        }
        for uv_curve in &dropped.uv_curves {
            let ivl = uv_curve.natural_interval();
            let mut sol = Solution::new();
            sol.num_variables = 3;
            sol.num_objects = 2;
            sol.objects[0] = ctx.curve_handle;
            sol.objects[1] = ctx.surface_handle;
            sol.solution_type = SolutionType::RangeOfValues;
            let start = uv_curve.evaluate_point(k, ivl.min())?;
            sol.start.solution_value = dropped.dist_to_surface;
            set_p(&mut sol.start, 0, ivl.min())?;
            set_p(&mut sol.start, 1, start.x)?;
            set_p(&mut sol.start, 2, start.y)?;
            let end = uv_curve.evaluate_point(k, ivl.max())?;
            sol.end.solution_value = dropped.dist_to_surface;
            set_p(&mut sol.end, 0, ivl.max())?;
            set_p(&mut sol.end, 1, end.x)?;
            set_p(&mut sol.end, 2, end.y)?;
            tmp.add(&sol);
        }
        if !dropped.uv_curves.is_empty() {
            solutions.reset();
            solutions.append(&tmp);
        }
        return Ok(());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::geom::solution_array::{Contact, ObjectKind};
    use crate::math::Vec3;
    use crate::nurbs::b_spline_curve::BSplineCurve;
    use crate::nurbs::b_spline_surface::BSplineSurface;
    use crate::nurbs::crv_basic::Curve as NurbCurve;
    use crate::nurbs::math::{CPoint, NOW};

    #[derive(Debug, Clone, Copy, PartialEq)]
    struct H(u8);
    impl SolutionObject for H {
        fn kind(&self) -> ObjectKind {
            ObjectKind::Surface
        }
    }

    fn plane() -> Surface {
        let net = (0..2)
            .map(|i| {
                (0..2)
                    .map(|j| CPoint::new(f64::from(i), f64::from(j), 0.0, NOW))
                    .collect()
            })
            .collect();
        let knots = vec![0.0, 0.0, 1.0, 1.0];
        let n = crate::nurbs::srf_basic::Surface::new(net, 1, 1, knots.clone(), knots).unwrap();
        Surface::from(BSplineSurface::from_nurb(n, false))
    }

    fn line() -> Curve {
        let n = NurbCurve::new(
            vec![
                CPoint::new(0.0, 0.5, 0.0, NOW),
                CPoint::new(1.0, 0.5, 0.0, NOW),
            ],
            1,
            vec![0.0, 0.0, 1.0, 1.0],
        )
        .unwrap();
        Curve::from(BSplineCurve::from_nurb(n, 3, false))
    }

    fn range(t0: f64, t1: f64) -> Solution<H> {
        let mut s = Solution::new();
        s.solution_type = SolutionType::RangeOfValues;
        s.num_variables = 3;
        s.start.parameters[0] = t0;
        s.start.parameters[1] = t0;
        s.start.parameters[2] = 0.5;
        s.end.parameters[0] = t1;
        s.end.parameters[1] = t1;
        s.end.parameters[2] = 0.5;
        s
    }

    #[test]
    fn touching_short_ranges_combine() {
        let k = Kernel::default();
        let mut sols = SolutionArray::new();
        sols.add(&range(0.100, 0.105));
        sols.add(&range(0.105, 0.110));
        // across the surface seam (v jumps by the domain size): kept apart
        let mut seam = range(0.500, 0.505);
        seam.start.parameters[2] = 1.5;
        seam.end.parameters[2] = 1.5;
        sols.add(&seam);
        global_curve_intersect_combine_solutions(
            &k,
            &plane(),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            &line(),
            &Extent1d::from_min_max(0.0, 1.0),
            1e-6,
            &CombineContext::default(),
            &mut sols,
        )
        .unwrap();
        assert_eq!(sols.len(), 2);
        let first = sols.get_at(0).unwrap();
        assert_eq!(first.end.parameters[0], 0.110);
        assert_eq!(sols.get_at(1).unwrap().start.parameters[0], 0.500);
    }

    #[test]
    fn leading_point_on_coincident_line_becomes_range() {
        let k = Kernel::default();
        let mut sols = SolutionArray::new();
        let mut s = range(0.2, 0.2);
        s.solution_type = SolutionType::SingleValue;
        sols.add(&s);
        global_curve_intersect_combine_solutions(
            &k,
            &plane(),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            &line(),
            &Extent1d::from_min_max(0.0, 1.0),
            1e-6,
            &CombineContext::default(),
            &mut sols,
        )
        .unwrap();
        // the line lies on the surface: its drop (an iso line) replaces the point
        // by the whole coincident range
        assert_eq!(sols.len(), 1);
        let r = sols.get_at(0).unwrap();
        assert_eq!(r.solution_type, SolutionType::RangeOfValues);
        assert_eq!(r.start.parameters[..3], [0.0, 0.0, 0.5]);
        assert_eq!(r.end.parameters[..3], [1.0, 1.0, 0.5]);
    }

    #[test]
    fn empty_solutions_stay_empty() {
        let k = Kernel::default();
        let mut sols: SolutionArray<H> = SolutionArray::new();
        global_curve_intersect_combine_solutions(
            &k,
            &plane(),
            &Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            &line(),
            &Extent1d::from_min_max(0.0, 1.0),
            1e-6,
            &CombineContext::default(),
            &mut sols,
        )
        .unwrap();
        assert!(sols.is_empty());
    }

    /// The line and curve intersection failures keep the operation, status and entity of the
    /// bare `Upstream` statuses they replace (a sample); their class is the cause's.
    #[test]
    fn typed_surface_failures_keep_their_upstream_answers() {
        use crate::error::surface::{SolverPart, SurfaceFailure};
        use crate::error::ErrorKind;
        let samples: [(&'static str, SurfaceFailure, i32, ErrorKind); 2] = [
            (
                OP_LSI,
                SurfaceFailure::SolverWithout {
                    part: SolverPart::SurfaceCache,
                },
                crate::error::SM_ERR_NULL_POINTER,
                ErrorKind::InvalidInput,
            ),
            (
                OP_COMBINE,
                SurfaceFailure::Defect {
                    why: "the curve's evaluation returned no first derivative, though it \
                          returns the derivatives 0..=1 asked of it",
                },
                crate::error::SM_ERR,
                ErrorKind::StageFailed,
            ),
        ];
        for (operation, failure, code, kind) in samples {
            let typed = SolidError::surface(operation, failure);
            let before = SolidError::Upstream {
                operation,
                code,
                entity: None,
            };
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.upstream_status(), Some(code), "{typed:?}");
            assert_eq!(typed.kind(), kind, "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), None, "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(typed.surface_failure(), Some(&failure));
        }
    }

    /// The torus of radii 30 and 5 about z.
    fn torus(k: &Kernel) -> Surface {
        let mut torus = crate::surface::torus::Torus::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            30.0,
            5.0,
            crate::surface::torus::TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(k).unwrap();
        Surface::from(torus)
    }

    /// The parameters at which the line `p + t d` meets the torus of radii `major` and
    /// `minor` about z, from its implicit equation
    /// `(|X|² + R² − r²)² − 4 R² (x² + y²) = 0`: every sign change over `[0, 200]` in
    /// steps of 1e-3, closed in by bisection.
    fn torus_line_roots(p: Vec3, d: Vec3, major: f64, minor: f64) -> Vec<f64> {
        let f = |t: f64| {
            let x = p + t * d;
            let s = x.dot(x) + major * major - minor * minor;
            s * s - 4.0 * major * major * (x.x * x.x + x.y * x.y)
        };
        let mut roots = Vec::new();
        let step = 1.0e-3;
        let mut a = 0.0;
        while a < 200.0 {
            let b = a + step;
            if f(a) == 0.0 || f(a) * f(b) < 0.0 {
                let (mut lo, mut hi) = (a, b);
                for _ in 0..80 {
                    let mid = 0.5 * (lo + hi);
                    if f(lo) * f(mid) <= 0.0 {
                        hi = mid;
                    } else {
                        lo = mid;
                    }
                }
                roots.push(0.5 * (lo + hi));
            }
            a = b;
        }
        roots
    }

    /// A ray that enters and leaves a torus near its crown, 2.5° and 2° off the surface
    /// and 2.75 apart, as the search for the infinite region of a two-sided revolve of
    /// 130° / 40° fires it: every crossing is found, the entry before the exit (the
    /// guesses alone gave the exit as the first hit, tangency-labs/solidcraft#20499).
    #[test]
    fn a_line_through_a_torus_near_its_crown_gets_every_crossing() {
        let k = Kernel::default();
        let s = torus(&k);
        let p = Vec3::new(82.388_988_497_226_59, 82.388_988_497_226_59, 10.000_04);
        let d = Vec3::new(
            -0.850_300_871_283_819_5,
            -0.523_935_376_527_776_1,
            -0.049_801_099_552_871_67,
        );
        let expected = torus_line_roots(p, d, 30.0, 5.0);
        // the entry and the exit near the crown, and the two crossings of the tube beyond
        assert_eq!(expected.len(), 4, "{expected:?}");
        assert!((expected[0] - 100.409_930_8).abs() < 1.0e-6, "{expected:?}");
        assert!((expected[1] - 103.163_922_1).abs() < 1.0e-6, "{expected:?}");
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let sols = s
            .base_global_line_intersect::<H>(&k, H(0), &domain, p, d, None, false, 2.0e-5)
            .unwrap();
        let found: Vec<f64> = sols
            .as_slice()
            .iter()
            .map(|sol| {
                assert_eq!(sol.solution_type, SolutionType::SingleValue);
                sol.start.parameters[0]
            })
            .collect();
        assert_eq!(
            found.len(),
            expected.len(),
            "{found:?} against {expected:?}"
        );
        for (t, e) in found.iter().zip(&expected) {
            assert!((t - e).abs() < 1.0e-9, "{found:?} against {expected:?}");
        }
        for sol in sols.as_slice() {
            let [t, u, v] = [0, 1, 2].map(|i| sol.start.parameters[i]);
            let on_surface = s.evaluate_point(&k, Point2::new(u, v)).unwrap();
            assert!(on_surface.distance(p + t * d) < 1.0e-9);
            // each a transversal crossing, typed by its certified root
            assert_eq!(sol.contact, Contact::Crossing, "{t}");
        }
    }

    /// A line dipping 2e-6 below the torus's crown where the crown meets the seam crosses it
    /// twice, at 0.5 ± 0.0259 (the gap along the crown's tangent is s⁴/(8R²r), so the dip is
    /// met at s = ±(8R²r·2e-6)^¼ ≈ ±0.518 on a line of length 20). The guesses miss both,
    /// and the pass stops at its box limit on the leaves there: it adds no root from them,
    /// neither one crossing of the two, which would make a ray's count odd, nor both. The
    /// solve's answer about the contact is the guesses' alone: no root added (the work
    /// counter of roots added at the leaf-to-solution boundary) and no point at either
    /// crossing.
    #[test]
    fn a_leaf_the_pass_does_not_finish_adds_no_crossing() {
        let k = Kernel::default();
        let s = torus(&k);
        let p = Vec3::new(30.0, -10.0, 5.0 - 2.0e-6);
        let d = Vec3::new(0.0, 20.0, 0.0);
        let expected = torus_line_roots(p, d, 30.0, 5.0);
        assert_eq!(expected.len(), 2, "{expected:?}");
        assert!((expected[0] - 0.474_10).abs() < 1.0e-4, "{expected:?}");
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        k.work_counters.reset();
        let sols = s
            .base_global_line_intersect::<H>(&k, H(0), &domain, p, d, None, false, 1.0e-6)
            .unwrap();
        assert!(k.work_counters.line_leaves_at_limit() > 0);
        assert_eq!(k.work_counters.line_roots_added(), 0);
        let crossings = sols
            .as_slice()
            .iter()
            .filter(|sol| {
                sol.solution_type == SolutionType::SingleValue
                    && expected
                        .iter()
                        .any(|e| (sol.start.parameters[0] - e).abs() < 1.0e-6)
            })
            .count();
        assert_eq!(crossings, 0, "a crossing published: {:?}", sols.as_slice());
        // the leaves there stop at their limit: whatever the guesses return about the
        // contact is undecided
        assert!(sols
            .as_slice()
            .iter()
            .all(|sol| sol.contact == Contact::Undecided));
        // and so is the line dipping 5e-7 below the crown at 45° (tolerance 1e-6), the
        // guesses' range or points about its two crossings
        let c = 30.0 / 2.0_f64.sqrt();
        let along = Vec3::new(-1.0, 1.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let p = Vec3::new(c, c, 5.0 - 5.0e-7) - 10.0 * along;
        let sols = s
            .base_global_line_intersect::<H>(
                &k,
                H(0),
                &domain,
                p,
                20.0 * along,
                None,
                false,
                1.0e-6,
            )
            .unwrap();
        assert!(!sols.is_empty());
        assert!(
            sols.as_slice()
                .iter()
                .all(|sol| sol.contact == Contact::Undecided),
            "{:?}",
            sols.as_slice()
        );
    }

    /// A shallow secant of the unit cylinder, 9.09e-5 deep at tolerance 1.0001e-3 (upstream's
    /// tangency test at its step 10): the certified pass types its two crossings, a hair
    /// apart, each a crossing by its certified root, and the line solve's join across the
    /// coincident stretch then makes upstream's range over both (its golden), which takes
    /// two typed solutions in: undecided, its ends' unique boxes dropped.
    #[test]
    fn a_shallow_secant_s_crossings_are_typed_crossings() {
        let k = Kernel::default();
        let mut c = crate::surface::cone::Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            1.0,
            0.0,
            360.0,
            1.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c);
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let y: f64 = 1.0 - 2.0e-3 / 22.0;
        let half = (1.0 - y * y).sqrt() / 2.0;
        let sols = s
            .base_global_line_intersect::<H>(
                &k,
                H(0),
                &domain,
                Vec3::new(-1.0, y, 0.5),
                Vec3::new(2.0, 0.0, 0.0),
                Some(&Extent1d::from_min_max(0.0, 1.0)),
                false,
                1.0001e-3,
            )
            .unwrap();
        assert_eq!(sols.len(), 1, "{:?}", sols.as_slice());
        let range = sols.as_slice()[0];
        assert_eq!(range.solution_type, SolutionType::RangeOfValues);
        assert!((range.start.parameters[0] - (0.5 - half)).abs() < 1.0e-9);
        assert!((range.end.parameters[0] - (0.5 + half)).abs() < 1.0e-9);
        assert_eq!((range.contact, range.unique), (Contact::Undecided, None));
    }

    /// A line crossing the unit cylinder exactly on its seam's meridian (along x at y = 0,
    /// z = ½; the seam toward +x) is reported there twice, at `u ≈ 0` and at `u ≈ 2π`, one
    /// point of the net on the two ends of the range (p8's dedupe measures u natively and
    /// never merges them). The two are one contact (answer11, Q1): both crossings, sharing
    /// one id no other solution has, so that the line's two contacts, the entry at
    /// `x = −1` (a certified root) and the seam exit, count twice, not three times. The id
    /// survives the line's rescale (a line vector of length 7), and a ray keeps only its
    /// first hit, the entry, with no id. Before the seam pair the two were undecided (an odd
    /// rest with two points).
    #[test]
    fn a_crossing_on_the_seam_reported_twice_is_one_contact() {
        let k = Kernel::default();
        let mut c = crate::surface::cone::Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            1.0,
            0.0,
            360.0,
            1.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c);
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let p = Vec3::new(-2.0, 0.0, 0.5);
        for len in [4.0, 7.0] {
            let sols = s
                .base_global_line_intersect::<H>(
                    &k,
                    H(0),
                    &domain,
                    p,
                    Vec3::new(len, 0.0, 0.0),
                    None,
                    false,
                    1.0e-5,
                )
                .unwrap();
            let all = sols.as_slice();
            assert_eq!(all.len(), 3, "{all:?}");
            let entry = 1.0 / len;
            assert!((all[0].start.parameters[0] - entry).abs() < 1.0e-12);
            assert_eq!(
                (all[0].contact, all[0].shared_contact),
                (Contact::Crossing, None)
            );
            let id = all[1].shared_contact;
            assert!(id.is_some(), "{all:?}");
            for sol in &all[1..] {
                assert!(
                    (sol.start.parameters[0] - 3.0 / len).abs() < 1.0e-12,
                    "{all:?}"
                );
                assert_eq!((sol.contact, sol.shared_contact), (Contact::Crossing, id));
            }
            let (u1, u2) = (all[1].start.parameters[1], all[2].start.parameters[1]);
            assert!(
                u1.min(u2) < 1.0e-9 && u1.max(u2) > domain.max().x - 1.0e-9,
                "{all:?}"
            );
        }
        let ray = s
            .base_global_line_intersect::<H>(
                &k,
                H(0),
                &domain,
                p,
                Vec3::new(4.0, 0.0, 0.0),
                None,
                true,
                1.0e-5,
            )
            .unwrap();
        assert_eq!(ray.len(), 1, "{:?}", ray.as_slice());
        let first = ray.as_slice()[0];
        assert_eq!(
            (first.contact, first.shared_contact),
            (Contact::Crossing, None)
        );
        assert!((first.start.parameters[0] - 0.25).abs() < 1.0e-12);
    }

    /// The solve's leaves are counted by how their pass ended: on the line dipping below
    /// the crown where it meets the seam, some stop at the limit, the others finish, and
    /// none is counted as not run (every leaf's line and domain can be searched).
    #[test]
    fn the_leaves_are_counted_by_how_their_pass_ended() {
        let k = Kernel::default();
        let s = torus(&k);
        let p = Vec3::new(30.0, -10.0, 5.0 - 2.0e-6);
        let d = Vec3::new(0.0, 20.0, 0.0);
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        k.work_counters.reset();
        s.base_global_line_intersect::<H>(&k, H(0), &domain, p, d, None, false, 1.0e-6)
            .unwrap();
        assert!(k.work_counters.line_leaves_at_limit() > 0);
        assert!(k.work_counters.line_leaves_finished() > 0);
        assert_eq!(k.work_counters.line_leaves_not_run(), 0);
    }

    /// A line along z inside the torus's hole, 24 from its axis (1 inside its inner
    /// equator), passes no nearer than 1 to the surface. The traversal culls by the line's
    /// distance every leaf and subtree whose box the line misses: a slab or sphere test,
    /// which made the side record undecided over the whole line until the culled strips were
    /// enclosed. Each strip is now excluded by distance, and the record is complete.
    #[test]
    fn a_solve_whose_culled_strips_are_excluded_is_complete() {
        let k = Kernel::default();
        let s = torus(&k);
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        k.work_counters.reset();
        let sols = s
            .base_global_line_intersect::<H>(
                &k,
                H(0),
                &domain,
                Vec3::new(0.0, 24.0, -10.0),
                Vec3::new(0.0, 0.0, 20.0),
                None,
                false,
                1.0e-6,
            )
            .unwrap();
        assert_eq!(sols.len(), 0);
        assert!(k.work_counters.line_strips() > 0);
        assert_eq!(k.work_counters.line_strips_at_limit(), 0);
        assert_eq!(sols.side_record(), &SideRecord::Complete);
    }

    /// The side record a solve returns is in the line's own parameter: the solver runs on
    /// the line scaled to length about 1 (here 20 times shorter), and the record is scaled
    /// back with the solutions before a ray keeps its first. On a line of length 20 through
    /// the torus near its crown, over the line interval [5, 5.2], both crossings (at
    /// 100.409930808 / 20 and 103.163922132 / 20) are typed crossings and the record is
    /// complete. Over [5, 5.0204966], which ends a hair past the first crossing, that
    /// crossing's component reaches the interval's end and is undecided: every interval of
    /// the record lies in the line interval, rounded outward, and holds the solution. The
    /// same line fired as a ray keeps its first hit, and only what starts before it.
    #[test]
    fn the_side_record_is_scaled_back_before_a_ray_keeps_its_first_hit() {
        let k = Kernel::default();
        let s = torus(&k);
        let p = Vec3::new(82.388_988_497_226_59, 82.388_988_497_226_59, 10.000_04);
        let d = 20.0
            * Vec3::new(
                -0.850_300_871_283_819_5,
                -0.523_935_376_527_776_1,
                -0.049_801_099_552_871_67,
            );
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let solve = |end: f64| {
            let interval = Extent1d::from_min_max(5.0, end);
            s.base_global_line_intersect::<H>(
                &k,
                H(0),
                &domain,
                p,
                d,
                Some(&interval),
                false,
                2.0e-5,
            )
            .unwrap()
        };
        let both = solve(5.2);
        assert_eq!(both.len(), 2);
        assert!(both
            .as_slice()
            .iter()
            .all(|x| x.contact == Contact::Crossing));
        assert_eq!(both.side_record(), &SideRecord::Complete);
        let end = 5.020_496_6;
        let sols = solve(end);
        let t: Vec<f64> = sols
            .as_slice()
            .iter()
            .map(|x| x.start.parameters[0])
            .collect();
        assert_eq!(t.len(), 1, "{t:?}");
        assert!((t[0] - 100.409_930_808 / 20.0).abs() < 1.0e-9, "{t:?}");
        let SideRecord::Undecided(spans) = sols.side_record() else {
            panic!("{:?}", sols.side_record());
        };
        for &[lo, hi] in spans {
            assert!(lo >= 5.0 - 1.0e-12 && hi <= end + 1.0e-12, "{spans:?}");
        }
        for x in &t {
            assert!(
                spans.iter().any(|&[lo, hi]| lo <= *x && *x <= hi),
                "{x} {spans:?}"
            );
        }

        let ray = s
            .base_global_line_intersect::<H>(&k, H(0), &domain, p, d, None, true, 2.0e-5)
            .unwrap();
        assert_eq!(ray.len(), 1);
        let first = ray.as_slice()[0].start.parameters[0];
        assert!((first - t[0]).abs() < 1.0e-9, "{first} {t:?}");
        match ray.side_record() {
            SideRecord::Undecided(spans) => {
                assert!(spans.iter().all(|&[lo, _]| lo <= first), "{spans:?}");
            }
            SideRecord::Complete => {}
            SideRecord::NotRun => panic!("not run"),
        }
    }

    /// A context with plain caches that keeps the certified pass's searches, or not.
    struct KeepingSearches(Option<RefCell<certified_line::LinePasses>>);

    impl crate::surface::surface::p1::SurfaceSolveContext<H> for KeepingSearches {
        fn build_surface_cache(
            &mut self,
            k: &Kernel,
            surface: &Surface,
            handle: Option<H>,
        ) -> Result<SurfaceCache<H>> {
            super::super::p1::get_surface_cache(k, surface, handle)
        }

        fn face_queries(
            &self,
        ) -> Option<&dyn crate::surface::surface_cache::face_context::FaceQueries<H>> {
            None
        }

        fn line_passes(&self) -> Option<&RefCell<certified_line::LinePasses>> {
            self.0.as_ref()
        }
    }

    /// A solve whose context keeps its operation's searches (`certified_line`, "Reuse within
    /// an operation") searches once and takes the kept search when the same solve comes
    /// again; both answers, side records and contacts included, are the answer of a solve
    /// whose context keeps none. The line is the torus's near its crown: four crossings.
    #[test]
    fn a_repeated_solve_takes_its_kept_search_and_the_same_answer() {
        // (the memos on whatever the build's features: the reuse is what is tested)
        let k = Kernel {
            memoize_curve_caches: true,
            ..Kernel::default()
        };
        let s = torus(&k);
        let p = Vec3::new(82.388_988_497_226_59, 82.388_988_497_226_59, 10.000_04);
        let d = Vec3::new(
            -0.850_300_871_283_819_5,
            -0.523_935_376_527_776_1,
            -0.049_801_099_552_871_67,
        );
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let solve = |context: &mut KeepingSearches| {
            s.global_line_intersect_with_context::<H>(
                &k,
                Some(H(0)),
                None,
                context,
                &domain,
                p,
                d,
                None,
                false,
                2.0e-5,
            )
            .unwrap()
        };
        let alone = solve(&mut KeepingSearches(None));
        assert_eq!(alone.len(), 4, "{alone:?}");
        assert_eq!(k.work_counters.line_searches_reused(), 0);
        let mut keeping = KeepingSearches(Some(RefCell::default()));
        let searches = k.work_counters.line_solves();
        let first = solve(&mut keeping);
        assert_eq!(k.work_counters.line_solves(), searches + 1);
        assert_eq!(k.work_counters.line_searches_reused(), 0);
        let again = solve(&mut keeping);
        assert_eq!(
            k.work_counters.line_solves(),
            searches + 1,
            "searched again"
        );
        assert_eq!(k.work_counters.line_searches_reused(), 1);
        assert_eq!(first, alone);
        assert_eq!(again, alone);
        assert_eq!(again.side_record(), alone.side_record());
        // a solve on other inputs (a tolerance one rounding wider) finds nothing kept: it
        // searches, and answers as a solve whose context keeps nothing
        let wider = f64::from_bits(2.0e-5_f64.to_bits() + 1);
        let other = |context: &mut KeepingSearches| {
            s.global_line_intersect_with_context::<H>(
                &k,
                Some(H(0)),
                None,
                context,
                &domain,
                p,
                d,
                None,
                false,
                wider,
            )
            .unwrap()
        };
        let other_alone = other(&mut KeepingSearches(None));
        let other_kept = other(&mut keeping);
        assert_eq!(k.work_counters.line_searches_reused(), 1);
        assert_eq!(other_kept, other_alone);
    }
}
