//! The bores of a body — the round holes and obround slots a fastener can pass through — and
//! the stacks of bores that line up across touching bodies, for placing rivets, bolts and pins.
//!
//! Every answer comes from the record's own faces ([`query::Record`]): an inward cylindrical
//! wall is a `Cylinder` face its shell reads against its natural normal, whose material side
//! then lies outside the cylinder; a wall that closes round the axis has a seam, an edge its
//! own loop uses twice, while a partial wall's sides are edges that run along the axis; two
//! partial walls of one radius and extent whose openings face each other across a gap are the
//! ends of an obround slot; a countersink or a chamfer that opens out of a wall's end is a
//! coaxial `Cone` face whose narrow rim is the wall's own circle, and the wall's extent runs on
//! through it. Axes, radii and rims come from the faces' analytic data, extents from the faces'
//! `v` domains held to the `v` their loops' pcurves' control points bound (the height along the
//! axis, the analytic surfaces' own parameter), the rest from
//! the boundary edges' evaluated ends and middles. Nothing here meshes the body, and no `u`
//! parameter or edge interval value is read for its own meaning: an interval only says where
//! an edge's points are.
//!
//! Within one body the slot ends are paired by comparing each pair (bounded by
//! [`MAX_FACES`]). Across bodies the round holes join through buckets of their lines' feet
//! (each line's nearest point to the origin, taken with its own holes' axis: the axes one
//! feature produces agree exactly, so such lines' feet land in the same or a neighbouring
//! bucket; axes that differ by more than rounding can, far along them, land further apart, and
//! such a pair is at the edge of the tolerance anyway), so a design of many parallel holes
//! compares only neighbours; every comparison the stacks pass makes counts against
//! [`MAX_CROSSES`], and past it the call refuses rather than go quiet for a long time.
//!
//! Not upstream: SolidCraft's hole stacks (tangency-labs/solidcraft#2073, MIT OR Apache-2.0),
//! reworked from the display tessellation onto the exact record.
//!
//! Deviations: none (no upstream counterpart). No recursion.

use std::collections::BTreeMap;

use super::curve::CurveEval;
use super::geometry::{Curve, Curve2, Frame, Surface};
use super::query::{Face, Loop, Record};
use super::vec::{v3, VecExt};
use crate::error::{Result, SolidError};
use crate::math::Vec3;

/// What [`stacks`] names in its refusals.
const OP: &str = "hole_stacks";
/// Most faces one call reads; a larger body answers no bores rather than a slow scan.
const MAX_FACES: usize = 20_000;
/// Below this, in mm, an edge's ends share a place round the axis (a side) or a height (a
/// rim): the record's analytic pieces meet exactly, and this only allows for rounding.
const NEAR: f64 = 1e-6;
/// A cone joins a wall when their axes are this close, in mm: SMLib places a cone's frame a
/// hair off its hole's, and a countersink is one feature with the hole it opens.
const COAXIAL: f64 = 1e-3;
/// How far a slot end's two sides may sit from one diameter, in radians: an obround end is cut
/// by one plane through the axis, so they sit on one exactly.
const SLOT_END: f64 = 0.6;
/// Most comparisons the stacks pass makes before it refuses: past them there are too many
/// nearly-coaxial holes for one answer, and the caller narrows with names or diameter ranges.
const MAX_CROSSES: u64 = 4_000_000;
/// Two axes are parallel when their unit vectors' dot is this close to one either way round.
const PARALLEL: f64 = 1.0 - 1e-5;

/// One bore of one body: a round hole, or an obround slot (`slot`).
#[derive(Clone, Debug, PartialEq)]
pub struct Bore {
    /// A point on the bore's axis: a round hole's own axis; a slot's, halfway between its ends.
    pub point: Vec3,
    /// The unit axis, either way round.
    pub axis: Vec3,
    /// The bore's diameter.
    pub d: f64,
    /// The wall's extent along `axis`, measured from `point`.
    pub t: [f64; 2],
    /// The slot's shape across the axis.
    pub slot: Option<Slot>,
}

/// An obround slot's shape across the axis.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Slot {
    /// The overall length, end to end.
    pub length: f64,
    /// The unit direction of the length.
    pub along: Vec3,
}

/// What shapes the stacks: the largest gap between layers, and the largest offset between hole
/// axes, in mm.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Options {
    /// The largest gap between layers of one stack, in mm.
    pub touching: f64,
    /// The largest offset between the axes of one line of holes, in mm.
    pub axis_tolerance: f64,
}

/// One layer of a stack: the bores of one body that run together.
#[derive(Clone, Debug, PartialEq)]
pub struct Layer {
    /// The body, by its index into the `bodies` [`stacks`] took.
    pub body: usize,
    /// The layer's narrowest bore, by its index into that body's bores.
    pub bore: usize,
    /// The layer's narrowest diameter.
    pub d: f64,
    /// The extent from the stack's start.
    pub t: [f64; 2],
    /// The narrowest bore's slot shape.
    pub slot: Option<Slot>,
}

/// Bores that line up through two or more touching bodies.
#[derive(Clone, Debug, PartialEq)]
pub struct Stack {
    /// Unit, from the start into the stack.
    pub axis: Vec3,
    /// The point on the axis the first layer starts at.
    pub start: Vec3,
    /// The clamped thickness.
    pub grip: f64,
    /// The point on the axis the last layer ends at.
    pub end: Vec3,
    /// The stack's layers, from its start along `axis`.
    pub layers: Vec<Layer>,
}

/// The stacks among bodies' bores, and the bores in none, as `(body, bore)` indices.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Stacks {
    /// The stacks, ordered by their start points.
    pub stacks: Vec<Stack>,
    /// The bores in no stack, oriented the stack's way round.
    pub lone: Vec<Lone>,
}

/// A bore in no stack, oriented the stack's way round: the largest component of `axis`
/// negative, `at` its entry point and `depth` its wall's extent.
#[derive(Clone, Debug, PartialEq)]
pub struct Lone {
    /// The body, by its index into `bodies`.
    pub body: usize,
    /// The bore's entry point.
    pub at: Vec3,
    /// Unit axis, from the entry point into the bore.
    pub axis: Vec3,
    /// The bore's diameter, in mm.
    pub d: f64,
    /// The wall's extent, in mm.
    pub depth: f64,
    /// The slot's shape.
    pub slot: Option<Slot>,
}

/// One cylindrical wall of one face: a full turn (a round hole) or a partial one.
struct Wall {
    /// A point on the axis.
    point: Vec3,
    /// Unit axis.
    axis: Vec3,
    radius: f64,
    /// The extent along `axis` from `point`.
    t: [f64; 2],
    /// `None` when the wall closes round the axis; else the unit direction from the axis to
    /// the middle of its opening.
    opening: Option<Vec3>,
}

/// One conical face that may extend a wall: a countersink or a chamfer at a hole's end.
struct Cone {
    point: Vec3,
    axis: Vec3,
    /// The radius at `point`.
    radius_at_origin: f64,
    half_angle: f64,
    /// The extent along `axis` from `point`.
    t: [f64; 2],
}

/// One line of holes: its axis and a point on it, and the bores that sit on it.
struct Line {
    point: Vec3,
    axis: Vec3,
    members: Vec<usize>,
}

/// An edge's two ends and its middle. `None` where the curve cannot be read or a point is not
/// finite.
fn edge_points(curve: Option<&Curve>, interval: [f64; 2]) -> Option<([Vec3; 2], Vec3)> {
    let eval = CurveEval::new(curve?).ok()?;
    let a = eval.point(interval[0])?;
    let b = eval.point(interval[1])?;
    let mid = eval.point(interval[0] / 2.0 + interval[1] / 2.0)?;
    (a.is_finite() && b.is_finite() && mid.is_finite()).then_some(([a, b], mid))
}

/// Whether a face's material lies outside its surface: its shell then reads it against the
/// natural normal, which points away from the axis of a cylinder or cone.
fn material_outside(forward: Option<bool>) -> bool {
    forward == Some(false)
}

/// The bores of one record: every round hole, and every obround slot, with its wall's extent.
/// A countersink or chamfer at a hole's end counts towards its extent. Walls that close only
/// part of the way round and are not a slot's end are left out: a fastener cannot follow them.
/// A body with more than [`MAX_FACES`] faces is refused: scanning it would cost more than
/// the answer is worth, and answering no bores would read as a body without holes.
///
/// # Errors
/// [`SolidError::InvalidInput`] with `operation` [`OP`] when the record holds more than
/// [`MAX_FACES`] faces.
pub fn bores(record: &Record) -> Result<Vec<Bore>, SolidError> {
    if record.faces.len() > MAX_FACES {
        return Err(SolidError::InvalidInput {
            operation: OP,
            reason: "a body of more than 20000 faces",
        });
    }
    let edges: Vec<Option<([Vec3; 2], Vec3)>> = record
        .edges
        .iter()
        .map(|e| edge_points(e.curve.as_ref(), e.interval))
        .collect();
    let mut walls = Vec::new();
    let mut cones = Vec::new();
    for f in &record.faces {
        match f.surface.as_ref() {
            Some(Surface::Cylinder { frame, radius }) if material_outside(f.forward) => {
                if let Some(w) = wall(f, frame, *radius, &edges) {
                    walls.push(w);
                }
            }
            Some(Surface::Cone {
                frame,
                radius_at_origin,
                half_angle,
            }) if material_outside(f.forward)
                && radius_at_origin.is_finite()
                && half_angle.is_finite() =>
            {
                if let Some((point, axis)) = frame_axis(frame) {
                    if let Some(t) = frame_extent(f) {
                        cones.push(Cone {
                            point,
                            axis,
                            radius_at_origin: *radius_at_origin,
                            half_angle: *half_angle,
                            t,
                        });
                    }
                }
            }
            _ => {}
        }
    }
    through_cones(&mut walls, &cones);
    let mut out = Vec::new();
    let mut used = vec![false; walls.len()];
    // Pieces of one cylinder: the sheet model cuts a hole's wall as two half faces, each with
    // two sides on one diameter and no seam. Two halves of one cylinder — the same axis line,
    // the same radius, the same extent — close into one round hole before slots pair, so
    // halves of two different holes cannot pair as a slot across the gap between them.
    for (i, a) in walls.iter().enumerate() {
        let Some(oa) = a.opening else { continue };
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        for (j, b) in walls.iter().enumerate().skip(i + 1) {
            if used.get(i).copied().unwrap_or(true) || used.get(j).copied().unwrap_or(true) {
                continue;
            }
            let Some(ob) = b.opening else { continue };
            if !one_cylinder(a, b) || oa.dot(ob) > -0.9 {
                continue;
            }
            if let Some(x) = used.get_mut(i) {
                *x = true;
            }
            if let Some(y) = used.get_mut(j) {
                *y = true;
            }
            out.push(Bore {
                point: a.point,
                axis: a.axis,
                d: 2.0 * a.radius,
                t: a.t,
                slot: None,
            });
        }
    }
    for (i, a) in walls.iter().enumerate() {
        let Some(oa) = a.opening else { continue };
        if used.get(i).copied().unwrap_or(true) {
            continue;
        }
        for (j, b) in walls.iter().enumerate().skip(i + 1) {
            if used.get(i).copied().unwrap_or(true) || used.get(j).copied().unwrap_or(true) {
                continue;
            }
            let Some(ob) = b.opening else { continue };
            let Some((point, axis, t, slot)) = slot_of(a, oa, b, ob) else {
                continue;
            };
            if let Some(x) = used.get_mut(i) {
                *x = true;
            }
            if let Some(y) = used.get_mut(j) {
                *y = true;
            }
            out.push(Bore {
                point,
                axis,
                d: 2.0 * a.radius,
                t,
                slot: Some(slot),
            });
        }
    }
    for (i, w) in walls.iter().enumerate() {
        if w.opening.is_some() || used.get(i).copied().unwrap_or(true) {
            continue; // an unpaired partial wall is not a bore
        }
        out.push(Bore {
            point: w.point,
            axis: w.axis,
            d: 2.0 * w.radius,
            t: w.t,
            slot: None,
        });
    }
    Ok(out)
}

/// The wall of one cylinder face: `None` when it does not close round the axis and is not a
/// slot's end (two sides on one diameter, with an opening between them).
fn wall(f: &Face, frame: &Frame, radius: f64, edges: &[Option<([Vec3; 2], Vec3)>]) -> Option<Wall> {
    if !radius.is_finite() || radius <= 0.0 {
        return None;
    }
    let (point, axis) = frame_axis(frame)?;
    let t = frame_extent(f)?;
    let x = v3(frame.x_axis);
    let y = v3(frame.y_axis);
    // The edges of the face's loops, each with how many times this face uses it.
    let mut uses: BTreeMap<usize, usize> = BTreeMap::new();
    for l in &f.loops {
        let Loop::Edges(edges) = l else { continue };
        for u in edges {
            let n = uses.entry(u.edge).or_insert(0);
            *n += 1;
        }
    }
    let seam = uses.values().any(|n| *n >= 2);
    let mut sides = Vec::new();
    let mut rim = None;
    for (e, n) in &uses {
        let ([a, b], mid) = edges.get(*e).and_then(|x| x.as_ref())?;
        let along = (*b - *a).dot(axis);
        let perp = (*b - *a) - axis * along;
        if *n >= 2 {
            continue; // the seam, in its two uses
        }
        if perp.len() <= NEAR {
            if along.abs() > NEAR {
                sides.push(*a); // an edge along the axis: its end carries its angle
            }
        } else if rim.is_none() {
            // An edge around the axis, between the sides: its middle lies in the covered arc.
            rim = Some(*mid);
        }
    }
    if seam {
        return Some(Wall {
            point,
            axis,
            radius,
            t,
            opening: None,
        });
    }
    // The two sides on one diameter: the end of an obround slot.
    let angle = |p: Vec3| {
        let d = p - point;
        d.dot(y).atan2(d.dot(x))
    };
    let (Some(first), Some(second)) = (sides.first(), sides.get(1)) else {
        return None;
    };
    let (a, b) = (angle(*first), angle(*second));
    let spread = (a - b).abs();
    let delta = spread.min(std::f64::consts::TAU - spread);
    if (delta - std::f64::consts::PI).abs() > SLOT_END {
        return None;
    }
    let mid = rim?;
    let radial = mid - point - axis * (mid - point).dot(axis);
    let radial = radial.normalized()?;
    Some(Wall {
        point,
        axis,
        radius,
        t,
        opening: Some(-radial),
    })
}

/// The frame's axis: its origin, on the axis, and its unit z.
fn frame_axis(frame: &Frame) -> Option<(Vec3, Vec3)> {
    let axis = v3(frame.x_axis).cross(v3(frame.y_axis)).normalized()?;
    let point = v3(frame.origin);
    (point.is_finite() && axis.is_finite()).then_some((point, axis))
}

/// The face's extent along its surface's axis: the `v` span of its loops. The `v` domain is a
/// rectangle around the loops, and a body rebuilt later in a design's timeline can hold it
/// looser than the face's own boundary, so the loops' pcurves' control points bound it too
/// (a spline or rational spline lies within its control points' `v`, held inside the domain):
/// the tighter of the two holds. A face with no loops takes its domain.
fn frame_extent(f: &Face) -> Option<[f64; 2]> {
    let (d0, d1) = (f.domain[1][0], f.domain[1][1]);
    if !(d0.is_finite() && d1.is_finite() && d0 < d1) {
        return None;
    }
    let mut lo = f64::INFINITY;
    let mut hi = f64::NEG_INFINITY;
    // A control point the hull cannot take (not finite, or a weight the convex-hull property
    // needs positive) leaves the loops' bound to the domain alone.
    let mut whole = true;
    for l in &f.loops {
        let Loop::Edges(edges) = l else { continue };
        for u in edges {
            let vs: Vec<Option<f64>> = match &u.pcurve {
                Curve2::Spline { points, .. } => points.iter().map(|p| Some(p[1])).collect(),
                Curve2::RationalSpline { homogeneous, .. } => homogeneous
                    .iter()
                    .map(|h| (h[2].is_finite() && h[2] > 0.0).then(|| h[1] / h[2]))
                    .collect(),
            };
            for v in vs {
                let v = match v {
                    Some(v) if v.is_finite() => v,
                    _ => {
                        whole = false;
                        continue;
                    }
                };
                let v = if v < d0 {
                    d0
                } else if v > d1 {
                    d1
                } else {
                    v
                };
                lo = lo.min(v);
                hi = hi.max(v);
            }
        }
    }
    // No loops at all, or one the hull cannot bound: the domain is all the face says about
    // its extent.
    let (lo, hi) = if whole && lo < hi {
        (d0.max(lo), d1.min(hi))
    } else {
        (d0, d1)
    };
    (lo < hi).then_some([lo, hi])
}

/// Whether two partial walls are two halves of one cylinder: one axis line, one radius, one
/// extent. (A slot whose ends sit closer than [`COAXIAL`] apart also passes, and closes into
/// a round hole: at that stride the slot is a hole anyway.)
fn one_cylinder(a: &Wall, b: &Wall) -> bool {
    if a.axis.dot(b.axis).abs() < PARALLEL || (a.radius - b.radius).abs() > NEAR {
        return false;
    }
    let gap = b.point - a.point;
    let along = gap.dot(a.axis);
    if (gap - a.axis * along).len() > COAXIAL {
        return false;
    }
    let (u0, u1) = if b.axis.dot(a.axis) >= 0.0 {
        (b.t[0], b.t[1])
    } else {
        (-b.t[1], -b.t[0])
    };
    (a.t[0] - (u0 + along)).abs() <= COAXIAL && (a.t[1] - (u1 + along)).abs() <= COAXIAL
}

/// The slot two facing partial walls make: its centre, axis, extent and shape. `None` when
/// they are not one slot.
fn slot_of(a: &Wall, oa: Vec3, b: &Wall, ob: Vec3) -> Option<(Vec3, Vec3, [f64; 2], Slot)> {
    if a.axis.dot(b.axis).abs() < PARALLEL || (a.radius - b.radius).abs() > NEAR {
        return None;
    }
    let gap = b.point - a.point;
    let along = gap.dot(a.axis);
    let across = gap - a.axis * along;
    let dir = across.normalized()?; // halves on one axis are not a slot
    let (u0, u1) = if b.axis.dot(a.axis) >= 0.0 {
        (b.t[0], b.t[1])
    } else {
        (-b.t[1], -b.t[0])
    };
    if (a.t[0] - (u0 + along)).abs() > COAXIAL || (a.t[1] - (u1 + along)).abs() > COAXIAL {
        return None;
    }
    if oa.dot(dir) < 0.9 || ob.dot(dir) > -0.9 {
        return None;
    }
    Some((
        a.point + across * 0.5,
        a.axis,
        a.t,
        Slot {
            length: across.len() + 2.0 * a.radius,
            along: dir,
        },
    ))
}

/// Walls through their cones: a countersink or a chamfer at a wall's end, whose narrow rim is
/// the wall's own circle, extends the wall's extent to the cone's far end.
fn through_cones(walls: &mut [Wall], cones: &[Cone]) {
    for w in walls.iter_mut() {
        for c in cones {
            if w.axis.dot(c.axis).abs() < PARALLEL {
                continue;
            }
            let off = c.point - w.point;
            let along = off.dot(w.axis);
            if (off - w.axis * along).len() > COAXIAL {
                continue;
            }
            let map = |x: f64| (c.point + c.axis * x - w.point).dot(w.axis);
            let (m0, m1) = (map(c.t[0]), map(c.t[1]));
            let (lo, hi) = (m0.min(m1), m0.max(m1));
            let radius_at = |t: f64| {
                let s = (w.point + w.axis * t - c.point).dot(c.axis);
                c.radius_at_origin + s * c.half_angle.tan()
            };
            let rim = NEAR.max(w.radius * 2e-3);
            // A countersink or chamfer opens out: the cone's far end is no narrower than its
            // rim at the wall. A cone that narrows away (a drill point) is the bore's own
            // closed end, not an opening.
            if (radius_at(w.t[0]) - w.radius).abs() <= rim
                && radius_at(lo) >= radius_at(w.t[0]) - rim
                && (hi - w.t[0]).abs() <= COAXIAL
                && lo < w.t[0]
            {
                w.t[0] = lo;
            } else if (radius_at(w.t[1]) - w.radius).abs() <= rim
                && radius_at(hi) >= radius_at(w.t[1]) - rim
                && (lo - w.t[1]).abs() <= COAXIAL
                && hi > w.t[1]
            {
                w.t[1] = hi;
            }
        }
    }
}

/// The stacks among bodies' bores, and the bores in none. Round holes whose axes sit within
/// `axis_tolerance` of one line form one line of holes; along it, runs of bores whose extents
/// sit within `touching` of each other are stacks, one layer per body's stretch (a counterbore and its
/// through hole are one layer, of their narrowest opening), and a stack needs two bodies. A
/// slot joins the line of any round hole that runs through it; slots that no round hole runs
/// through stack on the axis through the middle of the length they share.
///
/// # Errors
/// [`SolidError::InvalidInput`] when the pass makes more than [`MAX_CROSSES`] comparisons:
/// there are too many nearly-coaxial holes for one answer, and the caller narrows the call
/// with fewer bodies or diameter ranges.
pub fn stacks(bodies: &[&[Bore]], o: &Options) -> Result<Stacks> {
    let refs: Vec<(usize, usize)> = bodies
        .iter()
        .enumerate()
        .flat_map(|(b, bs)| (0..bs.len()).map(move |i| (b, i)))
        .collect();
    let bore = |r: usize| -> Option<&Bore> {
        refs.get(r)
            .and_then(|&(b, i)| bodies.get(b).and_then(|bs| bs.get(i)))
    };
    let mut crosses: u64 = 0;
    let mut parent: Vec<usize> = (0..refs.len()).collect();
    let (rounds, slots): (Vec<usize>, Vec<usize>) =
        (0..refs.len()).partition(|&i| bore(i).is_some_and(|b| b.slot.is_none()));
    // Round holes join through buckets of their lines' feet (each line's nearest point to the
    // origin): lines within `axis_tolerance` have feet in the same or a neighbouring bucket,
    // so only those are compared, exactly.
    let cell = o.axis_tolerance.max(1e-9);
    let mut cells: BTreeMap<[i64; 3], Vec<usize>> = BTreeMap::new();
    for &r in &rounds {
        if let Some(b) = bore(r) {
            if let Some(c) = foot(b, cell) {
                cells.entry(c).or_default().push(r);
            }
        }
    }
    for (here, members) in &cells {
        for dx in -1i64..=1 {
            for dy in -1i64..=1 {
                for dz in -1i64..=1 {
                    // A bore far enough out to saturate its bucket has no representable
                    // neighbour past the edge: checked adds skip that cell instead of
                    // overflowing.
                    let (Some(nx), Some(ny), Some(nz)) = (
                        here[0].checked_add(dx),
                        here[1].checked_add(dy),
                        here[2].checked_add(dz),
                    ) else {
                        continue;
                    };
                    let Some(others) = cells.get(&[nx, ny, nz]) else {
                        continue;
                    };
                    for &i in members {
                        for &j in others {
                            if i >= j {
                                continue; // each pair once, over both buckets
                            }
                            crosses += 1;
                            if crosses > MAX_CROSSES {
                                return Err(too_many());
                            }
                            let (Some(a), Some(b)) = (bore(i), bore(j)) else {
                                continue;
                            };
                            if parallel(a.axis, b.axis) && apart(a, b) <= o.axis_tolerance {
                                join(&mut parent, i, j);
                            }
                        }
                    }
                }
            }
        }
    }
    // Each line of round holes, and the slots its axis runs through.
    let mut threaded = vec![false; refs.len()];
    let mut lines: Vec<Line> = Vec::new();
    for members in groups(&mut parent, &rounds) {
        let Some(first) = members.first().and_then(|&i| bore(i)) else {
            continue;
        };
        let (p, axis) = (first.point, canonical(first.axis));
        let mut members = members;
        for &s in &slots {
            crosses += 1;
            if crosses > MAX_CROSSES {
                return Err(too_many());
            }
            if let Some(sl) = bore(s) {
                if parallel(sl.axis, axis) && offset(p, axis, sl) <= o.axis_tolerance {
                    // The slot joins the line only where a round bore could actually run
                    // with it: a bore far along the axis never reaches the slot's stretch,
                    // and a slot held by such a bore would lose the slots it should stack
                    // with.
                    let st = extent(sl, p, axis);
                    let reach = members.iter().any(|&m| {
                        bore(m).is_some_and(|b| {
                            let (m0, m1) = extent(b, p, axis);
                            m0 <= st.1 + o.touching && st.0 <= m1 + o.touching
                        })
                    });
                    if reach {
                        members.push(s);
                        if let Some(x) = threaded.get_mut(s) {
                            *x = true;
                        }
                    }
                }
            }
        }
        lines.push(Line {
            point: p,
            axis,
            members,
        });
    }
    // Slots that no round hole runs through, on one centre line: the axis through the middle
    // of the length they share.
    let free: Vec<usize> = slots
        .iter()
        .copied()
        .filter(|&s| !threaded.get(s).copied().unwrap_or(true))
        .collect();
    for (k, &i) in free.iter().enumerate() {
        for &j in free.iter().skip(k + 1) {
            crosses += 1;
            if crosses > MAX_CROSSES {
                return Err(too_many());
            }
            if let (Some(a), Some(b)) = (bore(i), bore(j)) {
                if same_slot_line(a, b, o.axis_tolerance) {
                    join(&mut parent, i, j);
                }
            }
        }
    }
    for members in groups(&mut parent, &free) {
        let Some(first) = members.first().and_then(|&i| bore(i)) else {
            continue;
        };
        let Some(sa) = first.slot else { continue };
        // A chain of slots can run past the end of where it began (slots centred at 0, 5,
        // 10 and 15 mm): every pair meets its neighbour, but no one axis runs through all.
        // Cut the chain into runs that share a centre interval, earliest first, and run
        // each run's axis through the middle of what it shares.
        let mut spans: Vec<(f64, f64, usize)> = members
            .iter()
            .filter_map(|&i| bore(i).map(|h| (i, h)))
            .filter_map(|(i, h)| {
                let s = h.slot?;
                let c = (h.point - first.point).dot(sa.along);
                let half = ((s.length - h.d) / 2.0).max(0.0);
                Some((c, half, i))
            })
            .collect();
        spans.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.2.cmp(&b.2)));
        let mut run: Vec<usize> = Vec::new();
        let mut lo = f64::NEG_INFINITY;
        let mut hi = f64::INFINITY;
        for (c, half, i) in spans {
            if lo.max(c - half) > hi.min(c + half) && !run.is_empty() {
                let mid = lo / 2.0 + hi / 2.0;
                lines.push(Line {
                    point: first.point + sa.along * mid,
                    axis: canonical(first.axis),
                    members: std::mem::take(&mut run),
                });
                lo = f64::NEG_INFINITY;
                hi = f64::INFINITY;
            }
            run.push(i);
            lo = lo.max(c - half);
            hi = hi.min(c + half);
        }
        if !run.is_empty() {
            let mid = lo / 2.0 + hi / 2.0;
            lines.push(Line {
                point: first.point + sa.along * mid,
                axis: canonical(first.axis),
                members: run,
            });
        }
    }
    // Along each line, runs of extents that sit within `touching`, one layer per body's
    // stretch: a clevis around its lug gives two layers of one body.
    let mut in_stack = vec![false; refs.len()];
    let mut out = Vec::new();
    for line in &lines {
        let mut pieces: Vec<(f64, f64, usize)> = Vec::new();
        for &m in &line.members {
            if let Some(h) = bore(m) {
                let (t0, t1) = extent(h, line.point, line.axis);
                pieces.push((t0, t1, m));
            }
        }
        pieces.sort_by(|a, b| a.0.total_cmp(&b.0).then(a.1.total_cmp(&b.1)));
        let mut runs: Vec<Vec<(f64, f64, usize)>> = Vec::new();
        let mut reach = f64::NEG_INFINITY;
        for piece in pieces {
            match runs.last_mut() {
                Some(run) if piece.0 <= reach + o.touching => {
                    run.push(piece);
                    reach = reach.max(piece.1);
                }
                _ => {
                    runs.push(vec![piece]);
                    reach = piece.1;
                }
            }
        }
        for run in runs {
            let mut layers: Vec<(Layer, Vec<usize>)> = Vec::new();
            for (t0, t1, m) in &run {
                let (Some(h), Some(&(body, bi))) = (bore(*m), refs.get(*m)) else {
                    continue;
                };
                let layer = Layer {
                    body,
                    bore: bi,
                    d: h.d,
                    t: [*t0, *t1],
                    slot: h.slot,
                };
                match layers.last_mut() {
                    Some((l, ids)) if l.body == body => {
                        l.t[0] = l.t[0].min(layer.t[0]);
                        l.t[1] = l.t[1].max(layer.t[1]);
                        if layer.d < l.d {
                            l.d = layer.d;
                            l.slot = layer.slot;
                            l.bore = layer.bore;
                        }
                        ids.push(*m);
                    }
                    _ => layers.push((layer, vec![*m])),
                }
            }
            let distinct: std::collections::BTreeSet<usize> =
                layers.iter().map(|(l, _)| l.body).collect();
            if distinct.len() < 2 {
                continue;
            }
            let t_start = layers
                .iter()
                .map(|(l, _)| l.t[0])
                .fold(f64::INFINITY, f64::min);
            let t_end = layers
                .iter()
                .map(|(l, _)| l.t[1])
                .fold(f64::NEG_INFINITY, f64::max);
            for (_, ids) in &layers {
                for i in ids {
                    if let Some(x) = in_stack.get_mut(*i) {
                        *x = true;
                    }
                }
            }
            let layers = layers
                .into_iter()
                .map(|(mut l, _)| {
                    l.t[0] -= t_start;
                    l.t[1] -= t_start;
                    l
                })
                .collect();
            out.push(Stack {
                axis: line.axis,
                start: line.point + line.axis * t_start,
                grip: t_end - t_start,
                end: line.point + line.axis * t_end,
                layers,
            });
        }
    }
    out.sort_by(|a, b| {
        a.start
            .x
            .total_cmp(&b.start.x)
            .then(a.start.y.total_cmp(&b.start.y))
            .then(a.start.z.total_cmp(&b.start.z))
    });
    let lone = (0..refs.len())
        .filter(|i| !in_stack.get(*i).copied().unwrap_or(true))
        .filter_map(|i| {
            let (body, bore) = refs.get(i).copied()?;
            let h = bodies.get(body)?.get(bore)?;
            // The stack's way round: the largest component negative, so a lone hole
            // reports from its top, and its entry point and depth come with it.
            let axis = canonical(h.axis);
            let t = if axis.dot(h.axis) >= 0.0 {
                h.t
            } else {
                [-h.t[1], -h.t[0]]
            };
            Some(Lone {
                body,
                at: h.point + axis * t[0],
                axis,
                d: h.d,
                depth: t[1] - t[0],
                slot: h.slot,
            })
        })
        .collect();
    Ok(Stacks { stacks: out, lone })
}

/// A line's foot: its nearest point to the origin, as bucket indices of side `cell`.
fn foot(b: &Bore, cell: f64) -> Option<[i64; 3]> {
    let p = b.point - b.axis * b.point.dot(b.axis);
    p.is_finite()
        .then(|| [bucket(p.x, cell), bucket(p.y, cell), bucket(p.z, cell)])
}

/// The bucket a coordinate falls in. The cast saturates, so a coordinate beyond any bucket
/// only collides with the other coordinates out there, which the exact tests then reject.
fn bucket(x: f64, cell: f64) -> i64 {
    (x / cell).floor() as i64
}

/// Union-find root.
fn find(p: &mut [usize], mut i: usize) -> usize {
    for _ in 0..p.len() {
        match p.get(i) {
            Some(up) if *up != i => i = *up,
            _ => break,
        }
    }
    i
}

fn join(p: &mut [usize], i: usize, j: usize) {
    let (ri, rj) = (find(p, i), find(p, j));
    if let Some(x) = p.get_mut(ri.max(rj)) {
        *x = ri.min(rj);
    }
}

/// The groups of a union-find, each with its members in id order, the groups in root order.
fn groups(parent: &mut [usize], ids: &[usize]) -> Vec<Vec<usize>> {
    let mut by_root: BTreeMap<usize, Vec<usize>> = BTreeMap::new();
    for &i in ids {
        let root = find(parent, i);
        match by_root.get_mut(&root) {
            Some(g) => g.push(i),
            None => {
                by_root.insert(root, vec![i]);
            }
        }
    }
    by_root.into_values().collect()
}

/// Two axes are parallel, either way round.
fn parallel(a: Vec3, b: Vec3) -> bool {
    a.dot(b).abs() > PARALLEL
}

/// The distance, across `axis`, from the axis line through `p` to a bore's centre line (a
/// point for a round hole, a segment for a slot).
fn offset(p: Vec3, axis: Vec3, h: &Bore) -> f64 {
    let flat = |v: Vec3| v - axis * v.dot(axis);
    let w = flat(h.point - p);
    match h.slot {
        None => w.len(),
        Some(s) => {
            let half = ((s.length - h.d) / 2.0).max(0.0);
            let along = w.dot(s.along);
            let along = if along < -half {
                -half
            } else if along > half {
                half
            } else {
                along
            };
            (w - s.along * along).len()
        }
    }
}

/// A bore's extent along `axis`, measured from `p`: its own extent, turned round when its axis
/// points the other way.
fn extent(h: &Bore, p: Vec3, axis: Vec3) -> (f64, f64) {
    let base = (h.point - p).dot(axis);
    let d = h.axis.dot(axis);
    // A bore's ends land on the line at its own axial step times how much of its axis
    // points along the line's, either way round.
    let (t0, t1) = (h.t[0] * d.abs(), h.t[1] * d.abs());
    if d >= 0.0 {
        (base + t0, base + t1)
    } else {
        (base - t1, base - t0)
    }
}

/// How far apart two round bores' lines sit over the stretch the bores themselves reach:
/// the perpendicular distance from `a`'s line to `b`'s, at each bore's two ends. Skewed
/// lines drift apart along the axis, so one measurement at a frame origin can read tight
/// while the bores sit apart; exactly parallel lines measure the same at every end.
fn apart(a: &Bore, b: &Bore) -> f64 {
    let (b0, b1) = extent(b, a.point, a.axis);
    let at = |s: f64| {
        let w = a.point + a.axis * s - b.point;
        (w - b.axis * w.dot(b.axis)).len()
    };
    at(a.t[0]).max(at(a.t[1])).max(at(b0)).max(at(b1))
}

/// Two slots on one axis line: parallel, the same way across, one centre line, and overlapping.
fn same_slot_line(a: &Bore, b: &Bore, tol: f64) -> bool {
    let (Some(sa), Some(sb)) = (a.slot, b.slot) else {
        return false;
    };
    if !parallel(a.axis, b.axis) || sa.along.dot(sb.along).abs() < 1.0 - 1e-4 {
        return false;
    }
    let w = b.point - a.point;
    let flat = w - a.axis * w.dot(a.axis);
    let along = flat.dot(sa.along);
    let across = (flat - sa.along * along).len();
    let (ha, hb) = (
        ((sa.length - a.d) / 2.0).max(0.0),
        ((sb.length - b.d) / 2.0).max(0.0),
    );
    across <= tol && along.abs() <= ha + hb + tol
}

/// The stack's way along an axis: the largest component negative (−z for a vertical axis), so
/// a stack of plates lying on the XY plane starts at its top.
fn canonical(a: Vec3) -> Vec3 {
    let big = if a.x.abs() >= a.y.abs() && a.x.abs() >= a.z.abs() {
        a.x
    } else if a.y.abs() >= a.z.abs() {
        a.y
    } else {
        a.z
    };
    if big > 0.0 {
        -a
    } else {
        a
    }
}

/// The refusal for too many comparisons.
fn too_many() -> SolidError {
    SolidError::invalid_input(
        OP,
        "too many nearly-coaxial holes: narrow the call with fewer bodies or diameter ranges",
    )
}

#[cfg(test)]
#[path = "hole_stacks_tests.rs"]
mod tests;
