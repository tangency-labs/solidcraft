//! A trimmed face as an iso-line T-mesh: full-length grid lines from the size rules plus the
//! face's motorcycle traces (which run only as far as they reach), whose rectangles that lie
//! well inside the face are meshed directly. The band between those rectangles (a staircase)
//! and the face's boundary nodes is triangulated with both kept as constraints and no point
//! added. The boundary stays exactly the shared edge nodes, so the face meets its neighbours
//! exactly, and only the band next to the boundary is irregular (cut-cell meshing).

use solidcraft_geom::Ring;

use super::cdt;
use super::motorcycle::Trace;

/// A cell is kept when the boundary stays at least this fraction of its smaller side away from
/// it (in the metric): closer, the band triangle between them would be a sliver.
const CLEARANCE: f64 = 0.35;
/// Two lines closer than this fraction of a grid cell would leave a strip of slivers between
/// them: a grid line that close to a trace moves onto the trace (the trace runs through its
/// peak, which is the point of it), and a trace that close to an earlier one joins it.
const SNAP: f64 = 0.25;

/// One direction's lines: positions (increasing) and whether each runs the whole face.
struct Lines {
    at: Vec<f64>,
    full: Vec<bool>,
}

/// The full lines `grid` plus the traces' lines in this direction (see [`SNAP`]; the first and
/// last grid lines, the face's parameter box, never move). Returns the lines and, per trace
/// (in order), its line's index.
fn lines(grid: &[f64], traces: &[f64]) -> Option<(Lines, Vec<usize>)> {
    let mut at: Vec<f64> = grid.to_vec();
    let mut full = vec![true; at.len()];
    let mut place = Vec::with_capacity(traces.len());
    for &c in traces {
        // The full-grid cell around c, and every line inside it.
        let k = grid.partition_point(|g| *g < c);
        let (lo, hi) = (*grid.get(k.saturating_sub(1))?, *grid.get(k.min(grid.len().saturating_sub(1)))?);
        let w = (hi - lo).max(f64::MIN_POSITIVE);
        let last = grid.len().saturating_sub(1);
        let near = at.iter().enumerate().filter(|(_, x)| (**x - c).abs() < SNAP * w).min_by(|a, b| (a.1 - c).abs().total_cmp(&(b.1 - c).abs()));
        match near {
            // An earlier trace, or a box edge: join it.
            Some((i, _)) if !full.get(i).copied().unwrap_or(false) || i == 0 || i == last => place.push(i),
            // An inner grid line: it moves onto the trace.
            Some((i, _)) => {
                *at.get_mut(i)? = c;
                place.push(i);
            }
            None => {
                at.push(c);
                full.push(false);
                place.push(at.len() - 1);
            }
        }
    }
    // Sort, carrying the trace indices along.
    let mut order: Vec<usize> = (0..at.len()).collect();
    order.sort_by(|a, b| at.get(*a).copied().unwrap_or(0.0).total_cmp(&at.get(*b).copied().unwrap_or(0.0)));
    let mut rank = vec![0; at.len()];
    for (r, &i) in order.iter().enumerate() {
        *rank.get_mut(i)? = r;
    }
    let lines =
        Lines { at: order.iter().filter_map(|&i| at.get(i).copied()).collect(), full: order.iter().filter_map(|&i| full.get(i).copied()).collect() };
    let place = place.iter().filter_map(|&i| rank.get(i).copied()).collect::<Vec<_>>();
    (place.len() == traces.len()).then_some((lines, place))
}

/// Mesh the region bounded by `loops` (ranges of `boundary`, each closed; even–odd, so inner
/// loops are holes) with the full grid lines `us` × `vs` (increasing, spanning the region's
/// parameter box) and the motorcycle `traces`, measured in the metric `scale`. Boundary
/// vertices keep their indices; grid points follow. Triangles counter-clockwise in (u, v).
/// `None` when no cell is kept or the band cannot be triangulated (the face then goes to
/// Delaunay).
pub(crate) fn cut_grid(
    boundary: &[[f64; 2]],
    loops: &[std::ops::Range<usize>],
    us: &[f64],
    vs: &[f64],
    traces: &[Trace],
    scale: [f64; 2],
) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    if us.len() < 2 || vs.len() < 2 || !scale.iter().all(|s| s.is_finite() && *s > 0.0) {
        return None;
    }
    // Traces on u = c (axis 0) are lines in u; on v = c, lines in v.
    let tu: Vec<&Trace> = traces.iter().filter(|t| t.axis == 0).collect();
    let tv: Vec<&Trace> = traces.iter().filter(|t| t.axis == 1).collect();
    // Room for the traces' lines among the grid's: the full lines are thinned (as the face's
    // grid is) until the cells fit, rather than the face falling through to Delaunay, whose
    // refinement budget a grid this size is past.
    let (mut us, mut vs) = (us.to_vec(), vs.to_vec());
    let cells = |us: &[f64], vs: &[f64]| (us.len() + tu.len()).saturating_sub(1).saturating_mul((vs.len() + tv.len()).saturating_sub(1));
    for _ in 0..64 {
        if cells(&us, &vs) <= super::grid::MAX_FACE_GRID || (us.len() <= 2 && vs.len() <= 2) {
            break;
        }
        let cap = us.len().saturating_mul(vs.len()).saturating_mul(9) / 10;
        super::grid::thin(&mut us, &mut vs, cap);
    }
    let (lu, pu) = lines(&us, &tu.iter().map(|t| t.at).collect::<Vec<_>>())?;
    let (lv, pv) = lines(&vs, &tv.iter().map(|t| t.at).collect::<Vec<_>>())?;
    let (nu, nv) = (lu.at.len(), lv.at.len());
    let (cu, cv) = (nu - 1, nv - 1);
    let cells = cu.checked_mul(cv)?;
    if cells > super::grid::MAX_FACE_GRID {
        return None;
    }
    // Which sides of the fine cells are lines: the vertical side at column i in row j, the
    // horizontal side at row j in column i. A trace covers a side it runs at least half along.
    let covers = |t: &Trace, a: f64, b: f64| (t.to.min(b) - t.from.max(a)) >= 0.5 * (b - a);
    let mut vert = vec![false; nu * cv];
    for i in 0..nu {
        let full = lu.full.get(i).copied().unwrap_or(false);
        for j in 0..cv {
            let (a, b) = (*lv.at.get(j)?, *lv.at.get(j + 1)?);
            *vert.get_mut(j * nu + i)? = full || tu.iter().zip(&pu).any(|(t, &k)| k == i && covers(t, a, b));
        }
    }
    let mut horiz = vec![false; nv * cu];
    for j in 0..nv {
        let full = lv.full.get(j).copied().unwrap_or(false);
        for i in 0..cu {
            let (a, b) = (*lu.at.get(i)?, *lu.at.get(i + 1)?);
            *horiz.get_mut(j * cu + i)? = full || tv.iter().zip(&pv).any(|(t, &k)| k == j && covers(t, a, b));
        }
    }
    // Rectangles: fine cells joined across sides that are no line. A T-mesh's faces are
    // rectangles; anything else (a trace ending in the open after snapping) falls back to
    // every line running the whole face.
    let rects = rectangles(cu, cv, &vert, &horiz).or_else(|| {
        vert.iter_mut().for_each(|s| *s = true);
        horiz.iter_mut().for_each(|s| *s = true);
        rectangles(cu, cv, &vert, &horiz)
    })?;
    // In the metric, so clearances are lengths on the surface.
    let gx: Vec<f64> = lu.at.iter().map(|u| u * scale[0]).collect();
    let gy: Vec<f64> = lv.at.iter().map(|v| v * scale[1]).collect();
    let segs: Vec<([f64; 2], [f64; 2])> = loops
        .iter()
        .flat_map(|r| {
            let n = r.len();
            (0..n).filter_map(move |k| Some((*boundary.get(r.start + k)?, *boundary.get(r.start + (k + 1) % n)?)))
        })
        .map(|(a, b)| ([a[0] * scale[0], a[1] * scale[1]], [b[0] * scale[0], b[1] * scale[1]]))
        .collect();
    let keep = clear_cells(&gx, &gy, &segs, &rects)?;
    if !keep.iter().any(|k| *k) {
        return None;
    }
    let kept = |i: isize, j: isize| -> bool {
        i >= 0
            && j >= 0
            && (i as usize) < cu
            && (j as usize) < cv
            && rects.of.get(j as usize * cu + i as usize).and_then(|r| keep.get(*r)).copied().unwrap_or(false)
    };
    // Grid points on the kept rectangles' corners, numbered after the boundary.
    let mut used = vec![false; nu * nv];
    for (r, b) in rects.boxes.iter().enumerate() {
        if keep.get(r).copied().unwrap_or(false) {
            for (i, j) in [(b[0], b[1]), (b[2], b[1]), (b[2], b[3]), (b[0], b[3])] {
                *used.get_mut(j * nu + i)? = true;
            }
        }
    }
    let n = boundary.len();
    let mut id: Vec<Option<u32>> = vec![None; nu * nv];
    let mut extra: Vec<[f64; 2]> = Vec::new();
    let mut node = |i: usize, j: usize, extra: &mut Vec<[f64; 2]>| -> Option<u32> {
        let slot = id.get_mut(j * nu + i)?;
        if let Some(k) = *slot {
            return Some(k);
        }
        let k = u32::try_from(n + extra.len()).ok()?;
        extra.push([*lu.at.get(i)?, *lv.at.get(j)?]);
        *slot = Some(k);
        Some(k)
    };
    let mut tris: Vec<[u32; 3]> = Vec::new();
    // The staircase: pieces of kept rectangles' sides with no kept rectangle across,
    // counter-clockwise around the kept region (so the band lies to their right).
    let mut stairs: Vec<(u32, u32)> = Vec::new();
    for (r, b) in rects.boxes.iter().enumerate() {
        if !keep.get(r).copied().unwrap_or(false) {
            continue;
        }
        let [i0, j0, i1, j1] = *b;
        // The perimeter counter-clockwise as grid points, each with the fine cell across the
        // side that leaves it.
        let mut ring: Vec<((usize, usize), (isize, isize))> = Vec::new();
        for i in i0..i1 {
            ring.push(((i, j0), (i as isize, j0 as isize - 1)));
        }
        for j in j0..j1 {
            ring.push(((i1, j), (i1 as isize, j as isize)));
        }
        for i in (i0 + 1..=i1).rev() {
            ring.push(((i, j1), (i as isize - 1, j1 as isize)));
        }
        for j in (j0 + 1..=j1).rev() {
            ring.push(((i0, j), (i0 as isize - 1, j as isize - 1)));
        }
        let corners = ring.iter().filter(|((i, j), _)| used.get(j * nu + i).copied().unwrap_or(false)).count();
        let mut poly: Vec<u32> = Vec::with_capacity(corners);
        let mut across: Vec<(isize, isize)> = Vec::with_capacity(corners);
        for &((i, j), c) in &ring {
            if used.get(j * nu + i).copied().unwrap_or(false) {
                poly.push(node(i, j, &mut extra)?);
                across.push(c);
            }
        }
        let poly = Ring::new(poly);
        if poly.len() == 4 {
            let [a, b, c, d] = [*poly.first()?, *poly.get(1)?, *poly.get(2)?, *poly.get(3)?];
            tris.push([a, b, c]);
            tris.push([a, c, d]);
        } else {
            // Hanging nodes from neighbours: a fan from the rectangle's centre.
            let centre = [0.5 * (lu.at.get(i0)? + lu.at.get(i1)?), 0.5 * (lv.at.get(j0)? + lv.at.get(j1)?)];
            let m = u32::try_from(n + extra.len()).ok()?;
            extra.push(centre);
            for (&a, &b) in poly.edges() {
                tris.push([m, a, b]);
            }
        }
        for ((&a, &b), &(ci, cj)) in poly.edges().zip(&across) {
            if !kept(ci, cj) {
                stairs.push((a, b));
            }
        }
    }
    let mut all = boundary.to_vec();
    all.extend_from_slice(&extra);
    let band = cdt::band(&all, n, loops, &stairs, scale)?;
    tris.extend(band);
    // Every triangle counter-clockwise with some area: one that is not has folded.
    for t in &tris {
        let (a, b, c) = (all.get(t[0] as usize)?, all.get(t[1] as usize)?, all.get(t[2] as usize)?);
        let area = (b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]);
        if !(area > 0.0) {
            return None;
        }
    }
    Some((extra, tris))
}

/// The rectangles of a T-mesh over `cu` × `cv` fine cells: per cell its rectangle, and per
/// rectangle its corners as grid indices `[i0, j0, i1, j1]`. `None` if a face is not a
/// rectangle.
struct Rects {
    of: Vec<usize>,
    boxes: Vec<[usize; 4]>,
}

fn rectangles(cu: usize, cv: usize, vert: &[bool], horiz: &[bool]) -> Option<Rects> {
    let nu = cu + 1;
    let mut of = vec![usize::MAX; cu * cv];
    let mut boxes = Vec::new();
    for j0 in 0..cv {
        for i0 in 0..cu {
            if *of.get(j0 * cu + i0)? != usize::MAX {
                continue;
            }
            // Grow right while no line separates, then up while the whole row is open.
            let mut i1 = i0 + 1;
            while i1 < cu && !*vert.get(j0 * nu + i1)? {
                i1 += 1;
            }
            let mut j1 = j0 + 1;
            while j1 < cv && (i0..i1).all(|i| horiz.get(j1 * cu + i).is_some_and(|h| !*h)) {
                j1 += 1;
            }
            let r = boxes.len();
            for j in j0..j1 {
                for i in i0..i1 {
                    let slot = of.get_mut(j * cu + i)?;
                    if *slot != usize::MAX {
                        return None;
                    }
                    *slot = r;
                }
            }
            boxes.push([i0, j0, i1, j1]);
        }
    }
    // A rectangle: no line inside it, and lines all round it.
    for b in &boxes {
        let [i0, j0, i1, j1] = *b;
        for j in j0..j1 {
            for i in i0 + 1..i1 {
                if *vert.get(j * nu + i)? {
                    return None;
                }
            }
        }
        for i in i0..i1 {
            for j in j0 + 1..j1 {
                if *horiz.get(j * cu + i)? {
                    return None;
                }
            }
        }
        for j in j0..j1 {
            if (i0 > 0 && !*vert.get(j * nu + i0)?) || (i1 < cu && !*vert.get(j * nu + i1)?) {
                return None;
            }
        }
        for i in i0..i1 {
            if (j0 > 0 && !*horiz.get(j0 * cu + i)?) || (j1 < cv && !*horiz.get(j1 * cu + i)?) {
                return None;
            }
        }
    }
    Some(Rects { of, boxes })
}

/// Per rectangle: kept when every fine cell in it is inside the region and clear of the
/// boundary (see [`CLEARANCE`]). `gx`, `gy`: the lines in the metric; `segs`: the boundary.
fn clear_cells(gx: &[f64], gy: &[f64], segs: &[([f64; 2], [f64; 2])], rects: &Rects) -> Option<Vec<bool>> {
    let (cu, cv) = (gx.len().saturating_sub(1), gy.len().saturating_sub(1));
    let cells = cu * cv;
    let cell = |i: usize, j: usize| -> Option<([f64; 2], [f64; 2])> {
        let (x0, x1, y0, y1) = (*gx.get(i)?, *gx.get(i + 1)?, *gy.get(j)?, *gy.get(j + 1)?);
        // The clearance of the rectangle the cell belongs to, not of the (possibly thin) cell.
        let b = rects.boxes.get(*rects.of.get(j * cu + i)?)?;
        let (w, h) = (gx.get(b[2])? - gx.get(b[0])?, gy.get(b[3])? - gy.get(b[1])?);
        let pad = CLEARANCE * w.min(h);
        Some(([x0 - pad, y0 - pad], [x1 + pad, y1 + pad]))
    };
    // Cells the boundary comes near: each segment marks the cells whose padded box it touches.
    let mut near = vec![false; cells];
    let maxpad = rects
        .boxes
        .iter()
        .filter_map(|b| Some(CLEARANCE * (gx.get(b[2])? - gx.get(b[0])?).min(gy.get(b[3])? - gy.get(b[1])?)))
        .fold(0.0f64, f64::max);
    let span = |g: &[f64], lo: f64, hi: f64| {
        let a = g.partition_point(|t| *t < lo - maxpad).saturating_sub(1);
        let b = (g.partition_point(|t| *t <= hi + maxpad) + 1).min(g.len().saturating_sub(1));
        (a, b)
    };
    for (a, b) in segs {
        let (i0, i1) = span(gx, a[0].min(b[0]), a[0].max(b[0]));
        let (j0, j1) = span(gy, a[1].min(b[1]), a[1].max(b[1]));
        for j in j0..j1 {
            for i in i0..i1 {
                let Some((lo, hi)) = cell(i, j) else { continue };
                if segment_meets_box(*a, *b, lo, hi)
                    && let Some(c) = near.get_mut(j * cu + i)
                {
                    *c = true;
                }
            }
        }
    }
    // The other cells are wholly inside or wholly outside: even–odd along each row's middle.
    let mut keep = vec![true; rects.boxes.len()];
    for j in 0..cv {
        let (Some(y0), Some(y1)) = (gy.get(j), gy.get(j + 1)) else { return None };
        let ym = 0.5 * (y0 + y1);
        let mut cross: Vec<f64> =
            segs.iter().filter(|(a, b)| (a[1] > ym) != (b[1] > ym)).map(|(a, b)| a[0] + (ym - a[1]) / (b[1] - a[1]) * (b[0] - a[0])).collect();
        cross.sort_by(f64::total_cmp);
        for i in 0..cu {
            let k = j * cu + i;
            let (Some(x0), Some(x1)) = (gx.get(i), gx.get(i + 1)) else { return None };
            let xm = 0.5 * (x0 + x1);
            let inside = cross.partition_point(|c| *c < xm) % 2 == 1;
            if near.get(k).copied().unwrap_or(true) || !inside {
                *keep.get_mut(*rects.of.get(k)?)? = false;
            }
        }
    }
    Some(keep)
}

/// Whether the segment `a`–`b` meets the closed box `lo`–`hi` (Liang–Barsky clipping).
fn segment_meets_box(a: [f64; 2], b: [f64; 2], lo: [f64; 2], hi: [f64; 2]) -> bool {
    let (mut t0, mut t1) = (0.0f64, 1.0f64);
    for d in 0..2 {
        let delta = b[d] - a[d];
        for (p, q) in [(-delta, a[d] - lo[d]), (delta, hi[d] - a[d])] {
            if p == 0.0 {
                if q < 0.0 {
                    return false;
                }
            } else {
                let r = q / p;
                if p < 0.0 {
                    t0 = t0.max(r);
                } else {
                    t1 = t1.min(r);
                }
                if t0 > t1 {
                    return false;
                }
            }
        }
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    fn circle(c: [f64; 2], r: f64, n: usize, ccw: bool) -> Vec<[f64; 2]> {
        (0..n)
            .map(|k| {
                let a = std::f64::consts::TAU * k as f64 / n as f64 * if ccw { 1.0 } else { -1.0 };
                [c[0] + r * a.cos(), c[1] + r * a.sin()]
            })
            .collect()
    }

    /// A single loop over all `n` boundary points.
    fn one(n: usize) -> [std::ops::Range<usize>; 1] {
        [std::ops::Range { start: 0, end: n }]
    }

    fn lines(a: f64, b: f64, n: usize) -> Vec<f64> {
        (0..=n).map(|k| a + (b - a) * k as f64 / n as f64).collect()
    }

    /// Every boundary segment is the side of exactly one triangle, every inner side of two,
    /// and the triangles cover the region's area.
    fn check(boundary: &[[f64; 2]], loops: &[std::ops::Range<usize>], area: f64, out: &(Vec<[f64; 2]>, Vec<[u32; 3]>)) {
        let (extra, tris) = out;
        let mut all = boundary.to_vec();
        all.extend_from_slice(extra);
        let mut sides: std::collections::HashMap<(u32, u32), i32> = std::collections::HashMap::new();
        let mut sum = 0.0;
        for t in tris {
            let [a, b, c] = t.map(|i| all[i as usize]);
            let ar = 0.5 * ((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]));
            assert!(ar > 0.0, "counter-clockwise");
            sum += ar;
            for e in 0..3 {
                *sides.entry((t[e], t[(e + 1) % 3])).or_default() += 1;
            }
        }
        for r in loops {
            let n = r.len();
            for k in 0..n {
                let (a, b) = ((r.start + k) as u32, (r.start + (k + 1) % n) as u32);
                assert_eq!(sides.get(&(a, b)).copied().unwrap_or(0), 1, "boundary side {a}-{b} used once, forwards");
                sides.remove(&(a, b));
            }
        }
        for (&(a, b), &k) in &sides {
            assert_eq!(k, 1);
            assert_eq!(sides.get(&(b, a)).copied(), Some(1), "inner side {a}-{b} shared");
        }
        assert!((sum - area).abs() < 1e-9 * area.max(1.0), "area {sum} vs {area}");
    }

    #[test]
    fn a_disc_is_cells_and_a_band() {
        let b = circle([0.0, 0.0], 1.0, 64, true);
        let loops = one(64);
        let poly_area = 0.5 * 64.0 * (std::f64::consts::TAU / 64.0).sin();
        let Some(out) = cut_grid(&b, &loops, &lines(-1.0, 1.0, 10), &lines(-1.0, 1.0, 10), &[], [1.0, 1.0]) else { panic!("meshes") };
        check(&b, &loops, poly_area, &out);
        // Most of the disc is grid cells (two triangles each).
        assert!(out.1.len() > 100, "{}", out.1.len());
    }

    #[test]
    fn a_square_with_a_hole() {
        let mut b = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        b.extend(circle([2.0, 2.0], 1.0, 32, false));
        let loops = [0..4, 4..36];
        let hole = 0.5 * 32.0 * (std::f64::consts::TAU / 32.0).sin();
        let Some(out) = cut_grid(&b, &loops, &lines(0.0, 4.0, 16), &lines(0.0, 4.0, 16), &[], [1.0, 1.0]) else { panic!("meshes") };
        check(&b, &loops, 16.0 - hole, &out);
    }

    #[test]
    fn a_curved_triangle_in_a_stretched_metric() {
        // A three-cornered patch (as a vertex blend in its parameters), u twice as long as v.
        let mut b: Vec<[f64; 2]> = (0..=8).map(|k| [k as f64 / 8.0, 0.0]).collect();
        b.extend((1..8).map(|k| {
            let a = std::f64::consts::FRAC_PI_2 * k as f64 / 8.0;
            [a.cos(), a.sin()]
        }));
        b.extend((1..8).rev().map(|k| [0.0, k as f64 / 8.0]));
        let n = b.len();
        let loops = one(n);
        let area: f64 = (0..n)
            .map(|k| {
                let (p, q) = (b[k], b[(k + 1) % n]);
                0.5 * (p[0] * q[1] - q[0] * p[1])
            })
            .sum();
        let Some(out) = cut_grid(&b, &loops, &lines(0.0, 1.0, 12), &lines(0.0, 1.0, 6), &[], [2.0, 1.0]) else { panic!("meshes") };
        check(&b, &loops, area, &out);
    }

    /// The hole's motorcycle traces (lines through its four extremes) become T-mesh lines that
    /// stop where the traces stop; the mesh stays exact on the boundary and covers the region.
    #[test]
    fn a_square_with_a_hole_and_its_traces() {
        let mut b = vec![[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]];
        b.extend(circle([1.7, 2.3], 1.0, 32, false));
        let loops = [0..4, 4..36];
        let hole = 0.5 * 32.0 * (std::f64::consts::TAU / 32.0).sin();
        let traces = super::super::motorcycle::traces(&b, &loops, [1.0, 1.0], 0.0);
        assert_eq!(traces.len(), 4, "{traces:?}");
        let Some(out) = cut_grid(&b, &loops, &lines(0.0, 4.0, 16), &lines(0.0, 4.0, 16), &traces, [1.0, 1.0]) else { panic!("meshes") };
        check(&b, &loops, 16.0 - hole, &out);
        // The traces run through the hole's extremes: along v = 2.3 from its u-extremes and
        // along u = 1.7 from its v-extremes; their lines are in the grid.
        for (d, x) in [(0, 1.7), (1, 2.3)] {
            assert!(out.0.iter().any(|p| (p[d] - x).abs() < 1e-9), "a grid point on line {d} = {x}");
        }
    }

    #[test]
    fn nothing_kept_is_refused() {
        let b = circle([0.0, 0.0], 0.1, 16, true);
        assert!(cut_grid(&b, &one(16), &lines(-1.0, 1.0, 1), &lines(-1.0, 1.0, 1), &[], [1.0, 1.0]).is_none());
    }

    #[test]
    fn segments_meet_boxes() {
        assert!(segment_meets_box([-1.0, 0.5], [2.0, 0.5], [0.0, 0.0], [1.0, 1.0]));
        assert!(!segment_meets_box([-1.0, 2.0], [2.0, 2.0], [0.0, 0.0], [1.0, 1.0]));
        assert!(segment_meets_box([0.5, 0.5], [0.6, 0.6], [0.0, 0.0], [1.0, 1.0]));
        assert!(!segment_meets_box([2.0, 0.0], [3.0, 1.0], [0.0, 0.0], [1.0, 1.0]));
    }
}
