// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::operands::bind_extrude_selection_geometry;
use crate::design::decode::operands::bind_extrude_selection_identities;
use cadmpeg_core::decode::u64_from_index;

use crate::design::decode::operands::parse_extrude_selection_group;
use crate::design::decode::operands::parse_extrude_selection_member;

use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::resolved_extrude_profile_selection;
use crate::ids::neutral_sketch_curve_id;
use crate::ids::neutral_sketch_point_id;
use crate::records::decal::DesignRecordHeader;

use crate::records::feature::scope::DesignParameterScope;

use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::sketch_geometry::SketchPoint;
use crate::records::sketch_relations::SketchRelationOperand;

use crate::records::topology::construction::DesignConstructionOperandIdentity;
use crate::records::topology::construction::DesignConstructionPersistentIdentity;

use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use cadmpeg_ir::math::Point2;
use cadmpeg_ir::math::Point3;
use cadmpeg_ir::math::Vector3;
use cadmpeg_ir::sketches::Sketch;
use cadmpeg_ir::sketches::SketchEntity;
use cadmpeg_ir::sketches::SketchEntityId;
use cadmpeg_ir::sketches::SketchEntityUse;
use cadmpeg_ir::sketches::SketchGeometry;
use cadmpeg_ir::sketches::SketchGeometryDefinition;
use cadmpeg_ir::sketches::SketchId;

#[test]
fn extrude_selection_group_and_members_have_exact_counted_frames() {
    crate::test_support::with_decode_context(|decode_ctx| {
        let scope = DesignParameterScope::try_new(
            crate::records::feature::scope::DesignParameterScopeDraft {
                id: "f3d:Design/BulkStream.dat:scope#12".into(),
                byte_offset: 1000,
                class_tag: crate::records::references::DesignClassTag::try_from("301".to_owned())
                    .unwrap(),
                record_index: 12,
                frame_length: 200,
                kind_offset: 1100,
                feature_ordinal: std::num::NonZeroU32::MIN,
                feature_ordinal_offset: 0,
                history_state_id: None,

                previous_history_state_id: None,
                previous_history_state_id_offset: None,
                reference_count_offset: 1080,
                reference_members: crate::records::identity::ReferenceRun::from_columns(
                    vec![100],
                    vec![1085],
                    "reference_members",
                )
                .unwrap(),
                payload: crate::records::feature::scope::DesignFeatureKind::Extrude
                    .try_into()
                    .unwrap(),
                unclosed_construction_operand_groups: Vec::new(),
                paired_class_tag: crate::records::references::DesignClassTag::try_from(
                    "261".to_owned(),
                )
                .unwrap(),
                paired_byte_offset: 1200,
            }
            .with_fixture_layout(),
        )
        .unwrap();
        let record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#100".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("331".to_owned())
                .unwrap(),
            record_index: 100,
        };
        let mut group_bytes = Vec::new();
        indexed_header(&mut group_bytes, *b"331", 100);
        group_bytes.extend_from_slice(&[0; 10]);
        group_bytes.push(1);
        group_bytes.extend_from_slice(&12u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        group_bytes.extend_from_slice(&2u32.to_le_bytes());
        for member in [200u32, 201] {
            group_bytes.push(1);
            group_bytes.extend_from_slice(&member.to_le_bytes());
            group_bytes.extend_from_slice(&[0; 6]);
        }
        group_bytes.extend_from_slice(&180u32.to_le_bytes());
        group_bytes.extend_from_slice(&0.25f64.to_le_bytes());
        group_bytes.extend_from_slice(&180u32.to_le_bytes());
        group_bytes.push(1);
        group_bytes.extend_from_slice(&102u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        group_bytes.extend_from_slice(&[1, 1, 0, 1]);
        group_bytes.extend_from_slice(&101u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 7]);
        group_bytes.push(1);
        group_bytes.extend_from_slice(&12u32.to_le_bytes());
        group_bytes.extend_from_slice(&[0; 6]);
        let paired_at = group_bytes.len();
        indexed_header(&mut group_bytes, *b"259", 100);

        let mut group = parse_extrude_selection_group(&group_bytes, &scope, 0, &record)
            .expect("group decode resources")
            .expect("counted Extrude selection group");
        assert_eq!(
            group
                .members()
                .iter()
                .map(|member| member.value)
                .collect::<Vec<_>>(),
            [200, 201]
        );
        assert_eq!(group.opaque_index.get(), 180);
        assert_eq!(group.opaque_scalar().get(), 0.25);
        assert_eq!(group.variant, Some(true));
        assert_eq!(group.paired_byte_offset(), u64_from_index(paired_at));

        let member_record = DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:record#200".into(),
            byte_offset: 0,
            class_tag: crate::records::references::DesignClassTag::try_from("290".to_owned())
                .unwrap(),
            record_index: 200,
        };
        let mut member_bytes = Vec::new();
        indexed_header(&mut member_bytes, *b"290", 200);
        member_bytes.extend_from_slice(&[0; 10]);
        member_bytes.extend_from_slice(&586u64.to_le_bytes());
        lp_utf16(&mut member_bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
        lp_utf16(&mut member_bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
        member_bytes.extend_from_slice(&2u32.to_le_bytes());
        member_bytes.extend_from_slice(&[0; 5]);
        indexed_header(&mut member_bytes, *b"290", 201);

        let mut member = parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
            .expect("fixed Extrude selection member");
        assert_eq!(member.local_id, 586);
        assert_eq!(member.next_byte_offset(), 190);
        assert_eq!(member.next_record_index, 201);
        assert!(!member.tail_slot_present);
        assert_eq!(member.tail_slot_offset, 185);

        member_bytes[185] = 1;
        let member_with_slot =
            parse_extrude_selection_member(&member_bytes, &group, 0, &member_record)
                .expect("Extrude selection member with present tail slot");
        assert!(member_with_slot.tail_slot_present);
        assert_eq!(member_with_slot.tail_slot_offset, 185);

        let terminal_member =
            parse_extrude_selection_member(&member_bytes[..190], &group, 0, &member_record)
                .expect("terminal fixed Extrude selection member");
        assert_eq!(terminal_member.next_byte_offset(), 190);
        assert_eq!(terminal_member.next_record_index, 0);

        let mut long_member_bytes = member_bytes[..190].to_vec();
        long_member_bytes.push(0);
        long_member_bytes.extend_from_slice(&2u32.to_le_bytes());
        long_member_bytes.extend_from_slice(&1u64.to_le_bytes());
        long_member_bytes.push(0);
        indexed_header(&mut long_member_bytes, *b"290", 201);
        let long_member =
            parse_extrude_selection_member(&long_member_bytes, &group, 0, &member_record)
                .expect("selection member with the fourteen-byte tail");
        assert_eq!(long_member.local_id, 586);
        assert_eq!(long_member.next_byte_offset(), 204);
        assert_eq!(long_member.next_record_index, 201);
        assert!(long_member.tail_slot_present);
        assert_eq!(
            serde_json::from_value::<
                crate::records::topology::extrude_selection::DesignExtrudeSelectionMember,
            >(serde_json::to_value(&long_member).unwrap())
            .unwrap(),
            long_member
        );
        let terminal_long_member =
            parse_extrude_selection_member(&long_member_bytes[..204], &group, 0, &member_record)
                .expect("terminal selection member with the fourteen-byte tail");
        assert_eq!(terminal_long_member.next_byte_offset(), 204);
        assert_eq!(terminal_long_member.next_record_index, 0);
        for at in 190..204 {
            let mut invalid = long_member_bytes.clone();
            invalid[at] ^= 0x80;
            assert!(parse_extrude_selection_member(&invalid, &group, 0, &member_record).is_none());
        }
        long_member_bytes[185] = 0;
        assert!(
            parse_extrude_selection_member(&long_member_bytes, &group, 0, &member_record).is_none()
        );

        // Extended tails run from the 190-byte frame to the next indexed header.
        let extended = |slot: u8, tail: &[u8]| {
            let mut bytes = member_bytes[..190].to_vec();
            bytes[185] = slot;
            bytes.extend_from_slice(tail);
            indexed_header(&mut bytes, *b"307", 321);
            bytes
        };
        for (slot, tail) in [
            (0u8, vec![0u8; 5]),
            (
                0,
                [&[0x2d, 0][..], &[7u8; 21][..], &[1, 0, 0, 0][..]].concat(),
            ),
            (
                1,
                [
                    &[0u8; 4][..],
                    &1u32.to_le_bytes(),
                    &3u32.to_le_bytes(),
                    &[9u8; 20],
                ]
                .concat(),
            ),
            (1, vec![0x55; 196]),
            (1, vec![0; 1024]),
        ] {
            let bytes = extended(slot, &tail);
            let member = parse_extrude_selection_member(&bytes, &group, 0, &member_record)
                .expect("selection member with an extended tail");
            assert_eq!(member.local_id, 586);
            assert_eq!(member.next_byte_offset(), u64_from_index(190 + tail.len()));
            assert_eq!(member.next_record_index, 321);
            assert_eq!(member.tail_slot_present, slot == 1);
            assert!(member.scope_tail.is_none());
            // An incomplete header is not a boundary: the tail then runs to
            // the end of the stream.
            let header_at = 190 + tail.len();
            let mut incomplete = Vec::new();
            let mut broken = bytes.clone();
            broken[header_at + 4] = b'x';
            incomplete.push(broken);
            incomplete.push(bytes[..bytes.len() - 1].to_vec());
            for broken in incomplete {
                let ended = parse_extrude_selection_member(&broken, &group, 0, &member_record)
                    .expect("selection member whose tail runs to the stream end");
                assert_eq!(ended.next_record_index, 0);
                assert_eq!(ended.next_byte_offset(), u64_from_index(broken.len()));
            }
        }
        // A tail past the length bound, and the typed tail lengths, are not extended.
        for tail_len in [1025usize, 14, 50] {
            let bytes = extended(0, &vec![0x55; tail_len]);
            assert!(
                parse_extrude_selection_member(&bytes, &group, 0, &member_record).is_none(),
                "tail length {tail_len}"
            );
        }
        // A tail with no following header runs to the end of the stream.
        let mut terminal_tail = member_bytes[..190].to_vec();
        terminal_tail.extend_from_slice(&[0x55; 130]);
        let terminal_extended =
            parse_extrude_selection_member(&terminal_tail, &group, 0, &member_record)
                .expect("selection member with an extended tail at the stream end");
        assert_eq!(terminal_extended.next_byte_offset(), 320);
        assert_eq!(terminal_extended.next_record_index, 0);
        // The header-bounded length limit does not apply at the stream end.
        terminal_tail.extend_from_slice(&vec![0; 1024]);
        let long_terminal =
            parse_extrude_selection_member(&terminal_tail, &group, 0, &member_record)
                .expect("selection member with a long tail at the stream end");
        assert_eq!(
            long_terminal.next_byte_offset(),
            u64_from_index(terminal_tail.len())
        );
        assert_eq!(long_terminal.next_record_index, 0);

        let mut edge_identity_bytes = Vec::new();
        indexed_header(&mut edge_identity_bytes, *b"278", 5887);
        edge_identity_bytes.extend_from_slice(&[0; 12]);
        edge_identity_bytes.push(1);
        edge_identity_bytes.extend_from_slice(&5890u32.to_le_bytes());
        edge_identity_bytes.extend_from_slice(&[0; 6]);
        edge_identity_bytes.extend_from_slice(&1u32.to_le_bytes());
        lp_utf16(
            &mut edge_identity_bytes,
            "ad3001bb-a0fc-44c2-9b7a-c8b8fb70bfc0",
        );
        lp_utf16(
            &mut edge_identity_bytes,
            "1d8b67fc-c638-4af3-b13d-776dce4f472d",
        );
        let edge_identity =
            crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
                .expect("fixed edge-treatment selection identity");
        assert_eq!(edge_identity.local_id, 5890);
        assert!(!edge_identity.layout.is_compact());
        assert_eq!(edge_identity.layout.local_id_offset(), 24);
        assert_eq!(edge_identity.asset_id_offset, 42);
        assert_eq!(edge_identity.context_id_offset, 118);

        edge_identity_bytes.remove(22);
        let compact_edge_identity =
            crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
                .expect("compact fixed edge-treatment selection identity");
        assert!(compact_edge_identity.layout.is_compact());
        assert_eq!(compact_edge_identity.local_id, 5890);
        assert_eq!(compact_edge_identity.layout.local_id_offset(), 23);
        assert_eq!(compact_edge_identity.asset_id_offset, 41);
        assert_eq!(compact_edge_identity.context_id_offset, 117);

        edge_identity_bytes.remove(21);
        let shortest_edge_identity =
            crate::design::decode::operands::parse_edge_identity_member(&edge_identity_bytes, 0)
                .expect("short compact edge-treatment selection identity");
        assert!(shortest_edge_identity.layout.is_compact());
        assert_eq!(shortest_edge_identity.local_id, 5890);
        assert_eq!(shortest_edge_identity.layout.local_id_offset(), 22);
        assert_eq!(shortest_edge_identity.asset_id_offset, 40);
        assert_eq!(shortest_edge_identity.context_id_offset, 116);

        group.id = "f3d:Design/BulkStream.dat:selection-group#100".into();
        member.id = "f3d:Design/BulkStream.dat:selection-member#200".into();
        let mut relocated = member.into_draft();
        relocated.byte_offset += 74;
        relocated.local_id_offset += 74;
        relocated.asset_id_offset += 74;
        relocated.context_id_offset += 74;
        relocated.tail_slot_offset += 74;
        relocated.next_byte_offset += 74;
        member =
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMember::try_new(
                relocated,
            )
            .unwrap();
        let identity = DesignConstructionOperandIdentity::try_new(
        crate::records::topology::construction::DesignConstructionOperandIdentityDraft {
            id: "f3d:Design/BulkStream.dat:operand-identity#50".into(),
            group_record_index: 50,
            wrappers: vec![crate::records::topology::construction::DesignIdentityWrapper {
                record_index: 150,
                byte_offset: 50,
                class_tag: crate::records::references::DesignClassTag::try_from("289".to_owned())
                    .unwrap(),
            }],
            following_record_index: 200,
            following_byte_offset: 74,
            following_class_tag: crate::records::references::DesignClassTag::try_from(
                "290".to_owned(),
            )
            .unwrap(),
            tracking_path: None,
            persistent_identity: Some(
                DesignConstructionPersistentIdentity::try_new(
                    crate::records::topology::construction::DesignConstructionPersistentIdentityDraft {
                        local_id: 586,
                        local_id_offset: 95,
                        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "df9087bd-02a6-4a3f-a132-7e69990f323c".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 107,
                        context_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "0b2382d1-caaf-4eb9-b40d-a6322a7ed829".to_owned(),
                        )
                        .unwrap(),
                        context_id_offset: 187,
                        tail_slot_present: false,
                        tail_slot_offset: 259,
                        next_record_index: 201,
                        next_byte_offset: 264,
                    },
                )
                .unwrap(),
            ),
        },
    )
    .unwrap();
        bind_extrude_selection_identities(
            std::slice::from_mut(&mut member),
            std::slice::from_ref(&identity),
        );
        assert_eq!(member.operand_identity_ids, [identity.id]);
        let mut owning_scope = scope;
        if let crate::records::feature::scope::DesignScopePayloadMut::Extrude(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusion(slot)
        | crate::records::feature::scope::DesignScopePayloadMut::Extrusao(slot) =
            owning_scope.payload_mut()
        {
            slot.get_or_insert_with(Default::default).extrude_profile = Some(
                DesignSketchProfileOperand::try_new(
                    crate::records::topology::sketch_profile::DesignSketchProfileOperandDraft {
                        scope_reference_ordinal: 1,
                        record_index: 300,
                        byte_offset: 3000,
                        class_tag: crate::records::references::DesignClassTag::try_from(
                            "308".to_owned(),
                        )
                        .unwrap(),
                        asset_id: crate::records::mesh::DesignRelaxedGuidText::try_from(
                            "df9087bd-02a6-4a3f-a132-7e69990f323c".to_owned(),
                        )
                        .unwrap(),
                        asset_id_offset: 3040,
                        entity_id: crate::records::identity::DesignEntityId::try_from(
                            "0_172".to_owned(),
                        )
                        .expect("valid entity identity"),
                        entity_reference_offset: 3120,
                        region_selection: None,
                        paired_class_tag: crate::records::references::DesignClassTag::try_from(
                            "259".to_owned(),
                        )
                        .unwrap(),
                        paired_byte_offset: 3200,
                    },
                )
                .unwrap(),
            );
        }
        let curve = SketchCurveIdentity {
            id: "f3d:Design/BulkStream.dat:sketch-curve#400".into(),
            record_index: 400,
            owner_reference: Some(172),
            class_tag: crate::records::references::DesignClassTag::try_from("270".to_owned())
                .unwrap(),
            byte_offset: 4000,
            geometry_offset: 100,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(586).unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
        };
        bind_extrude_selection_geometry(
            std::slice::from_mut(&mut member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[],
            &[curve.clone()],
        );
        assert!(matches!(
            member.resolved_geometry,
            Some(SketchRelationOperand::Curve {
                record_index: 400,
                primary_id: 586,
                secondary_id: 0,
            })
        ));

        // Ownerless curve uniquely matches
        let mut ownerless_curve = curve.clone();
        ownerless_curve.owner_reference = None;
        let mut test_member = member.clone();
        test_member.resolved_geometry = None;
        bind_extrude_selection_geometry(
            std::slice::from_mut(&mut test_member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[],
            &[ownerless_curve.clone()],
        );
        assert!(matches!(
            test_member.resolved_geometry,
            Some(SketchRelationOperand::Curve {
                record_index: 400,
                primary_id: 586,
                secondary_id: 0,
            })
        ));

        // Owned plus ownerless curve with same ID resolves to None (ambiguity)
        let mut second_curve = ownerless_curve;
        second_curve.record_index = 401;
        test_member.resolved_geometry = None;
        bind_extrude_selection_geometry(
            std::slice::from_mut(&mut test_member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[],
            &[curve, second_curve],
        );
        assert!(test_member.resolved_geometry.is_none());

        // Ownerless point uniquely matches
        let point = SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
            id: "f3d:Design/BulkStream.dat:sketch-point#500".into(),
            record_index: 500,
            owner_reference: None,
            class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                .unwrap(),
            byte_offset: 5000,
            coordinate_offset: 141,
            companion: crate::records::sketch_geometry::SketchPointCompanion {
                incident_curves: Vec::new(),
            },
            record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                586,
                crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                None,
                0.0,
            ),
            paired_reference: 0,
            coordinates: Point2::new(0.0, 0.0),
        })
        .unwrap();

        test_member.resolved_geometry = None;
        bind_extrude_selection_geometry(
            std::slice::from_mut(&mut test_member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[point.clone()],
            &[],
        );
        assert!(matches!(
            test_member.resolved_geometry,
            Some(SketchRelationOperand::Point {
                record_index: 500,
                persistent_id: Some(586),
            })
        ));

        // Owned plus ownerless point with same ID resolves to None (ambiguity)
        let owned_point =
            SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
                id: "f3d:Design/BulkStream.dat:sketch-point#501".into(),
                record_index: 501,
                owner_reference: Some(172),
                class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned())
                    .unwrap(),
                byte_offset: 5100,
                coordinate_offset: 141,
                companion: crate::records::sketch_geometry::SketchPointCompanion {
                    incident_curves: Vec::new(),
                },
                record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
                    586,
                    crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
                    None,
                    0.0,
                ),
                paired_reference: 0,
                coordinates: Point2::new(0.0, 0.0),
            })
            .unwrap();

        test_member.resolved_geometry = None;
        bind_extrude_selection_geometry(
            std::slice::from_mut(&mut test_member),
            std::slice::from_ref(&group),
            std::slice::from_ref(&owning_scope),
            &[point, owned_point],
            &[],
        );
        assert!(test_member.resolved_geometry.is_none());

        let all_members = group
            .members()
            .iter()
            .map(|member| member.value)
            .collect::<Vec<_>>();
        group.try_set_members(all_members[..1].to_vec()).unwrap();
        let sketch_id = SketchId::mint("f3d:model:sketch#172").unwrap();
        let sketch = Sketch {
            id: sketch_id.clone(),
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![vec![SketchEntityUse {
                entity: neutral_sketch_curve_id(&sketch_id, 586, 0),
                reversed: false,
            }]])
            .unwrap(),
            native_ref: None,
        };
        let profile_work = ProfileGeometryWork::new();
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    profile_work: &profile_work,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            }) if actual_sketch == &sketch_id && profiles.as_slice() == [0]
        ));
        let mut point_member = member.clone();
        point_member.id = "f3d:Design/BulkStream.dat:selection-member#201".into();
        let mut draft = point_member.into_draft();
        draft.record_index = 201;

        point_member =
            crate::records::topology::extrude_selection::DesignExtrudeSelectionMember::try_new(
                draft,
            )
            .unwrap();
        point_member.group_member_ordinal = 1;
        point_member.local_id = 587;
        point_member.resolved_geometry = Some(SketchRelationOperand::Point {
            record_index: 401,
            persistent_id: Some(587),
        });
        group.try_set_members(all_members).unwrap();
        let mut sketch = sketch;
        let second_profile_id = SketchEntityId::mint("synthetic:test:id#second-profile").unwrap();
        sketch.profiles.push_single(SketchEntityUse {
            entity: second_profile_id.clone(),
            reversed: false,
        });
        let point_entity = SketchEntity::new(
            neutral_sketch_point_id(&sketch_id, 587),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Point {
                position: Point2::new(0.5, 1.0),
            })
            .unwrap(),
        );
        let line_entity = SketchEntity::new(
            neutral_sketch_curve_id(&sketch_id, 586, 0),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 0.0),
                end: Point2::new(1.0, 0.0),
            })
            .unwrap(),
        );
        let second_profile_entity = SketchEntity::new(
            second_profile_id,
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(0.0, 1.0),
                end: Point2::new(1.0, 1.0),
            })
            .unwrap(),
        );
        let profile_entities = [line_entity, second_profile_entity, point_entity];
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                &[member.clone(), point_member],
                &sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &profile_entities,
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    profile_work: &profile_work,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            }) if actual_sketch == &sketch_id && profiles.as_slice() == [0, 1]
        ));
        member.resolved_geometry = None;
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    profile_work: &profile_work,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchSelection {
                sketch: ref actual_sketch,
                selections: ref actual_selections,
            }) if actual_sketch == &sketch_id && actual_selections.as_slice() == [group.id.clone()]
        ));
        let mut single_profile_sketch = sketch.clone();
        single_profile_sketch
            .profiles
            .edit(|profiles| profiles.truncate(1))
            .unwrap();
        assert!(matches!(
            resolved_extrude_profile_selection(
                &sketch_id,
                &group,
                std::slice::from_ref(&member),
                &single_profile_sketch,
                crate::design::profile_select::ExtrudeProfileResolution {
                    entities: &[],
                    spatial_sketches: &[],
                    spatial_entities: &[],
                    histories: &[],
                    scope_histories: &std::collections::HashMap::new(),
                    linear_tolerance: 1.0e-6,
                    angular_tolerance: 1.0e-9,
                    profile_work: &profile_work,
                    ctx: decode_ctx,
                }
                .scoped(&[]),
                None,
                None,
            ).unwrap(),
            cadmpeg_ir::features::ProfileRef::Planar(cadmpeg_ir::features::PlanarProfileRef::SketchProfiles {
                sketch: ref actual_sketch,
                ref profiles,
            }) if actual_sketch == &sketch_id && profiles.as_slice() == [0]
        ));
    });
}

/// Build a 240-byte selection member with synthetic values.
fn scope_tail_member_bytes(scope: u32) -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"290", 200);
    bytes.extend_from_slice(&[0; 10]);
    bytes.extend_from_slice(&586u64.to_le_bytes());
    lp_utf16(&mut bytes, "df9087bd-02a6-4a3f-a132-7e69990f323c");
    lp_utf16(&mut bytes, "0b2382d1-caaf-4eb9-b40d-a6322a7ed829");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 5]);
    assert_eq!(bytes.len(), 190);
    bytes.push(0);
    bytes.extend_from_slice(&0.1f64.to_le_bytes());
    bytes.extend_from_slice(&0x104u32.to_le_bytes());
    for (reference, separator) in [(31u32, &[1u8, 0, 0][..]), (32, &[0][..]), (scope, &[][..])] {
        bytes.push(1);
        bytes.extend_from_slice(&u64::from(reference).to_le_bytes());
        bytes.extend_from_slice(&[0; 2]);
        bytes.extend_from_slice(separator);
    }
    assert_eq!(bytes.len(), 240);
    indexed_header(&mut bytes, *b"290", 201);
    bytes
}

#[test]
fn extrude_selection_scope_tail_member_has_exact_240_byte_frame() {
    use crate::design::decode::operands::parse_extrude_identity_member;
    let bytes = scope_tail_member_bytes(12);
    let member =
        parse_extrude_identity_member(&bytes, 0, Some(12)).expect("scope-tail selection member");
    assert_eq!(member.next_byte_offset, 240);
    assert_eq!(member.next_record_index, 201);
    assert!(!member.tail_slot_present);
    let tail = member.scope_tail.expect("scope tail");
    assert_eq!(f64::from_bits(tail.opaque_scalar_bits), 0.1);
    assert_eq!(tail.opaque_word, 0x104);
    assert_eq!(tail.chain_references, [31, 32]);
    assert_eq!(tail.scope_record_index, 12);
    // Terminal form.
    let terminal = parse_extrude_identity_member(&bytes[..240], 0, Some(12))
        .expect("terminal scope-tail member");
    assert_eq!(terminal.next_record_index, 0);
    assert_eq!(terminal.next_byte_offset, 240);
    // Callers that supply no scope refuse the form.
    assert!(parse_extrude_identity_member(&bytes, 0, None).is_none());
    // The closing reference has to name the group scope.
    assert!(parse_extrude_identity_member(&bytes, 0, Some(13)).is_none());
    assert!(parse_extrude_identity_member(&scope_tail_member_bytes(13), 0, Some(12)).is_none());
}

#[test]
fn extrude_selection_scope_tail_member_refuses_each_broken_condition() {
    use crate::design::decode::operands::parse_extrude_identity_member;
    let bytes = scope_tail_member_bytes(12);
    // Every fixed byte of the tail: leading zero, word, separators, reference
    // markers and padding.
    for at in [
        190, 199, 200, 201, 202, 203, 213, 214, 215, 216, 217, 227, 228, 229, 239,
    ] {
        let mut broken = bytes.clone();
        broken[at] ^= 0x40;
        assert!(
            parse_extrude_identity_member(&broken, 0, Some(12)).is_none(),
            "byte {at}"
        );
    }
    // A truncated tail is not the scope-tail form; it can only be an
    // extended tail that runs to the end of the stream.
    for len in [191, 205, 239] {
        let truncated = parse_extrude_identity_member(&bytes[..len], 0, Some(12))
            .expect("extended tail at the stream end");
        assert!(truncated.scope_tail.is_none());
        assert_eq!(truncated.next_byte_offset, u64_from_index(len));
    }
    // The typed lengths are not extended tails.
    assert!(parse_extrude_identity_member(&bytes[..240], 0, Some(13)).is_none());
    // A present optional slot does not admit the form.
    let mut slot = bytes.clone();
    slot[185] = 1;
    assert!(parse_extrude_identity_member(&slot, 0, Some(12)).is_none());
}
