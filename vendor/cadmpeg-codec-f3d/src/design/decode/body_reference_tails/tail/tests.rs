// SPDX-License-Identifier: Apache-2.0
use super::parse;
use cadmpeg_core::decode::View;

fn local(target: u64, bytes: &mut Vec<u8>) {
    bytes.push(1);
    bytes.extend_from_slice(&target.to_le_bytes());
    bytes.extend_from_slice(&[0, 0]);
}

#[test]
fn empty_first_run_preserves_either_trailing_form() {
    for trailing in [8u32, 9] {
        let mut bytes = Vec::new();
        for value in [0u32, 0, 1] {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        local(7132, &mut bytes);
        bytes.extend_from_slice(&trailing.to_le_bytes());
        let tail = parse(View::over_retained(&bytes), false).expect("standard tail");
        assert_eq!(tail.first.count.value, 0);
        assert_eq!(tail.second.count.value, 1);
        assert_eq!(tail.trailing.value, trailing);
        assert_eq!(tail.trailing.offset, 23);
        assert_eq!(tail.end, 27);
    }
}

#[test]
fn counted_runs_consume_every_variable_width_reference() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&15u32.to_le_bytes());
    for target in 1..=15 {
        local(target, &mut bytes);
    }
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.push(0);
    bytes.push(1);
    bytes.extend_from_slice(&16u64.to_le_bytes());
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"7ACC2A03-0261-4879-A14A-A93D661A5BDC");
    bytes.extend_from_slice(&[0, 0]);
    bytes.extend_from_slice(&9u32.to_le_bytes());
    let tail = parse(View::over_retained(&bytes), false).expect("counted tail");
    assert_eq!(tail.form.value, 1);
    assert_eq!(tail.first.count.value, 15);
    assert_eq!(tail.first.end, 173);
    assert_eq!(tail.second.count.value, 2);
    assert_eq!(tail.second.end, 229);
    assert_eq!(tail.trailing.value, 9);
}

#[test]
fn extra_zero_requires_the_independently_selected_form() {
    let bytes: Vec<_> = [0u32, 0, 0, 0, 9]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(parse(View::over_retained(&bytes), false).is_none());
    let tail = parse(View::over_retained(&bytes), true).expect("explicit extra zero");
    assert_eq!(tail.extra_zero.expect("zero lane").value, 0);
    assert_eq!(tail.trailing.value, 9);
    for end in 0..bytes.len() {
        assert!(parse(View::over_retained(&bytes[..end]), true).is_none());
    }
    let corrupt: Vec<_> = [0u32, u32::MAX, 0, 8]
        .into_iter()
        .flat_map(u32::to_le_bytes)
        .collect();
    assert!(parse(View::over_retained(&corrupt), false).is_none());
}
