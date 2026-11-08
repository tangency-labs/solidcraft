//! tangency-labs/solidcore#1441: an offset sheet whose face lies on an `OffsetSurface`
//! can be exported as NURBS. The flat export (the SMLib fork's `FlatFromBrep`, ported in
//! SolidCraft) writes a face whose surface is no `SmBSplineSurface` kind through
//! `SmSurface::ApproximateSurface(approx tol, 5 subdivisions, copy B-splines)`; that is
//! ported (`Surface::approximate_surface`, #1533), so the case of the issue exports: a
//! quarter circle r 5 about +Z swept 10 along +Z, offset by 2, is a quarter cylinder r 7
//! whose face's offset surface approximates within the B-rep's approximation tolerance.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)] // test helpers

#[path = "support/offset_fixture.rs"]
mod offset_fixture;

use solidcore::curve::curve::Curve;
use solidcore::curve::ellipse::Ellipse;
use solidcore::geom::axis2_placement::Axis2Placement;
use solidcore::geom::extent1d::Extent1d;
use solidcore::geom::tol::{brep_approx_tol_3d, SmallTopoType};
use solidcore::geom::vector3d::Vec3;
use solidcore::surface::surface::SurfaceKind;
use solidcore::sweep::api_brep as sweeps;
use solidcore::topology::brep_data_write::bspline_surface_part;
use solidcore::Kernel;

#[test]
fn offset_sheet_face_exports_as_nurbs() {
    let mut k = Kernel::default();
    let frame = Axis2Placement::new(
        Vec3::new(0.0, 0.0, 0.0),
        Vec3::new(1.0, 0.0, 0.0),
        Vec3::new(0.0, 1.0, 0.0),
    );
    let quarter = Extent1d::from_min_max(0.0, 90.0);
    let arc = Curve::from(
        Ellipse::create_canonical_circle(&k, &frame, 5.0, Some(&quarter), None).unwrap(),
    );
    let mut sheet =
        sweeps::create_linear_sweep(&mut k, &[Some(&arc)], Vec3::new(0.0, 0.0, 1.0), 10.0, false)
            .unwrap();
    let moved = offset_fixture::offset_full(&mut k, &mut sheet, 2.0).unwrap();

    // one face, on an offset surface: no B-spline part, so the export approximates it
    assert_eq!(moved.face_list().len(), 1);
    let f = moved.face_list()[0];
    let face = moved.face(f).unwrap();
    let surface = face.surface.as_ref().unwrap();
    assert!(
        matches!(surface.kind(), SurfaceKind::OffsetSurface(_)),
        "the moved face is not on an offset surface"
    );
    assert!(bspline_surface_part(surface).unwrap().is_none());

    // as the export calls it: the B-rep's approximation tolerance, 5 subdivisions,
    // B-splines copied
    let tol = brep_approx_tol_3d(&k, &moved, SmallTopoType::NormalSize);
    let approx = surface.approximate_surface(&k, tol, 5, true).unwrap();
    let nurbs = approx.surface.expect("an approximating B-spline surface");
    assert!(bspline_surface_part(&nurbs).unwrap().is_some());
    assert!(approx.achieved_tol >= 0.0 && approx.achieved_tol.is_finite());

    // over the face's domain the NURBS stays within the achieved tolerance of the offset
    // surface (upstream accepts its last test row, so the achieved tolerance, not `tol`,
    // bounds it) and both lie on the r 7 quarter cylinder, 0 <= z <= 10
    let bound = approx.achieved_tol.max(tol) + tol;
    let domain = face.uv_domain;
    for i in 0..=10 {
        for j in 0..=10 {
            let uv = domain.evaluate(f64::from(i) / 10.0, f64::from(j) / 10.0);
            let a = surface.evaluate_point(&k, uv).unwrap();
            let b = nurbs.evaluate_point(&k, uv).unwrap();
            let gap = a.distance(b);
            assert!(gap <= bound, "({i}, {j}): gap {gap} > {bound}");
            let r = (b.x * b.x + b.y * b.y).sqrt();
            assert!((r - 7.0).abs() <= bound, "({i}, {j}): radius {r}");
            assert!(
                b.x >= -bound && b.y >= -bound,
                "({i}, {j}): {b:?} off the quarter"
            );
            assert!(
                (-bound..=10.0 + bound).contains(&b.z),
                "({i}, {j}): z {}",
                b.z
            );
        }
    }
}
