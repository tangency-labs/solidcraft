// SPDX-License-Identifier: Apache-2.0

use super::{assert_valid, decode};
use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_geometry_with_history_smbh;
use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_base_feature;

#[test]
fn product_history_owner_survives_two_component_archive_merge() {
    let component = f3d_with_smbh_and_protein_with_generated_base_feature(
        &synthetic_geometry_with_history_smbh(),
    );
    let root = f3d_without_brep(
        "owner-root",
        "root.f3d",
        &[("left.f3d", XREF_ROLE), ("right.f3d", XREF_ROLE)],
    );
    let archive = f3z_archive(
        "root.f3d",
        &[
            ("root.f3d", &root),
            ("left.f3d", &component),
            ("right.f3d", &component),
        ],
    );
    let decoded = decode(archive);
    let model = &decoded.ir().model;
    assert!(!model.features.is_empty());
    let owners = model
        .features
        .iter()
        .filter_map(|row| row.product_definition.as_ref())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(owners.len(), 2);
    for feature in &model.features {
        let owner = feature
            .product_definition
            .as_ref()
            .expect("member feature owner");
        assert!(model.product_definitions.iter().any(|row| &row.id == owner));
    }
    assert_valid(&decoded);
}

#[test]
fn root_sketch_owner_follows_document_membership() {
    use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
    use crate::test_support::zip_test::f3d_with_smbh_and_protein_with_generated_sketch_dimension;

    let document =
        f3d_with_smbh_and_protein_with_generated_sketch_dimension(&synthetic_geometry_smbh());
    let standalone = decode(document.clone());
    let standalone_model = &standalone.ir().model;
    assert_eq!(standalone_model.sketches.len(), 1);
    let sketch = &standalone_model.sketches[0].id;
    assert!(standalone_model
        .product_definitions
        .iter()
        .all(|definition| !definition.sketches.contains(sketch)));
    assert_valid(&standalone);

    let root = f3d_without_brep("owner-root", "root.f3d", &[("member.f3d", XREF_ROLE)]);
    let archive = f3z_archive(
        "root.f3d",
        &[("root.f3d", &root), ("member.f3d", &document)],
    );
    let merged = decode(archive);
    let model = &merged.ir().model;
    assert_eq!(model.sketches.len(), 1);
    let holders = model
        .product_definitions
        .iter()
        .filter(|definition| definition.sketches.contains(&model.sketches[0].id))
        .collect::<Vec<_>>();
    assert_eq!(holders.len(), 1);
    assert!(holders[0]
        .id
        .as_str()
        .ends_with("archive:product-definition#0"));
    assert!(model.occurrences.iter().any(|occurrence| matches!(
        &occurrence.prototype,
        cadmpeg_ir::products::PrototypeReference::Local { definition }
            if definition == &holders[0].id
    )));
    assert_valid(&merged);
}
