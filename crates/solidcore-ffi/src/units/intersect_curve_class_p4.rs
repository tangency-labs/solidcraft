//! FFI wrappers for unit `intersect.curve_class_p4`.
//!
//! Shim: `harness/cpp/shim/intersect_curve_class_p4.cpp` (`sc_intersect_curve_class_p4_run`).
//! Upstream: `source/SMLib/src/SmCurveClass.cpp`, `source/SMLib/inc/SmCurveClass.h` (the full
//! `SmCurveClassification` constructor, `GetLoopCrossings`, `GetLoopTouches`,
//! `SmCurveInterval::GetIvlBound_EdgeuseObj` and the static `sm_SqueezeOutSmallestInterval`).
//!
//! A script is `has_face [face] n_obj obj*n_obj n_ops op*n_ops`. It builds a pool of objects
//! (curves, planes, B-spline surfaces, points and "other" stand-ins; the objects of unit
//! `intersect.curve_class_p1` without its topology stand-ins), then, with `has_face`, the
//! one-face B-rep of the face block (the block of [`heal_face_props::encode_face`]: a plane or
//! surface of revolution with a vertex loop or edge loops of line edges, built by
//! `SmBrepConstructor`), whose topology joins the pool after the objects in the order of
//! [`topology_count`]: the face, then per loop the loop and its vertex (a vertex loop) or, per
//! edge use, its edge, its vertex and the edge use (an edge loop). It constructs two default
//! classifications `cc[0]`, `cc[1]` (`SmCurveClassification()`), then runs the ops, each on a
//! target `t` (`0` or `1`). The setup ops `0..=29` are those of unit `intersect.curve_class_p1`
//! (see its module docs and `intersect_curve_class_p3`).
//!
//! | code | op (`op t ...`)                              | upstream                                   | outputs after the status |
//! |------|----------------------------------------------|--------------------------------------------|--------------------------|
//! | 0..=29 | as in `intersect.curve_class_p1`           | the setters of `SmCurveClassification`, `SmCurveInterval`, `SmPointClassification` | |
//! | 30   |                                              | `sm_SqueezeOutSmallestInterval(cc)`        |                          |
//! | 31   | `j do_min`                                   | `cc[j].GetIvlBound_EdgeuseObj(do_min, NULL)` | `edgeuse` (pool id, `-1` none) |
//! | 32   | `n loop*n with_props n_seeds (field idx value)*n_seeds` | `GetLoopCrossings(loops, with_props ? &props : NULL, ..)`, `props` an `SmFaceProps(ctx)` with the seeds of the `heal.face_props` shim applied (a preset at `SM_PROPSTAGE_2a` or later skips `SetProps_Stage2`) | `counts raw done` (each a length and its entries; a touch is `connected dir loop_index param pt_classify_index`), then with `with_props` the props snapshot ([`heal_face_props::SNAP_LEN`] values) |
//! | 33   | `curve a b uv tol data_size owned current s1 s2 uv2` | `cc = SmCurveClassification(curve, [a, b], uv, tol, data_size, NULL, owned, current, s1, s2, uv2, NULL)` | |
//! | 34   | as 32                                        | `GetLoopTouches(loops, with_props ? &props : NULL, touches)` | on success `touches` (a length and its entries, as 32), then with `with_props` the props snapshot; nothing else on a failing status (the list then holds what was collected before the failure) |
//!
//! Each op writes `op status outputs state`, the state of `cc[t]` being as in
//! `intersect_curve_class_p3`.
//!
//! [`run`] accepts only well-formed scripts with bounded finite data. The shim additionally
//! rejects at run time (`SC_ERR_ARG`) what is undefined upstream: the squeeze of a classification
//! without intervals (`FindBestSqueezeEnd(0)` reads `m_pData[0]`); `GetIvlBound_EdgeuseObj` of an
//! interval past the intervals, of an interval whose back pointer is not its classification, on a
//! classification without face (UB-124: `GetUpwardFaceuse` / `GetSurface` on `NULL`), and of an
//! edge end on a face with a surface when the classification or edge curve is `NULL` (both are
//! evaluated); `GetLoopCrossings` or `GetLoopTouches` over loops for a classification without
//! curve (UB-124: `GetNaturalInterval` on `NULL`) or without intervals
//! (`GetPointClassifiesConnectedTo` reads `m_pData[0]`).

use crate::units::heal_face_props::{self, FaceSpec, Loops};
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
/// Largest number of face props seeds of a `GetLoopCrossings` op.
pub const MAX_SEEDS: usize = 32;
/// Largest loop count of `GetLoopCrossings`.
pub const MAX_LOOPS: usize = 8;

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
    /// An edge use of the face (B-rep topology only).
    pub const EDGEUSE: usize = 9;
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
    pub const SQUEEZE: usize = 30;
    pub const IVL_BOUND: usize = 31;
    pub const LOOP_CROSSINGS: usize = 32;
    pub const CONSTRUCT: usize = 33;
    pub const LOOP_TOUCHES: usize = 34;
    /// Largest op code.
    pub const LAST: usize = 34;
}

mod sys {
    extern "C" {
        pub fn sc_intersect_curve_class_p4_run(
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

impl<'a> Reader<'a> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

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

    /// An integer in `0..=max`.
    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v}")));
        }
        Ok(v as usize)
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

    fn flag(&mut self) -> Result<bool, FfiError> {
        Ok(self.index(1)? == 1)
    }

    fn take(&mut self, k: usize) -> Result<&'a [f64], FfiError> {
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

/// The number of pool objects of the face's B-rep: the face, then per loop the loop and its
/// vertex (a vertex loop) or three per edge use (its edge, vertex and the edge use).
#[must_use]
pub fn topology_count(face: Option<&FaceSpec>) -> usize {
    match face.map(|f| &f.loops) {
        None => 0,
        Some(Loops::Vertex(_)) => 3,
        Some(Loops::Edges(loops)) => 1 + loops.iter().map(|l| 1 + 3 * l.len()).sum::<usize>(),
        // one edge use per circle loop
        Some(Loops::Circles(loops)) => 1 + 4 * loops.len(),
    }
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
    let n_obj = rd.index(MAX_OBJECTS)?;
    for _ in 0..n_obj {
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
            kind::OTHER => {}
            // the topology is the face's B-rep
            _ => return Err(bad("topology objects come from the face")),
        }
    }
    let n_pool = n_obj + topology_count(face);
    let n_ops = rd.index(MAX_OPS)?;
    for _ in 0..n_ops {
        let code = rd.index(op::LAST)?;
        rd.index(1)?;
        let obj = |rd: &mut Reader<'_>| rd.object(n_pool).map(|_| ());
        match code {
            op::SET_SIZE | op::REMOVE_AT | op::IVL_RESET => {
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
            op::IVL_BOUND => {
                rd.index(MAX_SIZE)?;
                rd.flag()?;
            }
            op::LOOP_CROSSINGS | op::LOOP_TOUCHES => {
                let n = rd.index(MAX_LOOPS)?;
                for _ in 0..n {
                    obj(&mut rd)?;
                }
                let with_props = rd.flag()?;
                let n_seeds = rd.index(MAX_SEEDS)?;
                let mut seeds = Vec::new();
                for _ in 0..n_seeds {
                    let field = rd.index(heal_face_props::SEED_FIELDS as usize - 1)?;
                    rd.index(3)?;
                    let v = rd.finite()?;
                    if v.abs() > 1e9 || !heal_face_props::seed_ok(field as i64, v) {
                        return Err(bad("face props seed out of range"));
                    }
                    seeds.push((field as i64, v));
                }
                // UB-131: SetProps_Stage2 (run when the props passed are not at stage 2a
                // yet) dereferences the null m_pFace that SetProps_Gaps leaves for a stage
                // at or past GAPSa; the seeds apply to the props only when they are passed
                if with_props && heal_face_props::stage2_derefs_null_face(&seeds) {
                    return Err(bad(
                        "seeded props would dereference the null face of SetProps_Stage2 (UB-131)",
                    ));
                }
            }
            op::CONSTRUCT => {
                obj(&mut rd)?;
                rd.finite()?;
                rd.finite()?;
                obj(&mut rd)?;
                rd.value()?;
                rd.index(MAX_SIZE)?;
                rd.flag()?;
                rd.flag()?;
                obj(&mut rd)?;
                obj(&mut rd)?;
                obj(&mut rd)?;
            }
            // SQUEEZE: no arguments
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
            sys::sc_intersect_curve_class_p4_run(
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
