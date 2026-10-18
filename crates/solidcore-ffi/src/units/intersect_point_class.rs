//! FFI wrappers for unit `intersect.point_class`.
//!
//! Shim: `harness/cpp/shim/intersect_point_class.cpp` (`sc_intersect_point_class_run`).
//! Upstream: `source/SMLib/src/SmPointClass.cpp`, `source/SMLib/inc/SmPointClass.h`, the
//! point-classification overloads of `SmTol` (`source/SMLib/src/SmTol.cpp`) and the
//! `SmSpaceBend` / `SmSpaceUnbend` class type queries.
//!
//! A script is `n_obj obj*n_obj tol0 tol1 n_ops op*n_ops`. It builds a pool of objects,
//! constructs `pc[0] = SmPointClassification(SmZoneTol3d(tol0))` and `pc[1]` likewise,
//! then runs the ops, each on a target `t` (`0` or `1`; "other" is `pc[1 - t]`).
//!
//! Objects (`kind data`): `0` curve block (as unit `curve.curve_p6`: `0 dim oob n p m
//! cpts(x y z w) knots`, `1 dim P1 D1 P2 D2`, `2 3 P V d0 d1 scale io`; or as unit
//! `curve.ellipse_p1` without `pOptNurb`: `3 dim C X Y d0 d1 rx ry io` an `SmEllipse`, `4 dim
//! C X Y d0 d1 r io` an `SmCircle`), `1 O[3] N[3]`
//! (`SmPlane(O, N)`), `2` volume stand-in, `3 P[3]` (an `SmPoint3d`), `4` vertex, `5 c`
//! (an edge on pool curve `c`, `-1` none), `6 s` (a face on pool plane `s`, `-1` none),
//! `7` region stand-in, `8` edgeuse stand-in, `9` other stand-in.
//!
//! | code | op (`op t ...`)                        | upstream                         | outputs after the status |
//! |------|----------------------------------------|----------------------------------|--------------------------|
//! | 0    | `has tol`                              | `ReSet(has ? &tol : NULL)`       |                          |
//! | 1    | `class obj`                            | `SetClassObject`                 |                          |
//! | 2    | `v snap`                               | `SetTParam`                      |                          |
//! | 3    | `u v`                                  | `SetUVParam`                     |                          |
//! | 4    | `u v w`                                | `SetUVWParam`                    |                          |
//! | 5    |                                        | `UnSetParameters`                |                          |
//! | 6    | `g`                                    | `SetGap3d`                       |                          |
//! | 7    | `v`                                    | `SetPreSnapParam`                |                          |
//! | 8    | `P[3]`                                 | `SetSSSPoint`                    |                          |
//! | 9    | `tol`                                  | `SetSrcZoneTol3d`                |                          |
//! | 10   | `obj`                                  | `SetFace`                        |                          |
//! | 11   |                                        | `operator=` (from other)         |                          |
//! | 12   |                                        | `CopyClassificationOnly` (other) |                          |
//! | 13   |                                        | `CopySSSPoint` (other)           |                          |
//! | 14   |                                        | `Combine(other)`                 | `which` (0 this, 1 other) |
//! | 15   | `P[3]`                                 | `ComputePointParameters`         |                          |
//! | 16   |                                        | `FindObjPoint3d`                 | `P[3]` on success        |
//! | 17   | `s` (plane or `-1`)                    | `GetGap3d`                       | `gap`                    |
//! | 18   | `tol obj value p0..p9 off pre`         | `LoadFromIntersection`           |                          |
//! | 19   | `P[3]`                                 | `MergeIntoObject`                | `n_edges n_vertices`     |
//! | 20   |                                        | `GetBrep() != NULL`              | `has_brep`               |
//! | 21   | `j`                                    | `IsConnectedTo(SmTopology*)`     | `connected`              |
//! | 22   | `n j*n`                                | `IsConnectedTo(SmTArray, ULONG&)`| `connected index` (`-1` = `SM_UNDEF_ULONG`) |
//! | 23   |                                        | `GetCurveObject`                 | object id                |
//! | 24   |                                        | `SmTol::GetObjZoneTol3d(pc)`     | `tol`                    |
//! | 25   |                                        | `SmTol::GetXSectTol3d(pc)`       | `tol`                    |
//! | 26   |                                        | `SmTol::GetSrcZoneTol3d(pc)`     | `tol`                    |
//! | 27   | `g`                                    | `SetDeviation`                   |                          |
//! | 28   |                                        | `SmSpaceBend` / `SmSpaceUnbend` `GetClassType`, `GetClassTypeString` | `bend unbend len chars len chars` |
//!
//! Each op writes `op status outputs state`, the state of `pc[t]` being `class object
//! face deviation t u v uvw[3] param_set pre_snap sss_flag sss[3] tolerance
//! vertex_object edge_object face_object` (object ids are pool indices, `-1` for
//! `NULL`, `-2` for an object outside the pool).
//!
//! [`run`] accepts only well-formed scripts with bounded finite data whose
//! `SetClassObject` pairs a class with no object or an object of that kind (no object
//! for `UNKNOWN` and `NOTHING`), and whose loads read inside the 10 solution parameters. The shim additionally rejects, at run time, the ops that would call
//! a member of a stand-in object or of an `SmPoint3d` (see its file comment).

use crate::units::curve_curve_p6::MAX_COORD;
use crate::{call_with_buffer, FfiError};

/// Largest pool size.
pub const MAX_OBJECTS: usize = 32;
/// Largest op count.
pub const MAX_OPS: usize = 256;
/// Largest magnitude of a tolerance, gap or pre-snap value (`SM_USE_DEFAULT` and
/// `SM_BIG_DOUBLE` fit).
pub const MAX_VALUE: f64 = 1e21;
/// Largest control point index `n` of a B-spline block.
pub const MAX_INDEX: usize = 64;
/// Largest degree of a B-spline block.
pub const MAX_DEGREE: usize = 9;

const NOZ: f64 = f64::MAX;
const NOW: f64 = -f64::MAX;

/// Object kind codes.
#[allow(missing_docs)]
pub mod kind {
    pub const CURVE: usize = 0;
    pub const PLANE: usize = 1;
    pub const VOLUME: usize = 2;
    pub const POINT: usize = 3;
    pub const VERTEX: usize = 4;
    pub const EDGE: usize = 5;
    pub const FACE: usize = 6;
    pub const REGION: usize = 7;
    pub const EDGEUSE: usize = 8;
    pub const OTHER: usize = 9;
}

/// Op codes.
#[allow(missing_docs)]
pub mod op {
    pub const RESET: usize = 0;
    pub const SET_CLASS_OBJECT: usize = 1;
    pub const SET_T: usize = 2;
    pub const SET_UV: usize = 3;
    pub const SET_UVW: usize = 4;
    pub const UNSET: usize = 5;
    pub const SET_GAP: usize = 6;
    pub const SET_PRESNAP: usize = 7;
    pub const SET_SSS: usize = 8;
    pub const SET_SRC_TOL: usize = 9;
    pub const SET_FACE: usize = 10;
    pub const ASSIGN: usize = 11;
    pub const COPY_CLASS_ONLY: usize = 12;
    pub const COPY_SSS: usize = 13;
    pub const COMBINE: usize = 14;
    pub const COMPUTE: usize = 15;
    pub const FIND: usize = 16;
    pub const GAP: usize = 17;
    pub const LOAD: usize = 18;
    pub const MERGE: usize = 19;
    pub const BREP: usize = 20;
    pub const CONNECTED: usize = 21;
    pub const CONNECTED_ANY: usize = 22;
    pub const CURVE_OBJECT: usize = 23;
    pub const OBJ_TOL: usize = 24;
    pub const XSECT_TOL: usize = 25;
    pub const SRC_TOL: usize = 26;
    pub const SET_DEVIATION: usize = 27;
    pub const BEND_TYPES: usize = 28;
    /// Largest op code.
    pub const LAST: usize = 28;
}

mod sys {
    extern "C" {
        pub fn sc_intersect_point_class_run(
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

crate::reader::unit_reader!();

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

    /// `-1` (`None`) or an index below `len`.
    fn opt_index(&mut self, len: usize) -> Result<Option<usize>, FfiError> {
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

fn euclid([x, y, z, w]: [f64; 4]) -> [f64; 3] {
    let z = if z == NOZ { 0.0 } else { z };
    if w == NOW {
        [x, y, z]
    } else {
        [x / w, y / w, z / w]
    }
}

/// A clamped B-spline `n p m cpts knots` (as unit `curve.curve_p6` accepts).
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
    if knots.iter().any(|k| !k.is_finite() || k.abs() > MAX_COORD)
        || knots.windows(2).any(|w| w.first() > w.last())
    {
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

/// A curve block.
fn curve_block(rd: &mut Reader<'_>) -> Result<(), FfiError> {
    let kind = rd.index(4)?;
    let dim = rd.index(3)?;
    if dim < 2 {
        return Err(bad("curve block: dimension must be 2 or 3"));
    }
    match kind {
        3 | 4 => {
            // an SmEllipse / SmCircle (as unit curve.ellipse_p1 accepts them)
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

/// The object kind a classification type (code) needs; `None` for `UNKNOWN` and
/// `NOTHING` (no object).
fn kind_of_class(class: usize) -> Result<Option<usize>, FfiError> {
    Ok(match class {
        100 | 110 => None,
        101 => Some(kind::REGION),
        102 => Some(kind::FACE),
        103 => Some(kind::EDGE),
        104 => Some(kind::VERTEX),
        105 => Some(kind::POINT),
        106 => Some(kind::CURVE),
        107 => Some(kind::PLANE),
        108 => Some(kind::VOLUME),
        109 => Some(kind::EDGEUSE),
        _ => return Err(bad(format!("bad classification type {class}"))),
    })
}

/// Checks the static shape of a script (what [`run`] checks before calling the shim).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a malformed script.
pub fn validate(input: &[f64]) -> Result<(), FfiError> {
    let mut rd = Reader::new(input);
    let n_obj = rd.index(MAX_OBJECTS)?;
    let mut kinds: Vec<usize> = Vec::new();
    for _ in 0..n_obj {
        let k = rd.index(kind::OTHER)?;
        match k {
            kind::CURVE => curve_block(&mut rd)?,
            kind::PLANE => {
                rd.vec3()?;
                rd.nonzero3()?;
            }
            kind::POINT => {
                rd.vec3()?;
            }
            kind::EDGE | kind::FACE => {
                let want = if k == kind::EDGE {
                    kind::CURVE
                } else {
                    kind::PLANE
                };
                if let Some(j) = rd.opt_index(kinds.len())? {
                    if kinds.get(j) != Some(&want) {
                        return Err(bad("edge/face geometry of the wrong kind"));
                    }
                }
            }
            _ => {}
        }
        kinds.push(k);
    }
    rd.bounded(MAX_VALUE)?;
    rd.bounded(MAX_VALUE)?;
    let n_ops = rd.index(MAX_OPS)?;
    let topo = |j: usize| {
        matches!(
            kinds.get(j),
            Some(&(kind::VERTEX | kind::EDGE | kind::FACE))
        )
    };
    for _ in 0..n_ops {
        let code = rd.index(op::LAST)?;
        rd.index(1)?;
        match code {
            op::RESET => {
                rd.flag()?;
                rd.bounded(MAX_VALUE)?;
            }
            op::SET_CLASS_OBJECT => {
                let class = rd.index(110)?;
                let want = kind_of_class(class)?;
                // no object is fine for any class (the class becomes UNKNOWN); an object
                // must be of the class's kind
                if let Some(j) = rd.opt_index(kinds.len())? {
                    if kinds.get(j).copied() != want {
                        return Err(bad("SetClassObject: object kind does not match the class"));
                    }
                }
            }
            op::SET_T => {
                rd.finite()?;
                rd.flag()?;
            }
            op::SET_UV => {
                rd.finite()?;
                rd.finite()?;
            }
            op::SET_UVW | op::SET_SSS | op::COMPUTE | op::MERGE => {
                rd.vec3()?;
            }
            op::SET_GAP | op::SET_DEVIATION | op::SET_PRESNAP | op::SET_SRC_TOL => {
                rd.bounded(MAX_VALUE)?;
            }
            op::SET_FACE => {
                if let Some(j) = rd.opt_index(kinds.len())? {
                    if kinds.get(j) != Some(&kind::FACE) {
                        return Err(bad("SetFace: not a face"));
                    }
                }
            }
            op::GAP => {
                if let Some(j) = rd.opt_index(kinds.len())? {
                    if kinds.get(j) != Some(&kind::PLANE) {
                        return Err(bad("GetGap3d: not a surface"));
                    }
                }
            }
            op::LOAD => {
                rd.bounded(MAX_VALUE)?;
                let obj = rd
                    .opt_index(kinds.len())?
                    .ok_or_else(|| bad("LoadFromIntersection: no object"))?;
                if kinds.get(obj) == Some(&kind::POINT) {
                    return Err(bad("LoadFromIntersection: a point is not an SmObject"));
                }
                rd.bounded(MAX_VALUE)?;
                for _ in 0..10 {
                    rd.finite()?;
                }
                // the parameters read (curve/edge 1, surface/face 2, volume 3) stay inside
                // the 10 of an SmSolutionEnd
                let read = match kinds.get(obj).copied() {
                    Some(kind::CURVE | kind::EDGE) => 1,
                    Some(kind::PLANE | kind::FACE) => 2,
                    Some(kind::VOLUME) => 3,
                    _ => 0,
                };
                let off = rd.index(9)?;
                if off + read > 10 {
                    return Err(bad(
                        "LoadFromIntersection: parameters past the solution end",
                    ));
                }
                rd.bounded(MAX_VALUE)?;
            }
            op::CONNECTED => {
                let j = rd.index(MAX_OBJECTS)?;
                if !topo(j) {
                    return Err(bad("IsConnectedTo: target is not topology"));
                }
            }
            op::CONNECTED_ANY => {
                let n = rd.index(16)?;
                for _ in 0..n {
                    let j = rd.index(MAX_OBJECTS)?;
                    if !topo(j) {
                        return Err(bad("IsConnectedTo: target is not topology"));
                    }
                }
            }
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
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and is a validated script (every
        // count, index, kind and curve block the shim reads); buf is valid for
        // buf.len() writes and the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_intersect_point_class_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
