//! Import contracts for repeated parts, using only SolidCraft's public API.
use solidcraft_geom::Vec3;
use solidcraft_kernel::{ExportBody, ExportProduct, StepHeader, box_solid, measure, step_export, step_export_products, step_import};

#[test]
fn repeated_instances_preserve_geometry_appearance_sources_and_roundtrip() {
    let part = box_solid(Vec3::ZERO, Vec3::new(2.0, 3.0, 4.0)).unwrap();
    let first = [[1., 0., 0., 0.], [0., 1., 0., 0.], [0., 0., 1., 0.], [20., 30., 40., 1.]];
    let second = [[0., 1., 0., 0.], [-1., 0., 0., 0.], [0., 0., 1., 0.], [100., 0., 0., 1.]];
    let products = [
        ExportProduct { name: "Assembly".into(), bodies: vec![], children: vec![(1, first, "Block:0".into()), (1, second, "Block:1".into())] },
        ExportProduct {
            name: "Block".into(),
            bodies: vec![ExportBody { name: "Cuboid".into(), body: &part, color: Some([0.2, 0.4, 0.6]) }],
            children: vec![],
        },
    ];
    let text = step_export_products(&products, 0, &StepHeader::default()).unwrap();
    let imported = step_import(&text).unwrap();
    assert!(imported.warnings.is_empty(), "{:?}", imported.warnings);
    assert_eq!(imported.bodies.len(), 2);
    let (first, second) = (&imported.bodies[0], &imported.bodies[1]);
    assert_eq!((first.source_item, &first.face_origin), (second.source_item, &second.face_origin));
    assert_eq!(first.body.face_sources(), second.body.face_sources());
    assert_eq!(first.body.paint(), second.body.paint());
    assert_eq!(first.color, Some([0.2, 0.4, 0.6]));
    assert_eq!(second.color, first.color);
    assert_eq!(second.path, ["Assembly", "Block"]);
    assert_eq!(second.instance.as_deref(), Some("Block:1"));
    assert_eq!(imported.tree[0].children.len(), 2);
    let bounds = [(Vec3::new(20., 30., 40.), Vec3::new(22., 33., 44.)), (Vec3::new(97., 0., 0.), Vec3::new(100., 2., 4.))];
    for (instance, (lo, hi)) in imported.bodies.iter().zip(bounds) {
        assert!(instance.closed);
        let measured = measure(&instance.body).unwrap();
        assert!((measured.volume - 24.0).abs() < 1e-6);
        assert!((measured.area - 52.0).abs() < 1e-6);
        assert!(measured.bbox.min.dist(lo) < 1e-6 && measured.bbox.max.dist(hi) < 1e-6);
        let displayed = instance.body.display_mesh(0.01).unwrap();
        assert!(displayed.bounds().min.dist(lo) < 1e-6 && displayed.bounds().max.dist(hi) < 1e-6);
        assert!(displayed.measure().volume > 0.0);
        let saved = step_export(&[&instance.body], "placed cuboid").unwrap();
        let reopened = step_import(&saved).unwrap();
        assert!(reopened.warnings.is_empty(), "{:?}", reopened.warnings);
        let measured = measure(&reopened.bodies[0].body).unwrap();
        assert!((measured.volume - 24.0).abs() < 1e-6);
        assert!(measured.bbox.min.dist(lo) < 1e-6 && measured.bbox.max.dist(hi) < 1e-6);
    }
}
