//! FFI wrappers for unit `nurbs.b_spline_surface_p1`.
//!
//! Shim: `harness/cpp/shim/nurbs_b_spline_surface_p1.cpp` (`sc_nurbs_b_spline_surface_p1_eval`).
//! Upstream: `source/SMLib/src/SmBSplineSurface.cpp`, `source/SMLib/inc/SmBSplineSurface.h`.
//!
//! The input starts with a surface block saying how `this` is built: `ctor`, then for
//! `ctor != 0` `oob form`, then for `ctor >= 2` a surface. `ctor` is one of [`Ctor`]:
//! `0` none (static ops), `1` `SmBSplineSurface()` (no NURB), `2` / `3` the fast
//! constructor with an owned / borrowed NURB, `4` the `(n, m, p, q, r, s, Pw, U, V)`
//! constructor, `5` `(const gw_SURFACE*)`, `6` the copy constructor of a `2` surface.
//! `form` is set with `SetBSplineSurfaceForm`. A surface is `n m p q r s`,
//! `(n+1)(m+1)` control points `x y z w` (v index fastest), `r+1` u knots, `s+1` v
//! knots. The op's parameters follow:
//!
//! | [`Op`]           | upstream                       | parameters                               |
//! |------------------|--------------------------------|------------------------------------------|
//! | `State`          | (none)                         |                                          |
//! | `Counts`         | `GetDegree`, `GetNumber*`, `IsRational`, `GetOutOfBoundsEnabled`, `GetBSplineSurfaceForm` |  |
//! | `Domain`         | `GetNaturalUVDomain`           |                                          |
//! | `Knots`          | `GetKnots`                     | `v_dir has_ivl a b`                      |
//! | `Greville`       | `GetGrevilleAbscissa`          | `v_dir index`                            |
//! | `GetCanonical`   | `GetCanonical`                 |                                          |
//! | `Net`            | `GetControlPointNet`           |                                          |
//! | `ControlPoints`  | `GetControlPointsPointer`      |                                          |
//! | `Parallel`       | `IsParallelToVector`           | `x y z`                                  |
//! | `BBox`           | `CalculateBoundingBox`         | `D[4] normal pseudo polar has_pg B[9] has_plg nv V[3 nv] expand` |
//! | `Transform`      | `Transform`                    | frame `has_scale sx sy sz`               |
//! | `Eq`             | `operator==`                   | a second surface block                   |
//! | `IsKindOf`       | `IsKindOf`                     | `type`                                   |
//! | `Type`           | `GetType`, `GetClassType`      |                                          |
//! | `MakeNurb`       | `MakeNurb`                     |                                          |
//! | `Copy`           | `Copy`                         |                                          |
//! | `Offset`         | `CreateOffsetSurface`          | `dist tol`                               |
//! | `DegenCurve`     | `IsDegenerateCurve`            | `angle has_dom D[4] tol`                 |
//! | `StepInv`        | `STEPInversion`                | `D[4] P[3] tol has_guess gu gv`          |
//! | `SetForm`        | `SetBSplineSurfaceForm`, `ToggleSwapUVBit`, `RebuildSTEPFromNURBParameters` | `form` |
//! | `Bilinear`       | `CreateBilinearSurface`        | `P00[3] P10[3] P01[3] P11[3]`            |
//! | `Canonical`      | `CreateCanonical`              | `ud vd ncp P[3 ncp] form nu UK[nu] UM[nu] nv VK[nv] VM[nv] kt has_w [nw W[nw]] has_dom D[4]` |
//! | `Cone`           | `CreateConePatch`              | frame `rb rt a0 a1 h quintic`            |
//! | `Sweep`          | `CreateLinearSweep`            | `dim` curve `vx vy vz`                   |
//! | `ReadDb`         | `ReadFromDB`                   | `file_type version nb B[nb]`             |
//!
//! A domain `D` is `umin vmin umax vmax`; a frame is `O[3] X[3] Y[3]`
//! (`SmAxis2Placement::SetCanonical`; its failure status is the op status). A curve is
//! `n p m`, `n+1` points `x y z w`, `m+1` knots. `BBox`'s pseudo guess has basis `B`;
//! its polar guess is the polar box of the vectors `V` added in order. The last five
//! ops are static (`ctor == 0`); the others need a surface. The output is the op status
//! (`0` = `SM_SUCCESS`, else the `SmStatus`), the op's outputs on success, then for
//! surface ops the state of `this`; ops creating a surface output its state on success
//! (`Sweep` then outputs the swept curve's points). A state is `has_nurb oob borrowed
//! knot_type form [n m p q r s cpts u v]`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour:
//! well-formed clamped surfaces (`r = n+p+1`, `s = m+q+1`, `1 <= p <= n`,
//! `1 <= q <= m`, degrees at most 17, finite data, consistent dimension and
//! rationality, non-decreasing knots with end multiplicity exactly `degree + 1` and
//! interior multiplicity at most the degree), 3D surfaces for the ops that evaluate
//! (`BBox`, `Offset`, `DegenCurve`, `StepInv`), a surface without NURB only for the
//! ops that handle one (`State`, `Counts`, `Domain`, `Knots`, `GetCanonical`,
//! `Parallel`, `Eq`, `IsKindOf`, `Type`, `MakeNurb`, `Copy`, `SetForm`), canonical
//! data whose expanded knots give at least one control point per direction once
//! degrees, weight count and point count pass upstream's checks (with a domain inside
//! the knot range), well-formed curves for `Sweep`, and database streams holding every
//! value `ReadFromDB` reads (ASCII: one group per line, plain numbers).

use crate::{call_with_buffer, FfiError};

/// Largest control point index accepted by [`eval`].
pub const MAX_INDEX: usize = 512;
/// Largest NURB degree accepted by [`eval`].
pub const MAX_DEGREE: usize = 17;
/// `NL_NOZ`.
const NOZ: f64 = f64::MAX;
/// `NL_NOW`.
const NOW: f64 = -f64::MAX;

/// How `this` is built.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Ctor {
    /// No surface (static ops).
    None = 0,
    /// `SmBSplineSurface()`: no NURB.
    Empty = 1,
    /// `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`.
    FastOwned = 2,
    /// `SmBSplineSurface(gw_SURFACE*, TRUE, NULL)`.
    FastBorrowed = 3,
    /// `SmBSplineSurface(n, m, p, q, r, s, Pw, U, V)`.
    Data = 4,
    /// `SmBSplineSurface(const gw_SURFACE*)`.
    GwCopy = 5,
    /// The copy constructor of a [`Ctor::FastOwned`] surface.
    Copy = 6,
}

impl Ctor {
    /// The code in the input.
    #[must_use]
    pub fn code(self) -> f64 {
        f64::from(self as i32)
    }
}

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    State,
    Counts,
    Domain,
    Knots,
    Greville,
    GetCanonical,
    Net,
    ControlPoints,
    Parallel,
    BBox,
    Transform,
    Eq,
    IsKindOf,
    Type,
    MakeNurb,
    Copy,
    Offset,
    DegenCurve,
    StepInv,
    SetForm,
    Bilinear,
    Canonical,
    Cone,
    Sweep,
    ReadDb,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::State => 0,
            Op::Counts => 1,
            Op::Domain => 2,
            Op::Knots => 3,
            Op::Greville => 4,
            Op::GetCanonical => 5,
            Op::Net => 6,
            Op::ControlPoints => 7,
            Op::Parallel => 8,
            Op::BBox => 9,
            Op::Transform => 10,
            Op::Eq => 11,
            Op::IsKindOf => 12,
            Op::Type => 13,
            Op::MakeNurb => 14,
            Op::Copy => 15,
            Op::Offset => 16,
            Op::DegenCurve => 17,
            Op::StepInv => 18,
            Op::SetForm => 19,
            Op::Bilinear => 30,
            Op::Canonical => 31,
            Op::Cone => 32,
            Op::Sweep => 33,
            Op::ReadDb => 34,
        }
    }

    /// Whether the op is static (no `this`).
    #[must_use]
    pub fn is_static(self) -> bool {
        matches!(
            self,
            Op::Bilinear | Op::Canonical | Op::Cone | Op::Sweep | Op::ReadDb
        )
    }

    /// Whether the op handles a surface without NURB.
    #[must_use]
    pub fn handles_empty(self) -> bool {
        matches!(
            self,
            Op::State
                | Op::Counts
                | Op::Domain
                | Op::Knots
                | Op::GetCanonical
                | Op::Parallel
                | Op::Eq
                | Op::IsKindOf
                | Op::Type
                | Op::MakeNurb
                | Op::Copy
                | Op::SetForm
        )
    }

    /// Whether the op evaluates the surface (needs a 3D net).
    #[must_use]
    pub fn evaluates(self) -> bool {
        matches!(self, Op::BBox | Op::Offset | Op::DegenCurve | Op::StepInv)
    }
}

mod sys {
    extern "C" {
        pub fn sc_nurbs_b_spline_surface_p1_eval(
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

/// Checks a clamped knot vector of degree `p` with `m + 1` knots.
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
            return Err(bad(format!(
                "{what}: interior multiplicity above the degree"
            )));
        }
    }
    Ok(())
}

/// Checks that all points share their 2D / rational markers; returns whether 3D.
fn check_points(pts: &[[f64; 4]], what: &str) -> Result<bool, FfiError> {
    let [_, _, z0, w0] = *pts
        .first()
        .ok_or_else(|| bad(format!("{what}: no points")))?;
    for &[_, _, z, w] in pts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad(format!("{what}: mixed dimension or rationality")));
        }
    }
    Ok(z0 != NOZ)
}

fn points(rd: &mut Reader<'_>, k: usize) -> Result<Vec<[f64; 4]>, FfiError> {
    (0..k)
        .map(|_| Ok([rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?]))
        .collect()
}

/// Reads and validates a surface `n m p q r s cpts u v`; returns whether it is 3D.
fn surface(rd: &mut Reader<'_>) -> Result<bool, FfiError> {
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
    let three_d = check_points(&pts, "surface")?;
    check_knots(&u, p, "surface u")?;
    check_knots(&v, q, "surface v")?;
    Ok(three_d)
}

/// Reads and validates a curve `n p m cpts knots`.
fn curve(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let pts = points(rd, n + 1)?;
    let knots = rd.finites(m + 1)?;
    check_points(&pts, "curve")?;
    check_knots(&knots, p, "curve")
}

/// Reads a surface block; returns `(ctor, Some(three_d))` (`None` without NURB).
fn surface_block(rd: &mut Reader<'_>) -> Result<(usize, Option<bool>), FfiError> {
    let ctor = rd.index(6)?;
    if ctor == 0 {
        return Ok((0, None));
    }
    rd.flag()?;
    rd.index(11)?;
    let three_d = if ctor == 1 { None } else { Some(surface(rd)?) };
    Ok((ctor, three_d))
}

/// Number tokens of an ASCII database line (plain decimal / scientific numbers only).
fn line_numbers(line: &str, min: usize) -> Result<Vec<f64>, FfiError> {
    if line.len() > 200 {
        return Err(bad("read_db: line too long"));
    }
    let toks: Vec<&str> = line.split_ascii_whitespace().collect();
    if toks.len() < min {
        return Err(bad("read_db: too few values on a line"));
    }
    toks.iter()
        .map(|t| {
            if !t
                .bytes()
                .all(|c| c.is_ascii_digit() || b"+-.eE".contains(&c))
            {
                return Err(bad("read_db: not a plain number"));
            }
            match t.parse::<f64>() {
                Ok(v) if v.is_finite() => Ok(v),
                _ => Err(bad("read_db: not a finite number")),
            }
        })
        .collect()
}

fn int_of(v: f64, max: usize) -> Result<usize, FfiError> {
    if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
        return Err(bad(format!("read_db: bad size {v}")));
    }
    Ok(v as usize)
}

/// Checks that a database stream holds everything `ReadFromDB` reads.
fn check_db(bytes: &[u8], ascii: bool, version: usize) -> Result<(), FfiError> {
    let header = |n: usize, m: usize, p: usize, q: usize| (n, m, p, q);
    if ascii {
        let text = core::str::from_utf8(bytes).map_err(|_| bad("read_db: not text"))?;
        let mut lines = text.split('\n');
        let mut next = |min: usize| -> Result<Vec<f64>, FfiError> {
            line_numbers(
                lines.next().ok_or_else(|| bad("read_db: too few lines"))?,
                min,
            )
        };
        let l0 = next(2)?;
        let l1 = next(2)?;
        let l2 = next(1)?;
        let get = |l: &[f64], i: usize| l.get(i).copied().unwrap_or(-1.0);
        let (n, m, p, q) = header(
            int_of(get(&l0, 0), MAX_INDEX)?,
            int_of(get(&l0, 1), MAX_INDEX)?,
            int_of(get(&l1, 0), MAX_DEGREE)?,
            int_of(get(&l1, 1), MAX_DEGREE)?,
        );
        let rat = get(&l2, 0);
        if version > 32 {
            int_of(get(&next(1)?, 0), 4)?;
            int_of(get(&next(1)?, 0), 11)?;
        }
        if rat != 0.0 && rat != 1.0 {
            if rat.fract() != 0.0 || rat.abs() > 32767.0 {
                return Err(bad("read_db: bad rational flag"));
            }
            return Ok(());
        }
        let per = if rat == 1.0 { 4 } else { 3 };
        for _ in 0..(n + 1) * (m + 1) {
            next(per)?;
        }
        for _ in 0..(n + p + 2) + (m + q + 2) {
            next(1)?;
        }
        Ok(())
    } else {
        let rd8 = |at: usize| -> Result<i64, FfiError> {
            let b = bytes
                .get(at..at + 8)
                .ok_or_else(|| bad("read_db: short header"))?;
            let mut a = [0u8; 8];
            a.copy_from_slice(b);
            Ok(i64::from_ne_bytes(a))
        };
        let rd2 = |at: usize| -> Result<i16, FfiError> {
            let b = bytes
                .get(at..at + 2)
                .ok_or_else(|| bad("read_db: short header"))?;
            Ok(i16::from_ne_bytes([
                b.first().copied().unwrap_or(0),
                b.get(1).copied().unwrap_or(0),
            ]))
        };
        let size = |v: i64, max: usize| -> Result<usize, FfiError> {
            usize::try_from(v)
                .ok()
                .filter(|&x| x <= max)
                .ok_or_else(|| bad("read_db: bad size"))
        };
        let n = size(rd8(0)?, MAX_INDEX)?;
        let m = size(rd8(8)?, MAX_INDEX)?;
        let p = size(i64::from(rd2(16)?), MAX_DEGREE)?;
        let q = size(i64::from(rd2(18)?), MAX_DEGREE)?;
        let rat = rd2(20)?;
        let mut at = 22;
        if version > 32 {
            let kt = rd8(at)?;
            let form = rd8(at + 8)?;
            if !(0..=4).contains(&kt) || !(0..=11).contains(&form) {
                return Err(bad("read_db: knot type or form out of range"));
            }
            at += 16;
        }
        if rat != 0 && rat != 1 {
            return Ok(());
        }
        let per = if rat == 1 { 4 } else { 3 };
        let doubles = (n + 1) * (m + 1) * per + (n + p + 2) + (m + q + 2);
        if bytes.len() < at + 8 * doubles {
            return Err(bad("read_db: short data"));
        }
        Ok(())
    }
}

fn validate_static(op: Op, rd: &mut Reader<'_>) -> Result<(), FfiError> {
    match op {
        Op::Bilinear => {
            rd.finites(12)?;
        }
        Op::Canonical => {
            let ud = rd.index(40)?;
            let vd = rd.index(40)?;
            let ncp = rd.index(MAX_INDEX * 8)?;
            rd.finites(3 * ncp)?;
            rd.index(11)?;
            let nu = rd.index(MAX_INDEX)?;
            let uk = rd.finites(nu)?;
            let mut tu = 0usize;
            for _ in 0..nu {
                tu += rd.index(64)?;
            }
            let nv = rd.index(MAX_INDEX)?;
            let vk = rd.finites(nv)?;
            let mut tv = 0usize;
            for _ in 0..nv {
                tv += rd.index(64)?;
            }
            rd.index(4)?;
            let weights_ok = if rd.flag()? {
                let nw = rd.index(MAX_INDEX * 8)?;
                rd.finites(nw)?;
                nw == 0 || nw == ncp
            } else {
                true
            };
            let has_dom = rd.flag()?;
            let dom = rd.finites(4)?;
            let degrees_ok = (1..=32).contains(&ud) && (1..=32).contains(&vd);
            let nuc = tu.wrapping_sub(ud + 1);
            let nvc = tv.wrapping_sub(vd + 1);
            if degrees_ok && weights_ok && nuc.wrapping_mul(nvc) == ncp {
                if tu <= ud || tv <= vd || nuc == 0 || nvc == 0 {
                    return Err(bad("canonical: need a control point per direction"));
                }
                if uk.windows(2).any(|w| w.first() >= w.last())
                    || vk.windows(2).any(|w| w.first() >= w.last())
                {
                    return Err(bad("canonical: knots not increasing"));
                }
                if has_dom {
                    let (Some(&u0), Some(&u1), Some(&v0), Some(&v1)) =
                        (uk.first(), uk.last(), vk.first(), vk.last())
                    else {
                        return Err(bad("canonical: no knots"));
                    };
                    let [a, b, c, d] = [
                        dom.first().copied().unwrap_or(0.0),
                        dom.get(1).copied().unwrap_or(0.0),
                        dom.get(2).copied().unwrap_or(0.0),
                        dom.get(3).copied().unwrap_or(0.0),
                    ];
                    if !(u0 <= a && a < c && c <= u1 && v0 <= b && b < d && d <= v1) {
                        return Err(bad("canonical: domain outside the knot range"));
                    }
                }
            }
        }
        Op::Cone => {
            rd.finites(14)?;
            rd.flag()?;
        }
        Op::Sweep => {
            let dim = rd.index(3)?;
            if dim == 0 {
                return Err(bad("sweep: dimension 0"));
            }
            curve(rd)?;
            rd.finites(3)?;
        }
        Op::ReadDb => {
            let ascii = !rd.flag()?;
            let version = rd.index(100)?;
            let nb = rd.index(1 << 20)?;
            let bytes: Vec<u8> = (0..nb)
                .map(|_| rd.index(255).map(|b| b as u8))
                .collect::<Result<_, _>>()?;
            check_db(&bytes, ascii, version)?;
        }
        _ => return Err(bad("not a static op")),
    }
    Ok(())
}

fn validate_surface_op(op: Op, rd: &mut Reader<'_>, three_d: Option<bool>) -> Result<(), FfiError> {
    match three_d {
        None if !op.handles_empty() => return Err(bad("op needs a NURB")),
        Some(false) if op.evaluates() => return Err(bad("op needs a 3D surface")),
        _ => {}
    }
    match op {
        Op::State
        | Op::Counts
        | Op::Domain
        | Op::GetCanonical
        | Op::Net
        | Op::ControlPoints
        | Op::Type
        | Op::MakeNurb
        | Op::Copy => {}
        Op::Knots => {
            rd.flag()?;
            rd.flag()?;
            rd.finites(2)?;
        }
        Op::Greville => {
            rd.flag()?;
            rd.index(MAX_INDEX + 4)?;
        }
        Op::Parallel => {
            rd.finites(3)?;
        }
        Op::BBox => {
            rd.finites(4)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
            rd.finites(9)?;
            rd.flag()?;
            let nv = rd.index(16)?;
            rd.finites(3 * nv)?;
            rd.flag()?;
        }
        Op::Transform => {
            rd.finites(9)?;
            rd.flag()?;
            rd.finites(3)?;
        }
        Op::Eq => {
            let (ctor, _) = surface_block(rd)?;
            if ctor == 0 {
                return Err(bad("eq: the other surface needs a constructor"));
            }
        }
        Op::IsKindOf => {
            rd.index(1 << 20)?;
        }
        Op::Offset => {
            rd.finite()?;
            if rd.finite()? <= 0.0 {
                return Err(bad("offset: need a positive tolerance"));
            }
        }
        Op::DegenCurve => {
            if rd.finite()? < 0.0 {
                return Err(bad("degenerate: negative angle"));
            }
            rd.flag()?;
            rd.finites(4)?;
            if rd.finite()? < 0.0 {
                return Err(bad("degenerate: negative tolerance"));
            }
        }
        Op::StepInv => {
            rd.finites(7)?;
            if rd.finite()? <= 0.0 {
                return Err(bad("step inversion: need a positive tolerance"));
            }
            rd.flag()?;
            rd.finites(2)?;
        }
        Op::SetForm => {
            rd.index(11)?;
        }
        _ => return Err(bad("not a surface op")),
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let (ctor, three_d) = surface_block(&mut rd)?;
    if (ctor == 0) != op.is_static() {
        return Err(bad("static ops need ctor 0, surface ops a constructor"));
    }
    if ctor == 0 {
        validate_static(op, &mut rd)?;
    } else {
        validate_surface_op(op, &mut rd, three_d)?;
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
        // reads for `op` (validated above: surface blocks with well-formed clamped
        // surfaces, every index, count and flag the shim passes to upstream, NURB-less
        // surfaces only for ops that handle them, complete database streams); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nurbs_b_spline_surface_p1_eval(
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
