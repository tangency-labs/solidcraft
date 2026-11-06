// SPDX-License-Identifier: Apache-2.0
//! Source-less edges that store no parameter range.

use std::io::Cursor;

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::target::TargetRequest;
use cadmpeg_ir::codec::write::{EncodeInput, Encoder};
use cadmpeg_ir::codec::{Codec, DecodeOptions};
use cadmpeg_ir::geometry::analytic::CircleCurve;
use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
use cadmpeg_ir::math::{Point3, Vector3};

use crate::F3dCodec;

#[test]
fn generated_source_less_closed_circle_edge_without_range_spans_one_revolution() {
    let source_less = cadmpeg_test_support::solids::solid_cylinder();
    let mut encoded = Vec::new();
    F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded))
        .expect("source-less cylinder encode");
    let round_trip = F3dCodec
        .decode(&mut Cursor::new(encoded), &DecodeOptions::default())
        .expect("source-less cylinder round trip");
    let model = &round_trip.ir().model;
    let circle_ranges = model
        .edges
        .iter()
        .filter(|edge| edge.start == edge.end)
        .filter(|edge| {
            edge.curve().is_some_and(|curve_id| {
                model.curves.iter().any(|curve| {
                    curve.id == *curve_id
                        && matches!(
                            curve.geometry.solved(),
                            Some(SolvedCurveGeometry::Circle(_))
                        )
                })
            })
        })
        .map(|edge| edge.param_range().map(cadmpeg_ir::units::FiniteVector::get))
        .collect::<Vec<_>>();
    assert_eq!(
        circle_ranges,
        vec![Some([0.0, std::f64::consts::TAU]); 2],
        "each closed circle edge spans one revolution"
    );
}

#[test]
fn generated_source_less_closed_edge_off_its_carrier_start_without_range_is_refused() {
    let mut source_less = cadmpeg_test_support::solids::solid_cylinder();
    let bottom = source_less
        .model
        .curves
        .iter_mut()
        .find(|curve| curve.id.as_str().ends_with("#bottom"))
        .expect("the cylinder has a bottom circle");
    bottom.geometry = CurveGeometry::Solved(SolvedCurveGeometry::Circle(
        CircleCurve::try_new(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(0.0, 1.0, 0.0),
            cadmpeg_test_support::solids::CYLINDER_RADIUS,
        )
        .expect("circle"),
    ));
    let mut encoded = Vec::new();
    let result = F3dCodec
        .plan(EncodeInput::new(&source_less, None), TargetRequest::Inherit)
        .and_then(|plan| plan.write_to(&mut encoded));
    assert!(
        matches!(result, Err(CodecError::NotImplemented(_))),
        "a closed edge whose vertex is not at its circle's parameter 0 is refused"
    );
}
