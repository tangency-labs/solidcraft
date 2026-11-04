// SPDX-License-Identifier: Apache-2.0

#[cfg(target_pointer_width = "64")]
fn refuses_wrapped_offset(
    mutate: impl FnOnce(&mut crate::design::decode::sketch::ParsedSketchRelation),
    operation: &str,
) {
    use crate::records::decal::DesignRecordHeader;
    use crate::records::sketch_relations::SketchRelationDefinition;

    let payload = super::relation_record(&[(300, 0)], &[], 201, 1, &[300]);
    let mut parsed =
        super::tested_parse_classed_sketch_relation(&payload, super::SketchRelationClass::Plain)
            .expect("plain fixture relation parses");
    mutate(&mut parsed);
    let header = DesignRecordHeader {
        id: "f3d:BulkStream.dat:design-record-header#0".to_owned(),
        record_index: 7,
        class_tag: "298".to_owned().try_into().expect("fixture class tag"),
        byte_offset: 0,
    };
    let definition = SketchRelationDefinition::new(parsed.state, None)
        .expect("fixture constraint mask is admitted");
    let mut output = Vec::new();
    let error = super::admit_sketch_relation(
        &mut output,
        "BulkStream.dat",
        &header,
        &payload,
        parsed,
        definition,
    )
    .expect_err("offset above u32 must be refused before it wraps");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(message)
            if message.contains(operation)));
    assert!(output.is_empty());
}

#[test]
#[cfg(target_pointer_width = "64")]
fn sketch_relation_member_offset_refuses_u32_wrap() {
    refuses_wrapped_offset(
        |parsed| parsed.members[0].reference.offset += 1_usize << 32,
        "f3d sketch relation member offset",
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn sketch_relation_return_offset_refuses_u32_wrap() {
    refuses_wrapped_offset(
        |parsed| parsed.return_members[0].offset += 1_usize << 32,
        "f3d sketch relation return offset",
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn sketch_relation_auxiliary_offset_refuses_u32_wrap() {
    refuses_wrapped_offset(
        |parsed| {
            parsed
                .auxiliary_references
                .push(crate::records::identity::Located {
                    value: 300,
                    offset: 1_usize << 32,
                });
        },
        "f3d sketch relation auxiliary offset",
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn sketch_relation_state_offset_refuses_u32_wrap() {
    refuses_wrapped_offset(
        |parsed| parsed.state_offset += 1_usize << 32,
        "f3d sketch relation state offset",
    );
}

#[test]
#[cfg(target_pointer_width = "64")]
fn sketch_relation_owner_offset_refuses_u32_wrap() {
    refuses_wrapped_offset(
        |parsed| parsed.owner_reference_offset += 1_usize << 32,
        "f3d sketch relation owner offset",
    );
}
