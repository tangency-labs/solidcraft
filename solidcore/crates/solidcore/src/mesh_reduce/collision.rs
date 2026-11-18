//! Conservative intersection checks and a refittable index of stable face IDs.

use super::{
    check_cancel, check_stride, difference, grow, product, subtract_expansion, MeshReductionError,
};

type Triangle = [[f64; 3]; 3];

#[derive(Clone, Copy, PartialEq)]
pub(super) struct Bounds {
    min: [f64; 3],
    max: [f64; 3],
}

impl Bounds {
    const EMPTY: Self = Self {
        min: [f64::INFINITY; 3],
        max: [f64::NEG_INFINITY; 3],
    };

    fn is_empty(self) -> bool {
        self.min[0] > self.max[0]
    }

    pub(super) fn triangle(t: Triangle) -> Self {
        if !t.iter().flatten().all(|x| x.is_finite()) {
            return Self {
                min: [f64::NEG_INFINITY; 3],
                max: [f64::INFINITY; 3],
            };
        }
        let mut result = Self {
            min: t[0],
            max: t[0],
        };
        for p in &t[1..] {
            for axis in 0..3 {
                result.min[axis] = result.min[axis].min(p[axis]);
                result.max[axis] = result.max[axis].max(p[axis]);
            }
        }
        result
    }

    pub(super) fn union(self, other: Self) -> Self {
        if self.is_empty() {
            return other;
        }
        if other.is_empty() {
            return self;
        }
        let mut result = self;
        for axis in 0..3 {
            result.min[axis] = result.min[axis].min(other.min[axis]);
            result.max[axis] = result.max[axis].max(other.max[axis]);
        }
        result
    }

    pub(super) fn overlaps(self, other: Self, padding: f64) -> bool {
        !padding.is_finite()
            || padding < 0.0
            || (0..3).all(|i| {
                self.min[i] - padding <= other.max[i] && other.min[i] - padding <= self.max[i]
            })
    }
}

/// Complete binary tree with spatially arranged, stable-ID leaf slots.
pub(super) struct Bvh {
    base: usize,
    leaves: Vec<usize>,
    ids: Vec<usize>,
    nodes: Vec<Bounds>,
}

impl Bvh {
    #[cfg(test)]
    pub(super) fn new(len: usize, triangle: impl FnMut(usize) -> Option<Triangle>) -> Self {
        Self::new_with_cancel(len, triangle, &mut || false).unwrap()
    }

    pub(super) fn new_with_cancel(
        len: usize,
        mut triangle: impl FnMut(usize) -> Option<Triangle>,
        cancel: &mut impl FnMut() -> bool,
    ) -> Result<Self, MeshReductionError> {
        check_cancel(cancel)?;
        let base = len.max(1).next_power_of_two();
        let mut nodes = vec![Bounds::EMPTY; 2 * base];
        for id in 0..len {
            check_stride(id, cancel)?;
            nodes[base + id] = triangle(id).map_or(Bounds::EMPTY, Bounds::triangle);
        }
        let mut slots: Vec<usize> = (0..base).collect();
        fn arrange(
            slots: &mut [usize],
            bounds: &[Bounds],
            cancel: &mut impl FnMut() -> bool,
        ) -> Result<(), MeshReductionError> {
            if slots.len() >= super::CANCEL_STRIDE {
                check_cancel(cancel)?;
            }
            if slots.len() <= 1 {
                return Ok(());
            }
            let mut aggregate = Bounds::EMPTY;
            for (i, &id) in slots.iter().enumerate() {
                if slots.len() >= super::CANCEL_STRIDE {
                    check_stride(i, cancel)?;
                }
                aggregate = aggregate.union(bounds[id]);
            }
            let axis = if aggregate.is_empty() {
                0
            } else {
                (0..3)
                    .max_by(|&a, &c| {
                        (aggregate.max[a] - aggregate.min[a])
                            .total_cmp(&(aggregate.max[c] - aggregate.min[c]))
                    })
                    .unwrap_or(0)
            };
            slots.select_nth_unstable_by(slots.len() / 2, |&a, &b| {
                let center = |id: usize| {
                    let b = bounds[id];
                    if b.is_empty() {
                        None
                    } else {
                        Some(b.min[axis] * 0.5 + b.max[axis] * 0.5)
                    }
                };
                match (center(a), center(b)) {
                    (Some(a), Some(b)) => a.total_cmp(&b),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                }
                .then_with(|| a.cmp(&b))
            });
            if slots.len() >= super::CANCEL_STRIDE {
                check_cancel(cancel)?;
            }
            let mid = slots.len() / 2;
            let (left, right) = slots.split_at_mut(mid);
            arrange(left, bounds, cancel)?;
            arrange(right, bounds, cancel)?;
            Ok(())
        }
        arrange(&mut slots, &nodes[base..], cancel)?;

        // Permute the leaf bounds in place. During this pass leaves maps each
        // original slot to its destination; afterwards it maps IDs to nodes.
        let mut leaves = vec![0; base];
        for (slot, &id) in slots.iter().enumerate() {
            check_stride(slot, cancel)?;
            leaves[id] = slot;
        }
        let mut swaps = 0usize;
        for i in 0..base {
            check_stride(i, cancel)?;
            while leaves[i] != i {
                if base >= super::CANCEL_STRIDE {
                    check_stride(swaps, cancel)?;
                }
                swaps = swaps.wrapping_add(1);
                let dest = leaves[i];
                nodes.swap(base + i, base + dest);
                leaves.swap(i, dest);
            }
        }
        leaves.truncate(len);
        for (slot, &id) in slots.iter().enumerate() {
            check_stride(slot, cancel)?;
            if id < len {
                leaves[id] = base + slot;
            }
        }
        for node in (1..base).rev() {
            check_stride(node, cancel)?;
            nodes[node] = nodes[2 * node].union(nodes[2 * node + 1]);
        }
        check_cancel(cancel)?;
        Ok(Self {
            base,
            leaves,
            ids: slots,
            nodes,
        })
    }

    pub(super) fn update(&mut self, id: usize, triangle: Option<Triangle>) {
        let mut node = self.leaves[id];
        let bounds = triangle.map_or(Bounds::EMPTY, Bounds::triangle);
        if self.nodes[node] == bounds {
            return;
        }
        self.nodes[node] = bounds;
        while node > 1 {
            node /= 2;
            let bounds = self.nodes[2 * node].union(self.nodes[2 * node + 1]);
            if self.nodes[node] == bounds {
                break;
            }
            self.nodes[node] = bounds;
        }
    }

    pub(super) fn face_bounds(&self, id: usize) -> Option<Bounds> {
        let bounds = self.nodes[self.leaves[id]];
        if bounds.is_empty() {
            None
        } else {
            Some(bounds)
        }
    }

    /// Visit overlapping stable face IDs until the predicate reports a hit.
    pub(super) fn any_overlap(
        &self,
        triangle: Triangle,
        padding: f64,
        predicate: impl FnMut(usize) -> bool,
    ) -> bool {
        self.any_overlapping_bounds(Bounds::triangle(triangle), padding, predicate)
    }

    /// Visit faces overlapping precomputed bounds, conservatively for invalid padding.
    pub(super) fn any_overlapping_bounds(
        &self,
        bounds: Bounds,
        padding: f64,
        mut predicate: impl FnMut(usize) -> bool,
    ) -> bool {
        let mut node = 1;
        loop {
            let b = self.nodes[node];
            if !b.is_empty() && bounds.overlaps(b, padding) {
                if node < self.base {
                    node *= 2;
                    continue;
                }
                let id = self.ids[node - self.base];
                if id < self.leaves.len() && predicate(id) {
                    return true;
                }
            }
            while node > 1 && node % 2 == 1 {
                node /= 2;
            }
            if node == 1 {
                return false;
            }
            node += 1;
        }
    }

    #[cfg(test)]
    pub(super) fn query(&self, triangle: Triangle, padding: f64) -> Vec<usize> {
        let bounds = Bounds::triangle(triangle);
        let valid = padding.is_finite()
            && padding >= 0.0
            && triangle.iter().flatten().all(|x| x.is_finite());
        let mut result = Vec::new();
        let mut stack = vec![1];
        while let Some(node) = stack.pop() {
            let b = self.nodes[node];
            if b.is_empty() || (valid && !bounds.overlaps(b, padding)) {
                continue;
            }
            if node >= self.base {
                let id = self.ids[node - self.base];
                if id < self.leaves.len() {
                    result.push(id);
                }
            } else {
                stack.push(2 * node + 1);
                stack.push(2 * node);
            }
        }
        result
    }
}

// Exact expansion signs; the parent helpers return None on overflow or when
// a product residual could underflow. Never turn such uncertainty into a miss.
fn sign(expansion: &[f64]) -> i8 {
    match expansion.last() {
        Some(x) if *x > 0.0 => 1,
        Some(x) if *x < 0.0 => -1,
        _ => 0,
    }
}
// Restrict the filter to a range where even the low components of exact
// differences and their triple products cannot underflow or overflow. Outside
// it, retain the expansion arithmetic's conservative uncertainty handling.
fn filter_range(points: &[[f64; 3]]) -> bool {
    points
        .iter()
        .flatten()
        .all(|x| *x == 0.0 || (x.abs() >= 1.0e-40 && x.abs() <= 1.0e40))
}

fn filtered_sign(det: f64, permanent: f64, error: f64) -> Option<i8> {
    // Callers only use this filter in the range where products cannot
    // underflow. A zero permanent therefore proves every determinant term
    // contains an exactly zero coordinate difference, not a cancellation.
    if permanent == 0.0 {
        Some(0)
    } else if det.abs() > permanent * error {
        Some(if det > 0.0 { 1 } else { -1 })
    } else {
        None
    }
}

fn orient2(a: [f64; 3], b: [f64; 3], c: [f64; 3], axis: usize) -> Option<i8> {
    let (x, y) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    if a == b || b == c || c == a {
        if a.iter().chain(&b).chain(&c).all(|x| x.is_finite()) {
            return Some(0);
        }
    }
    if filter_range(&[a, b, c]) {
        let bx = b[x] - a[x];
        let by = b[y] - a[y];
        let cx = c[x] - a[x];
        let cy = c[y] - a[y];
        let p = bx * cy;
        let q = by * cx;
        if let Some(s) = filtered_sign(p - q, p.abs() + q.abs(), 1.0e-14) {
            return Some(s);
        }
    }
    orient2_exact(a, b, c, axis)
}

fn orient2_exact(a: [f64; 3], b: [f64; 3], c: [f64; 3], axis: usize) -> Option<i8> {
    let (x, y) = match axis {
        0 => (1, 2),
        1 => (2, 0),
        _ => (0, 1),
    };
    let bx = difference(b[x], a[x])?;
    let by = difference(b[y], a[y])?;
    let cx = difference(c[x], a[x])?;
    let cy = difference(c[y], a[y])?;
    Some(sign(&subtract_expansion(
        &product(&bx, &cy)?,
        &product(&by, &cx)?,
    )?))
}

fn orient3(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Option<i8> {
    if a == b || a == c || a == d || b == c || b == d || c == d {
        if a.iter()
            .chain(&b)
            .chain(&c)
            .chain(&d)
            .all(|x| x.is_finite())
        {
            return Some(0);
        }
    }
    if filter_range(&[a, b, c, d]) {
        let u = [b[0] - a[0], b[1] - a[1], b[2] - a[2]];
        let v = [c[0] - a[0], c[1] - a[1], c[2] - a[2]];
        let w = [d[0] - a[0], d[1] - a[1], d[2] - a[2]];
        let p = [v[1] * w[2], v[2] * w[0], v[0] * w[1]];
        let q = [v[2] * w[1], v[0] * w[2], v[1] * w[0]];
        let det = u[0] * (p[0] - q[0]) + u[1] * (p[1] - q[1]) + u[2] * (p[2] - q[2]);
        let permanent = u[0].abs() * (p[0].abs() + q[0].abs())
            + u[1].abs() * (p[1].abs() + q[1].abs())
            + u[2].abs() * (p[2].abs() + q[2].abs());
        if let Some(s) = filtered_sign(det, permanent, 1.0e-13) {
            return Some(s);
        }
    }
    orient3_exact(a, b, c, d)
}

fn orient3_exact(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Option<i8> {
    let mut u = Vec::with_capacity(3);
    let mut v = Vec::with_capacity(3);
    let mut w = Vec::with_capacity(3);
    for i in 0..3 {
        u.push(difference(b[i], a[i])?);
        v.push(difference(c[i], a[i])?);
        w.push(difference(d[i], a[i])?);
    }
    let term = |i: usize, j: usize, k: usize| -> Option<Vec<f64>> {
        product(
            u[i].as_slice(),
            &subtract_expansion(
                &product(v[j].as_slice(), w[k].as_slice())?,
                &product(v[k].as_slice(), w[j].as_slice())?,
            )?,
        )
    };
    let mut det = term(0, 1, 2)?;
    for x in term(1, 2, 0)?.into_iter().chain(term(2, 0, 1)?) {
        det = grow(&det, x)?;
    }
    Some(sign(&det))
}

fn projection(t: Triangle) -> Option<usize> {
    for axis in 0..3 {
        if orient2(t[0], t[1], t[2], axis)? != 0 {
            return Some(axis);
        }
    }
    None
}

fn point_inside(p: [f64; 3], t: Triangle, axis: usize) -> Option<bool> {
    let s = [
        orient2(t[0], t[1], p, axis)?,
        orient2(t[1], t[2], p, axis)?,
        orient2(t[2], t[0], p, axis)?,
    ];
    Some(!s.contains(&1) || !s.contains(&-1))
}

// Whether two coplanar projected segments meet anywhere other than their
// shared indexed endpoint or their identical indexed edge.
fn segments(
    a: ([usize; 2], [[f64; 3]; 2]),
    b: ([usize; 2], [[f64; 3]; 2]),
    axis: usize,
) -> Option<bool> {
    let (ai, ap) = a;
    let (bi, bp) = b;
    let s = [
        orient2(ap[0], ap[1], bp[0], axis)?,
        orient2(ap[0], ap[1], bp[1], axis)?,
        orient2(bp[0], bp[1], ap[0], axis)?,
        orient2(bp[0], bp[1], ap[1], axis)?,
    ];
    if s[0] * s[1] > 0 || s[2] * s[3] > 0 {
        return Some(false);
    }
    let shared = [bi.contains(&ai[0]), bi.contains(&ai[1])];
    if s == [0; 4] {
        let dims = match axis {
            0 => [1, 2],
            1 => [2, 0],
            _ => [0, 1],
        };
        let dim = dims.into_iter().find(|&d| ap[0][d] != ap[1][d])?;
        let lo = ap[0][dim].min(ap[1][dim]).max(bp[0][dim].min(bp[1][dim]));
        let hi = ap[0][dim].max(ap[1][dim]).min(bp[0][dim].max(bp[1][dim]));
        if lo > hi {
            return Some(false);
        }
        if shared == [true, true] {
            return Some(false);
        }
        if shared[0] != shared[1] && lo == hi {
            let p = ap[usize::from(shared[1])];
            return Some(p[dim] != lo);
        }
        return Some(true);
    }
    if shared[0] != shared[1] {
        return Some(false);
    }
    Some(true)
}

fn coplanar_segment(
    ids: [usize; 2],
    p: [[f64; 3]; 2],
    ti: [usize; 3],
    t: Triangle,
    axis: usize,
) -> Option<bool> {
    for i in 0..2 {
        if !ti.contains(&ids[i]) && point_inside(p[i], t, axis)? {
            return Some(true);
        }
    }
    for j in 0..3 {
        let k = (j + 1) % 3;
        if segments((ids, p), ([ti[j], ti[k]], [t[j], t[k]]), axis)? {
            return Some(true);
        }
    }
    Some(false)
}

// A separating edge certifies disjoint interiors. Boundary contact is safe
// only when every vertex of the other triangle on the line is an indexed
// endpoint of that edge; otherwise the existing segment tests must decide.
fn coplanar_separated(
    ai: [usize; 3],
    a: Triangle,
    bi: [usize; 3],
    b: Triangle,
    axis: usize,
) -> Option<bool> {
    for (ids, t, other_ids, other) in [(ai, a, bi, b), (bi, b, ai, a)] {
        for i in 0..3 {
            let j = (i + 1) % 3;
            let interior = orient2(t[i], t[j], t[(i + 2) % 3], axis)?;
            if interior == 0 {
                return None;
            }
            let mut separated = true;
            for k in 0..3 {
                let side = orient2(t[i], t[j], other[k], axis)?;
                if side == interior
                    || (side == 0 && other_ids[k] != ids[i] && other_ids[k] != ids[j])
                {
                    separated = false;
                    break;
                }
            }
            if separated {
                return Some(true);
            }
        }
    }
    Some(false)
}

fn edge_triangle(
    ids: [usize; 2],
    p: [[f64; 3]; 2],
    ti: [usize; 3],
    t: Triangle,
    axis: usize,
    s: [i8; 2],
) -> Option<bool> {
    if s[0] * s[1] > 0 {
        return Some(false);
    }
    if s == [0, 0] {
        return coplanar_segment(ids, p, ti, t, axis);
    }
    if s[0] == 0 || s[1] == 0 {
        let i = usize::from(s[0] != 0);
        return if ti.contains(&ids[i]) {
            Some(false)
        } else {
            point_inside(p[i], t, axis)
        };
    }
    let side = [
        orient3(p[0], p[1], t[0], t[1])?,
        orient3(p[0], p[1], t[1], t[2])?,
        orient3(p[0], p[1], t[2], t[0])?,
    ];
    Some(!side.contains(&1) || !side.contains(&-1))
}

/// True for forbidden intersection, degeneracy, inconsistent shared IDs, or
/// an arithmetic uncertainty. Shared indexed vertices and edges alone are safe.
pub(super) fn conflicts(ai: [usize; 3], a: Triangle, bi: [usize; 3], b: Triangle) -> bool {
    fn classify(ai: [usize; 3], a: Triangle, bi: [usize; 3], b: Triangle) -> Option<bool> {
        if !a.iter().chain(&b).flatten().all(|v| v.is_finite()) {
            return None;
        }
        for i in 0..3 {
            for j in 0..3 {
                if ai[i] == bi[j] && a[i] != b[j] {
                    return None;
                }
            }
        }
        if ai[0] == ai[1]
            || ai[1] == ai[2]
            || ai[2] == ai[0]
            || bi[0] == bi[1]
            || bi[1] == bi[2]
            || bi[2] == bi[0]
        {
            return None;
        }
        let aa = projection(a)?;
        let ba = projection(b)?;
        if ai.iter().all(|id| bi.contains(id)) {
            return Some(true);
        }
        // With a common indexed edge, distinct planes meet only on that edge.
        // In one plane, the third vertices overlap exactly when they lie on
        // the same side of the common edge.
        let shared = ai.iter().filter(|id| bi.contains(id)).count();
        if shared == 2 {
            let opposite_a = (0..3).find(|&i| !bi.contains(&ai[i]))?;
            let opposite_b = (0..3).find(|&i| !ai.contains(&bi[i]))?;
            if orient3(a[0], a[1], a[2], b[opposite_b])? != 0 {
                return Some(false);
            }
            let edge = [(opposite_a + 1) % 3, (opposite_a + 2) % 3];
            let side_a = orient2(a[edge[0]], a[edge[1]], a[opposite_a], aa)?;
            let side_b = orient2(a[edge[0]], a[edge[1]], b[opposite_b], aa)?;
            return if side_a == 0 || side_b == 0 {
                None
            } else {
                Some(side_a == side_b)
            };
        }
        let mut a_side = [0; 3];
        for i in 0..3 {
            if !bi.contains(&ai[i]) {
                a_side[i] = orient3(b[0], b[1], b[2], a[i])?;
            }
        }
        // The common indexed vertex lies on both planes. If every other
        // vertex is strictly on the same side, the triangles can touch only
        // at that vertex. A zero at any unshared vertex is not a certificate.
        let separated = |s: [i8; 3]| {
            s.iter().filter(|&&v| v == 0).count() == shared && (!s.contains(&1) || !s.contains(&-1))
        };
        if separated(a_side) {
            return Some(false);
        }
        let mut b_side = [0; 3];
        for i in 0..3 {
            if !ai.contains(&bi[i]) {
                b_side[i] = orient3(a[0], a[1], a[2], b[i])?;
            }
        }
        if separated(b_side) {
            return Some(false);
        }
        // Restrict this shortcut to the safe filter range so it cannot bypass
        // an uncertainty that the existing edge tests would have reported.
        if a_side == [0; 3]
            && b_side == [0; 3]
            && filter_range(&[a[0], a[1], a[2], b[0], b[1], b[2]])
            && coplanar_separated(ai, a, bi, b, aa)?
        {
            return Some(false);
        }
        for i in 0..3 {
            let j = (i + 1) % 3;
            if edge_triangle(
                [ai[i], ai[j]],
                [a[i], a[j]],
                bi,
                b,
                ba,
                [a_side[i], a_side[j]],
            )? || edge_triangle(
                [bi[i], bi[j]],
                [b[i], b[j]],
                ai,
                a,
                aa,
                [b_side[i], b_side[j]],
            )? {
                return Some(true);
            }
        }
        Some(false)
    }
    classify(ai, a, bi, b).unwrap_or(true)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn orientation_filters_match_expansions() {
        let mut state = 0x1234_5678_9abc_def0_u64;
        let mut next = || {
            state = state
                .wrapping_mul(6364136223846793005)
                .wrapping_add(1442695040888963407);
            ((state >> 32) as i32 as f64) / 1.0e8
        };
        for _ in 0..1000 {
            let a = [next(), next(), next()];
            let b = [next(), next(), next()];
            let c = [next(), next(), next()];
            let d = [next(), next(), next()];
            for axis in 0..3 {
                assert_eq!(orient2(a, b, c, axis), orient2_exact(a, b, c, axis));
            }
            assert_eq!(orient3(a, b, c, d), orient3_exact(a, b, c, d));
        }
        let a = [0., 0., 0.];
        let b = [1., 1., 0.];
        let c = [2., 2. + f64::EPSILON * 2., 0.];
        for axis in 0..3 {
            assert_eq!(orient2(a, b, c, axis), orient2_exact(a, b, c, axis));
        }
        let d = [1., 1., f64::EPSILON];
        assert_eq!(orient3(a, b, c, d), orient3_exact(a, b, c, d));
        assert_eq!(
            orient3(a, b, c, [3., 3., 0.]),
            orient3_exact(a, b, c, [3., 3., 0.])
        );
    }

    #[test]
    fn structural_zero_orientations_match_exact() {
        assert_eq!(filtered_sign(0.0, 0.0, 1.0e-14), Some(0));
        assert_eq!(filtered_sign(0.0, 2.0, 1.0e-14), None);
        for axis in 0..3 {
            let mut a = [1., 2., 3.];
            let mut b = [4., 5., 6.];
            let mut c = [7., 8., 9.];
            let mut d = [10., 11., 12.];
            a[axis] = 5.;
            b[axis] = 5.;
            c[axis] = 5.;
            d[axis] = 5.;
            assert_eq!(orient3(a, b, c, d), orient3_exact(a, b, c, d));
            assert_eq!(orient3(a, b, c, d), Some(0));
            for projection in 0..3 {
                assert_eq!(
                    orient2(a, b, c, projection),
                    orient2_exact(a, b, c, projection)
                );
            }
        }
    }

    #[test]
    fn coplanar_separation_matches_existing_edge_checks() {
        fn legacy(ai: [usize; 3], a: Triangle, bi: [usize; 3], b: Triangle, axis: usize) -> bool {
            for i in 0..3 {
                let j = (i + 1) % 3;
                if coplanar_segment([ai[i], ai[j]], [a[i], a[j]], bi, b, axis).unwrap()
                    || coplanar_segment([bi[i], bi[j]], [b[i], b[j]], ai, a, axis).unwrap()
                {
                    return true;
                }
            }
            false
        }

        let points = [[0., 0.], [2., 0.], [0., 2.], [2., 2.], [1., 0.], [1., 1.]];
        let permutations = [
            [0, 1, 2],
            [0, 2, 1],
            [1, 0, 2],
            [1, 2, 0],
            [2, 0, 1],
            [2, 1, 0],
        ];
        for axis in 0..3 {
            let points = points.map(|p| match axis {
                0 => [3., p[0], p[1]],
                1 => [p[1], 3., p[0]],
                _ => [p[0], p[1], 3.],
            });
            let mut triangles = Vec::new();
            for i in 0..points.len() {
                for j in i + 1..points.len() {
                    for k in j + 1..points.len() {
                        if orient2(points[i], points[j], points[k], axis) != Some(0) {
                            triangles.push([i, j, k]);
                        }
                    }
                }
            }
            for &a_ids in &triangles {
                for &b_ids in &triangles {
                    if a_ids == b_ids {
                        continue;
                    }
                    for order_a in permutations {
                        for order_b in permutations {
                            let ai = order_a.map(|i| a_ids[i]);
                            let bi = order_b.map(|i| b_ids[i]);
                            let a = ai.map(|i| points[i]);
                            let b = bi.map(|i| points[i]);
                            let expected = legacy(ai, a, bi, b, axis);
                            if coplanar_separated(ai, a, bi, b, axis).unwrap() {
                                assert!(!expected, "{ai:?} {bi:?} axis {axis}");
                            }
                            assert_eq!(
                                conflicts(ai, a, bi, b),
                                expected,
                                "{ai:?} {bi:?} axis {axis}"
                            );
                        }
                    }
                }
            }
        }
        let a = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]];
        for (b, expected) in [
            ([[0., 0., 0.], [-1., 0., 0.], [0., -1., 0.]], true),
            ([[1., 0., 0.], [3., 0., 0.], [2., -1., 0.]], true),
            ([[3., 0., 0.], [4., 0., 0.], [3., -1., 0.]], false),
            ([[3., 1., 0.], [4., 1., 0.], [3., 2., 0.]], false),
        ] {
            let ai = [0, 1, 2];
            let bi = [3, 4, 5];
            assert_eq!(legacy(ai, a, bi, b, 2), expected);
            assert_eq!(conflicts(ai, a, bi, b), expected);
            if expected {
                assert_eq!(coplanar_separated(ai, a, bi, b, 2), Some(false));
            }
        }
    }

    #[test]
    fn orientation_extremes_fall_back_and_duplicates_are_zero() {
        let a = [0., 0., 0.];
        for scale in [1.0e-200, 1.0e200] {
            let b = [scale, 0., 0.];
            let c = [0., scale, 0.];
            let d = [0., 0., scale];
            assert_eq!(orient2(a, b, c, 2), orient2_exact(a, b, c, 2));
            assert_eq!(orient3(a, b, c, d), orient3_exact(a, b, c, d));
            assert_eq!(orient2(a, b, a, 2), Some(0));
            assert_eq!(orient3(a, b, c, b), Some(0));
        }
        assert_eq!(orient3(a, a, a, [f64::INFINITY, 0., 0.]), None);
    }

    #[test]
    fn index_refits_removed_and_moved_faces() {
        let a = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let b = [[10., 0., 0.], [11., 0., 0.], [10., 1., 0.]];
        let triangles = [Some(a), Some(b), None];
        let mut tree = Bvh::new(triangles.len(), |id| triangles[id]);
        assert_eq!(tree.query(a, 0.), vec![0]);
        tree.update(0, None);
        tree.update(2, Some(a));
        assert_eq!(tree.query(a, 0.), vec![2]);
        tree.update(2, Some(b));
        assert!(tree.query(a, 0.).is_empty());
        assert_eq!(tree.query(a, 10.), vec![1, 2]);
    }

    #[test]
    fn construction_permutation_keeps_bounds_with_stable_ids() {
        let triangles: Vec<_> = [8., 1., 6., 3., 10., 0., 5.]
            .into_iter()
            .map(|x| Some([[x, 0., 0.], [x + 0.25, 0., 0.], [x, 1., 0.]]))
            .collect();
        let mut calls = Vec::new();
        let mut tree = Bvh::new(triangles.len(), |id| {
            calls.push(id);
            triangles[id]
        });
        assert_eq!(calls, (0..triangles.len()).collect::<Vec<_>>());
        for (id, triangle) in triangles.iter().enumerate() {
            assert_eq!(tree.query(triangle.unwrap(), 0.), vec![id]);
        }
        tree.update(2, None);
        tree.update(4, triangles[2]);
        assert_eq!(tree.query(triangles[2].unwrap(), 0.), vec![4]);
        let invalid = [[f64::NAN, 0., 0.]; 3];
        tree.update(1, Some(invalid));
        assert!(tree.query(triangles[0].unwrap(), 0.).contains(&1));
        tree.update(1, None);
        assert!(!tree.query(triangles[0].unwrap(), 0.).contains(&1));
    }

    #[test]
    fn overlap_traversal_short_circuits_and_matches_query_after_refits() {
        let a = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let b = [[10., 0., 0.], [11., 0., 0.], [10., 1., 0.]];
        let triangles = [Some(a), Some(b), Some(a), None, Some(b)];
        let mut tree = Bvh::new(triangles.len(), |id| triangles[id]);
        tree.update(0, Some(a));
        tree.update(2, None);
        tree.update(3, Some(a));
        assert!(tree.face_bounds(2).is_none());
        assert!(tree
            .face_bounds(3)
            .unwrap()
            .overlaps(Bounds::triangle(a), 0.));
        let fan = Bounds::triangle(a).union(Bounds::triangle(b));
        let mut fan_ids = Vec::new();
        assert!(!tree.any_overlapping_bounds(fan, 0., |id| {
            fan_ids.push(id);
            false
        }));
        fan_ids.sort_unstable();
        assert_eq!(fan_ids, vec![0, 1, 3, 4]);
        assert!(Bounds::triangle(a).overlaps(Bounds::triangle(b), f64::NAN));
        for padding in [0., 10., f64::NAN, -1.] {
            let expected = tree.query(a, padding);
            let mut visited = Vec::new();
            assert!(!tree.any_overlap(a, padding, |id| {
                visited.push(id);
                false
            }));
            assert_eq!(visited, expected);
            let mut count = 0;
            assert_eq!(
                tree.any_overlap(a, padding, |_| {
                    count += 1;
                    true
                }),
                !expected.is_empty()
            );
            assert_eq!(count, usize::from(!expected.is_empty()));
        }
        assert!(!Bvh::new(0, |_| None).any_overlap(a, 0., |_| true));
    }

    #[test]
    fn shared_edge_permutations_and_uncertainty() {
        let a = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]];
        for (point, expected) in [
            ([0., -1., 0.], false),
            ([1., 1., 0.], true),
            ([0., 0., 1.], false),
        ] {
            let b = [a[0], a[1], point];
            for order in [[0, 1, 2], [1, 0, 2], [2, 0, 1], [2, 1, 0]] {
                let ids = order.map(|id| if id == 2 { 3 } else { id });
                let tri = [b[order[0]], b[order[1]], b[order[2]]];
                assert_eq!(conflicts([0, 1, 2], a, ids, tri), expected);
                assert_eq!(conflicts(ids, tri, [0, 1, 2], a), expected);
            }
        }
        assert!(conflicts(
            [0, 1, 2],
            a,
            [0, 1, 3],
            [a[0], a[1], [1., 0., 0.]]
        ));
        assert!(conflicts(
            [0, 1, 2],
            a,
            [0, 1, 3],
            [a[0], [2.1, 0., 0.], [0., -1., 0.]]
        ));
    }

    #[test]
    fn adjacency_and_coplanar_overlaps() {
        let a = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]];
        let edge = [[2., 0., 0.], [0., 0., 0.], [0., -2., 0.]];
        let overlap = [[2., 0., 0.], [0., 0., 0.], [1., 1., 0.]];
        let vertex = [[0., 0., 0.], [-1., 0., 0.], [0., -1., 0.]];
        assert!(!conflicts([0, 1, 2], a, [1, 0, 3], edge));
        assert!(conflicts([0, 1, 2], a, [1, 0, 3], overlap));
        assert!(!conflicts([0, 1, 2], a, [0, 3, 4], vertex));
        assert!(conflicts([0, 1, 2], a, [3, 4, 5], overlap));
        let contained = [[0.25, 0.25, 0.], [0.5, 0.25, 0.], [0.25, 0.5, 0.]];
        assert!(conflicts([0, 1, 2], a, [3, 4, 5], contained));
        let folded = [[2., 0., 0.], [0., 0., 0.], [0., 0., 1.]];
        assert!(!conflicts([0, 1, 2], a, [1, 0, 3], folded));
    }

    #[test]
    fn crossing_and_disjoint_sheets() {
        let a = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]];
        let crossing = [[0.5, 0.5, -1.], [0.5, 0.5, 1.], [1., 0.5, 1.]];
        let above = [[0., 0., 0.1], [2., 0., 0.1], [0., 2., 0.1]];
        assert!(conflicts([0, 1, 2], a, [3, 4, 5], crossing));
        assert!(!conflicts([0, 1, 2], a, [3, 4, 5], above));
        assert!(conflicts([0, 1, 2], a, [3, 4, 5], [[0.; 3]; 3]));
        let tiny = [[0., 0., 0.], [1.0e-200, 0., 0.], [0., 1.0e-200, 0.]];
        assert!(conflicts([0, 1, 2], tiny, [3, 4, 5], above));
    }

    #[test]
    fn thin_sheet_crossing_and_separation() {
        let sheet = [[0., 0., 0.], [2., 0., 0.], [0., 2., 0.]];
        let disjoint = [[0.2, 0.2, 1.0e-8], [1., 0.2, 1.0e-8], [0.2, 1., 1.0e-8]];
        let crossing = [[0.2, 0.2, -1.0e-8], [1., 0.2, 1.0e-8], [0.2, 1., 1.0e-8]];
        assert!(!conflicts([0, 1, 2], sheet, [3, 4, 5], disjoint));
        assert!(conflicts([0, 1, 2], sheet, [3, 4, 5], crossing));
        assert!(conflicts([3, 4, 5], crossing, [0, 1, 2], sheet));
    }
}
