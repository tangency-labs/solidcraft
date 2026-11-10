use super::*;
use solidcore_record::brep::{Curve, Curve2, Edge, Face, OrientedEdge, Shell, VERSION, Vertex};

/// A body written face by face: each face is its plane (origin, u, v), its loops as corner lists
/// running counter-clockwise about u × v, and the side its shell uses.
struct Fixture {
    body: Brep,
    flip: bool,
}

impl Fixture {
    fn new() -> Self {
        Fixture {
            body: Brep {
                version: VERSION,
                tolerance: 1e-6,
                regions: Vec::new(),
                shells: Vec::new(),
                faces: Vec::new(),
                edges: Vec::new(),
                vertices: Vec::new(),
            },
            flip: false,
        }
    }

    fn vertex(&mut self, p: [f64; 3]) -> u32 {
        self.body.vertices.push(Vertex { point: p, tolerance: 1e-6 });
        self.body.vertices.len() as u32 - 1
    }

    /// A loop of line edges; every other edge is stored the other way round.
    fn lines(&mut self, pts: &[[f64; 3]]) -> Loop {
        let ids: Vec<u32> = pts.iter().map(|p| self.vertex(*p)).collect();
        let mut edges = Vec::new();
        for k in 0..pts.len() {
            let (a, b) = (k, (k + 1) % pts.len());
            self.flip = !self.flip;
            let (s, e) = if self.flip { (b, a) } else { (a, b) };
            self.body.edges.push(Edge {
                curve: Curve::Line { start: pts[s], end: pts[e] },
                nurbs: None,
                interval: [0.0, 1.0],
                vertices: [ids[s], ids[e]],
                tolerance: 1e-6,
                uses: Vec::new(),
            });
            edges.push(use_of(self.body.edges.len() as u32 - 1, !self.flip));
        }
        Loop::Edges { edges }
    }

    /// A loop of one closed polyline edge.
    fn polyline(&mut self, pts: &[[f64; 3]]) -> Loop {
        let v = self.vertex(pts[0]);
        let mut points = pts.to_vec();
        points.push(pts[0]);
        let n = points.len() as f64 - 1.0;
        self.body.edges.push(Edge {
            curve: Curve::Polyline { points },
            nurbs: None,
            interval: [0.0, n],
            vertices: [v, v],
            tolerance: 1e-6,
            uses: Vec::new(),
        });
        Loop::Edges { edges: vec![use_of(self.body.edges.len() as u32 - 1, true)] }
    }

    fn face(&mut self, surface: Surface, loops: Vec<Loop>, forward: bool) {
        self.body.faces.push(Face { surface, nurbs: None, domain: [[-1e3, 1e3], [-1e3, 1e3]], loops });
        let face = self.body.faces.len() as u32 - 1;
        if self.body.shells.is_empty() {
            self.body.shells.push(Shell::Faces { faces: Vec::new(), wires: Vec::new() });
        }
        if let Some(Shell::Faces { faces, .. }) = self.body.shells.first_mut() {
            faces.push(solidcore_record::brep::OrientedFace { face, forward });
        }
    }

    /// A prism from `z0` to `z1` over an outline running counter-clockwise seen from +Z, with
    /// holes running clockwise. The top is held with its plane upside down and used reversed.
    fn prism(outer: &[[f64; 2]], holes: &[Vec<[f64; 2]>], z: [f64; 2]) -> Brep {
        let mut f = Fixture::new();
        let at = |p: [f64; 2], h: f64| [p[0], p[1], h];
        let ring = |pts: &[[f64; 2]], h: f64| pts.iter().map(|p| at(*p, h)).collect::<Vec<_>>();
        let rev = |mut v: Vec<[f64; 3]>| {
            v.reverse();
            v
        };
        // Bottom: natural normal -Z, out of the material.
        let mut loops = vec![f.lines(&rev(ring(outer, z[0])))];
        loops.extend(holes.iter().map(|h| f.lines(&rev(ring(h, z[0])))).collect::<Vec<_>>());
        f.face(Surface::Plane { origin: [0.0, 0.0, z[0]], u_axis: [0.0, 1.0, 0.0], v_axis: [1.0, 0.0, 0.0] }, loops, true);
        // Top: natural normal -Z too, so the shell uses it reversed.
        let mut loops = vec![f.lines(&rev(ring(outer, z[1])))];
        loops.extend(holes.iter().map(|h| f.lines(&rev(ring(h, z[1])))).collect::<Vec<_>>());
        f.face(Surface::PlaneThrough { origin: [0.0, 0.0, z[1]], u_point: [0.0, 1.0, z[1]], v_point: [1.0, 0.0, z[1]] }, loops, false);
        for pts in std::iter::once(outer).chain(holes.iter().map(Vec::as_slice)) {
            for k in 0..pts.len() {
                let (a, b) = (pts[k], pts[(k + 1) % pts.len()]);
                let quad = [at(a, z[0]), at(b, z[0]), at(b, z[1]), at(a, z[1])];
                let lp = f.lines(&quad);
                let u = [b[0] - a[0], b[1] - a[1], 0.0];
                f.face(Surface::Plane { origin: at(a, z[0]), u_axis: u, v_axis: [0.0, 0.0, z[1] - z[0]] }, vec![lp], true);
            }
        }
        f.body
    }
}

fn use_of(edge: u32, forward: bool) -> OrientedEdge {
    OrientedEdge { edge, forward, pcurve: Curve2::Spline { knots: vec![0.0, 0.0, 1.0, 1.0], points: vec![[0.0, 0.0], [1.0, 0.0]] } }
}

fn rect(x0: f64, y0: f64, x1: f64, y1: f64) -> Vec<[f64; 2]> {
    vec![[x0, y0], [x1, y0], [x1, y1], [x0, y1]]
}

fn near(a: Vec3, b: [f64; 3]) -> bool {
    (a - Vec3::new(b[0], b[1], b[2])).len() < 1e-9
}

#[test]
fn a_box_has_six_sides_largest_first() {
    let sides = flat_sides(&Fixture::prism(&rect(0.0, 0.0, 40.0, 20.0), &[], [0.0, 10.0]));
    assert_eq!(sides.len(), 6, "{sides:?}");
    let areas: Vec<f64> = sides.iter().map(|s| s.area).collect();
    for (got, want) in areas.iter().zip([800.0, 800.0, 400.0, 400.0, 200.0, 200.0]) {
        assert!((got - want).abs() < 1e-9, "{areas:?}");
    }
    // The two largest are the bottom and the top, each facing out of the box.
    let big: Vec<&FlatSide> = sides.iter().take(2).collect();
    let bottom = big.iter().find(|s| near(s.normal, [0.0, 0.0, -1.0])).expect("a bottom");
    let top = big.iter().find(|s| near(s.normal, [0.0, 0.0, 1.0])).expect("a top");
    assert!(bottom.offset.abs() < 1e-9 && (top.offset - 10.0).abs() < 1e-9, "{bottom:?} {top:?}");
    let x = sides.iter().find(|s| near(s.normal, [1.0, 0.0, 0.0])).expect("a +X side");
    assert!((x.offset - 40.0).abs() < 1e-9 && (x.area - 200.0).abs() < 1e-9, "{x:?}");
}

#[test]
fn a_hole_through_a_plate_takes_its_area_off_both_faces() {
    let hole = {
        let mut h = rect(10.0, 5.0, 20.0, 15.0);
        h.reverse();
        h
    };
    let sides = flat_sides(&Fixture::prism(&rect(0.0, 0.0, 40.0, 20.0), &[hole], [0.0, 3.0]));
    let z: Vec<&FlatSide> = sides.iter().filter(|s| s.normal.z.abs() > 0.5).collect();
    assert_eq!(z.len(), 2);
    assert!(z.iter().all(|s| (s.area - 700.0).abs() < 1e-9), "{z:?}");
    // The hole's walls face into the hole, out of the material.
    let wall = sides.iter().find(|s| near(s.normal, [1.0, 0.0, 0.0]) && (s.offset - 10.0).abs() < 1e-9).expect("the hole's left wall");
    assert!((wall.area - 30.0).abs() < 1e-9, "{wall:?}");
}

#[test]
fn faces_in_one_plane_add_up() {
    // A U: the ends of its two arms lie in one plane.
    let u = [[0.0, 0.0], [30.0, 0.0], [30.0, 20.0], [20.0, 20.0], [20.0, 5.0], [10.0, 5.0], [10.0, 20.0], [0.0, 20.0]];
    let sides = flat_sides(&Fixture::prism(&u, &[], [0.0, 4.0]));
    let ends: Vec<&FlatSide> = sides.iter().filter(|s| near(s.normal, [0.0, 1.0, 0.0])).collect();
    assert_eq!(ends.len(), 2, "the arms' ends and the inside of the U: {ends:?}");
    let arms = ends.iter().find(|s| (s.offset - 20.0).abs() < 1e-9).expect("the arms' ends");
    assert!((arms.area - 80.0).abs() < 1e-9, "{arms:?}");
    let base = sides.first().expect("sides");
    assert!((base.area - (600.0 - 150.0)).abs() < 1e-9, "{base:?}");
}

#[test]
fn a_curved_edge_is_integrated_on_the_curve() {
    // A disc-like hole as one closed polyline of 48 corners: Gauss–Legendre is exact on each
    // straight piece, so the area is the polygon's to rounding.
    let n = 48;
    let r = 6.0;
    let hole: Vec<[f64; 3]> = (0..n)
        .map(|k| {
            let t = -std::f64::consts::TAU * f64::from(k) / f64::from(n);
            [20.0 + r * t.cos(), 10.0 + r * t.sin(), 0.0]
        })
        .collect();
    let polygon = 0.5 * f64::from(n) * r * r * (std::f64::consts::TAU / f64::from(n)).sin();
    let mut f = Fixture::new();
    let outer = f.lines(&[[0.0, 0.0, 0.0], [40.0, 0.0, 0.0], [40.0, 20.0, 0.0], [0.0, 20.0, 0.0]]);
    let inner = f.polyline(&hole);
    f.face(Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [0.0, 1.0, 0.0] }, vec![outer, inner], false);
    let sides = flat_sides(&f.body);
    assert_eq!(sides.len(), 1);
    assert!((sides[0].area - (800.0 - polygon)).abs() < 1e-9, "{:?} want {}", sides[0], 800.0 - polygon);
    assert!(near(sides[0].normal, [0.0, 0.0, -1.0]), "{:?}", sides[0]);
}

#[test]
fn broken_or_empty_bodies_give_no_sides_and_never_panic() {
    assert!(flat_sides(&Fixture::new().body).is_empty());
    let mut b = Fixture::prism(&rect(0.0, 0.0, 1.0, 1.0), &[], [0.0, 1.0]);
    b.vertices.iter_mut().for_each(|v| v.point = [f64::NAN; 3]);
    let _ = flat_sides(&b);
    let mut b = Fixture::prism(&rect(0.0, 0.0, 1.0, 1.0), &[], [0.0, 1.0]);
    b.edges.clear();
    assert!(flat_sides(&b).is_empty());
    let mut b = Fixture::prism(&rect(0.0, 0.0, 1.0, 1.0), &[], [0.0, 1.0]);
    for f in &mut b.faces {
        f.surface = Surface::Plane { origin: [0.0; 3], u_axis: [1.0, 0.0, 0.0], v_axis: [2.0, 0.0, 0.0] };
    }
    assert!(flat_sides(&b).is_empty());
}

#[test]
fn a_level_face_under_a_point_is_its_outline_and_holes() {
    let hole = {
        let mut h = rect(10.0, 5.0, 20.0, 15.0);
        h.reverse();
        h
    };
    let plate = Fixture::prism(&rect(0.0, 0.0, 40.0, 20.0), &[hole], [0.0, 3.0]);
    let top = level_face_at(&plate, Vec3::new(5.0, 5.0, 3.0), 1e-6).expect("the top face");
    assert!((top.z - 3.0).abs() < 1e-12 && top.error == 0.0, "{top:?}");
    assert_eq!((top.shape.outer.len(), top.shape.holes.len()), (4, 1), "{top:?}");
    let area = |r: &[Vec2]| (0..r.len()).map(|k| r[k].x * r[(k + 1) % r.len()].y - r[(k + 1) % r.len()].x * r[k].y).sum::<f64>().abs() / 2.0;
    assert!((area(&top.shape.outer) - 800.0).abs() < 1e-9 && (area(&top.shape.holes[0]) - 100.0).abs() < 1e-9);
    assert!(level_face_at(&plate, Vec3::new(5.0, 5.0, 0.0), 1e-6).is_none(), "the bottom looks down");
    assert!(level_face_at(&plate, Vec3::new(15.0, 10.0, 3.0), 1e-6).is_none(), "in the hole");
    assert!(level_face_at(&plate, Vec3::new(0.0, 10.0, 1.5), 1e-6).is_none(), "a side is not level");
    assert!(level_face_at(&plate, Vec3::new(5.0, 5.0, 2.0), 1e-6).is_none(), "inside the plate");
}

#[test]
fn a_pick_means_the_side_of_the_face_it_is_on() {
    let hole = {
        let mut h = rect(10.0, 5.0, 20.0, 15.0);
        h.reverse();
        h
    };
    let plate = Fixture::prism(&rect(0.0, 0.0, 40.0, 20.0), &[hole], [0.0, 3.0]);
    let side = |p: [f64; 3]| side_at(&plate, Vec3::new(p[0], p[1], p[2]), 1e-6).map(|s| s.normal);
    assert!(side([5.0, 5.0, 3.0]).is_some_and(|n| near(n, [0.0, 0.0, 1.0])));
    assert!(side([5.0, 5.0, 0.0]).is_some_and(|n| near(n, [0.0, 0.0, -1.0])));
    assert!(side([20.0, 0.0, 1.5]).is_some_and(|n| near(n, [0.0, -1.0, 0.0])), "a side wall");
    assert!(side([15.0, 5.0, 1.5]).is_some_and(|n| near(n, [0.0, 1.0, 0.0])), "the hole's wall, facing into it");
    assert_eq!(side([15.0, 10.0, 3.0]), None, "over the hole");
    assert_eq!(side([60.0, 10.0, 3.0]), None, "in the top's plane, off the face");
}
