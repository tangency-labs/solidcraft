// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Write};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;
use cadmpeg_ir::report::Severity;
use zip::CompressionMethod;

use crate::loss::F3dLossCode;
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::{f3d_native, f3d_native_mut, update_f3d_native};
use crate::test_support::smbh_blocks_test::{
    generated_curve_block, generated_pcurve_block, generated_surface_block,
};
use crate::test_support::smbh_curves_test::{
    push_solved_cache_first_head, stamped_law_curve_subtype,
    synthetic_geometry_with_cache_first_curve_smbh, synthetic_geometry_with_law_curve_smbh,
    synthetic_geometry_with_stamped_law_curve_smbh,
};
use crate::test_support::smbh_geometry_test::{
    append_generated_record_tail, replace_generated_record_head, synthetic_geometry_smbh,
};
use crate::test_support::smbh_header_test::with_save_format;
use crate::test_support::smbh_revision_test::{
    assert_parameterized_tail, decoded_revision_loft_member,
    push_parameterized_revision_surface_tail, push_revision_loft_body, push_revision_surface_tail,
    regenerated_procedural_surface_span, synthetic_revision_surface_smbh,
    synthetic_revision_surface_subtype_span,
};
use crate::test_support::tokens_test::{
    t_dbl, t_ident, t_long, t_pos, t_ref, t_str, t_subident, t_vec,
};
use crate::test_support::zip_test::{
    assert_revision_surface_round_trip, f3d_with_deflated_smbh, f3d_with_smbh,
    f3d_with_smbh_and_protein, set_zip_entry_uncompressed_size,
};
use crate::F3dCodec;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

#[test]
fn null_locus_native_retained_limit_refuses_before_owned_wire_conversion() {
    use crate::records::dimension_null_locus_wire::{Wire, OWNED_WIRE_CONVERSIONS};
    use crate::records::dimensions::DesignDimensionLocusPair;
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let pair: DesignDimensionLocusPair = serde_json::from_str(
        r#"{"id":"f3d:test:dimension-locus-pair#0","companion_record_index":1,"governing_companion_record_index":2,"byte_offset":10,"class_tag":"274","record_index":3,"frame_length":80,"first_geometry_record_index":0,"first_geometry_reference_offset":35,"first_role":0,"first_role_offset":45,"second_geometry_record_index":41,"second_geometry_reference_offset":50,"second_role":1,"second_role_offset":60,"paired_class_tag":"273","paired_byte_offset":90}"#,
    )
    .unwrap();
    let record_bytes = serde_json::to_vec(&Wire::from(&pair)).unwrap().len();
    let native = super::F3dNative {
        design_dimension_null_locus_pairs: vec![pair].try_into().unwrap(),
        ..super::F3dNative::default()
    };
    let name = "design_dimension_null_locus_pairs";
    // The stored text is the record's only retained charge.
    let retained_limit = u64::try_from(record_bytes - 1).unwrap();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = retained_limit;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    OWNED_WIRE_CONVERSIONS.with(|count| count.set(0));
    let error = native.store(&limited, &mut namespace).unwrap_err();
    assert_eq!(OWNED_WIRE_CONVERSIONS.with(std::cell::Cell::get), 0);
    assert!(namespace.arenas().values().all(Vec::is_empty));
    let cadmpeg_core::CodecError::ResourceLimit(first) = cadmpeg_core::CodecError::from(error)
    else {
        panic!("native storage refusal");
    };
    assert_eq!(first.dimension, ResourceDimension::RetainedBytes);
    assert_eq!(first.limit, retained_limit);
    assert_eq!(first.operation, "serialize native record");
    assert_eq!(limited.resource_refusal(), Some(first));
    namespace.arenas_mut().clear();
    native
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    assert_eq!(namespace.arenas()[name].len(), 1);
}

#[test]
fn native_load_refuses_orphan_history_row_with_child_and_parent() {
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    crate::native::F3dNative::default()
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    let board = crate::history_records::AsmBulletinBoard {
        id: "f3d:native:bulletin#1".into(),
        parent: "f3d:native:missing-state#1".into(),
        byte_offset: 0,
        owner_ref: 0,
        number: 0,
        changes: Vec::new(),
    };
    namespace.arenas_mut().insert(
        "asm_bulletin_boards".into(),
        cadmpeg_ir::native::arena_from(
            &cadmpeg_test_support::service_decode_context(),
            [Ok::<_, cadmpeg_ir::NativeConvertError>(board)],
        )
        .unwrap(),
    );
    let error = crate::native::F3dNative::load(&namespace).unwrap_err();
    let message = error.to_string();
    assert!(message.contains("f3d:native:bulletin#1"));
    assert!(message.contains("f3d:native:missing-state#1"));
}

#[test]
fn native_arenas_have_pinned_shape_and_typed_round_trip() {
    let catalogue_names = crate::native::F3D_FAMILIES
        .iter()
        .map(|row| row.arena)
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(crate::native::F3D_FAMILIES.len(), 85);
    assert!(catalogue_names.contains("asm_face_boundaries"));
    assert!(catalogue_names.contains("design_intent_failures"));
    assert!(catalogue_names.contains("design_body_reference_tails"));
    assert!(catalogue_names.contains("design_placement_constructions"));
    assert!(catalogue_names.contains("design_sheet_metal_settings"));
    assert_eq!(
        catalogue_names,
        crate::native::F3D_ARENA_NAMES
            .iter()
            .copied()
            .collect::<std::collections::BTreeSet<_>>()
    );
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let original = decoded.ir().native.namespace("f3d").unwrap();
    let typed = crate::native::F3dNative::load(original).unwrap();
    let mut round_trip = cadmpeg_ir::NativeNamespace::default();
    typed
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut round_trip,
        )
        .unwrap();
    assert_eq!(typed, crate::native::F3dNative::load(&round_trip).unwrap());
    for name in crate::native::F3D_ARENA_NAMES {
        assert_eq!(
            round_trip.arenas().get(*name),
            original.arenas().get(*name),
            "native arena {name} did not survive a typed round trip"
        );
    }
    assert_eq!(
        round_trip
            .arenas()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::F3D_ARENA_NAMES
    );
    for records in round_trip.arenas().values() {
        for record in records {
            let json = serde_json::to_value(record).unwrap();
            assert_eq!(json["id"], record.id());
            assert!(json.as_object().unwrap().len() > 1);
        }
    }
}

/// `store` emits one arena for every pinned name, empty families included.
/// This is the registry-to-emitter agreement: a family added to
/// `F3D_FAMILIES` without its name in `F3D_ARENA_NAMES`, or the reverse,
/// fails here.
#[test]
fn store_emits_every_pinned_arena_name() {
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    crate::native::F3dNative::default()
        .store(
            &cadmpeg_test_support::service_decode_context(),
            &mut namespace,
        )
        .unwrap();
    assert_eq!(
        namespace
            .arenas()
            .keys()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        crate::native::F3D_ARENA_NAMES
    );
}

#[test]
fn diff_reports_design_material_assignment_changes() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&synthetic_geometry_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let mut edited = decoded.ir().clone();
    let assignment = &mut edited
        .native
        .namespace_mut("f3d")
        .arenas_mut()
        .get_mut("design_material_assignments")
        .unwrap()[0];
    let mut assignment_fields = assignment.fields().unwrap().clone();
    assignment_fields.insert("entity_suffix".into(), serde_json::json!(123_456));
    *assignment = cadmpeg_ir::NativeRecord::new(
        cadmpeg_ir::ids::Identity::new(assignment.id()).expect("valid identity"),
        assignment_fields,
    )
    .expect("valid native identity");
    let report = cadmpeg_ir::diff(decoded.ir(), &edited);
    let arena = report
        .per_arena
        .iter()
        .find(|arena| arena.kind == "native.f3d.design_material_assignments")
        .unwrap();
    assert_eq!(arena.modified.len(), 1);
}

#[test]
fn decode_transfers_generated_tolerant_coedge_parameters_and_topology() {
    let mut smbh = synthetic_geometry_smbh();
    let mut parameter_tail = Vec::new();
    t_dbl(&mut parameter_tail, 0.25);
    t_dbl(&mut parameter_tail, 0.75);
    t_ref(&mut parameter_tail, -1);
    t_long(&mut parameter_tail, 0);
    t_long(&mut parameter_tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &parameter_tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("generated tolerant coedges must decode");
    let mut decoded = EditableDecodeResult::from(decoded);

    assert_eq!(decoded.ir().model.coedges.len(), 3);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert_eq!(decoded.ir().model.shells[0].faces().len(), 1);
    assert_eq!(
        f3d_native(decoded.ir())
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.parameter_range.get())
            .collect::<Vec<_>>(),
        vec![[0.25, 0.75]; 3]
    );
    assert!(f3d_native(decoded.ir())
        .tolerant_coedge_parameters
        .iter()
        .all(|parameters| matches!(
            parameters.extension,
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Empty { target: None }
        )));

    decoded.ir_mut().model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    update_f3d_native(&mut decoded.ir_mut(), |native| {
        native.tolerant_coedge_parameters[0].parameter_range =
            cadmpeg_ir::units::FiniteVector::new([-1.5, 2.25]).expect("finite interval");
    });
    let mut edited = Vec::new();
    crate::test_support::plan_inherited_write(decoded.ir(), decoded.source_fidelity(), &mut edited)
        .expect("tolerant coedge sense edit");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("edited tolerant coedge round trip");
    assert_eq!(
        round_trip.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(round_trip.ir()).tolerant_coedge_parameters[0]
            .parameter_range
            .get(),
        [-1.5, 2.25]
    );
}

#[test]
fn decode_selects_tolerant_coedge_extension_from_save_format() {
    for (release, fixed_tail, expected) in [
        (
            23000u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, -1);
                t_long(&mut bytes, 1);
                bytes.extend_from_slice(&[0x0a, 0x0f]);
                t_long(&mut bytes, 22800);
                bytes.extend_from_slice(&[0x10, 0x0a]);
                t_dbl(&mut bytes, -2.0);
                bytes.push(0x0a);
                t_dbl(&mut bytes, 3.0);
                t_long(&mut bytes, 0);
                bytes
            },
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some(
                    cadmpeg_ir::units::FiniteVector::new([-2.0, 3.0]).expect("finite interval"),
                ),
            },
        ),
        (
            21900u32,
            {
                let mut bytes = Vec::new();
                t_ref(&mut bytes, 17);
                bytes
            },
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::Reference { target: Some(17) },
        ),
        (
            21400u32,
            Vec::new(),
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::None {},
        ),
    ] {
        let mut smbh = synthetic_geometry_smbh();
        smbh[15..19].copy_from_slice(&release.to_le_bytes());
        let mut tail = Vec::new();
        t_dbl(&mut tail, -0.5);
        t_dbl(&mut tail, 1.5);
        tail.extend_from_slice(&fixed_tail);
        append_generated_record_tail(&mut smbh, "coedge", &tail);
        replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("release-selected tolerant coedges must decode");
        assert_eq!(
            f3d_native(decoded.ir())
                .tolerant_coedge_parameters
                .iter()
                .map(|parameters| parameters.extension.clone())
                .collect::<Vec<_>>(),
            vec![expected; 3]
        );
    }
}

/// A tolerant coedge whose payload carries identifier tokens outside the
/// embedded scope: the freestanding embedded-curve type name before the sense
/// flag and trailing `null_curve` placeholders after the extension fields.
/// Identifiers are not fields, so the extension decodes exactly as it does
/// without them and the serialized token count stays defined over the value
/// tokens.
#[test]
fn tolerant_coedge_extension_ignores_payload_identifiers() {
    let mut smbh = synthetic_geometry_smbh();
    smbh[15..19].copy_from_slice(&23000u32.to_le_bytes());
    let mut tail = Vec::new();
    t_dbl(&mut tail, -0.5);
    t_dbl(&mut tail, 1.5);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    t_ident(&mut tail, "intcurve");
    tail.extend_from_slice(&[0x0a, 0x0f]);
    t_ident(&mut tail, "par_int_cur");
    t_long(&mut tail, 22800);
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    t_ident(&mut tail, "null_curve");
    t_ident(&mut tail, "null_curve");
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("ident-bearing tolerant coedges must decode");
    assert_eq!(
        f3d_native(decoded.ir())
            .tolerant_coedge_parameters
            .iter()
            .map(|parameters| parameters.extension.clone())
            .collect::<Vec<_>>(),
        vec![
            cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: true,
                payload_token_count: 1,
                parameter_range: Some(
                    cadmpeg_ir::units::FiniteVector::new([-2.0, 3.0]).expect("finite interval"),
                ),
            };
            3
        ]
    );
}

#[test]
fn decode_transfers_embedded_tolerant_coedge_use_curves() {
    let mut smbh = synthetic_geometry_smbh();
    let mut tail = Vec::new();
    t_dbl(&mut tail, 0.0);
    t_dbl(&mut tail, 1.0);
    t_ref(&mut tail, -1);
    t_long(&mut tail, 1);
    tail.extend_from_slice(&[0x0a, 0x0f]);
    tail.extend_from_slice(&generated_curve_block());
    tail.extend_from_slice(&[0x10, 0x0a]);
    t_dbl(&mut tail, -2.0);
    tail.push(0x0a);
    t_dbl(&mut tail, 3.0);
    t_long(&mut tail, 0);
    append_generated_record_tail(&mut smbh, "coedge", &tail);
    replace_generated_record_head(&mut smbh, "coedge", "tcoedge");

    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("embedded tolerant-coedge curves must decode"),
    );
    assert_eq!(
        decoded
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        3
    );
    assert!(decoded.ir().model.coedges.iter().all(|coedge| {
        coedge.use_curve.as_ref().is_some_and(|use_| {
            use_.parameter_range.endpoints() == [-2.0, 3.0]
                && decoded.ir().model.curves.iter().any(|curve| {
                    curve.id == use_.curve
                        && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(ref nurbs)) if nurbs.degree() == 2)
                })
        })
    }));
    let first_use_curve = decoded.ir().model.coedges[0]
        .use_curve
        .as_ref()
        .and_then(|use_| {
            decoded
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == use_.curve)
        })
        .expect("first embedded use curve");
    let Some(SolvedCurveGeometry::Nurbs(first_use_curve)) = first_use_curve.geometry.solved()
    else {
        panic!("embedded use curve must be NURBS")
    };
    assert_eq!(
        first_use_curve.control_points()[0],
        cadmpeg_ir::math::Point3::new(20.0, 0.0, 0.0)
    );
    assert_eq!(
        first_use_curve.control_points()[2],
        cadmpeg_ir::math::Point3::new(0.0, 0.0, 0.0)
    );
    assert_eq!(
        first_use_curve.knots().as_slice(),
        [-1.0, -1.0, -1.0, -0.0, -0.0, -0.0]
    );

    let mut edited = decoded.ir().clone();
    let use_curve = edited.model.coedges[0]
        .use_curve
        .clone()
        .expect("first coedge use curve")
        .curve;
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == use_curve)
        .expect("embedded use-curve carrier");
    let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) =
        &mut curve.geometry
    else {
        panic!("embedded use curve must be NURBS")
    };
    nurbs
        .try_map_control_points(|index, point| {
            let mut point = point.get();
            if index == 0 {
                point.x += 1.0;
            }
            cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        })
        .expect("pole edit admission");
    let expected = nurbs.clone();
    let mut preserved = Vec::new();
    crate::test_support::plan_inherited_write(&edited, decoded.source_fidelity(), &mut preserved)
        .expect("embedded use-curve edit");
    let preserved = F3dCodec
        .decode(&mut Cursor::new(preserved), &DecodeOptions::default())
        .expect("embedded use-curve edit round trip");
    assert!(preserved.ir().model.curves.iter().any(|curve| {
        curve.id == use_curve
            && matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(ref curve)) if *curve == expected)
    }));

    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let generated_curve_id = cadmpeg_ir::ids::CurveId::mint("generated:test:tolerant-use-curve#0")
        .expect("identity grammar");
    source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
        parameter_range: None,
        id: generated_curve_id.clone(),
        geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            expected.clone(),
        )),
        source_object: None,
    });
    let tolerant_coedge = source_less.model.coedges[0].id.clone();
    source_less.model.coedges[0].use_curve = Some(cadmpeg_ir::topology::CoedgeUseCurve {
        curve: generated_curve_id,
        parameter_range: cadmpeg_ir::topology::ParameterInterval::new([-2.0, 3.0]).unwrap(),
    });
    f3d_native_mut(&mut source_less).tolerant_coedge_parameters =
        vec![cadmpeg_asm::brep::records::TolerantCoedgeParameters {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                cadmpeg_asm::asm_format!("generated"),
            ),
            coedge: tolerant_coedge,
            record_index: 0,
            parameter_range: cadmpeg_ir::units::FiniteVector::new([0.0, 1.0])
                .expect("finite interval"),
            extension: cadmpeg_asm::brep::records::TolerantCoedgeExtension::EmbeddedCurve {
                target: None,
                curve_reversed: false,
                payload_token_count: 0,
                parameter_range: Some(
                    cadmpeg_ir::units::FiniteVector::new([-2.0, 3.0]).expect("finite interval"),
                ),
            },
        }];
    let mut generated = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut generated))
        .expect("source-less embedded use curves");
    let generated = F3dCodec
        .decode(&mut Cursor::new(generated), &DecodeOptions::default())
        .expect("source-less embedded use-curve round trip");
    assert_eq!(
        generated
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| coedge.use_curve.is_some())
            .count(),
        1
    );
    assert!(generated.ir().model.curves.iter().any(|curve| {
        matches!(curve.geometry, cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(ref curve)) if *curve == expected)
    }));
}

#[test]
fn decode_frames_history_less_stream_whose_final_record_ends_at_eof() {
    // A history-less `.smb` stream has no `delta_state` boundary and its final
    // `End-of-ASM-data` record ends at EOF without the `0x11` terminator.
    let mut smbh = synthetic_geometry_smbh();
    let marker = smbh
        .windows(b"\x0d\x0bdelta_state".len())
        .position(|window| window == b"\x0d\x0bdelta_state")
        .expect("generated history boundary");
    smbh.truncate(marker);
    for name in ["End", "of", "ASM"] {
        t_subident(&mut smbh, name);
    }
    t_ident(&mut smbh, "data"); // no trailing 0x11
    assert!(cadmpeg_asm::asm_header::solved_record_limit(&smbh).is_none());

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh_and_protein(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("history-less stream must decode");
    assert_eq!(decoded.ir().model.faces.len(), 1);
    assert_eq!(decoded.ir().model.edges.len(), 3);
    assert_eq!(decoded.ir().model.vertices.len(), 3);
}

#[test]
fn stamped_law_intcurve_round_trips_byte_exactly() {
    use cadmpeg_ir::geometry::{
        LawExpression, LawFormula, ProceduralCurveDefinition, SolvedCurveGeometry,
    };

    // Formula names exceed 255 bytes to exercise the u16 (`0x08`) length prefix
    // the serializer selects for long law text.
    let primary_name = format!("TRANS({},TRANS1)", "VEC(X,X2,X3)*COS(X)+".repeat(20));
    let raw_name = "VEC(X,X2,X3)*COS(X)+".repeat(20);
    assert!(primary_name.len() > 255 && raw_name.len() > 255);
    let subtype = stamped_law_curve_subtype(&primary_name, &raw_name);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_stamped_law_curve_smbh(&subtype),
            )),
            &DecodeOptions::default(),
        )
        .expect("stamped law intcurve decode");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition(), ProceduralCurveDefinition::Law { .. }))
        .expect("stamped law construction");
    let ProceduralCurveDefinition::Law {
        version,
        primary,
        additional,
        ..
    } = procedural.definition()
    else {
        unreachable!()
    };
    let version = version.as_ref().expect("version stamp");
    assert_eq!(version.stamp(), 20900);
    assert_eq!(version.post_enum(), 0);
    assert_eq!(version.parameter_range(), [None, None]);
    assert!(
        matches!(primary.formula(), LawFormula::Named { name, .. } if name.as_str() == primary_name)
    );
    assert!(matches!(
        primary.formula().variables()[0],
        LawExpression::TransformVec { .. }
    ));
    assert_eq!(additional.len(), 4);
    assert!(matches!(additional[0].formula(), LawFormula::Null {}));
    assert!(matches!(additional[1].formula(), LawFormula::Null {}));
    assert!(
        matches!(additional[2].formula(), LawFormula::Named { name, .. } if name.as_str() == raw_name)
    );
    assert!(
        matches!(additional[3].formula(), LawFormula::Named { name, .. }
        if name.as_str() == "TRANS(VEC(X,X2,X3),TRANS1)")
    );
    assert!(matches!(
        additional[3].formula().variables()[0],
        LawExpression::TransformVec { .. }
    ));

    // Byte-exact re-emission of the subtype span. The solved cache uses
    // integer-valued control points so the cm->mm scaling round-trip is exact.
    let solved = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| decoded.ir().model.procedural_curve_owner(&procedural.id) == Some(&curve.id))
        .and_then(|curve| match curve.geometry.solved_cache() {
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => Some(nurbs.clone()),
            _ => None,
        })
        .expect("solved cache");
    let owner = decoded
        .ir()
        .model
        .procedural_curve_owner(&procedural.id)
        .expect("law curve owner");
    let mut regenerated = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_curve(
        &mut regenerated,
        decoded.ir(),
        owner,
        &solved,
    )
    .expect("regenerate stamped law curve");
    let inner = regenerated.iter().position(|&b| b == 0x0f).unwrap();
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(
        &regenerated,
        inner,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap()
    .bytes();
    assert_eq!(span, subtype.as_slice());
}

#[test]
fn legacy_law_intcurve_round_trips_byte_exactly() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SolvedCurveGeometry};

    let smbh = synthetic_geometry_with_law_curve_smbh();
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("legacy law intcurve decode");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition(), ProceduralCurveDefinition::Law { .. }))
        .expect("legacy law construction");
    let ProceduralCurveDefinition::Law { version, .. } = procedural.definition() else {
        unreachable!()
    };
    assert!(version.is_none());

    let original = {
        let marker = smbh
            .windows(b"law_int_cur".len())
            .position(|window| window == b"law_int_cur")
            .unwrap()
            - 3;
        cadmpeg_asm::nurbs::subtypes::subtype_span(
            &smbh,
            marker,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .unwrap()
        .bytes()
        .to_vec()
    };
    let solved = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| decoded.ir().model.procedural_curve_owner(&procedural.id) == Some(&curve.id))
        .and_then(|curve| match curve.geometry.solved_cache() {
            Some(SolvedCurveGeometry::Nurbs(nurbs)) => Some(nurbs.clone()),
            _ => None,
        })
        .expect("solved cache");
    let owner = decoded
        .ir()
        .model
        .procedural_curve_owner(&procedural.id)
        .expect("law curve owner");
    let mut regenerated = Vec::new();
    crate::writer::generate::native_geometry::native_procedural_curve(
        &mut regenerated,
        decoded.ir(),
        owner,
        &solved,
    )
    .expect("regenerate legacy law curve");
    let inner = regenerated.iter().position(|&b| b == 0x0f).unwrap();
    let span = cadmpeg_asm::nurbs::subtypes::subtype_span(
        &regenerated,
        inner,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap()
    .bytes();
    assert_eq!(span, original.as_slice());
}

#[test]
fn generated_cache_first_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "spring_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        curve.push(0x15);
                        curve.extend_from_slice(&4i64.to_le_bytes());
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first spring decode");
    let ProceduralCurveDefinition::Spring(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected spring construction")
    };
    let layout = &definition_payload.layout().to_raw();
    let direction = definition_payload.direction();

    let cadmpeg_ir::geometry::SpringLayout::CacheFirst { context, form } = layout else {
        panic!("expected cache-first spring layout")
    };
    assert_eq!(form.revision.get(), 23100);
    assert_eq!(form.solved_range, [Some(-1.0), Some(2.0)]);
    assert_eq!(form.extension, 7);
    assert_eq!(*direction, 4);
    assert_eq!(context.parameter_range().endpoints(), [-1.0, 2.0]);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first spring round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        source_less.model.procedural_curves[0].definition()
    );
}

#[test]
fn generated_cache_first_parametric_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamily};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "par_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        curve.push(0x0a);
                        curve.push(0x0b);
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first parametric decode");
    let ProceduralCurveDefinition::SurfaceCurve {
        family:
            SurfaceCurveFamily::Parametric {
                context,
                tail: Some(tail),
            },
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected surface-curve construction")
    };
    assert_eq!(tail.form.revision().get(), 23100);
    assert_eq!(tail.form.extension(), 7);
    assert!(tail.flags.flag);
    assert_eq!(tail.flags.second_flag, Some(false));
    assert_eq!(
        *tail.form.solved_range(),
        [
            cadmpeg_ir::scalar::FiniteReal::new(-1.0),
            cadmpeg_ir::scalar::FiniteReal::new(2.0)
        ]
    );
    assert_eq!(context.parameter_range().endpoints(), [-1.0, 2.0]);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first parametric encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first parametric round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        source_less.model.procedural_curves[0].definition()
    );
}

#[test]
fn generated_cache_first_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cache_first_curve_smbh(
                    "off_surf_int_cur",
                    push_solved_cache_first_head,
                    |curve| {
                        for value in [-1.0, 2.0, -3.0, 4.0] {
                            curve.push(0x0a);
                            t_dbl(curve, value);
                        }
                        curve.extend_from_slice(&generated_curve_block());
                        curve.push(0x0b);
                        curve.push(0x0b);
                        curve.push(0x0a);
                        t_dbl(curve, -0.5);
                        curve.push(0x0a);
                        t_dbl(curve, 1.5);
                        t_dbl(curve, -0.25);
                        t_dbl(curve, 0.75);
                        t_dbl(curve, 1.25);
                    },
                ),
            )),
            &DecodeOptions::default(),
        )
        .expect("cache-first surface-offset decode");
    let ProceduralCurveDefinition::SurfaceOffset(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected surface-offset construction")
    };
    let cache_first = definition_payload.cache_first();
    let base_u_range = definition_payload.base_u_range();
    let base_v_range = definition_payload.base_v_range();
    let base_endpoints = definition_payload.base_endpoints();
    let base_range = definition_payload.base_range();
    let distance = definition_payload.distance();
    let shift = definition_payload.shift();
    let scale = definition_payload.scale();
    let form = cache_first
        .as_ref()
        .expect("cache-first surface-offset form");
    assert_eq!(form.revision.get(), 23100);
    assert_eq!(form.extension, 7);
    assert_eq!(base_u_range.endpoints(), [-1.0, 2.0]);
    assert_eq!(base_v_range.endpoints(), [-3.0, 4.0]);
    assert_eq!(base_endpoints, [None, None]);
    assert_eq!(base_range.endpoints(), [-0.5, 1.5]);
    assert_eq!(distance.get(), -2.5);
    assert_eq!(shift.get(), 0.75);
    assert_eq!(scale.get(), 1.25);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cache-first surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cache-first surface-offset round trip");
    let expected = source_less.model.procedural_curves[0].definition().clone();
    let mut actual = round_trip.ir().model.procedural_curves[0]
        .definition()
        .clone();
    let (
        ProceduralCurveDefinition::SurfaceOffset(expected_payload),
        ProceduralCurveDefinition::SurfaceOffset(actual_payload),
    ) = (&expected, &mut actual)
    else {
        panic!("expected surface-offset round trip")
    };
    let round_trip_base = actual_payload.base().clone();
    *actual_payload =
        cadmpeg_ir::geometry::curve_payloads::SurfaceOffsetCurveConstruction::try_new(
            actual_payload.context().clone(),
            *actual_payload.discontinuity_flag(),
            [
                actual_payload.base_u_range().endpoints(),
                actual_payload.base_v_range().endpoints(),
            ],
            (
                expected_payload.base().clone(),
                actual_payload.base_range().endpoints(),
                actual_payload
                    .base_endpoints()
                    .map(|endpoint| endpoint.map(cadmpeg_ir::scalar::FiniteReal::get)),
            ),
            cadmpeg_ir::geometry::CacheContract::from_form(
                actual_payload
                    .cache_first()
                    .map(cadmpeg_ir::geometry::CacheFirstCurveForm::to_raw),
            ),
            actual_payload.distance().get(),
            [actual_payload.shift().get(), actual_payload.scale().get()],
        )
        .unwrap();
    assert_eq!(actual, expected);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == round_trip_base));
}

#[test]
fn generated_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh.clone(), "offset");

    // The revision-gated layout shares byte positions with the pre-revision
    // U/V sense enums but no grammar, so its four-boolean carrier run travels
    // in the revision form and leaves the enum slots empty.
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("revision offset decode");
    let ProceduralSurfaceDefinition::Offset(definition_payload) =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected offset surface construction")
    };
    let u_sense = definition_payload.u_sense();
    let v_sense = definition_payload.v_sense();
    let extension = definition_payload.extension();
    assert_eq!((*u_sense, *v_sense), (None, None));
    let cadmpeg_ir::geometry::OffsetExtension::Revision { form } = extension else {
        panic!("expected revision offset extension")
    };
    assert_eq!(form.flags, [false, true, false, false]);
}

#[test]
fn generated_parameterized_revision_offset_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("off_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0b);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.push(0x0b);
        t_dbl(surface, 0.3);
        for flag in [false, true, false, false] {
            surface.push(if flag { 0x0a } else { 0x0b });
        }
        push_parameterized_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh.clone(), "offset");

    let subtype = synthetic_revision_surface_subtype_span(&smbh);
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision offset decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    // Cache form 2 stores no fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Offset(definition_payload) =
        procedural.definition()
    else {
        panic!("expected offset surface construction")
    };
    let extension = &definition_payload.extension().to_raw();
    let cadmpeg_ir::geometry::OffsetExtension::Revision { form } = extension else {
        panic!("expected revision form")
    };
    assert!(form.cache.parameterization().is_some());
    let parameterization = form
        .cache
        .parameterization()
        .expect("tail parameterization");
    assert_eq!(parameterization.u_interval, [Some(0.25), None]);
    assert_eq!(parameterization.v_interval, [Some(-1.5), Some(3.5)]);
    assert_eq!(
        (parameterization.u_closure, parameterization.v_closure),
        (1, 0)
    );
    assert_eq!(
        (
            parameterization.u_singularity,
            parameterization.v_singularity
        ),
        (2, 3)
    );
    assert_eq!(regenerated_procedural_surface_span(result.ir()), subtype);
}

#[test]
fn generated_revision_orthogonal_taper_round_trips() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        surface.push(0x0a);
    });
    assert_revision_surface_round_trip(smbh, "taper");
}

#[test]
fn generated_revision_orthogonal_taper_decodes_sense_true() {
    let smbh = synthetic_revision_surface_smbh("ortho_spl_sur", |surface| {
        t_ident(surface, "spline");
        surface.extend_from_slice(&generated_surface_block());
        surface.extend_from_slice(&[0x0b; 4]);
        surface.extend_from_slice(&generated_curve_block());
        surface.push(0x0a);
        t_dbl(surface, -1.0);
        surface.push(0x0a);
        t_dbl(surface, 2.0);
        surface.extend_from_slice(&generated_pcurve_block());
        t_dbl(surface, 0.5);
        push_revision_surface_tail(surface);
        // Trailing orthogonal-sense logical set true.
        surface.push(0x0a);
    });
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("ortho revision decode");
    let definition = &result
        .ir()
        .model
        .procedural_surfaces
        .first()
        .expect("ortho construction")
        .definition();
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Taper(definition_payload) = definition
    else {
        panic!("expected taper definition, got {definition:?}");
    };
    let taper = definition_payload.taper();
    assert_eq!(
        *taper,
        cadmpeg_ir::geometry::TaperSurfaceKind::Orthogonal { sense: true }
    );
}

#[test]
fn generated_revision_sweep_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("sweep_sur", |surface| {
        surface.push(0x0b);
        t_long(surface, -1);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        t_pos(surface, [1.0, 2.0, 3.0]);
        t_vec(surface, [0.0, 0.0, 1.0]);
        t_vec(surface, [1.0, 0.0, 0.0]);
        t_vec(surface, [0.0, 1.0, 0.0]);
        t_long(surface, 1);
        surface.push(0x0b);
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        surface.push(0x0a);
        t_dbl(surface, 0.0);
        surface.push(0x0a);
        t_dbl(surface, 0.5);
        t_dbl(surface, 0.0);
        surface.push(0x0b);
        t_str(surface, "MTRAIL(EDGE1)");
        t_long(surface, 1);
        t_str(surface, "EDGE");
        surface.extend_from_slice(&generated_curve_block());
        surface.extend_from_slice(&[0x0b, 0x0b]);
        t_dbl(surface, 0.0);
        t_dbl(surface, 1.0);
        surface.push(0x0b);
        push_revision_surface_tail(surface);
    });
    assert_revision_surface_round_trip(smbh, "sweep");
}

#[test]
fn generated_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(surface, 1, Some(-1), push_revision_surface_tail);
    });
    assert_revision_surface_round_trip(smbh, "loft");
}

#[test]
fn generated_parameterized_revision_loft_surface_round_trips() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(
            surface,
            1,
            Some(-1),
            push_parameterized_revision_surface_tail,
        );
    });
    assert_revision_surface_round_trip(smbh.clone(), "loft");

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("parameterized revision loft decode");
    let procedural = &result.ir().model.procedural_surfaces[0];
    // Cache form 2 stores no fit tolerance.
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Loft(definition_payload) =
        procedural.definition()
    else {
        panic!("expected a loft construction")
    };
    let revision_form = definition_payload
        .revision_form()
        .map(cadmpeg_ir::geometry::LoftRevisionForm::to_raw);

    let form = revision_form.as_ref().expect("revision form");
    assert_parameterized_tail(&form.cache);
}

#[test]
fn revision_loft_member_omits_the_asm_integer_in_an_early_save_format_stream() {
    // Save format 22600: the constraint subdata follows the first flag with no
    // ASM integer between them.
    let smbh = with_save_format(
        synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
            push_revision_loft_body(surface, 1, None, push_revision_surface_tail);
        }),
        22600,
    );
    let subtype = synthetic_revision_surface_subtype_span(&smbh);

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("early-era revision loft decode");
    let member = decoded_revision_loft_member(decoded.ir());
    assert!(matches!(
        &member.form,
        cadmpeg_ir::geometry::LoftMemberForm::Support {
            type_code: 1,
            first_flag: false,
            asm_extension: None,
            ..
        }
    ));
    assert_eq!(regenerated_procedural_surface_span(decoded.ir()), subtype);
}

#[test]
fn revision_loft_type_zero_member_stores_two_pcurve_slots() {
    let smbh = synthetic_revision_surface_smbh("loft_spl_sur", |surface| {
        push_revision_loft_body(surface, 0, Some(-1), push_revision_surface_tail);
    });
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("type-zero revision loft decode");
    let member = decoded_revision_loft_member(decoded.ir());
    assert!(matches!(
        &member.form,
        cadmpeg_ir::geometry::LoftMemberForm::PcurvePair {
            pcurve: Some(_),
            secondary_pcurve: None,
            asm_extension: Some(-1),
            ..
        }
    ));
    assert_revision_surface_round_trip(smbh, "loft");
}

#[test]
fn malformed_tspline_cage_degrades_to_a_loss_note() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Breps.BlobParts/Body1.smbh", stored)
        .unwrap();
    zip.write_all(&synthetic_geometry_smbh()).unwrap();
    zip.start_file(
        "FusionAssetName[Active]/TSplines.BlobParts/Cage1.tsm",
        stored,
    )
    .unwrap();
    // An edge-root index far outside the half-edge range makes the cage
    // internally inconsistent while the entry itself stays well-formed.
    zip.write_all(b"tsm 1.0\ner 999\n").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let result = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("an inconsistent cage must not fail the document decode");
    assert!(result.ir().model.subds.is_empty());
    assert!(result
        .report()
        .losses
        .iter()
        .any(|loss| loss.severity == cadmpeg_ir::report::Severity::Error
            && loss.message.contains("T-spline control cage not decoded")));
}

#[test]
fn malformed_paramesh_reports_its_entry_and_parser_failure() {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    let entry = "FusionAssetName[Active]/ParaMeshGeometry.BlobParts/broken.paramesh";
    zip.start_file(entry, stored).unwrap();
    zip.write_all(b"not a paramesh container").unwrap();
    let archive = zip.finish().unwrap().into_inner();

    let decoded = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect("independent malformed mesh entry must not abort document decode");
    assert!(decoded.report().losses.iter().any(|loss| {
        loss.code == F3dLossCode::MeshContainerUndecoded.kind()
            && loss.severity == Severity::Error
            && loss.message.contains(entry)
            && loss.message.contains("paramesh container has no magic")
    }));
}

#[test]
fn oversized_zip_entry_declaration_is_rejected_before_allocation() {
    let mut archive = f3d_with_deflated_smbh(&synthetic_geometry_smbh());
    let target = b"FusionAssetName[Active]/Breps.BlobParts/Body1.smbh";
    set_zip_entry_uncompressed_size(&mut archive, target, u32::MAX - 1);

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("oversized inflated entry must be rejected");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(_))
        ),
        "{error:?}"
    );
}

#[test]
fn write_path_protein_bounds_remain_local_constants() {
    // Decode nested Protein ZIPs charge through ArchiveSnapshot / begin_expand.
    // The write-path rewriter has no DecodeContext and keeps these local caps.
    assert_eq!(crate::container::MAX_ARCHIVE_BYTES, 256 * 1024 * 1024);
    assert_eq!(
        crate::container::MAX_INFLATED_ENTRY_BYTES,
        128 * 1024 * 1024
    );
}

#[test]
fn oversized_nested_protein_entry_is_rejected_before_allocation() {
    let target = b"AssetData/InstanceProperties.bin";
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    zip.start_file(std::str::from_utf8(target).unwrap(), stored)
        .unwrap();
    zip.write_all(b"properties").unwrap();
    let mut protein = zip.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX);

    let error = crate::materials::patch_protein_appearances(
        &protein,
        &std::collections::BTreeMap::new(),
        &mut Vec::new(),
    )
    .expect_err("oversized nested Protein entry must be rejected");
    assert!(error.to_string().contains("inflated bytes"));
}

#[test]
fn nested_protein_decode_charges_through_session_expand_ceilings() {
    use cadmpeg_core::decode::ResourceDimension;

    let target = b"AssetData/InstanceProperties.bin";
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    nested
        .start_file(std::str::from_utf8(target).unwrap(), deflated)
        .unwrap();
    nested.write_all(b"properties").unwrap();
    let mut protein = nested.finish().unwrap().into_inner();
    set_zip_entry_uncompressed_size(&mut protein, target, u32::MAX - 1);

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut outer, stored);
    outer
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
    outer.write_all(&synthetic_geometry_smbh()).unwrap();
    outer
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    outer.write_all(&protein).unwrap();
    let archive = outer.finish().unwrap().into_inner();

    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &DecodeOptions::default())
        .expect_err("nested Protein inflate must refuse session expand ceilings");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::DecompressedBytes
        ),
        "{error:?}"
    );
}

#[test]
fn nested_protein_decode_honors_operator_per_expand_ceiling() {
    use cadmpeg_core::decode::ResourceDimension;

    let target = b"AssetData/InstanceProperties.bin";
    let payload = vec![b'x'; 64 * 1024];
    let mut nested = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let deflated = crate::zip_write::file_options(CompressionMethod::Deflated);
    nested
        .start_file(std::str::from_utf8(target).unwrap(), deflated)
        .unwrap();
    nested.write_all(&payload).unwrap();
    let protein = nested.finish().unwrap().into_inner();

    let stored = crate::zip_write::file_options(CompressionMethod::Stored);
    let mut outer = zip::ZipWriter::new(Cursor::new(Vec::new()));
    write_synthetic_manifests(&mut outer, stored);
    outer
        .start_file(
            "FusionAssetName[Active]/Breps.BlobParts/BREP.synthetic.smbh",
            stored,
        )
        .unwrap();
    outer.write_all(&synthetic_geometry_smbh()).unwrap();
    outer
        .start_file(
            "FusionAssetName[Active]/ProteinAssets.BlobParts/ProteinAsset.0.protein",
            stored,
        )
        .unwrap();
    outer.write_all(&protein).unwrap();
    let archive = outer.finish().unwrap().into_inner();

    let mut options = DecodeOptions::default();
    options.policy.limits.max_decompressed_bytes_per_expand = 1024;
    let error = F3dCodec
        .decode(&mut Cursor::new(archive), &options)
        .expect_err("operator per-expand ceiling must bind nested Protein inflate");
    assert!(
        matches!(
            error,
            cadmpeg_ir::DecodeFailure::Codec(cadmpeg_core::CodecError::ResourceLimit(limit))
                if limit.dimension == ResourceDimension::DecompressedBytes
        ),
        "{error:?}"
    );
}

#[test]
fn fragment_history_join_lists_absent_owners_and_retains_local_children() {
    let owners = std::collections::HashMap::from([("parent".into(), 0)]);
    let children = vec![("local", "parent"), ("external", "absent")];
    assert!(
        super::group_by_owner(children.clone(), &owners, 1, |row| row.0, |row| row.1, None)
            .is_err()
    );
    let mut skipped = Vec::new();
    let grouped = super::group_by_owner(
        children,
        &owners,
        1,
        |row| row.0,
        |row| row.1,
        Some(&mut skipped),
    )
    .unwrap();
    assert_eq!(grouped, vec![vec![("local", "parent")]]);
    assert_eq!(skipped.len(), 1);
    assert!(skipped[0].contains("external"));
    assert!(skipped[0].contains("absent"));
}
