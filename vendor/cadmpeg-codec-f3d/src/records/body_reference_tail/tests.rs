// SPDX-License-Identifier: Apache-2.0
use super::{BodyTailReference, DesignBodyReferenceTail, DesignBodyReferenceTailWire};
use crate::records::identity::Located;

fn wire(trailer: u32) -> DesignBodyReferenceTailWire {
    let mut reference = vec![1];
    reference.extend_from_slice(&u64::MAX.to_le_bytes());
    reference.extend_from_slice(&[0, 0]);
    DesignBodyReferenceTailWire {
        id: "f3d:Design/BulkStream.dat:body-reference-tail#10".into(),
        record_index: 17,
        byte_offset: 10,
        serialization_form: Located {
            value: 0,
            offset: 30,
        },
        first_count_offset: 34,
        first_references: Vec::new(),
        second_count_offset: 38,
        second_references: vec![BodyTailReference::new(42, reference).expect("local reference")],
        extra_zero: None,
        trailing_form: Located {
            value: trailer,
            offset: 53,
        },
    }
}

#[test]
fn empty_first_run_does_not_select_the_trailing_form() {
    for trailer in [8, 9] {
        let value = DesignBodyReferenceTail::try_from(wire(trailer)).expect("admitted tail");
        assert!(value.first_references.is_empty());
        assert_eq!(value.second_references[0].target(), Some(u64::MAX));
        assert!(!value.second_references[0].is_null());
        let encoded = serde_json::to_string(&value).expect("native JSON");
        let decoded: DesignBodyReferenceTail = serde_json::from_str(&encoded).expect("native read");
        assert_eq!(decoded, value);
    }
}

#[test]
fn reference_encoding_and_locations_are_admitted_before_use() {
    assert!(BodyTailReference::new(42, vec![1]).is_err());
    assert!(BodyTailReference::new(42, vec![0, 0]).is_err());
    let null = BodyTailReference::new(42, vec![0]).expect("null reference");
    assert!(null.is_null());
    assert_eq!(null.target(), None);
    let mut incorrect = wire(9);
    incorrect.second_count_offset += 1;
    assert!(DesignBodyReferenceTail::try_from(incorrect).is_err());
    let mut incorrect = wire(9);
    incorrect.trailing_form.offset += 1;
    assert!(DesignBodyReferenceTail::try_from(incorrect).is_err());
    let mut incorrect = wire(9);
    incorrect.second_references[0].offset += 1;
    assert!(DesignBodyReferenceTail::try_from(incorrect).is_err());
}

#[test]
fn extra_zero_remains_a_separate_admitted_lane() {
    let mut extended = wire(9);
    extended.extra_zero = Some(Located {
        value: 0,
        offset: 53,
    });
    extended.trailing_form.offset = 57;
    let value = DesignBodyReferenceTail::try_from(extended.clone()).expect("extra zero");
    assert_eq!(value.extra_zero.expect("extra lane").offset, 53);
    extended.trailing_form.value = 8;
    assert!(DesignBodyReferenceTail::try_from(extended).is_err());
}
