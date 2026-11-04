// SPDX-License-Identifier: Apache-2.0
//! Synthetic ASM construction-history partitions.
//!
//! [`HistoryPartition`] writes the history grammar of
//! [ASM §3](https://github.com/cadmpeg/cadmpeg/blob/main/docs/formats/asm.md#3-history-partition):
//! the `Begin-of-ASM-History-Data` preamble, linked `delta_state` records with
//! their bulletin-board chains, and the tail snapshot of archived record
//! revisions closed by `End-of-ASM-data`. [`DesignScale`] pairs an active
//! model of independent triangular sheet bodies with a history that builds it
//! one state at a time.
#![allow(clippy::unwrap_used)]

use crate::test_support::smbh_header_test::smbh_header_prefix;
use crate::test_support::tokens_test::{
    push_u8_string, t_dbl, t_end, t_ident, t_long, t_pos, t_ref, t_subident, t_vec,
};

/// One bulletin-board entry. References use the history revision namespace.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum EntityChange {
    Insert { new: i64 },
    Update { old: i64, new: i64 },
    Delete { old: i64 },
}

impl EntityChange {
    fn references(self) -> [i64; 2] {
        match self {
            Self::Insert { new } => [-1, new],
            Self::Update { old, new } => [old, new],
            Self::Delete { old } => [old, -1],
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct BulletinBoard {
    pub(crate) owner: i64,
    pub(crate) number: i64,
    pub(crate) changes: Vec<EntityChange>,
}

/// A history partition in chain order: `states[0]` is the head, the current
/// model, and the last state is the tail, the initial model.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub(crate) struct HistoryPartition {
    pub(crate) states: Vec<Vec<BulletinBoard>>,
    /// Archived record revisions in revision order. Each is a complete record,
    /// its name chain and payload, without the `0x11` terminator. The first
    /// is an edge, which the `End-of-ASM-History-Section` chain wraps.
    pub(crate) snapshot: Vec<Vec<u8>>,
}

impl HistoryPartition {
    /// Append the partition after an active record table whose final record
    /// ends with its `0x11` terminator.
    pub(crate) fn append_to(&self, smbh: &mut Vec<u8>) {
        let count = i64::try_from(self.states.len()).unwrap();
        assert!(count > 0, "a history partition has a head state");
        // The head state's id equals the preamble stream size.
        let state_id = |node: i64| count - node;
        for part in ["Begin", "of", "ASM", "History"] {
            t_subident(smbh, part);
        }
        t_ident(smbh, "Data");
        t_ident(smbh, "history_stream");
        for value in [state_id(0), state_id(0), 0, count] {
            t_long(smbh, value);
        }
        for reference in [-1, 0, count - 1, -1] {
            t_ref(smbh, reference);
        }
        t_end(smbh);
        for (node, boards) in (0..).zip(&self.states) {
            t_ident(smbh, "delta_state");
            for value in [state_id(node), 1, 0] {
                t_long(smbh, value);
            }
            let previous = if node == 0 { -1 } else { node - 1 };
            let next = if node + 1 == count { -1 } else { node + 1 };
            for reference in [previous, next, node, -1, 0] {
                t_ref(smbh, reference);
            }
            smbh.push(0x0b);
            for board in boards {
                t_long(smbh, 1);
                t_ref(smbh, board.owner);
                t_long(smbh, board.number);
                for change in &board.changes {
                    t_long(smbh, 1);
                    for reference in change.references() {
                        t_ref(smbh, reference);
                    }
                }
                t_long(smbh, 0);
            }
            t_long(smbh, 0);
            // The separator's terminator delimits the next `delta_state`.
            t_long(smbh, 0);
            t_end(smbh);
        }
        for part in ["End", "of", "ASM", "History"] {
            t_subident(smbh, part);
        }
        t_ident(smbh, "Section");
        let mut snapshot = self.snapshot.iter();
        if let Some(first) = snapshot.next() {
            assert!(
                first.starts_with(b"\x0d\x04edge"),
                "the history section wraps only an edge snapshot"
            );
            smbh.extend_from_slice(first);
        }
        t_end(smbh);
        for record in snapshot {
            smbh.extend_from_slice(record);
            t_end(smbh);
        }
        // The final record ends at the stream boundary without a terminator.
        for part in ["End", "of", "ASM"] {
            t_subident(smbh, part);
        }
        t_ident(smbh, "data");
    }
}

/// Records that open a sheet body: body, region, and shell.
const BODY_HEADER_RECORDS: i64 = 3;
/// Records per triangular face: face, loop, plane, three coedges, three
/// edges, three vertices, and three points.
const FACE_RECORDS: i64 = 15;
/// Offset of a face's first edge within its record block.
const FIRST_EDGE: i64 = 6;
/// Offset of a face's first vertex within its record block.
const FIRST_VERTEX: i64 = 9;
/// Spacing between neighbouring faces, in centimetres.
const FACE_PITCH: f64 = 2.0;
/// Edge parameter end before an archived edge's update.
const ARCHIVED_EDGE_END: f64 = 0.5;
/// Every this many states, one state inserts a one-face tool body that the
/// next state deletes.
const TOOL_BODY_PERIOD: usize = 4;

/// Record count of a sheet body with `faces` faces.
fn body_records(faces: i64) -> i64 {
    BODY_HEADER_RECORDS + faces * FACE_RECORDS
}

/// A design of sheet bodies of independent triangular faces built over a
/// linked history. Bodies spread evenly over the states, oldest first. Every
/// newer state also updates edges of earlier bodies, each edge at most once,
/// and periodically a state inserts a tool body that the next state deletes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct DesignScale {
    pub(crate) states: usize,
    pub(crate) bodies: usize,
    pub(crate) faces_per_body: usize,
    pub(crate) updates_per_state: usize,
}

impl DesignScale {
    /// Active `RecordTable` length, the `asmheader` included.
    pub(crate) fn active_records(self) -> usize {
        1 + self.bodies * usize::try_from(self.body_records()).unwrap()
    }

    fn faces(self) -> i64 {
        i64::try_from(self.faces_per_body).unwrap()
    }

    fn body_records(self) -> i64 {
        body_records(self.faces())
    }

    fn body_base(self, body: usize) -> i64 {
        1 + i64::try_from(body).unwrap() * self.body_records()
    }

    /// Bulletin-board entries across every state.
    pub(crate) fn total_changes(self) -> usize {
        self.history()
            .states
            .iter()
            .flatten()
            .map(|board| board.changes.len())
            .sum()
    }

    /// The active model with an empty history partition.
    pub(crate) fn snapshot_smbh(self) -> Vec<u8> {
        let mut smbh = self.active_smbh();
        t_ident(&mut smbh, "delta_state");
        smbh
    }

    /// The active model followed by its construction history.
    pub(crate) fn history_smbh(self) -> Vec<u8> {
        let mut smbh = self.active_smbh();
        self.history().append_to(&mut smbh);
        smbh
    }

    fn active_smbh(self) -> Vec<u8> {
        let mut smbh = smbh_header_prefix();
        t_ident(&mut smbh, "asmheader");
        push_u8_string(&mut smbh, "231.6.3.65535");
        t_end(&mut smbh);
        for body in 0..self.bodies {
            let ordinal = i64::try_from(body).unwrap();
            for record in sheet_body_records(self.body_base(body), ordinal, self.faces()) {
                smbh.extend_from_slice(&record);
            }
        }
        smbh
    }

    /// Chronological state that inserts `body`.
    fn insertion_state(self, body: usize) -> usize {
        body * self.states / self.bodies
    }

    fn history(self) -> HistoryPartition {
        let edges_per_body = 3 * self.faces_per_body;
        assert!(self.states >= 2 && self.bodies >= self.states && self.faces_per_body >= 1);
        assert!(
            self.updates_per_state <= edges_per_body,
            "each state inserts at least one body, whose edges cover the updates"
        );
        // Chronological updates: the next edges, in body order, of bodies
        // that an earlier state inserted. Each is (edge, first vertex of its
        // face, ordinal within the face).
        let mut updated_edges = vec![Vec::new(); self.states];
        let mut cursor = 0usize;
        for (state, edges) in updated_edges.iter_mut().enumerate().skip(1) {
            for _ in 0..self.updates_per_state {
                let body = cursor / edges_per_body;
                assert!(self.insertion_state(body) < state);
                let within = i64::try_from(cursor % edges_per_body).unwrap();
                let face = self.body_base(body) + BODY_HEADER_RECORDS + within / 3 * FACE_RECORDS;
                let ordinal = within % 3;
                edges.push((face + FIRST_EDGE + ordinal, face + FIRST_VERTEX, ordinal));
                cursor += 1;
            }
        }
        let first_revision = i64::try_from(self.active_records()).unwrap();
        let mut partition = HistoryPartition::default();
        // States are written head first, so a tool body's deletion allocates
        // the revisions that its older insertion state then inserts.
        let mut tool_revisions = None;
        for state in (0..self.states).rev() {
            let mut boards = Vec::new();
            let mut changes = Vec::new();
            for &(edge, first_vertex, ordinal) in &updated_edges[state] {
                let old = first_revision + i64::try_from(partition.snapshot.len()).unwrap();
                changes.push(EntityChange::Update { old, new: edge });
                let mut archived = edge_record(
                    first_vertex + ordinal,
                    first_vertex + (ordinal + 1) % 3,
                    ARCHIVED_EDGE_END,
                );
                archived.pop();
                partition.snapshot.push(archived);
            }
            if state % TOOL_BODY_PERIOD == 2 {
                let base = first_revision + i64::try_from(partition.snapshot.len()).unwrap();
                let ordinal = i64::try_from(state).unwrap();
                for (old, mut record) in (base..).zip(sheet_body_records(base, -1 - ordinal, 1)) {
                    changes.push(EntityChange::Delete { old });
                    record.pop();
                    partition.snapshot.push(record);
                }
                tool_revisions = Some(base..base + body_records(1));
            }
            if !changes.is_empty() {
                boards.push(BulletinBoard {
                    owner: 0,
                    number: 2,
                    changes,
                });
            }
            let mut inserted = (0..self.bodies)
                .filter(|&body| self.insertion_state(body) == state)
                .flat_map(|body| {
                    let first = self.body_base(body);
                    first..first + self.body_records()
                })
                .map(|new| EntityChange::Insert { new })
                .collect::<Vec<_>>();
            if state % TOOL_BODY_PERIOD == 1 {
                if let Some(revisions) = tool_revisions.take() {
                    inserted.extend(revisions.map(|new| EntityChange::Insert { new }));
                }
            }
            boards.push(BulletinBoard {
                owner: 0,
                number: 1,
                changes: inserted,
            });
            partition.states.push(boards);
        }
        assert!(
            tool_revisions.is_none(),
            "every deleted tool body is inserted"
        );
        partition
    }
}

/// One sheet body of `faces` planar triangular faces, each in the face layout
/// of `synthetic_geometry_smbh`. Its records are numbered from `base`; face
/// `f` lies at `(ordinal, f) × FACE_PITCH` in x and y. Each record keeps its
/// terminator.
fn sheet_body_records(base: i64, ordinal: i64, faces: i64) -> Vec<Vec<u8>> {
    let [region, shell] = [1, 2].map(|offset| base + offset);
    let face_block = |face: i64| base + BODY_HEADER_RECORDS + face * FACE_RECORDS;
    let mut records = vec![
        framed(|r| {
            t_ident(r, "body");
            t_ref(r, -1);
            t_long(r, 1000 + ordinal);
            t_ref(r, -1);
            t_ref(r, region);
            t_ref(r, -1);
            t_ref(r, -1);
        }),
        framed(|r| {
            t_ident(r, "region");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, -1);
            t_ref(r, shell);
            t_ref(r, base);
        }),
        framed(|r| {
            t_ident(r, "shell");
            t_ref(r, -1);
            t_long(r, -1);
            for _ in 0..3 {
                t_ref(r, -1);
            }
            t_ref(r, face_block(0));
            t_ref(r, -1);
            t_ref(r, region);
        }),
    ];
    for face in 0..faces {
        let next_face = if face + 1 == faces {
            -1
        } else {
            face_block(face + 1)
        };
        let origin =
            [ordinal, face].map(|lane| FACE_PITCH * f64::from(i32::try_from(lane).unwrap()));
        records.extend(face_records(face_block(face), shell, next_face, origin));
    }
    records
}

/// One planar triangular face with its loop, surface, coedges, edges,
/// vertices, and points, numbered from `base`, with its corner at `origin`.
fn face_records(base: i64, shell: i64, next_face: i64, origin: [f64; 2]) -> Vec<Vec<u8>> {
    let [loop_, surface] = [1, 2].map(|offset| base + offset);
    let coedge = |ordinal: i64| base + 3 + ordinal % 3;
    let edge = |ordinal: i64| base + FIRST_EDGE + ordinal % 3;
    let vertex = |ordinal: i64| base + FIRST_VERTEX + ordinal % 3;
    let point = |ordinal: i64| base + 12 + ordinal;
    let [x0, y0] = origin;
    let mut records = vec![
        framed(|r| {
            t_ident(r, "face");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, next_face);
            t_ref(r, loop_);
            t_ref(r, shell);
            t_ref(r, -1);
            t_ref(r, surface);
            r.extend_from_slice(&[0x0b, 0x0b]);
        }),
        framed(|r| {
            t_ident(r, "loop");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, -1);
            t_ref(r, coedge(0));
            t_ref(r, base);
        }),
        framed(|r| {
            t_subident(r, "plane");
            t_ident(r, "surface");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_pos(r, [x0, y0, 0.0]);
            t_vec(r, [0.0, 0.0, 1.0]);
            t_vec(r, [1.0, 0.0, 0.0]);
            r.push(0x0b);
        }),
    ];
    for ordinal in 0..3 {
        records.push(framed(|r| {
            t_ident(r, "coedge");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, coedge(ordinal + 1));
            t_ref(r, coedge(ordinal + 2));
            t_ref(r, -1);
            t_ref(r, edge(ordinal));
            r.push(0x0b);
            t_ref(r, loop_);
            t_long(r, 0);
            t_ref(r, -1);
        }));
    }
    for ordinal in 0..3 {
        records.push(edge_record(vertex(ordinal), vertex(ordinal + 1), 1.0));
    }
    for (ordinal, (owner, index)) in (0..).zip([(edge(0), 0), (edge(0), 1), (edge(2), 0)]) {
        records.push(framed(|r| {
            t_ident(r, "vertex");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_ref(r, owner);
            t_long(r, index);
            t_ref(r, point(ordinal));
        }));
    }
    for [x, y] in [[0.0, 0.0], [1.0, 0.0], [0.0, 1.0]] {
        records.push(framed(|r| {
            t_ident(r, "point");
            t_ref(r, -1);
            t_long(r, -1);
            t_ref(r, -1);
            t_pos(r, [x0 + x, y0 + y, 0.0]);
        }));
    }
    records
}

/// One record written by `write`, closed by its terminator.
fn framed(write: impl FnOnce(&mut Vec<u8>)) -> Vec<u8> {
    let mut r = Vec::new();
    write(&mut r);
    t_end(&mut r);
    r
}

/// A curveless edge between two vertex slots, with its terminator.
fn edge_record(start: i64, end: i64, parameter_end: f64) -> Vec<u8> {
    let mut r = Vec::new();
    t_ident(&mut r, "edge");
    t_ref(&mut r, -1);
    t_long(&mut r, -1);
    t_ref(&mut r, -1);
    t_ref(&mut r, start);
    t_dbl(&mut r, 0.0);
    t_ref(&mut r, end);
    t_dbl(&mut r, parameter_end);
    t_ref(&mut r, -1);
    t_ref(&mut r, -1);
    r.push(0x0b);
    push_u8_string(&mut r, "unknown");
    t_end(&mut r);
    r
}
