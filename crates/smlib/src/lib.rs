//! NVIDIA's SMLib B-rep kernel (tangency-labs/smlib, Apache-2.0), statically linked behind its C
//! API, with a safe Rust wrapper. Only `crates/kernel` uses this crate (ADR 0002).
//!
//! The C API never lets a C++ exception, type or pointer out, never changes its inputs, and
//! serialises every call behind one lock (SMLib keeps process-global state). This wrapper owns
//! the handles, turns status codes into [`Error`], and sends SMLib's one-line-per-call log to the
//! `log` crate (target `smlib`).
//!
//! Built without the `native` feature, the crate needs no C++ toolchain and every call returns
//! [`Error::NotBuilt`].
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

#[cfg(all(feature = "native", feature = "solidcore"))]
compile_error!("features `native` and `solidcore` are exclusive: pick one backend");

use std::ffi::CString;
use std::path::Path;
use std::sync::Once;
use std::sync::atomic::{AtomicI32, Ordering};

mod check;
pub mod export;
mod ffi;
pub mod flat;
pub mod history;
pub mod import;
mod measure;
mod ops;
pub mod pcurves;
mod sheets;
mod split;
pub mod ssi;
pub mod sweeps;
pub mod trim;

pub use check::{CheckLevel, CheckReport};
pub use measure::{MEASURE_ACCURACY, MassProperties};
pub use ops::{Blend, BooleanOp, ChamferSecond, Hole, HoleEdge, MAX_POINTS, MAX_UNION_BREPS};

/// Why an SMLib call failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// SMLib's own status (1001 `SM_ERR`, 1007 `SM_ERR_INVALID_INPUT`, …).
    Status(i32),
    /// An argument the C API rejected before SMLib saw it.
    Argument,
    /// SMLib reported success but returned no body (for a boolean: the result is empty).
    Empty,
    /// SMLib reported success, but a solid input gave a result that is not a closed solid.
    NotSolid,
    /// Selection point `point` is farther than the tolerance from every edge (face).
    NoMatch { point: usize },
    /// Selection point `point` is equally near to two edges (faces).
    Ambiguous { point: usize },
    /// No room for the operation: an inward offset (a shell's wall, faces moved in) leaves none
    /// inside the body, or a blend is larger than the faces it rolls on allow.
    NoRoom,
    /// An offset whose walls would meet (a shell's): the offsets of two faces touch without
    /// crossing, a hole's wall offset just reaching a side's, so the result would pinch to
    /// nothing along a line. A thinner or a thicker wall builds.
    WallsTouch,
    /// SMLib cannot build this exactly yet (a variable radius whose law has a corner).
    Unsupported,
    /// A blend's edges meet at a vertex of a kind no SMLib blend corner handles yet.
    CornerUnsupported,
    /// SMLib does not set back the corner of corner point `corner`: SMLib sets back a corner
    /// only where every edge meeting it is blended and they are all convex or all concave.
    CornerRefused { corner: usize },
    /// SMLib reported success but the operation did nothing: a blend that made no face, a face
    /// move or draft that moved no face.
    NoEffect,
    /// The answer is not exact in the input's geometry (a box that would wrap round its
    /// surface, an analytic surface whose speed across a side varies along it).
    NotExact,
    /// A drafted extrusion could not close the corner (or wall) at `at` on its profile, for `why`.
    Corner { why: sweeps::CornerRefusal, at: [f64; 3] },
    /// A C++ exception (most likely out of memory) was caught at the boundary.
    Exception,
    /// The C API is a different version from the one this crate was written against.
    Version(i32),
    /// This build has no SMLib (the `native` feature is off).
    NotBuilt,
    /// A B-rep record SMLib cannot take (see [`import::Unsupported`]).
    Record(import::Unsupported),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Status(1007) => write!(f, "SMLib rejected the input (SM_ERR_INVALID_INPUT)"),
            Error::Status(s) => write!(f, "SMLib failed (status {s})"),
            Error::Argument => write!(f, "invalid argument for SMLib"),
            Error::Empty => write!(f, "SMLib returned an empty body"),
            Error::NotSolid => write!(f, "SMLib's result is not a closed solid"),
            Error::NoMatch { point } => write!(f, "selection point {} is not near any edge or face", point + 1),
            Error::Ambiguous { point } => write!(f, "selection point {} is equally near to two edges or faces", point + 1),
            Error::NoRoom => write!(f, "there is no room for it: an offset leaves none inside the body, or a blend is larger than its faces allow"),
            Error::WallsTouch => write!(f, "the offset's walls would meet along a line, which leaves no solid there"),
            Error::Unsupported => write!(f, "not supported yet: SMLib cannot build this exactly"),
            Error::CornerUnsupported => write!(f, "not supported yet: a corner of this kind is not supported yet by SMLib's blends"),
            Error::CornerRefused { corner } => write!(
                f,
                "not supported yet: SMLib does not set back corner {}: it sets back a corner only where every edge meeting it is blended, all convex or all concave",
                corner + 1
            ),
            Error::NoEffect => write!(f, "SMLib made no change: it reported success, but the operation did nothing"),
            Error::NotExact => write!(f, "SMLib cannot build this exactly in the input's geometry"),
            Error::Corner { why, at } => {
                let at = format!("({:.4}, {:.4}, {:.4})", at[0], at[1], at[2]);
                match why {
                    sweeps::CornerRefusal::Apart => write!(f, "the draft's continued curves do not meet at the corner at {at}"),
                    sweeps::CornerRefusal::WallCloses => write!(f, "the draft closes the wall at {at} before the far end"),
                    sweeps::CornerRefusal::NotExact => {
                        write!(f, "the draft cannot continue the walls exactly: the curve from {at} is neither a line nor an arc")
                    }
                }
            }
            Error::Exception => write!(f, "SMLib raised an internal exception"),
            Error::Version(v) => write!(f, "SMLib C API version {v}, expected {}", ffi::ABI_VERSION),
            Error::NotBuilt => write!(f, "not supported yet: this build has no SMLib"),
            Error::Record(u) => write!(f, "{u}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

fn error(s: i32) -> Error {
    match s {
        ffi::ERR_ARG | ffi::ERR_BUFFER => Error::Argument,
        ffi::ERR_EMPTY => Error::Empty,
        ffi::ops::ERR_NOT_SOLID => Error::NotSolid,
        ffi::ops::ERR_NO_ROOM => Error::NoRoom,
        ffi::ops::ERR_WALLS_TOUCH => Error::WallsTouch,
        ffi::ops::ERR_UNSUPPORTED => Error::Unsupported,
        ffi::ops::ERR_CORNER_UNSUPPORTED => Error::CornerUnsupported,
        ffi::ops::ERR_NO_EFFECT => Error::NoEffect,
        ffi::trim::ERR_NOT_EXACT => Error::NotExact,
        ffi::ERR_EXCEPTION => Error::Exception,
        ffi::NOT_BUILT => Error::NotBuilt,
        s => Error::Status(s),
    }
}

fn check(s: i32) -> Result<()> {
    if s == ffi::OK { Ok(()) } else { Err(error(s)) }
}

fn c_path(p: &Path) -> Result<CString> {
    CString::new(p.to_string_lossy().as_bytes()).map_err(|_| Error::Argument)
}

/// One C API call's log line, `smlib_fn(args) -> status (1.234 ms)`: the function and the time
/// the C API measured for it (inside its lock, so waiting for another thread's call is not in it).
fn call_time(line: &str) -> Option<(&str, std::time::Duration)> {
    let name = line.get(..line.find('(')?)?;
    if !name.starts_with("smlib_") || !name.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_') {
        return None;
    }
    let tail = line.strip_suffix(" ms)")?;
    let ms: f64 = tail.get(tail.rfind(" (")? + 2..)?.parse().ok()?;
    (ms.is_finite() && ms >= 0.0).then(|| (name, std::time::Duration::from_secs_f64(ms / 1000.0)))
}

fn log_line(level: i32, text: String) {
    // Every call's log line carries the C API's own timing: the profiler's SMLib spans.
    if solidcraft_profile::enabled()
        && let Some((name, took)) = call_time(&text)
    {
        solidcraft_profile::record("smlib", solidcraft_profile::intern(name), took);
    }
    if level == 0 {
        log::debug!(target: "smlib", "{text}");
    } else {
        log::warn!(target: "smlib", "{text}");
    }
}

static INIT: Once = Once::new();
/// What [`init`] found, kept for every later call (0: ready).
static INIT_RESULT: AtomicI32 = AtomicI32::new(0);

/// Checks the C API's version, creates SMLib's context and routes its log to the `log` crate.
/// Every other call runs it first; calling it at start-up keeps the cost out of the first operation.
pub fn init() -> Result<()> {
    INIT.call_once(|| {
        let v = ffi::abi_version();
        let result = if v == ffi::NOT_BUILT {
            v
        } else if v != ffi::ABI_VERSION {
            v.saturating_add(10_000)
        } else {
            ffi::set_log(Some(log_line));
            ffi::init()
        };
        INIT_RESULT.store(result, Ordering::Release);
    });
    match INIT_RESULT.load(Ordering::Acquire) {
        ffi::OK => Ok(()),
        v if v >= 10_000 => Err(Error::Version(v - 10_000)),
        s => Err(error(s)),
    }
}

/// Where failing calls leave their inputs for replay (see the C API's `smlib_set_dump_dir`);
/// `None` turns that off.
pub fn set_dump_dir(dir: Option<&Path>) -> Result<()> {
    init()?;
    let c = dir.map(c_path).transpose()?;
    check(ffi::set_dump_dir(c.as_deref()))
}

/// The SMLib call running right now, if any: for crash reports. Takes no lock.
pub fn current_call() -> Option<String> {
    let mut buf = [0u8; 512];
    let n = ffi::current_call(&mut buf);
    let s = buf.get(..n).map(|b| String::from_utf8_lossy(b).into_owned()).unwrap_or_default();
    (!s.is_empty()).then_some(s)
}

/// An SMLib B-rep. Immutable: operations return new bodies.
pub struct Brep(ffi::Handle);

impl std::fmt::Debug for Brep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("smlib::Brep")
    }
}

impl Brep {
    /// An axis-aligned box from its lower corner, with positive edge lengths (mm).
    pub fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Brep> {
        init()?;
        ffi::make_box(&origin, &size).map(Brep).map_err(error)
    }

    /// Reads a native SMLib `.smb` file.
    pub fn read(path: &Path, ascii: bool) -> Result<Brep> {
        init()?;
        ffi::read(&c_path(path)?, ascii).map(Brep).map_err(error)
    }

    /// Writes this body to a native SMLib `.smb` file.
    pub fn write(&self, path: &Path, ascii: bool) -> Result<()> {
        check(ffi::write(&self.0, &c_path(path)?, ascii))
    }

    /// A deep copy.
    pub fn try_clone(&self) -> Result<Brep> {
        ffi::copy(&self.0).map(Brep).map_err(error)
    }

    /// This body moved by the affine `matrix` (column-major: columns 0–2 the images of x, y and
    /// z, column 3 the translation), as SMLib's `SmBrep::Transform` takes a matrix: reflections
    /// mirror it, a non-uniform scale or a shear turns its geometry into NURBS, tolerances scale
    /// with it.
    pub fn transformed(&self, matrix: &[[f64; 4]; 4]) -> Result<Brep> {
        ffi::transform(&self.0, matrix).map(Brep).map_err(error)
    }

    /// The enclosed volume (mm³) at `relative_accuracy` (SMLib clamps it to [1e-8, 1e-1]).
    pub fn volume(&self, relative_accuracy: f64) -> Result<f64> {
        ffi::volume(&self.0, relative_accuracy).map_err(error)
    }

    /// Face, edge and vertex counts.
    pub fn counts(&self) -> Result<[usize; 3]> {
        let c = ffi::counts(&self.0).map_err(error)?;
        Ok(c.map(|n| usize::try_from(n).unwrap_or(0)))
    }
}

#[cfg(test)]
mod tests;
