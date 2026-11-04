// SPDX-License-Identifier: Apache-2.0

use crate::records::configuration::{DesignConfiguration, DesignConfigurationKind};
use crate::test_support::configuration_table::{property_target, string_cell, table_document};
use crate::test_support::native_test::update_f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_smbh;

const FINDING: &str = "Fusion Design configurations have no single authored table order";

fn document(name: &str, rows: bool) -> DesignConfiguration {
    let rows = if rows {
        vec![("r", "Row", vec![("P", string_cell("x"))])]
    } else {
        Vec::new()
    };
    let order = if rows.is_empty() { vec![] } else { vec!["r"] };
    DesignConfiguration::try_new(
        name.into(),
        DesignConfigurationKind::Table,
        table_document("1", &[("P", property_target("Part Number"))], &rows, &order)
            .as_object()
            .unwrap()
            .clone(),
    )
    .unwrap()
}

#[test]
fn configuration_documents_with_rows_in_two_tables_have_no_single_order() {
    use cadmpeg_ir::codec::Codec;
    let decoded = cadmpeg_test_support::EditableDecodeResult::from(
        crate::F3dCodec
            .decode(
                &mut std::io::Cursor::new(f3d_with_smbh(&synthetic_geometry_smbh())),
                &cadmpeg_ir::codec::DecodeOptions::default(),
            )
            .unwrap(),
    );
    for (second_has_rows, expected) in [(false, false), (true, true)] {
        let mut ir = decoded.ir().clone();
        update_f3d_native(&mut ir, |native| {
            native.design_configurations = vec![
                document("a.dsgcfg", true),
                document("b.dsgcfg", second_has_rows),
            ];
        });
        let findings = crate::test_support::with_decode_context(|ctx| {
            super::super::validate_native(ctx, &ir).unwrap()
        });
        assert_eq!(
            findings.iter().any(|finding| finding.message == FINDING),
            expected,
            "{findings:#?}"
        );
    }
}
