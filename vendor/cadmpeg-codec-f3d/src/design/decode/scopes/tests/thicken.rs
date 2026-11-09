// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::thicken_shell::exact_thicken_operation;
use crate::records::feature::direct_face::DesignThickenOperation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::test_support::{indexed_header, push_marked_reference};
use cadmpeg_core::decode::u64_from_index;
use cadmpeg_ir::report::loss::LossNote;

const MARKER_ZERO: &[u8] = &[0];
const MARKER_ONE: &[u8] = &[1, 0, 0, 0, 0];
const MARKER_ONE_PADDED: &[u8] = &[1, 0, 0, 0, 0, 0];
const MARKER_ONE_FLAGGED: &[u8] = &[1, 0, 0, 0, 0, 1];

struct ThickenFrame<'a> {
    prologue: &'a [u8],
    form: u32,
    groups: &'a [u32],
    flags: [u8; 2],
    scalar_zero: bool,
    table: &'a [u32],
    thickness: f64,
}

impl ThickenFrame<'_> {
    fn group_first() -> Self {
        Self {
            prologue: MARKER_ONE,
            form: 4,
            groups: &[200],
            flags: [0, 1],
            scalar_zero: false,
            table: &[200, 201, 74],
            thickness: -1.0,
        }
    }

    fn encode(&self) -> (Vec<u8>, DesignParameterScope) {
        let mut bytes = Vec::new();
        indexed_header(&mut bytes, *b"347", 1);
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(self.prologue);
        bytes.extend_from_slice(&self.form.to_le_bytes());
        bytes.extend_from_slice(&u32::try_from(self.groups.len()).unwrap().to_le_bytes());
        for group in self.groups {
            push_marked_reference(&mut bytes, *group);
        }
        bytes.extend_from_slice(&self.flags);
        push_marked_reference(&mut bytes, 74);
        if self.scalar_zero {
            bytes.extend_from_slice(&0u32.to_le_bytes());
        }
        bytes.extend_from_slice(&1u32.to_le_bytes());
        push_marked_reference(&mut bytes, 300);
        bytes.extend_from_slice(&[0; 8]);
        bytes.extend_from_slice(&36u32.to_le_bytes());
        for unit in "00000000-0000-0000-0000-000000000000".encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes.extend_from_slice(&[0; 3]);
        let reference_count_offset = bytes.len();
        bytes.extend_from_slice(&u32::try_from(self.table.len()).unwrap().to_le_bytes());
        for reference in self.table {
            push_marked_reference(&mut bytes, *reference);
        }
        bytes.resize(bytes.len() + 120, 0);
        let frame_length = bytes.len();
        indexed_header(&mut bytes, *b"258", 1);
        let scalar_start = bytes.len();
        indexed_header(&mut bytes, *b"277", 74);
        bytes.resize(scalar_start + 104, 0);
        bytes[scalar_start + 40..scalar_start + 48].copy_from_slice(&self.thickness.to_le_bytes());
        indexed_header(&mut bytes, *b"261", 74);

        let mut scope =
            DesignParameterScope::empty("f3d:test:thicken#0", DesignFeatureKind::Thicken, 1);
        scope
            .try_edit(|draft| {
                draft.frame_length = u64_from_index(frame_length);
                draft.reference_count_offset = u64_from_index(reference_count_offset);
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(self.table.to_vec());
                draft.paired_byte_offset = draft.byte_offset + draft.frame_length;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        (bytes, scope)
    }

    fn decode(&self) -> (Option<DesignThickenOperation>, Vec<LossNote>) {
        let (bytes, scope) = self.encode();
        decode(&bytes, &scope)
    }
}

fn decode(
    bytes: &[u8],
    scope: &DesignParameterScope,
) -> (Option<DesignThickenOperation>, Vec<LossNote>) {
    let mut losses = Vec::new();
    let operation = exact_thicken_operation(
        bytes,
        &crate::design::test_support::indexed_record_offsets_for_test(bytes),
        scope,
        &mut losses,
    );
    (operation, losses)
}

fn thickness(frame: &ThickenFrame<'_>) -> Option<(f64, u32)> {
    frame.decode().0.map(|operation| {
        (
            operation.signed_thickness.get(),
            operation.thickness_record_index,
        )
    })
}

#[test]
fn every_prologue_form_reads_the_signed_thickness() {
    for prologue in [
        MARKER_ZERO,
        MARKER_ONE,
        MARKER_ONE_PADDED,
        MARKER_ONE_FLAGGED,
    ] {
        for scalar_zero in [false, true] {
            let frame = ThickenFrame {
                prologue,
                scalar_zero,
                ..ThickenFrame::group_first()
            };
            assert_eq!(thickness(&frame), Some((-1.0, 74)));
        }
    }
}

#[test]
fn scalar_first_table_reads_the_signed_thickness() {
    let frame = ThickenFrame {
        prologue: MARKER_ONE_PADDED,
        scalar_zero: true,
        table: &[74, 200, 201, 202],
        thickness: 0.5,
        ..ThickenFrame::group_first()
    };
    assert_eq!(thickness(&frame), Some((0.5, 74)));
}

#[test]
fn two_groups_name_ordered_references_in_table_order() {
    let frame = ThickenFrame {
        groups: &[200, 203],
        flags: [0, 0],
        table: &[200, 201, 202, 203, 204, 74],
        ..ThickenFrame::group_first()
    };
    assert_eq!(thickness(&frame), Some((-1.0, 74)));
    let reversed = ThickenFrame {
        groups: &[200, 200],
        ..frame
    };
    assert_eq!(thickness(&reversed), None);
}

#[test]
fn admitted_scope_reports_its_flag_bytes() {
    let (operation, losses) = ThickenFrame {
        flags: [1, 0],
        ..ThickenFrame::group_first()
    }
    .decode();
    assert!(operation.is_some());
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code,
        crate::loss::F3dLossCode::FeatureThickenFlagsAssumed.kind()
    );
    assert!(losses[0].message.contains("flag bytes 01 00 at byte 44"));
}

#[test]
fn six_byte_prologue_reports_its_flag_byte() {
    let (operation, losses) = ThickenFrame {
        prologue: MARKER_ONE_FLAGGED,
        flags: [1, 1],
        ..ThickenFrame::group_first()
    }
    .decode();
    assert!(operation.is_some());
    assert!(losses[0]
        .message
        .contains("flag bytes 01 01 at byte 45 and prologue flag byte 01 at byte 25"));
}

#[test]
fn frame_outside_the_grammar_stays_unread() {
    let rejected = [
        ThickenFrame {
            prologue: &[2],
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            prologue: &[1, 0, 0, 0, 0, 2],
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            form: 1,
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            groups: &[],
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            groups: &[201],
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            flags: [0, 2],
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            table: &[200, 74, 201],
            ..ThickenFrame::group_first()
        },
        ThickenFrame {
            thickness: 0.0,
            ..ThickenFrame::group_first()
        },
    ];
    for frame in &rejected {
        let (operation, losses) = frame.decode();
        assert_eq!(operation, None);
        assert!(losses.is_empty());
    }
}

#[test]
fn reference_table_must_follow_the_guid() {
    let (bytes, mut scope) = ThickenFrame::group_first().encode();
    scope
        .try_edit(|draft| {
            draft.reference_count_offset += 1;
            draft.layout_fixture_references();
        })
        .unwrap();
    assert_eq!(decode(&bytes, &scope).0, None);
}
