// SolidCraft: cold projection samples are immutable geometry, independent of the
// query point. Keep owned snapshots (never pointer identities), and reuse the
// exact upstream grid and its first-minimum tie rule. Newton remains unchanged.
use crate::*;
use std::collections::VecDeque;
use std::mem::size_of;
use std::sync::{Arc, Mutex};

const MAX_BYTES: usize = 32 << 20;
const MAX_GEOMETRY_BYTES: usize = 256 << 10;
const MAX_ENTRIES: usize = 128;
const MAX_ROWS: usize = 4096;

#[derive(Clone, Copy)]
struct Sample {
    uv: (f64, f64),
    point: Point3,
    order: usize,
}
struct Node {
    sample: usize,
    axis: usize,
    children: [Option<usize>; 2],
}
struct Prepared {
    surface: Surface,
    samples: Vec<Sample>,
    nodes: Vec<Node>,
    root: Option<usize>,
    bytes: usize,
}
struct Cache {
    entries: VecDeque<Arc<Prepared>>,
    bytes: usize,
}
static CACHE: Mutex<Cache> = Mutex::new(Cache { entries: VecDeque::new(), bytes: 0 });

fn spline_bytes<P>(knots: &(KnotVec, KnotVec), points: &[Vec<P>]) -> Option<usize> {
    if points.is_empty() || points.len() > MAX_ROWS {
        return None;
    }
    let knots = knots.0.len().checked_add(knots.1.len())?.checked_mul(size_of::<f64>())?;
    let rows = points.len().checked_mul(size_of::<Vec<P>>())?;
    let count = points.iter().try_fold(0usize, |n, row| n.checked_add(row.len()))?;
    let bytes = count.checked_mul(size_of::<P>())?.checked_add(knots)?.checked_add(rows)?;
    (bytes <= MAX_GEOMETRY_BYTES).then_some(bytes)
}

fn curve_bytes(curve: &Curve) -> Option<usize> {
    match curve {
        Curve::Line(_) => Some(size_of::<Line<Point3>>()),
        Curve::BSplineCurve(c) => c.control_points().len().checked_mul(size_of::<Point3>())?.checked_add(c.knot_vec().len().checked_mul(8)?),
        Curve::NurbsCurve(c) => c.control_points().len().checked_mul(size_of::<Vector4>())?.checked_add(c.knot_vec().len().checked_mul(8)?),
        // Procedural intersection profiles can themselves own other surfaces.
        Curve::IntersectionCurve(_) => None,
    }
    .filter(|bytes| *bytes <= MAX_GEOMETRY_BYTES)
}

fn geometry_bytes(surface: &Surface) -> Option<usize> {
    let bytes = match surface {
        Surface::BSplineSurface(s) => {
            let bytes = spline_bytes(s.knot_vecs(), s.control_points())?;
            if !s.control_points().iter().flatten().all(|p| [p.x, p.y, p.z].iter().all(|x| x.is_finite())) {
                return None;
            }
            bytes
        }
        Surface::NurbsSurface(s) => {
            let bytes = spline_bytes(s.knot_vecs(), s.control_points())?;
            if !s.control_points().iter().flatten().all(|p| [p.x, p.y, p.z, p.w].iter().all(|x| x.is_finite())) {
                return None;
            }
            bytes
        }
        Surface::RevolutedCurve(s) => curve_bytes(s.entity().entity_curve())?,
        Surface::Plane(_) => return None,
    };
    bytes.checked_add(size_of::<Surface>())
}

fn same_curve(a: &Curve, b: &Curve) -> bool {
    match (a, b) {
        (Curve::Line(a), Curve::Line(b)) => a == b,
        (Curve::BSplineCurve(a), Curve::BSplineCurve(b)) => a == b,
        (Curve::NurbsCurve(a), Curve::NurbsCurve(b)) => a == b,
        _ => false,
    }
}

fn same_surface(a: &Surface, b: &Surface) -> bool {
    match (a, b) {
        (Surface::BSplineSurface(a), Surface::BSplineSurface(b)) => a == b,
        (Surface::NurbsSurface(a), Surface::NurbsSurface(b)) => a == b,
        (Surface::RevolutedCurve(a), Surface::RevolutedCurve(b)) => {
            a.orientation() == b.orientation()
                && a.transform() == b.transform()
                && a.entity().origin() == b.entity().origin()
                && a.entity().axis() == b.entity().axis()
                && same_curve(a.entity().entity_curve(), b.entity().entity_curve())
        }
        _ => false,
    }
}

fn coordinate(p: Point3, axis: usize) -> f64 {
    match axis {
        0 => p.x,
        1 => p.y,
        _ => p.z,
    }
}

fn build_tree(indices: &mut [usize], samples: &[Sample], nodes: &mut Vec<Node>, depth: usize) -> Option<usize> {
    if indices.is_empty() {
        return None;
    }
    let axis = depth % 3;
    let middle = indices.len() / 2;
    indices.select_nth_unstable_by(middle, |a, b| {
        let key = |i: &usize| samples.get(*i).map_or(f64::INFINITY, |s| coordinate(s.point, axis));
        key(a).total_cmp(&key(b)).then(a.cmp(b))
    });
    let (left, right) = indices.split_at_mut(middle);
    let (index, right) = right.split_first_mut()?;
    let index = *index;
    // At most 10,201 samples: this balanced recursion has fewer than 15 levels.
    let children = [build_tree(left, samples, nodes, depth + 1), build_tree(right, samples, nodes, depth + 1)];
    let node = nodes.len();
    nodes.push(Node { sample: index, axis, children });
    Some(node)
}

impl Prepared {
    fn new(surface: &Surface, geometry: usize) -> Option<Self> {
        let division: usize = if matches!(surface, Surface::RevolutedCurve(_)) { 100 } else { 50 };
        let (Some((u0, u1)), Some((v0, v1))) = surface.try_range_tuple() else {
            return None;
        };
        if ![u0, u1, v0, v1].iter().all(|x| x.is_finite()) {
            return None;
        }
        let count = (division + 1) * (division + 1);
        let mut samples = Vec::with_capacity(count);
        for i in 0..=division {
            for j in 0..=division {
                let a = i as f64 / division as f64;
                let b = j as f64 / division as f64;
                let uv = (u0 * (1.0 - a) + u1 * a, v0 * (1.0 - b) + v1 * b);
                let point = surface.subs(uv.0, uv.1);
                // The upstream strict-distance scan ignores a non-finite distance.
                if point.x.is_finite() && point.y.is_finite() && point.z.is_finite() {
                    samples.push(Sample { uv, point, order: i * (division + 1) + j });
                }
            }
        }
        if samples.is_empty() {
            return None;
        }
        let mut nodes = Vec::with_capacity(samples.len());
        let root = build_tree(&mut (0..samples.len()).collect::<Vec<_>>(), &samples, &mut nodes, 0);
        let bytes = geometry
            .checked_add(samples.capacity().checked_mul(size_of::<Sample>())?)?
            .checked_add(nodes.capacity().checked_mul(size_of::<Node>())?)?
            .checked_add(size_of::<Self>())?;
        (bytes <= MAX_BYTES).then(|| Self { surface: surface.clone(), samples, nodes, root, bytes })
    }

    fn nearest(&self, point: Point3) -> Option<(f64, f64)> {
        fn visit(prepared: &Prepared, node: usize, point: Point3, best: &mut (f64, usize, Option<usize>)) {
            let Some(node) = prepared.nodes.get(node) else { return };
            let Some(sample) = prepared.samples.get(node.sample) else { return };
            let distance = sample.point.distance2(point);
            if distance.is_finite() && (distance < best.0 || (distance == best.0 && sample.order < best.1)) {
                *best = (distance, sample.order, Some(node.sample));
            }
            let delta = coordinate(point, node.axis) - coordinate(sample.point, node.axis);
            let (near, far) = if delta < 0.0 { (0, 1) } else { (1, 0) };
            if let Some(child) = node.children.get(near).copied().flatten() {
                visit(prepared, child, point, best);
            }
            // Include ties so duplicate points still select the first grid node.
            if delta * delta <= best.0 {
                if let Some(child) = node.children.get(far).copied().flatten() {
                    visit(prepared, child, point, best);
                }
            }
        }
        let mut best = (f64::INFINITY, usize::MAX, None);
        visit(self, self.root?, point, &mut best);
        self.samples.get(best.2?).map(|sample| sample.uv)
    }
}

/// SolidCraft's exact nearest cold-grid sample, with bounded shared preparation.
/// Unsupported or oversized geometry returns `None` and uses upstream presearch.
pub fn seed(surface: &Surface, point: Point3) -> Option<(f64, f64)> {
    if ![point.x, point.y, point.z].iter().all(|x| x.is_finite()) {
        return None;
    }
    let geometry = geometry_bytes(surface)?;
    let existing = {
        let mut cache = CACHE.lock().ok()?;
        let at = cache.entries.iter().position(|entry| same_surface(&entry.surface, surface));
        at.and_then(|at| cache.entries.remove(at)).map(|entry| {
            cache.entries.push_back(entry.clone());
            entry
        })
    };
    if let Some(entry) = existing {
        return entry.nearest(point);
    }
    // Evaluation and tree construction never hold the cache lock; concurrent
    // queries can finish using their immutable Arc while entries are evicted.
    let prepared = Arc::new(Prepared::new(surface, geometry)?);
    let entry = {
        let mut cache = CACHE.lock().ok()?;
        if let Some(existing) = cache.entries.iter().find(|entry| same_surface(&entry.surface, surface)) {
            existing.clone()
        } else {
            while cache.entries.len() >= MAX_ENTRIES || cache.bytes.saturating_add(prepared.bytes) > MAX_BYTES {
                let old = cache.entries.pop_front()?;
                cache.bytes = cache.bytes.saturating_sub(old.bytes);
            }
            cache.bytes = cache.bytes.saturating_add(prepared.bytes);
            cache.entries.push_back(prepared.clone());
            prepared
        }
    };
    entry.nearest(point)
}
