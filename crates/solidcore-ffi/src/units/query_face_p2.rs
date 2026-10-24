//! FFI wrappers for unit `query.face_p2`.
//!
//! Shim: `harness/cpp/shim/query_face_p2.cpp` (`sc_query_face_p2_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp` (`SmFace::PointClassify`,
//! `SmFace::CurveOnClassify`), `source/SMLib/src/SmProtoTopology.cpp`
//! (`sm_ClassifyParamOnFaces`, `SmProtoVertex::ContainsPoint`,
//! `GetCaptureDistance`), `source/SMLib/inc/SmProtoTopology.h` (`DistanceTo`).
//!
//! B-rep ops start with a B-rep block, as in [`super::topology_face_p2`] (the shim
//! builds a fresh `SmBrep` per call and deletes it after):
//!
//! | block                                         | B-rep                                     |
//! |-----------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `1 curve t0 t1 validate_only init_gaps`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)`   |
//! | `3 validate_only init_gaps`                   | the wire network of `prog_test` `my_test_topology` |
//!
//! then an edit block `n (kind sel value)*` (the entity is `sel` modulo the count of
//! its kind; `kind` `0` face, `1` edge, `2` vertex: its stored zone tolerance set to
//! `value`; `3` face: the UV trim curves of its edge uses built, `value` unused). Indices refer to the export's
//! traversal order. Every B-rep op's output starts with `0`, or with `-1 status` when
//! the B-rep could not be built (nothing else follows).
//!
//! A point classification block `pc` is `class kind index gap src_tol presnap tparam u
//! v param_set` (`kind` `1` face, `2` edge, `3` vertex, `0` none; `index` its export
//! index).
//!
//! | [`Op`]            | parameters after the blocks | outputs |
//! |-------------------|-----------------------------|---------|
//! | `Export`          |                             | the graph (see [`counts`]) |
//! | `PointClassify`   | `f u v src_tol do_bnd use_uv tgt_only has_loops n loop* pc_tol uv_ok cache_mode repeats` | `status has_trim_cache pc`; cached/repeated cases wrap each query in five cache flags before/after (present, trimmed, containment, modified, tree) |
//! | `ClassifyParam`   | `u v tol f1 dom1[4] f2 dom2[4]` | `which` |
//! | `CurveOnClassify` | `f dim P[3] Q[3] tol do_pc big_gaps check3d uv_ok` | `status modified n (t0 t1 pc pc pc)*`, or `-3 status` when the line could not be made |
//!
//! Geometry op (no B-rep block, no leading `0`):
//!
//! | [`Op`]        | parameters | outputs |
//! |---------------|------------|---------|
//! | `ProtoVertex` | `approx_tol capture override P[3] tol Q[3]` | `contains distance pv_capture mgr_capture` |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, planes with a non-zero normal, indices
//! inside the B-rep's export (it builds the B-rep once more to count), finite inputs.

use std::path::Path;

use crate::validate::{bad, check_knots, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Largest number of edits or target loops.
pub const MAX_LIST: usize = 64;
/// Largest tolerance an edit or tolerance argument may set.
pub const MAX_TOL: f64 = 1.0e3;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    PointClassify = 1,
    ClassifyParam = 2,
    CurveOnClassify = 3,
    ProtoVertex = 20,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// Whether the op starts with a B-rep block.
    #[must_use]
    pub fn is_brep(self) -> bool {
        self.code() < Self::ProtoVertex.code()
    }
}

mod sys {
    extern "C" {
        pub fn sc_query_face_p2_eval(
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

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
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

    /// An index below `count`.
    fn below(&mut self, count: usize) -> Result<usize, FfiError> {
        let i = self.index(usize::MAX >> 12)?;
        if i >= count {
            return Err(bad(format!("index {i} out of range ({count})")));
        }
        Ok(i)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }
}

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed for
/// curves when `planar`).
fn check_points(rd: &mut Reader<'_>, k: usize, planar: bool, what: &str) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        if z == NOZ && !planar {
            return Err(bad(format!("{what}: 2D data")));
        }
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad(format!("{what}: weight out of range")));
        }
        let z = if z == NOZ { 0.0 } else { z };
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad(format!("{what}: point out of range")));
        }
    }
    Ok(())
}

/// The parameter domain `[u0, v0, u1, v1]` of a surface block, `None` for a plane.
type Domain = Option<[f64; 4]>;

/// A surface block; returns its domain.
fn surface(rd: &mut Reader<'_>) -> Result<Domain, FfiError> {
    let kind = rd.index(1)?;
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok(None);
    }
    rd.flag()?;
    rd.index(11)?;
    let n = rd.index(MAX_INDEX)?;
    let m = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || q == 0 || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad(
            "surface: need 1 <= p <= n, 1 <= q <= m, r = n+p+1, s = m+q+1",
        ));
    }
    check_points(rd, (n + 1) * (m + 1), false, "surface")?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok(Some([u0, v0, u1, v1]))
}

/// A B-spline curve block; returns its parameter range.
fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve")
}

/// A B-rep block.
fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(3)? {
        0 => {
            let p = Path::new(path);
            if p.extension().and_then(|e| e.to_str()) != Some("smb") || !p.is_file() {
                return Err(bad("B-rep file: need an existing .smb file"));
            }
            rd.flag()?;
        }
        1 => {
            let (a, b) = curve(rd)?;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            if !(a <= t0 && t0 < t1 && t1 <= b) {
                return Err(bad("wire edge interval outside the curve's"));
            }
        }
        2 => {
            let dom = surface(rd)?;
            let d = rd.finites(4)?;
            let (Some(&a), Some(&b2), Some(&c), Some(&e)) =
                (d.first(), d.get(1), d.get(2), d.get(3))
            else {
                return Err(bad("domain too short"));
            };
            if a >= c || b2 >= e {
                return Err(bad("face domain not ordered"));
            }
            match dom {
                Some([u0, v0, u1, v1]) => {
                    if a < u0 || c > u1 || b2 < v0 || e > v1 {
                        return Err(bad("face domain outside the surface's"));
                    }
                }
                None => {
                    if d.iter().any(|x| x.abs() > 1e4) {
                        return Err(bad("plane face domain too large"));
                    }
                }
            }
        }
        _ => {}
    }
    rd.flag()?;
    rd.flag()?;
    Ok(())
}

/// The entity counts of an export, in the shim's order: regions, shells, face uses,
/// faces, loop uses, loops, edge uses, edges, vertex uses, vertices. `None` when the
/// export is a build failure.
#[must_use]
pub fn counts(export: &[f64]) -> Option<[usize; 10]> {
    if export.first() != Some(&0.0) {
        return None;
    }
    let mut c = [0usize; 10];
    for (slot, v) in c.iter_mut().zip(export.get(2..12)?) {
        *slot = *v as usize;
    }
    Some(c)
}

/// A tolerance argument: finite, `|t| <= MAX_TOL`.
fn tol(rd: &mut Reader<'_>) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    if t.abs() > MAX_TOL {
        return Err(bad("tolerance too large"));
    }
    Ok(t)
}

/// The edit block (selectors are taken modulo the entity count by the shim).
fn edits(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(MAX_LIST)?;
    for _ in 0..n {
        rd.index(3)?;
        rd.index(usize::MAX >> 12)?;
        let v = tol(rd)?;
        if v < 0.0 {
            return Err(bad("negative tolerance edit"));
        }
    }
    Ok(())
}

/// The parameters of a B-rep op after the blocks.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let n_faces = c[3];
    let n_loops = c[5];
    match op {
        Op::Export | Op::ProtoVertex => {}
        Op::PointClassify => {
            rd.below(n_faces)?;
            rd.finites(2)?;
            tol(rd)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(n_loops)?;
            }
            tol(rd)?;
            rd.flag()?;
            rd.index(2)?;
            let repeats = rd.index(16)?;
            if repeats == 0 {
                return Err(bad("zero point-classification queries"));
            }
        }
        Op::ClassifyParam => {
            rd.finites(2)?;
            tol(rd)?;
            for _ in 0..2 {
                rd.below(n_faces)?;
                rd.finites(4)?;
            }
        }
        Op::CurveOnClassify => {
            rd.below(n_faces)?;
            let dim = rd.index(3)?;
            if dim < 2 {
                return Err(bad("curve dimension must be 2 or 3"));
            }
            let p = rd.vec3()?;
            let q = rd.vec3()?;
            if p == q {
                return Err(bad("degenerate line"));
            }
            tol(rd)?;
            for _ in 0..4 {
                rd.flag()?;
            }
        }
    }
    Ok(())
}

/// The parameters of the geometry op.
fn geom_args(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let approx = rd.finite()?;
    if !(1e-8..=1.0).contains(&approx) {
        return Err(bad("approx tolerance outside [1e-8, 1]"));
    }
    tol(rd)?;
    tol(rd)?;
    rd.vec3()?;
    tol(rd)?;
    rd.vec3()?;
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(1 << 16, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_query_face_p2_eval(
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

/// Runs `op` on `input` (see the module docs); `path` is the `.smb` file of a file
/// B-rep block (ignored otherwise).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the wrapper rejects; the shim's status.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader::new(input);
    if !op.is_brep() {
        geom_args(&mut rd)?;
        rd.done()?;
        return raw(op, "", input);
    }
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    edits(&mut rd)?;
    if op == Op::Export {
        rd.done()?;
        return raw(op, path, input);
    }
    let mut unedited = block.to_vec();
    unedited.push(0.0);
    let export = raw(Op::Export, path, &unedited)?;
    let Some(c) = counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    brep_args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}
