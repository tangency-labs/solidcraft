// SPDX-License-Identifier: Apache-2.0

use std::f64::consts::{FRAC_PI_2, PI, TAU};

use cadmpeg_ir::math::Point2;
use cadmpeg_ir::scalar::{Angle, Length};
use cadmpeg_ir::sketches::{
    SketchEntity, SketchEntityId, SketchGeometry, SketchGeometryDefinition, SketchId,
};

use super::super::closed_sketch_profiles;
use crate::loss::F3dLossCode;

fn line_entity(sketch: &SketchId, id: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(id).expect("fixture edge ID"),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .expect("valid line geometry"),
    )
}

fn arc_entity(
    sketch: &SketchId,
    id: &str,
    center: (f64, f64),
    radius: f64,
    start_angle: f64,
    end_angle: f64,
) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(id).expect("fixture edge ID"),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(center.0, center.1),
            radius: Length::new(radius).expect("valid radius"),
            start_angle: Angle::new(start_angle).expect("valid start angle"),
            end_angle: Angle::new(end_angle).expect("valid end angle"),
        })
        .expect("valid arc geometry"),
    )
}

#[test]
fn branched_arc_loop() {
    let sketch = SketchId::mint("f3d:model:sketch#branched-arc-loop").expect("valid sketch ID");

    // Two quarter arcs meeting at (0.0, 10.0) with a shared divider line to (0.0, 0.0)
    // and two bottom lines to complete two adjacent closed cells.
    let entities = vec![
        arc_entity(
            &sketch,
            "synthetic:test:id#arc-right",
            (0.0, 0.0),
            10.0,
            0.0,
            FRAC_PI_2,
        ),
        arc_entity(
            &sketch,
            "synthetic:test:id#arc-left",
            (0.0, 0.0),
            10.0,
            FRAC_PI_2,
            PI,
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#divider",
            (0.0, 10.0),
            (0.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#bottom-right",
            (0.0, 0.0),
            (10.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#bottom-left",
            (-10.0, 0.0),
            (0.0, 0.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert!(profiles.iter().all(|profile| profile.len() == 3));
    assert!(profiles.iter().all(|profile| profile
        .iter()
        .any(|entity_use| entity_use.entity.as_str() == "synthetic:test:id#divider")));
    assert!(profiles.losses.is_empty());
}

#[test]
fn duplicate_shared_wall_yielding_two_profiles() {
    let sketch = SketchId::mint("f3d:model:sketch#duplicate-wall").expect("valid sketch ID");

    // Two adjacent squares sharing an interior wall at u = 1.0.
    // The shared wall is included twice as two distinct entity IDs with identical geometry.
    let entities = vec![
        line_entity(
            &sketch,
            "synthetic:test:id#left-bottom",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#shared-wall-1",
            (1.0, 0.0),
            (1.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#left-top",
            (1.0, 1.0),
            (0.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#left-outer",
            (0.0, 1.0),
            (0.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#shared-wall-2",
            (1.0, 0.0),
            (1.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#right-bottom",
            (1.0, 0.0),
            (2.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#right-outer",
            (2.0, 0.0),
            (2.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#right-top",
            (2.0, 1.0),
            (1.0, 1.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 4);
    assert_eq!(profiles[1].len(), 4);

    let has_wall_1 = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#shared-wall-1")
    };
    let has_wall_2 = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#shared-wall-2")
    };

    assert!(
        (has_wall_1(&profiles[0]) && has_wall_2(&profiles[1]))
            || (has_wall_2(&profiles[0]) && has_wall_1(&profiles[1]))
    );
    assert!(profiles.losses.is_empty());
}

#[test]
fn dangling_spur_pruned_to_one_loop() {
    let sketch = SketchId::mint("f3d:model:sketch#dangling-spur").expect("valid sketch ID");

    // A closed triangle plus a dangling spur extending from vertex (1.0, 0.0) to (2.0, 0.0).
    let entities = vec![
        line_entity(
            &sketch,
            "synthetic:test:id#tri-bottom",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#tri-hypot",
            (1.0, 0.0),
            (0.0, 1.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#tri-left",
            (0.0, 1.0),
            (0.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#dangling-spur",
            (1.0, 0.0),
            (2.0, 0.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 1);
    assert_eq!(profiles[0].len(), 3);
    assert!(!profiles[0]
        .iter()
        .any(|u| u.entity.as_str() == "synthetic:test:id#dangling-spur"));

    assert_eq!(profiles.losses.len(), 1);
    assert_eq!(
        profiles.losses[0].code,
        F3dLossCode::SketchCurveUnprofiled.kind()
    );
    assert!(profiles.losses[0]
        .message
        .contains("synthetic:test:id#dangling-spur"));
}

#[test]
fn bridge_input_producing_only_closed_loops() {
    let sketch = SketchId::mint("f3d:model:sketch#bridge-input").expect("valid sketch ID");

    // Two independent closed triangles joined by an open bridge segment between them.
    let entities = vec![
        // Triangle 1
        line_entity(
            &sketch,
            "synthetic:test:id#t1-bottom",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#t1-hypot",
            (1.0, 0.0),
            (0.0, 1.0),
        ),
        line_entity(&sketch, "synthetic:test:id#t1-left", (0.0, 1.0), (0.0, 0.0)),
        // Bridge
        line_entity(
            &sketch,
            "synthetic:test:id#bridge-edge",
            (1.0, 0.0),
            (3.0, 0.0),
        ),
        // Triangle 2
        line_entity(
            &sketch,
            "synthetic:test:id#t2-bottom",
            (3.0, 0.0),
            (4.0, 0.0),
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#t2-hypot",
            (4.0, 0.0),
            (3.0, 1.0),
        ),
        line_entity(&sketch, "synthetic:test:id#t2-left", (3.0, 1.0), (3.0, 0.0)),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 3);
    assert_eq!(profiles[1].len(), 3);
    assert!(!profiles.iter().any(|p| p
        .iter()
        .any(|u| u.entity.as_str() == "synthetic:test:id#bridge-edge")));

    assert_eq!(profiles.losses.len(), 1);
    assert_eq!(
        profiles.losses[0].code,
        F3dLossCode::SketchCurveUnprofiled.kind()
    );
    assert!(profiles.losses[0]
        .message
        .contains("synthetic:test:id#bridge-edge"));
}

#[test]
fn complementary_half_arcs_not_deduplicated() {
    let sketch = SketchId::mint("f3d:model:sketch#complementary-arcs").expect("valid sketch ID");

    // Two 180-degree half-arcs of radius 2.0 around (0.0, 0.0) sharing endpoints at (-2.0, 0.0)
    // and (2.0, 0.0), divided by a diameter line.
    let entities = vec![
        arc_entity(
            &sketch,
            "synthetic:test:id#upper-arc",
            (0.0, 0.0),
            2.0,
            0.0,
            PI,
        ),
        arc_entity(
            &sketch,
            "synthetic:test:id#lower-arc",
            (0.0, 0.0),
            2.0,
            PI,
            TAU,
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#diameter",
            (-2.0, 0.0),
            (2.0, 0.0),
        ),
    ];

    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, &sketch, &entities, 1.0e-6)
    })
    .expect("closed sketch profiles");

    assert_eq!(profiles.len(), 2);
    assert_eq!(profiles[0].len(), 2);
    assert_eq!(profiles[1].len(), 2);

    let has_upper = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#upper-arc")
    };
    let has_lower = |profile: &[cadmpeg_ir::sketches::SketchEntityUse]| {
        profile
            .iter()
            .any(|u| u.entity.as_str() == "synthetic:test:id#lower-arc")
    };

    assert!(
        (has_upper(&profiles[0]) && has_lower(&profiles[1]))
            || (has_lower(&profiles[0]) && has_upper(&profiles[1]))
    );
    assert!(profiles.losses.is_empty());
}

fn profile_entity_sets(
    sketch: &SketchId,
    entities: &[SketchEntity],
) -> Vec<std::collections::BTreeSet<String>> {
    let profiles = crate::test_support::with_decode_context(|decode_ctx| {
        closed_sketch_profiles(decode_ctx, sketch, entities, 1.0e-6)
    })
    .expect("closed sketch profiles");
    let mut sets: Vec<_> = profiles
        .iter()
        .map(|profile| {
            profile
                .iter()
                .map(|entity_use| {
                    entity_use
                        .entity
                        .as_str()
                        .trim_start_matches("synthetic:test:id#")
                        .to_owned()
                })
                .collect::<std::collections::BTreeSet<_>>()
        })
        .collect();
    sets.sort();
    sets
}

fn names(items: &[&str]) -> std::collections::BTreeSet<String> {
    items.iter().map(|item| (*item).to_owned()).collect()
}

fn rotate(rotation: f64, (u, v): (f64, f64)) -> (f64, f64) {
    let (sin, cos) = rotation.sin_cos();
    (cos * u - sin * v, sin * u + cos * v)
}

/// A rectangular body under a chord, with a notch above the chord. The notch
/// arc leaves the chord's left end along the chord direction. `arc_id` and
/// `chord_id` set which curve sorts first by identity.
fn tangent_notch(
    sketch: &SketchId,
    arc_id: &str,
    chord_id: &str,
    rotation: f64,
    mirrored: bool,
) -> Vec<SketchEntity> {
    let side = if mirrored { -1.0 } else { 1.0 };
    let at = |u: f64, v: f64| rotate(rotation, (u, side * v));
    let top = (FRAC_PI_2 * 2.0 / 3.0).sin();
    let rise = 1.0 - (FRAC_PI_2 * 2.0 / 3.0).cos();
    let (first, second) = if mirrored {
        (rotation + FRAC_PI_2 - PI / 3.0, rotation + FRAC_PI_2)
    } else {
        (rotation - FRAC_PI_2, rotation - FRAC_PI_2 + PI / 3.0)
    };
    let id = |name: &str| format!("synthetic:test:id#{name}");
    vec![
        arc_entity(sketch, &id(arc_id), at(0.0, 1.0), 1.0, first, second),
        line_entity(sketch, &id(chord_id), at(0.0, 0.0), at(3.0, 0.0)),
        line_entity(sketch, &id("notch-close"), at(top, rise), at(3.0, 0.0)),
        line_entity(sketch, &id("body-left-top"), at(-2.0, 0.0), at(0.0, 0.0)),
        line_entity(sketch, &id("body-right-top"), at(3.0, 0.0), at(5.0, 0.0)),
        line_entity(sketch, &id("body-right"), at(5.0, 0.0), at(5.0, -3.0)),
        line_entity(sketch, &id("body-bottom"), at(5.0, -3.0), at(-2.0, -3.0)),
        line_entity(sketch, &id("body-left"), at(-2.0, -3.0), at(-2.0, 0.0)),
    ]
}

#[test]
fn tangent_arc_and_chord_separate_by_curvature_for_each_identity_order_rotation_and_side() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-notch").expect("valid sketch ID");
    for (arc_id, chord_id) in [("a-arc", "b-chord"), ("b-arc", "a-chord")] {
        for rotation in [0.0, PI / 6.0, FRAC_PI_2, PI, 3.0 * FRAC_PI_2] {
            for mirrored in [false, true] {
                let entities = tangent_notch(&sketch, arc_id, chord_id, rotation, mirrored);
                assert_eq!(
                    profile_entity_sets(&sketch, &entities),
                    {
                        let mut expected = vec![
                            names(&[arc_id, chord_id, "notch-close"]),
                            names(&[
                                chord_id,
                                "body-left-top",
                                "body-right-top",
                                "body-right",
                                "body-bottom",
                                "body-left",
                            ]),
                        ];
                        expected.sort();
                        expected
                    },
                    "{arc_id} {chord_id} rotation {rotation} mirrored {mirrored}"
                );
            }
        }
    }
}

/// Two arcs leave the left node with one tangent direction, and the wall
/// between them bounds the lens. `small` and `large` set the identity order.
fn tangent_arc_pair(
    sketch: &SketchId,
    small: &str,
    large: &str,
    large_radius: f64,
    rotation: f64,
) -> Vec<SketchEntity> {
    let at = |u: f64, v: f64| rotate(rotation, (u, v));
    let id = |name: &str| format!("synthetic:test:id#{name}");
    let sweep = PI / 3.0;
    let start = rotation - FRAC_PI_2;
    let small_end = (sweep.sin(), 1.0 - sweep.cos());
    let large_end = (
        large_radius * sweep.sin(),
        large_radius * (1.0 - sweep.cos()),
    );
    let corner = large_end.0 + 2.0;
    vec![
        arc_entity(sketch, &id(small), at(0.0, 1.0), 1.0, start, start + sweep),
        arc_entity(
            sketch,
            &id(large),
            at(0.0, large_radius),
            large_radius,
            start,
            start + sweep,
        ),
        line_entity(
            sketch,
            &id("wall"),
            at(small_end.0, small_end.1),
            at(large_end.0, large_end.1),
        ),
        line_entity(
            sketch,
            &id("right-top"),
            at(large_end.0, large_end.1),
            at(corner, 0.0),
        ),
        line_entity(sketch, &id("left-top"), at(-2.0, 0.0), at(0.0, 0.0)),
        line_entity(sketch, &id("right"), at(corner, 0.0), at(corner, -3.0)),
        line_entity(sketch, &id("bottom"), at(corner, -3.0), at(-2.0, -3.0)),
        line_entity(sketch, &id("left"), at(-2.0, -3.0), at(-2.0, 0.0)),
    ]
}

#[test]
fn tangent_arcs_on_one_side_order_the_smaller_radius_counterclockwise() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-arcs").expect("valid sketch ID");
    for (small, large) in [("a-small", "b-large"), ("b-small", "a-large")] {
        for rotation in [0.0, PI] {
            let entities = tangent_arc_pair(&sketch, small, large, 2.5, rotation);
            let mut expected = vec![
                names(&[small, large, "wall"]),
                names(&[large, "right-top", "right", "bottom", "left", "left-top"]),
            ];
            expected.sort();
            assert_eq!(
                profile_entity_sets(&sketch, &entities),
                expected,
                "{small} {large} rotation {rotation}"
            );
        }
    }
}

#[test]
fn tangent_arcs_on_opposite_sides_order_by_the_sign_of_curvature() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-opposite").expect("valid sketch ID");
    let id = |name: &str| format!("synthetic:test:id#{name}");
    let sweep = PI / 3.0;
    let up_end = (sweep.sin(), 1.0 - sweep.cos());
    let down_end = (2.0 * sweep.sin(), -2.0 * (1.0 - sweep.cos()));
    for (up, down) in [("a-up", "b-down"), ("b-up", "a-down")] {
        let entities = vec![
            arc_entity(
                &sketch,
                &id(up),
                (0.0, 1.0),
                1.0,
                -FRAC_PI_2,
                -FRAC_PI_2 + sweep,
            ),
            arc_entity(
                &sketch,
                &id(down),
                (0.0, -2.0),
                2.0,
                FRAC_PI_2 - sweep,
                FRAC_PI_2,
            ),
            line_entity(&sketch, &id("wall"), up_end, down_end),
            line_entity(&sketch, &id("right"), down_end, (down_end.0, -3.0)),
            line_entity(&sketch, &id("bottom"), (down_end.0, -3.0), (-2.0, -3.0)),
            line_entity(&sketch, &id("left"), (-2.0, -3.0), (-2.0, 0.0)),
            line_entity(&sketch, &id("left-top"), (-2.0, 0.0), (0.0, 0.0)),
        ];
        let mut expected = vec![
            names(&[up, down, "wall"]),
            names(&[down, "right", "bottom", "left", "left-top"]),
        ];
        expected.sort();
        assert_eq!(
            profile_entity_sets(&sketch, &entities),
            expected,
            "{up} {down}"
        );
    }
}

/// One component holds a tie the identity order sorts correctly at one chord
/// end and wrongly at the other, beside rays that have no tie.
#[test]
fn every_tangent_node_in_a_component_orders_by_curvature_when_identity_order_differs() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-mixed").expect("valid sketch ID");
    let id = |name: &str| format!("synthetic:test:id#{name}");
    let sweep = PI / 3.0;
    let (run, rise) = (sweep.sin(), 1.0 - sweep.cos());
    let entities = vec![
        arc_entity(
            &sketch,
            &id("a-left-arc"),
            (0.0, 1.0),
            1.0,
            -FRAC_PI_2,
            -FRAC_PI_2 + sweep,
        ),
        line_entity(&sketch, &id("b-chord"), (0.0, 0.0), (6.0, 0.0)),
        arc_entity(
            &sketch,
            &id("c-right-arc"),
            (6.0, 1.0),
            1.0,
            -FRAC_PI_2 - sweep,
            -FRAC_PI_2,
        ),
        line_entity(&sketch, &id("top"), (run, rise), (6.0 - run, rise)),
        line_entity(&sketch, &id("left-top"), (-2.0, 0.0), (0.0, 0.0)),
        line_entity(&sketch, &id("right-top"), (6.0, 0.0), (8.0, 0.0)),
        line_entity(&sketch, &id("right"), (8.0, 0.0), (8.0, -3.0)),
        line_entity(&sketch, &id("bottom"), (8.0, -3.0), (-2.0, -3.0)),
        line_entity(&sketch, &id("left"), (-2.0, -3.0), (-2.0, 0.0)),
    ];
    let mut expected = vec![
        names(&["a-left-arc", "b-chord", "c-right-arc", "top"]),
        names(&[
            "b-chord",
            "left-top",
            "right-top",
            "right",
            "bottom",
            "left",
        ]),
    ];
    expected.sort();
    assert_eq!(profile_entity_sets(&sketch, &entities), expected);
}

fn node_order(entities: &[SketchEntity]) -> Vec<usize> {
    let edges: Vec<_> = entities
        .iter()
        .map(|entity| {
            let ends = match entity.geometry.definition() {
                SketchGeometryDefinition::Line { start, end } => [start.get(), end.get()],
                SketchGeometryDefinition::Arc { center, radius, .. } => [
                    center.get(),
                    Point2::new(center.get().u + radius.get(), 0.0),
                ],
                _ => unreachable!("fixture holds lines and arcs"),
            };
            (entity, ends)
        })
        .collect();
    let mut half_edges: Vec<usize> = (0..entities.len()).map(|edge| edge * 2).collect();
    super::super::order_outgoing_half_edges(&mut half_edges, &edges);
    let first = half_edges.iter().position(|edge| *edge == 0).unwrap_or(0);
    half_edges.rotate_left(first);
    half_edges
}

/// The arc curves left of the chord, so curvature orders it after the chord.
/// Its tilt below the chord orders it before the chord once the tilt is
/// outside the tolerance.
#[test]
fn tangent_tolerance_selects_curvature_inside_and_angle_outside() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-boundary").expect("valid sketch ID");
    let order_for = |tilt: f64| {
        let start = -FRAC_PI_2 + tilt;
        node_order(&[
            line_entity(&sketch, "synthetic:test:id#chord", (0.0, 0.0), (3.0, 0.0)),
            arc_entity(
                &sketch,
                "synthetic:test:id#arc",
                (-tilt.sin(), tilt.cos()),
                1.0,
                start,
                start + 1.0,
            ),
            line_entity(&sketch, "synthetic:test:id#back", (0.0, 0.0), (-1.0, 0.0)),
        ])
    };
    assert_eq!(order_for(-0.5e-9), vec![0, 2, 4]);
    assert_eq!(order_for(-2.0e-9), vec![0, 4, 2]);
    assert_eq!(order_for(2.0e-9), vec![0, 2, 4]);
}

/// Directions next to the negative `u` axis lie at opposite ends of the
/// `atan2` range but form one tie.
#[test]
fn tangent_ties_group_across_the_angle_branch_cut() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-cut").expect("valid sketch ID");
    let order_for = |line_end_v: f64, tilt: f64| {
        node_order(&[
            line_entity(
                &sketch,
                "synthetic:test:id#b-chord",
                (0.0, 0.0),
                (-3.0, line_end_v),
            ),
            arc_entity(
                &sketch,
                "synthetic:test:id#a-arc",
                (tilt.sin(), -tilt.cos()),
                1.0,
                FRAC_PI_2 + tilt,
                FRAC_PI_2 + tilt + 1.0,
            ),
            line_entity(
                &sketch,
                "synthetic:test:id#c-forward",
                (0.0, 0.0),
                (1.0, 0.0),
            ),
        ])
    };
    for (line_end_v, tilt) in [
        (1.0e-12, 1.0e-12),
        (-1.0e-12, -1.0e-12),
        (1.0e-12, -1.0e-12),
        (-1.0e-12, 1.0e-12),
    ] {
        assert_eq!(
            order_for(line_end_v, tilt),
            vec![0, 2, 4],
            "line end {line_end_v} tilt {tilt}"
        );
    }
}

/// Identity order is wrong in both ties, so each group must sort on its own.
#[test]
fn each_tangent_group_at_one_node_orders_by_curvature() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-groups").expect("valid sketch ID");
    let order = node_order(&[
        arc_entity(
            &sketch,
            "synthetic:test:id#a-forward-arc",
            (0.0, 1.0),
            1.0,
            -FRAC_PI_2,
            -FRAC_PI_2 + 1.0,
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#b-forward-line",
            (0.0, 0.0),
            (1.0, 0.0),
        ),
        arc_entity(
            &sketch,
            "synthetic:test:id#c-back-arc",
            (0.0, -1.0),
            1.0,
            FRAC_PI_2,
            FRAC_PI_2 + 1.0,
        ),
        line_entity(
            &sketch,
            "synthetic:test:id#d-back-line",
            (0.0, 0.0),
            (-1.0, 0.0),
        ),
    ]);
    assert_eq!(order, vec![0, 6, 4, 2]);
}

/// Three arcs leave the node with one exact direction, so no ray lies outside
/// the tie and no angular gap marks a group boundary. Each arc lies left of
/// the arcs with a larger radius next to the node.
#[test]
fn tangent_group_holding_every_ray_at_a_node_orders_by_curvature() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-all").expect("valid sketch ID");
    let id = |name: &str| format!("synthetic:test:id#{name}");
    let sweep = PI / 3.0;
    let arc = |name: &str, radius: f64| {
        arc_entity(
            &sketch,
            &id(name),
            (0.0, radius),
            radius,
            -FRAC_PI_2,
            -FRAC_PI_2 + sweep,
        )
    };
    let order_for =
        |radii: [f64; 3]| node_order(&[arc("a", radii[0]), arc("b", radii[1]), arc("c", radii[2])]);
    assert_eq!(order_for([1.0, 2.0, 3.0]), vec![0, 4, 2]);
    assert_eq!(order_for([3.0, 2.0, 1.0]), vec![0, 2, 4]);

    let end = |radius: f64| (radius * sweep.sin(), radius * (1.0 - sweep.cos()));
    for (small, middle, large) in [("a", "b", "c"), ("c", "b", "a"), ("b", "c", "a")] {
        let entities = vec![
            arc(small, 1.0),
            arc(middle, 2.0),
            arc(large, 3.0),
            line_entity(&sketch, &id("inner-wall"), end(1.0), end(2.0)),
            line_entity(&sketch, &id("outer-wall"), end(2.0), end(3.0)),
        ];
        let mut expected = vec![
            names(&[small, middle, "inner-wall"]),
            names(&[middle, large, "outer-wall"]),
        ];
        expected.sort();
        assert_eq!(
            profile_entity_sets(&sketch, &entities),
            expected,
            "{small} {middle} {large}"
        );
    }
}

#[test]
fn collinear_rays_with_equal_curvature_keep_identity_order() {
    let sketch = SketchId::mint("f3d:model:sketch#collinear-rays").expect("valid sketch ID");
    let order = node_order(&[
        line_entity(&sketch, "synthetic:test:id#short", (0.0, 0.0), (1.0, 0.0)),
        line_entity(&sketch, "synthetic:test:id#long", (0.0, 0.0), (2.0, 0.0)),
        line_entity(&sketch, "synthetic:test:id#back", (0.0, 0.0), (-1.0, 0.0)),
    ]);
    assert_eq!(order, vec![0, 4, 2]);
}

/// Identity decides a tie when the stored directions differ inside the
/// tolerance. The directions here sort opposite to identity. The last case has
/// no ray outside the tie.
#[test]
fn equal_curvature_ties_order_by_identity_over_angle() {
    let sketch = SketchId::mint("f3d:model:sketch#tangent-identity").expect("valid sketch ID");
    let ray = |name: &str, end: (f64, f64)| {
        line_entity(
            &sketch,
            &format!("synthetic:test:id#{name}"),
            (0.0, 0.0),
            end,
        )
    };
    assert_eq!(
        node_order(&[
            ray("a", (1.0, 1.0e-12)),
            ray("b", (1.0, 0.0)),
            ray("back", (-1.0, 0.0)),
        ]),
        vec![0, 2, 4]
    );
    assert_eq!(
        node_order(&[
            ray("b", (1.0, 0.0)),
            ray("a", (1.0, 1.0e-12)),
            ray("back", (-1.0, 0.0)),
        ]),
        vec![0, 4, 2]
    );
    assert_eq!(
        node_order(&[
            ray("c", (1.0, 0.0)),
            ray("a", (1.0, 2.0e-12)),
            ray("b", (1.0, 1.0e-12)),
        ]),
        vec![0, 2, 4]
    );
}
