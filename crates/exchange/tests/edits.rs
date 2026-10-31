//! The snapshot contract: the feature timeline, and edits (suppress, partition, face split).

mod common;

use std::f64::consts::PI;

use common::*;
use solidcraft_exchange::{
    AppliedEdit, BodyName, Curve, Direction, Edit, Error, FeatureId, FeatureKind, Mm, Operation, Plane, Point, Surface, open, to_json,
};

fn plane(point: [f64; 3], normal: [f64; 3]) -> Plane {
    Plane::new(Point(point), Direction::new(normal).unwrap())
}

/// The timeline: a box that makes the body and a cylinder that cuts it, with their dimensions,
/// and the faces each made.
#[test]
fn features_list_their_kinds_dimensions_and_faces() {
    let s = snap_with(&holed_doc(), Vec::new()).unwrap();
    assert_eq!(s.features.len(), 2);
    let (block, hole) = (&s.features[0], &s.features[1]);
    assert_eq!(
        block.kind,
        FeatureKind::Box { operation: Operation::NewBody, corner: Some(Point([0.0; 3])), size: Some([Mm(20.0), Mm(20.0), Mm(10.0)]) }
    );
    assert_eq!(
        hole.kind,
        FeatureKind::Cylinder {
            operation: Operation::Cut,
            base: Some(Point([10.0, 10.0, -1.0])),
            axis: Some(Direction::Z),
            radius: Some(Mm(3.0)),
            height: Some(Mm(12.0))
        }
    );
    assert_eq!((block.kind_name.as_str(), hole.kind_name.as_str()), ("box", "cylinder"));
    assert!(hole.inputs.iter().any(|i| i.label == "Radius"));
    let body = &s.bodies[0];
    for f in [block, hole] {
        assert_eq!(f.bodies, vec![body.name.clone()]);
        assert!(!f.suppressed);
    }
    let made = |id: FeatureId| body.faces.iter().filter(|f| f.features.first() == Some(&id)).count();
    assert_eq!((made(block.id), made(hole.id)), (6, 2));
}

/// Suppressing the hole fills it: six planes, the full volume; the design itself keeps it.
#[test]
fn suppressing_a_feature_rebuilds_without_it() {
    let doc = holed_doc();
    let hole = snap_with(&doc, Vec::new()).unwrap().features[1].id;
    let s = snap_with(&doc, vec![Edit::Suppress(hole)]).unwrap();
    let b = &s.bodies[0];
    assert_eq!(b.faces.len(), 6);
    assert!((volume(b) - 4000.0).abs() < 1e-6, "{}", volume(b));
    assert!(b.faces.iter().all(|f| matches!(f.surface, Surface::Plane { .. })));
    assert!(s.features[1].suppressed && s.features[1].bodies.is_empty());
    let [AppliedEdit { edit, features, bodies, .. }] = s.edits.as_slice() else { panic!("{:?}", s.edits) };
    assert_eq!((edit, features.as_slice(), bodies.len()), (&Edit::Suppress(hole), &[hole][..], 0));
    // The same design without the edit still has its hole.
    assert_eq!(snap_with(&doc, Vec::new()).unwrap().bodies[0].faces.len(), 8);
    assert!(matches!(snap_with(&doc, vec![Edit::Suppress(FeatureId(999))]), Err(Error::NoFeature(FeatureId(999)))));
}

/// A partition at x = 5 cuts the plate in two (the hole stays whole in the second), each with a
/// cut face naming the partition's feature; a face split at x = 15 splits the top in two pieces
/// named after it and leaves the solid as it was.
#[test]
fn partitions_and_face_splits_are_named_after_their_features() {
    let doc = holed_doc();
    let whole = volume(&snap_with(&doc, Vec::new()).unwrap().bodies[0]);
    let cut = Edit::Partition { name: "Mid".into(), plane: plane([5.0, 0.0, 0.0], [1.0, 0.0, 0.0]), bodies: Vec::new() };
    let s = snap_with(&doc, vec![cut]).unwrap();
    assert_eq!(s.bodies.len(), 2, "{:?}", s.bodies.iter().map(|b| &b.name).collect::<Vec<_>>());
    let [edit] = s.edits.as_slice() else { panic!("{:?}", s.edits) };
    assert_eq!((edit.bodies.len(), edit.features.len()), (1, 1));
    let mut volumes: Vec<f64> = s.bodies.iter().map(volume).collect();
    volumes.sort_by(f64::total_cmp);
    assert!((volumes[0] - 1000.0).abs() < 1e-6, "{volumes:?}");
    assert!((volumes[0] + volumes[1] - whole).abs() < 1e-6, "{volumes:?} {whole}");
    assert!((whole - (4000.0 - PI * 9.0 * 10.0)).abs() < 1.0, "{whole}");
    for b in &s.bodies {
        assert!(b.closed);
        assert_eq!(b.faces.iter().filter(|f| f.features.first() == Some(&edit.features[0])).count(), 1, "{}: one cut face", b.name);
    }
    // The top, split at x = 15.
    let plain = snap_with(&doc, Vec::new()).unwrap();
    let top = plain.bodies[0].faces.iter().find(|f| matches!(f.surface, Surface::Plane { normal, .. } if normal.get()[2] > 0.5)).unwrap();
    let split = Edit::SplitFace {
        name: "Strip".into(),
        body: plain.bodies[0].name.clone(),
        face: top.name.clone(),
        plane: plane([15.0, 0.0, 0.0], [1.0, 0.0, 0.0]),
    };
    let s = snap_with(&doc, vec![split]).unwrap();
    let b = &s.bodies[0];
    assert_eq!(b.faces.len(), 9);
    let pieces = b.faces.iter().filter(|f| f.name.as_str().starts_with(&format!("{}#", top.name))).count();
    assert_eq!(pieces, 2, "{:?}", b.faces.iter().map(|f| &f.name).collect::<Vec<_>>());
    assert!((volume(b) - whole).abs() < 1e-6, "{} {whole}", volume(b));
    assert!(
        b.edges.iter().any(|e| matches!(e.curve, Curve::Line { start, end } if (start.0[0] - 15.0).abs() < 1e-9 && (end.0[0] - 15.0).abs() < 1e-9))
    );
}

#[test]
fn hostile_edits_are_refused_without_panicking() {
    let doc = holed_doc();
    let x = plane([5.0, 0.0, 0.0], [1.0, 0.0, 0.0]);
    for (e, typed) in [
        (Edit::Partition { name: String::new(), plane: x, bodies: Vec::new() }, false),
        (Edit::Partition { name: "P".into(), plane: Plane::new(Point([f64::NAN, 0.0, 0.0]), Direction::X), bodies: Vec::new() }, false),
        (Edit::Partition { name: "B".into(), plane: x, bodies: vec![BodyName::from("Nope")] }, true),
        (Edit::SplitFace { name: "S".into(), body: "Nope".into(), face: "F1:+z".into(), plane: x }, true),
        (Edit::SplitFace { name: "S".into(), body: "Body1".into(), face: "nope".into(), plane: x }, true),
        (Edit::Suppress(FeatureId(u64::MAX)), true),
    ] {
        let r = snap_with(&doc, vec![e.clone()]);
        assert!(r.is_err(), "{e:?}");
        if typed {
            assert!(matches!(r, Err(Error::NoBody(_) | Error::NoFace { .. } | Error::NoFeature(_))), "{e:?}: {r:?}");
        }
    }
    assert!(matches!(snap_with(&doc, vec![Edit::Suppress(FeatureId(1)); solidcraft_exchange::MAX_EDITS + 1]), Err(Error::Limit(_))));
    // A plane that misses every body cuts nothing.
    let miss = Edit::Partition { name: "Far".into(), plane: plane([500.0, 0.0, 0.0], [1.0, 0.0, 0.0]), bodies: Vec::new() };
    let s = snap_with(&doc, vec![miss]).unwrap();
    assert_eq!((s.bodies.len(), s.edits[0].features.len()), (1, 0));
    // The JSON form; a zero normal is no direction.
    let edits: Vec<Edit> =
        serde_json::from_str(r#"[{"suppress": 2}, {"partition": {"name": "Mid", "plane": {"point": [5, 0, 0], "normal": [2, 0, 0]}}}]"#).unwrap();
    assert_eq!(edits, vec![Edit::Suppress(FeatureId(2)), Edit::Partition { name: "Mid".into(), plane: x, bodies: Vec::new() }]);
    for bad in [
        "{",
        r#"[{"explode": 1}]"#,
        r#"[{"suppress": -1}]"#,
        r#"[{"partition": {"name": "x"}}]"#,
        r#"[{"partition": {"name": "x", "plane": {"point": [0, 0, 0], "normal": [0, 0, 0]}}}]"#,
        r#"[{"suppress": 1, "extra": 2}]"#,
    ] {
        assert!(serde_json::from_str::<Vec<Edit>>(bad).is_err(), "{bad}");
    }
    // Edits give the same bytes every time.
    let cut = Edit::Partition { name: "Mid".into(), plane: x, bodies: Vec::new() };
    let design = open(&bytes(&doc)).unwrap();
    let options = solidcraft_exchange::Options::default().with_edits(vec![cut]);
    assert_eq!(to_json(&design.snapshot(&options).unwrap()).unwrap(), to_json(&design.snapshot(&options).unwrap()).unwrap());
}
