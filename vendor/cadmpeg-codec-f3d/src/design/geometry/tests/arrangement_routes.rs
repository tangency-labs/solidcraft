// SPDX-License-Identifier: Apache-2.0

use super::*;

const ARRANGEMENT_FACE_TEST_TOLERANCE: f64 = 1.0e-7;

#[test]
fn arrangement_admitted_route_keeps_two_faces() {
    let (sketch, entities, _, _) = super::coincident_circle_arc_arrangement();
    let arena = DecodeArena::new();
    let policy = DecodePolicy::default();
    let (ctx, _) = DecodeContext::from_root_bytes(&[], &arena, &policy).unwrap();
    let work = super::local_profile_work();
    let faces = super::sketch_arrangement_faces(
        &sketch,
        &entities,
        ARRANGEMENT_FACE_TEST_TOLERANCE,
        &work,
        &ctx,
    )
    .unwrap()
    .unwrap();
    assert_eq!(faces.len(), 2);
}

#[test]
fn every_curve_arrangement_joins_collinear_runs_and_t_junctions() {
    // A 10 x 4 rectangle whose top runs as two collinear lines, split by a
    // divider that ends on the one-piece bottom line, with a profile table
    // that knows only the outline.
    let sketch_id = SketchId::mint("synthetic:test:id#collinear-plate").unwrap();
    let line = |id: &str, start: (f64, f64), end: (f64, f64)| {
        SketchEntity::new(
            SketchEntityId::mint(id).unwrap(),
            sketch_id.clone(),
            SketchGeometry::try_from(SketchGeometryDefinition::Line {
                start: Point2::new(start.0, start.1),
                end: Point2::new(end.0, end.1),
            })
            .unwrap(),
        )
    };
    let entities = vec![
        line("synthetic:test:id#bottom", (0.0, 0.0), (10.0, 0.0)),
        line("synthetic:test:id#right", (10.0, 0.0), (10.0, 4.0)),
        line("synthetic:test:id#top-right", (10.0, 4.0), (5.0, 4.0)),
        line("synthetic:test:id#top-left", (5.0, 4.0), (0.0, 4.0)),
        line("synthetic:test:id#left", (0.0, 4.0), (0.0, 0.0)),
        line("synthetic:test:id#divider", (5.0, 4.0), (5.0, 0.0)),
    ];
    let outline = ["bottom", "right", "top-right", "top-left", "left"]
        .iter()
        .map(|name| SketchEntityUse {
            entity: SketchEntityId::mint(format!("synthetic:test:id#{name}")).unwrap(),
            reversed: false,
        })
        .collect::<Vec<_>>();
    let sketch = Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: cadmpeg_ir::sketches::SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(vec![outline]).unwrap(),
        native_ref: None,
    };
    let work = super::local_profile_work();
    let regions = crate::test_support::with_decode_context(|ctx| {
        super::super::atomic_sketch_regions(&sketch, &entities, 1.0e-6, &work, ctx)
    })
    .unwrap()
    .expect("the arrangement of every curve");
    let mut areas: Vec<f64> = regions
        .iter()
        .map(|region| {
            cadmpeg_ir::math::planar::polygon_area_twice(&region.polyline)
                .unwrap()
                .get()
                .abs()
                / 2.0
        })
        .collect();
    areas.sort_by(f64::total_cmp);
    assert_eq!(areas.len(), 2, "{areas:?}");
    assert!(
        areas.iter().all(|area| (area - 20.0).abs() < 1e-9),
        "{areas:?}"
    );
}
