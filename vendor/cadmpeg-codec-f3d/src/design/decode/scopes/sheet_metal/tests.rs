// SPDX-License-Identifier: Apache-2.0

use crate::test_support::write_marked_reference;

fn source_kinds<'a>(kinds: &'a [(u32, &'a str)]) -> impl Fn(u32, &str) -> bool + 'a {
    move |record_index, expected| {
        let mut matches = kinds.iter().filter(|(owner, _)| *owner == record_index);
        matches.next().is_some_and(|(_, kind)| *kind == expected) && matches.next().is_none()
    }
}

#[test]
fn hem_direction_fixtures_keep_flip_separate_from_compatibility_lanes() {
    let controls: [(Vec<u8>, &[u32], bool); 2] = [
        (
            crate::test_support::verification::bytes(
                "hem/flat.bin",
                include_bytes!("../../../../test_support/hem/flat.bin"),
            ),
            &[703, 706, 708, 710, 713, 717, 720, 724],
            false,
        ),
        (
            crate::test_support::verification::bytes(
                "hem/flipped.bin",
                include_bytes!("../../../../test_support/hem/flipped.bin"),
            ),
            &[703, 706, 708, 717, 720, 724, 754, 757],
            true,
        ),
    ];
    for (bytes, references, expected_flip) in controls {
        let operation = super::exact_hem_operation(
            &bytes,
            0,
            bytes.len(),
            &references.iter().copied(),
            &source_kinds(&[(703, "HemGap"), (706, "HemLength")]),
        )
        .expect("authored Hem direction control");
        assert_eq!(operation.flipped, expected_flip);
        assert_eq!(
            operation.bend_position,
            cadmpeg_ir::features::SheetMetalHemBendPosition::Adjacent
        );
        assert_eq!(operation.settings_record_index, 724);
        assert_eq!(operation.bend_radius.get(), 0.25);
    }
}

/// Field values written into a synthetic gap-and-length `Hem` frame.
struct HemFixture {
    header_shift: usize,
    wrapper: u32,
    settings: u32,
    gap_owner: u32,
    length_owner: u32,
    aggregate_group: u32,
    edge_group: u32,
    bend_radius: f64,
}

/// A synthetic frame plus the offsets the reader is expected to derive.
struct HemFrame {
    bytes: Vec<u8>,
    paired_at: usize,
    bend_radius_offset: u64,
}

#[test]
fn hem_scope_binds_parameters_edge_groups_and_rule_radius() {
    // Groups before owners; roles come from marked slots under both header shifts.
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    for header_shift in [0usize, 4] {
        let frame = hem_frame(&HemFixture {
            header_shift,
            wrapper: 308,
            settings: 311,
            gap_owner: 301,
            length_owner: 304,
            aggregate_group: 240,
            edge_group: 251,
            bend_radius: 0.25,
        });

        let operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references.iter().copied(),
            &source_kinds(&[(301, "HemGap"), (304, "HemLength")]),
        )
        .expect("fixed Hem operation");
        assert_eq!(operation.edge_wrapper_record_index, 308);
        assert_eq!(operation.settings_record_index, 311);
        assert_eq!(
            operation.parameter_owners,
            crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLength {
                gap_owner_record_index: 301,
                length_owner_record_index: 304,
            }
        );
        assert_eq!(operation.aggregate_group_record_index.get(), 240);
        assert_eq!(operation.aggregate_operand_record_index(), 243);
        assert_eq!(operation.edge_group_record_index.get(), 251);
        assert_eq!(operation.edge_operand_record_index(), 254);
        assert_eq!(operation.bend_radius.get(), 0.25);
        assert_eq!(operation.bend_radius_offset, frame.bend_radius_offset);
    }
}

#[test]
fn hem_scope_refuses_a_frame_whose_owner_slot_is_absent() {
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    let mut frame = hem_frame(&HemFixture {
        header_shift: 0,
        wrapper: 308,
        settings: 311,
        gap_owner: 301,
        length_owner: 304,
        aggregate_group: 240,
        edge_group: 251,
        bend_radius: 0.25,
    });
    // Move the length-owner reference one byte later, as the rolled form does.
    let at = 85 + 53;
    frame.bytes[at..at + 11].fill(0);
    frame.bytes[at + 1] = 1;
    frame.bytes[at + 2..at + 6].copy_from_slice(&304u32.to_le_bytes());
    assert!(
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references.iter().copied(),
            &source_kinds(&[(301, "HemGap"), (304, "HemLength")]),
        )
        .is_none()
    );
    assert!(
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references.iter().copied(),
            &source_kinds(&[(301, "HemGap"), (301, "HemGap"), (304, "HemLength")]),
        )
        .is_none()
    );
}

#[test]
fn hem_scope_reads_the_rolled_owner_layout() {
    let references = [708, 717, 720, 724, 775, 788, 790, 793];
    let frame = rolled_hem_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references.iter().copied(),
        &source_kinds(&[(775, "HemRadius"), (788, "HemAngle")]),
    )
    .expect("rolled Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::feature::sheet_metal::DesignHemParameterOwners::RadiusAngle {
            radius_owner_record_index: 775,
            angle_owner_record_index: 788,
        }
    );
    assert_eq!(operation.bend_radius.get(), 0.25);
    assert_eq!(operation.bend_radius_offset, 160);
}

#[test]
fn hem_scope_reads_the_teardrop_owner_layout() {
    let references = [703, 706, 708, 717, 720, 724, 775, 777, 780];
    let frame = teardrop_hem_frame();
    let operation = crate::design::decode::scopes::sheet_metal::exact_hem_operation(
        &frame.bytes,
        0,
        frame.paired_at,
        &references.iter().copied(),
        &source_kinds(&[(703, "HemGap"), (706, "HemLength"), (775, "HemRadius")]),
    )
    .expect("teardrop Hem operation");
    assert_eq!(
        operation.parameter_owners,
        crate::records::feature::sheet_metal::DesignHemParameterOwners::GapLengthRadius {
            gap_owner_record_index: 703,
            length_owner_record_index: 706,
            radius_owner_record_index: 775,
        }
    );
    assert_eq!(operation.bend_radius.get(), 0.25);
    assert_eq!(operation.bend_radius_offset, 170);
}

#[test]
fn hem_scope_refuses_an_owner_layout_whose_parameter_kinds_name_another_form() {
    let references = [240, 243, 251, 254, 301, 304, 308, 311];
    let frame = hem_frame(&HemFixture {
        header_shift: 0,
        wrapper: 308,
        settings: 311,
        gap_owner: 301,
        length_owner: 304,
        aggregate_group: 240,
        edge_group: 251,
        bend_radius: 0.25,
    });

    assert!(
        crate::design::decode::scopes::sheet_metal::exact_hem_operation(
            &frame.bytes,
            0,
            frame.paired_at,
            &references.iter().copied(),
            &source_kinds(&[(301, "HemRadius"), (304, "HemAngle")]),
        )
        .is_none()
    );
}

/// Build a gap-and-length `Hem` frame from the settled fixed-section layout.
///
/// Every offset is computed from the layout rather than counted by hand.
fn hem_frame(fixture: &HemFixture) -> HemFrame {
    let common = 85 + fixture.header_shift;
    let paired_at = 494 + fixture.header_shift;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 8, fixture.wrapper);
    write_marked_reference(&mut bytes, common + 19, fixture.settings);
    bytes[common + 30..common + 34].copy_from_slice(&1u32.to_le_bytes());
    bytes[common + 36..common + 40].copy_from_slice(&4u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 42, fixture.gap_owner);
    write_marked_reference(&mut bytes, common + 53, fixture.length_owner);
    let radius_at = common + 71;
    bytes[radius_at..radius_at + 8].copy_from_slice(&fixture.bend_radius.to_le_bytes());
    write_marked_reference(&mut bytes, common + 108, fixture.aggregate_group);
    write_marked_reference(&mut bytes, common + 135, fixture.edge_group);

    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: u64::try_from(radius_at).expect("radius offset fits u64"),
    }
}

/// Build the rolled `Hem` frame. Its header shift is four bytes and its owner
/// slots are thirteen bytes apart.
fn rolled_hem_frame() -> HemFrame {
    let common = 89;
    let paired_at = 498;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 8, 708);
    write_marked_reference(&mut bytes, common + 19, 724);
    bytes[common + 30..common + 34].copy_from_slice(&1u32.to_le_bytes());
    bytes[common + 36..common + 40].copy_from_slice(&4u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 41, 788);
    write_marked_reference(&mut bytes, common + 54, 775);
    bytes[common + 71..common + 79].copy_from_slice(&0.25f64.to_le_bytes());
    write_marked_reference(&mut bytes, common + 108, 717);
    write_marked_reference(&mut bytes, common + 135, 790);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 160,
    }
}

/// Build the teardrop `Hem` frame. The third parameter owner shifts the group
/// slots by ten bytes and moves the fixed rule radius to offset eighty-one.
fn teardrop_hem_frame() -> HemFrame {
    let common = 89;
    let paired_at = 519;
    let mut bytes = vec![0; paired_at];
    bytes[common..common + 4].copy_from_slice(&3u32.to_le_bytes());
    bytes[common + 4..common + 8].copy_from_slice(&1u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 8, 708);
    write_marked_reference(&mut bytes, common + 19, 724);
    bytes[common + 30..common + 34].copy_from_slice(&1u32.to_le_bytes());
    bytes[common + 36..common + 40].copy_from_slice(&4u32.to_le_bytes());
    write_marked_reference(&mut bytes, common + 42, 703);
    write_marked_reference(&mut bytes, common + 53, 706);
    write_marked_reference(&mut bytes, common + 64, 775);
    bytes[common + 81..common + 89].copy_from_slice(&0.25f64.to_le_bytes());
    write_marked_reference(&mut bytes, common + 118, 717);
    write_marked_reference(&mut bytes, common + 145, 777);
    HemFrame {
        bytes,
        paired_at,
        bend_radius_offset: 170,
    }
}

#[test]
fn hem_common_controls_preserve_flip_and_refuse_unknown_lanes() {
    for mut frame in [rolled_hem_frame(), teardrop_hem_frame()] {
        let common = 89;
        assert_eq!(
            super::hem_common_controls(&frame.bytes, common),
            Some(false)
        );
        frame.bytes[common + 40] = 1;
        assert_eq!(super::hem_common_controls(&frame.bytes, common), Some(true));
        for (offset, value) in [(0, 4), (30, 2), (34, 1), (35, 1), (36, 3), (40, 2)] {
            let mut invalid = frame.bytes.clone();
            invalid[common + offset] = value;
            assert_eq!(super::hem_common_controls(&invalid, common), None);
        }
        assert_eq!(
            super::hem_common_controls(&frame.bytes[..common + 40], common),
            None
        );
    }
}
