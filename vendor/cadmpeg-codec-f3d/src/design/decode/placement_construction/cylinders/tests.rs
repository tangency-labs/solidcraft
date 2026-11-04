// SPDX-License-Identifier: Apache-2.0

use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::placement_construction::DesignPlacementConstruction;

fn placement() -> DesignPlacementConstruction {
    let mut bytes = Vec::new();
    crate::test_support::indexed_header(&mut bytes, *b"317", 255);
    bytes.extend_from_slice(&[0; 10]);
    for value in [0.0_f64; 3] {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&[0; 10]);
    let rows = [
        [1.0_f64, 0.0, 0.0, 0.1],
        [0.0, 0.0, 1.0, 0.2],
        [0.0, -1.0, 0.0, 0.3],
        [0.0, 0.0, 0.0, 1.0],
    ];
    for value in rows.into_iter().flatten() {
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0.0_f64.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.push(0);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&[0; 2]);
    bytes.extend_from_slice(&2u32.to_le_bytes());
    bytes.extend_from_slice(&0u32.to_le_bytes());
    crate::test_support::with_decode_context(|ctx| {
        crate::design::decode::placement_construction::decode_frame(
            ctx,
            "f3d:Design/BulkStream.dat:design-placement-construction#0".into(),
            cadmpeg_core::decode::View::over_retained(&bytes),
            9,
        )
        .unwrap()
        .unwrap()
    })
}

fn scope() -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#1",
        DesignFeatureKind::CylinderPrimitive,
        1,
    );
    let primitive = serde_json::from_value(serde_json::json!({
        "height": 2.6, "height_record_index": 259,
        "height_offset": 1000, "diameter": 0.5, "diameter_record_index": 262,
        "diameter_offset": 1100, "operation": "join", "operation_offset": 20,
    }))
    .unwrap();
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![251, 255, 259, 262, 264]);
            draft.payload =
                crate::records::feature::primitives::DesignSolidPrimitive::Cylinder(primitive)
                    .into();
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.paired_byte_offset.max(draft.kind_offset + 96);
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.layout_fixture_tail();
        })
        .unwrap();
    scope
}

#[test]
fn named_cylinder_binds_decoded_version_nine_frame() {
    let placement = placement();
    let expected = placement.fields.matrix.unwrap();
    let mut scope = scope();
    assert!(super::bind(std::slice::from_mut(&mut scope), &[placement]).is_empty());
    let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
        scope.payload()
    else {
        panic!("expected cylinder");
    };
    assert_eq!(cylinder.transform, Some(expected));
    assert_eq!(
        expected.value.rows(),
        [
            [1.0, 0.0, 0.0, 0.1],
            [0.0, 0.0, 1.0, 0.2],
            [0.0, -1.0, 0.0, 0.3],
            [0.0, 0.0, 0.0, 1.0],
        ]
    );
    let restored: DesignParameterScope =
        serde_json::from_value(serde_json::to_value(&scope).unwrap()).unwrap();
    assert_eq!(restored, scope);
    assert!(super::bind(std::slice::from_mut(&mut scope), &[]).is_empty());
    assert_eq!(scope, restored);
}

#[test]
fn named_cylinder_reports_unbound_placement() {
    let placement = placement();
    let mut foreign = placement.clone();
    foreign.id = "f3d:Other/BulkStream.dat:design-placement-construction#0".into();
    let mut flipped = placement.clone();
    flipped.fields.axis_flipped.as_mut().unwrap().value = 1;
    let mut other_version = placement.clone();
    other_version.type_version = 8;
    for placements in [
        vec![],
        vec![foreign],
        vec![placement.clone(), placement.clone()],
        vec![flipped],
        vec![other_version],
    ] {
        let mut scope = scope();
        let losses = super::bind(std::slice::from_mut(&mut scope), &placements);
        assert_eq!(losses.len(), 1);
        assert_eq!(
            losses[0].code,
            crate::loss::F3dLossCode::CylinderPlacementUnbound
                .note("")
                .code
        );
        let crate::records::feature::scope::DesignScopePayload::CylinderPrimitive(Some(cylinder)) =
            scope.payload()
        else {
            panic!("expected cylinder");
        };
        assert!(cylinder.transform.is_none());
    }
}
