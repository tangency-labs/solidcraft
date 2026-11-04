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

use cadmpeg_core::convert::f64_from_index;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::smbh_blends_test::synthetic_g2_blend_spl_sur_smbh;
use crate::test_support::smbh_curves_test::synthetic_geometry_with_helix_curve_smbh;
use crate::test_support::smbh_surfaces_test::{
    expected_revision_surface_tail_parameterization,
    synthetic_cacheless_revision_text_law_sweep_smbh, synthetic_comp_spl_sur_smbh,
    synthetic_cyl_spl_sur_smbh, synthetic_explicit_formula_sweep_smbh,
    synthetic_explicit_guide_sweep_smbh, synthetic_explicit_surface_sweep_smbh,
    synthetic_framed_deformable_surface_smbh, synthetic_full_deformable_surface_smbh,
    synthetic_helix_surface_smbh, synthetic_law_driven_sweep_smbh, synthetic_loft_spl_sur_smbh,
    synthetic_minimal_deformable_surface_smbh, synthetic_net_spl_sur_smbh,
    synthetic_off_spl_sur_smbh, synthetic_profile_first_sweep_smbh,
    synthetic_referenced_t_spl_sur_smbh, synthetic_revision_deformable_surface_smbh,
    synthetic_revision_text_law_sweep_smbh, synthetic_rot_spl_sur_smbh,
    synthetic_ruled_spl_sur_smbh, synthetic_scaled_compound_loft_smbh, synthetic_skin_spl_sur_smbh,
    synthetic_sum_spl_sur_smbh, synthetic_surface_curve_deformable_smbh,
    synthetic_taper_spl_sur_smbh, synthetic_text_law_driven_sweep_smbh,
};
use crate::test_support::tokens_test::renamed_generated_subtype;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

#[test]
fn generated_source_less_rejects_duplicate_procedural_surface_owners() {
    for (smbh, label) in [
        (synthetic_cyl_spl_sur_smbh(), "cached"),
        (synthetic_helix_surface_smbh(true), "cacheless"),
    ] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("generated {label} surface decode: {error}"));
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        let duplicate = source_less.model.procedural_surfaces[0].clone();
        source_less.model.procedural_surfaces.push(duplicate);

        let error = F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains("multiple procedural constructions"),
            "unexpected {label} duplicate-owner error: {error}"
        );
    }
}

#[test]
fn generated_source_less_refuses_procedural_construction_loss_on_analytic_carriers() {
    use cadmpeg_ir::geometry::{
        CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_cyl_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated procedural surface decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let surface_id = source_less
        .model
        .procedural_surface_owner(&source_less.model.procedural_surfaces[0].id)
        .expect("procedural surface owner")
        .clone();
    source_less
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == surface_id)
        .unwrap()
        .geometry = SurfaceGeometry::Procedural {
        construction: source_less.model.procedural_surfaces[0].id.clone(),
        cache: Some(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
    };
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("analytic carrier must not discard its procedural surface");
    assert!(error
        .to_string()
        .contains("cannot retain its construction on analytic carrier"));

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated procedural curve decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let curve_id = source_less
        .model
        .procedural_curve_owner(&source_less.model.procedural_curves[0].id)
        .expect("procedural curve owner")
        .clone();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .unwrap()
        .geometry = CurveGeometry::Procedural {
        construction: source_less.model.procedural_curves[0].id.clone(),
        cache: Some(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
    };
    let error = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("analytic carrier must not discard its procedural curve");
    assert!(error
        .to_string()
        .contains("cannot retain its construction on carrier"));
}

#[test]
fn generated_minimal_deformable_surface_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_minimal_deformable_surface_smbh())),
            &DecodeOptions::default(),
        )
        .expect("deformable surface decode");
    let ProceduralSurfaceDefinition::Deformable(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected deformable surface")
    };
    let construction = &definition_payload.construction().to_raw();

    let DeformableSurfaceData::Minimal { vectors, selector } = &construction.data else {
        panic!("expected minimal deformable surface")
    };
    assert_eq!(vectors[2].z, 1.0);
    assert_eq!(*selector, 0);
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
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::Deformable(..)
    ));
}

#[test]
fn generated_framed_deformable_surfaces_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    for mode in [1, 3] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_framed_deformable_surface_smbh(
                    mode,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!("expected deformable surface")
        };
        let construction = &definition_payload.construction().to_raw();

        match &construction.data {
            DeformableSurfaceData::Plain {
                frame,
                parameter_triples,
            } => {
                assert_eq!(mode, 1);
                assert_eq!(frame.point.z, 60.0);
                assert_eq!(parameter_triples.len(), 2);
            }
            DeformableSurfaceData::Guided {
                frame,
                guide_parameter,
                ..
            } => {
                assert_eq!(mode, 3);
                assert_eq!(frame.point.z, 60.0);
                assert_eq!(*guide_parameter, 0.9);
            }
            DeformableSurfaceData::Minimal { .. }
            | DeformableSurfaceData::SurfaceCurve { .. }
            | DeformableSurfaceData::Full { .. }
            | DeformableSurfaceData::RevisionMode3 { .. } => {
                panic!("wrong mode")
            }
        }
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
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Deformable(..)
        ));
    }
}

#[test]
fn generated_revision_deformable_mode3_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_revision_deformable_surface_smbh())),
            &DecodeOptions::default(),
        )
        .expect("revision deformable surface decode");
    let ProceduralSurfaceDefinition::Deformable(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected deformable surface")
    };
    let construction = &definition_payload.construction().to_raw();

    let revision_form = construction.cache.form().expect("revision deformable form");
    assert_eq!(revision_form.revision.get(), 22_506);
    assert_eq!(
        revision_form.support_bounds,
        [Some(0.0), Some(1.0), Some(0.0), Some(1.0)]
    );
    let DeformableSurfaceData::RevisionMode3 {
        leading_parameter,
        trailing_point,
        parameters,
        trailing_value,
        ..
    } = &construction.data
    else {
        panic!("expected revision mode-3 data")
    };
    assert_eq!(*leading_parameter, 2.5);
    assert_eq!(
        *trailing_point,
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert_eq!(*parameters, [4.5, 5.5, 6.5]);
    assert_eq!(*trailing_value, 19);

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("revision deformable source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("revision deformable source-less round trip");
    let ProceduralSurfaceDefinition::Deformable(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip deformable surface")
    };
    let construction = &definition_payload.construction().to_raw();

    assert_eq!(
        construction
            .cache
            .form()
            .expect("round-trip revision form")
            .support_bounds,
        [Some(0.0), Some(1.0), Some(0.0), Some(1.0)]
    );
    assert!(matches!(
        construction.data,
        DeformableSurfaceData::RevisionMode3 {
            trailing_value: 19,
            ..
        }
    ));
}

#[test]
fn generated_surface_curve_deformable_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_surface_curve_deformable_smbh())),
            &DecodeOptions::default(),
        )
        .unwrap();
    let ProceduralSurfaceDefinition::Deformable(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!()
    };
    let construction = &definition_payload.construction().to_raw();

    let DeformableSurfaceData::SurfaceCurve {
        native_id,
        first_parameter,
        selector,
        second_parameter,
        curve,
        parameter_triples,
        ..
    } = &construction.data
    else {
        panic!()
    };
    assert_eq!((*native_id, *selector), (42, 3));
    assert_eq!(parameter_triples, &[[0.1, 0.2, 0.3]]);
    let curve = curve.clone();
    let range = [*first_parameter, *second_parameter];
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less
        .model
        .curves
        .iter_mut()
        .find(|candidate| candidate.id == curve)
        .expect("surface-curve deformable curve")
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
        .unwrap();
    let round = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .unwrap();
    assert!(matches!(
        round.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::Deformable(..)
    ));
    assert!(round.ir().model.curves.iter().any(|curve| matches!(
        &curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))
                    if curve.degree() == 1
                        && curve.knots().as_slice() == [range[0], range[0], range[1], range[1]]
    )));
}

#[test]
fn generated_full_deformable_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{DeformableSurfaceData, ProceduralSurfaceDefinition};
    for expected_version_value in [None, Some(226)] {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_full_deformable_surface_smbh(
                    expected_version_value,
                ))),
                &DecodeOptions::default(),
            )
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable(definition_payload) =
            &decoded.ir().model.procedural_surfaces[0].definition()
        else {
            panic!()
        };
        let construction = &definition_payload.construction().to_raw();

        let DeformableSurfaceData::Full {
            selector,
            native_id,
            first_parameter,
            version_value,
            second_parameter,
            curve,
            frames,
            trailing_value,
            ..
        } = &construction.data
        else {
            panic!()
        };
        assert_eq!((*selector, *native_id), (7, 42));
        assert_eq!(*version_value, expected_version_value);
        assert_eq!(frames[0].parameter, 0.4);
        assert_eq!(frames[1].parameter, 0.5);
        assert_eq!(*trailing_value, 99);
        let curve = curve.clone();
        let range = [*first_parameter, *second_parameter];
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less
            .model
            .curves
            .iter_mut()
            .find(|candidate| candidate.id == curve)
            .expect("full deformable curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(-1.0, 2.0, 3.0),
                cadmpeg_ir::math::Vector3::new(3.0, -4.0, 2.0)
                    .unit()
                    .unwrap(),
            )
            .unwrap(),
        ));
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .unwrap();
        let round = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap();
        let ProceduralSurfaceDefinition::Deformable(definition_payload) =
            &round.ir().model.procedural_surfaces[0].definition()
        else {
            panic!()
        };
        let construction = &definition_payload.construction().to_raw();

        assert!(matches!(
            construction.data,
            DeformableSurfaceData::Full { version_value, .. }
                if version_value == expected_version_value
        ));
        assert!(round.ir().model.curves.iter().any(|curve| matches!(
            &curve.geometry,
            cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve))
                    if curve.degree() == 1
                        && curve.knots().as_slice() == [range[0], range[0], range[1], range[1]]
        )));
    }
}

#[test]
fn generated_t_spline_surface_resolves_shared_subtransform_source_less() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, TSplineSubtransform};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_referenced_t_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("referenced T-spline decode");
    let ProceduralSurfaceDefinition::TSpline { construction } =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected T-spline surface")
    };
    let TSplineSubtransform::Resolved {
        transform: resolved,
        ..
    } = construction.subtransform()
    else {
        panic!("expected resolved T-spline reference")
    };
    assert_eq!(resolved.program.as_str(), "degree 3\nv 1 0 0 0\n");
    assert_eq!(resolved.values.as_str(), "100verts 1\n");

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less referenced T-spline encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less referenced T-spline round trip");
    let ProceduralSurfaceDefinition::TSpline { construction } =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip T-spline surface")
    };
    assert!(matches!(
        construction.subtransform(),
        TSplineSubtransform::Inline(_)
    ));
}

#[test]
fn generated_explicit_formula_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_formula_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit formula sweep decode");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected native sweep")
    };
    let (profile, spine, Some(native)) = (
        definition_payload.profile(),
        definition_payload.spine(),
        definition_payload
            .native()
            .as_deref()
            .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),
    ) else {
        panic!("expected native sweep")
    };

    let SweepSurfaceLayout::ExplicitFormula {
        mode,
        profile_range,
        profile_frame,
        origin,
        path_range,
        formula,
        ..
    } = &native.layout
    else {
        panic!("expected explicit formula sweep")
    };
    assert_eq!(*mode, 7);
    assert_eq!(*profile_range, [-0.5, 1.5]);
    assert_eq!(profile_frame.as_ref().unwrap().0.z, 30.0);
    assert_eq!(origin.z, 60.0);
    assert_eq!(*path_range, [-20.0, 30.0]);
    assert!(matches!(formula, cadmpeg_ir::geometry::LawFormula::Null {}));
    let profile = profile.clone();
    let spine = spine.clone();

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    for (ordinal, curve_id) in [&profile, &spine].into_iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(
                    f64_from_index(ordinal).expect("fixture index is exact in f64"),
                    2.0,
                    -1.0,
                ),
                cadmpeg_ir::math::Vector3::new(3.0, -2.0, 4.0)
                    .unit()
                    .unwrap(),
            )
            .unwrap(),
        ));
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less explicit formula sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit formula sweep round trip");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip explicit formula sweep")
    };
    let (profile, spine, Some(native)) = (
        definition_payload.profile(),
        definition_payload.spine(),
        definition_payload
            .native()
            .as_deref()
            .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),
    ) else {
        panic!("expected round-trip explicit formula sweep")
    };

    assert!(matches!(
        native.layout,
        SweepSurfaceLayout::ExplicitFormula { .. }
    ));
    for (curve_id, knots) in [
        (profile, [-0.5, -0.5, 1.5, 1.5]),
        (spine, [-2.0, -2.0, 3.0, 3.0]),
    ] {
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == *curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
            if curve.degree() == 1 && curve.knots().as_slice() == knots
        ));
    }
}

#[test]
fn generated_source_less_sweep_refuses_missing_native_graph() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let mut decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_formula_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated native sweep decode")
        .into_parts()
        .0;
    decoded.source = None;
    decoded
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    decoded.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Sweep(definition_payload) = definition else {
            panic!("expected generated sweep")
        };
        let mut edited_native = definition_payload
            .native()
            .as_deref()
            .map(|native| Box::new(native.to_raw()));
        let native = &mut edited_native;

        *native = None;
        *definition_payload = cadmpeg_ir::geometry::surface_payloads::SweepSurfacePayload::try_new(
            definition_payload.profile().clone(),
            definition_payload.spine().clone(),
            edited_native,
        )
        .unwrap();
    });

    let error = F3dCodec
        .plan(EncodeInput::new(&decoded, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut Vec::new()))
        .expect_err("a sweep without its native graph must not be guessed");
    assert!(matches!(
        error,
        cadmpeg_core::CodecError::NotImplemented(message)
            if message.contains("lacks its native construction graph")
    ));
}

#[test]
fn generated_explicit_guide_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_guide_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit guide sweep decode");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected native sweep")
    };
    let (profile, spine, Some(native)) = (
        definition_payload.profile(),
        definition_payload.spine(),
        definition_payload
            .native()
            .as_deref()
            .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),
    ) else {
        panic!("expected native sweep")
    };

    let SweepSurfaceLayout::ExplicitGuide {
        mode,
        profile_range,
        profile_frame,
        path_range,
        guide_curve,
        guide_range,
        guide_modes,
        guide_parameters,
        trailing_flags,
        ..
    } = &native.layout
    else {
        panic!("expected explicit guide sweep")
    };
    assert_eq!(*mode, 8);
    assert!(profile_frame.is_none());
    assert_eq!(*guide_range, [0.0, 1.0]);
    assert_eq!(*guide_modes, [11, 12]);
    assert_eq!(guide_parameters[5], 0.6);
    assert_eq!(*trailing_flags, [true, false, true]);
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), [path_range[0] / 10.0, path_range[1] / 10.0]),
        (guide_curve.clone(), *guide_range),
    ];

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit guide sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(
                    f64_from_index(ordinal).expect("fixture index is exact in f64"),
                    -2.0,
                    1.0,
                ),
                cadmpeg_ir::math::Vector3::new(2.0, 4.0, -3.0)
                    .unit()
                    .unwrap(),
            )
            .unwrap(),
        ));
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less explicit guide sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit guide sweep round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::Sweep(definition_payload) if matches!((definition_payload.native(),), (Some(native),) if matches!(native.layout, SweepSurfaceLayout::ExplicitGuide { .. }))));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
                    if curve.degree() == 1
                        && curve.knots().as_slice() == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_explicit_surface_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_explicit_surface_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("explicit surface sweep decode");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected native sweep")
    };
    let (profile, spine, Some(native)) = (
        definition_payload.profile(),
        definition_payload.spine(),
        definition_payload
            .native()
            .as_deref()
            .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),
    ) else {
        panic!("expected native sweep")
    };

    let SweepSurfaceLayout::ExplicitSurface {
        mode,
        profile_range,
        path_range,
        singularity,
        auxiliary_curve,
        support_flag,
        legacy_flag,
        ..
    } = &native.layout
    else {
        panic!("expected explicit surface sweep")
    };
    assert_eq!((*mode, *singularity), (9, 1));
    assert!(auxiliary_curve.is_some());
    assert!(*support_flag);
    assert_eq!(*legacy_flag, Some(false));
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), [path_range[0] / 10.0, path_range[1] / 10.0]),
    ];

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("explicit surface sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(
                    f64_from_index(ordinal).expect("fixture index is exact in f64"),
                    1.0,
                    -2.0,
                ),
                cadmpeg_ir::math::Vector3::new(4.0, 2.0, -3.0)
                    .unit()
                    .unwrap(),
            )
            .unwrap(),
        ));
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less explicit surface sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less explicit surface sweep round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::Sweep(definition_payload) if matches!((definition_payload.native(),), (Some(native),) if matches!(native.layout, SweepSurfaceLayout::ExplicitSurface { .. }))));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
                    if curve.degree() == 1
                        && curve.knots().as_slice() == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_law_driven_sweep_decodes_and_writes_full_graph() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_law_driven_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("law-driven sweep decode");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected native sweep")
    };
    let (profile, spine, Some(native)) = (
        definition_payload.profile(),
        definition_payload.spine(),
        definition_payload
            .native()
            .as_deref()
            .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),
    ) else {
        panic!("expected native sweep")
    };

    let SweepSurfaceLayout::LawDriven {
        mode,
        profile_range,
        first_law,
        first_mode,
        second_law,
        formula_mode,
        formula,
        path_range,
        ..
    } = &native.layout
    else {
        panic!("expected law-driven sweep")
    };
    assert_eq!((*mode, *first_mode, *formula_mode), (10, 21, 23));
    assert!(matches!(first_law.as_ref(), LawExpression::Double { value } if *value == 2.5));
    assert!(matches!(second_law.as_ref(), LawExpression::Vector { value } if value.z == 3.0));
    assert!(matches!(formula, cadmpeg_ir::geometry::LawFormula::Null {}));
    let bounded_curves = [
        (profile.clone(), *profile_range),
        (spine.clone(), *path_range),
    ];

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    for (ordinal, (curve_id, _)) in bounded_curves.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == *curve_id)
            .expect("law-driven sweep curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(
                    f64_from_index(ordinal).expect("fixture index is exact in f64"),
                    -1.0,
                    2.0,
                ),
                cadmpeg_ir::math::Vector3::new(3.0, 4.0, -2.0)
                    .unit()
                    .unwrap(),
            )
            .unwrap(),
        ));
    }
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less law-driven sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law-driven sweep round trip");
    assert!(matches!(
        &round_trip.ir().model.procedural_surfaces[0].definition(), ProceduralSurfaceDefinition::Sweep(definition_payload) if matches!((definition_payload.native(),), (Some(native),) if matches!(native.layout, SweepSurfaceLayout::LawDriven { .. }))));
    for (curve_id, range) in bounded_curves {
        assert!(matches!(
            round_trip
                .ir()
                .model
                .curves
                .iter()
                .find(|curve| curve.id == curve_id)
                .map(|curve| &curve.geometry),
            Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
                    if curve.degree() == 1
                        && curve.knots().as_slice() == [range[0], range[0], range[1], range[1]]
        ));
    }
}

#[test]
fn generated_text_law_driven_sweep_preserves_expression_tokens() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_text_law_driven_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("text-law sweep decode");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected native sweep")
    };
    let (Some(native),) = (definition_payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),)
    else {
        panic!("expected native sweep")
    };

    let SweepSurfaceLayout::LawDriven {
        first_law,
        second_law,
        ..
    } = &native.layout
    else {
        panic!("expected law-driven sweep")
    };
    assert!(matches!(
        first_law.as_ref(),
        LawExpression::Text { value } if value == "0.008726867790758789*X"
    ));
    assert!(matches!(
        second_law.as_ref(),
        LawExpression::Text { value } if value == "VEC(1,1,1)"
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
        .expect("text-law sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("text-law sweep round trip");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-tripped native sweep")
    };
    let (Some(native),) = (definition_payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),)
    else {
        panic!("expected round-tripped native sweep")
    };

    let SweepSurfaceLayout::LawDriven {
        first_law,
        second_law,
        ..
    } = &native.layout
    else {
        panic!("expected round-tripped law-driven sweep")
    };
    assert!(matches!(
        first_law.as_ref(),
        LawExpression::Text { value } if value == "0.008726867790758789*X"
    ));
    assert!(matches!(
        second_law.as_ref(),
        LawExpression::Text { value } if value == "VEC(1,1,1)"
    ));
}

#[test]
fn generated_revision_text_law_sweep_decodes_and_round_trips() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_revision_text_law_sweep_smbh())),
            &DecodeOptions::default(),
        )
        .expect("revision text-law sweep decode");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &decoded.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected native revision sweep")
    };
    let (Some(native),) = (definition_payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),)
    else {
        panic!("expected native revision sweep")
    };

    assert_eq!(native.cache.form().unwrap().revision.get(), 23100);
    let SweepSurfaceLayout::LawDriven {
        first_law,
        second_law,
        formula,
        ..
    } = &native.layout
    else {
        panic!("expected revision law-driven sweep")
    };
    assert!(matches!(
        first_law.as_ref(),
        LawExpression::Text { value } if value == "0.008726867790758789*X"
    ));
    assert!(matches!(
        second_law.as_ref(),
        LawExpression::Text { value } if value == "VEC(1,1,1)"
    ));
    assert!(
        matches!(formula, cadmpeg_ir::geometry::LawFormula::Named { name, .. }
        if name.as_str() == "ROTATE(DOMAIN(VEC(1,0,0),0,0.8),TRANS1)")
    );
    assert!(matches!(
        formula.variables(),
        [LawExpression::TransformVec { .. }]
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
        .expect("revision text-law sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("revision text-law sweep round trip");
    let ProceduralSurfaceDefinition::Sweep(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-tripped revision sweep")
    };
    let (Some(native),) = (definition_payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),)
    else {
        panic!("expected round-tripped revision sweep")
    };

    assert_eq!(native.cache.form().unwrap().revision.get(), 23100);
    assert!(matches!(
        native.layout,
        SweepSurfaceLayout::LawDriven {
            first_law: ref first,
            second_law: ref second,
            ..
        } if matches!(first.as_ref(), LawExpression::Text { value } if value == "0.008726867790758789*X")
            && matches!(second.as_ref(), LawExpression::Text { value } if value == "VEC(1,1,1)")
    ));
}

#[test]
fn generated_cacheless_revision_text_law_sweep_preserves_parameterization() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralSurfaceDefinition, SweepSurfaceLayout};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_cacheless_revision_text_law_sweep_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("cacheless revision text-law sweep decode");
    let procedural = &decoded.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let ProceduralSurfaceDefinition::Sweep(definition_payload) = procedural.definition() else {
        panic!("expected cacheless native revision sweep")
    };
    let (Some(native),) = (definition_payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),)
    else {
        panic!("expected cacheless native revision sweep")
    };

    let form = native.cache.form().expect("revision form");
    assert_eq!(form.revision.get(), 23100);
    assert!(form.cache.parameterization().is_some());
    assert_eq!(
        form.cache.parameterization(),
        Some(&expected_revision_surface_tail_parameterization())
    );
    let SweepSurfaceLayout::LawDriven {
        first_law,
        second_law,
        ..
    } = &native.layout
    else {
        panic!("expected cacheless law-driven sweep")
    };
    assert!(matches!(
        first_law.as_ref(),
        LawExpression::Text { value } if value == "0.008726867790758789*X"
    ));
    assert!(matches!(
        second_law.as_ref(),
        LawExpression::Text { value } if value == "VEC(1,1,1)"
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
        .expect("cacheless revision text-law sweep encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("cacheless revision text-law sweep round trip");
    let procedural = &round_trip.ir().model.procedural_surfaces[0];
    assert_eq!(procedural.cache_fit_tolerance(), None);
    let ProceduralSurfaceDefinition::Sweep(definition_payload) = procedural.definition() else {
        panic!("expected round-tripped cacheless native revision sweep")
    };
    let (Some(native),) = (definition_payload
        .native()
        .as_deref()
        .map(cadmpeg_ir::geometry::SweepSurfaceConstruction::to_raw),)
    else {
        panic!("expected round-tripped cacheless native revision sweep")
    };

    assert!(native
        .cache
        .form()
        .unwrap()
        .cache
        .parameterization()
        .is_some());
    assert_eq!(
        native.cache.form().unwrap().cache.parameterization(),
        Some(&expected_revision_surface_tail_parameterization())
    );
}

#[test]
fn generated_legacy_surface_names_select_modern_layouts() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let cases = [
        (
            renamed_generated_subtype(
                synthetic_skin_spl_sur_smbh(0, false),
                "skin_spl_sur",
                "skinsur",
            ),
            "skin",
        ),
        (
            renamed_generated_subtype(synthetic_net_spl_sur_smbh(), "net_spl_sur", "netsur"),
            "net",
        ),
        (
            renamed_generated_subtype(
                synthetic_profile_first_sweep_smbh(),
                "sweep_spl_sur",
                "sweepsur",
            ),
            "sweep",
        ),
        (
            renamed_generated_subtype(
                synthetic_scaled_compound_loft_smbh(true),
                "scaled_cloft_spl_sur",
                "sclclftsur",
            ),
            "scaled_compound_loft",
        ),
        (
            renamed_generated_subtype(synthetic_cyl_spl_sur_smbh(), "cyl_spl_sur", "cylsur"),
            "extrusion",
        ),
    ];
    for (smbh, expected) in cases {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{expected} legacy decode: {error}"));
        let definition = &decoded.ir().model.procedural_surfaces[0].definition();
        assert!(
            matches!(
                (expected, definition),
                ("skin", ProceduralSurfaceDefinition::Skin(..))
                    | ("net", ProceduralSurfaceDefinition::Net(..))
                    | ("sweep", ProceduralSurfaceDefinition::Sweep(..))
                    | (
                        "scaled_compound_loft",
                        ProceduralSurfaceDefinition::ScaledCompoundLoft(..)
                    )
                    | ("extrusion", ProceduralSurfaceDefinition::Extrusion(_))
            ),
            "wrong definition for {expected}: {definition:?}"
        );
    }
}

#[test]
fn generated_procedural_surface_tolerance_presence_matches_native_grammar() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let required = [
        (synthetic_skin_spl_sur_smbh(0, false), "skin surface"),
        (synthetic_net_spl_sur_smbh(), "net surface"),
    ];
    for (smbh, family) in required {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} decode: {error}"));
        assert!(decoded.ir().model.procedural_surfaces[0]
            .cache_fit_tolerance()
            .is_some());
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less.model.procedural_surfaces[0]
            .set_cache_fit_tolerance(None)
            .unwrap();
        let error = F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut Vec::new()))
            .unwrap_err();
        assert!(
            error
                .to_string()
                .contains(&format!("{family} requires a native cache-fit tolerance")),
            "unexpected {family} error: {error}"
        );
    }

    let optional = [
        (synthetic_comp_spl_sur_smbh(), "compound"),
        (synthetic_taper_spl_sur_smbh("taper_spl_sur"), "taper"),
        (synthetic_ruled_spl_sur_smbh("rule_sur", true), "ruled"),
        (synthetic_sum_spl_sur_smbh("sum_spl_sur", true), "sum"),
        (synthetic_rot_spl_sur_smbh("rot_spl_sur"), "revolution"),
        (synthetic_off_spl_sur_smbh("off_spl_sur"), "offset"),
        (synthetic_cyl_spl_sur_smbh(), "extrusion"),
        (
            synthetic_g2_blend_spl_sur_smbh("g2_blend_spl_sur", false),
            "G2 blend",
        ),
    ];
    for (smbh, family) in optional {
        let decoded = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("optional-tolerance surface decode");
        let (mut source_less, _, _) = decoded.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();
        source_less.model.procedural_surfaces[0]
            .set_cache_fit_tolerance(None)
            .unwrap();
        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less surface without optional tolerance");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .unwrap_or_else(|error| panic!("{family} round trip: {error}"));
        assert_eq!(
            round_trip.ir().model.procedural_surfaces.len(),
            1,
            "{family} procedural surface was not reconstructed"
        );
        assert_eq!(
            round_trip.ir().model.procedural_surfaces[0].cache_fit_tolerance(),
            None
        );
    }

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_loft_spl_sur_smbh("loft_spl_sur"))),
            &DecodeOptions::default(),
        )
        .expect("loft decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    source_less.model.procedural_surfaces[0]
        .set_cache_fit_tolerance(None)
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less loft without optional tolerance");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less loft round trip");
    assert!(matches!(
        round_trip.ir().model.procedural_surfaces[0].definition(),
        ProceduralSurfaceDefinition::Loft(..)
    ));
    assert_eq!(
        round_trip.ir().model.procedural_surfaces[0].cache_fit_tolerance(),
        None
    );
}
