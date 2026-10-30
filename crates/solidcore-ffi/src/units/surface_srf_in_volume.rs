//! FFI wrappers for unit `surface.srf_in_volume`.
//!
//! Shim: `harness/cpp/shim/surface_srf_in_volume.cpp` (`sc_surface_srf_in_volume_eval`).
//! Upstream: `source/SMLib/src/SmSrfInVolume.cpp`, `source/SMLib/inc/SmSrfInVolume.h`.
//!
//! Every input starts with a block building the `SmSrfInVolume` `c` the op runs on: a
//! surface block (`0 oob form n m p q r s cpts(x y z w) u v`: an
//! `SmBSplineSurface(gw_SURFACE*, FALSE, NULL)`, `1 O[3] N[3]`: `SmPlane(O, N)`, or
//! `2 O[3] X[3] Y[3] r start end h swap inside`: `SmCylinder(O, X, Y, r, start, end, h,
//! swap, inside, FALSE)` with its NURB made), a
//! volume block (`n` maps, each `kind [T[12]] has_orient [O[12]]` with kind `0` a plain
//! `SmVolume`, `1` an `SmTransform(ctx, origin, X, Y, &Z)` from the twelve numbers `T`,
//! `2` a default `SmTwistVolume`, `3` a default `SmUnbendVolume`, `4` a default
//! `SmBendVolume`, `5` / `6` / `7` the component constructors of `SmTwistVolume`
//! (`origin axis mid period min_dist`), `SmUnbendVolume` and `SmBendVolume` (`center axis
//! mid neutral_dist singularity_tol`), their eleven numbers `C` following the kind; `O`
//! becomes the orient map (not for the component kinds, which build their own); maps are chained with `AddNextMap(map, 2)`, as unit `surface.volume_p1`), then
//! `in_param copy_flag owner_flag` (the constructor's arguments). Every output starts
//! with the status (`0` = `SM_SUCCESS`, else the `SmStatus`); the rest:
//!
//! | [`Op`]        | upstream                                   | parameters                    | outputs |
//! |---------------|--------------------------------------------|-------------------------------|---------|
//! | `Info`        | `GetType`, `GetDegree`, `Get*UVDomain`, `GetMaxAnalyticDomain`, `Owns*`, `ConvertUVFrom*`, `IsKindOf` | `uv nt T[nt]` | `type du dv natural[4] step[4] max[4] owns_s owns_v st_a uv st_b uv kind[nt]` state |
//! | `Eval`        | `Evaluate`                                 | `uv hu hv ul vl oh nz`        | defined entries of `D[hu+1][hv+1]` (on success) |
//! | `EvalSimple`  | `EvaluateSimple`                           | `uv hu hv ul vl oh`           | as `Eval` |
//! | `EvalPoint`   | `EvaluatePoint`                            | `uv`                          | point |
//! | `Normal`      | `EvaluateNormal`                           | `uv ul vl`                    | normal |
//! | `Knots`       | `GetKnots`                                 | `v_dir has_ivl a b`           | `nk K nm M` |
//! | `StepInv`     | `STEPInversion`                            | `dom P tol`                   | `uv loc` |
//! | `Reverse`     | `Reverse`                                  | `v_dir`                       | state |
//! | `SwapUv`      | `SwapUV`                                   |                               | state |
//! | `Trim`        | `TrimWithDomain`                           | `dom`                         | `dom'` state |
//! | `Transform`   | `Transform`                                | `O X Y has_scale S`           | state |
//! | `Split`       | `SplitAt`                                  | `v_dir t`                     | state of each half |
//! | `Copy`        | `Copy` (copy constructor)                  |                               | state of the copy |
//! | `CopyCtor`    | the copy constructor `SmSrfInVolume(const SmSrfInVolume&)`, called directly (not through `Copy`) | | state of the copy, `flag` (original == copy) |
//! | `Equal`       | `operator==`                               | `self` [object block]         | flag |
//! | `SetOwner`    | `SetOwnerFlag`                             | `flag`                        | state |
//! | `MakeOwner`   | `MakeOwner`                                |                               | state |
//! | `SetBase`     | `SetBaseSurface`                           | surface block `in_param copy owns` | state |
//! | `SetVolume`   | `SetVolume`                                | volume block `copy owns`      | state |
//! | `Mirror`      | `CreateMirrorSurface`                      | `O X Y`                       | state of the mirrored surface |
//! | `Exact`       | `MakeExactBSplineIfPossible`               |                               | on success: `made [surface]` |
//! | `AssertValid` | `AssertValid(&list, level, SM_NO_WALK)` (virtual, through `SmSurface*`) | `level` (`0`, `1`, `2` or `4`) | `ok n` then per report `type list test assert_type ok reporter owner other owner_type other_type tol value dist3d name message message_for_file owner_string other_string` (texts `len chars...`; objects `0` null, `1` the object, `9` another), then `last_add` |
//! | `WriteDb`     | `WriteToDB` (ASCII, 17 digits; a generator helper) | `version`            | the bytes |
//! | `ReadDb`      | `SmSrfInVolume::ReadFromDB` (static, ASCII) | `version nb B[nb]` (no object block) | `made [state]` |
//!
//! `uv` is two numbers, `dom` `u0 v0 u1 v1`, `ul vl oh nz` flags. A state is
//! `owner_flag in_param` the base surface (`type has_nurb [n m p q r s cpts knots]`)
//! and the volume chain (`n` then per map `type [matrix(16) if a transform | two members
//! if a twist, unbend or bend volume] has_orient [O(16)] has_inv [I(16)]`).
//! `Exact` returns `made = 0` for a successful null result; otherwise its surface
//! record is `type has_nurb [n m p q r s cpts(x y z w) u_knots v_knots]`, including
//! the concrete derived type (for example, a plane) rather than a B-spline stand-in,
//! then `dom[4] finite` and, for a finite natural domain, `st P st N` of
//! `EvaluatePoint` / `EvaluateNormal(uv, TRUE, TRUE)` at the 3 x 3 grid of interval
//! fractions `0, 0.5, 1`.
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: finite
//! values, well-formed clamped B-splines, planes with a non-zero normal, derivative
//! orders up to 3 (upstream's fixed arrays), a plain `SmVolume` head map only where
//! nothing copies the volume (its `Copy` fails and the copy would share it), except
//! for `Copy` and `CopyCtor` where the result must not own the shared volume (the
//! copy constructor attempts that copy even for owner flag 1, whose result does not
//! own what the failed `Copy` left shared),
//! `STEPInversion` through any map kind (no domain box reaches the volume's
//! inverse solves), domains and split
//! parameters inside the base B-spline's domain, placements with independent axes, and
//! database text that is a complete ASCII record without a plain `SmVolume` or a
//! B-spline volume (reading a plain volume fills a null volume).
//! A cylinder base needs independent placement axes, a positive radius and height and
//! `-360 <= start < end <= start + 360`; it runs only `Info`, the evaluation ops (it
//! is there for the analytic base's virtuals the object forwards to, such as
//! `ConvertUVFrom*`) and `Exact` (an unbend maps a cylinder about its axis to a
//! plane), not `SetBase`.

use crate::validate::{bad, check_knots, NOW};
use crate::{call_with_buffer, FfiError};

/// Most maps in a volume chain.
pub const MAX_MAPS: usize = 3;
/// Largest control point index.
pub const MAX_INDEX: usize = 16;
/// Largest degree.
pub const MAX_DEGREE: usize = 5;
/// Largest coordinate magnitude.
pub const MAX_COORD: f64 = 1.0e6;
/// Most bytes of a database stream.
pub const MAX_DB_BYTES: usize = 1 << 18;
/// Largest derivative order `Evaluate` accepts.
pub const MAX_DERIV: usize = 3;

/// Shim operations.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Op {
    Info,
    Eval,
    EvalSimple,
    EvalPoint,
    Normal,
    Knots,
    StepInv,
    Reverse,
    SwapUv,
    Trim,
    Transform,
    Split,
    Copy,
    Equal,
    SetOwner,
    MakeOwner,
    SetBase,
    SetVolume,
    Mirror,
    Exact,
    /// The copy constructor `SmSrfInVolume(const SmSrfInVolume&)`, invoked directly.
    CopyCtor,
    /// `AssertValid` (the shim refuses, at levels `0..=2`, a base surface whose
    /// inherited `SmSurface` checks read outputs a failed call left unwritten).
    AssertValid,
    /// `SmSrfInVolume::ReadFromDB` (static, ASCII).
    ReadDb = 40,
    /// `WriteToDB` of an object block (a generator helper).
    WriteDb = 41,
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
        pub fn sc_surface_srf_in_volume_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

/// What a validated object block says about the object.
#[derive(Debug, Clone, Copy)]
#[allow(clippy::struct_excessive_bools)]
struct Block {
    /// The base surface's parameter domain `[u0, v0, u1, v1]` (`None` for a plane).
    domain: Option<[f64; 4]>,
    /// The head map is a plain `SmVolume`.
    head_base: bool,
    /// Every map is an `SmTransform`.
    all_transforms: bool,
    /// Some map is a plain `SmVolume`.
    any_base: bool,
    /// The constructor copies the volume.
    copies_volume: bool,
    /// The copy constructor's result owns the volume: the constructor's effective owner
    /// flag (`owner_flag | copy_flag`, both canonical `0..=3`) is 2 or 3.
    owns_volume: bool,
    /// The base surface is a cylinder.
    cylinder: bool,
}

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

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    /// A `uv` point of bounded size.
    fn uv(&mut self) -> Result<[f64; 2], FfiError> {
        Ok([self.finite()?, self.finite()?])
    }

    /// A placement `O X Y`: independent axes.
    fn placement(&mut self) -> Result<(), FfiError> {
        self.vec3()?;
        let x = self.nonzero3()?;
        let y = self.nonzero3()?;
        let cross = [
            x[1] * y[2] - x[2] * y[1],
            x[2] * y[0] - x[0] * y[2],
            x[0] * y[1] - x[1] * y[0],
        ];
        let c2: f64 = cross.iter().map(|c| c * c).sum();
        let l2: f64 = x.iter().map(|c| c * c).sum::<f64>() * y.iter().map(|c| c * c).sum::<f64>();
        if c2 < 1e-6 * l2 {
            return Err(bad("placement axes parallel"));
        }
        Ok(())
    }
}

/// Checks `k` control points sharing their rational marker.
fn check_points(rd: &mut Reader<'_>, k: usize) -> Result<(), FfiError> {
    let mut w0 = None;
    for _ in 0..k {
        let x = rd.num()?;
        let y = rd.num()?;
        let z = rd.num()?;
        let w = rd.num()?;
        let rational = w != NOW;
        if *w0.get_or_insert(rational) != rational {
            return Err(bad("surface: mixed rationality"));
        }
        if rational && !(1e-3..=1e3).contains(&w) {
            return Err(bad("surface: weight out of range"));
        }
        if [x, y, z]
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad("surface: point out of range"));
        }
    }
    Ok(())
}

/// A surface block; returns its parameter domain (`None` for a plane or a cylinder) and
/// whether it is a cylinder (only where `cylinder_ok`).
fn surface(rd: &mut Reader<'_>, cylinder_ok: bool) -> Result<(Option<[f64; 4]>, bool), FfiError> {
    // 3: a B-spline (as 0) whose raw knots are written back after construction
    let kind = rd.index(3)?;
    if kind == 1 {
        rd.vec3()?;
        rd.nonzero3()?;
        return Ok((None, false));
    }
    if kind == 2 {
        if !cylinder_ok {
            return Err(bad("cylinder base not allowed here"));
        }
        rd.placement()?;
        let r = rd.finite()?;
        let (a0, a1) = (rd.finite()?, rd.finite()?);
        let h = rd.finite()?;
        if r < 1e-6 || h < 1e-6 {
            return Err(bad("cylinder: need a positive radius and height"));
        }
        if a0 < -360.0 || a1 > a0 + 360.0 || a1 <= a0 || a1 > 360.0 {
            return Err(bad("cylinder: need -360 <= start < end <= start + 360"));
        }
        rd.flag()?;
        rd.flag()?;
        return Ok((None, true));
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
    check_points(rd, (n + 1) * (m + 1))?;
    let u = rd.finites(r + 1)?;
    let v = rd.finites(s + 1)?;
    let (u0, u1) = check_knots(&u, p, "surface u")?;
    let (v0, v1) = check_knots(&v, q, "surface v")?;
    Ok((Some([u0, v0, u1, v1]), false))
}

/// A volume block; returns (head is a plain volume, all maps transforms, some map is a
/// plain volume).
fn volume(rd: &mut Reader<'_>) -> Result<(bool, bool, bool), FfiError> {
    let n = rd.index(MAX_MAPS)?;
    if n == 0 {
        return Err(bad("volume without maps"));
    }
    let (mut head_base, mut all_transforms, mut any_base) = (false, true, false);
    for i in 0..n {
        let kind = rd.index(7)?;
        if kind == 1 {
            for _ in 0..4 {
                rd.vec3()?;
            }
        }
        let component = kind >= 5;
        if component {
            // origin / center, axis and mid direction (non-zero), then two numbers
            rd.vec3()?;
            rd.nonzero3()?;
            rd.nonzero3()?;
            rd.finite()?;
            rd.finite()?;
        }
        if i == 0 {
            head_base = kind == 0;
        }
        all_transforms &= kind == 1;
        any_base |= kind == 0;
        if rd.flag()? {
            if component {
                return Err(bad("component volume with an orient map"));
            }
            for _ in 0..4 {
                rd.vec3()?;
            }
        }
    }
    Ok((head_base, all_transforms, any_base))
}

/// An object block (see the module docs).
fn object_block(rd: &mut Reader<'_>) -> Result<Block, FfiError> {
    let (domain, cylinder) = surface(rd, true)?;
    let (head_base, all_transforms, any_base) = volume(rd)?;
    rd.flag()?;
    let copy_flag = rd.index(3)?;
    let owner_flag = rd.index(3)?;
    let copies_volume = copy_flag == 2 || copy_flag == 3;
    if head_base && copies_volume {
        return Err(bad("constructor copies a plain volume"));
    }
    Ok(Block {
        domain,
        head_base,
        all_transforms,
        any_base,
        copies_volume,
        // the constructor's copy bits set the same owner bits (`lOwnerFlag |= copy`)
        owns_volume: matches!(owner_flag | copy_flag, 2 | 3),
        cylinder,
    })
}

/// A parameter inside the base surface's domain by a margin (anywhere bounded for a
/// plane).
fn inside(rd: &mut Reader<'_>, c: &Block, v_dir: bool) -> Result<f64, FfiError> {
    let t = rd.finite()?;
    if let Some([u0, v0, u1, v1]) = c.domain {
        let (a, b) = if v_dir { (v0, v1) } else { (u0, u1) };
        let m = 1e-3 * (b - a);
        if t < a + m || t > b - m {
            return Err(bad("parameter not inside the domain"));
        }
    }
    Ok(t)
}

fn validate_op(op: Op, rd: &mut Reader<'_>, c: &Block) -> Result<(), FfiError> {
    if c.cylinder
        && !matches!(
            op,
            Op::Info | Op::Eval | Op::EvalSimple | Op::EvalPoint | Op::Normal | Op::Exact
        )
    {
        return Err(bad("cylinder base: only Info, the evaluation ops and Exact"));
    }
    match op {
        Op::Info => {
            rd.uv()?;
            let nt = rd.index(64)?;
            for _ in 0..nt {
                rd.finite()?;
            }
        }
        Op::Eval | Op::EvalSimple => {
            rd.uv()?;
            rd.index(MAX_DERIV)?;
            rd.index(MAX_DERIV)?;
            let flags = if op == Op::Eval { 4 } else { 3 };
            for _ in 0..flags {
                rd.flag()?;
            }
        }
        Op::EvalPoint => {
            rd.uv()?;
        }
        Op::Normal => {
            rd.uv()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::Knots => {
            rd.flag()?;
            rd.flag()?;
            rd.finite()?;
            rd.finite()?;
        }
        Op::StepInv => {
            // `InvEvaluatePoint` / `InvMapPoint` run with no domain box here, so none of
            // `surface.volume_p2`'s undefined-box paths (`inv_eval_undefined`,
            // `inv_eval_unbend`) can be reached: every map kind is allowed.
            for _ in 0..4 {
                rd.finite()?;
            }
            rd.vec3()?;
            let tol = rd.finite()?;
            if tol <= 0.0 {
                return Err(bad("step inversion: need a positive tolerance"));
            }
        }
        Op::Reverse => {
            rd.flag()?;
        }
        Op::SwapUv | Op::MakeOwner | Op::Exact => {}
        Op::AssertValid => {
            let level = rd.index(4)?;
            if level == 3 {
                return Err(bad("assert valid level"));
            }
        }
        Op::Trim => {
            let [u0, v0, u1, v1] = [rd.finite()?, rd.finite()?, rd.finite()?, rd.finite()?];
            if let Some([a0, b0, a1, b1]) = c.domain {
                let (mu, mv) = (1e-3 * (a1 - a0), 1e-3 * (b1 - b0));
                if u0 < a0 || u1 > a1 || v0 < b0 || v1 > b1 || u1 - u0 < mu || v1 - v0 < mv {
                    return Err(bad("trim: domain not a proper part of the surface domain"));
                }
            } else if u1 <= u0 || v1 <= v0 {
                return Err(bad("trim: empty domain"));
            }
        }
        Op::Transform => {
            rd.placement()?;
            rd.flag()?;
            rd.vec3()?;
        }
        Op::Split => {
            if c.head_base {
                return Err(bad("split copies the volume"));
            }
            let v_dir = rd.flag()?;
            inside(rd, c, v_dir)?;
        }
        Op::Copy => {
            // Copy uses the copy constructor. A failed plain-volume Copy leaves
            // the pointer shared, safe only when the result does not own it.
            if c.head_base && c.owns_volume {
                return Err(bad("copy owns a plain volume"));
            }
        }
        Op::CopyCtor => {
            // The copy constructor attempts the volume copy whenever the original owns
            // either part, but its result owns the volume only for owner flags 2 and 3:
            // the base `SmVolume::Copy` fails without setting the output pointer, so an
            // owner-1 result shares a plain volume it does not own (safe), while an
            // owning one would delete the original's volume.
            if c.head_base && c.owns_volume {
                return Err(bad("copy constructor owns a plain volume"));
            }
        }
        Op::Equal => {
            if !rd.flag()? {
                object_block(rd)?;
            }
        }
        Op::SetOwner => {
            rd.index(3)?;
        }
        Op::SetBase => {
            surface(rd, false)?;
            rd.flag()?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::SetVolume => {
            volume(rd)?;
            rd.flag()?;
            rd.flag()?;
        }
        Op::ReadDb => return Err(bad("not an object op")),
        Op::WriteDb => {
            if c.any_base {
                return Err(bad("write: a plain volume cannot be read back"));
            }
            rd.index(1 << 20)?;
        }
        Op::Mirror => {
            if !c.all_transforms || c.copies_volume {
                return Err(bad("mirror: only transform maps"));
            }
            rd.placement()?;
        }
    }
    if op == Op::MakeOwner && c.head_base {
        return Err(bad("make owner copies a plain volume"));
    }
    Ok(())
}

/// Whether `bytes` holds a digit run (as `operator>>` would extract it) equal to
/// `SmVolume_TYPE` (`22001`) or `SmBSplineVolume_TYPE` (`22004`).
fn has_unreadable_volume_type(bytes: &[u8]) -> bool {
    let mut value: u64 = 0;
    let mut in_run = false;
    for &c in bytes.iter().chain(core::iter::once(&b' ')) {
        if c.is_ascii_digit() {
            value = value.saturating_mul(10).saturating_add(u64::from(c - b'0'));
            in_run = true;
        } else {
            if in_run && (value == 22001 || value == 22004) {
                return true;
            }
            value = 0;
            in_run = false;
        }
    }
    false
}

/// Checks `input` for `op` (layouts in the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input [`eval`] rejects.
pub fn validate(op: Op, input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    if op == Op::ReadDb {
        rd.index(1 << 20)?;
        let nb = rd.index(MAX_DB_BYTES)?;
        let mut bytes = Vec::new();
        for _ in 0..nb {
            bytes.push(rd.index(255)? as u8);
        }
        if !bytes.ends_with(b"\n") {
            return Err(bad("read: incomplete ASCII record"));
        }
        if has_unreadable_volume_type(&bytes) {
            return Err(bad("read: the text names a plain or B-spline volume"));
        }
        return rd.done();
    }
    let c = object_block(&mut rd)?;
    validate_op(op, &mut rd, &c)?;
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
        // reads for `op` (validated above: well-formed blocks, every index, count and
        // flag the shim passes to upstream, no owning copy of a plain volume); buf is
        // valid for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_srf_in_volume_eval(
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

    /// An object block for the `Copy` / `CopyCtor` guard tests: a unit bilinear
    /// B-spline surface and one volume map (a plain `SmVolume` or an identity
    /// `SmTransform`), `in_param = false`, the given constructor copy flag and owner
    /// flag.
    fn object_input(plain_volume: bool, copy: u64, owner: u64) -> Vec<f64> {
        // surface block kind 0: `oob form n m p q r s cpts u v`
        let mut v = vec![0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 3.0, 3.0];
        v.extend([
            0.0, 0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 0.0, 1.0, 1.0, 1.0, 0.0, 1.0,
        ]);
        v.extend([0.0, 0.0, 1.0, 1.0, 0.0, 0.0, 1.0, 1.0]);
        // volume block: one map
        v.push(1.0);
        if plain_volume {
            v.extend([0.0, 0.0]);
        } else {
            v.push(1.0);
            v.extend([0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
            v.push(0.0);
        }
        // in_param copy_flag owner_flag
        v.extend([0.0, copy as f64, owner as f64]);
        v
    }

    #[test]
    fn copy_ctor_rejects_only_a_plain_volume_its_result_owns() {
        // a failed base `Copy` leaves the pointer shared: safe while the copy does not
        // own the volume (owner flags 0 and 1)
        for owner in [0, 1] {
            for copy in [0, 1] {
                assert!(
                    validate(Op::CopyCtor, &object_input(true, copy, owner)).is_ok(),
                    "plain volume, copy {copy}, owner {owner}"
                );
            }
            // the constructor's own rule still rejects copying a plain volume
            for copy in [2, 3] {
                assert!(
                    matches!(
                        validate(Op::CopyCtor, &object_input(true, copy, owner)),
                        Err(FfiError::InvalidArgument(_))
                    ),
                    "constructor copy {copy} of a plain volume, owner {owner}"
                );
            }
        }
        // an owning copy would delete the volume the failed `Copy` left shared
        for owner in [2, 3] {
            assert!(matches!(
                validate(Op::CopyCtor, &object_input(true, 0, owner)),
                Err(FfiError::InvalidArgument(_))
            ));
        }
        // the same owner flags are fine with a copyable volume
        for owner in [0, 3] {
            assert!(validate(Op::CopyCtor, &object_input(false, 0, owner)).is_ok());
        }
    }

    /// The virtual `Copy` runs the same copy constructor as `CopyCtor`, so the same
    /// domain is defined: a plain `SmVolume` head only where the result's effective
    /// owner flag is 0 or 1. The base `SmVolume::Copy` fails without setting the output
    /// pointer, so a non-owning result shares the volume safely; an owning result (2 or
    /// 3) would delete the original's volume.
    #[test]
    fn copy_accepts_only_a_non_owning_plain_volume_result() {
        // a failed base `Copy` leaves the pointer shared: safe while the copy does not
        // own the volume (owner flags 0 and 1)
        for owner in [0, 1] {
            for copy in [0, 1] {
                assert!(
                    validate(Op::Copy, &object_input(true, copy, owner)).is_ok(),
                    "plain volume, copy {copy}, owner {owner}"
                );
            }
            // the constructor's own rule still rejects copying a plain volume
            for copy in [2, 3] {
                assert!(
                    matches!(
                        validate(Op::Copy, &object_input(true, copy, owner)),
                        Err(FfiError::InvalidArgument(_))
                    ),
                    "constructor copy {copy} of a plain volume, owner {owner}"
                );
            }
        }
        // an owning copy would delete the volume the failed `Copy` left shared
        for owner in [2, 3] {
            for copy in [0, 1] {
                assert!(
                    matches!(
                        validate(Op::Copy, &object_input(true, copy, owner)),
                        Err(FfiError::InvalidArgument(_))
                    ),
                    "owning copy of a plain volume, copy {copy}, owner {owner}"
                );
            }
        }
        // a copyable volume head keeps every effective owner flag defined
        for owner in 0..=3 {
            for copy in 0..=3 {
                assert!(
                    validate(Op::Copy, &object_input(false, copy, owner)).is_ok(),
                    "transform volume, copy {copy}, owner {owner}"
                );
            }
        }
    }
}
