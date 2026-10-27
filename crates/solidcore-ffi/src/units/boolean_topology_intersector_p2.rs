//! FFI wrappers for unit `boolean.topology_intersector_p2`.
//!
//! Shim: `harness/cpp/shim/boolean_topology_intersector_p2.cpp`
//! (`sc_boolean_topology_intersector_p2_eval`). Upstream:
//! `source/SMLib/src/SmTopologyIntersector.cpp`.
//!
//! Part 2 runs on part 1's intersector and script: the inputs, outputs, handles, ops 1
//! to 33 and their validation are those of [`super::boolean_topology_intersector_p1`];
//! this binding adds the ops of part 2 (`pcf` as in part 1; `cc` a curve
//! classification `curve t0 t1 tol n (a b pci pci pci)*`, each `pci` a point
//! classification input `class h t has_t dev`):
//!
//! | op | name | args | outputs |
//! |----|------|------|---------|
//! | 41 | `RemoveRelationship`        | `h h`                    | `status` |
//! | 42 | `CheckForSqueezeVertices`   | `h h`                    | `status n_deleted_v n_surviving_v n_deleted_e` |
//! | 43 | `DeleteFaces`               | `n h* has_keep n h*`     | `status` |
//! | 44 | `FixGaps`                   | `n h*`                   | `status modified n_deleted_v n_deleted_e` |
//! | 45 | `IntersectTopology`         |                          | `status n_pv n_pe n_pf` |
//! | 46 | `SetRelationships`          |                          | `status n_pv n_pe n_pf` |
//! | 47 | `IntersectInsertRelate`     |                          | `status` |
//! | 48 | `FixupClassMates`           | `cc cc`                  | `status ccf ccf` |
//! | 49 | `FixupSplitVertexPair`      | `index cc cc`            | `status changed ccf ccf` |
//! | 50 | `EdgeEdgeIntersect`         | `n h* n h* manifold ignore` | `status n h*(split1) n_new1 n h*(split2) n_new2 n_verts max min` |
//! | 51 | `EdgeFaceCoincidence`       | `n h* n h* ignore nesting` | `status n h*(imprinted) n h*(processed) n_new n h*(split) max min` |
//! | 52 | `CloseIntersectionLoop`     | `h`                      | `status made_new_topology dv de n h*` (the deleted vertex and edge counts, then the other mate of each edge of the vertex) |
//!
//! `ccf` is `n (a b pcf pcf pcf)*`. Ops 42 to 44 and 47 to 52 emit their outputs only
//! on a success status. Ops 42 to 44, 47 and 50 to 52 may change topology (52 through
//! `IIREdgeBrep`), so 52 reports the relationships it can make itself.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour, on top of
//! part 1's rules: vertex handles where upstream calls vertex members (42, 44, 51; a
//! vertex classification of 48/49 must name a vertex), faces (43), edges with a
//! curve and vertices (50), an interval index inside both classifications (49), a
//! classification curve; a vertex or null for 52, with both intersector B-reps; no
//! handle-taking op may follow an op that may change topology; 45 and 46 need the
//! manager (45 both intersector
//! B-reps, 47 the maps of constructor 0), 47 both B-reps, a nonzero approximation
//! tolerance (HANG-25) of magnitude at most [`IIR_MAX_APPROX`] (a time-budget guard, not
//! UB: larger ones finish but take seconds to minutes) and B-rep blocks below
//! [`HANG_COORD`] (HANG-16's class: the full upstream intersection does not finish
//! otherwise) and it must be the last op.
//! The shim checks what depends on the B-reps' and the manager's state (mates that
//! are not vertices where upstream dereferences them, edges of a vertex without
//! curve, faces without surface, 46 without the maps reaching `Relate`'s insert, 52's
//! edges without curve, vertices or (the B-rep vertex's) primary edge use) and
//! answers `-2`; it also answers `-2` for a 47 whose manager teardown would read a
//! freed proto edge (UB-175: the shim runs a verbatim copy of
//! `IntersectInsertRelate` that unlinks the stale entries first).
//!
//! 49 checks the index against both classifications only: upstream bounds its
//! `index + 1` writes by the B-rep classification's size, so with a shorter other
//! classification the write lands in `SmTArray`'s spare slots (at least 16 are
//! allocated) and is hidden by the `RemoveAt`. That holds only while
//! [`MAX_INTERVALS`] is below 16; past it the guard must refuse such pairs.

use super::boolean_topology_intersector_p1::{
    bad, eval_with, Counts, Entry, ExtraOps, HandleKind, Op, Reader, ScriptState,
};
use crate::FfiError;

/// The magnitude from which `IntersectInsertRelate`'s full intersection does not finish.
pub const HANG_COORD: f64 = 1.0e154;

/// The largest approximation-tolerance magnitude `IntersectInsertRelate` (47) is run
/// with: a **time-budget guard** on a defined input, not undefined behaviour. With a
/// larger tolerance the full upstream intersection still finishes but slows down
/// sharply (the C++ side of `Mouse.smb` x `vase_rev.smb`: 0.5 s at |1|, 5-10 s at |10|,
/// 27-29 s at |1e3|, 20 s at |1e6|; `torus.smb` x `ellips_ts2.smb` 1.2-1.9 s from |10|;
/// see `plan/findings.md`). The bound is 10x below the largest measured magnitude
/// that stays under 2 s on every op-47 fixture (|1|). Generated tolerances are at most
/// 1e-2.
pub const IIR_MAX_APPROX: f64 = 0.1;
/// Most intervals in one classification.
pub const MAX_INTERVALS: usize = 16;

/// Part 2's script op codes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum ScriptOp2 {
    RemoveRelationship = 41,
    Squeeze = 42,
    DeleteFaces = 43,
    FixGaps = 44,
    IntersectTopology = 45,
    SetRelationships = 46,
    IntersectInsertRelate = 47,
    FixupClassMates = 48,
    FixupSplitVertexPair = 49,
    EdgeEdge = 50,
    EdgeFaceCoincidence = 51,
    CloseIntersectionLoop = 52,
}

impl ScriptOp2 {
    /// The op's code.
    #[must_use]
    pub fn code(self) -> i64 {
        self as i64
    }

    /// The op of a code.
    #[must_use]
    pub fn from_code(code: i64) -> Option<Self> {
        use ScriptOp2::{
            DeleteFaces, EdgeEdge, EdgeFaceCoincidence, FixGaps, FixupClassMates,
            FixupSplitVertexPair, IntersectInsertRelate, IntersectTopology, RemoveRelationship,
            SetRelationships, Squeeze,
        };
        const ALL: [ScriptOp2; 12] = [
            RemoveRelationship,
            Squeeze,
            DeleteFaces,
            FixGaps,
            IntersectTopology,
            SetRelationships,
            IntersectInsertRelate,
            FixupClassMates,
            FixupSplitVertexPair,
            EdgeEdge,
            EdgeFaceCoincidence,
            ScriptOp2::CloseIntersectionLoop,
        ];
        ALL.iter().copied().find(|o| o.code() == code)
    }

    /// Whether the op may delete or split topology (no handle-taking op may follow).
    #[must_use]
    pub fn mutates(self) -> bool {
        matches!(
            self,
            Self::Squeeze
                | Self::DeleteFaces
                | Self::FixGaps
                | Self::IntersectInsertRelate
                | Self::EdgeEdge
                | Self::EdgeFaceCoincidence
                | Self::CloseIntersectionLoop
        )
    }
}

mod sys {
    extern "C" {
        pub fn sc_boolean_topology_intersector_p2_eval(
            op: i32,
            path: *const std::ffi::c_char,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A point classification input `class h t has_t dev`.
fn point_class_input(rd: &mut Reader<'_>, c: &Counts) -> Result<(), FfiError> {
    let cls = rd.int(100, 110)?; // SmPointClassificationType
    let k = c.handle(rd, true)?;
    // SM_PC_VERTEX: FixupSplitVertexPair calls GetVertexObject()->GetPoint()
    if cls == 104 && k != HandleKind::Vertex {
        return Err(bad("vertex classification of a non-vertex"));
    }
    rd.finite()?;
    rd.flag()?;
    rd.finite()?;
    Ok(())
}

/// A curve classification `curve t0 t1 tol n (a b pci pci pci)*`; returns its size.
fn classification(rd: &mut Reader<'_>, c: &Counts) -> Result<usize, FfiError> {
    c.handle_of(rd, &[HandleKind::EdgeCurve, HandleKind::Curve], false)?;
    rd.finite()?;
    rd.finite()?;
    rd.finite()?; // tolerance (negative accepted, as upstream)
    let n = rd.index(MAX_INTERVALS)?;
    for _ in 0..n {
        rd.finite()?;
        rd.finite()?;
        for _ in 0..3 {
            point_class_input(rd, c)?;
        }
    }
    Ok(n)
}

/// Part 2's ops.
struct Part2;

impl ExtraOps for Part2 {
    fn max_code(&self) -> i64 {
        52
    }

    fn op(
        &self,
        code: i64,
        rd: &mut Reader<'_>,
        c: &mut Counts,
        st: &mut ScriptState,
    ) -> Result<bool, FfiError> {
        use HandleKind::{Edge, Face, Vertex};
        let Some(op) = ScriptOp2::from_code(code) else {
            return Ok(false);
        };
        let takes_handles = !matches!(
            op,
            ScriptOp2::IntersectTopology
                | ScriptOp2::SetRelationships
                | ScriptOp2::IntersectInsertRelate
        );
        if st.mutated && takes_handles {
            return Err(bad("handle-taking op after a mutating op"));
        }
        match op {
            ScriptOp2::RemoveRelationship => {
                let any = [
                    Vertex,
                    Edge,
                    Face,
                    HandleKind::EdgeCurve,
                    HandleKind::FaceSurface,
                    HandleKind::Curve,
                    HandleKind::Brep,
                ];
                c.handle_of(rd, &any, true)?;
                c.handle_of(rd, &any, true)?;
            }
            ScriptOp2::Squeeze => {
                c.handle_of(rd, &[Vertex], false)?;
                c.handle_of(rd, &[Vertex], false)?;
            }
            ScriptOp2::DeleteFaces => {
                c.list(rd, &[Face], false)?;
                rd.flag()?;
                c.list(rd, &[Edge], true)?;
            }
            ScriptOp2::FixGaps => c.list(rd, &[Vertex], false)?,
            ScriptOp2::IntersectTopology => {
                if !st.has_mgr || !st.have_breps[0] || !st.have_breps[1] {
                    return Err(bad("IntersectTopology needs the manager and both B-reps"));
                }
            }
            ScriptOp2::SetRelationships => {
                // (without the maps, constructor 1, the shim refuses a definition with
                // both final topologies: only those reach Relate's map insert)
                if !st.has_mgr {
                    return Err(bad("SetRelationships needs the manager"));
                }
            }
            ScriptOp2::IntersectInsertRelate => {
                if st.ctor != 0 || !st.have_breps[0] || !st.have_breps[1] {
                    return Err(bad("IntersectInsertRelate needs both B-reps and the maps"));
                }
                // the full intersection upstream does not finish for a zero approximation
                // tolerance (HANG-25; negative and tiny positive ones return) or
                // coordinates whose squares overflow (HANG-16's class)
                if st.approx == 0.0 || st.block_max_abs >= HANG_COORD {
                    return Err(bad(
                        "IntersectInsertRelate needs a nonzero tolerance and coordinates below 1e154",
                    ));
                }
                // time budget, not UB: a large tolerance finishes, but slowly
                if st.approx.is_nan() || st.approx.abs() > IIR_MAX_APPROX {
                    return Err(bad(
                        "IntersectInsertRelate's time budget needs |tolerance| <= 0.1",
                    ));
                }
                st.closed = true;
            }
            ScriptOp2::FixupClassMates => {
                classification(rd, c)?;
                classification(rd, c)?;
            }
            ScriptOp2::FixupSplitVertexPair => {
                let index = rd.index(MAX_INTERVALS)?;
                let n1 = classification(rd, c)?;
                let n2 = classification(rd, c)?;
                if index >= n1 || index >= n2 {
                    return Err(bad("interval index outside a classification"));
                }
            }
            ScriptOp2::EdgeEdge => {
                c.list(rd, &[Edge], true)?;
                c.list(rd, &[Edge], true)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp2::EdgeFaceCoincidence => {
                c.list(rd, &[Vertex], true)?;
                c.list(rd, &[Edge], true)?;
                rd.flag()?;
                rd.flag()?;
            }
            ScriptOp2::CloseIntersectionLoop => {
                // (BrepCurveSolve and the edge lists read both B-reps)
                if !st.have_breps[0] || !st.have_breps[1] {
                    return Err(bad("CloseIntersectionLoop needs both B-reps"));
                }
                c.handle_of(rd, &[Vertex], true)?;
            }
        }
        if op.mutates() {
            st.mutated = true;
        }
        Ok(true)
    }
}

/// Runs `op` on `input` (see the module docs and part 1's); `paths` are the `.smb`
/// files of the two file B-rep blocks (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, paths: [&str; 2], input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let entry: Entry = sys::sc_boolean_topology_intersector_p2_eval;
    eval_with(entry, Some(&Part2), op, paths, input)
}
