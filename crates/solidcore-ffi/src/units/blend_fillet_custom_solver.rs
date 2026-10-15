//! FFI wrappers for unit `blend.fillet_custom_solver`.
//!
//! Shim: `harness/cpp/shim/blend_fillet_custom_solver.cpp`
//! (`sc_blend_fillet_custom_solver_eval`).
//! Upstream: `source/SMLib/src/SmFilletCustomSolver.cpp`.
//!
//! A curve is `n p m`, `n+1` control points `x y z w`, `m+1` knots
//! (`SmBSplineCurve(gw_CURVE*, 3, FALSE)`). A surface is `n m p q r s`, `(n+1)(m+1)`
//! control points `x y z w` (v index fastest), `r+1` u knots, `s+1` v knots
//! (`SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`). A solver block is `same_base`, surface
//! A, surface B (absent when `same_base`), `r0 r1 o0 o1 angle ext`: the solver's rail 0
//! offsets A by `r0 * o0`, rail 1 offsets B (or A) by `r1 * o1`, the radii are
//! `m_dFilletRadii`, the orientations `m_dOrientations`, `angle` is the tower or secant
//! angle and `ext` `m_dSurfaceExtensionFactor`; its edge uses are `NULL`. A surface
//! selector is `0` `NULL`, `1` `GetSurface(0)`, `2` `GetSurface(1)`, `3` another offset
//! surface of A, `4` an offset surface of a copy of A.
//!
//! | [`Op`]        | upstream                                          | parameters |
//! |---------------|---------------------------------------------------|------------|
//! | `Create`      | `SmEiffelTowerCrossSectionFSG::CreateSurface`      | `tol rail1 rail2 k` then `k` x `p1[3] p2[3]` (the fillet points' `SrfPos(0)`, `SrfPos(1)`) |
//! | `JacEiffel`, `JacSecant` | `SmEiffelTowerFS::LoadJacobian`, `SmSecantConstantRadiusFS::LoadJacobian` | solver block, `rail ne np sel1 off1 sel2 off2 nx x[nx] nf f[nf] has_j rows cols` |
//! | `InitEiffel`, `InitSecant` | `SmEiffelTowerFS::LoadInitialValues`, `SmSecantConstantRadiusFS::LoadInitialValues` | solver block, `do2 rail sel1 s1_index dim` `dim` x `min max`, `np per[np] ng guess[ng]` |
//!
//! Output: the op status (`0` = `SM_SUCCESS`, else the `SmStatus`), then: `Create` both
//! rails' final state (`has n p m cpts knots`) and on success `has` and the surface state
//! (`has n m p q r s cpts u v`); `Jac*` `ne np found`, `f[nf]`, the `rows x cols`
//! Jacobian when given and the two offset distances afterwards; `Init*` the rail of
//! `rpSurface2` (`-1` `NULL`), `rlSurface2Index` (`999` untouched), the `dim` intervals,
//! `np' per[np']`, `ng' guess[ng']`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped 3D curves and surfaces (`1 <= p <= n`, at most [`MAX_INDEX`]
//! control points per direction, degrees at most [`MAX_DEGREE`], finite data,
//! consistent rationality, non-decreasing knots with end multiplicity exactly
//! `degree + 1` and interior multiplicity at most the degree); `Create` needs a
//! non-negative tolerance, at most [`MAX_POINTS`] fillet points and rails over the same
//! interval (the skinning re-intersects rail 2 at `pIntersectionsWithRails[k..2k]`,
//! past the `k` parameters `CreateSurface` passes, when synchronising the rails changes
//! rail 2's interval), except for two non-rational 2-point degree-1 rails whose
//! directions agree to well inside upstream's `SM_EFF_ZERO_SQRT` angle (they always
//! take the ruled-surface branch, which never skins); `Jac*` needs every `crX` read (the UV offsets the upstream
//! index logic picks, plus one) inside `x`, `nf >= ne + 3`, and a Jacobian with
//! `ne + 3` rows and room for both offsets' columns; `Init*` needs rail `0` or `1`, a
//! non-`NULL` first surface and, with `do2`, the guess and interval indices it reads
//! and writes (`s1_index + 1 < ng`, `s1_index + 1 < dim`, `ng + 1 < dim`) in range.
//!
//! One known exception (`plan/upstream-issues.md` UB-99): when `CreateSurface` skins and
//! the first `N_CreateSkinSrfBoundaryContinuity` attempt fails after refining the
//! derivative curves, upstream's retry reads a freed knot vector of the derivative
//! surfaces. Predicting that needs the skinning itself, so such inputs are not rejected
//! (the difftest case `gen0137` is one).

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 64;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 9;
/// Largest number of fillet points accepted by [`eval`].
pub const MAX_POINTS: usize = 64;
/// Largest array length (`x`, `f`, guesses, intervals, Jacobian sides) accepted by
/// [`eval`].
pub const MAX_ARRAY: usize = 32;
/// Largest `|crX|` entry accepted by [`eval`] (offset surfaces extend their base
/// surface to reach it).
pub const MAX_PARAM: f64 = 1.0e6;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `SmEiffelTowerCrossSectionFSG::CreateSurface`.
    Create,
    /// `SmEiffelTowerFS::LoadJacobian`.
    JacEiffel,
    /// `SmSecantConstantRadiusFS::LoadJacobian`.
    JacSecant,
    /// `SmEiffelTowerFS::LoadInitialValues`.
    InitEiffel,
    /// `SmSecantConstantRadiusFS::LoadInitialValues`.
    InitSecant,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::Create => 0,
            Op::JacEiffel => 1,
            Op::JacSecant => 2,
            Op::InitEiffel => 3,
            Op::InitSecant => 4,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_blend_fillet_custom_solver_eval(
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
        if !v.is_finite() {
            return Err(bad("non-finite value"));
        }
        Ok(v)
    }

    fn finites(&mut self, k: usize) -> Result<Vec<f64>, FfiError> {
        (0..k).map(|_| self.finite()).collect()
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
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
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(), FfiError> {
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
    Ok(())
}

/// Checks that all points are 3D and share their rational marker.
fn check_points(pts: &[[f64; 4]], what: &str) -> Result<(), FfiError> {
    let [_, _, _, w0] = *pts.first().ok_or_else(|| bad(format!("{what}: no points")))?;
    for &[_, _, z, w] in pts {
        if z == NOZ {
            return Err(bad(format!("{what}: 2D data")));
        }
        if (w == NOW) != (w0 == NOW) {
            return Err(bad(format!("{what}: mixed rationality")));
        }
        if w != NOW && w <= 0.0 {
            return Err(bad(format!("{what}: non-positive weight")));
        }
    }
    Ok(())
}

fn points(rd: &mut Reader<'_>, k: usize) -> Result<Vec<[f64; 4]>, FfiError> {
    (0..k)
        .map(|_| Ok([rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?]))
        .collect()
}

/// A validated rail: its natural interval and, for a non-rational 2-point degree-1
/// curve, the vector from its first to its last control point (`IsLine`'s direction).
struct Rail {
    ivl: (f64, f64),
    line: Option<[f64; 3]>,
}

/// Reads and validates a curve `n p m cpts knots`.
fn curve(rd: &mut Reader<'_>) -> Result<Rail, FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let pts = points(rd, n + 1)?;
    let knots = rd.finites(m + 1)?;
    check_points(&pts, "curve")?;
    check_knots(&knots, p, "curve")?;
    let a = knots.first().copied().unwrap_or(0.0);
    let b = knots.last().copied().unwrap_or(0.0);
    let line = match (p, pts.as_slice()) {
        (1, [[x0, y0, z0, w0], [x1, y1, z1, _]]) if *w0 == NOW => {
            Some([x1 - x0, y1 - y0, z1 - z0])
        }
        _ => None,
    };
    Ok(Rail { ivl: (a, b), line })
}

/// Whether two rails certainly take `CreateSurface`'s ruled-surface branch: both are
/// non-rational 2-point lines (`IsLine` returns their end-to-end vector) at least
/// `1e-7` long (`IsLine`'s zero-length test is `1e-8`), and the angle between them is
/// below `5e-7`, half of upstream's `SM_EFF_ZERO_SQRT` test.
fn certainly_ruled(r1: &Rail, r2: &Rail) -> bool {
    let (Some(a), Some(b)) = (r1.line, r2.line) else {
        return false;
    };
    let dot = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let (la, lb) = (dot(a, a).sqrt(), dot(b, b).sqrt());
    if !(la >= 1e-7 && lb >= 1e-7 && la.is_finite() && lb.is_finite()) {
        return false;
    }
    let cross = [
        a[1] * b[2] - a[2] * b[1],
        a[2] * b[0] - a[0] * b[2],
        a[0] * b[1] - a[1] * b[0],
    ];
    let sin = dot(cross, cross).sqrt() / (la * lb);
    dot(a, b) > 0.0 && sin < 5e-7
}

/// Reads and validates a surface `n m p q r s cpts u v`.
fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
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
    let pts = points(rd, (n + 1) * (m + 1))?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    check_points(&pts, "surface")?;
    check_knots(&u, p, "surface u")?;
    check_knots(&v, q, "surface v")?;
    Ok(())
}

fn validate_create(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let tol = rd.finite()?;
    if tol < 0.0 {
        return Err(bad("create: negative tolerance"));
    }
    let rail1 = curve(rd)?;
    let rail2 = curve(rd)?;
    if rail1.ivl != rail2.ivl && !certainly_ruled(&rail1, &rail2) {
        return Err(bad("create: rails over different intervals"));
    }
    let k = rd.index(MAX_POINTS)?;
    rd.finites(6 * k)?;
    Ok(())
}

/// Reads the solver block; returns whether both rails share their base.
fn solver(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
    let same = rd.flag()?;
    surface(rd)?;
    if !same {
        surface(rd)?;
    }
    rd.finites(6)?;
    Ok(same)
}

/// The base identity of a selector (`0` A, `1` B, `2` the copy of A).
fn selector_base(sel: usize, same: bool) -> Option<usize> {
    match sel {
        1 | 3 => Some(0),
        2 => Some(usize::from(!same)),
        4 => Some(2),
        _ => None,
    }
}

fn validate_jac(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let same = solver(rd)?;
    let rail = rd.index(3)?;
    let ne = rd.index(MAX_ARRAY)?;
    let np = rd.index(MAX_ARRAY)?;
    let sel1 = rd.index(4)?;
    let off1 = rd.index(MAX_ARRAY)?;
    let sel2 = rd.index(4)?;
    let off2 = rd.index(MAX_ARRAY)?;
    let nx = rd.index(MAX_ARRAY)?;
    let x = rd.finites(nx)?;
    if x.iter().any(|v| v.abs() > MAX_PARAM) {
        return Err(bad("jacobian: crX entry too large"));
    }
    let nf = rd.index(MAX_ARRAY)?;
    rd.finites(nf)?;
    let has_j = rd.flag()?;
    let rows = rd.index(MAX_ARRAY)?;
    let cols = rd.index(MAX_ARRAY)?;

    // the offsets the upstream index logic picks (base of rail 0 is A, of rail 1 B
    // or A)
    let (b1, b2) = (selector_base(sel1, same), selector_base(sel2, same));
    let mut n = np;
    let mut pick = |rail_base: usize, first: (usize, Option<usize>, usize), second| {
        let mut off = n;
        n += 2;
        for (r, b, o) in [first, second] {
            if rail == r && b == Some(rail_base) {
                off = o;
                n -= 2;
            }
        }
        off
    };
    let s1 = pick(0, (0, b1, off1), (1, b2, off2));
    let s2 = pick(usize::from(!same), (1, b1, off1), (0, b2, off2));
    if s1 + 1 >= nx || s2 + 1 >= nx {
        return Err(bad("jacobian: crX too short for the UV offsets"));
    }
    if nf < ne + 3 {
        return Err(bad("jacobian: rF too short"));
    }
    if has_j && (rows < ne + 3 || cols <= s1.max(s2) + 1) {
        return Err(bad("jacobian: matrix too small"));
    }
    Ok(())
}

fn validate_init(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    solver(rd)?;
    let do2 = rd.flag()?;
    rd.index(1)?;
    let sel1 = rd.index(4)?;
    if sel1 == 0 {
        return Err(bad("init: NULL first surface"));
    }
    let s1_index = rd.index(MAX_ARRAY)?;
    let dim = rd.index(MAX_ARRAY)?;
    rd.finites(2 * dim)?;
    let np = rd.index(MAX_ARRAY)?;
    for _ in 0..np {
        rd.flag()?;
    }
    let ng = rd.index(MAX_ARRAY)?;
    let g = rd.finites(ng)?;
    if g.iter().any(|v| v.abs() > MAX_PARAM) {
        return Err(bad("init: guess too large"));
    }
    if do2 && (s1_index + 1 >= ng || s1_index + 1 >= dim || ng + 1 >= dim) {
        return Err(bad("init: guess or interval index out of range"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    match op {
        Op::Create => validate_create(&mut rd)?,
        Op::JacEiffel | Op::JacSecant => validate_jac(&mut rd)?,
        Op::InitEiffel | Op::InitSecant => validate_init(&mut rd)?,
    }
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: well-formed clamped 3D curves and surfaces,
        // every index, count and flag the shim passes to upstream, the array sizes
        // upstream indexes); buf is valid for buf.len() writes and the shim honours the
        // capacity; len is valid.
        unsafe {
            sys::sc_blend_fillet_custom_solver_eval(
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
