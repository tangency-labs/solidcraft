// SPDX-License-Identifier: Apache-2.0

use super::*;

const PROFILE_PROGRAM_TOLERANCE: f64 = 1.0e-7;

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
    .unwrap();
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
    .unwrap();
    assert_eq!(region, None);
}

#[test]
fn counted_partial_curve_members_refuse_two_regions_with_the_same_multiset() {
    let (sketch, entities) = divided_circle(false);
    let members = [entities[1].id().clone(), entities[0].id().clone()];
    let region = crate::test_support::with_decode_context(|ctx| {
        super::super::arrangement_region_matching_curve_members(
            &sketch,
            &entities,
            &members,
            PROFILE_PROGRAM_TOLERANCE,
            &ProfileGeometryWork::new(),
            ctx,
        )
    })
    .unwrap();
    assert_eq!(region, None);
}
