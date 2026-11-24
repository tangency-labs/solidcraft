//! The fork's perforated plate (tangency-labs/smlib@edf7414, capi test `ops_kernel`; the
//! case of WR-527, #1526/#1527, #2363), ported: k x k r 2 holes on a 20 mm pitch, k = 8
//! and 16, united in balanced rounds with `boolean_2d` and cut from the plate in 2D.
//! Upstream's work there grows with the square of the holes: `Boolean2D` grows every
//! face's plane to cover the whole sheet, so every pair of faces passed the coincident
//! surface check and classified its edges on the other face, and each hole imprinted in
//! the plate classified every hole imprinted before it.
//!
//! The fork's test checked a ratio of times (`t256 <= 8 t64 + 0.1`), which tripped under
//! load. It now counts the work (`SmWorkCounters`, here [`Kernel::work_counters`]) and
//! prints the time without checking it. With WR-527's box tests the holes' boxes
//! lie apart, so the unions pair no faces, and the cut pairs the plate's one face with each
//! hole: k^2 face pairs, 64 and 256, four times the pairs for four times the holes. Each
//! hole imprinted in the plate lies outside the other holes' boxes, so no inner loop is
//! classified: 0 and 0. Without that box test (the fork before edf7414, and this port
//! before it became the only path) the same run classified 4032 = 64 x 63 and
//! 65280 = 256 x 255 loops, the n^2 the count is there to catch. Upstream also pairs every face, 2080 = 64 x 65 / 2 and
//! 32896 = 256 x 257 / 2 pairs, which this test refuses. The plate
//! keeps its exact area either way.
//!
//! Not the fork's: the same plate as a solid, 12 thick, with its holes cut in 3D by one
//! `boolean_lists` call (`plate_cut_marks_one_face_for_each_face_it_deletes`,
//! `plate_cut_solves_no_proto_vertex_that_has_proto_edges`).
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;
use std::time::Instant;

use solidcore::boolean::api_brep::{
    boolean as boolean_3d, boolean_2d, boolean_lists, boolean_union, Boolean2d, Boolean2dOutcome,
};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::a_object::BooleanOperation;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::kernel::UpstreamFixes;
use solidcore::mass::integrator::properties::{brep_area, brep_volume_default};
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder};
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::topology::brep::p1;
use solidcore::topology::brep::p2::is_manifold_solid;
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::{Brep, Kernel};

const RADIUS: f64 = 2.0;
const PITCH: f64 = 20.0;

fn boolean(k: &mut Kernel, a: Brep, b: Brep, op: Boolean2d) -> Brep {
    match boolean_2d(k, a, b, op).unwrap() {
        Boolean2dOutcome::Result(r) => r,
        Boolean2dOutcome::Unassigned { .. } => panic!("2D boolean left its operands"),
    }
}

/// A planar face bounded by the circle r 2 about (x, y, 0) (`smlib_planar_faces` of one
/// full arc: `SmApiCreatePlanarFaces` of an `SmCircle`).
fn disk(k: &mut Kernel, x: f64, y: f64) -> Brep {
    let frame = Axis2Placement::new(Point3::new(x, y, 0.0), Vec3::X, Vec3::Y);
    let circle = Ellipse::create_canonical_circle(k, &frame, RADIUS, None, None).unwrap();
    create_planar_faces(k, &[Curve::from(circle)]).unwrap()
}

/// The `w` x `w` plate from the origin (`smlib_planar_faces` of its four sides).
fn plate(k: &mut Kernel, w: f64) -> Brep {
    let corners = [(0.0, 0.0), (w, 0.0), (w, w), (0.0, w), (0.0, 0.0)];
    let sides: Vec<Curve> = corners
        .windows(2)
        .map(|s| {
            let (start, end) = (s[0], s[1]);
            Curve::from(
                create_line_segment(
                    Point3::new(start.0, start.1, 0.0),
                    Point3::new(end.0, end.1, 0.0),
                )
                .unwrap(),
            )
        })
        .collect();
    create_planar_faces(k, &sides).unwrap()
}

/// The plate with `n` x `n` holes cut in 2D: checks its exact area, its face pairs (n^2)
/// and its inner loops classified (none), and returns the face pairs.
fn plate_pairs(n: u32) -> u64 {
    let mut k = Kernel::default();
    let t0 = Instant::now();
    k.work_counters.reset();
    let mut holes = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (PITCH * f64::from(i + 1), PITCH * f64::from(j + 1));
            holes.push(disk(&mut k, x, y));
        }
    }
    // united in balanced rounds
    while holes.len() > 1 {
        let mut next = Vec::new();
        let mut round = holes.into_iter();
        while let Some(a) = round.next() {
            match round.next() {
                Some(b) => next.push(boolean(&mut k, a, b, Boolean2d::Union)),
                None => next.push(a),
            }
        }
        holes = next;
    }
    let w = PITCH * f64::from(n + 1);
    let plate = plate(&mut k, w);
    let holes = holes.pop().unwrap();
    assert_eq!(holes.face_list().len(), (n * n) as usize);
    let mut cut = boolean(&mut k, plate, holes, Boolean2d::Difference);
    let seconds = t0.elapsed().as_secs_f64();
    let pairs = k.work_counters.face_pairs();
    let loops = k.work_counters.loops_classified();
    let area = brep_area(&k, &mut cut, 1e-8).unwrap();
    let exact = w * w - f64::from(n * n) * PI * RADIUS * RADIUS;
    println!(
        "plate with {} holes in 2D: {seconds:.3} s, area {area:.6}, exact {exact:.6}",
        n * n
    );
    println!("  {pairs} face pairs examined, {loops} inner loops classified");
    assert!(
        (area - exact).abs() <= 1e-7 * exact.abs(),
        "area {area}, exact {exact}"
    );
    assert_eq!((pairs, loops), (u64::from(n * n), 0));
    pairs
}

#[test]
fn fork_edf7414_perforated_plate_work_grows_as_the_holes() {
    let (pairs64, pairs256) = (plate_pairs(8), plate_pairs(16));
    assert_eq!(pairs256, 4 * pairs64);
}

/// The `w` x `w` plate's sides and its `n` x `n` hole circles r 2 on the pitch, as one
/// list of curves.
fn plate_and_hole_curves(k: &mut Kernel, n: u32) -> (Vec<Curve>, f64) {
    let w = PITCH * f64::from(n + 1);
    let corners = [(0.0, 0.0), (w, 0.0), (w, w), (0.0, w), (0.0, 0.0)];
    let mut curves: Vec<Curve> = corners
        .windows(2)
        .map(|s| {
            let (start, end) = (s[0], s[1]);
            Curve::from(
                create_line_segment(
                    Point3::new(start.0, start.1, 0.0),
                    Point3::new(end.0, end.1, 0.0),
                )
                .unwrap(),
            )
        })
        .collect();
    for i in 0..n {
        for j in 0..n {
            let centre = Point3::new(PITCH * f64::from(i + 1), PITCH * f64::from(j + 1), 0.0);
            let frame = Axis2Placement::new(centre, Vec3::X, Vec3::Y);
            let circle = Ellipse::create_canonical_circle(k, &frame, RADIUS, None, None).unwrap();
            curves.push(Curve::from(circle));
        }
    }
    (curves, w)
}

/// The plate with its `n` x `n` holes made in one call (`create_planar_faces` of its sides
/// and the hole circles, `CreatePlanarFacesWith3DCurves`): checks its one face, its loops
/// and its exact area, and returns the loop walks it took.
///
/// Each hole's loop goes into the face its first point classifies in: the natural face
/// (walking its two loops), then the plate, whose outer loop is walked. The plate holds
/// every hole placed before as an inner loop, and the point lies outside each of them,
/// by more than the walk's tolerance: their walks cannot find it inside or on them, so
/// they are not walked. Three walks a hole, where walking every hole placed before cost
/// `N (N - 1) / 2` more: 2016 for 64 holes, 32640 for 256.
fn plate_in_one_call_walks(n: u32) -> u64 {
    let mut k = Kernel::new(&solidcore::kernel::KernelOptions::default());
    k.memoize_curve_caches = true; // the count is the shortcut's, with the memo on
    let (curves, w) = plate_and_hole_curves(&mut k, n);
    let t0 = Instant::now();
    k.work_counters.reset();
    let mut plate = create_planar_faces(&mut k, &curves).unwrap();
    let seconds = t0.elapsed().as_secs_f64();
    let walks = k.work_counters.loop_walks();
    let holes = u64::from(n * n);
    assert_eq!(plate.face_list().len(), 1);
    assert_eq!(plate.loops().iter().count() as u64, holes + 1);
    let area = brep_area(&k, &mut plate, 1e-8).unwrap();
    let exact = w * w - f64::from(n * n) * PI * RADIUS * RADIUS;
    println!("plate with {holes} holes in one call: {seconds:.3} s, {walks} loop walks");
    assert!(
        (area - exact).abs() <= 1e-7 * exact.abs(),
        "area {area}, exact {exact}"
    );
    walks
}

#[test]
fn a_plate_made_in_one_call_walks_each_hole_a_fixed_number_of_times() {
    assert_eq!(plate_in_one_call_walks(8), 3 * 64);
    assert_eq!(plate_in_one_call_walks(16), 3 * 256);
}

/// The plate's point classifications (`point_3d_classify`, as a hole's loop is placed)
/// are the same, field for field, whether its holes' walks are left out or every loop is
/// walked (`memoize_curve_caches` off): at a grid over the plate and its border, and at
/// points on each hole's circle, inside and outside it by a tenth, one and ten times the
/// classification tolerance, and at its centre. Leaving them out walks fewer loops.
#[test]
fn a_plate_classifies_its_points_the_same_with_its_holes_left_out() {
    use solidcore::intersect::point_class::{PointClassType, PointClassification};
    use solidcore::query::face::p1::{point_3d_classify, Point3dClassifyOptions};
    use solidcore::EntityRef;

    let n = 4;
    let mut k = Kernel::default();
    let (curves, w) = plate_and_hole_curves(&mut k, n);
    let plate = create_planar_faces(&mut k, &curves).unwrap();
    let f = plate.face_list()[0];
    let tol = 1e-6;
    let mut points = Vec::new();
    for i in -2..=42 {
        for j in -2..=42 {
            points.push(Point3::new(
                w * f64::from(i) / 40.0,
                w * f64::from(j) / 40.0,
                0.0,
            ));
        }
    }
    for i in 1..=n {
        for j in 1..=n {
            let (x, y) = (PITCH * f64::from(i), PITCH * f64::from(j));
            points.push(Point3::new(x, y, 0.0));
            for a in 0..12 {
                let (s, c) = (f64::from(a) * PI / 6.0 + 0.1).sin_cos();
                for d in [0.0, 0.1, -0.1, 1.0, -1.0, 10.0, -10.0] {
                    let r = RADIUS + d * tol;
                    points.push(Point3::new(x + r * c, y + r * s, 0.0));
                }
            }
        }
    }
    let classify = |memo: bool| {
        let mut k = Kernel::new(&solidcore::kernel::KernelOptions::default());
        k.memoize_curve_caches = memo;
        let mut b = plate.clone_standing_for_self().unwrap();
        let classes: Vec<(String, PointClassType)> = points
            .iter()
            .map(|&p| {
                let mut pc = PointClassification::<EntityRef>::new(tol);
                let r = point_3d_classify(
                    &k,
                    &mut b,
                    f,
                    p,
                    tol,
                    false,
                    &mut pc,
                    Point3dClassifyOptions::default(),
                );
                (format!("{r:?} {pc:?}"), pc.point_class())
            })
            .collect();
        (classes, k.work_counters.loop_walks())
    };
    let (left_out, fewer) = classify(true);
    let (walked, every) = classify(false);
    for ((p, a), b) in points.iter().zip(&left_out).zip(&walked) {
        assert_eq!(a, b, "at {p:?}");
    }
    // the points take every class the face has: in it, on an edge, outside it
    for class in [
        PointClassType::Face,
        PointClassType::Edge,
        PointClassType::Unknown,
    ] {
        assert!(walked.iter().any(|c| c.1 == class), "{class:?}");
    }
    println!(
        "{} points: {fewer} loop walks, {every} walking every loop",
        points.len()
    );
    assert!(fewer < every / 4, "{fewer} of {every}");
}

/// The plate with its `n` x `n` holes extruded 12 high in one sweep (`extrude_region` of
/// the outline and the hole circles, as a sketch and an extrude make it, with every fix
/// on as SolidCraft runs): checks its topology and its exact volume, and returns the loop
/// walks it took. Both caps are made as [`plate_in_one_call_walks`]' plate, and the rest
/// of the sweep walks four loops whatever the holes.
fn plate_extruded_walks(n: u32) -> u64 {
    use solidcore::eval::Frame;
    use solidcore::kernel::UpstreamFixes;
    use solidcore::mass::integrator::properties::brep_volume_default;
    use solidcore::math::Point2;
    use solidcore::prism::extrude::extrude_region;
    use solidcore::prism::profile::{Loop2, Region2, Seg2};

    let mut k = Kernel::new(&solidcore::kernel::KernelOptions::default());
    k.memoize_curve_caches = true; // the count is the shortcut's, with the memo on
    k.fixes = UpstreamFixes::all();
    let w = PITCH * f64::from(n + 1);
    let q = Point2::new;
    let corners = [q(0.0, 0.0), q(w, 0.0), q(w, w), q(0.0, w)];
    let outer = Loop2 {
        segs: (0..4)
            .map(|i| Seg2::Line {
                a: corners[i],
                b: corners[(i + 1) % 4],
            })
            .collect(),
    };
    let mut holes = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let center = q(PITCH * f64::from(i + 1), PITCH * f64::from(j + 1));
            let circle = Loop2 {
                segs: vec![Seg2::Arc {
                    center,
                    radius: RADIUS,
                    start: 0.0,
                    sweep: 2.0 * PI,
                }],
            };
            holes.push(circle.reversed());
        }
    }
    let frame = Frame {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    let t0 = Instant::now();
    k.work_counters.reset();
    let mut body = extrude_region(&mut k, &frame, &Region2 { outer, holes }, 0.0, 12.0).unwrap();
    let seconds = t0.elapsed().as_secs_f64();
    let walks = k.work_counters.loop_walks();
    let holes = n * n;
    // the caps, the four sides and each hole's wall, one face closed by a seam edge
    // between its two circles (V - E + F - (L - F) = 2 (1 - genus), genus = holes)
    let counts = (
        body.face_list().len(),
        body.edge_list().len(),
        body.loops().iter().count(),
        body.vertex_list().len(),
    );
    let expected = |a: u32, b: u32| (a * holes + b) as usize;
    assert_eq!(
        counts,
        (
            expected(1, 6),
            expected(3, 12),
            expected(3, 6),
            expected(2, 8)
        )
    );
    let volume = brep_volume_default(&k, &mut body, 1e-9).unwrap();
    let exact = 12.0 * (w * w - f64::from(holes) * PI * RADIUS * RADIUS);
    println!("plate with {holes} holes extruded: {seconds:.3} s, {walks} loop walks");
    assert!(
        (volume - exact).abs() <= 1e-7 * exact,
        "volume {volume}, exact {exact}"
    );
    walks
}

#[test]
fn an_extruded_plate_walks_each_hole_a_fixed_number_of_times() {
    assert_eq!(plate_extruded_walks(8), 2 * 3 * 64 + 4);
    assert_eq!(plate_extruded_walks(16), 2 * 3 * 256 + 4);
}

/// The solid plate, 12 thick, minus `n` x `n` cylinders r 4 through it on the 20 mm
/// pitch, in one `boolean_lists` call with SolidCraft's fixes: checks the cut (a wall
/// for each hole, three edges for each hole, the exact volume) and returns the faces its
/// shell traversals marked and the point solves it ran of a proto vertex against a proto
/// edge.
fn plate_cut_counts(n: u32) -> (u64, u64) {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let (w, thick, radius) = (PITCH * f64::from(n), 12.0, 4.0);
    let plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), w, w, thick).unwrap();
    let mut tools = Vec::new();
    for i in 0..n {
        for j in 0..n {
            let (x, y) = (PITCH * (f64::from(i) + 0.5), PITCH * (f64::from(j) + 0.5));
            let base = Point3::new(x, y, -1.0);
            tools.push(create_cylinder(&mut k, base, radius, thick + 2.0).unwrap());
        }
    }
    let holes = (n * n) as usize;
    k.work_counters.reset();
    let mut cut = boolean_lists(&mut k, &[plate], &[], &tools, &[], 3)
        .unwrap()
        .breps;
    let marked = k.work_counters.shell_traversal_faces();
    let solves = k.work_counters.proto_vertex_point_solves();
    assert_eq!(cut.len(), 1);
    let cut = &mut cut[0];
    assert_eq!(p1::faces(cut, None).unwrap().len(), holes + 6);
    assert_eq!(p1::edges(cut, None).unwrap().len(), 3 * holes + 12);
    let volume = brep_volume_default(&k, cut, 1e-6).unwrap();
    let exact = w * w * thick - holes as f64 * PI * radius * radius * thick;
    println!("plate minus {holes} cylinders: volume {volume:.6}, exact {exact:.6}");
    println!("  {marked} faces marked by shell traversals");
    println!("  {solves} point solves of a proto vertex against a proto edge");
    assert!(
        (volume - exact).abs() <= 1e-6 * exact,
        "volume {volume}, exact {exact}"
    );
    (marked, solves)
}

/// The cut deletes the two discs each cylinder cuts out of the plate's top and bottom,
/// and each deletion asks whether the disc's one vertex is still joined to the shell.
/// It is, through the first face at that vertex: one face marked for each face deleted,
/// 2 N for N holes. Walking on over the rest of the shell marked
/// (7/2) N^2 + (23/2) N faces: 2475 for 25 holes and 36150 for 100.
#[test]
fn plate_cut_marks_one_face_for_each_face_it_deletes() {
    let marked = (plate_cut_counts(5).0, plate_cut_counts(10).0);
    assert_eq!(marked, (2 * 25, 2 * 100));
}

/// Each cylinder meets the plate's top and bottom in a circle: a closed proto edge that
/// starts and ends at its one proto vertex, 2 N of each for N holes. The boolean removes
/// a proto vertex lying on a proto edge only when it has no proto edges of its own, and
/// here every proto vertex has one, so nothing can be removed and no point solve is
/// needed: 0 and 0. Solving each proto vertex against every other proto edge ran
/// 4 N^2 - 2 N solves: 2450 for 25 holes and 39800 for 100.
#[test]
fn plate_cut_solves_no_proto_vertex_that_has_proto_edges() {
    let solves = (plate_cut_counts(5).1, plate_cut_counts(10).1);
    assert_eq!(solves, (0, 0));
}

// ---- the plate as a solid, cut a group of holes at a time ---------------------------------

/// The solid version, as SolidCraft cuts a pattern of holes: a 2 mm plate with `n` x `n`
/// holes r 0.5 on a 2.54 mm pitch, cut as cylinders a group of 8 at a time, one group after
/// another, then checked. Returns the drops of the plate's vertices onto the curves the cuts
/// classify on its faces.
///
/// A vertex can only meet a curve within the drop's tolerance, and a hole's circles lie far
/// from the vertices of the holes cut before: only the 2 drops of each hole's own cut are
/// solved, 32 and 72 for 16 and 36 holes, where every curve used to be dropped onto every
/// vertex in the plate, 352 and 1528, growing with the square of the holes. The plate keeps
/// one cylinder face per hole and checks clean either way.
fn solid_plate_vertex_drops(n: u32) -> u64 {
    const HOLE: f64 = 0.5;
    const STEP: f64 = 2.54;
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    k.memoize_curve_caches = true; // the count is the shortcut's, with the memo on
    let w = STEP * f64::from(n + 1);
    let mut plate = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), w, w, 2.0).unwrap();
    let holes: Vec<Brep> = (0..n * n)
        .map(|i| {
            let (x, y) = (STEP * f64::from(i % n + 1), STEP * f64::from(i / n + 1));
            create_cylinder(&mut k, Point3::new(x, y, -1.0), HOLE, 4.0).unwrap()
        })
        .collect();
    k.work_counters.reset();
    let mut holes = holes.into_iter();
    loop {
        let group: Vec<Brep> = holes.by_ref().take(8).collect();
        let mut group = group.into_iter();
        let Some(first) = group.next() else { break };
        let tool = group.fold(first, |a, b| boolean_union(&mut k, a, b).unwrap());
        plate = boolean_3d(&mut k, plate, tool, BooleanOperation::Difference)
            .unwrap()
            .result;
    }
    plate.use_arena_naming().unwrap();
    let lines = check_brep(&mut k, &mut plate, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
    assert!(is_manifold_solid(&plate).unwrap());
    assert_eq!(plate.face_list().len(), 6 + (n * n) as usize);
    let drops = k.work_counters.vertex_drops();
    println!("solid plate with {} holes: {drops} vertex drops", n * n);
    drops
}

#[test]
fn a_solid_plate_cut_a_group_at_a_time_drops_only_its_own_vertices() {
    assert_eq!(solid_plate_vertex_drops(4), 32);
    assert_eq!(solid_plate_vertex_drops(6), 72);
}

// ---- a plate joined onto its own top: one face meeting many ------------------------------

/// The plate with `n` x `n` holes extruded from `z0` to `z1` (as [`plate_extruded_walks`]'
/// plate, with every fix on as SolidCraft runs).
fn plate_prism(k: &mut Kernel, n: u32, z0: f64, z1: f64) -> Brep {
    use solidcore::eval::Frame;
    use solidcore::math::Point2;
    use solidcore::prism::extrude::extrude_region;
    use solidcore::prism::profile::{Loop2, Region2, Seg2};

    let w = PITCH * f64::from(n + 1);
    let q = Point2::new;
    let corners = [q(0.0, 0.0), q(w, 0.0), q(w, w), q(0.0, w)];
    let outer = Loop2 {
        segs: (0..4)
            .map(|i| Seg2::Line {
                a: corners[i],
                b: corners[(i + 1) % 4],
            })
            .collect(),
    };
    let holes = (0..n * n)
        .map(|i| {
            let center = q(PITCH * f64::from(i % n + 1), PITCH * f64::from(i / n + 1));
            Loop2 {
                segs: vec![Seg2::Arc {
                    center,
                    radius: RADIUS,
                    start: 0.0,
                    sweep: 2.0 * PI,
                }],
            }
            .reversed()
        })
        .collect();
    let frame = Frame {
        origin: [0.0; 3],
        x_axis: [1.0, 0.0, 0.0],
        y_axis: [0.0, 1.0, 0.0],
    };
    extrude_region(k, &frame, &Region2 { outer, holes }, z0, z1).unwrap()
}

/// The plate with `n` x `n` holes, 12 thick, and the same plate stacked on its top, 12
/// more, joined in one boolean (as an extrude of the top face that joins): checks the
/// join's exact volume and returns the seam and pole tests it ran
/// ([`solidcore::kernel::WorkCounters::seam_tests`]).
///
/// The face the two plates share has an edge for each hole and no seam: each plate's side
/// of it is scanned for one once, not once for every face it meets, and the curves on it
/// ask nothing more. The count left grows as the holes, 47 for each hole and 40 more: 228
/// and 792 for 4 and 16 holes (a hole's wall closes on itself across a seam, so the curves
/// on it still ask about its edges and vertices). Asking, for every curve on the shared
/// face, about every edge and vertex of it ran 14 N^2 + 163 N + 416 tests for N holes:
/// 1292 and 6608, and 24428 for 36 (#18705).
fn stacked_plate_seam_tests(n: u32) -> u64 {
    let mut k = Kernel {
        fixes: UpstreamFixes::all(),
        ..Kernel::default()
    };
    let below = plate_prism(&mut k, n, 0.0, 12.0);
    let above = plate_prism(&mut k, n, 12.0, 24.0);
    let t0 = Instant::now();
    k.work_counters.reset();
    let mut joined = boolean_union(&mut k, below, above).unwrap();
    let seconds = t0.elapsed().as_secs_f64();
    let tests = k.work_counters.seam_tests();
    let pairs = k.work_counters.face_pairs();
    let w = PITCH * f64::from(n + 1);
    let holes = n * n;
    let volume = brep_volume_default(&k, &mut joined, 1e-9).unwrap();
    let exact = 24.0 * (w * w - f64::from(holes) * PI * RADIUS * RADIUS);
    println!(
        "plate with {holes} holes joined onto its top: {seconds:.3} s, {pairs} face pairs, \
         {tests} seam and pole tests, faces {}, edges {}",
        joined.face_list().len(),
        joined.edge_list().len()
    );
    assert!(
        (volume - exact).abs() <= 1e-7 * exact,
        "volume {volume}, exact {exact}"
    );
    tests
}

#[test]
fn a_plate_joined_onto_its_top_tests_each_face_for_a_seam_once() {
    let counts = [2, 4].map(stacked_plate_seam_tests);
    assert_eq!(counts, [47 * 4 + 40, 47 * 16 + 40]);
}
