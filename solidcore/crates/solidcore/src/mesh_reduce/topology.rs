//! Input topology checks using compact, sorted records.
//!
//! Undirected edges are grouped with their incident face and direction.
//! Disconnected vertex links are accepted, as they were by the reducer's
//! original edge-based validation.

use super::{
    buffer, check_cancel, check_stride, dot, face_normal, filled, invalid, invariant, Result,
};

/// Compare directions without changing the original corner attributes. Scaling
/// first avoids overflow and underflow for finite, nonzero normals.
pub(super) fn same_normal_direction(a: [f64; 3], b: [f64; 3]) -> bool {
    fn direction(v: [f64; 3]) -> Option<[f64; 3]> {
        if !v.iter().all(|x| x.is_finite()) {
            return None;
        }
        let scale = v.iter().fold(0.0_f64, |m, x| m.max(x.abs()));
        if scale == 0.0 {
            return None;
        }
        Some(v.map(|x| x / scale))
    }
    if a == b {
        return a.iter().all(|x| x.is_finite()) && a.iter().any(|&x| x != 0.0);
    }

    direction(a).zip(direction(b)).is_some_and(|(a, b)| {
        a.into_iter()
            .zip(b)
            .all(|(x, y)| (x - y).abs() <= 32.0 * f64::EPSILON)
    })
}

/// Normalized topology. Retained facet slots keep their original order and winding.
#[derive(Debug)]
pub(super) struct PreparedTopology {
    pub protected: Vec<bool>,
    pub retained: Vec<bool>,
    pub triangles: usize,
    pub closed: bool,
}

fn triangle_at(indices: &[u32], face: usize) -> Option<[u32; 3]> {
    let start = face.checked_mul(3)?;
    match indices.get(start..start.checked_add(3)?)? {
        &[a, b, c] => Some([a, b, c]),
        _ => None,
    }
}

// Integer records only. Heap sorting needs no scratch allocation and checks cancellation
// through its comparisons rather than leaving one input-sized uninterruptible sort.
fn sort_records<T: Ord>(items: &mut [T], cancel: &mut impl FnMut() -> bool) -> Result<()> {
    fn swap<T>(items: &mut [T], a: usize, b: usize) -> Result<()> {
        if a == b {
            return items
                .get(a)
                .map(|_| ())
                .ok_or_else(|| invariant("invalid sort slot"));
        }
        let (low, high) = if a < b { (a, b) } else { (b, a) };
        let (left, right) = items
            .split_at_mut_checked(high)
            .ok_or_else(|| invariant("invalid sort slot"))?;
        let a = left
            .get_mut(low)
            .ok_or_else(|| invariant("invalid sort slot"))?;
        let b = right
            .first_mut()
            .ok_or_else(|| invariant("invalid sort slot"))?;
        std::mem::swap(a, b);
        Ok(())
    }
    fn sift<T: Ord>(
        items: &mut [T],
        mut root: usize,
        end: usize,
        work: &mut usize,
        cancel: &mut impl FnMut() -> bool,
    ) -> Result<()> {
        loop {
            check_stride(*work, cancel)?;
            *work = work.wrapping_add(1);
            let left = root
                .checked_mul(2)
                .and_then(|n| n.checked_add(1))
                .ok_or_else(|| invariant("sort child overflow"))?;
            if left >= end {
                return Ok(());
            }
            let right = left
                .checked_add(1)
                .ok_or_else(|| invariant("sort child overflow"))?;
            let mut child = left;
            if right < end
                && items
                    .get(left)
                    .ok_or_else(|| invariant("invalid sort slot"))?
                    < items
                        .get(right)
                        .ok_or_else(|| invariant("invalid sort slot"))?
            {
                child = right;
            }
            if items
                .get(root)
                .ok_or_else(|| invariant("invalid sort slot"))?
                >= items
                    .get(child)
                    .ok_or_else(|| invariant("invalid sort slot"))?
            {
                return Ok(());
            }
            swap(items, root, child)?;
            root = child;
        }
    }
    check_cancel(cancel)?;
    let mut work = 0;
    let len = items.len();
    for root in (0..len / 2).rev() {
        sift(items, root, len, &mut work, cancel)?;
    }
    for end in (1..len).rev() {
        swap(items, 0, end)?;
        sift(items, 0, end, &mut work, cancel)?;
    }
    check_cancel(cancel)
}

fn check_duplicate(
    indices: &[u32],
    normals: Option<&[[f64; 3]]>,
    first: usize,
    other: usize,
) -> Result<()> {
    let a = triangle_at(indices, first).ok_or_else(|| invariant("missing original triangle"))?;
    let b = triangle_at(indices, other).ok_or_else(|| invariant("missing duplicate triangle"))?;
    let start = a
        .iter()
        .position(|vertex| b.first() == Some(vertex))
        .ok_or_else(|| invariant("mismatched duplicate vertices"))?;
    for corner in 0..3 {
        let original = (start + corner) % 3;
        if a.get(original) != b.get(corner) {
            return Err(invalid("oppositely wound duplicate triangle"));
        }
        if let Some(normals) = normals {
            let x = first
                .checked_mul(3)
                .and_then(|n| n.checked_add(original))
                .and_then(|n| normals.get(n))
                .ok_or_else(|| invalid("invalid corner normals"))?;
            let y = other
                .checked_mul(3)
                .and_then(|n| n.checked_add(corner))
                .and_then(|n| normals.get(n))
                .ok_or_else(|| invalid("invalid corner normals"))?;
            if x != y {
                return Err(invalid("duplicate triangle has conflicting corner normals"));
            }
        }
    }
    Ok(())
}

#[cfg(test)]
fn validate(indices: &[u32], positions: &[[f64; 3]]) -> Result<Vec<bool>> {
    validate_with_cancel(indices, positions, None, &mut || false).map(|topology| topology.protected)
}

/// Validate topology and mark endpoints of boundary, sharp and normal-seam edges.
pub(super) fn validate_with_cancel(
    indices: &[u32],
    positions: &[[f64; 3]],
    corner_normals: Option<&[[f64; 3]]>,
    cancel: &mut impl FnMut() -> bool,
) -> Result<PreparedTopology> {
    if indices.len() % 3 != 0 {
        return Err(invalid("indices are not triangle triples"));
    }
    let original_triangles = indices.len() / 3;
    let mut retained = filled(original_triangles, true)?;
    let mut triangles = original_triangles;
    // Drop the grouping records before allocating the edge table. Original IDs in the
    // key select the earliest occurrence, while the mask preserves input facet order.
    {
        let mut faces = buffer(original_triangles)?;
        for (i, tri) in indices.chunks_exact(3).enumerate() {
            check_stride(i, cancel)?;
            let &[a, b, c] = tri else {
                return Err(invalid("indices are not triangle triples"));
            };
            if a == b
                || b == c
                || c == a
                || a as usize >= positions.len()
                || b as usize >= positions.len()
                || c as usize >= positions.len()
            {
                return Err(invalid("invalid triangle indices"));
            }
            let mut face = [a, b, c];
            face.sort_unstable();
            faces.push((face, i));
        }
        sort_records(&mut faces, cancel)?;
        let mut first = None;
        for (i, &(key, face)) in faces.iter().enumerate() {
            check_stride(i, cancel)?;
            if let Some((_, original)) = first.filter(|(previous, _)| *previous == key) {
                check_duplicate(indices, corner_normals, original, face)?;
                *retained
                    .get_mut(face)
                    .ok_or_else(|| invariant("invalid duplicate slot"))? = false;
                triangles = triangles
                    .checked_sub(1)
                    .ok_or_else(|| invariant("duplicate count underflow"))?;
            } else {
                first = Some((key, face));
            }
        }
    }

    // The low bit records direction; the remaining bits are the face index.
    let mut edges = Vec::new();
    edges
        .try_reserve_exact(
            triangles
                .checked_mul(3)
                .ok_or_else(|| invalid("mesh topology is too large"))?,
        )
        .map_err(|_| invalid("mesh topology is too large"))?;
    for (face, tri) in indices.chunks_exact(3).enumerate() {
        check_stride(face, cancel)?;
        if !retained
            .get(face)
            .copied()
            .ok_or_else(|| invariant("invalid retained face"))?
        {
            continue;
        }
        let &[a, b, c] = tri else {
            return Err(invalid("indices are not triangle triples"));
        };
        for (from, to) in [(a, b), (b, c), (c, a)] {
            let (lo, hi) = if from < to { (from, to) } else { (to, from) };
            let key = (u64::from(lo) << 32) | u64::from(hi);
            edges.push((key, (face << 1) | usize::from(from < to)));
        }
    }
    check_cancel(cancel)?;
    sort_records(&mut edges, cancel)?;
    check_cancel(cancel)?;

    let mut protected = Vec::new();
    protected
        .try_reserve_exact(positions.len())
        .map_err(|_| invalid("mesh topology is too large"))?;
    protected.resize(positions.len(), false);
    let triangle = |face: usize| {
        let start = face.checked_mul(3)?;
        let end = start.checked_add(3)?;
        match indices.get(start..end)? {
            &[a, b, c] => Some([a, b, c]),
            _ => None,
        }
    };
    let normal = |face: usize| {
        let [a, b, c] = triangle(face)?;
        face_normal([
            *positions.get(a as usize)?,
            *positions.get(b as usize)?,
            *positions.get(c as usize)?,
        ])
    };
    let corner_normal = |normals: &[[f64; 3]], face: usize, vertex: u32| {
        let tri = triangle(face)?;
        let corner = tri.iter().position(|&v| v == vertex)?;
        normals
            .get(face.checked_mul(3)?.checked_add(corner)?)
            .copied()
    };
    let mut i = 0;
    let mut groups = 0usize;
    let mut closed = triangles != 0;
    while i < edges.len() {
        check_stride(groups, cancel)?;
        groups += 1;
        let &(key, first) = edges.get(i).ok_or_else(|| invalid("invalid edge group"))?;
        let mut end = i + 1;
        while let Some(&(next_key, _)) = edges.get(end) {
            if next_key != key {
                break;
            }
            check_stride(end, cancel)?;
            end += 1;
        }
        let lo = (key >> 32) as u32;
        let hi = key as u32;
        let already_protected = *protected
            .get(lo as usize)
            .ok_or_else(|| invalid("invalid triangle indices"))?
            && *protected
                .get(hi as usize)
                .ok_or_else(|| invalid("invalid triangle indices"))?;
        let sharp = match end - i {
            1 => {
                closed = false;
                true
            }
            2 => {
                let second = edges
                    .get(i + 1)
                    .ok_or_else(|| invalid("invalid edge group"))?
                    .1;
                if first & 1 == second & 1 {
                    return Err(invalid("non-manifold or inconsistently wound edge"));
                }
                if already_protected {
                    false
                } else {
                    let seam = if let Some(normals) = corner_normals {
                        let mut seam = false;
                        for vertex in [lo, hi] {
                            let a = corner_normal(normals, first >> 1, vertex)
                                .ok_or_else(|| invalid("invalid corner normals"))?;
                            let b = corner_normal(normals, second >> 1, vertex)
                                .ok_or_else(|| invalid("invalid corner normals"))?;
                            seam |= !same_normal_direction(a, b);
                        }
                        seam
                    } else {
                        false
                    };
                    // An uncertain face normal must not remove a potentially sharp edge.
                    seam || normal(first >> 1)
                        .zip(normal(second >> 1))
                        .is_none_or(|(a, b)| dot(a, b) <= 0.8660254037844386)
                }
            }
            _ => return Err(invalid("non-manifold or inconsistently wound edge")),
        };
        if sharp {
            *protected
                .get_mut(lo as usize)
                .ok_or_else(|| invalid("invalid triangle indices"))? = true;
            *protected
                .get_mut(hi as usize)
                .ok_or_else(|| invalid("invalid triangle indices"))? = true;
        }
        i = end;
    }
    check_cancel(cancel)?;
    Ok(PreparedTopology {
        protected,
        retained,
        triangles,
        closed,
    })
}

#[cfg(test)]
mod tests {
    use super::{same_normal_direction, sort_records, validate, validate_with_cancel};

    const P: [[f64; 3]; 5] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.0, 0.0, 1.0],
        [-1.0, 0.0, 0.0],
    ];

    #[test]
    fn accepts_open_and_closed_manifolds_and_disconnected_vertex_links() {
        assert!(validate(&[], &[]).is_ok());
        assert!(validate(&[0, 1, 2, 1, 3, 2], &P).is_ok());
        assert!(validate(&[0, 2, 1, 0, 1, 3, 1, 2, 3, 2, 0, 3], &P).is_ok());
        assert!(validate(&[0, 1, 2, 0, 3, 4], &P).is_ok());
    }

    #[test]
    fn rejects_invalid_topology() {
        assert!(validate(&[0, 1], &P).is_err());
        assert!(validate(&[0, 1, 5], &P).is_err());
        assert!(validate(&[0, 1, 1], &P).is_err());
        assert!(validate(&[0, 1, 2, 2, 1, 0], &P).is_err());
        assert!(validate(&[0, 1, 2, 0, 1, 3], &P).is_err());
        assert!(validate(&[0, 1, 2, 1, 0, 3, 0, 1, 4], &P).is_err());
    }

    #[test]
    fn protects_only_boundary_and_sharp_edge_endpoints() {
        let indices = [
            0, 1, 4, 0, 4, 3, 1, 2, 5, 1, 5, 4, 3, 4, 7, 3, 7, 6, 4, 5, 8, 4, 8, 7,
        ];
        let mut positions = [[0.0; 3]; 9];
        for y in 0..3 {
            for x in 0..3 {
                positions[y * 3 + x] = [x as f64, y as f64, 0.0];
            }
        }
        let boundary = vec![true, true, true, true, false, true, true, true, true];
        assert_eq!(validate(&indices, &positions).unwrap(), boundary);
        for y in 0..3 {
            positions[y * 3 + 2][2] = 1.0;
        }
        assert_eq!(validate(&indices, &positions).unwrap(), vec![true; 9]);
    }

    #[test]
    fn normal_directions_ignore_magnitude_but_not_invalid_or_opposite_vectors() {
        assert!(same_normal_direction([0.0, 0.0, 1.0], [0.0, 0.0, 2.0]));
        assert!(same_normal_direction(
            [f64::MAX, f64::MAX, 0.0],
            [f64::MIN_POSITIVE, f64::MIN_POSITIVE, 0.0],
        ));
        assert!(!same_normal_direction([0.0; 3], [0.0; 3]));
        assert!(!same_normal_direction([0.0, 0.0, 1.0], [0.0, 0.0, -1.0]));
        assert!(!same_normal_direction(
            [f64::NAN, 0.0, 1.0],
            [0.0, 0.0, 1.0]
        ));
        assert!(!same_normal_direction(
            [f64::INFINITY, 0.0, 1.0],
            [1.0, 0.0, 0.0]
        ));
    }

    #[test]
    fn a_seam_at_one_edge_endpoint_protects_the_other_endpoint() {
        let positions = [
            [0.0, 0.0, 0.0],
            [1.0, 0.0, 0.0],
            [1.0, 1.0, 0.0],
            [0.0, 1.0, 0.0],
            [0.5, 0.5, 0.0],
        ];
        let indices = [0, 1, 4, 1, 2, 4, 2, 3, 4, 3, 0, 4];
        let mut normals = [[0.0, 0.0, 1.0]; 12];
        let protected = |normals: &[[f64; 3]]| {
            validate_with_cancel(&indices, &positions, Some(normals), &mut || false)
                .map(|topology| topology.protected)
        };
        assert_eq!(
            protected(&normals).unwrap(),
            vec![true, true, true, true, false]
        );
        normals[1] = [0.0, 0.0, 2.0];
        assert_eq!(
            protected(&normals).unwrap(),
            vec![true, true, true, true, false]
        );
        normals[1] = [1.0, 0.0, 0.0];
        assert_eq!(protected(&normals).unwrap(), vec![true; 5]);
        assert!(protected(&[]).is_err());
    }

    #[test]
    fn integer_record_sort_orders_ties_and_cancels_during_its_work() {
        let original: Vec<_> = (0..20_000).rev().map(|value| (value % 31, value)).collect();
        let mut expected = original.clone();
        expected.sort_unstable();
        let mut ordered = original.clone();
        sort_records(&mut ordered, &mut || false).unwrap();
        assert_eq!(ordered, expected);
        let mut cancelled = original;
        let mut checks = 0;
        let error = sort_records(&mut cancelled, &mut || {
            checks += 1;
            checks == 3
        })
        .unwrap_err();
        assert!(error.is_cancelled());
        assert_eq!(
            checks, 3,
            "cancelled sorting must not keep comparing records"
        );
    }
}
