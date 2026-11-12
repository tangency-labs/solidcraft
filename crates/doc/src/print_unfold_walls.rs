//! Unfold for Printing, generator 2: the printable parts made from the body itself. Every face
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
//! the body's shell, so they can't stand in a wall either. A bend is never a press fit, and the
//! ends of a cut stay mitred, so where three faces meet every wall ends as it does without.

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, Body};

use crate::print_unfold::{EdgeKind, EdgeTurn, UnfoldEdge, UnfoldFace, UnfoldGraph, UnfoldLayout};
use crate::print_unfold_flat::FlatLayout;
use crate::print_unfold_group::{SLIT, socket_possible};
use crate::sheet::{CRUMB, DEFAULT_NUB_HEIGHT, DEFAULT_NUB_RADIUS, MIN_FINGER_WIDTH, PressFit};
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
pub const PRESS_MIN_WALL: f64 = 1.2;

/// The walls of generator 2: their thickness, the film a bend turns on, and the clearance
/// between two walls where they meet (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WallPrint {
    pub wall: f64,
    pub film: f64,
    pub clearance: f64,
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
    if w.wall >= PRESS_MIN_WALL { fingers_between(keep, len - keep) } else { Vec::new() }
}

/// How far from each end of a cut its fingers stay (mm).
fn press_keep(w: WallPrint) -> f64 {
    w.wall + w.clearance + 1.5
}

/// The stops of the fingers that fill a cut from `lo` to `hi` along it; none where that is
/// too short for one.
fn fingers_between(lo: f64, hi: f64) -> Vec<f64> {
    let room = hi - lo;
    if !(room.is_finite() && room >= 4.0) {
        return Vec::new();
    }
    let most = (room / MIN_FINGER_WIDTH).floor();
    let count = if most >= 1.0 { (most as usize).min(PRESS_MOST) } else { 1 };
    (0..=count).map(|k| lo + room * k as f64 / count as f64).collect()
}

/// Whether a cut that turns by `turn` and is `len` long can be a press fit: its faces meet square
/// round the outside of the body, and it has room for a finger.
pub fn press_possible(turn: EdgeTurn, len: f64, w: WallPrint) -> bool {
    socket_possible(turn) && !press_fingers(len, w).is_empty()
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
    /// its fingers where a cut is a press fit (`stops`, [`Self::press_stops`]), their nubs and
    /// dimples on them.
    fn wall(&self, f: usize, layout: &UnfoldLayout, splits: &[usize], w: WallPrint, stops: &[Vec<f64>]) -> Result<Body> {
        let Some(face) = self.faces.get(f) else { return err("a face of the pattern is not a face of the body") };
        let (t, c) = (w.wall, w.clearance);
        let over = (t * 0.05).max(1e-3);
        let n = face.normal();
        let radius = DEFAULT_NUB_RADIUS.min(t / 2.0 - 0.3);
        let fit = PressFit { radius, height: DEFAULT_NUB_HEIGHT, tolerance: 0.0 };
        let mut wall = kernel::extrude_planar_regions(&face.plane, &[face.region()], -t, 0.0)?;
        let (mut tools, mut caps): (Vec<Body>, Vec<(bool, Body)>) = (Vec::new(), Vec::new());
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
                    let stops = stops.get(e).map_or(&[][..], Vec::as_slice);
                    let (Some(&lo), Some(&hi), Some(into_other)) = (stops.first(), stops.last(), into_face(other, edge)) else {
                        tools.push(mitre(0.0, len)?);
                        continue;
                    };
                    // A press fit. The fingers are dealt out by turns from the first face; the
                    // ends of the cut stay mitred (a little into where this wall ends short
                    // anyway, so no skin is left between the two cuts).
                    let count = stops.len() - 1;
                    let own = |j: usize| j.is_multiple_of(2) == (edge.faces.0 == f);
                    let half = c / 2.0;
                    tools.push(mitre(0.0, if own(0) { lo + half } else { lo - half + over })?);
                    tools.push(mitre(if own(count - 1) { hi - half } else { hi + half - over }, len)?);
                    // The other face's plane: `into_other` in it, and which way is out of the body.
                    let flat = Plane { origin: edge.ends.0, x: d, y: into_other };
                    let up = d.cross(into_other).dot(other.normal()) > 0.0;
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
                    // A nub where a finger ends, in the middle of the corner the walls share: on
                    // the first face's finger where two fingers meet. What stands next to it
                    // (the other's finger, or its mitred end) gets the dimple.
                    if fit.radius < fit.height {
                        continue;
                    }
                    let (ball, back) = (fit.ball(), fit.ball() - fit.height);
                    let middle = inward * (t / 2.0 / (angle / 2.0).cos());
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
                        let (u0, u1) = (at - dir * eps, at + dir * (fit.height + eps));
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
        let small = || DocError::Invalid("a face is too small for its wall: nothing of it is left between the faces it meets".into());
        for tool in &tools {
            wall = kernel::boolean(&wall, tool, kernel::BoolOp::Cut)?.ok_or_else(small)?;
        }
        // The dimples first, then what the cuts left in one piece, then its nubs on it.
        for (_, cap) in caps.iter().filter(|(mine, _)| !mine) {
            wall = kernel::boolean(&wall, cap, kernel::BoolOp::Cut)?.ok_or_else(small)?;
        }
        let mut wall = whole(wall)?;
        for (_, cap) in caps.iter().filter(|(mine, _)| *mine) {
            wall = kernel::boolean(&wall, cap, kernel::BoolOp::Union)?.ok_or_else(small)?;
        }
        Ok(wall)
    }

    /// Where the fingers of each edge begin and end along it ([`press_fingers`]); none for an
    /// edge that is no press fit. A finger keeps its wall up to the face's outline, so a cut has
    /// none where another wall of the same part lies beside it in the flat (a wall folded out
    /// next to it, at an inside corner of a face's outline): there the two would print as one
    /// and the bend beside them could not turn. The cut stays mitred along that stretch.
    fn press_stops(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: WallPrint, kinds: &[String]) -> Vec<Vec<f64>> {
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
            let (false, true, true, true, Some(d)) =
                (bend, kinds.get(e).is_some_and(|k| k == "press"), socket_possible(edge.turn), w.wall >= PRESS_MIN_WALL && room >= 4.0, d)
            else {
                out.push(Vec::new());
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
            out.push(match best {
                Some((a, b)) => {
                    fingers_between(if a == 0 { keep } else { at(a) + BESIDE_CLEAR }, if b == count { len - keep } else { at(b) - BESIDE_CLEAR })
                }
                None => Vec::new(),
            });
        }
        out
    }

    /// Whether each edge of the body gets the fingers of a press fit, as [`Self::wall_solids`]
    /// makes them from the same `kinds`: not every cut asked to be one has room for them.
    pub fn press_edges(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: WallPrint, kinds: &[String]) -> Vec<bool> {
        self.press_stops(layout, flat, splits, w, kinds).iter().map(|stops| !stops.is_empty()).collect()
    }

    /// The printable body of each part of the flat pattern `flat` (split at `splits`), in the
    /// order of its parts: the walls of the part's faces turned flat, and a film under each of
    /// its bends. `kinds`: the joint of every edge of the body; a cut that is `press` gets
    /// fingers where it can ([`press_possible`]), every other cut is mitred.
    pub fn wall_solids(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: WallPrint, kinds: &[String]) -> Result<Vec<Body>> {
        if !(w.wall.is_finite() && w.film.is_finite() && w.clearance.is_finite() && w.film > 0.0 && w.wall > w.film && w.clearance >= 0.0) {
            return err("the wall must be thicker than the film a bend turns on");
        }
        if w.wall < PRESS_MIN_WALL && kinds.iter().any(|k| k == "press") {
            return err(format!("a press fit needs a wall at least {PRESS_MIN_WALL} mm thick"));
        }
        let Some(base) = self.faces.get(layout.base) else { return err("the base face is not a face of the body") };
        let floor = Plane { origin: Vec3::new(0.0, 0.0, base.plane.origin.z), x: Vec3::X, y: Vec3::Y };
        let stops = self.press_stops(layout, flat, splits, w, kinds);
        let mut out = Vec::with_capacity(flat.parts.len());
        for part in &flat.parts {
            let mut pieces = Vec::with_capacity(part.faces.len() * 2);
            for &f in &part.faces {
                let Some(m) = flat.moves.get(f) else { continue };
                pieces.push(kernel::transform_matrix(&self.wall(f, layout, splits, w, &stops)?, *m)?);
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
