//! FFI wrappers for unit `topology.face_p2`.
//!
//! Shim: `harness/cpp/shim/topology_face_p2.cpp` (`sc_topology_face_p2_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp`.
//!
//! B-rep ops start with a B-rep block (the shim builds a fresh `SmBrep` per call and
//! deletes it after), as in [`super::topology_vertexuse`]:
//!
//! | block                                         | B-rep                                     |
//! |-----------------------------------------------|-------------------------------------------|
//! | `0 rebuild validate_only init_gaps`           | `ReadFromFile(path, SM_ASCII, rebuild)`   |
//! | `1 curve t0 t1 validate_only init_gaps`       | `CreateWireEdgeFromCurve(curve, [t0, t1])`|
//! | `2 surface u0 v0 u1 v1 validate_only init_gaps` | `CreateFaceFromSurface(surface, dom)`   |
//! | `3 validate_only init_gaps`                   | the wire network of `prog_test` `my_test_topology` |
//!
//! A surface block is `0 oob form n m p q r s cpts(x y z w) u v` (`SmBSplineSurface`) or
//! `1 O[3] N[3]` (`SmPlane(O, N)`); a curve block `dim n p m cpts(x y z w) knots`
//! (`SmBSplineCurve`); an optional curve is `0` or `1 curve`. Indices refer to the
//! export's traversal order (`-1` null where allowed). Every B-rep op's output starts
//! with `0`, or with `-1 status` when the B-rep could not be built (nothing else
//! follows). `-2` in an output is "upstream would dereference null" (the shim did not
//! call it). Curves in outputs are the shim's `put_curve` blocks.
//!
//! | [`Op`]          | parameters after the block | outputs |
//! |-----------------|----------------------------|---------|
//! | `Export`        |                            | the graph (see [`counts`] and the difftest runner) |
//! | `FaceQuery`     | `f tgt_only has_loops n loop*` | `has_surface dom[4] rect tol fu1 fu2 upward loops outer_loops type class_type kind_of*9 type_string class_string` |
//! | `Set`           | `f u0 v0 u1 v1 fu`         | `dom[4] upward fu1 fu2 loops` (after `SetUVDomain`, `SetFaceuse`) |
//! | `Points`        | `f count has_dom [u0 v0 u1 v1]` (`SetUVDomain` first when given) | `status n uv[2n] pv[6n]` |
//! | `Internal`      | `f`                        | `status x y z` |
//! | `FromFace`      | `f orient want_3d want_uv` | `status loops orients points curves_3d curves_uv` (counted lists) |
//! | `Trim`          | `f has_tol tol has_3d n opt_curve* has_uv n opt_curve* want_3d want_uv` | `status n3 curve* nuv curve*` (`-1` counts when not asked) |
//! | `FixUv`         | `eu prev vertex tol dist face_dom u0 v0 u1 v1` | `status fixed curve(eu) curve(prev)` |
//! | `BestUv`        | `eu n (kind [curve] orient)*` (kind `0` null, `1` a curve, `2` a copy of the edge use's UV trim curve) | `status which curve slot_flags*n` |
//! | `Connected`     | `eu1 eu2 singularity_ok seam_ok` | `connected on_singularity crosses_seam` |
//! | `Rescue`        | `f to_null`                | `owned_before nulled owned_after` |
//! | `RemoveSuspect` | `n (start_vertex end_vertex)*` | per edge use, whether it holds a UV trim curve |
//!
//! Geometry ops (no B-rep block, no leading `0`):
//!
//! | [`Op`]       | parameters | outputs |
//! |--------------|------------|---------|
//! | `SeamCross`  | `uv1[2] uv2[2] dom[4] closed_u closed_v shift[2]` | `ret uv1[2] shift[2]` |
//! | `Layout`     | `flags`    | `num area volume moments*8` |
//! | `OdeIso`     | `surface u t origin[3] thickness flags n` | `status dydx[n]` |
//! | `OdeArea`    | `surface curve t min_v accuracy ny yscale* origin[3] thickness flags n dydx*` | `status n dydx*` |
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: existing
//! `.smb` files, well-formed clamped B-splines, planes with a non-zero normal, indices
//! inside the B-rep's export (it builds the B-rep once more to count), finite inputs,
//! derivative arrays long enough for the property layout.

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
    FaceQuery = 1,
    Set = 2,
    Points = 3,
    Internal = 4,
    FromFace = 5,
    Trim = 6,
    FixUv = 7,
    BestUv = 8,
    Connected = 9,
    Rescue = 10,
    RemoveSuspect = 11,
    SeamCross = 20,
    Layout = 21,
    OdeIso = 22,
    OdeArea = 23,
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
        self.code() < Self::SeamCross.code()
    }
}

mod sys {
    extern "C" {
        pub fn sc_topology_face_p2_eval(
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

/// Largest point count of `Points`.
pub const MAX_POINTS: usize = 4096;
/// Largest number of curves in a list argument.
pub const MAX_CURVES: usize = 8;
/// Largest derivative array of the ODE ops.
pub const MAX_DYDX: usize = 64;
/// Largest number of classification intervals of `RemoveSuspect`.
pub const MAX_INTERVALS: usize = 16;

/// `SmPrecisePropertyLayout(flags).m_lNumComponents`.
#[must_use]
pub fn layout_components(flags: u64) -> usize {
    let mut n = 1;
    for (bit, count) in [
        (0x01, 1),
        (0x08, 1),
        (0x02, 3),
        (0x04, 9),
        (0x10, 3),
        (0x20, 9),
    ] {
        if flags & bit != 0 {
            n += count;
        }
    }
    n
}

/// An index below `count` or `-1`.
fn opt_below(rd: &mut Reader<'_>, count: usize) -> Result<(), FfiError> {
    let v = rd.num()?;
    if v == -1.0 {
        return Ok(());
    }
    if v.fract() != 0.0 || v < 0.0 || v >= count as f64 {
        return Err(bad(format!("bad index {v} (count {count})")));
    }
    Ok(())
}

/// An optional curve (`0` or `1 curve`) of dimension `dim` (any when `None`); whether
/// it was present.
fn opt_curve(rd: &mut Reader<'_>, dim: Option<usize>) -> Result<bool, FfiError> {
    if !rd.flag()? {
        return Ok(false);
    }
    let d = rd.input.get(rd.at).copied().unwrap_or(0.0);
    if let Some(want) = dim {
        if d != want as f64 {
            return Err(bad("curve of the wrong dimension"));
        }
    }
    curve(rd)?;
    Ok(true)
}

/// Checks the arguments after the B-rep block against the B-rep's counts.
fn brep_args(op: Op, rd: &mut Reader<'_>, c: [usize; 10]) -> Result<(), FfiError> {
    let [_, _, n_fu, n_f, _, n_l, n_eu, _, _, n_v] = c;
    match op {
        Op::Export => {}
        Op::FaceQuery => {
            rd.below(n_f)?;
            rd.flag()?;
            rd.flag()?;
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                rd.below(n_l)?;
            }
        }
        Op::Set => {
            rd.below(n_f)?;
            rd.finites(4)?;
            opt_below(rd, n_fu)?;
        }
        Op::Points => {
            rd.below(n_f)?;
            rd.index(MAX_POINTS)?;
            if rd.flag()? {
                rd.finites(4)?;
            }
        }
        Op::Internal => {
            rd.below(n_f)?;
        }
        Op::FromFace => {
            rd.below(n_f)?;
            rd.index(2)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Trim => {
            rd.below(n_f)?;
            rd.flag()?;
            let tol = rd.finite()?;
            if !(0.0..=1.0e3).contains(&tol) {
                return Err(bad("trim tolerance out of range"));
            }
            let has_3d = rd.flag()?;
            let n3 = rd.index(MAX_CURVES)?;
            let mut all_3d = true;
            for _ in 0..n3 {
                all_3d &= opt_curve(rd, Some(3))?;
            }
            let has_uv = rd.flag()?;
            let nuv = rd.index(MAX_CURVES)?;
            for _ in 0..nuv {
                opt_curve(rd, Some(2))?;
            }
            let want_3d = rd.flag()?;
            rd.flag()?;
            // UV trimming indexes the 3D list in step with the UV list
            if has_uv && has_3d && want_3d && (n3 < nuv || !all_3d) {
                return Err(bad("trim: the 3D list must cover the UV list"));
            }
        }
        Op::FixUv => {
            rd.below(n_eu)?;
            opt_below(rd, n_eu)?;
            opt_below(rd, n_v)?;
            rd.finite()?;
            rd.finite()?;
            rd.flag()?;
            rd.finites(4)?;
        }
        Op::BestUv => {
            rd.below(n_eu)?;
            let n = rd.index(MAX_CURVES)?;
            for _ in 0..n {
                // 0 null, 1 a curve block, 2 the edge use's own UV trim curve
                if rd.index(2)? == 1 {
                    let d = rd.input.get(rd.at).copied().unwrap_or(0.0);
                    if d != 2.0 {
                        return Err(bad("candidate UV curve must be 2D"));
                    }
                    curve(rd)?;
                }
                rd.index(2)?;
            }
        }
        Op::Connected => {
            rd.below(n_eu)?;
            rd.below(n_eu)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Rescue => {
            rd.below(n_f)?;
            rd.flag()?;
        }
        Op::RemoveSuspect => {
            let n = rd.index(MAX_INTERVALS)?;
            for _ in 0..2 * n {
                opt_below(rd, n_v)?;
            }
        }
        Op::SeamCross | Op::Layout | Op::OdeIso | Op::OdeArea => {
            return Err(bad("not a B-rep op"));
        }
    }
    Ok(())
}

/// Checks a geometry op's arguments.
fn geom_args(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::SeamCross => {
            rd.finites(8)?;
            rd.flag()?;
            rd.flag()?;
            rd.finites(2)?;
        }
        Op::Layout => {
            rd.index(255)?;
        }
        Op::OdeIso => {
            surface(rd)?;
            rd.finites(5)?;
            let thick = rd.finite()?;
            if thick.abs() > 1.0e3 {
                return Err(bad("thickness out of range"));
            }
            let flags = rd.index(63)?;
            let n = rd.index(MAX_DYDX)?;
            if n < 2 || n < layout_components(flags as u64) + 1 {
                return Err(bad("derivative array too short for the layout"));
            }
        }
        Op::OdeArea => {
            surface(rd)?;
            let (a, b) = curve(rd)?;
            let t = rd.finite()?;
            if !(a <= t && t <= b) {
                return Err(bad("curve parameter outside its interval"));
            }
            rd.finite()?;
            let acc = rd.finite()?;
            if !(1.0e-12..=1.0).contains(&acc) {
                return Err(bad("accuracy out of range"));
            }
            let ny = rd.index(MAX_DYDX)?;
            for _ in 0..ny {
                let s = rd.finite()?;
                if s <= 0.0 {
                    return Err(bad("y scale must be positive"));
                }
            }
            rd.finites(3)?;
            let thick = rd.finite()?;
            if thick.abs() > 1.0e3 {
                return Err(bad("thickness out of range"));
            }
            let flags = rd.index(63)?;
            if ny < layout_components(flags as u64) {
                return Err(bad("y scale shorter than the layout"));
            }
            let n = rd.index(MAX_DYDX)?;
            rd.finites(n)?;
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
            sys::sc_topology_face_p2_eval(
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
    if !op.is_brep() {
        geom_args(op, &mut rd)?;
        rd.done()?;
        return raw(op, "", input);
    }
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
