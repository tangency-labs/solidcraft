use super::*;
use solidcraft_geom::Plane;

fn stamped_stock() -> Body {
    crate::box_solid(Vec3::new(-20., -15., 0.), Vec3::new(20., 15., 24.)).unwrap().with_box_certificate(7).unwrap()
}

fn cup(stock: &Body) -> Body {
    crate::shell(stock, &[Vec3::new(0., 0., 24.)], 1.).unwrap().with_shell_certificate(9).unwrap()
}

#[test]
fn stamped_shell_keeps_authored_painted_and_split_boundaries() {
    let stock = stamped_stock();
    let ordinary = cup(&stock);
    assert!(!ordinary.regularization_protected);
    let painted = stock.clone().with_paint(Some(Paint { opacity: 1., faces: vec![FacePaint { face: 0, color: [1., 0., 0.], opacity: 1. }] }));
    let side = (0..stock.face_count()).find(|i| stock.planar_face(*i).is_some_and(|p| p.normal().dot(Vec3::X) > 0.999 && p.origin.x == 20.)).unwrap();
    let split = crate::split_faces(&stock, &[side], &crate::SplitTool::Plane(Plane { origin: Vec3::new(0., 0., 8.), ..Plane::XY })).unwrap();
    assert!(split.split_lines().is_some());
    // SMLib's shell makes no section strips: the ordinary cup is the stock's five kept faces,
    // their five linings and the rim (11). A paint over a whole face authors no boundary inside
    // it, so the painted cup has those same faces; the split side's line stays an edge on both
    // walls (13).
    for (protected, faces) in [(painted, ordinary.face_count()), (split, ordinary.face_count() + 2)] {
        let result = cup(&protected);
        assert!(result.regularization_protected);
        assert_eq!(result.face_count(), faces, "authored boundaries must survive Shell");
        assert_eq!(result.split_lines(), protected.split_lines());
        assert!(result.validity().is_empty());
        let expected = 40. * 30. * 24. - 38. * 28. * 23.;
        let mesh = result.mesh(&crate::MeshTolerance::chord(1e-5)).unwrap();
        assert!((mesh.measure().volume - expected).abs() < 1e-6);
        assert!(mesh.contains(Vec3::new(19.5, 0., 12.)));
        assert!(!mesh.contains(Vec3::new(0., 0., 12.)));
        let count = result.face_count();
        assert_eq!(result.clone().regularize_certified_planar_faces().unwrap().face_count(), count);
        let placed = crate::transform(&result, Vec3::new(30., -40., 7.), Vec3::ZERO, Vec3::Z, 0.3).unwrap();
        assert!(placed.regularization_protected);
        for lump in placed.lumps().unwrap() {
            assert!(lump.regularization_protected);
        }
        let cleared = result.clone().with_paint(None).with_split_lines(None);
        assert!(cleared.regularization_protected, "cleared appearance must not erase inherited boundary protection");
        assert_eq!(cleared.regularize_certified_planar_faces().unwrap().face_count(), count);
    }
}

#[test]
fn protection_is_identity_and_survives_tool_operand_and_compounds() {
    let a = stamped_stock();
    let protected = a.clone().with_paint(Some(Paint { opacity: 1., faces: vec![FacePaint { face: 0, color: [0., 1., 0.], opacity: 1. }] }));
    let cleared = protected.with_paint(None);
    assert!(!a.same(&cleared));
    let tool = crate::box_solid(Vec3::new(-5., -5., 23.), Vec3::new(5., 5., 30.)).unwrap().inherit_regularization_protection(&cleared);
    let cut = crate::boolean(&a, &tool, crate::BoolOp::Cut).unwrap().unwrap();
    assert!(cut.regularization_protected);
    let remote = crate::box_solid(Vec3::new(100., 0., 0.), Vec3::new(110., 10., 10.)).unwrap();
    let joined = crate::ops::join_pieces(vec![cleared, remote]).unwrap().unwrap();
    assert!(joined.regularization_protected);
    assert_eq!(joined.lumps().unwrap().len(), 2);
    assert!(joined.lumps().unwrap().iter().all(|body| body.regularization_protected));
}
