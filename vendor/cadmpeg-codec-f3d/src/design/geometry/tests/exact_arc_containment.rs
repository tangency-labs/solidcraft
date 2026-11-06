// SPDX-License-Identifier: Apache-2.0

use super::*;

const EPS_EXACT_ARC_REGION: f64 = 1.0e-6;

fn sketch_id() -> SketchId {
    SketchId::mint("synthetic:test:id#exact-arc").unwrap()
}

fn entity(name: &str, definition: SketchGeometryDefinition) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint(format!("synthetic:test:id#exact-arc-{name}")).unwrap(),
        sketch_id(),
        SketchGeometry::try_from(definition).unwrap(),
    )
}

fn line(name: &str, start: Point2, end: Point2) -> SketchEntity {
    entity(name, SketchGeometryDefinition::Line { start, end })
}

fn arc(name: &str, center: Point2, radius: f64, start_angle: f64, end_angle: f64) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Arc {
            center,
            radius: Length::new(radius).unwrap(),
            start_angle: Angle::new(start_angle).unwrap(),
            end_angle: Angle::new(end_angle).unwrap(),
        },
    )
}

fn circle(name: &str, center: Point2, radius: f64) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Circle {
            center,
            radius: Length::new(radius).unwrap(),
        },
    )
}

fn quadratic_nurbs(name: &str, points: [Point2; 3], weights: Option<Vec<f64>>) -> SketchEntity {
    entity(
        name,
        SketchGeometryDefinition::Nurbs {
            curve: PcurveNurbs::from_lanes(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                points.to_vec(),
                weights,
                false,
            )
            .unwrap(),
        },
    )
}

/// A 10 x 10 square whose top side is a rational NURBS that rises to about
/// v = 10.86 at u = 5.
fn nurbs_topped_square() -> Vec<SketchEntity> {
    vec![
        line("bottom", Point2::new(0.0, 0.0), Point2::new(10.0, 0.0)),
        line("right", Point2::new(10.0, 0.0), Point2::new(10.0, 10.0)),
        quadratic_nurbs(
            "top",
            [
                Point2::new(10.0, 10.0),
                Point2::new(5.0, 12.0),
                Point2::new(0.0, 10.0),
            ],
            Some(vec![1.0, 0.75, 1.0]),
        ),
        line("left", Point2::new(0.0, 10.0), Point2::new(0.0, 0.0)),
    ]
}

fn sketch(profiles: &[&[SketchEntity]]) -> Sketch {
    let profiles = profiles
        .iter()
        .map(|profile| {
            profile
                .iter()
                .map(|entity| SketchEntityUse {
                    entity: entity.id().clone(),
                    reversed: false,
                })
                .collect::<Vec<_>>()
        })
        .collect::<Vec<_>>();
    Sketch {
        id: sketch_id(),
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

/// Resolve the region at `point` and require that no local cap skipped work.
fn region_at(
    sketch: &Sketch,
    entities: &[SketchEntity],
    point: Point2,
) -> Option<SketchProfileRegion> {
    let work = local_profile_work();
    let region = crate::test_support::with_decode_context(|decode_ctx| {
        region_containing_points(
            sketch,
            entities,
            &[Point3::new(point.u, point.v, 0.0)],
            EPS_EXACT_ARC_REGION,
            &work,
            decode_ctx,
        )
    })
    .unwrap();
    work.finish_selection();
    let losses = work.losses();
    assert!(losses.is_empty(), "unexpected losses {losses:?}");
    region
}

fn loops(outer: u32, holes: Vec<u32>) -> SketchProfileRegion {
    SketchProfileRegion::loops(outer, holes).unwrap()
}

fn line_square(prefix: &str, low: Point2, side: f64) -> Vec<SketchEntity> {
    let corners = [
        low,
        Point2::new(low.u + side, low.v),
        Point2::new(low.u + side, low.v + side),
        Point2::new(low.u, low.v + side),
    ];
    (0..corners.len())
        .map(|index| {
            line(
                &format!("{prefix}-{index}"),
                corners[index],
                corners[(index + 1) % corners.len()],
            )
        })
        .collect()
}

/// Resolve the arrangement region at `point` and require that no local cap
/// skipped work.
fn arrangement_region_at(
    sketch: &Sketch,
    entities: &[SketchEntity],
    point: Point2,
) -> Option<SketchProfileRegion> {
    let work = local_profile_work();
    let region = crate::test_support::with_decode_context(|decode_ctx| {
        arrangement_region_containing_points(
            sketch,
            entities,
            &[point],
            EPS_EXACT_ARC_REGION,
            &work,
            decode_ctx,
        )
    })
    .unwrap();
    work.finish_selection();
    let losses = work.losses();
    assert!(losses.is_empty(), "unexpected losses {losses:?}");
    region
}

#[test]
fn nurbs_bounded_profile_with_circular_hole_resolves_region_and_hole() {
    let outline = nurbs_topped_square();
    let hole = [circle("hole", Point2::new(5.0, 5.0), 2.0)];
    let sketch = sketch(&[&outline, &hole]);
    let entities = [outline.as_slice(), &hole].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(1.0, 1.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5.0, 5.0)),
        Some(loops(1, Vec::new()))
    );
}

#[test]
fn circle_around_nurbs_bounded_island_resolves_ring_and_island() {
    let rim = [circle("rim", Point2::new(5.0, 5.0), 20.0)];
    let island = nurbs_topped_square();
    let sketch = sketch(&[&rim, &island]);
    let entities = [rim.as_slice(), &island].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5.0, 20.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(1.0, 1.0)),
        Some(loops(1, Vec::new()))
    );
}

#[test]
fn nurbs_bounded_profile_with_slot_hole_resolves_region_and_hole() {
    use std::f64::consts::FRAC_PI_2;

    let outline = nurbs_topped_square();
    let slot = [
        line("slot-low", Point2::new(3.0, 4.0), Point2::new(7.0, 4.0)),
        arc(
            "slot-right",
            Point2::new(7.0, 5.0),
            1.0,
            -FRAC_PI_2,
            FRAC_PI_2,
        ),
        line("slot-high", Point2::new(7.0, 6.0), Point2::new(3.0, 6.0)),
        arc(
            "slot-left",
            Point2::new(3.0, 5.0),
            1.0,
            FRAC_PI_2,
            3.0 * FRAC_PI_2,
        ),
    ];
    let sketch = sketch(&[&outline, &slot]);
    let entities = [outline.as_slice(), &slot].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(1.0, 1.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5.0, 5.0)),
        Some(loops(1, Vec::new()))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(7.5, 5.0)),
        Some(loops(1, Vec::new()))
    );
}

/// The arc side of the C-shaped loop is about 60 800 long, so a tube cover at
/// the NURBS target error of the loop would need several hundred thousand
/// tubes.
#[test]
fn nurbs_bounded_profile_with_large_arc_side_resolves_region_and_hole() {
    const RADIUS: f64 = 10_000.0;
    const GAP: f64 = 0.1;
    let end_angle = std::f64::consts::TAU - GAP;
    let on_rim = |angle: f64| Point2::new(RADIUS * angle.cos(), RADIUS * angle.sin());
    let outline = [
        arc("c-arc", Point2::new(0.0, 0.0), RADIUS, GAP, end_angle),
        quadratic_nurbs(
            "c-closing",
            [on_rim(end_angle), Point2::new(10_100.0, 0.0), on_rim(GAP)],
            None,
        ),
    ];
    let hole = [circle("c-hole", Point2::new(0.0, 0.0), 100.0)];
    let sketch = sketch(&[&outline, &hole]);
    let entities = [outline.as_slice(), &hole].concat();

    assert_eq!(
        region_at(&sketch, &entities, Point2::new(5_000.0, 0.0)),
        Some(loops(0, vec![1]))
    );
    assert_eq!(
        region_at(&sketch, &entities, Point2::new(0.0, 0.0)),
        Some(loops(1, Vec::new()))
    );
}

/// Each half of the rim is about 31 400 long, so covering it with tubes at
/// the arrangement's NURBS target error of about 0.1 would take some 314 000
/// tubes.
#[test]
fn arrangement_certifies_a_large_circle_around_a_nurbs_profile_exactly() {
    let rim = [circle("far-rim", Point2::new(5.0, 5.0), 10_000.0)];
    let island = nurbs_topped_square();
    let sketch = sketch(&[&rim, &island]);
    let entities = [rim.as_slice(), &island].concat();
    let work = local_profile_work();
    let faces = crate::test_support::with_decode_context(|decode_ctx| {
        sketch_arrangement_faces(&sketch, &entities, EPS_EXACT_ARC_REGION, &work, decode_ctx)
    })
    .unwrap();

    // The disc of the rim and the square.
    assert_eq!(faces.map(|faces| faces.len()), Some(2));
    work.finish_selection();
    assert!(work.losses().is_empty());
}

/// The square's arrangement face has no hole ring for the circle inside it,
/// so it is not the region between them; whole profiles name the hole.
#[test]
fn arrangement_region_refuses_a_face_around_a_circular_hole() {
    let outline = line_square("plate", Point2::new(0.0, 0.0), 10.0);
    let hole = [circle("plate-hole", Point2::new(5.0, 5.0), 2.0)];
    let sketch = sketch(&[&outline, &hole]);
    let entities = [outline.as_slice(), &hole].concat();
    let between = Point2::new(1.0, 1.0);

    assert_eq!(arrangement_region_at(&sketch, &entities, between), None);
    assert_eq!(
        region_at(&sketch, &entities, between),
        Some(loops(0, vec![1]))
    );
}

#[test]
fn arrangement_region_keeps_a_face_beside_a_separate_circle() {
    let outline = line_square("block", Point2::new(0.0, 0.0), 10.0);
    let disc = [circle("block-disc", Point2::new(20.0, 5.0), 2.0)];
    let sketch = sketch(&[&outline, &disc]);
    let entities = [outline.as_slice(), &disc].concat();

    for point in [Point2::new(1.0, 1.0), Point2::new(20.0, 5.0)] {
        assert!(matches!(
            arrangement_region_at(&sketch, &entities, point),
            Some(SketchProfileRegion::Trimmed { .. })
        ));
    }
}
