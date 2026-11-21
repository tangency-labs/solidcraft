//! The layouts of a face pattern that space their stations by distance on the surface rather than
//! by its parameters, so they stay even where the parameters crowd (a sphere's poles), each over a
//! set of faces at once:
//!
//! - [`spaced_stations`]: rows a spacing apart, measured along the surface, each row with as many
//!   stations as fit its own length. The rows run round the parameter the surface turns in (a
//!   sphere's, cylinder's or cone's longitude), so on a sphere they are its circles of latitude.
//! - [`spiral_stations`]: a count of stations on a golden-ratio (Fibonacci) lattice laid even over
//!   each face's area by its area's distribution: on a whole sphere, the Fibonacci sphere.
//! - [`fill_stations`]: that lattice relaxed, the stations pushing each other and the boundary apart
//!   until their spacing is even, moving from face to face of the set.
//! - [`geodesic_stations`]: the vertices of an icosahedron with each of its triangles divided into
//!   `frequency²`, on a set of faces of one sphere.
//!
//! Every station is inside the faces' loops and at least the margin from the set's boundary, as
//! the grid's ([`super::grid_stations_across`]).

use std::collections::BTreeMap;
use std::f64::consts::TAU;

use super::{invalid, Face, Fit, Region, Station, MAX_STATION_COUNT};
use crate::eval::query::Query;
use crate::eval::vec::VecExt as _;
use crate::eval::{Axes, Surface};
use crate::math::Vec3;
use crate::Result;

/// The largest geodesic frequency: `10 f² + 2` vertices, 812 at 9, within [`MAX_STATION_COUNT`].
pub const MAX_GEODESIC_FREQUENCY: usize = 9;

/// Most places a spaced layout tries across its faces before it refuses the spacing as too small.
const MAX_SPACED_PLACES: usize = 16 * MAX_STATION_COUNT;

/// Pieces a parameter line is cut into to measure its arc length.
const ARC_PIECES: usize = 128;

/// Newton steps that refine the parameter at an arc length within its piece.
const ARC_NEWTON: usize = 4;

/// Five-point Gauss–Legendre nodes and weights on [-1, 1].
const GAUSS: [(f64, f64); 5] = [
    (0.0, 0.568_888_888_888_888_9),
    (-0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (0.538_469_310_105_683_1, 0.478_628_670_499_366_5),
    (-0.906_179_845_938_664, 0.236_926_885_056_189_1),
    (0.906_179_845_938_664, 0.236_926_885_056_189_1),
];

/// Cells across and along a face its area is read in, for the spiral's distribution.
const AREA_ACROSS: usize = 256;
const AREA_ALONG: usize = 64;

/// The golden ratio's conjugate, `(√5 − 1) / 2`: the spiral's turn per station.
const GOLDEN: f64 = 0.618_033_988_749_894_9;

/// Rounds of an even fill's relaxation (Lloyd's method).
const FILL_ROUNDS: usize = 60;

/// Samples of the faces an even fill takes for each station: with fewer, each station's middle
/// moves in steps of the samples' spacing, and the layout stays uneven by about that much.
const FILL_SAMPLES: usize = 64;

/// Lines across a face along which [`closed`] compares its ends.
const CLOSED_LINES: usize = 5;

/// How far a face's evaluated surface may lie from the sphere it is a form of, as a share of the
/// radius (its NURBS form is exact up to rounding; a fitted one, to well within this).
const ON_SPHERE: f64 = 1e-7;

/// The relative rounding a parameter range or a point on a sphere is read within.
const ROUNDING: f64 = 1e-9;

/// Where a grid's stations stand along one of a face's parameters, in the extent's unit share of
/// it: `steps + 1 - 2 start` of them from `lo` to `hi`, the first `start` steps in from `lo` (half a
/// step at an end that is no edge, none at a fitted edge).
#[derive(Clone, Copy, Debug)]
pub(super) struct Axis {
    lo: f64,
    hi: f64,
    start: f64,
    steps: f64,
    /// The grid without a fit: cell centres, kept within `[0, 1)`.
    cells: bool,
}

impl Axis {
    /// `n` cells across the extent, a station at each centre.
    pub(super) fn cells(n: usize) -> Axis {
        Axis {
            lo: 0.0,
            hi: 1.0,
            start: 0.5,
            steps: n as f64,
            cells: true,
        }
    }

    /// Station `i`'s place (`i` past the ends for a turned grid); `None` for any but the one
    /// station of an axis with no steps.
    pub(super) fn at(&self, i: f64) -> Option<f64> {
        if self.steps > 0.0 {
            Some(self.lo + (i + self.start) / self.steps * (self.hi - self.lo))
        } else {
            (i == 0.0).then(|| self.middle())
        }
    }

    pub(super) fn middle(&self) -> f64 {
        0.5 * (self.lo + self.hi)
    }

    /// Whether a place is within the part of the extent the grid spans.
    pub(super) fn holds(&self, x: f64) -> bool {
        if self.cells {
            (0.0..1.0).contains(&x)
        } else {
            x >= self.lo - ROUNDING && x <= self.hi + ROUNDING
        }
    }
}

/// Whether face `f` closes to a point at `e` on parameter `k` (a sphere's pole): the line of its
/// other parameter there has no length.
fn pole(f: &Face, k: usize, e: f64, scale: f64) -> bool {
    Arc::new(f, 1 - k, e, range(f, 1 - k)).is_some_and(|a| a.length() <= ROUNDING * (1.0 + scale))
}

/// `n` stations along parameter `k` of face `f`, the outer ones `inset` (mm, along the line of
/// the parameter through the middle of the extent) in from the extent's ends: an end where the
/// face closes to a point keeps half a step clear instead, and a parameter the face turns round
/// in has its stations half a step from the seam. `None` where the face is too short for one
/// station `inset` in from its edges.
pub(super) fn fitted_axis(f: &Face, k: usize, n: usize, inset: f64) -> Option<Axis> {
    let even = Axis {
        cells: false,
        ..Axis::cells(n)
    };
    if closed(f, k) {
        return Some(even);
    }
    let [e0, e1] = range(f, k);
    let [o0, o1] = range(f, 1 - k);
    let line = Arc::new(f, k, 0.5 * (o0 + o1), [e0, e1])?;
    let length = line.length();
    if !(length > 0.0 && e1 > e0) {
        return None;
    }
    let (p0, p1) = (pole(f, k, e0, length), pole(f, k, e1, length));
    let edges = f64::from(u8::from(!p0) + u8::from(!p1));
    if edges * inset > length {
        return None;
    }
    let share = |t: f64| (t - e0) / (e1 - e0);
    let lo = if p0 {
        0.0
    } else {
        share(line.param_at(f, inset)?)
    };
    let hi = if p1 {
        1.0
    } else {
        share(line.param_at(f, length - inset)?)
    };
    let half = |p: bool| if p { 0.5 } else { 0.0 };
    Some(Axis {
        lo,
        hi,
        start: half(p0),
        steps: n as f64 - 1.0 + half(p0) + half(p1),
        cells: false,
    })
}

/// Arc lengths along a line of `length` for `step`-apart stations: from `a0` in from its start to
/// `a1` in from its end, the ends included, as many as come nearest `step` apart (one in the
/// middle where the two are one place). Empty where the ends cross; `None` for more than
/// [`MAX_SPACED_PLACES`].
fn fitted_places(length: f64, a0: f64, a1: f64, step: f64) -> Option<Vec<f64>> {
    let room = length - a0 - a1;
    if !(room >= 0.0) {
        return Some(Vec::new());
    }
    let gaps = (room / step).round();
    if !(gaps >= 1.0) {
        return Some(vec![a0 + 0.5 * room]);
    }
    if gaps >= MAX_SPACED_PLACES as f64 {
        return None;
    }
    Some(
        (0..=gaps as usize)
            .map(|i| a0 + room * i as f64 / gaps)
            .collect(),
    )
}

/// A layout of rows a distance apart ([`spaced_stations`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spacing {
    /// How far apart the stations of a row are, along it.
    pub spacing: f64,
    /// How far apart the rows are, across them.
    pub row_spacing: f64,
    /// Every other row moved half a spacing along.
    pub staggered: bool,
}

/// Whether face `f`'s surface turns round in parameter `k` over the face's extent: periodic in it
/// with the extent a whole period, or (a spline's closed form) meeting itself across the extent,
/// its two ends the same points along lines of the other parameter through the extent.
fn closed(f: &Face, k: usize) -> bool {
    let Some([lo, hi]) = f.extent.get(k).copied() else {
        return false;
    };
    if let Some(Some(period)) = f.surface.domain().period.get(k).copied() {
        return hi - lo >= period * (1.0 - ROUNDING);
    }
    let [b0, b1] = range(f, 1 - k);
    (0..CLOSED_LINES).all(|i| {
        let b = b0 + (b1 - b0) * (i as f64 + 0.5) / CLOSED_LINES as f64;
        let at = |a: f64| f.surface.point(uv(k, a, b));
        match (at(lo), at(0.5 * (lo + hi)), at(hi)) {
            (Some(p), Some(m), Some(q)) => p.dist(q) <= ROUNDING * (1.0 + p.dist(m)),
            _ => false,
        }
    })
}

/// The parameter face `f`'s rows run along: the one its surface turns round in where only one
/// does, else the first.
fn rows_along(f: &Face) -> usize {
    usize::from(!closed(f, 0) && closed(f, 1))
}

/// The parameters at `a` along parameter `k` and `b` along the other.
fn uv(k: usize, a: f64, b: f64) -> [f64; 2] {
    if k == 0 {
        [a, b]
    } else {
        [b, a]
    }
}

/// The extent of face `f` along parameter `k`.
fn range(f: &Face, k: usize) -> [f64; 2] {
    f.extent.get(k).copied().unwrap_or([0.0, 0.0])
}

/// How fast face `f`'s surface moves along parameter `k` at `uv`: `|S_u|` or `|S_v|`.
fn speed(f: &Face, k: usize, uv: [f64; 2]) -> Option<f64> {
    let (_, su, sv) = f.surface.derivs(uv[0], uv[1])?;
    Some(if k == 0 { su.len() } else { sv.len() })
}

/// The arc length along parameter `k` from `a` to `b`, the other parameter at `fixed`.
fn piece_length(f: &Face, k: usize, fixed: f64, a: f64, b: f64) -> Option<f64> {
    let (half, middle) = (0.5 * (b - a), 0.5 * (a + b));
    let mut sum = 0.0;
    for (x, w) in GAUSS {
        sum += w * speed(f, k, uv(k, middle + half * x, fixed))?;
    }
    Some(sum * half)
}

/// Arc length along a parameter line of a face: parameter `k` from one end of a range to the
/// other, the other parameter at `fixed`, measured piece by piece.
struct Arc {
    k: usize,
    fixed: f64,
    /// The parameter at the ends of the pieces, and the length from the start to each.
    ts: Vec<f64>,
    at: Vec<f64>,
}

impl Arc {
    fn new(f: &Face, k: usize, fixed: f64, [from, to]: [f64; 2]) -> Option<Arc> {
        let ts: Vec<f64> = (0..=ARC_PIECES)
            .map(|i| from + (to - from) * i as f64 / ARC_PIECES as f64)
            .collect();
        let mut at = vec![0.0];
        let mut sum = 0.0;
        for w in ts.windows(2) {
            let (a, b) = (*w.first()?, *w.get(1)?);
            sum += piece_length(f, k, fixed, a, b)?;
            at.push(sum);
        }
        sum.is_finite().then_some(Arc { k, fixed, ts, at })
    }

    fn length(&self) -> f64 {
        self.at.last().copied().unwrap_or(0.0)
    }

    /// The parameter at arc length `s` from the start.
    fn param_at(&self, f: &Face, s: f64) -> Option<f64> {
        let i = self
            .at
            .partition_point(|&a| a <= s)
            .saturating_sub(1)
            .min(ARC_PIECES - 1);
        let (t0, t1) = (*self.ts.get(i)?, *self.ts.get(i + 1)?);
        let (a0, a1) = (*self.at.get(i)?, *self.at.get(i + 1)?);
        let (lo, hi) = (t0.min(t1), t0.max(t1));
        let mut t = if a1 > a0 {
            t0 + (t1 - t0) * ((s - a0) / (a1 - a0))
        } else {
            t0
        };
        for _ in 0..ARC_NEWTON {
            let gap = a0 + piece_length(f, self.k, self.fixed, t0, t)? - s;
            let d = speed(f, self.k, uv(self.k, t, self.fixed))?;
            if !(d > 0.0) {
                break;
            }
            // Along a decreasing parameter the length grows as the parameter falls.
            let dir = if t1 >= t0 { 1.0 } else { -1.0 };
            t = (t - dir * gap / d).max(lo).min(hi);
        }
        Some(t)
    }
}

/// Rows a distance apart on each of `faces` of `query`'s record, each row with as many stations
/// as fit its length at `spacing.spacing` apart, every station inside the faces and at least
/// `margin` from the boundary of the set. The rows run round the parameter a face's surface turns
/// in (a sphere's, a cylinder's or a cone's longitude), else along its first parameter, and are
/// spaced by `spacing.row_spacing` along the line across them through the middle of the face's
/// extent; each row's stations divide its length evenly (a closed row's all the way round), the
/// spacing rounded to fit. A row shorter than half the spacing (round a sphere's pole) holds none.
///
/// # Errors
/// [`crate::SolidError::InvalidInput`] for a spacing that is not above zero, a margin that is not
/// a number or is negative, no face or more than [`super::MAX_FACES`], a face listed twice or one
/// the record lacks or can't read, or a spacing so small the faces would take more than 16 000
/// places.
pub fn spaced_stations(
    query: &Query,
    faces: &[usize],
    spacing: &Spacing,
    fit: Option<&Fit>,
    margin: f64,
) -> Result<Vec<Station>> {
    if let Some(fit) = fit {
        fit.check()?;
    }
    let positive = |x: f64| x.is_finite() && x > 0.0;
    if !(positive(spacing.spacing) && positive(spacing.row_spacing)) {
        return Err(invalid("the spacings must be above zero"));
    }
    if !(margin.is_finite() && margin >= 0.0) {
        return Err(invalid("the margin must not be negative"));
    }
    let region = Region::new(query, faces)?;
    let cannot = || invalid("the face's surface can't be evaluated");
    let too_many = || invalid("the spacing is too small: more than 16000 places");
    let mut out = Vec::new();
    let mut total = 0.0;
    for f in &region.faces {
        let k = rows_along(f);
        let across = 1 - k;
        let [a0, a1] = range(f, k);
        let middle = 0.5 * (a0 + a1);
        let line = Arc::new(f, across, middle, range(f, across)).ok_or_else(cannot)?;
        let length = line.length();
        if !(length > 0.0) {
            continue;
        }
        let wraps = closed(f, k);
        // The rows' places across, by arc length: with a fit, from the fit in from each edge to
        // the same in from the other (half a row's spacing from a pole), else at the centres of
        // equal bands.
        let rows: Vec<f64> = if let Some(fit) = fit {
            let [b0, b1] = range(f, across);
            let end = |b: f64| {
                if pole(f, across, b, length) {
                    0.5 * spacing.row_spacing
                } else {
                    fit.along(across)
                }
            };
            fitted_places(length, end(b0), end(b1), spacing.row_spacing).ok_or_else(too_many)?
        } else {
            let n = (length / spacing.row_spacing).round().max(1.0);
            if n > MAX_SPACED_PLACES as f64 {
                return Err(too_many());
            }
            (0..n as usize)
                .map(|r| (r as f64 + 0.5) * length / n)
                .collect()
        };
        if rows.len() > MAX_SPACED_PLACES {
            return Err(too_many());
        }
        for (r, at) in rows.into_iter().enumerate() {
            let b = line.param_at(f, at).ok_or_else(cannot)?;
            let row = Arc::new(f, k, b, [a0, a1]).ok_or_else(cannot)?;
            let row_length = row.length();
            let odd = spacing.staggered && r & 1 == 1;
            // The stations' places along the row, by arc length.
            // Fitted, an open row's outer stations stand the fit in from its ends; staggered, the
            // odd rows' between the even rows'.
            let places: Vec<f64> = if let (Some(fit), false) = (fit, wraps) {
                let inset = fit.along(k);
                let even = fitted_places(row_length, inset, inset, spacing.spacing)
                    .ok_or_else(too_many)?;
                if odd {
                    even.windows(2)
                        .map(|w| match w {
                            [p, q] => 0.5 * (p + q),
                            _ => 0.0,
                        })
                        .collect()
                } else {
                    even
                }
            } else {
                let n = (row_length / spacing.spacing).round();
                if !(n >= 1.0) || n > MAX_SPACED_PLACES as f64 {
                    Vec::new()
                } else {
                    // Staggered, an open row's odd rows stand between the even rows'
                    // stations, one fewer; a closed row's even and odd rows sit a quarter of
                    // a spacing either side of the middle, so none lands on the seam.
                    let (first, cells) = match (spacing.staggered, wraps, odd) {
                        (true, false, true) => (1.0, (n as usize).saturating_sub(1)),
                        (true, true, false) => (0.25, n as usize),
                        (true, true, true) => (0.75, n as usize),
                        _ => (0.5, n as usize),
                    };
                    (0..cells)
                        .map(|c| (c as f64 + first) * row_length / n)
                        .collect()
                }
            };
            total += places.len() as f64;
            if total > MAX_SPACED_PLACES as f64 {
                return Err(too_many());
            }
            for s in places {
                let a = row.param_at(f, s).ok_or_else(cannot)?;
                let station = match fit {
                    Some(_) => region.fitted_station(f, uv(k, a, b), margin),
                    None => region.station(f, uv(k, a, b), margin),
                };
                if let Some(station) = station {
                    out.push(station);
                }
            }
        }
    }
    Ok(out)
}

/// A face's area read cell by cell over its extent: rows across its rows' parameter, cells along
/// it, each cell's area its centre's area element times its size, or nothing where the centre is
/// outside the face.
struct Area {
    /// The parameter the rows run along.
    k: usize,
    /// The area from the extent's start to the end of each row (one more than the rows).
    rows: Vec<f64>,
}

impl Area {
    fn new(f: &Face) -> Area {
        let k = rows_along(f);
        let [b0, b1] = range(f, 1 - k);
        let db = (b1 - b0) / AREA_ACROSS as f64;
        let mut rows = vec![0.0];
        let mut sum = 0.0;
        for r in 0..AREA_ACROSS {
            sum += row_area(f, k, b0 + db * (r as f64 + 0.5)).0 * db;
            rows.push(sum);
        }
        Area { k, rows }
    }

    fn total(&self) -> f64 {
        self.rows.last().copied().unwrap_or(0.0)
    }
}

/// The cells of the row of face `f` at `b` across parameter `k`: each one's area per unit across
/// (its area element times its length along, nothing outside the face), and their sum.
fn row_area(f: &Face, k: usize, b: f64) -> (f64, Vec<f64>) {
    let [a0, a1] = range(f, k);
    let da = (a1 - a0) / AREA_ALONG as f64;
    let cells: Vec<f64> = (0..AREA_ALONG)
        .map(|c| {
            let at = uv(k, a0 + da * (c as f64 + 0.5), b);
            match f.query.holds(f.index, at) {
                Some(true) => f.area(at).filter(|a| a.is_finite()).unwrap_or(0.0) * da,
                _ => 0.0,
            }
        })
        .collect();
    (cells.iter().sum(), cells)
}

/// Where along a run of cells (each `width` wide from `start`, with the weights `cells`) the
/// share `t` of their sum falls: the cell it falls in, and the way through it.
fn invert(cells: &[f64], t: f64, start: f64, width: f64) -> f64 {
    let mut left = t;
    for (i, w) in cells.iter().enumerate() {
        if left <= *w && *w > 0.0 {
            return start + width * (i as f64 + left / w);
        }
        left -= w;
    }
    start + width * cells.len() as f64
}

/// `count` split over faces in proportion to their areas, the remainders to the largest
/// fractions (the first of equal ones).
fn shares(count: usize, areas: &[f64]) -> Result<Vec<usize>> {
    let total: f64 = areas.iter().sum();
    if !(total > 0.0 && total.is_finite()) {
        return Err(invalid("the face has no area"));
    }
    let exact: Vec<f64> = areas.iter().map(|a| count as f64 * a / total).collect();
    let mut out: Vec<usize> = exact.iter().map(|x| x.floor() as usize).collect();
    let mut order: Vec<usize> = (0..areas.len()).collect();
    let fraction = |i: usize| exact.get(i).map_or(0.0, |x| x - x.floor());
    order.sort_by(|a, b| fraction(*b).total_cmp(&fraction(*a)).then(a.cmp(b)));
    let given: usize = out.iter().sum();
    for i in order.into_iter().take(count.saturating_sub(given)) {
        if let Some(n) = out.get_mut(i) {
            *n += 1;
        }
    }
    Ok(out)
}

/// A place on a region's faces: the face (in the region) and its parameters there.
type Spot = (usize, [f64; 2]);

/// The spiral's places on the region's faces, `(face, parameters)`: each face's share of `count`
/// on a golden-ratio lattice, its rows' parameter by the face's area across, its other by the
/// area along the row there. Places the area's cells put just outside a face are left out.
fn spiral_places(region: &Region, count: usize) -> Result<(Vec<Spot>, f64)> {
    let areas: Vec<Area> = region.faces.iter().map(Area::new).collect();
    let totals: Vec<f64> = areas.iter().map(Area::total).collect();
    let split = shares(count, &totals)?;
    let mut out = Vec::new();
    for (i, ((f, area), n)) in region.faces.iter().zip(&areas).zip(split).enumerate() {
        let k = area.k;
        let [b0, b1] = range(f, 1 - k);
        let [a0, a1] = range(f, k);
        let db = (b1 - b0) / AREA_ACROSS as f64;
        let da = (a1 - a0) / AREA_ALONG as f64;
        let row_cells: Vec<f64> = area
            .rows
            .windows(2)
            .map(|w| match w {
                [a, b] => b - a,
                _ => 0.0,
            })
            .collect();
        for j in 0..n {
            let t = (j as f64 + 0.5) / n as f64 * area.total();
            let b = invert(&row_cells, t, b0, db);
            let (sum, cells) = row_area(f, k, b);
            if !(sum > 0.0) {
                continue;
            }
            let a = invert(&cells, ((j as f64 + 0.5) * GOLDEN).fract() * sum, a0, da);
            let at = uv(k, a, b);
            if f.query.holds(f.index, at) == Some(true) {
                out.push((i, at));
            }
        }
    }
    Ok((out, totals.iter().sum()))
}

/// `count` stations on a golden-ratio (Fibonacci) lattice over `faces` of `query`'s record, even
/// over their area: each face takes its share of the count by its area, and its stations step
/// across it by equal shares of its area while turning by the golden ratio's share of the way
/// along. On a whole sphere this is the Fibonacci sphere; on a rectangle, the Fibonacci lattice.
/// Stations nearer than `margin` to the set's boundary are left out, so fewer can come back.
///
/// # Errors
/// [`crate::SolidError::InvalidInput`] for a count past [`MAX_STATION_COUNT`], a margin that is
/// not a number or is negative, no face or more than [`super::MAX_FACES`], a face listed twice or
/// one the record lacks or can't read, or faces with no area.
pub fn spiral_stations(
    query: &Query,
    faces: &[usize],
    count: usize,
    margin: f64,
) -> Result<Vec<Station>> {
    check_count(count, margin)?;
    let region = Region::new(query, faces)?;
    let (places, _) = spiral_places(&region, count)?;
    Ok(places
        .into_iter()
        .filter_map(|(i, at)| region.station(region.faces.get(i)?, at, margin))
        .collect())
}

fn check_count(count: usize, margin: f64) -> Result<()> {
    if count > MAX_STATION_COUNT {
        return Err(invalid("at most 1000 stations"));
    }
    if !(margin.is_finite() && margin >= 0.0) {
        return Err(invalid("the margin must not be negative"));
    }
    Ok(())
}

/// One station of an even fill: its face (in the region), parameters and point.
#[derive(Clone, Copy)]
struct Place {
    face: usize,
    uv: [f64; 2],
    point: Vec3,
}

fn place(region: &Region, face: usize, uv: [f64; 2]) -> Option<Place> {
    let point = region.faces.get(face)?.surface.point(uv)?;
    Some(Place { face, uv, point })
}

/// Points in cubes `size` wide, to find the nearest of them to a point without measuring to all.
struct Bins {
    size: f64,
    cells: BTreeMap<[i64; 3], Vec<usize>>,
    /// The cubes furthest out from the first point's along any axis.
    reach: i64,
}

impl Bins {
    fn new(points: &[Vec3], size: f64) -> Bins {
        let mut cells: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
        for (i, p) in points.iter().enumerate() {
            cells.entry(Bins::cell(*p, size)).or_default().push(i);
        }
        let reach = cells
            .keys()
            .flat_map(|a| {
                cells.keys().map(move |b| {
                    (0..3)
                        .map(|k| {
                            (a.get(k).copied().unwrap_or(0) - b.get(k).copied().unwrap_or(0)).abs()
                        })
                        .max()
                        .unwrap_or(0)
                })
            })
            .max()
            .unwrap_or(0);
        Bins { size, cells, reach }
    }

    fn cell(p: Vec3, size: f64) -> [i64; 3] {
        [p.x, p.y, p.z].map(|x| (x / size).floor() as i64)
    }

    /// The index of the point of `points` (the ones binned) nearest `p`: the cubes searched shell
    /// by shell outwards until the nearest found is nearer than the next shell can be.
    fn nearest(&self, points: &[Vec3], p: Vec3) -> Option<usize> {
        let [x, y, z] = Bins::cell(p, self.size);
        let mut best: Option<(f64, usize)> = None;
        for r in 0..=self.reach + 1 {
            for i in -r..=r {
                for j in -r..=r {
                    for k in -r..=r {
                        if i.abs().max(j.abs()).max(k.abs()) != r {
                            continue;
                        }
                        for &n in self.cells.get(&[x + i, y + j, z + k]).into_iter().flatten() {
                            let d = points.get(n).map_or(f64::INFINITY, |q| q.dist(p));
                            if best.is_none_or(|(b, m)| d < b || (d == b && n < m)) {
                                best = Some((d, n));
                            }
                        }
                    }
                }
            }
            // Every point not yet seen is more than `r` cubes away along some axis.
            if best.is_some_and(|(b, _)| b <= r as f64 * self.size) {
                break;
            }
        }
        best.map(|(_, n)| n)
    }
}

/// `count` stations spread evenly over `faces` of `query`'s record together, moving from face to
/// face of the set: a centroidal Voronoi layout by Lloyd's method on samples. The faces are
/// sampled densely and evenly over their area (the spiral's lattice, [`spiral_stations`], at
/// [`FILL_SAMPLES`] times the count), keeping the samples at least `margin` from the set's
/// boundary; the stations start on the spiral of the count, and each round every station moves
/// to the middle of the samples nearer it than any other station, brought back onto the faces.
/// Each station's region of the faces is then about as large as each other's and as round as its
/// neighbours let it be: on a flat face, a hexagonal layout.
///
/// # Errors
/// As [`spiral_stations`]'s.
pub fn fill_stations(
    query: &Query,
    faces: &[usize],
    count: usize,
    margin: f64,
) -> Result<Vec<Station>> {
    check_count(count, margin)?;
    let region = Region::new(query, faces)?;
    let (start, area) = spiral_places(&region, count)?;
    let mut places: Vec<Place> = start
        .into_iter()
        .filter_map(|(i, at)| place(&region, i, at))
        .collect();
    let (dense, _) = spiral_places(&region, FILL_SAMPLES * count)?;
    let samples: Vec<Place> = dense
        .into_iter()
        .filter_map(|(i, at)| place(&region, i, at))
        .filter(|s| margin <= 0.0 || region.boundary.distance(query, s.point) >= margin)
        .collect();
    if places.len() > 1 && !samples.is_empty() {
        let h = (2.0 * area / (3.0_f64.sqrt() * places.len() as f64)).sqrt();
        let sample_points: Vec<Vec3> = samples.iter().map(|s| s.point).collect();
        let sample_bins = Bins::new(&sample_points, h);
        for _ in 0..FILL_ROUNDS {
            let points: Vec<Vec3> = places.iter().map(|p| p.point).collect();
            let bins = Bins::new(&points, h);
            let mut sums = vec![(Vec3::ZERO, 0usize); places.len()];
            for s in &sample_points {
                if let Some(slot) = bins.nearest(&points, *s).and_then(|n| sums.get_mut(n)) {
                    slot.0 += *s;
                    slot.1 += 1;
                }
            }
            for (p, (sum, n)) in places.iter_mut().zip(sums) {
                if n == 0 {
                    continue;
                }
                let middle = sum * (1.0 / n as f64);
                // The middle lies off a curved face: onto it, from the sample nearest it, on that
                // sample's face; that sample itself where the face does not hold the middle.
                let Some(near) = sample_bins
                    .nearest(&sample_points, middle)
                    .and_then(|i| samples.get(i))
                else {
                    continue;
                };
                let onto = region.faces.get(near.face).and_then(|f| {
                    let at = query.face_uv(f.index, middle, 2.0 * near.point.dist(middle))?;
                    (query.holds(f.index, at) == Some(true))
                        .then(|| place(&region, near.face, at))
                        .flatten()
                });
                *p = onto.unwrap_or(*near);
            }
        }
    }
    Ok(places
        .into_iter()
        .filter_map(|p| region.station(region.faces.get(p.face)?, p.uv, margin))
        .collect())
}

/// The directions from a sphere's centre to the vertices of an icosahedron, a vertex on +z and
/// one beside it towards +x turned by `turn` (radians) about z, with each triangle divided into
/// `frequency²`: `10 f² + 2` unit vectors, each once.
fn geodesic_directions(frequency: usize, turn: f64) -> Vec<Vec3> {
    let (z, r) = (1.0 / 5.0_f64.sqrt(), 2.0 / 5.0_f64.sqrt());
    let ring = |half: f64, z: f64| -> Vec<Vec3> {
        (0..5)
            .map(|i| {
                let a = turn + TAU * (f64::from(i) + half) / 5.0;
                Vec3::new(r * a.cos(), r * a.sin(), z)
            })
            .collect()
    };
    let (upper, lower) = (ring(0.0, z), ring(0.5, -z));
    let next = |v: &[Vec3]| -> Vec<Vec3> { v.iter().cycle().skip(1).take(5).copied().collect() };
    let (upper_next, lower_next) = (next(&upper), next(&lower));
    let mut triangles = Vec::new();
    for (((u, un), l), ln) in upper.iter().zip(&upper_next).zip(&lower).zip(&lower_next) {
        triangles.push((Vec3::Z, *u, *un));
        triangles.push((*u, *l, *un));
        triangles.push((*un, *l, *ln));
        triangles.push((-Vec3::Z, *ln, *l));
    }
    let n = frequency as f64;
    let mut out: Vec<Vec3> = Vec::new();
    for (a, b, c) in triangles {
        for i in 0..=frequency {
            for j in 0..=frequency - i {
                let (wb, wc) = (i as f64, j as f64);
                let p = (a * (n - wb - wc) + b * wb + c * wc) * (1.0 / n);
                if let Some(d) = p.normalized() {
                    if !out.iter().any(|q| q.dist(d) <= ROUNDING) {
                        out.push(d);
                    }
                }
            }
        }
    }
    out
}

/// A face's sphere, as its record names it: its frame and radius.
fn sphere(query: &Query, f: &Face) -> Option<(Axes, f64)> {
    match query.record().faces.get(f.index)?.surface.as_ref()? {
        Surface::Sphere { frame, radius } => Some((Axes::new(frame), *radius)),
        _ => None,
    }
}

/// The stations on `faces` of `query`'s record (all of one sphere) at the vertices of a geodesic
/// sphere of `frequency`: an icosahedron in the first face's sphere's frame, a vertex at its pole
/// and one towards its x axis turned by `turn` (radians) about the pole, each triangle divided
/// into `frequency²`. A vertex on the faces (on a face's surface, inside its loops or on one of
/// them, as a seam is) and at least `margin` from the set's boundary stands a station, so a whole
/// sphere takes all `10 f² + 2` and a dome the part over it. A vertex where the face's
/// parameters give no normal (a pole) takes the sphere's own there, the way the face's normal
/// turns, and the frame's x axis along.
///
/// # Errors
/// [`crate::SolidError::InvalidInput`] for a frequency of 0 or past
/// [`MAX_GEODESIC_FREQUENCY`], a turn that is not a number, a margin that is not a number or is
/// negative, no face or more than [`super::MAX_FACES`], a face listed twice or one the record
/// lacks or can't read, or faces that are not all of one sphere.
pub fn geodesic_stations(
    query: &Query,
    faces: &[usize],
    frequency: usize,
    turn: f64,
    margin: f64,
) -> Result<Vec<Station>> {
    if !(1..=MAX_GEODESIC_FREQUENCY).contains(&frequency) {
        return Err(invalid("a geodesic's frequency is 1 to 9"));
    }
    if !turn.is_finite() {
        return Err(invalid("the turn must be a number"));
    }
    check_count(0, margin)?;
    let region = Region::new(query, faces)?;
    let not_one = || invalid("a geodesic layout needs faces of one sphere");
    let (frame, radius) = region
        .faces
        .first()
        .and_then(|f| sphere(query, f))
        .ok_or_else(not_one)?;
    let r = radius.abs();
    let near = ROUNDING * (1.0 + r);
    // Each face with the way its natural normal turns against the sphere's outward one, read
    // in the middle of its extent.
    let mut sides = Vec::new();
    for f in &region.faces {
        let (a, fr) = sphere(query, f).ok_or_else(not_one)?;
        if a.o.dist(frame.o) > near || (fr.abs() - r).abs() > near {
            return Err(not_one());
        }
        let [[u0, u1], [v0, v1]] = f.extent;
        let (p, su, sv) = f
            .surface
            .derivs(0.5 * (u0 + u1), 0.5 * (v0 + v1))
            .ok_or_else(not_one)?;
        sides.push(if su.cross(sv).dot(p - frame.o) < 0.0 {
            -1.0
        } else {
            1.0
        });
    }
    let accept = ON_SPHERE * (1.0 + r);
    let mut out = Vec::new();
    for d in geodesic_directions(frequency, turn) {
        let outward = frame.x * d.x + frame.y * d.y + frame.z * d.z;
        let p = frame.o + outward * r;
        let found = region.faces.iter().zip(&sides).find_map(|(f, side)| {
            if !query.face_contains(f.index, p, accept) {
                return None;
            }
            let at = query.face_uv(f.index, p, accept)?;
            let (_, su, sv) = f.surface.derivs(at[0], at[1])?;
            if su.cross(sv).normalized().is_some() {
                return Some(f.station_at(at, margin, &region.boundary));
            }
            // Where the parameters give no normal: the sphere's own.
            let to_edge = region.boundary.distance(query, p);
            if to_edge < margin {
                return Some(None);
            }
            let natural = outward * *side;
            let normal = if f.flip { -natural } else { natural };
            let along = (frame.x - normal * frame.x.dot(normal)).normalized()?;
            Some(Some(Station {
                point: p,
                normal,
                along,
                margin: to_edge,
            }))
        });
        out.extend(found.flatten());
    }
    Ok(out)
}

#[cfg(test)]
#[path = "face_pattern_layouts_tests.rs"]
mod tests;
