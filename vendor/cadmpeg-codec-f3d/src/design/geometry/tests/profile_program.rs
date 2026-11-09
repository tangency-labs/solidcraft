// SPDX-License-Identifier: Apache-2.0

use super::*;

const PROFILE_PROGRAM_TOLERANCE: f64 = 1.0e-7;

mod arrangement_islands;

fn divided_circle(two_dividers: bool) -> (Sketch, Vec<SketchEntity>) {
    let id = SketchId::mint("synthetic:test:id#curve-program").unwrap();
    let mut entities = vec![SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#circle").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(5.0).unwrap(),
        })
        .unwrap(),
    )];
    for (name, x) in if two_dividers {
        vec![("left", -2.0), ("right", 2.0)]
    } else {
        vec![("left", 0.0)]
    } {
        entities.push(SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(x, -6.0),
                end: Point2::new(x, 6.0),
            })
            .unwrap(),
        ));
    }
    (
        Sketch {
            id,
            name: None,
            configuration: None,
            visible: None,
            placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
                Point3::new(0.0, 0.0, 0.0),
                Vector3::new(0.0, 0.0, 1.0),
                Vector3::new(1.0, 0.0, 0.0),
            )
            .unwrap(),
            profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
            native_ref: None,
        },
        entities,
    )
}

#[test]
fn typed_profile_curve_program_resolves_one_trimmed_atomic_region() {
    let (sketch, entities) = divided_circle(true);
    let program = [
        entities[0].id().clone(),
        entities[1].id().clone(),
        entities[0].id().clone(),
        entities[2].id().clone(),
    ];
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .map(|found| found.region)
    .expect("one strip region");
    let SketchProfileRegion::Trimmed {
        outer_boundary: boundary,
        hole_boundaries: holes,
    } = region
    else {
        panic!("trimmed region");
    };
    assert!(holes.is_empty());
    assert_eq!(boundary.len(), 4);
    assert_eq!(
        boundary
            .iter()
            .filter(|use_| use_.entity == *entities[0].id())
            .count(),
        2
    );
    let mut reversed = program.to_vec();
    reversed.reverse();
    let reversed_region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &reversed,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .map(|found| found.region);
    assert_eq!(
        reversed_region,
        Some(SketchProfileRegion::Trimmed {
            outer_boundary: boundary,
            hole_boundaries: holes
        })
    );
}

#[test]
fn typed_profile_curve_program_refuses_two_half_disks_with_the_same_program() {
    let (sketch, entities) = divided_circle(false);
    let program = [entities[0].id().clone(), entities[1].id().clone()];
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .map(|found| found.region);
    assert_eq!(region, None);
}

fn line_entity(sketch: &SketchId, name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap(),
    )
}

/// Four counterclockwise lines named `{name}-0` to `{name}-3` around the
/// rectangle from `low` to `high`.
fn rectangle(
    sketch: &SketchId,
    name: &str,
    low: (f64, f64),
    high: (f64, f64),
) -> Vec<SketchEntity> {
    let corners = [low, (high.0, low.1), high, (low.0, high.1)];
    (0..4)
        .map(|index| {
            line_entity(
                sketch,
                &format!("{name}-{index}"),
                corners[index],
                corners[(index + 1) % 4],
            )
        })
        .collect()
}

fn sketch_with_loops(id: SketchId, loops: &[&[SketchEntity]]) -> Sketch {
    let profiles = loops
        .iter()
        .map(|entities| {
            entities
                .iter()
                .map(|entity| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap(),
        native_ref: None,
    }
}

/// The member ring that names every curve of `entities` in one direction.
fn ring(entities: &[SketchEntity], reversed: bool) -> Vec<(&SketchEntity, bool)> {
    entities.iter().map(|entity| (entity, reversed)).collect()
}

fn member_regions(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> Option<Vec<SketchProfileRegion>> {
    member_selection(sketch, entities, rings).map(|selected| selected.regions)
}

/// Whether the rings that `rings` select have an edge of coincident curves.
fn selects_coincident_edge(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> bool {
    member_selection(sketch, entities, rings)
        .expect("selected rings")
        .coincident
}

fn member_selection(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> Option<super::super::MemberRingRegions> {
    let rings = rings
        .iter()
        .map(|ring| {
            ring.iter()
                .map(|(entity, reversed)| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: *reversed,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_matching_member_rings(
            sketch,
            entities,
            &rings,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
}

/// The exterior and hole rings of the one region that `rings` select.
fn single_region(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> (
    Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>,
    Vec<Vec<cadmpeg_ir::features::SketchProfileBoundaryUse>>,
) {
    let regions = member_regions(sketch, entities, rings).expect("one region");
    let [SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("one trimmed region: {regions:?}");
    };
    (
        outer_boundary.iter().cloned().collect(),
        hole_boundaries
            .iter()
            .map(|ring| ring.iter().cloned().collect())
            .collect(),
    )
}

fn ring_entities(ring: &[cadmpeg_ir::features::SketchProfileBoundaryUse]) -> Vec<SketchEntityId> {
    let mut entities = ring
        .iter()
        .map(|use_| use_.entity.clone())
        .collect::<Vec<_>>();
    entities.sort_unstable();
    entities
}

fn sorted_ids(entities: &[&SketchEntity]) -> Vec<SketchEntityId> {
    let mut ids = entities
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<Vec<_>>();
    ids.sort_unstable();
    ids
}

#[test]
fn counted_member_directions_select_one_of_two_half_disks_with_the_same_curves() {
    let (sketch, entities) = divided_circle(false);
    let [circle, line] = [&entities[0], &entities[1]];
    let (left, _) = single_region(&sketch, &entities, &[vec![(line, false), (circle, false)]]);
    assert!(left
        .iter()
        .any(|use_| use_.entity == *line.id() && !use_.reversed));
    let (right, _) = single_region(&sketch, &entities, &[vec![(line, true), (circle, false)]]);
    assert!(right
        .iter()
        .any(|use_| use_.entity == *line.id() && use_.reversed));
    assert!(!selects_coincident_edge(
        &sketch,
        &entities,
        &[vec![(line, true), (circle, false)]]
    ));
    assert_eq!(
        member_regions(&sketch, &entities, &[vec![(line, false), (circle, true)]]),
        None
    );
}

#[test]
fn counted_members_select_a_t_junction_region_with_a_curve_outside_the_profile_loops() {
    // A 40 x 30 rectangle with one profile loop, and a diagonal from its
    // lower-left corner that ends on the top side at (20, 30).
    let id = SketchId::mint("synthetic:test:id#t-junction").unwrap();
    let mut entities = rectangle(&id, "side", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&entities]);
    entities.push(line_entity(&id, "diagonal", (0.0, 0.0), (20.0, 30.0)));
    let [bottom, right, top, left, diagonal] = [0, 1, 2, 3, 4].map(|index| &entities[index]);

    let (outer, holes) = single_region(
        &sketch,
        &entities,
        &[vec![
            (right, false),
            (bottom, false),
            (top, false),
            (diagonal, true),
        ]],
    );
    assert!(holes.is_empty());
    assert_eq!(
        ring_entities(&outer),
        sorted_ids(&[bottom, right, top, diagonal])
    );
    let top_use = outer.iter().find(|use_| use_.entity == *top.id()).unwrap();
    let [start, end] = top_use.parameter_range.endpoints();
    assert!(
        start.min(end).abs() < 1e-9 && (start.max(end) - 0.5).abs() < 1e-9,
        "{start} {end}"
    );

    let (triangle, holes) = single_region(
        &sketch,
        &entities,
        &[vec![(left, false), (top, false), (diagonal, false)]],
    );
    assert!(holes.is_empty());
    assert_eq!(triangle.len(), 3);

    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[vec![
                (right, false),
                (bottom, false),
                (top, false),
                (diagonal, false),
            ]],
        ),
        None
    );
}

#[test]
fn a_counted_hole_ring_names_the_island_of_the_selected_face() {
    // An 80 x 60 rectangle, a 20 x 20 island, and a bridge line from the
    // outer corner (0, 0) to the island corner (30, 20).
    let id = SketchId::mint("synthetic:test:id#bridge").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (80.0, 60.0));
    let island = rectangle(&id, "island", (30.0, 20.0), (50.0, 40.0));
    let sketch = sketch_with_loops(id.clone(), &[&island]);
    let mut entities = outer.clone();
    entities.extend(island.iter().cloned());
    entities.push(line_entity(&id, "bridge", (0.0, 0.0), (30.0, 20.0)));

    let (exterior, holes) = single_region(
        &sketch,
        &entities,
        &[ring(&outer, false), ring(&island, true)],
    );
    assert_eq!(
        ring_entities(&exterior),
        sorted_ids(&outer.iter().collect::<Vec<_>>())
    );
    assert!(exterior.iter().all(|use_| !use_.reversed));
    let [hole] = holes.as_slice() else {
        panic!("one hole ring: {holes:?}");
    };
    assert_eq!(
        ring_entities(hole),
        sorted_ids(&island.iter().collect::<Vec<_>>())
    );
    assert!(hole.iter().all(|use_| use_.reversed), "{hole:?}");

    let (interior, holes) = single_region(&sketch, &entities, &[ring(&island, false)]);
    assert!(holes.is_empty());
    assert!(interior.iter().all(|use_| !use_.reversed));

    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&outer, false)]),
        None
    );
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&outer, false), ring(&island, false)]
        ),
        None
    );
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&island, true)]),
        None
    );

    let program = outer
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<Vec<_>>();
    let ordered = |sketch: &Sketch, entities: &[SketchEntity]| {
        crate::test_support::with_decode_context(|ctx| {
            super::super::arrangement_region_matching_curve_program(
                sketch,
                entities,
                &program,
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
        .map(|found| found.region)
    };
    let both_loops = sketch_with_loops(id.clone(), &[&outer, &island]);
    assert_eq!(
        ordered(&both_loops, &entities),
        None,
        "the outer face encloses the island"
    );
    let outer_loop = sketch_with_loops(id, &[&outer]);
    assert!(
        ordered(&outer_loop, &outer).is_some(),
        "the outer face alone is a region"
    );
}

#[test]
fn a_counted_hole_ring_names_only_an_island_that_no_smaller_face_holds() {
    let id = SketchId::mint("synthetic:test:id#nested-islands").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (100.0, 100.0));
    let middle = rectangle(&id, "middle", (20.0, 20.0), (80.0, 80.0));
    let inner = rectangle(&id, "inner", (40.0, 40.0), (60.0, 60.0));
    let sketch = sketch_with_loops(id, &[&outer, &middle, &inner]);
    let entities = [outer.clone(), middle.clone(), inner.clone()].concat();

    for (exterior, island) in [(&outer, &middle), (&middle, &inner)] {
        let (_, holes) = single_region(
            &sketch,
            &entities,
            &[ring(exterior, false), ring(island, true)],
        );
        let [hole] = holes.as_slice() else {
            panic!("one hole ring: {holes:?}");
        };
        assert_eq!(
            ring_entities(hole),
            sorted_ids(&island.iter().collect::<Vec<_>>())
        );
    }
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&outer, false), ring(&inner, true)]
        ),
        None
    );
    let (_, holes) = single_region(&sketch, &entities, &[ring(&inner, false)]);
    assert!(holes.is_empty());
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&inner, false), ring(&inner, false)]
        ),
        None
    );

    let regions = member_regions(
        &sketch,
        &entities,
        &[
            ring(&inner, false),
            ring(&outer, false),
            ring(&middle, true),
        ],
    )
    .expect("two regions");
    let [SketchProfileRegion::Trimmed {
        outer_boundary: innermost,
        hole_boundaries: innermost_holes,
    }, SketchProfileRegion::Trimmed {
        outer_boundary: exterior,
        hole_boundaries,
    }] = regions.as_slice()
    else {
        panic!("two trimmed regions: {regions:?}");
    };
    assert_eq!(
        ring_entities(innermost),
        sorted_ids(&inner.iter().collect::<Vec<_>>())
    );
    assert!(innermost_holes.is_empty());
    assert_eq!(
        ring_entities(exterior),
        sorted_ids(&outer.iter().collect::<Vec<_>>())
    );
    assert_eq!(hole_boundaries.len(), 1);
}

#[test]
fn counted_duplicate_members_select_the_region_around_a_circle_island() {
    // A 40 x 30 rectangle drawn twice and a circle inside it. The exterior
    // ring names both copies of each side.
    let id = SketchId::mint("synthetic:test:id#covered").unwrap();
    let first = rectangle(&id, "first", (0.0, 0.0), (40.0, 30.0));
    let second = rectangle(&id, "second", (0.0, 0.0), (40.0, 30.0));
    let circle = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#circle").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(20.0, 15.0),
            radius: Length::new(4.0).unwrap(),
        })
        .unwrap(),
    );
    let sketch = sketch_with_loops(id, &[&first, std::slice::from_ref(&circle)]);
    let mut entities = first.clone();
    entities.extend(second.iter().cloned());
    entities.push(circle.clone());
    let sides = [ring(&first, false), ring(&second, false)].concat();

    let (exterior, holes) =
        single_region(&sketch, &entities, &[sides.clone(), vec![(&circle, true)]]);
    assert_eq!(exterior.len(), 4);
    let [hole] = holes.as_slice() else {
        panic!("one hole ring: {holes:?}");
    };
    assert!(hole.iter().all(|use_| use_.entity == *circle.id()));

    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&first, false), vec![(&circle, true)]]
        ),
        None
    );
    let renamed = [
        ring(&first, false),
        ring(&second[..3], false),
        vec![(&circle, false)],
    ]
    .concat();
    assert_eq!(
        member_regions(&sketch, &entities, &[renamed, vec![(&circle, true)]]),
        None
    );
}

#[test]
fn a_coincident_edge_matches_when_one_of_its_curves_has_its_direction() {
    // A 10 x 5 rectangle drawn against its counterclockwise walk, whose
    // bottom side also lies on an earlier, longer line drawn the other way.
    // The longer line supplies the bottom edge.
    let id = SketchId::mint("synthetic:test:id#coincident-bottom").unwrap();
    let entities = vec![
        line_entity(&id, "top", (0.0, 5.0), (10.0, 5.0)),
        line_entity(&id, "right", (10.0, 5.0), (10.0, 0.0)),
        line_entity(&id, "left", (0.0, 0.0), (0.0, 5.0)),
        line_entity(&id, "long", (-5.0, 0.0), (15.0, 0.0)),
        line_entity(&id, "bottom", (10.0, 0.0), (0.0, 0.0)),
    ];
    let [top, right, left, long, bottom] = [0, 1, 2, 3, 4].map(|index| &entities[index]);
    let sketch = sketch_with_loops(
        id,
        &[&[top.clone(), right.clone(), left.clone(), bottom.clone()]],
    );
    let ring = |bottom_reversed: bool, long_reversed: bool| {
        vec![
            (top, true),
            (right, true),
            (left, true),
            (bottom, bottom_reversed),
            (long, long_reversed),
        ]
    };

    for (bottom_reversed, long_reversed) in [(true, true), (true, false), (false, false)] {
        let (exterior, holes) =
            single_region(&sketch, &entities, &[ring(bottom_reversed, long_reversed)]);
        assert!(holes.is_empty());
        assert_eq!(exterior.len(), 4);
        assert!(selects_coincident_edge(
            &sketch,
            &entities,
            &[ring(bottom_reversed, long_reversed)]
        ));
    }
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(false, true)]),
        None
    );
}

#[test]
fn a_hole_ring_of_coincident_circles_states_a_coincident_edge() {
    // A 40 x 30 rectangle around a circle island drawn twice.
    let id = SketchId::mint("synthetic:test:id#twin-island").unwrap();
    let sides = rectangle(&id, "sides", (0.0, 0.0), (40.0, 30.0));
    let circle = |name: &str| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(20.0, 15.0),
                radius: Length::new(4.0).unwrap(),
            })
            .unwrap(),
        )
    };
    let (first, twin) = (circle("first-circle"), circle("twin-circle"));
    let sketch = sketch_with_loops(id, &[&sides, std::slice::from_ref(&first)]);
    let mut entities = sides.clone();
    entities.push(first.clone());

    let single = [ring(&sides, false), vec![(&first, true)]];
    assert_eq!(single_region(&sketch, &entities, &single).1.len(), 1);
    assert!(!selects_coincident_edge(&sketch, &entities, &single));

    entities.push(twin.clone());
    let doubled = [ring(&sides, false), vec![(&first, true), (&twin, true)]];
    assert_eq!(single_region(&sketch, &entities, &doubled).1.len(), 1);
    assert!(selects_coincident_edge(&sketch, &entities, &doubled));
}

#[test]
fn internally_tangent_circles_leave_their_node_in_curvature_order() {
    // A radius-10 circle inside a radius-1000 circle, tangent at (0, 0):
    // both leave the node along x, and the smaller one turns faster.
    let id = SketchId::mint("synthetic:test:id#tangent-disks").unwrap();
    let circle = |name: &str, center: (f64, f64), radius: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(center.0, center.1),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let small = circle("small-disk", (0.0, 10.0), 10.0);
    let large = circle("large-disk", (0.0, 1000.0), 1000.0);
    let entities = vec![small.clone(), large.clone()];
    let sketch = sketch_with_loops(
        id,
        &[std::slice::from_ref(&small), std::slice::from_ref(&large)],
    );
    let faces = crate::test_support::with_decode_context(|ctx| {
        super::super::curve_arrangement_faces(
            &sketch,
            &entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
            true,
        )
    })
    .unwrap()
    .expect("the arrangement");
    let mut rings = faces
        .iter()
        .map(|face| {
            let mut curves = face.ring.curves().cloned().collect::<Vec<_>>();
            curves.sort_unstable();
            curves
        })
        .collect::<Vec<_>>();
    rings.sort_unstable();
    let mut both = vec![small.id().clone(), large.id().clone()];
    both.sort_unstable();
    assert_eq!(rings, [both, vec![small.id().clone()]]);
}

#[test]
fn departures_with_one_tangent_order_by_curvature_or_stay_unordered() {
    use super::super::{order_departures, Departure, DepartureCurvature};

    let departure = |angle: f64, curvature: DepartureCurvature| Departure { angle, curvature };
    let ordered = |departures: &[Departure]| {
        let mut order = (0..departures.len()).collect::<Vec<_>>();
        order_departures(&mut order, departures, PROFILE_PROGRAM_TOLERANCE).then_some(order)
    };
    let far = departure(2.0, DepartureCurvature::Exact(0.0));
    let tight = departure(0.0, DepartureCurvature::Exact(0.1));
    let wide = departure(0.0, DepartureCurvature::Exact(0.001));
    let right = departure(0.0, DepartureCurvature::Exact(-0.1));
    assert_eq!(ordered(&[tight, wide, right]), Some(vec![2, 1, 0]));
    assert_eq!(ordered(&[tight, tight, far]), None, "equal curvatures");
    let left_sign = departure(0.0, DepartureCurvature::Sign(1.0));
    let right_sign = departure(0.0, DepartureCurvature::Sign(-1.0));
    assert_eq!(ordered(&[left_sign, right_sign, far]), Some(vec![1, 0, 2]));
    assert_eq!(
        ordered(&[
            left_sign,
            departure(0.0, DepartureCurvature::Exact(0.0)),
            far
        ]),
        Some(vec![1, 0, 2]),
        "a straight departure has sign zero"
    );
    assert_eq!(ordered(&[left_sign, tight, far]), None, "equal signs");
    let chord = departure(0.5, DepartureCurvature::Unknown);
    assert!(ordered(&[chord, wide]).is_some(), "two edges need no order");
    assert_eq!(ordered(&[chord, wide, far]), None, "a chord departure");
    assert_eq!(
        ordered(&[
            departure(-std::f64::consts::PI, DepartureCurvature::Exact(0.0)),
            departure(std::f64::consts::PI, DepartureCurvature::Exact(0.1)),
            departure(1.0, DepartureCurvature::Exact(0.0)),
        ]),
        Some(vec![2, 0, 1]),
        "one tangent on both sides of the angle seam"
    );
    // Tangents 1e-8 apart: the radius-0.1 arc turns past the line within
    // the node's ball, so it orders after it.
    assert_eq!(
        ordered(&[
            departure(0.0, DepartureCurvature::Exact(10.0)),
            departure(1.0e-8, DepartureCurvature::Exact(0.0)),
            far,
        ]),
        Some(vec![1, 0, 2])
    );
}

#[test]
fn a_circle_resting_on_a_line_leaves_the_tangent_node_in_order() {
    // A unit circle on the bottom side of a 10 x 4 rectangle: at (0, 0) the
    // circle's clockwise tangent points along -x, as the westward side does.
    let id = SketchId::mint("synthetic:test:id#resting-disk").unwrap();
    let mut entities = rectangle(&id, "box", (-5.0, 0.0), (5.0, 4.0));
    let disk = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#resting-circle").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 1.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    );
    let sides = entities.clone();
    entities.push(disk.clone());
    let sketch = sketch_with_loops(id, &[&sides, std::slice::from_ref(&disk)]);
    let faces = crate::test_support::with_decode_context(|ctx| {
        super::super::curve_arrangement_faces(
            &sketch,
            &entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
            true,
        )
    })
    .unwrap()
    .expect("the arrangement");
    assert_eq!(faces.len(), 2);
    assert!(faces
        .iter()
        .any(|face| face.ring.curves().all(|curve| curve == disk.id())));
}

#[test]
fn a_circle_edge_across_two_pi_boxes_its_axis_extreme() {
    use super::super::{arrangement_edge_box, SketchArrangementEdge};

    let id = SketchId::mint("synthetic:test:id#wrap-circle").unwrap();
    let circle = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#wrap").unwrap(),
        id,
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0).unwrap(),
        })
        .unwrap(),
    );
    let range = [6.0, 1.0 + std::f64::consts::TAU];
    let end = |angle: f64| Point2::new(angle.cos(), angle.sin());
    let edge = SketchArrangementEdge {
        nodes: [0, 1],
        boundary: cadmpeg_ir::features::SketchProfileBoundaryUse {
            entity: circle.id().clone(),
            parameter_range: cadmpeg_ir::geometry::DirectedParameterRange::new(range).unwrap(),
            reversed: false,
        },
        polyline: vec![end(range[0]), end(range[1])],
        coincident: Vec::new(),
    };
    let certified = [Point2::new(-1.0, -1.0), Point2::new(1.0, 1.0)];
    let [low, high] = arrangement_edge_box(&edge, &[circle], certified).unwrap();
    assert_eq!(high.u, 1.0, "the box reaches (1, 0)");
    assert!(low.v < 0.0 && high.v > 0.8 && low.u > 0.5);
}

#[test]
fn a_clamped_nurbs_end_leaves_along_its_first_control_leg() {
    use super::super::{nurbs_departure, DepartureCurvature};
    use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};

    let curve = |knots: Vec<f64>, weights: Option<Vec<f64>>| {
        PcurveNurbs::new(
            2,
            knots,
            PcurveNurbsPoles::from_lanes(
                vec![
                    Point2::new(0.0, 0.0),
                    Point2::new(5.0, 0.0),
                    Point2::new(10.0, 5.0),
                ],
                weights,
            )
            .unwrap(),
            false,
        )
        .unwrap()
    };
    let clamped = curve(vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0], None);
    let start = nurbs_departure(&clamped, true).expect("clamped start");
    assert_eq!(start.angle, 0.0);
    assert!(matches!(start.curvature, DepartureCurvature::Sign(sign) if sign == 1.0));
    let end = nurbs_departure(&clamped, false).expect("clamped end");
    assert_eq!(end.angle, (-5.0_f64).atan2(-5.0));
    assert!(matches!(end.curvature, DepartureCurvature::Sign(sign) if sign == -1.0));
    let unclamped = curve(vec![0.0, 0.5, 1.0, 2.0, 3.0, 4.0], None);
    assert!(nurbs_departure(&unclamped, true).is_none());
    let negative = curve(
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        Some(vec![1.0, -1.0, 1.0]),
    );
    assert!(nurbs_departure(&negative, true).is_none());
    let rational = curve(
        vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
        Some(vec![1.0, 3.0, 0.5]),
    );
    let start = nurbs_departure(&rational, true).expect("rational start");
    assert_eq!(start.angle, 0.0);
    assert!(matches!(start.curvature, DepartureCurvature::Sign(sign) if sign == 1.0));
}

#[test]
fn a_coincident_curve_merges_once_in_its_own_direction() {
    let use_ = |name: &str, reversed: bool| SketchEntityUse {
        entity: SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        reversed,
    };
    let mut coincident = vec![use_("held", false)];
    assert!(super::super::merge_coincident(
        &mut coincident,
        use_("held", false)
    ));
    assert_eq!(coincident.len(), 1);
    assert!(super::super::merge_coincident(
        &mut coincident,
        use_("added", true)
    ));
    assert_eq!(coincident.len(), 2);
    assert!(!super::super::merge_coincident(
        &mut coincident,
        use_("held", true)
    ));
}

#[test]
fn an_exhausted_member_ring_budget_selects_nothing_and_marks_the_cap() {
    let (sketch, entities) = divided_circle(false);
    let [circle, line] = [&entities[0], &entities[1]];
    let rings = [vec![
        SketchEntityUse {
            entity: line.id().clone(),
            reversed: false,
        },
        SketchEntityUse {
            entity: circle.id().clone(),
            reversed: false,
        },
    ]];
    for (limit, selects) in [(usize::MAX, true), (0, false)] {
        let work = ProfileGeometryWork::with_arrangement_work(limit);
        let selected = crate::test_support::with_decode_context(|ctx| {
            super::super::arrangement_regions_matching_member_rings(
                &sketch,
                &entities,
                &rings,
                PROFILE_PROGRAM_TOLERANCE,
                &work,
                ctx,
            )
        })
        .unwrap();
        assert_eq!(selected.is_some(), selects, "limit {limit}");
        work.finish_selection();
        assert_eq!(work.losses().is_empty(), selects, "limit {limit}");
    }
}

#[test]
fn counted_hole_rings_follow_list_order_and_name_each_island_once() {
    let id = SketchId::mint("synthetic:test:id#two-islands").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (100.0, 50.0));
    let first = rectangle(&id, "first", (10.0, 10.0), (30.0, 30.0));
    let second = rectangle(&id, "second", (60.0, 10.0), (80.0, 30.0));
    let sketch = sketch_with_loops(id, &[&outer, &first, &second]);
    let entities = [outer.clone(), first.clone(), second.clone()].concat();
    let first_ids = sorted_ids(&first.iter().collect::<Vec<_>>());
    let second_ids = sorted_ids(&second.iter().collect::<Vec<_>>());

    for (rings, expected) in [
        (
            [ring(&outer, false), ring(&first, true), ring(&second, true)],
            [&first_ids, &second_ids],
        ),
        (
            [ring(&outer, false), ring(&second, true), ring(&first, true)],
            [&second_ids, &first_ids],
        ),
    ] {
        let (_, holes) = single_region(&sketch, &entities, &rings);
        assert_eq!(
            holes
                .iter()
                .map(|hole| ring_entities(hole))
                .collect::<Vec<_>>(),
            expected.map(Clone::clone)
        );
    }
    for rings in [
        vec![ring(&outer, false), ring(&first, true), ring(&first, true)],
        vec![ring(&outer, false), ring(&outer, false), ring(&first, true)],
        vec![ring(&outer, false), ring(&first, true)],
    ] {
        assert_eq!(member_regions(&sketch, &entities, &rings), None);
    }
}

#[test]
fn a_counted_ring_that_two_walks_match_selects_nothing() {
    // A triangle drawn twice in opposite directions inside a rectangle. With
    // every member forward, the triangle face and the outer walk of the
    // island each traverse one curve of every edge forward.
    let id = SketchId::mint("synthetic:test:id#doubled-triangle").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let corners = [(10.0, 10.0), (30.0, 10.0), (20.0, 20.0)];
    let forward = (0..3)
        .map(|index| {
            line_entity(
                &id,
                &format!("forward-{index}"),
                corners[index],
                corners[(index + 1) % 3],
            )
        })
        .collect::<Vec<_>>();
    let backward = (0..3)
        .map(|index| {
            line_entity(
                &id,
                &format!("backward-{index}"),
                corners[(index + 1) % 3],
                corners[index],
            )
        })
        .collect::<Vec<_>>();
    let sketch = sketch_with_loops(id, &[&outer, &forward]);
    let entities = [outer.clone(), forward.clone(), backward.clone()].concat();
    let both = |forward_reversed: bool, backward_reversed: bool| {
        [
            ring(&forward, forward_reversed),
            ring(&backward, backward_reversed),
        ]
        .concat()
    };
    assert_eq!(
        member_regions(&sketch, &entities, &[both(false, false)]),
        None
    );
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[ring(&outer, false), both(false, false)]
        ),
        None
    );

    let (_, holes) = single_region(&sketch, &entities, &[both(false, true)]);
    assert!(holes.is_empty());
    let (_, holes) = single_region(
        &sketch,
        &entities,
        &[ring(&outer, false), both(true, false)],
    );
    assert_eq!(holes.len(), 1);
}

#[test]
fn each_counted_member_states_the_direction_of_one_fragment() {
    // A line that ends on the bottom side at (5, 0) cuts it, so the walk of
    // the rectangle runs along two forward fragments of the bottom side.
    let id = SketchId::mint("synthetic:test:id#cut-bottom").unwrap();
    let mut entities = rectangle(&id, "side", (0.0, 0.0), (10.0, 5.0));
    let sketch = sketch_with_loops(id.clone(), &[&entities]);
    entities.push(line_entity(&id, "spur", (5.0, 0.0), (5.0, -3.0)));
    let [bottom, right, top, left] = [0, 1, 2, 3].map(|index| &entities[index]);
    let (exterior, _) = single_region(
        &sketch,
        &entities,
        &[vec![
            (bottom, false),
            (bottom, false),
            (right, false),
            (top, false),
            (left, false),
        ]],
    );
    assert_eq!(exterior.len(), 5);
    assert_eq!(
        member_regions(
            &sketch,
            &entities,
            &[vec![
                (bottom, false),
                (bottom, true),
                (right, false),
                (top, false),
                (left, false),
            ]],
        ),
        None
    );
}

#[test]
fn coincident_fragments_take_distinct_members_in_any_member_order() {
    // The bottom line runs under two shorter lines drawn the other way, so
    // each of its two fragments can take the bottom member or the member of
    // its own shorter line.
    let id = SketchId::mint("synthetic:test:id#two-shared-fragments").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (0.0, 0.0), (20.0, 0.0)),
        line_entity(&id, "right", (20.0, 0.0), (20.0, 5.0)),
        line_entity(&id, "top", (20.0, 5.0), (0.0, 5.0)),
        line_entity(&id, "left", (0.0, 5.0), (0.0, 0.0)),
        line_entity(&id, "first-half", (10.0, 0.0), (0.0, 0.0)),
        line_entity(&id, "second-half", (20.0, 0.0), (10.0, 0.0)),
    ];
    let [bottom, right, top, left, first_half, second_half] =
        [0, 1, 2, 3, 4, 5].map(|index| &entities[index]);
    let sketch = sketch_with_loops(id, &[&entities[..4]]);
    let sides = [(right, false), (top, false), (left, false)];
    for (first, second) in [
        ((first_half, true), (second_half, false)),
        ((first_half, false), (second_half, true)),
    ] {
        for members in [
            [(bottom, false), (bottom, true), first, second],
            [first, second, (bottom, true), (bottom, false)],
        ] {
            let ring = [members.as_slice(), sides.as_slice()].concat();
            let (exterior, holes) = single_region(&sketch, &entities, &[ring]);
            assert!(holes.is_empty());
            assert_eq!(exterior.len(), 5);
        }
    }
    let ring = [
        [
            (bottom, true),
            (bottom, true),
            (first_half, false),
            (second_half, false),
        ]
        .as_slice(),
        sides.as_slice(),
    ]
    .concat();
    assert_eq!(member_regions(&sketch, &entities, &[ring]), None);
    let ring = [
        [
            (bottom, false),
            (bottom, true),
            (first_half, false),
            (second_half, false),
        ]
        .as_slice(),
        sides.as_slice(),
    ]
    .concat();
    assert_eq!(member_regions(&sketch, &entities, &[ring]), None);
}

#[test]
fn a_curve_program_resolves_in_the_arrangement_of_the_profile_loops() {
    // The diagonal ends on the top side and lies in no profile loop, so the
    // profile-loop arrangement has one face: the whole rectangle.
    let id = SketchId::mint("synthetic:test:id#t-junction-program").unwrap();
    let mut entities = rectangle(&id, "side", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&entities]);
    entities.push(line_entity(&id, "diagonal", (0.0, 0.0), (20.0, 30.0)));
    let program = |indices: &[usize]| {
        let program = indices
            .iter()
            .map(|index| entities[*index].id().clone())
            .collect::<Vec<_>>();
        crate::test_support::with_decode_context(|ctx| {
            super::super::arrangement_region_matching_curve_program(
                &sketch,
                &entities,
                &program,
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
        .map(|found| found.region)
    };
    assert_eq!(program(&[1, 2, 4, 0]), None);
    let Some(SketchProfileRegion::Trimmed {
        outer_boundary,
        hole_boundaries,
    }) = program(&[0, 1, 2, 3])
    else {
        panic!("the whole rectangle");
    };
    assert!(hole_boundaries.is_empty());
    assert_eq!(outer_boundary.len(), 5);
}

#[test]
fn a_single_curve_fragment_keeps_the_member_that_only_it_can_take() {
    // The bottom line has a single-curve fragment from (0, 0) to (5, 0) and a
    // fragment that a shorter line drawn the other way also covers.
    let id = SketchId::mint("synthetic:test:id#forced-fragment").unwrap();
    let entities = vec![
        line_entity(&id, "bottom", (0.0, 0.0), (10.0, 0.0)),
        line_entity(&id, "right", (10.0, 0.0), (10.0, 5.0)),
        line_entity(&id, "top", (10.0, 5.0), (0.0, 5.0)),
        line_entity(&id, "left", (0.0, 5.0), (0.0, 0.0)),
        line_entity(&id, "half", (10.0, 0.0), (5.0, 0.0)),
    ];
    let [bottom, right, top, left, half] = [0, 1, 2, 3, 4].map(|index| &entities[index]);
    let sketch = sketch_with_loops(id, &[&entities[..4]]);
    let ring = |half_reversed: bool| {
        vec![
            (bottom, false),
            (bottom, true),
            (half, half_reversed),
            (right, false),
            (top, false),
            (left, false),
        ]
    };
    let (exterior, holes) = single_region(&sketch, &entities, &[ring(true)]);
    assert!(holes.is_empty());
    assert_eq!(exterior.len(), 5);
    assert_eq!(member_regions(&sketch, &entities, &[ring(false)]), None);
}

fn plain_sketch(id: SketchId) -> Sketch {
    Sketch {
        id,
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }
}

fn test_line(sketch: &SketchId, name: &str, start: (f64, f64), end: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Line {
            start: Point2::new(start.0, start.1),
            end: Point2::new(end.0, end.1),
        })
        .unwrap(),
    )
}

fn x_braced_window() -> (Sketch, Vec<SketchEntity>) {
    let id = SketchId::mint("synthetic:test:id#x-window").unwrap();
    let entities = vec![
        test_line(&id, "bottom", (0.0, 0.0), (10.0, 0.0)),
        test_line(&id, "right", (10.0, 0.0), (10.0, 10.0)),
        test_line(&id, "top", (10.0, 10.0), (0.0, 10.0)),
        test_line(&id, "left", (0.0, 10.0), (0.0, 0.0)),
        test_line(&id, "rise", (0.0, 0.0), (10.0, 10.0)),
        test_line(&id, "fall", (10.0, 0.0), (0.0, 10.0)),
    ];
    (plain_sketch(id), entities)
}

fn curve_ids(entities: &[SketchEntity], names: &[&str]) -> Vec<SketchEntityId> {
    names
        .iter()
        .map(|name| {
            entities
                .iter()
                .find(|entity| entity.id().as_str() == format!("synthetic:test:id#{name}"))
                .unwrap()
                .id()
                .clone()
        })
        .collect()
}

fn crossing_split(
    sketch: &Sketch,
    entities: &[SketchEntity],
    program: &[SketchEntityId],
) -> Option<Vec<SketchProfileRegion>> {
    crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_of_crossing_loop(
            sketch,
            entities,
            program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .map(|split| split.regions)
}

#[test]
fn self_crossing_loop_splits_into_the_four_triangles_of_an_x_braced_window() {
    let (sketch, entities) = x_braced_window();
    // One walk over the four triangles: each side once, each brace piece
    // once per adjoining triangle.
    let program = curve_ids(
        &entities,
        &[
            "bottom", "fall", "rise", "right", "rise", "fall", "top", "fall", "rise", "left",
            "rise", "fall",
        ],
    );
    let ordered = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .map(|found| found.region);
    assert_eq!(ordered, None, "no single face has the crossing loop");
    let regions = crossing_split(&sketch, &entities, &program).expect("four triangles");
    assert_eq!(regions.len(), 4);
    let atomic = crate::test_support::with_decode_context(|ctx| {
        super::super::atomic_sketch_regions(
            &sketch,
            &entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .unwrap();
    for region in &regions {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        assert_eq!(outer_boundary.len(), 3);
        let face = atomic
            .iter()
            .find(|face| face.boundary[..] == outer_boundary[..])
            .expect("the boundary of an atomic region");
        let area = cadmpeg_ir::math::planar::polygon_area_twice(&face.polyline)
            .unwrap()
            .get()
            .abs()
            / 2.0;
        assert!((area - 25.0).abs() < 1e-9, "{area}");
    }
    for index in 0..regions.len() {
        assert!(!regions[..index].contains(&regions[index]));
    }
}

#[test]
fn self_crossing_loop_selects_only_the_triangles_its_curves_count_out() {
    let (sketch, entities) = x_braced_window();
    let program = curve_ids(
        &entities,
        &["bottom", "fall", "rise", "top", "fall", "rise"],
    );
    let regions = crossing_split(&sketch, &entities, &program).expect("two opposite triangles");
    assert_eq!(regions.len(), 2);
}

#[test]
fn self_crossing_loop_skips_an_island_face_and_keeps_the_candidates_it_weighed() {
    // A 2 by 2 grid. The loop lists the curves of the lower-left and
    // upper-right quarters; the upper-left quarter uses the same curves but
    // encloses an island square, so it is no candidate. Without the island
    // the upper-left and lower-right quarters would tie with the selection.
    let id = SketchId::mint("synthetic:test:id#grid-island").unwrap();
    let entities = vec![
        test_line(&id, "bottom", (0.0, 0.0), (10.0, 0.0)),
        test_line(&id, "right", (10.0, 0.0), (10.0, 10.0)),
        test_line(&id, "top", (10.0, 10.0), (0.0, 10.0)),
        test_line(&id, "left", (0.0, 10.0), (0.0, 0.0)),
        test_line(&id, "mid", (0.0, 5.0), (10.0, 5.0)),
        test_line(&id, "split", (5.0, 0.0), (5.0, 10.0)),
        test_line(&id, "island-bottom", (1.0, 6.0), (2.0, 6.0)),
        test_line(&id, "island-right", (2.0, 6.0), (2.0, 7.0)),
        test_line(&id, "island-top", (2.0, 7.0), (1.0, 7.0)),
        test_line(&id, "island-left", (1.0, 7.0), (1.0, 6.0)),
    ];
    let sketch = plain_sketch(id);
    let program = curve_ids(
        &entities,
        &[
            "bottom", "split", "mid", "left", "top", "split", "mid", "right",
        ],
    );
    let split = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_of_crossing_loop(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("two diagonal quarters");
    assert!(!split.island_absence_assumed);
    let regions = split.regions;
    assert_eq!(regions.len(), 2);
    let names = |region: &SketchProfileRegion| {
        let SketchProfileRegion::Trimmed {
            outer_boundary,
            hole_boundaries,
        } = region
        else {
            panic!("a trimmed region");
        };
        assert!(hole_boundaries.is_empty());
        let mut names = outer_boundary
            .iter()
            .map(|use_| {
                use_.entity
                    .as_str()
                    .trim_start_matches("synthetic:test:id#")
                    .to_owned()
            })
            .collect::<Vec<_>>();
        names.sort();
        names.dedup();
        names
    };
    let mut quarters = regions.iter().map(names).collect::<Vec<_>>();
    quarters.sort();
    assert_eq!(
        quarters,
        [
            vec!["bottom", "left", "mid", "split"],
            vec!["mid", "right", "split", "top"],
        ]
    );
}

#[test]
fn self_crossing_loop_refuses_counts_no_set_of_regions_reaches() {
    let (sketch, entities) = x_braced_window();
    let program = curve_ids(&entities, &["bottom", "fall", "rise", "rise", "fall"]);
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
}

#[test]
fn self_crossing_loop_refuses_two_sets_with_the_same_counts() {
    // A disk of two semicircles cut by a horizontal and a vertical chord: the
    // upper-right and upper-left quarters use the same curves.
    let id = SketchId::mint("synthetic:test:id#quartered").unwrap();
    let arc = |name: &str, start: f64, end: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(5.0).unwrap(),
                start_angle: Angle::new(start).unwrap(),
                end_angle: Angle::new(end).unwrap(),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        arc("upper", 0.0, std::f64::consts::PI),
        arc("lower", std::f64::consts::PI, std::f64::consts::TAU),
        test_line(&id, "across", (-5.0, 0.0), (5.0, 0.0)),
        test_line(&id, "down", (0.0, -5.0), (0.0, 5.0)),
    ];
    let sketch = plain_sketch(id);
    let program = curve_ids(&entities, &["across", "upper", "down"]);
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
}

#[test]
fn self_crossing_loop_refuses_a_carrier_that_is_not_a_line_or_arc() {
    let (sketch, mut entities) = divided_circle(true);
    let program = [
        entities[0].id().clone(),
        entities[1].id().clone(),
        entities[0].id().clone(),
        entities[2].id().clone(),
    ];
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
    entities.truncate(1);
    assert_eq!(crossing_split(&sketch, &entities, &program), None);
}

#[test]
fn self_crossing_loop_over_the_candidate_limit_reports_the_cap() {
    // A rectangle cut by 16 verticals has 17 atomic regions, all in the loop.
    let id = SketchId::mint("synthetic:test:id#comb").unwrap();
    let mut entities = vec![
        test_line(&id, "bottom", (0.0, 0.0), (17.0, 0.0)),
        test_line(&id, "right", (17.0, 0.0), (17.0, 1.0)),
        test_line(&id, "top", (17.0, 1.0), (0.0, 1.0)),
        test_line(&id, "left", (0.0, 1.0), (0.0, 0.0)),
    ];
    for x in 1..=16 {
        entities.push(test_line(
            &id,
            &format!("cut{x}"),
            (f64::from(x), 0.0),
            (f64::from(x), 1.0),
        ));
    }
    let sketch = plain_sketch(id);
    let mut names = vec!["bottom".to_owned(), "right".to_owned(), "top".to_owned()];
    names.push("left".to_owned());
    names.extend((1..=16).map(|x| format!("cut{x}")));
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    let program = curve_ids(&entities, &names);
    let work = ProfileGeometryWork::new();
    let split = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_regions_of_crossing_loop(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &work,
            ctx,
        )
    })
    .unwrap()
    .map(|split| split.regions);
    assert_eq!(split, None);
    work.finish_selection();
    let losses = work.losses();
    let [loss] = losses.as_slice() else {
        panic!("expected one loss, got {losses:?}");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeatureProfileCrossingLoopCapped.kind()
    );
    assert!(loss.message.contains("MAX_CROSSING_LOOP_CANDIDATES (16)"));
}
