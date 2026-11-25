//! A planar face rebuilt as a spline sheet whose whole box is the face
//! (`topology::brep::sheet::face_spline_sheet`): the Coons patch of its three or four edges,
//! lying on the plane, its sides the edges. A face with a hole or five edges is refused; a face
//! that is not a plane has none.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::curve::curve::Curve;
use solidcore::geom::vector2d::Point2;
use solidcore::math::Point3;
use solidcore::primitive::api_curves::{create_arc, create_line_segment};
use solidcore::primitive::api_primitives::create_cylinder;
use solidcore::primitive::api_trim_ops::create_planar_faces;
use solidcore::surface::surface::SurfaceKind;
use solidcore::topology::brep::sheet::face_spline_sheet;
use solidcore::topology::{Brep, FaceId};
use solidcore::{Kernel, SolidError};

fn line(a: [f64; 2], b: [f64; 2]) -> Curve {
    Curve::from(
        create_line_segment(Point3::new(a[0], a[1], 0.0), Point3::new(b[0], b[1], 0.0))
            .expect("line"),
    )
}

/// The polygon through `pts` in the plane z = 0, closed.
fn polygon(pts: &[[f64; 2]]) -> Vec<Curve> {
    pts.iter()
        .zip(pts.iter().cycle().skip(1))
        .map(|(a, b)| line(*a, *b))
        .collect()
}

fn planar(k: &mut Kernel, curves: &[Curve]) -> (Brep, FaceId) {
    let b = create_planar_faces(k, curves).expect("planar face");
    let f = *b.face_list().first().expect("a face");
    (b, f)
}

/// The sheet's one face is a spline surface lying in z = 0 (a 9 by 9 grid of its box), and its
/// vertices are at `corners`, every corner one of them.
fn on_the_plane(k: &Kernel, sheet: &Brep, corners: &[[f64; 2]]) {
    let [f] = sheet.face_list() else {
        panic!("one face: {:?}", sheet.face_list());
    };
    let s = sheet.face(*f).unwrap().surface.as_ref().unwrap();
    assert!(
        matches!(s.kind(), SurfaceKind::BSpline(_)),
        "{:?}",
        s.kind()
    );
    let bx = s.natural_uv_domain_with_nurb(k).unwrap();
    for i in 0..=8 {
        for j in 0..=8 {
            let uv: Point2 = bx.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0);
            let p = s.evaluate_point(k, uv).unwrap();
            assert!(p.z.abs() <= 1e-12, "{p:?} off the plane");
        }
    }
    let seen: Vec<[f64; 2]> = sheet
        .vertex_list()
        .iter()
        .map(|&v| {
            let p = sheet.vertex(v).unwrap().point;
            [p.x, p.y]
        })
        .collect();
    // (a triangle's patch has its fourth side collapsed at a corner: the corners are the
    // vertices, without counting)
    let near =
        |p: &[f64; 2], q: &[f64; 2]| (p[0] - q[0]).abs() <= 1e-9 && (p[1] - q[1]).abs() <= 1e-9;
    for c in corners {
        assert!(
            seen.iter().any(|q| near(q, c)),
            "no vertex at {c:?} in {seen:?}"
        );
    }
    for q in &seen {
        assert!(
            corners.iter().any(|c| near(q, c)),
            "a vertex {q:?} off the corners"
        );
    }
}

#[test]
fn a_trapezoid_is_the_coons_patch_of_its_edges() {
    let mut k = Kernel::default();
    let corners = [[0.0, 0.0], [10.0, 0.0], [7.0, 6.0], [2.0, 6.0]];
    let (b, f) = planar(&mut k, &polygon(&corners));
    let sheet = face_spline_sheet(&mut k, &b, f).unwrap().expect("a plane");
    on_the_plane(&k, &sheet, &corners);
}

#[test]
fn a_triangle_and_a_face_with_an_arc_edge_convert() {
    let mut k = Kernel::default();
    let tri = [[0.0, 0.0], [10.0, 0.0], [3.0, 7.0]];
    let (b, f) = planar(&mut k, &polygon(&tri));
    let sheet = face_spline_sheet(&mut k, &b, f).unwrap().expect("a plane");
    on_the_plane(&k, &sheet, &tri);

    let arc = Curve::from(create_arc(Point3::new(5.0, 5.0, 0.0), 5.0, 0.0, 180.0).expect("arc"));
    let (b, f) = planar(
        &mut k,
        &[
            line([0.0, 0.0], [10.0, 0.0]),
            line([10.0, 0.0], [10.0, 5.0]),
            arc,
            line([0.0, 5.0], [0.0, 0.0]),
        ],
    );
    let sheet = face_spline_sheet(&mut k, &b, f).unwrap().expect("a plane");
    on_the_plane(
        &k,
        &sheet,
        &[[0.0, 0.0], [10.0, 0.0], [10.0, 5.0], [0.0, 5.0]],
    );
}

#[test]
fn a_face_with_a_hole_or_five_edges_is_refused() {
    let mut k = Kernel::default();
    let pentagon = [
        [0.0, 0.0],
        [10.0, 0.0],
        [12.0, 6.0],
        [5.0, 10.0],
        [-2.0, 6.0],
    ];
    let (b, f) = planar(&mut k, &polygon(&pentagon));
    assert!(matches!(
        face_spline_sheet(&mut k, &b, f),
        Err(SolidError::Unsupported { .. })
    ));
    let mut holed = polygon(&[[0.0, 0.0], [10.0, 0.0], [10.0, 10.0], [0.0, 10.0]]);
    holed.extend(polygon(&[[3.0, 3.0], [3.0, 6.0], [6.0, 6.0], [6.0, 3.0]]));
    let (b, f) = planar(&mut k, &holed);
    assert!(matches!(
        face_spline_sheet(&mut k, &b, f),
        Err(SolidError::Unsupported { .. })
    ));
}

#[test]
fn a_face_that_is_not_a_plane_has_none() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    let side = b
        .face_list()
        .iter()
        .copied()
        .find(|&f| {
            !matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Plane(_)
            )
        })
        .expect("the side");
    assert!(face_spline_sheet(&mut k, &b, side).unwrap().is_none());
}
