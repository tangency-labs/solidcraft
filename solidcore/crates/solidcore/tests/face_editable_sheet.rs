//! A face made a one-face sheet with a control net to edit (`topology::brep::sheet::editable_sheet`):
//! a cylinder's, a cone's, a sphere's or a torus's face as its surface's exact NURBS form (the
//! rational form the kernel keeps for it), when the face is its surface's whole box; a plane face
//! as the Coons spline of its edges (`face_spline_sheet`); a spline face as it is (none). A closed
//! surface comes back with one seam edge used twice, never two edges with a gap between them.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

use solidcore::geom::extent2d::Extent2d;
use solidcore::geom::tol::USE_DEFAULT;
use solidcore::geom::vector2d::Point2;
use solidcore::math::Point3;
use solidcore::nurbs::b_spline_surface::{BSplineSurface, SurfParamType};
use solidcore::primitive::api_primitives::{
    create_cone, create_cylinder, create_cylinder_surface, create_sphere, create_torus,
};
use solidcore::surface::surface::{Surface, SurfaceKind};
use solidcore::topology::brep::p3::new_brep;
use solidcore::topology::brep::sheet::{editable_sheet, face_on, sheet_on};
use solidcore::topology::brep::validation::{check_brep, CheckLevel};
use solidcore::topology::{Brep, FaceId};
use solidcore::{Kernel, SolidError};

/// The face of `b` that is not on a plane (a primitive's side).
fn curved_face(b: &Brep) -> FaceId {
    b.face_list()
        .iter()
        .copied()
        .find(|&f| {
            !matches!(
                b.face(f).unwrap().surface.as_ref().unwrap().kind(),
                SurfaceKind::Plane(_)
            )
        })
        .expect("a curved face")
}

/// The sheet's one face and its surface, a B-spline.
fn spline_of(sheet: &Brep) -> (&Surface, &BSplineSurface) {
    let [f] = sheet.face_list() else {
        panic!("one face: {:?}", sheet.face_list());
    };
    let s = sheet.face(*f).unwrap().surface.as_ref().unwrap();
    let SurfaceKind::BSpline(bs) = s.kind() else {
        panic!("a B-spline surface: {:?}", s.kind());
    };
    (s, bs)
}

/// Every point of a 9 by 9 grid of the sheet's surface over its box meets `on` (the analytic
/// surface's equation, its residual within 1e-9).
fn on_the_surface(k: &Kernel, sheet: &Brep, on: impl Fn(Point3) -> f64) {
    let (s, _) = spline_of(sheet);
    let bx = s.natural_uv_domain_with_nurb(k).unwrap();
    for i in 0..=8 {
        for j in 0..=8 {
            let uv: Point2 = bx.evaluate(f64::from(i) / 8.0, f64::from(j) / 8.0);
            let p = s.evaluate_point(k, uv).unwrap();
            assert!(on(p).abs() <= 1e-9, "{p:?} off the surface by {}", on(p));
        }
    }
}

/// The edges of the sheet used twice by its face (four edge uses, two in each face use): its
/// seams.
fn seams(sheet: &Brep) -> usize {
    sheet
        .edge_list()
        .iter()
        .filter(|&&e| sheet.edge(e).unwrap().edgeuses.len() == 4)
        .count()
}

/// The sheet is valid, rational and of `net` control points.
fn exact_form(k: &mut Kernel, sheet: &Brep, net: (usize, usize)) {
    let (_, bs) = spline_of(sheet);
    assert!(bs.is_rational(), "the exact form of a circle is rational");
    assert_eq!(
        (
            bs.number_control_points(SurfParamType::U),
            bs.number_control_points(SurfParamType::V)
        ),
        net
    );
    let mut checked = sheet_copy(k, sheet);
    let lines = check_brep(k, &mut checked, CheckLevel::Level2)
        .unwrap()
        .lines();
    assert!(lines.is_empty(), "{lines:?}");
}

fn sheet_copy(k: &mut Kernel, b: &Brep) -> Brep {
    solidcore::topology::brep::p3::brep_copy(k, b).unwrap()
}

#[test]
fn a_cylinders_side_is_its_exact_form_with_one_seam_edge() {
    let mut k = Kernel::default();
    let b = create_cylinder(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    let side = curved_face(&b);
    let sheet = editable_sheet(&mut k, &b, side).unwrap().expect("a sheet");
    exact_form(&mut k, &sheet, (9, 2));
    on_the_surface(&k, &sheet, |p| p.x.hypot(p.y) - 5.0);
    // the seam and the two circles
    assert_eq!(seams(&sheet), 1);
    assert_eq!(sheet.edge_list().len(), 3);
}

#[test]
fn a_cones_side_is_its_exact_form_with_one_seam_edge() {
    let mut k = Kernel::default();
    let b = create_cone(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0, 0.0, 10.0).unwrap();
    let side = curved_face(&b);
    let sheet = editable_sheet(&mut k, &b, side).unwrap().expect("a sheet");
    exact_form(&mut k, &sheet, (9, 2));
    // r = 5 (1 - z / 10)
    on_the_surface(&k, &sheet, |p| p.x.hypot(p.y) - 5.0 * (1.0 - p.z / 10.0));
    assert_eq!(seams(&sheet), 1);
}

#[test]
fn a_sphere_is_its_exact_form_with_one_seam_edge() {
    let mut k = Kernel::default();
    let b = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let face = *b.face_list().first().unwrap();
    let sheet = editable_sheet(&mut k, &b, face).unwrap().expect("a sheet");
    exact_form(&mut k, &sheet, (9, 5));
    on_the_surface(&k, &sheet, |p| p.x.hypot(p.y).hypot(p.z) - 5.0);
    // the seam from pole to pole; the poles are points
    assert_eq!(seams(&sheet), 1);
    assert_eq!(sheet.edge_list().len(), 1);
}

#[test]
fn a_torus_is_its_exact_form_with_its_two_seam_edges() {
    let mut k = Kernel::default();
    let b = create_torus(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 3.0).unwrap();
    let face = *b.face_list().first().unwrap();
    let sheet = editable_sheet(&mut k, &b, face).unwrap().expect("a sheet");
    exact_form(&mut k, &sheet, (9, 9));
    on_the_surface(&k, &sheet, |p| (p.x.hypot(p.y) - 10.0).hypot(p.z) - 3.0);
    assert_eq!(seams(&sheet), 2);
    assert_eq!(sheet.edge_list().len(), 2);
}

#[test]
fn a_face_over_part_of_its_surface_is_refused() {
    let mut k = Kernel::default();
    let cylinder = create_cylinder_surface(&k, Point3::new(0.0, 0.0, 0.0), 5.0, 10.0).unwrap();
    let surface = Surface::from(cylinder);
    let whole = surface.natural_uv_domain_with_nurb(&k).unwrap();
    let half = Extent2d::from_bounds(
        whole.min().x,
        whole.min().y,
        whole.max().x,
        0.5 * (whole.min().y + whole.max().y),
    );
    let mut b = new_brep(&mut k, USE_DEFAULT, USE_DEFAULT).unwrap();
    let f = face_on(&mut k, &mut b, surface, &half).unwrap();
    b.refresh_cached_topology();
    assert!(matches!(
        editable_sheet(&mut k, &b, f),
        Err(SolidError::Unsupported { .. })
    ));
}

#[test]
fn a_spline_face_has_none_and_the_input_is_unchanged() {
    let mut k = Kernel::default();
    let patch = BSplineSurface::create_bilinear_surface(
        Point3::new(0.0, 0.0, 0.0),
        Point3::new(10.0, 0.0, 0.0),
        Point3::new(0.0, 10.0, 1.0),
        Point3::new(10.0, 10.0, 0.0),
    )
    .unwrap();
    let (b, f) = sheet_on(&mut k, Surface::from(patch), 1e-6).unwrap();
    assert!(editable_sheet(&mut k, &b, f).unwrap().is_none());

    let solid = create_sphere(&mut k, Point3::new(0.0, 0.0, 0.0), 5.0).unwrap();
    let (faces, edges) = (solid.face_list().to_vec(), solid.edge_list().to_vec());
    let face = *faces.first().unwrap();
    editable_sheet(&mut k, &solid, face).unwrap();
    assert_eq!(solid.face_list(), faces.as_slice());
    assert_eq!(solid.edge_list(), edges.as_slice());
    assert!(matches!(
        solid.face(face).unwrap().surface.as_ref().unwrap().kind(),
        SurfaceKind::Sphere(_)
    ));
}
