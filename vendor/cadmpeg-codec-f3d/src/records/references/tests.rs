use super::{
    DesignMaterialAssignment, DesignMaterialAssignmentWire, LostEdgeReference,
    LostEdgeReferenceWire, DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT, LOST_EDGE_REFERENCE_CLONE_COUNT,
};

fn lost_edge() -> LostEdgeReference {
    serde_json::from_str(
        r#"{"id":"f3d:native:lost-edge#0","record_byte_offset":152,"class_tag_offset":156,"class_tag":"419","record_index":299,"record_index_offset":159,"byte_offset":181,"next_byte_offset":200,"next_class_tag":"326","next_record_index":300}"#,
    )
    .unwrap()
}

fn material(extra_fields: &str) -> DesignMaterialAssignment {
    let wire = format!(
        r#"{{"id":"f3d:native:material-assignment#0","asm_body_key":42,"asm_body_key_offset":10,"entity_suffix_offset":20,"entity_id":"0_985","entity_id_offset":30,"visual_guid":"11111111-2222-3333-4444-555555555555","visual_guid_offset":40{extra_fields}}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn lost_edge_reference_borrowed_wire_matches_owned_wire_bytes() {
    let record = lost_edge();
    let owned = LostEdgeReferenceWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn lost_edge_reference_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = lost_edge();
    let arena_name = "lost_edge_references";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    LOST_EDGE_REFERENCE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    LOST_EDGE_REFERENCE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()[arena_name][0]).unwrap(),
        serde_json::to_value(&record).unwrap()
    );
}

#[test]
fn material_assignment_borrowed_wire_matches_owned_wire_bytes() {
    for extra_fields in [
        "",
        r#", "physical_token":"","physical_token_offset":0"#,
        r#", "visual_preset":"Prism-002","visual_preset_offset":50"#,
        r#", "physical_token":"Mat","physical_token_offset":50,"visual_preset":"Prism-002","visual_preset_offset":80"#,
    ] {
        let record = material(extra_fields);
        let owned = DesignMaterialAssignmentWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn material_assignment_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = material(
        r#", "physical_token":"Mat","physical_token_offset":50,"visual_preset":"Prism-002","visual_preset_offset":80"#,
    );
    let arena_name = "design_material_assignments";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    DESIGN_MATERIAL_ASSIGNMENT_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&record))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()[arena_name][0]).unwrap(),
        serde_json::to_value(&record).unwrap()
    );
}
