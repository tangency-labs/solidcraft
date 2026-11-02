//! The B-rep kernel, solidcore (tangency-labs/solidcore, the Rust port of NVIDIA's SMLib,
//! Apache-2.0), behind a safe wrapper. Only `crates/kernel` uses this crate (ADR 0002).
//!
//! Every call runs in a solidcore kernel of its own, never changes its inputs, turns solidcore's
//! errors into [`Error`] and catches its panics ([`Error::Internal`]).
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]

use std::path::Path;

mod check;
mod core;
pub mod export;
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
pub use measure::{MEASURE_ACCURACY, MassProperties, PointClass};
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
    /// A blend larger than its faces allow, refused with the largest size that builds it
    /// (solidcore's fit, `SolidError::FilletRefused`): the size, what it sizes, and a point at the
    /// middle of the edge it was found for, when the refusal names one.
    TooLarge { size: f64, kind: FitKind, at: Option<[f64; 3]> },
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
    /// solidcore found its own state inconsistent part way through a call (a hole it rebuilt did
    /// not close).
    Exception,
    /// A B-rep record SMLib cannot take (see [`import::Unsupported`]).
    Record(import::Unsupported),
    /// solidcore reported an error (the call, then its message).
    Solidcore(String),
    /// solidcore panicked; the guard caught it (the call, then the panic's message).
    Internal(String),
    /// A step of SMLib solidcore has not ported yet, refused rather than built another way: what
    /// is missing, and the solidcore issue that ports it.
    Unported(&'static str),
}

/// What the largest size that builds a refused blend ([`Error::TooLarge`]) sizes.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum FitKind {
    /// A round's radius.
    Radius,
    /// An equal chamfer's distance.
    Distance,
    /// A factor on a chamfer's two distances.
    Scale,
    /// A chord round's width.
    Chord,
}

impl FitKind {
    /// The size's name, as a fillet's message names it ("the largest radius that fits").
    pub fn word(self) -> &'static str {
        match self {
            FitKind::Radius => "radius",
            FitKind::Distance => "distance",
            FitKind::Scale => "scale",
            FitKind::Chord => "chord width",
        }
    }
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
            Error::TooLarge { size, kind, at } => {
                write!(f, "the blend is larger than its faces allow")?;
                if let Some([x, y, z]) = at {
                    write!(f, " at the edge at ({x:.3}, {y:.3}, {z:.3})")?;
                }
                write!(f, "; the largest {} that fits is {size}", kind.word())
            }
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
            Error::Record(u) => write!(f, "{u}"),
            Error::Solidcore(e) => write!(f, "solidcore: {e}"),
            Error::Internal(e) => write!(f, "solidcore failed internally in {e}"),
            Error::Unported(what) => write!(f, "not supported yet: {what}"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Where failing calls leave their inputs for replay ([`core::dump`]); `None` turns that off.
pub fn set_dump_dir(dir: Option<&Path>) {
    core::dump::set_dir(dir.map(Path::to_path_buf));
}

/// The kernel call running right now, if any: for crash reports. Takes no lock.
pub fn current_call() -> Option<String> {
    core::dump::current()
}

/// A B-rep. Immutable: operations return new bodies.
pub struct Brep {
    body: core::Body,
}

impl std::fmt::Debug for Brep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("smlib::Brep")
    }
}

impl Brep {
    /// A body solidcore made.
    fn made_core(body: core::Body) -> Brep {
        Brep { body }
    }

    /// solidcore's body.
    fn core(&self) -> &core::Body {
        &self.body
    }

    /// An axis-aligned box from its lower corner, with positive edge lengths (mm).
    pub fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Brep> {
        core::make_box(origin, size).map(Brep::made_core)
    }

    /// Reads a native SMLib `.smb` file.
    pub fn read(path: &Path, ascii: bool) -> Result<Brep> {
        core::file::read(path, ascii).map(Brep::made_core)
    }

    /// Writes this body to a native SMLib `.smb` file.
    pub fn write(&self, path: &Path, ascii: bool) -> Result<()> {
        core::file::write(self.core(), path, ascii)
    }

    /// A deep copy.
    pub fn try_clone(&self) -> Result<Brep> {
        core::transform::copy(self.core()).map(Brep::made_core)
    }

    /// This body moved by the affine `matrix` (column-major: columns 0–2 the images of x, y and
    /// z, column 3 the translation), as SMLib's `SmBrep::Transform` takes a matrix: reflections
    /// mirror it, a non-uniform scale or a shear turns its geometry into NURBS, tolerances scale
    /// with it.
    pub fn transformed(&self, matrix: &[[f64; 4]; 4]) -> Result<Brep> {
        core::transform::transform(self.core(), matrix).map(Brep::made_core)
    }

    /// The enclosed volume (mm³) at `relative_accuracy` (SMLib clamps it to [1e-8, 1e-1]).
    pub fn volume(&self, relative_accuracy: f64) -> Result<f64> {
        core::volume(self.core(), relative_accuracy)
    }

    /// Face, edge and vertex counts.
    pub fn counts(&self) -> Result<[usize; 3]> {
        core::counts(self.core())
    }
}

#[cfg(test)]
mod tests;
