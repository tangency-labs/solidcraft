// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]
#![allow(
    clippy::cloned_ref_to_slice_refs,
    clippy::default_trait_access,
    clippy::if_not_else,
    clippy::needless_pass_by_value,
    clippy::range_plus_one,
    clippy::semicolon_if_nothing_returned,
    clippy::trivially_copy_pass_by_ref
)]

use cadmpeg_test_support::{edit, EditableDecodeResult};

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_asm::asm_header;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::TestEncode;
use crate::test_support::smbh_blocks_test::{generated_curve_block, generated_pcurve_block};
use crate::test_support::smbh_curves_test::{
    synthetic_geometry_with_deformable_curve_smbh, synthetic_geometry_with_helix_curve_smbh,
    synthetic_geometry_with_null_support_spring_smbh,
    synthetic_geometry_with_procedural_curve_smbh, synthetic_geometry_with_spring_smbh,
    synthetic_geometry_with_surface_offset_smbh,
};
use crate::test_support::smbh_pcurves_test::{
    assert_pcurve_close, assert_pcurve_geometry_close,
    synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh,
    synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh,
    synthetic_geometry_with_out_of_scope_pcurve_cache_smbh, synthetic_geometry_with_pcurve_smbh,
    synthetic_geometry_with_rational_pcurve_smbh,
    synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh,
    synthetic_geometry_with_ref_pcurve_smbh, synthetic_geometry_with_short_pcurve_tail_smbh,
    synthetic_geometry_with_wrapped_ref_pcurve_smbh,
    synthetic_inline_pcurve_with_referenced_support_smbh, with_inline_pcurve_non_boolean_wrapper,
    with_pcurve_discriminator, with_plane_pcurve_offset_vertices, with_ref_pcurve_companion_name,
    with_ref_pcurve_companion_reversed,
};
use crate::test_support::tokens_test::renamed_generated_subtype;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

#[test]
fn generated_surface_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_offset_smbh())),
                &DecodeOptions::default(),
            )
            .expect("surface-offset decode"),
    );
    let ProceduralCurveDefinition::SurfaceOffset(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected surface-offset construction")
    };
    let context = definition_payload.context();
    let discontinuity_flag = definition_payload.discontinuity_flag();
    let base_u_range = definition_payload.base_u_range();
    let base_v_range = definition_payload.base_v_range();
    let base = definition_payload.base();
    let base_range = definition_payload.base_range();
    let distance = definition_payload.distance();
    let shift = definition_payload.shift();
    let scale = definition_payload.scale();
    assert_eq!(base_u_range.endpoints(), [-1.0, 2.0]);
    assert_eq!(context.parameter_range().endpoints(), [0.0, 1.0]);
    assert!(*discontinuity_flag);
    assert_eq!(base_v_range.endpoints(), [-3.0, 4.0]);
    assert_eq!(base_range.endpoints(), [-0.5, 1.5]);
    assert_eq!(
        (distance.get(), shift.get(), scale.get()),
        (-2.5, 0.75, 1.25)
    );
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *base));

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::SurfaceOffset(definition_payload) = definition else {
            unreachable!()
        };
        let mut context_value = definition_payload.context().clone();
        let context = &mut context_value;
        let mut discontinuity_flag_value = *definition_payload.discontinuity_flag();
        let discontinuity_flag = &mut discontinuity_flag_value;
        let mut base_u_range_value = definition_payload.base_u_range().endpoints();
        let base_u_range = &mut base_u_range_value;
        let mut base_v_range_value = definition_payload.base_v_range().endpoints();
        let base_v_range = &mut base_v_range_value;
        let mut base_range_value = definition_payload.base_range().endpoints();
        let base_range = &mut base_range_value;
        let mut distance_value = definition_payload.distance().get();
        let distance = &mut distance_value;
        let mut shift_value = definition_payload.shift().get();
        let shift = &mut shift_value;
        let mut scale_value = definition_payload.scale().get();
        let scale = &mut scale_value;
        edit::replace(context, |previous| {
            let sides = previous.sides().clone();
            let mut range = previous.parameter_range().endpoints();
            let discontinuities =
                cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
            {
                let context_parameter_range: &mut [f64; 2] = &mut range;

                (*context_parameter_range) = [-1.5, 2.5];
                *discontinuity_flag = false;
                *base_u_range = [-2.0, 5.0];
                *base_v_range = [-6.0, 7.0];
                *base_range = [-0.75, 1.75];
                (*distance, *shift, *scale) = (3.5, -0.25, 0.8);
            };
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
        })
        .unwrap();
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::SurfaceOffsetCurveConstruction::try_new(
                context_value,
                discontinuity_flag_value,
                [base_u_range_value, base_v_range_value],
                (
                    definition_payload.base().clone(),
                    base_range_value,
                    definition_payload
                        .base_endpoints()
                        .map(|endpoint| endpoint.map(cadmpeg_ir::scalar::FiniteReal::get)),
                ),
                cadmpeg_ir::geometry::CacheContract::from_form(
                    definition_payload
                        .cache_first()
                        .map(cadmpeg_ir::geometry::CacheFirstCurveForm::to_raw),
                ),
                distance_value,
                [shift_value, scale_value],
            )
            .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("surface-offset scalar regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated surface-offset decode");
    assert!(
        match regenerated.ir().model.procedural_curves[0].definition() {
            ProceduralCurveDefinition::SurfaceOffset(matched_payload) =>
                matches!((matched_payload.context(), matched_payload.discontinuity_flag(), matched_payload.base_u_range().endpoints(), matched_payload.base_v_range().endpoints(), matched_payload.base_range().endpoints(), matched_payload.distance().get(), matched_payload.shift().get(), matched_payload.scale().get(),), (context, false, [-2.0, 5.0], [-6.0, 7.0], [-0.75, 1.75], 3.5, -0.25, 0.8,) if context.parameter_range().endpoints() == [-1.5, 2.5]),
            _ => false,
        }
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less surface-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface-offset round trip");
    let ProceduralCurveDefinition::SurfaceOffset(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip surface offset")
    };
    let discontinuity_flag = definition_payload.discontinuity_flag();
    let base_u_range = definition_payload.base_u_range();
    let base_v_range = definition_payload.base_v_range();
    let base_range = definition_payload.base_range();
    let distance = definition_payload.distance();
    let shift = definition_payload.shift();
    let scale = definition_payload.scale();
    assert_eq!(base_u_range.endpoints(), [-1.0, 2.0]);
    assert!(*discontinuity_flag);
    assert_eq!(base_v_range.endpoints(), [-3.0, 4.0]);
    assert_eq!(base_range.endpoints(), [-0.5, 1.5]);
    assert_eq!(
        (distance.get(), shift.get(), scale.get()),
        (-2.5, 0.75, 1.25)
    );
}

#[test]
fn generated_spring_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_spring_smbh())),
                &DecodeOptions::default(),
            )
            .expect("spring decode"),
    );
    let ProceduralCurveDefinition::Spring(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected spring construction")
    };
    let direction = definition_payload.direction();

    assert_eq!(*direction, -3);
    assert!(definition_payload
        .support_context()
        .sides()
        .iter()
        .all(|side| side.surface.is_some() && side.pcurve.is_some()));

    let mut edited = result.ir().clone();
    let expected_flag = edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Spring(definition_payload) = definition else {
            unreachable!()
        };
        let mut edited_layout = definition_payload.layout().to_raw();
        let mut edited_direction = *definition_payload.direction();
        let layout = &mut edited_layout;
        let direction = &mut edited_direction;

        let cadmpeg_ir::geometry::SpringLayout::ContextFirst {
            parameter_range,
            discontinuity_flag,
            ..
        } = layout
        else {
            panic!("expected context-first spring")
        };
        *parameter_range = [-2.0, 3.0];
        let expected_flag = !*discontinuity_flag;
        *discontinuity_flag = expected_flag;
        *direction = 4;
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = cadmpeg_ir::geometry::curve_payloads::SpringCurvePayload::try_new(
            edited_layout,
            edited_direction,
        )
        .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
        expected_flag
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("spring tail regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated spring decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::Spring(definition_payload) if matches!((definition_payload.layout(), definition_payload.direction(),), (cadmpeg_ir::geometry::SpringLayout::ContextFirst {
                parameter_range,
                discontinuity_flag,
                ..
            }, 4,) if *discontinuity_flag == expected_flag && parameter_range.endpoints() == [-2.0, 3.0])));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less spring round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::Spring(definition_payload) if matches!((definition_payload.direction(),), (-3,))));
}

#[test]
fn generated_null_support_spring_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_null_support_spring_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("null-support spring decode");
    let ProceduralCurveDefinition::Spring(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected spring construction")
    };
    let layout = &definition_payload.layout().to_raw();
    let direction = definition_payload.direction();

    assert_eq!(*direction, 4);
    let cadmpeg_ir::geometry::SpringLayout::ContextFirst {
        supports,
        first_pcurve,
        second_pcurve,
        parameter_range,
        discontinuity_flag,
        ..
    } = layout
    else {
        panic!("expected context-first spring")
    };
    assert!(*discontinuity_flag);
    assert_eq!(
        supports[0],
        cadmpeg_ir::geometry::SpringSupport::Ranges([[-2.0, 3.0], [-4.0, 5.0]])
    );
    assert_eq!(
        supports[1],
        cadmpeg_ir::geometry::SpringSupport::Ranges([[-6.0, 7.0], [-8.0, 9.0]])
    );
    assert_eq!(
        **first_pcurve,
        cadmpeg_ir::geometry::SpringPcurve::Range([-10.0, 11.0])
    );
    assert_eq!(*second_pcurve, None);
    assert_eq!(*parameter_range, [-1.0, 2.0]);

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less null-support spring encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less null-support spring round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        source_less.model.procedural_curves[0].definition()
    );
}

#[test]
fn generated_deformable_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{DeformableCurveData, ProceduralCurveDefinition};

    for mode in [8, 3] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_deformable_curve_smbh(mode),
                )),
                &DecodeOptions::default(),
            )
            .expect("deformable decode");
        let ProceduralCurveDefinition::Deformable(definition_payload) =
            &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected deformable construction")
        };
        let context = definition_payload.context();
        let cache_first = definition_payload.cache_first();
        let source = definition_payload.source();
        let source_parameter_range = definition_payload.source_parameter_range();
        let data = &definition_payload.data().to_raw();
        let cadmpeg_ir::geometry::DeformableCurveSource::Curve { curve: source } = source else {
            panic!("expected resolved deformable source")
        };
        assert_eq!(cache_first.revision.get(), 23100);
        assert_eq!(context.parameter_range().endpoints(), [-1.0, 2.0]);
        assert_eq!(
            source_parameter_range.map(|bound| bound.map(cadmpeg_ir::scalar::FiniteReal::get)),
            [Some(0.0), Some(1.0)]
        );
        assert!(result
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *source));
        match (mode, data) {
            (
                8,
                DeformableCurveData::VectorField {
                    vectors,
                    parameter_pairs,
                },
            ) => {
                assert_eq!(vectors[3], cadmpeg_ir::math::Vector3::new(10.0, 11.0, 12.0));
                assert_eq!(parameter_pairs, &[[-1.0, 0.25], [2.0, 3.5]]);
            }
            (3, DeformableCurveData::Mode3 { trailing_value, .. }) => {
                assert_eq!(*trailing_value, 6);
            }
            _ => panic!("wrong deformable discriminator payload"),
        }
        let expected_data = data.clone();
        let source = source.clone();

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == source)
            .expect("deformable source carrier")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                1,
                vec![0.0, 0.0, 1.0, 1.0],
                vec![
                    cadmpeg_ir::math::Point3::new(3.0, -2.0, 5.0),
                    cadmpeg_ir::math::Point3::new(5.0, 2.0, 4.0),
                ],
                None,
                false,
            )
            .unwrap(),
        ));
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less deformable encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less deformable round trip");
        let ProceduralCurveDefinition::Deformable(definition_payload) =
            &round_trip.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected round-trip deformable construction")
        };
        let round_source = definition_payload.source();
        let round_data = &definition_payload.data().to_raw();
        let cadmpeg_ir::geometry::DeformableCurveSource::Curve {
            curve: round_source,
        } = round_source
        else {
            panic!("expected resolved round-trip deformable source")
        };
        match (&expected_data, round_data) {
            (DeformableCurveData::VectorField { .. }, DeformableCurveData::VectorField { .. }) => {
                assert_eq!(round_data, &expected_data)
            }
            (DeformableCurveData::Mode3 { .. }, DeformableCurveData::Mode3 { .. }) => {
                assert_eq!(round_data, &expected_data)
            }
            _ => panic!("round-trip deformable discriminator changed"),
        }
        assert!(round_trip
            .ir()
            .model
            .curves
            .iter()
            .any(|curve| curve.id == *round_source));
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *round_source)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
            if curve.degree() == 1
                    && curve.knots().as_slice() == [0.0, 0.0, 1.0, 1.0]
                    && curve.control_points() == [
                        cadmpeg_ir::math::Point3::new(3.0, -2.0, 5.0),
                        cadmpeg_ir::math::Point3::new(5.0, 2.0, 4.0),
                    ]
        ));
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_deformable_curve_smbh(8),
            )),
            &DecodeOptions::default(),
        )
        .expect("native-reference deformable decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Deformable(definition_payload) = definition else {
            panic!("expected deformable construction")
        };
        let mut source_value = definition_payload.source().clone();
        let source = &mut source_value;
        {
            *source = cadmpeg_ir::geometry::DeformableCurveSource::NativeReference {
                flag: false,
                index: 10_000,
            };
        };
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::DeformableCurveConstruction::try_new(
                definition_payload.context().clone(),
                definition_payload.cache_first().to_raw(),
                source_value,
                definition_payload
                    .source_parameter_range()
                    .map(|bound| bound.map(cadmpeg_ir::scalar::FiniteReal::get)),
                definition_payload.data().to_raw(),
            )
            .unwrap();
    });
    let mut encoded = Vec::new();
    F3dCodec
        .encode(&source_less, &mut encoded)
        .expect("native-reference deformable encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("native-reference deformable round trip");
    assert!(
        match &round_trip.ir().model.procedural_curves[0].definition() {
            ProceduralCurveDefinition::Deformable(matched_payload) => matches!(
                (matched_payload.source(),),
                (
                    cadmpeg_ir::geometry::DeformableCurveSource::NativeReference {
                        flag: false,
                        index: 10_000,
                    },
                )
            ),
            _ => false,
        }
    );
}

#[test]
fn generated_f3d_rewrites_procedural_curve_fit_tolerance() {
    let source = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated procedural-curve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.025))
        .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("procedural-curve fit regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated procedural-curve decode");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.025)
    );
}

#[test]
fn generated_source_less_refuses_lossy_procedural_curve_fallbacks() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_procedural_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated procedural curve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.procedural_curves[0].replace_definition(
        ProceduralCurveDefinition::BlendSpine {
            blend_surface: None,
        },
    );
    let mut encoded = Vec::new();
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect_err("typed intersection must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("lacks its native blend construction"));

    source_less.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::Unknown {
        native_kind: None,
        record: None,
        cache: None,
    });
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("unknown construction must not degrade to a cache-only curve");
    assert!(error
        .to_string()
        .contains("cannot be regenerated losslessly"));
}

#[test]
fn generated_source_less_rejects_duplicate_procedural_curve_owners() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated helix decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let duplicate = source_less.model.procedural_curves[0].clone();
    source_less.model.procedural_curves.push(duplicate);
    let mut encoded = Vec::new();
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect_err("duplicate procedural construction must be rejected");
    assert!(error
        .to_string()
        .contains("multiple procedural constructions"));
}

#[test]
fn generated_f3d_rewrites_topology_bound_nurbs_curve() {
    let source = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated intcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id.as_str() == "f3d:brep:entity#19")
        .expect("topology-bound intcurve");
    let cadmpeg_ir::geometry::CurveGeometry::Procedural {
        cache: Some(cache), ..
    } = &mut curve.geometry
    else {
        panic!("procedural carrier with a solved cache")
    };
    let SolvedCurveGeometry::Nurbs(mut nurbs) = cache.clone() else {
        panic!("expected NURBS edge carrier")
    };
    let mut control_points = nurbs.pole_rows().raw_points();
    control_points[1].x = 14.0;
    control_points[1].z = -3.0;
    nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![-1.0, -1.0, 2.0, 2.0, 2.0],
        control_points,
        nurbs.pole_rows().weights(),
        nurbs.periodic(),
    )
    .unwrap();
    *cache = SolvedCurveGeometry::Nurbs(nurbs.clone());
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("topology-bound NURBS regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intcurve decode");
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn nurbs_pcurve_block_decodes_without_length_scaling() {
    use cadmpeg_asm::nurbs::pcurve::decode_pcurve_cache;

    // A degree-1 2D pcurve. Unlike model-space NURBS control points, these
    // are UV parameters and therefore must not be converted from cm to mm.
    let b = generated_pcurve_block();

    let pcurve = decode_pcurve_cache(&b).expect("2D pcurve block decodes");
    assert_eq!(pcurve.degree(), 1);
    assert_eq!(pcurve.knots().as_slice(), [0.0, 0.0, 1.0, 1.0]);
    assert_eq!(pcurve.control_points()[0].u, 0.25);
    assert_eq!(pcurve.control_points()[1].v, 1.5);
}

#[test]
fn ref_pcurve_resolves_intcurve_uv_slot() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    let mut intcurve = generated_curve_block();
    intcurve.extend_from_slice(&generated_pcurve_block());

    let (pcurve, _) = cadmpeg_asm::nurbs::proc_curve::pcurve_for_selector_with_chart(
        &ctx,
        &cadmpeg_asm::nurbs::toks::lex_test_span(
            &intcurve,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .expect("valid single-record byte fixture"),
        2,
        &cadmpeg_asm::nurbs::toks::test_table(
            &intcurve,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .expect("valid single-record byte fixture"),
    )
    .transpose()
    .expect("decode the selected pcurve with its chart")
    .expect("intcurve slot 2 carries the UV cache");
    assert_eq!(pcurve.control_points()[0].u, 0.25);
    assert_eq!(pcurve.control_points()[1].v, 1.5);
    assert!(
        cadmpeg_asm::nurbs::proc_curve::pcurve_for_selector_with_chart(
            &ctx,
            &cadmpeg_asm::nurbs::toks::lex_test_span(
                &intcurve,
                cadmpeg_asm::kernel_header::RefWidth::Eight
            )
            .expect("valid single-record byte fixture"),
            1,
            &cadmpeg_asm::nurbs::toks::test_table(
                &intcurve,
                cadmpeg_asm::kernel_header::RefWidth::Eight
            )
            .expect("valid single-record byte fixture"),
        )
        .transpose()
        .expect("decode the selected pcurve with its chart")
        .is_none()
    );
}

#[test]
fn ref_pcurve_rejects_orphan_typed_slot() {
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
        &[],
        &arena,
        &cadmpeg_core::decode::DecodePolicy::default(),
    )
    .expect("test decode context");
    let mut target = b"\x0f\x0d\x0bint_int_cur".to_vec();
    target.extend_from_slice(&generated_curve_block());
    target.extend_from_slice(&generated_pcurve_block());
    target.push(0x10);
    let mut source = b"\x0f\x0d\x03ref\x04".to_vec();
    source.extend_from_slice(&0i64.to_le_bytes());
    source.push(0x10);
    let mut active = target;
    active.extend_from_slice(&source);

    assert!(
        cadmpeg_asm::nurbs::proc_curve::pcurve_for_selector_with_chart(
            &ctx,
            &cadmpeg_asm::nurbs::toks::lex_test_span(
                &source,
                cadmpeg_asm::kernel_header::RefWidth::Eight
            )
            .expect("valid single-record byte fixture"),
            2,
            &cadmpeg_asm::nurbs::toks::test_table(
                &active,
                cadmpeg_asm::kernel_header::RefWidth::Eight
            )
            .expect("valid single-record byte fixture"),
        )
        .transpose()
        .expect("decode the selected pcurve with its chart")
        .is_none(),
        "a pcurve without its typed support surface is not a carrier"
    );
}

#[test]
fn decode_attaches_generated_pcurve_to_its_coedge() {
    let f3d = f3d_with_smbh(&with_plane_pcurve_offset_vertices(
        synthetic_geometry_with_pcurve_smbh(),
        [-1.0, 2.0],
    ));
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|c| !c.pcurves.is_empty())
            .count(),
        1
    );
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);
}

#[test]
fn inline_pcurve_scope_is_its_exact_carrier_identity() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_inline_pcurve_on_nurbs_surface_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("structurally unique inline pcurve decode");

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn inline_pcurve_owns_its_carrier_ahead_of_referenced_supports() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_inline_pcurve_with_referenced_support_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("inline pcurve with referenced support");

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn wrapped_ref_pcurve_resolves_its_subtype_carrier() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_wrapped_ref_pcurve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("wrapped ref pcurve decode");

    assert_eq!(result.ir().model.pcurves.len(), 1);
    assert_eq!(
        result
            .ir()
            .model
            .coedges
            .iter()
            .filter(|coedge| !coedge.pcurves.is_empty())
            .count(),
        1
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
}

#[test]
fn unique_bs2_intcurve_role_is_its_ref_pcurve_carrier() {
    for discriminator in [2, -2] {
        let smbh = with_pcurve_discriminator(
            synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
            discriminator,
        );
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("structurally unique ref pcurve decode");

        assert_eq!(result.ir().model.pcurves.len(), 1);
        assert_eq!(
            result
                .ir()
                .model
                .coedges
                .iter()
                .filter(|coedge| !coedge.pcurves.is_empty())
                .count(),
            1
        );
        assert!(result
            .report()
            .losses
            .iter()
            .all(|loss| !loss.message.contains("explicit UV pcurve reference")));
    }
}

#[test]
fn negative_ref_pcurve_reverses_its_uv_parameterization() {
    let smbh = with_pcurve_discriminator(
        synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
        -2,
    );
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("reversed ref pcurve decode");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } =
        &result.ir().model.pcurves[0].geometry
    else {
        panic!("ref pcurve is not a NURBS");
    };
    assert_eq!(
        nurbs.pole_rows().raw_points().first(),
        Some(&cadmpeg_ir::math::Point2::new(-1.25, 5.5))
    );
}

#[test]
fn ref_pcurve_selector_reversal_xors_intcurve_reversal() {
    let smbh = with_pcurve_discriminator(
        with_ref_pcurve_companion_reversed(
            synthetic_geometry_with_ref_pcurve_on_nurbs_surface_smbh(),
        ),
        -2,
    );
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("doubly reversed ref pcurve decode");
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } =
        &result.ir().model.pcurves[0].geometry
    else {
        panic!("ref pcurve is not a NURBS");
    };
    assert_eq!(
        nurbs.pole_rows().raw_points().first(),
        Some(&cadmpeg_ir::math::Point2::new(-1.75, 4.5))
    );
}

#[test]
fn generated_inline_pcurve_tail_requires_four_adjacent_booleans() {
    let decode = |smbh: Vec<u8>| {
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated inline pcurve decode")
            .into_parts()
            .0
            .model
            .pcurves
            .into_iter()
            .next()
            .expect("generated inline pcurve")
    };

    let complete = decode(synthetic_geometry_with_pcurve_smbh());
    assert_eq!(
        complete.native_tail_flags(),
        Some([true, false, true, false])
    );
    assert_eq!(
        complete
            .uv_offset()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([-1.0, 2.0])
    );

    let short = decode(synthetic_geometry_with_short_pcurve_tail_smbh());
    assert_eq!(short.native_tail_flags(), None);
    assert_eq!(
        short.uv_offset().map(cadmpeg_ir::units::FiniteVector::get),
        Some([-1.0, 2.0])
    );
}

#[test]
fn generated_inline_pcurve_fit_tolerance_is_scoped() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated inline pcurve decode");
    assert_eq!(
        result.ir().model.pcurves[0]
            .fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.001)
    );
}

#[test]
fn generated_pcurve_geometry_dispatch_follows_discriminator() {
    for smbh in [
        with_pcurve_discriminator(synthetic_geometry_with_pcurve_smbh(), 2),
        with_inline_pcurve_non_boolean_wrapper(synthetic_geometry_with_pcurve_smbh()),
        renamed_generated_subtype(
            synthetic_geometry_with_pcurve_smbh(),
            "exp_par_cur",
            "bad_par_cur",
        ),
        synthetic_geometry_with_out_of_scope_pcurve_cache_smbh(),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 0),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 1),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), -1),
        with_pcurve_discriminator(synthetic_geometry_with_ref_pcurve_smbh(), 7),
        with_ref_pcurve_companion_name(synthetic_geometry_with_ref_pcurve_smbh(), b"badcurve"),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated mismatched pcurve decode");
        assert!(result.ir().model.pcurves.is_empty());
        assert!(result
            .ir()
            .model
            .coedges
            .iter()
            .all(|coedge| coedge.pcurves.is_empty()));
        let note = result
            .report()
            .losses
            .iter()
            .find(|loss| loss.message.contains("explicit UV pcurve reference"))
            .expect("undecoded pcurve loss note");
        assert!(note.message.contains("Native kinds: pcurve=1."));
    }
}

#[test]
fn generated_pcurve_reports_dangling_carrier_reference() {
    let mut smbh = synthetic_geometry_with_pcurve_smbh();
    let start = asm_header::record_stream_start(&smbh).unwrap();
    let limit = asm_header::solved_record_limit(&smbh).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &smbh,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let coedge = &records[7];
    let record = &mut smbh[coedge.offset..coedge.offset + coedge.len];
    let pcurve_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[pcurve_ref + 1..pcurve_ref + 9].copy_from_slice(&999i64.to_le_bytes());

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("dangling pcurve reference remains a successful topology decode");
    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("explicit UV pcurve reference"))
        .expect("dangling pcurve loss note");
    assert!(note.message.contains("Native kinds: dangling-reference=1."));
}

#[test]
fn generated_f3d_rewrites_nurbs_pcurve_control_points() {
    let source = f3d_with_smbh(&synthetic_geometry_with_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let pcurve = &mut edited.model.pcurves[0];
    assert_eq!(pcurve.wrapper_reversed(), Some(false));
    assert_eq!(pcurve.native_tail_flags(), Some([true, false, true, false]));
    assert_eq!(
        pcurve.uv_offset().map(cadmpeg_ir::units::FiniteVector::get),
        Some([-1.0, 2.0])
    );
    assert_eq!(
        pcurve
            .fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.001)
    );
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry else {
        panic!("expected NURBS pcurve")
    };
    let mut control_points = nurbs.pole_rows().raw_points();
    control_points[0].u = -0.5;
    control_points[1].v = 2.25;
    *nurbs = cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
        1,
        vec![-1.0, -1.0, 2.0, 2.0],
        control_points,
        nurbs.pole_rows().weights(),
        true,
    )
    .unwrap();
    let cadmpeg_ir::geometry::pcurve::PcurveMetadata::AsmInline { form: inline } =
        &mut pcurve.metadata
    else {
        panic!("decoded fixture uses ASM inline pcurve metadata")
    };
    inline.wrapper_reversed = true;
    inline.native_tail_flags = [false, true, false, true];
    {
        let replacement = [-2.0, 3.0];
        edit::replace(inline, |previous| {
            Ok::<_, &str>(cadmpeg_ir::geometry::pcurve::PcurveInlineForm::new(
                previous.wrapper_reversed,
                previous.native_tail_flags,
                cadmpeg_ir::units::FiniteVector::new(replacement).expect("finite fixture range"),
                previous.fit_tolerance(),
            ))
        })
    }
    .unwrap();
    inline.set_fit_tolerance(cadmpeg_ir::geometry::FitTolerance::try_new(0.0025).unwrap());
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert_pcurve_close(&round_trip.ir().model.pcurves[0], &expected);
}

#[test]
fn generated_f3d_scopes_inline_pcurve_edits() {
    let source =
        f3d_with_smbh(&synthetic_geometry_with_additional_out_of_scope_pcurve_cache_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated scoped pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let pcurve = &mut edited.model.pcurves[0];
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry else {
        panic!("expected NURBS pcurve")
    };
    nurbs
        .try_map_control_points(|pole_index, point| {
            let mut point = point.get();
            if pole_index == 0 {
                point.u = -0.75;
            }
            cadmpeg_ir::units::FinitePoint2::new(point).ok_or(())
        })
        .expect("pole edit admission");
    let cadmpeg_ir::geometry::pcurve::PcurveMetadata::AsmInline { form: inline } =
        &mut pcurve.metadata
    else {
        panic!("decoded fixture uses ASM inline pcurve metadata")
    };
    inline.set_fit_tolerance(cadmpeg_ir::geometry::FitTolerance::try_new(0.0025).unwrap());
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("scoped pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated scoped pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert_pcurve_close(&round_trip.ir().model.pcurves[0], &expected);
}

#[test]
fn generated_f3d_rewrites_rational_pcurve_weights() {
    let source = f3d_with_smbh(&synthetic_geometry_with_rational_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rational pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } =
        &mut edited.model.pcurves[0].geometry
    else {
        panic!("expected rational pcurve")
    };
    nurbs
        .try_map_control_points(|pole_index, point| {
            let mut point = point.get();
            if pole_index == 0 {
                point.u = -0.25;
            }
            cadmpeg_ir::units::FinitePoint2::new(point).ok_or(())
        })
        .expect("pole edit admission");
    let mut weights = nurbs.pole_rows().weights();
    if let Some(weights) = &mut weights {
        weights[1] = 0.75;
    }
    let poles = cadmpeg_ir::geometry::pcurve::PcurveNurbsPoles::from_lanes(
        nurbs.pole_rows().raw_points(),
        weights,
    );
    {
        let replacement = poles.unwrap();
        edit::replace(nurbs, |previous| {
            cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
                previous.degree(),
                previous.knots().to_vec(),
                replacement,
                previous.periodic(),
            )
        })
    }
    .unwrap();
    let expected = edited.model.pcurves[0].clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("rational pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rational pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert_pcurve_close(&round_trip.ir().model.pcurves[0], &expected);
}

#[test]
fn generated_f3d_rewrites_ref_form_pcurve_geometry_and_offset() {
    let source = f3d_with_smbh(&synthetic_geometry_with_ref_pcurve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated ref-form pcurve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let pcurve = &mut edited.model.pcurves[0];
    assert_eq!(pcurve.wrapper_reversed(), None);
    assert_eq!(pcurve.fit_tolerance(), None);
    assert_eq!(
        pcurve.uv_offset().map(cadmpeg_ir::units::FiniteVector::get),
        Some([-2.0, 4.0])
    );
    let cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs { nurbs } = &mut pcurve.geometry else {
        panic!("expected ref-form NURBS pcurve")
    };
    nurbs
        .try_map_control_points(|pole_index, point| {
            let mut point = point.get();
            if pole_index == 0 {
                point.u = -0.75;
            }
            if pole_index == 1 {
                point.v = 3.5;
            }
            cadmpeg_ir::units::FinitePoint2::new(point).ok_or(())
        })
        .expect("pole edit admission");
    edit::replace(nurbs, |previous| {
        let mut knots = previous.knots().to_vec();
        {
            let knots: &mut [f64] = &mut knots;
            knots.copy_from_slice(&[-1.0, -1.0, 2.0, 2.0]);
        };
        cadmpeg_ir::geometry::pcurve::PcurveNurbs::new(
            previous.degree(),
            knots,
            previous.pole_rows().clone(),
            previous.periodic(),
        )
    })
    .unwrap();
    let cadmpeg_ir::geometry::pcurve::PcurveMetadata::General { form: metadata } =
        &mut pcurve.metadata
    else {
        panic!("decoded fixture uses general pcurve metadata")
    };
    metadata.uv_offset = Some(cadmpeg_ir::units::FiniteVector::new([-3.0, 5.0]).unwrap());
    let expected = pcurve.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("ref-form pcurve regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated ref-form pcurve decode");
    assert_eq!(round_trip.ir().model.pcurves.len(), 1);
    assert_pcurve_close(&round_trip.ir().model.pcurves[0], &expected);

    edited.source = None;
    edited
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut source_less = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&edited, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut source_less))
        .expect("source-less ref-form pcurve encode");
    let source_less_round_trip = F3dCodec
        .decode(&mut Cursor::new(source_less), &DecodeOptions::default())
        .expect("source-less ref-form pcurve round trip");
    let actual = &source_less_round_trip.ir().model.pcurves[0];
    assert_pcurve_geometry_close(&actual.geometry, &expected.geometry);
    assert_eq!(actual.wrapper_reversed(), expected.wrapper_reversed());
    assert_eq!(actual.native_tail_flags(), expected.native_tail_flags());
    assert_eq!(actual.uv_offset(), expected.uv_offset());
    assert_eq!(actual.fit_tolerance(), expected.fit_tolerance());
    assert!(source_less_round_trip
        .ir()
        .model
        .coedges
        .iter()
        .any(|coedge| coedge.pcurves.iter().any(|use_| use_.pcurve == actual.id)));

    let mut mixed = edited;
    let mut inline = mixed.model.pcurves[0].clone();
    inline.id = cadmpeg_ir::ids::PcurveId::mint("generated:test:mixed-inline-pcurve#0")
        .expect("identity grammar");
    let Some(uv_offset) = inline.uv_offset() else {
        panic!("ref-form fixture carries a UV offset")
    };
    inline.metadata = cadmpeg_ir::geometry::pcurve::PcurveMetadata::AsmInline {
        form: cadmpeg_ir::geometry::pcurve::PcurveInlineForm::new(
            false,
            [true, false, true, false],
            uv_offset,
            cadmpeg_ir::geometry::FitTolerance::try_new(0.002)
                .expect("finite non-negative fixture tolerance"),
        ),
    };
    mixed.model.coedges[1].pcurves = vec![cadmpeg_ir::topology::PcurveUse {
        pcurve: inline.id.clone(),
        isoparametric: None,
        parameter_range: None,
    }];
    mixed.model.pcurves.push(inline);
    let mut mixed_bytes = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&mixed, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut mixed_bytes))
        .expect("mixed inline/ref-form pcurve encode");
    let mixed_round_trip = F3dCodec
        .decode(&mut Cursor::new(mixed_bytes), &DecodeOptions::default())
        .expect("mixed inline/ref-form pcurve round trip");
    assert_eq!(mixed_round_trip.ir().model.pcurves.len(), 2);
    assert!(mixed_round_trip
        .ir()
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed().is_none()));
    assert!(mixed_round_trip
        .ir()
        .model
        .pcurves
        .iter()
        .any(|pcurve| pcurve.wrapper_reversed() == Some(false)));
    assert!(mixed_round_trip
        .ir()
        .model
        .coedges
        .iter()
        .flat_map(|coedge| coedge.pcurves.iter().map(|use_| &use_.pcurve))
        .all(|pcurve_id| mixed_round_trip
            .ir()
            .model
            .pcurves
            .iter()
            .any(|pcurve| pcurve.id == *pcurve_id)));
}
