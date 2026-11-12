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
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use std::f64::consts::PI;
use std::time::Instant;

use solidcore::boolean::api_brep::{boolean_2d, Boolean2d, Boolean2dOutcome};
use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::mass::integrator::properties::brep_area;
use solidcore::math::{Point3, Vec3};
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_trim_ops::create_planar_faces;
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
    k.memoize_curve_caches = true; // the count is the shortcut's, whatever `curve-memo-off` says
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
    k.memoize_curve_caches = true; // the count is the shortcut's, whatever `curve-memo-off` says
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
