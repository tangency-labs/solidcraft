//! FFI wrappers for unit `topology.vertexuse`.
//!
//! Shim: `harness/cpp/shim/topology_vertexuse.cpp` (`sc_topology_vertexuse_eval`).
//! Upstream: `source/SMLib/src/SmVertexuse.cpp`.
//!
//! Every op starts with a B-rep block (the shim builds a fresh `SmBrep` per call and
//! deletes it after), as in [`super::topology_edgeuse_p1`]:
//!
//! | block                                         | B-rep                                     |
//! |-----------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `1 curve t0 t1 validate_only init_gaps`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)`   |
//! | `3 validate_only init_gaps`                   | the wire network of `prog_test` `my_test_topology` |
//! | `5 np P[3]*np ne (a b has_curve [curve])*ne validate_only init_gaps` | `MakeShellVertex` at each point, then `MakeEdgeTopology` edges (a line, or the curve) as in the wire network |
//!
//! A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (`SmBSplineSurface`) or
//! `1 O[3] N[3]` (`SmPlane(O, N)`); a curve block `dim n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`). Indices refer to the export's traversal order. Every op's output
//! starts with `0`, or with `-1 status` when the B-rep could not be built (nothing else
//! follows). `-2` in an output is "upstream would dereference null" (the shim did not
//! call it). "gap states" are the vertex/edge and vertex/face cache states of every
//! vertex use, in order. A face's surface in the graph export may start with an exact
//! analytic head, `2 type version surf_oob gen_oob nbytes byte*nbytes` (an
//! `SmSurfOfRevolution`: its binary `WriteToDB` record,
//! `harness/cpp/shim/sc_analytic_export.h`), followed by the regular surface block.
//!
//! | [`Op`]         | parameters after the block | outputs |
//! |----------------|----------------------------|---------|
//! | `Export`       |                            | the graph (see [`counts`] and the difftest runner) |
//! | `VuQuery`      | `vu`                       | `type vertex eu fu lu shell is_loop is_shell is_edge has_eg has_fg strut type_code kind_of*9 eg_init fg_init` |
//! | `Connected`    | `vu type index`            | `connected` |
//! | `Gaps`         | `vu which flag want_out`   | per `which` (`0` `GetVertexEdgeGap`, `1` `GetVertexFaceGap`, `2` `CalcVertexEdgeGap`, `3` `CalcVertexFaceGap`, `4` `InitVertexFaceGap`), then the gap states |
//! | `Notify`       | `vu operation`             | the gap states |
//! | `UvPoint`      | `vu ok_to_make`            | `status u v` |
//! | `UvSector`     | `vu ok_to_make`            | `status vu beg_eu end_eu beg_t end_t beg_uv end_uv beg_tan end_tan angle flags*6` |
//! | `MissingSeam`  | `vu want_u want_v uv_ok`   | `missing u v` |
//! | `OnPole`       | `vu`                       | `on_pole`, then the gap states |
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

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    VuQuery = 1,
    Connected = 2,
    Gaps = 3,
    Notify = 4,
    UvPoint = 5,
    UvSector = 6,
    MissingSeam = 7,
    OnPole = 8,
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
        pub fn sc_topology_vertexuse_eval(
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

/// [`surface`] for a wrapper with its own reader (`topology_brep_p2`): validates the
/// surface block of `input` at `at`; returns its domain and the position after it.
pub(crate) fn surface_at(input: &[f64], at: usize) -> Result<(Domain, usize), FfiError> {
    let mut rd = Reader::new(input);
    rd.at = at;
    let d = surface(&mut rd)?;
    Ok((d, rd.at))
}

/// [`curve`] for a wrapper with its own reader: validates the curve block of `input` at
/// `at`; returns its parameter range and the position after it.
pub(crate) fn curve_at(input: &[f64], at: usize) -> Result<((f64, f64), usize), FfiError> {
    let mut rd = Reader::new(input);
    rd.at = at;
    let r = curve(&mut rd)?;
    Ok((r, rd.at))
}

/// [`brep_block`] for a wrapper with its own reader: validates the B-rep block at the
/// start of `input`; returns the position after it.
pub(crate) fn brep_block_at(input: &[f64], path: &str) -> Result<usize, FfiError> {
    let mut rd = Reader::new(input);
    brep_block(&mut rd, path)?;
    Ok(rd.at)
}

/// A B-rep block.
fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    let kind = rd.index(5)?;
    match kind {
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
        2 | 4 => {
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
            if kind == 4 {
                // kind 4 adds an inner vertex loop at these fractions of the domain
                for _ in 0..2 {
                    let t = rd.finite()?;
                    if !(0.0..=1.0).contains(&t) {
                        return Err(bad("vertex loop fraction outside [0, 1]"));
                    }
                }
            }
        }
        5 => {
            // shell vertices, then edges between two distinct ones (a line or a curve)
            let np = rd.index(MAX_NETWORK_POINTS)?;
            for _ in 0..np {
                let p = rd.vec3()?;
                if p.iter().any(|c| c.abs() > 1e6) {
                    return Err(bad("network point out of range"));
                }
            }
            let ne = rd.index(MAX_NETWORK_EDGES)?;
            for _ in 0..ne {
                let a = rd.index(np.saturating_sub(1))?;
                let b = rd.index(np.saturating_sub(1))?;
                if np == 0 || a == b {
                    return Err(bad("network edge needs two distinct points"));
                }
                if rd.flag()? {
                    curve(rd)?;
                }
            }
        }
        _ => {}
    }
    rd.flag()?;
    rd.flag()?;
    Ok(())
}

/// Most points of a kind-5 network block.
pub const MAX_NETWORK_POINTS: usize = 64;
/// Most edges of a kind-5 network block.
pub const MAX_NETWORK_EDGES: usize = 128;

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

/// Checks the arguments after the B-rep block against the B-rep's counts.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, _, _, _, _, _, _, n_vu, _] = c;
    if op == Op::Export {
        return Ok(());
    }
    rd.below(n_vu)?;
    match op {
        Op::Export | Op::VuQuery | Op::OnPole => {}
        Op::Connected => {
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
        }
        Op::Gaps => {
            rd.index(4)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Notify => {
            rd.index(MAX_NOTIFY)?;
        }
        Op::UvPoint | Op::UvSector => {
            rd.flag()?;
        }
        Op::MissingSeam => {
            rd.flag()?;
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
            sys::sc_topology_vertexuse_eval(
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
    let mut rd = Reader::new(input);
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
