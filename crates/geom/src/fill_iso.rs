//! Filling a closed loop of exact B-spline curves with tensor B-spline patches whose boundary
//! rows *are* those curves, so that every edge lies on the fill exactly and its pcurve is a
//! straight line in (u, v), affine in the edge's own parameter.
//!
//! The formulation:
//!
//! - **The patches.** A loop of four edges `e₀ … e₃` bounds one patch on the unit square: `e₀`
//!   runs along `v = 0` from `u = 0` to 1, `e₁` up `u = 1`, `e₂` back along `v = 1` and `e₃` down
//!   `u = 0`. A loop of three has no `e₃`: the side `u = 0` collapses to the loop's first corner
//!   (a pole, as a three-sided tensor patch must have). A loop of `N ≥ 5` is `N` four-sided
//!   patches about a centre point: each edge is split at the middle of its parameter interval,
//!   a spoke runs from each edge's midpoint `M_k` to the centre `C`, and patch `k` has the
//!   corners `V_k, M_k, C, M_{k−1}` (`V_k` the corner where edge `k` starts): the first half of
//!   edge `k` along `v = 0`, spoke `k` up `u = 1`, spoke `k − 1` back along `v = 1` and the
//!   second half of edge `k − 1` down `u = 0`. Each spoke is one row of the two patches it
//!   divides, the same control points in both.
//! - **One space.** Every side, each on `[0, 1]` by an affine map of its parameter (which changes
//!   neither the curve nor its pieces), is made compatible with every other exactly: split into
//!   Bézier pieces at every break of any, each piece raised to the highest degree (NURBS Book
//!   §5.5), joined again with each break repeated as often as that degree
//!   ([`crate::spline_space::embed`]). Every patch is a net in that space both ways.
//! - **The start**: each patch is the bilinearly blended Coons patch of its sides (S. A. Coons,
//!   1967), built on the Greville abscissae (a linear function is reproduced exactly there), the
//!   spokes starting as straight lines to the centre, the mean of the midpoints.
//! - **The inside**: with the loop's rows held, the other control points (each patch's interior,
//!   the spokes' and the centre, each once) minimise, by least squares, the misfit to the
//!   conditions plus a vanishing multiple of the patches' thin-plate energy
//!   `∫∫ |S_uu|² + 2|S_uv|² + |S_vv|²` (which alone decides a G0 fill):
//!   - G1 at stations along an edge: `n·∂S/∂ν = 0` (`ν` across the edge into the fill, `n` the
//!     neighbour's normal), with weak rows `∂S/∂ν = s·w` (`w` the neighbour's leave direction,
//!     `s` the side's own speed) that keep the fill leaving into itself;
//!   - G2: `n·∂²S/∂ν² = (∂S/∂ν·w)²·κ`, `κ` the neighbour's normal curvature along `w`, the speed
//!     read from the last solution;
//!   - across each spoke, the two patches' derivatives away from it opposed (`∂S_k/∂u + ∂S_{k+1}/∂v
//!     = 0` on the shared parameter): the patches join tangent there;
//!   - points the fill passes through, located on the last solution (nearest point of any patch)
//!     and fitted again (Hoschek's parameter correction).
//!
//!   At a corner where the neighbours' tangent planes disagree no surface is G1 to both, and at
//!   the centre the spokes' tangency conditions overdetermine the patches: the stations near
//!   either are fitted but not held to the angle tolerance.
//! - **Refinement**: while the normals miss the neighbours' (or each other's across a spoke) by
//!   more than the angle tolerance, a point is missed, or a patch folds, every span is halved
//!   (knot insertion carries the sides into the finer space exactly), up to [`MAX_ISO_SPANS`].
//! - **A crown** fills the middle out along the normals, on control points clear of the rows the
//!   conditions hold, so the loop and the conditions across it do not move.
//!
//! The fill's boundary is exact, whatever its inside: a station's gap is nought by construction.
//! Rational edges are not supported (the Coons sum needs one weight function for all sides).
//! Input is hostile: sizes are capped and every evaluation checked; nothing here panics.

use crate::Vec3;
use crate::bspline::{HPoint, MAX_SPLINE_DEGREE, NurbsSurface, SplineError, SplineResult, check_knots, invalid, segment};
use crate::fill_general::Across;
use crate::spline_space::{Space, embed};

/// Most spans of the fill's space.
pub const MAX_ISO_SPANS: usize = 64;
/// Most edges of a loop.
pub const MAX_ISO_EDGES: usize = 16;
/// Most free control points one least-squares solve takes (its system is dense).
const MAX_UNKNOWN_POINTS: usize = 500;
/// Stations per span of a side where conditions are asked and checked.
const STATIONS_PER_SPAN: usize = 6;
/// Fewest stations along a side.
const MIN_STATIONS: usize = 12;
/// Weight of the weak Hermite rows against the G1 rows.
const HERMITE_WEIGHT: f64 = 0.05;
/// Rounds of relinearising G2 and relocating the points.
const ROUNDS: usize = 3;
/// The share of a side, from a clashing corner or the centre, whose stations are not held to
/// the angle tolerance.
const CLASH_SHARE: f64 = 0.15;
/// How many points each way the fold check samples.
const FOLD_SAMPLES: usize = 24;
/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]` (exact to degree 7).
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];

/// One edge of the loop: a non-rational B-spline from the loop's corner before it to the one
/// after (the loop's direction).
#[derive(Clone, Debug, PartialEq)]
pub struct IsoEdge {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub points: Vec<Vec3>,
}

impl IsoEdge {
    /// The edge's parameter interval.
    pub fn domain(&self) -> [f64; 2] {
        [self.knots.get(self.degree).copied().unwrap_or(0.0), self.knots.get(self.points.len()).copied().unwrap_or(0.0)]
    }

    /// The parameter a loop of five or more edges splits the edge at: its interval's middle.
    pub fn middle(&self) -> f64 {
        let [a, b] = self.domain();
        0.5 * (a + b)
    }
}

/// What the fill is asked for besides its boundary.
#[derive(Clone, Copy, Debug)]
pub struct IsoSpec<'a> {
    /// How far a point may be from the fill.
    pub tol: f64,
    /// How far (radians) the fill's normal may turn from a G1/G2 neighbour's.
    pub angle_tol: f64,
    /// How full the middle is made (as [`crate::fill::fill_loop`]'s crown).
    pub crown: f64,
    /// Points the fill passes through.
    pub points: &'a [Vec3],
}

/// What a side of a patch is, running the patch's loop counter-clockwise in (u, v) from `uv[0]`
/// to `uv[1]` (a straight line, affine in the curve's parameter).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum IsoSide {
    /// The piece `[from, to]` of loop edge `edge` (its own parameter, running the loop's way).
    Edge { edge: usize, from: f64, to: f64, uv: [[f64; 2]; 2] },
    /// Spoke `spoke` over its parameter `[0, 1]` (from its edge's midpoint to the centre),
    /// run that way (`forward`) or back.
    Spoke { spoke: usize, forward: bool, uv: [[f64; 2]; 2] },
}

/// One patch of the fill and its sides (three around a pole, else four).
#[derive(Clone, Debug, PartialEq)]
pub struct IsoPatch {
    pub surface: NurbsSurface,
    pub sides: Vec<IsoSide>,
}

/// The fill and what it achieved.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoFill {
    pub patches: Vec<IsoPatch>,
    /// Each spoke as a curve (degree, knots on `[0, 1]`, control points): from its edge's
    /// midpoint to the centre. Empty for a fill of one patch.
    pub spokes: Vec<IsoEdge>,
    pub achieved: Achieved,
}

/// What a fill achieves at its stations and points (its boundary is exact).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Achieved {
    /// The largest angle (radians) between the fill's normal and a G1/G2 neighbour's (or, across
    /// a spoke, between the two patches') at a station held to the tolerance, and at any station.
    pub normal_deviation: f64,
    pub normal_deviation_all: f64,
    /// The largest difference (1/length) between the fill's normal curvature across a G2 edge
    /// and the neighbour's.
    pub curvature_mismatch: f64,
    /// The farthest point from the fill.
    pub point_deviation: f64,
}

/// A curve on `[0, 1]`.
#[derive(Clone, Debug)]
struct Curve {
    space: Space,
    points: Vec<Vec3>,
}

/// The piece `[a, b]` of `edge`, on `[0, 1]` (run backwards when `reverse`).
fn on_unit(edge: &IsoEdge, [a, b]: [f64; 2], reverse: bool) -> SplineResult<Curve> {
    let p = edge.degree;
    let h: Vec<HPoint> = edge.points.iter().map(|q| HPoint::weighted(*q, 1.0)).collect();
    let (knots, h) = segment(p, &edge.knots, &h, a, b)?;
    let map = |k: f64| if reverse { (b - k) / (b - a) } else { (k - a) / (b - a) };
    let mut knots: Vec<f64> = knots.iter().map(|k| map(*k)).collect();
    let mut points: Vec<Vec3> = h.iter().map(|q| q.xyz()).collect();
    if reverse {
        knots.reverse();
        points.reverse();
    }
    // The ends exactly on the unit interval (the map's rounding).
    for k in knots.iter_mut().take(p + 1) {
        *k = 0.0;
    }
    let n = knots.len();
    for k in knots.iter_mut().skip(n - p - 1) {
        *k = 1.0;
    }
    Ok(Curve { space: Space::new(p, knots)?, points })
}

/// The straight line from `a` to `b` on `[0, 1]` (a spoke's start, or a pole when `a == b`).
fn line(a: Vec3, b: Vec3) -> SplineResult<Curve> {
    Ok(Curve { space: Space::polynomial(1, 0.0, 1.0)?, points: vec![a, b] })
}

/// The breaks of a space: its distinct knots.
fn breaks(s: &Space) -> Vec<f64> {
    let mut out: Vec<f64> = s.knots().to_vec();
    out.dedup();
    out
}

/// One space holding every curve: every break of any (and `extra`), each repeated as often as the
/// highest degree; and each curve's control points there (each Bézier piece raised to that
/// degree). Exact.
fn compatible(curves: &[&Curve], extra: &[f64]) -> SplineResult<(Space, Vec<Vec<Vec3>>)> {
    let q = curves.iter().map(|s| s.space.degree()).max().unwrap_or(1);
    let mut all: Vec<f64> = curves.iter().flat_map(|s| breaks(&s.space)).chain(extra.iter().copied()).filter(|x| (0.0..=1.0).contains(x)).collect();
    all.sort_by(f64::total_cmp);
    all.dedup();
    let mut knots = vec![0.0; q + 1];
    for &x in all.iter().filter(|x| **x > 0.0 && **x < 1.0) {
        knots.extend(std::iter::repeat_n(x, q));
    }
    knots.extend(std::iter::repeat_n(1.0, q + 1));
    let target = Space::new(q, knots)?;
    let mut out = Vec::with_capacity(curves.len());
    for c in curves {
        let p = c.space.degree();
        let h: Vec<HPoint> = c.points.iter().map(|x| HPoint::weighted(*x, 1.0)).collect();
        let mut points: Vec<Vec3> = Vec::new();
        for w in all.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (_, piece) = segment(p, c.space.knots(), &h, a, b)?;
            let piece: Vec<Vec3> = piece.iter().map(|x| x.xyz()).collect();
            let raised = embed(&Space::polynomial(p, a, b)?, &piece, &Space::polynomial(q, a, b)?)?;
            points.extend(raised.into_iter().skip(usize::from(!points.is_empty())));
        }
        if points.len() != target.count() {
            return invalid("the sides' pieces do not fill the common space");
        }
        out.push(points);
    }
    Ok((target, out))
}

/// The Greville abscissae of a space (where its splines reproduce a linear function).
fn greville(s: &Space) -> Vec<f64> {
    let (p, k) = (s.degree(), s.knots());
    (0..s.count()).map(|i| (1..=p).map(|r| k.get(i + r).copied().unwrap_or(0.0)).sum::<f64>() / p as f64).collect()
}

/// A tensor patch in the fill's space both ways, control point `(i, j)` (i along u) at
/// `i + c·j`, `c` the space's count.
#[derive(Clone, Debug)]
struct Patch {
    space: Space,
    net: Vec<Vec3>,
    /// The patch collapses along `u = 0` (a three-sided patch).
    pole: bool,
}

impl Patch {
    fn count(&self) -> usize {
        self.space.count()
    }

    fn at(&self, i: usize, j: usize) -> Vec3 {
        self.net.get(i + self.count() * j).copied().unwrap_or(Vec3::ZERO)
    }

    /// The coefficients of `∂^(du+dv) S / ∂u^du ∂v^dv` at (u, v) on each control point it
    /// depends on: `(i, j, coefficient)`.
    fn row(&self, u: f64, v: f64, du: usize, dv: usize) -> SplineResult<Vec<(usize, usize, f64)>> {
        let (iu, bu) = self.space.basis(u, du)?;
        let (iv, bv) = self.space.basis(v, dv)?;
        let mut out = Vec::with_capacity(bu.len() * bv.len());
        for (b, nv) in bv.iter().enumerate() {
            for (a, nu) in bu.iter().enumerate() {
                out.push((iu + a, iv + b, nu * nv));
            }
        }
        Ok(out)
    }

    fn eval(&self, u: f64, v: f64, du: usize, dv: usize) -> SplineResult<Vec3> {
        Ok(self.row(u, v, du, dv)?.iter().fold(Vec3::ZERO, |s, (i, j, c)| s + self.at(*i, *j) * *c))
    }

    fn normal(&self, u: f64, v: f64) -> Option<Vec3> {
        let (su, sv) = (self.eval(u, v, 1, 0).ok()?, self.eval(u, v, 0, 1).ok()?);
        su.cross(sv).normalized()
    }

    fn surface(&self) -> SplineResult<NurbsSurface> {
        let c = self.count();
        let ctrl: Vec<HPoint> = (0..c).flat_map(|j| (0..c).map(move |i| (i, j))).map(|(i, j)| HPoint::weighted(self.at(i, j), 1.0)).collect();
        let knots = self.space.knots().to_vec();
        NurbsSurface::from_homogeneous([self.space.degree(); 2], [knots.clone(), knots], ctrl)
    }
}

/// Where side `s` of a patch runs on the unit square: (u, v) at its start and end.
fn side_uv(s: usize) -> [[f64; 2]; 2] {
    match s {
        0 => [[0.0, 0.0], [1.0, 0.0]],
        1 => [[1.0, 0.0], [1.0, 1.0]],
        2 => [[1.0, 1.0], [0.0, 1.0]],
        _ => [[0.0, 1.0], [0.0, 0.0]],
    }
}

/// At fraction `t` of side `s`: (u, v), and the derivative across into the patch
/// (`(du, dv, sign)`: the order taken along u or v, and its sign).
fn station_on(s: usize, t: f64) -> ([f64; 2], (usize, usize, f64)) {
    let [a, b] = side_uv(s);
    let uv = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let across = match s {
        0 => (0, 1, 1.0),
        1 => (1, 0, -1.0),
        2 => (0, 1, -1.0),
        _ => (1, 0, 1.0),
    };
    (uv, across)
}

/// The fill's control points: the patches, and which of their points are free (each free point
/// one unknown, a spoke's or the centre's shared by the patches it bounds).
struct Quilt {
    patches: Vec<Patch>,
    /// Per patch, per control point: its unknown, `None` where the loop holds it.
    slots: Vec<Vec<Option<usize>>>,
    unknowns: usize,
    /// The loop edges' pieces: (edge, edge parameter range, patch, side).
    pieces: Vec<(usize, [f64; 2], usize, usize)>,
    /// Spokes: (patch on whose `u = 1` it runs, patch on whose `v = 1` it runs).
    spokes: Vec<(usize, usize)>,
}

impl Quilt {
    /// The fill's patches over the loop's sides in the space refined by `extra` breaks.
    fn new(edges: &[IsoEdge], extra: &[f64]) -> SplineResult<Quilt> {
        let n = edges.len();
        let corner = |k: usize| edges.get(k).and_then(|e| e.points.first()).copied().unwrap_or(Vec3::ZERO);
        if n <= 4 {
            // One patch: the edges are its sides (side 2 and 3 run back along their rows).
            let mut sides: Vec<Curve> = edges.iter().enumerate().map(|(k, e)| on_unit(e, e.domain(), k >= 2)).collect::<SplineResult<_>>()?;
            if n == 3 {
                sides.push(line(corner(0), corner(0))?);
            }
            let refs: Vec<&Curve> = sides.iter().collect();
            let (space, rows) = compatible(&refs, extra)?;
            let (Some(c0), Some(c1), Some(c2), Some(c3)) = (rows.first(), rows.get(1), rows.get(2), rows.get(3)) else { return invalid("sides") };
            let patch = coons(&space, c0, c2, c3, c1, n == 3)?;
            let c = space.count();
            let mut slots = vec![None; c * c];
            let mut unknowns = 0;
            for j in 1..c - 1 {
                for i in 1..c - 1 {
                    if let Some(s) = slots.get_mut(i + c * j) {
                        *s = Some(unknowns);
                        unknowns += 1;
                    }
                }
            }
            let pieces = edges.iter().enumerate().map(|(k, e)| (k, e.domain(), 0, k)).collect();
            return Ok(Quilt { patches: vec![patch], slots: vec![slots], unknowns, pieces, spokes: Vec::new() });
        }
        // N patches about the centre: halves of each edge, and straight spokes to start.
        let middle: Vec<Vec3> = edges.iter().map(|e| eval_edge(e, e.middle())).collect::<SplineResult<_>>()?;
        let centre = middle.iter().fold(Vec3::ZERO, |s, p| s + *p) * (1.0 / n as f64);
        let mut curves: Vec<Curve> = Vec::with_capacity(3 * n);
        for e in edges {
            let [a, b] = e.domain();
            curves.push(on_unit(e, [a, e.middle()], false)?);
            // The second half runs up u = 0 of the next patch: from V (v = 0) back to M.
            curves.push(on_unit(e, [e.middle(), b], true)?);
        }
        for m in &middle {
            curves.push(line(*m, centre)?);
        }
        let refs: Vec<&Curve> = curves.iter().collect();
        let (space, rows) = compatible(&refs, extra)?;
        let c = space.count();
        let row = |r: usize| rows.get(r).cloned().ok_or_else(|| SplineError::Invalid("rows".into()));
        let mut patches = Vec::with_capacity(n);
        for k in 0..n {
            let prev = (k + n - 1) % n;
            // v = 0: edge k's first half; u = 0: edge k − 1's second half, from V_k up to M_{k−1};
            // u = 1: spoke k; v = 1: spoke k − 1, from M_{k−1} to C.
            patches.push(coons(&space, &row(2 * k)?, &row(2 * n + prev)?, &row(2 * prev + 1)?, &row(2 * n + k)?, false)?);
        }
        // Unknowns: each spoke's points past its midpoint, the centre once, each patch's
        // interior.
        let mut unknowns = 0usize;
        let mut next = || {
            unknowns += 1;
            unknowns - 1
        };
        let centre_id = next();
        let spoke_ids: Vec<Vec<usize>> = (0..n).map(|_| (1..c - 1).map(|_| next()).collect()).collect();
        let mut slots = Vec::with_capacity(n);
        for k in 0..n {
            let prev = (k + n - 1) % n;
            let mut s = vec![None; c * c];
            for j in 1..c {
                for i in 1..c {
                    let id = match (i == c - 1, j == c - 1) {
                        (true, true) => centre_id,
                        (true, false) => *spoke_ids.get(k).and_then(|x| x.get(j - 1)).ok_or_else(|| SplineError::Invalid("spoke".into()))?,
                        (false, true) => *spoke_ids.get(prev).and_then(|x| x.get(i - 1)).ok_or_else(|| SplineError::Invalid("spoke".into()))?,
                        (false, false) => next(),
                    };
                    if let Some(slot) = s.get_mut(i + c * j) {
                        *slot = Some(id);
                    }
                }
            }
            slots.push(s);
        }
        let mut pieces = Vec::with_capacity(2 * n);
        for (k, e) in edges.iter().enumerate() {
            let [a, b] = e.domain();
            pieces.push((k, [a, e.middle()], k, 0));
            pieces.push((k, [e.middle(), b], (k + 1) % n, 3));
        }
        let spokes = (0..n).map(|k| (k, (k + 1) % n)).collect();
        Ok(Quilt { patches, slots, unknowns, pieces, spokes })
    }

    /// Write the unknowns' values into every patch that holds them.
    fn set(&mut self, values: &[Vec3]) {
        for (patch, slots) in self.patches.iter_mut().zip(&self.slots) {
            for (p, s) in patch.net.iter_mut().zip(slots) {
                if let Some(v) = s.and_then(|s| values.get(s)) {
                    *p = *v;
                }
            }
        }
    }
}

/// A point of an edge.
fn eval_edge(e: &IsoEdge, t: f64) -> SplineResult<Vec3> {
    Space::new(e.degree, e.knots.clone())?.eval(&e.points, t, 0)
}

/// The bilinearly blended Coons patch: `c0`, `c1` along u at v = 0 and 1, `d0`, `d1` along v at
/// u = 0 and 1, all running with their parameter.
fn coons(space: &Space, c0: &[Vec3], c1: &[Vec3], d0: &[Vec3], d1: &[Vec3], pole: bool) -> SplineResult<Patch> {
    let g = greville(space);
    let c = space.count();
    let (Some(p00), Some(p10), Some(p01), Some(p11)) = (c0.first().copied(), c0.last().copied(), c1.first().copied(), c1.last().copied()) else {
        return invalid("a side has no points");
    };
    let get = |a: &[Vec3], k: usize| a.get(k).copied().unwrap_or(Vec3::ZERO);
    let mut net = Vec::with_capacity(c * c);
    for (j, &y) in g.iter().enumerate() {
        for (i, &x) in g.iter().enumerate() {
            let ruled_v = get(c0, i) * (1.0 - y) + get(c1, i) * y;
            let ruled_u = get(d0, j) * (1.0 - x) + get(d1, j) * x;
            let bilinear = (p00 * (1.0 - x) + p10 * x) * (1.0 - y) + (p01 * (1.0 - x) + p11 * x) * y;
            // The boundary rows exactly the sides' own points.
            net.push(match (i, j) {
                (_, 0) => get(c0, i),
                (_, j) if j + 1 == c => get(c1, i),
                (0, _) => get(d0, j),
                (i, _) if i + 1 == c => get(d1, j),
                _ => ruled_v + ruled_u - bilinear,
            });
        }
    }
    Ok(Patch { space: space.clone(), net, pole })
}

/// One condition row: coefficients on patches' control points `(patch, i, j, coefficient)`,
/// dotted with `dir` when given, else on one coordinate `coord`; its value and weight.
struct Row {
    terms: Vec<(usize, usize, usize, f64)>,
    dir: Option<Vec3>,
    coord: usize,
    value: f64,
    weight: f64,
}

/// A station of a loop edge: its patch, side and fraction there, its condition, whether it is
/// held to the angle tolerance, and the side's speed there.
struct Station {
    patch: usize,
    side: usize,
    t: f64,
    across: Across,
    held: bool,
    speed: f64,
}

/// Fill the loop `edges` (three to [`MAX_ISO_EDGES`], each ending where the next starts) with
/// patches on them (module docs). `across(k, ts)` gives the neighbour's conditions at the
/// parameters `ts` of edge `k` (its own parameter; `None`: G0 there); `clash[k]`: the
/// neighbours' tangent planes disagree at the corner where edge `k` starts.
pub fn iso_fill(
    edges: &[IsoEdge],
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
    clash: &[bool],
    spec: &IsoSpec,
) -> SplineResult<IsoFill> {
    let n = edges.len();
    if !(3..=MAX_ISO_EDGES).contains(&n) {
        return Err(SplineError::Unsupported(format!("a patch on a loop of {n} edges (three to {MAX_ISO_EDGES})")));
    }
    if !(spec.tol.is_finite() && spec.tol > 0.0 && spec.angle_tol.is_finite() && spec.angle_tol > 0.0) {
        return invalid("tolerances must be positive");
    }
    if !(crate::fill::MIN_CROWN..=crate::fill::MAX_CROWN).contains(&spec.crown) {
        return invalid("a crown must be between -0.9 and 3");
    }
    if spec.points.len() > crate::fill_general::MAX_FILL_POINTS || spec.points.iter().any(|p| !p.is_finite()) {
        return invalid("too many points, or a point that is not finite");
    }
    for (k, e) in edges.iter().enumerate() {
        let p = e.degree;
        if p == 0 || p > MAX_SPLINE_DEGREE || e.points.len() <= p || e.knots.len() != e.points.len() + p + 1 {
            return invalid(format!("edge {k}'s degree, knots and control points do not fit"));
        }
        check_knots(&e.knots)?;
        let [a, b] = e.domain();
        if !(a < b) || e.points.iter().any(|q| !q.is_finite()) {
            return invalid(format!("edge {k} has an empty domain or a control point that is not finite"));
        }
    }
    let mut all = edges.iter().flat_map(|e| e.points.iter().copied());
    let first = all.next().ok_or_else(|| SplineError::Invalid("an edge has no control points".into()))?;
    let (lo, hi) = all.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let size = lo.dist(hi);
    for k in 0..n {
        let end = edges.get(k).and_then(|e| e.points.last()).copied().unwrap_or(Vec3::ZERO);
        let next = edges.get((k + 1) % n).and_then(|e| e.points.first()).copied().unwrap_or(Vec3::ZERO);
        if end.dist(next) > 1e-9 * size.max(1.0) {
            return invalid(format!("edge {k} does not end where edge {} starts", (k + 1) % n));
        }
    }
    let mut extra: Vec<f64> = Vec::new();
    let mut spans = 1usize;
    loop {
        let quilt = Quilt::new(edges, &extra)?;
        let stations = stations(edges, &quilt, &mut *across, clash)?;
        let (quilt, report) = solve(quilt, &stations, spec)?;
        let fits = report.normal_deviation <= spec.angle_tol && report.point_deviation <= spec.tol && !quilt.patches.iter().any(folds);
        let space = quilt.patches.first().map(|p| p.space.clone()).ok_or_else(|| SplineError::Invalid("no patch".into()))?;
        let finer = quilt.unknowns * 4 > MAX_UNKNOWN_POINTS;
        if fits || spans * 2 > MAX_ISO_SPANS || finer {
            if !fits {
                return Err(SplineError::Unsupported(format!(
                    "a patch on these edges within the tolerances: normals within {:.4}° of the neighbours', points within {:.2e}",
                    report.normal_deviation.to_degrees(),
                    report.point_deviation
                )));
            }
            let quilt = crowned(quilt, &stations, spec.crown, size)?;
            return finished(quilt, edges, report);
        }
        // Halve every span.
        spans *= 2;
        let b = breaks(&space);
        extra.extend(b.windows(2).map(|w| 0.5 * (w[0] + w[1])));
    }
}

/// The patches, their sides and the spokes.
fn finished(quilt: Quilt, edges: &[IsoEdge], achieved: Achieved) -> SplineResult<IsoFill> {
    let n = edges.len();
    let mut patches = Vec::with_capacity(quilt.patches.len());
    for (k, patch) in quilt.patches.iter().enumerate() {
        let mut sides: Vec<IsoSide> = Vec::with_capacity(4);
        for &(edge, [from, to], p, s) in &quilt.pieces {
            if p == k {
                sides.push(IsoSide::Edge { edge, from, to, uv: side_uv(s) });
            }
        }
        if !quilt.spokes.is_empty() {
            // Spoke k up u = 1 (from its midpoint to the centre), spoke k − 1 back along v = 1.
            sides.push(IsoSide::Spoke { spoke: k, forward: true, uv: side_uv(1) });
            sides.push(IsoSide::Spoke { spoke: (k + n - 1) % n, forward: false, uv: side_uv(2) });
        }
        // The loop's order round the patch: by the side each starts on.
        let start = |s: &IsoSide| match s {
            IsoSide::Edge { uv, .. } | IsoSide::Spoke { uv, .. } => (0..4).position(|q| side_uv(q) == *uv).unwrap_or(4),
        };
        sides.sort_by_key(start);
        patches.push(IsoPatch { surface: patch.surface()?, sides });
    }
    let spokes = quilt
        .spokes
        .iter()
        .filter_map(|&(left, _)| {
            let p = quilt.patches.get(left)?;
            let c = p.count();
            Some(IsoEdge { degree: p.space.degree(), knots: p.space.knots().to_vec(), points: (0..c).map(|j| p.at(c - 1, j)).collect() })
        })
        .collect();
    Ok(IsoFill { patches, spokes, achieved })
}

/// The stations of every loop edge with a condition: enough of them for the space's spans.
fn stations(
    edges: &[IsoEdge],
    quilt: &Quilt,
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
    clash: &[bool],
) -> SplineResult<Vec<Station>> {
    let n = edges.len();
    let mut out = Vec::new();
    let Some(space) = quilt.patches.first().map(|p| &p.space) else { return Ok(out) };
    let count = (breaks(space).len().saturating_sub(1) * STATIONS_PER_SPAN).max(MIN_STATIONS);
    for &(k, [a, b], patch, side) in &quilt.pieces {
        let [ea, eb] = edges.get(k).map(IsoEdge::domain).unwrap_or([0.0, 1.0]);
        // Stations inside the piece (its corners are its neighbours' corners); every side runs
        // the loop's way round its patch, as the edge does.
        let ts: Vec<f64> = (0..count).map(|i| (i as f64 + 0.5) / count as f64).collect();
        let params: Vec<f64> = ts.iter().map(|t| a + (b - a) * t).collect();
        let found = across(k, &params).map_err(SplineError::Invalid)?;
        let Some(p) = quilt.patches.get(patch) else { continue };
        for (&param, c) in params.iter().zip(found) {
            let Some(c) = c else { continue };
            let t = (param - a) / (b - a);
            let f = (param - ea) / (eb - ea);
            let near_start = clash.get(k).copied().unwrap_or(false) && f < CLASH_SHARE;
            let near_end = clash.get((k + 1) % n).copied().unwrap_or(false) && f > 1.0 - CLASH_SHARE;
            let (uv, (du, dv, _)) = station_on(side, t);
            let speed = p.eval(uv[0], uv[1], dv, du).map(|d| d.len()).unwrap_or(0.0);
            out.push(Station { patch, side, t, across: c, held: !(near_start || near_end), speed });
        }
    }
    Ok(out)
}

/// The fill's free control points by least squares (module docs), and what they achieve.
fn solve(mut quilt: Quilt, stations: &[Station], spec: &IsoSpec) -> SplineResult<(Quilt, Achieved)> {
    if quilt.unknowns == 0 {
        return Ok((quilt, Achieved::default()));
    }
    if quilt.unknowns > MAX_UNKNOWN_POINTS {
        return Err(SplineError::TooLarge(format!("{} free control points (at most {MAX_UNKNOWN_POINTS})", quilt.unknowns)));
    }
    let energy: Vec<Vec<(usize, usize, f64)>> = quilt.patches.iter().map(energy).collect::<SplineResult<_>>()?;
    let mut located: Vec<(usize, [f64; 2])> = Vec::new();
    let mut report = Achieved::default();
    let spoke_count = quilt.patches.first().map_or(0, |p| (breaks(&p.space).len().saturating_sub(1) * STATIONS_PER_SPAN).max(MIN_STATIONS));
    for round in 0..ROUNDS {
        let mut rows: Vec<Row> = Vec::new();
        for s in stations {
            let Some(patch) = quilt.patches.get(s.patch) else { continue };
            let (uv, (du, dv, sign)) = station_on(s.side, s.t);
            let first: Vec<(usize, usize, usize, f64)> =
                patch.row(uv[0], uv[1], du, dv)?.into_iter().map(|(i, j, c)| (s.patch, i, j, c * sign)).collect();
            rows.push(Row { terms: first.clone(), dir: Some(s.across.normal), coord: 0, value: 0.0, weight: 1.0 });
            let w = s.across.leave * s.speed;
            for (c, value) in [w.x, w.y, w.z].into_iter().enumerate() {
                rows.push(Row { terms: first.clone(), dir: None, coord: c, value, weight: HERMITE_WEIGHT });
            }
            if let Some(bend) = s.across.bend {
                let along = (patch.eval(uv[0], uv[1], du, dv)? * sign).dot(s.across.leave);
                let second = patch.row(uv[0], uv[1], 2 * du, 2 * dv)?.into_iter().map(|(i, j, c)| (s.patch, i, j, c)).collect();
                rows.push(Row { terms: second, dir: Some(s.across.normal), coord: 0, value: along * along * bend.dot(s.across.normal), weight: 1.0 });
            }
        }
        // Across each spoke the two patches' derivatives away from it are opposed.
        for &(left, right) in &quilt.spokes {
            let (Some(a), Some(b)) = (quilt.patches.get(left), quilt.patches.get(right)) else { continue };
            for i in 0..spoke_count {
                let t = (i as f64 + 0.5) / spoke_count as f64;
                let mut terms: Vec<(usize, usize, usize, f64)> = a.row(1.0, t, 1, 0)?.into_iter().map(|(i, j, c)| (left, i, j, c)).collect();
                terms.extend(b.row(t, 1.0, 0, 1)?.into_iter().map(|(i, j, c)| (right, i, j, c)));
                for c in 0..3 {
                    rows.push(Row { terms: terms.clone(), dir: None, coord: c, value: 0.0, weight: 1.0 });
                }
            }
        }
        for (p, (patch, uv)) in spec.points.iter().zip(&located) {
            let Some(on) = quilt.patches.get(*patch) else { continue };
            let terms: Vec<(usize, usize, usize, f64)> = on.row(uv[0], uv[1], 0, 0)?.into_iter().map(|(i, j, c)| (*patch, i, j, c)).collect();
            for (c, value) in [p.x, p.y, p.z].into_iter().enumerate() {
                rows.push(Row { terms: terms.clone(), dir: None, coord: c, value, weight: 1.0 });
            }
        }
        let solved = least_squares(&quilt, &rows, &energy)?;
        quilt.set(&solved);
        located = spec.points.iter().map(|p| nearest(&quilt, *p)).collect();
        report = measure(&quilt, stations, spec.points, &located, spoke_count)?;
        let relinearise = stations.iter().any(|s| s.across.bend.is_some()) || !spec.points.is_empty();
        if !relinearise && round == 0 {
            break;
        }
    }
    Ok((quilt, report))
}

/// A patch's thin-plate energy coefficients between its control points (flat indices), of the
/// terms `G⁽ʳ⁾ ⊗ G⁽ˢ⁾` with `(r, s)` = (2, 0), (1, 1) twice, (0, 2).
fn energy(patch: &Patch) -> SplineResult<Vec<(usize, usize, f64)>> {
    let s = &patch.space;
    let n = s.count();
    let mut g: [Vec<Vec<f64>>; 3] = std::array::from_fn(|_| vec![vec![0.0; n]; n]);
    let b = breaks(s);
    for w in b.windows(2) {
        let (lo, hi) = (w[0], w[1]);
        for (x, wt) in GAUSS4 {
            let t = 0.5 * (lo + hi) + 0.5 * (hi - lo) * x;
            let weight = 0.5 * (hi - lo) * wt;
            for (r, gr) in g.iter_mut().enumerate() {
                let (first, vals) = s.basis(t, r)?;
                for (a, va) in vals.iter().enumerate() {
                    for (c, vc) in vals.iter().enumerate() {
                        if let Some(cell) = gr.get_mut(first + a).and_then(|row| row.get_mut(first + c)) {
                            *cell += weight * va * vc;
                        }
                    }
                }
            }
        }
    }
    let at = |r: usize, a: usize, b: usize| g.get(r).and_then(|x| x.get(a)).and_then(|x| x.get(b)).copied().unwrap_or(0.0);
    let mut out = Vec::new();
    for j in 0..n {
        for l in 0..n {
            for i in 0..n {
                for k in 0..n {
                    let e = at(2, i, k) * at(0, j, l) + 2.0 * at(1, i, k) * at(1, j, l) + at(0, i, k) * at(2, j, l);
                    if e != 0.0 {
                        out.push((i + n * j, k + n * l, e));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The free points minimising the weighted rows' squares plus a vanishing multiple of the
/// patches' energy, the loop's rows held: the normal equations, solved by Cholesky.
fn least_squares(quilt: &Quilt, rows: &[Row], energy: &[Vec<(usize, usize, f64)>]) -> SplineResult<Vec<Vec3>> {
    let dim = 3 * quilt.unknowns;
    let mut a = vec![0.0; dim * dim];
    let mut b = vec![0.0; dim];
    let slot = |p: usize, flat: usize| quilt.slots.get(p).and_then(|s| s.get(flat)).copied().flatten();
    let value = |p: usize, flat: usize| quilt.patches.get(p).and_then(|x| x.net.get(flat)).copied().unwrap_or(Vec3::ZERO);
    let mut condition_trace = 0.0;
    for r in rows {
        let mut unknown: Vec<(usize, f64)> = Vec::new();
        let mut known = 0.0;
        for &(p, i, j, c) in &r.terms {
            let flat = i + quilt.patches.get(p).map_or(0, Patch::count) * j;
            let coords = match r.dir {
                Some(d) => [d.x, d.y, d.z],
                None => std::array::from_fn(|k| if k == r.coord { 1.0 } else { 0.0 }),
            };
            let v = value(p, flat);
            for (k, f) in coords.iter().enumerate() {
                if *f == 0.0 {
                    continue;
                }
                match slot(p, flat) {
                    Some(x) => unknown.push((3 * x + k, c * f)),
                    None => known += c * f * [v.x, v.y, v.z][k],
                }
            }
        }
        let rhs = r.value - known;
        let w2 = r.weight * r.weight;
        for &(x, cx) in &unknown {
            for &(y, cy) in &unknown {
                if let Some(cell) = a.get_mut(x * dim + y) {
                    *cell += w2 * cx * cy;
                }
            }
            if let Some(cell) = b.get_mut(x) {
                *cell += w2 * cx * rhs;
            }
            condition_trace += w2 * cx * cx;
        }
    }
    // The energy decides what the rows leave free; with no rows it is all there is.
    let energy_trace: f64 =
        energy.iter().enumerate().flat_map(|(p, e)| e.iter().filter(move |(x, y, _)| x == y && slot(p, *x).is_some())).map(|(_, _, e)| 3.0 * e).sum();
    let lambda = if condition_trace > 0.0 && energy_trace > 0.0 { 1e-7 * condition_trace / energy_trace } else { 1.0 };
    for (p, entries) in energy.iter().enumerate() {
        for &(f, g, e) in entries {
            let Some(x) = slot(p, f) else { continue };
            match slot(p, g) {
                Some(y) => {
                    for k in 0..3 {
                        if let Some(cell) = a.get_mut((3 * x + k) * dim + 3 * y + k) {
                            *cell += lambda * e;
                        }
                    }
                }
                None => {
                    let fixed = value(p, g);
                    for (k, v) in [fixed.x, fixed.y, fixed.z].into_iter().enumerate() {
                        if let Some(cell) = b.get_mut(3 * x + k) {
                            *cell -= lambda * e * v;
                        }
                    }
                }
            }
        }
    }
    let x = cholesky(a, b, dim)?;
    Ok((0..quilt.unknowns)
        .map(|i| Vec3::new(x.get(3 * i).copied().unwrap_or(0.0), x.get(3 * i + 1).copied().unwrap_or(0.0), x.get(3 * i + 2).copied().unwrap_or(0.0)))
        .collect())
}

/// Solve the symmetric positive definite `a·x = b` (dense, `n × n`).
fn cholesky(mut a: Vec<f64>, mut b: Vec<f64>, n: usize) -> SplineResult<Vec<f64>> {
    let at = |a: &[f64], r: usize, c: usize| a.get(r * n + c).copied().unwrap_or(0.0);
    for c in 0..n {
        let mut d = at(&a, c, c);
        for k in 0..c {
            d -= at(&a, c, k).powi(2);
        }
        if !(d > 0.0) || !d.is_finite() {
            return invalid("the patch's equations are singular");
        }
        let d = d.sqrt();
        if let Some(x) = a.get_mut(c * n + c) {
            *x = d;
        }
        for r in c + 1..n {
            let mut v = at(&a, r, c);
            for k in 0..c {
                v -= at(&a, r, k) * at(&a, c, k);
            }
            if let Some(x) = a.get_mut(r * n + c) {
                *x = v / d;
            }
        }
    }
    for r in 0..n {
        let mut v = b.get(r).copied().unwrap_or(0.0);
        for k in 0..r {
            v -= at(&a, r, k) * b.get(k).copied().unwrap_or(0.0);
        }
        if let Some(x) = b.get_mut(r) {
            *x = v / at(&a, r, r);
        }
    }
    for r in (0..n).rev() {
        let mut v = b.get(r).copied().unwrap_or(0.0);
        for k in r + 1..n {
            v -= at(&a, k, r) * b.get(k).copied().unwrap_or(0.0);
        }
        if let Some(x) = b.get_mut(r) {
            *x = v / at(&a, r, r);
        }
    }
    if b.iter().any(|x| !x.is_finite()) {
        return invalid("the patch's equations have no finite solution");
    }
    Ok(b)
}

/// What the fill achieves at the stations, across the spokes and at the points.
fn measure(quilt: &Quilt, stations: &[Station], points: &[Vec3], located: &[(usize, [f64; 2])], spoke_count: usize) -> SplineResult<Achieved> {
    let mut r = Achieved::default();
    let mut angle = |a: f64, held: bool| {
        r.normal_deviation_all = r.normal_deviation_all.max(a);
        if held {
            r.normal_deviation = r.normal_deviation.max(a);
        }
    };
    let mut mismatch = 0.0f64;
    for s in stations {
        let Some(patch) = quilt.patches.get(s.patch) else { continue };
        let (uv, (du, dv, sign)) = station_on(s.side, s.t);
        let Some(normal) = patch.normal(uv[0], uv[1]) else { continue };
        angle(normal.dot(s.across.normal).abs().clamp(0.0, 1.0).acos(), s.held);
        if let Some(bend) = s.across.bend {
            let first = patch.eval(uv[0], uv[1], du, dv)? * sign;
            let second = patch.eval(uv[0], uv[1], 2 * du, 2 * dv)?;
            let mine = second.dot(s.across.normal) / first.len2().max(1e-300);
            mismatch = mismatch.max((mine - bend.dot(s.across.normal)).abs());
        }
    }
    // Across the spokes, clear of the centre.
    for &(left, right) in &quilt.spokes {
        let (Some(a), Some(b)) = (quilt.patches.get(left), quilt.patches.get(right)) else { continue };
        for i in 0..spoke_count {
            let t = (i as f64 + 0.5) / spoke_count as f64;
            if let (Some(na), Some(nb)) = (a.normal(1.0, t), b.normal(t, 1.0)) {
                angle(na.dot(nb).clamp(-1.0, 1.0).acos(), t < 1.0 - CLASH_SHARE);
            }
        }
    }
    r.curvature_mismatch = mismatch;
    for (p, (patch, uv)) in points.iter().zip(located) {
        if let Some(on) = quilt.patches.get(*patch) {
            r.point_deviation = r.point_deviation.max(on.eval(uv[0], uv[1], 0, 0)?.dist(*p));
        }
    }
    Ok(r)
}

/// The patch and parameters of the fill's point nearest `p`: the nearest of a grid of samples
/// on every patch, then Gauss–Newton inside that patch's square.
fn nearest(quilt: &Quilt, p: Vec3) -> (usize, [f64; 2]) {
    const GRID: usize = 24;
    let mut best = (f64::INFINITY, 0usize, [0.5, 0.5]);
    for (k, patch) in quilt.patches.iter().enumerate() {
        for i in 0..=GRID {
            for j in 0..=GRID {
                let uv = [i as f64 / GRID as f64, j as f64 / GRID as f64];
                if let Ok(q) = patch.eval(uv[0], uv[1], 0, 0) {
                    let d = q.dist(p);
                    if d < best.0 {
                        best = (d, k, uv);
                    }
                }
            }
        }
    }
    let (_, k, [mut u, mut v]) = best;
    let Some(patch) = quilt.patches.get(k) else { return (k, [u, v]) };
    for _ in 0..30 {
        let (Ok(q), Ok(a), Ok(b)) = (patch.eval(u, v, 0, 0), patch.eval(u, v, 1, 0), patch.eval(u, v, 0, 1)) else { break };
        let r = q - p;
        let (aa, ab, bb) = (a.dot(a), a.dot(b), b.dot(b));
        let det = aa * bb - ab * ab;
        if !(det.abs() > 1e-300) {
            break;
        }
        let (ga, gb) = (a.dot(r), b.dot(r));
        u = (u - (bb * ga - ab * gb) / det).clamp(0.0, 1.0);
        v = (v - (aa * gb - ab * ga) / det).clamp(0.0, 1.0);
    }
    (k, [u, v])
}

/// Whether a patch folds back on itself: its normal turns against the middle's somewhere on a
/// grid (clear of a three-sided patch's pole at u = 0, where the normal is undefined).
fn folds(patch: &Patch) -> bool {
    let Some(mid) = patch.normal(0.5, 0.5) else { return true };
    for i in 0..=FOLD_SAMPLES {
        if patch.pole && i == 0 {
            continue;
        }
        for j in 0..=FOLD_SAMPLES {
            let (u, v) = (i as f64 / FOLD_SAMPLES as f64, j as f64 / FOLD_SAMPLES as f64);
            match patch.normal(u, v) {
                Some(n) if n.dot(mid) > 0.0 => {}
                _ => return true,
            }
        }
    }
    false
}

/// The fill with its middle filled out by `crown` times the loop's `size` along the normals, on
/// control points clear of the rows the conditions hold (one row from a G0 edge, two from G1,
/// three from G2), so the loop and the conditions across it do not move. One patch rises by
/// `16·u(1−u)·v(1−v)`; patches about a centre by `u(2−u)·v(2−v)`, nought on the loop and one at
/// the centre, the same on a spoke from either side.
fn crowned(mut quilt: Quilt, stations: &[Station], crown: f64, size: f64) -> SplineResult<Quilt> {
    if crown == 0.0 {
        return Ok(quilt);
    }
    let held = 1 + stations.iter().map(|s| if s.across.bend.is_some() { 2 } else { 1 }).max().unwrap_or(0);
    let about_centre = !quilt.spokes.is_empty();
    let bump = |x: f64, y: f64| if about_centre { x * (2.0 - x) * y * (2.0 - y) } else { 16.0 * x * (1.0 - x) * y * (1.0 - y) };
    let mut moved = std::collections::HashMap::new();
    for (k, patch) in quilt.patches.iter().enumerate() {
        let c = patch.count();
        let g = greville(&patch.space);
        let far = if about_centre { c } else { c.saturating_sub(held) };
        for j in held..far {
            for i in held..far {
                let Some(Some(id)) = quilt.slots.get(k).and_then(|s| s.get(i + c * j)) else { continue };
                let (x, y) = (g.get(i).copied().unwrap_or(0.5), g.get(j).copied().unwrap_or(0.5));
                let Some(n) = patch.normal(x, y) else { continue };
                moved.entry(*id).or_insert_with(|| patch.at(i, j) + n * (crown * size * bump(x, y)));
            }
        }
    }
    if moved.is_empty() {
        return Err(SplineError::Unsupported("a crown on a fill with no room inside its held rows".into()));
    }
    for (patch, slots) in quilt.patches.iter_mut().zip(&quilt.slots) {
        for (p, s) in patch.net.iter_mut().zip(slots) {
            if let Some(v) = s.and_then(|s| moved.get(&s)) {
                *p = *v;
            }
        }
    }
    Ok(quilt)
}
