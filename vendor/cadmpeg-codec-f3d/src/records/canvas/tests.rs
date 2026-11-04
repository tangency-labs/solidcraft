// SPDX-License-Identifier: Apache-2.0

#[test]
fn canvas_prologue_reconstructs_both_flags_and_fixed_zero_bytes() {
    for first_flag in [0, 1] {
        for visible in [0, 1] {
            let mut bytes = [0; 15];
            bytes[10] = first_flag;
            bytes[14] = visible;
            let prologue = crate::records::canvas::DesignCanvasPrologue::try_from(bytes)
                .expect("Canvas prologue");
            assert_eq!(prologue.bytes(), bytes);
            assert_eq!(prologue.visible(), visible != 0);
        }
    }
    for offset in 0..15 {
        let mut bytes = [0; 15];
        bytes[offset] = if matches!(offset, 10 | 14) { 2 } else { 1 };
        assert!(crate::records::canvas::DesignCanvasPrologue::try_from(bytes).is_err());
    }
}

#[test]
fn canvas_geometry_prologue_decodes_visibility_in_both_forms() {
    use crate::records::canvas::DesignCanvasPrologue;
    let mut expanded = [0; 15];
    expanded[14] = 1;
    assert!(DesignCanvasPrologue::try_from(expanded).is_ok());
    assert_eq!(
        DesignCanvasPrologue::try_from(expanded)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(true)
    );

    expanded[14] = 0;
    assert_eq!(
        DesignCanvasPrologue::try_from(expanded)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(false)
    );

    let mut compact = [0; 15];
    compact[10] = 1;
    assert!(DesignCanvasPrologue::try_from(compact).is_ok());
    assert_eq!(
        DesignCanvasPrologue::try_from(compact)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(false)
    );

    compact[14] = 1;
    assert_eq!(
        DesignCanvasPrologue::try_from(compact)
            .ok()
            .map(DesignCanvasPrologue::visible),
        Some(true)
    );

    compact[11] = 1;
    assert!(DesignCanvasPrologue::try_from(compact).is_err());
}

#[test]
fn canvas_geometry_payload_preserves_source_float_bits() {
    let mut bytes = [0; 77];
    bytes[..4].copy_from_slice(&(-0.0_f32).to_le_bytes());
    for (offset, value) in [
        (5, -0.0_f64),
        (13, 2.5),
        (21, -3.5),
        (29, 1.0),
        (37, -0.0),
        (45, 0.0),
        (53, -0.0),
        (61, 0.0),
        (69, 1.0),
    ] {
        bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
    }
    let payload = crate::records::canvas::DesignCanvasGeometryPayload::try_from(bytes.as_slice())
        .expect("Canvas payload");
    assert_eq!(payload.bytes(), bytes);
    let (opacity, frame) = payload.decoded();
    let origin = frame.origin().get();
    let u_axis = frame.u_axis();
    let v_axis = frame.v_axis();
    assert_eq!(opacity.to_bits(), (-0.0_f32).to_bits());
    assert_eq!(origin.x.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(origin.y, 25.0);
    assert_eq!(origin.z, -35.0);
    assert_eq!(u_axis.as_raw().y.to_bits(), (-0.0_f64).to_bits());
    assert_eq!(v_axis.as_raw().x.to_bits(), (-0.0_f64).to_bits());
    assert!(crate::records::canvas::DesignCanvasGeometryPayload::try_from(&bytes[..76]).is_err());
}

fn canvas_wire_fixture() -> serde_json::Value {
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
        "second_boundary_present_offset": 280, "plane_entity_suffix": 200,
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

fn canvas_image_fixture() -> super::DesignCanvasImage {
    let mut wire = canvas_wire_fixture();
    wire["id"] = serde_json::json!("f3d:native:canvas#0");
    serde_json::from_value(wire).unwrap()
}

#[test]
fn canvas_image_borrowed_wire_matches_owned_wire_bytes() {
    let image = canvas_image_fixture();
    let owned = super::DesignCanvasImageWire::from(image.clone());
    assert_eq!(
        serde_json::to_vec(&image).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn canvas_image_native_retained_limit_refuses_before_record_clone() {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy, ResourceDimension};

    let image = canvas_image_fixture();
    let arena_name = "design_canvas_images";
    let needed = serde_json::to_vec(&image).unwrap().len();
    let arena = DecodeArena::new();
    let mut policy = DecodePolicy::service();
    policy.limits.max_retained_bytes = u64::try_from(needed).unwrap() - 1;
    let (limited, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let mut namespace = cadmpeg_ir::NativeNamespace::default();
    super::CANVAS_IMAGE_CLONE_COUNT.with(|count| count.set(0));
    let error = namespace
        .set_arena(&limited, arena_name, std::slice::from_ref(&image))
        .unwrap_err();
    super::CANVAS_IMAGE_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    assert!(matches!(
        cadmpeg_core::CodecError::from(error),
        cadmpeg_core::CodecError::ResourceLimit(limit)
            if limit.dimension == ResourceDimension::RetainedBytes
                && limit.operation == "serialize native record"
    ));

    let (service, _) =
        DecodeContext::from_root_bytes(&[], &arena, &DecodePolicy::service()).unwrap();
    namespace
        .set_arena(&service, arena_name, std::slice::from_ref(&image))
        .unwrap();
    assert_eq!(namespace.arenas()[arena_name].len(), 1);
}

#[test]
fn canvas_image_wire_derives_visibility_and_geometry_values() {
    let base = canvas_wire_fixture();
    for first_flag in [0, 1] {
        for visible in [false, true] {
            let mut value = base.clone();
            let mut prologue: cadmpeg_ir::native::bytes::NativeBytes =
                serde_json::from_value(value["geometry_prologue"].clone())
                    .expect("Canvas prologue bytes");
            prologue[10] = first_flag;
            prologue[14] = u8::from(visible);
            value["geometry_prologue"] = serde_json::json!(prologue);
            value["visible"] = serde_json::json!(visible);
            let wire: crate::records::canvas::DesignCanvasImageWire =
                serde_json::from_value(value).expect("Canvas wire");
            let expected = serde_json::to_string(&wire).expect("Canvas wire bytes");
            let image: crate::records::canvas::DesignCanvasImage =
                serde_json::from_str(&expected).expect("Canvas image");
            assert_eq!(
                serde_json::to_string(&image).expect("Canvas image bytes"),
                expected
            );
        }
    }
    for geometry_reference_offset in [424, 428] {
        let mut value = base.clone();
        value["geometry_reference_offset"] = serde_json::json!(geometry_reference_offset);
        let wire: crate::records::canvas::DesignCanvasImageWire =
            serde_json::from_value(value).expect("Canvas scope form");
        let expected = serde_json::to_string(&wire).expect("Canvas scope wire");
        let image: crate::records::canvas::DesignCanvasImage =
            serde_json::from_str(&expected).expect("Canvas scope binding");
        assert_eq!(image.scope_byte_offset(), 402);
        assert_eq!(
            serde_json::to_string(&image).expect("Canvas scope bytes"),
            expected
        );
    }
    let mut unicode = base.clone();
    unicode["label"] = serde_json::json!("A😀");
    unicode["asset_name"] = serde_json::json!("图😀.png");
    for (field, value) in [
        ("geometry_frame_length", 223),
        ("paired_geometry_byte_offset", 323),
        ("paired_component_reference_offset", 343),
        ("asset_byte_offset", 353),
        ("asset_name_offset", 378),
        ("geometry_reference_offset", 414),
    ] {
        unicode[field] = serde_json::json!(value);
    }
    let wire: crate::records::canvas::DesignCanvasImageWire =
        serde_json::from_value(unicode).expect("Canvas Unicode wire");
    let expected = serde_json::to_string(&wire).expect("Canvas Unicode bytes");
    let image: crate::records::canvas::DesignCanvasImage =
        serde_json::from_str(&expected).expect("Canvas Unicode frame");
    assert_eq!(image.scope_byte_offset(), 392);
    assert_eq!(
        serde_json::to_string(&image).expect("Canvas Unicode output"),
        expected
    );
    for field in [
        "scope_reference_offset",
        "visibility_offset",
        "geometry_frame_length",
        "paired_geometry_byte_offset",
        "paired_component_reference_offset",
        "second_boundary_present_offset",
        "plane_reference_offset",
        "component_reference_offset",
        "asset_reference_offset",
        "asset_byte_offset",
        "asset_name_offset",
        "label_offset",
        "geometry_reference_offset",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!(0);
        let error = serde_json::from_value::<crate::records::canvas::DesignCanvasImage>(value)
            .expect_err("misplaced Canvas field")
            .to_string();
        assert!(error.contains(field));
    }
    for field in [
        "geometry_class_tag",
        "paired_geometry_class_tag",
        "asset_class_tag",
        "label",
        "asset_name",
    ] {
        let mut value = base.clone();
        value[field] = serde_json::json!("");
        let error = serde_json::from_value::<crate::records::canvas::DesignCanvasImage>(value)
            .expect_err("empty Canvas field")
            .to_string();
        assert!(error.contains(field));
    }
    let mut repeated_record = base.clone();
    repeated_record["asset_record_index"] = serde_json::json!(101);
    let error =
        serde_json::from_value::<crate::records::canvas::DesignCanvasImage>(repeated_record)
            .expect_err("repeated Canvas record identity")
            .to_string();
    assert!(error.contains("asset_record_index"));
    let mut overflow = base.clone();
    overflow["geometry_byte_offset"] = serde_json::json!(u64::MAX);
    let error = serde_json::from_value::<crate::records::canvas::DesignCanvasImage>(overflow)
        .expect_err("Canvas extent overflow")
        .to_string();
    assert!(error.contains("geometry_byte_offset"));
    let mut boundary_offset = base.clone();
    boundary_offset["boundary_coordinate_offsets"][0] = serde_json::json!(0);
    let error =
        serde_json::from_value::<crate::records::canvas::DesignCanvasImage>(boundary_offset)
            .expect_err("Canvas boundary offset")
            .to_string();
    assert!(error.contains("boundary_coordinate_offsets"));
    for (field, replacement) in [
        ("visible", serde_json::json!(false)),
        ("opacity", serde_json::json!(0.5)),
        ("origin", serde_json::json!({"x":11.0,"y":20.0,"z":30.0})),
        ("u_axis", serde_json::json!({"x":1.0,"y":1.0,"z":0.0})),
        ("v_axis", serde_json::json!({"x":0.0,"y":0.0,"z":0.0})),
        (
            "boundary_segments",
            serde_json::json!([[{"u":-2.0,"v":-1.0},{"u":3.0,"v":-1.0}],[{"u":-2.0,"v":4.0},{"u":2.0,"v":4.0}]]),
        ),
    ] {
        let mut value = base.clone();
        value[field] = replacement;
        let error = serde_json::from_value::<crate::records::canvas::DesignCanvasImage>(value)
            .expect_err("inconsistent decoded Canvas value")
            .to_string();
        assert!(error.contains(field));
    }
}

#[test]
fn canvas_geometry_payload_decodes_opacity_and_plane_frame() {
    use crate::records::canvas::DesignCanvasGeometryPayload;
    use cadmpeg_ir::math::{Point3, Vector3};
    let mut payload = [0; 77];
    payload[..4].copy_from_slice(&0.75f32.to_le_bytes());
    for (offset, value) in [
        (5, 1.0f64),
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

    assert_eq!(
        DesignCanvasGeometryPayload::try_from(payload.as_slice())
            .ok()
            .map(|payload| {
                let (opacity, frame) = payload.decoded();
                (
                    opacity,
                    frame.origin().get(),
                    *frame.u_axis().as_raw(),
                    *frame.v_axis().as_raw(),
                )
            }),
        Some((
            0.75,
            Point3::new(10.0, 20.0, 30.0),
            Vector3::new(1.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
        ))
    );

    payload[4] = 1;
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[4] = 0;
    payload[53..61].copy_from_slice(&1.0f64.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[53..61].copy_from_slice(&0.0f64.to_le_bytes());
    payload[29..37].copy_from_slice(&f64::NAN.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
    payload[29..37].copy_from_slice(&1.0f64.to_le_bytes());
    payload[5..13].copy_from_slice(&1.0e308f64.to_le_bytes());
    assert!(DesignCanvasGeometryPayload::try_from(payload.as_slice()).is_err());
}

#[test]
fn canvas_placement_preserves_ordered_corners_in_model_space() {
    use crate::records::canvas::DesignCanvasBounds;
    use cadmpeg_ir::features::FeatureUnitPlaneFrame;
    use cadmpeg_ir::math::{Point2, Point3, Vector3};
    const EPS_CORNER: f64 = 1.0e-10;
    let plane = FeatureUnitPlaneFrame::new(
        Point3::new(10.0, 20.0, 30.0),
        Vector3::new(0.0, 1.0, 0.0),
        Vector3::new(0.0, 0.0, 1.0),
    )
    .unwrap();
    for (e, f) in [
        ((5.0, 0.0), (0.0, 2.0)),
        ((-5.0, 0.0), (0.0, 2.0)),
        ((5.0, 0.0), (0.0, -2.0)),
        ((-5.0, 0.0), (0.0, -2.0)),
        ((0.0, 5.0), (-2.0, 0.0)),
        ((0.0, -5.0), (2.0, 0.0)),
        ((3.0, 4.0), (-8.0, 6.0)),
        ((-3.0, -4.0), (-8.0, 6.0)),
    ] {
        let a = Point2::new(3.0, -1.0);
        let b = Point2::new(a.u + e.0, a.v + e.1);
        let c = Point2::new(a.u + f.0, a.v + f.1);
        let d = Point2::new(b.u + f.0, b.v + f.1);
        let segments = [[a, b], [c, d]];
        let native = DesignCanvasBounds::try_from(segments).unwrap();
        assert_eq!(native.segments(), segments);
        let (frame, bounds) = native.image_placement(plane).unwrap();
        let [minimum, maximum] = bounds.corners().map(cadmpeg_ir::units::FinitePoint2::get);
        assert_eq!(minimum, Point2::new(0.0, 0.0));
        for (uv, expected) in [
            ((0.0, 0.0), a),
            ((maximum.u, 0.0), b),
            ((0.0, maximum.v), c),
            ((maximum.u, maximum.v), d),
        ] {
            let actual = frame
                .origin()
                .get()
                .translated(*frame.u_axis().as_raw(), uv.0)
                .translated(*frame.v_axis().as_raw(), uv.1);
            let expected = Point3::new(10.0, 20.0 + 10.0 * expected.u, 30.0 + 10.0 * expected.v);
            assert!(
                actual.distance(expected) < EPS_CORNER,
                "{actual:?} != {expected:?}"
            );
        }
    }
}

#[test]
fn canvas_bounds_reject_nonrectangles_and_nonfinite_coordinates() {
    use crate::records::canvas::DesignCanvasBounds;
    use cadmpeg_ir::math::Point2;
    for segments in [
        [
            [Point2::new(0.0, 0.0), Point2::new(0.0, 0.0)],
            [Point2::new(0.0, 1.0), Point2::new(0.0, 1.0)],
        ],
        [
            [Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            [Point2::new(0.5, 1.0), Point2::new(1.5, 1.0)],
        ],
        [
            [Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            [Point2::new(0.0, 1.0), Point2::new(2.0, 1.0)],
        ],
    ] {
        assert!(DesignCanvasBounds::try_from(segments).is_err());
    }
    for invalid in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        assert!(DesignCanvasBounds::try_from([
            [Point2::new(0.0, 0.0), Point2::new(1.0, 0.0)],
            [Point2::new(0.0, 1.0), Point2::new(invalid, 1.0)],
        ])
        .is_err());
    }
}

#[test]
fn canvas_bounds_admit_coordinate_roundoff_and_reject_overflow() {
    use crate::records::canvas::DesignCanvasBounds;
    use cadmpeg_ir::math::Point2;
    assert!(DesignCanvasBounds::try_from([
        [Point2::new(-2.0, -1.0), Point2::new(3.0, -1.0)],
        [
            Point2::new(f64::from_bits((-2.0f64).to_bits() + 4), 4.0),
            Point2::new(f64::from_bits(3.0f64.to_bits() + 4), 4.0)
        ],
    ])
    .is_ok());
    assert!(DesignCanvasBounds::try_from([
        [Point2::new(-f64::MAX, 0.0), Point2::new(f64::MAX, 0.0)],
        [Point2::new(-f64::MAX, 1.0), Point2::new(f64::MAX, 1.0)],
    ])
    .is_err());
}
