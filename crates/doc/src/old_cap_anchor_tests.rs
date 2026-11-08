use crate::Document;

/// Sketch1 (a stadium) and Extrude1, Sketch2 on Extrude1's bottom cap and Extrude2 joined from it,
/// then Sketch3 on the same cap, as 0.30.1 saved them (#13323): the attachments carry an
/// `extrude_cap` certificate, which the SMLib kernel's join no longer marks the cap with.
fn old_design(edit: impl FnOnce(&mut serde_json::Value)) -> crate::Model {
    let mut features: serde_json::Value = serde_json::from_str(include_str!("old_cap_anchor_fixture.json")).unwrap();
    edit(&mut features);
    let doc = Document { features: serde_json::from_value(features).unwrap(), ..Document::default() };
    let doc = crate::format::read(&doc.to_json()).unwrap();
    let mut model = crate::Model::new();
    model.evaluate(&doc);
    model
}

fn sketch3(features: &mut serde_json::Value) -> &mut serde_json::Value {
    features.as_array_mut().unwrap().iter_mut().find(|f| f["name"] == "Sketch3").unwrap()
}

#[test]
fn a_sketch_on_a_cap_that_a_join_left_uncertified_is_found_by_its_name() {
    let model = old_design(|_| {});
    for id in [3, 5] {
        assert!(model.result(id).unwrap().error.is_none(), "feature {id}: {:?}", model.result(id));
    }
    let state = model.state();
    let sketch = &state.sketch(5).expect("Sketch3 should be built").plane;
    assert!(sketch.normal().dot(solidcore_record::Vec3::new(0.0, 1.0, 0.0)) > 1.0 - 1e-9, "{sketch:?}");
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
