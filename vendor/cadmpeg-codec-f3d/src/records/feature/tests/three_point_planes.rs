// SPDX-License-Identifier: Apache-2.0

use cadmpeg_core::decode::u64_from_index;

use crate::records::feature::work_geometry::{
    DesignVertexRecipe, DesignVertexResolution as Resolution, DesignWorkPlaneConstruction as Plane,
};

fn inputs() -> Box<[DesignVertexRecipe; 3]> {
    Box::new(std::array::from_fn(|ordinal| {
        let record_index = 2 + u32::try_from(ordinal).expect("fixture value fits u32") * 5;
        let base = 10 + u64_from_index(ordinal) * 100;
        serde_json::from_value(serde_json::json!({
            "record_index": record_index, "byte_offset": base, "class_tag": "369",
            "paired_byte_offset": base + 10, "paired_class_tag": "261",
            "recipe_record_index": record_index + 3, "recipe_record_byte_offset": base + 20,
            "recipe_id": format!("vertex-{ordinal}"), "recipe_prefix_offset": base + 31,
            "recipe_prefix_bytes": "00ff", "recipe_references": [],
            "recipe_program_offset": base + 33, "recipe_program": [0],
            "next_record_index": record_index + 5, "next_byte_offset": base + 40
        }))
        .unwrap()
    }))
}

#[test]
fn three_point_planes_reject_every_partial_resolution_at_both_admission_routes() {
    for mask in 0..8 {
        let mut inputs = inputs();
        for (ordinal, input) in inputs.iter_mut().enumerate() {
            if mask & (1 << ordinal) != 0 {
                input.resolution =
                    Resolution::new(4, i64::try_from(ordinal).expect("fixture value fits i64"));
            }
        }
        let wire = serde_json::json!({"kind": "three_point", "placement_record_index": 9, "inputs": inputs});
        let valid = mask == 0 || mask == 7;
        assert_eq!(
            Plane::try_new(9, inputs).is_ok(),
            valid,
            "native mask {mask}"
        );
        assert_eq!(
            serde_json::from_value::<Plane>(wire).is_ok(),
            valid,
            "serde mask {mask}"
        );
    }
}

#[test]
fn three_point_resolution_requires_one_state_and_distinct_vertex_slots() {
    let mut plane = Plane::try_new(9, inputs()).unwrap();
    let resolved = [0, 1, 2].map(|slot| Resolution::new(i64::MIN, slot).unwrap());
    plane.try_set_resolution(resolved).unwrap();
    let before = plane.clone();
    for (replacement, field) in [
        (Resolution::new(5, 1).unwrap(), "recipe_state_id"),
        (
            Resolution::new(i64::MIN, 0).unwrap(),
            "resolved_vertex_slot",
        ),
    ] {
        let mut invalid = resolved;
        invalid[1] = replacement;
        assert!(plane
            .try_set_resolution(invalid)
            .unwrap_err()
            .contains(field));
        assert_eq!(plane, before);
        let mut inputs = Box::new(before.inputs().clone());
        inputs[1].resolution = Some(replacement);
        let wire = serde_json::json!({"kind": "three_point", "placement_record_index": 9, "inputs": inputs});
        assert!(Plane::try_new(9, inputs).unwrap_err().contains(field));
        assert!(serde_json::from_value::<Plane>(wire)
            .unwrap_err()
            .to_string()
            .contains(field));
    }
    let wire = serde_json::to_value(&plane).unwrap();
    assert_eq!(serde_json::from_value::<Plane>(wire).unwrap(), plane);
    plane.clear_resolution();
    assert!(plane
        .inputs()
        .iter()
        .all(|input| input.resolution.is_none()));
}
