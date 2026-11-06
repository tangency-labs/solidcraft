// SPDX-License-Identifier: Apache-2.0

#[test]
fn fixture_attributed_frames_retain_reference_namespaces_and_independent_ordinals() {
    #[derive(serde::Deserialize)]
    struct Case {
        label: String,
        source_hex: cadmpeg_ir::native::bytes::NativeBytes,
        component: u64,
        ordinal: u32,
        path_discriminator: u32,
        component_guid: String,
        occurrence_key: String,
        key_record_index: u64,
        key_segment_index: u32,
        owner: u64,
        matrix: Option<[f64; 16]>,
    }
    let cases: Vec<Case> = serde_json::from_str(&crate::test_support::verification::text(
        "xref/attributed_records.json",
        include_str!("../../../../test_support/xref/attributed_records.json"),
    ))
    .unwrap();
    for case in cases {
        let occurrence = super::super::attributed_component_occurrence(
            case.source_hex.as_ref(),
            0,
            "f3d:Design/BulkStream.dat",
        )
        .unwrap_or_else(|| panic!("{}", case.label));
        assert_eq!(
            occurrence.component_record_index, case.component,
            "{}",
            case.label
        );
        assert_eq!(
            occurrence.occurrence_ordinal(),
            case.ordinal,
            "{}",
            case.label
        );
        assert_eq!(
            occurrence.component_guid.as_str(),
            case.component_guid,
            "{}",
            case.label
        );
        assert_eq!(
            occurrence.occurrence_guid.as_str(),
            case.occurrence_key,
            "{}",
            case.label
        );
        assert_eq!(occurrence.owner_entity, case.owner, "{}", case.label);
        assert_eq!(
            occurrence
                .transform()
                .map(|matrix| matrix.value.rows().concat()),
            case.matrix.map(|matrix| matrix.to_vec()),
            "{}",
            case.label
        );
        let wire = serde_json::to_value(&occurrence).unwrap();
        assert_eq!(
            wire["attributed"]["path_discriminator"], case.path_discriminator,
            "{}",
            case.label
        );
        assert_eq!(
            wire["attributed"]["key_record_index"], case.key_record_index,
            "{}",
            case.label
        );
        assert_eq!(
            wire["attributed"]["key_segment_index"], case.key_segment_index,
            "{}",
            case.label
        );
        let decoded: crate::records::feature::assembly_features::DesignComponentOccurrence =
            serde_json::from_value(wire).unwrap();
        assert_eq!(decoded, occurrence, "{}", case.label);
    }
}
