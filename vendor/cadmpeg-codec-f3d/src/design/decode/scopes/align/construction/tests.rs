// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::feature::align::DesignAlignForm;
use crate::test_support::align::{align_constructor_fixture, align_prefix_fixture};

#[test]
fn align_constructor_retains_every_word_code_and_scalar() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (prefix, scope) = align_prefix_fixture(form);
        let envelope = super::super::decode_align_envelope(&prefix, &scope).unwrap();
        let bytes = align_constructor_fixture(&scope);
        let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
        let construction = decode_align_construction(&bytes, &records, &scope, &envelope).unwrap();
        assert_eq!(
            construction
                .marked_words
                .iter()
                .map(|word| word.value)
                .collect::<Vec<_>>(),
            [814, 816, 818, 820, 800, 822, 824, 800]
        );
        assert_eq!(
            construction
                .codes
                .iter()
                .map(|code| code.value)
                .collect::<Vec<_>>(),
            [7, 9, 3, 11, 17]
        );
        for (value, expected) in construction
            .matrix_scalars
            .value
            .iter()
            .zip((0_u32..16).map(|value| f64::from(value) * 0.25))
        {
            assert_eq!(value.get().to_bits(), expected.to_bits());
        }
        assert_eq!(
            construction.scalar.value.get().to_bits(),
            0.625_f64.to_bits()
        );
    }
}

#[test]
fn align_constructor_refuses_changed_framing_owners_and_nonfinite_scalars() {
    let (prefix, scope) = align_prefix_fixture(DesignAlignForm::Compact);
    let envelope = super::super::decode_align_envelope(&prefix, &scope).unwrap();
    let bytes = align_constructor_fixture(&scope);
    let decode = |bytes: &[u8]| {
        let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
        decode_align_construction(bytes, &records, &scope, &envelope)
    };
    for at in [
        layout::HEADER_ZERO,
        layout::FIRST_MARKER,
        layout::SECOND_MARKER,
        layout::THIRD_MARKER,
        layout::FOURTH_MARKER,
        layout::OWNER_MARKER,
        layout::FIRST_COMPANION_MARKER,
        layout::SECOND_COMPANION_MARKER,
        layout::REPEATED_OWNER_MARKER,
        layout::FIRST_PADDING,
        layout::SECOND_PADDING,
        layout::THIRD_PADDING,
        layout::FOURTH_PADDING,
        layout::MATRIX_ZERO,
        layout::MATRIX_TAIL_ZERO,
        layout::OWNER_ZERO,
        layout::OWNER_PADDING,
        layout::FIRST_COMPANION_PADDING,
        layout::SECOND_COMPANION_PADDING,
        layout::REPEATED_OWNER_PADDING,
        layout::OWNER_WORD,
        layout::REPEATED_OWNER_WORD,
    ] {
        let mut changed = bytes.clone();
        changed[at] ^= 2;
        assert!(decode(&changed).is_none(), "offset {at}");
    }
    for at in [layout::MATRIX_SCALARS, layout::SCALAR] {
        let mut changed = bytes.clone();
        changed[at..at + 8].copy_from_slice(&f64::NAN.to_le_bytes());
        assert!(decode(&changed).is_none());
    }
    let mut longer = bytes.clone();
    longer.insert(layout::LEN, 0);
    assert!(decode(&longer).is_none());
    for end in 0..bytes.len() {
        assert!(decode(&bytes[..end]).is_none());
    }
}
