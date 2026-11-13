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
            solidcore_record::brep::Loop::Edges { edges } => edges.iter().any(|u| edges.iter().filter(|w| w.edge == u.edge).count() > 1),
            solidcore_record::brep::Loop::Vertex { .. } => false,
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

/// A body mirrored in SMLib reads back from its record as the same valid solid: a plate with a
/// through hole (the hole's cylinder comes out inside out) and a box with two rounds meeting at a
/// corner (their intersection edge has a tolerance of its own, wider than the body's). Read back
/// from the record, the hole's cylinder swept the wrong way about its axis 30 mm off its NURBS,
/// and the rounds' shared edge lost its tolerance to the body's.
#[test]
fn mirrored_bodies_read_back_from_their_records_valid() {
    let plate = crate::box_solid(Vec3::ZERO, Vec3::new(30., 40., 20.)).unwrap();
    let hole = crate::cylinder(Vec3::new(20., 20., -5.), Vec3::Z, 5.0, 30.0).unwrap();
    let holed = crate::boolean(&plate, &hole, crate::BoolOp::Cut).unwrap().unwrap();
    let rounded = crate::fillet(&plate, &[Vec3::new(15., 0., 20.), Vec3::new(0., 0., 10.)], 3.0).unwrap();
    // The mirror in x = 30.
    let mirror = [[-1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [60., 0., 0., 1.]];
    for (what, body) in [("holed plate", holed), ("rounded box", rounded)] {
        let mirrored = transform_matrix(&body, mirror).unwrap();
        let read_back = Body::from_record(mirrored.record().clone()).unwrap();
        assert!(matches!(crate::validity(&read_back).smlib, Some(crate::SmlibCheck::Valid)), "{what}: {:?}", crate::validity(&read_back).smlib);
        let (want, got) = (crate::measure(&body).unwrap().volume, crate::measure(&read_back).unwrap().volume);
        assert!((got - want).abs() < 1e-6 * want, "{what}: volume {got}, expected {want}");
    }
}

/// A long body mirrored in SMLib measures as its source does, exactly. SMLib's mirror gives each
/// line as a B-spline over the line's whole parameter range (control points 1,234,567 edge
/// lengths out): a box 50 x 900 x 50 mirrored in x = 0 could not be measured ("invalid B-rep
/// record: flat arrays", past 1e9 from an edge of 810 mm; #14413). Every straight edge comes
/// back as the line between its vertices.
#[test]
fn a_long_mirrored_body_measures_as_its_source() {
    let mirror = [[-1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]];
    for (what, corner, size) in [
        ("short box", Vec3::new(100., 0., 0.), Vec3::new(50., 500., 50.)),
        ("box along y", Vec3::new(100., 0., 0.), Vec3::new(50., 900., 50.)),
        ("box along y, far from the plane", Vec3::new(3000., -450., 0.), Vec3::new(50., 1100., 50.)),
        ("box along x", Vec3::new(100., 0., 0.), Vec3::new(2000., 50., 50.)),
        ("box along z", Vec3::new(100., 0., 0.), Vec3::new(50., 50., 2000.)),
    ] {
        let body = crate::box_solid(corner, corner + size).unwrap();
        let mirrored = transform_matrix(&body, mirror).unwrap();
        assert!(
            mirrored.record().edges.iter().all(|e| matches!(e.curve, solidcore_record::brep::Curve::Line { .. }) && e.nurbs.is_none()),
            "{what}: a straight edge is not a line"
        );
        let (want, got) = (crate::measure(&body).unwrap(), crate::measure(&mirrored).unwrap_or_else(|e| panic!("{what}: {e}")));
        assert!((got.volume - want.volume).abs() < 1e-9 * want.volume, "{what}: volume {}, expected {}", got.volume, want.volume);
        let (lo, hi) = (got.bbox.min, got.bbox.max);
        let (wlo, whi) = (want.bbox.min, want.bbox.max);
        let expected = (Vec3::new(-whi.x, wlo.y, wlo.z), Vec3::new(-wlo.x, whi.y, whi.z));
        assert!(lo.dist(expected.0) < 1e-9 && hi.dist(expected.1) < 1e-9, "{what}: box {lo:?}..{hi:?}, expected {expected:?}");
        let read_back = Body::from_record(mirrored.record().clone()).unwrap();
        assert!(matches!(crate::validity(&read_back).smlib, Some(crate::SmlibCheck::Valid)), "{what}: {:?}", crate::validity(&read_back).smlib);
    }
}

/// A scaling or shear of a box maps its record as a rigid motion does: its faces stay planes, on
/// the same parameters, and the volume is the determinant's (#14422). A reflection, or a body
/// with a cylinder, still goes through SMLib and keeps the right volume.
#[test]
fn a_box_scaled_or_sheared_keeps_its_planes() {
    let planes = |b: &Body| b.record().faces.iter().all(|f| matches!(f.surface, solidcore_record::brep::Surface::Plane { .. }));
    let volume = |b: &Body| b.mesh(&crate::MeshTolerance::chord(1e-3)).unwrap().measure().volume;
    let source = crate::box_solid(Vec3::ZERO, Vec3::new(10., 10., 10.)).unwrap();
    assert!(planes(&source));
    for (matrix, want) in [
        ([[2., 0., 0., 0.], [0., 3., 0., 0.], [0., 0., 0.5, 0.], [0., 0., 0., 1.]], 3000.),
        ([[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1.0001, 0.], [5., 0., 0., 1.]], 1000.1),
        ([[1., 0., 0., 0.], [0.5, 1., 0., 0.], [0., 0., 2., 0.], [0., 0., 0., 1.]], 2000.),
    ] {
        let scaled = transform_matrix(&source, matrix).unwrap();
        assert_topology(&source, &scaled, matrix);
        assert!(planes(&scaled));
        assert!((volume(&scaled) - want).abs() < 1e-6, "{} vs {want}", volume(&scaled));
        assert!(scaled.record().tolerance >= source.record().tolerance);
    }
    let mirrored = transform_matrix(&source, [[-2., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [0., 0., 0., 1.]]).unwrap();
    assert!((volume(&mirrored) - 2000.).abs() < 1e-6);
    let stretched = transform_matrix(&periodic_cylinder(), [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 2., 0.], [0., 0., 0., 1.]]).unwrap();
    assert!((volume(&stretched) - 4000. * std::f64::consts::PI).abs() < 5., "{}", volume(&stretched));
}
