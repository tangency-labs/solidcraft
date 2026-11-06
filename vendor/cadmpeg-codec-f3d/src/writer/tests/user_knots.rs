// SPDX-License-Identifier: Apache-2.0
use crate::F3dCodec;
use cadmpeg_ir::{
    codec::{Codec, DecodeOptions},
    scalar::NonNegativeReal,
    subd::SubdUserKnot,
};
use std::io::Cursor;

#[test]
fn retained_f3d_round_trips_user_knot_edits_and_extension_removal() {
    let source = crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_form(
        &crate::test_support::smbh_geometry_test::synthetic_geometry_smbh(),
    );
    let decoded = F3dCodec
        .decode(&mut Cursor::new(&source), &DecodeOptions::default())
        .expect("source decode");
    let (mut edited, _, fidelity) = decoded.into_parts();
    let mut unsupported = edited.clone();
    unsupported.model.subds[0]
        .cage
        .edit_vertices(|vertices| {
            vertices[0].set_point(
                cadmpeg_ir::features::FinitePoint3::new(cadmpeg_ir::math::Point3::new(
                    17.0, 0.0, 0.0,
                ))
                .expect("finite control point"),
            );
            Ok(())
        })
        .expect("admitted cage vertex edit");
    let error = crate::test_support::plan_inherited_write(&unsupported, &fidelity, &mut Vec::new())
        .expect_err("a user-knot patch must not approve other cage edits");
    assert!(
        matches!(error, cadmpeg_core::CodecError::NotImplemented(message)
        if message.contains("unchanged cage geometry and topology"))
    );

    edited.model.subds[0]
        .cage
        .edit_edges(|edges| {
            edges[0].user_knot = Some(SubdUserKnot::Override(
                NonNegativeReal::new(0.0).expect("zero"),
            ));
            Ok(())
        })
        .expect("user-knot edit");
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(&edited, &fidelity, &mut encoded)
        .expect("write user knots");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(&encoded), &DecodeOptions::default())
        .expect("read user knots");
    assert_eq!(round_trip.ir().model.subds, edited.model.subds);
    let (mut removed, _, fidelity) = round_trip.into_parts();
    removed.model.subds[0]
        .cage
        .edit_edges(|edges| {
            for edge in edges {
                edge.user_knot = None;
            }
            Ok(())
        })
        .expect("remove extension");
    let mut encoded = Vec::new();
    crate::test_support::plan_inherited_write(&removed, &fidelity, &mut encoded)
        .expect("write extension removal");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("read extension removal");
    assert_eq!(round_trip.ir().model.subds, removed.model.subds);
}
