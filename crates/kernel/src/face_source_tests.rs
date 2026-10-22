use crate::{FaceSource, extrude, transform_matrix};
use solidcraft_geom::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};

fn assert_exact_planes(body: &crate::Body) {
    let mesh = body.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
    for (index, _) in body.face_sources().unwrap().iter().enumerate() {
        let Some(plane) = body.face_plane(index) else { continue };
        let mut count = 0;
        for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if *face as usize != index {
                continue;
            }
            for point in mesh.tri(triangle).unwrap() {
                assert!((point - plane.origin).dot(plane.normal()).abs() < 1e-7);
                count += 1;
            }
        }
        assert!(count > 0);
        let normal = body.faces(0.001).unwrap()[index].plane_normal.unwrap();
        assert!(normal.dot(plane.normal()) > 0.999999);
    }
}

#[test]
fn box_certificates_are_six_construction_roles_with_dimension_stable_axes() {
    for (a, b) in [
        (Vec3::new(2., 3., 4.), Vec3::new(12., 23., 34.)),
        (Vec3::new(12., 23., 34.), Vec3::new(2., 3., 4.)),
        (Vec3::new(2., -8., 4.), Vec3::new(22., 43., 54.)),
    ] {
        let body = crate::box_solid(a, b).unwrap().with_box_certificate(42).unwrap();
        let mut roles = std::collections::HashSet::new();
        for (index, source) in body.face_sources().unwrap().iter().enumerate() {
            let Some(FaceSource::Box { producer, axis, high, plane }) = source else { panic!("uncertified box face") };
            assert_eq!(*producer, Some(42));
            assert!(roles.insert((*axis, *high)));
            let unit = [Vec3::X, Vec3::Y, Vec3::Z][*axis as usize];
            let bound = if *high { a.max(b) } else { a.min(b) };
            assert_eq!(plane.origin, unit * bound.dot(unit));
            assert!(plane.normal().dist(unit * if *high { 1. } else { -1. }) < 1e-12);
            assert_eq!(body.face_plane(index), Some(*plane));
        }
        assert_eq!(roles.len(), 6);
        assert_exact_planes(&body);
    }
}

#[test]
fn cap_roles_follow_exact_offset_planes_and_canonical_profile_memberships() {
    let outer = Loop2::polygon(&[Vec2::new(-10., -10.), Vec2::new(10., -10.), Vec2::new(10., 10.), Vec2::new(-10., 10.)]);
    let h1 = Loop2::circle(Vec2::new(-4., 0.), 1.);
    let h2 = Loop2::circle(Vec2::new(4., 0.), 2.);
    let plane = Plane::new(Vec3::new(30., 40., 50.), Vec3::Y, Vec3::Z).unwrap();
    for (start, end) in [(-9., -2.), (7., -3.), (2., 11.)] {
        for holes in [vec![h1.clone(), h2.clone()], vec![h2.clone().reversed(), h1.clone().reversed()]] {
            let region = Region2 { outer: outer.clone().reversed(), holes };
            let raw = extrude(&plane, std::slice::from_ref(&region), start, end).unwrap().remove(0);
            let signatures: Vec<_> = raw
                .face_sources()
                .unwrap()
                .iter()
                .map(|source| match source {
                    Some(FaceSource::ExtrudeCap { .. }) => {
                        Some((vec!["l4".into(), "l1".into(), "l3".into(), "l2".into()], vec![vec!["c2".into()], vec!["c1".into()]]))
                    }
                    _ => None,
                })
                .collect();
            let body = raw.with_extrude_cap_certificates(42, 11, &signatures).unwrap();
            let mut caps = std::collections::HashSet::new();
            for source in body.face_sources().unwrap().iter().flatten() {
                if let FaceSource::CertifiedExtrudeCap { producer, sketch, high, outer, holes, plane: cap } = source {
                    assert_eq!((*producer, *sketch), (42, 11));
                    assert!(caps.insert(*high));
                    assert_eq!(outer, &["l1", "l2", "l3", "l4"]);
                    assert_eq!(holes, &[vec!["c1"], vec!["c2"]]);
                    assert!(cap.origin.dist(plane.origin + plane.normal() * if *high { start.max(end) } else { start.min(end) }) < 1e-10);
                    assert!(cap.normal().dot(plane.normal()) * if *high { 1. } else { -1. } > 0.999999);
                }
            }
            assert_eq!(caps.len(), 2);
            assert_exact_planes(&body);
            let expected = (400. - 5. * std::f64::consts::PI) * (end - start).abs();
            assert!((body.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume - expected).abs() < 0.03);
        }
    }
}

#[test]
fn construction_planes_follow_reflected_and_sheared_affine_images() {
    let body = crate::box_solid(Vec3::new(2., 3., 4.), Vec3::new(12., 23., 34.)).unwrap().with_box_certificate(42).unwrap();
    for m in [
        [[0., 2., 0., 0.], [-1., 0., 0., 0.], [0., 0., 3., 0.], [25., 30., 40., 1.]],
        [[-1., 0., 0., 0.], [0.4, 2., 0., 0.], [0., 0.3, 1., 0.], [8., 9., 10., 1.]],
    ] {
        let placed = transform_matrix(&body, m).unwrap();
        for (original, transformed) in body.face_sources().unwrap().iter().zip(placed.face_sources().unwrap()) {
            let (
                Some(FaceSource::Box { producer: a, axis: aa, high: ah, plane: ap }),
                Some(FaceSource::Box { producer: b, axis: ba, high: bh, plane: bp }),
            ) = (original, transformed)
            else {
                panic!("box identity changed")
            };
            assert_eq!((a, aa, ah), (b, ba, bh));
            let p = ap.origin;
            let expected = Vec3::new(
                m[0][0] * p.x + m[1][0] * p.y + m[2][0] * p.z + m[3][0],
                m[0][1] * p.x + m[1][1] * p.y + m[2][1] * p.z + m[3][1],
                m[0][2] * p.x + m[1][2] * p.y + m[2][2] * p.z + m[3][2],
            );
            assert!(bp.origin.dist(expected) < 1e-10);
        }
        assert_exact_planes(&placed);
    }
}

#[test]
fn material_changes_preserve_only_unchanged_planar_support_certificates() {
    let base = crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap().with_box_certificate(42).unwrap();
    let tool = crate::box_solid(Vec3::new(5., -1., -1.), Vec3::new(11., 11., 11.)).unwrap();
    let cut = crate::boolean(&base, &tool, crate::BoolOp::Cut).unwrap().unwrap();
    assert!((cut.mesh(&crate::MeshTolerance::chord(1e-4)).unwrap().measure().volume - 500.).abs() < 1e-8);
    // Unchanged uniquely certified supports retain their role; a newly cut
    // support must never inherit the removed opposite wall's source identity.
    let mut retained = 0;
    let mut new_wall = 0;
    for index in 0..cut.face_count() {
        let actual = cut.planar_face(index).unwrap();
        let source = cut.face_sources().and_then(|s| s.get(index)).and_then(Option::as_ref);
        if let Some(FaceSource::Box { producer: Some(42), axis, high, .. }) = source {
            assert!(!(*axis == 0 && *high));
            let canonical = cut.face_plane(index).unwrap();
            assert!(actual.normal().dot(canonical.normal()) > 1. - 1e-10);
            assert!(actual.height(canonical.origin).abs() < 1e-10);
            retained += 1;
        }
        if actual.normal().x > 0.9 {
            assert!((actual.origin.x - 5.).abs() < 1e-10);
            assert!(!matches!(source, Some(FaceSource::Box { producer: Some(_), .. })));
            new_wall += 1;
        }
    }
    assert_eq!((retained, new_wall), (5, 1));
}

#[test]
fn canonical_revolve_axis_is_per_profile_and_independent_of_input_axis_sign() {
    for x in [-4., 4.] {
        let region = Region2 {
            outer: Loop2::polygon(&[Vec2::new(x - 1., 0.), Vec2::new(x + 1., 0.), Vec2::new(x + 1., 2.), Vec2::new(x - 1., 2.)]),
            holes: vec![],
        };
        let axis = crate::canonical_revolve_axis(&Plane::XY, &region, Vec2::default(), Vec2::Y).unwrap();
        assert_eq!(axis, crate::canonical_revolve_axis(&Plane::XY, &region, Vec2::default(), -Vec2::Y).unwrap());
        assert!(axis.cross(Plane::XY.to_world(region.centroid())).dot(Vec3::Z) > 0.);
        assert_eq!(axis, Vec3::Y * if x < 0. { 1. } else { -1. });
    }
}

#[test]
fn linear_side_frame_stays_fixed_when_endpoints_and_depth_change() {
    for (lo, hi, start, end) in [(3., 23., 0., 7.), (-9., 43., -3., 19.)] {
        let region =
            Region2 { outer: Loop2::polygon(&[Vec2::new(2., lo), Vec2::new(12., lo), Vec2::new(12., hi), Vec2::new(2., hi)]), holes: vec![] };
        let body = extrude(&Plane::XY, &[region], start, end).unwrap().remove(0);
        let plane = body
            .face_sources()
            .unwrap()
            .iter()
            .enumerate()
            .find_map(|(index, source)| match source {
                Some(FaceSource::ExtrudeSide { segment: Seg2::Line { a, b }, .. }) if a.x == 2. && b.x == 2. => body.face_plane(index),
                _ => None,
            })
            .unwrap();
        assert_eq!(plane.origin, Vec3::X * 2.);
        assert_eq!(plane.x, -Vec3::Y);
        assert_eq!(plane.normal(), -Vec3::X);
    }
}

#[test]
fn reflected_hole_walls_and_low_caps_keep_exact_outward_planes() {
    let outer = Loop2::polygon(&[Vec2::new(-10., -10.), Vec2::new(10., -10.), Vec2::new(10., 10.), Vec2::new(-10., 10.)]);
    let hole = Loop2::polygon(&[Vec2::new(-3., -2.), Vec2::new(3., -2.), Vec2::new(3., 2.), Vec2::new(-3., 2.)]);
    let body = extrude(&Plane::XY, &[Region2 { outer, holes: vec![hole] }], -4., 7.).unwrap().remove(0);
    let reflected = transform_matrix(&body, [[-1., 0., 0., 0.], [0., 2., 0., 0.], [0.2, 0., 1., 0.], [30., 40., 50., 1.]]).unwrap();
    assert_exact_planes(&reflected);
    assert_eq!(
        reflected.face_sources().unwrap().iter().filter(|source| matches!(source, Some(FaceSource::ExtrudeSide { plane: Some(_), .. }))).count(),
        8
    );
    assert_eq!(
        reflected.face_sources().unwrap().iter().filter(|source| matches!(source, Some(FaceSource::ExtrudeCap { high: false, .. }))).count(),
        1
    );
}

#[test]
fn side_basis_and_attached_coordinates_stay_continuous_through_vertical() {
    let mut previous = None;
    for epsilon in [1e-6, -1e-6] {
        let region =
            Region2 { outer: Loop2::polygon(&[Vec2::ZERO, Vec2::new(epsilon, 10.), Vec2::new(-5., 10.), Vec2::new(-5., 0.)]), holes: vec![] };
        let body = extrude(&Plane::XY, &[region], 0., 5.).unwrap().remove(0);
        let plane = body
            .face_sources()
            .unwrap()
            .iter()
            .enumerate()
            .find_map(|(index, source)| match source {
                Some(FaceSource::ExtrudeSide { segment: Seg2::Line { a, b }, .. }) if a.dist(Vec2::ZERO) < 1e-12 && b.y == 10. && b.x == epsilon => {
                    body.face_plane(index)
                }
                _ => None,
            })
            .unwrap();
        assert!(plane.x.dot(Vec3::Y) > 0.999999);
        assert_eq!(plane.y, Vec3::Z);
        let attached = plane.to_world(Vec2::new(3., 2.));
        assert!(attached.dist(Vec3::new(epsilon * 0.3, 3., 2.)) < 1e-10);
        if let Some(old) = previous {
            assert!(attached.dist(old) < 1e-6);
        }
        previous = Some(attached);
        assert_exact_planes(&body);
    }
}

#[test]
fn constructed_annulus_sides_follow_exact_source_edges() {
    let ring = Region2 { outer: Loop2::circle(Vec2::default(), 10.), holes: vec![Loop2::circle(Vec2::default(), 9.).reversed()] };
    for (start, end) in [(0., 7.), (-7., 0.), (-7., 7.)] {
        let bodies = extrude(&Plane::XY, std::slice::from_ref(&ring), start, end).unwrap();
        let body = &bodies[0];
        let sources = body.face_sources().unwrap();
        assert_eq!(sources.len(), body.face_count());
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
        let mut inner = 0;
        let mut outer = 0;
        for (index, source) in sources.iter().enumerate() {
            let Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { radius, .. }, .. }) = source else { continue };
            if (*radius - 10.).abs() < 1e-12 {
                outer += 1;
            } else {
                assert!((*radius - 9.).abs() < 1e-12);
                inner += 1;
            }
            let mut vertices = 0;
            for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
                if *face as usize != index {
                    continue;
                }
                for point in mesh.tri(triangle).unwrap() {
                    assert!((point.x.hypot(point.y) - radius).abs() < 1e-8, "face {index}, source {radius}, {point:?}");
                    vertices += 1;
                }
            }
            assert!(vertices > 0);
        }
        assert_eq!((outer, inner), (2, 2), "full circles split into two actual edges each");
        assert_eq!(sources.iter().filter(|source| matches!(source, Some(FaceSource::ExtrudeCap { .. }))).count(), 2);
    }
}

#[test]
fn constructed_side_identity_survives_affine_placement_without_recognition() {
    let region = Region2 { outer: Loop2::circle(Vec2::default(), 10.), holes: vec![] };
    let bodies = extrude(&Plane::named("XY").unwrap(), &[region], 0., 7.).unwrap();
    let source = &bodies[0];
    let placed = transform_matrix(source, [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [25., 30., 40., 1.]]).unwrap();
    for (original, placed) in source.face_sources().unwrap().iter().zip(placed.face_sources().unwrap()) {
        match (original, placed) {
            (Some(FaceSource::ExtrudeSide { segment: a, .. }), Some(FaceSource::ExtrudeSide { segment: b, .. })) => assert_eq!(a, b),
            (Some(FaceSource::ExtrudeCap { high: a, region: ar, .. }), Some(FaceSource::ExtrudeCap { high: b, region: br, .. })) => {
                assert_eq!(a, b);
                assert_eq!(ar, br)
            }
            _ => panic!("source role changed"),
        }
    }
    assert!((placed.size() - source.size()).abs() < 1e-8);
    assert!(placed.faces(0.001).unwrap().iter().all(|face| face.centroid.dist(Vec3::new(25., 30., 43.5)) < 11.));
}

#[test]
fn constructed_large_arc_split_and_reversal_preserve_one_source_segment() {
    let arc = Seg2::Arc { center: Vec2::default(), radius: 10., start: 0., sweep: std::f64::consts::PI * 5. / 3. };
    let outline = Loop2 { segs: vec![arc, Seg2::Line { a: arc.end(), b: arc.start() }] };
    for outline in [outline.clone(), outline.reversed()] {
        let body = extrude(&Plane::XY, &[Region2 { outer: outline, holes: vec![] }], 0., 7.).unwrap().remove(0);
        let sources = body.face_sources().unwrap();
        let arcs: Vec<_> = sources
            .iter()
            .filter_map(|source| match source {
                Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { radius, sweep, .. }, .. }) => Some((radius, sweep)),
                _ => None,
            })
            .collect();
        assert_eq!(arcs.len(), 2, "large input arc explicitly creates two actual boundary edges");
        assert!(arcs.iter().all(|(radius, sweep)| (**radius - 10.).abs() < 1e-12 && (sweep.abs() - std::f64::consts::PI * 5. / 3.).abs() < 1e-12));
        assert_eq!(sources.iter().filter(|source| matches!(source, Some(FaceSource::ExtrudeSide { segment: Seg2::Line { .. }, .. }))).count(), 1);
        let mesh = body.mesh(&crate::MeshTolerance::chord(0.001)).unwrap();
        for (triangle, face) in mesh.triangles.iter().zip(&mesh.tri_face) {
            if matches!(sources[*face as usize], Some(FaceSource::ExtrudeSide { segment: Seg2::Arc { .. }, .. })) {
                assert!(mesh.tri(triangle).unwrap().iter().all(|point| (point.x.hypot(point.y) - 10.).abs() < 1e-7));
            }
        }
    }
}

/// A certified block cut by, or joined with, a spherical shell (r 18.5 to 40) through both its
/// spheres: booleans with a tool that has a void (#451) keep the sources of the block's ends,
/// which they leave as they were.
#[test]
fn hollow_tool_booleans_keep_planar_support_certificates() {
    use std::f64::consts::{PI, TAU};
    let ring = Region2 {
        outer: Loop2 {
            segs: vec![
                Seg2::Line { a: Vec2::new(18.5, 0.), b: Vec2::new(40., 0.) },
                Seg2::Arc { center: Vec2::ZERO, radius: 40., start: 0., sweep: PI },
                Seg2::Line { a: Vec2::new(-40., 0.), b: Vec2::new(-18.5, 0.) },
                Seg2::Arc { center: Vec2::ZERO, radius: 18.5, start: PI, sweep: -PI },
            ],
        },
        holes: vec![],
    };
    let shell = crate::revolve(&Plane::XY, &[ring], Vec2::ZERO, Vec2::X, TAU).unwrap().remove(0);
    assert_eq!(shell.shell_count(), 2, "a solid with a void");
    let block = crate::box_solid(Vec3::new(8., -4., -4.), Vec3::new(50., 4., 4.)).unwrap().with_box_certificate(42).unwrap();
    for op in [crate::BoolOp::Cut, crate::BoolOp::Union] {
        let r = crate::boolean(&block, &shell, op).unwrap().unwrap();
        let ends: Vec<f64> = (0..r.face_count())
            .filter(|i| {
                matches!(r.face_sources().and_then(|s| s.get(*i)).and_then(Option::as_ref), Some(FaceSource::Box { producer: Some(42), axis: 0, .. }))
            })
            .map(|i| r.planar_face(i).unwrap().origin.x)
            .collect();
        assert!([8., 50.].iter().all(|x| ends.iter().any(|e| (e - x).abs() < 1e-9)), "{op:?}: {ends:?}");
        assert_exact_planes(&r);
    }
}
