//! Delete Face: a fillet, a hole and a chamfer taken off, healed from the neighbouring planes.

use serde_json::{Value, json};

use crate::Session;

fn run(s: &mut Session, id: &str, p: Value) -> Value {
    match s.execute(id, &p) {
        Ok(v) => v,
        Err(e) => panic!("{id} {p}: {e}"),
    }
}

fn volume(s: &mut Session, b: &str) -> f64 {
    run(s, "inspect.measure", json!({ "bodies": [b] }))["bodies"][0]["volume_mm3"].as_f64().unwrap_or(0.0)
}

#[test]
fn delete_a_fillet_face() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 20]], "radius": 5}));
    assert!(volume(&mut s, "B") < 24000.0 - 1.0);
    // A point on the fillet's surface: 45° round from its axis (35, ·, 15).
    let h = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
    let r = run(&mut s, "face.delete", json!({"faces": [[35.0 + h, 15, 15.0 + h]]}));
    assert!(r["feature"].is_u64(), "{r}");
    assert!((volume(&mut s, "B") - 24000.0).abs() < 1e-3 * 24000.0, "{}", volume(&mut s, "B"));
    // Undo-able as a timeline feature, and named after its kind.
    let f = s.doc.features.last().unwrap();
    assert_eq!(f.kind.type_name(), "DeleteFaceFeature");
}

/// A through hole picked in the Delete selection: both halves of its wall go, and the plate
/// closes up.
#[test]
fn delete_a_hole_through_the_selection() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 50, "width": 30, "height": 10, "body_name": "Plate"}));
    run(&mut s, "solid.cylinder", json!({"base": [25, 15, -1], "radius": 4, "height": 12, "body_name": "Drill"}));
    run(&mut s, "solid.combine", json!({"target": "Plate", "tools": ["Drill"], "operation": "cut"}));
    let v = volume(&mut s, "Plate");
    assert!(v < 15000.0 - 100.0, "{v}");
    run(&mut s, "selection.delete", json!({"items": [{"type": "face", "body": "Plate", "index": 0, "point": [29, 15, 5]}]}));
    assert!((volume(&mut s, "Plate") - 15000.0).abs() < 1e-3 * 15000.0, "{}", volume(&mut s, "Plate"));
}

/// A face whose neighbours can't close the gap is refused, and the design is unchanged.
#[test]
fn a_box_face_cannot_be_deleted() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 10, "width": 10, "height": 10, "body_name": "B"}));
    let n = s.doc.features.len();
    let e = s.execute("face.delete", &json!({"faces": [[5, 5, 10]]})).unwrap_err().to_string();
    assert!(e.contains("Delete Face"), "{e}");
    assert_eq!(s.doc.features.len(), n);
}

/// Deleting the round on a cylinder's top edge gives the plain cylinder back: the cylinder's
/// side extends up to the top plane to close the gap. Delete Face refuses it today because only
/// planar neighbours extend ([T-40](https://tickets.solidcraft.io/t/40), #14938); a chamfer on
/// the same edge is refused the same way.
#[test]
fn delete_a_round_on_a_cylinder_edge() {
    let mut s = Session::default();
    run(&mut s, "solid.cylinder", json!({"radius": 10, "height": 20, "body_name": "Post"}));
    run(&mut s, "solid.fillet", json!({"edges": [[10, 0, 20]], "radius": 2}));
    let full = std::f64::consts::PI * 10.0 * 10.0 * 20.0;
    assert!(volume(&mut s, "Post") < full - 1.0);
    // A point on the round: 45° round its section circle, centred 8 mm out at height 18.
    let h = 2.0 * std::f64::consts::FRAC_1_SQRT_2;
    let r = run(&mut s, "face.delete", json!({"faces": [[8.0 + h, 0, 18.0 + h]]}));
    assert!(r["feature"].is_u64(), "{r}");
    assert!((volume(&mut s, "Post") - full).abs() < 1e-3 * full, "{} vs {full}", volume(&mut s, "Post"));
}

/// A deleted face whose stored name no longer finds a face (a design saved by an older version
/// names merged faces differently) is still deleted by the point it was picked at (#14206).
#[test]
fn a_deleted_face_with_a_stale_name_is_found_by_its_point() {
    let mut s = Session::default();
    run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
    run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 20]], "radius": 5}));
    let h = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
    run(&mut s, "face.delete", json!({"faces": [[35.0 + h, 15, 15.0 + h]]}));
    assert!((volume(&mut s, "B") - 24000.0).abs() < 1e-3 * 24000.0);
    let id = s.doc.features.last().map(|f| f.id).unwrap_or_default();
    s.doc_mut().feature_mut(id).unwrap().face_names = vec!["F99:side:l9".to_string()];
    s.refresh();
    assert!((volume(&mut s, "B") - 24000.0).abs() < 1e-3 * 24000.0, "{}", volume(&mut s, "B"));
}

/// With a stale name, the point must hold exactly one face of the feature's body: on an edge two
/// faces hold it, and in the air none does, so nothing is deleted and the face is to be picked again.
#[test]
fn a_stale_name_deletes_nothing_when_the_point_holds_no_face_or_several() {
    for point in [[35.0, 15.0, 20.0], [20.0, 15.0, 60.0]] {
        let mut s = Session::default();
        run(&mut s, "solid.box", json!({"length": 40, "width": 30, "height": 20, "body_name": "B"}));
        run(&mut s, "solid.fillet", json!({"edges": [[40, 15, 20]], "radius": 5}));
        let h = 5.0 * std::f64::consts::FRAC_1_SQRT_2;
        run(&mut s, "face.delete", json!({"faces": [[35.0 + h, 15, 15.0 + h]]}));
        let id = s.doc.features.last().map(|f| f.id).unwrap_or_default();
        let f = s.doc_mut().feature_mut(id).unwrap();
        f.face_names = vec!["F99:side:l9".to_string()];
        for (p, q) in f.kind.face_points_mut().into_iter().zip([point]) {
            *p = solidcore_record::Vec3::new(q[0], q[1], q[2]);
        }
        s.refresh();
        assert!(volume(&mut s, "B") < 24000.0 - 1.0, "{point:?}: the fillet stays");
    }
}
