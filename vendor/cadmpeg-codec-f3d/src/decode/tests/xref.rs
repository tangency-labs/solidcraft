// SPDX-License-Identifier: Apache-2.0
//! Decode-owner external-reference projection tests.

use super::super::report_xref_placement_overrides;
use crate::loss::F3dLossCode;

fn placement_table() -> crate::xref::XrefTable {
    crate::xref::XrefTable {
        designs: Vec::new(),
        references: vec![crate::records::xref::XrefReference {
            current_pose: None,
            operation_pose: None,
            contained_poses: Vec::new(),
            owned_poses: Vec::new(),
            id: "f3d:xref:reference#4-occurrence-0".into(),
            ordinal: 4,
            occurrence_ordinal: 0,
            placement_source: None,
            from: "root.f3d".to_owned().try_into().unwrap(),
            relative_path: "part.f3d".to_owned().try_into().unwrap(),
            neutron_role: "role-guid".to_owned().try_into().unwrap(),
            neutron_data: "data-guid".into(),
            transform: None,
        }],
        placement_failures: Vec::new(),
        placement_metadata_missing: false,
        placement_overrides: vec![crate::xref::PlacementOverride {
            ordinal: 4,
            count: 2,
        }],
        ..Default::default()
    }
}

#[test]
fn superseded_xref_placements_have_a_distinct_loss_note() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    let table = placement_table();

    report_xref_placement_overrides(&mut report, &table);

    let loss = report
        .losses
        .iter()
        .find(|loss| loss.code == F3dLossCode::XrefPlacementSuperseded.kind())
        .expect("superseded placement loss");
    assert_eq!(loss.message, "2 structured placement record(s) for external occurrence part.f3d and role role-guid were superseded by scope-bound Component Insert carrier(s)");
}

#[test]
fn missing_placement_metadata_has_a_distinct_loss_note() {
    let mut report = cadmpeg_ir::codec::DecodeBody {
        transfer: cadmpeg_ir::report::decode::DecodeTransfer::full(true),
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses: Vec::new(),
        notes: Vec::new(),
        transfer_ledger: Default::default(),
    };
    let mut table = placement_table();
    table.placement_metadata_missing = true;
    super::super::report_xref_placement_failures(&mut report, &table);
    assert_eq!(report.losses.len(), 1);
    assert_eq!(
        report.losses[0].code,
        F3dLossCode::XrefPlacementMetadataMissing.kind()
    );
}

#[test]
fn assembly_property_note_preserves_role_and_data_text() {
    let table = placement_table();
    assert_eq!(
        super::super::XrefPropertyNote(&table.references[0]).to_string(),
        "neutronRole role-guid, neutronData data-guid"
    );
}
