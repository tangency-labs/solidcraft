// SPDX-License-Identifier: Apache-2.0

#[test]
fn decal_mapping_modes_preserve_all_bytes_with_canonical_known_mode() {
    use crate::records::decal::{DesignDecalMappingMode, UnrecognizedDecalMappingMode};
    assert_eq!(
        DesignDecalMappingMode::from_code(0x60),
        DesignDecalMappingMode::FitToFaces
    );
    assert!(UnrecognizedDecalMappingMode::try_from(0x60).is_err());
    for code in 0..=u8::MAX {
        let mode = DesignDecalMappingMode::from_code(code);
        let wire = code.to_string();
        assert_eq!(serde_json::to_string(&mode).expect("Decal mode wire"), wire);
        assert_eq!(
            serde_json::from_str::<DesignDecalMappingMode>(&wire).expect("Decal mode"),
            mode
        );
        if code != 0x60 {
            let unknown = UnrecognizedDecalMappingMode::try_from(code).expect("unrecognized mode");
            assert_eq!(DesignDecalMappingMode::Unknown(unknown), mode);
        }
    }
}

fn decal_wire_fixture() -> serde_json::Value {
    let base = serde_json::json!({
        "id": "decal", "scope_record_index": 23, "asset_reference_offset": 222,
        "mapping_mode": 96, "mapping_mode_offset": 232, "target_group_record_index": 24,
        "target_group_reference_offset": 234, "asset_class_tag": "258", "asset_record_index": 17,
        "asset_byte_offset": 100, "asset_frame_length": 30, "asset_entity_suffix": 50,
        "asset_entity_reference_offset": 120, "name_class_tag": "279", "name_record_index": 18,
        "name_byte_offset": 130, "name_frame_length": 41, "asset_name": "mark.png", "asset_name_offset": 155
    });
    base
}

fn decal_image_fixture() -> super::DesignDecalImage {
    let mut wire = decal_wire_fixture();
    wire["id"] = serde_json::json!("f3d:native:decal#0");
    serde_json::from_value(wire).unwrap()
}

#[test]
fn decal_image_borrowed_wire_matches_owned_wire_bytes() {
    let image = decal_image_fixture();
    let owned = super::DesignDecalImageWire::from(image.clone());
    assert_eq!(
        serde_json::to_vec(&image).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn decal_image_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let image = decal_image_fixture();
    let arena_name = "design_decal_images";
    let needed = serde_json::to_vec(&image).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::DECAL_IMAGE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&image))
        .unwrap_err();
    super::DECAL_IMAGE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&image))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn decal_image_wire_derives_consecutive_records_and_scope_offsets() {
    let base = decal_wire_fixture();
    for mode in [0, 0x60, 0x61, 0xff] {
        let mut value = base.clone();
        value["mapping_mode"] = serde_json::json!(mode);
        let wire: crate::records::decal::DesignDecalImageWire =
            serde_json::from_value(value).expect("Decal wire");
        let expected = serde_json::to_string(&wire).expect("Decal wire bytes");
        let image: crate::records::decal::DesignDecalImage =
            serde_json::from_str(&expected).expect("Decal binding");
        assert_eq!(image.scope_byte_offset(), 200);
        assert_eq!(
            serde_json::to_string(&image).expect("Decal binding bytes"),
            expected
        );
    }
    let mut unicode = base.clone();
    unicode["asset_name"] = serde_json::json!("图😀.png");
    unicode["name_frame_length"] = serde_json::json!(39);
    unicode["asset_record_index"] = serde_json::json!(u32::MAX - 1);
    unicode["name_record_index"] = serde_json::json!(u32::MAX);
    let wire: crate::records::decal::DesignDecalImageWire =
        serde_json::from_value(unicode).expect("Decal Unicode wire");
    let expected = serde_json::to_string(&wire).expect("Decal Unicode bytes");
    let image: crate::records::decal::DesignDecalImage =
        serde_json::from_str(&expected).expect("Decal Unicode name");
    assert_eq!(
        serde_json::to_string(&image).expect("Decal Unicode output"),
        expected
    );
    for field in [
        "asset_reference_offset",
        "mapping_mode_offset",
        "target_group_reference_offset",
        "asset_frame_length",
        "asset_entity_reference_offset",
        "name_record_index",
        "name_byte_offset",
        "name_frame_length",
        "asset_name_offset",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!(0);
        let error = serde_json::from_value::<crate::records::decal::DesignDecalImage>(value)
            .expect_err("invalid Decal field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in ["asset_class_tag", "name_class_tag", "asset_name"] {
        let mut value = base.clone();
        value[field] = serde_json::json!("");
        let error = serde_json::from_value::<crate::records::decal::DesignDecalImage>(value)
            .expect_err("empty Decal field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in ["asset_reference_offset", "asset_byte_offset"] {
        let mut value = base.clone();
        value[field] = serde_json::json!(u64::MAX);
        let error = serde_json::from_value::<crate::records::decal::DesignDecalImage>(value)
            .expect_err("Decal byte extent overflow")
            .to_string();
        assert!(error.contains(field));
    }
    let mut no_successor = base;
    no_successor["asset_record_index"] = serde_json::json!(u32::MAX);
    no_successor["name_record_index"] = serde_json::json!(u32::MAX);
    let error = serde_json::from_value::<crate::records::decal::DesignDecalImage>(no_successor)
        .expect_err("Decal record index overflow")
        .to_string();
    assert!(error.contains("name_record_index"));
}
