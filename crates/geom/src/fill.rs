//! Filling a closed loop of curves with one surface: a non-rational bicubic B-spline height
//! field over the loop's best-fit plane, fitted to the loop by least squares with a smoothness
//! term. A loop that cannot be seen as the boundary of a height field over that plane (it
//! crosses itself, folds back or runs along the plane's normal when seen along that normal) is
//! refused, never filled with wrong geometry.
//!
//! References: Piegl & Tiller, *The NURBS Book* (2nd ed.) — §9.4.1 and §9.4.3 (least squares
//! approximation: minimise the sum of squared residuals, eq. 9.62, through the normal
//! equations, eq. 9.65), §3.4 (tensor product B-spline surfaces), §3.3 (derivative of a
//! B-spline curve, eq. 3.4: control values at the knot averages reproduce a linear function),
//! A2.1 (span search).

use crate::Vec3;
use crate::bspline::{Dir, HPoint, NurbsSurface, Side, SplineError, SplineResult, find_span, invalid, local_basis_array};

/// Most loop points a fill takes.
pub const MAX_LOOP_POINTS: usize = 8192;
/// Most spans of the height field in each direction.
pub const MAX_FILL_SPANS: usize = 64;
/// How full a patch's middle may be made (`crown`): 0 the least-bending fill, positive fuller,
/// negative flatter, in fractions of the loop's size. Outside this range the value is refused.
pub const MIN_CROWN: f64 = -0.9;
pub const MAX_CROWN: f64 = 3.0;
/// Fewest spans of the height field in each direction (the first fit tried).
const MIN_FILL_SPANS: usize = 4;
/// The smoothness energy's terms: `(order in u, order in v, weight)` of `∫∫ (∂ h)²`.
const ENERGY: [(usize, usize, f64); 3] = [(2, 0, 1.0), (1, 1, 2.0), (0, 2, 1.0)];
/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]` (exact to degree 7).
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];
/// The smoothness term's weight per fitted point (see [`fill_loop`]).
pub(crate) const SMOOTHNESS: f64 = 1e-7;
/// A loop step shorter than this fraction of its length in space, seen along the normal, runs
/// along the normal (steeper than about 87° from the plane).
const MIN_STEP_SLOPE: f64 = 0.05;
/// Consecutive steps closer to reversed than this cosine fold the loop back on itself.
const FOLD_COS: f64 = -0.999;

/// A loop's fill: the surface (`S(u, v) = o + u·x + v·y + h(u, v)·n` over the plane with normal
/// `n`, so `∂S/∂u × ∂S/∂v` points along `n`), the plane's frame, whether the loop runs counter-
/// clockwise about it, the largest distance from a sampled loop point to the surface along `n`,
/// and how many points of the loop were evaluated.
#[derive(Clone, Debug, PartialEq)]
pub struct LoopFill {
    pub surface: NurbsSurface,
    /// The plane's origin and its axes `x`, `y` (the surface's parameter directions).
    pub origin: Vec3,
    pub axes: [Vec3; 2],
    pub normal: Vec3,
    pub counter_clockwise: bool,
    pub deviation: f64,
    pub evaluations: usize,
    /// The crown the fill was asked for, how many control points took its bump, and how far the
    /// bump moved the surface at the sampled loop points (always exactly 0: the bump vanishes
    /// along the loop, so the boundary is untouched).
    pub crown: f64,
    pub crowned: usize,
    pub crown_drift: f64,
}

/// Fill the closed loop of `sides` curves (side `k` given by `point(k, t)`, `t` from 0 at its
/// start to 1 at its end, which is where side `k+1` starts) with one surface passing within
/// `tol` of the loop.
///
/// - Sampling: each side at `t = i/s` (fitted) and halfway between (checked), `s` from 16
///   doubling while the fit needs denser points, at most [`MAX_LOOP_POINTS`] points in all. The
///   sample parameters are the same run either way, so the fill does not depend on where the
///   loop starts or which way it runs.
/// - The plane: through the fitted points' centroid, normal by Newell's method, turned so that
///   its largest component is positive; its first axis points to the fitted point farthest from
///   the centroid in the plane (the highest of points as far). So the plane and its axes, and with them the grid and the fill,
///   move with the loop (a reflected axis gives the same grid).
/// - Refused ("not supported yet"): a loop that, seen along the normal, has a step much shorter
///   than in space (it runs along the normal), turns straight back, or crosses or touches
///   itself; such a loop does not bound a height field over the plane.
/// - The height field: a bicubic B-spline with uniform knots over the projected loop's box
///   (widened by 1%); its control points sit at the knot averages in the plane, so its surface
///   parameters are plane coordinates, and their heights are the unknowns. The heights minimise
///   `Σ (h(uₖ, vₖ) − hₖ)² + λ·A · ∫∫ h_uu² + 2·h_uv² + h_vv²` over the fitted points — least
///   squares (eq. 9.62, normal equations eq. 9.65) plus the thin-plate bending energy over the
///   box, `A` the area of one grid cell (so each cell's energy counts like a squared second
///   difference of heights, and the balance is the same at any size). The energy decides the
///   inside, where there are no points; it is zero only for planes, so a plane through the
///   loop is reproduced exactly, and any other rim gets the least-bending surface on it, which
///   is free to turn at the rim (it is not the surface that made the rim). It does not depend
///   on the grid's directions. `λ = 10⁻⁷ ·` (fitted points) `/` (control points): just enough
///   to fix the inside, small enough not to pull the surface off the loop.
/// - Grids of 4, 8, 16, 32, 64 spans across the box's longer side are tried, with as many across
///   the shorter side as keep the cells about square; a grid whose cells are under twice the
///   fitted points' largest step in a direction (cells between points would be left to the
///   energy alone) waits for denser sampling. The first grid within `tol` of every sampled
///   point (measured along the normal, which bounds the distance) is the fill; otherwise the
///   sampling doubles, and when that runs out, "not supported yet" with the closest deviation
///   reached.
/// - `crown` (0 for the plain fill, between [`MIN_CROWN`] and [`MAX_CROWN`]) fills the middle:
///   the surface gains `crown · size · 16·s·(1−s)·t·(1−t)` (`s`, `t` the box's coordinates scaled
///   to 0…1, `size` the box's diagonal: 1 at the middle, 0 on the box's boundary), applied to the
///   control points whose support keeps clear of the loop, so the bump vanishes along the loop
///   and the boundary does not move at all. The fitted net may be too coarse to hold such a
///   point; it is then refined by knot insertion, which leaves the surface itself as it is. A
///   loop whose fill leaves no room inside it even so is refused.
pub fn fill_loop(sides: usize, point: &mut dyn FnMut(usize, f64) -> Option<Vec3>, tol: f64, crown: f64) -> SplineResult<LoopFill> {
    if sides == 0 || sides.saturating_mul(16) > MAX_LOOP_POINTS / 2 {
        return Err(SplineError::TooLarge(format!("a loop of {sides} sides (at most {})", MAX_LOOP_POINTS / 32)));
    }
    if !(tol.is_finite() && tol > 0.0) {
        return invalid("the fill tolerance must be positive");
    }
    // A range test also throws out NaN.
    if !(MIN_CROWN..=MAX_CROWN).contains(&crown) {
        return invalid("the crown must be between -0.9 and 3");
    }
    let mut evaluations = 0usize;
    let mut eval = |k: usize, t: f64| -> SplineResult<Vec3> {
        evaluations += 1;
        point(k, t).filter(|p| p.is_finite()).ok_or_else(|| SplineError::Invalid(format!("side {k} has no finite point at {t}")))
    };
    let mut per = 16usize;
    // Fitted points: side by side, per side at i/per (i < per).
    let mut fit: Vec<Vec3> = Vec::with_capacity(sides * per);
    for k in 0..sides {
        for i in 0..per {
            fit.push(eval(k, i as f64 / per as f64)?);
        }
    }
    let mut deviation = f64::INFINITY;
    loop {
        let mut check = Vec::with_capacity(fit.len());
        for k in 0..sides {
            for i in 0..per {
                check.push(eval(k, (i as f64 + 0.5) / per as f64)?);
            }
        }
        // The loop in order: fitted and checked points alternating.
        let all: Vec<Vec3> = fit.iter().zip(&check).flat_map(|(a, b)| [*a, *b]).collect();
        match fit_once(&fit, &all, tol, crown)? {
            Fit::Done(mut fill) => {
                fill.evaluations = evaluations;
                return Ok(*fill);
            }
            Fit::Denser(dev) => deviation = deviation.min(dev),
            Fit::Exhausted(dev) => {
                deviation = deviation.min(dev);
                break;
            }
        }
        if all.len() * 2 > MAX_LOOP_POINTS {
            break;
        }
        fit = all;
        per *= 2;
    }
    Err(SplineError::Unsupported(format!("filling a loop this wavy with one surface (it stays {deviation:e} off the loop, tolerance {tol:e})")))
}

/// What one round of [`fill_loop`] gave.
enum Fit {
    Done(Box<LoopFill>),
    /// Within tolerance only with cells finer than the points allow: sample denser.
    Denser(f64),
    /// The finest grid tried is not within tolerance.
    Exhausted(f64),
}

/// The plane a loop's height field stands on (see [`fill_loop`]): through the fitted points'
/// centroid `o`, normal `n` by Newell's method turned so that its largest component is positive,
/// first axis `x` towards the fitted point farthest from the centroid in the plane, `y = n × x`;
/// and whether the points run counter-clockwise about `n`.
#[derive(Clone, Copy, Debug)]
pub(crate) struct LoopFrame {
    pub(crate) o: Vec3,
    pub(crate) x: Vec3,
    pub(crate) y: Vec3,
    pub(crate) n: Vec3,
    pub(crate) counter_clockwise: bool,
}

/// The [`LoopFrame`] of the loop of points `fit`.
pub(crate) fn loop_frame(fit: &[Vec3]) -> SplineResult<LoopFrame> {
    let m = fit.len();
    if m < 3 {
        return invalid("a loop needs three points");
    }
    let o = fit.iter().fold(Vec3::ZERO, |a, p| a + *p) / m as f64;
    let at = |k: usize| fit.get(k % m).copied().unwrap_or(Vec3::ZERO);
    let area = (0..m).fold(Vec3::ZERO, |a, k| a + (at(k) - o).cross(at(k + 1) - o));
    let Some(normal) = area.normalized() else { return invalid("the loop encloses no area") };
    let largest = [normal.x, normal.y, normal.z].into_iter().fold(0.0f64, |a, c| if c.abs() > a.abs() { c } else { a });
    let counter_clockwise = largest > 0.0;
    let n = if counter_clockwise { normal } else { normal * -1.0 };
    // The first axis towards the fitted point farthest from the centroid in the plane: the axes,
    // the grid and so the fill move with the loop.
    let flat = |p: &Vec3| {
        let d = *p - o;
        d - n * d.dot(n)
    };
    // Points as far as each other (within rounding) are told apart by their height, and points
    // alike in both by their coordinates: the same point is chosen however the loop is given,
    // and as the loop moves, but for ties its symmetry makes.
    let key = |p: &Vec3| (flat(p).len(), (*p - o).dot(n));
    let reach = fit.iter().map(|p| key(p).0).fold(0.0, f64::max);
    let near = 1e-9 * reach;
    let far = fit.iter().fold(None::<&Vec3>, |best, p| match best {
        None => Some(p),
        Some(b) => {
            let ((db, hb), (dp, hp)) = (key(b), key(p));
            let better = if (dp - db).abs() > near {
                dp > db
            } else if (hp - hb).abs() > near {
                hp > hb
            } else {
                (p.x, p.y, p.z) > (b.x, b.y, b.z)
            };
            Some(if better { p } else { b })
        }
    });
    let far = far.map(flat).unwrap_or(Vec3::ZERO);
    let x = far.normalized().ok_or_else(|| SplineError::Invalid("the loop has no extent in its plane".into()))?;
    let y = n.cross(x);
    Ok(LoopFrame { o, x, y, n, counter_clockwise })
}

/// One round of [`fill_loop`]: fit the points `fit`, check every point of `all` (the loop in
/// order, `fit` among them).
fn fit_once(fit: &[Vec3], all: &[Vec3], tol: f64, crown: f64) -> SplineResult<Fit> {
    let m = fit.len();
    let LoopFrame { o, x, y, n, counter_clockwise } = loop_frame(fit)?;
    let project = |pts: &[Vec3]| -> (Vec<[f64; 2]>, Vec<f64>) {
        (pts.iter().map(|p| [(*p - o).dot(x), (*p - o).dot(y)]).collect(), pts.iter().map(|p| (*p - o).dot(n)).collect())
    };
    let (uv, h) = project(all);
    check_projectable(all, &uv)?;
    let (fuv, fh) = project(fit);
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in &uv {
        for d in 0..2 {
            lo[d] = lo[d].min(q[d]);
            hi[d] = hi[d].max(q[d]);
        }
    }
    for d in 0..2 {
        let pad = 0.01 * (hi[d] - lo[d]);
        lo[d] -= pad;
        hi[d] += pad;
    }
    if !(lo[0] < hi[0] && lo[1] < hi[1] && (hi[0] - lo[0]).is_finite() && (hi[1] - lo[1]).is_finite()) {
        return invalid("the loop's extent is not finite");
    }
    // Spans per direction: `level` across the longer side of the box, as many across the
    // shorter as keep the cells about square (at least one). A grid finer than the fitted
    // points can hold in a direction (cells under twice their largest step along it) is tried
    // only after denser sampling; the coarsest is always tried.
    let extent = [hi[0] - lo[0], hi[1] - lo[1]];
    let mut step = [0.0f64; 2];
    for k in 0..m {
        let (p, q) = (fuv.get(k).copied().unwrap_or([0.0; 2]), fuv.get((k + 1) % m).copied().unwrap_or([0.0; 2]));
        for d in 0..2 {
            step[d] = step[d].max((q[d] - p[d]).abs());
        }
    }
    let longer = extent[0].max(extent[1]);
    let mut deviation = f64::INFINITY;
    let mut level = MIN_FILL_SPANS;
    while level <= MAX_FILL_SPANS {
        let spans = [0, 1].map(|d| ((extent[d] / longer * level as f64).ceil() as usize).clamp(1, level));
        let holds = (0..2).all(|d| extent[d] / spans[d] as f64 >= 2.0 * step[d]);
        if !holds && level > MIN_FILL_SPANS {
            return Ok(Fit::Denser(deviation));
        }
        let grid = Grid::new(spans, lo, hi);
        let heights = grid.fit(&fuv, &fh)?;
        let dev = uv.iter().zip(&h).map(|(q, hk)| (grid.height(&heights, *q) - hk).abs()).fold(0.0, f64::max);
        if !dev.is_finite() {
            return invalid("the fit is not finite");
        }
        deviation = deviation.min(dev);
        if dev <= tol && holds {
            let surface = grid.surface(&heights, o, x, y, n)?;
            let (surface, crowned, crown_drift) = crown_fill(surface, &uv, crown, n)?;
            let fill = LoopFill {
                surface,
                origin: o,
                axes: [x, y],
                normal: n,
                counter_clockwise,
                deviation: dev,
                evaluations: 0,
                crown,
                crowned,
                crown_drift,
            };
            return Ok(Fit::Done(Box::new(fill)));
        }
        if !holds {
            return Ok(Fit::Denser(deviation));
        }
        level *= 2;
    }
    Ok(Fit::Exhausted(deviation))
}

/// The fitted surface with the crown's bump (see [`fill_loop`]): `crown == 0` gives it back as it
/// is. A control point whose support rectangle keeps clear of the loop takes the bump's blossom
/// coefficients, so the surface between the perturbed points is the bubble exactly and the
/// surface over the loop is not moved at all (those points' basis functions are zero all along
/// it). A fitted net is often too coarse for that; it is then refined by knot insertion, which
/// leaves the surface itself as it is, until every point whose basis function is not zero at the
/// box's middle takes the bump (where the middle is inside the loop), so the middle rises by the
/// bubble's full `crown · size` there, as documented, not by the share of it a coarse net's few
/// clear points carry. Returns the crowned surface, how many
/// control points took the bump, and the farthest the bump moved the surface at a sampled loop
/// point (0).
fn crown_fill(surface: NurbsSurface, uv: &[[f64; 2]], crown: f64, normal: Vec3) -> SplineResult<(NurbsSurface, usize, f64)> {
    if crown == 0.0 {
        return Ok((surface, 0, 0.0));
    }
    if !normal.is_finite() {
        return invalid("the loop's extent is not finite");
    }
    crown_fill_along(surface, uv, crown, &|_| Some(normal), MAX_FILL_SPANS)
}

/// [`crown_fill`] moving each control point along `along(ξ, η)` (at its Greville point, the mean
/// of its knots each way) rather than one normal: what a fill that is not a height field takes,
/// with its own normal there. Its net may be refined up to `max_spans` each way.
pub(crate) fn crown_fill_along(
    surface: NurbsSurface,
    uv: &[[f64; 2]],
    crown: f64,
    along: &dyn Fn([f64; 2]) -> Option<Vec3>,
    max_spans: usize,
) -> SplineResult<(NurbsSurface, usize, f64)> {
    if crown == 0.0 {
        return Ok((surface, 0, 0.0));
    }
    let gap = loop_gap(uv);
    let mut base = surface;
    loop {
        if let Some((crowned, points)) = crown_net(&base, uv, gap, crown, along)?.filter(|c| c.2).map(|(c, n, _)| (c, n)) {
            let mut drift = 0.0f64;
            for q in uv {
                if let (Ok(a), Ok(b)) = (base.eval(q[0], q[1]), crowned.eval(q[0], q[1])) {
                    drift = drift.max(a.dist(b));
                }
            }
            if drift > 0.0 {
                return Err(SplineError::Unsupported("crowning a patch whose loop the fill cannot keep clear".into()));
            }
            return Ok((crowned, points, drift));
        }
        // No room inside the loop yet: halve the cells (the surface itself does not change).
        let spans = [0, 1].map(|d| base.count()[d].saturating_sub(3));
        if spans[0] >= max_spans && spans[1] >= max_spans {
            return Err(SplineError::Unsupported("crowning a patch whose loop leaves no room inside it".into()));
        }
        for (d, &s) in spans.iter().enumerate() {
            if s < max_spans {
                base = subdivide(&base, d)?;
            }
        }
    }
}

/// The crown's bump in the net `base`: a control point whose support rectangle keeps clear of the
/// loop takes `crown · size · B(ξ, η)`, `B` the bubble's blossom at the point's knots (so the
/// surface between the perturbed points is exactly the bubble, not a fit of it) and `size` the
/// box's diagonal. `None` when the net has no such point; with it, whether every point
/// supporting the box's middle took the bump (or the middle is outside the loop).
fn crown_net(
    base: &NurbsSurface,
    uv: &[[f64; 2]],
    gap: [f64; 2],
    crown: f64,
    along: &dyn Fn([f64; 2]) -> Option<Vec3>,
) -> SplineResult<Option<(NurbsSurface, usize, bool)>> {
    let [cu, cv] = base.count();
    let degree = base.degree();
    let knots = base.knots();
    let domain = base.domain();
    let size = (domain[0][1] - domain[0][0]).hypot(domain[1][1] - domain[1][0]);
    if !(size > 0.0 && size.is_finite()) {
        return invalid("the loop's extent is not finite");
    }
    // The bubble `4·s·(1−s)` per direction, as a quadratic in the plane coordinate.
    let quad = |d: usize| {
        let (lo, hi) = (domain[d][0], domain[d][1]);
        let w = hi - lo;
        (-4.0 * lo * hi / (w * w), 4.0 * (lo + hi) / (w * w), -4.0 / (w * w))
    };
    let (qu, qv) = (quad(0), quad(1));
    let (Some(knots_u), Some(knots_v)) = (knots.first(), knots.get(1)) else { return invalid("the surface has no knot vectors") };
    let mut ctrl = base.homogeneous().to_vec();
    let mut points = 0usize;
    let middle = [0.5 * (domain[0][0] + domain[0][1]), 0.5 * (domain[1][0] + domain[1][1])];
    // The points whose basis is not zero at the middle, and those of them that took the bump.
    let (mut at_middle, mut raised_middle) = (0usize, 0usize);
    let holds = |span: [f64; 2], x: f64| span[0] < x && x < span[1];
    for j in 0..cv {
        let Some((v_span, v_knots)) = support(knots_v, degree[1], j) else { continue };
        for i in 0..cu {
            let Some((u_span, u_knots)) = support(knots_u, degree[0], i) else { continue };
            let supports = holds(u_span, middle[0]) && holds(v_span, middle[1]);
            at_middle += usize::from(supports);
            if !clear_of_loop(u_span, v_span, uv, gap) {
                continue;
            }
            let bump = crown * size * blossom(qu, u_knots) * blossom(qv, v_knots);
            let mean = |t: [f64; 3]| (t[0] + t[1] + t[2]) / 3.0;
            let Some(normal) = along([mean(u_knots), mean(v_knots)]).filter(|n| n.is_finite()) else { continue };
            let Some(slot) = ctrl.get_mut(j * cu + i) else { continue };
            let h = *slot;
            let Some(p) = h.project() else { continue };
            *slot = HPoint::weighted(p + normal * bump, h.w);
            points += 1;
            raised_middle += usize::from(supports);
        }
    }
    if points == 0 {
        return Ok(None);
    }
    let middle_held = raised_middle == at_middle || !inside_loop(middle, uv);
    Ok(Some((NurbsSurface::from_homogeneous(degree, knots.clone(), ctrl)?, points, middle_held)))
}

/// The same surface with a knot inserted in the middle of every span of direction `d` (0 = u):
/// the surface itself does not change, only its net is finer, which is what gives the crown room
/// inside the loop.
fn subdivide(surface: &NurbsSurface, d: usize) -> SplineResult<NurbsSurface> {
    let (degree, count) = (surface.degree()[d], surface.count()[d]);
    let knots = surface.knots()[d].clone();
    let mut mids = Vec::new();
    for i in degree..count {
        let (a, b) = (knots.get(i).copied().unwrap_or(0.0), knots.get(i + 1).copied().unwrap_or(0.0));
        if b > a {
            mids.push(0.5 * (a + b));
        }
    }
    let mut out = surface.clone();
    for m in mids {
        out = out.insert_knot(if d == 0 { Dir::S } else { Dir::T }, m)?;
    }
    Ok(out)
}

/// A control point's knot interval — where its basis is non-zero — and its knot triple, whose
/// blossom is the value the point must take for the spline to reproduce a quadratic exactly.
fn support(knots: &[f64], degree: usize, i: usize) -> Option<([f64; 2], [f64; 3])> {
    let interval = [*knots.get(i)?, *knots.get(i + degree + 1)?];
    let triple = [*knots.get(i + 1)?, *knots.get(i + 2)?, *knots.get(i + 3)?];
    (interval[1] >= interval[0]).then_some((interval, triple))
}

/// A quadratic `a + b·x + c·x²`'s blossom at a knot triple.
fn blossom(quad: (f64, f64, f64), t: [f64; 3]) -> f64 {
    let (a, b, c) = quad;
    let mean = (t[0] + t[1] + t[2]) / 3.0;
    let pair = (t[0] * t[1] + t[1] * t[2] + t[0] * t[2]) / 3.0;
    a + b * mean + c * pair
}

/// Whether the parameter rectangle `u` × `v` keeps clear of the sampled loop: its middle is
/// inside the loop and no loop segment comes within `gap` of it. A control point whose support
/// keeps clear takes the crown's bump without moving the surface over the loop.
fn clear_of_loop(u: [f64; 2], v: [f64; 2], uv: &[[f64; 2]], gap: [f64; 2]) -> bool {
    if !inside_loop([(u[0] + u[1]) / 2.0, (v[0] + v[1]) / 2.0], uv) {
        return false;
    }
    let lo = [u[0] - gap[0], v[0] - gap[1]];
    let hi = [u[1] + gap[0], v[1] + gap[1]];
    let m = uv.len();
    (0..m).all(|k| {
        let (Some(a), Some(b)) = (uv.get(k).copied(), uv.get((k + 1) % m).copied()) else { return false };
        !segment_meets_rect(a, b, lo, hi)
    })
}

/// The largest step between neighbouring samples of the loop, per direction: a rectangle this far
/// clear of the sampled loop cannot be entered by the edges between the samples either.
fn loop_gap(uv: &[[f64; 2]]) -> [f64; 2] {
    let m = uv.len();
    let mut gap = [0.0f64; 2];
    for k in 0..m {
        let (Some(a), Some(b)) = (uv.get(k).copied(), uv.get((k + 1) % m).copied()) else { continue };
        for d in 0..2 {
            gap[d] = gap[d].max((b[d] - a[d]).abs());
        }
    }
    gap
}

/// Whether `p` is inside the closed loop `uv` (ray casting).
pub(crate) fn inside_loop(p: [f64; 2], uv: &[[f64; 2]]) -> bool {
    let m = uv.len();
    let mut inside = false;
    for k in 0..m {
        let (Some(a), Some(b)) = (uv.get(k).copied(), uv.get((k + 1) % m).copied()) else { continue };
        if (a[1] > p[1]) != (b[1] > p[1]) && p[0] < a[0] + (p[1] - a[1]) / (b[1] - a[1]) * (b[0] - a[0]) {
            inside = !inside;
        }
    }
    inside
}

/// Whether the segment `p`–`q` meets the rectangle `lo`–`hi` (Liang–Barsky clipping).
fn segment_meets_rect(p: [f64; 2], q: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> bool {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for d in 0..2 {
        let delta = q[d] - p[d];
        if delta == 0.0 {
            if p[d] < lo[d] || p[d] > hi[d] {
                return false;
            }
            continue;
        }
        let (mut a, mut b) = ((lo[d] - p[d]) / delta, (hi[d] - p[d]) / delta);
        if a > b {
            std::mem::swap(&mut a, &mut b);
        }
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return false;
        }
    }
    true
}

/// Refuse a loop that does not bound a height field over its plane (see [`fill_loop`]).
pub(crate) fn check_projectable(points: &[Vec3], uv: &[[f64; 2]]) -> SplineResult<()> {
    let m = uv.len();
    let q = |k: usize| uv.get(k % m).copied().unwrap_or([0.0; 2]);
    let p = |k: usize| points.get(k % m).copied().unwrap_or(Vec3::ZERO);
    let step = |k: usize| {
        let (a, b) = (q(k), q(k + 1));
        [b[0] - a[0], b[1] - a[1]]
    };
    for k in 0..m {
        let d = step(k);
        let flat = d[0].hypot(d[1]);
        let space = p(k).dist(p(k + 1));
        if space > 0.0 && flat < MIN_STEP_SLOPE * space {
            return Err(SplineError::Unsupported("filling a loop that runs along its plane's normal".into()));
        }
        let e = step(k + 1);
        let fe = e[0].hypot(e[1]);
        if flat > 0.0 && fe > 0.0 && (d[0] * e[0] + d[1] * e[1]) < FOLD_COS * flat * fe {
            return Err(SplineError::Unsupported("filling a loop that folds back on itself".into()));
        }
    }
    // Non-adjacent segments must not meet: a sweep over the segments' u extents.
    let mut segs: Vec<(f64, f64, usize)> = (0..m)
        .map(|k| {
            let (a, b) = (q(k), q(k + 1));
            (a[0].min(b[0]), a[0].max(b[0]), k)
        })
        .collect();
    segs.sort_by(|a, b| a.0.total_cmp(&b.0));
    for (i, &(_, hi, k)) in segs.iter().enumerate() {
        for &(lo2, _, l) in segs.get(i + 1..).into_iter().flatten() {
            if lo2 > hi {
                break;
            }
            let adjacent = (k + 1) % m == l || (l + 1) % m == k;
            if !adjacent && segments_meet(q(k), q(k + 1), q(l), q(l + 1)) {
                return Err(SplineError::Unsupported("filling a loop that crosses itself seen along its plane's normal".into()));
            }
        }
    }
    Ok(())
}

/// Whether the closed segments `ab` and `cd` share a point.
pub(crate) fn segments_meet(a: [f64; 2], b: [f64; 2], c: [f64; 2], d: [f64; 2]) -> bool {
    let orient = |p: [f64; 2], q: [f64; 2], r: [f64; 2]| (q[0] - p[0]) * (r[1] - p[1]) - (q[1] - p[1]) * (r[0] - p[0]);
    let within =
        |p: [f64; 2], q: [f64; 2], r: [f64; 2]| r[0] >= p[0].min(q[0]) && r[0] <= p[0].max(q[0]) && r[1] >= p[1].min(q[1]) && r[1] <= p[1].max(q[1]);
    let (d1, d2, d3, d4) = (orient(c, d, a), orient(c, d, b), orient(a, b, c), orient(a, b, d));
    if ((d1 > 0.0 && d2 < 0.0) || (d1 < 0.0 && d2 > 0.0)) && ((d3 > 0.0 && d4 < 0.0) || (d3 < 0.0 && d4 > 0.0)) {
        return true;
    }
    (d1 == 0.0 && within(c, d, a)) || (d2 == 0.0 && within(c, d, b)) || (d3 == 0.0 && within(a, b, c)) || (d4 == 0.0 && within(a, b, d))
}

/// A bicubic height field's grid: uniform clamped knots over `[lo, hi]` in each direction,
/// `count` control points along each.
pub(crate) struct Grid {
    pub(crate) knots: [Vec<f64>; 2],
    pub(crate) count: [usize; 2],
}

impl LoopFill {
    /// The surface's point over (or under) `p`, along the normal.
    pub fn point_over(&self, p: Vec3) -> SplineResult<Vec3> {
        let d = p - self.origin;
        let [x, y] = self.axes;
        self.surface.eval(d.dot(x), d.dot(y))
    }
}

impl Grid {
    pub(crate) fn new(spans: [usize; 2], lo: [f64; 2], hi: [f64; 2]) -> Grid {
        let knots = [0, 1].map(|d| {
            let (a, b, s) = (lo[d], hi[d], spans[d]);
            let mut k = vec![a; 4];
            k.extend((1..s).map(|i| a + (b - a) * i as f64 / s as f64));
            k.extend([b; 4]);
            k
        });
        Grid { knots, count: spans.map(|s| s + 3) }
    }

    /// The length of one span in direction `d`.
    pub(crate) fn cell(&self, d: usize) -> f64 {
        let (Some(k), Some(&c)) = (self.knots.get(d), self.count.get(d)) else { return 0.0 };
        (k.last().copied().unwrap_or(0.0) - k.first().copied().unwrap_or(0.0)) / (c - 3) as f64
    }

    /// The knot average `(kᵢ₊₁ + kᵢ₊₂ + kᵢ₊₃)/3` of control point `i` in direction `d`.
    fn average(&self, d: usize, i: usize) -> f64 {
        self.knots.get(d).map_or(0.0, |k| (1..=3).map(|t| k.get(i + t).copied().unwrap_or(0.0)).sum::<f64>() / 3.0)
    }

    /// The four non-zero basis functions' `r`-th derivatives and the first index, at `x` in
    /// direction `d`.
    pub(crate) fn basis_derivative(&self, d: usize, x: f64, r: usize) -> Option<(usize, [f64; 4])> {
        let kn = self.knots.get(d)?;
        let x = x.clamp(*kn.first()?, *kn.last()?);
        let span = find_span(kn, 3, *self.count.get(d)?, x)?;
        let side = if x >= *kn.last()? { Side::Left } else { Side::Right };
        let first = span.checked_sub(3)?;
        let mut out = [0.0; 4];
        for (a, o) in out.iter_mut().enumerate() {
            *o = kn.get(first + a..first + a + 5).map_or(0.0, |l| local_basis_array(l, x, side, r).get(r).copied().unwrap_or(0.0));
        }
        Some((first, out))
    }

    /// The height at `q` of the net `heights` (i fastest).
    fn height(&self, heights: &[f64], q: [f64; 2]) -> f64 {
        let (Some((i0, bu)), Some((j0, bv))) = (self.basis_derivative(0, q[0], 0), self.basis_derivative(1, q[1], 0)) else { return f64::NAN };
        let mut s = 0.0;
        for (b, wv) in bv.iter().enumerate() {
            for (a, wu) in bu.iter().enumerate() {
                s += wu * wv * heights.get((j0 + b) * self.count[0] + i0 + a).copied().unwrap_or(f64::NAN);
            }
        }
        s
    }

    /// The control heights fitting the points (see [`fill_loop`]): the normal equations, banded
    /// (unknowns i fastest, so coupled unknowns are at most `3·count[0] + 3` apart), solved by
    /// Cholesky factorisation.
    fn fit(&self, uv: &[[f64; 2]], h: &[f64]) -> SplineResult<Vec<f64>> {
        let [cu, cv] = self.count;
        let n = cu * cv;
        let mut band = Band::new(n, 3 * cu + 3);
        let mut rhs = vec![0.0; n];
        let mut fitted = 0usize;
        for (q, hk) in uv.iter().zip(h) {
            let (Some((i0, bu)), Some((j0, bv))) = (self.basis_derivative(0, q[0], 0), self.basis_derivative(1, q[1], 0)) else {
                return invalid("a loop point is off the grid");
            };
            let mut terms = [(0usize, 0.0f64); 16];
            for (t, slot) in terms.iter_mut().enumerate() {
                let (a, b) = (t % 4, t / 4);
                *slot = ((j0 + b) * cu + i0 + a, bu.get(a).copied().unwrap_or(0.0) * bv.get(b).copied().unwrap_or(0.0));
            }
            for &(r, wr) in &terms {
                if let Some(slot) = rhs.get_mut(r) {
                    *slot += wr * hk;
                }
                for &(s, ws) in &terms {
                    band.add(r, s, wr * ws);
                }
            }
            fitted += 1;
        }
        // The thin-plate energy ∫∫ h_uu² + 2·h_uv² + h_vv² over the grid's box, times the area of
        // one cell: per cell that is the squared second difference of the heights, so it weighs
        // against the data term (squared heights) alike at any size. Each term is a product of
        // one-direction Gram matrices of the basis functions' derivatives.
        let lambda = SMOOTHNESS * fitted as f64 / n as f64 * self.cell(0) * self.cell(1);
        self.add_energy(&mut band, lambda);
        band.solve(rhs)
    }

    /// Add `lambda` times the thin-plate energy `∫∫ h_uu² + 2·h_uv² + h_vv²` over the grid's box
    /// to the normal equations `band` (unknowns i fastest): each term is a product of
    /// one-direction Gram matrices of the basis functions' derivatives.
    pub(crate) fn add_energy(&self, band: &mut Band, lambda: f64) {
        let cu = self.count[0];
        self.energy(lambda, &mut |(i, j), (k, l), e| band.add(j * cu + i, l * cu + k, e));
    }

    /// The entries of `lambda` times the thin-plate energy's matrix (see [`Grid::add_energy`]),
    /// control point `(i, j)` against `(k, l)`, for every pair whose supports overlap.
    pub(crate) fn energy(&self, lambda: f64, add: &mut dyn FnMut((usize, usize), (usize, usize), f64)) {
        let [cu, cv] = self.count;
        let gram = |d: usize, r: usize| -> Vec<f64> {
            let c = self.count.get(d).copied().unwrap_or(0);
            let mut g = vec![0.0; c * c];
            let Some(k) = self.knots.get(d) else { return g };
            for w in k.windows(2) {
                let &[a, b] = w else { continue };
                if !(b > a) {
                    continue;
                }
                for (gx, gw) in GAUSS4 {
                    let x = 0.5 * (a + b) + 0.5 * (b - a) * gx;
                    let Some((first, vals)) = self.basis_derivative(d, x, r) else { continue };
                    for (i, vi) in vals.iter().enumerate() {
                        for (j, vj) in vals.iter().enumerate() {
                            if let Some(slot) = g.get_mut((first + i) * c + first + j) {
                                *slot += 0.5 * (b - a) * gw * vi * vj;
                            }
                        }
                    }
                }
            }
            g
        };
        let grams: [[Vec<f64>; 3]; 2] = [0, 1].map(|d| [0, 1, 2].map(|r| gram(d, r)));
        let g = |d: usize, r: usize, i: usize, k: usize| {
            let c = self.count.get(d).copied().unwrap_or(0);
            grams.get(d).and_then(|m| m.get(r)).and_then(|m| m.get(i * c + k)).copied().unwrap_or(0.0)
        };
        for j in 0..cv {
            for i in 0..cu {
                for l in j.saturating_sub(3)..(j + 4).min(cv) {
                    for k in i.saturating_sub(3)..(i + 4).min(cu) {
                        let e: f64 = ENERGY.iter().map(|&(ru, rv, w)| w * g(0, ru, i, k) * g(1, rv, j, l)).sum();
                        add((i, j), (k, l), lambda * e);
                    }
                }
            }
        }
    }

    /// The surface of the net `heights` over the plane `o`, `x`, `y`, `n`: control point `(i, j)`
    /// at the knot averages `(ξᵢ, ηⱼ)` in the plane, `heights[j·count[0] + i]` along `n`.
    pub(crate) fn surface(&self, heights: &[f64], o: Vec3, x: Vec3, y: Vec3, n: Vec3) -> SplineResult<NurbsSurface> {
        let [cu, cv] = self.count;
        let mut ctrl = Vec::with_capacity(cu * cv);
        for j in 0..cv {
            for i in 0..cu {
                let hij = heights.get(j * cu + i).copied().unwrap_or(f64::NAN);
                ctrl.push(HPoint::weighted(o + x * self.average(0, i) + y * self.average(1, j) + n * hij, 1.0));
            }
        }
        NurbsSurface::from_homogeneous([3, 3], self.knots.clone(), ctrl)
    }
}

/// A symmetric banded matrix: the upper band, row `r` holding columns `r ..= r + bw`.
#[derive(Clone)]
pub(crate) struct Band {
    n: usize,
    bw: usize,
    a: Vec<f64>,
}

impl Band {
    pub(crate) fn new(n: usize, bw: usize) -> Band {
        Band { n, bw, a: vec![0.0; n * (bw + 1)] }
    }
    fn index(&self, r: usize, s: usize) -> Option<usize> {
        let (r, s) = if r <= s { (r, s) } else { (s, r) };
        (s - r <= self.bw && s < self.n).then_some(r * (self.bw + 1) + (s - r))
    }
    fn get(&self, r: usize, s: usize) -> f64 {
        self.index(r, s).and_then(|i| self.a.get(i)).copied().unwrap_or(0.0)
    }
    fn set(&mut self, r: usize, s: usize, v: f64) {
        if let Some(x) = self.index(r, s).and_then(|i| self.a.get_mut(i)) {
            *x = v;
        }
    }
    /// Add `v` at `(r, s)` with `r ≤ s` (the lower half is the same by symmetry: callers adding
    /// a full outer product reach each stored entry once through `r ≤ s`).
    pub(crate) fn add(&mut self, r: usize, s: usize, v: f64) {
        if r <= s
            && let Some(x) = self.index(r, s).and_then(|i| self.a.get_mut(i))
        {
            *x += v;
        }
    }
    /// Solve `A·x = b` by banded Cholesky factorisation `A = Uᵀ·U`; an error when `A` is not
    /// positive definite.
    pub(crate) fn solve(self, b: Vec<f64>) -> SplineResult<Vec<f64>> {
        self.factor()?.substitute(b)
    }

    /// The Cholesky factor `U` of `A = Uᵀ·U`, in place; an error when `A` is not positive
    /// definite.
    pub(crate) fn factor(mut self) -> SplineResult<Band> {
        let (n, bw) = (self.n, self.bw);
        for r in 0..n {
            let mut d = self.get(r, r);
            for k in r.saturating_sub(bw)..r {
                let u = self.get(k, r);
                d -= u * u;
            }
            if !(d > 0.0) {
                return invalid("the fit's equations are singular");
            }
            let d = d.sqrt();
            self.set(r, r, d);
            for s in r + 1..(r + bw + 1).min(n) {
                let mut v = self.get(r, s);
                for k in s.saturating_sub(bw)..r {
                    v -= self.get(k, r) * self.get(k, s);
                }
                self.set(r, s, v / d);
            }
        }
        Ok(self)
    }

    /// [`Band::factor`] by rows: once row `r` of `U` is known, the rows below it within the band
    /// are updated from it at once (right-looking), which walks memory row by row instead of down
    /// columns. The same factor, to rounding; what the general fill, whose bands are wide, uses.
    pub(crate) fn factor_rows(mut self) -> SplineResult<Band> {
        let (n, bw) = (self.n, self.bw);
        let w = bw + 1;
        let mut row = vec![0.0; w];
        for r in 0..n {
            let Some(here) = self.a.get_mut(r * w..r * w + w) else { return invalid("the fit's equations are out of shape") };
            let d = here.first().copied().unwrap_or(0.0);
            if !(d > 0.0) {
                return invalid("the fit's equations are singular");
            }
            let d = d.sqrt();
            for (k, x) in here.iter_mut().enumerate() {
                *x = if k == 0 { d } else { *x / d };
            }
            row.copy_from_slice(here);
            let end = (r + bw + 1).min(n);
            for s in r + 1..end {
                let us = row.get(s - r).copied().unwrap_or(0.0);
                if us == 0.0 {
                    continue;
                }
                let len = end - s;
                let (Some(dst), Some(src)) = (self.a.get_mut(s * w..s * w + len), row.get(s - r..s - r + len)) else {
                    return invalid("the fit's equations are out of shape");
                };
                for (x, y) in dst.iter_mut().zip(src) {
                    *x -= us * y;
                }
            }
        }
        Ok(self)
    }

    /// Solve `Uᵀ·U·x = b` with a factor from [`Band::factor`].
    pub(crate) fn substitute(&self, mut b: Vec<f64>) -> SplineResult<Vec<f64>> {
        let (n, bw) = (self.n, self.bw);
        // Uᵀ·y = b, then U·x = y.
        for r in 0..n {
            let mut v = b.get(r).copied().unwrap_or(0.0);
            for k in r.saturating_sub(bw)..r {
                v -= self.get(k, r) * b.get(k).copied().unwrap_or(0.0);
            }
            if let Some(x) = b.get_mut(r) {
                *x = v / self.get(r, r);
            }
        }
        for r in (0..n).rev() {
            let mut v = b.get(r).copied().unwrap_or(0.0);
            for s in r + 1..(r + bw + 1).min(n) {
                v -= self.get(r, s) * b.get(s).copied().unwrap_or(0.0);
            }
            if let Some(x) = b.get_mut(r) {
                *x = v / self.get(r, r);
            }
        }
        if b.iter().any(|x| !x.is_finite()) {
            return invalid("the fit is not finite");
        }
        Ok(b)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The closed polygon through `corners` in space, side `k` from corner `k` to `k+1`.
    fn polygon3(corners: &[Vec3]) -> impl FnMut(usize, f64) -> Option<Vec3> + '_ {
        move |k, t| {
            let n = corners.len();
            Some(corners.get(k)?.lerp(*corners.get((k + 1) % n)?, t))
        }
    }

    /// The polygon `corners` in the plane, lifted by `z(x, y)` all along.
    fn lifted<'a>(corners: &'a [[f64; 2]], z: &'a dyn Fn(f64, f64) -> f64) -> impl FnMut(usize, f64) -> Option<Vec3> + 'a {
        move |k, t| {
            let n = corners.len();
            let (a, b) = (corners.get(k)?, corners.get((k + 1) % n)?);
            let (x, y) = (a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t);
            Some(Vec3::new(x, y, z(x, y)))
        }
    }

    fn ring(n: usize, r: f64) -> Vec<[f64; 2]> {
        (0..n).map(|k| k as f64 * std::f64::consts::TAU / n as f64).map(|a| [r * a.cos(), r * a.sin()]).collect()
    }

    /// The farthest the surface is from `z` over the polygon's inside (sampled on a grid).
    fn off_inside(f: &LoopFill, corners: &[[f64; 2]], z: &dyn Fn(f64, f64) -> f64) -> f64 {
        let polygon = crate::Ring::from(corners.to_vec());
        let inside = |x: f64, y: f64| polygon.contains([x, y]);
        let s = &f.surface;
        let [du, dv] = s.domain();
        let mut worst = 0.0f64;
        for i in 0..=40 {
            for j in 0..=40 {
                let (u, v) = ((du[0] + (du[1] - du[0]) * i as f64 / 40.0).min(du[1]), (dv[0] + (dv[1] - dv[0]) * j as f64 / 40.0).min(dv[1]));
                let p = s.eval(u, v).unwrap();
                if inside(p.x, p.y) {
                    worst = worst.max((p.z - z(p.x, p.y)).abs());
                }
            }
        }
        worst
    }

    #[test]
    fn planes_are_reproduced_and_rims_of_quadrics_are_filled_closely() {
        // A tilted plane over a hexagon: the smoothness energy vanishes on it, so the fill is it.
        let hex = ring(6, 10.0);
        let plane = |x: f64, y: f64| 0.3 * x - 0.2 * y + 1.0;
        let f = fill_loop(6, &mut lifted(&hex, &plane), 1e-6, 0.0).unwrap();
        let inside = off_inside(&f, &hex, &plane);
        eprintln!("plane: deviation {:e}, inside off by {inside:e}, {} evaluations", f.deviation, f.evaluations);
        assert!(f.deviation <= 1e-6 && inside < 1e-9);
        // A saddle and a dome: the least-bending fill through the rim is not the quadric (only
        // planes are reproduced), but stays near it (the plate is free to turn at the rim).
        let saddle = |x: f64, y: f64| 0.05 * x * y;
        let dome = |x: f64, y: f64| 3.0 - 0.02 * (x * x + y * y);
        for (name, z, bound) in [("saddle", &saddle as &dyn Fn(f64, f64) -> f64, 0.1), ("dome", &dome, 1.0)] {
            let f = fill_loop(6, &mut lifted(&hex, z), 2e-5, 0.0).unwrap();
            let inside = off_inside(&f, &hex, z);
            eprintln!("{name}: deviation {:e}, inside off by {inside:e}", f.deviation);
            assert!(f.deviation <= 2e-5 && inside < bound, "{name}: {inside}");
        }
    }

    #[test]
    fn concave_loops_fill_and_the_result_does_not_depend_on_where_the_loop_starts() {
        // A dart, an arrow-head pentagon and a plus, on a cylinder.
        let z = |x: f64, _: f64| 50.0 - (1600.0 - x * x).sqrt();
        let dart = vec![[0.0, 0.0], [20.0, -10.0], [8.0, 0.0], [20.0, 10.0]];
        let arrow = vec![[0.0, 0.0], [10.0, -8.0], [10.0, -3.0], [20.0, 0.0], [10.0, 3.0]];
        let plus = vec![
            [-3.0, -9.0],
            [3.0, -9.0],
            [3.0, -3.0],
            [9.0, -3.0],
            [9.0, 3.0],
            [3.0, 3.0],
            [3.0, 9.0],
            [-3.0, 9.0],
            [-3.0, 3.0],
            [-9.0, 3.0],
            [-9.0, -3.0],
            [-3.0, -3.0],
        ];
        let flat_corner = vec![[-10.0, -10.0], [0.0, -10.0], [10.0, -10.0], [10.0, 10.0], [-10.0, 10.0]];
        for (name, c) in [("dart", dart), ("arrow", arrow), ("plus", plus), ("flat corner", flat_corner)] {
            let n = c.len();
            let f = fill_loop(n, &mut lifted(&c, &z), 1e-4, 0.0).unwrap();
            let inside = off_inside(&f, &c, &z);
            eprintln!("{name}: deviation {:e}, inside off the cylinder by {inside:e}, {} evaluations", f.deviation, f.evaluations);
            // The least-bending fill, not the cylinder: within a few tenths of a millimetre of it.
            assert!(f.deviation <= 1e-4 && inside < 0.2, "{name}: {inside}");
            // The same loop from another corner and the other way round.
            let mut back = c.clone();
            back.rotate_left(2);
            back.reverse();
            let g = fill_loop(n, &mut lifted(&back, &z), 1e-4, 0.0).unwrap();
            assert_ne!(f.counter_clockwise, g.counter_clockwise);
            for (x, y) in [(4.0, 0.0), (6.0, 1.0), (5.0, -1.0)] {
                let p = Vec3::new(x, y, z(x, y));
                let gap = f.point_over(p).unwrap().dist(g.point_over(p).unwrap());
                assert!(gap < 1e-7, "{name}: {gap}");
            }
        }
    }

    #[test]
    fn the_fill_is_the_same_at_any_size_and_place() {
        // The arrow on the cylinder, scaled by 100 and by 1/100, turned and moved: the same fill,
        // scaled and moved alike.
        let z = |x: f64, _: f64| 50.0 - (1600.0 - x * x).sqrt();
        let arrow = [[0.0, 0.0], [10.0, -8.0], [10.0, -3.0], [20.0, 0.0], [10.0, 3.0]];
        let f = fill_loop(5, &mut lifted(&arrow, &z), 1e-4, 0.0).unwrap();
        let axis = Vec3::new(1.0, -2.0, 0.5).normalized().unwrap();
        let shift = Vec3::new(30.0, -12.0, 7.0);
        for scale in [100.0, 0.01] {
            let place = |p: Vec3| {
                // Scaled, turned by 0.9 about `axis` (Rodrigues), moved.
                let p = p * scale;
                let (c, s) = (0.9f64.cos(), 0.9f64.sin());
                p * c + axis.cross(p) * s + axis * (axis.dot(p) * (1.0 - c)) + shift * scale
            };
            let mut moved = |k: usize, t: f64| lifted(&arrow, &z)(k, t).map(place);
            let g = fill_loop(5, &mut moved, 1e-4 * scale, 0.0).unwrap();
            let mut worst = 0.0f64;
            for (x, y) in [(4.0, 0.0), (6.0, 1.0), (5.0, -1.0), (9.0, -2.0), (12.0, 0.5)] {
                let p = Vec3::new(x, y, z(x, y));
                worst = worst.max(g.point_over(place(p)).unwrap().dist(place(f.point_over(p).unwrap())) / scale);
            }
            eprintln!("arrow scaled by {scale}, turned and moved: {} evaluations, surfaces apart by {worst:e} (relative)", g.evaluations);
            assert!(worst < 1e-7, "scale {scale}: {worst}");
        }
    }

    #[test]
    fn a_long_narrow_slot_fills() {
        // A 200 × 1 mm slot on a cylinder of radius 300 running along it.
        let z = |x: f64, _: f64| 300.0 - (90_000.0 - (x - 100.0) * (x - 100.0)).sqrt();
        let slot = [[0.0, 0.0], [200.0, 0.0], [200.0, 1.0], [0.0, 1.0]];
        let f = fill_loop(4, &mut lifted(&slot, &z), 2e-4, 0.0).unwrap();
        let inside = off_inside(&f, &slot, &z);
        eprintln!("slot: deviation {:e}, inside off the cylinder by {inside:e}, {} evaluations", f.deviation, f.evaluations);
        assert!(f.deviation <= 2e-4 && inside < 1e-2);
    }

    #[test]
    fn loops_that_do_not_bound_a_height_field_are_refused() {
        let unsupported = |r: SplineResult<LoopFill>| matches!(r, Err(SplineError::Unsupported(_)));
        let flat = |_: f64, _: f64| 0.0;
        // A figure eight (lobes of different sizes).
        let eight = [[0.0, 0.0], [10.0, 10.0], [10.0, 0.0], [0.0, 6.0]];
        assert!(unsupported(fill_loop(4, &mut lifted(&eight, &flat), 1e-4, 0.0)));
        // A loop with a vertical spike in it (up along the normal and down again).
        let spike = [
            Vec3::ZERO,
            Vec3::new(10.0, 0.0, 0.0),
            Vec3::new(10.0, 10.0, 0.0),
            Vec3::new(5.0, 10.0, 0.0),
            Vec3::new(5.0, 10.0, 3.0),
            Vec3::new(5.0, 10.0, 0.0),
            Vec3::new(0.0, 10.0, 0.0),
        ];
        assert!(unsupported(fill_loop(7, &mut polygon3(&spike), 1e-4, 0.0)));
        // A loop that runs back over itself.
        let back = [[0.0, 0.0], [10.0, 0.0], [4.0, 0.0], [10.0, 10.0], [0.0, 10.0]];
        assert!(unsupported(fill_loop(5, &mut lifted(&back, &flat), 1e-4, 0.0)));
        // A loop too wavy for one surface.
        let wavy = |x: f64, y: f64| (x * 9.0).sin() * (y * 7.0).cos();
        assert!(unsupported(fill_loop(64, &mut lifted(&ring(64, 10.0), &wavy), 1e-6, 0.0)));
    }

    #[test]
    fn hostile_loops_are_refused() {
        let flat = |_: f64, _: f64| 0.0;
        let five = ring(5, 10.0);
        assert!(fill_loop(0, &mut lifted(&five, &flat), 1e-4, 0.0).is_err());
        assert!(fill_loop(5, &mut |_, _| Some(Vec3::X), 1e-4, 0.0).is_err(), "no area");
        assert!(fill_loop(5, &mut |_, _| None, 1e-4, 0.0).is_err());
        assert!(fill_loop(5, &mut |k, t| Some(Vec3::new(k as f64, t, f64::NAN)), 1e-4, 0.0).is_err());
        assert!(fill_loop(5, &mut lifted(&five, &flat), 0.0, 0.0).is_err() && fill_loop(5, &mut lifted(&five, &flat), f64::NAN, 0.0).is_err());
        assert!(fill_loop(10_000, &mut lifted(&five, &flat), 1e-4, 0.0).is_err());
        let huge = |x: f64, y: f64| (x * y) * 1e300;
        let _ = fill_loop(5, &mut lifted(&five, &huge), 1e-4, 0.0);
        let tiny: Vec<[f64; 2]> = five.iter().map(|p| [p[0] * 1e-300, p[1] * 1e-300]).collect();
        let _ = fill_loop(5, &mut lifted(&tiny, &flat), 1e-4, 0.0);
        let big: Vec<[f64; 2]> = five.iter().map(|p| [p[0] * 1e300, p[1] * 1e300]).collect();
        let _ = fill_loop(5, &mut lifted(&big, &flat), 1e-4, 0.0);
    }

    /// A patch whose rim is a dome: the crown fills its middle by `crown` times the loop's size
    /// and leaves the loop — the body's own edges — exactly where it was.
    #[test]
    fn a_crown_fills_the_middle_of_a_patch_and_leaves_its_loop_alone() {
        let hex = ring(6, 10.0);
        let dome = |x: f64, y: f64| 3.0 - 0.02 * (x * x + y * y);
        let plain = fill_loop(6, &mut lifted(&hex, &dome), 2e-5, 0.0).unwrap();
        // Crown 0 is the fit as it was: no refinement, no bump, nothing moved.
        assert_eq!((plain.crown, plain.crowned, plain.crown_drift), (0.0, 0, 0.0));
        assert_eq!(plain.surface.count(), [19, 17], "crown 0 keeps the fitted net (16 × 14 spans)");
        let crown = 0.25;
        let full = fill_loop(6, &mut lifted(&hex, &dome), 2e-5, crown).unwrap();
        assert!(full.crowned > 0, "the crown needs room inside the loop");
        assert_eq!(full.crown_drift, 0.0, "the bump must not move the loop at all");
        assert_eq!(full.deviation, plain.deviation, "the crown must not touch the fit");
        // The middle of the box rises by `crown` times the loop's size: the bubble is 1 there.
        let [du, dv] = plain.surface.domain();
        let (uc, vc) = ((du[0] + du[1]) / 2.0, (dv[0] + dv[1]) / 2.0);
        let size = (du[1] - du[0]).hypot(dv[1] - dv[0]);
        let mid = |f: &LoopFill| f.surface.eval(uc, vc).unwrap_or(Vec3::ZERO);
        let bump = (mid(&full) - mid(&plain)).dot(plain.normal);
        eprintln!("crown {crown}: {} points took the bump, the middle rose by {bump} of {}", full.crowned, crown * size);
        assert!((bump - crown * size).abs() < 1e-9 * size, "the middle rose by {bump}, wanted {}", crown * size);
        // A flatter crown lowers the middle by the same.
        let flat = fill_loop(6, &mut lifted(&hex, &dome), 2e-5, -crown).unwrap();
        let dip = (mid(&flat) - mid(&plain)).dot(plain.normal);
        assert!((dip + crown * size).abs() < 1e-9 * size, "the middle fell by {dip}");
        // The loop's own points are still on the surface, as closely as they were.
        let off = |f: &LoopFill| {
            hex.iter()
                .map(|p| f.point_over(Vec3::new(p[0], p[1], dome(p[0], p[1]))).map_or(f64::INFINITY, |q| (q.z - dome(p[0], p[1])).abs()))
                .fold(0.0f64, f64::max)
        };
        assert!(off(&full) <= off(&plain) + 1e-9, "the crowned fill left the loop: {:e} against {:e}", off(&full), off(&plain));
    }

    #[test]
    fn hostile_crowns_are_refused() {
        let hex = ring(6, 10.0);
        let dome = |x: f64, y: f64| 3.0 - 0.02 * (x * x + y * y);
        for crown in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY, 3.01, -0.91, 1e300] {
            assert!(fill_loop(6, &mut lifted(&hex, &dome), 2e-5, crown).is_err(), "crown {crown}");
        }
        assert!(fill_loop(6, &mut lifted(&hex, &dome), 2e-5, MAX_CROWN).is_ok());
        assert!(fill_loop(6, &mut lifted(&hex, &dome), 2e-5, MIN_CROWN).is_ok());
    }

    #[test]
    fn inside_loop_classifies_points_against_a_square_and_an_l_shape() {
        // A 4x4 square: the ray cast toggles at an edge only when exactly one of its
        // endpoints has y strictly greater than the point's y, so the bottom and left
        // edges (where the point's own y matches the lower endpoint) are treated as part
        // of the inside, while the top and right edges (where it matches the higher
        // endpoint) are not. The same rule resolves the corners: only the bottom-left one,
        // which lies on two "inside" edges, counts as inside.
        let square = [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        assert!(inside_loop([2.0, 2.0], &square), "the middle is inside");
        assert!(!inside_loop([6.0, 6.0], &square), "outside the box is outside");
        assert!(inside_loop([2.0, 0.0], &square), "the bottom edge is inside");
        assert!(!inside_loop([2.0, 4.0], &square), "the top edge is outside");
        assert!(inside_loop([0.0, 2.0], &square), "the left edge is inside");
        assert!(!inside_loop([4.0, 2.0], &square), "the right edge is outside");
        assert!(inside_loop([0.0, 0.0], &square), "the bottom-left corner is inside");
        assert!(!inside_loop([4.0, 0.0], &square), "the bottom-right corner is outside");
        assert!(!inside_loop([4.0, 4.0], &square), "the top-right corner is outside");
        assert!(!inside_loop([0.0, 4.0], &square), "the top-left corner is outside");

        // An L shape (the square above with its top-right quadrant missing): the notch
        // is outside even though it sits inside the square's bounding box, and every leg
        // of the L is inside.
        let l_shape = [[0.0, 0.0], [4.0, 0.0], [4.0, 2.0], [2.0, 2.0], [2.0, 4.0], [0.0, 4.0]];
        assert!(!inside_loop([3.0, 3.0], &l_shape), "the notch is outside");
        assert!(inside_loop([1.0, 1.0], &l_shape), "the bottom-left leg is inside");
        assert!(inside_loop([3.0, 1.0], &l_shape), "the bottom-right leg is inside");
        assert!(inside_loop([1.0, 3.0], &l_shape), "the top-left leg is inside");
    }

    #[test]
    fn segments_meet_detects_crossing_touching_and_overlapping_segments() {
        // Crossing: the square's two diagonals.
        assert!(segments_meet([0.0, 0.0], [4.0, 4.0], [0.0, 4.0], [4.0, 0.0]), "the diagonals cross");
        // Apart: neither collinear nor crossing.
        assert!(!segments_meet([0.0, 0.0], [1.0, 0.0], [5.0, 1.0], [6.0, 2.0]), "the segments are apart");
        // Sharing only an endpoint still counts as meeting: the segments are closed.
        assert!(segments_meet([0.0, 0.0], [2.0, 2.0], [2.0, 2.0], [4.0, 0.0]), "sharing an endpoint meets");
        // Collinear and overlapping (sharing the stretch from x = 2 to x = 4).
        assert!(segments_meet([0.0, 0.0], [4.0, 0.0], [2.0, 0.0], [6.0, 0.0]), "collinear and overlapping meets");
        // Collinear but apart: a gap from x = 2 to x = 3 between them.
        assert!(!segments_meet([0.0, 0.0], [2.0, 0.0], [3.0, 0.0], [5.0, 0.0]), "collinear but apart does not meet");
    }
}
