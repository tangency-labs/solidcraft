//! Frames ([`Frames`]): frame-stock paths of lines and arcs (their pieces, traversal, section
//! planes and bend clearance), straight members trimmed to planes and to each other, and gusset
//! plates between two support faces. Their values are the record's
//! (`solidcore_record::frame`); the refusals are mirrored here, so the application reads why
//! without holding a kernel's types.

use solidcore_record::frame::{Axis, Cut, Dimensions, End, Extent, MemberData, PieceData, Placement, Prepared, Side, SupportData, SupportFrames};
use solidcore_record::{Plane, Region2, Seg2, Vec2, Vec3};

use crate::Backend;

/// Why a frame-stock path or its section can't be used.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PathRefusal {
    /// A line has no finite direction or has coordinates outside the model's range.
    Line,
    /// The circle data do not define one finite open arc.
    Arc,
    /// No finite plane can be laid square to the starting tangent.
    Frame,
    /// Two consecutive curves do not meet; the index names the later curve.
    Connection(usize),
    /// Two consecutive curves do not have the same tangent; the index names the later curve.
    Tangency(usize),
    /// The path has no two distinct ends.
    Closed,
    /// The section reaches the bend axis; the index names that arc.
    Bend(usize),
    /// The section has no finite certified bound for bend clearance.
    Profile,
    /// The path is empty or longer than the kernel takes.
    Count,
}

/// A frame-stock path refused, or a kernel call that failed, in the kernel's words.
#[derive(Clone, Debug, PartialEq, thiserror::Error)]
pub enum PathFailure {
    /// The kernel refuses the path or its section.
    #[error("{words}")]
    Refused {
        /// Why.
        why: PathRefusal,
        /// The kernel's words for it.
        words: String,
    },
    /// The kernel call itself failed, in its words.
    #[error("{0}")]
    Failed(String),
}

/// Why a straight member, or an end of it, can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MemberRefusal {
    /// The line has no length, or the section box is empty.
    Degenerate,
    /// An end's plane runs along the member.
    RunsAlong,
    /// An end's plane isn't finite, runs along the member, or faces into it.
    WrongWay,
    /// An end would reach farther from the line's start than the kernel takes
    /// ([`Frames::MAX_REACH`]).
    TooFar,
    /// The ends cross inside the section: nothing of the member is left.
    EndsCross,
    /// The side asked for of the target member faces away from the member butted against it.
    FacesAway,
}

/// Why a trim of one member can't be made ([`Frames::trim_member`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimRefusal {
    /// The kernel's refusal, for the member (`target` none) or for target `target`.
    Member {
        /// The target, by index, it is about; none: the trimmed member.
        target: Option<usize>,
        /// The kernel's reason.
        why: MemberRefusal,
    },
    /// Targets `first` and `second` (by index) both move end `end` (0 the start, 1 the end).
    SameEnd {
        /// The first target that moves it.
        first: usize,
        /// The second.
        second: usize,
        /// The end.
        end: usize,
    },
    /// None of the targets meets the member.
    MeetsNone,
}

/// What a member is trimmed to ([`Frames::trim_member`]).
#[derive(Clone, Copy, Debug)]
pub enum TrimTarget<'a> {
    /// A side of another member, with its ends as they are now (start, end: the plane each is cut
    /// in, or square at the line's end): the side asked for, or the side the trimmed member meets.
    Member {
        /// The target member.
        member: &'a MemberData,
        /// Its ends.
        ends: &'a [Option<Plane>; 2],
        /// The side asked for.
        side: Option<Side>,
    },
    /// A plane.
    Plane(&'a Plane),
}

/// A gusset's support or plate refused with the kernel's reason, or the kernel's own error.
#[derive(Clone, Debug, PartialEq)]
pub enum GussetFailure<E> {
    /// Refused, with why in the kernel's words: a support off the first face's plane, open or
    /// over-budget loops, no material, a contact that doesn't fit.
    Refused(String),
    /// The kernel's own error.
    Kernel(E),
}

/// The frame geometry of one kernel ([`crate::OpKind::Frames`]).
pub trait Frames: Backend {
    /// The most a stock member may lean from vertical and still have its section follow world up.
    const VERTICAL_TOLERANCE: f64;
    /// The farthest (mm) a member's end may move from its line's start.
    const MAX_REACH: f64;

    /// One finite straight piece of stock from `a` to `b`.
    fn stock_line(&self, a: Vec3, b: Vec3) -> Result<PieceData, PathFailure>;
    /// One open spatial arc from `a` to `b` on the circle of `center`, unit `normal` and
    /// `radius`, turning positively about the normal.
    fn stock_spatial_arc(&self, a: Vec3, b: Vec3, center: Vec3, normal: Vec3, radius: f64) -> Result<PieceData, PathFailure>;
    /// A sketch line or arc on `plane`, placed in world coordinates.
    fn stock_planar(&self, plane: &Plane, segment: &Seg2) -> Result<PieceData, PathFailure>;
    /// The line from `start` to `end` over its parameter `interval`.
    fn stock_retained_line(&self, start: Vec3, end: Vec3, interval: [f64; 2]) -> Result<PieceData, PathFailure>;
    /// The length of `piece` when it is a line; `None` for a bend.
    fn stock_line_length(&self, piece: &PieceData) -> Result<Option<f64>, PathFailure>;
    /// `pieces` turned as needed to run along one open tangent chain in the order given. They
    /// change only on success.
    fn orient_stock(&self, pieces: &mut [PieceData]) -> Result<(), PathFailure>;
    /// A section's x and y axes for a member along `direction`: y as near world up as it can be,
    /// then both turned by `turn` (radians) about the path. `None` for a direction without one.
    fn member_axes(&self, direction: Vec3, turn: f64) -> Result<Option<(Vec3, Vec3)>, PathFailure>;
    /// The section plane at the start of `first`, with the section anchored at `at` and turned
    /// by `turn`.
    fn stock_section_plane(&self, first: &PieceData, at: Vec2, turn: f64) -> Result<Plane, PathFailure>;
    /// The section planes at both ends of `path`, using the sweep's transported frame.
    fn stock_terminal_planes(&self, path: &[PieceData], at: Vec2, turn: f64) -> Result<[Plane; 2], PathFailure>;
    /// Whether `region`, anchored at `at` and turned by `turn`, clears every bend of `path`.
    fn check_bend_clearance(&self, path: &[PieceData], region: &Region2, at: Vec2, turn: f64) -> Result<(), PathFailure>;

    /// The member along the line from `start` to `end` with its section in `section`, the
    /// section's box `lo`..`hi` in the plane's coordinates.
    fn member_along(&self, section: &Plane, start: Vec3, end: Vec3, lo: Vec2, hi: Vec2) -> Result<MemberData, MemberRefusal>;
    /// The middle line of `member` and where its `ends` (start, end: the plane each is cut in,
    /// or square at the line's end) cross it.
    fn member_axis(&self, member: &MemberData, ends: &[Option<Plane>; 2]) -> Result<Axis, MemberRefusal>;
    /// The range `member`'s section is extruded over for its `ends`, and which ends are then cut
    /// at a slant in their planes.
    fn member_extent(&self, member: &MemberData, ends: &[Option<Plane>; 2]) -> Result<Extent, MemberRefusal>;
    /// Where the ends (start, end) of `member`, now as `ends`, are cut after trimming it to
    /// `targets`, `gap` short of each: none for an end no target moves. `end` chooses which end
    /// each target moves.
    fn trim_member(
        &self,
        member: &MemberData,
        ends: &[Option<Plane>; 2],
        end: End,
        targets: &[TrimTarget<'_>],
        gap: f64,
    ) -> Result<[Option<Cut>; 2], TrimRefusal>;

    /// The plane a gusset support's faces share: `candidate` when it is the first, else `first`
    /// when `candidate` lies on it (within `tolerance`, for a body of `size`).
    fn gusset_support_plane(&self, first: Option<&Plane>, candidate: &Plane, size: f64, tolerance: f64) -> Result<Plane, GussetFailure<Self::Error>>;
    /// A gusset support's material: each face's loops of trimmed edge pieces (`faces`, in the
    /// source plane's coordinates) joined into regions, holes kept.
    fn gusset_support(&self, frames: &SupportFrames, faces: &[Vec<Vec<Seg2>>]) -> Result<SupportData, GussetFailure<Self::Error>>;
    /// The gusset plate between `supports` with `dimensions` on `placement`'s side of the joint,
    /// both contact strips proved to sit on their supports' material.
    fn gusset_prepare(
        &self,
        supports: [&SupportData; 2],
        dimensions: Dimensions,
        placement: Placement,
    ) -> Result<Prepared, GussetFailure<Self::Error>>;
    /// A saved support pick `at` put back on `plane`, for an edit dialog.
    fn gusset_support_pick(&self, plane: &Plane, at: Vec3) -> Vec3;
}
