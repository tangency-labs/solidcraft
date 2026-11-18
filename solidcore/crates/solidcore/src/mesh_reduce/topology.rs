//! Input topology checks using compact, sorted records.
//!
//! Undirected edges are grouped with their incident face and direction.
//! Disconnected vertex links are accepted, as they were by the reducer's
//! original edge-based validation.

use super::{check_cancel, check_stride, dot, face_normal, MeshReductionError};

#[cfg(test)]
fn validate(indices: &[u32], positions: &[[f64; 3]]) -> Result<Vec<bool>, MeshReductionError> {
    validate_with_cancel(indices, positions, &mut || false)
}

/// Validate topology and mark endpoints of boundary and sharp source edges.
pub(super) fn validate_with_cancel(
    indices: &[u32],
    positions: &[[f64; 3]],
    cancel: &mut impl FnMut() -> bool,
) -> Result<Vec<bool>, MeshReductionError> {
    let invalid = MeshReductionError::Invalid;
    if indices.len() % 3 != 0 {
        return Err(invalid("indices are not triangle triples"));
    }

    // Keep the two sorts separate so their allocations do not overlap.
    {
        let mut faces = Vec::new();
        faces
            .try_reserve_exact(indices.len() / 3)
            .map_err(|_| invalid("mesh topology is too large"))?;
        for (i, tri) in indices.chunks_exact(3).enumerate() {
            check_stride(i, cancel)?;
            let [a, b, c] = [tri[0], tri[1], tri[2]];
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
            faces.push(face);
        }
        check_cancel(cancel)?;
        faces.sort_unstable();
        check_cancel(cancel)?;
        for (i, pair) in faces.windows(2).enumerate() {
            check_stride(i, cancel)?;
            if pair[0] == pair[1] {
                return Err(invalid("duplicate triangle"));
            }
        }
    }

    // The low bit records direction; the remaining bits are the face index.
    let mut edges = Vec::new();
    edges
        .try_reserve_exact(indices.len())
        .map_err(|_| invalid("mesh topology is too large"))?;
    for (face, tri) in indices.chunks_exact(3).enumerate() {
        check_stride(face, cancel)?;
        for (from, to) in [(tri[0], tri[1]), (tri[1], tri[2]), (tri[2], tri[0])] {
            let (lo, hi) = if from < to { (from, to) } else { (to, from) };
            let key = (u64::from(lo) << 32) | u64::from(hi);
            edges.push((key, (face << 1) | usize::from(from < to)));
        }
    }
    check_cancel(cancel)?;
    edges.sort_unstable();
    check_cancel(cancel)?;

    let mut protected = Vec::new();
    protected
        .try_reserve_exact(positions.len())
        .map_err(|_| invalid("mesh topology is too large"))?;
    protected.resize(positions.len(), false);
    let normal = |face: usize| {
        let tri = &indices[face * 3..face * 3 + 3];
        face_normal([
            positions[tri[0] as usize],
            positions[tri[1] as usize],
            positions[tri[2] as usize],
        ])
    };
    let mut i = 0;
    let mut groups = 0usize;
    while i < edges.len() {
        check_stride(groups, cancel)?;
        groups += 1;
        let (key, first) = edges[i];
        let mut end = i + 1;
        while end < edges.len() && edges[end].0 == key {
            check_stride(end, cancel)?;
            end += 1;
        }
        let sharp = match end - i {
            1 => true,
            2 => {
                let second = edges[i + 1].1;
                if first & 1 == second & 1 {
                    return Err(invalid("non-manifold or inconsistently wound edge"));
                }
                // An uncertain normal must not allow a potentially sharp edge
                // to be removed; positional validation is handled by the caller.
                if protected[(key >> 32) as usize] && protected[key as u32 as usize] {
                    false
                } else {
                    normal(first >> 1)
                        .zip(normal(second >> 1))
                        .map_or(true, |(a, b)| dot(a, b) <= 0.8660254037844386)
                }
            }
            _ => return Err(invalid("non-manifold or inconsistently wound edge")),
        };
        if sharp {
            protected[(key >> 32) as usize] = true;
            protected[key as u32 as usize] = true;
        }
        i = end;
    }
    check_cancel(cancel)?;
    Ok(protected)
}

#[cfg(test)]
mod tests {
    use super::validate;

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
}
