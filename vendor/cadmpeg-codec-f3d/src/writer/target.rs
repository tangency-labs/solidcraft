// SPDX-License-Identifier: Apache-2.0
//! F3D target resolution, preservation dispatch, and export reporting.

use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::ResolvedWrite, ArenaCoverage, Consumption, EncodeInput, ExportBody, PatchConsumption,
    WritePath,
};
use cadmpeg_ir::document::CadIr;

use crate::loss::F3dLossCode;
use crate::{ids, F3dCodec, PreservedWritePath};

/// Plan the export the resolved request names.
///
/// `Explicit(id)` refuses an id outside the synthesis catalog, and is
/// otherwise the replay law's compare: preserving the retained archive is
/// eligible exactly when `id` is the source's dialect.
///
/// `Inherit` asks for preservation instead: a valid retained baseline
/// replays or patches whatever dialect the source is, the F3Z
/// multi-document row and the recovery row included, which the generator
/// could never synthesize. Where the baseline is not usable, `Inherit`
/// synthesizes the source's own dialect, and refuses when that dialect is
/// not a target. There is no fall-through to the catalog default: a
/// same-format conversion never silently changes what the file is.
///
/// The catalog default supplies the target only when there is nothing to
/// inherit: the document has no source, or a source of another format.
///
/// The sealed encoder stamps the target identity and maps
/// [`Consumption`] onto the report; this body states only what was done.
pub(crate) fn plan(
    input: EncodeInput<'_>,
    target: &ResolvedWrite<'_>,
) -> Result<ExportBody, CodecError> {
    if target.entry().is_none() {
        return match preserve(input)? {
            Preservation::Written {
                bytes,
                write_path,
                notes,
            } => Ok(preserved_body(input.ir, write_path, bytes, notes)),
            Preservation::SourceImageUnavailable => Err(target.unavailable(
                "its retained source image is unavailable for preservation and the generator \
                 cannot synthesize it",
            )),
        };
    }
    if target.preserves_source() {
        return match preserve(input)? {
            Preservation::Written {
                bytes,
                write_path,
                notes,
            } => Ok(preserved_body(input.ir, write_path, bytes, notes)),
            Preservation::SourceImageUnavailable => {
                synthesized_body(input, SynthesisCause::SourceImageUnavailable)
            }
        };
    }
    let cause = if let Some(message) = target.displacement_message() {
        SynthesisCause::Displaced(message)
    } else if target.source_preservation_eligible()
        && input
            .fidelity
            .and_then(|fidelity| fidelity.retained_record(ids::FILE_SOURCE_IMAGE_ID))
            .is_none()
    {
        SynthesisCause::SourceImageUnavailable
    } else {
        SynthesisCause::Fresh
    };
    synthesized_body(input, cause)
}

enum Preservation {
    Written {
        bytes: Vec<u8>,
        write_path: PreservedWritePath,
        notes: Vec<String>,
    },
    /// The only reason preservation is ever declined.
    SourceImageUnavailable,
}

fn preserve(input: EncodeInput<'_>) -> Result<Preservation, CodecError> {
    let Some(record) = input
        .fidelity
        .and_then(|sidecar| sidecar.retained_record(ids::FILE_SOURCE_IMAGE_ID))
    else {
        return Ok(Preservation::SourceImageUnavailable);
    };
    let Some(data) = record.data() else {
        return Err(CodecError::Malformed(
            "retained F3D source image has no bytes".into(),
        ));
    };
    let mut bytes = Vec::new();
    let mut notes = Vec::new();
    let write_path = F3dCodec::write_preserved_bytes(input.ir, data, &mut bytes, &mut notes)?;
    Ok(Preservation::Written {
        bytes,
        write_path,
        notes,
    })
}

fn preserved_body(
    ir: &CadIr,
    write_path: PreservedWritePath,
    bytes: Vec<u8>,
    notes: Vec<String>,
) -> ExportBody {
    let write_path = match write_path {
        PreservedWritePath::Patched => WritePath::Patched {
            consumption: PatchConsumption::Replayed,
        },
        PreservedWritePath::VerbatimReplay => WritePath::VerbatimReplay,
    };
    // Replay runs only while the document digest matches its decode baseline,
    // and the patcher refuses unless the patched baseline digests equal to the
    // target document, so every arena is carried.
    let mut body = body(ir, write_path, ArenaCoverage::Complete, Vec::new(), bytes);
    body.notes.extend(notes);
    body
}

enum SynthesisCause {
    Fresh,
    Displaced(String),
    SourceImageUnavailable,
}

impl SynthesisCause {
    fn into_fidelity(self) -> (Consumption, Option<cadmpeg_ir::report::loss::LossNote>) {
        match self {
            Self::Fresh => (Consumption::NotConsumed, None),
            Self::Displaced(message) => (
                Consumption::NotConsumed,
                Some(F3dLossCode::SourceDialectDisplaced.note(message)),
            ),
            Self::SourceImageUnavailable => (
                Consumption::Degraded {
                    reason: "preserved F3D source image is unavailable".into(),
                },
                Some(
                    F3dLossCode::SourcePreservedImageUnavailable
                        .note("preserved F3D source image is unavailable; regenerated from IR"),
                ),
            ),
        }
    }
}

fn synthesized_body(
    input: EncodeInput<'_>,
    cause: SynthesisCause,
) -> Result<ExportBody, CodecError> {
    let mut bytes = Vec::new();
    let coverage = super::generate::write_new(input.ir, &mut bytes)?;
    let (consumption, loss) = cause.into_fidelity();
    let mut losses: Vec<_> = loss.into_iter().collect();
    for appearance in &input.ir.model.appearances {
        let written: &[&str] = match appearance.schema.as_deref().unwrap_or("GenericSchema") {
            "GenericSchema" => &["reflectivity_at_0deg", "refraction_index"],
            "PrismOpaqueSchema" | "PrismMetalSchema" => &["surface_roughness"],
            "PrismTransparentSchema" => &["refraction_index"],
            _ => &[],
        };
        let omitted = appearance
            .properties
            .keys()
            .filter(|key| !written.contains(&key.as_str()))
            .collect::<Vec<_>>();
        if !omitted.is_empty() {
            losses.push(F3dLossCode::WriterAppearancePropertiesOmitted.note(format!(
                    "{} appearance property record(s) of {} were not written: {}",
                    omitted.len(),
                    appearance.id,
                    omitted
                        .iter()
                        .map(|key| key.as_str())
                        .collect::<Vec<_>>()
                        .join(", ")
                )));
        }
    }
    Ok(body(
        input.ir,
        WritePath::Synthesized { consumption },
        ArenaCoverage::Declared(coverage),
        losses,
        bytes,
    ))
}

fn body(
    ir: &CadIr,
    write_path: WritePath,
    coverage: ArenaCoverage,
    losses: Vec<cadmpeg_ir::report::loss::LossNote>,
    bytes: Vec<u8>,
) -> ExportBody {
    let path_note = match &write_path {
        WritePath::VerbatimReplay => "preserved source container replayed verbatim",
        WritePath::Patched { .. } => "preserved source container replayed with semantic patches",
        WritePath::Synthesized { .. } => "source container regenerated from IR",
    };
    ExportBody {
        bytes,
        census: cadmpeg_ir::report::export::EntityCensus {
            basis: cadmpeg_ir::report::export::CensusBasis::IrArenas,
            counts: ir.census(),
        },
        write_path,
        coverage,
        losses,
        notes: vec![
            path_note.into(),
            "entity counts are derived from the IR".into(),
        ],
    }
}
