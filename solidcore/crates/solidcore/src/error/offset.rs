//! Why an offset, shell, thicken or local operation is refused, in solidcore's terms.
//!
//! Upstream: none. These describe the refusals of the offset subsystem
//! (`crate::offset`). Each also records, for difftest only, the status the C++ (or the
//! SMLib fork the refusal comes from) reports for it (crate-private
//! `comparison_status`, read through [`crate::SolidError::sm_status`]).
//!
//! | Rust                                   | Status difftest compares                  |
//! |----------------------------------------|-------------------------------------------|
//! | [`OffsetFailure::BlendRailWithoutSupport`], [`OffsetFailure::FollowedBlendIncomplete`] | `SM_ERR_NULL_POINTER` (the face move's) |
//! | [`OffsetFailure::Collapses`]           | `SM_ERR` (#2350, solidcore-native)        |
//! | [`OffsetFailure::Defect`]              | the site's (`SM_ERR`, or another through `with_status`) |
//! | [`OffsetFailure::DistanceUnresolved`]  | `SM_ERR_INVALID_INPUT` (solidcore-native) |
//! | [`OffsetFailure::DraftClosesWall`]     | `SM_ERR` (#20767, solidcore-native)       |
//! | [`OffsetFailure::EdgeHasNoCurve`], [`OffsetFailure::FaceHasNoSurface`], [`OffsetFailure::FaceHasNoOuterFaceuse`], [`OffsetFailure::VertexNotGiven`], [`OffsetFailure::EdgeuseNotGiven`], [`OffsetFailure::CurveKindUnsupported`], [`OffsetFailure::EdgeuseHasNoUvTrim`], [`OffsetFailure::UnnamedFaceHasNoSurface`] | `SM_ERR_NULL_POINTER`, or the site's through `with_status` |
//! | [`OffsetFailure::EdgeVanishes`]        | `SM_ERR_OFFSET_EDGE_VANISHES` (fork 34318f4) |
//! | [`OffsetFailure::ExactCurveUncut`]     | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::FaceCloses`]          | `SM_ERR` (#19904, solidcore-native)       |
//! | [`OffsetFailure::FaceMoveEmpty`]       | `SM_ERR_INVALID_INPUT` (the face move's)  |
//! | [`OffsetFailure::FacesApart`]          | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::FacesMoveRefused`], [`OffsetFailure::BlendSectionNotFound`], [`OffsetFailure::MoveCollapses`] | `SM_ERR` (the face move's) |
//! | [`OffsetFailure::Folds`]               | `SM_ERR` (WR-718, solidcore-native)       |
//! | [`OffsetFailure::LoopGapEndsAmbiguous`] | `SM_ERR`                                 |
//! | [`OffsetFailure::LoopGapPassLimit`]    | `SM_ERR`                                  |
//! | [`OffsetFailure::LoopGapUncertified`]  | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::LoopsCross`]          | `SM_ERR_BAD_INTERSECTIONS` (WR-345)       |
//! | [`OffsetFailure::LooseTangentEdge`]    | `SM_ERR` (WR-530, solidcore-native)       |
//! | [`OffsetFailure::OffsetNotBuilt`]     | `SM_ERR` (`OffsetBrep`'s `SER(SM_ERR)`)   |
//! | [`OffsetFailure::ExtendedCurveOffDomain`] | `SM_ERR` (#21743, solidcore-native)    |
//! | [`OffsetFailure::PastSide`]            | `SM_ERR` (#18564, solidcore-native)       |
//! | [`OffsetFailure::PeriodicBranchAmbiguous`] | `SM_ERR` (solidcore-native)            |
//! | [`OffsetFailure::ReachUncertified`]    | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::SeamOpened`]          | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::SplitAtEdgeEnd`]      | `SM_ERR`                                  |
//! | [`OffsetFailure::TangentBranchUncertified`] | `SM_ERR` (solidcore-native)            |
//! | [`OffsetFailure::TangentEndOffCurve`]  | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::ToleranceUnresolved`] | `SM_ERR_INVALID_INPUT` (solidcore-native, WR-749) |
//! | [`OffsetFailure::TubeFolds`]           | `SM_ERR` (#13685, solidcore-native)       |
//! | [`OffsetFailure::TubeNotCertified`]    | `SM_ERR` (#13685, solidcore-native)       |
//! | [`OffsetFailure::Unsound`]             | `SM_ERR` (solidcore-native)               |
//! | [`OffsetFailure::WallsMeetAcross`]     | `SM_ERR` (#18239, solidcore-native)       |
//! | [`OffsetFailure::WallsTouch`]          | `SM_ERR_OFFSET_WALLS_TOUCH` (fork a759622) |
//! | the other causes of `offset_geometry_creation` ([`OffsetFailure::VertexCurveNotArc`] to [`OffsetFailure::VertexArcsNotOneSphere`]) | `SM_ERR` |
//! | [`OffsetFailure::OffsetBaseKindUnsupported`], [`OffsetFailure::GapCornerBlendNotMade`] | `SM_ERR` (`offset_executive::p1`) |
//! | [`OffsetFailure::EdgeOffsetsUnpaired`], [`OffsetFailure::GapEndWithoutEdge`], [`OffsetFailure::FaceuseNotGiven`], [`OffsetFailure::SelfIntersectionWithoutUvCurve`], [`OffsetFailure::FaceuseHasNoFace`] | `SM_ERR_NULL_POINTER` (`offset_executive::p1`) |
//! | [`OffsetFailure::BrepNotGiven`] | `SM_ERR_NULL_POINTER`, or the site's through `with_status` (`offset_executive::p2`) |
//! | [`OffsetFailure::ShellLeavesNoCavity`], [`OffsetFailure::ShellWallUnresolved`], [`OffsetFailure::ShellFaceNotOfBody`] | `SM_ERR_INVALID_INPUT` (`offset_executive::p2`) |
//! | [`OffsetFailure::ExtendedFaceCollapses`], [`OffsetFailure::TrimPastExtension`] | `SM_ERR` (`offset_executive::p2`) |
//! | [`OffsetFailure::FaceHasNoFaceuse`], [`OffsetFailure::FaceuseHasNoShell`], [`OffsetFailure::ShellHasNoRegion`], [`OffsetFailure::FaceuseHasNoMate`], [`OffsetFailure::EdgeuseHasNoEdge`], [`OffsetFailure::EdgeuseHasNoMate`], [`OffsetFailure::EdgeuseHasNoRadial`], [`OffsetFailure::EdgeuseHasNoFaceuse`] | `SM_ERR_NULL_POINTER` (`offset_executive::p1`, `offset_executive::p2`) |
//!
//! A stage of an offset routine that fails as the kernel's fault is an
//! [`OffsetFailure::Defect`] with its reason, a defect ([`ErrorKind::StageFailed`]).
//!
//! Deviations: not upstream. Entity ids are those of the operation's input B-rep. The causes
//! named at the sites of `offset_faces`, `offset_geometry_creation` and
//! `offset_executive/p1` that returned a bare status
//! ([`OffsetFailure::FacesMoveRefused`] and those after it) answer with that status too: they
//! replaced one ([`OffsetFailure::is_failed_routine`]).

use super::{
    EntityRef, ErrorKind, Failure, Reason, SM_ERR, SM_ERR_BAD_INTERSECTIONS, SM_ERR_INVALID_INPUT,
    SM_ERR_NULL_POINTER, SM_ERR_OFFSET_EDGE_VANISHES, SM_ERR_OFFSET_WALLS_TOUCH,
};
use crate::offset::offset_faces::OffsetFacesRefusal;
use crate::topology::{EdgeId, EdgeuseId, FaceId, FaceuseId, ShellId};

/// Why an offset, shell, thicken or local operation is refused.
///
/// A caller outside the crate matches it with a wildcard: a reason it does not name yet still
/// has its class ([`OffsetFailure::kind`]) and says whether it is a defect
/// ([`OffsetFailure::is_defect`]), which every reason must answer here.
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum OffsetFailure {
    /// A rail `edge` of a blend that follows a moved face has no face across it from the
    /// blend (the blend is on both its sides, or a side has no face): the blend rolls on no
    /// face there, so it cannot be taken off and put back.
    BlendRailWithoutSupport {
        /// The rail, of the operation's input.
        edge: EdgeId,
    },
    /// The section of the blend `face` that follows a moved face is not found: the midpoint
    /// of one rail does not drop onto the other rail, or a rail's point onto the blend's own
    /// surface, so the angle between the faces it rolls on cannot be measured.
    BlendSectionNotFound {
        /// The blend face, of the operation's input.
        face: FaceId,
    },
    /// The operation was given no B-rep (a null body: upstream's `NER`, or its native 1007
    /// at the shell and offset APIs).
    BrepNotGiven,
    /// The offset of `face` by `distance` collapses onto a line or point: the face has a
    /// radius of curvature `radius` on the displaced side that the distance reaches, within
    /// the B-rep's tolerance (a round inset by its radius). The offset is no surface there,
    /// and the faces beside it would have to meet across it, which the offset does not
    /// build (#2350).
    ///
    /// Also an extended inset's face whose offset reaches or passes such a radius (WR-346:
    /// the face is dropped and its neighbours mated across it) where that mating cannot
    /// bridge it: there the face is not built inside out, it is left out, so what fails is
    /// the collapse, not a fold. `radius` is then a radius the face attains, sampled, not
    /// a certified minimum, or, where the proof finds none, `|distance|`: an upper bound
    /// (within the tolerance) on the face's least radius, not one it is known to attain.
    Collapses {
        /// The face whose offset collapses.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// A radius of curvature the face attains on the displaced side, or (WR-346, where
        /// no radius is proved) `|distance|`, an upper bound on its least radius.
        radius: f64,
    },
    /// Where an offset edge's curve meets a neighbour's offset surface, the curve has no
    /// tangent or the surface no tangent plane: whether the curve passes through the surface
    /// or touches it cannot be decided there.
    CrossingUndecided,
    /// The curve of an edge is of a kind the routine does not handle: none of the B-spline
    /// curve kinds (B-spline, line, ellipse, hyperbola, parabola) whose B-spline data it
    /// reads. `edge` is the edge of the input where it is one (`None` for an edge the offset
    /// made, whose curve came from the input's or a construction); `curve_type` is the
    /// curve's class type.
    CurveKindUnsupported {
        /// The edge, of the operation's input, where it is one.
        edge: Option<EdgeId>,
        /// The curve's class type (`GetType`).
        curve_type: i64,
    },
    /// A failure that is the kernel's fault, reviewed at its site, with the written reason:
    /// `why` names the step that failed and why that is the kernel's fault, not the
    /// geometry's. A defect ([`ErrorKind::StageFailed`]); status `SM_ERR` unless its site
    /// says otherwise.
    Defect {
        /// The step that failed, and why that is the kernel's fault.
        why: &'static str,
    },
    /// The offset distance is not a finite length at least the input B-rep's tolerance,
    /// the distance below which the model does not tell two points apart: an offset
    /// that close lies within tolerance of the face it comes from.
    DistanceUnresolved {
        /// The offset distance asked for (its magnitude, as the executive takes it).
        distance: f64,
        /// The smallest distance the input resolves: its B-rep tolerance.
        resolution: f64,
    },
    /// A draft of `face`, a cone or cylinder about the pull, would carry its drafted cone
    /// into another shell of the body (a cavity's wall, or another lump), first at `wall`,
    /// a face of that shell: the wall between them would close, and the faces cross.
    /// `largest` is the largest draft angle that keeps a wall, radians
    /// ([`crate::offset::draft::largest_clear_angle`], #20767). Refused before anything
    /// changes.
    DraftClosesWall {
        /// The drafted face, of the operation's input.
        face: FaceId,
        /// The face of the other shell the drafted cone reaches first.
        wall: FaceId,
        /// The largest draft angle that keeps a wall, radians.
        largest: f64,
    },
    /// Edge `edge` of the body the operation was given has no curve (`None` where the handle
    /// names no edge of the input).
    EdgeHasNoCurve {
        /// The edge, of the operation's input.
        edge: Option<EdgeId>,
    },
    /// After the synchronization, the offset edge uses on one side of edge `edge` outnumber
    /// those on the other: a split at the other side's break was refused (too near an end of
    /// a piece), so the pieces do not pair one to one and the caps between them are not made.
    EdgeOffsetsUnpaired {
        /// The edge, of the operation's input.
        edge: EdgeId,
    },
    /// The offset moves a vertex along an edge to or past the edge's other end: the
    /// edge would vanish, a change of topology the offset does not make.
    EdgeVanishes {
        /// The edge, when known.
        edge: Option<EdgeId>,
        /// A face the edge bounds, when the edge itself is not known.
        face: Option<FaceId>,
    },
    /// An edge use of the operation's input names no edge. A caller can build such a body
    /// (`Brep::edgeuse_mut`), so it is the caller's input, not the kernel's fault.
    EdgeuseHasNoEdge {
        /// The edge use, of the operation's input.
        edgeuse: EdgeuseId,
    },
    /// An edge use of the operation's input has no face use (it is in no loop of a face).
    /// A caller can build such a body (`Brep::edgeuse_mut`, `Brep::loopuse_mut`), so it is
    /// the caller's input, not the kernel's fault.
    EdgeuseHasNoFaceuse {
        /// The edge use, of the operation's input.
        edgeuse: EdgeuseId,
    },
    /// An edge use of the operation's input has no mate. A caller can build such a body
    /// (`Brep::add_edgeuse_record`), so it is the caller's input, not the kernel's fault.
    EdgeuseHasNoMate {
        /// The edge use, of the operation's input.
        edgeuse: EdgeuseId,
    },
    /// An edge use of the operation's input has no radial (no other use around its edge).
    /// A caller can build such a body (`Brep::add_edgeuse_record`), so it is the caller's
    /// input, not the kernel's fault.
    EdgeuseHasNoRadial {
        /// The edge use, of the operation's input.
        edgeuse: EdgeuseId,
    },
    /// An edge use of the edge `edge` has no UV trim curve (`None` where the handle names no
    /// edge of the input), which the cap between the edge's offsets is made from, and which
    /// the matching of an offset face's edge uses to the input's drops onto (the offset
    /// builds none).
    EdgeuseHasNoUvTrim {
        /// The edge, of the operation's input, where the handle names one.
        edge: Option<EdgeId>,
    },
    /// The mating of two offset edges was given no edge use for one of the four it takes
    /// (the two original edge uses and their offsets), or the synchronization of an edge's
    /// offsets no original edge use.
    EdgeuseNotGiven,
    /// The extended surfaces of the faces beside an edge meet on an exact curve (an
    /// analytic special case's line or conic) that could not be cut into its pieces inside
    /// both extended domains with their UV curves
    /// ([`crate::surface::surface::SurfaceIntersection::clip_exact_curves`]; the failure's
    /// cause says why). The edge's offsets cannot be mated on it.
    ExactCurveUncut,
    /// An offset edge's curve (its UV curve extended along its face's offset, or an
    /// intersection of the extended surfaces beside it) is cut at neither end where
    /// `TrimExtendedEdge` looks for a cut: no trim interval, so the edge is not extended or
    /// mated.
    ExtendedEdgeUntrimmed,
    /// The UV trim curve `ExtendTrimEdges` extends along an offset edge has the middle of
    /// its own interval outside the domain of the offset face's extended surface, `outside`
    /// from it: the curve is in another surface's parameters, so no piece of it lies on the
    /// extended surface and the edge cannot be extended along it (a lift there would run
    /// along the domain's boundary, where the surface clamps, not over the curve; #21743).
    ExtendedCurveOffDomain {
        /// The edge, of the operation's input, where the handle names one.
        edge: Option<EdgeId>,
        /// How far the curve's midpoint lies outside the extended domain, in parameters.
        outside: f64,
    },
    /// The offset surface of `face`, offset by `distance`, collapses (onto its spine or past
    /// it: a round offset by its radius or more) in an extended offset, which builds every
    /// face it offsets (only an inset that extends convex edges leaves such a face out and
    /// mates its neighbours across it). The collapse test that finds it gives no radius.
    ExtendedFaceCollapses {
        /// The face, of the operation's input.
        face: FaceId,
        /// The offset distance asked for (as the executive holds it).
        distance: f64,
    },
    /// In a local operation, `face`, a band bounded by the two faces `beside` (a ring between
    /// a countersink's cone and the well below it, or the cone itself between the ring and
    /// the hole above), closes to nothing as the faces move: its two sides, each where its
    /// surface meets a neighbour's new surface, pass over each other, so keeping the face
    /// would turn it inside out. Decided in closed form where
    /// the three surfaces are planes, cylinders and cones about one axis, before anything
    /// changes. `largest` is how far the faces could move before the band closes: the
    /// fraction of the move at which its two edges meet, times the move (#19904).
    FaceCloses {
        /// The face that closes, of the operation's input.
        face: FaceId,
        /// The faces across its two edges, of the operation's input.
        beside: [FaceId; 2],
        /// How far the faces could move before it closes.
        largest: f64,
    },
    /// The UV domain of an offset face's UV trim curves could not be found (they cover no
    /// part of the surface's domain, or the computation failed: the failure's cause says
    /// which), so its surface domain cannot be updated. [`ErrorKind::Failed`], neither a
    /// defect nor a refusal on its own, as the blend's `ExecutiveStepFailed`: the kept cause
    /// decides ([`crate::SolidError::is_defect`], [`crate::SolidError::is_refusal`]), and
    /// [`crate::SolidError::cause`] gives its own class.
    FaceDomainUnresolved,
    /// A face of the operation's input has no face use. A caller can build such a body
    /// (`Brep::face_mut`), so it is the caller's input, not the kernel's fault.
    FaceHasNoFaceuse {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// Face `face` of the body the operation was given has no face use on the outside of
    /// its body: none at all, or only an inner one with no mate.
    FaceHasNoOuterFaceuse {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// Face `face` of the body the operation was given has no surface.
    FaceHasNoSurface {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// A face move was given no face (`faces` is 0), or a distance that is zero or not a
    /// number.
    FaceMoveEmpty {
        /// How many faces it was given.
        faces: usize,
        /// The distance it was given.
        distance: f64,
    },
    /// In a local operation, the new surfaces of `face` and of its neighbour across an
    /// edge meet nowhere near the edge, so no extension of either makes them meet there
    /// and the edge cannot be found again. (The reach,
    /// [`crate::offset::offset_executive::reach::local_extension_reach`], no longer refuses
    /// it: it lists such edges in `LocalReach::apart`, where a step's wall closes the gap
    /// or the edge's offset fails.)
    FacesApart {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// A face move ([`crate::offset::offset_faces::OffsetFacesExecutive`]) refused, for
    /// `refusal`: a blend that follows a moved face cannot be taken off, followed or put back
    /// as it was, or the move moved no face. The executive's `reason` says it in words.
    FacesMoveRefused {
        /// The executive's refusal (`GetRefusal`).
        refusal: OffsetFacesRefusal,
    },
    /// A face use of the operation's input names no face. A caller can build such a body
    /// (`Brep::add_faceuse_record`), so it is the caller's input, not the kernel's fault.
    FaceuseHasNoFace {
        /// The face use, of the operation's input.
        faceuse: FaceuseId,
    },
    /// A face use of the operation's input has no mate. A caller can build such a body
    /// (`Brep::add_faceuse_record`), so it is the caller's input, not the kernel's fault.
    FaceuseHasNoMate {
        /// The face use, of the operation's input.
        faceuse: FaceuseId,
    },
    /// A face use of the operation's input is in no shell. A caller can build such a body
    /// (`Brep::faceuse_mut`), so it is the caller's input, not the kernel's fault.
    FaceuseHasNoShell {
        /// The face use, of the operation's input.
        faceuse: FaceuseId,
    },
    /// The face-use offset was given no face use.
    FaceuseNotGiven,
    /// The offset of `face` by `distance` folds: the wall is thicker than a radius of
    /// curvature of the face on its side (WR-718; every offset but the local operation).
    Folds {
        /// The face whose offset folds.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// The largest wall that works.
        largest: f64,
    },
    /// A followed blend handed to a step of the face move has no blend face or no rail though
    /// it is not marked taken off: the caller's list is not one the move's first step made.
    FollowedBlendIncomplete,
    /// The corner blend that closes a gap between an edge's offsets could not be made
    /// (`CreateCornerBlend`). [`ErrorKind::Failed`], neither a defect nor a refusal on its
    /// own: the blend's error is kept as the cause and decides
    /// ([`crate::SolidError::is_defect`]). `edge` is the input's edge whose offsets leave the
    /// gap.
    GapCornerBlendNotMade {
        /// The edge, of the operation's input.
        edge: EdgeId,
    },
    /// A gap between an edge's offsets ends at a vertex of a face beside it that has no edge
    /// (a vertex loop's, such as a cone's apex): the corner blend that would close the gap
    /// has no edge to blend to. `edge` is the input's edge whose offsets leave the gap.
    GapEndWithoutEdge {
        /// The edge, of the operation's input.
        edge: EdgeId,
    },
    /// A lamina edge (one face) given to `ExtendTrimEdges` with an offset edge use on its
    /// other side: the second UV curve is built only for an edge with a face on each side,
    /// so that side's offset edge is not extended.
    LaminaEdgeOtherSide,
    /// A local operation's geometry creator was asked for the surface of a face that is not
    /// among the faces it was given.
    LocalFaceNotListed,
    /// A gap in an offset face's loop is longer than `tolerance`, but its two ends are both
    /// within `tolerance` of the vertex the two edge uses share, or neither is, so the gap
    /// has no one end that the vertex closes. A defect ([`ErrorKind::StageFailed`]): the gap
    /// is in the offset loop the kernel built, and ends that close to the vertex are valid
    /// geometry near the tolerance (a gap between `tolerance` and twice it about the vertex)
    /// that the search does not handle.
    LoopGapEndsAmbiguous {
        /// The face, of the operation's input, whose offset left the gap, where it comes from
        /// one.
        face: Option<FaceId>,
        /// How far the end of the edge use before the gap is from the shared vertex.
        end_distance: f64,
        /// How far the start of the edge use after the gap is from the shared vertex.
        start_distance: f64,
        /// The distance within which an end is at the vertex: ten times the larger of the two
        /// edges' tolerances and the approximation tolerance. The factor is the offset's edge
        /// meeting tolerance, `10 * max(edge tolerance, approximation tolerance)`, ported from
        /// the fork's step walls (tangency-labs/smlib 5d428ae, WR-530 addendum) and used alike
        /// by `is_step_across_edge` and `extend_trim_edges`; no derivation of the ten is
        /// recorded.
        tolerance: f64,
    },
    /// The gap search of an offset face's loops closes one gap per pass and reached its
    /// limit of `limit` passes before a pass found no gap (the fork's `lGuard`): the face's
    /// loops had `limit` gaps or more, or a gap a pass did not really close was found again.
    /// A face with fewer gaps, each closed once, passes. A defect ([`ErrorKind::StageFailed`]):
    /// the gaps are in the offset loops the kernel built, so either the kernel's fixed pass
    /// limit stops a valid face, or the search does not converge.
    LoopGapPassLimit {
        /// The face, of the operation's input, whose offset left the gaps, where it comes
        /// from one.
        face: Option<FaceId>,
        /// The passes the search makes.
        limit: usize,
    },
    /// An offset face has a gap in its loops, and the straight segment that would bridge
    /// it could not be established within tolerance (the fork's
    /// `SmOffsetExecutive::CloseLoopGap`). Refused where the bridge construction is
    /// uncertified, before the result's soundness is judged.
    LoopGapUncertified {
        /// The face, of the operation's input, whose offset left the gap, where it comes
        /// from one.
        face: Option<FaceId>,
    },
    /// Loops of the offset of `face` cross in a way the offset does not resolve.
    LoopsCross {
        /// The face whose offset's loops cross, when known.
        face: Option<FaceId>,
        /// How they cross.
        crossing: LoopCrossing,
    },
    /// In a local operation, an edge tangent in the input whose new curve, lifted onto
    /// one face's new surface, lands on the other face's new surface farther than ten
    /// times the larger of the edge tolerance and the approximation tolerance (WR-530):
    /// only one of the two faces moved, so they no longer meet along the edge, and no
    /// later step of a local operation closes the gap.
    LooseTangentEdge {
        /// The tangent edge, of the operation's input, where the handle names one.
        edge: Option<EdgeId>,
        /// How far the new curve, dropped onto the other face's new surface, lands from
        /// it at most (`max_drop_to_surf`; NaN where the drop gave no finite distance).
        max_drop_to_surf: f64,
        /// The limit it was held to: ten times the larger of the edge tolerance and the
        /// approximation tolerance.
        meet_tol: f64,
    },
    /// The surface of the moved face `face`, offset by `distance`, collapses onto its spine
    /// or past it (a round moved in by its radius): there is no surface to move the face to.
    MoveCollapses {
        /// The face to move, of the operation's input.
        face: FaceId,
        /// The distance it was to move (signed, as the move takes it).
        distance: f64,
    },
    /// The offset of `face` by `distance` is neither proved regular nor proved to fold, so no
    /// wall size is named: the proof ran out of patches (where the face's normal vanishes, a
    /// flat patch whose boundary curves leave a corner tangent to each other; where the
    /// offset collapses; or where a fold point is not proved), or a fold was proved but the
    /// face does not bend towards that side anywhere the sizing looked.
    NotCertified {
        /// The face whose offset is not certified.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
    },
    /// The base of an implicit offset surface is of a kind with no B-spline form that
    /// `ReplaceImplicitOffsets` reads (an offset surface: an offset of an offset), so its
    /// offset is not approximated. `surface_type` is the base's class type.
    OffsetBaseKindUnsupported {
        /// The base surface's class type (`GetType`).
        surface_type: i64,
    },
    /// The solid offset (`DoSolidOffset`) failed: `OffsetBrep` answers its own `SM_ERR`
    /// over it, keeping the offset's error as the cause. [`ErrorKind::Failed`], neither a
    /// defect nor a refusal on its own: the kept cause decides
    /// ([`crate::SolidError::is_defect`], [`crate::SolidError::is_refusal`]).
    OffsetNotBuilt,
    /// In a local operation, the plane of `face` moves past where the surface of `side`, a
    /// face beside it across an edge, ends: the sphere's or the cylinder's extent along the
    /// plane's normal stops short of the new plane, so no extension of the side meets it.
    /// The two met at an angle before the move (a plane moved off a round it was tangent to
    /// is not this: a wall closes that gap). `largest` is how far the plane could move and
    /// still meet the side: the distance from its old place to the side's extent there
    /// (#18564). Refused before anything changes.
    PastSide {
        /// The moved face, of the operation's input.
        face: FaceId,
        /// The face beside it whose surface ends, of the operation's input.
        side: FaceId,
        /// How far the face could move and still meet the side.
        largest: f64,
    },
    /// More than one surface-surface intersection branch claims the same periodic
    /// boundary event, so the branch through that event is ambiguous.
    PeriodicBranchAmbiguous,
    /// The two rails of an edge's cap run over different parameter intervals (`first`,
    /// `second`, each `[min, max]`), within `SM_EFF_ZERO` at neither end or one: the cap
    /// between them is made on one interval.
    RailIntervalsDiffer {
        /// The first rail's interval.
        first: [f64; 2],
        /// The second rail's interval.
        second: [f64; 2],
    },
    /// In a local operation, a surface beside an edge or at a vertex of `face` moves, and
    /// how far the surfaces there must be extended to meet has no certified bound
    /// ([`crate::offset::offset_executive::reach::local_extension_reach`]): a pair or corner
    /// of kinds it does not bound, or an edge piece left undecided. Refused before anything
    /// changes.
    ReachUncertified {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// A local operation gives `face`, which meets itself across edges (seams of its
    /// closed surface), a surface on which a seam's two UV lines (the face's loops are
    /// copied onto it) do not lift to one curve: it is not closed in the seam's direction,
    /// or the lines are not its seam, one period apart. Each moved edge is found again where the new surfaces on its two sides
    /// meet; across a seam both sides are the face's own new surface, which a surface open
    /// across it cannot close onto itself, so the face has no edge there (the topology the
    /// operation keeps cannot be built).
    SeamOpened {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// A curve where the offset surface meets itself came without its UV curve on one side
    /// (the intersection leaves the entry null), which the merge of the self-intersections
    /// cuts the offset face along. `face` is the input's face whose offset surface it is.
    SelfIntersectionWithoutUvCurve {
        /// The face, of the operation's input.
        face: FaceId,
    },
    /// The shell's face selection at `index` (in the order given) names no face of the body
    /// it was given: it is empty, of another body, or a face the body does not have.
    ShellFaceNotOfBody {
        /// The selection's position in the list given.
        index: usize,
    },
    /// A shell of the operation's input is in no region. A caller can build such a body
    /// (`Brep::shell_mut`), so it is the caller's input, not the kernel's fault.
    ShellHasNoRegion {
        /// The shell, of the operation's input.
        shell: ShellId,
    },
    /// An inward shell of a closed solid by `distance` leaves no offset of it at all: the
    /// wall is as thick as the solid or thicker, so there is no cavity to take away
    /// (WR-454). A thinner wall passes.
    ShellLeavesNoCavity {
        /// The offset distance asked for (signed, as the shell takes it).
        distance: f64,
    },
    /// Across a shell's tangent gap, the curve between an offset edge's settled vertices is
    /// not certified from the edge's curve within the endpoint bound (its tolerance and the
    /// vertices'): the vertices are off the curve by more, a drop onto it found no foot,
    /// they drop to one point, or its rail is not exactly reparameterised onto `[0, 1]`.
    ShellTangentCurveUncertified,
    /// Across a shell's tangent gap, no UV curve dropped onto the extended face runs over
    /// the whole interval of the edge's rebuilt curve.
    ShellTangentUvUnmatched,
    /// Across a shell's tangent gap, moving a settled vertex onto the rebuilt curve's end
    /// would leave another edge at that vertex ending `moved` from it, past its `bound` (its
    /// tolerance and the vertex's).
    ShellTangentVertexMoveTooFar {
        /// How far the other edge's end would be from the moved vertex.
        moved: f64,
        /// The farthest it may be.
        bound: f64,
    },
    /// The shell's wall `wall` (the offset distance's magnitude) is below `smallest`, twice
    /// the body's B-rep tolerance: a wall the shell does not resolve. A wall of at least
    /// `smallest` passes.
    ShellWallUnresolved {
        /// The wall asked for.
        wall: f64,
        /// The smallest wall the shell resolves.
        smallest: f64,
    },
    /// An offset edge that a loop gap's bridge or a step corner must split is not split at
    /// the point: the edge split takes the point for one of the edge's ends (within an end
    /// vertex's tolerance along the edge, or at an end of its parameter interval), though the
    /// caller kept the point farther than ten times the edge's tolerance from both end
    /// vertices. `vertex_tolerance`, the larger of the two end vertices' tolerances, says how
    /// far along the edge an end reaches at most. A defect ([`ErrorKind::StageFailed`]): the
    /// edge is one the kernel built, and the caller's check that the point is clear of the
    /// ends and the split's own check disagree.
    SplitAtEdgeEnd {
        /// The face, of the operation's input, whose offset holds the edge, where it comes
        /// from one.
        face: Option<FaceId>,
        /// The larger tolerance of the edge's two end vertices.
        vertex_tolerance: f64,
    },
    /// Two curved faces tangent at the old edge now cross, but the intersection
    /// branch on their retained sides has not been certified.
    TangentBranchUncertified,
    /// In a local operation, an edge's new curve ends beside an edge that was tangent in
    /// the input and whose new surfaces cross, and that edge, already mated, has no new
    /// vertex on the curve: the end is not where the edges meet, and no cut chosen by
    /// distance stands in for it.
    TangentEndOffCurve,
    /// The offsets of the two edge uses of a concave tangent edge leave gaps between them:
    /// the offset does not close such a gap at the edge.
    TangentGapsAtConcaveEdge,
    /// The offset B-rep's tolerance would be nonzero but at or below the effective zero
    /// (`SM_EFF_ZERO`, 1e-12): a tolerance no B-rep can hold (`ValidatePointers` test 0
    /// needs more), as double arithmetic places edges and vertices farther apart than
    /// that (WR-749). It is the magnitude of the member approximation tolerance, or, for
    /// the static and API offsets, the source's B-rep tolerance.
    ToleranceUnresolved {
        /// The tolerance asked for, as given (the executive takes its magnitude).
        tolerance: f64,
    },
    /// The UV trim curves of an extended offset face, on its extended surface, reach
    /// outside both its offset surface's domain and the extension's (by more than
    /// `SM_EFF_ZERO`): the face is trimmed on neither surface.
    TrimPastExtension,
    /// The tube of radius `|distance|` a rounded offset caps `edge` with folds over itself: the
    /// edge bends more tightly than that, on the side the tube faces (#13685).
    TubeFolds {
        /// The edge whose tube folds.
        edge: EdgeId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// The largest distance at which this edge's tube is proved not to fold, when one is.
        /// Only the tube is proved there: other defects can still appear at or near it.
        largest: Option<f64>,
    },
    /// The tube of radius `|distance|` a rounded offset caps `edge` with is neither proved
    /// regular nor proved to fold (#13685): its proof ran out of pieces, or the edge has no
    /// two faces or no NURBS form to decide it on.
    TubeNotCertified {
        /// The edge whose tube is not certified.
        edge: EdgeId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
    },
    /// A face of the body the operation was given has no surface, behind a handle that names
    /// no face of the input (a backend whose handles carry no topology entity): the
    /// [`OffsetFailure::FaceHasNoSurface`] of a face it cannot name.
    UnnamedFaceHasNoSurface,
    /// A local operation built a body that fails what its input did not
    /// ([`crate::topology::brep::validation::soundness`] of both): refused, the input B-rep
    /// left as it came in.
    Unsound {
        /// What the built body fails.
        what: Unsoundness,
    },
    /// At a vertex with three bounding arcs, an arc that misses a neighbour's plane lies in
    /// the other neighbour's plane: the arc cannot be re-made between the two planes.
    VertexArcInPlane,
    /// At a vertex with three bounding arcs, an arc that misses a neighbour's plane meets
    /// that plane only `distance` from its midpoint (no meeting at all: the largest finite
    /// value), farther than ten times its `radius`: the arc cannot be re-made there.
    VertexArcMissesPlane {
        /// How far the nearest meeting of the arc's circle with the plane is from the arc's
        /// midpoint.
        distance: f64,
        /// The arc's radius.
        radius: f64,
    },
    /// At a vertex with three bounding arcs, the sphere through them could not be made
    /// (`CreateSphereFromArcs`): the arcs are not of one centre and radius, or do not meet
    /// at their ends. [`ErrorKind::Failed`], neither a defect nor a refusal on its own: the
    /// sphere's error is kept as the cause and decides ([`crate::SolidError::is_defect`]).
    VertexArcsNotOneSphere,
    /// A vertex with three bounding offset edges whose curves are not all arcs (a line where
    /// a face was shelled): the cap through three arcs of one sphere cannot be made there.
    VertexCurveNotArc,
    /// The vertex offset was given no vertex.
    VertexNotGiven,
    /// The inset of `face` by `distance` meets the inset of a planar face opposite it across
    /// material exactly twice the distance away (a square boss inset by half its width):
    /// the material between them has no offset left, a change of topology the offset does not
    /// make. `largest` is half the narrowest such width: the largest wall that fits (#18239).
    WallsMeetAcross {
        /// A face of the narrowest such pair.
        face: FaceId,
        /// The offset distance asked for (signed, as the operation takes it).
        distance: f64,
        /// The largest wall that fits.
        largest: f64,
    },
    /// Loops of the offset of `face` touch without crossing, so the offset walls they
    /// come from meet there.
    WallsTouch {
        /// The face whose offset's loops touch, when known.
        face: Option<FaceId>,
        /// How they touch.
        contact: WallContact,
    },
}

/// What a local operation's or a solid offset's built body fails ([`OffsetFailure::Unsound`]).
/// The check that fails names the entity where the reason is about one
/// ([`Unsoundness::entity`]).
#[derive(Debug, Clone, Copy, PartialEq)]
#[non_exhaustive]
pub enum Unsoundness {
    /// The check at `SM_LEVEL_2` reports `failures` failure lines more than the input's
    /// check did, counted per class
    /// ([`crate::topology::brep::validation::BrepCheck::failures_beyond`]); the first such
    /// class is test `test_index` of the reporting type `reporting_type` (`None` for a line
    /// without a report: a failed status).
    Check {
        /// The number of failure lines beyond the input's.
        failures: usize,
        /// The first failed report's reporting type and test index.
        first: Option<(i64, u64)>,
    },
    /// The body's pointer check failed, so its geometry was not checked (`AssertValid`
    /// is skipped then).
    GeometryUnchecked,
    /// A gap in a loop of the body cannot be closed within the tolerances its certificates
    /// allow. No check returns it yet; it is named so a caller's match covers it when one
    /// does.
    LoopGapNotClosable,
    /// A loop of the body has a certified area that leaves its orientation undecided. No
    /// check returns it yet (as [`crate::topology::brep::SolidTest::LoopOrientationUndecided`]);
    /// it is named so a caller's match covers it when one does.
    LoopOrientationUndecided,
    /// The input was a manifold solid and the body is not.
    NotManifoldSolid,
    /// The tolerance of an edge or vertex of the body grew to `tolerance`, past the `bound` its
    /// certificate allows (`docs/validity-and-tolerances.md` §8: a constructed result's
    /// tolerances come from its certificates). No check returns it yet; it is named so a
    /// caller's match covers it when one does.
    ToleranceBeyondBound {
        /// The entity of the operation's input the edge or vertex comes from (as every entity
        /// these failures name is the input's), `None` where it comes from none (one the
        /// operation made): the built body's own ids name nothing a caller holds.
        entity: Option<EntityRef>,
        /// Its tolerance.
        tolerance: f64,
        /// The largest tolerance its certificate allows.
        bound: f64,
    },
    /// The body is a manifold solid whose volume's enclosure lies below 0.
    VolumeNotPositive,
    /// The body is a manifold solid whose volume's enclosure still holds 0 after its
    /// budget: its sign is not proved.
    VolumeUndecided,
}

impl Unsoundness {
    /// The entity of the operation's input the reason names: the one the edge or vertex whose
    /// tolerance grew past its bound comes from.
    #[must_use]
    pub const fn entity(&self) -> Option<EntityRef> {
        match *self {
            Self::ToleranceBeyondBound { entity, .. } => entity,
            Self::Check { .. }
            | Self::GeometryUnchecked
            | Self::NotManifoldSolid
            | Self::VolumeNotPositive
            | Self::VolumeUndecided
            | Self::LoopOrientationUndecided
            | Self::LoopGapNotClosable => None,
        }
    }
}

/// How loops of an offset face touch without crossing ([`OffsetFailure::WallsTouch`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum WallContact {
    /// They run along one another.
    RunAlong,
    /// They meet without crossing (tangentially, or coming up to one another and going
    /// back on the same side).
    Touch,
}

/// How loops of an offset face cross in a way the offset does not resolve
/// ([`OffsetFailure::LoopsCross`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[non_exhaustive]
pub enum LoopCrossing {
    /// The loops arrive at a crossing more than one way.
    ArriveMoreThanOneWay,
    /// They cross at a vertex of a loop.
    AtLoopVertex,
    /// The face comes apart around a hole.
    ComesApartAroundHole,
    /// The loops leave a crossing more than one way.
    LeaveMoreThanOneWay,
    /// The loops leave a crossing no way.
    LeaveNoWay,
    /// The face shares an edge with another face.
    SharesEdge,
    /// A stretch of a loop is neither inside nor outside another loop.
    StretchUnclassified,
    /// Three loops cross at one point.
    ThreeAtOnePoint,
    /// The offset's results are not merged, so the walls the crossing loops come from are
    /// not cut where they pass through each other; only the merge cuts them (the local
    /// operation, always unmerged, included where its walls leave the face on one side
    /// of it at a crossing).
    Unmerged,
}

impl OffsetFailure {
    /// The entity the refusal names: the face, or the edge (else its face).
    #[must_use]
    pub fn entity(&self) -> Option<EntityRef> {
        match *self {
            Self::Folds { face, .. }
            | Self::NotCertified { face, .. }
            | Self::Collapses { face, .. }
            | Self::WallsMeetAcross { face, .. }
            | Self::SeamOpened { face }
            | Self::FacesApart { face }
            | Self::PastSide { face, .. }
            | Self::FaceCloses { face, .. }
            | Self::DraftClosesWall { face, .. }
            | Self::ReachUncertified { face }
            | Self::BlendSectionNotFound { face }
            | Self::FaceHasNoSurface { face }
            | Self::FaceHasNoOuterFaceuse { face }
            | Self::SelfIntersectionWithoutUvCurve { face }
            | Self::MoveCollapses { face, .. }
            | Self::ExtendedFaceCollapses { face, .. }
            | Self::FaceHasNoFaceuse { face } => Some(EntityRef::Face(face)),
            Self::WallsTouch { face, .. }
            | Self::LoopsCross { face, .. }
            | Self::LoopGapUncertified { face }
            | Self::LoopGapPassLimit { face, .. }
            | Self::LoopGapEndsAmbiguous { face, .. }
            | Self::SplitAtEdgeEnd { face, .. } => face.map(EntityRef::Face),
            Self::EdgeVanishes { edge, face } => edge
                .map(EntityRef::Edge)
                .or_else(|| face.map(EntityRef::Face)),
            Self::LooseTangentEdge { edge, .. }
            | Self::ExtendedCurveOffDomain { edge, .. }
            | Self::EdgeHasNoCurve { edge }
            | Self::CurveKindUnsupported { edge, .. }
            | Self::EdgeuseHasNoUvTrim { edge } => edge.map(EntityRef::Edge),
            Self::TubeFolds { edge, .. } | Self::TubeNotCertified { edge, .. } => {
                Some(EntityRef::Edge(edge))
            }
            Self::Unsound { what } => what.entity(),
            Self::FaceuseHasNoFace { faceuse }
            | Self::FaceuseHasNoShell { faceuse }
            | Self::FaceuseHasNoMate { faceuse } => Some(EntityRef::Faceuse(faceuse)),
            Self::ShellHasNoRegion { shell } => Some(EntityRef::Shell(shell)),
            Self::EdgeuseHasNoEdge { edgeuse }
            | Self::EdgeuseHasNoMate { edgeuse }
            | Self::EdgeuseHasNoRadial { edgeuse }
            | Self::EdgeuseHasNoFaceuse { edgeuse } => Some(EntityRef::Edgeuse(edgeuse)),
            Self::BlendRailWithoutSupport { edge }
            | Self::EdgeOffsetsUnpaired { edge }
            | Self::GapEndWithoutEdge { edge }
            | Self::GapCornerBlendNotMade { edge } => Some(EntityRef::Edge(edge)),
            Self::FacesMoveRefused { .. }
            | Self::FollowedBlendIncomplete
            | Self::FaceMoveEmpty { .. }
            | Self::VertexNotGiven
            | Self::EdgeuseNotGiven
            | Self::VertexCurveNotArc
            | Self::VertexArcInPlane
            | Self::VertexArcMissesPlane { .. }
            | Self::FaceDomainUnresolved
            | Self::OffsetNotBuilt
            | Self::ExtendedEdgeUntrimmed
            | Self::LaminaEdgeOtherSide
            | Self::TangentGapsAtConcaveEdge
            | Self::RailIntervalsDiffer { .. }
            | Self::ShellTangentCurveUncertified
            | Self::ShellTangentUvUnmatched
            | Self::ShellTangentVertexMoveTooFar { .. }
            | Self::CrossingUndecided
            | Self::LocalFaceNotListed
            | Self::VertexArcsNotOneSphere
            | Self::UnnamedFaceHasNoSurface
            | Self::OffsetBaseKindUnsupported { .. }
            | Self::FaceuseNotGiven
            | Self::ExactCurveUncut
            | Self::TangentEndOffCurve
            | Self::TangentBranchUncertified
            | Self::PeriodicBranchAmbiguous
            | Self::DistanceUnresolved { .. }
            | Self::ToleranceUnresolved { .. }
            | Self::Defect { .. }
            | Self::BrepNotGiven
            | Self::ShellLeavesNoCavity { .. }
            | Self::ShellWallUnresolved { .. }
            | Self::ShellFaceNotOfBody { .. }
            | Self::TrimPastExtension => None,
        }
    }

    /// True for a failure whose text names none of the faces or edges it is about: the
    /// failure's own entity is said after its text ([`crate::error::Failure`]'s `Display`).
    /// The others name theirs in the text, or have none.
    #[must_use]
    pub const fn names_entity_after_text(&self) -> bool {
        matches!(
            self,
            Self::Defect { .. }
                | Self::LoopGapPassLimit { .. }
                | Self::LoopGapEndsAmbiguous { .. }
                | Self::SplitAtEdgeEnd { .. }
                | Self::OffsetBaseKindUnsupported { .. }
                | Self::OffsetNotBuilt
                | Self::EdgeOffsetsUnpaired { .. }
                | Self::GapEndWithoutEdge { .. }
                | Self::GapCornerBlendNotMade { .. }
                | Self::FaceuseNotGiven
                | Self::SelfIntersectionWithoutUvCurve { .. }
                | Self::FaceuseHasNoFace { .. }
                | Self::BrepNotGiven
                | Self::ShellLeavesNoCavity { .. }
                | Self::ShellWallUnresolved { .. }
                | Self::ShellFaceNotOfBody { .. }
                | Self::ExtendedFaceCollapses { .. }
                | Self::TrimPastExtension
                | Self::FaceHasNoFaceuse { .. }
                | Self::FaceuseHasNoShell { .. }
                | Self::ShellHasNoRegion { .. }
                | Self::FaceuseHasNoMate { .. }
                | Self::EdgeuseHasNoEdge { .. }
                | Self::EdgeuseHasNoMate { .. }
                | Self::EdgeuseHasNoRadial { .. }
                | Self::EdgeuseHasNoFaceuse { .. }
                | Self::FacesMoveRefused { .. }
                | Self::BlendRailWithoutSupport { .. }
                | Self::BlendSectionNotFound { .. }
                | Self::FollowedBlendIncomplete
                | Self::EdgeHasNoCurve { .. }
                | Self::FaceHasNoSurface { .. }
                | Self::FaceHasNoOuterFaceuse { .. }
                | Self::MoveCollapses { .. }
                | Self::FaceMoveEmpty { .. }
                | Self::VertexNotGiven
                | Self::EdgeuseNotGiven
                | Self::CurveKindUnsupported { .. }
                | Self::VertexCurveNotArc
                | Self::VertexArcInPlane
                | Self::VertexArcMissesPlane { .. }
                | Self::FaceDomainUnresolved
                | Self::ExtendedEdgeUntrimmed
                | Self::LaminaEdgeOtherSide
                | Self::TangentGapsAtConcaveEdge
                | Self::EdgeuseHasNoUvTrim { .. }
                | Self::RailIntervalsDiffer { .. }
                | Self::ShellTangentCurveUncertified
                | Self::ShellTangentUvUnmatched
                | Self::ShellTangentVertexMoveTooFar { .. }
                | Self::CrossingUndecided
                | Self::LocalFaceNotListed
                | Self::VertexArcsNotOneSphere
                | Self::UnnamedFaceHasNoSurface
        )
    }

    /// True for a routine's failure, which answers with the status an offset routine returns
    /// (`SolidError::failed_routine_status`): a bare status
    /// ([`OffsetFailure::Defect`]), or a cause named at a site
    /// that returns one ([`OffsetFailure::LoopGapPassLimit`],
    /// [`OffsetFailure::LoopGapEndsAmbiguous`], [`OffsetFailure::SplitAtEdgeEnd`], those of
    /// `offset_executive::p1` (the module table's `offset_executive::p1` rows, from
    /// [`OffsetFailure::OffsetBaseKindUnsupported`]), and those of
    /// [`OffsetFailure::is_named_at_a_status_site`]). The other refusals are solidcore's and
    /// stand for no status.
    #[must_use]
    pub const fn is_failed_routine(&self) -> bool {
        matches!(
            self,
            Self::Defect { .. }
                | Self::LoopGapPassLimit { .. }
                | Self::LoopGapEndsAmbiguous { .. }
                | Self::SplitAtEdgeEnd { .. }
                | Self::OffsetBaseKindUnsupported { .. }
                | Self::EdgeOffsetsUnpaired { .. }
                | Self::GapEndWithoutEdge { .. }
                | Self::GapCornerBlendNotMade { .. }
                | Self::FaceuseNotGiven
                | Self::SelfIntersectionWithoutUvCurve { .. }
                | Self::FaceuseHasNoFace { .. }
                | Self::BrepNotGiven
                | Self::ShellLeavesNoCavity { .. }
                | Self::ShellWallUnresolved { .. }
                | Self::ShellFaceNotOfBody { .. }
                | Self::ExtendedFaceCollapses { .. }
                | Self::TrimPastExtension
                | Self::FaceHasNoFaceuse { .. }
                | Self::FaceuseHasNoShell { .. }
                | Self::ShellHasNoRegion { .. }
                | Self::FaceuseHasNoMate { .. }
                | Self::EdgeuseHasNoEdge { .. }
                | Self::EdgeuseHasNoMate { .. }
                | Self::EdgeuseHasNoRadial { .. }
                | Self::EdgeuseHasNoFaceuse { .. }
        ) || self.is_named_at_a_status_site()
    }

    /// True for a cause named at a site of `offset_faces` or `offset_geometry_creation` that
    /// returned a bare status ([`OffsetFailure::FacesMoveRefused`] to
    /// [`OffsetFailure::VertexArcsNotOneSphere`]): each keeps the status of the site it
    /// replaced.
    #[must_use]
    pub const fn is_named_at_a_status_site(&self) -> bool {
        matches!(
            self,
            Self::FacesMoveRefused { .. }
                | Self::BlendRailWithoutSupport { .. }
                | Self::BlendSectionNotFound { .. }
                | Self::FollowedBlendIncomplete
                | Self::EdgeHasNoCurve { .. }
                | Self::FaceHasNoSurface { .. }
                | Self::FaceHasNoOuterFaceuse { .. }
                | Self::MoveCollapses { .. }
                | Self::FaceMoveEmpty { .. }
                | Self::VertexNotGiven
                | Self::EdgeuseNotGiven
                | Self::CurveKindUnsupported { .. }
                | Self::VertexCurveNotArc
                | Self::VertexArcInPlane
                | Self::VertexArcMissesPlane { .. }
                | Self::FaceDomainUnresolved
                | Self::ExtendedEdgeUntrimmed
                | Self::LaminaEdgeOtherSide
                | Self::TangentGapsAtConcaveEdge
                | Self::EdgeuseHasNoUvTrim { .. }
                | Self::RailIntervalsDiffer { .. }
                | Self::ShellTangentCurveUncertified
                | Self::ShellTangentUvUnmatched
                | Self::ShellTangentVertexMoveTooFar { .. }
                | Self::CrossingUndecided
                | Self::LocalFaceNotListed
                | Self::VertexArcsNotOneSphere
                | Self::UnnamedFaceHasNoSurface
        )
    }

    /// True for a failure that is the kernel's fault, not a fact of the input: a stage
    /// reviewed as the kernel's fault ([`OffsetFailure::Defect`], whose class
    /// [`ErrorKind::StageFailed`] says it too), or a named failure of the loop-gap search on the offset
    /// loops the kernel built ([`OffsetFailure::LoopGapPassLimit`],
    /// [`OffsetFailure::LoopGapEndsAmbiguous`], [`OffsetFailure::SplitAtEdgeEnd`]). Report it.
    /// Said per cause, as the class of a loop-gap failure does not say it.
    #[must_use]
    pub const fn is_defect(&self) -> bool {
        match self {
            Self::BlendRailWithoutSupport { .. }
            | Self::BlendSectionNotFound { .. }
            | Self::BrepNotGiven
            | Self::Collapses { .. }
            | Self::CrossingUndecided
            | Self::CurveKindUnsupported { .. }
            | Self::DistanceUnresolved { .. }
            | Self::DraftClosesWall { .. }
            | Self::EdgeHasNoCurve { .. }
            | Self::EdgeOffsetsUnpaired { .. }
            | Self::EdgeVanishes { .. }
            | Self::EdgeuseHasNoEdge { .. }
            | Self::EdgeuseHasNoFaceuse { .. }
            | Self::EdgeuseHasNoMate { .. }
            | Self::EdgeuseHasNoRadial { .. }
            | Self::EdgeuseHasNoUvTrim { .. }
            | Self::EdgeuseNotGiven
            | Self::ExactCurveUncut
            | Self::ExtendedEdgeUntrimmed
            | Self::ExtendedFaceCollapses { .. }
            | Self::FaceCloses { .. }
            | Self::FaceDomainUnresolved
            | Self::OffsetNotBuilt
            | Self::FaceHasNoFaceuse { .. }
            | Self::FaceHasNoOuterFaceuse { .. }
            | Self::FaceHasNoSurface { .. }
            | Self::FaceMoveEmpty { .. }
            | Self::FacesApart { .. }
            | Self::FacesMoveRefused { .. }
            | Self::FaceuseHasNoFace { .. }
            | Self::FaceuseHasNoMate { .. }
            | Self::FaceuseHasNoShell { .. }
            | Self::FaceuseNotGiven
            | Self::Folds { .. }
            | Self::FollowedBlendIncomplete
            | Self::GapCornerBlendNotMade { .. }
            | Self::GapEndWithoutEdge { .. }
            | Self::LaminaEdgeOtherSide
            | Self::LocalFaceNotListed
            | Self::LoopGapUncertified { .. }
            | Self::LoopsCross { .. }
            | Self::LooseTangentEdge { .. }
            | Self::ExtendedCurveOffDomain { .. }
            | Self::MoveCollapses { .. }
            | Self::NotCertified { .. }
            | Self::OffsetBaseKindUnsupported { .. }
            | Self::PastSide { .. }
            | Self::PeriodicBranchAmbiguous
            | Self::RailIntervalsDiffer { .. }
            | Self::ReachUncertified { .. }
            | Self::SeamOpened { .. }
            | Self::SelfIntersectionWithoutUvCurve { .. }
            | Self::ShellFaceNotOfBody { .. }
            | Self::ShellHasNoRegion { .. }
            | Self::ShellLeavesNoCavity { .. }
            | Self::ShellTangentCurveUncertified
            | Self::ShellTangentUvUnmatched
            | Self::ShellTangentVertexMoveTooFar { .. }
            | Self::ShellWallUnresolved { .. }
            | Self::TangentBranchUncertified
            | Self::TangentEndOffCurve
            | Self::TangentGapsAtConcaveEdge
            | Self::ToleranceUnresolved { .. }
            | Self::TrimPastExtension
            | Self::TubeFolds { .. }
            | Self::TubeNotCertified { .. }
            | Self::UnnamedFaceHasNoSurface
            | Self::Unsound { .. }
            | Self::VertexArcInPlane
            | Self::VertexArcMissesPlane { .. }
            | Self::VertexArcsNotOneSphere
            | Self::VertexCurveNotArc
            | Self::VertexNotGiven
            | Self::WallsMeetAcross { .. }
            | Self::WallsTouch { .. } => false,
            Self::Defect { .. }
            | Self::LoopGapEndsAmbiguous { .. }
            | Self::LoopGapPassLimit { .. }
            | Self::SplitAtEdgeEnd { .. } => true,
        }
    }

    /// The status the C++ reports for the refusal (difftest's comparison detail).
    pub(crate) const fn comparison_status(&self) -> i32 {
        match self {
            Self::BlendRailWithoutSupport { .. }
            | Self::BrepNotGiven
            | Self::CurveKindUnsupported { .. }
            | Self::EdgeHasNoCurve { .. }
            | Self::EdgeOffsetsUnpaired { .. }
            | Self::EdgeuseHasNoEdge { .. }
            | Self::EdgeuseHasNoFaceuse { .. }
            | Self::EdgeuseHasNoMate { .. }
            | Self::EdgeuseHasNoRadial { .. }
            | Self::EdgeuseHasNoUvTrim { .. }
            | Self::EdgeuseNotGiven
            | Self::FaceHasNoFaceuse { .. }
            | Self::FaceHasNoOuterFaceuse { .. }
            | Self::FaceHasNoSurface { .. }
            | Self::FaceuseHasNoFace { .. }
            | Self::FaceuseHasNoMate { .. }
            | Self::FaceuseHasNoShell { .. }
            | Self::FaceuseNotGiven
            | Self::FollowedBlendIncomplete
            | Self::GapEndWithoutEdge { .. }
            | Self::SelfIntersectionWithoutUvCurve { .. }
            | Self::ShellHasNoRegion { .. }
            | Self::UnnamedFaceHasNoSurface
            | Self::VertexNotGiven => SM_ERR_NULL_POINTER,
            Self::BlendSectionNotFound { .. }
            | Self::Collapses { .. }
            | Self::CrossingUndecided
            | Self::Defect { .. }
            | Self::DraftClosesWall { .. }
            | Self::ExactCurveUncut
            | Self::ExtendedEdgeUntrimmed
            | Self::ExtendedFaceCollapses { .. }
            | Self::FaceCloses { .. }
            | Self::FaceDomainUnresolved
            | Self::OffsetNotBuilt
            | Self::FacesApart { .. }
            | Self::FacesMoveRefused { .. }
            | Self::Folds { .. }
            | Self::GapCornerBlendNotMade { .. }
            | Self::LaminaEdgeOtherSide
            | Self::LocalFaceNotListed
            | Self::LoopGapEndsAmbiguous { .. }
            | Self::LoopGapPassLimit { .. }
            | Self::LoopGapUncertified { .. }
            | Self::LooseTangentEdge { .. }
            | Self::ExtendedCurveOffDomain { .. }
            | Self::MoveCollapses { .. }
            | Self::NotCertified { .. }
            | Self::OffsetBaseKindUnsupported { .. }
            | Self::PastSide { .. }
            | Self::PeriodicBranchAmbiguous
            | Self::RailIntervalsDiffer { .. }
            | Self::ReachUncertified { .. }
            | Self::SeamOpened { .. }
            | Self::ShellTangentCurveUncertified
            | Self::ShellTangentUvUnmatched
            | Self::ShellTangentVertexMoveTooFar { .. }
            | Self::SplitAtEdgeEnd { .. }
            | Self::TangentBranchUncertified
            | Self::TangentEndOffCurve
            | Self::TangentGapsAtConcaveEdge
            | Self::TrimPastExtension
            | Self::TubeFolds { .. }
            | Self::TubeNotCertified { .. }
            | Self::Unsound { .. }
            | Self::VertexArcInPlane
            | Self::VertexArcMissesPlane { .. }
            | Self::VertexArcsNotOneSphere
            | Self::VertexCurveNotArc
            | Self::WallsMeetAcross { .. } => SM_ERR,
            Self::DistanceUnresolved { .. } | Self::ToleranceUnresolved { .. } => {
                SM_ERR_INVALID_INPUT
            }
            Self::EdgeVanishes { .. } => SM_ERR_OFFSET_EDGE_VANISHES,
            Self::FaceMoveEmpty { .. }
            | Self::ShellFaceNotOfBody { .. }
            | Self::ShellLeavesNoCavity { .. }
            | Self::ShellWallUnresolved { .. } => SM_ERR_INVALID_INPUT,
            Self::LoopsCross { .. } => SM_ERR_BAD_INTERSECTIONS,
            Self::WallsTouch { .. } => SM_ERR_OFFSET_WALLS_TOUCH,
        }
    }

    /// The refusal's stable [`ErrorKind`].
    #[must_use]
    pub const fn kind(&self) -> ErrorKind {
        match self {
            Self::BlendRailWithoutSupport { .. }
            | Self::BlendSectionNotFound { .. }
            | Self::CrossingUndecided
            | Self::CurveKindUnsupported { .. }
            | Self::DraftClosesWall { .. }
            | Self::EdgeOffsetsUnpaired { .. }
            | Self::EdgeuseHasNoUvTrim { .. }
            | Self::ExactCurveUncut
            | Self::ExtendedEdgeUntrimmed
            | Self::FaceCloses { .. }
            | Self::FacesApart { .. }
            | Self::FacesMoveRefused { .. }
            | Self::GapEndWithoutEdge { .. }
            | Self::LaminaEdgeOtherSide
            | Self::LoopGapUncertified { .. }
            | Self::LooseTangentEdge { .. }
            | Self::ExtendedCurveOffDomain { .. }
            | Self::OffsetBaseKindUnsupported { .. }
            | Self::PastSide { .. }
            | Self::PeriodicBranchAmbiguous
            | Self::RailIntervalsDiffer { .. }
            | Self::ReachUncertified { .. }
            | Self::SeamOpened { .. }
            | Self::SelfIntersectionWithoutUvCurve { .. }
            | Self::ShellTangentCurveUncertified
            | Self::ShellTangentUvUnmatched
            | Self::ShellTangentVertexMoveTooFar { .. }
            | Self::TangentBranchUncertified
            | Self::TangentEndOffCurve
            | Self::TangentGapsAtConcaveEdge
            | Self::TrimPastExtension
            | Self::Unsound { .. }
            | Self::VertexArcInPlane
            | Self::VertexArcMissesPlane { .. }
            | Self::VertexCurveNotArc => ErrorKind::Unsupported,
            Self::Collapses { .. }
            | Self::ExtendedFaceCollapses { .. }
            | Self::MoveCollapses { .. }
            | Self::ShellLeavesNoCavity { .. }
            | Self::WallsMeetAcross { .. } => ErrorKind::OffsetCollapses,
            Self::Defect { .. }
            | Self::LoopGapEndsAmbiguous { .. }
            | Self::LoopGapPassLimit { .. }
            | Self::SplitAtEdgeEnd { .. } => ErrorKind::StageFailed,
            Self::BrepNotGiven
            | Self::DistanceUnresolved { .. }
            | Self::EdgeHasNoCurve { .. }
            | Self::EdgeuseHasNoEdge { .. }
            | Self::EdgeuseHasNoFaceuse { .. }
            | Self::EdgeuseHasNoMate { .. }
            | Self::EdgeuseHasNoRadial { .. }
            | Self::EdgeuseNotGiven
            | Self::FaceHasNoFaceuse { .. }
            | Self::FaceHasNoOuterFaceuse { .. }
            | Self::FaceHasNoSurface { .. }
            | Self::FaceMoveEmpty { .. }
            | Self::FaceuseHasNoFace { .. }
            | Self::FaceuseHasNoMate { .. }
            | Self::FaceuseHasNoShell { .. }
            | Self::FaceuseNotGiven
            | Self::FollowedBlendIncomplete
            | Self::LocalFaceNotListed
            | Self::ShellFaceNotOfBody { .. }
            | Self::ShellHasNoRegion { .. }
            | Self::ShellWallUnresolved { .. }
            | Self::ToleranceUnresolved { .. }
            | Self::UnnamedFaceHasNoSurface
            | Self::VertexNotGiven => ErrorKind::InvalidInput,
            Self::EdgeVanishes { .. } => ErrorKind::OffsetEdgeVanishes,
            // neither a defect nor a refusal: the failure keeps the error of the step that
            // failed, which decides (`SolidError::is_defect`, `is_refusal`)
            Self::FaceDomainUnresolved
            | Self::OffsetNotBuilt
            | Self::GapCornerBlendNotMade { .. }
            | Self::VertexArcsNotOneSphere => ErrorKind::Failed,
            Self::Folds { .. }
            | Self::NotCertified { .. }
            | Self::TubeFolds { .. }
            | Self::TubeNotCertified { .. } => ErrorKind::OffsetFolds,
            Self::LoopsCross { .. } => ErrorKind::OffsetLoopsCross,
            Self::WallsTouch { .. } => ErrorKind::OffsetWallsTouch,
        }
    }

    /// The same refusal about `face` where it names no face yet (the face of the input
    /// an offset face comes from, known only to the caller of the stage that refused).
    #[must_use]
    pub const fn on_face(self, face: FaceId) -> Self {
        match self {
            Self::EdgeVanishes {
                edge: None,
                face: None,
            } => Self::EdgeVanishes {
                edge: None,
                face: Some(face),
            },
            Self::LoopsCross {
                face: None,
                crossing,
            } => Self::LoopsCross {
                face: Some(face),
                crossing,
            },
            Self::WallsTouch {
                face: None,
                contact,
            } => Self::WallsTouch {
                face: Some(face),
                contact,
            },
            other => other,
        }
    }
}

/// "the offset of face 3" or "an offset face".
fn offset_of(face: Option<FaceId>) -> String {
    face.map_or_else(
        || "an offset face".to_owned(),
        |f| format!("the offset of face {}", f.index()),
    )
}

impl core::fmt::Display for LoopCrossing {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(match self {
            LoopCrossing::ArriveMoreThanOneWay => {
                "the loops arrive at a crossing more than one way"
            }
            LoopCrossing::AtLoopVertex => "they cross at a loop vertex",
            LoopCrossing::ComesApartAroundHole => "the face comes apart around a hole",
            LoopCrossing::LeaveMoreThanOneWay => "the loops leave a crossing more than one way",
            LoopCrossing::LeaveNoWay => "the loops leave a crossing no way",
            LoopCrossing::SharesEdge => "the face shares an edge with another face",
            LoopCrossing::StretchUnclassified => {
                "a stretch of a loop is neither inside nor outside another loop"
            }
            LoopCrossing::ThreeAtOnePoint => "three loops cross at one point",
            LoopCrossing::Unmerged => {
                "the results are not merged, so the walls the loops come from are not cut where \
                 they cross; only a merged offset cuts them"
            }
        })
    }
}

impl core::fmt::Display for Unsoundness {
    /// What the body built fails, without the refusal [`OffsetFailure`]'s words add.
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Unsoundness::Check { failures, first } => {
                write!(
                    f,
                    "the local operation's body fails its check where its input did not \
                     ({failures} failure lines more"
                )?;
                if let Some((reporting_type, test_index)) = first {
                    write!(f, ", the first test #{test_index} of type {reporting_type}")?;
                }
                f.write_str(")")
            }
            Unsoundness::GeometryUnchecked => f.write_str(
                "the local operation's body fails its pointer check, so its geometry cannot \
                 be checked",
            ),
            Unsoundness::NotManifoldSolid => {
                f.write_str("the body built is not a manifold solid, as its input was")
            }
            Unsoundness::VolumeNotPositive => f.write_str(
                "the local operation's body is a manifold solid whose volume is not positive",
            ),
            Unsoundness::VolumeUndecided => f.write_str(
                "the local operation's body is a manifold solid whose volume's sign could \
                 not be proved",
            ),
            Unsoundness::LoopOrientationUndecided => f.write_str(
                "a loop of the body built has a certified area that leaves its orientation \
                 undecided",
            ),
            Unsoundness::LoopGapNotClosable => f.write_str(
                "a gap in a loop of the body built cannot be closed within the tolerances its \
                 certificates allow",
            ),
            Unsoundness::ToleranceBeyondBound {
                entity,
                tolerance,
                bound,
            } => {
                match entity {
                    Some(e) => write!(f, "the tolerance at {e} of the input")?,
                    None => f.write_str("the tolerance of an edge or vertex")?,
                }
                write!(
                    f,
                    " grew to {tolerance:e}, past the {bound:e} its certificate allows"
                )
            }
        }
    }
}

impl core::fmt::Display for OffsetFailure {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match *self {
            Self::Folds {
                face,
                distance,
                largest,
            } => write!(
                f,
                "the offset of face {} by {distance} folds: the wall is thicker than a \
                 radius of curvature of the face on that side (the largest wall that works \
                 is {largest})",
                face.index()
            ),
            Self::OffsetNotBuilt => f.write_str("the offset could not be built"),
            Self::NotCertified { face, distance } => write!(
                f,
                "the offset of face {} by {distance} is neither proved regular nor proved to \
                 fold, so no size is named",
                face.index()
            ),
            Self::TubeFolds {
                edge,
                distance,
                largest,
            } => {
                write!(
                    f,
                    "the rounded offset's tube about edge {} by {distance} folds over itself: \
                     the edge bends more tightly than that on the side the tube faces",
                    edge.index()
                )?;
                match largest {
                    Some(r) => write!(
                        f,
                        " (the largest distance at which this edge's tube is proved not to \
                         fold is {r})"
                    ),
                    None => f.write_str(" (no distance at which it does not fold is proved)"),
                }
            }
            Self::TubeNotCertified { edge, distance } => write!(
                f,
                "the rounded offset's tube about edge {} by {distance} is not proved regular, \
                 nor proved to fold, so no size is named",
                edge.index()
            ),
            Self::Collapses {
                face,
                distance,
                radius,
            } => write!(
                f,
                "the offset of face {} by {distance} collapses: the face has a radius of \
                 curvature {radius} on that side, which the offset reaches (a round offset by \
                 its radius is a line, not a face; the faces beside it would have to meet \
                 across it, which the offset does not build)",
                face.index()
            ),
            Self::WallsMeetAcross {
                face,
                distance,
                largest,
            } => write!(
                f,
                "the offset of face {} by {distance} vanishes: the wall reaches half the width \
                 between two opposite faces (the largest wall that fits is {largest})",
                face.index()
            ),
            Self::WallsTouch { face, contact } => {
                let how = match contact {
                    WallContact::RunAlong => "run along one another",
                    WallContact::Touch => "touch without crossing",
                };
                write!(
                    f,
                    "the loops of {} {how}, so the offset walls would meet there",
                    offset_of(face)
                )
            }
            Self::LoopsCross { face, crossing } => write!(
                f,
                "the loops of {} cross in a way the offset cannot resolve: {crossing}",
                offset_of(face)
            ),
            Self::EdgeVanishes { edge, face } => {
                let which = match (edge, face) {
                    (Some(e), _) => format!("edge {}", e.index()),
                    (None, Some(face)) => format!("an edge of face {}", face.index()),
                    (None, None) => "an edge".to_owned(),
                };
                write!(
                    f,
                    "the offset moves a vertex of {which} to or past the edge's other end: \
                     the edge would vanish, a change of topology the offset does not make"
                )
            }
            Self::DistanceUnresolved {
                distance,
                resolution,
            } => write!(
                f,
                "invalid input: offset distance {distance:e} is not a finite length of at \
                 least {resolution:e}, the B-rep tolerance (the smallest distance the model \
                 resolves)"
            ),
            Self::SeamOpened { face } => write!(
                f,
                "face {} meets itself across a seam whose two UV lines do not lift to one \
                 curve on the surface it is given (not closed that way, or the lines not on \
                 its seam one period apart): the seam would have no edge",
                face.index()
            ),
            Self::FacesApart { face } => write!(
                f,
                "the new surfaces of face {} and of a neighbour meet nowhere near their \
                 edge: no extension makes them meet there",
                face.index()
            ),
            Self::PastSide {
                face,
                side,
                largest,
            } => write!(
                f,
                "face {} moves past where the surface of face {} beside it ends: it meets \
                 that side only while it moves less than {largest}",
                face.index(),
                side.index()
            ),
            Self::FaceCloses {
                face,
                beside,
                largest,
            } => write!(
                f,
                "face {} between faces {} and {} closes to nothing as the faces move: its \
                 edges pass over each other once they move more than {largest}",
                face.index(),
                beside[0].index(),
                beside[1].index()
            ),
            Self::DraftClosesWall {
                face,
                wall,
                largest,
            } => write!(
                f,
                "drafted, face {} reaches face {} of another shell of the body, closing the \
                 wall between them: the largest draft angle that keeps a wall is {largest} \
                 radians ({} degrees)",
                face.index(),
                wall.index(),
                largest.to_degrees()
            ),
            Self::ReachUncertified { face } => write!(
                f,
                "a surface beside an edge or at a vertex of face {} moves, and how far the \
                 surfaces there must be extended to meet has no certified bound",
                face.index()
            ),
            Self::Unsound { what } => match what {
                Unsoundness::NotManifoldSolid => write!(f, "{what}: refused"),
                _ => write!(f, "{what}: refused, the input unchanged"),
            },
            Self::LoopGapUncertified { face } => write!(
                f,
                "{} leaves a gap in its loops, and the straight segment that would bridge \
                 it could not be established within tolerance: the gap stays open",
                offset_of(face)
            ),
            Self::ExactCurveUncut => f.write_str(
                "the extended faces beside an edge meet on an exact curve that could not be \
                 cut into its pieces with their UV curves, so the edge's offsets cannot be \
                 mated",
            ),
            Self::TangentBranchUncertified => f.write_str(
                "two curved faces tangent at the old edge now cross, but the intersection \
                 branch on their retained sides has no certificate",
            ),
            Self::PeriodicBranchAmbiguous => f.write_str(
                "more than one surface-surface intersection branch claims the same periodic \
                 boundary event, so the branch through that event is ambiguous",
            ),
            Self::ExtendedCurveOffDomain { edge, outside } => write!(
                f,
                "the UV curve extended along the offset of {} has its midpoint {outside:e} \
                 outside the extended surface's domain: it is not in that surface's parameters, \
                 so the edge is not extended",
                edge.map_or_else(|| "an edge".to_owned(), |e| format!("edge {}", e.index()))
            ),
            Self::LooseTangentEdge {
                edge,
                max_drop_to_surf,
                meet_tol,
            } => write!(
                f,
                "in a local operation, the new faces beside {} that was tangent no longer \
                 meet along it: one moved off the other ({max_drop_to_surf:e} apart, limit {meet_tol:e}), and \
                 nothing closes the gap",
                edge.map_or_else(|| "an edge".to_owned(), |e| format!("edge {}", e.index()))
            ),
            Self::TangentEndOffCurve => f.write_str(
                "an edge ends beside a tangent edge whose new faces now cross, and that edge's new \
                 vertex is not on the edge's new curve, so the edge cannot end there",
            ),
            Self::ToleranceUnresolved { tolerance } => write!(
                f,
                "invalid input: offset tolerance {tolerance:e} is nonzero but at or below the \
                 effective zero (1e-12): no B-rep can hold a tolerance that small"
            ),
            Self::Defect { why } => write!(f, "{}: {why}", ErrorKind::StageFailed),
            // (these name their face through the failure's entity, said after the text)
            Self::LoopGapPassLimit { limit, .. } => write!(
                f,
                "{}: the gap search of an offset face's loops closes one gap per pass and \
                 reached its limit of {limit} passes before a pass found no gap",
                ErrorKind::StageFailed
            ),
            Self::LoopGapEndsAmbiguous {
                end_distance,
                start_distance,
                tolerance,
                ..
            } => write!(
                f,
                "{}: a gap in an offset face's loop has no one end at the vertex its edges \
                 share: its ends are {end_distance:e} and {start_distance:e} from it, against \
                 {tolerance:e}",
                ErrorKind::StageFailed
            ),
            Self::SplitAtEdgeEnd {
                vertex_tolerance, ..
            } => write!(
                f,
                "{}: an offset edge could not be split where a loop gap or a step corner \
                 needs it: the split takes the point for an end of the edge (its end \
                 vertices' tolerance is up to {vertex_tolerance:e})",
                ErrorKind::StageFailed
            ),
            // (the ones with an entity name it through the failure's entity, said after the
            // text)
            Self::FacesMoveRefused { refusal } => write!(
                f,
                "the face move is refused: {}",
                faces_move_refusal_says(refusal)
            ),
            Self::BlendRailWithoutSupport { .. } => f.write_str(
                "a rail of a blend that follows a moved face has no face across it from the \
                 blend, so the blend cannot be taken off and put back",
            ),
            Self::BlendSectionNotFound { .. } => f.write_str(
                "the section of a blend that follows a moved face is not found: a rail's point \
                 drops onto neither the other rail nor the blend's surface",
            ),
            Self::FollowedBlendIncomplete => f.write_str(
                "invalid input: a followed blend not taken off has no blend face or rail",
            ),
            Self::EdgeHasNoCurve { .. } => f.write_str("invalid input: the edge has no curve"),
            Self::FaceHasNoSurface { .. } => f.write_str("invalid input: the face has no surface"),
            Self::FaceHasNoOuterFaceuse { .. } => {
                f.write_str("invalid input: the face has no face use on the outside of its body")
            }
            Self::MoveCollapses { distance, .. } => write!(
                f,
                "the surface of a face moved by {distance} collapses onto its spine: there is no \
                 surface to move the face to"
            ),
            Self::FaceMoveEmpty { faces, distance } => write!(
                f,
                "invalid input: a face move needs a face and a nonzero distance ({faces} faces, \
                 distance {distance})"
            ),
            Self::VertexNotGiven => f.write_str("invalid input: the vertex offset was given no vertex"),
            Self::EdgeuseNotGiven => f.write_str(
                "invalid input: the mating of two offset edges was given no edge use for one of \
                 the four it takes, or the synchronization of an edge's offsets none",
            ),
            Self::CurveKindUnsupported { curve_type, .. } => write!(
                f,
                "an edge's curve (type {curve_type}) is of no B-spline curve kind, which the \
                 routine reads"
            ),
            Self::VertexCurveNotArc => f.write_str(
                "a curve bounding a vertex with three offset edges is not an arc, so no sphere \
                 caps the vertex",
            ),
            Self::VertexArcInPlane => f.write_str(
                "an arc bounding a vertex lies in a neighbour's plane, so it cannot be re-made \
                 between the planes",
            ),
            Self::VertexArcMissesPlane { distance, radius } => write!(
                f,
                "an arc of radius {radius} bounding a vertex meets a neighbour's plane only \
                 {distance} from its midpoint, farther than ten radii"
            ),
            Self::FaceDomainUnresolved => f.write_str(
                "the UV domain of an offset face's trim curves could not be found, so its \
                 surface domain is not updated",
            ),
            Self::ExtendedEdgeUntrimmed => f.write_str(
                "an offset edge's extended curve is cut at neither end, so it is not extended or \
                 mated",
            ),
            Self::LaminaEdgeOtherSide => f.write_str(
                "a lamina edge has an offset edge use on its other side, which is not extended: \
                 only an edge with a face on each side gets the second UV curve",
            ),
            Self::TangentGapsAtConcaveEdge => f.write_str(
                "the offsets beside a concave tangent edge leave gaps between them, which the \
                 offset does not close",
            ),
            Self::EdgeuseHasNoUvTrim { .. } => f.write_str(
                "an edge use has no UV trim curve, which the cap between the edge's offsets is \
                 made from and the matching of offset edge uses drops onto",
            ),
            Self::RailIntervalsDiffer { first, second } => write!(
                f,
                "the rails of an edge's cap run over different intervals ([{}, {}] and [{}, {}])",
                first[0], first[1], second[0], second[1]
            ),
            Self::ShellTangentCurveUncertified => f.write_str(
                "across a shell's tangent gap, the curve between an offset edge's settled \
                 vertices is not certified within their tolerances",
            ),
            Self::ShellTangentUvUnmatched => f.write_str(
                "across a shell's tangent gap, no UV curve on the extended face runs over the \
                 whole rebuilt edge",
            ),
            Self::ShellTangentVertexMoveTooFar { moved, bound } => write!(
                f,
                "across a shell's tangent gap, moving a vertex would leave another edge's end \
                 {moved:e} from it, past {bound:e}"
            ),
            Self::CrossingUndecided => f.write_str(
                "where an offset edge's curve meets a neighbour's offset surface, the curve has \
                 no tangent or the surface no tangent plane: whether it passes through is not \
                 decided",
            ),
            Self::UnnamedFaceHasNoSurface => f.write_str("invalid input: a face has no surface"),
            Self::VertexArcsNotOneSphere => f.write_str(
                "the sphere through the three arcs bounding a vertex could not be made",
            ),
            Self::LocalFaceNotListed => f.write_str(
                "invalid input: the face is not among the faces the local operation was given",
            ),
            Self::OffsetBaseKindUnsupported { surface_type } => write!(
                f,
                "the base of an offset surface (type {surface_type}) has no B-spline form, which \
                 the approximation of its offset reads"
            ),
            Self::EdgeOffsetsUnpaired { .. } => f.write_str(
                "the offset pieces on the two sides of an edge do not pair one to one (a split \
                 near a piece's end was refused), so the caps between them are not made",
            ),
            Self::GapEndWithoutEdge { .. } => f.write_str(
                "a gap between an edge's offsets ends at a vertex with no edge, so no corner \
                 blend closes it",
            ),
            Self::GapCornerBlendNotMade { .. } => f.write_str(
                "the corner blend that closes a gap between an edge's offsets could not be made",
            ),
            Self::FaceuseNotGiven => {
                f.write_str("invalid input: the face-use offset was given no face use")
            }
            Self::SelfIntersectionWithoutUvCurve { .. } => f.write_str(
                "a curve where the offset surface meets itself has no UV curve on one side, \
                 so the offset face is not cut along it",
            ),
            Self::FaceuseHasNoFace { .. } => {
                f.write_str("invalid input: a face use of the body has no face")
            }
            Self::BrepNotGiven => f.write_str("invalid input: the operation was given no B-rep"),
            Self::ShellLeavesNoCavity { distance } => write!(
                f,
                "an inward shell by {distance} leaves no offset of the solid: the wall is as \
                 thick as the solid or thicker, so there is no cavity"
            ),
            Self::ShellWallUnresolved { wall, smallest } => write!(
                f,
                "invalid input: the shell's wall {wall:e} is below {smallest:e}, twice the \
                 body's tolerance"
            ),
            Self::ShellFaceNotOfBody { index } => write!(
                f,
                "invalid input: the shell's face selection {index} names no face of the body \
                 it was given"
            ),
            Self::ExtendedFaceCollapses { distance, .. } => write!(
                f,
                "the face's offset surface collapses at the distance {distance}, and the \
                 extended offset builds every face it offsets"
            ),
            Self::TrimPastExtension => f.write_str(
                "an extended offset face's trim reaches past both its offset surface's domain \
                 and the extension's",
            ),
            Self::FaceHasNoFaceuse { .. } => {
                f.write_str("invalid input: a face of the body has no face use")
            }
            Self::FaceuseHasNoShell { .. } => {
                f.write_str("invalid input: a face use of the body is in no shell")
            }
            Self::ShellHasNoRegion { .. } => {
                f.write_str("invalid input: a shell of the body is in no region")
            }
            Self::FaceuseHasNoMate { .. } => {
                f.write_str("invalid input: a face use of the body has no mate")
            }
            Self::EdgeuseHasNoEdge { .. } => {
                f.write_str("invalid input: an edge use of the body has no edge")
            }
            Self::EdgeuseHasNoMate { .. } => {
                f.write_str("invalid input: an edge use of the body has no mate")
            }
            Self::EdgeuseHasNoRadial { .. } => {
                f.write_str("invalid input: an edge use of the body has no radial")
            }
            Self::EdgeuseHasNoFaceuse { .. } => {
                f.write_str("invalid input: an edge use of the body has no face use")
            }
        }
    }
}

/// What a face move's refusal says ([`OffsetFailure::FacesMoveRefused`]).
const fn faces_move_refusal_says(refusal: OffsetFacesRefusal) -> &'static str {
    match refusal {
        OffsetFacesRefusal::None => "not refused",
        OffsetFacesRefusal::NoRoom => "a blend that follows a moved face no longer fits",
        OffsetFacesRefusal::Unsupported => {
            "a blend on a moved face cannot be taken off and put back as it was"
        }
        OffsetFacesRefusal::NotExact => {
            "following a blend needs a face continued that cannot be continued exactly"
        }
        OffsetFacesRefusal::NoEffect => "the local operation moved no face onto its new surface",
        OffsetFacesRefusal::NotPutBack => {
            "the blends that follow the moved faces were not put back"
        }
    }
}

impl crate::SolidError {
    /// [`crate::SolidError::Failed`] for an offset routine that fails with `failure`.
    #[must_use]
    #[track_caller]
    pub(crate) fn offset(operation: &'static str, failure: OffsetFailure) -> Self {
        Failure::new(operation, failure).into_error()
    }

    /// The offset failure behind this error, for a [`crate::SolidError::Failed`] with a
    /// [`Reason::Offset`].
    #[must_use]
    pub fn offset_failure(&self) -> Option<&OffsetFailure> {
        match self {
            Self::Failed(f) => match &f.reason {
                Reason::Offset(o) => Some(o),
                _ => None,
            },
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::error::SM_ERR_ASSERT_FAILURE;
    use crate::SolidError;

    /// The offset failures that replaced a bare status keep its status and operation, and
    /// answer as the status did; their class is their own (a defect's is
    /// [`ErrorKind::StageFailed`]), and so is their entity (the input face a cause names). A sample of each converted file
    /// (`offset_executive/{crossing_loops,cavity,reach,step_gaps}`, `draft`).
    #[test]
    fn offset_failures_keep_the_status_of_the_upstream_they_replace() {
        let face = FaceId::from_index(3);
        let defect = OffsetFailure::Defect { why: "x" };
        // (the operation, the failure, the status kept, its class)
        let samples = [
            (
                "offset::offset_executive::crossing_loops::resolve_crossing_loops",
                defect,
                Some(SM_ERR_ASSERT_FAILURE),
                ErrorKind::StageFailed,
            ),
            (
                "offset::offset_executive::cavity::is_cavity_region",
                defect,
                Some(SM_ERR_NULL_POINTER),
                ErrorKind::StageFailed,
            ),
            (
                "offset::offset_executive::crossing_loops::resolve_crossing_loops",
                OffsetFailure::FaceHasNoSurface { face },
                None,
                ErrorKind::InvalidInput,
            ),
            (
                "offset::draft",
                OffsetFailure::FaceHasNoSurface { face },
                Some(SM_ERR_INVALID_INPUT),
                ErrorKind::InvalidInput,
            ),
            (
                "offset::draft",
                OffsetFailure::FaceHasNoOuterFaceuse { face },
                Some(SM_ERR_INVALID_INPUT),
                ErrorKind::InvalidInput,
            ),
            (
                "offset::offset_executive::step_gaps",
                OffsetFailure::LoopGapPassLimit {
                    face: Some(face),
                    limit: 64,
                },
                None,
                ErrorKind::StageFailed,
            ),
            (
                "offset::offset_executive::step_gaps",
                OffsetFailure::LoopGapEndsAmbiguous {
                    face: None,
                    end_distance: 1e-4,
                    start_distance: 2e-4,
                    tolerance: 1e-3,
                },
                None,
                ErrorKind::StageFailed,
            ),
            (
                "offset::offset_executive::step_gaps",
                OffsetFailure::SplitAtEdgeEnd {
                    face: None,
                    vertex_tolerance: 1e-3,
                },
                None,
                ErrorKind::StageFailed,
            ),
            (
                "offset::op",
                OffsetFailure::FaceDomainUnresolved,
                None,
                ErrorKind::Failed,
            ),
        ];
        let statuses = [
            SM_ERR_ASSERT_FAILURE,
            SM_ERR_NULL_POINTER,
            SM_ERR_NULL_POINTER,
            SM_ERR_INVALID_INPUT,
            SM_ERR_INVALID_INPUT,
            SM_ERR,
            SM_ERR,
            SM_ERR,
            SM_ERR,
        ];
        for ((operation, failure, kept, class), code) in samples.into_iter().zip(statuses) {
            assert!(failure.is_failed_routine());
            let typed = match kept {
                Some(status) => Failure::new(operation, failure).with_status(status),
                None => Failure::new(operation, failure),
            }
            .into_error();
            let before = SolidError::failed_routine_at(operation, code, failure.entity());
            assert_eq!(typed.sm_status(), before.sm_status(), "{typed:?}");
            assert_eq!(typed.failed_routine_status(), Some(code), "{typed:?}");
            assert_eq!(SolidError::failed_routine_status(&typed), Some(code));
            assert_eq!(typed.kind(), class, "{typed:?}");
            assert_eq!(typed.operation(), before.operation(), "{typed:?}");
            assert_eq!(typed.entity(), before.entity(), "{typed:?}");
            assert_eq!(typed.is_port_failure(), before.is_port_failure());
            assert_eq!(typed.is_status_answer(), before.is_status_answer());
            assert_eq!(
                typed.is_unported_or_cancelled(),
                before.is_unported_or_cancelled()
            );
            assert_eq!(typed.offset_failure(), Some(&failure));
        }
        // the face is said once, after the text, as the bare status's entity was
        assert_eq!(
            Failure::new("offset::draft", OffsetFailure::FaceHasNoSurface { face })
                .with_status(SM_ERR_INVALID_INPUT)
                .into_error()
                .to_string(),
            "offset::draft: invalid input: the face has no surface (face 3)"
        );
        assert_eq!(
            SolidError::offset(
                "offset::op",
                OffsetFailure::LoopGapPassLimit {
                    face: Some(face),
                    limit: 64,
                },
            )
            .to_string(),
            "offset::op: stage_failed: the gap search of an offset face's loops closes one gap \
             per pass and reached its limit of 64 passes before a pass found no gap (face 3)"
        );
        // a refusal replaced no status and does not answer as one
        let folds = SolidError::offset(
            "offset::op",
            OffsetFailure::Folds {
                face: FaceId::from_index(1),
                distance: 2.0,
                largest: 1.0,
            },
        );
        assert_eq!(folds.failed_routine_status(), None);
        assert_eq!(
            SolidError::failed_routine("offset::op", SM_ERR).offset_failure(),
            None
        );
    }

    /// Every offset failure's class, and whether it is a defect of the kernel, pinned. A new
    /// reason must answer both (`kind` and `is_defect` match exhaustively) and adds its arm to
    /// `name` and its row here, each at its place in name order, so a caller outside the
    /// crate, which has to match with a wildcard, still gets its class and never only its
    /// text (#15035). Nothing is numbered: the rows are checked to be in name order.
    #[test]
    fn every_offset_failure_keeps_its_class() {
        use OffsetFailure as O;
        let face = FaceId::from_index(1);
        // (the failure, its class, a defect, whether it replaces an upstream status),
        // in name order
        let pinned = [
            (
                O::BlendRailWithoutSupport {
                    edge: EdgeId::from_index(2),
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::BlendSectionNotFound { face },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (O::BrepNotGiven, ErrorKind::InvalidInput, false, true),
            (
                O::Collapses {
                    face,
                    distance: -5.0,
                    radius: 5.0,
                },
                ErrorKind::OffsetCollapses,
                false,
                false,
            ),
            (O::CrossingUndecided, ErrorKind::Unsupported, false, true),
            (
                O::CurveKindUnsupported {
                    edge: None,
                    curve_type: 0,
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            // a failure reviewed as the kernel's fault, with its reason
            (O::Defect { why: "x" }, ErrorKind::StageFailed, true, true),
            (
                O::DistanceUnresolved {
                    distance: 1e-9,
                    resolution: 1e-6,
                },
                ErrorKind::InvalidInput,
                false,
                false,
            ),
            (
                O::DraftClosesWall {
                    face,
                    wall: face,
                    largest: 0.04,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::EdgeHasNoCurve { edge: None },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::EdgeOffsetsUnpaired {
                    edge: EdgeId::from_index(4),
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::EdgeVanishes {
                    edge: None,
                    face: None,
                },
                ErrorKind::OffsetEdgeVanishes,
                false,
                false,
            ),
            (
                O::EdgeuseHasNoEdge {
                    edgeuse: EdgeuseId::from_index(3),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::EdgeuseHasNoFaceuse {
                    edgeuse: EdgeuseId::from_index(3),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::EdgeuseHasNoMate {
                    edgeuse: EdgeuseId::from_index(3),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::EdgeuseHasNoRadial {
                    edgeuse: EdgeuseId::from_index(3),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::EdgeuseHasNoUvTrim { edge: None },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (O::EdgeuseNotGiven, ErrorKind::InvalidInput, false, true),
            (O::ExactCurveUncut, ErrorKind::Unsupported, false, false),
            (
                O::ExtendedCurveOffDomain {
                    edge: None,
                    outside: 12.5,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::ExtendedEdgeUntrimmed,
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ExtendedFaceCollapses {
                    face,
                    distance: 0.5,
                },
                ErrorKind::OffsetCollapses,
                false,
                true,
            ),
            (
                O::FaceCloses {
                    face,
                    beside: [face, face],
                    largest: 0.5,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            // its class is its cause's
            (O::FaceDomainUnresolved, ErrorKind::Failed, false, true),
            (
                O::FaceHasNoFaceuse { face },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::FaceHasNoOuterFaceuse { face },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::FaceHasNoSurface { face },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::FaceMoveEmpty {
                    faces: 0,
                    distance: 1.0,
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (O::FacesApart { face }, ErrorKind::Unsupported, false, false),
            (
                O::FacesMoveRefused {
                    refusal: OffsetFacesRefusal::NoRoom,
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::FaceuseHasNoFace {
                    faceuse: FaceuseId::from_index(2),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::FaceuseHasNoMate {
                    faceuse: FaceuseId::from_index(2),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::FaceuseHasNoShell {
                    faceuse: FaceuseId::from_index(2),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (O::FaceuseNotGiven, ErrorKind::InvalidInput, false, true),
            (
                O::Folds {
                    face,
                    distance: 2.0,
                    largest: 1.0,
                },
                ErrorKind::OffsetFolds,
                false,
                false,
            ),
            (
                O::FollowedBlendIncomplete,
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            // its cause decides
            (
                O::GapCornerBlendNotMade {
                    edge: EdgeId::from_index(4),
                },
                ErrorKind::Failed,
                false,
                true,
            ),
            (
                O::GapEndWithoutEdge {
                    edge: EdgeId::from_index(4),
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (O::LaminaEdgeOtherSide, ErrorKind::Unsupported, false, true),
            (O::LocalFaceNotListed, ErrorKind::InvalidInput, false, true),
            (
                O::LoopGapEndsAmbiguous {
                    face: None,
                    end_distance: 1e-4,
                    start_distance: 2e-4,
                    tolerance: 1e-3,
                },
                ErrorKind::StageFailed,
                true,
                true,
            ),
            (
                O::LoopGapPassLimit {
                    face: None,
                    limit: 64,
                },
                ErrorKind::StageFailed,
                true,
                true,
            ),
            (
                O::LoopGapUncertified { face: None },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::LoopsCross {
                    face: None,
                    crossing: LoopCrossing::SharesEdge,
                },
                ErrorKind::OffsetLoopsCross,
                false,
                false,
            ),
            (
                O::LooseTangentEdge {
                    edge: None,
                    max_drop_to_surf: 0.5,
                    meet_tol: 1e-4,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::MoveCollapses {
                    face,
                    distance: -1.0,
                },
                ErrorKind::OffsetCollapses,
                false,
                true,
            ),
            (
                O::NotCertified {
                    face,
                    distance: 2.0,
                },
                ErrorKind::OffsetFolds,
                false,
                false,
            ),
            (
                O::OffsetBaseKindUnsupported { surface_type: 7 },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            // its class is its cause's
            (O::OffsetNotBuilt, ErrorKind::Failed, false, false),
            (
                O::PastSide {
                    face,
                    side: face,
                    largest: 480.0,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::PeriodicBranchAmbiguous,
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (
                O::RailIntervalsDiffer {
                    first: [0.0, 1.0],
                    second: [0.0, 2.0],
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ReachUncertified { face },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (O::SeamOpened { face }, ErrorKind::Unsupported, false, false),
            (
                O::SelfIntersectionWithoutUvCurve { face },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ShellFaceNotOfBody { index: 1 },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::ShellHasNoRegion {
                    shell: ShellId::from_index(1),
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::ShellLeavesNoCavity { distance: -5.0 },
                ErrorKind::OffsetCollapses,
                false,
                true,
            ),
            (
                O::ShellTangentCurveUncertified,
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ShellTangentUvUnmatched,
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ShellTangentVertexMoveTooFar {
                    moved: 1e-3,
                    bound: 1e-5,
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ShellWallUnresolved {
                    wall: 0.1,
                    smallest: 0.25,
                },
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::SplitAtEdgeEnd {
                    face: None,
                    vertex_tolerance: 1e-3,
                },
                ErrorKind::StageFailed,
                true,
                true,
            ),
            (
                O::TangentBranchUncertified,
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (O::TangentEndOffCurve, ErrorKind::Unsupported, false, false),
            (
                O::TangentGapsAtConcaveEdge,
                ErrorKind::Unsupported,
                false,
                true,
            ),
            (
                O::ToleranceUnresolved { tolerance: 1e-13 },
                ErrorKind::InvalidInput,
                false,
                false,
            ),
            (O::TrimPastExtension, ErrorKind::Unsupported, false, true),
            (
                O::TubeFolds {
                    edge: EdgeId::from_index(2),
                    distance: 1.2,
                    largest: Some(1.0),
                },
                ErrorKind::OffsetFolds,
                false,
                false,
            ),
            (
                O::TubeNotCertified {
                    edge: EdgeId::from_index(2),
                    distance: 1.2,
                },
                ErrorKind::OffsetFolds,
                false,
                false,
            ),
            (
                O::UnnamedFaceHasNoSurface,
                ErrorKind::InvalidInput,
                false,
                true,
            ),
            (
                O::Unsound {
                    what: Unsoundness::NotManifoldSolid,
                },
                ErrorKind::Unsupported,
                false,
                false,
            ),
            (O::VertexArcInPlane, ErrorKind::Unsupported, false, true),
            (
                O::VertexArcMissesPlane {
                    distance: 30.0,
                    radius: 1.0,
                },
                ErrorKind::Unsupported,
                false,
                true,
            ),
            // its cause decides
            (O::VertexArcsNotOneSphere, ErrorKind::Failed, false, true),
            (O::VertexCurveNotArc, ErrorKind::Unsupported, false, true),
            (O::VertexNotGiven, ErrorKind::InvalidInput, false, true),
            (
                O::WallsMeetAcross {
                    face,
                    distance: 1.0,
                    largest: 1.0,
                },
                ErrorKind::OffsetCollapses,
                false,
                false,
            ),
            (
                O::WallsTouch {
                    face: None,
                    contact: WallContact::Touch,
                },
                ErrorKind::OffsetWallsTouch,
                false,
                false,
            ),
        ];
        let name = |o: &O| -> &'static str {
            match o {
                O::BlendRailWithoutSupport { .. } => "BlendRailWithoutSupport",
                O::BlendSectionNotFound { .. } => "BlendSectionNotFound",
                O::BrepNotGiven => "BrepNotGiven",
                O::Collapses { .. } => "Collapses",
                O::CrossingUndecided => "CrossingUndecided",
                O::CurveKindUnsupported { .. } => "CurveKindUnsupported",
                O::Defect { .. } => "Defect",
                O::DistanceUnresolved { .. } => "DistanceUnresolved",
                O::DraftClosesWall { .. } => "DraftClosesWall",
                O::EdgeHasNoCurve { .. } => "EdgeHasNoCurve",
                O::EdgeOffsetsUnpaired { .. } => "EdgeOffsetsUnpaired",
                O::EdgeVanishes { .. } => "EdgeVanishes",
                O::EdgeuseHasNoEdge { .. } => "EdgeuseHasNoEdge",
                O::EdgeuseHasNoFaceuse { .. } => "EdgeuseHasNoFaceuse",
                O::EdgeuseHasNoMate { .. } => "EdgeuseHasNoMate",
                O::EdgeuseHasNoRadial { .. } => "EdgeuseHasNoRadial",
                O::EdgeuseHasNoUvTrim { .. } => "EdgeuseHasNoUvTrim",
                O::EdgeuseNotGiven => "EdgeuseNotGiven",
                O::ExactCurveUncut => "ExactCurveUncut",
                O::ExtendedEdgeUntrimmed => "ExtendedEdgeUntrimmed",
                O::ExtendedFaceCollapses { .. } => "ExtendedFaceCollapses",
                O::FaceCloses { .. } => "FaceCloses",
                O::FaceDomainUnresolved => "FaceDomainUnresolved",
                O::FaceHasNoFaceuse { .. } => "FaceHasNoFaceuse",
                O::FaceHasNoOuterFaceuse { .. } => "FaceHasNoOuterFaceuse",
                O::FaceHasNoSurface { .. } => "FaceHasNoSurface",
                O::FaceMoveEmpty { .. } => "FaceMoveEmpty",
                O::FacesApart { .. } => "FacesApart",
                O::FacesMoveRefused { .. } => "FacesMoveRefused",
                O::FaceuseHasNoFace { .. } => "FaceuseHasNoFace",
                O::FaceuseHasNoMate { .. } => "FaceuseHasNoMate",
                O::FaceuseHasNoShell { .. } => "FaceuseHasNoShell",
                O::FaceuseNotGiven => "FaceuseNotGiven",
                O::Folds { .. } => "Folds",
                O::FollowedBlendIncomplete => "FollowedBlendIncomplete",
                O::GapCornerBlendNotMade { .. } => "GapCornerBlendNotMade",
                O::GapEndWithoutEdge { .. } => "GapEndWithoutEdge",
                O::LaminaEdgeOtherSide => "LaminaEdgeOtherSide",
                O::LocalFaceNotListed => "LocalFaceNotListed",
                O::LoopGapEndsAmbiguous { .. } => "LoopGapEndsAmbiguous",
                O::LoopGapPassLimit { .. } => "LoopGapPassLimit",
                O::LoopGapUncertified { .. } => "LoopGapUncertified",
                O::LoopsCross { .. } => "LoopsCross",
                O::LooseTangentEdge { .. } => "LooseTangentEdge",
                O::ExtendedCurveOffDomain { .. } => "ExtendedCurveOffDomain",
                O::MoveCollapses { .. } => "MoveCollapses",
                O::NotCertified { .. } => "NotCertified",
                O::OffsetBaseKindUnsupported { .. } => "OffsetBaseKindUnsupported",
                O::OffsetNotBuilt => "OffsetNotBuilt",
                O::PastSide { .. } => "PastSide",
                O::PeriodicBranchAmbiguous => "PeriodicBranchAmbiguous",
                O::RailIntervalsDiffer { .. } => "RailIntervalsDiffer",
                O::ReachUncertified { .. } => "ReachUncertified",
                O::SeamOpened { .. } => "SeamOpened",
                O::SelfIntersectionWithoutUvCurve { .. } => "SelfIntersectionWithoutUvCurve",
                O::ShellFaceNotOfBody { .. } => "ShellFaceNotOfBody",
                O::ShellHasNoRegion { .. } => "ShellHasNoRegion",
                O::ShellLeavesNoCavity { .. } => "ShellLeavesNoCavity",
                O::ShellTangentCurveUncertified => "ShellTangentCurveUncertified",
                O::ShellTangentUvUnmatched => "ShellTangentUvUnmatched",
                O::ShellTangentVertexMoveTooFar { .. } => "ShellTangentVertexMoveTooFar",
                O::ShellWallUnresolved { .. } => "ShellWallUnresolved",
                O::SplitAtEdgeEnd { .. } => "SplitAtEdgeEnd",
                O::TangentBranchUncertified => "TangentBranchUncertified",
                O::TangentEndOffCurve => "TangentEndOffCurve",
                O::TangentGapsAtConcaveEdge => "TangentGapsAtConcaveEdge",
                O::ToleranceUnresolved { .. } => "ToleranceUnresolved",
                O::TrimPastExtension => "TrimPastExtension",
                O::TubeFolds { .. } => "TubeFolds",
                O::TubeNotCertified { .. } => "TubeNotCertified",
                O::UnnamedFaceHasNoSurface => "UnnamedFaceHasNoSurface",
                O::Unsound { .. } => "Unsound",
                O::VertexArcInPlane => "VertexArcInPlane",
                O::VertexArcMissesPlane { .. } => "VertexArcMissesPlane",
                O::VertexArcsNotOneSphere => "VertexArcsNotOneSphere",
                O::VertexCurveNotArc => "VertexCurveNotArc",
                O::VertexNotGiven => "VertexNotGiven",
                O::WallsMeetAcross { .. } => "WallsMeetAcross",
                O::WallsTouch { .. } => "WallsTouch",
            }
        };
        let names: Vec<&str> = pinned.iter().map(|(o, ..)| name(o)).collect();
        assert!(
            names.windows(2).all(|w| w[0] < w[1]),
            "each failure is listed once, in name order: {names:?}"
        );
        for (o, class, defect, replaces) in &pinned {
            assert_eq!(o.kind(), *class, "{o:?}");
            assert_eq!(o.is_defect(), *defect, "{o:?}");
            // what replaced a bare status is a defect, or a cause named at a site of `offset_faces` or `offset_geometry_creation` that returned
            // a status; `FaceCloses` and `DraftClosesWall` are refusals of
            // their own, raised where no status was returned
            assert_eq!(o.is_failed_routine(), *replaces, "{o:?}");
        }
    }

    /// A reviewed defect is of class 127 with its site's status, and says why.
    #[test]
    fn a_reviewed_defect_is_class_127_with_its_reason() {
        let why =
            "the offset edge use just built has no mate, which the offset's own pairing gives it";
        let defect = OffsetFailure::Defect { why };
        for status in [SM_ERR, SM_ERR_NULL_POINTER] {
            let e = Failure::new("offset::op", defect)
                .with_status(status)
                .into_error();
            super::super::tests::assert_defect(
                &e,
                &SolidError::failed_routine("offset::op", status),
                "offset::op",
                status,
                &format!("offset::op: stage_failed: {why}"),
            );
            assert_eq!(e.offset_failure(), Some(&defect));
            assert_eq!(e.entity(), None);
        }
        assert_eq!(
            SolidError::offset("offset::op", defect).sm_status(),
            Some(SM_ERR)
        );
    }

    /// The causes named at the bare and stage sites of `offset_faces` and
    /// `offset_geometry_creation` answer with the status of the site they replaced, for the
    /// goldens (`failed_routine_status`), and name the input's entity where they carry one.
    #[test]
    fn the_named_offset_causes_keep_the_status_of_their_site() {
        let edge = EdgeId::from_index(4);
        let face = FaceId::from_index(3);
        let samples = [
            (
                OffsetFailure::FacesMoveRefused {
                    refusal: OffsetFacesRefusal::NoEffect,
                },
                SM_ERR,
                None,
            ),
            (
                OffsetFailure::BlendRailWithoutSupport { edge },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edge(edge)),
            ),
            (
                OffsetFailure::EdgeHasNoCurve { edge: Some(edge) },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edge(edge)),
            ),
            (
                OffsetFailure::FaceHasNoSurface { face },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Face(face)),
            ),
            (
                OffsetFailure::MoveCollapses {
                    face,
                    distance: -2.0,
                },
                SM_ERR,
                Some(EntityRef::Face(face)),
            ),
            (
                OffsetFailure::FaceMoveEmpty {
                    faces: 0,
                    distance: 1.0,
                },
                SM_ERR_INVALID_INPUT,
                None,
            ),
            (
                OffsetFailure::CurveKindUnsupported {
                    edge: None,
                    curve_type: 7,
                },
                SM_ERR_NULL_POINTER,
                None,
            ),
            (OffsetFailure::ExtendedEdgeUntrimmed, SM_ERR, None),
            (OffsetFailure::LocalFaceNotListed, SM_ERR, None),
            (OffsetFailure::VertexArcsNotOneSphere, SM_ERR, None),
            (
                OffsetFailure::UnnamedFaceHasNoSurface,
                SM_ERR_NULL_POINTER,
                None,
            ),
            // and those of `offset_executive::p1`
            (
                OffsetFailure::OffsetBaseKindUnsupported { surface_type: 7 },
                SM_ERR,
                None,
            ),
            (
                OffsetFailure::EdgeOffsetsUnpaired { edge },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edge(edge)),
            ),
            (
                OffsetFailure::GapEndWithoutEdge { edge },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edge(edge)),
            ),
            (
                OffsetFailure::GapCornerBlendNotMade { edge },
                SM_ERR,
                Some(EntityRef::Edge(edge)),
            ),
            (OffsetFailure::FaceuseNotGiven, SM_ERR_NULL_POINTER, None),
            (
                OffsetFailure::SelfIntersectionWithoutUvCurve { face },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Face(face)),
            ),
            (
                OffsetFailure::FaceuseHasNoFace {
                    faceuse: FaceuseId::from_index(2),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Faceuse(FaceuseId::from_index(2))),
            ),
            // and those of `offset_executive::p2`
            (OffsetFailure::BrepNotGiven, SM_ERR_NULL_POINTER, None),
            (
                OffsetFailure::ShellLeavesNoCavity { distance: -5.0 },
                SM_ERR_INVALID_INPUT,
                None,
            ),
            (
                OffsetFailure::ShellWallUnresolved {
                    wall: 0.1,
                    smallest: 0.25,
                },
                SM_ERR_INVALID_INPUT,
                None,
            ),
            (
                OffsetFailure::ShellFaceNotOfBody { index: 1 },
                SM_ERR_INVALID_INPUT,
                None,
            ),
            (
                OffsetFailure::ExtendedFaceCollapses {
                    face,
                    distance: 0.5,
                },
                SM_ERR,
                Some(EntityRef::Face(face)),
            ),
            (OffsetFailure::TrimPastExtension, SM_ERR, None),
            (
                OffsetFailure::FaceHasNoFaceuse { face },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Face(face)),
            ),
            (
                OffsetFailure::FaceuseHasNoShell {
                    faceuse: FaceuseId::from_index(2),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Faceuse(FaceuseId::from_index(2))),
            ),
            (
                OffsetFailure::ShellHasNoRegion {
                    shell: ShellId::from_index(1),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Shell(ShellId::from_index(1))),
            ),
            (
                OffsetFailure::FaceuseHasNoMate {
                    faceuse: FaceuseId::from_index(2),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Faceuse(FaceuseId::from_index(2))),
            ),
            (
                OffsetFailure::EdgeuseHasNoEdge {
                    edgeuse: EdgeuseId::from_index(3),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edgeuse(EdgeuseId::from_index(3))),
            ),
            (
                OffsetFailure::EdgeuseHasNoMate {
                    edgeuse: EdgeuseId::from_index(3),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edgeuse(EdgeuseId::from_index(3))),
            ),
            (
                OffsetFailure::EdgeuseHasNoRadial {
                    edgeuse: EdgeuseId::from_index(3),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edgeuse(EdgeuseId::from_index(3))),
            ),
            (
                OffsetFailure::EdgeuseHasNoFaceuse {
                    edgeuse: EdgeuseId::from_index(3),
                },
                SM_ERR_NULL_POINTER,
                Some(EntityRef::Edgeuse(EdgeuseId::from_index(3))),
            ),
        ];
        for (cause, status, entity) in samples {
            assert!(cause.is_failed_routine(), "{cause:?}");
            assert!(!cause.is_defect(), "{cause:?}");
            let e = SolidError::offset("offset::op", cause);
            assert_eq!(e.failed_routine_status(), Some(status), "{cause:?}");
            assert_eq!(SolidError::failed_routine_status(&e), Some(status));
            assert_eq!(e.sm_status(), Some(status), "{cause:?}");
            assert_eq!(e.entity(), entity, "{cause:?}");
            assert!(!e.is_defect(), "{cause:?}");
        }
        // a solidcore-native refusal (#21743) replaced no status: it is not a failed routine's
        // status, but reports `SM_ERR` to the comparison and names its edge
        let cause = OffsetFailure::ExtendedCurveOffDomain {
            edge: Some(edge),
            outside: 12.5,
        };
        assert!(!cause.is_failed_routine(), "{cause:?}");
        assert!(!cause.is_defect(), "{cause:?}");
        let e = SolidError::offset("offset::op", cause);
        assert_eq!(SolidError::failed_routine_status(&e), None, "{cause:?}");
        assert_eq!(e.sm_status(), Some(SM_ERR), "{cause:?}");
        assert_eq!(e.entity(), Some(EntityRef::Edge(edge)), "{cause:?}");
        assert_eq!(e.kind(), ErrorKind::Unsupported, "{cause:?}");
        // a site that kept another status keeps it through `with_status`
        let e = Failure::new("offset::op", OffsetFailure::FaceHasNoOuterFaceuse { face })
            .with_status(SM_ERR_INVALID_INPUT)
            .into_error();
        assert_eq!(e.failed_routine_status(), Some(SM_ERR_INVALID_INPUT));
        assert_eq!(
            e.to_string(),
            "offset::op: invalid input: the face has no face use on the outside of its body \
             (face 3)"
        );
        // a contextual cause's kept cause decides its class: a defect, a refusal, or
        // neither (an invalid input), whose own class the cause keeps
        for cause in [
            OffsetFailure::FaceDomainUnresolved,
            OffsetFailure::VertexArcsNotOneSphere,
            OffsetFailure::GapCornerBlendNotMade { edge },
        ] {
            let around = |inner: SolidError| {
                Failure::new("offset::op", cause)
                    .caused_by(inner)
                    .into_error()
            };
            let defect = around(SolidError::offset(
                "offset::inner",
                OffsetFailure::Defect { why: "x" },
            ));
            assert!(defect.is_defect() && !defect.is_refusal(), "{cause:?}");
            assert_eq!(defect.failed_routine_status(), Some(SM_ERR));
            let refusal = around(SolidError::offset(
                "offset::inner",
                OffsetFailure::Folds {
                    face,
                    distance: 2.0,
                    largest: 1.0,
                },
            ));
            assert!(refusal.is_refusal() && !refusal.is_defect(), "{cause:?}");
            assert_eq!(refusal.failed_routine_status(), Some(SM_ERR));
            let input = around(SolidError::invalid_input(
                "geom::extent2d::Extent2d::intersect",
                "disjoint",
            ));
            assert!(!input.is_refusal() && !input.is_defect(), "{cause:?}");
            assert_eq!(input.kind(), ErrorKind::Failed);
            assert_eq!(
                input.cause().map(SolidError::kind),
                Some(ErrorKind::InvalidInput)
            );
            assert_eq!(input.failed_routine_status(), Some(SM_ERR));
        }
    }

    /// A body whose edge or vertex tolerance grew past its certificate's bound is refused
    /// naming the input's entity it comes from and both numbers (#15035); the reasons without
    /// an entity name none.
    #[test]
    fn an_unsound_body_names_the_entity_its_check_names() {
        let edge = EntityRef::Edge(EdgeId::from_index(4));
        let grown = |entity| OffsetFailure::Unsound {
            what: Unsoundness::ToleranceBeyondBound {
                entity,
                tolerance: 2e-4,
                bound: 1e-5,
            },
        };
        assert_eq!(grown(Some(edge)).entity(), Some(edge));
        assert_eq!(grown(Some(edge)).kind(), ErrorKind::Unsupported);
        assert_eq!(
            grown(Some(edge)).to_string(),
            "the tolerance at edge 4 of the input grew to 2e-4, past the 1e-5 its certificate \
             allows: refused, the input unchanged"
        );
        // an edge or vertex that comes from none of the input's
        assert_eq!(grown(None).entity(), None);
        assert_eq!(
            grown(None).to_string(),
            "the tolerance of an edge or vertex grew to 2e-4, past the 1e-5 its certificate \
             allows: refused, the input unchanged"
        );
        for (what, says) in [
            (
                Unsoundness::LoopOrientationUndecided,
                "a loop of the body built has a certified area that leaves its orientation \
                 undecided",
            ),
            (
                Unsoundness::LoopGapNotClosable,
                "a gap in a loop of the body built cannot be closed within the tolerances its \
                 certificates allow",
            ),
        ] {
            let o = OffsetFailure::Unsound { what };
            assert_eq!(what.entity(), None);
            assert_eq!(o.entity(), None);
            assert_eq!(
                o.to_string(),
                format!("{says}: refused, the input unchanged")
            );
        }
    }
}
