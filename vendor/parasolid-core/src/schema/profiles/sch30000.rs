//! Onshape V30 raw decoding definitions for exact-key runtime selection.
//!
//! Evidence: Siemens XT Format Reference (April 2008), printed pp. 5-19, 31-34,
//! 54-57, 77-78, 87-119; paired Onshape fixtures; self-describing V30 model edits
//! for types 12/19/70/74. Revision 3 adds explicitly maintained V30 geometry
//! definitions, audited against a local exact-key catalog (no runtime dependency).
//! <https://ww3.cad.de/foren/ubb/uploads/schulze/XT_Format_April_2008_tcm73-62642.pdf>
//!
//! Revision 2 adds ellipse and sphere: 24 types / 202 fields. Revision 3 adds
//! 20 complex-geometry/dependency types: 44 types / 356 fields.
//! Names are project-owned. Class 1040 is input-declared; its complete
//! membership is not claimed. Type 74's generic pointers have no class constraint.
//! The Rust table is the maintained source; local M8.1 Python/JSON are snapshots.

use crate::{
    BuiltinProfileCoverage, BuiltinProfileMetadata, BuiltinSchemaProfile, FieldDefinition, FieldType, ParseError, SchemaSource, TypeDefinition,
};

// Checked against canonical compiled definitions by the development harness.
const PROFILE_SHA256: &str = "67e0f3f90c9025c16269c0b03d2365834d949797e1f4c0eb4255b153960b7bf4";

/// Construct the verified subset profile for `SCH_3000000_30000`.
///
/// Evidence covers neutral binary and text inputs with zero user fields, the
/// listed raw types, and box, prism, cylinder and through-hole B-Rep solids.
///
/// # Errors
///
/// Returns `schema.invalid_builtin_profile` if the compiled table is inconsistent.
pub fn onshape_sch30000() -> Result<BuiltinSchemaProfile, ParseError> {
    BuiltinSchemaProfile::new(
        BuiltinProfileMetadata {
            profile_id: "onshape-sch30000-r3".to_owned(),
            revision: 3,
            provider_schema: "30000".to_owned(),
            producer_scope: "Onshape".to_owned(),
            coverage: BuiltinProfileCoverage::VerifiedSubset,
            evidence_manifest_sha256: None,
            profile_sha256: PROFILE_SHA256.to_owned(),
        },
        vec!["SCH_3000000_30000".to_owned()],
        definitions(),
    )
}

fn field(name: &str, code: FieldType, class: u16, count: u32) -> FieldDefinition {
    FieldDefinition { name: name.to_owned(), field_type: code, pointer_class: class, element_count: count, transmitted: true }
}

fn node(node_type: u16, fields: Vec<FieldDefinition>) -> TypeDefinition {
    TypeDefinition::from_fields(node_type, format!("onshape_type_{node_type}"), "Onshape V30 verified raw subset", fields, SchemaSource::Base)
}

#[allow(clippy::too_many_lines)] // The complete reviewed table stays in transmit order.
pub(crate) fn base_definitions() -> Vec<TypeDefinition> {
    use FieldType::{
        Character as C, Double as F, Integer as D, Logical as L, PointerIndex as P, UnicodeCharacter as W, UnsignedByte as U, Vector as V,
    };
    vec![
        // Public reference pp. 87-90 plus model-embedded V30 edits.
        node(
            12,
            vec![
                field("max_local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("attribute_lists", P, 70, 0),
                field("construction_surfaces", P, 1006, 0),
                field("construction_curves", P, 1008, 0),
                field("construction_points", P, 29, 0),
                field("construction_mesh", P, 1006, 0),
                field("construction_polyline", P, 1008, 0),
                field("key_ref", P, 102, 0),
                field("size_precision", F, 0, 0),
                field("linear_precision", F, 0, 0),
                field("instances", P, 11, 0),
                field("next_body", P, 12, 0),
                field("previous_body", P, 12, 0),
                field("storage_state", U, 0, 0),
                field("container_ref", P, 1040, 0),
                field("body_kind", U, 0, 0),
                field("geometry_state", U, 0, 0),
                field("legacy_shell", P, 13, 0),
                field("boundary_surfaces", P, 1006, 0),
                field("boundary_curves", P, 1008, 0),
                field("boundary_points", P, 29, 0),
                field("boundary_mesh", P, 1006, 0),
                field("boundary_polyline", P, 1008, 0),
                field("region_head", P, 19, 0),
                field("edge_head", P, 16, 0),
                field("vertex_head", P, 18, 0),
                field("index_origin", D, 0, 0),
                field("index_values", P, 82, 0),
                field("node_id_values", P, 82, 0),
                field("schema_values", P, 82, 0),
                field("child_body", P, 12, 0),
                field("min_local_id", D, 0, 0),
            ],
        ),
        // Public reference pp. 92-93.
        node(
            13,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("legacy_body", P, 12, 0),
                field("next_shell", P, 13, 0),
                field("back_faces", P, 14, 0),
                field("wire_edges", P, 16, 0),
                field("isolated_vertex", P, 18, 0),
                field("region_ref", P, 19, 0),
                field("front_faces", P, 14, 0),
            ],
        ),
        // Public reference pp. 93-95.
        node(
            14,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("precision", F, 0, 0),
                field("next_back", P, 14, 0),
                field("previous_back", P, 14, 0),
                field("loop_head", P, 15, 0),
                field("back_shell", P, 13, 0),
                field("surface_ref", P, 1006, 0),
                field("orientation", C, 0, 0),
                field("next_surface_face", P, 14, 0),
                field("previous_surface_face", P, 14, 0),
                field("next_front", P, 14, 0),
                field("previous_front", P, 14, 0),
                field("front_shell", P, 13, 0),
            ],
        ),
        // Public reference pp. 95-96.
        node(
            15,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("fin_ref", P, 17, 0),
                field("face_ref", P, 14, 0),
                field("next_loop", P, 15, 0),
            ],
        ),
        // Public reference pp. 98-99.
        node(
            16,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("precision", F, 0, 0),
                field("fin_ref", P, 17, 0),
                field("previous_edge", P, 16, 0),
                field("next_edge", P, 16, 0),
                field("curve_ref", P, 1008, 0),
                field("next_curve_edge", P, 16, 0),
                field("previous_curve_edge", P, 16, 0),
                field("owner_ref", P, 1029, 0),
            ],
        ),
        // Public reference pp. 96-97.
        node(
            17,
            vec![
                field("annotations", P, 1019, 0),
                field("loop_ref", P, 15, 0),
                field("forward_fin", P, 17, 0),
                field("backward_fin", P, 17, 0),
                field("end_vertex", P, 18, 0),
                field("radial_fin", P, 17, 0),
                field("edge_ref", P, 16, 0),
                field("curve_ref", P, 1008, 0),
                field("next_vertex_fin", P, 17, 0),
                field("orientation", C, 0, 0),
            ],
        ),
        // Public reference pp. 97-98.
        node(
            18,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("fin_head", P, 17, 0),
                field("previous_vertex", P, 18, 0),
                field("next_vertex", P, 18, 0),
                field("point_ref", P, 29, 0),
                field("precision", F, 0, 0),
                field("owner_ref", P, 1029, 0),
            ],
        ),
        // Public reference pp. 91-92 plus model-embedded V30 edits.
        node(
            19,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("body_ref", P, 12, 0),
                field("next_region", P, 19, 0),
                field("previous_region", P, 19, 0),
                field("shell_head", P, 13, 0),
                field("region_kind", C, 0, 0),
                field("owner_ref", P, 12, 0),
            ],
        ),
        // Public reference pp. 9-10, 77-78.
        node(
            29,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1011, 0),
                field("next_point", P, 29, 0),
                field("previous_point", P, 29, 0),
                field("position", V, 0, 0),
            ],
        ),
        // Public reference pp. 31-32.
        node(
            30,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1010, 0),
                field("next_curve", P, 1008, 0),
                field("previous_curve", P, 1008, 0),
                field("indirect_owner", P, 141, 0),
                field("orientation", C, 0, 0),
                field("origin", V, 0, 0),
                field("tangent", V, 0, 0),
            ],
        ),
        // Public reference pp. 32-34.
        node(
            31,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1010, 0),
                field("next_curve", P, 1008, 0),
                field("previous_curve", P, 1008, 0),
                field("indirect_owner", P, 141, 0),
                field("orientation", C, 0, 0),
                field("center", V, 0, 0),
                field("normal", V, 0, 0),
                field("x_direction", V, 0, 0),
                field("radius", F, 0, 0),
            ],
        ),
        // Public reference pp. 34-35; V30 paired inputs put sense BEFORE centre,
        // unlike the printed ELLIPSE struct. Producer geometry confirms this
        // order independently, including negative sense and nonzero centres.
        node(
            32,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1010, 0),
                field("next_curve", P, 1008, 0),
                field("previous_curve", P, 1008, 0),
                field("indirect_owner", P, 141, 0),
                field("orientation", C, 0, 0),
                field("center", V, 0, 0),
                field("normal", V, 0, 0),
                field("x_direction", V, 0, 0),
                field("major_radius", F, 0, 0),
                field("minor_radius", F, 0, 0),
            ],
        ),
        // Public reference pp. 54-55.
        node(
            50,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1007, 0),
                field("next_surface", P, 1006, 0),
                field("previous_surface", P, 1006, 0),
                field("indirect_owner", P, 141, 0),
                field("orientation", C, 0, 0),
                field("origin", V, 0, 0),
                field("normal", V, 0, 0),
                field("x_direction", V, 0, 0),
            ],
        ),
        // Public reference pp. 55-57.
        node(
            51,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1007, 0),
                field("next_surface", P, 1006, 0),
                field("previous_surface", P, 1006, 0),
                field("indirect_owner", P, 141, 0),
                field("orientation", C, 0, 0),
                field("origin", V, 0, 0),
                field("axis", V, 0, 0),
                field("radius", F, 0, 0),
                field("x_direction", V, 0, 0),
            ],
        ),
        // Public reference pp. 59-60. Radius precedes the two axis vectors.
        node(
            53,
            vec![
                field("local_id", D, 0, 0),
                field("annotations", P, 1019, 0),
                field("owner_ref", P, 1007, 0),
                field("next_surface", P, 1006, 0),
                field("previous_surface", P, 1006, 0),
                field("indirect_owner", P, 141, 0),
                field("orientation", C, 0, 0),
                field("center", V, 0, 0),
                field("radius", F, 0, 0),
                field("axis", V, 0, 0),
                field("x_direction", V, 0, 0),
            ],
        ),
        // Public reference pp. 100-101 plus model-embedded V30 edits.
        node(
            70,
            vec![
                field("local_id", D, 0, 0),
                field("entry_kind", U, 0, 0),
                field("transmission_flag", L, 0, 0),
                field("owner_ref", P, 1013, 0),
                field("next_list", P, 70, 0),
                field("previous_list", P, 70, 0),
                field("entry_count", D, 0, 0),
                field("block_capacity", D, 0, 0),
                field("cursor_index", D, 0, 0),
                field("cursor_block", P, 1012, 0),
                field("first_block", P, 1012, 0),
            ],
        ),
        // Public reference pp. 101-102 plus model-embedded V30 edits.
        node(74, vec![field("used_count", D, 0, 0), field("index_origin", D, 0, 0), field("next_block", P, 74, 0), field("entries", P, 0, 1)]),
        // Public reference pp. 102.
        node(79, vec![field("identifier_bytes", C, 0, 1)]),
        // Public reference pp. 103-107.
        node(
            80,
            vec![
                field("next_definition", P, 80, 0),
                field("identifier_ref", P, 79, 0),
                field("kind_id", D, 0, 0),
                field("event_actions", U, 0, 8),
                field("field_labels", P, 99, 0),
                field("allowed_owners", L, 0, 14),
                field("value_kinds", U, 0, 1),
            ],
        ),
        // Public reference pp. 107-110.
        node(
            81,
            vec![
                field("local_id", D, 0, 0),
                field("definition_ref", P, 80, 0),
                field("owner_ref", P, 1015, 0),
                field("next_annotation", P, 1019, 0),
                field("previous_annotation", P, 1019, 0),
                field("next_same_kind", P, 81, 0),
                field("previous_same_kind", P, 81, 0),
                field("value_arrays", P, 1018, 1),
            ],
        ),
        // Public reference pp. 110.
        node(82, vec![field("integers", D, 0, 1)]),
        // Public reference pp. 110.
        node(83, vec![field("reals", F, 0, 1)]),
        // Public reference pp. 110.
        node(84, vec![field("text_bytes", C, 0, 1)]),
        // Public reference pp. 111.
        node(98, vec![field("utf16_units", W, 0, 1)]),
    ]
}

// V30 complex geometry: public XT reference pp. 39-53, 57-62, 67-72,
// 112, 116-118; V30 records independently compared with the caller catalog.
// Keep the base table separate: V13 reuses only that explicitly bounded list.
fn definitions() -> Vec<TypeDefinition> {
    use FieldType::{Character as C, Double as F, Integer as D, PointerIndex as P, UnsignedByte as U, Vector as V};
    let mut result = base_definitions();
    let additional = [38, 40, 41, 52, 133, 141];
    let mut geometry: Vec<_> = super::sch13006::definitions().into_iter().filter(|d| additional.contains(&d.node_type)).collect();
    geometry.extend(super::sch13006::sp_curve_definitions());
    geometry.extend(super::sch13006::bspline_surface_definitions());
    for mut definition in geometry {
        definition.name = format!("onshape_type_{}", definition.node_type);
        "V30 complex geometry dependency".clone_into(&mut definition.description);
        match definition.node_type {
            // V30 adds the retained UV-data reference to the V13 intersection.
            38 => definition.fields.push(field("intersection_data", P, 204, 0)),
            41 => definition.fields.insert(1, field("limit_state", C, 0, 0)),
            // V30 widens the old individual analytic form into a pointer class.
            135 => definition.fields[1].pointer_class = 1036,
            125 => {
                for (i, class) in [1030, 1031, 1032, 1033].into_iter().enumerate() {
                    definition.fields[17 + i].pointer_class = class;
                }
            }
            _ => {}
        }
        result.push(definition);
    }
    // Torus shares the reviewed surface prefix, followed by its exact frame/radii.
    let mut torus = vec![
        field("local_id", D, 0, 0),
        field("annotations", P, 1019, 0),
        field("owner_ref", P, 1007, 0),
        field("next_surface", P, 1006, 0),
        field("previous_surface", P, 1006, 0),
        field("indirect_owner", P, 141, 0),
        field("orientation", C, 0, 0),
    ];
    torus.extend([
        field("center", V, 0, 0),
        field("axis", V, 0, 0),
        field("major_radius", F, 0, 0),
        field("minor_radius", F, 0, 0),
        field("x_direction", V, 0, 0),
    ]);
    result.push(node(54, torus));
    for kind in [87, 89] {
        result.push(node(kind, vec![field("values", V, 0, 1)]));
    }
    // This two-field layout is also independently present in embedded V30 data
    // (tests/test_builtin_intersection_data_runtime.py); no UV semantics invented.
    result.push(node(204, vec![field("uv_kind", U, 0, 0), field("values", F, 0, 1)]));
    result
}
