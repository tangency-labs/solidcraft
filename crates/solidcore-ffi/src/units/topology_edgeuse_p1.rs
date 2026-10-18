//! FFI wrappers for unit `topology.edgeuse_p1`.
//!
//! Shim: `harness/cpp/shim/topology_edgeuse_p1.cpp` (`sc_topology_edgeuse_p1_eval`).
//! Upstream: `source/SMLib/src/SmEdgeuse.cpp`, `source/SMLib/src/SmFaceuse.cpp`.
//!
//! B-rep ops ([`Op::is_brep`]) start with a B-rep block (the shim builds a fresh
//! `SmBrep` per call and deletes it after):
//!
//! | block                                         | B-rep                                     |
//! |-----------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `1 curve t0 t1 validate_only init_gaps`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)`   |
//! | `3 validate_only init_gaps`                   | the wire network of `prog_test` `my_test_topology` |
//! | `4 n points(x y z)*n validate_only init_gaps` | actual `MakeShellVertex` calls (0..64 vertices) |
//! | `5 has_face [surface u0 v0 u1 v1] networks validate_only init_gaps` | an optional face (as kind 2) and `networks` (0..=3) copies of the wire network in one B-rep |
//! | `6 surface u0 v0 u1 v1 validate_only init_gaps` | as kind 2 on a B-spline surface, every edge curve then an `SmCrvOnSurf` over its UV trim curve |
//!
//! A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (`SmBSplineSurface`) or
//! `1 O[3] N[3]` (`SmPlane(O, N)`); a curve block `dim n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`). Indices refer to the export's traversal order. Every B-rep op's
//! output starts with `0`, or with `-1 status` when the B-rep could not be built
//! (nothing else follows). `-2` in an output is "upstream would dereference null"
//! (the shim did not call it).
//!
//! | [`Op`]      | parameters after the block              | outputs |
//! |-------------|-----------------------------------------|---------|
//! | `Export`    |                                         | the graph (see [`counts`] and the difftest runner) |
//! | `EuQuery`   | `eu vertex`                             | `edge orient cw ccw has_ef has_ccw ef_init loop shell_eu vu lu fu face shell brep tol mate radial strut has_uv uv_owner has_fail fail_owner own_uv corner type kind_of*9` |
//! | `Connected` | `eu type index`                         | `connected` |
//! | `FuQuery`   | `fu tgt_only has_loops n loops*n`       | `shell region face mate orient type kind_of*9`, then lists: loop uses, edge uses, neighbours, neighbours skipping lamina edges |
//! | `Collect`   | `fu has_threshold threshold`            | `status` list |
//! | `UvLoopGap` | `eu`                                    | `gap` |
//! | `SetUv`     | `eu kind [curve] dmax delete has_fail fail_status` | `0` or `-2`, then the edge use's and its mate's state, the edge tolerance, the argument record's B-rep flag |
//! | `Gaps`      | `eu which flag want_out`                | per `which` (`0` `GetMaxEdgeFaceGap`, `1` `GetEdgeCCWEdgeGap`, `2` `CalcMaxEdgeFaceGap`, `3` `CalcEdgeCCWEdgeGap`) |
//! | `SetOrient` | `eu orientation`                        | `orient mate radial` |
//!
//! The graph export ends with a versioned extension: `3 infinite_region`, then
//! one raw void flag per region and one raw wire-edge-use/vertex-use head index
//! per shell (`-1` for other member classes), then a vertex-use index list per
//! vertex in native list order, then literal next/last indices per face use.
//! The original graph prefix and the
//! ten entity counts remain unchanged. This supplies shell state independently
//! of the shell operations being compared.
//!
//! Geometry ops: `StepOff` (`surface dist P[3] N[3] u v jac` -> `status found F0 F1
//! J00 J01 J10 J11`), `DistDev` (`surface P[3] u v value obj want_dev` -> `dist dev`;
//! `obj` 0 null, 1 the surface, 2 a line), `Seam` (`n curve*n a b called` -> `seam`).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, planes with a non-zero normal,
//! parameters within 100 domain lengths of a surface's domain, plane-face domains
//! bounded by 1e6 for graph export (1e4 for other operations), indices inside the
//! B-rep's export (it builds the B-rep once more to count), finite inputs.

use std::path::Path;

use crate::{call_with_buffer, FfiError};

/// Largest control point index.
pub const MAX_INDEX: usize = 32;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e300;
/// Largest number of curves for `Seam` and of target loops for `FuQuery`.
pub const MAX_LIST: usize = 64;

const NOW: f64 = -f64::MAX;
const NOZ: f64 = f64::MAX;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Export = 0,
    EuQuery = 1,
    Connected = 2,
    FuQuery = 3,
    Collect = 4,
    UvLoopGap = 5,
    SetUv = 6,
    Gaps = 7,
    StepOff = 8,
    DistDev = 9,
    Seam = 10,
    SetOrient = 11,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        self as i32
    }

    /// True for the ops starting with a B-rep block.
    #[must_use]
    pub fn is_brep(self) -> bool {
        !matches!(self, Op::StepOff | Op::DistDev | Op::Seam)
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_edgeuse_p1_eval(
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

pub(crate) fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

/// Sequential reader over the input (shared with unit `topology.edgeuse_p2`).
pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [f64],
    pub(crate) at: usize,
}

impl Reader<'_> {
    pub(crate) fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    pub(crate) fn finite(&mut self) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("non-finite or huge value"));
        }
        Ok(v)
    }

    pub(crate) fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    pub(crate) fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    pub(crate) fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    pub(crate) fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    /// An index below `count`.
    pub(crate) fn below(&mut self, count: usize) -> Result<usize, FfiError> {
        let i = self.index(usize::MAX >> 12)?;
        if i >= count {
            return Err(bad(format!("index {i} out of range ({count})")));
        }
        Ok(i)
    }

    /// An index below `count` or `-1`.
    pub(crate) fn below_or_none(&mut self, count: usize) -> Result<(), FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(());
        }
        self.at -= 1;
        self.below(count).map(|_| ())
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    pub(crate) fn done(&self) -> Result<(), FfiError> {
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
            return Err(bad(format!("{what}: interior multiplicity above the degree")));
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
        if [x, y, z].iter().any(|c| !c.is_finite() || c.abs() > MAX_COORD) {
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
pub(crate) fn curve(rd: &mut Reader<'_>) -> Result<(f64, f64), FfiError> {
    curve_up_to(rd, MAX_INDEX)
}

/// A B-spline curve block with at most `max_index + 1` control points; returns its
/// parameter range.
pub(crate) fn curve_up_to(rd: &mut Reader<'_>, max_index: usize) -> Result<(f64, f64), FfiError> {
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve: dimension must be 2 or 3"));
    }
    let n = rd.index(max_index)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(max_index + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    check_points(rd, n + 1, dim == 2, "curve")?;
    let knots = rd.finites(m + 1)?;
    check_knots(&knots, p, "curve")
}

/// A `uv` point within 100 domain lengths of `dom` (anywhere bounded for a plane).
fn uv(rd: &mut Reader<'_>, dom: Domain) -> Result<(), FfiError> {
    let u = rd.finite()?;
    let v = rd.finite()?;
    if let Some([u0, v0, u1, v1]) = dom {
        let (du, dv) = (u1 - u0, v1 - v0);
        if u < u0 - 100.0 * du || u > u1 + 100.0 * du || v < v0 - 100.0 * dv || v > v1 + 100.0 * dv {
            return Err(bad("uv too far outside the domain"));
        }
    } else if u.abs() > 1e6 || v.abs() > 1e6 {
        return Err(bad("plane uv too large"));
    }
    Ok(())
}

/// A B-rep block.
pub(crate) fn brep_block(rd: &mut Reader<'_>, path: &str) -> Result<(), FfiError> {
    brep_block_with_plane_limit(rd, path, 1e4)
}

/// The surface and parameter domain of a face block (`2`, `6`, and `5` with a face):
/// whether the surface is a B-spline surface (not a plane).
pub(crate) fn face_block(rd: &mut Reader<'_>, plane_domain_limit: f64) -> Result<bool, FfiError> {
        let dom = surface(rd)?;
        let d = rd.finites(4)?;
        let (Some(&a), Some(&b2), Some(&c), Some(&e)) = (d.first(), d.get(1), d.get(2), d.get(3))
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
                if d.iter().any(|x| x.abs() > plane_domain_limit) {
                    return Err(bad("plane face domain too large"));
                }
            }
        }
    Ok(dom.is_some())
}

fn brep_block_with_plane_limit(rd: &mut Reader<'_>, path: &str, plane_domain_limit: f64) -> Result<(), FfiError> {
    match rd.index(6)? {
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
            face_block(rd, plane_domain_limit)?;
        }
        6 => {
            // kind 6 (edges as `SmCrvOnSurf`) needs an `SmBSplineSurface` base
            if !face_block(rd, plane_domain_limit)? {
                return Err(bad("CrvOnSurf face: the base must be a B-spline surface"));
            }
        }
        5 => {
            if rd.flag()? {
                face_block(rd, plane_domain_limit)?;
            }
            rd.index(3)?;
        }
        4 => {
            let count = rd.index(MAX_LIST)?;
            for _ in 0..count {
                rd.vec3()?;
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

/// Checks the arguments after the B-rep block against the B-rep's counts.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, n_fu, _, _, n_loop, n_eu, _, _, n_v] = c;
    match op {
        Op::Export => {}
        Op::EuQuery => {
            rd.below(n_eu)?;
            rd.below_or_none(n_v)?;
        }
        Op::Connected => {
            rd.below(n_eu)?;
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
        Op::FuQuery => {
            rd.below(n_fu)?;
            rd.flag()?;
            rd.flag()?;
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                rd.below(n_loop)?;
            }
        }
        Op::Collect => {
            rd.below(n_fu)?;
            rd.flag()?;
            rd.finite()?;
        }
        Op::UvLoopGap => {
            rd.below(n_eu)?;
        }
        Op::SetUv => {
            rd.below(n_eu)?;
            let kind = rd.index(4)?;
            if kind == 2 || kind == 4 {
                curve(rd)?;
            }
            rd.finite()?;
            rd.flag()?;
            rd.flag()?;
            let s = rd.num()?;
            if s.fract() != 0.0 || s.abs() > 1.0e6 {
                return Err(bad("bad fail status"));
            }
        }
        Op::Gaps => {
            rd.below(n_eu)?;
            rd.index(3)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::SetOrient => {
            rd.below(n_eu)?;
            rd.index(2)?;
        }
        Op::StepOff | Op::DistDev | Op::Seam => return Err(bad("not a B-rep op")),
    }
    Ok(())
}

/// Checks a geometry op's input.
fn geom_args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::StepOff => {
            let dom = surface(rd)?;
            rd.finite()?;
            rd.vec3()?;
            rd.vec3()?;
            uv(rd, dom)?;
            rd.flag()?;
        }
        Op::DistDev => {
            let dom = surface(rd)?;
            rd.vec3()?;
            uv(rd, dom)?;
            rd.finite()?;
            rd.index(2)?;
            rd.flag()?;
        }
        Op::Seam => {
            let n = rd.index(MAX_LIST)?;
            for _ in 0..n {
                curve(rd)?;
            }
            let a = rd.finite()?;
            let b = rd.finite()?;
            if a > b {
                return Err(bad("edge domain not ordered"));
            }
            rd.flag()?;
        }
        _ => return Err(bad("not a geometry op")),
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
            sys::sc_topology_edgeuse_p1_eval(
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
    if !op.is_brep() {
        geom_args(op, &mut rd)?;
        rd.done()?;
        return raw(op, path, input);
    }
    // Export constructs the graph but does not run an edge-use geometry
    // operation. Actual native shell probes cover plane domains through 1e6;
    // retain the existing 1e4 bound for the other edge-use operations.
    let plane_domain_limit = if op == Op::Export { 1e6 } else { 1e4 };
    brep_block_with_plane_limit(&mut rd, path, plane_domain_limit)?;
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
