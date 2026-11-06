// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::records::topology::entity_selection::{
    DesignProfileCurveEntry, DesignProfileCurveLoop, DesignProfileCurveSelection,
};

fn selection(
    record_index: u32,
    ordinal: u32,
    primary: u64,
    outer: bool,
) -> DesignProfileCurveSelection {
    let identity_record_offset = 500;
    let mut selection = DesignProfileCurveSelection {
        id: format!("stream:profile-{record_index}"),
        scope_record_index: 7,
        group_record_index: 9,
        group_member_ordinal: ordinal,
        record_index,
        byte_offset: 100,
        owner_text: "42".into(),
        owner_text_offset: 200,
        sketch_scope_record_index: 7,
        sketch_scope_offset: 300,
        identity_record_offset,
        identity_end_offset: 629,
        loop_lists: [
            vec![vec![DesignProfileCurveLoop {
                outer,
                entries: vec![DesignProfileCurveEntry {
                    primary_id: primary,
                    primary_id_offset: 588,
                    secondary_id: 0,
                    entry_fields: [1, 1, 1],
                }],
            }]],
            vec![],
        ],
    };
    selection.identity_end_offset = selection.lists_end().unwrap_or(0);
    selection
}

#[test]
fn typed_profile_regions_preserve_holes_and_refuse_ambiguous_loops() {
    let mut group = group();
    group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    let placements = [placement()];
    let sketch_id = neutral_sketch_id(&placements[0]);
    let curves = [curve(31, 11, 0), curve(32, 12, 0), curve(33, 13, 0)];
    let entities: Vec<_> = [(11, 9.0), (12, 2.0), (13, 1.0)]
        .into_iter()
        .map(|(id, radius)| {
            SketchEntity::new(
                neutral_sketch_curve_id(&sketch_id, id, 0),
                sketch_id.clone(),
                SketchGeometry::try_from(SketchGeometryDefinition::Circle {
                    center: Point2::new(0.0, 0.0),
                    radius: Length::new(radius).unwrap(),
                })
                .unwrap(),
            )
        })
        .collect();
    let mut sketches = [Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::try_resolved(
            Point3::new(0.0, 0.0, 0.0),
            Vector3::new(0.0, 0.0, 1.0),
            Vector3::new(1.0, 0.0, 0.0),
        )
        .unwrap(),
        profiles: cadmpeg_ir::sketches::SketchProfiles::try_from(
            entities
                .iter()
                .map(|entity| {
                    vec![SketchEntityUse {
                        entity: entity.id().clone(),
                        reversed: false,
                    }]
                })
                .collect::<Vec<_>>(),
        )
        .unwrap(),
        native_ref: None,
    }];
    let mut first = selection(10, 0, 11, true);
    let mut hole = selection(10, 0, 12, true).loop_lists[0][0][0].clone();
    hole.outer = false;
    first.loop_lists[0][0].push(hole);
    first.loop_lists[0][0][1].entries[0].primary_id_offset = 633;
    first.identity_end_offset = first.lists_end().unwrap();
    let second = selection(11, 1, 13, true);
    let mut selections = [first, second];
    let resolve = |sketches: &[Sketch], selections: &[DesignProfileCurveSelection]| {
        let mut resolution =
            planar_resolution(&[], &placements, &curves, sketches, &entities).path_resolution();
        resolution.profile_curves = selections;
        resolve_entity_selection_profile(&group, &resolution)
    };
    assert_eq!(
        resolve(&sketches, &selections),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch_id.clone(),
                vec![
                    SketchProfileRegion::loops(0, vec![1]).unwrap(),
                    SketchProfileRegion::loops(2, vec![]).unwrap()
                ]
            )
            .unwrap()
        ))
    );
    let mut path_resolution =
        planar_resolution(&[], &placements, &curves, &sketches, &entities).path_resolution();
    path_resolution.profile_curves = &selections;
    assert!(super::super::resolve_profile_curve_path(&group, "stream", &path_resolution).is_none());
    let mut repeated = selections.clone();
    repeated[1].loop_lists = repeated[0].loop_lists.clone();
    repeated[1].identity_end_offset = repeated[0].identity_end_offset;
    assert_eq!(
        resolve(&sketches, &repeated),
        Some(cadmpeg_ir::features::ProfileRef::Planar(
            cadmpeg_ir::features::PlanarProfileRef::sketch_regions(
                sketch_id.clone(),
                vec![SketchProfileRegion::loops(0, vec![1]).unwrap()],
            )
            .unwrap()
        ))
    );
    selections[0].loop_lists[0][0][1].entries[0].primary_id = 11;
    assert!(resolve(&sketches, &selections).is_none());
    selections[0].loop_lists[0][0][1].entries[0].primary_id = 12;
    let mut profiles = sketches[0].profiles.iter().cloned().collect::<Vec<_>>();
    profiles.push(profiles[0].clone());
    sketches[0].profiles = cadmpeg_ir::sketches::SketchProfiles::try_from(profiles).unwrap();
    assert!(resolve(&sketches, &selections).is_none());
}
