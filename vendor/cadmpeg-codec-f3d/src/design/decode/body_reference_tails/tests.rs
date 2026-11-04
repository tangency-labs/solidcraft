// SPDX-License-Identifier: Apache-2.0
use super::*;
use crate::test_support::body_reference_tail_test::{archive, frame, metadata};

#[test]
fn inline_nested_prefix_and_nonzero_source_origin_keep_exact_locations() {
    for inline in [false, true] {
        for trailer in [8, 9] {
            let (bytes, frame) = frame(inline, trailer, 7);
            crate::test_support::with_decode_context(|ctx| {
                let root = View::over_retained(&bytes);
                let header = prefix(root.child(frame.start, frame.member_end).unwrap(), frame, 0)
                    .expect("registered prefix");
                let fields = tail::parse(header, false).expect("reference tail");
                assert_eq!(fields.form.offset, if inline { 243 } else { 203 });
                assert_eq!(fields.first.count.value, 0);
                assert_eq!(fields.second.count.value, 1);
                assert_eq!(fields.trailing.value, trailer);
                let references = references(ctx, root, fields.second).expect("retained reference");
                assert_eq!(references[0].target(), Some(u64::MAX));
                assert!(
                    prefix(root.child(frame.start, frame.member_end).unwrap(), frame, 1).is_none()
                );
            });
        }
    }
}

#[test]
fn archive_registration_selects_the_family_and_unrecognized_forms_abstain() {
    let (bytes, frame) = frame(true, 9, 0);
    for (version, expected) in [(1, 1), (2, 0)] {
        let source = archive(&bytes, &metadata(frame, version));
        crate::test_support::with_decode_context(|ctx| {
            crate::test_support::zip_test::with_scan(&source, |scan| {
                let decoded = decode(ctx, scan).expect("native tails");
                assert_eq!(decoded.records.len(), expected);
                assert_eq!(decoded.unparsed, 0);
                if let Some(record) = decoded.records.first() {
                    assert_eq!(record.first_references.len(), 0);
                    assert_eq!(record.second_references.len(), 1);
                    assert_eq!(record.trailing_form.value, 9);
                }
            });
        });
    }
    let mut corrupt = bytes;
    corrupt[frame.member_end - 4..frame.member_end].copy_from_slice(&0u32.to_le_bytes());
    let source = archive(&corrupt, &metadata(frame, 1));
    crate::test_support::with_decode_context(|ctx| {
        crate::test_support::zip_test::with_scan(&source, |scan| {
            let decoded = decode(ctx, scan).expect("source-only tail");
            assert!(decoded.records.is_empty());
            assert_eq!(decoded.unparsed, 1);
        });
    });
}
