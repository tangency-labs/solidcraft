//! The certified pass of the line/surface leaf solve: every transversal crossing of a line
//! with one leaf of a surface whose net is the surface, found with a proof that none is
//! left in the boxes it decides.
//!
//! Not upstream. The leaf solve of `SmLSIGlobalSolver::LocalSolve`
//! ([`super::p2::LsiGlobalSolver::local_solve_branch`]) solves a few guesses by Newton's
//! method and decides from the patch's normal map whether a second crossing can exist. That
//! map is no proof (a screw strip has no normal perpendicular to its axis, yet a line
//! parallel to the axis crosses it once a turn), and a guess that settles on one of two
//! crossings of a leaf loses the other: a ray that enters and leaves a torus near its crown
//! within one leaf was given its exit as the first hit. The guesses stay as they are (they
//! give today's numbers for the roots they find); this pass runs after them and adds what
//! they left.
//!
//! The method is interval Newton on `F(u, v, t) = S(u, v) − P − t D` over a scratch
//! subdivision of the leaf's parameter box (Toth, "On ray tracing parametric surfaces",
//! SIGGRAPH 1985; the tests are Krawczyk's operator as Moore and Rump state them: Rump,
//! "Verification methods", Acta Numerica 19 (2010), Thm 13.3, and
//! `docs/definitions-and-certificates.md` §4.3). Nothing of the surface cache is changed:
//! the boxes are plain parameter rectangles of the surface's own net
//! ([`crate::geom::nurbs_enclosure`]), enclosed in rounded intervals. For each box, in this
//! order:
//!
//! 0. **Cut at a knot line** inside it, so that every box enclosed lies on one polynomial
//!    piece of the net (a box whose enclosure is refused, weights not proved positive over
//!    it, is halved).
//! 1. **Excluded by distance** when its enclosure proves every point of the surface over it
//!    farther than the tolerance from the line. The tolerance is the leaf solve's own: a
//!    line passing within it is a hit there (`SmSurface::LocalCurveIntersect`), so a box is
//!    never dropped for merely missing the line. **Excluded by t** when its line parameters
//!    (the feet of its points, widened by the tolerance) lie wholly outside the line's own
//!    interval: the solve's line interval, and for a ray the parameters from just behind its
//!    start (the bound a ray's hit is skipped by). With neither, nothing is excluded by t.
//!    The leaf's interval is a slab test of rounded boxes, no proof, and excludes nothing.
//! 2. **Regular** when the contraction `q = ‖I − Y F′(X)‖` of Krawczyk's operator over the
//!    box is below 1: every matrix of the interval Jacobian is then nonsingular, `F` is
//!    injective on the box (`F(a) − F(b) = A (a − b)` with each row of `A` a gradient at its
//!    own point of the box, so `A` lies in the hull: the reason a normal map at single points
//!    proves nothing), and the box holds at most one root, with any line parameter (`F′`
//!    does not depend on `t`). Krawczyk's operator then decides: its image apart from the
//!    box, **no root**; strictly inside, exactly one. A root on the box's border passes
//!    neither test from inside: it is certified in a small box about it
//!    ([`crate::geom::krawczyk::krawczyk_region`], epsilon-inflation), and the box is
//!    decided once the hull of the two is regular too. A box with no root holds no
//!    interior local minimum of the gap to the line either (`[S_u S_v −D]` would be singular
//!    there), so the least gap over it lies on its border: it ends the search for roots, but
//!    it is no proof that the line passes farther than the tolerance.
//! 3. **Left** when it is neither and is at the floor: its image is no larger than the
//!    tolerance, it cannot be halved, or the cutting stops there because the line runs along
//!    the surface or touches it in a degenerate contact. That last is a box over which the
//!    surface lies within the tolerance of a plane through the line and the system of the
//!    line's fold on the surface, `f₁ = 0`, `∂(f₁, f₂)/∂(u, v) = 0` (`f₁`, `f₂` the
//!    distances from two perpendicular planes through the line, the first holding the
//!    surface normal at the box's middle), has no regular interval Jacobian over the box. A
//!    non-degenerate isolated contact (a tangent contact whose fold is regular), a near miss
//!    or the stretch between the two crossings of a shallow secant has one (the fold is a
//!    point there), and such a box is halved on, so that two crossings a hair apart are
//!    still certified one by one; along a line lying in the surface, or beside it, the
//!    determinant vanishes identically and no halving ever would decide the box. So does a
//!    degenerate isolated contact (a line tangent to a torus's crown circle: the
//!    determinant vanishes on two curves crossing at the contact), so a box left is no proof
//!    that the line lies in the surface. A box left holds what Krawczyk's test cannot decide
//!    (a tangent contact, a pole, an apex, a line in the surface); this pass reports nothing
//!    for it, and the guesses' answer for it stands as it was.
//! 4. **Halved** otherwise, across the parameter direction that carries the surface farther
//!    from the line over the box (the larger of `|S_u × D| Δu` and `|S_v × D| Δv`): seen
//!    along the line the halves are then about as wide as they are long, which is what
//!    makes a box regular, and a direction the line runs along is not cut for nothing. A
//!    direction in which the box's image is already within half the tolerance is at the
//!    floor's size and is not cut again (both there: step 3).
//!
//! A certified root is refined by Newton's method inside the box it is the only root of,
//! and reported only when it lies on the leaf's line interval and the gap `|S − L|` at the
//! reported point is itself enclosed within the tolerance; otherwise it is unshown.
//!
//! **What the pass decided.** Each pass ends in a [`Stop`]: not run (a line or domain
//! nothing can be proved of), finished (every box decided, excluded or left), or at the
//! limit. It records every box it did not exclude by distance as a [`FloorBox`]: its
//! `(u, v)` box, its line parameters and a [`FloorKind`] (excluded by t, left, no root,
//! edge, root, unshown, or unvisited: the boxes still to visit when the pass stopped at the
//! limit). An edge box is a box with no root one of whose sides lies on a border where a
//! path down the gap to the line can end without a root: the net's knot range (a periodic
//! seam among it: nothing wraps across the seam, which loses answers there, never a
//! contact), the solve's `(u, v)` domain, an interior knot line of multiplicity at least
//! the degree (the net below `C¹` in parameter there, a crease not proved `G¹`), or a
//! stretch of the leaf's border that no searched leaf or culled strip covers
//! ([`LeafSides`]: a leaf outside the face, which gets no distance verdict). A
//! line passing within the tolerance just beyond such a border, with no root on either
//! side, leaves only boxes with no root about it; their record is what keeps that contact.
//! A box that is no edge and has no root needs a neighbour's record for its contact. A box
//! the pass did not visit carries its line parameters too (the whole line where they cannot
//! be enclosed), so that a ray can tell whether it lies before a hit.
//!
//! **The exclusion phase.** After a finished pass (only there: the records of a leaf say
//! that a box is excluded by distance by its absence, which an unfinished leaf does not
//! prove), a phase with its own budget ([`MAX_EXCLUSION_BOXES`] enclosures) cuts the boxes
//! with no root, plain and edge, as step 3 cuts, down to their floor (a direction is not cut
//! once its image is within half the tolerance), and drops each piece its enclosure proves
//! farther than the tolerance (step 1). Pieces of a regular box with no root are regular and
//! hold no root. Soundness rests on one invariant: nothing leaves the records without a
//! proof that the surface over it is farther than the tolerance from the line; a box with
//! no root holds no root, so keeping it never loses a contact. Running out of the budget
//! keeps what the phase has not decided; it never makes the leaf stop at the limit.
//!
//! **Culled strips.** The traversal culls a leaf by the line's distance or parameter (its
//! box grown by the tolerance, its pseudo box, the overlap of the two, a ray's start, the
//! line's interval) and prunes a subtree by its sphere bound: slab and sphere tests of
//! rounded boxes, no proof. The domain of each (met with the solve's) is a strip that
//! [`enclose_strip`] runs step 1 on, halving in a work list with its own budget
//! ([`MAX_STRIP_BOXES`] enclosures): a box is excluded by distance, excluded by t (a record),
//! or kept undecided ([`FloorKind::Culled`], with its line parameters, the whole line where
//! they cannot be enclosed) at the floor's size or where it cannot be enclosed; out of the
//! budget, the boxes left to visit are kept undecided and the strip stops at its limit. A
//! strip records every box it does not exclude by distance, as a pass does, so it reads as a
//! searched leaf with no roots: one all excluded by distance proves that no contact is
//! there. A leaf outside the face (its trim classification) gets no distance verdict and no
//! strip: a stretch of a leaf's border facing it is a boundary.
//!
//! **The drop.** After every leaf's pass and every strip, one step over the whole solve
//! ([`drop_fenced`]) drops sets of boxes with no root proved farther than the tolerance.
//! Below, a box is *excluded* when no record holds it in a leaf or strip whose pass finished
//! or stopped at its limit and whose records the solve kept: such a pass records every box
//! it did not exclude by distance. A box excluded by t is a record and does not count (`g`,
//! the gap to the infinite line, is not bounded by a t verdict). A set `R` of regular boxes
//! with no root, plain or edge, is dropped whole when
//!
//! 1. every boundary side of a member (the surface's, the solve's, a stretch facing nothing
//!    searched) is shown farther than the tolerance by an enclosure of that side;
//! 2. every crease side of a member is shown farther by its own enclosure, or lies in closed
//!    excluded boxes (no other member meets it, and what lies across it is read as below);
//! 3. every other point of `R`'s outer border lies in a closed excluded box: no record but a
//!    member meets a member, corners included, and every leaf across a member's side on its
//!    leaf's border is read as above (a leaf whose pass did not run, or whose records were
//!    not kept, proves nothing, and keeps the set).
//!
//! `R` is compact, so `g` has a least value over it, at some `p`. On a boundary or crease
//! side, 1 or 2 gives `g(p) > tol`, and on `R`'s outer border 3 does. Otherwise every box
//! about `p` is a member, so regular, and `S` is `C¹` there (step 0 cuts at every knot, and a
//! side that is no crease has knot multiplicity below the degree, for a rational net too);
//! `g > 0`, since no member holds a root; so `p` would be a critical point of `g`, with
//! `S − L(t)` orthogonal to `S_u`, `S_v` and `D`, which a regular box rules out. The sets are
//! the connected sets of such records (closed boxes meeting, across leaf borders too), each
//! taken whole, so no set touches another and none waits for another to drop: the release
//! order is a single round, decided on the records as they were before any drop, so no leaf
//! drops by reading the final records of a leaf that reads its own. A seam is a boundary
//! side (nothing wraps across it). The side enclosures have their own budget
//! ([`MAX_DROP_ENCLOSURES`]); past it, a set not yet shown is kept. A box left at the floor
//! is never a member: the argument needs a regular box.
//!
//! **Components.** After the drop, the kept boxes of every searched leaf and strip (left,
//! edge, plain with no root, root, unshown, unvisited, culled; not those excluded by t) and
//! the whole domain of every leaf whose pass did not run make the solve's [`components`]:
//! boxes join where their closed `(u, v)` boxes meet (across leaf borders too; not across a
//! seam) and their line parameters overlap. A component records its boxes, its `(u, v)` and
//! line parameter hulls, whether it reaches an end of the line's own interval, whether it
//! lies in or touches a leaf at the limit, a leaf not run or a stretch nothing searched, and
//! the certified roots in it. Each is undecided until the typing of components. The solve's
//! [`side_record`] ([`SideRecord`], owned by its solution array) is not run when no solved
//! leaf's pass ran, whatever the strips found (the strips are then not enclosed), or, with
//! every leaf culled, when no strip ran; complete when no component is left and every cull
//! has its strip; and undecided
//! otherwise, over each component's line parameters and over the whole line's interval
//! where a cull's strip could not be placed. Nothing reads the records yet.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`LineNet`], [`isolate`], [`PassLine`], [`CertifiedRoot`], [`Isolation`], [`Stop`], [`FloorBox`], [`FloorKind`], [`Exclusion`], [`LeafSides`], [`leaf_sides`], [`enclose_strip`], [`SearchedLeaf`], [`drop_fenced`], [`Drops`], [`components`], [`Components`], [`Component`], [`side_record`] | (not upstream) |
//!
//! Deviations: none (no upstream counterpart). Bounds: no recursion (work lists); a box is
//! halved until its middle is one of its ends; at most [`MAX_SCRATCH_BOXES`] boxes a leaf,
//! past which the pass stops for that leaf and reports what it has certified (each root
//! stands on its own proof), and at most [`MAX_EXCLUSION_BOXES`] enclosures in its
//! exclusion phase, so a leaf's records are at most the two together and the boxes left to
//! visit; at most [`MAX_STRIP_BOXES`] enclosures a strip; at most
//! [`crate::geom::krawczyk::KRAWCZYK_STEPS`] Krawczyk steps a box; at most
//! [`MAX_MEETING_TESTS`] pairs of boxes tested in one sweep, [`MAX_SOLVE_BOXES`] boxes in
//! one solve's drop and components, past which nothing is dropped, the components are not
//! built and the whole line is undecided, and [`MAX_DROP_ENCLOSURES`] side enclosures in
//! one solve's drop.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::blend::fillet_section::interval::{cmax, cmin, round_down, round_up, Ivl};
use crate::blend::fillet_section::series::V3;
use crate::blend::fillet_section::station::Nurbs;
use crate::error::Result;
use crate::geom::extent1d::Extent1d;
use crate::geom::extent2d::Extent2d;
use crate::geom::krawczyk::{
    contraction, inverse_of_columns2, inverse_of_columns3, k_operator, krawczyk_region, newton,
    Linearised, KRAWCZYK_STEPS,
};
use crate::geom::nurbs_enclosure::nurbs_partials_over_box_about;
use crate::geom::solution_array::{Contact, SideRecord};
use crate::kernel::{CancelToken, Kernel};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
use crate::surface::surface::Surface;

const OP: &str = "surface::surface::certified_line::isolate";

/// The most enclosures one leaf's exclusion phase makes (module docs, "The exclusion
/// phase"): the root pass's own bound, so that a leaf's records stay within the two
/// bounds together and the boxes left to visit.
pub(crate) const MAX_EXCLUSION_BOXES: usize = MAX_SCRATCH_BOXES;

/// The most boxes one leaf's pass encloses. A leaf with a transversal crossing takes some
/// tens (the issue's torus leaf with two crossings at 2° and 2.5°: 63); the most measured
/// over the kernel's goldens and cases is recorded with the pull request that set this,
/// and the bound is several times that. Past it the pass stops for the leaf.
pub(crate) const MAX_SCRATCH_BOXES: usize = 1 << 12;

/// The most enclosures one culled strip makes (module docs, "Culled strips"): a leaf's own
/// bound, since a strip is a leaf, or a subtree of leaves, the traversal culled.
pub(crate) const MAX_STRIP_BOXES: usize = MAX_SCRATCH_BOXES;

/// The most side enclosures one solve's drop makes (module docs, "The drop"): one for each
/// of the most records the drop reads.
pub(crate) const MAX_DROP_ENCLOSURES: usize = MAX_SOLVE_BOXES;

/// Three intervals: a vector's components, or a box in `(u, v, t)`.
type I3 = [Ivl; 3];

/// The exact net of a surface the certified pass runs on, with its parameter range.
#[derive(Debug, Clone)]
pub(crate) struct LineNet {
    net: Nurbs,
    /// The knots' range in u and in v.
    range: [[f64; 2]; 2],
    /// The interior knots of multiplicity at least the degree, in u and in v: the knot lines
    /// across which the net is below `C¹` in parameter (each once, ascending).
    creases: [Vec<f64>; 2],
}

impl LineNet {
    /// The net of `surface` when its net is the surface (the kinds of
    /// [`Surface::certified_bounding_box`], B-splines in power-basis form apart) and the
    /// enclosures take it (positive weights, knots and points that agree); `None` for any
    /// other surface, which keeps the guesses alone.
    ///
    /// # Errors
    ///
    /// Those of making a missing NURB.
    pub(crate) fn of(k: &Kernel, surface: &Surface) -> Result<Option<Self>> {
        let Some(base) = surface.certified_net(k)? else {
            return Ok(None);
        };
        if base.form() == BSplineSurfaceForm::Polynomial {
            return Ok(None);
        }
        let Ok(net) = Nurbs::of(&base) else {
            return Ok(None);
        };
        let ends = |knots: &[f64], p: usize, n: usize| match (knots.get(p), knots.get(n)) {
            (Some(&a), Some(&b)) if a < b && a.is_finite() && b.is_finite() => Some([a, b]),
            _ => None,
        };
        let (Some(u), Some(v)) = (ends(&net.u, net.p, net.nu), ends(&net.v, net.q, net.nv)) else {
            return Ok(None);
        };
        let creases = [creases(&net.u, net.p, u), creases(&net.v, net.q, v)];
        Ok(Some(Self {
            net,
            range: [u, v],
            creases,
        }))
    }

    /// Whether a side of the `(u, v)` box `b` lies on a border where a path down the gap to
    /// the line can end without a root (module docs, "What the pass decided").
    fn on_a_border(&self, solve: &Extent2d, sides: &LeafSides, b: Scratch) -> bool {
        self.side_kinds(solve, sides, b)
            .iter()
            .any(|&k| k != SideKind::Plain)
    }

    /// What each side of the `(u, v)` box `b` lies on, in the order u = `b.u.lo`, u =
    /// `b.u.hi`, v = `b.v.lo`, v = `b.v.hi`: a boundary (the knots' range, a periodic seam
    /// among them since nothing wraps across it; the solve's `(u, v)` domain `solve`, on it
    /// or past it, as a leaf is trimmed to it but a leaf within a rounding of it is kept
    /// whole; a stretch of the leaf's border that no searched leaf or strip covers), a
    /// crease knot line (step 0 cuts boxes exactly there), or neither.
    fn side_kinds(&self, solve: &Extent2d, sides: &LeafSides, b: Scratch) -> [SideKind; 4] {
        let [[u0, u1], [v0, v1]] = self.range;
        let [cu, cv] = &self.creases;
        let (lo, hi) = (solve.min(), solve.max());
        let unsearched = sides.faces_unsearched(b);
        let kind = |boundary: bool, crease: bool, unsearched: bool| {
            if boundary || unsearched {
                SideKind::Boundary
            } else if crease {
                SideKind::Crease
            } else {
                SideKind::Plain
            }
        };
        let on_u = |x: f64| x <= u0 || x >= u1 || x <= lo.x || x >= hi.x;
        let on_v = |y: f64| y <= v0 || y >= v1 || y <= lo.y || y >= hi.y;
        let [c0, c1, c2, c3] = unsearched;
        [
            kind(on_u(b.u.lo), cu.contains(&b.u.lo), c0),
            kind(on_u(b.u.hi), cu.contains(&b.u.hi), c1),
            kind(on_v(b.v.lo), cv.contains(&b.v.lo), c2),
            kind(on_v(b.v.hi), cv.contains(&b.v.hi), c3),
        ]
    }

    /// The first knot of the net strictly inside `u` (in u) or, with none there, strictly
    /// inside `v` (in v): whether it is in u, and the knot.
    fn knot_inside(&self, u: Ivl, v: Ivl) -> Option<(bool, f64)> {
        let inside = |knots: &[f64], x: Ivl| knots.iter().copied().find(|&k| k > x.lo && k < x.hi);
        inside(&self.net.u, u)
            .map(|k| (true, k))
            .or_else(|| inside(&self.net.v, v).map(|k| (false, k)))
    }

    /// Whether `[u.lo, u.hi] x [v.lo, v.hi]` lies on the knots' range.
    fn holds(&self, u: Ivl, v: Ivl) -> bool {
        let [[u0, u1], [v0, v1]] = self.range;
        u.lo >= u0 && u.hi <= u1 && v.lo >= v0 && v.hi <= v1 && u.lo <= u.hi && v.lo <= v.hi
    }

    /// `S − about`, `S_u` and `S_v` over the box, enclosed; `None` where the box leaves the
    /// knots' range or the enclosure is refused (a break of the surface inside the box,
    /// weights not proved positive over it).
    fn over(&self, u: Ivl, v: Ivl, about: [f64; 3]) -> Option<[I3; 3]> {
        if !self.holds(u, v) {
            return None;
        }
        // continuity 0: a box may cross a knot line the surface is continuous across; the
        // hull of the pieces' partials then holds each one-sided partial, which is what the
        // mean-value form needs (`geom::krawczyk`, module docs)
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 1, 0, about).ok()?;
        let out = [i3(p.at(0, 0)), i3(p.at(1, 0)), i3(p.at(0, 1))];
        out.iter().flatten().all(|x| x.is_finite()).then_some(out)
    }

    /// The second partials `S_uu`, `S_uv` and `S_vv` over the box, enclosed; `None` as
    /// [`LineNet::over`], and where the box crosses a knot line the surface is not `C¹`
    /// across (the fold's system reads the first partials as a continuous function).
    fn second(&self, u: Ivl, v: Ivl, about: [f64; 3]) -> Option<[I3; 3]> {
        if !self.holds(u, v) {
            return None;
        }
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 2, 1, about).ok()?;
        let out = [i3(p.at(2, 0)), i3(p.at(1, 1)), i3(p.at(0, 2))];
        out.iter().flatten().all(|x| x.is_finite()).then_some(out)
    }

    /// `S − about` over the box, enclosed, without its partials; `None` as
    /// [`LineNet::over`].
    fn value_over(&self, u: Ivl, v: Ivl, about: [f64; 3]) -> Option<I3> {
        if !self.holds(u, v) {
            return None;
        }
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 0, 0, about).ok()?;
        let out = i3(p.at(0, 0));
        out.iter().all(|x| x.is_finite()).then_some(out)
    }

    /// `S − about` at the point `(u, v)`, enclosed.
    fn at(&self, u: f64, v: f64, about: [f64; 3]) -> Option<I3> {
        let (u, v) = (Ivl::from(u), Ivl::from(v));
        if !self.holds(u, v) {
            return None;
        }
        let p =
            nurbs_partials_over_box_about(&self.net, u.lo, u.hi, v.lo, v.hi, 0, 0, about).ok()?;
        let out = i3(p.at(0, 0));
        out.iter().all(|x| x.is_finite()).then_some(out)
    }
}

/// The knots of `knots` (degree `degree`) strictly inside `range` whose multiplicity is at
/// least the degree, each once: the net is `C^(degree − multiplicity)` across a knot, so
/// below `C¹` there.
fn creases(knots: &[f64], degree: usize, [lo, hi]: [f64; 2]) -> Vec<f64> {
    let mut out: Vec<f64> = Vec::new();
    let mut i = 0;
    while let Some(&k) = knots.get(i) {
        let run = knots
            .get(i..)
            .map_or(0, |rest| rest.iter().take_while(|&&x| x == k).count());
        if k > lo && k < hi && run >= degree {
            out.push(k);
        }
        i += run.max(1);
    }
    out
}

/// What a side of a box lies on ([`LineNet::side_kinds`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum SideKind {
    /// The surface's or the solve's boundary, or a stretch of the leaf's border that no
    /// searched leaf or strip covers: nothing across it was searched.
    Boundary,
    /// A crease knot line (the net below `C¹` in parameter across it).
    Crease,
    /// Neither.
    Plain,
}

/// What lies across the four sides of one searched leaf's (or culled strip's) domain
/// (module docs, "What the pass decided"): for each side, the other searched leaves and
/// strips across it and the stretch of it each covers. A stretch of a side inside the
/// solve's domain that none covers faces a leaf outside the face, which gets no distance
/// verdict: a box side on it counts as a boundary.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct LeafSides {
    /// The leaf's domain, `[[u0, u1], [v0, v1]]`.
    bounds: [[f64; 2]; 2],
    /// Whether each side (u = u0, u = u1, v = v0, v = v1) lies inside the solve's domain.
    inside: [bool; 4],
    /// For each side, the leaves across it with the stretch each covers along it (in v for
    /// the sides of constant u, in u for the others), by the stretch's start.
    across: [Vec<(usize, [f64; 2])>; 4],
}

impl LeafSides {
    /// A leaf that is the whole solve: every side lies on the solve's border.
    #[cfg(test)]
    pub(crate) fn whole_solve(domain: &Extent2d) -> Self {
        Self {
            bounds: bounds_of(domain),
            inside: [false; 4],
            across: Default::default(),
        }
    }

    /// For each side of the box `b` (in the order of [`LineNet::side_kinds`]): whether it
    /// lies on the leaf's border inside the solve's domain and some of it faces no other
    /// searched leaf or strip.
    fn faces_unsearched(&self, b: Scratch) -> [bool; 4] {
        let ranges = side_ranges(b);
        core::array::from_fn(|s| {
            let on = self.on_side(b, s);
            match (on, self.inside.get(s), self.across.get(s), ranges.get(s)) {
                (true, Some(true), Some(across), Some(&range)) => !covered(across, range),
                _ => false,
            }
        })
    }

    /// Whether the side `s` of the box `b` lies on the same side of the leaf's border.
    fn on_side(&self, b: Scratch, s: usize) -> bool {
        let [[u0, u1], [v0, v1]] = self.bounds;
        match s {
            0 => b.u.lo <= u0,
            1 => b.u.hi >= u1,
            2 => b.v.lo <= v0,
            _ => b.v.hi >= v1,
        }
    }

    /// The leaves across the side `s` of the box `b` (on the leaf's border there) that
    /// meet it, ends included.
    fn leaves_across(&self, b: Scratch, s: usize) -> impl Iterator<Item = usize> + '_ {
        let range = side_ranges(b).get(s).copied();
        let on = self.on_side(b, s);
        self.across
            .get(s)
            .into_iter()
            .flatten()
            .filter(move |(_, [c0, c1])| on && range.is_some_and(|[a, z]| *c0 <= z && *c1 >= a))
            .map(|&(j, _)| j)
    }
}

/// `[[u0, u1], [v0, v1]]` of a domain.
fn bounds_of(d: &Extent2d) -> [[f64; 2]; 2] {
    let (lo, hi) = (d.min(), d.max());
    [[lo.x, hi.x], [lo.y, hi.y]]
}

/// The stretch each side of `b` spans along it (in v for the sides of constant u).
fn side_ranges(b: Scratch) -> [[f64; 2]; 4] {
    let (u, v) = ([b.u.lo, b.u.hi], [b.v.lo, b.v.hi]);
    [v, v, u, u]
}

/// Whether the stretches `across` (by their start) cover `[a, z]`.
fn covered(across: &[(usize, [f64; 2])], [a, z]: [f64; 2]) -> bool {
    let mut reach = a;
    for &(_, [c0, c1]) in across {
        if reach >= z {
            break;
        }
        if c0 > reach {
            return false;
        }
        if c1 > reach {
            reach = c1;
        }
    }
    reach >= z
}

/// What lies across each searched leaf's sides, from the domains of every leaf the
/// traversal searched (`domains`) and the solve's `(u, v)` domain: two leaves are across a
/// side from each other when that side's line is the same number in both and their
/// stretches along it meet.
pub(crate) fn leaf_sides(domains: &[Extent2d], solve: &Extent2d) -> Vec<LeafSides> {
    let all: Vec<[[f64; 2]; 2]> = domains.iter().map(bounds_of).collect();
    // each leaf's sides by their line: u1, u0, v1, v0 (what a side u = u0 meets is a u1)
    let keyed = |pick: fn(&[[f64; 2]; 2]) -> f64| {
        let mut k: Vec<(f64, usize)> = all.iter().enumerate().map(|(i, b)| (pick(b), i)).collect();
        k.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.cmp(&b.1)));
        k
    };
    let by: [Vec<(f64, usize)>; 4] = [
        keyed(|&[[_, u1], _]| u1),
        keyed(|&[[u0, _], _]| u0),
        keyed(|&[_, [_, v1]]| v1),
        keyed(|&[_, [v0, _]]| v0),
    ];
    let (lo, hi) = (solve.min(), solve.max());
    all.iter()
        .enumerate()
        .map(|(i, &bounds)| {
            let [[u0, u1], [v0, v1]] = bounds;
            let lines = [u0, u1, v0, v1];
            let mut across: [Vec<(usize, [f64; 2])>; 4] = Default::default();
            for (s, slot) in across.iter_mut().enumerate() {
                let (Some(&line), Some(keys)) = (lines.get(s), by.get(s)) else {
                    continue;
                };
                let first = keys.partition_point(|&(x, _)| x.total_cmp(&line).is_lt());
                let same = keys.get(first..).unwrap_or_default();
                for &(x, j) in same {
                    if x.total_cmp(&line).is_ne() {
                        break;
                    }
                    let Some(&[[a0, a1], [c0, c1]]) = all.get(j) else {
                        continue;
                    };
                    let ([b0, b1], [o0, o1]) = if s < 2 {
                        ([c0, c1], [v0, v1])
                    } else {
                        ([a0, a1], [u0, u1])
                    };
                    // (a domain with no area covers no stretch: nothing was searched there)
                    let area = a0 < a1 && c0 < c1;
                    if j != i && area && b0 <= o1 && b1 >= o0 {
                        slot.push((j, [b0, b1]));
                    }
                }
                slot.sort_by(|(i, [a, _]), (j, [b, _])| a.total_cmp(b).then(i.cmp(j)));
            }
            LeafSides {
                bounds,
                inside: [u0 > lo.x, u1 < hi.x, v0 > lo.y, v1 < hi.y],
                across,
            }
        })
        .collect()
}

/// The components of a vector of intervals.
fn i3(v: V3<Ivl>) -> I3 {
    [v.x, v.y, v.z]
}

/// The components of a vector.
fn f3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// The vector of three components.
fn v3(a: [f64; 3]) -> Vec3 {
    let [x, y, z] = a;
    Vec3::new(x, y, z)
}

/// `a · b`, enclosed.
fn dot(a: [f64; 3], b: &I3) -> Ivl {
    a.iter()
        .zip(b)
        .fold(Ivl::from(0.0), |s, (&x, &y)| s + Ivl::from(x) * y)
}

/// `a × b`, enclosed.
fn cross(a: [f64; 3], b: &I3) -> I3 {
    let [ax, ay, az] = a.map(Ivl::from);
    let [bx, by, bz] = *b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// `a + b`, enclosed.
fn add(a: &I3, b: &I3) -> I3 {
    let [a0, a1, a2] = *a;
    let [b0, b1, b2] = *b;
    [a0 + b0, a1 + b1, a2 + b2]
}

/// `a − t d`, enclosed.
fn less(a: &I3, t: Ivl, d: [f64; 3]) -> I3 {
    let [a0, a1, a2] = *a;
    let [d0, d1, d2] = d.map(Ivl::from);
    [a0 - t * d0, a1 - t * d1, a2 - t * d2]
}

/// The squares of the least and of the greatest length a vector of `v` can have, each
/// rounded outward.
fn length_squared(v: &I3) -> Ivl {
    let sum = |f: fn(Ivl) -> f64| {
        v.iter().fold(Ivl::from(0.0), |s, &x| {
            let m = Ivl::from(f(x));
            s + m * m
        })
    };
    Ivl::new(sum(Ivl::mig).lo, sum(Ivl::mag).hi)
}

/// The middles of a vector of intervals.
fn mids(v: &I3) -> [f64; 3] {
    v.map(Ivl::mid)
}

/// `a` strictly inside `b`, component by component (a NaN end is never inside).
fn inside(a: &I3, b: &I3) -> bool {
    a.iter().zip(b).all(|(x, y)| x.lo > y.lo && x.hi < y.hi)
}

/// `a` apart from `b` in some component (a NaN end is never apart).
fn apart(a: &I3, b: &I3) -> bool {
    a.iter().zip(b).any(|(x, y)| x.hi < y.lo || x.lo > y.hi)
}

/// `a` within `b`, ends included.
fn within(a: Ivl, b: Ivl) -> bool {
    a.lo >= b.lo && a.hi <= b.hi
}

/// The common part of `a` and `b`, `None` where they are apart.
fn meet(a: Ivl, b: Ivl) -> Option<Ivl> {
    let (lo, hi) = (a.lo.max(b.lo), a.hi.min(b.hi));
    (lo <= hi).then_some(Ivl::new(lo, hi))
}

/// The line `P + t D` of a pass, with what its tests read.
#[derive(Debug, Clone, Copy)]
struct ScratchLine {
    p: [f64; 3],
    d: [f64; 3],
    /// `|D|²`, enclosed.
    d2: Ivl,
    /// The leaf's line interval: a root off it is unshown.
    interval: [f64; 2],
    /// The line's own interval ([`PassLine::span`]): a box off it is excluded by t.
    span: [f64; 2],
    /// The tolerance as a distance, and as a stretch of the line's parameter (rounded up).
    tol: f64,
    tol_t: f64,
    /// `(tol |D|)²`, rounded up.
    tol_cross2: f64,
    /// The solve's `(u, v)` domain ([`PassLine::solve_domain`]).
    solve_domain: Extent2d,
}

impl ScratchLine {
    /// The line of `line` with the leaf's `interval`; `None` where nothing can be proved of
    /// it (no tolerance, a line of no length, an empty interval).
    fn of(line: &PassLine, interval: &Extent1d) -> Option<Self> {
        let tol = line.tol;
        let d = f3(line.vector);
        let d2 = length_squared(&d.map(Ivl::from));
        let usable = tol > 0.0 && tol.is_finite() && d2.lo > 0.0 && d2.is_finite();
        let [s0, s1] = line.span;
        if !usable || !(interval.min() <= interval.max()) || !(s0 <= s1) {
            return None;
        }
        let tol_i = Ivl::from(tol);
        Some(Self {
            p: f3(line.point),
            d,
            d2,
            interval: [interval.min(), interval.max()],
            span: line.span,
            tol,
            tol_t: (tol_i / d2.sqrt()).hi,
            tol_cross2: (tol_i * tol_i * d2).hi,
            solve_domain: line.solve_domain,
        })
    }

    /// Whether the points `r + P` are all farther than the tolerance from the line.
    fn farther(&self, r: &I3) -> bool {
        length_squared(&cross(self.d, r)).lo > self.tol_cross2
    }

    /// The line parameters of the feet of the points `r + P` widened by the tolerance:
    /// as Krawczyk's steps start from them, and rounded outward (the enclosure the
    /// verdicts read).
    fn feet(&self, r: &I3) -> (Ivl, Ivl) {
        let foot = self.parameter(r);
        (
            Ivl::new(foot.lo - self.tol_t, foot.hi + self.tol_t),
            Ivl::new(
                round_down(foot.lo - self.tol_t),
                round_up(foot.hi + self.tol_t),
            ),
        )
    }

    /// `S − P` from an enclosure of `S − about`.
    fn off_the_start(&self, s: &I3, about: [f64; 3]) -> I3 {
        let [a0, a1, a2] = about;
        let [p0, p1, p2] = self.p;
        let shift = [
            Ivl::from(a0) - Ivl::from(p0),
            Ivl::from(a1) - Ivl::from(p1),
            Ivl::from(a2) - Ivl::from(p2),
        ];
        add(s, &shift)
    }

    /// The line parameters of the feet of the points `r + P`, enclosed.
    fn parameter(&self, r: &I3) -> Ivl {
        dot(self.d, r) / self.d2
    }
}

/// A crossing of the line and the surface, proved to exist and to be the only one in a box.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct CertifiedRoot {
    /// `(u, v, t)`: Newton's root, inside [`CertifiedRoot::enclosure`].
    pub point: [f64; 3],
    /// A box that holds the root.
    pub enclosure: [Ivl; 3],
    /// The `(u, v)` box in which the root is the only one, with any line parameter.
    pub unique: [Ivl; 2],
}

/// The line every leaf's pass of one solve searches, with what bounds it.
#[derive(Debug, Clone, Copy)]
pub(crate) struct PassLine {
    /// The line's point: the line is `point + t vector`.
    pub point: Point3,
    /// The line's vector.
    pub vector: Vec3,
    /// The line's own interval of `t` (module docs, step 1): the solve's line interval
    /// where it has one, from just behind the start for a ray; an end without a bound is
    /// infinite.
    pub span: [f64; 2],
    /// The solve's `(u, v)` domain, whose border is an edge's ([`FloorKind::Edge`]).
    pub solve_domain: Extent2d,
    /// The tolerance: a line passing within it of the surface is a hit.
    pub tol: f64,
}

/// How a leaf's pass ended.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) enum Stop {
    /// Not run: nothing can be proved of the line or the domain (no tolerance, a line of no
    /// length, a domain off the net's knots, an empty interval). The guesses alone answer.
    #[default]
    NotRun,
    /// Every box was decided, excluded or left.
    Finished,
    /// Stopped at [`MAX_SCRATCH_BOXES`] with boxes still to visit ([`FloorKind::Unvisited`]).
    AtLimit,
}

/// What a box the pass did not exclude by distance is (module docs, "What the pass
/// decided").
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum FloorKind {
    /// Its line parameters lie wholly outside the line's own interval (step 1).
    ExcludedByT,
    /// Left at the floor (step 3).
    Left,
    /// No root (step 2), with a side on a border where a path down the gap can end.
    Edge,
    /// No root (step 2), and no side on such a border.
    NoRoot,
    /// Its root, certified and reported, the only one in `unique` with any line parameter.
    Root { unique: [Ivl; 2] },
    /// Its root, certified but not reported ([`Isolation::unshown`]), the only one in
    /// `unique`.
    Unshown { unique: [Ivl; 2] },
    /// Not visited: the pass stopped at [`MAX_SCRATCH_BOXES`] first.
    Unvisited,
    /// A box of a culled strip ([`enclose_strip`]) that step 1 did not exclude: at the
    /// floor's size, not to be enclosed, or still to visit when the strip's budget ran out.
    Culled,
}

/// A box the pass did not exclude by distance, as it decided it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct FloorBox {
    /// The box in u.
    pub u: Ivl,
    /// The box in v.
    pub v: Ivl,
    /// Its line parameters: the feet of its points on the line, widened by the tolerance;
    /// `None` for a box whose line parameters were never enclosed (unvisited, or left
    /// because its enclosure was refused or not finite).
    pub t: Option<Ivl>,
    /// What the pass decided of it.
    pub kind: FloorKind,
}

/// What one leaf's pass found.
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Isolation {
    /// The certified crossings, each once, in the order found.
    pub roots: Vec<CertifiedRoot>,
    /// Boxes enclosed.
    pub boxes: usize,
    /// Boxes left at the floor (module docs, step 3).
    pub left: usize,
    /// Certified roots not reported because the gap at Newton's point was not enclosed
    /// within the tolerance, or the point lies off the leaf's line interval.
    pub unshown: usize,
    /// How the pass ended.
    pub stop: Stop,
    /// Every box not excluded by distance, in the order decided, then the boxes still to
    /// visit at the limit; after a finished pass, what its exclusion phase kept (the solve's
    /// drop, [`drop_fenced`], comes after every leaf's pass).
    pub floor: Vec<FloorBox>,
    /// What the exclusion phase did (module docs, "The exclusion phase").
    pub exclusion: Exclusion,
}

/// What a leaf's exclusion phase did (module docs, "The exclusion phase").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Exclusion {
    /// Whether it ran: only after a finished root pass.
    pub ran: bool,
    /// The enclosures it made: at most its budget.
    pub boxes: usize,
    /// Whether it stopped at its budget with work left. What it had not decided is kept;
    /// the root pass stays finished.
    pub out_of_budget: bool,
    /// Pieces of boxes with no root it proved farther than the tolerance.
    pub excluded: usize,
}

/// What Krawczyk's test says of a regular box.
enum Regular {
    /// No root in the box.
    Empty,
    /// Exactly one, with its enclosure and the `(u, v)` box it is unique in.
    Root(I3, [Ivl; 2]),
    /// Not decided.
    Open,
}

/// A parameter box of the work list.
#[derive(Clone, Copy)]
struct Scratch {
    u: Ivl,
    v: Ivl,
}

/// The certified crossings of `line` with the surface of `net` over the leaf's `domain`,
/// reported for `t` in the leaf's `interval` (module docs), with what the pass decided of
/// every box; `sides` is what lies across the leaf's sides.
///
/// # Errors
///
/// A cancel.
pub(crate) fn isolate(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    interval: &Extent1d,
    sides: &LeafSides,
    cancel: Option<&CancelToken>,
) -> Result<Isolation> {
    isolate_within(
        net,
        line,
        domain,
        interval,
        sides,
        cancel,
        MAX_EXCLUSION_BOXES,
    )
}

/// [`isolate`] with the exclusion phase's budget `exclusion_budget`.
fn isolate_within(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    interval: &Extent1d,
    sides: &LeafSides,
    cancel: Option<&CancelToken>,
    exclusion_budget: usize,
) -> Result<Isolation> {
    let mut out = Isolation::default();
    let Some(line) = ScratchLine::of(line, interval) else {
        // nothing can be proved of such a line: the guesses alone answer
        return Ok(out);
    };
    let (lo, hi) = (domain.min(), domain.max());
    let (u, v) = (Ivl::new(lo.x, hi.x), Ivl::new(lo.y, hi.y));
    if !net.holds(u, v) {
        // nor of a domain off the net's knots
        return Ok(out);
    }
    let mut work = vec![Scratch { u, v }];
    out.stop = Stop::Finished;
    while let Some(b) = work.pop() {
        if out.boxes >= MAX_SCRATCH_BOXES {
            out.stop = Stop::AtLimit;
            work.push(b);
            // each with its line parameters, so that a ray can tell whether it lies
            // before a hit (the whole line where they cannot be enclosed)
            out.floor.extend(work.iter().map(|w| {
                FloorBox {
                    u: w.u,
                    v: w.v,
                    t: Some(
                        enclosed(net, &line, w.u, w.v)
                            .map(|e| line.feet(&e.r).1)
                            .filter(|feet| feet.is_finite())
                            .unwrap_or_else(Ivl::whole),
                    ),
                    kind: FloorKind::Unvisited,
                }
            }));
            break;
        }
        if out.boxes % 64 == 0 {
            if let Some(token) = cancel {
                token.check(OP)?;
            }
        }
        out.boxes += 1;
        let (kind, t) = match visit(net, &line, b) {
            Visit::ExcludedByDistance => continue,
            Visit::ExcludedByT(t) => (FloorKind::ExcludedByT, Some(t)),
            Visit::NoRoot(t) => {
                let kind = if net.on_a_border(&line.solve_domain, sides, b) {
                    FloorKind::Edge
                } else {
                    FloorKind::NoRoot
                };
                (kind, Some(t))
            }
            Visit::Root(root, t) => {
                // one root, however many boxes hold it on their borders: the same root
                // when its enclosure lies in a box another is the only root of (Rump,
                // Thm 13.3), or when the hull of the two enclosures is regular (it then
                // holds one root at most); roots with enclosures apart are two
                let known = out.roots.iter().any(|r| {
                    let [ru, rv, _] = root.enclosure;
                    let [qu, qv] = r.unique;
                    let [su, sv, _] = r.enclosure;
                    (within(ru, qu) && within(rv, qv))
                        || (meet(ru, su).is_some()
                            && meet(rv, sv).is_some()
                            && regular_over(net, &line, ru.hull(su), rv.hull(sv)))
                });
                let unique = root.unique;
                if !known {
                    out.roots.push(root);
                }
                (FloorKind::Root { unique }, Some(t))
            }
            Visit::Unshown(unique, t) => {
                out.unshown += 1;
                (FloorKind::Unshown { unique }, Some(t))
            }
            Visit::Left(t) => {
                out.left += 1;
                (FloorKind::Left, t)
            }
            Visit::Halve(first, second) => {
                work.push(second);
                work.push(first);
                continue;
            }
        };
        out.floor.push(FloorBox {
            u: b.u,
            v: b.v,
            t,
            kind,
        });
    }
    if out.stop == Stop::Finished {
        let mut phase = Exclude {
            net,
            line: &line,
            sides,
            budget: exclusion_budget,
            cancel,
            done: Exclusion {
                ran: true,
                ..Exclusion::default()
            },
        };
        phase.run(&mut out.floor)?;
        out.exclusion = phase.done;
    }
    Ok(out)
}

/// Step 1 of the pass over a culled strip, the `(u, v)` box `domain` (module docs, "Culled
/// strips"): every box excluded by distance, recorded as excluded by t, or kept undecided
/// ([`FloorKind::Culled`]). The strip has no roots and no exclusion phase; it is
/// [`Stop::Finished`] when every box was decided or kept at the floor, [`Stop::AtLimit`]
/// when its budget ran out (the boxes left to visit kept undecided, with their line
/// parameters), and [`Stop::NotRun`] where nothing can be proved of the line or the domain.
///
/// # Errors
///
/// A cancel.
pub(crate) fn enclose_strip(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    cancel: Option<&CancelToken>,
) -> Result<Isolation> {
    enclose_strip_within(net, line, domain, cancel, MAX_STRIP_BOXES)
}

/// [`enclose_strip`] with the budget `budget`.
fn enclose_strip_within(
    net: &LineNet,
    line: &PassLine,
    domain: &Extent2d,
    cancel: Option<&CancelToken>,
    budget: usize,
) -> Result<Isolation> {
    let mut out = Isolation::default();
    let [s0, s1] = line.span;
    let Some(line) = ScratchLine::of(line, &Extent1d::from_min_max(s0, s1)) else {
        return Ok(out);
    };
    let (lo, hi) = (domain.min(), domain.max());
    let (u, v) = (Ivl::new(lo.x, hi.x), Ivl::new(lo.y, hi.y));
    if !net.holds(u, v) {
        return Ok(out);
    }
    let culled = |b: Scratch, t: Option<Ivl>| FloorBox {
        u: b.u,
        v: b.v,
        t,
        kind: FloorKind::Culled,
    };
    let mut work = vec![Scratch { u, v }];
    out.stop = Stop::Finished;
    while let Some(b) = work.pop() {
        if out.boxes >= budget {
            out.stop = Stop::AtLimit;
            work.push(b);
            // each with its line parameters (the whole line where they cannot be enclosed)
            out.floor.extend(work.iter().map(|&w| {
                let t = enclosed(net, &line, w.u, w.v)
                    .map(|e| line.feet(&e.r).1)
                    .filter(|feet| feet.is_finite())
                    .unwrap_or_else(Ivl::whole);
                culled(w, Some(t))
            }));
            break;
        }
        if out.boxes % 64 == 0 {
            if let Some(token) = cancel {
                token.check(OP)?;
            }
        }
        out.boxes += 1;
        // a box that cannot be enclosed is halved across its wider side
        let wider = |t: Option<Ivl>| {
            let split_u = b.u.hi - b.u.lo >= b.v.hi - b.v.lo;
            halves(b, split_u, if split_u { b.u.mid() } else { b.v.mid() }, t)
        };
        // (step 1 reads the surface's points alone: their partials only where it halves)
        let Some(r) = enclosed_value(net, &line, b.u, b.v) else {
            match wider(None) {
                Visit::Halve(first, second) => {
                    work.push(second);
                    work.push(first);
                }
                _ => out.floor.push(culled(b, None)),
            }
            continue;
        };
        if line.farther(&r) {
            continue;
        }
        let (_, feet) = line.feet(&r);
        if !feet.is_finite() {
            out.floor.push(culled(b, None));
            continue;
        }
        if feet.hi < s0 || feet.lo > s1 {
            out.floor.push(FloorBox {
                u: b.u,
                v: b.v,
                t: Some(feet),
                kind: FloorKind::ExcludedByT,
            });
            continue;
        }
        // halved as step 4 halves, down to the floor of step 3 (across its wider side where
        // its partials cannot be enclosed)
        let halved = match enclosed(net, &line, b.u, b.v) {
            Some(e) => cuts(&line, b, &e.s_u, &e.s_v)
                .filter(|c| c.cut_u || c.cut_v)
                .map(|c| {
                    halves(
                        b,
                        c.split_u,
                        if c.split_u { b.u.mid() } else { b.v.mid() },
                        Some(feet),
                    )
                }),
            None => Some(wider(Some(feet))),
        };
        match halved {
            Some(Visit::Halve(first, second)) => {
                work.push(second);
                work.push(first);
            }
            _ => out.floor.push(culled(b, Some(feet))),
        }
    }
    Ok(out)
}

/// The exclusion phase of one leaf (module docs, "The exclusion phase").
struct Exclude<'a> {
    net: &'a LineNet,
    line: &'a ScratchLine,
    sides: &'a LeafSides,
    /// The most enclosures it makes.
    budget: usize,
    cancel: Option<&'a CancelToken>,
    /// What it did so far.
    done: Exclusion,
}

impl Exclude<'_> {
    /// Takes one enclosure from the budget; false (and out of budget) when none is left.
    fn spend(&mut self) -> Result<bool> {
        if self.done.boxes >= self.budget {
            self.done.out_of_budget = true;
            return Ok(false);
        }
        if self.done.boxes % 64 == 0 {
            if let Some(token) = self.cancel {
                token.check(OP)?;
            }
        }
        self.done.boxes += 1;
        Ok(true)
    }

    /// The phase over the records `floor` of a finished root pass: the boxes with no root
    /// cut down to the floor, each piece proved farther than the tolerance dropped.
    fn run(&mut self, floor: &mut Vec<FloorBox>) -> Result<()> {
        let (todo, mut kept): (Vec<FloorBox>, Vec<FloorBox>) = std::mem::take(floor)
            .into_iter()
            .partition(|r| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge));
        let mut todo: Vec<FloorBox> = todo.into_iter().rev().collect();
        while let Some(r) = todo.pop() {
            if !self.spend()? {
                // out of budget: what is left is kept as it is
                kept.push(r);
                kept.extend(std::mem::take(&mut todo).into_iter().rev());
                break;
            }
            match self.cut(r) {
                Cut::Excluded => self.done.excluded += 1,
                Cut::Kept(piece) => kept.push(piece),
                Cut::Halves(first, second) => {
                    todo.push(second);
                    todo.push(first);
                }
            }
        }
        *floor = kept;
        Ok(())
    }

    /// One box with no root (a piece of one: it holds no root and is regular, as the box
    /// it was cut from is): excluded by distance, kept at the floor (or where it cannot be
    /// enclosed or cut), or cut in two.
    fn cut(&self, r: FloorBox) -> Cut {
        let (net, line) = (self.net, self.line);
        let b = Scratch { u: r.u, v: r.v };
        let Some(e) = enclosed(net, line, b.u, b.v) else {
            return Cut::Kept(r);
        };
        if line.farther(&e.r) {
            return Cut::Excluded;
        }
        let (_, feet) = line.feet(&e.r);
        if !feet.is_finite() {
            return Cut::Kept(r);
        }
        let [s0, s1] = line.span;
        let kind_of = |b: Scratch| {
            if net.on_a_border(&line.solve_domain, self.sides, b) {
                FloorKind::Edge
            } else {
                FloorKind::NoRoot
            }
        };
        let kind = if feet.hi < s0 || feet.lo > s1 {
            FloorKind::ExcludedByT
        } else {
            kind_of(b)
        };
        let piece = FloorBox {
            t: Some(feet),
            kind,
            ..r
        };
        if kind == FloorKind::ExcludedByT {
            return Cut::Kept(piece);
        }
        // the floor of step 3: a direction is not cut once its image is within half the
        // tolerance
        let halved = cuts(line, b, &e.s_u, &e.s_v)
            .filter(|c| c.cut_u || c.cut_v)
            .map(|c| {
                halves(
                    b,
                    c.split_u,
                    if c.split_u { b.u.mid() } else { b.v.mid() },
                    None,
                )
            });
        match halved {
            // (each half carries the line parameters of the box it is cut from until it is
            // enclosed: they hold its own)
            Some(Visit::Halve(first, second)) => Cut::Halves(
                FloorBox {
                    u: first.u,
                    v: first.v,
                    t: Some(feet),
                    kind: kind_of(first),
                },
                FloorBox {
                    u: second.u,
                    v: second.v,
                    t: Some(feet),
                    kind: kind_of(second),
                },
            ),
            _ => Cut::Kept(piece),
        }
    }
}

/// What the exclusion phase makes of one box with no root.
enum Cut {
    /// Proved farther than the tolerance.
    Excluded,
    /// Kept, as it now is.
    Kept(FloorBox),
    /// Cut in two, the first to visit first.
    Halves(FloorBox, FloorBox),
}

/// The side `s` of the box `b` (in the order of [`LineNet::side_kinds`]), as a box of no
/// width.
fn side_of(b: Scratch, s: usize) -> Scratch {
    match s {
        0 => Scratch {
            u: Ivl::from(b.u.lo),
            ..b
        },
        1 => Scratch {
            u: Ivl::from(b.u.hi),
            ..b
        },
        2 => Scratch {
            v: Ivl::from(b.v.lo),
            ..b
        },
        _ => Scratch {
            v: Ivl::from(b.v.hi),
            ..b
        },
    }
}

/// Whether the closed boxes `a` and `b` meet.
fn boxes_meet(a: Scratch, b: Scratch) -> bool {
    a.u.lo <= b.u.hi && a.u.hi >= b.u.lo && a.v.lo <= b.v.hi && a.v.hi >= b.v.lo
}

/// The most pairs of boxes [`meeting_pairs`] tests: about a thousand for each of the most
/// records a leaf keeps, the root pass's and its exclusion phase's together.
const MAX_MEETING_TESTS: usize = 1 << 23;

/// Calls `found(i, j)`, `i < j`, for every pair of the closed `(u, v)` boxes `rects` that
/// meet, by a sweep in u; false (with pairs left unreported) once it would test more than
/// [`MAX_MEETING_TESTS`] pairs.
fn meeting_pairs(rects: &[[Ivl; 2]], mut found: impl FnMut(usize, usize)) -> bool {
    let lo_u = |i: usize| rects.get(i).map_or(f64::NAN, |[u, _]| u.lo);
    let mut order: Vec<usize> = (0..rects.len()).collect();
    order.sort_by(|&a, &b| lo_u(a).total_cmp(&lo_u(b)).then(a.cmp(&b)));
    let mut active: Vec<usize> = Vec::new();
    let mut tests = 0_usize;
    for &i in &order {
        let Some(&[ui, vi]) = rects.get(i) else {
            continue;
        };
        tests = tests.saturating_add(active.len());
        if tests > MAX_MEETING_TESTS {
            return false;
        }
        // (sorted by u.lo: a box still active meets this one in u when it reaches it)
        active.retain(|&j| rects.get(j).is_some_and(|[uj, _]| uj.hi >= ui.lo));
        for &j in &active {
            if rects
                .get(j)
                .is_some_and(|[_, vj]| vj.lo <= vi.hi && vj.hi >= vi.lo)
            {
                found(j.min(i), j.max(i));
            }
        }
        active.push(i);
    }
    true
}

/// The least member of `i`'s set (every member points to a smaller one or itself, so the
/// walk ends), halving the path on the way.
fn root_of(parent: &mut [usize], mut i: usize) -> usize {
    while let Some(&p) = parent.get(i) {
        if p >= i {
            break;
        }
        let up = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(i) {
            *slot = up;
        }
        i = p;
    }
    i
}

/// Joins the sets of `i` and `j`, named by the lesser of their least members.
fn join(parent: &mut [usize], i: usize, j: usize) {
    let (a, b) = (root_of(parent, i), root_of(parent, j));
    let (keep, gone) = if a <= b { (a, b) } else { (b, a) };
    if let Some(slot) = parent.get_mut(gone) {
        *slot = keep;
    }
}

/// One searched leaf of a solve as the components read it (module docs, "Components").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct SearchedLeaf {
    /// The leaf's domain.
    pub domain: Extent2d,
    /// How its pass ended.
    pub stop: Stop,
    /// What its pass kept ([`Isolation::floor`]); `None` where the solve did not keep it
    /// (past [`MAX_SOLVE_BOXES`]): nothing is then known of the leaf's boxes, and no box is
    /// read as excluded there for having no record.
    pub floor: Option<Vec<FloorBox>>,
    /// Its certified roots ([`Isolation::roots`]).
    pub roots: Vec<CertifiedRoot>,
    /// Whether it is a culled strip ([`enclose_strip`]) rather than a leaf the traversal
    /// solved.
    pub strip: bool,
}

impl SearchedLeaf {
    /// Whether a box of this leaf that no record holds is excluded by distance (module docs,
    /// "The drop"): its pass finished, or stopped at its limit with the boxes it had still
    /// to visit recorded, and its records are kept. A pass that did not run proves nothing.
    fn reads_absence(&self) -> bool {
        self.floor.is_some() && matches!(self.stop, Stop::Finished | Stop::AtLimit)
    }
}

/// What a solve's drop did (module docs, "The drop").
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Drops {
    /// The side enclosures it made: at most its budget.
    pub enclosures: usize,
    /// Whether it stopped at its budget (a set it had not shown farther is kept) or did not
    /// run past its bounds on boxes or on the pairs it tests.
    pub out_of_budget: bool,
    /// Plain boxes with no root it dropped.
    pub no_root: usize,
    /// Edge boxes it dropped.
    pub edge: usize,
    /// Of the boxes it dropped, those in sets that read another leaf's records: a set over
    /// more than one leaf, or with a member's side on its leaf's border read across it.
    pub across: usize,
}

/// The solve's drop (module docs, "The drop"): drops from the records of `leaves` (every
/// searched leaf and culled strip of one solve, `sides` what lies across each) every
/// connected set of regular boxes with no root, plain or edge, whose least gap to `line` is
/// shown farther than the tolerance by its boundary and crease sides and by the excluded
/// boxes around it. Decided on the records as they are, then applied at once; nothing runs
/// where a leaf's records are not kept and its boxes are past [`MAX_SOLVE_BOXES`], and at
/// most `budget` side enclosures are made.
///
/// # Errors
///
/// A cancel.
pub(crate) fn drop_fenced(
    net: &LineNet,
    line: &PassLine,
    leaves: &mut [SearchedLeaf],
    sides: &[LeafSides],
    cancel: Option<&CancelToken>,
    budget: usize,
) -> Result<Drops> {
    let mut done = Drops::default();
    let [s0, s1] = line.span;
    let Some(scratch) = ScratchLine::of(line, &Extent1d::from_min_max(s0, s1)) else {
        return Ok(done);
    };
    let gone = {
        let view: &[SearchedLeaf] = leaves;
        // every record of every leaf whose records are kept, in leaf order
        let mut recs: Vec<(usize, FloorBox)> = Vec::new();
        for (l, leaf) in view.iter().enumerate() {
            if let Some(floor) = &leaf.floor {
                recs.extend(floor.iter().map(|&r| (l, r)));
                if recs.len() > MAX_SOLVE_BOXES {
                    done.out_of_budget = true;
                    return Ok(done);
                }
            }
        }
        let n = recs.len();
        let member = |i: usize| {
            recs.get(i)
                .is_some_and(|(_, r)| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge))
        };
        let scratch_of = |i: usize| recs.get(i).map(|(_, r)| Scratch { u: r.u, v: r.v });
        let rects: Vec<[Ivl; 2]> = recs.iter().map(|(_, r)| [r.u, r.v]).collect();
        let mut near: Vec<Vec<usize>> = vec![Vec::new(); n];
        let within = meeting_pairs(&rects, |i, j| {
            if let Some(list) = near.get_mut(i) {
                list.push(j);
            }
            if let Some(list) = near.get_mut(j) {
                list.push(i);
            }
        });
        if !within {
            done.out_of_budget = true;
            return Ok(done);
        }
        // the connected sets of boxes with no root, each named by its least box
        let mut parent: Vec<usize> = (0..n).collect();
        for (i, list) in near.iter().enumerate() {
            for &j in list {
                if member(i) && member(j) {
                    join(&mut parent, i, j);
                }
            }
        }
        let set: Vec<usize> = (0..n).map(|i| root_of(&mut parent, i)).collect();
        let readable = |l: usize| view.get(l).is_some_and(SearchedLeaf::reads_absence);
        // what keeps each set (conditions 2 and 3 as far as the records tell them), whether
        // it reads across a leaf's border, and the sides it must show farther (1 and 2)
        let mut blocked = vec![false; n];
        let mut across = vec![false; n];
        let mut needs: Vec<Vec<Scratch>> = vec![Vec::new(); n];
        for i in 0..n {
            let (Some(&(l, _)), Some(b)) = (recs.get(i), scratch_of(i)) else {
                continue;
            };
            if !member(i) {
                continue;
            }
            let own = set.get(i).copied().unwrap_or(i);
            let list = near.get(i).map_or(&[][..], Vec::as_slice);
            let mut stop = !readable(l);
            let mut reads = false;
            let mut want: Vec<Scratch> = Vec::new();
            for &j in list {
                if !member(j) {
                    // a record that is not a member: never excluded (one excluded by t
                    // among them)
                    stop = true;
                } else if recs.get(j).is_some_and(|&(m, _)| m != l) {
                    reads = true;
                }
            }
            if let Some(own_sides) = sides.get(l) {
                let kinds = net.side_kinds(&line.solve_domain, own_sides, b);
                for (s, &kind) in kinds.iter().enumerate() {
                    let side = side_of(b, s);
                    let on_border = own_sides.on_side(b, s);
                    // (a side that is no boundary and lies on the leaf's border is covered by
                    // the leaves across it)
                    let across_read = || own_sides.leaves_across(b, s).all(readable);
                    match kind {
                        SideKind::Boundary => want.push(side),
                        SideKind::Crease => {
                            let alone = !list.iter().any(|&j| {
                                member(j) && scratch_of(j).is_some_and(|o| boxes_meet(o, side))
                            });
                            if alone && (!on_border || across_read()) {
                                reads |= on_border;
                            } else {
                                want.push(side);
                            }
                        }
                        SideKind::Plain => {
                            if on_border {
                                if across_read() {
                                    reads = true;
                                } else {
                                    stop = true;
                                }
                            }
                        }
                    }
                }
            } else {
                stop = true;
            }
            if let Some(flag) = blocked.get_mut(own) {
                *flag |= stop;
            }
            if let Some(flag) = across.get_mut(own) {
                *flag |= reads;
            }
            if let Some(slot) = needs.get_mut(own) {
                slot.extend(want);
            }
        }
        // the sides of each set not kept by the above, shown farther by their own
        // enclosures, the sets in the order of their least box
        for i in 0..n {
            if !member(i) || set.get(i) != Some(&i) || blocked.get(i) != Some(&false) {
                continue;
            }
            let mut shown = true;
            for side in needs.get(i).map_or(&[][..], Vec::as_slice) {
                if done.enclosures >= budget {
                    done.out_of_budget = true;
                    shown = false;
                    break;
                }
                if done.enclosures % 64 == 0 {
                    if let Some(token) = cancel {
                        token.check(OP)?;
                    }
                }
                done.enclosures += 1;
                let farther = enclosed_value(net, &scratch, side.u, side.v)
                    .is_some_and(|r| scratch.farther(&r));
                if !farther {
                    shown = false;
                    break;
                }
            }
            if let Some(flag) = blocked.get_mut(i) {
                *flag = !shown;
            }
        }
        let gone: Vec<bool> = (0..n)
            .map(|i| {
                member(i)
                    && set
                        .get(i)
                        .is_some_and(|&own| blocked.get(own) == Some(&false))
            })
            .collect();
        for (i, &g) in gone.iter().enumerate() {
            if !g {
                continue;
            }
            match recs.get(i).map(|(_, r)| r.kind) {
                Some(FloorKind::Edge) => done.edge += 1,
                _ => done.no_root += 1,
            }
            if set
                .get(i)
                .is_some_and(|&own| across.get(own) == Some(&true))
            {
                done.across += 1;
            }
        }
        gone
    };
    let mut gone = gone.into_iter();
    for leaf in leaves.iter_mut() {
        if let Some(floor) = &mut leaf.floor {
            floor.retain(|_| !gone.next().unwrap_or(false));
        }
    }
    Ok(done)
}

/// What a box of a component is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) enum Part {
    /// A box a leaf's pass kept.
    Floor(FloorKind),
    /// The whole domain of a leaf whose pass did not run: nothing is known of it.
    NotRunLeaf,
}

/// A box of a solve's components.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct PlacedBox {
    /// The leaf it lies in (an index of the solve's searched leaves).
    pub leaf: usize,
    /// The box in u.
    pub u: Ivl,
    /// The box in v.
    pub v: Ivl,
    /// Its line parameters; the whole line where they were not enclosed.
    pub t: Ivl,
    /// What it is.
    pub part: Part,
}

/// How many boxes of each kind a component holds.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Holds {
    /// Boxes left at the floor.
    pub left: usize,
    /// Edge boxes.
    pub edge: usize,
    /// Plain boxes with no root the exclusion phase kept.
    pub no_root: usize,
    /// Boxes holding a certified root, reported.
    pub root: usize,
    /// Boxes holding a certified root not reported.
    pub unshown: usize,
    /// Boxes a pass stopped at its limit before visiting.
    pub unvisited: usize,
    /// Leaves whose pass did not run.
    pub not_run: usize,
    /// Boxes of a culled strip that step 1 did not exclude.
    pub culled: usize,
}

/// What a component lies in or touches that no pass decided.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub(crate) struct Touches {
    /// A leaf whose pass stopped at its limit.
    pub unfinished: bool,
    /// A leaf whose pass did not run.
    pub not_run: bool,
    /// A stretch of a leaf's border that no searched leaf or strip covers (a leaf outside the
    /// face: [`LeafSides`]).
    pub unsearched: bool,
}

/// A connected part of what the passes of a solve did not prove farther than the
/// tolerance (module docs, "Components").
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct Component {
    /// Its boxes, indices of [`Components::boxes`], ascending.
    pub boxes: Vec<usize>,
    /// The hull of its boxes in u and in v.
    pub u: Ivl,
    /// The hull in v.
    pub v: Ivl,
    /// The hull of its boxes' line parameters (each the feet widened by the tolerance).
    pub t: Ivl,
    /// Whether that hull reaches an end of the line's own interval.
    pub reaches_end: bool,
    /// The leaves it lies in or touches whose passes did not decide them.
    pub touches: Touches,
    /// Its boxes by kind.
    pub holds: Holds,
    /// The certified roots in it, `(u, v, t)`.
    pub roots: Vec<[f64; 3]>,
    /// Its type: undecided until the typing of components.
    pub contact: Contact,
}

/// The components of one solve (module docs, "Components").
#[derive(Debug, Clone, Default, PartialEq)]
pub(crate) struct Components {
    /// Every box of every component, sorted.
    pub boxes: Vec<PlacedBox>,
    /// The components, by their first box.
    pub list: Vec<Component>,
    /// Whether the solve's boxes or the tests joining them passed their bounds: nothing is
    /// then known of the components, and the whole line is undecided.
    pub over_budget: bool,
}

/// The most boxes a solve's drop reads and its components hold (sixteen leaves' worth at a
/// leaf's bounds); past it nothing is dropped and the components are not built
/// ([`Components::over_budget`]).
pub(crate) const MAX_SOLVE_BOXES: usize = 1 << 16;

impl Components {
    /// Components not built: past the bounds.
    pub(crate) fn over_budget() -> Self {
        Self {
            over_budget: true,
            ..Self::default()
        }
    }
}

/// The components of a solve over the whole surface: the kept boxes of every searched leaf
/// and culled strip (`leaves`, with `sides` what lies across each), and the whole domain of
/// each leaf whose pass did not run, joined where their closed `(u, v)` boxes meet and their
/// line parameters overlap. Boxes excluded by t are not in them. `span` is the line's own
/// interval. Deterministic: the boxes are sorted, and a component is named by its first.
/// Not built ([`Components::over_budget`]) where a leaf's records were not kept.
pub(crate) fn components(
    leaves: &[SearchedLeaf],
    sides: &[LeafSides],
    span: [f64; 2],
) -> Components {
    let mut out = Components::default();
    for (l, leaf) in leaves.iter().enumerate() {
        if leaf.stop == Stop::NotRun {
            let [[u0, u1], [v0, v1]] = bounds_of(&leaf.domain);
            out.boxes.push(PlacedBox {
                leaf: l,
                u: Ivl::new(u0, u1),
                v: Ivl::new(v0, v1),
                t: Ivl::whole(),
                part: Part::NotRunLeaf,
            });
        }
        let Some(floor) = &leaf.floor else {
            // (records not kept: nothing is known of the components)
            if leaf.stop == Stop::NotRun {
                continue;
            }
            return Components::over_budget();
        };
        for r in floor {
            if r.kind != FloorKind::ExcludedByT {
                out.boxes.push(PlacedBox {
                    leaf: l,
                    u: r.u,
                    v: r.v,
                    t: r.t.unwrap_or_else(Ivl::whole),
                    part: Part::Floor(r.kind),
                });
            }
        }
        if out.boxes.len() > MAX_SOLVE_BOXES {
            return Components::over_budget();
        }
    }
    out.boxes.sort_by(|a, b| {
        a.u.lo
            .total_cmp(&b.u.lo)
            .then(a.v.lo.total_cmp(&b.v.lo))
            .then(a.u.hi.total_cmp(&b.u.hi))
            .then(a.v.hi.total_cmp(&b.v.hi))
            .then(a.t.lo.total_cmp(&b.t.lo))
            .then(a.t.hi.total_cmp(&b.t.hi))
            .then(a.leaf.cmp(&b.leaf))
    });
    let n = out.boxes.len();
    let rects: Vec<[Ivl; 2]> = out.boxes.iter().map(|b| [b.u, b.v]).collect();
    let mut parent: Vec<usize> = (0..n).collect();
    let boxes = &out.boxes;
    let within = meeting_pairs(&rects, |i, j| {
        let overlap = match (boxes.get(i), boxes.get(j)) {
            (Some(a), Some(b)) => a.t.lo <= b.t.hi && b.t.lo <= a.t.hi,
            _ => false,
        };
        if overlap {
            join(&mut parent, i, j);
        }
    });
    if !within {
        return Components::over_budget();
    }
    let [s0, s1] = span;
    let mut named: Vec<Option<usize>> = vec![None; n];
    let mut of_box: Vec<usize> = Vec::new();
    for i in 0..n {
        let Some(&b) = out.boxes.get(i) else {
            continue;
        };
        let first = root_of(&mut parent, i);
        let c = if let Some(c) = named.get(first).copied().flatten() {
            c
        } else {
            let c = out.list.len();
            if let Some(slot) = named.get_mut(first) {
                *slot = Some(c);
            }
            out.list.push(Component {
                boxes: Vec::new(),
                u: b.u,
                v: b.v,
                t: b.t,
                reaches_end: false,
                touches: Touches::default(),
                holds: Holds::default(),
                roots: Vec::new(),
                contact: Contact::Undecided,
            });
            c
        };
        of_box.push(c);
        let flags = facing(&b, leaves, sides);
        let Some(comp) = out.list.get_mut(c) else {
            continue;
        };
        comp.boxes.push(i);
        comp.u = comp.u.hull(b.u);
        comp.v = comp.v.hull(b.v);
        comp.t = comp.t.hull(b.t);
        comp.reaches_end = comp.t.lo <= s0 || comp.t.hi >= s1;
        comp.touches.unfinished |= flags.unfinished;
        comp.touches.not_run |= flags.not_run;
        comp.touches.unsearched |= flags.unsearched;
        let h = &mut comp.holds;
        let count = match b.part {
            Part::NotRunLeaf => Some(&mut h.not_run),
            Part::Floor(FloorKind::Left) => Some(&mut h.left),
            Part::Floor(FloorKind::Edge) => Some(&mut h.edge),
            Part::Floor(FloorKind::NoRoot) => Some(&mut h.no_root),
            Part::Floor(FloorKind::Root { .. }) => Some(&mut h.root),
            Part::Floor(FloorKind::Unshown { .. }) => Some(&mut h.unshown),
            Part::Floor(FloorKind::Unvisited) => Some(&mut h.unvisited),
            Part::Floor(FloorKind::Culled) => Some(&mut h.culled),
            // (never placed)
            Part::Floor(FloorKind::ExcludedByT) => None,
        };
        if let Some(count) = count {
            *count += 1;
        }
    }
    // each certified root in the component of a box of its leaf holding it
    for (l, leaf) in leaves.iter().enumerate() {
        for root in &leaf.roots {
            let [u, v, t] = root.point;
            let holder = out.boxes.iter().position(|b| {
                b.leaf == l
                    && matches!(b.part, Part::Floor(FloorKind::Root { .. }))
                    && b.u.lo <= u
                    && u <= b.u.hi
                    && b.v.lo <= v
                    && v <= b.v.hi
                    && b.t.lo <= t
                    && t <= b.t.hi
            });
            let comp = holder
                .and_then(|i| of_box.get(i).copied())
                .and_then(|c| out.list.get_mut(c));
            if let Some(comp) = comp {
                comp.roots.push(root.point);
            }
        }
    }
    out
}

/// What the box `b` of a component touches across its leaf's border: a leaf whose pass
/// stopped at its limit (or `b` lies in one), a leaf whose pass did not run (or `b` is
/// one), and a stretch that no searched leaf or strip covers.
fn facing(b: &PlacedBox, leaves: &[SearchedLeaf], sides: &[LeafSides]) -> Touches {
    let own = leaves.get(b.leaf).map(|l| l.stop);
    let mut touches = Touches {
        unfinished: own == Some(Stop::AtLimit),
        not_run: b.part == Part::NotRunLeaf,
        unsearched: false,
    };
    let Some(leaf_sides) = sides.get(b.leaf) else {
        return touches;
    };
    let s = Scratch { u: b.u, v: b.v };
    touches.unsearched = leaf_sides.faces_unsearched(s).iter().any(|&c| c);
    for side in 0..4 {
        for j in leaf_sides.leaves_across(s, side) {
            match leaves.get(j).map(|l| l.stop) {
                Some(Stop::AtLimit) => touches.unfinished = true,
                Some(Stop::NotRun) => touches.not_run = true,
                _ => {}
            }
        }
    }
    touches
}

/// The side record of a solve (module docs, "Components"): not run when the solve has a
/// leaf the traversal solved and no such leaf's pass ran, whatever its culled strips found
/// (the solutions are then the guesses' alone, and a strip's boxes undecided around a leaf
/// not run narrow nothing), or when it has none and no strip ran; otherwise undecided over
/// every component's line parameters (each undecided until the typing of components) and
/// over the whole of the line's own interval `span` where a cull's strip could not be
/// placed (`unplaced`: nothing has enclosed it) or the components passed their bounds;
/// complete only with none of these. A strip's boxes it left undecided are in the
/// components, over their own line parameters. A leaf whose pass ran beside leaves not run
/// leaves the record undecided: a finished one may have added certified roots, so the
/// solutions are no longer the guesses' alone.
pub(crate) fn side_record(
    leaves: &[SearchedLeaf],
    components: &Components,
    unplaced: bool,
    span: [f64; 2],
) -> SideRecord {
    if leaves_not_run(leaves) {
        return SideRecord::NotRun;
    }
    let [s0, s1] = span;
    let mut spans: Vec<[f64; 2]> = Vec::new();
    if unplaced || components.over_budget {
        spans.push(span);
    }
    for c in &components.list {
        let (lo, hi) = (cmax(c.t.lo, s0), cmin(c.t.hi, s1));
        if c.t.lo.is_nan() || c.t.hi.is_nan() {
            // (nothing is known of where it lies)
            spans.push(span);
        } else if lo <= hi {
            spans.push([lo, hi]);
        }
    }
    if spans.is_empty() {
        SideRecord::Complete
    } else {
        SideRecord::undecided(spans)
    }
}

/// Whether the side record of a solve over `leaves` is not run ([`side_record`]): the
/// passes of the leaves the traversal solved (not the culled strips) all did not run, or,
/// with no such leaf (every leaf culled), there are strips and none of them ran. (`all` over
/// no leaves is true, hence the second case's own test.)
pub(crate) fn leaves_not_run(leaves: &[SearchedLeaf]) -> bool {
    let mut solved = leaves.iter().filter(|l| !l.strip).peekable();
    if solved.peek().is_some() {
        solved.all(|l| l.stop == Stop::NotRun)
    } else {
        !leaves.is_empty() && leaves.iter().all(|l| l.stop == Stop::NotRun)
    }
}

/// What a box of the work list becomes; each verdict but the first and the halving carries
/// the box's line parameters (the feet of its points widened by the tolerance).
enum Visit {
    /// Proved farther than the tolerance from the line (step 1).
    ExcludedByDistance,
    /// Its line parameters wholly outside the line's own interval (step 1).
    ExcludedByT(Ivl),
    /// Regular, and Krawczyk's test proves it holds no root (step 2).
    NoRoot(Ivl),
    /// Regular, with its one root, certified and shown.
    Root(CertifiedRoot, Ivl),
    /// Regular, with its one root certified but not reported ([`Isolation::unshown`]), and
    /// the `(u, v)` box that root is the only one in.
    Unshown([Ivl; 2], Ivl),
    /// At the floor (step 3), with its line parameters where it was enclosed.
    Left(Option<Ivl>),
    /// Its two halves, the first to visit first.
    Halve(Scratch, Scratch),
}

/// A box enclosed about the surface's point at its middle: the quotient's terms are then
/// of the size of the box, not of its distance from the origin.
struct Enclosed {
    /// `S − P` at the box's middle, enclosed.
    s_mid: I3,
    /// The point the box is enclosed about.
    about: [f64; 3],
    /// `S − P` over the box.
    r: I3,
    /// `S_u` over the box.
    s_u: I3,
    /// `S_v` over the box.
    s_v: I3,
}

/// The box `[u, v]` enclosed; `None` where the net refuses it (it leaves the knots'
/// range, or weights are not proved positive over it).
fn enclosed(net: &LineNet, line: &ScratchLine, u: Ivl, v: Ivl) -> Option<Enclosed> {
    let s_mid = net.at(u.mid(), v.mid(), line.p)?;
    let [m0, m1, m2] = mids(&s_mid);
    let [p0, p1, p2] = line.p;
    let about = [p0 + m0, p1 + m1, p2 + m2];
    let [s, s_u, s_v] = net.over(u, v, about)?;
    let r = line.off_the_start(&s, about);
    Some(Enclosed {
        s_mid,
        about,
        r,
        s_u,
        s_v,
    })
}

/// `S − P` over the box `[u, v]`, enclosed about the surface's point at its middle as
/// [`enclosed`] encloses it, without the partials (what step 1 reads); `None` where the net
/// refuses it.
fn enclosed_value(net: &LineNet, line: &ScratchLine, u: Ivl, v: Ivl) -> Option<I3> {
    let s_mid = net.at(u.mid(), v.mid(), line.p)?;
    let [m0, m1, m2] = mids(&s_mid);
    let [p0, p1, p2] = line.p;
    let about = [p0 + m0, p1 + m1, p2 + m2];
    let s = net.value_over(u, v, about)?;
    Some(line.off_the_start(&s, about))
}

/// How the floor of step 3 sees a box: whether each direction is still cut (its image
/// along it longer than half the tolerance), and which one step 4 halves.
#[derive(Clone, Copy)]
struct Cuts {
    cut_u: bool,
    cut_v: bool,
    split_u: bool,
}

/// [`Cuts`] of the box `b` from its partials; `None` where the sizes are not finite.
fn cuts(line: &ScratchLine, b: Scratch, s_u: &I3, s_v: &I3) -> Option<Cuts> {
    let (du, dv) = (b.u.hi - b.u.lo, b.v.hi - b.v.lo);
    let (along_u, along_v) = (
        round_up(length_squared(s_u).hi.sqrt() * du),
        round_up(length_squared(s_v).hi.sqrt() * dv),
    );
    // how far each direction carries the surface from the line over the box, times |D|
    let (off_u, off_v) = (
        length_squared(&cross(line.d, s_u)).hi.sqrt() * du,
        length_squared(&cross(line.d, s_v)).hi.sqrt() * dv,
    );
    if !(along_u.is_finite() && along_v.is_finite() && off_u.is_finite() && off_v.is_finite()) {
        return None;
    }
    // a direction is cut while the box's image along it is longer than half the tolerance
    let (cut_u, cut_v) = (
        round_up(along_u + along_u) > line.tol,
        round_up(along_v + along_v) > line.tol,
    );
    // step 4: across the direction that carries the surface farther from the line (the
    // longer one where neither does: a box the line runs along in both)
    let split_u = if cut_u != cut_v {
        cut_u
    } else if off_u > 0.0 || off_v > 0.0 {
        off_u >= off_v
    } else {
        along_u >= along_v
    };
    Some(Cuts {
        cut_u,
        cut_v,
        split_u,
    })
}

/// One box (module docs, steps 1 to 4).
fn visit(net: &LineNet, line: &ScratchLine, b: Scratch) -> Visit {
    // 0. one polynomial piece a box
    if let Some((in_u, knot)) = net.knot_inside(b.u, b.v) {
        return if in_u {
            Visit::Halve(
                Scratch {
                    u: Ivl::new(b.u.lo, knot),
                    ..b
                },
                Scratch {
                    u: Ivl::new(knot, b.u.hi),
                    ..b
                },
            )
        } else {
            Visit::Halve(
                Scratch {
                    v: Ivl::new(b.v.lo, knot),
                    ..b
                },
                Scratch {
                    v: Ivl::new(knot, b.v.hi),
                    ..b
                },
            )
        };
    }
    let (um, vm) = (b.u.mid(), b.v.mid());
    // a box that cannot be enclosed is halved across its wider side (left where it cannot)
    let Some(Enclosed {
        s_mid,
        about,
        r,
        s_u,
        s_v,
    }) = enclosed(net, line, b.u, b.v)
    else {
        let split_u = b.u.hi - b.u.lo >= b.v.hi - b.v.lo;
        return halves(b, split_u, if split_u { um } else { vm }, None);
    };

    // 1. farther than the tolerance from the line everywhere, or off the line's own interval
    if line.farther(&r) {
        return Visit::ExcludedByDistance;
    }
    // (Krawczyk's steps start from `t`; the verdicts read its enclosure `feet`)
    let (t, feet) = line.feet(&r);
    if !feet.is_finite() {
        // nothing is proved of a box whose line parameters cannot be enclosed
        return Visit::Left(None);
    }
    let [s0, s1] = line.span;
    if feet.hi < s0 || feet.lo > s1 {
        return Visit::ExcludedByT(feet);
    }

    // 2. a regular interval Jacobian: at most one root, and Krawczyk's test for it
    if let Some(found) = regular(net, line, b, &s_mid, [&s_u, &s_v], t) {
        match found {
            Regular::Empty => return Visit::NoRoot(feet),
            Regular::Root(enclosure, unique) => {
                return match refined(net, line, enclosure, unique) {
                    Some(root) => Visit::Root(root, feet),
                    None => Visit::Unshown(unique, feet),
                };
            }
            Regular::Open => {}
        }
    }

    // 3. the floor: no larger than the tolerance, the line running along the surface, or
    // not to be halved
    let Some(c) = cuts(line, b, &s_u, &s_v) else {
        return Visit::Left(Some(feet));
    };
    if !c.cut_u && !c.cut_v {
        return Visit::Left(Some(feet));
    }
    if let Some(m) = in_a_plane_through_the_line(line, &r, t, [&s_u, &s_v]) {
        if fold_is_a_point(net, line, b, about, m, [&s_u, &s_v]) == Some(false) {
            return Visit::Left(Some(feet));
        }
    }

    // 4. halve across the direction that carries the surface farther from the line
    halves(b, c.split_u, if c.split_u { um } else { vm }, Some(feet))
}

/// The two halves of `b` cut at `mid` in u (`split_u`) or in v; the box is left where
/// `mid` is not strictly inside it (a box one rounding wide), with its line parameters `t`
/// where it was enclosed.
fn halves(b: Scratch, split_u: bool, mid: f64, t: Option<Ivl>) -> Visit {
    let (lo, hi) = if split_u {
        (b.u.lo, b.u.hi)
    } else {
        (b.v.lo, b.v.hi)
    };
    if !(mid > lo && mid < hi) {
        return Visit::Left(t);
    }
    let (first, second) = (Ivl::new(lo, mid), Ivl::new(mid, hi));
    if split_u {
        Visit::Halve(Scratch { u: first, ..b }, Scratch { u: second, ..b })
    } else {
        Visit::Halve(Scratch { v: first, ..b }, Scratch { v: second, ..b })
    }
}

/// The unit normal `m` of a plane through the line that the surface over a box lies within
/// the tolerance of: the surface normal at the box's middle made perpendicular to the line
/// (`r` the box's `S − P`, `t` its line parameters). `None` where the surface does not lie
/// that close to that plane, or that normal has no part perpendicular to the line.
fn in_a_plane_through_the_line(
    line: &ScratchLine,
    r: &I3,
    t: Ivl,
    partials: [&I3; 2],
) -> Option<[f64; 3]> {
    let [s_u, s_v] = partials;
    let n = v3(mids(s_u)).cross(v3(mids(s_v)));
    let dv = v3(line.d);
    let dd = dv.dot(dv);
    let across = n - dv * (n.dot(dv) / dd);
    let len = across.length();
    if !(len.is_finite() && len > 0.0 && dd.is_finite() && dd > 0.0) {
        return None;
    }
    let m = f3(across * (1.0 / len));
    // |m| is 1 to rounding, and m · D is 0 to rounding: both enter the bound as they are
    let off = dot(m, r) - t * dot(m, &line.d.map(Ivl::from));
    let unit = round_down(length_squared(&m.map(Ivl::from)).lo.sqrt());
    (unit > 0.0 && round_up(off.mag() / unit) <= line.tol).then_some(m)
}

/// Whether the line's fold on the surface is a point over the box (module docs, step 3):
/// with `f₂ = m · (S − P)` and `f₁ = n · (S − P)`, `n` the unit vector across the line and
/// `m`, the system `G = (f₁, f₁ᵤ f₂ᵥ − f₁ᵥ f₂ᵤ)` has a regular interval Jacobian over the
/// box (the contraction of Krawczyk's operator below 1), so `G` is injective there.
/// `Some(false)` where the system is enclosed and that is not so: the box is then one the
/// line runs along. `None` where the system cannot be enclosed over the box (it crosses a
/// knot line the surface is not `C¹` across): nothing is known of the fold there.
fn fold_is_a_point(
    net: &LineNet,
    line: &ScratchLine,
    b: Scratch,
    about: [f64; 3],
    m: [f64; 3],
    partials: [&I3; 2],
) -> Option<bool> {
    let n = v3(line.d).cross(v3(m));
    let len = n.length();
    if !(len.is_finite() && len > 0.0) {
        return None;
    }
    let n = f3(n * (1.0 / len));
    let [s_uu, s_uv, s_vv] = net.second(b.u, b.v, about)?;
    let [s_u, s_v] = partials;
    // the first and second partials of f₁ (along n) and f₂ (along m)
    let (f1u, f1v, f2u, f2v) = (dot(n, s_u), dot(n, s_v), dot(m, s_u), dot(m, s_v));
    let (f1uu, f1uv, f1vv) = (dot(n, &s_uu), dot(n, &s_uv), dot(n, &s_vv));
    let (f2uu, f2uv, f2vv) = (dot(m, &s_uu), dot(m, &s_uv), dot(m, &s_vv));
    let det_u = f1uu * f2v + f1u * f2uv - f1uv * f2u - f1v * f2uu;
    let det_v = f1uv * f2v + f1u * f2vv - f1vv * f2u - f1v * f2uv;
    let jac = [[f1u, f1v], [det_u, det_v]];
    let Some(y) = inverse_of_columns2(&[[f1u.mid(), det_u.mid()], [f1v.mid(), det_v.mid()]]) else {
        // the middle of the fold's Jacobian is singular to rounding
        return Some(false);
    };
    let (_, q) = contraction(&y, &jac);
    Some(q < 1.0)
}

/// Krawczyk's test on a box whose interval Jacobian is regular; `None` where it is not
/// (or no preconditioner exists).
fn regular(
    net: &LineNet,
    line: &ScratchLine,
    b: Scratch,
    s_mid: &I3,
    partials: [&I3; 2],
    t: Ivl,
) -> Option<Regular> {
    let [s_u, s_v] = partials;
    let minus_d = line.d.map(|x| -x);
    let y = inverse_of_columns3(&[mids(s_u), mids(s_v), minus_d])?;
    let jac: [[Ivl; 3]; 3] =
        core::array::from_fn(|k| match (s_u.get(k), s_v.get(k), minus_d.get(k)) {
            (Some(&a), Some(&c), Some(&e)) => [a, c, Ivl::from(e)],
            _ => [Ivl::whole(); 3],
        });
    let (c, q) = contraction(&y, &jac);
    if !(q < 1.0) {
        return None;
    }
    // every root of the box lies in K(X) ∩ X, for each X the steps reach
    let mut x: I3 = [b.u, b.v, t];
    let mut at_mid = *s_mid;
    for _ in 0..KRAWCZYK_STEPS {
        let mid = mids(&x);
        let [_, _, tm] = mid;
        let g = less(&at_mid, Ivl::from(tm), line.d);
        let k = k_operator(&mid, &y, &g, &c, &x);
        if apart(&k, &x) {
            return Some(Regular::Empty);
        }
        if inside(&k, &x) {
            return Some(Regular::Root(k, [b.u, b.v]));
        }
        let [x0, x1, x2] = x;
        let [k0, k1, k2] = k;
        let (Some(n0), Some(n1), Some(n2)) = (meet(x0, k0), meet(x1, k1), meet(x2, k2)) else {
            return Some(Regular::Empty);
        };
        let next = [n0, n1, n2];
        if next == x {
            break;
        }
        x = next;
        let Some(s) = net.at(n0.mid(), n1.mid(), line.p) else {
            return Some(Regular::Open);
        };
        at_mid = s;
    }
    Some(on_the_border(net, line, b, &x, &y))
}

/// A regular box whose root, if it has one, sits on its border (`x` is what Krawczyk's
/// steps left of the box): the root certified in a small box about `x`'s middle, and the
/// hull of that box and the whole box proved regular too. The hull then holds no root but
/// that one, so the box holds it or none.
fn on_the_border(
    net: &LineNet,
    line: &ScratchLine,
    b: Scratch,
    x: &I3,
    y: &[[f64; 3]; 3],
) -> Regular {
    let value = |p: [Ivl; 3]| -> Result<Option<[Ivl; 3]>> {
        let [u, v, t] = p;
        Ok(net.over(u, v, line.p).map(|[s, _, _]| less(&s, t, line.d)))
    };
    let jacobian = |p: [Ivl; 3]| -> Result<Option<[[Ivl; 3]; 3]>> {
        let [u, v, _] = p;
        Ok(net
            .over(u, v, line.p)
            .map(|[_, s_u, s_v]| [s_u, s_v, line.d.map(|e| Ivl::from(-e))]))
    };
    let Ok(Some((root, _))) = krawczyk_region(mids(x), *y, &value, &jacobian) else {
        return Regular::Open;
    };
    let [ru, rv, _] = root;
    let (hu, hv) = (b.u.hull(ru), b.v.hull(rv));
    if !regular_over(net, line, hu, hv) {
        return Regular::Open;
    }
    // the hull's only root is the one certified
    if meet(ru, b.u).is_none() || meet(rv, b.v).is_none() {
        return Regular::Empty;
    }
    if within(ru, b.u) && within(rv, b.v) {
        return Regular::Root(root, [hu, hv]);
    }
    // its box reaches past the border: the root is certified again in the small box
    // about the middle of the first, which tells the box holding it from its neighbour
    // unless the root is within that small box of the border, where both report it (one
    // root: the identity test in `isolate`)
    let Ok(Some((tight, _))) = krawczyk_region(mids(&root), *y, &value, &jacobian) else {
        return Regular::Open;
    };
    let [tu, tv, _] = tight;
    let inside_first = within(tu, ru) && within(tv, rv);
    if !inside_first {
        return Regular::Open;
    }
    if meet(tu, b.u).is_some() && meet(tv, b.v).is_some() {
        Regular::Root(tight, [hu, hv])
    } else {
        Regular::Empty
    }
}

/// Whether the interval Jacobian of `F` over the `(u, v)` box is regular (the contraction
/// of Krawczyk's operator below 1): the box then holds one root at most, with any line
/// parameter.
fn regular_over(net: &LineNet, line: &ScratchLine, u: Ivl, v: Ivl) -> bool {
    let Some([_, s_u, s_v]) = net.over(u, v, line.p) else {
        return false;
    };
    let minus_d = line.d.map(|e| -e);
    let Some(y) = inverse_of_columns3(&[mids(&s_u), mids(&s_v), minus_d]) else {
        return false;
    };
    let jac: [[Ivl; 3]; 3] =
        core::array::from_fn(|k| match (s_u.get(k), s_v.get(k), minus_d.get(k)) {
            (Some(&a), Some(&c), Some(&e)) => [a, c, Ivl::from(e)],
            _ => [Ivl::whole(); 3],
        });
    let (_, q) = contraction(&y, &jac);
    q < 1.0
}

/// The certified root as it is reported: Newton's root from the enclosure's middle where
/// it lies in the enclosure (else that middle), kept when it is on the leaf's line interval
/// and the gap there is enclosed within the tolerance. `None` otherwise: nothing is
/// reported that is not shown.
fn refined(
    net: &LineNet,
    line: &ScratchLine,
    enclosure: I3,
    unique: [Ivl; 2],
) -> Option<CertifiedRoot> {
    let at = |p: [f64; 3]| -> Result<Linearised<3>> {
        let [u, v, t] = p;
        Ok(net
            .over(Ivl::from(u), Ivl::from(v), line.p)
            .map(|[s, s_u, s_v]| {
                (
                    mids(&less(&s, Ivl::from(t), line.d)),
                    [mids(&s_u), mids(&s_v), line.d.map(|e| -e)],
                )
            }))
    };
    let start = mids(&enclosure);
    let point = match newton(start, &at, &inverse_of_columns3) {
        Ok(Some(p)) => p,
        _ => start,
    };
    // Newton's point stands for the root only inside the box proved to hold it
    let held = point
        .iter()
        .zip(&enclosure)
        .all(|(&x, e)| x >= e.lo && x <= e.hi);
    let point = if held { point } else { start };
    let [u, v, t] = point;
    let [t0, t1] = line.interval;
    if !(t >= t0 && t <= t1) {
        return None;
    }
    let s = net.at(u, v, line.p)?;
    let gap2 = length_squared(&less(&s, Ivl::from(t), line.d));
    let shown = gap2.hi <= (Ivl::from(line.tol) * Ivl::from(line.tol)).lo;
    shown.then_some(CertifiedRoot {
        point,
        enclosure,
        unique,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::surface::cone::Cone;
    use crate::surface::torus::{Torus, TorusOptions};

    /// The pass over the whole of `s`, for `t` in `[t0, t1]` (the line's interval and the
    /// leaf's).
    fn pass(k: &Kernel, s: &Surface, p: Vec3, d: Vec3, [t0, t1]: [f64; 2], tol: f64) -> Isolation {
        pass_in(k, s, p, d, [t0, t1], [t0, t1], tol)
    }

    /// The pass over the whole of `s`, the line's own interval `span` and the leaf's
    /// `[t0, t1]`, with the solve's drop over that one leaf after it.
    fn pass_in(
        k: &Kernel,
        s: &Surface,
        p: Vec3,
        d: Vec3,
        span: [f64; 2],
        [t0, t1]: [f64; 2],
        tol: f64,
    ) -> Isolation {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let domain = s.natural_uv_domain_with_nurb(k).unwrap();
        let interval = Extent1d::from_min_max(t0, t1);
        let line = PassLine {
            point: p,
            vector: d,
            span,
            solve_domain: domain,
            tol,
        };
        let sides = LeafSides::whole_solve(&domain);
        let mut found = isolate(&net, &line, &domain, &interval, &sides, None).unwrap();
        let mut leaf = [SearchedLeaf {
            domain,
            stop: found.stop,
            floor: Some(std::mem::take(&mut found.floor)),
            roots: Vec::new(),
            strip: false,
        }];
        drop_fenced(&net, &line, &mut leaf, &[sides], None, MAX_DROP_ENCLOSURES).unwrap();
        found.floor = leaf[0].floor.take().unwrap();
        found
    }

    /// The line parameters of the certified roots, in order.
    fn parameters(found: &Isolation) -> Vec<f64> {
        let mut t: Vec<f64> = found.roots.iter().map(|r| r.point[2]).collect();
        t.sort_by(f64::total_cmp);
        t
    }

    /// The unit cylinder about z, of height 1.
    fn cylinder(k: &Kernel) -> Surface {
        let mut c = Cone::new_cylinder_bounded(
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
        c.make_nurb(k).unwrap();
        Surface::from(c)
    }

    /// The line along x at height 0.5 and `y`, from x = −1 over a length of 2: it meets
    /// the unit cylinder where `x = ±√(1 − y²)`, at `t = 0.5 ± √(1 − y²) / 2`.
    fn across(y: f64) -> (Vec3, Vec3) {
        (Vec3::new(-1.0, y, 0.5), Vec3::new(2.0, 0.0, 0.0))
    }

    /// The tolerance and the step of upstream's `my_test_csi_tangency`.
    const TOL: f64 = 1.0001e-3;
    const STEP: f64 = 2.0e-3 / 22.0;

    /// The entry and exit of a ray near a torus's crown (2.5° and 2° off the surface,
    /// 2.75 apart) are two certified crossings: the torus of radii 30 and 5 about z and the
    /// ray of tangency-labs/solidcraft#20499. The parameters are the roots of the torus's
    /// quartic along the line (100.409930808 and 103.163922132, computed by two parties
    /// in closed form).
    #[test]
    fn two_crossings_near_a_torus_crown_are_certified_apart() {
        let k = Kernel::default();
        let mut torus = Torus::new(
            &k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            30.0,
            5.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(&k).unwrap();
        let s = Surface::from(torus);
        let p = Vec3::new(82.388_988_497_226_59, 82.388_988_497_226_59, 10.000_04);
        let d = Vec3::new(
            -0.850_300_871_283_819_5,
            -0.523_935_376_527_776_1,
            -0.049_801_099_552_871_67,
        );
        let found = pass(&k, &s, p, d, [99.0, 105.0], 2.0e-5);
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 2, "{t:?}");
        assert!((t[0] - 100.409_930_808).abs() < 1.0e-8, "{t:?}");
        assert!((t[1] - 103.163_922_132).abs() < 1.0e-8, "{t:?}");
        for r in &found.roots {
            let [u, v, t] = r.point;
            let on_surface = s
                .evaluate_point(&k, crate::math::Point2::new(u, v))
                .unwrap();
            assert!(on_surface.distance(p + t * d) < 1.0e-10);
        }
    }

    /// A secant far shallower than the tolerance is still two crossings, each certified:
    /// upstream's tangency test at its step 10, 9.09e-5 deep at tolerance 1.0001e-3.
    #[test]
    fn a_shallow_secant_is_two_certified_crossings() {
        let k = Kernel::default();
        let s = cylinder(&k);
        for step in 0..=10 {
            let y = 1.0 - f64::from(11 - step) * STEP;
            let (p, d) = across(y);
            let found = pass(&k, &s, p, d, [0.0, 1.0], TOL);
            assert_eq!(found.stop, Stop::Finished);
            let half = (1.0 - y * y).sqrt() / 2.0;
            let t = parameters(&found);
            assert_eq!(t.len(), 2, "step {step}: {t:?}");
            assert!((t[0] - (0.5 - half)).abs() < 1.0e-12, "step {step}: {t:?}");
            assert!((t[1] - (0.5 + half)).abs() < 1.0e-12, "step {step}: {t:?}");
        }
    }

    /// A tangent line and a line that misses within the tolerance hold no crossing, and
    /// the contact is left to the floor: the box about it is neither excluded nor decided.
    #[test]
    fn a_tangent_line_and_a_near_miss_are_left_to_the_floor() {
        let k = Kernel::default();
        let s = cylinder(&k);
        for step in [0, 1, 10] {
            let (p, d) = across(1.0 + f64::from(step) * STEP);
            let found = pass(&k, &s, p, d, [0.0, 1.0], TOL);
            assert_eq!(found.stop, Stop::Finished);
            assert!(found.roots.is_empty(), "step {step}: {:?}", found.roots);
            assert!(found.left > 0, "step {step}");
        }
    }

    /// A line farther from the surface than the tolerance leaves nothing: every box is
    /// excluded.
    #[test]
    fn a_line_farther_than_the_tolerance_is_excluded() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let (p, d) = across(1.0 + 2.0 * TOL);
        let found = pass(&k, &s, p, d, [0.0, 1.0], TOL);
        assert_eq!(found.stop, Stop::Finished);
        assert!(found.roots.is_empty());
        assert_eq!(found.left, 0);
    }

    /// A line along a generator of the cylinder is left to the floor without being cut
    /// down to the tolerance: far fewer boxes than the 1 / 1e-5 a cut to the tolerance
    /// along it would take.
    #[test]
    fn a_line_along_a_generator_is_left_in_few_boxes() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let found = pass(
            &k,
            &s,
            Vec3::new(1.0, 0.0, -1.0),
            Vec3::new(0.0, 0.0, 3.0),
            [0.0, 1.0],
            1.0e-5,
        );
        assert_eq!(found.stop, Stop::Finished);
        assert!(found.roots.is_empty(), "{:?}", found.roots);
        assert!(found.left > 0);
        assert!(found.boxes < 500, "{} boxes", found.boxes);
    }

    /// A crossing on a knot line of the net (the cylinder's quarter arcs meet at 90°) is
    /// one root, though boxes on both sides of the line hold it on their borders.
    #[test]
    fn a_crossing_on_a_knot_line_is_one_root() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let found = pass(
            &k,
            &s,
            Vec3::new(0.0, -2.0, 0.5),
            Vec3::new(0.0, 4.0, 0.0),
            [0.0, 1.0],
            1.0e-5,
        );
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 2, "{t:?}");
        assert!(
            (t[0] - 0.25).abs() < 1.0e-12 && (t[1] - 0.75).abs() < 1.0e-12,
            "{t:?}"
        );
    }

    /// The sphere of radius 5 about the origin, whole.
    fn sphere(k: &Kernel) -> Surface {
        let mut s = crate::surface::sphere::Sphere::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, -90.0, 360.0, 90.0),
            5.0,
            crate::surface::sphere::SphereOptions::default(),
        )
        .unwrap();
        s.make_nurb(k).unwrap();
        Surface::from(s)
    }

    /// A line tangent to a sphere, one that misses it within the tolerance, and one
    /// through its poles: the tangent contact and the poles are left to the floor (no
    /// box about them is regular), the pass finishes well inside its bound, and the
    /// crossings it does certify are the sphere's (none for the first two).
    #[test]
    fn a_tangent_line_and_a_line_through_the_poles_of_a_sphere_finish() {
        let k = Kernel::default();
        let s = sphere(&k);
        let tol = 1.0e-6;
        for gap in [0.0, 0.5 * tol] {
            // along x at y = 3, z = 4 + gap: tangent to the sphere at (0, 3, 4) for no gap
            let p = Vec3::new(-10.0, 3.0 * (1.0 + gap / 5.0), 4.0 * (1.0 + gap / 5.0));
            let found = pass(&k, &s, p, Vec3::new(20.0, 0.0, 0.0), [0.0, 1.0], tol);
            assert_eq!(
                found.stop,
                Stop::Finished,
                "gap {gap}: {} boxes",
                found.boxes
            );
            assert!(found.roots.is_empty(), "gap {gap}: {:?}", found.roots);
            assert!(found.left > 0, "gap {gap}");
            assert!(
                found.boxes < MAX_SCRATCH_BOXES / 4,
                "gap {gap}: {} boxes",
                found.boxes
            );
        }
        let found = pass(
            &k,
            &s,
            Vec3::new(0.0, 0.0, -10.0),
            Vec3::new(0.0, 0.0, 20.0),
            [0.0, 1.0],
            tol,
        );
        assert_eq!(found.stop, Stop::Finished, "{} boxes", found.boxes);
        assert!(found.boxes < MAX_SCRATCH_BOXES / 2, "{} boxes", found.boxes);
        for t in parameters(&found) {
            assert!(
                (t - 0.25).abs() < 1.0e-9 || (t - 0.75).abs() < 1.0e-9,
                "{t}"
            );
        }
    }

    /// A line along a generator that lies on a knot line of the net which no halving of
    /// the domain lands on (a 300° cylinder's arcs meet at 100° and 200°): the box is cut
    /// at the knot, and the line is left in few boxes, as on any other generator.
    #[test]
    fn a_line_along_a_generator_on_a_knot_line_is_left_in_few_boxes() {
        let k = Kernel::default();
        let mut c = Cone::new_cylinder_bounded(
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            1.0,
            0.0,
            300.0,
            1.0,
            false,
            false,
            true,
        );
        c.make_nurb(&k).unwrap();
        let s = Surface::from(c);
        let a = 100.0_f64.to_radians();
        let found = pass(
            &k,
            &s,
            Vec3::new(a.cos(), a.sin(), -1.0),
            Vec3::new(0.0, 0.0, 3.0),
            [0.0, 1.0],
            1.0e-5,
        );
        assert_eq!(found.stop, Stop::Finished, "{} boxes", found.boxes);
        assert!(found.roots.is_empty(), "{:?}", found.roots);
        assert!(found.left > 0);
        assert!(found.boxes < 500, "{} boxes", found.boxes);
    }
    /// The torus of radii 30 and 5 about z, whole: its net has knots at the quarters of
    /// both parameters, each of multiplicity 2 at degree 2 (below `C¹` in parameter).
    fn torus(k: &Kernel) -> Surface {
        let mut torus = Torus::new(
            k,
            Vec3::ZERO,
            Vec3::X,
            Vec3::Y,
            Extent2d::from_bounds(0.0, 0.0, 360.0, 360.0),
            30.0,
            5.0,
            TorusOptions::default(),
        )
        .unwrap();
        torus.make_nurb(k).unwrap();
        Surface::from(torus)
    }

    /// A line through the cylinder's axis along x, at a height of half and nine tenths of
    /// the tolerance above its top rim, passes within the tolerance of the rim at x = ±1
    /// (t = 0.25 and 0.75) with no root on the surface. The boxes about those points have
    /// no root, and a side on the top rim (the end of the net's knots in v), or on the
    /// crease under it at x = −1, keeps them as edge boxes: not one of them is a plain box
    /// with no root, and some lie on the rim.
    #[test]
    fn a_line_just_beyond_a_rim_is_kept_in_edge_boxes() {
        let k = Kernel::default();
        let s = cylinder(&k);
        for h in [0.5, 0.9] {
            let found = pass(
                &k,
                &s,
                Vec3::new(-2.0, 0.0, 1.0 + h * TOL),
                Vec3::new(4.0, 0.0, 0.0),
                [0.0, 1.0],
                TOL,
            );
            assert_eq!(found.stop, Stop::Finished);
            assert!(found.roots.is_empty(), "h {h}: {:?}", found.roots);
            for at in [0.25, 0.75] {
                let near: Vec<&FloorBox> = found
                    .floor
                    .iter()
                    .filter(|r| r.t.is_some_and(|t| t.lo <= at && at <= t.hi))
                    .collect();
                assert!(!near.is_empty(), "h {h}, t {at}: {:?}", found.floor);
                for r in &near {
                    assert_eq!(r.kind, FloorKind::Edge, "h {h}, t {at}: {r:?}");
                }
                // (the exclusion phase cuts edge boxes too: a piece under the rim is an
                // edge box by the crease at u = π it touches; the rim keeps the others)
                assert!(
                    near.iter().any(|r| r.v.hi == 1.0),
                    "h {h}, t {at}: {near:?}"
                );
            }
        }
    }

    /// A line along the torus's radius at 90° in its middle plane crosses the tube where
    /// u = 0.25, a quarter knot, at the inner equator (t = 0.25) and at the outer one
    /// (t = 0.75, where v starts and ends). Boxes with no root beside the crossings that
    /// touch u = 0.25 but neither end of the knots are kept as edge boxes by that knot
    /// alone.
    #[test]
    fn a_torus_quarter_knot_keeps_edge_boxes() {
        let k = Kernel::default();
        let s = torus(&k);
        let found = pass(
            &k,
            &s,
            Vec3::new(0.0, 20.0, 0.0),
            Vec3::new(0.0, 20.0, 0.0),
            [0.0, 1.0],
            1.0e-6,
        );
        assert_eq!(found.stop, Stop::Finished);
        let by_the_knot = found.floor.iter().filter(|r| {
            r.kind == FloorKind::Edge
                && (r.u.lo == 0.25 || r.u.hi == 0.25)
                && r.u.lo > 0.0
                && r.u.hi < 1.0
                && r.v.lo > 0.0
                && r.v.hi < 1.0
        });
        assert!(by_the_knot.count() > 0, "{:?}", found.floor);
        // and no box with no root touching the knot is a plain one
        assert!(!found
            .floor
            .iter()
            .any(|r| r.kind == FloorKind::NoRoot && (r.u.lo == 0.25 || r.u.hi == 0.25)));
    }

    /// A line tangent to the torus's crown circle at 45° touches it at one point with a
    /// gap of fourth order; the fold is singular along the crown, so the pass halves the
    /// boxes along it down to the floor's size and stops at its bound. What it did not
    /// reach is recorded as unvisited, never as excluded.
    #[test]
    fn a_pass_at_its_limit_records_the_boxes_it_did_not_visit() {
        let k = Kernel::default();
        let s = torus(&k);
        let c = 30.0 / 2.0_f64.sqrt();
        let along = Vec3::new(-1.0, 1.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let found = pass(
            &k,
            &s,
            Vec3::new(c, c, 5.0) - 10.0 * along,
            20.0 * along,
            [0.0, 1.0],
            1.0e-6,
        );
        assert_eq!(found.stop, Stop::AtLimit);
        assert_eq!(found.boxes, MAX_SCRATCH_BOXES);
        let unvisited: Vec<&FloorBox> = found
            .floor
            .iter()
            .filter(|r| r.kind == FloorKind::Unvisited)
            .collect();
        assert!(!unvisited.is_empty());
        // each with its line parameters enclosed, so that a ray can tell whether it lies
        // before a hit
        assert!(unvisited.iter().all(|r| r.t.is_some()));
        // the boxes still to visit come last
        let first = found.floor.len() - unvisited.len();
        assert!(found.floor[first..]
            .iter()
            .all(|r| r.kind == FloorKind::Unvisited));
        // a finished pass leaves none
        let (p, d) = across(0.5);
        let done = pass(&k, &cylinder(&k), p, d, [0.0, 1.0], TOL);
        assert_eq!(done.stop, Stop::Finished);
        assert!(!done.floor.iter().any(|r| r.kind == FloorKind::Unvisited));
    }

    /// A pass that nothing can be proved of (no tolerance, a line of no length) does not
    /// run, and is counted as not run: not as stopped at the limit, not as finished.
    #[test]
    fn a_pass_that_does_not_run_is_not_counted_at_the_limit() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let (p, d) = across(0.5);
        for (d, tol) in [(d, 0.0), (Vec3::ZERO, TOL), (d, f64::NAN)] {
            let found = pass(&k, &s, p, d, [0.0, 1.0], tol);
            assert_eq!(found.stop, Stop::NotRun);
            assert_eq!(found.boxes, 0);
            assert!(found.floor.is_empty() && found.roots.is_empty());
            k.work_counters.add_line_pass(0, 0, found.stop);
        }
        assert_eq!(k.work_counters.line_leaves_not_run(), 3);
        assert_eq!(k.work_counters.line_leaves_at_limit(), 0);
        assert_eq!(k.work_counters.line_leaves_finished(), 0);
    }

    /// The line across the cylinder at y = 0.5 meets it at t = 0.5 ∓ √0.75 / 2. With the
    /// leaf's interval [0, 0.5] inside the line's [0, 1], the second root is certified but
    /// lies off the leaf: it is unshown, not excluded. With the line's own interval [0, 0.5]
    /// the boxes wholly past it are excluded by t, and no box that is not wholly past it
    /// is.
    #[test]
    fn excluded_by_t_reads_the_line_s_interval_not_the_leaf_s() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let (p, d) = across(0.5);
        let half = 0.75_f64.sqrt() / 2.0;
        let (near, far) = (0.5 - half, 0.5 + half);
        let holds = |r: &FloorBox, at: f64| r.t.is_some_and(|t| t.lo <= at && at <= t.hi);

        let found = pass_in(&k, &s, p, d, [0.0, 1.0], [0.0, 0.5], TOL);
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!((t[0] - near).abs() < 1.0e-12, "{t:?}");
        assert!(found.unshown > 0);
        assert!(found
            .floor
            .iter()
            .any(|r| matches!(r.kind, FloorKind::Unshown { .. }) && holds(r, far)));
        assert!(!found
            .floor
            .iter()
            .any(|r| r.kind == FloorKind::ExcludedByT && holds(r, far)));

        let found = pass_in(&k, &s, p, d, [0.0, 0.5], [0.0, 0.5], TOL);
        assert_eq!(found.stop, Stop::Finished);
        let t = parameters(&found);
        assert_eq!(t.len(), 1, "{t:?}");
        assert!((t[0] - near).abs() < 1.0e-12, "{t:?}");
        assert!(found.floor.iter().any(|r| r.kind == FloorKind::ExcludedByT));
        for r in &found.floor {
            let past = r.t.is_some_and(|t| t.lo > 0.5 || t.hi < 0.0);
            assert_eq!(past, r.kind == FloorKind::ExcludedByT, "{r:?}");
        }
    }

    /// A searched leaf over `domain`, its pass ended at `stop`, with the records `floor`.
    fn leaf_of(domain: Extent2d, stop: Stop, floor: Option<Vec<FloorBox>>) -> SearchedLeaf {
        SearchedLeaf {
            domain,
            stop,
            floor,
            roots: Vec::new(),
            strip: false,
        }
    }

    /// The solve's drop over `leaves` (leaves of a solve over the cylinder's whole domain),
    /// with the line through `p` along `d` and a budget of `budget` side enclosures.
    fn drop_over(
        k: &Kernel,
        p: Vec3,
        d: Vec3,
        leaves: &mut [SearchedLeaf],
        budget: usize,
    ) -> Drops {
        let s = cylinder(k);
        let net = LineNet::of(k, &s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(k).unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol: TOL,
        };
        let doms: Vec<Extent2d> = leaves.iter().map(|l| l.domain).collect();
        let sides = leaf_sides(&doms, &solve);
        drop_fenced(&net, &line, leaves, &sides, None, budget).unwrap()
    }

    /// The solve's drop over one finished leaf over `domain` holding the records `recs`:
    /// the records it keeps, and what it did.
    fn excluded(
        k: &Kernel,
        p: Vec3,
        d: Vec3,
        domain: Extent2d,
        recs: Vec<FloorBox>,
        budget: usize,
    ) -> (Vec<FloorBox>, Drops) {
        let mut leaves = [leaf_of(domain, Stop::Finished, Some(recs))];
        let done = drop_over(k, p, d, &mut leaves, budget);
        (leaves[0].floor.take().unwrap(), done)
    }

    /// A record of `kind` over `[u0, u1] × [v0, v1]`, its line parameters `[0, 1]`.
    fn rec(kind: FloorKind, [u0, u1]: [f64; 2], [v0, v1]: [f64; 2]) -> FloorBox {
        FloorBox {
            u: Ivl::new(u0, u1),
            v: Ivl::new(v0, v1),
            t: Some(Ivl::new(0.0, 1.0)),
            kind,
        }
    }

    /// A connected set of plain boxes with no root whose every touching box is excluded by
    /// distance has its least gap on its border, so it is dropped whole. A set beside a box
    /// that may hold the contact (a box left at the floor), beside an edge box whose crease
    /// side faces such a box, or beside an edge box on the solve's border within the
    /// tolerance is kept. A set touching the border of a leaf with nothing searched across
    /// it has a boundary side there, which an enclosure shows farther: it is dropped.
    #[test]
    fn a_set_with_no_root_fenced_by_excluded_boxes_is_dropped() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = (
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            Vec3::new(4.0, 0.0, 0.0),
        );
        // the net's first crease, the quarter knot at u = π/2
        let q = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][0];
        let set = || {
            vec![
                rec(FloorKind::NoRoot, [1.0, 1.1], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.5, 0.6]),
            ]
        };
        let (kept, done) = excluded(&k, p, d, whole, set(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.no_root, 3);
        assert_eq!(done.across, 0);

        // beside a box left at the floor
        let mut recs = set();
        recs.push(rec(FloorKind::Left, [1.2, 1.3], [0.4, 0.5]));
        let (kept, _) = excluded(&k, p, d, whole, recs, MAX_DROP_ENCLOSURES);
        assert_eq!(kept.len(), 4, "{kept:?}");

        // beside an edge box on the crease u = π/2, whose crease side faces a box left
        let mut recs = set();
        recs.push(rec(FloorKind::Edge, [1.2, q], [0.5, 0.6]));
        recs.push(rec(FloorKind::Left, [q, 1.7], [0.5, 0.6]));
        let (kept, _) = excluded(&k, p, d, whole, recs, MAX_DROP_ENCLOSURES);
        assert_eq!(kept.len(), 5, "{kept:?}");

        // beside an edge box on the top rim where the line passes within the tolerance of
        // it (at u = π, half the tolerance above)
        let pi = std::f64::consts::PI;
        let recs = vec![
            rec(FloorKind::NoRoot, [3.0, 3.1], [0.85, 0.95]),
            rec(FloorKind::Edge, [3.1, pi], [0.9, 1.0]),
        ];
        let (kept, _) = excluded(&k, p, d, whole, recs, MAX_DROP_ENCLOSURES);
        assert_eq!(kept.len(), 2, "{kept:?}");

        // touching the border of a leaf with nothing searched across it: the side on u = 1
        // is a boundary side, about one from the line
        let leaf = Extent2d::from_bounds(1.0, 0.0, 2.0, 1.0);
        let (kept, done) = excluded(&k, p, d, leaf, set(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert!(done.enclosures > 0);
    }

    /// An edge box alone is dropped when its side on the solve's border (the top rim) is
    /// shown farther than the tolerance by an enclosure of that side, and its other sides
    /// face boxes excluded by distance: with the line three tolerances above the rim at
    /// u = π. With the line half a tolerance above it, the side is not shown farther, and
    /// the box is kept.
    #[test]
    fn an_edge_box_is_dropped_only_with_its_border_side_shown_farther() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let pi = std::f64::consts::PI;
        let edge = || vec![rec(FloorKind::Edge, [pi - 1.0e-3, pi], [0.99, 1.0])];
        let d = Vec3::new(4.0, 0.0, 0.0);
        let (kept, done) = excluded(
            &k,
            Vec3::new(-2.0, 0.0, 1.0 + 3.0 * TOL),
            d,
            whole,
            edge(),
            MAX_DROP_ENCLOSURES,
        );
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.edge, 1);
        assert!(done.enclosures > 0);
        let (kept, done) = excluded(
            &k,
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            d,
            whole,
            edge(),
            MAX_DROP_ENCLOSURES,
        );
        assert_eq!(kept, edge());
        assert_eq!(done.edge, 0);
        // with no budget left for the enclosure of its side, it is kept too
        let (kept, done) = excluded(
            &k,
            Vec3::new(-2.0, 0.0, 1.0 + 3.0 * TOL),
            d,
            whole,
            edge(),
            0,
        );
        assert_eq!(kept, edge());
        assert!(done.out_of_budget);
    }

    /// A set of plain and edge boxes with no root is one set: a plain box beside an edge box
    /// on the top rim and the crease at u = π, and a second edge box across that crease
    /// (two edge boxes side by side, which the rule for one edge box at a time kept, each
    /// waiting for the other). With the line three tolerances above the rim, every
    /// boundary and crease side is shown farther and every other side faces boxes excluded
    /// by distance: all three are dropped, and so are the two edge boxes alone. With the
    /// line half a tolerance above the rim, the rim at u = π is within the tolerance, and
    /// the whole set is kept.
    #[test]
    fn a_set_of_plain_and_edge_boxes_is_dropped_whole() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        // the crease at u = π, the knot between the second and third quarter arcs
        let c = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][1];
        let d = Vec3::new(4.0, 0.0, 0.0);
        let edges = || {
            vec![
                rec(FloorKind::Edge, [3.1, c], [0.9, 1.0]),
                rec(FloorKind::Edge, [c, 3.2], [0.9, 1.0]),
            ]
        };
        let set = || {
            let mut recs = edges();
            recs.insert(0, rec(FloorKind::NoRoot, [3.0, 3.1], [0.85, 0.95]));
            recs
        };
        let far = Vec3::new(-2.0, 0.0, 1.0 + 3.0 * TOL);
        let (kept, done) = excluded(&k, far, d, whole, set(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.no_root, done.edge, done.across), (1, 2, 0));
        let (kept, done) = excluded(&k, far, d, whole, edges(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.edge, 2);

        let near = Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL);
        let (kept, done) = excluded(&k, near, d, whole, set(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, set());
        assert_eq!(done.no_root + done.edge, 0);
    }

    /// A crease side shared by two members is shown farther by its own enclosure or keeps
    /// the set: a line along y half a tolerance outside the cylinder's crease at u = π
    /// (x = −1) at height 0.5 passes within the tolerance of that knot line, and the two
    /// edge boxes on either side of it are kept; three tolerances outside it, both are
    /// dropped.
    #[test]
    fn a_crease_side_within_the_tolerance_keeps_its_set() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let c = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][1];
        let pair = || {
            vec![
                rec(FloorKind::Edge, [c - 0.05, c], [0.45, 0.55]),
                rec(FloorKind::Edge, [c, c + 0.05], [0.45, 0.55]),
            ]
        };
        let d = Vec3::new(0.0, 2.0, 0.0);
        let at = |h: f64| Vec3::new(-1.0 - h * TOL, -1.0, 0.5);
        let (kept, done) = excluded(&k, at(0.5), d, whole, pair(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, pair());
        assert_eq!(done.edge, 0);
        let (kept, done) = excluded(&k, at(3.0), d, whole, pair(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!(done.edge, 2);
    }

    /// A crease side no other member meets, with nothing but boxes excluded by distance
    /// across it, lies in closed excluded boxes and needs no enclosure: an edge box beside
    /// the crease at u = π half a tolerance from the line (the line of the test above) is
    /// dropped with no side enclosed, alone in the solve, and across a finished leaf with no
    /// record. Across a leaf whose pass did not run, the side must be enclosed, which fails,
    /// and the box is kept; and a box left at the floor that touches it only at a corner
    /// keeps it too.
    #[test]
    fn a_crease_side_alone_reads_the_boxes_across_it() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let c = LineNet::of(&k, &cylinder(&k)).unwrap().unwrap().creases[0][1];
        let edge = || vec![rec(FloorKind::Edge, [c - 0.05, c], [0.45, 0.55])];
        let (p, d) = (
            Vec3::new(-1.0 - 0.5 * TOL, -1.0, 0.5),
            Vec3::new(0.0, 2.0, 0.0),
        );
        let (kept, done) = excluded(&k, p, d, whole, edge(), MAX_DROP_ENCLOSURES);
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.edge, done.enclosures), (1, 0));

        let (a, b) = (
            Extent2d::from_bounds(1.0, 0.0, c, 1.0),
            Extent2d::from_bounds(c, 0.0, 4.0, 1.0),
        );
        let run = |across: SearchedLeaf| {
            let mut leaves = [leaf_of(a, Stop::Finished, Some(edge())), across];
            let done = drop_over(&k, p, d, &mut leaves, MAX_DROP_ENCLOSURES);
            (leaves[0].floor.clone().unwrap(), done)
        };
        let (kept, done) = run(leaf_of(b, Stop::Finished, Some(Vec::new())));
        assert!(kept.is_empty(), "{kept:?}");
        assert_eq!((done.edge, done.across, done.enclosures), (1, 1, 0));
        let (kept, done) = run(leaf_of(b, Stop::NotRun, Some(Vec::new())));
        assert_eq!(kept, edge());
        assert_eq!(done.enclosures, 1);

        let mut recs = edge();
        recs.push(rec(FloorKind::Left, [c, c + 0.05], [0.55, 0.6]));
        let (kept, _) = excluded(&k, p, d, whole, recs.clone(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, recs);
    }

    /// A box excluded by t is a record, and no proof of the gap to the line: a set it
    /// touches is kept.
    #[test]
    fn boxes_excluded_by_t_do_not_fence_a_set() {
        let k = Kernel::default();
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = (
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            Vec3::new(4.0, 0.0, 0.0),
        );
        let recs = vec![
            rec(FloorKind::NoRoot, [1.0, 1.1], [0.4, 0.5]),
            rec(FloorKind::NoRoot, [1.1, 1.2], [0.4, 0.5]),
            FloorBox {
                t: Some(Ivl::new(2.0, 3.0)),
                ..rec(FloorKind::ExcludedByT, [1.2, 1.3], [0.4, 0.5])
            },
        ];
        let (kept, done) = excluded(&k, p, d, whole, recs.clone(), MAX_DROP_ENCLOSURES);
        assert_eq!(kept, recs);
        assert_eq!(done.no_root, 0);
    }

    /// A set on a leaf's border reads the leaf across it: across a finished leaf (or one at
    /// its limit, or a culled strip) with no record touching it, its every touching box is
    /// excluded, and it is dropped; across a leaf whose pass did not run, a leaf whose
    /// records the solve did not keep (no record there is then no proof), or a record that
    /// is not excluded, it is kept. A leaf whose records were not kept also leaves the
    /// components unbuilt.
    #[test]
    fn a_set_on_a_leaf_border_reads_the_leaf_across() {
        let k = Kernel::default();
        let (p, d) = (
            Vec3::new(-2.0, 0.0, 1.0 + 0.5 * TOL),
            Vec3::new(4.0, 0.0, 0.0),
        );
        let (a, b) = (
            Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0),
            Extent2d::from_bounds(1.0, 0.0, 2.0, 1.0),
        );
        let set = || {
            vec![
                rec(FloorKind::NoRoot, [1.0, 1.1], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.4, 0.5]),
                rec(FloorKind::NoRoot, [1.1, 1.2], [0.5, 0.6]),
            ]
        };
        let run = |across: SearchedLeaf| {
            let mut leaves = [across, leaf_of(b, Stop::Finished, Some(set()))];
            let done = drop_over(&k, p, d, &mut leaves, MAX_DROP_ENCLOSURES);
            (leaves[1].floor.clone().unwrap(), done)
        };
        for stop in [Stop::Finished, Stop::AtLimit] {
            let (kept, done) = run(leaf_of(a, stop, Some(Vec::new())));
            assert!(kept.is_empty(), "{stop:?}: {kept:?}");
            assert_eq!((done.no_root, done.across), (3, 3), "{stop:?}");
            // (the side on u = 1 is read across, not enclosed)
            assert_eq!(done.enclosures, 0, "{stop:?}");
        }
        let strip = SearchedLeaf {
            strip: true,
            ..leaf_of(a, Stop::Finished, Some(Vec::new()))
        };
        let (kept, _) = run(strip);
        assert!(kept.is_empty(), "{kept:?}");

        let (kept, done) = run(leaf_of(a, Stop::NotRun, Some(Vec::new())));
        assert_eq!(kept, set());
        assert_eq!(done.no_root, 0);
        let (kept, done) = run(leaf_of(a, Stop::Finished, None));
        assert_eq!(kept, set());
        assert_eq!(done.no_root, 0);
        let left = vec![rec(FloorKind::Left, [0.9, 1.0], [0.4, 0.5])];
        let (kept, _) = run(leaf_of(a, Stop::Finished, Some(left)));
        assert_eq!(kept, set());

        let leaves = [
            leaf_of(a, Stop::Finished, None),
            leaf_of(b, Stop::Finished, Some(set())),
        ];
        let whole = cylinder(&k).natural_uv_domain_with_nurb(&k).unwrap();
        let c = components(&leaves, &leaf_sides(&[a, b], &whole), [0.0, 1.0]);
        assert!(c.over_budget);
        assert_eq!(
            side_record(&leaves, &c, false, [0.0, 1.0]),
            SideRecord::Undecided(vec![[0.0, 1.0]])
        );
    }

    /// A line tangent to the cylinder leaves boxes with no root about the contact, which
    /// the exclusion phase cuts. Its budget running out keeps what it has not decided: the
    /// pass stays finished, the phase spent exactly its budget, and the boxes with no root
    /// it kept hold every piece a phase with its whole budget kept.
    #[test]
    fn an_exclusion_out_of_budget_keeps_its_boxes_and_the_pass_finished() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (p, d) = across(1.0);
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: domain,
            tol: TOL,
        };
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let sides = LeafSides::whole_solve(&domain);
        let run =
            |budget| isolate_within(&net, &line, &domain, &interval, &sides, None, budget).unwrap();
        let no_root = |f: &Isolation| -> Vec<FloorBox> {
            f.floor
                .iter()
                .filter(|r| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge))
                .copied()
                .collect()
        };
        let whole = run(MAX_EXCLUSION_BOXES);
        assert_eq!(whole.stop, Stop::Finished);
        assert!(whole.exclusion.ran && !whole.exclusion.out_of_budget);
        assert!(whole.exclusion.boxes > 2);
        for budget in [0, 2] {
            let short = run(budget);
            assert_eq!(short.stop, Stop::Finished);
            assert!(short.exclusion.out_of_budget);
            assert_eq!(short.exclusion.boxes, budget);
            assert_eq!(short.roots, whole.roots);
            // every piece the whole phase kept lies in a box the short one kept
            let kept = no_root(&short);
            assert!(!kept.is_empty());
            for r in no_root(&whole) {
                assert!(
                    kept.iter().any(|o| within(r.u, o.u) && within(r.v, o.v)),
                    "budget {budget}: {r:?}"
                );
            }
        }
    }

    /// A periodic seam counts as a border (nothing wraps across it): a crossing a ten
    /// thousandth of a turn from the cylinder's seam leaves boxes with no root on the seam,
    /// and every one of them is an edge box.
    #[test]
    fn a_seam_side_counts_as_a_border() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let domain = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (u0, u1) = (domain.min().x, domain.max().x);
        let a = 1.0e-4 * std::f64::consts::TAU;
        let found = pass(
            &k,
            &s,
            Vec3::new(-2.0, a.sin(), 0.5),
            Vec3::new(4.0, 0.0, 0.0),
            [0.0, 1.0],
            TOL,
        );
        assert_eq!(found.stop, Stop::Finished);
        let on_seam: Vec<&FloorBox> = found
            .floor
            .iter()
            .filter(|r| matches!(r.kind, FloorKind::NoRoot | FloorKind::Edge))
            .filter(|r| r.u.lo == u0 || r.u.hi == u1)
            .collect();
        assert!(!on_seam.is_empty(), "{:?}", found.floor);
        assert!(
            on_seam.iter().all(|r| r.kind == FloorKind::Edge),
            "{on_seam:?}"
        );
        // and the side on it is a boundary's
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let b = Scratch {
            u: Ivl::new(u0, 0.1),
            v: Ivl::new(0.4, 0.6),
        };
        let kinds = net.side_kinds(&domain, &LeafSides::whole_solve(&domain), b);
        assert_eq!(
            kinds,
            [
                SideKind::Boundary,
                SideKind::Plain,
                SideKind::Plain,
                SideKind::Plain
            ]
        );
    }

    /// The passes of `doms` (leaves of the solve over the surface's whole domain) on the line
    /// `p + t d`, `t` in `[0, 1]`, the solve's drop over them, and their components.
    fn components_of(
        k: &Kernel,
        s: &Surface,
        p: Vec3,
        d: Vec3,
        tol: f64,
        doms: &[Extent2d],
    ) -> (Vec<SearchedLeaf>, Components) {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let solve = s.natural_uv_domain_with_nurb(k).unwrap();
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: solve,
            tol,
        };
        let sides = leaf_sides(doms, &solve);
        let interval = Extent1d::from_min_max(0.0, 1.0);
        let mut leaves: Vec<SearchedLeaf> = doms
            .iter()
            .zip(&sides)
            .map(|(dom, own)| {
                let f = isolate(&net, &line, dom, &interval, own, None).unwrap();
                SearchedLeaf {
                    domain: *dom,
                    stop: f.stop,
                    floor: Some(f.floor),
                    roots: f.roots,
                    strip: false,
                }
            })
            .collect();
        drop_fenced(&net, &line, &mut leaves, &sides, None, MAX_DROP_ENCLOSURES).unwrap();
        let c = components(&leaves, &sides, [0.0, 1.0]);
        (leaves, c)
    }

    /// A line half the tolerance from the cylinder at a quadrant (u = π/2, a crease of its
    /// net) misses it near t = 0.5, with no root. Split into two leaves at that quadrant,
    /// the boxes it leaves on both sides are one component across the two leaves, holding
    /// boxes left at the floor, its line parameters about 0.5, and nothing undecided
    /// around it but itself. With the second leaf not searched (as a leaf outside the face,
    /// which no strip covers), the boxes on the quadrant face it: they are edge boxes, and
    /// the component touches a stretch nothing searched.
    #[test]
    fn components_join_across_leaf_borders() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        // the net's first crease, the quarter knot at u = π/2
        let q = LineNet::of(&k, &s).unwrap().unwrap().creases[0][0];
        let (p, d) = across(1.0 + 0.5 * TOL);
        let doms = [
            Extent2d::from_bounds(0.0, 0.0, q, 1.0),
            Extent2d::from_bounds(q, 0.0, whole.max().x, 1.0),
        ];
        let (leaves, c) = components_of(&k, &s, p, d, TOL, &doms);
        assert!(leaves.iter().all(|l| l.stop == Stop::Finished));
        assert!(!c.over_budget);
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        let one = &c.list[0];
        let from: std::collections::BTreeSet<usize> =
            one.boxes.iter().map(|&i| c.boxes[i].leaf).collect();
        assert_eq!(from.len(), 2);
        assert!(one.holds.left > 0);
        assert!(one.roots.is_empty());
        assert!(one.t.lo <= 0.5 && 0.5 <= one.t.hi, "{:?}", one.t);
        assert!(!one.reaches_end);
        assert_eq!(one.touches, Touches::default());
        assert_eq!(one.contact, Contact::Undecided);
        assert_eq!(
            side_record(&leaves, &c, false, [0.0, 1.0]),
            SideRecord::Undecided(vec![[one.t.lo, one.t.hi]])
        );

        let (leaves, c) = components_of(&k, &s, p, d, TOL, &doms[..1]);
        assert_eq!(c.list.len(), 1, "{:?}", c.list);
        assert!(c.list[0].touches.unsearched);
        let floor = leaves[0].floor.clone().unwrap();
        let mut on_quadrant = floor.iter().filter(|r| r.u.hi == q);
        assert!(on_quadrant.clone().count() > 0);
        assert!(on_quadrant.all(|r| matches!(r.kind, FloorKind::Edge | FloorKind::Left)));
    }

    /// A line tangent to the torus's crown circle at 45°, tolerance 1e-6, touches it once.
    /// The pass stops at its limit there (the fold is singular along the crown), and what it
    /// kept, the boxes it did not visit among them, is one component: in a leaf at the
    /// limit, about the contact at t = 0.5.
    #[test]
    fn the_crown_tangency_at_45_degrees_is_one_component() {
        let k = Kernel::default();
        let s = torus(&k);
        let c = 30.0 / 2.0_f64.sqrt();
        let along = Vec3::new(-1.0, 1.0, 0.0) * (1.0 / 2.0_f64.sqrt());
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (leaves, comps) = components_of(
            &k,
            &s,
            Vec3::new(c, c, 5.0) - 10.0 * along,
            20.0 * along,
            1.0e-6,
            &[whole],
        );
        assert_eq!(leaves[0].stop, Stop::AtLimit);
        assert_eq!(comps.list.len(), 1, "{:?}", comps.list);
        let one = &comps.list[0];
        assert!(one.touches.unfinished);
        assert!(one.holds.unvisited > 0);
        assert!(one.t.lo <= 0.5 && 0.5 <= one.t.hi, "{:?}", one.t);
    }

    /// A culled strip the line passes farther than the tolerance from is excluded by
    /// distance whole: the quarter of the cylinder from u = π to 3π/2 (x and y below 0) and
    /// the line across the cylinder at y = 1 + 2 tol, at least 1 from it. The strip finishes
    /// with no record, and as a searched leaf with no record it leaves the side record
    /// complete.
    #[test]
    fn a_culled_strip_excluded_by_distance_reads_as_searched() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let (c1, c2) = (net.creases[0][1], net.creases[0][2]);
        let quarter = Extent2d::from_bounds(c1, 0.0, c2, 1.0);
        let (p, d) = across(1.0 + 2.0 * TOL);
        let line = PassLine {
            point: p,
            vector: d,
            span: [0.0, 1.0],
            solve_domain: whole,
            tol: TOL,
        };
        let found = enclose_strip(&net, &line, &quarter, None).unwrap();
        assert_eq!(found.stop, Stop::Finished);
        assert!(found.floor.is_empty(), "{:?}", found.floor);
        assert!(found.roots.is_empty());
        let leaves = [SearchedLeaf {
            strip: true,
            ..leaf_of(quarter, found.stop, Some(found.floor))
        }];
        let c = components(&leaves, &leaf_sides(&[quarter], &whole), [0.0, 1.0]);
        assert!(c.list.is_empty());
        assert_eq!(
            side_record(&leaves, &c, false, [0.0, 1.0]),
            SideRecord::Complete
        );
    }

    /// A culled strip that runs out of its budget keeps what it left undecided with its line
    /// parameters: the quarter of the cylinder from u = 0 to π/2 (x and y above 0), whose
    /// feet on the line across the cylinder at y = 0.5 lie in t = (x + 1) / 2, [0.5, 1],
    /// with the line's own interval [−1, 2]. Stopped after three enclosures, it is at its
    /// limit, every box it kept is undecided with its line parameters, and the side record
    /// is undecided only inside the line parameters of the quarter's enclosure (about
    /// [0.5, 1], widened by the tolerance and the enclosure), not over the whole line.
    /// With its whole budget it finishes, keeping only boxes about the crossing at
    /// x = √0.75, t = (1 + √0.75) / 2.
    #[test]
    fn a_culled_strip_out_of_budget_is_undecided_over_its_boxes_only() {
        let k = Kernel::default();
        let s = cylinder(&k);
        let net = LineNet::of(&k, &s).unwrap().unwrap();
        let whole = s.natural_uv_domain_with_nurb(&k).unwrap();
        let quarter = Extent2d::from_bounds(0.0, 0.0, net.creases[0][0], 1.0);
        let (p, d) = across(0.5);
        let span = [-1.0, 2.0];
        let line = PassLine {
            point: p,
            vector: d,
            span,
            solve_domain: whole,
            tol: TOL,
        };
        let found = enclose_strip_within(&net, &line, &quarter, None, 3).unwrap();
        assert_eq!(found.stop, Stop::AtLimit);
        assert_eq!(found.boxes, 3);
        assert!(!found.floor.is_empty());
        assert!(found
            .floor
            .iter()
            .all(|r| r.kind == FloorKind::Culled && r.t.is_some()));
        let leaves = [SearchedLeaf {
            strip: true,
            ..leaf_of(quarter, found.stop, Some(found.floor))
        }];
        let c = components(&leaves, &leaf_sides(&[quarter], &whole), span);
        let record = side_record(&leaves, &c, false, span);
        let SideRecord::Undecided(spans) = &record else {
            panic!("{record:?}");
        };
        // (inside the line parameters of the quarter's own enclosure, which lie inside the
        // line's interval)
        let scratch = ScratchLine::of(&line, &Extent1d::from_min_max(-1.0, 2.0)).unwrap();
        let (u, v) = (Ivl::new(0.0, net.creases[0][0]), Ivl::new(0.0, 1.0));
        let (_, feet) = scratch.feet(&enclosed(&net, &scratch, u, v).unwrap().r);
        assert!(feet.lo > -1.0 && feet.hi < 2.0, "{feet:?}");
        for &[lo, hi] in spans {
            assert!(lo >= feet.lo && hi <= feet.hi, "{spans:?} {feet:?}");
        }

        let done = enclose_strip(&net, &line, &quarter, None).unwrap();
        assert_eq!(done.stop, Stop::Finished);
        assert!(!done.floor.is_empty());
        let at = (1.0 + 0.75_f64.sqrt()) / 2.0;
        for r in &done.floor {
            assert_eq!(r.kind, FloorKind::Culled, "{r:?}");
            assert!(r.t.is_some_and(|t| (t.mid() - at).abs() < 1.0e-2), "{r:?}");
        }
    }

    /// The side record: not run when no leaf's pass ran; complete with no component and
    /// every cull's strip placed; undecided over each component's line parameters, and over
    /// the whole of the line's interval with a cull whose strip could not be placed.
    #[test]
    fn the_side_record_has_three_states() {
        let domain = Extent2d::from_bounds(0.0, 0.0, 1.0, 1.0);
        let leaf = |stop| leaf_of(domain, stop, Some(Vec::new()));
        let span = [0.0, 1.0];
        let none = Components::default();
        assert_eq!(
            side_record(&[leaf(Stop::NotRun)], &none, true, span),
            SideRecord::NotRun
        );
        assert_eq!(
            side_record(&[leaf(Stop::Finished)], &none, false, span),
            SideRecord::Complete
        );
        assert_eq!(
            side_record(&[leaf(Stop::Finished)], &none, true, span),
            SideRecord::Undecided(vec![span])
        );
        assert_eq!(
            side_record(
                &[leaf(Stop::Finished)],
                &Components::over_budget(),
                false,
                span
            ),
            SideRecord::Undecided(vec![span])
        );
        // a leaf not run among others is a component over the whole line
        let leaves = [leaf(Stop::Finished), leaf(Stop::NotRun)];
        let c = components(&leaves, &leaf_sides(&[domain, domain], &domain), span);
        assert_eq!(c.list.len(), 1);
        assert!(c.list[0].touches.not_run);
        assert_eq!(
            side_record(&leaves, &c, false, span),
            SideRecord::Undecided(vec![span])
        );
        // (a leaf whose pass stopped at its limit beside one not run: undecided too)
        let leaves = [leaf(Stop::AtLimit), leaf(Stop::NotRun)];
        let c = components(&leaves, &leaf_sides(&[domain, domain], &domain), span);
        assert_eq!(
            side_record(&leaves, &c, false, span),
            SideRecord::Undecided(vec![span])
        );

        // every solved leaf not run: not run, whatever its strips found (a strip that
        // finished with a box kept, one with none, one at its limit) and with a cull unplaced
        let strip = |stop, floor: Vec<FloorBox>| SearchedLeaf {
            strip: true,
            ..leaf_of(domain, stop, Some(floor))
        };
        let kept = vec![FloorBox {
            t: Some(Ivl::new(0.4, 0.6)),
            ..rec(FloorKind::Culled, [0.2, 0.3], [0.2, 0.3])
        }];
        let leaves = [
            leaf(Stop::NotRun),
            strip(Stop::Finished, kept.clone()),
            strip(Stop::Finished, Vec::new()),
            strip(Stop::AtLimit, kept.clone()),
        ];
        let sides = leaf_sides(&[domain; 4], &domain);
        let c = components(&leaves, &sides, span);
        assert!(!c.list.is_empty());
        for unplaced in [false, true] {
            assert_eq!(side_record(&leaves, &c, unplaced, span), SideRecord::NotRun);
        }

        // no solved leaf (every leaf culled): the strips decide
        let only = |strips: &[SearchedLeaf], unplaced: bool| {
            let sides = leaf_sides(&vec![domain; strips.len()], &domain);
            side_record(strips, &components(strips, &sides, span), unplaced, span)
        };
        assert_eq!(
            only(&[strip(Stop::Finished, Vec::new())], false),
            SideRecord::Complete
        );
        assert_eq!(
            only(&[strip(Stop::Finished, Vec::new())], true),
            SideRecord::Undecided(vec![span])
        );
        let SideRecord::Undecided(spans) = only(&[strip(Stop::AtLimit, kept.clone())], false)
        else {
            panic!("a strip's kept box is undecided");
        };
        assert_eq!(spans, vec![[0.4, 0.6]]);
        assert_eq!(
            only(&[strip(Stop::NotRun, Vec::new())], false),
            SideRecord::NotRun
        );
        // (a strip not run beside one that ran is nothing known: undecided over the line)
        assert_eq!(
            only(
                &[
                    strip(Stop::NotRun, Vec::new()),
                    strip(Stop::Finished, Vec::new())
                ],
                false
            ),
            SideRecord::Undecided(vec![span])
        );
    }
}
