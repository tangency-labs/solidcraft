//! Why a blend fails, in solidcore's terms: the cause of the typed blend failure record.
//!
//! Upstream: none. These describe failures of the blend subsystem (`crate::blend`: the
//! fillet and feature executives, the fillet geometry, solvers, sections, corners,
//! overflows, intersectors, outlines, removal and the exact and rolling-ball fillets).
//!
//! **The record.** A blend failure is a [`Failure`] (shared by every area) whose reason is
//! a [`BlendFailure`], the cause: typed, with the numbers the check computed. Its class is
//! [`BlendFailure::class`], an [`ErrorKind`] derived from the cause, so class and cause
//! cannot disagree; [`ErrorKind::is_defect`], [`ErrorKind::is_refusal`] and
//! [`ErrorKind::sized`] say what kind of failure it is. The failure's [`super::Site`] says
//! where it is on the caller's body and its [`super::Fit`] the size that builds, where the
//! check computed one.
//!
//! **Layout.** Each cause's doc names its class and the status difftest compares; the arms
//! of [`BlendFailure::class`] and `comparison_status` are the source. The enums that detail
//! a cause live in their own files under `error/blend/`. Every enum's variants and the arms
//! of every match on them are kept in name order, one arm per pattern, so changes that each
//! add a cause touch different lines unless their names fall next to each other; a test
//! checks the order.
//!
//! **Statuses.** Each failure also records, for difftest and the goldens only, the status
//! the C++ reports for it (crate-private, read through [`crate::SolidError::sm_status`] and
//! `upstream_status`); it never decides the type. The status a cause gives unless its
//! site says otherwise is named in its doc.
//!
//! **The phrase causes** (the nine causes `phrase_causes!` lists, [`BlendFailure::Failed`] first) are the
//! shape the blend sites were converted to before the record, some of them refusals not yet
//! moved onto it. Each keeps the kind of its status, as before the record, and reads as
//! neither a defect nor a refusal ([`BlendFailure::is_phrase`]); its phrase stays in its
//! message. They go class by class: a site that has a finer cause moves to it, and one that
//! is the kernel's fault to [`BlendFailure::Defect`], with its reason. A test counts the
//! library sites that build a phrase cause, per file, and lets the count only fall.
//! [`BlendFailure::StageFailed`] is the form of a defect not yet reviewed, also counted per
//! file. The conversion guide: `docs/failure-codes-and-reproduction.md` §6.
//!
//! Deviations: not upstream. The failure's `operation` path says which routine. No cause
//! names an entity of its own: a failure is at the entity the site it replaces named
//! (`SolidError::blend_at`), until the site moves onto the typed [`super::Site`]. A check
//! of a caller's arguments that returned a status is a cause of class
//! [`ErrorKind::InvalidInput`] keeping that status; the other checks of a caller's arguments
//! stay [`crate::SolidError::InvalidInput`]. Every cause stands in for the status its site
//! returned ([`BlendFailure::replaces_upstream_status`]), so the decisions that read an
//! upstream status read these alike. Only part of `crate::blend` uses these yet; the files
//! that still build bare statuses are listed, with their counts, in this module's ratchet
//! test.

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_ASSERT_FAILURE, SM_ERR_BAD_INTERSECTIONS,
    SM_ERR_INVALID_INPUT, SM_ERR_NOT_CONVERGING, SM_ERR_NOT_WITHIN_TOLERANCE, SM_ERR_NULL_POINTER,
    SM_ERR_OUTSIDE_OF_DOMAIN,
};
use crate::blend::fillet_executive::NoRoomReason;

mod band;
mod cliff;
mod corner;
mod crease_trim;
mod crossing;
mod ease;
mod keep;
mod law_corner;
mod no_room;
mod overflow;
mod removal;
mod section;
mod set_up;
mod solver;
pub mod trace;
mod weld;

pub use band::BandInput;
pub use cliff::CliffInput;
pub use corner::CornerInput;
pub use corner::CornerPatchMiss;
pub use corner::CornerShape;
pub use corner::CornerStep;
pub use corner::MixedStep;
pub use crease_trim::CreaseTrimStop;
pub use crossing::CrossingInput;
pub use ease::EaseGeometry;
pub use ease::EaseInput;
pub use ease::EaseStop;
pub use keep::KeepStop;
pub use law_corner::LawCornerSearch;
pub use no_room::no_room_class;
pub use overflow::OverflowDrop;
pub use overflow::OverflowInput;
pub use removal::removal_class;
pub use removal::RemovalRefusal;
pub use section::SectionDegeneracy;
pub use section::SectionInput;
pub use section::SectionLimit;
pub use section::SectionProof;
pub use set_up::FilletSetUp;
pub use solver::AnalyticShape;
pub use solver::BaseSolverMethod;
pub use solver::SolverFailure;
pub use trace::TraceFailure;
pub use weld::WeldGap;

/// The phrase causes, by variant name: the one list [`BlendFailure::is_phrase`] and the
/// phrase-site scan's test (`phrase_sites::BLEND`) read. It only shrinks, as sites move to
/// typed causes; a new variant is never a phrase cause, so it needs no line here.
/// `phrase_causes!(m(args))` expands to `m!(args; Failed, Missing, ...)`.
macro_rules! phrase_causes {
    ($then:ident($($args:tt)*)) => {
        $then!($($args)*; Failed, Missing, InvalidValue, Inconsistent, BadIntersections,
            NotConverged, OutsideDomain, NotWithinTolerance, PassedOn)
    };
}

/// Whether `cause` is one of the variants named.
macro_rules! matches_phrase_cause {
    ($cause:expr; $($variant:ident),*) => {
        matches!($cause, $(BlendFailure::$variant { .. })|*)
    };
}

/// Why a blend fails: the cause of the typed blend failure record. Its class is
/// [`BlendFailure::class`].
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum BlendFailure {
    /// The search for the parameter `length` along a curve from parameter `start` stopped
    /// at parameter `stopped_at`: its Newton iteration stopped moving before the length
    /// was reached (a length past the curve's end is no failure). Not converged
    /// ([`ErrorKind::NotConverged`]); status `SM_ERR_NOT_CONVERGING`, as upstream's
    /// `FindParameterAtArcLength` reports it.
    ArcLengthNotConverged {
        /// The parameter the search starts from.
        start: f64,
        /// The arc length asked (negative: towards the curve's start).
        length: f64,
        /// The parameter where the iteration stopped.
        stopped_at: f64,
    },
    /// Intersections the routine needs are not usable, as the phrase says (for example "the
    /// two surfaces meet in more than two points"); upstream `SM_ERR_BAD_INTERSECTIONS`. A
    /// phrase cause ([`BlendFailure::is_phrase`]).
    BadIntersections(&'static str),
    /// The ball corner's triangle between its three contacts is not one rational patch
    /// from the pole tried: the far side's image under the stereographic projection is
    /// not one arc of under a half turn (the triangle is too large, or a contact lies at
    /// the pole's antipode), or a weight of the patch's net is not positive. Unsupported
    /// ([`ErrorKind::Unsupported`]): the corner's geometry, not the kernel's fault; a
    /// triangle too large for one patch is fanned into three.
    BallTriangleNotOnePatch {
        /// The contact the patch's pole was put at.
        pole: usize,
    },
    /// At the section at `s` along the trace a variable round's balls change their radius at
    /// least as fast as their centres move (`|r'| = rate >= |c'| = speed`): each ball holds
    /// the next, so no rolling-ball surface exists there (#15256). A refusal
    /// ([`ErrorKind::BallsFold`]).
    /// Status `SM_ERR`.
    BallsNest {
        /// The section's parameter along the trace.
        s: f64,
        /// `|r'|`: how fast the balls' radius changes along the trace.
        rate: f64,
        /// `|c'|`: how fast the balls' centres move along the trace.
        speed: f64,
    },
    /// An input the planar-band contact query reads is not there (a support, an edge use, an
    /// offset, a foot of a drop, ...): the input named. Status `SM_ERR_NULL_POINTER` and the
    /// kind of that status ([`ErrorKind::Missing`]), as the phrase cause it replaces; typed,
    /// so not a phrase cause ([`BlendFailure::is_phrase`]).
    BandInputMissing(BandInput),
    /// A vertex given to bevel is not a vertex of the filleted edges. A check of the
    /// caller's arguments ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`
    /// (upstream `NER` of its corner).
    BevelVertexNotFilleted,
    /// A surface binding of the fillet insertion is resolved after its face's surface was
    /// removed: a stale binding the caller kept past an edit of the body; the failure is at
    /// the face. A check of the caller's input ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER`.
    BoundSurfaceRemoved,
    /// The face named for a chamfer's first size is neither of its edge's two faces; the
    /// failure is at the edge. A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    ChamferFaceNotOfEdge,
    /// The chamfer size given for an edge is not one it can take: a distance, chord or angle
    /// that is not finite and positive, setbacks at the edge's wedge that are not finite and
    /// positive (an angle so small the second underflows to 0), distances that fold at the
    /// wedge (each must be more than the other's cosine of it), any size where the edge's
    /// faces make no wedge (tangent or folded back), or on a knife edge any size but two
    /// equal distances; the failure is at the edge. An angle past the room a wedge leaves
    /// is [`crate::blend::blend_refusal::BlendRefusal::ChamferAngleTooLarge`] instead. A
    /// check of the caller's arguments ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_INVALID_INPUT`.
    ChamferSizeInvalid,
    /// A cliff face lacks geometry needed to trim the blend. Status `SM_ERR_NULL_POINTER`.
    /// Its class is [`ErrorKind::Missing`].
    CliffInputMissing {
        /// The face that lacks the input.
        face: crate::topology::FaceId,
        /// The missing geometry.
        input: CliffInput,
    },
    /// The interior section did not divide the periodic blend face. Status `SM_ERR`.
    /// Its class is [`ErrorKind::Failed`].
    ClosedCliffWouldNotSplit {
        /// The blend face in the sheet being trimmed.
        face: crate::topology::FaceId,
    },
    /// The two intersection curves of the fillet pairs at a corner where three fillets
    /// meet do not cross: the global search found no common point. An empty search
    /// ([`ErrorKind::NotFound`]); status `SM_ERR`.
    CornerCurvesDoNotMeet,
    /// The two intersection curves of the fillet pairs at a corner where three fillets
    /// meet cross `crossings` times, not once: the corner's vertex is built only at a
    /// single crossing. A missing construction ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR`.
    CornerCurvesMeetMoreThanOnce {
        /// The crossings found.
        crossings: usize,
    },
    /// A fillet corner step is given a corner, or a pseudo-B-rep, that is not as the corner
    /// topology makes it: `input` names what it lacks or holds otherwise, `entity` the
    /// entity it is about (of the pseudo-B-rep, of the target body, or the corner's vertex)
    /// where the step has one. A check of the caller's corner ([`ErrorKind::InvalidInput`]):
    /// the corner steps (`crate::blend::fillet_corner`) are public and read a `FilletCorner`
    /// and a `FilletBrep` that a caller can build part by part, while the fillet executive
    /// builds them whole before it runs the steps. Status that of the site it replaces
    /// ([`CornerInput::upstream_status`] unless the site says otherwise).
    CornerInputInvalid {
        /// What the corner or its pseudo-B-rep lacks or holds otherwise.
        input: CornerInput,
        /// The entity it is about, where the step has one.
        entity: Option<EntityRef>,
    },
    /// A corner patch surface has no live face in the intermediate body: building the face
    /// on that surface gave none, or it is gone. An empty search ([`ErrorKind::NotFound`]):
    /// neither a defect nor a refusal; status `SM_ERR_NULL_POINTER` (the fork's `NER` of
    /// `GetPatchFaces`).
    CornerPatchFaceNotFound {
        /// The surface's index among the corner's patch surfaces.
        surface: usize,
    },
    /// A fillet corner's surface, patch or face that the corner does not make: the miss
    /// named. Its class and status are the miss's ([`CornerPatchMiss::class`],
    /// [`CornerPatchMiss::upstream_status`]).
    CornerPatchNotMade(CornerPatchMiss),
    /// The drop of a degenerate corner's vertex onto one of its blend's base surfaces found
    /// `solutions` points, not one, and the side of a periodic seam the face's trim lies on
    /// does not pick one of them: no single foot to end the rails at. An empty search
    /// ([`ErrorKind::NotFound`]); status `SM_ERR`.
    CornerPointDropUnresolved {
        /// The points the drop found.
        solutions: usize,
    },
    /// A corner of the fillet was refused at its vertex (the fork's `bRefused`): the
    /// executive's error info holds the corner's error, comment and the size that builds.
    /// [`ErrorKind::Failed`], neither a defect nor a refusal on its own; status `SM_ERR`.
    CornerRefused,
    /// A fillet corner whose edges meet in a shape the corner topology does not build: the
    /// shape named. Unsupported ([`ErrorKind::Unsupported`]), a fact of the body's edges at
    /// the vertex; status that of the site it replaces ([`CornerShape::upstream_status`]).
    CornerShapeNotBuilt(CornerShape),
    /// The intersection of two surfaces at a fillet corner (two fillet surfaces, or a
    /// fillet surface and a side face's extended surface) failed: the intersection's
    /// error is kept as the cause and decides whether the failure is a defect or a
    /// refusal. A failed stage ([`ErrorKind::Failed`]); status `SM_ERR`.
    CornerSurfacesNotIntersected,
    /// A ball or chamfer corner has no construction at the step named: a solve that ends
    /// within rounding found or proved nothing, or the geometry is one the construction
    /// does not build. The step's class ([`CornerStep::class`]); status `SM_ERR`.
    CornerUnbuildable(CornerStep),
    /// The crease trim (#17673) has no construction to end a crease piece of curve kind
    /// `kind` exactly on the meet at its start or end (`at_end`), at parameter `t` of the
    /// crease: a curve with no control polygon (an offset, projected, composite, ...
    /// curve). Unsupported
    /// ([`ErrorKind::Unsupported`]): never a piece left ending short of its meet.
    /// Status `SM_ERR`.
    CreasePieceEndUnbuildable {
        /// The piece's curve kind.
        kind: &'static str,
        /// True for the piece's end, false for its start.
        at_end: bool,
        /// The end's parameter on the crease.
        t: f64,
    },
    /// A blend trimmed at its creases (the fork's `TrimAtCreases`, the bound of its open ends
    /// included) is not built for a reason with no blend-too-large reason
    /// ([`CreaseTrimStop`] says why, and gives the kind). The executive notes the refusal in
    /// its error info as well; status `SM_ERR`.
    CreaseTrimNotBuilt(CreaseTrimStop),
    /// A cross-section type is given as unknown. A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    CrossSectionTypeUnknown,
    /// An input a rail's crossing of an edge is judged from is not there (the crossing's
    /// budget, an edge use's radial, the side edge's curve, the crossing's place on it): the
    /// input named. Status `SM_ERR_NULL_POINTER` and the kind of that status
    /// ([`ErrorKind::Missing`]); typed, so not a phrase cause ([`BlendFailure::is_phrase`]).
    CrossingInputMissing(CrossingInput),
    /// What a rail's crossing of an edge is judged from (a face normal or binormal at the
    /// crossing) was not a finite number, so the crossing is neither classed concave nor
    /// convex (#17016). A defect ([`ErrorKind::StageFailed`]), status `SM_ERR`.
    CrossingNotFinite,
    /// A curve the step needs as a B-spline curve, the curve of fillet edge `edge` (`None`
    /// where the step names no edge), is of kind `kind` (its type string), which is not of
    /// a B-spline class. An unsupported kind ([`ErrorKind::Unsupported`]); status `SM_ERR`
    /// unless its site says otherwise (`SM_ERR_NULL_POINTER` where upstream dereferences
    /// the failed cast).
    CurveWithoutNet {
        /// The fillet edge whose curve it is, where the step names one.
        edge: Option<crate::topology::EdgeId>,
        /// The curve's kind, as its type string.
        kind: &'static str,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// A curve or surface the ease of a round before a setback blends has no B-spline form
    /// (the fork's `SM_CAST_PTR(SmBSplineSurface/Curve, ..)`; [`EaseGeometry`] says which).
    /// A kind the ease does not build on ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR_NULL_POINTER` (upstream `NER` of the cast).
    EaseGeometryNotBSpline(EaseGeometry),
    /// A thing the ease of a round before a setback reads lacks the input named
    /// ([`EaseInput`]: a corner's vertex, a round's solver, rail, surface or centre line, a
    /// solver's offset surface, a face's surface, an edge's curve or an edge use's face). The
    /// ease reads the caller's body and the rounds of the solvers a caller adds and drives
    /// through the session's own steps (`FilletSession::new_constant_radius_solver`,
    /// `load_solver`, `calc_solver_geometry`, `create_fillet_brep`) alike, so the cause names
    /// what is missing and leaves the blame open, as [`BlendFailure::CliffInputMissing`]
    /// does: status
    /// `SM_ERR_NULL_POINTER` (upstream `NER`) and the kind of that status
    /// ([`ErrorKind::Missing`]), neither a defect nor a refusal.
    EaseInputMissing(EaseInput),
    /// The ease of a round before a setback stops where the geometry gives it nothing to
    /// build on ([`EaseStop`] says which, and gives the kind); status `SM_ERR`.
    EaseNotBuilt(EaseStop),
    /// The edge at `index` of the edges given has no edge use the fillet can roll on (it
    /// does not lie between two faces it can blend). A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    EdgeNotFilletable {
        /// The edge's index in the edges given.
        index: usize,
    },
    /// The edge at `index` of the edges given is not a live edge of the target body. A check
    /// of the caller's arguments ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_INVALID_INPUT`.
    EdgeNotInTarget {
        /// The edge's index in the edges given.
        index: usize,
    },
    /// The fillet insertion's split of an edge at `parameter` is declined by the native split
    /// (`MakeVertexSplitEdge`'s `SM_ERR`): the parameter lies outside the edge or within an
    /// end vertex's tolerance of an end, or the body is not open for edits. The intersector
    /// that asks for the split passes over a failed one. Neither a defect nor a refusal
    /// ([`ErrorKind::Failed`]); status `SM_ERR`.
    EdgeSplitDeclined {
        /// The edge parameter the split was asked at.
        parameter: f64,
    },
    /// An edge use of the caller's body a fillet reads (the first edge use of a solver the
    /// caller set up, or a solver's or side edge use the fillet geometry reads) has no edge;
    /// the failure is at the edge use. A check of the caller's
    /// input ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`.
    EdgeUseWithoutEdge,
    /// An edge of the caller's body a fillet reads (one a smooth sequence of filleted edges
    /// joins, a filleted edge, an edge of a face it crosses, or a filleted or side edge the
    /// fillet geometry's vertex calculators read) has no curve; the failure is at the edge.
    /// A check of the caller's body ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_NULL_POINTER` unless its site says otherwise.
    EdgeWithoutCurve,
    /// The exact blend given to the rational round cannot place the edge's middle round its
    /// cylinder's or cone's axis: from where the axis crosses the conic's plane, the half-line
    /// to the edge's middle (moved along the axis onto the plane) has no direction, or does
    /// not cross the conic ahead of the axis. The plane's section of a cylinder or cone
    /// encloses where its own axis crosses the plane, and an edge of that round lies off the
    /// axis, so the exact blend is not the one of the edge the caller's solver names. A check
    /// of the caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    ExactBlendAxisCannotPlaceEdge {
        /// The distance from where the axis crosses the plane to the edge's middle moved onto
        /// it, in the conic's own coordinates (where it is the unit circle); at or below the
        /// kernel's zero the half-line has no direction.
        length: f64,
        /// The discriminant of the half-line's line crossing the conic; negative: it misses
        /// (meaningless when `length` is at or below the kernel's zero).
        discriminant: f64,
        /// Where the half-line crosses the conic, in half-line lengths from the axis, when
        /// the crossing was computed; not positive: behind the axis.
        crossing: Option<f64>,
    },
    /// A step of the fillet executive failed (`FILEXEC_ERR`): the executive's error info
    /// says which (its code and comment, and for a blend too large its reason and the size
    /// that builds), and the error of the stage that failed, where there is one, is kept as
    /// the cause. [`ErrorKind::Failed`], neither a defect nor a refusal on its own, so the
    /// kept cause decides ([`crate::SolidError::is_defect`]); status `SM_ERR`.
    ExecutiveStepFailed,
    /// The end of a kept rail's run past a face's boundary lies on the lines of two side
    /// edges of the face (straight edges that are not blended), and the extension joins the
    /// end to one side line only. A construction the overflow
    /// does not build ([`ErrorKind::Unsupported`]); status `SM_ERR`.
    ExtensionEndOnTwoSideLines,
    /// A face given to a surface fillet, or a face of the caller's body a fillet reads, has
    /// no surface; the failure is at the face. A check of the caller's arguments and body
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT` (`SM_ERR_NULL_POINTER`
    /// where a stage reads the surface unchecked, upstream `NER`, and `SM_ERR` where the
    /// fork's tangent roll-over returns that).
    FaceHasNoSurface,
    /// The routine could not continue, as the phrase says (for example "the fillet's law
    /// has no value at the base"). Upstream `return SE(SM_ERR)`. A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    Failed(&'static str),
    /// A surface/surface fillet's offset surfaces meet in more than one curve, and the
    /// curves at `first` and `second` of the intersection cross: the fillet would cross
    /// itself, which it does not build. Unsupported ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR`.
    FilletCrossesItself {
        /// The first curve's index in the intersection.
        first: usize,
        /// The second curve's index in the intersection.
        second: usize,
    },
    /// A link of the fillet structures a routine is given (the fillet body, its corners and
    /// solvers) is not there, or not as the routine needs: the link named. The caller's
    /// input ([`ErrorKind::InvalidInput`]): a caller can build those structures with the
    /// public API and leave the link out; status `SM_ERR_NULL_POINTER` where upstream
    /// dereferences the link, `SM_ERR` where it checks it.
    FilletSetUpInvalid(FilletSetUp),
    /// The fillet vertex a vertex calculator runs on has no fillet corner, whose filleted
    /// vertex and kind the calculators read. The caller's input
    /// ([`ErrorKind::InvalidInput`]): the calculators are public, and a fillet vertex a
    /// caller builds need not have a corner; status `SM_ERR_NULL_POINTER` (upstream
    /// dereferences the corner).
    FilletVertexWithoutCorner,
    /// A G^n cross-section is asked with an order outside 2 to 4, or a thumbweight of 1 or
    /// more. A check of the caller's arguments ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_INVALID_INPUT`.
    GnSectionInvalid {
        /// The continuity order asked.
        order: usize,
        /// The thumbweight asked.
        thumbweight: f64,
    },
    /// A consistency check of the routine's own data did not hold, as the phrase says
    /// (upstream `AERN`, `SM_ERR_ASSERT_FAILURE`). A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    Inconsistent(&'static str),
    /// The 3D curve built between two rails' ends drops onto the surface of the face `face`
    /// as `curves` UV curves, not one: a curve that crosses a closed surface's seam or
    /// leaves the surface's domain has no one UV curve on it. Unsupported
    /// ([`ErrorKind::Unsupported`]); status `SM_ERR`.
    InterpolantDropsToCurves {
        /// The body's face.
        face: crate::topology::FaceId,
        /// The UV curves the drop made.
        curves: usize,
    },
    /// The curve a surface/surface intersection picked has no UV curve on its first surface
    /// (or its second, `on_second`), and the surface's drop of it made none: a curve that
    /// crosses a closed surface's seam has no one UV curve on its domain, and a curve that
    /// leaves the surface has none either. Unsupported ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR`.
    IntersectionCurveHasNoUvCurve {
        /// True for the second surface, false for the first.
        on_second: bool,
    },
    /// The curve a surface/surface intersection picked is of kind `kind` (its type string),
    /// which is not of a B-spline class: the result is a B-spline curve, trimmed or joined
    /// from it, and this kind has none. An unsupported kind ([`ErrorKind::Unsupported`]);
    /// status `SM_ERR` unless its site says otherwise (`SM_ERR_NULL_POINTER` where it
    /// replaces a `NER`).
    IntersectionCurveNotBSpline {
        /// The curve's kind, as its type string.
        kind: &'static str,
    },
    /// Neither end of the intersection curve of a fillet surface with a side face's
    /// extended surface, at a 3x2 mixed corner, drops onto the face `face` the corner's
    /// vertex is on, within ten times the solver's approximation tolerance. An empty
    /// search ([`ErrorKind::NotFound`]); status `SM_ERR`.
    IntersectionEndsOffFace {
        /// The face the vertex is on.
        face: crate::topology::FaceId,
    },
    /// A value the routine is given or computes with is not usable, as the phrase says
    /// (for example "a G^n section's order"). Upstream returns `SM_ERR_INVALID_INPUT` as a
    /// status here, so its kind is [`ErrorKind::InvalidInput`]. A phrase cause
    /// ([`BlendFailure::is_phrase`]); an argument check is [`crate::SolidError::InvalidInput`].
    InvalidValue(&'static str),
    /// A Join some of whose blends roll a rail over a smooth far edge of their faces has no
    /// blend whose rails roll on the two faces its shared section ends on, so the section
    /// has no ends to take (#16491). Unsupported ([`ErrorKind::Unsupported`]): the kernel
    /// builds no section for such a Join; status `SM_ERR`.
    JoinBlendsRollOffSection,
    /// A curve of kind `kind` that a blend joins end to end with another
    /// (`SmBSplineCurve::JoinWith`: a fillet corner's edge curves, a removed fillet's edge
    /// curves) has no B-spline part to join. Unsupported ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR_NULL_POINTER` (upstream `NER` of the cast).
    JoinCurveNotBSpline {
        /// The curve's kind.
        kind: &'static str,
    },
    /// The `blend`th blend of a Join (0 for the first) has no ball found in the plane of the
    /// Join's shared section: the plane solve from the ball's feet at the corner's vertex
    /// stopped without a ball. The solve reports no residual, so the stop proves nothing
    /// about the ball. Not converged ([`ErrorKind::NotConverged`]); status `SM_ERR`.
    JoinSectionNotConverged {
        /// The blend's place among the Join's blends.
        blend: usize,
    },
    /// A round kept round a feature standing in its band (the fork's `KeepFeaturesInBands`)
    /// is not built ([`KeepStop`] says why, and gives the kind). The executive notes the
    /// refusal in its error info as well; status `SM_ERR`.
    KeptFeatureNotBuilt(KeepStop),
    /// A search the exact round at a radius law's corner makes finds nothing: the
    /// [`LawCornerSearch`] `what`, at the corner at index `corner` of the round's law corners.
    /// An empty search ([`ErrorKind::NotFound`]): neither a defect nor a refusal; status
    /// `SM_ERR_NULL_POINTER` (the fork's `NER` of the search's result).
    LawCornerNotFound {
        /// The corner's index among the round's law corners.
        corner: usize,
        /// What the search looked for.
        what: LawCornerSearch,
    },
    /// A base radius law, which has no value of its own, is evaluated. Unsupported
    /// ([`ErrorKind::Unsupported`]); status `SM_ERR`.
    LawHasNoValue,
    /// A radius law is evaluated over an interval of zero length. A check of the caller's
    /// arguments ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    LawIntervalEmpty,
    /// The face `face` a point or curve is built on (a mate vertex's face, or the face a
    /// rail/rail interpolation curve lies on) has a surface of kind `kind` (its type string),
    /// which is not of a B-spline class: the point or curve is evaluated on the surface's
    /// B-spline extension, and this kind has none. An unsupported kind
    /// ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER` (the `NER` of the cast it
    /// replaces).
    MateFaceSurfaceNotBSpline {
        /// The body's face.
        face: crate::topology::FaceId,
        /// The surface's kind, as its type string.
        kind: &'static str,
    },
    /// Something the routine needs is missing, as the phrase says (for example "the edge
    /// use's edge"). Upstream `NER(ptr)` on a `NULL` pointer, or a `NULL` it would
    /// dereference. A phrase cause ([`BlendFailure::is_phrase`]).
    /// Status `SM_ERR_NULL_POINTER`.
    Missing(&'static str),
    /// The Mixed corner (a convex and a concave blend meeting at a tangent seam) has no
    /// construction for its geometry at the step named: a crossing its construction needs
    /// is not there, its shape is degenerate there, or the shape is one the construction
    /// does not build. The step's class ([`MixedStep::class`]); status `SM_ERR`.
    MixedCornerUnbuildable(MixedStep),
    /// The corner's vertex is computed from the fillet vertex `vertex` across a fillet edge,
    /// which is not processed yet: the fillet executive computes a corner's vertices in an
    /// order that processes it first, so only a caller running the corner calculators by
    /// hand out of order reaches this. The caller's set-up ([`ErrorKind::InvalidInput`]);
    /// status `SM_ERR`.
    NeighbourVertexNotProcessed {
        /// The fillet vertex across the edge.
        vertex: crate::topology::VertexId,
    },
    /// No blended edge's curve takes the drop of the middle of an extension's chain, so the
    /// side the blend takes off from is not known. An empty search ([`ErrorKind::NotFound`]);
    /// status `SM_ERR`.
    NoBlendedEdgeNearExtension,
    /// The fillet is given no edges, or the executive asked to preview its fillet holds no
    /// solver (no edge was set up). A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT` (`SM_ERR` for the
    /// preview).
    NoEdgesGiven,
    /// A surface fillet is given a face of the first surface's body where the first surface
    /// has none (it is itself that face, or a free surface). A check of the caller's
    /// arguments ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    NoFirstBody,
    /// A blend larger than its faces allow, for the reason named (classes 100 to 124, 128
    /// and 129; [`no_room_class`]): [`ErrorKind::FilletTooLarge`] (100) for a weld bead
    /// larger than its faces take ([`NoRoomReason::BeadCutsFace`],
    /// [`NoRoomReason::LegPastFace`], [`NoRoomReason::NoToeAtLeg`]). A refusal, but for
    /// [`NoRoomReason::CliffSubtractionLeavesNoSolid`], a defect
    /// ([`ErrorKind::CliffSubtractionLeavesNoSolid`]): the failure's fit, where the check
    /// computed one, is the size that builds.
    /// Status `SM_ERR`.
    NoRoom(NoRoomReason),
    /// The routine is asked about no vertex (`None` given for the vertex argument). The
    /// caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER` (upstream
    /// dereferences it).
    NoVertexGiven,
    /// A blend surface not certified within its tolerance in the work its certificate is
    /// allowed: the G^n section's certificate would halve spans past its budget
    /// ([`crate::blend::fillet_section::section::GN_HALVING_BUDGET`]), or has more stations
    /// than the fork's stop (4096), with spans still over its target (a tenth of the
    /// tolerance). The round is refused (not built), but not as a fact of its geometry: the
    /// budget, not the geometry, decides it, so its class is [`ErrorKind::NotWithinTolerance`],
    /// not one of the refusal classes ([`ErrorKind::is_refusal`] is false), and no size is
    /// named (the budget says nothing about which size would certify). Like a cancellation, no fallback turns it into
    /// another result ([`crate::SolidError::is_unported_or_cancelled`]). Status `SM_ERR`: it
    /// stands for the fork's own stop, the status `SM_ERR` with "the blend could not be
    /// certified within its tolerance", which the port returned as
    /// [`BlendFailure::Failed`] before it was typed.
    NotCertified {
        /// The spans halved when it was refused.
        halvings: usize,
        /// The certificate's stations when it was refused.
        stations: usize,
    },
    /// An iteration did not converge, as the phrase says; upstream
    /// `SM_ERR_NOT_CONVERGING`. A phrase cause ([`BlendFailure::is_phrase`]).
    NotConverged(&'static str),
    /// A result is farther than the tolerance from where it must be, as the phrase says;
    /// upstream `SM_ERR_NOT_WITHIN_TOLERANCE`. A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    NotWithinTolerance(&'static str),
    /// A parameter lies outside its domain, as the phrase says; upstream
    /// `SM_ERR_OUTSIDE_OF_DOMAIN`. A phrase cause ([`BlendFailure::is_phrase`]).
    OutsideDomain(&'static str),
    /// A face or edge a blend's overflow reads lacks the input named (a face's surface, an
    /// edge's curve or one of its vertices). The overflows' helpers, which the trims at
    /// creases and the kept features share, read the caller's body and the sheet the
    /// executive built alike, so the failure names no entity, and the cause
    /// names what is missing and leaves the blame open, as [`BlendFailure::CliffInputMissing`]
    /// does: status `SM_ERR_NULL_POINTER` (upstream `NER`) and the kind of that status
    /// ([`ErrorKind::Missing`]), neither a defect nor a refusal.
    OverflowInputMissing(OverflowInput),
    /// A point of the cliff overflow does not drop onto the geometry named
    /// ([`OverflowDrop`]): the closest-point search found no foot (within ten tolerances, for
    /// an edge's curve). An empty search ([`ErrorKind::NotFound`]); status `SM_ERR`.
    OverflowPointDoesNotDrop(OverflowDrop),
    /// A list of the fillet's parameters (cross-section types, radius types, radii) holds
    /// neither one entry nor one per edge. A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    ParameterListSizes {
        /// The edges to fillet.
        edges: usize,
        /// The cross-section types given.
        cross_sections: usize,
        /// The radius types given.
        radius_types: usize,
        /// The radii given.
        radii: usize,
    },
    /// A stage the routine calls failed, as the phrase says, and the routine answers with
    /// that stage's `status` (upstream `SER` of the stage's status, or a status chosen at
    /// run time). The stage's error is not kept as a cause: it may be a solidcore-side
    /// failure, which the bare status it replaces did not carry. A phrase cause
    /// ([`BlendFailure::is_phrase`]).
    PassedOn {
        /// What failed.
        what: &'static str,
        /// The status the routine answers with (a comparison detail).
        status: i32,
    },
    /// A list that takes one entry per edge holds `given` entries for `edges` edges. A
    /// check of the caller's arguments ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_INVALID_INPUT`.
    PerEdgeListLength {
        /// The edges to fillet.
        edges: usize,
        /// The entries the list holds.
        given: usize,
    },
    /// An end given to a surface/plane crossing could not be put on the crossing within
    /// tolerance. An empty search ([`ErrorKind::NotFound`]); status `SM_ERR`.
    PlaneCurveEndOff {
        /// True for the crossing's end, false for its start.
        at_end: bool,
        /// The larger of the distance from the given end to the crossing point found and
        /// its distance to the plane; `None` when the drop or projection found no point.
        distance: Option<f64>,
    },
    /// The crossing's Hermite fit still has spans farther than a tenth of the tolerance
    /// from the exact curve when its station cap or its 20 refinement passes ran out, or
    /// when a pass found no point to split any of them at.
    /// Not within tolerance ([`ErrorKind::NotWithinTolerance`]); status `SM_ERR`.
    PlaneCurveFitCap {
        /// The stations held when fitting stopped.
        stations: usize,
        /// The refinement passes completed.
        passes: usize,
    },
    /// The crossing's trace leaves the surface's parameter domain in a direction it is not
    /// closed in, before reaching its end. Outside the domain ([`ErrorKind::OutsideDomain`]);
    /// status `SM_ERR`.
    PlaneCurveLeavesDomain {
        /// The station's surface parameter u.
        u: f64,
        /// The station's surface parameter v.
        v: f64,
    },
    /// A surface/plane trace's step was halved below its minimum at a station without a
    /// step being accepted: the march stopped. A defect ([`ErrorKind::StageFailed`]);
    /// status `SM_ERR`.
    PlaneCurveStepVanished {
        /// The last accepted station's surface parameter u.
        u: f64,
        /// The last accepted station's surface parameter v.
        v: f64,
        /// The step after halving.
        step: f64,
        /// The minimum step allowed.
        minimum: f64,
    },
    /// Tracing a surface/plane crossing found a station where the surface touches the
    /// plane: its tangent along the crossing has no length, so the crossing has no direction.
    /// Degenerate ([`ErrorKind::Degenerate`]); status `SM_ERR`.
    PlaneCurveTouches {
        /// The station's surface parameter u.
        u: f64,
        /// The station's surface parameter v.
        v: f64,
    },
    /// The radius law of a round is not positive at `s`, where the section is built on it:
    /// a check on the caller's law ([`ErrorKind::InvalidInput`]), status `SM_ERR`.
    RadiusLawNotPositive {
        /// The law's parameter.
        s: f64,
        /// The radius the law gives there.
        radius: f64,
    },
    /// The radius type of the edge at `index` is not one an edge's fillet is set up with
    /// (a constant radius, a constant distance or a variable radius). A check of the
    /// caller's arguments ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    RadiusTypeNotForEdges {
        /// The edge's index in the edges given.
        index: usize,
        /// The radius type given.
        kind: crate::blend::fillet_solver::p1::FilletSolverType,
    },
    /// The local solve of the crossing of two solvers' rails on the face `face` stopped
    /// without finding it. The solve reports neither its residual nor its iterations, so the
    /// stop proves nothing about the crossing. Not converged ([`ErrorKind::NotConverged`]);
    /// status `SM_ERR`.
    RailCrossingNotConverged {
        /// The face both rails lie on.
        face: crate::topology::FaceId,
    },
    /// At a blending corner a rail meets the side edge `edge` nowhere: every guess along
    /// the side edge, again with a doubled extension, and every continuation of the side
    /// edge past its far end found no point where the rail crosses it. An empty search
    /// ([`ErrorKind::NotFound`]); status `SM_ERR`. (At other corners the same miss is a
    /// vertex status the corner analyses, not a failure.)
    RailEndMissesSideEdge {
        /// The side edge of the body.
        edge: crate::topology::EdgeId,
    },
    /// The rail's curve does not cross the end face's surface (its extension, or an end
    /// cap's plane) within `tolerance`, the solver's approximation tolerance: the rail end
    /// at a mixed-convexity end has no crossing. An empty search ([`ErrorKind::NotFound`]);
    /// status `SM_ERR` (the vertex is marked `SurfIntFailure`).
    RailMissesEndSurface {
        /// The tolerance of the crossing.
        tolerance: f64,
    },
    /// The blending geometry's rail `rail` is not a blending rail: the fillet executive
    /// makes a blends geometry with blending rails only, so only a fillet B-rep set up by
    /// hand gives one. The caller's set-up ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    RailNotBlending {
        /// The rail.
        rail: crate::topology::EdgeId,
    },
    /// A fillet removal refuses the face it is asked to remove, for the reason named
    /// (classes 130 and 131; [`removal_class`]). A refusal that no size decides.
    /// Status `SM_ERR`.
    RemovalRefused(RemovalRefusal),
    /// The two seam curves of a closed surface are asked for between two fillet vertices
    /// of which neither lies at the surface's corner at v-min (`(u, v) = (min, min)`) nor
    /// at its corner at v-max: both ends of the seam are cut off, and the seam curves are
    /// built only with at most one end cut. A missing construction
    /// ([`ErrorKind::Unsupported`]); status `SM_ERR`.
    SeamCornersBothCut,
    /// The fillet vertex at the cut end of a closed surface's seam does not drop onto the
    /// seam's rib (the constant-u iso-curve at u-min) within `tolerance`: the drop that
    /// minimises the distance found no foot. An empty search ([`ErrorKind::NotFound`]);
    /// status `SM_ERR`.
    SeamCutOffRib {
        /// The tolerance of the drop.
        tolerance: f64,
    },
    /// The two seam curves of the surface a UV trim curve is made on are asked for, but
    /// the surface is not closed at its v-min or v-max corners. The fillet executive asks
    /// for them only on a closed join's fillet surface, which it closes, so only a caller
    /// passing another surface reaches this: the caller's input
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR`.
    SeamSurfaceNotClosed,
    /// A G^n section, its system or its stations degenerate where they are built (the
    /// section's area does not fall with its thumbweight, a weight or a length is zero, an
    /// arc is a half turn or more, a normal or tangent vanishes, the faces' normals agree, the
    /// rails or stations have no length, ...): [`ErrorKind::Degenerate`], status `SM_ERR`.
    SectionDegenerate(SectionDegeneracy),
    /// A G^n section's inputs do not fit together (a range past the round's stations, rails
    /// and a centre line over different intervals, a span past the definition's last
    /// station): a check on the caller's input ([`ErrorKind::InvalidInput`]), status
    /// `SM_ERR`.
    SectionInputInvalid(SectionInput),
    /// A G^n section's certificate could not prove what it must (a weight or the radius
    /// positive over a box, an enclosure, a bound finite): its
    /// interval enclosures were too wide. Not a fact of the geometry, which a narrower box
    /// may prove, nor the kernel's fault: [`ErrorKind::NotWithinTolerance`], neither defect
    /// nor refusal, like [`BlendFailure::NotCertified`]. Status `SM_ERR`; a certificate
    /// that reads it (with its operation) subdivides the box.
    SectionNotProved(SectionProof),
    /// A G^n section's construction or certificate does not handle its input (a face that
    /// is not a NURBS surface, a weight that is not positive, a blend face whose definition
    /// it does not enclose, an order past the series it holds): [`ErrorKind::Unsupported`],
    /// status `SM_ERR`.
    SectionUnsupported(SectionLimit),
    /// An edge's curve of kind `kind`, in a smooth sequence of filleted edges, has no
    /// B-spline form to join; the failure is at the edge. Unsupported
    /// ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER` (upstream `NER` of the
    /// cast).
    SequenceCurveNotBSpline {
        /// The curve's kind.
        kind: &'static str,
    },
    /// The plane of a set-back section passes through a point of the filleted edge that has
    /// `feet` closest points on a solver's offset surface, where the section's solve starts
    /// from the one closest point. Unsupported ([`ErrorKind::Unsupported`]): the kernel
    /// picks no start among several; status `SM_ERR`.
    SetBackFootNotUnique {
        /// The closest points found.
        feet: usize,
    },
    /// A rail's end was looked for along a chain of side edges, each continuing the last
    /// across a vertex (a seam's or a rolled-over edge's), and the chain reached `steps`
    /// edges, the most solidcore follows, without the end. Unsupported
    /// ([`ErrorKind::Unsupported`]): the bound keeps the search's recursion finite; status
    /// `SM_ERR`.
    SideEdgeChainPastBound {
        /// The most side edges followed.
        steps: usize,
    },
    /// A fillet size (a radius, a chamfer's distance, or a setback a chamfer's angle or
    /// chord gives at its edge's wedge) is below eight times the body's
    /// tolerance, the smallest the fillet builds. A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    SizeBelowResolution {
        /// The size given.
        size: f64,
        /// The body's tolerance.
        tolerance: f64,
    },
    /// A quantity a variable round's skin check computes (a distance from the skin to its
    /// balls, or the balls' radius rate and centre speed at a section) was still not a finite
    /// number, last at the section at `s` along the trace, when the refinement reached its
    /// most rounds or stations (#15256): a defect ([`ErrorKind::StageFailed`]). A value that
    /// is not finite before the cap splits its span instead.
    /// Status `SM_ERR`.
    SkinMeasureNotFinite {
        /// The section's parameter along the trace.
        s: f64,
    },
    /// A variable round's skin of balls' touch circles was still measured farther than
    /// `asked` from its balls when its refinement reached its most rounds or stations
    /// (#15256): the worst distance measured was `reached` (infinite where a span's section
    /// could not be placed on the skin to be measured), with `stations` stations after
    /// `rounds` rounds. Not within the tolerance ([`ErrorKind::NotWithinTolerance`]): no
    /// tolerance is loosened and the skin is not kept.
    /// Status `SM_ERR`.
    SkinPastStationCap {
        /// The tolerance asked: the solver's approximation tolerance.
        asked: f64,
        /// The largest distance measured from the skin to its balls at the cap.
        reached: f64,
        /// The stations the skin had at the cap.
        stations: usize,
        /// The refinement rounds run.
        rounds: usize,
    },
    /// A failure of the fillet solver (`crate::blend::fillet_solver`): its class and status
    /// are the cause's ([`SolverFailure::class`], status `SM_ERR` unless the cause says
    /// otherwise). Typed, so not a phrase cause ([`BlendFailure::is_phrase`]).
    Solver(SolverFailure),
    /// The face a ball is asked to touch is named by an index past a fillet solver's two
    /// faces, first (0) and second (1). A check of the caller's arguments
    /// ([`ErrorKind::InvalidInput`]); status `SM_ERR_INVALID_INPUT`.
    SolverFaceIndexInvalid {
        /// The face index given.
        index: usize,
    },
    /// A fillet edge's split face was recorded without the face that was split or the new
    /// face. The caller's input ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`
    /// (upstream dereferences both faces).
    SplitFaceNotGiven,
    /// A surface/surface intersection asked for the curve that leaves its start along the
    /// heading it was given (`SrfSrfOptions::closed_by_start_direction`) found curves
    /// through both points, but none leaves the start along that heading. An empty search
    /// ([`ErrorKind::NotFound`]): the heading is computed by the kernel's own caller (the
    /// turned-back arc of `fillet_geom::p5`), so a miss is not the user's input; status
    /// `SM_ERR`.
    SrfSrfHeadingNotFollowed,
    /// A surface/surface intersection found no curve through its end points: of the
    /// `candidates` intersection curves, none passes within the approximation tolerance of
    /// both (or of the reference point). An empty search ([`ErrorKind::NotFound`]); status
    /// `SM_ERR`.
    SrfSrfNoCurveThroughEnds {
        /// The intersection curves found and tried.
        candidates: usize,
    },
    /// A stage of the blend failed with no reason the kernel names: a defect. The failure
    /// holds the operation, the status (for the goldens only) and where in solidcore's source
    /// it was built ([`Failure::origin`]). The form not yet reviewed: new code builds
    /// [`BlendFailure::Defect`], a named cause or a refusal instead.
    /// Its class is [`ErrorKind::StageFailed`] (127); status `SM_ERR` unless its site says otherwise.
    StageFailed,
    /// The solve of the ball's position at a station of the blend did not converge: a defect,
    /// never "no root" (a solve that stops proves nothing about the root).
    /// Its class is [`ErrorKind::StationDidNotConverge`] (125); status `SM_ERR`.
    StationDidNotConverge {
        /// The station: the blend's parameter along its spine.
        s: f64,
        /// The residual the solve stopped at (`None` where the solve does not report it).
        residual: Option<f64>,
        /// The tolerance it had to reach.
        tolerance: f64,
        /// The last step it took (`None` where the solve does not report it).
        step: Option<f64>,
        /// The iterations it took (`None` where the solve does not report them).
        iterations: Option<u32>,
    },
    /// The fillet executive is given no target body, or is run before one is set. A check
    /// of the caller's arguments ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_INVALID_INPUT` (`SM_ERR_NULL_POINTER` where a stage finds it unset).
    TargetNotGiven,
    /// The fillet trace failed, for the reason the [`TraceFailure`] names: the standard and
    /// custom fillet solvers, the fillet intersector and the law corners. Its class and
    /// status are the cause's ([`TraceFailure::class`]; the status as each [`TraceFailure`]
    /// variant says).
    Trace(TraceFailure),
    /// The collection of the faces the rails enclose reached past a rail (the trim leaks):
    /// the failure's site face, where the check names it, is the face it reached. A defect
    /// ([`ErrorKind::TrimLeaks`]).
    /// Status `SM_ERR`.
    TrimLoopDoesNotSeparate,
    /// The boundary loop of the faces the rails enclose is open: the failure's site vertex
    /// is where. A defect ([`ErrorKind::TrimLeaks`]).
    /// Status `SM_ERR`.
    TrimLoopOpen,
    /// The start or end (or both) of a fillet edge's UV trim curve does not drop onto the
    /// surface: neither the intersecting drops, at seven tolerances growing tenfold from
    /// a billionth of the surface's size, nor the drop that minimises the distance within
    /// `tolerance` found a point. An empty search ([`ErrorKind::NotFound`]); status
    /// `SM_ERR`.
    UvCurveEndOffSurface {
        /// True when the start found no point.
        start: bool,
        /// True when the end found no point.
        end: bool,
        /// The tolerance of the drop that minimises the distance.
        tolerance: f64,
    },
    /// The ends of a fillet edge's UV trim curve drop onto the surface `start_solutions`
    /// and `end_solutions` times (after the solutions within the parameter tolerance of
    /// the first are merged) in a way the curve is not built for: both more than twice,
    /// or one more than twice at points that differ in both parameters (not on a
    /// degenerate side). A missing construction ([`ErrorKind::Unsupported`]); status
    /// `SM_ERR`.
    UvCurveEndsUnresolved {
        /// The solutions of the start.
        start_solutions: usize,
        /// The solutions of the end.
        end_solutions: usize,
    },
    /// The surface a fillet edge's UV trim curve is made on (the surface of face `face`,
    /// where the caller names one) is of kind `kind` (its type string), which is not of a
    /// B-spline class: a trim curve between two points off a common iso-curve is the
    /// surface's section by a plane, made on its B-spline net, and this kind has none. An
    /// unsupported kind ([`ErrorKind::Unsupported`]); status `SM_ERR_NULL_POINTER` (the
    /// `NER` of the cast it replaces).
    UvCurveSurfaceNotBSpline {
        /// The face whose surface it is, where the caller names one.
        face: Option<crate::topology::FaceId>,
        /// The surface's kind, as its type string.
        kind: &'static str,
    },
    /// The edge at `index` of the edges given has a variable radius and no radius law. A
    /// check of the caller's arguments ([`ErrorKind::InvalidInput`]); status
    /// `SM_ERR_INVALID_INPUT`.
    VariableRadiusLawMissing {
        /// The edge's index in the edges given.
        index: usize,
    },
    /// A variable radius is asked and no list of radius laws is given. A check of the
    /// caller's arguments ([`ErrorKind::InvalidInput`]); status `SM_ERR_NULL_POINTER`
    /// (upstream dereferences the list).
    VariableRadiusLawsNotGiven,
    /// A weld bead the kernel does not build yet (#15129, [`crate::blend::weld_bead`]), for
    /// the gap named: not a fact of the geometry and not a defect, so no size is named. Its
    /// class is [`ErrorKind::Unsupported`]. Not upstream.
    /// Status `SM_ERR`.
    WeldNotBuilt(WeldGap),
}

impl BlendFailure {
    /// The entity the failure names, if any: the entity of a
    /// [`BlendFailure::CornerInputInvalid`] where its step has one.
    #[must_use]
    pub const fn entity(&self) -> Option<EntityRef> {
        match self {
            Self::CornerInputInvalid { entity, .. } => *entity,
            _ => None,
        }
    }

    /// The status the C++ reports for the failure unless its site says otherwise (a
    /// comparison detail).
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::ArcLengthNotConverged { .. } => SM_ERR_NOT_CONVERGING,
            Self::BadIntersections(_) => SM_ERR_BAD_INTERSECTIONS,
            Self::BallTriangleNotOnePatch { .. } => SM_ERR,
            Self::BallsNest { .. } => SM_ERR,
            Self::BandInputMissing(_) => SM_ERR_NULL_POINTER,
            Self::BevelVertexNotFilleted => SM_ERR_NULL_POINTER,
            Self::BoundSurfaceRemoved => SM_ERR_NULL_POINTER,
            Self::ChamferFaceNotOfEdge => SM_ERR_INVALID_INPUT,
            Self::ChamferSizeInvalid => SM_ERR_INVALID_INPUT,
            Self::CliffInputMissing { .. } => SM_ERR_NULL_POINTER,
            Self::ClosedCliffWouldNotSplit { .. } => SM_ERR,
            Self::CornerCurvesDoNotMeet => SM_ERR,
            Self::CornerCurvesMeetMoreThanOnce { .. } => SM_ERR,
            Self::CornerInputInvalid { input, .. } => input.upstream_status(),
            Self::CornerPatchFaceNotFound { .. } => SM_ERR_NULL_POINTER,
            Self::CornerPatchNotMade(miss) => miss.upstream_status(),
            Self::CornerPointDropUnresolved { .. } => SM_ERR,
            Self::CornerRefused => SM_ERR,
            Self::CornerShapeNotBuilt(shape) => shape.upstream_status(),
            Self::CornerSurfacesNotIntersected => SM_ERR,
            Self::CornerUnbuildable(_) => SM_ERR,
            Self::CreasePieceEndUnbuildable { .. } => SM_ERR,
            Self::CreaseTrimNotBuilt(_) => SM_ERR,
            Self::CrossSectionTypeUnknown => SM_ERR_INVALID_INPUT,
            Self::CrossingInputMissing(_) => SM_ERR_NULL_POINTER,
            Self::CrossingNotFinite => SM_ERR,
            Self::CurveWithoutNet { .. } => SM_ERR,
            Self::Defect { .. } => SM_ERR,
            Self::EaseGeometryNotBSpline(_) => SM_ERR_NULL_POINTER,
            Self::EaseInputMissing(_) => SM_ERR_NULL_POINTER,
            Self::EaseNotBuilt(_) => SM_ERR,
            Self::EdgeNotFilletable { .. } => SM_ERR_INVALID_INPUT,
            Self::EdgeNotInTarget { .. } => SM_ERR_INVALID_INPUT,
            Self::EdgeSplitDeclined { .. } => SM_ERR,
            Self::EdgeUseWithoutEdge => SM_ERR_NULL_POINTER,
            Self::EdgeWithoutCurve => SM_ERR_NULL_POINTER,
            Self::ExactBlendAxisCannotPlaceEdge { .. } => SM_ERR,
            Self::ExecutiveStepFailed => SM_ERR,
            Self::ExtensionEndOnTwoSideLines => SM_ERR,
            Self::FaceHasNoSurface => SM_ERR_INVALID_INPUT,
            Self::Failed(_) => SM_ERR,
            Self::FilletCrossesItself { .. } => SM_ERR,
            Self::FilletSetUpInvalid(_) => SM_ERR_NULL_POINTER,
            Self::FilletVertexWithoutCorner => SM_ERR_NULL_POINTER,
            Self::GnSectionInvalid { .. } => SM_ERR_INVALID_INPUT,
            Self::Inconsistent(_) => SM_ERR_ASSERT_FAILURE,
            Self::InterpolantDropsToCurves { .. } => SM_ERR,
            Self::IntersectionCurveHasNoUvCurve { .. } => SM_ERR,
            Self::IntersectionCurveNotBSpline { .. } => SM_ERR,
            Self::IntersectionEndsOffFace { .. } => SM_ERR,
            Self::InvalidValue(_) => SM_ERR_INVALID_INPUT,
            Self::JoinBlendsRollOffSection => SM_ERR,
            Self::JoinCurveNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::JoinSectionNotConverged { .. } => SM_ERR,
            Self::KeptFeatureNotBuilt(_) => SM_ERR,
            Self::LawCornerNotFound { .. } => SM_ERR_NULL_POINTER,
            Self::LawHasNoValue => SM_ERR,
            Self::LawIntervalEmpty => SM_ERR,
            Self::MateFaceSurfaceNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::Missing(_) => SM_ERR_NULL_POINTER,
            Self::MixedCornerUnbuildable(_) => SM_ERR,
            Self::NeighbourVertexNotProcessed { .. } => SM_ERR,
            Self::NoBlendedEdgeNearExtension => SM_ERR,
            Self::NoEdgesGiven => SM_ERR_INVALID_INPUT,
            Self::NoFirstBody => SM_ERR_INVALID_INPUT,
            Self::NoRoom(_) => SM_ERR,
            Self::NoVertexGiven => SM_ERR_NULL_POINTER,
            Self::NotCertified { .. } => SM_ERR,
            Self::NotConverged(_) => SM_ERR_NOT_CONVERGING,
            Self::NotWithinTolerance(_) => SM_ERR_NOT_WITHIN_TOLERANCE,
            Self::OutsideDomain(_) => SM_ERR_OUTSIDE_OF_DOMAIN,
            Self::OverflowInputMissing(_) => SM_ERR_NULL_POINTER,
            Self::OverflowPointDoesNotDrop(_) => SM_ERR,
            Self::ParameterListSizes { .. } => SM_ERR_INVALID_INPUT,
            Self::PassedOn { status, .. } => *status,
            Self::PerEdgeListLength { .. } => SM_ERR_INVALID_INPUT,
            Self::PlaneCurveEndOff { .. } => SM_ERR,
            Self::PlaneCurveFitCap { .. } => SM_ERR,
            Self::PlaneCurveLeavesDomain { .. } => SM_ERR,
            Self::PlaneCurveStepVanished { .. } => SM_ERR,
            Self::PlaneCurveTouches { .. } => SM_ERR,
            Self::RadiusLawNotPositive { .. } => SM_ERR,
            Self::RadiusTypeNotForEdges { .. } => SM_ERR_INVALID_INPUT,
            Self::RailCrossingNotConverged { .. } => SM_ERR,
            Self::RailEndMissesSideEdge { .. } => SM_ERR,
            Self::RailMissesEndSurface { .. } => SM_ERR,
            Self::RailNotBlending { .. } => SM_ERR,
            Self::RemovalRefused(_) => SM_ERR,
            Self::SeamCornersBothCut => SM_ERR,
            Self::SeamCutOffRib { .. } => SM_ERR,
            Self::SeamSurfaceNotClosed => SM_ERR,
            Self::SectionDegenerate(_) => SM_ERR,
            Self::SectionInputInvalid(_) => SM_ERR,
            Self::SectionNotProved(_) => SM_ERR,
            Self::SectionUnsupported(_) => SM_ERR,
            Self::SequenceCurveNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::SetBackFootNotUnique { .. } => SM_ERR,
            Self::SideEdgeChainPastBound { .. } => SM_ERR,
            Self::SizeBelowResolution { .. } => SM_ERR_INVALID_INPUT,
            Self::SkinMeasureNotFinite { .. } => SM_ERR,
            Self::SkinPastStationCap { .. } => SM_ERR,
            Self::Solver(s) => s.comparison_status(),
            Self::SolverFaceIndexInvalid { .. } => SM_ERR_INVALID_INPUT,
            Self::SplitFaceNotGiven => SM_ERR_NULL_POINTER,
            Self::SrfSrfHeadingNotFollowed => SM_ERR,
            Self::SrfSrfNoCurveThroughEnds { .. } => SM_ERR,
            Self::StageFailed => SM_ERR,
            Self::StationDidNotConverge { .. } => SM_ERR,
            Self::TargetNotGiven => SM_ERR_INVALID_INPUT,
            Self::Trace(t) => t.comparison_status(),
            Self::TrimLoopDoesNotSeparate => SM_ERR,
            Self::TrimLoopOpen => SM_ERR,
            Self::UvCurveEndOffSurface { .. } => SM_ERR,
            Self::UvCurveEndsUnresolved { .. } => SM_ERR,
            Self::UvCurveSurfaceNotBSpline { .. } => SM_ERR_NULL_POINTER,
            Self::VariableRadiusLawMissing { .. } => SM_ERR_INVALID_INPUT,
            Self::VariableRadiusLawsNotGiven => SM_ERR_NULL_POINTER,
            Self::WeldNotBuilt(_) => SM_ERR,
        }
    }

    /// The failure's class, its stable [`ErrorKind`], derived from the cause: see each
    /// variant. A phrase cause keeps the kind of its status ([`ErrorKind::Failed`],
    /// [`ErrorKind::Missing`], [`ErrorKind::InvalidInput`], ...), as before the record
    /// ([`BlendFailure::is_phrase`]).
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn class(&self) -> ErrorKind {
        match self {
            Self::ArcLengthNotConverged { .. } => ErrorKind::NotConverged,
            Self::BadIntersections(_) => ErrorKind::of_status(self.comparison_status()),
            Self::BallTriangleNotOnePatch { .. } => ErrorKind::Unsupported,
            Self::BallsNest { .. } => ErrorKind::BallsFold,
            Self::BandInputMissing(_) => ErrorKind::Missing,
            Self::BevelVertexNotFilleted => ErrorKind::InvalidInput,
            Self::BoundSurfaceRemoved => ErrorKind::InvalidInput,
            Self::ChamferFaceNotOfEdge => ErrorKind::InvalidInput,
            Self::ChamferSizeInvalid => ErrorKind::InvalidInput,
            Self::CliffInputMissing { .. } => ErrorKind::Missing,
            Self::ClosedCliffWouldNotSplit { .. } => ErrorKind::Failed,
            Self::CornerCurvesDoNotMeet => ErrorKind::NotFound,
            Self::CornerCurvesMeetMoreThanOnce { .. } => ErrorKind::Unsupported,
            Self::CornerInputInvalid { .. } => ErrorKind::InvalidInput,
            Self::CornerPatchFaceNotFound { .. } => ErrorKind::NotFound,
            Self::CornerPatchNotMade(miss) => miss.class(),
            Self::CornerPointDropUnresolved { .. } => ErrorKind::NotFound,
            Self::CornerRefused => ErrorKind::Failed,
            Self::CornerShapeNotBuilt(_) => ErrorKind::Unsupported,
            Self::CornerSurfacesNotIntersected => ErrorKind::Failed,
            Self::CornerUnbuildable(step) => step.class(),
            Self::CreasePieceEndUnbuildable { .. } => ErrorKind::Unsupported,
            Self::CreaseTrimNotBuilt(stop) => stop.class(),
            Self::CrossSectionTypeUnknown => ErrorKind::InvalidInput,
            Self::CrossingInputMissing(_) => ErrorKind::Missing,
            Self::CrossingNotFinite => ErrorKind::StageFailed,
            Self::CurveWithoutNet { .. } => ErrorKind::Unsupported,
            Self::Defect { .. } => ErrorKind::StageFailed,
            Self::EaseGeometryNotBSpline(_) => ErrorKind::Unsupported,
            Self::EaseInputMissing(_) => ErrorKind::Missing,
            Self::EaseNotBuilt(stop) => stop.class(),
            Self::EdgeNotFilletable { .. } => ErrorKind::InvalidInput,
            Self::EdgeNotInTarget { .. } => ErrorKind::InvalidInput,
            Self::EdgeSplitDeclined { .. } => ErrorKind::Failed,
            Self::EdgeUseWithoutEdge => ErrorKind::InvalidInput,
            Self::EdgeWithoutCurve => ErrorKind::InvalidInput,
            Self::ExactBlendAxisCannotPlaceEdge { .. } => ErrorKind::InvalidInput,
            Self::ExecutiveStepFailed => ErrorKind::Failed,
            Self::ExtensionEndOnTwoSideLines => ErrorKind::Unsupported,
            Self::FaceHasNoSurface => ErrorKind::InvalidInput,
            Self::Failed(_) => ErrorKind::of_status(self.comparison_status()),
            Self::FilletCrossesItself { .. } => ErrorKind::Unsupported,
            Self::FilletSetUpInvalid(_) => ErrorKind::InvalidInput,
            Self::FilletVertexWithoutCorner => ErrorKind::InvalidInput,
            Self::GnSectionInvalid { .. } => ErrorKind::InvalidInput,
            Self::Inconsistent(_) => ErrorKind::of_status(self.comparison_status()),
            Self::InterpolantDropsToCurves { .. } => ErrorKind::Unsupported,
            Self::IntersectionCurveHasNoUvCurve { .. } => ErrorKind::Unsupported,
            Self::IntersectionCurveNotBSpline { .. } => ErrorKind::Unsupported,
            Self::IntersectionEndsOffFace { .. } => ErrorKind::NotFound,
            Self::InvalidValue(_) => ErrorKind::of_status(self.comparison_status()),
            Self::JoinBlendsRollOffSection => ErrorKind::Unsupported,
            Self::JoinCurveNotBSpline { .. } => ErrorKind::Unsupported,
            Self::JoinSectionNotConverged { .. } => ErrorKind::NotConverged,
            Self::KeptFeatureNotBuilt(stop) => stop.class(),
            Self::LawCornerNotFound { .. } => ErrorKind::NotFound,
            Self::LawHasNoValue => ErrorKind::Unsupported,
            Self::LawIntervalEmpty => ErrorKind::InvalidInput,
            Self::MateFaceSurfaceNotBSpline { .. } => ErrorKind::Unsupported,
            Self::Missing(_) => ErrorKind::of_status(self.comparison_status()),
            Self::MixedCornerUnbuildable(step) => step.class(),
            Self::NeighbourVertexNotProcessed { .. } => ErrorKind::InvalidInput,
            Self::NoBlendedEdgeNearExtension => ErrorKind::NotFound,
            Self::NoEdgesGiven => ErrorKind::InvalidInput,
            Self::NoFirstBody => ErrorKind::InvalidInput,
            Self::NoRoom(reason) => no_room_class(*reason),
            Self::NoVertexGiven => ErrorKind::InvalidInput,
            Self::NotCertified { .. } => ErrorKind::NotWithinTolerance,
            Self::NotConverged(_) => ErrorKind::of_status(self.comparison_status()),
            Self::NotWithinTolerance(_) => ErrorKind::of_status(self.comparison_status()),
            Self::OutsideDomain(_) => ErrorKind::of_status(self.comparison_status()),
            Self::OverflowInputMissing(_) => ErrorKind::Missing,
            Self::OverflowPointDoesNotDrop(_) => ErrorKind::NotFound,
            Self::ParameterListSizes { .. } => ErrorKind::InvalidInput,
            Self::PassedOn { .. } => ErrorKind::of_status(self.comparison_status()),
            Self::PerEdgeListLength { .. } => ErrorKind::InvalidInput,
            Self::PlaneCurveEndOff { .. } => ErrorKind::NotFound,
            Self::PlaneCurveFitCap { .. } => ErrorKind::NotWithinTolerance,
            Self::PlaneCurveLeavesDomain { .. } => ErrorKind::OutsideDomain,
            Self::PlaneCurveStepVanished { .. } => ErrorKind::StageFailed,
            Self::PlaneCurveTouches { .. } => ErrorKind::Degenerate,
            Self::RadiusLawNotPositive { .. } => ErrorKind::InvalidInput,
            Self::RadiusTypeNotForEdges { .. } => ErrorKind::InvalidInput,
            Self::RailCrossingNotConverged { .. } => ErrorKind::NotConverged,
            Self::RailEndMissesSideEdge { .. } => ErrorKind::NotFound,
            Self::RailMissesEndSurface { .. } => ErrorKind::NotFound,
            Self::RailNotBlending { .. } => ErrorKind::InvalidInput,
            Self::RemovalRefused(reason) => removal_class(*reason),
            Self::SeamCornersBothCut => ErrorKind::Unsupported,
            Self::SeamCutOffRib { .. } => ErrorKind::NotFound,
            Self::SeamSurfaceNotClosed => ErrorKind::InvalidInput,
            Self::SectionDegenerate(_) => ErrorKind::Degenerate,
            Self::SectionInputInvalid(_) => ErrorKind::InvalidInput,
            Self::SectionNotProved(_) => ErrorKind::NotWithinTolerance,
            Self::SectionUnsupported(_) => ErrorKind::Unsupported,
            Self::SequenceCurveNotBSpline { .. } => ErrorKind::Unsupported,
            Self::SetBackFootNotUnique { .. } => ErrorKind::Unsupported,
            Self::SideEdgeChainPastBound { .. } => ErrorKind::Unsupported,
            Self::SizeBelowResolution { .. } => ErrorKind::InvalidInput,
            Self::SkinMeasureNotFinite { .. } => ErrorKind::StageFailed,
            Self::SkinPastStationCap { .. } => ErrorKind::NotWithinTolerance,
            Self::Solver(s) => s.class(),
            Self::SolverFaceIndexInvalid { .. } => ErrorKind::InvalidInput,
            Self::SplitFaceNotGiven => ErrorKind::InvalidInput,
            Self::SrfSrfHeadingNotFollowed => ErrorKind::NotFound,
            Self::SrfSrfNoCurveThroughEnds { .. } => ErrorKind::NotFound,
            Self::StageFailed => ErrorKind::StageFailed,
            Self::StationDidNotConverge { .. } => ErrorKind::StationDidNotConverge,
            Self::TargetNotGiven => ErrorKind::InvalidInput,
            Self::Trace(t) => t.class(),
            Self::TrimLoopDoesNotSeparate => ErrorKind::TrimLeaks,
            Self::TrimLoopOpen => ErrorKind::TrimLeaks,
            Self::UvCurveEndOffSurface { .. } => ErrorKind::NotFound,
            Self::UvCurveEndsUnresolved { .. } => ErrorKind::Unsupported,
            Self::UvCurveSurfaceNotBSpline { .. } => ErrorKind::Unsupported,
            Self::VariableRadiusLawMissing { .. } => ErrorKind::InvalidInput,
            Self::VariableRadiusLawsNotGiven => ErrorKind::InvalidInput,
            Self::WeldNotBuilt(_) => ErrorKind::Unsupported,
        }
    }

    /// True for a phrase cause ([`BlendFailure::Failed`] to [`BlendFailure::PassedOn`]): a
    /// site not yet converted to the record. Its class is its status's kind, as before the
    /// record, and it reads as neither a defect nor a refusal
    /// ([`crate::SolidError::is_defect`], [`crate::SolidError::is_refusal`]) until it moves to
    /// a typed cause.
    #[must_use]
    pub const fn is_phrase(&self) -> bool {
        phrase_causes!(matches_phrase_cause(self))
    }

    /// True when the failure stands in for a bare status its site returned (an
    /// [`crate::SolidError::Upstream`] before solidcore described it): the decisions that
    /// read an upstream status ([`crate::error::upstream_status`]) treat it as that status,
    /// and the goldens read it. Every cause does: the status stays reachable for the goldens
    /// while the cause, not the status, decides the type. [`BlendFailure::NotCertified`]
    /// stands for the fork's `SM_ERR` at its certificate's stop (and, past the work budget
    /// solidcore adds, for the same stop reached sooner).
    #[must_use]
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    pub const fn replaces_upstream_status(&self) -> bool {
        match self {
            Self::ArcLengthNotConverged { .. } => true,
            Self::BadIntersections(_) => true,
            Self::BallTriangleNotOnePatch { .. } => true,
            Self::BallsNest { .. } => true,
            Self::BandInputMissing(_) => true,
            Self::BevelVertexNotFilleted => true,
            Self::BoundSurfaceRemoved => true,
            Self::ChamferFaceNotOfEdge => true,
            Self::ChamferSizeInvalid => true,
            Self::CliffInputMissing { .. } => true,
            Self::ClosedCliffWouldNotSplit { .. } => true,
            Self::CornerCurvesDoNotMeet => true,
            Self::CornerCurvesMeetMoreThanOnce { .. } => true,
            Self::CornerInputInvalid { .. } => true,
            Self::CornerPatchFaceNotFound { .. } => true,
            Self::CornerPatchNotMade(_) => true,
            Self::CornerPointDropUnresolved { .. } => true,
            Self::CornerRefused => true,
            Self::CornerShapeNotBuilt(_) => true,
            Self::CornerSurfacesNotIntersected => true,
            Self::CornerUnbuildable(_) => true,
            Self::CreasePieceEndUnbuildable { .. } => true,
            Self::CreaseTrimNotBuilt(_) => true,
            Self::CrossSectionTypeUnknown => true,
            Self::CrossingInputMissing(_) => true,
            Self::CrossingNotFinite => true,
            Self::CurveWithoutNet { .. } => true,
            Self::Defect { .. } => true,
            Self::EaseGeometryNotBSpline(_) => true,
            Self::EaseInputMissing(_) => true,
            Self::EaseNotBuilt(_) => true,
            Self::EdgeNotFilletable { .. } => true,
            Self::EdgeNotInTarget { .. } => true,
            Self::EdgeSplitDeclined { .. } => true,
            Self::EdgeUseWithoutEdge => true,
            Self::EdgeWithoutCurve => true,
            Self::ExactBlendAxisCannotPlaceEdge { .. } => true,
            Self::ExecutiveStepFailed => true,
            Self::ExtensionEndOnTwoSideLines => true,
            Self::FaceHasNoSurface => true,
            Self::Failed(_) => true,
            Self::FilletCrossesItself { .. } => true,
            Self::FilletSetUpInvalid(_) => true,
            Self::FilletVertexWithoutCorner => true,
            Self::GnSectionInvalid { .. } => true,
            Self::Inconsistent(_) => true,
            Self::InterpolantDropsToCurves { .. } => true,
            Self::IntersectionCurveHasNoUvCurve { .. } => true,
            Self::IntersectionCurveNotBSpline { .. } => true,
            Self::IntersectionEndsOffFace { .. } => true,
            Self::InvalidValue(_) => true,
            Self::JoinBlendsRollOffSection => true,
            Self::JoinCurveNotBSpline { .. } => true,
            Self::JoinSectionNotConverged { .. } => true,
            Self::KeptFeatureNotBuilt(_) => true,
            Self::LawCornerNotFound { .. } => true,
            Self::LawHasNoValue => true,
            Self::LawIntervalEmpty => true,
            Self::MateFaceSurfaceNotBSpline { .. } => true,
            Self::Missing(_) => true,
            Self::MixedCornerUnbuildable(_) => true,
            Self::NeighbourVertexNotProcessed { .. } => true,
            Self::NoBlendedEdgeNearExtension => true,
            Self::NoEdgesGiven => true,
            Self::NoFirstBody => true,
            Self::NoRoom(_) => true,
            Self::NoVertexGiven => true,
            Self::NotCertified { .. } => true,
            Self::NotConverged(_) => true,
            Self::NotWithinTolerance(_) => true,
            Self::OutsideDomain(_) => true,
            Self::OverflowInputMissing(_) => true,
            Self::OverflowPointDoesNotDrop(_) => true,
            Self::ParameterListSizes { .. } => true,
            Self::PassedOn { .. } => true,
            Self::PerEdgeListLength { .. } => true,
            Self::PlaneCurveEndOff { .. } => true,
            Self::PlaneCurveFitCap { .. } => true,
            Self::PlaneCurveLeavesDomain { .. } => true,
            Self::PlaneCurveStepVanished { .. } => true,
            Self::PlaneCurveTouches { .. } => true,
            Self::RadiusLawNotPositive { .. } => true,
            Self::RadiusTypeNotForEdges { .. } => true,
            Self::RailCrossingNotConverged { .. } => true,
            Self::RailEndMissesSideEdge { .. } => true,
            Self::RailMissesEndSurface { .. } => true,
            Self::RailNotBlending { .. } => true,
            Self::RemovalRefused(_) => true,
            Self::SeamCornersBothCut => true,
            Self::SeamCutOffRib { .. } => true,
            Self::SeamSurfaceNotClosed => true,
            Self::SectionDegenerate(_) => true,
            Self::SectionInputInvalid(_) => true,
            Self::SectionNotProved(_) => true,
            Self::SectionUnsupported(_) => true,
            Self::SequenceCurveNotBSpline { .. } => true,
            Self::SetBackFootNotUnique { .. } => true,
            Self::SideEdgeChainPastBound { .. } => true,
            Self::SizeBelowResolution { .. } => true,
            Self::SkinMeasureNotFinite { .. } => true,
            Self::SkinPastStationCap { .. } => true,
            Self::Solver(_) => true,
            Self::SolverFaceIndexInvalid { .. } => true,
            Self::SplitFaceNotGiven => true,
            Self::SrfSrfHeadingNotFollowed => true,
            Self::SrfSrfNoCurveThroughEnds { .. } => true,
            Self::StageFailed => true,
            Self::StationDidNotConverge { .. } => true,
            Self::TargetNotGiven => true,
            Self::Trace(_) => true,
            Self::TrimLoopDoesNotSeparate => true,
            Self::TrimLoopOpen => true,
            Self::UvCurveEndOffSurface { .. } => true,
            Self::UvCurveEndsUnresolved { .. } => true,
            Self::UvCurveSurfaceNotBSpline { .. } => true,
            Self::VariableRadiusLawMissing { .. } => true,
            Self::VariableRadiusLawsNotGiven => true,
            Self::WeldNotBuilt(_) => true,
        }
    }
}

impl core::fmt::Display for BlendFailure {
    /// A typed cause is worded from its class and its numbers; a phrase cause by its phrase.
    #[allow(
        clippy::match_same_arms,
        reason = "one arm per variant, sorted by name, so causes added in parallel touch disjoint lines"
    )]
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Self::ArcLengthNotConverged {
                start,
                length,
                stopped_at,
            } => write!(
                f,
                "{}: the search for the parameter {length} along the curve from {start} stopped at {stopped_at}",
                self.class()
            ),
            Self::BadIntersections(what) => f.write_str(what),
            Self::BallTriangleNotOnePatch { pole } => write!(
                f,
                "{}: the ball corner's triangle is not one rational patch from the pole at contact {pole}",
                self.class()
            ),
            Self::BallsNest { s, rate, speed } => write!(
                f,
                "{}: at s = {s} the balls' radius changes at {rate}, as fast as their centres move ({speed}): no rolling-ball surface exists there",
                self.class()
            ),
            Self::BandInputMissing(input) => write!(f, "{} is missing", input.what()),
            Self::BevelVertexNotFilleted => write!(
                f,
                "{}: a vertex given to bevel is not a vertex of the filleted edges",
                self.class()
            ),
            Self::BoundSurfaceRemoved => write!(
                f,
                "{}: the face's surface was removed after its binding was captured",
                self.class()
            ),
            Self::ChamferFaceNotOfEdge => write!(
                f,
                "{}: the face named for the chamfer's first size is neither of its edge's faces",
                self.class()
            ),
            Self::ChamferSizeInvalid => write!(
                f,
                "{}: the chamfer size is not one the edge can take (sizes and setbacks finite and positive, distances that do not fold, faces that make a wedge, on a knife edge two equal distances)",
                self.class()
            ),
            Self::CliffInputMissing { face, input } => {
                write!(f, "cliff face {face:?} is missing its {}", input.what())
            }
            Self::ClosedCliffWouldNotSplit { face } => write!(
                f,
                "the closed cliff blend face {face:?} does not divide at its interior section"
            ),
            // (the entity is the failure's site, which the failure prints)
            Self::CornerCurvesDoNotMeet => write!(
                f,
                "{}: the corner's two intersection curves do not cross",
                self.class()
            ),
            Self::CornerCurvesMeetMoreThanOnce { crossings } => write!(
                f,
                "{}: the corner's two intersection curves cross {crossings} times, not once",
                self.class()
            ),
            Self::CornerInputInvalid { input, .. } => {
                write!(f, "{}: the fillet corner given: {input:?}", self.class())
            }
            Self::CornerPatchFaceNotFound { surface } => write!(
                f,
                "{}: corner patch surface {surface} has no face in the intermediate body",
                self.class()
            ),
            Self::CornerPatchNotMade(miss) => write!(f, "{}: {miss:?}", self.class()),
            // (the entity is the failure's site, which the failure prints)
            Self::CornerPointDropUnresolved { solutions } => write!(
                f,
                "{}: the corner's vertex drops onto the surface at {solutions} points, not one",
                self.class()
            ),
            Self::CornerRefused => write!(
                f,
                "{}: a corner was refused at its vertex (the error info says why)",
                self.class()
            ),
            Self::CornerShapeNotBuilt(shape) => write!(f, "{}: {shape:?}", self.class()),
            Self::CornerSurfacesNotIntersected => write!(
                f,
                "{}: the intersection of the corner's surfaces failed",
                self.class()
            ),
            Self::CornerUnbuildable(step) => write!(
                f,
                "{}: the corner has no construction: {}",
                self.class(),
                step.what()
            ),
            Self::CreasePieceEndUnbuildable { kind, at_end, t } => write!(
                f,
                "{}: the crease piece's {} at t = {t} cannot be ended on its meet: no construction for a {kind} curve there",
                self.class(),
                if *at_end { "end" } else { "start" },
            ),
            Self::CreaseTrimNotBuilt(stop) => write!(f, "{}: {}", self.class(), stop.what()),
            Self::CrossSectionTypeUnknown => {
                write!(f, "{}: a cross-section type is unknown", self.class())
            }
            Self::CrossingInputMissing(input) => write!(f, "{} is missing", input.what()),
            Self::CrossingNotFinite => write!(
                f,
                "{}: the rail's crossing of an edge is not judged from finite values",
                self.class()
            ),
            Self::CurveWithoutNet { edge, kind } => {
                write!(f, "{}: the curve", self.class())?;
                if let Some(edge) = edge {
                    write!(f, " of edge {edge:?}")?;
                }
                write!(f, " is a {kind} curve, which is not of a B-spline class")
            }
            Self::Defect { why } => write!(f, "{}: {why}", self.class()),
            Self::EaseGeometryNotBSpline(geometry) => {
                write!(f, "{}: {}", self.class(), geometry.what())
            }
            Self::EaseInputMissing(input) => write!(f, "{}: {}", self.class(), input.what()),
            Self::EaseNotBuilt(stop) => write!(f, "{}: {}", self.class(), stop.what()),
            Self::EdgeNotFilletable { index } => write!(
                f,
                "{}: edge {index} of the edges given has no edge use the fillet can roll on",
                self.class()
            ),
            Self::EdgeNotInTarget { index } => write!(
                f,
                "{}: edge {index} of the edges given is not an edge of the target body",
                self.class()
            ),
            Self::EdgeSplitDeclined { parameter } => write!(
                f,
                "{}: the edge is not split at parameter {parameter} (outside the edge or at an end)",
                self.class()
            ),
            Self::EdgeUseWithoutEdge => {
                write!(f, "{}: an edge use of the body has no edge", self.class())
            }
            Self::EdgeWithoutCurve => write!(
                f,
                "{}: an edge of the body the fillet reads has no curve",
                self.class()
            ),
            Self::ExactBlendAxisCannotPlaceEdge {
                length,
                discriminant,
                crossing,
            } => {
                write!(
                    f,
                    "{}: the exact blend given cannot place the edge's middle round the cylinder's or \
                     cone's axis (distance {length}, discriminant {discriminant}",
                    self.class()
                )?;
                if let Some(mu) = crossing {
                    write!(f, ", crossing at {mu}")?;
                }
                write!(f, "), so the blend given does not match this edge")
            }
            Self::ExecutiveStepFailed => write!(
                f,
                "{}: a step of the fillet executive failed (the error info says which)",
                self.class()
            ),
            Self::ExtensionEndOnTwoSideLines => write!(
                f,
                "{}: the end of a rail's run past a face's boundary lies on the lines of two of \
                 the face's side edges",
                self.class()
            ),
            Self::FaceHasNoSurface => {
                write!(f, "{}: a face of the fillet has no surface", self.class())
            }
            Self::Failed(what) => f.write_str(what),
            Self::FilletCrossesItself { first, second } => write!(
                f,
                "{}: the offset surfaces' intersection curves {first} and {second} cross, so the fillet would cross itself",
                self.class()
            ),
            Self::FilletSetUpInvalid(part) => write!(f, "{}: {}", self.class(), part.what()),
            Self::FilletVertexWithoutCorner => write!(
                f,
                "{}: the fillet vertex has no fillet corner",
                self.class()
            ),
            Self::GnSectionInvalid { order, thumbweight } => write!(
                f,
                "{}: a G^n cross-section takes an order of 2 to 4 and a thumbweight below 1, not order {order} and thumbweight {thumbweight}",
                self.class()
            ),
            Self::Inconsistent(what) => write!(f, "a consistency check failed: {what}"),
            Self::InterpolantDropsToCurves { face, curves } => write!(
                f,
                "{}: the curve between the rails' ends drops onto face {face:?} as {curves} UV curves, not one",
                self.class()
            ),
            Self::IntersectionCurveHasNoUvCurve { on_second } => write!(
                f,
                "{}: the intersection curve has no UV curve on the {} surface",
                self.class(),
                if *on_second { "second" } else { "first" },
            ),
            Self::IntersectionCurveNotBSpline { kind } => write!(
                f,
                "{}: the intersection curve is a {kind}, which has no B-spline form",
                self.class()
            ),
            Self::IntersectionEndsOffFace { face } => write!(
                f,
                "{}: neither end of the intersection curve is on face {face:?}",
                self.class()
            ),
            Self::InvalidValue(what) => f.write_str(what),
            Self::JoinBlendsRollOffSection => write!(
                f,
                "{}: no blend of the Join rolls on the two faces its shared section ends on",
                self.class()
            ),
            Self::JoinCurveNotBSpline { kind } => write!(
                f,
                "{}: a {kind} curve to join end to end has no B-spline part",
                self.class()
            ),
            Self::JoinSectionNotConverged { blend } => write!(
                f,
                "{}: the solve for blend {blend}'s ball in the Join's section plane stopped without a ball",
                self.class()
            ),
            Self::KeptFeatureNotBuilt(stop) => write!(f, "{}: {}", self.class(), stop.what()),
            Self::LawCornerNotFound { corner, what } => write!(
                f,
                "{}: law corner {corner} has no {}",
                self.class(),
                what.what()
            ),
            Self::LawHasNoValue => {
                write!(f, "{}: a base radius law has no value of its own", self.class())
            }
            Self::LawIntervalEmpty => write!(
                f,
                "{}: the radius law is evaluated over an interval of zero length",
                self.class()
            ),
            Self::MateFaceSurfaceNotBSpline { face, kind } => write!(
                f,
                "{}: the face {face:?} has a {kind} surface, which is not of a B-spline class: no point or curve is built on it",
                self.class()
            ),
            Self::Missing(what) => write!(f, "{what} is missing"),
            Self::MixedCornerUnbuildable(step) => write!(
                f,
                "{}: the mixed corner has no construction: {}",
                self.class(),
                step.what()
            ),
            Self::NeighbourVertexNotProcessed { vertex } => write!(
                f,
                "{}: the corner's neighbouring fillet vertex {} is not processed yet",
                self.class(),
                vertex.index()
            ),
            Self::NoBlendedEdgeNearExtension => write!(
                f,
                "{}: no blended edge takes the drop of the middle of an extension's chain",
                self.class()
            ),
            Self::NoEdgesGiven => write!(f, "{}: no edges are given to fillet", self.class()),
            Self::NoFirstBody => write!(
                f,
                "{}: a face of the first surface's body is given where the first surface has no body",
                self.class()
            ),
            Self::NoRoom(reason) => write!(f, "{}: {reason:?}", self.class()),
            Self::NoVertexGiven => write!(f, "{}: no vertex given", self.class()),
            Self::NotCertified { halvings, stations } => write!(
                f,
                "the blend could not be certified within its tolerance in the work allowed \
                 ({halvings} spans halved, {stations} stations)"
            ),
            Self::NotConverged(what) => f.write_str(what),
            Self::NotWithinTolerance(what) => f.write_str(what),
            Self::OutsideDomain(what) => f.write_str(what),
            Self::OverflowInputMissing(input) => write!(f, "{}: {}", self.class(), input.what()),
            Self::OverflowPointDoesNotDrop(drop) => {
                write!(f, "{}: {}", self.class(), drop.what())
            }
            Self::ParameterListSizes {
                edges,
                cross_sections,
                radius_types,
                radii,
            } => write!(
                f,
                "{}: the fillet's parameter lists ({cross_sections} cross-section types, {radius_types} radius types, {radii} radii) each take one entry or one per edge, for {edges} edges",
                self.class()
            ),
            Self::PassedOn { what, .. } => f.write_str(what),
            Self::PerEdgeListLength { edges, given } => write!(
                f,
                "{}: a list of one entry per edge holds {given} entries for {edges} edges",
                self.class()
            ),
            Self::PlaneCurveEndOff { at_end, distance } => {
                write!(
                    f,
                    "{}: the crossing's {} is off the surface or the plane",
                    self.class(),
                    if *at_end { "end" } else { "start" },
                )?;
                match distance {
                    Some(distance) => write!(f, " by {distance}"),
                    None => f.write_str(": the drop or projection found no point"),
                }
            }
            Self::PlaneCurveFitCap { stations, passes } => write!(
                f,
                "{}: the crossing's Hermite fit is still off the exact curve by more than a tenth of the tolerance with {stations} stations after {passes} passes",
                self.class()
            ),
            Self::PlaneCurveLeavesDomain { u, v } => write!(
                f,
                "{}: the crossing leaves the surface's domain at (u, v) = ({u}, {v}) before its end",
                self.class()
            ),
            Self::PlaneCurveStepVanished {
                u,
                v,
                step,
                minimum,
            } => write!(
                f,
                "{}: the crossing's step at (u, v) = ({u}, {v}) was halved to {step}, below its minimum {minimum}, without an accepted step",
                self.class()
            ),
            Self::PlaneCurveTouches { u, v } => write!(
                f,
                "{}: the surface touches the plane at (u, v) = ({u}, {v}): the crossing has no direction",
                self.class()
            ),
            Self::RadiusLawNotPositive { s, radius } => write!(
                f,
                "{}: the radius law is {radius} at s = {s}, not positive",
                self.class()
            ),
            Self::RadiusTypeNotForEdges { index, kind } => write!(
                f,
                "{}: edge {index}'s radius type {kind:?} is not one an edge's fillet is set up with",
                self.class()
            ),
            Self::RailCrossingNotConverged { face } => write!(
                f,
                "{}: the local solve of the two rails' crossing on the face {face:?} stopped without finding it",
                self.class()
            ),
            Self::RailEndMissesSideEdge { edge } => write!(
                f,
                "{}: the rail meets the side edge {edge:?} nowhere",
                self.class()
            ),
            Self::RailMissesEndSurface { tolerance } => write!(
                f,
                "{}: the rail does not cross the end face's surface within {tolerance}",
                self.class()
            ),
            Self::RailNotBlending { rail } => write!(
                f,
                "{}: rail {} of the blending geometry is not a blending rail",
                self.class(),
                rail.index()
            ),
            Self::RemovalRefused(reason) => write!(f, "{}: {reason:?}", self.class()),
            Self::SeamCornersBothCut => write!(
                f,
                "{}: neither end of the seam curves is at a corner of the closed surface: both ends cut off",
                self.class()
            ),
            Self::SeamCutOffRib { tolerance } => write!(
                f,
                "{}: the seam's cut end does not drop onto the seam's rib within {tolerance}",
                self.class()
            ),
            Self::SeamSurfaceNotClosed => write!(
                f,
                "{}: the surface whose seam curves are asked for is not closed",
                self.class()
            ),
            Self::SectionDegenerate(what) => write!(f, "{}: {}", self.class(), what.what()),
            Self::SectionInputInvalid(what) => write!(f, "{}: {}", self.class(), what.what()),
            Self::SectionNotProved(what) => write!(
                f,
                "{}: the section's certificate did not prove {}",
                self.class(),
                what.what()
            ),
            Self::SectionUnsupported(limit) => match limit {
                SectionLimit::CurveWeightNotPositive => {
                    write!(f, "{}: a curve's weights are not positive", self.class())
                }
                SectionLimit::DefinitionFaceNotOneRadius => write!(
                    f,
                    "{}: only a round of one radius between NURBS faces is certified by its definition",
                    self.class()
                ),
                SectionLimit::DegreeBelowOne => {
                    write!(f, "{}: a curve or face is of degree 0", self.class())
                }
                SectionLimit::FaceNotNurbs => write!(
                    f,
                    "{}: a face is not a NURBS surface: its section of order 2 and up cannot be built or certified",
                    self.class()
                ),
                SectionLimit::FaceWeightNotPositive => {
                    write!(f, "{}: a face's weights are not positive", self.class())
                }
                SectionLimit::OrderPastSeries { order, most } => write!(
                    f,
                    "{}: the order {order} is past the most the series hold ({most})",
                    self.class()
                ),
            },
            Self::SequenceCurveNotBSpline { kind } => write!(
                f,
                "{}: a {kind} curve of a smooth sequence of filleted edges has no B-spline form to join",
                self.class()
            ),
            Self::SetBackFootNotUnique { feet } => write!(
                f,
                "{}: the set-back section's point has {feet} closest points on the offset surface, not one",
                self.class()
            ),
            Self::SideEdgeChainPastBound { steps } => write!(
                f,
                "{}: the rail's end was not found within {steps} side edges",
                self.class()
            ),
            Self::SizeBelowResolution { size, tolerance } => write!(
                f,
                "{}: the size {size} is below eight times the body's tolerance {tolerance}, the smallest a fillet builds",
                self.class()
            ),
            Self::SkinMeasureNotFinite { s } => write!(
                f,
                "{}: the skin check computed a value that is not finite at s = {s}",
                self.class()
            ),
            Self::SkinPastStationCap {
                asked,
                reached,
                stations,
                rounds,
            } => write!(
                f,
                "{}: the round's skin is {reached} from its balls, past {asked}, with {stations} stations after {rounds} rounds",
                self.class()
            ),
            Self::Solver(cause) => cause.fmt(f),
            Self::SolverFaceIndexInvalid { index } => write!(
                f,
                "{}: a fillet solver's faces are its first (0) and second (1), not face {index}",
                self.class()
            ),
            Self::SplitFaceNotGiven => write!(
                f,
                "{}: the face that was split or the new face was not given",
                self.class()
            ),
            Self::SrfSrfHeadingNotFollowed => write!(
                f,
                "{}: no intersection curve through the points leaves the start along the heading given",
                self.class()
            ),
            Self::SrfSrfNoCurveThroughEnds { candidates } => write!(
                f,
                "{}: none of the {candidates} intersection curves passes through the end points",
                self.class()
            ),
            Self::StageFailed => write!(f, "{}", self.class()),
            Self::StationDidNotConverge {
                s,
                residual,
                tolerance,
                step,
                iterations,
            } => {
                let shown = |x: Option<String>| x.unwrap_or_else(|| "unreported".into());
                write!(
                    f,
                    "{}: the station at s = {s} did not converge: residual {} > {tolerance} after {} iterations (last step {})",
                    self.class(),
                    shown(residual.map(|r| r.to_string())),
                    shown(iterations.map(|n| n.to_string())),
                    shown(step.map(|d| d.to_string())),
                )
            }
            Self::TargetNotGiven => {
                write!(f, "{}: the fillet has no target body", self.class())
            }
            Self::Trace(t) => write!(f, "{}: {t}", t.class()),
            Self::TrimLoopDoesNotSeparate => {
                write!(
                    f,
                    "{}: the faces the rails enclose reach past the rails (the trim loop does not separate)",
                    self.class()
                )
            }
            Self::TrimLoopOpen => write!(f, "{}: the trim loop is open", self.class()),
            Self::UvCurveEndOffSurface {
                start,
                end,
                tolerance,
            } => write!(
                f,
                "{}: the UV trim curve's {} not drop onto the surface (last tolerance {tolerance})",
                self.class(),
                match (start, end) {
                    (true, true) => "start and end do",
                    (true, false) => "start does",
                    _ => "end does",
                },
            ),
            Self::UvCurveEndsUnresolved {
                start_solutions,
                end_solutions,
            } => write!(
                f,
                "{}: the UV trim curve's ends drop onto the surface {start_solutions} and {end_solutions} times, which no curve is built for",
                self.class()
            ),
            Self::UvCurveSurfaceNotBSpline { face, kind } => {
                write!(f, "{}: the UV curve's surface", self.class())?;
                if let Some(face) = face {
                    write!(f, " of face {face:?}")?;
                }
                write!(f, " is a {kind} surface, which is not of a B-spline class")
            }
            Self::VariableRadiusLawMissing { index } => write!(
                f,
                "{}: edge {index} of the edges given has a variable radius and no radius law",
                self.class()
            ),
            Self::VariableRadiusLawsNotGiven => write!(
                f,
                "{}: a variable radius is asked and no radius laws are given",
                self.class()
            ),
            Self::WeldNotBuilt(WeldGap::BeadsMeetAtVertex) => write!(
                f,
                "{}: weld beads that meet at a vertex are not built yet (their corner, #17916)",
                self.class()
            ),
            Self::WeldNotBuilt(WeldGap::CurvedJoint) => write!(
                f,
                "{}: a weld bead is built only along a straight edge between two planes so far",
                self.class()
            ),
            Self::WeldNotBuilt(WeldGap::ObtuseToe) => write!(
                f,
                "{}: a weld bead with an obtuse toe (one leg at most the other's cosine of the angle between the faces) is not built yet",
                self.class()
            ),
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for a blend routine that fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend(operation: &'static str, failure: BlendFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// [`crate::SolidError::Failed`] for a blend routine that fails with `failure` about
    /// `entity` (where the bare status it replaces named one).
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_at(
        operation: &'static str,
        failure: BlendFailure,
        entity: EntityRef,
    ) -> Self {
        Failure::new(operation, failure).at(entity).into_error()
    }

    /// [`crate::SolidError::Failed`] for a blend routine that could not continue, as the
    /// phrase says, with status [`SM_ERR`] (upstream `return SE(SM_ERR)`): a phrase
    /// cause.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_failed(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::Failed(what))
    }

    /// [`crate::SolidError::Failed`] for something a blend routine needs that is not there,
    /// as the phrase says, with status [`SM_ERR_NULL_POINTER`] (upstream `NER(ptr)` on a
    /// `NULL` pointer): a phrase cause.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_missing(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::Missing(what))
    }

    /// [`crate::SolidError::Failed`] for an input of the planar-band contact query that is
    /// not there ([`BlendFailure::BandInputMissing`]), with status [`SM_ERR_NULL_POINTER`].
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_band_input_missing(operation: &'static str, input: BandInput) -> Self {
        Self::blend(operation, BlendFailure::BandInputMissing(input))
    }

    /// [`crate::SolidError::Failed`] for an input of a rail crossing's judgement that is not
    /// there ([`BlendFailure::CrossingInputMissing`]), with status [`SM_ERR_NULL_POINTER`].
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_crossing_input_missing(
        operation: &'static str,
        input: CrossingInput,
    ) -> Self {
        Self::blend(operation, BlendFailure::CrossingInputMissing(input))
    }

    /// [`crate::SolidError::Failed`] for a value a blend routine is given or computes with
    /// that is not usable, as the phrase says, with status [`SM_ERR_INVALID_INPUT`]
    /// (upstream `return(SM_ERR_INVALID_INPUT)`): a phrase cause.
    #[cfg(test)]
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_invalid_value(operation: &'static str, what: &'static str) -> Self {
        Self::blend(operation, BlendFailure::InvalidValue(what))
    }

    /// [`crate::SolidError::Failed`] for a fillet solver failure ([`BlendFailure::Solver`]),
    /// with the cause's status.
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_solver(operation: &'static str, failure: SolverFailure) -> Self {
        Self::blend(operation, BlendFailure::Solver(failure))
    }

    /// [`crate::SolidError::Failed`] for a value the kernel built itself and then did not
    /// find (upstream `NER(ptr)` on a `NULL` pointer): a defect, [`BlendFailure::Defect`]
    /// with `why`, keeping the status [`SM_ERR_NULL_POINTER`].
    #[must_use]
    #[track_caller]
    pub(crate) fn blend_null_defect(operation: &'static str, why: &'static str) -> Self {
        Failure::new(operation, BlendFailure::Defect { why })
            .with_status(SM_ERR_NULL_POINTER)
            .into_error()
    }

    /// The blend failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Blend`].
    #[must_use]
    pub fn blend_failure(&self) -> Option<&BlendFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Blend(b) => Some(b),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::blend::fillet_exact::BlendSpine;
    use crate::SolidError;

    #[test]
    fn blend_failures_keep_the_status_and_kind_of_the_upstream_they_replace() {
        let cases = [
            (BlendFailure::Failed("x"), SM_ERR),
            (BlendFailure::Missing("x"), SM_ERR_NULL_POINTER),
            (BlendFailure::InvalidValue("x"), SM_ERR_INVALID_INPUT),
            (BlendFailure::Inconsistent("x"), SM_ERR_ASSERT_FAILURE),
            (
                BlendFailure::BadIntersections("x"),
                SM_ERR_BAD_INTERSECTIONS,
            ),
            (BlendFailure::NotConverged("x"), SM_ERR_NOT_CONVERGING),
            (BlendFailure::OutsideDomain("x"), SM_ERR_OUTSIDE_OF_DOMAIN),
            (
                BlendFailure::NotWithinTolerance("x"),
                SM_ERR_NOT_WITHIN_TOLERANCE,
            ),
            (
                BlendFailure::PassedOn {
                    what: "x",
                    status: SM_ERR_NOT_CONVERGING,
                },
                SM_ERR_NOT_CONVERGING,
            ),
            (
                BlendFailure::PassedOn {
                    what: "x",
                    status: SM_ERR_NULL_POINTER,
                },
                SM_ERR_NULL_POINTER,
            ),
        ];
        let edge = EntityRef::Edge(crate::topology::EdgeId::from_index(4));
        for (failure, code) in cases {
            for entity in [None, Some(edge)] {
                let typed = match entity {
                    Some(e) => SolidError::blend_at("blend::op", failure, e),
                    None => SolidError::blend("blend::op", failure),
                };
                let before = SolidError::Upstream {
                    operation: "blend::op",
                    code,
                    entity,
                };
                assert_eq!(typed.sm_status(), before.sm_status(), "{failure:?}");
                assert_eq!(typed.upstream_status(), Some(code), "{failure:?}");
                assert_eq!(super::super::upstream_status(&typed), Some(code));
                // a phrase cause keeps its status's kind, and is neither a defect nor a refusal
                // until its site is converted
                assert_eq!(typed.kind(), before.kind(), "{failure:?}");
                assert!(failure.is_phrase(), "{failure:?}");
                assert!(!typed.is_defect() && !typed.is_refusal() && !typed.sized());
                assert_eq!(typed.operation(), before.operation(), "{failure:?}");
                assert_eq!(typed.entity(), before.entity(), "{failure:?}");
                assert_eq!(typed.is_port_failure(), before.is_port_failure());
                assert_eq!(typed.is_status_answer(), before.is_status_answer());
                assert_eq!(
                    typed.is_unported_or_cancelled(),
                    before.is_unported_or_cancelled()
                );
                assert!(failure.replaces_upstream_status());
                assert_eq!(typed.blend_failure(), Some(&failure));
            }
        }
        assert_eq!(
            SolidError::blend_missing("blend::op", "the edge use's edge").to_string(),
            "blend::op: the edge use's edge is missing"
        );
        assert_eq!(
            SolidError::blend_failed("blend::op", "the law has no value").to_string(),
            "blend::op: the law has no value"
        );
        assert_eq!(
            SolidError::blend_at(
                "blend::op",
                BlendFailure::Failed("the result is not a manifold solid"),
                edge
            )
            .to_string(),
            "blend::op: the result is not a manifold solid (edge 4)"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::Inconsistent("two loops")).to_string(),
            "blend::op: a consistency check failed: two loops"
        );
        assert_eq!(
            SolidError::blend_invalid_value("blend::op", "x").kind(),
            ErrorKind::InvalidInput
        );
        assert_eq!(SolidError::sm_err("blend::op").blend_failure(), None);
    }

    fn target_edge(i: u32) -> crate::error::TargetEdge {
        use crate::blend::fillet_classification::FilletBody;
        use crate::blend::fillet_executive::session::FilletObject;
        let object = FilletObject::Topology(
            FilletBody::Target,
            EntityRef::Edge(crate::topology::EdgeId::from_index(i)),
        );
        crate::error::TargetEdge::of_fillet_object(&object).unwrap()
    }

    #[test]
    fn only_topology_of_the_target_body_is_a_site() {
        use crate::blend::fillet_classification::FilletBody;
        use crate::blend::fillet_executive::session::FilletObject;
        use crate::error::{TargetEdge, TargetFace, TargetVertex};
        let e = EntityRef::Edge(crate::topology::EdgeId::from_index(3));
        let f = EntityRef::Face(crate::topology::FaceId::from_index(5));
        let v = EntityRef::Vertex(crate::topology::VertexId::from_index(7));
        let t = |entity| FilletObject::Topology(FilletBody::Target, entity);
        assert_eq!(target_edge(3).id().index(), 3);
        assert_eq!(TargetFace::of_fillet_object(&t(f)).unwrap().id().index(), 5);
        assert_eq!(
            TargetVertex::of_fillet_object(&t(v)).unwrap().id().index(),
            7
        );
        assert_eq!(TargetEdge::of_fillet_object(&t(f)), None);
        for body in [FilletBody::Pseudo, FilletBody::SecondBase] {
            assert_eq!(
                TargetEdge::of_fillet_object(&FilletObject::Topology(body, e)),
                None
            );
        }
        assert_eq!(TargetEdge::of_fillet_object(&FilletObject::Executive), None);
    }

    #[test]
    fn the_class_is_derived_from_the_cause() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        let cases = [
            (
                BlendFailure::NoRoom(R::BallDoesNotFit),
                ErrorKind::BallDoesNotFit,
            ),
            (
                BlendFailure::NoRoom(R::RailLeavesFace),
                ErrorKind::RailLeavesFace,
            ),
            (
                BlendFailure::NoRoom(R::ConcaveOverflow),
                ErrorKind::RailLeavesFace,
            ),
            (
                BlendFailure::NoRoom(R::FaceConsumed),
                ErrorKind::FaceConsumed,
            ),
            (
                BlendFailure::NoRoom(R::BandsOverlap),
                ErrorKind::BandsOverlap,
            ),
            (BlendFailure::NoRoom(R::BallsFold), ErrorKind::BallsFold),
            (
                BlendFailure::NoRoom(R::OffsetFolds),
                ErrorKind::ChamferOffsetFolds,
            ),
            (BlendFailure::NoRoom(R::CutsThePart), ErrorKind::CutsThePart),
            (
                BlendFailure::NoRoom(R::BreaksThrough),
                ErrorKind::BreaksThrough,
            ),
            (
                BlendFailure::NoRoom(R::OffsetCollapses),
                ErrorKind::BallOffsetCollapses,
            ),
            (
                BlendFailure::NoRoom(R::CornersTakeWholeEdge),
                ErrorKind::CornersTakeWholeEdge,
            ),
            (
                BlendFailure::NoRoom(R::RoundIntoMaterial),
                ErrorKind::RunsIntoMaterial,
            ),
            (
                BlendFailure::NoRoom(R::RailRunsPastFaceIntoMaterial),
                ErrorKind::RunsIntoMaterial,
            ),
            (
                BlendFailure::NoRoom(R::OverflowWithConcaveBlend),
                ErrorKind::CliffOverflowRefused,
            ),
            (
                BlendFailure::NoRoom(R::OverflowWithRollover),
                ErrorKind::CliffOverflowRefused,
            ),
            (
                BlendFailure::NoRoom(R::OverflowWithGlobalMerge),
                ErrorKind::CliffOverflowRefused,
            ),
            (
                BlendFailure::NoRoom(R::CliffEndFaceNotPlanar),
                ErrorKind::CliffEndFaceNotPlanar,
            ),
            (
                BlendFailure::NoRoom(R::CliffLawNotHeld),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffEndNotCut),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryOnTwoFaces),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryBranches),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryClosed),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffBoundaryLeavesAlongNoSideEdge),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffClosingFaceUnbounded),
                ErrorKind::CliffOverflowShape,
            ),
            (
                BlendFailure::NoRoom(R::CliffSubtractionLeavesNoSolid),
                ErrorKind::CliffSubtractionLeavesNoSolid,
            ),
            (
                BlendFailure::NoRoom(R::CreaseAcrossTangentEdge),
                ErrorKind::CreaseAcrossTangentEdge,
            ),
            (
                BlendFailure::NoRoom(R::RailBelowResolution),
                ErrorKind::RailBelowResolution,
            ),
            (
                BlendFailure::NoRoom(R::RailPastAxis),
                ErrorKind::RailPastAxis,
            ),
            (
                BlendFailure::NoRoom(R::JoinOffsetsDiffer),
                ErrorKind::JoinOffsetsDiffer,
            ),
            (
                BlendFailure::NoRoom(R::JoinRailMissesSideEdge),
                ErrorKind::BallDoesNotFit,
            ),
            (
                BlendFailure::NoRoom(R::StripBelowResolution),
                ErrorKind::StripBelowResolution,
            ),
            (
                BlendFailure::NoRoom(R::EndPastSideEdge),
                ErrorKind::EndPastSideEdge,
            ),
            (
                BlendFailure::NoRoom(R::EndOverflowsSideEdge),
                ErrorKind::EndOverflowsSideEdge,
            ),
            (
                BlendFailure::NoRoom(R::EndPassesOverEndFace),
                ErrorKind::EndOverflowsSideEdge,
            ),
            (
                BlendFailure::NoRoom(R::EndRollsIntoSmoothEnd),
                ErrorKind::EndRollsIntoSmoothEnd,
            ),
            (
                BlendFailure::NoRoom(R::EndAtSeamVertex),
                ErrorKind::EndAtSeamVertex,
            ),
            (
                BlendFailure::NoRoom(R::TangentContinuationEndUnsupported),
                ErrorKind::TangentContinuationEnd,
            ),
            (
                BlendFailure::NoRoom(R::OverflowKeepsNothing),
                ErrorKind::OverflowKeepsNothing,
            ),
            // a weld bead's (#15129): too large, naming the size that builds
            (
                BlendFailure::NoRoom(R::LegPastFace),
                ErrorKind::FilletTooLarge,
            ),
            (
                BlendFailure::NoRoom(R::NoToeAtLeg),
                ErrorKind::FilletTooLarge,
            ),
            (
                BlendFailure::NoRoom(R::BeadCutsFace),
                ErrorKind::FilletTooLarge,
            ),
            // a weld bead not built yet: neither a defect nor a refusal
            (
                BlendFailure::WeldNotBuilt(WeldGap::BeadsMeetAtVertex),
                ErrorKind::Unsupported,
            ),
            (
                BlendFailure::WeldNotBuilt(WeldGap::CurvedJoint),
                ErrorKind::Unsupported,
            ),
            (
                BlendFailure::WeldNotBuilt(WeldGap::ObtuseToe),
                ErrorKind::Unsupported,
            ),
            (
                BlendFailure::StationDidNotConverge {
                    s: 0.42,
                    residual: Some(3e-6),
                    tolerance: 1e-9,
                    step: Some(1e-4),
                    iterations: Some(50),
                },
                ErrorKind::StationDidNotConverge,
            ),
            (BlendFailure::TrimLoopOpen, ErrorKind::TrimLeaks),
            (BlendFailure::TrimLoopDoesNotSeparate, ErrorKind::TrimLeaks),
            (BlendFailure::StageFailed, ErrorKind::StageFailed),
            (BlendFailure::Defect { why: "x" }, ErrorKind::StageFailed),
        ];
        for (cause, class) in cases {
            assert_eq!(cause.class(), class, "{cause:?}");
            let e = SolidError::blend("blend::op", cause);
            assert_eq!(e.kind(), class, "{cause:?}");
            // every cause stands in for its site's status, which the goldens read
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{cause:?}");
            assert_eq!(e.is_defect(), class.is_defect(), "{cause:?}");
            assert_eq!(e.is_refusal(), class.is_refusal(), "{cause:?}");
            assert_eq!(e.sized(), class.sized(), "{cause:?}");
            assert!(!e.is_port_failure(), "{cause:?}");
        }
    }

    #[test]
    fn a_fillet_removal_refusal_is_a_refusal_no_size_decides() {
        use RemovalRefusal as R;
        let cases = [
            (R::FaceGrowsPastDomain, ErrorKind::RemovalNotExact),
            (R::CornerNeedsFittedIntersection, ErrorKind::RemovalNotExact),
            (R::SideEdgeNotStraight, ErrorKind::RemovalNotExact),
            (R::CornerOffSideLine, ErrorKind::RemovalNotExact),
            (R::TangentSurfacesDoNotMeet, ErrorKind::RemovalNotExact),
            (
                R::UnsupportedSharpSpine(BlendSpine::Conic),
                ErrorKind::RemovalNotExact,
            ),
            (R::RailTrimOutsideTolerance, ErrorKind::RemovalNotExact),
            (R::SeamTrimMissesNewVertex, ErrorKind::RemovalNotExact),
            (R::NotFourEdges, ErrorKind::NotARemovableFillet),
            (R::AdjacentEdgesTangent, ErrorKind::NotARemovableFillet),
            (R::NotOneTangentChain, ErrorKind::NotARemovableFillet),
            (R::OppositeEdgeNotTangent, ErrorKind::NotARemovableFillet),
            (R::NoTangentEdge, ErrorKind::NotARemovableFillet),
            (R::EdgeWithoutTwoVertices, ErrorKind::NotARemovableFillet),
            (R::EdgeNotBetweenTwoFaces, ErrorKind::NotARemovableFillet),
            (R::TangentFaceNotBSpline, ErrorKind::NotARemovableFillet),
            (R::EndFaceNotBSpline, ErrorKind::NotARemovableFillet),
            (R::NotARing, ErrorKind::NotARemovableFillet),
            (R::MoreThanTwoRails, ErrorKind::NotARemovableFillet),
            (R::RailsNotBetweenTwoFaces, ErrorKind::NotARemovableFillet),
            (R::SeamDoesNotJoinRails, ErrorKind::NotARemovableFillet),
            (R::RailVertexHasOtherEdges, ErrorKind::NotARemovableFillet),
            (R::SeamCrossesSharpEdgeTwice, ErrorKind::NotARemovableFillet),
            (R::SeamsCrossApart, ErrorKind::NotARemovableFillet),
            (R::SeamHasMoreThanTwoTrims, ErrorKind::NotARemovableFillet),
            (R::SeamHasNoOwnTrim, ErrorKind::NotARemovableFillet),
        ];
        for (reason, class) in cases {
            let cause = BlendFailure::RemovalRefused(reason);
            assert_eq!(cause.class(), class, "{reason:?}");
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            let e = SolidError::blend("blend::fillet_removal", cause);
            assert_eq!(e.kind(), class, "{reason:?}");
            // the status the C++ reports stays, for the goldens
            assert_eq!(e.upstream_status(), Some(SM_ERR), "{reason:?}");
            assert!(e.is_refusal() && !e.is_defect() && !e.sized(), "{reason:?}");
        }
        assert_eq!(
            SolidError::blend(
                "blend::fillet_removal",
                BlendFailure::RemovalRefused(R::FaceGrowsPastDomain)
            )
            .to_string(),
            "blend::fillet_removal: removal_not_exact: FaceGrowsPastDomain"
        );
    }

    #[test]
    fn a_corner_shape_not_built_is_unsupported_with_its_sites_status() {
        use CornerShape as S;
        let shapes = [
            (S::MoreThanFourEdges, SM_ERR),
            (S::BlendedEdgeOffRing, SM_ERR),
            (S::AdjacentBlendedEdges, SM_ERR),
            (S::RingEdgesShareNoFace, SM_ERR_NULL_POINTER),
            (S::BevelNotOfThreeEdges, SM_ERR),
            (S::NoConvexSideEdge, SM_ERR_NULL_POINTER),
            (S::RingEdgeOffClosedBlendFaces, SM_ERR),
            (S::SideEdgeNotBesideBlend, SM_ERR_NULL_POINTER),
            (S::EndSideEdgesShareNoFace, SM_ERR),
            (S::EdgeNotBetweenTwoFaces, SM_ERR),
            (S::SeamCapEndFaceNotExtended, SM_ERR),
        ];
        for (shape, status) in shapes {
            let cause = BlendFailure::CornerShapeNotBuilt(shape);
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            let e = SolidError::blend("blend::fillet_corner::topology", cause);
            assert_eq!(e.kind(), ErrorKind::Unsupported, "{shape:?}");
            assert_eq!(e.upstream_status(), Some(status), "{shape:?}");
            assert!(!e.is_defect(), "{shape:?}");
        }
        assert_eq!(
            SolidError::blend(
                "blend::op",
                BlendFailure::CornerShapeNotBuilt(S::MoreThanFourEdges)
            )
            .to_string(),
            "blend::op: unsupported: MoreThanFourEdges"
        );
        let drop = BlendFailure::CornerPointDropUnresolved { solutions: 2 };
        assert!(!drop.is_phrase() && drop.replaces_upstream_status());
        let e = SolidError::blend("blend::op", drop);
        assert_eq!(e.kind(), ErrorKind::NotFound);
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert!(!e.is_defect());
    }

    #[test]
    fn a_corner_patch_not_made_takes_its_misss_class_and_status() {
        use CornerPatchMiss as M;
        use ErrorKind as K;
        let misses = [
            (M::VariableRadiusNotFourSides, K::Unsupported, SM_ERR),
            (
                M::VariableRadiusWithoutInterpolation,
                K::Unsupported,
                SM_ERR,
            ),
            (M::VariableRadiusSideNotCrossSection, K::Unsupported, SM_ERR),
            (M::VariableRadiusGeneratorsDiffer, K::Unsupported, SM_ERR),
            (M::VariableRadiusBlendCurvesDiffer, K::Unsupported, SM_ERR),
            (
                M::CrossSectionWithoutCentreLine,
                K::Unsupported,
                SM_ERR_NULL_POINTER,
            ),
            (M::CentreLineFootNotUnique, K::NotFound, SM_ERR),
            (M::CrossSectionRadiusVaries, K::Unsupported, SM_ERR),
            (
                M::CrossSectionEndsAligned,
                K::Degenerate,
                SM_ERR_INVALID_INPUT,
            ),
            (M::OppositeEdgeNotCrossSection, K::Unsupported, SM_ERR),
            (M::RailNotNamedByItsGeometry, K::Unsupported, SM_ERR),
            (M::NoRailAtInterpolationStart, K::NotFound, SM_ERR),
            (M::OppositeFootNotUnique, K::NotFound, SM_ERR),
            (M::OppositeEndsNotAcrossFillet, K::Unsupported, SM_ERR),
            (
                M::OppositeNormalVanished,
                K::Degenerate,
                SM_ERR_INVALID_INPUT,
            ),
            (M::LinearOppositeFillet, K::Unsupported, SM_ERR),
            (M::OppositeRadiusNotLarger, K::Unsupported, SM_ERR),
            (M::VariableRadiusReSolveFailed, K::Failed, SM_ERR),
            (M::RailRailCurveCrossesCorner, K::Unsupported, SM_ERR),
            (M::RailLeavesOppositeFillet, K::Unsupported, SM_ERR),
            (
                M::CurveNotBSpline { kind: "offset" },
                K::Unsupported,
                SM_ERR,
            ),
            (
                M::TooFewBoundaryCurves { curves: 3 },
                K::Unsupported,
                SM_ERR,
            ),
            (M::FaceNotMade, K::Failed, SM_ERR),
            (
                M::MixedCornerNotFourEdges { edges: 3 },
                K::Unsupported,
                SM_ERR,
            ),
            (
                M::ExtendedEdgeDropNotOneCurve { curves: 2 },
                K::NotFound,
                SM_ERR,
            ),
            (M::GapPatchNotFourEdges { edges: 3 }, K::Unsupported, SM_ERR),
        ];
        for (miss, kind, status) in misses {
            let cause = BlendFailure::CornerPatchNotMade(miss);
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            let e = SolidError::blend("blend::fillet_corner", cause);
            assert_eq!(e.kind(), kind, "{miss:?}");
            assert_eq!(e.upstream_status(), Some(status), "{miss:?}");
            assert!(!e.is_defect() && !e.is_refusal(), "{miss:?}");
        }
        assert_eq!(
            SolidError::blend(
                "blend::op",
                BlendFailure::CornerPatchNotMade(M::TooFewBoundaryCurves { curves: 3 })
            )
            .to_string(),
            "blend::op: unsupported: TooFewBoundaryCurves { curves: 3 }"
        );
    }

    /// A corner step's check of the corner a caller gives it is invalid input, never a
    /// defect, with the status of the site it replaces and the entity it names.
    #[test]
    fn a_corner_input_invalid_is_invalid_input_with_its_sites_status() {
        use crate::blend::fillet_corner::p3::FilletCornerKind;
        use CornerInput as I;
        let inputs = [
            (
                I::FilletedEdgeUsesNotGiven {
                    kind: FilletCornerKind::NByTwo,
                },
                SM_ERR_NULL_POINTER,
            ),
            (
                I::FilletedEdgeUsesNotTwo {
                    kind: FilletCornerKind::NByTwo,
                    given: 1,
                },
                SM_ERR,
            ),
            (I::NoRailXEdgeUseVertex, SM_ERR_NULL_POINTER),
            (I::VertexNotRailXRail, SM_ERR),
            (I::EdgeUseWithoutFilletGeometry, SM_ERR_NULL_POINTER),
            (I::EdgeWithoutCurve, SM_ERR_NULL_POINTER),
            (I::MixedCornerWithoutOnEdgeEdge, SM_ERR_NULL_POINTER),
            (I::MixedCornerWithoutOnExtendedEdgeEdge, SM_ERR_NULL_POINTER),
            (I::MixedCornerEdgesDoNotMeet, SM_ERR),
            (I::MixedCornerVertexNotInCorner, SM_ERR),
        ];
        for (input, status) in inputs {
            let cause = BlendFailure::CornerInputInvalid {
                input,
                entity: None,
            };
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            let e = SolidError::blend("blend::fillet_corner", cause);
            assert_eq!(e.kind(), ErrorKind::InvalidInput, "{input:?}");
            assert_eq!(e.upstream_status(), Some(status), "{input:?}");
            assert!(!e.is_defect(), "{input:?}");
            assert_eq!(e.entity(), None, "{input:?}");
        }
        // the failure takes the entity from the cause, with no `at` of its own
        let at = EntityRef::Edge(crate::topology::EdgeId::from_index(3));
        let e = SolidError::blend(
            "blend::op",
            BlendFailure::CornerInputInvalid {
                input: I::EdgeWithoutCurve,
                entity: Some(at),
            },
        );
        assert_eq!(e.entity(), Some(at));
        assert_eq!(
            e.to_string(),
            "blend::op: invalid_input: the fillet corner given: EdgeWithoutCurve (edge 3)"
        );
        // the links other routines read too are the fillet set-up's, with its statuses
        let link = BlendFailure::FilletSetUpInvalid(FilletSetUp::PrimaryEdgeUse);
        assert!(!link.is_phrase() && link.replaces_upstream_status());
        let e = SolidError::blend("blend::op", link);
        assert_eq!(e.kind(), ErrorKind::InvalidInput);
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert!(!e.is_defect());
        assert_eq!(
            e.to_string(),
            "blend::op: invalid_input: a fillet edge has no primary edge use"
        );
    }

    #[test]
    fn only_a_stage_failure_built_on_purpose_is_a_defect() {
        // a refusal not yet on the record, as `record_no_room` returns it: a phrase cause
        let refusal = SolidError::blend_failed(
            "blend::fillet_executive::p3",
            "the blend is too large for its faces",
        );
        assert_eq!(refusal.kind(), ErrorKind::Failed);
        assert!(!refusal.is_defect() && !refusal.is_refusal() && !refusal.sized());
        let missing = SolidError::blend_missing("blend::op", "the edge use's edge");
        assert_eq!(missing.kind(), ErrorKind::Missing);
        assert!(!missing.is_defect() && !missing.is_refusal());
        // a phrase cause passing on an offset status keeps that status's kind but is still not
        // classed
        let passed = SolidError::blend(
            "blend::op",
            BlendFailure::PassedOn {
                what: "x",
                status: crate::error::SM_ERR_OFFSET_WALLS_TOUCH,
            },
        );
        assert_eq!(passed.kind(), ErrorKind::OffsetWallsTouch);
        assert!(!passed.is_defect() && !passed.is_refusal());
        // a stage failure built on purpose is class 127, a defect
        let stage = SolidError::blend("blend::op", BlendFailure::StageFailed);
        assert_eq!(stage.kind(), ErrorKind::StageFailed);
        assert_eq!(stage.kind().code(), 127);
        assert!(stage.is_defect() && !stage.is_refusal() && !stage.sized());
    }

    /// A reviewed defect is of class 127 with the stage failure's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the station solve was started off its own previous station, which it never leaves";
        let defect = BlendFailure::Defect { why };
        assert!(!defect.is_phrase() && defect.replaces_upstream_status());
        for status in [SM_ERR, SM_ERR_NULL_POINTER] {
            let e = Failure::new("blend::op", defect)
                .with_status(status)
                .into_error();
            super::super::tests::assert_defect(
                &e,
                &Failure::new("blend::op", BlendFailure::StageFailed)
                    .with_status(status)
                    .into_error(),
                "blend::op",
                status,
                &format!("blend::op: stage_failed: {why}"),
            );
            assert_eq!(e.blend_failure(), Some(&defect));
        }
        assert_eq!(
            SolidError::blend("blend::op", defect).sm_status(),
            Some(SM_ERR)
        );
    }

    #[test]
    fn typed_causes_are_worded_from_their_numbers() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        let station = SolidError::blend(
            "blend::op",
            BlendFailure::StationDidNotConverge {
                s: 0.42,
                residual: Some(3e-6),
                tolerance: 1e-9,
                step: Some(0.0001),
                iterations: Some(50),
            },
        );
        assert_eq!(
            station.to_string(),
            "blend::op: station_did_not_converge: the station at s = 0.42 did not converge: residual 0.000003 > 0.000000001 after 50 iterations (last step 0.0001)"
        );
        // a solve that reports only whether it converged (#15256's skin check)
        let unreported = SolidError::blend(
            "blend::op",
            BlendFailure::StationDidNotConverge {
                s: 0.5,
                residual: None,
                tolerance: 1e-9,
                step: None,
                iterations: None,
            },
        );
        assert_eq!(
            unreported.to_string(),
            "blend::op: station_did_not_converge: the station at s = 0.5 did not converge: residual unreported > 0.000000001 after unreported iterations (last step unreported)"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::NoRoom(R::OverflowWithRollover))
                .to_string(),
            "blend::op: cliff_overflow_refused: OverflowWithRollover"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::TrimLoopOpen).to_string(),
            "blend::op: trim_leaks: the trim loop is open"
        );
        assert_eq!(
            SolidError::blend("blend::op", BlendFailure::StageFailed).to_string(),
            "blend::op: stage_failed"
        );
        // the site and the fit, when known, follow the cause
        let sited = Failure::new("blend::op", BlendFailure::NoRoom(R::BallDoesNotFit))
            .or_at(crate::error::Site::edge(target_edge(4)))
            .with_fit(crate::error::Fit::new(
                2.5,
                crate::error::FitMeasure::Radius,
                Some(target_edge(4)),
            ))
            .into_error();
        assert_eq!(
            sited.to_string(),
            "blend::op: ball_does_not_fit: BallDoesNotFit at edge 4; the radius that builds is 2.5"
        );
        assert_eq!(sited.fit().map(|f| f.size), Some(2.5));
        assert!(sited.sized());
    }

    /// One of each cause the fillet geometry's `p2.rs` names (TE-B-02, #19810).
    fn fillet_geom_p2_causes() -> [BlendFailure; 16] {
        let face = crate::topology::FaceId::from_index(3);
        [
            BlendFailure::ArcLengthNotConverged {
                start: 0.0,
                length: 2.5,
                stopped_at: 0.5,
            },
            BlendFailure::SrfSrfHeadingNotFollowed,
            BlendFailure::SrfSrfNoCurveThroughEnds { candidates: 2 },
            BlendFailure::IntersectionCurveNotBSpline { kind: "SmIsoCurve" },
            BlendFailure::IntersectionCurveHasNoUvCurve { on_second: true },
            BlendFailure::RailCrossingNotConverged { face },
            BlendFailure::SideEdgeChainPastBound { steps: 64 },
            BlendFailure::RailEndMissesSideEdge {
                edge: crate::topology::EdgeId::from_index(7),
            },
            BlendFailure::JoinSectionNotConverged { blend: 1 },
            BlendFailure::JoinBlendsRollOffSection,
            BlendFailure::SetBackFootNotUnique { feet: 2 },
            BlendFailure::MateFaceSurfaceNotBSpline {
                face,
                kind: "SmOffsetSurface",
            },
            BlendFailure::InterpolantDropsToCurves { face, curves: 0 },
            BlendFailure::SplitFaceNotGiven,
            BlendFailure::FilletVertexWithoutCorner,
            BlendFailure::FilletSetUpInvalid(FilletSetUp::SolverOffsetSurface),
        ]
    }

    /// The fillet solver causes the fillet geometry's `p2.rs` raises (copied from #20715 and
    /// #20732): the caller's input, status `SM_ERR_NULL_POINTER` by default, never a phrase,
    /// a defect or a refusal.
    #[test]
    fn the_fillet_geometry_p2_solver_causes_are_the_callers_input() {
        let fillet_geom = crate::blend::fillet_geom::p1::FilletGeomId(2);
        let cases = [
            (
                SolverFailure::EdgeUseWithoutFace,
                "an edge use of the body has no face",
            ),
            (
                SolverFailure::SolverEdgeUseMissing { index: 1 },
                "the solver has no edge use 1 of the edge to fillet",
            ),
            (
                SolverFailure::FilletGeomWithoutSolver { fillet_geom },
                "fillet geometry FilletGeomId(2) names no fillet solver",
            ),
        ];
        for (solver, message) in cases {
            let cause = BlendFailure::Solver(solver);
            assert_eq!(cause.class(), ErrorKind::InvalidInput, "{cause:?}");
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            assert_eq!(cause.comparison_status(), SM_ERR_NULL_POINTER);
            let e = SolidError::blend_solver("blend::op", solver);
            assert_eq!(e.kind(), ErrorKind::InvalidInput);
            assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
            assert!(!e.is_defect() && !e.is_refusal(), "{e:?}");
            assert_eq!(e.to_string(), format!("blend::op: {message}"));
        }
    }

    /// The causes of the fillet geometry's `p2.rs` (TE-B-02, #19810): each class, status and
    /// message; none a phrase, a defect or a refusal, and each stands in for its status.
    #[test]
    fn the_fillet_geometry_p2_causes_keep_their_status_and_name_their_class() {
        use ErrorKind as K;
        let expected = [
            (
                K::NotConverged,
                SM_ERR_NOT_CONVERGING,
                "the search for the parameter 2.5 along the curve from 0 stopped at 0.5",
            ),
            (
                K::NotFound,
                SM_ERR,
                "no intersection curve through the points leaves the start along the heading given",
            ),
            (
                K::NotFound,
                SM_ERR,
                "none of the 2 intersection curves passes through the end points",
            ),
            (
                K::Unsupported,
                SM_ERR,
                "the intersection curve is a SmIsoCurve, which has no B-spline form",
            ),
            (
                K::Unsupported,
                SM_ERR,
                "the intersection curve has no UV curve on the second surface",
            ),
            (
                K::NotConverged,
                SM_ERR,
                "the local solve of the two rails' crossing on the face FaceId(3) stopped without finding it",
            ),
            (
                K::Unsupported,
                SM_ERR,
                "the rail's end was not found within 64 side edges",
            ),
            (
                K::NotFound,
                SM_ERR,
                "the rail meets the side edge EdgeId(7) nowhere",
            ),
            (
                K::NotConverged,
                SM_ERR,
                "the solve for blend 1's ball in the Join's section plane stopped without a ball",
            ),
            (
                K::Unsupported,
                SM_ERR,
                "no blend of the Join rolls on the two faces its shared section ends on",
            ),
            (
                K::Unsupported,
                SM_ERR,
                "the set-back section's point has 2 closest points on the offset surface, not one",
            ),
            (
                K::Unsupported,
                SM_ERR_NULL_POINTER,
                "the face FaceId(3) has a SmOffsetSurface surface, which is not of a B-spline class: no point or curve is built on it",
            ),
            (
                K::Unsupported,
                SM_ERR,
                "the curve between the rails' ends drops onto face FaceId(3) as 0 UV curves, not one",
            ),
            (
                K::InvalidInput,
                SM_ERR_NULL_POINTER,
                "the face that was split or the new face was not given",
            ),
            (
                K::InvalidInput,
                SM_ERR_NULL_POINTER,
                "the fillet vertex has no fillet corner",
            ),
            (
                K::InvalidInput,
                SM_ERR_NULL_POINTER,
                "a fillet solver lacks one of its two offset surfaces",
            ),
        ];
        for (cause, (class, status, message)) in fillet_geom_p2_causes().into_iter().zip(expected) {
            assert_eq!(cause.class(), class, "{cause:?}");
            assert!(
                !cause.is_phrase() && cause.replaces_upstream_status(),
                "{cause:?}"
            );
            assert_eq!(cause.comparison_status(), status, "{cause:?}");
            let e = SolidError::blend("blend::op", cause);
            assert_eq!(e.kind(), class, "{cause:?}");
            assert_eq!(e.blend_failure(), Some(&cause));
            assert_eq!(e.upstream_status(), Some(status), "{cause:?}");
            assert_eq!(super::super::upstream_status(&e), Some(status), "{cause:?}");
            assert!(!e.is_defect() && !e.is_refusal() && !e.sized(), "{cause:?}");
            assert_eq!(e.to_string(), format!("blend::op: {class}: {message}"));
        }
    }

    fn assert_plane_curve_cause(cause: BlendFailure, class: ErrorKind, message: &str) {
        assert_eq!(cause.class(), class);
        assert!(!cause.is_phrase());
        assert!(cause.replaces_upstream_status());
        assert_eq!(cause.comparison_status(), SM_ERR);
        let e = SolidError::blend("blend::op", cause);
        assert_eq!(e.kind(), class);
        assert_eq!(e.operation(), "blend::op");
        assert_eq!(e.blend_failure(), Some(&cause));
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert_eq!(e.is_defect(), class.is_defect());
        assert!(!e.is_refusal() && !e.sized());
        assert_eq!(e.to_string(), format!("blend::op: {class}: {message}"));
    }

    /// TE-B-03 (#19811): the causes of the fillet geometry's part 4 keep the status of the
    /// `SER`/`NER` they replace, take their class from the cause and word their numbers.
    #[test]
    fn the_fillet_corner_geometry_causes_keep_their_status_and_name_their_numbers() {
        use crate::topology::{EdgeId, FaceId};
        let face = FaceId::from_index(3);
        let cases: [(BlendFailure, ErrorKind, i32, &str); 21] = [
            (
                BlendFailure::SeamCornersBothCut,
                ErrorKind::Unsupported,
                SM_ERR,
                "neither end of the seam curves is at a corner of the closed surface: both ends cut off",
            ),
            (
                BlendFailure::SeamCutOffRib { tolerance: 1e-6 },
                ErrorKind::NotFound,
                SM_ERR,
                "the seam's cut end does not drop onto the seam's rib within 0.000001",
            ),
            (
                BlendFailure::UvCurveEndOffSurface {
                    start: false,
                    end: true,
                    tolerance: 0.5,
                },
                ErrorKind::NotFound,
                SM_ERR,
                "the UV trim curve's end does not drop onto the surface (last tolerance 0.5)",
            ),
            (
                BlendFailure::UvCurveEndsUnresolved {
                    start_solutions: 1,
                    end_solutions: 3,
                },
                ErrorKind::Unsupported,
                SM_ERR,
                "the UV trim curve's ends drop onto the surface 1 and 3 times, which no curve is built for",
            ),
            (
                BlendFailure::UvCurveSurfaceNotBSpline {
                    face: Some(face),
                    kind: "SmPlane",
                },
                ErrorKind::Unsupported,
                SM_ERR_NULL_POINTER,
                "the UV curve's surface of face FaceId(3) is a SmPlane surface, which is not of a \
                 B-spline class",
            ),
            (
                BlendFailure::UvCurveSurfaceNotBSpline {
                    face: None,
                    kind: "SmPlane",
                },
                ErrorKind::Unsupported,
                SM_ERR_NULL_POINTER,
                "the UV curve's surface is a SmPlane surface, which is not of a B-spline class",
            ),
            (
                BlendFailure::MateFaceSurfaceNotBSpline {
                    face,
                    kind: "SmPlane",
                },
                ErrorKind::Unsupported,
                SM_ERR_NULL_POINTER,
                "the face FaceId(3) has a SmPlane surface, which is not of a B-spline class: no \
                 point or curve is built on it",
            ),
            (
                BlendFailure::CurveWithoutNet {
                    edge: Some(EdgeId::from_index(2)),
                    kind: "SmCircle",
                },
                ErrorKind::Unsupported,
                SM_ERR,
                "the curve of edge EdgeId(2) is a SmCircle curve, which is not of a B-spline class",
            ),
            (
                BlendFailure::CornerSurfacesNotIntersected,
                ErrorKind::Failed,
                SM_ERR,
                "the intersection of the corner's surfaces failed",
            ),
            (
                BlendFailure::CornerCurvesDoNotMeet,
                ErrorKind::NotFound,
                SM_ERR,
                "the corner's two intersection curves do not cross",
            ),
            (
                BlendFailure::CornerCurvesMeetMoreThanOnce { crossings: 2 },
                ErrorKind::Unsupported,
                SM_ERR,
                "the corner's two intersection curves cross 2 times, not once",
            ),
            (
                BlendFailure::IntersectionEndsOffFace { face },
                ErrorKind::NotFound,
                SM_ERR,
                "neither end of the intersection curve is on face FaceId(3)",
            ),
            (
                BlendFailure::RailMissesEndSurface { tolerance: 0.25 },
                ErrorKind::NotFound,
                SM_ERR,
                "the rail does not cross the end face's surface within 0.25",
            ),
            (
                BlendFailure::NoVertexGiven,
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
                "no vertex given",
            ),
            (
                BlendFailure::FilletSetUpInvalid(FilletSetUp::RadialEdgeUse),
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
                "an edge use of a side edge has no radial edge use",
            ),
            (
                BlendFailure::FilletSetUpInvalid(FilletSetUp::EdgeUseGeom),
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
                "an edge use of a fillet edge other than a cross-section spans no fillet geometry",
            ),
            (
                BlendFailure::FilletVertexWithoutCorner,
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
                "the fillet vertex has no fillet corner",
            ),
            (
                BlendFailure::FilletSetUpInvalid(FilletSetUp::VertexEndRib),
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
                "a fillet vertex at a fillet's end has no end rib",
            ),
            (
                BlendFailure::NeighbourVertexNotProcessed {
                    vertex: crate::topology::VertexId::from_index(5),
                },
                ErrorKind::InvalidInput,
                SM_ERR,
                "the corner's neighbouring fillet vertex 5 is not processed yet",
            ),
            (
                BlendFailure::RailNotBlending {
                    rail: EdgeId::from_index(6),
                },
                ErrorKind::InvalidInput,
                SM_ERR,
                "rail 6 of the blending geometry is not a blending rail",
            ),
            (
                BlendFailure::SeamSurfaceNotClosed,
                ErrorKind::InvalidInput,
                SM_ERR,
                "the surface whose seam curves are asked for is not closed",
            ),
        ];
        for (cause, class, status, message) in cases {
            assert_eq!(cause.class(), class, "{cause:?}");
            assert!(!cause.is_phrase() && cause.replaces_upstream_status());
            assert_eq!(cause.comparison_status(), status, "{cause:?}");
            let e = SolidError::blend("blend::op", cause);
            assert_eq!(e.kind(), class);
            assert_eq!(e.upstream_status(), Some(status), "{cause:?}");
            assert_eq!(e.blend_failure(), Some(&cause));
            assert_eq!(e.to_string(), format!("blend::op: {class}: {message}"));
        }
        // a failed corner intersection reads as the error it chains
        let refusal = SolidError::blend("blend::ssi", BlendFailure::CornerCurvesDoNotMeet);
        let chained = Failure::new("blend::op", BlendFailure::CornerSurfacesNotIntersected)
            .caused_by(refusal)
            .into_error();
        assert_eq!(chained.upstream_status(), Some(SM_ERR));
        let defect = SolidError::blend("blend::ssi", BlendFailure::Defect { why: "x" });
        let chained = Failure::new("blend::op", BlendFailure::CornerSurfacesNotIntersected)
            .caused_by(defect)
            .into_error();
        assert!(chained.is_defect());
    }

    #[test]
    fn a_plane_curve_touch_names_its_station() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveTouches { u: 0.25, v: 0.5 },
            ErrorKind::Degenerate,
            "the surface touches the plane at (u, v) = (0.25, 0.5): the crossing has no direction",
        );
    }

    #[test]
    fn a_plane_curve_end_off_names_the_end_and_distance_when_found() {
        for (at_end, end) in [(false, "start"), (true, "end")] {
            for (distance, detail) in [
                (Some(0.125), " by 0.125"),
                (None, ": the drop or projection found no point"),
            ] {
                assert_plane_curve_cause(
                    BlendFailure::PlaneCurveEndOff { at_end, distance },
                    ErrorKind::NotFound,
                    &format!("the crossing's {end} is off the surface or the plane{detail}"),
                );
            }
        }
    }

    #[test]
    fn a_plane_curve_leaving_the_domain_names_its_station() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveLeavesDomain { u: -0.25, v: 0.5 },
            ErrorKind::OutsideDomain,
            "the crossing leaves the surface's domain at (u, v) = (-0.25, 0.5) before its end",
        );
    }

    #[test]
    fn a_plane_curve_vanished_step_is_a_defect_with_its_numbers() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveStepVanished {
                u: 0.25,
                v: 0.5,
                step: 0.0000000005,
                minimum: 0.000000001,
            },
            ErrorKind::StageFailed,
            "the crossing's step at (u, v) = (0.25, 0.5) was halved to 0.0000000005, below its minimum 0.000000001, without an accepted step",
        );
    }

    #[test]
    fn a_plane_curve_fit_cap_names_its_stations_and_passes() {
        assert_plane_curve_cause(
            BlendFailure::PlaneCurveFitCap {
                stations: 128,
                passes: 20,
            },
            ErrorKind::NotWithinTolerance,
            "the crossing's Hermite fit is still off the exact curve by more than a tenth of the tolerance with 128 stations after 20 passes",
        );
    }

    /// The G^n section's causes: each class, status `SM_ERR`, and wording with its numbers.
    #[test]
    fn the_section_causes_name_their_class_and_numbers() {
        let rows = [
            (
                BlendFailure::SectionNotProved(SectionProof::FaceWeightPositive),
                ErrorKind::NotWithinTolerance,
                "the section's certificate did not prove a face's weights positive over the box",
            ),
            (
                BlendFailure::SectionDegenerate(SectionDegeneracy::AreaDoesNotFall),
                ErrorKind::Degenerate,
                "the section's area does not fall with its thumbweight",
            ),
            (
                BlendFailure::SectionUnsupported(SectionLimit::OrderPastSeries {
                    order: 9,
                    most: 7,
                }),
                ErrorKind::Unsupported,
                "the order 9 is past the most the series hold (7)",
            ),
            (
                BlendFailure::RadiusLawNotPositive {
                    s: 0.5,
                    radius: -0.25,
                },
                ErrorKind::InvalidInput,
                "the radius law is -0.25 at s = 0.5, not positive",
            ),
            (
                BlendFailure::SectionInputInvalid(SectionInput::RangeOutsideStations),
                ErrorKind::InvalidInput,
                "the round's range is not within its stations",
            ),
        ];
        for (cause, class, message) in rows {
            assert_plane_curve_cause(cause, class, message);
            assert!(!class.is_defect());
        }
    }

    #[test]
    fn a_ball_triangle_not_one_patch_is_unsupported_with_its_pole() {
        let e = SolidError::blend(
            "blend::op",
            BlendFailure::BallTriangleNotOnePatch { pole: 2 },
        );
        assert_eq!(e.kind(), ErrorKind::Unsupported);
        assert_eq!(e.upstream_status(), Some(SM_ERR));
        assert!(!e.is_defect());
        assert!(!e.is_port_failure());
        assert!(
            e.to_string().contains(
                "the ball corner's triangle is not one rational patch from the pole at contact 2"
            ),
            "{e}"
        );
    }

    #[test]
    fn a_mixed_corner_step_takes_its_class_and_keeps_sm_err() {
        for (step, class) in [
            (MixedStep::RailMissesRailWall, ErrorKind::NotFound),
            (MixedStep::ConcaveEdgeStraight, ErrorKind::Degenerate),
            (MixedStep::TangentPlaneTurns, ErrorKind::Unsupported),
        ] {
            let e = SolidError::blend("blend::op", BlendFailure::MixedCornerUnbuildable(step));
            assert_eq!(e.kind(), class, "{e}");
            assert_eq!(e.upstream_status(), Some(SM_ERR));
            assert!(!e.is_defect() && !e.is_port_failure(), "{e}");
            assert!(e.to_string().contains(step.what()), "{e}");
        }
    }

    #[test]
    fn a_corner_step_takes_its_class_keeps_sm_err_and_is_neither_defect_nor_refusal() {
        for (step, class) in [
            (
                CornerStep::BallCentreLineMissesOffset,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CornerStep::BallCentreNotProved,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CornerStep::ChamferSurfaceOffEdgeLine,
                ErrorKind::Unsupported,
            ),
            (
                CornerStep::BallOffsetPlanesDegenerate,
                ErrorKind::Degenerate,
            ),
            (CornerStep::BallPatchNormalUndecided, ErrorKind::Degenerate),
            (CornerStep::BallContactFootNotFound, ErrorKind::NotFound),
            (
                CornerStep::BallCentreOffRound,
                ErrorKind::NotWithinTolerance,
            ),
            (
                CornerStep::CommonFaceNotPlanar,
                ErrorKind::NotWithinTolerance,
            ),
        ] {
            let e = SolidError::blend("blend::op", BlendFailure::CornerUnbuildable(step));
            assert_eq!(e.kind(), class, "{e}");
            assert_eq!(e.upstream_status(), Some(SM_ERR));
            assert!(
                !e.is_defect() && !e.is_refusal() && !e.is_port_failure(),
                "{e}"
            );
            assert!(!e.is_unported_or_cancelled(), "{e}");
            assert!(e.to_string().contains(step.what()), "{e}");
        }
    }

    #[test]
    fn a_corner_input_is_invalid_input_with_the_status_its_site_returned() {
        for (input, status) in [
            (CornerInput::EventNotSet, SM_ERR_NULL_POINTER),
            (CornerInput::RingNotSet, SM_ERR_NULL_POINTER),
            (CornerInput::EdgeWithoutCurve, SM_ERR_NULL_POINTER),
            (CornerInput::FilletBrepNotGiven, SM_ERR_NULL_POINTER),
            (CornerInput::TopologyMismatch, SM_ERR),
            (CornerInput::VertexNotProcessed, SM_ERR),
            (CornerInput::EventMalformed, SM_ERR),
        ] {
            let e = SolidError::blend(
                "blend::op",
                BlendFailure::CornerInputInvalid {
                    input,
                    entity: None,
                },
            );
            assert_eq!(e.kind(), ErrorKind::InvalidInput, "{e}");
            assert_eq!(e.upstream_status(), Some(status), "{e}");
            assert!(!e.is_defect() && !e.is_port_failure(), "{e}");
            assert!(!e.is_unported_or_cancelled(), "{e}");
            assert!(e.to_string().contains(&format!("{input:?}")), "{e}");
        }
        // the site's status overrides the input's
        let e = Failure::new(
            "blend::op",
            BlendFailure::CornerInputInvalid {
                input: CornerInput::EventNotSet,
                entity: None,
            },
        )
        .with_status(SM_ERR)
        .into_error();
        assert_eq!(e.upstream_status(), Some(SM_ERR), "{e}");
    }

    #[test]
    fn a_stage_failure_keeps_its_operation_status_and_origin_but_shows_neither() {
        let before = line!();
        let f =
            Failure::new("blend::op", BlendFailure::StageFailed).with_status(SM_ERR_NULL_POINTER);
        let after = line!();
        assert!((before..after).contains(&f.origin().line()));
        // the path is written with the host's separator (`\` on Windows), so compare its
        // components, not its text
        assert!(
            std::path::Path::new(f.origin().file()).ends_with("error/blend.rs"),
            "{}",
            f.origin().file()
        );
        let e = f.into_error();
        assert_eq!(e.operation(), "blend::op");
        assert_eq!(e.sm_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.upstream_status(), Some(SM_ERR_NULL_POINTER));
        assert_eq!(e.kind(), ErrorKind::StageFailed);
        assert!(e.is_defect());
        let shown = e.to_string();
        assert!(!shown.contains("1006") && !shown.contains(".rs"), "{shown}");
        // where a failure was built is not part of what failed
        let other = Failure::new("blend::op", BlendFailure::StageFailed)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        assert_eq!(e, other);
        // the helpers record their caller, not themselves
        let before = line!();
        let helper = SolidError::blend_failed("blend::op", "x");
        let after = line!();
        let at = helper.failure().unwrap().origin().line();
        assert!((before..after).contains(&at), "{at}");
    }

    #[test]
    fn the_stage_error_is_the_source_and_the_classes_follow_the_cause_chain() {
        use crate::blend::fillet_executive::NoRoomReason as R;
        use std::error::Error as _;
        let refusal = SolidError::blend("blend::stage", BlendFailure::NoRoom(R::BandsOverlap));
        let collapsed = Failure::new("blend::op", BlendFailure::Failed("the stage failed"))
            .caused_by(refusal.clone())
            .into_error();
        assert_eq!(
            collapsed.source().map(ToString::to_string),
            Some(refusal.to_string())
        );
        assert!(refusal.source().is_none());
        // a phrase collapse over a refusal reads as the refusal, not also as a defect
        assert!(!collapsed.is_defect() && collapsed.is_refusal() && collapsed.sized());
        assert!(!refusal.is_defect() && refusal.is_refusal());
        // so does a stage failure built on purpose over a refusal: the deepest class decides
        let stage = Failure::new("blend::op", BlendFailure::StageFailed)
            .caused_by(refusal.clone())
            .into_error();
        assert!(!stage.is_defect() && stage.is_refusal() && stage.sized());
        // and a refusal over a stage failure reads as the defect
        let defect = SolidError::blend("blend::stage", BlendFailure::StageFailed);
        assert!(defect.is_defect() && !defect.is_refusal());
        let over = Failure::new("blend::op", BlendFailure::NoRoom(R::BallDoesNotFit))
            .caused_by(defect)
            .into_error();
        assert!(over.is_defect() && !over.is_refusal() && !over.sized());
        // the message still shows the chain, as before
        assert_eq!(
            collapsed.to_string(),
            "blend::op: the stage failed; caused by blend::stage: bands_overlap: BandsOverlap"
        );
    }

    #[test]
    fn or_at_fills_empty_slots_and_never_changes_an_outcome() {
        use crate::error::Site;
        let edge = EntityRef::Edge(crate::topology::EdgeId::from_index(9));
        let point = crate::math::Point3::new(1.0, 2.0, 3.0);
        let at_edge = Site::edge(target_edge(1));
        let e = SolidError::blend("blend::op", BlendFailure::StageFailed)
            .or_at(at_edge)
            .or_at(Site::edge(target_edge(2)).or(Site::point(point)));
        let site = e.site().copied().unwrap();
        assert_eq!(site.edge, Some(target_edge(1)), "a set slot is kept");
        assert_eq!(site.at, Some(point), "an empty slot is filled");
        assert_eq!(
            e.to_string(),
            "blend::op: stage_failed at edge 1, (1, 2, 3)"
        );
        // errors without a record are returned as they are
        assert_eq!(
            SolidError::sm_err("blend::op").or_at(at_edge),
            SolidError::sm_err("blend::op")
        );
        // the decisions that read a failure's entity read the untyped entity, which `or_at`
        // never writes
        let open_base = |entity| {
            let failure = BlendFailure::Failed("a kept feature would leave the base open");
            match entity {
                Some(at) => SolidError::blend_at("blend::feature_executive", failure, at),
                None => SolidError::blend("blend::feature_executive", failure),
            }
        };
        for entity in [None, Some(edge)] {
            let e = open_base(entity);
            let filled = e.clone().or_at(at_edge.or(Site::point(point)));
            assert_eq!(filled.entity(), e.entity());
            assert_eq!(filled.upstream_status(), e.upstream_status());
            assert_eq!(filled.kind(), e.kind());
            assert_eq!(
                crate::blend::feature_executive::is_open_base_refusal(&filled),
                crate::blend::feature_executive::is_open_base_refusal(&e)
            );
        }
        assert!(crate::blend::feature_executive::is_open_base_refusal(
            &open_base(Some(edge))
        ));
        assert!(!crate::blend::feature_executive::is_open_base_refusal(
            &open_base(None)
        ));
    }

    /// The blend routines describe their failures: their library code builds no bare
    /// `Upstream` status, except in the files listed, each with the exact number of bare
    /// sites it still holds. A part of the typed-errors work that converts a file edits
    /// only that file's line: lowers its count, or removes the line once the file holds
    /// none.
    #[test]
    fn the_blend_routines_build_no_bare_statuses() {
        const WAITING: &[(&str, usize)] = &[];
        let found = super::super::query::tests::bare_statuses_in("blend", 100);
        let waiting: Vec<(String, usize)> = WAITING
            .iter()
            .map(|(file, n)| ((*file).to_owned(), *n))
            .collect();
        assert_eq!(found, waiting, "bare statuses per blend file");
        for (file, n) in WAITING {
            assert!(*n > 0, "{file} holds no bare status: remove its line");
        }
        assert!(
            WAITING.windows(2).all(|w| w[0].0 < w[1].0),
            "WAITING is sorted by path, one file per line"
        );
    }

    /// #19823: each cause the fillet executive names is no phrase, replaces the status it
    /// keeps, has its class (the executive's checks of the caller's arguments are
    /// `InvalidInput`, neither a defect nor a refusal), and is worded with its class.
    #[test]
    fn the_fillet_executive_causes_keep_their_status_and_class() {
        let rows = [
            (
                BlendFailure::NoEdgesGiven,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::SizeBelowResolution {
                    size: 1e-6,
                    tolerance: 1e-6,
                },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::PerEdgeListLength { edges: 2, given: 1 },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::ParameterListSizes {
                    edges: 3,
                    cross_sections: 2,
                    radius_types: 1,
                    radii: 1,
                },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::FaceHasNoSurface,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::NoFirstBody,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::TargetNotGiven,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::EdgeNotInTarget { index: 1 },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::EdgeNotFilletable { index: 1 },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::ChamferSizeInvalid,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::ChamferFaceNotOfEdge,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::SolverFaceIndexInvalid { index: 2 },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::GnSectionInvalid {
                    order: 9,
                    thumbweight: 0.5,
                },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::CrossSectionTypeUnknown,
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::RadiusTypeNotForEdges {
                    index: 0,
                    kind: crate::blend::fillet_solver::p1::FilletSolverType::SurfSurf,
                },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::VariableRadiusLawsNotGiven,
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
            ),
            (
                BlendFailure::VariableRadiusLawMissing { index: 2 },
                ErrorKind::InvalidInput,
                SM_ERR_INVALID_INPUT,
            ),
            (
                BlendFailure::BevelVertexNotFilleted,
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
            ),
            (
                BlendFailure::LawIntervalEmpty,
                ErrorKind::InvalidInput,
                SM_ERR,
            ),
            // not the executive's: the rational round's check of the exact blend it is given
            (
                BlendFailure::ExactBlendAxisCannotPlaceEdge {
                    length: 0.0,
                    discriminant: 1.0,
                    crossing: None,
                },
                ErrorKind::InvalidInput,
                SM_ERR,
            ),
            (
                BlendFailure::EdgeWithoutCurve,
                ErrorKind::InvalidInput,
                SM_ERR_NULL_POINTER,
            ),
            (BlendFailure::LawHasNoValue, ErrorKind::Unsupported, SM_ERR),
            (
                BlendFailure::SequenceCurveNotBSpline { kind: "offset" },
                ErrorKind::Unsupported,
                SM_ERR_NULL_POINTER,
            ),
            (
                BlendFailure::JoinCurveNotBSpline { kind: "offset" },
                ErrorKind::Unsupported,
                SM_ERR_NULL_POINTER,
            ),
            (
                BlendFailure::FilletCrossesItself {
                    first: 0,
                    second: 1,
                },
                ErrorKind::Unsupported,
                SM_ERR,
            ),
            (BlendFailure::CornerRefused, ErrorKind::Failed, SM_ERR),
            (BlendFailure::ExecutiveStepFailed, ErrorKind::Failed, SM_ERR),
        ];
        for (cause, class, status) in rows {
            assert!(!cause.is_phrase(), "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert_eq!(cause.class(), class, "{cause:?}");
            assert_eq!(cause.comparison_status(), status, "{cause:?}");
            let text = cause.to_string();
            assert!(text.starts_with(&format!("{class}: ")), "{text}");
            let error = SolidError::blend("blend::op", cause);
            assert_eq!(error.kind(), class, "{error:?}");
            assert_eq!(error.upstream_status(), Some(status), "{error:?}");
            assert!(!error.is_defect() && !error.is_refusal(), "{error:?}");
        }
        assert_eq!(
            BlendFailure::PerEdgeListLength { edges: 2, given: 1 }.to_string(),
            "invalid_input: a list of one entry per edge holds 1 entries for 2 edges"
        );
    }

    /// #19824: the causes the fillet session's helpers name are no phrases, replace the
    /// status they keep, have their class (neither a defect nor a refusal), and are worded
    /// with their class.
    #[test]
    fn the_fillet_session_causes_keep_their_status_and_class() {
        let rows = [
            (
                BlendFailure::EdgeSplitDeclined { parameter: 0.25 },
                ErrorKind::Failed,
                SM_ERR,
            ),
            (
                BlendFailure::LawCornerNotFound {
                    corner: 1,
                    what: LawCornerSearch::SharedEdge,
                },
                ErrorKind::NotFound,
                SM_ERR_NULL_POINTER,
            ),
            (
                BlendFailure::CornerPatchFaceNotFound { surface: 0 },
                ErrorKind::NotFound,
                SM_ERR_NULL_POINTER,
            ),
        ];
        for (cause, class, status) in rows {
            assert!(!cause.is_phrase(), "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert_eq!(cause.class(), class, "{cause:?}");
            assert_eq!(cause.comparison_status(), status, "{cause:?}");
            let text = cause.to_string();
            assert!(text.starts_with(&format!("{class}: ")), "{text}");
            let error = SolidError::blend("blend::op", cause);
            assert_eq!(error.kind(), class, "{error:?}");
            assert_eq!(error.upstream_status(), Some(status), "{error:?}");
            assert!(!error.is_defect() && !error.is_refusal(), "{error:?}");
        }
        assert_eq!(
            BlendFailure::LawCornerNotFound {
                corner: 2,
                what: LawCornerSearch::CornerEdge,
            }
            .to_string(),
            "not_found: law corner 2 has no edge between the rail points on a side's face"
        );
        // the caller's input: an edge use without an edge, a stale surface binding
        for cause in [
            BlendFailure::EdgeUseWithoutEdge,
            BlendFailure::BoundSurfaceRemoved,
        ] {
            assert!(!cause.is_phrase(), "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            let error = SolidError::blend("blend::op", cause);
            assert_eq!(error.kind(), ErrorKind::InvalidInput, "{error:?}");
            assert_eq!(
                error.upstream_status(),
                Some(SM_ERR_NULL_POINTER),
                "{error:?}"
            );
            assert!(!error.is_defect(), "{error:?}");
        }
        // a face or edge of the caller's body without its geometry keeps the status of the
        // stage that read it
        let face = Failure::new("blend::op", BlendFailure::FaceHasNoSurface)
            .with_status(SM_ERR_NULL_POINTER)
            .into_error();
        assert_eq!(face.kind(), ErrorKind::InvalidInput, "{face:?}");
        assert_eq!(
            face.upstream_status(),
            Some(SM_ERR_NULL_POINTER),
            "{face:?}"
        );
    }

    /// #19825: each cause the ease of a round before a setback names is no phrase, replaces
    /// the status it keeps, has its class (a missing input is `Missing`, neither a defect
    /// nor a refusal; a stop has its stop's kind), and is worded with its class.
    #[test]
    fn the_ease_causes_keep_their_status_and_class() {
        let mut rows = Vec::new();
        for input in [
            EaseInput::CornerVertex,
            EaseInput::SetbackFace,
            EaseInput::RoundSolver,
            EaseInput::SurfaceGenerator,
            EaseInput::RoundRail,
            EaseInput::RoundSurface,
            EaseInput::CentreLine,
            EaseInput::SolverSurface,
            EaseInput::EdgeCurve,
            EaseInput::FaceSurface,
            EaseInput::EdgeUseFace,
        ] {
            rows.push((
                BlendFailure::EaseInputMissing(input),
                ErrorKind::Missing,
                SM_ERR_NULL_POINTER,
            ));
        }
        for (stop, class) in [
            (EaseStop::SetbackVertexNotPlaced, ErrorKind::Unsupported),
            (EaseStop::SetbackNotOnOneRound, ErrorKind::Unsupported),
            (EaseStop::SectionsNotCircular, ErrorKind::Unsupported),
            (EaseStop::SetbackFaceCarriesNoRail, ErrorKind::NotFound),
            (EaseStop::SetbackVertexOffRail, ErrorKind::NotFound),
            (EaseStop::EasePastEdgeEnd, ErrorKind::OutsideDomain),
            (EaseStop::PlaneOriginNotOneClosest, ErrorKind::NotFound),
            (EaseStop::NoSectionInPlane, ErrorKind::NotFound),
            (EaseStop::EaseStartOffRail, ErrorKind::NotFound),
            (EaseStop::EaseHasNoLength, ErrorKind::Degenerate),
            (EaseStop::RailNotOneUvCurve, ErrorKind::Unsupported),
            (EaseStop::UvCurveOffRailInterval, ErrorKind::Unsupported),
            (EaseStop::NoDirectionAlongRails, ErrorKind::Unsupported),
        ] {
            rows.push((BlendFailure::EaseNotBuilt(stop), class, SM_ERR));
        }
        for geometry in [
            EaseGeometry::RoundSurface,
            EaseGeometry::RailCurve,
            EaseGeometry::TracedUvCurve,
            EaseGeometry::DroppedUvCurve,
        ] {
            rows.push((
                BlendFailure::EaseGeometryNotBSpline(geometry),
                ErrorKind::Unsupported,
                SM_ERR_NULL_POINTER,
            ));
        }
        rows.push((
            BlendFailure::ArcLengthNotConverged {
                start: 0.5,
                length: 2.0,
                stopped_at: 1.25,
            },
            ErrorKind::NotConverged,
            SM_ERR_NOT_CONVERGING,
        ));
        for (cause, class, status) in rows {
            assert!(!cause.is_phrase(), "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert_eq!(cause.class(), class, "{cause:?}");
            assert_eq!(cause.comparison_status(), status, "{cause:?}");
            let text = cause.to_string();
            assert!(text.starts_with(&format!("{class}: ")), "{text}");
            let error = SolidError::blend("blend::op", cause);
            assert_eq!(error.kind(), class, "{error:?}");
            assert_eq!(error.upstream_status(), Some(status), "{error:?}");
            assert!(!error.is_defect(), "{error:?}");
            assert!(!error.is_refusal(), "{error:?}");
        }
        assert_eq!(
            BlendFailure::EaseNotBuilt(EaseStop::EasePastEdgeEnd).to_string(),
            "outside_domain: the setback plus the ease runs past the end of the filleted edge"
        );
    }

    /// #19825: each cause the overflows name is no phrase, replaces the status it
    /// keeps, has its class (a missing input is `Missing`, neither a defect nor a refusal:
    /// the overflow's helpers read the caller's body and the executive's sheet alike), and
    /// is worded with its class.
    #[test]
    fn the_cliff_overflow_causes_keep_their_status_and_class() {
        let mut rows = Vec::new();
        for input in [
            OverflowInput::FaceSurface,
            OverflowInput::EdgeCurve,
            OverflowInput::EdgeStartVertex,
            OverflowInput::EdgeEndVertex,
            OverflowInput::EdgeOtherVertex,
        ] {
            rows.push((
                BlendFailure::OverflowInputMissing(input),
                ErrorKind::Missing,
                SM_ERR_NULL_POINTER,
            ));
        }
        for drop in [
            OverflowDrop::EdgePieceEndOntoEdge,
            OverflowDrop::EdgeMiddleOntoFilletFace,
            OverflowDrop::LoopSampleOntoSurface,
        ] {
            rows.push((
                BlendFailure::OverflowPointDoesNotDrop(drop),
                ErrorKind::NotFound,
                SM_ERR,
            ));
        }
        rows.push((
            BlendFailure::ExtensionEndOnTwoSideLines,
            ErrorKind::Unsupported,
            SM_ERR,
        ));
        rows.push((
            BlendFailure::NoBlendedEdgeNearExtension,
            ErrorKind::NotFound,
            SM_ERR,
        ));
        for (cause, class, status) in rows {
            assert!(!cause.is_phrase(), "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert_eq!(cause.class(), class, "{cause:?}");
            assert_eq!(cause.comparison_status(), status, "{cause:?}");
            let text = cause.to_string();
            assert!(text.starts_with(&format!("{class}: ")), "{text}");
            let error = SolidError::blend("blend::op", cause);
            assert_eq!(error.kind(), class, "{error:?}");
            assert_eq!(error.upstream_status(), Some(status), "{error:?}");
            assert!(!error.is_defect(), "{error:?}");
        }
        assert_eq!(
            BlendFailure::OverflowInputMissing(OverflowInput::FaceSurface).to_string(),
            "missing: a face a blend's overflow reads has no surface"
        );
    }

    /// #19825: each cause of a kept feature or a trim at the creases that is not built is no
    /// phrase, replaces the status `SM_ERR` it keeps, has its stop's class (none a defect or
    /// a refusal), and is worded with its class.
    #[test]
    fn the_kept_feature_and_crease_trim_causes_keep_their_status_and_class() {
        let mut rows = Vec::new();
        for (stop, class) in [
            (KeepStop::WallsNotUpright, ErrorKind::Unsupported),
            (KeepStop::WallsNotAllCopied, ErrorKind::Failed),
            (KeepStop::WallMeetsNoRound, ErrorKind::NotFound),
            (KeepStop::RoundNotClosed, ErrorKind::NotFound),
            (KeepStop::RoundNotSewn, ErrorKind::Failed),
        ] {
            rows.push((BlendFailure::KeptFeatureNotBuilt(stop), class));
        }
        for (stop, class) in [
            (
                CreaseTrimStop::NoSectionThroughEndVertex,
                ErrorKind::Degenerate,
            ),
            (
                CreaseTrimStop::PieceOnBoundaryAwayFromCreases,
                ErrorKind::Unsupported,
            ),
            (
                CreaseTrimStop::CornerBetweenAddingAndTaking,
                ErrorKind::Unsupported,
            ),
            (
                CreaseTrimStop::SectionEndAlongUncutEdge,
                ErrorKind::Unsupported,
            ),
            (CreaseTrimStop::TakesOffNothing, ErrorKind::NotFound),
            (CreaseTrimStop::ReachesEveryFace, ErrorKind::NotFound),
            (CreaseTrimStop::MovedEdgeLeavesGeometry, ErrorKind::Failed),
            (CreaseTrimStop::NotSewn, ErrorKind::Failed),
        ] {
            rows.push((BlendFailure::CreaseTrimNotBuilt(stop), class));
        }
        for (cause, class) in rows {
            assert!(!cause.is_phrase(), "{cause:?}");
            assert!(cause.replaces_upstream_status(), "{cause:?}");
            assert_eq!(cause.class(), class, "{cause:?}");
            assert_eq!(cause.comparison_status(), SM_ERR, "{cause:?}");
            let text = cause.to_string();
            assert!(text.starts_with(&format!("{class}: ")), "{text}");
            let error = SolidError::blend("blend::op", cause);
            assert_eq!(error.kind(), class, "{error:?}");
            assert_eq!(error.upstream_status(), Some(SM_ERR), "{error:?}");
            assert!(!error.is_defect() && !error.is_refusal(), "{error:?}");
        }
        assert_eq!(
            BlendFailure::CreaseTrimNotBuilt(CreaseTrimStop::NotSewn).to_string(),
            "failed: a blend past its faces does not sew into the body along its creases and \
             rails"
        );
    }

    /// The `BlendFailure` variants the per-file scan (`phrase_sites::BLEND`) counts are
    /// exactly the phrase causes (`phrase_causes!`, which [`BlendFailure::is_phrase`] is
    /// built from), so the per-file pins in `error/phrase_sites.rs` see every phrase cause.
    #[test]
    fn the_phrase_scan_lists_exactly_the_phrase_variants() {
        use super::super::site_scan::Builder;
        let scanned: Vec<&str> = super::super::phrase_sites::BLEND
            .builders
            .iter()
            .filter_map(|b| match b {
                Builder::Variant("BlendFailure", v) => Some(*v),
                _ => None,
            })
            .collect();
        macro_rules! names {
            (; $($variant:ident),*) => {
                vec![$(stringify!($variant)),*]
            };
        }
        let mut names: Vec<&str> = phrase_causes!(names());
        names.sort_unstable();
        let mut scanned = scanned;
        scanned.sort_unstable();
        assert_eq!(
            scanned, names,
            "phrase_sites::BLEND must list exactly the phrase variants of BlendFailure"
        );
    }

    /// The G1-knot loop's catch of `find_fillet_tsect_pnt` is not an empty search: it keeps
    /// `SM_ERR`, chains the caught error, reads as neither defect nor refusal on its own
    /// (so an invalid-input cause is not reported as `NotFound`), and a defect cause makes
    /// it a defect.
    #[test]
    fn cross_section_point_failed_passes_its_cause_on() {
        let wrap = |inner: SolidError| {
            Failure::new(
                "blend::op",
                BlendFailure::Solver(SolverFailure::CrossSectionPointFailed { t: 0.5 }),
            )
            .caused_by(inner)
            .into_error()
        };
        let inner = SolidError::blend_solver("blend::inner", SolverFailure::EdgeUseWithoutMate);
        assert_eq!(inner.kind(), ErrorKind::InvalidInput);
        let e = wrap(inner.clone());
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert_eq!(e.kind(), ErrorKind::Failed);
        assert_eq!(e.cause(), Some(&inner));
        assert!(!e.is_defect() && !e.is_refusal());
        let inner = SolidError::blend_null_defect("blend::inner", "why");
        let e = wrap(inner.clone());
        assert_eq!(e.sm_status(), Some(SM_ERR));
        assert!(e.is_defect() && inner.is_defect());
    }

    /// The causes (every enum of this module and of `error/blend/`) and every match on their
    /// variants (but for one with a `_` arm) are kept in name order, one arm per pattern, so changes that each add a cause or its arms touch
    /// different lines. Names compare as bytes.
    #[test]
    fn the_causes_and_the_arms_of_their_matches_stay_sorted_by_name() {
        /// The identifier `text` starts with.
        fn ident(text: &str) -> &str {
            let end = text
                .find(|c: char| !(c.is_ascii_alphanumeric() || c == '_'))
                .unwrap_or(text.len());
            &text[..end]
        }
        /// The variant `Path::Name` that `text` starts with, and the text after it; `""` and
        /// `text` where it starts with none.
        fn variant(text: &str) -> (&str, &str) {
            let path = ident(text);
            match text[path.len()..].strip_prefix("::") {
                Some(after) if !path.is_empty() => {
                    let name = ident(after);
                    (name, &after[name.len()..])
                }
                _ => ("", text),
            }
        }
        let blend = include_str!("blend.rs");
        let modules: Vec<String> = blend
            .lines()
            .filter_map(|l| {
                Some(format!(
                    "blend/{}.rs",
                    l.strip_prefix("pub mod ")
                        .or_else(|| l.strip_prefix("mod "))?
                        .strip_suffix(';')?
                ))
            })
            .collect();
        let files = [
            ("blend.rs", blend),
            ("blend/band.rs", include_str!("blend/band.rs")),
            ("blend/cliff.rs", include_str!("blend/cliff.rs")),
            ("blend/corner.rs", include_str!("blend/corner.rs")),
            ("blend/crease_trim.rs", include_str!("blend/crease_trim.rs")),
            ("blend/crossing.rs", include_str!("blend/crossing.rs")),
            ("blend/ease.rs", include_str!("blend/ease.rs")),
            ("blend/keep.rs", include_str!("blend/keep.rs")),
            ("blend/law_corner.rs", include_str!("blend/law_corner.rs")),
            ("blend/no_room.rs", include_str!("blend/no_room.rs")),
            ("blend/overflow.rs", include_str!("blend/overflow.rs")),
            ("blend/removal.rs", include_str!("blend/removal.rs")),
            ("blend/section.rs", include_str!("blend/section.rs")),
            ("blend/set_up.rs", include_str!("blend/set_up.rs")),
            ("blend/solver.rs", include_str!("blend/solver.rs")),
            ("blend/trace.rs", include_str!("blend/trace.rs")),
            ("blend/weld.rs", include_str!("blend/weld.rs")),
        ];
        let listed: Vec<&str> = files.iter().skip(1).map(|(file, _)| *file).collect();
        assert_eq!(
            listed, modules,
            "every `error/blend/` module is listed here, in order"
        );
        let mut enums = 0;
        let mut matches = 0;
        for (file, src) in files {
            // the library code: the tests' own matches are not causes
            let lines: Vec<&str> = src.lines().take_while(|l| *l != "mod tests {").collect();
            for (i, line) in lines.iter().enumerate() {
                let t = line.trim_start();
                let indent = line.len() - t.len();
                let is_enum = t.starts_with("pub enum ") && t.ends_with(" {");
                let is_match =
                    t.ends_with(" {") && (t.starts_with("match ") || t.contains("=> match "));
                if !is_enum && !is_match {
                    continue;
                }
                let pad = " ".repeat(indent + 4);
                let close = format!("{}}}", " ".repeat(indent));
                let close_arm = format!("{close},");
                let mut names = Vec::new();
                let mut wildcard = false;
                for l in lines[i + 1..]
                    .iter()
                    .take_while(|l| **l != close && **l != close_arm)
                {
                    let Some(rest) = l.strip_prefix(pad.as_str()) else {
                        continue;
                    };
                    assert!(
                        !(is_match && rest.starts_with("| ")),
                        "{file}:{}: one arm per pattern, not `|` groups: {l}",
                        i + 1
                    );
                    let pattern = rest.split("=>").next().unwrap_or_default();
                    assert!(
                        !(is_match
                            && rest.starts_with(char::is_alphabetic)
                            && pattern.contains(" | ")),
                        "{file}:{}: one arm per pattern, not `|` groups: {l}",
                        i + 1
                    );
                    wildcard |= rest.starts_with("_ ");
                    // a match arm's variant, and the variant nested in its payload
                    // (`Self::WeldNotBuilt(WeldGap::CurvedJoint)`), each `Path::Name`
                    let name = if is_enum {
                        (ident(rest), "")
                    } else {
                        let (outer, after) = variant(rest);
                        let inner = after.strip_prefix('(').map_or("", |p| variant(p).0);
                        (outer, inner)
                    };
                    if name.0.starts_with(|c: char| c.is_ascii_uppercase()) {
                        names.push(name);
                    }
                }
                if wildcard || names.is_empty() {
                    continue;
                }
                for w in names.windows(2) {
                    assert!(
                        w[0] < w[1] || (is_match && w[0] == w[1]),
                        "{file}:{}: `{:?}` is before `{:?}`: keep them sorted by name",
                        i + 1,
                        w[0],
                        w[1]
                    );
                }
                if is_enum {
                    enums += 1;
                } else {
                    matches += 1;
                }
            }
        }
        assert!(
            enums >= 20 && matches >= 24,
            "{enums} enums and {matches} matches checked"
        );
    }
}
