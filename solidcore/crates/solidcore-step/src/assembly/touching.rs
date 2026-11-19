//! Loops of one face that touch at a vertex (a hole whose rim touches the face's outer edge at
//! one point, two holes that touch each other). Some systems write each as a bound of its own.
//! ISO 10303-42 has distinct bounds of a face share no vertex (`face`, informal proposition 2),
//! and an edge loop may overlap itself (`edge_loop`): its form of such a face is one loop through
//! the vertex twice. The kernel's is the same: it takes no face whose loops share a vertex (its
//! face tests 32, 38 and 39, `docs/validity-and-tolerances.md` §4), and takes the one loop
//! through the vertex twice.
//!
//! Where two loops each pass once through a vertex, and no other loop does, the face lies in two
//! corners there: between the first loop's way in and the second's way out, and between the
//! second's way in and the first's way out (each loop runs with the face on its left, so the
//! second loop lies in the corner the first one turns through). The one loop that bounds the
//! same region runs along the first loop to the vertex, round the second and on along the
//! first. That is decided by the uses alone: no direction is measured, so a rim tangent to the
//! edge it touches (the usual case) is read as any other.
//!
//! Loops joined this way stay one connected boundary only while they touch as a tree: two loops
//! that touch at two vertices, or a ring of loops each touching the next, cut the face in two,
//! which is two faces and not one loop. And where three or more passes meet at one vertex,
//! which way in goes with which way out depends on how the loops lie round the vertex. Such
//! loops are left as written, and the reader says where ([`Joins::left`]).

use std::collections::BTreeMap;

/// What [`join`] makes of a face's edge loops.
#[derive(Debug, Default, PartialEq, Eq)]
pub(super) struct Joins {
    /// Each loop made of several: the loop that stays (the face's outer loop where it is one of
    /// them, else the first of them) and its uses in order, each the loop it was written in and
    /// its position there.
    pub(super) walks: Vec<(usize, Vec<(usize, usize)>)>,
    /// Per loop, whether it is now part of another.
    pub(super) gone: Vec<bool>,
    /// The vertices at which loops touch and are left as written, in ascending order.
    pub(super) left: Vec<u32>,
}

/// The root of `l`'s set, halving the path to it.
fn find(parent: &mut [usize], mut l: usize) -> usize {
    loop {
        let Some(&p) = parent.get(l) else { return l };
        if p == l {
            return l;
        }
        let up = parent.get(p).copied().unwrap_or(p);
        if let Some(slot) = parent.get_mut(l) {
            *slot = up;
        }
        l = up;
    }
}

/// The face's edge loops that touch at one vertex, joined into one loop through it (see the
/// module's notes). `starts` gives, per loop, the vertex each of its uses starts at; `outer` is
/// the face's outer loop; `sound` says, per loop, that it runs the way its place in the face has
/// it run (the outer loop counter-clockwise, the others clockwise): only such loops are joined;
/// `same_place` says that two uses that start at one vertex start at one place of the face's
/// (u, v) too (not a period apart).
pub(super) fn join(starts: &[Vec<u32>], outer: Option<usize>, sound: &[bool], same_place: &dyn Fn((usize, usize), (usize, usize)) -> bool) -> Joins {
    let count = starts.len();
    let mut joins = Joins { walks: Vec::new(), gone: vec![false; count], left: Vec::new() };
    // Each vertex's passes: the uses that start at it.
    let mut passes: BTreeMap<u32, Vec<(usize, usize)>> = BTreeMap::new();
    for (l, vs) in starts.iter().enumerate() {
        for (k, &v) in vs.iter().enumerate() {
            passes.entry(v).or_default().push((l, k));
        }
    }
    // The loops that touch, as sets; a set with a touch that cannot be joined is left whole.
    let mut parent: Vec<usize> = (0..count).collect();
    let mut blocked = vec![false; count];
    let mut partner: BTreeMap<(usize, usize), (usize, usize)> = BTreeMap::new();
    let mut touches: Vec<(u32, usize)> = Vec::new();
    for (&v, at) in &passes {
        let Some(&(first, _)) = at.first() else { continue };
        if at.iter().all(|&(l, _)| l == first) {
            continue;
        }
        touches.push((v, first));
        let is_sound = |l: usize| sound.get(l).copied().unwrap_or(false);
        let pair = match at.as_slice() {
            &[a, b] if is_sound(a.0) && is_sound(b.0) && same_place(a, b) => Some((a, b)),
            _ => None,
        };
        // A touch between two loops already joined another way closes a ring of loops.
        let ring = pair.is_some_and(|(a, b)| find(&mut parent, a.0) == find(&mut parent, b.0));
        let root = find(&mut parent, first);
        for &(l, _) in at {
            let other = find(&mut parent, l);
            if other != root {
                let was = blocked.get(other).copied().unwrap_or(false);
                if let Some(slot) = parent.get_mut(other) {
                    *slot = root;
                }
                if was && let Some(slot) = blocked.get_mut(root) {
                    *slot = true;
                }
            }
        }
        match pair {
            Some((a, b)) if !ring => {
                partner.insert(a, b);
                partner.insert(b, a);
            }
            _ => {
                if let Some(slot) = blocked.get_mut(root) {
                    *slot = true;
                }
            }
        }
    }
    if touches.is_empty() {
        return joins;
    }
    for (v, l) in touches {
        let root = find(&mut parent, l);
        if blocked.get(root).copied().unwrap_or(false) {
            joins.left.push(v);
        }
    }
    // Each set that can be joined, walked from the loop that stays: along a loop, and at a use
    // whose start another loop of the set touches, round that loop first (and round the loops
    // that one touches, likewise) before the use itself.
    let mut stays: BTreeMap<usize, usize> = BTreeMap::new();
    for l in 0..count {
        let root = find(&mut parent, l);
        if blocked.get(root).copied().unwrap_or(false) {
            continue;
        }
        let stay = stays.entry(root).or_insert(l);
        if outer == Some(l) {
            *stay = l;
        }
    }
    let mut visited = vec![false; count];
    for &stay in stays.values() {
        let mut walk: Vec<(usize, usize)> = Vec::new();
        // (loop, the position its walk starts at, how many of its uses are walked).
        let mut stack: Vec<(usize, usize, usize)> = vec![(stay, 0, 0)];
        if let Some(slot) = visited.get_mut(stay) {
            *slot = true;
        }
        while let Some((l, from, mut done)) = stack.pop() {
            let n = starts.get(l).map_or(0, Vec::len);
            while done < n {
                // Round the loop from `from`: `from` and `done` are both under `n`.
                let round = from + done;
                let at = (l, if round < n { round } else { round - n });
                if let Some(&(other, position)) = partner.get(&at)
                    && let Some(seen) = visited.get_mut(other)
                    && !*seen
                {
                    *seen = true;
                    stack.push((l, from, done));
                    stack.push((other, position, 0));
                    break;
                }
                walk.push(at);
                done += 1;
            }
        }
        if walk.iter().any(|&(l, _)| l != stay) {
            for &(l, _) in &walk {
                if l != stay
                    && let Some(slot) = joins.gone.get_mut(l)
                {
                    *slot = true;
                }
            }
            joins.walks.push((stay, walk));
        }
    }
    joins
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all(_: (usize, usize), _: (usize, usize)) -> bool {
        true
    }

    /// The vertices a walk passes, in order.
    fn route(starts: &[Vec<u32>], walk: &[(usize, usize)]) -> Vec<u32> {
        walk.iter().map(|&(l, k)| starts[l][k]).collect()
    }

    /// A hole that touches the outer loop at one vertex is walked between the two outer uses
    /// that meet there, from its use that starts at the vertex.
    #[test]
    fn a_hole_touching_the_outer_loop_is_walked_at_the_vertex() {
        let starts = vec![vec![1, 2, 3, 4], vec![7, 3, 8]];
        let joins = join(&starts, Some(0), &[true, true], &all);
        assert_eq!(joins.walks, vec![(0, vec![(0, 0), (0, 1), (1, 1), (1, 2), (1, 0), (0, 2), (0, 3)])]);
        assert_eq!(route(&starts, &joins.walks[0].1), [1, 2, 3, 8, 7, 3, 4]);
        assert_eq!(joins.gone, [false, true]);
        assert!(joins.left.is_empty());
        // The outer loop stays whichever is written first, and loops apart stay as written.
        let starts = vec![vec![7, 3, 8], vec![20, 21], vec![1, 2, 3, 4]];
        let joins = join(&starts, Some(2), &[true; 3], &all);
        assert_eq!(joins.walks, vec![(2, vec![(2, 0), (2, 1), (0, 1), (0, 2), (0, 0), (2, 2), (2, 3)])]);
        assert_eq!(joins.gone, [true, false, false]);
    }

    /// Loops that touch as a chain (the outer loop, a hole on it, a hole on that hole) are one
    /// loop; two holes that touch away from the outer loop are one inner loop, the first.
    #[test]
    fn loops_that_touch_as_a_tree_are_one_loop() {
        let starts = vec![vec![1, 2, 3], vec![2, 5, 6], vec![6, 9]];
        let joins = join(&starts, Some(0), &[true; 3], &all);
        assert_eq!(joins.walks.len(), 1);
        assert_eq!(route(&starts, &joins.walks[0].1), [1, 2, 5, 6, 9, 6, 2, 3]);
        assert_eq!(joins.gone, [false, true, true]);
        let starts = vec![vec![1, 2, 3], vec![10, 11], vec![11, 12]];
        let joins = join(&starts, Some(0), &[true; 3], &all);
        assert_eq!(joins.walks, vec![(1, vec![(1, 0), (2, 0), (2, 1), (1, 1)])]);
        assert_eq!(joins.gone, [false, false, true]);
    }

    /// What is not one loop is left as written, with the vertices it touches at: two loops that
    /// touch at two vertices, a ring of three loops, three passes at one vertex, a loop that
    /// runs the wrong way, and two uses a period apart; a loop joined to any of them too.
    #[test]
    fn loops_that_cut_the_face_or_meet_three_at_a_vertex_are_left() {
        let none = |joins: &Joins, left: &[u32]| {
            assert!(joins.walks.is_empty(), "{joins:?}");
            assert!(joins.gone.iter().all(|g| !g), "{joins:?}");
            assert_eq!(joins.left, left);
        };
        none(&join(&[vec![1, 2, 3, 4], vec![2, 9, 4]], Some(0), &[true; 2], &all), &[2, 4]);
        none(&join(&[vec![1, 2, 3], vec![2, 5, 6], vec![6, 3]], Some(0), &[true; 3], &all), &[2, 3, 6]);
        none(&join(&[vec![1, 2, 3], vec![2, 5], vec![2, 6]], Some(0), &[true; 3], &all), &[2]);
        none(&join(&[vec![1, 2, 3], vec![2, 5], vec![5, 6]], Some(0), &[true, true, false], &all), &[2, 5]);
        none(&join(&[vec![1, 2, 3], vec![2, 5]], Some(0), &[true; 2], &|_, _| false), &[2]);
        // A loop through one of its own vertices twice (a seam's ends) touches no other loop.
        none(&join(&[vec![1, 2, 1, 3], vec![7, 8]], Some(0), &[true; 2], &all), &[]);
        // Beside loops left as written, others are still joined.
        let joins = join(&[vec![1, 2, 3, 4], vec![2, 9, 4], vec![20, 21], vec![21, 22]], Some(0), &[true; 4], &all);
        assert_eq!(joins.walks, vec![(2, vec![(2, 0), (3, 0), (3, 1), (2, 1)])]);
        assert_eq!(joins.left, [2, 4]);
    }
}
