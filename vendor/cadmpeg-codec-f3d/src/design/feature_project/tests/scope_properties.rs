// SPDX-License-Identifier: Apache-2.0
use crate::design::feature_project::scope_properties;
use crate::records::feature::scope::{DesignFeatureKind, DesignParameterScope};

#[test]
fn scope_properties_record_reference_members() {
    let scope = DesignParameterScope::empty(
        "f3d:Design/BulkStream.dat:design-parameter-scope#10",
        DesignFeatureKind::Combine,
        10,
    );

    let properties = scope_properties(&scope, "f3d:Design/BulkStream.dat", &[]).unwrap();
    assert_eq!(properties.len(), 1);
    assert_eq!(
        properties[&cadmpeg_core::nonblank_literal!("reference:0")],
        "10"
    );
}
