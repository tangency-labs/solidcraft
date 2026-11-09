//! Why an operation failed, as data. An application words these for its users; the
//! [`std::fmt::Display`] here is a plain statement of the variant for logs and tests.

use solidcore::SolidError;

/// SMLib's `SM_ERR_INVALID_INPUT`, the status of input the kernel refuses.
pub const INVALID_INPUT: i32 = solidcore::error::SM_ERR_INVALID_INPUT;

/// Why an operation failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The kernel's own status (1001 `SM_ERR`, 1007 `SM_ERR_INVALID_INPUT`, …).
    Status(i32),
    /// An argument refused before the kernel saw it: not finite, out of range, or an index
    /// past its list.
    Argument,
    /// The kernel reported success but the result has no faces (for a boolean: it is empty).
    Empty,
    /// The kernel reported success, but a solid input gave a result that is not a closed solid.
    NotSolid,
    /// No room for the operation: an inward offset (a shell's wall, faces moved in) leaves none
    /// inside the body, or a blend is larger than the faces it rolls on allow.
    NoRoom,
    /// An offset whose walls would meet along a line (a shell's): the offsets of two faces
    /// touch without crossing.
    WallsTouch,
    /// A blend larger than its faces allow, refused with the largest size that builds it
    /// (the kernel's fit, `SolidError::FilletRefused`): the size, what it sizes, a point at the
    /// middle of the edge it was found for, and why there is no room, when the refusal names them.
    TooLarge {
        /// The largest size that builds it.
        size: f64,
        /// What the size sizes.
        kind: FitKind,
        /// A point at the middle of the edge it was found for.
        at: Option<[f64; 3]>,
        /// Why there is no room.
        why: Option<RoomReason>,
    },
    /// A shell, offset or thicken whose wall is thicker than a face curves (the kernel's
    /// `OffsetFailure::Folds`): the largest wall that works, when the kernel names one.
    WallFolds {
        /// The largest wall that works.
        largest: Option<f64>,
    },
    /// The kernel cannot build this exactly yet (a variable radius that is zero inside its
    /// edge): what it cannot.
    Unsupported(&'static str),
    /// A blend's edges meet at a vertex of a kind no blend corner handles yet.
    CornerUnsupported,
    /// The kernel does not set back corner `corner` (an index into the call's corners): it sets
    /// back a corner only where every edge meeting it is blended, all convex or all concave.
    CornerRefused {
        /// The corner's index among the call's corners.
        corner: usize,
    },
    /// The rounds would leave a face whose boundary crosses itself (rounds overlapping where
    /// a wall is thinner than both): the points where it does, and the largest size that
    /// leaves no crossing, when the kernel finds it.
    RoundsCross {
        /// Points where a face's boundary crosses itself.
        at: Vec<[f64; 3]>,
        /// The largest size that leaves no crossing.
        fit: Option<f64>,
    },
    /// A band's room is not known at an end of the edge: a face's side there leans away from
    /// the edge (how far its far end lies past the plane square to the edge), or is not
    /// straight (no lean). The vertex is a record index.
    TaperEndUnsupported {
        /// The edge's vertex at that end.
        vertex: usize,
        /// The side's lean past the square, when the side is straight.
        lean: Option<f64>,
    },
    /// The faces of the edge close to a point at both its ends: no band fits there.
    FacesCloseAtBothEnds,
    /// A variable round's radius is larger than its band's room at a station of the edge:
    /// the station (from the start the call named) and the largest radius proved to fit there.
    RoomRunsOut {
        /// The station, a fraction of the way along the edge.
        at: f64,
        /// The largest radius proved to fit there.
        largest: f64,
    },
    /// The kernel reported success but the operation did nothing: a blend that made no face, a
    /// face move or draft that moved no face, an extension shorter than the side can carry.
    NoEffect,
    /// The answer is not exact in the input's geometry.
    NotExact,
    /// A planar profile the kernel refuses, and why (`solidcore::prism::profile::check_regions`).
    Profile(solidcore::prism::profile::ProfileRefusal),
    /// The geometry has no value where one is needed (the kernel's `Degenerate`): a normal
    /// at a pole or an apex, a zero-length vector, a collapsed edge.
    Degenerate,
    /// The kernel is unsure whether a region is material, and the answer depends on it (a
    /// point's class, mass properties): refused, never read as material or void. The
    /// region, by its kernel id's index.
    UnsureRegion {
        /// The kernel region's id index.
        region: u32,
    },
    /// A drafted extrusion's corner its rule cannot close (the kernel's taper refusal): why, and
    /// where when the kernel's point is finite. Not a blend's [`Error::CornerRefused`], which
    /// names a corner of a fillet's call that is not set back.
    DraftCornerRefused {
        /// Why.
        why: crate::sweep::CornerRefusal,
        /// The corner's point.
        at: Option<[f64; 3]>,
    },
    /// A sweep path the kernel leaves undecided for every exact shape it is read as (a
    /// comparison within its rounding bound of the path tolerance): the first such piece of the
    /// path and its comparison.
    PathUndecided {
        /// The piece's index in the path.
        piece: usize,
        /// The comparison.
        check: crate::sweep::PathCheck,
    },
    /// A sweep path whose pieces `join` and `join + 1` are not proved to meet to the path gap; for
    /// the last join (`join + 1` the path's length), its end and its start, neither proved to meet
    /// (closed) nor proved apart (open).
    PathGap {
        /// The join's index: after piece `join`.
        join: usize,
    },
    /// An extension that cannot run as far as asked (a weight of the extended surface would
    /// vanish, a pole, an apex or a full turn is reached, or its length there cannot be
    /// measured to the kernel's stop): the longest distance proved to work, when there is one.
    TooFar {
        /// The longest distance that works.
        largest: Option<f64>,
    },
    /// The kernel found its own state inconsistent part way through a call, or was called
    /// before the preparation it needs.
    Exception,
    /// The application cancelled the call.
    Cancelled,
    /// Ops was called from inside a kernel call or from the failure callback.
    Reentered,
    /// A record the conversion cannot carry to the kernel, and why.
    Record(String),
    /// The kernel panicked; the session caught it: the call, then the panic's message.
    Internal(String),
    /// A step the kernel has not ported yet, refused rather than built another way: what is
    /// missing.
    Unported(&'static str),
    /// An edge that has no blend (the kernel's `BlendRefusal::EdgeNotBlendable`): its index
    /// and why.
    EdgeNotBlendable {
        /// The edge's index.
        edge: usize,
        /// Why it has no blend.
        reason: Unblendable,
    },
    /// A blend that cannot end where its edge runs on tangentially into an edge not blended
    /// with it (the kernel's `BlendRefusal::ContinuesInto`): blending that edge too, its
    /// tangent chain ([`crate::blend::tangent_chain`]), runs the blend on.
    ContinuesInto {
        /// The blended edge's index.
        edge: usize,
        /// The index of the edge it runs on into.
        continues_into: usize,
    },
    /// A round too small to change the part (the kernel's `BlendRefusal::RoundTooSmall`): it
    /// lies within the edge's tolerance of the edge.
    RoundTooSmall {
        /// The edge's index.
        edge: usize,
        /// The least turn of its faces' normals across it, radians.
        turn: f64,
        /// The edge's tolerance.
        tolerance: f64,
        /// The least radius that rounds it.
        smallest: f64,
    },
    /// A corner's setback that stops short of where the rails of the rounds beside an edge
    /// cross it (the kernel's `BlendRefusal::SetbackTooShort`).
    SetbackTooShort {
        /// The corner's vertex index.
        vertex: usize,
        /// The least setback.
        least: f64,
    },
    /// A corner's setback that does not fit on its edges, or on its rounds where a far
    /// corner cuts one (the kernel's `BlendRefusal::SetbackTooLong`): one below `largest`
    /// fits.
    SetbackTooLong {
        /// The corner's vertex index.
        vertex: usize,
        /// The bound below which a setback fits.
        largest: f64,
    },
    /// A chamfer's angle that runs out past the other face instead of meeting it (the
    /// kernel's `BlendRefusal::ChamferAngleTooLarge`): no chamfer of the edge between its
    /// faces has it. Radians.
    ChamferAngleTooLarge {
        /// The edge's index.
        edge: usize,
        /// The angle asked.
        angle: f64,
        /// The bound below which an angle meets the other face.
        largest: f64,
    },
}

/// Why an edge has no blend ([`Error::EdgeNotBlendable`]), as the kernel names it
/// (`NotBlendable`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Unblendable {
    /// Its faces meet tangentially all along it: nothing to round.
    Smooth,
    /// It is not between two faces.
    NotTwoFaces,
    /// Its faces are tangent, or folded back on each other, where the blend is set up.
    Tangent,
    /// The side to blend cannot be told.
    Ambiguous,
}

/// Why a blend has no room, as the kernel names its refusal (`NoRoomReason`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RoomReason {
    /// The rolling ball does not fit a face it rolls on.
    BallDoesNotFit,
    /// A contact line leaves the face it rolls on.
    RailLeavesFace,
    /// The blends take the whole of a face they roll on.
    FaceConsumed,
    /// One blend's contact line runs inside the band another takes off a face they share.
    BandsOverlap,
    /// The balls fold over one another along the edge.
    BallsFold,
    /// A face curves tighter than a chamfer's offset along it.
    OffsetFolds,
    /// The blend would take all of the part between its contact lines and cut it in pieces.
    CutsThePart,
    /// The blend's section would break through a wall into the hollow behind it.
    BreaksThrough,
    /// The ball is as wide as a face it rolls on, so its centre stays put.
    OffsetCollapses,
    /// The blend's corners take the whole of its edge.
    CornersTakeWholeEdge,
    /// A blend past its faces runs off a face into the material.
    RunsIntoMaterial,
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

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::Status(INVALID_INPUT) => {
                write!(f, "solidcore rejected the input (SM_ERR_INVALID_INPUT)")
            }
            Error::Status(s) => write!(f, "solidcore failed (status {s})"),
            Error::Argument => write!(f, "invalid argument"),
            Error::Empty => write!(f, "the result is empty"),
            Error::NotSolid => write!(f, "the result is not a closed solid"),
            Error::NoRoom => write!(f, "no room"),
            Error::WallsTouch => write!(f, "the offset's walls meet"),
            Error::TooLarge { size, kind, .. } => {
                write!(f, "too large: the largest {kind:?} that fits is {size}")
            }
            Error::WallFolds { largest } => write!(f, "the wall folds (largest {largest:?})"),
            Error::CornerUnsupported => {
                write!(f, "a blend corner of this kind is not supported yet")
            }
            Error::CornerRefused { corner } => write!(f, "corner {corner} is not set back"),
            Error::RoundsCross { at, fit } => write!(
                f,
                "the rounds leave a face whose boundary crosses itself at {at:?} (largest {fit:?})"
            ),
            Error::TaperEndUnsupported { vertex, lean } => write!(
                f,
                "the band's room is not known at vertex {vertex} (its side leans {lean:?})"
            ),
            Error::FacesCloseAtBothEnds => {
                write!(f, "the faces close to a point at both ends of the edge")
            }
            Error::RoomRunsOut { at, largest } => write!(
                f,
                "the room runs out at {at} along the edge (largest radius there {largest})"
            ),
            Error::NoEffect => write!(f, "the operation did nothing"),
            Error::NotExact => write!(f, "not exact in the input's geometry"),
            Error::Profile(r) => write!(
                f,
                "planar profile refused: {}",
                match r {
                    solidcore::prism::profile::ProfileRefusal::Budget =>
                        "a loop not closed, empty, or past the segment budget",
                    solidcore::prism::profile::ProfileRefusal::LineOrArc =>
                        "lines and circular arcs must be finite and of some length",
                    solidcore::prism::profile::ProfileRefusal::FreeForm =>
                        "free-form segments must be finite and of some length",
                }
            ),
            Error::Degenerate => write!(f, "degenerate geometry where a value is needed"),
            Error::UnsureRegion { region } => {
                write!(f, "unsure whether region {region} is material")
            }
            Error::DraftCornerRefused { why, at } => {
                write!(f, "the draft corner at {at:?} is refused: {why:?}")
            }
            Error::PathUndecided { piece, check } => write!(
                f,
                "the sweep path is undecided at piece {piece} ({check:?})"
            ),
            Error::PathGap { join } => write!(
                f,
                "the sweep path's pieces {join} and {} do not meet",
                join.saturating_add(1)
            ),
            Error::TooFar { largest } => write!(f, "too far to extend (largest {largest:?})"),
            Error::Exception => write!(f, "solidcore raised an internal exception"),
            Error::Cancelled => write!(f, "cancelled"),
            Error::Reentered => write!(
                f,
                "solidcore-ops was called from inside a call or the failure callback"
            ),
            Error::Record(w) => write!(f, "record: {w}"),
            Error::Internal(e) => write!(f, "solidcore failed internally in {e}"),
            Error::Unsupported(what) | Error::Unported(what) => {
                write!(f, "not supported yet: {what}")
            }
            Error::EdgeNotBlendable { edge, reason } => {
                write!(f, "edge {edge} has no blend ({reason:?})")
            }
            Error::ContinuesInto {
                edge,
                continues_into,
            } => write!(
                f,
                "the blend of edge {edge} cannot end where it runs on into edge {continues_into}"
            ),
            Error::RoundTooSmall { edge, smallest, .. } => write!(
                f,
                "the round of edge {edge} is too small to change the part: the smallest is {smallest}"
            ),
            Error::SetbackTooShort { vertex, least } => {
                write!(f, "the setback at vertex {vertex} is shorter than {least}")
            }
            Error::SetbackTooLong { vertex, largest } => {
                write!(f, "the setback at vertex {vertex} does not fit: below {largest} fits")
            }
            Error::ChamferAngleTooLarge {
                edge,
                angle,
                largest,
            } => write!(
                f,
                "the chamfer angle {angle} rad of edge {edge} runs past the other face: below {largest} rad fits"
            ),
        }
    }
}

impl std::error::Error for Error {}

/// An operation's result.
pub type Result<T> = std::result::Result<T, Error>;

/// A kernel error as the operation's, by its type: a kernel status where it stands for one
/// (input refused, not converged, the kernel's other failures), a refusal of what the kernel does
/// not support or has not ported, a cancellation, or an inconsistency the kernel found in itself.
#[must_use]
pub(crate) fn status(e: SolidError) -> Error {
    use solidcore::error::{SM_ERR, SM_ERR_INVALID_INPUT, SM_ERR_NOT_CONVERGING};
    match e {
        SolidError::InvalidInput { .. } => Error::Status(SM_ERR_INVALID_INPUT),
        SolidError::NotConverged { .. } => Error::Status(SM_ERR_NOT_CONVERGING),
        SolidError::NotFound { .. } => Error::Status(SM_ERR),
        SolidError::Degenerate { .. } => Error::Degenerate,
        SolidError::UnsureRegion { region, .. } => Error::UnsureRegion {
            region: region.index(),
        },
        SolidError::Unsupported { reason, .. } => Error::Unsupported(reason),
        SolidError::Unported { upstream, .. } => Error::Unported(upstream),
        SolidError::Cancelled { .. } => Error::Cancelled,
        SolidError::PortInvariant { .. } | SolidError::PreparationRequired { .. } => {
            Error::Exception
        }
        // the kernel's statuses (Upstream, Nlib, the refusals the families map further), and any
        // kind the kernel adds by the status it stands for
        e => Error::Status(e.sm_status().unwrap_or(SM_ERR)),
    }
}
