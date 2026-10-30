//! Filling a closed loop of exact B-spline curves, rational or not, with tensor B-spline patches
//! whose boundary rows *are* those curves, so that every edge lies on the fill exactly and its
//! pcurve is a straight line in (u, v), affine in the edge's own parameter.
//!
//! The formulation:
//!
//! - **The sides.** The region's corners are the loop's vertices where it turns by more than
//!   [`IsoSpec::corner_turn`]; a side is the chain of edges from one corner to the next, held as
//!   one exact B-spline on `[0, 1]` (each edge, on its own stretch of it by an affine map of its
//!   parameter, raised to the side's degree, the pieces joined at a knot of full multiplicity),
//!   so a vertex where the loop runs straight on (a seam) is never a patch's corner: a patch
//!   corner there would be flat (its two derivatives parallel), degenerate and refused by a
//!   B-rep's checks. A loop with fewer than three corners keeps its edges as its sides.
//! - **The patches.** A loop of four sides `e₀ … e₃` bounds one patch on the unit square: `e₀`
//!   runs along `v = 0` from `u = 0` to 1, `e₁` up `u = 1`, `e₂` back along `v = 1` and `e₃` down
//!   `u = 0`. A loop of three has no `e₃`: the side `u = 0` collapses to the loop's first corner
//!   (a pole, as a three-sided tensor patch must have). A loop of `N ≥ 5` sides is `N`
//!   four-sided patches about a centre point: each side is split at its middle, a spoke runs
//!   from each side's midpoint `M_k` to the centre `C`, and patch `k` has the corners
//!   `V_k, M_k, C, M_{k−1}` (`V_k` the corner where side `k` starts): the first half of side `k`
//!   along `v = 0`, spoke `k` up `u = 1`, spoke `k − 1` back along `v = 1` and the second half of
//!   side `k − 1` down `u = 0`. Each spoke is one row of the two patches it divides, the same
//!   control points in both. Every edge's pcurve is a straight line on its stretch of a patch's
//!   side, affine in its parameter.
//! - **Rational edges** (a circle's arc, as every conic is held) are boundary rows of a rational
//!   patch: the fill works on homogeneous control points `(w·P, w)`. An edge's homogeneous points
//!   may be scaled by a constant without changing its curve or its parameter, so each edge is
//!   scaled for the weights at the loop's corners to agree; that is possible exactly when the
//!   product of the edges' end-to-start weight ratios round the loop is one (SMLib's conics have
//!   weight one at their ends), and refused otherwise. Inside, the weights are the Coons blend of
//!   the boundary's and stay fixed, so the surface `S = Σ R_ij P_ij`, `R_ij = w_ij N_i N_j / W`,
//!   is linear in the free points and everything below is a linear least-squares problem.
//! - **The spaces.** Every side, each on `[0, 1]` by an affine map of its parameter (which
//!   changes neither the curve nor its pieces), is carried exactly into a common space: one
//!   patch has a space along u holding its two sides that run along u, and one along v; patches
//!   about a centre share one space both ways, which every edge half and spoke runs in. A space
//!   keeps each side's continuity at its breaks (a knot of multiplicity `m` in a side of degree
//!   `p` is one of multiplicity `m + q − p` at degree `q`, NURBS Book §5.5), and the breaks
//!   refinement adds are simple knots, so the fill is as smooth across them as its degree allows.
//!   The degree is the sides' highest, and at least one more than the highest continuity order
//!   n asked (G1 quadratic … G4 quintic), so the fill's own derivatives up to order n are
//!   continuous across the knots refinement adds.
//! - **The start**: each patch is the bilinearly blended Coons patch of its sides (S. A. Coons,
//!   1967), built on the Greville abscissae (a linear function is reproduced exactly there), the
//!   spokes starting as straight lines to the centre, the mean of the midpoints.
//! - **The inside**: with the loop's rows held, the other control points (each patch's interior,
//!   the spokes' and the centre, each once) minimise, by least squares, the misfit to the
//!   conditions plus a vanishing multiple of the patches' fairing energy of order n + 1,
//!   `∫∫ Σ_r C(n+1, r) |∂ᵘʳ ∂ᵥⁿ⁺¹⁻ʳ S|²` (the thin plate `∫∫ |S_uu|² + 2|S_uv|² + |S_vv|²` for G0
//!   and G1, of the control net on the polynomial basis; it alone decides a G0 fill), so the
//!   change of curvature the conditions ask at the edges is spread over the fill. The conditions are those of G^n continuity in the linkage-curve
//!   formulation ([`crate::linkage`]): at stations along an edge, the iso-parameter curve `c`
//!   of the fill leaving across it (σ growing into the fill) must have its height above the
//!   neighbour vanish to order n:
//!   - order 1: `n·c′ = 0` (`n` the neighbour's normal), with weak rows `w·c′ = s`, `t·c′ = a`
//!     (`w` the neighbour's leave direction, `t = n × w` along the edge) that keep the fill
//!     leaving into itself, about as fast: `(a, s)` is the Coons start's own cross derivative
//!     `d` turned about the edge into the neighbour's tangent plane (`a = t·d`, `s` the length of
//!     the rest of `d`). The Coons start is compatible at every corner (its cross derivative there
//!     is the tangent of the side it meets, as any tensor patch's must be: the two share the
//!     corner's next control point), so the target is too wherever that side lies in the
//!     neighbour's tangent plane; a target that is not (`t·c′ = 0` up to the corner) is a
//!     contradiction the least squares can only fold the corner's spans over. They act in the
//!     tangent plane only, so they never pull against the G1 row;
//!   - order k = 2 … n: `n·c⁽ᵏ⁾ = k!·[σᵏ] Σ_m H_m(x(σ))/m!` ([`crate::linkage::Jet::targets`]):
//!     the normal curvature (G2), its rate of change (G3) and that rate's (G4) are the
//!     neighbour's, in the fill's own leave direction. The right-hand side reads `c′ … c⁽ᵏ⁻¹⁾`
//!     from the last solution, so the rows are solved again, a few rounds;
//!   - across a patch's own C0 knot lines (a side's break where it is only C0 in its
//!     parameter), the derivatives either side in the ratio of the crossing sides' speeds
//!     ([`knot_line_rows`]): the fill is tangent continuous there too;
//!   - across each spoke, the two patches' derivatives away from it opposed up to the linear
//!     gluing function of G1 joins at an N-valent point (C. Loop, J. Peters, J. Hahn):
//!     `∂S_k/∂u + ∂S_{k+1}/∂v = α(t)·∂S/∂t` on the shared parameter t (0 at the edge's
//!     midpoint, 1 at the centre), `α(t) = 2·cos(2π/N)·t`: the patches join tangent there. At
//!     the midpoint `α = 0` (the edge's two halves leave it at the same speed, opposed); at the
//!     centre it is the value the spokes' tangents `d_k` meet when they are any affine image of
//!     the regular N-star, `d_{k−1} + d_{k+1} = 2·cos(2π/N)·d_k`. Parametric C1 (`α = 0`) asks
//!     `d_{k−1} + d_{k+1} = 0` there, which for N other than a multiple of four only `d = 0`
//!     meets: every patch would have a flat corner at the centre;
//!   - points the fill passes through, located on the last solution (nearest point of any patch)
//!     and fitted again (Hoschek's parameter correction).
//!
//!   At a corner where the neighbours' tangent planes disagree no surface is G1 to both, and at
//!   the centre the spokes' tangency conditions overdetermine the patches: the stations near
//!   either are fitted but not held to the angle tolerance.
//! - **Refinement**: while the normals at the stations of the sides running along u (or v) miss
//!   the neighbours' (or each other's across a spoke) by more than the angle tolerance beyond the
//!   edge's own lean out of the neighbour's tangent plane (a fill that holds an edge cannot come
//!   nearer its neighbour than the edge itself does), or a jump
//!   of order 2 … n carried over the loop's size does ([`within_order`]), or the space that way
//!   has too few rows for the conditions from opposite sides to stay apart, the spans that way are
//!   halved (both ways where a point is missed, a patch folds, or the patches share one space),
//!   up to [`MAX_ISO_SPANS`] and the free points one solve takes ([`MAX_BANDED_POINTS`] for one
//!   patch, whose system is banded, and about a centre, whose patches' banded interiors are
//!   eliminated onto the unknowns they share, at most [`MAX_SHARED_UNKNOWNS`]). A
//!   fill that then still misses the angle tolerance or a point, or folds, is refused; one that
//!   misses only an order above G1 is the closest the finest space holds, its jumps reported
//!   ([`Achieved::jumps`]).
//! - **A crown** fills the middle out along the normals, on control points clear of the rows the
//!   conditions hold, so the loop and the conditions across it do not move.
//!
//! The fill's boundary is exact, whatever its inside: a station's gap is nought by construction.
//! Input is hostile: sizes are capped and every evaluation checked; nothing here panics.

use crate::Vec3;
use crate::bspline::{HPoint, MAX_SPLINE_DEGREE, NurbsSurface, SplineError, SplineResult, check_knots, invalid, segment};
use crate::fill_general::Across;
use crate::linkage::MAX_CONTINUITY;
use crate::spline_space::{Cond, Interpolant, Space};

/// Most spans of the fill's space.
pub const MAX_ISO_SPANS: usize = 64;
/// Most edges of a loop.
pub const MAX_ISO_EDGES: usize = 16;
/// Most free control points of one solve: of one patch (its system is banded: see
/// [`Quilt::new`]), or of the patches about a centre (each patch's interior banded, eliminated
/// onto the unknowns the patches share: [`BlockNormal`]).
const MAX_BANDED_POINTS: usize = 6000;
/// Most unknowns (coordinates) the patches about a centre share once their interiors are
/// eliminated: the spokes, the centre and the points beside the spokes that rows across them
/// couple. Their system is dense: 4000² values, 128 MB.
const MAX_SHARED_UNKNOWNS: usize = 4000;
/// Stations per span of a side where conditions are asked and checked.
const STATIONS_PER_SPAN: usize = 6;
/// Stations per control point of a row.
const STATIONS_PER_POINT: usize = 3;
/// Fewest stations along a side.
const MIN_STATIONS: usize = 12;
/// Weight of the weak Hermite rows (in the tangent plane) against the G1 rows.
const HERMITE_WEIGHT: f64 = 0.05;
/// Rounds of relinearising the rows of order two and up and relocating the points, per order
/// above one (each order's right-hand side reads the lower derivatives of the last solution).
const ROUNDS_PER_ORDER: usize = 2;
/// Rounds when only points are relocated.
const ROUNDS: usize = 3;
/// The share of a side, from a clashing corner or the centre, whose stations are not held to
/// the angle tolerance.
const CLASH_SHARE: f64 = 0.15;
/// The share of a side, from either of its corners, whose stations' mismatches of order two and
/// up are not reported: at a corner the two sides' cross-boundary conditions meet and
/// overdetermine the fill (the compatibility of boundary data at a vertex).
const CORNER_SHARE: f64 = 0.1;
/// How many points each way the fold check samples.
const FOLD_SAMPLES: usize = 24;
/// How far, relative to one, the corners' weights may disagree once the edges are scaled.
const WEIGHT_TOL: f64 = 1e-9;
/// Six-point Gauss–Legendre nodes and weights on `[−1, 1]` (exact to degree 11: the Gram
/// matrices of quintics).
const GAUSS6: [(f64, f64); 6] = [
    (-0.932_469_514_203_152, 0.171_324_492_379_170),
    (-0.661_209_386_466_265, 0.360_761_573_048_139),
    (-0.238_619_186_083_197, 0.467_913_934_572_691),
    (0.238_619_186_083_197, 0.467_913_934_572_691),
    (0.661_209_386_466_265, 0.360_761_573_048_139),
    (0.932_469_514_203_152, 0.171_324_492_379_170),
];

/// One edge of the loop: a B-spline from the loop's corner before it to the one after (the
/// loop's direction), rational when it has weights.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoEdge {
    pub degree: usize,
    pub knots: Vec<f64>,
    pub points: Vec<Vec3>,
    /// One positive weight per point; `None` for a polynomial edge.
    pub weights: Option<Vec<f64>>,
}

impl IsoEdge {
    /// The edge's parameter interval.
    pub fn domain(&self) -> [f64; 2] {
        [self.knots.get(self.degree).copied().unwrap_or(0.0), self.knots.get(self.points.len()).copied().unwrap_or(0.0)]
    }

    /// The middle of the edge's parameter interval.
    pub fn middle(&self) -> f64 {
        let [a, b] = self.domain();
        0.5 * (a + b)
    }

    /// The unit tangent at parameter `t` (the loop's way), `None` where the edge stops.
    pub fn tangent(&self, t: f64) -> Option<Vec3> {
        let space = Space::new(self.degree, self.knots.clone()).ok()?;
        let h = self.homogeneous();
        let (a, da) = (space.eval(&h, t, 0).ok()?, space.eval(&h, t, 1).ok()?);
        let p = a.project()?;
        ((da.xyz() - p * da.w) * (1.0 / a.w)).normalized()
    }

    /// The homogeneous control points.
    fn homogeneous(&self) -> Vec<HPoint> {
        let w = |i: usize| self.weights.as_ref().and_then(|w| w.get(i)).copied().unwrap_or(1.0);
        self.points.iter().enumerate().map(|(i, p)| HPoint::weighted(*p, w(i))).collect()
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
    /// How far (radians) the loop may turn at a vertex for it not to be a corner of the region
    /// (the edges either side are then one side of the fill).
    pub corner_turn: f64,
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
    /// Each spoke as a curve (degree, knots on `[0, 1]`, control points and, on a rational fill,
    /// weights): from its edge's midpoint to the centre. Empty for a fill of one patch.
    pub spokes: Vec<IsoEdge>,
    /// Where each spoke starts: on loop edge `.0` at its parameter `.1` (the middle of a side).
    pub feet: Vec<(usize, f64)>,
    pub achieved: Achieved,
}

/// What a fill achieves at its stations and points (its boundary is exact).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Achieved {
    /// The largest angle (radians) between the fill's normal and a G1/G2 neighbour's (or, across
    /// a spoke, between the two patches') at a station held to the tolerance, and at any station.
    pub normal_deviation: f64,
    pub normal_deviation_all: f64,
    /// The largest angle (radians) by which the fill's normal at a station held to the tolerance
    /// misses the neighbour's beyond the edge's own lean out of the neighbour's tangent plane
    /// there (the fill holds the edge, so it can come no nearer): what the angle tolerance is
    /// held to.
    pub normal_excess: f64,
    /// `jumps[k]`, k = 2 … 4: the largest jump, clear of the loop's corners, of the k-th
    /// derivative of the linkage curves' height above the neighbour per unit length of their
    /// foot (`(n·c⁽ᵏ⁾ − target) / |x₁|ᵏ`, 1/lengthᵏ⁻¹) across the edges asked for order k or more:
    /// the normal curvature (k = 2), its rate of change (3) and that rate's (4). Zero below 2.
    pub jumps: [f64; MAX_CONTINUITY + 1],
    /// The farthest point from the fill.
    pub point_deviation: f64,
}

/// A curve on `[0, 1]`, on homogeneous control points.
#[derive(Clone, Debug)]
struct Curve {
    space: Space,
    points: Vec<HPoint>,
}

/// The piece `[a, b]` of `edge` (homogeneous points scaled by `scale`), on `[0, 1]` (run
/// backwards when `reverse`).
fn on_unit(edge: &IsoEdge, scale: f64, [a, b]: [f64; 2], reverse: bool) -> SplineResult<Curve> {
    let h: Vec<HPoint> = edge.homogeneous().into_iter().map(|q| q * scale).collect();
    piece_on_unit(edge.degree, &edge.knots, &h, [a, b], reverse)
}

/// The piece `[a, b]` of the curve of degree `p` on `knots` and homogeneous points `h`, on
/// `[0, 1]` (run backwards when `reverse`).
fn piece_on_unit(p: usize, knots: &[f64], h: &[HPoint], [a, b]: [f64; 2], reverse: bool) -> SplineResult<Curve> {
    let (knots, mut points) = segment(p, knots, h, a, b)?;
    let map = |k: f64| if reverse { (b - k) / (b - a) } else { (k - a) / (b - a) };
    let mut knots: Vec<f64> = knots.iter().map(|k| map(*k)).collect();
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

/// The straight line from `a` to `b` on `[0, 1]`, homogeneous (a spoke's start, or a pole when
/// the points are one).
fn line(a: HPoint, b: HPoint) -> SplineResult<Curve> {
    Ok(Curve { space: Space::polynomial(1, 0.0, 1.0)?, points: vec![a, b] })
}

/// The breaks of a space: its distinct knots.
fn breaks(s: &Space) -> Vec<f64> {
    let mut out: Vec<f64> = s.knots().to_vec();
    out.dedup();
    out
}

/// Breaks of different sides closer than this (on `[0, 1]`) are one break: the rounding of
/// mapping each side's parameter onto the unit interval.
const BREAK_TOL: f64 = 1e-12;

/// The knots of `s` with each one within [`BREAK_TOL`] of one of `breaks` moved onto it.
fn snapped(s: &Space, breaks: &[f64]) -> SplineResult<Space> {
    let knots = s.knots().iter().map(|k| breaks.iter().copied().find(|b| (b - k).abs() <= BREAK_TOL).unwrap_or(*k)).collect();
    Space::new(s.degree(), knots)
}

/// One space holding every curve, of degree `q` (at least each curve's), and each curve's
/// control points there. Each break of a curve keeps that curve's continuity there: a knot of
/// multiplicity `m` in a curve of degree `p` is one of multiplicity `m + q − p` in the space (as
/// raising the degree needs, NURBS Book §5.5), the most any curve asks (breaks of different
/// curves within [`BREAK_TOL`] made one); the breaks `extra` adds are simple knots. Exact: the
/// space contains every curve, which is carried into it by [`carried`].
fn compatible(curves: &[&Curve], extra: &[f64], q: usize) -> SplineResult<(Space, Vec<Vec<HPoint>>)> {
    let q = curves.iter().map(|s| s.space.degree()).max().unwrap_or(1).max(q);
    let mut need: Vec<(f64, usize)> = Vec::new();
    for c in curves {
        let p = c.space.degree();
        for (x, m) in interior_knots(&c.space) {
            let k = (m + q - p).min(q);
            match need.iter_mut().find(|(y, _)| (*y - x).abs() <= BREAK_TOL) {
                Some((_, have)) => *have = (*have).max(k),
                None => need.push((x, k)),
            }
        }
    }
    need.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut knots = vec![0.0; q + 1];
    for &(x, k) in &need {
        knots.extend(std::iter::repeat_n(x, k));
    }
    knots.extend(std::iter::repeat_n(1.0, q + 1));
    let target = Space::new(q, knots)?.refined(extra)?;
    let canonical: Vec<f64> = need.iter().map(|(x, _)| *x).collect();
    let mut out = Vec::with_capacity(curves.len());
    for c in curves {
        out.push(carried(&snapped(&c.space, &canonical)?, &c.points, &target)?);
    }
    Ok((target, out))
}

/// The distinct interior knots of a space and how often each is repeated.
fn interior_knots(s: &Space) -> Vec<(f64, usize)> {
    let [a, b] = s.domain();
    let mut out: Vec<(f64, usize)> = Vec::new();
    for &k in s.knots().iter().filter(|k| **k > a && **k < b) {
        match out.last_mut() {
            Some((x, m)) if *x == k => *m += 1,
            _ => out.push((k, 1)),
        }
    }
    out
}

/// The control points in `to` of the curve with control points `ctrl` in `from`, which `to`
/// contains: the curve interpolated at `to`'s Greville abscissae (where the interpolation
/// problem is uniquely solvable, Schoenberg–Whitney), which is the curve itself since it lies in
/// the space; its end points are the curve's own.
fn carried(from: &Space, ctrl: &[HPoint], to: &Space) -> SplineResult<Vec<HPoint>> {
    let conds: Vec<Cond> = greville(to).into_iter().map(|t| Cond { t, der: 0 }).collect();
    let data = conds.iter().map(|c| from.eval(ctrl, c.t, 0)).collect::<SplineResult<Vec<HPoint>>>()?;
    let mut points = Interpolant::new(to.clone(), conds)?.fit(&data)?;
    if let (Some(f), Some(first)) = (points.first_mut(), ctrl.first()) {
        *f = *first;
    }
    if let (Some(l), Some(last)) = (points.last_mut(), ctrl.last()) {
        *l = *last;
    }
    Ok(points)
}

/// The Greville abscissae of a space (where its splines reproduce a linear function).
fn greville(s: &Space) -> Vec<f64> {
    let (p, k) = (s.degree(), s.knots());
    (0..s.count()).map(|i| (1..=p).map(|r| k.get(i + r).copied().unwrap_or(0.0)).sum::<f64>() / p as f64).collect()
}

/// A stretch of a loop edge on one side of the region: edge `edge` over its parameters
/// `range` (the loop's way), on the fraction `at` of the side.
#[derive(Clone, Copy, Debug, PartialEq)]
struct SidePiece {
    edge: usize,
    range: [f64; 2],
    at: [f64; 2],
}

/// The region's sides (module docs): the chains of edges between the loop's corners, the first
/// starting at a corner; each edge its own side when the loop has fewer than three corners.
fn loop_sides(edges: &[IsoEdge], corner_turn: f64) -> Vec<Vec<SidePiece>> {
    let n = edges.len();
    let whole = |k: usize| SidePiece { edge: k, range: edges.get(k).map_or([0.0, 1.0], IsoEdge::domain), at: [0.0, 1.0] };
    // A corner where edge k starts: the tangents either side differ by more than the turn.
    let corner: Vec<bool> = (0..n)
        .map(|k| {
            let (Some(a), Some(b)) = (edges.get((k + n - 1) % n), edges.get(k)) else { return true };
            match (a.tangent(a.domain()[1]), b.tangent(b.domain()[0])) {
                (Some(x), Some(y)) => x.dot(y) < corner_turn.cos(),
                _ => true,
            }
        })
        .collect();
    let corners: Vec<usize> = (0..n).filter(|k| corner.get(*k).copied().unwrap_or(true)).collect();
    if corners.len() < 3 {
        return (0..n).map(|k| vec![whole(k)]).collect();
    }
    // Each edge's share of its side: its control polygon's length (any positive measure keeps the
    // side exact; this one keeps its speed near the edges' own).
    let length = |k: usize| edges.get(k).map_or(0.0, |e| e.points.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).max(f64::MIN_POSITIVE);
    let mut sides = Vec::with_capacity(corners.len());
    for (c, &first) in corners.iter().enumerate() {
        let next = corners.get((c + 1) % corners.len()).copied().unwrap_or(first);
        let count = (next + n - first - 1) % n + 1;
        let chain: Vec<usize> = (0..count).map(|i| (first + i) % n).collect();
        let total: f64 = chain.iter().map(|k| length(*k)).sum();
        let mut from = 0.0;
        let mut side = Vec::with_capacity(chain.len());
        for (i, &k) in chain.iter().enumerate() {
            let to = if i + 1 == chain.len() { 1.0 } else { from + length(k) / total };
            side.push(SidePiece { at: [from, to], ..whole(k) });
            from = to;
        }
        sides.push(side);
    }
    sides
}

/// A side as one curve on `[0, 1]` (module docs): its edges (homogeneous points scaled by
/// `scales`) each carried exactly into the side's degree and laid on its stretch, joined at
/// knots of full multiplicity.
fn side_curve(edges: &[IsoEdge], scales: &[f64], side: &[SidePiece]) -> SplineResult<Curve> {
    let mut parts = Vec::with_capacity(side.len());
    for sp in side {
        let e = edges.get(sp.edge).ok_or_else(|| SplineError::Invalid("a side's edge".into()))?;
        parts.push(on_unit(e, scales.get(sp.edge).copied().unwrap_or(1.0), sp.range, false)?);
    }
    let q = parts.iter().map(|c| c.space.degree()).max().unwrap_or(1);
    let mut knots = vec![0.0; q + 1];
    let mut points: Vec<HPoint> = Vec::new();
    for (i, (part, sp)) in parts.iter().zip(side).enumerate() {
        let (space, rows) = compatible(&[part], &[], q)?;
        let [a, b] = sp.at;
        let interior = space.knots().iter().skip(q + 1).take(space.count().saturating_sub(q + 1));
        knots.extend(interior.map(|x| a + (b - a) * x));
        let row = rows.into_iter().next().ok_or_else(|| SplineError::Invalid("a side's piece".into()))?;
        // The joint's point is the previous piece's last.
        points.extend(row.into_iter().skip(usize::from(i > 0)));
        knots.extend(std::iter::repeat_n(b, if i + 1 == side.len() { q + 1 } else { q }));
    }
    Ok(Curve { space: Space::new(q, knots)?, points })
}

/// A piece of a loop edge on a patch's side: `piece` (fraction `piece.at` of loop side `side_of`),
/// on patch `patch`'s side `side` over the fraction `along` of it.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Piece {
    piece: SidePiece,
    side_of: usize,
    patch: usize,
    side: usize,
    along: [f64; 2],
}

impl Piece {
    /// The edge's parameter at fraction `x` of the piece.
    fn param(&self, x: f64) -> f64 {
        let [a, b] = self.piece.range;
        if x == 0.0 {
            a
        } else if x == 1.0 {
            b
        } else {
            a + (b - a) * x
        }
    }
}

/// The edges' pieces on the loop side `side_of` between its fractions `lo` and `hi`, on patch
/// `patch`'s side `side`, the fraction `lo` of the loop side at the patch side's start and `hi`
/// at its end.
fn pieces_between(side: &[SidePiece], side_of: usize, [lo, hi]: [f64; 2], patch: usize, s: usize) -> Vec<Piece> {
    let mut out = Vec::new();
    for sp in side {
        let [g0, g1] = sp.at;
        let (a, b) = (g0.max(lo), g1.min(hi));
        if !(a < b) {
            continue;
        }
        let at = |g: f64| {
            if g == g0 {
                sp.range[0]
            } else if g == g1 {
                sp.range[1]
            } else {
                sp.range[0] + (sp.range[1] - sp.range[0]) * (g - g0) / (g1 - g0)
            }
        };
        let piece = SidePiece { edge: sp.edge, range: [at(a), at(b)], at: [a, b] };
        out.push(Piece { piece, side_of, patch, side: s, along: [(a - lo) / (hi - lo), (b - lo) / (hi - lo)] });
    }
    out
}

/// Where on the loop the fraction `g` of a side is: (edge, its parameter).
fn on_side(side: &[SidePiece], g: f64) -> Option<(usize, f64)> {
    let sp = side.iter().find(|sp| sp.at[0] <= g && g <= sp.at[1])?;
    let [g0, g1] = sp.at;
    let t = if g == g0 {
        sp.range[0]
    } else if g == g1 {
        sp.range[1]
    } else {
        sp.range[0] + (sp.range[1] - sp.range[0]) * (g - g0) / (g1 - g0)
    };
    Some((sp.edge, t))
}

/// A tensor patch in the fill's spaces along u and v, control point `(i, j)` (i along u) at
/// `i + c·j`, `c` the u space's count, with its fixed weights.
#[derive(Clone, Debug)]
struct Patch {
    spaces: [Space; 2],
    net: Vec<Vec3>,
    weights: Vec<f64>,
    /// Whether any weight differs from one (else the polynomial basis is used as it is).
    rational: bool,
    /// The patch collapses along `u = 0` (a three-sided patch).
    pole: bool,
}

impl Patch {
    /// The control points along u and along v.
    fn counts(&self) -> [usize; 2] {
        [self.spaces[0].count(), self.spaces[1].count()]
    }

    fn flat(&self, i: usize, j: usize) -> usize {
        i + self.spaces[0].count() * j
    }

    fn at(&self, i: usize, j: usize) -> Vec3 {
        self.net.get(self.flat(i, j)).copied().unwrap_or(Vec3::ZERO)
    }

    fn weight(&self, i: usize, j: usize) -> f64 {
        self.weights.get(self.flat(i, j)).copied().unwrap_or(1.0)
    }

    /// The coefficients of `∂^(du+dv) S / ∂u^du ∂v^dv` at (u, v) on each control point it
    /// depends on: `(i, j, coefficient)`. On a rational patch these are the derivatives of the
    /// rational basis `R_ij = w_ij N_i N_j / W`, by the Leibniz rule for `w_ij N_i N_j = R_ij W`:
    /// `R^(a,b) = (f^(a,b) − Σ_{(k,l) ≠ (0,0)} C(a,k) C(b,l) W^(k,l) R^(a−k,b−l)) / W`.
    fn row(&self, u: f64, v: f64, du: usize, dv: usize) -> SplineResult<Vec<(usize, usize, f64)>> {
        self.row_sided(u, v, du, dv, [false; 2])
    }

    /// The basis of the space along `d` at `t`, from the left of a knot when `left`.
    fn basis(&self, d: usize, t: f64, r: usize, left: bool) -> SplineResult<(usize, Vec<f64>)> {
        let space = self.spaces.get(d).ok_or_else(|| SplineError::Invalid("a patch has two directions".into()))?;
        if left { space.basis_left(t, r) } else { space.basis(t, r) }
    }

    /// [`Patch::row`] with each direction's basis taken from the left of a knot where `left`
    /// says so: the derivative of the piece ending there.
    fn row_sided(&self, u: f64, v: f64, du: usize, dv: usize, left: [bool; 2]) -> SplineResult<Vec<(usize, usize, f64)>> {
        if !self.rational {
            let (iu, bu) = self.basis(0, u, du, left[0])?;
            let (iv, bv) = self.basis(1, v, dv, left[1])?;
            let mut out = Vec::with_capacity(bu.len() * bv.len());
            for (b, nv) in bv.iter().enumerate() {
                for (a, nu) in bu.iter().enumerate() {
                    out.push((iu + a, iv + b, nu * nv));
                }
            }
            return Ok(out);
        }
        // The basis and its derivatives up to the orders asked, each way.
        let ders = |d: usize, t: f64, top: usize| -> SplineResult<(usize, Vec<Vec<f64>>)> {
            let mut first = 0;
            let mut out = Vec::with_capacity(top + 1);
            for r in 0..=top {
                let (f, b) = self.basis(d, t, r, left.get(d).copied().unwrap_or(false))?;
                first = f;
                out.push(b);
            }
            Ok((first, out))
        };
        let (iu, bu) = ders(0, u, du)?;
        let (iv, bv) = ders(1, v, dv)?;
        let width = (bu.first().map_or(0, Vec::len), bv.first().map_or(0, Vec::len));
        let basis = |d: &[Vec<f64>], r: usize, a: usize| d.get(r).and_then(|x| x.get(a)).copied().unwrap_or(0.0);
        // W^(k,l) at the point.
        let mut wd = vec![vec![0.0; dv + 1]; du + 1];
        for b in 0..width.1 {
            for a in 0..width.0 {
                let w = self.weight(iu + a, iv + b);
                for (k, row) in wd.iter_mut().enumerate() {
                    for (l, x) in row.iter_mut().enumerate() {
                        *x += w * basis(&bu, k, a) * basis(&bv, l, b);
                    }
                }
            }
        }
        let w0 = wd.first().and_then(|r| r.first()).copied().unwrap_or(0.0);
        if !(w0 > 0.0) {
            return invalid("a patch's weights vanish");
        }
        let binom = |n: usize, k: usize| (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64);
        let mut out = Vec::with_capacity(width.0 * width.1);
        let mut r = vec![vec![0.0; dv + 1]; du + 1];
        for b in 0..width.1 {
            for a in 0..width.0 {
                let w = self.weight(iu + a, iv + b);
                for total in 0..=du + dv {
                    for k in 0..=du.min(total) {
                        let l = total - k;
                        if l > dv {
                            continue;
                        }
                        let mut x = w * basis(&bu, k, a) * basis(&bv, l, b);
                        for i in 0..=k {
                            for j in 0..=l {
                                if i == 0 && j == 0 {
                                    continue;
                                }
                                let wij = wd.get(i).and_then(|x| x.get(j)).copied().unwrap_or(0.0);
                                let rr = r.get(k - i).and_then(|x| x.get(l - j)).copied().unwrap_or(0.0);
                                x -= binom(k, i) * binom(l, j) * wij * rr;
                            }
                        }
                        if let Some(slot) = r.get_mut(k).and_then(|x| x.get_mut(l)) {
                            *slot = x / w0;
                        }
                    }
                }
                out.push((iu + a, iv + b, r.get(du).and_then(|x| x.get(dv)).copied().unwrap_or(0.0)));
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
        let [cu, cv] = self.counts();
        let ctrl: Vec<HPoint> =
            (0..cv).flat_map(|j| (0..cu).map(move |i| (i, j))).map(|(i, j)| HPoint::weighted(self.at(i, j), self.weight(i, j))).collect();
        let [su, sv] = &self.spaces;
        NurbsSurface::from_homogeneous([su.degree(), sv.degree()], [su.knots().to_vec(), sv.knots().to_vec()], ctrl)
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
    /// The loop edges' pieces on the patches' sides.
    pieces: Vec<Piece>,
    /// Spokes: (patch on whose `u = 1` it runs, patch on whose `v = 1` it runs).
    spokes: Vec<(usize, usize)>,
    /// Where each spoke starts on the loop: (edge, parameter).
    feet: Vec<(usize, f64)>,
}

/// The constants each edge's homogeneous points are scaled by so that the weights at every
/// corner of the loop agree (module docs); refused when no such constants exist.
fn corner_scales(edges: &[IsoEdge]) -> SplineResult<Vec<f64>> {
    let ends = |e: &IsoEdge| {
        let w = e.weights.as_ref();
        (w.and_then(|w| w.first()).copied().unwrap_or(1.0), w.and_then(|w| w.last()).copied().unwrap_or(1.0))
    };
    let mut scales = Vec::with_capacity(edges.len());
    let mut c = 1.0 / edges.first().map_or(1.0, |e| ends(e).0);
    for (k, e) in edges.iter().enumerate() {
        if k > 0 {
            let (_, before) = edges.get(k - 1).map(ends).unwrap_or((1.0, 1.0));
            c *= before / ends(e).0;
        }
        scales.push(c);
    }
    let closing = edges.last().map_or(1.0, |e| ends(e).1) * scales.last().copied().unwrap_or(1.0);
    if !(closing.is_finite() && (closing - 1.0).abs() <= WEIGHT_TOL) {
        return Err(SplineError::Unsupported(
            "a patch on rational edges whose weights at the loop's corners cannot be made to agree (their end-to-start ratios round the loop multiply to more or less than one)".into(),
        ));
    }
    Ok(scales)
}

impl Quilt {
    /// The fill's patches over the loop's sides at degree `degree` (at least) in spaces refined
    /// by `extra` breaks (along u and v). One patch has a space each way, each holding the two
    /// sides that run that way; about a centre, the space along each spoke is the two patches'
    /// either side of it (one's along v, the other's along u), holding the spoke and the two edge
    /// halves it meets those patches beside.
    fn new(edges: &[IsoEdge], loop_sides: &[Vec<SidePiece>], extra: &[Vec<f64>; 2], degree: usize) -> SplineResult<Quilt> {
        let n = loop_sides.len();
        let scales = corner_scales(edges)?;
        let whole: Vec<Curve> = loop_sides.iter().map(|side| side_curve(edges, &scales, side)).collect::<SplineResult<_>>()?;
        let part = |k: usize, range: [f64; 2], reverse: bool| -> SplineResult<Curve> {
            let c = whole.get(k).ok_or_else(|| SplineError::Invalid("sides".into()))?;
            piece_on_unit(c.space.degree(), c.space.knots(), &c.points, range, reverse)
        };
        if n <= 4 {
            // One patch: the loop's sides are its sides (side 2 and 3 run back along their rows).
            let mut sides: Vec<Curve> = (0..n).map(|k| part(k, [0.0, 1.0], k >= 2)).collect::<SplineResult<_>>()?;
            if n == 3 {
                let pole = whole.first().and_then(|c| c.points.first().copied()).unwrap_or(HPoint::ZERO);
                sides.push(line(pole, pole)?);
            }
            let (Some(s0), Some(s1), Some(s2), Some(s3)) = (sides.first(), sides.get(1), sides.get(2), sides.get(3)) else { return invalid("sides") };
            let (su, along_u) = compatible(&[s0, s2], &extra[0], degree)?;
            let (sv, along_v) = compatible(&[s3, s1], &extra[1], degree)?;
            let (Some(c0), Some(c2), Some(c3), Some(c1)) = (along_u.first(), along_u.get(1), along_v.first(), along_v.get(1)) else {
                return invalid("sides");
            };
            let patch = coons([su, sv], c0, c2, c3, c1, n == 3)?;
            // The free points numbered with the shorter direction fastest, which keeps the
            // normal equations' band narrow.
            let [cu, cv] = patch.counts();
            let mut slots = vec![None; cu * cv];
            let (inner_u, inner_v) = (cu.saturating_sub(2), cv.saturating_sub(2));
            for j in 1..cv - 1 {
                for i in 1..cu - 1 {
                    let id = if inner_u <= inner_v { (i - 1) + inner_u * (j - 1) } else { (j - 1) + inner_v * (i - 1) };
                    if let Some(s) = slots.get_mut(i + cu * j) {
                        *s = Some(id);
                    }
                }
            }
            let unknowns = inner_u * inner_v;
            let pieces = loop_sides.iter().enumerate().flat_map(|(k, side)| pieces_between(side, k, [0.0, 1.0], 0, k)).collect();
            return Ok(Quilt { patches: vec![patch], slots: vec![slots], unknowns, pieces, spokes: Vec::new(), feet: Vec::new() });
        }
        // N patches about the centre: halves of each side, and straight spokes to start.
        let middle: Vec<HPoint> = whole.iter().map(|c| c.space.eval(&c.points, 0.5, 0)).collect::<SplineResult<_>>()?;
        let centre = middle.iter().filter_map(|h| h.project()).fold(Vec3::ZERO, |s, p| s + p) * (1.0 / n as f64);
        let mut curves: Vec<Curve> = Vec::with_capacity(3 * n);
        for k in 0..n {
            curves.push(part(k, [0.0, 0.5], false)?);
            // The second half runs up u = 0 of the next patch: from V (v = 0) back to M.
            curves.push(part(k, [0.5, 1.0], true)?);
        }
        for m in &middle {
            curves.push(line(*m, HPoint::weighted(centre, 1.0))?);
        }
        // Space k runs along spoke k: it is patch k's along v (its sides edge k − 1's second half
        // and spoke k) and patch k + 1's along u (edge k + 1's first half and spoke k), so it
        // holds those three curves' breaks only, not every edge's.
        let mut spaces: Vec<Space> = Vec::with_capacity(n);
        // Each curve carried into the space it runs in: [first half, second half] per edge, and
        // each spoke.
        let mut first_half: Vec<Option<Vec<HPoint>>> = vec![None; n];
        let mut second_half: Vec<Option<Vec<HPoint>>> = vec![None; n];
        let mut spoke_rows: Vec<Vec<HPoint>> = Vec::with_capacity(n);
        for k in 0..n {
            let (prev, next) = ((k + n - 1) % n, (k + 1) % n);
            let refs = [
                curves.get(2 * prev + 1).ok_or_else(|| SplineError::Invalid("curves".into()))?,
                curves.get(2 * next).ok_or_else(|| SplineError::Invalid("curves".into()))?,
                curves.get(2 * n + k).ok_or_else(|| SplineError::Invalid("curves".into()))?,
            ];
            let (space, mut rows) = compatible(&refs, &extra[0], degree)?;
            let spoke = rows.pop().ok_or_else(|| SplineError::Invalid("rows".into()))?;
            let first = rows.pop().ok_or_else(|| SplineError::Invalid("rows".into()))?;
            let second = rows.pop().ok_or_else(|| SplineError::Invalid("rows".into()))?;
            second_half[prev] = Some(second);
            first_half[next] = Some(first);
            spoke_rows.push(spoke);
            spaces.push(space);
        }
        let got = |v: &[Option<Vec<HPoint>>], k: usize| v.get(k).cloned().flatten().ok_or_else(|| SplineError::Invalid("rows".into()));
        let space = |k: usize| spaces.get(k).cloned().ok_or_else(|| SplineError::Invalid("spaces".into()));
        let spoke_row = |k: usize| spoke_rows.get(k).cloned().ok_or_else(|| SplineError::Invalid("rows".into()));
        let mut patches = Vec::with_capacity(n);
        for k in 0..n {
            let prev = (k + n - 1) % n;
            // Along u, space k − 1: v = 0 edge k's first half, v = 1 spoke k − 1 (from M_{k−1} to
            // C); along v, space k: u = 0 edge k − 1's second half (from V_k up to M_{k−1}),
            // u = 1 spoke k.
            patches.push(coons(
                [space(prev)?, space(k)?],
                &got(&first_half, k)?,
                &spoke_row(prev)?,
                &got(&second_half, prev)?,
                &spoke_row(k)?,
                false,
            )?);
        }
        // The spokes are shared rows: their weights the same in both patches (the Coons blends
        // agree on them), and so are their points once solved.
        // Unknowns: each spoke's points past its midpoint, the centre once, each patch's
        // interior.
        let mut unknowns = 0usize;
        let mut next = || {
            unknowns += 1;
            unknowns - 1
        };
        let centre_id = next();
        let spoke_ids: Vec<Vec<usize>> = spaces.iter().map(|sp| (1..sp.count().saturating_sub(1)).map(|_| next()).collect()).collect();
        let mut slots = Vec::with_capacity(n);
        for k in 0..n {
            let prev = (k + n - 1) % n;
            let [cu, cv] = patches.get(k).map(Patch::counts).ok_or_else(|| SplineError::Invalid("patch".into()))?;
            let mut s = vec![None; cu * cv];
            for j in 1..cv {
                for i in 1..cu {
                    let id = match (i == cu - 1, j == cv - 1) {
                        (true, true) => centre_id,
                        (true, false) => *spoke_ids.get(k).and_then(|x| x.get(j - 1)).ok_or_else(|| SplineError::Invalid("spoke".into()))?,
                        (false, true) => *spoke_ids.get(prev).and_then(|x| x.get(i - 1)).ok_or_else(|| SplineError::Invalid("spoke".into()))?,
                        (false, false) => next(),
                    };
                    if let Some(slot) = s.get_mut(i + cu * j) {
                        *slot = Some(id);
                    }
                }
            }
            slots.push(s);
        }
        let mut pieces = Vec::with_capacity(2 * n);
        let mut feet = Vec::with_capacity(n);
        for (k, side) in loop_sides.iter().enumerate() {
            pieces.extend(pieces_between(side, k, [0.0, 0.5], k, 0));
            pieces.extend(pieces_between(side, k, [0.5, 1.0], (k + 1) % n, 3));
            feet.push(on_side(side, 0.5).ok_or_else(|| SplineError::Invalid("a side's middle".into()))?);
        }
        let spokes = (0..n).map(|k| (k, (k + 1) % n)).collect();
        Ok(Quilt { patches, slots, unknowns, pieces, spokes, feet })
    }

    /// Most free control points one solve takes, one patch or several about a centre.
    fn most_unknowns(shared: bool) -> usize {
        let _ = shared;
        MAX_BANDED_POINTS
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

/// The bilinearly blended Coons patch of homogeneous sides: `c0`, `c1` along u at v = 0 and 1,
/// `d0`, `d1` along v at u = 0 and 1, all running with their parameter. Points and weights are
/// blended alike, so the boundary rows are the sides' own and the weights inside are the blend
/// of theirs (refused where one is not positive).
fn coons(spaces: [Space; 2], c0: &[HPoint], c1: &[HPoint], d0: &[HPoint], d1: &[HPoint], pole: bool) -> SplineResult<Patch> {
    let (gu, gv) = (greville(&spaces[0]), greville(&spaces[1]));
    let (cu, cv) = (gu.len(), gv.len());
    let (Some(p00), Some(p10), Some(p01), Some(p11)) = (c0.first().copied(), c0.last().copied(), c1.first().copied(), c1.last().copied()) else {
        return invalid("a side has no points");
    };
    let get = |a: &[HPoint], k: usize| a.get(k).copied().unwrap_or(HPoint::ZERO);
    let mut net = Vec::with_capacity(cu * cv);
    let mut weights = Vec::with_capacity(cu * cv);
    for (j, &y) in gv.iter().enumerate() {
        for (i, &x) in gu.iter().enumerate() {
            let ruled_v = get(c0, i) * (1.0 - y) + get(c1, i) * y;
            let ruled_u = get(d0, j) * (1.0 - x) + get(d1, j) * x;
            let bilinear = (p00 * (1.0 - x) + p10 * x) * (1.0 - y) + (p01 * (1.0 - x) + p11 * x) * y;
            // The boundary rows exactly the sides' own points.
            let h = match (i, j) {
                (_, 0) => get(c0, i),
                (_, j) if j + 1 == cv => get(c1, i),
                (0, _) => get(d0, j),
                (i, _) if i + 1 == cu => get(d1, j),
                _ => ruled_v + ruled_u - bilinear,
            };
            let p = h
                .project()
                .filter(|_| h.w > 0.0)
                .ok_or_else(|| SplineError::Unsupported("a rational patch whose blended weights are not positive".into()))?;
            net.push(p);
            weights.push(h.w);
        }
    }
    let rational = weights.iter().any(|w| *w != 1.0);
    Ok(Patch { spaces, net, weights, rational, pole })
}

/// One condition row: `Σ along·P = value` over its terms' control points, and its weight.
#[derive(Clone)]
struct Row {
    terms: Vec<Term>,
    value: f64,
    weight: f64,
}

/// A control point `(i, j)` of patch `patch` in a row, its coefficient a vector (dotted with
/// the point).
#[derive(Clone, Copy)]
struct Term {
    patch: usize,
    i: usize,
    j: usize,
    along: Vec3,
}

/// The terms of a patch's derivative coefficients `(i, j, c)` taken along `dir`.
fn along(patch: usize, coefficients: &[(usize, usize, f64)], dir: Vec3) -> Vec<Term> {
    coefficients.iter().map(|&(i, j, c)| Term { patch, i, j, along: dir * c }).collect()
}

/// The three rows `Σ terms·P = value` on each coordinate.
fn on_each_axis(rows: &mut Vec<Row>, patch_terms: &[(usize, Vec<(usize, usize, f64)>)], value: Vec3, weight: f64) {
    for (axis, v) in [(Vec3::X, value.x), (Vec3::Y, value.y), (Vec3::Z, value.z)] {
        let terms = patch_terms.iter().flat_map(|(p, c)| along(*p, c, axis)).collect();
        rows.push(Row { terms, value: v, weight });
    }
}

/// A station of a loop edge: its patch, side and fraction there, its condition, whether it is
/// held to the angle tolerance, whether it is clear of the loop's corners (its curvature and
/// curvature change are reported), and how fast the fill should leave there.
struct Station {
    patch: usize,
    side: usize,
    t: f64,
    across: Across,
    /// The continuity order asked here: the neighbour's, or less near a corner whose neighbours
    /// do not agree to it ([`Asked`]): orders up to `full` at their full weight, those above it
    /// up to `order` at `fade` of it.
    order: usize,
    full: usize,
    fade: f64,
    /// How far (radians) the edge itself leans out of the neighbour's tangent plane here: the
    /// fill holds the edge, so its normal can come no nearer the neighbour's than that.
    tilt: f64,
    held: bool,
    clear: bool,
    /// The weak rows' target for the cross derivative `c′` ([`stations`]): `w·c′ = speed`,
    /// `t·c′ = slide`.
    speed: f64,
    slide: f64,
}

impl Station {
    /// The continuity order its conditions ask: 1 (G1) … 4 (G4).
    fn order(&self) -> usize {
        self.order
    }
}

/// What a station asks, by how near it is to corners whose neighbours agree to less (see
/// [`corner_orders`]): within [`CLASH_SHARE`] of such a corner the orders above the corner's
/// agreement (at least G1) are not asked, and over the next [`CLASH_SHARE`] they come back
/// smoothly (`fade` from 0 to 1, a smoothstep), so the fill is not strained where they start;
/// where the corner's tangent planes disagree the station is not held to the angle tolerance.
struct Asked {
    asked: usize,
    full: usize,
    fade: f64,
    held: bool,
}

impl Asked {
    /// For a station asking `asked`, at fraction `distance` of its side from each of the side's
    /// two corners, whose neighbours agree to `agreement` (`None`: no cap).
    fn near(asked: usize, ends: [(Option<usize>, f64); 2]) -> Asked {
        let mut a = Asked { asked, full: asked, fade: 1.0, held: true };
        for (agreement, distance) in ends {
            let Some(agreed) = agreement.filter(|g| *g < asked) else { continue };
            if distance >= 2.0 * CLASH_SHARE {
                continue;
            }
            a.full = a.full.min(agreed.max(1));
            let x = ((distance - CLASH_SHARE) / CLASH_SHARE).clamp(0.0, 1.0);
            a.fade = a.fade.min(x * x * (3.0 - 2.0 * x));
            if agreed == 0 && distance < CLASH_SHARE {
                a.held = false;
            }
        }
        a
    }

    /// The highest order asked at all.
    fn order(&self) -> usize {
        if self.fade > 0.0 { self.asked } else { self.full }
    }
}

/// The derivatives `[c′, c″, …, c⁽ⁿ⁾]` (n = `order`) of the linkage curve `c` leaving a station
/// across its edge into the patch: the iso-parameter curve, σ growing into the patch.
fn linkage(patch: &Patch, s: &Station, order: usize) -> SplineResult<Vec<Vec3>> {
    let (uv, (du, dv, sign)) = station_on(s.side, s.t);
    (1..=order).map(|k| Ok(patch.eval(uv[0], uv[1], k * du, k * dv)? * sign.powi(k as i32))).collect()
}

/// What orders 2 … n ask of `n·c⁽ᵏ⁾` at a station ([`crate::linkage::Jet::targets`]), read
/// from the linkage curve's last derivatives `c` (element k − 2 is order k's); empty for G1.
fn targets(s: &Station, c: &[Vec3]) -> Vec<f64> {
    let Some(jet) = s.across.jet else { return Vec::new() };
    let (t, w) = (s.across.normal.cross(s.across.leave), s.across.leave);
    let x: Vec<[f64; 2]> = c.iter().map(|d| [d.dot(t), d.dot(w)]).collect();
    let mut out = jet.targets(&x).unwrap_or_default();
    out.truncate(s.order().saturating_sub(1));
    out
}

/// To what order the neighbours agree at each corner of the region (`[k]`: where side `k`
/// starts), from their conditions at its two edges' ends ([`corner_agreement`]); no cap
/// (`usize::MAX`) where either edge asks only G0.
fn corner_orders(
    edges: &[IsoEdge],
    sides: &[Vec<SidePiece>],
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
    size: f64,
    angle_tol: f64,
) -> SplineResult<Vec<usize>> {
    let n = sides.len();
    let mut out = vec![usize::MAX; n];
    for (k, slot) in out.iter_mut().enumerate() {
        // The corner where side k starts: the end of the side before's last edge, the start of
        // its own first.
        let (Some(last), Some(first)) = (sides.get((k + n - 1) % n).and_then(|s| s.last()), sides.get(k).and_then(|s| s.first())) else { continue };
        let (prev, k) = (last.edge, first.edge);
        let (Some(ea), Some(eb)) = (edges.get(prev), edges.get(k)) else { continue };
        let (end, start) = (last.range[1], first.range[0]);
        let before = across(prev, &[end]).map_err(SplineError::Invalid)?.into_iter().next().flatten();
        let after = across(k, &[start]).map_err(SplineError::Invalid)?.into_iter().next().flatten();
        if let (Some(x), Some(y), Some(d0), Some(d1)) = (before, after, ea.tangent(end), eb.tangent(start)) {
            *slot = corner_agreement(&x, &y, [d0, d1], size, angle_tol);
        }
    }
    Ok(out)
}

/// To what order the neighbours `before` and `after` the loop's corner agree there (where no
/// surface can meet both with more): 0 when no surface can be within `angle_tol` of both
/// tangent planes there, or (with both neighbours' jets) their twists between the two edges
/// differ (at a corner the fill's normal is the loop's own, `d₀ × d₁` of the
/// edges' tangents `directions`, so each neighbour's must be within the tolerance of it; at a
/// corner where the loop runs straight on, the two must be within twice the tolerance of each
/// other); else the highest order k up to both jets' such that their forms `H_2 … H_k` agree
/// along the two tangents once carried over half the loop's `size` ([`turn`]), as the fill's
/// own jumps are held. A corner whose neighbours agree to the order asked caps nothing.
pub fn corner_agreement(before: &Across, after: &Across, directions: [Vec3; 2], size: f64, angle_tol: f64) -> usize {
    let cos = before.normal.dot(after.normal);
    let tangent = match directions[0].cross(directions[1]).normalized() {
        Some(corner) if directions[0].dot(directions[1]) < (2.0 * angle_tol).cos() => {
            [before.normal, after.normal].iter().all(|n| n.dot(corner).abs() >= angle_tol.cos())
        }
        _ => cos.abs() >= (2.0 * angle_tol).cos(),
    };
    if !tangent {
        return 0;
    }
    let (Some(a), Some(b)) = (before.jet, after.jet) else { return 1 };
    // Heights along each one's own normal; along the other's they change sign with the order.
    let sign = cos.signum();
    let form = |x: &Across, jet: &crate::linkage::Jet, m: usize, d: Vec3| {
        let (t, w) = (x.normal.cross(x.leave), x.leave);
        jet.form(m, [d.dot(t), d.dot(w)])
    };
    // G1 along both edges fixes the fill's twist `n·S_uv` at the corner twice, from each
    // neighbour's second form between the two edges' directions, `H₂(d₀, d₁)`
    // (`(H₂(d₀ + d₁) − H₂(d₀ − d₁)) / 4`): where those differ, not even G1 holds there.
    let [d0, d1] = directions;
    let twist = |x: &Across, jet: &crate::linkage::Jet| Some(0.25 * (form(x, jet, 2, d0 + d1)? - form(x, jet, 2, d0 - d1)?));
    match (twist(before, &a), twist(after, &b)) {
        (Some(x), Some(y)) if turn((x - sign * y).abs(), 2, size) <= angle_tol => {}
        _ => return 0,
    }
    let mut agreed = 1;
    for m in 2..=a.order.min(b.order) {
        let same = directions.iter().all(|d| match (form(before, &a, m, *d), form(after, &b, m, *d)) {
            (Some(x), Some(y)) => turn((x - sign * y).abs(), m, size) <= angle_tol,
            _ => false,
        });
        if !same {
            return agreed;
        }
        agreed = m;
    }
    agreed
}

/// Fill the loop `edges` (three to [`MAX_ISO_EDGES`], each ending where the next starts) with
/// patches on them (module docs). `across(k, ts)` gives the neighbour's conditions at the
/// parameters `ts` of edge `k` (its own parameter; `None`: G0 there). Where the neighbours at a
/// corner agree to a lower order than asked ([`corner_orders`]), the stations within
/// [`CLASH_SHARE`] of it ask no more than that (at least G1), and where their tangent planes
/// disagree they are not held to the angle tolerance either: no surface can meet both better.
pub fn iso_fill(
    edges: &[IsoEdge],
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
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
        if let Some(w) = &e.weights
            && (w.len() != e.points.len() || w.iter().any(|x| !(x.is_finite() && *x > 0.0)))
        {
            return invalid(format!("edge {k}'s weights must be positive, one per control point"));
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
    // The highest continuity asked anywhere, from the conditions at each edge's middle (an
    // edge's continuity is one along it).
    let mut order = 0usize;
    for k in 0..n {
        let Some(e) = edges.get(k) else { continue };
        let found = across(k, &[e.middle()]).map_err(SplineError::Invalid)?;
        for c in found.into_iter().flatten() {
            order = order.max(c.jet.map_or(1, |j| j.order));
        }
    }
    if order > MAX_CONTINUITY {
        return invalid(format!("a patch's continuity is at most G{MAX_CONTINUITY}"));
    }
    let degree = if order == 0 { 1 } else { (order + 1).min(MAX_SPLINE_DEGREE) };
    if !(spec.corner_turn.is_finite() && spec.corner_turn >= 0.0) {
        return invalid("the corner turn must be a finite angle");
    }
    let sides = loop_sides(edges, spec.corner_turn);
    let corners = corner_orders(edges, &sides, &mut *across, size, spec.angle_tol)?;
    let mut extra: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    let mut spans = [1usize; 2];
    loop {
        let quilt = Quilt::new(edges, &sides, &extra, degree)?;
        let stations = stations(edges, &quilt, &mut *across, &corners)?;
        let spaces = quilt.patches.first().map(|p| p.spaces.clone()).ok_or_else(|| SplineError::Invalid("no patch".into()))?;
        let shared = !quilt.spokes.is_empty();
        // Rows enough for the conditions from opposite sides not to share control points, in
        // every patch.
        let fewest = |d: usize| quilt.patches.iter().filter_map(|p| p.spaces.get(d).map(Space::count)).min().unwrap_or(0);
        let cramped = [fewest(0), fewest(1)].map(|c| c < 2 * (order + 1));
        if cramped.iter().any(|c| *c) && refinable(&spans, cramped, shared, quilt.unknowns) {
            halve(&mut extra, &mut spans, &spaces, cramped, shared);
            continue;
        }
        let (quilt, report, along) = solve(quilt, &stations, spec)?;
        let folded = quilt.patches.iter().any(folds);
        let mut wanted = along.missing(order, size, spec.angle_tol);
        if folded || report.point_deviation > spec.tol {
            wanted = [true; 2];
        }
        // The tangent plane, the points and an unfolded fill are what a patch must have; the
        // orders above G1 are held as tightly as the finest space allows, and reported.
        let must = !folded && report.point_deviation <= spec.tol && along.angle.iter().all(|a| *a <= spec.angle_tol);
        let refine = refinable(&spans, wanted, shared, quilt.unknowns);
        if !wanted.iter().any(|w| *w) || (must && !refine) {
            let quilt = crowned(quilt, &stations, spec.crown, size)?;
            return finished(quilt, report);
        }
        if !refine {
            let jumps =
                report.jumps.get(2..=order).filter(|_| order >= 2).map(|j| format!(", jumps of orders 2 to {order} {j:?}")).unwrap_or_default();
            let folded = if folded { ", and it folds" } else { "" };
            return Err(SplineError::Unsupported(format!(
                "a patch on these edges within the tolerances: normals within {:.4}° of the neighbours' beyond the edges' own lean{jumps}, points within {:.2e}{folded}",
                report.normal_excess.to_degrees(),
                report.point_deviation
            )));
        }
        halve(&mut extra, &mut spans, &spaces, wanted, shared);
    }
}

/// Whether the spans may be halved in the `wanted` directions (both when the patches share one
/// space, about a centre): within [`MAX_ISO_SPANS`] each way, and the free points, doubling for
/// each direction, within what one solve takes ([`Quilt::most_unknowns`]).
fn refinable(spans: &[usize; 2], wanted: [bool; 2], shared: bool, unknowns: usize) -> bool {
    let wanted = if shared { [true; 2] } else { wanted };
    let grows = wanted.iter().filter(|w| **w).count() as u32;
    let spans_ok = spans.iter().zip(wanted).all(|(s, w)| !w || s * 2 <= MAX_ISO_SPANS);
    spans_ok && unknowns.saturating_mul(2usize.pow(grows)) <= Quilt::most_unknowns(shared)
}

/// Halve every span of the spaces in the `wanted` directions: the breaks that do it added to
/// `extra`. About a centre, every spoke's space takes the middles of a uniform grid as fine as
/// the refinement has reached.
fn halve(extra: &mut [Vec<f64>; 2], spans: &mut [usize; 2], spaces: &[Space; 2], wanted: [bool; 2], shared: bool) {
    if shared {
        // Each spoke's space has the breaks of its own sides: every one of them gets the same
        // further breaks, the middles of the uniform grid at the level reached so far.
        let n = spans[0];
        extra[0].extend((0..n).map(|i| (2 * i + 1) as f64 / (2 * n) as f64));
        spans[0] *= 2;
        spans[1] *= 2;
        return;
    }
    for (((e, n), s), w) in extra.iter_mut().zip(spans.iter_mut()).zip(spaces).zip(wanted) {
        if w {
            e.extend(breaks(s).windows(2).map(|w| 0.5 * (w[0] + w[1])));
            *n *= 2;
        }
    }
}

/// Whether every jump of order 2 … `order` is within the angle tolerance once carried into the
/// fill: by Taylor's theorem a jump `j` of order k (1/lengthᵏ⁻¹) turns the normal by
/// `j·dᵏ⁻¹/(k − 1)!` at a distance `d` from the edge, and no point of the fill is farther from
/// the loop than half its `size`. So each order is held as tightly as the tangent plane is.
pub fn within_order(jumps: &[f64; MAX_CONTINUITY + 1], order: usize, size: f64, angle_tol: f64) -> bool {
    (2..=order).all(|k| jumps.get(k).is_some_and(|j| turn(*j, k, size) <= angle_tol))
}

/// How far (radians) a jump `jump` of order `k` turns the normal over half of `size`
/// ([`within_order`]).
pub fn turn(jump: f64, k: usize, size: f64) -> f64 {
    let factorial: f64 = (1..k).map(|i| i as f64).product();
    jump * (0.5 * size).powi(k as i32 - 1) / factorial
}

/// The patches, their sides and the spokes.
fn finished(quilt: Quilt, achieved: Achieved) -> SplineResult<IsoFill> {
    let n = quilt.patches.len();
    let mut patches = Vec::with_capacity(n);
    for (k, patch) in quilt.patches.iter().enumerate() {
        // Each side with where it starts round the patch: its side of the square, and how far
        // along it.
        let mut sides: Vec<((usize, f64), IsoSide)> = Vec::with_capacity(4);
        for pc in quilt.pieces.iter().filter(|pc| pc.patch == k) {
            let [a, b] = side_uv(pc.side);
            let lerp = |x: f64| [a[0] + (b[0] - a[0]) * x, a[1] + (b[1] - a[1]) * x];
            let [from, to] = pc.piece.range;
            sides.push(((pc.side, pc.along[0]), IsoSide::Edge { edge: pc.piece.edge, from, to, uv: [lerp(pc.along[0]), lerp(pc.along[1])] }));
        }
        if !quilt.spokes.is_empty() {
            // Spoke k up u = 1 (from its midpoint to the centre), spoke k − 1 back along v = 1.
            sides.push(((1, 0.0), IsoSide::Spoke { spoke: k, forward: true, uv: side_uv(1) }));
            sides.push(((2, 0.0), IsoSide::Spoke { spoke: (k + n - 1) % n, forward: false, uv: side_uv(2) }));
        }
        // The loop's order round the patch.
        sides.sort_by(|a, b| a.0.0.cmp(&b.0.0).then(a.0.1.total_cmp(&b.0.1)));
        patches.push(IsoPatch { surface: patch.surface()?, sides: sides.into_iter().map(|(_, s)| s).collect() });
    }
    let spokes = quilt
        .spokes
        .iter()
        .filter_map(|&(left, _)| {
            let p = quilt.patches.get(left)?;
            let [cu, cv] = p.counts();
            let weights = p.rational.then(|| (0..cv).map(|j| p.weight(cu - 1, j)).collect());
            let along = &p.spaces[1];
            Some(IsoEdge { degree: along.degree(), knots: along.knots().to_vec(), points: (0..cv).map(|j| p.at(cu - 1, j)).collect(), weights })
        })
        .collect();
    Ok(IsoFill { patches, spokes, feet: quilt.feet.clone(), achieved })
}

/// Stations along a side: [`STATIONS_PER_SPAN`] a span and [`STATIONS_PER_POINT`] per control
/// point of a row (each order of continuity holds one row of them, so the stations must outnumber
/// a row's points well for the conditions to hold between them too), at least [`MIN_STATIONS`].
fn station_count(space: &Space) -> usize {
    (breaks(space).len().saturating_sub(1) * STATIONS_PER_SPAN).max(STATIONS_PER_POINT * space.count()).max(MIN_STATIONS)
}

/// The stations of every loop edge with a condition: enough of them for the space's spans.
fn stations(
    edges: &[IsoEdge],
    quilt: &Quilt,
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
    corners: &[usize],
) -> SplineResult<Vec<Station>> {
    let n = corners.len();
    let mut out = Vec::new();
    for pc in &quilt.pieces {
        let (k, patch, side) = (pc.piece.edge, pc.patch, pc.side);
        // The space the side runs in: u for sides 0 and 2, v for 1 and 3.
        let Some(space) = quilt.patches.get(patch).and_then(|p| p.spaces.get(side % 2)) else { continue };
        // As many as the side's space asks, for the piece's share of the side.
        let count = ((station_count(space) as f64) * (pc.along[1] - pc.along[0])).ceil().max(2.0) as usize;
        // Stations inside the piece (its corners are its neighbours' corners); every side runs
        // the loop's way round its patch, as the edge does.
        let xs: Vec<f64> = (0..count).map(|i| (i as f64 + 0.5) / count as f64).collect();
        let params: Vec<f64> = xs.iter().map(|x| pc.param(*x)).collect();
        let found = across(k, &params).map_err(SplineError::Invalid)?;
        let Some(p) = quilt.patches.get(patch) else { continue };
        for ((&x, &param), c) in xs.iter().zip(&params).zip(found) {
            let Some(c) = c else { continue };
            // Along the patch's side, and along the region's side (from its corner).
            let t = pc.along[0] + (pc.along[1] - pc.along[0]) * x;
            let f = pc.piece.at[0] + (pc.piece.at[1] - pc.piece.at[0]) * x;
            let asked = c.jet.map_or(1, |j| j.order);
            let ends = [(corners.get(pc.side_of).copied(), f), (corners.get((pc.side_of + 1) % n.max(1)).copied(), 1.0 - f)];
            let ask = Asked::near(asked, ends);
            let clear = f > CORNER_SHARE && f < 1.0 - CORNER_SHARE && ask.full == asked;
            let (uv, (du, dv, sign)) = station_on(side, t);
            // The Coons start's cross derivative turned about the edge into the neighbour's
            // tangent plane: its part along the edge kept, the rest laid along the leave
            // direction at its own length. At a corner it is the next side's tangent, which the
            // patch's cross derivative is there whatever its inside (module docs).
            let d = p.eval(uv[0], uv[1], du, dv)? * sign;
            let slide = d.dot(c.normal.cross(c.leave));
            let speed = (d.dot(d) - slide * slide).max(0.0).sqrt();
            let tilt = edges.get(k).and_then(|e| e.tangent(param)).map_or(0.0, |d| d.dot(c.normal).abs().clamp(0.0, 1.0).asin());
            out.push(Station {
                patch,
                side,
                t,
                across: c,
                order: ask.order(),
                full: ask.full,
                fade: ask.fade,
                tilt,
                held: ask.held,
                clear,
                speed,
                slide,
            });
        }
    }
    Ok(out)
}

/// The fill's free control points by least squares (module docs), and what they achieve.
fn solve(mut quilt: Quilt, stations: &[Station], spec: &IsoSpec) -> SplineResult<(Quilt, Achieved, Along)> {
    if quilt.unknowns == 0 {
        return Ok((quilt, Achieved::default(), Along::default()));
    }
    let most = Quilt::most_unknowns(!quilt.spokes.is_empty());
    if quilt.unknowns > most {
        return Err(SplineError::TooLarge(format!("{} free control points (at most {most})", quilt.unknowns)));
    }
    let fairing = stations.iter().map(Station::order).max().unwrap_or(1).max(1) + 1;
    let energy: Vec<Vec<(usize, usize, f64)>> = quilt.patches.iter().map(|p| energy(p, fairing)).collect::<SplineResult<_>>()?;
    let mut located: Vec<(usize, [f64; 2])> = Vec::new();
    let mut report = Achieved::default();
    let mut along = Along::default();
    let spoke_count = quilt.patches.iter().map(|p| station_count(&p.spaces[0])).max().unwrap_or(0);
    let top = stations.iter().map(Station::order).max().unwrap_or(1);
    // The mean span of each space of a patch.
    let span = |p: &Patch| p.spaces.clone().map(|s| 1.0 / breaks(&s).len().saturating_sub(1).max(1) as f64);
    let rounds = if top >= 2 { ROUNDS_PER_ORDER * top } else { ROUNDS };
    let smooth = knot_line_rows(&quilt, spec.angle_tol)?;
    for round in 0..rounds {
        let mut rows: Vec<Row> = smooth.clone();
        for s in stations {
            let Some(patch) = quilt.patches.get(s.patch) else { continue };
            // The span across the side: of v for sides 0 and 2, of u for 1 and 3.
            linkage_rows(&mut rows, patch, s, span(patch).get(1 - s.side % 2).copied().unwrap_or(1.0))?;
        }
        // Across each spoke the two patches' derivatives away from it are opposed, up to the
        // gluing function's share of the derivative along it (module docs).
        let gluing = 2.0 * (std::f64::consts::TAU / quilt.patches.len() as f64).cos();
        for &(left, right) in &quilt.spokes {
            let (Some(a), Some(b)) = (quilt.patches.get(left), quilt.patches.get(right)) else { continue };
            for i in 0..spoke_count {
                let t = (i as f64 + 0.5) / spoke_count as f64;
                let along: Vec<(usize, usize, f64)> = a.row(1.0, t, 0, 1)?.into_iter().map(|(i, j, c)| (i, j, -gluing * t * c)).collect();
                on_each_axis(&mut rows, &[(left, a.row(1.0, t, 1, 0)?), (right, b.row(t, 1.0, 0, 1)?), (left, along)], Vec3::ZERO, 1.0);
            }
        }
        for (p, (patch, uv)) in spec.points.iter().zip(&located) {
            let Some(on) = quilt.patches.get(*patch) else { continue };
            on_each_axis(&mut rows, &[(*patch, on.row(uv[0], uv[1], 0, 0)?)], *p, 1.0);
        }
        let solved = least_squares(&quilt, &rows, &energy)?;
        quilt.set(&solved);
        located = spec.points.iter().map(|p| nearest(&quilt, *p)).collect();
        (report, along) = measure(&quilt, stations, spec.points, &located, spoke_count)?;
        let relinearise = top >= 2 || !spec.points.is_empty();
        if !relinearise && round == 0 {
            break;
        }
    }
    Ok((quilt, report, along))
}

/// Rows that keep a patch tangent continuous across its own C0 knot lines (a break where a
/// side is only C0 in its parameter, such as a circle's quarter joint once in homogeneous form):
/// along the line `u = x` (or `v = x`), at stations, the derivative across it from the right is
/// the one from the left times `r`, the ratio the two sides crossing the line have there (they
/// are tangent continuous, their speeds may differ), blended linearly between them; at the sides
/// the rows hold by construction. A line where a crossing side turns a corner is left alone: no
/// surface is tangent across it there. A three-sided patch's pole takes the other side's ratio.
fn knot_line_rows(quilt: &Quilt, angle_tol: f64) -> SplineResult<Vec<Row>> {
    let mut rows = Vec::new();
    for (k, patch) in quilt.patches.iter().enumerate() {
        for d in 0..2 {
            let (Some(space), Some(other)) = (patch.spaces.get(d), patch.spaces.get(1 - d)) else { continue };
            let count = station_count(other);
            for (x, m) in interior_knots(space) {
                if m < space.degree() {
                    continue;
                }
                // The point (x, t) in the patch's (u, v), and its one-sided derivatives across.
                let at = |t: f64| if d == 0 { [x, t] } else { [t, x] };
                let across = |t: f64, left: bool| -> SplineResult<Vec<(usize, usize, f64)>> {
                    let [u, v] = at(t);
                    let (du, dv) = if d == 0 { (1, 0) } else { (0, 1) };
                    patch.row_sided(u, v, du, dv, if d == 0 { [left, false] } else { [false, left] })
                };
                let speed = |row: &[(usize, usize, f64)]| row.iter().fold(Vec3::ZERO, |s, (i, j, c)| s + patch.at(*i, *j) * *c);
                let ratio = |t: f64| -> SplineResult<Option<f64>> {
                    let (a, b) = (speed(&across(t, true)?), speed(&across(t, false)?));
                    let (la, lb) = (a.len(), b.len());
                    if !(la > 0.0 && lb > 0.0) {
                        return Ok(None);
                    }
                    Ok((a.dot(b) / (la * lb) >= angle_tol.cos()).then_some(lb / la))
                };
                let ends = [ratio(0.0)?, ratio(1.0)?];
                let (r0, r1) = match ends {
                    [Some(a), Some(b)] => (a, b),
                    [Some(a), None] | [None, Some(a)] if patch.pole && d == 1 => (a, a),
                    _ => continue,
                };
                for i in 0..count {
                    let t = (i as f64 + 0.5) / count as f64;
                    let r = r0 * (1.0 - t) + r1 * t;
                    let mut terms = across(t, false)?;
                    terms.extend(across(t, true)?.into_iter().map(|(i, j, c)| (i, j, -r * c)));
                    on_each_axis(&mut rows, &[(k, terms)], Vec3::ZERO, 1.0);
                }
            }
        }
    }
    Ok(rows)
}

/// A station's rows (module docs), linearised about the patch as it stands:
/// - G1: `n·c′ = 0`, and the weak Hermite rows `w·c′ = s`, `t·c′ = a` in the neighbour's tangent
///   plane only (`t = n × w`), so they never pull against the G1 row;
/// - orders k = 2 … n: `n·c⁽ᵏ⁾ = T_k(x₁ … x_{k−1})` ([`crate::linkage::Jet::linearised`]) by
///   Newton's method: `T_k` is replaced by its tangent at the last solution's `x⁰`, so the row is
///   `n·c⁽ᵏ⁾ − Σ_j ∇_j T_k · x_j = T_k(x⁰) − Σ_j ∇_j T_k · x_j⁰`, with `x_j = (t·c⁽ʲ⁾, w·c⁽ʲ⁾)`
///   and `c⁽ʲ⁾ = signʲ ∂ʲS` across. Each is weighed by `hᵏ⁻¹` against the G1 row (`h` the span),
///   so every order says how far it misses over one span and none swamps the others as the
///   spans shrink.
fn linkage_rows(rows: &mut Vec<Row>, patch: &Patch, s: &Station, span: f64) -> SplineResult<()> {
    let (uv, (du, dv, sign)) = station_on(s.side, s.t);
    let (n, w) = (s.across.normal, s.across.leave);
    let t = n.cross(w);
    let order = s.order();
    // The coefficients of c′ … c⁽ⁿ⁾ on the control points.
    let derivative: Vec<Vec<(usize, usize, f64)>> = (1..=order)
        .map(|k| {
            let sign_k = sign.powi(k as i32);
            Ok(patch.row(uv[0], uv[1], k * du, k * dv)?.into_iter().map(|(i, j, c)| (i, j, c * sign_k)).collect())
        })
        .collect::<SplineResult<_>>()?;
    let Some(first) = derivative.first() else { return Ok(()) };
    rows.push(Row { terms: along(s.patch, first, n), value: 0.0, weight: 1.0 });
    for (dir, value) in [(w, s.speed), (t, s.slide)] {
        rows.push(Row { terms: along(s.patch, first, dir), value, weight: HERMITE_WEIGHT });
    }
    let Some(jet) = s.across.jet else { return Ok(()) };
    let c = linkage(patch, s, jet.order)?;
    let x0: Vec<[f64; 2]> = c.iter().map(|d| [d.dot(t), d.dot(w)]).collect();
    let Some(targets) = jet.linearised(&x0) else { return Ok(()) };
    for (k, target) in (2..=order).zip(targets) {
        let Some(top) = derivative.get(k - 1) else { continue };
        let mut terms = along(s.patch, top, n);
        let mut value = target.value;
        for (j, (g, x)) in target.gradient.iter().zip(&x0).enumerate().take(k - 1) {
            let Some(cj) = derivative.get(j) else { continue };
            terms.extend(along(s.patch, cj, (t * g[0] + w * g[1]) * -1.0));
            value -= g[0] * x[0] + g[1] * x[1];
        }
        let fade = if k <= s.full { 1.0 } else { s.fade };
        rows.push(Row { terms, value, weight: fade * span.powi(k as i32 - 1) });
    }
    Ok(())
}

/// A patch's fairing energy for a fill asked G^n, between its control points (flat indices): the
/// thin plate (order 2) plus, for n ≥ 2, the energy of order `m = n + 1`, each scaled to unit
/// trace ([`energy_of`]). The higher order leaves the change the conditions ask (of curvature,
/// of its rate…) spread over the fill rather than squeezed into the first span along its edges;
/// the thin plate, whose null space (the planes) the loop pins, keeps the system definite where
/// the higher order's (polynomials of degree below m) is not.
fn energy(patch: &Patch, m: usize) -> SplineResult<Vec<(usize, usize, f64)>> {
    let thin = energy_of(patch, 2)?;
    if m <= 2 {
        return Ok(thin);
    }
    let high = energy_of(patch, m)?;
    let trace = |e: &[(usize, usize, f64)]| e.iter().filter(|(a, b, _)| a == b).map(|(_, _, x)| *x).sum::<f64>();
    let (t2, tm) = (trace(&thin), trace(&high));
    if !(t2 > 0.0 && tm > 0.0) {
        return Ok(thin);
    }
    Ok(thin.into_iter().map(|(a, b, x)| (a, b, x / t2)).chain(high.into_iter().map(|(a, b, x)| (a, b, x / tm))).collect())
}

/// A patch's fairing energy of order `m` between its control points (flat indices):
/// `∫∫ Σ_r C(m, r) |∂ᵘʳ ∂ᵥᵐ⁻ʳ S|²`, the terms `G⁽ʳ⁾ ⊗ G⁽ᵐ⁻ʳ⁾` of the spaces' Gram matrices (order 2
/// is the thin plate `∫∫ |S_uu|² + 2|S_uv|² + |S_vv|²`).
fn energy_of(patch: &Patch, m: usize) -> SplineResult<Vec<(usize, usize, f64)>> {
    let (gu, gv) = (gram(&patch.spaces[0], m)?, gram(&patch.spaces[1], m)?);
    let at = |g: &[Vec<Vec<f64>>], r: usize, a: usize, b: usize| g.get(r).and_then(|x| x.get(a)).and_then(|x| x.get(b)).copied().unwrap_or(0.0);
    let binom = |n: usize, k: usize| (0..k).fold(1.0, |c, i| c * (n - i) as f64 / (i + 1) as f64);
    let [cu, cv] = patch.counts();
    let mut out = Vec::new();
    for j in 0..cv {
        for l in 0..cv {
            for i in 0..cu {
                for k in 0..cu {
                    let e: f64 = (0..=m).map(|r| binom(m, r) * at(&gu, r, i, k) * at(&gv, m - r, j, l)).sum();
                    if e != 0.0 {
                        out.push((i + cu * j, k + cu * l, e));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The Gram matrices `∫ N_a⁽ʳ⁾ N_b⁽ʳ⁾` of a space's basis for r = 0 … m (Gauss–Legendre on
/// each span, exact for the degrees used).
fn gram(s: &Space, m: usize) -> SplineResult<Vec<Vec<Vec<f64>>>> {
    let n = s.count();
    let mut g: Vec<Vec<Vec<f64>>> = vec![vec![vec![0.0; n]; n]; m + 1];
    for w in breaks(s).windows(2) {
        let (lo, hi) = (w[0], w[1]);
        for (x, wt) in GAUSS6 {
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
    Ok(g)
}

/// The free points minimising the weighted rows' squares plus a vanishing multiple of the
/// patches' energy, the loop's rows held: the normal equations, solved by Cholesky.
fn least_squares(quilt: &Quilt, rows: &[Row], energy: &[Vec<(usize, usize, f64)>]) -> SplineResult<Vec<Vec3>> {
    let dim = 3 * quilt.unknowns;
    let slot = |p: usize, flat: usize| quilt.slots.get(p).and_then(|s| s.get(flat)).copied().flatten();
    let mut a = if quilt.spokes.is_empty() { Normal::new(dim, bandwidth(quilt, rows, energy)) } else { Normal::Blocks(BlockNormal::new(quilt)) };
    let mut b = vec![0.0; dim];
    let value = |p: usize, flat: usize| quilt.patches.get(p).and_then(|x| x.net.get(flat)).copied().unwrap_or(Vec3::ZERO);
    let mut condition_trace = 0.0;
    for r in rows {
        let mut unknown: Vec<(usize, f64)> = Vec::new();
        let mut known = 0.0;
        for t in &r.terms {
            let flat = quilt.patches.get(t.patch).map_or(0, |p| p.flat(t.i, t.j));
            let v = value(t.patch, flat);
            for (k, (f, x)) in [t.along.x, t.along.y, t.along.z].into_iter().zip([v.x, v.y, v.z]).enumerate() {
                if f == 0.0 {
                    continue;
                }
                match slot(t.patch, flat) {
                    Some(id) => unknown.push((3 * id + k, f)),
                    None => known += f * x,
                }
            }
        }
        let rhs = r.value - known;
        let w2 = r.weight * r.weight;
        for &(x, cx) in &unknown {
            for &(y, cy) in &unknown {
                a.add(x, y, w2 * cx * cy);
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
                        a.add(3 * x + k, 3 * y + k, lambda * e);
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
    let x = a.solve(b)?;
    Ok((0..quilt.unknowns)
        .map(|i| Vec3::new(x.get(3 * i).copied().unwrap_or(0.0), x.get(3 * i + 1).copied().unwrap_or(0.0), x.get(3 * i + 2).copied().unwrap_or(0.0)))
        .collect())
}

/// The widest coupling between two unknowns (coordinates) in any row or energy term: the
/// normal equations' half bandwidth.
fn bandwidth(quilt: &Quilt, rows: &[Row], energy: &[Vec<(usize, usize, f64)>]) -> usize {
    let slot = |p: usize, i: usize, j: usize| quilt.patches.get(p).and_then(|x| quilt.slots.get(p)?.get(x.flat(i, j)).copied().flatten());
    let mut bw = 0usize;
    for r in rows {
        let ids = r.terms.iter().filter_map(|t| slot(t.patch, t.i, t.j));
        let (lo, hi) = ids.fold((usize::MAX, 0usize), |(lo, hi), x| (lo.min(x), hi.max(x)));
        if lo <= hi {
            bw = bw.max(3 * (hi - lo) + 2);
        }
    }
    for (p, entries) in energy.iter().enumerate() {
        let Some(slots) = quilt.slots.get(p) else { continue };
        for &(f, g, _) in entries {
            if let (Some(Some(x)), Some(Some(y))) = (slots.get(f), slots.get(g)) {
                bw = bw.max(3 * x.abs_diff(*y) + 2);
            }
        }
    }
    bw
}

/// The normal equations' matrix: banded where the band is narrow against the size, else dense.
enum Normal {
    Band(crate::fill::Band),
    Dense(Vec<f64>, usize),
    Blocks(BlockNormal),
}

impl Normal {
    fn new(n: usize, bw: usize) -> Normal {
        if 4 * bw < n { Normal::Band(crate::fill::Band::new(n, bw)) } else { Normal::Dense(vec![0.0; n * n], n) }
    }

    /// Add `v` at `(r, s)` (callers add both halves of a symmetric matrix).
    fn add(&mut self, r: usize, s: usize, v: f64) {
        match self {
            Normal::Band(b) => b.add(r, s, v),
            Normal::Dense(a, n) => {
                if let Some(x) = a.get_mut(r * *n + s) {
                    *x += v;
                }
            }
            Normal::Blocks(m) => m.add(r, s, v),
        }
    }

    fn solve(self, b: Vec<f64>) -> SplineResult<Vec<f64>> {
        match self {
            Normal::Band(m) => m.factor_rows()?.substitute(b),
            Normal::Dense(a, n) => cholesky(a, b, n),
            Normal::Blocks(m) => m.solve(b),
        }
    }
}

/// The normal equations of patches about a centre, solved by blocks: each patch's interior
/// unknowns (coordinates of control points only that patch holds, coupled to no other patch's)
/// form a banded block, eliminated exactly onto the shared unknowns (the spokes', the centre's,
/// and any a row couples across patches) by their Schur complement, which is solved dense; the
/// interiors then follow by back substitution. Entries are kept sparse until the blocks are
/// known (callers add both halves of the symmetric matrix).
struct BlockNormal {
    /// Each coordinate's patch when only one patch holds its point, else `None`.
    owner: Vec<Option<usize>>,
    entries: std::collections::HashMap<(usize, usize), f64>,
}

impl BlockNormal {
    fn new(quilt: &Quilt) -> BlockNormal {
        let mut holders: Vec<Vec<usize>> = vec![Vec::new(); quilt.unknowns];
        for (p, slots) in quilt.slots.iter().enumerate() {
            for id in slots.iter().flatten() {
                if let Some(h) = holders.get_mut(*id)
                    && !h.contains(&p)
                {
                    h.push(p);
                }
            }
        }
        let owner = holders.iter().flat_map(|h| {
            let o = if let [p] = h.as_slice() { Some(*p) } else { None };
            [o; 3]
        });
        BlockNormal { owner: owner.collect(), entries: std::collections::HashMap::new() }
    }

    fn add(&mut self, r: usize, s: usize, v: f64) {
        if v != 0.0 {
            *self.entries.entry((r, s)).or_insert(0.0) += v;
        }
    }

    fn solve(mut self, b: Vec<f64>) -> SplineResult<Vec<f64>> {
        let n = b.len();
        // A coordinate coupled to another patch's interior is shared too.
        let coupled: Vec<usize> = self
            .entries
            .keys()
            .filter(|(r, c)| matches!((self.owner.get(*r).copied().flatten(), self.owner.get(*c).copied().flatten()), (Some(a), Some(b)) if a != b))
            .flat_map(|(r, c)| [*r, *c])
            .collect();
        for x in coupled {
            if let Some(o) = self.owner.get_mut(x) {
                *o = None;
            }
        }
        let shared: Vec<usize> = (0..n).filter(|x| self.owner.get(*x).copied().flatten().is_none()).collect();
        if shared.len() > MAX_SHARED_UNKNOWNS {
            return Err(SplineError::TooLarge(format!(
                "{} unknowns shared by the patches about the centre (at most {MAX_SHARED_UNKNOWNS})",
                shared.len()
            )));
        }
        let mut place = vec![usize::MAX; n];
        for (i, x) in shared.iter().enumerate() {
            if let Some(slot) = place.get_mut(*x) {
                *slot = i;
            }
        }
        let patches = self.owner.iter().flatten().copied().max().map_or(0, |m| m + 1);
        let mut interior: Vec<Vec<usize>> = vec![Vec::new(); patches];
        for (x, o) in self.owner.iter().enumerate() {
            if let Some(p) = o
                && let Some(list) = interior.get_mut(*p)
            {
                if let Some(slot) = place.get_mut(x) {
                    *slot = list.len();
                }
                list.push(x);
            }
        }
        let m = shared.len();
        // The shared block, and per patch its block, its coupling to the shared unknowns.
        let mut s_mat = vec![0.0; m * m];
        let mut blocks: Vec<std::collections::HashMap<(usize, usize), f64>> = vec![std::collections::HashMap::new(); patches];
        let mut couple: Vec<std::collections::HashMap<(usize, usize), f64>> = vec![std::collections::HashMap::new(); patches];
        for ((r, c), v) in std::mem::take(&mut self.entries) {
            let (or, oc) = (self.owner.get(r).copied().flatten(), self.owner.get(c).copied().flatten());
            let (pr, pc) = (place.get(r).copied().unwrap_or(0), place.get(c).copied().unwrap_or(0));
            match (or, oc) {
                (None, None) => {
                    if let Some(x) = s_mat.get_mut(pr * m + pc) {
                        *x += v;
                    }
                }
                (Some(p), Some(_)) => {
                    if let Some(blk) = blocks.get_mut(p) {
                        *blk.entry((pr, pc)).or_insert(0.0) += v;
                    }
                }
                // Interior row, shared column (the transposed entry is its mirror).
                (Some(p), None) => {
                    if let Some(cp) = couple.get_mut(p) {
                        *cp.entry((pr, pc)).or_insert(0.0) += v;
                    }
                }
                (None, Some(_)) => {}
            }
        }
        let mut rhs: Vec<f64> = shared.iter().map(|x| b.get(*x).copied().unwrap_or(0.0)).collect();
        let mut solved_parts: Vec<(crate::fill::Band, Vec<f64>, Vec<(usize, Vec<f64>)>)> = Vec::with_capacity(patches);
        for p in 0..patches {
            let ids = interior.get(p).map(Vec::as_slice).unwrap_or(&[]);
            let k = ids.len();
            let blk = blocks.get(p).cloned().unwrap_or_default();
            let bw = blk.keys().map(|(r, c)| r.abs_diff(*c)).max().unwrap_or(0);
            let mut band = crate::fill::Band::new(k.max(1), bw);
            for (&(r, c), &v) in &blk {
                band.add(r, c, v);
            }
            if k == 0 {
                solved_parts.push((band, Vec::new(), Vec::new()));
                continue;
            }
            let band = band.factor_rows()?;
            // g = A_kk⁻¹ b_k; per shared column j coupled to this patch, z_j = A_kk⁻¹ A_k,j.
            let bk: Vec<f64> = ids.iter().map(|x| b.get(*x).copied().unwrap_or(0.0)).collect();
            let g = band.substitute(bk)?;
            let mut columns: std::collections::BTreeMap<usize, Vec<f64>> = std::collections::BTreeMap::new();
            for (&(r, j), &v) in couple.get(p).into_iter().flatten() {
                if let Some(cell) = columns.entry(j).or_insert_with(|| vec![0.0; k]).get_mut(r) {
                    *cell += v;
                }
            }
            let mut z: Vec<(usize, Vec<f64>)> = Vec::with_capacity(columns.len());
            for (j, col) in columns {
                z.push((j, band.substitute(col)?));
            }
            // S −= A_Bk·Z, rhs −= A_Bk·g, A_Bk the transpose of the coupling.
            let coupling = couple.get(p).cloned().unwrap_or_default();
            let mut by_column: std::collections::BTreeMap<usize, Vec<(usize, f64)>> = std::collections::BTreeMap::new();
            for (&(r, j), &v) in &coupling {
                by_column.entry(j).or_default().push((r, v));
            }
            for (i, entries) in &by_column {
                let dot = |w: &[f64]| entries.iter().map(|(r, v)| v * w.get(*r).copied().unwrap_or(0.0)).sum::<f64>();
                if let Some(x) = rhs.get_mut(*i) {
                    *x -= dot(&g);
                }
                for (j, zj) in &z {
                    if let Some(x) = s_mat.get_mut(i * m + j) {
                        *x -= dot(zj);
                    }
                }
            }
            solved_parts.push((band, g, z));
        }
        let xs = if m > 0 { cholesky(s_mat, rhs, m)? } else { Vec::new() };
        let mut x = vec![0.0; n];
        for (i, id) in shared.iter().enumerate() {
            if let Some(slot) = x.get_mut(*id) {
                *slot = xs.get(i).copied().unwrap_or(0.0);
            }
        }
        for (p, (_, g, z)) in solved_parts.iter().enumerate() {
            let ids = interior.get(p).map(Vec::as_slice).unwrap_or(&[]);
            for (r, id) in ids.iter().enumerate() {
                let mut v = g.get(r).copied().unwrap_or(0.0);
                for (j, zj) in z {
                    v -= zj.get(r).copied().unwrap_or(0.0) * xs.get(*j).copied().unwrap_or(0.0);
                }
                if let Some(slot) = x.get_mut(*id) {
                    *slot = v;
                }
            }
        }
        if x.iter().any(|v| !v.is_finite()) {
            return invalid("the patches' equations have no finite solution");
        }
        Ok(x)
    }
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
fn measure(
    quilt: &Quilt,
    stations: &[Station],
    points: &[Vec3],
    located: &[(usize, [f64; 2])],
    spoke_count: usize,
) -> SplineResult<(Achieved, Along)> {
    let mut r = Achieved::default();
    let mut along = Along::default();
    let mut jumps = [0.0f64; MAX_CONTINUITY + 1];
    for s in stations {
        let Some(patch) = quilt.patches.get(s.patch) else { continue };
        let (uv, _) = station_on(s.side, s.t);
        let Some(normal) = patch.normal(uv[0], uv[1]) else { continue };
        let a = normal.dot(s.across.normal).abs().clamp(0.0, 1.0).acos();
        r.normal_deviation_all = r.normal_deviation_all.max(a);
        let d = s.side % 2;
        if s.held {
            r.normal_deviation = r.normal_deviation.max(a);
            r.normal_excess = r.normal_excess.max(a - s.tilt);
            if let Some(x) = along.angle.get_mut(d) {
                *x = x.max(a - s.tilt);
            }
        }
        if s.order() < 2 || !s.clear {
            continue;
        }
        // The linkage curve's height above the neighbour, per unit length along its foot.
        let c = linkage(patch, s, s.across.jet.map_or(1, |j| j.order))?;
        let n = s.across.normal;
        let Some(first) = c.first() else { continue };
        let foot = (*first - n * first.dot(n)).len().max(1e-300);
        for (k, want) in (2..).zip(targets(s, &c)) {
            let (Some(ck), Some(jump)) = (c.get(k - 1), jumps.get_mut(k)) else { continue };
            let here = ((n.dot(*ck) - want) / foot.powi(k as i32)).abs();
            *jump = jump.max(here);
            if let Some(x) = along.jumps.get_mut(d).and_then(|j| j.get_mut(k)) {
                *x = x.max(here);
            }
        }
    }
    // Across the spokes, clear of the centre.
    for &(left, right) in &quilt.spokes {
        let (Some(a), Some(b)) = (quilt.patches.get(left), quilt.patches.get(right)) else { continue };
        for i in 0..spoke_count {
            let t = (i as f64 + 0.5) / spoke_count as f64;
            if let (Some(na), Some(nb)) = (a.normal(1.0, t), b.normal(t, 1.0)) {
                let a = na.dot(nb).clamp(-1.0, 1.0).acos();
                r.normal_deviation_all = r.normal_deviation_all.max(a);
                if t < 1.0 - CLASH_SHARE {
                    r.normal_deviation = r.normal_deviation.max(a);
                    r.normal_excess = r.normal_excess.max(a);
                    along.angle = along.angle.map(|x| x.max(a));
                }
            }
        }
    }
    r.jumps = jumps;
    for (p, (patch, uv)) in points.iter().zip(located) {
        if let Some(on) = quilt.patches.get(*patch) {
            r.point_deviation = r.point_deviation.max(on.eval(uv[0], uv[1], 0, 0)?.dist(*p));
        }
    }
    Ok((r, along))
}

/// What a fit achieves at the stations of the sides running along u (`[0]`) and along v (`[1]`):
/// where it falls short, the spans that way are halved.
#[derive(Clone, Copy, Debug, Default)]
struct Along {
    angle: [f64; 2],
    jumps: [[f64; MAX_CONTINUITY + 1]; 2],
}

impl Along {
    /// The directions whose sides miss the angle tolerance, or an order's ([`within_order`]).
    fn missing(&self, order: usize, size: f64, angle_tol: f64) -> [bool; 2] {
        [0, 1]
            .map(|d| self.angle.get(d).is_some_and(|a| *a > angle_tol) || self.jumps.get(d).is_some_and(|j| !within_order(j, order, size, angle_tol)))
    }
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

/// Whether a patch folds back on itself: on a grid of its parameters, its normal reverses between
/// neighbouring points (the surface turns back over itself), or vanishes inside it. A patch may
/// turn its normal through more than a right angle overall (a dome meeting a tube's wall, a
/// window round more than half a cylinder): only a reversal between neighbours is a fold. The
/// normal may vanish at a corner (where two sides run on straight, as on a closed loop's pieces)
/// and on a three-sided patch's pole at u = 0.
fn folds(patch: &Patch) -> bool {
    let n = FOLD_SAMPLES;
    if patch.normal(0.5, 0.5).is_none() {
        return true;
    }
    let at = |i: usize, j: usize| patch.normal(i as f64 / n as f64, j as f64 / n as f64);
    let mut grid: Vec<Option<Vec3>> = Vec::with_capacity((n + 1) * (n + 1));
    for i in 0..=n {
        for j in 0..=n {
            let normal = at(i, j);
            let excused = (patch.pole && i == 0) || ((i == 0 || i == n) && (j == 0 || j == n));
            if normal.is_none() && !excused {
                return true;
            }
            grid.push(normal);
        }
    }
    let get = |i: usize, j: usize| grid.get(i * (n + 1) + j).copied().flatten();
    for i in 0..=n {
        for j in 0..=n {
            let Some(a) = get(i, j) else { continue };
            for (k, l) in [(i + 1, j), (i, j + 1)] {
                if k <= n && l <= n && get(k, l).is_some_and(|b| a.dot(b) <= 0.0) {
                    return true;
                }
            }
        }
    }
    false
}

/// The fill with its middle filled out by `crown` times the loop's `size` along the normals (to
/// the side the neighbours' normals face), on
/// control points clear of the rows the conditions hold (n + 1 rows from a G^n edge), so the loop and the conditions across it do not move. One patch
/// rises by `16·u(1−u)·v(1−v)`; patches about a centre by `u(2−u)·v(2−v)`, nought on the loop and
/// one at the centre, the same on a spoke from either side.
fn crowned(mut quilt: Quilt, stations: &[Station], crown: f64, size: f64) -> SplineResult<Quilt> {
    if crown == 0.0 {
        return Ok(quilt);
    }
    let held = 1 + stations.iter().map(Station::order).max().unwrap_or(0);
    // Out to the side the neighbours' normals face (the patches all turn one way).
    let mut facing = 0.0;
    for s in stations {
        let (uv, _) = station_on(s.side, s.t);
        if let Some(n) = quilt.patches.get(s.patch).and_then(|p| p.normal(uv[0], uv[1])) {
            facing += n.dot(s.across.normal);
        }
    }
    let out = if facing < 0.0 { -1.0 } else { 1.0 };
    let about_centre = !quilt.spokes.is_empty();
    let bump = |x: f64, y: f64| if about_centre { x * (2.0 - x) * y * (2.0 - y) } else { 16.0 * x * (1.0 - x) * y * (1.0 - y) };
    let mut moved = std::collections::HashMap::new();
    for (k, patch) in quilt.patches.iter().enumerate() {
        let [cu, cv] = patch.counts();
        let (gu, gv) = (greville(&patch.spaces[0]), greville(&patch.spaces[1]));
        let far = |c: usize| if about_centre { c } else { c.saturating_sub(held) };
        for j in held..far(cv) {
            for i in held..far(cu) {
                let Some(Some(id)) = quilt.slots.get(k).and_then(|s| s.get(patch.flat(i, j))) else { continue };
                let (x, y) = (gu.get(i).copied().unwrap_or(0.5), gv.get(j).copied().unwrap_or(0.5));
                let Some(n) = patch.normal(x, y) else { continue };
                moved.entry(*id).or_insert_with(|| patch.at(i, j) + n * (out * crown * size * bump(x, y)));
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

#[cfg(test)]
#[path = "fill_iso_tests.rs"]
mod tests;
