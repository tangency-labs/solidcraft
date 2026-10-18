//! FFI wrappers for unit `query.edge`.
//!
//! Shim: `harness/cpp/shim/query_edge.cpp` (`sc_query_edge_eval`). Upstream:
//! `source/SMLib/src/SmEdge.cpp` (`CalculateBoundingBox`, `CalculateTightBoundingBox`,
//! `ClosestPoint`, `RadialSectorClassify`, `RadialSectorOfCurveClassify`).
//!
//! Every op starts with a B-rep block, then an edit block (the shim builds a fresh
//! `SmBrep` per call and deletes it after):
//!
//! | block                 | B-rep                                     |
//! |-----------------------|-------------------------------------------|
//! | `0 rebuild`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `2 surface u0 v0 u1 v1` | `CreateFaceFromSurface(surface, dom)`   |
//! | `4 curve t0 t1`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//!
//! The edit block `edge tol has0 P0[3] has1 P1[3] drop` changes edge `edge` (`-1`:
//! nothing): `SetTolerance(tol, FALSE, FALSE)` when `tol >= 0`, `SetPoint` of its start
//! / end vertex when `has0` / `has1`, `SetCurve(NULL, ...)` when `drop`. A surface block
//! is `0 oob form n m p q r s cpts(x y z w) u v` (`SmBSplineSurface`) or `1 O[3] N[3]`
//! (`SmPlane(O, N)`); a curve block `kind dim ...`: `0 dim oob n p m cpts(x y z w)
//! knots` (`SmBSplineCurve`), `1 dim P1 D1 P2 D2` (`SmHermiteCurve`), `2 3 P V d0 d1
//! scale io` (bounded `SmLine`), `3 3 C X Y d0 d1 rx ry io` (`SmEllipse`) or `4 3 C X Y
//! d0 d1 r io` (`SmCircle`). Indices refer to the export's traversal order (layout of
//! unit `topology.edgeuse_p1`'s export). Every output starts with `0`, or with `-1
//! status` when the B-rep could not be built (nothing else follows). An op output
//! `-2` is "upstream would dereference null" (the shim did not call it).
//!
//! | [`Op`]        | parameters after the edit block   | outputs |
//! |---------------|-----------------------------------|---------|
//! | `Export`      |                                   | the graph ([`counts`]) |
//! | `BBox`        | `edge normal pseudo polar tight`  | `status`, then on success the asked boxes: `min[3] max[3]`, `basis[9] ivl[6]`, `state basis[9] dom[4]` |
//! | `Tight`       | `edge`                            | `status`, then on success `min[3] max[3]` (of an `Init`ed box when not set) |
//! | `Closest`     | `edge P[3]`                       | `status`, then on success `found [t dist]` |
//! | `Radial`      | `sector_eu classify_eu nparam`    | `status`, then on success `result` |
//! | `RadialCurve` | `sector_eu t curve a b orient force` | `status`, then on success `result` |
//!
//! The radial ops classify around the sector edge use's edge; the shim refuses (`-2`)
//! when that edge has no curve, or the sector edge use, its radial partner or (for
//! `Radial`) the classified edge use is not a loop edge use with a face surface, and
//! `Closest` on an edge without curve.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines and analytic curves, planes with a
//! non-zero normal, intervals within the curve's range, indices inside the B-rep's
//! export (it builds the B-rep once more to count), finite bounded inputs, and no curve
//! drop before a radial op.

use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e12;

const NOW: f64 = -f64::MAX;
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    BBox = 1,
    Tight = 2,
    Closest = 3,
    Radial = 4,
    RadialCurve = 5,
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
        pub fn sc_query_edge_eval(
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

/// Checks `k` control points sharing their rational marker (`z = NOZ` allowed when
/// `planar`).
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

/// A line `P V d0 d1 scale io`.
fn line(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    rd.nonzero3()?;
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if (d1 - d0).abs() < 1e-6 {
        return Err(bad("line: domain too small"));
    }
    let scale = rd.finite()?;
    if scale <= 0.0 {
        return Err(bad("line: scale not positive"));
    }
    rd.flag()?;
    Ok(())
}

/// An ellipse or circle placement and domain `C X Y d0 d1`.
fn conic_frame(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.vec3()?;
    let x = rd.nonzero3()?;
    let y = rd.nonzero3()?;
    let cross = [
        x[1] * y[2] - x[2] * y[1],
        x[2] * y[0] - x[0] * y[2],
        x[0] * y[1] - x[1] * y[0],
    ];
    if cross.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
        return Err(bad("ellipse: parallel axes"));
    }
    let d0 = rd.finite()?;
    let d1 = rd.finite()?;
    if d0.abs() > 720.0 || d1.abs() > 720.0 {
        return Err(bad("ellipse: domain end beyond 720 degrees"));
    }
    if !(1e-4..=360.0).contains(&(d1 - d0)) {
        return Err(bad("ellipse: domain length outside [1e-4, 360]"));
    }
    Ok(())
}

fn radius(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let r = rd.finite()?;
    if !(1e-6..=1e6).contains(&r) {
        return Err(bad("ellipse: radius outside [1e-6, 1e6]"));
    }
    Ok(())
}

/// A curve block (see the module docs); returns the parameter range a sub-interval
/// must lie in.
fn curve_block(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    let kind = rd.index(4)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        0 => {
            rd.flag()?;
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
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok((0.0, 1.0))
        }
        2 => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            line(rd)?;
            Ok((-MAX_COORD / 8.0, MAX_COORD / 8.0))
        }
        _ => {
            if dim != 3 {
                return Err(bad("ellipse block: dimension must be 3"));
            }
            conic_frame(rd)?;
            radius(rd)?;
            if kind == 3 {
                radius(rd)?;
            }
            rd.flag()?;
            // (the analytic domain is in degrees; the curve parameter runs over [0, 1])
            Ok((0.0, 1.0))
        }
    }
}

/// An increasing interval `t0 t1` inside `[a, b]`.
fn sub_interval(rd: &mut Reader<'_>, (a, b): (f64, f64)) -> Result<(), FfiError> {
    let t0 = rd.finite()?;
    let t1 = rd.finite()?;
    if !(a <= t0 && t0 < t1 && t1 <= b) {
        return Err(bad("interval outside the curve's range"));
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
        4 => {
            let range = curve_block(rd)?;
            sub_interval(rd, range)?;
        }
        _ => return Err(bad("unknown B-rep block")),
    }
    Ok(())
}

/// The empty edit block.
const NO_EDITS: [f64; 11] = [-1.0, -1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0];

/// An edit block for a B-rep with `n_edges` edges; returns whether it drops a curve.
fn edits(rd: &mut Reader<'_>, n_edges: usize) -> Result<bool, FfiError> {
    let e = rd.num()?;
    let active = e != -1.0;
    if active {
        rd.at -= 1;
        rd.below(n_edges)?;
    }
    let tol = rd.finite()?;
    if tol > 1e3 {
        return Err(bad("edit: tolerance too large"));
    }
    rd.flag()?;
    rd.vec3()?;
    rd.flag()?;
    rd.vec3()?;
    Ok(rd.flag()? && active)
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

/// Checks the op arguments against the B-rep's counts.
fn op_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10], dropped: bool) -> Result<(), FfiError> {
    let [_, _, _, _, _, _, n_eu, n_e, _, _] = c;
    match op {
        Op::Export => {}
        Op::BBox => {
            rd.below(n_e)?;
            for _ in 0..4 {
                rd.flag()?;
            }
        }
        Op::Tight => {
            rd.below(n_e)?;
        }
        Op::Closest => {
            rd.below(n_e)?;
            rd.vec3()?;
        }
        Op::Radial | Op::RadialCurve => {
            if dropped {
                return Err(bad("radial op after a curve drop"));
            }
            rd.below(n_eu)?;
            if op == Op::Radial {
                rd.below(n_eu)?;
                let t = rd.finite()?;
                if !(-1.0..=2.0).contains(&t) {
                    return Err(bad("normalised parameter outside [-1, 2]"));
                }
            } else {
                rd.finite()?;
                let range = curve_block(rd)?;
                sub_interval(rd, range)?;
                rd.index(2)?;
                rd.flag()?;
            }
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
            sys::sc_query_edge_eval(
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
    let mut plain = block.to_vec();
    plain.extend(NO_EDITS);
    let export = raw(Op::Export, path, &plain)?;
    let Some(c) = counts(&export) else {
        // the B-rep could not be built: every op reports the same failure
        return Ok(export);
    };
    let dropped = edits(&mut rd, c[7])?;
    op_args(op, &mut rd, c, dropped)?;
    rd.done()?;
    raw(op, path, input)
}
