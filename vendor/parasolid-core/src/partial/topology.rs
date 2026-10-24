// Modified by sldkit; see the crate-root PARTIAL_READERS.md.
// SPDX-License-Identifier: Apache-2.0
//! Typed topology record parsing ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
//!
//! Six fixed-width record families live at Parasolid stream scope and form the
//! B-rep chain
//!
//! ```text
//! bridge 00 0e .refs[4] -> compact surface carrier
//!              .refs[2] -> loop head 00 0f
//!                            .refs[1] -> coedge 00 11 ring (via .refs[3] = next)
//!                                          .refs[6] -> edge-use 00 10 .refs[3] -> compact curve
//!                                          .refs[4] -> vertex-use 00 12 .refs[4] -> world point 00 1d
//! ```
//!
//! Every record opens with `00 TT`, an optional `0xff`, then a big-endian `attr`
//! (u16) and, for most families, an `ordinal`/`seq` (u32). The magic
//! `c2 bc 92 8f 99 6e 00 00` anchors the bridge, edge-use, and vertex-use
//! parses. Records are keyed by `attr` within one stream (one site); attribute
//! ids collide across sites, so this codec resolves references only within the
//! single active partition stream it decodes.

use std::collections::{HashMap, HashSet};

use super::view::View;

use super::layout::world_point as world_pt;

/// The magic anchoring magic-bearing topology records ([spec §5](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/sldprt.md#4-typed-topology-records)).
pub const MAGIC: [u8; 8] = [0xc2, 0xbc, 0x92, 0x8f, 0x99, 0x6e, 0x00, 0x00];

/// A parsed topology record. Only the fields the chain walk needs are kept.
#[derive(Debug, Clone)]
pub struct Record {
    pub attr: u16,
    /// Big-endian `refs` array (length varies by family).
    pub refs: Vec<u16>,
    /// Orientation marker (`0x2b` forward / `0x2d` reversed), when the family
    /// carries one.
    pub marker: Option<u8>,
    /// World-point coordinates as stored, for `00 1d` only.
    /// The historical `xyz_m` name reflects the metre-based embedded profile;
    /// the reader itself does not convert length units.
    pub xyz_m: Option<[f64; 3]>,
    /// Byte offset of the world-point coordinates, for `00 1d` only.
    pub xyz_offset: Option<usize>,
    /// Owning entity reference carried by bridge records.
    pub owner: Option<u16>,
    /// Byte offset of the record's tag within the stream body.
    pub offset: usize,
    /// Exact decoded field intervals; opaque gaps are intentionally excluded.
    pub read_ranges: Vec<(usize, usize)>,
}

/// Read `count` big-endian u16 refs starting at `at`.
fn refs_be(buf: &[u8], at: usize, count: usize) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(count);
    for i in 0..count {
        out.push(View::u16_be_at(buf, at + 2 * i)?);
    }
    Some(out)
}

fn refs_tripled(buf: &[u8], at: usize, count: usize) -> Option<Vec<u16>> {
    let mut out = Vec::with_capacity(count);
    for index in 0..count {
        let p = at + index * 3;
        if buf.get(p + 2) != Some(&1) {
            return None;
        }
        out.push(View::u16_be_at(buf, p)?);
    }
    Some(out)
}

/// Advance past the tag and an optional `0xff` byte, returning the body start.
fn body_start(buf: &[u8], off: usize, tag_lo: u8) -> Option<usize> {
    if buf.get(off) != Some(&0x00) || buf.get(off + 1) != Some(&tag_lo) {
        return None;
    }
    let mut p = off + 2;
    if buf.get(p) == Some(&0xff) {
        p += 1;
    }
    Some(p)
}

fn attr_at(buf: &[u8], p: usize) -> Option<u16> {
    let a = View::u16_be_at(buf, p)?;
    if a == 0 { None } else { Some(a) }
}

/// Bridge `00 0e`: 37-byte body, magic at body+8, `refs[5]` at body+16,
/// marker at body+26. `refs[4]` = surface carrier, `refs[2]` = loop head.
/// The deltas form stores the owner as a `[hi][lo][01]` triple, so the magic
/// sits at body+9 and the five refs follow as triples with the marker after.
fn parse_bridge(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x0e)?;
    if buf.get(p + 8) == Some(&1) && buf.get(p + 9..p + 17) == Some(MAGIC.as_slice()) {
        let attr = attr_at(buf, p)?;
        let owner = View::u16_be_at(buf, p + 6)?;
        let refs = refs_tripled(buf, p + 17, 5)?;
        let marker = *buf.get(p + 32)?;
        if marker != 0x2b && marker != 0x2d {
            return None;
        }
        return Some(Record {
            attr,
            refs,
            marker: Some(marker),
            xyz_m: None,
            xyz_offset: None,
            owner: (owner > 1).then_some(owner),
            offset: off,
            read_ranges: vec![(off, p + 2), (p + 6, p + 33)],
        });
    }
    if p + 37 > buf.len() || buf.get(p + 8..p + 16)? != MAGIC {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let owner = View::u16_be_at(buf, p + 6)?;
    let tripled = (0..5).all(|index| buf.get(p + 18 + index * 3) == Some(&1));
    let (refs, marker) = if tripled { (refs_tripled(buf, p + 16, 5)?, *buf.get(p + 31)?) } else { (refs_be(buf, p + 16, 5)?, *buf.get(p + 26)?) };
    if marker != 0x2b && marker != 0x2d {
        return None;
    }
    Some(Record {
        attr,
        refs,
        marker: Some(marker),
        xyz_m: None,
        xyz_offset: None,
        owner: (owner > 1).then_some(owner),
        offset: off,
        read_ranges: vec![(off, p + 2), (p + 6, p + if tripled { 32 } else { 27 })],
    })
}

/// Loop head `00 0f`: minimal 14-byte body, no magic, `refs[4]` at body+6.
/// `refs[1]` = first coedge, `refs[2]` = owning bridge, `refs[3]` = next sibling.
fn parse_loop(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x0f)?;
    if p + 14 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (refs, end) = if let Some(refs) = refs_tripled(buf, p + 6, 4) { (refs, p + 18) } else { (refs_be(buf, p + 6, 4)?, p + 14) };
    Some(Record { attr, refs, marker: None, xyz_m: None, xyz_offset: None, owner: None, offset: off, read_ranges: vec![(off, p + 2), (p + 6, end)] })
}

fn record(attr: u16, refs: Vec<u16>, marker: Option<u8>, offset: usize) -> Record {
    Record { attr, refs, marker, xyz_m: None, xyz_offset: None, owner: None, offset, read_ranges: Vec::new() }
}

/// Return all syntactically valid edge-use readings at one offset.
///
/// A prefixed edge-use does not carry the complete six-cell array in the
/// compact form. The third post-magic cell is the support-curve carrier, so
/// preserve that field and leave the other cells as sentinels. The missing
/// canonical-coedge slot is resolved from the coedge table by the graph walk.
fn parse_edge_use_candidates(buf: &[u8], off: usize) -> Vec<Record> {
    let Some(p) = body_start(buf, off, 0x10) else {
        return Vec::new();
    };
    if p + 28 > buf.len() {
        return Vec::new();
    }
    let Some(attr) = attr_at(buf, p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if buf.get(p + 8..p + 16) == Some(MAGIC.as_slice())
        && let Some(refs) = refs_be(buf, p + 16, 6)
    {
        let mut parsed = record(attr, refs, None, off);
        parsed.read_ranges = vec![(off, p + 2), (p + 8, p + 28)];
        out.push(parsed);
    }

    let magic_end = (p + 16).min(buf.len().saturating_sub(MAGIC.len()));
    for magic in p + 9..=magic_end {
        if buf.get(magic..magic + MAGIC.len()) != Some(MAGIC.as_slice()) {
            continue;
        }
        let q = magic + MAGIC.len();
        for prefix_first in [true, false] {
            let mut decoded = Vec::new();
            let mut at = q;
            while decoded.len() < 8 {
                let (matches, reference_at) = if prefix_first {
                    // `[01][hi][lo]` triples.
                    (buf.get(at) == Some(&1), at + 1)
                } else {
                    // `[hi][lo][01]` triples.
                    (buf.get(at + 2) == Some(&1), at)
                };
                if !matches {
                    break;
                }
                let Some(reference) = View::u16_be_at(buf, reference_at) else {
                    break;
                };
                decoded.push(reference);
                at += 3;
            }
            if decoded.len() >= 3 {
                let mut refs = vec![0; 6];
                refs[3] = decoded[2];
                let mut parsed = record(attr, refs, None, off);
                parsed.read_ranges = vec![(off, p + 2), (magic, at)];
                out.push(parsed);
            }
        }
    }
    deduplicate_records(out)
}

/// Edge-use `00 10`: 28-byte body, magic at body+8, `refs[6]` at body+16.
/// `refs[0]` = canonical forward coedge when the bare record stores one;
/// `refs[3]` = support curve carrier. Prefixed records leave `refs[0]` as a
/// sentinel because their compact payload has no canonical-coedge slot.
///
/// Coedge `00 11`: 21-byte body, no magic, `refs[9]` at body+2, marker at
/// body+20. `refs[1]` = owning loop, `refs[3]` = next coedge, `refs[4]` = start
/// vertex-use, `refs[5]` = twin coedge, `refs[6]` = edge-use.
fn parse_coedge_candidates(buf: &[u8], off: usize) -> Vec<Record> {
    let Some(p) = body_start(buf, off, 0x11) else {
        return Vec::new();
    };
    if p + 21 > buf.len() {
        return Vec::new();
    }
    let Some(attr) = attr_at(buf, p) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let (Some(refs), Some(marker)) = (refs_be(buf, p + 2, 9), buf.get(p + 20).copied())
        && matches!(marker, 0x2b | 0x2d)
    {
        let mut parsed = record(attr, refs, Some(marker), off);
        parsed.read_ranges = vec![(off, p + 21)];
        out.push(parsed);
    }
    if let (Some(refs), Some(marker)) = (refs_tripled(buf, p + 2, 9), buf.get(p + 29).copied())
        && matches!(marker, 0x2b | 0x2d)
    {
        let mut parsed = record(attr, refs, Some(marker), off);
        parsed.read_ranges = vec![(off, p + 30)];
        out.push(parsed);
    }
    deduplicate_records(out)
}

fn deduplicate_records(mut records: Vec<Record>) -> Vec<Record> {
    records.dedup_by(|right, left| right.attr == left.attr && right.refs == left.refs && right.marker == left.marker && right.offset == left.offset);
    records
}

/// Vertex-use `00 12`: 24-byte body, magic at body+16, `refs[5]` at body+6.
/// `refs[4]` = world-point attr.
fn parse_vertex_use(buf: &[u8], off: usize) -> Option<Record> {
    let p = body_start(buf, off, 0x12)?;
    if p + 24 > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (refs, read_end) = if buf.get(p + 16..p + 24) == Some(MAGIC.as_slice()) {
        (refs_be(buf, p + 6, 5)?, p + 24)
    } else {
        let magic =
            (p + 21..=(p + 32).min(buf.len().saturating_sub(MAGIC.len()))).find(|at| buf.get(*at..*at + MAGIC.len()) == Some(MAGIC.as_slice()))?;
        let count = (magic.checked_sub(p + 6)?) / 3;
        if count < 5 || p + 6 + count * 3 != magic {
            return None;
        }
        (refs_tripled(buf, p + 6, count)?, magic + MAGIC.len())
    };
    Some(Record {
        attr,
        refs,
        marker: None,
        xyz_m: None,
        xyz_offset: None,
        owner: None,
        offset: off,
        read_ranges: vec![(off, p + 2), (p + 6, read_end)],
    })
}

/// World point `00 1d`: 38-byte body, no magic, `refs[4]` at body+6, xyz as
/// three big-endian f64 (metres) at body+14.
fn parse_point(buf: &[u8], off: usize, prefixed: bool) -> Option<Record> {
    let p = body_start(buf, off, 0x1d)?;
    if p + world_pt::LEN > buf.len() {
        return None;
    }
    let attr = attr_at(buf, p)?;
    let (refs, xyz_at) = if prefixed {
        let mut refs = Vec::new();
        let mut cursor = p + world_pt::REFS;
        while buf.get(cursor + 2) == Some(&1) && refs.len() < 16 {
            refs.push(View::u16_be_at(buf, cursor)?);
            cursor += 3;
        }
        if refs.is_empty() {
            return None;
        }
        (refs, cursor)
    } else {
        (refs_be(buf, p + world_pt::REFS, 4)?, p + world_pt::XYZ)
    };
    if refs.first().is_none_or(|reference| *reference > 1) {
        return None;
    }
    let x = View::f64_be_at(buf, xyz_at)?;
    let y = View::f64_be_at(buf, xyz_at + 8)?;
    let z = View::f64_be_at(buf, xyz_at + 16)?;
    for v in [x, y, z] {
        // Reject exponent-poisoned reads from a misaligned candidate: real part
        // coordinates in metres sit well under this cap.
        if !v.is_finite() || v.abs() > 1e4 {
            return None;
        }
    }
    Some(Record {
        attr,
        refs,
        marker: None,
        xyz_m: Some([x, y, z]),
        xyz_offset: Some(xyz_at),
        owner: None,
        offset: off,
        read_ranges: vec![(off, p + 2), (p + world_pt::REFS, xyz_at + 24)],
    })
}

/// The topology record tables of one stream, each keyed by `attr`.
#[derive(Default)]
pub struct Tables {
    pub bridges: HashMap<u16, Record>,
    pub loops: HashMap<u16, Record>,
    pub edge_uses: HashMap<u16, Record>,
    pub coedges: HashMap<u16, Record>,
    pub vertex_uses: HashMap<u16, Record>,
    pub points: HashMap<u16, Record>,
}

impl Tables {
    /// Merge deltas without replacing partition topology membership.
    ///
    /// The change roster does not identify a partition face that a deltas
    /// bridge supersedes. Preserve a partition bridge when its identity is
    /// present and add only missing deltas bridges reached by the final-state
    /// body-relation selector.
    pub fn merge_deltas(&mut self, mut deltas: Self, final_state_refs: Option<&HashSet<u16>>) {
        if self.bridges.is_empty() {
            self.bridges = deltas.bridges;
        } else if let Some(final_state_refs) = final_state_refs {
            deltas
                .bridges
                .retain(|attr, record| final_state_refs.contains(attr) || record.owner.is_some_and(|owner| final_state_refs.contains(&owner)));
            merge_missing(&mut self.bridges, deltas.bridges);
        }
        merge_missing(&mut self.loops, deltas.loops);
        merge_missing(&mut self.edge_uses, deltas.edge_uses);
        merge_missing(&mut self.coedges, deltas.coedges);
        merge_missing(&mut self.vertex_uses, deltas.vertex_uses);
        self.points.extend(deltas.points.drain());
    }
}

fn merge_missing(target: &mut HashMap<u16, Record>, source: HashMap<u16, Record>) {
    for (attr, record) in source {
        target.entry(attr).or_insert(record);
    }
}

type CandidateMap = HashMap<u16, Vec<Record>>;
type CoedgeEvidence = [bool; 7];

/// Keep the latest record occurrence for each attribute while retaining all
/// frame readings at that occurrence. A stream can contain overlapping payload
/// bytes, and a later complete record has the same override semantics as the
/// ordinary topology tables.
fn insert_candidates(target: &mut CandidateMap, records: Vec<Record>) {
    let Some(first) = records.first() else {
        return;
    };
    let attr = first.attr;
    match target.entry(attr) {
        std::collections::hash_map::Entry::Vacant(entry) => {
            entry.insert(records);
        }
        std::collections::hash_map::Entry::Occupied(mut entry) => {
            let current = entry.get();
            if current.first().is_some_and(|record| record.offset < first.offset) {
                entry.insert(records);
            } else if current.first().is_some_and(|record| record.offset == first.offset) {
                let candidates = entry.get_mut();
                candidates.extend(records);
                *candidates = deduplicate_records(std::mem::take(candidates));
            }
        }
    }
}

fn loop_is_owned(record: &Record, bridges: &HashMap<u16, Record>) -> bool {
    record.refs.get(2).is_some_and(|owner| *owner != 0 && bridges.contains_key(owner))
}

/// Collect independent graph invariants for one coedge frame. The first four
/// fields identify the owner, vertex-use, edge-use, and ring; reciprocal links
/// and loop-head membership provide additional confirmation. No field is a
/// byte-position discriminator.
fn coedge_evidence(
    candidate: &Record,
    loops: &[Record],
    bridges: &HashMap<u16, Record>,
    vertex_uses: &HashMap<u16, Record>,
    edge_candidates: &CandidateMap,
    coedge_candidates: &CandidateMap,
) -> CoedgeEvidence {
    let owner = candidate.refs.get(1).copied().unwrap_or(0);
    let owner_valid = owner != 0 && loops.iter().rev().find(|loop_| loop_.attr == owner).is_some_and(|loop_| loop_is_owned(loop_, bridges));
    let start = candidate.refs.get(4).copied().unwrap_or(0);
    let start_valid = start != 0 && vertex_uses.contains_key(&start);
    let edge = candidate.refs.get(6).copied().unwrap_or(0);
    let edge_valid = edge != 0 && edge_candidates.contains_key(&edge);
    let next = candidate.refs.get(3).copied().unwrap_or(0);
    let next_candidates = coedge_candidates.get(&next);
    let next_valid = next != 0 && next_candidates.is_some();
    let next_owner_valid =
        next_candidates.is_some_and(|candidates| candidates.iter().any(|next_candidate| next_candidate.refs.get(1) == Some(&owner)));
    let previous = candidate.refs.get(2).copied().unwrap_or(0);
    let previous_valid = previous == 0
        || coedge_candidates.get(&previous).is_some_and(|candidates| {
            candidates
                .iter()
                .any(|previous_candidate| previous_candidate.refs.get(3) == Some(&candidate.attr) && previous_candidate.refs.get(1) == Some(&owner))
        });
    let loop_head_valid =
        loops.iter().rev().any(|loop_| loop_.attr == owner && loop_is_owned(loop_, bridges) && loop_.refs.get(1) == Some(&candidate.attr));
    [owner_valid, start_valid, edge_valid, next_valid && next_owner_valid, next_valid, previous_valid, loop_head_valid]
}

fn evidence_dominates(left: CoedgeEvidence, right: CoedgeEvidence) -> bool {
    let at_least_as_supported = left.iter().zip(right).all(|(left, right)| *left || !right);
    let strictly_more_supported = left.iter().zip(right).any(|(left, right)| *left && !right);
    at_least_as_supported && strictly_more_supported
}

fn select_coedge(
    candidates: &[Record],
    loops: &[Record],
    bridges: &HashMap<u16, Record>,
    vertex_uses: &HashMap<u16, Record>,
    edge_candidates: &CandidateMap,
    coedge_candidates: &CandidateMap,
) -> Option<Record> {
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }
    let evidence: Vec<CoedgeEvidence> =
        candidates.iter().map(|candidate| coedge_evidence(candidate, loops, bridges, vertex_uses, edge_candidates, coedge_candidates)).collect();
    let maximal: Vec<usize> = (0..candidates.len())
        .filter(|&index| {
            !evidence.iter().enumerate().any(|(other, other_evidence)| other != index && evidence_dominates(*other_evidence, evidence[index]))
        })
        .collect();
    if maximal.len() == 1 { candidates.get(maximal[0]).cloned() } else { None }
}

fn edge_candidate_is_valid(candidate: &Record, coedges: &HashMap<u16, Record>, curve_attrs: Option<&HashSet<u16>>) -> bool {
    let canonical = candidate.refs.first().copied().unwrap_or(0);
    let curve = candidate.refs.get(3).copied().unwrap_or(0);
    let canonical_valid = canonical == 0 || coedges.contains_key(&canonical);
    let curve_valid = curve == 0 || curve_attrs.is_none_or(|attrs| attrs.contains(&curve));
    canonical_valid && curve_valid
}

fn select_edge_use(candidates: &[Record], coedges: &HashMap<u16, Record>, curve_attrs: Option<&HashSet<u16>>) -> Option<Record> {
    if candidates.len() == 1 {
        return candidates.first().cloned();
    }
    let mut valid = candidates.iter().filter(|candidate| edge_candidate_is_valid(candidate, coedges, curve_attrs));
    let first = valid.next()?.clone();
    if valid.next().is_none() { Some(first) } else { None }
}

fn xyz_bytes(xyz_m: [f64; 3]) -> [u8; 24] {
    let mut bytes = [0; 24];
    for (slot, value) in bytes.chunks_exact_mut(8).zip(xyz_m) {
        slot.copy_from_slice(&value.to_be_bytes());
    }
    bytes
}

/// Replace coordinates only when the old bytes still match the parsed record.
pub fn patch_point_values(buf: &mut [u8], xyz_at: usize, old_xyz_m: [f64; 3], new_xyz_m: [f64; 3]) -> bool {
    let old_bytes = xyz_bytes(old_xyz_m);
    if buf.get(xyz_at..xyz_at + old_bytes.len()) != Some(old_bytes.as_slice()) {
        return false;
    }
    let new_bytes = xyz_bytes(new_xyz_m);
    let Some(bytes) = buf.get_mut(xyz_at..xyz_at + new_bytes.len()) else {
        return false;
    };
    bytes.copy_from_slice(&new_bytes);
    true
}

/// Replace one world-point record while preserving its framing.
pub fn patch_point(buf: &mut [u8], attr: u16, xyz_m: [f64; 3]) -> bool {
    let Some(record) = scan(buf).points.remove(&attr) else {
        return false;
    };
    let Some(old_xyz_m) = record.xyz_m else {
        return false;
    };
    let Some(xyz_at) = record.xyz_offset else {
        return false;
    };
    patch_point_values(buf, xyz_at, old_xyz_m, xyz_m)
}

/// Scan the stream body for every typed topology record. Successful records do
/// not advance the scan past their extent because valid records can overlap an
/// enclosing payload. Family-specific framing gates reject payload coincidences.
/// Later full records replace earlier records with the same `attr`, matching
/// partition-base plus deltas-override merge order.
pub fn scan(body: &[u8]) -> Tables {
    scan_with_point_framing(body, false, None)
}

/// Scan a partition stream with the typed curve attributes available to
/// resolve an otherwise ambiguous edge-use reference orientation.
pub fn scan_with_curve_attrs(body: &[u8], curve_attrs: &HashSet<u16>) -> Tables {
    scan_with_point_framing(body, false, Some(curve_attrs))
}

/// Scan a deltas stream with the typed curve attributes available to resolve
/// an otherwise ambiguous edge-use reference orientation.
pub fn scan_deltas_with_curve_attrs(body: &[u8], curve_attrs: &HashSet<u16>) -> Tables {
    scan_with_point_framing(body, true, Some(curve_attrs))
}

fn scan_with_point_framing(body: &[u8], prefixed_points: bool, curve_attrs: Option<&HashSet<u16>>) -> Tables {
    let mut t = Tables::default();
    let mut loop_candidates = Vec::new();
    let mut edge_candidates = CandidateMap::new();
    let mut coedge_candidates = CandidateMap::new();
    let mut i = 0usize;
    while i + 14 <= body.len() {
        if body[i] != 0x00 {
            i += 1;
            continue;
        }
        match body[i + 1] {
            0x0e => {
                if let Some(record) = parse_bridge(body, i) {
                    t.bridges.insert(record.attr, record);
                }
            }
            0x0f => {
                if let Some(record) = parse_loop(body, i) {
                    loop_candidates.push(record);
                }
            }
            0x10 => insert_candidates(&mut edge_candidates, parse_edge_use_candidates(body, i)),
            0x11 => insert_candidates(&mut coedge_candidates, parse_coedge_candidates(body, i)),
            0x12 => {
                if let Some(record) = parse_vertex_use(body, i) {
                    t.vertex_uses.insert(record.attr, record);
                }
            }
            0x1d => {
                let record =
                    if prefixed_points { parse_point(body, i, true).or_else(|| parse_point(body, i, false)) } else { parse_point(body, i, false) };
                if let Some(record) = record {
                    t.points.insert(record.attr, record);
                }
            }
            _ => {}
        }
        i += 1;
    }

    for (attr, candidates) in &coedge_candidates {
        if let Some(record) = select_coedge(candidates, &loop_candidates, &t.bridges, &t.vertex_uses, &edge_candidates, &coedge_candidates) {
            t.coedges.insert(*attr, record);
        }
    }
    for (attr, candidates) in edge_candidates {
        if let Some(record) = select_edge_use(&candidates, &t.coedges, curve_attrs) {
            t.edge_uses.insert(attr, record);
        }
    }
    for record in loop_candidates {
        let owner = record.refs.get(2).copied().unwrap_or(0);
        let first = record.refs.get(1).copied().unwrap_or(0);
        if t.bridges.contains_key(&owner) && t.coedges.get(&first).is_some_and(|coedge| coedge.refs.get(1) == Some(&record.attr)) {
            t.loops.insert(record.attr, record);
        }
    }
    t
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bridge_with_refs(refs: &[u16], tripled: bool) -> Vec<u8> {
        let mut bytes = vec![0, 0x0e];
        bytes.extend(0x1234_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        bytes.extend(0x4321_u16.to_be_bytes());
        bytes.extend(MAGIC);
        for reference in refs {
            bytes.extend(reference.to_be_bytes());
            if tripled {
                bytes.push(1);
            }
        }
        bytes.push(0x2d);
        bytes.resize(40, 0);
        bytes
    }

    #[test]
    fn bridge_deltas_form_reads_tripled_owner_and_refs() {
        let expected: Vec<u16> = vec![0x101, 0x202, 0x303, 0x404, 0x505];
        let mut bytes = vec![0, 0x0e, 0xff];
        bytes.extend(0x1234_u16.to_be_bytes());
        bytes.extend(7_u32.to_be_bytes());
        bytes.extend(0x4321_u16.to_be_bytes());
        bytes.push(1);
        bytes.extend(MAGIC);
        for reference in &expected {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(0x2b);
        bytes.resize(48, 0);

        let bridge = parse_bridge(&bytes, 0).expect("deltas-form bridge");
        assert_eq!(bridge.attr, 0x1234);
        assert_eq!(bridge.owner, Some(0x4321));
        assert_eq!(bridge.refs, expected);
        assert_eq!(bridge.marker, Some(0x2b));
    }

    #[test]
    fn bridge_refs_accept_adjacent_and_tripled_cells() {
        let expected = vec![0x101, 0x202, 0x303, 0x404, 0x505];
        for tripled in [false, true] {
            let bytes = bridge_with_refs(&expected, tripled);
            let bridge = parse_bridge(&bytes, 0).unwrap_or_else(|| panic!("bridge tripled={tripled} bytes={bytes:02x?}"));
            assert_eq!(bridge.attr, 0x1234);
            assert_eq!(bridge.owner, Some(0x4321));
            assert_eq!(bridge.refs, expected);
            assert_eq!(bridge.marker, Some(0x2d));
        }
    }

    fn topology_bridge(attr: u16, loop_attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0x0e];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend(MAGIC);
        for reference in [0, 0, loop_attr, 0, 100] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes.push(0x2b);
        bytes.extend([0; 10]);
        bytes
    }

    fn topology_loop(attr: u16, first_coedge: u16, bridge_attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0x0f];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0, first_coedge, bridge_attr, 0] {
            bytes.extend(reference.to_be_bytes());
        }
        bytes
    }

    fn topology_tripled_coedge(attr: u16, refs: [u16; 9]) -> Vec<u8> {
        let mut bytes = vec![0, 0x11];
        bytes.extend(attr.to_be_bytes());
        for reference in refs {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }
        bytes.push(0x2b);
        bytes
    }

    fn topology_edge_use(attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0x10];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend(MAGIC);
        bytes.extend([0_u16, 0, 0, 0, 0, 0].into_iter().flat_map(u16::to_be_bytes));
        bytes
    }

    fn topology_vertex_use(attr: u16) -> Vec<u8> {
        let mut bytes = vec![0, 0x12];
        bytes.extend(attr.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend([0_u16, 0, 0, 0, 60].into_iter().flat_map(u16::to_be_bytes));
        bytes.extend(MAGIC);
        bytes
    }

    #[test]
    fn coedge_tripled_frame_wins_over_marker_like_reference_byte() {
        let mut body = Vec::new();
        body.extend(topology_bridge(10, 20));
        body.extend(topology_loop(20, 30, 10));
        body.extend(topology_tripled_coedge(30, [0, 20, 0, 30, 50, 0, 0x2b40, 0, 0]));
        body.extend(topology_edge_use(0x2b40));
        body.extend(topology_vertex_use(50));

        let tables = scan(&body);
        let coedge = tables.coedges.get(&30).expect("tripled coedge");
        assert_eq!(coedge.refs[1], 20);
        assert_eq!(coedge.refs[4], 50);
        assert_eq!(coedge.refs[6], 0x2b40);
        assert!(tables.loops.contains_key(&20));
    }

    #[test]
    fn suffix_edge_use_frame_wins_when_first_reference_starts_with_one() {
        let mut bytes = vec![0, 0x10];
        bytes.extend(40_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        bytes.extend(0_u16.to_be_bytes());
        bytes.extend([1, 0, 0]);
        bytes.extend(MAGIC);
        for reference in [0x0101_u16, 0x0102, 0x0103] {
            bytes.extend(reference.to_be_bytes());
            bytes.push(1);
        }

        assert!(scan(&bytes).edge_uses.is_empty(), "ambiguous without a carrier set");
        let curve_attrs = HashSet::from([0x0103]);
        let tables = scan_deltas_with_curve_attrs(&bytes, &curve_attrs);
        assert_eq!(tables.edge_uses[&40].refs[3], 0x0103);
    }

    #[test]
    fn patch_point_uses_parsed_adjacent_coordinate_offset() {
        let mut bytes = vec![0, 0x1d];
        bytes.extend(60_u16.to_be_bytes());
        bytes.extend(0_u32.to_be_bytes());
        for reference in [0_u16, 0x0102, 0, 0] {
            bytes.extend(reference.to_be_bytes());
        }
        for value in [1.0_f64, 2.0, 3.0] {
            bytes.extend(value.to_be_bytes());
        }

        assert!(!patch_point_values(&mut bytes, 11, [1.0, 2.0, 3.0], [4.0, 5.0, 6.0]));
        assert!(patch_point(&mut bytes, 60, [4.0, 5.0, 6.0]));

        let point = parse_point(&bytes, 0, false).expect("adjacent world point");
        assert_eq!(point.refs, vec![0, 0x0102, 0, 0]);
        assert_eq!(point.xyz_m, Some([4.0, 5.0, 6.0]));
        assert_eq!(point.xyz_offset, Some(16));
    }
}
