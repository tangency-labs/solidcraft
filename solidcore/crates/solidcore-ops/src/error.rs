//! Why an operation failed, as data. An application words these for its users; the
//! [`std::fmt::Display`] here is a plain statement of the variant for logs and tests.

use solidcore::SolidError;

pub use solidcore::topology::brep::SolidTest;

/// SMLib's `SM_ERR_INVALID_INPUT`, the status of input the kernel refuses.
pub const INVALID_INPUT: i32 = solidcore::error::SM_ERR_INVALID_INPUT;

/// Why an operation failed.
#[derive(Clone, Debug, PartialEq)]
pub enum Error {
    /// The kernel's own status (1001 `SM_ERR`, 1007 `SM_ERR_INVALID_INPUT`, …).
    Status(i32),
    /// Input the kernel refuses, in its words (its `SolidError::InvalidInput`): the routine that
    /// refused it and what was wrong, as the kernel states them. Displays as
    /// `"{operation}: {reason}"`.
    Refused {
        /// The kernel routine that refused the input.
        operation: &'static str,
        /// What was wrong with it.
        reason: &'static str,
    },
    /// An argument refused before the kernel saw it: not finite, out of range, or an index
    /// past its list.
    Argument,
    /// The kernel reported success but the result has no faces (for a boolean: it is empty).
    Empty,
    /// The kernel reported success, but a solid input gave a result that is not a closed solid.
    NotSolid,
    /// A body that is not a closed solid facing out, and the test it fails (the kernel's
    /// [`SolidTest`], the first that fails in the order the tests run). [`Error::NotSolid`]
    /// stays for the calls that return it; [`crate::Body::check_solid`] returns this one.
    #[non_exhaustive]
    NotSolidBecause(SolidTest),
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
    /// back a corner only where every edge meeting it is blended, all convex or all concave, or
    /// of mixed convexity with curvature-continuous rounds between planes.
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
    /// Curves on a face that do not divide it into regions ([`crate::trim`]): an open curve
    /// whose ends reach neither the face's boundary nor another curve leaves a slit, and curves
    /// off the face cut nothing.
    CurveDoesNotCut,
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
    /// The curves bounding a patch (`crate::surface`) that do not meet end to end: the end of
    /// curve `join` is not proved within the model's zone tolerance of an end of the next one
    /// (for the last join, `join + 1` the number of curves, of the first), `gap` apart (the
    /// kernel's distance between the nearest ends).
    BoundaryGap {
        /// The join's index: after curve `join`.
        join: usize,
        /// How far apart the ends are.
        gap: f64,
    },
    /// A curve network (`crate::network`) whose u-curve `u` and v-curve `v` do not cross
    /// once: the kernel's intersector found `count` crossings.
    NoCrossing {
        /// The u-curve's index.
        u: usize,
        /// The v-curve's index.
        v: usize,
        /// How many crossings were found.
        count: usize,
    },
    /// A curve network whose u-curve `u` and v-curve `v` are not proved within the model's
    /// zone tolerance at the network's common crossing parameters: `gap` bounds how far apart
    /// they are there (the curves do not cross at one parameter of each family).
    NetworkGap {
        /// The u-curve's index.
        u: usize,
        /// The v-curve's index.
        v: usize,
        /// The certified bound of their distance.
        gap: f64,
    },
    /// A change to a curve that would move it farther than the tolerance asked (a knot
    /// removed, its degree lowered): the kernel's certified bound of how far it would move.
    PastTolerance {
        /// The certified bound of the move.
        bound: f64,
    },
    /// An extension that cannot run as far as asked (a weight of the extended surface would
    /// vanish, a pole, an apex or a full turn is reached, or its length there cannot be
    /// measured to the kernel's stop): the longest distance proved to work, when there is one.
    TooFar {
        /// The longest distance that works.
        largest: Option<f64>,
    },
    /// An offset whose distance from its curve, measured at stations along it, strays from the
    /// distance asked by more than the tolerance: the tolerance and the worst found.
    OffsetPastTolerance {
        /// The tolerance asked.
        tolerance: f64,
        /// The largest difference found between a station's distance and the distance asked.
        worst: f64,
    },
    /// A loft guided by rails whose guided edges were asked within `asked` of their rails: at
    /// the most wires asked, `wires`, the bound the kernel proved was still `reached`
    /// (`solidcore::sweep::loft_rails`).
    RailPastTolerance {
        /// The tolerance asked.
        asked: f64,
        /// The largest bound proved between a rail and the skin.
        reached: f64,
        /// The wires (sections and stations) at the cap.
        wires: usize,
    },
    /// The kernel found its own state inconsistent part way through a call, or was called
    /// before the preparation it needs.
    Exception,
    /// The kernel found one of its own invariants broken part way through a call (the
    /// kernel's `SolidError::PortInvariant`): its class, that it is a defect, the operation, and
    /// what did not hold. Not [`Error::Exception`], which ops' own consistency checks and a
    /// missing preparation still give.
    #[non_exhaustive]
    Invariant {
        /// The kernel's class of the failure (`SolidError::kind`, today
        /// [`solidcore::error::ErrorKind::PortInvariant`]).
        class: solidcore::error::ErrorKind,
        /// True: the failure is the kernel's fault, not a fact of the input (a consistency check
        /// of its own that did not hold), a bug to report, not a refusal.
        defect: bool,
        /// The kernel operation whose invariant failed.
        operation: &'static str,
        /// What did not hold.
        reason: &'static str,
    },
    /// A contact test the kernel's certified bracket of the boundary distance does not decide
    /// (`solidcore::query::contact`): the tolerance lies inside `[lower, upper]` (or the
    /// boundaries are apart but one body's enclosure of the other is not decided), or a
    /// distance's bracket is still wider than asked. Never read as either answer. `other` is
    /// the index of the body tested among the call's others.
    ContactUndecided {
        /// The other body's index among the call's (0 for one).
        other: usize,
        /// No point of either boundary is nearer the other than this.
        lower: f64,
        /// A pair of points, one on each boundary, is this far apart.
        upper: f64,
    },
    /// The application cancelled the call.
    Cancelled,
    /// A body built from a record whose blend face's net its definition does not certify (the
    /// kernel's `DefinitionNotCertified`): the face, by its index in the body's face order, and
    /// the bound ε its definition states.
    DefinitionNotCertified {
        /// The face's index in the body's face order.
        face: usize,
        /// The bound the definition states.
        eps: f64,
    },
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
    /// A corner's setback too short for the corner: it stops short of where the rails of the
    /// rounds beside an edge cross it, or its fill is not certified at it (the kernel's
    /// `BlendRefusal::SetbackTooShort`, whose `why` says which).
    SetbackTooShort {
        /// The corner's vertex index; `None` where the vertex is no longer the body's.
        vertex: Option<usize>,
        /// The least setback.
        least: f64,
        /// The corner's point, where the kernel's session notes it.
        at: Option<[f64; 3]>,
        /// Whether a setback of `least` builds the corner: its fill's test named it (not the
        /// rails', which `least` passes only), and the kernel's fill is the corner (no other
        /// patch replaces it).
        builds: bool,
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
    /// An offset, shell, thicken, sheet offset or draft the kernel refuses with a reason of
    /// its own (its `OffsetFailure`, other than those [`Error::WallsTouch`],
    /// [`Error::WallFolds`] and [`Error::Unsupported`] carry): why, and the face it names, by
    /// its record index where the call takes the body's [`crate::Order`] (else by its index
    /// among the body's faces), when it names one the body holds.
    OffsetRefused {
        /// The face the refusal names.
        face: Option<usize>,
        /// Why.
        why: OffsetRefusal,
    },
    /// A fillet or chamfer the kernel fails with a reason it states in words (its
    /// `BlendFailure`, which replaced a bare status): the kernel's class of the failure, the
    /// status it stands for, and the kernel's statement of it. Transitional, as
    /// [`OffsetRefusal::Phrase`] is: each reason gets a type of its own once the kernel gives
    /// it a cause of its own.
    BlendFailed {
        /// The kernel's class of the failure.
        class: solidcore::error::ErrorKind,
        /// The status the failure stands for (1001 `SM_ERR`, ...).
        status: i32,
        /// The kernel's statement of it.
        phrase: String,
    },
}

/// Why the kernel refuses an offset or a draft ([`Error::OffsetRefused`]), as its
/// `OffsetFailure` names it. More reasons get names of their own, so match it with a wildcard.
#[derive(Clone, Debug, PartialEq)]
#[non_exhaustive]
pub enum OffsetRefusal {
    /// The face's offset by `distance` is not proved regular, and no wall size would help:
    /// the face bends towards that side nowhere the kernel's sizing looked, so no radius of
    /// curvature limits the wall, and the proof ran out (not a wall thicker than the face
    /// curves, [`Error::WallFolds`]).
    NotCertified {
        /// The offset distance asked for (signed).
        distance: f64,
    },
    /// The face's offset by `distance` collapses onto a line or point: the face has a radius
    /// of curvature `radius` on the displaced side that the distance reaches.
    Collapses {
        /// The offset distance asked for (signed).
        distance: f64,
        /// The radius of curvature the face attains on the displaced side.
        radius: f64,
    },
    /// Loops of the face's offset cross in a way the offset does not resolve: how.
    LoopsCross {
        /// How they cross.
        crossing: solidcore::error::offset::LoopCrossing,
    },
    /// The offset distance is not a finite length at least the body's tolerance, the
    /// smallest distance it resolves.
    DistanceUnresolved {
        /// The distance asked for.
        distance: f64,
        /// The smallest distance the body resolves.
        resolution: f64,
    },
    /// The offset body's tolerance would be nonzero but no more than the effective zero: one
    /// no body can hold.
    ToleranceUnresolved {
        /// The tolerance asked for.
        tolerance: f64,
    },
    /// The face meets itself across a seam, and the surface the operation gives it is not
    /// closed there.
    SeamOpened,
    /// A vertex moves to or past an edge's other end. The offset does not make the
    /// topology change needed when that edge vanishes.
    EdgeVanishes,
    /// The faces beside an edge meet on an exact curve that could not be cut into its pieces.
    ExactCurveUncut,
    /// In a local operation, the new surfaces of the face and of its neighbour across an
    /// edge meet nowhere near the edge, so the edge cannot be found again.
    FacesApart,
    /// How far the surfaces beside an edge or at a vertex of the face must be extended to
    /// meet has no certified bound (a pair or corner of kinds it does not bound yet): refused
    /// before anything changed.
    ReachUncertified,
    /// An offset face has a gap in its loops, and the segment that would bridge it is not
    /// established within tolerance.
    LoopGapUncertified,
    /// The body built fails what the input did not: what.
    Unsound {
        /// What the built body fails.
        what: solidcore::error::offset::Unsoundness,
    },
    /// A failure the kernel states only in words so far: a routine's own failure (its phrase
    /// causes `Failed`, `Missing`, `InvalidValue` and `Inconsistent`), or a reason this version
    /// of ops does not name yet. Transitional: each gets a variant of its own here once the
    /// kernel gives it a cause of its own.
    Phrase {
        /// The kernel's class of the failure.
        class: solidcore::error::ErrorKind,
        /// True when the failure is the kernel's fault, not a fact of the input (a consistency
        /// check of its own that did not hold): a bug to report, not a refusal.
        defect: bool,
        /// The kernel's statement of it.
        phrase: String,
    },
}

impl std::fmt::Display for OffsetRefusal {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            OffsetRefusal::NotCertified { distance } => {
                write!(f, "its offset by {distance} is not proved regular")
            }
            OffsetRefusal::Collapses { distance, radius } => write!(
                f,
                "its offset by {distance} collapses (a radius of curvature {radius})"
            ),
            OffsetRefusal::LoopsCross { crossing } => write!(
                f,
                "its offset's loops cross in a way the offset cannot resolve: {crossing}"
            ),
            OffsetRefusal::DistanceUnresolved {
                distance,
                resolution,
            } => write!(
                f,
                "the distance {distance} is below the body's resolution {resolution}"
            ),
            OffsetRefusal::ToleranceUnresolved { tolerance } => {
                write!(f, "the tolerance {tolerance} is one no body can hold")
            }
            OffsetRefusal::SeamOpened => write!(f, "its surface opens across its seam"),
            OffsetRefusal::EdgeVanishes => write!(
                f,
                "a vertex moves to or past an edge's other end; the edge would vanish, \
                 a change of topology the offset does not make"
            ),
            OffsetRefusal::ExactCurveUncut => {
                write!(f, "an exact curve between extended faces could not be cut")
            }
            OffsetRefusal::FacesApart => {
                write!(
                    f,
                    "its new surface and a neighbour's meet nowhere near their edge"
                )
            }
            OffsetRefusal::ReachUncertified => {
                write!(
                    f,
                    "how far its neighbours extend to meet it has no certified bound"
                )
            }
            OffsetRefusal::LoopGapUncertified => {
                write!(
                    f,
                    "a gap in its offset's loops cannot be bridged within tolerance"
                )
            }
            OffsetRefusal::Unsound { what } => write!(f, "{what}"),
            OffsetRefusal::Phrase { phrase, .. } => f.write_str(phrase),
        }
    }
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
    /// A blend past its faces keeps none of its edge, no corner taking any of it: a knife
    /// edge's chamfer wider than the part takes all of it.
    OverflowKeepsNothing,
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

impl RoomReason {
    /// The reason in words, as a fillet's message gives it.
    #[must_use]
    pub const fn words(self) -> &'static str {
        match self {
            RoomReason::BallDoesNotFit => "the rolling ball does not fit a face it rolls on",
            RoomReason::RailLeavesFace => "a contact line leaves the face it rolls on",
            RoomReason::FaceConsumed => "the blends take the whole of a face they roll on",
            RoomReason::BandsOverlap => {
                "one blend runs inside the band another takes off a face they share"
            }
            RoomReason::BallsFold => "the rolling ball folds over itself along the edge",
            RoomReason::OffsetFolds => "a face curves tighter than the offset along it",
            RoomReason::CutsThePart => "the blend would cut the part in pieces",
            RoomReason::BreaksThrough => {
                "the blend would break through a wall into the hollow behind it"
            }
            RoomReason::OffsetCollapses => {
                "the ball is as wide as a face it rolls on, so its centre stays put and the round cannot end at the edge's end"
            }
            RoomReason::CornersTakeWholeEdge => "its corners take the whole of the edge",
            RoomReason::RunsIntoMaterial => "the blend runs off a face into the material",
            RoomReason::OverflowKeepsNothing => {
                "the round or chamfer runs past its faces and keeps none of its edge"
            }
        }
    }
}

impl FitKind {
    /// The size's name, as a fillet's message names it ("the largest radius that fits").
    #[must_use]
    pub const fn word(self) -> &'static str {
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
            Error::Status(INVALID_INPUT) => {
                write!(f, "solidcore rejected the input (SM_ERR_INVALID_INPUT)")
            }
            Error::Status(s) => write!(f, "solidcore failed (status {s})"),
            Error::Refused { operation, reason } => write!(f, "{operation}: {reason}"),
            Error::Argument => write!(f, "invalid argument"),
            Error::Empty => write!(f, "the result is empty"),
            Error::NotSolid => write!(f, "the result is not a closed solid"),
            Error::NotSolidBecause(test) => {
                write!(f, "the body is not a closed solid facing out: {test}")
            }
            Error::NoRoom => write!(f, "no room"),
            Error::RailPastTolerance {
                asked,
                reached,
                wires,
            } => write!(
                f,
                "the loft's guided edges were asked within {asked} of their rails; at {wires} wires the bound proved is {reached}"
            ),
            Error::OffsetPastTolerance { tolerance, worst } => write!(
                f,
                "the offset strays up to {worst} from the distance asked, past the tolerance {tolerance}"
            ),
            Error::WallsTouch => write!(f, "the offset's walls meet"),
            Error::DefinitionNotCertified { face, eps } => {
                write!(f, "the definition of face {face} does not certify its net within {eps}")
            }
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
            Error::CurveDoesNotCut => write!(f, "the curves do not divide the face into regions"),
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
            Error::BoundaryGap { join, gap } => write!(
                f,
                "the boundary curves {join} and {} do not meet: {gap} apart",
                join.saturating_add(1)
            ),
            Error::NoCrossing { u, v, count } => write!(
                f,
                "the network's u-curve {u} and v-curve {v} cross {count} times, not once"
            ),
            Error::NetworkGap { u, v, gap } => write!(
                f,
                "the network's u-curve {u} and v-curve {v} are up to {gap} apart where they cross"
            ),
            Error::PastTolerance { bound } => {
                write!(f, "the change would move the curve by up to {bound}")
            }
            Error::TooFar { largest } => write!(f, "too far to extend (largest {largest:?})"),
            Error::Exception => write!(f, "solidcore raised an internal exception"),
            Error::Invariant {
                class,
                defect,
                operation,
                reason,
            } => {
                let defect = if *defect { ", a defect to report" } else { "" };
                write!(
                    f,
                    "solidcore broke its own invariant in {operation} ({class}{defect}): {reason}"
                )
            }
            Error::ContactUndecided {
                other,
                lower,
                upper,
            } => write!(
                f,
                "contact with body {other} is undecided: the boundary distance is in [{lower}, {upper}]"
            ),
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
            Error::SetbackTooShort {
                vertex: Some(vertex),
                least,
                ..
            } => {
                write!(f, "the setback at vertex {vertex} is shorter than {least}")
            }
            Error::SetbackTooShort {
                vertex: None,
                least,
                ..
            } => write!(f, "the setback at a corner is shorter than {least}"),
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
            Error::OffsetRefused { face, why } => match face {
                Some(face) => write!(f, "the offset is refused at face {face}: {why}"),
                None => write!(f, "the offset is refused: {why}"),
            },
            Error::BlendFailed { status, phrase, .. } => {
                write!(f, "the blend failed (status {status}): {phrase}")
            }
        }
    }
}

impl std::error::Error for Error {}

/// An operation's result.
pub type Result<T> = std::result::Result<T, Error>;

/// A kernel error as the operation's, by its type: input it refuses in its words
/// ([`Error::Refused`]), a kernel status where it stands for one (not converged, the kernel's
/// other failures), a refusal of what the kernel does
/// not support or has not ported, a cancellation, a broken invariant of the kernel's
/// ([`Error::Invariant`]), or a call made before the preparation it needs.
#[must_use]
pub(crate) fn status(e: SolidError) -> Error {
    use solidcore::error::{SM_ERR, SM_ERR_NOT_CONVERGING};
    match e {
        SolidError::InvalidInput { operation, reason } => Error::Refused { operation, reason },
        SolidError::NotConverged { .. } => Error::Status(SM_ERR_NOT_CONVERGING),
        SolidError::NotFound { .. } => Error::Status(SM_ERR),
        SolidError::Degenerate { .. } => Error::Degenerate,
        SolidError::UnsureRegion { region, .. } => Error::UnsureRegion {
            region: region.index(),
        },
        SolidError::Unsupported { reason, .. } => Error::Unsupported(reason),
        SolidError::Unported { upstream, .. } => Error::Unported(upstream),
        SolidError::Cancelled { .. } => Error::Cancelled,
        e @ SolidError::PortInvariant { operation, reason } => Error::Invariant {
            class: e.kind(),
            defect: true,
            operation,
            reason,
        },
        SolidError::PreparationRequired { .. } => Error::Exception,
        // the kernel's statuses (Upstream, Failed, the refusals the families map further), and any
        // kind the kernel adds by the status it stands for
        e => Error::Status(e.sm_status().unwrap_or(SM_ERR)),
    }
}
