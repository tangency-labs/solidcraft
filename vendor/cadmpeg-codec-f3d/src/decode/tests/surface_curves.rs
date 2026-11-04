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

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::smbh_curves_test::{
    synthetic_geometry_with_early_close_projection_smbh, synthetic_geometry_with_projection_smbh,
    synthetic_geometry_with_silhouette_smbh, synthetic_geometry_with_surface_curve_smbh,
    synthetic_geometry_with_three_surface_intersection_smbh,
};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn generated_projection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionRole, ProjectionTail};

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_projection_smbh())),
                &DecodeOptions::default(),
            )
            .expect("projection decode"),
    );
    let ProceduralCurveDefinition::Projection(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected projection")
    };
    let context = definition_payload.context();
    let discontinuity_flag = definition_payload.discontinuity_flag();
    let source = definition_payload.source();
    let tail = &definition_payload.tail().to_raw();

    assert!(context.sides().iter().all(|side| side.surface.is_some()));
    assert!(*discontinuity_flag);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: ProjectionRole::Surf2,
        }
    );

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Projection(definition_payload) = definition else {
            unreachable!()
        };
        let mut edited_context = definition_payload.context().clone();
        let mut edited_discontinuity_flag = *definition_payload.discontinuity_flag();
        let mut edited_tail = definition_payload.tail().to_raw();
        let context = &mut edited_context;
        let discontinuity_flag = &mut edited_discontinuity_flag;
        let tail = &mut edited_tail;

        edit::replace(context, |previous| {
            let sides = previous.sides().clone();
            let mut range = previous.parameter_range().endpoints();
            let discontinuities =
                cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
            {
                let context_parameter_range: &mut [f64; 2] = &mut range;

                (*context_parameter_range) = [-1.0, 2.0];
                *discontinuity_flag = false;
                let ProjectionTail::Ranged {
                    flag,
                    parameter_range,
                    role,
                } = tail
                else {
                    unreachable!()
                };
                *flag = false;
                *parameter_range = [-4.0, 5.0];
                *role = ProjectionRole::Surf1;
            };
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
        })
        .unwrap();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::ProjectionCurvePayload::try_new(
                edited_context,
                edited_discontinuity_flag,
                definition_payload.source().clone(),
                edited_tail,
            )
            .unwrap();
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("projection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated projection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::Projection(definition_payload) if matches!((definition_payload.context(), definition_payload.discontinuity_flag(), definition_payload.tail(),), (context, false, ProjectionTail::Ranged {
                flag: false,
                parameter_range,
                role,
            },) if context.parameter_range().endpoints() == [-1.0, 2.0]
                && cadmpeg_ir::scalar::FiniteReal::raw_array(*parameter_range) == [-4.0, 5.0]
                && *role == ProjectionRole::Surf1)));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less projection round trip");
    let ProceduralCurveDefinition::Projection(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip projection")
    };
    let discontinuity_flag = definition_payload.discontinuity_flag();
    let tail = &definition_payload.tail().to_raw();

    assert!(*discontinuity_flag);
    assert_eq!(
        tail,
        &ProjectionTail::Ranged {
            flag: true,
            parameter_range: [-2.0, 3.0],
            role: ProjectionRole::Surf2,
        }
    );
}

#[test]
fn generated_early_close_projection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, ProjectionTail};

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_early_close_projection_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("early-close projection decode"),
    );
    assert!(matches!(
        result.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::Projection(definition_payload) if matches!((definition_payload.discontinuity_flag(), definition_payload.tail(),), (true, ProjectionTail::EarlyClose { flag: true },))));

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Projection(definition_payload) = definition else {
            unreachable!()
        };
        let mut edited_tail = definition_payload.tail().to_raw();
        let (ProjectionTail::EarlyClose { flag },) = (&mut edited_tail,) else {
            unreachable!()
        };

        *flag = false;
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::ProjectionCurvePayload::try_new(
                definition_payload.context().clone(),
                *definition_payload.discontinuity_flag(),
                definition_payload.source().clone(),
                edited_tail,
            )
            .unwrap();
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("early-close projection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated early-close projection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::Projection(definition_payload) if matches!((definition_payload.tail(),), (ProjectionTail::EarlyClose { flag: false },))));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less early-close projection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less early-close projection round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::Projection(definition_payload) if matches!((definition_payload.discontinuity_flag(), definition_payload.tail(),), (true, ProjectionTail::EarlyClose { flag: true },))));
}

#[test]
fn generated_three_surface_intersection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SolvedSurfaceGeometry, SurfaceGeometry};

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_three_surface_intersection_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("three-surface intersection decode"),
    );
    let ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected three-surface intersection")
    };
    let context = definition_payload.context();
    let selector = definition_payload.selector();
    let third = definition_payload.third();

    assert_eq!(*selector, 7);
    assert!(context.sides().iter().all(|side| side.surface.is_some()));
    let third_surface = result
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("third support surface");
    assert!(
        matches!(third_surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) if { sphere_surface.radius().get() == -12.5 })
    );

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) = definition
        else {
            unreachable!()
        };
        let mut edited_context = definition_payload.context().clone();
        let mut edited_selector = *definition_payload.selector();
        let context = &mut edited_context;
        let selector = &mut edited_selector;

        edit::replace(context, |previous| {
            let sides = previous.sides().clone();
            let mut range = previous.parameter_range().endpoints();
            let discontinuities =
                cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
            {
                let context_parameter_range: &mut [f64; 2] = &mut range;

                (*context_parameter_range) = [-1.0, 2.0];
                *selector = -4;
            };
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
        })
        .unwrap();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::ThreeSurfaceIntersectionCurvePayload::try_new(
                edited_context,
                edited_selector,
                definition_payload.third().clone(),
            )
            .unwrap();
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("three-surface intersection regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated three-surface intersection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(), ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) if matches!((definition_payload.context(), definition_payload.selector(),), (context, -4,) if context.parameter_range().endpoints() == [-1.0, 2.0])));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less three-surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less three-surface intersection round trip");
    let ProceduralCurveDefinition::ThreeSurfaceIntersection(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip three-surface intersection")
    };
    let selector = definition_payload.selector();
    let third = definition_payload.third();

    assert_eq!(*selector, 7);
    let third_surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == third.surface.as_ref())
        .expect("round-trip third support surface");
    assert!(
        matches!(third_surface.geometry, SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) if { sphere_surface.radius().get() == -12.5 })
    );
}

#[test]
fn generated_prefix_only_surface_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SurfaceCurveFamilyKind};

    for (name, expected_family) in [
        ("blend_int_cur", SurfaceCurveFamilyKind::Blend),
        ("surf_int_cur", SurfaceCurveFamilyKind::SurfaceConstrained),
        ("par_int_cur", SurfaceCurveFamilyKind::Parametric),
        ("skin_int_cur", SurfaceCurveFamilyKind::Skin),
    ] {
        let result = EditableDecodeResult::from(
            F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_surface_curve_smbh(
                        name,
                    ))),
                    &DecodeOptions::default(),
                )
                .unwrap_or_else(|error| panic!("{name} decode failed: {error}")),
        );
        let ProceduralCurveDefinition::SurfaceCurve { family } =
            &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected {name} surface curve")
        };
        assert_eq!(family.kind(), expected_family);
        let context = family.context();
        assert!(context.sides().iter().all(|side| side.surface.is_some()));

        let mut edited = result.ir().clone();
        edited.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::SurfaceCurve { family } = definition else {
                unreachable!()
            };
            edit::replace(
                &mut *(match family {
                    cadmpeg_ir::geometry::SurfaceCurveFamily::Blend { context, .. }
                    | cadmpeg_ir::geometry::SurfaceCurveFamily::SurfaceConstrained {
                        context,
                        ..
                    }
                    | cadmpeg_ir::geometry::SurfaceCurveFamily::Parametric { context, .. }
                    | cadmpeg_ir::geometry::SurfaceCurveFamily::Skin { context, .. } => context,
                }),
                |previous| {
                    let sides = previous.sides().clone();
                    let mut range = previous.parameter_range().endpoints();
                    let discontinuities =
                        cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
                    {
                        let range: &mut [f64; 2] = &mut range;

                        *range = [-1.0, 2.0];
                    };
                    cadmpeg_ir::geometry::IntcurveSupportContext::try_new(
                        sides,
                        range,
                        discontinuities,
                    )
                },
            )
            .unwrap();
        });
        let mut regenerated = Vec::new();
        crate::test_support::plan_inherited_write(
            &edited,
            result.source_fidelity(),
            &mut regenerated,
        )
        .unwrap_or_else(|error| panic!("{name} context regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::SurfaceCurve { ref family }
                if family.context().parameter_range().endpoints() == [-1.0, 2.0]
        ));

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            &round_trip.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::SurfaceCurve { family } if family.kind() == expected_family
        ));
    }
}

#[test]
fn generated_silhouette_curves_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SilhouetteKind};

    for (name, draft_factor) in [
        ("silh_int_cur", None),
        ("para_silh_int_cur", None),
        ("taper_silh_int_cur", Some(0.35)),
    ] {
        let result = EditableDecodeResult::from(
            F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_silhouette_smbh(
                        name,
                        draft_factor,
                    ))),
                    &DecodeOptions::default(),
                )
                .unwrap_or_else(|error| panic!("{name} decode failed: {error}")),
        );
        let ProceduralCurveDefinition::Silhouette(definition_payload) =
            &result.ir().model.procedural_curves[0].definition()
        else {
            panic!("expected {name} silhouette")
        };
        let silhouette = definition_payload.silhouette();
        let cast_surface = definition_payload.cast_surface();
        let light_direction = definition_payload.light_direction();
        assert!(result
            .ir()
            .model
            .surfaces
            .iter()
            .any(|surface| surface.id == *cast_surface));
        assert_eq!(
            *light_direction,
            cadmpeg_ir::math::Vector3::new(0.0, -1.0, 0.0)
        );
        match (silhouette, draft_factor) {
            (SilhouetteKind::Standard {}, None) if name == "silh_int_cur" => {}
            (SilhouetteKind::Parametric {}, None) if name == "para_silh_int_cur" => {}
            (
                SilhouetteKind::Taper {
                    draft_factor: actual,
                },
                Some(expected),
            ) => {
                assert_eq!(actual.get(), expected);
            }
            _ => panic!("wrong silhouette family for {name}"),
        }

        let mut edited = result.ir().clone();
        edited.model.procedural_curves[0].edit_definition(|definition| {
            let ProceduralCurveDefinition::Silhouette(definition_payload) = definition else {
                unreachable!()
            };
            let silhouette = match definition_payload.silhouette() {
                SilhouetteKind::Taper { .. } => SilhouetteKind::Taper {
                    draft_factor: cadmpeg_ir::scalar::FiniteReal::new(-0.2).unwrap(),
                },
                kind => kind.clone(),
            };
            *definition_payload =
                cadmpeg_ir::geometry::curve_payloads::SilhouetteCurveConstruction::try_new(
                    definition_payload.context().clone(),
                    silhouette,
                    definition_payload.cast_surface().clone(),
                    cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0),
                )
                .unwrap();
        });
        let mut regenerated = Vec::new();
        crate::test_support::plan_inherited_write(
            &edited,
            result.source_fidelity(),
            &mut regenerated,
        )
        .unwrap_or_else(|error| panic!("{name} regeneration failed: {error}"));
        let regenerated = F3dCodec
            .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("regenerated {name} decode failed: {error}"));
        assert!(matches!(
            regenerated.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Silhouette(definition_payload)
                if *definition_payload.light_direction()
                    == cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
                    && match definition_payload.silhouette() {
                        SilhouetteKind::Taper { draft_factor } => draft_factor.get() == -0.2,
                        _ => true,
                    }
        ));

        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{name} source-less encode failed: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{name} round trip failed: {error}"));
        assert!(matches!(
            round_trip.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Silhouette { .. }
        ));
    }
}
