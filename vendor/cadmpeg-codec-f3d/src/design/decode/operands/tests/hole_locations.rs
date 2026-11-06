// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::operands::bind_hole_construction_sketch_points;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::hole::DesignHoleConstruction;
use crate::records::feature::work_geometry::DesignWorkPointInputCarrier;
use crate::records::sketch_geometry::SketchPoint;
use crate::test_support::indexed_header;
use crate::test_support::lp_utf16;
use cadmpeg_ir::math::Point2;
use std::collections::HashMap;

const STREAM: &str = "f3d:Design/BulkStream.dat";
const INPUT: u32 = 100;
const SKETCH: u32 = 1627;
const POINT: u64 = 379;

/// The direct sketch-point envelope of input `INPUT`, closed by its
/// point-data record `INPUT + 4`.
fn envelope() -> Vec<u8> {
    let mut bytes = Vec::new();
    indexed_header(&mut bytes, *b"338", INPUT);
    bytes.extend_from_slice(&[0; 10]);
    bytes.push(1);
    bytes.extend_from_slice(&(INPUT + 3).to_le_bytes());
    bytes.extend_from_slice(&[0; 6]);
    bytes.extend_from_slice(&1u32.to_le_bytes());
    lp_utf16(&mut bytes, "7b3dec6f-f69c-4bfa-a537-9274f341c66e");
    lp_utf16(&mut bytes, "2b40eee7-408c-429b-9216-8b6f7e9a62c9");
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 4]);
    indexed_header(&mut bytes, *b"258", INPUT);
    indexed_header(&mut bytes, *b"294", INPUT + 1);
    indexed_header(&mut bytes, *b"303", INPUT + 2);
    indexed_header(&mut bytes, *b"305", INPUT + 3);
    bytes.extend_from_slice(&[0; 9]);
    bytes.push(1);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&SKETCH.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&u32::try_from(POINT).unwrap().to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    indexed_header(&mut bytes, *b"288", INPUT + 4);
    bytes
}

fn header() -> DesignRecordHeader {
    DesignRecordHeader {
        id: format!("{STREAM}:design-record#{INPUT}"),
        record_index: INPUT,
        class_tag: crate::records::references::DesignClassTag::try_from("338".to_owned()).unwrap(),
        byte_offset: 0,
    }
}

fn sketch_point(stream: &str, record_index: u32, owner: u32, persistent_id: u64) -> SketchPoint {
    SketchPoint::try_from(crate::records::sketch_geometry::SketchPointDraft {
        id: format!("{stream}:sketch-point#{record_index}"),
        record_index,
        owner_reference: Some(owner),
        class_tag: crate::records::references::DesignClassTag::try_from("256".to_owned()).unwrap(),
        byte_offset: u64::from(record_index),
        coordinate_offset: 141,
        companion: crate::records::sketch_geometry::SketchPointCompanion {
            incident_curves: Vec::new(),
        },
        record_form: crate::records::sketch_geometry::SketchPointRecordForm::version11(
            persistent_id,
            crate::records::sketch_geometry::SketchPointClosure::Selector0State0,
            Some(2),
            0.0,
        ),
        paired_reference: 0,
        coordinates: Point2::new(0.0, 0.0),
    })
    .unwrap()
}

/// A sketch-point Hole carrier whose one location is the point-data record
/// `INPUT + 4` with vertex input `INPUT`.
fn construction() -> DesignHoleConstruction {
    serde_json::from_value(serde_json::json!({
        "point_record_index": 1, "point_record_byte_offset": 2,
        "position": [0.0, 0.0, 0.0], "position_offset": 3,
        "direction": [0.0, 0.0, 1.0], "direction_offset": 4,
        "point_parameters": [0.0, 0.0], "point_parameter_offsets": [5, 6],
        "reference_type": 13, "reference_type_offset": 7,
        "input_record_indices": [8], "input_record_offsets": [9],
        "locations": [{
            "record_index": INPUT + 4, "position": [1.0, 2.0, 0.0],
            "position_offset": 10, "reference_type": 10,
            "vertex_input": {"record_index": INPUT, "reference_offset": 11}
        }]
    }))
    .unwrap()
}

/// The native id of the sketch point bound to the location input, if any.
fn bound_point(headers: &[DesignRecordHeader], sketch_points: &[SketchPoint]) -> Option<String> {
    let headers: HashMap<_, _> = headers
        .iter()
        .map(|header| ((STREAM, header.record_index), header))
        .collect();
    let mut construction = construction();
    bind_hole_construction_sketch_points(
        &envelope(),
        STREAM,
        &mut construction,
        &headers,
        sketch_points,
    )
    .unwrap();
    match construction.locations[0].vertex_input.as_ref()?.carrier()? {
        DesignWorkPointInputCarrier::SketchPoint { selection } => {
            assert_eq!(selection.sketch_record_index, SKETCH);
            assert_eq!(selection.point_persistent_id, POINT);
            Some(selection.point_native_id.clone())
        }
        _ => None,
    }
}

#[test]
fn a_hole_location_input_binds_the_sketch_point_its_envelope_names() {
    let point = sketch_point(STREAM, 7, SKETCH, POINT);
    let others = [
        sketch_point(STREAM, 8, SKETCH, POINT + 1),
        sketch_point(STREAM, 9, SKETCH + 1, POINT),
        sketch_point("f3d:Design2/BulkStream.dat", 7, SKETCH, POINT),
    ];
    let mut points = others.to_vec();
    points.push(point.clone());
    assert_eq!(bound_point(&[header()], &points), Some(point.id.clone()));
}

#[test]
fn a_hole_location_input_without_one_named_sketch_point_stays_unbound() {
    let point = sketch_point(STREAM, 7, SKETCH, POINT);
    assert_eq!(bound_point(&[], std::slice::from_ref(&point)), None);
    assert_eq!(
        bound_point(&[header()], &[sketch_point(STREAM, 8, SKETCH, POINT + 1)]),
        None
    );
    assert_eq!(
        bound_point(
            &[header()],
            &[point.clone(), sketch_point(STREAM, 8, SKETCH, POINT)]
        ),
        None
    );
    let mut displaced = header();
    displaced.byte_offset = 1;
    assert_eq!(bound_point(&[displaced], &[point]), None);
}
