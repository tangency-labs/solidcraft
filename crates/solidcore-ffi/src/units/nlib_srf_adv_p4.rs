//! FFI wrappers for unit `nlib.srf_adv_p4`.
//!
//! Shim: `harness/cpp/shim/nlib_srf_adv_p4.cpp` (`sc_nlib_srf_adv_p4_eval`).
//! Upstream: `source/SMLib/src/NL_SrfAdv.cpp`.
//!
//! | [`Op`]          | upstream                         | input                                     |
//! |-----------------|----------------------------------|-------------------------------------------|
//! | `BVectors`      | `N_CreateBVectors`               | curve, `B0`, `k`, `t[0..=k]`              |
//! | `Coons`         | `N_CreateCoonsSrf`               | curves `U0 U1 V0 V1`                      |
//! | `SkinDerivs`    | `N_CreateSkinSrfBoundaryDerivs`  | curve, `V`, `bndy`                        |
//! | `MergeKnots`    | `N_MergeKnotVectors`             | `p`, `cnt R`, `R`, `cnt S`, `S`           |
//! | `RotationRate`  | `ST_GetRotationRateOfChange`     | traj, `u`, `0` curve Z / `1` Z samples    |
//! | `SweepTangent`  | `N_SweepCrossTangentCrv`         | `dtraj scale dscale`, `rma drma`, curve   |
//! | `AdjustDeriv`   | `N_AdjustDerivSrf`               | surface, `dir cst usg inplace`            |
//! | `AdjustWeight`  | `N_AdjustWeightScale`            | `Pw Qw Rw` (x y z w each)                 |
//! | `Approx`        | `N_ApproxCrossBoundaryDerivs`    | curve, `Ds De eps inplace`                |
//! | `CoonsBoundary` | `N_CreateCoonsBoundaryCrvs`      | curves `L dL R dR B dB T dT`, `cpf`       |
//! | `CrossDeriv`    | `N_CreateCrossBoundaryDerivCrv`  | curve, `Ns Ne Bs Be`                      |
//!
//! A surface is encoded `n m p q`, `(n+1)(m+1)` control points `x y z w` (row-major),
//! `n+p+2` u knots, `m+q+2` v knots; a curve `n p`, `n+1` control points, `n+p+2`
//! knots; a point `x y z`; a 3x3 matrix row-major. [`eval`] accepts only finite input
//! and well-formed clamped surfaces and curves (degrees `1..=NL_MAXDEG`, more control
//! points than the degree, non-decreasing knots with end multiplicity exactly `p+1`
//! and interior multiplicity at most `p`; curve weights all `NOW` or all positive). It
//! rejects every argument that would make
//! upstream index outside its arrays: flags outside their ranges (`bndy`, `cst` and
//! `usg` in `1..=4`, `dir` in `1..=2`), an empty parameter list, decreasing knots or
//! fewer than two knots for `MergeKnots`, and a negative `eps`; for `Approx` also
//! end angles upstream cannot classify (see `approx_angles_ok`).

use crate::validate::{bad, euclid, NOW};
use crate::{call_with_buffer, FfiError};

/// Largest highest net index (`n`, `m`) of a surface accepted by [`eval`].
pub const MAX_INDEX: usize = 256;
/// Largest highest control point index of a curve accepted by [`eval`].
pub const MAX_CRV_INDEX: usize = 4096;
/// Upstream `NL_MAXDEG`.
pub const MAX_DEGREE: usize = 17;
/// Largest number of parameters of `BVectors`.
pub const MAX_PARAMS: usize = 1024;
/// Largest knot count of `MergeKnots`.
pub const MAX_KNOTS: usize = 4096;

/// The upstream routine run by the shim.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `N_CreateBVectors`.
    BVectors,
    /// `N_CreateCoonsSrf`.
    Coons,
    /// `N_CreateSkinSrfBoundaryDerivs`.
    SkinDerivs,
    /// `N_MergeKnotVectors`.
    MergeKnots,
    /// `ST_GetRotationRateOfChange`.
    RotationRate,
    /// `N_SweepCrossTangentCrv`.
    SweepTangent,
    /// `N_AdjustDerivSrf`.
    AdjustDeriv,
    /// `N_AdjustWeightScale`.
    AdjustWeight,
    /// `N_ApproxCrossBoundaryDerivs`.
    Approx,
    /// `N_CreateCoonsBoundaryCrvs`.
    CoonsBoundary,
    /// `N_CreateCrossBoundaryDerivCrv`.
    CrossDeriv,
}

impl Op {
    fn code(self) -> i32 {
        match self {
            Op::BVectors => 0,
            Op::Coons => 1,
            Op::SkinDerivs => 2,
            Op::MergeKnots => 3,
            Op::RotationRate => 4,
            Op::SweepTangent => 5,
            Op::AdjustDeriv => 6,
            Op::AdjustWeight => 7,
            Op::Approx => 8,
            Op::CoonsBoundary => 9,
            Op::CrossDeriv => 10,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_nlib_srf_adv_p4_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!();

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() {
            return Err(bad("non-finite input"));
        }
        Ok(v)
    }

    fn index(&mut self, lo: usize, hi: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < lo as f64 || v > hi as f64 {
            return Err(bad(format!("bad index/count/flag {v}")));
        }
        Ok(v as usize)
    }
}

/// `N_KnotVectorIsValid` for degree `p` (end multiplicities exactly `p + 1`,
/// non-decreasing, interior multiplicities at most `p`).
fn valid_knots(u: &[f64], p: usize) -> bool {
    let (Some(&first), Some(&last)) = (u.first(), u.last()) else {
        return false;
    };
    if u.len() < 2 * p + 2 || first >= last || u.windows(2).any(|w| w.first() > w.get(1)) {
        return false;
    }
    let mut runs: Vec<(f64, usize)> = Vec::new();
    for &x in u {
        match runs.last_mut() {
            Some((y, k)) if *y == x => *k += 1,
            _ => runs.push((x, 1)),
        }
    }
    runs.iter().all(|&(x, k)| {
        if x == first || x == last {
            k == p + 1
        } else {
            k <= p
        }
    })
}

fn surface(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(1, MAX_INDEX)?;
    let m = rd.index(1, MAX_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    let q = rd.index(1, MAX_DEGREE)?;
    if p > n || q > m {
        return Err(bad("degree above the highest net index"));
    }
    rd.slice(4 * (n + 1) * (m + 1))?;
    let u = rd.slice(n + p + 2)?;
    let v = rd.slice(m + q + 2)?;
    if !valid_knots(u, p) || !valid_knots(v, q) {
        return Err(bad("invalid knot vector"));
    }
    Ok(())
}

/// A validated curve: degree, control points `x y z w` and knots.
type CrvShape<'a> = (usize, &'a [f64], &'a [f64]);

fn curve<'a>(rd: &mut Reader<'a>) -> Result<CrvShape<'a>, FfiError> {
    let n = rd.index(1, MAX_CRV_INDEX)?;
    let p = rd.index(1, MAX_DEGREE)?;
    if p > n {
        return Err(bad("degree above the highest control point index"));
    }
    let pts = rd.slice(4 * (n + 1))?;
    // Weights: all NOW (non-rational) or all positive; a zero or negative weight in
    // a curve is no valid curve (upstream then reads uninitialised data).
    let ws: Vec<f64> = pts.chunks(4).filter_map(|c| c.get(3).copied()).collect();
    if !(ws.iter().all(|&w| w == NOW) || ws.iter().all(|&w| w > 0.0)) {
        return Err(bad("mixed or non-positive weights"));
    }
    let u = rd.slice(n + p + 2)?;
    if !valid_knots(u, p) {
        return Err(bad("invalid knot vector"));
    }
    Ok((p, pts, u))
}

/// Upstream's `N_VectorsAngle(a, b)` in degrees, or `None` where it fails
/// (`N_FloatOpIsBad(a.b, |a||b|, NL_DIVISION)`); same operations in the same order.
fn vectors_angle(a: [f64; 3], b: [f64; 3]) -> Option<f64> {
    let dot3 = |u: [f64; 3], v: [f64; 3]| u[0] * v[0] + u[1] * v[1] + u[2] * v[2];
    let dot = dot3(a, b);
    let (ma, mb) = (dot3(a, a).sqrt(), dot3(b, b).sqrt());
    let (x, y) = (dot.abs(), (ma * mb).abs());
    let bad = if y == 0.0 {
        true
    } else if x == 0.0 {
        false
    } else {
        !((y <= 1.0 && x < f64::MAX * y) || (y >= 1.0 && x > f64::MIN_POSITIVE * y))
    };
    if bad {
        return None;
    }
    // Same as upstream's two `if`s (NaN stays NaN).
    let arg = (dot / (ma * mb)).clamp(-1.0, 1.0);
    Some(180.0 * (arg.acos() / NL_PI))
}

/// Upstream `NL_PI`.
const NL_PI: f64 = std::f64::consts::PI;

/// `N_ApproxCrossBoundaryDerivs` reads an uninitialised angle when `N_VectorsAngle`
/// of its end control points fails (its status is ignored), and an uninitialised
/// knot when the end angles are neither both within `eps` nor each strictly on one
/// side of it (an angle equal to `eps`, or NaN). Refuse the inputs where the first
/// pass (`As`/`Bs` at the start, `Ae`/`Be` at the end) hits either case.
fn approx_angles_ok(crv: CrvShape<'_>, ds: &[f64], de: &[f64], eps: f64) -> Result<(), FfiError> {
    let (p, pts, u) = crv;
    let pt = |i: usize| -> Result<[f64; 3], FfiError> {
        match pts.get(4 * i..4 * i + 4) {
            Some(&[x, y, z, w]) => Ok(euclid([x, y, z, w])),
            _ => Err(bad("input too short")),
        }
    };
    let vec3 = |v: &[f64]| -> Result<[f64; 3], FfiError> {
        match *v {
            [x, y, z] => Ok([x, y, z]),
            _ => Err(bad("input too short")),
        }
    };
    let kn = |i: usize| u.get(i).copied().ok_or_else(|| bad("input too short"));
    let n = pts.len() / 4 - 1;
    let fs = (kn(p + 1)? - kn(p)?) / p as f64;
    let fe = (kn(n + 1)? - kn(n)?) / p as f64;
    let along =
        |a: [f64; 3], t: f64, v: [f64; 3]| [a[0] + t * v[0], a[1] + t * v[1], a[2] + t * v[2]];
    let bs = along(pt(0)?, fs, vec3(ds)?);
    let be = along(pt(n)?, -fe, vec3(de)?);
    let (Some(als), Some(ale)) = (vectors_angle(pt(1)?, bs), vectors_angle(pt(n - 1)?, be)) else {
        return Err(bad(
            "end derivative angle undefined (upstream reads an uninitialised angle)",
        ));
    };
    let within = als <= eps && ale <= eps;
    let split = (als > eps && ale < eps) || (als < eps && ale > eps) || (als > eps && ale > eps);
    if !within && !split {
        return Err(bad(
            "end angle equal to the tolerance (upstream reads an uninitialised knot)",
        ));
    }
    Ok(())
}

fn knots(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let cnt = rd.index(2, MAX_KNOTS)?;
    let k = rd.slice(cnt)?;
    if k.windows(2).any(|w| w.first() > w.get(1)) {
        return Err(bad("decreasing knots"));
    }
    Ok(())
}

fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    match op {
        Op::BVectors => {
            curve(&mut rd)?;
            rd.slice(3)?;
            let k = rd.index(0, MAX_PARAMS - 1)?;
            rd.slice(k + 1)?;
        }
        Op::Coons => {
            for _ in 0..4 {
                curve(&mut rd)?;
            }
        }
        Op::SkinDerivs => {
            curve(&mut rd)?;
            rd.slice(3)?;
            rd.index(1, 4)?;
        }
        Op::MergeKnots => {
            rd.index(0, MAX_DEGREE)?;
            knots(&mut rd)?;
            knots(&mut rd)?;
        }
        Op::RotationRate => {
            curve(&mut rd)?;
            rd.num()?;
            if rd.index(0, 1)? == 0 {
                curve(&mut rd)?;
            } else {
                rd.slice(3)?;
                rd.index(0, 1)?;
                rd.slice(4)?;
                rd.index(0, 1)?;
                rd.slice(4)?;
            }
        }
        Op::SweepTangent => {
            rd.slice(9 + 18)?;
            curve(&mut rd)?;
        }
        Op::AdjustDeriv => {
            surface(&mut rd)?;
            rd.index(1, 2)?;
            rd.index(1, 4)?;
            rd.index(1, 4)?;
            rd.index(0, 1)?;
        }
        Op::AdjustWeight => {
            rd.slice(12)?;
        }
        Op::Approx => {
            let crv = curve(&mut rd)?;
            let ds = rd.slice(3)?;
            let de = rd.slice(3)?;
            let eps = rd.num()?;
            if eps < 0.0 {
                return Err(bad("negative angular tolerance"));
            }
            approx_angles_ok(crv, ds, de, eps)?;
            rd.index(0, 1)?;
        }
        Op::CoonsBoundary => {
            for _ in 0..8 {
                curve(&mut rd)?;
            }
            rd.index(0, 1)?;
        }
        Op::CrossDeriv => {
            curve(&mut rd)?;
            rd.slice(12)?;
        }
    }
    rd.done()
}

/// Runs `op` on `input`; returns the shim's output record.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the
        // shim reads for `op` (validated above, including surface and curve shapes,
        // knot vectors, flags and every count upstream uses for indexing); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_nlib_srf_adv_p4_eval(
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
