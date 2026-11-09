use crate::Document;
use solidcraft_geom::prelude::*;

/// Sketch1 (a stadium) and Extrude1, Sketch2 on Extrude1's bottom cap and Extrude2 joined from it,
/// then Sketch3 on the same cap, as 0.30.1 saved them (#13323): the attachments carry an
/// `extrude_cap` certificate, which the SMLib kernel's join no longer marks the cap with.
fn old_doc(edit: impl FnOnce(&mut serde_json::Value)) -> Document {
    let mut fixture: serde_json::Value = serde_json::from_str(include_str!("old_cap_anchor_fixture.json")).unwrap();
    edit(&mut fixture["features"]);
    let doc = Document {
        features: serde_json::from_value(fixture["features"].take()).unwrap(),
        params: serde_json::from_value(fixture["params"].take()).unwrap(),
        ..Document::default()
    };
    crate::format::read(&doc.to_json()).unwrap()
}

fn evaluate(doc: &Document) -> crate::Model {
    let mut model = crate::Model::new();
    model.evaluate(doc);
    model
}

fn old_design(edit: impl FnOnce(&mut serde_json::Value)) -> crate::Model {
    evaluate(&old_doc(edit))
}

#[test]
fn the_name_does_not_rescue_a_cap_on_an_offset_body() {
    // Moved 5 mm along the cap's own plane: the plane and the saved point still match the body as
    // built, but the sketch would stay behind at the old place. It stays lost instead.
    let mut doc = old_doc(|_| {});
    let mut offset = crate::IDENTITY;
    offset[3][0] = 5.;
    doc.body_offsets.insert("Body1".into(), offset);
    let model = evaluate(&doc);
    assert!(model.result(5).unwrap().error.is_some(), "{:?}", model.result(5));
}

fn sketch3(features: &mut serde_json::Value) -> &mut serde_json::Value {
    features.as_array_mut().unwrap().iter_mut().find(|f| f["name"] == "Sketch3").unwrap()
}

#[test]
fn a_sketch_on_a_cap_that_a_join_left_uncertified_is_found_by_its_name() {
    let model = old_design(|_| {});
    for id in [1, 2, 3, 4, 5] {
        assert!(model.result(id).unwrap().error.is_none(), "feature {id}: {:?}", model.result(id).map(|r| (&r.name, &r.error)));
    }
    let state = model.state();
    let sketch = &state.sketch(5).expect("Sketch3 should be built").plane;
    // The plane the file saved: origin (0, 0, 0), x direction (-1, 0, 0), normal +Y.
    assert!(sketch.origin.len() < 1e-9, "{sketch:?}");
    assert!((sketch.x - solidcore_record::Vec3::new(-1.0, 0.0, 0.0)).len() < 1e-9, "{sketch:?}");
    assert!((sketch.normal() - solidcore_record::Vec3::new(0.0, 1.0, 0.0)).len() < 1e-9, "{sketch:?}");
}

#[test]
fn the_name_does_not_rescue_a_cap_that_is_not_where_the_sketch_was() {
    // The plane the sketch was attached in is 1 mm off the face the name finds now.
    let model = old_design(|features| {
        sketch3(features)["plane"]["plane"]["origin"] = serde_json::json!([0.0, 1.0, 0.0]);
    });
    assert!(model.result(5).unwrap().error.is_some(), "{:?}", model.result(5));
    // A name that is no longer a face of the body is not guessed either.
    let model = old_design(|features| {
        sketch3(features)["plane"]["name"] = serde_json::json!("F2:end");
    });
    assert!(model.result(5).unwrap().error.is_some(), "{:?}", model.result(5));
}
