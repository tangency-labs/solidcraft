// SPDX-License-Identifier: Apache-2.0
//! Edge-recipe selector contexts, recipe structures and their sidecar counts.

#[derive(serde::Serialize)]
struct NestedRecord<'a, T: serde::Serialize> {
    id: &'static str,
    value: &'a T,
}

fn selector_context() -> super::DesignEdgeRecipeSelectorContext {
    serde_json::from_value(serde_json::json!({
        "selector": 3,
        "clause_entries": [null],
        "clause_triplet_edge_slots": [null],
        "incidence_matching_edge_slots": [7],
        "unique_incidence_edge_slot": 7,
        "boundary_count_matching_edge_slots": [7, 8]
    }))
    .unwrap()
}

fn surface_patch_clause() -> super::DesignSurfacePatchRecipeClause {
    serde_json::from_str(r#"{"fields":[[0],[0],[2,0],[0,0],[0],[0,0]],"face_reference_ordinals":[0,0],"edge_reference_ordinals":[0,0],"payload_entry_count":0,"entries":[]}"#).unwrap()
}

fn topology_recipe_side() -> super::DesignTopologyRecipeSide {
    serde_json::from_str(r#"{"field_count":2,"header_value":1,"scalars":[0],"payload_prefix":[0],"payload_entry_count":0,"entries":[]}"#).unwrap()
}

#[test]
fn edge_recipe_selector_context_borrowed_wire_matches_owned_wire_bytes() {
    let context = selector_context();
    let owned = super::DesignEdgeRecipeSelectorContextWire::from(context.clone());
    assert_eq!(
        serde_json::to_vec(&context).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn edge_recipe_selector_context_native_retained_limit_refuses_before_clone() {
    let context = selector_context();
    let record = NestedRecord {
        id: "f3d:native:selector-context#0",
        value: &context,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::EDGE_RECIPE_SELECTOR_CONTEXT_CLONE_COUNT.with(|count| count.set(0)),
        || super::EDGE_RECIPE_SELECTOR_CONTEXT_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn surface_patch_structure_borrowed_wire_matches_owned_wire_bytes() {
    let structure = super::DesignSurfacePatchRecipeStructure {
        clauses: [surface_patch_clause(), surface_patch_clause()],
    };
    let owned = super::DesignSurfacePatchRecipeStructureWire::from(structure.clone());
    assert_eq!(
        serde_json::to_vec(&structure).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn surface_patch_structure_native_retained_limit_refuses_before_clone() {
    let structure = super::DesignSurfacePatchRecipeStructure {
        clauses: [surface_patch_clause(), surface_patch_clause()],
    };
    let record = NestedRecord {
        id: "f3d:native:surface-patch-structure#0",
        value: &structure,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::SURFACE_PATCH_STRUCTURE_CLONE_COUNT.with(|count| count.set(0)),
        || super::SURFACE_PATCH_STRUCTURE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn surface_patch_clause_borrowed_wire_matches_owned_wire_bytes() {
    let clause = surface_patch_clause();
    let owned = super::DesignSurfacePatchRecipeClauseWire::from(clause.clone());
    assert_eq!(
        serde_json::to_vec(&clause).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn surface_patch_clause_native_retained_limit_refuses_before_clone() {
    let clause = surface_patch_clause();
    let record = NestedRecord {
        id: "f3d:native:surface-patch-clause#0",
        value: &clause,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::SURFACE_PATCH_CLAUSE_CLONE_COUNT.with(|count| count.set(0)),
        || super::SURFACE_PATCH_CLAUSE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn topology_recipe_side_borrowed_wire_matches_owned_wire_bytes() {
    let side = topology_recipe_side();
    let owned = super::DesignTopologyRecipeSideWire::from(side.clone());
    assert_eq!(
        serde_json::to_vec(&side).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn topology_recipe_side_native_retained_limit_refuses_before_clone() {
    let side = topology_recipe_side();
    let record = NestedRecord {
        id: "f3d:native:topology-recipe-side#0",
        value: &side,
    };
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "design_parameter_scopes",
        || super::TOPOLOGY_RECIPE_SIDE_CLONE_COUNT.with(|count| count.set(0)),
        || super::TOPOLOGY_RECIPE_SIDE_CLONE_COUNT.with(std::cell::Cell::get),
    );
}

#[test]
fn selector_context_wire_rejects_partial_clauses_and_derives_singleton() {
    let entry = super::DesignTopologyRecipeEntry {
        selector: 3,
        boundary_edge_count: std::num::NonZeroU32::new(4).unwrap(),
        topology_triplets: std::array::from_fn(|_| super::DesignTopologyRecipeTriplet {
            outer: std::num::NonZeroU32::new(3).unwrap(),
            middle: 2,
            incident: Some(super::DesignTopologyIncident {
                ordinal: 1,
                side: super::DesignTopologyIncidentSide::Preceding,
            }),
        }),
    };
    for edges in [vec![], vec![7], vec![7, 8]] {
        for count in [0, 1, 3] {
            let entries: Vec<_> = (0..count)
                .map(|index| (index % 2 == 0).then_some(entry))
                .collect();
            let slots: Vec<_> = (0..count)
                .map(|index| (index % 2 == 0).then(|| [vec![7, 8], vec![7]]))
                .collect();
            let mut wire = serde_json::json!({
                "selector": 3, "clause_entries": entries,
                "clause_triplet_edge_slots": slots,
                "incidence_matching_edge_slots": edges,
                "boundary_count_matching_edge_slots": [7, 8]
            });
            if edges.len() == 1 {
                wire["unique_incidence_edge_slot"] = serde_json::json!(7);
            }
            let context: super::DesignEdgeRecipeSelectorContext =
                serde_json::from_value(wire.clone()).unwrap();
            assert_eq!(context.clauses.len(), count);
            assert_eq!(serde_json::to_value(&context).unwrap(), wire);
            let mut invalid = wire.clone();
            invalid["unique_incidence_edge_slot"] = serde_json::json!(9);
            assert!(
                serde_json::from_value::<super::DesignEdgeRecipeSelectorContext>(invalid)
                    .unwrap_err()
                    .to_string()
                    .contains("unique_incidence_edge_slot")
            );
            for field in ["clause_entries", "clause_triplet_edge_slots"] {
                let mut invalid = wire.clone();
                invalid[field]
                    .as_array_mut()
                    .unwrap()
                    .push(serde_json::Value::Null);
                assert!(
                    serde_json::from_value::<super::DesignEdgeRecipeSelectorContext>(invalid)
                        .unwrap_err()
                        .to_string()
                        .contains("clause_triplet_edge_slots")
                );
                if count != 0 {
                    let mut invalid = wire.clone();
                    invalid[field][0] = serde_json::Value::Null;
                    let error =
                        serde_json::from_value::<super::DesignEdgeRecipeSelectorContext>(invalid)
                            .unwrap_err()
                            .to_string();
                    assert!(error.contains("clause_entries"));
                    assert!(error.contains("clause_triplet_edge_slots"));
                }
            }
        }
    }
}

#[test]
fn topology_recipe_derived_ordinals_preserve_wire_and_reject_conflicts() {
    for field in ["incident_edge_ordinal", "incident_side"] {
        let mut wire = serde_json::json!({"outer":3,"middle":2,"vertex_ordinal":2,"incident_edge_ordinal":1,"incident_side":"preceding"});
        wire.as_object_mut().unwrap().remove(field);
        assert!(
            serde_json::from_value::<super::DesignTopologyRecipeTriplet>(wire)
                .unwrap_err()
                .to_string()
                .contains(field)
        );
    }
    for (outer, vertex) in [(1_u32, 0_u32), (4, 3), (u32::MAX, u32::MAX - 1)] {
        let wire = format!(r#"{{"outer":{outer},"middle":-1,"vertex_ordinal":{vertex}}}"#);
        let triplet: super::DesignTopologyRecipeTriplet = serde_json::from_str(&wire).unwrap();
        assert_eq!(triplet.vertex_ordinal(), vertex);
        assert_eq!(serde_json::to_string(&triplet).unwrap(), wire);
        let mut invalid = serde_json::to_value(triplet).unwrap();
        invalid["vertex_ordinal"] = serde_json::json!(outer);
        assert!(
            serde_json::from_value::<super::DesignTopologyRecipeTriplet>(invalid)
                .unwrap_err()
                .to_string()
                .contains("vertex_ordinal")
        );
    }
    let wire = r#"{"selector":0,"boundary_edge_count":4,"topology_triplets":[{"outer":3,"middle":2,"vertex_ordinal":2,"incident_edge_ordinal":1,"incident_side":"preceding"},{"outer":3,"middle":2,"vertex_ordinal":2,"incident_edge_ordinal":1,"incident_side":"preceding"}],"common_incident_edge_ordinal":1}"#;
    let entry: super::DesignTopologyRecipeEntry = serde_json::from_str(wire).unwrap();
    assert_eq!(serde_json::to_string(&entry).unwrap(), wire);
    let mut invalid = serde_json::to_value(entry).unwrap();
    invalid["common_incident_edge_ordinal"] = serde_json::json!(2);
    assert!(
        serde_json::from_value::<super::DesignTopologyRecipeEntry>(invalid)
            .unwrap_err()
            .to_string()
            .contains("common_incident_edge_ordinal")
    );
}

#[test]
fn surface_patch_recipe_requires_two_clauses_and_preserves_root_wire() {
    let clause = r#"{"fields":[[0],[0],[2,0],[0,0],[0],[0,0]],"face_reference_ordinals":[0,0],"edge_reference_ordinals":[0,0],"payload_entry_count":0,"entries":[]}"#;
    let wire = format!(r#"{{"root":2,"clauses":[{clause},{clause}]}}"#);
    let structure: super::DesignSurfacePatchRecipeStructure = serde_json::from_str(&wire).unwrap();
    assert_eq!(serde_json::to_string(&structure).unwrap(), wire);
    let invalid_root = wire.replace("\"root\":2", "\"root\":1");
    assert!(
        serde_json::from_str::<super::DesignSurfacePatchRecipeStructure>(&invalid_root)
            .unwrap_err()
            .to_string()
            .contains("root")
    );
    for clauses in [
        String::new(),
        clause.to_owned(),
        format!("{clause},{clause},{clause}"),
    ] {
        let invalid = format!(r#"{{"root":2,"clauses":[{clauses}]}}"#);
        assert!(
            serde_json::from_str::<super::DesignSurfacePatchRecipeStructure>(&invalid)
                .unwrap_err()
                .to_string()
                .contains("clauses")
        );
    }
}

#[test]
fn recipe_sidecar_rejects_disagreeing_counts() {
    let side = serde_json::json!({"field_count": 3, "header_value": 0,
        "scalars": [0], "payload_prefix": [0], "payload_entry_count": 0, "entries": []});
    assert!(serde_json::from_value::<super::DesignTopologyRecipeSide>(side).is_err());
    let side = serde_json::json!({"field_count": 2, "header_value": 0,
        "scalars": [0], "payload_prefix": [0], "payload_entry_count": 1, "entries": []});
    assert!(serde_json::from_value::<super::DesignTopologyRecipeSide>(side).is_err());
    let clause = serde_json::json!({"fields": [], "face_reference_ordinals": [0, 0],
        "edge_reference_ordinals": [0, 0], "payload_entry_count": 1, "entries": []});
    assert!(serde_json::from_value::<super::DesignSurfacePatchRecipeClause>(clause).is_err());
}

#[test]
fn recipe_sidecar_derives_counts_without_changing_wire() {
    let side = serde_json::json!({"field_count": 2, "header_value": 0,
        "scalars": [0], "payload_prefix": [0], "payload_entry_count": 0, "entries": []});
    let record: super::DesignTopologyRecipeSide = serde_json::from_value(side.clone()).unwrap();
    assert_eq!(record.field_count(), 2);
    assert_eq!(serde_json::to_value(record).unwrap(), side);
}
