//! FFI wrappers for unit `query.face_p3`.
//!
//! Shim: `harness/cpp/shim/query_face_p3.cpp` (`sc_query_face_p3_eval`).
//! Upstream: `source/SMLib/src/SmFace.cpp` (`SmFace::CalculateTightBoundingBox`,
//! `SmFace::SectorClassifyCurveOn`, `sms_FindAndClassifyLoopBisector`, and the file-static
//! `sm_FaceClassifyUVLine` through `SmFace::CalculateAnInternalPoint`).
//!
//! Every input starts with a B-rep block of unit `query.face_p4` (its shim's builders are
//! compiled into this unit's shim; [`super::query_face_p4`] documents and validates the
//! block), then `face_sel` (the face, modulo the face count) and the [`Kind`]'s case block:
//!
//! | [`Kind`]   | case block |
//! |------------|------------|
//! | `Tight`    | (none) |
//! | `Internal` | (none) |
//! | `Bisector` | `target_sel mask containment`: the target loop (modulo the loop count) of the closed loops `SetProps_Stage3` collects (the props are built as unit `query.face_p4`'s props mode `1`); the loops whose bit is set in `mask` are marked classified, their `SmClassifyLoopIO` records (zero-filled, as `SetSize` makes them) get `m_eContainmentType` `containment` |
//! | `Sector`   | `loop_sel eu_sel mode frac curve_kind ta tb curve_tol edge_tol singular sa sb uv_ok` |
//!
//! Sector: the edge use `eu_sel` (modulo) of loop `loop_sel` (modulo, `GetLoops` order) in
//! `GetEdgeuses` order; `mode` `0` crosses its edge at fraction `frac` of the edge
//! interval (both edge uses the same, `SM_OT_SAME` and `SM_OT_OPPOSITE`), `1` its start
//! vertex (it, `SM_OT_SAME`, and its clockwise neighbour at its end, `SM_OT_OPPOSITE`, as
//! `SmFace` classifies at a vertex), `2` the same with its radial edge use as the second.
//! The curve is an `SmLine`: `curve_kind` `0` from the crossing point to the surface
//! point at fractions `(ta, tb)` of the face domain (classified at its start), `1` the
//! reverse (classified at its end, `SM_OT_OPPOSITE`), `2` the 2D line from the crossing
//! point's surface parameters (`DropPoint`) to `(ta, tb)`. `edge_tol` `-1` uses the edges'
//! zone tolerances. `(sa, sb)` are the fractions of the singularity parameter.
//!
//! With `export_only` the output is `0`, the graph of [`super::topology_face_p2`], the
//! resolved case block and its length; otherwise `0` and the results:
//!
//! | [`Kind`]   | resolved case (export) | results |
//! |------------|------------------------|---------|
//! | `Tight`    | `rejected [face]` | `rejected [status min(3) max(3) has_trim_cache]` |
//! | `Internal` | `rejected [face]` | `rejected [status point(3)]` |
//! | `Bisector` | `rejected [face target n loop* mask containment closed_u closed_v poles n_pp pp(3)* bad_missing_seam zone_tol]` | `rejected [status start_on_seam stop_on_seam closed_3d has_curve [line_start(2) line_end(2)] n (min max (class vertex)x3)*]` |
//! | `Sector`   | `rejected [face eu1 eu2 p1 p2 tol1 tol2 o1 o2 dim start(3) end(3) curve_orient curve_tol singular su sv uv_ok]` | `rejected [status inside problem classify_problem]` |
//!
//! A failed build gives `-1 status`. **Rejected** (`rejected` `1`): no face; for the
//! bisector no closed loop, a face without surface, or a pole without its four pole
//! points; for the sector a face without surface or loops, a vertex loop, an edge (or
//! second edge use) without curve, a crossing point that does not drop to the surface,
//! a degenerate curve (shorter than `1e-9`).
//!
//! [`eval`] accepts only what upstream handles without undefined behaviour: the B-rep
//! block of [`super::query_face_p4::check_brep_block`], selectors up to `2^20`, fractions in
//! `[0, 1]`, tolerances in `[0, 1]`, a mask up to `255`, a containment code up to `8`.
//! For `Tight` the B-rep block's values (other than the rational / planar markers) are
//! also bounded by [`TIGHT_MAX_COORD`]: upstream's global point solves on a B-spline
//! face with a control point of `1e10` take 20-55 s and one of `1e300` over 10 minutes
//! (a time bound, not undefined behaviour; the generated and fixture cases stay below
//! `1e4`).

use crate::{call_with_buffer, FfiError};

/// The upstream function a call runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[allow(missing_docs)]
pub enum Kind {
    Tight = 0,
    Bisector = 1,
    Sector = 2,
    Internal = 3,
}

mod sys {
    extern "C" {
        pub fn sc_query_face_p3_eval(
            kind: i32,
            export_only: i32,
            path: *const std::ffi::c_char,
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

/// Sequential reader over the case block.
struct Reader<'a> {
    input: &'a [f64],
    at: usize,
}

impl Reader<'_> {
    fn num(&mut self) -> Result<f64, FfiError> {
        let v = self
            .input
            .get(self.at)
            .copied()
            .ok_or_else(|| bad("input too short"))?;
        self.at += 1;
        Ok(v)
    }

    fn index(&mut self, max: usize) -> Result<usize, FfiError> {
        let v = self.num()?;
        if v.fract() != 0.0 || v < 0.0 || v > max as f64 {
            return Err(bad(format!("bad index/count {v} (max {max})")));
        }
        Ok(v as usize)
    }

    fn unit(&mut self, what: &str) -> Result<f64, FfiError> {
        let v = self.num()?;
        if !(0.0..=1.0).contains(&v) {
            return Err(bad(format!("{what}: outside [0, 1]")));
        }
        Ok(v)
    }
}

/// The largest selector value.
pub const MAX_SELECTOR: usize = 1 << 20;

/// The largest magnitude of a B-rep block value for `Tight` (module docs).
pub const TIGHT_MAX_COORD: f64 = 1.0e6;

/// Runs `kind` on `input` (a B-rep block, `face_sel` and the case block); `path` is the
/// `.smb` file of a kind `0` B-rep block (ignored otherwise). With `export_only` the
/// B-rep graph and the resolved case are returned instead of the results.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim would not handle safely (module
/// docs); [`FfiError::Status`] for a shim failure.
pub fn eval(kind: Kind, export_only: bool, path: &str, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    let at = super::query_face_p4::check_brep_block(input, path)?;
    let mut rd = Reader { input, at };
    rd.index(MAX_SELECTOR)?;
    match kind {
        Kind::Tight => {
            // the query.face_p4 markers: no weight (-f64::MAX), planar data (f64::MAX)
            let block = input.get(..at).unwrap_or_default();
            if block
                .iter()
                .any(|v| v.abs() > TIGHT_MAX_COORD && v.abs() != f64::MAX)
            {
                return Err(bad("tight: B-rep value beyond TIGHT_MAX_COORD"));
            }
        }
        Kind::Internal => {}
        Kind::Bisector => {
            rd.index(MAX_SELECTOR)?;
            rd.index(255)?;
            rd.index(8)?;
        }
        Kind::Sector => {
            rd.index(MAX_SELECTOR)?;
            rd.index(MAX_SELECTOR)?;
            rd.index(2)?;
            rd.unit("edge fraction")?;
            rd.index(2)?;
            rd.unit("target u fraction")?;
            rd.unit("target v fraction")?;
            rd.unit("curve tolerance")?;
            let edge_tol = rd.num()?;
            if edge_tol != -1.0 && !(0.0..=1.0).contains(&edge_tol) {
                return Err(bad("edge tolerance: -1 or in [0, 1]"));
            }
            rd.index(1)?;
            rd.unit("singularity u fraction")?;
            rd.unit("singularity v fraction")?;
            rd.index(1)?;
        }
    }
    if rd.at != input.len() {
        return Err(bad("trailing input"));
    }
    let c_path = std::ffi::CString::new(path).map_err(|_| bad("path contains NUL"))?;
    call_with_buffer(1 << 16, |buf, len| {
        // SAFETY: input is valid for input.len() reads, c_path is a NUL-terminated
        // string, buf is valid for buf.len() writes and len for one write.
        unsafe {
            sys::sc_query_face_p3_eval(
                kind as i32,
                i32::from(export_only),
                c_path.as_ptr(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
