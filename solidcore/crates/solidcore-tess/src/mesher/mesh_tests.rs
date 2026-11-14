//! The mesh path ([`crate::mesh()`]): every face by constrained Delaunay with lattice seeds and
//! shaping by its triangles in 3D, closed, sized by the spec and the caller's size function, the
//! same plan for any number of threads.

use std::collections::BTreeMap;

use solidcore::eval::plan::EvalSurface;
use solidcore_ops::boolean::boolean;
use solidcore_ops::{BooleanOp, make_box, make_cylinder, make_sphere, make_torus};
use solidcore_record::Vec3 as P;
use solidcore_record::brep;

use crate::{Mesh, MeshSpec, PlanStats, TessError, TessPlan, WeldedMesh};

use solidcore::Vec3;

use crate::vec::VecExt as _;

use super::{PoleCap, SplitSide, cap_ring, edges, on_edges, size};

fn record(body: &solidcore_ops::Body) -> brep::Brep {
    body.to_record().expect("the body's record").0
}

/// tangency-labs/solidcraft#14915: a plate 60 x 60 x 10 with a ball of radius 20 sunk 0.1 mm into
/// its top (centre at z = 19.9), joined: the two meet at under 6 degrees round a circle of radius
/// 1.9975 on the plate's top face. Its box diagonal: 98.44.
fn sunk_ball() -> (brep::Brep, f64) {
    let plate = make_box([-30.0, -30.0, -10.0], [60.0, 60.0, 10.0]).expect("plate");
    let ball = make_sphere([0.0, 0.0, 19.9], 20.0).expect("ball");
    let joined = boolean(plate, ball, BooleanOp::Union).expect("join").expect("a body");
    (record(&joined), (2.0 * 60f64.powi(2) + 49.9f64.powi(2)).sqrt())
}

fn sub(a: P, b: P) -> [f64; 3] {
    [a.x - b.x, a.y - b.y, a.z - b.z]
}
fn dot(a: [f64; 3], b: [f64; 3]) -> f64 {
    a[0] * b[0] + a[1] * b[1] + a[2] * b[2]
}
fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [a[1] * b[2] - a[2] * b[1], a[2] * b[0] - a[0] * b[2], a[0] * b[1] - a[1] * b[0]]
}
fn length(a: [f64; 3]) -> f64 {
    dot(a, a).sqrt()
}

/// The smallest of a triangle's three angles in 3D (degrees).
fn smallest_angle([a, b, c]: [P; 3]) -> f64 {
    let at = |o: P, x: P, y: P| {
        let (u, v) = (sub(x, o), sub(y, o));
        length(cross(u, v)).atan2(dot(u, v)).to_degrees()
    };
    at(a, b, c).min(at(b, c, a)).min(at(c, a, b))
}

/// Triangle `t`'s corners.
fn corners(m: &Mesh, t: &[u32; 3]) -> [P; 3] {
    t.map(|i| m.positions[i as usize])
}

/// The smallest angle of face `face`'s triangles (degrees), and how many it has.
fn worst_of(m: &Mesh, face: u32) -> (f64, usize) {
    let tris: Vec<&[u32; 3]> = m.triangles.iter().zip(&m.tri_face).filter(|(_, f)| **f == face).map(|(t, _)| t).collect();
    (tris.iter().map(|t| smallest_angle(corners(m, t))).fold(180.0, f64::min), tris.len())
}

/// The smallest angle of any triangle (degrees), its face and its corners.
fn worst_anywhere(m: &Mesh) -> (f64, u32, [P; 3]) {
    m.triangles
        .iter()
        .zip(&m.tri_face)
        .map(|(t, f)| (smallest_angle(corners(m, t)), *f, corners(m, t)))
        .fold((180.0, u32::MAX, [P::ZERO; 3]), |w, x| if x.0 < w.0 { x } else { w })
}

/// A body on the mesh path at a side of 2 mm and a chord of 0.01 mm: every triangle keeps 20
/// degrees in 3D, nothing folds and it is closed.
fn shaped_in_3d(b: &brep::Brep, size: f64, what: &str) -> PlanStats {
    let spec = MeshSpec { chord: 0.01, size: Some(2.0), min_angle: 25.0 };
    let (plan, stats) = meshed(b, &spec, size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{what}: {refused:?}");
    let (worst, face, at) = worst_anywhere(&m);
    eprintln!("{what} on the mesh path: smallest angle {worst} degrees (face {face}, corners {at:?}) over {} triangles", m.triangles.len());
    assert!(worst >= 20.0, "{what}: smallest angle {worst} degrees, on face {face}, corners {at:?}; unshaped: {:?}", stats.unshaped);
    assert_nothing_folds(&m, what);
    let (w, refused) = plan.welded().unwrap();
    assert!(refused.is_empty(), "{what}: {refused:?}");
    assert_closed(&w, what);
    stats
}

/// A sphere of radius 10, its poles included: 20 degrees in 3D (FieldCraft's trial at
/// e728f90e: 1.10 degrees at the pole, shaped in (u, v) scaled by the mean speeds; the display
/// 4.94).
#[test]
fn a_sphere_on_the_mesh_path_keeps_its_angle_at_its_poles() {
    let b = record(&make_sphere([0.0; 3], 10.0).expect("sphere"));
    let stats = shaped_in_3d(&b, 20.0 * 3f64.sqrt(), "sphere");
    // Each pole is its cap's fan: nothing left short there.
    assert!(stats.unshaped.is_empty(), "{:?}", stats.unshaped);
}

/// A torus 20 / 5: its speed round the tube varies by 3 / 5 to 5 / 3 of the mean between its
/// inside and outside (FieldCraft's trial at e728f90e: a bent tube at 0.06 degrees; the display
/// 9.54).
#[test]
fn a_torus_on_the_mesh_path_keeps_its_angle() {
    let b = record(&make_torus([0.0; 3], 20.0, 5.0).expect("torus"));
    let _ = shaped_in_3d(&b, (50f64.powi(2) * 2.0 + 10f64.powi(2)).sqrt(), "torus");
}

/// A box with its top edges rounded: the fillets are strips a few sides wide, curved across
/// (FieldCraft's trial at e728f90e: 0.08 degrees on a blend).
#[test]
fn rounded_edges_on_the_mesh_path_keep_their_angle() {
    let name = "box_with_filleted_top";
    let _ = shaped_in_3d(&crate::fixtures::fixture(name), crate::fixtures::size_of(name), name);
}

/// Two faces of one edge split the same side in a round: each gives its fraction in its own
/// metric, apart by their rounding, and the edge takes one node, not two a rounding apart (the
/// plate's top face had a triangle of 1.3e-13 degrees at 5c1a53ef7c).
#[test]
fn two_faces_splitting_one_side_put_one_node_on_its_edge() {
    let b = record(&make_box([0.0; 3], [20.0, 10.0, 5.0]).expect("box"));
    let rules = size::Rules::new(0.01, 22.9, None);
    let mut nodes: Vec<edges::Nodes> = (0..b.edges.len()).map(|e| edges::sample_edge(&b, e, &rules, 100.0).expect("nodes")).collect();
    let ei = (0..b.edges.len()).find(|&e| matches!(b.edges[e].curve, brep::Curve::Line { .. })).expect("a line");
    assert_eq!(nodes[ei].points.len(), 2, "one segment");
    let (p0, p1) = (nodes[ei].points[0], nodes[ei].points[1]);
    let faces: Vec<usize> = b.edges[ei].uses.iter().map(|u| u.face as usize).collect();
    let [f, g] = faces.as_slice() else { panic!("two faces: {faces:?}") };
    let mut ts = on_edges(&b.faces[*f], &nodes, &[SplitSide { from: p0, to: p1, at: vec![0.5] }]);
    ts.extend(on_edges(&b.faces[*g], &nodes, &[SplitSide { from: p1, to: p0, at: vec![0.5 + 1e-13] }]));
    assert!(ts.len() == 2 && ts.iter().all(|(e, _)| *e == ei), "{ts:?}");
    let params: Vec<f64> = ts.iter().map(|(_, t)| *t).collect();
    assert!(params[0] != params[1], "apart by rounding: {params:?}");
    edges::insert_at(&b, ei, &mut nodes[ei], &params).expect("inserted");
    assert_eq!(nodes[ei].points.len(), 3, "one node: {:?}", nodes[ei].params);
}

/// Every triangle's facet normal on the side of its corners' surface normals (nothing folded
/// over or reversed).
fn assert_nothing_folds(m: &Mesh, what: &str) {
    for (t, f) in m.triangles.iter().zip(&m.tri_face) {
        let [a, b, c] = corners(m, t);
        let facet = cross(sub(b, a), sub(c, a));
        let n = t.iter().map(|&i| m.normals[i as usize]).fold([0.0; 3], |s, n| [s[0] + n.x, s[1] + n.y, s[2] + n.z]);
        assert!(dot(facet, n) > 0.0, "{what}: triangle {t:?} of face {f} folds over: facet {facet:?}, surface {n:?}");
    }
}

/// Closed: every side of every triangle is a side of exactly one other, run the other way, and
/// no triangle has two corners on one vertex.
fn assert_closed(w: &WeldedMesh, what: &str) {
    // Per side (its vertices in order): its uses, and the uses one way less the other.
    let mut sides: BTreeMap<(u32, u32), (u32, i32)> = BTreeMap::new();
    for t in &w.triangles {
        assert!(t[0] != t[1] && t[1] != t[2] && t[2] != t[0], "{what}: triangle {t:?} has two corners on one vertex");
        for k in 0..3 {
            let (a, b) = (t[k], t[(k + 1) % 3]);
            let s = sides.entry((a.min(b), a.max(b))).or_default();
            s.0 += 1;
            s.1 += if a < b { 1 } else { -1 };
        }
    }
    let open: Vec<_> = sides.iter().filter(|(_, (uses, net))| *uses != 2 || *net != 0).collect();
    assert!(open.is_empty(), "{what}: {} of {} sides are not shared by two triangles run opposite ways: {:?}", open.len(), sides.len(), &open[..open.len().min(8)]);
}

fn meshed(b: &brep::Brep, spec: &MeshSpec, size: f64, size_at: Option<&(dyn Fn(P) -> f64 + Sync)>) -> (TessPlan, PlanStats) {
    match crate::mesh(b, spec, size, size_at) {
        Ok(done) => done,
        Err(e) => panic!("{e}"),
    }
}

/// The plate a ball is sunk into (#14915) on the mesh path: no needle on the plate's top face,
/// nothing folded anywhere, and closed. The display path on this body (`plan` at
/// `MeshTolerance::display_of(98.44)`, a chord of 0.098 mm) leaves needles on the top face round
/// the contact circle, the smallest 0.853 degrees, and facets folded over at that chord
/// (#14915): its flat face has no size, its boundary nodes alone and a Ruppert pass with as many
/// points as it has nodes. On the mesh path the top face has the spec's side, lattice seeds of
/// it, and shaping to 25 degrees, which grades from the circle's chords out to the side.
#[test]
fn a_plate_a_ball_sinks_into_has_no_needles_on_the_mesh_path() {
    let (b, size) = sunk_ball();
    let spec = MeshSpec::chord(crate::MeshTolerance::export_of(size).chord);
    let (plan, stats) = meshed(&b, &spec, size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    // The plate's top face: the one whose triangles lie in z = 0 out to the plate's corners.
    let mut top: Vec<u32> = m
        .triangles
        .iter()
        .zip(&m.tri_face)
        .filter(|(t, _)| corners(&m, t).iter().all(|p| p.z.abs() < 1e-9) && corners(&m, t).iter().any(|p| p.x.abs() > 29.9))
        .map(|(_, f)| *f)
        .collect();
    top.sort_unstable();
    top.dedup();
    let [top] = top.as_slice() else { panic!("one top face: {top:?}") };
    let (worst, count) = worst_of(&m, *top);
    assert!(worst >= 20.0, "the top face's smallest angle is {worst} degrees over {count} triangles; unshaped: {:?}", stats.unshaped);
    // The ball's face too, its pole included (FieldCraft's trial at e728f90e: 1.95 degrees at
    // the pole, shaped in (u, v) scaled by the mean speeds).
    let (worst, face, at) = worst_anywhere(&m);
    assert!(worst >= 20.0, "smallest angle {worst} degrees, on face {face}, corners {at:?}; unshaped: {:?}", stats.unshaped);
    // The ball's pole is its cap's fan: the ball's face is not left short.
    let ball: Vec<usize> = (0..plan.surfaces.len()).filter(|&f| !matches!(plan.surfaces[f], EvalSurface::Plane { .. })).collect();
    assert!(stats.unshaped.iter().all(|u| !ball.contains(&u.face)), "the ball's faces {ball:?}; unshaped: {:?}", stats.unshaped);
    assert_nothing_folds(&m, "sunk ball");
    let (w, refused) = plan.welded().unwrap();
    assert!(refused.is_empty(), "{refused:?}");
    assert_closed(&w, "sunk ball");
}

/// A 20 x 10 x 5 box at a side of 2 mm: every triangle keeps 25 degrees (the faces' corners
/// are right angles, so Ruppert shaping reaches its angle everywhere), no side is longer than
/// 1.5 times the side asked (the refinement's own limit), and it is closed. The display path
/// meshes each of these faces as two triangles of its corners.
#[test]
fn a_box_on_the_mesh_path_keeps_its_angle_and_its_side() {
    let b = record(&make_box([0.0; 3], [20.0, 10.0, 5.0]).expect("box"));
    let size = (20f64.powi(2) + 10f64.powi(2) + 5f64.powi(2)).sqrt();
    let spec = MeshSpec { chord: 0.01, size: Some(2.0), min_angle: 25.0 };
    let (plan, stats) = meshed(&b, &spec, size, None);
    assert!(stats.over_tolerance.is_empty(), "{:?}", stats.over_tolerance);
    assert_eq!((stats.planar, stats.structured, stats.delaunay), (6, 0, 0));
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    for t in &m.triangles {
        let c = corners(&m, t);
        let worst = smallest_angle(c);
        assert!(worst >= 25.0 - 1e-9, "triangle {c:?}: smallest angle {worst}");
        for k in 0..3 {
            let side = length(sub(c[k], c[(k + 1) % 3]));
            assert!(side <= 1.5 * 2.0 + 1e-9, "triangle {c:?}: a side of {side}");
        }
    }
    assert_nothing_folds(&m, "box");
    let (w, _) = plan.welded().unwrap();
    assert_closed(&w, "box");
}

/// A cylinder's wall, shaped by its triangles in 3D, keeps 20 degrees (FieldCraft's trial at
/// e728f90e: 0.08 degrees on a cylinder's side, shaped in (u, v) and then flipped toward the
/// surface's normals as the display is).
#[test]
fn a_cylinder_wall_on_the_mesh_path_is_shaped() {
    let b = record(&make_cylinder([0.0; 3], 10.0, 20.0).expect("cylinder"));
    let size = (20f64.powi(2) * 2.0 + 20f64.powi(2)).sqrt();
    let spec = MeshSpec { chord: 0.01, size: Some(2.0), min_angle: 25.0 };
    let (plan, stats) = meshed(&b, &spec, size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    let wall: Vec<u32> = (0..b.faces.len() as u32).filter(|&f| !matches!(plan.surfaces[f as usize], EvalSurface::Plane { .. })).collect();
    let [wall] = wall.as_slice() else { panic!("one curved face: {wall:?}") };
    let (worst, count) = worst_of(&m, *wall);
    eprintln!("cylinder wall on the mesh path: smallest angle {worst} degrees over {count} triangles");
    assert!(worst >= 20.0, "the wall's smallest angle is {worst} degrees over {count} triangles; unshaped: {:?}", stats.unshaped);
    assert_nothing_folds(&m, "cylinder");
    let (w, _) = plan.welded().unwrap();
    assert_closed(&w, "cylinder");
}

/// A size function that asks for half the side inside a ball on the box's top face: the sides
/// there are shorter (none longer than 1.5 times the local side, its edge segments no longer
/// than it) and the face has more triangles there than without it; it stays closed.
#[test]
fn a_size_function_gives_shorter_sides_where_it_asks() {
    let b = record(&make_box([0.0; 3], [20.0, 10.0, 5.0]).expect("box"));
    let size = (20f64.powi(2) + 10f64.powi(2) + 5f64.powi(2)).sqrt();
    let spec = MeshSpec { chord: 0.01, size: Some(2.0), min_angle: 25.0 };
    let centre = P::new(10.0, 5.0, 5.0);
    let near = |p: P, r: f64| length(sub(p, centre)) < r;
    let half = move |p: P| if near(p, 3.0) { 1.0 } else { f64::INFINITY };
    let (plain, _) = meshed(&b, &spec, size, None);
    let (sized_plan, stats) = meshed(&b, &spec, size, Some(&half));
    assert!(stats.over_tolerance.is_empty(), "{:?}", stats.over_tolerance);
    let (plain, sized) = (plain.evaluate().0, sized_plan.evaluate().0);
    let inside = |m: &Mesh| {
        m.triangles
            .iter()
            .filter(|t| {
                let [a, b, c] = corners(m, t);
                near((a + b + c) * (1.0 / 3.0), 2.5)
            })
            .count()
    };
    let (before, after) = (inside(&plain), inside(&sized));
    assert!(after > before, "triangles inside the ball: {after} with the function, {before} without");
    for t in &sized.triangles {
        let c = corners(&sized, t);
        for k in 0..3 {
            let (a, d) = (c[k], c[(k + 1) % 3]);
            let side = length(sub(a, d));
            if near((a + d) * 0.5, 2.5) {
                assert!(side <= 1.5 * 1.0 + 1e-9, "a side of {side} at {:?}, inside the ball", (a + d) * 0.5);
            }
        }
    }
    let (w, _) = sized_plan.welded().unwrap();
    assert_closed(&w, "box with a size function");
}

/// Faces are planned on as many threads as there are: one thread and eight give the same plan
/// bit for bit (a plate with four holes, enough curved faces for the pool).
#[cfg(not(target_arch = "wasm32"))]
#[test]
fn the_mesh_path_gives_one_plan_on_any_number_of_threads() {
    let mut plate = make_box([0.0; 3], [40.0, 40.0, 10.0]).expect("plate");
    for (x, y) in [(10.0, 10.0), (30.0, 10.0), (10.0, 30.0), (30.0, 30.0)] {
        let hole = make_cylinder([x, y, -1.0], 3.0, 12.0).expect("hole");
        plate = boolean(plate, hole, BooleanOp::Subtract).expect("cut").expect("a body");
    }
    let b = record(&plate);
    let size = (2.0 * 40f64.powi(2) + 10f64.powi(2)).sqrt();
    let spec = MeshSpec { chord: 0.02, size: Some(3.0), min_angle: 25.0 };
    let on = |threads: usize| {
        let pool = rayon::ThreadPoolBuilder::new().num_threads(threads).stack_size(8 << 20).build().expect("a pool");
        pool.install(|| meshed(&b, &spec, size, None))
    };
    let (one, eight) = (on(1), on(8));
    assert!(one.0 == eight.0, "the plans differ");
    assert_eq!(one.1, eight.1);
}

/// A spec out of range is refused, naming what.
#[test]
fn a_spec_out_of_range_is_refused() {
    let b = record(&make_box([0.0; 3], [2.0, 2.0, 2.0]).expect("box"));
    for (spec, what) in [
        (MeshSpec { chord: 0.0, size: None, min_angle: 25.0 }, "chord"),
        (MeshSpec { chord: f64::NAN, size: None, min_angle: 25.0 }, "chord"),
        (MeshSpec { chord: 0.01, size: Some(-1.0), min_angle: 25.0 }, "size"),
        (MeshSpec { chord: 0.01, size: Some(f64::INFINITY), min_angle: 25.0 }, "size"),
        (MeshSpec { chord: 0.01, size: None, min_angle: 0.0 }, "angle"),
        (MeshSpec { chord: 0.01, size: None, min_angle: 31.0 }, "angle"),
    ] {
        match crate::mesh(&b, &spec, 3.5, None) {
            Err(TessError::Invalid(m)) => assert!(m.contains(what), "{spec:?}: {m}"),
            other => panic!("{spec:?}: {:?}", other.map(|(_, s)| s)),
        }
    }
}

/// A rational spline sphere of radius 10, as a kernel writes one: u round the axis in four
/// quarter arcs, v from the south pole to the north in two, each arc's middle weight 1/2^(1/2).
/// Its parameters run unevenly round each arc.
fn spline_sphere() -> EvalSurface {
    let w = std::f64::consts::FRAC_1_SQRT_2;
    let round: [([f64; 2], f64); 9] =
        [([1.0, 0.0], 1.0), ([1.0, 1.0], w), ([0.0, 1.0], 1.0), ([-1.0, 1.0], w), ([-1.0, 0.0], 1.0), ([-1.0, -1.0], w), ([0.0, -1.0], 1.0), ([1.0, -1.0], w), ([1.0, 0.0], 1.0)];
    let meridian: [([f64; 2], f64); 5] = [([0.0, -1.0], 1.0), ([1.0, -1.0], w), ([1.0, 0.0], 1.0), ([1.0, 1.0], w), ([0.0, 1.0], 1.0)];
    let mut ctrl = Vec::new();
    for ([r, z], wv) in meridian {
        for ([x, y], wu) in round {
            let wt = wu * wv;
            ctrl.push([10.0 * x * r * wt, 10.0 * y * r * wt, 10.0 * z * wt, wt]);
        }
    }
    let knots_u = vec![0.0, 0.0, 0.0, 0.25, 0.25, 0.5, 0.5, 0.75, 0.75, 1.0, 1.0, 1.0];
    let knots_v = vec![0.0, 0.0, 0.0, 0.5, 0.5, 1.0, 1.0, 1.0];
    EvalSurface::nurbs([2, 2], [knots_u, knots_v], [9, 5], ctrl)
}

/// A pole's ring on a rational spline sphere, a full turn round its north pole from the seam
/// back to it, at 25 degrees: 14 segments, and every neighbour pair, the pair across the seam
/// included, seen from the pole at 25 degrees or more in 3D. (At 83e819468f the ring was even in
/// its parameter, and one pair turned 19.7 degrees.)
#[test]
fn a_pole_ring_is_even_round_the_pole_in_3d() {
    let s = spline_sphere();
    let v = 0.985;
    let point = |q: [f64; 2]| s.derivs(q[0], q[1]).expect("evaluates").0;
    // The seam's node at the ring, from both sides, and the pole.
    let (before, after, pole) = ([1.0, v], [0.0, v], [0.0, 1.0]);
    let uvs = vec![before, after, pole];
    let fixed = vec![point(before), point(after), point(pole)];
    assert!(fixed[0].dist(fixed[1]) < 1e-9, "the seam's two copies are one point");
    let cap = PoleCap { lp: 0, copies: vec![2], before: 0, after: 1 };
    let min_angle = 25f64.to_radians();
    let Ok(inner) = cap_ring(&s, &cap, &uvs, &fixed, 1.0, min_angle) else { panic!("a ring") };
    assert_eq!(inner.len() + 1, 14, "segments");
    let ring: Vec<Vec3> = std::iter::once(fixed[0]).chain(inner.iter().map(|(_, p)| *p)).chain(std::iter::once(fixed[1])).collect();
    for (k, w) in ring.windows(2).enumerate() {
        let turn = edges::turn(w[0] - fixed[2], w[1] - fixed[2]).to_degrees();
        assert!(turn >= 25.0, "pair {k} turns {turn} degrees at the pole");
    }
}

