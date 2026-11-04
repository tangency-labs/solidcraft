// SPDX-License-Identifier: Apache-2.0

use crate::records::{
    dimensions::{
        DesignDimensionAnnotationOperand as Operand, DesignDimensionLocusPair as Pair,
        DesignDimensionLocusPairDraft as Draft,
    },
    identity::Located,
};
use std::num::NonZeroU32;

fn draft(first: Option<NonZeroU32>, base: u64, length: u64) -> Draft {
    let prefix = if first.is_some() { 40 } else { 25 };
    Draft {
        id: "pair".into(),
        companion_record_index: 1,
        governing_companion_record_index: 2,
        byte_offset: base,
        class_tag: "274".to_owned().try_into().unwrap(),
        record_index: 3,
        frame_length: length,
        opaque_index: first.map(|_| Located {
            value: 4,
            offset: base + 35,
        }),
        loci: [
            Operand {
                geometry_record_index: first,
                geometry_reference_offset: base + prefix,
                role: 0,
                role_offset: base + prefix + 10,
            },
            Operand {
                geometry_record_index: NonZeroU32::new(41),
                geometry_reference_offset: base + prefix + 15,
                role: 1,
                role_offset: base + prefix + 25,
            },
        ],
        paired_class_tag: "273".to_owned().try_into().unwrap(),
        paired_byte_offset: base + length,
    }
}

#[test]
fn locus_pair_derives_each_form_and_preserves_its_smallest_extent() {
    for (first, length) in [(None, 55), (NonZeroU32::new(41), 70)] {
        let payload = draft(first, 100, length);
        let pair = Pair::try_new(payload.clone()).unwrap();
        assert_eq!(pair.clone().into_draft(), payload);
        let wire = serde_json::to_value(&pair).unwrap();
        assert_eq!(serde_json::from_value::<Pair>(wire).unwrap(), pair);
        let mut short = payload;
        short.frame_length -= 1;
        short.paired_byte_offset -= 1;
        assert!(Pair::try_new(short).unwrap_err().contains("frame_length"));
        let boundary = Pair::try_new(draft(first, u64::MAX - length, length)).unwrap();
        assert_eq!(boundary.paired_byte_offset(), u64::MAX);
        let mut overflow = boundary.into_draft();
        overflow.frame_length += 1;
        assert!(Pair::try_new(overflow)
            .unwrap_err()
            .contains("paired_byte_offset"));
    }
}

#[test]
fn locus_pair_rejects_stale_wire_offsets_and_missing_geometry() {
    for first in [None, NonZeroU32::new(40)] {
        let pair = Pair::try_new(draft(first, 100, 80)).unwrap();
        let wire = serde_json::to_value(&pair).unwrap();
        for field in [
            "opaque_index_offset",
            "first_geometry_reference_offset",
            "first_role_offset",
            "second_geometry_reference_offset",
            "second_role_offset",
            "paired_byte_offset",
        ] {
            if first.is_none() && field == "opaque_index_offset" {
                continue;
            }
            let mut invalid = wire.clone();
            invalid[field] = 0.into();
            assert!(serde_json::from_value::<Pair>(invalid)
                .unwrap_err()
                .to_string()
                .contains(field));
        }
        let mut invalid = wire;
        invalid["second_geometry_record_index"] = 0.into();
        assert!(serde_json::from_value::<Pair>(invalid)
            .unwrap_err()
            .to_string()
            .contains("second_geometry_record_index"));
        let mut missing_second = pair.into_draft();
        missing_second.loci[1].geometry_record_index = None;
        assert!(Pair::try_new(missing_second).is_err());
    }
    let mut missing_opaque = draft(NonZeroU32::new(40), 100, 80);
    missing_opaque.opaque_index = None;
    assert!(Pair::try_new(missing_opaque)
        .unwrap_err()
        .contains("opaque_index"));
    let mut stray_opaque = draft(None, 100, 80);
    stray_opaque.opaque_index = Some(Located {
        value: 4,
        offset: 135,
    });
    assert!(Pair::try_new(stray_opaque)
        .unwrap_err()
        .contains("opaque_index"));
}
