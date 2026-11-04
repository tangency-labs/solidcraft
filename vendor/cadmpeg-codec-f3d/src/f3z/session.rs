// SPDX-License-Identifier: Apache-2.0
//! Independent member decoding and deterministic archive composition.

use std::collections::BTreeMap;
use std::sync::Arc;

use cadmpeg_core::decode::{DecodeArena, DecodeCheckpoint, DecodeContext, DecodeTrace, View};
use cadmpeg_core::CodecError;
use cadmpeg_ir::codec::{DecodeFailure, DecodeResult, Decoded, FormatId};

use super::archive::{self, ClassifiedMember};
use crate::container::{ContainerScan, F3dContainerKind};

/// The role of one archive entry in a multi-document session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum F3zMemberKind {
    /// A logical model document.
    Design,
    /// A document that places external model documents.
    PlacementOnly,
    /// A drawing entry.
    Drawing,
    /// An entry excluded from model decoding, with its reason.
    Skipped(String),
}

/// Stable member identity and outgoing archive references.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct F3zMember {
    id: String,
    kind: F3zMemberKind,
    bytes: u64,
    references: Vec<String>,
    reference_error: Option<String>,
}

impl F3zMember {
    /// The exact archive path, stable across opens and decode schedules.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// The member's role or exclusion reason.
    pub fn kind(&self) -> &F3zMemberKind {
        &self.kind
    }
    /// The uncompressed member length from the archive directory.
    pub fn byte_size(&self) -> u64 {
        self.bytes
    }
    /// Outgoing member paths in reference order. Missing targets keep their path.
    pub fn references(&self) -> &[String] {
        &self.references
    }
    /// A reference-metadata error; member decoding supplies its final loss or refusal.
    pub fn reference_error(&self) -> Option<&str> {
        self.reference_error.as_deref()
    }
}

/// An independent member result, including its report body and resource trace.
///
/// A skipped entry has no decoded document. A failed document retains its
/// error so composition can apply the root or external-member error rule.
#[derive(Debug)]
pub struct F3zDecodedMember {
    id: String,
    origin: Arc<()>,
    pub(super) decoded: Option<Result<Decoded, CodecError>>,
    operation_poses: OperationPoses,
    trace: DecodeTrace,
}

/// Definition-local operation poses collected from one member document.
pub(super) type OperationPoses = BTreeMap<String, crate::xref::operation_pose::OperationPose>;

impl F3zDecodedMember {
    /// The stable archive member path.
    pub fn id(&self) -> &str {
        &self.id
    }
    /// Deterministic payload estimate, in bytes, excluding allocator overhead.
    ///
    /// This is the compact JSON length of `(ir, (transfer, coverage, losses,
    /// notes, transfer_ledger), source_fidelity)`, plus the UTF-8 member ID
    /// length and compact trace storage on this target. A failed
    /// result uses its error text length; a skipped result has no payload.
    /// It is an estimate of retained storage, not an RSS limit.
    pub fn retained_bytes_estimate(&self) -> Result<u64, CodecError> {
        let mut bytes = memory::ByteCounter::default();
        if let Some(decoded) = &self.decoded {
            match decoded {
                Ok(decoded) => memory::count_decoded(&mut bytes, decoded)?,
                Err(error) => bytes.add(error.to_string().len())?,
            }
        }
        bytes.add(self.id.len())?;
        bytes.add_u64(self.trace.retained_bytes_estimate())?;
        Ok(bytes.total())
    }

    /// The member document and report, its decode error, or no result for a skipped entry.
    pub fn decoded(&self) -> Option<Result<&Decoded, &CodecError>> {
        self.decoded.as_ref().map(Result::as_ref)
    }
}

/// A scanned F3Z archive with independent member decodes.
///
/// Open and classification charge the caller's context once. Each member
/// starts with a private copy of the post-open resource counters and a private
/// scan cache. Composition charges reachable member decodes at their original
/// traversal positions. Each member path decodes at most once: later
/// references reuse its definition, charged per reference, or report its
/// failure again. Arrival order does not affect
/// composition.
///
/// Member traces are accepted when their charges fit the cumulative counters
/// and each observed optional-enrichment threshold has the same result.
/// Composition rebinds member-local derived spaces to fresh IDs in traversal
/// order. A member with a formatted address-space diagnostic is executed again
/// when its spaces move. A refused trace or changed decision also executes the
/// member decoder at its traversal position. This preserves the cumulative
/// budget and exact refusal site.
/// Unreachable and skipped entries add no member-decode charges.
///
/// The input and expansion arena must outlive this session. The session is
/// `Send + Sync`; each decode uses a new local arena and shares no mutable cache.
pub struct F3zSession<'a> {
    pub(super) scan: ContainerScan<'a>,
    pub(super) archive: archive::ArchiveSession,
    pub(super) model_root: String,
    pub(super) omitted_drawing_root: Option<String>,
    checkpoint: DecodeCheckpoint,
    members: Vec<F3zMember>,
    origin: Arc<()>,
    #[cfg(test)]
    reexecutions: std::sync::atomic::AtomicUsize,
    #[cfg(test)]
    require_original_spaces: std::sync::atomic::AtomicBool,
}

impl crate::F3dCodec {
    /// Opens an F3Z directory and classifies its members without decoding models.
    /// The context limits the open; model expansions use temporary arenas.
    pub fn open_archive<'a>(
        &self,
        ctx: &DecodeContext<'a>,
        root: View<'a>,
    ) -> Result<F3zSession<'a>, CodecError> {
        F3zSession::open_scanned(ctx, super::observer::scan(ctx, root)?)
    }
}

impl<'a> F3zSession<'a> {
    pub(super) fn open_scanned(
        ctx: &DecodeContext<'a>,
        scan: ContainerScan<'a>,
    ) -> Result<Self, CodecError> {
        if !matches!(scan.kind, F3dContainerKind::MultiDocument { .. }) {
            return Err(CodecError::WrongFormat(
                "archive session requires F3Z".into(),
            ));
        }
        let (model_root, omitted_drawing_root, archive) =
            super::observer::phase(super::observer::F3zPhase::Classify, || {
                let (model_root, omitted_drawing_root) = archive::model_root(ctx, &scan)?;
                let archive = archive::classify_members(ctx, &scan, true)?;
                Ok((model_root, omitted_drawing_root, archive))
            })?;
        let (checkpoint, mut members) =
            super::observer::phase(super::observer::F3zPhase::SessionOpen, || {
                archive.member_checkpoint(&model_root)?;
                let checkpoint = ctx.checkpoint();
                let mut members = Vec::new();
                for (entry, &bytes) in scan.entries.iter().zip(&scan.archive_member_sizes) {
                    let mut references = Vec::new();
                    let mut reference_error = None;
                    let kind = match archive.members.get(&entry.name) {
                        Some(ClassifiedMember::Scanned { metadata, .. }) => {
                            let mut kind = F3zMemberKind::Design;
                            if let Some(metadata) = metadata {
                                references.extend(metadata.references.iter().cloned());
                                reference_error.clone_from(&metadata.error);
                                if metadata.placement_only && metadata.error.is_none() {
                                    kind = F3zMemberKind::PlacementOnly;
                                }
                            }
                            kind
                        }
                        Some(ClassifiedMember::Unreadable(reason)) => {
                            F3zMemberKind::Skipped(reason.clone())
                        }
                        None if entry.name.to_ascii_lowercase().ends_with(".f2d") => {
                            F3zMemberKind::Drawing
                        }
                        None => F3zMemberKind::Skipped("not an F3D model document".into()),
                    };
                    members.push(F3zMember {
                        id: entry.name.clone(),
                        kind,
                        bytes,
                        references,
                        reference_error,
                    });
                }
                Ok((checkpoint, members))
            })?;
        members.sort_by(|a, b| (a.id != model_root, &a.id).cmp(&(b.id != model_root, &b.id)));
        Ok(Self {
            scan,
            archive,
            model_root,
            omitted_drawing_root,
            checkpoint,
            members,
            origin: Arc::new(()),
            #[cfg(test)]
            reexecutions: std::sync::atomic::AtomicUsize::new(0),
            #[cfg(test)]
            require_original_spaces: std::sync::atomic::AtomicBool::new(false),
        })
    }

    /// Members in deterministic list order, with the model root first.
    pub fn members(&self) -> &[F3zMember] {
        &self.members
    }

    /// Largest declared single-member byte working estimate available after open.
    ///
    /// Each estimate adds the model archive's uncompressed member length to
    /// the sum of its inner ZIP entries' declared uncompressed lengths. Parsed
    /// records, output, allocator overhead and unlisted expansion are excluded.
    /// Returns `None` if a readable member's sum exceeds `u64`.
    /// Callers can gate later parallel decode by this estimate plus retained results.
    pub fn largest_member_working_bytes_estimate(&self) -> Option<u64> {
        let mut largest = 0;
        for member in self.archive.members.values() {
            if let ClassifiedMember::Scanned { working_bytes, .. } = member {
                largest = largest.max((*working_bytes)?);
            }
        }
        Some(largest)
    }

    /// Decodes one listed member with private counters and a private scan cache.
    /// Errors from model decoding are held in the returned member result.
    pub fn decode_member(&self, id: &str) -> Result<F3zDecodedMember, CodecError> {
        if !self.members.iter().any(|member| member.id == id) {
            return Err(CodecError::malformed(format_args!(
                "unknown F3Z member {id}"
            )));
        }
        let arena = DecodeArena::new();
        let ctx = self.checkpoint.resume(&arena);
        let mut operation_poses = OperationPoses::new();
        let decoded = match self.archive.members.get(id) {
            Some(ClassifiedMember::Scanned { .. }) => {
                Some(self.decode_in_context(&ctx, id).map(|(decoded, poses)| {
                    operation_poses = poses;
                    decoded
                }))
            }
            _ => None,
        };
        Ok(F3zDecodedMember {
            id: id.to_owned(),
            origin: Arc::clone(&self.origin),
            decoded,
            operation_poses,
            trace: ctx.take_trace(),
        })
    }

    /// Composes one result for every listed entry, in any arrival order.
    /// Missing, duplicate, and foreign-session results are refused.
    pub fn compose(
        &self,
        members: impl IntoIterator<Item = F3zDecodedMember>,
    ) -> Result<DecodeResult, DecodeFailure> {
        let arena = DecodeArena::new();
        let ctx = self.checkpoint.resume(&arena);
        ctx.take_trace();
        let result = self
            .compose_into(&ctx, members)
            .map_err(DecodeFailure::from)
            .and_then(|decoded| DecodeResult::from_decoded(&ctx, decoded, FormatId::new("f3d")));
        ctx.finish_session()?;
        result
    }

    pub(super) fn compose_into(
        &self,
        ctx: &DecodeContext<'_>,
        members: impl IntoIterator<Item = F3zDecodedMember>,
    ) -> Result<Decoded, CodecError> {
        let mut results = BTreeMap::new();
        for member in members {
            if !Arc::ptr_eq(&member.origin, &self.origin) || results.contains_key(&member.id) {
                return Err(CodecError::malformed(
                    "duplicate or foreign F3Z member result",
                ));
            }
            results.insert(member.id.clone(), member);
        }
        if results.len() != self.members.len()
            || self
                .members
                .iter()
                .any(|member| !results.contains_key(&member.id))
        {
            return Err(CodecError::malformed(
                "F3Z composition requires every listed member",
            ));
        }
        super::compose(ctx, self, &mut results)
    }

    pub(super) fn take_member(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
        results: &mut BTreeMap<String, F3zDecodedMember>,
    ) -> Result<(Decoded, OperationPoses), CodecError> {
        if let Some(result) = results.remove(id) {
            if let Some(mut decoded) = result.decoded {
                if ctx.accept_trace_rebinding(&result.trace, |spaces| {
                    #[cfg(test)]
                    if self
                        .require_original_spaces
                        .load(std::sync::atomic::Ordering::Relaxed)
                    {
                        return spaces.is_identity();
                    }
                    rebinding::rebind_member(&mut decoded, spaces)
                }) {
                    return decoded.map(|decoded| (decoded, result.operation_poses));
                }
            }
        }
        #[cfg(test)]
        self.reexecutions
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        self.decode_in_context(ctx, id)
    }

    fn decode_in_context(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<(Decoded, OperationPoses), CodecError> {
        super::observer::observed(id, || self.decode_member_scan(ctx, id))
    }

    fn decode_member_scan(
        &self,
        ctx: &DecodeContext<'_>,
        id: &str,
    ) -> Result<(Decoded, OperationPoses), CodecError> {
        self.archive.with_member_scan(&self.scan, id, |scan| {
            let arena = DecodeArena::new();
            ctx.with_arena(&arena, |local| {
                crate::decode::decode_archive_member(local, scan, &self.archive.layers).map(
                    |mut decoded| {
                        let poses = std::mem::take(&mut decoded.operation_poses);
                        (decoded.into_decoded(), poses)
                    },
                )
            })
        })
    }
}

#[cfg(test)]
impl F3zSession<'_> {
    pub(crate) fn require_original_spaces_for_test(&self, required: bool) {
        self.require_original_spaces
            .store(required, std::sync::atomic::Ordering::Relaxed);
    }

    /// Total member decodes executed by composition calls on this archive.
    pub(crate) fn composition_reexecutions(&self) -> usize {
        self.reexecutions.load(std::sync::atomic::Ordering::Relaxed)
    }

    pub(crate) fn compose_in_traversal_for_test(
        &self,
        ctx: &DecodeContext<'_>,
    ) -> Result<Decoded, CodecError> {
        super::compose(ctx, self, &mut BTreeMap::new())
    }
}

#[cfg(test)]
mod tests;

mod memory;

mod rebinding;
