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

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::smbh_curves_test::{
    synthetic_geometry_with_compound_curve_smbh,
    synthetic_geometry_with_consistent_exact_curve_smbh,
    synthetic_geometry_with_early_close_projection_smbh, synthetic_geometry_with_helix_curve_smbh,
    synthetic_geometry_with_law_curve_smbh, synthetic_geometry_with_projection_smbh,
    synthetic_geometry_with_silhouette_smbh, synthetic_geometry_with_spring_smbh,
    synthetic_geometry_with_subset_curve_smbh, synthetic_geometry_with_surface_curve_smbh,
    synthetic_geometry_with_surface_offset_smbh,
    synthetic_geometry_with_three_surface_intersection_smbh,
    synthetic_geometry_with_two_sided_offset_curve_smbh,
    synthetic_geometry_with_vector_offset_curve_smbh,
};
use crate::test_support::smbh_surfaces_test::{
    synthetic_compound_loft_smbh, synthetic_law_spl_sur_smbh, synthetic_scaled_compound_loft_smbh,
    synthetic_skin_spl_sur_smbh, synthetic_sub_spl_sur_smbh,
};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

#[test]
fn generated_procedural_curve_optional_tolerance_absence_round_trips() {
    let cases = [
        (
            synthetic_geometry_with_consistent_exact_curve_smbh(),
            "exact",
        ),
        (synthetic_geometry_with_law_curve_smbh(), "law"),
        (synthetic_geometry_with_projection_smbh(), "projection"),
        (
            synthetic_geometry_with_early_close_projection_smbh(),
            "early-close projection",
        ),
        (synthetic_geometry_with_compound_curve_smbh(), "compound"),
        (
            synthetic_geometry_with_surface_curve_smbh("surf_int_cur"),
            "surface curve",
        ),
        (
            synthetic_geometry_with_silhouette_smbh("para_silh_int_cur", None),
            "silhouette",
        ),
        (
            synthetic_geometry_with_surface_offset_smbh(),
            "surface offset",
        ),
        (synthetic_geometry_with_spring_smbh(), "spring"),
        (
            synthetic_geometry_with_three_surface_intersection_smbh(),
            "three-surface intersection",
        ),
        (
            synthetic_geometry_with_two_sided_offset_curve_smbh(),
            "two-sided offset",
        ),
        (
            synthetic_geometry_with_vector_offset_curve_smbh(),
            "vector offset",
        ),
        (synthetic_geometry_with_subset_curve_smbh(), "subset"),
        (synthetic_geometry_with_helix_curve_smbh(), "helix"),
    ];
    for (smbh, family) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} decode: {error}"));
        assert_eq!(
            decoded.ir().model.procedural_curves.len(),
            1,
            "{family} fixture must decode one procedural curve"
        );
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less.model.procedural_curves[0]
            .set_cache_fit_tolerance(None)
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap_or_else(|error| panic!("{family} source-less encode: {error}"));
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{family} round trip: {error}"));
        assert_eq!(
            round_trip.ir().model.procedural_curves.len(),
            1,
            "{family} procedural curve was not reconstructed"
        );
        assert_eq!(
            round_trip.ir().model.procedural_curves[0].cache_fit_tolerance(),
            None,
            "{family} invented a cache-fit tolerance"
        );
    }
}

#[test]
fn generated_compound_loft_decodes_scale_and_zero_tail() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, CompoundLoftTail, ProceduralSurfaceDefinition,
    };

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_compound_loft_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound-loft decode");
    let ProceduralSurfaceDefinition::CompoundLoft(definition_payload) =
        &result.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected compound loft")
    };
    let construction = &definition_payload.construction().to_raw();

    let scale = &construction.scales.as_slice()[0];
    assert_eq!(construction.scales.as_slice().len(), 1);
    assert_eq!(scale.members.len(), 1);
    assert!(scale.members[0].data.pcurve.is_some());
    assert_eq!(scale.auxiliaries.len(), 1);
    assert_eq!(scale.tail, [2, 3]);
    assert_eq!(construction.flags, [true, false]);
    let CompoundLoftTail::Zero {
        flags,
        direction,
        trailing_flags,
    } = &construction.tail
    else {
        panic!("expected zero tail")
    };
    assert_eq!(*flags, [false, true]);
    assert_eq!(direction.selector(), 0);
    assert!(matches!(direction, CompoundLoftDirection::Vector { .. }));
    assert_eq!(*trailing_flags, [true, false]);
    let member_curve = scale.members[0].curve.clone();

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut missing_tolerance = source_less.clone();
    missing_tolerance.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(None)
        .unwrap();
    let error = F3dCodec
        .plan(
            EncodeInput::new(&missing_tolerance, None),
            TargetRequest::Inherit,
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("compound loft without its required tolerance must be rejected");
    assert!(
        error
            .to_string()
            .contains("compound-loft surface requires a native cache-fit tolerance"),
        "unexpected error: {error}"
    );
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == member_curve)
        .expect("compound-loft member curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
            cadmpeg_ir::math::Vector3::new(4.0, -3.0, 2.0)
                .unit()
                .unwrap(),
        )
        .unwrap(),
    ));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less compound-loft encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound-loft round trip");
    let ProceduralSurfaceDefinition::CompoundLoft(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip compound loft")
    };
    let construction = &definition_payload.construction().to_raw();

    assert_eq!(construction.scales.as_slice().len(), 1);
    assert_eq!(construction.flags, [true, false]);
    assert!(matches!(
        construction.tail,
        CompoundLoftTail::Zero {
            direction: CompoundLoftDirection::Vector { .. },
            ..
        }
    ));
    let member_curve = &construction.scales.as_slice()[0].members[0].curve;
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *member_curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
            if curve.degree() == 1 && curve.knots().as_slice() == [0.0, 0.0, 1.0, 1.0]
    ));
}

#[test]
fn generated_compound_loft_writes_every_tail_shape_source_less() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, CompoundLoftTail, ProceduralSurfaceDefinition,
    };
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_compound_loft_smbh())),
            &DecodeOptions::default(),
        )
        .expect("compound-loft decode");
    let ProceduralSurfaceDefinition::CompoundLoft(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected compound loft")
    };
    let construction = &definition_payload.construction().to_raw();

    let scale = construction.scales.as_slice()[0].clone();
    let curve = scale.path.clone();
    let line_curve = cadmpeg_ir::ids::CurveId::mint("generated:test:compound_loft_tail_line#0")
        .expect("identity grammar");
    let tails = [
        CompoundLoftTail::Six {
            flags: [true, false],
            scale: Box::new(scale.clone()),
            selector: 31,
            direction: Vector3::new(0.0, 1.0, 0.0),
            parameter_range: [-0.5, 1.5],
            curve: line_curve.clone(),
        },
        CompoundLoftTail::Seven {
            first_flag: true,
            first_scale: Some(Box::new(scale.clone())),
            second_flag: false,
            second_scale: Box::new(scale.clone()),
            selector: -7,
            direction: Vector3::new(1.0, 0.0, 0.0),
            trailing_flags: [false, true],
        },
        CompoundLoftTail::Zero {
            flags: [false, true],
            direction: CompoundLoftDirection::Curve {
                curve,
                selector: std::num::NonZeroI64::new(4).unwrap(),
            },
            trailing_flags: [true, true],
        },
    ];

    for (tail_index, expected) in tails.into_iter().enumerate() {
        let mut source_less = decoded.ir().clone();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less.model.curves.push(cadmpeg_ir::geometry::Curve {
            parameter_range: None,
            id: line_curve.clone(),
            geometry: cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
                cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                    cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                    cadmpeg_ir::math::Vector3::new(4.0, -2.0, 1.0)
                        .unit()
                        .unwrap(),
                )
                .unwrap(),
            )),
            source_object: None,
        });
        source_less.model.procedural_surfaces[0].edit_definition(|definition| {
            let ProceduralSurfaceDefinition::CompoundLoft(definition_payload) = definition else {
                unreachable!()
            };
            let mut edited_construction = Box::new(definition_payload.construction().to_raw());
            let construction = &mut edited_construction;

            construction.tail = expected.clone();
            let restored_cache = definition_payload.legacy_cache();
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::CompoundLoftSurfacePayload::try_new(
                    *edited_construction,
                    None,
                )
                .unwrap();
            definition
                .set_legacy_cache(restored_cache)
                .expect("the rebuilt construction states the same legacy cache slot");
        });
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less compound-loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less compound-loft round trip");
        assert_eq!(
            round_trip.ir().model.procedural_surfaces.len(),
            1,
            "tail {tail_index} did not decode"
        );
        let ProceduralSurfaceDefinition::CompoundLoft(definition_payload) =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip compound loft")
        };
        let construction = &definition_payload.construction().to_raw();

        match (&expected, &construction.tail) {
            (
                CompoundLoftTail::Six { .. },
                CompoundLoftTail::Six {
                    parameter_range,
                    curve,
                    ..
                },
            ) => {
                assert!(matches!(
                    round_trip
                        .ir()
                        .model
                        .curves
                        .iter()
                        .find(|candidate| candidate.id == *curve)
                        .map(|curve| &curve.geometry),
                    Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
                        if curve.degree() == 1
                            && curve.knots().as_slice()
                                == [
                                    parameter_range[0],
                                    parameter_range[0],
                                    parameter_range[1],
                                    parameter_range[1],
                                ]
                ));
            }
            (CompoundLoftTail::Seven { .. }, CompoundLoftTail::Seven { first_scale, .. }) => {
                assert!(first_scale.is_some());
            }
            (
                CompoundLoftTail::Zero { .. },
                CompoundLoftTail::Zero {
                    direction: CompoundLoftDirection::Curve { selector, .. },
                    ..
                },
            ) => assert_eq!(selector.get(), 4),
            _ => panic!("compound-loft tail shape changed"),
        }
    }
}

#[test]
fn generated_scaled_compound_loft_decodes_full_direct_branch() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftShape,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected scaled compound loft")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        construction.shape,
        ScaledCompoundLoftShape::Full {}
    ));
    assert_eq!(construction.singularity, 11);
    assert_eq!(construction.discontinuities[0], [0.25]);
    assert!(construction.discontinuities[1..].iter().all(Vec::is_empty));
    assert!(construction.discontinuity_flag);
    assert_eq!(construction.scales.as_slice().len(), 1);
    assert_eq!(construction.flags, [true, false]);
    assert_eq!(construction.selector, 0);
    assert!(matches!(
        construction.branch,
        ScaledCompoundLoftBranch::Direct {
            flag: true,
            direction: CompoundLoftDirection::Vector { .. },
        }
    ));
    assert_eq!(construction.trailing_flags, [false, true]);
    assert_eq!(construction.tail_kind, 2);
    assert_eq!(construction.tail_singularity, 12);

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut missing_tolerance = source_less.clone();
    missing_tolerance.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(None)
        .unwrap();
    assert!(F3dCodec
        .plan(
            EncodeInput::new(&missing_tolerance, None),
            TargetRequest::Inherit
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("full scaled compound loft without tolerance must fail")
        .to_string()
        .contains("full shape requires a native cache-fit tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less scaled compound-loft encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less scaled compound-loft round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::ScaledCompoundLoft(..)
    ));
}

#[test]
fn generated_scaled_compound_loft_writes_all_middle_branches_source_less() {
    use cadmpeg_ir::geometry::{
        CompoundLoftDirection, ProceduralSurfaceDefinition, ScaledCompoundLoftBranch,
        ScaledCompoundLoftShape,
    };
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(true))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected scaled compound loft")
    };
    let construction = &definition_payload.construction().to_raw();

    let scale = construction.scales.as_slice()[0].clone();
    let curve = scale.path.clone();
    let cases = [
        (
            ScaledCompoundLoftShape::Full {},
            ScaledCompoundLoftBranch::ExtendedVector {
                first_scale: None,
                second_scale: Box::new(scale.clone()),
                selector: 9,
                direction: Vector3::new(1.0, 0.0, 0.0),
            },
        ),
        (
            ScaledCompoundLoftShape::Full {},
            ScaledCompoundLoftBranch::ExtendedCurve {
                scale: None,
                flag: true,
                singularity: 13,
                curve: curve.clone(),
            },
        ),
        (
            ScaledCompoundLoftShape::Full {},
            ScaledCompoundLoftBranch::Direct {
                flag: false,
                direction: CompoundLoftDirection::Curve {
                    curve: curve.clone(),
                    selector: std::num::NonZeroI64::new(4).unwrap(),
                },
            },
        ),
    ];

    for (case_index, (shape, branch)) in cases.into_iter().enumerate() {
        let mut source_less = decoded.ir().clone();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less.model.procedural_surfaces[0].edit_definition(|definition| {
            let ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) = definition
            else {
                unreachable!()
            };
            let mut edited_construction = Box::new(definition_payload.construction().to_raw());
            let construction = &mut edited_construction;

            construction.shape = shape;
            construction.branch = branch;
            let restored_cache = definition_payload.legacy_cache();
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::ScaledCompoundLoftSurfacePayload::try_new(
                    edited_construction,
                    None,
                )
                .unwrap();
            definition
                .set_legacy_cache(restored_cache)
                .expect("the rebuilt construction states the same legacy cache slot");
        });
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less scaled compound-loft encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("source-less scaled compound-loft round trip");
        assert_eq!(
            round_trip.ir().model.procedural_surfaces.len(),
            1,
            "scaled compound-loft case {case_index} did not decode"
        );
        let ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip scaled compound loft")
        };
        let construction = &definition_payload.construction().to_raw();

        assert!(matches!(
            (&construction.shape, &construction.branch),
            (
                ScaledCompoundLoftShape::Full {},
                ScaledCompoundLoftBranch::ExtendedVector { .. }
                    | ScaledCompoundLoftBranch::ExtendedCurve { .. }
                    | ScaledCompoundLoftBranch::Direct {
                        direction: CompoundLoftDirection::Curve { .. },
                        ..
                    }
            )
        ));
    }
}

#[test]
fn generated_scaled_compound_loft_none_shape_round_trips_as_procedural_face() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, ScaledCompoundLoftShape, SurfaceGeometry,
    };

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_scaled_compound_loft_smbh(false))),
            &DecodeOptions::default(),
        )
        .expect("scaled compound-loft none-shape decode");
    let ProceduralSurfaceDefinition::ScaledCompoundLoft(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected scaled compound loft")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        construction.shape,
        ScaledCompoundLoftShape::None {
            parameter_ranges: [[-1.0, 2.0], [-3.0, 4.0]],
            ..
        }
    ));
    let owner = decoded
        .ir()
        .model
        .procedural_surface_owner(&decoded.ir().model.procedural_surfaces[0].id)
        .expect("procedural surface owner")
        .clone();
    assert!(matches!(
        decoded
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == owner)
            .expect("procedural owner")
            .geometry,
        SurfaceGeometry::Procedural { ref construction, .. }
            if *construction == decoded.ir().model.procedural_surfaces[0].id
    ));
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut unexpected_tolerance = source_less.clone();
    unexpected_tolerance.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(Some(0.04))
        .unwrap();
    assert!(F3dCodec
        .plan(
            EncodeInput::new(&unexpected_tolerance, None),
            TargetRequest::Inherit
        )
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("none-shape scaled compound loft with tolerance must fail")
        .to_string()
        .contains("none shape cannot carry a cache-fit tolerance"));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less scaled compound-loft none-shape encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less scaled compound-loft none-shape round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::ScaledCompoundLoft(..)
    ));
}

#[test]
fn generated_skin_surface_decodes_recursive_spline_law() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SkinSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, false))),
            &DecodeOptions::default(),
        )
        .expect("skin surface decode");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert_eq!(construction.surface_boolean, 1);
    assert_eq!(construction.surface_normal, 2);
    assert_eq!(construction.surface_direction, 3);
    assert_eq!(construction.count, 4);
    assert_eq!(construction.parameter, 0.25);
    assert!(matches!(
        construction.layout,
        SkinSurfaceLayout::Compact { .. }
    ));
    assert_eq!(construction.direction.z, 1.0);
    assert_eq!(construction.trailing_parameter, 0.75);
    assert!(
        matches!(&construction.formula, cadmpeg_ir::geometry::LawFormula::Named { name, .. } if name.as_str() == "skin-law")
    );
    assert!(matches!(
        construction.formula.variables(),
        [LawExpression::Spline {
            native_id: 5,
            knots,
            controls,
            ..
        }] if knots == &[0.0, 0.5, 1.0] && controls == &[1.0, 2.0, 3.0]
    ));
    assert_eq!(construction.discontinuities[0], [0.1]);
    assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
    assert!(construction.discontinuity_flag);

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less skin surface encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less skin surface round trip");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        construction.formula.variables(),
        [LawExpression::Spline { native_id: 5, .. }]
    ));
}

#[test]
fn generated_law_surfaces_decode_and_round_trip_modern_and_legacy_layouts() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    for (name, legacy_ranges) in [("law_spl_sur", false), ("lawsur", true)] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_law_spl_sur_smbh(
                    name,
                    legacy_ranges,
                    0,
                ))),
                &DecodeOptions::default(),
            )
            .expect("law surface decode");
        let ProceduralSurfaceDefinition::Law(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected law surface")
        };
        let construction = &definition_payload.construction().to_raw();

        assert_eq!(
            construction.parameter_ranges,
            legacy_ranges.then_some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert!(
            matches!(&construction.primary, cadmpeg_ir::geometry::LawFormula::Named { name, .. } if name.as_str() == "primary-law")
        );
        assert!(matches!(
            construction.primary.variables(),
            [LawExpression::Algebraic { operator, operands }]
                if operator == "SET" && operands.len() == 1
        ));
        assert_eq!(construction.additional.len(), 1);
        assert!(
            matches!(&construction.additional[0], cadmpeg_ir::geometry::LawFormula::Named { name, .. } if name.as_str() == "aux-law")
        );
        assert!(matches!(
            construction.additional[0].variables(),
            [LawExpression::Algebraic { operator, operands }]
                if operator == "TERM" && operands.len() == 2
        ));
        assert_eq!(construction.discontinuities[0], [0.1]);
        assert_eq!(construction.discontinuities[1], [0.2, 0.3]);
        assert_eq!(
            decoded.ir().model.procedural_surfaces[0]
                .cache_fit_tolerance()
                .map(cadmpeg_ir::geometry::FitTolerance::get),
            Some(0.07)
        );

        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Law(definition_payload) =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip law surface")
        };
        let construction = &definition_payload.construction().to_raw();

        assert_eq!(
            construction.parameter_ranges,
            legacy_ranges.then_some([[-1.0, 2.0], [-3.0, 4.0]])
        );
        assert_eq!(construction.additional.len(), 1);
    }
}

#[test]
fn generated_sub_surfaces_decode_and_write_exact_support_graphs() {
    use cadmpeg_ir::geometry::{
        ProceduralSurfaceDefinition, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    for name in ["sub_spl_sur", "subsur"] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_sub_spl_sur_smbh(name))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let procedural = &decoded.ir().model.procedural_surfaces[0];
        let ProceduralSurfaceDefinition::SubSurface(definition_payload) = procedural.definition()
        else {
            panic!("expected sub-surface")
        };
        let support = definition_payload.support();
        let parameter_ranges = definition_payload.parameter_ranges();
        assert_eq!(
            parameter_ranges.map(cadmpeg_ir::units::FiniteVector::get),
            [[-1.0, 2.0], [-3.0, 4.0]]
        );
        assert!(matches!(decoded
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == *support)
        .map(|surface| &surface.geometry), Some(SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)))
            if {
                let origin = plane_surface.origin();
                *origin == cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0)
            }));
        assert!(matches!(
            decoded
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| {
                    decoded.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
                })
                .map(|surface| &surface.geometry),
            Some(SurfaceGeometry::Procedural { .. })
        ));

        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        assert!(
            match round_trip.ir().model.procedural_surfaces[0].definition() {
                ProceduralSurfaceDefinition::SubSurface(matched_payload) => matches!(
                    (&matched_payload
                        .parameter_ranges()
                        .map(cadmpeg_ir::units::FiniteVector::get),),
                    ([[-1.0, 2.0], [-3.0, 4.0]],)
                ),
                _ => false,
            }
        );
    }
}

#[test]
fn generated_law_surfaces_round_trip_every_standard_tail_mode() {
    use cadmpeg_ir::geometry::{LawSurfaceTail, ProceduralSurfaceDefinition};

    for selector in 1..=4 {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_law_spl_sur_smbh(
                    "law_spl_sur",
                    false,
                    selector,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Law(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected law surface")
        };
        let construction = &definition_payload.construction().to_raw();

        assert!(match (&construction.tail, selector) {
            (
                LawSurfaceTail::Summary {
                    parameters,
                    fit_tolerance,
                    closures: [0, 2],
                    singularities: [1, 3],
                },
                1,
            ) => parameters[0] == [0.0, 0.5, 1.0] && fit_tolerance.get() == 0.08,
            (
                LawSurfaceTail::None {
                    parameter_ranges: [[-0.5, 1.5], [-2.0, 2.0]],
                    closures: [1, 2],
                    singularities: [0, 4],
                },
                2,
            ) => true,
            (LawSurfaceTail::Historical {}, 3) | (LawSurfaceTail::Optimal {}, 4) => true,
            _ => false,
        });
        assert_eq!(
            decoded.ir().model.procedural_surfaces[0].cache_fit_tolerance(),
            None
        );
        assert!(matches!(
            decoded
                .ir()
                .model
                .surfaces
                .iter()
                .find(|surface| {
                    decoded
                        .ir()
                        .model
                        .procedural_surface_owner(&decoded.ir().model.procedural_surfaces[0].id)
                        == Some(&surface.id)
                })
                .map(|surface| &surface.geometry),
            Some(cadmpeg_ir::geometry::SurfaceGeometry::Procedural { .. })
        ));
        let expected_tail = construction.tail.clone();

        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap();
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Law(definition_payload) =
            &round_trip.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected round-trip law surface")
        };
        let construction = &definition_payload.construction().to_raw();

        assert_eq!(construction.tail, expected_tail);
    }
}

#[test]
fn generated_skin_surface_round_trips_structural_law_nodes() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(1, false))),
            &DecodeOptions::default(),
        )
        .expect("skin structural-law decode");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        construction.formula.variables(),
        [
            LawExpression::Null {},
            LawExpression::Transform {
                enums: [4, 5, 6],
                ..
            },
            LawExpression::Edge {
                parameters: [-0.25, 1.25],
                ..
            }
        ]
    ));
    let LawExpression::Edge { curve, .. } = &construction.formula.variables()[2] else {
        unreachable!()
    };
    let law_edge = curve.id.clone();

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == law_edge)
        .expect("law edge curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            cadmpeg_ir::math::Point3::new(1.0, -2.0, 3.0),
            cadmpeg_ir::math::Vector3::new(4.0, 2.0, -1.0)
                .unit()
                .unwrap(),
        )
        .unwrap(),
    ));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less structural-law encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less structural-law round trip");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert_eq!(construction.formula.variables().len(), 3);
    let LawExpression::Edge { curve, .. } = &construction.formula.variables()[2] else {
        panic!("expected round-trip edge law")
    };
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|candidate| candidate.id == curve.id)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
            if curve.degree() == 1
                && curve.knots().as_slice() == [-0.25, -0.25, 1.25, 1.25]
    ));
}

#[test]
fn generated_skin_surface_round_trips_expanded_profiles() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SkinSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(0, true))),
            &DecodeOptions::default(),
        )
        .expect("expanded skin decode");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    let SkinSurfaceLayout::Profiles { profiles, tail, .. } = &construction.layout else {
        panic!("expected expanded skin profiles")
    };
    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].type_code, 9);
    assert_eq!(profiles[0].data.asm_extension, -1);
    assert!(profiles[0].data.pcurve.is_some());
    assert!(profiles[0].data.direction.is_some());
    assert_eq!(*tail, [-1, 7]);
    let profile_curve = profiles[0].curve.clone();

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == profile_curve)
        .expect("skin profile curve")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
        cadmpeg_ir::geometry::analytic::LineCurve::try_new(
            cadmpeg_ir::math::Point3::new(2.0, -1.0, 3.0),
            cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0)
                .unit()
                .unwrap(),
        )
        .unwrap(),
    ));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less expanded skin encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less expanded skin round trip");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        &construction.layout,
        SkinSurfaceLayout::Profiles { profiles, .. }
            if profiles.len() == 1 && profiles[0].data.direction.is_some()
    ));
    let SkinSurfaceLayout::Profiles { profiles, .. } = &construction.layout else {
        unreachable!()
    };
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == profiles[0].curve)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
            if curve.degree() == 1 && curve.knots().as_slice() == [0.0, 0.0, 1.0, 1.0]
    ));
}

#[test]
fn generated_skin_surface_round_trips_fixed_arity_algebraic_laws() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .expect("algebraic skin law decode");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        construction.formula.variables(),
        [
            LawExpression::Algebraic {
                operator,
                operands,
            },
            LawExpression::Algebraic {
                operator: dot,
                operands: vectors,
            }
        ] if operator == "SIN"
            && matches!(operands.as_slice(), [LawExpression::Algebraic { operator, operands }]
                if operator == "ABS"
                    && matches!(operands.as_slice(), [LawExpression::Double { value }] if *value == -2.5))
            && dot == "DOT"
            && matches!(vectors.as_slice(), [LawExpression::Vector { .. }, LawExpression::Vector { .. }])
    ));

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less algebraic skin encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less algebraic skin round trip");
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip skin surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert_eq!(construction.formula.variables().len(), 2);
}

#[test]
fn source_less_writer_rejects_invalid_and_unframed_law_arities() {
    use cadmpeg_ir::geometry::{LawExpression, LawFormula, ProceduralSurfaceDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Skin(definition_payload) = definition else {
            panic!()
        };
        let mut edited_construction = Box::new(definition_payload.construction().to_raw());
        let construction = &mut edited_construction;

        let LawFormula::Named { variables, .. } = &mut construction.formula else {
            panic!()
        };
        variables[0] = LawExpression::Algebraic {
            operator: "SIN".into(),
            operands: Vec::new(),
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = cadmpeg_ir::geometry::surface_payloads::SkinSurfacePayload::try_new(
            edited_construction,
            None,
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("the rebuilt construction states the same legacy cache slot");
    });
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error.to_string().contains("requires 1 operands, got 0"));

    source_less.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Skin(definition_payload) = definition else {
            panic!()
        };
        let mut edited_construction = Box::new(definition_payload.construction().to_raw());
        let construction = &mut edited_construction;

        let LawFormula::Named { variables, .. } = &mut construction.formula else {
            panic!()
        };
        variables[0] = LawExpression::Algebraic {
            operator: "MIN".into(),
            operands: vec![LawExpression::Double { value: 1.0 }],
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = cadmpeg_ir::geometry::surface_payloads::SkinSurfacePayload::try_new(
            edited_construction,
            None,
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("the rebuilt construction states the same legacy cache slot");
    });
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .unwrap_err();
    assert!(error.to_string().contains("unresolved variable arity"));
}

#[test]
fn generated_skin_surface_round_trips_set_compose_rotate_and_term_laws() {
    use cadmpeg_ir::geometry::{LawExpression, LawFormula, ProceduralSurfaceDefinition};
    use cadmpeg_ir::math::Vector3;

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_skin_spl_sur_smbh(2, false))),
            &DecodeOptions::default(),
        )
        .unwrap();
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Skin(definition_payload) = definition else {
            panic!()
        };
        let mut edited_construction = Box::new(definition_payload.construction().to_raw());
        let construction = &mut edited_construction;

        let LawFormula::Named { variables, .. } = &mut construction.formula else {
            panic!()
        };
        *variables = vec![
            LawExpression::Algebraic {
                operator: "SET".into(),
                operands: vec![LawExpression::Double { value: -2.0 }],
            },
            LawExpression::Algebraic {
                operator: "O".into(),
                operands: vec![
                    LawExpression::Algebraic {
                        operator: "ABS".into(),
                        operands: vec![LawExpression::Double { value: -2.5 }],
                    },
                    LawExpression::Algebraic {
                        operator: "SIN".into(),
                        operands: vec![LawExpression::Double { value: 0.25 }],
                    },
                ],
            },
            LawExpression::Algebraic {
                operator: "ROTATE".into(),
                operands: vec![
                    LawExpression::Vector {
                        value: Vector3::new(1.0, 2.0, 3.0),
                    },
                    LawExpression::Transform {
                        scalars: [0.0; 13],
                        enums: [0, 0, 0],
                    },
                ],
            },
            LawExpression::Algebraic {
                operator: "TERM".into(),
                operands: vec![
                    LawExpression::Vector {
                        value: Vector3::new(4.0, 5.0, 6.0),
                    },
                    LawExpression::Integer { value: 1 },
                ],
            },
        ];
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = cadmpeg_ir::geometry::surface_payloads::SkinSurfacePayload::try_new(
            edited_construction,
            None,
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("the rebuilt construction states the same legacy cache slot");
    });

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .unwrap();
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    let ProceduralSurfaceDefinition::Skin(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!()
    };
    let construction = &definition_payload.construction().to_raw();

    assert!(matches!(
        construction.formula.variables(),
        [
            LawExpression::Algebraic { operator: set, operands: set_operands },
            LawExpression::Algebraic { operator: compose, operands: compose_operands },
            LawExpression::Algebraic { operator: rotate, operands: rotate_operands },
            LawExpression::Algebraic { operator: term, operands: term_operands },
        ] if set == "SET" && set_operands.len() == 1
            && compose == "O" && compose_operands.len() == 2
            && rotate == "ROTATE" && rotate_operands.len() == 2
            && term == "TERM" && term_operands.len() == 2
    ));
}
