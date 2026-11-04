// SPDX-License-Identifier: Apache-2.0
//! Historical face contexts: every binding stage is complete.

use cadmpeg_core::decode::index_from_u32;

#[test]
fn historical_loop_wire_preserves_each_complete_binding_stage() {
    for count in [0_u32, 1, 3] {
        for stage in 0..4 {
            let mut wire = serde_json::json!({
                "loop_slot": 10,
                "coedge_slots": (0..count).map(|index| 20 + index).collect::<Vec<_>>(),
                "edge_slots": (0..count).map(|index| 30 + index).collect::<Vec<_>>()
            });
            if count != 0 {
                if stage >= 1 {
                    wire["vertex_slots"] =
                        serde_json::json!((0..count).map(|index| 40 + index).collect::<Vec<_>>());
                }
                if stage >= 2 {
                    wire["point_slots"] =
                        serde_json::json!((0..count).map(|index| 50 + index).collect::<Vec<_>>());
                }
                if stage >= 3 {
                    wire["positions"] = serde_json::json!((0..count)
                        .map(|index| cadmpeg_ir::math::Point3::new(f64::from(index), 0.0, 0.0))
                        .collect::<Vec<_>>());
                }
            }
            let context: super::DesignHistoricalFaceLoopContext =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.boundary.coedges().count(), index_from_u32(count));
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            for field in ["edge_slots", "vertex_slots", "point_slots", "positions"] {
                let mut invalid = wire.clone();
                let mut values = invalid
                    .get(field)
                    .and_then(serde_json::Value::as_array)
                    .cloned()
                    .unwrap_or_default();
                for _ in 0..=count {
                    values.push(if field == "positions" {
                        serde_json::to_value(cadmpeg_ir::math::Point3::new(9.0, 0.0, 0.0)).unwrap()
                    } else {
                        serde_json::json!(99)
                    });
                }
                invalid[field] = serde_json::Value::Array(values);
                assert!(
                    serde_json::from_value::<super::DesignHistoricalFaceLoopContext>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains(field)
                );
            }
        }
    }
}

fn historical_loop_for_serialization(
    count: u32,
    stage: u32,
) -> super::DesignHistoricalFaceLoopContext {
    let mut wire = serde_json::json!({
        "loop_slot": 10,
        "coedge_slots": (0..count).map(|index| 20 + index).collect::<Vec<_>>(),
        "edge_slots": (0..count).map(|index| 30 + index).collect::<Vec<_>>()
    });
    if stage >= 1 {
        wire["vertex_slots"] =
            serde_json::json!((0..count).map(|index| 40 + index).collect::<Vec<_>>());
    }
    if stage >= 2 {
        wire["point_slots"] =
            serde_json::json!((0..count).map(|index| 50 + index).collect::<Vec<_>>());
    }
    if stage >= 3 {
        wire["positions"] = serde_json::json!((0..count)
            .map(|index| cadmpeg_ir::math::Point3::new(f64::from(index), 0.0, 0.0))
            .collect::<Vec<_>>());
    }
    serde_json::from_value(wire).unwrap()
}

#[test]
fn historical_loop_borrowed_wire_matches_owned_wire_bytes() {
    for count in [0, 1, 3] {
        for stage in 0..4 {
            let loop_context = historical_loop_for_serialization(count, stage);
            let owned = super::DesignHistoricalFaceLoopWire::from(loop_context.clone());
            assert_eq!(
                serde_json::to_vec(&loop_context).unwrap(),
                serde_json::to_vec(&owned).unwrap()
            );
        }
    }
}

#[test]
fn historical_loop_native_retained_limit_refuses_before_clone() {
    #[derive(serde::Serialize)]
    struct NestedRecord<'a> {
        id: &'static str,
        value: &'a super::DesignHistoricalFaceLoopContext,
    }
    let loop_context = historical_loop_for_serialization(3, 3);
    let record = NestedRecord {
        id: "f3d:native:historical-loop#0",
        value: &loop_context,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::HISTORICAL_FACE_LOOP_CLONE_COUNT.with(|count| count.set(0)),
        || super::HISTORICAL_FACE_LOOP_CLONE_COUNT.with(std::cell::Cell::get),
    );
}
