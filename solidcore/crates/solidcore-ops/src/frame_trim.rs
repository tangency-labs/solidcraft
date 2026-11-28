//! The frame-member trim workflow: a straight frame member laid out along its line, where its ends cross
//! its middle line, the range its section is extruded over, and which end each trim target moves
//! and where it is cut.
//!
//! Each is a recipe of kernel calls (`solidcore::prism::member`): the kernel computes the middle
//! line, the crossings, the butt joints, the cuts against planes and the extrusion range; ops
//! converts the record's planes and vectors (`solidcore-convert`), takes the targets in turn,
//! keeps the end each one moves, and refuses two targets that move the same end or none that
//! meets the member. It computes no geometric quantity.

use solidcore::eval::Frame;
use solidcore::prism::member as km;
use solidcore_convert as convert;
use solidcore_record::{Plane, Vec2, Vec3};

pub use solidcore::prism::member::{Refusal, MAX_REACH};
pub use solidcore_record::frame::{Axis, Cut, End, Extent, MemberData, Side};

/// A frame member laid out: its section's plane at the line's start (the profile's own origin
/// at the plane's origin, its normal along the line), its line and its section's box.
#[derive(Clone, Debug, PartialEq)]
pub struct Member(km::Member);

impl Member {
    /// The member along the line from `start` to `end` with its section in `section`, the
    /// section's box `lo`..`hi` in the plane's coordinates.
    ///
    /// # Errors
    /// [`Refusal::Degenerate`] when the line has no length, the box is empty, a number is not
    /// finite or the plane's axes are not unit and square (`solidcore::prism::member::Member::along`).
    pub fn along(
        section: &Plane,
        start: Vec3,
        end: Vec3,
        lo: Vec2,
        hi: Vec2,
    ) -> Result<Self, Refusal> {
        km::Member::along(
            convert::plane(section),
            convert::vec3(start),
            convert::vec3(end),
            convert::vec2(lo),
            convert::vec2(hi),
        )
        .map(Self)
    }

    /// Its frame, direction, length and section box, field for field.
    #[must_use]
    pub fn data(&self) -> MemberData {
        convert::frame::record_member(&self.0)
    }

    /// The member whose data is `data`, checked as the kernel checks its own members
    /// (`solidcore::prism::member::Member::from_parts`): every member's own [`Member::data`]
    /// makes that member again, field for field.
    ///
    /// # Errors
    /// [`Refusal::Degenerate`] for data [`Member::along`] never gives: a number not finite, a
    /// plane or direction not unit and square, a length not above zero or an empty box.
    pub fn from_data(data: &MemberData) -> Result<Self, Refusal> {
        km::Member::from_parts(
            convert::plane(&data.frame),
            convert::vec3(data.dir),
            data.length,
            convert::vec2(data.lo),
            convert::vec2(data.hi),
        )
        .map(Self)
    }

    /// The line's unit direction.
    #[must_use]
    pub fn dir(&self) -> Vec3 {
        convert::record_vec3(self.0.dir)
    }

    /// The line's length.
    #[must_use]
    pub fn length(&self) -> f64 {
        self.0.length
    }
}

/// What a member is trimmed to.
#[derive(Clone, Copy, Debug)]
pub enum Target<'a> {
    /// A side of another member, with its ends as they are now: the side asked for, or the side
    /// the trimmed member meets.
    Member {
        /// The target member.
        member: &'a Member,
        /// Its ends (start, end).
        ends: &'a [Option<Plane>; 2],
        /// The side asked for.
        side: Option<Side>,
    },
    /// A plane.
    Plane(&'a Plane),
}

/// Why a trim of one member can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TrimRefusal {
    /// The kernel's refusal, for the member (`target` none) or for target `target`.
    Member {
        /// The target, by index, it is about; none: the trimmed member.
        target: Option<usize>,
        /// The kernel's reason.
        why: Refusal,
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

/// The record's ends (start, end) as the kernel's frames.
fn frames(ends: &[Option<Plane>; 2]) -> [Option<Frame>; 2] {
    ends.each_ref().map(|e| e.as_ref().map(convert::plane))
}

/// The middle line of `m` and where its `ends` (start, end: the plane each is cut in, or square
/// at the line's end) cross it.
///
/// # Errors
/// [`Refusal::RunsAlong`] when an end's plane runs along the member.
pub fn axis(m: &Member, ends: &[Option<Plane>; 2]) -> Result<Axis, Refusal> {
    km::axis(&m.0, &frames(ends)).map(|a| convert::frame::record_axis(&a))
}

/// The range `m`'s section is extruded over for its `ends`, and which ends are then cut at a
/// slant in their planes.
///
/// # Errors
/// The kernel's refusal: an end facing the wrong way, too far, or ends that cross.
pub fn extent(m: &Member, ends: &[Option<Plane>; 2]) -> Result<Extent, Refusal> {
    km::extent(&m.0, &frames(ends)).map(|e| convert::frame::record_extent(&e))
}

/// Where the ends (start, end) of `m`, now as `ends`, are cut after trimming it to `targets`: each
/// end a target moves where the kernel cuts it (`gap` short of the target), none for an end no
/// target moves. `end` chooses which end each target moves.
///
/// # Errors
/// A [`TrimRefusal`]: the kernel's for the member or a target, two targets moving one end, or
/// none meeting the member.
pub fn trim(
    m: &Member,
    ends: &[Option<Plane>; 2],
    end: End,
    targets: &[Target<'_>],
    gap: f64,
) -> Result<[Option<Cut>; 2], TrimRefusal> {
    let axis =
        km::axis(&m.0, &frames(ends)).map_err(|why| TrimRefusal::Member { target: None, why })?;
    let mut out: [Option<Cut>; 2] = [None, None];
    let mut by: [Option<usize>; 2] = [None, None];
    for (k, target) in targets.iter().enumerate() {
        let refused = |why| TrimRefusal::Member {
            target: Some(k),
            why,
        };
        let cut = match target {
            Target::Member {
                member,
                ends: target_ends,
                side,
            } => {
                let target_axis = km::axis(&member.0, &frames(target_ends)).map_err(refused)?;
                km::butt(
                    &m.0,
                    &axis,
                    convert::frame::end(end),
                    &member.0,
                    &target_axis,
                    side.map(convert::frame::side),
                    gap,
                )
                .map_err(refused)?
            }
            Target::Plane(plane) => km::to_plane(
                &m.0,
                &axis,
                convert::frame::end(end),
                &convert::plane(plane),
                gap,
            ),
        };
        let Some((which, cut)) = cut else { continue };
        if let Some(first) = by.get(which).copied().flatten() {
            return Err(TrimRefusal::SameEnd {
                first,
                second: k,
                end: which,
            });
        }
        if let (Some(slot), Some(e)) = (by.get_mut(which), out.get_mut(which)) {
            *slot = Some(k);
            *e = Some(convert::frame::record_cut(&cut));
        }
    }
    if by.iter().all(Option::is_none) {
        return Err(TrimRefusal::MeetsNone);
    }
    Ok(out)
}
