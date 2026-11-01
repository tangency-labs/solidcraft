//! Two levels of occlusion acceleration: an instance tree over projected bounds, and one
//! triangle hierarchy per shared mesh. Per-placement node bounds are small; projected
//! triangle coordinates are evaluated only in visited leaves. Covered line intervals prune
//! subsequent nodes and instances, including the entire query once coverage is complete.
use std::sync::Arc;
use std::sync::atomic::{AtomicUsize, Ordering};
use web_time::Instant;

use solidcraft_doc::apply_point;
use solidcraft_geom::{Aabb3, Mesh, Vec3};

use super::meshes::{Placement, Scene, WORKERS, claim, transformed_bounds};
use super::progress::Stage;
use super::projection::{Drawing, Line, Part, Path, View, bounds, clip, cross, key, merge_lines, occluded, overlap, xy};

const TOO_COMPLEX: &str = "view exceeds export work budget; hide bodies or increase tolerance";
const MAX_LINES: usize = 1_000_000;
const MAX_SEGMENTS: usize = 4_000_000;
const MAX_SHARED_TRIANGLES: usize = 2_000_000;
const MAX_VIEW_NODES: usize = 256 * 1024 * 1024 / std::mem::size_of::<Aabb3>();
const WORK: usize = 200_000_000;

fn parallel<T: Sync, R: Send>(items: &[T], f: impl Fn(&T) -> Result<R, &'static str> + Sync) -> Result<Vec<R>, &'static str> {
    solidcraft_kernel::map_on_threads(items, WORKERS, f).into_iter().map(|r| r.ok_or("export worker failed")?).collect()
}

struct Node {
    bounds: Aabb3,
    start: usize,
    end: usize,
    children: Option<(usize, usize)>,
}
struct Bvh {
    nodes: Vec<Node>,
    ids: Vec<usize>,
}
impl Bvh {
    fn new(boxes: &[Aabb3], flat: bool) -> Self {
        let mut tree = Self { nodes: Vec::new(), ids: (0..boxes.len()).collect() };
        if !boxes.is_empty() {
            tree.build(boxes, 0, boxes.len(), flat);
        }
        tree
    }
    fn build(&mut self, boxes: &[Aabb3], start: usize, end: usize, flat: bool) -> usize {
        let b = self.ids.get(start..end).into_iter().flatten().filter_map(|i| boxes.get(*i)).fold(Aabb3::EMPTY, |b, t| b.union(t));
        let index = self.nodes.len();
        self.nodes.push(Node { bounds: b, start, end, children: None });
        if end - start > 8 {
            let size = b.size();
            let axis = if !flat && size.z > size.x.max(size.y) {
                2
            } else if size.x > size.y {
                0
            } else {
                1
            };
            let value = |i: &usize| {
                boxes.get(*i).map(|b| b.center()).map_or(0., |p| match axis {
                    0 => p.x,
                    1 => p.y,
                    _ => p.z,
                })
            };
            let mid = start + (end - start) / 2;
            if let Some(ids) = self.ids.get_mut(start..end) {
                ids.select_nth_unstable_by(mid - start, |a, b| value(a).total_cmp(&value(b)).then_with(|| a.cmp(b)));
            }
            let left = self.build(boxes, start, mid, flat);
            let right = self.build(boxes, mid, end, flat);
            if let Some(node) = self.nodes.get_mut(index) {
                node.children = Some((left, right));
            }
        }
        index
    }
}

struct Edge {
    vertices: [u32; 2],
    start: usize,
    end: usize,
}
struct Side {
    key: [[i64; 3]; 2],
    vertices: [u32; 2],
    triangle: usize,
}
struct Shared {
    mesh: Arc<Mesh>,
    tree: Bvh,
    edges: Vec<Edge>,
    incident: Vec<usize>,
}
impl Shared {
    fn new(mesh: &Arc<Mesh>, eps: f64, progress: &Stage) -> Result<Self, &'static str> {
        let mut boxes = Vec::with_capacity(mesh.triangles.len());
        let mut sides = Vec::new();
        for (i, indices) in mesh.triangles.iter().enumerate() {
            if i.is_multiple_of(1024) {
                progress.check()?;
            }
            let points = mesh.tri(indices).ok_or("invalid export triangle")?;
            if points.iter().any(|p| !p.is_finite()) {
                return Err("model contains invalid coordinates");
            }
            boxes.push(bounds(points));
            for (a, b) in [(0, 1), (1, 2), (2, 0)] {
                let (Some(pa), Some(pb), Some(ia), Some(ib)) = (points.get(a), points.get(b), indices.get(a), indices.get(b)) else {
                    return Err("invalid export edge");
                };
                let (ka, kb) = (key(*pa, eps), key(*pb, eps));
                if ka == kb {
                    continue;
                }
                let (key, vertices) = if ka < kb { ([ka, kb], [*ia, *ib]) } else { ([kb, ka], [*ib, *ia]) };
                sides.push(Side { key, vertices, triangle: i });
            }
        }
        let tree = Bvh::new(&boxes, false);
        sides.sort_unstable_by(|a, b| a.key.cmp(&b.key).then_with(|| a.triangle.cmp(&b.triangle)));
        let mut edges: Vec<Edge> = Vec::new();
        let mut incident = Vec::with_capacity(sides.len());
        let mut previous = None;
        for side in sides {
            if previous != Some(side.key) {
                edges.push(Edge { vertices: side.vertices, start: incident.len(), end: incident.len() });
                previous = Some(side.key);
            }
            incident.push(side.triangle);
            if let Some(e) = edges.last_mut() {
                e.end = incident.len();
            }
        }
        Ok(Self { mesh: mesh.clone(), tree, edges, incident })
    }
}

/// A placed triangle in the view.
fn triangle(view: &View, shared: &Shared, placed: &Placement, id: usize) -> Result<[Vec3; 3], &'static str> {
    let indices = shared.mesh.triangles.get(id).ok_or("invalid export triangle")?;
    let [a, b, c] = shared.mesh.tri(indices).ok_or("invalid export triangle")?;
    Ok([
        view.project(apply_point(&placed.transform, a))?,
        view.project(apply_point(&placed.transform, b))?,
        view.project(apply_point(&placed.transform, c))?,
    ])
}

struct Projected {
    nodes: Vec<Aabb3>,
    segments: Vec<(Vec3, Vec3)>,
}
impl Projected {
    fn new(shared: &Shared, placed: &Placement, view: &View, eps: f64, segments: &AtomicUsize, progress: &Stage) -> Result<Self, &'static str> {
        let mut nodes = vec![Aabb3::EMPTY; shared.tree.nodes.len()];
        // Exact projected leaf bounds avoid false near-eye errors from empty corners of a
        // rotated model-space box. Interior bounds are just unions of the child bounds.
        let mut front = vec![false; shared.mesh.triangles.len()];
        for (i, node) in shared.tree.nodes.iter().enumerate().rev() {
            if i.is_multiple_of(1024) {
                progress.check()?;
            }
            let mut b = Aabb3::EMPTY;
            if let Some((left, right)) = node.children {
                b = nodes.get(left).ok_or("invalid export tree")?.union(nodes.get(right).ok_or("invalid export tree")?);
            } else {
                for id in shared.tree.ids.get(node.start..node.end).ok_or("invalid export tree")? {
                    let p = triangle(view, shared, placed, *id)?;
                    b = b.union(&bounds(p));
                    if let Some(f) = front.get_mut(*id) {
                        // A positive projective scale preserves the projected winding.
                        *f = cross(xy(p[1] - p[0]), xy(p[2] - p[0])) > 0.;
                    }
                }
            }
            if let Some(slot) = nodes.get_mut(i) {
                *slot = b;
            }
        }
        let mut result = Vec::new();
        let mut add = |a: Vec3, b: Vec3| -> Result<(), &'static str> {
            let (a, b) = (view.project(apply_point(&placed.transform, a))?, view.project(apply_point(&placed.transform, b))?);
            if xy(a).dist(xy(b)) > eps {
                if !claim(segments, 1, MAX_SEGMENTS) {
                    return Err(TOO_COMPLEX);
                }
                result.push((a, b));
            }
            Ok(())
        };
        for (i, edge) in shared.mesh.edges.iter().enumerate() {
            if !placed.visibility.get(i).copied().unwrap_or(false) {
                continue;
            }
            for w in edge.windows(2) {
                if let (Some(a), Some(b)) = (w.first(), w.get(1)) {
                    add(*a, *b)?;
                }
            }
        }
        for edge in &shared.edges {
            let mut positive = false;
            let mut negative = false;
            for id in shared.incident.get(edge.start..edge.end).ok_or("invalid export adjacency")? {
                if front.get(*id).copied().ok_or("invalid export adjacency")? {
                    positive = true;
                } else {
                    negative = true;
                }
            }
            if (positive && negative) || edge.end - edge.start == 1 {
                let a = shared.mesh.positions.get(edge.vertices[0] as usize).ok_or("invalid silhouette")?;
                let b = shared.mesh.positions.get(edge.vertices[1] as usize).ok_or("invalid silhouette")?;
                add(*a, *b)?;
            }
        }
        Ok(Self { nodes, segments: result })
    }
}

/// A sorted union, updated as occluders are found. A node's line interval already covered by
/// nearer triangles cannot change the result and is skipped before visiting its children.
#[derive(Default)]
struct Coverage {
    spans: Vec<(f64, f64)>,
}
impl Coverage {
    fn contains(&self, lo: f64, hi: f64) -> bool {
        self.spans.iter().any(|(a, b)| *a <= lo && *b >= hi)
    }
    fn add(&mut self, mut lo: f64, mut hi: f64) -> Result<(), &'static str> {
        let first = self.spans.partition_point(|(_, b)| *b + 1e-9 < lo);
        let mut end = first;
        while let Some((a, b)) = self.spans.get(end) {
            if *a > hi + 1e-9 {
                break;
            }
            lo = lo.min(*a);
            hi = hi.max(*b);
            end += 1;
        }
        if first == end && self.spans.len() >= MAX_LINES / 2 {
            return Err(TOO_COMPLEX);
        }
        self.spans.splice(first..end, [(lo, hi)]);
        Ok(())
    }
}

fn rectangle(a: Vec3, b: Vec3, box_: &Aabb3) -> Option<(f64, f64)> {
    let (mut lo, mut hi) = (0., 1.);
    for (v0, v1) in [
        (a.x - box_.min.x, b.x - box_.min.x),
        (box_.max.x - a.x, box_.max.x - b.x),
        (a.y - box_.min.y, b.y - box_.min.y),
        (box_.max.y - a.y, box_.max.y - b.y),
    ] {
        if !clip(&mut lo, &mut hi, v0, v1) {
            return None;
        }
    }
    Some((lo, hi))
}

struct Budget<'a> {
    global: &'a AtomicUsize,
    progress: &'a Stage,
    local: usize,
    spent: usize,
}
impl<'a> Budget<'a> {
    fn new(global: &'a AtomicUsize, progress: &'a Stage) -> Self {
        Self { global, progress, local: 0, spent: 0 }
    }
    fn spend(&mut self) -> Result<(), &'static str> {
        if self.local == 0 {
            self.progress.check()?;
            let mut old = self.global.load(Ordering::Relaxed);
            loop {
                let granted = old.min(4096);
                if granted == 0 {
                    return Err(TOO_COMPLEX);
                }
                match self.global.compare_exchange_weak(old, old - granted, Ordering::Relaxed, Ordering::Relaxed) {
                    Ok(_) => {
                        self.local = granted;
                        break;
                    }
                    Err(current) => old = current,
                }
            }
        }
        self.local -= 1;
        self.spent += 1;
        Ok(())
    }
}

struct Query<'a, 'b> {
    shared: &'a [Shared],
    scene: &'a Scene,
    projected: &'a [Projected],
    instances: &'a Bvh,
    view: &'a View,
    depth_eps: f64,
    a: Vec3,
    b: Vec3,
    bounds: Aabb3,
    covered: Coverage,
    budget: &'b mut Budget<'a>,
}
impl Query<'_, '_> {
    fn relevant(&self, b: &Aabb3) -> bool {
        if b.max.z <= self.a.z.min(self.b.z) + self.depth_eps || !overlap(b, &self.bounds) {
            return false;
        }
        rectangle(self.a, self.b, b).is_some_and(|(lo, hi)| !self.covered.contains(lo, hi))
    }
    fn instance_node(&mut self, index: usize) -> Result<(), &'static str> {
        self.budget.spend()?;
        let node = self.instances.nodes.get(index).ok_or("invalid instance tree")?;
        if !self.relevant(&node.bounds) {
            return Ok(());
        }
        if let Some((left, right)) = node.children {
            let depth = |i: usize| self.instances.nodes.get(i).map_or(f64::NEG_INFINITY, |n| n.bounds.max.z);
            let (near, far) = if depth(left) >= depth(right) { (left, right) } else { (right, left) };
            self.instance_node(near)?;
            if !self.covered.contains(0., 1.) {
                self.instance_node(far)?;
            }
        } else {
            for id in self.instances.ids.get(node.start..node.end).ok_or("invalid instance tree")? {
                self.mesh_node(*id, 0)?;
                if self.covered.contains(0., 1.) {
                    break;
                }
            }
        }
        Ok(())
    }
    fn mesh_node(&mut self, instance: usize, index: usize) -> Result<(), &'static str> {
        self.budget.spend()?;
        let placed = self.scene.placements.get(instance).ok_or("invalid instance")?;
        let shared = self.shared.get(placed.mesh).ok_or("invalid shared geometry")?;
        let projected = self.projected.get(instance).ok_or("invalid instance bounds")?;
        let b = projected.nodes.get(index).ok_or("invalid mesh tree")?;
        if !self.relevant(b) {
            return Ok(());
        }
        let node = shared.tree.nodes.get(index).ok_or("invalid mesh tree")?;
        if let Some((left, right)) = node.children {
            let depth = |i: usize| projected.nodes.get(i).map_or(f64::NEG_INFINITY, |b| b.max.z);
            let (near, far) = if depth(left) >= depth(right) { (left, right) } else { (right, left) };
            self.mesh_node(instance, near)?;
            if !self.covered.contains(0., 1.) {
                self.mesh_node(instance, far)?;
            }
        } else {
            for id in shared.tree.ids.get(node.start..node.end).ok_or("invalid mesh tree")? {
                self.budget.spend()?;
                let points = triangle(self.view, shared, placed, *id)?;
                if let Some((lo, hi)) = occluded(self.a, self.b, points, self.depth_eps) {
                    self.covered.add(lo, hi)?;
                    if self.covered.contains(0., 1.) {
                        break;
                    }
                }
            }
        }
        Ok(())
    }
}

/// The lines of `scene`'s placed meshes and of the `sketch` runs, seen from `view`.
pub(crate) fn drawing(scene: &Scene, sketch: &[Part], view: &View) -> Result<Drawing, &'static str> {
    let start = Instant::now();
    let scale = scene
        .placements
        .iter()
        .filter_map(|p| scene.meshes.get(p.mesh).map(|m| transformed_bounds(m, &p.transform)))
        .chain(sketch.iter().map(|p| bounds(p.points.iter().copied())))
        .fold(Aabb3::EMPTY, |b, p| b.union(&p))
        .diagonal()
        .max(1.);
    if !scale.is_finite() {
        return Err("model contains invalid coordinates");
    }
    let eps = scale * 1e-8;
    // Indexed input can have many triangles but very few positions. Bound adjacency/sort
    // scratch space as well as the retained mesh bytes before starting parallel builders.
    let unique_triangles = scene.meshes.iter().try_fold(0usize, |n, m| n.checked_add(m.triangles.len()));
    if unique_triangles.is_none_or(|n| n > MAX_SHARED_TRIANGLES) {
        return Err("view exceeds export memory budget; hide bodies or increase tolerance");
    }
    let progress = Stage::new("dxf_index", scene.meshes.len());
    let shared = parallel(&scene.meshes, |m| {
        progress.check()?;
        let shared = Shared::new(m, eps, &progress)?;
        progress.advance(1)?;
        Ok(shared)
    })?;
    let count = scene.placements.iter().try_fold(0usize, |n, p| n.checked_add(shared.get(p.mesh)?.tree.nodes.len()));
    if count.is_none_or(|n| n > MAX_VIEW_NODES) {
        return Err("view exceeds export memory budget; hide bodies or increase tolerance");
    }
    let count = AtomicUsize::new(0);
    let progress = Stage::new("dxf_projection", scene.placements.len());
    let projected = parallel(&scene.placements, |p| {
        progress.check()?;
        let projected = Projected::new(shared.get(p.mesh).ok_or("missing shared geometry")?, p, view, eps, &count, &progress)?;
        progress.advance(1)?;
        Ok(projected)
    })?;
    let boxes = projected.iter().map(|p| p.nodes.first().copied().unwrap_or(Aabb3::EMPTY)).collect::<Vec<_>>();
    let mut instances = Bvh::new(&boxes, true);
    // The view is fixed for the entire drawing. Sort the at-most-eight members of each
    // instance leaf once, rather than allocating/sorting on every line query.
    for node in instances.nodes.iter().filter(|n| n.children.is_none()) {
        if let Some(ids) = instances.ids.get_mut(node.start..node.end) {
            ids.sort_unstable_by(|a, b| {
                boxes
                    .get(*b)
                    .map_or(f64::NEG_INFINITY, |b| b.max.z)
                    .total_cmp(&boxes.get(*a).map_or(f64::NEG_INFINITY, |b| b.max.z))
                    .then_with(|| a.cmp(b))
            });
        }
    }
    let segment_count = count.load(Ordering::Relaxed);
    let projection_ms = start.elapsed().as_secs_f64() * 1000.;
    let start = Instant::now();
    let depth_eps = view.depth_eps(eps);
    let global = AtomicUsize::new(WORK);
    let output = AtomicUsize::new(0);
    let chunks = projected.iter().flat_map(|p| p.segments.chunks(128)).collect::<Vec<_>>();
    let progress = Stage::new("dxf_occlusion", segment_count);
    let results = parallel(&chunks, |chunk| {
        progress.check()?;
        let mut budget = Budget::new(&global, &progress);
        let mut lines = Vec::new();
        for &(a, b) in *chunk {
            let mut query = Query {
                shared: &shared,
                scene,
                projected: &projected,
                instances: &instances,
                view,
                depth_eps,
                a,
                b,
                bounds: bounds([a, b]),
                covered: Coverage::default(),
                budget: &mut budget,
            };
            query.instance_node(0)?;
            let mut cursor = 0.;
            let mut emit = |lo: f64, hi: f64, hidden: bool| -> Result<(), &'static str> {
                let (p, q) = (xy(a + (b - a) * lo), xy(a + (b - a) * hi));
                if p.dist(q) > eps {
                    if !claim(&output, 1, MAX_LINES) {
                        return Err(TOO_COMPLEX);
                    }
                    lines.push(Line { a: p, b: q, hidden });
                }
                Ok(())
            };
            for (lo, hi) in query.covered.spans {
                emit(cursor, lo, false)?;
                emit(lo, hi, true)?;
                cursor = hi;
            }
            emit(cursor, 1., false)?;
        }
        progress.advance(chunk.len())?;
        Ok((lines, budget.spent))
    })?;
    let work = results.iter().map(|(_, n)| *n).sum();
    let lines = merge_lines(results.into_iter().flat_map(|(lines, _)| lines).collect(), eps);
    progress.check()?;
    // Shown sketch curves: finished sketches are split where bodies cover them, like body
    // edges; the active sketch is drawn on top, as in the viewport. Each curve run stays one
    // path per visible or hidden stretch, so a circle remains one polyline.
    let progress = Stage::new("dxf_occlusion", sketch.len());
    let results = parallel(sketch, |part| {
        progress.check()?;
        let mut budget = Budget::new(&global, &progress);
        let mut paths: Vec<Path> = Vec::new();
        let mut previous: Option<Vec3> = None;
        for p in &part.points {
            let b = view.project(*p)?;
            let Some(a) = previous.replace(b) else { continue };
            if xy(a).dist(xy(b)) <= eps {
                continue;
            }
            if !claim(&count, 1, MAX_SEGMENTS) {
                return Err(TOO_COMPLEX);
            }
            let mut covered = Coverage::default();
            if !part.on_top && !instances.nodes.is_empty() {
                let mut query = Query {
                    shared: &shared,
                    scene,
                    projected: &projected,
                    instances: &instances,
                    view,
                    depth_eps,
                    a,
                    b,
                    bounds: bounds([a, b]),
                    covered,
                    budget: &mut budget,
                };
                query.instance_node(0)?;
                covered = query.covered;
            }
            let mut cursor = 0.;
            let mut emit = |lo: f64, hi: f64, hidden: bool| -> Result<(), &'static str> {
                let (p, q) = (xy(a + (b - a) * lo), xy(a + (b - a) * hi));
                if p.dist(q) <= eps {
                    return Ok(());
                }
                if let Some(path) = paths.last_mut().filter(|path| path.hidden == hidden && path.points.last().is_some_and(|e| e.dist(p) <= eps)) {
                    path.points.push(q);
                } else {
                    if !claim(&output, 1, MAX_LINES) {
                        return Err(TOO_COMPLEX);
                    }
                    paths.push(Path { points: vec![p, q], hidden });
                }
                Ok(())
            };
            for (lo, hi) in covered.spans {
                emit(cursor, lo, false)?;
                emit(lo, hi, true)?;
                cursor = hi;
            }
            emit(cursor, 1., false)?;
        }
        progress.advance(1)?;
        Ok((paths, budget.spent))
    })?;
    let work = results.iter().fold(work, |n: usize, (_, spent)| n.saturating_add(*spent));
    let sketch = results.into_iter().flat_map(|(paths, _)| paths).collect();
    progress.check()?;
    Ok(Drawing { lines, work, segments: count.load(Ordering::Relaxed), projection_ms, occlusion_ms: start.elapsed().as_secs_f64() * 1000., sketch })
}

#[cfg(test)]
mod tests {
    use super::*;
    use solidcraft_render::Camera;
    #[test]
    fn coverage_handles_disjoint_spans_and_early_complete_coverage() {
        let mut c = Coverage::default();
        c.add(0.4, 0.6).unwrap();
        c.add(0., 0.2).unwrap();
        assert!(c.contains(0.05, 0.15));
        assert!(!c.contains(0., 1.));
        c.add(0.2, 0.9).unwrap();
        c.add(0.8, 1.).unwrap();
        assert_eq!(c.spans, vec![(0., 1.)]);
    }

    fn box_mesh() -> Mesh {
        let mut mesh = Mesh::default();
        mesh.positions = vec![
            Vec3::new(0., 0., 0.),
            Vec3::new(8., 0., 0.),
            Vec3::new(8., 6., 0.),
            Vec3::new(0., 6., 0.),
            Vec3::new(0., 0., 3.),
            Vec3::new(8., 0., 3.),
            Vec3::new(8., 6., 3.),
            Vec3::new(0., 6., 3.),
        ];
        mesh.triangles =
            vec![[0, 2, 1], [0, 3, 2], [4, 5, 6], [4, 6, 7], [0, 1, 5], [0, 5, 4], [1, 2, 6], [1, 6, 5], [2, 3, 7], [2, 7, 6], [3, 0, 4], [3, 4, 7]];
        mesh.edges = [(0, 1), (1, 2), (2, 3), (3, 0), (4, 5), (5, 6), (6, 7), (7, 4), (0, 4), (1, 5), (2, 6), (3, 7)]
            .into_iter()
            .map(|(a, b)| vec![mesh.positions[a], mesh.positions[b]])
            .collect();
        mesh
    }

    fn canonical(lines: Vec<Line>) -> Vec<([i64; 2], [i64; 2], bool)> {
        let k = |p: solidcraft_geom::Vec2| [(p.x * 1e6).round() as i64, (p.y * 1e6).round() as i64];
        let mut result = lines
            .into_iter()
            .map(|l| {
                let (a, b) = (k(l.a), k(l.b));
                if a < b { (a, b, l.hidden) } else { (b, a, l.hidden) }
            })
            .collect::<Vec<_>>();
        result.sort_unstable();
        result
    }

    #[test]
    fn shared_rotated_instances_match_flat_occlusion_in_orthographic_and_perspective_views() {
        use std::sync::Arc;
        let mesh = box_mesh();
        let mut placements = Vec::new();
        let mut flat = Vec::new();
        for (offset, angle) in [(Vec3::ZERO, 0.), (Vec3::new(3., 2., 4.), 0.3), (Vec3::new(2., 1., -4.), -0.6)] {
            let transform = solidcraft_doc::rigid(offset, Vec3::ZERO, Vec3::Z, angle);
            placements.push(Placement { mesh: 0, transform, visibility: vec![true; mesh.edges.len()] });
            let mut placed = mesh.clone();
            placed.positions = placed.positions.iter().map(|p| apply_point(&transform, *p)).collect();
            placed.edges = placed.edges.iter().map(|e| e.iter().map(|p| apply_point(&transform, *p)).collect()).collect();
            flat.push(placed);
        }
        let scene = Scene { meshes: vec![Arc::new(mesh)], placements, triangles: 36, bytes: 0 };
        for (yaw, pitch, roll) in [(0., std::f64::consts::FRAC_PI_2, 0.), (0.7, 0.4, 0.2), (0., 0., 0.)] {
            for fov in [0., 0.6] {
                let cam = Camera { target: Vec3::new(4., 3., 1.), yaw, pitch, roll, fov, distance: 100., ..Default::default() };
                let expected = super::super::projection::reference_drawing(&flat, &cam).unwrap();
                let actual = drawing(&scene, &[], &View::from_camera(&cam)).unwrap();
                assert_eq!(canonical(actual.lines), canonical(expected), "yaw={yaw}, pitch={pitch}, fov={fov}");
            }
        }
    }

    #[test]
    fn repeated_surface_over_one_million_triangles_keeps_one_mesh_and_exports_all_placements() {
        use solidcraft_render::StandardView;
        use std::sync::Arc;
        let mut mesh = Mesh::default();
        for y in 0..=100 {
            for x in 0..=100 {
                mesh.positions.push(Vec3::new(x as f64 / 10., y as f64 / 10., 0.));
            }
        }
        for y in 0..100u32 {
            for x in 0..100u32 {
                let a = y * 101 + x;
                mesh.triangles.extend([[a, a + 1, a + 102], [a, a + 102, a + 101]]);
            }
        }
        mesh.edges = vec![
            vec![Vec3::ZERO, Vec3::new(10., 0., 0.)],
            vec![Vec3::new(10., 0., 0.), Vec3::new(10., 10., 0.)],
            vec![Vec3::new(10., 10., 0.), Vec3::new(0., 10., 0.)],
            vec![Vec3::new(0., 10., 0.), Vec3::ZERO],
        ];
        let placements = (0..60)
            .map(|i| Placement {
                mesh: 0,
                transform: solidcraft_doc::rigid(Vec3::new((i % 10) as f64 * 20., (i / 10) as f64 * 20., 0.), Vec3::ZERO, Vec3::Z, 0.),
                visibility: vec![true; 4],
            })
            .collect();
        let scene = Scene { meshes: vec![Arc::new(mesh)], placements, triangles: 1_200_000, bytes: 0 };
        let mut cam = Camera { target: Vec3::new(95., 55., 0.), fov: 0., distance: 300., ..Default::default() };
        cam.set_view(StandardView::Top);
        let result = drawing(&scene, &[], &View::from_camera(&cam)).unwrap();
        assert_eq!(scene.meshes.len(), 1);
        assert_eq!(result.lines.len(), 240);
        assert!(result.lines.iter().all(|l| !l.hidden));
    }
}
