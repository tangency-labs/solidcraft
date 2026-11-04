// SPDX-License-Identifier: Apache-2.0
//! Borrowed wire views for persistent work geometry.

use cadmpeg_ir::native::bytes::NativeBytes;

use super::{
    DesignRecipeReference, DesignVertexRecipe, DesignWorkPlaneConstruction, DesignWorkPointInput,
    DesignWorkPointInputCarrier, DesignWorkPointPlaneSelection,
    DesignWorkPointSketchPointSelection,
};
use serde::{Serialize, Serializer};

#[derive(Serialize)]
struct VertexRecipeRef<'a> {
    record_index: u32,
    byte_offset: u64,
    class_tag: &'a str,
    paired_byte_offset: u64,
    paired_class_tag: &'a str,
    recipe_record_index: u32,
    recipe_record_byte_offset: u64,
    recipe_id: &'a str,
    recipe_prefix_offset: u64,
    recipe_prefix_bytes: NativeBytes<&'a [u8]>,
    recipe_references: &'a [DesignRecipeReference],
    recipe_program_offset: u64,
    recipe_program: &'a [i32],
    #[serde(skip_serializing_if = "Option::is_none")]
    recipe_state_id: Option<i64>,
    #[serde(skip_serializing_if = "Option::is_none")]
    resolved_vertex_slot: Option<i64>,
    next_record_index: u32,
    next_byte_offset: u64,
}

impl Serialize for DesignVertexRecipe {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        VertexRecipeRef {
            record_index: self.record_index(),
            byte_offset: self.byte_offset(),
            class_tag: self.class_tag.as_str(),
            paired_byte_offset: self.paired_byte_offset,
            paired_class_tag: self.paired_class_tag.as_str(),
            recipe_record_index: self.recipe_record_index(),
            recipe_record_byte_offset: self.recipe_record_byte_offset,
            recipe_id: &self.recipe_id,
            recipe_prefix_offset: self.recipe_prefix_offset(),
            recipe_prefix_bytes: (&self.recipe_prefix_bytes).into(),
            recipe_references: &self.recipe_references,
            recipe_program_offset: self.recipe_program_offset,
            recipe_program: &self.recipe_program,
            recipe_state_id: self.resolution.map(|resolution| resolution.state_id),
            resolved_vertex_slot: self
                .resolution
                .map(super::DesignVertexResolution::vertex_slot),
            next_record_index: self.next_record_index(),
            next_byte_offset: self.next_byte_offset,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
struct WorkPointInputRef<'a> {
    record_index: u32,
    reference_offset: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    carrier: Option<WorkPointCarrierRef<'a>>,
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkPointCarrierRef<'a> {
    EdgeRecipe {
        operand_id: &'a str,
    },
    VertexRecipe {
        recipe: &'a DesignVertexRecipe,
    },
    WorkPlane {
        selection: PlaneSelectionRef<'a>,
    },
    SketchPoint {
        selection: SketchPointSelectionRef<'a>,
    },
}

#[derive(Serialize)]
struct PlaneSelectionRef<'a> {
    class_tag: &'a str,
    asset_id: &'a super::DesignRelaxedGuidText,
    asset_id_offset: u64,
    context_id: &'a super::DesignRelaxedGuidText,
    context_id_offset: u64,
    identity_record_index: u32,
    identity_record_offset: u64,
    primary_identity: u64,
    primary_identity_offset: u64,
    work_plane_scope_record_index: u32,
    next_record_index: u32,
    next_byte_offset: u64,
}

impl<'a> PlaneSelectionRef<'a> {
    fn new(selection: &'a DesignWorkPointPlaneSelection, record_index: u32) -> Self {
        Self {
            class_tag: selection.class_tag.as_str(),
            asset_id: &selection.asset_id,
            asset_id_offset: selection.asset_id_offset,
            context_id: &selection.context_id,
            context_id_offset: selection.context_id_offset,
            identity_record_index: record_index + 3,
            identity_record_offset: selection.identity_record_offset,
            primary_identity: selection.primary_identity,
            primary_identity_offset: selection.primary_identity_offset(),
            work_plane_scope_record_index: selection.work_plane_scope_record_index,
            next_record_index: record_index + 4,
            next_byte_offset: selection.next_byte_offset(),
        }
    }
}

#[derive(Serialize)]
struct SketchPointSelectionRef<'a> {
    class_tag: &'a str,
    asset_id: &'a super::DesignRelaxedGuidText,
    asset_id_offset: u64,
    context_id: &'a super::DesignRelaxedGuidText,
    context_id_offset: u64,
    identity_record_index: u32,
    identity_record_offset: u64,
    sketch_record_index: u32,
    sketch_record_index_offset: u64,
    point_persistent_id: u64,
    point_persistent_id_offset: u64,
    point_native_id: &'a str,
    next_record_index: u32,
    next_byte_offset: u64,
}

impl<'a> SketchPointSelectionRef<'a> {
    fn new(selection: &'a DesignWorkPointSketchPointSelection, record_index: u32) -> Self {
        Self {
            class_tag: selection.class_tag.as_str(),
            asset_id: &selection.asset_id,
            asset_id_offset: selection.asset_id_offset,
            context_id: &selection.context_id,
            context_id_offset: selection.context_id_offset,
            identity_record_index: record_index + 3,
            identity_record_offset: selection.identity_record_offset,
            sketch_record_index: selection.sketch_record_index,
            sketch_record_index_offset: selection.sketch_record_index_offset(),
            point_persistent_id: selection.point_persistent_id,
            point_persistent_id_offset: selection.point_persistent_id_offset(),
            point_native_id: &selection.point_native_id,
            next_record_index: record_index + 4,
            next_byte_offset: selection.next_byte_offset(),
        }
    }
}

impl Serialize for DesignWorkPointInput {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let carrier = self.carrier.as_deref().map(|carrier| match carrier {
            DesignWorkPointInputCarrier::EdgeRecipe { operand_id } => {
                WorkPointCarrierRef::EdgeRecipe { operand_id }
            }
            DesignWorkPointInputCarrier::VertexRecipe { recipe } => {
                WorkPointCarrierRef::VertexRecipe { recipe }
            }
            DesignWorkPointInputCarrier::WorkPlane { selection } => {
                WorkPointCarrierRef::WorkPlane {
                    selection: PlaneSelectionRef::new(selection, self.record_index),
                }
            }
            DesignWorkPointInputCarrier::SketchPoint { selection } => {
                WorkPointCarrierRef::SketchPoint {
                    selection: SketchPointSelectionRef::new(selection, self.record_index),
                }
            }
        });
        WorkPointInputRef {
            record_index: self.record_index,
            reference_offset: self.reference_offset,
            carrier,
        }
        .serialize(serializer)
    }
}

#[derive(Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum WorkPlaneConstructionRef<'a> {
    ThreePoint {
        placement_record_index: u32,
        inputs: &'a [DesignVertexRecipe; 3],
    },
}

impl Serialize for DesignWorkPlaneConstruction {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        WorkPlaneConstructionRef::ThreePoint {
            placement_record_index: self.placement_record_index,
            inputs: &self.inputs,
        }
        .serialize(serializer)
    }
}

#[cfg(test)]
mod tests {
    use super::{DesignVertexRecipe, DesignWorkPlaneConstruction, DesignWorkPointInput};
    use serde::Serialize;

    #[derive(Serialize)]
    struct Record<'a, T> {
        id: &'static str,
        value: &'a T,
    }

    fn vertex_recipe() -> DesignVertexRecipe {
        serde_json::from_value(serde_json::json!({
            "record_index": 2,
            "byte_offset": 10,
            "class_tag": "369",
            "paired_byte_offset": 20,
            "paired_class_tag": "261",
            "recipe_record_index": 5,
            "recipe_record_byte_offset": 30,
            "recipe_id": "f3d:design:recipe#1",
            "recipe_prefix_offset": 41,
            "recipe_prefix_bytes": "00ff",
            "recipe_references": [],
            "recipe_program_offset": 43,
            "recipe_program": [0],
            "next_record_index": 7,
            "next_byte_offset": 50
        }))
        .expect("vertex recipe wire")
    }

    #[test]
    fn vertex_recipe_borrowed_json_bytes_match_owned_wire() {
        let recipe = vertex_recipe();
        let owned = super::super::DesignVertexRecipeWire::from(recipe.clone());
        assert_eq!(
            serde_json::to_vec(&recipe).expect("borrowed wire"),
            serde_json::to_vec(&owned).expect("owned wire")
        );
    }

    #[test]
    fn vertex_recipe_native_writer_refuses_retained_limit_before_clone() {
        let recipe = vertex_recipe();
        let owned = super::super::DesignVertexRecipeWire::from(recipe.clone());
        let record = Record {
            id: "f3d:design:vertex-recipe#1",
            value: &recipe,
        };
        super::super::WORK_GEOMETRY_CLONE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "value": owned}),
        );
        super::super::WORK_GEOMETRY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn work_point_input_borrowed_json_bytes_match_owned_wire() {
        let input = DesignWorkPointInput::try_new(
            2,
            12,
            Some(Box::new(super::DesignWorkPointInputCarrier::VertexRecipe {
                recipe: vertex_recipe(),
            })),
        )
        .expect("work point input");
        let owned = super::super::DesignWorkPointInputWire::from(input.clone());
        assert_eq!(
            serde_json::to_vec(&input).expect("borrowed wire"),
            serde_json::to_vec(&owned).expect("owned wire")
        );
        let record = Record {
            id: "f3d:design:work-point-input#1",
            value: &input,
        };
        super::super::WORK_GEOMETRY_CLONE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "value": owned}),
        );
        super::super::WORK_GEOMETRY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }

    #[test]
    fn work_plane_construction_borrowed_json_bytes_match_owned_wire() {
        let recipe = vertex_recipe();
        let inputs = Box::new([recipe.clone(), recipe.clone(), recipe]);
        let plane = DesignWorkPlaneConstruction::try_new(9, inputs)
            .expect("three unresolved vertex recipes");
        let owned = super::super::DesignWorkPlaneConstructionWire::from(plane.clone());
        assert_eq!(
            serde_json::to_vec(&plane).expect("borrowed wire"),
            serde_json::to_vec(&owned).expect("owned wire")
        );
        let record = Record {
            id: "f3d:design:work-plane-construction#1",
            value: &plane,
        };
        super::super::WORK_GEOMETRY_CLONE_COUNT.with(|count| count.set(0));
        cadmpeg_test_support::native_serialization::assert_native_limit(
            &record,
            serde_json::json!({"id": record.id, "value": owned}),
        );
        super::super::WORK_GEOMETRY_CLONE_COUNT.with(|count| assert_eq!(count.get(), 0));
    }
}
