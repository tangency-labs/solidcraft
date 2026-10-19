//! FFI wrappers for unit `surface.cone_p2`.
//!
//! Shim: `harness/cpp/shim/surface_cone_p2.cpp` (`sc_surface_cone_p2_eval`).
//! Upstream: `source/SMLib/src/SmCone.cpp` (`CreateOffsetSurface`, `IsNurbSurfaceCone`,
//! `RebuildSTEPFromNURBParameters`, the file-static `sm_GetEllipseTrimIntervals`).
//!
//! [`cone_op`] builds a cone or cylinder from a [`Build`] (the `surface.cone_p1` block,
//! with its `MakeNurb`) and runs one [`Op`] on it. The record is the construction status
//! (raw `SmStatus`, `1000` = `SM_SUCCESS`), then on success:
//!
//! * `Offset`: `status has [state]` (`CreateOffsetSurface(ctx, distance,
//!   SmApproxTol3d(tol), ..)`; `has` is a non-`NULL` result);
//! * `Recognize`: `result has [state]`: the [`Edit`] is applied to the cone's NURB, the
//!   `SmBSplineSurface` copy of it is given to `IsNurbSurfaceCone(ctx, .., scale)`
//!   (`result` is its `SmBoolean`, or the error status it returns through `SER`);
//! * `Rebuild`: `status state`: the [`Edit`] is applied to the cone's NURB, then
//!   `RebuildSTEPFromNURBParameters`; the state is the cone's afterwards;
//! * `Trim`: `1000 n [min max]*n`, the intervals of `sm_GetEllipseTrimIntervals`.
//!
//! A state is `type is_cyl is_cone is_cylinder base semi inside swap planar make_nurb_gen
//! bounded O[3] X[3] Y[3] D[4] bot_r top_r MAXD[4] apex_status apex_cyl apex[3]
//! apex_param height_ok [height] have_polar angle_count [first last] polar_curve
//! gen_curve has_nurb [p q nu nv cpts(4 nu nv, v fastest) nku KU nkv KV]`, a curve `has
//! [type step0 step1 has_nurb [p n cpts(4 n) nk knots]]`.
//!
//! The op record is the single value `-1002` (`SC_ERR_ARG`) where the upstream code would
//! read uninitialised values or crash (the shim tests this with the same upstream calls):
//! `Offset` and `Trim` on a cone with NURB but no polar curve (`GetHeight`); `Offset`
//! whose apex truncation cannot make its cone or gets a non-finite height; `Trim` whose
//! domain conversion fails or whose seam line is parallel to the ellipse plane (or of zero
//! length).
//!
//! The wrappers accept finite values only (as the `surface.cone_p1` wrappers; upstream
//! answers statuses or degenerate geometry for any finite input) and, for `Trim`, ellipse
//! axes that are long enough and far from parallel.

use crate::{call_with_buffer, FfiError};

pub use super::surface_cone_p1::{Build, Mode};

mod sys {
    extern "C" {
        pub fn sc_surface_cone_p2_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// An edit of the cone's NURB control points (homogeneous `x y z w`), applied before
/// `IsNurbSurfaceCone` or `RebuildSTEPFromNURBParameters`. The last row is the last
/// control points along the generator (linear) direction: `u` with `swap_uv`, else `v`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Edit {
    /// No edit.
    None,
    /// `p = R p + w t` on the last row (`R` row-major).
    MoveLast {
        /// Rotation matrix, row-major.
        r: [f64; 9],
        /// Translation.
        t: [f64; 3],
    },
    /// `(x, y, z, w) *= f` on the last row (the same Euclidean points).
    ScaleLast {
        /// Factor.
        f: f64,
    },
    /// `p = R p + w t` on every control point.
    MoveAll {
        /// Rotation matrix, row-major.
        r: [f64; 9],
        /// Translation.
        t: [f64; 3],
    },
    /// `z = 0` on every control point.
    Flatten,
}

impl Edit {
    /// The block `kind f R[9] t[3]`.
    fn block(&self) -> [f64; 14] {
        let mut o = [0.0; 14];
        let (kind, f, r, t) = match *self {
            Edit::None => (0.0, 0.0, [0.0; 9], [0.0; 3]),
            Edit::MoveLast { r, t } => (1.0, 0.0, r, t),
            Edit::ScaleLast { f } => (2.0, f, [0.0; 9], [0.0; 3]),
            Edit::MoveAll { r, t } => (3.0, 0.0, r, t),
            Edit::Flatten => (4.0, 0.0, [0.0; 9], [0.0; 3]),
        };
        let mut it = o.iter_mut();
        for v in [kind, f].into_iter().chain(r).chain(t) {
            if let Some(slot) = it.next() {
                *slot = v;
            }
        }
        o
    }
}

/// The upstream member run on the cone.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `CreateOffsetSurface(ctx, distance, SmApproxTol3d(tol), ..)`.
    Offset {
        /// Signed offset distance.
        distance: f64,
        /// Approximation tolerance.
        tol: f64,
    },
    /// `IsNurbSurfaceCone(ctx, copy of the edited NURB, .., scale)`.
    Recognize {
        /// NURB edit.
        edit: Edit,
        /// Tolerance scale.
        scale: f64,
    },
    /// `RebuildSTEPFromNURBParameters` after an edit of the NURB.
    Rebuild {
        /// NURB edit.
        edit: Edit,
    },
    /// `sm_GetEllipseTrimIntervals(ctx, origin, x, y, rx, ry, start, end, cone, dom, tol)`.
    Trim {
        /// Ellipse origin.
        origin: [f64; 3],
        /// Ellipse X axis.
        x: [f64; 3],
        /// Ellipse Y axis.
        y: [f64; 3],
        /// X radius.
        rx: f64,
        /// Y radius.
        ry: f64,
        /// Start angle (degrees).
        start: f64,
        /// End angle (degrees).
        end: f64,
        /// The cone's NURB trim domain as fractions `f0 f1 f2 f3` of its natural domain
        /// `D`: the rectangle with the corners `D.Evaluate(f0, f1)` and
        /// `D.Evaluate(f2, f3)` (each in `[0, 1]`).
        dom: [f64; 4],
        /// Tolerance.
        tol: f64,
    },
}

impl Op {
    fn code(&self) -> i32 {
        match self {
            Op::Offset { .. } => 1,
            Op::Recognize { .. } => 2,
            Op::Rebuild { .. } => 3,
            Op::Trim { .. } => 4,
        }
    }
}

fn bad(msg: impl Into<String>) -> FfiError {
    FfiError::InvalidArgument(msg.into())
}

fn finite(v: &[f64], what: &str) -> Result<(), FfiError> {
    if v.iter().all(|x| x.is_finite()) {
        Ok(())
    } else {
        Err(bad(format!("{what}: non-finite value")))
    }
}

fn len(v: [f64; 3]) -> f64 {
    let [x, y, z] = v;
    (x * x + y * y + z * z).sqrt()
}

fn cross(a: [f64; 3], b: [f64; 3]) -> [f64; 3] {
    let [ax, ay, az] = a;
    let [bx, by, bz] = b;
    [ay * bz - az * by, az * bx - ax * bz, ax * by - ay * bx]
}

/// Checks two axes are long enough and far from parallel.
fn axes(x: [f64; 3], y: [f64; 3], what: &str) -> Result<(), FfiError> {
    finite(&x, what)?;
    finite(&y, what)?;
    let (lx, ly) = (len(x), len(y));
    if lx < 1e-6 || ly < 1e-6 || len(cross(x, y)) < 1e-3 * lx * ly {
        return Err(bad(format!("{what}: degenerate or parallel axes")));
    }
    Ok(())
}

fn b(v: bool) -> f64 {
    f64::from(u8::from(v))
}

/// The `surface.cone_p1` block `mode oob O[3] X[3] Y[3] p[8]`.
fn push_build(o: &mut Vec<f64>, s: &Build) -> Result<(), FfiError> {
    finite(&s.origin, "origin")?;
    finite(&s.x, "placement")?;
    finite(&s.y, "placement")?;
    let (code, p): (f64, [f64; 8]) = match s.mode {
        Mode::CanonicalSemi {
            bot,
            semi,
            height: h,
            swap,
            inside,
        } => {
            finite(&[bot, semi, h.unwrap_or(0.0)], "canonical")?;
            (
                0.0,
                [bot, semi, b(h.is_some()), h.unwrap_or(0.0), b(swap), b(inside), 0.0, 0.0],
            )
        }
        Mode::Canonical {
            bot,
            top,
            height: h,
            swap,
            inside,
        } => {
            finite(&[bot, top, h], "canonical")?;
            (1.0, [bot, top, h, b(swap), b(inside), 0.0, 0.0, 0.0])
        }
        Mode::CylCanonical { radius } => {
            finite(&[radius], "cylinder")?;
            (2.0, [radius, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0])
        }
        Mode::ConeUnbounded {
            base,
            semi,
            swap,
            inside,
        } => {
            finite(&[base, semi], "cone")?;
            (3.0, [base, semi, b(swap), b(inside), 0.0, 0.0, 0.0, 0.0])
        }
        Mode::ConeBounded {
            bot,
            top,
            start,
            end,
            height: h,
            swap,
            inside,
            make_nurb_gen,
        } => {
            finite(&[bot, top, start, end, h], "cone")?;
            (
                4.0,
                [bot, top, start, end, h, b(swap), b(inside), b(make_nurb_gen)],
            )
        }
        Mode::CylBounded {
            radius,
            start,
            end,
            height: h,
            swap,
            inside,
            make_nurb_gen,
        } => {
            finite(&[radius, start, end, h], "cylinder")?;
            (
                5.0,
                [radius, start, end, h, b(swap), b(inside), b(make_nurb_gen), 0.0],
            )
        }
        Mode::CylUnbounded {
            radius,
            swap,
            inside,
        } => {
            finite(&[radius], "radius")?;
            (6.0, [radius, b(swap), b(inside), 0.0, 0.0, 0.0, 0.0, 0.0])
        }
    };
    o.extend([code, b(s.oob)]);
    o.extend_from_slice(&s.origin);
    o.extend_from_slice(&s.x);
    o.extend_from_slice(&s.y);
    o.extend_from_slice(&p);
    Ok(())
}

fn push_edit(o: &mut Vec<f64>, e: &Edit) -> Result<(), FfiError> {
    let block = e.block();
    finite(&block, "edit")?;
    o.extend_from_slice(&block);
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Offset { distance, tol } => {
            finite(&[*distance, *tol], "offset")?;
            o.extend([*distance, *tol]);
        }
        Op::Recognize { edit, scale } => {
            push_edit(o, edit)?;
            finite(&[*scale], "recognize scale")?;
            o.push(*scale);
        }
        Op::Rebuild { edit } => push_edit(o, edit)?,
        Op::Trim {
            origin,
            x,
            y,
            rx,
            ry,
            start,
            end,
            dom,
            tol,
        } => {
            finite(origin, "trim origin")?;
            axes(*x, *y, "trim ellipse")?;
            finite(&[*rx, *ry, *start, *end, *tol], "trim")?;
            if !dom.iter().all(|f| (0.0..=1.0).contains(f)) {
                return Err(bad("trim domain: fractions outside [0, 1]"));
            }
            o.extend_from_slice(origin);
            o.extend_from_slice(x);
            o.extend_from_slice(y);
            o.extend([*rx, *ry, *start, *end]);
            o.extend_from_slice(dom);
            o.push(*tol);
        }
    }
    Ok(())
}

/// Builds the cone `s` and runs `op` on it; returns the shim's record (layout in the
/// module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn cone_op(s: &Build, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_build(&mut input, s)?;
    push_op(&mut input, op)?;
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: finite values and, for the trim,
        // independent ellipse axes); buf is valid for buf.len() writes and the shim
        // honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_cone_p2_eval(
                op.code(),
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}
