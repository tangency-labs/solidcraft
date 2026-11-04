// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{
    f3d_with_redirections_json, f3d_with_reference_table_and_identity_xrefs,
    f3z_archive_with_design_description, redirections_json,
};
use serde_json::{json, Value};

#[test]
fn derived_dependency_keeps_xref_merge_without_its_target() {
    for mixed in [false, true] {
        let targets = if mixed {
            vec![("source.f3d", "derived-role"), ("child.f3d", XREF_ROLE)]
        } else {
            vec![("source.f3d", "derived-role")]
        };
        let mut table: serde_json::Value =
            serde_json::from_str(&redirections_json("root.f3d", &targets)).unwrap();
        table["references"][0]["type"] = "DERIVED".into();
        let placements = if mixed {
            vec![("child.f3d", XREF_ROLE)]
        } else {
            Vec::new()
        };
        let root = f3d_with_reference_table_and_identity_xrefs(
            "assembly-design",
            "root.f3d",
            &placements,
            &serde_json::to_vec(&table).unwrap(),
        );
        let child = f3d_with_smbh(&synthetic_geometry_smbh());
        let description = serde_json::to_vec(&json!({
            "name": "Autodesk Design Description", "version": "0.1",
            "designDescription": {"id": "0", "designGraphs": [{
                "rootIds": [0], "designObjectRefs": [], "designObjects": [{
                    "id": 0, "relativePath": "root.f3d", "contentType": "f3d",
                    "references": [{"type": "DERIVED", "ids": [1], "relationships": [{
                        "id": 1, "metadata": {"workflow": "PushPullDerive", "neutronRole": "derived-role"}
                    }]}]
                }, {"id": 1, "relativePath": "source.f3d", "contentType": "f3d", "references": []}]
            }]}
        })).unwrap();
        let archive = f3z_archive_with_design_description(
            "root.f3d",
            &[("root.f3d", &root), ("child.f3d", &child)],
            &description,
        );
        let merged = decode(archive.clone());
        if !mixed {
            assert_eq!(
                merged
                    .source_fidelity()
                    .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
                    .and_then(|record| record.data()),
                Some(archive.as_slice())
            );
        }
        assert_eq!(merged.ir().model.bodies.len(), usize::from(mixed));
        assert_eq!(merged.ir().model.occurrences.len(), usize::from(mixed));
        let native = f3d_native(merged.ir());
        assert_eq!(native.derived_references.len(), 1);
        assert_eq!(native.xref_references.len(), usize::from(mixed));
        if mixed {
            assert!(merged.ir().model.bodies[0]
                .id
                .as_str()
                .contains("reference-1/occurrence-0/"));
        }
        for code in [
            crate::loss::F3dLossCode::XrefTableUndecoded,
            crate::loss::F3dLossCode::XrefMemberUndecoded,
            crate::loss::F3dLossCode::XrefMemberMissing,
        ] {
            assert!(!merged
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == code.kind()));
        }
        assert_valid(&merged);
    }
}

fn row(kind: &str, target: &str) -> Value {
    json!({
        "from": "root.f3d", "relativePath": target, "type": kind,
        "properties": [
            {"neutronRole": {"value": "role", "dataType": "STRING"}},
            {"neutronData": {"value": "data", "dataType": "STRING"}}
        ]
    })
}

fn table(rows: &[Value]) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "name": "RedirectionsStream", "schema-version": 0,
        "designs": [{"file-version": 1, "targetFileName": "root.f3d",
                     "displayName": "root", "lineageUrn": "urn:l", "versionUrn": "urn:v"}],
        "references": rows
    }))
    .unwrap()
}

#[test]
fn unknown_reference_type_reports_one_table_loss() {
    for kinds in [
        vec!["UNKNOWN"],
        vec!["XREF", "UNKNOWN"],
        vec!["DERIVED", "UNKNOWN", "XREF"],
    ] {
        let bytes = table(
            &kinds
                .iter()
                .map(|kind| row(kind, "part.f3d"))
                .collect::<Vec<_>>(),
        );
        for container_only in [false, true] {
            let decoded = F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_redirections_json("assembly-design", &bytes)),
                    &DecodeOptions {
                        depth: if container_only {
                            cadmpeg_core::decode::DecodeDepth::Container
                        } else {
                            cadmpeg_core::decode::DecodeDepth::Full
                        },
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            if !container_only {
                let native = f3d_native(decoded.ir());
                assert!(native.xref_references.is_empty());
                assert!(native.derived_references.is_empty());
            }
            assert_eq!(
                decoded
                    .report()
                    .losses
                    .iter()
                    .filter(|loss| loss.code == F3dLossCode::XrefTableUndecoded.kind())
                    .count(),
                1
            );
        }
    }
}

#[test]
fn derived_rows_decode_without_table_loss_and_survive_native_reload() {
    for mixed in [false, true] {
        let mut rows = vec![row("DERIVED", "source.f3d")];
        if mixed {
            rows.push(row("XREF", "child.f3d"));
        }
        let bytes = table(&rows);
        for container_only in [false, true] {
            let decoded = F3dCodec
                .decode(
                    &mut Cursor::new(f3d_with_redirections_json("assembly-design", &bytes)),
                    &DecodeOptions {
                        depth: if container_only {
                            cadmpeg_core::decode::DecodeDepth::Container
                        } else {
                            cadmpeg_core::decode::DecodeDepth::Full
                        },
                        ..DecodeOptions::default()
                    },
                )
                .unwrap();
            assert!(!decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code == F3dLossCode::XrefTableUndecoded.kind()));
            if container_only {
                assert!(decoded.ir().native.namespace("f3d").is_none());
                continue;
            }
            let native = f3d_native(decoded.ir());
            assert_eq!(native.derived_references.len(), 1);
            assert_eq!(native.xref_references.len(), usize::from(mixed));
            assert_eq!(
                native.derived_references[0].relative_path.as_str(),
                "source.f3d"
            );
        }
    }
}
