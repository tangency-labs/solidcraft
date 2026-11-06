// SPDX-License-Identifier: Apache-2.0

fn native(valid: bool) -> crate::native::F3dNative {
    use crate::records::{
        decal::DesignRecordHeader,
        feature::scope::{DesignFeatureKind, DesignParameterScope},
        references::DesignClassTag,
    };
    let mut native = super::operand_group_carrier::native_with_identity();
    if valid {
        native
            .design_parameter_scopes
            .push(DesignParameterScope::empty(
                "f3d:Design/BulkStream.dat:design-parameter-scope#10",
                DesignFeatureKind::Fillet,
                10,
            ));
        native.design_record_headers.push(DesignRecordHeader {
            id: "f3d:Design/BulkStream.dat:design-record-header#101".into(),
            record_index: 101,
            class_tag: DesignClassTag::try_from("297".to_owned()).unwrap(),
            byte_offset: 100,
        });
    }
    native
}

#[test]
fn edge_identity_valid_slot_has_no_finding() {
    crate::test_support::with_decode_context(|service_ctx| {
        let mut ir = cadmpeg_ir::examples::unit_cube().unwrap();
        let native = native(true);
        native
            .store(
                &cadmpeg_test_support::service_decode_context(),
                ir.native.namespace_mut("f3d"),
            )
            .unwrap();
        let ctx = super::super::Ctx::new(&ir, &native, service_ctx);
        let mut findings = Vec::new();
        let records = crate::test_support::with_decode_context(|decode_ctx| {
            super::super::validate_edge_identity_operands(decode_ctx, &ctx, &mut findings, &[])
        })
        .unwrap();
        assert!(findings.is_empty());
        assert!(records.contains(&("f3d:Design/BulkStream.dat", 101)));
    })
}
