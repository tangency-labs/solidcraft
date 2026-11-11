//! Unfold for Printing: the printable body of a carved-out group, made from its faces in the
//! flat. Like every part, the group lies with its outer skin on the print bed, its wall above.
//! Its bends are inside corners of the body, so they turn the other way from the rest of the
//! print: toward the outer skin. No groove is needed for that, the walls move apart at the back
//! as they fold. A slit [`SLIT`] wide is left open over each bend, down to the film the bend
//! turns on.
//!
//! At the border of the group the faces outside it run on to the edge with their wall. What a
//! face of the group does there follows the joint of that edge:
//! - `butt`: it ends short of the edge, by the wall of the face it meets and the clearance;
//! - `mitre`: it ends on the plane halfway between the two faces, the clearance short of it;
//! - `press` and `filament`: it ends as a butt joint does, and fingers of it stand on to the
//!   edge through the other face, which has a socket for each ([`socket_fingers`]). A press fit
//!   has a nub on both end faces of every finger, a filament joint a channel through them.

use solidcore_record::{Loop2, Plane, Region2, Vec2, Vec3};
use solidcraft_geom::prelude::*;
use solidcraft_kernel::{self as kernel, Body};

use crate::print_unfold::{EdgeKind, EdgeTurn, UnfoldGraph, UnfoldLayout};
use crate::print_unfold_flat::FlatLayout;
use crate::sheet::{DEFAULT_CHANNEL_TOLERANCE, DEFAULT_FILAMENT, DEFAULT_NUB_HEIGHT, DEFAULT_NUB_RADIUS, PressFit, socket_fingers};
use crate::{DocError, Result, apply_point};

/// The slit left open over a bend of a carved-out group (mm).
pub const SLIT: f64 = 0.6;

/// The wall of a carved-out group: its thickness, the film its bends turn on, and the clearance
/// to the faces it meets (mm).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GroupWall {
    pub wall: f64,
    pub film: f64,
    pub clearance: f64,
}

fn err<T>(msg: impl Into<String>) -> Result<T> {
    Err(DocError::Invalid(msg.into()))
}

fn area(pts: &[Vec2]) -> f64 {
    let n = pts.len();
    (0..n).filter_map(|i| Some(pts.get(i)?.cross(*pts.get((i + 1) % n)?))).sum::<f64>() / 2.0
}

/// The polygon `pts` with edge `i` (from corner `i` to the next) moved in by `by[i]`; none
/// where nothing of it is left.
fn inset(pts: &[Vec2], by: &[f64]) -> Option<Vec<Vec2>> {
    let n = pts.len();
    if n < 3 || by.len() != n {
        return None;
    }
    let sign = if area(pts) >= 0.0 { 1.0 } else { -1.0 };
    // Each edge's line, moved in: a point on it and its direction.
    let line = |i: usize| -> Option<(Vec2, Vec2)> {
        let (a, b) = (*pts.get(i)?, *pts.get((i + 1) % n)?);
        let d = (b - a).normalized()?;
        let inward = Vec2::new(-d.y, d.x) * sign;
        Some((a + inward * *by.get(i)?, d))
    };
    let mut out = Vec::with_capacity(n);
    for i in 0..n {
        let ((p, d), (q, e)) = (line((i + n - 1) % n)?, line(i)?);
        let cross = d.cross(e);
        if cross.abs() < 1e-9 {
            out.push(q);
            continue;
        }
        let s = (q - p).cross(e) / cross;
        out.push(p + d * s);
    }
    let left = area(&out) * sign;
    (left > 1e-6 && left <= area(pts).abs() + 1e-6).then_some(out)
}

/// Whether a border edge that turns by `turn` can take sockets: its two faces meet square round
/// the outside of the body.
pub fn socket_possible(turn: EdgeTurn) -> bool {
    matches!(turn, EdgeTurn::Outside(angle) if (angle - std::f64::consts::FRAC_PI_2).abs() < 0.17)
}

/// An edge of a face in the flat: from `at` along `d`, `n` into the face, at height `z` (the
/// outer skin).
#[derive(Clone, Copy)]
struct FlatEdge {
    at: Vec2,
    d: Vec2,
    n: Vec2,
    len: f64,
    z: f64,
}

impl FlatEdge {
    fn point(&self, along: f64, across: f64) -> Vec2 {
        self.at + self.d * along + self.n * across
    }

    /// A box along the edge: `u` along it, `a` across (into the face), `h` above the outer skin.
    fn block(&self, u: (f64, f64), a: (f64, f64), h: (f64, f64)) -> Result<Body> {
        let outer = Loop2::polygon(&[self.point(u.0, a.0), self.point(u.1, a.0), self.point(u.1, a.1), self.point(u.0, a.1)]).ccw();
        let plane = Plane { origin: Vec3::new(0.0, 0.0, self.z), x: Vec3::X, y: Vec3::Y };
        Ok(kernel::extrude_planar_regions(&plane, &[Region2 { outer, holes: Vec::new() }], h.0, h.1)?)
    }
}

impl UnfoldGraph {
    /// The printable body of each carved-out group of `layout`, laid flat as `flat` has it
    /// (split at `splits`), in the order of the groups' parts. `kinds`: the joint of every edge
    /// of the body (`butt`, `mitre`, `press`, `filament`; a bend's is not read).
    pub fn group_solids(&self, layout: &UnfoldLayout, flat: &FlatLayout, splits: &[usize], w: GroupWall, kinds: &[String]) -> Result<Vec<Body>> {
        if !(w.wall.is_finite() && w.film.is_finite() && w.clearance.is_finite() && w.film > 0.0 && w.wall > w.film && w.clearance >= 0.0) {
            return err("the wall of a carved-out group must be thicker than its film");
        }
        let Some(base) = self.faces.get(layout.base) else { return err("the base face is not a face of the body") };
        let z = base.plane.origin.z;
        let fit = PressFit { radius: DEFAULT_NUB_RADIUS, height: DEFAULT_NUB_HEIGHT, tolerance: 0.0 };
        let (t, c) = (w.wall, w.clearance);
        let over = (t * 0.05).max(1e-3);
        let mut out = Vec::new();
        for part in flat.parts.iter().filter(|p| p.group.is_some()) {
            let mut pieces = Vec::new();
            for &f in &part.faces {
                let (Some(face), Some(m)) = (self.faces.get(f), flat.moves.get(f)) else { continue };
                let to_flat = |p: Vec3| {
                    let q = apply_point(m, p);
                    Vec2::new(q.x, q.y)
                };
                let loops: Vec<Vec<Vec2>> = face.loops.iter().map(|lp| lp.iter().map(|p| to_flat(*p)).collect()).collect();
                let Some(outer) = loops.first() else { continue };
                let n = outer.len();
                let middle = outer.iter().fold(Vec2::default(), |s, p| s + *p) * (1.0 / n.max(1) as f64);
                // How far each edge of the outline ends short of where the face ends: under the
                // film's top and above it. Then what stands on it, and what is cut from it.
                let (mut low, mut high) = (Vec::with_capacity(n), Vec::with_capacity(n));
                let (mut fingers, mut cuts, mut nubs): (Vec<Body>, Vec<Body>, Vec<Body>) = (Vec::new(), Vec::new(), Vec::new());
                for i in 0..n {
                    let edge = face.edges.get(i).and_then(|e| Some((*e, self.edges.get(*e)?)));
                    let Some((e, edge)) = edge else {
                        low.push(0.0);
                        high.push(0.0);
                        continue;
                    };
                    if layout.kinds.get(e) == Some(&EdgeKind::Bend) && !splits.contains(&e) {
                        low.push(0.0);
                        high.push(SLIT / 2.0);
                        continue;
                    }
                    let (EdgeTurn::Outside(angle), true) = (edge.turn, layout.group.get(edge.across(f)) != layout.group.get(f)) else {
                        low.push(0.0);
                        high.push(0.0);
                        continue;
                    };
                    let butt = t / angle.sin().max(1.0 / 3.0) + c;
                    let kind = kinds.get(e).map_or("butt", String::as_str);
                    // The edge in the flat, along the body's edge from its first end.
                    let (p, q) = (to_flat(edge.ends.0), to_flat(edge.ends.1));
                    let flat_edge = (q - p).normalized().map(|d| {
                        let side = Vec2::new(-d.y, d.x);
                        FlatEdge { at: p, d, n: if side.dot(middle - p) >= 0.0 { side } else { side * -1.0 }, len: (q - p).len(), z }
                    });
                    match (kind, flat_edge) {
                        ("mitre", Some(fe)) => {
                            // From the clearance at the outer skin to the wall's width more at the inner one.
                            let half = (std::f64::consts::PI - angle) / 2.0;
                            let (at, extra) = (c / half.sin().max(0.1), t / half.tan().max(0.1));
                            let lean = extra / t;
                            let far = t + extra + 1.0;
                            let profile = Loop2::polygon(&[
                                Vec2::new(-far, -1.0),
                                Vec2::new(at - lean, -1.0),
                                Vec2::new(at + extra + lean, t + 1.0),
                                Vec2::new(-far, t + 1.0),
                            ])
                            .ccw();
                            let mid = fe.point(fe.len / 2.0, 0.0);
                            let plane = Plane { origin: Vec3::new(mid.x, mid.y, z), x: Vec3::new(fe.n.x, fe.n.y, 0.0), y: Vec3::Z };
                            let reach = fe.len / 2.0 + far;
                            cuts.push(kernel::extrude_planar_regions(&plane, &[Region2 { outer: profile, holes: Vec::new() }], -reach, reach)?);
                            low.push(0.0);
                            high.push(0.0);
                        }
                        ("press" | "filament", Some(fe)) if socket_possible(edge.turn) => {
                            for (s, end) in socket_fingers(fe.len) {
                                let (u0, u1) = (s + c / 2.0, end - c / 2.0);
                                fingers.push(fe.block((u0, u1), (0.0, butt + over), (0.0, t))?);
                                if kind != "press" {
                                    continue;
                                }
                                // A nub on both end faces, in the middle of the corner the two walls share.
                                let (ball, back) = (fit.ball(), fit.ball() - fit.height);
                                for (face_at, dir) in [(u0, -1.0), (u1, 1.0)] {
                                    let centre = fe.point(face_at - dir * back, t / 2.0);
                                    let (a, b) = (face_at - dir * over, face_at + dir * (fit.height + over));
                                    let wide = ball + over;
                                    let cap = fe.block((a.min(b), a.max(b)), (t / 2.0 - wide, t / 2.0 + wide), (t / 2.0 - wide, t / 2.0 + wide))?;
                                    let nub = kernel::boolean(
                                        &kernel::sphere(Vec3::new(centre.x, centre.y, z + t / 2.0), ball)?,
                                        &cap,
                                        kernel::BoolOp::Intersect,
                                    )?
                                    .ok_or_else(|| DocError::Invalid("a press-fit nub of a carved-out group came out empty".into()))?;
                                    nubs.push(nub);
                                }
                            }
                            if kind == "filament" {
                                let side = (DEFAULT_FILAMENT + DEFAULT_CHANNEL_TOLERANCE) / 2.0;
                                cuts.push(fe.block((-1.0, fe.len + 1.0), (t / 2.0 - side, t / 2.0 + side), (t / 2.0 - side, t / 2.0 + side))?);
                            }
                            low.push(butt);
                            high.push(butt);
                        }
                        _ => {
                            low.push(butt);
                            high.push(butt);
                        }
                    }
                }
                let holes: Vec<Loop2> = loops.iter().skip(1).map(|lp| Loop2::polygon(lp)).collect();
                let small =
                    || DocError::Invalid("a carved-out face is too small for its wall: nothing of it is left between the faces it meets".into());
                let plane = Plane { origin: Vec3::new(0.0, 0.0, z), x: Vec3::X, y: Vec3::Y };
                let mut solids = Vec::with_capacity(2 + fingers.len());
                for (by, z0, z1) in [(&low, 0.0, w.film), (&high, w.film, t)] {
                    let pts = inset(outer, by).ok_or_else(small)?;
                    let region = Region2 { outer: Loop2::polygon(&pts), holes: holes.clone() };
                    solids.push(kernel::extrude_planar_regions(&plane, &[region], z0, z1)?);
                }
                solids.extend(fingers);
                for mut solid in solids {
                    let mut left = true;
                    for cut in &cuts {
                        match kernel::boolean(&solid, cut, kernel::BoolOp::Cut)? {
                            Some(rest) => solid = rest,
                            None => {
                                left = false;
                                break;
                            }
                        }
                    }
                    if left {
                        pieces.push(solid);
                    }
                }
                pieces.extend(nubs);
            }
            if pieces.is_empty() {
                return err("a carved-out group has no faces");
            }
            let refs: Vec<&Body> = pieces.iter().collect();
            let body = match kernel::union_all(&refs) {
                Ok(body) => body,
                Err(_) => kernel::join_apart(&pieces)?.ok_or_else(|| DocError::Invalid("the walls of a carved-out group can't be joined".into()))?,
            };
            out.push(body);
        }
        Ok(out)
    }
}
