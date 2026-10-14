//! FFI wrappers for unit `intersect.curve_class_p1`.
//!
//! Shim: `harness/cpp/shim/intersect_curve_class_p1.cpp` (`sc_intersect_curve_class_p1_run`).
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h` (the
//! part-1 members of `SmCurveClassification` and `SmCurveInterval`, `SmTouchData`'s touch-list
//! helpers, the static `sm_*` helpers) and the classification overloads of `SmTol`.
//!
//! A script is `n_obj obj*n_obj n_ops op*n_ops`. It builds a pool of objects, constructs two
//! default classifications `cc[0]`, `cc[1]` (`SmCurveClassification()`), then runs the ops,
//! each on a target `t` (`0` or `1`; "other" is `cc[1 - t]`).
//!
//! Objects (`kind data`): `0` curve block (`0 dim oob n p m cpts(x y z w) knots` an
//! `SmBSplineCurve`, `1 dim P1 D1 P2 D2` an `SmHermiteCurve`, `2 3 P V d0 d1 scale io` a bounded
//! `SmLine`, `3 dim C X Y d0 d1 rx ry io` an `SmEllipse`, `4 dim C X Y d0 d1 r io` an
//! `SmCircle`), `1 O[3] N[3]` (`SmPlane(O, N)`), `2 oob n m p q r s cpts(x y z w, v fastest) u v`
//! (an `SmBSplineSurface`), `3 P[3]` (an `SmPoint3d`), `4` vertex, `5 c` (an edge on pool curve
//! `c`, `-1` none), `6 s` (a face on pool surface `s`, `-1` none), `7` loop stand-in, `8` other
//! stand-in (used for the topology intersector and other-classification pointers, which are
//! only stored).
//!
//! | code | op (`op t ...`)                              | upstream                                    | outputs after the status |
//! |------|----------------------------------------------|---------------------------------------------|--------------------------|
//! | 0    | `n`                                          | `SetSize`                                   |                          |
//! | 1    | `j`                                          | `RemoveAt`                                  |                          |
//! | 2    | `j a b`                                      | `GetAt(j).SetInterval(SmExtent1d(a, b))`    |                          |
//! | 3, 4 | `j v`                                        | `GetAt(j).SetIntervalMin` / `SetIntervalMax`| (status)                 |
//! | 5    | `j`                                          | `GetAt(j).ReSet()`                          |                          |
//! | 6    | `j k`                                        | `GetAt(j) = other.GetAt(k)`                 |                          |
//! | 7    | `j with owner a b`                           | `GetAt(j) = SmCurveInterval([ivl,] ctx, owner ? this : NULL)` |        |
//! | 8    | `j w class obj`                              | `SetClassObject` of start / mid / end (`w` 0 / 1 / 2) |                |
//! | 9..14| `j w v` / `j w u v` / `j w g` / `j w P[3]` / `j w v` / `j w tol` | `SetTParam`, `SetUVParam`, `SetGap3d`, `SetSSSPoint`, `SetPreSnapParam`, `SetSrcZoneTol3d` | |
//! | 15   | `obj owned`                                  | `SetCurve`                                  |                          |
//! | 16..18 | `obj`                                      | `SetSurface1`, `SetSurface2`, `SetFace`     |                          |
//! | 19   | `s1 s2 uv2 current ti`                       | `SetIntersectData`                          |                          |
//! | 20..23, 25 | `b`                                    | `SetIsCurveCurrent`, `SetIgnoreTangency`, `SetIsCurveOwned`, `SetOKToSetTolerance`, `SetTgtLoopsOnly` | |
//! | 24   | `obj b`                                      | `SetFilletData`                             |                          |
//! | 26   | `obj`                                        | `AddTgtLoop`                                |                          |
//! | 27   | `tol`                                        | `SmTol::SetZoneTol3d(cc, tol)`              |                          |
//! | 28, 29 | `obj`                                      | `SetUVCurve1`, `SetUVCurve2`                |                          |
//! | 30   | `j`                                          | `GetSubInterval`                            | `min max`                |
//! | 31   | `j w param`                                  | `GetClassifyCrvLoc(pc, param)`              | `P[3]`                   |
//! | 32   | `mids`                                       | `SetPointClassParameters`                   | (status)                 |
//! | 33   | `j`                                          | `FindBestSqueezeEnd`                        | `position`               |
//! | 34   | `obj`                                        | `IsReferenced`                              | `flag`                   |
//! | 35   |                                              | `sm_MakeConsistentClassifications`          | (status)                 |
//! | 36   | `n type*n`                                   | `GetType`, `IsKindOf`, `GetTypeString`      | `type flag*n len chars`  |
//! | 37   |                                              | `SmTol::GetSrcZoneTol3d(cc)`, `(NULL)`      | `tol system`             |
//! | 38   | `j`                                          | `SmTol::GetSrcZoneTol3d(ivl)`, `GetObjZoneTol3d(ivl)`, `(NULL)`, `GetScaledZero(ivl)` | 4 values |
//! | 39   | `obj`                                        | `sm_ObjectDimension`                        | `dim`                    |
//! | 40   | `c tol n_uvc uvc* n_srf srf* n_uv (u v)* param dev P[3]` | `sm_SetXSectCurveThroughPoint` | `modified param dev set3d P[3] curve uvcurve*` |
//! | 41   | `use_cc edge emate (s u v)*4 t0 t1 value P[3]` | `sm_ApplyPointSolution`                   | `P[3] flag mc me mm dev t0 t1 [curve]` |
//! | 42   | `c1 t1 c2 t2 dev`                            | `sm_GetCurveCurveAngle`                     | `angle`                  |
//! | 43   | `s uvc a0 a1 c2 b0 b1 tol n (type s0 s1 e0 e1 value)*n` | `sm_CullOrRefineCCIIn3d`         | `n (type s0 s1 e0 e1 value)*n` |
//! | 44   | `c s p1 tol1 p2 tol2`                        | `sm_GetXSectTol2dOr3d_ForClassifyPair`      | `tol`                    |
//! | 45   | `CP[3] EP[3] t0 t1 n (s uvc)*n count`        | `sm_GetCCISurfaceGaps`                      | `g1 g2 g3 n (u v)*n`     |
//! | 46   | `c n touch*n`                                | `SmTouchData::GetConnectedTouches` (with and without list) | `cnt cnt2 touch*cnt` |
//! | 47   | `n touch*n`                                  | `SmTouchData::GetWindingCount`              | `count orient`           |
//! | 48   | `e P[3] (s u v)*2 param`                     | `sm_SetXSectEdgeThroughPoint`               | `param dev modified`     |
//! | 49   | `e`                                          | `sm_GetCCIGeometry`                         | `n_surfaces count`       |
//! | 50   | `f P[3] tol`                                 | `sm_ComputeFacePointClass`                  | `class`                  |
//!
//! A touch is `connected_to loop_dir loop_index param pt_index pt_classify` (ids or `-1`).
//! A curve record is `min max n m (status [P[3]])*7`: the natural interval, the NURB's highest
//! control point and knot indices (`-1 -1` when not a B-spline), and the points at 7 evenly
//! spaced parameters.
//!
//! Each op writes `op status outputs state`, the state of `cc[t]` being `size min max src_tol
//! owned current ignore_tangency cross_hatch fillets ok_to_set_tol tgt_loops_only curve uv1 uv2
//! any_uv1 any_uv2 surface1 surface2 face ti other n_loops loop*n_loops interval*size`, an
//! interval `min max start mid end`, a point classification `class object face deviation t u v
//! uvw[3] param_set pre_snap sss_flag sss[3] src_tol` (object ids are pool indices, `-1` for
//! `NULL`, `-2` for an object outside the pool).
//!
//! [`run`] accepts only well-formed scripts with bounded finite data (counts, codes, object
//! indices, curve and surface blocks). The shim additionally rejects at run time (`SC_ERR_ARG`)
//! what is undefined upstream: interval indices past the size, objects of the wrong kind,
//! `NULL` curves or surfaces that would be dereferenced, `SetClassObject` with an object of
//! another kind than the class, point or stand-in objects for `ComputePointParameters`, the
//! topology walks of `sm_MakeConsistentClassifications` and `sm_ApplyPointSolution` over the
//! bare stand-in edges, odd-length winding touch lists.

use crate::{call_with_buffer, FfiError};

/// Largest pool size.
pub const MAX_OBJECTS: usize = 32;
/// Largest op count.
pub const MAX_OPS: usize = 128;
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

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

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
    pub const SUB_INTERVAL: usize = 30;
    pub const CLASSIFY_CRV_LOC: usize = 31;
    pub const SET_PC_PARAMS: usize = 32;
    pub const SQUEEZE_END: usize = 33;
    pub const IS_REFERENCED: usize = 34;
    pub const MAKE_CONSISTENT: usize = 35;
    pub const TYPE: usize = 36;
    pub const SRC_TOL: usize = 37;
    pub const IVL_TOLS: usize = 38;
    pub const OBJ_DIM: usize = 39;
    pub const XSECT_CURVE: usize = 40;
    pub const APPLY: usize = 41;
    pub const ANGLE: usize = 42;
    pub const CULL: usize = 43;
    pub const TOL_PAIR: usize = 44;
    pub const GAPS: usize = 45;
    pub const TOUCH_CONNECTED: usize = 46;
    pub const TOUCH_WINDING: usize = 47;
    pub const EDGE_THROUGH: usize = 48;
    pub const CCI_GEOMETRY: usize = 49;
    pub const FACE_POINT_CLASS: usize = 50;
    /// Largest op code.
    pub const LAST: usize = 50;
}

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p1_run(
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

/// Sequential reader over the input (also used by unit `intersect.curve_class_p7`).
pub(crate) struct Reader<'a> {
    pub(crate) input: &'a [f64],
    pub(crate) at: usize,
}

impl<'a> Reader<'a> {
    pub(crate) fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    /// A finite value of magnitude at most `max`.
    pub(crate) fn bounded(&mut self, max: f64) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !v.is_finite() || v.abs() > max {
            return Err(bad(format!("value {v} not finite or too large")));
        }
        Ok(v)
    }

    pub(crate) fn finite(&mut self) -> Result<f64, FfiError> {
        self.bounded(MAX_COORD)
    }

    pub(crate) fn value(&mut self) -> Result<f64, FfiError> {
        self.bounded(MAX_VALUE)
    }

    pub(crate) fn vec3(&mut self) -> Result<[f64; 3], FfiError> {
        Ok([self.finite()?, self.finite()?, self.finite()?])
    }

    pub(crate) fn nonzero3(&mut self) -> Result<[f64; 3], FfiError> {
        let v = self.vec3()?;
        if v.iter().map(|c| c * c).sum::<f64>() < 1e-12 {
            return Err(bad("zero vector"));
        }
        Ok(v)
    }

    /// An integer in `0..=max`.
    pub(crate) fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
    }

    /// `-1` (`None`) or an object index below `len`.
    pub(crate) fn object(&mut self, len: usize) -> Result<Option<usize>, FfiError> {
        let v = self.num()?;
        if v == -1.0 {
            return Ok(None);
        }
        if v.fract() != 0.0 || v < 0.0 || v >= len as f64 {
            return Err(bad(format!("bad object index {v}")));
        }
        Ok(Some(v as usize))
    }

    pub(crate) fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    pub(crate) fn take(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
        let end = self.at.checked_add(k).ok_or_else(|| bad("overflow"))?;
        let s = self
            .input
            .get(self.at..end)
            .ok_or_else(|| bad("input too short"))?;
        self.at = end;
        Ok(s)
    }
}

fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
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

fn touches(rd: &mut Reader<'_>, n_obj: usize) -> Result<(), FfiError> {
    let n = rd.index(64)?;
    for _ in 0..n {
        rd.object(n_obj)?;
        rd.index(8)?;
        rd.index(1 << 20)?;
        rd.finite()?;
        rd.index(1 << 20)?;
        rd.object(n_obj)?;
    }
    Ok(())
}

/// Checks the object pool block `n_obj obj*n_obj` (also used by unit
/// `intersect.curve_class_p7`) and returns the object count.
pub(crate) fn validate_objects(rd: &mut Reader<'_>) -> Result<usize, FfiError> {
    let n_obj = rd.index(MAX_OBJECTS)?;
    for i in 0..n_obj {
        let k = rd.index(kind::OTHER)?;
        match k {
            kind::CURVE => curve_block(rd)?,
            kind::PLANE => {
                rd.vec3()?;
                rd.nonzero3()?;
            }
            kind::BSURF => surface_block(rd)?,
            kind::POINT => {
                rd.vec3()?;
            }
            kind::EDGE | kind::FACE => {
                rd.object(i)?;
            }
            _ => {}
        }
    }
    Ok(n_obj)
}

/// Checks the static shape of a script (what [`run`] checks before calling the shim).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader { input, at: 0 };
    let n_obj = validate_objects(&mut rd)?;
    let n_ops = rd.index(MAX_OPS)?;
    for _ in 0..n_ops {
        let code = rd.index(op::LAST)?;
        rd.index(1)?;
        let obj = |rd: &mut Reader<'_>| rd.object(n_obj).map(|_| ());
        match code {
            op::SET_SIZE
            | op::REMOVE_AT
            | op::IVL_RESET
            | op::SUB_INTERVAL
            | op::SQUEEZE_END
            | op::IVL_TOLS => {
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
            | op::SET_UV_CURVE2
            | op::IS_REFERENCED
            | op::OBJ_DIM
            | op::CCI_GEOMETRY => obj(&mut rd)?,
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
            | op::SET_TGT_LOOPS_ONLY
            | op::SET_PC_PARAMS => {
                rd.flag()?;
            }
            op::SET_SRC_TOL => {
                rd.value()?;
            }
            op::CLASSIFY_CRV_LOC => {
                rd.index(MAX_SIZE)?;
                rd.index(2)?;
                rd.finite()?;
            }
            op::TYPE => {
                let n = rd.index(8)?;
                for _ in 0..n {
                    rd.index(1 << 20)?;
                }
            }
            op::XSECT_CURVE => {
                obj(&mut rd)?;
                rd.value()?;
                let n_uvc = rd.index(8)?;
                for _ in 0..n_uvc {
                    obj(&mut rd)?;
                }
                let n_srf = rd.index(8)?;
                for _ in 0..n_srf {
                    obj(&mut rd)?;
                }
                let n_uv = rd.index(8)?;
                for _ in 0..2 * n_uv {
                    rd.finite()?;
                }
                rd.finite()?;
                rd.value()?;
                rd.vec3()?;
            }
            op::APPLY => {
                rd.flag()?;
                obj(&mut rd)?;
                obj(&mut rd)?;
                for _ in 0..4 {
                    obj(&mut rd)?;
                    rd.finite()?;
                    rd.finite()?;
                }
                rd.finite()?;
                rd.finite()?;
                rd.value()?;
                rd.vec3()?;
            }
            op::ANGLE => {
                obj(&mut rd)?;
                rd.finite()?;
                obj(&mut rd)?;
                rd.finite()?;
                rd.value()?;
            }
            op::CULL => {
                obj(&mut rd)?;
                obj(&mut rd)?;
                rd.finite()?;
                rd.finite()?;
                obj(&mut rd)?;
                rd.finite()?;
                rd.finite()?;
                rd.value()?;
                let n = rd.index(16)?;
                for _ in 0..n {
                    rd.index(4)?;
                    for _ in 0..4 {
                        rd.finite()?;
                    }
                    rd.value()?;
                }
            }
            op::TOL_PAIR => {
                obj(&mut rd)?;
                obj(&mut rd)?;
                rd.finite()?;
                rd.value()?;
                rd.finite()?;
                rd.value()?;
            }
            op::GAPS => {
                rd.vec3()?;
                rd.vec3()?;
                rd.finite()?;
                rd.finite()?;
                let n = rd.index(8)?;
                for _ in 0..n {
                    obj(&mut rd)?;
                    obj(&mut rd)?;
                }
                rd.index(64)?;
            }
            op::TOUCH_CONNECTED => {
                rd.object(n_obj)?;
                touches(&mut rd, n_obj)?;
            }
            op::TOUCH_WINDING => touches(&mut rd, n_obj)?,
            op::EDGE_THROUGH => {
                obj(&mut rd)?;
                rd.vec3()?;
                for _ in 0..2 {
                    obj(&mut rd)?;
                    rd.finite()?;
                    rd.finite()?;
                }
                rd.finite()?;
            }
            op::FACE_POINT_CLASS => {
                obj(&mut rd)?;
                rd.vec3()?;
                rd.value()?;
            }
            // MAKE_CONSISTENT, SRC_TOL: no arguments
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
            sys::sc_intersect_curve_class_p1_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
