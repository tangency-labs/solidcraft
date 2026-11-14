//! Closed loft sections matched for the loft's walls: each wound the way the one before it is,
//! cut to the same vertices, and started at the segment that joins the first segment of the
//! section before it.
//!
//! Upstream: none (not in SMLib). SolidCraft's loft section matching (`crates/kernel/src/
//! build.rs` `loft_numbered_loops`, `loft_pairing.rs`, and the circle re-splitting of
//! `numbered.rs`), moved into the kernel so no application builds it.
//!
//! | Rust                       | Upstream |
//! |----------------------------|----------|
//! | [`match_loft_sections`]    | (none; SolidCraft's `loft_numbered_loops`) |
//! | [`LoftSection`]            | (none) |
//! | [`MatchedSection`]         | (none; SolidCraft's `NumberedLoop` after matching) |
//! | `correspond` (private)     | (none; SolidCraft's `loft_pairing::correspond`) |
//! | `circle_breaks` (private)  | (none; SolidCraft's `numbered::circle_breaks`) |
//!
//! **Winding.** Each section winds the way the section before it does about the normal they
//! share (its plane's normal, turned over where it faces back from the one before), so sections
//! whose planes turn on round the loft (a bar bent half a turn) keep one winding.
//!
//! **Pairing.** A loft's side faces run from a point of one section to a point of the next, so
//! every section needs the same number of vertices, in the same order round. Each section that
//! is not a full circle is paired with the one before it ([`correspond`]): each vertex of either
//! is paired with a vertex of the other or left alone, and a lone vertex then splits the other
//! section's segment where it lands, so both gain the vertices only the other had, and every
//! section before takes the same cuts. The pairing is the one that costs least, round both loops
//! in order. The cost of joining two points is how far apart they are, each section moved to its
//! centroid and scaled to unit area, in one frame (the first plane's axes, carried from plane to
//! plane by the least rotation between their normals), plus how much their tangents differ on
//! either side; a lone vertex costs a little more ([`UNPAIRED`]), so that two points that line up
//! pair. The cyclic alignment is a shortest path through the grid of both loops' vertices, found
//! for every start by divide and conquer: two best paths from different starts can always be
//! chosen not to cross, so each start's path is searched between its neighbours' ([`best_walk`]).
//! Between two pairs, a lone vertex lands at the same share of the length between the pairs; with
//! no pair at all, on the nearest point. A segment of no length has no vertex of its own: it is
//! left out of the pairing and of the matched section, and the piece before it runs over it.
//!
//! **Circles.** A full circle has no corners of its own: it is split at the directions of the
//! section before it (the first section that is not a circle when it comes first, or the one
//! with the most segments when every section is a circle) and started where its vertices line up
//! best with that section's.
//!
//! Deviations: the divide and conquer over the starts recurses at most [`MAX_DEPTH`] (64) deep,
//! well above the log2 of any grid within [`MAX_CELLS`] (about 22).

// The range tests refuse NaN by negation (`!(x > y)`), kept as written.
#![allow(clippy::neg_cmp_op_on_partial_ord, clippy::nonminimal_bool)]

use std::collections::{BTreeMap, BTreeSet};

use crate::error::{sweep::SweepFailure, Result, SolidError};
use crate::math::{Point2, Vec2, Vec3};
use crate::prism::profile::{casteljau, Seg2, H2};

/// The weight of the tangent terms against the distance between the points.
const TURN: f64 = 1.0;
/// What a lone vertex costs on top of its distance and tangents: less than any corner's
/// mismatch, so a sharp corner pairs with a sharp corner, but enough that two points which line
/// up pair rather than both splitting the other section.
const UNPAIRED: f64 = 0.1;
/// The most cells the grid of a pairing may have ((m + 1) by (n + 1): two sections of about 2000
/// segments each). Each start's search stores one byte a cell, and the search over every start
/// visits about log2(n) times the grid. Two sections with a larger grid are paired from one start
/// through a band of the grid ([`banded_walk`]), so no loft is refused for its size below the caller's
/// `max_segments` and [`LARGEST_BANDED`].
pub const MAX_CELLS: usize = 4_000_000;
/// The deepest the divide and conquer over the starts goes (log2 of the vertex count: about 22
/// for any loop within [`MAX_CELLS`]).
pub const MAX_DEPTH: usize = 64;
/// Shares of a gap closer than this are the same point: two lone vertices landing on each other
/// pair.
const SAME_SHARE: f64 = 1e-9;
/// The shortest edge a match makes where the sections have none (the kernel's zone tolerance,
/// `SM_ZONE_TOL_3D`): two lone vertices that would land closer than this to each other pair, and
/// cuts of one segment are kept at least this far apart and from its ends where it has room.
const MIN_EDGE: f64 = 1e-5;
/// How close a curved segment's parameter is found for a length along it, as a share of the
/// segment's length ([`param_at`]).
const LENGTH_TOL: f64 = 1e-12;
/// How many chords a curved segment's nearest point is found on.
const CHORDS: usize = 8;
/// The smallest arc a circle is split into (radians).
const MIN_SWEEP: f64 = 1e-9;
/// The least step between two cuts of one segment, and between a cut and the segment's ends,
/// when lone vertices land on the same point of it.
const CUT_GAP: f64 = 1e-9;

/// How far from perpendicular (the cosine between them) a section plane's axes may be, and how far
/// from one their lengths.
const AXES_TOL: f64 = 1e-6;

const OP: &str = "sweep::loft_sections";

/// The eight-point Gauss-Legendre rule on `[-1, 1]`.
const GAUSS_8: [(f64, f64); 8] = [
    (-0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
    (-0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (-0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (-0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.183_434_642_495_649_8, 0.362_683_783_378_362),
    (0.525_532_409_916_329, 0.313_706_645_877_887_3),
    (0.796_666_477_413_626_7, 0.222_381_034_453_374_47),
    (0.960_289_856_497_536_3, 0.101_228_536_290_376_26),
];
/// The pieces a curved segment's length and area integrals are summed over.
const PIECES: usize = 16;

/// A closed loft section: its plane (origin and unit axes, the normal `x × y`) and its loop in
/// the plane's coordinates, end to end.
#[derive(Clone, Debug)]
pub struct LoftSection {
    /// The plane's origin.
    pub origin: Vec3,
    /// The plane's x axis.
    pub x: Vec3,
    /// The plane's y axis.
    pub y: Vec3,
    /// The loop, in the plane's coordinates.
    pub segs: Vec<Seg2>,
}

/// A section as the walls are built through it: its segments, and for each the segments of the
/// section as given ([`LoftSection::segs`] indices) it runs over.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchedSection {
    /// The segments, in the plane's coordinates.
    pub segs: Vec<Seg2>,
    /// For each segment, the given segments it runs over (increasing).
    pub from: Vec<Vec<usize>>,
}

fn invariant(reason: &'static str) -> SolidError {
    SolidError::PortInvariant {
        operation: OP,
        reason,
    }
}

fn unpaired() -> SolidError {
    invariant("loft sections could not be paired")
}

/// `i` round a loop of `n` (0 for an empty loop).
fn wrap(i: usize, n: usize) -> usize {
    i.checked_rem(n).unwrap_or(0)
}

/// `v` started at its element `k` (unchanged for `k` past its end).
fn rotated<T: Clone>(v: &[T], k: usize) -> Vec<T> {
    if k >= v.len() {
        return v.to_vec();
    }
    v.iter().skip(k).chain(v.iter().take(k)).cloned().collect()
}

// --- Plane vectors -------------------------------------------------------------------------

fn dot(a: Vec2, b: Vec2) -> f64 {
    a.x * b.x + a.y * b.y
}

fn len2(a: Vec2) -> f64 {
    dot(a, a)
}

fn len(a: Vec2) -> f64 {
    len2(a).sqrt()
}

fn dist(a: Vec2, b: Vec2) -> f64 {
    len(a - b)
}

fn finite(a: Vec2) -> bool {
    a.x.is_finite() && a.y.is_finite()
}

fn unit(a: Vec2) -> Option<Vec2> {
    let l = len(a);
    (l > 0.0 && l.is_finite()).then(|| a * (1.0 / l))
}

fn from_angle(t: f64) -> Vec2 {
    Vec2::new(t.cos(), t.sin())
}

fn angle(a: Vec2) -> f64 {
    a.y.atan2(a.x)
}

fn perp(a: Vec2) -> Vec2 {
    Vec2::new(-a.y, a.x)
}

fn dot3(a: Vec3, b: Vec3) -> f64 {
    a.x * b.x + a.y * b.y + a.z * b.z
}

fn cross3(a: Vec3, b: Vec3) -> Vec3 {
    Vec3::new(
        a.y * b.z - a.z * b.y,
        a.z * b.x - a.x * b.z,
        a.x * b.y - a.y * b.x,
    )
}

fn unit3(a: Vec3) -> Option<Vec3> {
    let l = dot3(a, a).sqrt();
    (l > 1e-300 && l.is_finite()).then(|| a * (1.0 / l))
}

/// A unit vector perpendicular to `n`.
fn any_perp(n: Vec3) -> Vec3 {
    let other = if n.x.abs() < 0.9 {
        Vec3::new(1.0, 0.0, 0.0)
    } else {
        Vec3::new(0.0, 1.0, 0.0)
    };
    unit3(cross3(n, other)).unwrap_or(Vec3::new(1.0, 0.0, 0.0))
}

/// `axis` (perpendicular to the unit normal `from`) carried onto the plane of the unit normal
/// `to` by the least rotation taking `from` onto `to`.
fn carried(axis: Vec3, from: Vec3, to: Vec3) -> Vec3 {
    let k = cross3(from, to);
    let (sin, cos) = (dot3(k, k).sqrt(), dot3(from, to));
    let turned = match unit3(k) {
        Some(k) if sin > 1e-12 => {
            axis * cos + cross3(k, axis) * sin + k * (dot3(k, axis) * (1.0 - cos))
        }
        _ => axis,
    };
    unit3(turned - to * dot3(to, turned)).unwrap_or_else(|| any_perp(to))
}

// --- Segments ------------------------------------------------------------------------------

/// The one homogeneous Bézier span of a curved segment.
fn span(s: &Seg2) -> Option<Vec<H2>> {
    let mut spans = s.spans()?;
    (spans.len() == 1).then(|| spans.pop()).flatten()
}

/// The derivative of a homogeneous Bézier span's point, as a plane vector.
fn span_derivative(sp: &[H2], t: f64) -> Vec2 {
    let d = sp.len().saturating_sub(1) as f64;
    let p = casteljau(sp, t);
    let diffs: Vec<H2> = sp
        .iter()
        .zip(sp.iter().skip(1))
        .map(|(a, b)| [(b[0] - a[0]) * d, (b[1] - a[1]) * d, (b[2] - a[2]) * d])
        .collect();
    let dp = casteljau(&diffs, t);
    let (x, y) = (p[0] / p[2], p[1] / p[2]);
    Vec2::new((dp[0] - x * dp[2]) / p[2], (dp[1] - y * dp[2]) / p[2])
}

/// The derivative of a segment at its parameter `t`.
fn derivative(s: &Seg2, t: f64) -> Vec2 {
    match *s {
        Seg2::Line { a, b } => b - a,
        Seg2::Arc {
            radius,
            start,
            sweep,
            ..
        } => perp(from_angle(start + sweep * t)) * (radius * sweep),
        _ => span(s).map_or(Vec2::new(f64::NAN, f64::NAN), |sp| span_derivative(&sp, t)),
    }
}

/// The unit tangent at the parameter `t` (0 or 1), else along the chord, else +x.
fn tangent(s: &Seg2, t: f64) -> Vec2 {
    unit(derivative(s, t))
        .or_else(|| unit(s.end() - s.start()))
        .unwrap_or(Vec2::new(1.0, 0.0))
}

/// `f` summed over a segment's parameter range by Gauss-Legendre on [`PIECES`] pieces.
fn integrate(f: impl Fn(f64) -> f64) -> f64 {
    let mut sum = 0.0;
    for k in 0..PIECES {
        let (a, b) = (k as f64 / PIECES as f64, (k + 1) as f64 / PIECES as f64);
        let (mid, half) = (0.5 * (a + b), 0.5 * (b - a));
        for (x, w) in GAUSS_8 {
            sum += w * half * f(mid + half * x);
        }
    }
    sum
}

/// A segment's length.
fn length(s: &Seg2) -> f64 {
    match *s {
        Seg2::Line { a, b } => dist(a, b),
        Seg2::Arc { radius, sweep, .. } => (radius * sweep).abs(),
        _ => integrate(|t| len(derivative(s, t))),
    }
}

/// The length of a segment from its start to its parameter `t` (in [0, 1]); at 1, [`length`].
fn length_to(s: &Seg2, t: f64) -> f64 {
    match *s {
        Seg2::Line { a, b } => dist(a, b) * t,
        Seg2::Arc { radius, sweep, .. } => (radius * sweep).abs() * t,
        _ => integrate(|u| t * len(derivative(s, t * u))),
    }
}

/// The parameter of the point a length `d` along a segment of length `l` from its start: the
/// share for a line or an arc, and for a curve (whose length is not proportional to its
/// parameter) the root of [`length_to`] by Newton's method inside a bisection bracket, to
/// [`LENGTH_TOL`] of the length.
fn param_at(s: &Seg2, d: f64, l: f64) -> f64 {
    if !(l > 0.0) {
        return 0.5;
    }
    let share = d / l;
    if matches!(s, Seg2::Line { .. } | Seg2::Arc { .. }) || !(share > 0.0 && share < 1.0) {
        return share;
    }
    let (mut lo, mut hi, mut t) = (0.0f64, 1.0f64, share);
    for _ in 0..100 {
        let f = length_to(s, t) - d;
        if !f.is_finite() {
            return share;
        }
        if f.abs() <= LENGTH_TOL * l {
            break;
        }
        if f > 0.0 {
            hi = t;
        } else {
            lo = t;
        }
        let next = t - f / len(derivative(s, t));
        t = if next > lo && next < hi {
            next
        } else {
            0.5 * (lo + hi)
        };
    }
    t
}

/// A homogeneous span split at `t` by de Casteljau: its left and right control points.
fn split_span(sp: &[H2], t: f64) -> (Vec<H2>, Vec<H2>) {
    let mut left = Vec::new();
    let mut right = Vec::new();
    let mut q = sp.to_vec();
    while let (Some(&first), Some(&last)) = (q.first(), q.last()) {
        left.push(first);
        right.push(last);
        q = q
            .iter()
            .zip(q.iter().skip(1))
            .map(|(a, b)| {
                [
                    a[0] + (b[0] - a[0]) * t,
                    a[1] + (b[1] - a[1]) * t,
                    a[2] + (b[2] - a[2]) * t,
                ]
            })
            .collect();
    }
    right.reverse();
    (left, right)
}

/// The plane point of a homogeneous point.
fn point_of(h: H2) -> Point2 {
    Vec2::new(h[0] / h[2], h[1] / h[2])
}

/// A conic's homogeneous control points as a segment (end weights scaled to one).
fn conic(h: &[H2]) -> Option<Seg2> {
    let (p0, p1, p2) = (*h.first()?, *h.get(1)?, *h.get(2)?);
    let w = p1[2] / (p0[2] * p2[2]).sqrt();
    w.is_finite().then(|| Seg2::Conic {
        a: point_of(p0),
        apex: point_of(p1),
        b: point_of(p2),
        w,
    })
}

/// A Bézier of weights one as a segment of `like`'s kind.
fn bezier(like: &Seg2, h: &[H2]) -> Option<Seg2> {
    let p: Vec<Point2> = h.iter().map(|q| point_of(*q)).collect();
    match *like {
        Seg2::Cubic { .. } => Some(Seg2::Cubic {
            p0: *p.first()?,
            p1: *p.get(1)?,
            p2: *p.get(2)?,
            p3: *p.get(3)?,
        }),
        Seg2::Bezier { n, p: old } => {
            let mut q = old;
            for (slot, x) in q.iter_mut().zip(&p) {
                *slot = *x;
            }
            Some(Seg2::Bezier { n, p: q })
        }
        _ => None,
    }
}

/// A segment split at its parameter `t`: the piece before and the piece after.
fn split_at(s: &Seg2, t: f64) -> Option<(Seg2, Seg2)> {
    match *s {
        Seg2::Line { a, b } => {
            let m = a + (b - a) * t;
            Some((Seg2::Line { a, b: m }, Seg2::Line { a: m, b }))
        }
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            let first = sweep * t;
            Some((
                Seg2::Arc {
                    center,
                    radius,
                    start,
                    sweep: first,
                },
                Seg2::Arc {
                    center,
                    radius,
                    start: start + first,
                    sweep: sweep - first,
                },
            ))
        }
        Seg2::Conic { .. } => {
            let (l, r) = split_span(&span(s)?, t);
            Some((conic(&l)?, conic(&r)?))
        }
        Seg2::Cubic { .. } | Seg2::Bezier { .. } => {
            let (l, r) = split_span(&span(s)?, t);
            Some((bezier(s, &l)?, bezier(s, &r)?))
        }
    }
}

/// A loop's signed area (counter-clockwise positive).
fn signed_area(segs: &[Seg2]) -> f64 {
    segs.iter()
        .map(|s| s.area_term().map_or(f64::NAN, |(a, _)| a))
        .sum()
}

/// A loop's centroid by area; with no area, the mean of its vertices.
fn centroid(segs: &[Seg2]) -> Point2 {
    let area = signed_area(segs);
    let (mut sx, mut sy) = (0.0, 0.0);
    for s in segs {
        if let Seg2::Line { a, b } = *s {
            sx += (b.y - a.y) / 6.0 * (a.x * a.x + a.x * b.x + b.x * b.x);
            sy -= (b.x - a.x) / 6.0 * (a.y * a.y + a.y * b.y + b.y * b.y);
        } else {
            sx += integrate(|t| {
                let (p, d) = (s.point_at(t), derivative(s, t));
                0.5 * p.x * p.x * d.y
            });
            sy -= integrate(|t| {
                let (p, d) = (s.point_at(t), derivative(s, t));
                0.5 * p.y * p.y * d.x
            });
        }
    }
    let c = Vec2::new(sx * (1.0 / area), sy * (1.0 / area));
    if area.abs() > 0.0 && finite(c) {
        return c;
    }
    let n = segs.len().max(1) as f64;
    let sum = segs.iter().fold(Vec2::ZERO, |acc, s| acc + s.start());
    sum * (1.0 / n)
}

/// Whether the loop is one full circle: arcs of one centre and radius, each starting where the
/// one before ends, all turning the same way, and together once round. Arcs that turn back over
/// each other (a half circle and its reverse) are not a circle.
fn is_circle(segs: &[Seg2]) -> bool {
    use std::f64::consts::TAU;
    let arcs: Vec<(Vec2, f64, f64, f64)> = segs
        .iter()
        .filter_map(|s| match *s {
            Seg2::Arc {
                center,
                radius,
                start,
                sweep,
            } => Some((center, radius, start, sweep)),
            _ => None,
        })
        .collect();
    let Some(&(c, r, _, w)) = arcs.first() else {
        return false;
    };
    let turned: f64 = arcs.iter().map(|a| a.3).sum();
    arcs.len() == segs.len()
        && arcs
            .iter()
            .all(|a| dist(a.0, c) < 1e-9 && (a.1 - r).abs() < 1e-9 && a.3 * w > 0.0)
        && (turned.abs() - TAU).abs() < 1e-9
        && arcs.iter().zip(arcs.iter().skip(1)).all(|(x, y)| {
            let gap = (y.2 - (x.2 + x.3)).rem_euclid(TAU);
            gap.min(TAU - gap) < 1e-9
        })
}

/// A loop as the matching carries it: its segments and the given segments each runs over.
#[derive(Clone, Debug)]
struct Traced {
    segs: Vec<Seg2>,
    from: Vec<Vec<usize>>,
}

impl Traced {
    fn reversed(&self) -> Traced {
        Traced {
            segs: self.segs.iter().rev().map(Seg2::reversed).collect(),
            from: self.from.iter().rev().cloned().collect(),
        }
    }

    /// Counter-clockwise; clockwise when `back`.
    fn oriented(&self, back: bool) -> Traced {
        let ccw = if signed_area(&self.segs) >= 0.0 {
            self.clone()
        } else {
            self.reversed()
        };
        if back {
            ccw.reversed()
        } else {
            ccw
        }
    }

    fn rotate_left(&mut self, k: usize) {
        if k < self.segs.len() && self.from.len() == self.segs.len() {
            self.segs = rotated(&self.segs, k);
            self.from = rotated(&self.from, k);
        }
    }

    /// Each segment `i` split at the parameters `cuts[i]` (increasing, inside (0, 1)), every
    /// piece over what its segment ran over.
    fn cut(&self, cuts: &[Vec<f64>]) -> Option<Traced> {
        let mut out = Traced {
            segs: Vec::new(),
            from: Vec::new(),
        };
        for (i, s) in self.segs.iter().enumerate() {
            let from = self.from.get(i)?;
            let (mut rest, mut done) = (*s, 0.0);
            // A conic's remaining piece as homogeneous control points: `conic` scales a piece's
            // end weights to one, which changes its parameter other than linearly, so the next
            // cut is taken from these, whose parameter is the segment's own, moved linearly.
            let mut rest_h = match s {
                Seg2::Conic { .. } => Some(span(s)?),
                _ => None,
            };
            for &t in cuts.get(i).map_or(&[][..], Vec::as_slice) {
                if !(t > done && t < 1.0) {
                    return None;
                }
                // The remaining piece runs from `done` to 1 of the segment.
                let u = (t - done) / (1.0 - done);
                let (piece, after) = match rest_h.take() {
                    Some(h) => {
                        let (l, r) = split_span(&h, u);
                        let pieces = (conic(&l)?, conic(&r)?);
                        rest_h = Some(r);
                        pieces
                    }
                    None => split_at(&rest, u)?,
                };
                out.segs.push(piece);
                out.from.push(from.clone());
                rest = after;
                done = t;
            }
            out.segs.push(rest);
            out.from.push(from.clone());
        }
        Some(out)
    }

    /// The loop without its segments of no length: the piece before each runs over it too.
    /// `None` when the loop has no length.
    fn without_points(&self) -> Option<Traced> {
        let lengths: Vec<f64> = self.segs.iter().map(length).collect();
        if lengths.iter().any(|l| !l.is_finite())
            || self
                .segs
                .iter()
                .any(|s| !finite(s.start()) || !finite(s.end()))
        {
            return None;
        }
        let keep: Vec<bool> = lengths.iter().map(|l| *l > 0.0).collect();
        let first = keep.iter().position(|k| *k)?;
        let n = self.segs.len();
        let mut out = Traced {
            segs: Vec::new(),
            from: Vec::new(),
        };
        for step in 0..n {
            let i = wrap(first + step, n);
            let from = self.from.get(i)?;
            if keep.get(i).copied().unwrap_or(false) {
                out.segs.push(*self.segs.get(i)?);
                out.from.push(from.clone());
            } else if let Some(last) = out.from.last_mut() {
                last.extend(from.iter().copied());
                last.sort_unstable();
                last.dedup();
            }
        }
        // The segments of no length before the first kept one came round last, onto the piece
        // before them; the loop still starts at the first kept segment from its start.
        Some(out)
    }
}

// --- Pairing -------------------------------------------------------------------------------

/// A section's plane coordinates in the frame the loft's sections share: the rotation (a
/// reflection where the section's plane faces back) from the plane's axes to the shared ones,
/// then the loop's centroid moved to the origin and its area scaled to one.
#[derive(Clone, Copy, Debug)]
struct Frame {
    u: Vec2,
    v: Vec2,
    centre: Vec2,
    scale: f64,
}

impl Frame {
    /// The frame of the loop `segs` whose shared axes are `u` and `v` (orthonormal, in plane
    /// coordinates).
    fn new(u: Vec2, v: Vec2, segs: &[Seg2]) -> Frame {
        let area = signed_area(segs).abs().sqrt();
        let perimeter: f64 = segs.iter().map(length).sum();
        // A loop with no area still gets a finite scale (its matching is then by position alone).
        let scale = if area.is_finite() && area > 0.0 {
            area
        } else if perimeter.is_finite() && perimeter > 0.0 {
            perimeter / 4.0
        } else {
            1.0
        };
        Frame {
            u,
            v,
            centre: centroid(segs),
            scale,
        }
    }

    fn point(&self, p: Vec2) -> Vec2 {
        self.dir(p - self.centre) * (1.0 / self.scale)
    }

    fn dir(&self, d: Vec2) -> Vec2 {
        Vec2::new(dot(self.u, d), dot(self.v, d))
    }

    /// The plane point of the shared point `q`.
    fn plane_point(&self, q: Vec2) -> Vec2 {
        self.centre + (self.u * q.x + self.v * q.y) * self.scale
    }
}

/// Where the cuts go that make two loops correspond: `a_cuts[i]` and `b_cuts[j]` are the
/// increasing parameters, inside (0, 1), at which segment `i` of the first and segment `j` of the
/// second split; cut, both have the same number of segments, and the second's segment `b_start`
/// joins the first's segment 0.
#[derive(Clone, Debug)]
struct Correspondence {
    a_cuts: Vec<Vec<f64>>,
    b_cuts: Vec<Vec<f64>>,
    b_start: usize,
}

/// One loop as the pairing sees it: its segments, and its vertices' places and tangents in the
/// shared frame.
struct Side<'a> {
    segs: &'a [Seg2],
    frame: Frame,
    at: Vec<Vec2>,
    /// The unit tangent arriving at each vertex, and leaving it.
    before: Vec<Vec2>,
    after: Vec<Vec2>,
    len: Vec<f64>,
    /// For each curved segment, its points at even parameters ([`nearest`]).
    chords: Vec<Vec<Vec2>>,
}

impl<'a> Side<'a> {
    fn new(segs: &'a [Seg2], frame: Frame) -> Result<Side<'a>> {
        let n = segs.len();
        let at: Vec<Vec2> = segs.iter().map(|s| frame.point(s.start())).collect();
        let after: Vec<Vec2> = segs.iter().map(|s| frame.dir(tangent(s, 0.0))).collect();
        let before: Vec<Vec2> = (0..n)
            .filter_map(|i| segs.get(wrap(i + n - 1, n)))
            .map(|s| frame.dir(tangent(s, 1.0)))
            .collect();
        let len: Vec<f64> = segs.iter().map(length).collect();
        let all = |v: &[Vec2]| v.iter().all(|p| finite(*p));
        let total: f64 = len.iter().sum();
        if n == 0
            || before.len() != n
            || !all(&at)
            || !all(&before)
            || !all(&after)
            || !(total.is_finite() && total > 0.0)
        {
            return Err(SolidError::InvalidInput {
                operation: OP,
                reason: "a loft section has no length or a point that is not finite",
            });
        }
        let chords = segs
            .iter()
            .map(|s| match s {
                Seg2::Line { .. } | Seg2::Arc { .. } => Vec::new(),
                _ => (0..=CHORDS)
                    .map(|k| s.point_at(k as f64 / CHORDS as f64))
                    .collect(),
            })
            .collect();
        Ok(Side {
            segs,
            frame,
            at,
            before,
            after,
            len,
            chords,
        })
    }

    fn count(&self) -> usize {
        self.segs.len()
    }

    /// Where the shared point `q` lands on segment `e`: the parameter of the segment's nearest
    /// point, that point and the unit tangent there (both in the shared frame).
    fn land(&self, e: usize, q: Vec2) -> Option<(f64, Vec2, Vec2)> {
        let s = self.segs.get(e)?;
        let chord = self.chords.get(e).map_or(&[][..], Vec::as_slice);
        let t = nearest(s, chord, self.frame.plane_point(q));
        let tangent = unit(derivative(s, t)).unwrap_or_else(|| {
            if t < 0.5 {
                tangent(s, 0.0)
            } else {
                tangent(s, 1.0)
            }
        });
        Some((t, self.frame.point(s.point_at(t)), self.frame.dir(tangent)))
    }

    /// The vertices from vertex `i` on round to vertex `k` (once round when they are the same),
    /// each with the length of the loop from `i` to it: `k` last, with the whole length.
    fn run(&self, i: usize, k: usize) -> Vec<(usize, f64)> {
        let n = self.count();
        let mut out = Vec::new();
        let (mut at, mut d) = (wrap(i, n), 0.0);
        for _ in 0..n {
            d += self.len.get(at).copied().unwrap_or(0.0);
            at = wrap(at + 1, n);
            out.push((at, d));
            if at == wrap(k, n) {
                break;
            }
        }
        out
    }
}

/// A point moving forward round a loop from one of its vertices: [`Cursor::to`] finds the
/// segment and parameter a growing distance on, without going past the vertex `end`.
struct Cursor<'s, 'a> {
    side: &'s Side<'a>,
    seg: usize,
    end: usize,
    before: f64,
}

impl Cursor<'_, '_> {
    fn to(&mut self, d: f64) -> Option<(usize, f64)> {
        let n = self.side.count();
        loop {
            let l = *self.side.len.get(self.seg)?;
            let next = wrap(self.seg + 1, n);
            if d < self.before + l || next == wrap(self.end, n) {
                let t = param_at(self.side.segs.get(self.seg)?, d - self.before, l);
                return Some((self.seg, t.clamp(1e-9, 1.0 - 1e-9)));
            }
            self.before += l;
            self.seg = next;
        }
    }
}

/// The parameter of the point of `s` nearest the plane point `p`; for a curve (neither a line nor
/// an arc), of the polyline through `chord`, its points at the parameters k / `CHORDS`.
fn nearest(s: &Seg2, chord: &[Vec2], p: Vec2) -> f64 {
    match *s {
        Seg2::Line { a, b } => {
            let d = b - a;
            let t = dot(p - a, d) / len2(d);
            if t.is_finite() {
                t.clamp(0.0, 1.0)
            } else {
                0.0
            }
        }
        Seg2::Arc {
            center,
            start,
            sweep,
            ..
        } => {
            use std::f64::consts::TAU;
            let w = sweep.abs();
            if !(w > 0.0) || dist(p, center) == 0.0 {
                return 0.5;
            }
            // How far round the arc, in its direction of travel, the point's direction lies.
            let along = (angle(p - center) - start) * sweep.signum();
            let u = along.rem_euclid(TAU);
            if u <= w {
                u / w
            } else if TAU - u < u - w {
                0.0
            } else {
                1.0
            }
        }
        // A curve: the nearest point of the polyline through its points at even parameters
        // (`CHORDS` pieces), as a cost needs it no closer and a search per grid cell is slow.
        _ => {
            let mut best = (f64::INFINITY, 0.0);
            for k in 0..CHORDS {
                let (t0, t1) = (k as f64 / CHORDS as f64, (k + 1) as f64 / CHORDS as f64);
                let (a, b) = (
                    chord.get(k).copied().unwrap_or_else(|| s.point_at(t0)),
                    chord.get(k + 1).copied().unwrap_or_else(|| s.point_at(t1)),
                );
                let d = b - a;
                let u = (dot(p - a, d) / len2(d)).clamp(0.0, 1.0);
                let u = if u.is_finite() { u } else { 0.0 };
                let gap = dist(p, a + d * u);
                if gap < best.0 {
                    best = (gap, t0 + (t1 - t0) * u);
                }
            }
            best.1
        }
    }
}

/// How much two unit tangents differ: 0 when they agree, 2 when they are opposite.
fn turn(a: Vec2, b: Vec2) -> f64 {
    1.0 - dot(a, b)
}

/// The two loops of a pairing: `a` along the grid's rows, `b` along its columns.
struct Grid<'a> {
    a: Side<'a>,
    b: Side<'a>,
}

impl Grid<'_> {
    /// Vertex `i` of `a` paired with vertex `j` of `b`.
    fn pair(&self, i: usize, j: usize) -> f64 {
        let g = |v: &Vec<Vec2>, k: usize| v.get(k).copied().unwrap_or(Vec2::ZERO);
        let (a, b) = (&self.a, &self.b);
        dist(g(&a.at, i), g(&b.at, j))
            + TURN
                * 0.5
                * (turn(g(&a.before, i), g(&b.before, j)) + turn(g(&a.after, i), g(&b.after, j)))
    }

    /// Vertex `i` of `x` alone, on segment `e` of `y`.
    fn alone(x: &Side, i: usize, y: &Side, e: usize) -> f64 {
        let (Some(&q), Some(&before), Some(&after)) =
            (x.at.get(i), x.before.get(i), x.after.get(i))
        else {
            return f64::INFINITY;
        };
        match y.land(e, q) {
            Some((_, p, t)) => {
                dist(q, p) + TURN * 0.5 * (turn(before, t) + turn(after, t)) + UNPAIRED
            }
            None => f64::INFINITY,
        }
    }

    fn alone_a(&self, i: usize, e: usize) -> f64 {
        Grid::alone(&self.a, i, &self.b, e)
    }

    fn alone_b(&self, j: usize, e: usize) -> f64 {
        Grid::alone(&self.b, j, &self.a, e)
    }
}

/// A step of a walk through the grid: from node (i, j) (the first i vertices of `a` and the
/// vertices of `b` from the start up to column j behind it), vertex i of `a` paired with vertex
/// j of `b`, or either alone, landing on the other loop's segment between the vertex behind and
/// the one ahead.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Step {
    Pair,
    A,
    B,
}

/// A walk from node (0, `start`) to (m, `start` + n): one correspondence round both loops, `b`
/// started at its vertex `start`.
#[derive(Clone, Debug)]
struct Walk {
    start: usize,
    cost: f64,
    steps: Vec<Step>,
}

impl Walk {
    /// For each row, the least and the greatest column the walk visits there.
    fn bounds(&self, m: usize) -> (Vec<usize>, Vec<usize>) {
        let (mut lo, mut hi) = (vec![usize::MAX; m + 1], vec![0usize; m + 1]);
        let (mut i, mut j) = (0usize, self.start);
        let mut visit = |i: usize, j: usize| {
            if let (Some(l), Some(h)) = (lo.get_mut(i), hi.get_mut(i)) {
                *l = (*l).min(j);
                *h = (*h).max(j);
            }
        };
        visit(i, j);
        for s in &self.steps {
            match s {
                Step::Pair => (i, j) = (i + 1, j + 1),
                Step::A => i += 1,
                Step::B => j += 1,
            }
            visit(i, j);
        }
        (lo, hi)
    }
}

/// The least-cost walk from (0, `s`) to (m, `s` + n) through the nodes of row i at columns
/// `lo[i]..=hi[i]`. Ties go to a pair, then to `a`'s vertex alone.
fn walk(g: &Grid, s: usize, lo: &[usize], hi: &[usize]) -> Option<Walk> {
    let (m, n) = (g.a.count(), g.b.count());
    // The rows' column ranges, kept to the columns a walk from this start can reach.
    let ranges: Vec<(usize, usize)> = (0..=m)
        .map(|i| {
            (
                lo.get(i).copied().unwrap_or(0).max(s),
                hi.get(i).copied().unwrap_or(0).min(s + n),
            )
        })
        .collect();
    let width = |r: &(usize, usize)| if r.1 >= r.0 { r.1 - r.0 + 1 } else { 0 };
    let mut offsets = Vec::new();
    let mut total = 0usize;
    for r in &ranges {
        offsets.push(total);
        total += width(r);
    }
    // How each node was reached: 0 a pair, 1 `a` alone, 2 `b` alone, 3 the start.
    let mut back = vec![3u8; total];
    let mut prev: Vec<f64> = Vec::new();
    let mut prev_range = (1usize, 0usize);
    for (i, &(l, h)) in ranges.iter().enumerate() {
        let mut cur = vec![f64::INFINITY; width(&(l, h))];
        let off = *offsets.get(i)?;
        for j in l..=h {
            let (mut best, mut how) = (if i == 0 && j == s { 0.0 } else { f64::INFINITY }, 3u8);
            if i > 0 {
                let within = |c: usize| c >= prev_range.0 && c <= prev_range.1;
                if j >= 1 && within(j - 1) {
                    if let Some(&p) = prev.get(j - 1 - prev_range.0) {
                        let c = p + g.pair(i - 1, wrap(j - 1, n));
                        if c < best {
                            (best, how) = (c, 0);
                        }
                    }
                }
                if within(j) {
                    if let Some(&p) = prev.get(j - prev_range.0) {
                        let c = p + g.alone_a(i - 1, wrap(j + n - 1, n));
                        if c < best {
                            (best, how) = (c, 1);
                        }
                    }
                }
            }
            if j > l {
                if let Some(&p) = cur.get(j - 1 - l) {
                    let c = p + g.alone_b(wrap(j - 1, n), wrap(i + m - 1, m));
                    if c < best {
                        (best, how) = (c, 2);
                    }
                }
            }
            if let Some(x) = cur.get_mut(j - l) {
                *x = best;
            }
            if let Some(x) = back.get_mut(off + j - l) {
                *x = how;
            }
        }
        prev = cur;
        prev_range = (l, h);
    }
    let end = s + n;
    if end < prev_range.0 || end > prev_range.1 {
        return None;
    }
    let cost = *prev.get(end - prev_range.0)?;
    if !cost.is_finite() {
        return None;
    }
    let mut steps = Vec::new();
    let (mut i, mut j) = (m, end);
    while (i, j) != (0, s) {
        let (l, _) = *ranges.get(i)?;
        let how = *back.get(offsets.get(i)? + j.checked_sub(l)?)?;
        match how {
            0 => {
                steps.push(Step::Pair);
                (i, j) = (i.checked_sub(1)?, j.checked_sub(1)?);
            }
            1 => {
                steps.push(Step::A);
                i = i.checked_sub(1)?;
            }
            2 => {
                steps.push(Step::B);
                j = j.checked_sub(1)?;
            }
            _ => return None,
        }
    }
    steps.reverse();
    Some(Walk {
        start: s,
        cost,
        steps,
    })
}

/// Whether `w` is better than `best`: cheaper beyond rounding, or as cheap from an earlier start.
fn better(w: &Walk, best: &Walk) -> bool {
    let tol = 1e-12 * best.cost.abs().max(1.0);
    w.cost < best.cost - tol || (w.cost <= best.cost + tol && w.start < best.start)
}

/// The least-cost walk over every start. The best walks from two starts can be chosen not to
/// cross (where they would, the cheaper of the two pieces between serves both), so a start's
/// walk is searched only between the walks of the starts either side of it, halving the range
/// of starts each time: about m·n·log2(n) steps, not m·n².
fn best_walk(g: &Grid) -> Result<Walk> {
    let (m, n) = (g.a.count(), g.b.count());
    let first = walk(g, 0, &vec![0; m + 1], &vec![n; m + 1]).ok_or_else(unpaired)?;
    let (lo, hi) = first.bounds(m);
    // The walk from start n is the first one moved on by a turn.
    let hi_n: Vec<usize> = hi.iter().map(|c| c + n).collect();
    let mut best = first;
    between(g, (0, &lo), (n, &hi_n), &mut best, 0)?;
    Ok(best)
}

/// Every start strictly between `low.0` and `high.0`: the walks searched to the right of the
/// columns `low.1` (the leftmost the walk from `low.0` visits on each row) and to the left of
/// `high.1` (the rightmost of the walk from `high.0`).
fn between(
    g: &Grid,
    low: (usize, &[usize]),
    high: (usize, &[usize]),
    best: &mut Walk,
    depth: usize,
) -> Result<()> {
    if high.0 <= low.0 + 1 {
        return Ok(());
    }
    if depth > MAX_DEPTH {
        return Err(invariant("loft section pairing went too deep"));
    }
    let m = g.a.count();
    let mid = low.0 + (high.0 - low.0) / 2;
    let w = walk(g, mid, low.1, high.1).ok_or_else(unpaired)?;
    if better(&w, best) {
        *best = w.clone();
    }
    let (lo, hi) = w.bounds(m);
    between(g, low, (mid, &hi), best, depth + 1)?;
    between(g, (mid, &lo), high, best, depth + 1)
}

/// How many times [`banded_walk`] moves its band onto the walk it found; the last band's walk
/// is kept.
const BAND_PASSES: usize = 8;
/// The most segments two sections of equal counts may have for the band of [`banded_walk`] within
/// [`MAX_CELLS`] to be one column wide either side (`(MAX_CELLS - 3) / 5`).
pub const LARGEST_BANDED: usize = 799_999;

/// A vertex turns more than this (one less the cosine of the angle between the tangents arriving
/// and leaving, about 26°) to be a corner the band of [`banded_walk`] is anchored at.
const CORNER_TURN: f64 = 0.1;

/// The corners of a loop ([`CORNER_TURN`]).
fn corners(side: &Side) -> Vec<usize> {
    (0..side.count())
        .filter(|&i| match (side.before.get(i), side.after.get(i)) {
            (Some(b), Some(a)) => turn(*b, *a) > CORNER_TURN,
            _ => false,
        })
        .collect()
}

/// The corners of the two loops paired as the pairing pairs the polygons through them (each loop's
/// corners joined by straight lines, in its frame): `(i, j)` vertex numbers in order from the
/// first. Polygons whose grid is past [`MAX_CELLS`] are paired through a band of it
/// ([`banded_walk`], at their shares of their lengths: every vertex of theirs is a corner). Empty
/// when either loop has fewer than three corners, or the polygons cannot be paired (past
/// [`LARGEST_BANDED`] corners): the corner pairs only centre the band of [`banded_walk`], so
/// without them it is centred on the shares round the whole loops, and nothing is refused for it.
fn corner_pairs(g: &Grid) -> Vec<(usize, usize)> {
    let (ca, cb) = (corners(&g.a), corners(&g.b));
    if ca.len() < 3 || cb.len() < 3 {
        return Vec::new();
    }
    let chords = |side: &Side, c: &[usize]| -> Vec<Seg2> {
        (0..c.len())
            .filter_map(|k| {
                let a = side.segs.get(*c.get(k)?)?.start();
                let b = side.segs.get(*c.get(wrap(k + 1, c.len()))?)?.start();
                Some(Seg2::Line { a, b })
            })
            .collect()
    };
    let (pa, pb) = (chords(&g.a, &ca), chords(&g.b, &cb));
    let (Ok(a), Ok(b)) = (Side::new(&pa, g.a.frame), Side::new(&pb, g.b.frame)) else {
        return Vec::new();
    };
    let small = Grid { a, b };
    let walked = if (ca.len() + 1).saturating_mul(cb.len() + 1) > MAX_CELLS {
        banded_walk(&small, false, MAX_CELLS)
    } else {
        best_walk(&small)
    };
    let Ok(w) = walked else {
        return Vec::new();
    };
    let n = cb.len();
    let (mut i, mut j) = (0usize, w.start);
    let mut out = Vec::new();
    for st in &w.steps {
        match st {
            Step::Pair => {
                if let (Some(&x), Some(&y)) = (ca.get(i), cb.get(wrap(j, n))) {
                    out.push((x, y));
                }
                (i, j) = (i + 1, j + 1);
            }
            Step::A => i += 1,
            Step::B => j += 1,
        }
    }
    out
}

/// For the rows `i0..i1` of `a` (vertex numbers, counted on round the loop), the columns of `b`
/// (from `j0` to at most `j1`, counted on likewise) whose share of `b`'s length from `j0` to `j1`
/// is nearest the row vertex's share of `a`'s length from `i0` to `i1`.
fn share_columns(g: &Grid, (i0, i1): (usize, usize), (j0, j1): (usize, usize)) -> Vec<usize> {
    let (m, n) = (g.a.count(), g.b.count());
    let la: f64 = (i0..i1)
        .map(|i| g.a.len.get(wrap(i, m)).copied().unwrap_or(0.0))
        .sum();
    let lb: f64 = (j0..j1)
        .map(|j| g.b.len.get(wrap(j, n)).copied().unwrap_or(0.0))
        .sum();
    let share = |d: f64, l: f64| if l > 0.0 { d / l } else { 0.0 };
    let mut out = Vec::new();
    let (mut k, mut db, mut da) = (j0, 0.0f64, 0.0f64);
    for i in i0..i1 {
        let target = share(da, la);
        while k < j1 {
            let next = db + g.b.len.get(wrap(k, n)).copied().unwrap_or(0.0);
            if (share(next, lb) - target).abs() <= (share(db, lb) - target).abs() {
                (db, k) = (next, k + 1);
            } else {
                break;
            }
        }
        out.push(k);
        da += g.a.len.get(wrap(i, m)).copied().unwrap_or(0.0);
    }
    out
}

/// The column of every row (0 to m), from `s` for row 0 to `s + n` for row m: between two paired
/// corners ([`corner_pairs`]), each of `a`'s vertices at its share of the length between them;
/// with no paired corners, from `b`'s vertex that pairs most cheaply with `a`'s vertex 0, at its
/// share of the length round.
fn band_centres(g: &Grid, anchored: bool) -> Option<Vec<usize>> {
    let (m, n) = (g.a.count(), g.b.count());
    let anchors = if anchored {
        corner_pairs(g)
    } else {
        Vec::new()
    };
    let Some(&(i_first, _)) = anchors.first() else {
        let s = (0..n).min_by(|x, y| g.pair(0, *x).total_cmp(&g.pair(0, *y)))?;
        let mut c = share_columns(g, (0, m), (s, s + n));
        c.push(s + n);
        return Some(c);
    };
    // The paired corners' columns counted on round `b` so they increase, from n on.
    let mut on: Vec<(usize, usize)> = Vec::new();
    for &(i, j) in &anchors {
        let floor = on.last().map_or(n, |&(_, p)| p + 1);
        let mut c = j + n;
        while c < floor {
            c += n;
        }
        on.push((i, c));
    }
    let (&(i0, j0), last) = (on.first()?, on.len());
    // The columns of rows i_first to i_first + m - 1, piece by piece between paired corners.
    let mut cols: Vec<usize> = Vec::new();
    for k in 0..last {
        let &(ia, ja) = on.get(k)?;
        let (ib, jb) = on.get(k + 1).copied().unwrap_or((i0 + m, j0 + n));
        cols.extend(share_columns(g, (ia, ib), (ja, jb)));
    }
    // Row i < i_first comes round after the last corner: its column, one turn of `b` back.
    let mut centre: Vec<usize> = (0..m)
        .map(|i| {
            let r = wrap(i + m - i_first, m);
            let c = cols.get(r).copied().unwrap_or(j0);
            if i < i_first {
                c.saturating_sub(n)
            } else {
                c
            }
        })
        .collect();
    let s = *centre.first()?;
    centre.push(s + n);
    // Columns from below n, so the start is one of `b`'s vertex numbers.
    let shift = s.checked_div(n).unwrap_or(0).saturating_mul(n);
    Some(
        centre
            .into_iter()
            .map(|c| c.saturating_sub(shift))
            .collect(),
    )
}

/// The least-cost walk for two loops whose whole grid is past `cells` ([`MAX_CELLS`] for a
/// match): from one start, through a band of columns about where each of `a`'s vertices falls
/// ([`band_centres`]: when `anchored`, at its share of the length between the corners the
/// polygons through both loops' corners pair, otherwise of the length round), as wide as the
/// bound allows (its cells, at most (m + 1)(2h + 1) + 2n, stay within `cells`: h is about 400
/// vertices for two loops of 5000). A walk that runs along an edge of the band (not the grid's)
/// may have been held off a cheaper one outside it, so the band is moved to centre on the walk
/// and searched again, up to [`BAND_PASSES`] times (at most that many times `cells` cells); the
/// last band's walk is kept. So the walk is the least-cost one within its band, proven the least
/// over the whole grid only when it does not run along the band's edge.
///
/// # Errors
/// [`SweepFailure::LoftSectionsTooLarge`] when the band would be empty (h = 0: about 800 000
/// segments a section for [`MAX_CELLS`]).
fn banded_walk(g: &Grid, anchored: bool, cells: usize) -> Result<Walk> {
    let (m, n) = (g.a.count(), g.b.count());
    let h = cells
        .saturating_sub(n.saturating_mul(2))
        .checked_div(m + 1)
        .unwrap_or(0)
        .saturating_sub(1)
        .checked_div(2)
        .unwrap_or(0);
    if h == 0 {
        return Err(SolidError::sweep(
            OP,
            SweepFailure::LoftSectionsTooLarge {
                first: m,
                second: n,
                largest: LARGEST_BANDED,
            },
        ));
    }
    let mut centre = band_centres(g, anchored).ok_or_else(unpaired)?;
    let s = *centre.first().ok_or_else(unpaired)?;
    let mut last: Option<Walk> = None;
    for _ in 0..BAND_PASSES {
        // Each row's band also spans the columns of the rows either side, so a walk can step
        // from any column of one row's band into the next row and run along it.
        let column = |i: usize, c: usize| centre.get(i).copied().unwrap_or(c);
        let lo: Vec<usize> = (0..=m)
            .map(|i| {
                let c = column(i, s);
                let before = i.checked_sub(1).map_or(c, |b| column(b, c));
                c.min(before).saturating_sub(h).max(s)
            })
            .collect();
        let hi: Vec<usize> = (0..=m)
            .map(|i| {
                let c = column(i, s);
                (c.max(column(i + 1, c)) + h).min(s + n)
            })
            .collect();
        let w = walk(g, s, &lo, &hi).ok_or_else(unpaired)?;
        let (vlo, vhi) = w.bounds(m);
        let held = (0..=m).any(|i| {
            let at = |v: &[usize], b: &[usize]| v.get(i).zip(b.get(i)).is_some_and(|(x, y)| x == y);
            (at(&vlo, &lo) && lo.get(i).is_some_and(|&b| b > s))
                || (at(&vhi, &hi) && hi.get(i).is_some_and(|&b| b < s + n))
        });
        if !held {
            return Ok(w);
        }
        centre = vlo
            .iter()
            .zip(&vhi)
            .map(|(&a, &b)| a + b.saturating_sub(a).checked_div(2).unwrap_or(0))
            .collect();
        last = Some(w);
    }
    last.ok_or_else(unpaired)
}

/// What an event of the correspondence is on one of the loops: one of its vertices, or a point
/// inside one of its segments.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Place {
    Vertex(usize),
    On(usize, f64),
}

/// The correspondence between loop `a` (in frame `fa`) and loop `b` (in frame `fb`): both wound
/// counter-clockwise in the shared frame. Between two pairs, the lone vertices of each loop land
/// on the other at the same share of the length between the pairs; with no pair at all, each
/// lands on the nearest point of the segment its walk puts it on.
fn correspond(a: &[Seg2], fa: Frame, b: &[Seg2], fb: Frame) -> Result<Correspondence> {
    let g = Grid {
        a: Side::new(a, fa)?,
        b: Side::new(b, fb)?,
    };
    let (m, n) = (g.a.count(), g.b.count());
    let w = if (m + 1).saturating_mul(n + 1) > MAX_CELLS {
        banded_walk(&g, true, MAX_CELLS)?
    } else {
        best_walk(&g)?
    };
    // The events round both loops, in order, from the one at `a`'s vertex 0.
    let mut events: Vec<(Place, Place)> = Vec::new();
    let (mut i, mut j) = (0usize, w.start);
    for s in &w.steps {
        match s {
            Step::Pair => {
                events.push((Place::Vertex(i), Place::Vertex(wrap(j, n))));
                (i, j) = (i + 1, j + 1);
            }
            Step::A => {
                let e = wrap(j + n - 1, n);
                let t =
                    g.b.land(e, *g.a.at.get(i).ok_or_else(unpaired)?)
                        .ok_or_else(unpaired)?
                        .0;
                events.push((Place::Vertex(i), Place::On(e, t)));
                i += 1;
            }
            Step::B => {
                let e = wrap(i + m - 1, m);
                let t =
                    g.a.land(e, *g.b.at.get(wrap(j, n)).ok_or_else(unpaired)?)
                        .ok_or_else(unpaired)?
                        .0;
                events.push((Place::On(e, t), Place::Vertex(wrap(j, n))));
                j += 1;
            }
        }
    }
    let first = events
        .iter()
        .position(|(p, _)| *p == Place::Vertex(0))
        .ok_or_else(unpaired)?;
    events = rotated(&events, first);
    let pairs: Vec<usize> = (0..events.len())
        .filter(|&k| matches!(events.get(k), Some((Place::Vertex(_), Place::Vertex(_)))))
        .collect();
    if pairs.is_empty() {
        keep_in_order(&mut events, &|_, _, ts| increasing(ts));
    } else {
        events = by_share(&g, &events, &pairs)?;
        let first = events
            .iter()
            .position(|(p, _)| *p == Place::Vertex(0))
            .ok_or_else(unpaired)?;
        events = rotated(&events, first);
        // Lone vertices that land on one point of a segment (both past its end, say), or nearer
        // each other or its ends than [`MIN_EDGE`], are moved apart along it, as little as keeps
        // them in order.
        keep_in_order(&mut events, &|side, e, ts| {
            let sd = if side == 0 { &g.a } else { &g.b };
            match (sd.segs.get(e), sd.len.get(e)) {
                (Some(s), Some(&l)) => spaced(s, l, ts),
                _ => spread(ts),
            }
        });
    }
    let mut a_cuts = vec![Vec::new(); m];
    let mut b_cuts = vec![Vec::new(); n];
    for (pa, pb) in &events {
        if let Place::On(e, t) = *pa {
            a_cuts.get_mut(e).ok_or_else(unpaired)?.push(t);
        }
        if let Place::On(e, t) = *pb {
            b_cuts.get_mut(e).ok_or_else(unpaired)?.push(t);
        }
    }
    for c in a_cuts.iter_mut().chain(b_cuts.iter_mut()) {
        c.sort_by(f64::total_cmp);
        if !valid_cuts(c) {
            return Err(unpaired());
        }
    }
    // Where the event at `a`'s vertex 0 is on `b`, once `b` is cut.
    let before = |e: usize| e + b_cuts.iter().take(e).map(Vec::len).sum::<usize>();
    let b_start = match events.first().map(|(_, pb)| *pb).ok_or_else(unpaired)? {
        Place::Vertex(k) => before(k),
        Place::On(e, t) => {
            before(e)
                + 1
                + b_cuts
                    .get(e)
                    .ok_or_else(unpaired)?
                    .iter()
                    .take_while(|&&c| c < t)
                    .count()
        }
    };
    Ok(Correspondence {
        a_cuts,
        b_cuts,
        b_start,
    })
}

/// Whether `ts` are strictly increasing, inside (0, 1).
fn valid_cuts(ts: &[f64]) -> bool {
    ts.iter().all(|t| *t > 0.0 && *t < 1.0) && ts.iter().zip(ts.iter().skip(1)).all(|(a, b)| b > a)
}

/// The events with every lone vertex moved, between each two consecutive pairs (`pairs`, the
/// indices of the paired events), to the point of the other loop at the same share of the length
/// between the pairs. Two lone vertices that land on each other become a pair. The events start
/// at the first pair.
fn by_share(g: &Grid, events: &[(Place, Place)], pairs: &[usize]) -> Result<Vec<(Place, Place)>> {
    let vertex = |p: Place| match p {
        Place::Vertex(k) => Some(k),
        Place::On(..) => None,
    };
    let mut out = Vec::new();
    for (q, &k0) in pairs.iter().enumerate() {
        let k1 = *pairs.get(wrap(q + 1, pairs.len())).ok_or_else(unpaired)?;
        let &(pa0, pb0) = events.get(k0).ok_or_else(unpaired)?;
        let &(pa1, pb1) = events.get(k1).ok_or_else(unpaired)?;
        let (i0, j0, i1, j1) = (
            vertex(pa0).ok_or_else(unpaired)?,
            vertex(pb0).ok_or_else(unpaired)?,
            vertex(pa1).ok_or_else(unpaired)?,
            vertex(pb1).ok_or_else(unpaired)?,
        );
        out.push((pa0, pb0));
        // The lone vertices between, each with its share of its own loop's length between.
        let shares = |run: Vec<(usize, f64)>| -> Option<(f64, Vec<(usize, f64)>)> {
            let total = run.last()?.1;
            let lone = run
                .iter()
                .take(run.len().saturating_sub(1))
                .map(|&(k, d)| (k, d / total))
                .collect();
            Some((total, lone))
        };
        let (la, lone_a) = shares(g.a.run(i0, i1)).ok_or_else(unpaired)?;
        let (lb, lone_b) = shares(g.b.run(j0, j1)).ok_or_else(unpaired)?;
        let mut on_a = Cursor {
            side: &g.a,
            seg: i0,
            end: i1,
            before: 0.0,
        };
        let mut on_b = Cursor {
            side: &g.b,
            seg: j0,
            end: j1,
            before: 0.0,
        };
        let (mut x, mut y) = (0usize, 0usize);
        // Each step places at least one lone vertex.
        let mut steps = 0usize;
        while x < lone_a.len() || y < lone_b.len() {
            steps += 1;
            if steps > lone_a.len() + lone_b.len() {
                return Err(unpaired());
            }
            let (ka, sa) = lone_a.get(x).copied().unwrap_or((0, f64::INFINITY));
            let (kb, sb) = lone_b.get(y).copied().unwrap_or((0, f64::INFINITY));
            // Not paired, they would leave an edge |sa - sb| times each loop's length between
            // the pairs long on both loops.
            if (sa - sb).abs() <= SAME_SHARE || (sa - sb).abs() * la.min(lb) < MIN_EDGE {
                out.push((Place::Vertex(ka), Place::Vertex(kb)));
                (x, y) = (x + 1, y + 1);
            } else if sa < sb {
                let (e, t) = on_b.to(sa * lb).ok_or_else(unpaired)?;
                out.push((Place::Vertex(ka), Place::On(e, t)));
                x += 1;
            } else {
                let (e, t) = on_a.to(sb * la).ok_or_else(unpaired)?;
                out.push((Place::On(e, t), Place::Vertex(kb)));
                y += 1;
            }
        }
    }
    Ok(out)
}

/// How [`keep_in_order`] places the landings on one segment: from the side (0 for `a`, 1 for `b`),
/// the segment and the landings' parameters in order.
type Adjust<'f> = dyn Fn(usize, usize, &[f64]) -> Vec<f64> + 'f;

/// The landings of the events kept in their order along each segment, as `adjust` places them:
/// two vertices can land on the same point. The events start at `a`'s vertex 0; along the
/// segment of `b` that point lies inside, the landings the events end with come first.
fn keep_in_order(events: &mut [(Place, Place)], adjust: &Adjust) {
    let count = events.len();
    let lead = events
        .iter()
        .take_while(|e| matches!(e.1, Place::On(..)))
        .count();
    for side in [0usize, 1] {
        let get = |e: &(Place, Place)| if side == 0 { e.0 } else { e.1 };
        let order: Vec<usize> = if side == 0 {
            (0..count).collect()
        } else {
            (lead..count).chain(0..lead).collect()
        };
        let mut runs: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
        for k in order {
            if let Some(Place::On(s, _)) = events.get(k).map(get) {
                runs.entry(s).or_default().push(k);
            }
        }
        for (s, at) in runs {
            let ts: Vec<f64> = at
                .iter()
                .filter_map(|&k| match events.get(k).map(get) {
                    Some(Place::On(_, t)) => Some(t),
                    _ => None,
                })
                .collect();
            for (&k, t) in at.iter().zip(adjust(side, s, &ts)) {
                if let Some(e) = events.get_mut(k) {
                    let p = if side == 0 { &mut e.0 } else { &mut e.1 };
                    *p = Place::On(s, t);
                }
            }
        }
    }
}

/// `ts` made strictly increasing inside (0, 1), each moved as little as the order allows.
fn increasing(ts: &[f64]) -> Vec<f64> {
    let k = ts.len();
    let gap = (1e-3f64).min(0.5 / (k as f64 + 1.0));
    let mut out: Vec<f64> = Vec::new();
    for (q, &t) in ts.iter().enumerate() {
        let floor = out.last().map_or(gap, |p| p + gap);
        let ceil = 1.0 - gap * (k - q) as f64;
        out.push(t.max(floor).min(ceil));
    }
    for q in (0..k.saturating_sub(1)).rev() {
        let Some(&next) = out.get(q + 1) else {
            continue;
        };
        if let Some(x) = out.get_mut(q) {
            *x = x.min(next - gap);
        }
    }
    if valid_cuts(&out) {
        out
    } else {
        (1..=k).map(|q| q as f64 / (k as f64 + 1.0)).collect()
    }
}

/// `ts` (in order along a segment) unchanged when they are strictly increasing inside (0, 1);
/// otherwise each moved on to at least [`CUT_GAP`] past the one before, and back to at least
/// that before the one after (and the segment's end).
fn spread(ts: &[f64]) -> Vec<f64> {
    if valid_cuts(ts) {
        return ts.to_vec();
    }
    let mut out: Vec<f64> = Vec::new();
    for &t in ts {
        let floor = out.last().map_or(CUT_GAP, |p| p + CUT_GAP);
        out.push(t.max(floor));
    }
    let mut next = 1.0;
    for x in out.iter_mut().rev() {
        *x = x.min(next - CUT_GAP);
        next = *x;
    }
    out
}

/// `ts` (in order along the segment `s`, of length `l`) unchanged when they are strictly
/// increasing inside (0, 1) and at least [`MIN_EDGE`] apart along it and from its ends.
/// Otherwise moved apart along its length as little as keeps them in order: to [`MIN_EDGE`] when
/// the segment has room for that, else evenly ([`spread`] when its lengths are not finite).
fn spaced(s: &Seg2, l: f64, ts: &[f64]) -> Vec<f64> {
    let k = ts.len();
    let d: Vec<f64> = ts.iter().map(|&t| length_to(s, t)).collect();
    if !(l.is_finite() && d.iter().all(|x| x.is_finite())) {
        return spread(ts);
    }
    let mut apart = valid_cuts(ts);
    let mut prev = 0.0;
    for &x in &d {
        apart &= x - prev >= MIN_EDGE;
        prev = x;
    }
    if apart && l - prev >= MIN_EDGE {
        return ts.to_vec();
    }
    let placed: Vec<f64> = if l > MIN_EDGE * (k as f64 + 1.0) {
        let mut out: Vec<f64> = Vec::new();
        for &x in &d {
            let floor = out.last().map_or(MIN_EDGE, |p| p + MIN_EDGE);
            out.push(x.max(floor));
        }
        let mut next = l;
        for x in out.iter_mut().rev() {
            *x = x.min(next - MIN_EDGE);
            next = *x;
        }
        out
    } else {
        (1..=k).map(|q| l * q as f64 / (k as f64 + 1.0)).collect()
    };
    let out: Vec<f64> = placed.iter().map(|&x| param_at(s, x, l)).collect();
    if valid_cuts(&out) {
        out
    } else {
        spread(ts)
    }
}

// --- Circles -------------------------------------------------------------------------------

/// `n` increasing break angles of one turn round a circle (each step and the closing step back
/// to the first, one turn on, positive) for a circle following a lead loop: `angles` (the lead's
/// vertex directions) when they go round once in order in the lead's direction (counter-
/// clockwise, or clockwise when `clockwise`), otherwise from the first of them at the lead's
/// perimeter `fractions`, or evenly. A non-star-shaped lead's directions turn back on
/// themselves: forcing them to increase wound the circle round many times.
fn circle_breaks(n: usize, angles: &[f64], fractions: &[f64], clockwise: bool) -> Vec<f64> {
    use std::f64::consts::TAU;
    let sign = if clockwise { -1.0 } else { 1.0 };
    // Angles measured in the lead's direction of travel.
    let along: Vec<f64> = angles.iter().map(|a| sign * a).collect();
    let run = (along.len() == n)
        .then(|| one_turn(&along))
        .flatten()
        .unwrap_or_else(|| {
            let start = along
                .first()
                .copied()
                .filter(|a| a.is_finite())
                .unwrap_or(0.0);
            let even = || (0..n).map(|i| i as f64 / n as f64).collect::<Vec<f64>>();
            let f = if usable_fractions(n, fractions) {
                fractions.to_vec()
            } else {
                even()
            };
            f.iter().map(|f| start + TAU * f).collect()
        });
    let mut breaks: Vec<f64> = run.into_iter().map(|a| sign * a).collect();
    if clockwise {
        breaks.reverse();
    }
    breaks
}

/// `along` unwrapped into one increasing turn (each step the shortest positive one), or `None`
/// when they do not go round once in order.
fn one_turn(along: &[f64]) -> Option<Vec<f64>> {
    let mut out: Vec<f64> = Vec::new();
    for &x in along {
        if !x.is_finite() {
            return None;
        }
        let next = match out.last() {
            Some(&prev) => {
                let step = (x - prev).rem_euclid(std::f64::consts::TAU);
                if step <= MIN_SWEEP {
                    return None;
                }
                prev + step
            }
            None => x,
        };
        out.push(next);
    }
    let (first, last) = (*out.first()?, *out.last()?);
    (last - first < std::f64::consts::TAU - MIN_SWEEP).then_some(out)
}

/// `fractions` are `n` finite, strictly increasing shares of a turn from 0, all under one.
fn usable_fractions(n: usize, fractions: &[f64]) -> bool {
    fractions.len() == n
        && fractions.first().is_some_and(|f| f.abs() < MIN_SWEEP)
        && fractions
            .iter()
            .zip(fractions.iter().skip(1))
            .all(|(a, b)| b - a > MIN_SWEEP)
        && fractions
            .last()
            .is_some_and(|f| f.is_finite() && *f < 1.0 - MIN_SWEEP)
}

/// Which arcs of `originals` (arcs of one circle) each arc of `pieces` (arcs of the same
/// circle) runs over: a circle re-split at other angles comes from every original arc it
/// overlaps.
fn arc_overlaps(pieces: &[Seg2], originals: &[Seg2]) -> Vec<Vec<usize>> {
    use std::f64::consts::TAU;
    // An arc as its counter-clockwise start angle and (positive) sweep.
    let span = |s: &Seg2| match *s {
        Seg2::Arc { start, sweep, .. } => Some(if sweep < 0.0 {
            (start + sweep, -sweep)
        } else {
            (start, sweep)
        }),
        _ => None,
    };
    let eps = 1e-9;
    pieces
        .iter()
        .map(|p| {
            let Some((p0, pw)) = span(p) else {
                return Vec::new();
            };
            (0..originals.len())
                .filter(|&j| {
                    originals.get(j).and_then(span).is_some_and(|(o0, ow)| {
                        // The original's start, seen from the piece's start going round.
                        let d = (o0 - p0).rem_euclid(TAU);
                        d < pw - eps || d + ow > TAU + eps || ow >= TAU - eps
                    })
                })
                .collect()
        })
        .collect()
}

/// The full circle `l` re-split into `n` arcs to follow a loop of `n` vertices whose directions
/// from its centre, seen in this loop's plane, are `angles` ([`circle_breaks`]), counter-
/// clockwise; each piece runs over every given segment the arcs of `l` it overlaps ran over.
fn circle_resampled(
    l: &Traced,
    n: usize,
    angles: &[f64],
    fractions: &[f64],
    clockwise: bool,
) -> Option<Traced> {
    if !is_circle(&l.segs) || n == 0 {
        return None;
    }
    let Some(Seg2::Arc { center, radius, .. }) = l.segs.first().copied() else {
        return None;
    };
    let a = circle_breaks(n, angles, fractions, clockwise);
    let segs: Vec<Seg2> = (0..a.len())
        .filter_map(|i| {
            let s = *a.get(i)?;
            let e = if i + 1 < a.len() {
                *a.get(i + 1)?
            } else {
                a.first()? + std::f64::consts::TAU
            };
            Some(Seg2::Arc {
                center,
                radius,
                start: s,
                sweep: e - s,
            })
        })
        .collect();
    let from = arc_overlaps(&segs, &l.segs)
        .into_iter()
        .map(|js| {
            js.iter()
                .filter_map(|j| l.from.get(*j))
                .flatten()
                .copied()
                .collect::<BTreeSet<usize>>()
                .into_iter()
                .collect()
        })
        .collect();
    Some(Traced { segs, from })
}

// --- Matching ------------------------------------------------------------------------------

/// A section wound for the matching: its plane, its loop, whether its plane faces back from the
/// one before, and its frame for the pairing.
struct Wound {
    origin: Vec3,
    x: Vec3,
    y: Vec3,
    l: Traced,
    back: bool,
    frame: Frame,
}

fn to_world(origin: Vec3, x: Vec3, y: Vec3, p: Point2) -> Vec3 {
    origin + x * p.x + y * p.y
}

/// The loft's closed sections, given in loft order, matched as its walls are built through them:
/// each wound the way the one before it is, cut to the vertices it shares with the others, and
/// started at the segment that joins the first segment of the section before it (see the module
/// notes). A section that would get more than `max_segments` segments is refused.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a section with no segments, no length or a point that is not
/// finite, plane axes that are not finite orthogonal unit vectors, a circle whose centre, radius
/// or angles are not finite or whose radius is not positive, or a match past `max_segments`;
/// [`SolidError::Failed`] with [`SweepFailure::LoftSectionsTooLarge`] for two sections past
/// [`LARGEST_BANDED`] segments; [`SolidError::PortInvariant`] when the pairing breaks down.
pub fn match_loft_sections(
    sections: &[LoftSection],
    max_segments: usize,
) -> Result<Vec<MatchedSection>> {
    let first = sections.first().ok_or(SolidError::InvalidInput {
        operation: OP,
        reason: "a loft needs sections",
    })?;
    if sections.iter().any(|s| s.segs.is_empty()) {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a loft section has no segments",
        });
    }
    let unit_axis = |v: Vec3| {
        let l = dot3(v, v).sqrt();
        l.is_finite() && (l - 1.0).abs() <= AXES_TOL
    };
    if sections.iter().any(|s| {
        !unit_axis(s.x)
            || !unit_axis(s.y)
            || !(dot3(s.x, s.y).abs() <= AXES_TOL)
            || !(s.origin.x.is_finite() && s.origin.y.is_finite() && s.origin.z.is_finite())
    }) {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a loft section's plane axes are not finite orthogonal unit vectors",
        });
    }
    // A circle is matched by its centre, radius and angles, not through `without_points`.
    let finite_arc = |s: &Seg2| match *s {
        Seg2::Arc {
            center,
            radius,
            start,
            sweep,
        } => {
            finite(center)
                && radius.is_finite()
                && radius > 0.0
                && start.is_finite()
                && sweep.is_finite()
        }
        _ => true,
    };
    if sections
        .iter()
        .any(|s| is_circle(&s.segs) && !s.segs.iter().all(finite_arc))
    {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a loft section has no length or a point that is not finite",
        });
    }
    let too_many = || SolidError::InvalidInput {
        operation: OP,
        reason: "matching loft sections would create too many segments",
    };
    let first_normal = cross3(first.x, first.y);
    // Each section wound as the one before it, and the frame the pairing compares sections in:
    // the first plane's axes, carried from each plane to the next by the least rotation that
    // takes one normal onto the other.
    let (mut normal, mut axis) = (first_normal, first.x);
    let mut wound: Vec<Wound> = Vec::new();
    for s in sections {
        let pn = cross3(s.x, s.y);
        let back = dot3(pn, normal) < 0.0;
        let n = if back { pn * -1.0 } else { pn };
        axis = carried(axis, normal, n);
        normal = n;
        let side = cross3(n, axis);
        let given = Traced {
            segs: s.segs.clone(),
            from: (0..s.segs.len()).map(|i| vec![i]).collect(),
        };
        let l = given.oriented(back);
        let frame = Frame::new(
            Vec2::new(dot3(axis, s.x), dot3(axis, s.y)),
            Vec2::new(dot3(side, s.x), dot3(side, s.y)),
            &l.segs,
        );
        wound.push(Wound {
            origin: s.origin,
            x: s.x,
            y: s.y,
            l,
            back,
            frame,
        });
    }
    // The sections that are not circles, paired one after the other; each pairing cuts every
    // section paired before it the same way, so their segments stay matched one to one.
    let mut done: Vec<Option<Traced>> = vec![None; wound.len()];
    let mut paired: Vec<usize> = Vec::new();
    for (k, w) in wound.iter().enumerate() {
        if is_circle(&w.l.segs) {
            continue;
        }
        let l = w.l.without_points().ok_or(SolidError::InvalidInput {
            operation: OP,
            reason: "a loft section has no length or a point that is not finite",
        })?;
        let Some(&last) = paired.last() else {
            if l.segs.len() > max_segments {
                return Err(too_many());
            }
            *done.get_mut(k).ok_or_else(unpaired)? = Some(l);
            paired.push(k);
            continue;
        };
        let (Some(Some(prev)), Some(fa)) = (done.get(last), wound.get(last).map(|w| w.frame))
        else {
            return Err(unpaired());
        };
        let c = correspond(&prev.segs, fa, &l.segs, w.frame)?;
        for &q in &paired {
            if let Some(r) = done.get_mut(q).and_then(Option::as_mut) {
                *r = r.cut(&c.a_cuts).ok_or_else(unpaired)?;
            }
        }
        let mut next = l.cut(&c.b_cuts).ok_or_else(unpaired)?;
        next.rotate_left(c.b_start);
        if next.segs.len() > max_segments {
            return Err(too_many());
        }
        *done.get_mut(k).ok_or_else(unpaired)? = Some(next);
        paired.push(k);
    }
    // A loop's vertices' directions from its centroid, in space.
    let corners = |w: (Vec3, Vec3, Vec3), l: &Traced| -> Vec<Vec3> {
        let c = to_world(w.0, w.1, w.2, centroid(&l.segs));
        l.segs
            .iter()
            .map(|s| to_world(w.0, w.1, w.2, s.start()) - c)
            .collect()
    };
    // The share of a loop's perimeter before each of its vertices: where a circle section breaks
    // when the directions it follows do not go round in order.
    let shares_of = |l: &Traced| -> Vec<f64> {
        let total: f64 = l.segs.iter().map(length).sum();
        l.segs
            .iter()
            .scan(0.0, |before, s| {
                let f = *before / total;
                *before += length(s);
                Some(f)
            })
            .collect()
    };
    // What the first section follows when it is a circle: the first section that is not, or with
    // circles only, the one with the most segments, wound as the first.
    let lead: Option<((Vec3, Vec3, Vec3), Traced)> = match paired.first() {
        Some(&f) => wound
            .get(f)
            .map(|w| (w.origin, w.x, w.y))
            .zip(done.get(f).cloned().flatten()),
        None => wound.iter().max_by_key(|w| w.l.segs.len()).map(|w| {
            let back = dot3(cross3(w.x, w.y), first_normal) < 0.0;
            ((w.origin, w.x, w.y), w.l.oriented(back))
        }),
    };
    let mut out: Vec<((Vec3, Vec3, Vec3), Traced)> = Vec::new();
    for (k, w) in wound.iter().enumerate() {
        if let Some(Some(r)) = done.get(k) {
            out.push(((w.origin, w.x, w.y), r.clone()));
            continue;
        }
        let (fp, fl) = out
            .last()
            .cloned()
            .or_else(|| lead.clone())
            .ok_or_else(unpaired)?;
        // Break angles: the followed corners seen in this plane. A re-split circle runs counter-
        // clockwise in its plane; wound the other way when the plane faces back.
        let local: Vec<Vec2> = corners(fp, &fl)
            .iter()
            .map(|d| Vec2::new(dot3(*d, w.x), dot3(*d, w.y)))
            .collect();
        let angles: Vec<f64> = local.iter().map(|d| angle(*d)).collect();
        // The followed loop winds counter-clockwise about the normal the sections share, so
        // clockwise in this plane's own axes when it faces back.
        let mut r = circle_resampled(&w.l, fl.segs.len(), &angles, &shares_of(&fl), w.back)
            .ok_or_else(unpaired)?
            .oriented(w.back);
        if r.segs.len() > max_segments {
            return Err(too_many());
        }
        // Start where every vertex lines up best with the followed one of the same number (the
        // largest sum of dot products of their directions from the centroids).
        let c = centroid(&r.segs);
        let m = r.segs.len();
        let fit = |k: usize| -> f64 {
            local
                .iter()
                .enumerate()
                .filter_map(|(i, d)| r.segs.get(wrap(i + k, m)).map(|s| dot(s.start() - c, *d)))
                .sum()
        };
        if let Some(k) = (0..m).min_by(|a, b| fit(*b).total_cmp(&fit(*a))) {
            r.rotate_left(k);
        }
        out.push(((w.origin, w.x, w.y), r));
    }
    Ok(out
        .into_iter()
        .map(|(_, t)| MatchedSection {
            segs: t.segs,
            from: t.from,
        })
        .collect())
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::indexing_slicing)]
mod tests {
    use std::f64::consts::PI;

    use super::*;

    fn p(x: f64, y: f64) -> Vec2 {
        Vec2::new(x, y)
    }

    fn line(a: (f64, f64), b: (f64, f64)) -> Seg2 {
        Seg2::Line {
            a: p(a.0, a.1),
            b: p(b.0, b.1),
        }
    }

    /// The closed polygon through `pts`.
    fn poly(pts: &[(f64, f64)]) -> Vec<Seg2> {
        (0..pts.len())
            .map(|i| line(pts[i], pts[(i + 1) % pts.len()]))
            .collect()
    }

    /// A counter-clockwise arc about `c` from `a` to `b`.
    fn arc(c: (f64, f64), a: (f64, f64), b: (f64, f64)) -> Seg2 {
        let start = (a.1 - c.1).atan2(a.0 - c.0);
        let sweep = ((b.1 - c.1).atan2(b.0 - c.0) - start).rem_euclid(2.0 * PI);
        Seg2::Arc {
            center: p(c.0, c.1),
            radius: len(p(a.0 - c.0, a.1 - c.1)),
            start,
            sweep,
        }
    }

    /// A section on the plane z = `z`, sketch axes along X and Y.
    fn at(z: f64, segs: Vec<Seg2>) -> LoftSection {
        LoftSection {
            origin: Vec3::new(0.0, 0.0, z),
            x: Vec3::new(1.0, 0.0, 0.0),
            y: Vec3::new(0.0, 1.0, 0.0),
            segs,
        }
    }

    fn starts(m: &MatchedSection) -> Vec<Vec2> {
        m.segs.iter().map(Seg2::start).collect()
    }

    fn near(a: Vec2, b: (f64, f64)) -> bool {
        dist(a, p(b.0, b.1)) < 1e-9
    }

    /// The vertex of the second section that vertex `a` of the first runs to.
    fn partner(m: &[MatchedSection], a: (f64, f64)) -> Vec2 {
        let k = starts(&m[0]).iter().position(|q| near(*q, a)).unwrap();
        starts(&m[1])[k]
    }

    /// Every segment has length, and each runs over at least one given segment.
    fn sound(m: &[MatchedSection]) {
        for s in m {
            assert_eq!(s.segs.len(), s.from.len());
            assert!(s.segs.iter().all(|x| length(x) > 0.0), "{:?}", s.segs);
            assert!(s.from.iter().all(|f| !f.is_empty()));
        }
    }

    /// The next number of a fixed sequence, in [0, 1).
    fn next(seed: &mut u64) -> f64 {
        *seed = seed
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (*seed >> 11) as f64 / (1u64 << 53) as f64
    }

    /// A counter-clockwise polygon of `n` vertices round `centre`, each at its own angle and
    /// distance (so the polygon is star-shaped but neither convex nor regular).
    fn random_polygon(seed: &mut u64, n: usize, centre: Vec2) -> Vec<Seg2> {
        let pts: Vec<(f64, f64)> = (0..n)
            .map(|k| {
                let a = (k as f64 + 0.8 * next(seed)) * std::f64::consts::TAU / n as f64;
                let q = centre + from_angle(a) * (5.0 + 10.0 * next(seed));
                (q.x, q.y)
            })
            .collect();
        poly(&pts)
    }

    /// The pairing's view of `l`, in the plane's own axes.
    fn side(l: &[Seg2]) -> Side<'_> {
        let frame = Frame::new(p(1.0, 0.0), p(0.0, 1.0), l);
        Side::new(l, frame).unwrap()
    }

    /// The search between neighbouring starts' walks finds a walk as cheap as trying every start
    /// over the whole grid.
    #[test]
    fn the_search_over_starts_is_as_good_as_trying_every_start() {
        let mut seed = 11u64;
        for _ in 0..80 {
            let m = 3 + (next(&mut seed) * 10.0) as usize;
            let n = 3 + (next(&mut seed) * 10.0) as usize;
            let a = random_polygon(&mut seed, m, Vec2::ZERO);
            let offset = p(next(&mut seed) * 4.0, next(&mut seed) * 4.0);
            let b = random_polygon(&mut seed, n, offset);
            let g = Grid {
                a: side(&a),
                b: side(&b),
            };
            let fast = best_walk(&g).unwrap();
            let slow = (0..n)
                .filter_map(|s| walk(&g, s, &vec![0; m + 1], &vec![2 * n; m + 1]))
                .map(|w| w.cost)
                .fold(f64::INFINITY, f64::min);
            assert!(
                (fast.cost - slow).abs() <= 1e-9 * slow.max(1.0),
                "{m} by {n}: {} against {slow}",
                fast.cost
            );
            // Its steps use every vertex of both loops once.
            let count = |s: Step| fast.steps.iter().filter(|x| **x == s).count();
            assert_eq!(count(Step::Pair) + count(Step::A), m);
            assert_eq!(count(Step::Pair) + count(Step::B), n);
        }
    }

    /// A 25 × 15 rectangle ending in a point (two tangent arcs meeting at (37, 7.5)) lofted 15 up
    /// to a 40.4 × 25 slot, both symmetric about y = 7.5. The slot's four segments have to become
    /// five: each slot vertex pairs with the corner it lines up with and the point with the
    /// middle of the round end (splitting the slot's longest segment in half split a long side).
    #[test]
    fn a_point_pairs_with_the_middle_of_a_round_end() {
        let tip = (37.0, 7.5);
        let point = vec![
            line((0.0, 0.0), (25.0, 0.0)),
            arc((25.0, 13.35), (25.0, 0.0), tip),
            arc((25.0, 1.65), tip, (25.0, 15.0)),
            line((25.0, 15.0), (0.0, 15.0)),
            line((0.0, 15.0), (0.0, 0.0)),
        ];
        let x = 35.4;
        let slot = vec![
            line((-5.0, 20.0), (-5.0, -5.0)),
            line((-5.0, -5.0), (x, -5.0)),
            arc((x, 7.5), (x, -5.0), (x, 20.0)),
            line((x, 20.0), (-5.0, 20.0)),
        ];
        let m = match_loft_sections(&[at(0.0, point), at(15.0, slot)], 100).unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (5, 5));
        assert!(
            near(partner(&m, tip), (x + 12.5, 7.5)),
            "{:?}",
            partner(&m, tip)
        );
        for (a, b) in [
            ((0.0, 0.0), (-5.0, -5.0)),
            ((25.0, 0.0), (x, -5.0)),
            ((25.0, 15.0), (x, 20.0)),
            ((0.0, 15.0), (-5.0, 20.0)),
        ] {
            assert!(
                near(partner(&m, a), b),
                "{a:?} runs to {:?}",
                partner(&m, a)
            );
        }
    }

    /// Two hood profiles (a thin band with a hooked lip at each end): 17 and 18 vertices, and
    /// each has a vertex the other lacks, on a straight side or nearly so (the second's right side
    /// has a point on it at (27.6, 40.8), its left side one at (-22.8, 41.25); the first's arch one
    /// at (-11.25, 48.47)).
    const HOOD_A: [(f64, f64); 17] = [
        (22.275732672046672, 42.4194875370215),
        (24.15373450104431, 42.5852135407361),
        (24.15373450104431, 46.40321410660799),
        (13.601185077757782, 48.265425651573544),
        (-7.956143139645345, 49.24551439515723),
        (-11.252079715296125, 48.4718337596306),
        (-18.555464567800477, 45.80523471830436),
        (-18.50202907367915, 43.007831671402265),
        (-17.2, 42.906809746055835),
        (-17.2, 41.46),
        (-18.723959594727756, 41.251188115959096),
        (-22.8, 41.251188115959096),
        (-22.8, 52.6),
        (27.600000000000005, 52.6),
        (27.600000000000005, 40.800000000000004),
        (24.1, 40.800000000000004),
        (22.31070331305888, 40.957898711557526),
    ];

    /// The second hood profile ([`HOOD_A`]).
    const HOOD_B: [(f64, f64); 18] = [
        (-22.8, 41.251188115959096),
        (-22.8, 52.6),
        (27.600000000000005, 52.6),
        (27.600000000000005, 40.800000000000004),
        (27.600000000000005, 38.0),
        (24.0, 38.0),
        (22.405477280919964, 38.20705014188878),
        (22.405477280919964, 39.550000000000004),
        (24.924192673472707, 39.772267172447684),
        (24.924192673472707, 42.84813511542108),
        (15.250770475114546, 45.97354789328374),
        (-9.315605172301247, 47.10818298471791),
        (-19.42968353817282, 42.3110290048853),
        (-19.42968353817282, 40.25972628736644),
        (-17.525020629683333, 40.10220901755372),
        (-17.525020629683333, 38.64960388282376),
        (-19.327507117348198, 38.51668382378396),
        (-22.398563727493077, 38.51668382378396),
    ];

    /// A hood profile on the plane x = `x` (sketch x along Y, sketch y along Z).
    fn hood(x: f64, pts: &[(f64, f64)]) -> LoftSection {
        LoftSection {
            origin: Vec3::new(x, 0.0, 0.0),
            x: Vec3::new(0.0, 1.0, 0.0),
            y: Vec3::new(0.0, 0.0, 1.0),
            segs: poly(pts),
        }
    }

    /// Each corner of the first hood profile runs to the corner of the second it lines up with,
    /// and each of the three vertices only one has splits the other's side where it lands: 19
    /// vertices in each section. Splitting the one with fewer vertices to 18 shifted one of the
    /// lips by a vertex.
    #[test]
    fn corners_pair_when_each_profile_has_a_vertex_the_other_lacks() {
        let m = match_loft_sections(&[hood(29.0, &HOOD_A), hood(50.0, &HOOD_B)], 100).unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (19, 19));
        let pairs = [
            (0, 7),
            (1, 8),
            (2, 9),
            (3, 10),
            (4, 11),
            (6, 12),
            (7, 13),
            (8, 14),
            (9, 15),
            (10, 16),
            (11, 17),
            (12, 1),
            (13, 2),
            (14, 4),
            (15, 5),
            (16, 6),
        ];
        for (i, j) in pairs {
            assert!(
                near(partner(&m, HOOD_A[i]), HOOD_B[j]),
                "{:?} runs to {:?}",
                HOOD_A[i],
                partner(&m, HOOD_A[i])
            );
        }
        let back = |b: (f64, f64)| {
            let k = starts(&m[1]).iter().position(|q| near(*q, b)).unwrap();
            starts(&m[0])[k]
        };
        // The second profile's point on its left side splits the first's left side, and its point
        // on the right side the first's right side.
        let q = back(HOOD_B[0]);
        assert!(
            (q.x + 22.8).abs() < 1e-9 && q.y > 41.25 && q.y < 52.6,
            "{q:?}"
        );
        let q = back(HOOD_B[3]);
        assert!(
            (q.x - 27.6).abs() < 1e-9 && q.y > 40.8 && q.y < 52.6,
            "{q:?}"
        );
        // The first profile's vertex on its arch splits the second's arch between the corners
        // either side of it.
        let (r, a, b) = (
            partner(&m, HOOD_A[5]),
            p(HOOD_B[11].0, HOOD_B[11].1),
            p(HOOD_B[12].0, HOOD_B[12].1),
        );
        let along = dot(r - a, b - a) / len2(b - a);
        assert!(
            ((r.x - a.x) * (b.y - a.y) - (r.y - a.y) * (b.x - a.x)).abs() < 1e-9 * len2(b - a)
                && along > 0.0
                && along < 1.0,
            "{r:?}"
        );
        // The sections given the other way round pair the same points.
        let r = match_loft_sections(&[hood(50.0, &HOOD_B), hood(29.0, &HOOD_A)], 100).unwrap();
        let mut ab: Vec<(Vec2, Vec2)> = starts(&m[0]).into_iter().zip(starts(&m[1])).collect();
        let mut ba: Vec<(Vec2, Vec2)> = starts(&r[1]).into_iter().zip(starts(&r[0])).collect();
        let key = |x: &(Vec2, Vec2)| (x.0.x, x.0.y);
        ab.sort_by(|x, y| key(x).partial_cmp(&key(y)).unwrap());
        ba.sort_by(|x, y| key(x).partial_cmp(&key(y)).unwrap());
        assert_eq!(ab.len(), ba.len());
        for (x, y) in ab.iter().zip(&ba) {
            assert!(
                dist(x.0, y.0) < 1e-9 && dist(x.1, y.1) < 1e-9,
                "{x:?} vs {y:?}"
            );
        }
    }

    /// Three sections, the first hood profile, the second, then the first again: the middle one
    /// gains the vertex only the outer ones have, so all three get the same 19 vertices and the
    /// outer two are cut alike.
    #[test]
    fn three_sections_pair_each_with_the_next() {
        let m = match_loft_sections(
            &[
                hood(29.0, &HOOD_A),
                hood(50.0, &HOOD_B),
                hood(71.0, &HOOD_A),
            ],
            100,
        )
        .unwrap();
        sound(&m);
        let counts: Vec<usize> = m.iter().map(|s| s.segs.len()).collect();
        assert_eq!(counts, [19, 19, 19]);
        for (a, b) in starts(&m[0]).iter().zip(starts(&m[2])) {
            assert!(dist(*a, b) < 1e-9, "{a:?} vs {b:?}");
        }
        for (i, j) in [(15, 5), (10, 16), (14, 4)] {
            assert!(near(partner(&m, HOOD_A[i]), HOOD_B[j]));
        }
    }

    /// A rectangle with a point in the middle of its left side lofted to one with a point in the
    /// middle of its right side: each point splits the other rectangle's side, so every vertex
    /// runs straight up (pairing the five vertices in turn joined the left point to a corner).
    #[test]
    fn a_point_only_the_other_section_has_splits_a_straight_side() {
        let left = poly(&[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
            (0.0, 10.0),
        ]);
        let right = poly(&[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 10.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let m = match_loft_sections(&[at(0.0, left), at(30.0, right)], 100).unwrap();
        sound(&m);
        assert_eq!(m[0].segs.len(), 6);
        for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
            assert!(dist(*a, b) < 1e-9, "{a:?} runs to {b:?}");
        }
    }

    /// The rectangle with a point on its left side and the one with a point on its right side
    /// ([`a_point_only_the_other_section_has_splits_a_straight_side`]), the second on a plane
    /// tilted 30 degrees about X, then on a plane facing back (its sketch y down, the profile
    /// drawn mirrored so it is the same in space): the frame carried from plane to plane pairs
    /// the same points.
    #[test]
    fn tilted_and_turned_planes_pair_the_same_points() {
        let left = poly(&[
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
            (0.0, 10.0),
        ]);
        let right = [
            (0.0, 0.0),
            (40.0, 0.0),
            (40.0, 10.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ];
        let (c, s) = ((PI / 6.0).cos(), (PI / 6.0).sin());
        let tilted = LoftSection {
            origin: Vec3::new(0.0, 0.0, 30.0),
            x: Vec3::new(1.0, 0.0, 0.0),
            y: Vec3::new(0.0, c, s),
            segs: poly(&right),
        };
        let mirrored: Vec<(f64, f64)> = right.iter().map(|&(x, y)| (x, -y)).collect();
        let turned = LoftSection {
            origin: Vec3::new(0.0, 0.0, 30.0),
            x: Vec3::new(1.0, 0.0, 0.0),
            y: Vec3::new(0.0, -1.0, 0.0),
            segs: poly(&mirrored),
        };
        for (second, flip) in [(tilted, 1.0), (turned, -1.0)] {
            let m = match_loft_sections(&[at(0.0, left.clone()), second], 100).unwrap();
            sound(&m);
            assert_eq!((m[0].segs.len(), m[1].segs.len()), (6, 6));
            for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
                assert!(dist(*a, p(b.x, flip * b.y)) < 1e-9, "{a:?} runs to {b:?}");
            }
        }
    }

    /// A rectangle lofted to the same rectangle with its corners rounded (radius 4), both orders:
    /// each corner runs to a point near the middle of its rounded corner, and the rounded
    /// corners' ends split the rectangle's sides next to the corners (12 vertices each).
    #[test]
    fn a_corner_runs_to_the_middle_of_a_rounded_corner() {
        let (w, h, r) = (40.0, 20.0, 4.0);
        let corner = |c: (f64, f64), start: f64| Seg2::Arc {
            center: p(c.0, c.1),
            radius: r,
            start,
            sweep: PI / 2.0,
        };
        let rounded = vec![
            line((r, 0.0), (w - r, 0.0)),
            corner((w - r, r), -PI / 2.0),
            line((w, r), (w, h - r)),
            corner((w - r, h - r), 0.0),
            line((w - r, h), (r, h)),
            corner((r, h - r), PI / 2.0),
            line((0.0, h - r), (0.0, r)),
            corner((r, r), PI),
        ];
        let square = poly(&[(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)]);
        for flip in [false, true] {
            let secs = if flip {
                [at(0.0, rounded.clone()), at(20.0, square.clone())]
            } else {
                [at(0.0, square.clone()), at(20.0, rounded.clone())]
            };
            let m = match_loft_sections(&secs, 100).unwrap();
            sound(&m);
            assert!(m.iter().all(|s| s.segs.len() == 12));
            let (sq, ro) = if flip { (&m[1], &m[0]) } else { (&m[0], &m[1]) };
            for (x, y, cx, cy) in [
                (0.0, 0.0, r, r),
                (w, 0.0, w - r, r),
                (w, h, w - r, h - r),
                (0.0, h, r, h - r),
            ] {
                let k = starts(sq).iter().position(|q| near(*q, (x, y))).unwrap();
                let (c, diagonal) = (p(cx, cy), p(x - cx, y - cy));
                let d = starts(ro)[k] - c;
                assert!(
                    (len(d) - r).abs() < 1e-9
                        && dot(d, diagonal) / (len(d) * len(diagonal)) > 10f64.to_radians().cos(),
                    "corner ({x}, {y}) runs to {:?}",
                    starts(ro)[k]
                );
            }
        }
    }

    /// A circle between two squares: it splits at the squares' corner directions (four arcs) and
    /// starts at the one the squares' first corner points to.
    #[test]
    fn a_circle_between_two_sections_splits_at_their_corners() {
        let square = poly(&[(-10.0, -10.0), (10.0, -10.0), (10.0, 10.0), (-10.0, 10.0)]);
        let circle = vec![Seg2::Arc {
            center: Vec2::ZERO,
            radius: 10.0,
            start: 0.0,
            sweep: 2.0 * PI,
        }];
        let m = match_loft_sections(
            &[at(0.0, square.clone()), at(20.0, circle), at(40.0, square)],
            100,
        )
        .unwrap();
        sound(&m);
        let counts: Vec<usize> = m.iter().map(|s| s.segs.len()).collect();
        assert_eq!(counts, [4, 4, 4]);
        for (q, c) in starts(&m[0]).iter().zip(starts(&m[1])) {
            assert!(
                dist(*q * (1.0 / len(*q)) * 10.0, c) < 1e-9,
                "{q:?} runs to {c:?}"
            );
        }
        assert!(m[1].from.iter().all(|f| f == &[0]));
    }

    /// A segment of no length (a corner drawn twice) has no vertex of its own: the loft pairs the
    /// rest, and the piece before it runs over it.
    #[test]
    fn a_segment_of_no_length_is_left_out() {
        let mut twice = poly(&[(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]);
        twice.insert(1, line((40.0, 0.0), (40.0, 0.0)));
        let plain = poly(&[(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]);
        for secs in [
            [at(0.0, twice.clone()), at(10.0, plain.clone())],
            [at(0.0, plain.clone()), at(10.0, twice.clone())],
        ] {
            let m = match_loft_sections(&secs, 100).unwrap();
            sound(&m);
            assert_eq!((m[0].segs.len(), m[1].segs.len()), (4, 4));
            let with = if secs[0].segs.len() == 5 {
                &m[0]
            } else {
                &m[1]
            };
            assert!(with.from.iter().any(|f| f == &[0, 1]), "{:?}", with.from);
            for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
                assert!(dist(*a, b) < 1e-9, "{a:?} runs to {b:?}");
            }
        }
    }

    /// A section with no area (a slit, there and back along one line) still pairs.
    #[test]
    fn a_section_with_no_area_pairs() {
        let slit = vec![
            line((-10.0, 0.0), (10.0, 0.0)),
            line((10.0, 0.0), (-10.0, 0.0)),
        ];
        let square = poly(&[(-10.0, -5.0), (10.0, -5.0), (10.0, 5.0), (-10.0, 5.0)]);
        let m = match_loft_sections(&[at(0.0, slit), at(10.0, square)], 100).unwrap();
        sound(&m);
        assert_eq!(m[0].segs.len(), m[1].segs.len());
    }

    /// Two lone vertices that land past the end of one segment of the other loop (two vertices a
    /// tenth of a nanometre before a corner) are cut apart along it, in order, not refused: to
    /// [`MIN_EDGE`] apart and from the corner, so the other loop gets no edge shorter than that.
    #[test]
    fn lone_vertices_landing_on_one_point_are_spread_apart() {
        let close = poly(&[
            (0.0, 0.0),
            (40.0 - 2e-10, 0.0),
            (40.0 - 1e-10, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let plain = poly(&[(0.0, 0.0), (40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]);
        let m = match_loft_sections(&[at(0.0, close), at(10.0, plain)], 100).unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (6, 6));
        for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
            assert!(dist(*a, b) < 2.0 * MIN_EDGE + 1e-9, "{a:?} runs to {b:?}");
        }
        let shortest = m[1].segs.iter().map(length).fold(f64::INFINITY, f64::min);
        assert!(
            shortest >= MIN_EDGE * (1.0 - 1e-9),
            "shortest edge {shortest}"
        );
    }

    /// The regular `n`-gon of radius `r` about the origin, its first vertex on +x.
    fn gon(n: usize, r: f64) -> Vec<Seg2> {
        let pts: Vec<(f64, f64)> = (0..n)
            .map(|k| {
                let q = from_angle(k as f64 * std::f64::consts::TAU / n as f64) * r;
                (q.x, q.y)
            })
            .collect();
        poly(&pts)
    }

    /// Sections whose grid would be past [`MAX_CELLS`] are paired through a band of it, not
    /// refused: two 5000-gons pair vertex for vertex, and a 3000-gon lofted to a 5000-gon of the
    /// same radius runs each vertex to the point of the other in about its direction.
    #[test]
    fn sections_too_large_for_the_whole_grid_are_paired_in_a_band() {
        let m = match_loft_sections(
            &[at(0.0, gon(5000, 10.0)), at(10.0, gon(5000, 20.0))],
            20_000,
        )
        .unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (5000, 5000));
        for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
            assert!(dist(*a * 2.0, b) < 1e-9, "{a:?} runs to {b:?}");
        }
        let m = match_loft_sections(
            &[at(0.0, gon(3000, 10.0)), at(10.0, gon(5000, 10.0))],
            20_000,
        )
        .unwrap();
        sound(&m);
        assert_eq!(m[0].segs.len(), m[1].segs.len());
        for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
            let turn = (angle(b) - angle(*a)).rem_euclid(2.0 * PI);
            assert!(
                turn.min(2.0 * PI - turn) < 2.0 * PI / 3000.0,
                "{a:?} runs to {b:?}"
            );
        }
        // Past the caller's bound, refused.
        assert!(
            match_loft_sections(&[at(0.0, gon(3000, 10.0)), at(10.0, gon(5000, 10.0))], 4000)
                .is_err()
        );
    }

    /// The 40 x 20 rectangle in 5000 segments, `split` of them along its bottom side or along its
    /// top side.
    fn split_rectangle(top: bool, split: usize) -> Vec<Seg2> {
        let mut pts = vec![(0.0, 0.0)];
        let along = |k: usize| 40.0 * k as f64 / split as f64;
        if top {
            pts.extend([(40.0, 0.0), (40.0, 20.0)]);
            pts.extend((1..split).map(|k| (40.0 - along(k), 20.0)));
            pts.push((0.0, 20.0));
        } else {
            pts.extend((1..split).map(|k| (along(k), 0.0)));
            pts.extend([(40.0, 0.0), (40.0, 20.0), (0.0, 20.0)]);
        }
        poly(&pts)
    }

    /// Two 5000-segment rectangles past [`MAX_CELLS`], one split along its bottom and the other
    /// along its top, pair all four corners: their vertex numbers differ by about 5000 at two of
    /// them, which no one turn of the numbering (lofts before the pairing) can line up.
    #[test]
    fn corners_pair_in_sections_too_large_for_the_whole_grid() {
        let (a, b) = (split_rectangle(false, 4997), split_rectangle(true, 4997));
        assert_eq!((a.len(), b.len()), (5000, 5000));
        let m = match_loft_sections(&[at(0.0, a), at(10.0, b)], 20_000).unwrap();
        sound(&m);
        assert_eq!(m[0].segs.len(), m[1].segs.len());
        for c in [p(0.0, 0.0), p(40.0, 0.0), p(40.0, 20.0), p(0.0, 20.0)] {
            let k = starts(&m[0])
                .iter()
                .position(|q| dist(*q, c) < 1e-12)
                .unwrap();
            assert!(
                dist(starts(&m[1])[k], c) < 1e-9,
                "{c:?} runs to {:?}",
                starts(&m[1])[k]
            );
        }
    }

    /// The `w` x `h` rectangle from (0, 0), its bottom, right, top and left sides each cut into
    /// `cuts` equal segments.
    fn cut_rectangle(w: f64, h: f64, cuts: [usize; 4]) -> Vec<Seg2> {
        let corners = [(0.0, 0.0), (w, 0.0), (w, h), (0.0, h)];
        let mut pts = Vec::new();
        for (k, &c) in cuts.iter().enumerate() {
            let (a, b) = (corners[k], corners[(k + 1) % 4]);
            for q in 0..c {
                let t = q as f64 / c as f64;
                pts.push((a.0 + (b.0 - a.0) * t, a.1 + (b.1 - a.1) * t));
            }
        }
        poly(&pts)
    }

    /// The band past [`MAX_CELLS`] finds the least-cost walk from its start over the whole grid
    /// where that walk is far from where the shares put it: a 10 x 1 and a 1.5 x 1 rectangle in
    /// 498 and 500 segments of about equal length (their corners at 45% and 30% of their
    /// perimeters), paired through a band of a hundredth of their grid (h = 39) and moved onto
    /// the walk, cost what the walk from the same start through the whole grid costs. (The search
    /// over every start finds one 1.8% cheaper here: the band keeps its one start.)
    #[test]
    fn the_band_finds_the_whole_grids_walk_from_its_start() {
        let a = cut_rectangle(10.0, 1.0, [227, 22, 227, 22]);
        let b = cut_rectangle(1.5, 1.0, [150, 100, 150, 100]);
        let frame = |segs: &[Seg2]| Frame::new(p(1.0, 0.0), p(0.0, 1.0), segs);
        let g = Grid {
            a: Side::new(&a, frame(&a)).unwrap(),
            b: Side::new(&b, frame(&b)).unwrap(),
        };
        let banded = banded_walk(&g, true, 40_000).unwrap();
        let (m, n) = (g.a.count(), g.b.count());
        let whole = walk(&g, banded.start, &vec![0; m + 1], &vec![2 * n; m + 1]).unwrap();
        assert!(
            (banded.cost - whole.cost).abs() <= 1e-9 * whole.cost,
            "band {} whole grid {}",
            banded.cost,
            whole.cost
        );
        let every = best_walk(&g).unwrap();
        assert!(every.cost <= banded.cost);
    }

    /// A square lofted to a diamond (a square turned 45 degrees, its corners on the axes) with a
    /// vertex in the middle of each side, where the square's corners point: each corner of the
    /// square runs straight up to the vertex in its direction, as lofts did before the pairing,
    /// not to a corner of the diamond (a turn of 45 degrees).
    #[test]
    fn a_square_to_a_diamond_runs_its_corners_straight_up() {
        let r = std::f64::consts::SQRT_2;
        let h = r / 2.0;
        let square = poly(&[(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)]);
        let diamond = poly(&[
            (r, 0.0),
            (h, h),
            (0.0, r),
            (-h, h),
            (-r, 0.0),
            (-h, -h),
            (0.0, -r),
            (h, -h),
        ]);
        let m = match_loft_sections(&[at(0.0, square), at(10.0, diamond)], 100).unwrap();
        sound(&m);
        for c in [(1.0, 1.0), (-1.0, 1.0), (-1.0, -1.0), (1.0, -1.0)] {
            let q = partner(&m, c);
            assert!(near(q, (c.0 * h, c.1 * h)), "{c:?} runs to {q:?}");
        }
    }

    /// A stadium (two half circles of radius 0.5 joined by straight sides `side` long) as a
    /// polygon of `n` vertices at equal lengths along it.
    fn stadium(side: f64, n: usize) -> Vec<Seg2> {
        let r = 0.5;
        let total = 2.0 * side + 2.0 * PI * r;
        let at_length = |d: f64| -> (f64, f64) {
            let half = side / 2.0;
            let arc = PI * r;
            if d < side {
                (-half + d, -r)
            } else if d < side + arc {
                let a = -PI / 2.0 + (d - side) / r;
                (half + r * a.cos(), r * a.sin())
            } else if d < 2.0 * side + arc {
                (half - (d - side - arc), r)
            } else {
                let a = PI / 2.0 + (d - 2.0 * side - arc) / r;
                (-half + r * a.cos(), r * a.sin())
            }
        };
        let pts: Vec<(f64, f64)> = (0..n)
            .map(|k| at_length(total * k as f64 / n as f64))
            .collect();
        poly(&pts)
    }

    /// Stadiums of 14 000 and of 12 000 vertices each, of different proportions (straight sides
    /// 9 against 0.5, and 30 against 0.5), past [`MAX_CELLS`]: lofts built them before the
    /// pairing, and the pairing builds them, within the kernel's 20 000 segments.
    #[test]
    fn large_stadiums_of_different_proportions_pair() {
        for (n, side) in [(14_000, 9.0), (12_000, 30.0)] {
            let m = match_loft_sections(
                &[at(0.0, stadium(side, n)), at(10.0, stadium(0.5, n))],
                20_000,
            )
            .unwrap();
            sound(&m);
            assert_eq!(m[0].segs.len(), m[1].segs.len());
            assert!(m[0].segs.len() <= 20_000, "{}", m[0].segs.len());
        }
    }

    /// A `w` x 1 rectangle whose bottom is a comb of `teeth` teeth 0.1 deep: every vertex of the
    /// comb a sharp corner.
    fn comb(w: f64, teeth: usize) -> Vec<Seg2> {
        let mut pts: Vec<(f64, f64)> = (0..2 * teeth)
            .map(|k| {
                (
                    w * k as f64 / (2 * teeth) as f64,
                    if k % 2 == 1 { -0.1 } else { 0.0 },
                )
            })
            .collect();
        pts.extend([(w, 0.0), (w, 1.0), (0.0, 1.0)]);
        poly(&pts)
    }

    /// A 10-wide and a 1.5-wide comb of 1100 teeth (2203 corners each, so even the grid of the
    /// polygons through their corners is past [`MAX_CELLS`]) pair every tooth with its tooth and
    /// the four corners of the rectangles with each other.
    #[test]
    fn combs_of_more_corners_than_the_grid_bound_pair_tooth_by_tooth() {
        let teeth = 1100;
        let (a, b) = (comb(10.0, teeth), comb(1.5, teeth));
        assert!((a.len() + 1) * (b.len() + 1) > MAX_CELLS);
        let m = match_loft_sections(&[at(0.0, a), at(10.0, b)], 20_000).unwrap();
        sound(&m);
        assert_eq!(m[0].segs.len(), m[1].segs.len());
        let tip = |w: f64, k: usize| p(w * (2 * k + 1) as f64 / (2 * teeth) as f64, -0.1);
        let pairs = (0..teeth).map(|k| (tip(10.0, k), tip(1.5, k))).chain([
            (p(10.0, 0.0), p(1.5, 0.0)),
            (p(10.0, 1.0), p(1.5, 1.0)),
            (p(0.0, 1.0), p(0.0, 1.0)),
        ]);
        for (x, y) in pairs {
            let k = starts(&m[0])
                .iter()
                .position(|q| dist(*q, x) < 1e-12)
                .unwrap();
            assert!(
                dist(starts(&m[1])[k], y) < 1e-9,
                "{x:?} runs to {:?}",
                starts(&m[1])[k]
            );
        }
    }

    /// A 10 x 1 comb of 1100 teeth and a 1.5-wide one with sides 200 high (2203 corners each):
    /// the second's teeth take a third of its length and the first's nearly all, so where the
    /// shares put the right corner and the teeth is some 1300 vertices from where they are,
    /// three times the band's half width. Moved onto the walk it finds, the band reaches them,
    /// and every tooth and corner pairs.
    #[test]
    fn combs_of_different_proportions_pair_tooth_by_tooth() {
        let teeth = 1100;
        let a = comb(10.0, teeth);
        let mut b = comb(1.5, teeth);
        let n = b.len();
        b[n - 3] = line((1.5, 0.0), (1.5, 200.0));
        b[n - 2] = line((1.5, 200.0), (0.0, 200.0));
        b[n - 1] = line((0.0, 200.0), (0.0, 0.0));
        let m = match_loft_sections(&[at(0.0, a), at(10.0, b)], 20_000).unwrap();
        sound(&m);
        assert_eq!(m[0].segs.len(), m[1].segs.len());
        let tip = |w: f64, k: usize| p(w * (2 * k + 1) as f64 / (2 * teeth) as f64, -0.1);
        let pairs = (0..teeth).map(|k| (tip(10.0, k), tip(1.5, k))).chain([
            (p(10.0, 0.0), p(1.5, 0.0)),
            (p(10.0, 1.0), p(1.5, 200.0)),
            (p(0.0, 1.0), p(0.0, 200.0)),
        ]);
        for (x, y) in pairs {
            let k = starts(&m[0])
                .iter()
                .position(|q| dist(*q, x) < 1e-12)
                .unwrap();
            assert!(
                dist(starts(&m[1])[k], y) < 1e-9,
                "{x:?} runs to {:?}",
                starts(&m[1])[k]
            );
        }
    }

    /// A half circle and the same half circle back (arcs of one centre and radius that do not go
    /// round once) is not a full circle: matched as any other loop, it stays on its half.
    #[test]
    fn a_half_circle_and_its_reverse_is_not_a_circle() {
        let back = vec![
            Seg2::Arc {
                center: p(0.0, 0.0),
                radius: 5.0,
                start: 0.0,
                sweep: PI,
            },
            Seg2::Arc {
                center: p(0.0, 0.0),
                radius: 5.0,
                start: PI,
                sweep: -PI,
            },
        ];
        assert!(!is_circle(&back));
        let whole = |k: usize| Seg2::Arc {
            center: p(0.0, 0.0),
            radius: 5.0,
            start: k as f64 * PI,
            sweep: PI,
        };
        assert!(is_circle(&[whole(0), whole(1)]));
        let m = match_loft_sections(&[at(0.0, gon(4, 10.0)), at(10.0, back)], 100).unwrap();
        for s in &m[1].segs {
            for t in [0.0, 0.25, 0.5, 0.75, 1.0] {
                assert!(s.point_at(t).y > -1e-9, "{:?}", s.point_at(t));
            }
        }
    }

    /// A segment whose geometry is not finite, or a plane axis that is not a unit vector, is
    /// refused.
    #[test]
    fn geometry_that_is_not_finite_is_refused() {
        let mut bad = gon(4, 10.0);
        bad[1] = line((0.0, 10.0), (f64::NAN, 0.0));
        assert!(match_loft_sections(&[at(0.0, bad), at(10.0, gon(4, 10.0))], 100).is_err());
        let mut tilted = at(10.0, gon(4, 10.0));
        tilted.x = Vec3::new(2.0, 0.0, 0.0);
        assert!(match_loft_sections(&[at(0.0, gon(4, 10.0)), tilted], 100).is_err());
    }

    /// The segments' lengths, of every matched section.
    fn lengths(m: &[MatchedSection]) -> Vec<f64> {
        m.iter().flat_map(|s| s.segs.iter().map(length)).collect()
    }

    fn refused(r: &Result<Vec<MatchedSection>>) -> bool {
        matches!(r, Err(SolidError::InvalidInput { .. }))
    }

    /// Plane axes of unit length that are not perpendicular are refused.
    #[test]
    fn plane_axes_that_are_not_perpendicular_are_refused() {
        let mut skew = at(10.0, gon(4, 10.0));
        skew.y = Vec3::new(0.01f64.sin(), 0.01f64.cos(), 0.0);
        assert!(refused(&match_loft_sections(
            &[at(0.0, gon(4, 10.0)), skew],
            100
        )));
    }

    /// A full circle of one arc is matched by its centre, radius and angles, which are not
    /// checked on the way the other sections are: one that is not finite, or of no radius (no
    /// length), is refused.
    #[test]
    fn a_circle_that_is_not_finite_is_refused() {
        let circle = |center: Vec2, radius: f64| Seg2::Arc {
            center,
            radius,
            start: 0.0,
            sweep: 2.0 * PI,
        };
        for c in [
            circle(p(f64::NAN, 0.0), 5.0),
            circle(p(0.0, 0.0), f64::NAN),
            circle(p(0.0, 0.0), 0.0),
        ] {
            assert!(refused(&match_loft_sections(
                &[at(0.0, gon(4, 10.0)), at(10.0, vec![c])],
                100
            )));
        }
    }

    /// A circle after a 40 x 20 rectangle splits at the rectangle's corner directions, and each
    /// of its vertices runs to the corner in its direction, whether the circle's plane has the
    /// rectangle's axes or its y axis turned over (its plane facing back). Before, the corners
    /// seen from the plane facing back went round clockwise, were not taken as one turn, and the
    /// circle split at its perimeter shares instead.
    #[test]
    fn a_circle_on_a_plane_facing_back_splits_at_the_corners() {
        let rect = poly(&[(-20.0, -10.0), (20.0, -10.0), (20.0, 10.0), (-20.0, 10.0)]);
        let circle = vec![Seg2::Arc {
            center: p(0.0, 0.0),
            radius: 5.0,
            start: 0.0,
            sweep: 2.0 * PI,
        }];
        for flip in [1.0, -1.0] {
            let c = LoftSection {
                origin: Vec3::new(0.0, 0.0, 10.0),
                x: Vec3::new(1.0, 0.0, 0.0),
                y: Vec3::new(0.0, flip, 0.0),
                segs: circle.clone(),
            };
            let m = match_loft_sections(&[at(0.0, rect.clone()), c], 100).unwrap();
            sound(&m);
            assert_eq!(m[1].segs.len(), 4);
            for (a, b) in starts(&m[0]).iter().zip(starts(&m[1])) {
                let world = p(b.x, flip * b.y);
                let (da, db) = (unit(*a).unwrap(), unit(world).unwrap());
                assert!(dist(da, db) < 1e-9, "flip {flip}: {a:?} runs to {world:?}");
            }
        }
    }

    /// A lone vertex lands at its share of the length between two pairs, also on a curve whose
    /// length is not proportional to its parameter: a cubic along the x axis from (0, 0) to
    /// (40, 0) with its first three control points together, so its length from the start to a
    /// point is that point's x, and half its length is at x = 20 (its parameter 0.5 is at x = 5).
    #[test]
    fn a_lone_vertex_lands_at_its_share_of_a_curves_length() {
        let a = vec![
            Seg2::Cubic {
                p0: p(0.0, 0.0),
                p1: p(0.0, 0.0),
                p2: p(0.0, 0.0),
                p3: p(40.0, 0.0),
            },
            line((40.0, 0.0), (40.0, 20.0)),
            line((40.0, 20.0), (0.0, 20.0)),
            line((0.0, 20.0), (0.0, 0.0)),
        ];
        let b = poly(&[
            (0.0, 0.0),
            (20.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let m = match_loft_sections(&[at(0.0, a), at(10.0, b)], 100).unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (5, 5));
        let k = starts(&m[1])
            .iter()
            .position(|q| dist(*q, p(20.0, 0.0)) < 1e-12)
            .unwrap();
        let q = starts(&m[0])[k];
        assert!(
            q.y.abs() < 1e-12 && (q.x - 20.0).abs() < 1e-9,
            "(20, 0) runs to {q:?}"
        );
    }

    /// Two cuts of one conic land on the conic at their own parameters: the conic from (0, 0)
    /// through the apex (1, 2) to (2, 0) of weight 7 is at (1, 7/4) at 0.5 and at
    /// (15/13, 21/13) at 0.75. Before, the second cut was taken on the first cut's remaining
    /// piece with its end weights scaled to one, whose parameter is not the conic's moved
    /// linearly, and landed at (1.25, 35/24).
    #[test]
    fn two_cuts_of_one_conic_land_at_their_parameters() {
        let t = Traced {
            segs: vec![Seg2::Conic {
                a: p(0.0, 0.0),
                apex: p(1.0, 2.0),
                b: p(2.0, 0.0),
                w: 7.0,
            }],
            from: vec![vec![0]],
        };
        let c = t.cut(&[vec![0.5, 0.75]]).unwrap();
        assert_eq!(c.segs.len(), 3);
        assert!(dist(c.segs[1].start(), p(1.0, 1.75)) < 1e-12);
        assert!(
            dist(c.segs[2].start(), p(15.0 / 13.0, 21.0 / 13.0)) < 1e-12,
            "{:?}",
            c.segs[2].start()
        );
        assert!(dist(c.segs[2].end(), p(2.0, 0.0)) < 1e-12);
    }

    /// A vertex 8e-8 past the middle of a straight side and the tip of a V across that middle
    /// (their shares of the length between the corners differ by 2e-9) run to each other, and no
    /// matched edge is shorter than [`MIN_EDGE`].
    #[test]
    fn lone_vertices_whose_shares_nearly_agree_pair() {
        let a = poly(&[
            (0.0, 0.0),
            (20.0 + 8e-8, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let b = poly(&[
            (0.0, 0.0),
            (20.0, -10.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let m = match_loft_sections(&[at(0.0, a), at(10.0, b)], 100).unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (5, 5));
        assert!(
            lengths(&m).iter().all(|l| *l >= MIN_EDGE),
            "{:?}",
            lengths(&m)
        );
        let k = starts(&m[1])
            .iter()
            .position(|q| dist(*q, p(20.0, -10.0)) < 1e-12)
            .unwrap();
        assert!(dist(starts(&m[0])[k], p(20.0 + 8e-8, 0.0)) < 1e-12);
    }

    /// The share placement pairs two lone vertices whose shares of the length between two pairs
    /// differ by 2e-9 (8e-8 along the 40-long sides), whatever the walk left them as: left apart,
    /// each lands about 8e-8 from the other, an edge that short on both loops.
    #[test]
    fn the_share_placement_pairs_lone_vertices_closer_than_an_edge() {
        let a = poly(&[
            (0.0, 0.0),
            (20.0 + 8e-8, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let b = poly(&[
            (0.0, 0.0),
            (20.0, 0.0),
            (40.0, 0.0),
            (40.0, 20.0),
            (0.0, 20.0),
        ]);
        let frame = |segs: &[Seg2]| Frame::new(p(1.0, 0.0), p(0.0, 1.0), segs);
        let g = Grid {
            a: Side::new(&a, frame(&a)).unwrap(),
            b: Side::new(&b, frame(&b)).unwrap(),
        };
        let v = Place::Vertex;
        let events = [
            (v(0), v(0)),
            (v(1), Place::On(1, 0.5)),
            (Place::On(0, 0.5), v(1)),
            (v(2), v(2)),
            (v(3), v(3)),
            (v(4), v(4)),
        ];
        let out = by_share(&g, &events, &[0, 3, 4, 5]).unwrap();
        assert!(out.contains(&(v(1), v(1))), "{out:?}");
        assert_eq!(out.len(), 5);
    }

    /// A lone vertex 0.004 from a corner of a 4000-long side lands 4e-6 from the corner of the
    /// 4-long side it pairs with: it is moved to [`MIN_EDGE`] from the corner, so no matched edge
    /// is shorter than that where the sections have none.
    #[test]
    fn a_lone_vertex_landing_beside_a_corner_keeps_an_edge_away() {
        let a = poly(&[
            (0.0, 0.0),
            (0.004, 0.0),
            (4000.0, 0.0),
            (4000.0, 20.0),
            (0.0, 20.0),
        ]);
        let b = poly(&[(0.0, 0.0), (4.0, 0.0), (4.0, 20.0), (0.0, 20.0)]);
        let m = match_loft_sections(&[at(0.0, a), at(10.0, b)], 100).unwrap();
        sound(&m);
        assert_eq!((m[0].segs.len(), m[1].segs.len()), (5, 5));
        let shortest = lengths(&m).into_iter().fold(f64::INFINITY, f64::min);
        assert!(
            shortest >= MIN_EDGE * (1.0 - 1e-9),
            "shortest edge {shortest}"
        );
    }

    /// `max_segments` bounds every section: the first one on its own, and circles.
    #[test]
    fn max_segments_bounds_every_section() {
        assert!(refused(&match_loft_sections(&[at(0.0, gon(5, 10.0))], 3)));
        let quarters: Vec<Seg2> = (0..4u8)
            .map(|k| Seg2::Arc {
                center: p(0.0, 0.0),
                radius: 5.0,
                start: f64::from(k) * PI / 2.0,
                sweep: PI / 2.0,
            })
            .collect();
        assert!(refused(&match_loft_sections(
            &[at(0.0, quarters.clone()), at(10.0, quarters)],
            3
        )));
    }
}
