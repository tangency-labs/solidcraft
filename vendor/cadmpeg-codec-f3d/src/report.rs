// SPDX-License-Identifier: Apache-2.0
//! Decode and inspection reports assembled from F3D identity and transfer facts.

use std::collections::BTreeMap;

use cadmpeg_core::dialect::DialectLayers;
use cadmpeg_ir::codec::DecodeBody;
use cadmpeg_ir::document::SourceMeta;
use cadmpeg_ir::report::loss::LossNote;
use cadmpeg_ir::ContainerSummary;

use crate::container::ContainerScan;

/// Identity owner of a decoded F3D member.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ReportScope {
    /// The member is the decoded document and owns its classified layers.
    Standalone,
    /// The containing F3Z archive owns identity and classifies every member.
    ArchiveMember(DialectLayers),
}

/// Build a decode body from route-owned losses. Identity is authored once, on
/// the document, by [`classify_document`].
pub(crate) fn build_decode_report(
    scan: &ContainerScan<'_>,
    transfer: cadmpeg_ir::report::decode::DecodeTransfer,
    mut losses: Vec<LossNote>,
) -> Result<DecodeBody, cadmpeg_core::CodecError> {
    header_losses(scan, &mut losses)?;
    Ok(DecodeBody {
        transfer,
        coverage: cadmpeg_ir::report::decode::Coverage::default(),
        losses,
        notes: crate::container::summary_notes(
            scan,
            if transfer.container_only() {
                crate::container::SummaryScope::ContainerOnly
            } else {
                crate::container::SummaryScope::FullDecode
            },
        ),
        transfer_ledger: cadmpeg_ir::report::decode::TransferLedger::default(),
    })
}

/// Constructs the document's source metadata once from route-owned identity.
///
/// A standalone document classifies its own layers and charges classification
/// and dialect losses ahead of route losses. An archive member uses the outer
/// archive layers already classified by the F3Z session.
pub(crate) fn classify_document(
    scan: &ContainerScan<'_>,
    scope: ReportScope,
    attributes: BTreeMap<String, String>,
    body: &mut DecodeBody,
) -> Result<SourceMeta, cadmpeg_core::CodecError> {
    let dialects = match scope {
        ReportScope::Standalone => {
            let (dialects, mut losses) = crate::dialect::classify_layers(scan)?;
            losses.extend(crate::dialect::dialect_losses(&dialects));
            losses.append(&mut body.losses);
            body.losses = losses;
            dialects
        }
        ReportScope::ArchiveMember(dialects) => dialects,
    };
    Ok(SourceMeta::classified(
        dialects,
        cadmpeg_core::text::named_entries("the f3d document", attributes)?,
    ))
}

/// Build a single-document inspection summary with the same dialect facts that
/// decode projects into losses.
pub(crate) fn build_inspection_summary(
    scan: &ContainerScan<'_>,
) -> Result<ContainerSummary, cadmpeg_core::CodecError> {
    let (layers, mut losses) = crate::dialect::classify_layers(scan)?;
    losses.extend(crate::dialect::dialect_losses(&layers));
    let mut summary = crate::container::summarize(scan, layers);
    header_losses(scan, &mut losses)?;
    summary.losses = losses;
    Ok(summary)
}

/// Preserve per-entry recovery diagnostics on all inspection and decode routes.
pub(crate) fn header_losses(
    scan: &ContainerScan<'_>,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    for diagnostic in &scan.entry_diagnostics {
        losses.push(crate::loss::F3dLossCode::ArchiveEntryUnreadable.note(diagnostic.to_owned()));
    }
    for (name, run, stored) in &scan.unindexed_record_runs {
        let retained = stored.checked_sub(run.start).ok_or_else(|| {
            cadmpeg_core::CodecError::malformed(
                "F3D unindexed record run exceeds its stored stream",
            )
        })?;
        losses.push(
            crate::loss::F3dLossCode::UnindexedRecordsRetained
                .note(format!(
                    "{name} stores {} record header(s) in {} byte(s) after its last indexed record, restarting at record index {}; they are not decoded and are retained natively.",
                    run.headers,
                    retained,
                    run.first_record_index
                ))
                .with_provenance(cadmpeg_ir::SourceProvenance::in_stream(
                    "f3d",
                    cadmpeg_ir::stream_name!("BulkStream.dat"),
                    cadmpeg_core::decode::u64_from_index(run.start),
                )),
        );
    }
    for diagnostic in &scan.manifest_diagnostics {
        let message = format!(
            "{} is unreadable; manifest selectors retained",
            diagnostic.field
        );
        losses.push(
            crate::loss::F3dLossCode::ManifestMetadataUnresolved
                .note(message)
                .with_provenance(cadmpeg_ir::SourceProvenance::in_stream(
                    "f3d",
                    cadmpeg_ir::stream_name!("Manifest.dat"),
                    cadmpeg_core::decode::u64_from_index(diagnostic.offset),
                )),
        );
    }

    for brep in &scan.breps {
        let Some(header) = brep
            .kernel
            .as_ref()
            .and_then(crate::container::KernelFraming::model_metadata)
        else {
            continue;
        };
        kernel_tolerance_losses(&brep.name, header.linear, header.angular, losses)?;
        for field in header.unreadable_product_fields() {
            let message = format!(
                "BREP {} header {field} is unreadable; independent records retained",
                brep.name
            );
            losses.push(crate::loss::F3dLossCode::KernelHeaderMetadataUnresolved.note(message));
        }
    }
    for name in crate::container::text_brep_names(scan) {
        let Some(crate::container::TextBrepFraming::Parsed(stream)) = scan.text_breps.get(name)
        else {
            continue;
        };
        kernel_tolerance_losses(
            name,
            stream
                .header
                .resabs_cm()
                .map(cadmpeg_ir::scalar::NonNegativeReal::get),
            stream
                .header
                .resnor()
                .map(cadmpeg_ir::scalar::NonNegativeReal::get),
            losses,
        )?;
        for diagnostic in stream.header.diagnostics.iter().chain(&stream.framing) {
            let message = format!(
                "text BREP {name} byte {}: {}; independent records retained",
                diagnostic.error.offset, diagnostic.error.reason
            );
            let stream_name = cadmpeg_ir::StreamName::try_from(name.to_owned())
                .map_err(cadmpeg_core::CodecError::malformed)?;
            losses.push(
                crate::loss::F3dLossCode::TextHeaderNoncanonical
                    .note(message)
                    .with_provenance(cadmpeg_ir::SourceProvenance::in_stream(
                        "f3d",
                        stream_name,
                        cadmpeg_core::decode::u64_from_index(diagnostic.error.offset),
                    )),
            );
        }
    }
    Ok(())
}

/// Interpret only positive finite tolerances in the canonical document units.
fn kernel_tolerance_losses(
    name: &str,
    linear_cm: Option<f64>,
    angular: Option<f64>,
    losses: &mut Vec<LossNote>,
) -> Result<(), cadmpeg_core::CodecError> {
    for (field, value) in [
        ("resabs", linear_cm.map(|value| value * 10.0)),
        ("resnor", angular),
    ] {
        if value.is_some_and(|value| !value.is_finite() || value <= 0.0) {
            let message = format!("BREP {name} header {field} cannot supply a positive finite document tolerance; value left uninterpreted");
            let stream = cadmpeg_ir::StreamName::try_from(name.to_owned())
                .map_err(cadmpeg_core::CodecError::malformed)?;
            losses.push(
                crate::loss::F3dLossCode::KernelHeaderToleranceUnresolved
                    .note(message)
                    .with_provenance(cadmpeg_ir::SourceProvenance::in_stream("f3d", stream, 0)),
            );
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use cadmpeg_core::decode::{DecodeArena, DecodeContext, DecodePolicy};

    use super::{build_decode_report, classify_document, ReportScope};
    use crate::test_support::zip_test::synthetic_f3d;
    use std::collections::BTreeMap;

    #[test]
    fn decode_report_includes_a_kernel_identity_collision_loss() {
        let bytes = synthetic_f3d(true);
        let arena = DecodeArena::new();
        let policy = DecodePolicy::default();
        let (ctx, root) = DecodeContext::from_root_bytes(&bytes, &arena, &policy).unwrap();
        let mut scan = crate::container::scan(&ctx, root).unwrap();
        scan.breps.push(scan.breps[0].clone());

        let mut report = build_decode_report(
            &scan,
            cadmpeg_ir::report::decode::DecodeTransfer::full(true),
            Vec::new(),
        )
        .unwrap();
        let source =
            classify_document(&scan, ReportScope::Standalone, BTreeMap::new(), &mut report)
                .unwrap();
        assert!(source.dialects().is_some());
        assert!(report
            .losses
            .iter()
            .any(|loss| loss.code == crate::loss::F3dLossCode::DialectLayerCollision.kind()));
    }
}
