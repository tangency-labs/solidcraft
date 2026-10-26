//! Filling a closed loop with one height field that meets the faces beside the loop with G1 or
//! G2: the surface of [`crate::fill::fill_loop`] (a bicubic B-spline height field over the loop's
//! plane, so its parameters are plane coordinates and every edge's pcurve is the edge projected
//! onto that plane, exactly), fitted to the faces' derivatives as well as to the loop.
//!
//! The formulation (Piegl & Tiller, *The NURBS Book*, §9.4.1 least squares; do Carmo,
//! *Differential Geometry of Curves and Surfaces*, §3-3 for the second fundamental form of a
//! graph):
//!
//! - **The plane and grid** are [`crate::fill::fill_loop`]'s: the surface is
//!   `S(u, v) = o + u·x + v·y + h(u, v)·n` with `h` a bicubic B-spline on a uniform grid over the
//!   projected loop's box, the heights its unknowns.
//! - **The conditions** at each station (a point of the loop with the face beside it) are linear
//!   in the heights, since every quantity of a graph is a derivative of `h`:
//!   - G0: `h(u, v)` is the station's height.
//!   - G1: the face's unit normal `N` (turned to `n`'s side) is the graph's when
//!     `h_u = −N·x / N·n` and `h_v = −N·y / N·n`.
//!   - G2: with G1, the surfaces share the curve and its tangent plane, so they agree to the
//!     second order exactly when their normal curvatures across the curve agree: along the
//!     direction `w` the face leaves the loop in (`a = w·x`, `b = w·y`),
//!     `(a² h_uu + 2ab h_uv + b² h_vv) · N·n = κ`, `κ` the face's normal curvature along `w`
//!     (its curvature vector `bend` against `N`).
//!
//!   Each row is weighed to a length on the grid's scale (the G1 rows times a cell, the G2 rows
//!   times a cell squared), and the heights minimise their squared misfit plus a small multiple
//!   of the thin-plate energy ([`FAIRNESS`]), which decides what the conditions leave free.
//! - **At a corner** of the loop the two faces' conditions meet, and where their curvatures
//!   across the corner differ (a round ending on a plane: the round's section has the round's
//!   curvature where it reaches the plane, which is flat across its own curve) no surface is G2
//!   to both there: the surface's curvature is continuous, and has to pass from one face's to the
//!   other's. A plane's G2 with G1 makes the surface flat to the second order all along its
//!   curve, so the passage is a stiff one, and asked over a few cells it pulls the surface off
//!   the loop along the whole side. The corner's own station holds the position only; the G2
//!   rows fade in over [`FADE_SHARE`] of the run from each corner to the next (a smoothstep of
//!   the distance along the loop), a stretch of the loop's own, which finer grids then resolve,
//!   and hold everywhere else. G1 rows fade likewise from a corner whose faces' tangent planes
//!   disagree (`clash`), where no surface is G1 to both.
//! - **Grids** of 4 to [`MAX_MATCHED_SPANS`] spans across the box's longer side are tried in turn,
//!   with denser sampling when a grid's cells are finer than twice the stations' step: the first
//!   whose surface is within `tol` of every station, whose normal is within `angle_tol` of the
//!   face's at every G1/G2 station clear of clashing corners, and whose normal curvature across
//!   the loop is within `curvature_tol` of the face's at every G2 station clear of the corners'
//!   fades, is the fill. Otherwise "not supported yet", with how close the best grid came.
//!
//! A loop that is not the boundary of a height field over its plane, or a face beside it that
//! stands along the plane's normal, is refused, never filled with wrong geometry. Input is
//! hostile: sizes are capped and every evaluation checked; nothing here panics.

use crate::Vec3;
use crate::bspline::{SplineError, SplineResult, invalid};
use crate::fill::{Band, Grid, LoopFill, LoopFrame, MAX_LOOP_POINTS, SMOOTHNESS, check_projectable, loop_frame};
use crate::fill_general::Station;

/// Most spans of the height field across the box's longer side.
pub const MAX_MATCHED_SPANS: usize = 128;
/// Fewest spans (the first grid tried).
const MIN_SPANS: usize = 4;
/// The share of the run from a corner to the next over which the conditions the corner cannot
/// hold fade in.
pub const FADE_SHARE: f64 = 0.15;
/// The thin-plate energy's weight per fitted station: a thousand times
/// [`crate::fill::fill_loop`]'s. Its fill only passes through points, which leave the inside to
/// the energy alone; here the slope and curvature rows also act a little way inside the loop,
/// and with the weaker energy they pull the surface up past a plane it is G2 to, a millimetre or
/// two in from the plane's curve (by 0.06 at a box corner's r5 rounds set back 8). This weight
/// keeps it under the plane but where it passes from a round's curvature to the plane's, and
/// costs the loop little (2e-5 mm, 0.025° and 4e-4 per mm there).
const FAIRNESS: f64 = 1e3 * SMOOTHNESS;
/// A face beside the loop whose normal is closer than this cosine to square with the plane's
/// stands along the plane's normal: the height field cannot be tangent to it.
const MIN_NORMAL_COS: f64 = 0.05;

/// What [`fill_matched`] is asked for.
#[derive(Clone, Copy, Debug)]
pub struct MatchedSpec {
    /// How far the surface may be from the loop.
    pub tol: f64,
    /// How far (radians) its normal may turn from a G1/G2 neighbour's.
    pub angle_tol: f64,
    /// How far (1/length) its normal curvature across the loop may be from a G2 neighbour's,
    /// clear of the corners' fades.
    pub curvature_tol: f64,
}

/// The fill and what it achieved.
#[derive(Clone, Debug, PartialEq)]
pub struct MatchedFill {
    /// The height field, its plane and its deviation from the loop (no crown).
    pub fill: LoopFill,
    /// The largest angle (radians) between the surface's normal and a neighbour's at the G1/G2
    /// stations held to the angle tolerance, and at any G1/G2 station.
    pub normal_deviation: f64,
    pub normal_deviation_all: f64,
    /// The largest difference (1/length) between the surface's normal curvature across the
    /// loop and the neighbour's, at the G2 stations clear of the corners' fades, and at any G2
    /// station.
    pub curvature_mismatch: f64,
    pub curvature_mismatch_all: f64,
    /// Spans of the grid each way, and stations sampled.
    pub spans: [usize; 2],
    pub stations: usize,
}

/// Fill the closed loop of `sides` sides with one height field meeting each station's face as
/// its `across` asks (see the module documentation). `stations(k, ts)` samples side `k` at the
/// parameters `ts` (0 at its start, which is where side `k − 1` ends, towards 1 at its end); a
/// station at `t = 0` marked `corner` is a corner of the loop.
pub fn fill_matched(
    sides: usize,
    stations: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Station>, String>,
    spec: &MatchedSpec,
) -> SplineResult<MatchedFill> {
    if sides == 0 || sides.saturating_mul(16) > MAX_LOOP_POINTS / 2 {
        return Err(SplineError::TooLarge(format!("a loop of {sides} sides (at most {})", MAX_LOOP_POINTS / 32)));
    }
    let positive = |x: f64| x.is_finite() && x > 0.0;
    if !(positive(spec.tol) && positive(spec.angle_tol) && positive(spec.curvature_tol)) {
        return invalid("the fill tolerances must be positive");
    }
    let mut sample = |ts: &[f64]| -> SplineResult<Vec<Vec<Station>>> {
        (0..sides)
            .map(|k| {
                let got = stations(k, ts).map_err(SplineError::Invalid)?;
                if got.len() == ts.len() && got.iter().all(finite_station) {
                    Ok(got)
                } else {
                    Err(SplineError::Invalid(format!("side {k} has no finite stations")))
                }
            })
            .collect()
    };
    let mut per = 16usize;
    let mut fit = sample(&(0..per).map(|i| i as f64 / per as f64).collect::<Vec<_>>())?;
    let mut best = Shortfall::default();
    let mut level = MIN_SPANS;
    loop {
        let check = sample(&(0..per).map(|i| (i as f64 + 0.5) / per as f64).collect::<Vec<_>>())?;
        // The loop in order: fitted and checked stations alternating, side by side.
        let mut all: Vec<(Station, bool)> = Vec::with_capacity(2 * per * sides);
        for (f, c) in fit.iter().zip(&check) {
            for (a, b) in f.iter().zip(c) {
                all.push((*a, true));
                all.push((*b, false));
            }
        }
        match fit_round(&all, spec, level)? {
            Round::Done(fill) => return Ok(*fill),
            Round::Denser(s, at) => {
                best = best.min(s);
                level = at;
            }
            Round::Exhausted(s) => {
                best = best.min(s);
                break;
            }
        }
        if all.len() * 2 > MAX_LOOP_POINTS {
            break;
        }
        fit = fit.iter().zip(&check).map(|(f, c)| f.iter().zip(c).flat_map(|(a, b)| [*a, *b]).collect()).collect();
        per *= 2;
    }
    Err(SplineError::Unsupported(format!(
        "filling this loop with one surface meeting its neighbours: the closest fit stays {:e} off the loop (tolerance {:e}), turns {:.4}° from \
         its neighbours (tolerance {:.4}°) and misses their curvature by {:e} (tolerance {:e})",
        best.deviation,
        spec.tol,
        best.angle.to_degrees(),
        spec.angle_tol.to_degrees(),
        best.curvature,
        spec.curvature_tol
    )))
}

fn finite_station(s: &Station) -> bool {
    s.p.is_finite() && s.across.is_none_or(|a| a.leave.is_finite() && a.normal.is_finite() && a.bend.is_none_or(|b| b.is_finite()))
}

/// How close a grid came.
#[derive(Clone, Copy, Debug)]
struct Shortfall {
    deviation: f64,
    angle: f64,
    curvature: f64,
}

impl Default for Shortfall {
    fn default() -> Shortfall {
        Shortfall { deviation: f64::INFINITY, angle: f64::INFINITY, curvature: f64::INFINITY }
    }
}

impl Shortfall {
    /// The closer of two, by how many times its tolerance each misses.
    fn min(self, other: Shortfall) -> Shortfall {
        let worst = |s: &Shortfall| s.deviation + s.angle + s.curvature;
        if worst(&other) < worst(&self) { other } else { self }
    }
}

enum Round {
    Done(Box<MatchedFill>),
    /// Denser sampling is needed for the grid of this level (the coarser ones fell short).
    Denser(Shortfall, usize),
    Exhausted(Shortfall),
}

/// A station as the fit takes it: its place over the plane, its height, and what the face beside
/// it asks there, in the plane's terms.
struct Row {
    q: [f64; 2],
    h: f64,
    fitted: bool,
    /// G1: the height field's gradient there, and the face's unit normal on `n`'s side.
    gradient: Option<([f64; 2], Vec3)>,
    /// G2: the leave direction's plane components `(a, b)` and the second derivative of `h`
    /// along them, `κ / N·n`, with `κ` itself.
    curvature: Option<([f64; 2], f64, f64)>,
    /// How near the station is to a corner, and to a clashing corner: its distance along the
    /// loop to the nearer of the two around it, as a share of the run between them (0 at a
    /// corner, ½ midway; 1 with no corner).
    to_corner: f64,
    to_clash: f64,
    corner: bool,
}

/// The stations in the plane `f`'s terms, with their distances to the corners along the loop.
fn rows(all: &[(Station, bool)], f: &LoopFrame) -> SplineResult<Vec<Row>> {
    let m = all.len();
    // Arc length along the loop (chords), and where its corners are.
    let mut s = vec![0.0f64; m + 1];
    for k in 0..m {
        let (Some((a, _)), Some((b, _))) = (all.get(k), all.get((k + 1) % m)) else { return invalid("stations") };
        let next = s.get(k).copied().unwrap_or(0.0) + a.p.dist(b.p);
        if let Some(slot) = s.get_mut(k + 1) {
            *slot = next;
        }
    }
    let total = s.get(m).copied().unwrap_or(0.0);
    if !(total > 0.0 && total.is_finite()) {
        return invalid("the loop has no length");
    }
    let at = |k: usize| s.get(k).copied().unwrap_or(0.0);
    let corners: Vec<f64> = (0..m).filter(|&k| all.get(k).is_some_and(|(st, _)| st.corner)).map(at).collect();
    let clashes: Vec<f64> = (0..m).filter(|&k| all.get(k).is_some_and(|(st, _)| st.clash)).map(at).collect();
    let share = |list: &[f64], x: f64| {
        if list.is_empty() {
            return 1.0;
        }
        let back = list.iter().map(|c| (x - c).rem_euclid(total)).fold(f64::INFINITY, f64::min);
        let ahead = list.iter().map(|c| (c - x).rem_euclid(total)).filter(|d| *d > 0.0).fold(total, f64::min);
        let run = back + ahead;
        if run > 0.0 { back.min(ahead) / run } else { 0.0 }
    };
    let mut out = Vec::with_capacity(m);
    for (k, (st, fitted)) in all.iter().enumerate() {
        let d = st.p - f.o;
        let q = [d.dot(f.x), d.dot(f.y)];
        let mut gradient = None;
        let mut curvature = None;
        if let Some(a) = st.across {
            let normal = a.normal.normalized().ok_or_else(|| SplineError::Invalid("a neighbour's normal has no direction".into()))?;
            let side = if normal.dot(f.n) >= 0.0 { 1.0 } else { -1.0 };
            let nn = normal * side;
            let c = nn.dot(f.n);
            if !(c > MIN_NORMAL_COS) {
                return Err(SplineError::Unsupported("filling a loop beside a face that stands along the fill's normal".into()));
            }
            gradient = Some(([-nn.dot(f.x) / c, -nn.dot(f.y) / c], nn));
            if let Some(bend) = a.bend {
                let kappa = bend.dot(nn);
                let w = a.leave;
                curvature = Some(([w.dot(f.x), w.dot(f.y)], kappa / c, kappa));
            }
        }
        out.push(Row {
            q,
            h: d.dot(f.n),
            fitted: *fitted,
            gradient,
            curvature,
            to_corner: share(&corners, at(k)),
            to_clash: share(&clashes, at(k)),
            corner: st.corner,
        });
    }
    Ok(out)
}

/// The smoothstep `3t² − 2t³` of `share / FADE_SHARE` (1 from [`FADE_SHARE`] on).
fn fade(share: f64) -> f64 {
    let t = (share / FADE_SHARE).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// The height field's derivatives at `q`: `[h, h_u, h_v, h_uu, h_uv, h_vv]`.
fn derivatives(grid: &Grid, heights: &[f64], q: [f64; 2]) -> Option<[f64; 6]> {
    let cu = *grid.count.first()?;
    let mut out = [0.0; 6];
    for (slot, (ru, rv)) in out.iter_mut().zip([(0, 0), (1, 0), (0, 1), (2, 0), (1, 1), (0, 2)]) {
        let (i0, bu) = grid.basis_derivative(0, q[0], ru)?;
        let (j0, bv) = grid.basis_derivative(1, q[1], rv)?;
        let mut s = 0.0;
        for (b, wv) in bv.iter().enumerate() {
            for (a, wu) in bu.iter().enumerate() {
                s += wu * wv * heights.get((j0 + b) * cu + i0 + a)?;
            }
        }
        *slot = s;
    }
    out.iter().all(|x| x.is_finite()).then_some(out)
}

/// One grid's fit of the rows: the normal equations of the weighed rows and the energy.
fn solve(grid: &Grid, rows: &[Row], cell: f64) -> SplineResult<Vec<f64>> {
    let [cu, cv] = grid.count;
    let n = cu * cv;
    let mut band = Band::new(n, 3 * cu + 3);
    let mut rhs = vec![0.0; n];
    let add = |terms: &[(usize, f64)], value: f64, weight: f64, band: &mut Band, rhs: &mut Vec<f64>| {
        let w2 = weight * weight;
        for &(r, cr) in terms {
            if let Some(slot) = rhs.get_mut(r) {
                *slot += w2 * cr * value;
            }
            for &(s, cs) in terms {
                band.add(r, s, w2 * cr * cs);
            }
        }
    };
    let mut fitted = 0usize;
    for row in rows.iter().filter(|r| r.fitted) {
        let basis = |r: [usize; 2]| -> SplineResult<(usize, [f64; 4], usize, [f64; 4])> {
            let (Some((i0, bu)), Some((j0, bv))) = (grid.basis_derivative(0, row.q[0], r[0]), grid.basis_derivative(1, row.q[1], r[1])) else {
                return invalid("a loop point is off the grid");
            };
            Ok((i0, bu, j0, bv))
        };
        // The 16 control heights a derivative of order `r` at the station draws on, scaled.
        let terms = |r: [usize; 2], scale: f64| -> SplineResult<Vec<(usize, f64)>> {
            let (i0, bu, j0, bv) = basis(r)?;
            let mut t = Vec::with_capacity(16);
            for (b, wv) in bv.iter().enumerate() {
                for (a, wu) in bu.iter().enumerate() {
                    t.push(((j0 + b) * cu + i0 + a, wu * wv * scale));
                }
            }
            Ok(t)
        };
        add(&terms([0, 0], 1.0)?, row.h, 1.0, &mut band, &mut rhs);
        fitted += 1;
        if row.corner {
            continue;
        }
        if let Some((g, _)) = row.gradient {
            let w = cell * fade(row.to_clash);
            if w > 0.0 {
                add(&terms([1, 0], 1.0)?, g[0], w, &mut band, &mut rhs);
                add(&terms([0, 1], 1.0)?, g[1], w, &mut band, &mut rhs);
            }
        }
        if let Some(([a, b], second, _)) = row.curvature {
            let w = cell * cell * fade(row.to_corner) * fade(row.to_clash);
            if w > 0.0 {
                // a² h_uu + 2ab h_uv + b² h_vv, the three derivatives' terms summed per control
                // height.
                let mut sum: Vec<(usize, f64)> = Vec::with_capacity(48);
                for (r, s) in [([2, 0], a * a), ([1, 1], 2.0 * a * b), ([0, 2], b * b)] {
                    for (idx, v) in terms(r, s)? {
                        match sum.iter_mut().find(|(i, _)| *i == idx) {
                            Some(slot) => slot.1 += v,
                            None => sum.push((idx, v)),
                        }
                    }
                }
                add(&sum, second, w, &mut band, &mut rhs);
            }
        }
    }
    let lambda = FAIRNESS * fitted as f64 / n as f64 * grid.cell(0) * grid.cell(1);
    grid.add_energy(&mut band, lambda);
    band.solve(rhs)
}

/// What a fitted grid achieves at the rows: the deviation from the loop, the normal's angle to
/// the neighbours' (held stations, all), the curvature mismatch (held stations, all).
fn achieved(grid: &Grid, heights: &[f64], rows: &[Row], f: &LoopFrame) -> SplineResult<[f64; 5]> {
    let reach = FADE_SHARE;
    let (mut dev, mut ang, mut ang_all, mut cur, mut cur_all) = (0.0f64, 0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for row in rows {
        let Some([h, hu, hv, huu, huv, hvv]) = derivatives(grid, heights, row.q) else { return invalid("the fit is not finite") };
        dev = dev.max((h - row.h).abs());
        if row.corner {
            continue;
        }
        let up = (1.0 + hu * hu + hv * hv).sqrt();
        let normal = (f.n - f.x * hu - f.y * hv) * (1.0 / up);
        if let Some((_, nn)) = row.gradient {
            let a = normal.dot(nn).clamp(-1.0, 1.0).acos();
            ang_all = ang_all.max(a);
            if row.to_clash >= reach {
                ang = ang.max(a);
            }
        }
        if let Some(([a, b], _, kappa)) = row.curvature {
            // The normal curvature along the unit tangent of the surface over (a, b): the second
            // fundamental form over the first.
            let t = f.x * a + f.y * b + f.n * (a * hu + b * hv);
            let ii = (a * a * huu + 2.0 * a * b * huv + b * b * hvv) / up;
            let k = ii / t.len2().max(1e-300);
            let miss = (k - kappa).abs();
            cur_all = cur_all.max(miss);
            if row.to_corner >= reach && row.to_clash >= reach {
                cur = cur.max(miss);
            }
        }
    }
    Ok([dev, ang, ang_all, cur, cur_all])
}

/// One round: the grids from `level` up, on these stations.
fn fit_round(all: &[(Station, bool)], spec: &MatchedSpec, level: usize) -> SplineResult<Round> {
    let fitted: Vec<Vec3> = all.iter().filter(|(_, f)| *f).map(|(s, _)| s.p).collect();
    let frame = loop_frame(&fitted)?;
    let points: Vec<Vec3> = all.iter().map(|(s, _)| s.p).collect();
    let rows = rows(all, &frame)?;
    let uv: Vec<[f64; 2]> = rows.iter().map(|r| r.q).collect();
    check_projectable(&points, &uv)?;
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
    // The fitted stations' largest step each way: a grid finer than half of it waits for denser
    // sampling.
    let fq: Vec<[f64; 2]> = rows.iter().filter(|r| r.fitted).map(|r| r.q).collect();
    let mut step = [0.0f64; 2];
    for k in 0..fq.len() {
        let (p, q) = (fq.get(k).copied().unwrap_or([0.0; 2]), fq.get((k + 1) % fq.len()).copied().unwrap_or([0.0; 2]));
        for d in 0..2 {
            step[d] = step[d].max((q[d] - p[d]).abs());
        }
    }
    let extent = [hi[0] - lo[0], hi[1] - lo[1]];
    let longer = extent[0].max(extent[1]);
    let mut best = Shortfall::default();
    let mut level = level;
    while level <= MAX_MATCHED_SPANS {
        let spans = [0, 1].map(|d| ((extent[d] / longer * level as f64).ceil() as usize).clamp(1, level));
        let holds = (0..2).all(|d| extent[d] / spans[d] as f64 >= 2.0 * step[d]);
        if !holds && level > MIN_SPANS {
            return Ok(Round::Denser(best, level));
        }
        let grid = Grid::new(spans, lo, hi);
        let cell = grid.cell(0).max(grid.cell(1));
        let heights = solve(&grid, &rows, cell)?;
        let [dev, ang, ang_all, cur, cur_all] = achieved(&grid, &heights, &rows, &frame)?;
        let s = Shortfall { deviation: dev, angle: ang, curvature: cur };
        best = best.min(s);
        if dev <= spec.tol && ang <= spec.angle_tol && cur <= spec.curvature_tol && holds {
            let surface = grid.surface(&heights, frame.o, frame.x, frame.y, frame.n)?;
            let fill = LoopFill {
                surface,
                origin: frame.o,
                axes: [frame.x, frame.y],
                normal: frame.n,
                counter_clockwise: frame.counter_clockwise,
                deviation: dev,
                evaluations: all.len(),
                crown: 0.0,
                crowned: 0,
                crown_drift: 0.0,
            };
            return Ok(Round::Done(Box::new(MatchedFill {
                fill,
                normal_deviation: ang,
                normal_deviation_all: ang_all,
                curvature_mismatch: cur,
                curvature_mismatch_all: cur_all,
                spans,
                stations: all.len(),
            })));
        }
        if !holds {
            return Ok(Round::Denser(best, level));
        }
        level *= 2;
    }
    Ok(Round::Exhausted(best))
}

#[cfg(test)]
#[path = "fill_matched_tests.rs"]
mod tests;
