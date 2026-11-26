//! `intersect_faces_read_only`: where a face of one body meets a face of another, both bodies
//! only read. A plate standing on a plate (T joint), a plate lapped on a plate, a rod standing
//! on a plate and a rod swept from a B-spline circle standing on a plate, each two separate
//! bodies, and two faces of one box: the curves are the joint's, the same as `intersect_faces`
//! gives on the same bodies built again, and the bodies are the same after the call, in their
//! file bytes and in memory. On the rod, `intersect_faces` itself changes the bodies in memory,
//! which the test asserts, so the check that the read-only call does not can fail.
//!
//! Also: faces kept as their definition (a screw thread's walls) against a plane, two faces
//! that touch at one point with the points asked for (a ball resting on the plate: the contact
//! point among them), and a read-only call on bodies an earlier `intersect_faces` left its
//! caches on.

#![allow(clippy::unwrap_used, clippy::panic)] // test

use solidcore::curve::curve::Curve;
use solidcore::error::SolidError;
use solidcore::geom::attribute::{AttributeStore, ObjectKey};
use solidcore::intersect::api_intersectors::{
    intersect_faces, intersect_faces_read_only, FaceBodies, Intersections,
};
use solidcore::kernel::Kernel;
use solidcore::math::Vec3;
use solidcore::primitive::api_curves::create_circle;
use solidcore::primitive::api_curves::create_line_segment;
use solidcore::primitive::api_primitives::{create_box, create_cylinder, create_sphere};
use solidcore::query::api_queries::edge_closest_point;
use solidcore::sweep::api_brep::create_linear_sweep;
use solidcore::sweep::screw::screw_sweep;
use solidcore::topology::api_files::write_brep_to_file;
use solidcore::topology::brep_data_write::FromBrepEnv;
use solidcore::topology::edge;
use solidcore::topology::topology::NotifyRecords;
use solidcore::topology::{Brep, EdgeId, EdgeuseId, FaceId};
use solidcore::Point3;

fn k() -> Kernel {
    Kernel {
        fixes: solidcore::kernel::UpstreamFixes::all(),
        ..Kernel::default()
    }
}

fn block(k: &mut Kernel, at: [f64; 3], size: [f64; 3]) -> Brep {
    create_box(
        k,
        Point3::new(at[0], at[1], at[2]),
        size[0],
        size[1],
        size[2],
    )
    .unwrap()
}

/// The plate every rod stands on: 100 by 100, 10 thick.
fn plate(k: &mut Kernel) -> Brep {
    block(k, [0.0; 3], [100.0, 100.0, 10.0])
}

/// A cylinder of radius 15 standing at (50, 50) on the plate, 50 high.
fn rod(k: &mut Kernel) -> Brep {
    create_cylinder(k, Point3::new(50.0, 50.0, 10.0), 15.0, 50.0).unwrap()
}

/// The same rod swept up from a B-spline circle: its foot and top edges are B-spline curves.
fn spline_rod(k: &mut Kernel) -> Brep {
    let circle = Curve::from(create_circle(Point3::new(50.0, 50.0, 10.0), 15.0).unwrap());
    create_linear_sweep(k, &[Some(&circle)], Vec3::new(0.0, 0.0, 1.0), 50.0, true).unwrap()
}

/// The edge of `b` nearest `p`.
fn edge_at(k: &Kernel, b: &Brep, p: [f64; 3]) -> EdgeId {
    let q = Point3::new(p[0], p[1], p[2]);
    let mut best = (f64::INFINITY, None);
    for &e in b.edge_list() {
        if let Ok(c) = edge_closest_point(k, b, e, q) {
            if c.closest.distance < best.0 {
                best = (c.closest.distance, Some(e));
            }
        }
    }
    best.1.unwrap()
}

/// The face of `b` that the edges nearest `p` and `q` share.
fn face_at(k: &Kernel, b: &Brep, p: [f64; 3], q: [f64; 3]) -> FaceId {
    let a = edge::faces(b, edge_at(k, b, p), None).unwrap();
    let c = edge::faces(b, edge_at(k, b, q), None).unwrap();
    let shared: Vec<FaceId> = a.into_iter().filter(|f| c.contains(f)).collect();
    assert_eq!(shared.len(), 1, "{p:?} {q:?}");
    *shared.first().unwrap()
}

fn smb(k: &Kernel, b: &Brep) -> Vec<u8> {
    let store = AttributeStore::new();
    let records = NotifyRecords::new();
    let env = FromBrepEnv {
        attributes: &store,
        key_of: &|e| b.owner_identity(e),
        records: &records,
        curve_key: &|_: EdgeId| ObjectKey(0),
        uv_curve_key: &|_: EdgeuseId| ObjectKey(0),
        surface_key: &|_: FaceId| ObjectKey(0),
    };
    write_brep_to_file(k, Some(b), &env, true).unwrap()
}

/// Both bodies as file bytes and as they are in memory.
fn state(k: &Kernel, a: &Brep, b: &Brep) -> (Vec<u8>, Vec<u8>, String, String) {
    (smb(k, a), smb(k, b), format!("{a:?}"), format!("{b:?}"))
}

/// Where `fa` of `a` meets `fb` of `b`, read only: both bodies are asserted the same after.
fn read_only(k: &Kernel, a: &Brep, fa: FaceId, b: &Brep, fb: FaceId) -> Intersections {
    let before = state(k, a, b);
    let got = intersect_faces_read_only(k, a, fa, b, fb, false).unwrap();
    assert!(state(k, a, b) == before, "a body changed");
    got
}

/// `intersect_faces` itself on `a` and `b` (the same bodies, built again): `got` must be its
/// curves, as `Debug` prints them. Whether that call, in place, changed either body.
fn in_place_agrees(
    k: &Kernel,
    got: &Intersections,
    a: &mut Brep,
    fa: FaceId,
    b: &mut Brep,
    fb: FaceId,
) -> bool {
    let before = state(k, a, b);
    let there = intersect_faces(k, FaceBodies::Two(a, b), fa, fb, false).unwrap();
    assert_eq!(format!("{:?}", got.curves), format!("{:?}", there.curves));
    state(k, a, b) != before
}

fn near(p: Point3, q: [f64; 3]) -> bool {
    p.distance(Point3::new(q[0], q[1], q[2])) <= 1e-6
}

/// The one curve of `got` runs from `p` to `q`, either way.
fn is_line(k: &Kernel, got: &Intersections, p: [f64; 3], q: [f64; 3]) {
    assert_eq!(got.curves.len(), 1, "{got:?}");
    let ends = got.curves.first().unwrap().ends(k).unwrap();
    let (s, e) = (ends.start, ends.end);
    assert!(
        (near(s, p) && near(e, q)) || (near(s, q) && near(e, p)),
        "{s:?} {e:?}"
    );
}

/// The curves of `got` run round a rod's foot, radius 15 about (50, 50) at z = 10, at their
/// ends and inside; together they are the whole circle, not part of it.
fn is_foot(k: &Kernel, got: &Intersections) {
    assert!(!got.curves.is_empty());
    let mut length = 0.0;
    for c in &got.curves {
        let span = c.natural_interval();
        length += c.length(k, &span, 1e-9).unwrap();
        for i in 0..=8 {
            let t = span.min() + (span.max() - span.min()) * f64::from(i) / 8.0;
            let p = c.evaluate_point(k, t).unwrap();
            let r = ((p.x - 50.0).powi(2) + (p.y - 50.0).powi(2)).sqrt();
            assert!(
                (r - 15.0).abs() <= 1e-6 && (p.z - 10.0).abs() <= 1e-9,
                "{p:?}"
            );
        }
    }
    let round = 2.0 * std::f64::consts::PI * 15.0;
    assert!(
        (length - round).abs() <= 1e-6 * round,
        "{length} against {round}"
    );
}

#[test]
fn a_plate_standing_on_a_plate_meets_it_along_its_foot() {
    let mut k = k();
    let make = |k: &mut Kernel| {
        (
            block(k, [45.0, 0.0, 10.0], [10.0, 100.0, 50.0]),
            block(k, [0.0; 3], [100.0, 100.0, 10.0]),
        )
    };
    let (web, base) = make(&mut k);
    let side = face_at(&k, &web, [45.0, 50.0, 10.0], [45.0, 0.0, 35.0]);
    let top = face_at(&k, &base, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let got = read_only(&k, &web, side, &base, top);
    is_line(&k, &got, [45.0, 0.0, 10.0], [45.0, 100.0, 10.0]);
    let (mut web, mut base) = make(&mut k);
    in_place_agrees(&k, &got, &mut web, side, &mut base, top);
}

#[test]
fn a_plate_lapped_on_a_plate_meets_it_along_its_edge() {
    let mut k = k();
    let make = |k: &mut Kernel| {
        (
            block(k, [0.0, 40.0, 5.0], [100.0, 50.0, 5.0]),
            block(k, [0.0; 3], [100.0, 50.0, 5.0]),
        )
    };
    let (upper, lower) = make(&mut k);
    let front = face_at(&k, &upper, [50.0, 40.0, 5.0], [0.0, 40.0, 7.5]);
    let top = face_at(&k, &lower, [50.0, 0.0, 5.0], [0.0, 25.0, 5.0]);
    let got = read_only(&k, &upper, front, &lower, top);
    is_line(&k, &got, [0.0, 40.0, 5.0], [100.0, 40.0, 5.0]);
    let (mut upper, mut lower) = make(&mut k);
    in_place_agrees(&k, &got, &mut upper, front, &mut lower, top);
}

#[test]
fn a_rod_standing_on_a_plate_meets_it_round_its_foot_and_leaves_both() {
    let mut k = k();
    let (r, p) = (rod(&mut k), plate(&mut k));
    let side = face_at(&k, &r, [65.0, 50.0, 10.0], [65.0, 50.0, 60.0]);
    let top = face_at(&k, &p, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let got = read_only(&k, &r, side, &p, top);
    is_foot(&k, &got);
    let (mut r, mut p) = (rod(&mut k), plate(&mut k));
    assert!(
        in_place_agrees(&k, &got, &mut r, side, &mut p, top),
        "intersect_faces itself no longer changes the bodies: the read-only check proves nothing here"
    );
}

#[test]
fn a_rod_with_b_spline_edges_meets_the_plate_round_its_foot_and_leaves_both() {
    let mut k = k();
    let (r, p) = (spline_rod(&mut k), plate(&mut k));
    let side = face_at(&k, &r, [65.0, 50.0, 10.0], [65.0, 50.0, 60.0]);
    let top = face_at(&k, &p, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let got = read_only(&k, &r, side, &p, top);
    is_foot(&k, &got);
    let (mut r, mut p) = (spline_rod(&mut k), plate(&mut k));
    in_place_agrees(&k, &got, &mut r, side, &mut p, top);
}

#[test]
fn one_body_passed_twice_is_one_body() {
    let mut k = k();
    let b = block(&mut k, [0.0; 3], [10.0, 10.0, 10.0]);
    let top = face_at(&k, &b, [5.0, 0.0, 10.0], [0.0, 5.0, 10.0]);
    let front = face_at(&k, &b, [5.0, 0.0, 10.0], [0.0, 0.0, 5.0]);
    let before = format!("{b:?}");
    let got = intersect_faces_read_only(&k, &b, top, &b, front, false).unwrap();
    assert_eq!(format!("{b:?}"), before, "the box changed");
    let mut again = block(&mut k, [0.0; 3], [10.0, 10.0, 10.0]);
    let there = intersect_faces(&k, FaceBodies::One(&mut again), top, front, false).unwrap();
    assert_eq!(format!("{:?}", got.curves), format!("{:?}", there.curves));
}

#[test]
fn a_face_not_of_its_body_is_refused() {
    let mut k = k();
    let (r, p) = (rod(&mut k), plate(&mut k));
    let top = face_at(&k, &p, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    // the box has six faces, the rod three: some face id of the box is none of the rod's
    let stranger = *p
        .face_list()
        .iter()
        .find(|f| !r.face_list().contains(f))
        .unwrap();
    let got = intersect_faces_read_only(&k, &r, stranger, &p, top, false);
    assert!(matches!(got, Err(SolidError::NotFound { .. })), "{got:?}");
}

/// A square thread, 0.9 by 0.9 at radius 4.1 to 5, two turns of pitch 1.5 about the z axis:
/// its walls are nets kept with their sweep definition.
fn thread(k: &mut Kernel) -> Brep {
    let points = [
        Vec3::new(4.1, 0.0, 0.0),
        Vec3::new(5.0, 0.0, 0.0),
        Vec3::new(5.0, 0.0, 0.9),
        Vec3::new(4.1, 0.0, 0.9),
    ];
    let profile: Vec<Curve> = points
        .iter()
        .zip(points.iter().cycle().skip(1))
        .map(|(&a, &b)| Curve::from(create_line_segment(a, b).unwrap()))
        .collect();
    screw_sweep(k, &profile, Vec3::ZERO, Vec3::Z, 1.5, 2.0, 1.5e-4).unwrap()
}

/// The faces of `b` kept with a definition beside their net.
fn defined_faces(b: &Brep) -> Vec<FaceId> {
    b.face_list()
        .iter()
        .copied()
        .filter(|&f| {
            b.face(f)
                .unwrap()
                .surface
                .as_ref()
                .is_some_and(|s| s.procedural_definition().is_some())
        })
        .collect()
}

#[test]
fn faces_kept_as_their_definition_are_read_as_intersect_faces_reads_them() {
    let mut k = k();
    // a block whose bottom face, at z = 1.2, cuts the thread's walls
    let make = |k: &mut Kernel| (thread(k), block(k, [-10.0, -10.0, 1.2], [20.0, 20.0, 10.0]));
    let (t, cap) = make(&mut k);
    let bottom = face_at(&k, &cap, [0.0, -10.0, 1.2], [-10.0, 0.0, 1.2]);
    let walls = defined_faces(&t);
    assert!(!walls.is_empty(), "the thread keeps no definition");
    let mut met = 0;
    for &w in &walls {
        let got = read_only(&k, &t, w, &cap, bottom);
        met += got.curves.len();
        let (mut t2, mut cap2) = make(&mut k);
        in_place_agrees(&k, &got, &mut t2, w, &mut cap2, bottom);
    }
    assert!(
        met > 0,
        "no wall meets the plane: the case reads no definition"
    );
}

#[test]
fn a_ball_resting_on_the_plate_with_points_asked_for() {
    let mut k = k();
    let make = |k: &mut Kernel| {
        (
            create_sphere(k, Point3::new(50.0, 50.0, 20.0), 10.0).unwrap(),
            plate(k),
        )
    };
    let (ball, p) = make(&mut k);
    let top = face_at(&k, &p, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    let mut points = Vec::new();
    for &f in ball.face_list() {
        let before = state(&k, &ball, &p);
        let got = intersect_faces_read_only(&k, &ball, f, &p, top, true).unwrap();
        assert!(state(&k, &ball, &p) == before, "a body changed");
        let (mut ball2, mut p2) = make(&mut k);
        let there =
            intersect_faces(&k, FaceBodies::Two(&mut ball2, &mut p2), f, top, true).unwrap();
        assert_eq!(format!("{:?}", got.curves), format!("{:?}", there.curves));
        assert_eq!(format!("{:?}", got.points), format!("{:?}", there.points));
        points.extend(got.points.unwrap_or_default());
    }
    // the ball touches the plate at the foot of its centre
    let foot = Point3::new(50.0, 50.0, 10.0);
    assert!(
        points.iter().any(|q| q.distance(foot) <= 1e-6),
        "no point at the contact: {points:?}"
    );
}

#[test]
fn bodies_an_earlier_intersection_left_its_caches_on_are_read_and_left_so() {
    let mut k = k();
    let (mut r, mut p) = (rod(&mut k), plate(&mut k));
    let side = face_at(&k, &r, [65.0, 50.0, 10.0], [65.0, 50.0, 60.0]);
    let top = face_at(&k, &p, [50.0, 0.0, 10.0], [0.0, 50.0, 10.0]);
    // in place first: the bodies now hold the caches and trims that call made
    intersect_faces(&k, FaceBodies::Two(&mut r, &mut p), side, top, false).unwrap();
    let got = read_only(&k, &r, side, &p, top);
    is_foot(&k, &got);
    in_place_agrees(&k, &got, &mut r, side, &mut p, top);
}
