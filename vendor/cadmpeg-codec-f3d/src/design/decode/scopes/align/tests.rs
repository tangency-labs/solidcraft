// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::test_support::align::align_prefix_fixture;

#[test]
fn align_prefix_decodes_both_forms_and_preserves_source_code() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (bytes, scope) = align_prefix_fixture(form);
        let envelope = decode_align_envelope(&bytes, &scope).unwrap();
        assert_eq!(envelope.form, form);
        assert_eq!(envelope.source_code.value, 7);
        assert_eq!(envelope.first_reference.value, 806);
        assert_eq!(envelope.second_reference.value, 802);
    }
}

#[test]
fn align_prefix_refuses_changed_constants_references_and_truncation() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (bytes, scope) = align_prefix_fixture(form);
        let shift = bytes.len() - compact::LEN;
        let mut offsets = vec![
            compact::HEADER_ZERO,
            compact::ZERO + shift,
            compact::FIRST_MARKER + shift,
            compact::FIRST_REFERENCE + shift,
            compact::FIRST_PADDING + shift,
            compact::SECOND_MARKER + shift,
            compact::SECOND_REFERENCE + shift,
            compact::SECOND_PADDING + shift,
            compact::IDENTITY_LENGTH + shift,
            compact::IDENTITY + shift,
            compact::TAIL_ZERO + shift,
        ];
        if form == DesignAlignForm::Extended {
            offsets.push(extended::EXTRA_CODE);
        }
        for offset in offsets {
            let mut changed = bytes.clone();
            changed[offset] ^= 2;
            assert!(
                decode_align_envelope(&changed, &scope).is_none(),
                "offset {offset}"
            );
        }
        for end in 0..bytes.len() {
            assert!(decode_align_envelope(&bytes[..end], &scope).is_none());
        }
    }
}

#[test]
fn align_native_paths_have_distinct_exact_losses() {
    let (bytes, scope) = align_prefix_fixture(DesignAlignForm::Compact);
    let mut envelope = decode_align_envelope(&bytes, &scope).unwrap();
    assert_eq!(
        align_native_loss(&scope, None).code.to_string(),
        "f3d/feature.align-envelope-unparsed"
    );
    assert_eq!(
        align_native_loss(&scope, Some(&envelope)).code.to_string(),
        "f3d/feature.align-construction-unparsed"
    );
    let bytes = crate::test_support::align::align_constructor_fixture(&scope);
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    envelope.construction =
        construction::decode_align_construction(&bytes, &records, &scope, &envelope);
    assert!(envelope.construction.is_some());
    assert_eq!(
        align_native_loss(&scope, Some(&envelope)).code.to_string(),
        "f3d/feature.align-operands-unresolved"
    );
}
