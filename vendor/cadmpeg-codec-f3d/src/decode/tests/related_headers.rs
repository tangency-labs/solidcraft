// SPDX-License-Identifier: Apache-2.0
//! Indexed records whose primary headers the decoder retains.

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload};
use crate::records::feature::thread::{DesignThreadConstruction, DesignThreadForm};

/// A reference-bearing compact Thread trailer names an indexed record outside
/// the scope reference table. Its primary header is retained with the scope
/// references, so the trailer reference resolves.
#[test]
fn compact_thread_trailer_record_is_a_related_record() {
    let construction = DesignThreadConstruction {
        form: DesignThreadForm::Compact(Some(crate::records::identity::Located {
            value: std::num::NonZeroU32::new(2075).unwrap(),
            offset: 144,
        })),
        designation_offset: 38,
        designation: cadmpeg_core::text::NonBlankString::new("M3x0.5").unwrap(),
        nominal_size: crate::records::feature::thread::DesignThreadNominalSize::try_from(
            "3.0".to_owned(),
        )
        .unwrap(),
        profile: cadmpeg_core::text::NonBlankString::new("ISO Metric profile").unwrap(),
        pitch: cadmpeg_ir::scalar::PositiveReal::new(0.05).unwrap(),
        face_group_record_indices: vec![988],
        diameters: crate::records::feature::thread::DesignThreadDiameters::new(
            0.3086, 0.2529, 0.2725,
        )
        .unwrap(),
        extended: None,
    };
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#100",
        DesignFeatureKind::Thread,
        987,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 200;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![988, 989]);
            draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.payload = DesignScopePayload::Thread(Some(construction));
        })
        .unwrap();
    let indices = super::super::design_intent::scope_related_indices(&[scope]);
    let stream = "f3d:Design/BulkStream.dat".to_owned();
    assert_eq!(
        indices,
        vec![(stream.clone(), 988), (stream.clone(), 989), (stream, 2075)]
    );
}

#[test]
fn extended_compact_thread_retains_the_hole_header_outside_its_reference_table() {
    let construction: DesignThreadConstruction = serde_json::from_value(serde_json::json!({
        "form": "compact_extended", "designation_offset": 46,
        "designation": "M8x1", "nominal_size_text": "8.0", "nominal_size": 8.0,
        "profile": "ISO Metric profile", "major_diameter": 0.8,
        "minor_diameter": 0.6, "pitch_diameter": 0.7, "pitch": 0.1,
        "face_group_record_indices": [109],
        "trailing_reference_record_index": 90, "trailing_reference_offset": 160
    }))
    .unwrap();
    let mut scope = DesignParameterScope::empty(
        "f3d:test:design-parameter-scope#100",
        DesignFeatureKind::Thread,
        100,
    );
    scope
        .try_edit(|draft| {
            draft.frame_length = 500;
            draft.paired_byte_offset = 500;
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![101, 104, 109, 112]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
            draft.payload = DesignScopePayload::Thread(Some(construction));
        })
        .unwrap();
    let indices = super::super::design_intent::scope_related_indices(&[scope]);
    assert_eq!(
        indices
            .into_iter()
            .collect::<std::collections::BTreeSet<_>>(),
        std::collections::BTreeSet::from(
            [90u32, 101, 104, 109, 112].map(|index| ("f3d:test".to_owned(), index))
        )
    );
}

/// A legacy owner stores scope `0` as a sentinel; the owner's related records
/// are its parameter and companion, and an ordinary owner adds its scope.
#[test]
fn owner_related_records_skip_the_zero_scope_sentinel() {
    let owner = |record_index: u32, scope_record_index: u32, frame_length: u64| {
        crate::records::parameters::DesignParameterOwner::try_from(
            crate::records::parameters::DesignParameterOwnerWire {
                id: format!("f3d:Design/BulkStream.dat:design-parameter-owner#{record_index}"),
                byte_offset: u64::from(record_index) * 200,
                frame_length,
                class_tag: crate::records::references::DesignClassTag::try_from("385".to_owned())
                    .unwrap(),
                record_index,
                scope_record_index,
                local_ordinal: 0,
                evaluated_value: 0.06,
                evaluated_value_offset: u64::from(record_index) * 200 + 40,
                parameter_record_index: record_index + 1,
                owned_ordinal: 0,
                variant: None,
                companion_record_index: record_index + 2,
            },
        )
        .unwrap()
    };
    let indices = super::super::design_intent::owner_related_indices(&[
        owner(251, 0, 68),
        owner(300, 207, 103),
    ]);
    let stream = "f3d:Design/BulkStream.dat".to_owned();
    assert_eq!(
        indices,
        [
            (stream.clone(), 252),
            (stream.clone(), 253),
            (stream.clone(), 207),
            (stream.clone(), 301),
            (stream, 302)
        ]
    );
}
