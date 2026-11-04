use super::{
    DesignEntityHeader, DesignEntityHeaderWire, DesignFeatureTimeline, DesignFeatureTimelineWire,
    SegmentType, SegmentTypeWire, ENTITY_HEADER_CLONE_COUNT, SEGMENT_TYPE_CLONE_COUNT,
    TIMELINE_CLONE_COUNT,
};

fn timeline() -> DesignFeatureTimeline {
    serde_json::from_str(
        r#"{"id":"f3d:Design/BulkStream.dat:design-feature-timeline#200","byte_offset":200,"class_tag":"256","record_index":35,"source_ordinal":0,"frame_length":100,"context_record_index":17,"context_record_index_offset":220,"item_count_offset":240,"item_record_indices":[101,102],"item_record_index_offsets":[245,256]}"#,
    )
    .unwrap()
}

fn header() -> DesignEntityHeader {
    serde_json::from_str(
        r#"{"id":"f3d:Design/BulkStream.dat:design-entity-header#0","byte_offset":0,"entity_id":"0_1","class_tag":"256","optional_slot_present":false,"module":"MSketch","record_reference":33,"record_reference_offset":40,"declared_reference_count":2,"reference_indices":[34,35],"reference_offsets":[50,61],"member_indices":[11],"member_offsets":[0]}"#,
    )
    .unwrap()
}

#[test]
fn feature_timeline_borrowed_wire_matches_owned_wire_bytes() {
    let record = timeline();
    let owned = DesignFeatureTimelineWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn feature_timeline_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = timeline();
    let arena_name = "design_feature_timelines";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    TIMELINE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    TIMELINE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
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
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn entity_header_borrowed_wire_matches_owned_wire_bytes() {
    for wire in [
        r#"{"id":"header","byte_offset":0,"entity_id":"0_1","class_tag":"256","optional_slot_present":false,"reference_indices":[],"reference_offsets":[]}"#,
        r#"{"id":"header","byte_offset":0,"entity_id":"0_1","class_tag":"256","optional_slot_present":false,"module":"MSketch","record_reference":33,"record_reference_offset":40,"declared_reference_count":2,"reference_indices":[34,35],"reference_offsets":[50,61],"member_indices":[11],"member_offsets":[0]}"#,
    ] {
        let record: DesignEntityHeader = serde_json::from_str(wire).unwrap();
        let owned = DesignEntityHeaderWire::from(record.clone());
        assert_eq!(
            serde_json::to_vec(&record).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn entity_header_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let record = header();
    let arena_name = "design_entity_headers";
    let needed = serde_json::to_vec(&record).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    ENTITY_HEADER_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&record))
        .unwrap_err();
    ENTITY_HEADER_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
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
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

fn segment(base_fields: &str) -> SegmentType {
    let wire = format!(
        r#"{{"id":"f3d:native/MetaStream.dat:design-type#0","byte_offset":0,"type_guid":"11111111-2222-3333-4444-555555555555","type_guid_offset":4{base_fields},"version":1,"version_offset":80,"module":"Fusion","entity_ids":[10,11],"entity_id_offsets":[90,98]}}"#
    );
    serde_json::from_str(&wire).unwrap()
}

#[test]
fn segment_type_borrowed_wire_matches_owned_wire_bytes() {
    for base_fields in [
        "",
        r#", "base_type_guid":"","base_type_guid_offset":44"#,
        r#", "base_type_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","base_type_guid_offset":44"#,
    ] {
        let entry = segment(base_fields);
        let owned = SegmentTypeWire::from(entry.clone());
        assert_eq!(
            serde_json::to_vec(&entry).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn segment_type_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let entry = segment(
        r#", "base_type_guid":"aaaaaaaa-bbbb-cccc-dddd-eeeeeeeeeeee","base_type_guid_offset":44"#,
    );
    let arena_name = "design_types";
    let needed = serde_json::to_vec(&entry).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    SEGMENT_TYPE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&entry))
        .unwrap_err();
    SEGMENT_TYPE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&entry))
        .unwrap();
    assert_eq!(
        serde_json::to_value(&namespace.arenas()[arena_name][0]).unwrap(),
        serde_json::to_value(&entry).unwrap()
    );
}

#[test]
fn segment_type_rejects_unbound_native_identity() {
    let valid = segment("");
    for id in [
        "",
        "f3d:native/MetaStream.dat:design-type#1",
        "f3d:native/MetaStream.dat:other#0",
        "f3d:native/BulkStream.dat:design-type#0",
    ] {
        let mut wire = SegmentTypeWire::from(valid.clone());
        wire.id = id.into();
        assert!(SegmentType::try_from(wire).is_err());
        let mut wire = serde_json::to_value(&valid).unwrap();
        wire["id"] = serde_json::json!(id);
        assert!(serde_json::from_value::<SegmentType>(wire).is_err());
    }
}
