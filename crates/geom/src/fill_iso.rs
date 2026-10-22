//! Filling a closed loop of three or four exact B-spline curves with one tensor B-spline patch
//! whose boundary rows *are* those curves, so that every edge lies on the patch exactly and its
//! pcurve is a straight line in (u, v), affine in the edge's own parameter.
//!
//! The formulation:
//!
//! - **The sides.** A loop of four curves `e₀ … e₃` bounds the unit square: `e₀` runs along
//!   `v = 0` from `u = 0` to 1, `e₁` along `u = 1`, `e₂` back along `v = 1` and `e₃` down
//!   `u = 0`. A loop of three has no `e₃`: the side `u = 0` collapses to the loop's first corner
//!   (a pole, as a three-sided tensor patch must have). Each curve's parameter is mapped affinely
//!   onto its side's `[0, 1]` (and reversed where the side runs back), which changes neither the
//!   curve nor its pieces.
//! - **One space each way.** The two curves along u (and the two along v) are made compatible
//!   exactly: split into their Bézier pieces at every break of either, each piece raised to the
//!   higher degree (NURBS Book §5.5), and joined again with each break repeated as often as that
//!   degree ([`crate::spline_space::embed`]). The tensor product of the two spaces holds the
//!   patch.
//! - **The start**: the bilinearly blended Coons patch of the four sides (S. A. Coons, 1967),
//!   whose control net is the two ruled nets less the bilinear one, each built on the other
//!   direction's Greville abscissae (a linear function is reproduced exactly there). Its boundary
//!   rows are the sides' own control points.
//! - **The inside**: with the boundary rows held, the interior control points minimise, by least
//!   squares, the misfit to the conditions plus a vanishing multiple of the thin-plate energy
//!   `∫∫ |S_uu|² + 2|S_uv|² + |S_vv|²` (which alone decides a G0 patch):
//!   - G1 at stations along a side: `n·∂S/∂ν = 0` (`ν` across the side into the patch, `n` the
//!     neighbour's normal), with weak rows `∂S/∂ν = s·w` (`w` the neighbour's leave direction,
//!     `s` the side's own speed) that keep the patch leaving into itself;
//!   - G2: `n·∂²S/∂ν² = (∂S/∂ν·w)²·κ`, `κ` the neighbour's normal curvature along `w`, the speed
//!     read from the last solution;
//!   - points the patch passes through, located on the last solution (nearest point) and fitted
//!     again (Hoschek's parameter correction).
//!
//!   At a corner where the neighbours' tangent planes disagree no surface is G1 to both: the
//!   stations near it are fitted but not held to the angle tolerance.
//! - **Refinement**: while the normals miss the neighbours' by more than the angle tolerance, a
//!   point is missed, or the patch folds, every span is halved both ways (knot insertion carries
//!   the sides into the finer space exactly), up to [`MAX_ISO_SPANS`].
//! - **A crown** fills the middle out along the patch's normals, on control points clear of the
//!   rows the conditions hold, so the loop and the conditions across it do not move.
//!
//! The boundary of a patch is exact, whatever its inside: a station's gap is nought by
//! construction. Rational sides are not supported (the Coons sum needs one weight function per
//! side pair). Input is hostile: sizes are capped and every evaluation checked; nothing here
//! panics.

use crate::Vec3;
use crate::bspline::{HPoint, MAX_SPLINE_DEGREE, NurbsSurface, SplineError, SplineResult, check_knots, invalid, segment};
use crate::fill_general::Across;
use crate::spline_space::{Space, embed};

/// Most spans of the patch along either direction.
pub const MAX_ISO_SPANS: usize = 64;
/// Most interior control points one least-squares solve takes (its system is dense).
const MAX_UNKNOWN_POINTS: usize = 400;
/// Stations per span of a side where conditions are asked and checked.
const STATIONS_PER_SPAN: usize = 6;
/// Fewest stations along a side.
const MIN_STATIONS: usize = 16;
/// Weight of the weak Hermite rows against the G1 rows.
const HERMITE_WEIGHT: f64 = 0.05;
/// Rounds of relinearising G2 and relocating the points.
const ROUNDS: usize = 3;
/// The share of a side, from a clashing corner, whose stations are not held to the angle
/// tolerance.
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
    fn domain(&self) -> [f64; 2] {
        [self.knots.get(self.degree).copied().unwrap_or(0.0), self.knots.get(self.points.len()).copied().unwrap_or(0.0)]
    }
}

/// What the patch is asked for besides its boundary.
#[derive(Clone, Copy, Debug)]
pub struct IsoSpec<'a> {
    /// How far a point may be from the patch.
    pub tol: f64,
    /// How far (radians) the patch's normal may turn from a G1/G2 neighbour's.
    pub angle_tol: f64,
    /// How full the middle is made (as [`crate::fill::fill_loop`]'s crown).
    pub crown: f64,
    /// Points the patch passes through.
    pub points: &'a [Vec3],
}

/// The patch and what it achieved.
#[derive(Clone, Debug, PartialEq)]
pub struct IsoFill {
    pub surface: NurbsSurface,
    /// Each edge's (u, v) at its domain's start and end: the pcurve is the straight line between,
    /// affine in the edge's parameter.
    pub sides: Vec<[[f64; 2]; 2]>,
    pub achieved: Achieved,
}

/// What a patch achieves at its stations and points (its boundary is exact).
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Achieved {
    /// The largest angle (radians) between the patch's normal and a G1/G2 neighbour's at a
    /// station held to the tolerance, and at any station.
    pub normal_deviation: f64,
    pub normal_deviation_all: f64,
    /// The largest difference (1/length) between the patch's normal curvature across a G2 side
    /// and the neighbour's.
    pub curvature_mismatch: f64,
    /// The farthest point from the patch.
    pub point_deviation: f64,
}

/// A side as the patch holds it: a B-spline over `[0, 1]`.
#[derive(Clone, Debug)]
struct Side {
    space: Space,
    points: Vec<Vec3>,
}

/// `edge` over `[0, 1]`, run backwards when `reverse`.
fn on_unit(edge: &IsoEdge, reverse: bool) -> SplineResult<Side> {
    let p = edge.degree;
    if p == 0 || p > MAX_SPLINE_DEGREE || edge.points.len() <= p || edge.knots.len() != edge.points.len() + p + 1 {
        return invalid("an edge's degree, knots and control points do not fit");
    }
    check_knots(&edge.knots)?;
    if edge.points.iter().any(|q| !q.is_finite()) {
        return invalid("an edge's control point is not a finite number");
    }
    let [a, b] = edge.domain();
    if !(a < b) {
        return invalid("an edge has an empty domain");
    }
    // Clamped on its own domain first (the same curve).
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
    Ok(Side { space: Space::new(p, knots)?, points })
}

/// The point `q` as a side (a pole).
fn pole(q: Vec3) -> SplineResult<Side> {
    Ok(Side { space: Space::polynomial(1, 0.0, 1.0)?, points: vec![q, q] })
}

/// The breaks of a space: its distinct knots.
fn breaks(s: &Space) -> Vec<f64> {
    let mut out: Vec<f64> = s.knots().to_vec();
    out.dedup();
    out
}

/// The sides in one space: every break of either, each repeated as often as the higher degree,
/// each Bézier piece raised to that degree. Exact.
fn compatible(sides: [&Side; 2], extra: &[f64]) -> SplineResult<(Space, [Vec<Vec3>; 2])> {
    let q = sides.iter().map(|s| s.space.degree()).max().unwrap_or(1);
    let mut all: Vec<f64> = sides.iter().flat_map(|s| breaks(&s.space)).chain(extra.iter().copied()).filter(|x| (0.0..=1.0).contains(x)).collect();
    all.sort_by(f64::total_cmp);
    all.dedup();
    let mut knots = vec![0.0; q + 1];
    for &x in all.iter().filter(|x| **x > 0.0 && **x < 1.0) {
        knots.extend(std::iter::repeat_n(x, q));
    }
    knots.extend(std::iter::repeat_n(1.0, q + 1));
    let target = Space::new(q, knots)?;
    let mut out: [Vec<Vec3>; 2] = [Vec::new(), Vec::new()];
    for (slot, side) in out.iter_mut().zip(sides) {
        let p = side.space.degree();
        let h: Vec<HPoint> = side.points.iter().map(|x| HPoint::weighted(*x, 1.0)).collect();
        let mut points: Vec<Vec3> = Vec::new();
        for w in all.windows(2) {
            let (a, b) = (w[0], w[1]);
            let (_, piece) = segment(p, side.space.knots(), &h, a, b)?;
            let piece: Vec<Vec3> = piece.iter().map(|x| x.xyz()).collect();
            let raised = embed(&Space::polynomial(p, a, b)?, &piece, &Space::polynomial(q, a, b)?)?;
            points.extend(raised.into_iter().skip(usize::from(!points.is_empty())));
        }
        if points.len() != target.count() {
            return invalid("the sides' pieces do not fill the common space");
        }
        *slot = points;
    }
    Ok((target, out))
}

/// The Greville abscissae of a space (where its splines reproduce a linear function).
fn greville(s: &Space) -> Vec<f64> {
    let (p, k) = (s.degree(), s.knots());
    (0..s.count()).map(|i| (1..=p).map(|r| k.get(i + r).copied().unwrap_or(0.0)).sum::<f64>() / p as f64).collect()
}

/// A tensor patch on two spaces, control point `(i, j)` (i along u) at `i + nu·j`.
#[derive(Clone, Debug)]
struct Patch {
    space: [Space; 2],
    net: Vec<Vec3>,
}

impl Patch {
    fn counts(&self) -> [usize; 2] {
        [self.space[0].count(), self.space[1].count()]
    }

    fn at(&self, i: usize, j: usize) -> Vec3 {
        self.net.get(i + self.counts()[0] * j).copied().unwrap_or(Vec3::ZERO)
    }

    /// The coefficients of `∂^(du+dv) S / ∂u^du ∂v^dv` at (u, v) on each control point it
    /// depends on: `(i, j, coefficient)`.
    fn row(&self, u: f64, v: f64, du: usize, dv: usize) -> SplineResult<Vec<(usize, usize, f64)>> {
        let (iu, bu) = self.space[0].basis(u, du)?;
        let (iv, bv) = self.space[1].basis(v, dv)?;
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
        let [cu, cv] = self.counts();
        let ctrl: Vec<HPoint> = (0..cv).flat_map(|j| (0..cu).map(move |i| (i, j))).map(|(i, j)| HPoint::weighted(self.at(i, j), 1.0)).collect();
        NurbsSurface::from_homogeneous(
            [self.space[0].degree(), self.space[1].degree()],
            [self.space[0].knots().to_vec(), self.space[1].knots().to_vec()],
            ctrl,
        )
    }
}

/// Where side `k` (of `n`) runs on the unit square: (u, v) at its start and end.
fn side_uv(k: usize) -> [[f64; 2]; 2] {
    match k {
        0 => [[0.0, 0.0], [1.0, 0.0]],
        1 => [[1.0, 0.0], [1.0, 1.0]],
        2 => [[1.0, 1.0], [0.0, 1.0]],
        _ => [[0.0, 1.0], [0.0, 0.0]],
    }
}

/// At fraction `t` of side `k`: (u, v), the direction across into the patch (`(du, dv, sign)`:
/// the derivative order taken along u or v, and its sign).
fn station_on(k: usize, t: f64) -> ([f64; 2], (usize, usize, f64)) {
    let [a, b] = side_uv(k);
    let uv = [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t];
    let across = match k {
        0 => (0, 1, 1.0),
        1 => (1, 0, -1.0),
        2 => (0, 1, -1.0),
        _ => (1, 0, 1.0),
    };
    (uv, across)
}

/// One condition row: coefficients on the net's points (dotted with `dir` when given, else on one
/// coordinate `coord`) and the value.
struct Row {
    terms: Vec<(usize, usize, f64)>,
    dir: Option<Vec3>,
    coord: usize,
    value: f64,
    weight: f64,
}

/// A station of a side: where, its condition, and whether it is held to the angle tolerance.
struct Station {
    side: usize,
    t: f64,
    across: Across,
    held: bool,
    /// The side's speed there (mm per unit of t).
    speed: f64,
}

/// Fill the loop `edges` (three or four, each ending where the next starts) with one patch on
/// them. `across(k, ts)` gives the neighbour's conditions at the parameters `ts` of edge `k` (its
/// own parameter; `None`: G0 there); `clash[k]`: the neighbours' tangent planes disagree at the
/// corner where edge `k` starts.
pub fn iso_fill(
    edges: &[IsoEdge],
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
    clash: &[bool],
    spec: &IsoSpec,
) -> SplineResult<IsoFill> {
    let n = edges.len();
    if !(3..=4).contains(&n) {
        return Err(SplineError::Unsupported(format!("a patch on a loop of {n} edges (three or four)")));
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
    // The sides, each on [0, 1] in its square's direction, and the corners they share exactly.
    let sides: Vec<Side> = edges.iter().enumerate().map(|(k, e)| on_unit(e, k >= 2)).collect::<SplineResult<_>>()?;
    let ends = |s: &Side| (s.points.first().copied().unwrap_or(Vec3::ZERO), s.points.last().copied().unwrap_or(Vec3::ZERO));
    let mut all = edges.iter().flat_map(|e| e.points.iter().copied());
    let first = all.next().ok_or_else(|| SplineError::Invalid("an edge has no control points".into()))?;
    let (lo, hi) = all.fold((first, first), |(lo, hi), p| (lo.min(p), hi.max(p)));
    let size = lo.dist(hi);
    let meets = |a: Vec3, b: Vec3| a.dist(b) <= 1e-9 * size.max(1.0);
    for k in 0..n {
        let end = edges.get(k).and_then(|e| e.points.last()).copied().unwrap_or(Vec3::ZERO);
        let next = edges.get((k + 1) % n).and_then(|e| e.points.first()).copied().unwrap_or(Vec3::ZERO);
        if !meets(end, next) {
            return invalid(format!("edge {k} does not end where edge {} starts", (k + 1) % n));
        }
    }
    let (Some(s0), Some(s1), Some(s2)) = (sides.first(), sides.get(1), sides.get(2)) else { return invalid("sides") };
    let s3 = match sides.get(3) {
        Some(s) => s.clone(),
        None => pole(ends(s0).0)?,
    };
    // Stations: along each side, in the edge's own parameter.
    let held_side = |k: usize, t: f64| {
        let near_start = clash.get(k).copied().unwrap_or(false) && t < CLASH_SHARE;
        let near_end = clash.get((k + 1) % n).copied().unwrap_or(false) && t > 1.0 - CLASH_SHARE;
        !(near_start || near_end)
    };
    let mut extra: [Vec<f64>; 2] = [Vec::new(), Vec::new()];
    let mut spans = 1usize;
    loop {
        let (su, [c0, c1]) = compatible([s0, s2], &extra[0])?;
        let (sv, [d0, d1]) = compatible([&s3, s1], &extra[1])?;
        let patch = coons(&su, &sv, &c0, &c1, &d0, &d1)?;
        let stations = stations(edges, &patch, &mut *across, &held_side)?;
        let (patch, report) = solve(patch, &stations, spec)?;
        let fits = report.normal_deviation <= spec.angle_tol && report.point_deviation <= spec.tol && !folds(&patch, n == 3);
        let fine = patch.counts().iter().any(|c| c.saturating_sub(2).pow(2) * 4 > MAX_UNKNOWN_POINTS);
        if fits || spans * 2 > MAX_ISO_SPANS || fine {
            if !fits {
                return Err(SplineError::Unsupported(format!(
                    "a patch on these edges within the tolerances: normals within {:.4}° of the neighbours', points within {:.2e}",
                    report.normal_deviation.to_degrees(),
                    report.point_deviation
                )));
            }
            let patch = crowned(patch, &stations, spec.crown, size)?;
            return Ok(IsoFill { surface: patch.surface()?, sides: (0..n).map(side_uv).collect(), achieved: report });
        }
        // Halve every span both ways.
        spans *= 2;
        for (d, s) in [&su, &sv].into_iter().enumerate() {
            let b = breaks(s);
            if let Some(list) = extra.get_mut(d) {
                list.extend(b.windows(2).map(|w| 0.5 * (w[0] + w[1])));
            }
        }
    }
}

/// The bilinearly blended Coons patch: `c0`, `c1` along u at v = 0 and 1, `d0`, `d1` along v at
/// u = 0 and 1, all running with their parameter.
fn coons(su: &Space, sv: &Space, c0: &[Vec3], c1: &[Vec3], d0: &[Vec3], d1: &[Vec3]) -> SplineResult<Patch> {
    let (gu, gv) = (greville(su), greville(sv));
    let (cu, cv) = (su.count(), sv.count());
    let corner = |a: &[Vec3], first: bool| if first { a.first().copied() } else { a.last().copied() };
    let (Some(p00), Some(p10), Some(p01), Some(p11)) = (corner(c0, true), corner(c0, false), corner(c1, true), corner(c1, false)) else {
        return invalid("a side has no points");
    };
    let mut net = Vec::with_capacity(cu * cv);
    for (j, &y) in gv.iter().enumerate() {
        for (i, &x) in gu.iter().enumerate() {
            let get = |a: &[Vec3], k: usize| a.get(k).copied().unwrap_or(Vec3::ZERO);
            let ruled_v = get(c0, i) * (1.0 - y) + get(c1, i) * y;
            let ruled_u = get(d0, j) * (1.0 - x) + get(d1, j) * x;
            let bilinear = (p00 * (1.0 - x) + p10 * x) * (1.0 - y) + (p01 * (1.0 - x) + p11 * x) * y;
            // The boundary rows exactly the sides' own points.
            net.push(match (i, j) {
                (_, 0) => get(c0, i),
                (_, j) if j + 1 == cv => get(c1, i),
                (0, _) => get(d0, j),
                (i, _) if i + 1 == cu => get(d1, j),
                _ => ruled_v + ruled_u - bilinear,
            });
        }
    }
    Ok(Patch { space: [su.clone(), sv.clone()], net })
}

/// The stations of every side with a condition: enough of them for the patch's spans.
fn stations(
    edges: &[IsoEdge],
    patch: &Patch,
    across: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Option<Across>>, String>,
    held: &dyn Fn(usize, f64) -> bool,
) -> SplineResult<Vec<Station>> {
    let mut out = Vec::new();
    for (k, e) in edges.iter().enumerate() {
        let along = if k % 2 == 0 { 0 } else { 1 };
        let spans = breaks(&patch.space[along]).len().saturating_sub(1).max(1);
        let count = (spans * STATIONS_PER_SPAN).max(MIN_STATIONS);
        // Inside the side: the corners are the neighbours' corners.
        let ts: Vec<f64> = (0..count).map(|i| (i as f64 + 0.5) / count as f64).collect();
        let [a, b] = e.domain();
        let params: Vec<f64> = ts.iter().map(|t| a + (b - a) * t).collect();
        let found = across(k, &params).map_err(SplineError::Invalid)?;
        for (&t, c) in ts.iter().zip(found) {
            let Some(c) = c else { continue };
            let (uv, (du, dv, _)) = station_on(k, t);
            let speed = patch.eval(uv[0], uv[1], dv, du).map(|d| d.len()).unwrap_or(0.0);
            out.push(Station { side: k, t, across: c, held: held(k, t), speed });
        }
    }
    Ok(out)
}

/// The patch's interior control points by least squares (module docs), and what they achieve.
fn solve(mut patch: Patch, stations: &[Station], spec: &IsoSpec) -> SplineResult<(Patch, Achieved)> {
    let [cu, cv] = patch.counts();
    let free = |i: usize, j: usize| i > 0 && j > 0 && i + 1 < cu && j + 1 < cv;
    let index = |i: usize, j: usize| (j - 1) * (cu - 2) + (i - 1);
    let m = (cu.saturating_sub(2)) * (cv.saturating_sub(2));
    if m == 0 {
        return Ok((patch, Achieved::default()));
    }
    if m > MAX_UNKNOWN_POINTS {
        return Err(SplineError::TooLarge(format!("{m} interior control points (at most {MAX_UNKNOWN_POINTS})")));
    }
    let energy = energy(&patch)?;
    let mut located: Vec<[f64; 2]> = Vec::new();
    let mut report = Achieved::default();
    for round in 0..ROUNDS {
        let mut rows: Vec<Row> = Vec::new();
        for s in stations {
            let (uv, (du, dv, sign)) = station_on(s.side, s.t);
            let cross = patch.row(uv[0], uv[1], du, dv)?;
            let scale = |r: Vec<(usize, usize, f64)>, f: f64| r.into_iter().map(|(i, j, c)| (i, j, c * f)).collect::<Vec<_>>();
            let first = scale(cross, sign);
            rows.push(Row { terms: first.clone(), dir: Some(s.across.normal), coord: 0, value: 0.0, weight: 1.0 });
            let w = s.across.leave * s.speed;
            for c in 0..3 {
                rows.push(Row { terms: first.clone(), dir: None, coord: c, value: [w.x, w.y, w.z][c], weight: HERMITE_WEIGHT });
            }
            if let Some(bend) = s.across.bend {
                let now = patch.eval(uv[0], uv[1], du, dv)? * sign;
                let along = now.dot(s.across.leave);
                let second = patch.row(uv[0], uv[1], 2 * du, 2 * dv)?;
                rows.push(Row { terms: second, dir: Some(s.across.normal), coord: 0, value: along * along * bend.dot(s.across.normal), weight: 1.0 });
            }
        }
        for (p, uv) in spec.points.iter().zip(&located) {
            for c in 0..3 {
                rows.push(Row { terms: patch.row(uv[0], uv[1], 0, 0)?, dir: None, coord: c, value: [p.x, p.y, p.z][c], weight: 1.0 });
            }
        }
        let solved = least_squares(&patch, &rows, &energy, m, &free, &index)?;
        for j in 1..cv - 1 {
            for i in 1..cu - 1 {
                if let (Some(slot), Some(p)) = (patch.net.get_mut(i + cu * j), solved.get(index(i, j))) {
                    *slot = *p;
                }
            }
        }
        // Points: located on this solution for the next round.
        located = spec.points.iter().map(|p| nearest(&patch, *p)).collect();
        report = measure(&patch, stations, spec.points, &located)?;
        let relinearise = stations.iter().any(|s| s.across.bend.is_some()) || !spec.points.is_empty();
        if !relinearise && round == 0 {
            break;
        }
    }
    Ok((patch, report))
}

/// The thin-plate energy's coefficients between control points (both indices flat), of the terms
/// `G⁽ʳ⁾ᵤ ⊗ G⁽ˢ⁾ᵥ` with `(r, s)` = (2, 0), (1, 1) twice, (0, 2).
fn energy(patch: &Patch) -> SplineResult<Vec<(usize, usize, f64)>> {
    let gram = |s: &Space| -> SplineResult<[Vec<Vec<f64>>; 3]> {
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
        Ok(g)
    };
    let (gu, gv) = (gram(&patch.space[0])?, gram(&patch.space[1])?);
    let [cu, cv] = patch.counts();
    let g = |m: &[Vec<Vec<f64>>; 3], r: usize, a: usize, b: usize| m.get(r).and_then(|x| x.get(a)).and_then(|x| x.get(b)).copied().unwrap_or(0.0);
    let mut out = Vec::new();
    for j in 0..cv {
        for l in 0..cv {
            for i in 0..cu {
                for k in 0..cu {
                    let e = g(&gu, 2, i, k) * g(&gv, 0, j, l) + 2.0 * g(&gu, 1, i, k) * g(&gv, 1, j, l) + g(&gu, 0, i, k) * g(&gv, 2, j, l);
                    if e != 0.0 {
                        out.push((i + cu * j, k + cu * l, e));
                    }
                }
            }
        }
    }
    Ok(out)
}

/// The interior points minimising the weighted rows' squares plus a vanishing multiple of the
/// energy, the boundary rows held: the normal equations, solved by Cholesky.
fn least_squares(
    patch: &Patch,
    rows: &[Row],
    energy: &[(usize, usize, f64)],
    m: usize,
    free: &dyn Fn(usize, usize) -> bool,
    index: &dyn Fn(usize, usize) -> usize,
) -> SplineResult<Vec<Vec3>> {
    let [cu, _] = patch.counts();
    let dim = 3 * m;
    let mut a = vec![0.0; dim * dim];
    let mut b = vec![0.0; dim];
    let mut condition_trace = 0.0;
    for r in rows {
        // The row over unknowns (index, coefficient) and its known part.
        let mut unknown: Vec<(usize, f64)> = Vec::new();
        let mut known = 0.0;
        for &(i, j, c) in &r.terms {
            let p = patch.at(i, j);
            let coords = match r.dir {
                Some(d) => [d.x, d.y, d.z],
                None => {
                    let mut e = [0.0; 3];
                    if let Some(x) = e.get_mut(r.coord) {
                        *x = 1.0;
                    }
                    e
                }
            };
            for (k, f) in coords.iter().enumerate() {
                if *f == 0.0 {
                    continue;
                }
                if free(i, j) {
                    unknown.push((3 * index(i, j) + k, c * f));
                } else {
                    known += c * f * [p.x, p.y, p.z][k];
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
    let energy_trace: f64 = energy.iter().filter(|(p, q, _)| p == q && free(p % cu, p / cu)).map(|(_, _, e)| 3.0 * e).sum();
    let lambda = if condition_trace > 0.0 && energy_trace > 0.0 { 1e-7 * condition_trace / energy_trace } else { 1.0 };
    for &(p, q, e) in energy {
        let (pi, pj, qi, qj) = (p % cu, p / cu, q % cu, q / cu);
        if !free(pi, pj) {
            continue;
        }
        let x = index(pi, pj);
        if free(qi, qj) {
            let y = index(qi, qj);
            for k in 0..3 {
                if let Some(cell) = a.get_mut((3 * x + k) * dim + 3 * y + k) {
                    *cell += lambda * e;
                }
            }
        } else {
            let fixed = patch.at(qi, qj);
            for (k, v) in [fixed.x, fixed.y, fixed.z].into_iter().enumerate() {
                if let Some(cell) = b.get_mut(3 * x + k) {
                    *cell -= lambda * e * v;
                }
            }
        }
    }
    let x = cholesky(a, b, dim)?;
    Ok((0..m)
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

/// What the patch achieves at the stations and points.
fn measure(patch: &Patch, stations: &[Station], points: &[Vec3], located: &[[f64; 2]]) -> SplineResult<Achieved> {
    let mut r = Achieved::default();
    for s in stations {
        let (uv, (du, dv, sign)) = station_on(s.side, s.t);
        let Some(normal) = patch.normal(uv[0], uv[1]) else { continue };
        let angle = normal.dot(s.across.normal).abs().clamp(0.0, 1.0).acos();
        r.normal_deviation_all = r.normal_deviation_all.max(angle);
        if s.held {
            r.normal_deviation = r.normal_deviation.max(angle);
        }
        if let Some(bend) = s.across.bend {
            let first = patch.eval(uv[0], uv[1], du, dv)? * sign;
            let second = patch.eval(uv[0], uv[1], 2 * du, 2 * dv)?;
            let speed2 = first.len2().max(1e-300);
            let mine = second.dot(s.across.normal) / speed2;
            r.curvature_mismatch = r.curvature_mismatch.max((mine - bend.dot(s.across.normal)).abs());
        }
    }
    for (p, uv) in points.iter().zip(located) {
        r.point_deviation = r.point_deviation.max(patch.eval(uv[0], uv[1], 0, 0)?.dist(*p));
    }
    Ok(r)
}

/// The parameters of the patch's point nearest `p`: the nearest of a grid of samples, then
/// Gauss–Newton inside the unit square.
fn nearest(patch: &Patch, p: Vec3) -> [f64; 2] {
    const GRID: usize = 32;
    let mut best = (f64::INFINITY, [0.5, 0.5]);
    for i in 0..=GRID {
        for j in 0..=GRID {
            let uv = [i as f64 / GRID as f64, j as f64 / GRID as f64];
            if let Ok(q) = patch.eval(uv[0], uv[1], 0, 0) {
                let d = q.dist(p);
                if d < best.0 {
                    best = (d, uv);
                }
            }
        }
    }
    let [mut u, mut v] = best.1;
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
    [u, v]
}

/// Whether the patch folds back on itself: its normal turns against the middle's somewhere on a
/// grid (clear of a three-sided patch's pole at u = 0, where the normal is undefined).
fn folds(patch: &Patch, three: bool) -> bool {
    let Some(mid) = patch.normal(0.5, 0.5) else { return true };
    for i in 0..=FOLD_SAMPLES {
        for j in 0..=FOLD_SAMPLES {
            let u = i as f64 / FOLD_SAMPLES as f64;
            if three && i == 0 {
                continue;
            }
            let v = j as f64 / FOLD_SAMPLES as f64;
            match patch.normal(u, v) {
                Some(n) if n.dot(mid) > 0.0 => {}
                _ => return true,
            }
        }
    }
    false
}

/// The patch with its middle filled out by `crown` times the loop's `size` along its normals, on
/// control points clear of the rows the conditions hold (one row from a G0 side, two from G1,
/// three from G2), so the loop and the conditions across it do not move.
fn crowned(mut patch: Patch, stations: &[Station], crown: f64, size: f64) -> SplineResult<Patch> {
    if crown == 0.0 {
        return Ok(patch);
    }
    let held = 1 + stations.iter().map(|s| if s.across.bend.is_some() { 2 } else { 1 }).max().unwrap_or(0);
    let [cu, cv] = patch.counts();
    let (gu, gv) = (greville(&patch.space[0]), greville(&patch.space[1]));
    let mut moved = 0usize;
    let base = patch.clone();
    for j in held..cv.saturating_sub(held) {
        for i in held..cu.saturating_sub(held) {
            let (x, y) = (gu.get(i).copied().unwrap_or(0.5), gv.get(j).copied().unwrap_or(0.5));
            let Some(n) = base.normal(x, y) else { continue };
            if let Some(p) = patch.net.get_mut(i + cu * j) {
                *p += n * (crown * size * 16.0 * x * (1.0 - x) * y * (1.0 - y));
                moved += 1;
            }
        }
    }
    if moved == 0 {
        return Err(SplineError::Unsupported("a crown on a patch with no room inside its held rows".into()));
    }
    Ok(patch)
}
