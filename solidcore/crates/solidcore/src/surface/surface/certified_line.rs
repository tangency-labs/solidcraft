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
//! 1. **Excluded** when its enclosure proves every point of the surface over it farther
//!    than the tolerance from the line, or its line parameters outside the leaf's interval.
//!    The tolerance is the leaf solve's own: a line passing within it is a hit there
//!    (`SmSurface::LocalCurveIntersect`), so a box is never dropped for merely missing the
//!    line.
//! 2. **Regular** when the contraction `q = ‖I − Y F′(X)‖` of Krawczyk's operator over the
//!    box is below 1: every matrix of the interval Jacobian is then nonsingular, `F` is
//!    injective on the box (`F(a) − F(b) = A (a − b)` with each row of `A` a gradient at its
//!    own point of the box, so `A` lies in the hull: the reason a normal map at single points
//!    proves nothing), and the box holds at most one root, with any line parameter (`F′`
//!    does not depend on `t`). Krawczyk's operator then decides: its image apart from the
//!    box, no root; strictly inside, exactly one. A root on the box's border passes neither
//!    test from inside: it is certified in a small box about it
//!    ([`crate::geom::krawczyk::krawczyk_region`], epsilon-inflation), and the box is
//!    decided once the hull of the two is regular too.
//! 3. **Left** when it is neither and is at the floor: its image is no larger than the
//!    tolerance, it cannot be halved, or the line runs along the surface there. That last
//!    is a box over which the surface lies within the tolerance of a plane through the line
//!    and the contact is not an isolated one: the system of the line's fold on the surface,
//!    `f₁ = 0`, `∂(f₁, f₂)/∂(u, v) = 0` (`f₁`, `f₂` the distances from two perpendicular
//!    planes through the line, the first holding the surface normal at the box's middle),
//!    has no regular interval Jacobian over the box. An isolated tangent contact, a near
//!    miss or the stretch between the two crossings of a shallow secant has one (the fold
//!    is a point there), and such a box is halved on, so that two crossings a hair apart are
//!    still certified one by one; along a line lying in the surface, or beside it, the
//!    determinant vanishes identically and no halving ever would decide the box. A box left
//!    holds what Krawczyk's test cannot decide (a tangent contact, a pole, an apex, a line
//!    in the surface); this pass reports nothing for it, and the guesses' answer for it
//!    stands as it was.
//! 4. **Halved** otherwise, across the parameter direction that carries the surface farther
//!    from the line over the box (the larger of `|S_u × D| Δu` and `|S_v × D| Δv`): seen
//!    along the line the halves are then about as wide as they are long, which is what
//!    makes a box regular, and a direction the line runs along is not cut for nothing. A
//!    direction in which the box's image is already within half the tolerance is at the
//!    floor's size and is not cut again (both there: step 3).
//!
//! A certified root is refined by Newton's method inside the box it is the only root of,
//! and reported only when the gap `|S − L|` at the reported point is itself enclosed within
//! the tolerance.
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`LineNet`], [`isolate`], [`CertifiedRoot`], [`Isolation`] | (not upstream) |
//!
//! Deviations: none (no upstream counterpart). Bounds: no recursion (a work list); a box is
//! halved until its middle is one of its ends; at most [`MAX_SCRATCH_BOXES`] boxes a leaf,
//! past which the pass stops for that leaf and reports what it has certified (each root
//! stands on its own proof); at most [`crate::geom::krawczyk::KRAWCZYK_STEPS`] Krawczyk
//! steps a box.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests

use crate::blend::fillet_section::interval::{round_down, round_up, Ivl};
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
use crate::kernel::{CancelToken, Kernel};
use crate::math::{Point3, Vec3};
use crate::nurbs::b_spline_surface::BSplineSurfaceForm;
use crate::surface::surface::Surface;

const OP: &str = "surface::surface::certified_line::isolate";

/// The most boxes one leaf's pass encloses. A leaf with a transversal crossing takes some
/// tens (the issue's torus leaf with two crossings at 2° and 2.5°: 63); the most measured
/// over the kernel's goldens and cases is recorded with the pull request that set this,
/// and the bound is several times that. Past it the pass stops for the leaf.
pub(crate) const MAX_SCRATCH_BOXES: usize = 1 << 12;

/// Three intervals: a vector's components, or a box in `(u, v, t)`.
type I3 = [Ivl; 3];

/// The exact net of a surface the certified pass runs on, with its parameter range.
#[derive(Debug, Clone)]
pub(crate) struct LineNet {
    net: Nurbs,
    /// The knots' range in u and in v.
    range: [[f64; 2]; 2],
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
        Ok(Some(Self { net, range: [u, v] }))
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
    /// The leaf's line interval.
    interval: [f64; 2],
    /// The tolerance as a distance, and as a stretch of the line's parameter (rounded up).
    tol: f64,
    tol_t: f64,
    /// `(tol |D|)²`, rounded up.
    tol_cross2: f64,
}

impl ScratchLine {
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
    /// Whether every box was decided, excluded or left: false when the pass stopped at
    /// [`MAX_SCRATCH_BOXES`].
    pub finished: bool,
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

/// The certified crossings of the line `point + t vector` with the surface of `net` over
/// `domain`, for `t` in `interval`, within `tol` (module docs).
///
/// # Errors
///
/// A cancel.
pub(crate) fn isolate(
    net: &LineNet,
    point: Point3,
    vector: Vec3,
    domain: &Extent2d,
    interval: &Extent1d,
    tol: f64,
    cancel: Option<&CancelToken>,
) -> Result<Isolation> {
    let mut out = Isolation::default();
    let d = f3(vector);
    let d2 = length_squared(&d.map(Ivl::from));
    let usable = tol > 0.0 && tol.is_finite() && d2.lo > 0.0 && d2.is_finite();
    let (lo, hi) = (domain.min(), domain.max());
    let (u, v) = (Ivl::new(lo.x, hi.x), Ivl::new(lo.y, hi.y));
    if !usable || !net.holds(u, v) || !(interval.min() <= interval.max()) {
        // nothing can be proved of such a line or domain: the guesses alone answer
        return Ok(out);
    }
    let tol_i = Ivl::from(tol);
    let line = ScratchLine {
        p: f3(point),
        d,
        d2,
        interval: [interval.min(), interval.max()],
        tol,
        tol_t: (tol_i / d2.sqrt()).hi,
        tol_cross2: (tol_i * tol_i * d2).hi,
    };
    let mut work = vec![Scratch { u, v }];
    out.finished = true;
    while let Some(b) = work.pop() {
        if out.boxes >= MAX_SCRATCH_BOXES {
            out.finished = false;
            break;
        }
        if out.boxes % 64 == 0 {
            if let Some(token) = cancel {
                token.check(OP)?;
            }
        }
        out.boxes += 1;
        match visit(net, &line, b) {
            Visit::Done(None) => {}
            Visit::Done(Some(root)) => {
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
                if !known {
                    out.roots.push(root);
                }
            }
            Visit::Left => out.left += 1,
            Visit::Unshown => out.unshown += 1,
            Visit::Halve(first, second) => {
                work.push(second);
                work.push(first);
            }
        }
    }
    Ok(out)
}

/// What a box of the work list becomes.
enum Visit {
    /// Decided or excluded, with its root if it has one.
    Done(Option<CertifiedRoot>),
    /// A certified root that is not reported ([`Isolation::unshown`]).
    Unshown,
    /// At the floor.
    Left,
    /// Its two halves, the first to visit first.
    Halve(Scratch, Scratch),
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
    let halved = || {
        let split_u = b.u.hi - b.u.lo >= b.v.hi - b.v.lo;
        halves(b, split_u, if split_u { um } else { vm })
    };
    // the surface's point at the box's middle, about which the box is enclosed: the
    // quotient's terms are then of the size of the box, not of its distance from the origin
    let Some(s_mid) = net.at(um, vm, line.p) else {
        return halved();
    };
    let [m0, m1, m2] = mids(&s_mid);
    let [p0, p1, p2] = line.p;
    let about = [p0 + m0, p1 + m1, p2 + m2];
    let Some([s, s_u, s_v]) = net.over(b.u, b.v, about) else {
        return halved();
    };
    let r = line.off_the_start(&s, about);

    // 1. farther than the tolerance from the line everywhere, or off the leaf's interval
    if length_squared(&cross(line.d, &r)).lo > line.tol_cross2 {
        return Visit::Done(None);
    }
    let foot = line.parameter(&r);
    let t = Ivl::new(foot.lo - line.tol_t, foot.hi + line.tol_t);
    let [t0, t1] = line.interval;
    if !(t.hi >= t0 && t.lo <= t1) {
        return Visit::Done(None);
    }

    // 2. a regular interval Jacobian: at most one root, and Krawczyk's test for it
    if let Some(found) = regular(net, line, b, &s_mid, [&s_u, &s_v], t) {
        match found {
            Regular::Empty => return Visit::Done(None),
            Regular::Root(enclosure, unique) => {
                return match refined(net, line, enclosure, unique) {
                    Some(root) => Visit::Done(Some(root)),
                    None => Visit::Unshown,
                };
            }
            Regular::Open => {}
        }
    }

    // 3. the floor: no larger than the tolerance, the line running along the surface, or
    // not to be halved
    let (du, dv) = (b.u.hi - b.u.lo, b.v.hi - b.v.lo);
    let (along_u, along_v) = (
        round_up(length_squared(&s_u).hi.sqrt() * du),
        round_up(length_squared(&s_v).hi.sqrt() * dv),
    );
    // how far each direction carries the surface from the line over the box, times |D|
    let (off_u, off_v) = (
        length_squared(&cross(line.d, &s_u)).hi.sqrt() * du,
        length_squared(&cross(line.d, &s_v)).hi.sqrt() * dv,
    );
    if !(along_u.is_finite() && along_v.is_finite() && off_u.is_finite() && off_v.is_finite()) {
        return Visit::Left;
    }
    // a direction is cut while the box's image along it is longer than half the tolerance
    let (cut_u, cut_v) = (
        round_up(along_u + along_u) > line.tol,
        round_up(along_v + along_v) > line.tol,
    );
    if !cut_u && !cut_v {
        return Visit::Left;
    }
    if let Some(m) = in_a_plane_through_the_line(line, &r, t, [&s_u, &s_v]) {
        if fold_is_a_point(net, line, b, about, m, [&s_u, &s_v]) == Some(false) {
            return Visit::Left;
        }
    }

    // 4. halve across the direction that carries the surface farther from the line (the
    // longer one where neither does: a box the line runs along in both)
    let split_u = if cut_u != cut_v {
        cut_u
    } else if off_u > 0.0 || off_v > 0.0 {
        off_u >= off_v
    } else {
        along_u >= along_v
    };
    halves(b, split_u, if split_u { um } else { vm })
}

/// The two halves of `b` cut at `mid` in u (`split_u`) or in v; the box is left where
/// `mid` is not strictly inside it (a box one rounding wide).
fn halves(b: Scratch, split_u: bool, mid: f64) -> Visit {
    let (lo, hi) = if split_u {
        (b.u.lo, b.u.hi)
    } else {
        (b.v.lo, b.v.hi)
    };
    if !(mid > lo && mid < hi) {
        return Visit::Left;
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

    /// The pass over the whole of `s`, for `t` in `[t0, t1]`.
    fn pass(k: &Kernel, s: &Surface, p: Vec3, d: Vec3, [t0, t1]: [f64; 2], tol: f64) -> Isolation {
        let net = LineNet::of(k, s).unwrap().unwrap();
        let domain = s.natural_uv_domain_with_nurb(k).unwrap();
        let interval = Extent1d::from_min_max(t0, t1);
        isolate(&net, p, d, &domain, &interval, tol, None).unwrap()
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
        assert!(found.finished);
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
            assert!(found.finished);
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
            assert!(found.finished);
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
        assert!(found.finished);
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
        assert!(found.finished);
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
        assert!(found.finished);
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
            assert!(found.finished, "gap {gap}: {} boxes", found.boxes);
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
        assert!(found.finished, "{} boxes", found.boxes);
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
        assert!(found.finished, "{} boxes", found.boxes);
        assert!(found.roots.is_empty(), "{:?}", found.roots);
        assert!(found.left > 0);
        assert!(found.boxes < 500, "{} boxes", found.boxes);
    }
}
