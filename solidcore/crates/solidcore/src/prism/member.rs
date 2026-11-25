//! Straight members: a planar section run along a line from the line's start, each end square at
//! the line's end or cut in a plane. Where a member's middle line is and where its ends cross it,
//! the plane an end moves to when the member is trimmed or extended to a plane or butted against
//! a side of another member, and the range its section is extruded over before slanted ends are
//! cut.
//!
//! Not upstream: SolidCraft's frame-member trim geometry (its document's `eval_frame.rs`), moved
//! into the kernel so the layers above compute no geometry (docs/layers.md §1). The trim workflow
//! that sequences these is `solidcore-ops`' (`frame_trim`).
//!
//! | Rust              | What it answers                                                  |
//! |-------------------|------------------------------------------------------------------|
//! | [`Member::along`] | a member from its section's frame, its line and its section box  |
//! | [`axis`]          | the middle line, and where each end crosses it                   |
//! | [`extent`]        | the extrusion range, and which ends are cut at a slant           |
//! | [`to_plane`]      | the end that moves to a plane, and where it is cut               |
//! | [`butt`]          | the end that butts against a side of another member, and its cut |
//! | [`Cut`]           | an end's cut: a point on its plane and its unit normal           |
//!
//! Deviations: none (no upstream counterpart). No recursion.

#![allow(clippy::neg_cmp_op_on_partial_ord)] // NaN-refusing tests, kept as written

use super::profile::{axes, to_local, to_world};
use crate::eval::Frame;
use crate::geom::geom_utility::p1::{line_line_closest_point, line_plane_intersect};
use crate::geom::vector3d::{Point3, Vec3};
use crate::math::{Point2, Vec2};

/// A plane meets a member when its normal leans at least this far (the cosine) from square to the
/// member's line: a plane closer to running along the member does not cut it.
pub const MIN_CROSSING: f64 = 1e-6;
/// Farthest (mm) an end may move from its line's start.
pub const MAX_REACH: f64 = 1e6;
/// Shortest (mm) a member may be along any line through its section.
pub const MIN_LENGTH: f64 = 1e-6;

/// Two lines are alongside each other, for a butt joint, when the squared sine of the angle
/// between them is at most this.
const ALONGSIDE: f64 = 1e-12;

/// A member laid out: its section's frame at the line's start (the profile's own origin at the
/// frame's origin, `x × y` along the line), the line's unit direction and length, and the
/// section's bounding box in the frame's coordinates.
#[derive(Clone, Debug, PartialEq)]
pub struct Member {
    /// The section's frame at the line's start.
    pub frame: Frame,
    /// The line's unit direction.
    pub dir: Vec3,
    /// The line's length.
    pub length: f64,
    /// The section box's low corner, in the frame's coordinates.
    pub lo: Point2,
    /// Its high corner.
    pub hi: Point2,
}

/// Why a member, or an end of it, can't be made.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Refusal {
    /// The line has no length, or the section box is empty.
    Degenerate,
    /// An end's plane runs along the member.
    RunsAlong,
    /// An end's plane isn't finite, runs along the member, or faces into it (a start's normal
    /// points back along the line, an end's on along it).
    WrongWay,
    /// An end would reach more than [`MAX_REACH`] from the line's start.
    TooFar,
    /// The ends cross inside the section: nothing of the member is left.
    EndsCross,
    /// The side asked for of the target member faces away from the member butted against it.
    FacesAway,
}

/// Which end moves: the one nearer the target, or the start or the end of the line.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum End {
    /// The end nearer the target.
    Nearer,
    /// The line's start.
    Start,
    /// The line's end.
    End,
}

/// A side of a member's section box, in its frame's x and y (`Right` is +x, `Top` +y).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Side {
    /// +x.
    Right,
    /// -x.
    Left,
    /// +y.
    Top,
    /// -y.
    Bottom,
}

/// A member's middle line (through the middle of its section box): its point at the line's
/// start, how far along the line from there each end crosses it, and the length between them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Axis {
    /// The middle line's point at the line's start.
    pub centre: Point3,
    /// Where the start crosses it, along the line from `centre`.
    pub t0: f64,
    /// Where the end crosses it.
    pub t1: f64,
    /// The length between them, `t1 - t0`.
    pub length: f64,
}

/// Where an end is cut: a point on its plane and the plane's unit normal, out of the member. The
/// plane's in-plane axes are the caller's to choose.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cut {
    /// A point on the plane.
    pub origin: Point3,
    /// The unit normal, out of the member.
    pub normal: Vec3,
}

/// The range along the line a member's section is extruded over, and which ends (start, end)
/// are then cut at a slant in their planes. A square end is the range's own end.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Extent {
    /// Where the extrusion starts, along the line from the section's frame.
    pub from: f64,
    /// Where it ends.
    pub to: f64,
    /// Whether the start and the end are cut at a slant.
    pub cut: [bool; 2],
}

impl Member {
    /// The member along the line from `start` to `end`, its section in `frame` with the box
    /// `lo`..`hi`.
    ///
    /// # Errors
    /// [`Refusal::Degenerate`] when the line has no length or the box is empty.
    pub fn along(
        frame: Frame,
        start: Point3,
        end: Point3,
        lo: Point2,
        hi: Point2,
    ) -> Result<Self, Refusal> {
        let length = start.distance(end);
        let dir = (end - start).unitize().map_err(|_| Refusal::Degenerate)?;
        if !(length > 0.0 && lo.x <= hi.x && lo.y <= hi.y) {
            return Err(Refusal::Degenerate);
        }
        Ok(Self {
            frame,
            dir,
            length,
            lo,
            hi,
        })
    }

    /// The middle of the section box.
    fn middle(&self) -> Point2 {
        Vec2::new((self.lo.x + self.hi.x) * 0.5, (self.lo.y + self.hi.y) * 0.5)
    }

    /// The section box's corners at the line's start, in one order for both ends.
    fn corners(&self) -> [Point3; 4] {
        let (lo, hi) = (self.lo, self.hi);
        [lo, Vec2::new(hi.x, lo.y), hi, Vec2::new(lo.x, hi.y)]
            .map(|c| to_world(&self.frame, c, 0.0))
    }
}

/// How far along `dir` from `p` the line crosses `plane`.
fn crossing(dir: Vec3, p: Point3, plane: &Frame) -> Result<f64, Refusal> {
    let (o, _, _, n) = axes(plane);
    if !(n.dot(dir).abs() >= MIN_CROSSING) {
        return Err(Refusal::RunsAlong);
    }
    line_plane_intersect(p, dir, o, n).map_err(|_| Refusal::RunsAlong)
}

/// The middle line of `m` and where its `ends` (start, end: a plane each, or square at the line's
/// end) cross it.
///
/// # Errors
/// [`Refusal::RunsAlong`] when an end's plane runs along the member.
pub fn axis(m: &Member, ends: &[Option<Frame>; 2]) -> Result<Axis, Refusal> {
    let centre = to_world(&m.frame, m.middle(), 0.0);
    let at = |end: &Option<Frame>, default: f64| match end {
        None => Ok(default),
        Some(plane) => crossing(m.dir, centre, plane),
    };
    let [start, end] = ends;
    let (t0, t1) = (at(start, 0.0)?, at(end, m.length)?);
    Ok(Axis {
        centre,
        t0,
        t1,
        length: t1 - t0,
    })
}

/// The range `m`'s section is extruded over for its `ends` (normals out of the member), and which
/// ends are cut at a slant. The ends must leave the whole section between them: their separation
/// is affine over the section, so the box's corresponding corners decide it, even where two
/// slanted ends overlap along the line.
///
/// # Errors
/// [`Refusal::WrongWay`], [`Refusal::TooFar`] or [`Refusal::EndsCross`].
pub fn extent(m: &Member, ends: &[Option<Frame>; 2]) -> Result<Extent, Refusal> {
    let corners = m.corners();
    let finite = |v: Vec3| v.x.is_finite() && v.y.is_finite() && v.z.is_finite();
    // How far along the line an end plane is at each corner.
    let reach = |end: &Option<Frame>, default: f64, start: bool| -> Result<[f64; 4], Refusal> {
        let Some(plane) = end else {
            return Ok([default; 4]);
        };
        let (o, _, _, n) = axes(plane);
        let nd = n.dot(m.dir);
        if !(finite(n) && finite(o) && nd.abs() >= MIN_CROSSING) || (nd < 0.0) != start {
            return Err(Refusal::WrongWay);
        }
        let mut out = [default; 4];
        for (t, c) in out.iter_mut().zip(corners) {
            *t = line_plane_intersect(c, m.dir, o, n).map_err(|_| Refusal::WrongWay)?;
        }
        Ok(out)
    };
    let [start, end] = ends;
    let s = reach(start, 0.0, true)?;
    let e = reach(end, m.length, false)?;
    if !s
        .iter()
        .chain(&e)
        .all(|t| t.is_finite() && t.abs() <= MAX_REACH)
    {
        return Err(Refusal::TooFar);
    }
    if !s.iter().zip(&e).all(|(a, b)| b - a > MIN_LENGTH) {
        return Err(Refusal::EndsCross);
    }
    let span = |ts: &[f64; 4]| {
        let (lo, hi) = ts
            .iter()
            .fold((f64::INFINITY, f64::NEG_INFINITY), |(lo, hi), &t| {
                (lo.min(t), hi.max(t))
            });
        let slants = hi - lo > 1e-9 * (1.0 + lo.abs());
        if slants {
            (lo, hi, true)
        } else {
            (lo, lo, false)
        }
    };
    let (s_lo, s_hi, s_cut) = span(&s);
    let (e_lo, e_hi, e_cut) = span(&e);
    // A slanted end: run the section past it, so the cut in its plane meets material everywhere.
    let from = if s_cut {
        s_lo - (s_hi - s_lo) - 1.0
    } else {
        s_lo
    };
    let to = if e_cut {
        e_hi + (e_hi - e_lo) + 1.0
    } else {
        e_hi
    };
    Ok(Extent {
        from,
        to,
        cut: [s_cut, e_cut],
    })
}

/// The end (0 the start, 1 the end) of a member whose axis is `axis` that moves to a target
/// crossing it at `t`: the nearer one, or the one asked for.
fn which_end(t: f64, axis: &Axis, end: End) -> usize {
    match end {
        End::Start => 0,
        End::End => 1,
        End::Nearer => usize::from((t - axis.t0).abs() > (t - axis.t1).abs()),
    }
}

/// The end of `m` (axis `axis`) that moves to `plane`, and where it is then cut: `gap` short of
/// `plane`, its normal out of the member. `None` when `m` runs along `plane`.
#[must_use]
pub fn to_plane(
    m: &Member,
    axis: &Axis,
    end: End,
    plane: &Frame,
    gap: f64,
) -> Option<(usize, Cut)> {
    let t = crossing(m.dir, axis.centre, plane).ok()?;
    let which = which_end(t, axis, end);
    // The member stays on the side its kept end is: the normal turned towards it.
    let keep = if which == 0 { m.dir } else { -m.dir };
    let (o, _, _, n) = axes(plane);
    let n = n.unitize().ok()?;
    let n = if n.dot(keep) < 0.0 { -n } else { n };
    Some((
        which,
        Cut {
            origin: o + n * gap,
            normal: -n,
        },
    ))
}

/// The end of `m` (axis `axis`) that butts against a side of `target` (axis `target_axis`), and
/// where it is then cut: `gap` short of that side, its normal out of `m`. No side asked
/// for: the side `m`'s kept end lies beyond. `None` when `m`'s middle line runs alongside
/// `target`, or misses its section or its length.
///
/// # Errors
/// [`Refusal::FacesAway`] when the side asked for faces away from `m`.
pub fn butt(
    m: &Member,
    axis: &Axis,
    end: End,
    target: &Member,
    target_axis: &Axis,
    side: Option<Side>,
    gap: f64,
) -> Result<Option<(usize, Cut)>, Refusal> {
    let (d, td) = (m.dir, target.dir);
    if !(d.cross(td).length_squared() > ALONGSIDE) {
        return Ok(None);
    }
    // Where the two middle lines come closest: along `m` from its centre, along the target from
    // its.
    let Ok((t, s)) = line_line_closest_point(axis.centre, d, target_axis.centre, td) else {
        return Ok(None);
    };
    // That point must lie in the target's section and along its length.
    let uv = to_local(&target.frame, axis.centre + d * t);
    let size = Vec2::new(target.hi.x - target.lo.x, target.hi.y - target.lo.y).length();
    let tol = 1e-6 * (1.0 + size + target.length);
    let inside = uv.x >= target.lo.x - tol
        && uv.x <= target.hi.x + tol
        && uv.y >= target.lo.y - tol
        && uv.y <= target.hi.y + tol
        && s >= target_axis.t0 - tol
        && s <= target_axis.t1 + tol;
    if !inside {
        return Ok(None);
    }
    let which = which_end(t, axis, end);
    let keep = if which == 0 { d } else { -d };
    let (o, x, y, _) = axes(&target.frame);
    let sides = [
        (Side::Right, x, o + x * target.hi.x),
        (Side::Left, -x, o + x * target.lo.x),
        (Side::Top, y, o + y * target.hi.y),
        (Side::Bottom, -y, o + y * target.lo.y),
    ];
    let pick = match side {
        Some(want) => sides.iter().find(|(s, _, _)| *s == want),
        None => sides
            .iter()
            .max_by(|a, b| a.1.dot(keep).total_cmp(&b.1.dot(keep))),
    };
    let Some(&(_, n, q)) = pick else {
        return Ok(None);
    };
    if !(n.dot(keep) >= MIN_CROSSING) {
        return Err(Refusal::FacesAway);
    }
    // The cut faces out of the member: back towards the target.
    Ok(n.unitize().ok().map(|n| {
        (
            which,
            Cut {
                origin: q + n * gap,
                normal: -n,
            },
        )
    }))
}
