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
pub mod failure;
pub mod flat;
pub mod history;
pub mod import;
mod measure;
mod ops;
pub mod pcurves;
mod sheets;
mod split;
pub mod ssi;
pub mod stop;
pub mod sweeps;
pub mod trace;
pub mod trim;

pub use check::CheckLevel;
pub use check::CheckReport;
pub use measure::MEASURE_ACCURACY;
pub use measure::MassProperties;
pub use measure::PointClass;
pub use ops::Blend;
pub use ops::BooleanOp;
pub use ops::ChamferSecond;
pub use ops::Hole;
pub use ops::HoleEdge;
pub use ops::MAX_POINTS;
pub use ops::MAX_UNION_BREPS;
pub use ops::Turn;

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
    /// A shell or offset that failed on a body with a face whose outline touches itself at
    /// `at`: a corner of one of its loops on another loop, within the two's tolerances (a pad
    /// or hole whose corner reaches the face's edge there). The walls offset from the two
    /// would meet at that point, which the offset does not build. Moved clear of the edge,
    /// or across it, the feature shells. `detail` is the kernel's own failure, kept whole.
    OutlineTouches { at: [f64; 3], detail: String },
    /// A blend larger than its faces allow, refused with the largest size that builds it
    /// (solidcore's fit, `SolidError::FilletRefused`): the size, what it sizes, a point at the
    /// middle of the edge it was found for, a point on the limiting face, and why there is no room,
    /// when the refusal names them.
    TooLarge { size: f64, kind: FitKind, at: Option<[f64; 3]>, face: Option<[f64; 3]>, why: Option<RoomReason> },
    /// A corner's setback too short for its fill: solidcore's `BlendRefusal::SetbackTooShort`
    /// from the fill's test, the corner's fill not certified at the setback asked. `least` is
    /// the least setback solidcore's fill takes, `at` the corner's point when solidcore gives
    /// it. `builds`: solidcore's fill is the corner, so a setback of `least` builds it; where
    /// another patch replaces it (a G2 corner), `least` is only what solidcore's corner needs,
    /// not a setback shown to build.
    SetbackTooShort { least: f64, at: Option<[f64; 3]>, builds: bool },
    /// A shell, offset or thicken whose wall is thicker than a face curves: the face's offset
    /// folds (solidcore's `OffsetFailure::Folds`). The largest wall that works,
    /// when solidcore names one.
    WallFolds { largest: Option<f64> },
    /// SMLib cannot build this operation exactly.
    Unsupported,
    /// A draft refused by the kernel whose drafted face would cut through a face it does not
    /// meet (solidcore's `drafted_through`): a hole's wall within the draft of the side. `at` is a
    /// point of that face between the face's plane and its drafted plane, `depth` how far it
    /// lies past the drafted plane.
    DraftCutsThrough { at: [f64; 3], depth: f64 },
    /// A face move whose tangent blends cannot follow the moved faces, with solidcore's reason
    /// (`SmOffsetFacesExecutive`'s refusal comment): `exact` false where the blend cannot be taken
    /// off and put back ([`Error::Unsupported`] without a reason), true where following it needs a
    /// surface or edge continued that cannot be continued exactly ([`Error::NotExact`] without
    /// one).
    BlendsNotFollowed { exact: bool, why: String },
    /// A blend's edges meet at a vertex of a kind no SMLib blend corner handles yet: solidcore's
    /// reason and the vertex's point, where it notes them.
    CornerUnsupported { why: Option<String>, at: Option<[f64; 3]> },
    /// A swept turn ([`Turn::Swept`]) solidcore does not build at the corner `at`: `room` where
    /// no radius builds it (a sharp concave side edge between the rounds, solidcore's `NoRoom`),
    /// else not supported there (`CornerUnsupported`: unequal or variable rounds, a corner of
    /// four edges, a curved, drafted or tangent side edge); `why` is solidcore's reason.
    TurnRefused { room: bool, why: String, at: Option<[f64; 3]> },
    /// SMLib does not set back the corner of corner point `corner`: SMLib sets back a corner
    /// only where every edge meeting it is blended and they are all convex or all concave.
    CornerRefused { corner: usize },
    /// SMLib reported success but the operation did nothing: a blend that made no face, a face
    /// move or draft that moved no face.
    NoEffect,
    /// A split tool runs into a face and ends inside it, leaving a crack, and no tool cuts any
    /// face through: the tool, taken over its whole surface, does not reach across.
    ToolStopsShort,
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
    /// A refusal solidcore-ops types, carried as it is: the app words it from its fields
    /// (an offset refused with its reason, [`solidcore_ops::Error::OffsetRefused`]; a fillet or
    /// chamfer failed with the kernel's statement of why, [`solidcore_ops::Error::BlendFailed`]).
    Refused(solidcore_ops::Error),
    /// SMLib rejected its input and solidcore said why (`SM_ERR_INVALID_INPUT` with its reason:
    /// `SolidError::InvalidInput`, or solidcore-ops' `Refused`): the routine that rejected it and
    /// what was wrong, rather than the bare status.
    Rejected { operation: &'static str, reason: &'static str },
    /// A typed refusal together with the kernel's complete diagnostic, including its
    /// operation, entities and nested causes. The refusal remains available as data.
    RefusedWithDiagnostic { refusal: solidcore_ops::Error, diagnostic: String },
    /// solidcore panicked; the guard caught it (the call, then the panic's message).
    Internal(String),
    /// A classification that needs one nearest point on a face's surface found `found` of them
    /// (the kernel's `compute_point_parameters`, its `IntersectFailure::NotOneFoot`): the
    /// point's place on the face is undecided, the operation refuses. Status `SM_ERR`.
    NotOneFoot {
        /// How many nearest points the wider solve found.
        found: usize,
    },
    /// A step of SMLib solidcore has not ported yet, refused rather than built another way: what
    /// is missing, and the solidcore issue that ports it.
    Unported(&'static str),
    /// The work's token was stopped ([`stop::Stop::stop`]) before or during the call: no result,
    /// even one solidcore finished first. Never a failure of the operation on its input.
    Cancelled(&'static str),
}

/// Why a blend has no room, and what the largest size that builds a refused blend sizes
/// ([`Error::TooLarge`]): ops' own, as its refusal gives them.
pub use solidcore_ops::FitKind;
pub use solidcore_ops::RoomReason;

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Status(1007) => write!(f, "SMLib rejected the input (SM_ERR_INVALID_INPUT)"),
            Error::Status(s) => write!(f, "SMLib failed (status {s})"),
            Error::Argument => write!(f, "invalid argument for SMLib"),
            Error::Empty => write!(f, "SMLib returned an empty body"),
            Error::NotSolid => write!(f, "SMLib's result is not a closed solid"),
            Error::NotOneFoot { found } => write!(f, "a classification found {found} nearest points on the face's surface, where one is needed"),
            Error::NoMatch { point } => write!(f, "selection point {} is not near any edge or face", point + 1),
            Error::Ambiguous { point } => write!(f, "selection point {} is equally near to two edges or faces", point + 1),
            Error::NoRoom => write!(f, "there is no room for it: an offset leaves none inside the body, or a blend is larger than its faces allow"),
            Error::WallsTouch => write!(f, "the offset's walls would meet along a line, which leaves no solid there"),
            Error::OutlineTouches { at: [x, y, z], detail } => write!(
                f,
                "a face's outline touches itself at ({x:.5}, {y:.5}, {z:.5}), where a corner of one of its loops lies on another, and the offset does not build walls that meet at a point; the kernel's failure: {detail}"
            ),
            Error::TooLarge { size, kind, at, face, why } => {
                write!(f, "the blend is larger than its faces allow")?;
                if let Some([x, y, z]) = at {
                    write!(f, " at the edge at ({x:.3}, {y:.3}, {z:.3})")?;
                }
                if let Some([x, y, z]) = face {
                    write!(f, ", limited by the face containing ({x:.3}, {y:.3}, {z:.3})")?;
                }
                if let Some(why) = why {
                    write!(f, ": {}", why.words_for(*kind))?;
                }
                write!(f, "; the largest {} that fits is {size}", kind.word())
            }
            Error::SetbackTooShort { least, at, builds } => {
                write!(f, "the setback is too short for the corner's fill")?;
                if let Some([x, y, z]) = at {
                    write!(f, " at ({x:.3}, {y:.3}, {z:.3})")?;
                }
                if *builds {
                    write!(f, "; the smallest setback that builds is {least}")
                } else {
                    write!(f, ": solidcore's corner needs a setback of at least {least}")
                }
            }
            Error::WallFolds { largest } => {
                write!(f, "the wall is thicker than a face curves, so the face's offset folds")?;
                match largest {
                    Some(w) => write!(f, "; the largest wall that works is {w}"),
                    None => Ok(()),
                }
            }
            Error::Unsupported => write!(f, "not supported yet: SMLib cannot build this exactly"),
            Error::DraftCutsThrough { at: [x, y, z], depth } => write!(
                f,
                "a drafted face would cut through the face at ({x:.3}, {y:.3}, {z:.3}), which reaches {depth:.3} past it and meets no edge of it"
            ),
            Error::CornerUnsupported { why, at } => {
                write!(f, "not supported yet: a corner of this kind is not supported yet by SMLib's blends")?;
                if let Some([x, y, z]) = at {
                    write!(f, " at ({x:.3}, {y:.3}, {z:.3})")?;
                }
                if let Some(why) = why {
                    write!(f, ": {why}")?;
                }
                Ok(())
            }
            Error::TurnRefused { why, at, .. } => {
                write!(f, "the swept corner is not built")?;
                if let Some([x, y, z]) = at {
                    write!(f, " at ({x:.3}, {y:.3}, {z:.3})")?;
                }
                write!(f, ": {why}")
            }
            Error::CornerRefused { corner } => write!(
                f,
                "not supported yet: SMLib does not set back corner {}: it sets back a corner only where every edge meeting it is blended, all convex or all concave",
                corner + 1
            ),
            Error::NoEffect => write!(f, "SMLib made no change: it reported success, but the operation did nothing"),
            Error::ToolStopsShort => write!(f, "the tool runs into the face but ends inside it, so it cuts nothing off"),
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
            Error::BlendsNotFollowed { exact: true, why } => write!(f, "SMLib cannot build this exactly in the input's geometry: {why}"),
            Error::BlendsNotFollowed { exact: false, why } => write!(f, "SMLib cannot do this yet: {why}"),
            Error::Exception => write!(f, "SMLib raised an internal exception"),
            Error::Record(u) => write!(f, "{u}"),
            Error::Solidcore(e) => write!(f, "solidcore: {e}"),
            Error::Refused(e) => write!(f, "{e}"),
            Error::Rejected { operation, reason } => write!(f, "SMLib rejected the input: {reason} ({operation})"),
            Error::RefusedWithDiagnostic { refusal, diagnostic } => write!(f, "{refusal}: {diagnostic}"),
            Error::Internal(e) => write!(f, "solidcore failed internally in {e}"),
            Error::Unported(what) => write!(f, "not supported yet: {what}"),
            Error::Cancelled(call) => write!(f, "{call}: cancelled"),
        }
    }
}

impl std::error::Error for Error {}

pub type Result<T> = std::result::Result<T, Error>;

/// Names this crate's fix set ([`trace::fixes`]) to solidcore-ops' session, so the calls the
/// app makes through solidcore-ops run with the fixes its modelling runs with. An application
/// calls it once as it starts; it is also done before this crate's first kernel call. Calling
/// it again does nothing.
pub fn start() {
    core::start();
}

/// Where failing calls leave their inputs for replay ([`core::dump`]); `None` turns that off.
pub fn set_dump_dir(dir: Option<&Path>) {
    core::dump::set_dir(dir.map(Path::to_path_buf));
}

/// The kernel call running right now, if any: for crash reports. Takes no lock.
pub fn current_call() -> Option<String> {
    core::dump::current()
}

/// Keeps one of the kernels calls run on out of use until dropped: the next calls run on another.
/// For tests of what happens when a design's bodies come from a kernel that is busy elsewhere.
#[doc(hidden)]
pub fn hold_a_kernel() -> core::HeldKernel {
    core::hold_a_kernel()
}

/// Is this thread inside a solidcore call? A panic there is caught and becomes
/// [`Error::Internal`]; crash reports mark it recovered.
pub fn in_call() -> bool {
    core::in_call()
}

/// Every pooled kernel's profile (solidcore's `profile` feature), written as JSON Lines: the
/// scopes and counted work of its calls so far (feature `solidcore-profile`).
///
/// # Errors
///
/// Those of writing to `w`.
#[cfg(feature = "solidcore-profile")]
pub fn write_kernel_profiles(w: &mut impl std::io::Write) -> std::io::Result<()> {
    core::write_kernel_profiles(w)
}

/// [`solidcore_ops::session::profiled`], re-exported so the applications can dump each
/// solidcore-ops call's kernel profile (feature `solidcore-profile`).
#[cfg(feature = "solidcore-profile")]
pub use solidcore_ops::session::profiled;

/// A panic inside a solidcore call, which the call catches as it catches any
/// ([`Error::Internal`], the kernel replaced by a fresh one): the crash-report check
/// (`diagnostics.crash_test`'s kernel mode). The crate's one deliberate panic.
#[allow(clippy::panic)]
pub fn crash_test() -> Result<()> {
    core::run("crash test", |_| -> std::result::Result<(), solidcore::SolidError> {
        std::panic::panic_any(String::from("crash test: an internal error in the kernel"))
    })
}

/// A B-rep. Immutable: operations return new bodies.
pub struct Brep {
    body: core::Body,
    /// Every bounded region with more than one shell had its outer shell confirmed on import.
    enclosing_shell_order_proven: bool,
    /// The body's number in a trace ([`trace`]).
    id: u64,
}

impl std::fmt::Debug for Brep {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("smlib::Brep")
    }
}

impl Brep {
    /// A body solidcore made.
    fn made_core(body: core::Body) -> Brep {
        Brep { body, enclosing_shell_order_proven: false, id: trace::next_id() }
    }

    /// solidcore's body.
    fn core(&self) -> &core::Body {
        &self.body
    }

    /// An axis-aligned box from its lower corner, with positive edge lengths (mm).
    pub fn make_box(origin: [f64; 3], size: [f64; 3]) -> Result<Brep> {
        let r = core::make_box(origin, size).map(Brep::made_core);
        trace::made("make_box", &[], &r, || serde_json::json!({"origin": origin, "size": size}));
        r
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
        let r = core::transform::copy(self.core()).map(Brep::made_core);
        trace::made("copy", &[self], &r, || serde_json::json!({}));
        r
    }

    /// This body moved by the affine `matrix` (column-major: columns 0–2 the images of x, y and
    /// z, column 3 the translation), as SMLib's `SmBrep::Transform` takes a matrix: reflections
    /// mirror it, a non-uniform scale or a shear turns its geometry into NURBS, tolerances scale
    /// with it.
    pub fn transformed(&self, matrix: &[[f64; 4]; 4]) -> Result<Brep> {
        let r = core::transform::transform(self.core(), matrix).map(Brep::made_core);
        trace::made("transform", &[self], &r, || serde_json::json!({"matrix": matrix}));
        r
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
