// SPDX-License-Identifier: Apache-2.0

use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation};
use cadmpeg_ir::ids::OccurrenceId;
use cadmpeg_ir::products::{AssemblyJoint, JointConnector, JointId, JointOperand, PairedJointKind};
use cadmpeg_ir::transform::Transform;

#[test]
fn an_omitted_nested_placement_keeps_an_unbound_joint_native_and_its_feature_valid() {
    let id = OccurrenceId::mint("f3d:model:occurrence#member").unwrap();
    let occurrence = serde_json::from_value(serde_json::json!({
        "id": id, "prototype": {"scope":"unresolved"}, "parent": {"kind":"root"},
        "ordinal":0, "transform":Transform::identity(), "scale":[1.0,1.0,1.0]
    }))
    .unwrap();
    let joint = |key: &str, residual: Vec<String>| {
        let connector = |operand| JointConnector {
            operand,
            frame: Transform::identity(),
            detached: false,
        };
        AssemblyJoint::paired(
            JointId::mint(format!("f3d:model:joint#{key}")).unwrap(),
            PairedJointKind::Fixed {
                angle: None,
                translation_offset: None,
                angular_limits: None,
                linear_limits: None,
            },
            [
                connector(JointOperand::occurrence(id.clone(), "member", residual)),
                connector(JointOperand::root("root", Vec::new())),
            ],
            None,
        )
    };
    let omitted = joint("omitted", vec!["omitted-child-key".to_owned()]);
    let bound = joint("bound", Vec::new());
    let feature = cadmpeg_ir::features::Feature {
        product_definition: None,
        id: cadmpeg_ir::features::FeatureId::mint("f3d:model:feature#joint").unwrap(),
        ordinal: 0,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: std::collections::BTreeMap::new(),
        source_tag: Some("Assemble".to_owned()),
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(
            FeatureDefinition::Operation(FeatureOperation::AssemblyJoint {
                joint: omitted.id.clone(),
            }),
        ),
        native_ref: None,
    };
    let mut model = cadmpeg_ir::document::Model::default();
    model.occurrences.push(occurrence);
    model.features.push(feature);
    let mut external = joint("external", Vec::new());
    external.connectors_mut().next().unwrap().operand = JointOperand::external(
        cadmpeg_ir::products::ExternalDocument::document_id("unbound-document"),
        "external-member",
        Vec::new(),
    );
    model.assembly_joints = vec![omitted, bound.clone(), external];
    let mut unchanged = model.clone();
    let mut losses = Vec::new();
    super::super::omit_unbound_member_joints(
        &mut unchanged,
        &std::collections::BTreeSet::new(),
        &mut losses,
    );
    assert_eq!(unchanged.assembly_joints, model.assembly_joints);
    assert!(losses.is_empty());
    let mut projected = model.clone();
    let members = std::collections::BTreeSet::from([id.clone()]);
    let mut losses = Vec::new();
    super::super::omit_unbound_member_joints(&mut projected, &members, &mut losses);
    assert_eq!(
        projected.assembly_joints.as_slice(),
        std::slice::from_ref(&bound)
    );
    assert!(matches!(
        projected.features[0].evaluation.definition(),
        FeatureDefinition::Operation(FeatureOperation::Native { .. })
    ));
    assert_eq!(losses.len(), 2);
    assert!(losses
        .iter()
        .all(|loss| loss.code == crate::loss::F3dLossCode::AssemblyJointUnprojected.kind()));
    let mut ir = cadmpeg_ir::CadIr::empty();
    ir.model = projected;
    let validation = cadmpeg_ir::validate_neutral(&ir, losses).unwrap();
    assert!(validation.is_ok(), "{validation:#?}");
}

#[test]
fn a_path_inside_a_placed_member_survives_an_omitted_sibling_reference() {
    let placed = OccurrenceId::mint("f3d:model:occurrence#placed").unwrap();
    let occurrence = serde_json::from_value(serde_json::json!({
        "id": placed, "prototype": {"scope":"unresolved"}, "parent": {"kind":"root"},
        "ordinal":0, "transform":Transform::identity(), "scale":[1.0,1.0,1.0]
    }))
    .unwrap();
    let connector = |operand| JointConnector {
        operand,
        frame: Transform::identity(),
        detached: false,
    };
    let joint = AssemblyJoint::paired(
        JointId::mint("f3d:model:joint#inside").unwrap(),
        PairedJointKind::Fixed {
            angle: None,
            translation_offset: None,
            angular_limits: None,
            linear_limits: None,
        },
        [
            connector(JointOperand::occurrence(
                placed.clone(),
                "placed",
                vec!["inner-key".to_owned()],
            )),
            connector(JointOperand::root("root", Vec::new())),
        ],
        None,
    );
    let mut model = cadmpeg_ir::document::Model::default();
    model.occurrences.push(occurrence);
    model.assembly_joints.push(joint.clone());
    let other_member = OccurrenceId::mint("f3d:model:occurrence#other-member").unwrap();
    let mut losses = Vec::new();
    super::super::omit_unbound_member_joints(
        &mut model,
        &std::collections::BTreeSet::from([other_member]),
        &mut losses,
    );
    assert_eq!(model.assembly_joints, [joint]);
    assert!(losses.is_empty());
}
