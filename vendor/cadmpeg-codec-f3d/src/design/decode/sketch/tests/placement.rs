// SPDX-License-Identifier: Apache-2.0
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::sketch::bind_sketch_graph;
use crate::design::decode::sketch::parse_sketch_placement_candidates;
use crate::design::sketch_project::{project_sketch_design, SketchProjectInputs};
use crate::records::entity_header::DesignEntityHeader;
use crate::records::entity_header::DESIGN_MODULE_SKETCH;
use crate::records::sketch_geometry::SketchCurveGeometry;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::test_support::lp_ascii;
use crate::test_support::push_marked_reference;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;

fn tested_bind_sketch_graph(
    entities: &[DesignEntityHeader],
    points: &mut [SketchPoint],
    curves: &mut [crate::records::sketch_geometry::SketchCurveIdentity],
    surfaces: &mut [crate::records::sketch_geometry::SketchSurface],
    relations: &mut [crate::records::sketch_relations::SketchRelation],
) -> Result<(), cadmpeg_core::CodecError> {
    bind_sketch_graph(entities, points, curves, surfaces, relations)
}

fn tested_parse_sketch_member_run(
    bytes: &[u8],
    from: usize,
    entity_suffix: u64,
) -> Vec<crate::records::identity::Located<u32>> {
    crate::design::decode::sketch::parse_sketch_member_run(bytes, from, entity_suffix)
}

fn tested_parse_legacy_sketch_member_run(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
) -> Option<Vec<crate::records::identity::Located<u32>>> {
    crate::design::decode::sketch::parse_legacy_sketch_member_run(bytes, primary_at, entity_suffix)
}

fn tested_parse_legacy_sketch_container_members(
    bytes: &[u8],
    primary_at: usize,
    entity_suffix: u32,
    records: &crate::design::decode::sketch::IndexedRecordOffsets,
) -> Option<Vec<crate::records::identity::Located<u32>>> {
    crate::design::decode::sketch::parse_legacy_sketch_container_members(
        bytes,
        primary_at,
        entity_suffix,
        records,
    )
}

fn candidates(
    bytes: &[u8],
    scope_record_index: u32,
    entity_id: &str,
    record_index: u32,
) -> Vec<DesignSketchPlacement> {
    let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
    parse_sketch_placement_candidates(
        bytes,
        scope_record_index,
        &crate::records::identity::DesignEntityId::try_from(entity_id.to_owned())
            .expect("valid entity ID"),
        record_index,
        &records,
    )
}

#[test]
fn sketch_placement_decodes_compact_identity_and_explicit_affine_frame() {
    fn placement_frame(
        record_index: u32,
        length: usize,
        transform_offset: usize,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"356");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = transform_offset + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"259");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact = candidates(&placement_frame(185, 201, 55, None), 177, "0_172", 185);
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].frame_length(), 201);
    assert_eq!(
        *compact[0].transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
    );
    assert_eq!(compact[0].transform_offset(), None);

    let transform = [
        [0.0, 0.0, 1.0, 12.0],
        [1.0, 0.0, 0.0, 34.0],
        [0.0, 1.0, 0.0, 56.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let explicit = candidates(
        &placement_frame(1773, 329, 55, Some(transform)),
        1765,
        "0_1761",
        1773,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length(), 329);
    assert_eq!(*explicit[0].transform(), transform);
    assert_eq!(explicit[0].transform_offset(), Some(55));

    for length in [305, 325] {
        let legacy = candidates(
            &placement_frame(1773, length, 48, Some(transform)),
            1765,
            "0_1761",
            1773,
        );
        assert_eq!(legacy.len(), 1);
        assert_eq!(legacy[0].frame_length(), u64_from_index(length));
        assert_eq!(*legacy[0].transform(), transform);
        assert_eq!(legacy[0].transform_offset(), Some(48));
    }
}

#[test]
fn entity_genesis_placement_decodes_compact_and_explicit_frames() {
    fn genesis_frame(
        record_index: u32,
        length: usize,
        form_byte: u8,
        transform: Option<[[f64; 4]; 4]>,
    ) -> Vec<u8> {
        let mut bytes = vec![0; length];
        bytes[0..4].copy_from_slice(&3u32.to_le_bytes());
        bytes[4..7].copy_from_slice(b"293");
        bytes[7..11].copy_from_slice(&record_index.to_le_bytes());
        bytes[55] = 1;
        bytes[65] = form_byte;
        if let Some(transform) = transform {
            for (ordinal, value) in transform.into_iter().flatten().enumerate() {
                let at = 66 + ordinal * 8;
                bytes[at..at + 8].copy_from_slice(&value.to_le_bytes());
            }
        }
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(b"261");
        bytes.extend_from_slice(&record_index.to_le_bytes());
        bytes
    }

    let compact = candidates(&genesis_frame(214, 213, 1, None), 206, "0_201", 214);
    assert_eq!(compact.len(), 1);
    assert_eq!(compact[0].frame_length(), 213);
    assert_eq!(
        *compact[0].transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
    );
    assert_eq!(compact[0].transform_offset(), None);

    let transform = [
        [0.0, 0.0, 1.0, 26.0],
        [1.0, 0.0, 0.0, 0.0],
        [0.0, 1.0, 0.0, 0.0],
        [0.0, 0.0, 0.0, 1.0],
    ];
    let explicit = candidates(
        &genesis_frame(3060, 341, 0, Some(transform)),
        3052,
        "0_3048",
        3060,
    );
    assert_eq!(explicit.len(), 1);
    assert_eq!(explicit[0].frame_length(), 341);
    assert_eq!(*explicit[0].transform(), transform);
    assert_eq!(explicit[0].transform_offset(), Some(66));

    // A mismatched form byte fails both lengths.
    assert!(candidates(&genesis_frame(214, 213, 0, None), 206, "0_201", 214).is_empty());
    assert!(candidates(
        &genesis_frame(3060, 341, 1, Some(transform)),
        3052,
        "0_3048",
        3060,
    )
    .is_empty());

    // The WorkPlane sibling of this record class carries a marked record
    // reference inside the zero run and must not decode as a placement.
    let mut workplane_like = genesis_frame(214, 213, 1, None);
    workplane_like[57] = 1;
    workplane_like[58..62].copy_from_slice(&788u32.to_le_bytes());
    assert!(candidates(&workplane_like, 206, "0_201", 214).is_empty());
}

#[test]
fn entity_genesis_placement_origin_scales_to_neutral_units() {
    let placement = |form: fn(
        crate::records::sketch_placement::SketchPlacementMatrix,
    ) -> crate::records::sketch_placement::DesignSketchFrameForm| {
        DesignSketchPlacement {
            frame: crate::records::sketch_placement::DesignSketchFrame::new(
                0,
                form(
                    crate::records::sketch_placement::SketchPlacementMatrix::try_from([
                        [0.0, 0.0, 1.0, 26.0],
                        [1.0, 0.0, 0.0, 0.0],
                        [0.0, 1.0, 0.0, 0.0],
                        [0.0, 0.0, 0.0, 1.0],
                    ])
                    .unwrap(),
                ),
            )
            .unwrap(),

            id: "f3d:native:design-sketch-placement#0".into(),
            scope_record_index: Some(10),
            entity_id: crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
                .expect("valid entity ID"),

            visibility: None,

            class_tag: crate::records::references::DesignClassTag::try_from("293".to_owned())
                .unwrap(),
            record_index: 11,

            paired_class_tag: crate::records::references::DesignClassTag::try_from(
                "261".to_owned(),
            )
            .unwrap(),
        }
    };
    let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: "f3d:native:sketch-point#0".into(),
        record_index: 20,
        owner_reference: Some(100),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        byte_offset: 0,
        coordinate_offset: 141,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            20,
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            Some(2),
            0.0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(120.0, 30.0),
    })
    .unwrap();
    let mut identityless_point = point.clone();
    identityless_point.id = "f3d:native:sketch-point#1".into();
    identityless_point.record_index = 21;
    identityless_point.coordinate_offset = 33;
    identityless_point
        .try_set_record_form(
            crate::records::sketch_geometry::SketchPointRecordForm::Version0 { flag: false },
        )
        .unwrap();

    // The `EntityGenesis`-flavor frame stores its origin in centimetres
    // while the sketch records carry ten-times-centimetre values; the
    // projected sketch origin scales by ten to stay commensurate.
    let (sketches, entities) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(
            decode_ctx,
            &SketchProjectInputs::new(
                &[placement(
                    crate::records::sketch_placement::DesignSketchFrameForm::ScopeGenesisExplicit,
                )],
                &[point.clone(), identityless_point],
                &[],
                &[],
                &[],
            ),
            1.0e-6,
        )
    })
    .expect("sketch lanes pair")
    .into_tuple();
    assert_eq!(sketches.len(), 1);
    assert_eq!(
        sketches[0]
            .resolved_placement()
            .map(|(origin, normal, u_axis)| (origin.get(), normal.get(), u_axis.get())),
        Some((
            Point3::new(260.0, 0.0, 0.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 1.0, 0.0),
        ))
    );
    assert!(matches!(*entities[0].geometry.definition(),
        cadmpeg_ir::sketches::SketchGeometryDefinition::Point { position }
            if position == Point2::new(120.0, 30.0)
    ));
    assert_eq!(
        entities[1].id().clone(),
        crate::ids::neutral_sketch_record_id(&sketches[0].id, 21)
    );

    // The explicit scope frame stores its origin in centimetres too.
    let (sketches, _) = crate::test_support::with_decode_context(|decode_ctx| {
        project_sketch_design(
            decode_ctx,
            &SketchProjectInputs::new(
                &[placement(
                    crate::records::sketch_placement::DesignSketchFrameForm::ScopeExplicit,
                )],
                &[point],
                &[],
                &[],
                &[],
            ),
            1.0e-6,
        )
    })
    .expect("sketch lanes pair")
    .into_tuple();
    assert_eq!(
        sketches[0]
            .resolved_placement()
            .map(|(origin, _, _)| origin.get()),
        Some(Point3::new(260.0, 0.0, 0.0))
    );
}

#[test]
fn feature_owned_sketch_placement_follows_member_run_head_reference() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"281");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);

    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    bytes.resize(80, 0);

    let head_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    for value in crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
        .rows()
        .into_iter()
        .flatten()
    {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0, 1]);
    bytes.resize(
        head_at + crate::design::decode::sketch::MEMBER_RUN_HEAD_FRAME,
        0,
    );

    let entity = DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:design-entity-header#0".into(),
        byte_offset: 0,

        entity_id: crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    };
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let placement = crate::design::decode::sketch::parse_member_run_head_placement(
        &bytes,
        entity.byte_offset,
        &entity.entity_id,
        &records,
    )
    .expect("feature-owned sketch placement");
    assert_eq!(placement.record_index, 200);
    assert_eq!(placement.byte_offset(), u64_from_index(head_at));
    assert_eq!(placement.paired_byte_offset(), u64_from_index(paired_at));
    assert_eq!(
        *placement.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
    );
    assert!(placement.member_run_head());
    assert_eq!(placement.scope_record_index, None);
    assert_eq!(
        tested_parse_legacy_sketch_container_members(&bytes, 0, 100, &records,),
        Some(Vec::new())
    );

    bytes.truncate(head_at);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 0, 1]);
    bytes.extend_from_slice(&173u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"284");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let compact = crate::design::decode::sketch::parse_member_run_head_placement(
        &bytes,
        entity.byte_offset,
        &entity.entity_id,
        &records,
    )
    .expect("compact identity sketch placement");
    assert_eq!(compact.frame_length(), 34);
    assert_eq!(
        *compact.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
    );
    assert_eq!(compact.transform_offset(), None);
}

#[test]
fn legacy_sketch_pair_decodes_its_complete_member_run() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"381");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [300u32, 301] {
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }

    let members =
        tested_parse_legacy_sketch_member_run(&bytes, 0, 100).expect("legacy sketch member run");
    assert_eq!(
        members.iter().map(|row| row.value).collect::<Vec<_>>(),
        [300, 301]
    );
    assert_eq!(
        members.iter().map(|row| row.offset).collect::<Vec<_>>(),
        [
            u64_from_index(paired_at + 46),
            u64_from_index(paired_at + 57)
        ]
    );
}

#[test]
fn legacy_line_orthogonalizes_its_auxiliary_normal() {
    let mut bytes = vec![0u8; 133];
    let values: [f64; 12] = [
        0.5,
        0.875,
        0.0,
        0.0,
        -1.75,
        0.0,
        0.0,
        -1.0,
        0.0,
        -0.000_037,
        0.000_184,
        0.999_999_982,
    ];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let SketchCurveGeometry::Line {
        direction, normal, ..
    } = crate::design::decode::sketch::decode_line(&bytes, 0)
        .expect("line parse")
        .expect("legacy line")
    else {
        panic!("expected line");
    };
    assert!((direction.as_raw().norm() - 1.0).abs() <= 1.0e-12);
    assert!((normal.as_raw().norm() - 1.0).abs() <= 1.0e-12);
    assert!(direction.as_raw().dot(*normal.as_raw()).abs() <= 1.0e-12);
    assert!(normal.as_raw().z > 0.0);

    bytes[133 + 7 * 8..133 + 8 * 8].copy_from_slice(&1.0f64.to_le_bytes());
    let SketchCurveGeometry::Line { direction, .. } =
        crate::design::decode::sketch::decode_line(&bytes, 0)
            .expect("line parse")
            .expect("reverse-parameterized line")
    else {
        panic!("expected line");
    };
    assert!((direction.as_raw().y + 1.0).abs() <= 1.0e-12);

    bytes[133 + 6 * 8..133 + 7 * 8].copy_from_slice(&0.6f64.to_le_bytes());
    bytes[133 + 7 * 8..133 + 8 * 8].copy_from_slice(&0.8f64.to_le_bytes());
    let SketchCurveGeometry::Line { direction, .. } =
        crate::design::decode::sketch::decode_line(&bytes, 0)
            .expect("line parse")
            .expect("line with stale auxiliary direction")
    else {
        panic!("expected line");
    };
    assert!((direction.as_raw().x).abs() <= 1.0e-12);
    assert!((direction.as_raw().y + 1.0).abs() <= 1.0e-12);
}

#[test]
fn spatial_line_with_parallel_auxiliary_normal_retains_its_endpoints() {
    let values: [f64; 12] = [0.0, 3.0, 0.0, 0.0, 0.0, 1.5, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let SketchCurveGeometry::Line {
        start,
        end,
        direction,
        normal,
    } = crate::design::decode::sketch::decode_line(&bytes, 0)
        .expect("line parse")
        .expect("spatial line")
    else {
        panic!("expected line");
    };
    assert_eq!(start.get(), Point3::new(0.0, 30.0, 0.0));
    assert_eq!(end.get(), Point3::new(0.0, 30.0, 15.0));
    assert_eq!(*direction.as_raw(), Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(*normal.as_raw(), Vector3::new(0.0, 1.0, 0.0));
}

#[test]
fn compact_planar_line_uses_its_implicit_normal() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&37u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);

    let SketchCurveGeometry::Line {
        start,
        end,
        direction,
        normal,
    } = crate::design::decode::sketch::decode_compact_planar_line(&bytes, 0)
        .expect("line parse")
        .expect("compact planar line")
    else {
        panic!("expected line");
    };
    assert_eq!(start.get(), Point3::new(5.0, 8.75, 0.0));
    assert_eq!(end.get(), Point3::new(5.0, -8.75, 0.0));
    assert_eq!(*direction.as_raw(), Vector3::new(0.0, -1.0, 0.0));
    assert_eq!(*normal.as_raw(), Vector3::new(0.0, 0.0, 1.0));
}

#[test]
fn retained_compact_planar_line_edit_preserves_its_reference_tail() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = Vec::new();
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(1);
    bytes.extend_from_slice(&37u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    let tail = bytes[72..].to_vec();
    let geometry = SketchCurveGeometry::line(
        Point3::new(10.0, 20.0, 0.0),
        Point3::new(30.0, 40.0, 0.0),
        Vector3::new(
            std::f64::consts::FRAC_1_SQRT_2,
            std::f64::consts::FRAC_1_SQRT_2,
            0.0,
        ),
        Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap();
    crate::writer::patch::records::patch_sketch_curves(
        &mut bytes,
        &[crate::writer::patch::edits::SketchCurveEdit {
            offset: 0,
            geometry_offset: 0,
            geometry,
        }],
    )
    .expect("compact planar line edit");
    assert_eq!(&bytes[72..], tail);
    assert_eq!(f64::from_le_bytes(bytes[0..8].try_into().unwrap()), 1.0);
    assert_eq!(f64::from_le_bytes(bytes[24..32].try_into().unwrap()), 2.0);
}

#[test]
fn text_frame_line_decodes_after_point_references() {
    let mut bytes = vec![0u8; 52 + 133];
    for reference in [2397u32, 2395] {
        bytes.push(1);
        bytes.extend_from_slice(&reference.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
        if reference == 2397 {
            bytes.push(0);
        }
    }
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"289");
    bytes.extend_from_slice(&2403u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    for value in [
        -5.75f64, 1.0, 0.0, 5.25, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }

    let (geometry, end) =
        crate::design::decode::sketch::decode_text_frame_line(&bytes, 52, 2403, 0)
            .expect("line parse")
            .expect("text-frame boundary line");
    assert_eq!(end, bytes.len());
    assert!(matches!(
        geometry,
        SketchCurveGeometry::Line { start, end, .. }
            if start.get() == Point3::new(-57.5, 10.0, 0.0)
                && end.get() == Point3::new(-5.0, 10.0, 0.0)
    ));
}

fn tested_decode_sketch_nurbs(
    payload: &[u8],
    record_at: usize,
) -> Option<Result<(SketchCurveGeometry, usize), cadmpeg_core::CodecError>> {
    crate::design::decode::sketch::decode_sketch_nurbs(payload, record_at)
}

fn tested_decode_legacy_sketch_nurbs(
    payload: &[u8],
    record_at: usize,
) -> Option<Result<(SketchCurveGeometry, usize), cadmpeg_core::CodecError>> {
    crate::design::decode::sketch::decode_legacy_sketch_nurbs(payload, record_at)
}

fn modern_sketch_nurbs_payload() -> Vec<u8> {
    let base = 133;
    let mut bytes = vec![0; base + 114];
    bytes[base..base + 8].copy_from_slice(&u64::MAX.to_le_bytes());
    bytes[base + 8..base + 12].copy_from_slice(&3u32.to_le_bytes());
    bytes[base + 12..base + 15].copy_from_slice(b"302");
    bytes[base + 15..base + 19].copy_from_slice(&7u32.to_le_bytes());
    bytes[base + 88] = 1;
    bytes[base + 90..base + 94].copy_from_slice(&1u32.to_le_bytes());
    bytes[base + 94..base + 102].copy_from_slice(&0.125f64.to_le_bytes());
    bytes[base + 102..base + 106].copy_from_slice(&4u32.to_le_bytes());
    bytes[base + 106..base + 110].copy_from_slice(&4u32.to_le_bytes());
    bytes[base + 110..base + 114].copy_from_slice(&8u32.to_le_bytes());
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        bytes.extend_from_slice(&knot.to_le_bytes());
    }
    for value in [0u32, 0, 8, 2, 2, 8] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    for coordinate in [0.0f64, 0.0, 0.0, 1.0, 0.0, 0.0] {
        bytes.extend_from_slice(&coordinate.to_le_bytes());
    }
    bytes
}

#[test]
fn modern_sketch_nurbs_reports_fit_tolerance_scale_overflow_at_source() {
    let mut bytes = modern_sketch_nurbs_payload();
    assert!(tested_decode_sketch_nurbs(&bytes, 17)
        .transpose()
        .unwrap()
        .is_some());
    bytes[133 + 94..133 + 102].copy_from_slice(&1.0e308f64.to_le_bytes());
    let error = tested_decode_sketch_nurbs(&bytes, 17)
        .transpose()
        .unwrap_err();
    assert!(error
        .to_string()
        .contains("byte 17 fit tolerance overflows millimetres"));
}

#[test]
fn modern_sketch_nurbs_reports_control_point_scale_overflow_at_source() {
    let mut bytes = modern_sketch_nurbs_payload();
    assert!(tested_decode_sketch_nurbs(&bytes, 17)
        .transpose()
        .unwrap()
        .is_some());
    let first_coordinate = 133 + 114 + 4 * 8 + 12 + 12;
    bytes[first_coordinate..first_coordinate + 8].copy_from_slice(&1.0e308f64.to_le_bytes());
    let error = tested_decode_sketch_nurbs(&bytes, 17)
        .transpose()
        .unwrap_err();
    assert!(error.to_string().contains("byte 17 overflows millimetres"));
}

fn legacy_sketch_nurbs_payload() -> Vec<u8> {
    let mut bytes = Vec::new();
    lp_ascii(&mut bytes, "256");
    bytes.extend_from_slice(&1200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for (name, value) in [("crv_primary_id", 700u64), ("crv_secondary_id", 0)] {
        lp_ascii(&mut bytes, name);
        lp_ascii(&mut bytes, "IntrinsicMetaTypeuint64");
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    assert_eq!(bytes.len(), 133);
    bytes.extend_from_slice(&[0xff; 8]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"257");
    bytes.extend_from_slice(&1200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    bytes.push(1);
    bytes.extend_from_slice(&1201u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&0.000_01f64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    push_marked_reference(&mut bytes, 1202);
    push_marked_reference(&mut bytes, 1203);
    push_marked_reference(&mut bytes, 1204);
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0x95, 0xd6, 0x26, 0xe8, 0x0b, 0x2e, 0x11, 0x3e]);
    for (values, capacity) in [
        (vec![0.0f64, 0.0, 0.0, 1.0, 1.0, 1.0], 8u32),
        (vec![1.0f64, 1.0, 1.0], 8),
        (vec![0.0f64, 0.0, 0.0, 0.5, 0.75, 0.0, 1.0, 0.0, 0.0], 8),
    ] {
        let count = u32::try_from(if values.len() == 9 {
            values.len() / 3
        } else {
            values.len()
        })
        .expect("test count");
        bytes.extend_from_slice(&count.to_le_bytes());
        bytes.extend_from_slice(&capacity.to_le_bytes());
        bytes.extend_from_slice(&8u32.to_le_bytes());
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
    }

    bytes
}

#[test]
fn legacy_sketch_nurbs_decodes_its_counted_arrays() {
    let mut bytes = legacy_sketch_nurbs_payload();
    let (geometry, end) = tested_decode_legacy_sketch_nurbs(&bytes, 0)
        .transpose()
        .expect("valid source scaling")
        .expect("legacy NURBS");
    let SketchCurveGeometry::Nurbs { geometry, .. } = geometry else {
        panic!("expected NURBS");
    };
    assert_eq!(end, bytes.len());
    assert_eq!(geometry.degree(), 2);
    assert_eq!(geometry.knots(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(geometry.poles().weights().collect::<Vec<_>>(), [1.0; 3]);
    assert_eq!(
        geometry.poles().points().nth(1).copied().unwrap(),
        Point3::new(5.0, 7.5, 0.0)
    );
    assert!((geometry.fit_tolerance().get() - 0.000_1).abs() <= f64::EPSILON);

    let mut invalid = bytes.clone();
    invalid[133 + 114..133 + 122].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(tested_decode_legacy_sketch_nurbs(&invalid, 0)
        .transpose()
        .unwrap()
        .is_none());
    for (offset, value) in [
        (133 + 42, 1.0e308_f64),
        (133 + 114 + 6 * 8 + 12 + 3 * 8 + 12, 1.0e308),
    ] {
        let mut invalid = bytes.clone();
        invalid[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
        assert!(tested_decode_legacy_sketch_nurbs(&invalid, 17)
            .transpose()
            .unwrap_err()
            .to_string()
            .contains("byte 17"));
    }

    push_marked_reference(&mut bytes, 201);
    let segment_type = |type_guid: &str, version, module: &str, entity_ids: Vec<u64>| {
        crate::records::entity_header::SegmentTypeData {
            byte_offset: 0,
            type_guid: type_guid.to_owned().try_into().expect("type GUID"),
            type_guid_offset: 0,
            base_type_guid: crate::records::entity_header::BaseTypeGuid::Absent,
            version,
            version_offset: 0,
            module: module.into(),
            entities: crate::records::identity::ReferenceRun::located(
                entity_ids
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
        }
    };
    let meta = crate::metastream::MetaStream {
        types: vec![
            segment_type(
                "D82E012F-6DDD-4AED-BDE1-C0F7F9100B9B",
                3,
                "MSketch",
                vec![1200],
            ),
            segment_type(
                "00000000-0000-0000-0000-000000000001",
                0,
                "MSketch",
                Vec::new(),
            ),
        ],
        records: vec![crate::metastream::RecordIndexEntry {
            entity_id: 1200,
            bulk_offset: 0,
        }],
        secondary_records: vec![crate::metastream::RecordIndexEntry {
            entity_id: 1200,
            bulk_offset: 141,
        }],
    };
    let curves = crate::design::decode::sketch::decode_sketch_curve_identities_from_stream(
        &bytes,
        &meta,
        "Design/BulkStream.dat",
    )
    .expect("primary NURBS frame with a nested subtype header");
    let [curve] = curves.as_slice() else {
        panic!("one indexed NURBS curve");
    };
    assert!(matches!(
        curve.geometry,
        Some(SketchCurveGeometry::Nurbs { .. })
    ));
    assert_eq!(curve.owner_reference, Some(201));
}

#[test]
fn sketch_geometry_tail_names_its_owner_container() {
    let mut bytes = vec![0u8; 112];
    bytes.push(1);
    bytes.extend_from_slice(&201u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"301");
    bytes.extend_from_slice(&400u32.to_le_bytes());
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&bytes[..123]),
        Some(201)
    );

    bytes[117] = 1;
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&bytes[..123]),
        None
    );

    let mut nested = vec![0u8; 140];
    nested[120..124].copy_from_slice(&3u32.to_le_bytes());
    nested[124..127].copy_from_slice(b"302");
    nested[127..131].copy_from_slice(&500u32.to_le_bytes());
    nested.push(1);
    nested.extend_from_slice(&201u32.to_le_bytes());
    nested.extend_from_slice(&[0; 6]);
    nested.extend_from_slice(&3u32.to_le_bytes());
    nested.extend_from_slice(b"303");
    nested.extend_from_slice(&501u32.to_le_bytes());
    assert_eq!(
        crate::design::decode::sketch::trailing_sketch_owner_reference(&nested[..151]),
        Some(201)
    );
}

fn sketch_graph_header(suffix: u64, members: Vec<u32>) -> DesignEntityHeader {
    DesignEntityHeader {
        id: format!("f3d:native:design-entity-header#{suffix}"),
        byte_offset: suffix,

        entity_id: crate::records::identity::DesignEntityId::try_from(format!("0_{suffix}"))
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            None,
            crate::records::identity::ReferenceRun::located(
                members
                    .into_iter()
                    .map(|value| crate::records::identity::Located { value, offset: 0 })
                    .collect(),
            ),
        )
        .expect("valid module registration"),
    }
}

fn sketch_graph_point(record_index: u32) -> SketchPoint {
    SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: format!("f3d:native:sketch-point#{record_index}"),
        record_index,
        owner_reference: None,
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        byte_offset: u64::from(record_index),
        coordinate_offset: 141,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            u64::from(record_index),
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            Some(2),
            0.0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
    })
    .unwrap()
}

#[test]
fn sketch_member_run_backfills_relation_free_owners() {
    let mut bytes = vec![0u8; 40];
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"282");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 41]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    let mut member_offsets = Vec::new();
    member_offsets.push(u64_from_index(bytes.len() + 1));
    bytes.push(1);
    bytes.extend_from_slice(&99u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    for member in [20u32, 21] {
        member_offsets.push(u64_from_index(bytes.len() + 1));
        bytes.push(1);
        bytes.extend_from_slice(&member.to_le_bytes());
        bytes.extend_from_slice(&[0; 6]);
    }
    bytes.extend_from_slice(&[0; 8]);
    assert_eq!(
        tested_parse_sketch_member_run(&bytes, 0, 100),
        vec![99, 20, 21]
            .into_iter()
            .zip(member_offsets)
            .map(|(value, offset)| crate::records::identity::Located { value, offset })
            .collect::<Vec<_>>()
    );
    assert_eq!(tested_parse_sketch_member_run(&bytes, 0, 101), vec![]);
    assert_eq!(
        tested_parse_sketch_member_run(&bytes, paired_at + 1, 100),
        vec![]
    );

    // Relation-free geometry named by the container's member run binds to
    // that sketch; records the run does not name stay unowned.
    let mut points = [
        sketch_graph_point(20),
        sketch_graph_point(21),
        sketch_graph_point(22),
    ];
    tested_bind_sketch_graph(
        &[sketch_graph_header(100, vec![20, 21, 99])],
        &mut points,
        &mut [],
        &mut [],
        &mut [],
    )
    .expect("member-run owners bind");
    assert_eq!(points[0].owner_reference, Some(100));
    assert_eq!(points[1].owner_reference, Some(100));
    assert_eq!(points[2].owner_reference, None);

    // Two sketches claiming one record is a structural conflict.
    let mut points = [sketch_graph_point(20)];
    assert!(tested_bind_sketch_graph(
        &[
            sketch_graph_header(100, vec![20]),
            sketch_graph_header(101, vec![20])
        ],
        &mut points,
        &mut [],
        &mut [],
        &mut [],
    )
    .is_err());
}

fn typed_sketch_reference(bytes: &mut Vec<u8>, target: u32) {
    bytes.push(1);
    bytes.extend_from_slice(&u64::from(target).to_le_bytes());
    bytes.extend_from_slice(&36u32.to_le_bytes());
    bytes.extend_from_slice(b"11111111-2222-3333-4444-555555555555");
    bytes.extend_from_slice(&[0, 0]);
}

#[test]
fn typed_legacy_sketch_pair_reads_wide_member_references_and_head_frame() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"380");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.resize(40, 0);
    let paired_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"381");
    bytes.extend_from_slice(&100u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 8]);
    typed_sketch_reference(&mut bytes, 200);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 7]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    for member in [300u32, 301] {
        typed_sketch_reference(&mut bytes, member);
    }
    let members =
        tested_parse_legacy_sketch_member_run(&bytes, 0, 100).expect("typed sketch member run");
    assert_eq!(
        members.iter().map(|row| row.value).collect::<Vec<_>>(),
        [300, 301]
    );
    assert_eq!(
        members.iter().map(|row| row.offset).collect::<Vec<_>>(),
        [
            u64_from_index(paired_at + 86),
            u64_from_index(paired_at + 137)
        ]
    );

    let head_at = bytes.len();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"283");
    bytes.extend_from_slice(&200u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 11]);
    for value in crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY
        .rows()
        .into_iter()
        .flatten()
    {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.push(0);
    typed_sketch_reference(&mut bytes, 152);
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(b"284");
    bytes.extend_from_slice(&201u32.to_le_bytes());
    let entity_id = crate::records::identity::DesignEntityId::try_from("0_100".to_owned())
        .expect("valid entity ID");
    let records = crate::design::test_support::indexed_record_offsets_for_test(&bytes);
    let placement = crate::design::decode::sketch::parse_member_run_head_placement(
        &bytes, 0, &entity_id, &records,
    )
    .expect("typed explicit head placement");
    assert_eq!(placement.byte_offset(), u64_from_index(head_at));
    assert_eq!(placement.frame_length(), 202);
    assert_eq!(
        placement.transform_offset(),
        Some(u64_from_index(head_at + 22))
    );
}

/// The compact planar line's trailing reference can use the 51-byte typed
/// spelling; a truncated typed reference is not admitted.
#[test]
fn compact_planar_line_admits_a_typed_trailing_reference() {
    let values: [f64; 9] = [0.5, 0.875, 0.0, 0.0, -1.75, 0.0, 0.0, -1.0, 0.0];
    let mut bytes = vec![0u8; 133];
    for value in values {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    typed_sketch_reference(&mut bytes, 37);
    let parsed =
        crate::design::decode::sketch::decode_compact_planar_line(&bytes, 0).expect("line parse");
    assert!(parsed.is_some());

    bytes.truncate(bytes.len() - 2);
    let truncated =
        crate::design::decode::sketch::decode_compact_planar_line(&bytes, 0).expect("line parse");
    assert!(truncated.is_none());
}

/// A compact planar line's z component is zero when it is at most one
/// floating-point epsilon of the same vector's largest planar coordinate,
/// floored at 1 cm; a larger z keeps the line rejected.
#[test]
fn compact_planar_line_reads_sub_resolution_z_noise_as_zero() {
    assert!(crate::design::decode::sketch::planar_z_is_zero(
        -23.79, 14.73, 1.0e-76
    ));
    assert!(crate::design::decode::sketch::planar_z_is_zero(
        0.0,
        0.5,
        f64::EPSILON
    ));
    assert!(!crate::design::decode::sketch::planar_z_is_zero(
        0.0,
        0.5,
        2.0 * f64::EPSILON
    ));
    assert!(!crate::design::decode::sketch::planar_z_is_zero(
        100.0, 0.0, 1.0e-12
    ));

    let line = |z: f64| {
        let values: [f64; 9] = [0.5, 0.875, z, 0.0, -1.75, 0.0, 0.0, -1.0, z];
        let mut bytes = vec![0u8; 133];
        for value in values {
            bytes.extend_from_slice(&value.to_le_bytes());
        }
        typed_sketch_reference(&mut bytes, 37);
        bytes
    };
    let noisy = line(1.0e-76);
    let parsed = crate::design::decode::sketch::decode_compact_planar_line(&noisy, 0)
        .expect("line parse")
        .expect("noisy planar line");
    let SketchCurveGeometry::Line { start, .. } = parsed else {
        panic!("expected line");
    };
    assert_eq!(start.get(), Point3::new(5.0, 8.75, 0.0));
    assert_eq!(
        crate::design::decode::sketch::compact_planar_line_z_noise(&noisy),
        Some([1.0e-76, 0.0, 1.0e-76])
    );
    assert_eq!(
        crate::design::decode::sketch::compact_planar_line_z_noise(&line(0.0)),
        None
    );

    let non_planar = line(1.0e-3);
    assert!(
        crate::design::decode::sketch::decode_compact_planar_line(&non_planar, 0)
            .expect("line parse")
            .is_none()
    );
}

fn member_run_test_entity() -> DesignEntityHeader {
    DesignEntityHeader {
        id: "f3d:Design/BulkStream.dat:design-entity-header#0".into(),
        byte_offset: 0,
        entity_id: crate::records::identity::DesignEntityId::try_from("0_173".to_owned())
            .expect("valid entity ID"),
        class_tag: crate::records::references::DesignClassTag::try_from("281".to_owned()).unwrap(),
        optional_slot_present: false,
        registration: crate::records::entity_header::DesignEntityRegistration::new(
            Some(DESIGN_MODULE_SKETCH.to_owned()),
            None,
            crate::records::identity::ReferenceRun::unlocated(Vec::new()),
        )
        .expect("valid module registration"),
    }
}

fn member_run_test_scope(
    record_index: u32,
    byte_offset: u64,
    bound_entity: Option<&str>,
) -> crate::records::feature::scope::DesignParameterScope {
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignParameterScopeDraft, DesignScopePayloadMut,
        DesignSketchEntityBinding,
    };
    use crate::records::identity::ReferenceRun;
    use crate::records::references::DesignClassTag;

    let mut scope = DesignParameterScope::try_new(
        DesignParameterScopeDraft {
            id: format!("f3d:Design/BulkStream.dat:design-parameter-scope#{byte_offset}"),
            byte_offset,
            class_tag: DesignClassTag::try_from("395".to_owned()).unwrap(),
            record_index,
            frame_length: 200,
            kind_offset: byte_offset + 110,
            feature_ordinal: std::num::NonZeroU32::MIN,
            feature_ordinal_offset: 0,
            history_state_id: None,
            previous_history_state_id: None,
            previous_history_state_id_offset: None,
            reference_count_offset: byte_offset + 80,
            reference_members: ReferenceRun::from_columns(
                vec![record_index + 1],
                vec![byte_offset + 85],
                "reference_members",
            )
            .unwrap(),
            payload: DesignFeatureKind::Skizze.try_into().unwrap(),
            unclosed_construction_operand_groups: Vec::new(),
            paired_class_tag: DesignClassTag::try_from("261".to_owned()).unwrap(),
            paired_byte_offset: byte_offset + 200,
        }
        .with_fixture_layout(),
    )
    .unwrap();
    if let (Some(entity_id), DesignScopePayloadMut::Skizze(slot)) =
        (bound_entity, scope.payload_mut())
    {
        *slot = Some(DesignSketchEntityBinding {
            entity_id: crate::records::identity::DesignEntityId::try_from(entity_id.to_owned())
                .expect("valid entity ID"),
            entity_reference_offset: byte_offset + 90,
        });
    }
    scope
}

#[test]
fn member_run_sketch_scope_prefers_the_scope_bound_to_the_entity() {
    let entity = member_run_test_entity();
    let stream = "f3d:Design/BulkStream.dat";
    let owner_before = |scopes: &[crate::records::feature::scope::DesignParameterScope],
                        next_entity_offset| {
        crate::design::decode::sketch::member_run_sketch_scope(
            scopes,
            stream,
            &entity,
            next_entity_offset,
        )
        .map(|scope| scope.record_index)
    };
    let owner = |scopes: &[crate::records::feature::scope::DesignParameterScope]| {
        owner_before(scopes, None)
    };
    assert_eq!(
        owner(&[
            member_run_test_scope(178, 1000, Some("0_173")),
            member_run_test_scope(956, 5000, None),
        ]),
        Some(178)
    );
    assert_eq!(
        owner(&[
            member_run_test_scope(178, 1000, None),
            member_run_test_scope(956, 5000, None),
        ]),
        None
    );
    assert_eq!(
        owner_before(
            &[member_run_test_scope(178, 1000, Some("0_999"))],
            Some(2000)
        ),
        None
    );
    assert_eq!(
        owner_before(
            &[
                member_run_test_scope(178, 1000, Some("0_999")),
                member_run_test_scope(956, 1500, None),
            ],
            Some(2000)
        ),
        Some(956)
    );
    assert_eq!(owner(&[member_run_test_scope(178, 1000, None)]), Some(178));
}

#[test]
fn typed_profile_compact_sketch_head_supplies_identity_placement() {
    let header = |bytes: &mut Vec<u8>, class: &[u8; 3], record: u32| {
        bytes.extend_from_slice(&3u32.to_le_bytes());
        bytes.extend_from_slice(class);
        bytes.extend_from_slice(&record.to_le_bytes());
    };
    let mut bytes = Vec::new();
    header(&mut bytes, b"280", 100);
    bytes.resize(40, 0);
    let paired = bytes.len();
    header(&mut bytes, b"281", 100);
    bytes.extend_from_slice(&[0; 8]);
    typed_sketch_reference(&mut bytes, 200);
    let head = bytes.len();
    header(&mut bytes, b"282", 200);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&[1, 0]);
    typed_sketch_reference(&mut bytes, 7);
    header(&mut bytes, b"283", 201);
    let entity =
        crate::records::identity::DesignEntityId::try_from("Sketch_100".to_owned()).unwrap();
    let parse = |bytes: &[u8]| {
        let records = crate::design::test_support::indexed_record_offsets_for_test(bytes);
        crate::design::decode::sketch::parse_member_run_head_placement(bytes, 0, &entity, &records)
    };
    let placement = parse(&bytes).expect("typed compact head");
    assert_eq!(placement.frame_length(), 74);
    assert_eq!(placement.paired_byte_offset(), u64_from_index(paired));
    assert_eq!(placement.byte_offset(), u64_from_index(head));
    assert!(placement.member_run_head());
    assert_eq!(placement.transform_offset(), None);
    assert_eq!(
        *placement.transform(),
        crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY.rows()
    );
    let mut invalid = bytes.clone();
    invalid[head + crate::layout::sketch_typed_compact_member_head::FLAGS] = 0;
    assert!(parse(&invalid).is_none());
    let mut invalid = bytes.clone();
    invalid.insert(head + 74, 0);
    assert!(parse(&invalid).is_none());
}
