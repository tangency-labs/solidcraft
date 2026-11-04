// SPDX-License-Identifier: Apache-2.0
//! Decode the ASM construction-history partition after the active model slice.
//!
//! [`decode`] reads `delta_state` headers, bulletin-board entity changes, and
//! history records while retaining source bytes for records without typed
//! semantics.

#![cfg_attr(
    test,
    allow(clippy::cloned_ref_to_slice_refs, clippy::default_trait_access)
)]

use cadmpeg_core::decode::u64_from_index;

pub(crate) mod binding;
mod blend_spine;

mod body_candidates;
mod body_index;
mod cache;
mod combine_selection;
pub(crate) mod compact_lineage;
mod delete_face;
pub(crate) mod edge_flange;
pub(crate) mod face_admission;
mod face_recipe;
pub(crate) mod fillet_blends;
mod hole_support;
mod loop_index;
mod move_face;
mod output_chain;
mod recipe_index;
mod reference_edge;
pub(crate) mod selection;
mod shell_walls;
pub(crate) mod stored_bodies;
mod surface_rows;
pub(crate) mod thread_face;
mod topology_cache;
mod treatment_index;

use body_candidates::BodyCandidates;
use body_index::CompleteBodyIndex;
use cache::SnapshotCache;
use loop_index::LoopIndex;
use recipe_index::RecipeTopologyCache;
use topology_cache::{TopologyQueryCache, VertexBoundaryIndex};
use treatment_index::TreatmentCache;

use crate::bytes::int_at;
use crate::history_records::AsmHistoricalTopologyDelta;
use crate::history_records::{
    AsmBulletinBoard, AsmDeltaState, AsmEntityChange, AsmEntityChangeKind,
    AsmHistoricalCarrierBinding, AsmHistoricalCoedge, AsmHistoricalCylinder, AsmHistoricalEdge,
    AsmHistoricalOptionalCarrierBinding, AsmHistoricalPoint, AsmHistoricalRelation,
    AsmHistoricalRow, AsmHistoricalTopology, AsmHistory, AsmHistoryRecord, AsmPreamble,
};
use crate::records::topology::{
    body_recipe::AsmHistoricalEntityKind, extrude_selection::DesignOperandRole,
};
use cadmpeg_asm::kernel_header::RefWidth;
use cadmpeg_ir::geometry::SolvedSurfaceGeometry;
use std::collections::{BTreeMap, BTreeSet, HashMap, HashSet};

const EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7: f64 = 1.0e-7;
const EPS_HISTORY_PARALLEL_DIRECTIONS_E7: f64 = 1.0e-7;
const EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9: f64 = 1.0e-9;
const EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12: f64 = 1.0e-12;
const EPS_HISTORY_SAME_AXIS_LINE_E9: f64 = 1.0e-9;
const EPS_HISTORY_SAME_AXIS_LINE_E8: f64 = 1.0e-8;
const EPS_HISTORY_BIND_MIRROR_SELECTION_PLANES_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E9: f64 = 1.0e-9;
const EPS_HISTORY_MIRROR_PLANES_COINCIDENT_E8: f64 = 1.0e-8;
const EPS_HISTORY_HISTORICAL_LOOP_PLANE_E9: f64 = 1.0e-9;
const EPS_HISTORY_BODY_BBOX_GROWTH_E3: f64 = 1.0e-3;
const EPS_HISTORY_BODY_BBOX_UNCHANGED_E4: f64 = 1.0e-4;
const EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2: f64 = 1.0e-2;
const EPS_HISTORY_EDGE_SEGMENT_LEN_SQ_E12: f64 = 1.0e-12;
const EPS_HISTORY_EDGE_COLLINEAR_DIST_SQ_E6: f64 = 1.0e-6;
const EPS_HISTORY_LINE_AXIS_DIR_NORM_SQ_E12: f64 = 1.0e-12;
const EPS_HISTORY_LINE_AXIS_CROSS_SQ_E5: f64 = 1.0e-5;

const DELTA: &[u8] = b"\x11\x0d\x0bdelta_state";
const PREAMBLE: &[u8] = b"\x0d\x0ehistory_stream";
/// Relative tolerance for matching independently decoded millimetre point carriers.
const WORK_POINT_POSITION_TOLERANCE: f64 = 1.0e-9;

pub(crate) fn graph_is_coherent(history: &AsmHistory) -> bool {
    if history.states().is_empty() {
        return false;
    }
    let mut by_index = HashMap::new();
    for state in history.states() {
        by_index.insert(state.node_index, state);
    }
    if by_index.len() != history.states().len()
        || history
            .states()
            .iter()
            .any(|state| state.node_index < 0 || state.parent != history.id)
    {
        return false;
    }
    let mut heads = history
        .states()
        .iter()
        .filter(|state| state.previous_ref.is_none());
    let head = heads.next();
    let tails = history
        .states()
        .iter()
        .filter(|state| state.next_ref.is_none())
        .count();
    if head.is_none() || heads.next().is_some() || tails != 1 {
        return false;
    }
    let Some(head) = head else {
        return false;
    };
    // The preamble's first field need not name the head state (asm-open-items HS-01).
    if history
        .preamble
        .is_some_and(|preamble| preamble.history_entry_count < 0)
    {
        return false;
    }
    let mut visited = HashSet::new();
    let mut previous = None;
    let mut current = Some(head.node_index);
    while let Some(index) = current {
        let Some(state) = by_index.get(&index) else {
            return false;
        };
        if visited.contains(&index) || state.previous_ref != previous {
            return false;
        }
        visited.insert(index);
        if state.version_flag != 1 || state.state_flag != 0 {
            return false;
        }
        for board in &state.bulletin_boards {
            if board.parent != state.id
                || board.changes.iter().any(|change| change.parent != board.id)
            {
                return false;
            }
        }
        if state
            .records
            .iter()
            .any(|record| record.parent != state.id || record.raw_bytes.is_empty())
        {
            return false;
        }
        previous = Some(index);
        current = state.next_ref;
    }
    visited.len() == history.states().len()
}

/// Decode the construction-history tail of an ASM stream: every `delta_state`
/// record ([spec §3.2](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#32-delta_state-records)) from `bytes`, each with its `BulletinBoard` chain of
/// per-entity insert/delete/update changes and the raw history-entity records
/// framed between it and the next `delta_state`. `stream` is the source ZIP
/// entry name, recorded in each decoded item's provenance. Returns `None` when
/// `bytes` carries no `delta_state` record (the stream is a construction
/// snapshot with no history tail) or a malformed history body. `width` is the
/// stream's integer/ref width (4 for `BinaryFile4`, 8 for `BinaryFile8`).
/// `remaining_retained_bytes` is the session's remaining retained-byte allowance,
/// less the estimates reserved for earlier history streams. A history whose
/// estimate does not fit keeps only its state headers. `binding_work` bounds
/// the topology binding of the history; a history that exhausts it has no
/// historical topology.
pub(crate) fn decode(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
    remaining_retained_bytes: &mut u64,
    binding_work: &cadmpeg_core::decode::WorkBudget,
) -> Result<Option<AsmHistory>, cadmpeg_core::CodecError> {
    let framing = HistoryFraming::scan(bytes, stream);
    let mut states = Vec::new();
    for (ordinal, &offset) in framing.delta_offsets.iter().enumerate() {
        let header = match read_delta_state_header(bytes, offset, width) {
            DeltaStateHeaderRead::Framed(header) => header,
            DeltaStateHeaderRead::Unframed => continue,
            DeltaStateHeaderRead::Invalid => return Ok(None),
        };
        let state_record_id = header.record_id(stream);
        let Some((bulletin_boards, body_end)) = decode_bulletin_boards(
            bytes,
            header.body_start,
            stream,
            offset,
            &state_record_id,
            width,
        ) else {
            return Ok(None);
        };
        let records = decode_history_records(
            ctx,
            bytes,
            body_end,
            framing.delta_offsets.get(ordinal + 1).copied(),
            stream,
            &state_record_id,
            width,
        )?;
        states.push(header.into_state(
            state_record_id,
            &framing.history_id,
            bulletin_boards,
            records,
        ));
    }
    bind_snapshot_revision_ids(&mut states)?;
    let binding_budget_exceeded = admit_history_binding(ctx, &states, remaining_retained_bytes)?
        == HistoryBindingAdmission::Skipped;
    if binding_budget_exceeded {
        for state in &mut states {
            state.bulletin_boards = Vec::new();
            state.records = Vec::new();
        }
    } else {
        bind_complete_record_tables(ctx, &mut states, bytes, width, binding_work)?;
    }
    if states.is_empty() {
        return Ok(None);
    }
    Ok(Some(framing.into_history(
        bytes,
        width,
        binding_budget_exceeded,
        states,
    )))
}

/// Decode the header-only index of one ASM history stream.
///
/// Each state carries its identity and chain links (state id, node index,
/// previous and next references) and no bulletin boards, record table or
/// bound transition. The index rejects a stream on the same header, framing
/// and bulletin-board conditions as [`decode`]; it frames the boards without
/// retaining them.
pub(crate) fn decode_header_index(
    bytes: &[u8],
    stream: &str,
    width: RefWidth,
) -> Option<AsmHistory> {
    let framing = HistoryFraming::scan(bytes, stream);
    let mut states = Vec::new();
    for &offset in &framing.delta_offsets {
        let header = match read_delta_state_header(bytes, offset, width) {
            DeltaStateHeaderRead::Framed(header) => header,
            DeltaStateHeaderRead::Unframed => continue,
            DeltaStateHeaderRead::Invalid => return None,
        };
        walk_bulletin_boards(bytes, header.body_start, width, |_| {})?;
        states.push(header.into_state(
            header.record_id(stream),
            &framing.history_id,
            Vec::new(),
            Vec::new(),
        ));
    }
    if states.is_empty() {
        return None;
    }
    Some(framing.into_history(bytes, width, false, states))
}

/// Stream-level layout of one ASM history: its preamble and `delta_state` offsets.
struct HistoryFraming {
    preamble_offset: Option<usize>,
    history_offset: usize,
    history_id: String,
    delta_offsets: Vec<usize>,
}

impl HistoryFraming {
    fn scan(bytes: &[u8], stream: &str) -> Self {
        let preamble_offset = bytes
            .windows(PREAMBLE.len())
            .position(|window| window == PREAMBLE);
        let history_offset = preamble_offset.unwrap_or(0);
        let history_id = crate::ids::native_scoped_id(
            stream,
            "asm-history",
            format_args!("{history_offset:010}"),
        );
        let mut delta_offsets = Vec::new();
        let mut search = 0usize;
        while let Some(relative) = bytes[search..]
            .windows(DELTA.len())
            .position(|window| window == DELTA)
        {
            let offset = search + relative;

            delta_offsets.push(offset);
            search = offset + DELTA.len();
        }
        Self {
            preamble_offset,
            history_offset,
            history_id,
            delta_offsets,
        }
    }

    fn into_history(
        self,
        bytes: &[u8],
        width: RefWidth,
        binding_budget_exceeded: bool,
        states: Vec<AsmDeltaState>,
    ) -> AsmHistory {
        let preamble = self
            .preamble_offset
            .and_then(|offset| decode_preamble(bytes, offset + PREAMBLE.len(), width))
            .map(|(stream_size, history_entry_count)| AsmPreamble {
                stream_size,
                history_entry_count,
            });
        AsmHistory::new(
            self.history_id,
            u64_from_index(self.history_offset),
            preamble,
            binding_budget_exceeded,
            states,
        )
    }
}

/// Fixed integer header of one `delta_state` record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct DeltaStateHeader {
    offset: usize,
    state_id: i64,
    version_flag: i64,
    state_flag: i64,
    previous: i64,
    next: i64,
    node_index: i64,
    partner: i64,
    owner_ref: i64,
    /// First byte after the `0x0b` marker that opens the state body.
    body_start: usize,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DeltaStateHeaderRead {
    /// The header is complete and the state body opens with `0x0b`.
    Framed(DeltaStateHeader),
    /// The header is complete and no state body follows it.
    Unframed,
    /// A header integer is missing or mistagged.
    Invalid,
}

fn read_delta_state_header(bytes: &[u8], offset: usize, width: RefWidth) -> DeltaStateHeaderRead {
    let mut position = offset + DELTA.len();
    let Some([state_id, version_flag, state_flag, previous, next, node_index, partner, owner_ref]) =
        (|| {
            Some([
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x04, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
                take_int(bytes, &mut position, 0x0c, width)?,
            ])
        })()
    else {
        return DeltaStateHeaderRead::Invalid;
    };
    if bytes.get(position) != Some(&0x0b) {
        return DeltaStateHeaderRead::Unframed;
    }
    DeltaStateHeaderRead::Framed(DeltaStateHeader {
        offset,
        state_id,
        version_flag,
        state_flag,
        previous,
        next,
        node_index,
        partner,
        owner_ref,
        body_start: position + 1,
    })
}

impl DeltaStateHeader {
    fn record_id(&self, stream: &str) -> String {
        let offset = self.offset;
        crate::ids::native_scoped_id(stream, "asm-delta-state", format_args!("{offset:010}"))
    }

    fn into_state(
        self,
        id: String,
        history_id: &str,
        bulletin_boards: Vec<crate::history_records::AsmBulletinBoard>,
        records: Vec<crate::history_records::AsmHistoryRecord>,
    ) -> AsmDeltaState {
        AsmDeltaState {
            id,
            parent: history_id.to_owned(),
            byte_offset: u64_from_index(self.offset),
            state_id: self.state_id,
            version_flag: self.version_flag,
            state_flag: self.state_flag,
            previous_ref: (self.previous >= 0).then_some(self.previous),
            next_ref: (self.next >= 0).then_some(self.next),
            node_index: self.node_index,
            partner_ref: (self.partner >= 0).then_some(self.partner),
            owner_ref: self.owner_ref,
            bulletin_boards,
            records,
            transition: None,
        }
    }
}

fn bind_snapshot_revision_ids(
    states: &mut [AsmDeltaState],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut old_references = Vec::new();
    for old_reference in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
        .filter_map(super::history_records::AsmEntityChange::old_ref)
    {
        old_references.push(old_reference);
    }
    old_references.sort_unstable_by(Ord::cmp);
    let Some(&first) = old_references.first() else {
        return Ok(());
    };
    let count = i64::try_from(old_references.len()).map_err(|_| {
        cadmpeg_core::CodecError::malformed("F3D ASM old reference count exceeds i64")
    })?;
    let Some(end) = first.checked_add(count) else {
        return Ok(());
    };
    if old_references.iter().copied().ne(first..end) {
        return Ok(());
    }
    let snapshot_record_count = states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .count();
    if snapshot_record_count != old_references.len() {
        return Ok(());
    }
    for (record, revision_id) in states
        .iter_mut()
        .flat_map(|state| &mut state.records)
        .filter(|record| record.name() != "End-of-ASM-data")
        .zip(old_references)
    {
        record.revision_id = Some(revision_id);
    }
    Ok(())
}

fn is_history_boundary_record(record: &AsmHistoryRecord) -> bool {
    matches!(
        record.name(),
        "End-of-ASM-History-Section" | "End-of-ASM-data"
    )
}

pub(crate) fn archived_active_record_count(
    states: &[AsmDeltaState],
) -> Result<Option<usize>, cadmpeg_core::CodecError> {
    let mut archived = Vec::new();
    for revision_id in states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
    {
        archived.push(revision_id);
    }
    archived.sort_unstable_by(Ord::cmp);
    let Some(&active_count) = archived.first() else {
        return Ok(None);
    };
    let count = i64::try_from(archived.len()).map_err(|_| {
        cadmpeg_core::CodecError::malformed("F3D archived revision count exceeds i64")
    })?;
    let Some(end) = active_count.checked_add(count) else {
        return Ok(None);
    };
    if active_count <= 0 || archived.iter().copied().ne(active_count..end) {
        return Ok(None);
    }
    Ok(usize::try_from(active_count).ok())
}

/// Return the active `RecordTable` length for a history that has no archived
/// snapshot. Insert-only chains use the active records themselves as every
/// revision, so their bulletin-board references must cover every non-header
/// slot exactly once.
pub(crate) fn insert_only_active_record_count(states: &[AsmDeltaState]) -> Option<usize> {
    let mut has_boundary_record = false;
    for record in states.iter().flat_map(|state| &state.records) {
        if record.revision_id.is_some() || !is_history_boundary_record(record) {
            return None;
        }
        has_boundary_record = true;
    }
    if !has_boundary_record {
        return None;
    }
    let mut inserted = BTreeSet::new();
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let new_ref = change.new_ref().filter(|_| change.old_ref().is_none())?;
        if new_ref <= 0 || inserted.contains(&new_ref) {
            return None;
        }
        inserted.insert(new_ref);
    }
    let &last = inserted.last()?;
    if inserted.iter().copied().ne(1..=last) {
        return None;
    }
    last.checked_add(1)
        .and_then(|count| usize::try_from(count).ok())
}

/// Estimated native `asm_delta_states` text bytes per changed record slot.
/// History streams with 206 to 385,790 changed slots retain 96.96 to 161.14
/// complete-scope bytes per slot. Rounded up to the next ten bytes.
pub(crate) const HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT: u64 = 170;

/// Whether one history stream's estimated retained bytes fit the session.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HistoryBindingAdmission {
    /// The estimate fits; it is reserved and the history is bound.
    Admitted,
    /// The estimate exceeds the remaining allowance; the history keeps its state
    /// headers only.
    Skipped,
}

/// Validate the chain, then reserve its estimated retained bytes before decoding
/// record tables. A skipped history leaves the allowance unchanged and keeps no
/// bulletin boards, history records or transitions. A chain that does not
/// validate is admitted and stays unbound.
pub(crate) fn admit_history_binding(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    remaining_retained_bytes: &mut u64,
) -> Result<HistoryBindingAdmission, cadmpeg_core::CodecError> {
    let mut archived_ids = Vec::new();
    for revision_id in states
        .iter()
        .flat_map(|state| &state.records)
        .filter_map(|record| record.revision_id)
    {
        archived_ids.push(revision_id);
    }
    archived_ids.sort_unstable_by(Ord::cmp);
    let Some(active_count) =
        archived_active_record_count(states)?.or_else(|| insert_only_active_record_count(states))
    else {
        return Ok(HistoryBindingAdmission::Admitted);
    };
    let Ok(active_count) = i64::try_from(active_count) else {
        return Ok(HistoryBindingAdmission::Admitted);
    };
    let mut changed_slots = Some(0_u64);
    if !walk_entity_versions(states, active_count, &archived_ids, |ordinal, versions| {
        let state = &states[ordinal];
        if state.next_ref.is_none() {
            changed_slots =
                changed_slots.and_then(|total| total.checked_add(u64_from_index(versions.len())));
        }
        for board in &state.bulletin_boards {
            changed_slots = changed_slots
                .and_then(|total| total.checked_add(u64_from_index(board.changes.len())));
        }
    }) {
        return Ok(HistoryBindingAdmission::Admitted);
    }
    let Some(remaining) = changed_slots
        .and_then(|count| count.checked_mul(HISTORY_RETAINED_BYTES_PER_CHANGED_SLOT))
        .and_then(|estimate| {
            ctx.observe_retained_allowance(*remaining_retained_bytes, estimate);
            remaining_retained_bytes.checked_sub(estimate)
        })
    else {
        return Ok(HistoryBindingAdmission::Skipped);
    };
    *remaining_retained_bytes = remaining;
    Ok(HistoryBindingAdmission::Admitted)
}

/// Walk the state chain from its head and give each state's entity-to-record
/// version map to `visit`. Returns false, after a partial walk, when the chain
/// or a bulletin-board change is inconsistent with the archived revisions.
pub(crate) fn walk_entity_versions(
    states: &[AsmDeltaState],
    active_count: i64,
    archived_ids: &[i64],
    mut visit: impl FnMut(usize, &BTreeMap<i64, i64>),
) -> bool {
    let mut by_node = HashMap::new();
    for (ordinal, state) in states.iter().enumerate() {
        by_node.insert(state.node_index, ordinal);
    }
    if by_node.len() != states.len() {
        return false;
    }
    let mut heads = states
        .iter()
        .enumerate()
        .filter(|(_, state)| state.previous_ref.is_none())
        .map(|(ordinal, _)| ordinal);
    let Some(mut ordinal) = heads.next() else {
        return false;
    };
    if heads.next().is_some() {
        return false;
    }

    let mut versions = BTreeMap::new();
    for id in 0..active_count {
        versions.insert(id, id);
    }
    let mut visited = HashSet::new();
    loop {
        let state = &states[ordinal];
        if !visited.insert(state.node_index) {
            return false;
        }
        visit(ordinal, &versions);
        for change in state
            .bulletin_boards
            .iter()
            .flat_map(|board| &board.changes)
        {
            match change.kind {
                AsmEntityChangeKind::Update { old, new } => {
                    if !versions.contains_key(&new) || archived_ids.binary_search(&old).is_err() {
                        return false;
                    }
                    versions.insert(new, old);
                }
                AsmEntityChangeKind::Insert { new } => {
                    if versions.remove(&new).is_none() {
                        return false;
                    }
                }
                AsmEntityChangeKind::Delete { old } => {
                    if versions.contains_key(&old) || archived_ids.binary_search(&old).is_err() {
                        return false;
                    }
                    versions.insert(old, old);
                }
            }
        }
        let Some(next) = state.next_ref else {
            break;
        };
        let Some(&next_ordinal) = by_node.get(&next) else {
            return false;
        };
        ordinal = next_ordinal;
    }
    visited.len() == states.len() && versions.len() == 1 && versions.get(&0) == Some(&0)
}

fn bind_complete_record_tables(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &mut [AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
    work: &cadmpeg_core::decode::WorkBudget,
) -> Result<(), cadmpeg_core::CodecError> {
    binding::bind(ctx, states, bytes, width, work).map(|_| ())
}

pub(crate) fn decode_record_archive(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    states: &[AsmDeltaState],
    bytes: &[u8],
    width: RefWidth,
) -> Result<Option<HistoricalRecordArchive>, cadmpeg_core::CodecError> {
    let Some(start) = cadmpeg_asm::asm_header::record_stream_start(bytes) else {
        return Ok(None);
    };
    let active_limit = cadmpeg_asm::asm_header::solved_record_limit(bytes).unwrap_or(bytes.len());
    let framed = match cadmpeg_asm::sab::frame(ctx, bytes, start, active_limit, width, None) {
        Ok(records) => records,
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => return Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            return Err(error.into_codec_error())
        }
        Err(_) => return Ok(None),
    };

    let insert_only = insert_only_active_record_count(states);
    let archived_count = archived_active_record_count(states)?;
    let Some(active_count) = archived_count.or(insert_only) else {
        return Ok(None);
    };
    if insert_only.is_some() && framed.len() != active_count {
        return Ok(None);
    }
    let Some(active_records) = framed.get(..active_count) else {
        return Ok(None);
    };
    let mut archived_frames = HashMap::new();
    for record in states
        .iter()
        .flat_map(|state| &state.records)
        .filter(|record| record.revision_id.is_some())
    {
        let Some(revision_id) = record.revision_id else {
            return Ok(None);
        };
        let Some(offset) = usize::try_from(record.byte_offset).ok() else {
            return Ok(None);
        };
        let Some(limit) = offset.checked_add(record.raw_bytes.len()) else {
            return Ok(None);
        };
        if bytes.get(offset..limit) != Some(record.raw_bytes.as_slice()) {
            return Ok(None);
        }
        let mut framed = match cadmpeg_asm::sab::frame(ctx, bytes, offset, limit, width, None) {
            Ok(records) => records,
            Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => {
                return Err(error.into())
            }
            Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
                return Err(error.into_codec_error())
            }
            Err(_) => return Ok(None),
        };
        if framed.len() != 1 {
            return Ok(None);
        }
        let Some(framed) = framed.pop() else {
            return Ok(None);
        };
        if framed.name != record.name() {
            return Ok(None);
        }

        if archived_frames.insert(revision_id, framed).is_some() {
            return Ok(None);
        }
    }
    Ok(historical_record_archive(
        states,
        active_records,
        archived_frames,
    ))
}

pub(crate) type HistoricalRecordArchive = HashMap<i64, cadmpeg_asm::sab::Record>;

fn historical_record_archive(
    states: &[AsmDeltaState],
    active_records: &[cadmpeg_asm::sab::Record],
    archived_frames: HashMap<i64, cadmpeg_asm::sab::Record>,
) -> Option<HistoricalRecordArchive> {
    if active_records
        .iter()
        .enumerate()
        .any(|(index, record)| record.index != index)
    {
        return None;
    }
    let active_count = i64::try_from(active_records.len()).ok()?;

    let mut revision_entities = HashMap::new();

    for entity_ref in 0..active_count {
        revision_entities.insert(entity_ref, entity_ref);
    }
    for change in states
        .iter()
        .flat_map(|state| &state.bulletin_boards)
        .flat_map(|board| &board.changes)
    {
        let Some(old_ref) = change.old_ref() else {
            continue;
        };
        let entity_ref = change.new_ref().unwrap_or(old_ref);

        if revision_entities.insert(old_ref, entity_ref).is_some() {
            return None;
        }
    }

    let mut records = HashMap::new();

    for (revision, record) in active_records.iter().enumerate() {
        let revision = i64::try_from(revision).ok()?;
        records.insert(revision, clone_historical_record(record));
    }
    for (revision_id, framed) in archived_frames {
        if records.insert(revision_id, framed).is_some() {
            return None;
        }
    }
    if records.len() != revision_entities.len() {
        return None;
    }
    for (&revision_ref, record) in &mut records {
        let &entity_ref = revision_entities.get(&revision_ref)?;
        let index = usize::try_from(entity_ref).ok()?;
        record.index = index;
        if std::sync::Arc::strong_count(&record.tokens) > 1 {
            let mut tokens = Vec::new();
            for token in record.tokens.iter() {
                tokens.push(clone_historical_token(token));
            }
            record.tokens = tokens.into();
        }
        for token in std::sync::Arc::make_mut(&mut record.tokens) {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 {
                let &entity_ref = revision_entities.get(reference)?;
                *reference = entity_ref;
            }
        }
    }
    Some(records)
}

fn clone_historical_record(record: &cadmpeg_asm::sab::Record) -> cadmpeg_asm::sab::Record {
    let name = record.name.clone();
    cadmpeg_asm::sab::Record {
        index: record.index,
        name,
        tokens: record.tokens.clone(),
        offset: record.offset,
        len: record.len,
    }
}

fn clone_historical_token(token: &cadmpeg_asm::sab::Token) -> cadmpeg_asm::sab::Token {
    use cadmpeg_asm::sab::Token;
    match token {
        Token::Str(value) => Token::Str(value.to_owned()),
        Token::Ident(value) => Token::Ident(value.to_owned()),
        Token::SubIdent(value) => Token::SubIdent(value.to_owned()),
        _ => token.clone(),
    }
}

/// Release derived topologies after projection and retain only late-selection rows.
pub(crate) fn discard_projection_caches(histories: &mut [AsmHistory]) {
    for history in histories {
        history.topology_scope = crate::history_records::AsmTopologyScope::Retained;
        for state in history.states_mut() {
            if let Some(transition) = &mut state.transition {
                transition.rows.retain_projection_rows();
            }
        }
        history.release_topologies();
    }
}

pub(crate) fn projection_was_finalized(histories: &[AsmHistory]) -> bool {
    !histories.is_empty()
        && histories.iter().all(|history| {
            history.topology_scope == crate::history_records::AsmTopologyScope::Retained
                && !history.states().is_empty()
        })
}

/// The keyed rows that decide the solidity and extent of one state's bodies.
struct HistoricalBodyIndex<'a> {
    body_regions: KeyedRows<'a, AsmHistoricalRelation>,
    region_shells: KeyedRows<'a, AsmHistoricalRelation>,
    shell_faces: KeyedRows<'a, AsmHistoricalRelation>,
    shell_wire_edges: KeyedRows<'a, AsmHistoricalRelation>,
    shell_free_vertices: KeyedRows<'a, AsmHistoricalRelation>,
    face_loops: KeyedRows<'a, AsmHistoricalRelation>,
    loop_coedges: KeyedRows<'a, AsmHistoricalRelation>,
    coedges: KeyedRows<'a, AsmHistoricalCoedge>,
    edges: KeyedRows<'a, AsmHistoricalEdge>,
    vertex_points: KeyedRows<'a, AsmHistoricalCarrierBinding>,
    point_positions: KeyedRows<'a, AsmHistoricalPoint>,
}

impl<'a> HistoricalBodyIndex<'a> {
    fn new(topology: &'a AsmHistoricalTopology) -> Self {
        Self {
            body_regions: KeyedRows::new(&topology.body_regions),
            region_shells: KeyedRows::new(&topology.region_shells),
            shell_faces: KeyedRows::new(&topology.shell_faces),
            shell_wire_edges: KeyedRows::new(&topology.shell_wire_edges),
            shell_free_vertices: KeyedRows::new(&topology.shell_free_vertices),
            face_loops: KeyedRows::new(&topology.face_loops),
            loop_coedges: KeyedRows::new(&topology.loop_coedges),
            coedges: KeyedRows::new(&topology.coedge_topology),
            edges: KeyedRows::new(&topology.edge_vertices),
            vertex_points: KeyedRows::new(&topology.vertex_points),
            point_positions: KeyedRows::new(&topology.point_positions),
        }
    }

    fn shells(&self, body: i64) -> impl Iterator<Item = i64> + '_ {
        self.body_regions
            .members(body)
            .unwrap_or_default()
            .iter()
            .flat_map(move |&region| self.region_shells.members(region).unwrap_or_default())
            .copied()
    }

    /// The edge of each coedge in the loops of `face`, once per coedge.
    fn face_edges(&self, face: i64) -> impl Iterator<Item = i64> + '_ {
        self.face_loops
            .members(face)
            .unwrap_or_default()
            .iter()
            .flat_map(move |&loop_| self.loop_coedges.members(loop_).unwrap_or_default())
            .filter_map(move |&coedge| self.coedges.get(coedge))
            .map(|coedge| coedge.edge)
    }

    /// The edges of a solid `body`, or `None` when the body has no shell or
    /// has a shell that is not closed.
    fn solid_edges(&self, body: i64) -> Option<Vec<i64>> {
        let mut solid_edges = Vec::new();
        let mut has_shell = false;
        for shell in self.shells(body) {
            let occupied = |rows: &KeyedRows<'_, AsmHistoricalRelation>| {
                rows.members(shell)
                    .is_some_and(|members| !members.is_empty())
            };
            if occupied(&self.shell_wire_edges) || occupied(&self.shell_free_vertices) {
                return None;
            }
            let faces = self.shell_faces.members(shell).unwrap_or_default();
            if faces.is_empty() {
                return None;
            }
            let mut coedge_edges = faces
                .iter()
                .flat_map(|&face| self.face_edges(face))
                .collect::<Vec<_>>();
            coedge_edges.sort_unstable();
            if coedge_edges
                .chunk_by(|left, right| left == right)
                .any(|uses| uses.len() < 2)
            {
                return None;
            }
            coedge_edges.dedup();
            solid_edges.append(&mut coedge_edges);
            has_shell = true;
        }
        has_shell.then_some(solid_edges)
    }

    fn is_solid(&self, body: i64) -> bool {
        self.solid_edges(body).is_some()
    }

    fn edge_vertex_positions(&self, edges: &[i64]) -> Vec<cadmpeg_ir::math::Point3> {
        let mut vertices = edges
            .iter()
            .filter_map(|&edge| self.edges.get(edge))
            .flat_map(|edge| [edge.start_vertex, edge.end_vertex])
            .collect::<Vec<_>>();
        vertices.sort_unstable();
        vertices.dedup();
        vertices
            .into_iter()
            .filter_map(|vertex| {
                self.vertex_points
                    .get(vertex)
                    .and_then(|binding| self.point_positions.get(binding.carrier))
                    .map(|p| p.position)
            })
            .collect()
    }

    fn edge_bounds(
        &self,
        edges: &[i64],
    ) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Point3)> {
        let positions = self.edge_vertex_positions(edges);
        if positions.is_empty() {
            return None;
        }
        let mut min_x = f64::INFINITY;
        let mut min_y = f64::INFINITY;
        let mut min_z = f64::INFINITY;
        let mut max_x = f64::NEG_INFINITY;
        let mut max_y = f64::NEG_INFINITY;
        let mut max_z = f64::NEG_INFINITY;
        for pos in positions {
            min_x = min_x.min(pos.x);
            min_y = min_y.min(pos.y);
            min_z = min_z.min(pos.z);
            max_x = max_x.max(pos.x);
            max_y = max_y.max(pos.y);
            max_z = max_z.max(pos.z);
        }
        Some((
            cadmpeg_ir::math::Point3::new(min_x, min_y, min_z),
            cadmpeg_ir::math::Point3::new(max_x, max_y, max_z),
        ))
    }
}

struct EdgeMidpointRequest {
    feature_index: usize,
    output_state_id: i64,
    slots: Vec<i64>,
    has_unresolved: bool,
}

pub(crate) fn bind_feature_outputs(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &mut [AsmHistory],
    active_bodies: &[cadmpeg_ir::topology::Body],
    scope_histories: &HashMap<String, String>,
) -> Result<Vec<cadmpeg_ir::report::loss::LossNote>, cadmpeg_core::CodecError> {
    use cadmpeg_ir::features::{EdgeSelection, FeatureDefinition, FeatureOperation};

    let mut losses = Vec::new();
    let output_chains = features
        .iter()
        .map(|feature| {
            let (current, previous) = feature_history_states(feature, scopes)?;
            output_chain::OutputChain::resolve(
                histories,
                current,
                previous,
                feature
                    .native_ref
                    .as_deref()
                    .and_then(|id| scope_histories.get(id))
                    .map(String::as_str),
            )
        })
        .collect::<Vec<_>>();
    let output_states = features
        .iter()
        .filter_map(|feature| feature_history_states(feature, scopes))
        .map(|(state_id, _)| state_id)
        .chain(
            output_chains
                .iter()
                .flatten()
                .flat_map(|chain| chain.states.iter().copied()),
        )
        .collect::<HashSet<_>>();
    let mut measured_states = output_states.clone();
    for feature in features.iter() {
        if let Some((_, previous_state_id)) = feature_history_states(feature, scopes) {
            measured_states.insert(previous_state_id);
        }
    }
    measured_states.extend(
        histories
            .iter()
            .flat_map(AsmHistory::states)
            .filter(|state| output_states.contains(&state.state_id))
            .filter_map(|state| state.transition.as_ref()?.previous_state_id),
    );
    let mut treatment_states = HashMap::<(usize, i64), Vec<EdgeMidpointRequest>>::new();
    for (feature_index, feature) in features.iter().enumerate() {
        let Some((state_id, previous_state_id)) = feature_history_states(feature, scopes) else {
            continue;
        };
        let op = match feature.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let selections: Vec<&EdgeSelection> = match op {
            FeatureOperation::Fillet { groups } => groups.iter().map(|g| &g.edges).collect(),
            FeatureOperation::Chamfer { groups, .. } => groups.iter().map(|g| &g.edges).collect(),
            _ => continue,
        };
        let mut slots = Vec::new();
        let mut has_unresolved = false;
        for sel in selections {
            let mut extract_slots = |edge_ids: &[cadmpeg_ir::ids::HistoricalEdgeId]| {
                if edge_ids.is_empty() {
                    has_unresolved = true;
                }
                for edge_id in edge_ids {
                    if let Some(slot) =
                        crate::ids::feature_input_edge_slot(&feature.id, previous_state_id, edge_id)
                    {
                        if !slots.contains(&slot) {
                            slots.push(slot);
                        }
                    } else {
                        has_unresolved = true;
                    }
                }
            };
            match sel {
                EdgeSelection::Historical { edges, .. } => {
                    extract_slots(edges.as_slice());
                }
                EdgeSelection::HistoricalPartial {
                    edges, unresolved, ..
                } => {
                    extract_slots(edges.as_slice());
                    if !unresolved.is_empty() {
                        has_unresolved = true;
                    }
                }
                EdgeSelection::Resolved { edges, .. } | EdgeSelection::Edges(edges) => {
                    if edges.is_empty() {
                        has_unresolved = true;
                    }
                    for edge_id in edges {
                        if let Some(slot) = stable_ref(edge_id.as_str()) {
                            if !slots.contains(&slot) {
                                slots.push(slot);
                            }
                        } else {
                            has_unresolved = true;
                        }
                    }
                }
                EdgeSelection::Native(_)
                | EdgeSelection::Unresolved
                | EdgeSelection::All
                | EdgeSelection::Generated { .. } => {
                    has_unresolved = true;
                }
            }
        }
        if has_unresolved || slots.is_empty() {
            losses.push(
                crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                    "feature {} edge selection cannot resolve historical midpoints for state {state_id}",
                    feature.id
                )),
            );
        }
        let source = feature.native_ref.as_deref().and_then(|scope| {
            let (history, _, previous) = if scope_histories.contains_key(scope) {
                bound_history_state_pair(
                    scope,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )
            } else {
                unique_history_state_pair(histories, state_id, previous_state_id)
            }?;
            history_state_indexes(histories, history, previous)
        });
        if let Some((history_index, _)) = source {
            treatment_states
                .entry((history_index, previous_state_id))
                .or_default()
                .push(EdgeMidpointRequest {
                    feature_index,
                    output_state_id: state_id,
                    slots,
                    has_unresolved,
                });
        } else if !slots.is_empty() {
            losses.push(
                crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                    "feature {} edge selection has no bound input state {previous_state_id}",
                    feature.id
                )),
            );
        }
    }
    let mut feature_edge_midpoints = HashMap::<usize, Vec<cadmpeg_ir::math::Point3>>::new();
    let mut state_deleted_bodies = HashMap::<i64, Vec<i64>>::new();
    let mut state_solid_bodies = HashMap::<i64, BTreeSet<i64>>::new();
    let mut state_body_centers = HashMap::<i64, Vec<(i64, cadmpeg_ir::math::Point3)>>::new();
    let mut state_body_bounds =
        HashMap::<i64, Vec<(i64, (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Point3))>>::new();
    let mut state_body_vertices =
        HashMap::<i64, BTreeMap<i64, Vec<cadmpeg_ir::math::Point3>>>::new();
    let mut state_inserted_plane =
        HashMap::<i64, (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)>::new();
    let mut state_solid_outputs = HashMap::<i64, Vec<i64>>::new();
    let mut state_outputs = HashMap::<(usize, i64), Option<Vec<i64>>>::new();
    for (history_index, history) in histories.iter_mut().enumerate() {
        let mut by_node = HashMap::new();
        for (index, state) in history.states().iter().enumerate() {
            by_node.insert(state.node_index, index);
        }
        if by_node.len() != history.states().len() {
            continue;
        }
        let order = history
            .states_oldest_first()
            .unwrap_or_else(|| (0..history.states().len()).collect());
        let mut walked_deletions: Option<(usize, Option<BTreeSet<i64>>)> = None;
        for (position, &index) in order.iter().enumerate() {
            let state = &history.states()[index];
            let walked = walked_deletions.take();
            if let Some(Some(&previous)) = state.next_ref.map(|node| by_node.get(&node)) {
                if output_states.contains(&state.state_id) {
                    if let (Some(transition), Some(prev_topology)) = (
                        state.transition.as_ref(),
                        history.topology(&history.states()[previous]),
                    ) {
                        let prev_index = HistoricalBodyIndex::new(prev_topology);
                        let whole_deleted: Vec<i64> = transition
                            .topology
                            .bodies
                            .deleted
                            .iter()
                            .copied()
                            .filter(|&b| prev_index.is_solid(b))
                            .collect();
                        if !whole_deleted.is_empty() {
                            state_deleted_bodies.insert(state.state_id, whole_deleted);
                        }
                    }
                }
            }
            let deleted_bodies = match state.next_ref.map(|node| by_node.get(&node)) {
                Some(Some(&previous)) if output_states.contains(&state.state_id) => {
                    match walked.filter(|(walked, _)| *walked == previous) {
                        Some((_, bodies)) => bodies,
                        None => state
                            .transition
                            .as_ref()
                            .and_then(|transition| {
                                bodies_touching(
                                    history.topology(&history.states()[previous])?,
                                    &[&changed_topology_family_refs(&transition.topology, true)],
                                )
                            })
                            .and_then(|touched| touched.into_iter().next()),
                    }
                }
                _ => None,
            };
            history.advance_topology(index);
            let cur_state_id = history.states()[index].state_id;
            if let Some(requests) = treatment_states.remove(&(history_index, cur_state_id)) {
                if let Some(prev_topology) = history.topology(&history.states()[index]) {
                    let edge_samples = KeyedRows::new(&prev_topology.edge_samples);
                    let is_line_edge = |slot: i64| -> bool {
                        let Some(ec) = prev_topology.edge_curves.iter().find(|e| e.entity == slot)
                        else {
                            return false;
                        };
                        let Some(ca) = prev_topology
                            .curve_axes
                            .iter()
                            .find(|c| Some(c.curve) == ec.carrier)
                        else {
                            return false;
                        };
                        let Some(ev) = prev_topology.edge_vertices.iter().find(|e| e.edge == slot)
                        else {
                            return false;
                        };
                        let Some(c0) = prev_topology
                            .vertex_points
                            .iter()
                            .find(|v| v.entity == ev.start_vertex)
                            .map(|v| v.carrier)
                        else {
                            return false;
                        };
                        let Some(p0) = prev_topology
                            .point_positions
                            .iter()
                            .find(|p| p.point == c0)
                            .map(|p| p.position)
                        else {
                            return false;
                        };
                        let vx = p0.x - ca.origin.x;
                        let vy = p0.y - ca.origin.y;
                        let vz = p0.z - ca.origin.z;
                        let dx = ca.direction.x;
                        let dy = ca.direction.y;
                        let dz = ca.direction.z;
                        let d_len_sq = dx * dx + dy * dy + dz * dz;
                        if d_len_sq <= EPS_HISTORY_LINE_AXIS_DIR_NORM_SQ_E12 {
                            return false;
                        }
                        let cx = vy * dz - vz * dy;
                        let cy = vz * dx - vx * dz;
                        let cz = vx * dy - vy * dx;
                        (cx * cx + cy * cy + cz * cz) / d_len_sq < EPS_HISTORY_LINE_AXIS_CROSS_SQ_E5
                    };
                    let edge_endpoints = |slot: i64| -> Option<(
                        cadmpeg_ir::math::Point3,
                        cadmpeg_ir::math::Point3,
                    )> {
                        let ev = prev_topology
                            .edge_vertices
                            .iter()
                            .find(|e| e.edge == slot)?;
                        let c0 = prev_topology
                            .vertex_points
                            .iter()
                            .find(|v| v.entity == ev.start_vertex)?
                            .carrier;
                        let c1 = prev_topology
                            .vertex_points
                            .iter()
                            .find(|v| v.entity == ev.end_vertex)?
                            .carrier;
                        let p0 = prev_topology
                            .point_positions
                            .iter()
                            .find(|p| p.point == c0)?
                            .position;
                        let p1 = prev_topology
                            .point_positions
                            .iter()
                            .find(|p| p.point == c1)?
                            .position;
                        Some((p0, p1))
                    };
                    for EdgeMidpointRequest {
                        feature_index,
                        output_state_id,
                        slots: explicit_slots,
                        has_unresolved,
                    } in requests
                    {
                        let transition_opt = history
                            .states()
                            .iter()
                            .find(|s| s.state_id == output_state_id)
                            .and_then(|s| s.transition.as_ref());
                        let curr_segments: Vec<(
                            cadmpeg_ir::math::Point3,
                            cadmpeg_ir::math::Point3,
                        )> = if let Some(transition) = transition_opt {
                            let find_vertex_point = |v_ent: i64| -> Option<i64> {
                                transition
                                    .rows
                                    .vertex_points
                                    .set
                                    .iter()
                                    .find(|v| v.entity == v_ent)
                                    .or_else(|| {
                                        prev_topology
                                            .vertex_points
                                            .iter()
                                            .find(|v| v.entity == v_ent)
                                    })
                                    .map(|v| v.carrier)
                            };
                            let find_point_pos = |pt_ent: i64| -> Option<cadmpeg_ir::math::Point3> {
                                transition
                                    .rows
                                    .point_positions
                                    .set
                                    .iter()
                                    .find(|p| p.point == pt_ent)
                                    .or_else(|| {
                                        prev_topology
                                            .point_positions
                                            .iter()
                                            .find(|p| p.point == pt_ent)
                                    })
                                    .map(|p| p.position)
                            };
                            transition
                                .rows
                                .edge_vertices
                                .set
                                .iter()
                                .filter_map(|ev| {
                                    let c0 = find_vertex_point(ev.start_vertex)?;
                                    let c1 = find_vertex_point(ev.end_vertex)?;
                                    if c0 == c1 {
                                        return None;
                                    }
                                    let q0 = find_point_pos(c0)?;
                                    let q1 = find_point_pos(c1)?;
                                    Some((q0, q1))
                                })
                                .collect()
                        } else {
                            Vec::new()
                        };
                        let is_merely_shortened =
                            |p0: cadmpeg_ir::math::Point3, p1: cadmpeg_ir::math::Point3| -> bool {
                                let dx = p1.x - p0.x;
                                let dy = p1.y - p0.y;
                                let dz = p1.z - p0.z;
                                let len_sq = dx * dx + dy * dy + dz * dz;
                                if len_sq <= EPS_HISTORY_EDGE_SEGMENT_LEN_SQ_E12 {
                                    return false;
                                }
                                curr_segments.iter().any(|&(q0, q1)| {
                                    let v0x = q0.x - p0.x;
                                    let v0y = q0.y - p0.y;
                                    let v0z = q0.z - p0.z;
                                    let c0x = v0y * dz - v0z * dy;
                                    let c0y = v0z * dx - v0x * dz;
                                    let c0z = v0x * dy - v0y * dx;
                                    let dist0_sq = (c0x * c0x + c0y * c0y + c0z * c0z) / len_sq;
                                    if dist0_sq > EPS_HISTORY_EDGE_COLLINEAR_DIST_SQ_E6 {
                                        return false;
                                    }
                                    let v1x = q1.x - p0.x;
                                    let v1y = q1.y - p0.y;
                                    let v1z = q1.z - p0.z;
                                    let c1x = v1y * dz - v1z * dy;
                                    let c1y = v1z * dx - v1x * dz;
                                    let c1z = v1x * dy - v1y * dx;
                                    let dist1_sq = (c1x * c1x + c1y * c1y + c1z * c1z) / len_sq;
                                    if dist1_sq > EPS_HISTORY_EDGE_COLLINEAR_DIST_SQ_E6 {
                                        return false;
                                    }
                                    let t0 = (v0x * dx + v0y * dy + v0z * dz) / len_sq;
                                    let t1 = (v1x * dx + v1y * dy + v1z * dz) / len_sq;
                                    let t_min = t0.min(t1);
                                    let t_max = t0.max(t1);
                                    t_max > 0.05 && t_min < 0.95 && (t_max - t_min) > 0.05
                                })
                            };
                        let slots: Vec<i64> = if !explicit_slots.is_empty() && !has_unresolved {
                            let filtered: Vec<i64> = explicit_slots
                                .iter()
                                .copied()
                                .filter(|&s| {
                                    !edge_endpoints(s)
                                        .is_some_and(|(p0, p1)| is_merely_shortened(p0, p1))
                                })
                                .collect();
                            if filtered.is_empty() {
                                explicit_slots
                            } else {
                                filtered
                            }
                        } else if let Some(transition) =
                            transition_opt.filter(|t| t.topology.edges.deleted.len() <= 128)
                        {
                            let line_slots: Vec<i64> = transition
                                .topology
                                .edges
                                .deleted
                                .iter()
                                .copied()
                                .filter(|&s| {
                                    is_line_edge(s)
                                        && edge_endpoints(s)
                                            .is_some_and(|(p0, p1)| !is_merely_shortened(p0, p1))
                                })
                                .collect();
                            let mut combined = explicit_slots;
                            let extra = if line_slots.is_empty() {
                                transition
                                    .topology
                                    .edges
                                    .deleted
                                    .iter()
                                    .copied()
                                    .filter(|&s| {
                                        edge_endpoints(s)
                                            .is_some_and(|(p0, p1)| !is_merely_shortened(p0, p1))
                                    })
                                    .collect::<Vec<_>>()
                            } else {
                                line_slots
                            };
                            for s in extra {
                                if !combined.contains(&s) {
                                    combined.push(s);
                                }
                            }
                            let filtered: Vec<i64> = combined
                                .iter()
                                .copied()
                                .filter(|&s| {
                                    !edge_endpoints(s)
                                        .is_some_and(|(p0, p1)| is_merely_shortened(p0, p1))
                                })
                                .collect();
                            if filtered.is_empty() {
                                combined
                            } else {
                                filtered
                            }
                        } else {
                            explicit_slots
                        };
                        let mut midpoints = Vec::new();
                        for slot in slots {
                            let sample = prev_topology
                                .edges
                                .contains(&slot)
                                .then(|| edge_samples.get(slot))
                                .flatten();
                            if let Some(sample) = sample {
                                midpoints.push(sample.interior);
                            } else if let Some((p0, p1)) = edge_endpoints(slot) {
                                midpoints.push(cadmpeg_ir::math::Point3::new(
                                    (p0.x + p1.x) * 0.5,
                                    (p0.y + p1.y) * 0.5,
                                    (p0.z + p1.z) * 0.5,
                                ));
                            } else {
                                losses.push(crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(
                                    format!("historical edge slot {slot} has no carrier sample in bound input state {cur_state_id}"),
                                ));
                            }
                        }
                        if !midpoints.is_empty() {
                            feature_edge_midpoints.insert(feature_index, midpoints);
                        }
                    }
                }
            }
            let measured_topology = measured_states
                .contains(&cur_state_id)
                .then(|| history.topology(&history.states()[index]))
                .flatten();
            if let Some(topology) = measured_topology {
                let body_index = HistoricalBodyIndex::new(topology);
                let solid_edges: BTreeMap<i64, Vec<i64>> = topology
                    .bodies
                    .iter()
                    .filter_map(|&b| Some((b, body_index.solid_edges(b)?)))
                    .collect();
                let mut bounds_list = Vec::new();
                let mut vertices_map = BTreeMap::new();
                for (&b, edges) in &solid_edges {
                    let vpos = body_index.edge_vertex_positions(edges);
                    if !vpos.is_empty() {
                        vertices_map.insert(b, vpos);
                    }
                    if let Some(bds) = body_index.edge_bounds(edges) {
                        bounds_list.push((b, bds));
                    } else {
                        losses.push(
                            crate::loss::F3dLossCode::HistoricalBodyBoundsUnresolved.note(
                                format!(
                                    "historical solid body {b} has unresolved bounds in state {cur_state_id}"
                                ),
                            ),
                        );
                    }
                }
                if !vertices_map.is_empty() {
                    state_body_vertices.insert(cur_state_id, vertices_map);
                }
                if output_states.contains(&cur_state_id) {
                    let centers: Vec<(i64, cadmpeg_ir::math::Point3)> = bounds_list
                        .iter()
                        .map(|&(b, (min_pt, max_pt))| {
                            let center = cadmpeg_ir::math::Point3::new(
                                (min_pt.x + max_pt.x) * 0.5,
                                (min_pt.y + max_pt.y) * 0.5,
                                (min_pt.z + max_pt.z) * 0.5,
                            );
                            (b, center)
                        })
                        .collect();
                    if !centers.is_empty() {
                        state_body_centers.insert(cur_state_id, centers.clone());
                    }
                    if let Some(transition) = history.states()[index].transition.as_ref() {
                        let mut chosen_plane = None;
                        for &fid in &transition.topology.faces.inserted {
                            if let Some(fs) =
                                topology.face_surfaces.iter().find(|f| f.entity == fid)
                            {
                                if let Some(sp) = topology
                                    .surface_planes
                                    .iter()
                                    .find(|s| s.surface == fs.carrier)
                                {
                                    if centers.len() >= 2 {
                                        let separates = centers.iter().any(|&(_, c1)| {
                                            centers.iter().any(|&(_, c2)| {
                                                let d1 = (c1.x - sp.origin.x) * sp.normal.x
                                                    + (c1.y - sp.origin.y) * sp.normal.y
                                                    + (c1.z - sp.origin.z) * sp.normal.z;
                                                let d2 = (c2.x - sp.origin.x) * sp.normal.x
                                                    + (c2.y - sp.origin.y) * sp.normal.y
                                                    + (c2.z - sp.origin.z) * sp.normal.z;
                                                d1 * d2 < -1.0e-4
                                            })
                                        });
                                        if separates {
                                            chosen_plane = Some((sp.origin, sp.normal));
                                            break;
                                        }
                                    }
                                    if chosen_plane.is_none() {
                                        chosen_plane = Some((sp.origin, sp.normal));
                                    }
                                }
                            }
                        }
                        if let Some(plane) = chosen_plane {
                            state_inserted_plane.insert(cur_state_id, plane);
                        }
                    }
                }
                if !bounds_list.is_empty() {
                    state_body_bounds.insert(cur_state_id, bounds_list);
                }
                state_solid_bodies.insert(cur_state_id, solid_edges.into_keys().collect());
            }
            let mut next_deletions = None;
            let outputs = (|| {
                let state = &history.states()[index];
                let previous = match state.next_ref {
                    Some(node) => Some(*by_node.get(&node)?),
                    None => None,
                };
                let transition = state.transition.as_ref()?;
                if transition.previous_state_id
                    != previous.map(|index| history.states()[index].state_id)
                {
                    return None;
                }
                let topology = history.topology(state)?;
                if !output_states.contains(&state.state_id) {
                    return None;
                }
                let successor_deleted = order
                    .get(position + 1)
                    .map(|&next| &history.states()[next])
                    .filter(|next| {
                        next.next_ref.and_then(|node| by_node.get(&node).copied()) == Some(index)
                    })
                    .and_then(|next| next.transition.as_ref())
                    .map(|next| changed_topology_family_refs(&next.topology, true))
                    .unwrap_or_default();
                let Some([mut affected, successor_bodies]) = bodies_touching(
                    topology,
                    &[
                        &changed_topology_family_refs(&transition.topology, false),
                        &successor_deleted,
                    ],
                )
                .and_then(|touched| <[_; 2]>::try_from(touched).ok()) else {
                    next_deletions = Some((index, None));
                    return None;
                };
                next_deletions = Some((index, Some(successor_bodies)));
                if previous.is_some() {
                    affected.extend(deleted_bodies?);
                }
                let prev_solid =
                    previous.and_then(|p| state_solid_bodies.get(&history.states()[p].state_id));
                let cur_solid = state_solid_bodies.get(&state.state_id);
                let solid_affected: Vec<i64> = affected
                    .iter()
                    .copied()
                    .filter(|&b| {
                        cur_solid.is_some_and(|cs| cs.contains(&b))
                            || prev_solid.is_some_and(|ps| ps.contains(&b))
                    })
                    .collect();
                Some((
                    state.state_id,
                    affected.into_iter().collect::<Vec<_>>(),
                    solid_affected,
                ))
            })();
            walked_deletions = next_deletions;
            history.retain_topology(index);
            let Some((state_id, outputs, solid_affected)) = outputs else {
                continue;
            };
            if !solid_affected.is_empty() {
                state_solid_outputs.insert(state_id, solid_affected);
            }
            state_outputs
                .entry((history_index, state_id))
                .and_modify(|outputs| *outputs = None)
                .or_insert_with(|| Some(outputs));
        }
    }
    for ((_, state_id), requests) in treatment_states {
        for EdgeMidpointRequest { feature_index, .. } in requests {
            losses.push(
                crate::loss::F3dLossCode::HistoricalEdgeMidpointsUnresolved.note(format!(
                    "feature {} edge selection has no available input state {state_id}",
                    features[feature_index].id
                )),
            );
        }
    }
    let mut active = HashMap::new();
    let stored_active = stored_bodies::ActiveBodies::new(active_bodies);
    for body in active_bodies {
        let Some(slot) = stable_ref(body.id.as_str()) else {
            continue;
        };
        let id = body.id.clone();
        active.insert(slot, id);
    }
    let mut ordered_indices: Vec<usize> = (0..features.len()).collect();
    ordered_indices.sort_by_key(|&i| features[i].ordinal);
    let mut prev_feature_solid: Option<&BTreeSet<i64>> = None;
    let mut absorbed_bodies: BTreeSet<i64> = BTreeSet::new();
    let mut absorbed_into = BTreeMap::<i64, i64>::new();
    for idx in ordered_indices {
        let feature = &mut features[idx];
        let Some((state_id, previous_state_id)) = feature_history_states(feature, scopes) else {
            continue;
        };
        let mut newly_absorbed = BTreeSet::new();
        let before_bds = state_body_bounds.get(&previous_state_id);
        let before_verts = state_body_vertices.get(&previous_state_id);
        let after_verts = state_body_vertices.get(&state_id);
        if let (Some(prev_bds), Some(cur_bds)) = (before_bds, state_body_bounds.get(&state_id)) {
            for (b1, (cmin, cmax)) in cur_bds {
                if absorbed_bodies.contains(b1) {
                    continue;
                }
                let Some((_, (pmin, pmax))) = prev_bds.iter().find(|(pb, _)| pb == b1) else {
                    continue;
                };
                let b1_grew = (cmax.x - cmin.x)
                    > (pmax.x - pmin.x) + EPS_HISTORY_BODY_BBOX_GROWTH_E3
                    || (cmax.y - cmin.y) > (pmax.y - pmin.y) + EPS_HISTORY_BODY_BBOX_GROWTH_E3
                    || (cmax.z - cmin.z) > (pmax.z - pmin.z) + EPS_HISTORY_BODY_BBOX_GROWTH_E3;
                if b1_grew {
                    for (b2, (b2_min, b2_max)) in prev_bds {
                        if b2 == b1 || absorbed_bodies.contains(b2) {
                            continue;
                        }
                        let b2_unchanged = cur_bds.iter().find(|(cb, _)| cb == b2).is_some_and(
                            |(_, (c2_min, c2_max))| {
                                (c2_min.x - b2_min.x).abs() < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                    && (c2_max.x - b2_max.x).abs()
                                        < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                    && (c2_min.y - b2_min.y).abs()
                                        < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                    && (c2_max.y - b2_max.y).abs()
                                        < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                    && (c2_min.z - b2_min.z).abs()
                                        < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                                    && (c2_max.z - b2_max.z).abs()
                                        < EPS_HISTORY_BODY_BBOX_UNCHANGED_E4
                            },
                        );
                        let prev_covered = pmin.x
                            <= b2_min.x + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && pmax.x >= b2_max.x - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && pmin.y <= b2_min.y + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && pmax.y >= b2_max.y - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && pmin.z <= b2_min.z + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && pmax.z >= b2_max.z - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2;
                        let cur_covers = cmin.x <= b2_min.x + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && cmax.x >= b2_max.x - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && cmin.y <= b2_min.y + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && cmax.y >= b2_max.y - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && cmin.z <= b2_min.z + EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2
                            && cmax.z >= b2_max.z - EPS_HISTORY_BODY_BBOX_CONTAINMENT_E2;
                        let vertices_absorbed = match (
                            before_verts.and_then(|m| m.get(b1)),
                            after_verts.and_then(|m| m.get(b1)),
                            before_verts.and_then(|m| m.get(b2)),
                        ) {
                            (Some(b1_prev), Some(b1_cur), Some(b2_prev)) => {
                                let close =
                                    |a: cadmpeg_ir::math::Point3, b: cadmpeg_ir::math::Point3| {
                                        let dx = a.x - b.x;
                                        let dy = a.y - b.y;
                                        let dz = a.z - b.z;
                                        dx * dx + dy * dy + dz * dz < 1.0e-4
                                    };
                                let b2_unique: Vec<_> = b2_prev
                                    .iter()
                                    .copied()
                                    .filter(|&v2| !b1_prev.iter().any(|&v1| close(v1, v2)))
                                    .collect();
                                !b2_unique.is_empty() && {
                                    let matched = b2_unique
                                        .iter()
                                        .filter(|&&v2| b1_cur.iter().any(|&vc| close(vc, v2)))
                                        .count();
                                    matched * 2 >= b2_unique.len()
                                }
                            }
                            _ => true,
                        };
                        if b2_unchanged && !prev_covered && cur_covers && vertices_absorbed {
                            newly_absorbed.insert(*b2);
                            absorbed_into.insert(*b2, *b1);
                        }
                    }
                }
            }
        }
        let op_ref = match feature.evaluation.definition() {
            FeatureDefinition::Operation(op) => op,
            FeatureDefinition::PostProcess { operation, .. } => operation,
        };
        let is_split_body = matches!(op_ref, FeatureOperation::SplitBody { .. });
        if let Some(midpoints) = feature_edge_midpoints.get(&idx) {
            let formatted = midpoints
                .iter()
                .map(|p| format!("{},{},{}", p.x, p.y, p.z))
                .collect::<Vec<_>>()
                .join(";");
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("edge_midpoints"))
                .or_insert(formatted);
        }
        if let Some(centers) = state_body_centers.get(&state_id) {
            let formatted = centers
                .iter()
                .filter(|(b, _)| !absorbed_bodies.contains(b) && !newly_absorbed.contains(b))
                .map(|(b, c)| format!("{b}:{},{},{}", c.x, c.y, c.z))
                .collect::<Vec<_>>()
                .join(";");
            if !formatted.is_empty() {
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("body_centers"))
                    .or_insert(formatted);
            }
        }
        if is_split_body {
            if let Some((o, n)) = state_inserted_plane.get(&state_id) {
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("split_plane"))
                    .or_insert_with(|| format!("{},{},{};{},{},{}", o.x, o.y, o.z, n.x, n.y, n.z));
            }
        }
        let before_solid = state_solid_bodies
            .get(&previous_state_id)
            .or(prev_feature_solid);
        let after_solid = state_solid_bodies.get(&state_id);
        let mut deleted_set = BTreeSet::new();
        if let (Some(before), Some(after)) = (before_solid, after_solid) {
            for &b in before.difference(after) {
                deleted_set.insert(b);
            }
        } else if let Some(del) = state_deleted_bodies.get(&state_id) {
            for &b in del {
                deleted_set.insert(b);
            }
        }
        if let (Some(prev_feat), Some(after)) = (prev_feature_solid, after_solid) {
            for &b in prev_feat.difference(after) {
                deleted_set.insert(b);
            }
        }
        deleted_set.extend(newly_absorbed.iter().copied());
        if after_solid.is_some() {
            prev_feature_solid = after_solid;
        }
        if !deleted_set.is_empty() {
            let formatted = deleted_set
                .iter()
                .map(i64::to_string)
                .collect::<Vec<_>>()
                .join(",");
            feature
                .source_properties
                .entry(cadmpeg_core::nonblank_literal!("deleted_bodies"))
                .or_insert(formatted);
        }
        if deleted_set.is_empty() && !combine_states_applied_operation(feature, scopes) {
            feature.evaluation.edit(|def, _| {
                if let FeatureDefinition::Operation(FeatureOperation::Combine {
                    op,
                    keep_tools,
                    ..
                })
                | FeatureDefinition::PostProcess {
                    operation: FeatureOperation::Combine { op, keep_tools, .. },
                    ..
                } = def
                {
                    if *op == cadmpeg_ir::features::BooleanKind::Join && *keep_tools {
                        *op = cadmpeg_ir::features::BooleanKind::Cut;
                    }
                }
            });
        }
        if let Some(solid_outputs) = state_solid_outputs.get(&state_id) {
            let mut filtered: Vec<i64> = if let (Some(before), Some(after)) =
                (before_solid, after_solid)
            {
                solid_outputs
                    .iter()
                    .copied()
                    .filter(|b| {
                        !absorbed_bodies.contains(b)
                            && (before.contains(b) || after.contains(b) || deleted_set.contains(b))
                    })
                    .collect()
            } else {
                solid_outputs
                    .iter()
                    .copied()
                    .filter(|b| !absorbed_bodies.contains(b))
                    .collect()
            };
            if let (Some(before), Some(after)) = (before_solid, after_solid) {
                for b in after
                    .difference(before)
                    .chain(before.difference(after))
                    .chain(deleted_set.iter())
                {
                    if !absorbed_bodies.contains(b) && !filtered.contains(b) {
                        filtered.push(*b);
                    }
                }
                filtered.sort_unstable();
            }
            if is_split_body {
                absorbed_into.retain(|_, host_b| !filtered.contains(host_b));
            }
            if !filtered.is_empty() {
                let formatted = filtered
                    .iter()
                    .map(i64::to_string)
                    .collect::<Vec<_>>()
                    .join(",");
                feature
                    .source_properties
                    .entry(cadmpeg_core::nonblank_literal!("affected_bodies"))
                    .or_insert(formatted);
            }
        }
        if let (Some(before), Some(after)) = (before_solid, after_solid) {
            for &b in after.difference(before) {
                absorbed_bodies.remove(&b);
            }
        }
        absorbed_bodies.extend(newly_absorbed);
        if state_id == previous_state_id {
            continue;
        }
        let chain_outputs = output_chains
            .get(idx)
            .and_then(Option::as_ref)
            .and_then(|chain| Some((chain.history, chain.outputs(&state_outputs)?)));
        let Some((history_index, outputs)) = chain_outputs else {
            losses.push(crate::loss::F3dLossCode::FeatureOutputStateChainUnbound.note(format!(
                "{} has no complete unique output transition chain from state {previous_state_id} to state {state_id}.",
                feature.id
            )));
            continue;
        };
        let source = historical_brep_source(&histories[history_index].id);
        let mut resolved = Vec::new();
        for slot in &outputs {
            if let Some(body) = stored_active.resolve(&[*slot], source) {
                resolved.extend(body.iter().cloned());
            }
        }
        if resolved.len() != outputs.len() {
            losses.push(crate::loss::F3dLossCode::FeatureOutputBodyUnbound.note(format!(
                "{} has {} affected body slot(s) without a unique active body in its BREP namespace.",
                feature.id, outputs.len() - resolved.len()
            )));
        }
        feature.evaluation.set_outputs(
            cadmpeg_ir::features::DistinctMembers::try_from(resolved)
                .map_err(cadmpeg_core::CodecError::from)?,
        );
        if let Some(scope) = feature
            .native_ref
            .as_deref()
            .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        {
            if stored_bodies::bind_history_outputs(feature, scope, &outputs, source, &stored_active)
            {
                bind_base_feature_output_selection(feature)?;
            }
        }
    }
    for feature in features {
        stored_bodies::finalize_body_sets(feature);
    }
    Ok(losses)
}

fn combine_states_applied_operation(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
) -> bool {
    feature
        .native_ref
        .as_deref()
        .and_then(|id| scopes.iter().find(|scope| scope.id == id))
        .and_then(|scope| scope.combine_operation())
        .is_some_and(|operation| {
            operation.form == crate::records::feature::combine::DesignCombineForm::OperandLists
        })
}

/// The history state a feature's scope produces and the state it follows.
fn feature_history_states(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
) -> Option<(i64, i64)> {
    let scope = feature
        .native_ref
        .as_deref()
        .and_then(|id| scopes.iter().find(|scope| scope.id == id))?;
    Some((
        scope.history_state_id()?,
        scope.previous_history_state_id()?,
    ))
}

fn bind_base_feature_output_selection(
    feature: &mut cadmpeg_ir::features::Feature,
) -> Result<(), cadmpeg_core::CodecError> {
    let cadmpeg_ir::features::FeatureDefinition::Operation(
        cadmpeg_ir::features::FeatureOperation::BaseFeature {
            bodies: cadmpeg_ir::features::BodySelection::Native(native),
        },
    ) = feature.evaluation.definition()
    else {
        return Ok(());
    };
    let mut selected = Vec::new();
    for body in feature.evaluation.outputs() {
        let id = body.clone();

        selected.push(id);
    }
    let bodies = cadmpeg_ir::features::BodySelection::Resolved {
        bodies: cadmpeg_ir::features::DistinctMembers::try_from(selected)
            .map_err(cadmpeg_core::CodecError::from)?,
        native: native.to_owned(),
    };
    feature
        .evaluation
        .set_definition(cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::BaseFeature { bodies },
        ));
    Ok(())
}

pub(crate) fn bind_sweep_result_modes(
    features: &mut [cadmpeg_ir::features::Feature],
    bodies: &[cadmpeg_ir::topology::Body],
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SweepMode, SweepShape};
    use cadmpeg_ir::topology::BodyKind;

    let mut body_kinds = HashMap::new();
    for body in bodies {
        body_kinds.insert(&body.id, body.kind);
    }
    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Sweep {
            shape: SweepShape::Unresolved { .. },
            ..
        }) = feature.evaluation.definition()
        else {
            continue;
        };
        if feature.evaluation.outputs().is_empty() {
            continue;
        }
        let mut all_sheet = true;
        let mut all_solid = true;
        for output in feature.evaluation.outputs() {
            match body_kinds.get(output) {
                Some(BodyKind::Sheet) => all_solid = false,
                Some(BodyKind::Solid) => all_sheet = false,
                _ => {
                    all_sheet = false;
                    all_solid = false;
                }
            }
        }
        let mode = if all_sheet {
            SweepMode::Surface {}
        } else if all_solid {
            SweepMode::Solid {
                op: cadmpeg_ir::features::SolidSweepOperation::NewBody,
            }
        } else {
            SweepMode::Unresolved {}
        };
        let mut solid_sections = Vec::new();
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Sweep { shape, .. }) = definition
            else {
                return;
            };
            let SweepShape::Unresolved { section, sections } =
                std::mem::replace(shape, SweepShape::unresolved(None))
            else {
                return;
            };
            *shape = match mode {
                SweepMode::Solid { op } => {
                    solid_sections.extend(sections.into_iter().map(Into::into));
                    SweepShape::Solid {
                        op,
                        section: section.into(),
                        sections: solid_sections,
                    }
                }
                _ => SweepShape::sheet_sections(mode, section, sections),
            };
        });
    }
}

/// Native history and neutral topology used to resolve feature body operands.
pub(crate) struct FeatureBodySelectionInputs<'a> {
    /// Decoded Design feature scopes.
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    /// Counted Design construction-operand groups.
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    /// Whole-body recipe operands.
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    /// Construction recipes backing whole-body operands.
    pub(crate) construction_recipes: &'a [crate::records::recipes::ConstructionRecipe],
    /// Persistent body identities in the active solved B-rep.
    pub(crate) persistent_design_links: &'a [crate::records::sketch_links::PersistentDesignLink],
    /// Independent ASM history graphs.
    pub(crate) histories: &'a [AsmHistory],
    /// Neutral top-level bodies.
    pub(crate) bodies: &'a [cadmpeg_ir::topology::Body],
    /// Neutral body regions.
    pub(crate) regions: &'a [cadmpeg_ir::topology::Region],
    /// Neutral region shells.
    pub(crate) shells: &'a [cadmpeg_ir::topology::Shell],
}

pub(crate) fn bind_feature_body_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'_>,
) -> Vec<cadmpeg_ir::report::loss::LossNote> {
    use cadmpeg_ir::features::{BodyMember, BodySelection, FeatureDefinition, FeatureOperation};

    let mut losses = Vec::new();
    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;
    let histories = inputs.histories;
    let bodies = inputs.bodies;
    let regions = inputs.regions;
    let shells = inputs.shells;

    let mut body_candidates = BodyCandidates::new(
        bodies,
        regions,
        shells,
        inputs.construction_recipes,
        inputs.persistent_design_links,
    );
    bind_pattern_body_selections(features, inputs, &mut body_candidates);
    let mut pattern_body_slots = HashMap::new();
    'pattern: for feature in features.iter() {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, pattern }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let cadmpeg_ir::features::patterns::PatternTransform::Circular { count, .. } =
            pattern.definition()
        else {
            continue;
        };
        let [cadmpeg_ir::features::patterns::PatternSeed::Bodies(BodySelection::Historical {
            bodies: seed_bodies,
            ..
        })] = seeds.as_slice()
        else {
            continue;
        };
        let [seed_body] = seed_bodies.as_slice() else {
            continue;
        };
        let Ok(expected_count) = usize::try_from(*count) else {
            continue;
        };
        if feature.evaluation.outputs().len().checked_add(1) != Some(expected_count) {
            continue;
        }
        let mut slots = BTreeSet::new();
        for slot in std::iter::once(historical_body_slot(seed_body.as_str())).chain(
            feature
                .evaluation
                .outputs()
                .iter()
                .map(|body| stable_ref(body.as_str())),
        ) {
            let Some(slot) = slot else {
                continue 'pattern;
            };
            slots.insert(slot);
        }
        if slots.len() != expected_count {
            continue;
        }
        let feature_id = feature.id.clone();
        pattern_body_slots.insert(feature_id, slots);
    }

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        let dependencies = &feature.dependencies;
        feature.evaluation.edit(|definition, _| {
        'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let Some(scope) = matching_scopes.next() else {
                break 'feature_edit;
            };
            if matching_scopes.next().is_some() {
                break 'feature_edit;
            }
            if matches!(
                definition,
                FeatureDefinition::Operation(FeatureOperation::Pattern { .. })
            ) {
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::BoundaryFill { tools, cells }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    bind_body_recipe_body_selection(
                        tools,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    for cell in cells {
                        bind_body_recipe_body_selection(
                            cell,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            body_recipe_operands,
                        );
                    }
                } else {
                    bind_direct_body_recipe_body_selection(tools, scope, inputs, &mut body_candidates);
                    for cell in cells {
                        bind_direct_body_recipe_body_selection(cell, scope, inputs, &mut body_candidates);
                    }
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Combine { operands, .. }) =
                definition
            {
                let refusal = combine_selection::refine(operands, feature_id, |target, tools| {
                        let (Some(state_id), Some(previous_state_id)) =
                            (scope.history_state_id(), scope.previous_history_state_id())
                        else {
                            bind_direct_body_recipe_body_selection(target, scope, inputs, &mut body_candidates);
                            bind_direct_body_recipe_body_selection(tools, scope, inputs, &mut body_candidates);
                            if matches!(
                                tools,
                                BodySelection::Native(_) | BodySelection::NativeSet(_)
                            ) {
                                if let Some(local) = combine_external_local_tools(scope) {
                                    *tools = local;
                                }
                            }
                            return;
                        };
                        if let Some(local) = combine_external_local_tools(scope) {
                            *tools = local;
                        }
                        if let Some(operation) = scope.combine_operation().filter(|operation| {
                            operation.form
                                == crate::records::feature::combine::DesignCombineForm::OperandLists
                        }) {
                            bind_operand_list_combine_bodies(
                                target,
                                tools,
                                scope,
                                operation,
                                feature_id,
                                previous_state_id,
                                (histories, inputs.scopes),
                            );
                            return;
                        }
                        let BodySelection::Native(native) = target else {
                            return;
                        };
                        let Some((history, state, _)) =
                            unique_history_state_pair(histories, state_id, previous_state_id)
                        else {
                            return;
                        };
                        let history_states = unique_feature_history_states(history);
                        let Some(body) = singleton_revised_input_body_across_state_chain(history, state, previous_state_id, &history_states) else {
                            return;
                        };
                        let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
                        let input_state = crate::ids::history_input_state_id(&prefix);
                        let body_id = crate::ids::history_input_body_id(&prefix, body);
                        let native_id = native.to_owned();
                        let target_bodies = vec![body_id];
                        if let Ok(historical) = cadmpeg_ir::features::BodySelection::historical(crate::ids::history_input_state_id(&prefix), target_bodies, native_id) {
                            *target = historical;
                        }
                        let Some(stream) = crate::ids::native_stream(&scope.id) else {
                            return;
                        };
                        let Some(operation) = scope.combine_operation() else {
                            return;
                        };
                        let mut native_tools = Vec::new();
                        for tool in operation.tools.iter() {
                            let native = format!("{stream}:design-record#{}", tool.record_index);
                            native_tools.push(native);
                        }
                        let current_history_source = historical_brep_source(&state.id);
                        let mut historical_tool_rows = Vec::new();
                        let mut direct_tool_rows = Vec::new();
                        for (record_index, native) in operation.tools.iter()
                            .map(|tool| tool.record_index).zip(&native_tools) {
                            let mut matching = body_recipe_operands.iter().filter(|operand| {
                                crate::ids::native_stream(&operand.id) == Some(stream)
                                    && operand.scope_record_index == scope.record_index
                                    && matches!(
                            operand.owner,
                            crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                        ) && operand.record_index() == record_index
                            });
                            let Some(operand) = matching.next() else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            if matching.next().is_some() {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            }
                            if let Some(body) = operand.resolved_body_slot {
                                let body = crate::ids::history_input_body_id(&prefix, body);
                                let repeated = historical_tool_rows
                                    .iter()
                                    .any(|row: &BodyMember<_>| row.body() == &body);
                                let row = body_member(body, native.to_owned());
                                let (false, Some(row)) = (repeated, row) else {
                                    historical_tool_rows.clear();
                                    direct_tool_rows.clear();
                                    break;
                                };
                                historical_tool_rows.push(row);
                                continue;
                            }
                            let Some(body) =
                                body_candidates.external(operand, current_history_source)
                            else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            let repeated = direct_tool_rows
                                .iter()
                                .any(|row: &BodyMember<_>| row.body() == &body);
                            let row = body_member(body, native.to_owned());
                            let (false, Some(row)) = (repeated, row) else {
                                historical_tool_rows.clear();
                                direct_tool_rows.clear();
                                break;
                            };
                            direct_tool_rows.push(row);
                        }
                        if historical_tool_rows.len() == native_tools.len() {
                            let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(historical_tool_rows) else {
                                return;
                            };
                            *tools = BodySelection::HistoricalSet {
                                state: input_state,
                                members,
                            };
                        } else if direct_tool_rows.len() == native_tools.len() {
                            *tools = if direct_tool_rows.len() == 1 {
                                let Some(row) = direct_tool_rows.pop() else { return; };
                                let (body, native) = row.into_parts();
                                let selected = vec![body];
                                let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(selected) else {
    return;
};
                                BodySelection::Resolved {
                                    bodies,
                                    native,
                                }
                            } else {
                                let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(direct_tool_rows) else {
                                    return;
                                };
                                BodySelection::ResolvedSet { members }
                            };
                        } else {
                            let tool_record_indices = operation.tools.iter().map(|tool| tool.record_index).collect::<Vec<_>>();
                            if let Some(tool_slots) = combine_recipe_family_tool_slots((stream, scope.record_index), &tool_record_indices, previous_state_id, body, body_recipe_operands, inputs.construction_recipes) {
                                if tool_slots.len() != native_tools.len() {
                                    return;
                                }
                                let Some(rows) = combine_historical_rows(&prefix, tool_slots, native_tools) else {
                                    return;
                                };
                                let Ok(members) =
                                    cadmpeg_ir::features::BodyMembers::try_from_rows(rows)
                                else {
                                    return;
                                };
                                *tools = BodySelection::HistoricalSet {
                                    state: input_state,
                                    members,
                                };
                                return;
                            }
                            let mut dependency_sets = dependencies.iter()
                                .filter_map(|dependency| pattern_body_slots.get(dependency));
                            let pattern_bodies = dependency_sets.next();
                            if dependency_sets.next().is_none() {
                              if let Some(pattern_bodies) = pattern_bodies {
                                if let Some(tool_slots) = pattern_combine_tool_slots(pattern_bodies, body, native_tools.len()) {
                                    if tool_slots.len() != native_tools.len() {
                                        return;
                                    }
                                    let Some(rows) = combine_historical_rows(&prefix, tool_slots, native_tools) else {
                                        return;
                                    };
                                    let Ok(members) =
                                        cadmpeg_ir::features::BodyMembers::try_from_rows(rows)
                                    else {
                                        return;
                                    };
                                    *tools = BodySelection::HistoricalSet {
                                        state: input_state,
                                        members,
                                    };
                                }
                              }
                            }
                        }
                    });
                if let Some(loss) = refusal {
                    losses.push(loss);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Coil {
                result: cadmpeg_ir::features::CoilResult::Boolean { targets, .. },
                ..
            }) = definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    bind_body_recipe_body_selection(
                        targets,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    bind_direct_body_recipe_body_selection(targets, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::DeleteBody { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    bind_body_recipe_body_selection(
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                } else {
                    bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            if let FeatureDefinition::Operation(FeatureOperation::Scale { bodies, .. }) =
                definition
            {
                if let Some(previous_state_id) = scope.previous_history_state_id() {
                    bind_body_recipe_body_selection(
                        bodies,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                    if matches!(bodies, BodySelection::Native(_)) {
                        bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                    }
                } else {
                    bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                }
                break 'feature_edit;
            }
            let (bodies, proof) = match definition {
                FeatureDefinition::Operation(FeatureOperation::MoveBody { bodies, .. }) => {
                    (bodies, BodySelectionProof::TopologyStableRevision)
                }
                FeatureDefinition::Operation(FeatureOperation::Shell {
                    bodies: Some(bodies),
                    ..
                }) => (bodies, BodySelectionProof::RevisedInput),
                FeatureDefinition::Operation(FeatureOperation::SplitBody { targets, .. }) => {
                    (targets, BodySelectionProof::RevisedInput)
                }
                _ => break 'feature_edit,
            };
            let BodySelection::Native(group_id) = bodies else {
                break 'feature_edit;
            };
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && group.role() == DesignOperandRole::BODIES_A
                    && crate::ids::native_stream(&group.id) == crate::ids::native_stream(&scope.id)
            });
            let Some(group) = matching_groups.next() else {
                break 'feature_edit;
            };
            if matching_groups.next().is_some() || group.members().len() != 1 {
                break 'feature_edit;
            }
            let (Some(state_id), Some(previous_state_id)) =
                (scope.history_state_id(), scope.previous_history_state_id())
            else {
                bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                break 'feature_edit;
            };
            let Some((history, state, _previous)) =
                unique_history_state_pair(histories, state_id, previous_state_id)
            else {
                bind_direct_body_recipe_body_selection(bodies, scope, inputs, &mut body_candidates);
                break 'feature_edit;
            };
            let history_states = unique_feature_history_states(history);
            let body = match proof {
                BodySelectionProof::TopologyStableRevision => {
                    singleton_body_revision_across_state_chain(history, state, previous_state_id, &history_states)
                }
                BodySelectionProof::RevisedInput => {
                    singleton_revised_input_body_across_state_chain(history, state, previous_state_id, &history_states)
                }
            };
            let Some(body) = body else {
                break 'feature_edit;
            };
            let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
            let state_id = crate::ids::history_input_state_id(&prefix);
            let body_id = crate::ids::history_input_body_id(&prefix, body);
            let native = group_id.to_owned();
            let selected = vec![body_id];
            if let Ok(historical) = cadmpeg_ir::features::BodySelection::historical(state_id, selected, native) {
                *bodies = historical;
            }
        }
        });
    }

    losses
}

fn combine_historical_rows(
    prefix: &cadmpeg_ir::ids::IdentityKey,
    slots: Vec<i64>,
    native_tools: Vec<String>,
) -> Option<Vec<cadmpeg_ir::features::BodyMember<cadmpeg_ir::ids::HistoricalBodyId>>> {
    let mut rows = Vec::new();
    for (slot, native) in slots.into_iter().zip(native_tools) {
        let body = crate::ids::history_input_body_id(prefix, slot);
        let row = body_member(body, native)?;

        rows.push(row);
    }
    Some(rows)
}

fn pattern_combine_tool_slots(
    pattern_bodies: &BTreeSet<i64>,
    target_body: i64,
    native_tool_count: usize,
) -> Option<Vec<i64>> {
    if !pattern_bodies.contains(&target_body)
        || pattern_bodies.len().checked_sub(1) != Some(native_tool_count)
    {
        return None;
    }
    Some(
        pattern_bodies
            .iter()
            .copied()
            .filter(|body| *body != target_body)
            .collect::<Vec<_>>(),
    )
}

fn combine_recipe_family_tool_slots(
    scope_key: (&str, u32),
    tool_record_indices: &[u32],
    previous_state_id: i64,
    target_body: i64,
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
) -> Option<Vec<i64>> {
    macro_rules! required {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return None,
            }
        };
    }
    type FamilyKey<'a> = (
        &'a crate::records::mesh::DesignRelaxedGuidText,
        &'a crate::records::mesh::DesignRelaxedGuidText,
        u64,
        u32,
        &'a str,
    );
    type FamilyMember<'a> = (u32, Option<i64>, &'a [i64]);
    let (stream, scope_record_index) = scope_key;

    if tool_record_indices.is_empty() {
        return None;
    }
    let mut seen = HashSet::new();
    for index in tool_record_indices {
        if !seen.insert(*index) {
            return None;
        }
    }
    let mut recipes_by_id =
        HashMap::<&str, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes.iter().filter(|recipe| {
        recipe.kind == crate::records::recipes::ConstructionRecipeKind::Body
            && crate::ids::native_stream(&recipe.id) == Some(stream)
    }) {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(recipe));
    }
    let mut families = BTreeMap::<FamilyKey<'_>, Vec<FamilyMember<'_>>>::new();
    for record_index in tool_record_indices {
        let mut matching = operands.iter().filter(|operand| {
            crate::ids::native_stream(&operand.id) == Some(stream)
                && operand.scope_record_index == scope_record_index
                && matches!(
                    operand.owner,
                    crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
                )
                && operand.record_index() == *record_index
        });
        let operand = required!(matching.next());
        if matching.next().is_some() || operand.references().len() != 1 {
            return None;
        }
        let recipe = required!(required!(recipes_by_id.get(operand.recipe_id.as_str())).as_ref());
        let design = required!(recipe.design.as_ref());
        let selector = required!(design.selector).value;
        if selector == 0 {
            return None;
        }
        let resolved = match (operand.resolved_body_state_id, operand.resolved_body_slot) {
            (Some(state), Some(body)) if state == previous_state_id => Some(body),
            (None, None) => None,
            _ => return None,
        };
        let reference = &operand.references()[0];
        let key = (
            &operand.asset_id,
            &operand.context_id,
            reference.design_reference,
            reference.form,
            design.id.value.as_str(),
        );
        families.entry(key).or_default().push((
            selector,
            resolved,
            reference.preceding_body_slots.as_slice(),
        ));
    }

    let mut selected = BTreeSet::new();
    for family in families.into_values() {
        let mut exact = BTreeSet::new();
        for body in family.iter().filter_map(|(_, body, _)| *body) {
            exact.insert(body);
        }
        if family.iter().all(|(_, body, _)| body.is_some()) {
            if exact.len() != family.len() {
                return None;
            }
            for body in exact {
                selected.insert(body);
            }
            continue;
        }
        let mut selectors = BTreeSet::new();
        for (selector, _, _) in &family {
            selectors.insert(*selector);
        }
        let count = required!(u32::try_from(family.len()).ok());
        if selectors.len() != family.len()
            || !(1..=count).all(|selector| selectors.contains(&selector))
        {
            return None;
        }
        let mut candidate_sets = family
            .iter()
            .map(|(_, _, candidates)| candidates)
            .filter(|candidates| !candidates.is_empty());
        let candidate_slice = required!(candidate_sets.next());
        let mut candidates = BTreeSet::new();
        for candidate in *candidate_slice {
            candidates.insert(*candidate);
        }
        if candidates.len() != family.len()
            || candidates.contains(&target_body)
            || !exact.is_subset(&candidates)
            || candidate_sets.any(|other| {
                other.iter().any(|body| !candidates.contains(body))
                    || candidates.iter().any(|body| !other.contains(body))
            })
        {
            return None;
        }
        for body in candidates {
            selected.insert(body);
        }
    }
    if selected.len() != tool_record_indices.len() || selected.contains(&target_body) {
        return None;
    }
    Some(selected.into_iter().collect::<Vec<_>>())
}

/// Bind an operand-list `Combine` through the browser node of each operand.
///
/// Leaves both selections native unless every node names a distinct body of the
/// preceding history state.
fn bind_operand_list_combine_bodies(
    target: &mut cadmpeg_ir::features::BodySelection,
    tools: &mut cadmpeg_ir::features::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    operation: &crate::records::feature::combine::DesignCombineOperation,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    (histories, scopes): (
        &[AsmHistory],
        &[crate::records::feature::scope::DesignParameterScope],
    ),
) -> Option<()> {
    use cadmpeg_ir::features::{BodyMembers, BodySelection};

    let stream = crate::ids::native_stream(&scope.id)?;
    let (history, _, previous) =
        unique_history_state_pair(histories, scope.history_state_id()?, previous_state_id)?;
    let input = history.topology(previous)?;
    let body_slot = |node: Option<crate::records::feature::combine::DesignCombineBodyNode>| {
        let node = node?;
        let creator = scopes.iter().find(|candidate| {
            candidate.record_index == node.creating_scope_record_index
                && crate::ids::native_stream(&candidate.id) == Some(stream)
        })?;
        let creator_state = creator.history_state_id()?;
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == creator_state);
        let (Some(state), None) = (states.next(), states.next()) else {
            return None;
        };
        let [slot] = state.transition.as_ref()?.topology.bodies.inserted[..] else {
            return None;
        };
        input.bodies.contains(&slot).then_some(slot)
    };
    let target_slot = body_slot(operation.target_body_node)?;
    let mut tool_slots = Vec::new();
    for tool in operation.tools.iter() {
        let slot = body_slot(tool.body_node)?;
        if slot == target_slot {
            return None;
        }
        tool_slots.push(slot);
    }
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let design_record = |record_index| format!("{stream}:design-record#{record_index}");
    let mut rows = Vec::new();
    for (slot, tool) in tool_slots.into_iter().zip(operation.tools.iter()) {
        rows.push(body_member(
            crate::ids::history_input_body_id(&prefix, slot),
            design_record(tool.record_index),
        )?);
    }
    let members = BodyMembers::try_from_rows(rows).ok()?;
    let target_selection = BodySelection::historical(
        crate::ids::history_input_state_id(&prefix),
        vec![crate::ids::history_input_body_id(&prefix, target_slot)],
        design_record(operation.target_record_index),
    )
    .ok()?;
    *target = target_selection;
    *tools = BodySelection::HistoricalSet {
        state: crate::ids::history_input_state_id(&prefix),
        members,
    };
    Some(())
}

fn combine_external_local_tools(
    scope: &crate::records::feature::scope::DesignParameterScope,
) -> Option<cadmpeg_ir::features::BodySelection> {
    let operation = scope.combine_operation()?;
    let mut bodies = Vec::new();
    for tool in operation.tools.iter() {
        let identity = tool.external_identity.as_ref()?;
        let id = crate::ids::neutral_combine_external_body_id(identity);

        if bodies.iter().any(|body| body == &id) {
            return None;
        }
        bodies.push(id);
    }
    let native = scope.id.clone();
    cadmpeg_ir::features::BodySelection::local(bodies, native).ok()
}

fn historical_body_slot(id: &str) -> Option<i64> {
    id.strip_prefix("f3d:history-input:body#")?
        .rsplit_once(':')?
        .1
        .parse()
        .ok()
}

fn bind_pattern_body_selections<'a>(
    features: &mut [cadmpeg_ir::features::Feature],
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) {
    use cadmpeg_ir::features::{
        patterns::PatternSeed, BodySelection, FeatureDefinition, FeatureOperation,
    };

    let scopes = inputs.scopes;
    let groups = inputs.groups;
    let body_recipe_operands = inputs.body_recipe_operands;

    for feature in features {
        let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) =
            feature.evaluation.definition()
        else {
            continue;
        };
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let stream = crate::ids::native_stream(&scope.id);
        let mut matching_groups = groups.iter().filter(|group| {
            group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::BODIES_B
                && !group.members().is_empty()
                && crate::ids::native_stream(&group.id) == stream
        });
        let Some(group) = matching_groups.next() else {
            continue;
        };
        if matching_groups.next().is_some() {
            continue;
        }
        let mut seed = if seeds.is_empty() {
            Some(PatternSeed::Bodies(BodySelection::Native(group.id.clone())))
        } else {
            None
        };
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| {
            let FeatureDefinition::Operation(FeatureOperation::Pattern { seeds, .. }) = definition
            else {
                return;
            };
            if let Some(seed) = seed.take() {
                seeds.push(seed);
            }
            let [PatternSeed::Bodies(selection)] = seeds.as_mut_slice() else {
                return;
            };
            if let Some(previous_state_id) = scope.previous_history_state_id() {
                bind_body_recipe_body_selection(
                    selection,
                    feature_id,
                    previous_state_id,
                    scope,
                    groups,
                    body_recipe_operands,
                );
            } else {
                bind_direct_body_recipe_body_selection(selection, scope, inputs, body_candidates);
            }
        });
    }
}

fn bind_body_recipe_body_selection(
    selection: &mut cadmpeg_ir::features::BodySelection,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
) {
    use cadmpeg_ir::features::BodySelection;

    let BodySelection::Native(group_id) = selection else {
        return;
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && matches!(
                group.role(),
                DesignOperandRole::BODIES_A
                    | DesignOperandRole::ROLE_0X5
                    | DesignOperandRole::BODIES_B
            )
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    let mut body_slots = Vec::new();
    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.owner.group() == Some((group.record_index, ordinal))
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(body_slot) = operand.resolved_body_slot else {
            return;
        };
        if !body_slots.contains(&body_slot) {
            body_slots.push(body_slot);
        }
    }

    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let mut body_ids = Vec::new();

    for slot in body_slots {
        body_ids.push(crate::ids::history_input_body_id(&prefix, slot));
    }
    let state = crate::ids::history_input_state_id(&prefix);
    let native = group.id.clone();
    if let Ok(historical) = cadmpeg_ir::features::BodySelection::historical(state, body_ids, native)
    {
        *selection = historical;
    }
}

fn direct_design_record_body<'a>(
    native: &str,
    scope: &crate::records::feature::scope::DesignParameterScope,
    stream: Option<&str>,
    operands: &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    body_candidates: &mut BodyCandidates<'a>,
) -> Option<cadmpeg_ir::features::BodyMember<cadmpeg_ir::ids::BodyId>> {
    let (native_stream_name, record_index) = native.rsplit_once(":design-record#")?;
    let record_index = record_index.parse::<u32>().ok()?;
    if Some(native_stream_name) != stream {
        return None;
    }
    let mut matching_operands = operands.iter().filter(|operand| {
        crate::ids::native_stream(&operand.id) == Some(native_stream_name)
            && operand.scope_record_index == scope.record_index
            && matches!(
                operand.owner,
                crate::records::topology::body_recipe::DesignOperandOwner::ScopeReference { .. }
            )
            && operand.record_index() == record_index
    });
    let operand = matching_operands.next()?;
    if matching_operands.next().is_some() {
        return None;
    }
    let body = body_candidates.direct(operand)?;
    body_member(body, native.to_owned())
}

fn bind_direct_body_recipe_body_selection<'a>(
    selection: &mut cadmpeg_ir::features::BodySelection,
    scope: &crate::records::feature::scope::DesignParameterScope,
    inputs: &FeatureBodySelectionInputs<'a>,
    body_candidates: &mut BodyCandidates<'a>,
) {
    use cadmpeg_ir::features::{BodyMember, BodySelection};

    let groups = inputs.groups;
    let operands = inputs.body_recipe_operands;

    let stream = crate::ids::native_stream(&scope.id);
    let native_members = match selection {
        BodySelection::Native(group_id) => {
            let mut matching_groups = groups.iter().filter(|group| {
                group.id == *group_id
                    && group.scope_record_index == scope.record_index
                    && matches!(
                        group.role(),
                        DesignOperandRole::BODIES_A
                            | DesignOperandRole::ROLE_0X5
                            | DesignOperandRole::BODIES_B
                    )
                    && crate::ids::native_stream(&group.id) == stream
            });
            match (matching_groups.next(), matching_groups.next()) {
                (Some(group), None) if !group.members().is_empty() => {
                    let mut selected = Vec::new();
                    for (ordinal, record_index) in group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .enumerate()
                    {
                        let Ok(ordinal) = u32::try_from(ordinal) else {
                            return;
                        };
                        let mut matching_operands = operands.iter().filter(|operand| {
                            operand.owner.group() == Some((group.record_index, ordinal))
                                && operand.record_index() == record_index
                                && crate::ids::native_stream(&operand.id) == stream
                        });
                        let Some(operand) = matching_operands.next() else {
                            return;
                        };
                        if matching_operands.next().is_some() {
                            return;
                        }
                        let Some(body) = body_candidates.direct(operand) else {
                            return;
                        };
                        if selected.contains(&body) {
                            return;
                        }

                        selected.push(body);
                    }
                    let Ok(bodies) = cadmpeg_ir::features::DistinctMembers::try_from(selected)
                    else {
                        return;
                    };
                    let native = group.id.clone();
                    *selection = BodySelection::Resolved { bodies, native };
                    return;
                }
                _ => {
                    // A single design-record identity names no operand group.
                    // Resolve it through the same per-record lookup the
                    // multi-member form uses.
                    let Some(row) = direct_design_record_body(
                        group_id,
                        scope,
                        stream,
                        operands,
                        body_candidates,
                    ) else {
                        return;
                    };
                    let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(vec![row])
                    else {
                        return;
                    };
                    *selection = BodySelection::ResolvedSet { members };
                    return;
                }
            }
        }
        BodySelection::NativeSet(native) => native.iter().map(String::as_str).collect::<Vec<_>>(),
        _ => return,
    };
    if native_members.is_empty() {
        return;
    }
    for (index, native) in native_members.iter().enumerate() {
        for previous in &native_members[..index] {
            if native == previous {
                return;
            }
        }
    }
    let mut rows = Vec::new();
    for native in &native_members {
        let Some(row) = direct_design_record_body(native, scope, stream, operands, body_candidates)
        else {
            return;
        };
        if rows
            .iter()
            .any(|existing: &BodyMember<_>| existing.body() == row.body())
        {
            return;
        }
        rows.push(row);
    }
    if let Ok(members) = cadmpeg_ir::features::BodyMembers::try_from_rows(rows) {
        *selection = BodySelection::ResolvedSet { members };
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum BodySelectionProof {
    TopologyStableRevision,
    RevisedInput,
}

fn unique_feature_history_states(history: &AsmHistory) -> HashMap<i64, Option<&AsmDeltaState>> {
    let mut states = HashMap::new();
    for state in history.states() {
        states
            .entry(state.state_id)
            .and_modify(|existing| *existing = None)
            .or_insert(Some(state));
    }
    states
}

fn singleton_revised_input_body_across_state_chain<'a>(
    history: &AsmHistory,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut revised = BTreeSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for &body in transition
            .topology
            .bodies
            .updated
            .iter()
            .chain(&transition.topology.bodies.deleted)
        {
            revised.insert(body);
        }
        let previous_id = transition.previous_state_id?;
        let Some(Some(previous)) = states.get(&previous_id) else {
            return None;
        };
        current = previous;
    }
    let input = history.topology(current)?;
    let mut candidates = input.bodies.iter().filter(|body| revised.contains(body));
    let body = candidates.next().copied()?;
    candidates.next().is_none().then_some(body)
}

fn singleton_body_revision_across_state_chain<'a>(
    history: &AsmHistory,
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<i64> {
    let result_topology = history.topology(state)?;
    let mut current = state;
    let mut visited = HashSet::new();
    let mut selected = None;
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let revision = body_revision_without_topology_change(current)?;
        if let TopologyStableBodyRevision::Revised(body) = revision {
            match selected {
                None => selected = Some(body),
                Some(selected) if selected == body => {}
                Some(_) => return None,
            }
        }
        let previous = current
            .transition
            .as_ref()
            .and_then(|transition| transition.previous_state_id)?;
        let Some(Some(previous)) = states.get(&previous) else {
            return None;
        };
        current = previous;
    }
    let body = selected?;
    let input_topology = history.topology(current)?;
    (result_topology.bodies.contains(&body) && input_topology.bodies.contains(&body))
        .then_some(body)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum TopologyStableBodyRevision {
    Unchanged,
    Revised(i64),
}

fn body_revision_without_topology_change(
    current: &AsmDeltaState,
) -> Option<TopologyStableBodyRevision> {
    let transition = current.transition.as_ref()?;
    let delta = &transition.topology;
    let body = match delta.bodies.updated.as_slice() {
        [] => TopologyStableBodyRevision::Unchanged,
        [body] => TopologyStableBodyRevision::Revised(*body),
        _ => return None,
    };
    if !delta.bodies.inserted.is_empty()
        || !delta.bodies.deleted.is_empty()
        || [
            &delta.regions,
            &delta.shells,
            &delta.faces,
            &delta.loops,
            &delta.coedges,
            &delta.edges,
            &delta.vertices,
        ]
        .into_iter()
        .any(|family| {
            !family.inserted.is_empty() || !family.deleted.is_empty() || !family.updated.is_empty()
        })
    {
        return None;
    }
    Some(body)
}

pub(crate) struct FeatureFaceSelectionInputs<'a> {
    pub(crate) scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    pub(crate) groups:
        &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
    pub(crate) operands: &'a [crate::records::topology::face::DesignFaceOperand],
    pub(crate) entity_operands:
        &'a [crate::records::topology::entity_selection::DesignEntitySelectionOperand],
    pub(crate) body_recipe_operands:
        &'a [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    pub(crate) histories: &'a mut [AsmHistory],
}

pub(crate) fn bind_feature_face_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
    input: FeatureFaceSelectionInputs<'_>,
) {
    let FeatureFaceSelectionInputs {
        scopes,
        groups,
        operands,
        entity_operands,
        body_recipe_operands,
        histories,
    } = input;
    let selection_states = features
        .iter()
        .map(|feature| face_selection_state(feature, scopes, histories))
        .collect::<Vec<_>>();
    let gates = features
        .iter()
        .zip(&selection_states)
        .enumerate()
        .filter(|(_, (feature, _))| face_selection_reads_state(feature.evaluation.definition()))
        .filter_map(|(position, (_, state))| {
            state.map(|state| (state.history, state.previous, position))
        })
        .collect::<Vec<_>>();
    let mut available = HashSet::new();
    visit_states_oldest_first(histories, gates, |history, previous, position| {
        if history.topology(previous).is_some() {
            available.insert(position);
        }
    });
    let histories = &*histories;
    for ((feature_position, feature), selection_state) in
        features.iter_mut().enumerate().zip(selection_states)
    {
        let selection_state = selection_state.filter(|_| available.contains(&feature_position));
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(FaceSelectionState {
                scope,
                previous_state_id,
                history,
                state,
                previous: selection_previous,
                ..
            }) = selection_state
            else {
                break 'feature_edit;
            };
            let Some(history) = histories.get(history) else {
                break 'feature_edit;
            };
            let (Some(selection_state_row), Some(selection_previous_row)) = (
                history.states().get(state),
                history.states().get(selection_previous),
            ) else {
                break 'feature_edit;
            };
            let Some(transition) = selection_state_row.transition.as_ref() else {
                break 'feature_edit;
            };
            match definition {
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Extrude { start, extent, .. },
                ) => {
                    if let cadmpeg_ir::features::ExtrudeStart::FromFace { face, .. } = start {
                        selection::bind_face_selection(
                            face,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        );
                        bind_entity_face_selection(
                            face,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                            groups,
                            entity_operands,
                        );
                    }
                    let sides = match extent {
                        cadmpeg_ir::features::ExtrudeExtent::OneSided { side }
                        | cadmpeg_ir::features::ExtrudeExtent::Symmetric { side } => vec![side],
                        cadmpeg_ir::features::ExtrudeExtent::TwoSided { first, second } => {
                            vec![first, second]
                        }
                    };
                    for side in sides {
                        if let cadmpeg_ir::features::LinearTermination::ToFace { face, .. } =
                            &mut side.termination
                        {
                            selection::bind_face_selection(
                                face,
                                scope,
                                groups,
                                operands,
                                &transition.topology.faces.updated,
                            );
                        }
                    }
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Pattern { seeds, .. },
                ) => {
                    for seed in seeds {
                        let cadmpeg_ir::features::patterns::PatternSeed::Faces(faces) = seed else {
                            continue;
                        };
                        selection::bind_face_selection(
                            faces,
                            scope,
                            groups,
                            operands,
                            &transition.topology.faces.updated,
                        );
                        bind_entity_face_selection(
                            faces,
                            FaceSelectionBinding {
                                feature_id,
                                previous_state_id,
                                operation_history_id: &history.id,
                                scope,
                                input_topologies: &mut *input_topologies,
                            },
                            groups,
                            entity_operands,
                        );
                    }
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::MoveFace { faces, motion },
                ) => {
                    move_face::bind_move_face_selection(
                        faces,
                        motion,
                        feature_id,
                        scope,
                        groups,
                        &move_face::MoveFaceStates {
                            histories,
                            history,
                            state: selection_state_row,
                            previous: selection_previous_row,
                        },
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Thicken { faces, .. },
                ) => {
                    selection::bind_face_selection(
                        faces,
                        scope,
                        groups,
                        operands,
                        &transition.topology.faces.updated,
                    );
                    selection::bind_body_recipe_face_selection(
                        faces,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        body_recipe_operands,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Shell {
                        removed_faces,
                        thickness: Some(thickness),
                        mode,
                        ..
                    },
                ) => {
                    shell_walls::bind_shell_removed_faces(
                        removed_faces,
                        feature_id,
                        scope,
                        groups,
                        &shell_walls::ShellWalls {
                            thickness: thickness.get(),
                            mode: *mode,
                            previous_state_id,
                            transition,
                            input: history.topology(selection_previous_row),
                            output: history.topology(selection_state_row),
                        },
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::KnitSurface { faces, .. },
                ) => {
                    bind_surface_stitch_face_selection(
                        faces,
                        FaceSelectionBinding {
                            feature_id,
                            previous_state_id,
                            operation_history_id: &history.id,
                            scope,
                            input_topologies: &mut *input_topologies,
                        },
                        groups,
                        entity_operands,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::SplitFace { targets, .. },
                ) => {
                    selection::bind_face_selection(
                        targets,
                        scope,
                        groups,
                        operands,
                        &transition.topology.faces.updated,
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::DeleteFace { faces, .. },
                ) => {
                    delete_face::bind_delete_face_selection(
                        faces,
                        feature_id,
                        scope,
                        groups,
                        previous_state_id,
                        transition,
                        history
                            .states()
                            .get(selection_previous)
                            .and_then(|state| history.topology(state)),
                    );
                }
                cadmpeg_ir::features::FeatureDefinition::Operation(
                    cadmpeg_ir::features::FeatureOperation::Hole {
                        face: Some(face), ..
                    },
                ) => {
                    bind_hole_face_selection(
                        face,
                        FaceSelectionBinding {
                            feature_id,
                            previous_state_id,
                            operation_history_id: &history.id,
                            scope,
                            input_topologies: &mut *input_topologies,
                        },
                        transition,
                        history
                            .states()
                            .get(selection_previous)
                            .and_then(|state| history.topology(state)),
                    );
                }
                _ => {}
            }
        });
    }
}

#[derive(Clone, Copy)]
struct FaceSelectionState<'a> {
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    previous_state_id: i64,
    history: usize,
    state: usize,
    previous: usize,
}

fn face_selection_state<'a>(
    feature: &cadmpeg_ir::features::Feature,
    scopes: &'a [crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Option<FaceSelectionState<'a>> {
    let native_ref = feature.native_ref.as_deref()?;
    let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    let spans_transitions = matches!(
        feature.evaluation.definition(),
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::MoveFace { .. }
        )
    );
    if !spans_transitions && state.transition.as_ref()?.previous_state_id != Some(previous_state_id)
    {
        return None;
    }
    let (history_index, state_index) = history_state_indexes(histories, history, state)?;
    let (_, previous_index) = history_state_indexes(histories, history, previous)?;
    Some(FaceSelectionState {
        scope,
        previous_state_id,
        history: history_index,
        state: state_index,
        previous: previous_index,
    })
}

fn face_selection_reads_state(definition: &cadmpeg_ir::features::FeatureDefinition) -> bool {
    matches!(
        definition,
        cadmpeg_ir::features::FeatureDefinition::Operation(
            cadmpeg_ir::features::FeatureOperation::Extrude { .. }
                | cadmpeg_ir::features::FeatureOperation::Pattern { .. }
                | cadmpeg_ir::features::FeatureOperation::MoveFace { .. }
                | cadmpeg_ir::features::FeatureOperation::Thicken { .. }
                | cadmpeg_ir::features::FeatureOperation::Shell { .. }
                | cadmpeg_ir::features::FeatureOperation::KnitSurface { .. }
                | cadmpeg_ir::features::FeatureOperation::SplitFace { .. }
                | cadmpeg_ir::features::FeatureOperation::Hole { face: Some(_), .. }
                | cadmpeg_ir::features::FeatureOperation::DeleteFace { heal: true, .. }
        )
    )
}

struct FaceSelectionBinding<'a> {
    feature_id: &'a cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    operation_history_id: &'a str,
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    input_topologies: &'a mut [cadmpeg_ir::features::FeatureInputTopology],
}

fn bind_entity_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let group_id = match selection {
        FaceSelection::Native(group_id) => group_id.as_str(),
        _ => return,
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }
    bind_entity_face_groups(
        selection,
        &group.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        std::slice::from_ref(&group),
        operands,
    );
}

// Keep the serialized-selection context explicit at this boundary so every
// admission input remains visible to the strict all-members proof.
fn bind_surface_stitch_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;
    let native_id = match selection {
        cadmpeg_ir::features::FaceSelection::Native(native_id) => native_id.as_str(),
        _ => return,
    };
    if native_id != scope.id {
        return;
    }
    let Some(input_end) = scope.reference_members().len().checked_sub(2) else {
        return;
    };
    if input_end == 0 || !input_end.is_multiple_of(2) {
        return;
    }
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups
        .iter()
        .filter(|group| {
            crate::ids::native_stream(&group.id) == stream
                && group.scope_record_index == scope.record_index
                && group.role() == DesignOperandRole::ROLE_0X5
                && group.extrude_role().is_none()
                && group.extrude_face_role().is_none()
        })
        .collect::<Vec<_>>();
    matching_groups.sort_by_key(|a| a.scope_reference_ordinal);
    if matching_groups.len().checked_mul(2) != Some(input_end)
        || matching_groups
            .iter()
            .enumerate()
            .zip(
                scope
                    .reference_members()
                    .values()
                    .step_by(2)
                    .zip(scope.reference_members().values().skip(1).step_by(2)),
            )
            .any(|((ordinal, group), (group_reference, member_reference))| {
                u32::try_from(ordinal * 2) != Ok(group.scope_reference_ordinal)
                    || group.record_index != *group_reference
                    || !group
                        .members()
                        .iter()
                        .map(|member| member.value)
                        .eq([*member_reference])
            })
    {
        return;
    }
    bind_entity_face_groups(
        selection,
        &scope.id,
        FaceSelectionBinding {
            feature_id,
            previous_state_id,
            operation_history_id,
            scope,
            input_topologies: &mut *input_topologies,
        },
        &matching_groups,
        operands,
    );
}

// Keep the shared selection-binding inputs explicit; this helper is the
// single admission point for both one-group and SurfaceStitch selections.
fn bind_entity_face_groups(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    native_id: &str,
    input: FaceSelectionBinding<'_>,
    groups: &[&crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    if groups.is_empty() {
        return;
    }
    let mut selected = Vec::<(&str, i64, bool)>::new();
    let stream = crate::ids::native_stream(&scope.id);
    for group in groups {
        if group.members().is_empty() {
            return;
        }
        for (ordinal, record_index) in group
            .members()
            .iter()
            .map(|member| member.value)
            .enumerate()
        {
            let Ok(ordinal) = u32::try_from(ordinal) else {
                return;
            };
            let mut matches = operands.iter().filter(|operand| {
                operand.scope_record_index == scope.record_index
                    && operand.group_record_index == group.record_index
                    && operand.group_member_ordinal == ordinal
                    && operand.record_index() == record_index
                    && crate::ids::native_stream(&operand.id) == stream
            });
            let Some(operand) = matches.next() else {
                return;
            };
            let [candidate] = operand.historical_face_candidates.as_slice() else {
                return;
            };
            if matches.next().is_some() {
                return;
            }
            let local = candidate.history_id == operation_history_id
                && candidate.historical.state_ids.contains(&previous_state_id);
            let Some(source) = historical_brep_source(&candidate.history_id) else {
                return;
            };
            if !selected.contains(&(source, candidate.face_slot, local)) {
                selected.push((source, candidate.face_slot, local));
            }
        }
    }
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let mut faces = Vec::new();
    for (source, face, local) in selected {
        let slot = if local {
            cadmpeg_ir::ids::IdentityKey::from(face)
        } else {
            let Ok(slot) =
                cadmpeg_ir::ids::IdentityKey::try_new(format!("{}:{source}:{face}", source.len()))
            else {
                return;
            };
            slot
        };
        let id = crate::ids::history_input_face_id(&prefix, slot);

        faces.push(id);
    }
    for face in &faces {
        if !topology.faces.contains(face) {
            let retained = face.clone();
            topology.faces.insert(retained);
        }
    }
    let native = native_id.to_owned();
    if let Ok(historical) = cadmpeg_ir::features::FaceSelection::historical(state_id, faces, native)
    {
        *selection = historical;
    }
}

fn bind_hole_face_selection(
    selection: &mut cadmpeg_ir::features::FaceSelection,
    input: FaceSelectionBinding<'_>,
    transition: &crate::history_records::AsmHistoricalTransition,
    preceding: Option<&crate::history_records::AsmHistoricalTopology>,
) {
    use cadmpeg_ir::features::FaceSelection;
    let FaceSelectionBinding {
        feature_id,
        previous_state_id,
        operation_history_id,
        scope,
        input_topologies,
    } = input;

    let FaceSelection::Native(native_id) = selection else {
        return;
    };
    let Some(construction) = scope.hole_construction() else {
        return;
    };
    if let Some(faces) = preceding
        .and_then(|preceding| hole_support::hole_support_faces(construction, transition, preceding))
    {
        let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
        let state_id = crate::ids::history_input_state_id(&prefix);
        let mut topologies = input_topologies
            .iter_mut()
            .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
        let Some(topology) = topologies.next() else {
            return;
        };
        if topologies.next().is_some() {
            return;
        }
        let selected: Vec<_> = faces
            .into_iter()
            .map(|slot| {
                crate::ids::history_input_face_id(&prefix, cadmpeg_ir::ids::IdentityKey::from(slot))
            })
            .collect();
        for face in &selected {
            if !topology.faces.contains(face) {
                topology.faces.insert(face.clone());
            }
        }
        if let Ok(historical) = cadmpeg_ir::features::FaceSelection::historical(
            state_id,
            selected,
            native_id.to_owned(),
        ) {
            *selection = historical;
        }
        return;
    }
    let Some(face_selection) = &construction.face_selection else {
        return;
    };
    let [candidate] = face_selection.historical_face_candidates.as_slice() else {
        return;
    };
    let local = candidate.history_id == operation_history_id;
    if local && !candidate.historical.state_ids.contains(&previous_state_id) {
        return;
    }
    let Some(source) = historical_brep_source(&candidate.history_id) else {
        return;
    };
    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let state_id = crate::ids::history_input_state_id(&prefix);
    let mut topologies = input_topologies
        .iter_mut()
        .filter(|topology| topology.id == state_id && topology.input_of == *feature_id);
    let Some(topology) = topologies.next() else {
        return;
    };
    if topologies.next().is_some() {
        return;
    }
    let slot = if local {
        cadmpeg_ir::ids::IdentityKey::from(candidate.face_slot)
    } else {
        let Ok(slot) = cadmpeg_ir::ids::IdentityKey::try_new(format!(
            "{}:{source}:{}",
            source.len(),
            candidate.face_slot
        )) else {
            return;
        };
        slot
    };
    let face = crate::ids::history_input_face_id(&prefix, slot);
    if !topology.faces.contains(&face) {
        let retained = face.clone();
        topology.faces.insert(retained);
    }
    let native = native_id.to_owned();
    let selected = vec![face];
    if let Ok(historical) =
        cadmpeg_ir::features::FaceSelection::historical(state_id, selected, native)
    {
        *selection = historical;
    }
}

pub(crate) fn bind_feature_path_selections(
    features: &mut [cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::{FeatureDefinition, FeatureOperation, SurfaceBoundary};

    for feature in features {
        let native_ref = feature.native_ref.as_deref();
        let feature_id = &feature.id;
        feature.evaluation.edit(|definition, _| 'feature_edit: {
            let Some(native_ref) = native_ref else {
                break 'feature_edit;
            };
            let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
            let Some(scope) = matching_scopes.next() else {
                break 'feature_edit;
            };
            if matching_scopes.next().is_some() {
                break 'feature_edit;
            }
            let Some(previous_state_id) = scope.previous_history_state_id() else {
                break 'feature_edit;
            };
            match definition {
                FeatureDefinition::Operation(FeatureOperation::FilledSurface {
                    boundary: SurfaceBoundary::Path(path),
                    ..
                }) => {
                    bind_entity_selection_path(
                        path,
                        feature_id,
                        previous_state_id,
                        scope,
                        groups,
                        operands,
                    );
                }
                FeatureDefinition::Operation(FeatureOperation::Loft { guidance, .. }) => {
                    let paths = match guidance {
                        cadmpeg_ir::features::LoftGuidance::Guides(paths) => paths,
                        cadmpeg_ir::features::LoftGuidance::Centerline(path) => {
                            std::slice::from_mut(path)
                        }
                    };
                    for path in paths {
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                FeatureDefinition::Operation(FeatureOperation::Sweep {
                    path, guide_rail, ..
                }) => {
                    if let Some(path) = path {
                        bind_entity_selection_path(
                            path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                    if let Some(guide_rail) = guide_rail {
                        bind_entity_selection_path(
                            &mut guide_rail.path,
                            feature_id,
                            previous_state_id,
                            scope,
                            groups,
                            operands,
                        );
                    }
                }
                _ => {}
            }
        });
    }
}

fn bind_entity_selection_path(
    path: &mut cadmpeg_ir::features::PathRef,
    feature_id: &cadmpeg_ir::features::FeatureId,
    previous_state_id: i64,
    scope: &crate::records::feature::scope::DesignParameterScope,
    groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    operands: &[crate::records::topology::entity_selection::DesignEntitySelectionOperand],
) {
    use cadmpeg_ir::features::PathRef;

    let PathRef::Native(group_id) = path else {
        return;
    };
    let stream = crate::ids::native_stream(&scope.id);
    let mut matching_groups = groups.iter().filter(|group| {
        group.id == *group_id
            && group.scope_record_index == scope.record_index
            && crate::ids::native_stream(&group.id) == stream
    });
    let Some(group) = matching_groups.next() else {
        return;
    };
    if matching_groups.next().is_some() || group.members().is_empty() {
        return;
    }

    let mut edge_slots = Vec::new();

    for (ordinal, record_index) in group
        .members()
        .iter()
        .map(|member| member.value)
        .enumerate()
    {
        let Ok(ordinal) = u32::try_from(ordinal) else {
            return;
        };
        let mut matching_operands = operands.iter().filter(|operand| {
            operand.group_record_index == group.record_index
                && operand.group_member_ordinal == ordinal
                && operand.record_index() == record_index
                && crate::ids::native_stream(&operand.id) == stream
        });
        let Some(operand) = matching_operands.next() else {
            return;
        };
        if matching_operands.next().is_some() {
            return;
        }
        let Some(edge_slot) = operand.resolved_edge_slot else {
            return;
        };
        edge_slots.push(edge_slot);
    }

    let prefix = crate::ids::feature_input_prefix(feature_id, previous_state_id);
    let mut edge_ids = Vec::new();

    for slot in edge_slots {
        edge_ids.push(crate::ids::history_input_edge_id(&prefix, slot));
    }
    let state = crate::ids::history_input_state_id(&prefix);
    let native = group.id.clone();
    if let Ok(historical) = PathRef::historical_edges(state, edge_ids, native) {
        *path = historical;
    }
}

/// History access for a pass that visits states oldest first.
pub(crate) enum BindingHistories<'a> {
    /// Keep only the visited state materialized between visits, advancing the
    /// previous visit's materialization to it. A visit reads the visited
    /// state or newer ones.
    Release(&'a mut [AsmHistory]),
    /// Leave materializations in place for a caller that holds the histories
    /// shared.
    Shared(&'a [AsmHistory]),
}

impl BindingHistories<'_> {
    fn get(&self) -> &[AsmHistory] {
        match self {
            Self::Release(histories) => histories,
            Self::Shared(histories) => histories,
        }
    }

    fn reborrow(&mut self) -> BindingHistories<'_> {
        match self {
            Self::Release(histories) => BindingHistories::Release(histories),
            Self::Shared(histories) => BindingHistories::Shared(histories),
        }
    }

    fn advance(&mut self, history_index: usize, state_index: usize) {
        if let Self::Release(histories) = self {
            if let Some(history) = histories.get_mut(history_index) {
                history.advance_topology(state_index);
            }
        }
    }

    fn retain(&mut self, history_index: usize, state_index: usize) {
        if let Self::Release(histories) = self {
            if let Some(history) = histories.get_mut(history_index) {
                history.retain_topology(state_index);
            }
        }
    }
}

/// Visit the jobs of each `(history index, state index)` group, oldest state
/// first within each history. Jobs keep their relative order within a group.
fn visit_state_groups_oldest_first<J>(
    mut histories: BindingHistories<'_>,
    mut jobs: Vec<(usize, usize, J)>,
    mut visit: impl FnMut(&[AsmHistory], usize, usize, Vec<J>),
) {
    let orders = histories
        .get()
        .iter()
        .map(AsmHistory::states_oldest_first)
        .collect::<Vec<_>>();
    jobs.sort_by_cached_key(|&(history_index, state_index, _)| {
        let position = orders
            .get(history_index)
            .and_then(Option::as_ref)
            .and_then(|order| order.iter().position(|&index| index == state_index));
        (history_index, position.is_none(), position)
    });
    let mut jobs = jobs.into_iter().peekable();
    while let Some((history_index, state_index, job)) = jobs.next() {
        let mut group = vec![job];
        while let Some((_, _, job)) =
            jobs.next_if(|&(history, state, _)| (history, state) == (history_index, state_index))
        {
            group.push(job);
        }
        histories.advance(history_index, state_index);
        visit(histories.get(), history_index, state_index, group);
        histories.retain(history_index, state_index);
    }
}

/// Visit `(history index, state index, job)` entries oldest first within each
/// history and keep only the visited state materialized after each visit.
fn visit_states_oldest_first<J>(
    histories: &mut [AsmHistory],
    jobs: Vec<(usize, usize, J)>,
    mut visit: impl FnMut(&AsmHistory, &AsmDeltaState, J),
) {
    visit_state_groups_oldest_first(
        BindingHistories::Release(histories),
        jobs,
        |histories, history_index, state_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(state) = history.states().get(state_index) else {
                return;
            };
            for job in group {
                visit(history, state, job);
            }
        },
    );
}

fn history_state_indexes(
    histories: &[AsmHistory],
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Option<(usize, usize)> {
    let history_index = histories
        .iter()
        .position(|item| std::ptr::eq(item, history))?;
    let state_index = history
        .states()
        .iter()
        .position(|item| std::ptr::eq(item, state))?;
    Some((history_index, state_index))
}

pub(crate) fn project_feature_input_topologies(
    features: &[cadmpeg_ir::features::Feature],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &mut [AsmHistory],
    edge_operands: &[crate::records::topology::edge_identity::DesignEdgeOperand],
    scope_histories: &HashMap<String, String>,
) -> Vec<cadmpeg_ir::features::FeatureInputTopology> {
    let mut jobs = Vec::new();
    for (feature_position, feature) in features.iter().enumerate() {
        let histories = &*histories;
        let Some(native_ref) = feature.native_ref.as_deref() else {
            continue;
        };
        let mut matching_scopes = scopes.iter().filter(|scope| scope.id == native_ref);
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(previous_state_id) = scope
            .previous_history_state_id()
            .or_else(|| {
                crate::design::feature_project::work_point_recipe_state_id(scope, edge_operands)
            })
            .or_else(|| crate::design::feature_project::work_plane_recipe_state_id(scope))
            .or_else(|| {
                crate::design::edge_resolve::edge_treatment_recipe_state_id(scope, edge_operands)
            })
            .or_else(|| effective_scope_previous_history_state_id(scope, scoped_histories))
        else {
            continue;
        };
        let Some((history, state)) = scope
            .history_state_id()
            .and_then(|state_id| {
                unique_history_state_pair(scoped_histories, state_id, previous_state_id)
                    .map(|(history, _, previous)| (history, previous))
            })
            .or_else(|| unique_history_state(scoped_histories, previous_state_id))
        else {
            continue;
        };
        let Some((history_index, state_index)) = history_state_indexes(histories, history, state)
        else {
            continue;
        };
        jobs.push((
            history_index,
            state_index,
            (feature_position, previous_state_id),
        ));
    }
    let mut projected = Vec::new();
    visit_states_oldest_first(
        histories,
        jobs,
        |history, state, (feature_position, previous_state_id)| {
            let Some(feature) = features.get(feature_position) else {
                return;
            };
            if let Some(input) = project_input_topology(feature, history, state, previous_state_id)
            {
                projected.push((feature_position, input));
            }
        },
    );
    projected.sort_by_key(|(position, _)| *position);
    projected.into_iter().map(|(_, input)| input).collect()
}

fn project_input_topology(
    feature: &cadmpeg_ir::features::Feature,
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> Option<cadmpeg_ir::features::FeatureInputTopology> {
    let topology = history.topology(state)?;
    let prefix = crate::ids::feature_input_prefix(&feature.id, previous_state_id);
    let bodies = project_input_members(&topology.bodies, |slot| {
        crate::ids::history_input_body_id(&prefix, slot)
    })?;
    let faces = project_input_members(&topology.faces, |slot| {
        crate::ids::history_input_face_id(&prefix, slot)
    })?;
    let edges = project_input_members(&topology.edges, |slot| {
        crate::ids::history_input_edge_id(&prefix, slot)
    })?;
    let vertices = project_input_members(&topology.vertices, |slot| {
        crate::ids::history_input_vertex_id(&prefix, slot)
    })?;
    let edge_geometry = project_input_edge_geometry(topology, &prefix);
    let id = crate::ids::history_input_state_id(&prefix);
    let input_of = feature.id.clone();
    let native_ref = state.id.clone();

    Some(cadmpeg_ir::features::FeatureInputTopology {
        id,
        input_of,
        bodies,
        faces,
        edges,
        vertices,
        edge_geometry,
        native_ref: Some(native_ref),
    })
}

/// Keep the edge geometry of the historical edges that some feature
/// definition selects, and of the edges that bound a face some feature
/// definition selects; consumers locate those edges, and through them the
/// selected faces, in the bodies they rebuild.
pub(crate) fn retain_selected_edge_geometry(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    features: &[cadmpeg_ir::features::Feature],
    input_topologies: &mut [cadmpeg_ir::features::FeatureInputTopology],
) -> Result<(), cadmpeg_core::CodecError> {
    use cadmpeg_ir::schema::rewrite::typed::RewriteIdentities;
    let mut sampled = HashSet::new();
    for geometry in input_topologies
        .iter()
        .flat_map(|topology| &topology.edge_geometry)
    {
        sampled.insert(geometry.edge.as_str());
        sampled.extend(
            geometry
                .faces
                .iter()
                .map(cadmpeg_ir::ids::HistoricalFaceId::as_str),
        );
    }
    if sampled.is_empty() {
        return Ok(());
    }
    let mut selected = HashSet::<String>::new();
    for feature in features {
        feature
            .evaluation
            .definition()
            .visit_local_members(ctx, &mut |target| {
                if sampled.contains(target) && !selected.contains(target) {
                    selected.insert(target.to_owned());
                }
                Ok(())
            })?;
    }
    drop(sampled);
    for topology in input_topologies {
        topology.edge_geometry.retain(|geometry| {
            selected.contains(geometry.edge.as_str())
                || geometry
                    .faces
                    .iter()
                    .any(|face| selected.contains(face.as_str()))
        });
    }
    Ok(())
}

/// Project the sampled edges of one historical state with their vertex
/// positions and the state faces whose loops use them.
fn project_input_edge_geometry(
    topology: &AsmHistoricalTopology,
    prefix: &cadmpeg_ir::ids::IdentityKey,
) -> Vec<cadmpeg_ir::features::HistoricalEdgeGeometry> {
    if topology.edge_samples.is_empty() {
        return Vec::new();
    }
    let positions = topology
        .point_positions
        .iter()
        .map(|point| (point.point, point.position))
        .collect::<HashMap<_, _>>();
    let vertex_positions = topology
        .vertex_points
        .iter()
        .filter_map(|binding| Some((binding.entity, *positions.get(&binding.carrier)?)))
        .collect::<HashMap<_, _>>();
    let endpoints = topology
        .edge_vertices
        .iter()
        .map(|edge| (edge.edge, (edge.start_vertex, edge.end_vertex)))
        .collect::<HashMap<_, _>>();
    let state_faces = topology.faces.iter().copied().collect::<HashSet<_>>();
    let mut loop_faces = HashMap::new();
    for relation in &topology.face_loops {
        if state_faces.contains(&relation.owner_ref) {
            for loop_ in &relation.member_refs {
                loop_faces.insert(*loop_, relation.owner_ref);
            }
        }
    }
    let mut edge_faces = HashMap::<i64, Vec<i64>>::new();
    for coedge in &topology.coedge_topology {
        if let Some(face) = loop_faces.get(&coedge.owner_loop) {
            let faces = edge_faces.entry(coedge.edge).or_default();
            if !faces.contains(face) {
                faces.push(*face);
            }
        }
    }
    let samples = topology
        .edge_samples
        .iter()
        .map(|sample| (sample.edge, sample))
        .collect::<HashMap<_, _>>();
    let position = |vertex: i64| {
        vertex_positions
            .get(&vertex)
            .copied()
            .and_then(cadmpeg_ir::features::FinitePoint3::new)
    };
    topology
        .edges
        .iter()
        .filter_map(|slot| {
            let sample = samples.get(slot)?;
            let (start, end) = endpoints.get(slot).map_or((None, None), |(start, end)| {
                (position(*start), position(*end))
            });
            let mut faces = edge_faces.get(slot).cloned().unwrap_or_default();
            faces.sort_unstable();
            Some(cadmpeg_ir::features::HistoricalEdgeGeometry {
                edge: crate::ids::history_input_edge_id(prefix, *slot),
                interior: cadmpeg_ir::features::FinitePoint3::new(sample.interior)?,
                tangent: sample.tangent.and_then(cadmpeg_ir::units::UnitVector3::new),
                start,
                end,
                faces: project_input_members(&faces, |face| {
                    crate::ids::history_input_face_id(prefix, face)
                })?,
            })
        })
        .collect()
}

fn project_input_members<T: Eq + std::hash::Hash>(
    slots: &[i64],
    id: impl FnMut(i64) -> T,
) -> Option<cadmpeg_ir::features::DistinctMembers<T>> {
    let members = slots.iter().copied().map(id).collect::<Vec<_>>();
    cadmpeg_ir::features::DistinctMembers::try_from(members).ok()
}

/// Resolve persistent vertex recipes in the last history-bearing feature state
/// that precedes their owning construction in authored timeline order.
pub(crate) fn bind_vertex_recipe_history(
    scopes: &mut [crate::records::feature::scope::DesignParameterScope],
    timelines: &[crate::records::entity_header::DesignFeatureTimeline],
    histories: &[AsmHistory],
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let source_ordinals =
        crate::design::feature_project::authored_scope_ordinals_per_stream(scopes, timelines)?;
    let mut input_states = HashMap::new();
    for scope in scopes.iter().filter(|scope| {
        matches!(
            scope.kind(),
            crate::records::feature::scope::DesignFeatureKind::WorkPlane
                | crate::records::feature::scope::DesignFeatureKind::WorkPoint
        )
    }) {
        let stream = crate::ids::native_stream(&scope.id).unwrap_or(crate::ids::DEFAULT_STREAM);
        let Some(&ordinal) = source_ordinals.get(&(stream, scope.record_index)) else {
            continue;
        };
        let mut predecessors = scopes.iter().filter_map(|candidate| {
            let candidate_stream =
                crate::ids::native_stream(&candidate.id).unwrap_or(crate::ids::DEFAULT_STREAM);
            let candidate_ordinal =
                *source_ordinals.get(&(candidate_stream, candidate.record_index))?;
            (candidate_stream == stream && candidate_ordinal < ordinal)
                .then_some((candidate_ordinal, candidate.history_state_id()?))
        });
        let Some(predecessor) = predecessors.next() else {
            continue;
        };
        let predecessor = predecessors.fold(predecessor, |latest, candidate| {
            if candidate.0 > latest.0 {
                candidate
            } else {
                latest
            }
        });
        let id = scope.id.clone();
        input_states.insert(id, predecessor.1);
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPoint
    }) {
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_point_construction_mut() else {
            continue;
        };
        let solved_position = cadmpeg_ir::math::Point3::new(
            construction.position[0].get() * 10.0,
            construction.position[1].get() * 10.0,
            construction.position[2].get() * 10.0,
        );
        for recipe in construction.rule.vertex_recipes_mut() {
            recipe.resolution = None;
            let Some(state_id) = state_id else {
                continue;
            };
            let Some((history, state)) = unique_history_state(histories, state_id) else {
                continue;
            };
            let Some(topology) = history.topology(state) else {
                continue;
            };
            let Some((vertex, position)) =
                vertex_recipe_candidate(recipe, topology, &mut topology_queries)
            else {
                continue;
            };
            if !point_matches(position, solved_position) {
                continue;
            }
            recipe.resolution = crate::records::feature::work_geometry::DesignVertexResolution::new(
                state_id, vertex,
            );
        }
    }

    for scope in scopes.iter_mut().filter(|scope| {
        scope.kind() == crate::records::feature::scope::DesignFeatureKind::WorkPlane
    }) {
        let transform = scope.work_plane_transform();
        let state_id = input_states.get(&scope.id).copied();
        let Some(construction) = scope.work_plane_construction_mut() else {
            continue;
        };
        construction.clear_resolution();
        let Some(state_id) = state_id else {
            continue;
        };
        let Some((history, state)) = unique_history_state(histories, state_id) else {
            continue;
        };
        let Some(topology) = history.topology(state) else {
            continue;
        };
        let [first, second, third] = construction.inputs();
        let (Some(first), Some(second), Some(third)) = (
            vertex_recipe_candidate(first, topology, &mut topology_queries),
            vertex_recipe_candidate(second, topology, &mut topology_queries),
            vertex_recipe_candidate(third, topology, &mut topology_queries),
        ) else {
            continue;
        };
        let candidates = [first, second, third];
        if !three_point_plane_matches(
            transform.map(crate::records::sketch_placement::SketchPlacementMatrix::rows),
            [first.1, second.1, third.1],
        ) {
            continue;
        }
        let [Some(first), Some(second), Some(third)] = candidates.map(|(vertex, _)| {
            crate::records::feature::work_geometry::DesignVertexResolution::new(state_id, vertex)
        }) else {
            continue;
        };
        construction
            .try_set_resolution([first, second, third])
            .map_err(cadmpeg_core::CodecError::malformed)?;
    }
    Ok(())
}

/// Resolve edge-treatment corner recipes in their bound feature-input state.
pub(crate) fn bind_edge_treatment_vertex_history(
    operands: &mut [crate::records::feature::work_geometry::DesignEdgeTreatmentVertexOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut topology_queries = TopologyQueryCache::default();
    let mut recipe_topologies = RecipeTopologyCache::default();
    for operand in operands {
        operand.recipe.resolution = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let (Some(state_id), Some(previous_state_id)) = (
            scope.history_state_id(),
            effective_scope_previous_history_state_id(scope, histories),
        ) else {
            continue;
        };
        let Some((history, _, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories,
        ) else {
            continue;
        };
        let Some(topology) = history.topology(previous) else {
            continue;
        };
        for reference in &mut operand.recipe.recipe_references {
            bind_historical_recipe_reference_candidates(
                reference,
                topology,
                &mut recipe_topologies,
            )?;
        }
        let Some(vertex) =
            recipe_reference_common_vertex(&operand.recipe, topology, &mut topology_queries)
        else {
            continue;
        };
        operand.recipe.resolution =
            crate::records::feature::work_geometry::DesignVertexResolution::new(
                previous_state_id,
                vertex,
            );
    }
    Ok(())
}

fn vertex_recipe_candidate<'a>(
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<(i64, cadmpeg_ir::math::Point3)> {
    let mut face_slots = Vec::new();
    for reference in &recipe.recipe_references {
        let mut slots = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology.faces.contains(face))
            .collect::<Vec<_>>();
        slots.sort_unstable_by(Ord::cmp);
        slots.dedup();
        let [slot] = slots.as_slice() else {
            return None;
        };

        face_slots.push(*slot);
    }
    if face_slots.is_empty() {
        return None;
    }
    let vertex = common_face_vertex(&face_slots, topology, topology_queries)?;
    unique_historical_vertex_position(vertex, topology).map(|position| (vertex, position))
}

fn three_point_plane_matches(
    transform: Option<[[f64; 4]; 4]>,
    points: [cadmpeg_ir::math::Point3; 3],
) -> bool {
    use cadmpeg_ir::math::{Point3, Vector3};

    let Some(transform) = transform else {
        return false;
    };
    let origin = Point3::new(
        transform[0][3] * 10.0,
        transform[1][3] * 10.0,
        transform[2][3] * 10.0,
    );
    let Some(normal) = Vector3::new(transform[0][2], transform[1][2], transform[2][2]).unit()
    else {
        return false;
    };
    let Some(point_normal) = points[1]
        .vector_from(points[0])
        .cross(points[2].vector_from(points[0]))
        .unit()
    else {
        return false;
    };
    let scale = points
        .iter()
        .flat_map(|point| [point.x.abs(), point.y.abs(), point.z.abs()])
        .fold(1.0_f64, f64::max);
    (normal.dot(point_normal).abs() - 1.0).abs() <= WORK_POINT_POSITION_TOLERANCE
        && points.iter().all(|point| {
            point.vector_from(origin).dot(normal).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

fn boundary_vertices_for_faces(
    faces: impl IntoIterator<Item = i64>,
    index: &VertexBoundaryIndex,
    edge_endpoints: &topology_cache::EdgeEndpoints,
) -> Option<HashSet<i64>> {
    let mut vertices = HashSet::new();
    for face in faces {
        let edges = index.boundaries.get(&face)?;
        for edge_slot in edges {
            let [start, end] = edge_endpoints.get(edge_slot).copied().flatten()?;
            if !index.vertices.contains(&start) || !index.vertices.contains(&end) {
                return None;
            }
            vertices.insert(start);
            vertices.insert(end);
        }
    }
    (!vertices.is_empty()).then_some(vertices)
}

fn common_face_vertex<'a>(
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let index = topology_queries
        .vertices
        .get(topology, VertexBoundaryIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, topology_cache::endpoint_index);
    let mut common = None::<HashSet<i64>>;
    for face in face_slots {
        let vertices = boundary_vertices_for_faces(std::iter::once(*face), index, edge_endpoints)?;
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
}

fn recipe_reference_common_vertex<'a>(
    recipe: &crate::records::feature::work_geometry::DesignVertexRecipe,
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let index = topology_queries
        .vertices
        .get(topology, VertexBoundaryIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, topology_cache::endpoint_index);
    let mut common = None::<HashSet<i64>>;
    for reference in &recipe.recipe_references {
        let faces = reference
            .candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| index.faces.contains(face));
        let vertices = boundary_vertices_for_faces(faces, index, edge_endpoints)?;
        if let Some(common) = &mut common {
            common.retain(|vertex| vertices.contains(vertex));
        } else {
            common = Some(vertices);
        }
    }
    let common = common?;
    if common.len() == 1 {
        common.into_iter().next()
    } else {
        None
    }
}

fn unique_historical_vertex_position(
    vertex: i64,
    topology: &AsmHistoricalTopology,
) -> Option<cadmpeg_ir::math::Point3> {
    let mut bindings = topology
        .vertex_points
        .iter()
        .filter(|binding| binding.entity == vertex);
    let point = bindings.next()?.carrier;
    if bindings.next().is_some() {
        return None;
    }
    let mut positions = topology
        .point_positions
        .iter()
        .filter(|position| position.point == point);
    let position = positions.next()?.position;
    (positions.next().is_none() && position.is_finite()).then_some(position)
}

fn point_matches(left: cadmpeg_ir::math::Point3, right: cadmpeg_ir::math::Point3) -> bool {
    [(left.x, right.x), (left.y, right.y), (left.z, right.z)]
        .into_iter()
        .all(|(left, right)| {
            let scale = left.abs().max(right.abs()).max(1.0);
            (left - right).abs() <= WORK_POINT_POSITION_TOLERANCE * scale
        })
}

fn unique_history_state(
    histories: &[AsmHistory],
    state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState)> {
    let mut matches = histories.iter().filter_map(|history| {
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == state_id);
        let state = states.next()?;
        states.next().is_none().then_some((history, state))
    });
    let state = matches.next()?;
    matches.next().is_none().then_some(state)
}

fn unique_history_state_in(history: &AsmHistory, state_id: i64) -> bool {
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    states.next().is_some() && states.next().is_none()
}

/// Read the scope's predecessor, else the bound transition predecessor, else
/// the raw `next` link. Reject disagreement between the transition and link.
pub(crate) fn effective_scope_previous_history_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id().or_else(|| {
        let state_id = scope.history_state_id()?;
        let (history, state) = unique_history_state(histories, state_id)?;
        transition_or_header_previous_state_id(history, state)
    })
}

/// Read the scope's predecessor, else its history header predecessor.
/// History reads use only `state_id`, `node_index`, and `next_ref`, so the
/// result does not depend on history binding.
pub(crate) fn header_scope_previous_history_state_id(
    scope: &crate::records::feature::scope::DesignParameterScope,
    histories: &[AsmHistory],
) -> Option<i64> {
    scope.previous_history_state_id().or_else(|| {
        let (history, state) = unique_history_state(histories, scope.history_state_id()?)?;
        header_previous_state_id(history, state)
    })
}

pub(crate) fn unique_history_state_pair(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    unique_state_pair_by(
        histories,
        state_id,
        previous_state_id,
        transition_or_header_previous_state_id,
    )
}

/// Resolve one state pair through history header links.
/// Read only `state_id`, `node_index`, and `next_ref`, so the result does not
/// depend on history binding.
pub(crate) fn unique_header_history_state_pair(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    unique_state_pair_by(
        histories,
        state_id,
        previous_state_id,
        header_previous_state_id,
    )
}

fn unique_state_pair_by(
    histories: &[AsmHistory],
    state_id: i64,
    previous_state_id: i64,
    predecessor: fn(&AsmHistory, &AsmDeltaState) -> Option<i64>,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut direct = histories.iter().filter_map(|history| {
        state_pair_by(history, state_id, previous_state_id, true, predecessor)
    });
    if let Some(pair) = direct.next() {
        return direct.next().is_none().then_some(pair);
    }
    let mut matches = histories.iter().filter_map(|history| {
        state_pair_by(history, state_id, previous_state_id, false, predecessor)
    });
    let pair = matches.next()?;
    matches.next().is_none().then_some(pair)
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum HemGapLengthForm {
    Flat,
    Open,
}

/// Distinguish Flat and Open Hem forms from the inserted coaxial bend carriers.
pub(crate) fn hem_gap_length_form_for_scope(
    scope: &crate::records::feature::scope::DesignParameterScope,
    edge_slot: i64,
    histories: &[AsmHistory],
) -> Option<HemGapLengthForm> {
    let (Some(state_id), Some(previous_state_id)) = (
        scope.history_state_id(),
        effective_scope_previous_history_state_id(scope, histories),
    ) else {
        return None;
    };
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    let (Some(previous_topology), Some(transition)) =
        (history.topology(previous), state.transition.as_ref())
    else {
        return None;
    };
    let current_topology = history.topology(state)?;
    let inserted_surfaces = &transition.topology.surfaces.inserted;
    if !current_topology
        .surface_cylinders
        .iter()
        .any(|cylinder| inserted_surfaces.contains(&cylinder.surface))
    {
        return None;
    }
    let edge_direction =
        historical_edge_axis(edge_slot, previous_topology).map(|(_, direction)| direction);
    let cylinders = || {
        current_topology
            .surface_cylinders
            .iter()
            .filter(|cylinder| {
                inserted_surfaces.contains(&cylinder.surface)
                    && edge_direction
                        .is_none_or(|direction| parallel_directions(direction, cylinder.axis))
            })
    };
    cylinders().next()?;
    hem_gap_length_form(cylinders())
}

fn hem_gap_length_form<'a>(
    mut cylinders: impl Iterator<Item = &'a AsmHistoricalCylinder>,
) -> Option<HemGapLengthForm> {
    let first = cylinders.next()?;
    let second = cylinders.next()?;
    if cylinders.next().is_some() {
        return None;
    }
    if !same_axis_line((first.origin, first.axis), (second.origin, second.axis)) {
        return None;
    }
    let [inner, outer] = if first.radius <= second.radius {
        [first.radius, second.radius]
    } else {
        [second.radius, first.radius]
    };
    if !inner.is_finite() || !outer.is_finite() || inner < 0.0 || outer <= inner {
        return None;
    }
    let thickness = outer - inner;
    let tolerance = EPS_HISTORY_HEM_GAP_LENGTH_FORM_E7 * (1.0 + outer.abs() + inner.abs());
    if (2.0 * inner - thickness).abs() <= tolerance {
        Some(HemGapLengthForm::Open)
    } else if 2.0 * inner < thickness - tolerance {
        Some(HemGapLengthForm::Flat)
    } else {
        None
    }
}

fn parallel_directions(left: cadmpeg_ir::math::Vector3, right: cadmpeg_ir::math::Vector3) -> bool {
    let left_length = left.norm();
    let right_length = right.norm();
    if !(left_length.is_finite()
        && left_length > 0.0
        && right_length.is_finite()
        && right_length > 0.0)
    {
        return false;
    }
    let dot = left
        .scale(1.0 / left_length)
        .dot(right.scale(1.0 / right_length));
    dot.is_finite() && (dot.abs() - 1.0).abs() <= EPS_HISTORY_PARALLEL_DIRECTIONS_E7
}

fn history_state_pair(
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    state_pair_by(
        history,
        state_id,
        previous_state_id,
        require_direct,
        transition_or_header_previous_state_id,
    )
}

fn state_pair_by(
    history: &AsmHistory,
    state_id: i64,
    previous_state_id: i64,
    require_direct: bool,
    predecessor: fn(&AsmHistory, &AsmDeltaState) -> Option<i64>,
) -> Option<(&AsmHistory, &AsmDeltaState, &AsmDeltaState)> {
    let mut states = history
        .states()
        .iter()
        .filter(|state| state.state_id == state_id);
    let state = states.next()?;
    if states.next().is_some()
        || (require_direct && predecessor(history, state) != Some(previous_state_id))
    {
        return None;
    }
    let mut previous_states = history
        .states()
        .iter()
        .filter(|state| state.state_id == previous_state_id);
    let previous = previous_states.next()?;
    if previous_states.next().is_some()
        || (!require_direct && !state_reaches_by(history, state, previous_state_id, predecessor))
    {
        return None;
    }
    Some((history, state, previous))
}

/// Return the one ASM history bound to a Design scope.
pub(crate) fn bound_scope_history<'a>(
    scope_id: &str,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<&'a AsmHistory> {
    let history_id = scope_histories.get(scope_id)?;
    let mut matches = histories.iter().filter(|history| &history.id == history_id);
    let history = matches.next()?;
    matches.next().is_none().then_some(history)
}

fn bound_history_state_pair<'a>(
    scope_id: &str,
    state_id: i64,
    previous_state_id: i64,
    scope_histories: &HashMap<String, String>,
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    history_state_pair(
        bound_scope_history(scope_id, scope_histories, histories)?,
        state_id,
        previous_state_id,
        false,
    )
}

fn insert_scope_history_binding(
    resolved: &mut HashMap<String, String>,
    scope_id: &str,
    history_id: &str,
) {
    let scope = scope_id.to_owned();
    let history = history_id.to_owned();
    resolved.insert(scope, history);
}

pub(crate) fn bind_scope_histories(
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    body_bindings: &[crate::records::bodies::DesignBodyBinding],
    body_recipe_operands: &[crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    histories: &[AsmHistory],
) -> HashMap<String, String> {
    let mut candidates = Vec::new();
    for scope in scopes {
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let scope_candidates = if let Some(previous_state_id) = scope.previous_history_state_id() {
            let direct = histories
                .iter()
                .filter(|history| {
                    history_state_pair(history, state_id, previous_state_id, true).is_some()
                })
                .collect::<Vec<_>>();
            if direct.is_empty() {
                histories
                    .iter()
                    .filter(|history| {
                        history_state_pair(history, state_id, previous_state_id, false).is_some()
                    })
                    .collect::<Vec<_>>()
            } else {
                direct
            }
        } else {
            histories
                .iter()
                .filter(|history| unique_history_state_in(history, state_id))
                .collect::<Vec<_>>()
        };
        if !scope_candidates.is_empty() {
            candidates.push((scope, scope_candidates));
        }
    }
    let mut resolved = HashMap::<String, String>::new();
    for (scope, candidates) in &candidates {
        if candidates.len() == 1 {
            insert_scope_history_binding(&mut resolved, &scope.id, &candidates[0].id);
            continue;
        }
        let next_scope_record_index = scopes
            .iter()
            .filter(|candidate| {
                crate::ids::same_native_occurrence(&candidate.id, &scope.id)
                    && candidate.record_index > scope.record_index
            })
            .map(|candidate| candidate.record_index)
            .min();
        let mut output_bindings = body_bindings.iter().filter(|binding| {
            crate::ids::same_native_occurrence(binding.id(), &scope.id)
                && binding.entity_suffix > u64::from(scope.record_index)
                && next_scope_record_index
                    .is_none_or(|next| binding.entity_suffix < u64::from(next))
        });
        if let Some(binding) = output_bindings.next() {
            if output_bindings.next().is_none() {
                let mut matching = candidates.iter().filter(|history| {
                    historical_brep_source(&history.id).is_some_and(|source| {
                        binding.blob_name().strip_prefix("BREP.") == Some(source)
                    })
                });
                if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                    insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                    continue;
                }
            }
        }
        let candidate_faces = body_recipe_operands
            .iter()
            .filter(|operand| {
                crate::ids::same_native_occurrence(&operand.id, &scope.id)
                    && operand.scope_record_index == scope.record_index
            })
            .flat_map(super::records::topology::body_recipe::DesignBodyRecipeOperand::references)
            .flat_map(|reference| &reference.candidate_faces);
        if candidate_faces.clone().next().is_some() {
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id).is_some_and(|source| {
                    candidate_faces
                        .clone()
                        .any(|face| active_brep_face_matches_source(face, source))
                })
            });
            if let Some(history) = matching.next().filter(|_| matching.next().is_none()) {
                insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                continue;
            }
        }
        let Some(construction) = scope.base_feature_construction() else {
            continue;
        };
        let mut referenced_histories = construction.body_reference_records().filter_map(|suffix| {
            let mut bindings = body_bindings.iter().filter(|binding| {
                crate::ids::same_native_occurrence(binding.id(), &scope.id)
                    && binding.entity_suffix == u64::from(suffix)
            });
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut matching = candidates.iter().filter(|history| {
                historical_brep_source(&history.id)
                    .is_some_and(|source| binding.blob_name().strip_prefix("BREP.") == Some(source))
            });
            let history = matching.next()?;
            matching.next().is_none().then_some(history.id.as_str())
        });
        let Some(history_id) = referenced_histories.next() else {
            continue;
        };
        if referenced_histories.all(|candidate| candidate == history_id) {
            insert_scope_history_binding(&mut resolved, &scope.id, history_id);
        }
    }
    let mut groups = HashMap::<(&str, i64, Option<i64>), Vec<usize>>::new();
    for (index, (scope, _)) in candidates.iter().enumerate() {
        let (Some(stream), Some(state_id)) = (
            crate::ids::native_stream(&scope.id),
            scope.history_state_id(),
        ) else {
            continue;
        };
        let key = (stream, state_id, scope.previous_history_state_id());
        groups.entry(key).or_default().push(index);
    }
    for members in groups.values() {
        let mut candidate_histories = HashSet::new();
        for index in members {
            for history in &candidates[*index].1 {
                candidate_histories.insert(history.id.as_str());
            }
        }
        if candidate_histories.len() != members.len() {
            continue;
        }
        loop {
            let mut assigned = HashSet::new();
            for index in members {
                if let Some(history_id) = resolved.get(&candidates[*index].0.id) {
                    let copy = history_id.to_owned();
                    assigned.insert(copy);
                }
            }
            if assigned.len()
                != members
                    .iter()
                    .filter(|index| resolved.contains_key(&candidates[**index].0.id))
                    .count()
            {
                break;
            }
            let mut progress = false;
            for index in members {
                let (scope, scope_candidates) = &candidates[*index];
                if resolved.contains_key(&scope.id) {
                    continue;
                }
                let mut remaining = scope_candidates
                    .iter()
                    .filter(|history| !assigned.contains(history.id.as_str()));
                if let Some(history) = remaining.next().filter(|_| remaining.next().is_none()) {
                    insert_scope_history_binding(&mut resolved, &scope.id, &history.id);
                    progress = true;
                }
            }
            if !progress {
                break;
            }
        }
    }
    resolved
}

fn history_state_reaches(
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
) -> bool {
    state_reaches_by(
        history,
        state,
        previous_state_id,
        transition_or_header_previous_state_id,
    )
}

fn state_reaches_by(
    history: &AsmHistory,
    state: &AsmDeltaState,
    previous_state_id: i64,
    predecessor: fn(&AsmHistory, &AsmDeltaState) -> Option<i64>,
) -> bool {
    let mut current = state;
    for _ in 0..=history.states().len() {
        if current.state_id == previous_state_id {
            return true;
        }
        let Some(previous_state_id) = predecessor(history, current) else {
            return false;
        };
        let mut states = history
            .states()
            .iter()
            .filter(|state| state.state_id == previous_state_id);
        let Some(previous) = states.next() else {
            return false;
        };
        if states.next().is_some() {
            return false;
        }
        current = previous;
    }
    false
}

/// Read the unique predecessor through the history header's `next` link.
/// Read only `state_id`, `node_index`, and `next_ref`, so the result does not
/// depend on history binding.
fn header_previous_state_id(history: &AsmHistory, state: &AsmDeltaState) -> Option<i64> {
    state.next_ref.and_then(|node_index| {
        let mut states = history
            .states()
            .iter()
            .filter(|candidate| candidate.node_index == node_index);
        let previous = states.next()?;
        states.next().is_none().then_some(previous.state_id)
    })
}

/// Read the bound transition predecessor, else the raw `next` link.
/// Reject disagreement between the transition predecessor and the header link.
fn transition_or_header_previous_state_id(
    history: &AsmHistory,
    state: &AsmDeltaState,
) -> Option<i64> {
    let linked = header_previous_state_id(history, state);
    let derived = state
        .transition
        .as_ref()
        .and_then(|transition| transition.previous_state_id);
    match (derived, linked) {
        (Some(derived), Some(linked)) if derived != linked => None,
        (Some(derived), _) => Some(derived),
        (None, Some(linked)) => Some(linked),
        (None, None) => None,
    }
}

fn history_state_index(history: &AsmHistory) -> HashMap<i64, Option<&AsmDeltaState>> {
    let mut states = HashMap::new();
    for state in history.states() {
        states
            .entry(state.state_id)
            .and_modify(|state| *state = None)
            .or_insert(Some(state));
    }
    states
}

fn exact_face_selection_group<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    operand_groups: &'a [crate::records::topology::construction::DesignConstructionOperandGroup],
) -> Option<&'a crate::records::topology::construction::DesignConstructionOperandGroup> {
    let stream = crate::ids::native_stream(&operand.id)?;
    if crate::ids::native_stream(&scope.id) != Some(stream) {
        return None;
    }
    let group_record_index = operand.group_record_index()?;
    let group_member_ordinal = usize::try_from(operand.group_member_ordinal()?).ok()?;
    let mut groups = operand_groups.iter().filter(|group| {
        crate::ids::native_stream(&group.id) == Some(stream)
            && group.scope_record_index == scope.record_index
            && group.record_index == group_record_index
            && group.role() == DesignOperandRole::ROLE_0X10
            && group
                .members()
                .get(group_member_ordinal)
                .map(|member| &member.value)
                == Some(&operand.record_index())
    });
    let group = groups.next()?;
    groups.next().is_none().then_some(group)
}

/// Bind one recipe reference to every live face or edge fragment carrying its
/// token and Design reference in the recipe-state topology.
fn bind_historical_recipe_reference_candidates<'a>(
    reference: &mut crate::records::dimensions::DesignRecipeReference,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    reference.candidate_faces.clear();
    reference.candidate_edges.clear();
    reference.alternate_selector_faces.clear();
    reference.alternate_selector_edges.clear();
    let index = recipe_topologies.get(topology);
    for tag_index in index.matching_tags(&reference.token, reference.design_reference) {
        let tag = &index.topology.persistent_subentity_tags[*tag_index];
        match tag.entity_kind {
            AsmHistoricalEntityKind::Face => {
                reference
                    .candidate_faces
                    .push(historical_face_id(tag.entity_ref)?);
            }
            AsmHistoricalEntityKind::Edge => {
                reference
                    .candidate_edges
                    .push(historical_edge_id(tag.entity_ref)?);
            }
            _ => {}
        }
    }
    reference
        .candidate_faces
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    reference.candidate_faces.dedup();
    reference
        .candidate_edges
        .sort_by(|left, right| left.as_str().cmp(right.as_str()));
    reference.candidate_edges.dedup();
    Ok(())
}

fn historical_recipe_faces<'a>(
    design_reference: i64,
    topology: &'a AsmHistoricalTopology,
    recipe_topologies: &mut RecipeTopologyCache<'a>,
) -> Result<Vec<cadmpeg_ir::ids::FaceId>, cadmpeg_core::CodecError> {
    let index = recipe_topologies.get(topology);
    let topology = index.topology;
    let mut faces = Vec::new();
    for tag_index in index.face_tags(design_reference) {
        let tag = &topology.persistent_subentity_tags[*tag_index];

        faces.push(historical_face_id(tag.entity_ref)?);
    }
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    Ok(faces)
}

fn direct_face_recipe_candidates(
    recipe_kind: crate::records::recipes::ConstructionRecipeKind,
    references: &[crate::records::dimensions::DesignRecipeReference],
    recipe_record_index: i32,
) -> Option<Vec<cadmpeg_ir::ids::FaceId>> {
    if recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face {
        return None;
    }
    let mut faces = Vec::new();
    for face in references
        .iter()
        .filter(|reference| reference.design_reference == i64::from(recipe_record_index))
        .flat_map(|reference| &reference.candidate_faces)
    {
        faces.push(face.clone());
    }
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    (!faces.is_empty()).then_some(faces)
}

fn historical_face_id(slot: i64) -> Result<cadmpeg_ir::ids::FaceId, cadmpeg_core::CodecError> {
    let text = format!("f3d:brep:entity#{slot}");
    cadmpeg_ir::ids::FaceId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

fn historical_edge_id(slot: i64) -> Result<cadmpeg_ir::ids::EdgeId, cadmpeg_core::CodecError> {
    let text = format!("f3d:brep:entity#{slot}");
    cadmpeg_ir::ids::EdgeId::mint(text).map_err(cadmpeg_core::CodecError::malformed)
}

pub(crate) fn bind_face_operand_history_candidates(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    mut histories: BindingHistories<'_>,
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(histories.get()) {
        return Ok(());
    }
    let recipe_record_indices = recipes
        .iter()
        .filter_map(|recipe| Some((recipe.id.as_str(), recipe.record_index?.value)))
        .collect::<std::collections::HashMap<_, _>>();
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        operand.preceding_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.historical_support_contexts.clear();
        operand.resolved_face_slots.clear();
        operand.resolved_active_face = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let histories = histories.get();
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(state_id) = scope.history_state_id() else {
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(scope, scoped_histories)
        else {
            continue;
        };
        let Some((history, state, previous)) = (if scoped_history.is_some() {
            bound_history_state_pair(
                &scope.id,
                state_id,
                previous_state_id,
                scope_histories,
                histories,
            )
        } else {
            unique_history_state_pair(histories, state_id, previous_state_id)
        }) else {
            continue;
        };
        let (Some((history_index, state_index)), Some((_, previous_index))) = (
            history_state_indexes(histories, history, state),
            history_state_indexes(histories, history, previous),
        ) else {
            continue;
        };
        jobs.push((
            history_index,
            previous_index,
            (position, scope, state_index),
        ));
    }
    let mut first_error = None;
    visit_state_groups_oldest_first(
        histories.reborrow(),
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous) else {
                return;
            };
            let states = history_state_index(history);
            let mut caches = FaceOperandCaches::default();
            for (position, scope, state_index) in group {
                let (Some(operand), Some(state)) = (
                    operands.get_mut(position),
                    history.states().get(state_index),
                ) else {
                    continue;
                };
                let transition = FaceOperandTransition {
                    scope,
                    history,
                    state,
                    previous,
                    topology,
                    states: &states,
                };
                if let Err(error) = bind_historical_face_operand(
                    operand,
                    &transition,
                    operand_groups,
                    &recipe_record_indices,
                    &mut caches,
                ) {
                    keep_first_error(&mut first_error, position, error);
                }
            }
        },
    );
    if let Some((_, error)) = first_error {
        return Err(error);
    }
    bind_profile_face_group_cardinality(
        decode,
        operands,
        scopes,
        operand_groups,
        histories.get(),
        scope_histories,
    )?;
    Ok(())
}

/// One face operand's scope and state pair with the previous-state topology.
struct FaceOperandTransition<'a> {
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    history: &'a AsmHistory,
    state: &'a AsmDeltaState,
    previous: &'a AsmDeltaState,
    topology: &'a AsmHistoricalTopology,
    states: &'a HashMap<i64, Option<&'a AsmDeltaState>>,
}

/// Indexes over the topologies of one previous state and its results.
#[derive(Default)]
struct FaceOperandCaches<'a> {
    topology_queries: TopologyQueryCache<'a>,
    recipe_topologies: RecipeTopologyCache<'a>,
    face_recipes: SnapshotCache<'a, AsmHistoricalTopology, face_recipe::FaceRecipeIndex>,
}

fn bind_historical_face_operand<'a>(
    operand: &mut crate::records::topology::face::DesignFaceOperand,
    transition: &FaceOperandTransition<'a>,
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    recipe_record_indices: &HashMap<&str, i32>,
    caches: &mut FaceOperandCaches<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    let &FaceOperandTransition {
        scope,
        history,
        state,
        previous,
        topology,
        states,
    } = transition;
    let previous_state_id = previous.state_id;
    let FaceOperandCaches {
        topology_queries,
        recipe_topologies,
        face_recipes,
    } = caches;
    let stream = crate::ids::native_stream(&operand.id);
    for reference in &mut operand.recipe_references {
        bind_historical_recipe_reference_candidates(reference, topology, recipe_topologies)?;
    }
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::DeleteFace {
        operand.resolved_face_slots = face_recipes
            .get(topology, face_recipe::FaceRecipeIndex::new)
            .resolve(operand)
            .into_iter()
            .collect();
        return Ok(());
    }
    if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
        operand.candidate_faces =
            historical_recipe_faces(i64::from(*recipe_record_index), topology, recipe_topologies)?;
        operand.unreferenced_candidate_faces = (operand.candidate_faces.iter().filter(|face| {
            !operand
                .recipe_references
                .iter()
                .flat_map(|reference| &reference.candidate_faces)
                .any(|candidate| candidate == *face)
        }))
        .cloned()
        .collect::<Vec<_>>();
        operand.alternate_selector_candidate_faces.clear();
    }
    let Some(changed_faces) = face_changes_across_state_chain(state, previous_state_id, states)
    else {
        return Ok(());
    };
    let direct_face_candidates =
        if let Some(record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
            direct_face_recipe_candidates(
                operand.recipe_kind,
                &operand.recipe_references,
                *record_index,
            )
        } else {
            None
        };
    let feature_family = crate::design::design_feature_family(&scope.kind());
    let thread_face_candidates = if feature_family
        == Some(crate::design::DesignFeatureFamily::Thread)
        && exact_face_selection_group(operand, scope, operand_groups).is_some()
    {
        operand
            .recipe_references
            .first()
            .and_then(|reference| {
                let candidates = effective_faces(reference);
                (!candidates.is_empty()).then_some(candidates)
            })
            .map(<[cadmpeg_ir::ids::FaceId]>::to_vec)
    } else {
        None
    };
    let nested_split_face_candidates = if scope.kind()
        == crate::records::feature::scope::DesignFeatureKind::SplitFace
        && exact_face_selection_group(operand, scope, operand_groups).is_some()
    {
        crate::design::face_resolve::nested_bounded_face_history_candidates(operand)
    } else {
        None
    };
    let grouped_reference_face_candidates = (!matches!(
        feature_family,
        Some(
            crate::design::DesignFeatureFamily::Thread | crate::design::DesignFeatureFamily::Split
        )
    ))
    .then(|| grouped_reference_face_candidate(operand, topology, &changed_faces))
    .flatten()
    .map(|face| vec![face]);
    let legacy_face_candidates = (|| {
        if feature_family != Some(crate::design::DesignFeatureFamily::Extrude) {
            return None;
        }
        let group_record_index = operand.group_record_index()?;
        let mut groups = operand_groups.iter().filter(|group| {
                crate::ids::native_stream(&group.id) == stream
                    && group.scope_record_index == scope.record_index
                    && group.record_index == group_record_index
                    && group.extrude_role().is_some_and(|role| {
                        matches!(
                            role,
                            crate::records::topology::extrude_selection::DesignExtrudeOperandRole::Faces(_)
                        )
                    })
                    && group.extrude_face_role().is_some()
            });
        groups.next()?;
        if groups.next().is_some() {
            return None;
        }
        let recipe_record_index = recipe_record_indices.get(operand.recipe_id.as_str())?;
        crate::design::face_resolve::legacy_face_recipe_reference_candidates(
            operand,
            *recipe_record_index,
        )
    })();
    let fallback_candidates;
    let history_candidates = if let Some(candidates) = direct_face_candidates
        .as_deref()
        .or(thread_face_candidates.as_deref())
        .or(nested_split_face_candidates.as_deref())
        .or(grouped_reference_face_candidates.as_deref())
    {
        candidates
    } else {
        fallback_candidates =
            crate::design::face_resolve::historical_face_operand_candidates(operand);
        &fallback_candidates
    };
    operand.preceding_candidate_faces = selection::faces_in_topology(history_candidates, topology);
    operand.changed_candidate_faces = (operand.preceding_candidate_faces.iter().filter(|face| {
        stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
    }))
    .cloned()
    .collect::<Vec<_>>();
    let support_candidates = if feature_family == Some(crate::design::DesignFeatureFamily::Extrude)
        && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
    {
        face_operand_support_candidates(
            history_candidates,
            operand.recipe_references.first(),
            recipe_record_indices
                .get(operand.recipe_id.as_str())
                .copied(),
        )
    } else {
        std::borrow::Cow::Borrowed(history_candidates)
    };
    operand.historical_support_contexts = historical_face_support_contexts(
        &support_candidates,
        history,
        topology,
        &changed_faces,
        topology_queries,
    );
    if direct_face_candidates.is_some() {
        operand.resolved_face_slots = operand
            .preceding_candidate_faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .collect::<Vec<_>>();
        return Ok(());
    }
    let preserves_stable_face_set = feature_family
            == Some(crate::design::DesignFeatureFamily::Shell)
            || operand
                .group_record_index()
                .is_some_and(|group_record_index| {
                    let mut groups = operand_groups.iter().filter(|group| {
                        crate::ids::native_stream(&group.id) == stream
                            && group.scope_record_index == scope.record_index
                            && group.record_index == group_record_index
                    });
                    let Some(group) = groups.next() else {
                        return false;
                    };
                    groups.next().is_none()
                        && group.extrude_face_role()
                            == Some(crate::records::topology::extrude_selection::DesignExtrudeFaceRole::Termination)
                });
    operand.resolved_face_slots = match &scope.payload() {
        crate::records::feature::scope::DesignScopePayload::OffsetFaces(Some(_))
        | crate::records::feature::scope::DesignScopePayload::DecalerLesFaces(Some(_)) => {
            let direct = resolve_direct_face_recipe_clauses(
                &operand.recipe_references,
                topology,
                &changed_faces,
            );
            if direct.is_empty() {
                crate::design::face_resolve::resolve_face_operand_history_candidates(operand)
                    .into_iter()
                    .collect()
            } else {
                direct
            }
        }
        _ => {
            let direct =
                crate::design::face_resolve::resolve_face_operand_history_candidates(operand);
            if let Some(direct) = direct {
                vec![direct]
            } else if scope.kind()
                == crate::records::feature::scope::DesignFeatureKind::SurfaceDeleteFace
            {
                crate::design::face_resolve::resolve_surface_delete_face_history_set(operand)
                    .unwrap_or_default()
            } else if preserves_stable_face_set {
                if let Some(stable) =
                    crate::design::face_resolve::resolve_stable_bounded_face_history_set(operand)
                {
                    stable
                } else {
                    crate::design::face_resolve::resolve_bounded_face_history_candidates(operand)
                        .unwrap_or_default()
                }
            } else if let Some(bounded) =
                crate::design::face_resolve::resolve_bounded_face_history_candidates(operand)
            {
                bounded
            } else {
                let pattern = if feature_family
                    == Some(crate::design::DesignFeatureFamily::CircularPattern)
                {
                    if let Some(result) = history.topology(state) {
                        resolve_pattern_face_by_surface_radius(
                            crate::design::face_resolve::face_operand_candidates(operand),
                            topology,
                            result,
                            &changed_faces,
                        )
                    } else {
                        None
                    }
                } else {
                    None
                };
                pattern.into_iter().collect()
            }
        }
    };
    if let Some(candidates) = &thread_face_candidates {
        // Thread's first prefix reference is its exclusive selection lane.
        // An unresolved lane must not fall back to construction context.
        operand.resolved_face_slots =
            crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                operand, candidates,
            )
            .or_else(|| {
                resolve_thread_face_by_transition(
                    scope,
                    candidates,
                    history,
                    topology,
                    &changed_faces,
                )
            })
            .into_iter()
            .collect();
    }
    if let Some(candidates) = &grouped_reference_face_candidates {
        // A grouped frame's unique changed topology-face reference is its
        // exact historical selection lane.
        operand.resolved_face_slots =
            crate::design::face_resolve::resolve_face_operand_history_candidate_from(
                operand, candidates,
            )
            .into_iter()
            .collect();
    }
    if feature_family == Some(crate::design::DesignFeatureFamily::Split) {
        operand.resolved_face_slots = resolve_split_tool_face(operand, topology)
            .into_iter()
            .collect();
    }
    if feature_family == Some(crate::design::DesignFeatureFamily::Loft)
        && operand.recipe_kind == crate::records::recipes::ConstructionRecipeKind::BoundedFace
        && state
            .transition
            .as_ref()
            .is_some_and(|transition| transition.previous_state_id == Some(previous_state_id))
    {
        if let Some((result, transition)) = history.topology(state).zip(state.transition.as_ref()) {
            if let Some(face) = resolve_bounded_face_recipe_target(
                operand,
                topology,
                result,
                &transition.topology.bodies.inserted,
                topology_queries,
            )? {
                operand.resolved_face_slots = vec![face];
            }
        }
    }
    if operand.resolved_face_slots.is_empty()
        && scope.kind() == crate::records::feature::scope::DesignFeatureKind::Draft
    {
        if let Some(result) = history.topology(state) {
            if let Some(face) = resolve_draft_face_by_surface_transition(operand, topology, result)
            {
                operand.resolved_face_slots = vec![face];
            }
        }
    }
    if operand.resolved_face_slots.is_empty() {
        if let Some(candidates) = legacy_face_candidates {
            match select_legacy_extrude_face_candidate(
                candidates,
                topology,
                &changed_faces,
                historical_brep_source(&history.id),
            ) {
                Some(LegacyFaceResolution::Historical(slot)) => {
                    operand.resolved_face_slots = vec![slot];
                }
                Some(LegacyFaceResolution::Active(face)) => {
                    operand.resolved_active_face = Some(face);
                }
                None => {}
            }
        }
    }
    Ok(())
}

/// Resolve a Draft face whose persistent selector lane is ambiguous in the
/// active B-rep by following the surface carrier through the exact Draft
/// transition.
///
/// Draft changes the selected face's actual surface geometry. The same
/// transition may update tangent or context faces, so the transition is
/// admissible only when exactly one face from the operand's alternate lane (or,
/// when no alternate lane exists, its exact candidate lane) changes surface
/// geometry. A missing, duplicate, or unchanged surface is not a selection
/// proof. Carrier record identities are not compared: a history update may
/// assign a new carrier record without changing the surface.
#[derive(Clone, Copy, PartialEq)]
enum DraftSurfaceGeometry {
    Plane {
        origin: cadmpeg_ir::math::Point3,
        normal: cadmpeg_ir::math::Vector3,
    },
    Cylinder {
        origin: cadmpeg_ir::math::Point3,
        axis: cadmpeg_ir::math::Vector3,
        radius: f64,
    },
    Axis {
        origin: cadmpeg_ir::math::Point3,
        direction: cadmpeg_ir::math::Vector3,
        radius: Option<f64>,
    },
}

fn draft_surface_geometry(
    topology: &crate::history_records::AsmHistoricalTopology,
    face: i64,
) -> Option<DraftSurfaceGeometry> {
    let mut bindings = topology
        .face_surfaces
        .iter()
        .filter(|binding| binding.entity == face)
        .map(|binding| binding.carrier);
    let carrier = bindings.next()?;
    if bindings.next().is_some() {
        return None;
    }
    if let Some(plane) = topology
        .surface_planes
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Plane {
            origin: plane.origin,
            normal: plane.normal,
        });
    }
    if let Some(cylinder) = topology
        .surface_cylinders
        .iter()
        .find(|surface| surface.surface == carrier)
    {
        return Some(DraftSurfaceGeometry::Cylinder {
            origin: cylinder.origin,
            axis: cylinder.axis,
            radius: cylinder.radius,
        });
    }
    topology
        .surface_axes
        .iter()
        .find(|surface| surface.surface == carrier)
        .map(|axis| DraftSurfaceGeometry::Axis {
            origin: axis.origin,
            direction: axis.direction,
            radius: topology
                .surface_radii
                .iter()
                .find(|radius| radius.surface == carrier)
                .map(|radius| radius.radius),
        })
}

fn resolve_draft_face_by_surface_transition(
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !matches!(
            crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program),
            Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { .. })
        )
        || operand.recipe_nodes.is_empty()
    {
        return None;
    }
    let mut candidate_slots = HashSet::new();
    for face in operand
        .candidate_faces
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        candidate_slots.insert(face);
    }
    if candidate_slots.is_empty() {
        return None;
    }
    let mut alternate_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.alternate_selector_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        alternate_slots.insert(face);
    }
    let mut exact_slots = BTreeSet::new();
    for face in operand
        .recipe_references
        .iter()
        .flat_map(|reference| &reference.candidate_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| candidate_slots.contains(face))
    {
        exact_slots.insert(face);
    }
    let has_alternates = !alternate_slots.is_empty();
    let candidates = if has_alternates {
        alternate_slots
    } else {
        exact_slots
    };
    if candidates.is_empty() {
        return None;
    }
    if !has_alternates {
        let mut faces = candidates.iter();
        let face = faces.next().copied()?;
        if faces.next().is_some() {
            return None;
        }
        return (preceding.faces.contains(&face) && result.faces.contains(&face)).then_some(face);
    }
    let mut changed = None;
    for face in candidates
        .into_iter()
        .filter(|face| preceding.faces.contains(face) && result.faces.contains(face))
        .filter(|face| {
            draft_surface_geometry(preceding, *face)
                .zip(draft_surface_geometry(result, *face))
                .is_some_and(|(before, after)| before != after)
        })
    {
        if changed.replace(face).is_some() {
            return None;
        }
    }
    changed
}

fn resolve_pattern_face_by_surface_radius(
    candidates: &[cadmpeg_ir::ids::FaceId],
    preceding: &crate::history_records::AsmHistoricalTopology,
    result: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let mut candidate_faces = HashSet::new();
    for face in candidates
        .iter()
        .filter_map(|face| stable_ref(face.as_str()))
    {
        candidate_faces.insert(face);
    }
    if candidate_faces.is_empty() {
        return None;
    }
    let mut result_radius = None;
    let mut bound_candidates = HashSet::new();
    for binding in result
        .face_surfaces
        .iter()
        .filter(|binding| candidate_faces.contains(&binding.entity))
    {
        if !bound_candidates.insert(binding.entity) {
            return None;
        }
        let mut radii = result
            .surface_radii
            .iter()
            .filter(|radius| radius.surface == binding.carrier);
        let radius = radii.next().map(|radius| radius.radius)?;
        if radii.next().is_some() || !radius.is_finite() || radius <= 0.0 {
            return None;
        }
        match result_radius {
            None => result_radius = Some(radius.to_bits()),
            Some(expected) if expected == radius.to_bits() => {}
            Some(_) => return None,
        }
    }
    if bound_candidates != candidate_faces {
        return None;
    }
    let result_radius = result_radius?;
    let mut matches = preceding
        .face_surfaces
        .iter()
        .filter(|binding| changed_faces.contains(&binding.entity))
        .filter_map(|binding| {
            let mut radii = preceding
                .surface_radii
                .iter()
                .filter(|radius| radius.surface == binding.carrier);
            let radius = radii.next()?;
            (radii.next().is_none() && radius.radius.to_bits() == result_radius)
                .then_some(binding.entity)
        });
    let face = matches.next()?;
    matches.next().is_none().then_some(face)
}

fn resolve_split_tool_face(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &crate::history_records::AsmHistoricalTopology,
) -> Option<i64> {
    if operand.group_record_index().is_some()
        || operand.group_member_ordinal().is_some()
        || operand.scope_reference_ordinal != 1
        || operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::Face
        || operand.recipe_program != [0, -1]
    {
        return None;
    }
    let [reference] = operand.recipe_references.as_slice() else {
        return None;
    };
    let candidates = selection::faces_in_topology(&reference.candidate_faces, topology);
    let [face] = candidates.as_slice() else {
        return None;
    };
    stable_ref(face.as_str())
}

fn effective_faces(
    reference: &crate::records::dimensions::DesignRecipeReference,
) -> &[cadmpeg_ir::ids::FaceId] {
    if reference.candidate_faces.is_empty() {
        &reference.alternate_selector_faces
    } else {
        &reference.candidate_faces
    }
}

fn resolve_thread_face_by_transition(
    scope: &crate::records::feature::scope::DesignParameterScope,
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &AsmHistory,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<i64> {
    let construction = scope.thread_construction()?;
    let source = historical_brep_source(&history.id)?;
    let mut source_candidates = candidates
        .iter()
        .filter(|face| active_brep_face_matches_source(face, source));
    source_candidates.next()?;
    if source_candidates.next().is_some() {
        return None;
    }
    let minimum_radius = construction.diameters.minor() * 5.0;
    let maximum_radius = construction.diameters.major() * 5.0;
    if !minimum_radius.is_finite() || !maximum_radius.is_finite() {
        return None;
    }
    let tolerance = EPS_HISTORY_RESOLVE_THREAD_FACE_BY_TRANSITION_E9 * (1.0 + maximum_radius.abs());
    let mut matching_faces = topology
        .faces
        .iter()
        .copied()
        .filter(|face| changed_faces.contains(face))
        .filter_map(|face| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == face);
            let binding = bindings.next()?;
            if bindings.next().is_some() {
                return None;
            }
            let mut cylinders = topology
                .surface_cylinders
                .iter()
                .filter(|cylinder| cylinder.surface == binding.carrier);
            let cylinder = cylinders.next()?;
            (cylinders.next().is_none()
                && cylinder.radius + tolerance >= minimum_radius
                && cylinder.radius <= maximum_radius + tolerance)
                .then_some(face)
        });
    let face = matching_faces.next()?;
    matching_faces
        .all(|candidate| candidate == face)
        .then_some(face)
}

fn grouped_reference_face_candidate(
    operand: &crate::records::topology::face::DesignFaceOperand,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Option<cadmpeg_ir::ids::FaceId> {
    if operand.recipe_kind != crate::records::recipes::ConstructionRecipeKind::BoundedFace
        || !crate::design::decode::dimension_frames::is_grouped_recipe_reference_frame(
            &operand.recipe_prefix_bytes,
        )
    {
        return None;
    }
    let mut candidates = operand
        .recipe_references
        .iter()
        .map(|reference| reference.design_reference)
        .filter(|reference| {
            topology.faces.contains(reference) && changed_faces.contains(reference)
        });
    let face = candidates.next()?;
    candidates
        .all(|candidate| candidate == face)
        .then(|| crate::ids::brep_face_id(face))
}

fn relation_members(
    relations: &[crate::history_records::AsmHistoricalRelation],
    owner: i64,
) -> Option<&[i64]> {
    let mut matches = relations
        .iter()
        .filter(|relation| relation.owner_ref == owner);
    let members = matches.next()?.member_refs.as_slice();
    matches.next().is_none().then_some(members)
}

fn resolve_bounded_face_recipe_target<'a>(
    operand: &crate::records::topology::face::DesignFaceOperand,
    preceding: &'a crate::history_records::AsmHistoricalTopology,
    result: &'a crate::history_records::AsmHistoricalTopology,
    inserted_bodies: &[i64],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<Option<i64>, cadmpeg_core::CodecError> {
    let Some(crate::design::decode::operands::FaceRecipeProgramKind::Counted { header_value }) =
        crate::design::decode::operands::face_recipe_program_kind(&operand.recipe_program)
    else {
        return Ok(None);
    };
    if operand.recipe_nodes.len() != header_value
        || operand
            .recipe_nodes
            .iter()
            .any(|node| node.recipe_structure.is_none())
    {
        return Ok(None);
    }
    let Some(first) = operand.recipe_references.first() else {
        return Ok(None);
    };
    let first_clause_len = operand
        .recipe_references
        .iter()
        .take_while(|reference| {
            reference.selector_offset == first.selector_offset
                && reference.token_offset == first.token_offset
        })
        .count();
    let first_clause = &operand.recipe_references[..first_clause_len];
    let topology_faces = selection::FamilySlots::new(&preceding.faces);
    let mut target_candidates = BTreeSet::new();
    for face in first_clause
        .first()
        .into_iter()
        .flat_map(effective_faces)
        .filter_map(|face| stable_ref(face.as_str()))
        .filter(|face| topology_faces.contains(*face))
    {
        target_candidates.insert(face);
    }
    for reference in first_clause.iter().skip(1) {
        let mut candidates = HashSet::new();
        for face in effective_faces(reference)
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .filter(|face| topology_faces.contains(*face))
        {
            candidates.insert(face);
        }
        target_candidates.retain(|face| candidates.contains(face));
    }
    let mut construction_faces = Vec::new();
    'bodies: for body in inserted_bodies {
        let Some(regions) = relation_members(&result.body_regions, *body) else {
            continue;
        };
        let mut unique = None;
        let mut ambiguous = false;
        for region in regions {
            let Some(shells) = relation_members(&result.region_shells, *region) else {
                continue 'bodies;
            };
            for shell in shells {
                let Some(faces) = relation_members(&result.shell_faces, *shell) else {
                    continue 'bodies;
                };
                for face in faces {
                    if unique.is_some_and(|prior| prior != *face) {
                        ambiguous = true;
                    }
                    unique = Some(*face);
                }
            }
        }
        if !ambiguous {
            if let Some(face) = unique {
                construction_faces.push(face);
            }
        }
    }
    if construction_faces.is_empty() {
        return Ok(None);
    }
    let mut face_loop_positions = |face,
                                   topology|
     -> Result<
        Option<(usize, Vec<cadmpeg_ir::math::Point3>)>,
        cadmpeg_core::CodecError,
    > {
        let contexts = face_boundary_contexts_for_slots(&[face], topology, topology_queries);
        let [context] = contexts.as_slice() else {
            return Ok(None);
        };
        let [loop_] = context.loops.as_slice() else {
            return Ok(None);
        };
        let crate::records::topology::historical_context::DesignHistoricalLoopBoundary::Positions(
            rows,
        ) = &loop_.boundary
        else {
            return Ok(None);
        };
        if rows.is_empty() {
            return Ok(None);
        }
        let positions = rows.iter().map(|row| row.position).collect::<Vec<_>>();
        Ok(Some((rows.len(), positions)))
    };
    let mut matches = Vec::new();
    for candidate in target_candidates {
        let Some((edge_count, candidate_points)) = face_loop_positions(candidate, preceding)?
        else {
            continue;
        };
        if edge_count != header_value {
            continue;
        }
        let mut matched = false;
        for face in &construction_faces {
            if let Some((construction_edge_count, construction_points)) =
                face_loop_positions(*face, result)?
            {
                if construction_edge_count >= edge_count
                    && cyclic_point_subsequence(&candidate_points, &construction_points)
                {
                    matched = true;
                    break;
                }
            }
        }
        if matched {
            matches.push(candidate);
        }
    }
    matches.sort_unstable_by(Ord::cmp);
    matches.dedup();
    let [face] = matches.as_slice() else {
        return Ok(None);
    };
    Ok(Some(*face))
}

fn cyclic_point_subsequence(
    candidate: &[cadmpeg_ir::math::Point3],
    construction: &[cadmpeg_ir::math::Point3],
) -> bool {
    let coincident = |left: &cadmpeg_ir::math::Point3, right: &cadmpeg_ir::math::Point3| {
        let dx = left.x - right.x;
        let dy = left.y - right.y;
        let dz = left.z - right.z;
        dx.mul_add(dx, dy.mul_add(dy, dz * dz)) <= EPS_HISTORY_CYCLIC_POINT_SUBSEQUENCE_E12
    };
    let matches_orientation = |candidate: &[cadmpeg_ir::math::Point3]| {
        construction.iter().enumerate().any(|(start, point)| {
            if !coincident(&candidate[0], point) {
                return false;
            }
            let mut cursor = start;
            candidate.iter().skip(1).all(|target| {
                let limit = start + construction.len();
                while cursor < limit {
                    cursor += 1;
                    if coincident(target, &construction[cursor % construction.len()]) {
                        return true;
                    }
                }
                false
            })
        })
    };
    if candidate.is_empty() || candidate.len() > construction.len() {
        return false;
    }
    if matches_orientation(candidate) {
        return true;
    }
    let reversed_matches = construction.iter().enumerate().any(|(start, point)| {
        if !coincident(&candidate[candidate.len() - 1], point) {
            return false;
        }
        let mut cursor = start;
        candidate.iter().rev().skip(1).all(|target| {
            let limit = start + construction.len();
            while cursor < limit {
                cursor += 1;
                if coincident(target, &construction[cursor % construction.len()]) {
                    return true;
                }
            }
            false
        })
    });
    reversed_matches
}

pub(crate) fn bind_body_recipe_operand_history_candidates(
    operands: &mut [crate::records::topology::body_recipe::DesignBodyRecipeOperand],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    mut histories: BindingHistories<'_>,
) -> Result<(), cadmpeg_core::CodecError> {
    if projection_was_finalized(histories.get()) {
        return Ok(());
    }
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        for reference in operand.reference_bindings_mut() {
            reference.preceding_candidate_faces.clear();
            reference.preceding_body_slots.clear();
        }
        operand.resolved_face_slot = None;
        operand.resolved_body_state_id = None;
        operand.resolved_body_slot = None;
        operand.resolved_body_face_slots.clear();
        if let Some((history, state, previous)) =
            body_recipe_operand_state_indexes(operand, scopes, histories.get())
        {
            jobs.push((history, previous, (position, state)));
        }
    }
    visit_state_groups_oldest_first(
        histories.reborrow(),
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous) else {
                return;
            };
            let Some(source) = historical_brep_source(&previous.id) else {
                return;
            };
            let states = history_state_index(history);
            let mut bound = Vec::new();
            for (position, state_index) in group {
                let (Some(operand), Some(state)) = (
                    operands.get_mut(position),
                    history.states().get(state_index),
                ) else {
                    continue;
                };
                if face_changes_across_state_chain(state, previous.state_id, &states).is_none() {
                    continue;
                }
                bind_body_recipe_operand_preceding_faces(operand, topology, source);
                bound.push(position);
            }
            let face_slots = bound
                .iter()
                .filter_map(|&position| operands.get(position))
                .flat_map(
                    crate::records::topology::body_recipe::DesignBodyRecipeOperand::references,
                )
                .map(|reference| {
                    reference
                        .preceding_candidate_faces
                        .iter()
                        .filter_map(|face| stable_ref(face.as_str()))
                        .collect::<BTreeSet<_>>()
                })
                .collect::<Vec<_>>();
            let mut body_slots = if face_slots.is_empty() {
                None
            } else {
                bodies_touching(topology, &face_slots.iter().collect::<Vec<_>>())
            }
            .map(Vec::into_iter);
            let mut complete_bodies = None;
            for position in bound {
                let Some(operand) = operands.get_mut(position) else {
                    continue;
                };
                for reference in operand.reference_bindings_mut() {
                    if let Some(bodies) = body_slots.as_mut().and_then(Iterator::next) {
                        *reference.preceding_body_slots = bodies.into_iter().collect();
                    }
                }
                resolve_body_recipe_operand_preceding_body(operand);
                let Some(body_slot) = operand.resolved_body_slot else {
                    continue;
                };
                if let Some(faces) = complete_bodies
                    .get_or_insert_with(|| CompleteBodyIndex::new(topology))
                    .faces(body_slot)
                {
                    operand.resolved_body_state_id = Some(previous.state_id);
                    operand.resolved_body_face_slots = faces;
                }
            }
        },
    );
    let mut recipes_by_id =
        HashMap::<_, Option<&crate::records::recipes::ConstructionRecipe>>::new();
    for recipe in recipes {
        recipes_by_id
            .entry(recipe.id.as_str())
            .and_modify(|recipe| *recipe = None)
            .or_insert(Some(recipe));
    }
    let identity = |operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand| -> Result<Option<_>, cadmpeg_core::CodecError> {
        let Some(stream) = crate::ids::native_stream(&operand.id) else { return Ok(None) };
        let Some(recipe) = recipes_by_id
            .get(operand.recipe_id.as_str())
            .and_then(|recipe| *recipe) else { return Ok(None) };
        let Some(design) = recipe.design.as_ref() else { return Ok(None) };
        let Some(selector) = design.selector else { return Ok(None) };
        Ok(Some((
            stream.to_owned(),
            operand.asset_id.clone(),
            operand.context_id.clone(),
            operand
                .references()
                .iter()
                .map(|reference| (reference.design_reference, reference.form)).collect::<Vec<_>>(),
            design.id.value.clone(),
            selector.value,
        )))
    };
    let mut resolved_by_identity = HashMap::new();
    for operand in operands.iter() {
        let (Some(identity), Some(body)) = (identity(operand)?, operand.resolved_body_slot) else {
            continue;
        };
        resolved_by_identity
            .entry(identity)
            .and_modify(|resolved| {
                if *resolved != Some(body) {
                    *resolved = None;
                }
            })
            .or_insert(Some(body));
    }
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        if operand.resolved_body_slot.is_some() {
            continue;
        }
        operand.resolved_body_slot = identity(operand)?
            .and_then(|identity| resolved_by_identity.get(&identity).copied())
            .flatten();
        let Some(body_slot) = operand.resolved_body_slot else {
            continue;
        };
        if let Some((history, _, previous)) =
            body_recipe_operand_state_indexes(operand, scopes, histories.get())
        {
            jobs.push((history, previous, (position, body_slot)));
        }
    }
    visit_state_groups_oldest_first(
        histories,
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous) else {
                return;
            };
            let complete_bodies = CompleteBodyIndex::new(topology);
            for (position, body_slot) in group {
                let (Some(operand), Some(faces)) =
                    (operands.get_mut(position), complete_bodies.faces(body_slot))
                else {
                    continue;
                };
                operand.resolved_body_state_id = Some(previous.state_id);
                operand.resolved_body_face_slots = faces;
            }
        },
    );
    Ok(())
}

/// Bind each reference's candidate faces present in the previous-state
/// topology.
fn bind_body_recipe_operand_preceding_faces(
    operand: &mut crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    topology: &AsmHistoricalTopology,
    source: &str,
) {
    for reference in operand.reference_bindings_mut() {
        *reference.preceding_candidate_faces = selection::faces_in_topology(
            reference
                .candidate_faces
                .iter()
                .filter(|face| active_brep_face_matches_source(face, source)),
            topology,
        );
    }
}

/// Resolve the operand's unique face and body from its references'
/// preceding faces and bodies, when they exist.
fn resolve_body_recipe_operand_preceding_body(
    operand: &mut crate::records::topology::body_recipe::DesignBodyRecipeOperand,
) {
    if let [reference] = operand.references().as_slice() {
        if let [face] = reference.preceding_candidate_faces.as_slice() {
            operand.resolved_face_slot = stable_ref(face.as_str());
        }
    }
    let Some(first) = operand.references().first() else {
        return;
    };
    if first.preceding_body_slots.is_empty()
        || operand
            .references()
            .iter()
            .any(|reference| reference.preceding_body_slots.is_empty())
    {
        return;
    }
    let mut intersection = BTreeSet::new();
    for body in &first.preceding_body_slots {
        intersection.insert(*body);
    }
    for reference in &operand.references()[1..] {
        intersection.retain(|body| reference.preceding_body_slots.contains(body));
    }
    if intersection.len() == 1 {
        operand.resolved_body_slot = intersection.into_iter().next();
    }
}

fn body_recipe_operand_history_pair<'a>(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &'a [AsmHistory],
) -> Option<(&'a AsmHistory, &'a AsmDeltaState, &'a AsmDeltaState)> {
    let stream = crate::ids::native_stream(&operand.id)?;
    let mut matching_scopes = scopes.iter().filter(|scope| {
        scope.record_index == operand.scope_record_index
            && crate::ids::native_stream(&scope.id) == Some(stream)
    });
    let scope = matching_scopes.next()?;
    if matching_scopes.next().is_some() {
        return None;
    }
    let state_id = scope.history_state_id()?;
    let previous_state_id = effective_scope_previous_history_state_id(scope, histories)?;
    let (history, state, previous) =
        unique_history_state_pair(histories, state_id, previous_state_id)?;
    Some((history, state, previous))
}

/// History, state and previous-state indexes of a body recipe operand.
fn body_recipe_operand_state_indexes(
    operand: &crate::records::topology::body_recipe::DesignBodyRecipeOperand,
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    histories: &[AsmHistory],
) -> Option<(usize, usize, usize)> {
    let (history, state, previous) = body_recipe_operand_history_pair(operand, scopes, histories)?;
    let (history_index, state_index) = history_state_indexes(histories, history, state)?;
    let (_, previous_index) = history_state_indexes(histories, history, previous)?;
    Some((history_index, state_index, previous_index))
}

fn active_brep_face_matches_source(face: &cadmpeg_ir::ids::FaceId, source: &str) -> bool {
    face.as_str().starts_with("f3d:brep:entity#") || entity_in_brep_source(face.as_str(), source)
}

fn entity_in_brep_source(id: &str, source: &str) -> bool {
    id.strip_prefix("f3d:brep/")
        .and_then(|tail| tail.strip_prefix(source))
        .is_some_and(|tail| tail.starts_with('/'))
}

#[derive(Debug, PartialEq)]
enum LegacyFaceResolution {
    Historical(i64),
    Active(cadmpeg_ir::ids::FaceId),
}

fn select_legacy_extrude_face_candidate(
    mut candidates: Vec<cadmpeg_ir::ids::FaceId>,
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    history_source: Option<&str>,
) -> Option<LegacyFaceResolution> {
    for changed_only in [true, false] {
        let mut faces = candidates.iter().filter(|face| {
            stable_ref(face.as_str()).is_some_and(|slot| {
                topology.faces.contains(&slot) && (!changed_only || changed_faces.contains(&slot))
            })
        });
        let face = faces.next();
        if faces.next().is_none() {
            let Some(face) = face else {
                continue;
            };
            if let Some(slot) = stable_ref(face.as_str()) {
                return Some(LegacyFaceResolution::Historical(slot));
            }
        }
    }
    if let Some(source) = history_source {
        let unique_index = {
            let mut matches = candidates
                .iter()
                .enumerate()
                .filter(|(_, face)| entity_in_brep_source(face.as_str(), source));
            let first = matches.next().map(|(index, _)| index);
            first.filter(|_| matches.next().is_none())
        };
        if let Some(index) = unique_index {
            return Some(LegacyFaceResolution::Active(candidates.swap_remove(index)));
        }
    }
    if candidates.len() == 1 {
        candidates.pop().map(LegacyFaceResolution::Active)
    } else {
        None
    }
}

/// One body/native selection row, minting the non-blank native member at the
/// boundary where the F3D record states it.
///
/// `None` when the record states a blank native member: a row the IR carrier
/// does not hold.
fn body_member<B>(body: B, native: String) -> Option<cadmpeg_ir::features::BodyMember<B>> {
    Some(cadmpeg_ir::features::BodyMember::new(
        body,
        cadmpeg_core::text::NonBlankString::new(native)?,
    ))
}

fn historical_brep_source(state_id: &str) -> Option<&str> {
    state_id
        .rsplit_once("/BREP.")
        .or_else(|| state_id.rsplit_once("BREP."))?
        .1
        .split_once(":asm-")
        .map(|(source, _)| source)
}

fn resolve_direct_face_recipe_clauses(
    references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &crate::history_records::AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
) -> Vec<i64> {
    let mut clauses = Vec::<(
        u64,
        u64,
        Vec<&crate::records::dimensions::DesignRecipeReference>,
    )>::new();
    for reference in references {
        let key = (reference.selector_offset, reference.token_offset);
        if let Some((_, _, references)) = clauses
            .iter_mut()
            .find(|(selector, token, _)| (*selector, *token) == key)
        {
            references.push(reference);
        } else {
            let grouped = vec![reference];
            clauses.push((key.0, key.1, grouped));
        }
    }
    let topology_faces = selection::FamilySlots::new(&topology.faces);
    let mut resolved = Vec::new();
    for (_, _, references) in clauses {
        let mut intersection = None::<HashSet<i64>>;
        for reference in references {
            let candidates = if reference.candidate_faces.is_empty() {
                &reference.alternate_selector_faces
            } else {
                &reference.candidate_faces
            };
            let mut eligible = HashSet::new();
            for face in candidates
                .iter()
                .filter_map(|face| stable_ref(face.as_str()))
                .filter(|face| topology_faces.contains(*face) && changed_faces.contains(face))
            {
                eligible.insert(face);
            }
            let candidates = eligible;
            if candidates.is_empty() {
                return Vec::new();
            }
            intersection = Some(match intersection {
                None => candidates,
                Some(mut intersection) => {
                    intersection.retain(|face| candidates.contains(face));
                    intersection
                }
            });
        }
        let Some(intersection) = intersection else {
            return Vec::new();
        };
        let mut candidates = intersection.into_iter();
        let Some(face) = candidates.next() else {
            return Vec::new();
        };
        if candidates.next().is_some() {
            return Vec::new();
        }
        if !resolved.contains(&face) {
            resolved.push(face);
        }
    }
    resolved
}

fn bind_profile_face_group_cardinality(
    decode: &cadmpeg_core::decode::DecodeContext<'_>,
    operands: &mut [crate::records::topology::face::DesignFaceOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    operand_groups: &[crate::records::topology::construction::DesignConstructionOperandGroup],
    histories: &[AsmHistory],
    scope_histories: &HashMap<String, String>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut state_indexes = SnapshotCache::default();
    for scope in scopes {
        let scoped_history = if scope_histories.contains_key(&scope.id) {
            let Some(history) = bound_scope_history(&scope.id, scope_histories, histories) else {
                continue;
            };
            Some(history)
        } else {
            None
        };
        let scoped_histories = scoped_history.map_or(histories, std::slice::from_ref);
        let Some(profile_groups) = crate::design::face_resolve::extrude_profile_group_roots(
            decode,
            scope,
            operand_groups,
        )?
        else {
            continue;
        };
        for group in profile_groups {
            let Some(indices) = crate::design::face_resolve::extrude_profile_group_operand_indices(
                decode,
                group,
                operand_groups,
                operands,
            )?
            else {
                continue;
            };
            if group.members().len() != indices.len()
                || indices.iter().any(|index| {
                    let operand = &operands[*index];
                    !operand.resolved_face_slots.is_empty()
                        || operand.resolved_active_face.is_some()
                })
            {
                continue;
            }
            let (Some(state_id), Some(previous_state_id)) = (
                scope.history_state_id(),
                effective_scope_previous_history_state_id(scope, scoped_histories),
            ) else {
                continue;
            };
            let history_pair = if scoped_history.is_some() {
                bound_history_state_pair(
                    &scope.id,
                    state_id,
                    previous_state_id,
                    scope_histories,
                    histories,
                )
            } else {
                unique_history_state_pair(histories, state_id, previous_state_id)
            };
            let Some((history, state, previous)) = history_pair else {
                continue;
            };
            let states = state_indexes.get(history, history_state_index);
            let changed_faces = face_changes_across_state_chain(state, previous_state_id, states);
            let (Some(topology), Some(changed_faces)) = (history.topology(previous), changed_faces)
            else {
                continue;
            };
            let paired_aggregate = crate::design::face_resolve::is_paired_extrude_profile_aggregate(
                group,
                operand_groups,
                operands,
            );
            let faces =
                if paired_aggregate {
                    if let Some(transition) = state.transition.as_ref().filter(|transition| {
                        transition.previous_state_id == Some(previous_state_id)
                    }) {
                        let mut preceding_faces = HashSet::new();
                        for face in &topology.faces {
                            preceding_faces.insert(*face);
                        }
                        let mut deleted = transition.topology.faces.deleted.clone();
                        deleted.sort_unstable_by(Ord::cmp);
                        deleted.dedup();
                        (deleted.len() == group.members().len()
                            && deleted.len() == transition.topology.faces.deleted.len()
                            && deleted.iter().all(|face| {
                                preceding_faces.contains(face)
                                    && topology
                                        .face_surfaces
                                        .iter()
                                        .filter(|binding| binding.entity == *face)
                                        .count()
                                        == 1
                            }))
                        .then_some(deleted)
                    } else {
                        None
                    }
                } else {
                    profile_face_group_cardinality_candidates(
                        topology,
                        &changed_faces,
                        group.members().len(),
                    )
                };
            let Some(faces) = faces else {
                continue;
            };
            for (index, face) in indices.into_iter().zip(faces) {
                let face_id = historical_face_id(face)?;
                let preceding_id = face_id.clone();
                operands[index].preceding_candidate_faces =
                    std::iter::once(preceding_id).collect::<Vec<_>>();
                operands[index].changed_candidate_faces =
                    std::iter::once(face_id).collect::<Vec<_>>();
                operands[index].resolved_face_slots = vec![face];
            }
        }
    }
    Ok(())
}

fn profile_face_group_cardinality_candidates(
    topology: &AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    member_count: usize,
) -> Option<Vec<i64>> {
    let preceding_faces = selection::FamilySlots::new(&topology.faces);
    let mut faces_by_carrier = HashMap::<i64, Vec<i64>>::new();
    for face in changed_faces
        .iter()
        .copied()
        .filter(|face| preceding_faces.contains(*face))
    {
        let mut bindings = topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == face);
        let Some(carrier) = bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if bindings.next().is_none() {
            faces_by_carrier.entry(carrier).or_default().push(face);
        }
    }
    let mut candidates = faces_by_carrier
        .into_values()
        .filter(|faces| faces.len() == member_count);
    let mut faces = candidates.next()?;
    if candidates.next().is_some() {
        return None;
    }
    faces.sort_unstable_by(Ord::cmp);
    faces.dedup();
    (faces.len() == member_count).then_some(faces)
}

fn face_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<HashSet<i64>> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut changed = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for face in transition
            .topology
            .faces
            .deleted
            .iter()
            .chain(&transition.topology.faces.updated)
        {
            changed.insert(*face);
        }
        let previous = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())?;
        current = previous;
    }
    Some(changed)
}

#[derive(Debug, PartialEq)]
struct EdgeChanges {
    deleted: HashSet<i64>,
    updated: HashSet<i64>,
}

fn edge_changes_across_state_chain<'a>(
    state: &'a AsmDeltaState,
    previous_state_id: i64,
    states: &HashMap<i64, Option<&'a AsmDeltaState>>,
) -> Option<EdgeChanges> {
    let mut current = state;
    let mut visited = HashSet::new();
    let mut deleted = HashSet::new();
    let mut updated = HashSet::new();
    while current.state_id != previous_state_id {
        if !visited.insert(current.state_id) {
            return None;
        }
        let transition = current.transition.as_ref()?;
        for edge in &transition.topology.edges.deleted {
            deleted.insert(*edge);
        }
        for edge in &transition.topology.edges.updated {
            updated.insert(*edge);
        }
        let previous = transition
            .previous_state_id
            .and_then(|id| states.get(&id).copied().flatten())?;
        current = previous;
    }
    Some(EdgeChanges { deleted, updated })
}

fn face_operand_support_candidates<'a>(
    candidates: &'a [cadmpeg_ir::ids::FaceId],
    first_reference: Option<&crate::records::dimensions::DesignRecipeReference>,
    recipe_reference: Option<i32>,
) -> std::borrow::Cow<'a, [cadmpeg_ir::ids::FaceId]> {
    let mut candidates = std::borrow::Cow::Borrowed(candidates);
    for face in first_reference
        .filter(|reference| Some(reference.design_reference) == recipe_reference.map(i64::from))
        .into_iter()
        .flat_map(effective_faces)
    {
        if !candidates.contains(face) {
            candidates.to_mut().push(face.clone());
        }
    }
    candidates
}

fn historical_face_support_contexts<'a>(
    candidates: &[cadmpeg_ir::ids::FaceId],
    history: &'a AsmHistory,
    preceding_topology: &'a AsmHistoricalTopology,
    changed_faces: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<crate::records::topology::historical_context::DesignHistoricalFaceSupportContext> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        preceding_faces.insert(*face);
    }
    let mut contexts = Vec::new();
    'candidates: for candidate in candidates {
        let Some(active_face_slot) = stable_ref(candidate.as_str()) else {
            continue;
        };
        let mut preceding_bindings = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == active_face_slot);
        let surface_slot = if let Some(binding) = preceding_bindings.next() {
            if preceding_bindings.next().is_some() {
                continue;
            }
            binding.carrier
        } else {
            let mut carriers = HashSet::new();
            for topology in history
                .states()
                .iter()
                .filter_map(|state| history.topology(state))
            {
                let mut bindings = topology
                    .face_surfaces
                    .iter()
                    .filter(|binding| binding.entity == active_face_slot);
                if let Some(binding) = bindings.next() {
                    if bindings.next().is_some() {
                        continue 'candidates;
                    }
                    carriers.insert(binding.carrier);
                }
            }
            if carriers.len() != 1 {
                continue;
            }
            let Some(carrier) = carriers.into_iter().next() else {
                continue;
            };
            carrier
        };
        let mut preceding_face_slots = preceding_topology
            .face_surfaces
            .iter()
            .filter(|binding| {
                binding.carrier == surface_slot && preceding_faces.contains(&binding.entity)
            })
            .map(|binding| binding.entity)
            .collect::<Vec<_>>();
        preceding_face_slots.sort_unstable_by(Ord::cmp);
        preceding_face_slots.dedup();
        if preceding_face_slots.is_empty() {
            continue;
        }
        let changed_preceding_face_slots = preceding_face_slots
            .iter()
            .copied()
            .filter(|face| changed_faces.contains(face))
            .collect::<Vec<_>>();
        let preceding_face_boundaries = face_boundary_contexts_for_slots(
            &preceding_face_slots,
            preceding_topology,
            topology_queries,
        );

        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceSupportContext {
                active_face_slot,
                surface_slot,
                preceding_face_slots,
                preceding_face_boundaries,
                changed_preceding_face_slots,
            },
        );
    }
    contexts
}

fn face_boundary_edges<'a>(
    faces: &[cadmpeg_ir::ids::FaceId],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<i64> {
    let boundaries = topology_queries
        .boundaries
        .get(topology, topology_cache::FaceBoundaries::new);
    let mut edges = Vec::new();
    for face in faces.iter().filter_map(|face| stable_ref(face.as_str())) {
        boundaries.extend(topology, face, &mut edges);
    }
    edges.sort_unstable_by(Ord::cmp);
    edges.dedup();
    edges
}

fn collect_reference_edge_sets<'a>(
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<Vec<i64>> {
    let mut sets = Vec::new();
    for faces in reference_faces {
        let faces = selection::faces_in_topology(faces, topology);
        let edges = face_boundary_edges(&faces, topology, topology_queries);

        sets.push(edges);
    }
    sets
}

fn face_boundary_contexts_for_slots<'a>(
    face_slots: &[i64],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Vec<crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext> {
    let index = topology_queries.loops.get(topology, LoopIndex::new);
    let edge_endpoints = topology_queries
        .endpoints
        .get(topology, topology_cache::endpoint_index);
    let mut contexts = Vec::new();
    'faces: for face_slot in face_slots {
        let Some(face_loops) = index.face_loops.get(face_slot).copied().flatten() else {
            continue;
        };
        let mut loops = Vec::new();
        for loop_slot in face_loops {
            let Some(loop_coedges) = index.loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            let mut coedges = Vec::new();
            for coedge_slot in loop_coedges {
                let Some(edge_slot) = index.coedge_edges.get(coedge_slot).copied().flatten() else {
                    continue 'faces;
                };

                coedges.push(
                    crate::records::topology::historical_context::DesignHistoricalLoopCoedge {
                        coedge_slot: *coedge_slot,
                        edge_slot,
                    },
                );
            }
            let boundary = historical_loop_boundary(coedges, index, edge_endpoints);

            loops.push(
                crate::records::topology::historical_context::DesignHistoricalFaceLoopContext {
                    loop_slot: *loop_slot,
                    boundary,
                },
            );
        }

        contexts.push(
            crate::records::topology::historical_context::DesignHistoricalFaceBoundaryContext {
                face_slot: *face_slot,
                loops,
            },
        );
    }
    contexts
}

fn historical_loop_boundary(
    coedges: Vec<crate::records::topology::historical_context::DesignHistoricalLoopCoedge>,
    index: &LoopIndex<'_>,
    edge_endpoints: &topology_cache::EdgeEndpoints,
) -> crate::records::topology::historical_context::DesignHistoricalLoopBoundary {
    use crate::records::topology::{
        historical_context::DesignHistoricalLoopBoundary,
        historical_context::DesignHistoricalLoopPoint,
        historical_context::DesignHistoricalLoopPosition,
        historical_context::DesignHistoricalLoopVertex,
    };
    let mut vertices = Vec::new();
    for (ordinal, coedge) in coedges.iter().enumerate() {
        let previous = coedges[(ordinal + coedges.len() - 1) % coedges.len()].edge_slot;

        let endpoints = |slot| edge_endpoints.get(&slot).copied().flatten();
        let (Some(previous), Some(current)) = (endpoints(previous), endpoints(coedge.edge_slot))
        else {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        };
        let mut shared = previous
            .into_iter()
            .filter(|vertex| current.contains(vertex));
        let Some(vertex_slot) = shared.next() else {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        };
        if shared.any(|vertex| vertex != vertex_slot) {
            return DesignHistoricalLoopBoundary::Coedges(coedges);
        }

        vertices.push(DesignHistoricalLoopVertex {
            coedge: coedge.clone(),
            vertex_slot,
        });
    }
    if vertices.is_empty() {
        return DesignHistoricalLoopBoundary::Coedges(coedges);
    }
    let mut points = Vec::new();
    for vertex in &vertices {
        let Some(point_slot) = index
            .vertex_points
            .get(&vertex.vertex_slot)
            .copied()
            .flatten()
        else {
            return DesignHistoricalLoopBoundary::Vertices(vertices);
        };

        points.push(DesignHistoricalLoopPoint {
            vertex: vertex.clone(),
            point_slot,
        });
    }
    let mut positions = Vec::new();
    for point in &points {
        let Some(position) = index.positions.get(&point.point_slot).copied().flatten() else {
            return DesignHistoricalLoopBoundary::Points(points);
        };

        positions.push(DesignHistoricalLoopPosition {
            point: point.clone(),
            position,
        });
    }
    DesignHistoricalLoopBoundary::Positions(positions)
}

fn preceding_support_face_slots(
    result_faces: &[cadmpeg_ir::ids::FaceId],
    result_topology: &AsmHistoricalTopology,
    preceding_topology: &AsmHistoricalTopology,
) -> Vec<i64> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding_topology.faces {
        preceding_faces.insert(*face);
    }
    let mut support_faces = Vec::new();
    for result_face in result_faces {
        let Some(result_face) = stable_ref(result_face.as_str()) else {
            continue;
        };
        let mut result_bindings = result_topology
            .face_surfaces
            .iter()
            .filter(|binding| binding.entity == result_face);
        let Some(carrier) = result_bindings.next().map(|binding| binding.carrier) else {
            continue;
        };
        if result_bindings.next().is_some() {
            continue;
        }
        let mut preceding_bindings = preceding_topology.face_surfaces.iter().filter(|binding| {
            binding.carrier == carrier && preceding_faces.contains(&binding.entity)
        });
        let Some(preceding_face) = preceding_bindings.next().map(|binding| binding.entity) else {
            continue;
        };
        if preceding_bindings.next().is_none() && !support_faces.contains(&preceding_face) {
            support_faces.push(preceding_face);
        }
    }
    support_faces
}

#[derive(Clone, Copy)]
struct EdgeBoundaryContext<'a, 'b> {
    state: &'a AsmDeltaState,
    topology: &'a AsmHistoricalTopology,
    boundary_edges: &'b [i64],
}

/// Face boundaries named by edge-recipe reference contexts, stored once per
/// historical state and face slot.
#[derive(Default)]
pub(crate) struct FaceBoundaryArena {
    records: BTreeMap<String, crate::records::topology::historical_context::AsmFaceBoundary>,
    /// Identities of faces whose loop topology is incomplete in their state.
    incomplete: HashSet<String>,
}

impl FaceBoundaryArena {
    /// Return the boundary identities of the complete `face_slots` in the
    /// state, in slot order, and store each boundary not yet present.
    fn intern<'a>(
        &mut self,
        state: &AsmDeltaState,
        topology: &'a AsmHistoricalTopology,
        face_slots: &[i64],
        topology_queries: &mut TopologyQueryCache<'a>,
    ) -> Vec<String> {
        let mut ids = Vec::new();
        for face_slot in face_slots {
            let Some(id) = crate::records::topology::historical_context::AsmFaceBoundary::identity(
                &state.id,
                state.byte_offset,
                *face_slot,
            ) else {
                continue;
            };
            if self.incomplete.contains(&id) {
                continue;
            }
            if !self.records.contains_key(&id) {
                let Some(boundary) = face_boundary_contexts_for_slots(
                    std::slice::from_ref(face_slot),
                    topology,
                    topology_queries,
                )
                .pop() else {
                    self.incomplete.insert(id);
                    continue;
                };
                self.records.insert(
                    id.clone(),
                    crate::records::topology::historical_context::AsmFaceBoundary {
                        id: id.clone(),
                        parent: state.id.clone(),
                        face_slot: boundary.face_slot,
                        loops: boundary.loops,
                    },
                );
            }
            ids.push(id);
        }
        ids
    }

    /// The stored boundaries in identity order.
    pub(crate) fn into_records(
        self,
    ) -> Vec<crate::records::topology::historical_context::AsmFaceBoundary> {
        self.records.into_values().collect()
    }
}

fn edge_recipe_reference_context<'a>(
    reference_ordinal: u32,
    reference: &crate::records::dimensions::DesignRecipeReference,
    result: EdgeBoundaryContext<'a, '_>,
    preceding: EdgeBoundaryContext<'a, '_>,
    changed_edges: &HashSet<i64>,
    topology_queries: &mut TopologyQueryCache<'a>,
    face_boundaries: &mut FaceBoundaryArena,
) -> crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
    let candidate_faces = if reference.candidate_faces.is_empty() {
        reference.alternate_selector_faces.as_slice()
    } else {
        reference.candidate_faces.as_slice()
    };
    let face_slots = |faces: &[cadmpeg_ir::ids::FaceId]| {
        faces
            .iter()
            .filter_map(|face| stable_ref(face.as_str()))
            .collect::<Vec<_>>()
    };
    let result_faces = selection::faces_in_topology(candidate_faces, result.topology);
    let result_face_boundary_ids = face_boundaries.intern(
        result.state,
        result.topology,
        &face_slots(&result_faces),
        topology_queries,
    );
    let result_edges = face_boundary_edges(&result_faces, result.topology, topology_queries);
    let result_shared_edge_slots = result
        .boundary_edges
        .iter()
        .copied()
        .filter(|edge| result_edges.contains(edge))
        .collect::<Vec<_>>();
    let preceding_faces = selection::faces_in_topology(candidate_faces, preceding.topology);
    let preceding_face_boundary_ids = face_boundaries.intern(
        preceding.state,
        preceding.topology,
        &face_slots(&preceding_faces),
        topology_queries,
    );
    let preceding_support_face_slots =
        preceding_support_face_slots(&result_faces, result.topology, preceding.topology);
    let preceding_support_face_boundary_ids = face_boundaries.intern(
        preceding.state,
        preceding.topology,
        &preceding_support_face_slots,
        topology_queries,
    );
    let preceding_edges =
        face_boundary_edges(&preceding_faces, preceding.topology, topology_queries);
    let shared_edge_slots = preceding
        .boundary_edges
        .iter()
        .copied()
        .filter(|edge| preceding_edges.contains(edge))
        .collect::<Vec<_>>();
    let changed_shared_edge_slots = shared_edge_slots
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    let mut support_edges = HashSet::new();
    for edge in preceding_support_face_boundary_ids
        .iter()
        .filter_map(|id| face_boundaries.records.get(id))
        .flat_map(|face| &face.loops)
        .flat_map(|face_loop| face_loop.boundary.coedges().map(|row| row.edge_slot))
    {
        support_edges.insert(edge);
    }
    let mut changed_reference_edge_slots = preceding_edges
        .iter()
        .copied()
        .chain(support_edges.iter().copied())
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    changed_reference_edge_slots.sort_unstable_by(Ord::cmp);
    changed_reference_edge_slots.dedup();
    crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext {
        reference_ordinal,
        result_faces,
        result_face_boundary_ids,
        result_shared_edge_slots,
        preceding_faces,
        preceding_face_boundary_ids,
        preceding_support_face_slots,
        preceding_support_face_boundary_ids,
        shared_edge_slots,
        changed_shared_edge_slots,
        changed_reference_edge_slots,
    }
}

/// Resolve the unique candidate edge shared by the non-null face references
/// in the first side of a standard edge recipe.
fn side_one_recipe_edge(
    structure: Option<&crate::records::topology::edge_recipe::DesignEdgeRecipeStructure>,
    reference_contexts: &[crate::records::topology::historical_context::DesignEdgeRecipeReferenceContext],
    selectors: &[crate::records::topology::edge_recipe::DesignEdgeRecipeSelectorContext],
    candidate_edges: &[i64],
) -> Option<i64> {
    let side = structure.and_then(|structure| structure.sides.first())?;
    let mut ordinals = Vec::new();
    for value in std::iter::once(side.header_value)
        .chain(side.scalars.iter().copied())
        .filter(|value| *value != 0)
    {
        let ordinal = usize::try_from(value)
            .ok()
            .and_then(|value| value.checked_sub(1))?;

        ordinals.push(ordinal);
    }
    ordinals.sort_unstable_by(Ord::cmp);
    ordinals.dedup();
    let mut edge_sets = Vec::new();
    for ordinal in ordinals {
        let context = reference_contexts.get(ordinal)?;
        if Some(context.reference_ordinal) != u32::try_from(ordinal).ok() {
            return None;
        }

        edge_sets.push(context.shared_edge_slots.as_slice());
    }
    if edge_sets.iter().any(|edges| edges.is_empty()) {
        return None;
    }
    let first = edge_sets.first()?;
    let mut candidates = first.to_vec();
    for edges in edge_sets.iter().skip(1) {
        candidates.retain(|candidate| edges.contains(candidate));
    }
    candidates.retain(|candidate| candidate_edges.contains(candidate));
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => {
            crate::design::edge_resolve::resolved_edge_candidate_intersection(selectors, edge_sets)
        }
    }
}

pub(crate) fn bind_edge_operand_history_candidates(
    operands: &mut [crate::records::topology::edge_identity::DesignEdgeOperand],
    scopes: &[crate::records::feature::scope::DesignParameterScope],
    recipes: &[crate::records::recipes::ConstructionRecipe],
    histories: BindingHistories<'_>,
    scope_histories: &HashMap<String, String>,
) -> Result<
    Vec<crate::records::topology::historical_context::AsmFaceBoundary>,
    cadmpeg_core::CodecError,
> {
    if projection_was_finalized(histories.get()) {
        return Ok(Vec::new());
    }
    let mut recipe_record_indices = HashMap::new();
    for recipe in recipes {
        if let Some(index) = recipe.record_index {
            recipe_record_indices.insert(recipe.id.as_str(), index.value);
        }
    }
    let mut active = Vec::new();
    let mut jobs = Vec::new();
    for (position, operand) in operands.iter_mut().enumerate() {
        operand.result_candidate_faces.clear();
        operand.result_boundary_edge_slots.clear();
        operand.preceding_candidate_faces.clear();
        operand.terminal_candidate_faces.clear();
        operand.changed_candidate_faces.clear();
        operand.preceding_boundary_edge_slots.clear();
        operand.terminal_boundary_edge_slots.clear();
        operand.changed_boundary_edge_slots.clear();
        operand.deleted_boundary_edge_slots.clear();
        operand.updated_boundary_edge_slots.clear();
        operand.treatment_radius_candidates.clear();
        operand.changed_boundary_edge_contexts.clear();
        operand.terminal_boundary_edge_contexts.clear();
        operand.recipe_reference_contexts.clear();
        operand.recipe_selectors.clear();
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        operand.resolved_axis = None;
        let stream = crate::ids::native_stream(&operand.id);
        let mut matching_scopes = scopes.iter().filter(|scope| {
            scope.record_index == operand.scope_record_index
                && crate::ids::native_stream(&scope.id) == stream
        });
        let Some(scope) = matching_scopes.next() else {
            continue;
        };
        if matching_scopes.next().is_some() {
            continue;
        }
        let Some(state_id) = scope.history_state_id() else {
            active.push((position, scope));
            continue;
        };
        let Some(previous_state_id) =
            effective_scope_previous_history_state_id(scope, histories.get())
        else {
            active.push((position, scope));
            continue;
        };
        let Some((history, state, previous)) = bound_history_state_pair(
            &scope.id,
            state_id,
            previous_state_id,
            scope_histories,
            histories.get(),
        ) else {
            continue;
        };
        let (Some((history_index, state_index)), Some((_, previous_index))) = (
            history_state_indexes(histories.get(), history, state),
            history_state_indexes(histories.get(), history, previous),
        ) else {
            continue;
        };
        jobs.push((
            history_index,
            previous_index,
            (position, scope, state_index),
        ));
    }
    let mut first_error = None;
    if !active.is_empty() {
        let terminal_topologies = terminal_topologies(histories.get());
        let mut topology_queries = TopologyQueryCache::default();
        for (position, scope) in active {
            let Some(operand) = operands.get_mut(position) else {
                continue;
            };
            if let Err(error) = bind_active_edge_operand_for_scope(
                operand,
                scope,
                &terminal_topologies,
                &mut topology_queries,
            ) {
                keep_first_error(&mut first_error, position, error);
            }
        }
    }
    let mut face_boundaries = FaceBoundaryArena::default();
    visit_state_groups_oldest_first(
        histories,
        jobs,
        |histories, history_index, previous_index, group| {
            let Some(history) = histories.get(history_index) else {
                return;
            };
            let Some(previous) = history.states().get(previous_index) else {
                return;
            };
            let Some(topology) = history.topology(previous) else {
                return;
            };
            let states = history_state_index(history);
            let mut caches = EdgeOperandCaches::default();
            for (position, scope, state_index) in group {
                let (Some(operand), Some(state)) = (
                    operands.get_mut(position),
                    history.states().get(state_index),
                ) else {
                    continue;
                };
                let Some(result_topology) = history.topology(state) else {
                    continue;
                };
                let transition = EdgeOperandTransition {
                    scope,
                    state,
                    previous,
                    result_topology,
                    topology,
                    states: &states,
                };
                if let Err(error) = bind_historical_edge_operand(
                    operand,
                    &transition,
                    &recipe_record_indices,
                    &mut caches,
                    &mut face_boundaries,
                ) {
                    keep_first_error(&mut first_error, position, error);
                }
            }
        },
    );
    match first_error {
        Some((_, error)) => Err(error),
        None => Ok(face_boundaries.into_records()),
    }
}

/// Keep the error of the earliest item, as a pass that stops at its first
/// failing item would return.
fn keep_first_error(
    first: &mut Option<(usize, cadmpeg_core::CodecError)>,
    position: usize,
    error: cadmpeg_core::CodecError,
) {
    if first
        .as_ref()
        .is_none_or(|(earliest, _)| position < *earliest)
    {
        *first = Some((position, error));
    }
}

/// The topology of each history's one terminal state.
fn terminal_topologies(histories: &[AsmHistory]) -> Vec<(i64, &AsmHistoricalTopology)> {
    let mut terminal_topologies = Vec::new();
    for history in histories {
        let mut preceding = HashSet::new();
        for state in history.states() {
            if let Some(previous) = state
                .transition
                .as_ref()
                .and_then(|transition| transition.previous_state_id)
            {
                preceding.insert(previous);
            }
        }
        let mut terminals = history
            .states()
            .iter()
            .filter(|state| !preceding.contains(&state.state_id));
        if let Some(state) = terminals.next().filter(|_| terminals.next().is_none()) {
            if let Some(topology) = history.topology(state) {
                terminal_topologies.push((state.state_id, topology));
            }
        }
    }
    terminal_topologies
}

/// One edge operand's scope and state pair with their topologies.
struct EdgeOperandTransition<'a> {
    scope: &'a crate::records::feature::scope::DesignParameterScope,
    state: &'a AsmDeltaState,
    previous: &'a AsmDeltaState,
    result_topology: &'a AsmHistoricalTopology,
    topology: &'a AsmHistoricalTopology,
    states: &'a HashMap<i64, Option<&'a AsmDeltaState>>,
}

/// Indexes over the topologies of one previous state and its results.
#[derive(Default)]
struct EdgeOperandCaches<'a> {
    treatment: TreatmentCache<'a>,
    topology_queries: TopologyQueryCache<'a>,
    recipe_topologies: RecipeTopologyCache<'a>,
}

fn bind_historical_edge_operand<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    transition: &EdgeOperandTransition<'a>,
    recipe_record_indices: &HashMap<&str, i32>,
    caches: &mut EdgeOperandCaches<'a>,
    face_boundaries: &mut FaceBoundaryArena,
) -> Result<(), cadmpeg_core::CodecError> {
    let &EdgeOperandTransition {
        scope,
        state,
        previous,
        result_topology,
        topology,
        states,
    } = transition;
    let previous_state_id = previous.state_id;
    let EdgeOperandCaches {
        treatment: treatment_cache,
        topology_queries,
        recipe_topologies,
    } = caches;
    for reference in &mut operand.recipe_references {
        bind_historical_recipe_reference_candidates(reference, topology, recipe_topologies)?;
    }
    if let Some(recipe_record_index) = recipe_record_indices.get(operand.recipe_id.as_str()) {
        operand.candidate_faces =
            historical_recipe_faces(i64::from(*recipe_record_index), topology, recipe_topologies)?;
    }
    let Some(changed_faces) = face_changes_across_state_chain(state, previous_state_id, states)
    else {
        return Ok(());
    };
    let Some(EdgeChanges {
        deleted: chain_deleted_edges,
        updated: chain_updated_edges,
    }) = edge_changes_across_state_chain(state, previous_state_id, states)
    else {
        return Ok(());
    };
    let preceding_faces = selection::FamilySlots::new(&topology.faces);
    let inserted_faces = result_topology
        .faces
        .iter()
        .copied()
        .filter(|face| !preceding_faces.contains(*face))
        .collect::<Vec<_>>();
    let result_edges = selection::FamilySlots::new(&result_topology.edges);
    let deleted_edges = topology
        .edges
        .iter()
        .copied()
        .filter(|edge| !result_edges.contains(*edge) && chain_deleted_edges.contains(edge))
        .collect::<Vec<_>>();
    let updated_edges = topology
        .edges
        .iter()
        .copied()
        .filter(|edge| {
            result_edges.contains(*edge)
                && (chain_deleted_edges.contains(edge) || chain_updated_edges.contains(edge))
        })
        .collect::<Vec<_>>();
    operand.recipe_state_id = Some(previous_state_id);
    operand.result_candidate_faces =
        selection::faces_in_topology(&operand.candidate_faces, result_topology);
    operand.result_boundary_edge_slots = face_boundary_edges(
        &operand.result_candidate_faces,
        result_topology,
        topology_queries,
    );
    operand.preceding_candidate_faces =
        selection::faces_in_topology(&operand.candidate_faces, topology);
    operand.changed_candidate_faces = (operand.preceding_candidate_faces.iter().filter(|face| {
        stable_ref(face.as_str()).is_some_and(|slot| changed_faces.contains(&slot))
    }))
    .cloned()
    .collect::<Vec<_>>();
    operand.preceding_boundary_edge_slots = face_boundary_edges(
        &operand.preceding_candidate_faces,
        topology,
        topology_queries,
    );
    let mut changed_edges = HashSet::new();
    for edge in deleted_edges.iter().chain(&updated_edges) {
        changed_edges.insert(*edge);
    }
    operand.changed_boundary_edge_slots = operand
        .preceding_boundary_edge_slots
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .collect::<Vec<_>>();
    operand.deleted_boundary_edge_slots = selection::boundary_edges_in_changes(
        &operand.preceding_boundary_edge_slots,
        &deleted_edges,
    );
    operand.updated_boundary_edge_slots = selection::boundary_edges_in_changes(
        &operand.preceding_boundary_edge_slots,
        &updated_edges,
    );
    operand.treatment_radius_candidates = treatment_radius_candidates(
        Some(&operand.result_candidate_faces),
        &inserted_faces,
        result_topology,
        topology,
        &deleted_edges,
        treatment_cache,
    );
    let edge_contexts = topology_queries
        .edge_contexts
        .get(topology, selection::EdgeContextIndex::new);
    operand.changed_boundary_edge_contexts = operand
        .changed_boundary_edge_slots
        .iter()
        .map(|&edge| edge_contexts.context(edge))
        .collect::<Vec<_>>();
    let mut reference_contexts = Vec::new();
    for (ordinal, reference) in operand.recipe_references.iter().enumerate() {
        let Ok(reference_ordinal) = u32::try_from(ordinal) else {
            continue;
        };
        let context = edge_recipe_reference_context(
            reference_ordinal,
            reference,
            EdgeBoundaryContext {
                state,
                topology: result_topology,
                boundary_edges: &operand.result_boundary_edge_slots,
            },
            EdgeBoundaryContext {
                state: previous,
                topology,
                boundary_edges: &operand.preceding_boundary_edge_slots,
            },
            &changed_edges,
            topology_queries,
            face_boundaries,
        );

        reference_contexts.push(context);
    }
    operand.recipe_reference_contexts = reference_contexts;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.resolved_edge_slot = surface_patch_edge_operand_slot(
            operand.surface_patch_recipe_structure.as_ref(),
            &operand.recipe_references,
            topology,
            topology_queries,
        );
        return Ok(());
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Sweep)
    {
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let mut candidate_edges = BTreeSet::new();
        for edge in reference_edge_sets.iter().flatten() {
            candidate_edges.insert(*edge);
        }
        let contexts = candidate_edges
            .into_iter()
            .map(|edge| selection::historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        operand.recipe_selectors =
            selection::recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
        operand.resolved_edge_slot =
            crate::design::edge_resolve::unique_incidence_edge_shared_by_reference_faces(
                &operand.recipe_selectors,
                reference_edge_sets.iter().map(Vec::as_slice),
            );
        return Ok(());
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let mut candidate_edges = BTreeSet::new();
        for edge in reference_edge_sets.iter().flatten() {
            candidate_edges.insert(*edge);
        }
        let contexts = candidate_edges
            .into_iter()
            .map(|edge| selection::historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        operand.recipe_selectors =
            selection::recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
        operand.resolved_edge_slot =
            crate::design::edge_resolve::resolved_edge_candidate_intersection(
                &operand.recipe_selectors,
                reference_edge_sets.iter().map(Vec::as_slice),
            )
            .or_else(|| {
                side_one_recipe_edge(
                    operand.recipe_structure.as_ref(),
                    &operand.recipe_reference_contexts,
                    &operand.recipe_selectors,
                    &operand.preceding_boundary_edge_slots,
                )
            });
        let resolved_axis = if let Some(edge) = operand.resolved_edge_slot {
            historical_edge_axis(edge, topology)
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
        return Ok(());
    }
    let edge_contexts = topology_queries
        .edge_contexts
        .get(topology, selection::EdgeContextIndex::new);
    let changed_edge_contexts = topology
        .edges
        .iter()
        .copied()
        .filter(|edge| changed_edges.contains(edge))
        .map(|edge| edge_contexts.context(edge))
        .collect::<Vec<_>>();
    operand.recipe_selectors = selection::recipe_selector_candidates(
        operand.recipe_structure.as_ref(),
        &changed_edge_contexts,
    );
    operand.resolved_edge_slot = side_one_recipe_edge(
        operand.recipe_structure.as_ref(),
        &operand.recipe_reference_contexts,
        &operand.recipe_selectors,
        &operand.preceding_boundary_edge_slots,
    );
    if operand.resolved_edge_slot.is_none()
        && operand.preceding_candidate_faces.is_empty()
        && matches!(
            crate::design::design_feature_family(&scope.kind()),
            Some(
                crate::design::DesignFeatureFamily::Fillet
                    | crate::design::DesignFeatureFamily::Chamfer
            )
        )
    {
        let preceding_edges = selection::FamilySlots::new(&topology.edges);
        operand.resolved_edge_slot = operand.recipe_structure.as_ref().and_then(|structure| {
            reference_edge::primary_reference_common_edge(structure, |ordinal| {
                let context = operand.recipe_reference_contexts.get(ordinal)?;
                if usize::try_from(context.reference_ordinal).ok()? != ordinal {
                    return None;
                }
                let [face] = context.preceding_faces.as_slice() else {
                    return None;
                };
                let face_slot = stable_ref(face.as_str())?;
                let [boundary_id] = context.preceding_face_boundary_ids.as_slice() else {
                    return None;
                };
                let boundary = face_boundaries.records.get(boundary_id)?;
                if boundary.face_slot != face_slot || boundary.loops.is_empty() {
                    return None;
                }
                let mut edges = Vec::new();
                for face_loop in &boundary.loops {
                    let before = edges.len();
                    for coedge in face_loop.boundary.coedges() {
                        if !preceding_edges.contains(coedge.edge_slot) {
                            return None;
                        }
                        edges.push(coedge.edge_slot);
                    }
                    if edges.len() == before {
                        return None;
                    }
                }
                Some((face_slot, edges))
            })
        });
    }
    Ok(())
}

/// A resolved axis with finite origin and unit direction.
fn design_axis(
    origin: cadmpeg_ir::math::Point3,
    direction: cadmpeg_ir::math::Vector3,
) -> Option<crate::records::feature::patterns::DesignAxis> {
    crate::records::feature::patterns::DesignAxis::from_parts(
        cadmpeg_ir::features::FinitePoint3::new(origin)?,
        cadmpeg_ir::features::FiniteVector3::new(direction)?,
    )
}

fn historical_edge_axis(
    edge: i64,
    topology: &AsmHistoricalTopology,
) -> Option<(cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3)> {
    if let Some(axis) = topology
        .edge_curves
        .iter()
        .find(|binding| binding.entity == edge)
        .and_then(|binding| binding.carrier)
        .and_then(|curve| topology.curve_axes.iter().find(|axis| axis.curve == curve))
    {
        return Some((axis.origin, axis.direction));
    }
    let mut support_surfaces = HashSet::new();
    for carrier in selection::historical_edge_context(edge, topology)
        .incident_loops
        .into_iter()
        .filter_map(|context| {
            let mut bindings = topology
                .face_surfaces
                .iter()
                .filter(|binding| binding.entity == context.face_slot);
            let carrier = bindings.next()?.carrier;
            bindings.next().is_none().then_some(carrier)
        })
    {
        support_surfaces.insert(carrier);
    }
    let mut axes = topology
        .surface_axes
        .iter()
        .filter(|axis| support_surfaces.contains(&axis.surface))
        .map(|axis| (axis.origin, axis.direction));
    let first = axes.next()?;
    axes.all(|axis| same_axis_line(first, axis))
        .then_some(first)
}

fn bind_active_edge_operand_for_scope<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    scope: &crate::records::feature::scope::DesignParameterScope,
    terminal_topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    bind_active_edge_operand_candidates(operand, terminal_topologies, topology_queries)?;
    if scope.kind() == crate::records::feature::scope::DesignFeatureKind::SurfacePatch
        && operand.surface_patch_recipe_structure.is_some()
    {
        operand.recipe_state_id = None;
        operand.resolved_edge_slot = None;
        let mut matched = None;
        let mut ambiguous = false;
        for (state_id, topology) in terminal_topologies {
            if let Some(edge) = surface_patch_edge_operand_slot(
                operand.surface_patch_recipe_structure.as_ref(),
                &operand.recipe_references,
                topology,
                topology_queries,
            ) {
                if matched.is_some() {
                    ambiguous = true;
                    break;
                }
                matched = Some((*state_id, edge));
            }
        }
        if !ambiguous {
            if let Some((state_id, edge)) = matched {
                operand.recipe_state_id = Some(state_id);
                operand.resolved_edge_slot = Some(edge);
            }
        }
    }
    if crate::design::design_feature_family(&scope.kind())
        == Some(crate::design::DesignFeatureFamily::Revolve)
    {
        let topology = operand.recipe_state_id.and_then(|state_id| {
            terminal_topologies
                .iter()
                .find(|(candidate, _)| *candidate == state_id)
                .map(|(_, topology)| *topology)
        });
        let resolved_axis = if let Some((edge, topology)) = operand.resolved_edge_slot.zip(topology)
        {
            historical_edge_axis(edge, topology)
                .and_then(|(origin, direction)| design_axis(origin, direction))
        } else {
            None
        };
        if let Some(axis) = resolved_axis {
            operand.resolved_axis = Some(axis);
        }
    }
    Ok(())
}

fn surface_patch_edge_operand_slot<'a>(
    structure: Option<&crate::records::topology::edge_recipe::DesignSurfacePatchRecipeStructure>,
    recipe_references: &[crate::records::dimensions::DesignRecipeReference],
    topology: &'a AsmHistoricalTopology,
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Option<i64> {
    let structure = structure?;
    let [first, second] = &structure.clauses;
    let common_edge_reference = common_surface_patch_reference(
        first.edge_reference_ordinals,
        second.edge_reference_ordinals,
    )?;
    let common_face_reference = common_surface_patch_reference(
        first.face_reference_ordinals,
        second.face_reference_ordinals,
    )?;
    let edge_reference = usize::try_from(common_edge_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))?;
    let face_reference = usize::try_from(common_face_reference)
        .ok()
        .and_then(|ordinal| recipe_references.get(ordinal))?;
    if edge_reference.candidate_edges.is_empty() {
        return None;
    }
    if face_reference.candidate_faces.is_empty()
        || face_reference
            .candidate_faces
            .iter()
            .collect::<HashSet<_>>()
            .len()
            != face_reference.candidate_faces.len()
    {
        return None;
    }
    let faces = selection::faces_in_topology(&face_reference.candidate_faces, topology);
    let face_boundary_edges = face_boundary_edges(&faces, topology, topology_queries);
    let mut candidates = edge_reference
        .candidate_edges
        .iter()
        .filter_map(|edge| stable_ref(edge.as_str()))
        .filter(|edge| face_boundary_edges.contains(edge))
        .collect::<Vec<_>>();
    candidates.sort_unstable_by(Ord::cmp);
    candidates.dedup();
    match candidates.as_slice() {
        [edge] => Some(*edge),
        _ => None,
    }
}

fn common_surface_patch_reference(left: [u32; 2], right: [u32; 2]) -> Option<u32> {
    let mut common = left.into_iter().collect::<HashSet<_>>();
    common.retain(|reference| right.contains(reference));
    (common.len() == 1)
        .then(|| common.into_iter().next())
        .flatten()
}

fn bind_active_edge_operand_candidates<'a>(
    operand: &mut crate::records::topology::edge_identity::DesignEdgeOperand,
    topologies: &[(i64, &'a AsmHistoricalTopology)],
    topology_queries: &mut TopologyQueryCache<'a>,
) -> Result<(), cadmpeg_core::CodecError> {
    let mut unique = None;
    for (state_id, topology) in topologies {
        let all_reference_faces =
            terminal_edge_recipe_reference_faces(&operand.recipe_references, None)?;
        let reference_faces = terminal_edge_recipe_reference_faces(
            &operand.recipe_references,
            operand.local_topology_references.as_deref(),
        )?;
        let terminal_faces = terminal_edge_recipe_faces(&operand.candidate_faces, &reference_faces);
        let candidate_faces = selection::faces_in_topology(&terminal_faces, topology);
        if topologies.len() != 1 && candidate_faces.is_empty() {
            continue;
        }
        let boundary_edges = face_boundary_edges(&candidate_faces, topology, topology_queries);
        let contexts = boundary_edges
            .iter()
            .copied()
            .map(|edge| selection::historical_edge_context(edge, topology))
            .collect::<Vec<_>>();
        let selectors =
            selection::recipe_selector_candidates(operand.recipe_structure.as_ref(), &contexts);
        let reference_edge_sets =
            collect_reference_edge_sets(&reference_faces, topology, topology_queries);
        let all_reference_edge_sets =
            collect_reference_edge_sets(&all_reference_faces, topology, topology_queries);
        let edge = crate::design::edge_resolve::resolved_edge_candidate_intersection(
            &selectors,
            reference_edge_sets.iter().map(Vec::as_slice),
        );
        if unique.is_some() {
            return Ok(());
        }
        unique = Some((
            *state_id,
            edge,
            candidate_faces,
            boundary_edges,
            contexts,
            all_reference_edge_sets,
            selectors,
        ));
    }
    let Some((
        state_id,
        edge,
        candidate_faces,
        boundary_edges,
        contexts,
        all_reference_edge_sets,
        selectors,
    )) = unique
    else {
        return Ok(());
    };
    operand.terminal_candidate_faces = candidate_faces;
    operand.terminal_boundary_edge_slots = boundary_edges;
    operand.terminal_boundary_edge_contexts = contexts;
    operand.terminal_reference_edge_slots = all_reference_edge_sets;
    operand.recipe_selectors = selectors;
    operand.recipe_state_id = Some(state_id);
    operand.resolved_edge_slot = edge;
    Ok(())
}

fn terminal_edge_recipe_faces(
    primary: &[cadmpeg_ir::ids::FaceId],
    reference_faces: &[Vec<cadmpeg_ir::ids::FaceId>],
) -> Vec<cadmpeg_ir::ids::FaceId> {
    let mut faces = (primary.iter().chain(reference_faces.iter().flatten()))
        .cloned()
        .collect::<Vec<_>>();
    faces.sort_by(|left, right| left.as_str().cmp(right.as_str()));
    faces.dedup();
    faces
}

fn terminal_edge_recipe_reference_faces(
    references: &[crate::records::dimensions::DesignRecipeReference],
    local_topology_references: Option<&[std::num::NonZeroU32]>,
) -> Result<Vec<Vec<cadmpeg_ir::ids::FaceId>>, cadmpeg_core::CodecError> {
    let mut selected = Vec::new();
    let mut append = |reference: &crate::records::dimensions::DesignRecipeReference| {
        let faces = if reference.candidate_faces.is_empty() {
            &reference.alternate_selector_faces
        } else {
            &reference.candidate_faces
        };
        let faces = faces.clone();

        selected.push(faces);
        Ok::<(), cadmpeg_core::CodecError>(())
    };
    if let Some(ordinals) = local_topology_references {
        for ordinal in ordinals {
            if let Some(reference) = usize::try_from(ordinal.get())
                .ok()
                .and_then(|ordinal| ordinal.checked_sub(1))
                .and_then(|ordinal| references.get(ordinal))
            {
                append(reference)?;
            }
        }
    } else {
        for reference in references {
            append(reference)?;
        }
    }
    Ok(selected)
}

fn treatment_radius_candidates<'a>(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate> {
    treatment_edge_candidates(
        result_candidate_faces,
        inserted_faces,
        result,
        preceding,
        deleted_edges,
        treatment_cache,
    )
    .0
}

fn treatment_edge_candidates<'a>(
    result_candidate_faces: Option<&[cadmpeg_ir::ids::FaceId]>,
    inserted_faces: &[i64],
    result: &'a AsmHistoricalTopology,
    preceding: &'a AsmHistoricalTopology,
    deleted_edges: &[i64],
    treatment_cache: &mut TreatmentCache<'a>,
) -> (
    Vec<crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate>,
    Vec<i64>,
) {
    let index = treatment_cache.get(result, preceding);
    let result_boundaries = &index.result_boundaries;
    let preceding_boundaries = &index.preceding_boundaries;
    let mut deleted_edge_set = HashSet::new();
    for edge in deleted_edges {
        deleted_edge_set.insert(*edge);
    }
    let mut candidate_edges = HashSet::new();
    for edge in result_candidate_faces
        .into_iter()
        .flatten()
        .filter_map(|face| stable_ref(face.as_str()))
        .filter_map(|face| result_boundaries.get(&face))
        .flatten()
        .copied()
    {
        candidate_edges.insert(edge);
    }
    let mut radii_out = Vec::new();
    let mut transitions_out = Vec::new();
    for inserted in inserted_faces {
        let Some((carrier, supports)) = index.supports.get(inserted) else {
            continue;
        };
        let Some(inserted_boundary) = result_boundaries.get(inserted) else {
            continue;
        };

        let radius = index
            .radii
            .get(carrier)
            .copied()
            .flatten()
            .and_then(cadmpeg_ir::scalar::PositiveReal::new)
            .filter(|_| {
                candidate_edges.is_empty() || !inserted_boundary.is_disjoint(&candidate_edges)
            });
        for (ordinal, left) in supports.iter().enumerate() {
            let Some(left_edges) = preceding_boundaries.get(left) else {
                continue;
            };
            for right in supports.iter().skip(ordinal + 1) {
                let Some(right_edges) = preceding_boundaries.get(right) else {
                    continue;
                };

                for edge in left_edges.intersection(right_edges).filter(|edge| {
                    deleted_edge_set.contains(edge)
                        && index.spines.get(carrier).is_none_or(|spine| {
                            spine.admits(
                                index.preceding_edges.get(edge).copied().unwrap_or_default(),
                            )
                        })
                }) {
                    transitions_out.push(*edge);
                    if let Some(radius) = radius {
                        radii_out.push(
                            crate::records::topology::edge_identity::DesignEdgeTreatmentRadiusCandidate {
                                edge_slot: *edge,
                                radius,
                            },
                        );
                    }
                }
            }
        }
    }
    radii_out.sort_by(|left, right| {
        left.radius
            .get()
            .total_cmp(&right.radius.get())
            .then(left.edge_slot.cmp(&right.edge_slot))
    });
    radii_out
        .dedup_by(|left, right| left.radius == right.radius && left.edge_slot == right.edge_slot);
    transitions_out.sort_unstable_by(Ord::cmp);
    transitions_out.dedup();
    (radii_out, transitions_out)
}

fn treatment_face_supports(
    inserted_faces: &[i64],
    result: &AsmHistoricalTopology,
    preceding: &AsmHistoricalTopology,
    result_boundaries: &HashMap<i64, HashSet<i64>>,
) -> Vec<(i64, i64, Vec<i64>)> {
    let mut preceding_faces = HashSet::new();
    for face in &preceding.faces {
        preceding_faces.insert(*face);
    }
    let mut preceding_surfaces = HashSet::new();
    for surface in &preceding.surfaces {
        preceding_surfaces.insert(*surface);
    }
    let result_carriers = unique_face_carriers(result);
    let preceding_carrier_faces = unique_carrier_faces(preceding, &preceding_faces);
    let mut adjacent_faces = HashMap::<i64, Vec<i64>>::new();
    for (face, edges) in result_boundaries {
        for edge in edges {
            adjacent_faces.entry(*edge).or_default().push(*face);
        }
    }
    let mut selected = Vec::new();
    for inserted in inserted_faces.iter().copied() {
        let Some(carrier) = result_carriers.get(&inserted).copied().flatten() else {
            continue;
        };
        if preceding_surfaces.contains(&carrier) {
            continue;
        }
        let Some(inserted_boundary) = result_boundaries.get(&inserted) else {
            continue;
        };
        let mut supports = inserted_boundary
            .iter()
            .filter_map(|edge| adjacent_faces.get(edge))
            .flatten()
            .copied()
            .filter(|face| *face != inserted)
            .filter_map(|face| result_carriers.get(&face).copied().flatten())
            .filter_map(|carrier| preceding_carrier_faces.get(&carrier).copied().flatten())
            .collect::<Vec<_>>();
        supports.sort_unstable_by(Ord::cmp);
        supports.dedup();

        selected.push((inserted, carrier, supports));
    }
    selected
}

fn face_boundary_edge_index(topology: &AsmHistoricalTopology) -> HashMap<i64, HashSet<i64>> {
    fn unique_relations(relations: &[AsmHistoricalRelation]) -> HashMap<i64, Option<&[i64]>> {
        let mut out = HashMap::<i64, Option<&[i64]>>::new();
        for relation in relations {
            out.entry(relation.owner_ref)
                .and_modify(|members| *members = None)
                .or_insert(Some(&relation.member_refs));
        }
        out
    }
    let face_loops = unique_relations(&topology.face_loops);
    let loop_coedges = unique_relations(&topology.loop_coedges);
    let mut coedge_edges = HashMap::<i64, Option<i64>>::new();
    for coedge in &topology.coedge_topology {
        coedge_edges
            .entry(coedge.coedge)
            .and_modify(|edge| *edge = None)
            .or_insert(Some(coedge.edge));
    }
    let mut boundaries = HashMap::new();
    'faces: for face in &topology.faces {
        let Some(loops) = face_loops.get(face).copied().flatten() else {
            continue;
        };
        let mut edges = HashSet::new();
        for loop_slot in loops {
            let Some(coedges) = loop_coedges.get(loop_slot).copied().flatten() else {
                continue 'faces;
            };
            for coedge in coedges {
                let Some(edge) = coedge_edges.get(coedge).copied().flatten() else {
                    continue 'faces;
                };
                edges.insert(edge);
            }
        }
        boundaries.insert(*face, edges);
    }
    boundaries
}

fn unique_face_carriers(topology: &AsmHistoricalTopology) -> HashMap<i64, Option<i64>> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        out.entry(binding.entity)
            .and_modify(|carrier| *carrier = None)
            .or_insert(Some(binding.carrier));
    }
    out
}

fn unique_carrier_faces(
    topology: &AsmHistoricalTopology,
    included_faces: &HashSet<i64>,
) -> HashMap<i64, Option<i64>> {
    let mut out = HashMap::new();
    for binding in &topology.face_surfaces {
        if !included_faces.contains(&binding.entity) {
            continue;
        }
        out.entry(binding.carrier)
            .and_modify(|face| *face = None)
            .or_insert(Some(binding.entity));
    }
    out
}

pub(crate) fn stable_ref(id: &str) -> Option<i64> {
    id.rsplit_once('#')?
        .1
        .split(':')
        .next()?
        .parse::<i64>()
        .ok()
}

pub(crate) fn same_axis_line(
    left: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
    right: (cadmpeg_ir::math::Point3, cadmpeg_ir::math::Vector3),
) -> bool {
    let direction_dot = left.1.dot(right.1);
    if (direction_dot.abs() - 1.0).abs() > EPS_HISTORY_SAME_AXIS_LINE_E9 {
        return false;
    }
    let distance = right.0.vector_from(left.0).cross(left.1).norm();
    distance.is_finite() && distance <= EPS_HISTORY_SAME_AXIS_LINE_E8
}

pub(crate) fn changed_topology_family_refs(
    delta: &AsmHistoricalTopologyDelta,
    deleted: bool,
) -> BTreeSet<i64> {
    let families = [
        &delta.bodies,
        &delta.regions,
        &delta.shells,
        &delta.faces,
        &delta.loops,
        &delta.coedges,
        &delta.edges,
        &delta.vertices,
        &delta.points,
        &delta.surfaces,
        &delta.curves,
        &delta.pcurves,
    ];
    let mut changed = BTreeSet::new();
    for family in families {
        let members = if deleted {
            family.deleted.iter().chain([].iter())
        } else {
            family.inserted.iter().chain(family.updated.iter())
        };
        for &member in members {
            changed.insert(member);
        }
    }
    changed
}

#[cfg(test)]
fn bodies_intersecting(
    topology: &AsmHistoricalTopology,
    changed: &BTreeSet<i64>,
) -> Option<BTreeSet<i64>> {
    bodies_touching(topology, &[changed]).and_then(|touched| touched.into_iter().next())
}

/// The bodies whose topological closure contains an entity of each query,
/// from one walk of the closures.
fn bodies_touching(
    topology: &AsmHistoricalTopology,
    queries: &[&BTreeSet<i64>],
) -> Option<Vec<BTreeSet<i64>>> {
    let mut queried = queries
        .iter()
        .enumerate()
        .flat_map(|(query, entities)| entities.iter().map(move |&entity| (entity, query)))
        .collect::<Vec<_>>();
    queried.sort_unstable();
    let mut touched = vec![BTreeSet::new(); queries.len()];
    walk_body_closures(topology, |body, entity| {
        let start = queried.partition_point(|&(queried, _)| queried < entity);
        for &(_, query) in queried[start..]
            .iter()
            .take_while(|&&(queried, _)| queried == entity)
        {
            touched[query].insert(body);
        }
    })?;
    Some(touched)
}

/// Visit every entity of each body's topological closure, one body after
/// another. `None` when the closure reads a relation or carrier the topology
/// lacks.
fn walk_body_closures(
    topology: &AsmHistoricalTopology,
    mut reach: impl FnMut(i64, i64),
) -> Option<()> {
    let body_regions = KeyedRows::new(&topology.body_regions);
    let region_shells = KeyedRows::new(&topology.region_shells);
    let shell_faces = KeyedRows::new(&topology.shell_faces);
    let shell_wire_edges = KeyedRows::new(&topology.shell_wire_edges);
    let shell_free_vertices = KeyedRows::new(&topology.shell_free_vertices);
    let face_loops = KeyedRows::new(&topology.face_loops);
    let loop_coedges = KeyedRows::new(&topology.loop_coedges);
    let coedges = KeyedRows::new(&topology.coedge_topology);
    let edges = KeyedRows::new(&topology.edge_vertices);
    let face_surfaces = KeyedRows::new(&topology.face_surfaces);
    let edge_curves = KeyedRows::new(&topology.edge_curves);
    let coedge_pcurves = KeyedRows::new(&topology.coedge_pcurves);
    let vertex_points = KeyedRows::new(&topology.vertex_points);
    let mut shell_edges = ShellRanks::new(&edges);
    let mut shell_vertices = ShellRanks::new(&vertex_points);
    for &body in &topology.bodies {
        reach(body, body);
        for &region in body_regions.members(body)? {
            reach(body, region);
            for &shell in region_shells.members(region)? {
                reach(body, shell);
                shell_edges.start_shell();
                shell_vertices.start_shell();
                for &edge in shell_wire_edges.members(shell)? {
                    shell_edges.visit(edges.rank(edge)?);
                }
                for &vertex in shell_free_vertices.members(shell)? {
                    shell_vertices.visit(vertex_points.rank(vertex)?);
                }
                for &face in shell_faces.members(shell)? {
                    reach(body, face);
                    reach(body, face_surfaces.get(face)?.carrier);
                    for &loop_ in face_loops.members(face)? {
                        reach(body, loop_);
                        for &coedge in loop_coedges.members(loop_)? {
                            reach(body, coedge);
                            shell_edges.visit(edges.rank(coedges.get(coedge)?.edge)?);
                            if let Some(pcurve) = coedge_pcurves
                                .get(coedge)
                                .and_then(|binding| binding.carrier)
                            {
                                reach(body, pcurve);
                            }
                        }
                    }
                }
                for &rank in &shell_edges.ranks {
                    let edge = edges.at(rank);
                    reach(body, edge.edge);
                    for vertex in [edge.start_vertex, edge.end_vertex] {
                        shell_vertices.visit(vertex_points.rank(vertex)?);
                    }
                    if let Some(curve) = edge_curves
                        .get(edge.edge)
                        .and_then(|binding| binding.carrier)
                    {
                        reach(body, curve);
                    }
                }
                for &rank in &shell_vertices.ranks {
                    let point = vertex_points.at(rank);
                    reach(body, point.entity);
                    reach(body, point.carrier);
                }
            }
        }
    }
    Some(())
}

/// One topology family's rows found by key in logarithmic time. Among rows
/// that share a key the last one wins.
struct KeyedRows<'a, T> {
    rows: &'a [T],
    /// Row positions in key order, when the rows are not already in key order.
    order: Option<Vec<usize>>,
}

impl<'a, T: AsmHistoricalRow> KeyedRows<'a, T> {
    fn new(rows: &'a [T]) -> Self {
        let order = (!rows.is_sorted_by_key(T::key)).then(|| {
            let mut order = (0..rows.len()).collect::<Vec<_>>();
            order.sort_by_key(|&row| rows[row].key());
            order
        });
        Self { rows, order }
    }

    fn len(&self) -> usize {
        self.rows.len()
    }

    /// The row at `rank` in key order.
    fn at(&self, rank: usize) -> &'a T {
        &self.rows[self.order.as_ref().map_or(rank, |order| order[rank])]
    }

    /// The key-order rank of the row with `key`; distinct keys have distinct
    /// ranks below `len`.
    fn rank(&self, key: i64) -> Option<usize> {
        let after = match &self.order {
            Some(order) => order.partition_point(|&row| self.rows[row].key() <= key),
            None => self.rows.partition_point(|row| row.key() <= key),
        };
        let rank = after.checked_sub(1)?;
        (self.at(rank).key() == key).then_some(rank)
    }

    fn get(&self, key: i64) -> Option<&'a T> {
        self.rank(key).map(|rank| self.at(rank))
    }
}

impl<'a> KeyedRows<'a, AsmHistoricalRelation> {
    fn members(&self, owner: i64) -> Option<&'a [i64]> {
        self.get(owner)
            .map(|relation| relation.member_refs.as_slice())
    }
}

/// The distinct row ranks one shell reaches, in first-visit order.
struct ShellRanks {
    /// The shell visit that last reached each rank.
    visits: Vec<usize>,
    shell: usize,
    ranks: Vec<usize>,
}

impl ShellRanks {
    fn new<T: AsmHistoricalRow>(rows: &KeyedRows<'_, T>) -> Self {
        Self {
            visits: vec![0; rows.len()],
            shell: 0,
            ranks: Vec::new(),
        }
    }

    fn start_shell(&mut self) {
        self.shell += 1;
        self.ranks.clear();
    }

    fn visit(&mut self, rank: usize) {
        if self.visits[rank] != self.shell {
            self.visits[rank] = self.shell;
            self.ranks.push(rank);
        }
    }
}

fn historical_topology(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    macro_rules! topology_some {
        ($value:expr) => {
            match $value {
                Some(value) => value,
                None => return Ok(None),
            }
        };
    }

    fn refs<'a>(ids: impl Iterator<Item = &'a str>) -> Option<Vec<i64>> {
        ids.map(stable_ref).collect::<Option<Vec<_>>>()
    }

    fn relations<'a, M>(
        items: impl Iterator<Item = (&'a str, M)>,
    ) -> Option<Vec<AsmHistoricalRelation>>
    where
        M: Iterator<Item = &'a str>,
    {
        let mut collected = Vec::new();
        for (owner, members) in items {
            let owner_ref = stable_ref(owner)?;
            let member_refs = refs(members)?;

            collected.push(AsmHistoricalRelation {
                owner_ref,
                member_refs,
            });
        }
        Some(collected)
    }

    let mut surface_radii = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let radius = match &surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    cylinder_surface.radius().get()
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Sphere(sphere_surface)) => {
                    sphere_surface.radius().get()
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    torus_surface.minor_radius().get()
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceRadius {
                surface: stable_ref(surface.id.as_str())?,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    let mut blend_radii = HashMap::new();
    for (owner, procedural) in &brep.procedural_surfaces {
        let cadmpeg_ir::geometry::ProceduralSurfaceDefinition::Blend(definition_payload) =
            procedural.definition()
        else {
            continue;
        };
        let radius = definition_payload.radius();

        let cadmpeg_ir::geometry::BlendRadiusLaw::Constant { signed_radius } = radius else {
            continue;
        };
        let Some(surface) = stable_ref(owner.as_str()) else {
            continue;
        };
        blend_radii.insert(surface, signed_radius.get().abs());
    }

    surface_radii.retain(|candidate| !blend_radii.contains_key(&candidate.surface));
    for (surface, radius) in blend_radii {
        surface_radii.push(crate::history_records::AsmHistoricalSurfaceRadius { surface, radius });
    }
    surface_radii.sort_by_key(|a| a.surface);
    let mut surface_cylinders = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = cylinder_surface.origin().get();
            let axis = *cylinder_surface.frame().axis().as_raw();
            let radius = cylinder_surface.radius().get();
            Some(crate::history_records::AsmHistoricalCylinder {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                axis,
                radius: radius.abs(),
            })
        })
        .collect::<Vec<_>>();
    surface_cylinders.sort_by_key(|a| a.surface);
    let mut surface_planes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            let Some(SolvedSurfaceGeometry::Plane(plane_surface)) = surface.geometry.solved()
            else {
                return None;
            };
            let origin = plane_surface.origin().get();
            let normal = *plane_surface.frame().axis().as_raw();
            Some(crate::history_records::AsmHistoricalPlane {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                normal,
            })
        })
        .collect::<Vec<_>>();
    surface_planes.sort_by_key(|a| a.surface);
    let mut surface_axes = brep
        .surfaces
        .iter()
        .filter_map(|surface| {
            use cadmpeg_ir::geometry::{SolvedSurfaceGeometry, SurfaceGeometry};
            let (origin, direction) = match surface.geometry {
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cylinder(cylinder_surface)) => {
                    let origin = cylinder_surface.origin().get();
                    let axis = *cylinder_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Cone(cone_surface)) => {
                    let origin = cone_surface.origin().get();
                    let axis = *cone_surface.frame().axis().as_raw();
                    (origin, axis)
                }
                SurfaceGeometry::Solved(SolvedSurfaceGeometry::Torus(torus_surface)) => {
                    let center = torus_surface.center().get();
                    let axis = *torus_surface.frame().axis().as_raw();
                    (center, axis)
                }
                _ => return None,
            };
            Some(crate::history_records::AsmHistoricalSurfaceAxis {
                surface: stable_ref(surface.id.as_str())?,
                origin,
                direction,
            })
        })
        .collect::<Vec<_>>();
    surface_axes.sort_by_key(|a| a.surface);

    let mut ring_neighbors = HashMap::new();
    for loop_ in &brep.loops {
        let ring = loop_.coedges();
        for (ordinal, coedge) in ring.iter().enumerate() {
            let key = (&loop_.id, coedge);

            ring_neighbors.entry(key).or_insert((
                &ring[(ordinal + 1) % ring.len()],
                &ring[(ordinal + ring.len() - 1) % ring.len()],
            ));
        }
    }

    let mut topology = AsmHistoricalTopology {
        bodies: topology_some!(refs(brep.bodies.iter().map(|entity| entity.id.as_str()))),
        regions: topology_some!(refs(brep.regions.iter().map(|entity| entity.id.as_str()))),
        shells: topology_some!(refs(brep.shells.iter().map(|entity| entity.id.as_str()))),
        faces: topology_some!(refs(brep.faces.iter().map(|entity| entity.id.as_str()))),
        loops: topology_some!(refs(brep.loops.iter().map(|entity| entity.id.as_str()))),
        coedges: topology_some!(refs(brep.coedges.iter().map(|entity| entity.id.as_str()))),
        edges: topology_some!(refs(brep.edges.iter().map(|entity| entity.id.as_str()))),
        vertices: topology_some!(refs(brep.vertices.iter().map(|entity| entity.id.as_str()))),
        points: topology_some!(refs(brep.points.iter().map(|entity| entity.id.as_str()))),
        surfaces: topology_some!(refs(brep.surfaces.iter().map(|entity| entity.id.as_str()))),
        surface_radii,
        surface_cylinders,
        surface_planes,
        surface_axes,
        curves: topology_some!(refs(brep.curves.iter().map(|entity| entity.id.as_str()))),
        curve_axes: brep
            .curves
            .iter()
            .filter_map(|curve| {
                use cadmpeg_ir::geometry::{CurveGeometry, SolvedCurveGeometry};
                let (origin, direction) = match curve.geometry {
                    CurveGeometry::Solved(SolvedCurveGeometry::Line(line_curve)) => {
                        let origin = line_curve.origin().get();
                        let direction = *line_curve.direction().as_raw();
                        (origin, direction)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Circle(circle_curve)) => {
                        let center = circle_curve.center().get();
                        let axis = *circle_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    CurveGeometry::Solved(SolvedCurveGeometry::Ellipse(ellipse_curve)) => {
                        let center = ellipse_curve.center().get();
                        let axis = *ellipse_curve.frame().axis().as_raw();
                        (center, axis)
                    }
                    _ => return None,
                };
                Some(crate::history_records::AsmHistoricalCurveAxis {
                    curve: stable_ref(curve.id.as_str())?,
                    origin,
                    direction,
                })
            })
            .collect::<Vec<_>>(),
        pcurves: topology_some!(refs(brep.pcurves.iter().map(|entity| entity.id.as_str()))),
        persistent_subentity_tags: Vec::new(),
        body_regions: topology_some!(relations(brep.bodies.iter().map(|body| {
            (
                body.id.as_str(),
                body.regions.iter().map(cadmpeg_ir::ids::RegionId::as_str),
            )
        }))),
        region_shells: topology_some!(relations(brep.regions.iter().map(|region| {
            (
                region.id.as_str(),
                region.shells.iter().map(cadmpeg_ir::ids::ShellId::as_str),
            )
        }))),
        shell_faces: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell.faces().iter().map(cadmpeg_ir::ids::FaceId::as_str),
            )
        }))),
        shell_wire_edges: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell
                    .wire_edges()
                    .iter()
                    .map(cadmpeg_ir::ids::EdgeId::as_str),
            )
        }))),
        shell_free_vertices: topology_some!(relations(brep.shells.iter().map(|shell| {
            (
                shell.id.as_str(),
                shell
                    .free_vertices()
                    .iter()
                    .map(cadmpeg_ir::ids::VertexId::as_str),
            )
        }))),
        face_loops: topology_some!(relations(brep.faces.iter().map(|face| {
            (
                face.id.as_str(),
                face.loops.iter().map(cadmpeg_ir::ids::LoopId::as_str),
            )
        }))),
        loop_coedges: topology_some!(relations(brep.loops.iter().map(|loop_| {
            (
                loop_.id.as_str(),
                loop_
                    .coedges()
                    .iter()
                    .map(cadmpeg_ir::ids::CoedgeId::as_str),
            )
        }))),
        coedge_topology: topology_some!(brep
            .coedges
            .iter()
            .map(|coedge| {
                let (next, previous) = ring_neighbors
                    .get(&(&coedge.owner_loop, &coedge.id))
                    .copied()?;
                Some(AsmHistoricalCoedge {
                    coedge: stable_ref(coedge.id.as_str())?,
                    owner_loop: stable_ref(coedge.owner_loop.as_str())?,
                    edge: stable_ref(coedge.edge.as_str())?,
                    next: stable_ref(next.as_str())?,
                    previous: stable_ref(previous.as_str())?,
                    radial_next: stable_ref(coedge.radial_next.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_vertices: topology_some!(brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalEdge {
                    edge: stable_ref(edge.id.as_str())?,
                    start_vertex: stable_ref(edge.start.as_str())?,
                    end_vertex: stable_ref(edge.end.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        face_surfaces: topology_some!(brep
            .faces
            .iter()
            .map(|face| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(face.id.as_str())?,
                    carrier: stable_ref(face.surface.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_curves: topology_some!(brep
            .edges
            .iter()
            .map(|edge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(edge.id.as_str())?,
                    carrier: match edge.curve() {
                        Some(curve) => Some(stable_ref(curve.as_str())?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()),
        coedge_pcurves: topology_some!(brep
            .coedges
            .iter()
            .map(|coedge| {
                Some(AsmHistoricalOptionalCarrierBinding {
                    entity: stable_ref(coedge.id.as_str())?,
                    carrier: match coedge.pcurves.first() {
                        Some(use_) => Some(stable_ref(use_.pcurve.as_str())?),
                        None => None,
                    },
                })
            })
            .collect::<Option<Vec<_>>>()),
        vertex_points: topology_some!(brep
            .vertices
            .iter()
            .map(|vertex| {
                Some(AsmHistoricalCarrierBinding {
                    entity: stable_ref(vertex.id.as_str())?,
                    carrier: stable_ref(vertex.point.as_str())?,
                })
            })
            .collect::<Option<Vec<_>>>()),
        point_positions: topology_some!(brep
            .points
            .iter()
            .map(|point| {
                Some(AsmHistoricalPoint {
                    point: stable_ref(point.id.as_str())?,
                    position: point.position().get(),
                })
            })
            .collect::<Option<Vec<_>>>()),
        edge_samples: historical_edge_samples(brep)?,
    };
    // Transitions diff each family in key order. The decoder appends the
    // containers it builds for saved free edges after the containers it walks,
    // and the curves it builds for tolerant coedges after the curves it walks.
    topology.bodies.sort_unstable();
    topology.regions.sort_unstable();
    topology.shells.sort_unstable();
    topology.curves.sort_unstable();
    topology.curve_axes.sort_by_key(|axis| axis.curve);
    for family in [
        &mut topology.body_regions,
        &mut topology.region_shells,
        &mut topology.shell_faces,
        &mut topology.shell_wire_edges,
        &mut topology.shell_free_vertices,
    ] {
        family.sort_by_key(|relation| relation.owner_ref);
    }
    Ok(Some(topology))
}

/// Sample each edge's solved carrier at the midpoint of its parameter interval.
///
/// An edge without a bounded solved carrier, or whose carrier has no finite
/// point there, has no sample. A resource refusal of the evaluator ends the
/// sampling.
fn historical_edge_samples(
    brep: &cadmpeg_asm::brep::AsmBrep,
) -> Result<Vec<crate::history_records::AsmHistoricalEdgeSample>, cadmpeg_core::decode::ResourceLimit>
{
    use cadmpeg_ir::eval::admission::EvaluationAdmission;
    use cadmpeg_ir::eval::finite_or_refusal;
    use cadmpeg_ir::geometry::CurveGeometry;

    let curves = brep
        .curves
        .iter()
        .map(|curve| (curve.id.as_str(), &curve.geometry))
        .collect::<HashMap<_, _>>();
    let mut samples = Vec::new();
    for edge in &brep.edges {
        let Some([start, end]) = edge.param_range().map(cadmpeg_ir::units::FiniteVector::get)
        else {
            continue;
        };
        let Some(CurveGeometry::Solved(geometry)) =
            edge.curve().and_then(|curve| curves.get(curve.as_str()))
        else {
            continue;
        };
        let Some(edge_ref) = stable_ref(edge.id.as_str()) else {
            continue;
        };
        let parameter = 0.5 * (start + end);
        let Some(interior) = finite_or_refusal(cadmpeg_ir::eval::decode::curve_point_solved(
            EvaluationAdmission::Standard,
            geometry,
            parameter,
        ))?
        else {
            continue;
        };
        let tangent = finite_or_refusal(cadmpeg_ir::eval::curve_tangent_solved(
            EvaluationAdmission::Standard,
            geometry,
            parameter,
        ))?
        .and_then(|tangent| cadmpeg_ir::units::UnitVector3::normalized_by_reciprocal(tangent.get()))
        .map(|tangent| *tangent.as_raw());
        samples.push(crate::history_records::AsmHistoricalEdgeSample {
            edge: edge_ref,
            interior: interior.get(),
            tangent,
        });
    }
    samples.sort_by_key(|sample| sample.edge);
    Ok(samples)
}

pub(crate) fn historical_topology_with_tags(
    brep: &crate::brep::Brep,
) -> Result<Option<AsmHistoricalTopology>, cadmpeg_core::CodecError> {
    let Some(mut topology) = historical_topology(&brep.asm)? else {
        return Ok(None);
    };
    for tag in &brep.persistent_subentity_tags {
        let (entity_kind, entity_ref) = match &tag.target {
            cadmpeg_ir::attributes::AttributeTarget::Face(face) => {
                (AsmHistoricalEntityKind::Face, stable_ref(face.as_str()))
            }
            cadmpeg_ir::attributes::AttributeTarget::Edge(edge) => {
                (AsmHistoricalEntityKind::Edge, stable_ref(edge.as_str()))
            }
            _ => continue,
        };
        let Some(entity_ref) = entity_ref else {
            continue;
        };
        let design_references = tag.design_references.clone();
        let token = tag.token.as_str().to_owned();

        topology.persistent_subentity_tags.push(
            crate::history_records::AsmHistoricalPersistentSubentityTag {
                entity_kind,
                entity_ref,
                selector: tag.selector,
                token,
                design_references,
                ordinal: tag.ordinal,
            },
        );
    }
    topology
        .persistent_subentity_tags
        .sort_by_key(|tag| tag.entity_ref);
    Ok(Some(topology))
}

#[cfg(test)]
pub(crate) fn materialize_record_table(
    versions: &BTreeMap<i64, i64>,
    archive: &HistoricalRecordArchive,
) -> Option<Vec<cadmpeg_asm::sab::Record>> {
    if versions.is_empty() {
        return None;
    }
    let mut records = Vec::new();
    for (&entity_ref, record_ref) in versions {
        let record = archive.get(record_ref)?;
        if i64::try_from(record.index).ok() != Some(entity_ref) {
            return None;
        }
        for token in record.tokens.iter() {
            let cadmpeg_asm::sab::Token::Ref(reference) = token else {
                continue;
            };
            if *reference >= 0 && !versions.contains_key(reference) {
                return None;
            }
        }
        records.push(clone_historical_record(record));
    }
    records.sort_unstable_by_key(|a| a.index);
    Some(records)
}

/// One framed item of a `BulletinBoard` chain, in stream order.
enum BulletinBoardItem {
    Board {
        offset: usize,
        owner_ref: i64,
        number: i64,
    },
    /// A change of the most recent board.
    Change {
        offset: usize,
        kind: AsmEntityChangeKind,
    },
}

/// Walk the `BulletinBoard` chain at `position` and return the offset after
/// it, or `None` when the chain is malformed.
fn walk_bulletin_boards(
    bytes: &[u8],
    mut position: usize,
    width: RefWidth,
    mut visit: impl FnMut(BulletinBoardItem),
) -> Option<usize> {
    if bytes.get(position) == Some(&0x11) {
        return Some(position);
    }
    loop {
        let board_offset = position;
        let present = take_int(bytes, &mut position, 0x04, width)?;
        if present == 0 {
            break;
        }
        let owner_ref = take_int(bytes, &mut position, 0x0c, width)?;
        let number = take_int(bytes, &mut position, 0x04, width)?;
        visit(BulletinBoardItem::Board {
            offset: board_offset,
            owner_ref,
            number,
        });
        loop {
            let change_offset = position;
            let present = take_int(bytes, &mut position, 0x04, width)?;
            if present == 0 {
                break;
            }
            let old = take_int(bytes, &mut position, 0x0c, width)?;
            let new = take_int(bytes, &mut position, 0x0c, width)?;
            let kind = match (old >= 0, new >= 0) {
                (false, true) => AsmEntityChangeKind::Insert { new },
                (true, false) => AsmEntityChangeKind::Delete { old },
                (true, true) => AsmEntityChangeKind::Update { old, new },
                (false, false) => return None,
            };
            visit(BulletinBoardItem::Change {
                offset: change_offset,
                kind,
            });
        }
    }
    Some(position)
}

fn decode_bulletin_boards(
    bytes: &[u8],
    position: usize,
    stream: &str,
    state_offset: usize,
    state_id: &str,
    width: RefWidth,
) -> Option<(Vec<AsmBulletinBoard>, usize)> {
    let mut boards = Vec::<AsmBulletinBoard>::new();
    let end = walk_bulletin_boards(bytes, position, width, |item| match item {
        BulletinBoardItem::Board {
            offset,
            owner_ref,
            number,
        } => {
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-bulletin-board",
                format_args!("{state_offset:010}:{:06}", boards.len()),
            );
            boards.push(AsmBulletinBoard {
                id,
                parent: state_id.to_owned(),
                byte_offset: u64_from_index(offset),
                owner_ref,
                number,
                changes: Vec::new(),
            });
        }
        BulletinBoardItem::Change { offset, kind } => {
            let (Some(ordinal), Some(board)) = (boards.len().checked_sub(1), boards.last_mut())
            else {
                return;
            };
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-entity-change",
                format_args!("{state_offset:010}:{ordinal:06}:{:06}", board.changes.len()),
            );
            let parent = board.id.clone();
            board.changes.push(AsmEntityChange {
                id,
                parent,
                byte_offset: u64_from_index(offset),
                kind,
            });
        }
    })?;
    Some((boards, end))
}

fn decode_history_records(
    ctx: &cadmpeg_core::decode::DecodeContext<'_>,
    bytes: &[u8],
    state_end: usize,
    next_delta: Option<usize>,
    stream: &str,
    state_id: &str,
    width: RefWidth,
) -> Result<Vec<AsmHistoryRecord>, cadmpeg_core::CodecError> {
    let mut start = state_end + usize::from(bytes.get(state_end) == Some(&0x11));
    if bytes.get(start) == Some(&0x04)
        && int_at(bytes, start + 1, width) == Some(0)
        && bytes.get(start + 1 + width.bytes()) == Some(&0x11)
    {
        start += 2 + width.bytes();
    }
    let limit = next_delta.map_or(bytes.len(), |offset| offset + 1);
    if start >= limit {
        return Ok(Vec::new());
    }
    match cadmpeg_asm::sab::frame_history(ctx, bytes, start, limit, width, None) {
        Ok(records) => {
            let mut decoded = Vec::new();
            for record in records {
                let mut entity_references = Vec::new();

                for token in record.tokens.iter() {
                    if let cadmpeg_asm::sab::Token::Ref(value) = token {
                        entity_references.push(*value);
                    }
                }
                let end = record.offset.checked_add(record.len).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed("F3D history record byte range overflows")
                })?;
                let source = bytes.get(record.offset..end).ok_or_else(|| {
                    cadmpeg_core::CodecError::malformed(
                        "F3D history record byte range exceeds stream",
                    )
                })?;
                let raw_bytes = source.to_vec();

                let id = crate::ids::native_scoped_id(
                    stream,
                    "asm-history-record",
                    format_args!("{:010}", record.offset),
                );
                let parent = state_id.to_owned();
                decoded.push(AsmHistoryRecord {
                    id,
                    parent,
                    revision_id: None,
                    byte_offset: u64_from_index(record.offset),
                    framing: crate::history_records::AsmHistoryRecordFraming::Framed {
                        index: u64_from_index(record.index),
                        name: record.name,
                        entity_references,
                    },
                    raw_bytes: raw_bytes.into(),
                });
            }
            Ok(decoded)
        }
        Err(cadmpeg_asm::stream_error::StreamFailure::Resource(error)) => Err(error.into()),
        Err(cadmpeg_asm::stream_error::StreamFailure::Operation(error)) => {
            Err(error.into_codec_error())
        }
        Err(error) => {
            let raw_bytes = bytes[start..limit].to_vec();
            let mut decoded = Vec::new();
            let id = crate::ids::native_scoped_id(
                stream,
                "asm-history-record",
                format_args!("{start:010}"),
            );
            let parent = state_id.to_owned();
            let error = format!("{error}");
            decoded.push(AsmHistoryRecord {
                id,
                parent,
                revision_id: None,
                byte_offset: u64_from_index(start),
                framing: crate::history_records::AsmHistoryRecordFraming::Opaque { error },
                raw_bytes: raw_bytes.into(),
            });
            Ok(decoded)
        }
    }
}

fn decode_preamble(bytes: &[u8], mut position: usize, width: RefWidth) -> Option<(i64, i64)> {
    let size = take_int(bytes, &mut position, 0x04, width)?;
    let duplicate = take_int(bytes, &mut position, 0x04, width)?;
    let zero = take_int(bytes, &mut position, 0x04, width)?;
    let entry_count = take_int(bytes, &mut position, 0x04, width)?;
    (size == duplicate && zero == 0).then_some((size, entry_count))
}

/// Read a tagged little-endian signed integer of the stream's ref width (4 or
/// 8 bytes) and advance past it.
fn take_int(bytes: &[u8], position: &mut usize, tag: u8, width: RefWidth) -> Option<i64> {
    if bytes.get(*position) != Some(&tag) {
        return None;
    }
    let value = int_at(bytes, *position + 1, width)?;
    *position += 1 + width.bytes();
    Some(value)
}

#[cfg(test)]
mod tests;
