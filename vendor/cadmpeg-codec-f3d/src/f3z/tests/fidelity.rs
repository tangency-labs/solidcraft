// SPDX-License-Identifier: Apache-2.0

use cadmpeg_test_support::EditableDecodeResult;

use crate::test_support::assembly_test::{f3d_without_brep, f3z_archive, XREF_ROLE};
use crate::test_support::smbh_geometry_test::synthetic_mixed_smbh;
use crate::test_support::zip_test::f3d_with_smbh;
use cadmpeg_ir::codec::DecodeOptions;
use std::io::Cursor;

#[test]
fn merged_archive_keeps_each_component_unknown_record_image_and_owner() {
    let component = f3d_with_smbh(&synthetic_mixed_smbh());
    let original = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(component.clone()),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    let unknowns = original.ir().native_unknowns("f3d").unwrap();
    assert!(!unknowns.is_empty());
    let root = f3d_without_brep("assembly-design", "root.f3d", &[("comp.f3d", XREF_ROLE)]);
    let archive = f3z_archive("root.f3d", &[("root.f3d", &root), ("comp.f3d", &component)]);
    let decoded = EditableDecodeResult::from(
        crate::test_support::archive_session::decode(
            &mut Cursor::new(archive),
            &DecodeOptions::default(),
        )
        .unwrap(),
    );
    for unknown in unknowns {
        let source_id = unknown.id.as_str();
        let original_record = original
            .source_fidelity()
            .retained_record(source_id)
            .unwrap();
        let suffix = source_id.strip_prefix("f3d:").unwrap();
        let id = format!("f3d:xref/role-{XREF_ROLE}/reference-0/occurrence-0/{suffix}");
        let retained = decoded
            .source_fidelity()
            .retained_record(&id)
            .expect("merged unknown has its source record");
        assert_eq!(retained.data(), original_record.data());
        assert_eq!(retained.sha256(), original_record.sha256());
        assert_eq!(retained.offset(), original_record.offset());
        assert_eq!(retained.byte_len(), original_record.byte_len());
        let owner = format!(
            "f3d:xref/role-{XREF_ROLE}%2Freference-0%2Foccurrence-0/{}",
            original_record.stream()
        );
        assert_eq!(retained.stream(), owner);
        if let Some(provenance) = decoded.source_fidelity().annotations.provenance.get(&id) {
            assert_eq!(provenance.stream(), owner);
        }
    }
    let source_image =
        format!("f3d:xref/role-{XREF_ROLE}/reference-0/occurrence-0/file:source-image#0");
    assert_eq!(
        decoded
            .source_fidelity()
            .retained_record(&source_image)
            .unwrap()
            .data(),
        Some(component.as_slice())
    );
    assert!(decoded
        .source_fidelity()
        .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
        .is_none());
}
