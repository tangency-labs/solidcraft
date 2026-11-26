//! A bounding volume hierarchy over boxes: the nearest hit along a ray, or every item whose box
//! passes a test, without looking at each item. Built once (median split on the widest axis of
//! the box centres, a few items per leaf) and only read afterwards. [`MeshIndex`] holds one over
//! a mesh's triangles and one over its edge segments, for picking.

use crate::prelude::*;
use crate::{Aabb3, Mesh};
use serde::{Deserialize, Serialize};
use solidcore_record::Vec3;

/// Most items in a leaf.
const LEAF: usize = 4;

#[derive(Clone, Copy, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct Node {
    bounds: Aabb3,
    /// A leaf: the first of its `count` items in [`Bvh::order`]. Otherwise its children are
    /// `first` and `first + 1`.
    first: u32,
    count: u32,
}

/// A hierarchy over boxes; items are the boxes' indices.
#[derive(Clone, Debug, Default)]
pub struct Bvh {
    nodes: Vec<Node>,
    order: Vec<u32>,
}

impl Bvh {
    /// The hierarchy over `boxes` (empty boxes are left out).
    pub fn new(boxes: &[Aabb3]) -> Bvh {
        let mut order: Vec<u32> =
            (0..boxes.len().min(u32::MAX as usize)).filter(|&i| boxes.get(i).is_some_and(|b| !b.is_empty() && finite(b))).map(|i| i as u32).collect();
        if order.is_empty() {
            return Bvh::default();
        }
        let item = |i: u32| boxes.get(i as usize).copied().unwrap_or(Aabb3::EMPTY);
        let centre = |i: u32| item(i).center();
        let mut nodes = vec![Node { bounds: Aabb3::EMPTY, first: 0, count: 0 }];
        // (node, start, end) in `order` still to be filled in.
        let mut todo = vec![(0usize, 0usize, order.len())];
        while let Some((n, start, end)) = todo.pop() {
            let Some(span) = order.get_mut(start..end) else { continue };
            let mut bounds = Aabb3::EMPTY;
            let mut centres = Aabb3::EMPTY;
            for &i in span.iter() {
                bounds = bounds.union(&item(i));
                centres.add(centre(i));
            }
            let size = centres.size();
            let axis = if size.x >= size.y && size.x >= size.z {
                0
            } else if size.y >= size.z {
                1
            } else {
                2
            };
            let extent = [size.x, size.y, size.z][axis];
            if span.len() <= LEAF || !(extent > 0.0) {
                if let Some(node) = nodes.get_mut(n) {
                    *node = Node { bounds, first: start as u32, count: span.len() as u32 };
                }
                continue;
            }
            let key = |i: &u32| {
                let c = centre(*i);
                [c.x, c.y, c.z][axis]
            };
            let mid = span.len() / 2;
            span.select_nth_unstable_by(mid, |a, b| key(a).total_cmp(&key(b)));
            let left = nodes.len();
            nodes.push(Node { bounds: Aabb3::EMPTY, first: 0, count: 0 });
            nodes.push(Node { bounds: Aabb3::EMPTY, first: 0, count: 0 });
            if let Some(node) = nodes.get_mut(n) {
                *node = Node { bounds, first: left as u32, count: 0 };
            }
            todo.push((left, start, start + mid));
            todo.push((left + 1, start + mid, end));
        }
        Bvh { nodes, order }
    }

    /// The box around everything (empty for no items).
    pub fn bounds(&self) -> Aabb3 {
        self.nodes.first().map_or(Aabb3::EMPTY, |n| n.bounds)
    }

    /// Calls `item` with every item in a leaf whose box, and every box above it, `enter` accepts.
    pub fn visit(&self, mut enter: impl FnMut(&Aabb3) -> bool, mut item: impl FnMut(u32)) {
        let mut stack = vec![0usize];
        while let Some(n) = stack.pop() {
            let Some(node) = self.nodes.get(n) else { continue };
            if !enter(&node.bounds) {
                continue;
            }
            if node.count > 0 {
                let (a, b) = (node.first as usize, node.first as usize + node.count as usize);
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    item(i);
                }
            } else {
                stack.push(node.first as usize + 1);
                stack.push(node.first as usize);
            }
        }
    }

    /// The nearest item along the ray from `o` along `d`: `hit` gives an item's distance along
    /// `d` (`None`: missed). Equal distances go to the lowest item, as a scan in item order
    /// keeping strictly nearer hits would answer. Boxes are entered nearest first, and one that
    /// starts past the best hit so far is skipped.
    pub fn nearest(&self, o: Vec3, d: Vec3, mut hit: impl FnMut(u32) -> Option<f64>) -> Option<(f64, u32)> {
        if ![o.x, o.y, o.z, d.x, d.y, d.z].iter().all(|v| v.is_finite()) {
            return None;
        }
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let mut best: Option<(f64, u32)> = None;
        let root = self.nodes.first().and_then(|n| ray_box(o, d, inv, &n.bounds, 0.0))?;
        let mut stack = vec![(0usize, root)];
        while let Some((n, t0)) = stack.pop() {
            if best.is_some_and(|(bt, _)| t0 > bt) {
                continue;
            }
            let Some(node) = self.nodes.get(n) else { continue };
            if node.count > 0 {
                let (a, b) = (node.first as usize, node.first as usize + node.count as usize);
                for &i in self.order.get(a..b).unwrap_or(&[]) {
                    if let Some(t) = hit(i)
                        && best.is_none_or(|(bt, bi)| t < bt || (t == bt && i < bi))
                    {
                        best = Some((t, i));
                    }
                }
                continue;
            }
            let (l, r) = (node.first as usize, node.first as usize + 1);
            let tl = self.nodes.get(l).and_then(|c| ray_box(o, d, inv, &c.bounds, 0.0));
            let tr = self.nodes.get(r).and_then(|c| ray_box(o, d, inv, &c.bounds, 0.0));
            // The nearer child is popped first.
            match (tl, tr) {
                (Some(a), Some(b)) if a <= b => stack.extend([(r, b), (l, a)]),
                (Some(a), Some(b)) => stack.extend([(l, a), (r, b)]),
                (Some(a), None) => stack.push((l, a)),
                (None, Some(b)) => stack.push((r, b)),
                (None, None) => {}
            }
        }
        best
    }

    /// The same traversal as nearest, charging every box examination and item
    /// test before doing it. A discovered hit is not returned when work remains
    /// and the caller's shared budget is exhausted.
    fn nearest_bounded(
        &self,
        o: Vec3,
        d: Vec3,
        remaining: &mut usize,
        mut hit: impl FnMut(u32) -> Option<f64>,
    ) -> Result<Option<(f64, u32)>, &'static str> {
        if ![o.x, o.y, o.z, d.x, d.y, d.z].iter().all(|value| value.is_finite()) {
            return Ok(None);
        }
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let test_box = |index: usize, remaining: &mut usize| -> Result<Option<f64>, &'static str> {
            let Some(node) = self.nodes.get(index) else { return Ok(None) };
            ray_work(remaining)?;
            Ok(ray_box(o, d, inv, &node.bounds, 0.0))
        };
        let Some(root) = test_box(0, remaining)? else { return Ok(None) };
        let mut best: Option<(f64, u32)> = None;
        let mut stack = vec![(0usize, root)];
        while let Some((index, distance)) = stack.pop() {
            if best.is_some_and(|(best, _)| distance > best) {
                continue;
            }
            // This node's box examination already paid for this visit, even
            // when an earlier sibling subsequently made its distance obsolete.
            let Some(node) = self.nodes.get(index) else { continue };
            if node.count > 0 {
                let start = node.first as usize;
                let end = start.checked_add(node.count as usize).ok_or("bounded raycast leaf index overflowed")?;
                for &item in self.order.get(start..end).unwrap_or(&[]) {
                    ray_work(remaining)?;
                    if let Some(distance) = hit(item)
                        && best.is_none_or(|(best, first)| distance < best || (distance == best && item < first))
                    {
                        best = Some((distance, item));
                    }
                }
                continue;
            }
            let left = node.first as usize;
            let right = left.checked_add(1).ok_or("bounded raycast child index overflowed")?;
            let a = test_box(left, remaining)?;
            let b = test_box(right, remaining)?;
            match (a, b) {
                (Some(a), Some(b)) if a <= b => stack.extend([(right, b), (left, a)]),
                (Some(a), Some(b)) => stack.extend([(left, a), (right, b)]),
                (Some(a), None) => stack.push((left, a)),
                (None, Some(b)) => stack.push((right, b)),
                (None, None) => {}
            }
        }
        Ok(best)
    }

    /// Calls `item(i, j)` for pairs of items, `i` from this hierarchy and `j` from `other`, in
    /// leaves whose boxes, and every pair of boxes above them, `enter` accepts. `enter` is asked
    /// again for a pair of boxes when it is taken up, so a bound that tightens while items are
    /// visited (the nearest pair so far) prunes what is still waiting. Of two children, the pair
    /// whose box centres are nearer is taken up first.
    pub fn visit_pairs(&self, other: &Bvh, mut enter: impl FnMut(&Aabb3, &Aabb3) -> bool, mut item: impl FnMut(u32, u32)) {
        if self.nodes.is_empty() || other.nodes.is_empty() {
            return;
        }
        let mut stack = vec![(0usize, 0usize)];
        while let Some((i, j)) = stack.pop() {
            let (Some(a), Some(b)) = (self.nodes.get(i), other.nodes.get(j)) else { continue };
            if !enter(&a.bounds, &b.bounds) {
                continue;
            }
            match (a.count > 0, b.count > 0) {
                (true, true) => {
                    let (ia, ib) = (a.first as usize, b.first as usize);
                    let left = self.order.get(ia..ia + a.count as usize).unwrap_or(&[]);
                    let right = other.order.get(ib..ib + b.count as usize).unwrap_or(&[]);
                    for &x in left {
                        for &y in right {
                            item(x, y);
                        }
                    }
                }
                // Split the node that is not a leaf, or the larger one.
                (leaf_a, leaf_b) => {
                    let split_a = !leaf_a && (leaf_b || a.bounds.diagonal() >= b.bounds.diagonal());
                    let pairs =
                        if split_a { [(a.first as usize, j), (a.first as usize + 1, j)] } else { [(i, b.first as usize), (i, b.first as usize + 1)] };
                    let gap = |(x, y): (usize, usize)| match (self.nodes.get(x), other.nodes.get(y)) {
                        (Some(x), Some(y)) => x.bounds.center().dist(y.bounds.center()),
                        _ => f64::INFINITY,
                    };
                    let [p, q] = pairs;
                    // The nearer pair is popped first.
                    if gap(p) <= gap(q) {
                        stack.extend([q, p]);
                    } else {
                        stack.extend([p, q]);
                    }
                }
            }
        }
    }
}

fn ray_work(remaining: &mut usize) -> Result<(), &'static str> {
    *remaining = remaining.checked_sub(1).ok_or("mesh raycast exceeded its work budget")?;
    Ok(())
}

fn finite(b: &Aabb3) -> bool {
    [b.min.x, b.min.y, b.min.z, b.max.x, b.max.y, b.max.z].iter().all(|v| v.is_finite())
}

/// Where the ray from `o` along `d` (`inv` = 1 / `d` per axis) enters box `b` grown by `pad`,
/// from 0 on; `None` when it misses or the box lies behind.
fn ray_box(o: Vec3, d: Vec3, inv: Vec3, b: &Aabb3, pad: f64) -> Option<f64> {
    let (mut t0, mut t1) = (0.0f64, f64::INFINITY);
    for (o, d, inv, lo, hi) in [(o.x, d.x, inv.x, b.min.x, b.max.x), (o.y, d.y, inv.y, b.min.y, b.max.y), (o.z, d.z, inv.z, b.min.z, b.max.z)] {
        let (lo, hi) = (lo - pad, hi + pad);
        if d == 0.0 || !inv.is_finite() {
            if o < lo || o > hi {
                return None;
            }
            continue;
        }
        let (a, b) = ((lo - o) * inv, (hi - o) * inv);
        let (a, b) = if a <= b { (a, b) } else { (b, a) };
        t0 = t0.max(a);
        t1 = t1.min(b);
        if t0 > t1 {
            return None;
        }
    }
    (t0 <= t1).then_some(t0)
}

/// The farthest any corner of `b` lies along `d` from `o` (0 when the box is behind).
fn farthest_along(o: Vec3, d: Vec3, b: &Aabb3) -> f64 {
    let along = |lo: f64, hi: f64, o: f64, d: f64| if d >= 0.0 { (hi - o) * d } else { (lo - o) * d };
    (along(b.min.x, b.max.x, o.x, d.x) + along(b.min.y, b.max.y, o.y, d.y) + along(b.min.z, b.max.z, o.z, d.z)).max(0.0)
}

/// A mesh's triangles and edge segments in hierarchies, for picking: the nearest triangle along
/// a ray, and the edge segments near one. Built from one mesh and answers only for it.
#[derive(Clone, Debug, Default)]
pub struct MeshIndex {
    triangle_tree: Bvh,
    triangle_count: usize,
    position_count: usize,
    edge_lengths: Vec<usize>,
    segments: Bvh,
    /// Per segment item: its edge, and its first point in that edge's polyline.
    segment_of: Vec<(u32, u32)>,
}

const MAX_WIRE_ITEMS: usize = 2_000_000;
const MAX_WIRE_NODES: usize = MAX_WIRE_ITEMS * 2;
const MAX_WIRE_DEPTH: usize = 64;

/// Opaque, bounded flat display-index data transferred from a preparation worker.
/// Hydration validates it against the supplied mesh before it can be queried.
#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct MeshIndexWire {
    #[serde(rename = "triangles")]
    triangle_tree: BvhWire,
    triangle_count: usize,
    position_count: usize,
    #[serde(deserialize_with = "wire_items")]
    edge_lengths: Vec<usize>,
    segments: BvhWire,
    #[serde(deserialize_with = "wire_items")]
    segment_of: Vec<(u32, u32)>,
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
struct BvhWire {
    #[serde(deserialize_with = "wire_nodes")]
    nodes: Vec<Node>,
    #[serde(deserialize_with = "wire_items")]
    order: Vec<u32>,
}

fn wire_array<'de, D, T>(deserializer: D, limit: usize) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    struct Bounded<T> {
        limit: usize,
        item: std::marker::PhantomData<T>,
    }
    impl<'de, T: Deserialize<'de>> serde::de::Visitor<'de> for Bounded<T> {
        type Value = Vec<T>;
        fn expecting(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
            write!(formatter, "at most {} flat display-index entries", self.limit)
        }
        fn visit_seq<A: serde::de::SeqAccess<'de>>(self, mut sequence: A) -> Result<Self::Value, A::Error> {
            use serde::de::Error;
            if sequence.size_hint().is_some_and(|length| length > self.limit) {
                return Err(A::Error::custom("display-index array exceeds its entry limit"));
            }
            let mut items = Vec::new();
            while let Some(item) = sequence.next_element()? {
                if items.len() >= self.limit {
                    return Err(A::Error::custom("display-index array exceeds its entry limit"));
                }
                items.try_reserve(1).map_err(|_| A::Error::custom("display-index allocation refused"))?;
                items.push(item);
            }
            Ok(items)
        }
    }
    deserializer.deserialize_seq(Bounded { limit, item: std::marker::PhantomData })
}

fn wire_nodes<'de, D: serde::Deserializer<'de>>(deserializer: D) -> Result<Vec<Node>, D::Error> {
    wire_array(deserializer, MAX_WIRE_NODES)
}

fn wire_items<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: serde::Deserializer<'de>,
    T: Deserialize<'de>,
{
    wire_array(deserializer, MAX_WIRE_ITEMS)
}

fn in_box(bounds: &Aabb3, point: Vec3) -> bool {
    point.is_finite()
        && bounds.min.x <= point.x
        && point.x <= bounds.max.x
        && bounds.min.y <= point.y
        && point.y <= bounds.max.y
        && bounds.min.z <= point.z
        && point.z <= bounds.max.z
}

impl BvhWire {
    fn of(bvh: &Bvh) -> Self {
        Self { nodes: bvh.nodes.clone(), order: bvh.order.clone() }
    }

    fn hydrate(self, items: usize, mut enclosed: impl FnMut(u32, &Aabb3) -> bool) -> Option<Bvh> {
        if items > MAX_WIRE_ITEMS || self.order.len() != items || self.nodes.len() > MAX_WIRE_NODES {
            return None;
        }
        if items == 0 {
            return self.nodes.is_empty().then_some(Bvh::default());
        }
        if self.nodes.is_empty() || self.nodes.len() > items.checked_mul(2)?.checked_sub(1)? {
            return None;
        }
        let mut visited = Vec::new();
        visited.try_reserve_exact(self.nodes.len()).ok()?;
        visited.resize(self.nodes.len(), false);
        let mut covered = Vec::new();
        covered.try_reserve_exact(items).ok()?;
        covered.resize(items, false);
        let mut pending = vec![(0usize, 0usize)];
        while let Some((index, depth)) = pending.pop() {
            let seen = visited.get_mut(index)?;
            if *seen || depth > MAX_WIRE_DEPTH {
                return None;
            }
            *seen = true;
            let node = self.nodes.get(index)?;
            if !finite(&node.bounds) || node.bounds.is_empty() {
                return None;
            }
            let start = node.first as usize;
            if node.count > 0 {
                let end = start.checked_add(node.count as usize)?;
                for &item in self.order.get(start..end)? {
                    let seen = covered.get_mut(item as usize)?;
                    if *seen || !enclosed(item, &node.bounds) {
                        return None;
                    }
                    *seen = true;
                }
            } else {
                let right = start.checked_add(1)?;
                for child in [start, right] {
                    let bounds = &self.nodes.get(child)?.bounds;
                    if !in_box(&node.bounds, bounds.min) || !in_box(&node.bounds, bounds.max) {
                        return None;
                    }
                    pending.push((child, depth + 1));
                }
            }
        }
        (visited.into_iter().all(|seen| seen) && covered.into_iter().all(|seen| seen)).then_some(Bvh { nodes: self.nodes, order: self.order })
    }
}

fn pad_pick_bounds(mut bounds: Aabb3) -> Aabb3 {
    // Grow flat boxes by enough to keep rays on a face from rounding out of the box.
    let scale = [bounds.min.x, bounds.min.y, bounds.min.z, bounds.max.x, bounds.max.y, bounds.max.z].iter().fold(1.0f64, |m, v| m.max(v.abs()));
    let e = scale * 1e-9;
    bounds.min -= Vec3::new(e, e, e);
    bounds.max += Vec3::new(e, e, e);
    bounds
}

impl MeshIndex {
    /// The index of `mesh`.
    pub fn new(mesh: &Mesh) -> MeshIndex {
        let mut index = Self::triangles_only(mesh);
        let outlines = Self::segments_only(mesh);
        index.segments = outlines.segments;
        index.segment_of = outlines.segment_of;
        index.edge_lengths = outlines.edge_lengths;
        index
    }

    /// Index the triangles without the display outlines of an imported mesh.
    pub fn triangles_only(mesh: &Mesh) -> MeshIndex {
        let boxes: Vec<Aabb3> = mesh
            .triangles
            .iter()
            .map(|t| {
                let mut bounds = Aabb3::EMPTY;
                for p in mesh.tri(t).into_iter().flatten() {
                    bounds.add(p);
                }
                if bounds.is_empty() { bounds } else { pad_pick_bounds(bounds) }
            })
            .collect();
        MeshIndex {
            triangle_tree: Bvh::new(&boxes),
            triangle_count: mesh.triangles.len(),
            position_count: mesh.positions.len(),
            ..Default::default()
        }
    }

    /// Index the edge polylines separately when a tool can pick display outlines.
    pub fn segments_only(mesh: &Mesh) -> MeshIndex {
        let mut segment_of = Vec::new();
        let mut seg_boxes = Vec::new();
        for (ei, e) in mesh.edges.iter().enumerate() {
            // A one-point edge still has its vertex: it stands as a segment from it to itself.
            let n = if e.len() == 1 { 1 } else { e.len().saturating_sub(1) };
            for si in 0..n {
                let (Some(a), Some(b)) = (e.get(si), e.get(si + 1).or(e.get(si))) else { continue };
                let mut bx = Aabb3::EMPTY;
                bx.add(*a);
                bx.add(*b);
                seg_boxes.push(pad_pick_bounds(bx));
                segment_of.push((ei as u32, si as u32));
            }
        }
        MeshIndex { segments: Bvh::new(&seg_boxes), segment_of, edge_lengths: mesh.edges.iter().map(Vec::len).collect(), ..Default::default() }
    }

    /// Copy the prepared flat index for transfer to another runtime.
    pub fn to_wire(&self) -> MeshIndexWire {
        MeshIndexWire {
            triangle_tree: BvhWire::of(&self.triangle_tree),
            triangle_count: self.triangle_count,
            position_count: self.position_count,
            edge_lengths: self.edge_lengths.clone(),
            segments: BvhWire::of(&self.segments),
            segment_of: self.segment_of.clone(),
        }
    }

    /// Restore a prepared display index using bounded validation only. Every
    /// stored item must occur exactly once, its mesh coordinates must lie in its
    /// leaf, and child boxes must lie in their parents. No hierarchy is built.
    pub fn from_wire(wire: MeshIndexWire, mesh: &Mesh) -> Option<Self> {
        if wire.triangle_count != mesh.triangle_count()
            || wire.position_count != mesh.positions.len()
            || wire.triangle_count > MAX_WIRE_ITEMS
            || wire.position_count > MAX_WIRE_ITEMS * 3
            || mesh.positions.iter().any(|point| !point.is_finite())
            || wire.edge_lengths.len() != mesh.edges.len()
            || wire.edge_lengths.len() > MAX_WIRE_ITEMS
            || wire.edge_lengths.iter().zip(&mesh.edges).any(|(&length, edge)| length != edge.len())
        {
            return None;
        }
        let mut segments = 0usize;
        for (edge, path) in mesh.edges.iter().enumerate() {
            let count = if path.len() == 1 { 1 } else { path.len().saturating_sub(1) };
            segments = segments.checked_add(count)?;
            if segments > MAX_WIRE_ITEMS || path.iter().any(|point| !point.is_finite()) {
                return None;
            }
            let start = segments.checked_sub(count)?;
            for station in 0..count {
                if wire.segment_of.get(start + station).copied() != Some((u32::try_from(edge).ok()?, u32::try_from(station).ok()?)) {
                    return None;
                }
            }
        }
        if wire.segment_of.len() != segments {
            return None;
        }
        let triangles = wire.triangle_tree.hydrate(mesh.triangle_count(), |item, bounds| {
            mesh.triangle_indices()
                .get(item as usize)
                .and_then(|triangle| mesh.tri(triangle))
                .is_some_and(|points| points.into_iter().all(|point| in_box(bounds, point)))
        })?;
        let edges = wire.segments.hydrate(segments, |item, bounds| {
            let Some(&(edge, station)) = wire.segment_of.get(item as usize) else { return false };
            let Some(path) = mesh.edges.get(edge as usize) else { return false };
            let Some(&a) = path.get(station as usize) else { return false };
            let b = path.get(station as usize + 1).copied().unwrap_or(a);
            in_box(bounds, a) && in_box(bounds, b)
        })?;
        Some(Self {
            triangle_tree: triangles,
            triangle_count: wire.triangle_count,
            position_count: wire.position_count,
            edge_lengths: wire.edge_lengths,
            segments: edges,
            segment_of: wire.segment_of,
        })
    }

    /// [`Mesh::raycast`] through the index: the same answer, including which triangle wins a tie.
    /// For a mesh other than the one indexed, the plain scan answers.
    pub fn raycast(&self, mesh: &Mesh, o: Vec3, dir: Vec3) -> Option<(f64, usize)> {
        self.first_triangle(mesh, o, dir)
    }

    fn first_triangle(&self, mesh: &Mesh, o: Vec3, dir: Vec3) -> Option<(f64, usize)> {
        let frame = crate::mesh::RayFrame::new(o, dir)?;
        if mesh.triangle_count() != self.triangle_count {
            return frame.nearest(mesh.triangle_indices().iter().map(|t| mesh.tri(t)));
        }
        self.triangle_tree
            .nearest(o, dir, |i| mesh.triangle_indices().get(i as usize).and_then(|t| mesh.tri(t)).and_then(|p| frame.hit(p)))
            .map(|(t, i)| (t, i as usize))
    }

    /// Depth and optional source face under a viewport ray.
    pub fn face_hit(&self, mesh: &Mesh, origin: Vec3, direction: Vec3) -> Option<(f64, Option<u32>)> {
        let (depth, triangle) = self.first_triangle(mesh, origin, direction)?;
        Some((depth, mesh.tri_face.get(triangle).copied()))
    }

    /// [`Self::raycast`] with one unit charged per examined BVH box and triangle test.
    /// Empty indices and invalid rays do no traversal work. A different triangle
    /// count preserves the old scan fallback, charging every scanned triangle.
    /// Exhaustion is an error, never a partial nearest-hit answer.
    pub fn raycast_bounded(&self, mesh: &Mesh, o: Vec3, dir: Vec3, remaining: &mut usize) -> Result<Option<(f64, usize)>, &'static str> {
        if ![o.x, o.y, o.z, dir.x, dir.y, dir.z].iter().all(|value| value.is_finite()) {
            return Ok(None);
        }
        let Some(frame) = crate::mesh::RayFrame::new(o, dir) else { return Ok(None) };
        if mesh.triangle_count() != self.triangle_count {
            let mut best: Option<(f64, usize)> = None;
            for (index, triangle) in mesh.triangle_indices().iter().enumerate() {
                ray_work(remaining)?;
                if let Some(distance) = mesh.tri(triangle).and_then(|points| frame.hit(points))
                    && best.is_none_or(|(best, _)| distance < best)
                {
                    best = Some((distance, index));
                }
            }
            return Ok(best);
        }
        self.triangle_tree
            .nearest_bounded(o, dir, remaining, |index| {
                mesh.triangle_indices().get(index as usize).and_then(|triangle| mesh.tri(triangle)).and_then(|points| frame.hit(points))
            })
            .map(|hit| hit.map(|(distance, index)| (distance, index as usize)))
    }

    /// Calls `f(edge, first point)` for every edge segment that may come within `a + b·t` of the
    /// ray from `o` along unit `d`, `t` being the distance along it (each at most once). A
    /// constant `a` suits an orthographic view; a perspective view widens with `b`.
    pub fn segments_near(&self, o: Vec3, d: Vec3, a: f64, b: f64, mut f: impl FnMut(usize, usize)) {
        if ![a, b, o.x, o.y, o.z, d.x, d.y, d.z].iter().all(|v| v.is_finite()) {
            return;
        }
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        self.segments.visit(
            |bx| {
                let reach = a.max(0.0) + b.max(0.0) * farthest_along(o, d, bx);
                ray_box(o, d, inv, bx, reach).is_some()
            },
            |i| {
                if let Some((e, s)) = self.segment_of.get(i as usize) {
                    f(*e as usize, *s as usize);
                }
            },
        );
    }
}

/// Most points in a [`PointCloudIndex`] leaf. Kept larger than [`LEAF`] so a multi-million-point
/// scan takes a few megabytes and builds in a fraction of a second.
const POINT_LEAF: usize = 64;

#[derive(Clone, Copy, Debug)]
struct PointNode {
    min: [f32; 3],
    max: [f32; 3],
    /// A leaf: the first of its `count` point indices in [`PointCloudIndex::order`]. Otherwise its
    /// children are `first` and `first + 1`.
    first: u32,
    count: u32,
}

impl PointNode {
    fn aabb(&self) -> Aabb3 {
        Aabb3 {
            min: Vec3::new(f64::from(self.min[0]), f64::from(self.min[1]), f64::from(self.min[2])),
            max: Vec3::new(f64::from(self.max[0]), f64::from(self.max[1]), f64::from(self.max[2])),
        }
    }
}

/// The nearest any corner of `b` lies along `d` from `o`.
fn nearest_along(o: Vec3, d: Vec3, b: &Aabb3) -> f64 {
    let along = |lo: f64, hi: f64, o: f64, d: f64| if d >= 0.0 { (lo - o) * d } else { (hi - o) * d };
    along(b.min.x, b.max.x, o.x, d.x) + along(b.min.y, b.max.y, o.y, d.y) + along(b.min.z, b.max.z, o.z, d.z)
}

/// A compact bounding-volume hierarchy over a point cloud's raw `[x, y, z]` coordinates (single
/// precision, local to the cloud's origin). Built once per point array so moving or rotating the
/// cloud never rebuilds it.
#[derive(Clone, Debug, Default)]
pub struct PointCloudIndex {
    nodes: Vec<PointNode>,
    order: Vec<u32>,
    point_count: usize,
}

impl PointCloudIndex {
    /// Build a hierarchy over `values` (`x, y, z` triples). Non-finite points are left out.
    pub fn new(values: &[f32]) -> PointCloudIndex {
        let point_count = (values.len() / 3).min(u32::MAX as usize);
        let pt = |i: u32| -> Option<[f32; 3]> {
            let base = (i as usize).checked_mul(3)?;
            let s = values.get(base..base.checked_add(3)?)?;
            let p = [*s.first()?, *s.get(1)?, *s.get(2)?];
            p.iter().all(|v| v.is_finite()).then_some(p)
        };
        let mut order: Vec<u32> = (0..point_count as u32).filter(|&i| pt(i).is_some()).collect();
        if order.is_empty() {
            return PointCloudIndex { nodes: Vec::new(), order, point_count };
        }
        let mut nodes = vec![PointNode { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 }];
        let mut todo = vec![(0usize, 0usize, order.len())];
        while let Some((n, start, end)) = todo.pop() {
            let Some(span) = order.get_mut(start..end) else { continue };
            let mut min = [f32::INFINITY; 3];
            let mut max = [f32::NEG_INFINITY; 3];
            for &i in span.iter() {
                if let Some(p) = pt(i) {
                    for k in 0..3 {
                        min[k] = min[k].min(p[k]);
                        max[k] = max[k].max(p[k]);
                    }
                }
            }
            let size = [max[0] - min[0], max[1] - min[1], max[2] - min[2]];
            let axis = if size[0] >= size[1] && size[0] >= size[2] {
                0
            } else if size[1] >= size[2] {
                1
            } else {
                2
            };
            let extent = size[axis];
            if span.len() <= POINT_LEAF || !(extent > 0.0 && extent.is_finite()) {
                if let Some(node) = nodes.get_mut(n) {
                    *node = PointNode { min, max, first: start as u32, count: span.len() as u32 };
                }
                continue;
            }
            let key = |i: &u32| pt(*i).map_or(0.0f32, |p| p[axis]);
            let mid = span.len() / 2;
            span.select_nth_unstable_by(mid, |a, b| key(a).total_cmp(&key(b)));
            let left = nodes.len();
            nodes.push(PointNode { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
            nodes.push(PointNode { min: [0.0; 3], max: [0.0; 3], first: 0, count: 0 });
            if let Some(node) = nodes.get_mut(n) {
                *node = PointNode { min, max, first: left as u32, count: 0 };
            }
            todo.push((left, start, start + mid));
            todo.push((left + 1, start + mid, end));
        }
        PointCloudIndex { nodes, order, point_count }
    }

    /// Number of points this index was built for.
    pub fn point_count(&self) -> usize {
        self.point_count
    }

    /// Visit every point index whose leaf box may come within `a + b·t` of the ray from `o` along
    /// unit `d` ahead of `o` and within `*max_t` along `d`. Nearer child boxes are entered first,
    /// and `visit` may tighten `max_t` as front hits are found so boxes behind them are skipped.
    pub fn visit_near_ray(&self, o: Vec3, d: Vec3, a: f64, b: f64, max_t: &mut f64, mut visit: impl FnMut(u32, &mut f64)) {
        if self.nodes.is_empty() || ![a, b, o.x, o.y, o.z, d.x, d.y, d.z].iter().all(|v| v.is_finite()) || !(*max_t >= 0.0) {
            return;
        }
        let a = a.max(0.0);
        let b = b.max(0.0);
        let inv = Vec3::new(1.0 / d.x, 1.0 / d.y, 1.0 / d.z);
        let test = |n: usize, cap: f64| -> Option<f64> {
            let bx = self.nodes.get(n)?.aabb();
            let near = nearest_along(o, d, &bx);
            if near > cap {
                return None;
            }
            let far = farthest_along(o, d, &bx).min(cap);
            if far < 0.0 {
                return None;
            }
            let reach = a + b * far;
            let enter = ray_box(o, d, inv, &bx, reach)?;
            (enter <= cap + reach).then_some(enter)
        };
        let Some(root_t) = test(0, *max_t) else { return };
        let mut stack = vec![(0usize, root_t)];
        while let Some((n, enter)) = stack.pop() {
            let Some(node) = self.nodes.get(n) else { continue };
            let bx = node.aabb();
            if nearest_along(o, d, &bx) > *max_t {
                continue;
            }
            let far = farthest_along(o, d, &bx).min(*max_t);
            let reach = a + b * far.max(0.0);
            if enter > *max_t + reach {
                continue;
            }
            if node.count > 0 {
                let (lo, hi) = (node.first as usize, node.first as usize + node.count as usize);
                for &i in self.order.get(lo..hi).unwrap_or(&[]) {
                    visit(i, max_t);
                }
                continue;
            }
            let (l, r) = (node.first as usize, node.first as usize + 1);
            let tl = test(l, *max_t);
            let tr = test(r, *max_t);
            match (tl, tr) {
                (Some(x), Some(y)) if x <= y => stack.extend([(r, y), (l, x)]),
                (Some(x), Some(y)) => stack.extend([(l, x), (r, y)]),
                (Some(x), None) => stack.push((l, x)),
                (None, Some(y)) => stack.push((r, y)),
                (None, None) => {}
            }
        }
    }
}

#[cfg(test)]
#[path = "bvh_tests.rs"]
mod tests;
