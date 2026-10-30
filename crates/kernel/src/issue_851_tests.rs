//! Fillets and chamfers where a curved wall runs out into a flat face in a knife edge (#851).
use crate::*;
use solidcraft_geom::Vec3;
use std::f64::consts::PI;

const R_WALL: f64 = 21.0;
const HEIGHT: f64 = 10.0;
/// x of the knife edge: the arc wall's far end.
const TIP_X: f64 = 62.0 + 1.0 / 6.0;

/// A 41⅙ × 21 plate plus the corner a circle of radius 21 leaves, extruded 10 mm: the arc wall
/// meets the front face square and runs out into the back face (y = 0) in a knife edge at (TIP_X, 0).
fn part() -> Body {
    let x0 = TIP_X - R_WALL;
    let plate = box_solid(Vec3::new(0.0, -R_WALL, 0.0), Vec3::new(x0, 0.0, HEIGHT)).unwrap();
    let corner = box_solid(Vec3::new(x0, -R_WALL, 0.0), Vec3::new(TIP_X, 0.0, HEIGHT)).unwrap();
    let disc = cylinder(Vec3::new(TIP_X, -R_WALL, 0.0), Vec3::Z, R_WALL, HEIGHT).unwrap();
    let corner = boolean(&corner, &disc, BoolOp::Cut).unwrap().unwrap();
    boolean(&plate, &corner, BoolOp::Union).unwrap().unwrap()
}

/// Area a ball of radius `r` takes off the cusp: the quadrilateral tip–wall-contact–wall-centre–
/// ball-centre-contact, less the wall's sector and the ball's.
fn removed_area(r: f64) -> f64 {
    let beta = ((R_WALL - r) / (R_WALL + r)).acos();
    let o = (TIP_X - (4.0 * R_WALL * r).sqrt(), -r);
    let c = (TIP_X, -R_WALL);
    let tip = (TIP_X, 0.0);
    let t1 = (o.0, 0.0);
    // Shoelace over t1 → tip → c → o.
    let pts = [t1, tip, c, o];
    let quad: f64 = (0..4).map(|i| pts[i].0 * pts[(i + 1) % 4].1 - pts[(i + 1) % 4].0 * pts[i].1).sum::<f64>().abs() / 2.0;
    quad - 0.5 * R_WALL * R_WALL * beta - 0.5 * r * r * (PI - beta)
}

fn knife() -> Vec3 {
    Vec3::new(TIP_X, 0.0, HEIGHT / 2.0)
}

/// The same part with the corner joined first: its faces come in another order, so the plane
/// and the wall swap sides at the knife edge.
fn part_other_order() -> Body {
    let x0 = TIP_X - R_WALL;
    let plate = box_solid(Vec3::new(0.0, -R_WALL, 0.0), Vec3::new(x0, 0.0, HEIGHT)).unwrap();
    let corner = box_solid(Vec3::new(x0, -R_WALL, 0.0), Vec3::new(TIP_X, 0.0, HEIGHT)).unwrap();
    let disc = cylinder(Vec3::new(TIP_X, -R_WALL, 0.0), Vec3::Z, R_WALL, HEIGHT).unwrap();
    let both = boolean(&corner, &plate, BoolOp::Union).unwrap().unwrap();
    boolean(&both, &disc, BoolOp::Cut).unwrap().unwrap()
}

/// The same solid as one box less the wall's cylinder: the plane comes before the wall in the
/// shell, so the plane is side 1 at the knife edge.
fn part_cut_from_a_box() -> Body {
    let slab = box_solid(Vec3::new(0.0, -R_WALL, 0.0), Vec3::new(TIP_X, 0.0, HEIGHT)).unwrap();
    let disc = cylinder(Vec3::new(TIP_X, -R_WALL, 0.0), Vec3::Z, R_WALL, HEIGHT).unwrap();
    boolean(&slab, &disc, BoolOp::Cut).unwrap().unwrap()
}

#[test]
fn fillet_the_knife_edge() {
    for (name, body) in [("plate first", part()), ("corner first", part_other_order()), ("cut from a box", part_cut_from_a_box())] {
        let before = measure(&body).unwrap().volume;
        for r in [0.5, 2.0, 5.0] {
            let rounded = fillet(&body, &[knife()], r).unwrap_or_else(|e| panic!("{name} r={r}: {e}"));
            assert!(rounded.validity().is_empty(), "{name} r={r}: {:?}", rounded.validity());
            assert_eq!(rounded.face_count(), body.face_count() + 1, "{name} r={r}: one blend face");
            let v = measure(&rounded).unwrap().volume;
            let expected = before - HEIGHT * removed_area(r);
            assert!((v - expected).abs() < 1e-3 * expected.abs(), "{name} r={r}: volume {v}, expected {expected}");
        }
    }
}

/// A ball that does not fit between the faces, and sizes at the ends of the range, are refused
/// or blended; they never panic or give an invalid solid.
#[test]
fn knife_edge_sizes_never_panic() {
    let body = part();
    for r in [1e-5, 1e-3, 10.0, 21.0, 40.0, 1e3, 1e6 - 1.0] {
        for round in [true, false] {
            let made = if round { fillet(&body, &[knife()], r) } else { chamfer(&body, &[knife()], r) };
            if let Ok(b) = made {
                assert!(b.validity().is_empty(), "r={r}: {:?}", b.validity());
            }
        }
    }
}

/// Mirrored parts reverse the edge and its faces' use of it: the blend is the same.
#[test]
fn knife_edge_blends_the_same_in_a_mirror() {
    let body = part();
    let reference = measure(&fillet(&body, &[knife()], 2.0).unwrap()).unwrap().volume;
    let mirror = |axis: usize| {
        let mut m = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        m[axis][axis] = -1.0;
        m
    };
    for (axis, at) in [(0, Vec3::new(-TIP_X, 0.0, 5.0)), (1, Vec3::new(TIP_X, 0.0, 5.0)), (2, Vec3::new(TIP_X, 0.0, -5.0))] {
        let mirrored = transform_matrix(&body, mirror(axis)).unwrap();
        let v = measure(&fillet(&mirrored, &[at], 2.0).unwrap()).unwrap().volume;
        assert!((v - reference).abs() < 1e-6, "mirror {axis}: volume {v}, expected {reference}");
    }
}

/// A point on the arc's top edge (half way round the wall).
fn arc_top() -> Vec3 {
    let (s, c) = (std::f64::consts::FRAC_PI_4.sin(), std::f64::consts::FRAC_PI_4.cos());
    Vec3::new(TIP_X - R_WALL * c, -R_WALL + R_WALL * s, HEIGHT)
}

/// Volume the arc's top edge takes off, by integrating the corner of the blend over the wall's
/// angle: the part of the sweep above the back face (y = 0) is not material. Rounds: the corner
/// between the wall and the top, less the ball's quarter disc; chamfers: its triangle.
fn arc_top_removed(size: f64, round: bool) -> f64 {
    let n = 2000;
    let step = size / f64::from(n);
    let mut total = 0.0;
    for i in 0..n {
        let rho = R_WALL + (f64::from(i) + 0.5) * step;
        for j in 0..n {
            let z = HEIGHT - size + (f64::from(j) + 0.5) * step;
            let inside = if round {
                (rho - (R_WALL + size)).powi(2) + (z - (HEIGHT - size)).powi(2) >= size * size
            } else {
                (rho - R_WALL) + (HEIGHT - z) <= size
            };
            if inside {
                total += rho * (R_WALL / rho).asin() * step * step;
            }
        }
    }
    total
}

/// Volume through the display planner (the mesh the app draws): closed and exact where the
/// legacy mesher behind `measure` can leak at a zero-angle corner like the one at the tip.
pub(crate) fn planned_volume(b: &Body) -> f64 {
    let mesh = b.mesh_plan(&crate::MeshTolerance::chord(2e-4)).unwrap().evaluate();
    let key = |p: Vec3| [p.x.to_bits(), p.y.to_bits(), p.z.to_bits()];
    let mut uses: std::collections::HashMap<([u64; 3], [u64; 3]), i32> = std::collections::HashMap::new();
    for t in &mesh.triangles {
        let [a, b, c] = mesh.tri(t).unwrap();
        for (p, q) in [(a, b), (b, c), (c, a)] {
            *uses.entry((key(p), key(q))).or_default() += 1;
            *uses.entry((key(q), key(p))).or_default() -= 1;
        }
    }
    assert!(uses.values().all(|n| *n == 0), "the mesh has cracks");
    mesh.measure().volume
}

/// The arc's top edge runs out tangent to the back face at the knife edge's top: the blend is cut
/// by that face along a curve that touches the wall's contact at the tip.
#[test]
fn fillet_and_chamfer_the_arc_top_edge() {
    for (name, body) in [("plate first", part()), ("corner first", part_other_order()), ("cut from a box", part_cut_from_a_box())] {
        let before = planned_volume(&body);
        for r in [0.5, 2.0, 4.0, 8.0] {
            for round in [true, false] {
                let what = if round { "fillet" } else { "chamfer" };
                let made = if round { fillet(&body, &[arc_top()], r) } else { chamfer(&body, &[arc_top()], r) };
                let made = made.unwrap_or_else(|e| panic!("{name} {what} r={r}: {e}"));
                assert!(made.validity().is_empty(), "{name} {what} r={r}: {:?}", made.validity());
                assert_eq!(made.face_count(), body.face_count() + 1, "{name} {what} r={r}: one blend face");
                let removed = before - planned_volume(&made);
                let expected = arc_top_removed(r, round);
                assert!((removed - expected).abs() < 0.01 * expected + 0.01, "{name} {what} r={r}: removed {removed}, expected {expected}");
            }
        }
    }
}

/// Mirrored parts take the edge the other way round and the tip at the other end of the chain.
#[test]
fn the_arc_top_edge_blends_the_same_in_a_mirror() {
    let body = part();
    let reference = planned_volume(&fillet(&body, &[arc_top()], 2.0).unwrap());
    let mirror = |axis: usize| {
        let mut m = [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]];
        m[axis][axis] = -1.0;
        m
    };
    let at = arc_top();
    for (axis, p) in [(0, Vec3::new(-at.x, at.y, at.z)), (1, Vec3::new(at.x, -at.y, at.z)), (2, Vec3::new(at.x, at.y, -at.z))] {
        let mirrored = transform_matrix(&body, mirror(axis)).unwrap();
        let v = planned_volume(&fillet(&mirrored, &[p], 2.0).unwrap_or_else(|e| panic!("mirror {axis}: {e}")));
        assert!((v - reference).abs() < 1e-4 * reference, "mirror {axis}: volume {v}, expected {reference}");
    }
}

/// Sizes from tiny to larger than the wall: blended, or refused, never a panic or a broken solid.
#[test]
fn arc_top_edge_sizes_never_panic() {
    let body = part();
    for r in [1e-5, 1e-3, 6.0, 10.0, 21.0, 40.0, 1e3] {
        for round in [true, false] {
            let made = if round { fillet(&body, &[arc_top()], r) } else { chamfer(&body, &[arc_top()], r) };
            if let Ok(b) = made {
                assert!(b.validity().is_empty(), "r={r}: {:?}", b.validity());
            }
        }
    }
}

fn tf() -> Vec3 {
    Vec3::new(20.0, -R_WALL, HEIGHT)
}
fn fl() -> Vec3 {
    Vec3::new(0.0, -R_WALL, HEIGHT / 2.0)
}
fn fu() -> Vec3 {
    Vec3::new(TIP_X - R_WALL, -R_WALL, HEIGHT / 2.0)
}

/// Volume two blends take off together: each alone, less the corner both would take.
fn removed_together(body: &Body, picks: &[Vec3], r: f64, overlap: f64) -> f64 {
    let before = planned_volume(body);
    picks.iter().map(|p| before - planned_volume(&fillet(body, &[*p], r).unwrap())).sum::<f64>() - overlap
}

/// The front's top edge and its left upright meet at a box corner; the edge's far end runs into
/// the curved wall. In either order, and picked together, the rounds end in each other where
/// they meet: the body is the one both would make (the two corner regions overlap by the
/// volume integrated over the box corner).
#[test]
fn fillet_the_top_front_edge_and_the_front_left_upright() {
    let body = part();
    let (before, r) = (planned_volume(&body), 2.0);
    let want = removed_together(&body, &[tf(), fl()], r, 0.766_957);
    let check = |name: &str, made: Result<Body>| {
        let made = made.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(made.validity().is_empty(), "{name}: {:?}", made.validity());
        let removed = before - planned_volume(&made);
        assert!((removed - want).abs() < 0.02, "{name}: removed {removed}, expected {want}");
        made.face_count()
    };
    let first = fillet(&body, &[tf()], r).unwrap();
    let second = fillet(&body, &[fl()], r).unwrap();
    let faces = [
        check("top front, then the upright", fillet(&first, &[fl()], r)),
        check("upright, then the top front", fillet(&second, &[tf()], r)),
        check("together", fillet(&body, &[tf(), fl()], r)),
    ];
    assert!(faces.iter().all(|f| *f == faces[0]), "face counts {faces:?}");
}

/// The front's top edge and the front upright at the arc meet at the wall: the round of the one
/// ends in the other. Either way round and picked together, the body is the one both make: each
/// alone, less the corner both would take.
#[test]
fn fillet_the_top_front_edge_and_the_front_upright_at_the_arc() {
    let body = part();
    let (before, r) = (planned_volume(&body), 2.0);
    let want = removed_together(&body, &[tf(), fu()], r, 0.699_134);
    let check = |name: &str, made: Result<Body>| {
        let made = made.unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(made.validity().is_empty(), "{name}: {:?}", made.validity());
        let removed = before - planned_volume(&made);
        assert!((removed - want).abs() < 0.02, "{name}: removed {removed}, expected {want}");
    };
    let first = fillet(&body, &[tf()], r).unwrap();
    check("top front, then the upright", fillet(&first, &[fu()], r));
    check("together", fillet(&body, &[tf(), fu()], r));
    check("together, the other way", fillet(&body, &[fu(), tf()], r));
}

/// A blend of the edge nearest `p` (round or flat), size `r`.
fn blend_one_edge(body: &Body, p: Vec3, r: f64, round: bool) -> Result<Body> {
    if round { fillet(body, &[p], r) } else { chamfer(body, &[p], r) }
}

/// The arc's top edge and the front's top edge meet at the wall, where the wall's edge with the
/// front stays sharp: the blend of each ends in the other. Either way round, picked together or
/// one after the other, the body is each alone less the corner the two take together (volumes
/// integrated over the swept corners).
#[test]
fn blend_the_arc_top_edge_and_the_top_front_edge() {
    let body = part();
    let before = planned_volume(&body);
    for (name, round, overlap) in [("fillet", true, 0.759), ("chamfer", false, 2.69)] {
        let alone = |p: Vec3| before - planned_volume(&blend_one_edge(&body, p, 2.0, round).unwrap());
        let want = alone(tf()) + alone(arc_top()) - overlap;
        let check = |how: &str, made: Result<Body>| {
            let made = made.unwrap_or_else(|e| panic!("{name} {how}: {e}"));
            assert!(made.validity().is_empty(), "{name} {how}: {:?}", made.validity());
            let removed = before - planned_volume(&made);
            assert!((removed - want).abs() < 0.06, "{name} {how}: removed {removed}, expected {want}");
        };
        let (a, t) = (blend_one_edge(&body, arc_top(), 2.0, round).unwrap(), blend_one_edge(&body, tf(), 2.0, round).unwrap());
        check("arc, then the front", blend_one_edge(&a, tf(), 2.0, round));
        check("front, then the arc", blend_one_edge(&t, arc_top(), 2.0, round));
        let both = |picks: &[Vec3]| if round { fillet(&body, picks, 2.0) } else { chamfer(&body, picks, 2.0) };
        check("together", both(&[tf(), arc_top()]));
        check("together, the other way", both(&[arc_top(), tf()]));
    }
}

/// The arc's top edge and the front upright at the arc meet at the same corner, with the front's
/// top edge sharp: again the body is each alone less the corner both take.
#[test]
fn fillet_the_arc_top_edge_and_the_front_upright_at_the_arc() {
    let body = part();
    let (before, r) = (planned_volume(&body), 2.0);
    let alone = |p: Vec3| before - planned_volume(&fillet(&body, &[p], r).unwrap());
    let want = alone(arc_top()) + alone(fu()) - 0.682_379;
    let check = |how: &str, made: Result<Body>| {
        let made = made.unwrap_or_else(|e| panic!("{how}: {e}"));
        assert!(made.validity().is_empty(), "{how}: {:?}", made.validity());
        let removed = before - planned_volume(&made);
        assert!((removed - want).abs() < 0.06, "{how}: removed {removed}, expected {want}");
    };
    // (The upright first, then the arc as a feature of its own, is not supported yet: the arc's
    // blend would run on over the upright's round, past the crease where the two meet.)
    let a = fillet(&body, &[arc_top()], r).unwrap();
    check("arc, then the upright", fillet(&a, &[fu()], r));
    check("together", fillet(&body, &[arc_top(), fu()], r));
    check("together, the other way", fillet(&body, &[fu(), arc_top()], r));
}

/// The three edges at the arc's front corner (the front's top edge, the arc's top edge and the
/// front upright at the arc) end in a sphere of the rounds' radius. The volume is the three
/// rounds, each cut where the ball that touches the three faces ends it, and the corner beyond
/// those cuts less the ball's part of it (integrated over the swept corners and the sphere's
/// wedge).
#[test]
fn fillet_the_three_edges_at_the_arcs_front_corner() {
    let body = part();
    let before = planned_volume(&body);
    let want = 67.05;
    for (name, picks) in [("as picked", [tf(), arc_top(), fu()]), ("another order", [fu(), tf(), arc_top()]), ("a third", [arc_top(), fu(), tf()])] {
        let made = fillet(&body, &picks, 2.0).unwrap_or_else(|e| panic!("{name}: {e}"));
        assert!(made.validity().is_empty(), "{name}: {:?}", made.validity());
        // The body's six faces, the three rounds and the sphere's patch.
        assert_eq!(made.face_count(), 10, "{name}");
        let removed = before - planned_volume(&made);
        assert!((removed - want).abs() < 0.12, "{name}: removed {removed}, expected {want}");
    }
}

/// #1263: check the unfilleted input separately from the three-edge result.
/// The circular wall's tessellation differs from its analytic volume by at most
/// its arc length times the chord tolerance times the height.
#[test]
fn unfilleted_knife_edge_input_has_a_closed_display_plan() {
    let want = HEIGHT * ((TIP_X - R_WALL) * R_WALL + R_WALL * R_WALL * (1.0 - PI / 4.0));
    for (name, body) in [("plate first", part()), ("corner first", part_other_order()), ("cut from a box", part_cut_from_a_box())] {
        let volume = planned_volume(&body);
        let tolerance = 0.5 * PI * R_WALL * 2e-4 * HEIGHT;
        assert!((volume - want).abs() < tolerance, "{name}: volume {volume}, expected {want}");
    }
}

/// Sizes at the ends of the range at a ball vertex: rounded or refused, never a panic.
#[test]
fn rounds_ending_in_a_sphere_of_any_size_never_panic() {
    let body = part();
    for r in [1e-5, 1e-3, 5.0, 9.0, 10.0, 21.0, 1e3] {
        if let Ok(b) = fillet(&body, &[tf(), arc_top(), fu()], r) {
            assert!(b.validity().is_empty(), "r={r}: {:?}", b.validity());
        }
    }
}
