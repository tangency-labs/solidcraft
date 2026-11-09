// SPDX-License-Identifier: Apache-2.0

use super::super::{bind_loft_and_revolve_sketch_selections, SketchProfileResolution};
use super::group;
use crate::ids::neutral_sketch_id;
use crate::records::sketch_placement::DesignSketchPlacement;
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::{
    BooleanOp, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    LoftGuidance, LoftSection, PlanarProfileRef, ProfileRef,
};
use cadmpeg_ir::sketches::{Sketch, SketchPlacement};

const STREAM: &str = "f3d:FusionAssetName[Active]/Design1/BulkStream.dat";

fn loft_binding(
    flags: [u8; 2],
    tail_length: usize,
) -> (
    Vec<cadmpeg_ir::report::loss::LossNote>,
    crate::records::topology::construction::DesignConstructionOperandGroup,
    Feature,
    cadmpeg_ir::sketches::SketchId,
) {
    let (archive, _, headers, entities) =
        crate::test_support::multiple_profiles::fixture_with_flags(1, Some((flags, tail_length)));
    let mut profile_group = group();
    profile_group.id = format!("{STREAM}:construction-group#9");
    profile_group.operand_role =
        crate::records::topology::construction::DesignConstructionOperandRole::Other(
            DesignOperandRole::PROFILE,
        );
    profile_group
        .try_set_members(vec![crate::records::identity::Located {
            value: 100,
            offset: 0,
        }])
        .unwrap();
    let placement = DesignSketchPlacement {
        frame: crate::records::sketch_placement::DesignSketchFrame::new(
            0,
            crate::records::sketch_placement::DesignSketchFrameForm::ScopeCompact,
        )
        .unwrap(),
        id: format!("{STREAM}:placement#20"),
        scope_record_index: Some(7),
        entity_id: crate::records::identity::DesignEntityId::try_from("0_172".to_owned()).unwrap(),
        visibility: None,
        class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned()).unwrap(),
        record_index: 20,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("277".to_owned())
            .unwrap(),
    };
    let sketch_id = neutral_sketch_id(&placement);
    let sketches = [Sketch {
        id: sketch_id.clone(),
        name: None,
        configuration: None,
        visible: None,
        placement: SketchPlacement::Unresolved {},
        profiles: cadmpeg_ir::sketches::SketchProfiles::default(),
        native_ref: None,
    }];
    let placements = [placement];
    let resolution = SketchProfileResolution {
        entities: &entities,
        entity_selection_operands: &[],
        placements: &placements,
        curve_identities: &[],
        sketches: &sketches,
        sketch_entities: &[],
        spatial_sketches: &[],
        spatial_sketch_entities: &[],
        linear_tolerance: 1.0e-7,
        angular_tolerance: 1.0e-9,
    };
    let mut features = vec![Feature {
        product_definition: None,
        id: FeatureId::mint("test:model:feature#loft").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Loft {
                sections: vec![LoftSection::Profile(ProfileRef::Planar(
                    PlanarProfileRef::Native(profile_group.id.clone()),
                ))],
                guidance: LoftGuidance::default(),
                op: BooleanOp::NewBody,
                closed: false,
                solid: true,
                ruled: false,
                linearize: false,
                max_degree: None,
                allow_multi_profile_faces: None,
            },
        )),
        native_ref: None,
    }];
    let losses = crate::test_support::zip_test::with_scan(&archive, |scan| {
        bind_loft_and_revolve_sketch_selections(
            scan,
            std::slice::from_mut(&mut profile_group),
            &headers,
            &resolution,
            &mut features,
        )
        .unwrap()
    });
    let [feature] = <[Feature; 1]>::try_from(features).unwrap();
    (losses, profile_group, feature, sketch_id)
}

fn loft_section(feature: &Feature) -> &LoftSection {
    let FeatureDefinition::Operation(FeatureOperation::Loft { sections, .. }) =
        feature.evaluation.definition()
    else {
        panic!("loft definition");
    };
    &sections[0]
}

#[test]
fn loft_compact_profile_flags_are_retained_and_report_the_assumption() {
    for tail_length in [89, 93] {
        let (losses, profile_group, feature, sketch_id) = loft_binding([0, 1], tail_length);
        assert_eq!(
            loft_section(&feature),
            &LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)))
        );
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code.local_code(),
            "feature.sketch-profile-flags-assumed"
        );
        let profile = profile_group.sketch_profile.unwrap();
        assert_eq!(profile.record_index, 100);
        assert_eq!(profile.compact_tail_flags, Some([false, true]));
    }
}

#[test]
fn loft_compact_profile_zero_flags_bind_without_an_assumption_loss() {
    let (losses, profile_group, feature, sketch_id) = loft_binding([0, 0], 93);
    assert_eq!(
        loft_section(&feature),
        &LoftSection::Profile(ProfileRef::Planar(PlanarProfileRef::Sketch(sketch_id)))
    );
    assert!(losses.is_empty());
    assert_eq!(
        profile_group.sketch_profile.unwrap().compact_tail_flags,
        Some([false, false])
    );
}
