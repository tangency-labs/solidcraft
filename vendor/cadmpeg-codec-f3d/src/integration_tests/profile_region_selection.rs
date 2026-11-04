// SPDX-License-Identifier: Apache-2.0
//! Counted partial-curve selections bind trimmed neutral Extrude regions.

use std::collections::HashMap;

use cadmpeg_ir::features::{Feature, PlanarProfileRef, ProfileRef, SketchProfileRegion};
use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{Sketch, SketchEntity, SketchGeometry, SketchPlacement};
use serde_json::json;

use crate::design::geometry::ProfileGeometryWork;
use crate::design::profile_select::{
    bind_extrude_profile_selections, ExtrudeProfileResolution, SketchCurveSelectionResolution,
};
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignScopePayload,
};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;

const REGION_TOLERANCE: f64 = 1.0e-7;
const ANGULAR_TOLERANCE: f64 = 1.0e-9;

fn fixture(
    members: &[u32],
) -> (
    Sketch,
    Vec<SketchEntity>,
    Vec<SketchCurveIdentity>,
    DesignParameterScope,
) {
    let sketch_id = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#42").unwrap();
    let geometry = [
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0}),
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 1.3}),
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
    ];
    let mut entities = Vec::new();
    let mut curves = Vec::new();
    for (index, geometry) in geometry.into_iter().enumerate() {
        let primary = u32::try_from(index + 1).unwrap();
        entities.push(SketchEntity::new(
            crate::ids::neutral_sketch_curve_id(&sketch_id, u64::from(primary), 0),
            sketch_id.clone(),
            serde_json::from_value::<SketchGeometry>(geometry).unwrap(),
        ));
        curves.push(SketchCurveIdentity {
            id: format!("f3d:Design/BulkStream.dat:curve#{primary}"),
            record_index: primary,
            owner_reference: Some(42),
            class_tag: "300".to_owned().try_into().unwrap(),
            byte_offset: 0,
            geometry_offset: 0,
            entity_genesis: None,
            primary_id: std::num::NonZeroU64::new(u64::from(primary)).unwrap(),
            secondary_id: 0,
            geometry: None,
            planar_z_noise: None,
        });
    }
    let profiles = vec![
        vec![json!({"entity": entities[0].id(), "reversed": false})],
        vec![json!({"entity": entities[1].id(), "reversed": false})],
        entities[2..]
            .iter()
            .map(|entity| json!({"entity": entity.id(), "reversed": false}))
            .collect(),
    ];
    let sketch = Sketch {
        id: sketch_id,
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 10.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: serde_json::from_value(json!(profiles)).unwrap(),
        native_ref: None,
    };
    let members = members
        .iter()
        .map(|primary| {
            json!({
                "kind": 3, "kind_offset": 0, "curve_primary_id": primary,
                "curve_primary_id_offset": 4, "incidence_words": [0, 0, 0, 0, 3, 8, 0, 0],
                "incidence_words_offset": 8,
            })
        })
        .collect::<Vec<_>>();
    let operand: DesignSketchProfileOperand = serde_json::from_value(json!({
        "scope_reference_ordinal": 0, "record_index": 100, "byte_offset": 10,
        "class_tag": "301", "asset_id": "01234567-89ab-cdef-0123-456789abcdef",
        "asset_id_offset": 20, "entity_id": "0_42", "entity_suffix": 42,
        "entity_reference_offset": 100, "paired_class_tag": "259", "paired_byte_offset": 200,
        "region_selection": {
            "record_index": 103, "byte_offset": 300, "class_tag": "302", "region_count_offset": 336,
            "regions": [{"member_count_offset": 340, "members": members}],
            "companion_class_tag": "260", "companion_byte_offset": 700,
        }
    }))
    .unwrap();
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#10",
        DesignScopePayload::Extrude(Some(DesignExtrudeScope {
            extrude_profile: Some(operand),
            ..DesignExtrudeScope::default()
        })),
        10,
    );
    (sketch, entities, curves, scope)
}

fn bound_profile(members: &[u32]) -> ProfileRef {
    let (sketch, entities, curves, scope) = fixture(members);
    let mut feature: Feature = serde_json::from_value(json!({
        "id": "synthetic:test:feature#cut", "ordinal": 0, "suppressed": false,
        "native_ref": scope.id,
        "definition": {"definition": "extrude", "profile": {"kind": "sketch", "value": sketch.id},
            "direction": {"kind": "profile_normal"}, "start": {"kind": "profile_plane"},
            "extent": {"kind": "one_sided", "side": {"termination": {"kind": "blind", "length": 1.0}}},
            "op": "cut", "solid": true}
    })).unwrap();
    crate::test_support::with_decode_context(|ctx| {
        let sketches = [sketch];
        let scopes = [scope];
        let work = ProfileGeometryWork::new();
        bind_extrude_profile_selections(
            std::slice::from_mut(&mut feature),
            &scopes,
            &[],
            &[],
            &sketches,
            &SketchCurveSelectionResolution {
                scopes: &scopes,
                groups: &[],
                operands: &[],
                profile_curves: &[],
                placements: &[],
                curve_identities: &curves,
                sketches: &sketches,
                sketch_entities: &entities,
                spatial_sketches: &[],
                spatial_sketch_entities: &[],
            },
            ExtrudeProfileResolution {
                entities: &entities,
                spatial_sketches: &[],
                spatial_entities: &[],
                histories: &[],
                scope_histories: &HashMap::new(),
                linear_tolerance: REGION_TOLERANCE,
                angular_tolerance: ANGULAR_TOLERANCE,
                profile_work: &work,
                ctx,
            },
        )
    })
    .unwrap();
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::Extrude { profile, .. },
    ) = feature.evaluation.into_parts().0
    else {
        panic!("Extrude");
    };
    profile
}

#[test]
fn counted_partial_curves_select_the_clipped_polygon_region() {
    let members = [2, 2, 4, 2, 6, 3, 5, 2];
    let profile = bound_profile(&members);
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("trimmed region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 8);
    let (sketch, entities, _, _) = fixture(&members);
    assert_eq!(
        outer_boundary
            .iter()
            .filter(|use_| use_.entity == *entities[1].id())
            .count(),
        4
    );
    for use_ in outer_boundary {
        let entity = entities
            .iter()
            .find(|entity| entity.id() == &use_.entity)
            .unwrap();
        for parameter in use_.parameter_range.endpoints() {
            let point = match entity.geometry.definition() {
                cadmpeg_ir::sketches::SketchGeometryDefinition::Circle { .. } => {
                    Point2::new(1.3 * parameter.cos(), 1.3 * parameter.sin())
                }
                cadmpeg_ir::sketches::SketchGeometryDefinition::Line { start, end } => Point2::new(
                    start.u + parameter * (end.u - start.u),
                    start.v + parameter * (end.v - start.v),
                ),
                _ => panic!("analytic boundary"),
            };
            assert!((point.u.abs().max(point.v.abs()) - 1.0).abs() <= REGION_TOLERANCE);
            assert!((point.u.hypot(point.v) - 1.3).abs() <= REGION_TOLERANCE);
        }
    }
    assert_eq!(sketch.profiles.len(), 3);
    let mut reordered = members;
    reordered.reverse();
    assert_eq!(bound_profile(&reordered), profile);
}

#[test]
fn incomplete_partial_curve_members_keep_the_native_selection() {
    assert!(matches!(
        bound_profile(&[2, 2, 3, 4, 5, 6]),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
    assert!(matches!(
        bound_profile(&[2, 2, 2, 2, 3, 4, 5, 99]),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
}
