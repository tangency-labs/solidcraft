// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::patterns::{
    DesignPathPatternSettings, PATH_PATTERN_SETTINGS_TYPE, PATH_PATTERN_START_TYPE,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;

fn settings() -> DesignPathPatternSettings {
    DesignPathPatternSettings {
        settings_record_index: 322,
        settings_byte_offset: 1_000,
        total_extent: Located {
            value: false,
            offset: 1_156,
        },
        flipped: Located {
            value: false,
            offset: 1_080,
        },
        path_oriented: Located {
            value: false,
            offset: 1_081,
        },
        path_group_record_index: 333,
        start_record_index: 319,
        start_parameter: Located {
            value: cadmpeg_ir::scalar::FiniteReal::ZERO,
            offset: 2_025,
        },
    }
}

fn native(
    settings: DesignPathPatternSettings,
    members: Vec<u32>,
    start_version: u32,
) -> crate::native::F3dNative {
    use crate::records::entity_header::{BaseTypeGuid, SegmentType, SegmentTypeData};
    let types = [
        (PATH_PATTERN_SETTINGS_TYPE.0, PATH_PATTERN_SETTINGS_TYPE.1),
        (PATH_PATTERN_START_TYPE.0, start_version),
    ];
    let mut scope = DesignParameterScope::empty(
        "f3d:Design1/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::PathPattern,
        315,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(members);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::PathPattern(slot) =
        scope.payload_mut()
    {
        *slot = Some(settings);
    }
    crate::native::F3dNative {
        design_types: types
            .into_iter()
            .enumerate()
            .map(|(offset, (guid, version))| {
                SegmentType::try_new(
                    format!("f3d:Design1/MetaStream.dat:design-type#{offset}"),
                    SegmentTypeData {
                        byte_offset: cadmpeg_core::decode::u64_from_index(offset),
                        type_guid: guid.to_owned().try_into().unwrap(),
                        type_guid_offset: 4,
                        base_type_guid: BaseTypeGuid::Absent,
                        version,
                        version_offset: 44,
                        module: "Fusion".into(),
                        entities: crate::records::identity::ReferenceRun::unlocated(Vec::new()),
                    },
                )
                .unwrap()
            })
            .collect(),
        design_record_headers: [
            (322, "256", 1_000),
            (319, "257", 2_000),
            (319, "258", 2_197),
        ]
        .into_iter()
        .map(
            |(record_index, class_tag, byte_offset)| crate::records::decal::DesignRecordHeader {
                id: format!("f3d:Design1/BulkStream.dat:design-record-header#{byte_offset}"),
                record_index,
                class_tag: class_tag.to_owned().try_into().unwrap(),
                byte_offset,
            },
        )
        .collect(),
        design_parameter_scopes: vec![scope],
        ..Default::default()
    }
}

fn finding_count(native: &crate::native::F3dNative) -> usize {
    crate::test_support::with_decode_context(|decode| {
        let ir = cadmpeg_ir::CadIr::empty();
        let ctx = crate::validate::Ctx::new(&ir, native, decode);
        let mut findings = Vec::new();
        super::validate(&ctx, &mut findings);
        findings.len()
    })
}

#[test]
fn path_pattern_settings_link_their_typed_records_and_offsets() {
    assert_eq!(finding_count(&native(settings(), vec![319, 333], 3)), 0);
    let mut moved_flag = settings();
    moved_flag.flipped.offset = 1_079;
    assert_eq!(finding_count(&native(moved_flag, vec![319, 333], 3)), 1);
    let mut moved_start = settings();
    moved_start.start_parameter.offset = 2_026;
    assert_eq!(finding_count(&native(moved_start, vec![319, 333], 3)), 1);
    assert_eq!(finding_count(&native(settings(), vec![333], 3)), 1);
    assert_eq!(finding_count(&native(settings(), vec![319], 3)), 1);
    assert_eq!(finding_count(&native(settings(), vec![319, 333], 4)), 1);
    let mut moved_settings = settings();
    moved_settings.settings_byte_offset = 1_001;
    assert_eq!(finding_count(&native(moved_settings, vec![319, 333], 3)), 1);
    let mut unknown_settings = settings();
    unknown_settings.settings_record_index = 321;
    assert_eq!(
        finding_count(&native(unknown_settings, vec![319, 333], 3)),
        1
    );
}
