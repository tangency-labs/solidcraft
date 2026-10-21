//! FFI wrappers for unit `intersect.curve_class_p3`.
//!
//! Shim: `harness/cpp/shim/intersect_curve_class_p3.cpp` (`sc_intersect_curve_class_p3_run`).
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h` (the part-3
//! members of `SmCurveClassification` and the static `sm_ChangeEdgePointClassToNearVertex` and
//! `sm_FixProblemsBySqueezing`).
//!
//! A script is `has_face [face] n_obj obj*n_obj n_ops op*n_ops`. It builds a pool of objects, constructs two
//! default classifications `cc[0]`, `cc[1]` (`SmCurveClassification()`), then runs the ops,
//! each on a target `t` (`0` or `1`; "other" is `cc[1 - t]`). The objects and the setup ops
//! `0..=29` are those of unit `intersect.curve_class_p1` (see its module docs).
//!
//! Objects (`kind data`): `0` curve block (`0 dim oob n p m cpts(x y z w) knots` an
//! `SmBSplineCurve`, `1 dim P1 D1 P2 D2` an `SmHermiteCurve`, `2 3 P V d0 d1 scale io` a bounded
//! `SmLine`, `3 dim C X Y d0 d1 rx ry io` an `SmEllipse`, `4 dim C X Y d0 d1 r io` an
//! `SmCircle`), `1 O[3] N[3]` (`SmPlane(O, N)`), `2 oob n m p q r s cpts(x y z w, v fastest) u v`
//! (an `SmBSplineSurface`), `3 P[3]` (an `SmPoint3d`), `4` vertex, `5 c` (an edge on pool curve
//! `c`, `-1` none), `6 s` (a face on pool surface `s`, `-1` none), `7` loop stand-in, `8` other
//! stand-in. With `has_face`, the one-face B-rep of the face block (as in unit
//! `intersect.curve_class_p4`: [`heal_face_props::encode_face`], built by `sc_one_face_brep.h`)
//! follows the objects in the pool, its topology numbered as [`topology_count`] counts it: the
//! face, then per loop the loop and its vertex (a vertex loop) or, per edge use, its edge, vertex
//! and the edge use (kind `9`). [`run`] takes the face separately and writes the prefix.
//!
//! | code | op (`op t ...`)                              | upstream                                   | outputs after the status |
//! |------|----------------------------------------------|--------------------------------------------|--------------------------|
//! | 0..=29 | as in `intersect.curve_class_p1`           | the setters of `SmCurveClassification`, `SmCurveInterval`, `SmPointClassification` | |
//! | 30   | `param last tol`                             | `FindInterval`                             | `index end_dist position` (on success) |
//! | 31   | `param tol`                                  | `FindPointClass`                           | point classification (on success) |
//! | 32   | `min max tol`                                | `IsCoincident`                             | `flag`                   |
//! | 33   | `pc param last force`                        | `InsertPointClass`                         | `made`                   |
//! | 34   | `j position`                                 | `RemovePointClass`                         |                          |
//! | 35   | `index`                                      | `MergePointClass(index, NULL, NULL)`       |                          |
//! | 36   |                                              | `CleanupAndValidate(other)`                | state of `other`         |
//! | 37   | `index P[3]`                                 | `sm_ChangeEdgePointClassToNearVertex`      | `changed`                |
//! | 38   |                                              | `sm_FixProblemsBySqueezing(cc, other)`     | state of `other`         |
//!
//! A point classification `pc` is `class obj t gap presnap src_tol sss_flag P[3]` (a default
//! `SmPointClassification` with `SetClassObject`, `SetTParam`, `SetGap3d`, `SetPreSnapParam`,
//! `SetSrcZoneTol3d` and, when `sss_flag` is 1, `SetSSSPoint`).
//!
//! Each op writes `op status outputs [state of other] state`, the state of `cc[t]` being `size
//! min max src_tol owned current ignore_tangency cross_hatch fillets ok_to_set_tol
//! tgt_loops_only curve uv1 uv2 any_uv1 any_uv2 surface1 surface2 face ti other n_loops
//! loop*n_loops interval*size`, an interval `min max start mid end`, a point classification
//! `class object face deviation t u v uvw[3] param_set pre_snap sss_flag sss[3] src_tol` (object
//! ids are pool indices, `-1` for `NULL`, `-2` for an object outside the pool).
//!
//! [`run`] accepts only well-formed scripts with bounded finite data. The shim additionally
//! rejects at run time (`SC_ERR_ARG`) what is undefined upstream: `FindInterval`,
//! `FindPointClass`, `IsCoincident`, `InsertPointClass` and the squeeze on a classification
//! without intervals (they read `m_pData[0]`) or, where the call would evaluate it, without a
//! curve; an `IsCoincident` walk past the intervals (it reads `m_pData[ii]` for every `ii` up to
//! the end index, which wraps when the end search fails); interval boundary classifications of
//! points (not an `SmObject`) where the zone tolerance lookups read them; the topology walks
//! (`SmEdge::GetVertex`) over the bare stand-in edges of `InsertPointClass`,
//! `MergePointClass`, `CleanupAndValidate` and `sm_ChangeEdgePointClassToNearVertex` (the edges
//! of the one-face B-rep are real and walked).

pub use super::intersect_curve_class_p4::topology_count;
use crate::units::heal_face_props::{self, FaceSpec};
use crate::validate::{bad, euclid, NOW, NOZ};
use crate::{call_with_buffer, FfiError};

/// Largest pool size.
pub const MAX_OBJECTS: usize = 32;
/// Largest op count.
pub const MAX_OPS: usize = 256;
/// Largest number of intervals set.
pub const MAX_SIZE: usize = 64;
/// Largest coordinate / parameter magnitude.
pub const MAX_COORD: f64 = 1e6;
/// Largest magnitude of a tolerance, gap, deviation or pre-snap value (`SM_USE_DEFAULT` and
/// `SM_BIG_DOUBLE` fit).
pub const MAX_VALUE: f64 = 1e21;
/// Largest control point index of a curve block.
pub const MAX_INDEX: usize = 64;
/// Largest control point index of a surface block (each direction).
pub const MAX_SURF_INDEX: usize = 16;
/// Largest degree.
pub const MAX_DEGREE: usize = 9;

/// Object kind codes.
#[allow(missing_docs)]
pub mod kind {
    pub const CURVE: usize = 0;
    pub const PLANE: usize = 1;
    pub const BSURF: usize = 2;
    pub const POINT: usize = 3;
    pub const VERTEX: usize = 4;
    pub const EDGE: usize = 5;
    pub const FACE: usize = 6;
    pub const LOOP: usize = 7;
    pub const OTHER: usize = 8;
}

/// Op codes.
#[allow(missing_docs)]
pub mod op {
    pub const SET_SIZE: usize = 0;
    pub const REMOVE_AT: usize = 1;
    pub const IVL_SET: usize = 2;
    pub const IVL_SET_MIN: usize = 3;
    pub const IVL_SET_MAX: usize = 4;
    pub const IVL_RESET: usize = 5;
    pub const IVL_ASSIGN: usize = 6;
    pub const IVL_NEW: usize = 7;
    pub const PC_SET_CLASS: usize = 8;
    pub const PC_SET_T: usize = 9;
    pub const PC_SET_UV: usize = 10;
    pub const PC_SET_GAP: usize = 11;
    pub const PC_SET_SSS: usize = 12;
    pub const PC_SET_PRESNAP: usize = 13;
    pub const PC_SET_SRC_TOL: usize = 14;
    pub const SET_CURVE: usize = 15;
    pub const SET_SURFACE1: usize = 16;
    pub const SET_SURFACE2: usize = 17;
    pub const SET_FACE: usize = 18;
    pub const SET_INTERSECT_DATA: usize = 19;
    pub const SET_IS_CURVE_CURRENT: usize = 20;
    pub const SET_IGNORE_TANGENCY: usize = 21;
    pub const SET_IS_CURVE_OWNED: usize = 22;
    pub const SET_OK_TO_SET_TOL: usize = 23;
    pub const SET_FILLET_DATA: usize = 24;
    pub const SET_TGT_LOOPS_ONLY: usize = 25;
    pub const ADD_TGT_LOOP: usize = 26;
    pub const SET_SRC_TOL: usize = 27;
    pub const SET_UV_CURVE1: usize = 28;
    pub const SET_UV_CURVE2: usize = 29;
    pub const FIND_INTERVAL: usize = 30;
    pub const FIND_PC: usize = 31;
    pub const IS_COINCIDENT: usize = 32;
    pub const INSERT: usize = 33;
    pub const REMOVE_PC: usize = 34;
    pub const MERGE: usize = 35;
    pub const CLEANUP: usize = 36;
    pub const NEAR_VERTEX: usize = 37;
    pub const SQUEEZE: usize = 38;
    /// Largest op code.
    pub const LAST: usize = 38;
}

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p3_run(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

crate::reader::unit_reader!(MAX_COORD);

impl Reader<'_> {
    /// A finite value of magnitude at most `max`.
    fn bounded(&mut self, max: f64) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > max {
            return Err(bad(format!("value {v} not finite or too large")));
        }
        Ok(v)
    }

    fn finite(&mut self) -> Result<f64, FfiError> {
        self.bounded(MAX_COORD)
    }

    fn value(&mut self) -> Result<f64, FfiError> {
        self.bounded(MAX_VALUE)
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

    /// `-1` (`None`) or an object index below `len`.
    fn object(&mut self, len: usize) -> Result<Option<usize>, FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(None);
        }
        if v.fract() != 0.0 || v < 0.0 || v >= len as f64 {
            return Err(bad(format!("bad object index {v}")));
        }
        Ok(Some(v as usize))
    }
}

/// Clamped, non-decreasing knots of degree `p` with interior multiplicity at most `p`.
fn check_knots(knots: &[f64], p: usize, what: &str) -> Result<(), FfiError> {
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
        return Err(bad(format!("{what}: knots not non-decreasing")));
    }
    let (Some(&a), Some(&b)) = (knots.first(), knots.last()) else {
        return Err(bad("empty knots"));
    };
    let m = knots.len() - 1;
    let short = || bad(format!("{what}: knots too short"));
    let head = knots.get(..=p).ok_or_else(short)?;
    let tail = knots
        .get(m.checked_sub(p).ok_or_else(short)?..)
        .ok_or_else(short)?;
    let inner = knots.get(p + 1..m - p).ok_or_else(short)?;
    if a >= b || head.iter().any(|&k| k != a) || tail.iter().any(|&k| k != b) {
        return Err(bad(format!("{what}: knots not clamped")));
    }
    if b - a < 1e-6 * a.abs().max(b.abs()).max(1.0) {
        return Err(bad(format!("{what}: parameter range too small")));
    }
    if inner.iter().any(|&k| k == a || k == b) {
        return Err(bad(format!("{what}: end multiplicity above p + 1")));
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

/// A clamped B-spline curve `n p m cpts knots`.
fn bspline(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let n = rd.index(MAX_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let m = rd.index(MAX_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || p > n || m != n + p + 1 {
        return Err(bad("curve: need 1 <= p <= n and m = n + p + 1"));
    }
    let cps = rd.take(4 * (n + 1))?;
    let knots = rd.take(m + 1)?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, z0, w0] = *cpts.first().ok_or_else(|| bad("empty curve"))?;
    for &[x, y, z, w] in cpts {
        if (z == NOZ) != (z0 == NOZ) || (w == NOW) != (w0 == NOW) {
            return Err(bad("curve: mixed dimension or rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || (z != NOZ && !z.is_finite())
            || (w != NOW && !w.is_finite())
        {
            return Err(bad("curve: non-finite control point"));
        }
        if w != NOW && w <= 1e-3 {
            return Err(bad("curve: non-positive weight"));
        }
        if euclid([x, y, z, w])
            .iter()
            .any(|c| !c.is_finite() || c.abs() > MAX_COORD)
        {
            return Err(bad("curve: Euclidean point out of range"));
        }
    }
    check_knots(knots, p, "curve")
}

/// A curve block.
fn curve_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let kind = rd.index(4)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        3 | 4 => {
            rd.vec3()?;
            rd.nonzero3()?;
            rd.nonzero3()?;
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            let len = (d1 - d0).abs();
            if d0.abs() > 720.0 || d1.abs() > 720.0 || !(1e-4..=360.0).contains(&len) {
                return Err(bad(
                    "conic: domain outside [-720, 720] or length outside [1e-4, 360]",
                ));
            }
            for _ in 0..(if kind == 3 { 2 } else { 1 }) {
                if rd.finite()? <= 0.0 {
                    return Err(bad("conic: radius not positive"));
                }
            }
            rd.flag()?;
            Ok(())
        }
        0 => {
            rd.flag()?;
            bspline(rd)
        }
        1 => {
            for _ in 0..4 {
                rd.vec3()?;
            }
            Ok(())
        }
        _ => {
            if dim != 3 {
                return Err(bad("line block: dimension must be 3"));
            }
            rd.vec3()?;
            rd.nonzero3()?;
            let d0 = rd.finite()?;
            let d1 = rd.finite()?;
            if (d1 - d0).abs() < 1e-6 {
                return Err(bad("line: domain too small"));
            }
            if rd.finite()? <= 1e-6 {
                return Err(bad("line: scale not positive"));
            }
            rd.flag()?;
            Ok(())
        }
    }
}

/// A B-spline surface block `oob n m p q r s cpts u v`.
fn surface_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    rd.flag()?;
    let n = rd.index(MAX_SURF_INDEX)?;
    let m = rd.index(MAX_SURF_INDEX)?;
    let p = rd.index(MAX_DEGREE)?;
    let q = rd.index(MAX_DEGREE)?;
    let r = rd.index(MAX_SURF_INDEX + MAX_DEGREE + 1)?;
    let s = rd.index(MAX_SURF_INDEX + MAX_DEGREE + 1)?;
    if p == 0 || q == 0 || p > n || q > m || r != n + p + 1 || s != m + q + 1 {
        return Err(bad("surface: bad sizes"));
    }
    let cps = rd.take(4 * (n + 1) * (m + 1))?;
    let (cpts, _) = cps.as_chunks::<4>();
    let [_, _, _, w0] = *cpts.first().ok_or_else(|| bad("empty surface"))?;
    for &[x, y, z, w] in cpts {
        if (w == NOW) != (w0 == NOW) {
            return Err(bad("surface: mixed rationality"));
        }
        if !x.is_finite()
            || !y.is_finite()
            || !z.is_finite()
            || (w != NOW && !(1e-3..=1e3).contains(&w))
        {
            return Err(bad("surface: bad control point"));
        }
        let d = if w == NOW { 1.0 } else { w };
        if [x / d, y / d, z / d].iter().any(|c| c.abs() > MAX_COORD) {
            return Err(bad("surface: point out of range"));
        }
    }
    check_knots(rd.take(r + 1)?, p, "surface u")?;
    check_knots(rd.take(s + 1)?, q, "surface v")
}

/// A point classification `class obj t gap presnap src_tol sss_flag P[3]`.
fn point_class(rd: &mut Reader<'_>, n_obj: usize) -> Result<(), FfiError> {
    rd.index(110)?;
    rd.object(n_obj)?;
    rd.value()?;
    rd.value()?;
    rd.value()?;
    rd.value()?;
    rd.flag()?;
    rd.vec3()?;
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
    let mut rd = Reader::new(input);
    let n_script = rd.index(MAX_OBJECTS)?;
    for i in 0..n_script {
        let k = rd.index(kind::OTHER)?;
        match k {
            kind::CURVE => curve_block(&mut rd)?,
            kind::PLANE => {
                rd.vec3()?;
                rd.nonzero3()?;
            }
            kind::BSURF => surface_block(&mut rd)?,
            kind::POINT => {
                rd.vec3()?;
            }
            kind::EDGE | kind::FACE => {
                rd.object(i)?;
            }
            _ => {}
        }
    }
    let n_obj = n_script + topology_count(face);
    let n_ops = rd.index(MAX_OPS)?;
    for _ in 0..n_ops {
        let code = rd.index(op::LAST)?;
        rd.index(1)?;
        let obj = |rd: &mut Reader<'_>| rd.object(n_obj).map(|_| ());
        match code {
            op::SET_SIZE | op::REMOVE_AT | op::IVL_RESET | op::MERGE => {
                rd.index(MAX_SIZE)?;
            }
            op::IVL_SET => {
                rd.index(MAX_SIZE)?;
                rd.finite()?;
                rd.finite()?;
            }
            op::IVL_SET_MIN | op::IVL_SET_MAX => {
                rd.index(MAX_SIZE)?;
                rd.finite()?;
            }
            op::IVL_ASSIGN => {
                rd.index(MAX_SIZE)?;
                rd.index(MAX_SIZE)?;
            }
            op::IVL_NEW => {
                rd.index(MAX_SIZE)?;
                rd.flag()?;
                rd.flag()?;
                rd.finite()?;
                rd.finite()?;
            }
            op::PC_SET_CLASS => {
                rd.index(MAX_SIZE)?;
                rd.index(2)?;
                rd.index(110)?;
                obj(&mut rd)?;
            }
            op::PC_SET_UV => {
                rd.index(MAX_SIZE)?;
                rd.index(2)?;
                rd.finite()?;
                rd.finite()?;
            }
            op::PC_SET_T | op::PC_SET_PRESNAP | op::PC_SET_GAP | op::PC_SET_SRC_TOL => {
                rd.index(MAX_SIZE)?;
                rd.index(2)?;
                rd.value()?;
            }
            op::PC_SET_SSS => {
                rd.index(MAX_SIZE)?;
                rd.index(2)?;
                rd.vec3()?;
            }
            op::SET_CURVE | op::SET_FILLET_DATA => {
                obj(&mut rd)?;
                rd.flag()?;
            }
            op::SET_SURFACE1
            | op::SET_SURFACE2
            | op::SET_FACE
            | op::ADD_TGT_LOOP
            | op::SET_UV_CURVE1
            | op::SET_UV_CURVE2 => obj(&mut rd)?,
            op::SET_INTERSECT_DATA => {
                obj(&mut rd)?;
                obj(&mut rd)?;
                obj(&mut rd)?;
                rd.flag()?;
                obj(&mut rd)?;
            }
            op::SET_IS_CURVE_CURRENT
            | op::SET_IGNORE_TANGENCY
            | op::SET_IS_CURVE_OWNED
            | op::SET_OK_TO_SET_TOL
            | op::SET_TGT_LOOPS_ONLY => {
                rd.flag()?;
            }
            op::SET_SRC_TOL => {
                rd.value()?;
            }
            op::FIND_INTERVAL => {
                rd.finite()?;
                rd.value()?;
                rd.value()?;
            }
            op::FIND_PC => {
                rd.finite()?;
                rd.value()?;
            }
            op::IS_COINCIDENT => {
                rd.finite()?;
                rd.finite()?;
                rd.value()?;
            }
            op::INSERT => {
                point_class(&mut rd, n_obj)?;
                rd.finite()?;
                rd.value()?;
                rd.flag()?;
            }
            op::REMOVE_PC => {
                rd.index(MAX_SIZE)?;
                rd.index(4)?;
            }
            op::NEAR_VERTEX => {
                rd.index(MAX_SIZE)?;
                rd.vec3()?;
            }
            // CLEANUP, SQUEEZE: no arguments
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
    call_with_buffer(262144, |buf, len| {
        // SAFETY: input is valid for input.len() reads and is a validated script (every
        // count, code, object index, curve and surface block the shim reads); buf is valid
        // for buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_intersect_curve_class_p3_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
