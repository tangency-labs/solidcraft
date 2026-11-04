// SPDX-License-Identifier: Apache-2.0

use super::*;
use crate::loss::F3dLossCode;
use crate::test_support::assembly_test::{
    f3d_with_identity_xrefs, f3d_with_redirections_json, f3d_without_brep, redirections_json,
};

#[test]
fn placement_admission_preserves_existing_identity_and_omits_only_new_mixed_rows() {
    for (admitted, mixed) in [(false, false), (false, true), (true, false)] {
        let merged = admitted || !mixed;
        let targets = [("child.f3d", XREF_ROLE)];
        let root = if admitted {
            f3d_with_identity_xrefs("assembly-design", "root.f3d", &targets)
        } else if mixed {
            let mut table: serde_json::Value = serde_json::from_str(&redirections_json(
                "root.f3d",
                &[("child.f3d", XREF_ROLE), ("source.f3d", "source-role")],
            ))
            .unwrap();
            table["references"][1]["type"] = "DERIVED".into();
            f3d_with_redirections_json("assembly-design", &serde_json::to_vec(&table).unwrap())
        } else {
            f3d_without_brep("assembly-design", "root.f3d", &targets)
        };
        let child =
            f3d_with_identity_xrefs("assembly-design", "child.f3d", &[("leaf.f3d", XREF_ROLE)]);
        let leaf = f3d_with_smbh(&synthetic_geometry_smbh());
        let archive = f3z_archive(
            "root.f3d",
            &[
                ("root.f3d", &root),
                ("child.f3d", &child),
                ("leaf.f3d", &leaf),
            ],
        );
        let result = decode(archive.clone());
        assert_eq!(
            result.ir().model.bodies.len(),
            usize::from(merged),
            "admitted={admitted} {:?}",
            result.report()
        );
        assert_eq!(result.ir().model.occurrences.len(), 2 * usize::from(merged));
        assert!(result.ir().model.assembly_joints.is_empty());
        let native = f3d_native(result.ir());
        assert_eq!(native.xref_references.len(), 1 + usize::from(merged));
        let reference = native
            .xref_references
            .iter()
            .find(|reference| reference.from.as_str() == "root.f3d")
            .unwrap();
        assert_eq!(reference.placement_source.is_some(), admitted);
        assert!(reference.transform.is_none());
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| { loss.code == F3dLossCode::XrefOccurrenceUnplaced.kind() })
                .count(),
            usize::from(mixed)
        );
        if merged {
            assert!(result.ir().model.occurrences.iter().all(|occurrence| {
                occurrence.transform == cadmpeg_ir::transform::Transform::identity()
            }));
        } else {
            assert_eq!(
                result
                    .source_fidelity()
                    .retained_record(crate::ids::FILE_SOURCE_IMAGE_ID)
                    .and_then(|record| record.data()),
                Some(archive.as_slice())
            );
            let loss = result
                .report()
                .losses
                .iter()
                .find(|loss| loss.code == F3dLossCode::XrefOccurrenceUnplaced.kind())
                .unwrap();
            assert!(loss.message.contains("child.f3d"));
            assert!(result
                .report()
                .notes
                .iter()
                .all(|note| !note.contains(": merged ")));
        }
        assert_eq!(native.derived_references.len(), usize::from(mixed));
        assert_eq!(
            result
                .report()
                .losses
                .iter()
                .filter(|loss| loss.code == F3dLossCode::XrefPlacementIdentityAssumed.kind())
                .count(),
            usize::from(!admitted && !mixed)
        );
        assert_valid(&result);
    }
}

#[test]
fn unadmitted_existing_references_report_one_identity_warning_each() {
    let result = decode(f3d_without_brep(
        "assembly-design",
        "root.f3d",
        &[("first.f3d", "role"), ("second.f3d", "role")],
    ));
    assert_eq!(result.ir().model.occurrences.len(), 2);
    assert!(result.ir().model.bodies.is_empty());
    assert!(result.ir().model.assembly_joints.is_empty());
    assert_eq!(f3d_native(result.ir()).xref_references.len(), 2);
    assert_eq!(
        result
            .report()
            .losses
            .iter()
            .filter(|loss| { loss.code == F3dLossCode::XrefPlacementIdentityAssumed.kind() })
            .count(),
        2
    );
    let warnings = result
        .report()
        .losses
        .iter()
        .filter(|loss| loss.code == F3dLossCode::XrefPlacementIdentityAssumed.kind())
        .collect::<Vec<_>>();
    assert!(warnings.iter().all(
        |loss| loss.severity == cadmpeg_ir::report::Severity::Warning
            && loss.message.contains("placement did not decode")
            && loss.message.contains("identity without proof")
    ));
    assert_valid(&result);
}
