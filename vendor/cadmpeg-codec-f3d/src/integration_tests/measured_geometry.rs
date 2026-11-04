// SPDX-License-Identifier: Apache-2.0
//! Measured body geometry across a source-less F3D write round trip,
//! compared without entity identities.

use std::io::Cursor;

use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::CadIr;
use cadmpeg_measure::test_support::round_trip::measures_agree;

use crate::F3dCodec;

/// `ir` written as a source-less F3D archive and decoded again.
fn round_tripped(ir: &CadIr) -> CadIr {
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(ir, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less encode");
    F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("decode the written archive")
        .ir()
        .clone()
}

/// A solid cylinder with radius 10 and height 30 whose closed circle edges
/// store no parameter range.
#[test]
fn source_less_solid_cylinder_keeps_measured_geometry() {
    let ir = cadmpeg_test_support::solids::solid_cylinder();
    let written = round_tripped(&ir);
    assert_eq!(written.model.bodies.len(), ir.model.bodies.len());
    measures_agree("cylinder", &ir, &written).expect("the round trip keeps its measured geometry");
}

/// The 10 mm cube under a rigid placement.
#[test]
fn source_less_placed_cube_keeps_measured_geometry() {
    let mut ir = cadmpeg_ir::examples::unit_cube().expect("unit cube fixture is admitted");
    ir.model.bodies[0].transform = Some(
        cadmpeg_ir::transform::Transform::affine([
            [0.0, -1.0, 0.0, 20.0],
            [1.0, 0.0, 0.0, -30.0],
            [0.0, 0.0, 1.0, 40.0],
        ])
        .expect("affine transform"),
    );
    let written = round_tripped(&ir);
    assert_eq!(written.model.bodies.len(), ir.model.bodies.len());
    measures_agree("placed cube", &ir, &written)
        .expect("the round trip keeps its measured geometry");
}
