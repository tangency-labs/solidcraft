// SPDX-License-Identifier: Apache-2.0

use crate::records::entity_header::{DesignFeatureTimeline, DesignTimelineFrame};
use crate::records::identity::Located;
use crate::test_support::native_test::reject_changed_id;
use std::num::NonZeroU64;

#[test]
fn timeline_id_binds_a_design_stream_and_frame_offset() {
    let make = |id: &str| {
        DesignFeatureTimeline::try_new(
            id.into(),
            DesignTimelineFrame::new(
                200,
                100,
                220,
                240,
                vec![Located {
                    value: 101,
                    offset: 245,
                }],
            )
            .unwrap(),
            "256".to_owned().try_into().unwrap(),
            NonZeroU64::new(35).unwrap(),
            0,
            NonZeroU64::new(17).unwrap(),
        )
    };
    let invalid = [
        "timeline",
        "stream:design-feature-timeline#200",
        "f3d:Design/BulkStream.dat:design-feature-timeline#201",
        "f3d:Design/BulkStream.dat:act-guid#200",
    ];
    for id in invalid {
        assert!(make(id).is_err());
    }
    reject_changed_id(
        make("f3d:Design/BulkStream.dat:design-feature-timeline#200").unwrap(),
        &invalid,
    );
}

#[test]
fn entity_identity_retains_suffix_without_changing_source_spelling() {
    for (text, suffix) in [
        ("_0", 0),
        ("part_0007", 7),
        ("part_+7", 7),
        ("part_a_18446744073709551615", u64::MAX),
        ("文😀_01", 1),
    ] {
        let identity = crate::records::identity::DesignEntityId::try_from(text.to_owned()).unwrap();
        assert_eq!(identity.as_str(), text);
        assert_eq!(identity.suffix(), suffix);
        assert_eq!(identity.into_string(), text);
    }
    for text in ["part", "part_", "part_-1", "part_18446744073709551616"] {
        assert!(crate::records::identity::DesignEntityId::try_from(text.to_owned()).is_err());
    }
    let identity = crate::records::identity::DesignEntityId::from_parts("part_", u64::MAX);
    assert_eq!(identity.as_str(), "part__18446744073709551615");
    assert_eq!(identity.suffix(), u64::MAX);
}
