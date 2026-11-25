//! `bspline_surface_part` is public (#1767): an exporter gets a surface's B-spline part
//! from solidcore itself. A plane's part (its NURB made when missing) spans the plane's
//! corners; a B-spline surface's part is the surface itself, borrowed.

#![allow(clippy::unwrap_used, clippy::indexing_slicing)] // test helpers

use std::borrow::Cow;

use solidcore::math::Point3;
use solidcore::primitive::api_primitives::create_plane_surface;
use solidcore::surface::surface::Surface;
use solidcore::topology::brep_data_write::bspline_surface_part;

#[test]
fn a_plane_s_part_spans_its_corners() {
    let plane = Surface::from(create_plane_surface(Point3::new(1.0, 2.0, 3.0), 4.0, 2.0).unwrap());
    let part = bspline_surface_part(&plane).unwrap().unwrap();
    let (nu, nv, points) = part.control_points().unwrap();
    assert_eq!((nu, nv, points.len()), (2, 2, 4));
    let mut corners: Vec<(f64, f64, f64)> = points.iter().map(|p| (p.x, p.y, p.z)).collect();
    corners.sort_by(|a, b| a.partial_cmp(b).unwrap());
    assert_eq!(
        corners,
        vec![
            (1.0, 2.0, 3.0),
            (1.0, 4.0, 3.0),
            (5.0, 2.0, 3.0),
            (5.0, 4.0, 3.0)
        ]
    );
}

#[test]
fn a_bspline_surface_s_part_is_the_surface_itself() {
    let plane = Surface::from(create_plane_surface(Point3::new(0.0, 0.0, 0.0), 1.0, 1.0).unwrap());
    let nurb = bspline_surface_part(&plane).unwrap().unwrap().into_owned();
    let surface = Surface::from(nurb);
    let part = bspline_surface_part(&surface).unwrap().unwrap();
    assert!(matches!(part, Cow::Borrowed(_)));
}
