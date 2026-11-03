//! A cut or a join never splits faces it doesn't touch: a flat wall with an opening cut into
//! it stays one face with an inner loop, whatever is cut or joined elsewhere afterwards.

use solidcraft_geom::Vec3;

use crate::{Body, BoolOp, FaceSurface, boolean, box_solid, face_surface, fillet, shell};

/// The faces of `b` lying in the plane through `point` with outward normal `normal`.
fn faces_on(b: &Body, normal: Vec3, point: Vec3) -> usize {
    (0..b.face_count())
        .filter(|&i| match face_surface(b, i) {
            Some(FaceSurface::Plane { normal: n, point: p }) => n.dot(normal) > 1. - 1e-9 && (p - point).dot(normal).abs() < 1e-6,
            _ => false,
        })
        .count()
}

/// The five outer sides of the open case: -y, +y, -x, +x and the bottom.
fn sides(w: f64, d: f64) -> [(Vec3, Vec3); 5] {
    [
        (Vec3::new(0., -1., 0.), Vec3::new(0., -d / 2., 0.)),
        (Vec3::new(0., 1., 0.), Vec3::new(0., d / 2., 0.)),
        (Vec3::new(-1., 0., 0.), Vec3::new(-w / 2., 0., 0.)),
        (Vec3::new(1., 0., 0.), Vec3::new(w / 2., 0., 0.)),
        (Vec3::new(0., 0., -1.), Vec3::new(0., 0., 0.)),
    ]
}

/// A Raspberry Pi case: a 95 x 66 box of height `h`, its upright edges rounded (5 mm) and
/// shelled 2 mm with the top open.
fn case(h: f64) -> Body {
    let b = box_solid(Vec3::new(-47.5, -33., 0.), Vec3::new(47.5, 33., h)).unwrap().with_box_certificate(7).unwrap();
    let corners = [(47.5, -33.), (-47.5, -33.), (47.5, 33.), (-47.5, 33.)].map(|(x, y)| Vec3::new(x, y, h / 2.));
    let rounded = fillet(&b, &corners, 5.).unwrap().with_preserved_planar_sources(&b).unwrap();
    shell(&rounded, &[Vec3::new(0., 0., h)], 2.).unwrap().with_shell_certificate(9).unwrap()
}

fn cut(b: &Body, lo: Vec3, hi: Vec3) -> Body {
    boolean(b, &box_solid(lo, hi).unwrap(), BoolOp::Cut).unwrap().unwrap()
}

fn assert_sides(b: &Body, h: f64, what: &str) {
    for (n, p) in sides(95., 66.) {
        assert_eq!(faces_on(b, n, p), 1, "{what} (height {h}): the outer side facing {n:?} is split");
    }
}

/// The README demo's case, at the original height and taller: ports through the long side and
/// the end, then vent slots through the floor. Every outer side stays one face, each opening
/// adds exactly its four sides, and every cut reports its exact history (so faces keep their
/// names): a wall with openings meets the openings' floors along several edges.
#[test]
fn pi_case_ports_and_vents_leave_the_walls_whole() {
    for h in [24., 32.] {
        let mut b = case(h);
        let mut faces = b.face_count();
        assert_sides(&b, h, "the shelled case");
        // USB-C and two micro-HDMI on the long side, sketched on its outer face and cut through
        // the wall (both faces of the tool on the wall's).
        for (x0, x1) in [(-36.5, -26.5), (-20.5, -12.5), (-7., 1.)] {
            b = cut(&b, Vec3::new(x0, -33., 7.5), Vec3::new(x1, -31., 11.5));
            faces += 4;
            assert!(b.history().is_some(), "port at x {x0}: the cut reports its history");
            assert_eq!(b.face_count(), faces, "port at x {x0} (height {h})");
            assert_sides(&b, h, "a port on the long side");
        }
        // The USB / Ethernet opening on the end.
        b = cut(&b, Vec3::new(45.5, -25., 8.), Vec3::new(47.5, 25., 22.));
        faces += 4;
        assert!(b.history().is_some(), "end opening: the cut reports its history");
        assert_eq!(b.face_count(), faces, "end opening (height {h})");
        assert_sides(&b, h, "the end opening");
        // Five vent slots through the floor, from its inner face to the bottom.
        for k in 0..5 {
            let y = -15.5 + 7. * k as f64;
            b = cut(&b, Vec3::new(-22., y, 0.), Vec3::new(6., y + 3., 2.));
            faces += 4;
            assert!(b.history().is_some(), "vent slot {k}: the cut reports its history");
            assert_eq!(b.face_count(), faces, "vent slot {k} (height {h})");
            assert_sides(&b, h, "a floor vent");
        }
    }
}

/// A small deterministic generator (no test dependency needed).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> f64 {
        self.0 = self.0.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (self.0 >> 11) as f64 / (1u64 << 53) as f64
    }
    fn range(&mut self, lo: f64, hi: f64) -> f64 {
        lo + (hi - lo) * self.next()
    }
}

/// Random openings through the walls and floor and bosses joined on the walls (sunk 1 mm into
/// the 2 mm wall), each on its
/// own patch of a side so none meets another: no side ever gains a face, whichever sides the
/// tools touch, and each tool adds exactly the faces it brings (an opening its four sides, a
/// boss its four sides and its end).
#[test]
fn random_cuts_and_joins_never_split_other_faces() {
    for seed in 1..=6u64 {
        let mut rng = Lcg(seed);
        let h = 24. + 8. * (seed % 2) as f64;
        let mut b = case(h);
        let mut faces = b.face_count();
        // Free patches: on the long sides x in [-40, 40] split in four, on the ends y in [-26,
        // 26] split in two, on the floor x in [-40, 40] by y in [-26, 26] split in four.
        let mut patches: Vec<(usize, usize)> =
            (0..4).flat_map(|k| [(0, k), (1, k)]).chain((0..2).flat_map(|k| [(2, k), (3, k)])).chain((0..4).map(|k| (4, k))).collect();
        for step in 0..8 {
            let pick = (rng.next() * patches.len() as f64) as usize % patches.len();
            let (side, k) = patches.swap_remove(pick);
            let join = side < 4 && rng.next() < 0.4;
            let (lo, hi) = match side {
                0 | 1 => {
                    let x0 = -40. + 20. * k as f64;
                    let (a, c) = (rng.range(x0 + 1., x0 + 8.), rng.range(x0 + 11., x0 + 19.));
                    let (z0, z1) = (rng.range(4., h / 2. - 1.), rng.range(h / 2. + 1., h - 4.));
                    let y: f64 = if side == 0 { -33. } else { 33. };
                    let (inside, outside) = if join { (y - y.signum(), y + 3. * y.signum()) } else { (y - 2. * y.signum(), y) };
                    (Vec3::new(a, inside.min(outside), z0), Vec3::new(c, inside.max(outside), z1))
                }
                2 | 3 => {
                    let y0 = -26. + 26. * k as f64;
                    let (a, c) = (rng.range(y0 + 1., y0 + 10.), rng.range(y0 + 15., y0 + 25.));
                    let (z0, z1) = (rng.range(4., h / 2. - 1.), rng.range(h / 2. + 1., h - 4.));
                    let x: f64 = if side == 2 { -47.5 } else { 47.5 };
                    let (inside, outside) = if join { (x - x.signum(), x + 3. * x.signum()) } else { (x - 2. * x.signum(), x) };
                    (Vec3::new(inside.min(outside), a, z0), Vec3::new(inside.max(outside), c, z1))
                }
                _ => {
                    let (x0, y0) = (-40. + 40. * (k % 2) as f64, -26. + 26. * (k / 2) as f64);
                    let (a, c) = (rng.range(x0 + 2., x0 + 15.), rng.range(x0 + 25., x0 + 38.));
                    let (e, f) = (rng.range(y0 + 2., y0 + 10.), rng.range(y0 + 15., y0 + 24.));
                    (Vec3::new(a, e, 0.), Vec3::new(c, f, 2.))
                }
            };
            let tool = box_solid(lo, hi).unwrap();
            b = boolean(&b, &tool, if join { BoolOp::Union } else { BoolOp::Cut }).unwrap().unwrap();
            faces += if join { 5 } else { 4 };
            let what = format!("seed {seed} step {step}: {} on side {side}", if join { "boss" } else { "opening" });
            assert_sides(&b, h, &what);
            assert_eq!(b.face_count(), faces, "{what} (height {h})");
        }
    }
}

/// The edges of `b` lying along the line through `a` and `z`.
fn edges_along(b: &Body, a: Vec3, z: Vec3) -> usize {
    let r = b.record();
    let on = |p: Vec3| p.dist_to_segment(a, z) < 1e-6;
    let point = |v: u32| Vec3::from(r.vertices[v as usize].point);
    r.edges.iter().filter(|e| on(point(e.vertices[0])) && on(point(e.vertices[1]))).count()
}

/// Vertices of `b` where just two straight edges meet in line (a straight edge split in two).
fn stations(b: &Body) -> usize {
    let r = b.record();
    let mut at: Vec<Vec<usize>> = vec![Vec::new(); r.vertices.len()];
    for (i, e) in r.edges.iter().enumerate() {
        for v in e.vertices {
            at[v as usize].push(i);
        }
    }
    let point = |v: u32| Vec3::from(r.vertices[v as usize].point);
    let dir = |e: usize| (point(r.edges[e].vertices[1]) - point(r.edges[e].vertices[0])).normalized();
    let line = |e: usize| matches!(r.edges[e].curve, solidcraft_geom::brep::Curve::Line { .. });
    at.iter()
        .filter(|es| match es.as_slice() {
            [a, b] => line(*a) && line(*b) && dir(*a).zip(dir(*b)).is_some_and(|(u, w)| u.dot(w).abs() > 1. - 1e-9),
            _ => false,
        })
        .count()
}

/// Cuts at several heights through a block's walls leave no vertices along its straight edges
/// (#1049): the edges they never touch stay one edge each, and so does an edge a later join
/// lies beside, instead of being drawn (and met by later features) as many pieces.
#[test]
fn cuts_leave_straight_edges_whole() {
    let mut b = box_solid(Vec3::new(0., 0., 0.), Vec3::new(40., 20., 10.)).unwrap().with_box_certificate(7).unwrap();
    // Notches through the front wall at different heights, a pocket in the top and a slot
    // through the end: each cut is sliced at its own heights.
    b = cut(&b, Vec3::new(5., -1., 2.), Vec3::new(10., 3., 4.));
    b = cut(&b, Vec3::new(15., -1., 5.), Vec3::new(20., 3., 8.));
    b = cut(&b, Vec3::new(25., 8., 6.), Vec3::new(30., 12., 11.));
    b = cut(&b, Vec3::new(37., 5., 3.), Vec3::new(41., 15., 7.));
    assert!(b.history().is_some(), "the cuts report their history");
    assert_eq!(stations(&b), 0, "a straight edge is split at a vertex nothing else uses");
    // The block's four upright edges and the top's far edges are one edge each.
    for (x, y) in [(0., 0.), (40., 0.), (0., 20.), (40., 20.)] {
        assert_eq!(edges_along(&b, Vec3::new(x, y, 0.), Vec3::new(x, y, 10.)), 1, "upright edge at ({x}, {y})");
    }
    assert_eq!(edges_along(&b, Vec3::new(0., 20., 10.), Vec3::new(40., 20., 10.)), 1, "the top's back edge");
    assert_eq!(edges_along(&b, Vec3::new(0., 0., 0.), Vec3::new(0., 20., 0.)), 1, "the bottom's left edge");
    // A boss joined on the top beside its back edge, then another cut: still one edge.
    b = boolean(&b, &box_solid(Vec3::new(2., 15., 10.), Vec3::new(8., 20., 14.)).unwrap(), BoolOp::Union).unwrap().unwrap();
    b = cut(&b, Vec3::new(12., 4., 1.), Vec3::new(14., 21., 9.));
    assert_eq!(stations(&b), 0, "after a join and another cut");
    assert_eq!(edges_along(&b, Vec3::new(0., 0., 0.), Vec3::new(0., 0., 10.)), 1, "upright edge after a join and a cut");
}
