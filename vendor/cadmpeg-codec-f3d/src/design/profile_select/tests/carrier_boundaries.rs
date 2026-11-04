// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::math::{Point2, Point3, Vector3};
use cadmpeg_ir::sketches::{
    Sketch, SketchEntity, SketchEntityId, SketchEntityUse, SketchGeometry,
    SketchGeometryDefinition, SketchId,
};
const EPS_TRANSITION_PROFILE: f64 = 1.0e-6;

#[test]
fn transition_profile_prefers_consistent_side_loops_and_combines_cap_boundaries() {
    use cadmpeg_ir::features::SketchProfileRegion;

    let sketch_id = SketchId::mint("synthetic:test:id#sketch").unwrap();
    let mut profiles = Vec::new();
    let mut entities = Vec::new();
    for (profile_index, corners) in [
        [[0.0, 0.0], [4.0, 0.0], [4.0, 4.0], [0.0, 4.0]],
        [[6.0, 0.0], [8.0, 0.0], [8.0, 2.0], [6.0, 2.0]],
        [[1.0, 1.0], [2.0, 1.0], [2.0, 2.0], [1.0, 2.0]],
        [[3.0, 1.0], [5.0, 1.0], [5.0, 3.0], [3.0, 3.0]],
    ]
    .into_iter()
    .enumerate()
    {
        let mut profile = Vec::new();
        for edge_index in 0..corners.len() {
            let id = SketchEntityId::mint(format!(
                "synthetic:test:id#profile-{profile_index}-edge-{edge_index}"
            ))
            .unwrap();
            profile.push(SketchEntityUse {
                entity: id.clone(),
                reversed: false,
            });
            let [start_u, start_v] = corners[edge_index];
            let [end_u, end_v] = corners[(edge_index + 1) % corners.len()];
            entities.push(SketchEntity::new(
                id,
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Line {
                    start: Point2::new(start_u, start_v),
                    end: Point2::new(end_u, end_v),
                })
                .unwrap(),
            ));
        }
        profiles.push(profile);
    }
    let sketch = Sketch {
        id: sketch_id,
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
    };
    let transition_selection =
        |selections: Vec<Option<crate::design::profile_select::ResolvedProfileSelection>>| {
            crate::test_support::with_decode_context(|decode_ctx| {
                crate::design::profile_select::transition_inserted_profile_selection(
                    &sketch,
                    &entities,
                    EPS_TRANSITION_PROFILE,
                    selections,
                    &crate::design::geometry::ProfileGeometryWork::new(),
                    decode_ctx,
                )
            })
            .unwrap()
        };

    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), Some(3), Some(3)]),
        Some(3)
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), None, Some(3)]),
        Some(3)
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([Some(3), Some(4)]),
        None
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection(std::iter::empty::<Option<u32>>()),
        None
    );
    assert_eq!(
        crate::design::profile_select::unique_resolved_selection([None::<u32>, None]),
        None
    );
    let region = crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
        SketchProfileRegion::loops(0, vec![1]).unwrap(),
    ]);
    assert_eq!(
        transition_selection(vec![
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1])),
        ]),
        Some(region.clone())
    );
    assert_eq!(
        transition_selection(vec![
            Some(region.clone()),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2]))
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Regions(Vec::new())),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1]))
    );
    assert_eq!(
        transition_selection(vec![Some(region)]),
        Some(
            crate::design::profile_select::ResolvedProfileSelection::Regions(vec![
                SketchProfileRegion::loops(0, vec![1],).unwrap(),
            ])
        )
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![1])),
            None,
        ]),
        Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0, 1]))
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![2])),
        ]),
        None
    );
    assert_eq!(
        transition_selection(vec![
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![0])),
            Some(crate::design::profile_select::ResolvedProfileSelection::Loops(vec![3])),
        ]),
        None
    );
    assert_eq!(transition_selection(vec![None]), None);
}
