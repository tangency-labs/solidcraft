// SPDX-License-Identifier: Apache-2.0
//! The source body of a `CopyPasteBodies` scope.

use crate::records::bodies::{DesignBodyBinding, DesignBodyMember};
use crate::records::decal::DesignRecordHeader;
use crate::records::feature::body_ops::DesignCopyPasteBodiesOperation;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};
use crate::records::identity::ReferenceRun;

const INVALID: &str = "Fusion Design parameter scope has an invalid paired frame";
const STREAM: &str = "f3d:Design/BulkStream.dat";

fn binding(entity_suffix: u32, ordinal: u32) -> DesignBodyBinding {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:design-body-binding#{}", 1000 + 100 * ordinal),
        "stream": "Design/BulkStream.dat",
        "pair_count": 4,
        "pair_ordinal": ordinal,
        "asm_body_key": ordinal,
        "asm_body_key_offset": 1000 + 100 * ordinal,
        "entity_suffix": entity_suffix,
        "entity_suffix_offset": 1008 + 100 * ordinal,
        "blob_name": "BREP.synthetic.smb",
        "blob_name_offset": 1050 + 100 * ordinal,
        "body": format!("f3d:brep/synthetic.smb/brep:entity#{ordinal}"),
    }))
    .unwrap()
}

fn member(entity_suffix: u32) -> DesignBodyMember {
    serde_json::from_value(serde_json::json!({
        "id": format!("{STREAM}:design-body-member#{}", 5000 + entity_suffix),
        "byte_offset": 5000 + entity_suffix,
        "entity_suffix": entity_suffix,
        "flags": 0,
    }))
    .unwrap()
}

/// A scope that copies the bodies with suffixes 11 and 13 to 12 and 14.
fn native() -> crate::native::F3dNative {
    let operation: DesignCopyPasteBodiesOperation = serde_json::from_value(serde_json::json!({
        "body_group_record_index": 501, "body_group_class_tag": "264", "body_group_byte_offset": 100,
        "body_operand_record_indices": [502, 504], "body_operand_record_offsets": [126, 137],
        "relation_record_index": 503, "relation_class_tag": "264", "relation_byte_offset": 200,
        "source_body_entity_suffixes": [11, 13], "source_body_entity_suffix_offsets": [225, 255],
        "copied_body_entity_suffixes": [12, 14], "copied_body_entity_suffix_offsets": [240, 270]
    }))
    .unwrap();
    let mut scope = DesignParameterScope::empty(
        &format!("{STREAM}:design-parameter-scope#17"),
        DesignFeatureKind::CopyPasteBodies,
        17,
    );
    scope
        .try_edit(|draft| {
            draft.reference_members = ReferenceRun::unlocated(vec![501, 502, 504]);
            draft.layout_fixture_references();
            draft.paired_byte_offset = draft.kind_offset + 22 + 110;
            draft.frame_length = draft.paired_byte_offset - draft.byte_offset;
            draft.feature_ordinal_offset = draft.kind_offset + 22;
        })
        .unwrap();
    if let crate::records::feature::scope::DesignScopePayloadMut::CopyPasteBodies(slot) =
        scope.payload_mut()
    {
        *slot = Some(operation);
    }
    let mut native = crate::native::F3dNative::default();
    for (record_index, byte_offset) in [
        (17, scope.byte_offset()),
        (501, 100),
        (502, 126),
        (503, 200),
        (504, 137),
    ] {
        native.design_record_headers.push(DesignRecordHeader {
            id: format!("{STREAM}:design-record-header#{record_index}"),
            record_index,
            class_tag: "264".to_owned().try_into().unwrap(),
            byte_offset,
        });
    }
    native.design_parameter_scopes.push(scope);
    native
        .design_body_bindings
        .extend([binding(12, 0), binding(14, 1)]);
    native
}

fn invalid(native: &crate::native::F3dNative) -> bool {
    crate::test_support::with_decode_context(|service_ctx| {
        let ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let ctx = super::super::Ctx::new(&ir, native, service_ctx);
        let mut findings = Vec::new();
        super::super::validate_parameter_scopes(&ctx, &mut findings).unwrap();
        findings.iter().any(|finding| finding.message == INVALID)
    })
}

#[test]
fn source_body_is_a_bodies_root_member_or_a_bound_body() {
    let mut native = native();
    // No source body is bound or listed.
    assert!(invalid(&native));
    // Listed sources need no binding of their own.
    native.design_body_members.extend([member(11), member(13)]);
    assert!(!invalid(&native));
    // A bound source also qualifies.
    let mut bound = native.clone();
    bound.design_body_members.clear();
    bound
        .design_body_bindings
        .extend([binding(11, 2), binding(13, 3)]);
    assert!(!invalid(&bound));
    // One unlisted source is refused.
    let mut partial = native.clone();
    partial.design_body_members.pop();
    assert!(invalid(&partial));
    // A listed copied body without a binding is refused.
    let mut unbound_copy = native.clone();
    unbound_copy.design_body_bindings.pop();
    assert!(invalid(&unbound_copy));
}

#[test]
fn copied_body_can_be_history_only_but_not_unsolved() {
    let unsolved = |ordinal: u32, history_only: bool| {
        let mut value = serde_json::to_value(binding(14, ordinal)).unwrap();
        let object = value.as_object_mut().unwrap();
        object.remove("body");
        object.insert("history_only".to_owned(), history_only.into());
        serde_json::from_value::<DesignBodyBinding>(value).unwrap()
    };
    let mut native = native();
    native.design_body_members.extend([member(11), member(13)]);
    native.design_body_bindings.pop();
    let mut consumed = native.clone();
    consumed.design_body_bindings.push(unsolved(1, true));
    assert!(!invalid(&consumed));
    native.design_body_bindings.push(unsolved(1, false));
    assert!(invalid(&native));
}
