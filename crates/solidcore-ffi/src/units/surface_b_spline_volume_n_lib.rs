//! FFI wrappers for unit `surface.b_spline_volume_n_lib`.
//!
//! Shim: `harness/cpp/shim/surface_b_spline_volume_n_lib.cpp`
//! (`sc_surface_b_spline_volume_n_lib_eval`). Upstream:
//! `source/SMLib/src/SmBSplineVolumeNLib.cpp`.
//!
//! Every op starts with a volume block, as in unit `surface.b_spline_volume`: `l m n p q
//! r` (highest control point indices and degrees), the knots `U` (`l+p+2` values), `V`
//! (`m+q+2`), `W` (`n+r+2`), the control points `x y z w` with the `w` index fastest,
//! then `v`, then `u`; `has_orient [O(12)]` and `n_next` followed by `T(12)` per next
//! map (`origin X Y Z` of an `SmTransform`). Every output starts with the status (`0` =
//! `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]      | upstream (through `SmVolume*`)                          | parameters        | outputs |
//! |-------------|---------------------------------------------------------|-------------------|---------|
//! | `Conts`     | `CalculateContinuitiesSimple(e, min, params, conts, geom)` (outputs preset to `G1`, `[-7, 9]`, `[C0]`) | `e geom` | `dir` |
//! | `HasDisc`   | `HasDiscontinuitiesSimple(given ? &d : NULL, geom)` (`d` after `SetUnaligned`) | `given geom` | `flag [has min dir_u dir_v dir_w]` |
//! | `CurveIn`   | `IsCurveInParamDomainSimple(c)` (`which` 0), `IsCurveInParamDomain(c, comp)` (1), `IsCurveInInSpaceDomain(c, comp)` (2) | `which comp curve` | `flag` |
//!
//! A `dir` is `unaligned min n params[n] n conts[n]` (continuities as `SmContinuityType`
//! codes). A curve block is `0 n p cpts(x y z w)[n+1] knots[n+p+2]` (an
//! `SmBSplineCurve(gw_CURVE*, 3, FALSE)`), `1 P V d0 d1` (a bounded `SmLine(P, V, [d0,
//! d1], 1, 3, ctx, NULL, FALSE)`), `2 C X Y d0 d1 rx ry` (an `SmEllipse(C, X, Y, [d0,
//! d1], rx, ry, 3, ctx, NULL, FALSE)`) or `3 P1 D1 P2 D2` (an `SmHermiteCurve`, dimension
//! 3).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: finite
//! values of magnitude at most [`MAX_COORD`]; volumes with at most [`MAX_CP`] control
//! points and degree [`MAX_DEGREE`] per direction, non-decreasing knots, at most
//! [`MAX_NEXT`] next maps; `SmVolumeParamType` codes `0..=8`; B-spline curves with `1 <=
//! p <= n`, `n <` [`MAX_CURVE_CP`], `p <=` [`MAX_DEGREE`], positive weights, clamped
//! knots with interior multiplicity at most `p` over a non-trivial range; lines with
//! non-zero directions; ellipses with non-zero axes, positive radii and domain ends
//! within [`MAX_ANGLE`].

use crate::{call_with_buffer, FfiError};

/// Most control points per volume direction.
pub const MAX_CP: usize = 8;
/// Highest degree per direction (volumes and curves).
pub const MAX_DEGREE: usize = 5;
/// Most next maps.
pub const MAX_NEXT: usize = 3;
/// Most control points of a B-spline curve.
pub const MAX_CURVE_CP: usize = 32;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1e8;
/// Largest magnitude of an ellipse domain end (degrees).
pub const MAX_ANGLE: f64 = 720.0;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Op {
    /// `CalculateContinuitiesSimple`.
    Conts,
    /// `HasDiscontinuitiesSimple`.
    HasDisc,
    /// `IsCurveInParamDomainSimple`, `IsCurveInParamDomain`, `IsCurveInInSpaceDomain`.
    CurveIn,
}

impl Op {
    /// The shim's op code.
    #[must_use]
    pub fn code(self) -> i32 {
        match self {
            Op::Conts => 0,
            Op::HasDisc => 1,
            Op::CurveIn => 2,
        }
    }
}

mod sys {
    extern "C" {
        pub fn sc_surface_b_spline_volume_n_lib_eval(
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

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        if !v.is_finite() || v.abs() > MAX_COORD {
            return Err(bad("value not finite or too large"));
        }
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v < 0.0 || v.fract() != 0.0 || v > max as f64 {
            return Err(bad(format!("index {v} outside 0..={max}")));
        }
        Ok(v as usize)
    }

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.num()? != 0.0)
    }

    fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.num()?, self.num()?, self.num()?])
    }

    fn nonzero3(&mut self) -> Result<(), FfiError> {
        if self.vec3()?.iter().all(|&c| c == 0.0) {
            return Err(bad("zero vector"));
        }
        Ok(())
    }

    fn take(&mut self, n: usize) -> Result<Vec<f64>, FfiError> {
        (0..n).map(|_| self.num()).collect()
    }

    /// A volume block.
    fn volume(&mut self) -> Result<(), FfiError> {
        let mut hi = [0usize; 3];
        for h in &mut hi {
            *h = self.index(MAX_CP - 1)?;
        }
        let mut deg = [0usize; 3];
        for d in &mut deg {
            *d = self.index(MAX_DEGREE)?;
        }
        for (h, d) in hi.iter().zip(deg) {
            // knots U[0..=h+d+1], non-decreasing
            let k = self.take(h + d + 2)?;
            if k.windows(2).any(|w| w.first() > w.last()) {
                return Err(bad("knots must be non-decreasing"));
            }
        }
        let count = (hi[0] + 1) * (hi[1] + 1) * (hi[2] + 1);
        self.take(4 * count)?;
        if self.flag()? {
            self.take(12)?;
        }
        let n_next = self.index(MAX_NEXT)?;
        self.take(12 * n_next)?;
        Ok(())
    }

    /// A clamped B-spline curve `n p cpts knots`.
    fn bspline(&mut self) -> Result<(), FfiError> {
        let n = self.index(MAX_CURVE_CP - 1)?;
        let p = self.index(MAX_DEGREE)?;
        if p == 0 || p > n {
            return Err(bad("curve: need 1 <= p <= n"));
        }
        let m = n + p + 1;
        let cps = self.take(4 * (n + 1))?;
        for c in cps.chunks(4) {
            let w = c.get(3).copied().unwrap_or(0.0);
            if w <= 1e-3 {
                return Err(bad("curve: non-positive weight"));
            }
            if c.iter().take(3).any(|x| (x / w).abs() > MAX_COORD) {
                return Err(bad("curve: Euclidean point out of range"));
            }
        }
        let knots = self.take(m + 1)?;
        if knots.windows(2).any(|w| w.first() > w.last()) {
            return Err(bad("curve: knots not non-decreasing"));
        }
        let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
            return Err(bad("empty knots"));
        };
        let short = || bad("curve: knots too short");
        let head = knots.get(..=p).ok_or_else(short)?;
        let tail = knots.get(m - p..).ok_or_else(short)?;
        let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
        if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
            return Err(bad("curve: knots not clamped"));
        }
        if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
            return Err(bad("curve: parameter range too small"));
        }
        if inner.iter().any(|&k| k == a || k == b) {
            return Err(bad("curve: end multiplicity above p + 1"));
        }
        let mut run = 1;
        for w in inner.windows(2) {
            run = if w.first() == w.last() { run + 1 } else { 1 };
            if run > p {
                return Err(bad("curve: interior multiplicity above the degree"));
            }
        }
        Ok(())
    }

    /// A curve block (see the module docs).
    fn curve(&mut self) -> Result<(), FfiError> {
        match self.index(3)? {
            0 => self.bspline(),
            1 => {
                self.vec3()?;
                self.nonzero3()?;
                self.num()?;
                self.num()?;
                Ok(())
            }
            2 => {
                self.vec3()?;
                self.nonzero3()?;
                self.nonzero3()?;
                for _ in 0..2 {
                    if self.num()?.abs() > MAX_ANGLE {
                        return Err(bad("ellipse: domain end too large"));
                    }
                }
                for _ in 0..2 {
                    if self.num()? <= 0.0 {
                        return Err(bad("ellipse: radius not positive"));
                    }
                }
                Ok(())
            }
            _ => {
                self.take(12)?;
                Ok(())
            }
        }
    }
}

/// Checks that `input` is a well-formed record for `op` (see the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] naming the first problem.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut r = Reader::new(input);
    r.volume()?;
    match op {
        Op::Conts => {
            r.index(8)?;
            r.flag()?;
        }
        Op::HasDisc => {
            r.flag()?;
            r.flag()?;
        }
        Op::CurveIn => {
            r.index(2)?;
            r.flag()?;
            r.curve()?;
        }
    }
    if r.at != input.len() {
        return Err(bad(format!(
            "input has {} values, op reads {}",
            input.len(),
            r.at
        )));
    }
    Ok(())
}

/// Runs `op` of the shim on `input` (layouts in the module docs) and returns its
/// outputs, starting with the status.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects; the shim's status.
pub fn eval(op: Op, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(op, input)?;
    call_with_buffer(1024, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (validated above: finite bounded values, volume and curve
        // records whose sizes match their counts and degrees within the limits, at most
        // MAX_NEXT next maps, enum codes in range); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_b_spline_volume_n_lib_eval(
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
