//! FFI wrappers for unit `geom.periodic_extent1d`.
//!
//! Shim: `harness/cpp/shim/geom_periodic_extent1d.cpp`.
//! Upstream: `source/SMLib/inc/SmPeriodicExtent1d.h`,
//! `source/SMLib/src/SmPeriodicExtent1d.cpp` (`SmPeriodicExtent1d`).
//!
//! | Rust   | shim                           | upstream                                  |
//! |--------|--------------------------------|-------------------------------------------|
//! | [`op`] | `sc_geom_periodic_extent1d_op` | one `SmPeriodicExtent1d` method per [`Op`] |

use crate::{check, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_periodic_extent1d_op(
            op: i32,
            a: *const f64,
            b: *const f64,
            s: *const f64,
            flag: i32,
            has_period: i32,
            out: *mut f64,
            op_status: *mut i32,
        ) -> i32;
    }
}

/// One `SmPeriodicExtent1d` operation on extents `a`, `b` and scalars `s`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Op {
    /// `SmPeriodicExtent1d(s[0])`.
    NewFull = 0,
    /// Copy constructor of `a`.
    Copy = 1,
    /// `a.GetPeriod()`.
    GetPeriod = 2,
    /// `a.SetFullPeriod(flag)`.
    SetFullPeriod = 3,
    /// `a.IsEmpty()`.
    IsEmpty = 4,
    /// `a.IsFullPeriod()`.
    IsFullPeriod = 5,
    /// `a.CrossesSeam()`.
    CrossesSeam = 6,
    /// `a.MapValue(s[0])`.
    MapValue = 7,
    /// `a.NormalizeValue(s[0])`.
    NormalizeValue = 8,
    /// `a.Compliment()`.
    Compliment = 9,
    /// `a.AddInterval(s[0], s[1], flag)`.
    AddInterval = 10,
    /// `a.ContainsValue(s[0])`.
    ContainsValue = 11,
    /// `a.SetMinMax(s[0], s[1], has_period ? &s[2] : NULL, flag)`.
    SetMinMax = 12,
    /// `a.IsContainedBy(b)`.
    IsContainedBy = 13,
    /// `a.AreDisjoint(b)`.
    AreDisjoint = 14,
    /// `a.GetMid()`.
    GetMid = 15,
}

/// Result of [`op`].
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct OpResult {
    /// Members of `a` (or of the constructed extent) afterwards:
    /// `[min, max, period, full (0/1)]`.
    pub state: [f64; 4],
    /// Scalar result (booleans as `1.0`/`0.0`; `0.0` when the operation has none).
    pub r: f64,
    /// `SmStatus` returned by the operation (`0` when it returns none).
    pub status: i32,
}

/// Runs `op` in upstream C++. Extents are raw members `[min, max, period, full]`;
/// `full` must be `0.0` or `1.0`.
pub fn op(
    op: Op,
    a: [f64; 4],
    b: [f64; 4],
    s: [f64; 3],
    flag: bool,
    has_period: bool,
) -> Result<OpResult, FfiError> {
    for e in [a, b] {
        if e[3] != 0.0 && e[3] != 1.0 {
            return Err(FfiError::InvalidArgument(format!("full flag {}", e[3])));
        }
    }
    let mut out = [0.0f64; 5];
    let mut status = 0i32;
    // SAFETY: `a`, `b` are valid for 4 reads, `s` for 3, `out` for 5 writes, `status`
    // is a valid i32; the shim reads/writes only within these and retains nothing.
    let st = unsafe {
        sys::sc_geom_periodic_extent1d_op(
            op as i32,
            a.as_ptr(),
            b.as_ptr(),
            s.as_ptr(),
            i32::from(flag),
            i32::from(has_period),
            out.as_mut_ptr(),
            &raw mut status,
        )
    };
    check(st)?;
    let [m0, m1, m2, m3, r] = out;
    Ok(OpResult {
        state: [m0, m1, m2, m3],
        r,
        status,
    })
}
