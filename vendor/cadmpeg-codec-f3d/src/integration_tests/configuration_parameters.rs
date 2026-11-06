// SPDX-License-Identifier: Apache-2.0

use std::io::{Cursor, Write};

use super::{assert_valid, decode};
use crate::design::test_support::parameter_record;
use crate::test_support::configuration_table::{
    expression_cell, object_target, root_record, table_document, with_root_segment, ROOT_RECORD,
};
use crate::test_support::manifest_test::write_synthetic_manifests;
use crate::test_support::native_test::f3d_native;
use crate::test_support::smbh_geometry_test::synthetic_geometry_smbh;
use crate::test_support::zip_test::f3d_with_configuration;

const TABLE: &str = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationTable.synthetic.dsgcfg";

fn assert_configuration_valid(decoded: &cadmpeg_test_support::EditableDecodeResult) {
    assert_valid(decoded);
    let findings = crate::test_support::with_decode_context(|ctx| {
        crate::validate::validate_native(ctx, decoded.ir())
            .expect("validate native parameters and table")
    });
    assert!(findings.is_empty(), "{findings:#?}");
}

fn archive(table: &[u8], second_discriminator: u64) -> Vec<u8> {
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    zip.start_file("FusionAssetName[Active]/Design1/BulkStream.dat", stored)
        .unwrap();
    for (index, name, expression, value, discriminator) in [
        (71u32, "Width", "20 mm", 2.0, 5u64),
        (72u32, "Height", "Width / 2", 1.0, second_discriminator),
    ] {
        let mut frame =
            parameter_record(None, expression, "User Parameter", Some("mm"), name, value);
        frame[7..11].copy_from_slice(&index.to_le_bytes());
        frame[22..30].copy_from_slice(&discriminator.to_le_bytes());
        zip.write_all(&frame).unwrap();
    }
    zip.start_file(TABLE, stored).unwrap();
    zip.write_all(table).unwrap();
    zip.finish().unwrap().into_inner()
}

fn width_table() -> Vec<u8> {
    serde_json::to_vec(&table_document(
        "1",
        &[("W", object_target("71", "UserParameterValue"))],
        &[
            ("large", "Large", vec![("W", expression_cell("30 mm", 3.0))]),
            ("small", "Small", vec![("W", expression_cell("10 mm", 1.0))]),
        ],
        &["small", "large"],
    ))
    .unwrap()
}

#[test]
fn configuration_empty_table_preserves_discriminator_five_user_parameters() {
    let decoded = decode(archive(b"{}", 5));
    let native = f3d_native(decoded.ir());
    assert_eq!(native.design_parameters.len(), 2);
    assert_eq!(native.design_configurations.len(), 1);
    assert!(native.design_configurations[0].payload().is_empty());
    assert!(decoded.ir().model.configurations.is_empty());
    let parameters = &decoded.ir().model.parameters;
    assert_eq!(parameters.len(), 2);
    let width = parameters.iter().find(|p| p.name == "Width").unwrap();
    let height = parameters.iter().find(|p| p.name == "Height").unwrap();
    assert_eq!(width.expression, "20 mm");
    assert_eq!(height.expression, "Width / 2");
    assert_eq!(
        width.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(20.0).unwrap()
        ))
    );
    assert_eq!(
        height.value,
        Some(cadmpeg_ir::features::ParameterValue::Length(
            cadmpeg_ir::scalar::Length::new(10.0).unwrap()
        ))
    );
    assert_eq!(height.dependencies.iter().collect::<Vec<_>>(), [&width.id]);
    for parameter in &native.design_parameters {
        assert_eq!(parameter.unit().unwrap().value, "mm");
        assert_eq!(parameter.owner_record_index(), None);
    }
    assert_configuration_valid(&decoded);
}

#[test]
fn configuration_parameter_cells_bind_to_neutral_parameter_identity() {
    let decoded = decode(with_root_segment(
        archive(&width_table(), 5),
        root_record(*b"256", ROOT_RECORD, "1", "large"),
    ));
    let native = f3d_native(decoded.ir());
    let active_row = native.design_configurations[0].active_row().unwrap();
    assert_eq!(
        (
            active_row.record_index,
            active_row.table_id.as_str(),
            active_row.row_id.as_str()
        ),
        (ROOT_RECORD, "1", "large")
    );
    let width = decoded
        .ir()
        .model
        .parameters
        .iter()
        .find(|p| p.name == "Width")
        .unwrap();
    let configurations = &decoded.ir().model.configurations;
    assert_eq!(configurations.len(), 2);
    for (ordinal, name, expression, active) in
        [(0, "Small", "10 mm", false), (1, "Large", "30 mm", true)]
    {
        let configuration = configurations
            .iter()
            .find(|c| c.name.as_deref() == Some(name))
            .unwrap();
        assert_eq!(configuration.ordinal, ordinal);
        assert_eq!(configuration.active, active);
        assert_eq!(
            configuration
                .parameter_overrides
                .get(&width.id)
                .map(String::as_str),
            Some(expression)
        );
        assert!(configuration.properties.is_empty());
    }
    let configuration_losses = [
        crate::loss::F3dLossCode::ConfigurationMemberUnassigned,
        crate::loss::F3dLossCode::ConfigurationRuleUnbound,
        crate::loss::F3dLossCode::ConfigurationParameterOverrideUnbound,
        crate::loss::F3dLossCode::ConfigurationFeatureSuppressionUnbound,
        crate::loss::F3dLossCode::ConfigurationActiveRowUnbound,
    ]
    .map(crate::loss::F3dLossCode::kind);
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| configuration_losses.contains(&loss.code)));
    assert_configuration_valid(&decoded);
}

#[test]
fn configuration_root_outside_its_form_reports_an_unbound_active_row() {
    let mut malformed = root_record(*b"256", ROOT_RECORD, "1", "large");
    malformed[53] = 2;
    let rule = "FusionAssetName[Active]/DesignConfigurationTable.BlobParts/DesignConfigurationRule.synthetic.dsgcfgrule";
    for (name, entry, payload, root) in [
        ("malformed root", TABLE, width_table(), malformed),
        (
            "rule only",
            rule,
            b"{}".to_vec(),
            root_record(*b"256", ROOT_RECORD, "1", "large"),
        ),
    ] {
        let decoded = decode(with_root_segment(
            f3d_with_configuration(&synthetic_geometry_smbh(), entry, &payload),
            root,
        ));
        assert!(
            decoded
                .report()
                .losses
                .iter()
                .any(|loss| loss.code
                    == crate::loss::F3dLossCode::ConfigurationActiveRowUnbound.kind()),
            "{name}"
        );
        assert!(
            decoded.ir().model.configurations.iter().all(|c| !c.active),
            "{name}"
        );
        assert!(
            f3d_native(decoded.ir())
                .design_configurations
                .iter()
                .all(|c| c.active_row().is_none()),
            "{name}"
        );
    }
    let decoded = decode(with_root_segment(
        f3d_with_configuration(&synthetic_geometry_smbh(), TABLE, &width_table()),
        root_record(*b"256", ROOT_RECORD, "1", "large"),
    ));
    assert!(!decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ConfigurationActiveRowUnbound.kind()));
    assert!(decoded
        .ir()
        .model
        .configurations
        .iter()
        .any(|c| c.active && c.name.as_deref() == Some("Large")));
}

#[test]
fn configuration_parameter_refusal_reports_unparsed_record() {
    let decoded = decode(archive(b"{}", 9));
    assert_eq!(decoded.ir().model.parameters.len(), 1);
    assert_eq!(f3d_native(decoded.ir()).design_parameters.len(), 1);
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ParameterRecordUnparsed.kind()));
    assert_configuration_valid(&decoded);
}

/// A frame whose plain tail names a registered parameter-tail record but
/// whose prefix matches no parameter grammar reports the unparsed-record
/// loss through the full decode.
#[test]
fn configuration_registered_tail_refusal_reports_unparsed_record() {
    const TAIL_TYPE_GUID: &str = "B0556F58-EE09-46A7-B18F-E296F48E5B13";
    let target = 181u64;
    let mut frame = parameter_record(None, "3 mm", "User Parameter", Some("mm"), "Width", 0.3);
    frame[22..30].copy_from_slice(&9u64.to_le_bytes());
    let tail = frame.len() - 12;
    frame[tail + 2..tail + 10].copy_from_slice(&target.to_le_bytes());
    let metadata = crate::test_support::streams_test::design_metastream_with_records(
        &[(
            TAIL_TYPE_GUID,
            "A7AEA631-985B-4DD1-8CE2-DE2C-14B54081",
            3,
            "CommonData",
            &[target],
        )],
        &[],
    );
    let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
    let stored = crate::zip_write::file_options(zip::CompressionMethod::Stored);
    write_synthetic_manifests(&mut zip, stored);
    for (name, payload) in [("BulkStream.dat", frame), ("MetaStream.dat", metadata)] {
        zip.start_file(format!("FusionAssetName[Active]/Design1/{name}"), stored)
            .unwrap();
        zip.write_all(&payload).unwrap();
    }
    let decoded = decode(zip.finish().unwrap().into_inner());
    assert!(decoded.ir().model.parameters.is_empty());
    assert!(decoded
        .report()
        .losses
        .iter()
        .any(|loss| loss.code == crate::loss::F3dLossCode::ParameterRecordUnparsed.kind()));
}
