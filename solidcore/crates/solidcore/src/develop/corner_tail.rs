//! Contour edits past the end of a shared flange corner.

use super::plane::Planar;
use crate::math::Vec2;
use crate::prism::profile::{Loop2, Region2, Seg2};

/// Sets a straight side back by `by` after height `h`, preserving holes.
/// Returns `None` if the step cannot meet the next straight edge without reversing it.
pub fn set_back_past(
    region: &Region2,
    from: Vec2,
    m: Vec2,
    rise: f64,
    h: f64,
    into: Vec2,
    by: f64,
) -> Option<Region2> {
    let segs = &region.outer.segs;
    let n = segs.len();
    let top = from + m * rise;
    let tol = rise.max(1.0) * 1e-9;
    let same = |p: Vec2, q: Vec2| p.dist(q) <= tol;
    let (k, side) = segs
        .iter()
        .enumerate()
        .find(|(_, g)| matches!(**g, Seg2::Line { a, b } if (same(a, from) && same(b, top)) || (same(a, top) && same(b, from))))?;
    let up = matches!(*side, Seg2::Line { a, .. } if same(a, from));
    let past_at = if up {
        k.checked_add(1).filter(|next| *next < n).unwrap_or(0)
    } else {
        previous(k, n).unwrap_or(0)
    };
    let Seg2::Line { a: pa, b: pb } = *segs.get(past_at)? else {
        return None;
    };
    let (p, q) = (from + m * h, from + m * h + into * by);
    let w = pb - pa;
    let cross = m.cross(w);
    if cross.abs() <= 1e-12 {
        return None;
    }
    let ahead = (pa - q).cross(w) / cross;
    let mut pts = if ahead > 0.0 {
        vec![from, p, q, q + m * ahead]
    } else {
        let across = into.cross(w);
        if across.abs() <= 1e-12 {
            return None;
        }
        vec![from, p, p + into * ((pa - p).cross(w) / across)]
    };
    let end = *pts.last()?;
    let kept = if up { pb - end } else { end - pa };
    if !(kept.dot(w) > 0.0) {
        return None;
    }
    if !up {
        pts.reverse();
    }
    let side: Vec<Seg2> = pts
        .windows(2)
        .filter_map(|pair| {
            let [a, b] = pair else { return None };
            Some(Seg2::Line { a: *a, b: *b })
        })
        .collect();
    let mut out = segs.clone();
    *out.get_mut(past_at)? = if up {
        Seg2::Line { a: end, b: pb }
    } else {
        Seg2::Line { a: pa, b: end }
    };
    out.splice(k..=k, side);
    Some(Region2 {
        outer: Loop2 { segs: out },
        holes: region.holes.clone(),
    })
}

fn previous(index: usize, count: usize) -> Option<usize> {
    index.checked_sub(1).or_else(|| count.checked_sub(1))
}

/// Restores a previous corner setback and applies the new setback to the same side.
///
/// # Errors
/// The contour cannot be edited unambiguously or with straight edges.
pub fn retarget_corner_tail(
    region: &Region2,
    edge: [Vec2; 2],
    anchor_origin: Vec2,
    anchor_direction: Vec2,
    bend_length: f64,
    old_cut: f64,
    new_cut: f64,
) -> Result<Region2, &'static str> {
    let [start, end] = edge;
    let distance = |point: Vec2| anchor_direction.cross(point - anchor_origin).abs();
    let (from, end) = if distance(start) <= distance(end) {
        (start, end)
    } else {
        (end, start)
    };
    let direction = (end - from)
        .normalized()
        .ok_or("the corner has no length")?;
    let along = if (from - anchor_origin).dot(anchor_direction) < bend_length / 2.0 {
        anchor_direction
    } else {
        -anchor_direction
    };
    let into = (along - direction * along.dot(direction))
        .normalized()
        .ok_or("the corner side runs along its bend")?;
    let rise_axis = if along.perp().dot(direction) > 0.0 {
        along.perp()
    } else {
        -along.perp()
    };
    replace_setback(region, from, end, into, rise_axis, old_cut, new_cut)
}

fn replace_setback(
    region: &Region2,
    from: Vec2,
    end: Vec2,
    into: Vec2,
    rise_axis: Vec2,
    old_cut: f64,
    new_cut: f64,
) -> Result<Region2, &'static str> {
    let height = from.dist(end);
    let direction = (end - from)
        .normalized()
        .ok_or("the corner has no length")?;
    let tolerance = height.max(1.0) * 1e-9;
    let same = |first: Vec2, second: Vec2| first.dist(second) <= tolerance;
    let old_end = end + into * old_cut;
    let mut restored = region.clone();
    if old_cut.abs() > 1e-12 {
        let mut steps = region
            .outer
            .segs
            .iter()
            .enumerate()
            .filter_map(|(index, segment)| match *segment {
                Seg2::Line { a, b } if same(a, end) && same(b, old_end) => Some((index, true)),
                Seg2::Line { a, b } if same(a, old_end) && same(b, end) => Some((index, false)),
                _ => None,
            });
        if let Some((step, forward)) = steps.next() {
            if steps.next().is_some() || region.outer.segs.len() < 4 {
                return Err("the corner's previous side trim is ambiguous");
            }
            let count = region.outer.segs.len();
            let start = previous(step, count).ok_or("the corner has no contour")?;
            let mut segments: Vec<_> = region
                .outer
                .segs
                .iter()
                .cycle()
                .skip(start)
                .take(count)
                .copied()
                .collect();
            let (
                Some(Seg2::Line {
                    a: first_start,
                    b: first_end,
                }),
                Some(Seg2::Line {
                    a: last_start,
                    b: last_end,
                }),
            ) = (segments.first().copied(), segments.get(2).copied())
            else {
                return Err("the corner's previous side trim was modified");
            };
            let (top, connected) = if forward {
                (
                    last_end,
                    same(first_start, from) && same(first_end, end) && same(last_start, old_end),
                )
            } else {
                (
                    first_start,
                    same(first_end, old_end) && same(last_start, end) && same(last_end, from),
                )
            };
            let past = if forward { 3 } else { count - 1 };
            let Some(Seg2::Line {
                a: past_start,
                b: past_end,
            }) = segments.get(past).copied()
            else {
                return Err("the edge past the corner's previous trim is not straight");
            };
            if !connected
                || !same(if forward { past_start } else { past_end }, top)
                || direction.cross(top - old_end).abs() > tolerance
                || (top - old_end).dot(direction) <= tolerance
            {
                return Err("the corner's previous side trim was modified");
            }
            let past_dir = past_end - past_start;
            let cross = direction.cross(past_dir);
            let original_top = (cross.abs() > 1e-12)
                .then(|| from + direction * ((past_start - from).cross(past_dir) / cross))
                .ok_or("the edge past the corner runs along its side")?;
            if (original_top - from).dot(direction) <= height + tolerance {
                return Err(
                    "the corner's previous side trim no longer reaches past the other flange",
                );
            }
            if let Some(segment) = segments.get_mut(past) {
                *segment = if forward {
                    Seg2::Line {
                        a: original_top,
                        b: past_end,
                    }
                } else {
                    Seg2::Line {
                        a: past_start,
                        b: original_top,
                    }
                };
            }
            let side = if forward {
                Seg2::Line {
                    a: from,
                    b: original_top,
                }
            } else {
                Seg2::Line {
                    a: original_top,
                    b: from,
                }
            };
            segments.splice(0..3, [side]);
            restored.outer.segs = segments;
        }
    }
    let mut sides = restored.outer.segs.iter().filter_map(|segment| {
        let Seg2::Line { a, b } = *segment else {
            return None;
        };
        let top = if same(a, from) {
            b
        } else if same(b, from) {
            a
        } else {
            return None;
        };
        let rise = (top - from).dot(direction);
        (rise > height + tolerance && direction.cross(top - from).abs() <= tolerance)
            .then_some(rise)
    });
    let Some(rise) = sides.next() else {
        if restored.outer.segs.iter().any(|segment| {
            let start = segment.start();
            let finish = segment.end();
            [end, old_end].into_iter().any(|tip| {
                (same(start, tip) && (finish - tip).dot(rise_axis) > tolerance)
                    || (same(finish, tip) && (start - tip).dot(rise_axis) > tolerance)
            })
        }) {
            return Err("the corner's taller side was modified; its trim cannot be changed");
        }
        return Ok(restored);
    };
    if sides.next().is_some() {
        return Err("the corner's taller side is ambiguous");
    }
    if new_cut.abs() <= 1e-12 {
        return Ok(restored);
    }
    set_back_past(&restored, from, direction, rise, height, into, new_cut)
        .ok_or("the edge past the corner's taller side is not straight")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn region(points: &[[f64; 2]]) -> Region2 {
        let segs = points
            .iter()
            .zip(points.iter().cycle().skip(1))
            .map(|(&[ax, ay], &[bx, by])| Seg2::Line {
                a: Vec2::new(ax, ay),
                b: Vec2::new(bx, by),
            })
            .collect();
        Region2 {
            outer: Loop2 { segs },
            holes: Vec::new(),
        }
    }

    #[test]
    fn corner_tail_ignores_unrelated_high_material_but_refuses_a_modified_continuation() {
        let from = Vec2::new(0.0, 0.0);
        let end = Vec2::new(0.0, 2.0);
        let run = |region: &Region2| {
            retarget_corner_tail(
                region,
                [from, end],
                from,
                Vec2::new(1.0, 0.0),
                10.0,
                0.5,
                1.0,
            )
        };
        for (points, refused) in [
            (
                [[0.0, 0.0], [0.0, 2.0], [3.0, 2.0], [3.0, 4.0], [6.0, 0.0]],
                false,
            ),
            (
                [[0.0, 0.0], [0.0, 2.0], [0.5, 2.25], [0.5, 4.0], [6.0, 0.0]],
                true,
            ),
        ] {
            let mut panel = region(&points);
            assert_eq!(run(&panel).is_err(), refused);
            panel.outer = panel.outer.reversed();
            assert_eq!(run(&panel).is_err(), refused);
        }
    }
}
