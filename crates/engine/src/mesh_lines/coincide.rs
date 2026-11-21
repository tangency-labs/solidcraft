//! Sketch curves that lie on a body's drawn lines. A finished sketch under an extrusion's
//! outline is seen edge on along that outline, so the view drawing would draw the outline twice:
//! once as the body's line and once as the sketch's. A cutting service reads the second copy as
//! another kind of line (#19507), so the body's line draws that stretch and the sketch's is
//! left out.
use solidcore_record::Vec2;
use solidcraft_geom::prelude::*;

use super::projection::{Line, Path};

/// Lines whose directions differ by more than this (the sine of the angle between them) do not
/// draw each other's stretch: a sketch line crossing an edge keeps its whole length. A sketch arc
/// and the body's arc are sampled apart, so their chords can lean a little against each other.
const MAX_LEAN: f64 = 0.3;

/// Cells of the body-line grid along the drawing's longer side, at most.
const GRID_CELLS: f64 = 64.;

/// The parts of `sketch` that no body line in `lines` draws. A stretch of a sketch run within
/// `reach` of a body line running the same way goes; a visible sketch stretch only goes under a
/// visible body line, a hidden one under any. `reach` covers both curves' chord tolerance, as each
/// is sampled on its own.
pub(crate) fn sketch_off_lines(sketch: Vec<Path>, lines: &[Line], reach: f64) -> Vec<Path> {
    if sketch.is_empty() || lines.is_empty() || !(reach > 0.) {
        return sketch;
    }
    let grid = Grid::new(lines, reach);
    let mut seen = vec![usize::MAX; lines.len()];
    let mut stamp = 0usize;
    let mut result = Vec::new();
    for path in sketch {
        let mut run: Vec<Vec2> = Vec::new();
        for pair in path.points.windows(2) {
            let (p, q) = (pair[0], pair[1]);
            let mut drawn: Vec<(f64, f64)> = Vec::new();
            if p.dist(q) > 0. {
                for i in grid.near(p, q) {
                    if seen[i] == stamp {
                        continue;
                    }
                    seen[i] = stamp;
                    let line = &lines[i];
                    if line.hidden && !path.hidden {
                        continue;
                    }
                    if let Some(span) = within(p, q, line.a, line.b, reach) {
                        drawn.push(span);
                    }
                }
            }
            stamp += 1;
            let mut cursor = 0.;
            for (lo, hi) in union(drawn) {
                keep(&mut run, &mut result, path.hidden, p, q, cursor, lo);
                end(&mut run, &mut result, path.hidden);
                cursor = hi;
            }
            keep(&mut run, &mut result, path.hidden, p, q, cursor, 1.);
        }
        end(&mut run, &mut result, path.hidden);
    }
    result
}

/// Add the stretch of `p`..`q` from `lo` to `hi` to the open run, starting one where it doesn't join.
fn keep(run: &mut Vec<Vec2>, result: &mut Vec<Path>, hidden: bool, p: Vec2, q: Vec2, lo: f64, hi: f64) {
    if hi <= lo {
        return;
    }
    let (a, b) = (p + (q - p) * lo, p + (q - p) * hi);
    if run.last().is_some_and(|last| last.dist(a) > 0.) {
        end(run, result, hidden);
    }
    if run.is_empty() {
        run.push(a);
    }
    run.push(b);
}

fn end(run: &mut Vec<Vec2>, result: &mut Vec<Path>, hidden: bool) {
    let points = std::mem::take(run);
    if points.len() > 1 {
        result.push(Path { points, hidden });
    }
}

/// Sorted, joined spans.
fn union(mut spans: Vec<(f64, f64)>) -> Vec<(f64, f64)> {
    spans.sort_by(|a, b| a.0.total_cmp(&b.0));
    let mut out: Vec<(f64, f64)> = Vec::new();
    for (lo, hi) in spans {
        match out.last_mut() {
            Some(last) if lo <= last.1 => last.1 = last.1.max(hi),
            _ => out.push((lo, hi)),
        }
    }
    out
}

/// The span of `p`..`q` (as 0..1) within `reach` of the segment `a`..`b`, when the two run the
/// same way. The points within reach of a segment form a capsule, which is convex, so the span is
/// one interval: the hull of the spans in its rectangle and its two end disks.
///
/// The span counts only where the two run together to an end: each end of the span is an end of
/// `p`..`q` or lies at `a` or `b`. Two lines that cross at a shallow angle come within reach of each
/// other for a short stretch whose ends lie away from both lines' ends; there the sketch line is
/// another line, so it keeps that stretch. Two chords of one curve stay within reach of each other
/// over their common length, so their span ends at a chord's end.
fn within(p: Vec2, q: Vec2, a: Vec2, b: Vec2, reach: f64) -> Option<(f64, f64)> {
    let (d, e) = (q - p, b - a);
    let (dl, el) = (d.len(), e.len());
    if dl <= 0. {
        return None;
    }
    if el > 0. && (d.x * e.y - d.y * e.x).abs() > MAX_LEAN * dl * el {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    let mut take = |span: Option<(f64, f64)>| {
        if let Some((s, t)) = span {
            lo = lo.min(s);
            hi = hi.max(t);
        }
    };
    take(disk(p, d, a, reach));
    take(disk(p, d, b, reach));
    if el > 0. {
        // In the segment's frame: along it `u` in 0..el, across it `v` within reach.
        let (along, across) = (e / el, Vec2::new(-e.y / el, e.x / el));
        let (u0, du) = ((p - a).dot(along), d.dot(along));
        let (v0, dv) = ((p - a).dot(across), d.dot(across));
        take(slab(u0, du, 0., el).and_then(|s| meet(s, slab(v0, dv, -reach, reach)?)));
    }
    // Floating-point slack for a span end that lies on the end disk's rim.
    let at_end = |t: f64| {
        let point = p + d * t;
        point.dist(a).min(point.dist(b)) <= reach * (1. + 1e-9) + f64::EPSILON
    };
    if !(lo.is_finite() && hi.is_finite()) || (lo > 0. && !at_end(lo)) || (hi < 1. && !at_end(hi)) {
        return None;
    }
    let (lo, hi) = (lo.max(0.), hi.min(1.));
    (hi > lo).then_some((lo, hi))
}

/// Where `x0 + t·dx` lies in `lo..hi`.
fn slab(x0: f64, dx: f64, lo: f64, hi: f64) -> Option<(f64, f64)> {
    if dx == 0. {
        return (lo..=hi).contains(&x0).then_some((f64::NEG_INFINITY, f64::INFINITY));
    }
    let (s, t) = ((lo - x0) / dx, (hi - x0) / dx);
    Some((s.min(t), s.max(t)))
}

fn meet(a: (f64, f64), b: (f64, f64)) -> Option<(f64, f64)> {
    let (lo, hi) = (a.0.max(b.0), a.1.min(b.1));
    (hi >= lo).then_some((lo, hi))
}

/// Where `p + t·d` lies within `r` of `c`.
fn disk(p: Vec2, d: Vec2, c: Vec2, r: f64) -> Option<(f64, f64)> {
    let f = p - c;
    let (a, b, k) = (d.dot(d), 2. * f.dot(d), f.dot(f) - r * r);
    let disc = b * b - 4. * a * k;
    if disc < 0. {
        return None;
    }
    let root = disc.sqrt();
    Some(((-b - root) / (2. * a), (-b + root) / (2. * a)))
}

/// The body lines by the grid cells their reach touches. A line is filed only in the cells along
/// it, so a long diagonal does not fill its whole bounding box.
struct Grid {
    origin: Vec2,
    cell: f64,
    columns: usize,
    rows: usize,
    cells: Vec<Vec<usize>>,
    reach: f64,
}

impl Grid {
    fn new(lines: &[Line], reach: f64) -> Self {
        let (mut min, mut max) = (Vec2::new(f64::INFINITY, f64::INFINITY), Vec2::new(f64::NEG_INFINITY, f64::NEG_INFINITY));
        for l in lines {
            for p in [l.a, l.b] {
                min = Vec2::new(min.x.min(p.x), min.y.min(p.y));
                max = Vec2::new(max.x.max(p.x), max.y.max(p.y));
            }
        }
        let origin = Vec2::new(min.x - reach, min.y - reach);
        let side = (max.x - min.x).max(max.y - min.y) + 2. * reach;
        let cell = (side / GRID_CELLS).max(reach).max(f64::MIN_POSITIVE);
        let count = |extent: f64| ((extent + 2. * reach) / cell).floor() as usize + 1;
        let (columns, rows) = (count(max.x - min.x), count(max.y - min.y));
        let mut grid = Grid { origin, cell, columns, rows, cells: vec![Vec::new(); columns * rows], reach };
        for (i, l) in lines.iter().enumerate() {
            for cell in grid.along(l.a, l.b) {
                grid.cells[cell].push(i);
            }
        }
        grid
    }

    /// The cells within reach of `p`..`q`, each once: the segment is walked in pieces no longer
    /// than a cell (fewer for a segment far longer than the grid), each piece taking the cells of
    /// its own box widened by the reach.
    fn along(&self, p: Vec2, q: Vec2) -> Vec<usize> {
        let most = (self.columns + self.rows) * 2;
        let pieces = ((p.dist(q) / self.cell).ceil() as usize).clamp(1, most.max(1));
        let mut cells = Vec::new();
        for k in 0..pieces {
            let (s, t) = (k as f64 / pieces as f64, (k + 1) as f64 / pieces as f64);
            let (c0, r0, c1, r1) = self.span(p + (q - p) * s, p + (q - p) * t);
            for r in r0..=r1 {
                cells.extend((c0..=c1).map(|c| r * self.columns + c));
            }
        }
        cells.sort_unstable();
        cells.dedup();
        cells
    }

    /// The cells of the box around `p`..`q`, widened by the reach and clamped to the grid.
    fn span(&self, p: Vec2, q: Vec2) -> (usize, usize, usize, usize) {
        let index = |x: f64, n: usize| (((x - self.reach) / self.cell).floor().max(0.) as usize).min(n - 1);
        let upper = |x: f64, n: usize| (((x + self.reach) / self.cell).floor().max(0.) as usize).min(n - 1);
        let (a, b) = (p - self.origin, q - self.origin);
        (index(a.x.min(b.x), self.columns), index(a.y.min(b.y), self.rows), upper(a.x.max(b.x), self.columns), upper(a.y.max(b.y), self.rows))
    }

    /// The body lines in the cells along `p`..`q`; a line can come more than once.
    fn near(&self, p: Vec2, q: Vec2) -> impl Iterator<Item = usize> + '_ {
        self.along(p, q).into_iter().flat_map(move |cell| self.cells[cell].iter().copied())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn line(a: (f64, f64), b: (f64, f64), hidden: bool) -> Line {
        Line { a: Vec2::new(a.0, a.1), b: Vec2::new(b.0, b.1), hidden, owner: None, covered: false, depth: 0. }
    }

    fn path(points: &[(f64, f64)], hidden: bool) -> Path {
        Path { points: points.iter().map(|p| Vec2::new(p.0, p.1)).collect(), hidden }
    }

    fn length(paths: &[Path]) -> f64 {
        paths.iter().map(|p| p.points.windows(2).map(|w| w[0].dist(w[1])).sum::<f64>()).sum()
    }

    /// The sketch square under an extrusion's outline goes; the sketch line past it stays (#19507).
    #[test]
    fn a_sketch_outline_under_a_body_outline_is_drawn_once() {
        let square = [
            line((0., 0.), (10., 0.), false),
            line((10., 0.), (10., 10.), false),
            line((10., 10.), (0., 10.), false),
            line((0., 10.), (0., 0.), false),
        ];
        let sketch = vec![path(&[(0., 0.), (10., 0.001), (10., 10.), (0., 10.), (0., 0.)], false), path(&[(10., 5.), (20., 5.)], false)];
        let kept = sketch_off_lines(sketch, &square, 0.02);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!((length(&kept) - 10.).abs() < 0.05, "{kept:?}");
    }

    /// Only the stretch a body line draws goes: a sketch line half on an edge keeps the other half.
    #[test]
    fn a_partly_drawn_sketch_line_keeps_the_rest() {
        let kept = sketch_off_lines(vec![path(&[(0., 0.), (20., 0.)], false)], &[line((5., 0.), (10., 0.), false)], 0.02);
        assert_eq!(kept.len(), 2, "{kept:?}");
        assert!((length(&kept) - 15.).abs() < 0.05, "{kept:?}");
    }

    /// A crossing edge, a far parallel edge or a hidden edge under a visible sketch line leave it whole.
    #[test]
    fn crossing_far_and_hidden_lines_keep_a_sketch_line() {
        let lines = [line((5., -5.), (5., 5.), false), line((0., 1.), (20., 1.), false), line((0., 0.), (20., 0.), true)];
        let kept = sketch_off_lines(vec![path(&[(0., 0.), (20., 0.)], false)], &lines, 0.02);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!((length(&kept) - 20.).abs() < 1e-9, "{kept:?}");
        // A hidden sketch stretch goes under a hidden body line, as both would be drawn alike.
        assert!(sketch_off_lines(vec![path(&[(0., 0.), (20., 0.)], true)], &lines[2..], 0.02).is_empty());
    }

    /// A sketch line that crosses a body line at a shallow angle keeps its whole length: they come
    /// within reach of each other only near the crossing, away from both lines' ends.
    #[test]
    fn a_shallow_crossing_keeps_the_whole_sketch_line() {
        let kept = sketch_off_lines(vec![path(&[(0., 0.), (20., 0.)], false)], &[line((0., -1.), (20., 1.), false)], 0.02);
        assert_eq!(kept.len(), 1, "{kept:?}");
        assert!((length(&kept) - 20.).abs() < 1e-9, "{kept:?}");
    }

    /// A long diagonal body line is filed only in the cells along it, and still draws the sketch
    /// line under it.
    #[test]
    fn a_long_diagonal_is_filed_along_its_length() {
        let lines = [line((0., 0.), (1000., 1000.), false), line((0., 1000.), (1., 1000.), false)];
        let grid = Grid::new(&lines, 0.02);
        let filed = grid.cells.iter().filter(|c| c.contains(&0)).count();
        assert!(filed < 4 * grid.columns, "{filed} of {} cells", grid.cells.len());
        let kept = sketch_off_lines(vec![path(&[(500., 500.), (600., 600.)], false)], &lines, 0.02);
        assert!(kept.is_empty(), "{kept:?}");
    }

    /// An arc sampled apart from the body's arc still goes: chords a tolerance apart lean a little.
    #[test]
    fn a_sketch_arc_under_the_bodys_arc_goes() {
        let arc =
            |n: usize, r: f64| (0..=n).map(|i| (i as f64 / n as f64) * std::f64::consts::PI).map(|t| (r * t.cos(), r * t.sin())).collect::<Vec<_>>();
        let body = arc(24, 5.);
        let lines: Vec<Line> = body.windows(2).map(|w| line(w[0], w[1], false)).collect();
        let kept = sketch_off_lines(vec![path(&arc(37, 5.), false)], &lines, 0.02);
        assert!(length(&kept) < 1e-6, "{kept:?}");
    }
}
