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

use super::{Lenient, Planned, PoleCap, Purpose, Shape, Sides, SplitSide, cap_fan, cap_ring, edges, on_edges, plan_face, poles, ring_off_boundary, seam, size, uv};

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

/// The kernel rig's `surfaces/cov-ruled-cylinder-rim-tangent-to-the-top-is-a-flat-ring`, its
/// body `Brim` as SolidCraft exports it (frozen in `tests/data`): one plane face in z = 10, the
/// annulus of radii 10 and 15 round (0, 0, 10), bounded by one loop that runs round the inner
/// circle, out along a line to the outer one, round it and back along the line (a slit). The
/// loop passes the inner circle's vertex twice; the triangulation has one node there, the
/// loop's first, while the shaping kept the boundary's sides by their places in the loop, so it
/// did not keep the slit's side at that vertex and flipped it away: "triangulation does not
/// follow the boundary". Meshed at SolidCraft's export spec (a chord of 1/5000 of the size, the
/// vertices' box diagonal and its edges' quarter points, as SolidCraft's `Body::size`): nothing
/// folds, no triangle side has more than two, every side of the slit has two (one either side)
/// and they add up to its length, every side with one lies on one of the two circles, and the
/// triangles' area is exactly what the boundary's polygons bound (the annulus less the segments
/// the outer circle's chords cut off, plus those the inner circle's chords cut off the hole), to
/// rounding: nothing is missing, nothing overlaps.
#[test]
fn a_ring_bounded_by_a_slit_loop_follows_its_boundary_on_the_mesh_path() {
    let b = crate::fixtures::fixture("rig_ruled_cylinder_rim_flat_ring");
    // SolidCraft's size of the body (`Body::size`): the box diagonal of its vertices and of each
    // edge's points at a quarter, a half and three quarters of its interval.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut add = |p: [f64; 3]| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    for v in &b.vertices {
        add(v.point);
    }
    for e in &b.edges {
        let curve = solidcore_convert::curve(e.parameters()).expect("the edge's curve");
        let c = solidcore::eval::CurveEval::new(&curve).expect("evaluates");
        let [t0, t1] = e.interval;
        for i in 1..4 {
            let p = c.point(t0 + (t1 - t0) * f64::from(i) / 4.0).expect("on the curve");
            add([p.x, p.y, p.z]);
        }
    }
    let size = length(sub(P::new(hi[0], hi[1], hi[2]), P::new(lo[0], lo[1], lo[2])));
    let chord = crate::MeshTolerance::export_of(size).chord;
    let (plan, _) = meshed(&b, &MeshSpec::chord(chord), size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    assert_nothing_folds(&m, "flat ring");
    let centre = P::new(0.0, 0.0, 10.0);
    let mut sides: BTreeMap<(u32, u32), u32> = BTreeMap::new();
    let mut area = 0.0;
    for t in &m.triangles {
        let [a, b, c] = corners(&m, t);
        area += 0.5 * length(cross(sub(b, a), sub(c, a)));
        for k in 0..3 {
            let (i, j) = (t[k], t[(k + 1) % 3]);
            *sides.entry((i.min(j), i.max(j))).or_default() += 1;
        }
    }
    let radius = |i: u32| length(sub(m.positions[i as usize], centre));
    let circle = |i: u32| [10.0, 15.0].into_iter().find(|r| (radius(i) - r).abs() <= 1e-9 * 15.0);
    // The slit: the line from the inner circle's vertex out to the outer one's.
    let point = |v: usize| P::new(b.vertices[v].point[0], b.vertices[v].point[1], b.vertices[v].point[2]);
    let (s0, s1) = (point(0), point(1));
    let along = sub(s1, s0);
    let on_slit = |i: u32| {
        let d = sub(m.positions[i as usize], s0);
        let f = dot(d, along) / dot(along, along);
        (-1e-12..=1.0 + 1e-12).contains(&f) && length(cross(d, along)) / length(along) <= 1e-9 * 15.0
    };
    // The area the boundary's polygons bound, exactly: the annulus, less the segment each chord of
    // the outer circle cuts off it, plus the segment each chord of the inner circle cuts off the
    // hole (r² (θ − sin θ) / 2 for a chord c of a circle of radius r, θ = 2 asin(c / 2r)).
    let segment = |r: f64, c: f64| {
        let theta = 2.0 * (c / (2.0 * r)).min(1.0).asin();
        0.5 * r * r * (theta - theta.sin())
    };
    let annulus = std::f64::consts::PI * (15.0f64.powi(2) - 10.0f64.powi(2));
    let (mut bounded, mut slit) = (annulus, 0.0);
    for (&(i, j), &uses) in &sides {
        let c = length(sub(m.positions[i as usize], m.positions[j as usize]));
        assert!(uses <= 2, "side {i}-{j} has {uses} triangles");
        if on_slit(i) && on_slit(j) {
            // The slit is kept: each of its sides has a triangle either side.
            assert_eq!(uses, 2, "the slit's side {i}-{j} has {uses} triangles");
            slit += c;
        }
        if uses == 1 {
            match (circle(i), circle(j)) {
                (Some(r), Some(q)) if r == q && r > 12.5 => bounded -= segment(r, c),
                (Some(r), Some(q)) if r == q => bounded += segment(r, c),
                _ => panic!("side {i}-{j} is open off the circles: radii {} and {}", radius(i), radius(j)),
            }
        }
    }
    assert!((slit - along.iter().map(|x| x * x).sum::<f64>().sqrt()).abs() <= 1e-9 * 15.0, "the slit's sides add up to {slit}");
    assert!((area - bounded).abs() <= 1e-9 * annulus, "area {area}, the boundary's polygons bound {bounded}");
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

/// The kernel rig's `booleans/1984-rivet-with-flush-heads-a-hair-off-its-hole`, its body `Sheet`
/// as SolidCraft exports it (frozen in `tests/data`): two cylinders a hair apart and two planes.
/// On the plane face 5 two arcs leave a vertex tangentially. Their sampled chords cross nowhere
/// when the body is prepared, but the nodes the shaping's rounds put on the edges move chords,
/// and a new chord of one arc crossed a chord of the other: the face was refused ("boundary
/// crosses itself"). Meshed at SolidCraft's export spec (a chord of 1/5000 of its size), the
/// faces of the edges that took nodes are untangled again, and it meshes: nothing refused,
/// nothing folds, and the body's mesh is closed.
#[test]
fn chords_moved_by_the_shaping_rounds_are_untangled_on_the_mesh_path() {
    let b = crate::fixtures::fixture("rig_rivet_flush_heads_sheet");
    // SolidCraft's size of the body (`Body::size`): the box diagonal of its vertices and of each
    // edge's points at a quarter, a half and three quarters of its interval. It only picks the
    // spec SolidCraft exports at (and the size the mesher is given), so the face is meshed as
    // SolidCraft meshes it.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut add = |p: [f64; 3]| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    for v in &b.vertices {
        add(v.point);
    }
    for e in &b.edges {
        let curve = solidcore_convert::curve(e.parameters()).expect("the edge's curve");
        let c = solidcore::eval::CurveEval::new(&curve).expect("evaluates");
        let [t0, t1] = e.interval;
        for i in 1..4 {
            let p = c.point(t0 + (t1 - t0) * f64::from(i) / 4.0).expect("on the curve");
            add([p.x, p.y, p.z]);
        }
    }
    let size = length(sub(P::new(hi[0], hi[1], hi[2]), P::new(lo[0], lo[1], lo[2])));
    let spec = MeshSpec::chord(crate::MeshTolerance::export_of(size).chord);
    let (plan, _) = meshed(&b, &spec, size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    assert_nothing_folds(&m, "rivet sheet");
    let (w, refused) = plan.welded().unwrap();
    assert!(refused.is_empty(), "{refused:?}");
    assert_closed(&w, "rivet sheet");
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

/// The kernel rig's `booleans/4431-tapered-text-cut-over-block-edge`, its body `Body1` as
/// SolidCraft exports it (frozen in `tests/data`). Its face 330 is a spline sliver 1 mm long and
/// 7e-5 mm wide at its base, tapering to a side collapsed to one point (a pole, within its
/// vertices' tolerance of 2e-5 mm). Its top quarter is narrower than that reach, so the node of
/// its loop below the pole, 0.25 mm from it, is a pole too by the kernel's classifier: the pole
/// caps took only the pole's copies as a cap and refused the face ("a pole next to another pole
/// on its loop"). Meshed at SolidCraft's export spec (a chord of 1/5000 of its size), it meshes:
/// the run of pole nodes is one cap, every face has triangles, nothing folds, and the body's
/// mesh is closed.
#[test]
fn a_sliver_narrower_than_its_pole_reach_meshes_on_the_mesh_path() {
    let b = crate::fixtures::fixture("rig_tapered_text_cut_over_block_edge");
    // SolidCraft's size of the body (`Body::size`): the box diagonal of its vertices and of each
    // edge's points at a quarter, a half and three quarters of its interval.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut add = |p: [f64; 3]| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    for v in &b.vertices {
        add(v.point);
    }
    for e in &b.edges {
        let curve = solidcore_convert::curve(e.parameters()).expect("the edge's curve");
        let c = solidcore::eval::CurveEval::new(&curve).expect("evaluates");
        let [t0, t1] = e.interval;
        for i in 1..4 {
            let p = c.point(t0 + (t1 - t0) * f64::from(i) / 4.0).expect("on the curve");
            add([p.x, p.y, p.z]);
        }
    }
    let size = length(sub(P::new(hi[0], hi[1], hi[2]), P::new(lo[0], lo[1], lo[2])));
    let spec = MeshSpec::chord(crate::MeshTolerance::export_of(size).chord);
    let (plan, _) = meshed(&b, &spec, size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    let mut faces: Vec<u32> = m.tri_face.clone();
    faces.sort_unstable();
    faces.dedup();
    assert_eq!(faces.len(), b.faces.len(), "every face has triangles");
    assert_nothing_folds(&m, "sliver body");
    let (w, refused) = plan.welded().unwrap();
    assert!(refused.is_empty(), "{refused:?}");
    assert_closed(&w, "sliver body");
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

/// The kernel rig's `booleans/2928-join-cap-touching-lips-with-offset-walls`, its body `Base` as
/// SolidCraft exports it (frozen in `tests/data`): two half-annulus lips joined onto a stadium
/// ring with their cylinder walls 0.0001 mm off the base's, which leaves a crescent of the base's
/// top face 0.0001 mm wide between two arcs. Every triangle of that crescent is a sliver of its
/// boundary nodes, too sharp, and its circumcentre encroaches on the arcs' sides; the shaping
/// halved those sides round after round, and the walls' with them, toward the crescent's width,
/// until edge 20 passed 20000 nodes and its faces were refused. Meshed at SolidCraft's export
/// spec (a chord of 1/5000 of its size), a triangle of boundary nodes narrower than the chord is
/// left as it is: the body meshes, nothing folds, the mesh is closed, every face short of its
/// shape is named for a sharp corner or a few degrees, and no edge is near the node bound.
#[test]
fn a_sliver_narrower_than_the_chord_is_not_split_without_end_on_the_mesh_path() {
    let b = crate::fixtures::fixture("rig_join_cap_touching_lips_base");
    // SolidCraft's size of the body (`Body::size`): the box diagonal of its vertices and of each
    // edge's points at a quarter, a half and three quarters of its interval. It only picks the
    // spec SolidCraft exports at (and the size the mesher is given), so the body is meshed as
    // SolidCraft meshes it.
    let (mut lo, mut hi) = ([f64::INFINITY; 3], [f64::NEG_INFINITY; 3]);
    let mut add = |p: [f64; 3]| {
        for k in 0..3 {
            lo[k] = lo[k].min(p[k]);
            hi[k] = hi[k].max(p[k]);
        }
    };
    for v in &b.vertices {
        add(v.point);
    }
    for e in &b.edges {
        let curve = solidcore_convert::curve(e.parameters()).expect("the edge's curve");
        let c = solidcore::eval::CurveEval::new(&curve).expect("evaluates");
        let [t0, t1] = e.interval;
        for i in 1..4 {
            let p = c.point(t0 + (t1 - t0) * f64::from(i) / 4.0).expect("on the curve");
            add([p.x, p.y, p.z]);
        }
    }
    let size = length(sub(P::new(hi[0], hi[1], hi[2]), P::new(lo[0], lo[1], lo[2])));
    let spec = MeshSpec::chord(crate::MeshTolerance::export_of(size).chord);
    let (plan, stats) = meshed(&b, &spec, size, None);
    let (m, refused) = plan.evaluate();
    assert!(refused.is_empty(), "{refused:?}");
    assert_nothing_folds(&m, "lips on a base");
    let (w, refused) = plan.welded().unwrap();
    assert!(refused.is_empty(), "{refused:?}");
    assert_closed(&w, "lips on a base");
    // The faces named short of their shape, as measured: the crescent's faces 9, 10 and 16 for
    // corners sharper than the spec, and faces 5 and 7 for the shaping's rounds alone, with a
    // smallest angle of 25.05 degrees. Faces 11 and 17, the lips' end walls (1 x 18 mm, a side
    // 0.0001 mm long at a corner), reach the spec at the longest side the body's extent gives with
    // its edges counted.
    let named: Vec<usize> = stats.unshaped.iter().map(|u| u.face).collect();
    assert_eq!(named, [5, 7, 9, 10, 16], "{:?}", stats.unshaped);
    for u in &stats.unshaped {
        let corner = u.limits.contains(&crate::ShapeLimit::Corner);
        assert_eq!(corner, [9, 10, 16].contains(&u.face), "{u:?}");
        assert!(corner || u.smallest_angle > 25.05, "{u:?}");
    }
    let most = plan.edges.iter().map(Vec::len).max().unwrap_or(0);
    assert!(most < edges::MAX_EDGE_NODES / 10, "an edge has {most} nodes");
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


/// A face of a spline sphere whose only loop is 45 nodes all at its north pole (a manufacturer
/// IGES fitting's face, #19469) has no area: the mesh path leaves the collapsed loop out and
/// gives the face no triangles, where it refused the body with "a loop of 45 nodes all at one
/// pole". The same face with a real loop beside it keeps meshing that loop.
#[test]
fn a_loop_collapsed_at_a_pole_bounds_no_area_on_the_mesh_path() {
    let s = spline_sphere();
    let face = poles::Face::new(s.clone(), 0.0);
    let nodes: Vec<[f64; 2]> = (0..45).map(|k| [f64::from(k) / 45.0, 1.0]).collect();
    let pts: Vec<Vec3> = nodes.iter().map(|q| s.derivs(q[0], q[1]).expect("evaluates").0).collect();
    let collapsed = seam::Loop { uv: nodes, pts };
    let rules = size::Rules::new(0.05, 22.9, None);
    let (h, speed) = size::face_size(&s, &uv::domain(&s), &rules);
    let shape = Shape { min_angle: 20.0, side: None, size_at: None, sides: Sides::Split };
    let mut stats = PlanStats::default();
    let plan = plan_face(3, "rational spline", &face, std::slice::from_ref(&collapsed), &rules, (h, speed), 1e-6, &mut stats, Lenient::No, Purpose::Mesh(shape));
    let Ok(Planned::Face(fp)) = plan else { panic!("the collapsed loop is left out") };
    assert_eq!(fp.face, 3);
    assert!(fp.triangles.is_empty() && fp.uv.is_empty());
}

/// The same collapsed loop run the other way round the pole has the surface's whole patch on its
/// left: the face has area, but its boundary is one point, so the mesh path refuses it with "a
/// loop of 45 nodes all at one pole" rather than leave a silent hole of zero-area triangles.
#[test]
fn a_loop_collapsed_at_a_pole_with_the_patch_on_its_left_is_refused() {
    let s = spline_sphere();
    let face = poles::Face::new(s.clone(), 0.0);
    let nodes: Vec<[f64; 2]> = (0..45).map(|k| [1.0 - f64::from(k) / 45.0, 1.0]).collect();
    let pts: Vec<Vec3> = nodes.iter().map(|q| s.derivs(q[0], q[1]).expect("evaluates").0).collect();
    let collapsed = seam::Loop { uv: nodes, pts };
    let rules = size::Rules::new(0.05, 22.9, None);
    let (h, speed) = size::face_size(&s, &uv::domain(&s), &rules);
    let shape = Shape { min_angle: 20.0, side: None, size_at: None, sides: Sides::Split };
    let mut stats = PlanStats::default();
    let plan = plan_face(3, "rational spline", &face, std::slice::from_ref(&collapsed), &rules, (h, speed), 1e-6, &mut stats, Lenient::No, Purpose::Mesh(shape));
    match plan {
        Err(why) => {
            let why = format!("{why:?}");
            assert!(why.contains("all at one pole") || why.contains("boundary winds around the surface"), "refused for another reason: {why}");
        }
        Ok(_) => panic!("a face whose boundary is one point was planned"),
    }
}

/// A loop with every node at a pole is refused ("a loop of N nodes all at one pole") when its
/// nodes are one point in 3D, and is no cap to fan (a sliver, meshed by its boundary nodes) when
/// they are spread along the collapsed line: the cap planner tells the two apart by the 3D points.
#[test]
fn an_all_pole_loop_is_refused_when_one_point_and_left_to_its_nodes_when_spread() {
    let s = spline_sphere();
    let face = poles::Face::new(s.clone(), 0.0);
    let uvs: Vec<[f64; 2]> = (0..45).map(|k| [f64::from(k) / 45.0, 1.0]).collect();
    let whole = 0..uvs.len();
    let ranges = std::slice::from_ref(&whole);
    let one_point: Vec<Vec3> = uvs.iter().map(|_| Vec3::new(0.0, 0.0, 1.0)).collect();
    let why = super::pole_caps(&face, &uvs, &one_point, ranges, 1e-3).err();
    assert_eq!(why.as_deref(), Some("a loop of 45 nodes all at one pole"));
    let spread: Vec<Vec3> = (0..45).map(|k| Vec3::new(f64::from(k), 0.0, 1.0)).collect();
    let caps = super::pole_caps(&face, &uvs, &spread, ranges, 1e-3).expect("a sliver is not refused");
    assert!(caps.is_empty());
}

/// A sliver face narrower than the reach all along (a degree 1 patch 5.6 mm long and 0.0002 mm
/// wide, cut along its diagonal: a triangle, #20167) has every boundary node at a collapsed
/// line, though its loop is no point. The mesh path (with its sides kept whole, as the planner
/// plans a face whose shaping would split sides on no edge) triangulates it by its boundary nodes
/// instead of refusing it with "a loop of nodes all at one pole".
#[test]
fn a_sliver_narrower_than_the_reach_is_meshed_by_its_boundary_nodes() {
    let ctrl = vec![[0.0, 0.0, 0.0, 1.0], [5.6, 0.0, 0.0, 1.0], [0.0, 0.0, 2e-4, 1.0], [5.6, 0.0, 2e-4, 1.0]];
    let s = EvalSurface::nurbs([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], [2, 2], ctrl);
    let face = poles::Face::new(s.clone(), 0.0);
    let mut nodes: Vec<[f64; 2]> = Vec::new();
    for k in 0..4 {
        let t = f64::from(k) / 4.0;
        nodes.push([t, 0.0]);
    }
    for k in 0..4 {
        let t = f64::from(k) / 4.0;
        nodes.push([1.0, t]);
    }
    for k in 0..4 {
        let t = f64::from(k) / 4.0;
        nodes.push([1.0 - t, 1.0 - t]);
    }
    let pts: Vec<Vec3> = nodes.iter().map(|q| s.derivs(q[0], q[1]).expect("evaluates").0).collect();
    let sliver = seam::Loop { uv: nodes, pts };
    let rules = size::Rules::new(0.001, 22.9, None);
    let (h, speed) = size::face_size(&s, &uv::domain(&s), &rules);
    let shape = Shape { min_angle: 20.0, side: None, size_at: None, sides: Sides::Kept };
    let mut stats = PlanStats::default();
    let plan = plan_face(7, "spline", &face, std::slice::from_ref(&sliver), &rules, (h, speed), 1e-3, &mut stats, Lenient::No, Purpose::Mesh(shape));
    let fp = match plan {
        Ok(Planned::Face(fp)) => fp,
        Ok(Planned::Split(sides)) => panic!("the sliver asked for {} sides split", sides.len()),
        Err(why) => panic!("the sliver is meshed, not refused: {why:?}"),
    };
    assert_eq!(fp.face, 7);
    assert!(!fp.triangles.is_empty());
    let n = u32::try_from(fp.uv.len()).expect("a count");
    assert!(fp.triangles.iter().all(|t| t.iter().all(|&i| i < n)));
}

/// A flat open patch 0.0005 mm by 0.0002 mm, every boundary node within the reach (0.001 mm) of a
/// collapsed line and of one point, has an area of 1e-7 mm2 though its surface has no period to
/// run round: the mesh path never gives it an empty plan (it meshes it or refuses it).
#[test]
fn a_tiny_open_patch_within_the_reach_is_never_given_an_empty_plan() {
    let ctrl = vec![[0.0, 0.0, 0.0, 1.0], [5e-4, 0.0, 0.0, 1.0], [0.0, 2e-4, 0.0, 1.0], [5e-4, 2e-4, 0.0, 1.0]];
    let s = EvalSurface::nurbs([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], [2, 2], ctrl);
    let face = poles::Face::new(s.clone(), 0.0);
    let nodes: Vec<[f64; 2]> = vec![[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, 0.5], [1.0, 1.0], [0.5, 1.0], [0.0, 1.0], [0.0, 0.5]];
    let pts: Vec<Vec3> = nodes.iter().map(|q| s.derivs(q[0], q[1]).expect("evaluates").0).collect();
    let tiny = seam::Loop { uv: nodes, pts };
    let rules = size::Rules::new(0.001, 22.9, None);
    let (h, speed) = size::face_size(&s, &uv::domain(&s), &rules);
    let shape = Shape { min_angle: 20.0, side: None, size_at: None, sides: Sides::Kept };
    let mut stats = PlanStats::default();
    let plan = plan_face(9, "spline", &face, std::slice::from_ref(&tiny), &rules, (h, speed), 1e-3, &mut stats, Lenient::No, Purpose::Mesh(shape));
    if let Ok(Planned::Face(fp)) = plan {
        assert!(!fp.triangles.is_empty(), "a face with area was given an empty plan");
    }
}

/// A flat open patch whose v span is 1e-10 of its domain (0.0005 mm by 2e-14 mm) has area: its
/// nodes are within the reach of one point, but the span is no rounding of a pole line, so the
/// mesh path never gives it an empty plan.
#[test]
fn an_open_patch_with_a_span_under_a_billionth_of_its_domain_is_never_given_an_empty_plan() {
    let ctrl = vec![[0.0, 0.0, 0.0, 1.0], [5e-4, 0.0, 0.0, 1.0], [0.0, 2e-4, 0.0, 1.0], [5e-4, 2e-4, 0.0, 1.0]];
    let s = EvalSurface::nurbs([1, 1], [vec![0.0, 0.0, 1.0, 1.0], vec![0.0, 0.0, 1.0, 1.0]], [2, 2], ctrl);
    let face = poles::Face::new(s.clone(), 0.0);
    let top = 1e-10;
    let nodes: Vec<[f64; 2]> = vec![[0.0, 0.0], [0.5, 0.0], [1.0, 0.0], [1.0, top], [0.5, top], [0.0, top]];
    let pts: Vec<Vec3> = nodes.iter().map(|q| s.derivs(q[0], q[1]).expect("evaluates").0).collect();
    let thin = seam::Loop { uv: nodes, pts };
    let rules = size::Rules::new(0.001, 22.9, None);
    let (h, speed) = size::face_size(&s, &uv::domain(&s), &rules);
    let shape = Shape { min_angle: 20.0, side: None, size_at: None, sides: Sides::Kept };
    let mut stats = PlanStats::default();
    let plan = plan_face(9, "spline", &face, std::slice::from_ref(&thin), &rules, (h, speed), 1e-3, &mut stats, Lenient::No, Purpose::Mesh(shape));
    if let Ok(Planned::Face(fp)) = plan {
        assert!(!fp.triangles.is_empty(), "a face with area was given an empty plan");
    }
}

/// The (u, v) points and 3D points of a cap test: ring nodes 0..=5 along v = 0 from (0, 0) to
/// (1, 0), and the run's nodes from 10 on, at the given (u, v) (their 3D points: the same, at
/// z = 0).
fn cap_case(run: &[[f64; 2]]) -> (Vec<[f64; 2]>, Vec<Vec3>) {
    let mut uv = vec![[f64::NAN; 2]; 10 + run.len()];
    for (k, q) in uv.iter_mut().enumerate().take(6) {
        *q = [k as f64 / 5.0, 0.0];
    }
    for (k, q) in run.iter().enumerate() {
        uv[10 + k] = *q;
    }
    let pos = uv.iter().map(|q| Vec3::new(q[0], q[1], 0.0)).collect();
    (uv, pos)
}

/// Every ring segment of `ring` is in exactly one triangle of `tris`, every triangle is
/// counter-clockwise in (u, v), and the triangles cover the polygon of the ring and the run.
fn assert_fan_covers(tris: &[[u32; 3]], ring: &[usize], run: &[usize], uv: &[[f64; 2]]) {
    let area = |[a, b, c]: [usize; 3]| {
        let (qa, qb, qc) = (uv[a], uv[b], uv[c]);
        0.5 * ((qb[0] - qa[0]) * (qc[1] - qa[1]) - (qb[1] - qa[1]) * (qc[0] - qa[0]))
    };
    for w in ring.windows(2) {
        let uses = tris.iter().filter(|t| t.contains(&(w[0] as u32)) && t.contains(&(w[1] as u32))).count();
        assert_eq!(uses, 1, "ring segment {w:?} in {uses} triangles: {tris:?}");
    }
    let mut sum = 0.0;
    for t in tris {
        let a = area(t.map(|i| i as usize));
        assert!(a > 0.0, "triangle {t:?} is not counter-clockwise: {a}");
        sum += a;
    }
    // The polygon: the ring from its start to its end, then the run back.
    let poly: Vec<usize> = ring.iter().copied().chain(run.iter().rev().copied()).collect();
    let whole: f64 = (0..poly.len()).map(|k| {
        let (p, q) = (uv[poly[k]], uv[poly[(k + 1) % poly.len()]]);
        0.5 * (p[0] * q[1] - q[0] * p[1])
    }).sum();
    assert!((sum - whole).abs() <= 1e-12, "the fan covers {sum}, the cap {whole}");
}

/// A run of poles at two points is fanned from each to an even share of the ring's inner nodes,
/// with one triangle where they meet; the fan covers the cap once.
#[test]
fn a_pole_run_at_two_points_is_fanned_evenly() {
    let run = [[0.4, 1.0], [0.6, 1.0]];
    let (uv, pos) = cap_case(&run);
    let ring = [0, 1, 2, 3, 4, 5];
    let tris = cap_fan(&[10, 11], &ring, &uv, &pos).expect("fanned");
    assert_fan_covers(&tris, &ring, &[10, 11], &uv);
    // Four inner segments, two each, then the ring's last segment from the last point.
    let from = |c: u32| tris.iter().filter(|t| t.contains(&c) && !t.contains(&(if c == 10 { 11 } else { 10 }))).count();
    assert_eq!((from(10), from(11)), (2, 3), "{tris:?}");
}

/// A run at more points than the ring has inner nodes still covers the cap once.
#[test]
fn a_pole_run_at_more_points_than_inner_nodes_is_fanned() {
    let run = [[0.3, 1.0], [0.5, 1.0], [0.7, 1.0]];
    let (uv, pos) = cap_case(&run);
    let ring = [0, 2, 4, 5];
    let tris = cap_fan(&[10, 11, 12], &ring, &uv, &pos).expect("fanned");
    assert_fan_covers(&tris, &ring, &[10, 11, 12], &uv);
}

/// A run that bends toward the ring past what its share of the ring can see would fan into
/// overlapping triangles: it is not fanned.
#[test]
fn a_pole_run_bending_toward_its_ring_is_not_fanned_over_itself() {
    let run = [[0.3, 1.0], [0.5, 0.1], [0.7, 1.0]];
    let (uv, pos) = cap_case(&run);
    assert!(cap_fan(&[10, 11, 12], &[0, 2, 4, 5], &uv, &pos).is_none());
}

/// A run at two points needs an inner node on its ring; copies of one point do not.
#[test]
fn a_pole_run_at_two_points_needs_an_inner_ring_node() {
    let (uv, pos) = cap_case(&[[0.4, 1.0], [0.6, 1.0]]);
    assert!(cap_fan(&[10, 11], &[0, 5], &uv, &pos).is_none());
    let (uv, mut pos) = cap_case(&[[0.4, 1.0], [0.6, 1.0]]);
    pos[11] = pos[10];
    let tris = cap_fan(&[10, 11], &[0, 5], &uv, &pos).expect("one point");
    assert_fan_covers(&tris, &[0, 5], &[10, 11], &uv);
}

/// The ring line from (0, 0) to (1, 0) with the pole above it at (0.5, 1): the loop's other
/// nodes, and the sides it asks to split.
fn ring_hits(rest: &[[f64; 2]], hole: &[[f64; 2]], pole: [f64; 2]) -> Vec<SplitSide> {
    // Loop: before (0), pole (1), after (2), then the rest; the hole is a loop of its own.
    let mut uvs = vec![[0.0, 0.0], pole, [1.0, 0.0]];
    uvs.extend_from_slice(rest);
    let outer = 0..uvs.len();
    uvs.extend_from_slice(hole);
    let ranges = [outer.clone(), outer.end..uvs.len()];
    let fixed: Vec<Vec3> = uvs.iter().map(|q| Vec3::new(q[0], q[1], 0.0)).collect();
    let cap = PoleCap { lp: 0, copies: vec![1], before: 0, after: 2 };
    ring_off_boundary(&cap, &uvs, &fixed, &ranges)
}

/// The ring is moved only where a node of a loop lies on it or on the pole's side of it within
/// its span: not for nodes beyond its ends or across it, nor where the pole's side is unknown.
#[test]
fn a_pole_ring_is_moved_only_off_a_boundary_it_meets() {
    let below = [[0.5, -1.0]];
    assert!(ring_hits(&below, &[], [0.5, 1.0]).is_empty(), "the rest of the loop lies across the ring");
    assert_eq!(ring_hits(&[[1.5, 0.0], [0.5, -1.0], [-0.5, 0.0]], &[], [0.5, 1.0]).len(), 0, "nodes beyond its ends");
    assert_eq!(ring_hits(&[[0.0, -1.0], [0.0, 0.0]], &[], [0.5, 1.0]).len(), 0, "a node at its start (t = 0)");
    let on = ring_hits(&[[0.5, 0.0], [0.5, -1.0]], &[], [0.5, 1.0]);
    assert_eq!(on.len(), 2, "a node on the ring");
    assert!(on.iter().all(|s| s.at == [0.5]));
    assert_eq!(ring_hits(&below, &[[0.5, 0.3], [0.6, 0.4], [0.4, 0.4]], [0.5, 1.0]).len(), 2, "a hole on the pole's side");
    // Its upper nodes past the ring's ends, its lower one across it: only its sides cross.
    assert_eq!(ring_hits(&below, &[[-0.1, 0.2], [1.1, 0.2], [0.5, -0.1]], [0.5, 1.0]).len(), 2, "a hole reaching over the ring");
    assert!(ring_hits(&[[0.5, 0.0]], &[], [0.5, f64::NAN]).is_empty(), "no pole side: no split");
    assert!(ring_hits(&[[0.5, 0.0]], &[], [0.5, 0.0]).is_empty(), "the pole on the ring line: no split");
}
