// SPDX-License-Identifier: Apache-2.0
//! Reference-aware to-face Extrude frames that omit a zero `Side1Offset`.

use crate::design::face_resolve::extrude_omits_zero_side_one_offset;
use crate::records::feature::extrude::{
    DesignExtrudeExtent, DesignExtrudeOperation, DesignExtrudePrologue, DesignExtrudeStart,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

fn prologue(extent: DesignExtrudeExtent) -> DesignExtrudePrologue {
    DesignExtrudePrologue::ReferenceAware {
        reference: None,
        operation: DesignExtrudeOperation::Cut,
        operation_offset: 28,
        direction_face_extend_values: [1, 1],
        side_extent_discriminators: [2, 0],
        side_extent_discriminator_offsets: [77, 90],
        first_side_target_ordinal: None,
        extent,
        direction_face_extend_offsets: [32, 36],
        direction_reversed: true,
        direction_reversed_offset: 40,
        solid_operation: true,
        solid_operation_offset: 41,
        start: DesignExtrudeStart::ProfilePlane,
        start_offset: 42,
    }
}

fn scope(class_tag: &str, paired_class_tag: &str, frame_length: u64) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::Extrude,
        10,
    );
    scope
        .try_edit(|draft| {
            draft.class_tag =
                crate::records::references::DesignClassTag::try_from(class_tag.to_owned()).unwrap();
            draft.paired_class_tag =
                crate::records::references::DesignClassTag::try_from(paired_class_tag.to_owned())
                    .unwrap();
            draft.frame_length = frame_length;
            draft.paired_byte_offset = draft.byte_offset + frame_length;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn wide_to_face_frames_omit_a_zero_offset() {
    let to_face = prologue(DesignExtrudeExtent::OneSidedToFace);
    for (class_tag, paired, length) in [("335", "266", 513), ("404", "262", 535)] {
        let admitted = scope(class_tag, paired, length);
        assert!(extrude_omits_zero_side_one_offset(&admitted, &to_face, 0));
        assert!(!extrude_omits_zero_side_one_offset(&admitted, &to_face, 1));
        assert!(!extrude_omits_zero_side_one_offset(
            &admitted,
            &prologue(DesignExtrudeExtent::OneSidedDistance),
            0
        ));
    }
    for (class_tag, paired, length) in [
        ("336", "266", 513),
        ("335", "258", 513),
        ("335", "266", 512),
        ("335", "266", 514),
        ("404", "266", 535),
        ("404", "262", 513),
    ] {
        assert!(!extrude_omits_zero_side_one_offset(
            &scope(class_tag, paired, length),
            &to_face,
            0
        ));
    }
}
