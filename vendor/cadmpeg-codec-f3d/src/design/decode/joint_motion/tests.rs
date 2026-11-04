// SPDX-License-Identifier: Apache-2.0
use std::collections::HashMap;

use super::{decode_stream, FREEDOM_ENTRY_PREFIX, ZERO_LANE_FREEDOM_ENTRY_PREFIX};

fn header(out: &mut Vec<u8>, class_tag: [u8; 3], record_index: u32) {
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&class_tag);
    out.extend_from_slice(&record_index.to_le_bytes());
}

fn reference(out: &mut Vec<u8>, target: u32) {
    out.push(1);
    out.extend_from_slice(&target.to_le_bytes());
    out.extend_from_slice(&[0; 6]);
}

fn freedom(out: &mut Vec<u8>, code: u32) {
    out.extend_from_slice(&FREEDOM_ENTRY_PREFIX);
    out.extend_from_slice(&3u32.to_le_bytes());
    out.extend_from_slice(&code.to_le_bytes());
}

/// Joint record `index`: twenty bytes, its self-reference, the two limit
/// flags, ten bytes, then the maximum and the minimum.
fn joint(out: &mut Vec<u8>, index: u32, limits: Option<(f64, f64)>) {
    header(out, *b"331", index);
    out.extend_from_slice(&[0; 20]);
    reference(out, index);
    match limits {
        Some(_) => out.extend_from_slice(&[1, 1]),
        None => out.extend_from_slice(&[0, 0]),
    }
    out.extend_from_slice(&[0; 10]);
    let (maximum, minimum) = limits.unwrap_or((0.0, 0.0));
    out.extend_from_slice(&maximum.to_le_bytes());
    out.extend_from_slice(&minimum.to_le_bytes());
    out.extend_from_slice(&[0; 8]);
}

/// Scope record `index` referencing joint record `joint`.
fn scope(out: &mut Vec<u8>, index: u32, joint: u32) {
    header(out, *b"285", index);
    out.extend_from_slice(&[0; 10]);
    reference(out, joint);
    out.extend_from_slice(&[0; 8]);
}

/// Current-generation motion record `index` naming `joint` at offset 24.
fn motion(out: &mut Vec<u8>, class_tag: [u8; 3], index: u32, joint: u32, codes: &[u32]) {
    header(out, class_tag, index);
    out.extend_from_slice(&[0; 13]);
    reference(out, joint);
    out.extend_from_slice(&[0; 9]);
    for code in codes {
        freedom(out, *code);
        out.extend_from_slice(&[0; 4]);
    }
    out.extend_from_slice(&[0; 8]);
}

fn codes(bytes: &[u8], scopes: &HashMap<u32, bool>) -> Vec<(u32, u32, Vec<u32>)> {
    decode_stream(bytes, "Design/BulkStream.dat", scopes)
        .into_iter()
        .map(|motion| {
            (
                motion.joint_record_index,
                motion.motion_record_index,
                motion
                    .freedoms
                    .iter()
                    .map(|freedom| freedom.value)
                    .collect(),
            )
        })
        .collect()
}

/// A revolute joint and a rigid joint of the current generation: the motion
/// record names its joint at offset 24 and the `Assemble` scope follows the
/// joint record. A partner record repeating the entry names the scope, not the
/// joint, and is not a motion record. The rigid joint has no entry.
#[test]
fn current_generation_motion_names_its_joint_and_limits() {
    let mut bytes = Vec::new();
    joint(&mut bytes, 10, Some((1.5, -1.5)));
    scope(&mut bytes, 11, 10);
    joint(&mut bytes, 12, None);
    scope(&mut bytes, 13, 12);
    motion(&mut bytes, *b"368", 20, 10, &[2]);
    motion(&mut bytes, *b"395", 21, 11, &[2]);
    let scopes = HashMap::from([(11, true), (13, true)]);
    assert_eq!(codes(&bytes, &scopes), [(10, 20, vec![2])]);
    let [motion] = decode_stream(&bytes, "Design/BulkStream.dat", &scopes)
        .try_into()
        .unwrap();
    let limits = motion.limits.expect("limit lanes");
    assert_eq!((limits.first_flag, limits.second_flag), (1, 1));
    assert_eq!((limits.maximum, limits.minimum), (1.5, -1.5));

    // The record after the joint is a decoded scope of another family.
    let scopes = HashMap::from([(11, false), (13, true)]);
    assert!(codes(&bytes, &scopes).is_empty());
}

/// A word-`1` entry stores an f64 after its code; the motion keeps the word
/// and the value. Another word is no freedom entry.
#[test]
fn a_word_one_freedom_entry_keeps_its_word_and_value() {
    let mut bytes = Vec::new();
    joint(&mut bytes, 10, None);
    scope(&mut bytes, 11, 10);
    header(&mut bytes, *b"309", 20);
    bytes.extend_from_slice(&[0; 13]);
    reference(&mut bytes, 10);
    bytes.extend_from_slice(&[0; 9]);
    for word in [1_u32, 2] {
        bytes.extend_from_slice(&FREEDOM_ENTRY_PREFIX);
        bytes.extend_from_slice(&word.to_le_bytes());
        bytes.extend_from_slice(&2u32.to_le_bytes());
        bytes.extend_from_slice(&std::f64::consts::TAU.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 8]);
    let scopes = HashMap::from([(11, true)]);
    let [read] = decode_stream(&bytes, "Design/BulkStream.dat", &scopes)
        .try_into()
        .unwrap();
    assert_eq!(
        read.freedoms
            .iter()
            .map(|freedom| freedom.value)
            .collect::<Vec<_>>(),
        [2]
    );
    assert_eq!(
        read.entry_words,
        [
            crate::records::feature::joint_motion::DesignJointFreedomWord {
                word: 1,
                zero_second_value: false,
                value: Some(std::f64::consts::TAU),
            }
        ]
    );

    // Word-`3` entries keep no words.
    let mut usual = Vec::new();
    joint(&mut usual, 10, None);
    scope(&mut usual, 11, 10);
    motion(&mut usual, *b"368", 20, 10, &[2]);
    let [read] = decode_stream(&usual, "Design/BulkStream.dat", &scopes)
        .try_into()
        .unwrap();
    assert!(read.entry_words.is_empty());
}

/// A word-`1` entry can store `0` where other entries store π; such an entry
/// is read only in a record without π entries, and only with word `1`. The
/// joint record's rest flag follows its limit flags and its rest value
/// follows the minimum.
#[test]
fn a_zero_lane_entry_and_a_rest_position_are_read() {
    let zero_lane = |out: &mut Vec<u8>, word: u32, code: u32| {
        out.extend_from_slice(&ZERO_LANE_FREEDOM_ENTRY_PREFIX);
        out.extend_from_slice(&word.to_le_bytes());
        out.extend_from_slice(&code.to_le_bytes());
        out.extend_from_slice(&0f64.to_le_bytes());
    };
    let mut bytes = Vec::new();
    header(&mut bytes, *b"331", 10);
    bytes.extend_from_slice(&[0; 20]);
    reference(&mut bytes, 10);
    bytes.extend_from_slice(&[1, 1, 1]);
    bytes.extend_from_slice(&[0; 9]);
    for value in [1.6_f64, -0.5, 0.25] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    scope(&mut bytes, 11, 10);
    header(&mut bytes, *b"309", 20);
    bytes.extend_from_slice(&[0; 13]);
    reference(&mut bytes, 10);
    bytes.extend_from_slice(&[0; 9]);
    zero_lane(&mut bytes, 3, 8);
    zero_lane(&mut bytes, 1, 2);
    bytes.extend_from_slice(&[0; 8]);
    let scopes = HashMap::from([(11, true)]);
    let [read] = decode_stream(&bytes, "Design/BulkStream.dat", &scopes)
        .try_into()
        .unwrap();
    assert_eq!(
        read.freedoms
            .iter()
            .map(|freedom| freedom.value)
            .collect::<Vec<_>>(),
        [2]
    );
    assert!(read.entry_words[0].zero_second_value);
    let limits = read.limits.expect("limit lanes");
    assert_eq!((limits.maximum, limits.minimum), (1.6, -0.5));
    assert_eq!(
        limits.rest,
        Some(crate::records::feature::joint_motion::DesignJointRest {
            flag: 1,
            value: 0.25
        })
    );

    // Beside a π entry, a zero-lane match is not an entry.
    let mut mixed = Vec::new();
    joint(&mut mixed, 10, None);
    scope(&mut mixed, 11, 10);
    header(&mut mixed, *b"309", 20);
    mixed.extend_from_slice(&[0; 13]);
    reference(&mut mixed, 10);
    mixed.extend_from_slice(&[0; 9]);
    freedom(&mut mixed, 8);
    mixed.extend_from_slice(&[0; 4]);
    zero_lane(&mut mixed, 1, 2);
    mixed.extend_from_slice(&[0; 8]);
    assert_eq!(codes(&mixed, &scopes), [(10, 20, vec![8])]);
}

/// Older generation: the motion record follows its joint record and
/// references it anywhere in its body.
#[test]
fn older_generation_motion_follows_its_joint_record() {
    let mut bytes = Vec::new();
    joint(&mut bytes, 30, None);
    header(&mut bytes, *b"357", 31);
    bytes.extend_from_slice(&[0; 30]);
    reference(&mut bytes, 30);
    freedom(&mut bytes, 1);
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(codes(&bytes, &HashMap::new()), [(30, 31, vec![1])]);
}

/// A freedom entry in a record that names no joint is not a joint motion.
#[test]
fn a_freedom_entry_outside_a_motion_record_is_ignored() {
    let mut bytes = Vec::new();
    header(&mut bytes, *b"300", 40);
    bytes.extend_from_slice(&[0; 20]);
    freedom(&mut bytes, 8);
    bytes.extend_from_slice(&[0; 8]);
    assert!(codes(&bytes, &HashMap::new()).is_empty());
}

#[test]
fn malformed_rest_lanes_keep_limits() {
    use crate::records::feature::{
        assembly::{
            DesignAssemblyAlignment, DesignAssemblyAlignmentForm, DesignAssemblyAxialOperandTarget,
            DesignAssemblyOperandFrame, DesignAssemblyOperandQualifier,
        },
        scope::{DesignFeatureKind, DesignParameterScope, DesignScopePayload},
    };
    let mut bytes = Vec::new();
    joint(&mut bytes, 10, Some((1.5, -1.5)));
    let end = bytes.len();
    let valid = super::joint_limits(&bytes, 0, end, 10).unwrap();
    assert!(!valid.rest_malformed);
    assert!(serde_json::to_value(&valid)
        .unwrap()
        .get("rest_malformed")
        .is_none());
    let flag_at = 31 + super::REST_FLAG_AFTER_SELF_REFERENCE;
    let value_at = 31 + super::REST_VALUE_AFTER_SELF_REFERENCE;
    let cases = [
        (2, 0.25, end),
        (1, f64::NAN, end),
        (1, f64::INFINITY, end),
        (1, f64::NEG_INFINITY, end),
        (1, 0.25, value_at + 7),
    ];
    for (flag, value, joint_end) in cases {
        bytes[flag_at] = flag;
        bytes[value_at..value_at + 8].copy_from_slice(&value.to_le_bytes());
        let lanes = super::joint_limits(&bytes, 0, joint_end, 10).unwrap();
        assert_eq!((lanes.first_flag, lanes.second_flag), (1, 1));
        assert_eq!((lanes.minimum, lanes.maximum), (-1.5, 1.5));
        assert_eq!(lanes.flags_offset, valid.flags_offset);
        assert_eq!(lanes.values_offset, valid.values_offset);
        assert!(lanes.rest.is_none());
        assert!(lanes.rest_malformed);
        let stored = serde_json::to_value(&lanes).unwrap();
        assert_eq!(stored["rest_malformed"], true);
        assert_eq!(
            serde_json::from_value::<crate::records::feature::joint_motion::DesignJointLimitLanes>(
                stored
            )
            .unwrap(),
            lanes
        );
        let mut scopes = Vec::new();
        for index in [1, 2] {
            let mut scope = DesignParameterScope::empty(
                &format!("f3d:Design/BulkStream.dat:design-parameter-scope#{index}"),
                DesignFeatureKind::JointOrigin,
                index,
            );
            scope.with_joint_origin_transform(
                crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
            );
            scopes.push(scope);
        }
        let mut scope = DesignParameterScope::empty(
            "f3d:Design/BulkStream.dat:design-parameter-scope#11",
            DesignFeatureKind::Assemble,
            11,
        );
        scope.try_edit(|draft| {
            draft.payload = DesignScopePayload::Assemble(Some(
                DesignAssemblyAlignment::try_new(0.0, [0.0; 3], Vec::new(), Some(
                    DesignAssemblyAlignmentForm::qualified(
                        [1, 2].map(|index| DesignAssemblyOperandFrame {
                            reference_record_index: index, reference_offset: 0,
                            transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
                            transform_offset: 0,
                        }),
                        [1, 2].map(|scope_record_index| DesignAssemblyOperandQualifier::AxialTarget {
                            target: DesignAssemblyAxialOperandTarget::DocumentRootJointOrigin { scope_record_index },
                        }),
                    ),
                )).unwrap(),
            ));
        }).unwrap();
        scopes.push(scope);
        for (code, limits_key, scale) in [(2, "angular_limits", 1.0), (8, "linear_limits", 10.0)] {
            let motion = crate::records::feature::joint_motion::DesignJointMotion {
                id: "f3d:Design/BulkStream.dat:design-joint-motion#12".into(),
                joint_record_index: 10,
                joint_byte_offset: 0,
                motion_record_index: 12,
                motion_byte_offset: cadmpeg_core::decode::u64_from_index(end),
                freedoms: vec![crate::records::identity::Located {
                    value: code,
                    offset: 0,
                }],
                entry_words: Vec::new(),
                limits: Some(lanes.clone()),
            };
            let projection = crate::design::assembly::project_assembly_joints(
                &scopes,
                &[],
                &crate::design::components::ComponentInserts::default(),
                &[motion],
            );
            assert_eq!(projection.joints.len(), 1);
            let joint = serde_json::to_value(&projection.joints[0]).unwrap();
            let limits = &joint["operands"]["kind"][limits_key];
            assert_eq!(limits["minimum"], -1.5 * scale);
            assert_eq!(limits["maximum"], 1.5 * scale);
            assert_eq!(projection.losses.len(), 1);
            assert_eq!(
                projection.losses[0].code,
                crate::loss::F3dLossCode::AssemblyJointRestUnprojected.kind()
            );
            assert!(projection.losses[0]
                .message
                .contains("stored rest that is malformed"));
        }
    }
    assert!(super::joint_limits(&bytes, 0, flag_at, 10).is_none());
}

#[test]
fn zero_lane_match_in_a_partner_record_is_not_a_freedom() {
    let mut bytes = Vec::new();
    joint(&mut bytes, 10, None);
    scope(&mut bytes, 11, 10);
    header(&mut bytes, *b"395", 21);
    bytes.extend_from_slice(&[0; 13]);
    reference(&mut bytes, 11);
    bytes.extend_from_slice(&[0; 9]);
    bytes.extend_from_slice(&ZERO_LANE_FREEDOM_ENTRY_PREFIX);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0f64.to_le_bytes());
    assert!(codes(&bytes, &HashMap::from([(11, true)])).is_empty());
}
