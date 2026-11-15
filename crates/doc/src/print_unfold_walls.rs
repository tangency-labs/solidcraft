//! Unfold for Printing: how the printable parts are made, from the body itself. Every face
//! becomes a wall where it lies on the body, the face's outline taken into the material by the
//! wall thickness, and only then turned into the flat. So folded up again, the walls are the
//! body's own shell, and no two of them can stand in each other.
//!
//! One rule serves every edge. Where two faces meet round the outside of the body their walls
//! share the corner behind the edge: each ends on the plane halfway between the two faces, half
//! the clearance short of it (a mitre). At a bend the two mitres open into the V-groove as the
//! walls turn flat, and a film under it joins them. Where two faces meet in an inside corner
//! their walls share nothing: each ends square at the edge, and a bend there gets a slit down to
//! the film, since the walls turn the other way. A wall never leaves its face's outline, so the
//! flat parts lie on each other only where the pattern's faces do.
//!
//! A cut can be a press fit in place of the mitre ([`press_fingers`]). The corner the two walls
//! share is then dealt out along the edge in stretches, a finger each: on its own stretch a wall
//! keeps the whole corner, on the other's it ends short of the other wall. A nub on the end face
//! of a finger snaps into a dimple in what stands next to it. The fingers too are nothing but
//! the body's shell, so they can't stand in a wall either. The ends of a cut stay mitred, so
//! where three faces meet every wall ends as it does without.
//!
//! A filament joint is locked by a length of filament pushed through a channel. On a cut it has
//! the fingers of a press fit, the channel through the middle of the corner they share.
//!
//! A bend can be a joint too, to hold its fold. Its core (a filament's channel, or a press
//! fit's nub and dimple) lies on the line halfway between the two faces, as far from the bend as
//! its walls need ([`fold_core`]), and its fingers are a sector round the bend's edge: the walls
//! turn about that edge, so a sector round it stays on itself and passes through nothing on the
//! way. Where one wall has such a finger the other is cut back round it, down to its film. A
//! channel runs on through the walls met at both ends of its edge, which is where the filament
//! goes in.

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, Body};

use crate::print_joint::{
    CRUMB, DEFAULT_CHANNEL_TOLERANCE, DEFAULT_FILAMENT, DEFAULT_FILM, DEFAULT_JOINT_WALL, FOLD_JOINT_ANGLES, MIN_FINGER_WIDTH, PressFit, PrintFold,
    fold_core,
};
use crate::print_unfold::{EdgeKind, EdgeTurn, UnfoldEdge, UnfoldFace, UnfoldGraph, UnfoldLayout};
use crate::print_unfold_flat::FlatLayout;
use crate::{DocError, Mat, Result, apply_point, mat_inverse};

/// The widest angle two faces turn by at an edge that is still mitred (degrees): beyond it
/// they are as good as in one plane, and the mitre would be a blade.
const MITRE_UP_TO: f64 = 170.0;

/// The most fingers a press fit has along one edge.
const PRESS_MOST: usize = 9;

/// How far beside a cut another wall of the flat pattern is looked for (mm), and how far from
/// one the fingers of the cut stay.
const BESIDE: f64 = 0.3;
const BESIDE_CLEAR: f64 = 1.0;

/// The thinnest wall a press fit's nub has room in (mm).
pub const PRESS_MIN_WALL: f64 = 1.5;

/// The wall a print starts with (mm).
pub const DEFAULT_WALL: f64 = 3.0;

/// The gap between two fingers of a press fit along the cut (mm). The clearance of the walls
/// (at a mitre, and between a finger's end and the other wall) is the print's own.
pub const DEFAULT_FINGER_GAP: f64 = 0.2;

/// A press fit's nub: its radius on the finger's end face (less in a thin wall) and how far it
/// stands out (mm).
const NUB_RADIUS: f64 = 1.0;
const NUB_HEIGHT: f64 = 0.5;

/// The narrowest finger of a press fit, and the shortest stretch of a cut that gets one (mm).
const FINGER_WIDTH: f64 = 3.0;
const FINGER_ROOM: f64 = 3.0;

/// The side of a filament channel (mm): the filament and its play.
const CHANNEL: f64 = DEFAULT_FILAMENT + DEFAULT_CHANNEL_TOLERANCE;

/// The thinnest wall a filament joint on a cut has room in (mm): the channel and a film of
/// wall either side of it.
pub const FILAMENT_MIN_WALL: f64 = CHANNEL + 2.0 * DEFAULT_FILM;

/// The slit left open over a bend in an inside corner (mm).
const SLIT: f64 = 0.3;

/// The walls of the print: their thickness, the film a bend turns on, the clearance between
/// two walls where they meet, and the gap between two fingers of a press fit (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallPrint {
    pub wall: f64,
    pub film: f64,
    pub clearance: f64,
    pub gap: f64,
}

/// What joins two walls along an edge. On a cut: a press fit's nubs, or a filament channel
/// through its fingers. On a bend: fingers round the edge, a sector `r` round it, with their
/// core `d` from the edge on the line halfway between the faces; a filament's channel runs
/// there, or a press fit's nub sits at `nub` ([`FoldCore::nub`]).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Fit {
    Press,
    Channel,
    Knuckle { d: f64, r: f64, nub: Option<Vec2> },
}

/// Whether two faces meet square round the outside of the body at an edge that turns by `turn`.
fn square(turn: EdgeTurn) -> bool {
    matches!(turn, EdgeTurn::Outside(angle) if (angle - std::f64::consts::FRAC_PI_2).abs() < 0.17)
}

/// The joint of an edge: what it is, and where its fingers begin and end along the edge.
#[derive(Clone, Debug)]
struct Joint {
    fit: Fit,
    stops: Vec<f64>,
}

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

/// Where the fingers of a press fit begin and end along a cut `len` long (from its first end,
/// mm): one finger between each two stops, the first the first face's, then by turns. Both ends
/// of the cut stay mitred, further than the walls of the faces met there reach, so no finger
/// stands in a corner of three faces. Empty where the cut is too short for a finger.
pub fn press_fingers(len: f64, w: WallPrint) -> Vec<f64> {
    let keep = press_keep(w);
    if w.wall >= PRESS_MIN_WALL { fingers_between(keep, len - keep, FINGER_WIDTH) } else { Vec::new() }
}

/// Where the fingers of a joint (a filament's, or with `press` a press fit's) begin and end
/// along a bend that turns by `turn` and is `len` long, with its core and the radius of its
/// fingers ([`fold_core`]). None where the bend can't take one: an inside corner, a fold the
/// fingers can't do, or too short for two fingers clear of both its ends.
fn knuckles(turn: EdgeTurn, len: f64, w: WallPrint, press: bool) -> Option<Joint> {
    let EdgeTurn::Outside(angle) = turn else { return None };
    let (lo, hi) = FOLD_JOINT_ANGLES;
    if !(angle.to_degrees() >= lo - 1e-6 && angle.to_degrees() <= hi + 1e-6) {
        return None;
    }
    let half = if press { NUB_RADIUS } else { CHANNEL / 2.0 };
    let core = fold_core(w.wall, angle, PrintFold { film: w.film, clearance: w.clearance }, half, press, DEFAULT_JOINT_WALL).ok()?;
    let keep = core.r + w.clearance + 1.5;
    let stops = fingers_between(keep, len - keep, MIN_FINGER_WIDTH);
    (stops.len() >= 3).then_some(Joint { fit: Fit::Knuckle { d: core.d, r: core.r, nub: press.then_some(core.nub) }, stops })
}

/// Whether a bend that turns by `turn` and is `len` long can be a filament joint, or with
/// `press` a press fit.
pub fn bend_joint_possible(turn: EdgeTurn, len: f64, w: WallPrint, press: bool) -> bool {
    knuckles(turn, len, w, press).is_some()
}

/// Whether a cut that turns by `turn` and is `len` long can be a filament joint: as a press
/// fit, with room for two fingers and a wall thick enough for the channel.
pub fn cut_filament_possible(turn: EdgeTurn, len: f64, w: WallPrint) -> bool {
    square(turn) && w.wall >= FILAMENT_MIN_WALL && press_fingers(len, w).len() >= 3
}

/// How far from each end of a cut its fingers stay (mm).
fn press_keep(w: WallPrint) -> f64 {
    w.wall + w.clearance + 1.5
}

/// The stops of the fingers that fill a cut from `lo` to `hi` along it; none where that is
/// too short for one `width` wide.
fn fingers_between(lo: f64, hi: f64, width: f64) -> Vec<f64> {
    let room = hi - lo;
    if !(room.is_finite() && room >= width.max(FINGER_ROOM)) {
        return Vec::new();
    }
    let most = (room / width).floor();
    let count = if most >= 1.0 { (most as usize).min(PRESS_MOST) } else { 1 };
    (0..=count).map(|k| lo + room * k as f64 / count as f64).collect()
}

/// Whether a cut that turns by `turn` and is `len` long can be a press fit: its faces meet square
/// round the outside of the body, and it has room for a finger.
pub fn press_possible(turn: EdgeTurn, len: f64, w: WallPrint) -> bool {
    square(turn) && !press_fingers(len, w).is_empty()
}

/// A box on `plane`: `x` and `y` in it, `z` along its normal.
fn block(plane: &Plane, x: (f64, f64), y: (f64, f64), z: (f64, f64)) -> Result<Body> {
    let outer = Loop2::polygon(&[Vec2::new(x.0, y.0), Vec2::new(x.1, y.0), Vec2::new(x.1, y.1), Vec2::new(x.0, y.1)]).ccw();
    Ok(kernel::extrude_planar_regions(plane, &[Region2 { outer, holes: Vec::new() }], z.0, z.1)?)
}

/// The unit direction in the plane of `face` from its edge `edge` into the face.
fn into_face(face: &UnfoldFace, edge: &UnfoldEdge) -> Option<Vec3> {
    let along = edge.ends.1 - edge.ends.0;
    let side = face.normal().cross(along.normalized()?).normalized()?;
    let step = (along.len() * 1e-4).max(1e-7);
    let middle = edge.middle();
    match (face.contains(middle + side * step), face.contains(middle - side * step)) {
        (true, false) => Some(side),
        (false, true) => Some(side * -1.0),
        _ => None,
    }
}

/// A wall after its cuts, in one piece: a crumb the cuts left loose is dropped.
fn whole(wall: Body) -> Result<Body> {
    let lumps = wall.lumps()?;
    if lumps.len() < 2 {
        return Ok(wall);
    }
    let mut kept = Vec::with_capacity(lumps.len());
    for lump in lumps {
        if kernel::measure(&lump)?.volume >= CRUMB {
            kept.push(lump);
        }
    }
    match kept.len() {
        0 => err("a face is too small for its wall: only crumbs of it are left between the faces it meets"),
        1 => Ok(kept.swap_remove(0)),
        _ => kernel::join_apart(&kept)?.ok_or_else(|| DocError::Invalid("the pieces of a wall can't be kept together".into())),
    }
}

impl UnfoldGraph {
    /// The wall of face `f` where it lies on the body: its outline `w.wall` into the material,
    /// mitred at the edges round the outside of the body, slit at its inside-corner bends, with
    /// its fingers where an edge has a joint (`joints`, [`Self::joints`]): a press fit's nubs
    /// and dimples on them, or a filament's channel through them.
    fn wall(&self, f: usize, layout: &UnfoldLayout, splits: &[usize], w: WallPrint, joints: &[Option<Joint>]) -> Result<Body> {
        let Some(face) = self.faces.get(f) else { return err("a face of the pattern is not a face of the body") };
        let (t, c) = (w.wall, w.clearance);
        let over = (t * 0.05).max(1e-3);
        let n = face.normal();
        let radius = NUB_RADIUS.min(t / 2.0 - 0.3);
        let nub = PressFit { radius, height: NUB_HEIGHT };
        let mut wall = kernel::extrude_planar_regions(&face.plane, &[face.region()], -t, 0.0)?;
        let (mut tools, mut caps, mut knuckled): (Vec<Body>, Vec<(bool, Body)>, Vec<Body>) = (Vec::new(), Vec::new(), Vec::new());
        // A filament channel's box round its core line, lined up with `side`, on through the
        // walls met at both ends of the edge.
        let (reach, bore) = (t + 1.0, CHANNEL / 2.0);
        let channel = |edge: &UnfoldEdge, d: Vec3, core: Vec3, side: Vec3| {
            let len = edge.ends.0.dist(edge.ends.1);
            block(&Plane { origin: edge.ends.0 + core, x: d, y: side }, (-reach, len + reach), (-bore, bore), (-bore, bore))
        };
        let mut seen: Vec<usize> = Vec::with_capacity(face.edges.len());
        for &e in &face.edges {
            if seen.contains(&e) {
                continue;
            }
            seen.push(e);
            let Some(edge) = self.edges.get(e) else { continue };
            let (Some(other), Some(d), Some(into)) =
                (self.faces.get(edge.across(f)), (edge.ends.1 - edge.ends.0).normalized(), into_face(face, edge))
            else {
                continue;
            };
            let len = edge.ends.0.dist(edge.ends.1);
            let bend = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e);
            match edge.turn {
                EdgeTurn::Outside(angle) if angle.to_degrees() < MITRE_UP_TO => {
                    // The plane halfway between the two faces: through the edge and the line
                    // that runs from it into the material at the same angle to both.
                    let Some(inward) = ((n + other.normal()) * -1.0).normalized() else { continue };
                    let (mut across, mut y) = (d.cross(inward), inward);
                    if across.dot(into) > 0.0 {
                        (across, y) = (across * -1.0, inward * -1.0);
                    }
                    let deep = t / (angle / 2.0).cos() + over;
                    let span = if y.dot(inward) > 0.0 { (-over, deep) } else { (-deep, over) };
                    let plane = Plane { origin: edge.ends.0 - across * (c / 2.0), x: d, y };
                    let mitre = |a: f64, b: f64| block(&plane, (a, b), span, (0.0, t + c + over));
                    let joint = joints.get(e).and_then(Option::as_ref);
                    let (fit, stops) = joint.map_or((Fit::Press, &[][..]), |j| (j.fit, j.stops.as_slice()));
                    let (Some(&lo), Some(&hi), Some(into_other)) = (stops.first(), stops.last(), into_face(other, edge)) else {
                        tools.push(mitre(0.0, len)?);
                        continue;
                    };
                    // A press fit. The fingers are dealt out by turns from the first face; the
                    // ends of the cut stay mitred (a little into where this wall ends short
                    // anyway, so no skin is left between the two cuts).
                    let count = stops.len() - 1;
                    let own = |j: usize| j.is_multiple_of(2) == (edge.faces.0 == f);
                    let half = w.gap / 2.0;
                    tools.push(mitre(0.0, if own(0) { lo + half } else { lo - half + over })?);
                    tools.push(mitre(if own(count - 1) { hi - half } else { hi + half - over }, len)?);
                    // The other face's plane: `into_other` in it, and which way is out of the body.
                    let flat = Plane { origin: edge.ends.0, x: d, y: into_other };
                    let up = d.cross(into_other).dot(other.normal()) > 0.0;
                    if let Fit::Knuckle { d: core, r, nub: seat } = fit {
                        // A joint on a bend. This face's own plane, as the other's above.
                        let mine = Plane { origin: edge.ends.0, x: d, y: into };
                        let out = d.cross(into).dot(n) > 0.0;
                        let (m, far) = (w.film + c, r + c + 1.0);
                        // A filament's channel through the fingers, or a press fit's nubs where
                        // two fingers meet: on the first face's, the dimples in the other's.
                        let bore = if seat.is_none() { Some(channel(edge, d, inward * core, into)?) } else { None };
                        let first = edge.faces.0 == f;
                        let mut fits: Vec<Body> = Vec::new();
                        if let Some(at) = seat {
                            let knob = PressFit { radius: NUB_RADIUS, height: NUB_HEIGHT };
                            let (ball, back) = (knob.ball(), knob.ball() - knob.height);
                            // From the edge into the first face, and down from its skin.
                            let (into_first, down) = if first { (into, n * -1.0) } else { (into_other, other.normal() * -1.0) };
                            let origin = edge.ends.0 + into_first * -at.x + down * at.y;
                            let seat = Plane { origin, x: d, y: inward };
                            let (eps, wide) = (over.min(0.05), ball + over);
                            for k in 1..count {
                                let Some(&stop) = stops.get(k) else { continue };
                                // The first face's finger there, and which way its nub points.
                                let dir = if (k - 1).is_multiple_of(2) { 1.0 } else { -1.0 };
                                let on = stop - dir * half;
                                let (u0, u1) = (on - dir * eps, on + dir * (knob.height + eps));
                                fits.push(
                                    kernel::boolean(
                                        &kernel::sphere(origin + d * (on - dir * back), ball)?,
                                        &block(&seat, (u0.min(u1), u0.max(u1)), (-wide, wide), (-wide, wide))?,
                                        kernel::BoolOp::Intersect,
                                    )?
                                    .ok_or_else(|| DocError::Invalid("a press-fit nub on a bend came out empty".into()))?,
                                );
                            }
                        }
                        // Mitred as at any bend, but for where its own fingers stand.
                        let mut from = 0.0;
                        for j in (0..count).filter(|j| own(*j)) {
                            let (Some(&a), Some(&b)) = (stops.get(j), stops.get(j + 1)) else { continue };
                            tools.push(mitre(from, a + half)?);
                            from = b - half;
                        }
                        tools.push(mitre(from, len)?);
                        for j in 0..count {
                            let (Some(&a), Some(&b)) = (stops.get(j), stops.get(j + 1)) else { continue };
                            if own(j) {
                                let (a, b) = (a + half, b - half);
                                // Its own finger: the wall ends the film and the clearance short
                                // of the other face's skin, and so does the sector round the edge
                                // that stands on it.
                                let skin = block(&flat, (a - 1.0, b + 1.0), (-far, far), if up { (-m, far) } else { (-far, m) })?;
                                tools.push(block(&flat, (a, b), (-far, far), if up { (-m, far) } else { (-far, m) })?);
                                let behind = block(&mine, (a - 1.0, b + 1.0), (-far, far), if out { (-far, 0.0) } else { (0.0, far) })?;
                                let mut finger = kernel::cylinder(edge.ends.0 + d * a, d, r, b - a)?;
                                let dimples = fits.iter().filter(|_| !first);
                                let cuts = [&skin].into_iter().chain(&bore).chain(dimples).map(|tool| (tool, kernel::BoolOp::Cut));
                                for (tool, op) in [(&behind, kernel::BoolOp::Intersect)].into_iter().chain(cuts) {
                                    finger = kernel::boolean(&finger, tool, op)?
                                        .ok_or_else(|| DocError::Invalid("a finger of a joint on a bend came out empty".into()))?;
                                }
                                knuckled.push(finger);
                            } else {
                                // The other's finger: this wall is cut back round it, the
                                // clearance clear, down to its film.
                                let (a, b) = (a - half, b + half);
                                let above = block(&mine, (a - 1.0, b + 1.0), (-far, far), if out { (-far, -w.film) } else { (w.film, far) })?;
                                if let Some(tool) =
                                    kernel::boolean(&kernel::cylinder(edge.ends.0 + d * a, d, r + c, b - a)?, &above, kernel::BoolOp::Intersect)?
                                {
                                    tools.push(tool);
                                }
                            }
                        }
                        tools.extend(bore);
                        // The first face's wall carries the nubs; the other's has the dimples,
                        // in its wall as in its fingers.
                        caps.extend(fits.into_iter().map(|cap| (first, cap)));
                        continue;
                    }
                    for j in 0..count {
                        let (Some(&a), Some(&b)) = (stops.get(j), stops.get(j + 1)) else { continue };
                        if !own(j) {
                            // The other's finger: this wall ends the clearance short of its wall.
                            let z = if up { (-(t + c), t) } else { (-t, t + c) };
                            tools.push(block(&flat, (a - half, b + half), (-t, 2.0 * t + c), z)?);
                        } else if angle > std::f64::consts::FRAC_PI_2 + 1e-6 {
                            // Its own: the whole corner, but nothing beyond the other face.
                            let z = if up { (0.0, t) } else { (-t, 0.0) };
                            tools.push(block(&flat, (a + half, b - half), (-t, 2.0 * t), z)?);
                        }
                    }
                    let middle = inward * (t / 2.0 / (angle / 2.0).cos());
                    if fit == Fit::Channel {
                        // A filament joint on a cut: the channel through the middle of the corner.
                        tools.push(channel(edge, d, middle, into)?);
                        continue;
                    }
                    // A nub where a finger ends, in the middle of the corner the walls share: on
                    // the first face's finger where two fingers meet. What stands next to it
                    // (the other's finger, or its mitred end) gets the dimple.
                    if nub.radius < nub.height {
                        continue;
                    }
                    let (ball, back) = (nub.ball(), nub.ball() - nub.height);
                    let seat = Plane { origin: edge.ends.0 + middle, x: d, y: inward };
                    let eps = over.min(0.05);
                    for (k, &stop) in stops.iter().enumerate() {
                        // The finger that carries the nub, and which way the nub points.
                        let (j, dir) = match (k.checked_sub(1), k < count) {
                            (Some(before), true) if before.is_multiple_of(2) => (before, 1.0),
                            (_, true) => (k, -1.0),
                            (Some(before), false) => (before, 1.0),
                            (None, false) => continue,
                        };
                        let at = stop - dir * half;
                        let (u0, u1) = (at - dir * eps, at + dir * (nub.height + eps));
                        let wide = ball + over;
                        let cap = kernel::boolean(
                            &kernel::sphere(edge.ends.0 + middle + d * (at - dir * back), ball)?,
                            &block(&seat, (u0.min(u1), u0.max(u1)), (-wide, wide), (-wide, wide))?,
                            kernel::BoolOp::Intersect,
                        )?
                        .ok_or_else(|| DocError::Invalid("a press-fit nub came out empty".into()))?;
                        caps.push((own(j), cap));
                    }
                }
                EdgeTurn::Inside if bend => {
                    // Above the film, half the slit from the edge.
                    let plane = Plane { origin: edge.ends.0, x: d, y: into };
                    let depth = if d.cross(into).dot(n) > 0.0 { (-(t + over), -w.film) } else { (w.film, t + over) };
                    tools.push(block(&plane, (0.0, len), (-over, SLIT / 2.0), depth)?);
                }
                _ => {}
            }
        }
        // The channel of a filament joint this face is no part of, where the edge ends on it.
        for (e, joint) in joints.iter().enumerate() {
            let (Some(joint), Some(edge)) = (joint, self.edges.get(e)) else { continue };
            if edge.faces.0 == f || edge.faces.1 == f || matches!(joint.fit, Fit::Press | Fit::Knuckle { nub: Some(_), .. }) {
                continue;
            }
            let ends_here = [edge.ends.0, edge.ends.1].iter().any(|p| face.loops.iter().flatten().any(|q| q.dist(*p) <= self.near));
            let (true, Some(a), Some(b), Some(d), EdgeTurn::Outside(angle)) =
                (ends_here, self.faces.get(edge.faces.0), self.faces.get(edge.faces.1), (edge.ends.1 - edge.ends.0).normalized(), edge.turn)
            else {
                continue;
            };
            let (Some(inward), Some(side)) = (((a.normal() + b.normal()) * -1.0).normalized(), into_face(a, edge)) else { continue };
            let core = match joint.fit {
                Fit::Knuckle { d, .. } => d,
                _ => t / 2.0 / (angle / 2.0).cos(),
            };
            tools.push(channel(edge, d, inward * core, side)?);
        }
        let small = || DocError::Invalid("a face is too small for its wall: nothing of it is left between the faces it meets".into());
        for tool in &tools {
            wall = kernel::boolean(&wall, tool, kernel::BoolOp::Cut)?.ok_or_else(small)?;
        }
        // The dimples first, then what the cuts left in one piece, then its fingers round the
        // bends and its nubs on it.
        // A wall has many nubs and dimples, each far from the next: they are cut and joined as
        // one body of several pieces, in one go, where they are shown to be apart.
        let at_once = |list: Vec<Body>| -> Result<Vec<Body>> {
            Ok(match kernel::union_apart(&list)? {
                Some(one) => vec![one],
                None => list,
            })
        };
        let (nubs, dimples): (Vec<(bool, Body)>, Vec<(bool, Body)>) = caps.into_iter().partition(|(mine, _)| *mine);
        for cap in at_once(dimples.into_iter().map(|(_, cap)| cap).collect())? {
            wall = kernel::boolean(&wall, &cap, kernel::BoolOp::Cut)?.ok_or_else(small)?;
        }
        let mut wall = whole(wall)?;
        let nubs = at_once(nubs.into_iter().map(|(_, cap)| cap).collect())?;
        for piece in knuckled.iter().chain(&nubs) {
            wall = kernel::boolean(&wall, piece, kernel::BoolOp::Union)?.ok_or_else(small)?;
        }
        Ok(wall)
    }

    /// The joint of each edge, and where its fingers begin and end along it; none for an edge
    /// that has none, or no room for one. On a cut a finger keeps its wall up to the face's
    /// outline, so a cut has none where another wall of the same part lies beside it in the
    /// flat (a wall folded out next to it, at an inside corner of a face's outline): there the
    /// two would print as one and the bend beside them could not turn. The cut stays mitred
    /// along that stretch.
    fn joints(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: WallPrint, kinds: &[String]) -> Vec<Option<Joint>> {
        let back: Vec<Option<Mat>> = flat.moves.iter().map(mat_inverse).collect();
        // Whether another face of its part lies at `at` (on the body, in the plane of face `f`)
        // once both are flat.
        let beside = |f: usize, at: Vec3| -> bool {
            let (Some(m), Some(part)) = (flat.moves.get(f), flat.parts.iter().find(|p| p.faces.contains(&f))) else { return false };
            let p = apply_point(m, at);
            part.faces.iter().filter(|g| **g != f).any(|&g| match (self.faces.get(g), back.get(g).copied().flatten()) {
                (Some(face), Some(inv)) => face.contains(apply_point(&inv, p)),
                _ => false,
            })
        };
        let keep = press_keep(w);
        let mut out = Vec::with_capacity(self.edges.len());
        for (e, edge) in self.edges.iter().enumerate() {
            let bend = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e);
            let len = edge.ends.0.dist(edge.ends.1);
            let room = len - 2.0 * keep;
            let d = (edge.ends.1 - edge.ends.0).normalized();
            let kind = kinds.get(e).map_or("", String::as_str);
            if bend {
                out.push(match kind {
                    "filament" => knuckles(edge.turn, len, w, false),
                    "press" => knuckles(edge.turn, len, w, true),
                    _ => None,
                });
                continue;
            }
            let fit = match kind {
                "press" if w.wall >= PRESS_MIN_WALL => Fit::Press,
                "filament" if w.wall >= FILAMENT_MIN_WALL => Fit::Channel,
                _ => {
                    out.push(None);
                    continue;
                }
            };
            let (true, true, Some(d)) = (square(edge.turn), room >= FINGER_ROOM, d) else {
                out.push(None);
                continue;
            };
            // Just outside each of the two faces, along the cut.
            let sides: Vec<(usize, Vec3)> =
                [edge.faces.0, edge.faces.1].into_iter().filter_map(|f| Some((f, into_face(self.faces.get(f)?, edge)? * -BESIDE))).collect();
            let count = (room / 0.5).ceil().max(1.0) as usize;
            let at = |i: usize| keep + room * i as f64 / count as f64;
            // The longest stretch with no wall beside it.
            let (mut best, mut run): (Option<(usize, usize)>, Option<usize>) = (None, None);
            for i in 0..=count {
                let free = !sides.iter().any(|(f, off)| beside(*f, edge.ends.0 + d * at(i) + *off));
                run = if free { run.or(Some(i)) } else { None };
                if let Some(from) = run
                    && best.is_none_or(|(a, b)| i - from > b - a)
                {
                    best = Some((from, i));
                }
            }
            let stops = match best {
                Some((a, b)) => fingers_between(
                    if a == 0 { keep } else { at(a) + BESIDE_CLEAR },
                    if b == count { len - keep } else { at(b) - BESIDE_CLEAR },
                    FINGER_WIDTH,
                ),
                None => Vec::new(),
            };
            // A filament holds only what it runs through on both walls: two fingers at least.
            let least = if fit == Fit::Channel { 3 } else { 2 };
            out.push((stops.len() >= least).then_some(Joint { fit, stops }));
        }
        // The fingers round a bend reach into the body. A bend keeps them only where they stay
        // clear of every wall that is not one of its own two or met at its ends, and of the
        // fingers round a bend before it: else it turns on its film alone.
        let mut kept: Vec<(usize, f64, f64, f64)> = Vec::new();
        for e in 0..out.len() {
            let (Some(Some(Joint { fit: Fit::Knuckle { r, .. }, stops })), Some(edge)) = (out.get(e), self.edges.get(e)) else { continue };
            let r = *r;
            let (Some(&lo), Some(&hi), Some(d)) = (stops.first(), stops.last(), (edge.ends.1 - edge.ends.0).normalized()) else { continue };
            let Some((u, v)) = self.faces.get(edge.faces.0).and_then(|face| Some((into_face(face, edge)?, face.normal()))) else { continue };
            // Points of the cylinder the fingers lie in: round the edge, along their stretch.
            let steps = (hi - lo).ceil().max(1.0) as usize;
            let mut points = Vec::with_capacity((steps + 1) * 25);
            for i in 0..=steps {
                let on = edge.ends.0 + d * (lo + (hi - lo) * i as f64 / steps as f64);
                points.push(on);
                for k in 0..12 {
                    let (sin, cos) = (f64::from(k) * std::f64::consts::PI / 6.0).sin_cos();
                    let out_by = u * cos + v * sin;
                    points.push(on + out_by * r);
                    points.push(on + out_by * (r / 2.0));
                }
            }
            let slack = w.clearance + 0.3;
            let in_wall = self.faces.iter().enumerate().any(|(g, face)| {
                let ends_here = [edge.ends.0, edge.ends.1].iter().any(|p| face.loops.iter().flatten().any(|q| q.dist(*p) <= self.near));
                if g == edge.faces.0 || g == edge.faces.1 || ends_here {
                    return false;
                }
                let normal = face.normal();
                points.iter().any(|p| {
                    let depth = (face.plane.origin - *p).dot(normal);
                    let foot = *p + normal * depth;
                    depth > -slack && depth < w.wall + slack && (face.contains(foot) || face.boundary_distance(foot) < slack)
                })
            });
            let in_fingers = kept.iter().any(|(other, reach, from, to)| {
                let Some((o, along)) = self.edges.get(*other).and_then(|o| Some((o, (o.ends.1 - o.ends.0).normalized()?))) else { return false };
                points.iter().any(|p| {
                    let at = (*p - o.ends.0).dot(along);
                    at > from - slack && at < to + slack && (*p - (o.ends.0 + along * at)).len() < reach + slack
                })
            });
            if in_wall || in_fingers {
                if let Some(slot) = out.get_mut(e) {
                    *slot = None;
                }
            } else {
                kept.push((e, r, lo, hi));
            }
        }
        out
    }

    /// Whether each edge of the body gets the fingers of its joint, as [`Self::wall_solids`]
    /// makes them from the same `kinds`: not every edge asked to have one has room for them.
    pub fn jointed_edges(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: WallPrint, kinds: &[String]) -> Vec<bool> {
        self.joints(layout, flat, splits, w, kinds).iter().map(Option::is_some).collect()
    }

    /// The printable body of each part of the flat pattern `flat` (split at `splits`), in the
    /// order of its parts: the walls of the part's faces turned flat, and a film under each of
    /// its bends. `kinds`: the joint of every edge of the body; an edge that is `press` or
    /// `filament` gets fingers where it can ([`Self::jointed_edges`]), every other cut is
    /// mitred and every other bend turns on its film alone.
    pub fn wall_solids(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: WallPrint, kinds: &[String]) -> Result<Vec<Body>> {
        if !(w.wall.is_finite()
            && w.film.is_finite()
            && w.clearance.is_finite()
            && w.film > 0.0
            && w.wall > w.film
            && w.clearance >= 0.0
            && w.gap.is_finite()
            && w.gap >= 0.0)
        {
            return err("the wall must be thicker than the film a bend turns on");
        }
        if w.wall < PRESS_MIN_WALL && kinds.iter().any(|k| k == "press") {
            return err(format!("a press fit needs a wall at least {PRESS_MIN_WALL} mm thick"));
        }
        let cut = |e: usize| !(layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e));
        if w.wall < FILAMENT_MIN_WALL && kinds.iter().enumerate().any(|(e, k)| k == "filament" && cut(e)) {
            return err(format!("a filament joint on a cut needs a wall at least {FILAMENT_MIN_WALL} mm thick"));
        }
        let Some(base) = self.faces.get(layout.base) else { return err("the base face is not a face of the body") };
        let floor = Plane { origin: Vec3::new(0.0, 0.0, base.plane.origin.z), x: Vec3::X, y: Vec3::Y };
        let joints = self.joints(layout, flat, splits, w, kinds);
        // Every wall where it lies on the body.
        let mut walls: Vec<Option<Body>> = vec![None; self.faces.len()];
        for &f in flat.parts.iter().flat_map(|p| &p.faces) {
            if let Some(slot) = walls.get_mut(f) {
                *slot = Some(self.wall(f, layout, splits, w, &joints)?);
            }
        }
        // The fingers of a joint on a bend reach into the body. They are no part of its shell,
        // so they must be shown to stand in no other wall ([`Self::joints`] keeps them clear;
        // this is the exact check of it). Two walls that share an edge are made to each other
        // there and are not looked at again: checking every such pair took minutes on a part.
        let knuckled = |f: usize| {
            self.faces
                .get(f)
                .is_some_and(|face| face.edges.iter().any(|e| matches!(joints.get(*e), Some(Some(Joint { fit: Fit::Knuckle { .. }, .. })))))
        };
        for f in (0..walls.len()).filter(|f| knuckled(*f)) {
            for g in 0..walls.len() {
                let (Some(Some(a)), Some(Some(b)), Some(face)) = (walls.get(f), walls.get(g), self.faces.get(f)) else { continue };
                let share = face.edges.iter().any(|e| self.edges.get(*e).is_some_and(|edge| edge.faces.0 == g || edge.faces.1 == g));
                if g == f || share || (knuckled(g) && g < f) {
                    continue;
                }
                if kernel::apart(a, b)? {
                    continue;
                }
                if let Some(both) = kernel::boolean(a, b, kernel::BoolOp::Intersect)?
                    && kernel::measure(&both)?.volume > 1e-3
                {
                    return err(
                        "the fingers of a joint on a bend stand in another wall of the body: the body is too thin there for a joint on that bend (let it turn on its film)",
                    );
                }
            }
        }
        let mut out = Vec::with_capacity(flat.parts.len());
        for part in &flat.parts {
            let mut pieces = Vec::with_capacity(part.faces.len() * 2);
            for &f in &part.faces {
                let (Some(m), Some(Some(wall))) = (flat.moves.get(f), walls.get(f)) else { continue };
                pieces.push(kernel::transform_matrix(wall, *m)?);
            }
            // The film under each bend of the part: as wide as the groove is at the film's top
            // (a slit's width at an inside corner), short of the bend's ends by as much.
            for (e, edge) in self.edges.iter().enumerate() {
                let bend = layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e);
                if !(bend && part.faces.contains(&edge.faces.0) && part.faces.contains(&edge.faces.1)) {
                    continue;
                }
                let Some(m) = flat.moves.get(edge.faces.0) else { continue };
                let (p, q) = (apply_point(m, edge.ends.0), apply_point(m, edge.ends.1));
                let (p, q) = (Vec2::new(p.x, p.y), Vec2::new(q.x, q.y));
                let Some(d) = (q - p).normalized() else { continue };
                let half = match edge.turn {
                    EdgeTurn::Outside(angle) => w.film * (angle / 2.0).tan().min(10.0) + w.clearance + 0.05,
                    EdgeTurn::Inside | EdgeTurn::Flat => SLIT / 2.0 + 0.05,
                };
                // Where a wall has its own finger of a joint on the bend, it ends the film and
                // the clearance short of the bend: the film reaches under it there too.
                let jointed = matches!(joints.get(e), Some(Some(Joint { fit: Fit::Knuckle { .. }, .. })));
                let half = if jointed { half.max(w.film + w.clearance + 0.3) } else { half };
                let len = (q - p).len();
                let short = half.min(len / 4.0);
                let strip = Plane { origin: Vec3::new(p.x, p.y, floor.origin.z), x: Vec3::new(d.x, d.y, 0.0), y: Vec3::new(-d.y, d.x, 0.0) };
                pieces.push(block(&strip, (short, len - short), (-half, half), (0.0, w.film))?);
            }
            if pieces.is_empty() {
                return err("a part of the pattern has no faces");
            }
            // All at once, or one after the other where that is refused. Never side by side as
            // loose pieces: a part is one body, or it is not made.
            let refs: Vec<&Body> = pieces.iter().collect();
            let body = match kernel::union_all(&refs) {
                Ok(body) => body,
                Err(_) => {
                    let apart = || DocError::Invalid("the walls of a part of the pattern can't be joined".into());
                    let mut rest = pieces.iter();
                    let mut body = rest.next().cloned().ok_or_else(apart)?;
                    for piece in rest {
                        body = kernel::boolean(&body, piece, kernel::BoolOp::Union)?.ok_or_else(apart)?;
                    }
                    body
                }
            };
            out.push(body);
        }
        Ok(out)
    }
}
