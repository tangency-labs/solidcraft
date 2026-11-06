// SPDX-License-Identifier: Apache-2.0

fn native() -> crate::native::F3dNative {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    let types = [
        (super::SETTINGS_GUID, 2, vec![50]),
        (super::RULE_GUID, 0, vec![51]),
        (super::PRESENTATION_GUID, 0, vec![52]),
    ];
    crate::native::F3dNative {
        design_types: types.into_iter().enumerate().map(|(offset, (guid, version, entities))| {
            SegmentType::try_new(format!("f3d:Design1/MetaStream.dat:design-type#{offset}"), SegmentTypeData {
                byte_offset: cadmpeg_core::decode::u64_from_index(offset), type_guid: guid.to_owned().try_into().unwrap(), type_guid_offset: 4,
                base_type_guid: BaseTypeGuid::Absent, version, version_offset: 44, module: "Fusion".into(),
                entities: crate::records::identity::ReferenceRun::unlocated(entities),
            }).unwrap()
        }).collect(),
        design_record_headers: [50, 51, 52, 53, 54].into_iter().map(|index| crate::records::decal::DesignRecordHeader {
            id: format!("f3d:Design1/BulkStream.dat:design-record-header#{index}"), record_index: index,
            class_tag: "256".to_owned().try_into().unwrap(), byte_offset: if index == 50 {20} else {1000 + u64::from(index)},
        }).collect(),
        design_sheet_metal_settings: vec![serde_json::from_value(serde_json::json!({
            "id": "f3d:Design1/BulkStream.dat:design-sheet-metal-settings#20", "record_index": 50,
            "byte_offset": 20, "paired_byte_offset": 100, "mask": 4, "inputs": {"version": "v2", "slots": [null, 51, null, null, null, null, null]},
            "rule_record_index": 51, "presentation_record_index": 52, "owner_record_index": 53, "context_record_index": 54,
        })).unwrap()],
        ..Default::default()
    }
}

#[test]
fn settings_native_links_require_the_retained_v3_tail_width() {
    use crate::records::sheet_metal_settings::{
        DesignSheetMetalSettingsInputs, V3RuleReferenceTail,
    };
    for (tail, paired_offset) in [
        (V3RuleReferenceTail::ZeroPair, 90),
        (V3RuleReferenceTail::OneZeroPair, 91),
    ] {
        for offset_delta in [0, 1] {
            let mut native = native();
            native.design_types[0].set_version(3);
            native.design_sheet_metal_settings[0].inputs = DesignSheetMetalSettingsInputs::V3 {
                slots: [None; 9],
                rule_reference_tail: tail,
            };
            native.design_sheet_metal_settings[0].paired_byte_offset = paired_offset + offset_delta;
            crate::test_support::with_decode_context(|decode| {
                let ir = cadmpeg_ir::CadIr::empty();
                let ctx = crate::validate::Ctx::new(&ir, &native, decode);
                let mut findings = Vec::new();
                super::validate(&ctx, &mut findings);
                assert_eq!(findings.len(), usize::from(offset_delta != 0));
            });
        }
    }
}

#[test]
fn settings_native_links_reject_missing_references_wrong_types_and_duplicate_carriers() {
    for case in 0..10 {
        let mut native = native();
        match case {
            0 => {}
            1 => native.design_sheet_metal_settings[0].owner_record_index = 99,
            2 => native.design_sheet_metal_settings[0].rule_record_index = 52,
            3 => {
                native.design_sheet_metal_settings[0].inputs =
                    crate::records::sheet_metal_settings::DesignSheetMetalSettingsInputs::V3 {
                        slots: [None; 9],
                        rule_reference_tail:
                            crate::records::sheet_metal_settings::V3RuleReferenceTail::OneZeroPair,
                    };
            }
            4 => native
                .design_sheet_metal_settings
                .push(native.design_sheet_metal_settings[0].clone()),
            5 => native.design_record_headers[0].byte_offset = 21,
            6 => native.design_sheet_metal_settings[0].paired_byte_offset = 21,
            7 => native.design_record_headers[0].class_tag = "257".to_owned().try_into().unwrap(),
            8 => native.design_types.reverse(),
            9 => native.design_types.push(native.design_types[0].clone()),
            _ => unreachable!(),
        }
        crate::test_support::with_decode_context(|decode| {
            let ir = cadmpeg_ir::CadIr::empty();
            let ctx = crate::validate::Ctx::new(&ir, &native, decode);
            let mut findings = Vec::new();
            super::validate(&ctx, &mut findings);
            assert_eq!(
                findings.len(),
                usize::from(case != 0 && case != 8),
                "case {case}"
            );
        });
    }
}
