//! The check: how close two placed parts come to each other.
//!
//! A part is its outline as a closed chain of points and the side its material is on. [`gap`] gives
//! the smallest distance between the surfaces of two parts, negative where one reaches into the
//! material of the other. Turning a pair through a cycle and asking for the gap at each step shows
//! how a hub sits on its shaft: flanks that stay in contact give a gap of nothing at every step, a
//! fit gives its clearance, and an overlap shows as a negative gap.
//!
//! The distance is measured from the points of each outline to the chords of the other, so it is as
//! fine as the outlines are: with chords of a few thousandths of a millimetre it is good to a
//! millionth.

use std::cell::Cell;

use crate::error::teeth::{TeethFailure, Why};
use crate::teeth::outline::{rotated, Outline, Point, Ring};
use crate::teeth::MAX_LENGTH;
use crate::SolidError;

/// On which side of its outline a part's material is.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Material {
    /// Inside the outline: a splined shaft.
    Inside,
    /// Outside the outline, which is then the part's opening: the hub that goes on a splined shaft.
    Outside,
}

/// A part in the plane of its teeth.
#[derive(Clone, Debug, PartialEq)]
pub struct Body {
    points: Vec<Point>,
    material: Material,
}

/// The most points a body may have.
pub const MAX_POINTS: usize = 8_000_000;

impl Body {
    /// A part from its outline, a closed chain of at least three points that runs counter-clockwise
    /// (as [`crate::teeth::Outline::points`] gives it), without repeating the first point, and does
    /// not cross itself. Points that meet, chords that share an end and stretches that only touch
    /// do not cross.
    pub fn new(points: Vec<Point>, material: Material) -> crate::Result<Body> {
        Body::new_typed(points, material)
            .map_err(|e| SolidError::teeth("teeth::mesh::Body::new", e))
    }

    /// [`Body::new`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
    pub(crate) fn new_typed(points: Vec<Point>, material: Material) -> Result<Body, TeethFailure> {
        if points.len() < 3 || points.len() > MAX_POINTS {
            let (value, up_to) = (points.len() as f64, MAX_POINTS as f64);
            return Err(TeethFailure::OutOfRange {
                what: "points of the outline",
                value,
                above: 2.0,
                up_to,
            });
        }
        if points
            .iter()
            .flatten()
            .any(|v| !(v.is_finite() && v.abs() <= MAX_LENGTH))
        {
            return Err(TeethFailure::Invalid(Why::BodyNotFinite));
        }
        let ring = Ring {
            chain: &points,
            teeth: 1,
        };
        if !(ring.doubled_area() > 0.0) {
            return Err(TeethFailure::Invalid(Why::NotCounterClockwise));
        }
        let size = points
            .iter()
            .flatten()
            .fold(0.0_f64, |most, v| most.max(v.abs()));
        if ring.crosses_itself(1e-9 * (1.0 + size))? {
            return Err(TeethFailure::Invalid(Why::BodyCrossesItself));
        }
        Ok(Body { points, material })
    }

    /// A part from an outline, which [`crate::teeth::Outline::new`] has already checked as
    /// [`Body::new`] would.
    pub(super) fn of_outline(outline: &Outline, material: Material) -> Result<Body, TeethFailure> {
        let points = outline.points();
        if points.len() < 3 || points.len() > MAX_POINTS {
            let (value, up_to) = (points.len() as f64, MAX_POINTS as f64);
            return Err(TeethFailure::OutOfRange {
                what: "points of the outline",
                value,
                above: 2.0,
                up_to,
            });
        }
        Ok(Body { points, material })
    }

    /// The part turned about its axis (the origin of its outline) by `angle` and then moved so that
    /// its axis is at `axis`.
    #[must_use]
    pub fn placed(&self, angle: f64, axis: Point) -> Body {
        let points = self
            .points
            .iter()
            .map(|&p| rotated(p, angle))
            .map(|[x, y]| [x + axis[0], y + axis[1]])
            .collect();
        Body {
            points,
            material: self.material,
        }
    }

    /// The points of the outline.
    pub fn points(&self) -> &[Point] {
        &self.points
    }

    /// The side the material is on.
    pub fn material(&self) -> Material {
        self.material
    }
}

/// The chords of a closed chain: each point with the next, the last with the first.
fn edges(points: &[Point]) -> impl Iterator<Item = (Point, Point)> + '_ {
    points
        .iter()
        .copied()
        .zip(points.iter().copied().cycle().skip(1))
}

/// The smallest distance found between two parts.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Gap {
    /// The distance between the surfaces; negative where one part is inside the material of the
    /// other.
    pub distance: f64,
    /// The point of one outline at which it was found.
    pub at: Point,
}

/// The smallest distance between the surfaces of two parts, looking no farther than `reach` from
/// either surface; none when the parts are farther apart than that everywhere.
///
/// A point of one part that lies in the material of the other counts with a negative distance. Each
/// part is looked at from the points of its outline and, on a chord longer than the reach (a long
/// flank, the flat of a polygon), from points along it no farther apart than the reach, so a corner
/// of one part in a long straight side of the other is found from either side. Where the two
/// outlines cross without any of those points lying in the other part (two long edges crossing, an
/// overlap thinner than the reach), the crossing itself is looked at closely and reported as an
/// overlap. How deep an overlap is, is known only up to `reach`, and no finer than the points
/// looked from.
///
/// The work of one measurement is bounded whatever the reach and the outlines. The points looked
/// from along the chords are at most four for each point of the two outlines, and a million more. A
/// reach so small that more would be needed (smaller than about a fifth of the outlines' mean
/// chord) sets them farther apart than the reach, and every chord is then looked at for a crossing
/// instead, so that an overlap is still found; the gap is then still found wherever it is within
/// the reach of a point of either outline. A reach must be more than a thousand-millionth of the
/// parts' farthest distance from the origin (and of a millimetre): below that the numbers cannot
/// tell a point from its neighbour, and such a reach is refused. Outlines so intricate that one
/// measurement would need more than [`MOST_WORK`] comparisons are refused rather than measured
/// slowly. Parts with points that are not numbers (a body placed at a turn or a place that is not a
/// number) are refused.
pub fn gap(a: &Body, b: &Body, reach: f64) -> crate::Result<Option<Gap>> {
    gap_typed(a, b, reach).map_err(|e| SolidError::teeth("teeth::mesh::gap", e))
}

/// [`gap`] with its failure as a [`TeethFailure`], for the tooth module's own callers.
pub(crate) fn gap_typed(a: &Body, b: &Body, reach: f64) -> Result<Option<Gap>, TeethFailure> {
    let reach = crate::teeth::in_range("reach", reach, 0.0, MAX_LENGTH)?;
    let mut size = 0.0_f64;
    for v in a.points.iter().chain(&b.points).flatten() {
        if !v.is_finite() {
            return Err(TeethFailure::Invalid(Why::PartNotFinite));
        }
        size = size.max(v.abs());
    }
    let finest = 1e-9 * (1.0 + size);
    if !(reach > finest) {
        return Err(TeethFailure::OutOfRange {
            what: "reach",
            value: reach,
            above: finest,
            up_to: MAX_LENGTH,
        });
    }
    // No two points of the parts are farther apart than the corners of the box about both: a reach
    // beyond that finds nothing more.
    let corners = |pick: fn(f64, f64) -> f64, start: f64| {
        a.points
            .iter()
            .chain(&b.points)
            .fold([start, start], |[x, y], p| [pick(x, p[0]), pick(y, p[1])])
    };
    let (low, high) = (
        corners(f64::min, f64::INFINITY),
        corners(f64::max, f64::NEG_INFINITY),
    );
    let reach = reach.min(2.0 * (high[0] - low[0]).hypot(high[1] - low[1]) + finest);
    let (grid_a, grid_b) = (Grid::new(a, reach), Grid::new(b, reach));
    let too_much = || grid_a.work.get().saturating_add(grid_b.work.get()) > MOST_WORK;
    let stride = stride(a, b, reach);
    let mut least: Option<Gap> = None;
    // The chords of the first part on which the two outlines can cross.
    let mut may_cross: Vec<u32> = Vec::new();
    for (first, from, own, onto) in [(true, a, &grid_a, &grid_b), (false, b, &grid_b, &grid_a)] {
        // The point looked from before this one: its chord, its distance from the other part, and
        // the point.
        let mut before: Option<(usize, f64, Point)> = None;
        let mut opening: Option<(f64, Point)> = None;
        // Between two points looked from, the outline is straight. It can reach the other part's
        // surface there only if the two distances together are no more than the piece is long.
        let mut note = |chord: usize, was: f64, from_point: Point, is: f64, to_point: Point| {
            let piece = (from_point[0] - to_point[0]).hypot(from_point[1] - to_point[1]);
            if stride > reach || was + is <= piece * (1.0 + 1e-9) {
                if let Ok(index) = u32::try_from(chord) {
                    if may_cross.last() != Some(&index) {
                        may_cross.push(index);
                    }
                }
            }
        };
        for (i, (p, q)) in edges(&from.points).enumerate() {
            for at in along(p, q, stride) {
                let found = onto.signed_at(at, reach);
                if let Some((distance, nearest)) = found {
                    if least.is_none_or(|least| distance < least.distance) {
                        least = Some(Gap { distance, at });
                    }
                    // The nearest point of the other part may itself lie in this part's material
                    // though the point looked from is not in the other's (a corner of the other
                    // part over a long side of this one). The other part's own points find that,
                    // unless the distance is close to the reach.
                    if distance >= 0.0
                        && distance * distance + stride * stride / 4.0 > reach * reach
                    {
                        if let Some(back) = own.signed(nearest, reach) {
                            if back < 0.0 && least.is_none_or(|least| back < least.distance) {
                                least = Some(Gap {
                                    distance: back,
                                    at: nearest,
                                });
                            }
                        }
                    }
                }
                if first {
                    let is = found.map_or(f64::INFINITY, |(distance, _)| distance.abs());
                    if let Some((chord, was, from_point)) = before {
                        note(chord, was, from_point, is, at);
                    }
                    before = Some((i, is, at));
                    opening = opening.or(Some((is, at)));
                }
            }
            if too_much() {
                return Err(TOO_INTRICATE);
            }
        }
        if let (Some((chord, was, from_point)), Some((is, to_point))) = (before, opening) {
            note(chord, was, from_point, is, to_point);
        }
    }
    if !least.is_some_and(|found| found.distance < 0.0) {
        if let Some(found) = crossing(&grid_a, &grid_b, &may_cross, stride.min(reach), reach)? {
            least = Some(found);
        }
    }
    Ok(least)
}

/// The most comparisons (of a point with a chord, or of a chord with the cells it passes) one
/// measurement may make: about a second's work. A measurement that would need more is refused.
pub const MOST_WORK: u64 = 400_000_000;

const TOO_INTRICATE: TeethFailure = TeethFailure::Invalid(Why::MeasureTooIntricate);

/// How far apart the points are that a chord is looked at from: the reach, or more where the two
/// parts together would otherwise need more than four points along their chords for each of their
/// own points, and a million more.
fn stride(a: &Body, b: &Body, reach: f64) -> f64 {
    let total: f64 = edges(&a.points)
        .chain(edges(&b.points))
        .map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]))
        .sum();
    let most = 4.0 * (a.points.len() + b.points.len()) as f64 + 1_000_000.0;
    reach.max(total / most)
}

/// How many points one measurement of `a` against `b` looks from at `reach`: on every chord of both
/// outlines its start and the points along it (see `along`). The count hangs on the lengths of the
/// chords alone, so it is the same wherever the parts are placed and however they are turned.
pub(super) fn looked_from(a: &Body, b: &Body, reach: f64) -> f64 {
    let stride = stride(a, b, reach);
    edges(&a.points)
        .chain(edges(&b.points))
        .map(|(p, q)| ((p[0] - q[0]).hypot(p[1] - q[1]) / stride).ceil().max(1.0))
        .sum()
}

/// The points a chord is looked at from: its start, and points along it no farther apart than
/// `stride` (not its end, which is the next chord's start).
fn along(p: Point, q: Point, stride: f64) -> impl Iterator<Item = Point> {
    let length = (p[0] - q[0]).hypot(p[1] - q[1]);
    // The stride keeps the count of all of them bounded (see `stride`), so the count is in range.
    let pieces = (length / stride).ceil().max(1.0).min(f64::from(u32::MAX)) as u32;
    (0..pieces).map(move |i| {
        let part = f64::from(i) / f64::from(pieces);
        [p[0] + (q[0] - p[0]) * part, p[1] + (q[1] - p[1]) * part]
    })
}

/// Where the chord from `p` to `q` crosses the chord from `u` to `v`, each passing from one side of
/// the other to the other side (chords that only touch do not cross).
fn crossing_point(p: Point, q: Point, u: Point, v: Point) -> Option<Point> {
    let side = |from: Point, to: Point, of: Point| {
        (to[0] - from[0]) * (of[1] - from[1]) - (to[1] - from[1]) * (of[0] - from[0])
    };
    let (p_side, q_side) = (side(u, v, p), side(u, v, q));
    let (u_side, v_side) = (side(p, q, u), side(p, q, v));
    if p_side * q_side < 0.0 && u_side * v_side < 0.0 {
        let part = p_side / (p_side - q_side);
        Some([p[0] + (q[0] - p[0]) * part, p[1] + (q[1] - p[1]) * part])
    } else {
        None
    }
}

/// An overlap at a place where the two outlines cross, if there is one: `near` are the chords of
/// the first part to try. Beside a crossing, each outline runs into the other part's material on
/// one side; points on both chords at `step` from the crossing (no more than the reach) and then
/// ever closer are tried until one lies in the other part.
fn crossing(
    grid_a: &Grid,
    grid_b: &Grid,
    near: &[u32],
    step: f64,
    reach: f64,
) -> Result<Option<Gap>, TeethFailure> {
    const CROSSINGS: u32 = 4096; // the most crossings looked at closely
    let mut looked = 0;
    for &index in near {
        let i = index as usize;
        let (p, q) = (grid_a.point(i), grid_a.point(i + 1));
        for j in grid_b.chords_along(p, q) {
            let (u, v) = (grid_b.point(j), grid_b.point(j + 1));
            let Some(x) = crossing_point(p, q, u, v) else {
                continue;
            };
            for halving in 0..=14 {
                let step = step / f64::from(1_u32 << halving);
                let mut deepest: Option<Gap> = None;
                for (onto, from, to) in [(grid_b, p, q), (grid_a, u, v)] {
                    let length = (to[0] - from[0]).hypot(to[1] - from[1]);
                    let reached = (x[0] - from[0]).hypot(x[1] - from[1]);
                    for place in [reached - step, reached + step] {
                        if !(length > 0.0 && place >= 0.0 && place <= length) {
                            continue;
                        }
                        let at = [
                            from[0] + (to[0] - from[0]) * place / length,
                            from[1] + (to[1] - from[1]) * place / length,
                        ];
                        if let Some(distance) = onto.signed(at, reach) {
                            if distance < 0.0
                                && deepest.is_none_or(|found| distance < found.distance)
                            {
                                deepest = Some(Gap { distance, at });
                            }
                        }
                    }
                }
                if deepest.is_some() {
                    return Ok(deepest);
                }
            }
            looked += 1;
            if looked >= CROSSINGS {
                return Ok(None);
            }
        }
        if grid_a.work.get().saturating_add(grid_b.work.get()) > MOST_WORK {
            return Err(TOO_INTRICATE);
        }
    }
    Ok(None)
}

/// The chords of a part sorted into square cells no smaller than the reach, so that the chords near
/// a point are in the point's cell and the eight around it. The chords of each cell follow each
/// other in `entries`, from the cell's place in `starts` to the next cell's.
struct Grid<'a> {
    body: &'a Body,
    low: Point,
    high: Point,
    cell: f64,
    columns: usize,
    rows: usize,
    starts: Vec<u32>,
    entries: Vec<u32>,
    /// The comparisons made so far against this part.
    work: Cell<u64>,
}

/// The fewest cells along one side of a grid allowed when the reach is small; a part of many points
/// is allowed more, about twice as many cells in all as it has points.
const GRID: f64 = 384.0;

impl<'a> Grid<'a> {
    fn new(body: &'a Body, reach: f64) -> Grid<'a> {
        let fold = |pick: fn(f64, f64) -> f64, start: f64| {
            body.points
                .iter()
                .fold([start, start], |[x, y], p| [pick(x, p[0]), pick(y, p[1])])
        };
        let (low, high) = (
            fold(f64::min, f64::INFINITY),
            fold(f64::max, f64::NEG_INFINITY),
        );
        let extent = (high[0] - low[0]).max(high[1] - low[1]);
        let lengths = || edges(&body.points).map(|(p, q)| (p[0] - q[0]).hypot(p[1] - q[1]));
        // A chord is entered in every cell it passes through, so a long straight piece (a long
        // flank, the flat of a polygon) does not force large cells. Cells no smaller than the reach
        // keep the search to a point's own cell and the eight around it; they grow only until the
        // chords together take a bounded count of places. Doubling from the smallest number to the
        // largest takes fewer than 2100 rounds.
        let count = body.points.len() as f64;
        let allowed = 6.0 * count + 1_000_000.0;
        let side = GRID.max((2.0 * count).sqrt());
        let mut cell = reach.max(extent / side).max(f64::MIN_POSITIVE);
        for _ in 0..2100 {
            let places: f64 = lengths()
                .map(|length| 2.0 * (length / cell).ceil() + 2.0)
                .sum();
            if !(places > allowed) {
                break;
            }
            cell *= 2.0;
        }
        let along =
            |from: f64, to: f64| (((to - from) / cell).floor().max(0.0).min(side) as usize) + 1;
        let (columns, rows) = (along(low[0], high[0]), along(low[1], high[1]));
        let mut grid = Grid {
            body,
            low,
            high,
            cell,
            columns,
            rows,
            starts: Vec::new(),
            entries: Vec::new(),
            work: Cell::new(0),
        };
        // Each chord with the cells it passes through, walked in steps no longer than a cell (each
        // step lies in at most four cells), then gathered cell by cell.
        let mut placed: Vec<(u32, u32)> = Vec::new();
        let mut of_chord: Vec<u32> = Vec::new();
        for (i, (p, q)) in edges(&body.points).enumerate() {
            let Ok(index) = u32::try_from(i) else { break };
            let length = (p[0] - q[0]).hypot(p[1] - q[1]);
            let steps = (length / cell).ceil().max(1.0).min(allowed) as usize;
            of_chord.clear();
            let mut from = p;
            for step in 1..=steps {
                let part = step as f64 / steps as f64;
                let to = [p[0] + (q[0] - p[0]) * part, p[1] + (q[1] - p[1]) * part];
                let ((c0, r0), (c1, r1)) = (grid.cell_of(from), grid.cell_of(to));
                for column in c0.min(c1)..=c0.max(c1) {
                    for row in r0.min(r1)..=r0.max(r1) {
                        of_chord.extend(u32::try_from(row * columns + column));
                    }
                }
                from = to;
            }
            of_chord.sort_unstable();
            of_chord.dedup();
            placed.extend(of_chord.iter().map(|&c| (c, index)));
        }
        let mut starts = vec![0_u32; columns * rows + 1];
        for &(c, _) in &placed {
            if let Some(n) = starts.get_mut(c as usize + 1) {
                *n = n.saturating_add(1);
            }
        }
        let mut sum = 0_u32;
        for n in &mut starts {
            sum = sum.saturating_add(*n);
            *n = sum;
        }
        let mut next = starts.clone();
        let mut entries = vec![0_u32; placed.len()];
        for &(c, index) in &placed {
            if let Some(n) = next.get_mut(c as usize) {
                if let Some(slot) = entries.get_mut(*n as usize) {
                    *slot = index;
                    *n = n.saturating_add(1);
                }
            }
        }
        (grid.starts, grid.entries) = (starts, entries);
        grid
    }

    /// The squared distance from `p` to the cell, as far as the chords in it reach: the cells of
    /// the last column and row hold all that lies beyond them.
    fn distance_to_cell(&self, p: Point, column: usize, row: usize) -> f64 {
        let off = |v: f64, low: f64, index: usize, count: usize| {
            let from = low + index as f64 * self.cell;
            let to = if index + 1 >= count {
                f64::INFINITY
            } else {
                from + self.cell
            };
            if v < from {
                from - v
            } else if v > to {
                v - to
            } else {
                0.0
            }
        };
        let (dx, dy) = (
            off(p[0], self.low[0], column, self.columns),
            off(p[1], self.low[1], row, self.rows),
        );
        dx * dx + dy * dy
    }

    /// The chords entered in one cell.
    fn in_cell(&self, column: usize, row: usize) -> &[u32] {
        let c = row * self.columns + column;
        match (self.starts.get(c), self.starts.get(c + 1)) {
            (Some(&from), Some(&to)) => self.entries.get(from as usize..to as usize).unwrap_or(&[]),
            _ => &[],
        }
    }

    /// The cell of a point of the part.
    fn cell_of(&self, p: Point) -> (usize, usize) {
        let along = |v: f64, low: f64, count: usize| {
            (((v - low) / self.cell).floor().max(0.0).min(count as f64) as usize)
                .min(count.saturating_sub(1))
        };
        (
            along(p[0], self.low[0], self.columns),
            along(p[1], self.low[1], self.rows),
        )
    }

    /// The chords entered in the cells around the chord from `p` to `q`, each once.
    fn chords_along(&self, p: Point, q: Point) -> Vec<usize> {
        let mut found: Vec<usize> = Vec::new();
        // Only the stretch of the chord within a cell of the part's box can meet its cells.
        let (mut enter, mut leave) = (0.0_f64, 1.0_f64);
        for (a, b, lo, hi) in [
            (p[0], q[0], self.low[0], self.high[0]),
            (p[1], q[1], self.low[1], self.high[1]),
        ] {
            let (lo, hi, d) = (lo - self.cell, hi + self.cell, b - a);
            if d == 0.0 {
                if a < lo || a > hi {
                    return found;
                }
            } else {
                let (t0, t1) = ((lo - a) / d, (hi - a) / d);
                enter = enter.max(t0.min(t1));
                leave = leave.min(t0.max(t1));
            }
        }
        if !(enter <= leave) {
            return found;
        }
        let point = |t: f64| [p[0] + (q[0] - p[0]) * t, p[1] + (q[1] - p[1]) * t];
        let (from, to) = (point(enter), point(leave));
        let length = (from[0] - to[0]).hypot(from[1] - to[1]);
        // Within the box a chord passes at most this many cells.
        let limit = 2.0 * (self.columns + self.rows) as f64 + 4.0;
        let steps = (length / self.cell).ceil().max(1.0).min(limit) as usize;
        for step in 0..=steps {
            let part = step as f64 / steps as f64;
            let (column, row) = self.cell_of([
                from[0] + (to[0] - from[0]) * part,
                from[1] + (to[1] - from[1]) * part,
            ]);
            for r in row.saturating_sub(1)..=(row + 1).min(self.rows.saturating_sub(1)) {
                for c in column.saturating_sub(1)..=(column + 1).min(self.columns.saturating_sub(1))
                {
                    found.extend(self.in_cell(c, r).iter().map(|&index| index as usize));
                }
            }
        }
        self.work.set(
            self.work
                .get()
                .saturating_add(found.len() as u64 + steps as u64),
        );
        found.sort_unstable();
        found.dedup();
        found
    }

    /// The point of the outline with this index (the indices go around).
    fn point(&self, i: usize) -> Point {
        let n = self.body.points.len();
        self.body
            .points
            .get(i % n.max(1))
            .copied()
            .unwrap_or([0.0, 0.0])
    }

    /// The distance from `p` to the part's surface if it is within `reach`, negative inside the
    /// material.
    fn signed(&self, p: Point, reach: f64) -> Option<f64> {
        self.signed_at(p, reach).map(|(distance, _)| distance)
    }

    /// As [`Grid::signed`], with the point of the surface nearest to `p`.
    fn signed_at(&self, p: Point, reach: f64) -> Option<(f64, Point)> {
        if p[0] < self.low[0] - reach
            || p[0] > self.high[0] + reach
            || p[1] < self.low[1] - reach
            || p[1] > self.high[1] + reach
        {
            return None;
        }
        let (column, row) = self.cell_of(p);
        // Squared distance, chord, place along the chord
        let mut best: Option<(f64, usize, f64)> = None;
        let mut compared = 1_u64;
        // The point's own cell first, then the eight around it; a cell all farther away than the
        // nearest chord found so far is passed by (a chord in it that comes nearer passes through a
        // nearer cell as well).
        let around =
            (row.saturating_sub(1)..=(row + 1).min(self.rows.saturating_sub(1))).flat_map(|r| {
                (column.saturating_sub(1)..=(column + 1).min(self.columns.saturating_sub(1)))
                    .map(move |c| (c, r))
            });
        for (c, r) in
            std::iter::once((column, row)).chain(around.filter(|&cell| cell != (column, row)))
        {
            if let Some((least, _, _)) = best {
                if self.distance_to_cell(p, c, r) > least {
                    continue;
                }
            }
            {
                for &index in self.in_cell(c, r) {
                    compared += 1;
                    let i = index as usize;
                    let (a, b) = (self.point(i), self.point(i + 1));
                    let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
                    let length2 = ex * ex + ey * ey;
                    if length2 == 0.0 {
                        continue;
                    }
                    let t = (((p[0] - a[0]) * ex + (p[1] - a[1]) * ey) / length2).clamp(0.0, 1.0);
                    let (dx, dy) = (p[0] - (a[0] + t * ex), p[1] - (a[1] + t * ey));
                    let d2 = dx * dx + dy * dy;
                    if best.is_none_or(|(least, _, _)| d2 < least) {
                        best = Some((d2, i, t));
                    }
                }
            }
        }
        self.work.set(self.work.get().saturating_add(compared));
        let (d2, i, t) = best?;
        let distance = d2.sqrt();
        if distance > reach {
            return None;
        }
        let in_material = self.inside_outline(p, i, t) == (self.body.material == Material::Inside);
        let (a, b) = (self.point(i), self.point(i + 1));
        let nearest = [a[0] + t * (b[0] - a[0]), a[1] + t * (b[1] - a[1])];
        Some((if in_material { -distance } else { distance }, nearest))
    }

    /// Whether `p`, whose nearest point of the outline is at `t` along chord `i`, is inside the
    /// outline.
    fn inside_outline(&self, p: Point, i: usize, t: f64) -> bool {
        let n = self.body.points.len();
        // The outward normal of a chord of a counter-clockwise outline (not of unit length where it
        // is used alone).
        let normal = |k: usize| {
            let (a, b) = (self.point(k), self.point(k + 1));
            let (ex, ey) = (b[0] - a[0], b[1] - a[1]);
            let length = ex.hypot(ey);
            if length > 0.0 {
                [ey / length, -ex / length]
            } else {
                [0.0, 0.0]
            }
        };
        // The normal of chord `k` or, where repeated points make it a chord of no length, of the
        // first chord with a length met going on from it by `step` (1 forwards, n - 1 backwards).
        let with_length = |k: usize, step: usize| {
            let mut k = k % n.max(1);
            for _ in 0..n {
                let m = normal(k);
                if m[0] != 0.0 || m[1] != 0.0 {
                    return m;
                }
                k = (k + step) % n.max(1);
            }
            [0.0, 0.0]
        };
        // At a corner the two chords that meet there decide together.
        let (corner, [nx, ny]) = if t <= 0.0 {
            let (m, k) = (with_length(i + n - 1, n.saturating_sub(1)), normal(i));
            (self.point(i), [m[0] + k[0], m[1] + k[1]])
        } else if t >= 1.0 {
            let (m, k) = (normal(i), with_length(i + 1, 1));
            (self.point(i + 1), [m[0] + k[0], m[1] + k[1]])
        } else {
            (self.point(i), normal(i))
        };
        (p[0] - corner[0]) * nx + (p[1] - corner[1]) * ny <= 0.0
    }
}
