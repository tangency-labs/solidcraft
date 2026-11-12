//! Dogbone reliefs: the round bite a router bit cuts into an inside corner of a solid so that a
//! square part seats in it.
//!
//! Not upstream. A round bit of radius `r` cannot cut a sharp inside corner: it leaves a round of
//! radius `r` there, and a mating part with a square corner does not go in. A relief is a
//! cylinder of the bit's radius along a concave straight edge between two planar faces, placed so
//! that the bit reaches (or nearly reaches) the sharp corner. [`edge_relief`] decides, for one
//! such edge as the caller reads it from a solid, whether it is an inside corner and which
//! cylinder relieves it. For the caller's choice of edges, [`outward`] gives a planar face's
//! outward normal and [`Edge::lean`] how far an edge leans from the bit's axis.
//!
//! A corner is described from the void's side: `a` and `b` are the unit directions of its two
//! walls away from the edge, square to it, the void (where the bit runs) is the wedge between
//! them, of angle `θ` in (0, π), and the material is outside it. The styles:
//! - [`Style::Normal`]: the centre on the wedge's bisector, at `r` from the corner: the circle
//!   passes through the corner. Of all circles of radius `r` through the corner with their centre
//!   in the wedge, this one removes the least material (the area beyond the walls is
//!   `r² (π − θ − sin θ)`, smallest for the centre on the bisector).
//! - [`Style::Minimal`]: the centre on the bisector at `r + gap`: the bite stops `gap` short of
//!   the sharp corner, for a mating corner that is slightly broken. `gap = 0` is `Normal`.
//! - [`Style::TBone`]: the centre on one wall's line, at `r` from the corner: the circle passes
//!   through the corner and bites that wall. At θ ≥ 90° the bite stays in that wall (tangent to
//!   the other wall at θ = 90°), so the relief hides along one side; at a sharper corner it also
//!   bites the other wall.
//!
//! Every function checks its input and returns [`SolidError::InvalidInput`] for a number that is
//! not finite or out of range; nothing here panics on input.

use crate::eval::vec::VecExt as _;
use crate::eval::Surface;
use crate::math::{Point3, Vec3};
use crate::{Result, SolidError};

const OP: &str = "dogbone";

/// Largest radius or gap accepted (mm): far past any real bit or part.
const MAX_SIZE: f64 = 1.0e6;

/// Two unit directions closer to parallel than this (radians, or its sine) are in line: a turn
/// this small is no corner.
const STRAIGHT: f64 = 1.0e-9;

/// Angular slack (radians) on the largest corner angle relieved, so a corner computed at 90°
/// plus rounding is still a 90° corner.
const ANGLE_SLACK: f64 = 1.0e-6;

/// Relative to the radius, the shortest stretch of a wall a circle must cut to bite it. A chord
/// is `2 r sin φ` long, `φ` the angle at which the circle crosses the wall's line, so a shorter
/// one crosses within about `STRAIGHT / 2` of tangent: the circle runs in line with the wall
/// there, as directions within [`STRAIGHT`] are in line, and touches it without biting (a
/// T-bone's other wall at 90°, a minimal circle that just reaches a wall).
const MIN_BITE: f64 = STRAIGHT;

/// Which wall a T-bone's bite is cut into.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Side {
    /// The longer of the two walls (the first, `a`, when they are equally long).
    #[default]
    Longer,
    /// The shorter of the two walls (the second, `b`, when they are equally long).
    Shorter,
}

/// Where the relief circle goes in its corner.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Style {
    /// On the bisector, through the corner.
    Normal,
    /// On the bisector, stopping `gap` short of the corner (`gap >= 0`).
    Minimal {
        /// How far short of the sharp corner the circle stops (mm).
        gap: f64,
    },
    /// On one wall's line, through the corner.
    TBone {
        /// Which wall the bite goes into.
        side: Side,
    },
}

/// What a relief is cut with and which corners get one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The bit's radius (mm), positive.
    pub radius: f64,
    /// Where the circle goes.
    pub style: Style,
    /// The largest corner (void wedge) angle relieved, radians in (0, π]: a corner more open than
    /// this (plus a microradian of slack for rounding) is skipped. π/2 relieves square and
    /// sharper corners.
    pub max_angle: f64,
}

impl Options {
    /// The options checked: a radius more than 0 and at most 10⁶ mm, a gap from 0 to 10⁶ mm, an
    /// angle in (0, π].
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] naming the value out of range.
    pub fn checked(self) -> Result<Options> {
        if !(self.radius.is_finite() && self.radius > 0.0 && self.radius <= MAX_SIZE) {
            return Err(invalid(
                "the bit's radius must be more than 0 and at most 10⁶ mm",
            ));
        }
        if let Style::Minimal { gap } = self.style {
            if !(gap.is_finite() && (0.0..=MAX_SIZE).contains(&gap)) {
                return Err(invalid(
                    "the minimal style's gap must be at least 0 and at most 10⁶ mm",
                ));
            }
        }
        if !(self.max_angle.is_finite()
            && self.max_angle > 0.0
            && self.max_angle <= std::f64::consts::PI)
        {
            return Err(invalid("the largest corner angle must be in (0°, 180°]"));
        }
        Ok(self)
    }
}

/// Why a corner gets no relief.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Skip {
    /// The corner is convex (an outside corner): a round bit cuts it sharp.
    Convex,
    /// The corner is more open than the largest angle relieved.
    TooOpen,
    /// A minimal relief whose gap is so large that the circle reaches neither wall.
    Unreached,
}

/// A relief in a corner, in the corner's own terms: its circle's centre as an offset from the
/// corner point.
#[derive(Clone, Copy, Debug, PartialEq)]
struct Wedge {
    /// The circle's centre less the corner point.
    centre: Vec3,
}

/// The relief of the inside corner whose walls leave it in unit directions `a` and `b`, the void
/// the wedge between them, walls `la` and `lb` long (only a T-bone reads them, to choose its
/// side). The walls are those [`edge_corner`] gives a concave corner: neither in line nor folded
/// back onto each other.
///
/// # Errors
/// [`SolidError::InvalidInput`] for options out of range, a direction or length that is not
/// finite, or walls in line.
fn wedge_relief(
    a: Vec3,
    b: Vec3,
    la: f64,
    lb: f64,
    options: &Options,
) -> Result<std::result::Result<Wedge, Skip>> {
    let o = options.checked()?;
    if !(a.is_finite() && b.is_finite() && la.is_finite() && lb.is_finite()) {
        return Err(invalid("a corner's walls must be finite"));
    }
    let angle = a.angle_between(b)?;
    if angle > o.max_angle + ANGLE_SLACK {
        return Ok(Err(Skip::TooOpen));
    }
    let r = o.radius;
    // a + b has length 2 cos(θ/2) > 0: the walls of a concave corner are not in line.
    let bisector = (a + b)
        .unitize()
        .map_err(|_| invalid("a corner's walls in line"))?;
    let centre = match o.style {
        Style::Normal => bisector * r,
        Style::Minimal { gap } => bisector * (r + gap),
        Style::TBone { side } => {
            let a_first = match side {
                Side::Longer => la >= lb,
                Side::Shorter => la < lb,
            };
            (if a_first { a } else { b }) * r
        }
    };
    if !bites(centre, a, r) && !bites(centre, b, r) {
        return Ok(Err(Skip::Unreached));
    }
    Ok(Ok(Wedge { centre }))
}

/// Whether the circle of radius `r` about `centre` (relative to the corner) bites the wall that
/// leaves the corner along unit `d`: the stretch of that ray inside the circle, its start clamped
/// to the corner, is longer than `r` times [`MIN_BITE`].
fn bites(centre: Vec3, d: Vec3, r: f64) -> bool {
    // |s d - c|² = r²: s² - 2 s (c·d) + |c|² - r² = 0.
    let h = centre.dot(d);
    let disc = h * h - (centre.length_squared() - r * r);
    if disc < 0.0 {
        return false;
    }
    let root = disc.sqrt();
    let far = h + root;
    let near = (h - root).max(0.0);
    far > near + r * MIN_BITE
}

/// A straight edge of a solid, by its two ends.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Edge {
    /// The start point.
    pub start: Point3,
    /// The end point.
    pub end: Point3,
}

impl Edge {
    /// The point a relief or a skipped edge is reported by: the middle of the edge's ends.
    #[must_use]
    pub fn middle(&self) -> Point3 {
        (self.start + self.end) * 0.5
    }

    /// How far the edge leans from the line of `axis` (radians, in [0, π/2]): 0 when it runs
    /// along `axis`, either way.
    ///
    /// # Errors
    /// [`SolidError::InvalidInput`] for a point or an axis that is not finite, a zero-length edge
    /// or a zero axis.
    pub fn lean(&self, axis: Vec3) -> Result<f64> {
        let along = self.end - self.start;
        if !(along.is_finite() && axis.is_finite()) {
            return Err(invalid("an edge's points and the axis must be finite"));
        }
        let (Ok(d), Ok(a)) = (along.unitize(), axis.unitize()) else {
            return Err(invalid("a vector of zero length"));
        };
        let off = d.angle_between(a)?;
        Ok(off.min(std::f64::consts::PI - off))
    }
}

/// The outward unit normal of a planar face: its plane's natural normal (`S_u × S_v`), reversed
/// where the solid's shell uses the face against it (`forward` false). `None` where `surface` is
/// not a plane, or its axes are not finite or lie in line.
#[must_use]
pub fn outward(surface: &Surface, forward: bool) -> Option<Vec3> {
    let (du, dv) = match surface {
        Surface::Plane { u_axis, v_axis, .. } => (Vec3::from(*u_axis), Vec3::from(*v_axis)),
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = Vec3::from(*origin);
            (Vec3::from(*u_point) - o, Vec3::from(*v_point) - o)
        }
        _ => return None,
    };
    let natural = du.cross(dv);
    if !natural.is_finite() {
        return None;
    }
    let n = natural.unitize().ok()?;
    Some(if forward { n } else { -n })
}

/// A planar face along a straight edge, one wall of the corner there, as [`edge_relief`] reads
/// it.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Wall<'a> {
    /// The face's plane's normal pointing out of the material, as [`outward`] gives it (need not
    /// be unit).
    pub outward: Vec3,
    /// Whether the face lies on the left of the edge's direction (start to end) seen from the
    /// outward side: the face's boundary runs along the edge from start to end, counted with the
    /// face's own sense. For a face whose loops run counter-clockwise seen from its surface's
    /// natural normal, that is `loop runs start→end` == `the face's shell uses the natural
    /// normal`.
    pub left: bool,
    /// The face's other straight edges that meet the edge at an end (a pocket wall's edges on
    /// the floor and at the top): the wall is as long as the longest, 0 with none. Only a T-bone
    /// reads it, to choose its side.
    pub sides: &'a [Edge],
}

/// The cylinder a relief along a straight inside edge is cut with.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ReliefCylinder {
    /// The centre of its base circle.
    pub base: Point3,
    /// Its unit axis, from the base.
    pub axis: Vec3,
    /// Its radius: the bit's.
    pub radius: f64,
    /// Its length along the axis.
    pub height: f64,
}

/// What [`edge_relief`] decides for a straight edge between two planar faces.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EdgeRelief {
    /// An inside corner: the cylinder that relieves it.
    Relief(ReliefCylinder),
    /// The faces continue each other: no corner.
    Smooth,
    /// A corner that gets no relief, and why.
    Skipped(Skip),
}

/// The relief of the straight edge `edge` between the planar faces `walls`, or why none. The
/// cylinder runs the edge's length, and on past an end by the bit's radius where that end is
/// open: `ends[0]` (at the start) and `ends[1]` (at the end) are the outward normals of the other
/// faces at that end (`None` for one that is not planar), and an end is open when it has such
/// faces and every one is planar and faces on the way the edge runs past it (the top of a pocket,
/// the faces a slot runs through). A pocket's floor faces back along the edge (blind), so the
/// relief cuts nothing below it.
///
/// # Errors
/// [`SolidError::InvalidInput`] for options out of range, a point or a normal that is not
/// finite, a zero-length edge, a zero normal or one along the edge.
pub fn edge_relief(
    edge: &Edge,
    walls: &[Wall<'_>; 2],
    ends: [&[Option<Vec3>]; 2],
    options: &Options,
) -> Result<EdgeRelief> {
    let o = options.checked()?;
    let directions = match edge_corner(edge, walls)? {
        EdgeCorner::Convex => return Ok(EdgeRelief::Skipped(Skip::Convex)),
        EdgeCorner::Smooth => return Ok(EdgeRelief::Smooth),
        EdgeCorner::Concave { walls } => walls,
    };
    let [wa, wb] = walls;
    let lengths = [wall_length(wa.sides)?, wall_length(wb.sides)?];
    let [at_start, at_end] = ends;
    let extend = [
        extension(edge.end, edge.start, at_start, o.radius)?,
        extension(edge.start, edge.end, at_end, o.radius)?,
    ];
    Ok(
        match relief_cylinder(edge, directions, lengths, extend, &o)? {
            Ok(cylinder) => EdgeRelief::Relief(cylinder),
            Err(skip) => EdgeRelief::Skipped(skip),
        },
    )
}

/// What a straight edge between two planar faces is.
#[derive(Clone, Copy, Debug, PartialEq)]
enum EdgeCorner {
    /// An outside edge: the material's angle there is less than 180°.
    Convex,
    /// The faces continue each other: no corner.
    Smooth,
    /// An inside edge: the walls' directions away from it (unit, square to the edge, along each
    /// face, in the order the faces were given).
    Concave {
        /// The walls' directions.
        walls: [Vec3; 2],
    },
}

/// Whether `edge` between `walls` is an inside (concave) corner, and if so, its walls'
/// directions.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or normal that is not finite, a zero-length edge, a
/// zero normal or one along the edge.
fn edge_corner(edge: &Edge, walls: &[Wall<'_>; 2]) -> Result<EdgeCorner> {
    let along = edge.end - edge.start;
    if !(along.is_finite() && walls.iter().all(|w| w.outward.is_finite())) {
        return Err(invalid(
            "an edge's points and its faces' normals must be finite",
        ));
    }
    let t = along
        .unitize()
        .map_err(|_| invalid("an edge of zero length"))?;
    let [fa, fb] = walls;
    let (na, nb) = (
        fa.outward
            .unitize()
            .map_err(|_| invalid("a face normal of zero length"))?,
        fb.outward
            .unitize()
            .map_err(|_| invalid("a face normal of zero length"))?,
    );
    // Into each face, square to the edge: n × t on the face's left of t, the other way on its right.
    let wall = |n: Vec3, left: bool| {
        let w = n.cross(t);
        if left {
            w
        } else {
            -w
        }
    };
    let (Ok(wa), Ok(wb)) = (wall(na, fa.left).unitize(), wall(nb, fb.left).unitize()) else {
        return Err(invalid("a face normal along its edge"));
    };
    // Going along face a away from the edge: in front of face b's plane (the void's side) at an
    // inside edge, behind it (in the material) at an outside one.
    let side = wa.dot(nb);
    if side.abs() <= STRAIGHT {
        return Ok(EdgeCorner::Smooth);
    }
    if side < 0.0 {
        return Ok(EdgeCorner::Convex);
    }
    Ok(EdgeCorner::Concave { walls: [wa, wb] })
}

/// How long a wall is beside an edge: the longest of its `sides`, 0 with none.
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point that is not finite.
fn wall_length(sides: &[Edge]) -> Result<f64> {
    let mut longest = 0.0;
    for side in sides {
        if !(side.start.is_finite() && side.end.is_finite()) {
            return Err(invalid("a wall's edges must be finite"));
        }
        let l = side.start.distance(side.end);
        if l > longest {
            longest = l;
        }
    }
    Ok(longest)
}

/// How far a relief runs past the end `here` of an edge whose other end is `there`, `faces` the
/// outward normals of the faces at `here` other than the walls (`None` for one that is not
/// planar): `radius` when there is one and every one faces on along the edge (open), else 0
/// (blind, such as a pocket's floor).
///
/// # Errors
/// As [`end_is_open`].
fn extension(there: Point3, here: Point3, faces: &[Option<Vec3>], radius: f64) -> Result<f64> {
    let mut any = false;
    for outward in faces {
        let Some(outward) = outward else {
            return Ok(0.0);
        };
        if !end_is_open(there, here, *outward)? {
            return Ok(0.0);
        }
        any = true;
    }
    Ok(if any { radius } else { 0.0 })
}

/// Whether the end `here` of a straight edge (its other end `there`) is open: the face there,
/// with outward normal `outward`, faces on the way the edge runs past `here`, so a relief may run
/// on through empty space; a pocket's floor faces back along the edge (blind).
///
/// # Errors
/// [`SolidError::InvalidInput`] for a point or vector that is not finite, a zero-length edge or
/// a zero normal.
fn end_is_open(there: Point3, here: Point3, outward: Vec3) -> Result<bool> {
    let along = here - there;
    if !(along.is_finite() && outward.is_finite()) {
        return Err(invalid("points and normals must be finite"));
    }
    let (Ok(d), Ok(n)) = (along.unitize(), outward.unitize()) else {
        return Err(invalid("a vector of zero length"));
    };
    Ok(d.dot(n) > STRAIGHT)
}

/// The relief cylinder of the inside `edge` with walls in directions `walls` (from
/// [`edge_corner`]), `lengths` long (a T-bone's side), running the edge's length plus `extend[0]`
/// past its start and `extend[1]` past its end (0 at a blind end); or why none.
///
/// # Errors
/// [`SolidError::InvalidInput`] for options or an extension out of range, a point that is not
/// finite or a zero-length edge.
fn relief_cylinder(
    edge: &Edge,
    walls: [Vec3; 2],
    lengths: [f64; 2],
    extend: [f64; 2],
    options: &Options,
) -> Result<std::result::Result<ReliefCylinder, Skip>> {
    let o = options.checked()?;
    let extensions = extend
        .iter()
        .all(|e| e.is_finite() && (0.0..=MAX_SIZE).contains(e));
    let along = edge.end - edge.start;
    if !(edge.start.is_finite() && along.is_finite() && extensions) {
        return Err(invalid(
            "an edge's points and extensions must be finite (extensions at least 0)",
        ));
    }
    let l = along.length();
    let axis = along
        .unitize()
        .map_err(|_| invalid("an edge of zero length"))?;
    let [wa, wb] = walls;
    let [la, lb] = lengths;
    let w = match wedge_relief(wa, wb, la, lb, &o)? {
        Ok(w) => w,
        Err(skip) => return Ok(Err(skip)),
    };
    let [before, after] = extend;
    Ok(Ok(ReliefCylinder {
        base: edge.start + w.centre - axis * before,
        axis,
        radius: o.radius,
        height: l + before + after,
    }))
}

fn invalid(reason: &'static str) -> SolidError {
    SolidError::InvalidInput {
        operation: OP,
        reason,
    }
}

#[cfg(test)]
#[path = "dogbone_tests.rs"]
mod tests;
