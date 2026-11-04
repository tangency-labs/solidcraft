// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::document::Model;
use cadmpeg_ir::products::ProductDefinition;
use cadmpeg_ir::sketches::Sketch;

use super::super::define_member;

fn sketch(name: &str) -> Sketch {
    serde_json::from_value(serde_json::json!({
        "id": format!("test:model:sketch#{name}"),
        "placement": {"kind": "unresolved"}
    }))
    .unwrap()
}

fn component(sketch: &Sketch) -> ProductDefinition {
    serde_json::from_value(serde_json::json!({
        "id": "test:model:product#component",
        "kind": "part",
        "sketches": [sketch.id]
    }))
    .unwrap()
}

#[test]
fn member_definition_owns_root_sketches_and_preserves_component_owners() {
    let root = sketch("root");
    let local = sketch("local");
    let definition = component(&local);
    let mut model = Model::default();
    model.sketches = vec![root.clone(), local];
    model.product_definitions = vec![definition.clone()];
    let ctx = cadmpeg_test_support::service_decode_context();

    let owner = define_member(&ctx, &mut model, "member").unwrap();

    assert_eq!(model.product_definitions.len(), 2);
    assert_eq!(model.product_definitions[0], definition);
    assert_eq!(model.product_definitions[1].id, owner);
    assert_eq!(model.product_definitions[1].sketches, [root.id]);
}

#[test]
fn member_definition_extends_existing_root_sketches_once() {
    let root = sketch("root");
    let later = sketch("later");
    let mut model = Model::default();
    model.sketches = vec![root.clone()];
    let ctx = cadmpeg_test_support::service_decode_context();
    let owner = define_member(&ctx, &mut model, "member").unwrap();
    model.sketches.push(later.clone());

    assert_eq!(define_member(&ctx, &mut model, "member").unwrap(), owner);
    assert_eq!(define_member(&ctx, &mut model, "member").unwrap(), owner);

    assert_eq!(model.product_definitions.len(), 1);
    let listed = &model.product_definitions[0].sketches;
    assert_eq!(listed.len(), 2);
    assert!(listed.contains(&root.id));
    assert!(listed.contains(&later.id));
}
