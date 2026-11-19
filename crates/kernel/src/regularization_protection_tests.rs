use super::*;
use solidcore_record::Plane;

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
    let painted = stock.clone().with_paint(Some(Paint {
        opacity: 1.,
        faces: vec![FacePaint { face: 0, color: [1., 0., 0.], opacity: 1., name: None }],
        name: None,
    }));
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
    let protected = a.clone().with_paint(Some(Paint {
        opacity: 1.,
        faces: vec![FacePaint { face: 0, color: [0., 1., 0.], opacity: 1., name: None }],
        name: None,
    }));
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

#[test]
fn matched_replacement_retains_sticky_protection_and_authored_lines_from_both_operands() {
    use crate::history::BodyHistory;
    fn protected(body: Body) -> Body {
        body.with_paint(Some(Paint { opacity: 1., faces: vec![FacePaint { face: 0, color: [1., 0., 0.], opacity: 1., name: None }], name: None }))
            .with_paint(None)
    }
    for cut in [false, true] {
        let base = crate::box_solid(Vec3::new(-1., -1., -2.), Vec3::new(1., 1., 0.)).unwrap();
        let top = (0..base.face_count()).find(|&i| base.planar_face(i).is_some_and(|p| p.normal().z > 0.99)).unwrap();
        let (low, high, normal) = if cut { (-1., 0., Vec3::Z) } else { (0., 1., -Vec3::Z) };
        let tool = crate::box_solid(Vec3::new(-1., -1., low), Vec3::new(1., 1., high)).unwrap();
        let opening = (0..tool.face_count()).find(|&i| tool.planar_face(i).is_some_and(|p| p.normal().dot(normal) > 0.99)).unwrap();
        let skin = crate::remove_faces(&tool, &[opening]).unwrap();
        assert!(!base.regularization_protected && !skin.regularization_protected);
        // Join takes the noncoincident path. Cut touches the four planar side
        // walls and takes their exact planar-difference/sew path.
        for protect_base in [false, true] {
            let (base, skin) = if protect_base { (protected(base.clone()), skin.clone()) } else { (base.clone(), protected(skin.clone())) };
            let result = crate::replace_matched_face(&base, top, &skin, cut, 1e-6).unwrap().expect("qualified planar replacement");
            assert!(result.regularization_protected, "cut={cut}, protect_base={protect_base}");
            assert!(result.paint().is_none() && result.split_lines().is_none(), "protection is sticky without current appearance metadata");
            assert!(!result.is_surface() && result.validity().is_empty());
            assert!((crate::measure(&result).unwrap().volume - if cut { 4. } else { 12. }).abs() < 1e-8);
            result.history().unwrap().check(&result, &[&base, &skin]).unwrap();
            let count = result.face_count();
            assert_eq!(
                result.regularize_certified_planar_faces().unwrap().face_count(),
                count,
                "future regularization must preserve authored boundaries"
            );
        }
        let base_line = vec![Vec3::new(-0.5, 0., -2.), Vec3::new(0.5, 0., -2.)];
        let skin_z = if cut { -1. } else { 1. };
        let skin_line = vec![Vec3::new(-0.5, 0., skin_z), Vec3::new(0.5, 0., skin_z)];
        let base = base.with_split_lines(Some(std::sync::Arc::new(vec![base_line.clone()])));
        let skin = skin.with_split_lines(Some(std::sync::Arc::new(vec![skin_line.clone()])));
        let result = crate::replace_matched_face(&base, top, &skin, cut, 1e-6).unwrap().expect("qualified split replacement");
        assert!(result.regularization_protected);
        assert_eq!(result.split_keep().iter().filter(|line| *line == &base_line).count(), 1);
        assert_eq!(result.split_keep().iter().filter(|line| *line == &skin_line).count(), 1);
        result.history().unwrap().check(&result, &[&base, &skin]).unwrap();
        assert_eq!(base.split_keep(), &[base_line]);
        assert_eq!(skin.split_keep(), &[skin_line]);
    }
}
