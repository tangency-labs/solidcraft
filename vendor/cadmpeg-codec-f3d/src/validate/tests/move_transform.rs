// SPDX-License-Identifier: Apache-2.0

use crate::design::decode::scopes::direct_face::move_operation_links_hold;
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::direct_face::{DesignMoveForm, DesignMoveOperation};
use crate::records::feature::scope::{
    DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
};

fn fixture(class: &str) -> (DesignParameterScope, DesignRecordHeader) {
    let mut scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:scope#17",
        DesignFeatureKind::Move,
        17,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = crate::records::identity::ReferenceRun::unlocated(vec![71]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let DesignScopePayloadMut::Move(slot) = scope.payload_mut() else {
        panic!("Move payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
        transform_offset: 448,
        transform_record_index: 71,
        form: if class == "452" {
            DesignMoveForm::Form5
        } else {
            DesignMoveForm::Form1
        },
        form_offset: 443,
    });
    (
        scope,
        DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#400".into(),
            record_index: 71,
            class_tag: class.to_owned().try_into().unwrap(),
            byte_offset: 400,
        },
    )
}

#[test]
fn move_transform_validator_checks_each_added_class_and_locations() {
    for class in [
        "330", "273", "408", "372", "289", "394", "406", "282", "403", "391", "452",
    ] {
        let (mut scope, mut header) = fixture(class);
        assert!(move_operation_links_hold(&scope, Some(&header)));
        assert!(!move_operation_links_hold(&scope, None));
        let mut shifted = scope.clone();
        let DesignScopePayloadMut::Move(Some(operation)) = shifted.payload_mut() else {
            panic!("Move payload");
        };
        operation.form_offset += 1;
        assert!(!move_operation_links_hold(&shifted, Some(&header)));
        let mut unbound = scope.clone();
        let DesignScopePayloadMut::Move(Some(operation)) = unbound.payload_mut() else {
            panic!("Move payload");
        };
        operation.transform_record_index = 72;
        let mut unbound_header = header.clone();
        unbound_header.record_index = 72;
        assert!(!move_operation_links_hold(&unbound, Some(&unbound_header)));
        header.byte_offset += 1;
        assert!(!move_operation_links_hold(&scope, Some(&header)));
        header.byte_offset -= 1;
        header.record_index += 1;
        assert!(!move_operation_links_hold(&scope, Some(&header)));
        header.record_index -= 1;
        let DesignScopePayloadMut::Move(Some(operation)) = scope.payload_mut() else {
            panic!("Move payload");
        };
        operation.form = if class == "452" {
            DesignMoveForm::Form1
        } else {
            DesignMoveForm::Form5
        };
        assert!(!move_operation_links_hold(&scope, Some(&header)));
        header.class_tag = "999".to_owned().try_into().unwrap();
        assert!(!move_operation_links_hold(&scope, Some(&header)));
    }
}

fn move_face_scope(kind: DesignFeatureKind, form: DesignMoveForm) -> DesignParameterScope {
    let mut scope = DesignParameterScope::empty("f3d:Design/BulkStream.dat:scope#17", kind, 17);
    scope
        .try_edit(|draft| {
            draft.reference_members =
                crate::records::identity::ReferenceRun::unlocated(vec![50, 60, 71]);
            draft.layout_fixture_references();
            draft.layout_fixture_tail();
        })
        .unwrap();
    let (DesignScopePayloadMut::MoveFace(slot) | DesignScopePayloadMut::DeplacerFace(slot)) =
        scope.payload_mut()
    else {
        panic!("MoveFace payload");
    };
    *slot = Some(DesignMoveOperation {
        transform: crate::records::sketch_placement::SketchPlacementMatrix::IDENTITY,
        transform_offset: 448,
        transform_record_index: 71,
        form,
        form_offset: 443,
    });
    scope
}

#[test]
fn move_face_validator_ignores_the_transform_class_and_checks_its_links() {
    for kind in [DesignFeatureKind::MoveFace, DesignFeatureKind::DeplacerFace] {
        for form in [DesignMoveForm::Form1, DesignMoveForm::Form5] {
            let scope = move_face_scope(kind.clone(), form);
            for class in ["313", "396", "999"] {
                let (_, header) = fixture(class);
                assert!(move_operation_links_hold(&scope, Some(&header)));
            }
            let (_, mut header) = fixture("313");
            assert!(!move_operation_links_hold(&scope, None));
            header.byte_offset += 1;
            assert!(!move_operation_links_hold(&scope, Some(&header)));
            header.byte_offset -= 1;
            header.record_index += 1;
            assert!(!move_operation_links_hold(&scope, Some(&header)));
            header.record_index -= 1;
            let mut unbound = scope.clone();
            let (DesignScopePayloadMut::MoveFace(Some(operation))
            | DesignScopePayloadMut::DeplacerFace(Some(operation))) = unbound.payload_mut()
            else {
                panic!("MoveFace payload");
            };
            operation.transform_record_index = 72;
            header.record_index = 72;
            assert!(!move_operation_links_hold(&unbound, Some(&header)));
        }
    }
}

#[test]
fn move_transform_validator_reports_shifted_matrix_location() {
    crate::test_support::with_decode_context(|decode| {
        for class in ["330", "452"] {
            let (scope, header) = fixture(class);
            let scope_header = DesignRecordHeader {
                id: "f3d:Design/BulkStream.dat:design-record-header#0".into(),
                record_index: 17,
                class_tag: scope.class_tag.clone(),
                byte_offset: scope.byte_offset(),
            };
            let mut native = crate::native::F3dNative {
                design_parameter_scopes: vec![scope],
                design_record_headers: vec![scope_header, header],
                ..Default::default()
            };
            let ir = cadmpeg_ir::examples::unit_cube().unwrap();
            let validate = |native: &crate::native::F3dNative| {
                let ctx = super::super::Ctx::new(&ir, native, decode);
                let mut findings = Vec::new();
                super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
                findings
            };
            assert!(validate(&native).is_empty());
            let DesignScopePayloadMut::Move(Some(operation)) =
                native.design_parameter_scopes[0].payload_mut()
            else {
                panic!("Move payload");
            };
            operation.transform_offset += 1;
            assert!(validate(&native).iter().any(|finding| finding.message
                == "Fusion Design parameter scope has an invalid paired frame"));
        }
    });
}
