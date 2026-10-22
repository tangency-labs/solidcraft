use super::*;
use crate::history::BodyHistory;

/// `p` moved by the column-major affine `matrix`.
fn moved(matrix: [[f64; 4]; 4], p: [f64; 3]) -> Vec3 {
    Vec3::new(
        matrix[0][0] * p[0] + matrix[1][0] * p[1] + matrix[2][0] * p[2] + matrix[3][0],
        matrix[0][1] * p[0] + matrix[1][1] * p[1] + matrix[2][1] * p[2] + matrix[3][1],
        matrix[0][2] * p[0] + matrix[1][2] * p[1] + matrix[2][2] * p[2] + matrix[3][2],
    )
}

/// A rigid placement moves the record and nothing else: the same regions, shells, faces' loops
/// (edges, directions and pcurves: each surface moves with its parameters), edges' ends and uses;
/// every vertex moved by the motion; and the history the identity's.
fn assert_topology(source: &Body, placed: &Body, matrix: [[f64; 4]; 4]) {
    let (a, b) = (source.record(), placed.record());
    assert_eq!(a.regions, b.regions);
    assert_eq!(a.shells, b.shells);
    assert_eq!(a.faces.len(), b.faces.len());
    for (fa, fb) in a.faces.iter().zip(&b.faces) {
        assert_eq!(fa.loops, fb.loops);
        assert_eq!(fa.domain, fb.domain);
    }
    assert_eq!(a.edges.len(), b.edges.len());
    for (ea, eb) in a.edges.iter().zip(&b.edges) {
        assert_eq!((ea.vertices, &ea.uses, ea.interval), (eb.vertices, &eb.uses, eb.interval));
    }
    assert_eq!(a.vertices.len(), b.vertices.len());
    for (va, vb) in a.vertices.iter().zip(&b.vertices) {
        assert!(Vec3::from(vb.point).dist(moved(matrix, va.point)) < 1e-9);
    }
    assert_eq!(placed.history(), Some(&<crate::history::History as BodyHistory>::identity(source)));
}

/// A cylinder whose wall is one face with a seam: the seam edge is used twice by the wall's loop.
fn periodic_cylinder() -> Body {
    crate::cylinder(Vec3::ZERO, Vec3::Z, 10.0, 20.0).unwrap()
}

#[test]
fn periodic_step_seam_survives_identity_rotation_and_translation() {
    let source = periodic_cylinder();
    let bytes = crate::step_export(&[&source], "placement regression").unwrap();
    let imported = crate::step_import(&bytes).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    let body = &imported.bodies.first().unwrap().body;
    // A face whose loop uses an edge twice (the seam).
    let seamed = body.record().faces.iter().any(|f| {
        f.loops.iter().any(|l| match l {
            solidcraft_geom::brep::Loop::Edges { edges } => edges.iter().any(|u| edges.iter().filter(|w| w.edge == u.edge).count() > 1),
            solidcraft_geom::brep::Loop::Vertex { .. } => false,
        })
    });
    assert!(seamed);
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
