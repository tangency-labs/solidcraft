// SPDX-License-Identifier: Apache-2.0

use super::super::{thread_class_pair_holds, thread_designation_offset_holds};
use crate::records::feature::thread::DesignThreadForm;
use crate::records::references::DesignClassTag;

#[test]
fn standard_compact_admits_any_class_pair_and_the_two_designation_offsets() {
    let tag = |text: &str| DesignClassTag::try_from(text.to_owned()).unwrap();
    for (class, paired) in [("901", "902"), ("334", "262"), ("414", "263")] {
        assert!(thread_class_pair_holds(
            DesignThreadForm::StandardCompact,
            &tag(class),
            &tag(paired)
        ));
    }
    // The legacy forms stay tied to their class pair.
    assert!(!thread_class_pair_holds(
        DesignThreadForm::StandardLegacy,
        &tag("901"),
        &tag("902")
    ));
    assert!(DesignThreadForm::StandardCompact.has_standard_reference_table());
    for offset in [38, 42] {
        assert!(thread_designation_offset_holds(
            DesignThreadForm::StandardCompact,
            1000 + offset,
            1000
        ));
    }
    for offset in [0, 37, 39, 41, 43] {
        assert!(!thread_designation_offset_holds(
            DesignThreadForm::StandardCompact,
            1000 + offset,
            1000
        ));
    }
    assert!(!thread_designation_offset_holds(
        DesignThreadForm::StandardCompact,
        10,
        1000
    ));
}

#[test]
fn extended_standard_thread_has_one_face_group_and_a_designation_at_46() {
    assert!(!DesignThreadForm::StandardExtended.has_standard_reference_table());
    assert_eq!(
        DesignThreadForm::StandardExtended.face_groups(&[101, 102, 120, 123]),
        [101]
    );
    assert_eq!(
        DesignThreadForm::StandardExtended.face_groups(&[101, 102, 105, 106, 109, 110, 120, 123]),
        [101, 105, 109]
    );
    assert!(thread_class_pair_holds(
        DesignThreadForm::StandardExtended,
        &DesignClassTag::try_from("393".to_owned()).unwrap(),
        &DesignClassTag::try_from("259".to_owned()).unwrap(),
    ));
    assert!(thread_designation_offset_holds(
        DesignThreadForm::StandardExtended,
        1046,
        1000
    ));
}

#[test]
fn extended_standard_thread_layout_rejects_short_prefixes_and_extra_references() {
    for delta in [34, 38, 42, 43] {
        assert!(!thread_designation_offset_holds(
            DesignThreadForm::StandardExtended,
            1000 + delta,
            1000
        ));
    }
    for delta in [44, 46, 50] {
        assert!(thread_designation_offset_holds(
            DesignThreadForm::StandardExtended,
            1000 + delta,
            1000
        ));
    }
    for count in [0, 2, 3, 5] {
        assert!(!DesignThreadForm::StandardExtended.reference_count_holds(count));
    }
    for count in [4, 6, 8] {
        assert!(DesignThreadForm::StandardExtended.reference_count_holds(count));
    }
    assert!(thread_designation_offset_holds(
        DesignThreadForm::CompactExtended(crate::records::identity::Located {
            value: std::num::NonZeroU32::new(7).unwrap(),
            offset: 1100,
        }),
        1046,
        1000
    ));
}

#[test]
fn extended_compact_thread_places_face_pairs_after_two_owners() {
    let form = DesignThreadForm::CompactExtended(crate::records::identity::Located {
        value: std::num::NonZeroU32::new(7).unwrap(),
        offset: 1100,
    });
    assert_eq!(form.face_reference_start(), 2);
    assert!(!form.has_standard_reference_table());
    assert!(form.reference_count_holds(4));
    assert!(form.reference_count_holds(6));
    assert!(!form.reference_count_holds(2));
    assert!(!form.reference_count_holds(5));
}
