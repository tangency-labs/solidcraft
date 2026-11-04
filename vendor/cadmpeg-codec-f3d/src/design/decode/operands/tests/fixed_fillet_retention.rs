// SPDX-License-Identifier: Apache-2.0

#[test]
fn fixed_fillet_unassigned_groups_emit_one_scope_loss() {
    use crate::records::feature::scope::{
        DesignFeatureKind, DesignParameterScope, DesignScopePayloadMut,
    };
    use crate::records::topology::construction::DesignConstructionOperandGroup;
    let mut scope = DesignParameterScope::empty("f3d:test:scope#10", DesignFeatureKind::Fillet, 10);
    let DesignScopePayloadMut::Fillet(slot) = scope.payload_mut() else {
        panic!("Fillet fixture");
    };
    *slot = Some(
        serde_json::from_value(serde_json::json!({"groups": [{
            "radii": [0.2], "radius_offsets": [2020], "radius_record_indexes": [202]
        }]}))
        .unwrap(),
    );
    let first: DesignConstructionOperandGroup = serde_json::from_value(serde_json::json!({
        "id": "f3d:test:group#100", "record_index": 100, "scope_record_index": 10,
        "scope_reference_ordinal": 0, "byte_offset": 0, "class_tag": "277",
        "members": [101], "member_offsets": [26], "role": 0x0000_0008_0000_0000_u64,
        "role_offset": 40, "paired_class_tag": "258", "paired_byte_offset": 80,
        "frame": {"member_count_offset": 21, "opaque_index": 1,
            "opaque_index_offset": 58, "opaque_scalar": 0.0,
            "opaque_scalar_offset": 62, "variant": false}
    }))
    .unwrap();
    assert!(super::super::fixed_fillet_assignment_losses(
        std::slice::from_ref(&scope),
        std::slice::from_ref(&first),
    )
    .is_empty());
    let mut second = first.clone();
    second.record_index = 102;
    second.id = "f3d:test:group#102".into();
    let losses = super::super::fixed_fillet_assignment_losses(&[scope], &[first, second]);
    assert_eq!(losses.len(), 1);
    assert_eq!(
        losses[0].code.local_code(),
        "feature.fixed-fillet-assignment-native"
    );
}
