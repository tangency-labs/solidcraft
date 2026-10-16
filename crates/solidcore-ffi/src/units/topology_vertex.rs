//! FFI wrappers for unit `topology.vertex`.
//!
//! Shim: `harness/cpp/shim/topology_vertex.cpp` (`sc_topology_vertex_eval`).
//! Upstream: `source/SMLib/src/SmVertex.cpp`.
//!
//! Every op starts with a B-rep block (the shim builds a fresh `SmBrep` per call and
//! deletes it after), as in [`super::topology_vertexuse`]:
//!
//! | block                                         | B-rep                                     |
//! |-----------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `1 curve t0 t1 validate_only init_gaps`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)`   |
//! | `3 validate_only init_gaps`                   | the wire network of `prog_test` `my_test_topology` |
//! | `4 closed n segments validate_only init_gaps` | a chain of wire edges (`MakeShellVertex`, `MakeWireEdgeVertex`, `MakeWireEdge`) |
//!
//! A chain segment is `0 a[3] b[3] smline has_ivl t0 t1 rev` (a line segment from `a`
//! to `b`: `SmLine::CreateLineSegment`, re-parameterised to `[t0, t1]` when `has_ivl`,
//! or `SmBSplineCurve::CreateLineSegment`) or `1 curve rev` (a B-spline curve block);
//! `rev` makes the edge run against the curve.
//!
//! A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (`SmBSplineSurface`) or
//! `1 O[3] N[3]` (`SmPlane(O, N)`); a curve block `dim n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`). Indices refer to the export's traversal order (`-1` selects null
//! where upstream takes a pointer). Every op's output starts with `0`, or with
//! `-1 status` when the B-rep could not be built (nothing else follows). `-2` in an
//! output is "upstream would dereference null or read an uninitialised value" (the shim
//! did not call it). A list is `count ids...`; "gap states" are the vertex/edge and
//! vertex/face cache states of every vertex use, in order; a gap is `kind vu length`
//! (`kind` 0 vertex/edge, 1 vertex/face; `-1` alone for none).
//!
//! | [`Op`]              | parameters after the block | outputs |
//! |---------------------|----------------------------|---------|
//! | `Export`            |                            | the graph (see [`counts`] and the difftest runner) |
//! | `Query`             | `v`                        | `type kind_of*9 primary_vu vertexuses brep_is_this shell loop wire lamina edges faces faceuses shells regions` |
//! | `Connected`         | `v type index`             | `connected` |
//! | `VuOfFace`          | `v face`                   | `vu` |
//! | `VusOfFace`         | `v face type index`        | the vertex use list |
//! | `VusOfEdge`         | `v edge type index`        | the vertex use list |
//! | `CommonEdges`       | `v other face`             | `GetEdges(face)` list, then the common edge list |
//! | `ShellInRegion`     | `v region`                 | `status shell` |
//! | `Tangent`           | `v e1 e2 angle_tol_deg`    | `tangent` |
//! | `Topological`       | `v`                        | `topological`, then the gap states |
//! | `OnPole`            | `v`                        | `on_pole`, then the gap states |
//! | `Gaps`              | `v want_array has_tol tol` | `0 max_gap (n gaps... or -1)`, then the gap states |
//! | `Notify`            | `v operation`              | the gap states |
//! | `SetPoint`          | `v x y z`                  | `point[3]`, then the gap states |
//! | `SetTolerance`      | `v tol only_if_larger cascade` | `tolerance` |
//! | `RefreshTolerance`  | `v nested`                 | `status tolerance edge_tolerances...` |
//! | `Exact`             | `v exact poly`             | `is_exact has_poly_vertex` |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, planes with a non-zero normal, indices
//! inside the B-rep's export (it builds the B-rep once more to count), finite inputs.

use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;

const NOW: f64 = -f64::MAX;
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    Query = 1,
    Connected = 2,
    VuOfFace = 3,
    VusOfFace = 4,
    VusOfEdge = 5,
    CommonEdges = 6,
    ShellInRegion = 7,
    Tangent = 8,
    Topological = 9,
    OnPole = 10,
    Gaps = 11,
    Notify = 12,
    SetPoint = 13,
    SetTolerance = 14,
    RefreshTolerance = 15,
    Exact = 16,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_vertex_eval(
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

/// Sequential reader over the input.
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

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks a clamped knot vector of degree `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(f64, f64), FfiError> {
    let m = knots
        .len()
        .checked_sub(1)
        .ok_or_else(|| bad(format!("{what}: no knots")))?;
    if knots.windows(2).any(|w| w.first() > w.last()) {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad(format!("{what}: empty knots")));
    };
    let short = || bad(format!("{what}: knots too short"));
    if m < 2 * p + 1 {
        return Err(short());
    }
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots.get(m - p..).ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above degree + 1")));
    }
    let mut run = 1;
    for w in inner.windows(2) {
        run = if w.first() == w.last() { run + 1 } else { 1 };
        if run > p {
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok((a, b))
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

/// Largest number of chain segments.
pub const MAX_SEGMENTS: usize = 8;

/// A chain block: `closed n` then `n` segments (see the module docs).
fn chain(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.flag()?;
    let n = rd.index(MAX_SEGMENTS)?;
    if n == 0 {
        return Err(bad("chain: no segment"));
    }
    for _ in 0..n {
        if rd.index(1)? == 0 {
            // coincident end points fail softly upstream (SAME_PNT_ER / Unitize)
            rd.vec3()?;
            rd.vec3()?;
            rd.flag()?;
            rd.flag()?;
            let t0 = rd.finite()?;
            let t1 = rd.finite()?;
            if t0 >= t1 || t1 - t0 < 1e-9 * t0.abs().max(t1.abs()).max(1.0) {
                return Err(bad("chain: bad line interval"));
            }
        } else {
            curve(rd)?;
        }
        rd.flag()?;
    }
    Ok(())
}

/// A B-rep block.
fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    match rd.index(4)? {
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
        4 => chain(rd)?,
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

/// `SmTopoTypes.h` codes `IsConnectedTo` dispatches on, with the export slot of their
/// kind.
const TARGET_TYPES: [(i64, usize); 10] = [
    (16012, 0), // SmRegion_TYPE
    (16013, 1), // SmShell_TYPE
    (16022, 2), // SmFaceuse_TYPE
    (16002, 3), // SmFace_TYPE
    (16004, 4), // SmLoopuse_TYPE
    (16003, 5), // SmLoop_TYPE
    (16005, 6), // SmEdgeuse_TYPE
    (16016, 7), // SmEdge_TYPE
    (16006, 8), // SmVertexuse_TYPE
    (16021, 9), // SmVertex_TYPE
];

/// Largest `SmNotifyOperation` value accepted (`SM_NO_UNKNOWN`).
pub const MAX_NOTIFY: usize = 16;

/// An optional index below `count` (`-1` for null).
fn opt_below(rd: &mut Reader<'_>, count: usize) -> Result<(), FfiError> {
    let i = rd.num()?;
    if i == -1.0 {
        return Ok(());
    }
    if i.fract() != 0.0 || i < 0.0 || i >= count as f64 {
        return Err(bad(format!("index {i} out of range ({count})")));
    }
    Ok(())
}

/// A target block `type index` of `IsConnectedTo`.
fn target(rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let t = rd.num()?;
    let i = rd.num()?;
    if i.fract() != 0.0 || i < -1.0 {
        return Err(bad("bad target index"));
    }
    if let Some(&(_, slot)) = TARGET_TYPES.iter().find(|(code, _)| *code as f64 == t) {
        let n = c.get(slot).copied().unwrap_or(0);
        if i >= 0.0 && i as usize >= n {
            return Err(bad("target index out of range"));
        }
    } else if t.fract() != 0.0 || !(0.0..=1.0e6).contains(&t) {
        return Err(bad("bad target type"));
    }
    Ok(())
}

/// Checks the arguments after the B-rep block against the B-rep's counts.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [n_r, _, _, n_f, _, _, _, n_e, _, n_v] = c;
    if op == Op::Export {
        return Ok(());
    }
    rd.below(n_v)?;
    match op {
        Op::Export | Op::Query | Op::Topological | Op::OnPole => {}
        Op::Connected => target(rd, c)?,
        Op::VuOfFace => opt_below(rd, n_f)?,
        Op::VusOfFace => {
            opt_below(rd, n_f)?;
            target(rd, c)?;
        }
        Op::VusOfEdge => {
            opt_below(rd, n_e)?;
            target(rd, c)?;
        }
        Op::CommonEdges => {
            opt_below(rd, n_v)?;
            opt_below(rd, n_f)?;
        }
        Op::ShellInRegion => opt_below(rd, n_r)?,
        Op::Tangent => {
            rd.below(n_e)?;
            rd.below(n_e)?;
            rd.finite()?;
        }
        Op::Gaps => {
            rd.flag()?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::Notify => {
            rd.index(MAX_NOTIFY)?;
        }
        Op::SetPoint => {
            rd.vec3()?;
        }
        Op::SetTolerance => {
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::RefreshTolerance => {
            rd.flag()?;
        }
        Op::Exact => {
            rd.flag()?;
            rd.flag()?;
        }
    }
    Ok(())
}

fn raw(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let cpath = std::ffi::CString::new(path).map_err(|_| bad("path has a NUL byte"))?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: cpath is a valid NUL-terminated string; input is valid for
        // input.len() reads and holds what the shim reads for `op` (validated by the
        // callers); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_topology_vertex_eval(
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
/// [`FfiError::InvalidArgument`] for a malformed input; [`FfiError::Status`] for a shim
/// failure.
pub fn eval(op: Op, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let mut rd = Reader { input, at: 0 };
    brep_block(&mut rd, path)?;
    let block = input.get(..rd.at).ok_or_else(|| bad("input too short"))?;
    let export = raw(Op::Export, path, block)?;
    if op == Op::Export {
        rd.done()?;
        return Ok(export);
    }
    let Some(c) = counts(&export) else {
        // the B-rep could not be built: the op reports the same failure
        return Ok(export);
    };
    brep_args(op, &mut rd, c)?;
    rd.done()?;
    raw(op, path, input)
}
