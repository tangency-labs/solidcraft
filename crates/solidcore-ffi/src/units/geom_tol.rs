//! FFI wrappers for unit `geom.tol`.
//!
//! Shim: `harness/cpp/shim/geom_tol.cpp`.
//! Upstream: `source/SMLib/inc/SmTol.h`, `source/SMLib/src/SmTol.cpp` (`SmTol`, the
//! `SmTol3d` classes) and `source/SMLib/inc/SmRelation.h` (`SmRelation`).
//!
//! | Rust         | shim                   | upstream                         |
//! |--------------|------------------------|----------------------------------|
//! | [`op`]       | `sc_geom_tol_op`       | one `SmTol` call per [`Op`]      |
//! | [`relation`] | `sc_geom_tol_relation` | a script of `SmRelation<int, int>` calls |
//! | [`curve_tols`] | `sc_geom_tol_curve`  | the `SmCurve` overloads of `GetScaledZero`, `MapTo1d`, `IsDegenerate` |
//! | [`volume_scaled_zero`] | `sc_geom_tol_volume` | `SmTol::GetScaledZero(const SmVolume&)` |

use crate::{call_with_buffer, check, FfiError};

use super::intersect_surface_drop_lib_p2::encode_curve;
pub use super::intersect_surface_drop_lib_p2::{CurveIn, MAX_COORD};

mod sys {
    extern "C" {
        pub fn sc_geom_tol_op(
            op: i32,
            model_size: f64,
            ratio: f64,
            a: *const f64,
            flag: i32,
            small: i32,
            kinds: *const i32,
            objs: *const f64,
            out: *mut f64,
            out_n: *mut i32,
        ) -> i32;
        pub fn sc_geom_tol_curve(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_tol_volume(
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
        pub fn sc_geom_tol_relation(
            card: i32,
            n1: i64,
            n2: i64,
            script: *const i64,
            n_script: i64,
            out: *mut i64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// One `SmTol` operation (see the shim for the operand layout).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `SizeEstimateToZoneTol3d(a0)`.
    SizeEstimate = 0,
    /// `InTol(a0, SmScaledZero(a1))`.
    InTol = 1,
    /// `CalcFaceZoneTol3d()`.
    CalcFaceZone = 2,
    /// `CalcEdgeZoneTol3d(a0, a1, flag ? &SmZoneTol3d(a2) : NULL)`.
    CalcEdgeZone = 3,
    /// `CalcVertexZoneTol3d(a0, a1, flag ? &SmZoneTol3d(a2) : NULL)`.
    CalcVertexZone = 4,
    /// `GetEffZero`, `GetEffZeroSq`, `GetEffZeroParam`, `GetEffZeroAngRad`.
    EffZeros = 5,
    /// `GetXSectTol3d(SmZoneTol3d(a0), SmZoneTol3d(a1))`.
    XSectFromZones = 6,
    /// `GetAngTolRad()`.
    AngTolRad = 7,
    /// `GetModelSizeEstimate()`.
    ModelSize = 8,
    /// `AreParallel(&v(a0..3), &v(a3..6), flag ? &a6 : NULL)`.
    AreParallel = 9,
    /// `ArePerpendicular(&v(a0..3), &v(a3..6), flag ? &a6 : NULL)`.
    ArePerpendicular = 10,
    /// `IsZeroSpeed(a0, a1, flag ? &SmScaledZero(a2) : NULL)`.
    IsZeroSpeed = 11,
    /// `IsDegenerate(SmVector3d(a0..3), flag ? &SmXSectTol3d(a3) : NULL)`.
    IsDegenerateVec3 = 12,
    /// `GetGapGain()`.
    GapGain = 13,
    /// `GetZoneTol3d(small)`.
    ZoneSys = 14,
    /// `GetZoneTol3d((const SmContext*)NULL, small)`.
    ZoneCtx = 15,
    /// `GetApproxTol3d(small)`.
    ApproxSys = 16,
    /// `GetApproxTol3d((const SmContext*)NULL, small)`.
    ApproxCtx = 17,
    /// `GetXSectTol3d(small)`.
    XSectSys = 18,
    /// `GetXSectTol3d((const SmContext*)NULL, small)`.
    XSectCtx = 19,
    /// `GetScaledZero()`, `GetScaledZeroSq()`, `GetScaledZeroSqrt()`.
    ScaledZeroSys = 20,
    /// `GetScaledZero`, `GetScaledZeroSq`, `GetScaledZeroSqrt` of `small` (1..=4)
    /// objects.
    ScaledZeroN = 21,
    /// Every tolerance constructor and `operator=` applied to `a0`.
    TolCtors = 22,
    /// `SmTol3d t(a0); t *= a1`.
    TolMul = 23,
    /// `MapTo1d(SmTol3d(a0), SmVector3d(a1, a2, a3), SmExtent1d(a4, a5))`.
    MapTo1d = 24,
}

/// Object kind for [`Op::ScaledZeroN`]: `double`.
pub const KIND_DOUBLE: i32 = 0;
/// Object kind for [`Op::ScaledZeroN`]: `SmVector2d`.
pub const KIND_VEC2: i32 = 1;
/// Object kind for [`Op::ScaledZeroN`]: `SmVector3d`.
pub const KIND_VEC3: i32 = 2;
/// Object kind for [`Op::ScaledZeroN`]: `SmExtent3d`.
pub const KIND_EXTENT3: i32 = 3;
/// Object kind for [`Op::ScaledZeroN`]: `SmExtent1d` (`d[0]`, `d[1]`, swapped if inverted).
pub const KIND_EXTENT1: i32 = 4;
/// Object kind for [`Op::ScaledZeroN`]: `SmExtent2d` (`d[0..4]`, stored as given).
pub const KIND_EXTENT2: i32 = 5;

/// Operands of one [`op`] call.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct OpArgs {
    /// System model size estimate (`SmTol::SetModelSizeEstimate`) for the call.
    pub model_size: f64,
    /// System large/small size ratio (`SmTol::SetLargeSmallSizeRatio`) for the call.
    pub ratio: f64,
    /// Scalar operands.
    pub a: [f64; 8],
    /// Optional-argument switch.
    pub flag: bool,
    /// `SmSmallTopoType` (0..=4), or the object count of [`Op::ScaledZeroN`].
    pub small: i32,
    /// Object kinds of [`Op::ScaledZeroN`].
    pub kinds: [i32; 4],
    /// Object data of [`Op::ScaledZeroN`] (6 doubles each).
    pub objs: [f64; 24],
}

/// Runs `op` in upstream C++ (with the system tolerances set from `args` and reset to
/// the defaults afterwards) and returns its result values.
pub fn op(op: Op, args: &OpArgs) -> Result<Vec<f64>, FfiError> {
    if !(0..=4).contains(&args.small) {
        return Err(FfiError::InvalidArgument(format!("small {}", args.small)));
    }
    if op == Op::ScaledZeroN && args.small == 0 {
        return Err(FfiError::InvalidArgument("object count 0".into()));
    }
    if args.kinds.iter().any(|k| !(0..=5).contains(k)) {
        return Err(FfiError::InvalidArgument("object kind".into()));
    }
    let mut out = [0.0f64; 16];
    let mut n = 0i32;
    // SAFETY: `a` (8), `kinds` (4) and `objs` (24) are valid for the reads the shim
    // makes; `out` is valid for 16 writes (the shim writes at most 14); `n` is a valid
    // i32. The shim retains nothing.
    let st = unsafe {
        sys::sc_geom_tol_op(
            op as i32,
            args.model_size,
            args.ratio,
            args.a.as_ptr(),
            i32::from(args.flag),
            args.small,
            args.kinds.as_ptr(),
            args.objs.as_ptr(),
            out.as_mut_ptr(),
            &raw mut n,
        )
    };
    check(st)?;
    let n = usize::try_from(n)
        .ok()
        .filter(|&n| n <= out.len())
        .ok_or_else(|| FfiError::InvalidArgument(format!("shim returned {n} values")))?;
    Ok(out.get(..n).unwrap_or_default().to_vec())
}

/// One step of a [`relation`] script.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum RelationStep {
    /// `RelatePair(first, second)`.
    Pair(i64, i64),
    /// `RelateToMany(first, seconds)`.
    Many(i64, Vec<i64>),
    /// `RemoveAll()`.
    RemoveAll,
    /// `RemovePair(first, second)`.
    RemovePair(i64, i64),
}

/// Runs `steps` on an upstream `SmRelation<int, int>` with cardinality `card` over
/// `n_first` first and `n_second` second objects. Returns the flat result of
/// `sc_geom_tol_relation`: the status of each step, then per first object
/// `GetSecond` (index or -1), `count`, sorted `GetSeconds`; then `count` + sorted
/// `GetFirsts` per second object, `GetAllFirsts` and `GetAllSeconds`.
pub fn relation(
    card: i32,
    n_first: i64,
    n_second: i64,
    steps: &[RelationStep],
) -> Result<Vec<i64>, FfiError> {
    if !(0..=3).contains(&card) {
        return Err(FfiError::InvalidArgument(format!("cardinality {card}")));
    }
    if !(0..=10_000).contains(&n_first) || !(0..=10_000).contains(&n_second) {
        return Err(FfiError::InvalidArgument("object count".into()));
    }
    let f_ok = |f: i64| (0..n_first).contains(&f);
    let s_ok = |s: i64| (0..n_second).contains(&s);
    let mut script = Vec::new();
    for step in steps {
        match step {
            RelationStep::Pair(f, s) => {
                if !f_ok(*f) || !s_ok(*s) {
                    return Err(FfiError::InvalidArgument("pair index".into()));
                }
                script.extend([0, *f, *s]);
            }
            RelationStep::Many(f, ss) => {
                if !f_ok(*f) || !ss.iter().all(|s| s_ok(*s)) {
                    return Err(FfiError::InvalidArgument("many index".into()));
                }
                script.extend([1, *f, ss.len() as i64]);
                script.extend(ss.iter().copied());
            }
            RelationStep::RemoveAll => script.push(2),
            RelationStep::RemovePair(f, s) => {
                if !f_ok(*f) || !s_ok(*s) {
                    return Err(FfiError::InvalidArgument("remove pair index".into()));
                }
                script.extend([3, *f, *s]);
            }
        }
    }
    let n_script = script.len() as i64;
    call_with_buffer(64, |buf, len| {
        // SAFETY: `script` is valid for `n_script` reads and every index in it was
        // checked against `n_first` / `n_second` above; `buf` is valid for
        // `buf.len()` writes and `len` for one write.
        unsafe {
            sys::sc_geom_tol_relation(
                card,
                n_first,
                n_second,
                script.as_ptr(),
                n_script,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// On a real `SmLine`, `SmCircle` or `SmBSplineCurve` `curve`: `GetScaledZero(const
/// SmCurve&)`, `MapTo1d(SmTol3d(tol), param, curve)` and `IsDegenerate(&curve,
/// SmExtent1d(interval), xsect ? &SmXSectTol3d(xsect) : NULL)` (as `1.0`/`0.0`), in this
/// order.
///
/// Accepts finite values of at most [`MAX_COORD`] magnitude. A `param` or interval
/// outside the curve's range is not undefined: `SmBSplineCurve::Evaluate` clamps and a
/// line and a circle extend, so the ignored `Evaluate`s of `SmCurve::IsDegenerate` never
/// fail (see `intersect_surface_drop_lib_p3`).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for a rejected input.
pub fn curve_tols(
    curve: &CurveIn,
    param: f64,
    tol: f64,
    interval: [f64; 2],
    xsect: Option<f64>,
) -> Result<Vec<f64>, FfiError> {
    let fin = |x: f64| {
        if x.is_finite() && x.abs() <= MAX_COORD {
            Ok(x)
        } else {
            Err(FfiError::InvalidArgument(
                "value not finite or too large".into(),
            ))
        }
    };
    let mut v = Vec::new();
    encode_curve(curve, &mut v)?;
    v.push(fin(param)?);
    v.push(fin(tol)?);
    v.push(fin(interval[0])?);
    v.push(fin(interval[1])?);
    v.push(if xsect.is_some() { 1.0 } else { 0.0 });
    v.push(fin(xsect.unwrap_or(0.0))?);
    let n_in = i64::try_from(v.len()).map_err(|_| FfiError::InvalidArgument("too long".into()))?;
    call_with_buffer(8, |buf: &mut [f64], len| {
        // SAFETY: `v` is valid for `n_in` reads; `buf` for `buf.len()` writes; `len` is a
        // valid i64; nothing is retained.
        unsafe {
            sys::sc_geom_tol_curve(
                v.as_ptr(),
                n_in,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

fn validate_volume_poles(poles: &[f64; 24]) -> Result<(), FfiError> {
    if poles.iter().any(|x| !x.is_finite()) {
        return Err(FfiError::InvalidArgument("value not finite".into()));
    }
    Ok(())
}

/// Returns one scaled-zero value for a real degree-1, unit-weight B-spline volume.
///
/// The eight poles are XYZ triples in a 2x2x2 mesh, with the last mesh index fastest.
/// Each knot vector is `[0, 0, 1, 1]`. All finite coordinates are accepted.
///
/// # Errors
///
/// Returns [`FfiError::InvalidArgument`] for nonfinite coordinates or an unexpected
/// output count, or the native shim's error.
pub fn volume_scaled_zero(poles: &[f64; 24]) -> Result<Vec<f64>, FfiError> {
    validate_volume_poles(poles)?;
    let mut out = [0.0; 1];
    let mut n = 0i64;
    // SAFETY: `poles` is valid for 24 reads; `out` for one write; `n` is a valid
    // i64. All coordinates are finite, counts are fixed, and nothing is retained.
    let st =
        unsafe { sys::sc_geom_tol_volume(poles.as_ptr(), 24, out.as_mut_ptr(), 1, &raw mut n) };
    check(st)?;
    if n != 1 {
        return Err(FfiError::InvalidArgument(format!(
            "shim returned {n} values, expected 1"
        )));
    }
    Ok(out.to_vec())
}

#[cfg(test)]
mod tests {
    use super::validate_volume_poles;
    use crate::FfiError;

    #[test]
    fn volume_poles_reject_nonfinite_at_every_position() {
        for index in 0..24 {
            for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
                let mut poles = [0.0; 24];
                poles[index] = value;
                assert!(matches!(
                    validate_volume_poles(&poles),
                    Err(FfiError::InvalidArgument(_))
                ));
            }
        }
    }

    #[test]
    fn volume_poles_accept_finite_meshes_without_shape_constraints() {
        let ordinary = [
            [0.0, 0.0, 0.0],
            [0.0, 0.0, 1.0],
            [0.0, 1.0, 0.0],
            [0.0, 1.0, 1.0],
            [1.0, 0.0, 0.0],
            [1.0, 0.0, 1.0],
            [1.0, 1.0, 0.0],
            [1.0, 1.0, 1.0],
        ];
        let mut skew = ordinary;
        for pole in &mut skew {
            pole[0] += 0.5 * pole[1] - 0.25 * pole[2];
            pole[1] += 0.75 * pole[2];
        }
        let mut reversed = ordinary;
        for pole in &mut reversed {
            pole[0] = -pole[0];
        }
        for corners in [[[0.0; 3]; 8], ordinary, skew, reversed, [[f64::MAX; 3]; 8]] {
            let poles = std::array::from_fn(|index| corners[index / 3][index % 3]);
            assert!(validate_volume_poles(&poles).is_ok());
        }
    }
}
