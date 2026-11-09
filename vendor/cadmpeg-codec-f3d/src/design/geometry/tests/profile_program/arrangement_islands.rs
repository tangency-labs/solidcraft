// SPDX-License-Identifier: Apache-2.0

use super::*;

/// The member-ring selection of `rings`.
fn selection(
    sketch: &Sketch,
    entities: &[SketchEntity],
    rings: &[Vec<(&SketchEntity, bool)>],
) -> super::super::super::MemberRingRegions {
    member_selection(sketch, entities, rings).expect("selected rings")
}

/// The hole count of the face with `curve`.
fn face_with(
    faces: &[super::super::super::SketchArrangementFace],
    curve: &SketchEntity,
) -> Option<usize> {
    let face = faces
        .iter()
        .find(|face| face.ring.curves().any(|candidate| candidate == curve.id()))
        .expect("the face");
    face.holes.as_ref().map(Vec::len)
}

/// The arrangement faces of every curve of `entities` in `sketch`.
fn all_faces(
    sketch: &Sketch,
    entities: &[SketchEntity],
) -> Vec<super::super::super::SketchArrangementFace> {
    crate::test_support::with_decode_context(|ctx| {
        super::super::super::curve_arrangement_faces(
            sketch,
            entities,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
            true,
        )
    })
    .unwrap()
    .expect("the arrangement")
}

#[test]
fn a_full_ellipse_leaves_counted_members_without_a_region() {
    let id = SketchId::mint("synthetic:test:id#ellipse-island").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&outer]);
    let mut entities = outer.clone();
    entities.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#ellipse").unwrap(),
        id,
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(20.0, 15.0),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(6.0).unwrap(),
                minor_radius: Length::new(3.0).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    ));
    assert_eq!(
        member_regions(&sketch, &entities, &[ring(&outer, false)]),
        None
    );
}

/// A full ellipse of `sketch` centred at `center`, with radii 6 and 3.
fn full_ellipse(sketch: &SketchId, center: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#far-ellipse").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(center.0, center.1),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(6.0).unwrap(),
                minor_radius: Length::new(3.0).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    )
}

#[test]
fn a_full_ellipse_beside_the_selected_face_keeps_the_selection() {
    let id = SketchId::mint("synthetic:test:id#ellipse-far").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let sketch = sketch_with_loops(id.clone(), &[&outer]);
    let rings = [ring(&outer, false)];
    let mut far = outer.clone();
    far.push(full_ellipse(&id, (100.0, 100.0)));
    let (exterior, holes) = single_region(&sketch, &far, &rings);
    assert_eq!(exterior.len(), 4);
    assert!(holes.is_empty());
    assert!(!selection(&sketch, &far, &rings).island_absence);
    // Radii 6 and 3 centred 5 beyond the side u = 40: the box spans u from
    // 39 to 51, across the side.
    let mut near = outer.clone();
    near.push(full_ellipse(&id, (45.0, 15.0)));
    assert!(
        selection(&sketch, &near, &rings).island_absence,
        "the face cannot exclude the ellipse"
    );
    // The major axis along v: the exact box spans u from 41 to 47.
    let mut turned = outer.clone();
    turned.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#turned-ellipse").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(44.0, 15.0),
            major_angle: cadmpeg_ir::scalar::Angle::new(std::f64::consts::FRAC_PI_2).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(6.0).unwrap(),
                minor_radius: Length::new(3.0).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    ));
    assert!(!selection(&sketch, &turned, &rings).island_absence);
    let mut unbounded = outer.clone();
    unbounded.push(SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#far-reference").unwrap(),
        id,
        SketchGeometry::try_from(SketchGeometryDefinition::ReferenceLine {
            origin: Point2::new(100.0, 100.0),
            direction: Point2::new(1.0, 0.0),
        })
        .unwrap(),
    ));
    assert!(
        selection(&sketch, &unbounded, &rings).island_absence,
        "a reference line has no box"
    );
}
/// A full ellipse of `sketch` centred at `center`, with radii 3e-5 and 2e-5.
fn tiny_ellipse(sketch: &SketchId, center: (f64, f64)) -> SketchEntity {
    SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#tiny-ellipse").unwrap(),
        sketch.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
            center: Point2::new(center.0, center.1),
            major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
            radii: cadmpeg_ir::sketches::EllipseRadii {
                major_radius: Length::new(3.0e-5).unwrap(),
                minor_radius: Length::new(2.0e-5).unwrap(),
            },
            bounds: None,
        })
        .unwrap(),
    )
}

#[test]
fn an_ellipse_beyond_the_sampled_bounds_of_a_curved_face_is_found_by_the_exact_arc() {
    // A radius-10 arc over 180 degrees, closed by its diameter in two
    // halves. The arc's extreme point lies midway between two polyline
    // vertices, so the polyline box stops 1.9e-4 short of the arc. An
    // ellipse 1e-4 inside the extreme point lies outside the polyline box
    // and inside the face; another straddles the extreme point.
    let id = SketchId::mint("synthetic:test:id#d-face").unwrap();
    let point = |degrees: f64| {
        let angle = degrees.to_radians();
        (10.0 * angle.cos(), 10.0 * angle.sin())
    };
    for (start, extreme) in [(-80.507_812_5, (10.0, 0.0)), (99.492_187_5, (-10.0, 0.0))] {
        let arc = SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#d-arc").unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Arc {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(10.0).unwrap(),
                start_angle: Angle::new(f64::to_radians(start)).unwrap(),
                end_angle: Angle::new(f64::to_radians(start + 180.0)).unwrap(),
            })
            .unwrap(),
        );
        let (top, bottom) = (point(start + 180.0), point(start));
        let middle = (
            f64::midpoint(top.0, bottom.0),
            f64::midpoint(top.1, bottom.1),
        );
        let upper = line_entity(&id, "d-upper", top, middle);
        let lower = line_entity(&id, "d-lower", middle, bottom);
        let curves = vec![arc.clone(), upper.clone(), lower.clone()];
        let sketch = sketch_with_loops(id.clone(), &[&curves]);
        let rings = [vec![(&arc, false), (&upper, false), (&lower, false)]];
        let (exterior, holes) = single_region(&sketch, &curves, &rings);
        assert_eq!(exterior.len(), 3, "arc from {start} degrees");
        assert!(holes.is_empty());
        // Far from the face, and inside the face box beyond the arc.
        for center in [(3.0 * extreme.0, 0.0), (0.8 * extreme.0, 8.0)] {
            let mut beside = curves.clone();
            beside.push(tiny_ellipse(&id, center));
            assert!(
                !selection(&sketch, &beside, &rings).island_absence,
                "ellipse at {center:?}"
            );
        }
        let mut inside = curves.clone();
        inside.push(tiny_ellipse(&id, (0.999_99 * extreme.0, 0.0)));
        assert_eq!(
            member_regions(&sketch, &inside, &rings),
            None,
            "the face holds an island that no ring names"
        );
        let mut straddling = curves.clone();
        straddling.push(tiny_ellipse(&id, extreme));
        let (exterior, holes) = single_region(&sketch, &straddling, &rings);
        assert_eq!(exterior.len(), 3, "arc from {start} degrees");
        assert!(holes.is_empty());
        assert!(
            selection(&sketch, &straddling, &rings).island_absence,
            "arc from {start} degrees"
        );
    }
}
#[test]
fn the_innermost_container_holds_the_island() {
    // Two concentric circles around a small square: the square lies in both
    // disks, and the inner disk lies inside the outer one.
    let id = SketchId::mint("synthetic:test:id#nested-disks").unwrap();
    let circle = |name: &str, radius: f64| {
        SketchEntity::new(
            SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                center: Point2::new(0.0, 0.0),
                radius: Length::new(radius).unwrap(),
            })
            .unwrap(),
        )
    };
    let square = rectangle(&id, "core", (-1.0, -1.0), (1.0, 1.0));
    let inner_disk = |inner_radius: f64, outer_radius: f64| {
        let inner = circle("inner-disk", inner_radius);
        let outer = circle("outer-disk", outer_radius);
        let mut entities = vec![inner.clone(), outer.clone()];
        entities.extend(square.iter().cloned());
        let sketch = sketch_with_loops(
            id.clone(),
            &[
                std::slice::from_ref(&inner),
                std::slice::from_ref(&outer),
                &square,
            ],
        );
        face_with(&all_faces(&sketch, &entities), &inner)
    };
    assert_eq!(inner_disk(10.0, 13.0), Some(1));
    assert_eq!(inner_disk(10.0, 10.1), Some(1), "a thin wall still nests");
    // Radius 1e4: each circle's polyline sagitta is about 0.19, more than
    // the 0.15 wall, and the exact arcs order the two disks.
    assert_eq!(inner_disk(1.0e4, 1.0e4 + 0.15), Some(1));
}
#[test]
fn an_island_near_a_face_boundary_takes_the_side_its_pieces_give() {
    // A radius-1e4 disk: its polyline sagitta is about 0.19, and its exact
    // arcs decide every island 0.14 or more from the circle.
    let id = SketchId::mint("synthetic:test:id#near-island").unwrap();
    let disk = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#disk").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(1.0e4).unwrap(),
        })
        .unwrap(),
    );
    let faces = |island: &[SketchEntity]| {
        let mut entities = vec![disk.clone()];
        entities.extend(island.iter().cloned());
        let sketch = sketch_with_loops(id.clone(), &[std::slice::from_ref(&disk), island]);
        all_faces(&sketch, &entities)
    };
    let inside = rectangle(&id, "near", (9_999.85, -0.005), (9_999.86, 0.005));
    assert_eq!(face_with(&faces(&inside), &disk), Some(1));
    let frame = rectangle(&id, "frame", (-2.0e4, -2.0e4), (2.0e4, 2.0e4));
    let outside = rectangle(&id, "outside", (10_000.14, -0.005), (10_000.15, 0.005));
    let mut framed = frame.clone();
    framed.extend(outside.iter().cloned());
    let arranged = faces(&framed);
    assert_eq!(face_with(&arranged, &frame[0]), Some(2));
    assert_eq!(face_with(&arranged, &disk), Some(0));

    // A triangle whose tip lies 5e-8 inside a radius-10 circle, within the
    // linear tolerance of the arc: the tip is a node of the circle, so the
    // triangle is no island of the disk.
    let small = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#small-disk").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(10.0).unwrap(),
        })
        .unwrap(),
    );
    let tip = (0.0, 10.0 - 5.0e-8);
    let triangle = vec![
        line_entity(&id, "tip-left", (-1.0, 9.0), tip),
        line_entity(&id, "tip-right", tip, (1.0, 9.0)),
        line_entity(&id, "tip-back", (1.0, 9.0), (-1.0, 9.0)),
    ];
    let mut entities = vec![small.clone()];
    entities.extend(triangle.iter().cloned());
    let sketch = sketch_with_loops(id.clone(), &[std::slice::from_ref(&small), &triangle]);
    assert_eq!(face_with(&all_faces(&sketch, &entities), &small), Some(0));

    // The disk alone as a curve program: the island outside it leaves the
    // region whole.
    let mut entities = vec![disk.clone()];
    entities.extend(outside.iter().cloned());
    let sketch = sketch_with_loops(id.clone(), &[std::slice::from_ref(&disk), &outside]);
    let found = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            std::slice::from_ref(disk.id()),
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("the disk region");
    assert!(!found.island_absence_assumed);
}
#[test]
fn an_ellipse_clear_of_the_exact_face_box_keeps_the_selection() {
    // The upper half of a radius-10 disk, and an ellipse 0.03 beyond the
    // disk on the x axis.
    let id = SketchId::mint("synthetic:test:id#upper-half").unwrap();
    let disk = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#half-disk").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Circle {
            center: Point2::new(0.0, 0.0),
            radius: Length::new(10.0).unwrap(),
        })
        .unwrap(),
    );
    let diameter = line_entity(&id, "diameter", (-10.0, 0.0), (10.0, 0.0));
    let ellipse = |center: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint("synthetic:test:id#clear-ellipse").unwrap(),
            id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Ellipse {
                center: Point2::new(center.0, center.1),
                major_angle: cadmpeg_ir::scalar::Angle::new(0.0).unwrap(),
                radii: cadmpeg_ir::sketches::EllipseRadii {
                    major_radius: Length::new(0.01).unwrap(),
                    minor_radius: Length::new(0.005).unwrap(),
                },
                bounds: None,
            })
            .unwrap(),
        )
    };
    let curves = vec![disk.clone(), diameter.clone()];
    let sketch = sketch_with_loops(id.clone(), &[&curves]);
    let ring = [vec![(&disk, false), (&diameter, false)]];
    let (exterior, _) = single_region(&sketch, &curves, &ring);
    assert_eq!(exterior.len(), 2);
    // Beside the disk, and inside the lower half, which the selected upper
    // half's box excludes.
    for center in [(10.04, 0.0), (0.0, -5.0)] {
        let mut with_ellipse = curves.clone();
        with_ellipse.push(ellipse(center));
        assert!(
            !selection(&sketch, &with_ellipse, &ring).island_absence,
            "ellipse at {center:?}"
        );
    }
}

#[test]
fn a_point_beside_an_exact_edge_of_a_face_with_a_curved_edge_selects_the_face() {
    // A 100 x 10 face whose top side is a NURBS: its polyline band is wide,
    // and the bottom line's band is zero.
    use cadmpeg_ir::geometry::pcurve::{PcurveNurbs, PcurveNurbsPoles};

    let id = SketchId::mint("synthetic:test:id#nurbs-top").unwrap();
    let top = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#nurbs-top-curve").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Nurbs {
            curve: PcurveNurbs::new(
                2,
                vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0],
                PcurveNurbsPoles::from_lanes(
                    vec![
                        Point2::new(100.0, 10.0),
                        Point2::new(50.0, 12.0),
                        Point2::new(0.0, 10.0),
                    ],
                    None,
                )
                .unwrap(),
                false,
            )
            .unwrap(),
        })
        .unwrap(),
    );
    let entities = vec![
        line_entity(&id, "nurbs-bottom", (0.0, 0.0), (100.0, 0.0)),
        line_entity(&id, "nurbs-right", (100.0, 0.0), (100.0, 10.0)),
        top,
        line_entity(&id, "nurbs-left", (0.0, 10.0), (0.0, 0.0)),
    ];
    let sketch = sketch_with_loops(id, &[&entities]);
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_containing_points(
            &sketch,
            &entities,
            &[Point2::new(50.0, 0.1)],
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("the face");
    assert!(!region.polyline_side_assumed);
    // The top is 10 + 4s(1 - s) at u = 100s. At u = 1e-5 it lies 4e-7 above
    // v = 10, and the chord from (0, 10) to any later polyline vertex up to
    // u = 50 at least 2e-7 above it. A point 2.5e-7 below the top lies in
    // the NURBS tube and inside the polyline.
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_containing_points(
            &sketch,
            &entities,
            &[Point2::new(1.0e-5, 10.0 + 1.5e-7)],
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("the face");
    assert!(region.polyline_side_assumed);
    let above = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_containing_points(
            &sketch,
            &entities,
            &[Point2::new(1.0e-5, 10.0 + 6.0e-7)],
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert!(above.is_none(), "2e-7 above the top");
}

#[test]
fn a_point_between_an_arc_and_its_polyline_selects_the_face_the_arc_bounds() {
    // A 20 x 20 square split by an arc about (0, 10) from (10, 0) to
    // (10, 20) that bulges to x = 14.14. The arc polyline has vertices at 0
    // and pi / 512 rad; a point at half that angle, 3e-5 inside the arc,
    // lies in the left face but beyond the chord, where the right face's
    // polyline holds it.
    let id = SketchId::mint("synthetic:test:id#bulge").unwrap();
    let radius = 200.0_f64.sqrt();
    let arc = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#bulge-arc").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 10.0),
            radius: Length::new(radius).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_4).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        })
        .unwrap(),
    );
    let mut entities = rectangle(&id, "frame", (0.0, 0.0), (20.0, 20.0));
    let right = entities[1].id().clone();
    entities.push(arc.clone());
    let sketch = sketch_with_loops(id, &[&entities]);
    let region = |point: Point2| {
        let found = crate::test_support::with_decode_context(|ctx| {
            super::super::super::arrangement_region_containing_points(
                &sketch,
                &entities,
                &[point],
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
        .expect("one face");
        let SketchProfileRegion::Trimmed { outer_boundary, .. } = found.region else {
            panic!("a trimmed region");
        };
        let on_right = outer_boundary.iter().any(|use_| use_.entity == right);
        (on_right, found.polyline_side_assumed)
    };
    assert_eq!(region(Point2::new(5.0, 10.0)), (false, false));
    assert_eq!(region(Point2::new(17.0, 10.0)), (true, false));
    let half_step = std::f64::consts::PI / 1024.0;
    let inside = radius - 3.0e-5;
    assert_eq!(
        region(Point2::new(
            inside * half_step.cos(),
            10.0 + inside * half_step.sin()
        )),
        (false, false)
    );
}

#[test]
fn a_reversed_arc_use_keeps_the_face_its_arc_bounds() {
    // The bulge square with the arc listed against its direction: (12, 10)
    // lies 12 from the arc centre, inside the radius 14.14, in the left face.
    let id = SketchId::mint("synthetic:test:id#reversed-bulge").unwrap();
    let arc = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#reversed-arc").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(0.0, 10.0),
            radius: Length::new(200.0_f64.sqrt()).unwrap(),
            start_angle: Angle::new(-std::f64::consts::FRAC_PI_4).unwrap(),
            end_angle: Angle::new(std::f64::consts::FRAC_PI_4).unwrap(),
        })
        .unwrap(),
    );
    let mut entities = rectangle(&id, "frame", (0.0, 0.0), (20.0, 20.0));
    let right = entities[1].id().clone();
    entities.push(arc.clone());
    let mut sketch = sketch_with_loops(id, &[&entities]);
    sketch.profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(vec![entities
        .iter()
        .map(|entity| SketchEntityUse {
            entity: entity.id().clone(),
            reversed: entity.id() == arc.id(),
        })
        .collect::<Vec<_>>()])
    .unwrap();
    for (point, on_right) in [((12.0, 10.0), false), ((17.0, 10.0), true)] {
        let found = crate::test_support::with_decode_context(|ctx| {
            super::super::super::arrangement_region_containing_points(
                &sketch,
                &entities,
                &[Point2::new(point.0, point.1)],
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
        .expect("one face");
        let SketchProfileRegion::Trimmed { outer_boundary, .. } = found.region else {
            panic!("a trimmed region");
        };
        assert_eq!(
            outer_boundary.iter().any(|use_| use_.entity == right),
            on_right,
            "point {point:?}"
        );
    }
}

#[test]
fn an_ellipse_across_a_face_of_every_curve_reaches_each_path_as_an_assumption() {
    // A 10 x 10 square in a Sketch without profile loops, and a 6 x 3
    // ellipse centred on its right side.
    let id = SketchId::mint("synthetic:test:id#square-and-ellipse").unwrap();
    let square = rectangle(&id, "square", (0.0, 0.0), (10.0, 10.0));
    let mut entities = square.clone();
    entities.push(full_ellipse(&id, (10.0, 5.0)));
    let sketch = plain_sketch(id);
    let found = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_containing_points(
            &sketch,
            &entities,
            &[Point2::new(2.0, 2.0)],
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("the square");
    assert!(found.island_absence_assumed);
    let program = square
        .iter()
        .map(|entity| entity.id().clone())
        .collect::<Vec<_>>();
    let found = crate::test_support::with_decode_context(|ctx| {
        super::super::super::arrangement_region_matching_curve_program(
            &sketch,
            &entities,
            &program,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap()
    .expect("the square");
    assert!(found.island_absence_assumed);
}

#[test]
fn an_ellipse_in_a_crossing_loop_face_shapes_the_split() {
    // The four triangles of the x-braced window, one walk over all of them.
    let (sketch, window) = x_braced_window();
    let program = curve_ids(
        &window,
        &[
            "bottom", "fall", "rise", "right", "rise", "fall", "top", "fall", "rise", "left",
            "rise", "fall",
        ],
    );
    let split = |ellipse: SketchEntity| {
        let mut entities = window.clone();
        entities.push(ellipse);
        crate::test_support::with_decode_context(|ctx| {
            super::super::super::arrangement_regions_of_crossing_loop(
                &sketch,
                &entities,
                &program,
                PROFILE_PROGRAM_TOLERANCE,
                &ProfileGeometryWork::new(),
                ctx,
            )
        })
        .unwrap()
    };
    // Across the right side, from the right triangle: the split keeps the
    // triangle and assumes no island in it.
    let across = split(full_ellipse(&sketch.id, (10.0, 5.0))).expect("four triangles");
    assert_eq!(across.regions.len(), 4);
    assert!(across.island_absence_assumed);
    // Inside the bottom triangle: that face holds an island, so no set of
    // the other faces reaches the curve counts.
    assert!(split(tiny_ellipse(&sketch.id, (5.0, 2.0))).is_none());
}

#[test]
fn an_ellipse_lies_in_the_smallest_face_that_holds_it() {
    // A 40 x 30 face with a square island 5..10: an ellipse beside the
    // island lies in the outer region, which no ring names it in; an ellipse
    // in the island lies outside the outer region.
    let id = SketchId::mint("synthetic:test:id#ellipse-with-hole").unwrap();
    let outer = rectangle(&id, "outer", (0.0, 0.0), (40.0, 30.0));
    let island = rectangle(&id, "island", (5.0, 5.0), (10.0, 10.0));
    let sketch = sketch_with_loops(id.clone(), &[&outer, &island]);
    let rings = [ring(&outer, false), ring(&island, true)];
    let with_ellipse = |center: (f64, f64)| {
        let mut entities = outer.clone();
        entities.extend(island.iter().cloned());
        entities.push(tiny_ellipse(&id, center));
        entities
    };
    assert_eq!(
        member_regions(&sketch, &with_ellipse((30.0, 20.0)), &rings),
        None
    );
    let in_island = with_ellipse((7.5, 7.5));
    let (_, holes) = single_region(&sketch, &in_island, &rings);
    assert_eq!(holes.len(), 1);
    assert!(!selection(&sketch, &in_island, &rings).island_absence);
}

#[test]
fn an_island_takes_its_side_from_a_node_clear_of_the_face_boundary() {
    // A triangle with its first node 9e-8 from two sides of a 10 x 10 square,
    // within the linear tolerance of both, and 1.27e-7 from the corner.
    let id = SketchId::mint("synthetic:test:id#corner-sliver").unwrap();
    let square = rectangle(&id, "square", (0.0, 0.0), (10.0, 10.0));
    let near = (9.0e-8, 9.0e-8);
    let triangle = vec![
        line_entity(&id, "sliver-in", (1.0, 0.5), near),
        line_entity(&id, "sliver-out", near, (0.5, 1.0)),
        line_entity(&id, "sliver-back", (0.5, 1.0), (1.0, 0.5)),
    ];
    let mut entities = square.clone();
    entities.extend(triangle.iter().cloned());
    let sketch = sketch_with_loops(id, &[&square, &triangle]);
    assert_eq!(
        face_with(&all_faces(&sketch, &entities), &square[0]),
        Some(1)
    );
}

#[test]
fn a_ray_through_a_fillet_end_counts_the_boundary_once() {
    // An L-shaped face: the square (0, 0)-(20, 20) without its corner block
    // above v = 8 and right of u = 12, with a radius-2 fillet about (14, 10)
    // from (12, 10) at angle pi to (14, 8). The island's first node (6, 10)
    // lies level with the fillet end, where the evaluated arc end sits
    // 2.4e-16 above the side u = 12.
    let id = SketchId::mint("synthetic:test:id#fillet-ray").unwrap();
    let fillet = SketchEntity::new(
        SketchEntityId::mint("synthetic:test:id#fillet").unwrap(),
        id.clone(),
        SketchGeometry::try_from(SketchGeometryDefinition::Arc {
            center: Point2::new(14.0, 10.0),
            radius: Length::new(2.0).unwrap(),
            start_angle: Angle::new(std::f64::consts::PI).unwrap(),
            end_angle: Angle::new(1.5 * std::f64::consts::PI).unwrap(),
        })
        .unwrap(),
    );
    let mut outline = vec![
        line_entity(&id, "l-bottom", (0.0, 0.0), (20.0, 0.0)),
        line_entity(&id, "l-right", (20.0, 0.0), (20.0, 8.0)),
        line_entity(&id, "l-step", (20.0, 8.0), (14.0, 8.0)),
        line_entity(&id, "l-inner", (12.0, 10.0), (12.0, 20.0)),
        line_entity(&id, "l-top", (12.0, 20.0), (0.0, 20.0)),
        line_entity(&id, "l-left", (0.0, 20.0), (0.0, 0.0)),
    ];
    outline.push(fillet);
    let island = rectangle(&id, "l-island", (4.0, 10.0), (6.0, 12.0));
    let mut entities = outline.clone();
    entities.extend(island.iter().cloned());
    let sketch = sketch_with_loops(id, &[&outline, &island]);
    assert_eq!(
        face_with(&all_faces(&sketch, &entities), &outline[0]),
        Some(1)
    );
}
