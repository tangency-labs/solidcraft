//! The part of a face an emboss offsets (#21347): a freeform face's offset is certified piece by
//! piece and never at a pole, so an emboss offsets only the part of its face under the profiles.

/// The outline, on the sketch plane, of the box an emboss cuts its face down to: the bounds of
/// the profiles' `points`, widened by `reach`, the farthest any point of the emboss's slab lies
/// from the face. Counter-clockwise from the lowest corner; `None` for no points, a point or a
/// reach that is not finite, or a negative reach.
#[must_use]
pub fn patch_outline(points: &[[f64; 2]], reach: f64) -> Option<[[f64; 2]; 4]> {
    if !(reach.is_finite() && reach >= 0.0) || points.iter().flatten().any(|c| !c.is_finite()) {
        return None;
    }
    let (first, rest) = points.split_first()?;
    let (mut lo, mut hi) = (*first, *first);
    for p in rest {
        lo = [lo[0].min(p[0]), lo[1].min(p[1])];
        hi = [hi[0].max(p[0]), hi[1].max(p[1])];
    }
    let (lo, hi) = (
        [lo[0] - reach, lo[1] - reach],
        [hi[0] + reach, hi[1] + reach],
    );
    Some([lo, [hi[0], lo[1]], hi, [lo[0], hi[1]]])
}

/// Of the pieces of a face inside boxes from [`patch_outline`] on a sketch plane (`frame`: its
/// origin and unit axes x and y), each given by a point `inside` it, the `group` of pieces
/// joined edge to edge it belongs to and its `distance` from the pick, those an emboss raises:
/// under each box (within `tol`), the group nearest the pick. The pieces on the far side of a
/// closed body are left out. Indices into `inside`, ascending, each once; `None` where two
/// groups under a box are as near the pick, within `tol`.
#[must_use]
pub fn picked_pieces(
    frame: [[f64; 3]; 3],
    boxes: &[[[f64; 2]; 4]],
    inside: &[[f64; 3]],
    group: &[usize],
    distance: &[f64],
    tol: f64,
) -> Option<Vec<usize>> {
    let [origin, x, y] = frame;
    let dot = |a: [f64; 3], b: [f64; 3]| a[0] * b[0] + a[1] * b[1] + a[2] * b[2];
    let sub = |a: [f64; 3], b: [f64; 3]| [a[0] - b[0], a[1] - b[1], a[2] - b[2]];
    let group_distance = |g: usize| {
        (0..inside.len())
            .filter(|j| group.get(*j) == Some(&g))
            .filter_map(|j| distance.get(j).copied())
            .fold(f64::INFINITY, f64::min)
    };
    let mut out = Vec::new();
    for [lo, _, hi, _] in boxes {
        let mut under: Vec<(f64, usize)> = inside
            .iter()
            .enumerate()
            .filter(|(_, p)| {
                let (u, v) = (dot(sub(**p, origin), x), dot(sub(**p, origin), y));
                (lo[0] - tol..=hi[0] + tol).contains(&u) && (lo[1] - tol..=hi[1] + tol).contains(&v)
            })
            .filter_map(|(i, _)| group.get(i).copied())
            .map(|g| (group_distance(g), g))
            .collect();
        under.sort_by(|a, b| a.0.total_cmp(&b.0));
        under.dedup_by_key(|(_, g)| *g);
        match under.as_slice() {
            [] => {}
            [(first, _), (second, _), ..] if second - first <= tol => return None,
            [(_, g), ..] => out.extend((0..inside.len()).filter(|j| group.get(*j) == Some(g))),
        }
    }
    out.sort_unstable();
    out.dedup();
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{patch_outline, picked_pieces};

    #[test]
    fn under_each_box_the_piece_nearest_the_pick_is_kept() {
        let (o, x, y) = ([0.0, 0.0, 0.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0]);
        let boxes = [
            [[-2.0, -2.0], [2.0, -2.0], [2.0, 2.0], [-2.0, 2.0]],
            [[8.0, -2.0], [12.0, -2.0], [12.0, 2.0], [8.0, 2.0]],
        ];
        let inside = [
            [0.0, 0.0, 5.0],
            [0.0, 0.0, -5.0],
            [10.0, 0.0, -5.0],
            [30.0, 0.0, 5.0],
            [1.0, 1.0, 5.0],
        ];
        let group = [0, 1, 2, 3, 0];
        let frame = [o, x, y];
        let above = [4.0, 14.0, 14.0, 4.0, 4.0];
        let below = [14.0, 4.0, 4.0, 14.0, 14.0];
        assert_eq!(
            picked_pieces(frame, &boxes, &inside, &group, &above, 1e-6),
            Some(vec![0, 2, 4])
        );
        assert_eq!(
            picked_pieces(frame, &boxes, &inside, &group, &below, 1e-6),
            Some(vec![1, 2])
        );
        let level = [4.0, 4.0, 4.0, 4.0, 4.0];
        assert_eq!(
            picked_pieces(frame, &boxes, &inside, &group, &level, 1e-6),
            None
        );
    }

    #[test]
    fn the_outline_is_the_points_bounds_widened_by_the_reach() {
        let got = patch_outline(&[[-3.0, 8.0], [3.0, 12.0], [0.0, 10.0]], 1.5);
        assert_eq!(
            got,
            Some([[-4.5, 6.5], [4.5, 6.5], [4.5, 13.5], [-4.5, 13.5]])
        );
    }

    #[test]
    fn no_points_or_a_bad_reach_have_no_outline() {
        assert_eq!(patch_outline(&[], 1.0), None);
        assert_eq!(patch_outline(&[[0.0, 0.0]], -1.0), None);
        assert_eq!(patch_outline(&[[0.0, f64::NAN]], 1.0), None);
    }
}
