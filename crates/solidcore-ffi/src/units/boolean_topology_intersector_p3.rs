//! FFI wrappers for unit `boolean.topology_intersector_p3`.
//!
//! Shim: `harness/cpp/shim/boolean_topology_intersector_p3.cpp`
//! (`sc_boolean_topology_intersector_p3_eval`). Upstream:
//! `source/SMLib/src/SmTopologyIntersector.cpp` (`MergeCurveClasses`,
//! `MergeCurveOnSurfaces`, `MergeCoincidentSurfaces`).
//!
//! Every call starts with the two B-rep blocks of [`super::boolean_topology_intersector_p1`]
//! (validated by its [`blocks_len`](super::boolean_topology_intersector_p1::blocks_len));
//! the output starts as that unit's (`0`, or `-1 status` / `-3 status` for a B-rep that
//! could not be built). Handles are its `brep kind index` triples: `kind` 0 vertex, 1
//! edge, 2 face, 3 an edge's curve, 4 a face's surface, 6 a B-rep, 8 null.
//!
//! | [`Op`]   | after the blocks | outputs |
//! |----------|------------------|---------|
//! | `Export` |                  | as the p1 unit's `Export` |
//! | `VertexUses` |              | per B-rep `-1` (absent) or `n_vertices (n vertexuse*)*`: each vertex's uses in its list order, as export indices (`-1` for a use the export's walk does not register; the shared export lists them in arena order) |
//! | `Run`    | `approx ang doing_fillets ignore_tangency n (h h)* merge args` | `status*` (one per pre-relation; `-2` for a null first object) `merge` then the merge's outputs |
//!
//! The intersector is made with both B-reps and the tolerances; `doing_fillets` and
//! `ignore_tangency` set its public `m_bDoingFillets` / `m_bIgnoreTangency` (`doing_fillets`
//! must be `0`: the fillet path needs a fillet context, without which upstream crashes);
//! the `n` pairs are `Relate`d before the merge.
//!
//! | merge | name | args | outputs |
//! |-------|------|------|---------|
//! | 1 | `MergeCurveOnSurfaces` | `h(surface) h(surface) tol h(curve) h(edge) h(edge) h(uv) h(uv)` (`h(uv)`: an edge use's UV trim curve, kind 13, or null) | `status deleted n h* n h*` then the state |
//! | 2 | `MergeCurveClasses` | `merge_singularities class class` | `status deleted n h* n h* class class` then the state |
//! | 3 | `MergeCoincidentSurfaces` | `h(surface) h(surface)` | `status n h* n h*` then the state |
//!
//! A `class` input is `h(curve) t0 t1 tol pc pc pc` (`pc` = `class h`: the start, mid and
//! end classifications of the single interval of `SmCurveClassification(curve, [t0, t1],
//! NULL, tol)`); a `class` output is `n (t0 t1 pc pc pc)*`. The state is, per B-rep,
//! `n_vertices n_edges n_faces` then every vertex and edge tolerance in traversal order
//! (re-walked after the merge), then for vertices, edges and faces the mapped B-rep
//! objects `n (h h(other mate))*` sorted by handle. Deleted objects are no longer in the
//! graph and read `-1 -3 -1`. `-2` instead of the merge's status means upstream would
//! dereference null (the shim did not call it) and nothing follows.

use std::ffi::CString;

use crate::units::boolean_topology_intersector_p1::{blocks_len, counts, split_exports};
use crate::{call_with_buffer, FfiError};

/// Largest number of pre-relations.
pub const MAX_RELATES: usize = 64;
/// Largest finite input magnitude.
pub const MAX_COORD: f64 = 1.0e300;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Run = 1,
    VertexUses = 2,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

/// The merge ops of `Run`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Merge {
    CurveOnSurfaces = 1,
    CurveClasses = 2,
    CoincidentSurfaces = 3,
}

mod sys {
    extern "C" {
        pub fn sc_boolean_topology_intersector_p3_eval(
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

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input after the B-rep blocks.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn int(&mut self, lo: i64, hi: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad integer {v} (expected {lo}..={hi})")));
        }
        Ok(v as i64)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Handle kinds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Kind {
    Vertex,
    Edge,
    Face,
    EdgeCurve,
    FaceSurface,
    Brep,
    Null,
    /// An edge use's UV trim curve (`GetUVTrimCurve`), by edge use index.
    TrimCurve,
}

/// Per B-rep `[vertices, edges, faces, edge uses]` (`None` for an absent one).
struct Counts([Option<[usize; 4]>; 2]);

impl Counts {
    /// A handle triple of one of the `allowed` kinds (or null when `allow_null`).
    fn handle(
        &self,
        rd: &mut Reader<'_>,
        allowed: &[Kind],
        allow_null: bool,
    ) -> Result<Kind, FfiError> {
        let b = rd.int(-1, 1)?;
        let kind = match rd.int(-1, 13)? {
            0 => Kind::Vertex,
            1 => Kind::Edge,
            2 => Kind::Face,
            3 => Kind::EdgeCurve,
            4 => Kind::FaceSurface,
            6 => Kind::Brep,
            8 => Kind::Null,
            13 => Kind::TrimCurve,
            _ => return Err(bad("bad handle kind")),
        };
        let i = rd.int(-1, i64::from(u32::MAX))?;
        if kind == Kind::Null {
            if !allow_null {
                return Err(bad("null handle not allowed here"));
            }
            return Ok(kind);
        }
        if !allowed.contains(&kind) {
            return Err(bad("handle of the wrong kind"));
        }
        if kind == Kind::Brep {
            let present = usize::try_from(i)
                .ok()
                .and_then(|i| self.0.get(i))
                .copied()
                .flatten()
                .is_some();
            if !present {
                return Err(bad("B-rep handle out of range"));
            }
            return Ok(kind);
        }
        let c = usize::try_from(b)
            .ok()
            .and_then(|b| self.0.get(b))
            .copied()
            .flatten()
            .ok_or_else(|| bad("handle names an absent B-rep"))?;
        let n = match kind {
            Kind::Vertex => c[0],
            Kind::Edge | Kind::EdgeCurve => c[1],
            Kind::TrimCurve => c[3],
            _ => c[2],
        };
        if usize::try_from(i).map_or(true, |i| i >= n) {
            return Err(bad("handle index out of range"));
        }
        Ok(kind)
    }

    /// A point classification `class h` (any topology object or null; the shim refuses
    /// an object of the wrong kind for its class).
    fn point_class(&self, rd: &mut Reader<'_>) -> Result<(), FfiError> {
        rd.int(100, 110)?; // SmPointClassificationType
        self.handle(rd, &[Kind::Vertex, Kind::Edge, Kind::Face], true)?;
        Ok(())
    }

    /// A classification `h(curve) t0 t1 tol pc pc pc`.
    fn class(&self, rd: &mut Reader<'_>) -> Result<(), FfiError> {
        self.handle(rd, &[Kind::EdgeCurve], true)?;
        let t0 = rd.finite()?;
        let t1 = rd.finite()?;
        if t0 > t1 {
            return Err(bad("classification interval not ordered"));
        }
        let tol = rd.finite()?;
        if tol < 0.0 {
            return Err(bad("negative classification tolerance"));
        }
        for _ in 0..3 {
            self.point_class(rd)?;
        }
        Ok(())
    }
}

/// Checks the `Run` input after the B-rep blocks.
fn run_args(rd: &mut Reader<'_>, c: &Counts) -> Result<(), FfiError> {
    use Kind::{Edge, EdgeCurve, Face, FaceSurface, TrimCurve, Vertex};
    let approx = rd.finite()?;
    let ang = rd.finite()?;
    if approx < 0.0 || ang < 0.0 {
        return Err(bad("negative tolerance"));
    }
    // m_bDoingFillets: the fillet path gives the classifications fillet data that the merge
    // reads as an SmFilletGeom context (set only by the fillet code); without one upstream
    // crashes, so the harness runs without it
    if rd.flag()? {
        return Err(bad("doing_fillets needs a fillet context; not supported"));
    }
    rd.flag()?;
    let n = rd.index(MAX_RELATES)?;
    for _ in 0..n {
        c.handle(rd, &[Vertex, Edge, Face], true)?;
        c.handle(rd, &[Vertex, Edge, Face], true)?;
    }
    match rd.int(1, 3)? {
        1 => {
            c.handle(rd, &[FaceSurface], true)?;
            c.handle(rd, &[FaceSurface], true)?;
            let tol = rd.finite()?;
            if tol < 0.0 {
                return Err(bad("negative curve tolerance"));
            }
            c.handle(rd, &[EdgeCurve], true)?;
            c.handle(rd, &[Edge], true)?;
            c.handle(rd, &[Edge], true)?;
            c.handle(rd, &[TrimCurve], true)?;
            c.handle(rd, &[TrimCurve], true)?;
        }
        2 => {
            rd.flag()?;
            c.class(rd)?;
            c.class(rd)?;
        }
        _ => {
            c.handle(rd, &[FaceSurface], true)?;
            c.handle(rd, &[FaceSurface], true)?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the capacity;
        // len is valid.
        unsafe {
            sys::sc_boolean_topology_intersector_p3_eval(
                op.code(),
                cpath.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Runs `op` on `input` (see the module docs); `paths` are the `.smb` files of the two
/// file B-rep blocks (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, paths: [&str; 2], input: &[f64]) -> Result<Vec<f64>, FfiError> {
    if paths.iter().any(|p| p.contains('\n')) {
        return Err(bad("path with a newline"));
    }
    let joined = format!("{}\n{}", paths[0], paths[1]);
    let n_blocks = blocks_len(paths, input)?;
    let block = input
        .get(..n_blocks)
        .ok_or_else(|| bad("input too short"))?;
    let export = raw(Op::Export, &joined, block)?;
    if op == Op::Export || op == Op::VertexUses {
        if n_blocks != input.len() {
            return Err(bad("trailing input"));
        }
        return if op == Op::Export {
            Ok(export)
        } else {
            raw(op, &joined, input)
        };
    }
    let Some((a, b)) = split_exports(&export) else {
        // a B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    // the export's counts in graph order: faces 3, edge uses 6, edges 7, vertices 9
    let count4 = |ex: &[f64]| counts(ex).map(|c| [c[9], c[7], c[3], c[6]]);
    let c = Counts([count4(&a), b.as_deref().and_then(count4)]);
    if c.0[0].is_none() {
        return Err(bad("first export unreadable"));
    }
    let rest = input
        .get(n_blocks..)
        .ok_or_else(|| bad("input too short"))?;
    let mut rd = Reader { input: rest, at: 0 };
    run_args(&mut rd, &c)?;
    rd.done()?;
    raw(op, &joined, input)
}
