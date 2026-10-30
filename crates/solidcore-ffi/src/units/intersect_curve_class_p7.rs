//! FFI wrappers for unit `intersect.curve_class_p7`.
//!
//! Shim: `harness/cpp/shim/intersect_curve_class_p7.cpp` (`sc_intersect_curve_class_p7_run`).
//! Upstream: `source/SMLib/src/SmCurveClass.cpp` (the part-7 members of
//! `SmCurveClassification` and `sm_SetDoFilletingGeometry`).
//!
//! A script is `has_face [face] objects cc0 cc1 n_ops op*n_ops`. `objects` is the object pool
//! block of unit `intersect.curve_class_p1` (see [`super::intersect_curve_class_p1`]); with
//! `has_face`, the one-face B-rep of the face block (as in unit `intersect.curve_class_p4`:
//! [`heal_face_props::encode_face`], built by `sc_one_face_brep.h`) follows the objects in the
//! pool, its topology numbered as [`topology_count`] counts it: the face, then per loop the
//! loop and its vertex (a vertex loop) or, per edge use, its edge, vertex and the edge use
//! (kind `9`). [`run`] takes the face separately and writes the prefix. Each classification
//! `cc[t]` is `curve s1 uv1 ignore_tangency tol fillet n interval*n`: `SetCurve(curve)`,
//! `SetSurface1(s1)`, `SetUVCurve1(uv1)`, `SetIgnoreTangency`, `SmTol::SetZoneTol3d(cc, tol)`,
//! `fillet` `-1` (none) or `0` / `1` for `SetFilletData(&cc[1 - t], fillet)`, `SetSize(n)`; an
//! interval is `a b (class obj t gap)*3` (`SetInterval(SmExtent1d(a, b))` and, for start, mid
//! and end, `SetClassObject(class, obj)`, `SetTParam(t)`, `SetGap3d(gap)`). Object ids are pool
//! indices, `-1` for `NULL`.
//!
//! | code | op (`op t ...`)          | upstream                              | outputs after the status |
//! |------|--------------------------|---------------------------------------|--------------------------|
//! | 0    | `j`                      | `GetPointClassByIndex(j, &param)`     | `param pc`               |
//! | 1    | `j`                      | `GetPointClassMateByIndex(j)`         | `k` (the mate is interval `k`'s end, `-1` none) |
//! | 2    | `j`                      | `CollapseInterval(j)`                 |                          |
//! | 3    | `n target*n`             | `GetPointClassifiesConnectedTo`       | `n (connected dir loop param index which)*n` (`which` = `3 * interval + 0/1/2` for start / mid / end)  |
//! | 4    | `target`                 | `FindVertexObjects(v, target, &params)` | `cnt id*cnt n param*n` |
//! | 5    | `target flag`            | `FindEdgeObjects(e, target, flag)`    | `cnt id*cnt`             |
//! | 6    |                          | `IsCrossedByEdge`                     | `flag`                   |
//! | 7    | `face`                   | `IsCrossingFace(face)`                | `flag`                   |
//! | 8    | `in_surface`             | `HasNearMisses`                       | `flag`                   |
//! | 9    | `j self report`          | `AreCoincidentMaybe(self ? cc : other, j, pos, report)` | `verdict pos` |
//! | 10   | `j do_point face old uv_ok` | `ClassifyFaceInterval`             |                          |
//! | 11   | `self`                   | `TestForPartialCoincidentEdges`       | `gap has_curve [curve]`  |
//! | 12   | `j todo`                 | `sm_SetDoFilletingGeometry(j, cc, todo)` |                       |
//! | 13   | `j do_point face old uv_ok` | `ClassifyFaceInterval` on the real face |                    |
//!
//! Each op writes `op status outputs state`, the state of `cc[t]` being `size interval*size`,
//! an interval `min max start mid end`, a point classification `class object face deviation t
//! u v uvw[3] param_set pre_snap sss_flag sss[3] src_tol`; a curve record is the one of unit
//! `intersect.curve_class_p1`.
//!
//! [`run`] accepts only well-formed scripts with bounded finite data. The shim additionally
//! rejects at run time (`SC_ERR_ARG`) what is undefined upstream (indices outside the
//! intervals, `NULL` curves that are dereferenced, non-B-spline edge curves that
//! `TestForPartialCoincidentEdges` would copy) and topology work the bare stand-in topology
//! cannot answer (face queries of `ClassifyFaceInterval`, connection tests across object kinds,
//! the edge-curve replacement of `sm_SetDoFilletingGeometry`). Op 13 is op 10 that also runs
//! the face queries when `face` is the one-face B-rep's face, the classification has a curve
//! and the interval's vertex or edge ends are that B-rep's own entities.

use super::intersect_curve_class_p1::{validate_objects, Reader};
pub use super::intersect_curve_class_p4::topology_count;
use crate::units::heal_face_props::{self, FaceSpec};
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

/// Largest number of intervals of a classification.
pub const MAX_IVLS: usize = 16;
/// Largest op count.
pub const MAX_OPS: usize = 64;

/// Op codes.
#[allow(missing_docs)]
pub mod op {
    pub const PC_BY_INDEX: usize = 0;
    pub const MATE: usize = 1;
    pub const COLLAPSE: usize = 2;
    pub const TOUCHES: usize = 3;
    pub const FIND_VERTEX: usize = 4;
    pub const FIND_EDGE: usize = 5;
    pub const CROSSED_BY_EDGE: usize = 6;
    pub const CROSSING_FACE: usize = 7;
    pub const NEAR_MISSES: usize = 8;
    pub const COIN_MAYBE: usize = 9;
    pub const CLASSIFY_FACE: usize = 10;
    pub const PARTIAL: usize = 11;
    pub const FILLET: usize = 12;
    pub const CLASSIFY_FACE_REAL: usize = 13;
    /// Largest op code.
    pub const LAST: usize = 13;
}

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p7_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A classification block.
fn classification(rd: &mut Reader<'_>, n_obj: usize) -> Result<(), FfiError> {
    for _ in 0..3 {
        rd.object(n_obj)?;
    }
    rd.flag()?;
    rd.value()?;
    let fillet = rd.num()?;
    if ![-1.0, 0.0, 1.0].contains(&fillet) {
        return Err(bad("fillet flag must be -1, 0 or 1"));
    }
    let n = rd.index(MAX_IVLS)?;
    for _ in 0..n {
        rd.finite()?;
        rd.finite()?;
        for _ in 0..3 {
            let class = rd.index(110)?;
            if class < 100 {
                return Err(bad("point classification type below 100"));
            }
            rd.object(n_obj)?;
            rd.finite()?;
            rd.value()?;
        }
    }
    Ok(())
}

/// Checks the static shape of a script (what [`run`] checks before calling the shim).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script or face.
pub fn validate(face: Option<&FaceSpec>, input: &[f64]) -> Result<(), FfiError> {
    if let Some(f) = face {
        heal_face_props::validate_face(f)?;
    }
    let mut rd = Reader { input, at: 0 };
    let n_obj = validate_objects(&mut rd)? + topology_count(face);
    classification(&mut rd, n_obj)?;
    classification(&mut rd, n_obj)?;
    let n_ops = rd.index(MAX_OPS)?;
    for _ in 0..n_ops {
        let code = rd.index(op::LAST)?;
        rd.index(1)?;
        match code {
            op::PC_BY_INDEX | op::MATE | op::COLLAPSE => {
                rd.index(MAX_IVLS)?;
            }
            op::TOUCHES => {
                let n = rd.index(8)?;
                for _ in 0..n {
                    rd.object(n_obj)?;
                }
            }
            op::FIND_VERTEX | op::CROSSING_FACE => {
                rd.object(n_obj)?;
            }
            op::FIND_EDGE => {
                rd.object(n_obj)?;
                rd.index(3)?;
            }
            op::NEAR_MISSES | op::PARTIAL => {
                rd.flag()?;
            }
            op::COIN_MAYBE => {
                rd.index(MAX_IVLS)?;
                rd.flag()?;
                rd.flag()?;
            }
            op::CLASSIFY_FACE | op::CLASSIFY_FACE_REAL => {
                rd.index(MAX_IVLS)?;
                rd.flag()?;
                rd.object(n_obj)?;
                rd.object(n_obj)?;
                rd.flag()?;
            }
            op::FILLET => {
                rd.index(MAX_IVLS)?;
                rd.index(7)?;
            }
            // CROSSED_BY_EDGE: no arguments
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
pub fn run(face: Option<&FaceSpec>, script: &[f64]) -> Result<Vec<f64>, FfiError> {
    validate(face, script)?;
    let mut input = Vec::new();
    match face {
        None => input.push(0.0),
        Some(f) => {
            input.push(1.0);
            heal_face_props::encode_face(f, &mut input);
        }
    }
    input.extend_from_slice(script);
    call_with_buffer(8192, |buf, len| {
        // SAFETY: input is valid for input.len() reads and is a validated script (every
        // count, code, object index, curve and surface block the shim reads); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_intersect_curve_class_p7_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
