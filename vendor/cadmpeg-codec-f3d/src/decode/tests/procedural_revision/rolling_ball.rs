// SPDX-License-Identifier: Apache-2.0

use crate::test_support::smbh_blends_test::synthetic_partial_rb_blend_spl_sur_smbh;
use crate::test_support::smbh_blends_test::synthetic_rb_blend_spl_sur_smbh;
use crate::test_support::smbh_curves_test::with_legacy_subtype;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::SolvedCurveGeometry;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use cadmpeg_test_support::edit;
use std::io::Cursor;

#[test]
fn decode_retains_generated_rolling_ball_definition() {
    use cadmpeg_ir::geometry::{BlendCrossSection, BlendRadiusLaw, ProceduralSurfaceDefinition};

    let f3d = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    let procedural = result.ir().model.procedural_surfaces.first().unwrap();
    assert_eq!(
        procedural
            .cache_fit_tolerance()
            .map(cadmpeg_ir::geometry::FitTolerance::get),
        Some(0.01)
    );
    let ProceduralSurfaceDefinition::Blend(definition_payload) = procedural.definition() else {
        panic!("expected rolling-ball blend")
    };
    let supports = definition_payload.supports();
    let spine = definition_payload.spine();
    let radius = definition_payload.radius();
    let cross_section = definition_payload.cross_section();

    assert!(supports.iter().all(Option::is_some));
    assert!(supports.iter().flatten().all(|support| result
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface.id == support.surface)));
    let spine = result
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| Some(&curve.id) == spine.as_ref())
        .expect("blend spine carrier");
    let Some(SolvedCurveGeometry::Nurbs(spine)) = spine.geometry.solved() else {
        panic!("expected NURBS blend spine")
    };
    assert_eq!(spine.control_points().len(), 3);
    assert_eq!(cross_section, &BlendCrossSection::Circular);
    assert_eq!(radius, &BlendRadiusLaw::constant(-3.0).unwrap());
}

#[test]
fn generated_solved_plane_plane_blend_decodes_as_analytic_cylinder() {
    use cadmpeg_ir::geometry::{
        nurbs::NurbsCurve, BlendRadiusLaw, CurveGeometry, ProceduralSurfaceDefinition,
        SolvedCurveGeometry, SolvedSurfaceGeometry, SurfaceGeometry,
    };
    use cadmpeg_ir::math::{Point3, Vector3};

    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated rolling-ball decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let (support_ids, spine_id) =
        source_less.model.procedural_surfaces[0].edit_definition(|definition| {
            let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
                panic!("expected rolling-ball definition")
            };
            let mut edited_supports = definition_payload.supports().clone();
            let mut edited_spine = definition_payload.spine().clone();
            let mut edited_radius = definition_payload.radius().clone();
            let (supports, Some(spine), radius) =
                (&mut edited_supports, &mut edited_spine, &mut edited_radius)
            else {
                panic!("expected rolling-ball definition")
            };

            let support_ids = [
                supports[0].as_ref().expect("first support").surface.clone(),
                supports[1]
                    .as_ref()
                    .expect("second support")
                    .surface
                    .clone(),
            ];
            let spine_id = spine.clone();
            *radius = BlendRadiusLaw::constant(-2.0).unwrap();
            let restored_cache = definition_payload.legacy_cache();
            *definition_payload =
                cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
                    edited_supports,
                    edited_spine,
                    edited_radius,
                    definition_payload.cross_section().clone(),
                    cadmpeg_ir::geometry::CacheContract::from_form(
                        definition_payload
                            .native()
                            .map(cadmpeg_ir::geometry::RollingBallConstruction::to_raw)
                            .map(Box::new),
                    ),
                )
                .unwrap();
            definition
                .set_legacy_cache(restored_cache)
                .expect("the rebuilt construction states the same legacy cache slot");
            (support_ids, spine_id)
        });
    let support_geometry = [
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
            )
            .unwrap(),
        )),
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(
            cadmpeg_ir::geometry::analytic::PlaneSurface::try_new(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 1.0, 0.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
        )),
    ];
    for (id, geometry) in support_ids.into_iter().zip(support_geometry) {
        source_less
            .model
            .surfaces
            .iter_mut()
            .find(|surface| surface.id == id)
            .expect("rolling-ball support")
            .geometry = geometry;
    }
    source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("rolling-ball spine")
        .geometry = CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(
        NurbsCurve::from_lanes(
            2,
            vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
            vec![
                Point3::new(2.0, 2.0, -4.0),
                Point3::new(2.0, 2.0, 0.0),
                Point3::new(2.0, 2.0, 7.0),
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
        .expect("source-less rolling-ball encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less rolling-ball round trip");
    let carrier_id = round_trip
        .ir()
        .model
        .procedural_surface_owner(&round_trip.ir().model.procedural_surfaces[0].id)
        .expect("rolling-ball carrier");
    assert!(matches!(round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .find(|surface| &surface.id == carrier_id)
        .expect("rolling-ball carrier")
        .geometry
        .solved_cache()
        .expect("solved rolling-ball cache"), SolvedSurfaceGeometry::Cylinder(cylinder_surface)
            if {
                let origin = cylinder_surface.origin();
    let axis = cylinder_surface.frame().axis().as_raw();
    let radius = cylinder_surface.radius().get();
                *origin == Point3::new(2.0, 2.0, -4.0)
                    && *axis == Vector3::new(0.0, 0.0, 1.0)
                    && radius == 2.0
            }));
}

#[test]
fn generated_rolling_ball_surface_aliases_decode_and_write_canonically() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    for name in ["rbblnsur", "pipe_spl_sur", "pipesur"] {
        let bytes =
            with_legacy_subtype(synthetic_rb_blend_spl_sur_smbh(), "rb_blend_spl_sur", name);
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&bytes)),
                &DecodeOptions::default(),
            )
            .expect("rolling-ball alias decode");
        assert!(matches!(
            result.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Blend(..)
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
            .expect("canonical rolling-ball encode");
        let round_trip = F3dCodec
            .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
            .expect("canonical rolling-ball round trip");
        assert!(matches!(
            round_trip.ir().model.procedural_surfaces[0].definition(),
            ProceduralSurfaceDefinition::Blend(..)
        ));
    }
}

#[test]
fn generated_f3d_rewrites_rolling_ball_radius_law() {
    use cadmpeg_ir::geometry::{BlendRadiusLaw, ProceduralSurfaceDefinition};

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.procedural_surfaces[0].edit_definition(|definition| {
        let ProceduralSurfaceDefinition::Blend(definition_payload) = definition else {
            panic!("expected rolling-ball blend")
        };
        let mut edited_radius = definition_payload.radius().clone();
        let radius = &mut edited_radius;

        *radius = BlendRadiusLaw::linear(-2.0, -4.0).unwrap();
        let restored_cache = definition_payload.legacy_cache();
        *definition_payload = cadmpeg_ir::geometry::surface_payloads::BlendSurfacePayload::try_new(
            definition_payload.supports().clone(),
            definition_payload.spine().clone(),
            edited_radius,
            definition_payload.cross_section().clone(),
            cadmpeg_ir::geometry::CacheContract::from_form(
                definition_payload
                    .native()
                    .map(cadmpeg_ir::geometry::RollingBallConstruction::to_raw)
                    .map(Box::new),
            ),
        )
        .unwrap();
        definition
            .set_legacy_cache(restored_cache)
            .expect("the rebuilt construction states the same legacy cache slot");
    });

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("rolling-ball radius regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        &round_trip.ir().model.procedural_surfaces[0].definition()
    else {
        panic!("expected round-trip rolling-ball blend")
    };
    let radius = definition_payload.radius();

    assert_eq!(radius, &BlendRadiusLaw::linear(-2.0, -4.0).unwrap());
}

#[test]
fn generated_f3d_rewrites_rolling_ball_spine_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        edited.model.procedural_surfaces[0].definition()
    else {
        panic!("expected rolling-ball spine")
    };
    let (Some(spine),) = (definition_payload.spine(),) else {
        panic!("expected rolling-ball spine")
    };

    let spine_id = spine.clone();
    let curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == spine_id)
        .expect("blend spine curve");
    let cadmpeg_ir::geometry::CurveGeometry::Solved(SolvedCurveGeometry::Nurbs(nurbs)) =
        &mut curve.geometry
    else {
        panic!("expected NURBS blend spine")
    };
    let mut control_points = nurbs.pole_rows().raw_points();
    control_points[1].x = 8.0;
    control_points[1].y = -6.0;
    *nurbs = cadmpeg_ir::geometry::nurbs::NurbsCurve::from_lanes(
        1,
        vec![-1.0, -1.0, 2.0, 2.0, 2.0],
        control_points,
        nurbs.pole_rows().weights(),
        nurbs.periodic(),
    )
    .unwrap();
    let expected = curve.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("blend-spine regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve == &expected));
}

#[test]
fn generated_f3d_rewrites_rolling_ball_support_cache() {
    use cadmpeg_ir::geometry::ProceduralSurfaceDefinition;

    let source = f3d_with_smbh(&synthetic_rb_blend_spl_sur_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated rolling-ball decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let ProceduralSurfaceDefinition::Blend(definition_payload) =
        edited.model.procedural_surfaces[0].definition()
    else {
        panic!("expected rolling-ball blend")
    };
    let supports = definition_payload.supports();

    let support_id = supports[0]
        .as_ref()
        .expect("first blend support")
        .surface
        .clone();
    let surface = edited
        .model
        .surfaces
        .iter_mut()
        .find(|surface| surface.id == support_id)
        .expect("blend support surface");
    let cadmpeg_ir::geometry::SurfaceGeometry::Solved(SolvedSurfaceGeometry::Nurbs(nurbs)) =
        &mut surface.geometry
    else {
        panic!("expected NURBS blend support")
    };
    nurbs
        .try_map_control_points(|index, pole| {
            let mut pole = pole.get();
            if index == 1 {
                pole.x = 6.0;
                pole.z = 4.0;
            }
            cadmpeg_ir::features::FinitePoint3::new(pole).ok_or_else(|| {
                cadmpeg_ir::geometry::nurbs::NurbsError::Structure(
                    "control_points contains a non-finite point".into(),
                )
            })
        })
        .expect("pole edit admission");
    edit::replace(nurbs, |previous| {
        let mut knots = previous.u_knots().to_vec();
        {
            let knots: &mut [f64] = &mut knots;
            knots.copy_from_slice(&[-1.0, -1.0, 2.0, 2.0]);
        };
        cadmpeg_ir::geometry::nurbs::NurbsSurface::new(
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.u_degree(),
                knots,
                previous.u_periodic(),
            ),
            cadmpeg_ir::geometry::nurbs::NurbsSurfaceAxis::new(
                previous.v_degree(),
                previous.v_knots().to_vec(),
                previous.v_periodic(),
            ),
            previous.pole_grid().clone(),
            previous.normal_reversed(),
        )
    })
    .unwrap();
    let expected = surface.clone();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("blend-support regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated rolling-ball decode");
    assert!(round_trip
        .ir()
        .model
        .surfaces
        .iter()
        .any(|surface| surface == &expected));
}

#[test]
fn decode_reports_generated_partial_rolling_ball_supports() {
    let f3d = f3d_with_smbh(&synthetic_partial_rb_blend_spl_sur_smbh());
    let result = F3dCodec
        .decode(&mut Cursor::new(f3d), &DecodeOptions::default())
        .unwrap();

    assert!(result.report().losses.iter().any(|loss| loss
        .message
        .contains("only one of two native supports resolved")));
}
