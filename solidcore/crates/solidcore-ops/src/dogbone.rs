//! Dogbone reliefs: the recipe that finds the inside corners of a body a router's round bit
//! cannot cut sharp, and the relief each gets, from the kernel's [`solidcore::dogbone`]
//! primitive (which computes every point, direction and length).
//!
//! [`plan`] reads the body's record: each straight edge picked (or each straight edge along a
//! picked planar face's normal that meets the face, such as the walls of a pocket on its floor)
//! between two planar faces, with the faces' planes, the wall edges beside it and the faces at
//! its ends; the kernel says whether it is an inside corner and, if so, the cylinder that
//! relieves it. A relief runs the edge's length, past an end only where the faces there face on
//! along the edge (open: the top of a pocket or both ends of a slot through the plate), so it
//! never cuts below a blind pocket's floor. The application cuts the cylinders from the body;
//! [`plan`] says which edges it skipped and why. [`faces_at`] says which face a picked point
//! means, on the exact faces.

use solidcore::dogbone as k;
use solidcore::Vec3;
use solidcore_record::brep::{Brep, Curve, Loop, Shell};

use crate::error::status;
use crate::{Body, Error, Order, Result};

pub use solidcore::dogbone::{Options, Side, Skip, Style};

/// How far (radians) an edge may lean from the bit's axis and still run along it.
pub const AXIS_TOLERANCE: f64 = 1.0e-6;

/// Most edges and faces one plan takes.
pub const MAX_PICKS: usize = 10_000;

/// What a body's reliefs are cut with, and which edges get one.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BodyOptions {
    /// The bit and the style.
    pub relief: Options,
    /// The bit's axis: a picked edge not along it is skipped. `None`: any direction. A picked
    /// face's corners are always along its normal.
    pub along: Option<[f64; 3]>,
}

/// Why an edge or a face gets no relief.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Why {
    /// The kernel's reason for the corner ([`Skip`]): convex, too open, ….
    Corner(Skip),
    /// The faces on either side continue each other: no corner.
    Smooth,
    /// The edge is not a straight line.
    NotStraight,
    /// The edge is not between exactly two faces of the body (a sheet's edge, a seam).
    NotTwoFaces,
    /// A face at the edge (or the picked face) is not planar.
    NotPlanar,
    /// The edge does not run along the bit's axis.
    NotAlong,
    /// The picked face has no straight edge along its normal at its corners.
    NoCorners,
}

/// The cylinder a relief is cut with ([`k::ReliefCylinder`]), in the record's points.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Cylinder {
    /// The centre of its base circle.
    pub base: [f64; 3],
    /// Its unit axis, from the base.
    pub axis: [f64; 3],
    /// Its radius: the bit's.
    pub radius: f64,
    /// Its length along the axis.
    pub height: f64,
}

/// A relief to cut: where its edge is and the cylinder.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Tool {
    /// The middle of the record edge relieved.
    pub at: [f64; 3],
    /// The cylinder to cut.
    pub cylinder: Cylinder,
}

/// An edge or a picked face that got no relief.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Skipped {
    /// The record edge, for an edge.
    pub edge: Option<usize>,
    /// The record face, for a picked face with no corners.
    pub face: Option<usize>,
    /// A point on it: an edge's middle, a face's first vertex.
    pub at: [f64; 3],
    /// Why.
    pub why: Why,
}

/// The reliefs of a body: what to cut, and what was skipped.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Plan {
    /// The cylinders to cut, one per relieved edge, in the order the edges were taken.
    pub tools: Vec<Tool>,
    /// The edges and faces skipped, and why.
    pub skipped: Vec<Skipped>,
}

/// The reliefs of record `body` at record edges `edges` and at the corners of record faces
/// `faces`, each edge once.
///
/// # Errors
/// [`Error::Argument`] for an index past the body's edges or faces, more than [`MAX_PICKS`]
/// of either, or options the kernel refuses; the kernel's refusal of a point that is not finite.
pub fn plan(body: &Brep, edges: &[usize], faces: &[usize], options: &BodyOptions) -> Result<Plan> {
    let relief = options.relief.checked().map_err(|_| Error::Argument)?;
    if edges.len() > MAX_PICKS || faces.len() > MAX_PICKS {
        return Err(Error::Argument);
    }
    if edges.iter().any(|e| *e >= body.edges.len()) || faces.iter().any(|f| *f >= body.faces.len())
    {
        return Err(Error::Argument);
    }
    let along = options.along.map(vec3);
    let mut out = Plan::default();
    let mut seen: Vec<usize> = Vec::new();
    for e in edges {
        if seen.contains(e) {
            continue;
        }
        seen.push(*e);
        relieve(body, *e, along, &relief, &mut out)?;
    }
    for f in faces {
        let Some(axis) = face_outward(body, *f)? else {
            out.skipped.push(Skipped {
                edge: None,
                face: Some(*f),
                at: face_point(body, *f),
                why: Why::NotPlanar,
            });
            continue;
        };
        let corners = face_corners(body, *f, axis)?;
        if corners.is_empty() {
            out.skipped.push(Skipped {
                edge: None,
                face: Some(*f),
                at: face_point(body, *f),
                why: Why::NoCorners,
            });
        }
        for e in corners {
            if seen.contains(&e) {
                continue;
            }
            seen.push(e);
            relieve(body, e, Some(axis), &relief, &mut out)?;
        }
    }
    Ok(out)
}

/// The record face of `body` (with `order`, its record's [`Order`]) that each of `points`
/// picks: the face nearest the point as trimmed, on the exact geometry
/// ([`Body::face_distances`]), when it is no farther than `reach`; `None` where no face is that
/// near. Of faces equally near (a point on the edge between two), the first in record order.
///
/// # Errors
/// [`Error::Argument`] for more than [`MAX_PICKS`] points, a point that is not finite, or a
/// `reach` that is not finite or is negative; any other kernel error.
pub fn faces_at(
    body: &Body,
    order: &Order,
    points: &[[f64; 3]],
    reach: f64,
) -> Result<Vec<Option<usize>>> {
    if points.len() > MAX_PICKS || !(reach.is_finite() && reach >= 0.0) {
        return Err(Error::Argument);
    }
    if points.iter().any(|p| !p.iter().all(|x| x.is_finite())) {
        return Err(Error::Argument);
    }
    let mut best: Vec<Option<(f64, usize)>> = vec![None; points.len()];
    if points.is_empty() {
        return Ok(Vec::new());
    }
    for f in 0..order.faces.len() {
        let ds = body.face_distances(order, f, points)?;
        for (b, d) in best.iter_mut().zip(ds) {
            match d {
                Some(d) if d <= reach && b.is_none_or(|(x, _)| d < x) => *b = Some((d, f)),
                _ => {}
            }
        }
    }
    Ok(best.into_iter().map(|b| b.map(|(_, f)| f)).collect())
}

/// The relief of edge `e`, or why none, added to `out`: the record read here, the corner and
/// its cylinder decided by the kernel ([`k::edge_relief`]).
fn relieve(
    body: &Brep,
    e: usize,
    along: Option<Vec3>,
    options: &Options,
    out: &mut Plan,
) -> Result<()> {
    let Some(line) = ends(body, e) else {
        return Err(Error::Argument);
    };
    let at = record_vec3(line.middle());
    let skip = |why: Why, out: &mut Plan| {
        out.skipped.push(Skipped {
            edge: Some(e),
            face: None,
            at,
            why,
        });
    };
    let Some(edge) = body.edges.get(e) else {
        return Err(Error::Argument);
    };
    if !matches!(edge.curve, Curve::Line { .. }) || edge.nurbs.is_some() {
        skip(Why::NotStraight, out);
        return Ok(());
    }
    if let Some(axis) = along {
        if line.lean(axis).map_err(status)? > AXIS_TOLERANCE {
            skip(Why::NotAlong, out);
            return Ok(());
        }
    }
    let [first, second] = match edge.uses.as_slice() {
        [a, b] if a.face != b.face => [a.face as usize, b.face as usize],
        _ => {
            skip(Why::NotTwoFaces, out);
            return Ok(());
        }
    };
    let (sides_a, sides_b) = (wall_sides(body, first, e), wall_sides(body, second, e));
    let (Some(wa), Some(wb)) = (
        wall(body, first, e, &sides_a)?,
        wall(body, second, e, &sides_b)?,
    ) else {
        skip(Why::NotPlanar, out);
        return Ok(());
    };
    let [v0, v1] = edge.vertices;
    let at_start = end_faces(body, v0 as usize, [first, second])?;
    let at_end = end_faces(body, v1 as usize, [first, second])?;
    let at_ends = [at_start.as_slice(), at_end.as_slice()];
    match k::edge_relief(&line, &[wa, wb], at_ends, options).map_err(status)? {
        k::EdgeRelief::Relief(c) => out.tools.push(Tool {
            at,
            cylinder: Cylinder {
                base: record_vec3(c.base),
                axis: record_vec3(c.axis),
                radius: c.radius,
                height: c.height,
            },
        }),
        k::EdgeRelief::Smooth => skip(Why::Smooth, out),
        k::EdgeRelief::Skipped(why) => skip(Why::Corner(why), out),
    }
    Ok(())
}

/// A record point or direction as the kernel's.
fn vec3(p: [f64; 3]) -> Vec3 {
    Vec3::new(p[0], p[1], p[2])
}

/// A kernel point or direction as the record's.
fn record_vec3(v: Vec3) -> [f64; 3] {
    [v.x, v.y, v.z]
}

/// Edge `e` from its start vertex's point to its end vertex's.
fn ends(body: &Brep, e: usize) -> Option<k::Edge> {
    let edge = body.edges.get(e)?;
    let [a, b] = edge.vertices;
    Some(k::Edge {
        start: vec3(body.vertices.get(a as usize)?.point),
        end: vec3(body.vertices.get(b as usize)?.point),
    })
}

/// Face `f` as a wall of edge `e` beside its straight edges `sides` ([`k::Wall`]); `None` when
/// it is not planar or not used once by one shell.
fn wall<'a>(body: &Brep, f: usize, e: usize, sides: &'a [k::Edge]) -> Result<Option<k::Wall<'a>>> {
    let Some(outward) = face_outward(body, f)? else {
        return Ok(None);
    };
    let (Some(shell_forward), Some(loop_forward)) = (shell_sense(body, f), loop_sense(body, f, e))
    else {
        return Ok(None);
    };
    // Loops run counter-clockwise about the natural normal, the face on their left; the outward
    // normal is the natural one when the shell uses the face forward.
    Ok(Some(k::Wall {
        outward,
        left: loop_forward == shell_forward,
        sides,
    }))
}

/// Face `f`'s outward unit normal ([`k::outward`]), if it is planar and one shell uses it once.
fn face_outward(body: &Brep, f: usize) -> Result<Option<Vec3>> {
    let Some(face) = body.faces.get(f) else {
        return Err(Error::Argument);
    };
    if face.nurbs.is_some() {
        return Ok(None);
    }
    let Some(forward) = shell_sense(body, f) else {
        return Ok(None);
    };
    let Ok(surface) = solidcore_convert::surface(&face.surface) else {
        return Ok(None);
    };
    Ok(k::outward(&surface, forward))
}

/// Whether the one shell using face `f` uses it along its natural normal; `None` when no shell,
/// or more than one use, has it.
fn shell_sense(body: &Brep, f: usize) -> Option<bool> {
    let mut found = None;
    for shell in &body.shells {
        if let Shell::Faces { faces, .. } = shell {
            for of in faces.iter().filter(|of| of.face as usize == f) {
                if found.is_some() {
                    return None;
                }
                found = Some(of.forward);
            }
        }
    }
    found
}

/// Whether face `f`'s loop runs along edge `e` from its start to its end; `None` when the face
/// doesn't use it, or uses it twice (a seam).
fn loop_sense(body: &Brep, f: usize, e: usize) -> Option<bool> {
    let face = body.faces.get(f)?;
    let mut found = None;
    for lp in &face.loops {
        if let Loop::Edges { edges } = lp {
            for oe in edges.iter().filter(|oe| oe.edge as usize == e) {
                if found.is_some() {
                    return None;
                }
                found = Some(oe.forward);
            }
        }
    }
    found
}

/// The straight edges of face `f`'s loops, other than `e`, that meet `e` at a vertex (the wall's
/// edges along the floor and the top): the kernel takes the wall's length beside `e` from them.
fn wall_sides(body: &Brep, f: usize, e: usize) -> Vec<k::Edge> {
    let mut out = Vec::new();
    let (Some(edge), Some(face)) = (body.edges.get(e), body.faces.get(f)) else {
        return out;
    };
    for lp in &face.loops {
        let Loop::Edges { edges } = lp else { continue };
        for oe in edges.iter().filter(|oe| oe.edge as usize != e) {
            let Some(other) = body.edges.get(oe.edge as usize) else {
                continue;
            };
            let meets = other.vertices.iter().any(|v| edge.vertices.contains(v));
            if !meets || !matches!(other.curve, Curve::Line { .. }) {
                continue;
            }
            if let Some(side) = ends(body, oe.edge as usize) {
                out.push(side);
            }
        }
    }
    out
}

/// The outward normals of the faces at vertex `v` other than `walls` (`None` for a face that is
/// not planar): the kernel reads from them whether a relief runs on past `v` (open) or stops
/// there (blind, such as a pocket's floor).
fn end_faces(body: &Brep, v: usize, walls: [usize; 2]) -> Result<Vec<Option<Vec3>>> {
    let mut out = Vec::new();
    for f in faces_at_vertex(body, v) {
        if !walls.contains(&f) {
            out.push(face_outward(body, f)?);
        }
    }
    Ok(out)
}

/// The faces with an edge ending at vertex `v`, each once.
fn faces_at_vertex(body: &Brep, v: usize) -> Vec<usize> {
    let mut out = Vec::new();
    for (i, face) in body.faces.iter().enumerate() {
        let touches = face.loops.iter().any(|lp| match lp {
            Loop::Edges { edges } => edges.iter().any(|oe| {
                body.edges
                    .get(oe.edge as usize)
                    .is_some_and(|ed| ed.vertices.iter().any(|x| *x as usize == v))
            }),
            Loop::Vertex { vertex } => *vertex as usize == v,
        });
        if touches {
            out.push(i);
        }
    }
    out
}

/// The straight edges along `axis` with a vertex on face `f`, not on it: the corners of a
/// pocket's walls on its floor, or of a slot's through a plate's face.
fn face_corners(body: &Brep, f: usize, axis: Vec3) -> Result<Vec<usize>> {
    let Some(face) = body.faces.get(f) else {
        return Err(Error::Argument);
    };
    let mut own: Vec<u32> = Vec::new();
    let mut vertices: Vec<u32> = Vec::new();
    for lp in &face.loops {
        match lp {
            Loop::Edges { edges } => {
                for oe in edges {
                    own.push(oe.edge);
                    if let Some(ed) = body.edges.get(oe.edge as usize) {
                        vertices.extend(ed.vertices);
                    }
                }
            }
            Loop::Vertex { vertex } => vertices.push(*vertex),
        }
    }
    let mut out = Vec::new();
    for (i, ed) in body.edges.iter().enumerate() {
        if own.iter().any(|o| *o as usize == i) || !ed.vertices.iter().any(|v| vertices.contains(v))
        {
            continue;
        }
        if !matches!(ed.curve, Curve::Line { .. }) || ed.nurbs.is_some() {
            continue;
        }
        let Some(line) = ends(body, i) else {
            continue;
        };
        if line.lean(axis).map_err(status)? <= AXIS_TOLERANCE {
            out.push(i);
        }
    }
    Ok(out)
}

/// A point of face `f` to name it by: its first vertex's.
fn face_point(body: &Brep, f: usize) -> [f64; 3] {
    let first = body.faces.get(f).and_then(|face| {
        face.loops.iter().find_map(|lp| match lp {
            Loop::Edges { edges } => edges
                .first()
                .and_then(|oe| body.edges.get(oe.edge as usize))
                .map(|ed| {
                    let [first, _] = ed.vertices;
                    first
                }),
            Loop::Vertex { vertex } => Some(*vertex),
        })
    });
    first
        .and_then(|v| body.vertices.get(v as usize))
        .map_or([0.0; 3], |v| v.point)
}

#[cfg(test)]
#[path = "dogbone_tests.rs"]
mod tests;
