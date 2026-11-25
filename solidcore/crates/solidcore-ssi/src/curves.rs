//! `ssi`'s curves as solidcore's intersection curves.
//!
//! An `ssi` curve is marched points, each with both surfaces' parameters. It becomes a
//! 3D curve and a UV curve on each surface that share one parameter (chord length) and
//! one knot vector: cubic Hermite interpolants through the points, with continuous
//! tangents. The 3D tangent lies along the two normals' cross product where the
//! surfaces cross (from the neighboring points where they are tangent), and each UV
//! tangent is that tangent pulled back through its surface's Jacobian where the
//! Jacobian is well conditioned (from the neighboring points near a pole). So the trims
//! are the intersection's own parameter curves, not fits projected onto the surfaces
//! afterwards (upstream issue #489).
//!
//! A curve is split where it crosses a closed surface's seam, so each UV curve stays in
//! its domain; at a collapsed edge (a pole, an apex), where the coordinate along the
//! edge is taken from the curve's own approach on each side; and at corners (a region
//! boundary's). It passes through the crossings and meetings `ssi` reports, as the
//! corpus's branches do; those come back beside the curves
//! ([`crate::Intersection::vertices`]). A point is settled onto both surfaces on every
//! knot line a curve crosses, and spans are halved, each new point settled, until the
//! deviation measured inside them is within the tolerance. Touch points become
//! degenerate curves of type [`TsectCurveType::Touching`], as the port makes them. The
//! deviation is measured at the points and at three points in every span; it is not
//! yet a proved bound.

use solidcore::curve::curve::Curve;
use solidcore::geom::vector2d::Point2;
use solidcore::geom::vector3d::Vec3;
use solidcore::kernel::Kernel;
use solidcore::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use solidcore::nurbs::b_spline_curve::BSplineCurve;
use solidcore::nurbs::b_spline_surface_n_lib::p2::EvaluateFlags;
use solidcore::surface::surface::p2::{SurfaceIntersection, TsectCurveType};
use solidcore::surface::surface::Surface;
use ssi::{CurvePoint, Intersection, Patch};

use crate::Error;

/// Below this sine of the angle between the normals the surfaces are tangent at a
/// point: the 3D tangent comes from the neighboring points, not the normals.
const SIN_CROSSING: f64 = 1e-3;

/// A normals' tangent further than this from the neighboring points' direction (the
/// cosine of the angle between them) is not used.
const COS_AGREE: f64 = 0.866;

/// Below this ratio of the Jacobian's squared singular values (near a pole or a
/// collapsed edge) a UV tangent comes from the neighboring points, not the pull-back.
const WELL_CONDITIONED: f64 = 1e-6;

/// A turn sharper than this between two steps (radians) is a corner, where a curve is
/// split; `ssi`'s steps turn at most 0.06.
const CORNER: f64 = 0.5;

/// A point within this fraction of a period of a seam lies on it (and the same for an
/// edge or a knot line).
const ON_SEAM: f64 = 1e-9;

/// A span with an end within this fraction of its domain of an edge has its UV
/// tangents limited so its UV curve stays inside ([`limit_at_edges`]).
const NEAR_EDGE: f64 = 1e-2;

/// A step whose ends both lie within this fraction of the domain of a collapsed edge
/// is tested for passing through its pole ([`pole_between`]); the test itself is
/// whether the other surface meets the pole.
const POLE_REACH: f64 = 0.1;

/// A pole lies on a step when the path through it is at most this many times the
/// step's length.
const ON_STEP: f64 = 1.05;

/// A point within this fraction of its domain of a collapsed edge, whose pole is
/// within the tolerance of it, is moved onto the edge ([`snap_to_pole`]).
const ON_POLE: f64 = 1e-5;

/// A new node must lie on its step: the path through it at most this many times the
/// step's length.
const NEAR_STEP: f64 = 1.5;

/// A new node must lie further than this fraction of its step from both ends.
const INSIDE: f64 = 1e-3;

/// Newton steps when settling a point onto both surfaces.
const MAX_SETTLE: usize = 8;

/// Rounds of halving the spans whose measured deviation is over the tolerance.
const MAX_REFINE: usize = 6;

/// The parameters (s in a span) at which the deviation is measured.
const MEASURED_AT: [f64; 3] = [0.25, 0.5, 0.75];

/// A point of a curve: in space, and on each surface.
#[derive(Clone, Copy, Debug)]
struct Node {
    p: [f64; 3],
    uv: [[f64; 2]; 2],
}

impl Node {
    fn of(q: &CurvePoint) -> Self {
        Node {
            p: q.p,
            uv: [[q.a.u, q.a.v], [q.b.u, q.b.v]],
        }
    }
}

fn sub<const N: usize>(a: [f64; N], b: [f64; N]) -> [f64; N] {
    std::array::from_fn(|i| a.get(i).copied().unwrap_or(0.0) - b.get(i).copied().unwrap_or(0.0))
}

fn add<const N: usize>(a: [f64; N], b: [f64; N]) -> [f64; N] {
    std::array::from_fn(|i| a.get(i).copied().unwrap_or(0.0) + b.get(i).copied().unwrap_or(0.0))
}

fn scale<const N: usize>(a: [f64; N], s: f64) -> [f64; N] {
    a.map(|x| x * s)
}

fn dot<const N: usize>(a: [f64; N], b: [f64; N]) -> f64 {
    a.iter().zip(&b).map(|(x, y)| x * y).sum()
}

fn norm<const N: usize>(a: [f64; N]) -> f64 {
    dot(a, a).sqrt()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let ([ax, ay, az], [bx, by, bz]) = (a, b);
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

fn arr(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

fn coord(n: &Node, s: usize, d: usize) -> Option<f64> {
    n.uv.get(s)?.get(d).copied()
}

fn set(n: &mut Node, s: usize, d: usize, x: f64) {
    if let Some(c) = n.uv.get_mut(s).and_then(|uv| uv.get_mut(d)) {
        *c = x;
    }
}

/// `n` moved by `sign` times the periods in `shift`.
fn shifted(n: &Node, shift: &[[f64; 2]; 2], sign: f64) -> Node {
    Node {
        p: n.p,
        uv: [
            add(n.uv[0], scale(shift[0], sign)),
            add(n.uv[1], scale(shift[1], sign)),
        ],
    }
}

/// The node a fraction `f` of the way from `a` to `b`, in space and parameters.
fn lerp(a: &Node, b: &Node, f: f64) -> Node {
    Node {
        p: add(a.p, scale(sub(b.p, a.p), f)),
        uv: [
            add(a.uv[0], scale(sub(b.uv[0], a.uv[0]), f)),
            add(a.uv[1], scale(sub(b.uv[1], a.uv[1]), f)),
        ],
    }
}

/// What the curves need to know of the two patches' parameters: each domain, which
/// directions are closed, where the knot lines are, and which edges are collapsed.
struct Domains {
    /// Per surface: `[u0, u1, v0, v1]`.
    domain: [[f64; 4]; 2],
    /// Per surface: closed in u, closed in v.
    closed: [[bool; 2]; 2],
    /// Per surface, per direction: the interior knot lines, where a parameterization
    /// may lose smoothness (a rational circle's second derivative jumps at its double
    /// knots), so no span of a UV curve may straddle one.
    knots: [[Vec<f64>; 2]; 2],
    /// The collapsed edges (a sphere's pole, a cone's apex, a triangle's corner): the
    /// surface, the direction held fixed along the edge, and its value there. Along
    /// one, the other coordinate names no point.
    collapsed: Vec<(usize, usize, f64)>,
}

impl Domains {
    fn of(patches: [&Patch; 2]) -> Result<Self, Error> {
        let mut domain = [[0.0; 4]; 2];
        let mut knots: [[Vec<f64>; 2]; 2] = Default::default();
        let mut collapsed = Vec::new();
        for (s, p) in patches.iter().enumerate() {
            let dom = p.domain().ok_or(Error::Unsupported(
                "a patch whose knots are too few for its poles",
            ))?;
            let [u0, u1, v0, v1] = dom;
            let inside = |all: &[f64], lo: f64, hi: f64| {
                let eps = ON_SEAM * (hi - lo);
                let mut lines: Vec<f64> = all
                    .iter()
                    .copied()
                    .filter(|&k| k > lo + eps && k < hi - eps)
                    .collect();
                lines.dedup();
                lines
            };
            if let (Some(d), Some(k)) = (domain.get_mut(s), knots.get_mut(s)) {
                *d = dom;
                *k = [inside(&p.u_knots, u0, u1), inside(&p.v_knots, v0, v1)];
            }
            collapsed.extend(
                collapsed_edges(p, dom)
                    .into_iter()
                    .map(|(d, at)| (s, d, at)),
            );
        }
        Ok(Domains {
            domain,
            closed: patches.map(Patch::closed),
            knots,
            collapsed,
        })
    }

    /// The domain's edges `[lo, hi]` of surface `s` in direction `d`.
    fn edges(&self, s: usize, d: usize) -> Option<[f64; 2]> {
        let dom = self.domain.get(s)?;
        Some([*dom.get(2 * d)?, *dom.get(2 * d + 1)?])
    }

    /// Each (surface, direction) with its edges `[lo, hi]`, where the surface is closed.
    fn periodic(&self) -> impl Iterator<Item = (usize, usize, [f64; 2])> + '_ {
        (0..2).flat_map(move |s| {
            (0..2).filter_map(move |d| {
                let closed = self.closed.get(s)?.get(d).copied()?;
                closed.then_some((s, d, self.edges(s, d)?))
            })
        })
    }

    /// The (surface, direction along the edge) of each collapsed edge `n` lies on.
    fn poles_at<'a>(&'a self, n: &'a Node) -> impl Iterator<Item = (usize, usize)> + 'a {
        self.collapsed.iter().filter_map(move |&(s, d, at)| {
            let [lo, hi] = self.edges(s, d)?;
            ((coord(n, s, d)? - at).abs() <= ON_SEAM * (hi - lo)).then_some((s, 1 - d))
        })
    }

    fn on_pole(&self, n: &Node) -> bool {
        self.poles_at(n).next().is_some()
    }

    /// `n` with each closed coordinate moved by whole periods to lie nearest `toward`'s
    /// (it may leave the domain: a step's far end on a's side of a seam).
    fn unwrapped(&self, n: &Node, toward: &Node) -> Node {
        let mut m = *n;
        for (s, d, [lo, hi]) in self.periodic() {
            if let (Some(x), Some(t)) = (coord(n, s, d), coord(toward, s, d)) {
                let period = hi - lo;
                set(&mut m, s, d, x - ((x - t) / period).round() * period);
            }
        }
        m
    }

    /// `n` with each closed coordinate moved by whole periods into the domain, to the
    /// copy nearest `toward`'s where it lies on a seam (both edges are in the domain).
    fn folded(&self, n: &Node, toward: &Node) -> Node {
        let mut m = *n;
        for (s, d, [lo, hi]) in self.periodic() {
            let (Some(x), Some(t)) = (coord(n, s, d), coord(toward, s, d)) else {
                continue;
            };
            let period = hi - lo;
            let eps = ON_SEAM * period;
            let inside = |y: f64| y >= lo - eps && y <= hi + eps;
            let base = lo + (x - lo).rem_euclid(period);
            let best = [base - period, base, base + period]
                .into_iter()
                .filter(|&y| inside(y))
                .min_by(|p, q| (p - t).abs().total_cmp(&(q - t).abs()))
                .unwrap_or(base);
            set(&mut m, s, d, best.clamp(lo, hi));
        }
        m
    }

    /// How the step from `a` to `b` crosses a seam; None when it crosses none.
    fn crossing(&self, a: &Node, b: &Node) -> Option<Cross> {
        // the whole periods b lies from a in each closed direction; `near` is b moved
        // back by them, to a's side of every seam the step crosses
        let mut shift = [[0.0; 2]; 2];
        let mut crossed: Option<(usize, usize, [f64; 2])> = None;
        for (s, d, [lo, hi]) in self.periodic() {
            let period = hi - lo;
            let (x, y) = (coord(a, s, d)?, coord(b, s, d)?);
            let turns = ((y - x) / period).round();
            if turns != 0.0 {
                if let Some(c) = shift.get_mut(s).and_then(|r| r.get_mut(d)) {
                    *c = turns * period;
                }
                crossed.get_or_insert((s, d, [lo, hi]));
            }
        }
        let (s, d, [lo, hi]) = crossed?;
        let near = shifted(b, &shift, -1.0);
        let eps = ON_SEAM * (hi - lo);
        let (x, y) = (coord(a, s, d)?, coord(&near, s, d)?);
        let on = |t: f64| (t - lo).abs() <= eps || (t - hi).abs() <= eps;
        if on(y) {
            return Some(Cross::AtB(near));
        }
        if on(x) {
            return Some(Cross::AtA(shifted(a, &shift, 1.0)));
        }
        let edge = if y > hi { hi } else { lo };
        let mut guess = lerp(a, &near, ((edge - x) / (y - x)).clamp(0.0, 1.0));
        set(&mut guess, s, d, edge);
        Some(Cross::Between {
            guess,
            held: (s, d),
            shift,
        })
    }

    /// The knot lines crossed strictly between `a` and `b`: (fraction of the way,
    /// surface, direction, knot), in order along the step.
    fn knots_crossed(&self, a: &Node, b: &Node) -> Vec<(f64, usize, usize, f64)> {
        let mut hits = Vec::new();
        for (s, per_dir) in self.knots.iter().enumerate() {
            for (d, lines) in per_dir.iter().enumerate() {
                let (Some(x), Some(y), Some([lo, hi])) =
                    (coord(a, s, d), coord(b, s, d), self.edges(s, d))
                else {
                    continue;
                };
                let eps = ON_SEAM * (hi - lo);
                for &kn in lines {
                    if (kn - x) * (kn - y) < 0.0 && (kn - x).abs() > eps && (kn - y).abs() > eps {
                        hits.push(((kn - x) / (y - x), s, d, kn));
                    }
                }
            }
        }
        hits.sort_by(|p, q| p.0.total_cmp(&q.0));
        hits
    }

    /// The (surface, direction) whose coordinate changes most from `a` to `b`, as a
    /// fraction of its domain, leaving out a coordinate along a collapsed edge the step
    /// runs beside (holding it sends Newton to the pole).
    fn most_changed(&self, a: &Node, b: &Node) -> (usize, usize) {
        let beside_pole = |s: usize, along: usize| {
            self.collapsed.iter().any(|&(cs, d, at)| {
                cs == s
                    && 1 - d == along
                    && self.edges(s, d).is_some_and(|[lo, hi]| {
                        [a, b].iter().all(|n| {
                            coord(n, s, d).is_some_and(|x| (x - at).abs() <= NEAR_EDGE * (hi - lo))
                        })
                    })
            })
        };
        let mut best = ((0, 0), -1.0);
        for s in 0..2 {
            for d in 0..2 {
                if beside_pole(s, d) {
                    continue;
                }
                if let (Some(x), Some(y), Some([lo, hi])) =
                    (coord(a, s, d), coord(b, s, d), self.edges(s, d))
                {
                    let change = (y - x).abs() / (hi - lo);
                    if change > best.1 {
                        best = ((s, d), change);
                    }
                }
            }
        }
        best.0
    }
}

/// The edges of the patch whose poles all coincide: (the direction held fixed along
/// the edge, its value).
fn collapsed_edges(p: &Patch, [u0, u1, v0, v1]: [f64; 4]) -> Vec<(usize, f64)> {
    let mut lo = [f64::INFINITY; 3];
    let mut hi = [f64::NEG_INFINITY; 3];
    for pole in &p.poles {
        for ((l, h), &x) in lo.iter_mut().zip(&mut hi).zip(pole) {
            *l = l.min(x);
            *h = h.max(x);
        }
    }
    let tol = 1e-12 * norm(sub(hi, lo));
    let (nu, nv) = (p.u_count, p.v_count);
    let point = |i: usize, j: usize| p.poles.get(i * nv + j).copied();
    // whether every pole along an edge lies on its first
    let together = |edge: Vec<Option<[f64; 3]>>| match edge.first() {
        Some(&Some(f)) => edge
            .iter()
            .all(|q| q.is_some_and(|q| norm(sub(q, f)) <= tol)),
        _ => false,
    };
    let last_u = nu.saturating_sub(1);
    let last_v = nv.saturating_sub(1);
    let mut out = Vec::new();
    if together((0..nv).map(|j| point(0, j)).collect()) {
        out.push((0, u0));
    }
    if together((0..nv).map(|j| point(last_u, j)).collect()) {
        out.push((0, u1));
    }
    if together((0..nu).map(|i| point(i, 0)).collect()) {
        out.push((1, v0));
    }
    if together((0..nu).map(|i| point(i, last_v)).collect()) {
        out.push((1, v1));
    }
    out
}

/// How a step between two nodes crosses a seam.
enum Cross {
    /// `b` is on the seam: the run ends at this copy of b, on a's side.
    AtB(Node),
    /// `a` is on the seam: the next run starts at this copy of a, on b's side.
    AtA(Node),
    /// The seam lies between them: where the step meets it on a's side (a guess to
    /// settle, its coordinate `held` on the seam), and the periods to b's side.
    Between {
        guess: Node,
        held: (usize, usize),
        shift: [[f64; 2]; 2],
    },
}

/// A surface's point and first derivatives at `uv`.
fn jet(k: &Kernel, s: &Surface, uv: [f64; 2]) -> Result<[[f64; 3]; 3], Error> {
    let [u, v] = uv;
    let d = s.evaluate(k, Point2::new(u, v), 1, 1, EvaluateFlags::default())?;
    let at = |i: usize, j: usize| d.get(i).and_then(|r| r.get(j)).map(|&x| arr(x));
    match (at(0, 0), at(1, 0), at(0, 1)) {
        (Some(p), Some(su), Some(sv)) => Ok([p, su, sv]),
        _ => Err(Error::Unsupported(
            "a surface evaluation without first derivatives",
        )),
    }
}

/// Whether `m` is a usable new node between `a` and `b`: finite, not at either end (a
/// settle can run on to an end, or to a pole), and on the step, not on another branch
/// Newton ran to.
fn between(m: &Node, a: &Node, b: &Node) -> bool {
    let finite =
        m.p.iter()
            .chain(m.uv.iter().flatten())
            .all(|x| x.is_finite());
    let chord = norm(sub(b.p, a.p));
    let (to_a, to_b) = (norm(sub(m.p, a.p)), norm(sub(m.p, b.p)));
    finite && to_a > INSIDE * chord && to_b > INSIDE * chord && to_a + to_b <= NEAR_STEP * chord
}

/// The two surfaces, to evaluate and settle points on.
struct Pair<'a> {
    k: &'a Kernel,
    surfaces: [&'a Surface; 2],
    /// Per surface: `[u0, u1, v0, v1]`.
    domain: [[f64; 4]; 2],
}

impl Pair<'_> {
    /// Surface `s`'s point and first derivatives at `uv`.
    fn jet(&self, s: usize, uv: [f64; 2]) -> Result<[[f64; 3]; 3], Error> {
        let surface = self
            .surfaces
            .get(s)
            .ok_or(Error::Unsupported("a third surface"))?;
        jet(self.k, surface, uv)
    }

    /// Surface `s`'s point at `uv`.
    /// Surface `s`'s point at `uv` clamped to its domain (an interpolant a rounding
    /// past its edge is measured there, not refused).
    fn point(&self, s: usize, [u, v]: [f64; 2]) -> Result<[f64; 3], Error> {
        let surface = self
            .surfaces
            .get(s)
            .ok_or(Error::Unsupported("a third surface"))?;
        let [u0, u1, v0, v1] = self.domain.get(s).copied().unwrap_or([u, u, v, v]);
        let at = Point2::new(u.clamp(u0, u1), v.clamp(v0, v1));
        Ok(arr(surface.evaluate_point(self.k, at)?))
    }

    /// The intersection point near `guess` with `guess`'s coordinate `held` (surface,
    /// direction) kept: Newton on `S_s - S_o = 0` in the three other parameters, while
    /// the gap shrinks. The guess itself where no step can be solved (the surfaces
    /// tangent there) or a surface cannot be evaluated.
    fn settle(&self, guess: Node, (s, d): (usize, usize)) -> Node {
        let o = 1 - s;
        let mut best: Option<(f64, Node)> = None;
        let mut n = guess;
        for _ in 0..MAX_SETTLE {
            let (Some(&uv_s), Some(&uv_o)) = (n.uv.get(s), n.uv.get(o)) else {
                break;
            };
            let (Ok(js), Ok(jo)) = (self.jet(s, uv_s), self.jet(o, uv_o)) else {
                break;
            };
            let f = sub(js[0], jo[0]);
            let gap = norm(f);
            if best.as_ref().is_some_and(|(g, _)| gap >= *g) {
                break;
            }
            best = Some((
                gap,
                Node {
                    p: scale(add(js[0], jo[0]), 0.5),
                    uv: n.uv,
                },
            ));
            let free = if d == 0 { js[2] } else { js[1] };
            let (c0, c1, c2) = (free, scale(jo[1], -1.0), scale(jo[2], -1.0));
            let det = dot(c0, cross(c1, c2));
            if gap == 0.0 || !(det.abs() > 1e-14 * norm(c0) * norm(c1) * norm(c2)) {
                break;
            }
            let r = scale(f, -1.0);
            let step = [
                dot(r, cross(c1, c2)) / det,
                dot(c0, cross(r, c2)) / det,
                dot(c0, cross(c1, r)) / det,
            ];
            if let Some(c) = n.uv.get_mut(s).and_then(|uv| uv.get_mut(1 - d)) {
                *c += step[0];
            }
            if let Some(uv) = n.uv.get_mut(o) {
                uv[0] += step[1];
                uv[1] += step[2];
            }
        }
        best.map_or(guess, |(_, n)| n)
    }

    /// The run with a node settled on every knot line it crosses.
    fn at_knots(&self, domains: &Domains, run: Vec<Node>) -> Vec<Node> {
        let mut out: Vec<Node> = Vec::with_capacity(run.len());
        for b in run {
            if let Some(a) = out.last().copied() {
                for (f, s, d, kn) in domains.knots_crossed(&a, &b) {
                    let mut guess = lerp(&a, &b, f);
                    set(&mut guess, s, d, kn);
                    let m = self.settle(guess, (s, d));
                    if between(&m, &a, &b) {
                        out.push(m);
                    }
                }
            }
            out.push(b);
        }
        out
    }
}

/// Where the step from `a` to `b` crosses a seam: ends `run` on a's side, pushing it to
/// `out`, and starts the next run on b's side (`b` itself is the caller's to push).
fn seam_step(
    pair: &Pair<'_>,
    domains: &Domains,
    run: &mut Vec<Node>,
    out: &mut Vec<Vec<Node>>,
    a: &Node,
    b: &Node,
) {
    let Some(cross) = domains.crossing(a, b) else {
        return;
    };
    let (end, start) = match cross {
        Cross::AtB(end) => (Some(end), None),
        Cross::AtA(start) => (None, Some(start)),
        Cross::Between { guess, held, shift } => {
            // the settled point, unless Newton left the step for another branch
            let settled = pair.settle(guess, held);
            let far = shifted(b, &shift, -1.0);
            let on_step = norm(sub(a.p, settled.p)) + norm(sub(settled.p, far.p))
                <= NEAR_STEP * norm(sub(far.p, a.p));
            let end = if on_step { settled } else { guess };
            (Some(end), Some(shifted(&end, &shift, 1.0)))
        }
    };
    run.extend(end);
    out.push(std::mem::take(run));
    run.extend(start);
}

/// The curve's nodes in runs that each stay on one side of every seam and end at every
/// collapsed edge on the curve. A closed curve split somewhere is one run from a split
/// round to it; a closed curve split nowhere is one run ending where it starts. Runs
/// pass through the crossings and meetings `ssi` reports, as the corpus's branches do.
fn runs(pair: &Pair<'_>, curve: &ssi::Curve, domains: &Domains, tol: f64) -> Vec<Vec<Node>> {
    // an open curve ends on a boundary, so an end within the tolerance of a pole ends at
    // it; a point inside a curve may pass that close to a pole without meeting it (a
    // loop 1e-7 about a sphere's pole, ssi-corpus c7_tiny_loop_sphere_1e-7)
    let last = curve.points.len().saturating_sub(1);
    let mut nodes: Vec<Node> = curve
        .points
        .iter()
        .enumerate()
        .map(|(i, q)| {
            let n = Node::of(q);
            if !curve.closed && (i == 0 || i == last) {
                snap_to_pole(pair, domains, n, tol)
            } else {
                n
            }
        })
        .collect();
    if curve.closed {
        nodes.extend(nodes.first().copied());
    }
    let ends_here = |n: &Node| domains.on_pole(n);
    let mut out: Vec<Vec<Node>> = Vec::new();
    let mut run: Vec<Node> = Vec::new();
    for node in nodes {
        if let Some(prev) = run.last().copied() {
            // a pole first: a step through one swings the coordinate along its edge by
            // any amount (half a period through a sphere's pole), which a seam test
            // would read as a crossing; each half of the step may still cross a seam of
            // either surface (a pole on the other surface's seam)
            if let Some((end, start)) = pole_between(pair, domains, &prev, &node, tol) {
                seam_step(pair, domains, &mut run, &mut out, &prev, &end);
                run.push(end);
                out.push(std::mem::take(&mut run));
                run.push(start);
                seam_step(pair, domains, &mut run, &mut out, &start, &node);
            } else {
                seam_step(pair, domains, &mut run, &mut out, &prev, &node);
            }
        }
        let split = !run.is_empty() && ends_here(&node);
        run.push(node);
        if split {
            out.push(std::mem::take(&mut run));
            run.push(node);
        }
    }
    out.push(run);
    // a closed curve's last run runs on into its first, unless it starts at a split
    if curve.closed && out.len() > 1 {
        let starts_split = curve
            .points
            .first()
            .map(Node::of)
            .is_some_and(|n| ends_here(&n));
        if !starts_split {
            if let Some(mut last) = out.pop() {
                if let Some(first) = out.first_mut() {
                    last.extend(first.get(1..).unwrap_or_default());
                    *first = last;
                }
            }
        }
    }
    out
}

/// `n` onto a collapsed edge it lies within [`ON_POLE`] of, where the edge's point (the
/// pole) is within `tol` of it: inverting a point near a pole finds the coordinate
/// across the edge only to about the square root of the rounding.
fn snap_to_pole(pair: &Pair<'_>, domains: &Domains, n: Node, tol: f64) -> Node {
    let mut n = n;
    for &(s, d, at) in &domains.collapsed {
        let (Some(x), Some([lo, hi]), Some(&uv)) =
            (coord(&n, s, d), domains.edges(s, d), n.uv.get(s))
        else {
            continue;
        };
        if x == at || (x - at).abs() > ON_POLE * (hi - lo) {
            continue;
        }
        let mut on = n;
        set(&mut on, s, d, at);
        let Some(&uv_on) = on.uv.get(s) else {
            continue;
        };
        if let (Ok(pole), Ok(here)) = (pair.point(s, uv_on), pair.point(s, uv)) {
            if norm(sub(pole, here)) <= tol && norm(sub(pole, n.p)) <= tol {
                n = on;
            }
        }
    }
    n
}

/// Where the step from `a` to `b` passes through the point of a collapsed edge (a
/// pole): on one surface the step runs beside the edge while the coordinate along it
/// swings by more than a quarter of its domain. The pole's node as the end of a run on
/// a's side and the start of one on b's side (the coordinate along the edge taken from
/// a and from b), the other surface's parameters solved at the pole; None when the
/// step crosses no pole, or passes beside it (the other surface misses the pole by
/// more than `tol`).
fn pole_between(
    pair: &Pair<'_>,
    domains: &Domains,
    a: &Node,
    b: &Node,
    tol: f64,
) -> Option<(Node, Node)> {
    for &(s, d, at) in &domains.collapsed {
        let along = 1 - d;
        let (Some([lo, hi]), Some([alo, ahi])) = (domains.edges(s, d), domains.edges(s, along))
        else {
            continue;
        };
        let near =
            |n: &Node| coord(n, s, d).is_some_and(|x| (x - at).abs() <= POLE_REACH * (hi - lo));
        let swing = match (coord(a, s, along), coord(b, s, along)) {
            (Some(x), Some(y)) => (y - x).abs(),
            _ => continue,
        };
        if !(near(a) && near(b) && swing > 0.25 * (ahi - alo)) {
            continue;
        }
        // the pole, and the other surface's parameters there by Gauss-Newton from the
        // point of the step nearest the edge
        let o = 1 - s;
        let (ea, eb) = (coord(a, s, d)? - at, coord(b, s, d)? - at);
        let f = (ea.abs() / (ea.abs() + eb.abs())).clamp(0.0, 1.0);
        let mut on_edge = *a;
        set(&mut on_edge, s, d, at);
        let pole = pair.point(s, *on_edge.uv.get(s)?).ok()?;
        // the pole must lie on the step, not merely on both surfaces
        if !(norm(sub(a.p, pole)) + norm(sub(pole, b.p)) <= ON_STEP * norm(sub(b.p, a.p))) {
            continue;
        }
        // the guess from b unwrapped to a's side of any seam the step crosses, and the
        // iterate kept in the domain
        let near_b = domains.unwrapped(b, a);
        let mut guess = lerp(a, &near_b, f);
        let mut gap = f64::INFINITY;
        for _ in 0..MAX_SETTLE {
            guess = domains.folded(&guess, a);
            let uv_o = *guess.uv.get(o)?;
            let j = pair.jet(o, uv_o).ok()?;
            gap = norm(sub(pole, j[0]));
            let Some(step) = pull_back(j[1], j[2], sub(pole, j[0])) else {
                break;
            };
            if let Some(q) = guess.uv.get_mut(o) {
                *q = add(uv_o, step);
            }
        }
        guess = domains.folded(&guess, a);
        let uv_o = *guess.uv.get(o)?;
        let gap = gap.min(norm(sub(pole, pair.point(o, uv_o).ok()?)));
        if !(gap <= tol) {
            return None;
        }
        // on a's side: the coordinate along the edge from a, a closed coordinate of the
        // other surface on a seam taken on a's side of it; on b's side the same from b
        let mut pole_node = Node { p: pole, uv: a.uv };
        set(&mut pole_node, s, d, at);
        if let Some(q) = pole_node.uv.get_mut(o) {
            *q = uv_o;
        }
        let end = domains.folded(&pole_node, a);
        let mut start = domains.folded(&pole_node, b);
        set(&mut start, s, along, coord(b, s, along)?);
        return Some((end, start));
    }
    None
}

/// The run with the coordinate along a collapsed edge, at an end on one, taken from
/// the neighboring node (the curve's own approach to the pole).
fn at_poles(domains: &Domains, mut run: Vec<Node>) -> Vec<Node> {
    let n = run.len();
    for (end, next, after) in [
        (0, 1, 2),
        (
            n.saturating_sub(1),
            n.saturating_sub(2),
            n.saturating_sub(3),
        ),
    ] {
        let (Some(&e), Some(&x)) = (run.get(end), run.get(next)) else {
            continue;
        };
        if end == next {
            continue;
        }
        let y = run
            .get(after)
            .filter(|_| after != next && after != end)
            .copied();
        let along: Vec<(usize, usize)> = domains.poles_at(&e).collect();
        if let Some(node) = run.get_mut(end) {
            for (s, d) in along {
                if let Some(c) = toward_pole(&e, &x, y.as_ref(), s, d) {
                    set(node, s, d, c);
                }
            }
        }
    }
    run
}

/// The coordinate `d` of surface `s` at the pole `e`, from the nodes `x` (next to it)
/// and `y` (next to `x`): extrapolated linearly in the distance from the pole, else
/// `x`'s.
fn toward_pole(e: &Node, x: &Node, y: Option<&Node>, s: usize, d: usize) -> Option<f64> {
    let cx = coord(x, s, d)?;
    let Some(y) = y else {
        return Some(cx);
    };
    let cy = coord(y, s, d)?;
    let (rx, ry) = (norm(sub(x.p, e.p)), norm(sub(y.p, e.p)));
    if !(ry > rx && rx > 0.0) {
        return Some(cx);
    }
    Some(cx - (cy - cx) * rx / (ry - rx))
}

/// The run without nodes that repeat the one before (a zero-length chord).
fn dedup(run: Vec<Node>, link: f64) -> Vec<Node> {
    let tiny = link * 1e-6;
    let mut out: Vec<Node> = Vec::with_capacity(run.len());
    for n in run {
        if out
            .last()
            .is_some_and(|m: &Node| norm(sub(m.p, n.p)) <= tiny)
        {
            continue;
        }
        out.push(n);
    }
    out
}

/// The run split at its corners: nodes where the curve turns by more than [`CORNER`].
fn at_corners(run: &[Node]) -> Vec<Vec<Node>> {
    let cos = CORNER.cos();
    let mut out = Vec::new();
    let mut piece: Vec<Node> = Vec::new();
    for (i, &node) in run.iter().enumerate() {
        piece.push(node);
        let (Some(a), Some(b)) = (i.checked_sub(1).and_then(|j| run.get(j)), run.get(i + 1)) else {
            continue;
        };
        let (into, out_of) = (sub(node.p, a.p), sub(b.p, node.p));
        if dot(into, out_of) < cos * norm(into) * norm(out_of) {
            out.push(std::mem::take(&mut piece));
            piece.push(node);
        }
    }
    out.push(piece);
    out
}

/// Derivatives at nodes from the neighboring points: Bessel's (the derivative of the
/// parabola through three points), one-sided at the ends of an open run, wrapped
/// round on a periodic one.
fn bessel<const N: usize>(x: &[[f64; N]], t: &[f64], periodic: bool) -> Vec<[f64; N]> {
    let n = x.len();
    let chord = |i: usize| -> Option<([f64; N], f64)> {
        let h = t.get(i + 1)? - t.get(i)?;
        Some((scale(sub(*x.get(i + 1)?, *x.get(i)?), 1.0 / h), h))
    };
    let mid = |(d0, h0): ([f64; N], f64), (d1, h1): ([f64; N], f64)| {
        scale(add(scale(d0, h1), scale(d1, h0)), 1.0 / (h0 + h1))
    };
    // at the near end of chord 0: d0 (2 h0 + h1) / (h0 + h1) - d1 h0 / (h0 + h1)
    let end = |(d0, h0): ([f64; N], f64), (d1, h1): ([f64; N], f64)| {
        scale(
            sub(scale(d0, 2.0 * h0 + h1), scale(d1, h0)),
            1.0 / (h0 + h1),
        )
    };
    (0..n)
        .map(|i| {
            let before = i.checked_sub(1).and_then(chord);
            let after = chord(i);
            let wrap_before = if periodic && i == 0 {
                n.checked_sub(2).and_then(chord)
            } else {
                None
            };
            let wrap_after = if periodic && i + 1 == n {
                chord(0)
            } else {
                None
            };
            match (before.or(wrap_before), after.or(wrap_after)) {
                (Some(b), Some(a)) => mid(b, a),
                (None, Some(a)) => chord(i + 1).map_or(a.0, |a2| end(a, a2)),
                // mirrored: the far end of the last chord
                (Some(b), None) => i
                    .checked_sub(2)
                    .and_then(chord)
                    .map_or(b.0, |b2| end(b, b2)),
                (None, None) => [0.0; N],
            }
        })
        .collect()
}

/// `t` pulled back through the Jacobian `[su sv]` (least squares); None where the
/// Jacobian is badly conditioned (near a pole or a collapsed edge).
fn pull_back(su: [f64; 3], sv: [f64; 3], t: [f64; 3]) -> Option<[f64; 2]> {
    let (g00, g01, g11) = (dot(su, su), dot(su, sv), dot(sv, sv));
    let det = g00 * g11 - g01 * g01;
    // the ratio of the metric's eigenvalues: det / (largest eigenvalue)^2
    let half = 0.5 * (g00 + g11);
    let largest = half + (half * half - det).max(0.0).sqrt();
    if !(det > WELL_CONDITIONED * largest * largest) {
        return None;
    }
    let (b0, b1) = (dot(su, t), dot(sv, t));
    Some([(g11 * b0 - g01 * b1) / det, (g00 * b1 - g01 * b0) / det])
}

/// The UV tangents `d` at `points` (surface `s`, parameters `t`) limited, in every span
/// with an end near an edge of the domain or on a collapsed edge (`pinned`), so that
/// each coordinate's interpolant is
/// monotone where its data are (Fritsch and Carlson's condition): a UV curve then never
/// passes the edge of its domain beside a pole or a boundary.
fn limit_at_edges(
    domains: &Domains,
    s: usize,
    t: &[f64],
    points: &[[f64; 2]],
    pinned: &[bool],
    d: &mut [[f64; 2]],
) {
    for dir in 0..2 {
        let Some([lo, hi]) = domains.edges(s, dir) else {
            continue;
        };
        let near = |x: f64| (x - lo).abs().min((hi - x).abs()) <= NEAR_EDGE * (hi - lo);
        for i in 0..points.len().saturating_sub(1) {
            let (Some(&p0), Some(&p1), Some(&t0), Some(&t1)) =
                (points.get(i), points.get(i + 1), t.get(i), t.get(i + 1))
            else {
                continue;
            };
            let (Some(&x0), Some(&x1)) = (p0.get(dir), p1.get(dir)) else {
                continue;
            };
            let at_pole = pinned.get(i).copied().unwrap_or(false)
                || pinned.get(i + 1).copied().unwrap_or(false);
            if !(near(x0) || near(x1) || at_pole) {
                continue;
            }
            let (Some(d0), Some(d1)) = (
                d.get(i).and_then(|q| q.get(dir).copied()),
                d.get(i + 1).and_then(|q| q.get(dir).copied()),
            ) else {
                continue;
            };
            let slope = (x1 - x0) / (t1 - t0);
            let (mut e0, mut e1) = (d0, d1);
            if slope == 0.0 {
                (e0, e1) = (0.0, 0.0);
            } else {
                let (a, b) = (e0 / slope, e1 / slope);
                if a < 0.0 {
                    e0 = 0.0;
                }
                if b < 0.0 {
                    e1 = 0.0;
                }
                let (a, b) = (e0 / slope, e1 / slope);
                let r = a * a + b * b;
                if r > 9.0 {
                    let tau = 3.0 / r.sqrt();
                    (e0, e1) = (tau * a * slope, tau * b * slope);
                }
            }
            if let Some(c) = d.get_mut(i).and_then(|q| q.get_mut(dir)) {
                *c = e0;
            }
            if let Some(c) = d.get_mut(i + 1).and_then(|q| q.get_mut(dir)) {
                *c = e1;
            }
        }
    }
}

/// The cubic Hermite interpolant on one span at `s` in [0, 1].
fn hermite<const N: usize>(
    p0: [f64; N],
    d0: [f64; N],
    p1: [f64; N],
    d1: [f64; N],
    h: f64,
    s: f64,
) -> [f64; N] {
    let (s2, s3) = (s * s, s * s * s);
    let (h00, h10, h01, h11) = (
        2.0 * s3 - 3.0 * s2 + 1.0,
        s3 - 2.0 * s2 + s,
        3.0 * s2 - 2.0 * s3,
        s3 - s2,
    );
    add(
        add(scale(p0, h00), scale(d0, h10 * h)),
        add(scale(p1, h01), scale(d1, h11 * h)),
    )
}

/// The interpolants of one run: the nodes' parameters, points and tangents.
struct Fit {
    /// The parameter at each node (chord length).
    t: Vec<f64>,
    /// The 3D points and tangents.
    x: Vec<[f64; 3]>,
    dx: Vec<[f64; 3]>,
    /// Each surface's UV points and tangents.
    uv: [Vec<[f64; 2]>; 2],
    duv: [Vec<[f64; 2]>; 2],
    /// Whether the normals are parallel at every node.
    tangent: bool,
    /// The largest gap between a node and either surface at its parameters.
    at_nodes: f64,
}

impl Fit {
    fn new(
        pair: &Pair<'_>,
        domains: &Domains,
        run: &[Node],
        periodic: bool,
    ) -> Result<Self, Error> {
        let mut t = Vec::with_capacity(run.len());
        let mut length = 0.0;
        let mut prev: Option<[f64; 3]> = None;
        for n in run {
            if let Some(q) = prev {
                length += norm(sub(n.p, q));
            }
            t.push(length);
            prev = Some(n.p);
        }
        let x: Vec<[f64; 3]> = run.iter().map(|n| n.p).collect();
        let uv: [Vec<[f64; 2]>; 2] = [
            run.iter().map(|n| n.uv[0]).collect(),
            run.iter().map(|n| n.uv[1]).collect(),
        ];
        let chord_x = bessel(&x, &t, periodic);
        let chord_uv = [bessel(&uv[0], &t, periodic), bessel(&uv[1], &t, periodic)];

        let mut dx = Vec::with_capacity(run.len());
        let mut duv: [Vec<[f64; 2]>; 2] = Default::default();
        let mut tangent = true;
        let mut at_nodes: f64 = 0.0;
        for (i, n) in run.iter().enumerate() {
            let jets = [pair.jet(0, n.uv[0])?, pair.jet(1, n.uv[1])?];
            for j in &jets {
                at_nodes = at_nodes.max(norm(sub(n.p, j[0])));
            }
            let normal = |j: &[[f64; 3]; 3]| {
                let c = cross(j[1], j[2]);
                let l = norm(c);
                (l > 0.0).then(|| scale(c, 1.0 / l))
            };
            let chord = chord_x.get(i).copied().unwrap_or([0.0; 3]);
            let speed = norm(chord);
            let along = match (normal(&jets[0]), normal(&jets[1])) {
                (Some(na), Some(nb)) => {
                    let c = cross(na, nb);
                    let sin = norm(c);
                    if sin > SIN_CROSSING {
                        tangent = false;
                        let dir = scale(c, 1.0 / sin);
                        let dir = if dot(dir, chord) < 0.0 {
                            scale(dir, -1.0)
                        } else {
                            dir
                        };
                        (speed > 0.0 && dot(dir, chord) >= COS_AGREE * speed)
                            .then(|| scale(dir, speed))
                    } else {
                        None
                    }
                }
                _ => None,
            };
            let d = along.unwrap_or(chord);
            let d = if d.iter().all(|x| x.is_finite()) {
                d
            } else {
                [0.0; 3]
            };
            dx.push(d);
            for (s, j) in jets.iter().enumerate() {
                let fallback = chord_uv
                    .get(s)
                    .and_then(|c| c.get(i))
                    .copied()
                    .unwrap_or([0.0; 2]);
                let q = pull_back(j[1], j[2], d).unwrap_or(fallback);
                let q = if q.iter().all(|x| x.is_finite()) {
                    q
                } else {
                    [0.0; 2]
                };
                if let Some(list) = duv.get_mut(s) {
                    list.push(q);
                }
            }
        }
        let pinned: Vec<bool> = run.iter().map(|n| domains.on_pole(n)).collect();
        for (s, list) in duv.iter_mut().enumerate() {
            if let Some(points) = uv.get(s) {
                limit_at_edges(domains, s, &t, points, &pinned, list);
            }
        }
        Ok(Fit {
            t,
            x,
            dx,
            uv,
            duv,
            tangent,
            at_nodes,
        })
    }

    /// Span `i`'s width, and its 3D and UV interpolants at `s` in [0, 1].
    fn at(&self, i: usize, s: f64) -> Option<([f64; 3], [[f64; 2]; 2])> {
        let h = self.t.get(i + 1)? - self.t.get(i)?;
        let p = hermite(
            *self.x.get(i)?,
            *self.dx.get(i)?,
            *self.x.get(i + 1)?,
            *self.dx.get(i + 1)?,
            h,
            s,
        );
        let mut uv = [[0.0; 2]; 2];
        for (k, out) in uv.iter_mut().enumerate() {
            let (q, d) = (self.uv.get(k)?, self.duv.get(k)?);
            *out = hermite(*q.get(i)?, *d.get(i)?, *q.get(i + 1)?, *d.get(i + 1)?, h, s);
        }
        Some((p, uv))
    }

    /// The largest gap inside span `i`, at [`MEASURED_AT`], between the 3D curve and
    /// either surface at its UV curve.
    fn span_gap(&self, pair: &Pair<'_>, i: usize) -> Result<f64, Error> {
        let mut gap: f64 = 0.0;
        for s in MEASURED_AT {
            let Some((p, uv)) = self.at(i, s) else {
                continue;
            };
            for (k, &q) in uv.iter().enumerate() {
                gap = gap.max(norm(sub(p, pair.point(k, q)?)));
            }
        }
        Ok(gap)
    }

    /// The node in the middle of span `i`, settled onto both surfaces with the
    /// coordinate that changes most across the span held.
    fn middle(&self, pair: &Pair<'_>, domains: &Domains, run: &[Node], i: usize) -> Option<Node> {
        let (a, b) = (run.get(i)?, run.get(i + 1)?);
        let (p, uv) = self.at(i, 0.5)?;
        Some(pair.settle(Node { p, uv }, domains.most_changed(a, b))).filter(|m| between(m, a, b))
    }

    /// The largest gap measured anywhere on the run.
    fn deviation(&self, pair: &Pair<'_>) -> Result<f64, Error> {
        let mut gap = self.at_nodes;
        for i in 0..self.t.len().saturating_sub(1) {
            gap = gap.max(self.span_gap(pair, i)?);
        }
        Ok(gap)
    }
}

/// The C1 cubic B-spline of a Hermite interpolant: knots at the nodes, each interior
/// knot twice (the curve's Bézier segments share their tangents, so the third copy
/// comes out exactly).
fn bspline(dim: u64, x: &[[f64; 3]], d: &[[f64; 3]], t: &[f64]) -> Result<Curve, Error> {
    let mut control: Vec<Vec3> = Vec::with_capacity(2 * x.len());
    let point = |p: [f64; 3]| Vec3::new(p[0], p[1], p[2]);
    let last = t.len().saturating_sub(1);
    for (i, ((&p, &dp), &ti)) in x.iter().zip(d).zip(t).enumerate() {
        let before = i.checked_sub(1).and_then(|j| t.get(j)).map(|&tj| ti - tj);
        let after = t.get(i + 1).map(|&tj| tj - ti);
        if i == 0 {
            control.push(point(p));
        }
        if let Some(h) = before {
            control.push(point(sub(p, scale(dp, h / 3.0))));
        }
        if let Some(h) = after {
            control.push(point(add(p, scale(dp, h / 3.0))));
        }
        if i == last {
            control.push(point(p));
        }
    }
    let mut multiplicities = vec![2usize; t.len()];
    if let Some(m) = multiplicities.first_mut() {
        *m = 4;
    }
    if let Some(m) = multiplicities.last_mut() {
        *m = 4;
    }
    let curve = BSplineCurve::create_canonical(&CanonicalData {
        dim,
        degree: 3,
        control_points: &control,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &multiplicities,
        knots: t,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })?;
    Ok(Curve::from(curve))
}

fn lift(uv: [f64; 2]) -> [f64; 3] {
    [uv[0], uv[1], 0.0]
}

/// One run as a 3D curve, its two UV curves, its type and its measured deviation, the
/// spans over `tol` halved (each new node settled) for up to [`MAX_REFINE`] rounds.
fn build_run(
    pair: &Pair<'_>,
    domains: &Domains,
    run: Vec<Node>,
    periodic: bool,
    tol: f64,
    out: &mut SurfaceIntersection,
) -> Result<(), Error> {
    let mut run = run;
    let mut fit = Fit::new(pair, domains, &run, periodic)?;
    for _ in 0..MAX_REFINE {
        let mut over = Vec::new();
        for i in 0..run.len().saturating_sub(1) {
            if fit.span_gap(pair, i)? > tol {
                over.push(i);
            }
        }
        if over.is_empty() {
            break;
        }
        // from the back, so earlier spans keep their indices
        for &i in over.iter().rev() {
            if let Some(m) = fit.middle(pair, domains, &run, i) {
                run.insert(i + 1, m);
            }
        }
        // the new nodes next to a pole at an end place its coordinate along the edge
        run = at_poles(domains, run);
        fit = Fit::new(pair, domains, &run, periodic)?;
    }
    let deviation = fit.deviation(pair)?;
    out.curves_3d.push(bspline(3, &fit.x, &fit.dx, &fit.t)?);
    for (s, list) in [&mut out.surface1_uv_curves, &mut out.surface2_uv_curves]
        .into_iter()
        .enumerate()
    {
        let (Some(p), Some(d)) = (fit.uv.get(s), fit.duv.get(s)) else {
            list.push(None);
            continue;
        };
        let p3: Vec<[f64; 3]> = p.iter().map(|&q| lift(q)).collect();
        let d3: Vec<[f64; 3]> = d.iter().map(|&q| lift(q)).collect();
        list.push(Some(bspline(2, &p3, &d3, &fit.t)?));
    }
    out.curve_types.push(if fit.tangent {
        TsectCurveType::Tangent
    } else {
        TsectCurveType::Crossing
    });
    out.deviations.push(deviation);
    Ok(())
}

/// A touch point as the port marks one: degenerate curves of type
/// [`TsectCurveType::Touching`], the deviation the two surfaces' points apart.
fn touch(pair: &Pair<'_>, q: &CurvePoint, out: &mut SurfaceIntersection) -> Result<(), Error> {
    let n = Node::of(q);
    let (a, b) = (pair.point(0, n.uv[0])?, pair.point(1, n.uv[1])?);
    let [x, y, z] = q.p;
    out.curves_3d
        .push(Curve::from(BSplineCurve::create_degenerate_curve(
            3,
            Vec3::new(x, y, z),
        )?));
    for (s, list) in [&mut out.surface1_uv_curves, &mut out.surface2_uv_curves]
        .into_iter()
        .enumerate()
    {
        let [u, v] = n.uv.get(s).copied().unwrap_or([f64::NAN; 2]);
        list.push(Some(Curve::from(BSplineCurve::create_degenerate_curve(
            2,
            Vec3::new(u, v, 0.0),
        )?)));
    }
    out.curve_types.push(TsectCurveType::Touching);
    out.deviations.push(norm(sub(a, b)));
    Ok(())
}

/// The intersection `ssi` found between `patches` (made from `surfaces` by
/// [`crate::patch::patch`]) as solidcore's intersection curves. `link` is the distance
/// (absolute) within which two points are one, and `tol` the deviation the spans are
/// refined to.
///
/// # Errors
///
/// [`Error::Kernel`] when a surface cannot be evaluated at a curve point or a curve
/// cannot be made.
pub fn build(
    k: &Kernel,
    surfaces: [&Surface; 2],
    patches: [&Patch; 2],
    found: &Intersection,
    link: f64,
    tol: f64,
) -> Result<SurfaceIntersection, Error> {
    let domains = Domains::of(patches)?;
    let pair = Pair {
        k,
        surfaces,
        domain: domains.domain,
    };
    let mut out = SurfaceIntersection::default();
    for curve in &found.curves {
        let pieces: Vec<Vec<Node>> = runs(&pair, curve, &domains, tol)
            .into_iter()
            .map(|run| dedup(pair.at_knots(&domains, at_poles(&domains, run)), link))
            .flat_map(|run| at_corners(&run))
            .filter(|run| run.len() > 1)
            .collect();
        let periodic = curve.closed
            && pieces.len() == 1
            && pieces.first().is_some_and(|r| match (r.first(), r.last()) {
                (Some(a), Some(b)) => a.uv == b.uv,
                _ => false,
            });
        for run in pieces {
            build_run(&pair, &domains, run, periodic, tol, &mut out)?;
        }
    }
    for q in &found.points {
        touch(&pair, q, &mut out)?;
    }
    Ok(out)
}
