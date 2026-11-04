// SPDX-License-Identifier: Apache-2.0

#[test]
fn mesh_record_identity_borrowed_wire_matches_owned_wire_bytes() {
    let record: super::DesignMeshRecordIdentity = serde_json::from_str(
        r#"{"class_tag":"256","record_index":1,"byte_offset":100,"frame_length":11}"#,
    )
    .unwrap();
    let owned = super::DesignMeshRecordIdentityWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn mesh_record_identity_native_retained_limit_refuses_before_record_clone() {
    #[derive(serde::Serialize)]
    struct NativeRecord<'a> {
        id: &'static str,
        identity: &'a super::DesignMeshRecordIdentity,
    }

    let identity: super::DesignMeshRecordIdentity = serde_json::from_str(
        r#"{"class_tag":"256","record_index":1,"byte_offset":100,"frame_length":11}"#,
    )
    .unwrap();
    let record = NativeRecord {
        id: "f3d:native:mesh-record#0",
        identity: &identity,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_mesh_features",
        || super::MESH_RECORD_IDENTITY_CLONE_COUNT.with(|count| count.set(0)),
        || super::MESH_RECORD_IDENTITY_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn mesh_feature_body_rows_preserve_wire_and_reject_duplicate_arrays() {
    let identity = serde_json::json!({
        "class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 200
    });
    let body = serde_json::json!({
        "body_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 575},
        "entry_name_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 62}, "guid_record": identity,
        "wrapper_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 40},
        "scene_state_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 95}, "scene_node_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 133},
        "scene_auxiliary_record": identity, "owner_record": identity,
        "entry_name": "mesh.paramesh", "entry_name_offset": 136,
        "fusion_uuid": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", "fusion_uuid_offset": 136,
        "transform": [[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "transform_offsets": [142, 271], "scope_reference_offset": 608,
        "wrapper_reference_offset": 619, "owner_reference_offset": 630,
        "guid_reference_offset": 641, "scene_node_reference_offset": 653,
        "collection_reference_offset": 664, "wrapper_body_reference_offset": 121,
        "entry_guid_reference_offset": 121, "guid_entry_reference_offset": 172,
        "scene_state_reference_offset": 133, "scene_auxiliary_reference_offset": 148
    });
    let base = serde_json::json!({
        "id": "mesh-feature", "scope_record": identity, "scope_base_record": {"class_tag": "256", "record_index": 104, "byte_offset": 270, "frame_length": 30},
        "collection_record": {"class_tag": "256", "record_index": 104, "byte_offset": 0, "frame_length": 95}, "collection_base_record": {"class_tag": "256", "record_index": 104, "byte_offset": 38, "frame_length": 57},
        "texture_table_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 29}, "body_count_offsets": [121, 21, 58],
        "body_record_indices": [104, 104], "scope_body_reference_offsets": [125, 136],
        "collection_body_reference_offsets": [62, 73], "texture_table_reference_offset": 27,
        "collection_owner_record": {"class_tag": "256", "record_index": 104, "byte_offset": 100, "frame_length": 273}, "collection_owner_reference_offset": 84,
        "collection_owner_backlink_offset": 362, "scope_owner_record_index": 109,
        "scope_owner_reference_offset": 289, "texture_flags_count_offset": 121,
        "texture_filename_count_offset": 125, "bodies": [body, body], "textures": []
    });
    for count in 0..=2 {
        let mut value = base.clone();
        for field in [
            "bodies",
            "body_record_indices",
            "scope_body_reference_offsets",
            "collection_body_reference_offsets",
        ] {
            value[field]
                .as_array_mut()
                .expect("wire array")
                .truncate(count);
        }
        value["collection_record"]["frame_length"] = serde_json::json!(73 + 11 * count);
        value["collection_base_record"]["frame_length"] = serde_json::json!(35 + 11 * count);
        value["collection_owner_reference_offset"] = serde_json::json!(62 + 11 * count);
        let wire: crate::records::mesh::DesignMeshFeatureWire =
            serde_json::from_value(value).expect("mesh wire");
        let expected = serde_json::to_string(&wire).expect("original mesh wire");
        let feature: crate::records::mesh::DesignMeshFeature =
            serde_json::from_str(&expected).expect("mesh body rows");
        assert_eq!(
            serde_json::to_string(&feature).expect("mesh wire"),
            expected
        );
    }
    for field in [
        "body_record_indices",
        "scope_body_reference_offsets",
        "collection_body_reference_offsets",
        "bodies",
    ] {
        let mut value = base.clone();
        value[field].as_array_mut().expect("wire array").pop();
        assert!(serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(value).is_err());
    }
    for field in [
        "body_count_offsets",
        "scope_body_reference_offsets",
        "collection_body_reference_offsets",
    ] {
        let mut value = base.clone();
        value[field][0] = serde_json::json!(0);
        let error = serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(value)
            .expect_err("misplaced body reference")
            .to_string();
        assert!(error.contains(field));
    }
    let mut wrong_count = base.clone();
    wrong_count["collection_record"]["frame_length"] = serde_json::json!(84);
    wrong_count["collection_base_record"]["frame_length"] = serde_json::json!(46);
    wrong_count["collection_owner_reference_offset"] = serde_json::json!(73);
    let error = serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(wrong_count)
        .expect_err("body count mismatch")
        .to_string();
    assert!(error.contains("bodies count"));
    let mut short_scope = base.clone();
    short_scope["scope_record"]["frame_length"] = serde_json::json!(76);
    short_scope["scope_base_record"]["byte_offset"] = serde_json::json!(146);
    short_scope["scope_owner_reference_offset"] = serde_json::json!(165);
    let error = serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(short_scope)
        .expect_err("body references overlap base")
        .to_string();
    assert!(error.contains("bodies reference run"));
    let mut changed_identity = base;
    changed_identity["body_record_indices"][0] = serde_json::json!(105);
    let error = serde_json::from_value::<crate::records::mesh::DesignMeshFeature>(changed_identity)
        .expect_err("conflicting body identity")
        .to_string();
    assert!(error.contains("body_record_indices"));
}

fn mesh_feature_wire_for_borrowed(count: usize, textures: bool) -> serde_json::Value {
    let identity = serde_json::json!({
        "class_tag":"256", "record_index":104, "byte_offset":100, "frame_length":200
    });
    let body = serde_json::json!({
        "body_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":575},
        "entry_name_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":62},
        "guid_record":identity,"wrapper_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":40},
        "scene_state_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":95},
        "scene_node_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":133},
        "scene_auxiliary_record":identity,"owner_record":identity,
        "entry_name":"mesh.paramesh","entry_name_offset":136,
        "fusion_uuid":"AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE","fusion_uuid_offset":136,
        "transform":[[1.0,0.0,0.0,0.0],[0.0,1.0,0.0,0.0],[0.0,0.0,1.0,0.0],[0.0,0.0,0.0,1.0]],
        "transform_offsets":[142,271],"scope_reference_offset":608,
        "wrapper_reference_offset":619,"owner_reference_offset":630,
        "guid_reference_offset":641,"scene_node_reference_offset":653,
        "collection_reference_offset":664,"wrapper_body_reference_offset":121,
        "entry_guid_reference_offset":121,"guid_entry_reference_offset":172,
        "scene_state_reference_offset":133,"scene_auxiliary_reference_offset":148
    });
    let texture_rows = if textures {
        let row = |ordinal, filename_ordinal, guid, flags_guid, filename_guid| {
            serde_json::json!({
                "ordinal":ordinal,"resource_guid":guid,"flags_guid_offset":flags_guid,
                "flags":7,"flags_offset":flags_guid + 36,
                "filename_ordinal":filename_ordinal,"filename_guid_offset":filename_guid,
                "filename_record":{"class_tag":"256","record_index":8,"byte_offset":300,"frame_length":35},
                "filename_record_reference_offset":filename_guid + 36,
                "filename":"a.png","filename_offset":325,
                "archive_entry_name":"Textures/a.png","asset":"test:model:asset#texture"
            })
        };
        serde_json::json!([
            row(1, 0, "BBBBBBBB-BBBB-4CCC-8DDD-EEEEEEEEEEEE", 173, 221),
            row(0, 1, "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE", 129, 272)
        ])
    } else {
        serde_json::json!([])
    };
    let body_indices = [104, 104];
    let scope_offsets = [125, 136];
    let collection_offsets = [62, 73];
    let body_rows = [body.clone(), body];
    serde_json::json!({
        "id":"f3d:native:mesh-feature#0", "scope_record":identity,
        "scope_base_record":{"class_tag":"256","record_index":104,"byte_offset":270,"frame_length":30},
        "collection_record":{"class_tag":"256","record_index":104,"byte_offset":0,"frame_length":73 + 11 * count},
        "collection_base_record":{"class_tag":"256","record_index":104,"byte_offset":38,"frame_length":35 + 11 * count},
        "texture_table_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":if textures {219} else {29}},
        "body_count_offsets":[121,21,58],
        "body_record_indices":&body_indices[..count],
        "scope_body_reference_offsets":&scope_offsets[..count],
        "collection_body_reference_offsets":&collection_offsets[..count],
        "texture_table_reference_offset":27,
        "collection_owner_record":{"class_tag":"256","record_index":104,"byte_offset":100,"frame_length":273},
        "collection_owner_reference_offset":62 + 11 * count,
        "collection_owner_backlink_offset":362,"scope_owner_record_index":109,
        "scope_owner_reference_offset":289,
        "texture_flags_count_offset":121,
        "texture_filename_count_offset":if textures {213} else {125},
        "bodies":&body_rows[..count], "textures":texture_rows
    })
}

#[test]
fn mesh_feature_borrowed_wire_matches_owned_wire_bytes() {
    for (count, textures) in [(0, false), (1, false), (2, true)] {
        let wire = mesh_feature_wire_for_borrowed(count, textures);
        let feature: super::DesignMeshFeature = serde_json::from_value(wire).unwrap();
        let owned = super::DesignMeshFeatureWire::from(feature.clone());
        assert_eq!(
            serde_json::to_vec(&feature).unwrap(),
            serde_json::to_vec(&owned).unwrap()
        );
    }
}

#[test]
fn mesh_feature_native_retained_limit_refuses_before_clone() {
    let feature: super::DesignMeshFeature =
        serde_json::from_value(mesh_feature_wire_for_borrowed(2, true)).unwrap();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &feature,
        "design_mesh_features",
        || super::MESH_FEATURE_CLONE_COUNT.with(|count| count.set(0)),
        || super::MESH_FEATURE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn mesh_scene_bounds_preserve_wire_and_check_corners_and_offsets() {
    let wire = r#"{"maximum":[1.0,2.0,3.0],"minimum":[-4.0,-5.0,-6.0],"offsets":[100,124]}"#;
    let bounds = crate::records::mesh::DesignMeshSceneBounds::from_wire(
        &serde_json::from_str(wire).unwrap(),
        [100, 124],
    )
    .unwrap();
    assert_eq!(
        serde_json::to_string(&bounds.into_wire([100, 124])).unwrap(),
        wire
    );
    assert_eq!(bounds.into_wire([100, 124]).offsets, [100, 124]);
    for (field, bad) in [
        ("minimum", serde_json::json!([2.0, -5.0, -6.0])),
        ("maximum", serde_json::json!([-5.0, 2.0, 3.0])),
        ("offsets", serde_json::json!([100, 125])),
        ("offsets", serde_json::json!([u64::MAX, 0])),
    ] {
        let mut invalid = serde_json::to_value(bounds.into_wire([100, 124])).unwrap();
        invalid[field] = bad;
        assert!(crate::records::mesh::DesignMeshSceneBounds::from_wire(
            &serde_json::from_value(invalid).unwrap(),
            [100, 124]
        )
        .unwrap_err()
        .clone()
        .contains(field));
    }
    assert!(crate::records::mesh::DesignMeshSceneBounds::new([0.0; 3], [0.0; 3]).is_ok());
    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(
            crate::records::mesh::DesignMeshSceneBounds::new([value, 1.0, 1.0], [0.0; 3]).is_err()
        );
        assert!(
            crate::records::mesh::DesignMeshSceneBounds::new([1.0; 3], [value, 0.0, 0.0]).is_err()
        );
    }
}

#[test]
fn mesh_record_identity_preserves_wire_and_rejects_invalid_headers() {
    let wire = r#"{"class_tag":"256","record_index":1,"byte_offset":100,"frame_length":11}"#;
    let record: crate::records::mesh::DesignMeshRecordIdentity =
        serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&record).unwrap(), wire);
    for (field, bad) in [
        ("class_tag", serde_json::json!("25x")),
        ("class_tag", serde_json::json!("2560")),
        ("record_index", serde_json::json!(0)),
        ("frame_length", serde_json::json!(10)),
        ("frame_length", serde_json::json!(u64::MAX)),
    ] {
        let mut invalid = serde_json::to_value(&record).unwrap();
        invalid[field] = bad;
        assert!(
            serde_json::from_value::<crate::records::mesh::DesignMeshRecordIdentity>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
}

#[test]
fn mesh_affine_transform_preserves_rows_and_rejects_invalid_maps() {
    let rows = [
        [-2.0, 0.0, 0.0, 3.0],
        [0.0, 4.0, 0.0, 5.0],
        [0.0, 0.0, 6.0, 7.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let value = crate::records::mesh::MeshAffineTransform::try_from(rows).unwrap();
    let json = serde_json::to_value(rows).unwrap();
    assert_eq!(serde_json::to_value(value).unwrap(), json);
    assert_eq!(
        serde_json::from_value::<crate::records::mesh::MeshAffineTransform>(json).unwrap(),
        value
    );
    for (row, column, invalid) in [(3, 0, 1.0), (3, 3, 0.0), (1, 1, 0.0)] {
        let mut changed = rows;
        changed[row][column] = invalid;
        assert!(
            serde_json::from_value::<crate::records::mesh::MeshAffineTransform>(
                serde_json::to_value(changed).unwrap()
            )
            .is_err()
        );
    }
    let mut cells = value.cells();
    cells[0] = f64::INFINITY;
    assert!(crate::records::mesh::MeshAffineTransform::new(cells).is_err());
    cells[0] = f64::MAX;
    assert!(crate::records::mesh::MeshAffineTransform::new(cells).is_err());
}

#[test]
fn mesh_affine_transform_hands_back_its_admitted_rows_and_keeps_the_bottom_row_spelling() {
    let rows = [
        [-2.0, 0.0, 0.0, 3.0],
        [0.0, 4.0, 0.0, 5.0],
        [0.0, 0.0, 6.0, 7.0],
        [-0.0, 0.0, -0.0, 1.0],
    ];
    let value = crate::records::mesh::MeshAffineTransform::try_from(rows).unwrap();
    assert_eq!(
        value
            .transform()
            .affine_rows()
            .map(|row| row.map(f64::to_bits)),
        [rows[0], rows[1], rows[2]].map(|row| row.map(f64::to_bits))
    );
    let cells: [f64; 16] = std::array::from_fn(|cell| rows[cell / 4][cell % 4]);
    assert_eq!(value.cells().map(f64::to_bits), cells.map(f64::to_bits));
    let wire = serde_json::to_string(&value).unwrap();
    assert_eq!(wire, serde_json::to_string(&rows).unwrap());
    let round_trip: crate::records::mesh::MeshAffineTransform =
        serde_json::from_str(&wire).unwrap();
    assert_eq!(
        round_trip.cells().map(f64::to_bits),
        cells.map(f64::to_bits)
    );
}

#[test]
fn mesh_texture_file_derives_basename_and_offset_without_wire_changes() {
    fn parse(
        wire: serde_json::Value,
    ) -> Result<crate::records::mesh::DesignMeshTextureTable, String> {
        let record = crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned())?,
            4,
            0,
            124,
        )?;
        crate::records::mesh::DesignMeshTextureTable::from_wire(
            record,
            21,
            69,
            vec![serde_json::from_value(wire).map_err(|error| error.to_string())?],
        )
    }
    let wire = serde_json::json!({
        "ordinal": 0, "resource_guid": "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "flags_guid_offset": 29, "flags": 7, "flags_offset": 65,
        "filename_ordinal": 0, "filename_guid_offset": 77,
        "filename_record": {"class_tag": "256", "record_index": 8, "byte_offset": 200, "frame_length": 39},
        "filename_record_reference_offset": 113,
        "filename": "é😀.png", "filename_offset": 225,
        "archive_entry_name": "Textures/é😀.png", "asset": "test:model:asset#texture"
    });
    // The basename has seven UTF-16 code units, including the surrogate pair.
    let table = parse(wire.clone()).unwrap();
    let resource = &table.resources()[0];
    assert_eq!(resource.file.filename(), "é😀.png");
    assert_eq!(resource.file.filename_offset(), 225);
    assert_eq!(serde_json::to_value(&table.into_wire().3[0]).unwrap(), wire);
    for field in ["filename", "archive_entry_name", "filename_offset"] {
        let mut bad = wire.clone();
        bad[field] = if field == "filename_offset" {
            226.into()
        } else {
            "other.png".into()
        };
        assert!(parse(bad).is_err());
    }
    let mut bad = wire.clone();
    bad["filename_record"]["frame_length"] = 38.into();
    assert!(parse(bad).is_err());
    let mut bad = wire;
    bad["filename_record"]["byte_offset"] = (u64::MAX - 20).into();
    assert!(parse(bad).is_err());
}

#[test]
fn design_guid_text_preserves_case_and_rejects_non_guids() {
    let wire = "\"aAaAaAaA-bBbB-4cCc-8dDd-eEeEeEeEeEeE\"";
    let value: crate::records::mesh::DesignGuidText = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), wire);
    for invalid in [
        "",
        "AAAAAAAA_BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "GAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
    ] {
        assert!(crate::records::mesh::DesignGuidText::try_from(invalid.to_owned()).is_err());
    }
}

#[test]
fn mesh_guid_record_requires_prefix_and_derives_join_offsets() {
    let guid = crate::records::mesh::DesignGuidText::try_from(
        "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE".to_owned(),
    )
    .unwrap();
    for frame_length in [83, 100] {
        let identity = crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            200,
            frame_length,
        )
        .unwrap();
        let record = crate::records::mesh::DesignMeshGuid::new(identity, guid.clone()).unwrap();
        assert_eq!(record.value_offset(), 236);
        assert_eq!(record.entry_reference_offset(), 272);
        assert_eq!(record.record().frame_length(), frame_length);
    }
    let short = crate::records::mesh::DesignMeshRecordIdentity::new(
        crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        4,
        200,
        82,
    )
    .unwrap();
    assert!(crate::records::mesh::DesignMeshGuid::new(short, guid).is_err());
}

#[test]
fn mesh_uuid_preserves_wire_and_requires_lowercase_version_four() {
    let text = "aaaaaaaa-bbbb-4ccc-8ddd-eeeeeeeeeeee";
    let value = crate::records::mesh::DesignMeshUuid::try_from(text.to_owned()).unwrap();
    assert_eq!(value.as_str(), text);
    assert_eq!(
        serde_json::to_value(&value).unwrap(),
        serde_json::json!(text)
    );
    assert_eq!(
        serde_json::from_value::<crate::records::mesh::DesignMeshUuid>(serde_json::json!(text))
            .unwrap(),
        value
    );
    for invalid in [
        "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE",
        "aaaaaaaa-bbbb-3ccc-8ddd-eeeeeeeeeeee",
        "aaaaaaaa-bbbb-4ccc-7ddd-eeeeeeeeeeee",
        "",
    ] {
        assert!(
            serde_json::from_value::<crate::records::mesh::DesignMeshUuid>(serde_json::json!(
                invalid
            ))
            .is_err()
        );
    }
}

#[test]
fn mesh_entry_name_layout_uses_utf16_units_and_exact_record_end() {
    let identity = |length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    let entry =
        crate::records::mesh::DesignMeshEntryName::new(identity(42), "a😀".to_owned()).unwrap();
    assert_eq!(entry.name_offset(), 136);
    assert_eq!(entry.guid_reference_offset(), 121);
    assert_eq!(entry.name(), "a😀");
    assert!(
        crate::records::mesh::DesignMeshEntryName::new(identity(40), "a😀".to_owned()).is_err()
    );
    assert!(
        crate::records::mesh::DesignMeshEntryName::new(identity(44), "a😀".to_owned()).is_err()
    );
    assert!(crate::records::mesh::DesignMeshEntryName::new(identity(36), String::new()).is_err());
}

#[test]
fn mesh_placement_layout_requires_prefix_and_terminal_reference() {
    let transform = crate::records::mesh::MeshAffineTransform::try_from([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    let identity = |length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    for length in [575, 800] {
        let placement =
            crate::records::mesh::DesignMeshPlacement::new(identity(length), transform).unwrap();
        assert_eq!(placement.transform_offsets(), [142, 271]);
        assert_eq!(placement.scope_reference_offset(), 608);
        assert_eq!(placement.wrapper_reference_offset(), 619);
        assert_eq!(placement.owner_reference_offset(), 630);
        assert_eq!(placement.guid_reference_offset(), 641);
        assert_eq!(placement.scene_node_reference_offset(), 653);
        assert_eq!(placement.collection_reference_offset(), 100 + length - 11);
    }
    assert!(crate::records::mesh::DesignMeshPlacement::new(identity(574), transform).is_err());
}

#[test]
fn mesh_fixed_record_derives_length_and_rejects_another_layout() {
    let identity = |length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    let wrapper =
        crate::records::mesh::DesignMeshFixedRecord::<40>::try_from(identity(40)).unwrap();
    assert_eq!(wrapper.byte_offset(), 100);
    assert_eq!(wrapper.record_index(), 4);
    let roundtrip: crate::records::mesh::DesignMeshRecordIdentity = wrapper.into();
    assert_eq!(roundtrip, identity(40));
    assert!(crate::records::mesh::DesignMeshFixedRecord::<40>::try_from(identity(95)).is_err());
    assert!(crate::records::mesh::DesignMeshFixedRecord::<95>::try_from(identity(40)).is_err());
}

#[test]
fn mesh_scene_forms_derive_bounds_and_transform_locations() {
    let identity = |length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            200,
            length,
        )
        .unwrap()
    };
    let bounds =
        crate::records::mesh::DesignMeshSceneBounds::new([1.0, 2.0, 3.0], [-1.0, -2.0, -3.0])
            .unwrap();
    let transform = crate::records::mesh::MeshAffineTransform::try_from([
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 1.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ])
    .unwrap();
    for (length, placement, offsets) in
        [(133, None, [284, 308]), (261, Some(transform), [412, 436])]
    {
        let node = crate::records::mesh::DesignMeshSceneNode::new(
            identity(length),
            Some(bounds),
            placement,
        )
        .unwrap();
        assert_eq!(node.bounds_offsets(), offsets);
        assert_eq!(node.state_reference_offset(), 233);
        assert_eq!(node.auxiliary_reference_offset(), 248);
        assert_eq!(
            node.transform().map(|located| located.offset),
            placement.map(|_| 284)
        );
        let (record, bound_wire, transform_wire) = node.clone().into_wire();
        assert_eq!(record, identity(length));
        assert_eq!(bound_wire.as_ref().unwrap().offsets, offsets);
        assert_eq!(
            crate::records::mesh::DesignMeshSceneNode::from_wire(
                record.clone(),
                bound_wire.clone(),
                transform_wire
            )
            .unwrap(),
            node
        );
        let mut bad_bounds = bound_wire.unwrap();
        bad_bounds.offsets[0] += 1;
        assert!(crate::records::mesh::DesignMeshSceneNode::from_wire(
            record,
            Some(bad_bounds),
            transform_wire
        )
        .is_err());
    }
    assert!(
        crate::records::mesh::DesignMeshSceneNode::new(identity(133), None, Some(transform))
            .is_err()
    );
    assert!(crate::records::mesh::DesignMeshSceneNode::new(identity(261), None, None).is_err());
    assert!(crate::records::mesh::DesignMeshSceneNode::from_wire(
        identity(261),
        None,
        Some(crate::records::identity::Located {
            value: transform,
            offset: 285
        })
    )
    .is_err());
    let state = crate::records::mesh::DesignMeshSceneState::new(
        identity(95).try_into().unwrap(),
        Some(bounds),
    );
    let (record, bound_wire) = state.clone().into_wire();
    assert_eq!(bound_wire.as_ref().unwrap().offsets, [246, 270]);
    assert_eq!(
        crate::records::mesh::DesignMeshSceneState::from_wire(record.clone(), bound_wire.clone())
            .unwrap(),
        state
    );
    let mut bad_bounds = bound_wire.unwrap();
    bad_bounds.offsets = [247, 271];
    assert!(
        crate::records::mesh::DesignMeshSceneState::from_wire(record, Some(bad_bounds)).is_err()
    );
}

#[test]
fn mesh_collection_owner_derives_fixed_and_terminal_backlinks() {
    let identity = |length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            100,
            length,
        )
        .unwrap()
    };
    for (length, relative) in [
        (252, 241),
        (400, 241),
        (273, 262),
        (400, 262),
        (200, 189),
        (400, 389),
    ] {
        let owner =
            crate::records::mesh::DesignMeshCollectionOwner::new(identity(length), 100 + relative)
                .unwrap();
        assert_eq!(owner.backlink_offset(), 100 + relative);
        assert_eq!(owner.record(), &identity(length));
        assert_eq!(
            crate::records::mesh::DesignMeshCollectionOwner::new(
                owner.record().clone(),
                owner.backlink_offset()
            )
            .unwrap(),
            owner
        );
    }
    for (length, offset) in [(250, 341), (272, 362), (400, 99), (400, 488)] {
        assert!(
            crate::records::mesh::DesignMeshCollectionOwner::new(identity(length), offset).is_err()
        );
    }
}

#[test]
fn mesh_texture_table_checks_permutations_and_preserves_wire_row_order() {
    const GUID_A: &str = "AAAAAAAA-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
    const GUID_B: &str = "BBBBBBBB-BBBB-4CCC-8DDD-EEEEEEEEEEEE";
    let record = |length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
            4,
            0,
            length,
        )
        .unwrap()
    };
    let row = |ordinal, filename_ordinal, guid, flags_guid, filename_guid| {
        serde_json::json!({
            "ordinal": ordinal, "resource_guid": guid, "flags_guid_offset": flags_guid,
            "flags": 7, "flags_offset": flags_guid + 36,
            "filename_ordinal": filename_ordinal, "filename_guid_offset": filename_guid,
            "filename_record": {"class_tag": "256", "record_index": 8, "byte_offset": 300, "frame_length": 35},
            "filename_record_reference_offset": filename_guid + 36,
            "filename": "a.png", "filename_offset": 325,
            "archive_entry_name": "Textures/a.png", "asset": "test:model:asset#texture"
        })
    };
    let rows = serde_json::json!([row(1, 0, GUID_B, 73, 121), row(0, 1, GUID_A, 29, 172)]);
    let parse = |rows: serde_json::Value| {
        crate::records::mesh::DesignMeshTextureTable::from_wire(
            record(219),
            21,
            113,
            serde_json::from_value(rows).unwrap(),
        )
    };
    let table = parse(rows.clone()).unwrap();
    assert_eq!(
        table
            .resources_in_flags_order()
            .iter()
            .map(|resource| resource.resource_guid.as_str())
            .collect::<Vec<_>>(),
        [GUID_A, GUID_B]
    );
    let (identity, flags_count, filename_count, encoded) = table.into_wire();
    assert_eq!(identity, record(219));
    assert_eq!((flags_count, filename_count), (21, 113));
    assert_eq!(serde_json::to_value(encoded).unwrap(), rows);
    let mut duplicate = rows.clone();
    duplicate[0]["ordinal"] = 0.into();
    duplicate[0]["flags_guid_offset"] = 29.into();
    duplicate[0]["flags_offset"] = 65.into();
    assert!(parse(duplicate).unwrap_err().contains("ordinal"));
    let mut duplicate = rows.clone();
    duplicate[0]["filename_ordinal"] = 1.into();
    duplicate[0]["filename_guid_offset"] = 172.into();
    duplicate[0]["filename_record_reference_offset"] = 208.into();
    assert!(parse(duplicate).unwrap_err().contains("filename_ordinal"));
    let mut duplicate = rows.clone();
    duplicate[0]["resource_guid"] = GUID_A.to_ascii_lowercase().into();
    assert!(parse(duplicate).unwrap_err().contains("resource_guid"));
    let mut out_of_range = rows.clone();
    out_of_range[0]["ordinal"] = 2.into();
    out_of_range[0]["flags_guid_offset"] = 117.into();
    out_of_range[0]["flags_offset"] = 153.into();
    assert!(parse(out_of_range).unwrap_err().contains("ordinal"));
    for field in [
        "flags_guid_offset",
        "flags_offset",
        "filename_guid_offset",
        "filename_record_reference_offset",
    ] {
        let mut bad = rows.clone();
        bad[0][field] = (u64::MAX - 35).into();
        assert!(parse(bad).is_err());
    }
    assert!(crate::records::mesh::DesignMeshTextureTable::from_wire(
        record(218),
        21,
        113,
        serde_json::from_value(rows.clone()).unwrap()
    )
    .is_err());
    assert!(crate::records::mesh::DesignMeshTextureTable::from_wire(
        record(219),
        21,
        112,
        serde_json::from_value(rows).unwrap()
    )
    .is_err());
    assert!(crate::records::mesh::DesignMeshTextureTable::new(record(29), Vec::new()).is_ok());
}

#[test]
fn mesh_scope_constructs_only_same_index_closing_bases() {
    let identity = |index, offset, length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned())
                .expect("class tag"),
            index,
            offset,
            length,
        )
        .expect("record identity")
    };
    let scope = crate::records::mesh::DesignMeshScope::new(
        identity(104, 100, 200),
        identity(104, 270, 30),
        109,
    )
    .expect("scope");
    assert_eq!(scope.base_record(), identity(104, 270, 30));
    assert_eq!(scope.owner_reference_offset(), 289);
    assert_eq!(scope.owner_record_index(), 109);
    for base in [
        identity(105, 270, 30),
        identity(104, 269, 30),
        identity(104, 270, 31),
    ] {
        assert!(
            crate::records::mesh::DesignMeshScope::new(identity(104, 100, 200), base, 109).is_err()
        );
    }
    assert!(crate::records::mesh::DesignMeshScope::new(
        identity(104, 100, 200),
        identity(104, 270, 30),
        0
    )
    .is_err());
    assert!(crate::records::mesh::DesignMeshScope::new(
        identity(104, 100, 54),
        identity(104, 124, 30),
        109
    )
    .is_err());
}

#[test]
fn mesh_collection_constructs_only_complete_nested_body_runs() {
    let identity = |index, offset, length| {
        crate::records::mesh::DesignMeshRecordIdentity::new(
            crate::records::references::DesignClassTag::try_from("256".to_owned())
                .expect("class tag"),
            index,
            offset,
            length,
        )
        .expect("record identity")
    };
    for count in [0, 1, 2, u64::from(u32::MAX)] {
        let length = 73 + 11 * count;
        let collection = crate::records::mesh::DesignMeshCollection::new(
            identity(104, 100, length),
            identity(104, 138, length - 38),
        )
        .expect("collection");
        assert_eq!(collection.base_record(), identity(104, 138, length - 38));
        assert_eq!(collection.body_count(), count);
        assert_eq!(collection.texture_table_reference_offset(), 127);
        assert_eq!(collection.owner_reference_offset(), 100 + length - 11);
    }
    for base in [
        identity(105, 138, 57),
        identity(104, 137, 57),
        identity(104, 138, 58),
    ] {
        assert!(
            crate::records::mesh::DesignMeshCollection::new(identity(104, 100, 95), base).is_err()
        );
    }
    for length in [72, 74, 73 + 11 * (u64::from(u32::MAX) + 1)] {
        assert!(crate::records::mesh::DesignMeshCollection::new(
            identity(104, 100, length),
            identity(104, 138, length - 38)
        )
        .is_err());
    }
}

#[test]
fn relaxed_guid_text_accepts_relaxed_only_value() {
    let wire = "\"GAAAAAAA_BBBB-4CCC-8DDD-EEEEEEEEEEEE\"";
    let value: crate::records::mesh::DesignRelaxedGuidText = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&value).unwrap(), wire);
}

#[test]
fn strict_guid_text_rejects_relaxed_only_value() {
    assert!(
        serde_json::from_str::<crate::records::mesh::DesignGuidText>(
            "\"GAAAAAAA_BBBB-4CCC-8DDD-EEEEEEEEEEEE\""
        )
        .is_err()
    );
}
