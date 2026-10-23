//! A trimmed face meshed as a grid in its own (u, v).

use std::collections::BTreeMap;

use crate::grid::{grid_lines, Dir};
use crate::surface::{basis, find_span, lerp, norm, norm2, sub, NurbsSurface};
use crate::MeshError;

/// A boundary point: where it is on the surface and where it is in space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct LoopPoint {
    /// Surface parameter u.
    pub u: f64,
    /// Surface parameter v.
    pub v: f64,
    /// The point in space: the edge's own point, which need not be exactly S(u, v).
    pub p: [f64; 3],
}

/// Options for [`mesh_trimmed`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct MeshOptions {
    /// Largest chord sag of a grid line from its iso-curve, in model units.
    pub tolerance: f64,
    /// Most grid lines in one knot span. A span that asks for more is capped and counted in
    /// [`TrimmedMesh::capped_spans`].
    pub max_lines_per_span: usize,
    /// Fewest cells across the face each way, so its boundary always crosses some.
    pub min_cells: usize,
    /// How close, as a fraction of the tolerance, the measured cell points (edge and diagonal
    /// midpoints) must be before the grid is accepted. `0.75` covers the worst case inside a
    /// triangle of a locally quadratic surface, `4/3` of its worst edge midpoint; `1.0` trusts the
    /// measured points and makes about half as many triangles on ordinary surfaces.
    pub measured_fraction: f64,
}

impl MeshOptions {
    /// `tolerance` with 64 lines per span at most, 8 cells across at least, and measured points
    /// within `0.75` of the tolerance.
    pub fn new(tolerance: f64) -> Self {
        Self {
            tolerance,
            max_lines_per_span: 64,
            min_cells: 8,
            measured_fraction: 0.75,
        }
    }
}

/// The mesh of one face.
#[derive(Debug, Clone, PartialEq)]
pub struct TrimmedMesh {
    /// Vertex positions. Boundary vertices are the loops' own points or lie on their segments.
    pub positions: Vec<[f64; 3]>,
    /// Each vertex's (u, v).
    pub uvs: Vec<[f64; 2]>,
    /// Triangles, anticlockwise about the surface normal (clockwise when `reversed`).
    pub triangles: Vec<[u32; 3]>,
    /// Grid lines used in u and v.
    pub grid: [usize; 2],
    /// Knot spans where the line count was capped (see [`MeshOptions::max_lines_per_span`]).
    pub capped_spans: usize,
    /// Lines added because a measured cell missed the tolerance where the estimate said it would
    /// not: zero for most polynomial surfaces, more for rational ones and twisted cells.
    pub refined_lines: usize,
    /// Zero-area triangles left out: those at a collapsed edge (a pole) and fans over a straight
    /// run of boundary.
    pub dropped_degenerate: usize,
}

/// A loop point split at grid lines: `border` when it lies on a grid line.
#[derive(Debug, Clone, Copy)]
struct Bp {
    u: f64,
    v: f64,
    id: u32,
    border: bool,
}

/// The run of a loop through one cell, from a border point to the next.
struct Chain {
    pts: Vec<Bp>,
    used: bool,
}

const NONE: u32 = u32::MAX;

/// Vertices: grid points made on first use, boundary points as they are met.
struct Verts {
    pos: Vec<[f64; 3]>,
    uv: Vec<[f64; 2]>,
    grid_id: Vec<u32>,
    n_v: usize,
}

impl Verts {
    fn add(&mut self, p: [f64; 3], uv: [f64; 2]) -> Result<u32, MeshError> {
        let id = u32::try_from(self.pos.len()).map_err(|_| MeshError::TooLarge)?;
        if id == NONE {
            return Err(MeshError::TooLarge);
        }
        self.pos.push(p);
        self.uv.push(uv);
        Ok(id)
    }

    /// Grid point (i, j); its position is filled in by `evaluate_grid_points`.
    fn grid(
        &mut self,
        i: usize,
        j: usize,
        u_lines: &[f64],
        v_lines: &[f64],
    ) -> Result<u32, MeshError> {
        let k = i * self.n_v + j;
        let existing = *self.grid_id.get(k).ok_or(MeshError::Internal)?;
        if existing != NONE {
            return Ok(existing);
        }
        let id = self.add([f64::NAN; 3], [at(u_lines, i)?, at(v_lines, j)?])?;
        *self.grid_id.get_mut(k).ok_or(MeshError::Internal)? = id;
        Ok(id)
    }
}

fn at(xs: &[f64], i: usize) -> Result<f64, MeshError> {
    xs.get(i).copied().ok_or(MeshError::Internal)
}

/// The largest index with `lines[i] <= t`, or 0.
fn line_index(lines: &[f64], t: f64) -> usize {
    lines.partition_point(|&x| x <= t).saturating_sub(1)
}

/// Mesh the part of the surface inside `loops`.
///
/// Each loop is closed (its last point is NOT a copy of its first) and may run either way round:
/// the one with the largest area in (u, v) becomes the outer, anticlockwise, and the rest holes,
/// clockwise, so the face is always on the left. Loops must not cross themselves or each other.
/// `reversed` turns every triangle over, for a face whose normal is against its surface's.
///
/// The face ends exactly at the loops' points, so a neighboring face meshed from the same edge
/// polylines meets it with no crack (T-junctions where the two grids differ).
pub fn mesh_trimmed(
    s: &NurbsSurface,
    loops: &[Vec<LoopPoint>],
    reversed: bool,
    opts: &MeshOptions,
) -> Result<TrimmedMesh, MeshError> {
    if !(opts.tolerance.is_finite() && opts.tolerance > 0.0) {
        return Err(MeshError::BadOptions(
            "tolerance must be finite and positive",
        ));
    }
    let mut loops: Vec<Vec<LoopPoint>> = loops.iter().filter(|l| l.len() >= 3).cloned().collect();
    if loops.is_empty() {
        return Err(MeshError::NoLoops);
    }
    if loops
        .iter()
        .flatten()
        .any(|p| !(p.u.is_finite() && p.v.is_finite()))
    {
        return Err(MeshError::BadOptions("loop point (u, v) not finite"));
    }

    // Grid lines over the loops' extent, from the last line at or before it to the first at or
    // after it, and at least min_cells across each way whatever the curvature asks for.
    let (mut u_min, mut u_max, mut v_min, mut v_max) = (
        f64::INFINITY,
        f64::NEG_INFINITY,
        f64::INFINITY,
        f64::NEG_INFINITY,
    );
    for p in loops.iter().flatten() {
        u_min = u_min.min(p.u);
        u_max = u_max.max(p.u);
        v_min = v_min.min(p.v);
        v_max = v_max.max(p.v);
    }
    let cells = opts.min_cells.max(1) as f64;
    let gu = grid_lines(
        s,
        Dir::U,
        opts.tolerance,
        (u_max - u_min) / cells,
        opts.max_lines_per_span,
    )?;
    let gv = grid_lines(
        s,
        Dir::V,
        opts.tolerance,
        (v_max - v_min) / cells,
        opts.max_lines_per_span,
    )?;
    let mut u_lines = clip(&gu.lines, u_min, u_max);
    let mut v_lines = clip(&gv.lines, v_min, v_max);
    if u_lines.len() < 2 || v_lines.len() < 2 {
        return Err(MeshError::BadOptions("loops outside the surface's domain"));
    }
    // The curvature bound placed the lines; measuring the cells settles them.
    if !(opts.measured_fraction > 0.0 && opts.measured_fraction <= 1.0) {
        return Err(MeshError::BadOptions("measured_fraction must be in (0, 1]"));
    }
    let refined_lines = refine(
        s,
        &mut u_lines,
        &mut v_lines,
        opts.measured_fraction * opts.tolerance,
    )?;
    let eps_u = 1e-9 * (at(&u_lines, u_lines.len() - 1)? - at(&u_lines, 0)?);
    let eps_v = 1e-9 * (at(&v_lines, v_lines.len() - 1)? - at(&v_lines, 0)?);

    // No loop point may sit on a grid line: nudge it into a cell by a hair (its 3D point is the
    // edge's and does not move). A loop that then crosses no line, a hole smaller than a cell,
    // gets a line of its own through its middle, and the nudge runs again.
    let mut settled = false;
    for _ in 0..=loops.len() {
        for p in loops.iter_mut().flatten() {
            p.u = nudge(&u_lines, p.u, eps_u)?;
            p.v = nudge(&v_lines, p.v, eps_v)?;
        }
        let mut added = false;
        for l in &loops {
            let (lu0, lu1) = l
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                    (a.min(p.u), b.max(p.u))
                });
            let (lv0, lv1) = l
                .iter()
                .fold((f64::INFINITY, f64::NEG_INFINITY), |(a, b), p| {
                    (a.min(p.v), b.max(p.v))
                });
            let crosses = |lines: &[f64], lo: f64, hi: f64| lines.iter().any(|&x| lo < x && x < hi);
            if crosses(&u_lines, lu0, lu1) || crosses(&v_lines, lv0, lv1) {
                continue;
            }
            // Through the longer extent, relative to the cell, so the line has room.
            if (lu1 - lu0) / eps_u >= (lv1 - lv0) / eps_v {
                insert_sorted(&mut u_lines, (lu0 + lu1) / 2.0);
            } else {
                insert_sorted(&mut v_lines, (lv0 + lv1) / 2.0);
            }
            added = true;
        }
        if !added {
            settled = true;
            break;
        }
    }
    if !settled {
        return Err(MeshError::LoopInsideCell);
    }
    let (n_u, n_v) = (u_lines.len(), v_lines.len());

    // Face on the left: the outermost anticlockwise, the rest clockwise.
    let areas: Vec<f64> = loops.iter().map(|l| signed_area(l)).collect();
    let outer = areas
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.abs().total_cmp(&b.1.abs()))
        .map_or(0, |(k, _)| k);
    for (k, (l, &a)) in loops.iter_mut().zip(&areas).enumerate() {
        if (k == outer) != (a > 0.0) {
            l.reverse();
        }
    }

    let mut verts = Verts {
        pos: Vec::new(),
        uv: Vec::new(),
        grid_id: vec![NONE; n_u * n_v],
        n_v,
    };

    // Split every loop segment where it crosses a grid line. A crossing's (u, v) is snapped to
    // its line; its 3D point is interpolated along the segment's 3D chord.
    let mut split: Vec<Vec<Bp>> = Vec::with_capacity(loops.len());
    let mut cuts: Vec<(f64, Option<f64>, Option<f64>)> = Vec::new();
    for l in &loops {
        let mut out = Vec::with_capacity(l.len() * 2);
        for (a, b) in l.iter().zip(l.iter().cycle().skip(1)) {
            out.push(Bp {
                u: a.u,
                v: a.v,
                id: verts.add(a.p, [a.u, a.v])?,
                border: false,
            });
            cuts.clear();
            let (ia, ib) = (line_index(&u_lines, a.u), line_index(&u_lines, b.u));
            for i in ia.min(ib) + 1..=ia.max(ib) {
                let x = at(&u_lines, i)?;
                cuts.push(((x - a.u) / (b.u - a.u), Some(x), None));
            }
            let (ja, jb) = (line_index(&v_lines, a.v), line_index(&v_lines, b.v));
            for j in ja.min(jb) + 1..=ja.max(jb) {
                let y = at(&v_lines, j)?;
                cuts.push(((y - a.v) / (b.v - a.v), None, Some(y)));
            }
            cuts.sort_by(|x, y| x.0.total_cmp(&y.0));
            for &(t, on_u, on_v) in &cuts {
                let u = on_u.unwrap_or(a.u + t * (b.u - a.u));
                let v = on_v.unwrap_or(a.v + t * (b.v - a.v));
                // Crossing a grid point exactly makes one point, not two.
                if out.last().is_some_and(|q: &Bp| {
                    q.border && (q.u - u).abs() < eps_u && (q.v - v).abs() < eps_v
                }) {
                    continue;
                }
                out.push(Bp {
                    u,
                    v,
                    id: verts.add(lerp(a.p, b.p, t), [u, v])?,
                    border: true,
                });
            }
        }
        split.push(out);
    }

    // Chains, filed by the cell they run through.
    let mut cut_cells: BTreeMap<usize, Vec<Chain>> = BTreeMap::new();
    for l in &split {
        let start = l
            .iter()
            .position(|p| p.border)
            .ok_or(MeshError::LoopInsideCell)?;
        let mut cur: Vec<Bp> = vec![*l.get(start).ok_or(MeshError::Internal)?];
        for p in l.iter().cycle().skip(start + 1).take(l.len()) {
            cur.push(*p);
            if p.border {
                let (a, b) = match cur.as_slice() {
                    [a, b, ..] => (*a, *b),
                    _ => return Err(MeshError::Internal),
                };
                let i = (n_u - 2).min(line_index(&u_lines, (a.u + b.u) / 2.0));
                let j = (n_v - 2).min(line_index(&v_lines, (a.v + b.v) / 2.0));
                let pts = std::mem::replace(&mut cur, vec![*p]);
                cut_cells
                    .entry(i * n_v + j)
                    .or_default()
                    .push(Chain { pts, used: false });
            }
        }
    }

    let mut tris: Vec<[u32; 3]> = Vec::new();
    let mut tri = |a: u32, b: u32, c: u32| tris.push(if reversed { [a, c, b] } else { [a, b, c] });

    // Whole cells, in or out by crossings of each row's center line. Every split segment lies in
    // one row, so each row tests only its own.
    let mut is_cut = vec![false; n_u * n_v];
    for &key in cut_cells.keys() {
        *is_cut.get_mut(key).ok_or(MeshError::Internal)? = true;
    }
    let mut rows: Vec<Vec<(Bp, Bp)>> = vec![Vec::new(); n_v - 1];
    for l in &split {
        for (a, b) in l.iter().zip(l.iter().cycle().skip(1)) {
            let j = (n_v - 2).min(line_index(&v_lines, (a.v + b.v) / 2.0));
            rows.get_mut(j).ok_or(MeshError::Internal)?.push((*a, *b));
        }
    }
    let mut xs: Vec<f64> = Vec::new();
    for (j, row) in rows.iter().enumerate() {
        let vc = (at(&v_lines, j)? + at(&v_lines, j + 1)?) / 2.0;
        xs.clear();
        xs.extend(
            row.iter()
                .filter(|(a, b)| (a.v <= vc) != (b.v <= vc))
                .map(|(a, b)| a.u + (vc - a.v) / (b.v - a.v) * (b.u - a.u)),
        );
        xs.sort_by(f64::total_cmp);
        let mut crossed = 0;
        for i in 0..n_u - 1 {
            let uc = (at(&u_lines, i)? + at(&u_lines, i + 1)?) / 2.0;
            while xs.get(crossed).is_some_and(|&x| x < uc) {
                crossed += 1;
            }
            if crossed % 2 == 0 || *is_cut.get(i * n_v + j).ok_or(MeshError::Internal)? {
                continue;
            }
            let a = verts.grid(i, j, &u_lines, &v_lines)?;
            let b = verts.grid(i + 1, j, &u_lines, &v_lines)?;
            let c = verts.grid(i + 1, j + 1, &u_lines, &v_lines)?;
            let d = verts.grid(i, j + 1, &u_lines, &v_lines)?;
            tri(a, b, c);
            tri(a, c, d);
        }
    }

    // Cut cells: chains joined anticlockwise round the cell by its corners, then ear-clipped.
    let mut poly: Vec<Bp> = Vec::new();
    for (&key, chains) in &mut cut_cells {
        let (i, j) = (key / n_v, key % n_v);
        let (u0, u1, v0, v1) = (
            at(&u_lines, i)?,
            at(&u_lines, i + 1)?,
            at(&v_lines, j)?,
            at(&v_lines, j + 1)?,
        );
        // Position round the cell boundary, 0 to 4 anticlockwise from (u0, v0); corner k is at k.
        let around = |p: &Bp| {
            let (d_b, d_r, d_t, d_l) = (
                (p.v - v0).abs(),
                (p.u - u1).abs(),
                (p.v - v1).abs(),
                (p.u - u0).abs(),
            );
            let m = d_b.min(d_r).min(d_t).min(d_l);
            if m == d_b {
                (p.u - u0) / (u1 - u0)
            } else if m == d_r {
                1.0 + (p.v - v0) / (v1 - v0)
            } else if m == d_t {
                2.0 + (u1 - p.u) / (u1 - u0)
            } else {
                3.0 + (v1 - p.v) / (v1 - v0)
            }
        };
        let corners = [
            (u0, v0, i, j),
            (u1, v0, i + 1, j),
            (u1, v1, i + 1, j + 1),
            (u0, v1, i, j + 1),
        ];
        for first in 0..chains.len() {
            if chains.get(first).ok_or(MeshError::Internal)?.used {
                continue;
            }
            // The polygon: a chain from its entry to its exit, then the cell's corners met going
            // anticlockwise from that exit to the nearest entry, then that chain, until back round.
            poly.clear();
            let mut cur = first;
            let mut closed = false;
            for _ in 0..=chains.len() {
                let exit = {
                    let c = chains.get_mut(cur).ok_or(MeshError::Internal)?;
                    c.used = true;
                    poly.extend_from_slice(&c.pts);
                    around(c.pts.last().ok_or(MeshError::Internal)?)
                };
                let mut next = None;
                let mut best = f64::INFINITY;
                for (ci, c) in chains.iter().enumerate() {
                    let d =
                        (around(c.pts.first().ok_or(MeshError::Internal)?) - exit).rem_euclid(4.0);
                    if d > 1e-12 && d < best {
                        best = d;
                        next = Some(ci);
                    }
                }
                let Some(next) = next else { break };
                let mut between: Vec<(f64, usize)> = (0..4)
                    .map(|k| ((k as f64 - exit).rem_euclid(4.0), k))
                    .filter(|&(d, _)| d > 1e-12 && d < best - 1e-12)
                    .collect();
                between.sort_by(|a, b| a.0.total_cmp(&b.0));
                for (_, k) in between {
                    let &(cu, cv, ci, cj) = corners.get(k).ok_or(MeshError::Internal)?;
                    poly.push(Bp {
                        u: cu,
                        v: cv,
                        id: verts.grid(ci, cj, &u_lines, &v_lines)?,
                        border: true,
                    });
                }
                if next == first {
                    closed = true;
                    break;
                }
                if chains.get(next).ok_or(MeshError::Internal)?.used {
                    break;
                }
                cur = next;
            }
            if !closed {
                return Err(MeshError::OpenCell { i, j });
            }
            ear_clip(&poly, &mut tri);
        }
    }

    // Grid points are evaluated last, only those used, a row's basis at a time.
    let grid_id = &verts.grid_id;
    let pos = &mut verts.pos;
    eval_rows(
        s,
        &u_lines,
        &v_lines,
        |i, j| grid_id.get(i * n_v + j).is_some_and(|&id| id != NONE),
        |i, j, p| {
            let id = *grid_id.get(i * n_v + j).ok_or(MeshError::Internal)?;
            *pos.get_mut(id as usize).ok_or(MeshError::Internal)? = p;
            Ok(())
        },
    )?;

    // A cell on a collapsed edge (a pole) has two corners at one point, so one of its triangles
    // has no area; so does a fan over a straight run of boundary. Neither covers anything.
    let before = tris.len();
    let positions = &verts.pos;
    tris.retain(|t| {
        let [a, b, c] = t.map(|k| positions.get(k as usize).copied().unwrap_or([f64::NAN; 3]));
        let (e1, e2) = (sub(b, a), sub(c, a));
        let longest = norm2(e1).max(norm2(e2)).max(norm2(sub(c, b)));
        let [x, y, z] = e1;
        let [p, q, r] = e2;
        let area2 = norm2([y * r - z * q, z * p - x * r, x * q - y * p]);
        area2 > 1e-24 * longest * longest
    });
    let dropped_degenerate = before - tris.len();
    Ok(TrimmedMesh {
        positions: verts.pos,
        uvs: verts.uv,
        triangles: tris,
        grid: [n_u, n_v],
        capped_spans: gu.capped_spans + gv.capped_spans,
        refined_lines,
        dropped_degenerate,
    })
}

/// Split grid columns and rows until the midpoint of every cell edge and of every cell diagonal
/// lies within `limit` of the surface point at the same (u, v), and return how many lines that
/// added. This is what makes the tolerance hold for rational surfaces, where the curvature bound
/// is only an estimate, and for the twist a cell's diagonal sees, which the per-direction bound
/// ignores.
fn refine(
    s: &NurbsSurface,
    u_lines: &mut Vec<f64>,
    v_lines: &mut Vec<f64>,
    limit: f64,
) -> Result<usize, MeshError> {
    let mut added = 0;
    for _ in 0..8 {
        let (n_u, n_v) = (u_lines.len(), v_lines.len());
        let u_mid: Vec<f64> = u_lines
            .windows(2)
            .map(|w| w.iter().sum::<f64>() / 2.0)
            .collect();
        let v_mid: Vec<f64> = v_lines
            .windows(2)
            .map(|w| w.iter().sum::<f64>() / 2.0)
            .collect();
        let corners = eval_grid(s, u_lines, v_lines)?;
        let along_u = eval_grid(s, &u_mid, v_lines)?;
        let along_v = eval_grid(s, u_lines, &v_mid)?;
        let centers = eval_grid(s, &u_mid, &v_mid)?;
        let pt = |g: &[[f64; 3]], w: usize, i: usize, j: usize| {
            g.get(i * w + j).copied().ok_or(MeshError::Internal)
        };
        let gap = |a: [f64; 3], b: [f64; 3], m: [f64; 3]| norm(sub(lerp(a, b, 0.5), m));
        // Sag of every u-chord (cell bottom and top edges) and v-chord (left and right edges).
        let mut sag_u = vec![0.0_f64; (n_u - 1) * n_v];
        for (k, e) in sag_u.iter_mut().enumerate() {
            let (i, j) = (k / n_v, k % n_v);
            *e = gap(
                pt(&corners, n_v, i, j)?,
                pt(&corners, n_v, i + 1, j)?,
                pt(&along_u, n_v, i, j)?,
            );
        }
        let mut sag_v = vec![0.0_f64; n_u * (n_v - 1)];
        for (k, e) in sag_v.iter_mut().enumerate() {
            let (i, j) = (k / (n_v - 1), k % (n_v - 1));
            *e = gap(
                pt(&corners, n_v, i, j)?,
                pt(&corners, n_v, i, j + 1)?,
                pt(&along_v, n_v - 1, i, j)?,
            );
        }
        let mut split_u = vec![false; n_u - 1];
        let mut split_v = vec![false; n_v - 1];
        for i in 0..n_u - 1 {
            for j in 0..n_v - 1 {
                let su = pt_val(&sag_u, n_v, i, j)?.max(pt_val(&sag_u, n_v, i, j + 1)?);
                let sv = pt_val(&sag_v, n_v - 1, i, j)?.max(pt_val(&sag_v, n_v - 1, i + 1, j)?);
                // A whole cell's two triangles share the diagonal from corner (i, j) to (i + 1, j + 1);
                // a cut cell's ear-clipped triangles can run along the other one as well.
                let center = pt(&centers, n_v - 1, i, j)?;
                let diag = gap(
                    pt(&corners, n_v, i, j)?,
                    pt(&corners, n_v, i + 1, j + 1)?,
                    center,
                )
                .max(gap(
                    pt(&corners, n_v, i + 1, j)?,
                    pt(&corners, n_v, i, j + 1)?,
                    center,
                ));
                let (mut cut_u, mut cut_v) = (su > limit, sv > limit);
                // A diagonal over the limit with both edges under it: halve the direction that
                // sags more; the next round measures again.
                if diag > limit && !cut_u && !cut_v {
                    if su >= sv {
                        cut_u = true;
                    } else {
                        cut_v = true;
                    }
                }
                if cut_u {
                    *split_u.get_mut(i).ok_or(MeshError::Internal)? = true;
                }
                if cut_v {
                    *split_v.get_mut(j).ok_or(MeshError::Internal)? = true;
                }
            }
        }
        let new_u: Vec<f64> = split_u
            .iter()
            .zip(&u_mid)
            .filter(|(s, _)| **s)
            .map(|(_, &m)| m)
            .collect();
        let new_v: Vec<f64> = split_v
            .iter()
            .zip(&v_mid)
            .filter(|(s, _)| **s)
            .map(|(_, &m)| m)
            .collect();
        if new_u.is_empty() && new_v.is_empty() {
            break;
        }
        added += new_u.len() + new_v.len();
        for m in new_u {
            insert_sorted(u_lines, m);
        }
        for m in new_v {
            insert_sorted(v_lines, m);
        }
        // A cap on the grid, so a surface the estimate cannot settle still finishes.
        if u_lines.len().saturating_mul(v_lines.len()) > 40_000_000 {
            break;
        }
    }
    Ok(added)
}

fn pt_val(g: &[f64], w: usize, i: usize, j: usize) -> Result<f64, MeshError> {
    g.get(i * w + j).copied().ok_or(MeshError::Internal)
}

/// Every surface point on the grid `us` x `vs`, at `i * vs.len() + j`.
fn eval_grid(s: &NurbsSurface, us: &[f64], vs: &[f64]) -> Result<Vec<[f64; 3]>, MeshError> {
    let n_v = vs.len();
    let mut out = vec![[0.0; 3]; us.len() * n_v];
    eval_rows(
        s,
        us,
        vs,
        |_, _| true,
        |i, j, p| {
            *out.get_mut(i * n_v + j).ok_or(MeshError::Internal)? = p;
            Ok(())
        },
    )?;
    Ok(out)
}

/// `lines` from the last at or before `lo` to the first at or after `hi`.
fn clip(lines: &[f64], lo: f64, hi: f64) -> Vec<f64> {
    let a = lines.partition_point(|&x| x <= lo).saturating_sub(1);
    let b = lines
        .partition_point(|&x| x < hi)
        .min(lines.len().saturating_sub(1));
    lines.get(a..=b).map(<[f64]>::to_vec).unwrap_or_default()
}

fn insert_sorted(lines: &mut Vec<f64>, x: f64) {
    let at = lines.partition_point(|&y| y < x);
    lines.insert(at, x);
}

/// `t` moved off any grid line within `eps`, into the cell beside it (inward at the last line).
fn nudge(lines: &[f64], t: f64, eps: f64) -> Result<f64, MeshError> {
    let i = line_index(lines, t);
    let x = at(lines, i)?;
    if (t - x).abs() < eps {
        return Ok(if i + 1 == lines.len() {
            x - 2.0 * eps
        } else {
            x + 2.0 * eps
        });
    }
    if let Some(&y) = lines.get(i + 1) {
        if (y - t).abs() < eps {
            return Ok(y - 2.0 * eps);
        }
    }
    Ok(t)
}

fn signed_area(l: &[LoopPoint]) -> f64 {
    l.iter()
        .zip(l.iter().cycle().skip(1))
        .map(|(p, q)| p.u * q.v - q.u * p.v)
        .sum::<f64>()
        / 2.0
}

fn cross(a: &Bp, b: &Bp, c: &Bp) -> f64 {
    (b.u - a.u) * (c.v - a.v) - (b.v - a.v) * (c.u - a.u)
}

/// Triangulate one simple polygon, anticlockwise in (u, v), by ear clipping.
fn ear_clip(poly: &[Bp], tri: &mut impl FnMut(u32, u32, u32)) {
    let Some(last) = poly.len().checked_sub(1) else {
        return;
    };
    // Drop consecutive duplicates; they make zero-area ears that block the scan.
    let mut ring: Vec<Bp> = poly
        .iter()
        .zip(poly.iter().cycle().skip(last))
        .filter(|(p, q)| p.id != q.id && (p.u != q.u || p.v != q.v))
        .map(|(p, _)| *p)
        .collect();
    let mut guard = 0;
    while ring.len() > 3 && guard < 10_000 {
        guard += 1;
        let m = ring.len();
        let mut ear = None;
        for k in 0..m {
            let (Some(a), Some(b), Some(c)) = (
                ring.get((k + m - 1) % m),
                ring.get(k),
                ring.get((k + 1) % m),
            ) else {
                return;
            };
            if cross(a, b, c) <= 0.0 {
                continue;
            }
            let blocked = ring.iter().enumerate().any(|(n, p)| {
                n != (k + m - 1) % m
                    && n != k
                    && n != (k + 1) % m
                    && cross(a, b, p) > 0.0
                    && cross(b, c, p) > 0.0
                    && cross(c, a, p) > 0.0
            });
            if !blocked {
                ear = Some((k, [a.id, b.id, c.id]));
                break;
            }
        }
        let Some((k, [a, b, c])) = ear else { break };
        tri(a, b, c);
        ring.remove(k);
    }
    // What ear clipping could not settle (collinear runs along the cell border) is fanned.
    if let Some((first, rest)) = ring.split_first() {
        for w in rest.windows(2) {
            if let [b, c] = w {
                tri(first.id, b.id, c.id);
            }
        }
    }
}

/// The surface at the points of the grid `u_lines` x `v_lines` that `want` names, each handed to
/// `put`: for each line in u, the basis once and the sum over u contracted into one row of v
/// poles, then each v a short dot product.
fn eval_rows(
    s: &NurbsSurface,
    u_lines: &[f64],
    v_lines: &[f64],
    want: impl Fn(usize, usize) -> bool,
    mut put: impl FnMut(usize, usize, [f64; 3]) -> Result<(), MeshError>,
) -> Result<(), MeshError> {
    let (p, q) = (s.u_degree, s.v_degree);
    let n_v = v_lines.len();
    let v_basis = v_lines
        .iter()
        .map(|&v| {
            let span = find_span(s.nv, q, v, &s.v_knots)?;
            Some((span - q, basis(span, v, q, &s.v_knots)?.0))
        })
        .collect::<Option<Vec<_>>>()
        .ok_or(MeshError::Internal)?;
    let mut row = vec![[0.0_f64; 4]; s.nv];
    for (i, &u) in u_lines.iter().enumerate() {
        if !(0..n_v).any(|j| want(i, j)) {
            continue;
        }
        let span = find_span(s.nu, p, u, &s.u_knots).ok_or(MeshError::Internal)?;
        let (n, _) = basis(span, u, p, &s.u_knots).ok_or(MeshError::Internal)?;
        row.fill([0.0; 4]);
        for (r, &w) in n.iter().enumerate() {
            let start = (span - p + r) * s.nv;
            let poles = s
                .hpoles
                .get(start..start + s.nv)
                .ok_or(MeshError::Internal)?;
            for (acc, h) in row.iter_mut().zip(poles) {
                for (a, x) in acc.iter_mut().zip(h) {
                    *a += w * x;
                }
            }
        }
        for (j, (first, nv)) in v_basis.iter().enumerate() {
            if !want(i, j) {
                continue;
            }
            let hs = row.get(*first..=first + q).ok_or(MeshError::Internal)?;
            let mut acc = [0.0_f64; 4];
            for (h, &b) in hs.iter().zip(nv) {
                for (a, x) in acc.iter_mut().zip(h) {
                    *a += b * x;
                }
            }
            let [x, y, z, w] = acc;
            put(i, j, [x / w, y / w, z / w])?;
        }
    }
    Ok(())
}
