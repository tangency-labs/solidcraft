//! How far a point lies from a triangle mesh, for measuring a level's error: a box hierarchy over
//! the original triangles and the closest point of a triangle. Display only and private to
//! [`super`]: a level's error says how coarse it is drawn, never anything a model depends on.

use std::cmp::Ordering;

use crate::vec::VecExt as _;
use solidcore_record::Vec3;

/// Most items in a leaf.
const LEAF: usize = 4;

/// An axis-aligned box.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(super) struct Aabb3 {
    pub(super) min: Vec3,
    pub(super) max: Vec3,
}

impl Aabb3 {
    pub(super) const EMPTY: Aabb3 = Aabb3 {
        min: Vec3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        max: Vec3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    };
    fn is_empty(&self) -> bool {
        !(self.min.x <= self.max.x && self.min.y <= self.max.y && self.min.z <= self.max.z)
    }
    pub(super) fn add(&mut self, p: Vec3) {
        if p.is_finite() {
            self.min = self.min.min(p);
            self.max = self.max.max(p);
        }
    }
    fn union(&self, o: &Aabb3) -> Aabb3 {
        if self.is_empty() {
            return *o;
        }
        if o.is_empty() {
            return *self;
        }
        Aabb3 {
            min: self.min.min(o.min),
            max: self.max.max(o.max),
        }
    }
    fn center(&self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            (self.min + self.max) * 0.5
        }
    }
    fn size(&self) -> Vec3 {
        if self.is_empty() {
            Vec3::ZERO
        } else {
            self.max - self.min
        }
    }
}

#[derive(Clone, Copy, Debug)]
struct Node {
    bounds: Aabb3,
    /// A leaf: the first of its `count` items in [`Bvh::order`]. Otherwise its children are
    /// `first` and `first + 1`.
    first: u32,
    count: u32,
}

/// A hierarchy over boxes; items are the boxes' indices.
#[derive(Clone, Debug, Default)]
pub(super) struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
}

impl Bvh {
    /// The hierarchy over `boxes` (empty boxes are left out).
    pub(super) fn new(boxes: &[Aabb3]) -> Bvh {
        let mut order: Vec<u32> = (0..boxes.len().min(u32::MAX as usize))
            .filter(|&i| boxes.get(i).is_some_and(|b| !b.is_empty() && finite(b)))
            // Proven: i < u32::MAX, the range being cut there.
            .map(|i| i as u32)
            .collect();
        if order.is_empty() {
            return Bvh::default();
        }
        let item = |i: u32| boxes.get(i as usize).copied().unwrap_or(Aabb3::EMPTY);
        let centre = |i: u32| item(i).center();
        let mut nodes = vec![Node {
            bounds: Aabb3::EMPTY,
            first: 0,
            count: 0,
        }];
        // (node, start, end) in `order` still to be filled in.
        let mut todo = vec![(0usize, 0usize, order.len())];
        while let Some((n, start, end)) = todo.pop() {
            let Some(span) = order.get_mut(start..end) else {
                continue;
            };
            let mut bounds = Aabb3::EMPTY;
            let mut centres = Aabb3::EMPTY;
            for &i in span.iter() {
                bounds = bounds.union(&item(i));
                centres.add(centre(i));
            }
            let size = centres.size();
            // The coordinate along the longest side.
            let axis: fn(Vec3) -> f64 = if size.x >= size.y && size.x >= size.z {
                |v| v.x
            } else if size.y >= size.z {
                |v| v.y
            } else {
                |v| v.z
            };
            let extent = axis(size);
            if span.len() <= LEAF || extent.partial_cmp(&0.0) != Some(Ordering::Greater) {
                if let Some(node) = nodes.get_mut(n) {
                    // Proven: start and the span's length are at most order.len(), which is at
                    // most u32::MAX (its range is cut there).
                    *node = Node {
                        bounds,
                        first: start as u32,
                        count: span.len() as u32,
                    };
                }
                continue;
            }
            let key = |i: &u32| axis(centre(*i));
            // Proven: the divisor is the constant 2, not zero.
            let mid = span.len() / 2;
            span.select_nth_unstable_by(mid, |a, b| key(a).total_cmp(&key(b)));
            let left = nodes.len();
            nodes.push(Node {
                bounds: Aabb3::EMPTY,
                first: 0,
                count: 0,
            });
            nodes.push(Node {
                bounds: Aabb3::EMPTY,
                first: 0,
                count: 0,
            });
            if let Some(node) = nodes.get_mut(n) {
                // Proven: nodes split spans longer than LEAF (4) in two, so there are fewer
                // than order.len() of them, at most u32::MAX.
                *node = Node {
                    bounds,
                    first: left as u32,
                    count: 0,
                };
            }
            todo.push((left, start, start + mid));
            todo.push((left + 1, start + mid, end));
        }
        Bvh { nodes, order }
    }

    /// Calls `item` with every item in a leaf whose box, and every box above it, `enter` accepts.
    pub(super) fn visit(&self, mut enter: impl FnMut(&Aabb3) -> bool, mut item: impl FnMut(u32)) {
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n) else {
                continue;
            };
            if !enter(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                let (a, b) = (
                    node.first as usize,
                    node.first as usize + node.count as usize,
                );
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    item(i);
                }
            } else {
                stack.push(node.first as usize + 1);
                stack.push(node.first as usize);
            }
        }
    }
}

fn finite(b: &Aabb3) -> bool {
    [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z]
        .iter()
        .all(|v| v.is_finite())
}

/// The point of triangle `abc` nearest `p` (Ericson, Real-Time Collision Detection, 5.1.5).
pub(super) fn closest_on_triangle(p: Vec3, a: Vec3, b: Vec3, c: Vec3) -> Vec3 {
    let (ab, ac, ap) = (b - a, c - a, p - a);
    let (d1, d2) = (ab.dot(ap), ac.dot(ap));
    if d1 <= 0.0 && d2 <= 0.0 {
        return a;
    }
    let bp = p - b;
    let (d3, d4) = (ab.dot(bp), ac.dot(bp));
    if d3 >= 0.0 && d4 <= d3 {
        return b;
    }
    let vc = d1 * d4 - d3 * d2;
    if vc <= 0.0 && d1 >= 0.0 && d3 <= 0.0 {
        let den = d1 - d3;
        return if den > 0.0 { a + ab * (d1 / den) } else { a };
    }
    let cp = p - c;
    let (d5, d6) = (ab.dot(cp), ac.dot(cp));
    if d6 >= 0.0 && d5 <= d6 {
        return c;
    }
    let vb = d5 * d2 - d1 * d6;
    if vb <= 0.0 && d2 >= 0.0 && d6 <= 0.0 {
        let den = d2 - d6;
        return if den > 0.0 { a + ac * (d2 / den) } else { a };
    }
    let va = d3 * d6 - d5 * d4;
    if va <= 0.0 && (d4 - d3) >= 0.0 && (d5 - d6) >= 0.0 {
        let den = (d4 - d3) + (d5 - d6);
        return if den > 0.0 {
            b + (c - b) * ((d4 - d3) / den)
        } else {
            b
        };
    }
    let den = va + vb + vc;
    if den.abs() < 1e-300 {
        return a;
    }
    a + ab * (vb / den) + ac * (vc / den)
}
