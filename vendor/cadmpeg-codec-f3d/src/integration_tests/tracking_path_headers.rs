// SPDX-License-Identifier: Apache-2.0
//! A construction-operand identity whose entity-tracking path does not follow
//! its identity wrapper's record index.

use crate::test_support::lost_edge_test::{TRACKING_IDENTITY_WRAPPER, TRACKING_WRAPPER};
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::step_conversion::convert_to_step;
use crate::test_support::zip_test::f3d_with_smbh_and_remove_body_tracking_path;

const INVALID_NESTED_FRAME: &str =
    "Fusion Design construction operand identity has an invalid nested frame";

#[test]
fn tracking_path_after_an_unrelated_wrapper_holds_its_headers_and_validates() {
    let bytes = f3d_with_smbh_and_remove_body_tracking_path(&synthetic_geometry_smbh());
    let conversion = convert_to_step(&bytes);
    let native = conversion
        .ir
        .native
        .namespace("f3d")
        .expect("F3D native namespace");
    let identities = &native.arenas()["design_construction_operand_identities"];
    let identity = identities
        .iter()
        .find(|identity| {
            identity
                .field("wrapper_record_indices")
                .and_then(|indices| indices.as_array().cloned())
                == Some(vec![TRACKING_IDENTITY_WRAPPER.into()])
        })
        .expect("the identity behind the RemoveBody group");
    let path = identity.field("tracking_path").expect("the tracking path");
    assert_eq!(
        path.get("wrapper_record_index")
            .and_then(serde_json::Value::as_u64),
        Some(u64::from(TRACKING_WRAPPER))
    );
    let held = native.arenas()["design_record_headers"]
        .iter()
        .filter_map(|header| {
            header
                .field("record_index")
                .and_then(|index| index.as_u64())
        })
        .collect::<std::collections::HashSet<_>>();
    for record_index in [TRACKING_WRAPPER, TRACKING_WRAPPER + 1, TRACKING_WRAPPER + 2] {
        assert!(held.contains(&u64::from(record_index)), "{record_index}");
    }
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, &conversion.ir)
            .expect("validate the native namespace")
    });
    assert!(
        findings
            .iter()
            .all(|finding| finding.message != INVALID_NESTED_FRAME),
        "{findings:?}"
    );
}
