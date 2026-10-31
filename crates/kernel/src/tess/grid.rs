//! Structured meshing of a face whose boundary runs along its parameter box: a grid of
//! interior lines (spaced by the surface's own curvature), joined to the shared edge nodes on
//! each side by a zipped strip (after Cadium's structured and zipped grids: the larger side
//! fans into the smaller one, so the edge nodes are used exactly as they are).

/// Most grid points of one face.
pub(crate) const MAX_FACE_GRID: usize = 150_000;

/// Keep at most `cap` grid points (`us` × `vs`) by taking evenly spaced lines in both
/// directions, the ends kept.
pub(crate) fn thin(us: &mut Vec<f64>, vs: &mut Vec<f64>, cap: usize) {
    let total = us.len().saturating_mul(vs.len());
    if total <= cap {
        return;
    }
    let f = (total as f64 / cap.max(4) as f64).sqrt();
    // At least the two ends in each direction (never more lines than there are).
    let keep = |len: usize| ((len as f64 / f).floor() as usize).max(2).min(len);
    let (mut nu, mut nv) = (keep(us.len()), keep(vs.len()));
    // Rounding can leave the product a little over: the longer direction gives up the rest.
    while nu.saturating_mul(nv) > cap.max(4) {
        if nu >= nv && nu > 2 {
            nu -= 1;
        } else if nv > 2 {
            nv -= 1;
        } else {
            break;
        }
    }
    let pick = |d: &mut Vec<f64>, n: usize| {
        if d.len() > n && n >= 2 {
            let len = d.len();
            *d = (0..n).filter_map(|i| d.get(i * (len - 1) / (n - 1)).copied()).collect();
        }
    };
    pick(us, nu);
    pick(vs, nv);
}

/// The four sides of a parameter box, in counter-clockwise order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Side {
    VMin,
    UMax,
    VMax,
    UMin,
}

const SIDES: [Side; 4] = [Side::VMin, Side::UMax, Side::VMax, Side::UMin];

impl Side {
    /// Bit in a node's side mask.
    fn bit(self) -> u8 {
        match self {
            Side::VMin => 1,
            Side::UMax => 2,
            Side::VMax => 4,
            Side::UMin => 8,
        }
    }
    /// The parameter that runs along this side.
    fn along(self) -> usize {
        match self {
            Side::VMin | Side::VMax => 0,
            Side::UMax | Side::UMin => 1,
        }
    }
}

/// A face's boundary split into its four sides: for each side (in [`SIDES`] order) the
/// boundary vertex indices along it, ordered by increasing `along` parameter, corners included.
pub(crate) struct Quad {
    sides: [Vec<u32>; 4],
    bbox: [(f64, f64); 2],
}

/// The boundary as four sides on its parameter box, when it is one: every node on the box,
/// exactly four corner nodes, and the chains between consecutive corners each on one side.
pub(crate) fn quad(boundary: &[[f64; 2]]) -> Option<Quad> {
    let n = boundary.len();
    if n < 4 {
        return None;
    }
    let mut bbox = [(f64::MAX, f64::MIN); 2];
    for p in boundary {
        for d in 0..2 {
            bbox[d] = (bbox[d].0.min(p[d]), bbox[d].1.max(p[d]));
        }
    }
    let ext = [bbox[0].1 - bbox[0].0, bbox[1].1 - bbox[1].0];
    if !(ext[0] > 0.0 && ext[1] > 0.0) || !(ext[0].is_finite() && ext[1].is_finite()) {
        return None;
    }
    let eps = [ext[0] * 1e-7, ext[1] * 1e-7];
    let mask = |p: &[f64; 2]| {
        let mut m = 0u8;
        if (p[1] - bbox[1].0).abs() <= eps[1] {
            m |= Side::VMin.bit();
        }
        if (p[0] - bbox[0].1).abs() <= eps[0] {
            m |= Side::UMax.bit();
        }
        if (p[1] - bbox[1].1).abs() <= eps[1] {
            m |= Side::VMax.bit();
        }
        if (p[0] - bbox[0].0).abs() <= eps[0] {
            m |= Side::UMin.bit();
        }
        m
    };
    let masks: Vec<u8> = boundary.iter().map(mask).collect();
    if masks.contains(&0) {
        return None;
    }
    let corners: Vec<usize> = (0..n).filter(|&i| masks.get(i).is_some_and(|m| m.count_ones() == 2)).collect();
    if corners.len() != 4 {
        return None;
    }
    let mut sides: [Vec<u32>; 4] = Default::default();
    let mut seen = 0u8;
    for (k, &c0) in corners.iter().enumerate() {
        let c1 = *corners.get((k + 1) % 4)?;
        let len = (c1 + n - c0) % n;
        let chain: Vec<usize> = (0..=len).map(|i| (c0 + i) % n).collect();
        let common = chain.iter().fold(0xFu8, |m, &i| m & masks.get(i).copied().unwrap_or(0));
        let side = SIDES.into_iter().find(|s| common & s.bit() != 0)?;
        if seen & side.bit() != 0 {
            return None;
        }
        seen |= side.bit();
        let d = side.along();
        let mut chain: Vec<u32> = chain.into_iter().map(|i| i as u32).collect();
        let coord = |i: &u32| boundary.get(*i as usize).map_or(0.0, |p| p[d]);
        if chain.first().map(coord) > chain.last().map(coord) {
            chain.reverse();
        }
        // The edge nodes must advance along the side.
        if chain.windows(2).any(|w| coord(&w[0]) >= coord(&w[1])) && chain.len() > 2 {
            return None;
        }
        let slot = SIDES.iter().position(|s| *s == side)?;
        *sides.get_mut(slot)? = chain;
    }
    Some(Quad { sides, bbox })
}

/// Lines along direction `d` taken from the boundary, when the two sides running along `d`
/// carry the same parameters (as opposite iso-lines of one surface sampled alike do): the grid
/// then meets both sides node for node. Interior values only.
pub(crate) fn aligned_lines(boundary: &[[f64; 2]], q: &Quad, d: usize) -> Option<Vec<f64>> {
    let (a, b) = if d == 0 { (q.sides.first()?, q.sides.get(2)?) } else { (q.sides.get(1)?, q.sides.get(3)?) };
    if a.len() != b.len() || a.len() < 3 {
        return None;
    }
    let (lo, hi) = q.bbox.get(d).copied()?;
    let eps = (hi - lo) * 1e-9;
    let mut lines = Vec::with_capacity(a.len());
    for (i, j) in a.iter().zip(b) {
        let (x, y) = (boundary.get(*i as usize)?[d], boundary.get(*j as usize)?[d]);
        if (x - y).abs() > eps {
            return None;
        }
        if x > lo + eps && x < hi - eps {
            lines.push(x);
        }
    }
    Some(lines)
}

/// A face straight across direction `1 − d` (no interior lines there, as a cylinder wall along
/// its axis): the two sides running along `d` zipped straight to each other, when the other two
/// sides are single segments. Triangles counter-clockwise, or `None`.
pub(crate) fn strip(boundary: &[[f64; 2]], q: &Quad, d: usize) -> Option<Vec<[u32; 3]>> {
    // Sides along u are VMin (low) and VMax (high); along v, UMax (high) and UMin (low).
    let (low, high, cross) = if d == 0 { (0, 2, [1, 3]) } else { (3, 1, [0, 2]) };
    if cross.iter().any(|&k| q.sides.get(k).is_none_or(|c| c.len() != 2)) {
        return None;
    }
    let mut tris = Vec::new();
    zip(boundary, q.sides.get(low)?, q.sides.get(high)?, d, d == 1, &mut tris)?;
    for t in &tris {
        if !(signed_area(boundary, *t)? > 0.0) {
            return None;
        }
    }
    Some(tris)
}

/// The interior lines of a structured face: `us`, `vs` strictly inside the box. Each
/// direction gets at least one line so every side has an inner chain to zip to.
pub(crate) fn interior(lines: Vec<f64>, (a, b): (f64, f64)) -> Vec<f64> {
    let inner: Vec<f64> = lines.into_iter().filter(|x| *x > a && *x < b).collect();
    if inner.is_empty() { vec![0.5 * (a + b)] } else { inner }
}

/// Triangulate a [`Quad`] face: boundary vertices are `0..boundary.len()`; the interior grid
/// (`us` × `vs`, strictly inside the box) follows, row by row. Returns the interior
/// parameters and the triangles (counter-clockwise in (u, v)), or `None` if a strip would
/// fold over.
pub(crate) fn structured(boundary: &[[f64; 2]], q: &Quad, us: &[f64], vs: &[f64]) -> Option<(Vec<[f64; 2]>, Vec<[u32; 3]>)> {
    let (nu, nv) = (us.len(), vs.len());
    if nu == 0 || nv == 0 || nu.checked_mul(nv)? > MAX_FACE_GRID {
        return None;
    }
    let n = u32::try_from(boundary.len()).ok()?;
    let g = |i: usize, j: usize| n + (j * nu + i) as u32;
    let mut uv: Vec<[f64; 2]> = boundary.to_vec();
    for &v in vs {
        for &u in us {
            uv.push([u, v]);
        }
    }
    let mut tris: Vec<[u32; 3]> = Vec::with_capacity(2 * (nu + 1) * (nv + 1) + 2 * boundary.len());
    for j in 0..nv.saturating_sub(1) {
        for i in 0..nu.saturating_sub(1) {
            let (a, b, c, d) = (g(i, j), g(i + 1, j), g(i + 1, j + 1), g(i, j + 1));
            tris.push([a, b, c]);
            tris.push([a, c, d]);
        }
    }
    for (slot, side) in SIDES.iter().enumerate() {
        let outer = q.sides.get(slot)?;
        let inner: Vec<u32> = match side {
            Side::VMin => (0..nu).map(|i| g(i, 0)).collect(),
            Side::VMax => (0..nu).map(|i| g(i, nv - 1)).collect(),
            Side::UMin => (0..nv).map(|j| g(0, j)).collect(),
            Side::UMax => (0..nv).map(|j| g(nu - 1, j)).collect(),
        };
        // The strip lies inside the box from its side: below the top and left of the right
        // side its triangles come out clockwise.
        let flip = matches!(side, Side::VMax | Side::UMin);
        zip(&uv, outer, &inner, side.along(), flip, &mut tris)?;
    }
    // Every triangle counter-clockwise: one that is not has folded over.
    for t in &tris {
        if !(signed_area(&uv, *t)? > 0.0) {
            return None;
        }
    }
    let extra = uv.split_off(boundary.len());
    Some((extra, tris))
}

fn signed_area(uv: &[[f64; 2]], t: [u32; 3]) -> Option<f64> {
    let (a, b, c) = (uv.get(t[0] as usize)?, uv.get(t[1] as usize)?, uv.get(t[2] as usize)?);
    Some((b[0] - a[0]) * (c[1] - a[1]) - (c[0] - a[0]) * (b[1] - a[1]))
}

/// Join a side's outer chain (edge nodes) to the inner chain (grid line), both ordered along
/// direction `d`: advance whichever chain's next node comes first along the side, so nodes at
/// the same parameter on both chains are joined straight across.
fn zip(uv: &[[f64; 2]], outer: &[u32], inner: &[u32], d: usize, flip: bool, tris: &mut Vec<[u32; 3]>) -> Option<()> {
    let mut push = |a: u32, b: u32, c: u32| tris.push(if flip { [a, c, b] } else { [a, b, c] });
    let coord = |chain: &[u32], k: usize| chain.get(k).and_then(|i| uv.get(*i as usize)).map(|p| p[d]);
    let (mut a, mut b) = (0usize, 0usize);
    while a + 1 < outer.len() || b + 1 < inner.len() {
        let advance_outer = if a + 1 >= outer.len() {
            false
        } else if b + 1 >= inner.len() {
            true
        } else {
            coord(outer, a + 1)? <= coord(inner, b + 1)?
        };
        if advance_outer {
            push(*outer.get(a)?, *outer.get(a + 1)?, *inner.get(b)?);
            a += 1;
        } else {
            push(*outer.get(a)?, *inner.get(b + 1)?, *inner.get(b)?);
            b += 1;
        }
    }
    Some(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A unit square boundary with `k` nodes per side, counter-clockwise from (0, 0).
    fn square(k: usize) -> Vec<[f64; 2]> {
        let mut b = Vec::new();
        for i in 0..k {
            b.push([i as f64 / k as f64, 0.0]);
        }
        for i in 0..k {
            b.push([1.0, i as f64 / k as f64]);
        }
        for i in 0..k {
            b.push([1.0 - i as f64 / k as f64, 1.0]);
        }
        for i in 0..k {
            b.push([0.0, 1.0 - i as f64 / k as f64]);
        }
        b
    }

    fn area(uv: &[[f64; 2]], tris: &[[u32; 3]]) -> f64 {
        tris.iter().map(|t| signed_area(uv, *t).unwrap_or(f64::NAN) * 0.5).sum()
    }

    #[test]
    fn grid_and_strips_tile_the_box_exactly() {
        for (k, us, vs) in [(1, vec![0.5], vec![0.5]), (5, vec![0.3, 0.6], vec![0.2, 0.4, 0.8]), (13, vec![0.5], vec![0.1, 0.9])] {
            let b = square(k);
            let Some(q) = quad(&b) else { panic!("a quad") };
            let Some((extra, tris)) = structured(&b, &q, &us, &vs) else { panic!("meshes") };
            let mut uv = b.clone();
            uv.extend(extra);
            assert!((area(&uv, &tris) - 1.0).abs() < 1e-12, "k = {k}");
            assert!(tris.iter().all(|t| signed_area(&uv, *t).is_some_and(|a| a > 0.0)));
            // Every boundary segment is used by exactly one triangle (no node skipped).
            let n = b.len() as u32;
            for i in 0..n {
                let (p, q) = (i, (i + 1) % n);
                let uses = tris.iter().filter(|t| (0..3).any(|e| (t[e] == p && t[(e + 1) % 3] == q) || (t[e] == q && t[(e + 1) % 3] == p))).count();
                assert_eq!(uses, 1, "boundary segment {p}-{q}");
            }
        }
    }

    #[test]
    fn a_pole_side_is_a_two_node_chain() {
        // A triangle-like patch whose top side is a pole (two copies of one point in 3D).
        let b = vec![[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 1.0], [0.0, 1.0]];
        let Some(q) = quad(&b) else { panic!("a quad") };
        let Some((extra, tris)) = structured(&b, &q, &[0.5], &[0.5]) else { panic!("meshes") };
        // One grid node, (0.5, 0.5), fanned to each of the five boundary segments: one triangle
        // each, the pole's single segment among them, every one of positive area, tiling the box.
        assert_eq!(extra, [[0.5, 0.5]]);
        assert_eq!(tris.len(), 5);
        let mut uv = b.clone();
        uv.extend(extra);
        assert!(tris.iter().all(|t| signed_area(&uv, *t).is_some_and(|a| a > 0.0)));
        assert!((area(&uv, &tris) - 1.0).abs() < 1e-12);
    }

    #[test]
    fn straight_faces_are_one_strip() {
        // Five nodes along the bottom, three along the top, single-segment ends.
        let b = vec![[0.0, 0.0], [0.25, 0.0], [0.5, 0.0], [0.75, 0.0], [1.0, 0.0], [1.0, 1.0], [0.5, 1.0], [0.0, 1.0]];
        let Some(q) = quad(&b) else { panic!("a quad") };
        let Some(tris) = strip(&b, &q, 0) else { panic!("a strip") };
        assert_eq!(tris.len(), 6);
        assert!((area(&b, &tris) - 1.0).abs() < 1e-12);
        // Transposed: the long sides run along v.
        let t: Vec<[f64; 2]> = b.iter().map(|p| [p[1], p[0]]).rev().collect();
        let Some(q) = quad(&t) else { panic!("a quad") };
        let Some(tris) = strip(&t, &q, 1) else { panic!("a strip") };
        assert!((area(&t, &tris) - 1.0).abs() < 1e-12);
        assert!(strip(&b, &q, 0).is_none() || strip(&t, &q, 0).is_none());
    }

    #[test]
    fn not_a_quad() {
        // A hole-like loop inside the box, and a five-cornered loop.
        assert!(quad(&[[0.0, 0.0], [1.0, 0.0], [1.0, 1.0]]).is_none());
        let pentagon = [[0.0, 0.0], [1.0, 0.0], [1.0, 0.5], [0.5, 1.0], [0.0, 1.0]];
        assert!(quad(&pentagon).is_none());
    }
}
