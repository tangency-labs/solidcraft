use super::*;
use crate::body::{p3, v3};
use mt::builder;

fn assert_topology(source: &Body, placed: &Body, matrix: [[f64; 4]; 4]) {
    use std::collections::HashMap;
    let mut vertices = HashMap::new();
    let mut edges = HashMap::new();
    for (a, b) in source.solid.face_iter().zip(placed.solid.face_iter()) {
        assert_eq!(a.orientation(), b.orientation());
        let aw = a.absolute_boundaries();
        let bw = b.absolute_boundaries();
        assert_eq!(aw.len(), bw.len());
        for (a, b) in aw.iter().zip(bw.iter()) {
            assert_eq!(a.len(), b.len());
            for (a, b) in a.edge_iter().zip(b.edge_iter()) {
                assert_eq!(a.orientation(), b.orientation());
                assert_ne!(a.id(), b.id());
                assert_eq!(*edges.entry(a.id()).or_insert(b.id()), b.id());
                for (a, b) in [(a.absolute_front(), b.absolute_front()), (a.absolute_back(), b.absolute_back())] {
                    assert_ne!(a.id(), b.id());
                    assert_eq!(*vertices.entry(a.id()).or_insert(b.id()), b.id());
                    let p = from_p3(a.point());
                    let expected = Vec3::new(
                        matrix[0][0] * p.x + matrix[1][0] * p.y + matrix[2][0] * p.z + matrix[3][0],
                        matrix[0][1] * p.x + matrix[1][1] * p.y + matrix[2][1] * p.z + matrix[3][1],
                        matrix[0][2] * p.x + matrix[1][2] * p.y + matrix[2][2] * p.z + matrix[3][2],
                    );
                    assert!(from_p3(b.point()).dist(expected) < 1e-9);
                }
            }
        }
    }
    assert_eq!(edges.len(), source.solid.edge_iter().map(|e| e.id()).collect::<std::collections::HashSet<_>>().len());
}

fn periodic_cylinder() -> Body {
    let b0 = builder::vertex(p3(Vec3::new(10., 0., 0.)));
    let b1 = builder::vertex(p3(Vec3::new(-10., 0., 0.)));
    let t0 = builder::vertex(p3(Vec3::new(10., 0., 20.)));
    let t1 = builder::vertex(p3(Vec3::new(-10., 0., 20.)));
    let bottom: mt::Wire =
        vec![builder::circle_arc(&b0, &b1, p3(Vec3::new(0., 10., 0.))), builder::circle_arc(&b1, &b0, p3(Vec3::new(0., -10., 0.)))].into();
    let top: mt::Wire =
        vec![builder::circle_arc(&t0, &t1, p3(Vec3::new(0., 10., 20.))), builder::circle_arc(&t1, &t0, p3(Vec3::new(0., -10., 20.)))].into();
    let seam = builder::line(&b0, &t0);
    let mut boundary: Vec<_> = bottom.edge_iter().cloned().collect();
    boundary.push(seam.clone());
    boundary.extend(top.inverse().edge_iter().cloned());
    boundary.push(seam.inverse());
    let wall = mt::Face::new_unchecked(
        vec![boundary.into()],
        mt::Surface::RevolutedCurve(mt::Processor::new(mt::RevolutedCurve::by_revolution(
            seam.inverse().oriented_curve(),
            p3(Vec3::ZERO),
            v3(Vec3::Z),
        ))),
    );
    let lower = builder::try_attach_plane(&[bottom.inverse()]).unwrap();
    let upper = builder::try_attach_plane(&[top]).unwrap();
    Body::new(Solid::new_unchecked(vec![vec![wall, lower, upper].into()])).unwrap()
}

#[test]
fn periodic_step_seam_survives_identity_rotation_and_translation() {
    let source = periodic_cylinder();
    let bytes = crate::step_export(&[&source], "placement regression").unwrap();
    let imported = crate::step_import(&bytes).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let body = &imported.bodies.first().unwrap().body;
    assert!(body.solid.face_iter().any(|f| f.absolute_boundaries().iter().any(|w| !w.is_simple())));
    let source_vertices: Vec<_> = body.solid.vertex_iter().map(|v| from_p3(v.point())).collect();
    let source_faces = body.solid.face_iter().count();
    let unchanged = crate::step_export(&[body], "source snapshot").unwrap();
    let before = body.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    assert!((before - 2000. * std::f64::consts::PI).abs() < 1.);
    for matrix in [
        [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]],
        [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [10., 10., 0., 1.]],
        [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [10., 10., 30., 1.]],
    ] {
        let moved = transform_matrix(body, matrix).unwrap();
        assert_topology(body, &moved, matrix);
        assert_eq!(moved.solid.face_iter().count(), source_faces);
        assert_eq!(moved.solid.vertex_iter().count(), source_vertices.len());
        let after = moved.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
        assert!((after - before).abs() < 1e-5, "{after} vs {before}");
        let roundtrip = crate::step_import(&crate::step_export(&[&moved], "placement regression").unwrap()).unwrap();
        assert!(roundtrip.warnings.is_empty(), "{:?}", roundtrip.warnings);
        assert!((roundtrip.bodies.first().unwrap().body.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume - before).abs() < 1e-4);
    }
    let rigid = transform(body, Vec3::new(10., 10., 30.), Vec3::ZERO, Vec3::Z, std::f64::consts::FRAC_PI_2).unwrap();
    assert_topology(body, &rigid, [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [10., 10., 30., 1.]]);
    assert_eq!(crate::step_export(&[body], "source snapshot").unwrap(), unchanged);
}
