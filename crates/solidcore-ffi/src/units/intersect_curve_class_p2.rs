//! FFI wrappers for unit `intersect.curve_class_p2`.
//!
//! Shim: `harness/cpp/shim/intersect_curve_class_p2.cpp` (`sc_intersect_curve_class_p2_run`).
//! Upstream: `source/SMLib/src/SmCurveClass.cpp` (the part-2 members of
//! `SmCurveClassification`, `SmCurveInterval::GetMinUV` / `GetMaxUV` and
//! `sm_RefineCCIToSurfaces`).
//!
//! A script is `objects cos cc0 cc1 n_ops op*n_ops`. `objects` is the object pool block of unit
//! `intersect.curve_class_p1` (see [`super::intersect_curve_class_p1`]). `cos` is
//! `n (uv surface)*n`: `SmCrvOnSurf(*uv, *surface, NULL, 0, ctx)` curves (a pool curve on a pool
//! B-spline surface) appended to the pool, so they have the next object indices. Each
//! classification `cc[t]` is `curve s1 uv1 s2 uv2 face ti other fillet current ignore ok_tol
//! tgt_only n_loops loop*n_loops tol n interval*n`: `SetCurve(curve)`, `SetUVCurve1(uv1)`,
//! `SetIntersectData(s1, s2, uv2, current, ti)`, `SetFace(face)`, `SetFilletData(other,
//! fillet)` (when either is given), `SetIgnoreTangency`, `SetOKToSetTolerance`,
//! `SetTgtLoopsOnly`, `AddTgtLoop` for each loop, `SmTol::SetZoneTol3d(cc, tol)`, `SetSize(n)`;
//! an interval is `a b (class obj t gap)*3` (`SetInterval(SmExtent1d(a, b))` and, for start,
//! mid and end, `SetClassObject(class, obj)`, `SetTParam(t)`, `SetGap3d(gap)`). `ti` and
//! `other` are `Other` stand-ins (stored, never dereferenced). Object ids are pool indices,
//! `-1` for `NULL`.
//!
//! | code | op (`op t ...`)                    | upstream                                    | outputs after the status |
//! |------|------------------------------------|---------------------------------------------|--------------------------|
//! | 0    |                                    | `cc[t] = cc[1 - t]` (`operator=`)           |                          |
//! | 1    | `j`                                | `GetPointObjZoneTol3d(j)`                   | `tol`                    |
//! | 2    | `brep`                             | `HasDeadTopologyPointObject(brep ? empty SmBrep : NULL)` | `flag`      |
//! | 3    | `j`                                | `GetClassifyCrvLoc(j, &param)`              | `param x y z`            |
//! | 4    | `j interval`                       | `InsertAt(j, SmCurveInterval(ctx, cc) + interval)` |                   |
//! | 5    | `curve has_ivl a b uv tol owned current s1 s2 uv2 ti` | `ReSet(curve, has_ivl ? &SmExtent1d(a, b) : NULL, uv, tol, 0, NULL, owned, current, s1, s2, uv2, ti)` | |
//! | 6    | `j end allow`                      | `ConnectIntervals(j, end, allow)`           |                          |
//! | 7    | `j tol class obj t gap`            | `UpdatePointClass(j, SmPointClassification(tol) + values)` |           |
//! | 8    | `j param collapse`                 | `EditIntervalMin(j, param, collapse)`       |                          |
//! | 9    | `j param collapse`                 | `EditIntervalMax(j, param, collapse)`       |                          |
//! | 10   | `j u v`                            | `GetAt(j).GetMinUV(uv = (u, v))`            | `u v`                    |
//! | 11   | `j u v`                            | `GetAt(j).GetMaxUV(uv = (u, v))`            | `u v`                    |
//! | 12   | `type s0 s1 e0 e1 value edge sss[3] count` | `sm_RefineCCIToSurfaces(&cc, sol, edge, flag, sss, dev, count)` | `flag sss[3] dev count type s0 s1 e0 e1 value` |
//!
//! Each op writes `op status outputs state`, the state of `cc[t]` being the full
//! classification record of unit `intersect.curve_class_p1` (`size interval src_tol flags ids
//! loops interval*size`). A curve deep-copied by `operator=` reports the id of the curve it
//! copies.
//!
//! [`run`] accepts only well-formed scripts with bounded finite data. The shim additionally
//! rejects at run time (`SC_ERR_ARG`) what is undefined upstream (indices outside the
//! intervals, `NULL` curves that are dereferenced, zone tolerances of point objects, which are
//! not `SmObject`s, `InsertAt(0)` into a non-empty classification, whose unsigned shift loop
//! never ends) and `sm_RefineCCIToSurfaces` past its entry checks (it walks the topology of the
//! bare stand-in edge).

use super::intersect_curve_class_p1::{validate_objects, Reader};
use crate::{call_with_buffer, FfiError};

/// Largest number of intervals of a classification (and largest interval index of an op).
pub const MAX_IVLS: usize = 16;
/// Largest number of target loops of a classification.
pub const MAX_LOOPS: usize = 4;
/// Largest number of curves on surfaces.
pub const MAX_COS: usize = 4;
/// Largest op count.
pub const MAX_OPS: usize = 64;

/// Op codes.
#[allow(missing_docs)]
pub mod op {
    pub const ASSIGN: usize = 0;
    pub const OBJ_TOL: usize = 1;
    pub const DEAD: usize = 2;
    pub const CRV_LOC: usize = 3;
    pub const INSERT: usize = 4;
    pub const RESET: usize = 5;
    pub const CONNECT: usize = 6;
    pub const UPDATE_PC: usize = 7;
    pub const EDIT_MIN: usize = 8;
    pub const EDIT_MAX: usize = 9;
    pub const MIN_UV: usize = 10;
    pub const MAX_UV: usize = 11;
    pub const REFINE: usize = 12;
    /// Largest op code.
    pub const LAST: usize = 12;
}

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p2_run(
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

/// A point classification `class obj t gap`.
fn point_class(rd: &mut Reader<'_>, n_obj: usize) -> Result<(), FfiError> {
    let class = rd.index(110)?;
    if class < 100 {
        return Err(bad("point classification type below 100"));
    }
    rd.object(n_obj)?;
    rd.finite()?;
    rd.value()?;
    Ok(())
}

/// An interval `a b pc*3`.
fn interval(rd: &mut Reader<'_>, n_obj: usize) -> Result<(), FfiError> {
    rd.finite()?;
    rd.finite()?;
    for _ in 0..3 {
        point_class(rd, n_obj)?;
    }
    Ok(())
}

/// A classification block.
fn classification(rd: &mut Reader<'_>, n_obj: usize) -> Result<(), FfiError> {
    for _ in 0..8 {
        rd.object(n_obj)?;
    }
    for _ in 0..5 {
        rd.flag()?;
    }
    let n_loops = rd.index(MAX_LOOPS)?;
    for _ in 0..n_loops {
        if rd.object(n_obj)?.is_none() {
            return Err(bad("NULL target loop"));
        }
    }
    rd.value()?;
    let n = rd.index(MAX_IVLS)?;
    for _ in 0..n {
        interval(rd, n_obj)?;
    }
    Ok(())
}

/// Checks the static shape of a script (what [`run`] checks before calling the shim).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let n_base = validate_objects(&mut rd)?;
    let n_cos = rd.index(MAX_COS)?;
    for _ in 0..n_cos {
        if rd.object(n_base)?.is_none() || rd.object(n_base)?.is_none() {
            return Err(bad("curve on surface without curve or surface"));
        }
    }
    let n_obj = n_base + n_cos;
    classification(&mut rd, n_obj)?;
    classification(&mut rd, n_obj)?;
    let n_ops = rd.index(MAX_OPS)?;
    for _ in 0..n_ops {
        let code = rd.index(op::LAST)?;
        rd.index(1)?;
        match code {
            op::OBJ_TOL | op::CRV_LOC => {
                rd.index(MAX_IVLS)?;
            }
            op::DEAD => {
                rd.flag()?;
            }
            op::INSERT => {
                rd.index(MAX_IVLS)?;
                interval(&mut rd, n_obj)?;
            }
            op::RESET => {
                rd.object(n_obj)?;
                rd.flag()?;
                rd.finite()?;
                rd.finite()?;
                rd.object(n_obj)?;
                rd.value()?;
                rd.flag()?;
                rd.flag()?;
                for _ in 0..4 {
                    rd.object(n_obj)?;
                }
            }
            op::CONNECT => {
                rd.index(MAX_IVLS)?;
                rd.index(4)?;
                rd.flag()?;
            }
            op::UPDATE_PC => {
                rd.index(MAX_IVLS)?;
                rd.value()?;
                point_class(&mut rd, n_obj)?;
            }
            op::EDIT_MIN | op::EDIT_MAX => {
                rd.index(MAX_IVLS)?;
                rd.finite()?;
                rd.flag()?;
            }
            op::MIN_UV | op::MAX_UV => {
                rd.index(MAX_IVLS)?;
                rd.finite()?;
                rd.finite()?;
            }
            op::REFINE => {
                rd.index(4)?;
                for _ in 0..5 {
                    rd.finite()?;
                }
                rd.object(n_obj)?;
                rd.vec3()?;
                rd.index(100)?;
            }
            // ASSIGN: no arguments
            _ => {}
        }
    }
    if rd.at != input.len() {
        return Err(bad("trailing input"));
    }
    Ok(())
}

/// Runs a script (see the module docs) and returns its output stream.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script; [`FfiError::Status`] with
/// `SC_ERR_ARG` when the shim rejects an op at run time.
pub fn run(input: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(input)?;
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and is a validated script (every
        // count, code, object index, curve and surface block the shim reads); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_intersect_curve_class_p2_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
