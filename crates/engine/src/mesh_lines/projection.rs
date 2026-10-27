//! Vector hidden-line removal: intersect each projected edge with triangle half planes and
//! its depth plane, then union occluded parameter intervals. A BVH bounds the candidate work.
#[cfg(test)]
use solidcraft_geom::Mesh;
use solidcraft_geom::{Aabb3, Plane, Vec2, Vec3};
use solidcraft_render::Camera;
use std::collections::BTreeMap;

pub(crate) struct Drawing {
    pub lines: Vec<Line>,
    pub work: usize,
    pub segments: usize,
    pub projection_ms: f64,
    pub occlusion_ms: f64,
    /// Shown sketch curves: one path per visible or hidden run of each curve.
    pub sketch: Vec<Path>,
    /// The tolerance the lines were merged at: spans no longer than it are dropped.
    pub eps: f64,
}

/// A connected run of a projected sketch curve.
#[derive(Clone, Debug)]
pub(crate) struct Path {
    pub points: Vec<Vec2>,
    pub hidden: bool,
}

/// One connected run of a sketch curve in world coordinates.
pub(crate) struct Part {
    pub points: Vec<Vec3>,
    /// Drawn over the bodies (the sketch being edited) rather than hidden behind them.
    pub on_top: bool,
}

/// Where lines are seen from: an origin, its right, up and back directions, and for a
/// perspective view the eye's distance behind the origin, whose plane lines are projected onto.
pub(crate) struct View {
    origin: Vec3,
    right: Vec3,
    up: Vec3,
    back: Vec3,
    perspective: Option<f64>,
}

impl View {
    /// A viewport camera's view: perspective onto the plane through its target when it has a
    /// field of view.
    pub(crate) fn from_camera(cam: &Camera) -> Self {
        let (right, up, back) = cam.basis();
        Self { origin: cam.target, right, up, back, perspective: (cam.fov > 0.).then_some(cam.distance) }
    }

    /// A drawing view's: orthographic, on `basis`, so lines come out in its plane's coordinates
    /// ([`Plane::to_local`]) and depth grows toward the viewer.
    pub(crate) fn orthographic(basis: &Plane) -> Self {
        Self { origin: basis.origin, right: basis.x, up: basis.y, back: basis.normal(), perspective: None }
    }

    /// `p` in the view: right and up, and a depth that grows toward the eye (the distance
    /// along back, or its reciprocal's change in perspective).
    pub(super) fn project(&self, p: Vec3) -> Result<Vec3, &'static str> {
        if !p.is_finite() {
            return Err("model contains invalid coordinates");
        }
        let d = p - self.origin;
        let z = d.dot(self.back);
        let projected = if let Some(distance) = self.perspective {
            let w = distance - z;
            if w <= distance * 1e-6 {
                return Err("model crosses the camera eye plane; zoom out before exporting");
            }
            Vec3::new(d.dot(self.right) * distance / w, d.dot(self.up) * distance / w, 1. / w)
        } else {
            Vec3::new(d.dot(self.right), d.dot(self.up), z)
        };
        if !projected.is_finite() || projected.len() > 1e12 {
            return Err("projected coordinates exceed the export range");
        }
        Ok(projected)
    }

    /// How far behind a surface a line must be to count as covered, in the depth `project` gives.
    pub(super) fn depth_eps(&self, eps: f64) -> f64 {
        self.perspective.map_or(eps, |distance| 1e-8 / distance)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct Line {
    pub a: Vec2,
    pub b: Vec2,
    pub hidden: bool,
    /// What drew it, when the scene keeps sources ([`super::meshes::Ask::sources`]).
    pub owner: Option<Owner>,
    /// Another source lay on this stretch too, and this one's owner drew it.
    pub covered: bool,
    /// How near the viewer its middle lies, in the depth [`View::project`] gives: of the sources
    /// on one stretch, the nearest draws it.
    pub depth: f64,
}

/// What drew a line: a placement, and its edge or a silhouette. Of sources level with each other on
/// a stretch, the first in this order draws it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) struct Owner {
    /// The index of the placement among the scene's.
    pub placement: u32,
    pub origin: Origin,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Origin {
    /// The placement's edge, as the body's edges are numbered.
    Edge(u32),
    /// A silhouette across the B-rep face of a triangle that faces the view.
    Silhouette(u32),
}

pub(super) fn xy(p: Vec3) -> Vec2 {
    Vec2::new(p.x, p.y)
}
pub(super) fn cross(a: Vec2, b: Vec2) -> f64 {
    a.x * b.y - a.y * b.x
}
pub(super) fn bounds(points: impl IntoIterator<Item = Vec3>) -> Aabb3 {
    let mut b = Aabb3::EMPTY;
    for p in points {
        b.add(p);
    }
    b
}
pub(super) fn overlap(a: &Aabb3, b: &Aabb3) -> bool {
    a.min.x <= b.max.x && b.min.x <= a.max.x && a.min.y <= b.max.y && b.min.y <= a.max.y
}

#[cfg(test)]
struct Triangle {
    p: [Vec3; 3],
    bounds: Aabb3,
}
#[cfg(test)]
struct Tree {
    bounds: Aabb3,
    children: Option<(Box<Tree>, Box<Tree>)>,
    triangles: Vec<Triangle>,
}
#[cfg(test)]
impl Tree {
    fn new(mut triangles: Vec<Triangle>) -> Self {
        let b = triangles.iter().fold(Aabb3::EMPTY, |b, t| b.union(&t.bounds));
        if triangles.len() <= 12 {
            return Self { bounds: b, children: None, triangles };
        }
        let x = b.size().x > b.size().y;
        let middle = triangles.len() / 2;
        triangles.select_nth_unstable_by(middle, |a, b| {
            let c = |t: &Triangle| if x { t.bounds.center().x } else { t.bounds.center().y };
            c(a).total_cmp(&c(b))
        });
        let right = triangles.split_off(middle);
        Self { bounds: b, children: Some((Box::new(Self::new(triangles)), Box::new(Self::new(right)))), triangles: Vec::new() }
    }
    fn intervals(&self, a: Vec3, b: Vec3, bx: &Aabb3, eps: f64, out: &mut Vec<(f64, f64)>, budget: &mut usize) -> Result<(), &'static str> {
        *budget = budget.checked_sub(1).ok_or("view too complex to export; hide some bodies")?;
        if !overlap(&self.bounds, bx) || self.bounds.max.z <= a.z.min(b.z) + eps {
            return Ok(());
        }
        if let Some((left, right)) = &self.children {
            left.intervals(a, b, bx, eps, out, budget)?;
            right.intervals(a, b, bx, eps, out, budget)?;
        } else {
            for t in &self.triangles {
                if overlap(&t.bounds, bx)
                    && let Some(span) = occluded(a, b, t.p, eps)
                {
                    out.push(span);
                }
            }
        }
        Ok(())
    }
}

/// Clip [lo,hi] by the linear inequality f(t) >= 0.
pub(super) fn clip(lo: &mut f64, hi: &mut f64, f0: f64, f1: f64) -> bool {
    if f0 < 0.0 && f1 < 0.0 {
        return false;
    }
    if (f0 < 0.0) != (f1 < 0.0) {
        let at = f0 / (f0 - f1);
        if f0 < 0.0 {
            *lo = lo.max(at);
        } else {
            *hi = hi.min(at);
        }
    }
    *hi > *lo
}
pub(super) fn occluded(a: Vec3, b: Vec3, t: [Vec3; 3], eps: f64) -> Option<(f64, f64)> {
    let [p, q, r] = t;
    let area = cross(xy(q - p), xy(r - p));
    if area.abs() < 1e-20 {
        return None;
    }
    let (mut lo, mut hi) = (0.0, 1.0);
    for (u, v) in [(p, q), (q, r), (r, p)] {
        let side = |w: Vec3| cross(xy(v - u), xy(w - u)) * area.signum();
        if !clip(&mut lo, &mut hi, side(a), side(b)) {
            return None;
        }
    }
    let depth = |w: Vec3| {
        let s = cross(xy(w - p), xy(r - p)) / area;
        let u = cross(xy(q - p), xy(w - p)) / area;
        p.z + s * (q.z - p.z) + u * (r.z - p.z) - w.z - eps
    };
    clip(&mut lo, &mut hi, depth(a), depth(b)).then_some((lo, hi))
}

pub(super) fn key(p: Vec3, eps: f64) -> [i64; 3] {
    [(p.x / eps).round() as i64, (p.y / eps).round() as i64, (p.z / eps).round() as i64]
}

/// Merge collinear projected intervals, giving visible strokes precedence. This also removes
/// a rear edge underneath only part of a visible edge, and rejoins subdivided silhouettes. With
/// owners, a stretch belongs to its nearest visible owner, else its nearest hidden one (owners level
/// within `eps` go in [`Owner`] order), and ends where that changes; the others there are counted
/// as covered (`covered` on the drawn line).
pub(super) fn merge_lines(lines: Vec<Line>, eps: f64) -> Vec<Line> {
    type Events = (Vec2, Vec2, Vec<(f64, Option<Owner>, i32, i32, f64)>);
    let mut groups: BTreeMap<[i64; 3], Events> = BTreeMap::new();
    for line in lines {
        let delta = line.b - line.a;
        let mut dir = delta / delta.len();
        if (dir.x.abs() > 1e-8 && dir.x < 0.0) || (dir.x.abs() <= 1e-8 && dir.y < 0.0) {
            dir = -dir;
        }
        let offset = cross(dir, line.a);
        let k = [(dir.x / 1e-8).round() as i64, (dir.y / 1e-8).round() as i64, (offset / eps).round() as i64];
        let (origin, dir, events) = groups.entry(k).or_insert((line.a, dir, Vec::new()));
        let (a, b) = ((line.a - *origin).dot(*dir), (line.b - *origin).dot(*dir));
        let (v, h) = if line.hidden { (0, 1) } else { (1, 0) };
        events.push((a.min(b), line.owner, v, h, line.depth));
        events.push((a.max(b), line.owner, -v, -h, line.depth));
    }
    let mut result = Vec::new();
    for (_, (origin, dir, mut events)) in groups {
        events.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        // How many visible and hidden stretches of each owner lie here, and the nearest of their
        // depths (one entry without owners).
        let mut active: BTreeMap<Option<Owner>, (i32, i32, f64)> = BTreeMap::new();
        let mut last = 0.0;
        let mut path: Option<Line> = None;
        for (at, owner, v, h, depth) in events {
            if at - last > eps {
                // The nearest owner with a stretch of this visibility here; a later one only when
                // it is nearer by more than eps.
                let nearest = |hidden: bool| {
                    let here = active.iter().filter(|(_, (v, h, _))| if hidden { *h > 0 } else { *v > 0 });
                    here.fold(None, |best: Option<(Option<Owner>, f64)>, (o, (_, _, d))| match best {
                        Some((_, near)) if *d <= near + eps => best,
                        _ => Some((*o, *d)),
                    })
                };
                let drawn = nearest(false).map(|(o, d)| (o, false, d)).or_else(|| nearest(true).map(|(o, d)| (o, true, d)));
                if let Some((owner, is_hidden, depth)) = drawn {
                    let a = origin + dir * last;
                    let b = origin + dir * at;
                    let covered = active.len() > 1;
                    if let Some(p) = path.as_mut().filter(|p| p.hidden == is_hidden && p.owner == owner && p.b.dist(a) <= eps) {
                        p.b = b;
                        p.covered |= covered;
                    } else {
                        if let Some(p) = path.take() {
                            result.push(p);
                        }
                        path = Some(Line { a, b, hidden: is_hidden, owner, covered, depth });
                    }
                } else if let Some(p) = path.take() {
                    result.push(p);
                }
            }
            let count = active.entry(owner).or_insert((0, 0, f64::NEG_INFINITY));
            count.0 += v;
            count.1 += h;
            count.2 = count.2.max(depth);
            if (count.0, count.1) == (0, 0) {
                active.remove(&owner);
            }
            last = at;
        }
        if let Some(p) = path {
            result.push(p);
        }
    }
    result
}

/// B-rep edges plus curved silhouettes. Triangle adjacency is welded by position because
/// different faces can have different vertex indices along their shared boundary.
#[cfg(test)]
fn edges(mesh: &Mesh, cam: &Camera, eps: f64) -> Vec<(Vec3, Vec3)> {
    let mut result = Vec::new();
    for (_, e) in mesh.edges.iter().enumerate().filter(|(i, _)| mesh.shaded_edge_visible(*i)) {
        result.extend(e.windows(2).filter_map(|w| Some((*w.first()?, *w.get(1)?))));
    }
    let mut adjacent: BTreeMap<_, (Vec3, Vec3, bool, bool, usize)> = BTreeMap::new();
    for indices in &mesh.triangles {
        let Some([a, b, c]) = mesh.tri(indices) else { continue };
        let n = (b - a).cross(c - a);
        let toward = if cam.fov > 0.0 { cam.eye() - (a + b + c) / 3.0 } else { cam.basis().2 };
        let front = n.dot(toward) > 1e-3 * n.len() * toward.len();
        for (a, b) in [(a, b), (b, c), (c, a)] {
            let (ka, kb) = (key(a, eps), key(b, eps));
            let k = if ka < kb { (ka, kb) } else { (kb, ka) };
            let e = adjacent.entry(k).or_insert((a, b, false, false, 0));
            e.2 |= front;
            e.3 |= !front;
            e.4 += 1;
        }
    }
    for (_, (a, b, front, back, n)) in adjacent {
        if (front && back) || n == 1 {
            result.push((a, b));
        }
    }
    result
}

#[cfg(test)]
pub(super) fn reference_drawing(meshes: &[Mesh], cam: &Camera) -> Result<Vec<Line>, &'static str> {
    let (right, up, back) = cam.basis();
    let scale = meshes.iter().fold(Aabb3::EMPTY, |b, m| b.union(&m.bounds())).diagonal().max(1.0);
    let eps = scale * 1e-8;
    let project = |p: Vec3| -> Result<Vec3, &'static str> {
        if !p.is_finite() {
            return Err("model contains invalid coordinates");
        }
        let d = p - cam.target;
        let z = d.dot(back);
        let projected = if cam.fov > 0.0 {
            let w = cam.distance - z;
            if w <= cam.distance * 1e-6 {
                return Err("model crosses the camera eye plane; zoom out before exporting");
            }
            Vec3::new(d.dot(right) * cam.distance / w, d.dot(up) * cam.distance / w, 1.0 / w)
        } else {
            Vec3::new(d.dot(right), d.dot(up), z)
        };
        if !projected.is_finite() || projected.len() > 1e12 {
            return Err("projected coordinates exceed the export range");
        }
        Ok(projected)
    };
    let mut triangles = Vec::new();
    let mut segments = Vec::new();
    for m in meshes {
        for t in &m.triangles {
            if let Some([a, b, c]) = m.tri(t) {
                let p = [project(a)?, project(b)?, project(c)?];
                triangles.push(Triangle { p, bounds: bounds(p) });
            }
        }
        for (a, b) in edges(m, cam, eps) {
            let (a, b) = (project(a)?, project(b)?);
            if xy(a).dist(xy(b)) > eps {
                segments.push((a, b));
            }
        }
    }
    let tree = Tree::new(triangles);
    let depth_eps = if cam.fov > 0.0 { 1e-8 / cam.distance } else { eps };
    let mut result = Vec::new();
    let mut budget = 20_000_000usize;
    for (a, b) in segments {
        let mut spans = Vec::new();
        tree.intervals(a, b, &bounds([a, b]), depth_eps, &mut spans, &mut budget)?;
        spans.sort_unstable_by(|a, b| a.0.total_cmp(&b.0));
        let mut merged: Vec<(f64, f64)> = Vec::new();
        for (lo, hi) in spans {
            if let Some(last) = merged.last_mut().filter(|last| lo <= last.1 + 1e-9) {
                last.1 = last.1.max(hi);
            } else {
                merged.push((lo, hi));
            }
        }
        let mut emit = |lo: f64, hi: f64, hidden: bool| {
            let depth = a.z + (b.z - a.z) * (lo + hi) / 2.0;
            let (a, b) = (xy(a + (b - a) * lo), xy(a + (b - a) * hi));
            if a.dist(b) <= eps {
                return;
            }
            result.push(Line { a, b, hidden, owner: None, covered: false, depth });
        };
        let mut cursor = 0.0;
        for (lo, hi) in merged {
            emit(cursor, lo, false);
            emit(lo, hi, true);
            cursor = hi;
        }
        emit(cursor, 1.0, false);
        if result.len() > 1_000_000 {
            return Err("view too complex to export; hide some bodies");
        }
    }
    Ok(merge_lines(result, eps))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn a_visible_edge_overrides_only_the_overlapping_part_of_a_hidden_edge() {
        let line = |a: f64, b: f64, hidden| Line { a: Vec2::new(a, 0.), b: Vec2::new(b, 0.), hidden, owner: None, covered: false, depth: 0. };
        let merged = merge_lines(vec![line(0., 10., true), line(3., 7., false)], 1e-8);
        assert_eq!(merged.len(), 3);
        let length = |hidden| merged.iter().filter(|l| l.hidden == hidden).map(|l| l.a.dist(l.b)).sum::<f64>();
        assert!((length(false) - 4.).abs() < 1e-8 && (length(true) - 6.).abs() < 1e-8);
    }

    /// Two sources on one stretch: the first visible one draws it and the stretch says another lay
    /// there; where only a hidden source lies, the first hidden one draws it; a stretch ends where
    /// its owner changes, even at the same visibility.
    #[test]
    fn a_shared_stretch_belongs_to_its_first_visible_source() {
        let owner = |placement| Some(Owner { placement, origin: Origin::Edge(0) });
        let line = |a: f64, b: f64, hidden, placement| Line {
            a: Vec2::new(a, 0.),
            b: Vec2::new(b, 0.),
            hidden,
            owner: owner(placement),
            covered: false,
            depth: 0.,
        };
        let merged = merge_lines(vec![line(0., 10., true, 0), line(2., 6., false, 1), line(4., 8., false, 2), line(5., 9., true, 3)], 1e-8);
        let mut got: Vec<(i64, i64, bool, u32, bool)> = merged
            .iter()
            .map(|l| ((l.a.x * 1e6).round() as i64, (l.b.x * 1e6).round() as i64, l.hidden, l.owner.map_or(9, |o| o.placement), l.covered))
            .collect();
        got.sort_unstable();
        let mm = |v: i64| v * 1_000_000;
        let want =
            vec![(mm(0), mm(2), true, 0, false), (mm(2), mm(6), false, 1, true), (mm(6), mm(8), false, 2, true), (mm(8), mm(10), true, 0, true)];
        assert_eq!(got, want);
    }

    /// Two visible sources on one stretch at different depths: the nearer draws it whatever the
    /// order of their owners, and the farther is counted as covered; level within eps, the first.
    #[test]
    fn a_shared_stretch_belongs_to_its_nearest_source() {
        let line = |placement, depth| Line {
            a: Vec2::new(0., 0.),
            b: Vec2::new(10., 0.),
            hidden: false,
            owner: Some(Owner { placement, origin: Origin::Edge(0) }),
            covered: false,
            depth,
        };
        for (near, far) in [(0, 1), (1, 0)] {
            let merged = merge_lines(vec![line(far, 0.), line(near, 5.)], 1e-8);
            let got: Vec<(Option<u32>, bool, f64)> = merged.iter().map(|l| (l.owner.map(|o| o.placement), l.covered, l.depth)).collect();
            assert_eq!(got, [(Some(near), true, 5.)]);
        }
        let merged = merge_lines(vec![line(1, 5.), line(0, 5. + 1e-9)], 1e-8);
        assert_eq!(merged.iter().map(|l| l.owner.map(|o| o.placement)).collect::<Vec<_>>(), [Some(0)]);
    }
    #[test]
    fn a_line_is_split_at_triangle_boundaries_and_depth_crossings() {
        let t = [Vec3::new(0., 0., 1.), Vec3::new(2., 0., 1.), Vec3::new(0., 2., 1.)];
        let (lo, hi) = occluded(Vec3::new(-1., 0.5, 0.), Vec3::new(3., 0.5, 0.), t, 0.).unwrap();
        assert!((lo - 0.25).abs() < 1e-9 && (hi - 0.625).abs() < 1e-9);
        assert!(occluded(Vec3::new(0., 0.5, 2.), Vec3::new(1., 0.5, 2.), t, 0.).is_none());
        let (lo, hi) = occluded(Vec3::new(0., 0.5, 0.), Vec3::new(1., 0.5, 2.), t, 0.).unwrap();
        assert!(lo.abs() < 1e-9 && (hi - 0.5).abs() < 1e-9);
    }
}
