//! Flat sides a body can be laid on: the planes of its planar faces, with how much of the body
//! touches each one ([`flat_sides`]). Arrange lays a body on the side it touches most of all
//! those it rests on whole; whether the rest of the body stays on one side of the plane is the
//! caller's to check on the exact body.
//!
//! The contact area of a side is worked out from the exact boundary curves of its faces (Green's
//! theorem on each loop, curves integrated by Gauss–Legendre between their breaks); faces that
//! lie in one plane add up. No mesh plays a part.
//!
//! A level face can also be the sheet things are laid out on ([`level_face_at`]).
//!
//! Moved from SolidCraft's `crates/geom/src/rest.rs` onto the kernel's own body ([`Body`]): the
//! hidden-line body's faces and curves, with the side its shell uses each face on.

use crate::arrange::Shape2;
use crate::eval::Surface;
use crate::hlr::body::{Brep, Loop};
use crate::hlr::curve::C3;
use crate::math::{Vec2, Vec3};

/// A body as its flat sides are read from it: its faces and curves, and whether its shell uses
/// each face along the face's natural normal (`true` where missing).
#[derive(Clone, Copy, Debug)]
pub struct Body<'a> {
    /// The faces, edges and vertices.
    pub brep: &'a Brep,
    /// Per face: whether the shell uses it along its surface's natural normal.
    pub forward: &'a [bool],
}

/// Most flat sides reported, largest first.
pub const MAX_SIDES: usize = 64;

/// A plane the body has planar faces in.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FlatSide {
    /// Out of the material, unit length.
    pub normal: Vec3,
    /// `normal · p` for a point `p` of the plane.
    pub offset: f64,
    /// The area of the body's faces in the plane, mm².
    pub area: f64,
    /// The direction of the longest straight edge of those faces, unit length, if they have one:
    /// what a laid-down body is squared up to.
    pub edge: Option<Vec3>,
}

/// The planes of `body`'s planar faces, largest contact area first (at most [`MAX_SIDES`]). A
/// face whose area can't be worked out is left out.
#[must_use]
pub fn flat_sides(body: Body<'_>) -> Vec<FlatSide> {
    let mut sides = all_sides(body);
    sides.truncate(MAX_SIDES);
    sides
}

/// The side of `body` whose face `at` is on, within `tol` of its plane and inside its outer
/// loop, outside its holes: what a picked face means. Every side is looked at, not only the
/// largest.
#[must_use]
pub fn side_at(body: Body<'_>, at: Vec3, tol: f64) -> Option<FlatSide> {
    for (i, face) in body.brep.faces.iter().enumerate() {
        let Some((o, u, v)) = plane_of(&face.surface) else {
            continue;
        };
        let Some(axis) = u.cross(v).unitize().ok() else {
            continue;
        };
        if axis.dot(at - o).abs() > tol {
            continue;
        }
        let Some(e1) = u.unitize().ok() else { continue };
        let e2 = axis.cross(e1);
        let Some(rings) = face_rings(body, i, o, e1, e2) else {
            continue;
        };
        if !inside(&rings.shape, Vec2::new((at - o).dot(e1), (at - o).dot(e2))) {
            continue;
        }
        let Some(signed) = face_area(body, i, axis) else {
            continue;
        };
        let natural = if signed > 0.0 { axis } else { axis * -1.0 };
        let normal = if body.forward.get(i).copied().unwrap_or(true) {
            natural
        } else {
            natural * -1.0
        };
        let offset = normal.dot(o);
        let same = 1e-6 * size(body).max(1.0);
        return all_sides(body)
            .into_iter()
            .find(|f| f.normal.dot(normal) > 1.0 - 1e-12 && (f.offset - offset).abs() <= same);
    }
    None
}

fn plane_of(surface: &Surface) -> Option<(Vec3, Vec3, Vec3)> {
    match surface {
        Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => Some((v3(*origin), v3(*u_axis), v3(*v_axis))),
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => {
            let o = v3(*origin);
            Some((o, v3(*u_point) - o, v3(*v_point) - o))
        }
        _ => None,
    }
}

fn inside(shape: &Shape2, p: Vec2) -> bool {
    winds(&shape.outer, p) && !shape.holes.iter().any(|h| winds(h, p))
}

/// The flat sides found so far, one per plane: the outward normal, its offset, the area, and the
/// longest straight edge with its length.
struct Side {
    normal: Vec3,
    offset: f64,
    area: f64,
    edge: Option<Vec3>,
    longest: f64,
}

fn all_sides(body: Body<'_>) -> Vec<FlatSide> {
    let tol = 1e-9 * size(body).max(1.0);
    let mut sides: Vec<Side> = Vec::new();
    for (i, face) in body.brep.faces.iter().enumerate() {
        let Some((o, u, v)) = plane_of(&face.surface) else {
            continue;
        };
        let Some(axis) = u.cross(v).unitize().ok() else {
            continue;
        };
        // The loops run counter-clockwise about the face's natural normal, so their signed area
        // about `axis` says which way that normal points, whatever form the plane is held in.
        let Some(signed) = face_area(body, i, axis) else {
            continue;
        };
        let area = signed.abs();
        if !(area.is_finite() && area > 0.0) {
            continue;
        }
        let natural = if signed > 0.0 { axis } else { axis * -1.0 };
        let normal = if body.forward.get(i).copied().unwrap_or(true) {
            natural
        } else {
            natural * -1.0
        };
        let offset = normal.dot(o);
        let edge = longest_line(body, i);
        match sides
            .iter_mut()
            .find(|s| s.normal.dot(normal) > 1.0 - 1e-12 && (s.offset - offset).abs() <= tol * 1e3)
        {
            Some(s) => {
                s.area += area;
                s.longest = s.longest.max(edge.map_or(0.0, |e| e.1));
                if edge.is_some_and(|e| e.1 >= s.longest) {
                    s.edge = edge.map(|e| e.0);
                }
            }
            None => sides.push(Side {
                normal,
                offset,
                area,
                edge: edge.map(|e| e.0),
                longest: edge.map_or(0.0, |e| e.1),
            }),
        }
    }
    sides.sort_by(|a, b| b.area.total_cmp(&a.area));
    sides
        .into_iter()
        .map(|s| FlatSide {
            normal: s.normal,
            offset: s.offset,
            area: s.area,
            edge: s.edge,
        })
        .collect()
}

/// The longest line edge of face `i`: its direction and length.
fn longest_line(body: Body<'_>, i: usize) -> Option<(Vec3, f64)> {
    let mut best: Option<(Vec3, f64)> = None;
    for lp in &body.brep.faces.get(i)?.loops {
        let Loop::Edges { edges } = lp else { continue };
        for oe in edges {
            let Some(edge) = body.brep.edges.get(oe.edge as usize) else {
                continue;
            };
            let Ok((C3::Line { a, b }, _)) = C3::of_edge(edge) else {
                continue;
            };
            let len = (b - a).length();
            if let Some(d) = (b - a)
                .unitize()
                .ok()
                .filter(|_| best.is_none_or(|(_, l)| len > l))
            {
                best = Some((d, len));
            }
        }
    }
    best
}

fn v3(p: [f64; 3]) -> Vec3 {
    Vec3::from_array(p)
}

/// The largest coordinate of any vertex of `body`: the scale its tolerances are taken at.
fn size(body: Body<'_>) -> f64 {
    body.brep.vertices.iter().fold(0.0f64, |m, v| {
        m.max(v.point.iter().fold(0.0f64, |a, x| a.max(x.abs())))
    })
}

/// The signed area of planar face `i` seen along `n`: the record keeps the outer loop
/// counter-clockwise and the inner ones clockwise about the face's natural normal, so the loops'
/// signed areas add up to the face's, positive when `n` is that normal.
fn face_area(body: Body<'_>, i: usize, n: Vec3) -> Option<f64> {
    let face = body.brep.faces.get(i)?;
    let mut twice = 0.0;
    for lp in &face.loops {
        let Loop::Edges { edges } = lp else { continue };
        for oe in edges {
            let edge = body.brep.edges.get(oe.edge as usize)?;
            // The curve runs from the edge's start vertex to its end (its interval does).
            let (c, _) = C3::of_edge(edge).ok()?;
            let sign = if oe.forward { 1.0 } else { -1.0 };
            twice += sign * swept(&c, n)?;
        }
    }
    Some(twice * 0.5)
}

/// The five-point Gauss–Legendre nodes on `[-1, 1]`.
const X: [f64; 5] = [
    -0.906_179_845_938_664,
    -0.538_469_310_105_683,
    0.0,
    0.538_469_310_105_683,
    0.906_179_845_938_664,
];
/// The weights of [`X`].
const W: [f64; 5] = [
    0.236_926_885_056_189,
    0.478_628_670_499_366,
    0.568_888_888_888_889,
    0.478_628_670_499_366,
    0.236_926_885_056_189,
];

/// `∫ n · (p × p') dt` over the curve: twice the signed area it sweeps about the origin, seen
/// along `n`.
fn swept(c: &C3, n: Vec3) -> Option<f64> {
    if let C3::Line { a, b } = c {
        return Some(n.dot(a.cross(*b)));
    }
    // Five-point Gauss–Legendre on each piece between breaks, each piece split in eight.
    let mut breaks = c.breaks();
    breaks.sort_by(f64::total_cmp);
    breaks.dedup();
    if breaks.len() > 10_000 {
        return None;
    }
    let mut sum = 0.0;
    for w in breaks.windows(2) {
        let &[a, b] = w else { continue };
        for k in 0..8 {
            let (lo, hi) = (
                a + (b - a) * f64::from(k) / 8.0,
                a + (b - a) * f64::from(k + 1) / 8.0,
            );
            let (mid, half) = ((lo + hi) * 0.5, (hi - lo) * 0.5);
            for (x, wt) in X.iter().zip(W) {
                let t = mid + half * x;
                sum += wt * half * n.dot(c.at(t).cross(c.d1(t)));
            }
        }
    }
    sum.is_finite().then_some(sum)
}

/// A level planar face seen from above: its outer ring and its holes, flattened, and how high it
/// lies.
#[derive(Clone, Debug, PartialEq)]
pub struct LevelFace {
    /// The outer ring and holes, in world X and Y.
    pub shape: Shape2,
    /// The face's height, mm.
    pub z: f64,
    /// How far the flattened rings may stray from the true boundary, mm.
    pub error: f64,
}

/// Most corners a flattened face may have.
pub const MAX_FACE_CORNERS: usize = 20_000;

/// The planar face of `body` that `at` lies on, when it is level and faces up (its outward
/// normal +Z within 1e-9) and `at` is on it within `tol`: inside its outer ring and outside its
/// holes. A face looking down is not something to lay parts on.
#[must_use]
pub fn level_face_at(body: Body<'_>, at: Vec3, tol: f64) -> Option<LevelFace> {
    for (i, face) in body.brep.faces.iter().enumerate() {
        let Some((o, u, v)) = plane_of(&face.surface) else {
            continue;
        };
        let Some(n) = u.cross(v).unitize().ok() else {
            continue;
        };
        if n.z.abs() < 1.0 - 1e-9 || (at.z - o.z).abs() > tol {
            continue;
        }
        let Some(signed) = face_area(body, i, n) else {
            continue;
        };
        // The outward normal: the natural one (loops counter-clockwise about it), flipped when
        // the shell uses the face reversed.
        let natural_up = (signed > 0.0) == (n.z > 0.0);
        if natural_up != body.forward.get(i).copied().unwrap_or(true) {
            continue;
        }
        let frame = (
            Vec3::new(0.0, 0.0, o.z),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        let Some(level) = face_rings(body, i, frame.0, frame.1, frame.2) else {
            continue;
        };
        if inside(&level.shape, Vec2::new(at.x, at.y)) {
            return Some(level);
        }
    }
    None
}

/// A planar face of any tilt, in its own frame: `e1`, `e2` along it and `normal` out of the
/// material (right-handed, unit). Its rings are in `(e1 · p, e2 · p)` and it lies at
/// `normal · p = height`.
#[derive(Clone, Debug, PartialEq)]
pub struct FlatFace {
    /// The outer ring and holes, in `(e1 · p, e2 · p)`.
    pub shape: Shape2,
    /// The first axis along the face, unit length.
    pub e1: Vec3,
    /// The second axis along the face, unit length.
    pub e2: Vec3,
    /// Out of the material, unit length.
    pub normal: Vec3,
    /// `normal · p` for a point `p` of the face.
    pub height: f64,
    /// How far the flattened rings may stray from the true boundary, mm.
    pub error: f64,
}

/// The planar face of `body` that `at` lies on, at any tilt: within `tol` of its plane, inside
/// its outer ring and outside its holes. Its frame is squared to the world as far as it can be:
/// `e1` level when the face is not, and X when it is.
#[must_use]
pub fn flat_face_at(body: Body<'_>, at: Vec3, tol: f64) -> Option<FlatFace> {
    for (i, face) in body.brep.faces.iter().enumerate() {
        let Some((o, u, v)) = plane_of(&face.surface) else {
            continue;
        };
        let Some(axis) = u.cross(v).unitize().ok() else {
            continue;
        };
        if axis.dot(at - o).abs() > tol {
            continue;
        }
        let Some(signed) = face_area(body, i, axis) else {
            continue;
        };
        let natural = if signed > 0.0 { axis } else { axis * -1.0 };
        let normal = if body.forward.get(i).copied().unwrap_or(true) {
            natural
        } else {
            natural * -1.0
        };
        let level = Vec3::new(0.0, 0.0, 1.0).cross(normal);
        let e1 = level.unitize().ok().unwrap_or(Vec3::new(1.0, 0.0, 0.0));
        let Some(e2) = normal.cross(e1).unitize().ok() else {
            continue;
        };
        let height = normal.dot(o);
        // From the plane's foot on the normal through the origin, flat coordinates are e1 · p, e2 · p.
        let Some(rings) = face_rings(body, i, normal * height, e1, e2) else {
            continue;
        };
        if inside(&rings.shape, Vec2::new(at.dot(e1), at.dot(e2))) {
            return Some(FlatFace {
                shape: rings.shape,
                e1,
                e2,
                normal,
                height,
                error: rings.error,
            });
        }
    }
    None
}

/// How far a flattened curve may stray from the true one, mm, before it is cut finer.
pub const CHORD: f64 = 0.01;

/// Face `i` flattened in the frame at `o` with axes `e1`, `e2` (unit, square): each loop's
/// curves cut between their breaks until every chord is within [`CHORD`] of the curve, up to 64
/// pieces (lines by their ends); the loop with the largest area is the outer ring. The height
/// is `o`'s.
fn face_rings(body: Body<'_>, i: usize, o: Vec3, e1: Vec3, e2: Vec3) -> Option<LevelFace> {
    let z = o.z;
    let flat = |p: Vec3| Vec2::new((p - o).dot(e1), (p - o).dot(e2));
    let face = body.brep.faces.get(i)?;
    let mut rings: Vec<Vec<Vec2>> = Vec::new();
    let mut error: f64 = 0.0;
    let mut corners = 0usize;
    for lp in &face.loops {
        let Loop::Edges { edges } = lp else { continue };
        let mut ring = Vec::new();
        for oe in edges {
            let edge = body.brep.edges.get(oe.edge as usize)?;
            let (c, _) = C3::of_edge(edge).ok()?;
            let [_, t1] = c.domain();
            let mut ts = c.breaks();
            if !matches!(c, C3::Line { .. }) {
                let sag = |a: f64, b: f64| {
                    let (pa, pb, pm) = (flat(c.at(a)), flat(c.at(b)), flat(c.at((a + b) * 0.5)));
                    (pm.x - (pa.x + pb.x) * 0.5).hypot(pm.y - (pa.y + pb.y) * 0.5)
                };
                let cut =
                    |a: f64, b: f64, n: u32, k: u32| a + (b - a) * f64::from(k) / f64::from(n);
                let mut fine: Vec<f64> = Vec::new();
                for w in ts.windows(2) {
                    let &[a, b] = w else { continue };
                    let mut n = 2u32;
                    while n < 64
                        && (0..n).any(|k| sag(cut(a, b, n, k), cut(a, b, n, k + 1)) > CHORD)
                    {
                        n *= 2;
                    }
                    fine.extend((0..n).map(|k| cut(a, b, n, k)));
                }
                fine.push(t1);
                ts = fine;
            }
            if !oe.forward {
                ts.reverse();
            }
            corners += ts.len();
            if corners > MAX_FACE_CORNERS {
                return None;
            }
            // Each point but the last: the next edge starts there.
            for w in ts.windows(2) {
                let &[t0, t1] = w else { continue };
                let (a, b, m) = (flat(c.at(t0)), flat(c.at(t1)), flat(c.at((t0 + t1) * 0.5)));
                error = error.max((m.x - (a.x + b.x) * 0.5).hypot(m.y - (a.y + b.y) * 0.5));
                ring.push(a);
            }
        }
        if ring.len() >= 3 && ring.iter().all(|p| p.x.is_finite() && p.y.is_finite()) {
            rings.push(ring);
        }
    }
    let areas: Vec<f64> = rings.iter().map(|r| ring_area(r)).collect();
    let outer = areas
        .iter()
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(b.1))
        .map(|(k, _)| k)?;
    let mut holes = Vec::new();
    let mut found = None;
    for (k, r) in rings.into_iter().enumerate() {
        if k == outer {
            found = Some(r);
        } else {
            holes.push(r);
        }
    }
    let outer = found?;
    error.is_finite().then_some(LevelFace {
        shape: Shape2 { outer, holes },
        z,
        error,
    })
}

/// Each side of the closed ring `r`: a corner and the next, the last back to the first.
fn sides(r: &[Vec2]) -> impl Iterator<Item = (Vec2, Vec2)> + '_ {
    r.iter().copied().zip(r.iter().copied().cycle().skip(1))
}

/// The unsigned area of the ring `r`.
fn ring_area(r: &[Vec2]) -> f64 {
    sides(r)
        .map(|(a, b)| a.x * b.y - b.x * a.y)
        .sum::<f64>()
        .abs()
        * 0.5
}

/// Whether `p` is inside the ring (even-odd).
fn winds(ring: &[Vec2], p: Vec2) -> bool {
    let mut inside = false;
    for (a, b) in sides(ring) {
        if (a.y > p.y) != (b.y > p.y) && p.x < a.x + (p.y - a.y) / (b.y - a.y) * (b.x - a.x) {
            inside = !inside;
        }
    }
    inside
}

#[cfg(test)]
mod tests;
