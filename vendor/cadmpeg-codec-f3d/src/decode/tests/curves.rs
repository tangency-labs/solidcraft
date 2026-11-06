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

use cadmpeg_test_support::{edit, EditableDecodeResult};

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::{Cursor, Read};

use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::smbh_curves_test::{
    synthetic_geometry_with_analytic_offset_supports_smbh,
    synthetic_geometry_with_cacheless_helix_curve_smbh,
    synthetic_geometry_with_compound_curve_smbh,
    synthetic_geometry_with_consistent_exact_curve_smbh,
    synthetic_geometry_with_decoy_curve_sense_smbh,
    synthetic_geometry_with_embedded_offset_supports_smbh,
    synthetic_geometry_with_helix_curve_smbh, synthetic_geometry_with_law_curve_smbh,
    synthetic_geometry_with_procedural_curve_smbh, synthetic_geometry_with_projection_smbh,
    synthetic_geometry_with_silhouette_smbh, synthetic_geometry_with_spring_smbh,
    synthetic_geometry_with_subset_curve_smbh, synthetic_geometry_with_surface_curve_smbh,
    synthetic_geometry_with_surface_intersection_smbh, synthetic_geometry_with_surface_offset_smbh,
    synthetic_geometry_with_two_sided_offset_curve_smbh,
    synthetic_geometry_with_vector_offset_curve_smbh, with_legacy_subtype,
};
use crate::test_support::smbh_surfaces_test::{
    synthetic_exact_spl_sur_smbh, synthetic_exact_spl_sur_with_decoy_sense_smbh,
};
use crate::test_support::tokens_test::{push_tagged_i64, t_dbl};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::geometry::SolvedCurveGeometry;

const EPS_CONE_ANGLE: f64 = 1.0e-12;

#[test]
fn transform_decodes_column_major_basis_and_scaled_translation() {
    use cadmpeg_asm::sab::{Record, Token};

    let record = Record {
        index: 0,
        name: "transform".into(),

        tokens: vec![
            Token::Vector3([1.0, 0.0, 0.0]),
            Token::Vector3([0.0, 1.0, 0.0]),
            Token::Vector3([0.0, 0.0, 1.0]),
            Token::Position([1.0, 2.0, 3.0]),
            Token::Double(1.0),
        ]
        .into(),
        offset: 0,
        len: 0,
    };
    let transform = cadmpeg_asm::brep::attributes::decode_transform(&record, 60.0).unwrap();
    assert_eq!(transform.rows()[0], [1.0, 0.0, 0.0, 600.0]);
    assert_eq!(transform.rows()[1], [0.0, 1.0, 0.0, 1200.0]);
    assert_eq!(transform.rows()[2], [0.0, 0.0, 1.0, 1800.0]);
    assert_eq!(transform.rows()[3], [0.0, 0.0, 0.0, 1.0]);
}

#[test]
fn nurbs_curve_block_decodes_to_carrier() {
    use cadmpeg_asm::nurbs::core::decode_curve_cache;

    // A degree-2 nubs curve with two unique knots at stored multiplicity 2:
    // sum(mults) 4, n_poles = 4 - (degree - 1) = 3.
    let mut b = Vec::new();
    b.extend_from_slice(b"\x0d\x04nubs");
    push_tagged_i64(&mut b, 0x04, 2); // degree
    push_tagged_i64(&mut b, 0x15, 0); // closure = open
    push_tagged_i64(&mut b, 0x04, 2); // n_unique_knots
    for (k, m) in [(0.0, 2i64), (1.0, 2)] {
        t_dbl(&mut b, k);
        push_tagged_i64(&mut b, 0x04, m);
    }
    for p in [[0.0, 0.0, 0.0], [1.0, 2.0, 0.0], [2.0, 0.0, 0.0]] {
        for c in p {
            t_dbl(&mut b, c);
        }
    }

    let c = decode_curve_cache(&b).expect("curve block decodes");
    assert_eq!(c.degree(), 2);
    assert_eq!(c.control_points().len(), 3);
    // Clamped knots: [0,0,0,1,1,1] (endpoint mult 2 + 1 = 3 each).
    assert_eq!(c.knots().as_slice(), [0.0, 0.0, 0.0, 1.0, 1.0, 1.0]);
    assert_eq!(c.control_points()[1].x, 10.0);
    assert_eq!(c.control_points()[1].y, 20.0);
    assert!(c.weights().is_none());
}

#[test]
fn decode_retains_generated_procedural_curve_fit_contract() {
    let f3d = f3d_with_smbh(&synthetic_geometry_with_procedural_curve_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir().model.procedural_curves.first().unwrap();
    assert!(matches!(
        procedural.definition(),
        cadmpeg_ir::geometry::ProceduralCurveDefinition::Unknown {
            native_kind: Some(native_kind),
            record: None,
        ..
    } if native_kind == "surf_surf_int_cur"
    ));
    assert_eq!(
        procedural
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.005)
    );
    assert_eq!(result.ir().model.curves.len(), 1);
}

#[test]
fn decode_retains_generated_helix_construction() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;
    use cadmpeg_ir::math::Point3;

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_helix_curve_smbh())),
                &DecodeOptions::default(),
            )
            .expect("generated helix decode"),
    );
    let procedural = result
        .ir()
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    let ProceduralCurveDefinition::Helix(helix_payload) = procedural.definition() else {
        panic!("expected helix construction")
    };
    let angle_range = helix_payload.angle_range();
    let center = helix_payload.center().as_raw();
    let major = helix_payload.major();
    let minor = helix_payload.minor();
    let pitch = helix_payload.pitch();
    let apex_factor = helix_payload.apex_factor();
    let axis = helix_payload.axis();

    assert_eq!(angle_range.get(), [0.0, std::f64::consts::TAU]);
    assert_eq!(*center, Point3::new(10.0, 20.0, 30.0));
    assert_eq!(*major, cadmpeg_ir::math::Vector3::new(20.0, 0.0, 0.0));
    assert_eq!(*minor, cadmpeg_ir::math::Vector3::new(0.0, 20.0, 0.0));
    assert_eq!(*pitch, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 40.0));
    assert_eq!(apex_factor.get(), 0.25);
    assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
    assert_eq!(
        procedural
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.005)
    );

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].replace_definition(ProceduralCurveDefinition::Helix(
        cadmpeg_ir::geometry::HelixCurveConstruction::try_new(
            [-1.0, 7.0],
            cadmpeg_ir::geometry::HelixFrame {
                center: Point3::new(12.0, 23.0, 34.0),
                major: cadmpeg_ir::math::Vector3::new(30.0, 0.0, 0.0),
                minor: cadmpeg_ir::math::Vector3::new(0.0, -30.0, 0.0),
                pitch: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 55.0),
                axis: cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0),
            },
            0.5,
            None,
        )
        .unwrap(),
    ));
    edited.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.012))
        .unwrap();
    let solved_curve_id = edited
        .model
        .procedural_curve_owner(&edited.model.procedural_curves[0].id)
        .expect("helix solved curve owner")
        .clone();
    let solved_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == solved_curve_id)
        .expect("helix solved curve");
    let cadmpeg_ir::geometry::CurveGeometry::Procedural {
        cache: Some(solved_cache),
        ..
    } = &mut solved_curve.geometry
    else {
        panic!("expected procedural helix carrier")
    };
    let SolvedCurveGeometry::Nurbs(mut edited_cache) = solved_cache.clone() else {
        panic!("expected helix NURBS cache")
    };
    edited_cache
        .try_map_control_points(|index, point| {
            let mut point = point.get();
            if index == 1 {
                point.x = 17.0;
                point.z = -2.0;
            }
            cadmpeg_ir::features::FinitePoint3::new(point).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        })
        .expect("pole edit admission");
    *solved_cache = SolvedCurveGeometry::Nurbs(edited_cache);
    let edited_definition = edited.model.procedural_curves[0].definition().clone();
    let edited_cache = solved_curve.geometry.clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("helix definition regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated helix decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &edited_definition
    );
    assert_eq!(
        regenerated.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.012)
    );
    assert!(regenerated
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == edited_cache));

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let expected = source_less.model.procedural_curves[0].definition().clone();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less helix encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less helix round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &expected
    );
    assert_eq!(
        round_trip.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.005)
    );
}

#[test]
fn cacheless_helix_construction_is_the_exact_edge_carrier() {
    use cadmpeg_ir::geometry::{CurveGeometry, ProceduralCurveDefinition};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_cacheless_helix_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("cacheless helix decode");
    let procedural = result
        .ir()
        .model
        .procedural_curves
        .first()
        .expect("helix construction");
    assert!(matches!(
        procedural.definition(),
        ProceduralCurveDefinition::Helix(_)
    ));
    assert_eq!(procedural.cache_fit_tolerance(), None);
    assert!(matches!(
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| {
                result.ir().model.procedural_curve_owner(&procedural.id) == Some(&curve.id)
            })
            .map(|curve| &curve.geometry),
        Some(CurveGeometry::Procedural { construction, .. }) if *construction == procedural.id
    ));
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "validation findings: {:?}",
        validation.findings
    );
    assert!(result
        .report()
        .losses
        .iter()
        .all(|loss| !loss.message.contains("procedural intcurve")));

    let expected = procedural.definition().clone();
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("cacheless helix source-less encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("cacheless helix source-less round trip");
    assert!(matches!(
        round_trip.ir().model.curves[0].geometry,
        CurveGeometry::Procedural { .. }
    ));
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &expected
    );
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].cache_fit_tolerance(),
        None
    );
}

#[test]
fn generated_law_intcurve_decodes_and_writes_recursive_formulas() {
    use cadmpeg_ir::geometry::{LawExpression, ProceduralCurveDefinition};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_law_curve_smbh())),
            &DecodeOptions::default(),
        )
        .expect("law intcurve decode");
    let procedural = decoded
        .ir()
        .model
        .procedural_curves
        .iter()
        .find(|curve| matches!(curve.definition(), ProceduralCurveDefinition::Law { .. }))
        .expect("law intcurve construction");
    let ProceduralCurveDefinition::Law {
        context,
        extension,
        primary,
        additional,
        ..
    } = procedural.definition()
    else {
        unreachable!()
    };
    assert_eq!(context.parameter_range().endpoints(), [-1.0, 2.0]);
    assert_eq!(*extension, 0);
    assert!(
        matches!(primary.formula(), cadmpeg_ir::geometry::LawFormula::Named { name, .. } if name.as_str() == "primary_law")
    );
    assert!(matches!(
        primary.formula().variables()[0],
        LawExpression::Edge { parameters, .. }
            if cadmpeg_ir::scalar::FiniteReal::raw_array(parameters) == [-0.5, 1.5]
    ));
    assert_eq!(additional.len(), 2);

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less law intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less law intcurve round trip");
    assert!(round_trip.ir().model.procedural_curves.iter().any(|curve| {
        matches!(
            curve.definition(),
            ProceduralCurveDefinition::Law { primary, .. }
                if matches!(primary.formula().variables()[0], LawExpression::Edge { .. })
        )
    }));
}

#[test]
fn generated_vector_offset_curve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, VectorOffsetRoles};

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_vector_offset_curve_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("generated vector-offset decode"),
    );
    let procedural = &result.ir().model.procedural_curves[0];
    let ProceduralCurveDefinition::VectorOffset(definition_payload) = procedural.definition()
    else {
        panic!("expected vector offset construction")
    };
    let source = definition_payload.source();
    let parameter_range = definition_payload.parameter_range();
    let offset = definition_payload.offset();
    let roles = definition_payload.roles();
    assert_eq!(parameter_range.endpoints(), [-2.0, 5.0]);
    assert_eq!(*offset, cadmpeg_ir::math::Vector3::new(5.0, -10.0, 20.0));
    assert_eq!(
        *roles,
        VectorOffsetRoles {
            source: 7,
            offset: 9,
        }
    );
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        procedural
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.008)
    );
    let expected_range = *parameter_range;
    let expected_offset = *offset;
    let expected_roles = *roles;

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::VectorOffset(definition_payload) = definition else {
            panic!("expected editable vector offset")
        };
        let mut parameter_range_value = definition_payload.parameter_range().endpoints();
        let parameter_range = &mut parameter_range_value;
        let mut offset_value = definition_payload.offset().get();
        let offset = &mut offset_value;
        {
            *parameter_range = [-3.0, 6.0];
            *offset = cadmpeg_ir::math::Vector3::new(8.0, -12.0, 25.0);
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::VectorOffsetCurveConstruction::try_new(
                definition_payload.source().clone(),
                parameter_range_value,
                offset_value,
                *definition_payload.roles(),
                None,
            )
            .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
    });
    edited.model.procedural_curves[0]
        .set_cache_fit_tolerance(Some(0.015))
        .unwrap();
    let edited_definition = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("vector-offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated vector-offset decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &edited_definition
    );
    assert_eq!(
        regenerated.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.015)
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let source_id = match source_less.model.procedural_curves[0].definition() {
        ProceduralCurveDefinition::VectorOffset(definition_payload) => {
            let source = definition_payload.source();
            source.clone()
        }
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == source_id)
        .expect("vector-offset source carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            1,
            vec![-2.0, -2.0, 5.0, 5.0],
            vec![
                cadmpeg_ir::math::Point3::new(-9.0, 2.0, 3.0),
                cadmpeg_ir::math::Point3::new(5.0, 9.0, -0.5),
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
        .expect("source-less vector-offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less vector-offset round trip");
    let ProceduralCurveDefinition::VectorOffset(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip vector offset")
    };
    let source = definition_payload.source();
    let parameter_range = definition_payload.parameter_range();
    let offset = definition_payload.offset();
    let roles = definition_payload.roles();
    assert_eq!(*parameter_range, expected_range);
    assert_eq!(*offset, expected_offset);
    assert_eq!(*roles, expected_roles);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert_eq!(
        round_trip.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.008)
    );
    assert!(matches!(
        round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *source)
            .map(|curve| &curve.geometry),
        Some(cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(curve)))
            if curve.degree() == 1
                && curve.knots().as_slice() == [-2.0, -2.0, 5.0, 5.0]
                && curve.control_points() == [
                    cadmpeg_ir::math::Point3::new(-9.0, 2.0, 3.0),
                    cadmpeg_ir::math::Point3::new(5.0, 9.0, -0.5),
                ]
    ));
}

#[test]
fn generated_subset_curve_decodes_edits_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_subset_curve_smbh())),
                &DecodeOptions::default(),
            )
            .expect("generated subset decode"),
    );
    let ProceduralCurveDefinition::Subset(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected subset construction")
    };
    let source = definition_payload.source();
    let parameter_range = definition_payload.parameter_range();
    let _ = definition_payload.sense();
    assert_eq!(parameter_range.endpoints(), [-1.5, 3.5]);
    assert!(result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    assert!(
        (result.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .expect("subset fit tolerance")
            .get()
            - 0.006)
            .abs()
            < 1.0e-12
    );

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Subset(definition_payload) = definition else {
            unreachable!()
        };
        let mut parameter_range_value = definition_payload.parameter_range().endpoints();
        let parameter_range = &mut parameter_range_value;
        {
            *parameter_range = [-2.0, 4.0];
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::SubsetCurveConstruction::try_new(
                definition_payload.source().clone(),
                parameter_range_value,
                *definition_payload.sense(),
                None,
            )
            .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
    });
    let expected_edit = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("subset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated subset decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &expected_edit
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let source_id = match source_less.model.procedural_curves[0].definition() {
        ProceduralCurveDefinition::Subset(definition_payload) => {
            let source = definition_payload.source();
            source.clone()
        }
        _ => unreachable!(),
    };
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == source_id)
        .expect("subset source carrier")
        .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
            1,
            vec![-1.5, -1.5, 3.5, 3.5],
            vec![
                cadmpeg_ir::math::Point3::new(8.5, 23.0, 29.25),
                cadmpeg_ir::math::Point3::new(13.5, 13.0, 31.75),
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
        .expect("source-less subset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less subset round trip");
    let ProceduralCurveDefinition::Subset(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip subset")
    };
    let source = definition_payload.source();
    let parameter_range = definition_payload.parameter_range();
    let _ = definition_payload.sense();
    assert_eq!(parameter_range.endpoints(), [-1.5, 3.5]);
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == *source));
    let source_curve = round_trip
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| curve.id == *source)
        .expect("round-trip subset source");
    assert_eq!(
        source_curve.geometry,
        cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
            cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
                1,
                vec![-1.5, -1.5, 3.5, 3.5],
                vec![
                    cadmpeg_ir::math::Point3::new(8.5, 23.0, 29.25),
                    cadmpeg_ir::math::Point3::new(13.5, 13.0, 31.75),
                ],
                None,
                false,
            )
            .expect("valid subset source curve")
        ))
    );
}

#[test]
fn generated_exact_intcurve_preserves_native_construction_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_consistent_exact_curve_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated exact intcurve decode");
    assert_eq!(
        result.ir().model.procedural_curves[0].definition(),
        &ProceduralCurveDefinition::Exact {
            cache: Some(cadmpeg_ir::geometry::LegacyCache::try_new(0.004).expect("fit tolerance"))
        }
    );
    assert_eq!(
        result.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.004)
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
        .expect("source-less exact intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less exact intcurve round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &ProceduralCurveDefinition::Exact {
            cache: Some(cadmpeg_ir::geometry::LegacyCache::try_new(0.004).expect("fit tolerance"))
        }
    );
    assert_eq!(
        round_trip.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.004)
    );
}

#[test]
fn generated_spline_carriers_write_explicit_forward_sense() {
    for (smbh, head) in [
        (
            synthetic_geometry_with_consistent_exact_curve_smbh(),
            "intcurve",
        ),
        (synthetic_exact_spl_sur_smbh("exact_spl_sur"), "spline"),
    ] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated spline carrier decode");
        let (mut source_less, _, _) = result.into_parts();
        source_less.source = None;
        source_less
            .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
            .unwrap();

        let mut encoded = Vec::new();
        F3dCodec
            .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
            .and_then(|plan| plan.write_to(&mut encoded))
            .expect("source-less spline carrier encode");
        let mut archive = zip::ZipArchive::new(Cursor::new(&encoded)).expect("generated F3D ZIP");
        let mut generated_smbh = Vec::new();
        archive
            .by_name("FusionAssetName[Active]/Breps.BlobParts/BREP.generated.smbh")
            .expect("generated BREP stream")
            .read_to_end(&mut generated_smbh)
            .expect("generated BREP bytes");
        let record_start = generated_smbh
            .windows(b"\x0d\x09asmheader".len())
            .position(|window| window == b"\x0d\x09asmheader")
            .expect("generated ASM record table");
        let records = cadmpeg_asm::test_support::sab::frame(
            &generated_smbh,
            record_start,
            generated_smbh.len(),
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .expect("generated ASM records must frame");
        let record = records
            .iter()
            .find(|record| record.head() == head)
            .expect("generated spline carrier record");
        let subtype = record
            .tokens
            .iter()
            .position(|token| matches!(token, cadmpeg_asm::sab::Token::SubtypeOpen))
            .expect("spline carrier subtype scope");
        assert!(subtype > 0);
        assert_eq!(record.tokens[subtype - 1], cadmpeg_asm::sab::Token::False);
    }
}

#[test]
fn generated_intcurve_sense_uses_token_adjacent_to_subtype() {
    let decode_curve = |smbh: Vec<u8>| {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated exact intcurve decode");
        let curve_id = result
            .ir()
            .model
            .procedural_curve_owner(&result.ir().model.procedural_curves[0].id)
            .expect("exact intcurve owner");
        result
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == *curve_id)
            .expect("exact intcurve carrier")
            .geometry
            .clone()
    };

    assert_eq!(
        decode_curve(synthetic_geometry_with_decoy_curve_sense_smbh()),
        decode_curve(synthetic_geometry_with_consistent_exact_curve_smbh())
    );
}

#[test]
fn generated_spline_surface_sense_uses_token_adjacent_to_subtype() {
    let decode_surface = |smbh: Vec<u8>| {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("generated exact spline-surface decode");
        let surface_id = result
            .ir()
            .model
            .procedural_surface_owner(&result.ir().model.procedural_surfaces[0].id)
            .expect("exact spline-surface owner");
        let geometry = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| surface.id == *surface_id)
            .expect("exact spline-surface carrier")
            .geometry
            .clone();
        let face_sense = result
            .ir()
            .model
            .faces
            .iter()
            .find(|face| face.surface == *surface_id)
            .expect("spline-surface face")
            .sense;
        (geometry, face_sense)
    };

    assert_eq!(
        decode_surface(synthetic_exact_spl_sur_with_decoy_sense_smbh()),
        decode_surface(synthetic_exact_spl_sur_smbh("exact_spl_sur"))
    );
}

#[test]
fn generated_legacy_intcurve_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let cases = [
        with_legacy_subtype(
            synthetic_geometry_with_consistent_exact_curve_smbh(),
            "exact_int_cur",
            "exactcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_vector_offset_curve_smbh(),
            "offset_int_cur",
            "offsetintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_subset_curve_smbh(),
            "subset_int_cur",
            "subsetintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_analytic_offset_supports_smbh(),
            "off_int_cur",
            "offintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_offset_smbh(),
            "off_surf_int_cur",
            "offsurfintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_projection_smbh(),
            "proj_int_cur",
            "projcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_intersection_smbh(),
            "int_int_cur",
            "surfintcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_spring_smbh(),
            "spring_int_cur",
            "blndsprngcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("blend_int_cur"),
            "blend_int_cur",
            "bldcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("surf_int_cur"),
            "surf_int_cur",
            "surfcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("par_int_cur"),
            "par_int_cur",
            "parcur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_surface_curve_smbh("skin_int_cur"),
            "skin_int_cur",
            "d5c2_cur",
        ),
        with_legacy_subtype(
            synthetic_geometry_with_silhouette_smbh("para_silh_int_cur", None),
            "para_silh_int_cur",
            "parasil",
        ),
    ];

    for bytes in cases {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("legacy intcurve alias decode");
        assert!(!matches!(
            result.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Unknown { .. }
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
            .expect("canonical source-less intcurve encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical intcurve round trip");
        assert!(!matches!(
            round_trip.ir().model.procedural_curves[0].definition(),
            ProceduralCurveDefinition::Unknown { .. }
        ));
    }
}

#[test]
fn generated_compound_intcurve_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_geometry_with_compound_curve_smbh())),
                &DecodeOptions::default(),
            )
            .expect("generated compound intcurve decode"),
    );
    let ProceduralCurveDefinition::Compound(compound) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected compound construction")
    };
    let parameters = compound.parameters();
    let components = compound.components();

    assert_eq!(
        parameters
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        [0.0, 0.5, 1.0]
    );
    assert_eq!(
        components
            .iter()
            .map(|item| item.parameter.get())
            .collect::<Vec<_>>(),
        [-2.0, 4.0]
    );
    assert_eq!(components.len(), 2);
    assert!(components.iter().all(|component| result
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.id == component.component)));
    assert!(
        (result.ir().model.procedural_curves[0]
            .cache_fit_tolerance()
            .expect("compound fit tolerance")
            .get()
            - 0.003)
            .abs()
            < 1.0e-12
    );
    let component_ids = components.to_vec();

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Compound(compound) = definition else {
            unreachable!()
        };
        let mut components = compound
            .components()
            .iter()
            .map(cadmpeg_ir::geometry::CompoundComponent::to_raw)
            .collect::<Vec<_>>();
        for (component, parameter) in components.iter_mut().zip([-3.0, 5.0]) {
            component.parameter = parameter;
        }
        let restored_cache = compound.legacy_cache();
        *compound = cadmpeg_ir::geometry::CompoundCurveConstruction::try_new(
            vec![-0.25, 0.75, 1.25],
            components,
            None,
        )
        .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
    });
    let expected_edit = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("compound intcurve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated compound intcurve decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &expected_edit
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    for (ordinal, component) in component_ids.iter().enumerate() {
        source_less
            .model
            .curves
            .iter_mut()
            .find(|curve| curve.id == component.component)
            .expect("compound component curve")
            .geometry = cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Line(
            cadmpeg_ir::geometry::analytic::LineCurve::try_new(
                cadmpeg_ir::math::Point3::new(
                    f64_from_index(ordinal).expect("fixture index is exact in f64"),
                    -1.0,
                    2.0,
                ),
                cadmpeg_ir::math::Vector3::new(2.0, 3.0, -4.0)
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
        .expect("source-less compound intcurve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less compound intcurve round trip");
    let ProceduralCurveDefinition::Compound(compound) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip compound construction")
    };
    let parameters = compound.parameters();
    let components = compound.components();

    assert_eq!(
        parameters
            .iter()
            .map(|value| value.get())
            .collect::<Vec<_>>(),
        [0.0, 0.5, 1.0]
    );
    assert_eq!(
        components
            .iter()
            .map(|item| item.parameter.get())
            .collect::<Vec<_>>(),
        [-2.0, 4.0]
    );
    assert_eq!(components.len(), 2);
    for (ordinal, component) in components.iter().enumerate() {
        let curve = round_trip
            .ir()
            .model
            .curves
            .iter()
            .find(|curve| curve.id == component.component)
            .expect("round-trip compound component");
        let Some(SolvedCurveGeometry::Nurbs(curve)) = curve.geometry.solved() else {
            panic!("compound line component was not lowered to NURBS")
        };
        assert_eq!(curve.degree(), 1);
        let range = [
            f64_from_index(ordinal).expect("fixture index is exact in f64") * 0.5,
            f64_from_index(ordinal + 1).expect("fixture index is exact in f64") * 0.5,
        ];
        assert_eq!(
            curve.knots().as_slice(),
            [range[0], range[0], range[1], range[1]]
        );
    }
}

#[test]
fn generated_two_sided_offset_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_two_sided_offset_curve_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("generated two-sided offset decode"),
    );
    let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected two-sided offset construction")
    };
    let context = definition_payload.context();
    let discontinuity_flag = definition_payload.discontinuity_flag();
    let offsets = definition_payload.offsets();
    assert_eq!(context.parameter_range().endpoints(), [-1.0, 2.0]);
    assert!(*discontinuity_flag);
    assert_eq!(
        cadmpeg_ir::scalar::FiniteReal::raw_lanes(context.discontinuities()),
        [vec![0.25, 0.75], vec![], vec![0.5]]
    );
    assert!(context
        .sides()
        .iter()
        .all(|side| side.surface.is_none() && side.pcurve.is_none()));
    assert_eq!(offsets.get(), [-2.0, 4.0]);

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) = definition else {
            unreachable!()
        };
        let mut context_value = definition_payload.context().clone();
        let context = &mut context_value;
        let mut discontinuity_flag_value = *definition_payload.discontinuity_flag();
        let discontinuity_flag = &mut discontinuity_flag_value;
        let mut offsets_value = definition_payload.offsets().get();
        let offsets = &mut offsets_value;
        edit::replace(context, |previous| {
            let sides = previous.sides().clone();
            let mut range = previous.parameter_range().endpoints();
            let mut discontinuities =
                cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
            {
                let context_parameter_range: &mut [f64; 2] = &mut range;
                let context_discontinuities: &mut [Vec<f64>; 3] = &mut discontinuities;

                (*context_parameter_range) = [-2.0, 3.0];
                (*context_discontinuities) = [vec![0.2, 0.8], vec![], vec![0.6]];
                *discontinuity_flag = false;
                *offsets = [-3.0, 5.0];
            };
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
        })
        .unwrap();
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::TwoSidedOffsetCurveConstruction::try_new(
                context_value,
                discontinuity_flag_value,
                offsets_value,
                None,
            )
            .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
    });
    let expected_edit = edited.model.procedural_curves[0].definition().clone();
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("two-sided offset regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated two-sided offset decode");
    assert_eq!(
        regenerated.ir().model.procedural_curves[0].definition(),
        &expected_edit
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
        .expect("source-less two-sided offset encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-sided offset round trip");
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        source_less.model.procedural_curves[0].definition()
    );
}

#[test]
fn generated_embedded_offset_supports_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{
        pcurve::PcurveGeometry, ProceduralCurveDefinition, SolvedSurfaceGeometry, SurfaceGeometry,
    };

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_embedded_offset_supports_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("embedded offset-support decode"),
    );
    let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected embedded two-sided offset")
    };
    let context = definition_payload.context();
    let offsets = definition_payload.offsets();
    assert_eq!(offsets.get(), [-1.0, 3.0]);
    for side in context.sides() {
        let surface_id = side.surface.as_ref().expect("embedded support surface");
        assert!(result.ir().model.surfaces.iter().any(|surface| {
            surface.id == *surface_id
                && matches!(
                    surface.geometry,
                    SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(_))
                )
        }));
        assert!(matches!(
            side.pcurve.as_ref().map(|binding| &binding.geometry),
            Some(PcurveGeometry::Nurbs { .. })
        ));
    }
    assert!(matches!(
        context.sides()[1].pcurve.as_ref().map(|binding| &binding.geometry),
        Some(PcurveGeometry::Nurbs { nurbs }) if nurbs.weights().is_some()
    ));

    let mut retained = result.ir().clone();
    retained.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) = definition else {
            unreachable!()
        };
        let mut context_value = definition_payload.context().clone();
        let context = &mut context_value;
        let mut discontinuity_flag_value = *definition_payload.discontinuity_flag();
        let discontinuity_flag = &mut discontinuity_flag_value;
        let mut offsets_value = definition_payload.offsets().get();
        let offsets = &mut offsets_value;
        edit::replace(context, |previous| {
            let sides = previous.sides().clone();
            let mut range = previous.parameter_range().endpoints();
            let mut discontinuities =
                cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
            {
                let context_parameter_range: &mut [f64; 2] = &mut range;
                let context_discontinuities: &mut [Vec<f64>; 3] = &mut discontinuities;

                (*context_parameter_range) = [-2.0, 5.0];
                for (side, discontinuities) in (*context_discontinuities).iter_mut().enumerate() {
                    for (ordinal, value) in discontinuities.iter_mut().enumerate() {
                        *value = 0.125
                            * f64_from_index(side + ordinal + 1)
                                .expect("fixture index is exact in f64");
                    }
                }
                *discontinuity_flag = false;
                *offsets = [-2.5, 4.5];
            };
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
        })
        .unwrap();
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::TwoSidedOffsetCurveConstruction::try_new(
                context_value,
                discontinuity_flag_value,
                offsets_value,
                None,
            )
            .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
    });
    let expected_retained = retained.model.procedural_curves[0].definition().clone();
    let mut retained_bytes = Vec::new();
    crate::test_support::plan_inherited_write(
        &retained,
        result.source_fidelity(),
        &mut retained_bytes,
    )
    .expect("retained embedded offset-support edit");
    let retained_round_trip = F3dCodec
        .decode(&mut Cursor::new(retained_bytes), &DecodeOptions::default())
        .expect("retained embedded offset-support round trip");
    assert_eq!(
        retained_round_trip.ir().model.procedural_curves[0].definition(),
        &expected_retained
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let mut expected = source_less.model.procedural_curves[0].definition().clone();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less embedded offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less embedded offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset(expected_payload) = &mut expected else {
        unreachable!()
    };
    let mut expected_context = expected_payload.context().clone();
    let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip embedded offset supports")
    };
    let actual_context = definition_payload.context();
    for side in 0..2 {
        let expected_surface = source_less
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == expected_context.sides()[side].surface.as_ref())
            .expect("source support surface");
        let actual_surface = round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == actual_context.sides()[side].surface.as_ref())
            .expect("round-trip support surface");
        assert_eq!(actual_surface.geometry, expected_surface.geometry);
        expected_context.set_surface(side, actual_context.sides()[side].surface.clone());
    }
    *expected_payload =
        cadmpeg_ir::geometry::curve_payloads::TwoSidedOffsetCurveConstruction::try_new(
            expected_context,
            *expected_payload.discontinuity_flag(),
            expected_payload.offsets().get(),
            None,
        )
        .unwrap();
    assert_eq!(
        round_trip.ir().model.procedural_curves[0].definition(),
        &expected
    );
}

#[test]
fn generated_mixed_offset_supports_write_source_less() {
    use cadmpeg_ir::geometry::ProceduralCurveDefinition;

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_embedded_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("generated embedded offset-support decode");
    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let first_support = source_less.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) = definition else {
            panic!("expected two-sided offset construction")
        };
        let mut context_value = definition_payload.context().clone();
        let context = &mut context_value;
        let edit_result = {
            edit::with_output(context, |previous| {
                let mut sides = previous.sides().clone();
                let range = previous.parameter_range().endpoints();
                let discontinuities =
                    cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
                let output = {
                    let context_sides: &mut [cadmpeg_ir::geometry::IntcurveSupportSide; 2] =
                        &mut sides;

                    (*context_sides)[1].surface = None;
                    (*context_sides)[1].pcurve = None;
                    (*context_sides)[0].pcurve = Some(
                        cadmpeg_ir::geometry::pcurve::PcurveGeometry::Line(
                            cadmpeg_ir::geometry::pcurve::LinePcurve::try_new(
                                cadmpeg_ir::math::Point2::new(1.0, 2.0),
                                cadmpeg_ir::math::Point2::new(3.0, -1.0),
                            )
                            .unwrap(),
                        )
                        .into(),
                    );
                    (*context_sides)[0]
                        .surface
                        .clone()
                        .expect("retained first support id")
                };
                cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
                    .map(|candidate| (candidate, output))
            })
            .unwrap()
        };
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload =
            cadmpeg_ir::geometry::curve_payloads::TwoSidedOffsetCurveConstruction::try_new(
                context_value,
                *definition_payload.discontinuity_flag(),
                definition_payload.offsets().get(),
                None,
            )
            .unwrap();
        match restored_cache {
            Some(cache) => definition
                .set_legacy_cache(cache)
                .expect("the rebuilt construction states the same legacy cache slot"),
            None => definition.clear_legacy_cache(),
        }
        edit_result
    });
    let expected_surface = source_less
        .model
        .surfaces
        .iter()
        .find(|surface| surface.id == first_support)
        .expect("retained first support")
        .geometry
        .clone();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less mixed offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip two-sided offset construction")
    };
    let context = definition_payload.context();
    assert!(context.sides()[1].surface.is_none() && context.sides()[1].pcurve.is_none());
    assert_eq!(
        context.sides()[0].pcurve,
        Some(
            cadmpeg_ir::geometry::pcurve::PcurveGeometry::Nurbs {
                nurbs: cadmpeg_ir::geometry::pcurve::PcurveNurbs::from_lanes(
                    1,
                    vec![0.0, 0.0, 1.0, 1.0],
                    vec![
                        cadmpeg_ir::math::Point2::new(1.0, 2.0),
                        cadmpeg_ir::math::Point2::new(4.0, 1.0),
                    ],
                    None,
                    false,
                )
                .unwrap(),
            }
            .into()
        )
    );
    let actual_surface = round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| Some(&surface.id) == context.sides()[0].surface.as_ref())
        .expect("round-trip first support");
    assert_eq!(actual_surface.geometry, expected_surface);
}

#[test]
fn generated_analytic_offset_supports_decode_and_write_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SolvedSurfaceGeometry, SurfaceGeometry};

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(
                &synthetic_geometry_with_analytic_offset_supports_smbh(),
            )),
            &DecodeOptions::default(),
        )
        .expect("analytic offset-support decode");
    let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected analytic two-sided offset")
    };
    let context = definition_payload.context();
    let offsets = definition_payload.offsets();
    assert_eq!(offsets.get(), [-1.5, 2.5]);
    let supports = context.sides().each_ref().map(|side| {
        result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("analytic support surface")
            .geometry
            .clone()
    });
    assert!(
        matches!(supports[0], SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
            if {
                let axis = cone_surface.frame().axis().as_raw();
        let half_angle = cone_surface.half_angle().get();
                (cone_surface.radius().get() == 10.0)
                    && (cone_surface.ratio().get() == 0.4)
                    && ((half_angle - std::f64::consts::FRAC_PI_6).abs() < EPS_CONE_ANGLE
                        && *axis == cadmpeg_ir::math::Vector3::new(0.0, 0.0, -1.0))
            })
    );
    assert!(
        matches!(supports[1], SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) if { torus_surface.minor_radius().get() == -7.5 })
    );

    let (mut source_less, _, _) = result.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let expected_geometries = supports;
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less analytic offset-support encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less analytic offset-support round trip");
    let ProceduralCurveDefinition::TwoSidedOffset(definition_payload) =
        &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip analytic offset supports")
    };
    let context = definition_payload.context();
    let offsets = definition_payload.offsets();
    assert_eq!(offsets.get(), [-1.5, 2.5]);
    for (side, expected) in context.sides().iter().zip(expected_geometries) {
        let actual = round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("round-trip analytic support surface");
        assert_eq!(actual.geometry, expected);
    }
}

#[test]
fn generated_surface_intersection_decodes_and_writes_source_less() {
    use cadmpeg_ir::geometry::{ProceduralCurveDefinition, SolvedSurfaceGeometry, SurfaceGeometry};

    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(
                    &synthetic_geometry_with_surface_intersection_smbh(),
                )),
                &DecodeOptions::default(),
            )
            .expect("surface intersection decode"),
    );
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
        ..
    } = &result.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected surface intersection")
    };
    assert!(*discontinuity_flag);
    let expected_geometries = context.sides().each_ref().map(|side| {
        result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("intersection support surface")
            .geometry
            .clone()
    });
    assert!(
        matches!(expected_geometries[0], SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface))
        if {
            let half_angle = cone_surface.half_angle().get();
            (half_angle - std::f64::consts::FRAC_PI_6).abs() < EPS_CONE_ANGLE
        })
    );
    assert!(matches!(
        expected_geometries[1],
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(_))
    ));

    let mut edited = result.ir().clone();
    edited.model.procedural_curves[0].edit_definition(|definition| {
        let ProceduralCurveDefinition::Intersection {
            context,
            discontinuity_flag,
            ..
        } = definition
        else {
            unreachable!()
        };
        edit::replace(context, |previous| {
            let sides = previous.sides().clone();
            let mut range = previous.parameter_range().endpoints();
            let discontinuities =
                cadmpeg_ir::scalar::FiniteReal::raw_lanes(previous.discontinuities());
            {
                let context_parameter_range: &mut [f64; 2] = &mut range;

                (*context_parameter_range) = [-1.0, 2.0];
                *discontinuity_flag = false;
            };
            cadmpeg_ir::geometry::IntcurveSupportContext::try_new(sides, range, discontinuities)
        })
        .unwrap()
    });
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, result.source_fidelity(), &mut regenerated)
        .expect("intersection context regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated intersection decode");
    assert!(matches!(
        regenerated.ir().model.procedural_curves[0].definition(),
        ProceduralCurveDefinition::Intersection {
            ref context,
            discontinuity_flag: false,
        ..
    } if context.parameter_range().endpoints() == [-1.0, 2.0]
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
        .expect("source-less surface intersection encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less surface intersection round trip");
    let ProceduralCurveDefinition::Intersection {
        context,
        discontinuity_flag,
        ..
    } = &round_trip.ir().model.procedural_curves[0].definition()
    else {
        panic!("expected round-trip surface intersection")
    };
    assert!(*discontinuity_flag);
    for (side, expected) in context.sides().iter().zip(expected_geometries) {
        let actual = round_trip
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| Some(&surface.id) == side.surface.as_ref())
            .expect("round-trip intersection support");
        assert_eq!(actual.geometry, expected);
    }
}
