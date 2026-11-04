// SPDX-License-Identifier: Apache-2.0
//! Writer-domain synthetic tests.
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

use std::io::Cursor;

use cadmpeg_ir::codec::{Codec, DecodeOptions};

use crate::test_support::native_test::{f3d_native, f3d_native_mut};
use crate::test_support::smbh_geometry_test::{
    synthetic_geometry_smbh, synthetic_geometry_with_body_color_smbh,
    synthetic_geometry_with_body_decimal_color_chain_smbh,
    synthetic_geometry_with_body_truecolor_chain_smbh, synthetic_geometry_with_face_color_smbh,
    synthetic_geometry_with_transform_smbh,
};
use crate::test_support::zip_test::f3d_with_smbh;
use crate::F3dCodec;

#[test]
fn generated_f3d_rewrites_body_transform() {
    let source = f3d_with_smbh(&synthetic_geometry_with_transform_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    assert_eq!(f3d_native(decoded.ir()).transform_hints.len(), 1);
    assert!(!f3d_native(decoded.ir()).transform_hints[0].rotation);
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::transform::Transform::affine([
        [1.0, 0.0, 0.0, 125.0],
        [0.0, 1.0, 0.0, -75.0],
        [0.0, 0.0, 1.0, 50.0],
    ])
    .expect("affine transform");
    edited.model.bodies[0].transform = Some(expected);
    f3d_native_mut(&mut edited).transform_hints[0].reflection = true;
    f3d_native_mut(&mut edited).body_native_keys[0].asm_body_key = Some(84);

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("body-transform regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.bodies[0].transform, Some(expected));
    assert!(!f3d_native(round_trip.ir()).transform_hints[0].rotation);
    assert!(f3d_native(round_trip.ir()).transform_hints[0].reflection);
    assert_eq!(
        f3d_native(round_trip.ir()).body_native_keys[0].asm_body_key,
        Some(84)
    );
}

#[test]
fn body_key_edit_does_not_rewrite_ordinal_design_selector() {
    let body = cadmpeg_ir::ids::BodyId::mint("f3d:brep:entity#1").expect("identity grammar");
    let mut baseline = crate::native::F3dNative::default();
    baseline
        .body_native_keys
        .push(cadmpeg_asm::brep::records::BodyNativeKey {
            source_namespace: cadmpeg_asm::brep::records::identity::NativeRecordNamespace::new(
                crate::ids::ID_FORMAT,
            ),
            body: body.clone(),
            record_index: 1,
            body_ordinal: 0,
            source_brep: Some("BREP.source.smb".into()),
            asm_body_key: Some(436),
        });
    baseline.body_visibilities.push(
        crate::records::bodies::BodyVisibility::try_from(
            crate::records::bodies::BodyVisibilityWire {
                id: "f3d:design:body-visibility#0".into(),
                body,
                stream: "Design1/BulkStream.dat".into(),
                byte_offset: 20,
                asm_body_key_offset: 40,
                asm_body_key: 0,
                entity_suffix: 1,
                visible: true,
            },
        )
        .unwrap(),
    );
    let mut target = baseline.clone();
    target.body_native_keys[0].asm_body_key = Some(500);

    let edits = crate::writer::patch::edits::validate_body_native_key_edits(
        crate::writer::patch::edits::PatchNatives {
            baseline: Some(&baseline),
            target: Some(&target),
        },
    )
    .expect("body-key edit");

    assert_eq!(edits.asm.get(&1), Some(&500));
    assert!(edits.design.is_empty());
}

#[test]
fn generated_f3d_rewrites_body_rgb_color() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color::new(0.7, 0.4, 0.2, 1.0).expect("valid color");
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("body-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_the_winning_truecolor_attribute() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_truecolor_chain_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated truecolor F3D decode");
    assert_eq!(
        decoded.ir().model.bodies[0].color,
        Some(
            cadmpeg_ir::topology::Color::new(32.0 / 255.0, 64.0 / 255.0, 96.0 / 255.0, 1.0)
                .expect("valid color")
        )
    );
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected =
        cadmpeg_ir::topology::Color::new(64.0 / 255.0, 128.0 / 255.0, 192.0 / 255.0, 1.0)
            .expect("valid color");
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("truecolor regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated truecolor decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rewrites_fixed_width_decimal_color_text() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_decimal_color_chain_smbh(
        "04227264",
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated decimal-color F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color::new(1.0 / 255.0, 2.0 / 255.0, 3.0 / 255.0, 1.0)
        .expect("valid color");
    edited.model.bodies[0].color = Some(expected);

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("decimal-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated decimal-color decode");
    assert_eq!(round_trip.ir().model.bodies[0].color, Some(expected));
}

#[test]
fn generated_f3d_rejects_lossy_truecolor_edit() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_truecolor_chain_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated truecolor F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.bodies[0].color = Some(
        cadmpeg_ir::topology::Color::new(0.5, 64.0 / 255.0, 96.0 / 255.0, 1.0)
            .expect("valid color"),
    );

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("nonrepresentable truecolor edit must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rejects_decimal_color_text_growth() {
    let source = f3d_with_smbh(&synthetic_geometry_with_body_decimal_color_chain_smbh(
        "255",
    ));
    let decoded = F3dCodec
        .decode(&mut Cursor::new(source), &DecodeOptions::default())
        .expect("generated decimal-color F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.bodies[0].color =
        Some(cadmpeg_ir::topology::Color::new(1.0, 0.0, 0.0, 1.0).expect("valid color"));

    let error = crate::test_support::plan_inherited_write(&edited, &fidelity, &mut Vec::new())
        .expect_err("wider decimal-color text must be rejected");
    assert!(matches!(error, cadmpeg_core::CodecError::NotImplemented(_)));
}

#[test]
fn generated_f3d_rewrites_face_rgb_color_and_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_with_face_color_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let expected = cadmpeg_ir::topology::Color::new(0.6, 0.3, 0.9, 1.0).expect("valid color");
    edited.model.faces[0].color = Some(expected);
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("face-color regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(round_trip.ir().model.faces[0].color, Some(expected));
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}

#[test]
fn generated_f3d_rewrites_edge_parameter_range() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.edges[0].carrier = cadmpeg_ir::topology::EdgeCarrier::new(
        edited.model.edges[0].curve().cloned(),
        Some([-2.5, 4.75]),
    )
    .unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("edge-range regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir().model.edges[0]
            .param_range()
            .map(cadmpeg_ir::units::FiniteVector::get),
        Some([-2.5, 4.75])
    );
}

#[test]
fn generated_f3d_rewrites_edge_native_metadata() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let owner = edited.model.coedges[0].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.edge_continuities[0].continuity = "tangent".into();
        native.edge_continuities[0].sense = cadmpeg_ir::topology::Sense::Reversed;
        native.edge_ownerships[0].owner_coedge = Some(owner.clone());
    }

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("edge-continuity regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        f3d_native(round_trip.ir()).edge_continuities[0].continuity,
        "tangent"
    );
    assert_eq!(
        f3d_native(round_trip.ir()).edge_continuities[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        f3d_native(round_trip.ir()).edge_ownerships[0].owner_coedge,
        Some(owner)
    );
}

#[test]
fn generated_f3d_rewrites_vertex_ownership() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let replacement = edited.model.edges[1].id.clone();
    {
        let mut native = f3d_native_mut(&mut edited);
        native.vertex_ownerships[1].owning_edge = replacement.clone();
        native.vertex_ownerships[1].endpoint_index =
            cadmpeg_asm::brep::records::EndpointSlot::Start;
    }

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("vertex-ownership regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let ownership = &f3d_native(round_trip.ir()).vertex_ownerships[1];
    assert_eq!(ownership.owning_edge, replacement);
    assert_eq!(ownership.endpoint_index.code(), 0);
}

#[test]
fn generated_f3d_rewrites_face_and_coedge_sense() {
    let source = f3d_with_smbh(&synthetic_geometry_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("generated F3D decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    edited.model.faces[0].sense = cadmpeg_ir::topology::Sense::Reversed;
    edited.model.coedges[0].sense = cadmpeg_ir::topology::Sense::Reversed;

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("orientation regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    assert_eq!(
        round_trip.ir().model.faces[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
    assert_eq!(
        round_trip.ir().model.coedges[0].sense,
        cadmpeg_ir::topology::Sense::Reversed
    );
}

#[test]
fn generated_f3d_rewrites_a_subset_edge_range_on_its_local_parameter() {
    use crate::test_support::smbh_curves_test::synthetic_geometry_with_subset_curve_smbh;

    let source = f3d_with_smbh(&synthetic_geometry_with_subset_curve_smbh());
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("subset curve decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let owner = edited
        .model
        .procedural_curves
        .iter()
        .find(|procedural| {
            matches!(
                procedural.definition(),
                cadmpeg_ir::geometry::ProceduralCurveDefinition::Subset(_)
            )
        })
        .and_then(|procedural| edited.model.procedural_curve_owner(&procedural.id))
        .cloned()
        .expect("subset curve");
    let index = edited
        .model
        .edges
        .iter()
        .position(|edge| edge.curve() == Some(&owner))
        .expect("edge on the subset curve");
    edited.model.edges[index].carrier =
        cadmpeg_ir::topology::EdgeCarrier::new(Some(owner), Some([0.5, 4.0])).unwrap();

    let mut regenerated = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut regenerated)
        .expect("subset edge-range regeneration");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(regenerated), &DecodeOptions::default())
        .expect("regenerated F3D decode");
    let [start, end] = round_trip.ir().model.edges[index]
        .param_range()
        .unwrap()
        .get();
    assert!((start - 0.5).abs() < 1.0e-12 && (end - 4.0).abs() < 1.0e-12);
}
