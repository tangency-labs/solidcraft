// SPDX-License-Identifier: Apache-2.0

use std::fmt::Write as _;

mod graphics;
mod parameter;
mod parameter_owner;
mod placement_matrix;

#[test]
fn parameter_discriminator_preserves_wire_and_rejects_partial_location() {
    let prefix = r#"{"id":"parameter","byte_offset":0,"class_tag":"123","record_index":1"#;
    let suffix = r#","source_ordinal":0,"owner_record_index":2,"expression":"1","expression_offset":40,"source_kind":"Distance","source_kind_offset":60,"kind":"feature","name":"d1","name_offset":80,"evaluated_value":1.0,"evaluated_value_offset":90}"#;
    for fields in [
        "",
        ",\"family_discriminator\":0,\"family_discriminator_offset\":22",
    ] {
        let wire = format!("{prefix}{fields}{suffix}");
        let value: crate::records::parameters::DesignParameter =
            serde_json::from_str(&wire).expect("parameter frame");
        assert_eq!(serde_json::to_string(&value).expect("parameter wire"), wire);
    }
    for fields in [
        ",\"family_discriminator\":0",
        ",\"family_discriminator_offset\":22",
    ] {
        let error = serde_json::from_str::<crate::records::parameters::DesignParameter>(&format!(
            "{prefix}{fields}{suffix}"
        ))
        .expect_err("partial discriminator location");
        assert!(error.to_string().contains("family_discriminator"));
    }
}

#[test]
fn selection_secondary_identities_preserve_wire_and_reject_partial_locations() {
    let fields = r#""record_index":2,"byte_offset":0,"class_tag":"365","asset_id":"0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d","asset_id_offset":100,"context_id":"1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e","context_id_offset":150,"identity_record_index":5,"identity_record_offset":180,"primary_identity":183,"primary_identity_offset":209"#;
    let suffix = r#","next_record_index":6,"next_byte_offset":225}"#;
    for prefix in ["{", "{\"id\":\"operand\",\"scope_record_index\":1,\"group_record_index\":2,\"group_member_ordinal\":0,"] {
        for identities in ["", ",\"secondary_identity\":249,\"secondary_identity_offset\":217", ",\"secondary_identity\":249,\"secondary_identity_offset\":217,\"curve_secondary_identity\":77,\"curve_secondary_identity_offset\":201"] {
            let wire = format!("{prefix}{fields}{identities}{suffix}");
            let wire = if identities.is_empty() {
                wire.replace("\"primary_identity_offset\":209", "\"primary_identity_offset\":201")
                    .replace("\"next_byte_offset\":225", "\"next_byte_offset\":209")
            } else {
                wire
            };
            let encoded = if prefix == "{" {
                let value: crate::records::feature::hole::DesignHoleFaceSelection = serde_json::from_str(&wire).expect("hole selection");
                serde_json::to_string(&value).expect("hole selection wire")
            } else {
                let value: crate::records::topology::entity_selection::DesignEntitySelectionOperand = serde_json::from_str(&wire).expect("entity selection");
                serde_json::to_string(&value).expect("entity selection wire")
            };
            assert_eq!(encoded, wire);
        }
        for field in ["secondary_identity", "secondary_identity_offset", "curve_secondary_identity", "curve_secondary_identity_offset"] {
            let wire = format!("{prefix}{fields},\"{field}\":1{suffix}");
            let error = if prefix == "{" {
                serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(&wire).expect_err("partial hole selection identity").to_string()
            } else {
                serde_json::from_str::<crate::records::topology::entity_selection::DesignEntitySelectionOperand>(&wire).expect_err("partial entity selection identity").to_string()
            };
            assert!(error.contains(field));
        }
        let wire = format!("{prefix}{fields},\"curve_secondary_identity\":77,\"curve_secondary_identity_offset\":201{suffix}");
        let error = if prefix == "{" {
            serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(&wire).unwrap_err().to_string()
        } else {
            serde_json::from_str::<crate::records::topology::entity_selection::DesignEntitySelectionOperand>(&wire).unwrap_err().to_string()
        };
        assert!(error.contains("secondary_identity"));
        assert!(error.contains("curve_secondary_identity"));
        for guid in [
            "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
            "1b2c3d4e-5f6a-4b7c-8d9e-0f1a2b3c4d5e",
        ] {
            let wire = format!("{prefix}{fields}{suffix}").replace(guid, "not-a-guid");
            let error = if prefix == "{" {
                serde_json::from_str::<crate::records::feature::hole::DesignHoleFaceSelection>(&wire)
                    .expect_err("non-GUID hole selection identity")
                    .to_string()
            } else {
                serde_json::from_str::<crate::records::topology::entity_selection::DesignEntitySelectionOperand>(&wire)
                    .expect_err("non-GUID entity selection identity")
                    .to_string()
            };
            assert!(error.contains("GUID"), "{error}");
        }
    }
}

#[test]
fn material_assignment_preserves_located_and_authored_token_wire() {
    let prefix = r#"{"id":"material#0","asm_body_key":42,"asm_body_key_offset":10,"entity_suffix_offset":20,"entity_id":"0_985","entity_id_offset":30,"visual_guid":"11111111-2222-3333-4444-555555555555","visual_guid_offset":40"#;
    for field in ["physical_token", "visual_preset"] {
        for value in ["\"\"", "\"Prism-002\""] {
            for offset in [0, 50] {
                let mut wire = format!("{prefix},\"{field}\":{value}");
                write!(wire, ",\"{field}_offset\":{offset}").unwrap();
                wire.push('}');
                let parsed: crate::records::references::DesignMaterialAssignment =
                    serde_json::from_str(&wire).expect("material token");
                assert_eq!(serde_json::to_string(&parsed).expect("material wire"), wire);
            }
        }
        let wire = format!("{prefix},\"{field}\":\"Prism-002\"}}");
        let error =
            serde_json::from_str::<crate::records::references::DesignMaterialAssignment>(&wire)
                .expect_err("unlocated material token")
                .to_string();
        assert!(error.contains(field));
        assert!(error.contains(&format!("{field}_offset")));
        let wire = format!("{prefix},\"{field}_offset\":50}}");
        let error =
            serde_json::from_str::<crate::records::references::DesignMaterialAssignment>(&wire)
                .expect_err("orphan material offset")
                .to_string();
        assert!(error.contains(field));
        assert!(error.contains(&format!("{field}_offset")));
    }
    let wire = format!("{prefix}}}");
    let parsed: crate::records::references::DesignMaterialAssignment =
        serde_json::from_str(&wire).expect("absent tokens");
    assert_eq!(serde_json::to_string(&parsed).expect("material wire"), wire);
    let padded = wire.replace("0_985", "0_+00985");
    let parsed: crate::records::references::DesignMaterialAssignment =
        serde_json::from_str(&padded).expect("preserved numeric spelling");
    assert_eq!(serde_json::to_string(&parsed).unwrap(), padded);
}

#[test]
fn recipe_design_id_preserves_source_and_authored_wire() {
    let prefix = r#"{"id":"recipe#0","byte_offset":27,"record_index_offset":11,"kind":"body""#;
    let suffix = r#","recipe_index":0,"record_index":12}"#;
    for value in ["\"\"", "\"301\""] {
        for offset in [0, 4] {
            let mut wire = format!("{prefix},\"design_id\":{value}");
            write!(wire, ",\"design_id_offset\":{offset}").unwrap();
            wire.push_str(suffix);
            let parsed: crate::records::recipes::ConstructionRecipe =
                serde_json::from_str(&wire).expect("recipe id");
            assert_eq!(serde_json::to_string(&parsed).expect("recipe wire"), wire);
        }
    }
    let wire = format!("{prefix}{suffix}");
    let parsed: crate::records::recipes::ConstructionRecipe =
        serde_json::from_str(&wire).expect("body-less recipe");
    assert_eq!(serde_json::to_string(&parsed).expect("recipe wire"), wire);
    let wire = format!("{prefix},\"design_id_offset\":4{suffix}");
    let error = serde_json::from_str::<crate::records::recipes::ConstructionRecipe>(&wire)
        .expect_err("orphan design id offset")
        .to_string();
    assert!(error.contains("design_id_offset"));
    let wire = format!("{prefix},\"design_id\":\"301\"{suffix}");
    let error = serde_json::from_str::<crate::records::recipes::ConstructionRecipe>(&wire)
        .expect_err("unlocated design id")
        .to_string();
    assert!(error.contains("design_id"));
    assert!(error.contains("design_id_offset"));
}

#[test]
fn segment_base_guid_preserves_source_and_authored_wire() {
    let prefix = r#"{"id":"f3d:native/MetaStream.dat:design-type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4"#;
    let suffix = r#","version":1,"version_offset":80,"module":"Fusion","entity_ids":[1],"entity_id_offsets":[]}"#;
    for value in ["\"\"", "\"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\""] {
        for offset in [0, 44] {
            let mut wire = format!("{prefix},\"base_type_guid\":{value}");
            write!(wire, ",\"base_type_guid_offset\":{offset}").unwrap();
            wire.push_str(suffix);
            let parsed: crate::records::entity_header::SegmentType =
                serde_json::from_str(&wire).expect("base GUID");
            let expected = if value == "\"\"" {
                crate::records::entity_header::BaseTypeGuid::EmptyRoot { offset }
            } else {
                crate::records::entity_header::BaseTypeGuid::Guid {
                    value: value.trim_matches('"').to_owned().try_into().unwrap(),
                    offset,
                }
            };
            assert_eq!(parsed.base_type_guid, expected);
            assert_eq!(serde_json::to_string(&parsed).expect("segment wire"), wire);
        }
    }
    let wire = format!("{prefix}{suffix}");
    let parsed: crate::records::entity_header::SegmentType =
        serde_json::from_str(&wire).expect("root type");
    assert_eq!(
        parsed.base_type_guid,
        crate::records::entity_header::BaseTypeGuid::Absent
    );
    assert_eq!(serde_json::to_string(&parsed).expect("segment wire"), wire);
    let wire = format!("{prefix},\"base_type_guid_offset\":44{suffix}");
    let error = serde_json::from_str::<crate::records::entity_header::SegmentType>(&wire)
        .expect_err("orphan base GUID offset")
        .to_string();
    assert!(error.contains("base_type_guid_offset"));
    let wire =
        format!("{prefix},\"base_type_guid\":\"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee\"{suffix}");
    let error = serde_json::from_str::<crate::records::entity_header::SegmentType>(&wire)
        .expect_err("unlocated base GUID")
        .to_string();
    assert!(error.contains("base_type_guid"), "{error}");
    assert!(error.contains("base_type_guid_offset"), "{error}");
}

#[test]
fn parameter_unit_preserves_source_and_authored_wire() {
    let prefix = r#"{"id":"parameter","byte_offset":0,"class_tag":"123","record_index":1,"source_ordinal":0,"owner_record_index":2,"expression":"1","expression_offset":40,"source_kind":"Distance","source_kind_offset":60,"kind":"feature""#;
    let suffix =
        r#","name":"d1","name_offset":80,"evaluated_value":1.0,"evaluated_value_offset":90}"#;
    let wire = format!("{prefix},\"unit\":\"mm\",\"unit_offset\":70{suffix}");
    let parsed: crate::records::parameters::DesignParameter =
        serde_json::from_str(&wire).expect("parameter unit");
    assert_eq!(
        serde_json::to_string(&parsed).expect("parameter wire"),
        wire
    );
    let wire = format!("{prefix}{suffix}");
    let parsed: crate::records::parameters::DesignParameter =
        serde_json::from_str(&wire).expect("dimensionless parameter");
    assert_eq!(
        serde_json::to_string(&parsed).expect("parameter wire"),
        wire
    );
    let wire = format!("{prefix},\"unit_offset\":70{suffix}");
    let error = serde_json::from_str::<crate::records::parameters::DesignParameter>(&wire)
        .expect_err("orphan unit offset")
        .to_string();
    assert!(error.contains("unit_offset"));
}

#[test]
fn timeline_items_preserve_wire_and_reject_unequal_offsets() {
    for offsets in ["[245,256]", "[245,278]"] {
        let wire = format!(
            r#"{{"id":"f3d:Design/BulkStream.dat:design-feature-timeline#200","byte_offset":200,"class_tag":"256","record_index":35,"source_ordinal":0,"frame_length":100,"context_record_index":17,"context_record_index_offset":220,"item_count_offset":240,"item_record_indices":[101,102],"item_record_index_offsets":{offsets}}}"#
        );
        let timeline: crate::records::entity_header::DesignFeatureTimeline =
            serde_json::from_str(&wire).expect("timeline items");
        assert_eq!(
            serde_json::to_string(&timeline).expect("timeline wire"),
            wire
        );
        for invalid_offsets in ["[]", "[245]", "[245,256,267]"] {
            let invalid = wire.replace(
                &format!("\"item_record_index_offsets\":{offsets}"),
                &format!("\"item_record_index_offsets\":{invalid_offsets}"),
            );
            let error =
                serde_json::from_str::<crate::records::entity_header::DesignFeatureTimeline>(
                    &invalid,
                )
                .expect_err("unequal timeline arrays")
                .to_string();
            assert!(error.contains("item_record_indices"));
            assert!(error.contains("item_record_index_offsets"));
        }
    }
}

#[test]
fn annotation_return_members_preserve_wire_and_reject_unequal_offsets() {
    let wire = r#"{"id":"annotation","governing_companion_record_index":2,"byte_offset":100,"class_tag":"256","record_index":3,"frame_length":120,"operands":[{"geometry_record_index":10,"geometry_reference_offset":125,"role":1,"role_offset":135},{"geometry_record_index":11,"geometry_reference_offset":140,"role":2,"role_offset":150}],"entity_genesis":0,"annotation_bytes":"","annotation_byte_offset":211,"governing_owner_record_index":4,"governing_owner_reference_offset":212,"return_members":[10,11],"return_member_offsets":[227,238],"paired_class_tag":"259","paired_byte_offset":220,"owner_reference":5,"owner_reference_offset":240}"#;
    let frame: crate::records::dimensions::DesignDimensionAnnotationFrame =
        serde_json::from_str(wire).expect("annotation return members");
    assert_eq!(
        serde_json::to_string(&frame).expect("annotation wire"),
        wire
    );
    for offsets in ["[]", "[227]", "[227,238,249]"] {
        let invalid = wire.replace(
            "\"return_member_offsets\":[227,238]",
            &format!("\"return_member_offsets\":{offsets}"),
        );
        let error = serde_json::from_str::<
            crate::records::dimensions::DesignDimensionAnnotationFrame,
        >(&invalid)
        .expect_err("unequal return arrays")
        .to_string();
        assert!(error.contains("return_members"));
        assert!(error.contains("return_member_offsets"));
    }
    let invalid = wire.replace("\"return_members\":[10,11]", "\"return_members\":[10,0]");
    let error = serde_json::from_str::<crate::records::dimensions::DesignDimensionAnnotationFrame>(
        &invalid,
    )
    .unwrap_err()
    .to_string();
    assert!(error.contains("return_members"));
}

#[test]
fn dimension_locus_rows_preserve_return_order_and_derive_state_views() {
    for (state, kinds, unknown) in [
        (0, r#"["coincident"]"#, 0),
        (32, r#"["perpendicular"]"#, 0),
        (16384, "[]", 16384),
    ] {
        let wire = format!(
            r#"{{"id":"locus-group","companion_record_index":2,"byte_offset":100,"class_tag":"256","record_index":3,"frame_length":150,"loci":[{{"geometry_record_index":11,"geometry_reference_offset":125,"role":0,"role_offset":135}},{{"geometry_record_index":10,"geometry_reference_offset":140,"role":0,"role_offset":150}}],"owner_reference":5,"owner_reference_offset":156,"owner_role":0,"owner_role_offset":166,"state":{state},"state_offset":170,"constraint_kinds":{kinds},"unknown_constraint_bits":{unknown},"return_members":[10,11],"return_member_offsets":[179,190],"next_class_tag":"259","next_record_index":4,"next_byte_offset":201}}"#
        );
        let group: crate::records::dimensions::DesignDimensionLocusGroup =
            serde_json::from_str(&wire).expect("locus group rows");
        assert_eq!(
            serde_json::to_string(&group).expect("locus group wire"),
            wire
        );
        assert_eq!(group.loci[0].geometry_record_index, 11);
        assert_eq!(group.loci[0].returned.value, 10);
        let value: serde_json::Value = serde_json::from_str(&wire).expect("locus JSON");
        for field in ["loci", "return_members", "return_member_offsets"] {
            let mut invalid = value.clone();
            invalid[field].as_array_mut().expect("locus array").pop();
            let error = serde_json::from_value::<
                crate::records::dimensions::DesignDimensionLocusGroup,
            >(invalid)
            .expect_err("unequal locus arrays")
            .to_string();
            assert!(error.contains(field), "{field}: {error}");
        }
        for (field, replacement) in [
            ("constraint_kinds", serde_json::json!(["parallel"])),
            ("unknown_constraint_bits", serde_json::json!(1)),
        ] {
            let mut invalid = value.clone();
            invalid[field] = replacement;
            let error = serde_json::from_value::<
                crate::records::dimensions::DesignDimensionLocusGroup,
            >(invalid)
            .expect_err("inconsistent state projection")
            .to_string();
            assert!(error.contains(field));
        }
    }
}

#[test]
fn segment_entity_runs_preserve_authored_and_located_wire() {
    for (ids, offsets) in [
        ("[]", "[]"),
        ("[10,11]", "[]"),
        ("[10,11]", "[0,0]"),
        ("[10,11]", "[80,88]"),
    ] {
        let wire = format!(
            r#"{{"id":"f3d:native/MetaStream.dat:design-type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4,"version":1,"version_offset":44,"module":"Fusion","entity_ids":{ids},"entity_id_offsets":{offsets}}}"#
        );
        let entry: crate::records::entity_header::SegmentType =
            serde_json::from_str(&wire).expect("type entity run");
        assert_eq!(serde_json::to_string(&entry).expect("type wire"), wire);
    }
    for (ids, offsets) in [("[]", "[80]"), ("[10,11]", "[80]"), ("[10]", "[80,88]")] {
        let wire = format!(
            r#"{{"id":"f3d:native/MetaStream.dat:design-type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4,"version":1,"version_offset":44,"module":"Fusion","entity_ids":{ids},"entity_id_offsets":{offsets}}}"#
        );
        let error = serde_json::from_str::<crate::records::entity_header::SegmentType>(&wire)
            .expect_err("partial entity locations")
            .to_string();
        assert!(error.contains("entity_ids/entity_id_offsets"));
    }
}

#[test]
fn entity_header_runs_derive_counts_and_preserve_absent_reference_slots() {
    let prefix = r#"{"id":"header","byte_offset":0,"entity_id":"0_1","class_tag":"256","optional_slot_present":false,"module":"MSketch""#;
    for (fields, references, offsets, members) in [
        ("", "[]", "[]", ""),
        (
            r#","record_reference_offset":40,"declared_reference_count":0"#,
            "[]",
            "[]",
            "",
        ),
        (
            r#","record_reference":33,"record_reference_offset":40,"declared_reference_count":2"#,
            "[34,35]",
            "[50,61]",
            r#","member_indices":[11],"member_offsets":[0]"#,
        ),
        ("", "[]", "[]", r#","member_indices":[11]"#),
    ] {
        let wire = format!("{prefix}{fields},\"reference_indices\":{references},\"reference_offsets\":{offsets}{members}}}");
        let header: crate::records::entity_header::DesignEntityHeader =
            serde_json::from_str(&wire).expect("header runs");
        assert_eq!(serde_json::to_string(&header).expect("header wire"), wire);
    }
    for suffix in [
        r#","declared_reference_count":1,"reference_indices":[],"reference_offsets":[]}"#,
        r#","reference_indices":[34,35],"reference_offsets":[50]}"#,
        r#","reference_indices":[],"reference_offsets":[50]}"#,
        r#","reference_indices":[],"reference_offsets":[],"member_indices":[11,12],"member_offsets":[0]}"#,
        r#","reference_indices":[],"reference_offsets":[],"member_offsets":[0]}"#,
    ] {
        assert!(
            serde_json::from_str::<crate::records::entity_header::DesignEntityHeader>(&format!(
                "{prefix}{suffix}"
            ))
            .is_err()
        );
    }
}

#[test]
fn sketch_auxiliary_rows_preserve_absent_and_complete_offset_runs() {
    let base = r#"{"id":"relation","record_index":1,"class_tag":"000","byte_offset":0,"state_offset":0,"owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"auxiliary_reference_offsets":[],"members":[],"resolved_members":[],"member_offsets":[],"owner_reference_offset":0,"state":0,"constraint_kinds":["coincident"],"unknown_constraint_bits":0,"member_relation_ordinals":[],"entity_genesis":null,"pattern":null,"return_members":[],"resolved_return_members":[],"return_member_offsets":[],"raw_bytes":"0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"}"#;
    for (values, offsets) in [
        (vec![], vec![]),
        (vec![2], vec![0]),
        (vec![2, 3], vec![0, 10]),
    ] {
        let expected = base
            .replace(
                "\"auxiliary_references\":[]",
                &format!(
                    "\"auxiliary_references\":{}",
                    serde_json::to_string(&values).unwrap()
                ),
            )
            .replace(
                "\"auxiliary_reference_offsets\":[]",
                &format!(
                    "\"auxiliary_reference_offsets\":{}",
                    serde_json::to_string(&offsets).unwrap()
                ),
            );
        let relation: crate::records::sketch_relations::SketchRelation =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(
            relation
                .auxiliary_references()
                .values()
                .copied()
                .collect::<Vec<_>>(),
            values
        );
        assert_eq!(
            relation
                .auxiliary_references()
                .offsets()
                .copied()
                .collect::<Vec<_>>(),
            offsets
        );
        assert_eq!(serde_json::to_string(&relation).unwrap(), expected);
    }
    for (values, offsets) in [
        (vec![2], vec![]),
        (vec![], vec![10]),
        (vec![2], vec![10, 20]),
        (vec![2, 3], vec![10]),
    ] {
        let mut wire: serde_json::Value = serde_json::from_str(base).unwrap();
        wire["auxiliary_references"] = serde_json::json!(values);
        wire["auxiliary_reference_offsets"] = serde_json::json!(offsets);
        assert!(
            serde_json::from_value::<crate::records::sketch_relations::SketchRelation>(wire)
                .unwrap_err()
                .to_string()
                .contains("auxiliary_reference")
        );
    }
}

#[test]
fn sketch_nurbs_poles_preserve_wire_and_reject_partial_weights() {
    let source_refused = r#"{"kind":"nurbs","subtype_class_tag":"302","subtype_record_index":7,"degree":1,"fit_tolerance":0.125,"scalar_width":4,"knots":[0.0,0.0,1.0,1.0],"weights":[],"control_points":[{"x":2.0,"y":3.0,"z":4.0},{"x":5.0,"y":6.0,"z":7.0}]}"#;
    assert!(
        serde_json::from_str::<crate::records::sketch_geometry::SketchCurveGeometry>(
            source_refused
        )
        .unwrap_err()
        .to_string()
        .contains("scalar_width")
    );
    let base = source_refused.replace("\"scalar_width\":4", "\"scalar_width\":8");
    for weights in ["[]", "[1.0,0.5]"] {
        let expected = base.replace("\"weights\":[]", &format!("\"weights\":{weights}"));
        let curve: crate::records::sketch_geometry::SketchCurveGeometry =
            serde_json::from_str(&expected).unwrap();
        assert_eq!(serde_json::to_string(&curve).unwrap(), expected);
    }
    for weights in ["[1.0]", "[1.0,0.5,1.0]"] {
        let wire = base.replace("\"weights\":[]", &format!("\"weights\":{weights}"));
        assert!(
            serde_json::from_str::<crate::records::sketch_geometry::SketchCurveGeometry>(&wire)
                .unwrap_err()
                .to_string()
                .contains("weights")
        );
    }
    let empty = crate::records::sketch_geometry::SketchCurveGeometry::nurbs_from_parts(
        None,
        crate::records::references::DesignClassTag::try_from("302".to_owned()).unwrap(),
        7,
        crate::records::sketch_geometry::SketchNurbsGeometry::from_parts(
            1,
            0.125,
            8,
            vec![0.0, 1.0],
            crate::records::sketch_geometry::SketchNurbsPoles::Rational(Vec::new()),
        )
        .unwrap(),
    );
    let wire = serde_json::to_string(&empty).unwrap();
    assert_eq!(
        serde_json::from_str::<crate::records::sketch_geometry::SketchCurveGeometry>(&wire)
            .unwrap(),
        empty
    );
}

#[test]
fn native_sketch_nurbs_refuses_decreasing_knots_and_negative_weight() {
    let wire = serde_json::json!({
        "kind": "nurbs", "subtype_class_tag": "302", "subtype_record_index": 7,
        "degree": 1, "fit_tolerance": 0.125, "scalar_width": 8,
        "knots": [0.0, 0.0, 1.0, 1.0], "weights": [1.0, 1.0],
        "control_points": [
            {"x": 2.0, "y": 3.0, "z": 4.0},
            {"x": 5.0, "y": 6.0, "z": 7.0}
        ]
    });
    let mut decreasing = wire.clone();
    decreasing["knots"] = serde_json::json!([1.0, 0.0, 1.0, 1.0]);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(decreasing)
            .unwrap_err()
            .to_string()
            .contains("nondecreasing")
    );
    let mut negative_weight = wire;
    negative_weight["weights"] = serde_json::json!([-1.0, 1.0]);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(
            negative_weight
        )
        .unwrap_err()
        .to_string()
        .contains("positive and finite")
    );
}

#[test]
fn native_sketch_nurbs_refuses_wrong_cardinality_and_negative_fit_tolerance() {
    let wire = serde_json::json!({
        "kind": "nurbs", "subtype_class_tag": "302", "subtype_record_index": 7,
        "degree": 1, "fit_tolerance": 0.125, "scalar_width": 8,
        "knots": [0.0, 0.0, 1.0, 1.0], "weights": [],
        "control_points": [
            {"x": 2.0, "y": 3.0, "z": 4.0},
            {"x": 5.0, "y": 6.0, "z": 7.0}
        ]
    });
    let mut wrong_count = wire.clone();
    wrong_count["degree"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(wrong_count)
            .unwrap_err()
            .to_string()
            .contains("knot count")
    );
    let mut negative_fit = wire;
    negative_fit["fit_tolerance"] = serde_json::json!(-0.125);
    assert!(
        serde_json::from_value::<crate::records::sketch_geometry::SketchCurveGeometry>(
            negative_fit
        )
        .unwrap_err()
        .to_string()
        .contains("fit_tolerance")
    );
}

#[test]
fn parameter_source_preserves_wire_and_rejects_an_owned_user_parameter() {
    let prefix = r#"{"id":"parameter","byte_offset":0,"class_tag":"123","record_index":1"#;
    let tail =
        r#","name":"d1","name_offset":80,"evaluated_value":1.0,"evaluated_value_offset":90}"#;
    for (source_kind, kind, owner) in [
        ("User Parameter", "user", ""),
        (
            "Linear Dimension-2",
            "dimension",
            ",\"owner_record_index\":2",
        ),
        ("Distance", "feature", ",\"owner_record_index\":2"),
    ] {
        for discriminator in [0, 3, 4, 5, 6] {
            let wire = format!("{prefix},\"family_discriminator\":{discriminator},\"family_discriminator_offset\":22,\"source_ordinal\":0{owner},\"expression\":\"1\",\"expression_offset\":40,\"source_kind\":\"{source_kind}\",\"source_kind_offset\":60,\"kind\":\"{kind}\"{tail}");
            let parameter: crate::records::parameters::DesignParameter =
                serde_json::from_str(&wire).unwrap();
            assert_eq!(parameter.source_kind(), source_kind);
            assert_eq!(serde_json::to_string(&parameter).unwrap(), wire);
            let value: serde_json::Value = serde_json::from_str(&wire).unwrap();
            let mut wrong_kind = value.clone();
            wrong_kind["kind"] = serde_json::json!(if kind == "user" { "feature" } else { "user" });
            assert!(
                serde_json::from_value::<crate::records::parameters::DesignParameter>(wrong_kind)
                    .unwrap_err()
                    .to_string()
                    .contains("kind")
            );
            let mut changed_owner = value.clone();
            if kind == "user" {
                changed_owner["owner_record_index"] = serde_json::json!(2);
                assert!(
                    serde_json::from_value::<crate::records::parameters::DesignParameter>(
                        changed_owner
                    )
                    .unwrap_err()
                    .to_string()
                    .contains("owner_record_index")
                );
            } else {
                changed_owner
                    .as_object_mut()
                    .unwrap()
                    .remove("owner_record_index");
                let ownerless: crate::records::parameters::DesignParameter =
                    serde_json::from_value(changed_owner.clone()).unwrap();
                assert_eq!(ownerless.owner_record_index(), None);
                assert_eq!(serde_json::to_value(&ownerless).unwrap(), changed_owner);
            }
            let mut invalid_discriminator = value.clone();
            invalid_discriminator["family_discriminator"] = serde_json::json!(7);
            assert!(
                serde_json::from_value::<crate::records::parameters::DesignParameter>(
                    invalid_discriminator
                )
                .unwrap_err()
                .to_string()
                .contains("family_discriminator")
            );
            let mut no_discriminator = value;
            no_discriminator
                .as_object_mut()
                .unwrap()
                .remove("family_discriminator");
            no_discriminator
                .as_object_mut()
                .unwrap()
                .remove("family_discriminator_offset");
            // Typed compact document parameters and compact owned parameters
            // both store no family discriminator.
            let parameter: crate::records::parameters::DesignParameter =
                serde_json::from_value(no_discriminator.clone()).unwrap();
            assert_eq!(serde_json::to_value(parameter).unwrap(), no_discriminator);
        }
    }
    assert!(
        crate::records::parameters::DesignParameterSource::new(String::new(), Some(2), None)
            .unwrap_err()
            .contains("source_kind")
    );
}

#[test]
fn a_construction_recipe_record_index_and_its_offset_occur_together() {
    let prefix = r#"{"id":"recipe","byte_offset":80"#;
    let suffix = r#","kind":"body","recipe_index":0"#;
    let complete = format!(r#"{prefix},"record_index_offset":64{suffix},"record_index":7}}"#);
    let recipe: crate::records::recipes::ConstructionRecipe =
        serde_json::from_str(&complete).expect("complete record index");
    assert_eq!(
        serde_json::to_string(&recipe).expect("recipe wire"),
        complete
    );

    let absent = format!("{prefix}{suffix}}}");
    let recipe: crate::records::recipes::ConstructionRecipe =
        serde_json::from_str(&absent).expect("recipe stating no record index");
    assert_eq!(recipe.record_index, None);
    assert_eq!(serde_json::to_string(&recipe).expect("recipe wire"), absent);

    for orphan in [
        format!(r#"{prefix}{suffix},"record_index":7}}"#),
        format!(r#"{prefix},"record_index_offset":64{suffix}}}"#),
    ] {
        let error = serde_json::from_str::<crate::records::recipes::ConstructionRecipe>(&orphan)
            .expect_err("orphan record index")
            .to_string();
        assert!(error.contains("record_index"), "{error}");
    }
}

#[test]
fn construction_recipe_design_preserves_wire_and_rejects_orphan_selector() {
    let prefix = r#"{"id":"recipe","byte_offset":80,"record_index_offset":64,"kind":"body"#;
    let suffix = r#","recipe_index":0,"record_index":7}"#;
    for fields in [
        "",
        ",\"design_id\":\"301\",\"design_id_offset\":12",
        ",\"design_id\":\"301\",\"design_id_offset\":12,\"design_selector\":{\"value\":2,\"byte_offset\":15}",
    ] {
        let wire = format!("{prefix}\"{fields}{suffix}");
        let recipe: crate::records::recipes::ConstructionRecipe = serde_json::from_str(&wire).unwrap();
        assert_eq!(serde_json::to_string(&recipe).unwrap(), wire);
    }
    let wire = format!("{prefix}\",\"design_selector\":{{\"value\":2,\"byte_offset\":15}}{suffix}");
    let error = serde_json::from_str::<crate::records::recipes::ConstructionRecipe>(&wire)
        .unwrap_err()
        .to_string();
    assert!(error.contains("design_id"));
    assert!(error.contains("design_selector"));
    for fields in [
        ",\"design_id\":\"301\"",
        ",\"design_id\":\"301\",\"design_selector\":{\"value\":2,\"byte_offset\":0}",
    ] {
        let wire = format!("{prefix}\"{fields}{suffix}");
        let error = serde_json::from_str::<crate::records::recipes::ConstructionRecipe>(&wire)
            .unwrap_err()
            .to_string();
        assert!(error.contains("design_id_offset"), "{error}");
    }
}

#[test]
fn companion_payload_absence_is_distinct_from_an_empty_payload() {
    let prefix = r#"{"id":"companion","byte_offset":0,"class_tag":"123","record_index":3,"owner_record_index":2,"timestamp_micros":1,"timestamp_micros_offset":42"#;
    let unbound = format!("{prefix}}}");
    let empty = format!("{prefix},\"payload_byte_offset\":58,\"payload_byte_length\":0}}");

    let companion: crate::records::parameters::DesignParameterCompanion =
        serde_json::from_str(&unbound).unwrap();
    assert!(companion.payload().is_none());
    assert_eq!(serde_json::to_string(&companion).unwrap(), unbound);

    let companion: crate::records::parameters::DesignParameterCompanion =
        serde_json::from_str(&empty).unwrap();
    let payload = companion.payload().expect("bound payload");
    assert_eq!(payload.byte_offset(), 58);
    assert_eq!(payload.byte_length(), 0);
    assert_eq!(serde_json::to_string(&companion).unwrap(), empty);

    for (invalid, field) in [
        (
            format!("{prefix},\"payload_byte_offset\":58}}"),
            "payload_byte_length",
        ),
        (
            format!("{prefix},\"payload_byte_length\":4}}"),
            "payload_byte_offset",
        ),
        (
            format!("{prefix},\"owned_recipe_ids\":[\"recipe\"]}}"),
            "owned_recipe_ids",
        ),
    ] {
        let error =
            serde_json::from_str::<crate::records::parameters::DesignParameterCompanion>(&invalid)
                .unwrap_err()
                .to_string();
        assert!(error.contains(field), "{error}");
    }
}

#[test]
fn companion_timestamp_preserves_wire_and_rejects_zero() {
    let wire = r#"{"id":"companion","byte_offset":0,"class_tag":"123","record_index":3,"owner_record_index":2,"timestamp_micros":1,"timestamp_micros_offset":42,"payload_byte_offset":58,"payload_byte_length":0}"#;
    let companion: crate::records::parameters::DesignParameterCompanion =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&companion).unwrap(), wire);
    let legacy = wire
        .replace("timestamp_micros_offset", "opaque_value_offset")
        .replace("timestamp_micros", "opaque_value");
    let companion: crate::records::parameters::DesignParameterCompanion =
        serde_json::from_str(&legacy).unwrap();
    assert_eq!(serde_json::to_string(&companion).unwrap(), wire);
    for invalid in [
        wire.replace("\"timestamp_micros\":1", "\"timestamp_micros\":0"),
        legacy.replace("\"opaque_value\":1", "\"opaque_value\":0"),
    ] {
        let error =
            serde_json::from_str::<crate::records::parameters::DesignParameterCompanion>(&invalid)
                .unwrap_err()
                .to_string();
        assert!(error.contains("timestamp_micros"));
    }
}

#[test]
fn dimension_operands_preserve_null_and_required_index_wires() {
    for index in [0, 7, u32::MAX] {
        let wire = format!(
            r#"{{"geometry_record_index":{index},"geometry_reference_offset":25,"role":3,"role_offset":35}}"#
        );
        let operand: crate::records::dimensions::DesignDimensionAnnotationOperand =
            serde_json::from_str(&wire).unwrap();
        assert_eq!(
            operand.geometry_record_index,
            std::num::NonZeroU32::new(index)
        );
        assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
        if index == 0 {
            let error = serde_json::from_str::<
                crate::records::dimensions::DesignDimensionPresentationOperand,
            >(&wire)
            .unwrap_err()
            .to_string();
            assert!(error.contains("geometry_record_index"));
        } else {
            let operand: crate::records::dimensions::DesignDimensionPresentationOperand =
                serde_json::from_str(&wire).unwrap();
            assert_eq!(operand.geometry_record_index.get(), index);
            assert_eq!(serde_json::to_string(&operand).unwrap(), wire);
        }
    }
}

#[test]
fn sketch_entity_identity_derives_suffix_without_changing_its_spelling() {
    for (name, suffix) in [
        ("Sketch_0", 0),
        ("Sketch_00017", 17),
        ("Sketch_+0017", 17),
        ("module_Sketch_42", 42),
        ("_18446744073709551615", u64::MAX),
    ] {
        let id = crate::records::identity::DesignEntityId::try_from(name.to_owned())
            .expect("valid entity ID");
        assert_eq!(id.as_str(), name);
        assert_eq!(id.suffix(), suffix);
        let wire = serde_json::json!({"entity_id": name, "entity_suffix": suffix, "entity_reference_offset": 20});
        let binding: crate::records::feature::scope::DesignSketchEntityBinding =
            serde_json::from_value(wire.clone()).expect("matching suffix");
        assert_eq!(serde_json::to_value(binding).unwrap(), wire);
        let mut mismatch = wire;
        mismatch["entity_suffix"] = (suffix ^ 1).into();
        let error = serde_json::from_value::<
            crate::records::feature::scope::DesignSketchEntityBinding,
        >(mismatch)
        .expect_err("contradictory suffix");
        assert!(error.to_string().contains("entity_suffix"));
    }
    for name in [
        "",
        "Sketch",
        "Sketch_",
        "Sketch_-1",
        "Sketch_++1",
        "Sketch_18446744073709551616",
    ] {
        assert!(
            crate::records::identity::DesignEntityId::try_from(name.to_owned())
                .expect_err("invalid entity ID")
                .contains("entity_id")
        );
    }
    let wire = serde_json::json!({
        "scope_reference_ordinal": 0, "record_index": 1, "byte_offset": 10,
        "class_tag": "300", "asset_id": "0a1b2c3d-4e5f-4a6b-8c7d-9e0f1a2b3c4d",
        "asset_id_offset": 20,
        "entity_id": "Sketch_00017", "entity_suffix": 17, "entity_reference_offset": 30,
        "paired_class_tag": "301", "paired_byte_offset": 40
    });
    let profile: crate::records::topology::sketch_profile::DesignSketchProfileOperand =
        serde_json::from_value(wire.clone()).expect("valid profile ID");
    assert_eq!(serde_json::to_value(profile).unwrap(), wire);
    let mut mismatch = wire.clone();
    mismatch["entity_suffix"] = 18.into();
    assert!(serde_json::from_value::<
        crate::records::topology::sketch_profile::DesignSketchProfileOperand,
    >(mismatch)
    .expect_err("mismatched profile suffix")
    .to_string()
    .contains("entity_suffix"));
    let mut invalid_asset = wire;
    invalid_asset["asset_id"] = "asset".into();
    assert!(serde_json::from_value::<
        crate::records::topology::sketch_profile::DesignSketchProfileOperand,
    >(invalid_asset)
    .expect_err("non-GUID profile asset identity")
    .to_string()
    .contains("GUID"));
}

#[test]
fn sketch_placement_preserves_identity_wire() {
    let wire = serde_json::json!({
        "id": "placement", "entity_id": "Sketch_+0007",
        "byte_offset": 10, "class_tag": "300", "record_index": 1,
        "frame_length": 201,
        "transform": [[1.0, 0.0, 0.0, 0.0], [0.0, 1.0, 0.0, 0.0],
                      [0.0, 0.0, 1.0, 0.0], [0.0, 0.0, 0.0, 1.0]],
        "paired_class_tag": "301", "paired_byte_offset": 211
    });
    let placement: crate::records::sketch_placement::DesignSketchPlacement =
        serde_json::from_value(wire.clone()).expect("matching identity");
    assert_eq!(placement.entity_id.suffix(), 7);
    assert_eq!(serde_json::to_value(placement).unwrap(), wire);
    for field in ["class_tag", "paired_class_tag"] {
        for invalid in ["", "12", "1234", "1a3", "１２３"] {
            let mut invalid_wire = wire.clone();
            invalid_wire[field] = invalid.into();
            assert!(serde_json::from_value::<
                crate::records::sketch_placement::DesignSketchPlacement,
            >(invalid_wire)
            .unwrap_err()
            .to_string()
            .contains(field));
        }
    }
}

#[test]
fn entity_header_identity_preserves_wire_spelling() {
    let wire = r#"{"id":"header","byte_offset":0,"entity_id":"0_+001","class_tag":"256","optional_slot_present":false,"reference_indices":[],"reference_offsets":[]}"#;
    let header: crate::records::entity_header::DesignEntityHeader =
        serde_json::from_str(wire).expect("matching identity");
    assert_eq!(header.entity_id.suffix(), 1);
    assert_eq!(serde_json::to_string(&header).unwrap(), wire);
    for class_tag in ["", "25", "25x", "2560", "٢٥٦"] {
        let mut invalid = serde_json::to_value(&header).unwrap();
        invalid["class_tag"] = class_tag.into();
        assert!(
            serde_json::from_value::<crate::records::entity_header::DesignEntityHeader>(invalid)
                .unwrap_err()
                .to_string()
                .contains("class_tag")
        );
    }
}

#[test]
fn sketch_visibility_derives_flag_offset_and_rejects_invalid_wire() {
    let wire =
        r#"{"stream_ordinal":1,"stream_ordinal_offset":30,"visible_offset":35,"visible":false}"#;
    let visibility: crate::records::sketch_placement::DesignSketchVisibility =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&visibility).unwrap(), wire);
    for (invalid, field) in [
        (
            wire.replace("\"stream_ordinal\":1", "\"stream_ordinal\":0"),
            "stream_ordinal",
        ),
        (
            wire.replace("\"visible_offset\":35", "\"visible_offset\":36"),
            "visible_offset",
        ),
        (
            wire.replace(
                "\"stream_ordinal_offset\":30",
                &format!("\"stream_ordinal_offset\":{}", u64::MAX),
            ),
            "stream_ordinal_offset",
        ),
    ] {
        assert!(
            serde_json::from_str::<crate::records::sketch_placement::DesignSketchVisibility>(
                &invalid
            )
            .unwrap_err()
            .to_string()
            .contains(field)
        );
    }
    let last = crate::records::sketch_placement::DesignSketchVisibility::new(
        std::num::NonZeroU32::MIN,
        u64::MAX - 5,
        true,
    )
    .unwrap();
    assert_eq!(last.visible_offset(), u64::MAX);
    assert!(
        crate::records::sketch_placement::DesignSketchVisibility::new(
            std::num::NonZeroU32::MIN,
            u64::MAX - 4,
            true
        )
        .is_err()
    );
}

#[test]
fn lost_edge_reference_derives_offsets_and_rejects_inconsistent_wire() {
    let wire = r#"{"id":"edge","record_byte_offset":152,"class_tag_offset":156,"class_tag":"419","record_index":299,"record_index_offset":159,"byte_offset":181,"next_byte_offset":200,"next_class_tag":"326","next_record_index":300}"#;
    let record: crate::records::references::LostEdgeReference = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    for field in [
        "class_tag_offset",
        "record_index_offset",
        "byte_offset",
        "next_byte_offset",
    ] {
        let mut invalid = serde_json::to_value(&record).unwrap();
        invalid[field] = serde_json::json!(0);
        assert!(
            serde_json::from_value::<crate::records::references::LostEdgeReference>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
    let mut invalid = serde_json::to_value(&record).unwrap();
    invalid["record_byte_offset"] = serde_json::json!(u64::MAX - 47);
    assert!(
        serde_json::from_value::<crate::records::references::LostEdgeReference>(invalid)
            .unwrap_err()
            .to_string()
            .contains("record_byte_offset")
    );
    let last = crate::records::references::LostEdgeReference::new(
        "edge".into(),
        u64::MAX - 48,
        "419".into(),
        299,
        "326".into(),
        300,
    )
    .unwrap();
    assert_eq!(last.next_byte_offset(), u64::MAX);
}

#[test]
fn lost_edge_reference_requires_three_digit_class_tags() {
    for invalid in ["", "12", "1234", "12a", "１２３"] {
        assert!(crate::records::references::LostEdgeReference::new(
            "edge".into(),
            0,
            invalid.into(),
            0,
            "000".into(),
            0
        )
        .unwrap_err()
        .contains("class_tag"));
        assert!(crate::records::references::LostEdgeReference::new(
            "edge".into(),
            0,
            "000".into(),
            0,
            invalid.into(),
            0
        )
        .unwrap_err()
        .contains("next_class_tag"));
        assert!(
            serde_json::from_value::<crate::records::references::DesignClassTag>(
                serde_json::json!(invalid)
            )
            .unwrap_err()
            .to_string()
            .contains("class_tag")
        );
    }
    for value in ["000", "019", "999"] {
        let tag: crate::records::references::DesignClassTag =
            serde_json::from_value(serde_json::json!(value)).unwrap();
        assert_eq!(tag.as_str(), value);
        assert_eq!(serde_json::to_value(tag).unwrap(), serde_json::json!(value));
    }
}

#[test]
fn sketch_placement_layouts_preserve_wire_and_reject_conflicting_offsets() {
    for (member, length, matrix_delta) in [
        (false, 201, None),
        (false, 213, None),
        (false, 305, Some(48)),
        (false, 325, Some(48)),
        (false, 329, Some(55)),
        (false, 341, Some(66)),
        (true, 34, None),
        (true, 162, Some(22)),
    ] {
        let mut transform = crate::records::identity::IDENTITY_MATRIX;
        if matrix_delta.is_some() {
            transform[0][3] = 12.0;
        }
        let paired = if member { 50 } else { 100 + length };
        let mut wire = serde_json::json!({
            "id": "placement", "scope_record_index": 1,
            "entity_id": "Sketch_7",
            "byte_offset": 100, "class_tag": "300", "record_index": 1,
            "frame_length": length, "transform": transform,
            "paired_class_tag": "301", "paired_byte_offset": paired,
        });
        if member {
            wire["member_run_head"] = true.into();
        }
        if let Some(delta) = matrix_delta {
            wire["transform_offset"] = (100 + delta).into();
        }
        let placement: crate::records::sketch_placement::DesignSketchPlacement =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(placement).unwrap(), wire);
        let mut invalid = wire.clone();
        invalid["transform_offset"] = 99.into();
        assert!(
            serde_json::from_value::<crate::records::sketch_placement::DesignSketchPlacement>(
                invalid
            )
            .unwrap_err()
            .to_string()
            .contains("transform_offset")
        );
        let mut invalid = wire.clone();
        invalid["member_run_head"] = (!member).into();
        assert!(
            serde_json::from_value::<crate::records::sketch_placement::DesignSketchPlacement>(
                invalid
            )
            .unwrap_err()
            .to_string()
            .contains("frame_length")
        );
        let mut invalid = wire.clone();
        invalid["transform"][0][0] = 2.0.into();
        assert!(
            serde_json::from_value::<crate::records::sketch_placement::DesignSketchPlacement>(
                invalid
            )
            .unwrap_err()
            .to_string()
            .contains("transform")
        );
        if !member {
            let mut invalid = wire;
            invalid["paired_byte_offset"] = 50.into();
            assert!(serde_json::from_value::<
                crate::records::sketch_placement::DesignSketchPlacement,
            >(invalid)
            .unwrap_err()
            .to_string()
            .contains("paired_byte_offset"));
        }
    }
}

#[test]
fn sketch_placement_extent_and_matrix_are_checked_at_construction() {
    use crate::records::sketch_placement::{
        DesignSketchFrame, DesignSketchFrameForm, SketchPlacementMatrix,
    };
    assert!(
        DesignSketchFrame::new(u64::MAX - 200, DesignSketchFrameForm::ScopeCompact)
            .unwrap_err()
            .contains("byte_offset")
    );
    let end = DesignSketchFrame::new(u64::MAX - 201, DesignSketchFrameForm::ScopeCompact).unwrap();
    assert_eq!(end.paired_byte_offset(), u64::MAX);
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut matrix = crate::records::identity::IDENTITY_MATRIX;
        matrix[0][3] = invalid;
        assert!(SketchPlacementMatrix::try_from(matrix)
            .unwrap_err()
            .contains("transform"));
    }
    let mut matrix = crate::records::identity::IDENTITY_MATRIX;
    matrix[3][0] = 1.0;
    assert!(SketchPlacementMatrix::try_from(matrix)
        .unwrap_err()
        .contains("transform"));
    let mut matrix = crate::records::identity::IDENTITY_MATRIX;
    matrix[0][1] = 1.0;
    matrix[1][1] = 0.0;
    assert!(SketchPlacementMatrix::try_from(matrix)
        .unwrap_err()
        .contains("transform"));
}

#[test]
fn timeline_frame_rejects_invalid_source_spans() {
    let wire = serde_json::json!({
        "id": "f3d:Design/BulkStream.dat:design-feature-timeline#200", "byte_offset": 200, "class_tag": "256",
        "record_index": 35, "source_ordinal": 0, "frame_length": 100,
        "context_record_index": 17, "context_record_index_offset": 220,
        "item_count_offset": 240, "item_record_indices": [101, 102],
        "item_record_index_offsets": [245, 278]
    });
    for (field, bad, diagnostic) in [
        ("class_tag", serde_json::json!("25x"), "class_tag"),
        ("record_index", serde_json::json!(0), "record_index"),
        (
            "context_record_index",
            serde_json::json!(0),
            "context_record_index",
        ),
        (
            "item_record_indices",
            serde_json::json!([101, 0]),
            "item_record_indices",
        ),
        (
            "item_record_indices",
            serde_json::json!([101, 101]),
            "item_record_indices",
        ),
        ("frame_length", serde_json::json!(u64::MAX), "frame_length"),
        (
            "context_record_index_offset",
            serde_json::json!(200),
            "context_record_index_offset",
        ),
        (
            "context_record_index_offset",
            serde_json::json!(231),
            "context_record_index_offset",
        ),
        (
            "context_record_index_offset",
            serde_json::json!(u64::MAX),
            "context_record_index_offset",
        ),
        (
            "item_count_offset",
            serde_json::json!(297),
            "item_count_offset",
        ),
        (
            "item_count_offset",
            serde_json::json!(u64::MAX),
            "item_count_offset",
        ),
        (
            "item_record_index_offsets",
            serde_json::json!([0, 0]),
            "item_record_index_offsets",
        ),
        (
            "item_record_index_offsets",
            serde_json::json!([244, 278]),
            "item_record_index_offsets",
        ),
        (
            "item_record_index_offsets",
            serde_json::json!([245, 255]),
            "item_record_index_offsets",
        ),
        (
            "item_record_index_offsets",
            serde_json::json!([245, 291]),
            "item_record_index_offsets",
        ),
        (
            "item_record_index_offsets",
            serde_json::json!([245, u64::MAX]),
            "item_record_index_offsets",
        ),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = bad;
        let error =
            serde_json::from_value::<crate::records::entity_header::DesignFeatureTimeline>(invalid)
                .unwrap_err()
                .to_string();
        assert!(error.contains(diagnostic), "{field}: {error}");
    }
    let empty =
        crate::records::entity_header::DesignTimelineFrame::new(200, 44, 220, 240, Vec::new())
            .unwrap();
    assert!(empty.items().is_empty());
}

#[test]
fn sketch_relation_definition_preserves_masks_and_rejects_mismatched_payloads() {
    use crate::records::sketch_relations::{
        SketchPatternDefinition as Kind, SketchRelationDefinition as Definition,
    };
    let patterns = [
        (
            0x1000_0000,
            Some(Kind::Circular {
                angle_parameter: Some(2),
                count_parameter: Some(3),
                evaluated_angle: cadmpeg_ir::scalar::FiniteReal::new(1.5).unwrap(),
                evaluated_count: crate::records::sketch_relations::SketchPatternCount::try_from(2)
                    .unwrap(),
            }),
        ),
        (
            0x2000_0000,
            Some(Kind::Rectangular {
                directions: std::array::from_fn(|_| {
                    crate::records::sketch_relations::SketchPatternDirection {
                        count_parameter: Some(2),
                        distance_parameter: Some(3),
                        evaluated_count:
                            crate::records::sketch_relations::SketchPatternCount::try_from(2)
                                .unwrap(),
                        direction: [1.0, 0.0, 0.0].try_into().unwrap(),
                        evaluated_distance: cadmpeg_ir::scalar::FiniteReal::new(1.5).unwrap(),
                    }
                }),
            }),
        ),
        (0x100_0000_0000, Some(Kind::TextFrame { text_reference: 2 })),
        (
            0x200_0000_0000,
            Some(Kind::TextPath {
                text_reference: 2,
                glyph_transforms: Vec::new(),
            }),
        ),
    ];
    for (mask, kind) in &patterns {
        for unknown in [0, 0x4000, 1 << 63] {
            let definition = Definition::new(mask | unknown, kind.clone()).unwrap();
            assert_eq!(definition.state(), mask | unknown);
            assert_eq!(definition.pattern(), kind.as_ref());
        }
        assert!(Definition::new(*mask, None).is_err());
        assert!(Definition::new(0, kind.clone()).is_err());
        assert!(Definition::new(mask | 1, kind.clone()).is_err());
        for (other_mask, _) in &patterns {
            if other_mask != mask {
                assert!(Definition::new(*other_mask, kind.clone()).is_err());
            }
        }
    }
    for state in [0, 1, 0x11, 0x4000, 0x8000_0000, 0x20_0000_0000, 0x1000_0001] {
        assert_eq!(Definition::new(state, None).unwrap().state(), state);
    }
    let wire = r#"{"id":"relation","record_index":1,"class_tag":"000","byte_offset":0,"state_offset":0,"owner_reference":1,"owner_entity_id":"owner","auxiliary_references":[],"auxiliary_reference_offsets":[],"rectangular_counted_reference_count":0,"members":[],"resolved_members":[],"member_offsets":[],"owner_reference_offset":0,"state":1099511627776,"constraint_kinds":["text_frame"],"unknown_constraint_bits":0,"member_relation_ordinals":[],"entity_genesis":null,"pattern":{"kind":"text_frame","text_reference":2},"return_members":[],"resolved_return_members":[],"return_member_offsets":[],"raw_bytes":"0000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"}"#;
    let relation: crate::records::sketch_relations::SketchRelation =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&relation).unwrap(), wire);
    let mut invalid: serde_json::Value = serde_json::from_str(wire).unwrap();
    invalid["pattern"] = serde_json::Value::Null;
    assert!(
        serde_json::from_value::<crate::records::sketch_relations::SketchRelation>(invalid)
            .unwrap_err()
            .to_string()
            .contains("pattern")
    );
}

#[test]
fn sketch_relation_runs_reject_partial_resolution_and_preserve_atomic_binding() {
    use crate::records::sketch_relations::{
        SketchRelationMembers, SketchRelationOperand, SketchRelationReturnMembers,
    };
    let unresolved = SketchRelationMembers::from_indices([(1, 25, 3), (2, 40, 5)]).unwrap();
    let mut resolved = unresolved.clone();
    resolved.resolve(|record_index| SketchRelationOperand::Record { record_index });
    for (row, (index, offset, ordinal)) in resolved.iter().zip([(1, 25, 3), (2, 40, 5)]) {
        assert_eq!(row.reference.record_index(), index);
        assert_eq!(
            row.reference.resolved(),
            Some(&SketchRelationOperand::Record {
                record_index: index
            })
        );
        assert_eq!((row.offset, row.relation_ordinal), (offset, Some(ordinal)));
    }
    assert!(
        SketchRelationMembers::try_from(vec![unresolved[0].clone(), resolved[1].clone()]).is_err()
    );
    assert!(
        SketchRelationMembers::try_from(vec![resolved[0].clone(), unresolved[1].clone()]).is_err()
    );
    let unresolved_return = SketchRelationReturnMembers::from_indices([(2, 60), (1, 75)]).unwrap();
    let mut resolved_return = unresolved_return.clone();
    resolved_return.resolve(|record_index| SketchRelationOperand::Record { record_index });
    assert!(SketchRelationReturnMembers::try_from(vec![
        unresolved_return[0].clone(),
        resolved_return[1].clone()
    ])
    .is_err());
    assert!(SketchRelationReturnMembers::try_from(vec![
        resolved_return[0].clone(),
        unresolved_return[1].clone()
    ])
    .is_err());
    let mut interrupted = unresolved.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interrupted.resolve(|record_index| {
            assert_ne!(record_index, 2, "interrupt the second resolution");
            SketchRelationOperand::Record { record_index }
        });
    }));
    assert!(result.is_err());
    assert_eq!(interrupted, unresolved);
    let mut interrupted_return = unresolved_return.clone();
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        interrupted_return.resolve(|record_index| {
            assert_ne!(record_index, 1, "interrupt the second resolution");
            SketchRelationOperand::Record { record_index }
        });
    }));
    assert!(result.is_err());
    assert_eq!(interrupted_return, unresolved_return);
}

#[test]
fn glyph_transform_preserves_finite_matrix_wire_without_an_affine_restriction() {
    for wire in [
        "[[1.0,0.0,0.0,0.5],[0.0,1.0,0.0,-0.0],[0.0,0.0,1.0,-2.5],[0.0,0.0,0.0,1.0]]",
        "[[0.0,0.0,0.0,0.0],[0.0,0.0,0.0,0.0],[0.0,0.0,0.0,0.0],[0.0,0.0,0.0,1.0]]",
        "[[1.0,0.0,0.0,0.5],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,-2.5],[0.0,0.0,0.0,2.0]]",
    ] {
        let transform: crate::records::sketch_relations::SketchGlyphTransform =
            serde_json::from_str(wire).unwrap();
        assert_eq!(serde_json::to_string(&transform).unwrap(), wire);
        assert_eq!(
            crate::records::sketch_relations::SketchGlyphTransform::try_from(transform.rows())
                .unwrap(),
            transform
        );
    }
    for ordinal in 0..16 {
        for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
            let mut rows = [[0.0; 4]; 4];
            rows[ordinal / 4][ordinal % 4] = value;
            assert!(
                crate::records::sketch_relations::SketchGlyphTransform::try_from(rows)
                    .unwrap_err()
                    .to_string()
                    .contains("glyph_transforms")
            );
        }
    }
}

#[test]
fn sketch_text_layout_preserves_flat_wire_pairs_and_independent_references() {
    use crate::records::sketch_geometry::SketchText;

    let prefix = r#"{"id":"text","record_index":1,"owner_reference":2,"class_tag":"000","class_version":4,"byte_offset":0,"text":"text","font_family":"Arial","font_weight":400,"height":10.0"#;
    let color = r#","color":{"r":0.0,"g":0.0,"b":0.0,"a":1.0}"#;
    let placement = r#","anchor":{"u":2.0,"v":3.0},"rotation":0.5"#;
    let alignment = r#","horizontal_alignment":3,"vertical_alignment":7"#;
    let txt = format!("{prefix}{color}{placement},\"raw_bytes\":\"\"}}");
    let decoded: SketchText = serde_json::from_str(&txt).expect("txt placement");
    assert_eq!(serde_json::to_string(&decoded).unwrap(), txt);
    for placement in ["", placement] {
        for alignment in ["", alignment] {
            for first_reference in ["", ",\"first_reference\":0", ",\"first_reference\":12"] {
                for second_reference in ["", ",\"second_reference\":0", ",\"second_reference\":13"]
                {
                    let wire = format!("{prefix},\"width_factor\":1.0{color}{placement}{alignment}{first_reference}{second_reference},\"raw_bytes\":\"\"}}");
                    let decoded: SketchText =
                        serde_json::from_str(&wire).expect("textex member pairs");
                    assert_eq!(serde_json::to_string(&decoded).unwrap(), wire);
                }
            }
        }
    }
}

#[test]
fn sketch_text_layout_rejects_partial_placement_and_alignment() {
    use crate::records::sketch_geometry::SketchText;

    let base = serde_json::json!({
        "id": "text", "record_index": 1, "owner_reference": 2,
        "class_tag": "000", "class_version": 4, "byte_offset": 0,
        "text": "text", "font_family": "Arial", "font_weight": 400,
        "height": 10.0, "width_factor": 1.0,
        "color": {"r": 0.0, "g": 0.0, "b": 0.0, "a": 1.0}, "raw_bytes": ""
    });
    for (field, value) in [
        ("anchor", serde_json::json!({"u": 2.0, "v": 3.0})),
        ("rotation", serde_json::json!(0.5)),
        ("horizontal_alignment", serde_json::json!(3)),
        ("vertical_alignment", serde_json::json!(7)),
    ] {
        let mut wire = base.clone();
        wire[field] = value;
        let error = serde_json::from_value::<SketchText>(wire).unwrap_err();
        assert!(error.to_string().contains(field), "{error}");
    }
}

#[test]
fn sketch_point_flags_preserve_numeric_wire_and_reject_non_boolean_values() {
    use crate::records::sketch_geometry::SketchPoint;
    use serde_json::json;

    for (form, count) in [
        (json!({"kind": "version0"}), 1),
        (json!({"kind": "version8"}), 7),
        (json!({"kind": "version10"}), 7),
        (
            json!({"kind": "version10_inline_typed", "trailing_reference": 3}),
            7,
        ),
        (
            json!({"kind": "version11", "padded_paired_reference": false,
                "companion_prefix_present_zero": false}),
            8,
        ),
        (
            json!({"kind": "version11_inline_typed", "trailing_reference": 3,
                "companion_prefix_present_zero": false}),
            8,
        ),
    ] {
        let mut base = json!({
            "id": "point", "record_index": 1, "class_tag": "000",
            "byte_offset": 0, "coordinate_offset": 1, "record_form": form,
            "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0}, "depth": 0.0
        });
        base["companion"] = json!({"incident_curves": []});
        if count > 1 {
            base["persistent_id"] = json!(4);
            base["closure"] = json!({"selector": 0, "state": 0});
        }
        let point: SketchPoint = serde_json::from_value(base.clone()).expect("omitted zero flags");
        assert_eq!(serde_json::to_value(point).unwrap(), base);
        for prefix_present_zero in [false, true] {
            let mut wire = base.clone();
            wire["companion"] = json!({"incident_curves": [7, 2]});
            if count == 8 {
                wire["record_form"]["companion_prefix_present_zero"] = json!(prefix_present_zero);
            } else if prefix_present_zero {
                continue;
            }
            let point =
                serde_json::from_value::<SketchPoint>(wire.clone()).expect("companion wire");
            let companion = point.companion();
            assert_eq!(companion.prefix_present_zero, prefix_present_zero);
            assert_eq!(companion.incident_curves, [7, 2]);
            assert_eq!(serde_json::to_value(point).unwrap(), wire);
        }
        for depth in [-7.5, 2.5] {
            let mut wire = base.clone();
            wire["depth"] = json!(depth);
            let decoded = serde_json::from_value::<SketchPoint>(wire.clone());
            if count > 1 {
                let point = decoded.expect("three-dimensional point form");
                assert_eq!(point.depth(), depth);
                assert_eq!(serde_json::to_value(point).unwrap(), wire);
            } else {
                let error = decoded.unwrap_err();
                assert!(error.to_string().contains("depth"), "{error}");
            }
        }
        for genesis in [0, 9, u64::MAX] {
            let mut wire = base.clone();
            wire["entity_genesis"] = json!(genesis);
            let decoded = serde_json::from_value::<SketchPoint>(wire.clone());
            if count == 8 {
                let point = decoded.expect("version-11 genesis");
                assert_eq!(point.entity_genesis(), Some(genesis));
                assert_eq!(serde_json::to_value(point).unwrap(), wire);
            } else {
                let error = decoded.unwrap_err();
                assert!(error.to_string().contains("entity_genesis"), "{error}");
            }
        }
        for lane in 0..8 {
            for value in [1, 2, 255] {
                let mut wire = base.clone();
                let mut flags = [0; 8];
                flags[lane] = value;
                wire["flags"] = json!(cadmpeg_ir::native::bytes::NativeBytes::from(flags));
                let decoded = serde_json::from_value::<SketchPoint>(wire.clone());
                if lane < count && value == 1 {
                    assert_eq!(
                        serde_json::to_value(decoded.expect("boolean flag")).unwrap(),
                        wire
                    );
                } else {
                    let error = decoded.unwrap_err();
                    assert!(error.to_string().contains("flags"), "{error}");
                }
            }
        }
    }
}

#[test]
fn empty_reference_runs_have_one_representation() {
    use crate::records::identity::{Located, ReferenceRun};

    let empty = ReferenceRun::<u32>::unlocated(Vec::new());
    assert_eq!(empty, ReferenceRun::located(Vec::new()));
    assert_eq!(empty.located_rows(), Some([].as_slice()));
    assert_eq!(
        ReferenceRun::<u32>::from_columns(Vec::new(), Vec::new(), "field").expect("empty columns"),
        ReferenceRun::located(Vec::new())
    );

    let unlocated = ReferenceRun::unlocated(vec![7u32]);
    assert!(unlocated.located_rows().is_none());
    assert_ne!(
        unlocated,
        ReferenceRun::located(vec![Located {
            value: 7u32,
            offset: 0
        }])
    );

    let header = crate::records::entity_header::DesignEntityHeader {
        id: "header".into(),
        byte_offset: 10,
        entity_id: crate::records::identity::DesignEntityId::from_parts("Sketch", 7),
        class_tag: "256".to_owned().try_into().unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some("MSketch".into()),
            Some(crate::records::entity_header::SketchHeaderReferences {
                record_reference: None,
                record_reference_offset: 20,
                references: Vec::new(),
            }),
            ReferenceRun::unlocated(Vec::new()),
        )
        .unwrap(),
    };
    let wire = r#"{"id":"header","byte_offset":10,"entity_id":"Sketch_7","class_tag":"256","optional_slot_present":false,"module":"MSketch","record_reference_offset":20,"declared_reference_count":0,"reference_indices":[],"reference_offsets":[]}"#;
    assert_eq!(serde_json::to_string(&header).unwrap(), wire);
    assert_eq!(
        serde_json::from_str::<crate::records::entity_header::DesignEntityHeader>(wire).unwrap(),
        header
    );
}

#[test]
fn dimension_locus_pairs_preserve_both_frame_forms_and_reject_a_stray_opaque_index() {
    let shared = r#""id":"f3d:test:dimension-locus-pair#0","companion_record_index":1,"governing_companion_record_index":2,"byte_offset":10,"class_tag":"274","record_index":3,"frame_length":80"#;
    let loci = r#""first_geometry_reference_offset":50,"first_role":0,"first_role_offset":60,"second_geometry_record_index":41,"second_geometry_reference_offset":65,"second_role":1,"second_role_offset":75,"paired_class_tag":"273","paired_byte_offset":90}"#;
    for (opaque, first, valid) in [
        (r#","opaque_index":4,"opaque_index_offset":45"#, 40, true),
        ("", 0, true),
        (r#","opaque_index":4,"opaque_index_offset":45"#, 0, false),
        ("", 40, false),
        (r#","opaque_index":4"#, 40, false),
    ] {
        let loci = if first == 0 {
            loci.replace(
                "\"first_geometry_reference_offset\":50",
                "\"first_geometry_reference_offset\":35",
            )
            .replace("\"first_role_offset\":60", "\"first_role_offset\":45")
            .replace(
                "\"second_geometry_reference_offset\":65",
                "\"second_geometry_reference_offset\":50",
            )
            .replace("\"second_role_offset\":75", "\"second_role_offset\":60")
        } else {
            loci.to_owned()
        };
        let wire = format!(r#"{{{shared}{opaque},"first_geometry_record_index":{first},{loci}"#);
        let parsed =
            serde_json::from_str::<crate::records::dimensions::DesignDimensionLocusPair>(&wire);
        if valid {
            assert_eq!(
                serde_json::to_string(&parsed.expect("dimension locus pair")).expect("pair wire"),
                wire
            );
        } else {
            assert!(parsed.is_err(), "{wire}");
        }
    }
}

#[test]
fn null_locus_arena_preserves_base_wire_fields_and_order() {
    let entry = r#"{"id":"f3d:test:dimension-locus-pair#0","companion_record_index":1,"governing_companion_record_index":2,"byte_offset":10,"class_tag":"274","record_index":3,"frame_length":80,"null_reference_offset":35,"null_role":0,"null_role_offset":45,"geometry_record_index":41,"geometry_reference_offset":50,"geometry_role":1,"geometry_role_offset":60,"paired_class_tag":"273","paired_byte_offset":90}"#;
    let wire = format!(r#"{{"design_dimension_null_locus_pairs":[{entry}]}}"#);
    let native: crate::native::F3dNative = serde_json::from_str(&wire).unwrap();
    let encoded = serde_json::to_string(&native).unwrap();
    assert!(encoded.contains(&format!(r#""design_dimension_null_locus_pairs":[{entry}]"#)));
    let decoded: crate::native::F3dNative = serde_json::from_str(&encoded).unwrap();
    assert_eq!(decoded, native);
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    let arena: Vec<crate::records::dimension_null_locus_wire::Wire> = namespace
        .arena_as("design_dimension_null_locus_pairs")
        .unwrap();
    assert_eq!(serde_json::to_string(&arena).unwrap(), format!("[{entry}]"));
    assert_eq!(crate::native::F3dNative::load(&namespace).unwrap(), native);
}

#[test]
fn segment_type_guid_preserves_relaxed_text_and_rejects_invalid_text() {
    for guid in ["g".repeat(36), "_".repeat(38), "bad".into()] {
        let wire = format!(
            r#"{{"id":"f3d:native/MetaStream.dat:design-type#0","byte_offset":0,"type_guid":"{guid}","type_guid_offset":4,"version":1,"version_offset":80,"module":"Fusion","entity_ids":[],"entity_id_offsets":[]}}"#
        );
        let decoded = serde_json::from_str::<crate::records::entity_header::SegmentType>(&wire);
        if guid == "bad" {
            assert!(decoded.is_err());
        } else {
            assert_eq!(serde_json::to_string(&decoded.unwrap()).unwrap(), wire);
        }
    }
}

#[test]
fn segment_base_guid_rejects_invalid_text() {
    let wire = r#"{"id":"f3d:native/MetaStream.dat:design-type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4,"base_type_guid":"invalid","version":1,"version_offset":80,"module":"Fusion","entity_ids":[],"entity_id_offsets":[]}"#;
    let error =
        serde_json::from_str::<crate::records::entity_header::SegmentType>(wire).unwrap_err();
    assert!(error.to_string().contains("base_type_guid"));
}

#[test]
fn material_assignment_sidecar_rejects_invalid_visual_guid() {
    let wire = serde_json::json!({
        "id": "material#0", "asm_body_key": 42, "asm_body_key_offset": 10,
        "entity_suffix_offset": 20, "entity_id": "0_985",
        "entity_id_offset": 30, "visual_guid": "not-a-guid", "visual_guid_offset": 40
    });
    assert!(serde_json::from_value::<super::references::DesignMaterialAssignment>(wire).is_err());
}

#[test]
fn visual_token_wire_preserves_revision_spelling() {
    for text in [
        "abcdef01-2222-3333-4444-555555555555",
        "ABCDEF01-2222-3333-4444-555555555555_Post2015_Post2015",
    ] {
        let wire = serde_json::json!(text);
        let token: super::references::DesignVisualToken =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(token).unwrap(), wire);
    }
    for text in [
        "",
        "not-a-guid",
        "abcdef01-2222-3333-4444-555555555555_post2015",
    ] {
        assert!(
            serde_json::from_value::<super::references::DesignVisualToken>(serde_json::json!(text))
                .is_err()
        );
    }
}

#[test]
fn sketch_link_sidecar_rejects_present_sentinels() {
    for sense in [-1, super::sketch_links::SKETCH_LINK_SENSE_UNCONSTRAINED] {
        let wire = serde_json::json!({"id": "link", "target": {"kind": "document"},
            "sketch_curve_id": 1, "ref_b": 0, "sense": sense, "role": 0, "closure": 0});
        assert!(serde_json::from_value::<super::sketch_links::SketchCurveLink>(wire).is_err());
    }
}

#[test]
fn sketch_link_sidecar_preserves_all_non_sentinel_senses() {
    for sense in [i64::MIN, -2, 0, 1, 2, i64::MAX] {
        let wire = serde_json::json!({"id": "link", "target": {"kind": "document"},
            "sketch_curve_id": 1, "ref_b": 0, "sense": sense, "role": 0, "closure": 0});
        let record: super::sketch_links::SketchCurveLink =
            serde_json::from_value(wire.clone()).unwrap();
        assert_eq!(serde_json::to_value(record).unwrap(), wire);
    }
}

#[test]
fn sketch_pattern_count_enforces_both_bounds() {
    for value in [0, 100_001, u32::MAX] {
        assert!(super::sketch_relations::SketchPatternCount::try_from(value).is_err());
        let error =
            serde_json::from_value::<super::sketch_relations::SketchPatternCount>(value.into())
                .unwrap_err();
        assert!(error.to_string().contains("evaluated_count"));
    }
    for value in [1, 100_000] {
        let count = super::sketch_relations::SketchPatternCount::try_from(value).unwrap();
        assert_eq!(count.get(), value);
        assert_eq!(
            serde_json::to_value(count).unwrap(),
            serde_json::json!(value)
        );
    }
}

#[test]
fn sketch_identity_sidecar_rejects_zero() {
    let point = serde_json::json!({"id": "point", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "coordinate_offset": 1, "record_form": {"kind": "version8"},
        "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0}, "depth": 0.0,
        "persistent_id": 0, "closure": {"selector": 0, "state": 0},
        "companion": {"incident_curves": []}});
    assert!(
        serde_json::from_value::<super::sketch_geometry::SketchPoint>(point)
            .unwrap_err()
            .to_string()
            .contains("persistent_id")
    );
    let curve = serde_json::json!({"id": "curve", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "geometry_offset": 0, "primary_id": 0, "secondary_id": 0});
    assert!(serde_json::from_value::<super::sketch_geometry::SketchCurveIdentity>(curve).is_err());
    let surface = serde_json::json!({"id": "surface", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "persistent_id": 0, "u_degree": 0, "v_degree": 0,
        "u_knots": [], "v_knots": [], "control_points": []});
    assert!(serde_json::from_value::<super::sketch_geometry::SketchSurface>(surface).is_err());
}

#[test]
fn sketch_point_admission_and_edits_keep_finite_coordinates() {
    let draft = super::sketch_geometry::SketchPointDraft {
        id: "point".into(),
        record_index: 1,
        owner_reference: None,
        class_tag: "000".to_owned().try_into().unwrap(),
        byte_offset: 0,
        coordinate_offset: 0,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: super::sketch_geometry::SketchPointRecordForm::version11(
            1,
            super::sketch_geometry::SketchPointClosure::Selector0State0,
            None,
            -2.0,
        ),
        paired_reference: 2,
        coordinates: cadmpeg_ir::math::Point2::new(1.0, -1.0),
    };
    let original = super::sketch_geometry::SketchPoint::try_from(draft.clone()).unwrap();
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        for coordinates in [
            cadmpeg_ir::math::Point2::new(value, 0.0),
            cadmpeg_ir::math::Point2::new(0.0, value),
        ] {
            let mut invalid = draft.clone();
            invalid.coordinates = coordinates;
            assert!(super::sketch_geometry::SketchPoint::try_from(invalid).is_err());
            let mut point = original.clone();
            assert!(point.try_set_coordinates(coordinates).is_err());
            assert_eq!(point, original);
        }
        let mut form = draft.record_form.clone();
        let super::sketch_geometry::SketchPointRecordForm::Version11 { depth, .. } = &mut form
        else {
            panic!("version 11");
        };
        *depth = value;
        let mut invalid = draft.clone();
        invalid.record_form = form.clone();
        assert!(super::sketch_geometry::SketchPoint::try_from(invalid).is_err());
        let mut point = original.clone();
        assert!(point.try_set_record_form(form).is_err());
        assert_eq!(point, original);
        let mut wire = super::sketch_geometry::SketchPointSerde::from(original.clone());
        wire.depth = value;
        assert!(super::sketch_geometry::SketchPoint::try_from(wire).is_err());
    }
}

#[test]
fn sketch_point_requires_a_distinct_companion_on_every_route() {
    let mut wire = serde_json::json!({"id": "point", "record_index": 1, "class_tag": "000",
        "byte_offset": 0, "coordinate_offset": 1, "record_form": {"kind": "version8"},
        "paired_reference": 2, "coordinates": {"u": 2.0, "v": 3.0}, "depth": 0.0,
        "persistent_id": 1, "closure": {"selector": 0, "state": 0},
        "companion": {"incident_curves": []}});
    let original: super::sketch_geometry::SketchPoint =
        serde_json::from_value(wire.clone()).unwrap();
    assert!(original.companion().incident_curves.is_empty());
    assert_eq!(serde_json::to_value(&original).unwrap(), wire);
    wire["companion"]["incident_curves"] = serde_json::json!([7, 7]);
    assert!(
        serde_json::from_value::<super::sketch_geometry::SketchPoint>(wire.clone())
            .unwrap_err()
            .to_string()
            .contains("incident_curves")
    );
    wire.as_object_mut().unwrap().remove("companion");
    assert!(
        serde_json::from_value::<super::sketch_geometry::SketchPoint>(wire)
            .unwrap_err()
            .to_string()
            .contains("companion")
    );
    let duplicate = super::sketch_geometry::SketchPointCompanion {
        incident_curves: vec![7, 7],
    };
    let draft = super::sketch_geometry::SketchPointDraft {
        id: "point".into(),
        record_index: 1,
        owner_reference: None,
        class_tag: "000".to_owned().try_into().unwrap(),
        byte_offset: 0,
        coordinate_offset: 0,
        record_form: original.record_form().clone().into_raw(),
        paired_reference: 2,
        coordinates: original.coordinates(),
        companion: duplicate.clone(),
    };
    assert!(super::sketch_geometry::SketchPoint::try_from(draft).is_err());
    let mut point = original.clone();
    assert!(point.try_set_companion(duplicate).is_err());
    assert_eq!(point, original);
}

#[test]
fn body_bounds_admit_only_ordered_finite_cache_frames() {
    let wire = serde_json::json!({"id": "f3d:Design/BulkStream.dat:design-body-bounds#0", "entity_suffix": 10, "entity_byte_offset": 0,
        "record_indices": [11, 12, 13], "record_byte_offsets": [20, 40, 60],
        "value_byte_offsets": [21, 41, 61], "maximum": {"x": 1.0, "y": 0.0, "z": 0.0},
        "minimum": {"x": 0.0, "y": 0.0, "z": 0.0}});
    let record: super::bodies::DesignBodyBounds = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(&record).unwrap(), wire);
    for (field, value) in [
        ("entity_suffix", serde_json::json!(u64::MAX)),
        ("entity_suffix", serde_json::json!(u32::MAX)),
        ("record_indices", serde_json::json!([11, 12, 14])),
        ("record_byte_offsets", serde_json::json!([20, 20, 60])),
        ("value_byte_offsets", serde_json::json!([20, 41, 61])),
        (
            "maximum",
            serde_json::json!({"x": -1.0, "y": 0.0, "z": 0.0}),
        ),
        ("maximum", serde_json::json!({"x": 0.0, "y": 0.0, "z": 0.0})),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::bodies::DesignBodyBounds>(invalid).is_err(),
            "{field}"
        );
    }
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let mut invalid = super::bodies::DesignBodyBoundsWire::from(record.clone());
        invalid.maximum.x = value;
        assert!(super::bodies::DesignBodyBounds::try_from(invalid).is_err());
    }
}

#[test]
fn body_binding_wire_rejects_invalid_pair_frames() {
    let wire = serde_json::json!({"id": "f3d:Design/BulkStream.dat:design-body-binding#10", "stream": "Design/BulkStream.dat",
        "pair_count": 1, "pair_ordinal": 0, "asm_body_key": 0, "asm_body_key_offset": 10,
        "entity_suffix": 0, "entity_suffix_offset": 18, "blob_name": "BREP.", "blob_name_offset": 19});
    let record: super::bodies::DesignBodyBinding = serde_json::from_value(wire.clone()).unwrap();
    assert_eq!(serde_json::to_value(record).unwrap(), wire);
    for (field, value) in [
        ("pair_count", serde_json::json!(0)),
        ("pair_ordinal", serde_json::json!(1)),
        ("asm_body_key_offset", serde_json::json!(u64::MAX)),
        ("entity_suffix_offset", serde_json::json!(19)),
        ("blob_name", serde_json::json!("body.smbh")),
        ("blob_name_offset", serde_json::json!(18)),
    ] {
        let mut invalid = wire.clone();
        invalid[field] = value;
        assert!(
            serde_json::from_value::<super::bodies::DesignBodyBinding>(invalid).is_err(),
            "{field}"
        );
    }
    let mut overflow = wire;
    overflow["asm_body_key_offset"] = u64::MAX.into();
    overflow["entity_suffix_offset"] = u64::MAX.into();
    overflow["blob_name_offset"] = u64::MAX.into();
    assert!(serde_json::from_value::<super::bodies::DesignBodyBinding>(overflow).is_err());
}

mod act_entities;

mod native_ids;

mod persistent_text;

mod locus_frames;
