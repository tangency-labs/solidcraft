// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::align::{align_envelope_links_hold, decode_align_envelope};
use crate::records::feature::align::DesignAlignForm;
use crate::test_support::align::align_prefix_fixture;

#[test]
fn align_validator_checks_both_forms_references_and_locations() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (bytes, scope) = align_prefix_fixture(form);
        let envelope = decode_align_envelope(&bytes, &scope).unwrap();
        assert!(align_envelope_links_hold(&scope, &envelope));
        for field in 0..6 {
            let mut changed = envelope.clone();
            match field {
                0 => changed.first_reference.value += 1,
                1 => changed.first_reference.offset += 1,
                2 => changed.second_reference.value += 1,
                3 => changed.second_reference.offset += 1,
                4 => changed.source_code.offset += 1,
                _ => {
                    changed.form = match form {
                        DesignAlignForm::Compact => DesignAlignForm::Extended,
                        DesignAlignForm::Extended => DesignAlignForm::Compact,
                    }
                }
            }
            assert!(!align_envelope_links_hold(&scope, &changed));
        }
    }
}

#[test]
fn align_validator_checks_complete_constructor_length_owners_and_field_locations() {
    for form in [DesignAlignForm::Compact, DesignAlignForm::Extended] {
        let (prefix, scope) = align_prefix_fixture(form);
        let mut envelope = decode_align_envelope(&prefix, &scope).unwrap();
        envelope.construction =
            Some(crate::test_support::align::align_construction_value_fixture(&scope));
        assert!(envelope.construction.is_some());
        assert!(align_envelope_links_hold(&scope, &envelope));
        for field in 0..20 {
            let mut changed = envelope.clone();
            let construction = changed.construction.as_mut().unwrap();
            match field {
                0 => construction.record_index += 1,
                1 => construction.byte_offset += 1,
                2 => construction.paired_byte_offset += 1,
                3 => construction.matrix_scalars.offset += 1,
                4 => construction.scalar.offset += 1,
                5 => construction.marked_words[4].value += 1,
                6 => construction.marked_words[7].value += 1,
                7..=14 => construction.marked_words[field - 7].offset += 1,
                _ => construction.codes[field - 15].offset += 1,
            }
            assert!(
                !align_envelope_links_hold(&scope, &changed),
                "field {field}"
            );
        }
    }
}
