// SPDX-License-Identifier: Apache-2.0
#![cfg_attr(test, allow(clippy::unwrap_used))]
//! Read and write Autodesk Fusion `.f3d` archives.
//!
//! [`F3dCodec`] implements [`cadmpeg_ir::codec::Codec`] and
//! [`cadmpeg_ir::codec::write::Encoder`]. Decoding produces a
//! [`CadIr`] document with B-rep topology, analytic and cached NURBS geometry,
//! body transforms, design and sketch records, construction history, and
//! appearances. Encoding replays an unchanged decoded archive byte for byte,
//! applies supported semantic edits to retained source data, or creates an
//! archive from the supported source-less profile.
//!
//! <!-- generated: capability f3d -->
//! Support: L5 ([ladder](https://github.com/cadmpeg/cadmpeg/blob/main/docs/format-support.md#fusion-360-f3d)).
//! <!-- /generated: capability f3d -->
//!
//! # Decode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::{CodecBackend, Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! for loss in &result.report().losses {
//!     eprintln!("{:?}: {}", loss.severity, loss.message);
//! }
//! # Ok(())
//! # }
//! ```
//!
//! [`Codec::inspect`](cadmpeg_ir::Codec::inspect) classifies the ZIP entries and reads ASM B-rep headers
//! without building geometry. `DecodeDepth::Container` provides the
//! corresponding metadata-only `CadIr`.
//!
//! # Encode
//!
//! ```no_run
//! use cadmpeg_codec_f3d::F3dCodec;
//! use cadmpeg_ir::codec::write::target::TargetRequest;
//! use cadmpeg_ir::codec::write::Encoder;
//! use cadmpeg_ir::{CodecBackend, Codec, DecodeOptions};
//! use std::fs::File;
//!
//! # fn main() -> Result<(), Box<dyn std::error::Error>> {
//! let mut input = File::open("part.f3d")?;
//! let result = F3dCodec.decode(&mut input, &DecodeOptions::default())?;
//! let (ir, _report, fidelity) = result.into_parts();
//! // Edit supported fields in `ir`.
//! let mut output = File::create("part-edited.f3d")?;
//! F3dCodec
//!     .plan(
//!         cadmpeg_ir::codec::write::EncodeInput::new(&ir, Some(&fidelity)),
//!         TargetRequest::Inherit,
//!     )?
//!     .write_to(&mut output)?;
//! # Ok(())
//! # }
//! ```
//!
//! Encoding with the retained `source_fidelity` sidecar replays or patches the
//! source archive. Omitting `fidelity` writes the source-less profile.
//!
//! # Data flow
//!
//! The Design body map selects every B-rep blob contributing bodies to the
//! document model; `.smb` and `.smbh` extensions do not choose either model or
//! history role. [`container`] locates history streams by the ASM header flag.
//! When a Design stream is absent or has no body-map carriers, every binary
//! BREP basename in the active asset is a model candidate. [`cadmpeg_asm::sab`] frames each selected
//! active record slice.
//! [`brep`] builds each topology chain from bodies through vertices and points,
//! while [`cadmpeg_asm::nurbs`] decodes cached spline carriers.
//! [`design`], [`history`], and [`materials`] populate source-native records and
//! appearance bindings. [`f3z`] merges multi-document archives into one model.
//!
//! ASM model-space lengths become millimetres. Directions, ratios, angles,
//! knots, weights, and UV parameters retain their native scale.
//!
//! Inspect [`cadmpeg_ir::report::decode::DecodeReport::losses`] before consuming a
//! decode. A stream that cannot produce geometry returns container metadata,
//! retained source data, and blocking geometry and topology losses. Referenced
//! carrier bytes needed for passthrough remain available as
//! [`cadmpeg_ir::unknown::UnknownRecord`] values.

mod act;
mod brep;
mod bytes;
mod container;
mod decode;
mod design;
mod dialect;
mod f3z;
pub use f3z::{
    observe_f3z_members, F3zDecodedMember, F3zMember, F3zMemberKind, F3zMemberObserver, F3zPhase,
    F3zSession,
};
mod history;
mod history_records;
mod ids;
/// Byte-offset constants generated from `docs/layouts/f3d.toml`.
mod layout;
mod loss;
mod manifest;
mod materials;
mod metastream;
mod native;
mod paramesh;
mod records;
mod report;
mod repro;
mod tsm;
mod validate;
mod writer;
mod xref;
mod zip_write;

use cadmpeg_core::bytes::contains;
use cadmpeg_core::decode::{DecodeContext, DecodeDepth, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::write::{
    target::{Catalog, ResolvedWrite},
    DepthNeed, EncodeInput, EncoderBackend, ExportBody,
};
use cadmpeg_ir::codec::{CodecBackend, Confidence, Decoded, FormatId};
use cadmpeg_ir::document::CadIr;
use cadmpeg_ir::hash::DOCUMENT_LOCAL_DIGEST_ATTRIBUTE;
use cadmpeg_ir::ContainerSummary;
use std::io::Write;

#[cfg(test)]
use crate::loss::F3dLossCode;

/// The ZIP local-file-header magic.
const ZIP_MAGIC: &[u8] = b"PK\x03\x04";

/// The Autodesk Fusion `.f3d` container codec.
#[derive(Debug, Default, Clone, Copy)]
pub struct F3dCodec;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PreservedWritePath {
    Patched,
    VerbatimReplay,
}

impl F3dCodec {
    /// Replay retained source bytes when the document baseline still matches;
    /// otherwise patch. Absent baseline: refuse. Returns which branch ran.
    fn write_preserved_bytes(
        ir: &CadIr,
        data: &[u8],
        writer: &mut dyn Write,
        notes: &mut Vec<String>,
    ) -> Result<PreservedWritePath, CodecError> {
        let expected = ir
            .source
            .as_ref()
            .and_then(|source| source.attributes.get(DOCUMENT_LOCAL_DIGEST_ATTRIBUTE))
            .ok_or_else(|| CodecError::NotImplemented("IR has no F3D document baseline".into()))?;
        if decode::document_local_sha256(ir)? != *expected {
            writer::patch::write_semantic(ir, data, writer, notes)?;
            return Ok(PreservedWritePath::Patched);
        }
        writer.write_all(data)?;
        Ok(PreservedWritePath::VerbatimReplay)
    }
}

impl CodecBackend for F3dCodec {
    fn repro_redactions(
        ir: &CadIr,
    ) -> Result<std::collections::BTreeMap<String, String>, CodecError> {
        repro::redactions(ir)
    }

    const FORMAT: FormatId = FormatId::new(dialect::FORMAT);

    fn validate_native(
        ctx: &DecodeContext<'_>,
        ir: &CadIr,
        _source: Option<&cadmpeg_ir::SourceFidelity>,
    ) -> Result<Vec<cadmpeg_ir::report::check::Finding>, CodecError> {
        validate::validate_native(ctx, ir)
    }

    fn detect_impl(
        &self,
        ctx: &cadmpeg_core::decode::DecodeContext<'_>,
        prefix: cadmpeg_core::decode::View<'_>,
    ) -> Result<Confidence, cadmpeg_core::CodecError> {
        let prefix = prefix.window();
        if !prefix.starts_with(ZIP_MAGIC) {
            if cadmpeg_container::ArchiveSnapshot::has_footer(prefix) {
                match cadmpeg_container::ArchiveSnapshot::contains_matching_name(
                    ctx,
                    cadmpeg_core::decode::View::over_retained(prefix),
                    |name| {
                        container::DETECT_MARKERS
                            .iter()
                            .chain(container::F3Z_DETECT_MARKERS)
                            .any(|marker| contains(name.as_bytes(), marker))
                    },
                ) {
                    Ok(true) => return Ok(Confidence::Medium),
                    Err(error @ CodecError::ResourceLimit(_)) => return Err(error),
                    Ok(false) | Err(_) => {}
                }
            }
            return Ok(Confidence::No);
        }
        for marker in container::DETECT_MARKERS
            .iter()
            .chain(container::F3Z_DETECT_MARKERS)
        {
            if contains(prefix, marker) {
                return Ok(Confidence::High);
            }
        }
        Ok(Confidence::Low)
    }

    fn inspect_impl(
        &self,
        ctx: &DecodeContext<'_>,
        root: View<'_>,
    ) -> Result<ContainerSummary, CodecError> {
        let scan = container::scan(ctx, root)?;
        match &scan.kind {
            container::F3dContainerKind::MultiDocument { .. } => f3z::inspect(ctx, &scan),
            container::F3dContainerKind::Document { .. } => report::build_inspection_summary(&scan),
        }
    }

    fn decode_impl(&self, ctx: &DecodeContext<'_>, root: View<'_>) -> Result<Decoded, CodecError> {
        decode::decode(ctx, root)
    }
}

impl EncoderBackend for F3dCodec {
    const FORMAT: FormatId = <Self as CodecBackend>::FORMAT;
    /// Patches the retained source, so the history and design records it rewrites
    /// must be present.
    const NEEDS: DepthNeed = DepthNeed::AtLeast(DecodeDepth::Full);
    type Target = Catalog;
    const TARGET: Catalog = Catalog::new(dialect::TARGETS, Some(0));

    fn plan_resolved(
        &self,
        input: EncodeInput<'_>,
        target: ResolvedWrite<'_>,
    ) -> Result<ExportBody, CodecError> {
        writer::target::plan(input, &target)
    }
}

#[cfg(test)]
mod golden_tests;
#[cfg(test)]
mod integration_tests;
#[cfg(test)]
mod test_support;
