// SPDX-License-Identifier: Apache-2.0
use super::{DesignPlacementConstruction, PlacementReference};

#[test]
fn native_placement_frames_retain_source_references_and_reject_inconsistent_locations() {
    for frame in crate::test_support::placement_construction::frames() {
        let fixture: serde_json::Value = serde_json::from_str(&frame).unwrap();
        let native = fixture["native"].clone();
        let record: DesignPlacementConstruction = serde_json::from_value(native.clone()).unwrap();
        assert_eq!(serde_json::to_value(&record).unwrap(), native);
        for field in ["member_offset", "paired_byte_offset"] {
            let mut invalid = native.clone();
            invalid[field] = serde_json::json!(invalid[field].as_u64().unwrap() + 1);
            assert!(serde_json::from_value::<DesignPlacementConstruction>(invalid).is_err());
        }
        let mut invalid = native.clone();
        invalid["fields"]["values"]["count"]["value"] = serde_json::json!(1000);
        assert!(serde_json::from_value::<DesignPlacementConstruction>(invalid).is_err());
        let mut invalid = native;
        invalid["fields"]["ref_type"]["offset"] = serde_json::json!(0);
        assert!(serde_json::from_value::<DesignPlacementConstruction>(invalid).is_err());
    }
}

#[test]
fn placement_references_keep_complete_inline_guid_and_cross_segment_encodings() {
    let mut raw = vec![1];
    raw.extend_from_slice(&34390_u64.to_le_bytes());
    raw.extend_from_slice(&36_u32.to_le_bytes());
    raw.extend_from_slice(b"2CA5A1CD-C99B-4C9B-91AF-57989148E841");
    raw.extend_from_slice(&[0, 0]);
    let local = PlacementReference::new(38, raw.clone()).unwrap();
    assert_eq!(local.local_target(), Some(34390));
    let wire = serde_json::to_value(&local).unwrap();
    assert_eq!(
        serde_json::from_value::<PlacementReference>(wire).unwrap(),
        local
    );
    assert!(PlacementReference::new(38, raw[..50].to_vec()).is_err());
    raw[50] = 1;
    raw.extend_from_slice(&7_u32.to_le_bytes());
    let cross_segment = PlacementReference::new(38, raw.clone()).unwrap();
    assert_eq!(cross_segment.local_target(), None);
    raw.push(0);
    assert!(PlacementReference::new(38, raw).is_err());
}
