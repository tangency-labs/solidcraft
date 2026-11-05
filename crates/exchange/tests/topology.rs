//! The snapshot contract: edges, corners and free-form faces.

mod common;

use std::collections::BTreeSet;
use std::f64::consts::PI;

use common::*;
use solidcraft_exchange::{Body, Bounds, ControlPoint, Curve, Edge, Knots, MeshVertex, Mm, Options, Point, Radians, Surface, Tolerance, open};

/// How far welding may move a point of the 30 mm plate (10⁻⁶ of its size, with margin).
const WELD: f64 = 1e-4;

/// The holed plate's edges: 12 box edges, the hole's 2 rim circles and the seam of its wall (one
/// cylinder face, closed along a line). Each is named, its curve is exact, its ends are corners,
/// and its path is a chain of triangle sides running along it.
#[test]
fn edges_are_named_exact_and_traced_by_the_mesh() {
    let s = snap_with(&holed_doc(), Vec::new()).unwrap();
    let b = &s.bodies[0];
    assert_eq!(b.edges.len(), 15, "{:?}", b.edges.iter().map(|e| &e.name).collect::<Vec<_>>());
    let names: BTreeSet<&str> = b.edges.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names.len(), b.edges.len(), "edge names are unique");
    let sides: BTreeSet<(MeshVertex, MeshVertex)> = b
        .faces
        .iter()
        .flat_map(|f| f.triangles.iter())
        .flat_map(|t| [(t[0], t[1]), (t[1], t[2]), (t[2], t[0])])
        .map(|(a, c)| (a.min(c), a.max(c)))
        .collect();
    let (mut lines, mut circles) = (0, 0);
    for (i, e) in b.edges.iter().enumerate() {
        assert_eq!(e.index.index(), i);
        let (f0, f1) = match e.bounds {
            Bounds::Between(f0, f1) => (f0, f1),
            Bounds::Seam(f) => (f, f),
        };
        assert!(e.path.len() >= 2, "{} has no path", e.name);
        assert!(e.path.windows(2).all(|w| sides.contains(&(w[0].min(w[1]), w[0].max(w[1])))), "{} runs along triangle sides", e.name);
        // Every side of the path lies between the edge's two faces.
        let between = |f: usize, v: MeshVertex, w: MeshVertex| {
            b.faces[f].triangles.iter().any(|t| (0..3).any(|k| (t[k], t[(k + 1) % 3]) == (v, w) || (t[k], t[(k + 1) % 3]) == (w, v)))
        };
        assert!(e.path.windows(2).all(|w| [f0, f1].iter().all(|f| between(f.index(), w[0], w[1]))), "{}", e.name);
        assert!(body_faces_bounded(b, e), "{}", e.name);
        // Its ends are corners, at the path's ends.
        let [start, end] = e.ends.unwrap_or_else(|| panic!("{} has no ends", e.name));
        assert!(dist(b.vertices[start.index()].point.0, at(b, e.path[0])) < WELD, "{}", e.name);
        assert!(dist(b.vertices[end.index()].point.0, at(b, *e.path.last().unwrap())) < WELD, "{}", e.name);
        match e.curve {
            Curve::Line { start: Point(s0), end: Point(s1) } => {
                lines += 1;
                assert!(dist(s0, at(b, e.path[0])) < WELD && dist(s1, at(b, *e.path.last().unwrap())) < WELD, "{}", e.name);
            }
            Curve::Circle { center: Point(c), axis, radius: Mm(r), angle: Radians(angle) } => {
                circles += 1;
                assert!((r - 3.0).abs() < 1e-9 && (c[0] - 10.0).abs() < 1e-9 && (c[1] - 10.0).abs() < 1e-9, "{}", e.name);
                assert!((axis.get()[2].abs() - 1.0).abs() < 1e-9 && (angle - 2.0 * PI).abs() < 1e-9, "{}: a full circle", e.name);
                for v in &e.path {
                    let p = at(b, *v);
                    assert!(((p[0] - 10.0).hypot(p[1] - 10.0) - 3.0).abs() < 1e-6, "{}", e.name);
                }
            }
            _ => panic!("{}: {:?}", e.name, e.curve),
        }
    }
    assert_eq!((lines, circles), (13, 2));
}

/// The edge is among the edges of each face it bounds, and its points are its path's positions.
fn body_faces_bounded(b: &Body, e: &Edge) -> bool {
    e.bounds.faces().all(|f| b.edges_of(f).any(|x| x.index == e.index)) && e.points(b).count() == e.path.len()
}

/// The corners: the box's 8 and the ends of the hole's seam. A box corner is where three
/// faces meet; each corner is a mesh vertex; names are unique.
#[test]
fn corners_are_where_edges_end() {
    let s = snap_with(&holed_doc(), Vec::new()).unwrap();
    let b = &s.bodies[0];
    assert_eq!(b.vertices.len(), 10, "{:?}", b.vertices);
    let names: BTreeSet<&str> = b.vertices.iter().map(|v| v.name.as_str()).collect();
    assert_eq!(names.len(), 10);
    for (k, v) in b.vertices.iter().enumerate() {
        assert_eq!(v.index.index(), k);
        let at_mesh = v.position.unwrap_or_else(|| panic!("{} has no mesh vertex", v.name));
        assert!(dist(at(b, at_mesh), v.point.0) < WELD, "{}", v.name);
        assert_eq!(b.edges_at(v.index).count(), v.edges.len());
        for e in b.edges_at(v.index) {
            assert!(e.ends.is_some_and(|ends| ends.contains(&v.index)), "{} {}", v.name, e.name);
        }
    }
    let corner = b.vertices.iter().find(|v| dist(v.point.0, [0.0, 0.0, 0.0]) < 1e-9).unwrap();
    assert_eq!((corner.name.as_str().split('|').count(), corner.edges.len()), (3, 3), "{}", corner.name);
}

/// A point of a NURBS patch (Cox–de Boor), and ∂S/∂u × ∂S/∂v by central differences.
struct Patch<'a> {
    du: usize,
    dv: usize,
    ku: &'a [f64],
    kv: &'a [f64],
    net: &'a [Vec<ControlPoint>],
}

fn basis(knots: &[f64], degree: usize, i: usize, t: f64) -> f64 {
    if degree == 0 {
        let (a, b) = (knots[i], knots[i + 1]);
        let last = knots[knots.len() - 1];
        return if (a <= t && t < b) || (t == last && b == last && a < b) { 1.0 } else { 0.0 };
    }
    let mut v = 0.0;
    let d1 = knots[i + degree] - knots[i];
    if d1 > 0.0 {
        v += (t - knots[i]) / d1 * basis(knots, degree - 1, i, t);
    }
    let d2 = knots[i + degree + 1] - knots[i + 1];
    if d2 > 0.0 {
        v += (knots[i + degree + 1] - t) / d2 * basis(knots, degree - 1, i + 1, t);
    }
    v
}

impl Patch<'_> {
    fn range(&self) -> ([f64; 2], [f64; 2]) {
        let (nu, nv) = (self.net.len(), self.net[0].len());
        ([self.ku[self.du], self.ku[nu]], [self.kv[self.dv], self.kv[nv]])
    }

    fn at(&self, u: f64, v: f64) -> [f64; 3] {
        let (mut sum, mut w) = ([0.0; 3], 0.0);
        for (i, row) in self.net.iter().enumerate() {
            let bu = basis(self.ku, self.du, i, u);
            if bu == 0.0 {
                continue;
            }
            for (j, c) in row.iter().enumerate() {
                let p = c.point.0;
                let b = bu * basis(self.kv, self.dv, j, v) * c.weight;
                for k in 0..3 {
                    sum[k] += b * p[k];
                }
                w += b;
            }
        }
        sum.map(|c| c / w)
    }

    /// The parameters of the patch point nearest `p`: the nearest of a grid with 16 steps per knot
    /// span (a patch can hold a whole closed outline, many spans long), then coarse to fine
    /// around it.
    fn nearest(&self, p: [f64; 3]) -> (f64, f64) {
        let ([u0, u1], [v0, v1]) = self.range();
        let steps = |knots: &[f64]| 16 * knots.windows(2).filter(|w| w[1] > w[0]).count().max(1);
        let (nu, nv) = (steps(self.ku), steps(self.kv));
        let mut best = (u0, v0);
        let mut nearest = f64::INFINITY;
        for i in 0..=nu {
            for j in 0..=nv {
                let (u, v) = (u0 + (u1 - u0) * i as f64 / nu as f64, v0 + (v1 - v0) * j as f64 / nv as f64);
                let d = dist(self.at(u, v), p);
                if d < nearest {
                    (nearest, best) = (d, (u, v));
                }
            }
        }
        let mut span = ((u1 - u0) / nu as f64, (v1 - v0) / nv as f64);
        for _ in 0..12 {
            let mut found = (f64::INFINITY, best);
            for i in -10..=10 {
                for j in -10..=10 {
                    let u = (best.0 + span.0 * f64::from(i) / 10.0).clamp(u0, u1);
                    let v = (best.1 + span.1 * f64::from(j) / 10.0).clamp(v0, v1);
                    let d = dist(self.at(u, v), p);
                    if d < found.0 {
                        found = (d, (u, v));
                    }
                }
            }
            best = found.1;
            span = (span.0 / 4.0, span.1 / 4.0);
        }
        best
    }

    fn normal(&self, u: f64, v: f64) -> [f64; 3] {
        let ([u0, u1], [v0, v1]) = self.range();
        let (hu, hv) = ((u1 - u0) * 1e-5, (v1 - v0) * 1e-5);
        let su = sub(self.at((u + hu).min(u1), v), self.at((u - hu).max(u0), v));
        let sv = sub(self.at(u, (v + hv).min(v1)), self.at(u, (v - hv).max(v0)));
        cross(su, sv)
    }
}

/// A closed spline outline extruded 15 mm: its side is one B-spline patch round the whole outline
/// (one knot span per piece of the outline), closed by a straight seam. Every mesh
/// vertex of such a face lies on its patch, and the patch's normal points the way the face's
/// triangles do (out of the material).
#[test]
fn free_form_faces_are_nurbs_patches_their_mesh_lies_on() {
    let bytes = include_bytes!("fixtures/spline-extrude.json");
    let s = open(bytes).unwrap().snapshot(&Options::default().with_tolerance(Tolerance::Absolute(0.01))).unwrap();
    let b = &s.bodies[0];
    let mut patches = 0;
    for f in &b.faces {
        let Surface::Nurbs { u: Knots { degree: du, knots: ku, .. }, v: Knots { degree: dv, knots: kv, .. }, net } = &f.surface else { continue };
        patches += 1;
        let patch = Patch { du: *du as usize, dv: *dv as usize, ku, kv, net };
        for v in f.mesh_vertices() {
            let p = at(b, v);
            let (u, w) = patch.nearest(p);
            assert!(dist(patch.at(u, w), p) < 1e-6, "{}: a vertex is {} off its patch", f.name, dist(patch.at(u, w), p));
        }
        for (_, [Point(a), Point(c), Point(d)]) in b.triangles().filter(|(on, _)| on.index == f.index) {
            let centre = [(a[0] + c[0] + d[0]) / 3.0, (a[1] + c[1] + d[1]) / 3.0, (a[2] + c[2] + d[2]) / 3.0];
            let (u, w) = patch.nearest(centre);
            assert!(dot(patch.normal(u, w), cross(sub(c, a), sub(d, a))) > 0.0, "{}: the patch faces into the material", f.name);
        }
    }
    assert_eq!(patches, 1, "{:?}", b.faces.iter().map(|f| (&f.name, &f.surface)).collect::<Vec<_>>());
    // The outlines and the seam are B-splines in the record (the seam a straight one): no line
    // or circle.
    assert_eq!(b.edges.iter().filter(|e| matches!(e.curve, Curve::Other)).count(), 3);
    assert!(b.edges.iter().all(|e| !e.path.is_empty()), "every edge is traced");
}


