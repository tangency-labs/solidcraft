// SPDX-License-Identifier: Apache-2.0
//! The registry is the oracle for the pinned ids, so the test reads it rather
//! than a second copy of the list.

#![allow(clippy::unwrap_used)]

use super::{
    classify_layers, dialect_loss, kernel_dialect_loss, F3dDialect, DECLARED_ROOT_DOCUMENT_MEMBERS,
    DECLARED_TOP_LEVEL_MANIFEST_VERSION, FORMAT,
};
use crate::loss::F3dLossCode;
use cadmpeg_core::dialect::{Admission, DialectMatch};
use std::collections::BTreeMap;

fn classify_document(version: &str) -> DialectMatch {
    F3dDialect::classify_document(Some(version))
}

fn classify_f3z(members: &[&str]) -> DialectMatch {
    F3dDialect::classify_f3z(members)
}

#[test]
fn enum_and_registry_rows_are_closed_bidirectionally() -> Result<(), Box<dyn std::error::Error>> {
    cadmpeg_test_support::assert_dialect_rows_closed(&F3dDialect::ALL.map(F3dDialect::id), FORMAT)?;
    Ok(())
}

#[test]
fn duplicate_kernel_identity_is_omitted_with_a_typed_loss() {
    let bytes = crate::test_support::zip_test::synthetic_f3d(true);
    let arena = cadmpeg_core::decode::DecodeArena::new();
    let policy = cadmpeg_core::decode::DecodePolicy::default();
    let (ctx, root) =
        cadmpeg_core::decode::DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
    let mut scan = crate::container::scan(&ctx, root).unwrap();
    scan.breps.push(scan.breps[0].clone());

    let (layers, losses) = classify_layers(&scan).unwrap();
    assert_eq!(losses.len(), 1);
    assert_eq!(losses[0].code, F3dLossCode::DialectLayerCollision.kind());
    assert_eq!(
        layers
            .iter()
            .filter(|layer| layer.format() == cadmpeg_asm::dialect::FORMAT)
            .count(),
        2
    );
}

#[test]
fn a_document_match_names_its_row_and_records_the_version_the_parse_read() {
    let matched = classify_document("3-2-0-0");

    assert_eq!(matched.format(), FORMAT);
    assert_eq!(matched.dialect().as_str(), "f3d:manifest-3-2-0-0");
    assert_eq!(
        matched.declared()[DECLARED_TOP_LEVEL_MANIFEST_VERSION],
        "3-2-0-0"
    );
    assert_eq!(matched.admission(), &Admission::Admitted);
}

#[test]
fn a_version_only_drift_lands_on_the_recovery_row_and_charges_the_loss() {
    // The parse ran the `3-2-0-0` layout and it fitted. The declaration names
    // no row this codec knows, so the reading is recorded verbatim, the row is
    // the recovery row, and the admission names the strategy applied.
    let matched = classify_document("3-3-0-0");

    assert_eq!(
        matched.declared()[DECLARED_TOP_LEVEL_MANIFEST_VERSION],
        "3-3-0-0"
    );
    assert_eq!(matched.dialect().as_str(), "f3d:unknown");
    assert!(matches!(matched.admission(), Admission::Unverified { .. }));
    assert_eq!(
        matched.using(),
        Some(cadmpeg_core::dialect_id!("f3d:manifest-3-2-0-0"))
    );

    let loss = dialect_loss(&matched).expect("the recovery is charged");
    assert_eq!(loss.code, F3dLossCode::SourceDialectUnverified.kind());
    assert!(loss.message.contains("3-3-0-0"));
    assert!(loss.message.contains("f3d:manifest-3-2-0-0"));
}

#[test]
fn a_residual_match_charges_without_inventing_a_substituted_grammar() {
    let matched =
        DialectMatch::residual(F3dDialect::Unknown.id()).with_declared(BTreeMap::from([(
            cadmpeg_core::nonblank_const!(DECLARED_TOP_LEVEL_MANIFEST_VERSION),
            "unframed".to_owned(),
        )]));

    let loss = dialect_loss(&matched).expect("a residual read is a recovery");
    assert_eq!(loss.code, F3dLossCode::SourceDialectUnverified.kind());
    assert!(loss.message.contains("residual parser path"));
    assert!(!loss.message.contains("f3d:manifest-3-2-0-0"));
}

#[test]
fn an_f3z_match_names_its_row_and_records_the_root_members() {
    let matched = classify_f3z(&["Assembly.f3d", "Part.f3d"]);

    assert_eq!(matched.format(), FORMAT);
    assert_eq!(matched.dialect().as_str(), "f3d:f3z-multi-document");
    assert_eq!(
        matched.declared()[DECLARED_ROOT_DOCUMENT_MEMBERS],
        "Assembly.f3d,Part.f3d"
    );
    assert!(
        !matched
            .declared()
            .contains_key(DECLARED_TOP_LEVEL_MANIFEST_VERSION),
        "the F3Z branch reads no version field, so it must declare none"
    );
    assert_eq!(matched.admission(), &Admission::Admitted);
}

#[test]
fn the_identity_rows_are_admitted_and_charge_nothing() {
    // A row parsed with the strategy it declares carries no recovery. The loss
    // and the admission are read from one value, so this pins both halves.
    for matched in [classify_document("3-2-0-0"), classify_f3z(&["Part.f3d"])] {
        assert_eq!(matched.admission(), &Admission::Admitted);
        assert!(dialect_loss(&matched).is_none());
    }
}

#[test]
fn the_totality_row_is_the_only_row_a_foreign_version_reaches() {
    assert_eq!(F3dDialect::Unknown.id().as_str(), "f3d:unknown");
    for matched in [classify_document("3-2-0-0"), classify_f3z(&["Part.f3d"])] {
        assert_ne!(
            matched.dialect().as_str(),
            F3dDialect::Unknown.id().as_str()
        );
    }
    assert_eq!(
        classify_document("4-0-0-0").dialect().as_str(),
        F3dDialect::Unknown.id().as_str()
    );
}

#[test]
fn a_carrier_collision_instance_is_not_presented_as_an_xref() {
    let matched = DialectMatch::unverified(
        cadmpeg_asm::dialect::ACIS_TEXT_ACIS,
        cadmpeg_core::dialect::Grammar::of(&cadmpeg_asm::dialect::ACIS_SAVE_FORMAT_218),
    )
    .with_declared(BTreeMap::from([(
        cadmpeg_core::nonblank_const!(cadmpeg_asm::dialect::DECLARED_CARRIER),
        "FusionAssetName[Active]/Breps.BlobParts/Body1.sat".to_owned(),
    )]))
    .with_instance("FusionAssetName[Active]/Breps.BlobParts/Body1.sat");

    let loss = kernel_dialect_loss(&matched).expect("unknown kernel grammar is unverified");
    assert!(!loss.message.contains("xref"));
    assert!(loss.message.contains("kernel carrier"));
}

#[test]
fn kernel_recovery_text_matches_shared_dialect_message() {
    let matched = DialectMatch::unverified(
        cadmpeg_asm::dialect::ACIS_TEXT_ACIS,
        cadmpeg_core::dialect::Grammar::of(&cadmpeg_asm::dialect::ACIS_SAVE_FORMAT_218),
    )
    .with_declared(BTreeMap::from([
        (
            cadmpeg_core::nonblank_const!(cadmpeg_asm::dialect::DECLARED_CARRIER),
            "Body.sat".to_owned(),
        ),
        (
            cadmpeg_core::nonblank_const!(cadmpeg_asm::dialect::DECLARED_SAVE_FORMAT_MAJOR),
            "218".to_owned(),
        ),
        (
            cadmpeg_core::nonblank_const!(cadmpeg_asm::dialect::DECLARED_SAVE_FORMAT_MINOR),
            "0".to_owned(),
        ),
    ]));
    let loss = kernel_dialect_loss(&matched).expect("unverified kernel loss");
    let expected =
        cadmpeg_asm::dialect::unverified_message("the kernel carrier Body.sat", &matched)
            .expect("shared kernel message");
    assert_eq!(loss.message, expected);
}
