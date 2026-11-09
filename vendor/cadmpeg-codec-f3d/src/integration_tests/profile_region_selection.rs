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
use crate::loss::F3dLossCode;
use crate::records::feature::scope::{
    DesignExtrudeScope, DesignParameterScope, DesignScopePayload,
};
use crate::records::sketch_geometry::SketchCurveIdentity;
use crate::records::topology::sketch_profile::DesignSketchProfileOperand;
use cadmpeg_ir::report::loss::LossNote;

const REGION_TOLERANCE: f64 = 1.0e-7;
const ANGULAR_TOLERANCE: f64 = 1.0e-9;

fn square_and_circles() -> Vec<serde_json::Value> {
    vec![
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 2.0}),
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 1.3}),
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
    ]
}

fn fixture(
    members: &[u32],
) -> (
    Sketch,
    Vec<SketchEntity>,
    Vec<SketchCurveIdentity>,
    DesignParameterScope,
) {
    fixture_with(square_and_circles(), &[&[0], &[1], &[2, 3, 4, 5]], members)
}

fn fixture_with(
    geometry: Vec<serde_json::Value>,
    profile_entities: &[&[usize]],
    members: &[u32],
) -> (
    Sketch,
    Vec<SketchEntity>,
    Vec<SketchCurveIdentity>,
    DesignParameterScope,
) {
    let members = members
        .iter()
        .map(|primary| (*primary, false))
        .collect::<Vec<_>>();
    fixture_with_flags(geometry, profile_entities, &members)
}

/// The fixture whose counted members carry a primary curve identity and the
/// first incidence value.
fn fixture_with_flags(
    geometry: Vec<serde_json::Value>,
    profile_entities: &[&[usize]],
    members: &[(u32, bool)],
) -> (
    Sketch,
    Vec<SketchEntity>,
    Vec<SketchCurveIdentity>,
    DesignParameterScope,
) {
    let sketch_id = cadmpeg_ir::sketches::SketchId::mint("synthetic:test:sketch#42").unwrap();
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
            geometry_group: None,
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
    let profiles = profile_entities
        .iter()
        .map(|indices| {
            indices
                .iter()
                .map(|index| json!({"entity": entities[*index].id(), "reversed": false}))
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
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
        .map(|(primary, reversed)| {
            json!({
                "kind": 3, "kind_offset": 0, "curve_primary_id": primary,
                "curve_primary_id_offset": 4,
                "incidence_words": [0, 0, 0, u32::from(*reversed), 3, 8, 0, 0],
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
    bound_fixture(fixture(members))
}

fn bound_fixture(
    fixture: (
        Sketch,
        Vec<SketchEntity>,
        Vec<SketchCurveIdentity>,
        DesignParameterScope,
    ),
) -> ProfileRef {
    bound_fixture_with_losses(fixture).0
}

/// `bound_fixture` with the losses of its profile geometry work.
fn bound_fixture_with_losses(
    (sketch, entities, curves, scope): (
        Sketch,
        Vec<SketchEntity>,
        Vec<SketchCurveIdentity>,
        DesignParameterScope,
    ),
) -> (ProfileRef, Vec<LossNote>) {
    let work = ProfileGeometryWork::new();
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
    (profile, work.losses())
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

#[test]
fn counted_members_name_each_fragment_of_a_curve_split_on_the_region_boundary() {
    let geometry = vec![
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": -2.0}, "end": {"u": 0.0, "v": -1.0}}),
    ];
    let profile = bound_fixture(fixture_with(
        geometry.clone(),
        &[&[0, 1, 2, 3]],
        &[1, 1, 2, 3, 4],
    ));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("square region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 5);
    let (_, entities, _, _) = fixture_with(geometry.clone(), &[&[0, 1, 2, 3]], &[]);
    assert_eq!(
        outer_boundary
            .iter()
            .filter(|use_| use_.entity == *entities[0].id())
            .count(),
        2
    );
    assert!(matches!(
        bound_fixture(fixture_with(geometry, &[&[0, 1, 2, 3]], &[1, 2, 3, 4])),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
}

#[test]
fn counted_members_name_both_curves_of_a_coincident_boundary_stretch() {
    let geometry = vec![
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 1.0}, "end": {"u": 2.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": 1.0}, "end": {"u": 2.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": 2.0}, "end": {"u": 0.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 2.0}, "end": {"u": 0.0, "v": 1.0}}),
    ];
    let loops: &[&[usize]] = &[&[0, 1, 2, 3], &[4, 5, 6, 7]];
    let profile = bound_fixture(fixture_with(geometry.clone(), loops, &[1, 2, 3, 3, 5, 4]));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("square region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 5);
    assert!(matches!(
        bound_fixture(fixture_with(geometry, loops, &[1, 2, 3, 5, 4])),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
}

#[test]
fn a_selection_through_a_coincident_edge_reports_the_direction_assumption() {
    let geometry = vec![
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 1.0}, "end": {"u": 2.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": 1.0}, "end": {"u": 2.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": 2.0}, "end": {"u": 0.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 2.0}, "end": {"u": 0.0, "v": 1.0}}),
    ];
    let loops: &[&[usize]] = &[&[0, 1, 2, 3], &[4, 5, 6, 7]];
    let (profile, losses) =
        bound_fixture_with_losses(fixture_with(geometry, loops, &[1, 2, 3, 3, 5, 4]));
    assert!(matches!(
        profile,
        ProfileRef::Planar(PlanarProfileRef::SketchRegions { .. })
    ));
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileCoincidentDirectionAssumed.kind()
    );
    assert_eq!(
        loss.message,
        "1 extrude profile selection(s) depended on the member flags of an edge of coincident curves: a selected ring has such an edge, or a ring with such an edge has the curves of a member list. The reading accepts such an edge when one distinct member that names one of its curves has that curve's traversal direction, and it checks no flag of the edge's other members; which curve's direction sets the flag is open (DR-124)."
    );

    let (profile, losses) = bound_fixture_with_losses(fixture(&[2, 2, 4, 2, 6, 3, 5, 2]));
    assert!(matches!(
        profile,
        ProfileRef::Planar(PlanarProfileRef::SketchRegions { .. })
    ));
    assert!(losses.is_empty(), "{losses:?}");
}

#[test]
fn a_selection_beside_a_full_ellipse_reports_the_island_absence_assumption() {
    // A thin ellipse across the diagonal beyond the corner (1, 1): its
    // exact box spans u and v from 0.915, across the corner, so the
    // square's sides cross the box.
    let ellipse =
        SketchGeometry::try_from(cadmpeg_ir::sketches::SketchGeometryDefinition::Ellipse {
            center: Point2::new(1.2, 1.2),
            major_angle: cadmpeg_ir::scalar::Angle::new(-std::f64::consts::FRAC_PI_4).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: cadmpeg_ir::scalar::Length::new(0.4).unwrap(),
                minor_radius: cadmpeg_ir::scalar::Length::new(0.05).unwrap(),
            },
            bounds: None,
        })
        .unwrap();
    let geometry = vec![
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": -2.0}, "end": {"u": 0.0, "v": -1.0}}),
        serde_json::to_value(ellipse).unwrap(),
    ];
    let (profile, losses) =
        bound_fixture_with_losses(fixture_with(geometry, &[&[0, 1, 2, 3]], &[1, 1, 2, 3, 4]));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("square region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        hole_boundaries, ..
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        F3dLossCode::FeatureProfileIslandAbsenceAssumed.kind()
    );
}

#[test]
fn counted_members_match_a_region_whose_edge_two_profiles_share() {
    let geometry = vec![
        json!({"kind": "line", "start": {"u": -1.0, "v": -1.0}, "end": {"u": 0.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": -1.0}, "end": {"u": 0.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 1.0}, "end": {"u": -1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": -1.0, "v": 1.0}, "end": {"u": -1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": -1.0}, "end": {"u": 1.0, "v": -1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": -1.0}, "end": {"u": 1.0, "v": 1.0}}),
        json!({"kind": "line", "start": {"u": 1.0, "v": 1.0}, "end": {"u": 0.0, "v": 1.0}}),
    ];
    let profile = bound_fixture(fixture_with(
        geometry,
        &[&[0, 1, 2, 3], &[1, 4, 5, 6]],
        &[1, 2, 3, 4],
    ));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("left square region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed { outer_boundary, .. }] = regions.as_slice() else {
        panic!("one atomic region");
    };
    assert_eq!(outer_boundary.len(), 4);
}

#[test]
fn counted_members_name_an_uncut_circle_once() {
    let geometry = vec![
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 1.0}),
        json!({"kind": "line", "start": {"u": -2.0, "v": -2.0}, "end": {"u": 2.0, "v": -2.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": -2.0}, "end": {"u": 2.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": 2.0}, "end": {"u": -2.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": -2.0, "v": 2.0}, "end": {"u": -2.0, "v": -2.0}}),
    ];
    let loops: &[&[usize]] = &[&[0], &[1, 2, 3, 4]];
    let profile = bound_fixture(fixture_with(geometry.clone(), loops, &[1]));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("disk region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        hole_boundaries, ..
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    assert!(matches!(
        bound_fixture(fixture_with(geometry, loops, &[1, 1])),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
}

#[test]
fn counted_members_name_each_coincident_uncut_circle_once() {
    let geometry = vec![
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 1.0}),
        json!({"kind": "circle", "center": {"u": 0.0, "v": 0.0}, "radius": 1.0}),
        json!({"kind": "line", "start": {"u": -2.0, "v": -2.0}, "end": {"u": 2.0, "v": -2.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": -2.0}, "end": {"u": 2.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": 2.0, "v": 2.0}, "end": {"u": -2.0, "v": 2.0}}),
        json!({"kind": "line", "start": {"u": -2.0, "v": 2.0}, "end": {"u": -2.0, "v": -2.0}}),
    ];
    let loops: &[&[usize]] = &[&[0], &[1], &[2, 3, 4, 5]];
    let profile = bound_fixture(fixture_with(geometry.clone(), loops, &[1, 2]));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("disk region: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        hole_boundaries, ..
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    assert!(matches!(
        bound_fixture(fixture_with(geometry, loops, &[1, 2, 2])),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
}

#[test]
fn a_counted_member_flag_states_a_traversal_against_the_curve_direction() {
    // A 4 x 3 rectangle with one profile loop, and a diagonal from its
    // lower-left corner that ends on the top side at (2, 3).
    let geometry = vec![
        json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 4.0, "v": 0.0}}),
        json!({"kind": "line", "start": {"u": 4.0, "v": 0.0}, "end": {"u": 4.0, "v": 3.0}}),
        json!({"kind": "line", "start": {"u": 4.0, "v": 3.0}, "end": {"u": 0.0, "v": 3.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 3.0}, "end": {"u": 0.0, "v": 0.0}}),
        json!({"kind": "line", "start": {"u": 0.0, "v": 0.0}, "end": {"u": 2.0, "v": 3.0}}),
    ];
    let loops: &[&[usize]] = &[&[0, 1, 2, 3]];
    let members = |diagonal_reversed| [(1, false), (2, false), (3, false), (5, diagonal_reversed)];
    let profile = bound_fixture(fixture_with_flags(geometry.clone(), loops, &members(true)));
    let ProfileRef::Planar(PlanarProfileRef::SketchRegions { regions, .. }) = &profile else {
        panic!("the region right of the diagonal: {profile:?}");
    };
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one atomic region");
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 4);
    assert!(matches!(
        bound_fixture(fixture_with_flags(geometry, loops, &members(false))),
        ProfileRef::Planar(PlanarProfileRef::SketchSelection { .. })
    ));
}
