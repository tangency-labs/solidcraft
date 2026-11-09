// SPDX-License-Identifier: Apache-2.0

use crate::design::profile_select::{
    bind_path_pattern_sketch_selections, SketchCurveSelectionResolution,
};
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::Located;
use crate::records::topology::construction::{
    DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
    DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
    DesignConstructionOperandRole,
};
use crate::records::topology::extrude_selection::DesignOperandRole;
use cadmpeg_ir::features::patterns::{PatternKind, PatternSeed, PatternTransform};
use cadmpeg_ir::features::{
    BodySelection, Feature, FeatureDefinition, FeatureEvaluation, FeatureId, FeatureOperation,
    PathRef,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";

fn group(scope_record_index: u32, record_index: u32) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index,
        scope_reference_ordinal: 4,
        record_index,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned()).unwrap(),
        members: vec![Located {
            value: record_index + 3,
            offset: 26,
        }],
        lost_edge_references: Vec::new(),
        frame: DesignConstructionOperandGroupFrame::try_from(
            DesignConstructionOperandGroupFrameDraft {
                member_count_offset: 0,
                auxiliary_records: Vec::new(),
                auxiliary_paths: Vec::new(),
                trailing_records: Vec::new(),
                trailing_transforms: Vec::new(),
                trailing_dual_transforms: Vec::new(),
                trailing_flags: Vec::new(),
                opaque_index: 99,
                opaque_index_offset: 18,
                opaque_scalar: 0.0,
                opaque_scalar_offset: 22,
                variant: false,
                closing_flag: false,
            },
        )
        .unwrap(),
        operand_role: DesignConstructionOperandRole::Other(DesignOperandRole::ROLE_0X5),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn pattern_feature(pattern: PatternKind) -> Feature {
    Feature {
        product_definition: None,
        id: FeatureId::mint("synthetic:test:id#f3d:feature:path-pattern".to_owned()).unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: FeatureEvaluation::from_definition(FeatureDefinition::Operation(
            FeatureOperation::Pattern {
                seeds: vec![PatternSeed::Bodies(BodySelection::Native(
                    "seed".to_owned(),
                ))],
                pattern,
            },
        )),
        native_ref: None,
    }
}

fn curve_driven(group_record_index: u32) -> PatternKind {
    PatternKind::new(PatternTransform::CurveDriven {
        path: Some(PathRef::Native(format!(
            "{STREAM}:design-construction-operand-group#{group_record_index}"
        ))),
        spacing: cadmpeg_ir::scalar::PositiveLength::new(25.0).unwrap(),
        count: 4,
    })
    .unwrap()
}

fn bound(pattern: PatternKind, groups: &[DesignConstructionOperandGroup]) -> PatternKind {
    let scopes = [
        DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#315"),
            DesignFeatureKind::PathPattern,
            315,
        ),
        DesignParameterScope::empty(
            &format!("{STREAM}:design-parameter-scope#401"),
            DesignFeatureKind::CPattern,
            401,
        ),
    ];
    let mut features = vec![pattern_feature(pattern)];
    bind_path_pattern_sketch_selections(
        &mut features,
        &SketchCurveSelectionResolution {
            scopes: &scopes,
            groups,
            operands: &[],
            profile_curves: &[],
            placements: &[],
            curve_identities: &[],
            sketches: &[],
            sketch_entities: &[],
            spatial_sketches: &[],
            spatial_sketch_entities: &[],
        },
    );
    let FeatureDefinition::Operation(FeatureOperation::Pattern { pattern, .. }) =
        features[0].evaluation.definition()
    else {
        panic!("pattern feature");
    };
    pattern.clone()
}

#[test]
fn path_pattern_binding_keeps_a_path_it_cannot_resolve() {
    let path_group = group(315, 333);
    assert_eq!(
        bound(
            PatternKind::UNRESOLVED_CURVE_DRIVEN,
            std::slice::from_ref(&path_group)
        ),
        PatternKind::UNRESOLVED_CURVE_DRIVEN
    );
    assert_eq!(
        bound(curve_driven(333), std::slice::from_ref(&path_group)),
        curve_driven(333)
    );
    assert_eq!(
        bound(curve_driven(420), &[group(401, 420)]),
        curve_driven(420)
    );
    assert_eq!(
        bound(curve_driven(333), &[path_group.clone(), path_group]),
        curve_driven(333)
    );
}
