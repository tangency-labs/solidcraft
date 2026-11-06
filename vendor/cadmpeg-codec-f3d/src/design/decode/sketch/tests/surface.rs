// SPDX-License-Identifier: Apache-2.0
use crate::design::decode::sketch::parse_sketch_surface;
use cadmpeg_ir::math::Point3;

fn tested_parse_sketch_surface(
    payload: &[u8],
    record_at: usize,
) -> Result<Option<crate::design::decode::sketch::ParsedSketchSurface>, cadmpeg_core::CodecError> {
    parse_sketch_surface(payload, record_at)
}

fn canonical_surface_payload() -> Vec<u8> {
    let mut payload = vec![0; 315];
    payload[20] = 1;
    payload[21..25].copy_from_slice(&2u32.to_le_bytes());
    payload[25..29].copy_from_slice(&13u32.to_le_bytes());
    payload[29..42].copy_from_slice(b"EntityGenesis");
    payload[42..46].copy_from_slice(&23u32.to_le_bytes());
    payload[46..69].copy_from_slice(b"IntrinsicMetaTypeuint64");
    payload[69..77].copy_from_slice(&17u64.to_le_bytes());
    payload[77..81].copy_from_slice(&11u32.to_le_bytes());
    payload[81..92].copy_from_slice(b"surface_tag");
    payload[92..96].copy_from_slice(&23u32.to_le_bytes());
    payload[96..119].copy_from_slice(b"IntrinsicMetaTypeuint64");
    payload[119..127].copy_from_slice(&29u64.to_le_bytes());
    payload[127..131].copy_from_slice(&4u32.to_le_bytes());
    let coordinates = [
        0.0f64, 0.0, 0.0, 0.0, 2.0, 0.0, 3.0, 0.0, 0.0, 3.0, 2.0, 1.0,
    ];
    for (index, coordinate) in coordinates.into_iter().enumerate() {
        let at = 131 + index * 8;
        payload[at..at + 8].copy_from_slice(&coordinate.to_le_bytes());
    }
    let degrees_at = 131 + coordinates.len() * 8;
    payload[degrees_at..degrees_at + 4].copy_from_slice(&1u32.to_le_bytes());
    payload[degrees_at + 4..degrees_at + 8].copy_from_slice(&1u32.to_le_bytes());
    payload[degrees_at + 8..degrees_at + 12].copy_from_slice(&4u32.to_le_bytes());
    let mut at = degrees_at + 12;
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        payload[at..at + 8].copy_from_slice(&knot.to_le_bytes());
        at += 8;
    }
    payload[at..at + 4].copy_from_slice(&4u32.to_le_bytes());
    at += 4;
    for knot in [0.0f64, 0.0, 1.0, 1.0] {
        payload[at..at + 8].copy_from_slice(&knot.to_le_bytes());
        at += 8;
    }
    payload[at..at + 4].copy_from_slice(&2u32.to_le_bytes());
    payload[at + 4..at + 8].copy_from_slice(&2u32.to_le_bytes());

    payload
}

#[test]
fn sketch_surface_parser_recovers_tensor_product_grid() {
    let payload = canonical_surface_payload();
    let surface = tested_parse_sketch_surface(&payload, 0)
        .expect("surface admission")
        .expect("canonical surface payload");
    assert_eq!(surface.entity_genesis, Some(17));
    assert_eq!(surface.persistent_id.get(), 29);
    assert_eq!(
        (
            surface.geometry.u_degree.get(),
            surface.geometry.v_degree.get()
        ),
        (1, 1)
    );
    assert_eq!(
        surface
            .geometry
            .u_knots
            .iter()
            .copied()
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .collect::<Vec<_>>(),
        [0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(
        surface
            .geometry
            .v_knots
            .iter()
            .copied()
            .map(cadmpeg_ir::scalar::FiniteReal::get)
            .collect::<Vec<_>>(),
        [0.0, 0.0, 1.0, 1.0]
    );
    assert_eq!(surface.geometry.control_points.len(), 2);
    assert_eq!(surface.geometry.control_points[0].len(), 2);
    assert_eq!(
        surface.geometry.control_points[1][1].get(),
        Point3::new(30.0, 20.0, 10.0)
    );
}

#[test]
fn sketch_surface_parser_refuses_scaled_coordinate_overflow() {
    let mut payload = canonical_surface_payload();
    payload[131..139].copy_from_slice(&f64::MAX.to_le_bytes());
    let error = tested_parse_sketch_surface(&payload, 0).expect_err("scaled coordinate overflow");
    assert!(matches!(error, cadmpeg_core::CodecError::Malformed(_)));
    assert!(error
        .to_string()
        .contains("control point 0 overflows millimetres"));
}
