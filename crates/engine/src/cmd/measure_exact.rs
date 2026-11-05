//! What a face measures on the surface behind it (#9086), and around it (#12048).
//!
//! A curved face's area is the area of the surface it lies on: a cylinder's band is
//! r·θ·h and a whole sphere 4πr². The display mesh's triangles run inside a curve, so
//! their sum falls short of it.
//!
//! A flat face's area is the area its exact edges enclose (#12372): its outer loop's area
//! less its holes'. The display mesh's polygon for a round hole runs inside the circle, so
//! the mesh's holes are too small and the face reads too big.

use std::f64::consts::PI;

use solidcore_record::Vec3;
use solidcore_record::brep::{Curve, Loop};
use solidcraft_geom::prelude::*;
use solidcraft_geom::{Mesh, UnitVec3};
use solidcraft_kernel::{Body, FaceSurface};

/// The area of the patch of faces `patch` of `body`, worked out on the surface behind
/// them: a flat patch, a cylinder's band, or a sphere. `None` where the surface does not
/// give one, and the caller keeps the measure of the mesh.
pub(crate) fn patch_area(body: &Body, mesh: &Mesh, patch: &[usize]) -> Option<f64> {
    match solidcraft_kernel::face_surface(body, *patch.first()?)? {
        FaceSurface::Plane { normal, point } => flat_patch(body, patch, normal, point),
        FaceSurface::Cylinder { origin, axis, radius } => cylinder_band(body, mesh, patch, origin, axis, radius),
        FaceSurface::Sphere { radius, .. } => sphere(body, patch, radius),
        _ => None,
    }
}

/// Two orthonormal directions in a plane and a point on it: the plane's own 2D coordinates.
struct PlaneFrame {
    origin: Vec3,
    normal: Vec3,
    x: Vec3,
    y: Vec3,
}

impl PlaneFrame {
    fn new(normal: UnitVec3, origin: Vec3) -> Option<PlaneFrame> {
        let n = normal.vec();
        // The world axis least along the normal gives a direction across it.
        let across = if n.x.abs() <= n.y.abs() && n.x.abs() <= n.z.abs() {
            Vec3::X
        } else if n.y.abs() <= n.z.abs() {
            Vec3::Y
        } else {
            Vec3::Z
        };
        let x = n.cross(across).normalized()?;
        Some(PlaneFrame { origin, normal: n, x, y: n.cross(x) })
    }

    /// The 2D coordinates of point `p`.
    fn point(&self, p: Vec3) -> (f64, f64) {
        let d = p - self.origin;
        (d.dot(self.x), d.dot(self.y))
    }

    /// The 2D components of direction `d`.
    fn dir(&self, d: Vec3) -> (f64, f64) {
        (d.dot(self.x), d.dot(self.y))
    }
}

/// The area of a flat patch: every face of it lies on one plane, and each face's area is the
/// area its loops of exact edges enclose.
fn flat_patch(body: &Body, patch: &[usize], normal: UnitVec3, point: Vec3) -> Option<f64> {
    let tol = (body.size() * 1e-9).max(1e-9);
    let frame = PlaneFrame::new(normal, point)?;
    let mut total = 0.0;
    for &face in patch {
        match solidcraft_kernel::face_surface(body, face)? {
            FaceSurface::Plane { normal: n, point: p }
                if n.vec().dot(frame.normal).abs() >= 1.0 - 1e-9 && (p - point).dot(frame.normal).abs() <= tol => {}
            _ => return None,
        }
        total += flat_face(body, face, &frame, tol)?;
    }
    (total.is_finite() && total > 0.0).then_some(total)
}

/// The area of flat face `face`: the area its outer loop encloses, less the areas its holes
/// enclose. The outer loop is the one that encloses the most: every hole lies inside it.
fn flat_face(body: &Body, face: usize, frame: &PlaneFrame, tol: f64) -> Option<f64> {
    let mut loops: Vec<f64> = Vec::new();
    for l in &body.record().faces.get(face)?.loops {
        let Loop::Edges { edges } = l else { continue };
        let mut enclosed = 0.0;
        for used in edges {
            enclosed += edge_sweep(body, used.edge as usize, used.forward, frame, tol)?;
        }
        loops.push(enclosed.abs());
    }
    let outer = loops.iter().copied().fold(0.0, f64::max);
    let area = outer - (loops.iter().sum::<f64>() - outer);
    (area.is_finite() && area > 0.0).then_some(area)
}

/// Four-point Gauss–Legendre nodes and weights on `[−1, 1]`.
const GAUSS4: [(f64, f64); 4] = [
    (-0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
    (-0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.339_981_043_584_856_3, 0.652_145_154_862_546_2),
    (0.861_136_311_594_052_6, 0.347_854_845_137_453_8),
];

/// Gauss panels along an edge that is neither a line nor a circle.
const SWEEP_PANELS: usize = 64;

/// The signed area that edge `edge` sweeps about the frame's origin, `½∫(x dy − y dx)`, run
/// along the loop's direction (`forward`: from the edge's start to its end). Summed round a
/// loop, it is the area the loop encloses (by Green's theorem). A circle or an arc of one is
/// worked out in closed form: the triangle to its chord and the circular segment beyond it.
/// `None` for an edge off the plane, or one whose curve gives no derivative.
fn edge_sweep(body: &Body, edge: usize, forward: bool, frame: &PlaneFrame, tol: f64) -> Option<f64> {
    let triangle = |a: Vec3, b: Vec3| {
        let ((ax, ay), (bx, by)) = (frame.point(a), frame.point(b));
        0.5 * (ax * by - bx * ay)
    };
    if let Some(c) = solidcraft_kernel::edge_circle(body, edge) {
        let along = c.axis.vec().dot(frame.normal);
        if along.abs() < 1.0 - 1e-9 || (c.center - frame.origin).dot(frame.normal).abs() > tol {
            return None;
        }
        // The arc runs counter-clockwise about its axis from its start to its end.
        let (a, b, turn) = if forward { (c.start, c.end, along.signum()) } else { (c.end, c.start, -along.signum()) };
        let segment = 0.5 * c.radius * c.radius * (c.angle - c.angle.sin());
        return Some(triangle(a, b) + turn * segment);
    }
    let e = body.record().edges.get(edge)?;
    if matches!(e.curve, Curve::Line { .. }) {
        let c = solidcraft_geom::brep::eval::CurveEval::new(e.parameters()).ok()?;
        let [t0, t1] = e.interval;
        let (a, b) = (c.point(t0)?, c.point(t1)?);
        let off = |p: Vec3| (p - frame.origin).dot(frame.normal).abs() > tol.max(e.tolerance);
        if off(a) || off(b) {
            return None;
        }
        return Some(if forward { triangle(a, b) } else { triangle(b, a) });
    }
    let c = solidcraft_geom::brep::eval::CurveEval::new(e.parameters()).ok()?;
    let [t0, t1] = e.interval;
    let h = (t1 - t0) / SWEEP_PANELS as f64;
    let mut swept = 0.0;
    for panel in 0..SWEEP_PANELS {
        let start = t0 + h * panel as f64;
        for (x, w) in GAUSS4 {
            let (p, d) = c.derivs(start + 0.5 * h * (x + 1.0))?;
            if (p - frame.origin).dot(frame.normal).abs() > tol.max(e.tolerance) {
                return None;
            }
            let ((px, py), (dx, dy)) = (frame.point(p), frame.dir(d));
            swept += 0.5 * h * w * 0.5 * (px * dy - py * dx);
        }
    }
    let swept = if forward { swept } else { -swept };
    swept.is_finite().then_some(swept)
}

/// The edges on the border of the patch: the ones a face of the patch shares with a face
/// outside it, and the ones it keeps to itself.
fn border_edges(mesh: &Mesh, patch: &[usize]) -> Vec<usize> {
    mesh.edge_faces
        .iter()
        .enumerate()
        .filter(|(_, faces)| {
            let inside = faces.iter().filter(|f| patch.contains(&(**f as usize))).count();
            inside > 0 && (faces.len() == 1 || inside < faces.len())
        })
        .map(|(edge, _)| edge)
        .collect()
}

/// The length of the edges around a face.
pub(crate) struct Perimeter {
    pub(crate) total: f64,
    /// The outer loop's length and the summed length of the hole loops, on a flat face with holes.
    pub(crate) outer_inner: Option<(f64, f64)>,
}

/// The perimeter of the patch of faces `patch`: the length of its drawn border edges, each as
/// long as `edge_len` says. A hidden seam is no border. `None` for a face with no border, such as
/// a whole sphere. Only a flat face tells an outer loop from its holes: the two rims of a
/// cylinder's side are neither.
pub(crate) fn patch_perimeter(mesh: &Mesh, patch: &[usize], flat: bool, edge_len: impl Fn(usize) -> f64) -> Option<Perimeter> {
    let edges: Vec<usize> = border_edges(mesh, patch).into_iter().filter(|&edge| mesh.edge_visible(edge)).collect();
    let lengths: Vec<f64> = edges.iter().map(|&edge| edge_len(edge)).collect();
    let total: f64 = lengths.iter().sum();
    if !(total > 0.0) {
        return None;
    }
    let outer_inner = flat.then(|| outer_loop_length(mesh, &edges, &lengths)).flatten().map(|outer| (outer, total - outer));
    Some(Perimeter { total, outer_inner })
}

/// The length of the outer loop of a flat face's border `edges`, when they form several loops.
/// The outer loop is the one with the widest box: every hole lies inside the outer loop, so a
/// hole's box fits in the outer loop's box, even when the hole's own length is the greater.
fn outer_loop_length(mesh: &Mesh, edges: &[usize], lengths: &[f64]) -> Option<f64> {
    let polylines: Vec<&Vec<Vec3>> = edges.iter().map(|&edge| mesh.edges.get(edge)).collect::<Option<_>>()?;
    let mut loop_of: Vec<usize> = (0..edges.len()).collect();
    // Two edges in one loop share an end. The ends are sorted along X and each one meets only
    // its close neighbours, so a face with many edges stays cheap.
    let tol = mesh.bounds().diagonal().max(1.0) * 1e-6;
    let mut ends: Vec<(Vec3, usize)> =
        polylines.iter().enumerate().flat_map(|(i, p)| [p.first(), p.last()].into_iter().flatten().map(move |end| (*end, i))).collect();
    ends.sort_by(|a, b| a.0.x.total_cmp(&b.0.x));
    for (i, (end, edge)) in ends.iter().enumerate() {
        for (other, other_edge) in ends.iter().skip(i + 1).take_while(|(other, _)| other.x - end.x <= tol) {
            if end.dist(*other) <= tol {
                let (a, b) = (loop_root(&loop_of, *edge), loop_root(&loop_of, *other_edge));
                if let Some(slot) = loop_of.get_mut(a) {
                    *slot = b;
                }
            }
        }
    }
    let roots: Vec<usize> = (0..edges.len()).map(|edge| loop_root(&loop_of, edge)).collect();
    let mut loops = roots.clone();
    loops.sort_unstable();
    loops.dedup();
    if loops.len() < 2 {
        return None;
    }
    let width = |id: usize| -> f64 {
        let mut points = polylines.iter().zip(&roots).filter(|(_, r)| **r == id).flat_map(|(p, _)| p.iter());
        let Some(first) = points.next() else { return 0.0 };
        let (mut low, mut high) = (*first, *first);
        for p in points {
            (low.x, low.y, low.z) = (low.x.min(p.x), low.y.min(p.y), low.z.min(p.z));
            (high.x, high.y, high.z) = (high.x.max(p.x), high.y.max(p.y), high.z.max(p.z));
        }
        low.dist(high)
    };
    let outer = loops.into_iter().max_by(|a, b| width(*a).total_cmp(&width(*b)))?;
    Some(lengths.iter().zip(&roots).filter(|(_, r)| **r == outer).map(|(length, _)| *length).sum())
}

/// The loop `edge` belongs to, as the edge that stands for that loop.
fn loop_root(loop_of: &[usize], mut edge: usize) -> usize {
    while let Some(&next) = loop_of.get(edge) {
        if next == edge {
            break;
        }
        edge = next;
    }
    edge
}

/// Whether edge `edge` is a straight line parallel to `axis`.
fn straight_along(body: &Body, edge: usize, axis: UnitVec3) -> bool {
    let Some(e) = body.record().edges.get(edge) else { return false };
    if !matches!(e.curve, Curve::Line { .. }) {
        return false;
    }
    let Ok(c) = solidcraft_geom::brep::eval::CurveEval::new(e.parameters()) else { return false };
    let [t0, t1] = e.interval;
    let (Some(a), Some(b)) = (c.point(t0), c.point(t1)) else { return false };
    (b - a).normalized().is_some_and(|d| d.dot(axis.vec()).abs() >= 1.0 - 1e-9)
}

/// The area of a cylindrical band: the radius of the surface, the turn the band covers
/// and the distance between its two rims.
fn cylinder_band(body: &Body, mesh: &Mesh, patch: &[usize], origin: Vec3, axis: UnitVec3, radius: f64) -> Option<f64> {
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    let tol = radius * 1e-6;
    // The whole patch lies on one cylinder.
    for f in patch {
        match solidcraft_kernel::face_surface(body, *f)? {
            FaceSurface::Cylinder { origin: o, axis: a, radius: r } => {
                if (r - radius).abs() > tol || o.dist(origin) > tol || a.vec().dot(axis.vec()).abs() < 1.0 - 1e-9 {
                    return None;
                }
            }
            _ => return None,
        }
    }
    // The rims of the band: the circles that bound it, each across the axis.
    let mut rims: Vec<(f64, f64)> = Vec::new();
    for edge in border_edges(mesh, patch) {
        // An edge that is not a circle but a straight line along the axis (a seam, a straight
        // cut along the band) leaves the turn of the band to its rims alone. Any other edge
        // (the saddle rim of a bore through the wall) cuts the band: not ours to measure.
        let Some(circle) = solidcraft_kernel::edge_circle(body, edge) else {
            if straight_along(body, edge, axis) {
                continue;
            }
            return None;
        };
        if circle.axis.vec().dot(axis.vec()).abs() < 1.0 - 1e-9 || (circle.radius - radius).abs() > tol {
            return None;
        }
        let d = circle.center - origin;
        let height = d.dot(axis.vec());
        if (d - axis.vec() * height).len() > tol {
            return None;
        }
        rims.push((height, circle.angle));
    }
    // The two rims: the band runs between them, and both cover the same turn.
    let mut levels: Vec<(f64, f64)> = Vec::new();
    for (height, angle) in rims {
        match levels.iter_mut().find(|(h, _)| (*h - height).abs() <= tol) {
            Some((_, a)) => *a += angle,
            None => levels.push((height, angle)),
        }
    }
    if levels.len() != 2 {
        return None;
    }
    levels.sort_by(|a, b| a.0.total_cmp(&b.0));
    let (low, low_turn) = levels[0];
    let (high, high_turn) = levels[1];
    let height = high - low;
    if !(height > 0.0) || (low_turn - high_turn).abs() > low_turn.max(high_turn) * 1e-6 {
        return None;
    }
    Some(radius * low_turn * height)
}

/// The area of a sphere: 4πr², when the body is the whole of it.
fn sphere(body: &Body, patch: &[usize], radius: f64) -> Option<f64> {
    if !(radius.is_finite() && radius > 0.0) {
        return None;
    }
    // The whole patch lies on one sphere.
    for f in patch {
        match solidcraft_kernel::face_surface(body, *f)? {
            FaceSurface::Sphere { radius: r, .. } if (r - radius).abs() <= radius * 1e-6 => {}
            _ => return None,
        }
    }
    // The body is the whole sphere: every face of it lies on that sphere and the patch
    // holds them all. A body with a face of its own (a sphere that was cut) is a piece of
    // the sphere, whose area this does not work out: the measure of the mesh stands in
    // for it.
    for f in 0..body.record().faces.len() {
        match solidcraft_kernel::face_surface(body, f)? {
            FaceSurface::Sphere { radius: r, .. } if (r - radius).abs() <= radius * 1e-6 => {}
            _ => return None,
        }
        if !patch.contains(&f) {
            return None;
        }
    }
    Some(4.0 * PI * radius * radius)
}
