// SPDX-License-Identifier: Apache-2.0

use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein;
use crate::F3dCodec;
use cadmpeg_ir::codec::Codec;
use cadmpeg_ir::codec::DecodeOptions;
use std::io::Cursor;

#[test]
fn decode_transfers_generated_protein_appearance() {
    let f3d = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.appearances.len(), 1);
    let appearance = &result.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-001"));
    assert_eq!(
        appearance.visual_guid.as_deref(),
        Some("11111111-2222-3333-4444-555555555555")
    );
    let color = appearance.base_color.expect("decoded diffuse color");
    assert_eq!((color.r(), color.g(), color.b()), (0.1, 0.2, 0.3));
    assert_eq!(
        appearance.physical_token.as_deref(),
        Some("PrismMaterial-018")
    );
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(
        appearance.category.as_deref(),
        Some("Plastic/Thermoplastic")
    );
    assert_eq!(result.ir().model.appearance_bindings.len(), 1);
    assert_eq!(f3d_native(result.ir()).act_entities.len(), 1);
    assert_eq!(f3d_native(result.ir()).act_entities[0].record_index(), 7);
    assert_eq!(f3d_native(result.ir()).act_entities[0].entity_id(), "0_985");
    assert_eq!(f3d_native(result.ir()).act_guids.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).act_guids[0].guid.as_str(),
        "eeeeeeee-1111-2222-3333-ffffffffffff"
    );
    assert_eq!(f3d_native(result.ir()).act_registry_channels.len(), 2);
    assert_eq!(f3d_native(result.ir()).act_table_references.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).act_table_references[0].target_record,
        9
    );
    assert_eq!(
        f3d_native(result.ir()).act_registry_channels[0].name(),
        "Appearance"
    );
    assert_eq!(
        f3d_native(result.ir()).act_registry_channels[1].name(),
        "PhysicalMaterial"
    );
    assert!(f3d_native(result.ir()).act_entities[0].in_table());
    assert_eq!(f3d_native(result.ir()).act_component_links.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).act_component_links[0]
            .layout()
            .entity_id(),
        "0_3"
    );
    assert_eq!(
        f3d_native(result.ir()).act_component_links[0]
            .layout()
            .display_name(),
        "(Unsaved)"
    );
    assert_eq!(
        f3d_native(result.ir()).act_component_links[0]
            .references
            .instance_root,
        12
    );
    assert_eq!(
        serde_json::to_value(&f3d_native(result.ir()).act_component_links[0]).unwrap()
            ["tracked_entity_record"],
        3
    );
    assert_eq!(
        f3d_native(result.ir()).act_component_links[0]
            .references
            .components_root,
        7
    );
    assert_eq!(
        f3d_native(result.ir()).act_component_links[0].registry_flag,
        crate::records::act::ActRegistryFlag::On
    );
    assert_eq!(
        Some(f3d_native(result.ir()).act_entities[0].channel_class_tag()),
        Some("261")
    );
    assert_eq!(
        result.ir().model.appearance_bindings[0].appearance,
        appearance.id
    );
    assert!(matches!(
        &result.ir().model.appearance_bindings[0].target,
        cadmpeg_ir::appearance::AppearanceTarget::Body(body) if body == &result.ir().model.bodies[0].id
    ));
    assert_eq!(
        result.ir().model.appearance_bindings[0]
            .channels
            .get("Appearance")
            .map(String::as_str),
        Some("aaaaaaaa-1111-2222-3333-bbbbbbbbbbbb")
    );
    assert_eq!(
        result.ir().model.appearance_bindings[0]
            .source_entity_id
            .as_deref(),
        Some("0_985")
    );
    assert_eq!(
        result.ir().model.appearance_bindings[0]
            .object_type
            .as_deref(),
        Some("Body")
    );
    assert_eq!(f3d_native(result.ir()).construction_recipes.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).construction_recipes[0].kind,
        crate::records::recipes::ConstructionRecipeKind::Body
    );
    assert_eq!(
        f3d_native(result.ir()).construction_recipes[0]
            .design
            .as_ref()
            .map(|design| design.id.value.as_str()),
        Some("322")
    );
    assert_eq!(
        f3d_native(result.ir()).construction_recipes[0]
            .record_index
            .map(|index| index.value),
        Some(123)
    );
    assert_eq!(f3d_native(result.ir()).persistent_references.len(), 10);
    assert!(f3d_native(result.ir())
        .persistent_references
        .iter()
        .any(|reference| reference.value == 439));
    assert!(f3d_native(result.ir())
        .persistent_references
        .iter()
        .any(|reference| {
            reference.value == 440
                && reference.kind
                    == crate::records::references::PersistentReferenceKind::CurvePrimary
        }));
    assert_eq!(f3d_native(result.ir()).lost_edge_references.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).lost_edge_references[0]
            .class_tag
            .as_str(),
        "419"
    );
    assert_eq!(
        f3d_native(result.ir()).lost_edge_references[0].record_index,
        4645
    );
    assert_eq!(
        f3d_native(result.ir()).lost_edge_references[0].next_record_index,
        4646
    );
    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("source parametric edge reference(s) were marked")));
    assert_eq!(f3d_native(result.ir()).design_types.len(), 12);
    let sketch = f3d_native(result.ir())
        .design_types
        .iter()
        .find(|design_type| {
            design_type
                .entities
                .values()
                .any(|registered| *registered == 277)
        })
        .cloned()
        .unwrap();
    assert_eq!(
        sketch.entities.values().copied().collect::<Vec<_>>(),
        vec![277]
    );
    assert_eq!(sketch.version, 4);
    assert_eq!(f3d_native(result.ir()).design_entity_headers.len(), 2);
    let sketch_header = f3d_native(result.ir())
        .design_entity_headers
        .iter()
        .find(|header| header.entity_id.suffix() == 277)
        .cloned()
        .expect("generated sketch entity header");
    assert_eq!(sketch_header.entity_id.as_str(), "0_277");
    assert_eq!(sketch_header.class_tag.as_str(), "257");
    assert!(sketch_header.optional_slot_present);
    assert_eq!(
        sketch_header.module(),
        Some(crate::records::entity_header::DESIGN_MODULE_SKETCH)
    );
    assert_eq!(
        sketch_header
            .sketch_references()
            .and_then(|list| list.record_reference),
        Some(584)
    );
    assert_eq!(sketch_header.declared_reference_count(), Some(2));
    assert_eq!(
        sketch_header
            .reference_values()
            .copied()
            .collect::<Vec<_>>(),
        [33, 44]
    );
    assert_eq!(f3d_native(result.ir()).design_record_headers.len(), 6);
    let record_33 = f3d_native(result.ir())
        .design_record_headers
        .iter()
        .find(|record| record.record_index == 33)
        .cloned()
        .expect("record 33");
    assert_eq!(record_33.class_tag.as_str(), "259");
    assert_eq!(f3d_native(result.ir()).sketch_relations.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].member_indices(),
        vec![100, 200]
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].return_member_indices(),
        vec![200, 100]
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].owner_reference,
        277
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].constraint_kinds(),
        [crate::records::sketch_relations::SketchConstraintKind::Parallel]
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0].unknown_constraint_bits(),
        0
    );
    assert!(f3d_native(result.ir()).sketch_relations[1]
        .auxiliary_references()
        .is_empty());
    assert_eq!(
        f3d_native(result.ir()).sketch_relations[0]
            .raw_bytes()
            .len(),
        101
    );
    assert_eq!(f3d_native(result.ir()).sketch_points.len(), 5);
    let point_500 = f3d_native(result.ir())
        .sketch_points
        .iter()
        .find(|point| point.persistent_id() == Some(500))
        .cloned()
        .expect("point 500");
    assert_eq!(point_500.coordinates().u, 12.5);
    assert_eq!(point_500.coordinates().v, -25.0);
    let point_600 = f3d_native(result.ir())
        .sketch_points
        .iter()
        .find(|point| point.persistent_id() == Some(600))
        .cloned()
        .expect("point 600");
    assert_eq!(point_600.coordinates().u, -40.0);
    assert_eq!(point_600.entity_genesis(), Some(9));
    assert_eq!(f3d_native(result.ir()).sketch_curve_identities.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).sketch_curve_identities[0]
            .primary_id
            .get(),
        440
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_curve_identities[0].secondary_id,
        0
    );
    assert_eq!(
        f3d_native(result.ir()).sketch_curve_identities[1].entity_genesis,
        Some(10)
    );
    assert!(matches!(
        f3d_native(result.ir()).sketch_curve_identities[0].geometry,
        Some(crate::records::sketch_geometry::SketchCurveGeometry::Arc { radius, .. })
            if radius.get() == 30.0
    ));
    assert!(matches!(
        &f3d_native(result.ir()).sketch_curve_identities[1].geometry,
        Some(crate::records::sketch_geometry::SketchCurveGeometry::Nurbs {
            carrier_reference: Some(42),
            geometry,
            ..
        }) if geometry.degree() == 2
            && geometry.poles().weights().next().is_none()
            && geometry.poles().point_count() == 3
    ));
    assert_eq!(f3d_native(result.ir()).design_body_members.len(), 2);
    assert_eq!(
        f3d_native(result.ir()).design_body_members[0].entity_suffix,
        985
    );
    assert_eq!(
        f3d_native(result.ir()).design_body_members[1].entity_suffix,
        8422
    );
    assert!(f3d_native(result.ir())
        .design_body_members
        .iter()
        .all(|member| member.flags == 0));
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, result.ir()).expect("validate the native namespace")
    })
    .is_empty());
}
