//! FFI wrappers for unit `query.tess`.
//!
//! Shim: `harness/cpp/shim/query_tess.cpp` (`sc_query_tess_eval`).
//! Upstream: `source/SMLib/src/SmTess.cpp`, `source/SMLib/inc/SmTess.h`.
//!
//! Every op but `Attr` reads the same blocks:
//!
//! * `S`: the surface `kind ...`: `0 oob form n m p q r s cpts(x y z w) u v`, a B-spline
//!   surface (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`, control points with the v index
//!   fastest), or `1 o x y sx sy D`, a bounded `SmPlane(o, x, y, (sx, sy), D)` (an
//!   `SmBSplineSurface`);
//! * the cache tolerances `ch ang max_edge min_edge ratio aspect` (the `SmTessSrfCache`
//!   constructor's order);
//! * the driver `kind vx vy vz sil_ch sil_ang ch ang max_edge min_edge ratio aspect` (`kind` 0 =
//!   none, 1 = `SmSurfaceTessDriver(ch, ang, max_edge, min_edge, ratio, aspect)`, 2 =
//!   `SmViewBasedTessDriver(v, sil_ch, sil_ang, ...)`);
//! * the poly B-rep `has` then `brep_tol face_tol label nloops` and per loop `etol k` then `k` x
//!   `x y z vtol` (one region, shell and face; a closed loop of `k` edges through the points;
//!   `label` the face's original face, `0` = none);
//! * the tree `n` then `n` x `parent c1 c2 aux split marked class D` (node 0 the top, `-1` =
//!   none, every node an `SmBezierAux2d` on the cache surface);
//! * the state `point_test have_t containment has_dom D` (`D` replaces the cache domain when
//!   `has_dom`);
//!
//! then the op's parameters:
//!
//! | [`Op`]     | upstream                                        | parameters      |
//! |------------|-------------------------------------------------|-----------------|
//! | `New`      | constructor, `GetType`, `IsKindOf`, the getters  | `k type..`      |
//! | `Test`     | `TestAgainstTolerances` on tree nodes            | `k node..`      |
//! | `BuildSub` | `BuildTreeWithSubdivision(surface)`              |                 |
//! | `Implant`  | `ImplantUVSegment` of poly edges                 | `k edge..`      |
//! | `Mark`     | `MarkFaceContainment`                            |                 |
//! | `Build`    | `BuildTree`                                      |                 |
//! | `Attr`     | `SmPolyToEUVUAttr` and its `MakeCopy` (alone: `id behavior eu vu`) | |
//!
//! Output: sections `key kind len values..` (kind 0 floats, 1 ints): the op statuses, the op
//! results (`New`: type, kinds, the getters; `Test`: `needs dir` per node), the state
//! `point_test have_t containment boundary_curves switch_fast n_subdivision_surfaces
//! original_face`, the cache domain with its chord and angle tolerances, the nodes in
//! allocation order (`count top` then per node `parent c1 c2 aux split marked class face surf
//! chord_ok angle_ok k edge..`; `surf` `-1` none, `-3` the cache surface, else the subdivision
//! surface index) and their domains and boxes. `Attr`: the fresh pointers, then `id behavior
//! type eu vu` of the attribute and of its copy.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: well-formed clamped
//! surfaces, finite bounded values, valid enumerators and indices, well-formed subdivision trees
//! with `SM_AD_BEZIER_SURFACE_HEAD` / `SM_AD_AUX_DATA` nodes, a driver wherever a node can pass
//! the surface tests (upstream reads its type unchecked), a poly B-rep where poly edges are
//! implanted or nodes may need a point in polygon test, and closed poly loops of three points
//! or more.

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 24;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude accepted by [`eval`].
pub const MAX_COORD: f64 = 1.0e6;
/// Largest number of subdivision tree nodes.
pub const MAX_NODES: usize = 256;
/// Largest number of poly loops.
pub const MAX_LOOPS: usize = 8;
/// Largest number of points of a poly loop.
pub const MAX_LOOP_POINTS: usize = 64;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// The constructor, the type queries and the getters.
    New,
    /// `TestAgainstTolerances`.
    Test,
    /// `BuildTreeWithSubdivision`.
    BuildSub,
    /// `ImplantUVSegment`.
    Implant,
    /// `MarkFaceContainment`.
    Mark,
    /// `BuildTree`.
    Build,
    /// `SmPolyToEUVUAttr` and `MakeCopy`.
    Attr,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::New => 0,
            Op::Test => 1,
            Op::BuildSub => 2,
            Op::Implant => 3,
            Op::Mark => 4,
            Op::Build => 5,
            Op::Attr => 6,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_query_tess_eval(
            op: i32,
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

    fn non_negative(&mut self) -> Result<f64, FfiError> {
        let v = self.finite()?;
        if v < 0.0 {
            return Err(bad("negative tolerance"));
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

    /// An index in `-1..max` (`-1` = none).
    fn opt_index(&mut self, max: i64) -> Result<i64, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < -1.0 || v >= max as f64 {
            return Err(bad(format!("bad index {v} (below {max})")));
        }
        Ok(v as i64)
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn domain(&mut self) -> Result<[f64; 4], FfiError> {
        let d = [
            self.finite()?,
            self.finite()?,
            self.finite()?,
            self.finite()?,
        ];
        if d[0] > d[2] || d[1] > d[3] {
            return Err(bad("domain not ordered"));
        }
        Ok(d)
    }

    /// A domain inside `outer`; with `strict` of positive size in both directions.
    fn domain_in(&mut self, outer: &[f64; 4], strict: bool) -> Result<[f64; 4], FfiError> {
        let d = self.domain()?;
        let o = outer;
        if d[0] < o[0] || d[1] < o[1] || d[2] > o[2] || d[3] > o[3] {
            return Err(bad("domain outside the surface domain"));
        }
        if strict && (d[2] - d[0] < 1e-6 * (o[2] - o[0]) || d[3] - d[1] < 1e-6 * (o[3] - o[1])) {
            return Err(bad("domain too small"));
        }
        Ok(d)
    }

    fn done(&self) -> Result<(), FfiError> {
        if self.at != self.input.len() {
            return Err(bad("trailing input"));
        }
        Ok(())
    }
}

/// Checks a clamped knot vector of degree `p`; returns its end values.
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

/// A B-spline surface block `oob form n m p q r s cpts u v`; returns its domain.
fn bspline_block(rd: &mut Reader<'_>) -> Result<[f64; 4], FfiError> {
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
    let mut w0 = None;
    for _ in 0..(n + 1) * (m + 1) {
        rd.finites(3)?;
        let w = rd.num()?;
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad("surface: mixed rationality"));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad("surface: weight out of range"));
        }
    }
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok([u0, v0, u1, v1])
}

/// Squared length of a vector.
fn len2(v: [f64; 3]) -> f64 {
    v[0] * v[0] + v[1] * v[1] + v[2] * v[2]
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ]
}

/// The surface block `kind ...` (a B-spline or a bounded plane); returns its domain.
fn surface_block(rd: &mut Reader<'_>) -> Result<[f64; 4], FfiError> {
    if rd.index(1)? == 0 {
        return bspline_block(rd);
    }
    rd.vec3()?;
    let x = rd.vec3()?;
    let y = rd.vec3()?;
    let (lx, ly) = (len2(x), len2(y));
    if !(1e-6..=1e6).contains(&lx) || !(1e-6..=1e6).contains(&ly) {
        return Err(bad("plane: axis length out of range"));
    }
    if len2(cross(x, y)) < 1e-6 * lx * ly {
        return Err(bad("plane: axes (nearly) parallel"));
    }
    for _ in 0..2 {
        let s = rd.finite()?;
        if !(1e-3..=1e3).contains(&s) {
            return Err(bad("plane: scale out of range"));
        }
    }
    let d = rd.domain()?;
    if d[2] - d[0] < 1e-6 || d[3] - d[1] < 1e-6 || d.iter().any(|v| v.abs() > 1e4) {
        return Err(bad("plane: domain size out of range"));
    }
    Ok(d)
}

/// The poly B-rep block; returns its edge count (`None` without poly B-rep).
fn poly(rd: &mut Reader<'_>) -> Result<Option<usize>, FfiError> {
    if !rd.flag()? {
        return Ok(None);
    }
    rd.non_negative()?;
    rd.non_negative()?;
    rd.index(1_000_000)?;
    let nl = rd.index(MAX_LOOPS)?;
    if nl == 0 {
        return Err(bad("poly: no loops"));
    }
    let mut edges = 0;
    for _ in 0..nl {
        rd.non_negative()?;
        let k = rd.index(MAX_LOOP_POINTS)?;
        if k < 3 {
            return Err(bad("poly: a loop needs three points"));
        }
        for _ in 0..k {
            rd.finites(3)?;
            rd.non_negative()?;
        }
        edges += k;
    }
    Ok(Some(edges))
}

/// A subdivision tree; returns its node count (0 = none).
fn tree(rd: &mut Reader<'_>, srf: &[f64; 4]) -> Result<usize, FfiError> {
    let n = rd.index(MAX_NODES)?;
    if n == 0 {
        return Ok(0);
    }
    let mut links = Vec::with_capacity(n);
    for _ in 0..n {
        let parent = rd.opt_index(n as i64)?;
        let c1 = rd.opt_index(n as i64)?;
        let c2 = rd.opt_index(n as i64)?;
        let aux = rd.index(6)?;
        if !matches!(aux, 4 | 6) {
            return Err(bad(
                "tree: aux type must be SM_AD_BEZIER_SURFACE_HEAD or SM_AD_AUX_DATA",
            ));
        }
        rd.index(2)?;
        rd.flag()?;
        rd.index(3)?;
        rd.domain_in(srf, false)?;
        links.push((parent, c1, c2));
    }
    // one root (node 0), child pairs pointing back, every node reached once from the root
    let mut seen = vec![false; n];
    let mut stack = vec![0usize];
    if links.first().map(|l| l.0) != Some(-1) {
        return Err(bad("tree: node 0 must be the root"));
    }
    while let Some(k) = stack.pop() {
        let Some(s) = seen.get_mut(k) else {
            return Err(bad("tree: bad node"));
        };
        if *s {
            return Err(bad("tree: node reached twice"));
        }
        *s = true;
        let (_, c1, c2) = links.get(k).copied().ok_or_else(|| bad("tree: bad node"))?;
        match (c1, c2) {
            (-1, -1) => {}
            (a, b) if a >= 0 && b >= 0 && a != b => {
                for c in [a as usize, b as usize] {
                    if links.get(c).map(|l| l.0) != Some(k as i64) {
                        return Err(bad("tree: child does not point back"));
                    }
                    stack.push(c);
                }
            }
            _ => return Err(bad("tree: need two children or none")),
        }
    }
    if seen.iter().any(|s| !s) {
        return Err(bad("tree: unreachable node"));
    }
    Ok(n)
}

fn validate_op(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    if op == Op::Attr {
        rd.index(1_000_000)?;
        rd.index(4)?;
        rd.index(1_000_000)?;
        rd.index(1_000_000)?;
        return Ok(());
    }
    let srf = surface_block(rd)?;
    rd.finites(6)?;
    let driver = rd.index(2)?;
    rd.finites(11)?;
    let edges = poly(rd)?;
    let n = tree(rd, &srf)?;
    rd.flag()?;
    rd.flag()?;
    rd.index(2)?;
    if rd.flag()? {
        rd.domain_in(&srf, true)?;
    } else {
        rd.finites(4)?;
    }
    let need = |ok: bool, what: &str| -> Result<(), FfiError> {
        if ok {
            Ok(())
        } else {
            Err(bad(what.to_string()))
        }
    };
    match op {
        Op::New => {
            let k = rd.index(16)?;
            for _ in 0..k {
                rd.index(1_000_000)?;
            }
        }
        Op::Test => {
            need(driver != 0, "test: upstream reads the driver unchecked")?;
            need(n > 0, "test: needs a tree")?;
            let k = rd.index(8)?;
            for _ in 0..k {
                rd.index(n - 1)?;
            }
        }
        Op::BuildSub => {
            need(driver != 0, "build: upstream reads the driver unchecked")?;
            need(
                n == 0,
                "build with subdivision: the tree must not exist (it would leak)",
            )?;
        }
        Op::Implant => {
            let e = edges.ok_or_else(|| bad("implant: needs a poly B-rep"))?;
            need(n > 0, "implant: needs a tree")?;
            let k = rd.index(16)?;
            for _ in 0..k {
                rd.index(e - 1)?;
            }
        }
        Op::Mark => {
            need(edges.is_some(), "mark: needs a poly B-rep")?;
            need(n > 0, "mark: needs a tree")?;
        }
        Op::Build => {
            need(driver != 0, "build: upstream reads the driver unchecked")?;
        }
        Op::Attr => {}
    }
    Ok(())
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    validate_op(op, &mut rd)?;
    rd.done()
}

/// Runs `op` on `input` (layouts in the module docs); returns the shim's output record.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1 << 16, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped surfaces, closed poly loops,
        // well-formed subdivision trees, every index, count and enumerator the shim passes to
        // upstream, a driver and a poly B-rep wherever upstream dereferences them); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_query_tess_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn common(driver: f64, n_tree: usize) -> Vec<f64> {
        let mut input = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0];
        for (x, y) in [(0.0, 0.0), (0.0, 1.0), (1.0, 0.0), (1.0, 1.0)] {
            input.extend([x, y, 0.0, NOW]);
        }
        input.extend([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
        input.extend([0.01, 0.3, 0.0, 0.0, 0.001, 0.0]);
        input.push(driver);
        input.extend([0.0, 0.0, 1.0, 0.0, 5.0, 0.0, 20.0, 0.0, 0.0, 0.001, 0.0]);
        input.push(0.0); // no poly
        input.push(n_tree as f64);
        for _ in 0..n_tree {
            input.extend([-1.0, -1.0, -1.0, 6.0, 0.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0]);
        }
        input.extend([1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]);
        input
    }

    #[test]
    fn rejects_truncation_and_trailing_data_before_cpp() {
        let mut input = common(1.0, 0);
        input.extend([1.0, 15150.0]);
        assert!(validate(Op::New, &input).is_ok());
        for end in 0..input.len() {
            assert!(validate(Op::New, &input[..end]).is_err(), "prefix {end}");
        }
        input.push(0.0);
        assert!(validate(Op::New, &input).is_err());
    }

    #[test]
    fn rejects_missing_driver_and_poly() {
        let mut input = common(0.0, 1);
        input.extend([1.0, 0.0]);
        assert!(validate(Op::Test, &input).is_err());
        let mut input = common(1.0, 1);
        input.extend([1.0, 0.0]);
        assert!(validate(Op::Test, &input).is_ok());
        let input = common(1.0, 1);
        assert!(validate(Op::Mark, &input).is_err());
        let input = common(0.0, 0);
        assert!(validate(Op::Build, &input).is_err());
    }
}
