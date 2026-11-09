// SPDX-License-Identifier: Apache-2.0

use super::{
    carrier, identity_matrix, placed, BTreeMap, DesignComponentOccurrence,
    DesignComponentOccurrencePlacement, DesignParameterScope, Feature, FeatureDefinition,
    FeatureId, FeatureOperation, Occurrence, DOCUMENT_ROOT, STREAM,
};
use crate::records::feature::occurrence_transfer::{
    DesignOccurrenceTransfer, DesignOccurrenceTransferSource, DesignPasteOwner,
};

const SOURCE: &str = "00000000-0000-4000-8000-000000000253";
const COPY: &str = "00000000-0000-4000-8000-000000000345";

fn translated(x: f64) -> [[f64; 4]; 4] {
    let mut transform = identity_matrix();
    transform[0][3] = x;
    transform
}

fn copy_scope(target: [[f64; 4]; 4]) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#348"),
        crate::records::feature::scope::DesignFeatureKind::CopyPaste,
        348,
    );
    scope.occurrence_transfer = Some(DesignOccurrenceTransfer {
        relation_record_index: 346,
        occurrence_record_index: 345,
        component_record_index: 159,
        target_owner: DesignPasteOwner {
            collection: 27,
            context: 26,
            component: 3,
        },
        target_transform: target.try_into().unwrap(),
        target_transform_offset: Some(41288),
        source: DesignOccurrenceTransferSource::Copy {
            source_transform: target.try_into().unwrap(),
            source_transform_offset: 41132,
        },
    });
    scope
}

fn source() -> DesignComponentOccurrence {
    carrier(
        253,
        SOURCE,
        DesignComponentOccurrencePlacement::Base,
        DOCUMENT_ROOT,
    )
}

fn copy() -> DesignComponentOccurrence {
    carrier(345, COPY, placed(2, translated(6.0)), DOCUMENT_ROOT)
}

fn product_occurrence(carrier: &DesignComponentOccurrence) -> Occurrence {
    serde_json::from_value(serde_json::json!({
        "id": crate::ids::neutral_component_occurrence_id(&carrier.occurrence_guid),
        "prototype": {"scope": "local", "definition": "f3d:model:component#copied"},
        "parent": {"kind": "root"},
        "ordinal": 0,
        "transform": [[1.0, 0.0, 0.0, 60.0], [0.0, 1.0, 0.0, 0.0], [0.0, 0.0, 1.0, 0.0]],
        "scale": [1.0, 1.0, 1.0],
    }))
    .unwrap()
}

fn native() -> FeatureDefinition {
    FeatureDefinition::Operation(FeatureOperation::Native {
        kind: "CopyPaste".into(),
        parameters: BTreeMap::new(),
    })
}

fn project(
    scope: &DesignParameterScope,
    carriers: &[DesignComponentOccurrence],
    projected: &[Occurrence],
) -> FeatureDefinition {
    let mut feature = Feature {
        product_definition: None,
        id: FeatureId::mint("f3d:model:feature#copy").expect("identity grammar"),
        ordinal: 3,
        name: None,
        suppressed: None,
        dependencies: cadmpeg_ir::features::DistinctMembers::default(),
        source_properties: BTreeMap::default(),
        source_tag: None,
        source_text: None,
        source_content: cadmpeg_ir::features::FeatureContent::default(),
        evaluation: cadmpeg_ir::features::FeatureEvaluation::from_definition(native()),
        native_ref: Some(scope.id.clone()),
    };
    super::super::project_copy_paste_transfer_features(
        std::slice::from_mut(&mut feature),
        std::slice::from_ref(scope),
        carriers,
        projected,
    );
    feature.evaluation.definition().clone()
}

#[test]
fn a_copy_transfer_inserts_the_carrier_its_relation_names() {
    let projected = [product_occurrence(&copy())];
    assert_eq!(
        project(
            &copy_scope(translated(6.0)),
            &[source(), copy()],
            &projected
        ),
        FeatureDefinition::Operation(FeatureOperation::InsertComponent {
            occurrence: projected[0].id.clone(),
        })
    );
}

#[test]
fn a_copy_transfer_stays_native_without_a_source_of_its_definition_or_its_placement() {
    let moved = translated(6.0);
    let projected = [product_occurrence(&copy())];

    let mut new_definition = copy();
    new_definition.component_record_index = 160;
    assert_eq!(
        project(&copy_scope(moved), &[source(), new_definition], &projected),
        native()
    );
    assert_eq!(project(&copy_scope(moved), &[copy()], &projected), native());
    let later_source = carrier(
        400,
        SOURCE,
        DesignComponentOccurrencePlacement::Base,
        DOCUMENT_ROOT,
    );
    assert_eq!(
        project(&copy_scope(moved), &[copy(), later_source], &projected),
        native()
    );
    assert_eq!(
        project(
            &copy_scope(translated(5.0)),
            &[source(), copy()],
            &projected
        ),
        native()
    );
}

#[test]
fn a_copy_transfer_stays_native_for_a_repeated_unprojected_or_disagreeing_carrier() {
    let moved = translated(6.0);
    let projected = [product_occurrence(&copy())];

    let mut repeated = carrier(
        360,
        "00000000-0000-4000-8000-000000000360",
        placed(3, moved),
        DOCUMENT_ROOT,
    );
    repeated.record_index = 345;
    assert_eq!(
        project(
            &copy_scope(moved),
            &[source(), copy(), repeated],
            &projected
        ),
        native()
    );
    assert_eq!(
        project(&copy_scope(moved), &[source(), copy()], &[]),
        native()
    );
    let mut disagreeing = copy_scope(moved);
    if let Some(transfer) = disagreeing.occurrence_transfer.as_mut() {
        transfer.component_record_index = 160;
    }
    assert_eq!(
        project(&disagreeing, &[source(), copy()], &projected),
        native()
    );
}
