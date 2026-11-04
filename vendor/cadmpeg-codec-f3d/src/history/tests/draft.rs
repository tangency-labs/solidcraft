// SPDX-License-Identifier: Apache-2.0
#![allow(clippy::unwrap_used)]

use crate::history::resolve_draft_face_by_surface_transition;
use crate::history_records::AsmHistoricalTopology;

#[test]
fn face_transition_requires_one_changed_surface_geometry() {
    use crate::history_records::{AsmHistoricalCarrierBinding, AsmHistoricalPlane};
    use crate::records::{
        dimensions::DesignRecipeReference, recipes::ConstructionRecipeKind,
        topology::face::DesignFaceOperand,
    };
    use cadmpeg_ir::ids::FaceId;
    use cadmpeg_ir::math::{Point3, Vector3};

    let face = |slot| FaceId::mint(format!("f3d:brep:entity#{slot}")).expect("identity grammar");
    let reference = |candidate_faces, alternate_selector_faces| DesignRecipeReference {
        selector: 2,
        selector_offset: 0,
        token: "1".into(),
        token_offset: 0,
        design_reference: 303,
        design_reference_offset: 0,
        candidate_faces,
        candidate_edges: Vec::new(),
        alternate_selector_faces,
        alternate_selector_edges: Vec::new(),
    };
    let mut operand: DesignFaceOperand = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:face-operand#1",
        "scope_record_index": 10,
        "scope_reference_ordinal": 0,
        "record_index": 1,
        "byte_offset": 0,
        "class_tag": "414",
        "paired_byte_offset": 100,
        "paired_class_tag": "258",
        "recipe_record_index": 4,
        "recipe_record_byte_offset": 116,
        "recipe_id": "f3d:test:recipe#4",
        "recipe_prefix_offset": 127,
        "recipe_prefix_bytes": "",
        "recipe_references": [],
        "recipe_kind": "bounded_face",
        "recipe_program_offset": 0,
        "recipe_program": [0, -1, 1],
        "recipe_node_offsets": [0],
        "recipe_nodes": [{
            "byte_offset": 0,
            "end_byte_offset": 12,
            "program": [0, -1, 1],
            "recipe_structure": null
        }],
        "candidate_faces": ["f3d:brep:entity#10", "f3d:brep:entity#11"],
        "preceding_candidate_faces": [],
        "changed_candidate_faces": [],
        "historical_support_contexts": [],
        "resolved_face_slots": [],
        "next_record_index": 5,
        "next_byte_offset": 200
    }))
    .expect("Draft face operand");
    operand.recipe_kind = ConstructionRecipeKind::BoundedFace;
    operand.recipe_references = vec![reference(Vec::new(), vec![face(10), face(11)])];

    let plane = |surface, normal| AsmHistoricalPlane {
        surface,
        origin: Point3::new(0.0, 0.0, 0.0),
        normal,
    };
    let preceding = AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 100,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 101,
            },
        ],
        surface_planes: vec![
            plane(100, Vector3::new(0.0, 0.0, 1.0)),
            plane(101, Vector3::new(0.0, 0.0, 1.0)),
        ],
        ..AsmHistoricalTopology::default()
    };
    let result = AsmHistoricalTopology {
        faces: vec![10, 11],
        face_surfaces: vec![
            AsmHistoricalCarrierBinding {
                entity: 10,
                carrier: 200,
            },
            AsmHistoricalCarrierBinding {
                entity: 11,
                carrier: 201,
            },
        ],
        surface_planes: vec![
            plane(200, Vector3::new(0.0, -0.1, 0.995)),
            plane(201, Vector3::new(0.0, 0.0, 1.0)),
        ],
        ..AsmHistoricalTopology::default()
    };

    assert_eq!(
        resolve_draft_face_by_surface_transition(&operand, &preceding, &result),
        Some(10)
    );

    let mut ambiguous = result.clone();
    ambiguous.surface_planes[1] = plane(201, Vector3::new(0.0, 0.1, 0.995));
    assert_eq!(
        resolve_draft_face_by_surface_transition(&operand, &preceding, &ambiguous),
        None
    );

    let mut exact = operand;
    exact.candidate_faces = vec![face(10), face(11)];
    exact.recipe_references = vec![reference(vec![face(10)], Vec::new())];
    assert_eq!(
        resolve_draft_face_by_surface_transition(&exact, &preceding, &preceding),
        Some(10)
    );
}
