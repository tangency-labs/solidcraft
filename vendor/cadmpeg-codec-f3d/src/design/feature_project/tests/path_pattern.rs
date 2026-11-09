// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::parameters::parse_design_parameter_record;
use crate::design::feature_project::path_pattern::project_path_pattern;
use crate::design::test_support::parameter_record;
use crate::records::{
    feature::scope::{DesignFeatureKind, DesignParameterScope},
    identity::{Located, ReferenceRun},
    parameters::DesignParameter,
    topology::{
        construction::{
            DesignConstructionOperandGroup, DesignConstructionOperandGroupDraft,
            DesignConstructionOperandGroupFrame, DesignConstructionOperandGroupFrameDraft,
            DesignConstructionOperandRole,
        },
        extrude_selection::DesignOperandRole,
    },
};
use cadmpeg_ir::features::{
    patterns::{PatternSeed, PatternTransform},
    BodySelection, FaceSelection, FeatureDefinition, FeatureOperation, PathRef,
};

const STREAM: &str = "f3d:Design/BulkStream.dat";
const SCOPE: u32 = 315;
const SEED_GROUP: u32 = 323;
const PATH_GROUP: u32 = 333;

fn settings(
    total_extent: bool,
    flipped: bool,
    path_oriented: bool,
    start_parameter: f64,
) -> crate::records::feature::patterns::DesignPathPatternSettings {
    crate::records::feature::patterns::DesignPathPatternSettings {
        settings_record_index: 322,
        settings_byte_offset: 1_000,
        total_extent: Located {
            value: total_extent,
            offset: 0,
        },
        flipped: Located {
            value: flipped,
            offset: 0,
        },
        path_oriented: Located {
            value: path_oriented,
            offset: 0,
        },
        path_group_record_index: PATH_GROUP,
        start_record_index: 343,
        start_parameter: Located {
            value: cadmpeg_ir::scalar::FiniteReal::new(start_parameter).unwrap(),
            offset: 0,
        },
    }
}

fn scope() -> DesignParameterScope {
    scope_with(Some(settings(false, false, false, 0.0)))
}

fn scope_with(
    settings: Option<crate::records::feature::patterns::DesignPathPatternSettings>,
) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#38353"),
        DesignFeatureKind::PathPattern,
        SCOPE,
    );
    scope
        .try_edit(|draft| {
            let members = [319, SEED_GROUP, 326, 331, PATH_GROUP, 336, 341, 343, 346];
            let shift = 11 * (u64::try_from(members.len()).unwrap() - 1);
            draft.reference_members = ReferenceRun::located(
                members
                    .into_iter()
                    .zip((14..).step_by(11))
                    .map(|(value, offset)| Located { value, offset })
                    .collect(),
            );
            draft.kind_offset += shift;
            draft.feature_ordinal_offset += shift;
            draft.frame_length += shift;
            draft.paired_byte_offset += shift;
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::PathPattern(slot) =
        scope.payload_mut()
    {
        *slot = settings;
    }
    scope
}

fn group(
    record_index: u32,
    scope_reference_ordinal: u32,
    role: DesignOperandRole,
    members: &[u32],
) -> DesignConstructionOperandGroup {
    DesignConstructionOperandGroup::try_from(DesignConstructionOperandGroupDraft {
        id: format!("{STREAM}:design-construction-operand-group#{record_index}"),
        scope_record_index: SCOPE,
        scope_reference_ordinal,
        record_index,
        byte_offset: 0,
        class_tag: crate::records::references::DesignClassTag::try_from("310".to_owned()).unwrap(),
        members: members
            .iter()
            .zip((26..).step_by(11))
            .map(|(value, offset)| Located {
                value: *value,
                offset,
            })
            .collect(),
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
        operand_role: DesignConstructionOperandRole::Other(role),
        role_offset: 0,
        paired_class_tag: crate::records::references::DesignClassTag::try_from("259".to_owned())
            .unwrap(),
        paired_byte_offset: 0,
    })
    .unwrap()
}

fn parameter(source_kind: &str, unit: Option<&str>, value: f64) -> DesignParameter {
    parse_design_parameter_record(&parameter_record(
        Some(331),
        "4",
        source_kind,
        unit,
        "d3",
        value,
    ))
    .unwrap()
}

fn count(value: f64) -> DesignParameter {
    parameter("countU", None, value)
}

fn distance(value: f64) -> DesignParameter {
    parameter("PathPatternDelta", Some("mm"), value)
}

fn groups(seed_role: DesignOperandRole) -> Vec<DesignConstructionOperandGroup> {
    vec![
        group(SEED_GROUP, 1, seed_role, &[326]),
        group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
    ]
}

fn pattern(definition: FeatureDefinition) -> (Vec<PatternSeed>, PatternTransform) {
    let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) = definition
    else {
        panic!("path pattern definition");
    };
    (seeds, pattern.definition().clone())
}

#[test]
fn path_pattern_reads_seed_path_count_and_spacing() {
    let (count, distance) = (count(4.0), distance(2.5));
    let parameters = [(0, &count), (1, &distance)];
    for (role, seed) in [
        (
            DesignOperandRole::BODIES_B,
            PatternSeed::Bodies(BodySelection::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            ))),
        ),
        (
            DesignOperandRole::ROLE_0X10,
            PatternSeed::Faces(FaceSelection::Native(format!(
                "{STREAM}:design-construction-operand-group#{SEED_GROUP}"
            ))),
        ),
    ] {
        let projected =
            project_path_pattern(&scope(), &groups(role), &parameters).expect("path pattern");
        assert!(projected.losses.is_empty());
        let (seeds, transform) = pattern(projected.definition);
        assert_eq!(seeds, vec![seed]);
        let PatternTransform::CurveDriven {
            path,
            spacing,
            count,
        } = transform
        else {
            panic!("curve-driven transform");
        };
        assert_eq!(
            path,
            Some(PathRef::Native(format!(
                "{STREAM}:design-construction-operand-group#{PATH_GROUP}"
            )))
        );
        assert!((spacing.get() - 25.0).abs() <= 1.0e-12);
        assert_eq!(count, 4);
    }
}

#[test]
fn path_pattern_without_usable_parameters_keeps_an_unresolved_curve_driven_form() {
    let valid_count = count(4.0);
    let valid_distance = distance(2.5);
    let fractional = count(2.5);
    let zero = count(0.0);
    let measured_count = parameter("countU", Some("mm"), 4.0);
    let unitless_distance = parameter("PathPatternDelta", None, 2.5);
    let other_kind = parameter("Radius", Some("mm"), 2.5);
    for parameters in [
        vec![(1, &valid_distance)],
        vec![(0, &valid_count)],
        vec![(1, &valid_count), (0, &valid_distance)],
        vec![(0, &fractional), (1, &valid_distance)],
        vec![(0, &zero), (1, &valid_distance)],
        vec![(0, &measured_count), (1, &valid_distance)],
        vec![(0, &valid_count), (1, &unitless_distance)],
        vec![(0, &valid_count), (1, &other_kind)],
        vec![(0, &valid_count), (0, &valid_count), (1, &valid_distance)],
    ] {
        let projected =
            project_path_pattern(&scope(), &groups(DesignOperandRole::BODIES_B), &parameters)
                .expect("path pattern");
        assert!(projected.losses.is_empty());
        let (seeds, transform) = pattern(projected.definition);
        assert_eq!(seeds.len(), 1);
        assert!(matches!(
            transform,
            PatternTransform::Unresolved {
                form: Some(cadmpeg_ir::features::patterns::PatternForm::CurveDriven)
            }
        ));
    }
}

#[test]
fn path_pattern_rejects_groups_outside_its_form() {
    let (count, distance) = (count(4.0), distance(2.5));
    let parameters = [(0, &count), (1, &distance)];
    for groups in [
        vec![group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326])],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_A, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X10, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[331]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 2, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326, 331]),
            group(331, 3, DesignOperandRole::ROLE_0X5, &[336]),
        ],
        vec![
            group(SEED_GROUP, 1, DesignOperandRole::BODIES_B, &[326]),
            group(PATH_GROUP, 4, DesignOperandRole::ROLE_0X5, &[336]),
            group(343, 7, DesignOperandRole::ROLE_0X5, &[346]),
        ],
    ] {
        assert!(project_path_pattern(&scope(), &groups, &parameters).is_none());
    }
    let mut unclosed = scope();
    unclosed.unclosed_construction_operand_groups = vec![343];
    assert!(
        project_path_pattern(&unclosed, &groups(DesignOperandRole::BODIES_B), &parameters)
            .is_none()
    );
}

#[test]
fn path_pattern_settings_select_the_spacing_and_name_dropped_placement() {
    let (count, distance) = (count(4.0), distance(6.0));
    let parameters = [(0, &count), (1, &distance)];
    let spacing_of = |scope: DesignParameterScope| {
        let projected =
            project_path_pattern(&scope, &groups(DesignOperandRole::BODIES_B), &parameters)
                .expect("path pattern");
        let (_, transform) = pattern(projected.definition);
        match transform {
            PatternTransform::CurveDriven { spacing, .. } => Some(spacing.get()),
            _ => None,
        }
    };
    assert!(
        spacing_of(scope_with(Some(settings(false, false, false, 0.0))))
            .is_some_and(|spacing| (spacing - 60.0).abs() <= 1.0e-12)
    );
    assert!(
        spacing_of(scope_with(Some(settings(true, false, false, 0.0))))
            .is_some_and(|spacing| (spacing - 20.0).abs() <= 1.0e-12)
    );
    assert_eq!(spacing_of(scope_with(None)), None);
    let one = self::count(1.0);
    assert!(project_path_pattern(
        &scope_with(Some(settings(true, false, false, 0.0))),
        &groups(DesignOperandRole::BODIES_B),
        &[(0, &one), (1, &distance)],
    )
    .is_some_and(|projected| matches!(
        pattern(projected.definition).1,
        PatternTransform::Unresolved { .. }
    )));

    let projected = project_path_pattern(
        &scope_with(Some(settings(false, true, true, 0.5))),
        &groups(DesignOperandRole::BODIES_B),
        &parameters,
    )
    .expect("path pattern");
    let [loss] = projected.losses.as_slice() else {
        panic!("one placement loss");
    };
    assert_eq!(
        loss.code,
        crate::loss::F3dLossCode::FeaturePathPatternPlacementNotTransferred.kind()
    );
    for setting in [
        "flipped direction",
        "path-direction orientation",
        "path parameter 0.5",
    ] {
        assert!(loss.message.contains(setting), "{}", loss.message);
    }
}

#[test]
fn path_pattern_settings_naming_another_path_group_leave_the_form_unresolved() {
    let (count, distance) = (count(4.0), distance(2.5));
    let mut other_group = settings(false, false, false, 0.0);
    other_group.path_group_record_index = PATH_GROUP + 1;
    let projected = project_path_pattern(
        &scope_with(Some(other_group)),
        &groups(DesignOperandRole::BODIES_B),
        &[(0, &count), (1, &distance)],
    )
    .expect("path pattern");
    assert!(matches!(
        pattern(projected.definition).1,
        PatternTransform::Unresolved { .. }
    ));
}
