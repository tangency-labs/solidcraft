//! FFI wrappers for unit `topology.brep_p3`.
//!
//! Shim: `harness/cpp/shim/topology_brep_p3.cpp` (`sc_topology_brep_p3_eval`).
//! Upstream: `source/SMLib/src/SmBrep.cpp`, `source/SM_API/src/SmApiQueries.cpp`.
//!
//! Except for `Construct` and `ApiCopyNull`, every input is a B-rep block of
//! [`super::topology_vertexuse`] (kinds `0..=3`), then `editing` (`-1` keep, `0` / `1`
//! set `m_bEditingEnabled`), then the op's parameters: the head of
//! [`super::topology_brep_p2`], whose shim builds the same B-rep, so its `Export`
//! gives the entity counts of the B-rep these ops run on (on its real geometry,
//! analytic kinds included). Indices name entities in that export's order. Every output starts with `0`, or `-1 status` when the B-rep
//! could not be built. `counts` is `faces edges vertices regions` of the B-rep after
//! the op (a fresh walk), then per region (list order) `void infinite nshells` and per
//! shell of it `type size has_head` (`m_tShellType`, `GetSize`, `m_pList != NULL`),
//! then the topology graph (the shim's `put_graph`: the ten kind counts of a fresh
//! fixture walk, then every object's owner / adjacency links as walk indices).
//! `tracker` follows the outputs of the edits (`DeleteFace`, `Wire`, `Orient`,
//! `Squeeze`, `TurnNurbs`, `TurnNurbsOffset`), which run under
//! `SmTrackTopologyChanges` on the B-rep: `n rm*`, `n add*`, `n chg*`, then `n` events
//! of `action caller data1 data2 data3`, each object as a pair (the shim's
//! `put_tracked`: by its saved address, never read).
//!
//! | [`Op`]          | parameters | outputs |
//! |-----------------|------------|---------|
//! | `Construct`     | `mse lsr` (no B-rep block) | `0 0 tol mse lsr editing infinite_void counts` (`SmBrep(mse, lsr)` in a context of its own) |
//! | `Copy`          | `analytics api` | `status tol editing counts` of the copy (`api`: `SmApiBrepCopy`, else the copy constructor with `analytics`) |
//! | `Merge`         | `analytics` | `status counts` (`MergeBrep` of the B-rep into a second build of it) |
//! | `AttributedCopy` | `behavior` (`SmAttributeBehaviorType`, 0 to 4) | `status tol editing counts`, then the attribute rows of the copy and of the source (`SmApiBrepCopy` after every face, `GetFaces` order, and edge, `GetEdges` order, got a long attribute, id = value = `10007 + 500 kind + index`; per face then per edge, sorted: `n` and per attribute `id behavior value users`, users counted within that B-rep) |
//! | `AttributedMerge` | `analytics behavior` | `status counts` of the second build, then its and the source's attribute rows (the B-rep tagged as for `AttributedCopy`, then `MergeBrep` into a second build of it) |
//! | `CopyVertices`  | `n v*` | `status new counts`, then four `n xyz*` snapshots (source/destination before, source/existing destination after), then `n (destination_walk_index xyz)*` in returned order (`CopyVertices` into a second build) |
//! | `DeleteFace`    | `face delete_ev nesting keep_region` (`-1` null) | `status deleted counts tracker` |
//! | `Wire`          | `which region v1 v2 curve t0 t1 orient end(3)` (`0` `MakeWireEdge`, `1` `MakeWireEdgeVertex`; region `-1` null) | `status counts tracker` |
//! | `Orient`        | `outward unify n face*` (`n = -1`: no list) | `status maybe_not_closed`, per face `u0 v0 u1 v1 upward_faceuse`, `counts tracker` |
//! | `Squeeze`       | `edge which v` (survivor: `0` `GetVertex`, `1` the other vertex, `2` vertex `v`) | `status counts tracker` |
//! | `TurnNurbs`     |            | `status`, per edge its curve block (the fixture's `put_curve`), per face its surface type, `counts tracker` |
//! | `ApiCopyNull`   | `0` (no B-rep block) | `0 status` (`SmApiBrepCopy(NULL, ..)`) |
//! | `TurnNurbsOffset` | `d` | `TurnNurbs` after wrapping every face-owned B-spline kind surface (detached from its face first) in an owning `SmOffsetSurface(d, ..)`; per face also its surface's natural UV domain (before `counts tracker`) |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: valid
//! B-rep blocks, indices inside the B-rep (it exports the B-rep once more to count),
//! a null region only when the B-rep has an infinite region (`Wire`), the vertices
//! the wire makers dereference, a 3D wire curve over a sub-interval of its range
//! (`t0 < t1`), finite values no larger than [`MAX_GEOMETRY`] (larger geometry hangs
//! the C++ in `SmCurve::GlobalCoincidenceChecker`, HANG-16).

use crate::{call_with_buffer, FfiError};

use super::topology_brep_p2 as p2;
use super::topology_vertexuse as vu;

/// Largest magnitude of any input value (HANG-16).
pub const MAX_GEOMETRY: f64 = 1.0e150;

/// Largest offset distance of a `TurnNurbsOffset` (a harness bound on the generated
/// inputs: `ApproximateOffsetSurface` is defined for any finite distance).
pub const MAX_OFFSET: f64 = 1.0e3;

/// Largest vertex list of a `CopyVertices` or face list of an `Orient`.
pub const MAX_LIST: usize = 32;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Construct = 2,
    Copy = 3,
    Merge = 4,
    CopyVertices = 5,
    DeleteFace = 6,
    Wire = 7,
    Orient = 8,
    Squeeze = 9,
    TurnNurbs = 10,
    ApiCopyNull = 11,
    TurnNurbsOffset = 12,
    AttributedCopy = 13,
    AttributedMerge = 14,
}

mod sys {
    extern "C" {
        pub fn sc_topology_brep_p3_eval(
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

crate::reader::unit_reader!(vu::MAX_COORD);

impl Reader<'_> {
    /// An index below `count` (as `topology_vertexuse`'s reader).
    fn below(&mut self, count: usize) -> Result<usize, FfiError> {
        let i = self.index(usize::MAX >> 12)?;
        if i >= count {
            return Err(bad(format!("index {i} out of range ({count})")));
        }
        Ok(i)
    }
}

fn int(rd: &mut Reader<'_>, lo: i64, hi: i64) -> Result<i64, FfiError> {
    let v = rd.num()?;
    if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
        return Err(bad(format!("bad integer {v} ({lo}..={hi})")));
    }
    Ok(v as i64)
}

/// `-1` or an index below `n`.
fn opt_below(rd: &mut Reader<'_>, n: usize) -> Result<Option<usize>, FfiError> {
    let v = int(rd, -1, i64::from(u32::MAX))?;
    if v < 0 {
        return Ok(None);
    }
    let v = v as usize;
    if v >= n {
        return Err(bad(format!("index {v} out of range ({n})")));
    }
    Ok(Some(v))
}

fn args(op: Op, rd: &mut Reader<'_>, c: &[usize; 10], has_infinite: bool) -> Result<(), FfiError> {
    let [n_r, _, _, n_f, _, _, _, n_e, _, n_v] = *c;
    match op {
        Op::Construct | Op::ApiCopyNull | Op::TurnNurbs => {}
        Op::TurnNurbsOffset => {
            let d = rd.finite()?;
            if d.abs() > MAX_OFFSET {
                return Err(bad(format!("offset distance {d} beyond {MAX_OFFSET}")));
            }
        }
        Op::Copy => {
            rd.flag()?;
            rd.flag()?;
        }
        Op::Merge => {
            rd.flag()?;
        }
        Op::AttributedCopy => {
            int(rd, 0, 4)?;
        }
        Op::AttributedMerge => {
            rd.flag()?;
            int(rd, 0, 4)?;
        }
        Op::CopyVertices => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(n_v)?;
            }
        }
        Op::DeleteFace => {
            rd.below(n_f)?;
            rd.flag()?;
            rd.flag()?;
            opt_below(rd, n_r)?;
        }
        Op::Wire => {
            let which = rd.index(1)?;
            if opt_below(rd, n_r)?.is_none() && !has_infinite {
                return Err(bad("null region without infinite region"));
            }
            rd.below(n_v)?;
            let v2 = opt_below(rd, n_v)?;
            if which == 0 && v2.is_none() {
                return Err(bad("MakeWireEdge needs an end vertex"));
            }
            if rd.index(3)? != 3 {
                return Err(bad("wire curve must be 3D"));
            }
            rd.at -= 1;
            let ((a, b), at) = vu::curve_at(rd.input, rd.at)?;
            rd.at = at;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            if !(a <= t0 && t0 < t1 && t1 <= b) {
                return Err(bad("wire interval outside the curve's"));
            }
            rd.index(2)?;
            rd.vec3()?;
        }
        Op::Orient => {
            rd.flag()?;
            rd.flag()?;
            let n = int(rd, -1, MAX_LIST as i64)?;
            for _ in 0..n.max(0) {
                rd.below(n_f)?;
            }
        }
        Op::Squeeze => {
            rd.below(n_e)?;
            rd.index(2)?;
            opt_below(rd, n_v)?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by
        // `eval`); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_topology_brep_p3_eval(
                op as i32,
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

/// The checks of [`eval`] that need no B-rep: the standalone ops' whole input (then
/// `None`: nothing is left to check), else the B-rep block, the editing flag and the
/// HANG-16 magnitude bound, returning the reader at the op's arguments and the head
/// (the B-rep block and flags) for the export.
fn check_head<'a>(
    op: Op,
    path: &str,
    input: &'a [f64],
) -> Result<Option<(Reader<'a>, &'a [f64])>, FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::Construct => {
            rd.finite()?;
            rd.finite()?;
            rd.done()?;
            return Ok(None);
        }
        Op::ApiCopyNull => {
            int(&mut rd, 0, 0)?;
            rd.done()?;
            return Ok(None);
        }
        _ => {}
    }
    rd.at = vu::brep_block_at(input, path)?;
    int(&mut rd, -1, 1)?;
    let head = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    // geometry beyond 1e150 overflows the squared distances of
    // SmCurve::GlobalCoincidenceChecker, reached by the UV trim curve builds of
    // OrientTrimmedSurfaces / TurnToNURBS, which then never returns (HANG-16, the
    // bound blend.fillet_geom_p1 uses; main's HANG-10)
    // (`-f64::MAX` is the non-rational weight `NOW`)
    if input
        .iter()
        .any(|&x| x.is_finite() && x.abs() > MAX_GEOMETRY && x != -f64::MAX)
    {
        return Err(bad("geometry value beyond 1e150 (HANG-16)"));
    }
    Ok(Some((rd, head)))
}

/// The op's arguments after the head, checked against the B-rep's entity counts
/// `counts` (`topology_vertexuse::counts` order) and whether it has an infinite region,
/// then nothing may be left.
fn check_args(
    op: Op,
    rd: &mut Reader<'_>,
    counts: &[usize; 10],
    has_infinite: bool,
) -> Result<(), FfiError> {
    args(op, rd, counts, has_infinite)?;
    rd.done()
}

/// The checks [`eval`] makes before calling the shim, without running it or exporting
/// the B-rep: `counts` and `has_infinite` describe the B-rep the caller built from the
/// same input (the export's counts in `eval`; the runner's Rust side passes its own
/// B-rep's). The runner applies them to the Rust side, so an input the FFI refuses is a
/// harness error on both sides and the Rust port never runs the guarded hang regimes: a
/// `TurnNurbsOffset` of `1e300` on `box_with_large_fillet.smb` (nightly fuzz case
/// `fuzz00282`) is refused here for the C++ (HANG-16) while the port, faithful to
/// upstream, spent 25 s rebuilding the face's UV trim curves on the approximated offset
/// surface and exceeded the fuzz time limit.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input, as [`eval`].
pub fn validate(
    op: Op,
    path: &str,
    input: &[f64],
    counts: &[usize; 10],
    has_infinite: bool,
) -> Result<(), FfiError> {
    match check_head(op, path, input)? {
        None => Ok(()),
        Some((mut rd, _)) => check_args(op, &mut rd, counts, has_infinite),
    }
}

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let Some((mut rd, head)) = check_head(op, path, input)? else {
        return raw(op, path, input);
    };
    let export = p2::eval(p2::Op::Export, path, head)?;
    let Some(c) = vu::counts(&export) else {
        // the B-rep could not be built: every op reports the same failure
        return Ok(export);
    };
    let extras = p2::eval(p2::Op::Extras, path, head)?;
    let has_infinite = extras.get(2).is_some_and(|&r| r >= 0.0);
    check_args(op, &mut rd, &c, has_infinite)?;
    raw(op, path, input)
}
