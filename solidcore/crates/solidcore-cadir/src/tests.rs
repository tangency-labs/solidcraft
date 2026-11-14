//! Models read into records: cadmpeg's unit cube as it is, and changed the ways a producer may
//! write the same solid (an edge's vertices the other way round, a face on a plane facing the
//! other way), and broken the ways hostile input may be. The cube is 10 mm across: 1000 mm³.

use super::*;
use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

fn cube() -> CadIr {
    cadmpeg_ir::examples::unit_cube().unwrap()
}

fn one(ir: &CadIr) -> Solid {
    let bodies = read_bodies(ir, None);
    assert!(bodies.failures.is_empty(), "{:?}", bodies.failures);
    assert_eq!(bodies.read.len(), 1);
    bodies.read.into_iter().next().unwrap().solid
}

/// The record's volume: positive when its faces face out of the material.
fn volume(s: &Solid) -> f64 {
    let (body, _) = solidcore_ops::Body::from_record(&s.brep, false).unwrap();
    body.volume(1e-9).unwrap()
}

#[test]
fn the_example_cube_is_a_closed_solid_record_of_its_volume() {
    let s = one(&cube());
    assert_eq!(
        (
            s.brep.faces.len(),
            s.brep.edges.len(),
            s.brep.vertices.len()
        ),
        (6, 12, 8)
    );
    assert!(!s.sheet);
    assert!(s.turned.is_empty(), "{:?}", s.turned);
    for e in &s.brep.edges {
        assert_eq!(e.uses.len(), 2);
    }
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
    // Each face's origin is its index in the model's faces.
    let mut origins = s.face_origin.clone();
    origins.sort_unstable();
    assert_eq!(origins, vec![0, 1, 2, 3, 4, 5]);
}

#[test]
fn an_edge_whose_vertices_are_written_the_other_way_round_is_turned_and_noted() {
    let mut ir = cube();
    let edge = ir.model.edges.first_mut().unwrap();
    std::mem::swap(&mut edge.start, &mut edge.end);
    let s = one(&ir);
    assert!(
        s.notes
            .iter()
            .any(|n| n.contains("1 edges whose vertices lie at the other ends")),
        "{:?}",
        s.notes
    );
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_face_on_a_plane_facing_the_other_way_keeps_its_side() {
    let mut ir = cube();
    let face = ir.model.faces.first().unwrap().clone();
    let surface = ir
        .model
        .surfaces
        .iter_mut()
        .find(|s| s.id == face.surface)
        .unwrap();
    let SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane)) = &surface.geometry else {
        panic!("{:?}", surface.geometry)
    };
    let o = plane.origin().get();
    let (n, u) = (
        plane.frame().axis().as_raw(),
        plane.frame().reference().as_raw(),
    );
    let flipped = cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
        Point3::new(o.x, o.y, o.z),
        Vector3::new(-n.x, -n.y, -n.z),
        Vector3::new(u.x, u.y, u.z),
    )
    .unwrap();
    surface.geometry = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(flipped));
    let f = ir.model.faces.first_mut().unwrap();
    f.sense = if f.sense == Sense::Forward {
        Sense::Reversed
    } else {
        Sense::Forward
    };
    let s = one(&ir);
    assert!(s.turned.is_empty(), "{:?}", s.turned);
    assert!((volume(&s) - 1000.0).abs() < 1e-6, "{}", volume(&s));
}

#[test]
fn a_missing_coedge_refuses_its_region_without_a_panic() {
    let mut ir = cube();
    ir.model.coedges.remove(0);
    let bodies = read_bodies(&ir, None);
    assert!(bodies.read.is_empty());
    assert_eq!(bodies.failures.len(), 1);
    assert!(
        bodies.failures[0].message.contains("missing coedge"),
        "{:?}",
        bodies.failures
    );
}

#[test]
fn a_vertex_far_from_its_edges_refuses_its_region() {
    let mut ir = cube();
    let v = ir.model.vertices.first().unwrap().point.clone();
    let p = ir.model.points.iter_mut().find(|p| p.id == v).unwrap();
    // The point moved 5 mm along x, through its wire form.
    let mut wire = serde_json::to_value(&*p).unwrap();
    let x = wire["position"]["x"].as_f64().unwrap();
    wire["position"]["x"] = serde_json::json!(x + 5.0);
    *p = serde_json::from_value(wire).unwrap();
    let bodies = read_bodies(&ir, None);
    assert!(bodies.read.is_empty());
    assert!(
        bodies.failures[0].message.contains("off its curve"),
        "{:?}",
        bodies.failures
    );
}

/// The side a record surface keeps against CADIR's: a sphere or torus whose (tube) radius is
/// negative has CADIR's normal pointing inward, so the record (with the radius's magnitude,
/// whose natural normal points out) is reversed against it; with a positive radius it is not.
/// Both signs are checked, so a reversal applied to both could not pass.
#[test]
fn a_negative_radius_turns_the_surface_side_and_a_positive_one_does_not() {
    use cadmpeg_ir::geometry::analytic::{SphereSurface, TorusSurface};
    use solidcore_record::brep::Surface as BSurface;
    let (c, z, x) = (
        Point3::new(1.0, 2.0, 3.0),
        Vector3::new(0.0, 0.0, 1.0),
        Vector3::new(1.0, 0.0, 0.0),
    );
    for r in [5.0, -5.0] {
        let g = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(
            SphereSurface::try_new(c, z, x, r).unwrap(),
        ));
        let (s, reversed) = geometry::surface(&g).unwrap();
        assert!(
            matches!(s, BSurface::Sphere { radius, .. } if radius == 5.0),
            "{s:?}"
        );
        assert_eq!(reversed, r < 0.0, "sphere radius {r}");
        let g = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(
            TorusSurface::try_new(c, z, x, 20.0, r).unwrap(),
        ));
        let (s, reversed) = geometry::surface(&g).unwrap();
        assert!(
            matches!(s, BSurface::Torus { minor_radius, major_radius, .. } if minor_radius == 5.0 && major_radius == 20.0),
            "{s:?}"
        );
        assert_eq!(reversed, r < 0.0, "torus tube radius {r}");
    }
}

/// A NURBS surface CADIR marks `normal_reversed` is reversed against its record, whose natural
/// normal is `Pu × Pv`; one not marked is not.
#[test]
fn a_nurbs_surface_marked_normal_reversed_turns_the_side() {
    use cadmpeg_ir::geometry::nurbs::{NurbsPoleGrid, NurbsSurface, NurbsSurfaceAxis};
    for flag in [false, true] {
        let axis = || NurbsSurfaceAxis::new(1, vec![0.0, 0.0, 1.0, 1.0], false);
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let poles = NurbsPoleGrid::Polynomial {
            rows: vec![
                vec![p(0.0, 0.0), p(0.0, 1.0)],
                vec![p(1.0, 0.0), p(1.0, 1.0)],
            ],
        };
        let g = SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(
            NurbsSurface::new(axis(), axis(), poles, flag).unwrap(),
        ));
        let (_, reversed) = geometry::surface(&g).unwrap();
        assert_eq!(reversed, flag);
    }
}

/// A polyline whose parameters decrease along its samples is the same map from parameter to
/// point: the point at each parameter is the sample given with it.
#[test]
fn a_polyline_with_decreasing_parameters_keeps_its_parameters() {
    use cadmpeg_ir::geometry::sampled::{PolylineCurve, PolylineSamples, PolylineVertex};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    let at = |t: f64, x: f64| PolylineVertex {
        parameter: t,
        point: Point3::new(x, 0.0, 0.0),
    };
    let samples = PolylineSamples::Parameterized {
        vertices: vec![at(2.0, 0.0), at(1.0, 4.0), at(0.0, 5.0)]
            .try_into()
            .unwrap(),
    };
    let g = CurveGeometry::Solved(SolvedCurveGeometry::Polyline(
        PolylineCurve::new(samples, 0.0).unwrap(),
    ));
    let Some(solidcore_record::brep::Curve::Spline { knots, points }) =
        geometry::curve(&g).unwrap()
    else {
        panic!("not a spline")
    };
    assert_eq!(knots, vec![0.0, 0.0, 1.0, 2.0, 2.0]);
    assert_eq!(
        points,
        vec![[5.0, 0.0, 0.0], [4.0, 0.0, 0.0], [0.0, 0.0, 0.0]]
    );
}

/// A placement that scales unevenly, by however little more than rounding, is refused; a
/// rotation is taken.
#[test]
fn a_placement_scaling_unevenly_is_refused() {
    let uneven = [
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0 + 5e-10, 0.0],
    ];
    assert!(geometry::Similarity::of(uneven).is_err());
    let (c, s) = (0.6, 0.8);
    let turned = [[c, -s, 0.0, 3.0], [s, c, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]];
    assert!(geometry::Similarity::of(turned).is_ok());
}
