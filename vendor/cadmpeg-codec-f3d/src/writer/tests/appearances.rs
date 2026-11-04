// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native_mut, update_f3d_native};
use crate::test_support::protein_test::generated_prism_instance_properties;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::{
    f3d_with_smbh_and_instance_properties, f3d_with_smbh_and_protein,
    f3d_with_smbh_and_protein_guids,
};
use crate::F3dCodec;

#[test]
fn generated_source_less_writes_unassigned_protein_appearance() {
    use std::collections::BTreeMap;

    use cadmpeg_ir::appearance::Appearance;
    use cadmpeg_ir::ids::AppearanceId;
    use cadmpeg_ir::topology::Color;

    let visual_guid = "11111111-2222-3333-4444-555555555555";
    let appearance_id =
        AppearanceId::mint("generated:test:appearance#0").expect("identity grammar");
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.model.appearances = vec![Appearance {
        id: appearance_id.clone(),
        name: Some("Prism-Generated".into()),
        asset_guid: Some(visual_guid.into()),
        library_id: None,
        visual_guid: Some(visual_guid.into()),
        physical_token: Some("PrismMaterial-Generated".into()),
        schema: Some("GenericSchema".into()),
        category: Some("Plastic/Generated".into()),
        base_color: Some(Color::new(0.15, 0.35, 0.75, 1.0).expect("valid color")),
        properties: BTreeMap::from([
            (
                cadmpeg_core::nonblank_literal!("reflectivity_at_0deg"),
                cadmpeg_ir::scalar::FiniteReal::new(0.25).expect("finite scalar"),
            ),
            (
                cadmpeg_core::nonblank_literal!("refraction_index"),
                cadmpeg_ir::scalar::FiniteReal::new(1.5).expect("finite scalar"),
            ),
        ]),
        textures: Vec::new(),
    }];
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less Protein appearance encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less Protein appearance round trip");
    assert_eq!(round_trip.ir().model.appearances.len(), 1);
    let appearance = &round_trip.ir().model.appearances[0];
    assert_eq!(appearance.name.as_deref(), Some("Prism-Generated"));
    assert_eq!(appearance.visual_guid.as_deref(), Some(visual_guid));
    assert_eq!(appearance.schema.as_deref(), Some("GenericSchema"));
    assert_eq!(appearance.category.as_deref(), Some("Plastic/Generated"));
    assert_eq!(
        appearance.base_color,
        Some(Color::new(0.15, 0.35, 0.75, 1.0).expect("valid color"))
    );
    assert_eq!(
        appearance
            .properties
            .get("reflectivity_at_0deg")
            .map(|value| value.get()),
        Some(0.25)
    );
    assert_eq!(
        appearance
            .properties
            .get("refraction_index")
            .map(|value| value.get()),
        Some(1.5)
    );
    assert!(round_trip.ir().model.appearance_bindings.is_empty());
    assert!(crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, round_trip.ir())
            .expect("validate the native namespace")
    })
    .is_empty());
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_rejects_material_assignment_without_presentation_graph() {
    use crate::records::references::DesignMaterialAssignment;

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    f3d_native_mut(&mut source_less).design_material_assignments = vec![DesignMaterialAssignment {
        id: "f3d:generated:material-assignment#0".into(),
        asm_body_key: 42,
        asm_body_key_offset: 0,

        entity_suffix_offset: 0,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_985".to_owned())
            .expect("valid entity ID"),
        entity_id_offset: 0,
        visual_guid: crate::records::references::DesignVisualToken::try_from(
            "11111111-2222-3333-4444-555555555555".to_owned(),
        )
        .unwrap(),
        visual_guid_offset: 0,
        physical_token: Some(crate::records::identity::RecordedValue {
            value: "PrismMaterial-Generated".into(),
            offset: 0,
        }),
        visual_preset: None,
    }];

    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("an incomplete generated presentation graph must be refused");
    assert!(error
        .to_string()
        .contains("requires a typed body-presentation B-rep and scene graph"));
}

#[test]
fn generated_source_less_rejects_collapsed_visibility_body_bindings() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    source_less.model.bodies[0].visible = Some(false);
    let body = source_less.model.bodies[0].id.clone();
    f3d_native_mut(&mut source_less).body_visibilities = [985, 986]
        .into_iter()
        .enumerate()
        .map(|(ordinal, entity_suffix)| {
            crate::records::bodies::BodyVisibility::try_from(
                crate::records::bodies::BodyVisibilityWire {
                    id: format!("f3d:generated-{ordinal}:body-visibility#42"),
                    body: body.clone(),
                    stream: "generated/Design1/BulkStream.dat".into(),
                    byte_offset: 0,
                    asm_body_key_offset: 0,
                    asm_body_key: 42,
                    entity_suffix,
                    visible: false,
                },
            )
            .unwrap()
        })
        .collect();

    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("conflicting body-map rows must not collapse");
    assert!(error
        .to_string()
        .contains("conflicts with the body-map key/suffix bijection"));
}

#[test]
fn generated_f3d_rejects_material_assignment_divergence() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        native.design_material_assignments[0]
            .physical_token
            .as_mut()
            .expect("material field")
            .value = "PrismMaterial-019".into();
    });

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("divergent assignment and appearance must fail");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_partial_material_assignment_identity_edit() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated material decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    update_f3d_native(&mut edited, |native| {
        let assignment = &mut native.design_material_assignments[0];
        assignment.entity_id =
            crate::records::identity::DesignEntityId::try_from("0_986".to_owned())
                .expect("valid entity ID");
    });

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("a partial presentation-graph identity edit must fail");
    assert!(error.to_string().contains(
        "requires synchronized body-presentation, browser-node, B-rep, and scene graphs"
    ));
}

#[test]
fn generated_f3d_rejects_invalid_or_structural_protein_property_edits() {
    let source = f3d_with_smbh_and_protein(&synthetic_geometry_smbh());
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("generated Protein decode"),
    );

    let mut invalid = decoded.ir().clone();
    invalid.model.appearances[0].properties.insert(
        cadmpeg_core::nonblank_literal!("refraction_index"),
        cadmpeg_ir::scalar::FiniteReal::HALF,
    );
    let error = crate::test_support::plan_inherited_write(
        &invalid,
        decoded.source_fidelity(),
        &mut Vec::new(),
    )
    .expect_err("out-of-range refraction must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("refraction_index"))
    );

    let (mut structural, _, fidelity) = decoded.into_parts();
    structural.model.appearances[0].properties.insert(
        cadmpeg_core::nonblank_literal!("unserialized_property"),
        cadmpeg_ir::scalar::FiniteReal::HALF,
    );
    let error = crate::test_support::plan_inherited_write(&structural, &fidelity, &mut Vec::new())
        .expect_err("new Protein property must be refused");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message) if message.contains("unchanged property set"))
    );
}

#[test]
fn generated_f3d_routes_appearance_edits_across_multiple_protein_assets() {
    let source = f3d_with_smbh_and_protein_guids(
        &synthetic_geometry_smbh(),
        &[
            "11111111-2222-3333-4444-555555555555",
            "99999999-2222-3333-4444-555555555555",
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated multi-Protein decode");
    assert_eq!(decoded.ir().model.appearances.len(), 2);
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.appearances[0].base_color =
        Some(cadmpeg_ir::topology::Color::new(0.2, 0.3, 0.4, 1.0).expect("valid color"));
    edited.model.appearances[1].base_color =
        Some(cadmpeg_ir::topology::Color::new(0.6, 0.7, 0.8, 1.0).expect("valid color"));

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("multi-Protein appearance regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated multi-Protein decode");
    assert_eq!(round_trip.ir().model.appearances, edited.model.appearances);
}

#[test]
fn generated_f3d_rewrites_prism_scalar_properties() {
    let source = f3d_with_smbh_and_instance_properties(
        &synthetic_geometry_smbh(),
        &[
            generated_prism_instance_properties(
                "PrismOpaqueSchema",
                "11111111-2222-3333-4444-555555555555",
            ),
            generated_prism_instance_properties(
                "PrismTransparentSchema",
                "99999999-2222-3333-4444-555555555555",
            ),
        ],
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated Prism decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let opaque = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismOpaqueSchema"))
        .expect("opaque appearance");
    opaque.properties.insert(
        cadmpeg_core::nonblank_literal!("surface_roughness"),
        cadmpeg_ir::scalar::FiniteReal::new(0.75).expect("finite scalar"),
    );
    let transparent = edited
        .model
        .appearances
        .iter_mut()
        .find(|appearance| appearance.schema.as_deref() == Some("PrismTransparentSchema"))
        .expect("transparent appearance");
    transparent.properties.insert(
        cadmpeg_core::nonblank_literal!("refraction_index"),
        cadmpeg_ir::scalar::FiniteReal::new(2.25).expect("finite scalar"),
    );

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("Prism scalar regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated Prism decode");
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismOpaqueSchema")
            && appearance
                .properties
                .get("surface_roughness")
                .map(|value| value.get())
                == Some(0.75)
    }));
    assert!(round_trip.ir().model.appearances.iter().any(|appearance| {
        appearance.schema.as_deref() == Some("PrismTransparentSchema")
            && appearance
                .properties
                .get("refraction_index")
                .map(|value| value.get())
                == Some(2.25)
    }));
}

#[test]
fn synthesized_appearance_properties_report_each_unwritten_key() {
    use cadmpeg_ir::{appearance::Appearance, scalar::FiniteReal};
    for (schema, supported) in [
        (
            "GenericSchema",
            vec!["reflectivity_at_0deg", "refraction_index"],
        ),
        ("PrismOpaqueSchema", vec!["surface_roughness"]),
        ("PrismMetalSchema", vec!["surface_roughness"]),
        ("PrismTransparentSchema", vec!["refraction_index"]),
        ("PhysMatSchema", Vec::new()),
    ] {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let properties = [
            "reflectivity_at_0deg",
            "refraction_index",
            "surface_roughness",
            "extra_property",
        ]
        .into_iter()
        .map(|key| {
            (
                cadmpeg_core::text::NonBlankString::new(key).unwrap(),
                FiniteReal::ONE,
            )
        })
        .collect();
        ir.model.appearances.push(Appearance {
            id: "test:model:appearance#properties".try_into().unwrap(),
            name: Some("test".into()),
            asset_guid: Some("11111111-2222-3333-4444-555555555555".into()),
            visual_guid: None,
            physical_token: None,
            library_id: None,
            schema: Some(schema.into()),
            category: None,
            base_color: Some(cadmpeg_ir::topology::Color::new(0.2, 0.3, 0.4, 1.0).unwrap()),
            properties,
            textures: Vec::new(),
        });
        let plan = F3dCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        let loss = plan
            .report()
            .losses
            .iter()
            .find(|loss| {
                loss.code == crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.kind()
            })
            .unwrap();
        assert!(loss.message.starts_with(&format!(
            "{} appearance property record(s)",
            4 - supported.len()
        )));
        for key in [
            "reflectivity_at_0deg",
            "refraction_index",
            "surface_roughness",
            "extra_property",
        ] {
            assert_eq!(loss.message.contains(key), !supported.contains(&key));
        }
        ir.model.appearances[0]
            .properties
            .retain(|key, _| supported.contains(&key.as_str()));
        let plan = F3dCodec
            .plan(EncodeInput::new(&ir, None), TargetRequest::Inherit)
            .unwrap();
        assert!(!plan
            .report()
            .losses
            .iter()
            .any(|loss| loss.code
                == crate::loss::F3dLossCode::WriterAppearancePropertiesOmitted.kind()));
    }
}
