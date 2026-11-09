// SPDX-License-Identifier: Apache-2.0
use crate::records::feature::scope::DesignParameterScope;
use crate::test_support::indexed_header;
use crate::test_support::push_marked_reference;

/// Byte-level choices for a synthetic `PathPattern` scope frame.
#[derive(Clone, Copy)]
struct PathPatternFrame {
    compact_prefix: bool,
    distance_type: u8,
    flip: u8,
    marker: u8,
    distance_marker: [u8; 4],
    settings_tail: usize,
    start_parameter: f64,
    foreign_start_frame: bool,
}

impl Default for PathPatternFrame {
    fn default() -> Self {
        Self {
            compact_prefix: false,
            distance_type: 1,
            flip: 1,
            marker: 1,
            distance_marker: [0xff; 4],
            settings_tail: 0,
            start_parameter: 0.5,
            foreign_start_frame: false,
        }
    }
}

/// A `PathPattern` scope frame, its settings record 322 naming path group
/// 333, and path-start records 319 and 320.
fn path_pattern_frame(frame: PathPatternFrame) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"300", 315);
    if frame.compact_prefix {
        bytes.extend_from_slice(&[0; 10]);
    } else {
        bytes.extend_from_slice(&[0; 9]);
        bytes.extend_from_slice(&1u32.to_le_bytes());
        bytes.push(0);
    }
    push_marked_reference(&mut bytes, 322);
    bytes.resize(60, 0);
    let settings_at = bytes.len();
    indexed_header(&mut bytes, *b"301", 322);
    bytes.resize(settings_at + 267 + frame.settings_tail, 0);
    bytes[settings_at + 79] = frame.marker;
    bytes[settings_at + 80] = frame.flip;
    bytes[settings_at + 81] = 0;
    let mut reference = Vec::new();
    push_marked_reference(&mut reference, 333);
    bytes[settings_at + 83..settings_at + 94].copy_from_slice(&reference);
    bytes[settings_at + 152..settings_at + 156].copy_from_slice(&frame.distance_marker);
    bytes[settings_at + 156] = frame.distance_type;
    if frame.foreign_start_frame {
        let foreign_at = bytes.len();
        indexed_header(&mut bytes, *b"305", 319);
        bytes.resize(foreign_at + 20, 0);
    }
    for (record_index, class_tag) in [(319, *b"302"), (320, *b"304")] {
        let start_at = bytes.len();
        indexed_header(&mut bytes, class_tag, record_index);
        bytes.resize(start_at + 197, 0);
        bytes[start_at + 25..start_at + 33].copy_from_slice(&frame.start_parameter.to_le_bytes());
    }
    indexed_header(&mut bytes, *b"303", 330);
    bytes
}

#[test]
fn path_pattern_settings_read_the_settings_and_path_start_records() {
    use crate::records::entity_header::SegmentType;

    let segment_type = |guid: &str, version: u32| {
        SegmentType::try_new(
            crate::ids::native_design_type_id("FusionAssetName[Active]/Design1/MetaStream.dat", 0),
            crate::design::test_support::design_type(guid, None, version, "Fusion", vec![0]),
        )
        .unwrap()
    };
    let settings = segment_type("BE1FB171-D98A-4A4D-99C2-3738627CF79C", 4);
    let old_settings = segment_type("BE1FB171-D98A-4A4D-99C2-3738627CF79C", 3);
    let start = segment_type("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44", 3);
    let newer_start = segment_type("69EE2FA7-BCC7-449E-9CA9-976CEFDFED44", 4);
    let other = segment_type("5A1BF548-241F-46FD-9FB5-E4B05126EB9D", 1);
    let scope = |members: Vec<u32>| {
        let mut scope = DesignParameterScope::empty(
            "f3d:FusionAssetName[Active]/Design1/BulkStream.dat:design-parameter-scope#0",
            crate::records::feature::scope::DesignFeatureKind::PathPattern,
            315,
        );
        scope
            .try_edit(|draft| {
                draft.reference_count_offset = 40;
                draft.reference_members =
                    crate::records::identity::ReferenceRun::unlocated(members);
                draft.paired_byte_offset = 200;
                draft.frame_length = 200;
                draft.layout_fixture_references();
                draft.layout_fixture_tail();
            })
            .unwrap();
        scope
    };
    let read = |frame: PathPatternFrame,
                settings_type: &SegmentType,
                second_start: &SegmentType,
                members: Vec<u32>| {
        let bytes = path_pattern_frame(frame);
        let class_types = std::collections::HashMap::from([
            (301, settings_type),
            (302, &start),
            (304, second_start),
        ]);
        super::exact_path_pattern_settings(
            &bytes,
            &crate::design::test_support::indexed_record_offsets_for_test(&bytes),
            &scope(members),
            &class_types,
        )
    };
    let valid = |frame: PathPatternFrame| read(frame, &settings, &other, vec![319, 320, 333]);
    for compact_prefix in [false, true] {
        let settings_read = valid(PathPatternFrame {
            compact_prefix,
            ..PathPatternFrame::default()
        })
        .expect("path-pattern settings");
        assert_eq!(settings_read.settings_record_index, 322);
        assert!(settings_read.total_extent.value);
        assert!(settings_read.flipped.value);
        assert!(!settings_read.path_oriented.value);
        assert_eq!(settings_read.path_group_record_index, 333);
        assert_eq!(settings_read.start_record_index, 319);
        assert!((settings_read.start_parameter.value.get() - 0.5).abs() <= f64::EPSILON);
    }
    assert!(valid(PathPatternFrame {
        distance_type: 0,
        ..PathPatternFrame::default()
    })
    .is_some_and(|read| !read.total_extent.value));
    for frame in [
        PathPatternFrame {
            distance_type: 2,
            ..PathPatternFrame::default()
        },
        PathPatternFrame {
            flip: 2,
            ..PathPatternFrame::default()
        },
        PathPatternFrame {
            marker: 0,
            ..PathPatternFrame::default()
        },
        PathPatternFrame {
            distance_marker: [0xff, 0xff, 0xff, 0],
            ..PathPatternFrame::default()
        },
        PathPatternFrame {
            settings_tail: 1,
            ..PathPatternFrame::default()
        },
        PathPatternFrame {
            start_parameter: f64::NAN,
            ..PathPatternFrame::default()
        },
        PathPatternFrame {
            foreign_start_frame: true,
            ..PathPatternFrame::default()
        },
    ] {
        assert!(valid(frame).is_none());
    }
    assert!(read(
        PathPatternFrame::default(),
        &old_settings,
        &other,
        vec![319, 320, 333]
    )
    .is_none());
    assert!(read(
        PathPatternFrame::default(),
        &settings,
        &start,
        vec![319, 320, 333]
    )
    .is_none());
    assert!(read(
        PathPatternFrame::default(),
        &settings,
        &newer_start,
        vec![319, 320, 333]
    )
    .is_none());
    assert!(read(
        PathPatternFrame::default(),
        &settings,
        &other,
        vec![320, 333]
    )
    .is_none());
    assert!(read(
        PathPatternFrame::default(),
        &settings,
        &other,
        vec![319, 320]
    )
    .is_none());
}
