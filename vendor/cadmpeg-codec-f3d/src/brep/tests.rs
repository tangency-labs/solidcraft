// SPDX-License-Identifier: Apache-2.0

use super::{persistent_design_links, persistent_subentity_tags, Brep};
use crate::records::recipes::CreationTimestamp;
use crate::records::sketch_links::{PersistentDesignLink, PersistentSubentityTag, SketchCurveLink};
use cadmpeg_asm::brep::annotations::AnnotationRecord;
use cadmpeg_asm::brep::records::BodyNativeKey;
use cadmpeg_asm::brep::AsmBrep;
use cadmpeg_ir::attributes::{AttributeTarget, AttributeValue, SourceAttribute};
use cadmpeg_ir::ids::BodyId;
use cadmpeg_ir::ids::{FaceId, RegionId};
use cadmpeg_ir::topology::{Body, BodyKind, Region};
use std::collections::{HashMap, HashSet};

fn with_limits<T>(
    max_retained: u64,
    run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T,
) -> T {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let mut policy = cadmpeg_core::decode::DecodePolicy::service();
    policy.limits.max_retained_bytes = max_retained;
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(&[], &arena, &policy)
        .expect("test decode context");
    run(&ctx)
}

fn with_context<T>(run: impl FnOnce(&cadmpeg_core::decode::DecodeContext<'_>) -> T) -> T {
    with_limits(u64::MAX, run)
}

fn generic_tag_attribute(
    target: AttributeTarget,
    versions: (i64, i64),
    group_count: i64,
    groups: Vec<AttributeValue>,
) -> SourceAttribute {
    SourceAttribute {
        id: "f3d:brep:attribute#1".try_into().expect("valid identity"),
        target,
        name: "ATTRIB_CUSTOM-attrib".into(),
        values: [
            AttributeValue::String("generic_tag_attrib_def".into()),
            AttributeValue::Integer(versions.0),
            AttributeValue::Integer(versions.1),
            AttributeValue::Integer(-1),
            AttributeValue::String("generic_tag_attrib_def ".into()),
            AttributeValue::Integer(group_count),
        ]
        .into_iter()
        .chain(groups)
        .collect(),
    }
}

#[test]
fn generic_tag_payload_accepts_both_equal_envelope_versions() {
    for (version, groups) in [
        (
            2,
            vec![
                AttributeValue::Integer(7),
                AttributeValue::String("97".into()),
                AttributeValue::Integer(0),
                AttributeValue::Integer(1),
                AttributeValue::Integer(302),
            ],
        ),
        (
            3,
            vec![
                AttributeValue::Integer(7),
                AttributeValue::String("97".into()),
                AttributeValue::Integer(0),
                AttributeValue::Integer(1),
                AttributeValue::Integer(302),
                AttributeValue::Integer(0),
            ],
        ),
    ] {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
            (version, version),
            1,
            groups,
        );
        assert_eq!(persistent_subentity_tags(&attribute).unwrap().len(), 1);
        assert_eq!(
            persistent_subentity_tags(&attribute).unwrap()[0].design_references,
            [302]
        );
    }
}

#[test]
fn generic_tag_payload_rejects_mixed_or_unsupported_envelope_versions() {
    let groups = vec![
        AttributeValue::Integer(7),
        AttributeValue::String("97".into()),
        AttributeValue::Integer(0),
        AttributeValue::Integer(0),
        AttributeValue::Integer(0),
    ];
    for versions in [(2, 3), (1, 1), (4, 4)] {
        let attribute = generic_tag_attribute(
            AttributeTarget::Face(FaceId::mint("f3d:test:face#1").expect("identity grammar")),
            versions,
            1,
            groups.clone(),
        );
        assert!(persistent_subentity_tags(&attribute).unwrap().is_empty());
    }
}

#[test]
fn generic_tag_payload_binds_modern_body_design_links() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
        (2, 2),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
        ],
    );
    let links = persistent_design_links(&attribute).unwrap();
    assert_eq!(links.len(), 1);
    assert_eq!(links[0].design_id.as_str(), "301");
    assert_eq!(links[0].design_reference, 1);
}

#[test]
fn generic_tag_payload_binds_legacy_body_design_links() {
    let attribute = generic_tag_attribute(
        AttributeTarget::Body(BodyId::mint("f3d:test:body#1").expect("identity grammar")),
        (3, 3),
        1,
        vec![
            AttributeValue::Integer(3),
            AttributeValue::String("301".into()),
            AttributeValue::Integer(1),
            AttributeValue::Integer(0),
            AttributeValue::Integer(0),
        ],
    );
    assert_eq!(persistent_design_links(&attribute).unwrap().len(), 1);
}

#[test]
fn brep_qualification_rewrites_owned_ids_and_cross_references() {
    let body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let region = RegionId::mint("f3d:brep:entity#2").expect("identity grammar");
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![Body {
                id: body.clone(),
                kind: BodyKind::default(),
                regions: vec![region.clone()],
                transform: None,
                name: None,
                color: None,
                visible: None,
            }],
            regions: vec![Region {
                id: region,
                body: body.clone(),
                shells: Vec::new(),
            }],

            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body,
                record_index: 1,
                body_ordinal: 0,
                source_brep: Some("BREP.source.smbh".into()),
                asm_body_key: Some(7),
            }],
            annotation_records: vec![AnnotationRecord {
                id: "f3d:brep:entity#1".into(),
                stream: "asset/BREP.source.smbh".into(),
                offset: 10,
                tag: cadmpeg_asm::brep::annotations::AnnotationTag::Record("body".into()),
                derived_fields: Vec::new(),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.qualify_ids(ctx, crate::ids::ID_FORMAT, "source"))
        .expect("qualify BREP");

    let qualified = BodyId::mint("f3d:brep/source/brep:entity#1").expect("identity grammar");
    assert_eq!(brep.asm.bodies[0].id, qualified);
    assert_eq!(brep.asm.regions[0].body, qualified);
    assert_eq!(brep.asm.body_native_keys[0].body, qualified);
    assert_eq!(
        brep.asm
            .body_native_keys
            .iter()
            .find(|record| record.body == qualified)
            .and_then(|record| record.asm_body_key.as_ref()),
        Some(&7)
    );
    assert_eq!(brep.asm.annotation_records[0].id, qualified.as_str());
    assert_eq!(
        brep.asm.body_native_keys[0].source_brep.as_deref(),
        Some("BREP.source.smbh")
    );
}

#[test]
fn body_key_retention_keeps_only_the_selected_connected_graph() {
    let body = |index, region| Body {
        id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: vec![
            RegionId::mint(format!("f3d:brep:entity#{region}")).expect("identity grammar")
        ],
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let native_key = |index, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![
                Body {
                    name: Some("f3d:brep:entity#3".into()),
                    ..body(1, 2)
                },
                body(3, 4),
            ],
            regions: vec![
                Region {
                    id: RegionId::mint("f3d:brep:entity#2").expect("identity grammar"),
                    body: BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
                    shells: Vec::new(),
                },
                Region {
                    id: RegionId::mint("f3d:brep:entity#4").expect("identity grammar"),
                    body: BodyId::mint("f3d:brep:entity#3").expect("identity grammar"),
                    shells: Vec::new(),
                },
            ],

            body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([20])))
        .expect("retain body graph");

    assert_eq!(brep.asm.bodies.len(), 1);
    assert_eq!(brep.asm.bodies[0].id.as_str(), "f3d:brep:entity#3");
    assert_eq!(brep.asm.regions.len(), 1);
    assert_eq!(brep.asm.regions[0].id.as_str(), "f3d:brep:entity#4");
    assert_eq!(brep.asm.body_native_keys.len(), 1);
    assert_eq!(
        brep.asm
            .body_native_keys
            .iter()
            .filter(|record| record.asm_body_key.is_some())
            .count(),
        1
    );
}

#[test]
fn body_key_retention_preserves_derived_links_for_reachable_targets() {
    let body = |index| Body {
        id: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        kind: BodyKind::default(),
        regions: Vec::new(),
        transform: None,
        name: None,
        color: None,
        visible: None,
    };
    let native_key = |index, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        record_index: index,
        body_ordinal: index - 1,
        source_brep: Some("BREP.source.smbh".into()),
        asm_body_key: Some(key),
    };
    let target = |index| {
        AttributeTarget::Body(
            BodyId::mint(format!("f3d:brep:entity#{index}")).expect("identity grammar"),
        )
    };
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![body(1), body(3)],

            body_native_keys: vec![native_key(1, 10), native_key(3, 20)],
            ..AsmBrep::default()
        },
        sketch_curve_links: vec![
            SketchCurveLink {
                id: "link-retained".into(),
                target: target(1),
                sketch_curve_id: 1,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            },
            SketchCurveLink {
                id: "link-dropped".into(),
                target: target(3),
                sketch_curve_id: 3,
                ref_b: 0,
                sense: None,
                role: 0,
                closure: 0,
            },
        ],
        persistent_design_links: vec![
            PersistentDesignLink {
                id: "design-retained".into(),
                target: target(1),
                design_id: "301".to_owned().try_into().unwrap(),

                design_reference: 1,
                ordinal: 0,
            },
            PersistentDesignLink {
                id: "design-dropped".into(),
                target: target(3),
                design_id: "303".to_owned().try_into().unwrap(),

                design_reference: 3,
                ordinal: 0,
            },
        ],
        persistent_subentity_tags: vec![
            PersistentSubentityTag {
                id: "tag-retained".into(),
                target: target(1),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![1],
                ordinal: 0,
            },
            PersistentSubentityTag {
                id: "tag-dropped".into(),
                target: target(3),
                selector: 1,
                token: cadmpeg_core::text::NonBlankString::new("97").unwrap(),
                design_references: vec![3],
                ordinal: 0,
            },
        ],
        creation_timestamps: vec![
            CreationTimestamp {
                id: "time-retained".into(),
                target: target(1),
                record_index: 1,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(1.0).unwrap(),
            },
            CreationTimestamp {
                id: "time-dropped".into(),
                target: target(3),
                record_index: 3,
                unix_microseconds: cadmpeg_ir::scalar::FiniteReal::new(3.0).unwrap(),
            },
        ],
        sketch_region_bodies_omitted: 0,
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
        .expect("retain body graph");

    assert_eq!(brep.sketch_curve_links.len(), 1);
    assert_eq!(brep.persistent_design_links.len(), 1);
    assert_eq!(brep.persistent_subentity_tags.len(), 1);
    assert_eq!(brep.creation_timestamps.len(), 1);
    assert_eq!(brep.persistent_subentity_tags[0].id, "tag-retained");
}

#[test]
fn body_key_retention_preserves_selectorless_neutral_roots() {
    let native_body = BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let projected_body = BodyId::mint("f3d:brep:saved-edge-body#5").expect("identity grammar");
    let mut brep = Brep {
        asm: AsmBrep {
            bodies: vec![
                Body {
                    id: native_body.clone(),
                    kind: BodyKind::Solid,
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                },
                Body {
                    id: projected_body.clone(),
                    kind: BodyKind::Wire,
                    regions: Vec::new(),
                    transform: None,
                    name: None,
                    color: None,
                    visible: None,
                },
            ],

            body_native_keys: vec![BodyNativeKey {
                source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                    crate::ids::ID_FORMAT,
                ),
                body: native_body,
                record_index: 1,
                body_ordinal: 0,
                source_brep: Some("BREP.source.smbh".into()),
                asm_body_key: Some(10),
            }],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    with_context(|ctx| brep.retain_body_keys(ctx, &HashSet::from([10])))
        .expect("retain body graph");

    assert_eq!(brep.asm.bodies.len(), 2);
    assert!(brep.asm.bodies.iter().any(|body| body.id == projected_body));
}

#[test]
fn body_selectors_use_ordinals_only_for_an_all_null_key_lane() {
    let native_key = |ordinal, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: key,
    };
    let mut brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![native_key(0, None), native_key(1, None)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    assert_eq!(brep.body_selectors().len(), 2);
    assert_eq!(
        brep.body_selectors()[&BodyId::mint("f3d:brep:entity#1").expect("identity grammar")],
        1
    );

    brep.asm.body_native_keys[1].asm_body_key = Some(7);
    assert_eq!(
        brep.body_selectors(),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            7
        )])
    );
}

#[test]
fn design_body_selectors_prefer_exact_keys_then_fall_back_to_ordinals() {
    let native_key = |ordinal, key| BodyNativeKey {
        source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
            crate::ids::ID_FORMAT,
        ),
        body: BodyId::mint(format!("f3d:brep:entity#{ordinal}")).expect("identity grammar"),
        record_index: ordinal,
        body_ordinal: ordinal,
        source_brep: Some("BREP.source.smb".into()),
        asm_body_key: Some(key),
    };
    let mut brep = Brep {
        asm: AsmBrep {
            body_native_keys: vec![native_key(0, 1), native_key(1, 0)],
            ..AsmBrep::default()
        },
        ..Brep::default()
    };

    assert_eq!(
        brep.body_selectors_for(&HashSet::from([0])).unwrap(),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#1").expect("identity grammar"),
            0
        )])
    );

    brep.asm.body_native_keys = vec![native_key(0, 436)];
    assert_eq!(
        brep.body_selectors_for(&HashSet::from([0])).unwrap(),
        HashMap::from([(
            BodyId::mint("f3d:brep:entity#0").expect("identity grammar"),
            0
        )])
    );
}

#[test]
fn source_curve_keys_retain_distinct_qualifiers_in_every_payload_form() {
    for qualifier in [0u64, 165, 26302, u64::MAX] {
        let mut forms = vec![vec![
            AttributeValue::Integer(1),
            AttributeValue::Integer(1),
            AttributeValue::Integer(3),
            AttributeValue::String(format!("149 {qualifier} 1 0 2 1")),
        ]];
        if let Ok(qualifier) = i64::try_from(qualifier) {
            for form in [0, 2] {
                let mut values = vec![
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(form),
                    AttributeValue::Integer(149),
                    AttributeValue::Integer(qualifier),
                    AttributeValue::Integer(1),
                    AttributeValue::Integer(2),
                    AttributeValue::Integer(1),
                ];
                if form == 2 {
                    values.push(AttributeValue::Integer(0));
                }
                forms.push(values);
            }
        }
        for values in forms {
            let payload = super::sketch_link_payload(&values).expect("source-curve key");
            assert_eq!((payload.sketch_curve_id, payload.ref_b), (149, qualifier));
        }
    }
}

/// Topology of one body in a sketch-region fixture.
#[derive(Clone, Copy)]
struct RegionBody {
    edges: u32,
    linked_edges: u32,
    coedge_links: bool,
    face_attribute: bool,
    wire_edge: bool,
}

const SKETCH_REGION: RegionBody = RegionBody {
    edges: 4,
    linked_edges: 4,
    coedge_links: false,
    face_attribute: false,
    wire_edge: false,
};

const SOLID: RegionBody = RegionBody {
    edges: 4,
    linked_edges: 0,
    coedge_links: true,
    face_attribute: true,
    wire_edge: false,
};

fn sketch_region_brep(bodies: &[RegionBody], key: Option<u64>) -> Brep {
    let mut asm = AsmBrep::default();
    let mut links = Vec::new();
    for (ordinal, spec) in (0_u32..).zip(bodies) {
        let entity = |slot: u32, index: u32| {
            format!(
                "f3d:brep:entity#{}",
                1000 * (ordinal + 1) + 100 * slot + index
            )
        };
        let body = entity(0, 1);
        let region = entity(1, 1);
        let shell = entity(2, 1);
        let face = entity(3, 1);
        let ring = entity(4, 1);
        asm.bodies.push(
            serde_json::from_value(serde_json::json!({"id": body, "regions": [region]})).unwrap(),
        );
        asm.regions.push(Region {
            id: RegionId::mint(region.clone()).unwrap(),
            body: BodyId::mint(body.clone()).unwrap(),
            shells: vec![cadmpeg_ir::ids::ShellId::mint(shell.clone()).unwrap()],
        });
        asm.shells.push(
            cadmpeg_ir::topology::Shell::new(
                cadmpeg_ir::ids::ShellId::mint(shell.clone()).unwrap(),
                RegionId::mint(region).unwrap(),
                vec![FaceId::mint(face.clone()).unwrap()],
                if spec.wire_edge {
                    vec![cadmpeg_ir::ids::EdgeId::mint(entity(5, 90)).unwrap()]
                } else {
                    Vec::new()
                },
                Vec::new(),
            )
            .unwrap(),
        );
        asm.faces.push(
            serde_json::from_value(serde_json::json!({
                "id": face, "shell": shell, "surface": entity(6, 1), "sense": "forward",
                "loops": {"classification": "unspecified", "loops": if spec.edges == 0 { vec![] } else { vec![ring.clone()] }},
            }))
            .unwrap(),
        );
        let coedges = (0..spec.edges)
            .map(|index| entity(7, index))
            .collect::<Vec<_>>();
        if spec.edges != 0 {
            asm.loops.push(
                serde_json::from_value(serde_json::json!({
                    "id": ring, "face": face, "boundary": {"kind": "ring", "coedges": coedges},
                }))
                .unwrap(),
            );
        }
        for index in 0..spec.edges {
            let edge = entity(5, index);
            asm.coedges.push(
                serde_json::from_value(serde_json::json!({
                    "id": coedges[usize::try_from(index).unwrap()], "owner_loop": ring, "edge": edge,
                    "radial_next": coedges[usize::try_from(index).unwrap()], "sense": "forward",
                }))
                .unwrap(),
            );
            let link_target = if index < spec.linked_edges {
                Some(AttributeTarget::Edge(
                    cadmpeg_ir::ids::EdgeId::mint(edge).unwrap(),
                ))
            } else if spec.coedge_links {
                Some(AttributeTarget::Coedge(
                    cadmpeg_ir::ids::CoedgeId::mint(
                        coedges[usize::try_from(index).unwrap()].clone(),
                    )
                    .unwrap(),
                ))
            } else {
                None
            };
            if let Some(target) = link_target {
                links.push(SketchCurveLink {
                    id: format!("link-{ordinal}-{index}"),
                    target,
                    sketch_curve_id: 1,
                    ref_b: 0,
                    sense: None,
                    role: 0,
                    closure: 0,
                });
            }
        }
        if spec.face_attribute {
            asm.attributes.push(
                serde_json::from_value(serde_json::json!({
                    "id": entity(8, 1), "target": {"kind": "face", "id": face},
                    "name": "generic_tag_attrib_def", "values": [],
                }))
                .unwrap(),
            );
        }
        asm.body_native_keys.push(BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            body: BodyId::mint(body).unwrap(),
            record_index: ordinal + 1,
            body_ordinal: ordinal,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: key,
        });
    }
    Brep {
        asm,
        sketch_curve_links: links,
        ..Brep::default()
    }
}

#[test]
fn sketch_regions_leave_a_keyless_blob_and_selectors_count_the_remaining_bodies() {
    let mut brep = sketch_region_brep(&[SKETCH_REGION, SOLID, SKETCH_REGION, SOLID], None);
    assert_eq!(
        with_context(|ctx| brep.omit_sketch_region_bodies(ctx, 2)).unwrap(),
        2
    );
    let ordinals = brep
        .asm
        .body_native_keys
        .iter()
        .map(|key| (key.record_index, key.body_ordinal))
        .collect::<Vec<_>>();
    assert_eq!(ordinals, [(2, 0), (4, 1)]);
    assert_eq!(brep.asm.bodies.len(), 2);
    assert_eq!(brep.asm.faces.len(), 2);
    let selected = brep.body_selectors_for(&HashSet::from([0])).unwrap();
    assert_eq!(
        selected.keys().map(BodyId::as_str).collect::<Vec<_>>(),
        ["f3d:brep:entity#2001"]
    );
    assert!(brep
        .sketch_curve_links
        .iter()
        .all(|link| matches!(link.target, AttributeTarget::Coedge(_))));
}

#[test]
fn only_a_keyless_body_with_fully_linked_edges_and_unattributed_faces_is_a_sketch_region() {
    let near_misses = [
        // A planar surface made from a Sketch: linked edges and a face attribute.
        RegionBody {
            face_attribute: true,
            ..SKETCH_REGION
        },
        // One edge stores no link.
        RegionBody {
            linked_edges: 3,
            ..SKETCH_REGION
        },
        // Links on coedges only, the form a solid made from a Sketch stores.
        RegionBody {
            linked_edges: 0,
            coedge_links: true,
            ..SKETCH_REGION
        },
        // No edges to prove the form.
        RegionBody {
            edges: 0,
            linked_edges: 0,
            ..SKETCH_REGION
        },
        // A wire member is outside the region form.
        RegionBody {
            wire_edge: true,
            ..SKETCH_REGION
        },
    ];
    for (case, spec) in near_misses.into_iter().enumerate() {
        let mut brep = sketch_region_brep(&[spec, SOLID], None);
        assert_eq!(
            with_context(|ctx| brep.omit_sketch_region_bodies(ctx, 1)).unwrap(),
            0,
            "{case}"
        );
        assert_eq!(brep.asm.bodies.len(), 2, "{case}");
        assert_eq!(brep.asm.body_native_keys[1].body_ordinal, 1, "{case}");
    }
    // A stored body key selects by key, so the form has no effect.
    let mut keyed = sketch_region_brep(&[SKETCH_REGION, SOLID], Some(7));
    assert_eq!(
        with_context(|ctx| keyed.omit_sketch_region_bodies(ctx, 1)).unwrap(),
        0
    );
    assert_eq!(keyed.asm.bodies.len(), 2);
    // The body map must hold one selector per remaining body.
    for selector_count in [0, 2] {
        let mut unmatched = sketch_region_brep(&[SKETCH_REGION, SOLID], None);
        assert_eq!(
            with_context(|ctx| unmatched.omit_sketch_region_bodies(ctx, selector_count)).unwrap(),
            0,
            "{selector_count}"
        );
        assert_eq!(unmatched.asm.bodies.len(), 2, "{selector_count}");
    }
}

#[test]
fn a_blob_of_only_sketch_regions_keeps_no_model_body() {
    let mut brep = sketch_region_brep(&[SKETCH_REGION], None);
    assert_eq!(
        with_context(|ctx| brep.omit_sketch_region_bodies(ctx, 0)).unwrap(),
        1
    );
    assert!(brep.asm.bodies.is_empty());
    assert!(brep.asm.body_native_keys.is_empty());
}
