//! Filling a closed loop with one surface that meets the faces beside the loop with a chosen
//! continuity (G0, G1 or G2 per station) and passes through given points, for any loop that
//! turns round once: loops that are not a height field over any plane, and loops of any number of
//! sides, are filled whole.
//!
//! The formulation:
//!
//! - **The parameter domain is developed from the loop itself**, not projected. The loop is
//!   walked as a polygon of its samples; each step keeps its length, and each turn between steps
//!   is split, against the patch's expected normal there (`up`), into its part about that normal
//!   (the turn *in* the patch: the boundary's geodesic turn, signed, so a reflex corner stays
//!   reflex) and the rest (the turn the boundary makes *with* the patch bending under it, which a
//!   plane cannot show). By Gauss–Bonnet the in-patch turns of a developable patch add up to a
//!   full turn; a curved patch makes up the difference with its curvature, so a shortfall is given
//!   back in proportion to the bending turns (a cap over a round rim, whose rim does not turn in
//!   its neighbours' tangent planes at all, develops to the circle), and a saddle's excess comes
//!   off the smooth stretches, so corners keep their angles. The headings then close exactly, and
//!   the small gap left in position is spread along the steps by length. A domain that crosses
//!   itself is refused. The domain's steps are as long as the loop's, so the parameters are
//!   millimetres along the boundary. (`up` is the neighbour's normal at a G1/G2 station; at a G0
//!   station, the loop's tangent × the neighbour's leave direction tilted towards the loop's mean
//!   plane.)
//! - **The surface is a bicubic B-spline over the domain's box** whose control points minimise,
//!   by least squares, the misfit to the boundary conditions plus a vanishing multiple of the
//!   thin-plate energy `∫∫ |S_uu|² + 2|S_uv|² + |S_vv|²` (which only decides what the conditions
//!   leave free):
//!   - G0, every station: the position.
//!   - G1: `n·∂S/∂ν = 0` (`ν` the domain's inward normal, `n` the neighbour's normal): the
//!     cross-boundary derivative lies in the neighbour's tangent plane, which with the boundary
//!     tangent makes the normals agree; plus weak Hermite rows `∂S/∂ν = s·w` (`w` the direction
//!     the neighbour leaves in across the edge, `s` the boundary's own speed), which say that the
//!     patch leaves into itself rather than folding back over the neighbour, and about how fast,
//!     but give way where full Hermite data cannot all hold (it is consistent only with a map
//!     exactly conformal at the boundary, which a curved patch's is not).
//!   - G2: `n·∂²S/∂ν² = (∂S/∂ν·w)²·κ` (`κ` the neighbour's normal curvature along `w`, read off
//!     the bridge's frames): the patch's normal curvature across the edge is the neighbour's. The
//!     speed is the fit's own, so the row is solved again with the last solution's speed.
//!
//!   Without G1/G2 the three coordinates are fitted apart on one shared matrix; with them, together
//!   (the geometric rows couple them), numbered so the band stays narrow. At corners the two
//!   sides' conditions overdetermine the surface: see [`Near`] and [`fades`]. The face is bounded
//!   by the body's own edges, so the surface only has to pass within the tolerance of them.
//! - **Points the patch must pass through** are located on the fitted surface (nearest point,
//!   inside the domain), added as position data there, and the fit and the location are repeated
//!   — Hoschek's parameter correction — so each point ends on the surface.
//! - **Grids** of 4 to [`MAX_GENERAL_SPANS`] spans across the domain's longer side are tried in
//!   turn; the first whose surface is within the tolerance of every station, whose normals are
//!   within the angle tolerance of the neighbours' at every G1 or G2 station (away from corners
//!   where the neighbours' tangent planes disagree: no surface can meet both there), that passes
//!   through the points, and that neither folds nor pinches inside the loop, is the fill. When a
//!   grid needs more stations than it has, the sampling doubles.
//! - **A crown** fills the middle out along the surface's own normals, on control points clear of
//!   the loop, so it moves neither the loop nor the conditions across it.

use crate::Vec3;
use crate::bspline::{HPoint, NurbsSurface, SplineError, SplineResult, invalid};
use crate::fill::{Band, Grid, MAX_LOOP_POINTS, SMOOTHNESS, inside_loop, segments_meet};

/// Most spans of the surface along the domain's longer side.
pub const MAX_GENERAL_SPANS: usize = 64;
/// Most points a patch can be asked to pass through.
pub const MAX_FILL_POINTS: usize = 64;
/// Fewest spans along the domain's longer side (the first grid tried).
const MIN_SPANS: usize = 4;
/// A turn between two loop steps sharper than this (radians) folds the loop back on itself.
const FOLD_TURN: f64 = 3.05;
/// How many points of the domain's box each way the fold check samples.
const FOLD_SAMPLES: usize = 32;
/// Rounds of locating the points on the surface and fitting again.
const POINT_ROUNDS: usize = 3;
/// A domain is rectilinear when this fraction of its length (weighted by how squarely it runs)
/// lies along two directions at right angles.
const RECTILINEAR: f64 = 0.5;
/// How far (radians) the domain's angle at a corner may be from the loop's for the corner to be
/// met conformally (see [`Near`]).
const CORNER_ANGLE_TOL: f64 = 0.5 * std::f64::consts::PI / 180.0;
/// Near a corner whose neighbours disagree, the G1 rows are left out over this share of the run to
/// the next corner (see [`fades`]).
const BLEND_SHARE: f64 = 0.4;
/// How close, in grid cells, a station is to a corner when it is near it (see [`Near`]): G1 and
/// G2 stations that near a corner whose neighbours disagree are not held to the angle tolerance.
const CLASH_CELLS: f64 = 2.0;

/// What the face beside a station asks the patch to repeat across it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Across {
    /// The unit direction the neighbour leaves in across the edge (its tangent plane, at right
    /// angles to the edge, away from the neighbour): where the patch goes.
    pub leave: Vec3,
    /// The neighbour's unit normal (either sign).
    pub normal: Vec3,
    /// G2: the neighbour's curvature vector along `leave` (unit speed).
    pub bend: Option<Vec3>,
}

/// One sample of the loop.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Station {
    pub p: Vec3,
    /// The patch's expected unit normal here, so that the loop runs counter-clockwise about it
    /// with the patch on its left: the loop's tangent × the direction into the patch. It need not
    /// be exact; it decides which way the loop turns in the patch.
    pub up: Vec3,
    /// G1 or G2 conditions (`None`: position only, G0).
    pub across: Option<Across>,
    /// The loop has a corner here: position only (two neighbours' conditions meet).
    pub corner: bool,
    /// A corner where the neighbours' tangent planes disagree: no surface is G1 to both, so the
    /// angle tolerance is not asked of the stations close to it.
    pub clash: bool,
}

/// What [`fill_general`] is asked for.
#[derive(Clone, Copy, Debug)]
pub struct FillSpec<'a> {
    /// How far the surface may be from the loop (and from the points).
    pub tol: f64,
    /// How far (radians) the surface's normal may turn from a G1/G2 neighbour's.
    pub angle_tol: f64,
    /// How full the middle is made (see [`crate::fill::fill_loop`]'s crown, between
    /// [`crate::fill::MIN_CROWN`] and [`crate::fill::MAX_CROWN`]): the bump, `crown` times the
    /// domain's diagonal at its middle, is laid along the surface's own normal, turned to the side
    /// the G1/G2 neighbours' normals face, on control points clear of the loop, so neither the
    /// loop nor the conditions across it move. A loop with no G1/G2 station has no side to crown
    /// to and is refused one.
    pub crown: f64,
    /// Points the surface passes through.
    pub points: &'a [Vec3],
}

/// The fill and what it achieved.
#[derive(Clone, Debug, PartialEq)]
pub struct GeneralFill {
    pub surface: NurbsSurface,
    /// Whether the loop, in the order given, runs counter-clockwise about the surface's normal
    /// `∂S/∂u × ∂S/∂v`.
    pub counter_clockwise: bool,
    /// The farthest station from the surface.
    pub deviation: f64,
    /// The largest angle (radians) between the surface's normal and a neighbour's at a G1/G2
    /// station held to the angle tolerance, and at any G1/G2 station.
    pub normal_deviation: f64,
    pub normal_deviation_all: f64,
    /// The largest difference (1/length) between the surface's normal curvature across the
    /// boundary and the neighbour's, at the G2 stations held to the angle tolerance.
    pub curvature_mismatch: f64,
    /// The farthest point from the surface.
    pub point_deviation: f64,
    /// Spans of the grid each way, and stations sampled.
    pub spans: [usize; 2],
    pub stations: usize,
}

/// Fill the closed loop of `sides` sides with one surface (see the module documentation).
/// `stations(k, ts)` samples side `k` at the parameters `ts` (0 at its start, which is where side
/// `k−1` ends, towards 1 at its end); it is asked for the same parameters again only when the
/// sampling grows.
pub fn fill_general(
    sides: usize,
    stations: &mut dyn FnMut(usize, &[f64]) -> Result<Vec<Station>, String>,
    spec: &FillSpec,
) -> SplineResult<GeneralFill> {
    if sides == 0 || sides.saturating_mul(16) > MAX_LOOP_POINTS / 2 {
        return Err(SplineError::TooLarge(format!("a loop of {sides} sides (at most {})", MAX_LOOP_POINTS / 32)));
    }
    if !(spec.tol.is_finite() && spec.tol > 0.0 && spec.angle_tol.is_finite() && spec.angle_tol > 0.0) {
        return invalid("the fill tolerances must be positive");
    }
    // A range test also throws out NaN.
    if !(crate::fill::MIN_CROWN..=crate::fill::MAX_CROWN).contains(&spec.crown) {
        return invalid("the crown must be between -0.9 and 3");
    }
    if spec.points.len() > MAX_FILL_POINTS || spec.points.iter().any(|p| !p.is_finite()) {
        return invalid(format!("a patch passes through at most {MAX_FILL_POINTS} finite points"));
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
    // Each round starts at the grid the last one stopped at: the coarser grids fell short with
    // fewer stations, and more stations do not make a coarse grid hold the loop closer.
    let mut level = MIN_SPANS;
    loop {
        let check = sample(&(0..per).map(|i| (i as f64 + 0.5) / per as f64).collect::<Vec<_>>())?;
        // Each side in order: fitted and checked stations alternating.
        let all: Vec<Vec<Station>> = fit.iter().zip(&check).map(|(f, c)| f.iter().zip(c).flat_map(|(a, b)| [*a, *b]).collect()).collect();
        let flat: Vec<Station> = all.iter().flatten().copied().collect();
        match fit_round(&flat, spec, level)? {
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
        if flat.len() * 2 > MAX_LOOP_POINTS {
            break;
        }
        fit = all;
        per *= 2;
    }
    let mut why = format!("filling this loop with one surface: the closest fit stays {:e} off the loop (tolerance {:e})", best.deviation, spec.tol);
    if fit.iter().flatten().any(|s| s.across.is_some()) {
        why += &format!(", turns {:.3}° from its neighbours (tolerance {:.3}°)", best.angle.to_degrees(), spec.angle_tol.to_degrees());
    }
    if !spec.points.is_empty() {
        why += &format!(", {:e} off its points", best.points);
    }
    Err(SplineError::Unsupported(why))
}

fn finite_station(s: &Station) -> bool {
    s.p.is_finite()
        && s.up.is_finite()
        && s.across.is_none_or(|a| a.leave.is_finite() && a.normal.is_finite() && a.bend.is_none_or(|b| b.is_finite()))
}

/// How close a grid came: what [`fill_general`] reports when none is close enough.
#[derive(Clone, Copy, Debug)]
struct Shortfall {
    deviation: f64,
    angle: f64,
    points: f64,
}

impl Default for Shortfall {
    fn default() -> Shortfall {
        Shortfall { deviation: f64::INFINITY, angle: f64::INFINITY, points: f64::INFINITY }
    }
}

impl Shortfall {
    /// The closer of two, by how many times its tolerance each misses.
    fn min(self, other: Shortfall) -> Shortfall {
        let worst = |s: &Shortfall| s.deviation.max(s.points) + s.angle;
        if worst(&other) < worst(&self) { other } else { self }
    }
}

enum Round {
    Done(Box<GeneralFill>),
    /// Denser sampling is needed for the grid of this level (the coarser ones fell short).
    Denser(Shortfall, usize),
    Exhausted(Shortfall),
}

/// The loop developed into the plane (see the module documentation).
struct Developed {
    /// The stations' places in the domain.
    d: Vec<[f64; 2]>,
    /// The domain's inward unit normal at each station.
    nu: Vec<[f64; 2]>,
    /// The boundary's speed at each station: loop length per domain length.
    speed: Vec<f64>,
    /// Whether the domain turns at each station by the loop's own angle there (to
    /// [`CORNER_ANGLE_TOL`]): a corner where it does not cannot be met conformally.
    true_angle: Vec<bool>,
    counter_clockwise: bool,
}

/// Develop the loop `st` into the plane.
fn develop(st: &[Station]) -> SplineResult<Developed> {
    let m = st.len();
    if m < 3 {
        return invalid("a loop needs three stations");
    }
    let at = |k: usize| st.get(k % m).map_or(Vec3::ZERO, |s| s.p);
    let chord: Vec<Vec3> = (0..m).map(|k| at(k + 1) - at(k)).collect();
    let len: Vec<f64> = chord.iter().map(|c| c.len()).collect();
    let total: f64 = len.iter().sum();
    if !(total > 0.0 && total.is_finite()) {
        return invalid("the loop has no length");
    }
    if len.iter().any(|l| !(*l > 1e-12 * total)) {
        return Err(SplineError::Unsupported("filling a loop with two stations in one place".into()));
    }
    // The turn at each station, split into its part in the patch (signed about `up`) and the rest.
    // A G0 station's `up` is the loop's tangent × its neighbour's leave direction: the patch's
    // normal if it carried on from the neighbour, which says nothing about the turns of a loop
    // whose neighbours stand square to the patch (a lid on walls). It is tilted towards the
    // loop's own mean plane (Newell's area vector, its part square to the tangent, at half
    // weight): that decides those turns, and where a loop wraps round so far that the mean plane
    // faces the other way, the neighbour's direction still wins.
    let area = (0..m).fold(Vec3::ZERO, |a, k| a + at(k).cross(at(k + 1)));
    let loop_normal = area.normalized().unwrap_or(Vec3::ZERO);
    let up: Vec<Vec3> = (0..m)
        .map(|k| {
            let Some(s) = st.get(k) else { return Vec3::ZERO };
            if s.across.is_some() {
                return s.up;
            }
            let (Some(a), Some(b)) = (chord.get((k + m - 1) % m), chord.get(k)) else { return s.up };
            let Some(t) = (*a + *b).normalized() else { return s.up };
            let square = loop_normal - t * loop_normal.dot(t);
            (s.up + square * 0.5).normalized().unwrap_or(s.up)
        })
        .collect();
    let mut full_turn = vec![0.0; m];
    let mut in_patch = vec![0.0; m];
    let mut bending = vec![0.0; m];
    for k in 0..m {
        let (Some(a), Some(b), Some(&up)) = (chord.get((k + m - 1) % m), chord.get(k), up.get(k)) else { continue };
        let (la, lb) = (a.len(), b.len());
        let cos = (a.dot(*b) / (la * lb)).clamp(-1.0, 1.0);
        let turn = cos.acos();
        if turn > FOLD_TURN {
            return Err(SplineError::Unsupported("filling a loop that folds back on itself".into()));
        }
        if let Some(f) = full_turn.get_mut(k) {
            *f = turn;
        }
        let axis = a.cross(*b);
        let Some(axis) = axis.normalized().filter(|_| axis.len() > 1e-15 * la * lb) else { continue };
        let along = axis.dot(up).clamp(-1.0, 1.0);
        if let (Some(g), Some(r)) = (in_patch.get_mut(k), bending.get_mut(k)) {
            *g = turn * along;
            *r = turn * (1.0 - along * along).max(0.0).sqrt();
        }
    }
    let geodesic: f64 = in_patch.iter().sum();
    let bend_total: f64 = bending.iter().sum();
    let sense = if geodesic >= 0.0 { 1.0 } else { -1.0 };
    let full = std::f64::consts::TAU;
    // A saddle's boundary turns more than once round in the patch: the excess comes off the
    // smooth stretches, so the corners keep their angles (a G1 patch is conformal at its
    // boundary, and a map conformal at a corner keeps its angle); only when the corners alone
    // turn too far are they scaled too.
    let is_corner = |k: usize| st.get(k).is_some_and(|s| s.corner || s.clash);
    let at_corners: f64 = (0..m).filter(|k| is_corner(*k)).filter_map(|k| in_patch.get(k)).sum::<f64>() * sense;
    let smooth = geodesic * sense - at_corners;
    let turns: Vec<f64> = if geodesic.abs() >= full && at_corners <= full && smooth > 0.0 {
        let f = (full - at_corners) / smooth;
        in_patch.iter().enumerate().map(|(k, g)| if is_corner(k) { *g } else { g * f }).collect()
    } else if geodesic.abs() >= full {
        in_patch.iter().map(|g| g * full / geodesic.abs()).collect()
    } else if bend_total > 1e-9 {
        let mu = (full - geodesic.abs()) / bend_total;
        in_patch.iter().zip(&bending).map(|(g, r)| g + sense * mu * r).collect()
    } else {
        return Err(SplineError::Unsupported("filling a loop that does not turn round".into()));
    };
    // Steps at their lengths along the headings, the gap spread by length.
    let mut heading = 0.0;
    let mut steps: Vec<[f64; 2]> = Vec::with_capacity(m);
    for k in 0..m {
        heading += turns.get(k).copied().unwrap_or(0.0);
        let l = len.get(k).copied().unwrap_or(0.0);
        steps.push([l * heading.cos(), l * heading.sin()]);
    }
    let gap = steps.iter().fold([0.0, 0.0], |g, s| [g[0] + s[0], g[1] + s[1]]);
    for (s, l) in steps.iter_mut().zip(&len) {
        s[0] -= gap[0] * l / total;
        s[1] -= gap[1] * l / total;
    }
    let mut d = Vec::with_capacity(m);
    let mut q = [0.0, 0.0];
    for s in &steps {
        d.push(q);
        q = [q[0] + s[0], q[1] + s[1]];
    }
    // Centred and turned so the domain, and with it the grid and the fill, does not depend on
    // where the loop starts: a mostly rectilinear domain (most of its length runs along two
    // directions at right angles, as a polygon of straight sides often does) with those
    // directions along the grid's lines, which hold data along them exactly; any other onto its
    // principal axes. The larger extent is along u.
    let centre = d.iter().fold([0.0, 0.0], |c, p| [c[0] + p[0] / m as f64, c[1] + p[1] / m as f64]);
    let (mut sxx, mut sxy, mut syy) = (0.0, 0.0, 0.0);
    for p in &d {
        let (x, y) = (p[0] - centre[0], p[1] - centre[1]);
        sxx += x * x;
        sxy += x * y;
        syy += y * y;
    }
    let (mut c4, mut s4) = (0.0, 0.0);
    for st in &steps {
        let (l, a) = (st[0].hypot(st[1]), st[1].atan2(st[0]));
        c4 += l * (4.0 * a).cos();
        s4 += l * (4.0 * a).sin();
    }
    let angle = if c4.hypot(s4) > RECTILINEAR * total { 0.25 * s4.atan2(c4) } else { 0.5 * (2.0 * sxy).atan2(sxx - syy) };
    let turned = |angle: f64| -> Vec<[f64; 2]> {
        let (c, s) = (angle.cos(), angle.sin());
        d.iter()
            .map(|p| {
                let (x, y) = (p[0] - centre[0], p[1] - centre[1]);
                [c * x + s * y, -s * x + c * y]
            })
            .collect()
    };
    let mut d = turned(angle);
    let spread =
        |d: &[[f64; 2]], k: usize| d.iter().map(|p| p[k]).fold(f64::NEG_INFINITY, f64::max) - d.iter().map(|p| p[k]).fold(f64::INFINITY, f64::min);
    if spread(&d, 1) > spread(&d, 0) * (1.0 + 1e-9) {
        d = turned(angle + std::f64::consts::FRAC_PI_2);
    }
    let skew: f64 = d.iter().map(|p| p[0] * p[0] * p[0] + p[1] * p[1] * p[1]).sum();
    if skew < 0.0 {
        for p in &mut d {
            *p = [-p[0], -p[1]];
        }
    }
    check_simple(&d)?;
    // The domain runs the way the loop turned in the patch.
    let area: f64 = (0..m).map(|k| cross2(get2(&d, k), get2(&d, k + 1))).sum::<f64>() * 0.5;
    if !(area * sense > 0.0) {
        return Err(SplineError::Unsupported("filling a loop that develops inside out".into()));
    }
    let mut nu = Vec::with_capacity(m);
    let mut speed = Vec::with_capacity(m);
    for k in 0..m {
        let (prev, next) = (get2(&d, k + m - 1), get2(&d, k + 1));
        let here = get2(&d, k);
        let (a, b) = ([here[0] - prev[0], here[1] - prev[1]], [next[0] - here[0], next[1] - here[1]]);
        let (la, lb) = (a[0].hypot(a[1]), b[0].hypot(b[1]));
        if !(la > 0.0 && lb > 0.0) {
            return Err(SplineError::Unsupported("filling a loop with two stations in one place".into()));
        }
        let t = [a[0] / la + b[0] / lb, a[1] / la + b[1] / lb];
        let lt = t[0].hypot(t[1]);
        let t = if lt > 1e-9 { [t[0] / lt, t[1] / lt] } else { [b[0] / lb, b[1] / lb] };
        // The left normal is inward on a counter-clockwise domain.
        nu.push([-t[1] * sense, t[0] * sense]);
        let space = len.get((k + m - 1) % m).copied().unwrap_or(0.0) + len.get(k).copied().unwrap_or(0.0);
        speed.push(space / (la + lb));
    }
    let true_angle = turns.iter().zip(&full_turn).map(|(t, f)| (t.abs() - f).abs() <= CORNER_ANGLE_TOL).collect();
    Ok(Developed { d, nu, speed, true_angle, counter_clockwise: sense > 0.0 })
}

fn get2(d: &[[f64; 2]], k: usize) -> [f64; 2] {
    d.get(k % d.len().max(1)).copied().unwrap_or([0.0; 2])
}

fn cross2(a: [f64; 2], b: [f64; 2]) -> f64 {
    a[0] * b[1] - a[1] * b[0]
}

/// Refuse a domain polygon whose non-adjacent sides meet.
fn check_simple(d: &[[f64; 2]]) -> SplineResult<()> {
    let m = d.len();
    let q = |k: usize| get2(d, k);
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
                return Err(SplineError::Unsupported("filling a loop whose development crosses itself".into()));
            }
        }
    }
    Ok(())
}

/// The basis of a grid at `q`: the first indices and the values of the four non-zero functions
/// and their first two derivatives, each way.
struct Basis {
    first: [usize; 2],
    /// `[direction][derivative][function]`.
    b: [[[f64; 4]; 3]; 2],
}

impl Basis {
    fn at(grid: &Grid, q: [f64; 2]) -> Option<Basis> {
        let mut first = [0usize; 2];
        let mut b = [[[0.0; 4]; 3]; 2];
        for d in 0..2 {
            for r in 0..3 {
                let (f, vals) = grid.basis_derivative(d, *q.get(d)?, r)?;
                *first.get_mut(d)? = f;
                *b.get_mut(d)?.get_mut(r)? = vals;
            }
        }
        Some(Basis { first, b })
    }

    /// The 16 (i, j, coefficient) terms of the derivative `Σ w[ru][rv] ∂^ru_u ∂^rv_v` at the point.
    fn cells(&self, w: &[(usize, usize, f64)]) -> [(usize, usize, f64); 16] {
        let mut out = [(0usize, 0usize, 0.0f64); 16];
        for (t, slot) in out.iter_mut().enumerate() {
            let (a, b) = (t % 4, t / 4);
            let c: f64 = w.iter().map(|&(ru, rv, k)| k * self.b[0][ru.min(2)][a] * self.b[1][rv.min(2)][b]).sum();
            *slot = (self.first[0] + a, self.first[1] + b, c);
        }
        out
    }

    /// The 16 (index, coefficient) terms of the derivative `Σ w[ru][rv] ∂^ru_u ∂^rv_v` at the
    /// point, over control points numbered i fastest in a grid `cu` wide.
    fn terms(&self, cu: usize, w: &[(usize, usize, f64)]) -> [(usize, f64); 16] {
        let mut out = [(0usize, 0.0f64); 16];
        for (t, slot) in out.iter_mut().enumerate() {
            let (a, b) = (t % 4, t / 4);
            let c: f64 = w.iter().map(|&(ru, rv, k)| k * self.b[0][ru.min(2)][a] * self.b[1][rv.min(2)][b]).sum();
            *slot = ((self.first[1] + b) * cu + self.first[0] + a, c);
        }
        out
    }
}

/// The normal equations of the fit, shared by the three coordinates.
struct System {
    band: Band,
    rhs: [Vec<f64>; 3],
    rows: usize,
}

impl System {
    fn new(n: usize, bw: usize) -> System {
        System { band: Band::new(n, bw), rhs: [vec![0.0; n], vec![0.0; n], vec![0.0; n]], rows: 0 }
    }

    /// One least-squares row: `Σ coefficient · control = target`.
    fn row(&mut self, terms: &[(usize, f64); 16], target: Vec3) {
        for &(r, wr) in terms {
            for (rhs, t) in self.rhs.iter_mut().zip([target.x, target.y, target.z]) {
                if let Some(slot) = rhs.get_mut(r) {
                    *slot += wr * t;
                }
            }
            for &(s, ws) in terms {
                self.band.add(r, s, wr * ws);
            }
        }
        self.rows += 1;
    }
}

/// A fitted surface's net on its grid.
struct Net<'a> {
    grid: &'a Grid,
    ctrl: Vec<Vec3>,
}

impl Net<'_> {
    /// `∂^ru_u ∂^rv_v S` at `q`.
    fn der(&self, q: [f64; 2], ru: usize, rv: usize) -> Option<Vec3> {
        let b = Basis::at(self.grid, q)?;
        let cu = *self.grid.count.first()?;
        let mut s = Vec3::ZERO;
        for (i, c) in b.terms(cu, &[(ru, rv, 1.0)]) {
            s += *self.ctrl.get(i)? * c;
        }
        Some(s)
    }

    fn point(&self, q: [f64; 2]) -> Option<Vec3> {
        self.der(q, 0, 0)
    }

    /// The unit normal `S_u × S_v` and the first derivatives.
    fn frame(&self, q: [f64; 2]) -> Option<(Vec3, Vec3, Vec3)> {
        let (su, sv) = (self.der(q, 1, 0)?, self.der(q, 0, 1)?);
        Some((su.cross(sv).normalized()?, su, sv))
    }
}

/// The weights `(ru, rv, w)` of the first and second derivatives along the domain direction `nu`.
fn along(nu: [f64; 2], order: usize) -> Vec<(usize, usize, f64)> {
    let [a, b] = nu;
    match order {
        1 => vec![(1, 0, a), (0, 1, b)],
        _ => vec![(2, 0, a * a), (1, 1, 2.0 * a * b), (0, 2, b * b)],
    }
}

/// One round: develop the loop `st` (the fitted stations at even places), then try the grids.
fn fit_round(st: &[Station], spec: &FillSpec, start: usize) -> SplineResult<Round> {
    let dev = develop(st)?;
    let m = st.len();
    let (mut lo, mut hi) = ([f64::INFINITY; 2], [f64::NEG_INFINITY; 2]);
    for q in &dev.d {
        for k in 0..2 {
            lo[k] = lo[k].min(q[k]);
            hi[k] = hi[k].max(q[k]);
        }
    }
    let pad = 0.05 * (hi[0] - lo[0]).max(hi[1] - lo[1]);
    for k in 0..2 {
        lo[k] -= pad;
        hi[k] += pad;
    }
    let extent = [hi[0] - lo[0], hi[1] - lo[1]];
    if !(extent[0] > 0.0 && extent[1] > 0.0 && extent[0].is_finite() && extent[1].is_finite()) {
        return invalid("the loop's development has no extent");
    }
    // The largest step between fitted stations, per direction.
    let mut step = [0.0f64; 2];
    for k in (0..m).step_by(2) {
        let (p, q) = (get2(&dev.d, k), get2(&dev.d, k + 2));
        for i in 0..2 {
            step[i] = step[i].max((q[i] - p[i]).abs());
        }
    }
    // Where the corners are in the domain, and those whose neighbours disagree.
    let corners: Vec<[f64; 2]> = st.iter().zip(&dev.d).filter(|(s, _)| s.corner || s.clash).map(|(_, d)| *d).collect();
    let rough: Vec<[f64; 2]> = st
        .iter()
        .zip(&dev.d)
        .zip(&dev.true_angle)
        .filter(|((s, _), true_angle)| s.clash || (s.corner && !**true_angle))
        .map(|((_, d), _)| *d)
        .collect();
    let clashes: Vec<[f64; 2]> = st.iter().zip(&dev.d).filter(|(s, _)| s.clash).map(|(_, d)| *d).collect();
    let fade = fades(st);
    let longer = extent[0].max(extent[1]);
    let mut best = Shortfall::default();
    let mut level = start.max(MIN_SPANS);
    while level <= MAX_GENERAL_SPANS {
        let spans = [0, 1].map(|d| ((extent[d] / longer * level as f64).ceil() as usize).clamp(1, level));
        let holds = (0..2).all(|d| extent[d] / spans[d] as f64 >= 2.0 * step[d]);
        if !holds && level > start {
            return Ok(Round::Denser(best, level));
        }
        let grid = Grid::new(spans, lo, hi);
        let cell = (grid.cell(0) * grid.cell(1)).sqrt();
        let near = Near::of(&dev.d, &corners, &rough, &clashes, CLASH_CELLS * cell);
        let (net, located) = fit_grid(&grid, st, &dev, spec, &near, &fade)?;
        let got = measure(&net, st, &dev, spec, &located, &near, &fade)?;
        best = best.min(got.shortfall);
        let within = got.shortfall.deviation <= spec.tol && got.shortfall.angle <= spec.angle_tol && got.shortfall.points <= spec.tol;
        if within && holds {
            check_folds(&net, &dev.d, lo, hi)?;
            let ctrl: Vec<HPoint> = net.ctrl.iter().map(|p| HPoint::weighted(*p, 1.0)).collect();
            let surface = NurbsSurface::from_homogeneous([3, 3], grid.knots.clone(), ctrl)?;
            let surface = crown(surface, &net, st, &dev, spec.crown)?;
            return Ok(Round::Done(Box::new(GeneralFill {
                surface,
                counter_clockwise: dev.counter_clockwise,
                deviation: got.shortfall.deviation,
                normal_deviation: got.shortfall.angle,
                normal_deviation_all: got.angle_all,
                curvature_mismatch: got.curvature,
                point_deviation: got.shortfall.points,
                spans,
                stations: m,
            })));
        }
        if !holds {
            return Ok(Round::Denser(best, level));
        }
        level *= 2;
    }
    Ok(Round::Exhausted(best))
}

/// Which stations are close to a corner, and to a corner whose neighbours disagree, on one grid.
///
/// At a corner the two sides' conditions meet and overdetermine the surface (the compatibility
/// of Hermite boundary data at a vertex): there one side's cross-boundary derivatives are the
/// other side's boundary curve, and the development's corner angle only approximates the loop's.
/// Close to any corner the second-derivative rows are left out, and close to a corner the domain
/// does not turn by the loop's own angle (a conformal map keeps angles), or where the neighbours'
/// tangent planes disagree, the first-derivative rows too: the positions decide there. The
/// neighbourhood is a few grid cells, so it shrinks as the grid refines. (Where the neighbours'
/// tangent planes disagree no surface is tangent to both; see [`fades`].)
struct Near {
    corner: Vec<bool>,
    rough: Vec<bool>,
    clash: Vec<bool>,
}

impl Near {
    fn of(d: &[[f64; 2]], corners: &[[f64; 2]], rough: &[[f64; 2]], clashes: &[[f64; 2]], reach: f64) -> Near {
        let close = |set: &[[f64; 2]]| -> Vec<bool> { d.iter().map(|q| set.iter().any(|c| (c[0] - q[0]).hypot(c[1] - q[1]) <= reach)).collect() };
        Near { corner: close(corners), rough: close(rough), clash: close(clashes) }
    }

    fn rough(&self, k: usize) -> bool {
        self.rough.get(k).copied().unwrap_or(true)
    }

    fn corner(&self, k: usize) -> bool {
        self.corner.get(k).copied().unwrap_or(true)
    }

    fn clash(&self, k: usize) -> bool {
        self.clash.get(k).copied().unwrap_or(true)
    }
}

/// Whether each station's G1 rows are asked (1) or not (0): not near a corner whose neighbours'
/// tangent planes disagree.
struct Fade {
    weight: Vec<f64>,
}

impl Fade {
    fn at(&self, k: usize) -> f64 {
        self.weight.get(k).copied().unwrap_or(0.0)
    }
}

/// The G1 rows' weights. At a corner whose neighbours' tangent planes disagree no surface is
/// tangent to both: its tangent plane there must hold both boundary tangents, so it is singular
/// at the corner however it is shaped. Within [`BLEND_SHARE`] of the run to the next corner of
/// such a corner the G1 rows are left out and the positions and the smoothness decide the
/// surface; those stations are not held to the angle tolerance. (Rows fading out towards the
/// corner instead keep asking for a tangent plane the corner cannot have, and the fit folds
/// there.)
fn fades(st: &[Station]) -> Fade {
    let m = st.len();
    let mut weight = vec![1.0f64; m];
    let clashes: Vec<usize> = (0..m).filter(|k| st.get(*k).is_some_and(|s| s.clash)).collect();
    if clashes.is_empty() || m < 3 {
        return Fade { weight };
    }
    let p = |k: usize| st.get(k % m).map_or(Vec3::ZERO, |s| s.p);
    let step = |k: usize| p(k + 1).dist(p(k));
    let is_corner = |k: usize| st.get(k % m).is_some_and(|s| s.corner || s.clash);
    for &c in &clashes {
        if let Some(w) = weight.get_mut(c) {
            *w = 0.0;
        }
        // Back along the incoming side, then on along the outgoing one.
        for back in [true, false] {
            let next = |k: usize| if back { (k + m - 1) % m } else { (k + 1) % m };
            // The run to the next corner.
            let (mut run, mut k) = (0.0, c);
            for _ in 0..m {
                let n = next(k);
                run += if back { step(n) } else { step(k) };
                k = n;
                if is_corner(k) {
                    break;
                }
            }
            let reach = BLEND_SHARE * run;
            let (mut dist, mut k) = (0.0, c);
            for _ in 0..m {
                let n = next(k);
                dist += if back { step(n) } else { step(k) };
                k = n;
                if dist >= reach || is_corner(k) {
                    break;
                }
                if let Some(w) = weight.get_mut(k) {
                    *w = 0.0;
                }
            }
        }
    }
    Fade { weight }
}

/// Fit the grid to the even stations' conditions and the points: the net, and where the points
/// ended up in the domain.
fn fit_grid<'a>(
    grid: &'a Grid,
    st: &[Station],
    dev: &Developed,
    spec: &FillSpec,
    near: &Near,
    fade: &Fade,
) -> SplineResult<(Net<'a>, Vec<[f64; 2]>)> {
    let cell = (grid.cell(0) * grid.cell(1)).sqrt();
    let mut rows: Vec<Hermite> = Vec::new();
    for (k, s) in st.iter().enumerate().step_by(2) {
        let Some(across) = s.across.filter(|_| !s.corner) else { continue };
        let (Some(&q), Some(&nu), Some(&speed)) = (dev.d.get(k), dev.nu.get(k), dev.speed.get(k)) else { continue };
        let weight = fade.at(k);
        if !(weight > 1e-3) {
            continue;
        }
        let full = weight >= 1.0;
        let bend = across.bend.filter(|_| full && !near.corner(k));
        rows.push(Hermite {
            q,
            nu,
            normal: across.normal,
            leave: across.leave,
            hermite: full && !near.rough(k),
            weight,
            curvature: bend.map(|b| across.normal.dot(b)),
            speed,
        });
    }
    // The fitted stations (every other one), then the points where they were last found.
    let fitted: Vec<([f64; 2], Vec3)> = st.iter().zip(&dev.d).step_by(2).map(|(s, q)| (*q, s.p)).collect();
    let mut extra: Vec<([f64; 2], Vec3)> = Vec::new();
    let rounds = if spec.points.is_empty() { 0 } else { POINT_ROUNDS };
    let mut round = 0;
    loop {
        let data: Vec<([f64; 2], Vec3)> = fitted.iter().chain(&extra).copied().collect();
        let net = if rows.is_empty() { fit_positions(grid, &data)? } else { fit_coupled(grid, &data, &mut rows, cell)? };
        let located = spec.points.iter().map(|p| locate(&net, &dev.d, *p)).collect::<SplineResult<Vec<_>>>()?;
        if round >= rounds {
            return Ok((net, located));
        }
        extra = located.iter().copied().zip(spec.points.iter().copied()).collect();
        round += 1;
    }
}

/// The fit to positions alone (G0 everywhere): the three coordinates apart, sharing one matrix.
fn fit_positions<'a>(grid: &'a Grid, data: &[([f64; 2], Vec3)]) -> SplineResult<Net<'a>> {
    let [cu, cv] = grid.count;
    let n = cu * cv;
    let mut sys = System::new(n, 3 * cu + 3);
    for (q, p) in data {
        let b = Basis::at(grid, *q).ok_or_else(|| SplineError::Invalid("a station is off the grid".into()))?;
        sys.row(&b.terms(cu, &[(0, 0, 1.0)]), *p);
    }
    let lambda = SMOOTHNESS * sys.rows as f64 / n as f64 * grid.cell(0) * grid.cell(1);
    grid.add_energy(&mut sys.band, lambda);
    let factor = sys.band.factor_rows()?;
    let [x, y, z] = sys.rhs;
    let (x, y, z) = (factor.substitute(x)?, factor.substitute(y)?, factor.substitute(z)?);
    net_of(grid, x.iter().zip(&y).zip(&z).map(|((x, y), z)| Vec3::new(*x, *y, *z)).collect())
}

fn net_of(grid: &Grid, ctrl: Vec<Vec3>) -> SplineResult<Net<'_>> {
    if ctrl.iter().any(|p| !p.is_finite()) {
        return invalid("the fit is not finite");
    }
    Ok(Net { grid, ctrl })
}

/// One G1/G2 station's rows (see [`fit_coupled`]).
struct Hermite {
    q: [f64; 2],
    nu: [f64; 2],
    /// The normal of the tangent plane asked for, and the direction the patch should leave in.
    normal: Vec3,
    leave: Vec3,
    /// Whether the weak Hermite rows are asked (not close to a corner the development does not
    /// keep the angle of, unless in a blend).
    hermite: bool,
    /// How much the G1 row weighs (see [`fades`]).
    weight: f64,
    /// G2: the neighbour's normal curvature along `leave`.
    curvature: Option<f64>,
    /// How fast the patch leaves: first the boundary's own speed, then what the fit gives.
    speed: f64,
}

/// How much the Hermite rows (the full first derivative `s·w`) weigh against the G1 rows: enough
/// to say which way and about how fast the patch leaves, little enough to give way where the
/// full data cannot all hold.
const HERMITE_WEIGHT: f64 = 0.05;
/// Rounds of updating the speed a G2 row's curvature is asked at.
const G2_ROUNDS: usize = 4;

/// The fit with G1 and G2 rows: the three coordinates together, since the rows that hold the
/// continuity are geometric and couple them.
///
/// - Positions, as for [`fit_positions`].
/// - G1: `n·∂S/∂ν = 0` — the patch's cross-boundary derivative lies in the tangent plane asked
///   for, which with the boundary tangent in it makes the normals agree. That is all G1 asks.
/// - Weak Hermite rows `∂S/∂ν = s·w` (weight [`HERMITE_WEIGHT`]): which way the patch leaves
///   (into the patch, not folding back over the neighbour) and about how fast. A whole set of
///   full Hermite data is consistent only with a boundary map that is exactly conformal at the
///   boundary's own speed, which a curved patch's is not, so these give way.
/// - G2: `n·∂²S/∂ν² = (∂S/∂ν·w)²·κ` — the patch's normal curvature across the edge is the
///   neighbour's. The speed is the fit's own, so the row is solved again with the speed from the
///   last solution (the matrix stays, so each round is a back substitution).
///
/// The unknowns are numbered with the grid's shorter direction fastest and the coordinates
/// innermost, which keeps the band narrow.
fn fit_coupled<'a>(grid: &'a Grid, data: &[([f64; 2], Vec3)], rows: &mut [Hermite], cell: f64) -> SplineResult<Net<'a>> {
    let [cu, cv] = grid.count;
    let n = cu * cv;
    let u_fast = cu <= cv;
    let fast = if u_fast { cu } else { cv };
    let index = |i: usize, j: usize, c: usize| if u_fast { (j * cu + i) * 3 + c } else { (i * cv + j) * 3 + c };
    let mut band = Band::new(3 * n, 3 * (3 * fast + 3) + 2);
    let mut rhs = vec![0.0; 3 * n];
    let mut count = 0usize;
    // A row on the three coordinates: `Σ_c Σ_t coefficient_t · weight_c · control_{t,c} = target`.
    let mut row = |band: &mut Band, rhs: &mut [f64], cells: &[(usize, usize, f64); 16], weights: [f64; 3], target: f64| {
        let mut terms = [(0usize, 0.0f64); 48];
        for (c, w) in weights.iter().enumerate() {
            for (t, &(i, j, k)) in cells.iter().enumerate() {
                if let Some(slot) = terms.get_mut(c * 16 + t) {
                    *slot = (index(i, j, c), k * w);
                }
            }
        }
        for &(r, wr) in &terms {
            if wr == 0.0 {
                continue;
            }
            if let Some(slot) = rhs.get_mut(r) {
                *slot += wr * target;
            }
            for &(s, ws) in &terms {
                band.add(r, s, wr * ws);
            }
        }
        count += 1;
    };
    let axes = [[1.0, 0.0, 0.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]];
    let coords = |p: Vec3| [p.x, p.y, p.z];
    for &(q, p) in data {
        let b = Basis::at(grid, q).ok_or_else(|| SplineError::Invalid("a station is off the grid".into()))?;
        let cells = b.cells(&[(0, 0, 1.0)]);
        for (axis, t) in axes.iter().zip(coords(p)) {
            row(&mut band, &mut rhs, &cells, *axis, t);
        }
    }
    // The right-hand side so far; the G2 rows' targets are added per round.
    let mut second: Vec<([(usize, usize, f64); 16], usize)> = Vec::new();
    for (h, r) in rows.iter().enumerate() {
        let b = Basis::at(grid, r.q).ok_or_else(|| SplineError::Invalid("a station is off the grid".into()))?;
        let first = b.cells(&along(r.nu, 1)).map(|(i, j, k)| (i, j, k * cell));
        let held = first.map(|(i, j, k)| (i, j, k * r.weight));
        row(&mut band, &mut rhs, &held, coords(r.normal), 0.0);
        if r.hermite {
            let w = HERMITE_WEIGHT * r.weight;
            let weak = first.map(|(i, j, k)| (i, j, k * w));
            for (axis, t) in axes.iter().zip(coords(r.leave * (r.speed * cell * w))) {
                row(&mut band, &mut rhs, &weak, *axis, t);
            }
        }
        if r.curvature.is_some() {
            let cells = b.cells(&along(r.nu, 2)).map(|(i, j, k)| (i, j, k * cell * cell));
            row(&mut band, &mut rhs, &cells, coords(r.normal), 0.0);
            second.push((cells, h));
        }
    }
    let lambda = SMOOTHNESS * count as f64 / (3 * n) as f64 * grid.cell(0) * grid.cell(1);
    grid.energy(lambda, &mut |(i, j), (k, l), e| {
        for c in 0..3 {
            band.add(index(i, j, c), index(k, l, c), e);
        }
    });
    let factor = band.factor_rows()?;
    let rounds = if second.is_empty() { 1 } else { G2_ROUNDS };
    let mut net = None;
    for _ in 0..rounds {
        let mut b = rhs.clone();
        for (cells, h) in &second {
            let Some(r) = rows.get(*h) else { continue };
            let target = r.curvature.unwrap_or(0.0) * r.speed * r.speed * cell * cell;
            for (c, w) in coords(r.normal).iter().enumerate() {
                for &(i, j, k) in cells {
                    if let Some(slot) = b.get_mut(index(i, j, c)) {
                        *slot += k * w * target;
                    }
                }
            }
        }
        let x = factor.substitute(b)?;
        let ctrl: Vec<Vec3> = (0..cv)
            .flat_map(|j| (0..cu).map(move |i| (i, j)))
            .map(|(i, j)| {
                Vec3::new(
                    x.get(index(i, j, 0)).copied().unwrap_or(f64::NAN),
                    x.get(index(i, j, 1)).copied().unwrap_or(f64::NAN),
                    x.get(index(i, j, 2)).copied().unwrap_or(f64::NAN),
                )
            })
            .collect();
        let current = net_of(grid, ctrl)?;
        for r in rows.iter_mut().filter(|r| r.curvature.is_some()) {
            let (Some(su), Some(sv)) = (current.der(r.q, 1, 0), current.der(r.q, 0, 1)) else {
                return invalid("the fit cannot be evaluated on the loop");
            };
            let ahead = (su * r.nu[0] + sv * r.nu[1]).dot(r.leave);
            if ahead > 0.0 {
                r.speed = ahead;
            }
        }
        net = Some(current);
    }
    net.ok_or_else(|| SplineError::Invalid("the fit made no rounds".into()))
}

/// Where in the domain the surface comes nearest `p`: the nearest of a grid of samples inside
/// the domain, then Gauss–Newton. A point nearest the surface outside the loop is refused.
fn locate(net: &Net, d: &[[f64; 2]], p: Vec3) -> SplineResult<[f64; 2]> {
    let kn = &net.grid.knots;
    let (Some(u0), Some(u1), Some(v0), Some(v1)) = (kn[0].first(), kn[0].last(), kn[1].first(), kn[1].last()) else {
        return invalid("the grid has no knots");
    };
    let (lo, hi) = ([*u0, *v0], [*u1, *v1]);
    let samples = 48;
    let mut best: Option<(f64, [f64; 2])> = None;
    for i in 0..=samples {
        for j in 0..=samples {
            let q = [lo[0] + (hi[0] - lo[0]) * i as f64 / samples as f64, lo[1] + (hi[1] - lo[1]) * j as f64 / samples as f64];
            if !inside_loop(q, d) {
                continue;
            }
            let Some(s) = net.point(q) else { continue };
            let dist = s.dist(p);
            if best.is_none_or(|(b, _)| dist < b) {
                best = Some((dist, q));
            }
        }
    }
    let Some((_, mut q)) = best else { return Err(SplineError::Unsupported("passing through points: the loop's development is too thin".into())) };
    for _ in 0..40 {
        let (Some(s), Some(su), Some(sv)) = (net.point(q), net.der(q, 1, 0), net.der(q, 0, 1)) else { break };
        let r = s - p;
        let (a, b, c) = (su.dot(su), su.dot(sv), sv.dot(sv));
        let det = a * c - b * b;
        if !(det.abs() > 1e-300) {
            break;
        }
        let (ga, gb) = (su.dot(r), sv.dot(r));
        let next = [(q[0] - (c * ga - b * gb) / det).clamp(lo[0], hi[0]), (q[1] - (a * gb - b * ga) / det).clamp(lo[1], hi[1])];
        let moved = (next[0] - q[0]).hypot(next[1] - q[1]);
        q = next;
        if moved < 1e-13 * (hi[0] - lo[0]) {
            break;
        }
    }
    if !inside_loop(q, d) {
        return Err(SplineError::Unsupported("passing through a point that is not over the inside of the loop".into()));
    }
    Ok(q)
}

/// What a fit achieved.
struct Measured {
    shortfall: Shortfall,
    angle_all: f64,
    curvature: f64,
}

/// Measure a fit against every station and the points.
fn measure(net: &Net, st: &[Station], dev: &Developed, spec: &FillSpec, located: &[[f64; 2]], near: &Near, fade: &Fade) -> SplineResult<Measured> {
    let off = |what: &str| SplineError::Invalid(format!("the fit cannot be evaluated {what}"));
    let sense = if dev.counter_clockwise { 1.0 } else { -1.0 };
    let (mut deviation, mut angle, mut angle_all, mut curvature) = (0.0f64, 0.0f64, 0.0f64, 0.0f64);
    for (k, s) in st.iter().enumerate() {
        let (Some(&q), Some(&nu)) = (dev.d.get(k), dev.nu.get(k)) else { continue };
        deviation = deviation.max(net.point(q).ok_or_else(|| off("on the loop"))?.dist(s.p));
        let Some(across) = s.across.filter(|_| !s.corner) else { continue };
        let (normal, su, sv) = net.frame(q).ok_or_else(|| off("on the loop"))?;
        // The neighbour's normal turned to the side the patch's normal should be on.
        let want = if across.normal.dot(s.up) * sense >= 0.0 { across.normal } else { across.normal * -1.0 };
        let a = normal.dot(want).clamp(-1.0, 1.0).acos();
        angle_all = angle_all.max(a);
        if near.clash(k) || fade.at(k) < 1.0 {
            continue;
        }
        angle = angle.max(a);
        if let Some(bend) = across.bend.filter(|_| !near.corner(k)) {
            let first = su * nu[0] + sv * nu[1];
            let second = net.der(q, 2, 0).zip(net.der(q, 1, 1)).zip(net.der(q, 0, 2)).ok_or_else(|| off("on the loop"))?;
            let ((uu, uv), vv) = second;
            let second = uu * (nu[0] * nu[0]) + uv * (2.0 * nu[0] * nu[1]) + vv * (nu[1] * nu[1]);
            let speed2 = first.dot(first);
            if speed2 > 0.0 {
                curvature = curvature.max((normal.dot(second) / speed2 - want.dot(bend)).abs());
            }
        }
    }
    let mut points = 0.0f64;
    for (q, p) in located.iter().zip(spec.points) {
        points = points.max(net.point(*q).ok_or_else(|| off("at a point"))?.dist(*p));
    }
    if ![deviation, angle, angle_all, curvature, points].iter().all(|x| x.is_finite()) {
        return invalid("the fit is not finite");
    }
    Ok(Measured { shortfall: Shortfall { deviation, angle, points }, angle_all, curvature })
}

/// The fill with its crown (see [`FillSpec::crown`]).
fn crown(surface: NurbsSurface, net: &Net, st: &[Station], dev: &Developed, crown: f64) -> SplineResult<NurbsSurface> {
    if crown == 0.0 {
        return Ok(surface);
    }
    // The side the neighbours' normals face, by the patch's normals at the G1/G2 stations.
    let mut side = 0.0;
    for (s, q) in st.iter().zip(&dev.d) {
        if let (Some(a), Some((n, _, _))) = (s.across, net.frame(*q)) {
            side += n.dot(a.normal).signum();
        }
    }
    if side == 0.0 {
        return Err(SplineError::Unsupported("crowning a patch with no g1 or g2 edge to say which side is out".into()));
    }
    let sign = side.signum();
    let along = |g: [f64; 2]| net.frame(g).map(|(n, _, _)| n * sign);
    let (surface, _, _) = crate::fill::crown_fill_along(surface, &dev.d, crown, &along, MAX_GENERAL_SPANS)?;
    Ok(surface)
}

/// Refuse a fit that folds over or pinches inside the loop: on a grid of samples inside the
/// domain, the normal must be defined everywhere (the surface's parameter speed nowhere collapses
/// against the rest) and must not turn over between neighbouring samples.
fn check_folds(net: &Net, d: &[[f64; 2]], lo: [f64; 2], hi: [f64; 2]) -> SplineResult<()> {
    let n = FOLD_SAMPLES;
    let mut cells: Vec<Option<(Vec3, f64)>> = Vec::with_capacity((n + 1) * (n + 1));
    for j in 0..=n {
        for i in 0..=n {
            let q = [lo[0] + (hi[0] - lo[0]) * i as f64 / n as f64, lo[1] + (hi[1] - lo[1]) * j as f64 / n as f64];
            if !inside_loop(q, d) {
                cells.push(None);
                continue;
            }
            let (Some(su), Some(sv)) = (net.der(q, 1, 0), net.der(q, 0, 1)) else { return invalid("the fit cannot be evaluated inside") };
            let c = su.cross(sv);
            cells.push(Some((c, c.len())));
        }
    }
    let mut sizes: Vec<f64> = cells.iter().flatten().map(|(_, l)| *l).collect();
    if sizes.is_empty() {
        return Ok(());
    }
    sizes.sort_by(f64::total_cmp);
    let median = sizes.get(sizes.len() / 2).copied().unwrap_or(0.0);
    let smallest = sizes.first().copied().unwrap_or(0.0);
    if !(smallest > 1e-3 * median) {
        return Err(SplineError::Unsupported("filling this loop: the surface pinches inside it".into()));
    }
    let at = |i: usize, j: usize| cells.get(j * (n + 1) + i).copied().flatten();
    for j in 0..=n {
        for i in 0..=n {
            let Some((a, _)) = at(i, j) else { continue };
            for (x, y) in [(i + 1, j), (i, j + 1)] {
                if let Some((b, _)) = at(x, y)
                    && a.dot(b) <= 0.0
                {
                    return Err(SplineError::Unsupported("filling this loop: the surface folds over inside it".into()));
                }
            }
        }
    }
    Ok(())
}

#[cfg(test)]
#[path = "fill_general_tests.rs"]
mod tests;
