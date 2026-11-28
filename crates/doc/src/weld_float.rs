//! Floating welds (T79): a fillet or groove weld along a sketched path, with no part at all: a
//! sketch's lines and arcs, a 3D sketch's, or construction lines. The section is the kernel's
//! fill of the channel ([`fill_channel`]), as on a part; it is carried along the path piece by
//! piece, each line extruded and each arc turned about its axis, so every piece is exact.
//!
//! SolidCraft's own: solidcore and solidcore-ops hold no weld (bherbruck on #18409). Nothing here
//! reads or writes a part. A spot weld needs nothing new to float: [`crate::weld_spot::spot_weld`]
//! takes a point and a normal, no part (a sketch point and the sketch's normal).
//!
//! | Rust              | What it is                                                          |
//! |-------------------|---------------------------------------------------------------------|
//! | [`FloatSection`]  | The weld's section: a fillet's legs, or a groove's two walls         |
//! | [`float_weld`]    | The weld carried along the path                                      |
//! | [`FloatWeld`]     | What [`float_weld`] gives: one body per weld, the fill, the notes    |
//! | [`FloatNote`]     | What the weld tool tells the user about a weld that built            |
//! | [`FloatRefusal`]  | Why a path or section is refused                                     |
//!
//! # The frame
//!
//! At the path's start the section lies in the plane square to the path: `y` is `up` (the
//! sketch's normal by default, Q19), `x` is `up × tangent`, left of the path seen from `up`'s tip
//! (Q181), so `x × y` is the path's direction. A line keeps the frame; an arc turns it with the
//! path. Where two pieces meet at an angle (a corner), the frame turns about the line square to
//! both pieces, by the angle between them, and the part of the section that turn moves forward
//! is turned with it: on the outside of the bend that closes the gap (wrapped round, Q03 a), on the
//! inside nothing is added and the pieces' overlap is united (Q182, Q198). A path that doubles back
//! on itself (a U-turn) has no line to turn about: the weld is split there into two (Q182) and the
//! frame carries on unchanged.
//!
//! # Not covered
//!
//! Free curves (a sketch's spline or conic, a 3D sketch's spline) are refused as not built yet
//! ([`FloatRefusal::FreeCurve`]): the kernel's sweep along them is fitted, not proved; a kernel
//! call that replaces a curve by arcs proved to lie within a set distance of it switches them on
//! later (Q200). An overlap at a corner longer than the pieces beside it is united like any other,
//! with no notice (Q199 asks for one): shortcut: no check of a piece's length against its
//! corners' overlap, add one if such a path turns up.

use solidcore_record::{Loop2, Plane, Region2, Seg2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{
    Body, BoolOp, ChannelFill, FillFailure, FillOptions, KernelError, PathSeg, affine, extrude_planar_regions, fill_channel, planar_regions_boolean,
    profiles, revolve_whole, union_all,
};

/// A floating weld's section, in the plane square to the path at its start (`x` left of the path,
/// `y` up, as in the module's docs).
#[derive(Debug, Clone, PartialEq)]
pub enum FloatSection {
    /// A fillet: its first leg `legs[0]` along `x` (in the sketch's plane), its second `legs[1]`
    /// at `angle` (radians, between 0 and pi; pi / 2 by default) from the first toward `y` (Q183).
    /// `flip_side` puts the first leg on the path's right, `flip_up` the second below the plane.
    Fillet { legs: [f64; 2], angle: f64, flip_side: bool, flip_up: bool },
    /// A groove between two sketched walls, `[left, right]`, each a chain of lines and arcs from
    /// its toe as [`fill_channel`] takes them; the depth along `-depth_normal` where it is given.
    Groove { walls: [Vec<Seg2>; 2], depth_normal: Option<Vec2> },
}

/// What [`float_weld`] gives.
#[derive(Debug)]
pub struct FloatWeld {
    /// One body per weld: one, or more where the path doubles back on itself (each U-turn splits).
    pub welds: Vec<Body>,
    /// The section's fill: its region (in the start's frame), its depths, what a caller may report.
    pub fill: ChannelFill,
    /// What the weld tool tells the user (Q199: unusual corners only).
    pub notes: Vec<FloatNote>,
}

/// What the weld tool tells the user about a floating weld that built.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum FloatNote {
    /// At joint `joint` (between piece `joint - 1` and piece `joint`; 0 where a closed path meets
    /// its start) the path turns `angle`
    /// radians out of the section's plane (a 3D sketch's corner): the section was carried round.
    CornerOutOfPlane { joint: usize, angle: f64 },
    /// At joint `joint` the path doubles back on itself: the weld is split there into two.
    UTurn { joint: usize },
}

/// Why a floating weld is refused.
#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum FloatRefusal {
    /// Input the weld cannot take.
    #[error("{0}")]
    Invalid(&'static str),
    /// Piece `joint` does not start where piece `joint - 1` ends.
    #[error("piece {joint} of the path does not start where the piece before it ends")]
    NotJoined { joint: usize },
    /// Piece `piece` is a free curve (a spline or conic): not built yet (Q200).
    #[error("piece {piece} of the path is a free curve: welds along splines are not built yet")]
    FreeCurve { piece: usize },
    /// Arc `piece` turns tighter than the section reaches toward its centre: its `radius` is no
    /// more than `reach`, so the section would cross the arc's axis.
    #[error("arc {piece} of the path (radius {radius:.2}) turns tighter than the weld reaches toward its centre ({reach:.2})")]
    ArcTighterThanSection { piece: usize, radius: f64, reach: f64 },
    /// The section cannot be filled as asked ([`FillFailure`]); `fit` is the nearest size that
    /// builds, where there is one.
    #[error("the weld's section cannot be filled as asked: {why}{}", fit.map(|f| format!("; {f:.2} mm builds")).unwrap_or_default())]
    Fill { why: FillFailure, fit: Option<f64> },
    /// The kernel refused a number, in its words.
    #[error(transparent)]
    Profile(#[from] profiles::ProfileRefused),
    /// A placement call refused its numbers, in the kernel's words.
    #[error(transparent)]
    Placement(#[from] affine::AffineRefused),
    /// A kernel call failed.
    #[error(transparent)]
    Kernel(#[from] KernelError),
}

type Result<T> = std::result::Result<T, FloatRefusal>;

/// The weld of `section` along `path` (lines and arcs, each starting where the one before ends),
/// its frame's `up` at the path's start (the sketch's normal, or a direction the user picked for a
/// 3D sketch; need not be unit or square to the path, only not along it), filled as `options`
/// asks. Nothing is read or written but the path.
///
/// # Errors
/// [`FloatRefusal::Invalid`] for an empty path, a piece of no length, an `up` along the path, legs
/// or an angle out of range; [`FloatRefusal::NotJoined`]; [`FloatRefusal::FreeCurve`];
/// [`FloatRefusal::ArcTighterThanSection`]; [`FloatRefusal::Fill`] with the size that builds; the
/// kernel's refusals otherwise.
pub fn float_weld(path: &[PathSeg], up: [f64; 3], section: &FloatSection, options: &FillOptions) -> Result<FloatWeld> {
    if path.is_empty() {
        return Err(FloatRefusal::Invalid("a floating weld needs a path"));
    }
    let fill = fill_section(section, options)?;
    let region = fill.region.clone();
    let zone = profiles::zone_tolerance()?;
    let mut welds = Vec::new();
    let mut notes = Vec::new();
    let mut bodies: Vec<Body> = Vec::new();
    // the frame at the end of the piece before: where it ends, its direction, up
    let mut before: Option<(Vec3, Vec3, Vec3)> = None;
    // where the path starts and its direction there, to close a path that ends where it starts
    let mut first: Option<(Vec3, Vec3)> = None;
    for (i, piece) in path.iter().enumerate() {
        let (start, tangent) = match *piece {
            PathSeg::Line { a, b } if len(b - a) > zone => (a, b - a),
            PathSeg::Line { .. } => return Err(FloatRefusal::Invalid("a path piece of no length")),
            PathSeg::Arc { a, center, axis, angle } => (a, arc_tangent(a, center, axis, angle)?),
            PathSeg::Curve { .. } | PathSeg::Bezier(_) => return Err(FloatRefusal::FreeCurve { piece: i }),
        };
        let up_here = match before {
            None => {
                // refused where `up` has no part square to the path
                affine::frame_axes(tangent.into(), up).map_err(|_| FloatRefusal::Invalid("an up direction along the path"))?;
                Vec3::from(up)
            }
            Some((end, t_before, n_before)) => {
                if len(start - end) > zone {
                    return Err(FloatRefusal::NotJoined { joint: i });
                }
                let [t, _] = affine::orthonormal_axes(tangent.into(), n_before.into())?;
                match corner(&region, end, t_before, n_before, t.into(), zone)? {
                    Corner::Straight => n_before,
                    Corner::UTurn => {
                        notes.push(FloatNote::UTurn { joint: i });
                        welds.push(united(&bodies)?);
                        bodies.clear();
                        n_before
                    }
                    Corner::Turned { up, fill, out_of_plane } => {
                        bodies.extend(fill);
                        if let Some(angle) = out_of_plane {
                            notes.push(FloatNote::CornerOutOfPlane { joint: i, angle });
                        }
                        up
                    }
                }
            }
        };
        let [t, n] = affine::orthonormal_axes(tangent.into(), up_here.into())?;
        let (t, n): (Vec3, Vec3) = (t.into(), n.into());
        first.get_or_insert((start, t));
        let plane = Plane { origin: start, x: left_of(t, n)?, y: n };
        match *piece {
            PathSeg::Line { a, b } => {
                bodies.push(extrude_planar_regions(&plane, std::slice::from_ref(&region), 0.0, len(b - a))?);
                before = Some((b, t, n));
            }
            PathSeg::Arc { a, center, axis, angle } => {
                let radius = len(a - center);
                let centre = plane.to_local(center);
                let reach = reach(&region, Vec2::new(centre.x / radius, centre.y / radius))?;
                if reach >= radius - zone {
                    return Err(FloatRefusal::ArcTighterThanSection { piece: i, radius, reach });
                }
                // the axis lies in the section's plane (square to the tangent); a positive turn
                // moves the section toward the plane's normal, the path's direction
                let along = plane.to_local(a + axis);
                bodies.extend(revolve_whole(&plane, std::slice::from_ref(&region), centre, along, angle.abs())?);
                let turn = affine::rotation([0.0; 3], axis.into(), angle)?;
                let end = affine::apply_point(&affine::rotation(center.into(), axis.into(), angle)?, a.into());
                before = Some((end.into(), affine::apply_point(&turn, t.into()).into(), affine::apply_point(&turn, n.into()).into()));
            }
            PathSeg::Curve { .. } | PathSeg::Bezier(_) => return Err(FloatRefusal::FreeCurve { piece: i }),
        }
    }
    // a closed path (a sketched rectangle) turns at its start too
    if let (Some((end, t, n)), Some((start, first_t))) = (before, first)
        && path.len() > 1
        && len(end - start) <= zone
        && let Corner::Turned { fill, out_of_plane, .. } = corner(&region, end, t, n, first_t, zone)?
    {
        bodies.extend(fill);
        if let Some(angle) = out_of_plane {
            notes.push(FloatNote::CornerOutOfPlane { joint: 0, angle });
        }
    }
    welds.push(united(&bodies)?);
    Ok(FloatWeld { welds, fill, notes })
}

/// The section's region filled as `options` asks.
fn fill_section(section: &FloatSection, options: &FillOptions) -> Result<ChannelFill> {
    let filled = match *section {
        FloatSection::Fillet { legs: [z1, z2], angle, flip_side, flip_up } => {
            if !(z1.is_finite() && z2.is_finite() && z1 > 0.0 && z2 > 0.0) {
                return Err(FloatRefusal::Invalid("fillet legs that are not finite and positive"));
            }
            if !(angle > 0.0 && angle < std::f64::consts::PI) {
                return Err(FloatRefusal::Invalid("an angle between the legs not between 0 and 180 degrees"));
            }
            let side = if flip_side { -1.0 } else { 1.0 };
            let lift = if flip_up { -1.0 } else { 1.0 };
            let root = Vec2::ZERO;
            let toe_1 = Vec2::new(side * z1, 0.0);
            let toe_2 = Vec2::new(side * z2 * angle.cos(), lift * z2 * angle.sin());
            // the second leg's wall on the left seen from the face (as the 26 Nov probe built it);
            // one flip mirrors the section, which swaps them
            let (left, right) = (vec![Seg2::Line { a: toe_2, b: root }], vec![Seg2::Line { a: toe_1, b: root }]);
            let (left, right) = if side * lift < 0.0 { (right, left) } else { (left, right) };
            fill_channel(&left, &right, None, options)?
        }
        FloatSection::Groove { walls: [ref left, ref right], depth_normal } => fill_channel(left, right, depth_normal, options)?,
    };
    filled.map_err(|refusal| FloatRefusal::Fill { why: refusal.why, fit: refusal.fit })
}

/// What happens to the frame at a joint.
enum Corner {
    /// The pieces run on in one direction.
    Straight,
    /// The path doubles back.
    UTurn,
    /// The frame turned: its new `up`, the bodies that close the outside of the bend, and the
    /// angle where the turn left the section's plane.
    Turned { up: Vec3, fill: Vec<Body>, out_of_plane: Option<f64> },
}

/// The corner at `at` from direction `t` (unit, with unit `up` square to it) to `next` (unit):
/// the frame turned about the line square to both, and the part of `region` that the turn moves
/// forward turned with it.
fn corner(region: &Region2, at: Vec3, t: Vec3, up: Vec3, next: Vec3, zone: f64) -> Result<Corner> {
    // directions within rounding of each other, or of each other's opposite
    if len(next - t) <= 1e-9 {
        return Ok(Corner::Straight);
    }
    if len(next + t) <= 1e-9 {
        return Ok(Corner::UTurn);
    }
    // frame_axes(t, next) gives x = y × t with y along next's part square to t: x runs along
    // next × t, so the turn's axis (along t × next) is -x
    let [x, _] = affine::frame_axes(t.into(), next.into())?;
    let axis = -Vec3::from(x);
    let angle = affine::signed_unit_angle(axis.into(), t.into(), next.into())?;
    let turned_up: Vec3 = affine::apply_point(&affine::rotation([0.0; 3], axis.into(), angle)?, up.into()).into();
    let plane = Plane { origin: at, x: left_of(t, up)?, y: up };
    // in the section's plane: the axis through the root, and the side the turn moves forward
    let a = plane.to_local(at + axis);
    let forward = Vec2::new(-a.y, a.x);
    let far = 10.0 * reach(region, a)?.abs().max(reach(region, forward)?.abs()).max(reach(region, Vec2::new(-forward.x, -forward.y))?.abs()) + 10.0;
    let half = Region2 {
        outer: Loop2::polygon(&[
            Vec2::new(-far * a.x, -far * a.y),
            Vec2::new(far * a.x, far * a.y),
            Vec2::new(far * (a.x + forward.x), far * (a.y + forward.y)),
            Vec2::new(far * (forward.x - a.x), far * (forward.y - a.y)),
        ]),
        holes: Vec::new(),
    };
    let ahead = planar_regions_boolean(std::slice::from_ref(region), &[half], BoolOp::Intersect, zone.clamp(1e-9, 1.0))?;
    let mut fill = Vec::new();
    for part in &ahead {
        fill.extend(revolve_whole(&plane, std::slice::from_ref(part), Vec2::ZERO, a, angle)?);
    }
    let in_plane = len(axis - up) <= 1e-9 || len(axis + up) <= 1e-9;
    Ok(Corner::Turned { up: turned_up, fill, out_of_plane: (!in_plane).then_some(angle) })
}

/// The direction an arc from `a` about `axis` through `center` runs at `a`, for a turn of `angle`.
fn arc_tangent(a: Vec3, center: Vec3, axis: Vec3, angle: f64) -> Result<Vec3> {
    if !(angle.is_finite() && angle != 0.0) {
        return Err(FloatRefusal::Invalid("an arc that turns by no angle"));
    }
    // frame_axes(axis, a - center) gives x = unit(a - center) × unit(axis): along -(axis × radius),
    // the direction of a positive turn
    let [x, _] = affine::frame_axes(axis.into(), (a - center).into())?;
    let x = Vec3::from(x);
    Ok(if angle > 0.0 { -x } else { x })
}

/// The unit `x` of a section's frame: `up × t`, left of `t` seen from `up`'s tip.
fn left_of(t: Vec3, up: Vec3) -> Result<Vec3> {
    // frame_axes(t, up) gives y = up (square to t) and x = y × t
    let [x, _] = affine::frame_axes(t.into(), up.into())?;
    Ok(x.into())
}

/// How far `region` reaches along the unit 2D direction `u` (an upper bound: an arc counts as its
/// whole circle).
fn reach(region: &Region2, u: Vec2) -> Result<f64> {
    let along = |p: Vec2| p.x * u.x + p.y * u.y;
    let mut most = f64::NEG_INFINITY;
    for seg in region.loops().flat_map(|l| &l.segs) {
        most = most.max(match *seg {
            Seg2::Line { a, b } => along(a).max(along(b)),
            Seg2::Arc { center, radius, .. } => along(center) + radius,
            _ => return Err(FloatRefusal::Invalid("a section bounded by curves other than lines and arcs")),
        });
    }
    Ok(most)
}

/// The bodies as one.
fn united(bodies: &[Body]) -> Result<Body> {
    match bodies {
        [] => Err(FloatRefusal::Invalid("a weld with no body")),
        [one] => Ok(one.clone()),
        _ => Ok(union_all(&bodies.iter().collect::<Vec<_>>())?),
    }
}

fn len(v: Vec3) -> f64 {
    (v.x * v.x + v.y * v.y + v.z * v.z).sqrt()
}

#[cfg(test)]
#[path = "weld_float_tests.rs"]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests;
