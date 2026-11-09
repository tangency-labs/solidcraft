// SPDX-License-Identifier: Apache-2.0
use crate::layout::current_extrude_non_target_extent_pair as pair;
use crate::records::feature::extrude::{
    DesignExtrudeDistanceTail, DesignExtrudeExtent, DesignExtrudeMeasurement, DesignExtrudePrologue,
};

const FIRST: usize = 77;
const SECOND: usize = FIRST + pair::SECOND_SIDE_EXTENT;
const TAPER_SLOT: usize = SECOND + 10;
const REFERENCE_COUNT: usize = 200;
const TAPER: u32 = 255;

/// A current reference-aware distance prologue with seven null slots, a
/// distance first side, and the side-two taper slot naming `TAPER`.
fn frame(direction: u32, second: u32, target_option: u32, measurement: u32) -> Vec<u8> {
    let mut bytes = vec![0; 240];
    bytes[28..32].copy_from_slice(&4u32.to_le_bytes());
    bytes[32..36].copy_from_slice(&direction.to_le_bytes());
    bytes[36..40].copy_from_slice(&1u32.to_le_bytes());
    bytes[46..54].copy_from_slice(&1f64.to_le_bytes());
    bytes[FIRST..FIRST + 4].copy_from_slice(&1u32.to_le_bytes());
    bytes[FIRST + pair::TARGET_OPTION..FIRST + pair::TARGET_OPTION + 4]
        .copy_from_slice(&target_option.to_le_bytes());
    bytes[SECOND..SECOND + 4].copy_from_slice(&second.to_le_bytes());
    bytes[TAPER_SLOT] = 1;
    bytes[TAPER_SLOT + 1..TAPER_SLOT + 9].copy_from_slice(&u64::from(TAPER).to_le_bytes());
    bytes[TAPER_SLOT + 12..TAPER_SLOT + 16].copy_from_slice(&measurement.to_le_bytes());
    bytes
}

fn parse(bytes: &[u8]) -> Option<DesignExtrudePrologue> {
    super::super::exact_current_extrude_prologue(bytes, 0, REFERENCE_COUNT, &[TAPER], false)
}

fn tail(
    target_option: u32,
    measurement: DesignExtrudeMeasurement,
    measurement_offset: usize,
) -> DesignExtrudeDistanceTail {
    DesignExtrudeDistanceTail {
        target_option,
        target_option_offset: u64::try_from(FIRST + pair::TARGET_OPTION).unwrap(),
        measurement,
        measurement_offset: u64::try_from(measurement_offset).unwrap(),
    }
}

#[test]
fn a_two_sided_or_symmetric_distance_prologue_reads_its_target_option_and_measurement() {
    for (direction, second, measurement, extent, expected) in [
        (
            3,
            0,
            2,
            DesignExtrudeExtent::SymmetricDistance,
            DesignExtrudeMeasurement::EachSide,
        ),
        (
            3,
            0,
            1,
            DesignExtrudeExtent::SymmetricDistance,
            DesignExtrudeMeasurement::WholeTravel,
        ),
        (
            2,
            1,
            0,
            DesignExtrudeExtent::TwoSidedDistance,
            DesignExtrudeMeasurement::Sided,
        ),
    ] {
        for target_option in [0, 1] {
            let parsed = parse(&frame(direction, second, target_option, measurement)).unwrap();
            assert_eq!(parsed.extent(), Some(extent));
            assert_eq!(
                parsed.distance_tail(),
                Some(tail(target_option, expected, TAPER_SLOT + 12))
            );
        }
    }

    let mut null_taper = frame(3, 0, 1, 2);
    null_taper[TAPER_SLOT..TAPER_SLOT + 16].fill(0);
    null_taper[TAPER_SLOT + 2..TAPER_SLOT + 6].copy_from_slice(&2u32.to_le_bytes());
    assert_eq!(
        parse(&null_taper).unwrap().distance_tail(),
        Some(tail(1, DesignExtrudeMeasurement::EachSide, TAPER_SLOT + 2))
    );
}

#[test]
fn a_one_sided_distance_prologue_keeps_target_option_zero_and_no_tail() {
    let parsed = parse(&frame(1, 0, 0, 0)).unwrap();
    assert_eq!(parsed.extent(), Some(DesignExtrudeExtent::OneSidedDistance));
    assert_eq!(parsed.distance_tail(), None);
    assert!(parse(&frame(1, 0, 1, 0)).is_none());
}

#[test]
fn target_option_one_needs_a_tail_that_agrees_with_the_direction() {
    assert!(parse(&frame(3, 0, 2, 2)).is_none());
    assert!(parse(&frame(3, 0, 1, 0)).is_none());
    assert!(parse(&frame(3, 0, 1, 3)).is_none());
    assert!(parse(&frame(2, 1, 1, 2)).is_none());

    let mut gap = frame(3, 0, 1, 2);
    gap[SECOND + 6] = 1;
    assert!(parse(&gap).is_none());
    let mut zero_run = frame(3, 0, 1, 2);
    zero_run[FIRST + 6] = 1;
    assert!(parse(&zero_run).is_none());
    let mut pad = frame(3, 0, 1, 2);
    pad[TAPER_SLOT + 11] = 1;
    assert!(parse(&pad).is_none());
    let foreign_taper = frame(3, 0, 1, 2);
    assert!(super::super::exact_current_extrude_prologue(
        &foreign_taper,
        0,
        REFERENCE_COUNT,
        &[TAPER + 1],
        false
    )
    .is_none());
    let short = frame(3, 0, 1, 2);
    assert!(super::super::exact_current_extrude_prologue(
        &short,
        0,
        TAPER_SLOT + 15,
        &[TAPER],
        false
    )
    .is_none());

    let untailed = parse(&frame(3, 0, 0, 0)).unwrap();
    assert_eq!(
        untailed.extent(),
        Some(DesignExtrudeExtent::SymmetricDistance)
    );
    assert_eq!(untailed.distance_tail(), None);
}
