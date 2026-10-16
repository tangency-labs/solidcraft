//! FFI wrappers for unit `geom.point_set`.
//!
//! Shim: `harness/cpp/shim/geom_point_set.cpp`.
//! Upstream: `source/SMLib/src/SmPointSet.cpp`, `source/SMLib/inc/SmPointSet.h`
//! (`SmPointSet3d`, `SmPointSequence`).
//!
//! | Rust    | shim                    | upstream                                  |
//! |---------|-------------------------|-------------------------------------------|
//! | [`run`] | `sc_geom_point_set_run` | `SmPointSequence` methods, per [`Cmd`]    |
//!
//! [`run`] builds one `SmPointSequence` and runs a program of [`Cmd`]s on it, returning
//! the constructor-unitized normal (3 values) followed by every command's output
//! (layouts in the shim's op table).

use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_geom_point_set_run(
            pts: *const f64,
            n_pts: i64,
            tol: f64,
            closed: i32,
            has_normal: i32,
            normal: *const f64,
            prog: *const f64,
            n_prog: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// One command of a point-sequence program (shim op table).
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Cmd {
    /// `GetCentroid()`; output 3.
    Centroid,
    /// `GetNormal()`; output 3.
    Normal,
    /// `GetBBox(z)`; output min 3, max 3, `z` after the call 3.
    BBox(Option<[f64; 3]>),
    /// `GetTightPBox(z)`; output basis 9, intervals 6, `z` after the call 3.
    TightPBox(Option<[f64; 3]>),
    /// `GetArea()`; output 1.
    Area,
    /// `PointContainment(p)`; output the enum value.
    Containment([f64; 3]),
    /// `SmPointSet3d::ClearCache()`; no output.
    ClearCache,
    /// `GetPoints().GetSize()`; output 1.
    Size,
}

impl Cmd {
    /// Number of output values.
    #[must_use]
    pub const fn output_len(self) -> usize {
        match self {
            Cmd::Centroid | Cmd::Normal => 3,
            Cmd::BBox(_) => 9,
            Cmd::TightPBox(_) => 18,
            Cmd::Area | Cmd::Containment(_) | Cmd::Size => 1,
            Cmd::ClearCache => 0,
        }
    }

    fn encode(self, out: &mut Vec<f64>) {
        let opt = |out: &mut Vec<f64>, z: Option<[f64; 3]>| match z {
            Some(z) => {
                out.push(1.0);
                out.extend(z);
            }
            None => out.extend([0.0; 4]),
        };
        match self {
            Cmd::Centroid => out.push(0.0),
            Cmd::Normal => out.push(1.0),
            Cmd::BBox(z) => {
                out.push(2.0);
                opt(out, z);
            }
            Cmd::TightPBox(z) => {
                out.push(3.0);
                opt(out, z);
            }
            Cmd::Area => out.push(4.0),
            Cmd::Containment(p) => {
                out.push(5.0);
                out.extend(p);
            }
            Cmd::ClearCache => out.push(6.0),
            Cmd::Size => out.push(7.0),
        }
    }
}

/// Builds `SmPointSequence(points, tol, closed, UNSURE, normal)` and runs `prog` on it.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for `Area`/`Containment` on an empty point set, or
/// `TightPBox` on an empty set with a normal (upstream reads outside the array); shim
/// statuses otherwise.
pub fn run(
    points: &[[f64; 3]],
    tol: f64,
    closed: bool,
    normal: Option<[f64; 3]>,
    prog: &[Cmd],
) -> Result<Vec<f64>, FfiError> {
    if points.is_empty()
        && prog
            .iter()
            .any(|c| matches!(c, Cmd::Area | Cmd::Containment(_)))
    {
        return Err(FfiError::InvalidArgument(
            "area/containment of an empty point set".into(),
        ));
    }
    if points.is_empty() && normal.is_some() && prog.iter().any(|c| matches!(c, Cmd::TightPBox(_)))
    {
        return Err(FfiError::InvalidArgument(
            "tight box of an empty planar point set".into(),
        ));
    }
    let flat_pts: Vec<f64> = points.iter().flatten().copied().collect();
    let mut flat = Vec::new();
    for c in prog {
        c.encode(&mut flat);
    }
    let n_pts =
        i64::try_from(points.len()).map_err(|_| FfiError::InvalidArgument("points".into()))?;
    let n_prog = i64::try_from(flat.len()).map_err(|_| FfiError::InvalidArgument("prog".into()))?;
    let nrm = normal.unwrap_or([0.0; 3]);
    let cap_hint = 3 + prog.iter().map(|c| c.output_len()).sum::<usize>();
    call_with_buffer(cap_hint, |buf: &mut [f64], len| {
        // SAFETY: `flat_pts` is valid for `3 * n_pts` reads, `nrm` for 3, `flat` for
        // `n_prog` reads and is a well-formed program (validated again in the shim);
        // `buf` is valid for `buf.len()` writes; `len` is a valid writable i64.
        unsafe {
            sys::sc_geom_point_set_run(
                flat_pts.as_ptr(),
                n_pts,
                tol,
                i32::from(closed),
                i32::from(normal.is_some()),
                nrm.as_ptr(),
                flat.as_ptr(),
                n_prog,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
