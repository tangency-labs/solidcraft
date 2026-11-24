//! Conservative intersection checks and a refittable index of stable face IDs.
//! Deviations: BVH arrangement recursion is bounded by `usize::BITS`.

use super::{
    buffer, check_cancel, check_stride, difference, filled, grow, invalid, invariant, product,
    subtract_expansion, Result,
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
        let [min, ..] = self.min;
        let [max, ..] = self.max;
        min > max
    }

    pub(super) fn triangle(t: Triangle) -> Self {
        if !t.iter().flatten().all(|x| x.is_finite()) {
            return Self {
                min: [f64::NEG_INFINITY; 3],
                max: [f64::INFINITY; 3],
            };
        }
        let [first, second, third] = t;
        let mut result = Self {
            min: first,
            max: first,
        };
        for p in [second, third] {
            for ((min, max), value) in result.min.iter_mut().zip(&mut result.max).zip(p) {
                *min = min.min(value);
                *max = max.max(value);
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
        for (((min, max), other_min), other_max) in result
            .min
            .iter_mut()
            .zip(&mut result.max)
            .zip(other.min)
            .zip(other.max)
        {
            *min = min.min(other_min);
            *max = max.max(other_max);
        }
        result
    }

    pub(super) fn overlaps(self, other: Self, padding: f64) -> bool {
        !padding.is_finite()
            || padding < 0.0
            || self
                .min
                .into_iter()
                .zip(self.max)
                .zip(other.min.into_iter().zip(other.max))
                .all(|((min, max), (other_min, other_max))| {
                    min - padding <= other_max && other_min - padding <= max
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
    ) -> Result<Self> {
        fn arrange(
            slots: &mut [usize],
            bounds: &[Bounds],
            cancel: &mut impl FnMut() -> bool,
            depth: u32,
        ) -> Result<()> {
            if depth > usize::BITS {
                return Err(invariant("BVH depth exceeded"));
            }
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
                aggregate = aggregate.union(
                    *bounds
                        .get(id)
                        .ok_or_else(|| invariant("invalid BVH slot"))?,
                );
            }
            let axis = if aggregate.is_empty() {
                0
            } else {
                aggregate
                    .max
                    .into_iter()
                    .zip(aggregate.min)
                    .enumerate()
                    .max_by(|(_, (amax, amin)), (_, (bmax, bmin))| {
                        (amax - amin).total_cmp(&(bmax - bmin))
                    })
                    .map_or(0, |(axis, _)| axis)
            };
            let mid = slots.len() / 2;
            if slots.get(mid).is_none() {
                return Err(invariant("invalid BVH partition"));
            }
            // All IDs are checked above, before the infallible comparator runs.
            slots.select_nth_unstable_by(mid, |&a, &b| {
                let center = |id: usize| {
                    let b = bounds.get(id)?;
                    if b.is_empty() {
                        None
                    } else {
                        Some(b.min.get(axis)? * 0.5 + b.max.get(axis)? * 0.5)
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
            let (left, right) = slots
                .split_at_mut_checked(mid)
                .ok_or_else(|| invariant("invalid BVH partition"))?;
            arrange(left, bounds, cancel, depth + 1)?;
            arrange(right, bounds, cancel, depth + 1)?;
            Ok(())
        }
        fn swap<T>(items: &mut [T], a: usize, b: usize) -> Result<()> {
            let (low, high) = if a < b { (a, b) } else { (b, a) };
            let (left, right) = items
                .split_at_mut_checked(high)
                .ok_or_else(|| invariant("invalid BVH slot"))?;
            let first = left
                .get_mut(low)
                .ok_or_else(|| invariant("invalid BVH slot"))?;
            let second = right
                .first_mut()
                .ok_or_else(|| invariant("invalid BVH slot"))?;
            std::mem::swap(first, second);
            Ok(())
        }
        check_cancel(cancel)?;
        let size_error = || invalid("BVH is too large");
        let base = len
            .max(1)
            .checked_next_power_of_two()
            .ok_or_else(size_error)?;
        let node_count = base.checked_mul(2).ok_or_else(size_error)?;
        let mut nodes = filled(node_count, Bounds::EMPTY)?;
        let mut slots = buffer(base)?;
        slots.extend(0..base);
        let mut leaves = filled(base, 0usize)?;
        for (id, node) in nodes.iter_mut().skip(base).take(len).enumerate() {
            check_stride(id, cancel)?;
            *node = triangle(id).map_or(Bounds::EMPTY, Bounds::triangle);
        }
        arrange(
            &mut slots,
            nodes
                .get(base..)
                .ok_or_else(|| invariant("invalid BVH nodes"))?,
            cancel,
            0,
        )?;

        // Permute the leaf bounds in place. During this pass leaves maps each
        // original slot to its destination; afterwards it maps IDs to nodes.
        for (slot, &id) in slots.iter().enumerate() {
            check_stride(slot, cancel)?;
            *leaves
                .get_mut(id)
                .ok_or_else(|| invariant("invalid BVH slot"))? = slot;
        }
        let mut swaps = 0usize;
        for i in 0..base {
            check_stride(i, cancel)?;
            while *leaves.get(i).ok_or_else(|| invariant("invalid BVH slot"))? != i {
                if base >= super::CANCEL_STRIDE {
                    check_stride(swaps, cancel)?;
                }
                swaps = swaps.wrapping_add(1);
                let dest = *leaves.get(i).ok_or_else(|| invariant("invalid BVH slot"))?;
                if dest >= base {
                    return Err(invariant("invalid BVH slot"));
                }
                swap(&mut nodes, base + i, base + dest)?;
                swap(&mut leaves, i, dest)?;
            }
        }
        leaves.truncate(len);
        for (slot, &id) in slots.iter().enumerate() {
            check_stride(slot, cancel)?;
            if id < len {
                *leaves
                    .get_mut(id)
                    .ok_or_else(|| invariant("invalid BVH slot"))? = base + slot;
            }
        }
        for node in (1..base).rev() {
            check_stride(node, cancel)?;
            let left = *nodes
                .get(2 * node)
                .ok_or_else(|| invariant("invalid BVH node"))?;
            let right = *nodes
                .get(2 * node + 1)
                .ok_or_else(|| invariant("invalid BVH node"))?;
            *nodes
                .get_mut(node)
                .ok_or_else(|| invariant("invalid BVH node"))? = left.union(right);
        }
        check_cancel(cancel)?;
        Ok(Self {
            base,
            leaves,
            ids: slots,
            nodes,
        })
    }

    pub(super) fn update(&mut self, id: usize, triangle: Option<Triangle>) -> Result<()> {
        let mut node = *self
            .leaves
            .get(id)
            .ok_or_else(|| invariant("invalid BVH face"))?;
        let bounds = triangle.map_or(Bounds::EMPTY, Bounds::triangle);
        let slot = self
            .nodes
            .get_mut(node)
            .ok_or_else(|| invariant("invalid BVH node"))?;
        if *slot == bounds {
            return Ok(());
        }
        *slot = bounds;
        while node > 1 {
            node /= 2;
            let left = *self
                .nodes
                .get(2 * node)
                .ok_or_else(|| invariant("invalid BVH node"))?;
            let right = *self
                .nodes
                .get(2 * node + 1)
                .ok_or_else(|| invariant("invalid BVH node"))?;
            let bounds = left.union(right);
            let slot = self
                .nodes
                .get_mut(node)
                .ok_or_else(|| invariant("invalid BVH node"))?;
            if *slot == bounds {
                break;
            }
            *slot = bounds;
        }
        Ok(())
    }

    pub(super) fn face_bounds(&self, id: usize) -> Result<Option<Bounds>> {
        let node = *self
            .leaves
            .get(id)
            .ok_or_else(|| invariant("invalid BVH face"))?;
        let bounds = *self
            .nodes
            .get(node)
            .ok_or_else(|| invariant("invalid BVH node"))?;
        Ok((!bounds.is_empty()).then_some(bounds))
    }

    #[cfg(test)]
    pub(super) fn any_overlap(
        &self,
        triangle: Triangle,
        padding: f64,
        mut predicate: impl FnMut(usize) -> bool,
    ) -> bool {
        self.any_overlapping_bounds(
            Bounds::triangle(triangle),
            padding,
            |id| Ok(predicate(id)),
            &mut || false,
        )
        .unwrap()
    }

    /// Visit faces overlapping precomputed bounds, conservatively for invalid padding.
    pub(super) fn any_overlapping_bounds(
        &self,
        bounds: Bounds,
        padding: f64,
        mut predicate: impl FnMut(usize) -> Result<bool>,
        cancel: &mut impl FnMut() -> bool,
    ) -> Result<bool> {
        let mut node = 1;
        let mut visited = 0usize;
        loop {
            check_stride(visited, cancel)?;
            visited = visited.wrapping_add(1);
            let b = *self
                .nodes
                .get(node)
                .ok_or_else(|| invariant("invalid BVH node"))?;
            if !b.is_empty() && bounds.overlaps(b, padding) {
                if node < self.base {
                    node *= 2;
                    continue;
                }
                let id = *self
                    .ids
                    .get(node - self.base)
                    .ok_or_else(|| invariant("invalid BVH slot"))?;
                if id < self.leaves.len() && predicate(id)? {
                    return Ok(true);
                }
            }
            while node > 1 && node % 2 == 1 {
                node /= 2;
            }
            if node == 1 {
                return Ok(false);
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
    if (a == b || b == c || c == a) && a.iter().chain(&b).chain(&c).all(|x| x.is_finite()) {
        return Some(0);
    }
    if filter_range(&[a, b, c]) {
        let bx = b.get(x)? - a.get(x)?;
        let by = b.get(y)? - a.get(y)?;
        let cx = c.get(x)? - a.get(x)?;
        let cy = c.get(y)? - a.get(y)?;
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
    let bx = difference(*b.get(x)?, *a.get(x)?)?;
    let by = difference(*b.get(y)?, *a.get(y)?)?;
    let cx = difference(*c.get(x)?, *a.get(x)?)?;
    let cy = difference(*c.get(y)?, *a.get(y)?)?;
    Some(sign(&subtract_expansion(
        &product(&bx, &cy)?,
        &product(&by, &cx)?,
    )?))
}

fn orient3(a: [f64; 3], b: [f64; 3], c: [f64; 3], d: [f64; 3]) -> Option<i8> {
    if (a == b || a == c || a == d || b == c || b == d || c == d)
        && a.iter()
            .chain(&b)
            .chain(&c)
            .chain(&d)
            .all(|x| x.is_finite())
    {
        return Some(0);
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
    let u = [
        difference(b[0], a[0])?,
        difference(b[1], a[1])?,
        difference(b[2], a[2])?,
    ];
    let v = [
        difference(c[0], a[0])?,
        difference(c[1], a[1])?,
        difference(c[2], a[2])?,
    ];
    let w = [
        difference(d[0], a[0])?,
        difference(d[1], a[1])?,
        difference(d[2], a[2])?,
    ];
    let term = |i: usize, j: usize, k: usize| -> Option<Vec<f64>> {
        product(
            u.get(i)?.as_slice(),
            &subtract_expansion(
                &product(v.get(j)?.as_slice(), w.get(k)?.as_slice())?,
                &product(v.get(k)?.as_slice(), w.get(j)?.as_slice())?,
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
        let dim = dims.into_iter().find(|&d| ap[0].get(d) != ap[1].get(d))?;
        let (a0, a1, b0, b1) = (
            *ap[0].get(dim)?,
            *ap[1].get(dim)?,
            *bp[0].get(dim)?,
            *bp[1].get(dim)?,
        );
        let lo = a0.min(a1).max(b0.min(b1));
        let hi = a0.max(a1).min(b0.max(b1));
        if lo > hi {
            return Some(false);
        }
        if shared == [true, true] {
            return Some(false);
        }
        if shared[0] != shared[1] && lo == hi {
            let p = ap.get(usize::from(shared[1]))?;
            return Some(*p.get(dim)? != lo);
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
    for (&id, &point) in ids.iter().zip(&p) {
        if !ti.contains(&id) && point_inside(point, t, axis)? {
            return Some(true);
        }
    }
    for j in 0..3 {
        let k = (j + 1) % 3;
        if segments(
            (ids, p),
            ([*ti.get(j)?, *ti.get(k)?], [*t.get(j)?, *t.get(k)?]),
            axis,
        )? {
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
            let interior = orient2(*t.get(i)?, *t.get(j)?, *t.get((i + 2) % 3)?, axis)?;
            if interior == 0 {
                return None;
            }
            let mut separated = true;
            for (&other_id, &point) in other_ids.iter().zip(&other) {
                let side = orient2(*t.get(i)?, *t.get(j)?, point, axis)?;
                if side == interior
                    || (side == 0 && other_id != *ids.get(i)? && other_id != *ids.get(j)?)
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
        return if ti.contains(ids.get(i)?) {
            Some(false)
        } else {
            point_inside(*p.get(i)?, t, axis)
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
        for (&aid, &ap) in ai.iter().zip(&a) {
            for (&bid, &bp) in bi.iter().zip(&b) {
                if aid == bid && ap != bp {
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
            let opposite_a = ai.iter().position(|id| !bi.contains(id))?;
            let opposite_b = bi.iter().position(|id| !ai.contains(id))?;
            if orient3(a[0], a[1], a[2], *b.get(opposite_b)?)? != 0 {
                return Some(false);
            }
            let edge = [*a.get((opposite_a + 1) % 3)?, *a.get((opposite_a + 2) % 3)?];
            let side_a = orient2(edge[0], edge[1], *a.get(opposite_a)?, aa)?;
            let side_b = orient2(edge[0], edge[1], *b.get(opposite_b)?, aa)?;
            return if side_a == 0 || side_b == 0 {
                None
            } else {
                Some(side_a == side_b)
            };
        }
        let mut a_side = [0; 3];
        for ((side, &id), &point) in a_side.iter_mut().zip(&ai).zip(&a) {
            if !bi.contains(&id) {
                *side = orient3(b[0], b[1], b[2], point)?;
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
        for ((side, &id), &point) in b_side.iter_mut().zip(&bi).zip(&b) {
            if !ai.contains(&id) {
                *side = orient3(a[0], a[1], a[2], point)?;
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
                [*ai.get(i)?, *ai.get(j)?],
                [*a.get(i)?, *a.get(j)?],
                bi,
                b,
                ba,
                [*a_side.get(i)?, *a_side.get(j)?],
            )? || edge_triangle(
                [*bi.get(i)?, *bi.get(j)?],
                [*b.get(i)?, *b.get(j)?],
                ai,
                a,
                aa,
                [*b_side.get(i)?, *b_side.get(j)?],
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
            f64::from((state >> 32) as i32) / 1.0e8
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
        tree.update(0, None).unwrap();
        tree.update(2, Some(a)).unwrap();
        assert_eq!(tree.query(a, 0.), vec![2]);
        tree.update(2, Some(b)).unwrap();
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
        tree.update(2, None).unwrap();
        tree.update(4, triangles[2]).unwrap();
        assert_eq!(tree.query(triangles[2].unwrap(), 0.), vec![4]);
        let invalid = [[f64::NAN, 0., 0.]; 3];
        tree.update(1, Some(invalid)).unwrap();
        assert!(tree.query(triangles[0].unwrap(), 0.).contains(&1));
        tree.update(1, None).unwrap();
        assert!(!tree.query(triangles[0].unwrap(), 0.).contains(&1));
    }

    #[test]
    fn overlap_traversal_short_circuits_and_matches_query_after_refits() {
        let a = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let b = [[10., 0., 0.], [11., 0., 0.], [10., 1., 0.]];
        let triangles = [Some(a), Some(b), Some(a), None, Some(b)];
        let mut tree = Bvh::new(triangles.len(), |id| triangles[id]);
        tree.update(0, Some(a)).unwrap();
        tree.update(2, None).unwrap();
        tree.update(3, Some(a)).unwrap();
        assert!(tree.face_bounds(2).unwrap().is_none());
        assert!(tree
            .face_bounds(3)
            .unwrap()
            .unwrap()
            .overlaps(Bounds::triangle(a), 0.));
        let fan = Bounds::triangle(a).union(Bounds::triangle(b));
        let mut fan_ids = Vec::new();
        assert!(!tree
            .any_overlapping_bounds(
                fan,
                0.,
                |id| {
                    fan_ids.push(id);
                    Ok(false)
                },
                &mut || false
            )
            .unwrap());
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
    fn oversized_bvh_refuses_before_reading_triangles() {
        for len in [usize::MAX, 1usize << (usize::BITS - 1)] {
            let mut called = false;
            let result = Bvh::new_with_cancel(
                len,
                |_| {
                    called = true;
                    None
                },
                &mut || false,
            );
            assert!(matches!(
                result,
                Err(crate::SolidError::InvalidInput { .. })
            ));
            assert!(!called);
        }
    }

    #[test]
    fn cancellation_interrupts_overlap_traversal() {
        use std::cell::Cell;
        let triangle = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]];
        let tree = Bvh::new(super::super::CANCEL_STRIDE * 2, |_| Some(triangle));
        let started = Cell::new(false);
        let visited = Cell::new(0);
        let result = tree.any_overlapping_bounds(
            Bounds::triangle(triangle),
            0.,
            |_| {
                started.set(true);
                visited.set(visited.get() + 1);
                Ok(false)
            },
            &mut || started.get(),
        );
        assert!(result.unwrap_err().is_cancelled());
        assert!(visited.get() > 0 && visited.get() < tree.leaves.len());
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
