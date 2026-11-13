//! Turning recognized surfaces into a plan a parametric design can be rebuilt from.
//!
//! [`recognize_mesh`](crate::recognize_mesh) says what surfaces a mesh stands for. This turns those
//! surfaces into the shape of a prismatic part: the two end faces give a sketch plane and a
//! thickness, the walls between them give a closed profile, the outer rounds give its corner
//! radii, and the cylinders through it give the bores and the bosses. The caller builds that with
//! the ordinary sketch and feature commands, so what comes out is a design, not a frozen body.
//!
//! Everything here is decided from the certified surfaces, never from the triangles: a wall is a
//! plane with an origin and a normal, and the profile is where those planes cross each other. The
//! mesh has already been left behind by the time a plan is made.
//!
//! What it will not do is guess. The profile is the intersection of the walls' half-planes, which
//! is convex, so every profile edge is then measured against the area of the face it came from: an
//! outline that turns back on itself, such as an L, gives edges far shorter than their own faces
//! and is refused with what did not add up, rather than rebuilt as a smaller rectangle that looks
//! plausible. Every surface the plan does not use is reported in `unused` with the reason, and
//! `accounted` says how much of the mesh's area the plan explains, so a caller can tell a complete
//! rebuild from a first approximation.

use solidcore_record::Vec3;
use solidcraft_geom::prelude::*;

use crate::mesh_recognize::{Recognition, Surface};
use crate::{KernelError, Result};

/// Walls one plan reads. A profile needs a handful; the limit is only there to bound the clipping.
const MAX_WALLS: usize = 512;
/// Round surfaces about the build axis that one plan reads.
const MAX_ROUNDS: usize = 512;
/// Surfaces listed in `unused`, largest area first.
const MAX_UNUSED: usize = 64;
/// Corners a profile may have.
const MAX_CORNERS: usize = 1024;
/// How much of the thickness a plane must reach across to be a wall of the outer profile, rather
/// than the wall of a step or a pocket.
const WALL_SPAN: f64 = 0.6;
/// How far a profile edge's length may disagree with the area of the face it came from before the
/// profile is refused as not being this part's outline.
const EDGE_AGREEMENT: std::ops::RangeInclusive<f64> = 0.5..=1.5;

fn invalid(message: &str) -> KernelError {
    KernelError::Invalid(format!("mesh rebuild: {message}"))
}

/// One corner of a profile, in the sketch's own coordinates (mm).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Corner {
    pub at: [f64; 2],
    /// The radius the corner is rounded by, or zero where it is sharp.
    pub radius: f64,
}

/// The closed outline the part is extruded from.
#[derive(Debug, Clone, PartialEq)]
pub enum Outline {
    /// A round part: one cylinder is the whole outline.
    Circle { centre: [f64; 2], radius: f64 },
    /// The corners in order, turning the way the sketch's own axes do.
    Polygon(Vec<Corner>),
}

/// A cylindrical hole through or into the part.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Bore {
    pub centre: [f64; 2],
    pub radius: f64,
    /// Whether it goes all the way through.
    pub through: bool,
    /// How deep it goes, for one that does not.
    pub depth: f64,
    /// Which face it is drilled from: the far end along the build axis, or the near one.
    pub from_top: bool,
    /// How far along the build axis the face it is drilled from sits, measured from the sketch
    /// plane. The far end face, unless a boss stands over the bore, in which case the top of that
    /// boss: that is where the material starts once the boss has been built, and `depth` is
    /// measured from there.
    pub at: f64,
}

/// A cylinder standing on the far end face.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Boss {
    pub centre: [f64; 2],
    pub radius: f64,
    pub height: f64,
}

/// A surface the plan does not account for, and why.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Unused {
    pub surface: &'static str,
    pub area: f64,
    pub reason: &'static str,
}

/// How to rebuild a prismatic part: a sketch plane, a profile, a thickness, and what stands on or
/// goes through it. Lengths are mm.
#[derive(Debug, Clone, PartialEq)]
pub struct Prismatic {
    /// Where the sketch sits: on the near end face, at that face's own centre.
    pub origin: Vec3,
    /// The direction the part is extruded along, away from `origin`.
    pub axis: Vec3,
    /// The sketch's own x and y directions, right-handed about `axis`.
    pub x_dir: Vec3,
    pub y_dir: Vec3,
    /// The distance between the two end faces.
    pub thickness: f64,
    pub outline: Outline,
    pub bores: Vec<Bore>,
    pub bosses: Vec<Boss>,
    /// The share of the mesh's area the plan's own surfaces cover, from 0 to 1.
    pub accounted: f64,
    pub unused: Vec<Unused>,
}

/// A wall of the profile: the material lies where `p . normal <= offset`, in sketch coordinates.
/// `area` is the area of every recognized face on this same line, which is what the profile edge
/// along it is checked against.
#[derive(Debug, Clone, Copy)]
struct Wall {
    normal: [f64; 2],
    offset: f64,
    area: f64,
}

/// A cylinder turning about the build axis, in sketch coordinates.
#[derive(Debug, Clone, Copy)]
struct Round {
    centre: [f64; 2],
    radius: f64,
    /// Where it reaches along the build axis, measured from the sketch plane.
    lo: f64,
    hi: f64,
    /// Whether its triangles face away from its axis: a boss rather than a bore.
    outward: bool,
    area: f64,
}

/// Plan the rebuild of a prismatic part from what a mesh was recognized as.
///
/// `axis` is the direction to extrude along; without one the plan takes the direction with the
/// most recognized area square to it, which is the one with end faces to extrude between.
/// `align` is how far (radians) a surface's own axis may lean from the build axis and still count
/// as along it or square to it, which on a scan has to cover the part's own skew.
pub fn prismatic_plan(found: &Recognition, axis: Option<Vec3>, align: f64) -> Result<Prismatic> {
    crate::guard("plan a prismatic rebuild", || {
        if !align.is_finite() || !(0.001..=0.3).contains(&align) {
            return Err(invalid("align must be between 0.0573 and 17.19 degrees"));
        }
        let axis = match axis {
            Some(given) => given.normalized().ok_or_else(|| invalid("the build axis must not be zero length"))?,
            None => {
                found.axes.iter().filter(|c| c.across > 0.).max_by(|a, b| a.across.total_cmp(&b.across)).map(|c| c.direction).ok_or_else(|| {
                    invalid(
                        "no direction with faces square to it, so there is nothing to extrude between: give the axis, or recognize at a \
                         larger tolerance",
                    )
                })?
            }
        };
        plan(found, axis, align)
    })
}

fn plan(found: &Recognition, axis: Vec3, align: f64) -> Result<Prismatic> {
    let parallel = align.cos();
    let square = align.sin();
    let tol = found.tolerance.max(1e-9);
    let (base, top) = ends(found, axis, parallel)?;
    let thickness = (top - base).dot(axis);
    if !(thickness > tol) {
        return Err(invalid("the two end faces are the same face, or the far one is behind the near one: give the other axis direction"));
    }
    let (origin, x_dir, y_dir) = frame(found, base, axis, square);

    let mut walls: Vec<Wall> = Vec::new();
    let mut rounds: Vec<Round> = Vec::new();
    let mut unused: Vec<Unused> = Vec::new();
    // Faces square to the axis that are neither end face, as (height, area).
    let mut parallels: Vec<(f64, f64)> = Vec::new();
    let mut used = 0.;
    let flat = |p: Vec3| [(p - origin).dot(x_dir), (p - origin).dot(y_dir)];
    for patch in &found.patches {
        match patch.surface {
            Surface::Plane { origin: at, normal } => {
                let facing = normal.dot(axis);
                if facing.abs() >= parallel {
                    // An end face, or another face parallel to them: the top of a boss, the floor
                    // of a blind bore, the floor of a pocket. Which it is only becomes clear once
                    // the bosses and the bores are known, so it waits for them below.
                    let height = (at - origin).dot(axis);
                    if height.abs() <= tol * 4. || (height - thickness).abs() <= tol * 4. {
                        used += patch.area;
                    } else {
                        parallels.push((height, patch.area));
                    }
                    continue;
                }
                if facing.abs() > square {
                    unused.push(Unused {
                        surface: "plane",
                        area: patch.area,
                        reason: "a sloped face, neither along the build axis nor square to it",
                    });
                    continue;
                }
                if reach(patch.bounds, axis) < thickness * WALL_SPAN {
                    unused.push(Unused {
                        surface: "plane",
                        area: patch.area,
                        reason: "a wall that does not run the whole thickness: the side of a step or a pocket",
                    });
                    continue;
                }
                let Some(n) = (normal - axis * facing).normalized() else { continue };
                let wall = Wall { normal: [n.dot(x_dir), n.dot(y_dir)], offset: (at - origin).dot(n), area: patch.area };
                // Faces of one wall that recognition found separately are one wall again here, so
                // that the edge check below measures the profile against the whole face.
                match walls.iter().position(|had| same_line(had, &wall, parallel, tol)) {
                    Some(at) => {
                        if let Some(had) = walls.get_mut(at) {
                            had.area += wall.area;
                        }
                    }
                    None if walls.len() < MAX_WALLS => walls.push(wall),
                    None => {
                        unused.push(Unused { surface: "plane", area: patch.area, reason: "more walls than one profile reads" });
                        continue;
                    }
                }
                used += patch.area;
            }
            Surface::Cylinder { origin: at, axis: own, radius } => {
                let facing = own.dot(axis);
                if facing.abs() < parallel {
                    unused.push(Unused { surface: "cylinder", area: patch.area, reason: "a cylinder that does not turn about the build axis" });
                    continue;
                }
                let (Some(lo), Some(hi)) = (patch.along.map(|a| a[0]), patch.along.map(|a| a[1])) else {
                    unused.push(Unused { surface: "cylinder", area: patch.area, reason: "a cylinder of unknown extent" });
                    continue;
                };
                if rounds.len() >= MAX_ROUNDS {
                    unused.push(Unused { surface: "cylinder", area: patch.area, reason: "more cylinders than one plan reads" });
                    continue;
                }
                let sign = if facing >= 0. { 1. } else { -1. };
                let height = (at - origin).dot(axis);
                let (a, b) = (height + lo * sign, height + hi * sign);
                rounds.push(Round { centre: flat(at), radius, lo: a.min(b), hi: a.max(b), outward: patch.outward.unwrap_or(true), area: patch.area });
            }
            Surface::Cone { .. } => {
                unused.push(Unused { surface: "cone", area: patch.area, reason: "a cone: a draft, a countersink or a chamfer, to add by hand" })
            }
            Surface::Sphere { .. } => unused.push(Unused { surface: "sphere", area: patch.area, reason: "a sphere: to add by hand" }),
            Surface::Torus { .. } => {
                unused.push(Unused { surface: "torus", area: patch.area, reason: "a round between two faces: fillet that edge by hand" })
            }
            Surface::Freeform => {
                unused.push(Unused { surface: "freeform", area: patch.area, reason: "nothing analytic certified here: to model by hand" })
            }
        }
    }
    walls.sort_by(|a, b| b.area.total_cmp(&a.area));

    let mut corners = clip(&walls, &rounds, tol)?;
    let outline = if corners.is_empty() {
        let (centre, radius, area) = sole_circle(&rounds, thickness, tol).ok_or_else(|| {
            invalid(
                "the walls do not close a profile: fewer than three faces run the whole thickness and no single cylinder surrounds the \
                 part. Recognize at a larger tolerance, or give another axis",
            )
        })?;
        used += area;
        Outline::Circle { centre, radius }
    } else {
        round_corners(&mut corners, &walls, &rounds, tol, &mut used, &mut unused);
        agrees(&corners, &walls, thickness, tol)?;
        Outline::Polygon(corners)
    };

    let (bores, bosses) = openings(&rounds, &outline, &walls, thickness, tol, &mut used, &mut unused);
    // The faces parallel to the ends, now that it is clear what stands on the part and what goes
    // into it: the top of a boss and the floor of a blind bore come with the feature that makes
    // them, and what is left is a step or a pocket the rebuild does not account for.
    let slack = tol * 4.;
    for (height, area) in parallels {
        let on_boss = bosses.iter().any(|boss| (height - thickness - boss.height).abs() <= slack);
        let on_bore = bores.iter().any(|bore| !bore.through && (height - (bore.at - bore.depth)).abs() <= slack);
        if on_boss || on_bore {
            used += area;
        } else if height > thickness {
            unused.push(Unused { surface: "plane", area, reason: "a face past the far end that no boss accounts for" });
        } else {
            unused.push(Unused { surface: "plane", area, reason: "a face between the ends: the floor of a step or a pocket" });
        }
    }
    unused.sort_by(|a, b| b.area.total_cmp(&a.area));
    unused.truncate(MAX_UNUSED);
    let accounted = if found.area > 0. { (used / found.area).clamp(0., 1.) } else { 0. };
    Ok(Prismatic { origin, axis, x_dir, y_dir, thickness, outline, bores, bosses, accounted, unused })
}

/// The two end faces: the largest plane facing each way along the axis. The largest, and not the
/// outermost, because the top of a boss and the floor of a counterbore are faces square to the
/// axis too, and either would otherwise set the thickness.
fn ends(found: &Recognition, axis: Vec3, parallel: f64) -> Result<(Vec3, Vec3)> {
    let (mut near, mut far): (Option<(f64, Vec3)>, Option<(f64, Vec3)>) = (None, None);
    for patch in &found.patches {
        let Surface::Plane { origin, normal } = patch.surface else { continue };
        let facing = normal.dot(axis);
        let slot = if facing <= -parallel {
            &mut near
        } else if facing >= parallel {
            &mut far
        } else {
            continue;
        };
        if slot.is_none_or(|(area, _)| patch.area > area) {
            *slot = Some((patch.area, origin));
        }
    }
    match (near, far) {
        (Some((_, base)), Some((_, top))) => Ok((base, top)),
        _ => Err(invalid(&format!(
            "no pair of end faces square to [{:.3}, {:.3}, {:.3}]: a prismatic rebuild needs a face at each end of the extrusion. Give \
             another axis, widen align, or recognize at a larger tolerance",
            axis.x, axis.y, axis.z
        ))),
    }
}

/// How far a patch's bounding box reaches along a direction. Exact where the box's own axes line up
/// with it, and an upper bound otherwise, so a wall is never dropped for being too short when it
/// is only leaning.
fn reach(bounds: Option<[Vec3; 2]>, axis: Vec3) -> f64 {
    let Some([lo, hi]) = bounds else { return f64::INFINITY };
    let size = hi - lo;
    size.x * axis.x.abs() + size.y * axis.y.abs() + size.z * axis.z.abs()
}

/// Whether two walls are faces of the same line of the profile.
fn same_line(a: &Wall, b: &Wall, parallel: f64, tol: f64) -> bool {
    a.normal[0] * b.normal[0] + a.normal[1] * b.normal[1] >= parallel && (a.offset - b.offset).abs() <= tol * 4.
}

/// The sketch's placement: its origin on the near end face, and its x and y directions. The
/// largest wall becomes the profile's bottom edge, so a part that is square to itself comes out
/// square to the sketch however it was turned in the scan.
fn frame(found: &Recognition, base: Vec3, axis: Vec3, square: f64) -> (Vec3, Vec3, Vec3) {
    let mut best: Option<(f64, Vec3)> = None;
    for patch in &found.patches {
        let Surface::Plane { normal, .. } = patch.surface else { continue };
        if normal.dot(axis).abs() > square {
            continue;
        }
        if best.is_none_or(|(area, _)| patch.area > area) {
            best = Some((patch.area, normal));
        }
    }
    let lead = best.map(|(_, n)| n).unwrap_or(if axis.x.abs() < 0.9 { Vec3::X } else { Vec3::Y });
    // A wall's outward normal points away from the material, so -normal is the sketch's +y and
    // that wall lies along the bottom of the profile.
    let Some(y) = (lead - axis * lead.dot(axis)).normalized().map(|n| -n) else {
        return (base, Vec3::X, Vec3::Y);
    };
    (base, y.cross(axis).normalized().unwrap_or(Vec3::X), y)
}

/// The convex profile the walls cut out, in order. Empty when there are fewer than three walls.
fn clip(walls: &[Wall], rounds: &[Round], tol: f64) -> Result<Vec<Corner>> {
    if walls.len() < 3 {
        return Ok(Vec::new());
    }
    // A square certain to hold the part, from how far the walls and the cylinders reach.
    let mut far = 1f64;
    for wall in walls {
        far = far.max(wall.offset.abs());
    }
    for round in rounds {
        far = far.max(round.centre[0].abs() + round.radius).max(round.centre[1].abs() + round.radius);
    }
    far = (far * 4.).min(1e9);
    let mut poly = vec![[-far, -far], [far, -far], [far, far], [-far, far]];
    let eps = tol * 1e-3 + 1e-12;
    for wall in walls.iter().take(MAX_WALLS) {
        poly = cut(&poly, *wall, eps);
        if poly.len() < 3 {
            return Err(invalid("the walls leave no profile: two of them face each other with no room between"));
        }
    }
    // A corner still out on the bounding square means the walls never closed: a wall of the part
    // was not recognized, or it is open on that side.
    let edge = far * 0.999;
    if poly.iter().any(|p| p[0].abs() >= edge || p[1].abs() >= edge) {
        return Err(invalid(
            "the walls do not close a profile: the outline runs off to infinity on at least one side, so a wall of the part was not \
             recognized. Recognize at a larger tolerance, or sketch the outline by hand from the patches",
        ));
    }
    let mut out: Vec<Corner> = Vec::new();
    for p in poly.iter().take(MAX_CORNERS) {
        if out.last().is_some_and(|last: &Corner| near(last.at, *p, tol)) {
            continue;
        }
        out.push(Corner { at: *p, radius: 0. });
    }
    if out.len() > 2 && out.first().zip(out.last()).is_some_and(|(a, b)| near(a.at, b.at, tol)) {
        out.pop();
    }
    if out.len() < 3 {
        return Err(invalid("the walls leave no profile: they meet in fewer than three corners"));
    }
    Ok(out)
}

fn near(a: [f64; 2], b: [f64; 2], tol: f64) -> bool {
    (a[0] - b[0]).abs() <= tol && (a[1] - b[1]).abs() <= tol
}

/// Keep the part of a convex polygon on the material side of one wall.
fn cut(poly: &[[f64; 2]], wall: Wall, eps: f64) -> Vec<[f64; 2]> {
    let side = |p: &[f64; 2]| p[0] * wall.normal[0] + p[1] * wall.normal[1] - wall.offset;
    let mut out = Vec::with_capacity(poly.len() + 1);
    for i in 0..poly.len() {
        let (Some(a), Some(b)) = (poly.get(i), poly.get((i + 1) % poly.len())) else { continue };
        let (da, db) = (side(a), side(b));
        if da <= eps {
            out.push(*a);
        }
        if (da > eps && db < -eps) || (da < -eps && db > eps) {
            let t = da / (da - db);
            if t.is_finite() {
                out.push([a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t]);
            }
        }
    }
    out
}

/// Each profile edge against the face it came from: the straight run between two rounded corners,
/// times the thickness, is the area that face must have. An outline that turns back on itself,
/// such as an L or a T, clips to a smaller convex profile whose edges are far shorter than their
/// own faces, and that is what this catches.
fn agrees(corners: &[Corner], walls: &[Wall], thickness: f64, tol: f64) -> Result<()> {
    let fit = tol.max(1e-6) * 4.;
    for i in 0..corners.len() {
        let (Some(a), Some(b)) = (corners.get(i), corners.get((i + 1) % corners.len())) else { continue };
        let length = (b.at[0] - a.at[0]).hypot(b.at[1] - a.at[1]) - a.radius - b.radius;
        let expected = length * thickness;
        if expected <= fit * thickness {
            continue;
        }
        let Some(wall) = wall_through(walls, a.at, b.at, fit) else {
            return Err(invalid(&format!(
                "the profile has an edge {length:.3} mm long that no recognized face lies on, so this is not the part's outline"
            )));
        };
        if !EDGE_AGREEMENT.contains(&(wall.area / expected)) {
            return Err(invalid(&format!(
                "the profile does not match the part: its edge along one face is {length:.3} mm long, which needs {expected:.1} mm2 of \
                 face, and that face has {:.1} mm2. An outline that turns back on itself, such as an L or a T, looks like this to a \
                 convex profile: rebuild the part in pieces, or sketch the outline by hand from the patches",
                wall.area
            )));
        }
    }
    Ok(())
}

/// One cylinder that is the whole outline of a round part: it runs most of the part's length and
/// nothing reaches outside it.
fn sole_circle(rounds: &[Round], thickness: f64, tol: f64) -> Option<([f64; 2], f64, f64)> {
    let span = tol.max(thickness * 0.5);
    let mut best: Option<&Round> = None;
    for round in rounds {
        if !round.outward || round.hi - round.lo < span {
            continue;
        }
        if best.is_none_or(|b| round.radius > b.radius) {
            best = Some(round);
        }
    }
    let outer = best?;
    let outside = rounds.iter().any(|r| (r.centre[0] - outer.centre[0]).hypot(r.centre[1] - outer.centre[1]) + r.radius > outer.radius + tol * 4.);
    (!outside).then_some((outer.centre, outer.radius, outer.area))
}

/// Give each corner the radius of the cylinder that rounds it: the one tangent to both of the
/// walls that meet there.
fn round_corners(corners: &mut [Corner], walls: &[Wall], rounds: &[Round], tol: f64, used: &mut f64, unused: &mut Vec<Unused>) {
    let fit = tol.max(1e-6) * 4.;
    let mut taken = vec![false; rounds.len()];
    let count = corners.len();
    for i in 0..count {
        let Some(corner) = corners.get(i).copied() else { continue };
        let (Some(prev), Some(next)) = (corners.get((i + count - 1) % count), corners.get((i + 1) % count)) else { continue };
        let (Some(a), Some(b)) = (wall_through(walls, prev.at, corner.at, fit), wall_through(walls, corner.at, next.at, fit)) else {
            continue;
        };
        for (j, round) in rounds.iter().enumerate() {
            if !round.outward || taken.get(j) == Some(&true) || !(round.radius > 0.) {
                continue;
            }
            let from = |wall: &Wall| wall.offset - (round.centre[0] * wall.normal[0] + round.centre[1] * wall.normal[1]);
            if (from(&a) - round.radius).abs() > fit || (from(&b) - round.radius).abs() > fit {
                continue;
            }
            if let Some(slot) = corners.get_mut(i) {
                slot.radius = round.radius;
            }
            if let Some(slot) = taken.get_mut(j) {
                *slot = true;
            }
            *used += round.area;
            break;
        }
    }
    for (j, round) in rounds.iter().enumerate() {
        if round.outward && taken.get(j) != Some(&true) && on_outline(round, corners) {
            unused.push(Unused { surface: "cylinder", area: round.area, reason: "an outer round that is not tangent to two walls" });
        }
    }
}

/// Whether a round sits out on the outline rather than inside it, so that leaving it out is worth
/// reporting.
fn on_outline(round: &Round, corners: &[Corner]) -> bool {
    corners.iter().any(|c| (c.at[0] - round.centre[0]).hypot(c.at[1] - round.centre[1]) <= round.radius * 2. + 1e-9)
}

/// The wall whose line holds both ends of one profile edge.
fn wall_through(walls: &[Wall], a: [f64; 2], b: [f64; 2], fit: f64) -> Option<Wall> {
    let mut best: Option<(f64, Wall)> = None;
    for wall in walls {
        let off = |p: [f64; 2]| (p[0] * wall.normal[0] + p[1] * wall.normal[1] - wall.offset).abs();
        let miss = off(a).max(off(b));
        if miss > fit {
            continue;
        }
        if best.is_none_or(|(had, _)| miss < had) {
            best = Some((miss, *wall));
        }
    }
    best.map(|(_, wall)| wall)
}

/// The bores through the profile and the bosses standing on it.
fn openings(
    rounds: &[Round],
    outline: &Outline,
    walls: &[Wall],
    thickness: f64,
    tol: f64,
    used: &mut f64,
    unused: &mut Vec<Unused>,
) -> (Vec<Bore>, Vec<Boss>) {
    let (mut bores, mut bosses): (Vec<(f64, f64, Bore)>, Vec<Boss>) = (Vec::new(), Vec::new());
    let slack = tol.max(1e-6) * 4.;
    for round in rounds {
        let inside = match outline {
            Outline::Circle { centre, radius } => {
                let away = (round.centre[0] - centre[0]).hypot(round.centre[1] - centre[1]);
                away + round.radius <= radius + slack && round.radius < radius - slack
            }
            // A corner round's own cylinder is part of the outline, not an opening in it: its
            // centre sits within its own radius of the corner it rounds.
            Outline::Polygon(corners) => {
                let outline = corners.iter().any(|c| {
                    (c.radius - round.radius).abs() <= slack
                        && (c.at[0] - round.centre[0]).hypot(c.at[1] - round.centre[1]) <= round.radius * 1.5 + slack
                });
                !outline
                    && walls.iter().all(|w| round.centre[0] * w.normal[0] + round.centre[1] * w.normal[1] - w.offset < -round.radius * 0.5 - slack)
            }
        };
        if !inside {
            continue;
        }
        if round.outward {
            if round.hi > thickness + slack {
                bosses.push(Boss { centre: round.centre, radius: round.radius, height: round.hi - thickness.max(round.lo) });
                *used += round.area;
            } else {
                unused.push(Unused { surface: "cylinder", area: round.area, reason: "a round inside the profile that stands on nothing" });
            }
            continue;
        }
        let through = round.lo <= slack && round.hi >= thickness - slack;
        if !through && round.lo > slack && round.hi < thickness - slack {
            unused.push(Unused { surface: "cylinder", area: round.area, reason: "a bore that reaches neither end face" });
            continue;
        }
        bores.push((round.lo, round.hi, Bore { centre: round.centre, radius: round.radius, through, depth: 0., from_top: true, at: 0. }));
        *used += round.area;
    }
    bosses.sort_by(|a, b| b.radius.total_cmp(&a.radius));
    // Where each bore is drilled from, now that the bosses are known: a bore under a boss starts
    // at the top of that boss, because that is where the material starts and where the face the
    // hole is drilled into will be once the boss has been built.
    let mut out = Vec::with_capacity(bores.len());
    for (lo, hi, mut bore) in bores {
        bore.from_top = bore.through || hi >= thickness - slack;
        let under = bosses
            .iter()
            .filter(|boss| (boss.centre[0] - bore.centre[0]).hypot(boss.centre[1] - bore.centre[1]) + bore.radius <= boss.radius + slack)
            .map(|boss| boss.height)
            .fold(0., f64::max);
        bore.at = if bore.from_top { thickness + under } else { 0. };
        bore.depth = if bore.from_top { (bore.at - lo).clamp(0., bore.at) } else { hi.max(0.) };
        out.push(bore);
    }
    out.sort_by(|a, b| b.radius.total_cmp(&a.radius));
    (out, bosses)
}

#[cfg(test)]
#[path = "mesh_rebuild_tests.rs"]
mod tests;
