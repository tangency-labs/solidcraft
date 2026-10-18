//! FFI wrappers for unit `surface.surf_of_revolution_p3`.
//!
//! Shim: `harness/cpp/shim/surface_surf_of_revolution_p3.cpp`
//! (`sc_surface_surf_of_revolution_p3_eval`).
//! Upstream: `source/SMLib/src/SmSurfOfRevolution.cpp`.
//!
//! [`surface_op`] builds a surface of revolution from a [`Surf`] (the surface block of
//! unit `surface.surf_of_revolution_p1`) and runs one [`Op`] on it; [`write_db`] returns
//! the bytes upstream `WriteToDB` writes for a surface (a generator helper) and
//! [`read_db`] reads them back with `SmSurfOfRevolution::ReadFromDB`. Records:
//!
//! * surface ops: the construction status (`0` = `SM_SUCCESS`, else the `SmStatus`),
//!   then on success the op's status and, on success, its outputs. A state is the
//!   layout of the `p1` FFI module. `Copy`: `state eq`; `Reparam`, `Rebuild`,
//!   `Transform`, `Update`: `state`; `ToNurbs`: `u v`; `Drop`: `n` then per solution
//!   `type nobjects nvariables names_this value p0 p1`; `Intersect`: the other surface's
//!   construction status, then (on success) the status and `more ncurves curves ntypes
//!   types ndevs devs nuv1 nuv2 nnull1 nnull2`; `Reverse`, `Swap`: `state`. `Rebuild` writes `SC_ERR_ARG` as its
//!   status when the NURB edit fails.
//! * `write_db`: the status, then the bytes. `read_db`: the status, then the state.
//!
//! The wrappers accept only what upstream handles without undefined behaviour: the
//! surface blocks of the `p1` module, finite parameters, ordered domains (with positive
//! extents for `Reparameterize`: a zero-width reparameterisation divides by zero in
//! NLib; the drop, intersection and update domains may be zero-width), the documented
//! drop operations (`SM_SO_MINIMIZE`, `SM_SO_MAXIMIZE`, `SM_SO_NORMALIZE`,
//! `SM_SO_INTERSECT`; the others read option vectors the drop passes as `NULL`) and
//! requests (`SM_SR_SINGLE`, `SM_SR_ALL`), non-negative tolerances, cones with
//! non-negative radii (not both zero) and a positive height, spheres with a positive
//! radius, independent frame axes, finite scales (zero, tiny and negative ones included), and
//! for `read_db` only the exact bytes [`write_db`] gives for the same surface (an
//! ASCII reader at the end of its stream leaves counts uninitialised).

use super::surface_surf_of_revolution_p1::{push_surf, Surf};
use crate::validate::bad;
use crate::{call_with_buffer, FfiError};

mod sys {
    extern "C" {
        pub fn sc_surface_surf_of_revolution_p3_eval(
            op: i32,
            input: *const f64,
            n_in: i64,
            out: *mut f64,
            cap: i64,
            out_len: *mut i64,
        ) -> i32;
    }
}

/// A NURB domain argument: the surface's natural domain, or explicit bounds.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Dom {
    /// `GetNaturalUVDomain()` of the surface.
    Natural,
    /// `umin vmin umax vmax`.
    Explicit([f64; 4]),
}

/// A placement `SmAxis2Placement(origin, x, y)` with an optional scale.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Frame {
    /// Origin.
    pub origin: [f64; 3],
    /// X axis.
    pub x: [f64; 3],
    /// Y axis.
    pub y: [f64; 3],
    /// Optional scale.
    pub scale: Option<[f64; 3]>,
}

/// The other operand of [`Op::Intersect`].
#[derive(Debug, Clone, PartialEq)]
pub enum Other {
    /// `IntersectWithSurfOfRevolution` with another surface (or the same one).
    Revolution(Option<Box<Surf>>),
    /// `IntersectWithCone` with `SmCone::CreateCanonical(A2P(o, x, y), r0, r1, h)`.
    Cone {
        /// Placement origin, X and Y axes.
        frame: [[f64; 3]; 3],
        /// Bottom radius.
        r0: f64,
        /// Top radius.
        r1: f64,
        /// Height.
        h: f64,
    },
    /// `IntersectWithSphere` with `SmSphere::CreateCanonical(A2P(o, x, y), r)`.
    Sphere {
        /// Placement origin, X and Y axes.
        frame: [[f64; 3]; 3],
        /// Radius.
        r: f64,
    },
}

/// The upstream member run on the surface.
#[derive(Debug, Clone, PartialEq)]
pub enum Op {
    /// `Copy`, then the copy's state and `*copy == *this`.
    Copy,
    /// `Reparameterize(domain)`.
    Reparam {
        /// The new NURB domain.
        dom: [f64; 4],
    },
    /// `ConvertUVFromSTEPToNURBS`.
    ToNurbs {
        /// Angle in degrees.
        u: f64,
        /// Generator parameter.
        v: f64,
    },
    /// `DropPointFast`.
    Drop {
        /// NURB domain.
        dom: Dom,
        /// `SmSolverOperationType` code.
        operation: i32,
        /// Test point.
        point: [f64; 3],
        /// Optional in-pointing vector.
        vec: Option<[f64; 3]>,
        /// Distance tolerance.
        tol: f64,
        /// Optional target distance.
        target: Option<f64>,
        /// `SmSolutionRequestedType` code.
        requested: i32,
    },
    /// `IntersectWithSurfOfRevolution` / `IntersectWithCone` / `IntersectWithSphere`.
    Intersect {
        /// The other surface.
        other: Other,
        /// This surface's NURB domain.
        dom: Dom,
        /// The other surface's NURB domain.
        other_dom: Dom,
        /// Optional approximation tolerance.
        tol: Option<f64>,
    },
    /// `RebuildSTEPFromNURBParameters`, after an optional
    /// `SmBSplineSurface::Transform` of the NURB alone.
    Rebuild {
        /// The NURB edit.
        edit: Option<Frame>,
    },
    /// `Transform(frame, scale)`.
    Transform {
        /// Placement and scale.
        frame: Frame,
    },
    /// `UpdateAnalyticalDomain(domain)`.
    Update {
        /// The new NURB domain.
        dom: Dom,
    },
    /// The inherited `SmBSplineSurface::Reverse(param)`.
    Reverse {
        /// `0` = `SM_SP_U`, `1` = `SM_SP_V`.
        param: u8,
    },
    /// The inherited `SmBSplineSurface::SwapUV()`.
    Swap,
}

impl Op {
    const fn code(&self) -> i32 {
        match self {
            Self::Copy => 0,
            Self::Reparam { .. } => 1,
            Self::ToNurbs { .. } => 2,
            Self::Drop { .. } => 3,
            Self::Intersect { .. } => 4,
            Self::Rebuild { .. } => 5,
            Self::Transform { .. } => 6,
            Self::Update { .. } => 7,
            Self::Reverse { .. } => 8,
            Self::Swap => 9,
        }
    }
}

const OP_WRITE_DB: i32 = 20;
const OP_READ_DB: i32 = 21;

/// Largest byte count of a database record.
pub const MAX_DB_BYTES: usize = 1 << 22;

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

fn bounds(d: [f64; 4], what: &str) -> Result<(), FfiError> {
    finite(&d, what)?;
    let [a, b, c, e] = d;
    if !(a < c && b < e) {
        return Err(bad(format!(
            "{what}: domain not ordered with positive extents"
        )));
    }
    Ok(())
}

fn push_dom(o: &mut Vec<f64>, d: Dom, what: &str) -> Result<(), FfiError> {
    match d {
        Dom::Natural => o.extend([1.0, 0.0, 0.0, 1.0, 1.0]),
        Dom::Explicit(b) => {
            // zero-width drop, intersection and update domains are valid upstream
            finite(&b, what)?;
            let [a, v0, c, v1] = b;
            if !(a <= c && v0 <= v1) {
                return Err(bad(format!("{what}: domain not ordered")));
            }
            o.push(0.0);
            o.extend_from_slice(&b);
        }
    }
    Ok(())
}

fn push_frame(o: &mut Vec<f64>, f: &Frame, what: &str) -> Result<(), FfiError> {
    finite(&f.origin, what)?;
    axes(f.x, f.y, what)?;
    o.extend_from_slice(&f.origin);
    o.extend_from_slice(&f.x);
    o.extend_from_slice(&f.y);
    match f.scale {
        Some(s) => {
            // any finite scale is valid upstream: `Transform` rejects a non-uniform
            // scale or one below SM_EFF_ZERO itself, and the B-spline transform of
            // the rebuild edit accepts zero and negative components
            finite(&s, what)?;
            o.push(1.0);
            o.extend_from_slice(&s);
        }
        None => o.extend([0.0, 1.0, 1.0, 1.0]),
    }
    Ok(())
}

fn tolerance(t: f64, what: &str) -> Result<(), FfiError> {
    if !(t.is_finite() && t >= 0.0) {
        return Err(bad(format!("{what}: tolerance must not be negative")));
    }
    Ok(())
}

fn push_op(o: &mut Vec<f64>, op: &Op) -> Result<(), FfiError> {
    match op {
        Op::Copy | Op::Swap => {}
        Op::Reparam { dom } => {
            bounds(*dom, "reparam")?;
            o.extend_from_slice(dom);
        }
        Op::ToNurbs { u, v } => {
            finite(&[*u, *v], "to_nurbs")?;
            o.extend([*u, *v]);
        }
        Op::Drop {
            dom,
            operation,
            point,
            vec,
            tol,
            target,
            requested,
        } => {
            push_dom(o, *dom, "drop domain")?;
            if !matches!(operation, 0 | 3 | 4 | 5) {
                return Err(bad("drop: operation"));
            }
            finite(point, "drop point")?;
            tolerance(*tol, "drop")?;
            if !matches!(requested, 0 | 1) {
                return Err(bad("drop: requested"));
            }
            o.push(f64::from(*operation));
            o.extend_from_slice(point);
            let v = vec.unwrap_or([0.0; 3]);
            finite(&v, "drop vector")?;
            o.push(f64::from(u8::from(vec.is_some())));
            o.extend_from_slice(&v);
            o.push(*tol);
            let t = target.unwrap_or(0.0);
            finite(&[t], "drop target")?;
            o.extend([f64::from(u8::from(target.is_some())), t]);
            o.push(f64::from(*requested));
        }
        Op::Intersect {
            other,
            dom,
            other_dom,
            tol,
        } => {
            match other {
                Other::Revolution(None) => o.extend([0.0, 1.0]),
                Other::Revolution(Some(s)) => {
                    o.extend([0.0, 0.0]);
                    push_surf(o, s)?;
                }
                Other::Cone { frame, r0, r1, h } => {
                    let [fo, fx, fy] = *frame;
                    finite(&fo, "cone")?;
                    axes(fx, fy, "cone")?;
                    finite(&[*r0, *r1, *h], "cone")?;
                    if *r0 < 0.0 || *r1 < 0.0 || (*r0 == 0.0 && *r1 == 0.0) || *h <= 0.0 {
                        return Err(bad("cone: radii and height"));
                    }
                    o.push(1.0);
                    o.extend_from_slice(&fo);
                    o.extend_from_slice(&fx);
                    o.extend_from_slice(&fy);
                    o.extend([*r0, *r1, *h]);
                }
                Other::Sphere { frame, r } => {
                    let [fo, fx, fy] = *frame;
                    finite(&fo, "sphere")?;
                    axes(fx, fy, "sphere")?;
                    if !(r.is_finite() && *r > 0.0) {
                        return Err(bad("sphere: radius"));
                    }
                    o.push(2.0);
                    o.extend_from_slice(&fo);
                    o.extend_from_slice(&fx);
                    o.extend_from_slice(&fy);
                    o.extend([*r, 0.0, 0.0]);
                }
            }
            push_dom(o, *dom, "intersect domain")?;
            push_dom(o, *other_dom, "intersect other domain")?;
            if let Some(t) = tol {
                tolerance(*t, "intersect")?;
            }
            o.extend([f64::from(u8::from(tol.is_some())), tol.unwrap_or(0.0)]);
        }
        Op::Rebuild { edit } => {
            if let Some(f) = edit {
                o.push(1.0);
                push_frame(o, f, "rebuild edit")?;
            } else {
                o.push(0.0);
                o.extend([0.0; 13]);
            }
        }
        Op::Transform { frame } => push_frame(o, frame, "transform")?,
        Op::Update { dom } => push_dom(o, *dom, "update domain")?,
        Op::Reverse { param } => {
            if *param > 1 {
                return Err(bad("reverse: param"));
            }
            o.push(f64::from(*param));
        }
    }
    Ok(())
}

fn call(op: i32, input: &[f64]) -> Result<Vec<f64>, FfiError> {
    call_with_buffer(4096, |buf, len| {
        // SAFETY: input is valid for input.len() reads and holds exactly what the shim
        // reads for `op` (built and validated above: well-formed surface blocks, bounded
        // counts, finite values, ordered domains, non-degenerate axes, non-negative
        // tolerances, documented operation codes); buf is valid for buf.len() writes and
        // the shim honours the capacity; len is valid.
        unsafe {
            sys::sc_surface_surf_of_revolution_p3_eval(
                op,
                input.as_ptr(),
                input.len() as i64,
                buf.as_mut_ptr(),
                buf.len() as i64,
                &raw mut *len,
            )
        }
    })
}

/// Builds the surface `s` and runs `op` on it; returns the shim's record (layout in
/// the module docs).
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn surface_op(s: &Surf, op: &Op) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_surf(&mut input, s)?;
    push_op(&mut input, op)?;
    call(op.code(), &input)
}

/// The record upstream `WriteToDB(db, version)` writes for the surface `s` (`binary`
/// or ASCII with 17 digits): the construction status, then (on success) the write
/// status and the bytes.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] for input the shim must not see; [`FfiError::Status`]
/// for a shim failure.
pub fn write_db(s: &Surf, binary: bool, version: u32) -> Result<Vec<f64>, FfiError> {
    let mut input = Vec::new();
    push_surf(&mut input, s)?;
    input.extend([f64::from(u8::from(binary)), f64::from(version)]);
    call(OP_WRITE_DB, &input)
}

/// The bytes of a successful [`write_db`], if both statuses are `0`.
///
/// # Errors
///
/// Those of [`write_db`]; [`FfiError::InvalidArgument`] for a byte value out of range.
pub fn write_db_bytes(s: &Surf, binary: bool, version: u32) -> Result<Option<Vec<u8>>, FfiError> {
    let rec = write_db(s, binary, version)?;
    match rec.as_slice() {
        [b, w, bytes @ ..] if *b == 0.0 && *w == 0.0 => bytes
            .iter()
            .map(|&x| {
                if (0.0..=255.0).contains(&x) && x.fract() == 0.0 {
                    Ok(Some(x as u8))
                } else {
                    Err(bad("write db: byte out of range"))
                }
            })
            .collect::<Result<Option<Vec<u8>>, FfiError>>(),
        _ => Ok(None),
    }
}

/// Reads `bytes` with `SmSurfOfRevolution::ReadFromDB` (`binary` or ASCII, `version`);
/// returns the shim's record: the status, then the state on success.
///
/// # Errors
///
/// [`FfiError::InvalidArgument`] unless `bytes` are exactly what [`write_db`] writes
/// for `s` with the same file type and version; [`FfiError::Status`] for a shim
/// failure.
pub fn read_db(s: &Surf, binary: bool, version: u32, bytes: &[u8]) -> Result<Vec<f64>, FfiError> {
    if bytes.len() > MAX_DB_BYTES {
        return Err(bad("read db: record too large"));
    }
    let written = write_db_bytes(s, binary, version)?;
    if written.as_deref() != Some(bytes) {
        return Err(bad("read db: bytes are not the surface's WriteToDB record"));
    }
    let mut input = vec![
        f64::from(u8::from(binary)),
        f64::from(version),
        bytes.len() as f64,
    ];
    input.extend(bytes.iter().map(|&b| f64::from(b)));
    call(OP_READ_DB, &input)
}
