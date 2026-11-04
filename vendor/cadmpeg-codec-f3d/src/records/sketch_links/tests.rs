// SPDX-License-Identifier: Apache-2.0

use super::{PersistentDesignLink, PersistentDesignLinkWire, PERSISTENT_DESIGN_LINK_CLONE_COUNT};

fn link() -> PersistentDesignLink {
    PersistentDesignLink {
        id: "f3d:native:persistent-link#0".into(),
        target: cadmpeg_ir::attributes::AttributeTarget::Document,
        design_id: "00123".to_owned().try_into().unwrap(),
        design_reference: -4,
        ordinal: 2,
    }
}

#[test]
fn persistent_design_link_borrowed_wire_matches_owned_wire_bytes() {
    let record = link();
    let owned = PersistentDesignLinkWire::from(record.clone());
    assert_eq!(
        serde_json::to_vec(&record).unwrap(),
        serde_json::to_vec(&owned).unwrap()
    );
}

#[test]
fn persistent_design_link_native_retained_limit_refuses_before_record_clone() {
    let record = link();
    crate::test_support::native_test::assert_borrowed_native_retained_limit(
        &record,
        "persistent_design_links",
        || PERSISTENT_DESIGN_LINK_CLONE_COUNT.with(|count| count.set(0)),
        || PERSISTENT_DESIGN_LINK_CLONE_COUNT.with(std::cell::Cell::get),
    );
}
