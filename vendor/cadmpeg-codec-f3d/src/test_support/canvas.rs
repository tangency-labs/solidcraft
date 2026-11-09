// SPDX-License-Identifier: Apache-2.0

pub(crate) fn canvas_wire_fixture() -> serde_json::Value {
    let mut payload = [0; 77];
    payload[..4].copy_from_slice(&0.75_f32.to_le_bytes());
    for (offset, value) in [
        (5, 1.0_f64),
        (13, 2.0),
        (21, 3.0),
        (29, 1.0),
        (37, 0.0),
        (45, 0.0),
        (53, 0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        payload[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let mut prologue = [0; 15];
    prologue[14] = 1;
    let base = serde_json::json!({
        "id": "canvas", "scope_record_index": 103, "scope_reference_offset": 247,
        "geometry_class_tag": "256", "geometry_record_index": 101,
        "geometry_reference_offset": 424, "geometry_byte_offset": 100,
        "geometry_prologue": cadmpeg_ir::native::bytes::NativeBytes::from(prologue), "visible": true, "visibility_offset": 125,
        "geometry_frame_length": 229, "paired_geometry_class_tag": "257",
        "paired_geometry_byte_offset": 329, "paired_component_reference_offset": 349,
        "boundary_segments": [[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],[{"u":-2.0,"v":4.0},{"u":3.0,"v":4.0}]],
        "boundary_coordinate_offsets": [126,134,142,150,281,289,297,305],
        "second_boundary_flag": true, "second_boundary_flag_offset": 280, "plane_entity_suffix": 200,
        "plane_reference_offset": 159, "component_entity_suffix": 201,
        "component_reference_offset": 258, "asset_class_tag": "258", "asset_record_index": 102,
        "asset_reference_offset": 270, "asset_byte_offset": 359, "asset_name": "image.png",
        "asset_name_offset": 384, "label": "Canvas", "label_offset": 317,
        "opacity": 0.75, "origin": {"x":10.0,"y":20.0,"z":30.0},
        "u_axis": {"x":1.0,"y":0.0,"z":0.0}, "v_axis": {"x":0.0,"y":0.0,"z":1.0},
        "geometry_payload": cadmpeg_ir::native::bytes::NativeBytes::from(payload.as_slice())
    });
    base
}

pub(crate) fn canvas_image_fixture() -> crate::records::canvas::DesignCanvasImage {
    let mut wire = canvas_wire_fixture();
    wire["id"] = serde_json::json!("f3d:native:canvas#0");
    serde_json::from_value(wire).unwrap()
}
