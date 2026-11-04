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

use cadmpeg_test_support::EditableDecodeResult;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::EncodeInput;
use std::io::Cursor;

use cadmpeg_asm::asm_header;
use cadmpeg_ir::codec::write::Encoder;
use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, update_f3d_native};
use crate::test_support::smbh_blends_test::synthetic_full_rolling_ball_smbh;
use crate::test_support::smbh_curves_test::synthetic_geometry_with_procedural_curve_smbh;
use crate::test_support::smbh_geometry_test::{
    synthetic_free_vertex_body_smbh, synthetic_geometry_smbh,
    synthetic_geometry_with_degenerate_curve_smbh, synthetic_geometry_with_face_attribute_smbh,
    synthetic_mixed_face_wire_body_smbh, synthetic_mixed_smbh, synthetic_wire_body_smbh,
};
use crate::test_support::smbh_pcurves_test::synthetic_geometry_with_pcurve_smbh;
use crate::test_support::smbh_surfaces_test::synthetic_exact_spl_sur_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn decode_builds_valid_topology_and_geometry() {
    use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
    use cadmpeg_ir::math::Point3;

    let f3d = f3d_with_smbh(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut cur, &DecodeOptions::default())
            .unwrap(),
    );

    assert!(result.report().geometry_transferred());
    assert!(result
        .report()
        .notes
        .iter()
        .all(|note| !note.starts_with("container-level inspection only")));
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.loops.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.edges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    let ownerships = f3d_native(result.ir()).vertex_ownerships;
    assert_eq!(ownerships.len(), 3);
    assert_eq!(
        ownerships
            .iter()
            .map(|metadata| metadata.endpoint_index.code())
            .collect::<Vec<_>>(),
        [0, 1, 0]
    );
    assert_eq!(result.ir().model.points.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);
    assert_eq!(f3d_native(result.ir()).face_sidedness.len(), 1);
    assert_eq!(f3d_native(result.ir()).face_sidedness[0].containment, None);
    let continuities = f3d_native(result.ir()).edge_continuities;
    assert_eq!(continuities.len(), 3);
    assert!(continuities
        .iter()
        .all(|metadata| metadata.continuity == "unknown"));
    assert!(continuities
        .iter()
        .all(|metadata| metadata.sense == cadmpeg_ir::topology::Sense::Forward));
    assert!(result
        .source_fidelity()
        .annotations
        .provenance
        .contains_key(result.ir().model.bodies[0].id.as_str()));

    // The plane decoded with its stored origin and complete parameter frame.
    match &result.ir().model.surfaces[0].geometry {
        SurfaceGeometry::Solved(SolvedSurfaceGeometry::Plane(plane_surface)) => {
            let origin = plane_surface.origin();
            let normal = plane_surface.frame().axis().as_raw();
            let u_axis = plane_surface.frame().reference().as_raw();
            assert_eq!(*origin, Point3::new(0.0, 0.0, 0.0));
            assert_eq!(normal.z, 1.0);
            assert_eq!(*u_axis, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected plane, got {other:?}"),
    }
    // Point coordinates converted centimetre → millimetre (×10).
    let xs: Vec<f64> = result
        .ir()
        .model
        .points
        .iter()
        .map(|p| p.position().get().x)
        .collect();
    assert!(xs.contains(&10.0));

    // The decoded document is internally valid: refs resolve, the loop ring
    // closes, no bounds violations.
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "validation findings: {:?}", report.findings);

    // Edges carry no analytic curve (their carriers were null), which is legal.
    assert!(result.ir().model.edges.iter().all(|e| e.curve().is_none()));
    // The loop's coedge ring is the three coedges in order.
    assert_eq!(result.ir().model.loops[0].coedges().len(), 3);
}

#[test]
fn history_topology_decode_matches_full_brep_graph() {
    for (bytes, expected_tag_count) in [
        (synthetic_geometry_with_pcurve_smbh(), 0),
        (synthetic_geometry_with_face_attribute_smbh(), 3),
        (synthetic_full_rolling_ball_smbh("rb_blend_spl_sur"), 0),
    ] {
        let arena = cadmpeg_core::decode::DecodeArena::new();
        let (ctx, _) = cadmpeg_core::decode::DecodeContext::from_root_bytes(
            &bytes,
            &arena,
            &cadmpeg_core::decode::DecodePolicy::service(),
        )
        .expect("fixture is within the service input limit");
        let start = asm_header::record_stream_start(&bytes).expect("record stream start");
        let limit = asm_header::solved_record_limit(&bytes).expect("solved record limit");
        let records = cadmpeg_asm::test_support::sab::frame(
            &bytes,
            start,
            limit,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .expect("frame BREP");

        let full_brep = crate::brep::decode(&ctx, &records, &bytes, "full", crate::ids::ID_FORMAT)
            .expect("valid BREP tolerances");
        let full = crate::history::historical_topology_with_tags(&full_brep)
            .expect("topology budget")
            .expect("full topology");
        let history_brep =
            crate::brep::decode_history_topology(&ctx, &records, &bytes, crate::ids::ID_FORMAT)
                .expect("valid BREP tolerances");
        let history = crate::history::historical_topology_with_tags(&history_brep)
            .expect("topology budget")
            .expect("history topology");

        assert_eq!(history, full);
        assert_eq!(history.persistent_subentity_tags.len(), expected_tag_count);
    }
}

#[test]
fn decode_transfers_generated_wire_body_topology() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let mut result = EditableDecodeResult::from(result);
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(result.ir().model.shells.len(), 1);
    assert!(result.ir().model.shells[0].faces().is_empty());
    assert_eq!(result.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(result.ir().model.edges.len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 2);
    assert_eq!(result.ir().model.points.len(), 2);
    assert_eq!(result.ir().model.curves.len(), 1);
    assert_eq!(f3d_native(result.ir()).wire_topologies.len(), 1);
    assert_eq!(
        f3d_native(result.ir()).wire_topologies[0].side,
        cadmpeg_asm::brep::records::WireSide::Out
    );
    assert_eq!(
        result.ir().model.shells[0].wire_edges()[0],
        result.ir().model.edges[0].id
    );
    assert!(!result
        .report()
        .losses
        .iter()
        .any(|loss| loss.message.contains("wire=")));
    update_f3d_native(&mut result.ir_mut(), |native| {
        native.wire_topologies[0].side = cadmpeg_asm::brep::records::WireSide::In;
    });
    let mut edited = Vec::new();
    crate::test_support::plan_inherited_write(result.ir(), result.source_fidelity(), &mut edited)
        .expect("wire-side retained edit");
    let edited = F3dCodec
        .decode(&mut Cursor::new(edited), &DecodeOptions::default())
        .expect("wire-side retained round trip");
    assert_eq!(
        f3d_native(edited.ir()).wire_topologies[0].side,
        cadmpeg_asm::brep::records::WireSide::In
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_transfers_isolated_vertex_wire_topology() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    assert_eq!(result.ir().model.bodies.len(), 1);
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(result.ir().model.shells[0].wire_edges().is_empty());
    assert_eq!(result.ir().model.shells[0].free_vertices().len(), 1);
    assert_eq!(result.ir().model.vertices.len(), 1);
    assert_eq!(result.ir().model.points.len(), 1);
    assert_eq!(
        result.ir().model.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!(f3d_native(result.ir()).vertex_ownerships.is_empty());
    let wire = &f3d_native(result.ir()).wire_topologies[0];
    assert!(wire.members.edges().is_empty());
    assert_eq!(
        wire.members.free_vertex().cloned(),
        Some(result.ir().model.vertices[0].id.clone())
    );
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "free-vertex findings: {:?}",
        validation.findings
    );
}

#[test]
fn decode_classifies_generated_mixed_face_wire_body_as_general() {
    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    assert_eq!(
        result.ir().model.bodies.len(),
        1,
        "mixed decode report: {:?}",
        result.report()
    );
    assert_eq!(
        result.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(result.ir().model.edges.len(), 4);
    assert_eq!(result.ir().model.curves.len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "mixed-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_degenerate_curve_decodes_regenerates_and_writes_source_less() {
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
    use cadmpeg_ir::math::Point3;

    let source = f3d_with_smbh(&synthetic_geometry_with_degenerate_curve_smbh());
    let decoded = EditableDecodeResult::from(
        F3dCodec
            .decode(&mut Cursor::new(&source), &DecodeOptions::default())
            .expect("generated degenerate curve decode"),
    );
    let curve = decoded
        .ir()
        .model
        .curves
        .iter()
        .find(|curve| {
            matches!(
                curve.geometry,
                CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(_))
            )
        })
        .expect("degenerate curve carrier");
    assert_eq!(
        curve.geometry,
        CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
            cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(0.0, 0.0, 0.0))
                .unwrap()
        ))
    );
    let curve_id = curve.id.clone();

    let mut edited = decoded.ir().clone();
    let edited_curve = edited
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id == curve_id)
        .expect("editable degenerate curve");
    edited_curve.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
        cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(2.0, 3.0, 4.0))
            .unwrap(),
    ));
    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, decoded.source_fidelity(), &mut regenerated)
        .expect("degenerate curve regeneration");
    let regenerated = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated degenerate curve decode");
    assert!(regenerated.ir().model.curves.iter().any(|curve| {
        curve.geometry
            == CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
                cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(
                    2.0, 3.0, 4.0,
                ))
                .unwrap(),
            ))
    }));

    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let expected = CurveGeometry::Solved(SolvedCurveGeometry::Degenerate(
        cadmpeg_ir::geometry::analytic::DegenerateCurve::try_new(Point3::new(0.0, 0.0, 0.0))
            .unwrap(),
    ));
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less degenerate curve encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less degenerate curve round trip");
    assert!(round_trip
        .ir()
        .model
        .curves
        .iter()
        .any(|curve| curve.geometry == expected));
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "degenerate-curve findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_general_face_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less general body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less general body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(round_trip.ir().model.edges.len(), 4);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "mixed-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_general_face_and_point_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_mixed_face_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated mixed body decode");
    let free = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let renamed = free
        .ir()
        .to_canonical_json()
        .expect("canonical free-vertex JSON")
        .replace("f3d:brep:", "generated:general_point_wire:");
    let mut free =
        cadmpeg_ir::document::CadIr::from_json(&renamed).expect("renamed free-vertex IR");
    source_less.model.shells[0].add_free_vertex(free.model.vertices[0].id.clone());
    source_less.model.vertices.append(&mut free.model.vertices);
    source_less.model.points.append(&mut free.model.points);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less face-and-point-wire body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less face-and-point-wire body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::General
    );
    assert_eq!(round_trip.ir().model.faces.len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].free_vertices().len(), 1);
    assert_eq!(f3d_native(round_trip.ir()).wire_topologies.len(), 2);
    assert!(f3d_native(round_trip.ir())
        .wire_topologies
        .iter()
        .any(|wire| wire.members.edges().is_empty() && wire.members.free_vertex().is_some()));
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "face-and-point-wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_solid_and_wire_bodies_together() {
    let mut source_less = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    let decoded_wire = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let wire_json = decoded_wire
        .ir()
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:combined_wire:");
    let mut wire =
        cadmpeg_ir::document::CadIr::from_json(&wire_json).expect("renamed combined wire IR");
    source_less.model.bodies.append(&mut wire.model.bodies);
    source_less.model.regions.append(&mut wire.model.regions);
    source_less.model.shells.append(&mut wire.model.shells);
    source_less.model.edges.append(&mut wire.model.edges);
    source_less.model.vertices.append(&mut wire.model.vertices);
    source_less.model.points.append(&mut wire.model.points);
    source_less.model.curves.append(&mut wire.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less solid-plus-wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less solid-plus-wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 2);
    assert_eq!(
        round_trip
            .ir()
            .model
            .bodies
            .iter()
            .map(|body| body.kind)
            .collect::<Vec<_>>(),
        [
            cadmpeg_ir::topology::BodyKind::Solid,
            cadmpeg_ir::topology::BodyKind::Wire,
        ]
    );
    assert_eq!(round_trip.ir().model.faces.len(), 6);
    assert_eq!(round_trip.ir().model.shells[1].wire_edges().len(), 1);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "combined-body findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_wire_body_topology() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    update_f3d_native(&mut source_less, |native| {
        native.wire_topologies[0].side = cadmpeg_asm::brep::records::WireSide::In;
    });
    let expected_curve = source_less.model.curves[0].geometry.clone();
    let expected_points = source_less
        .model
        .points
        .iter()
        .map(|point| point.position().get())
        .collect::<Vec<_>>();

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less wire body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less wire body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert_eq!(round_trip.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(
        f3d_native(round_trip.ir()).wire_topologies[0].side,
        cadmpeg_asm::brep::records::WireSide::In
    );
    assert_eq!(round_trip.ir().model.edges.len(), 1);
    assert_eq!(
        round_trip
            .ir()
            .model
            .points
            .iter()
            .map(|point| point.position().get())
            .collect::<Vec<_>>(),
        expected_points
    );
    assert_eq!(round_trip.ir().model.curves[0].geometry, expected_curve);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_isolated_vertex_wire() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    update_f3d_native(&mut source_less, |native| {
        native.wire_topologies[0].side = cadmpeg_asm::brep::records::WireSide::In;
    });

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less free-vertex wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less free-vertex wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(
        round_trip.ir().model.bodies[0].kind,
        cadmpeg_ir::topology::BodyKind::Wire
    );
    assert!(round_trip.ir().model.shells[0].wire_edges().is_empty());
    assert_eq!(round_trip.ir().model.shells[0].free_vertices().len(), 1);
    assert!(round_trip.ir().model.edges.is_empty());
    assert_eq!(round_trip.ir().model.vertices.len(), 1);
    assert_eq!(
        round_trip.ir().model.points[0].position().get(),
        cadmpeg_ir::math::Point3::new(10.0, 20.0, 30.0)
    );
    assert!(f3d_native(round_trip.ir()).vertex_ownerships.is_empty());
    let wire = &f3d_native(round_trip.ir()).wire_topologies[0];
    assert!(wire.members.edges().is_empty());
    assert_eq!(
        wire.members.free_vertex().cloned(),
        Some(round_trip.ir().model.vertices[0].id.clone())
    );
    assert_eq!(wire.side, cadmpeg_asm::brep::records::WireSide::In);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "free-vertex findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_edge_and_point_wires_on_one_shell() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let free = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_free_vertex_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated free-vertex body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let free_json = free
        .ir()
        .to_canonical_json()
        .expect("canonical free-vertex JSON");
    for namespace in ["generated:point_wire_one:", "generated:point_wire_two:"] {
        let renamed = free_json.replace("f3d:brep:", namespace);
        let mut free =
            cadmpeg_ir::document::CadIr::from_json(&renamed).expect("renamed free-vertex IR");
        source_less.model.shells[0].add_free_vertex(free.model.vertices[0].id.clone());
        source_less.model.vertices.append(&mut free.model.vertices);
        source_less.model.points.append(&mut free.model.points);
    }

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less mixed-wire shell encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less mixed-wire shell round trip");
    assert_eq!(round_trip.ir().model.shells[0].wire_edges().len(), 1);
    assert_eq!(round_trip.ir().model.shells[0].free_vertices().len(), 2);
    assert_eq!(f3d_native(round_trip.ir()).wire_topologies.len(), 3);
    assert!(f3d_native(round_trip.ir())
        .wire_topologies
        .iter()
        .any(|wire| wire.members.edges().len() == 1 && wire.members.free_vertex().is_none()));
    assert!(f3d_native(round_trip.ir())
        .wire_topologies
        .iter()
        .any(|wire| wire.members.edges().is_empty() && wire.members.free_vertex().is_some()));
    assert_eq!(
        f3d_native(round_trip.ir())
            .wire_topologies
            .iter()
            .filter(|wire| wire.members.edges().is_empty() && wire.members.free_vertex().is_some())
            .count(),
        2
    );
    assert_eq!(round_trip.ir().model.vertices.len(), 4);
    assert_eq!(round_trip.ir().model.points.len(), 4);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "mixed-wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_two_independent_wire_bodies() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire IR");
    second.model.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [1.0, 0.0, 0.0, 25.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ])
        .expect("affine transform"),
    );
    source_less.model.bodies.append(&mut second.model.bodies);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less two-wire-body encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less two-wire-body round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 2);
    assert!(round_trip
        .ir()
        .model
        .bodies
        .iter()
        .all(|body| body.kind == cadmpeg_ir::topology::BodyKind::Wire));
    assert_eq!(round_trip.ir().model.regions.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    assert_eq!(round_trip.ir().model.curves.len(), 2);
    assert_eq!(
        round_trip.ir().model.bodies[1]
            .transform
            .expect("second wire transform")
            .rows()[0][3],
        25.0
    );
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_edge_wire_ring() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_edge_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire edge IR");
    let second_edge = second.model.edges[0].id.clone();
    source_less.model.shells[0].add_wire_edge(second_edge);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-edge wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-edge wire round trip");
    assert_eq!(round_trip.ir().model.shells[0].wire_edges().len(), 2);
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    assert_eq!(round_trip.ir().model.curves.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_region_wire_body() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_region_two:");
    let mut second = cadmpeg_ir::document::CadIr::from_json(&second_json)
        .expect("renamed second wire region IR");
    let body_id = source_less.model.bodies[0].id.clone();
    let region_id = second.model.regions[0].id.clone();
    second.model.regions[0].body = body_id;
    source_less.model.bodies[0].regions.push(region_id);
    source_less.model.regions.append(&mut second.model.regions);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-region wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-region wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(round_trip.ir().model.bodies[0].regions.len(), 2);
    assert_eq!(round_trip.ir().model.regions.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert!(round_trip
        .ir()
        .model
        .regions
        .iter()
        .all(|region| region.body == round_trip.ir().model.bodies[0].id));
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn generated_source_less_writes_multi_shell_wire_region() {
    let decoded = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&synthetic_wire_body_smbh())),
            &DecodeOptions::default(),
        )
        .expect("generated wire body decode");
    let (mut source_less, _, _) = decoded.into_parts();
    source_less.source = None;
    source_less
        .set_native_unknowns(&cadmpeg_test_support::service_decode_context(), "f3d", &[])
        .unwrap();
    let second_json = source_less
        .to_canonical_json()
        .expect("canonical wire JSON")
        .replace("f3d:brep:", "generated:wire_shell_two:");
    let mut second =
        cadmpeg_ir::document::CadIr::from_json(&second_json).expect("renamed second wire shell IR");
    let region_id = source_less.model.regions[0].id.clone();
    let shell_id = second.model.shells[0].id.clone();
    second.model.shells[0].region = region_id;
    source_less.model.regions[0].shells.push(shell_id);
    source_less.model.shells.append(&mut second.model.shells);
    source_less.model.edges.append(&mut second.model.edges);
    source_less
        .model
        .vertices
        .append(&mut second.model.vertices);
    source_less.model.points.append(&mut second.model.points);
    source_less.model.curves.append(&mut second.model.curves);

    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less multi-shell wire encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less multi-shell wire round trip");
    assert_eq!(round_trip.ir().model.bodies.len(), 1);
    assert_eq!(round_trip.ir().model.regions.len(), 1);
    assert_eq!(round_trip.ir().model.regions[0].shells.len(), 2);
    assert_eq!(round_trip.ir().model.shells.len(), 2);
    assert!(round_trip
        .ir()
        .model
        .shells
        .iter()
        .all(|shell| shell.region == round_trip.ir().model.regions[0].id));
    assert_eq!(round_trip.ir().model.edges.len(), 2);
    let validation = cadmpeg_ir::validate::validate_neutral(round_trip.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(
        validation.is_ok(),
        "wire findings: {:?}",
        validation.findings
    );
}

#[test]
fn analytic_carrier_decode_covers_each_shape() {
    use cadmpeg_asm::brep::geometry;
    use cadmpeg_asm::sab::{Record, Token};
    use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry, SolvedSurfaceGeometry};

    fn rec(head: &str, tokens: Vec<Token>) -> Record {
        Record {
            index: 0,
            name: head.to_string(),

            tokens: tokens.into(),
            offset: 0,
            len: 0,
        }
    }
    let decode_surface = |record: &Record| geometry::decode_surface(record);
    let decode_curve = |record: &Record| geometry::decode_curve(record);
    let refn = || Token::Ref(-1);
    let base = || vec![refn(), Token::Long(-1), refn()];

    // cone with sine==0 decodes to a cylinder; |major| (cm) ×10 = radius (mm).
    let mut cyl = base();
    cyl.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]), // axis
        Token::Vector3([2.0, 0.0, 0.0]), // ref × r_major, |.|=2 cm
        Token::Double(1.0),              // ratio
        Token::Double(0.0),              // sine → cylinder
        Token::Double(1.0),              // cosine
        Token::Double(2.0),              // r1 = 2 cm
    ]);
    match decode_surface(&rec("cone", cyl)).unwrap().0 {
        SolvedSurfaceGeometry::Cylinder(cylinder_surface) => {
            let axis = *cylinder_surface.frame().axis().as_raw();
            let ref_direction = *cylinder_surface.frame().reference().as_raw();
            let radius = cylinder_surface.radius().get();
            assert_eq!(radius, 20.0);
            assert_eq!(axis.z, 1.0);
            assert_eq!(ref_direction, cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0));
        }
        other => panic!("expected cylinder, got {other:?}"),
    }

    let mut elliptical_cylinder = base();
    elliptical_cylinder.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([2.0, 0.0, 0.0]),
        Token::Double(0.4),
        Token::Double(0.0),
        Token::Double(1.0),
        Token::Double(2.0),
    ]);
    assert!(
        matches!(decode_surface(&rec("cone", elliptical_cylinder)).unwrap().0, SolvedSurfaceGeometry::Cone(cone_surface)
        if {
            (cone_surface.radius().get() == 20.0)
                && (cone_surface.ratio().get() == 0.4)
                && (cone_surface.half_angle().get() == 0.0)
        })
    );

    // cone with nonzero sine keeps the acute half-angle atan2(|sine|, |cosine|).
    // A both-negative sine/cosine pair has a positive slope (the radius still
    // grows along `+axis`, so the axis is kept), and the negative cosine
    // marks the inward native normal for the face-sense fold.
    let mut cone = base();
    cone.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([2.0, 0.0, 0.0]),
        Token::Double(1.0),
        Token::Double(-0.5), // sine (both-negative branch)
        Token::Double(-0.866_025_4),
        Token::Double(2.0),
    ]);
    let (geo, inward) = decode_surface(&rec("cone", cone)).unwrap();
    assert!(inward, "negative cosine points the native normal inward");
    match geo {
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let axis = cone_surface.frame().axis().as_raw();
            let ref_direction = cone_surface.frame().reference().as_raw();
            let half_angle = cone_surface.half_angle().get();
            assert!((half_angle - 0.5f64.atan2(0.866_025_4)).abs() < 1.0e-12);
            assert_eq!(axis.z, 1.0, "positive slope keeps the axis");
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
        }
        other => panic!("expected cone, got {other:?}"),
    }

    // A negative sine with positive cosine shrinks the radius along the
    // native axis; the IR cone grows along `+axis`, so the axis flips. The
    // radius comes from the major-axis vector, not the trailing u-parameter
    // scale double, which diverges on offset-derived surfaces.
    let mut shrinking = base();
    shrinking.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([4.655, 0.0, 0.0]), // |major| = 4.655 cm
        Token::Double(1.0),
        Token::Double(-0.5), // sine
        Token::Double(0.866_025_4),
        Token::Double(5.055), // u-parameter scale, not the radius
    ]);
    let (geo, inward) = decode_surface(&rec("cone", shrinking)).unwrap();
    assert!(!inward, "positive cosine keeps the outward normal");
    match geo {
        SolvedSurfaceGeometry::Cone(cone_surface) => {
            let axis = cone_surface.frame().axis().as_raw();
            let radius = cone_surface.radius().get();
            let half_angle = cone_surface.half_angle().get();
            assert!((half_angle - 0.5f64.atan2(0.866_025_4)).abs() < 1.0e-12);
            assert_eq!(axis.z, -1.0, "negative slope flips the axis");
            assert!((radius - 46.55).abs() < 1.0e-12);
        }
        other => panic!("expected cone, got {other:?}"),
    }

    // sphere: the signed radius identifies a concave carrier and is preserved.
    let mut sph = base();
    sph.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Double(-1.0), // concave
        Token::Vector3([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
    ]);
    let (geo, signed) = decode_surface(&rec("sphere", sph)).unwrap();
    assert!(!signed);
    match geo {
        SolvedSurfaceGeometry::Sphere(sphere_surface) => {
            let axis = sphere_surface.frame().axis().as_raw();
            let ref_direction = sphere_surface.frame().reference().as_raw();
            let radius = sphere_surface.radius().get();
            assert_eq!(radius, -10.0);
            assert_eq!(*axis, cadmpeg_ir::math::Vector3::new(0.0, 0.0, 1.0));
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
        }
        other => panic!("expected sphere, got {other:?}"),
    }

    // torus: major/minor ×10; signed minor radius is preserved.
    let mut tor = base();
    tor.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Double(1.0),  // major
        Token::Double(-2.0), // signed minor radius, with |minor| > major
        Token::Vector3([1.0, 0.0, 0.0]),
    ]);
    let (geo, inward_normal) = decode_surface(&rec("torus", tor)).unwrap();
    assert!(inward_normal);
    match geo {
        SolvedSurfaceGeometry::Torus(torus_surface) => {
            let ref_direction = torus_surface.frame().reference().as_raw();
            let major_radius = torus_surface.major_radius().get();
            let minor_radius = torus_surface.minor_radius().get();
            assert_eq!(major_radius, 10.0);
            assert_eq!(minor_radius, -20.0);
            assert_eq!(
                *ref_direction,
                cadmpeg_ir::math::Vector3::new(1.0, 0.0, 0.0)
            );
        }
        other => panic!("expected torus, got {other:?}"),
    }

    // ellipse with ratio 1 → circle; radius = |ref| (cm) ×10.
    let mut circ = base();
    circ.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([3.0, 0.0, 0.0]),
        Token::Double(1.0),
    ]);
    match decode_curve(&rec("ellipse", circ)).unwrap() {
        CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
            let radius = circle_curve.radius().get();
            assert_eq!(radius, 30.0)
        }
        other => panic!("expected circle, got {other:?}"),
    }

    // ellipse with ratio != 1 → ellipse; minor = major·|ratio|.
    let mut ell = base();
    ell.extend([
        Token::Position([0.0, 0.0, 0.0]),
        Token::Vector3([0.0, 0.0, 1.0]),
        Token::Vector3([4.0, 0.0, 0.0]),
        Token::Double(0.5),
    ]);
    match decode_curve(&rec("ellipse", ell)).unwrap() {
        CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
            let major_radius = ellipse_curve.major_radius().get();
            let minor_radius = ellipse_curve.minor_radius().get();
            assert_eq!(major_radius, 40.0);
            assert_eq!(minor_radius, 20.0);
        }
        other => panic!("expected ellipse, got {other:?}"),
    }

    // straight line: origin ×10, unit direction.
    let mut line = vec![refn(), refn(), refn()];
    line.extend([
        Token::Position([1.0, 0.0, 0.0]),
        Token::Vector3([0.0, 1.0, 0.0]),
    ]);
    match decode_curve(&rec("straight", line)).unwrap() {
        CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
            let origin = line_curve.origin().get();
            let direction = *line_curve.direction().as_raw();
            assert_eq!(origin.x, 10.0);
            assert_eq!(direction.y, 1.0);
        }
        other => panic!("expected line, got {other:?}"),
    }
}

#[test]
fn decode_succeeds_when_geometry_present() {
    let f3d = f3d_with_smbh(&synthetic_geometry_smbh());
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();
    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.surfaces.len(), 1);
}

#[test]
fn decode_keeps_face_on_unknown_surface() {
    use cadmpeg_ir::geometry::SolvedSurfaceGeometry;

    // Rename the plane so the face rests on an undecoded carrier.
    let mut smbh = synthetic_geometry_smbh();
    let needle = b"\x0e\x05plane";
    let pos = smbh
        .windows(needle.len())
        .position(|w| w == needle)
        .expect("plane subident present");
    smbh[pos + 2..pos + 7].copy_from_slice(b"splne");

    let f3d = f3d_with_smbh(&smbh);
    let mut cur = Cursor::new(f3d);
    let result = F3dCodec
        .decode(&mut cur, &DecodeOptions::default())
        .unwrap();

    assert!(result.report().geometry_transferred());
    assert_eq!(result.ir().model.faces.len(), 1);
    assert_eq!(result.ir().model.coedges.len(), 3);
    assert_eq!(result.ir().model.vertices.len(), 3);
    assert_eq!(result.ir().model.surfaces.len(), 1);

    let Some(SolvedSurfaceGeometry::Unknown { record }) =
        result.ir().model.surfaces[0].geometry.solved()
    else {
        panic!("expected unknown surface geometry");
    };
    let link = record.as_ref().expect("unknown surface links to a record");
    assert!(
        result
            .ir()
            .native_unknowns("f3d")
            .unwrap()
            .iter()
            .any(|u| u.id == *link),
        "the linked unknown record is present in the arena"
    );

    let note = result
        .report()
        .losses
        .iter()
        .find(|l| l.message.contains("unknown-geometry surface"))
        .expect("unknown-surface loss note present");
    assert_eq!(note.severity, cadmpeg_ir::report::Severity::Warning);
    assert!(note.message.contains("Native kinds: splne=1."));

    // The decoded document still validates.
    let report = cadmpeg_ir::validate::validate_neutral(result.ir(), Vec::new())
        .expect("validate the neutral model");
    assert!(report.is_ok(), "findings: {:?}", report.findings);
}

#[test]
fn cached_unmodeled_spline_families_retain_exact_shape_and_opaque_construction() {
    use cadmpeg_ir::geometry::{ProceduralSurfaceDefinition, SolvedSurfaceGeometry};

    for family in ["VBL_OFFSURF", "offsetvbsur", "skin_spl_sur2"] {
        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&synthetic_exact_spl_sur_smbh(family))),
                &DecodeOptions::default(),
            )
            .unwrap_or_else(|error| panic!("{family} cached decode: {error}"));
        let surface = result
            .ir()
            .model
            .surfaces
            .iter()
            .find(|surface| {
                matches!(
                    surface.geometry.solved_cache(),
                    Some(SolvedSurfaceGeometry::Nurbs(_))
                )
            })
            .unwrap_or_else(|| panic!("{family} must retain its solved NURBS carrier"));
        let procedural = result
            .ir()
            .model
            .procedural_surfaces
            .iter()
            .find(|procedural| {
                result.ir().model.procedural_surface_owner(&procedural.id) == Some(&surface.id)
            })
            .unwrap_or_else(|| panic!("{family} must retain its construction identity"));
        let ProceduralSurfaceDefinition::Unknown {
            record: Some(record),
            ..
        } = procedural.definition()
        else {
            panic!("{family} must retain its opaque construction")
        };
        assert!(result
            .ir()
            .native_unknowns("f3d")
            .unwrap()
            .iter()
            .any(|unknown| unknown.id == *record));
        assert!(!result
            .report()
            .losses
            .iter()
            .any(|loss| loss.message.contains("unknown-geometry surface")));
    }
}

#[test]
fn decode_reports_faces_with_missing_surface_references() {
    for (surface, condition) in [(-1i64, "null-reference=1"), (999, "dangling-reference=1")] {
        let mut smbh = synthetic_mixed_smbh();
        let start = asm_header::record_stream_start(&smbh).unwrap();
        let limit = asm_header::solved_record_limit(&smbh).unwrap();
        let records = cadmpeg_asm::test_support::sab::frame(
            &smbh,
            start,
            limit,
            cadmpeg_asm::kernel_header::RefWidth::Eight,
        )
        .unwrap();
        let face = records
            .iter()
            .filter(|record| record.head() == "face")
            .nth(1)
            .expect("second generated face");
        let record = &mut smbh[face.offset..face.offset + face.len];
        let surface_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
        record[surface_ref + 1..surface_ref + 9].copy_from_slice(&surface.to_le_bytes());

        let result = F3dCodec
            .decode(
                &mut Cursor::new(f3d_with_smbh(&smbh)),
                &DecodeOptions::default(),
            )
            .expect("missing face surface remains an explicitly lossy decode");
        assert_eq!(result.ir().model.faces.len(), 1);
        let note = result
            .report()
            .losses
            .iter()
            .find(|loss| loss.message.contains("required surface reference"))
            .unwrap_or_else(|| {
                panic!(
                    "missing face-surface loss note: {:?}",
                    result.report().losses
                )
            });
        assert!(note.message.contains(condition), "{}", note.message);
    }
}

#[test]
fn decode_reports_undecoded_edge_curve_kinds() {
    let mut smbh = synthetic_geometry_with_procedural_curve_smbh();
    let needle = b"nubs";
    let position = smbh
        .windows(needle.len())
        .position(|window| window == needle)
        .expect("procedural NURBS cache present");
    smbh[position] = b'x';

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("undecoded edge-curve carrier remains a successful topology decode");

    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("no decodable inline B-spline cache"))
        .expect("undecoded edge-curve loss note");
    assert!(
        note.message.contains("Native kinds: intcurve=1."),
        "{}",
        note.message
    );
}

#[test]
fn decode_reports_dangling_edge_curve_references() {
    let mut smbh = synthetic_geometry_smbh();
    let start = asm_header::record_stream_start(&smbh).unwrap();
    let limit = asm_header::solved_record_limit(&smbh).unwrap();
    let records = cadmpeg_asm::test_support::sab::frame(
        &smbh,
        start,
        limit,
        cadmpeg_asm::kernel_header::RefWidth::Eight,
    )
    .unwrap();
    let edge = &records[10];
    let record = &mut smbh[edge.offset..edge.offset + edge.len];
    let curve_ref = record.iter().rposition(|byte| *byte == 0x0c).unwrap();
    record[curve_ref + 1..curve_ref + 9].copy_from_slice(&999i64.to_le_bytes());

    let result = F3dCodec
        .decode(
            &mut Cursor::new(f3d_with_smbh(&smbh)),
            &DecodeOptions::default(),
        )
        .expect("dangling curve reference remains a successful topology decode");
    let note = result
        .report()
        .losses
        .iter()
        .find(|loss| loss.message.contains("no decodable inline B-spline cache"))
        .expect("dangling edge-curve loss note");
    assert!(note.message.contains("Native kinds: dangling-reference=1."));
}
