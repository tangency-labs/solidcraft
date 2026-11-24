//! The plan of a blend carried on past its edges' ends: for an application that builds an
//! equal chamfer or a constant round the blend executives refuse or fail at an edge ending on
//! a narrow round or cove the blend is wider than, by carrying the blend's section along the
//! whole of the edge's line as a prism and cutting the part's material within it from a convex
//! edge, or adding the air within it to a concave one.
//!
//! Not upstream (solidcore's). [`plan`] measures each edge's wedge ([`straight_wedge`]) and
//! lays out its section square to the edge, the prism's extent along the edge's line, and the
//! points the application finds its pieces and faces by: all the geometry is here, the
//! application only extrudes, intersects, cuts and joins. The section is the triangle of the
//! edge's line and the lines the chamfer's distance in along each face, or for a round the
//! region between the edge's line, the two contact lines (the radius times the cotangent of
//! half the faces' angle in along each face) and the round's arc, whose centre lies on the
//! bisector at the radius over the sine of that half angle. Only an edge one of whose ends
//! meets a curved face, along whose own faces the blend's band fits, gets a plan ([`fits`]):
//! anything else is the executives' to refuse or settle. Two edges whose lines come within
//! their sections' reaches of each other get no plan: their prisms would cut into each other's
//! band, which the blend executives settle and the prisms do not.

use crate::blend::carried_face::plane_normal;
use crate::blend::straight_wedge::{straight_wedge, StraightWedge};
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::query::face::p1::point_on_face;
use crate::topology::{vertex, EdgeId};
use crate::{Brep, Kernel, Point3, Result, Vec3};

/// The blend a run-out carries on.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunOutKind {
    /// An equal chamfer: the distance back along both faces.
    Chamfer,
    /// A constant round: its radius.
    Round,
}

/// A blend's section square to its edge, in the plan's frame (x along [`RunOut::x`], y along
/// [`RunOut::y`], from the edge's start), counterclockwise.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum RunOutSection {
    /// A chamfer's triangle: the edge, then the contact points on the first and second faces.
    Triangle([[f64; 2]; 3]),
    /// A round's section: the arc of `radius` about `centre` from the contact point `t0` on
    /// the first face, at angle `start`, through `sweep` (radians) to the contact point `t1`
    /// on the second face, then the lines through the three points of `behind` back to `t0`:
    /// a strip a tenth of the section's reach deep behind both faces, round the edge. The
    /// strip keeps the section from closing in a knife edge where the arc touches each face,
    /// which the booleans do not settle; it lies in the air beside a convex edge's faces, and
    /// in the part behind a concave edge's, so it adds nothing to the cut or the fill.
    Round {
        /// The contact point on the first face.
        t0: [f64; 2],
        /// The contact point on the second face.
        t1: [f64; 2],
        /// The arc's centre.
        centre: [f64; 2],
        /// The arc's radius.
        radius: f64,
        /// The angle of `t0` about the centre.
        start: f64,
        /// The arc's turn from `t0` to `t1`.
        sweep: f64,
        /// Behind the second face at `t1`, behind both faces at the edge, and behind the first
        /// face at `t0`.
        behind: [[f64; 2]; 3],
    },
}

/// One edge's run-out.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct RunOut {
    /// The section's plane's origin: the edge's start.
    pub origin: Point3,
    /// The section's x direction: into the first face, square to the edge.
    pub x: Vec3,
    /// The section's y direction: square to x and the edge, on the second face's side; x × y
    /// runs along the edge.
    pub y: Vec3,
    /// The section.
    pub section: RunOutSection,
    /// Where the prism starts along x × y from the origin: before the edge's start by more
    /// than the part's size.
    pub from: f64,
    /// Where the prism ends along x × y from the origin: past the edge's end by more than the
    /// part's size.
    pub to: f64,
    /// A point within the section at the edge's middle, halfway from the edge to the blend's
    /// face: the pieces holding it are the blend's.
    pub inside: Point3,
    /// The middle of the blend's face in the section at the edge's middle: the face the blend
    /// makes holds it.
    pub face_point: Point3,
    /// Whether the edge is concave: the section is air to add, not material to cut.
    pub concave: bool,
    /// On each of the edge's faces, a point a hundredth of the section's reach in from the
    /// edge's middle: the faces a concave edge's fill is backed by are found by them.
    pub backs: [Point3; 2],
    /// How far the section reaches from the edge along each face: how far a concave edge's
    /// faces are moved out to back its fill.
    pub reach: f64,
    /// The tolerance the points are found within: a billionth of the part's size.
    pub tolerance: f64,
}

/// The run-outs of `edges` of `b`, `each` the chamfer's distance or the round's radius of each
/// (`kind`): `None` where an edge has no wedge ([`straight_wedge`]), a size is not positive and
/// finite, the faces are too close to flat or to folded for the section, no face at an edge's
/// ends is curved or a face of the edge is too narrow for the blend's contact line ([`fits`]),
/// or two edges' lines come within their sections' reaches of each other.
///
/// # Errors
/// The kernel's errors from the wedges and the bounding box.
pub fn plan(
    k: &Kernel,
    b: &mut Brep,
    edges: &[EdgeId],
    kind: RunOutKind,
    each: &[f64],
) -> Result<Option<Vec<RunOut>>> {
    if edges.len() != each.len() || edges.is_empty() {
        return Ok(None);
    }
    let size = calculate_bounding_box(k, &mut *b, BoundingBoxOptions::default())?
        .size()
        .length();
    if !(size > 0.0 && size.is_finite()) {
        return Ok(None);
    }
    let tolerance = (size * 1e-9).max(1e-9);
    let mut wedges = Vec::with_capacity(edges.len());
    for &e in edges {
        match straight_wedge(k, b, e)? {
            Some(w) => wedges.push(w),
            None => return Ok(None),
        }
    }
    let mut plans = Vec::with_capacity(wedges.len());
    for (w, &d) in wedges.iter().zip(each) {
        let Some(p) = one(w, kind, d, size, tolerance) else {
            return Ok(None);
        };
        if !fits(k, b, w, p.reach, tolerance)? {
            return Ok(None);
        }
        plans.push(p);
    }
    for (i, (a, pa)) in wedges.iter().zip(&plans).enumerate() {
        for (c, pc) in wedges.iter().zip(&plans).skip(i + 1) {
            let apart = line_distance(a.start, a.along, c.start, c.along);
            if apart <= pa.reach + pc.reach + tolerance {
                return Ok(None);
            }
        }
    }
    Ok(Some(plans))
}

/// [`plan`] for one wedge `w`, `d` the distance or radius, `size` the part's.
fn one(w: &StraightWedge, kind: RunOutKind, d: f64, size: f64, tolerance: f64) -> Option<RunOut> {
    if !(d > 0.0 && d.is_finite()) {
        return None;
    }
    // the frame: x into one face, y square to it on the other face's side, x × y along the
    // edge, so the section runs counterclockwise in it: the face whose direction in turns
    // counterclockwise about the edge into the other's is taken first
    let [i0, i1] = if w.along.dot(w.into[0].cross(w.into[1])) < 0.0 {
        [w.into[1], w.into[0]]
    } else {
        w.into
    };
    let y = w.along.cross(i0);
    // the direction into the second face, in the frame
    let (c, s) = (i1.dot(i0), i1.dot(y));
    let bisector = i0 + i1;
    let bis_len = bisector.length();
    if !(bis_len > 0.0 && bis_len.is_finite()) {
        return None;
    }
    let bisector = bisector * (1.0 / bis_len);
    // the section, how far along the bisector from the edge the blend's face lies, and how far
    // the section reaches from the edge along each face
    let (section, face_at, reach) = match kind {
        RunOutKind::Chamfer => {
            let section = RunOutSection::Triangle([[0.0, 0.0], [d, 0.0], [d * c, d * s]]);
            // the chamfer's line between the two contact points crosses the bisector at its
            // middle
            (section, ((i0 + i1) * (d / 2.0)).length(), d)
        }
        RunOutKind::Round => {
            // half the angle between the faces: the angle between the directions in
            let half = 0.5 * c.clamp(-1.0, 1.0).acos();
            let (sin_h, tan_h) = (half.sin(), half.tan());
            if !(sin_h > 1e-9 && tan_h > 1e-9) {
                return None;
            }
            let t = d / tan_h;
            let centre_at = d / sin_h;
            let centre = [centre_at * bisector.dot(i0), centre_at * bisector.dot(y)];
            let (t0, t1) = ([t, 0.0], [t * c, t * s]);
            let angle = |p: [f64; 2]| (p[1] - centre[1]).atan2(p[0] - centre[0]);
            let start = angle(t0);
            // the arc between the contact points that faces the edge: the shorter one, its
            // angle that between the faces' normals
            let mut sweep = angle(t1) - start;
            if sweep > std::f64::consts::PI {
                sweep -= std::f64::consts::TAU;
            } else if sweep < -std::f64::consts::PI {
                sweep += std::f64::consts::TAU;
            }
            // the strip behind the faces: square to each face, away from the wedge between
            // them
            let h = t * 0.1;
            let (m0, m1) = ([0.0, -h], [-s * h, c * h]);
            let behind = [
                [t1[0] + m1[0], t1[1] + m1[1]],
                [m0[0] + m1[0], m0[1] + m1[1]],
                [t0[0] + m0[0], t0[1] + m0[1]],
            ];
            let section = RunOutSection::Round {
                t0,
                t1,
                centre,
                radius: d,
                start,
                sweep,
                behind,
            };
            (section, centre_at - d, t)
        }
    };
    if !(reach.is_finite() && face_at.is_finite() && face_at > 0.0) {
        return None;
    }
    let length = (w.end - w.start).length();
    let past = size + length + reach;
    let mid = (w.start + w.end) * 0.5;
    Some(RunOut {
        origin: w.start,
        x: i0,
        y,
        section,
        from: -past,
        to: length + past,
        inside: mid + bisector * (face_at / 2.0),
        face_point: mid + bisector * face_at,
        concave: w.concave,
        backs: [mid + i0 * (reach * 0.01), mid + i1 * (reach * 0.01)],
        reach,
        tolerance,
    })
}

/// Whether the run-out is the blend `w` asks for: a face at one of the edge's ends that is not
/// one of its two faces is curved (the narrow round or cove the blend is wider than), and each
/// of the edge's two faces holds the blend's contact line, `reach` in from the edge, along the
/// edge from `reach` past its start to `reach` before its end (from its middle on an edge
/// shorter than twice that): the blend's band lies on the edge's own faces, and only what its
/// ends meet is too narrow for it. A face narrower than the band is the blend executives'
/// refusal to give, with the largest size that fits; an edge ending on flat faces only is
/// theirs to settle.
fn fits(k: &Kernel, b: &mut Brep, w: &StraightWedge, reach: f64, tolerance: f64) -> Result<bool> {
    let mut curved_end = false;
    for v in w.vertices {
        for f in vertex::faces(b, v)? {
            if !w.faces.contains(&f) && plane_normal(k, b, f, tolerance)?.is_none() {
                curved_end = true;
            }
        }
    }
    if !curved_end {
        return Ok(false);
    }
    let length = (w.end - w.start).length();
    let inset = reach.min(length * 0.5);
    for (&face, &into) in w.faces.iter().zip(&w.into) {
        for i in 0..=SAMPLES {
            let at = inset + (length - 2.0 * inset) * (f64::from(i) / f64::from(SAMPLES));
            let p = w.start + w.along * at + into * reach;
            if point_on_face(k, b, face, p, tolerance)?.is_none() {
                return Ok(false);
            }
        }
    }
    Ok(true)
}

/// How many steps along the edge [`fits`] tries the contact lines at.
const SAMPLES: u32 = 16;

/// The least distance between the line through `p` along unit `u` and the line through `q`
/// along unit `v`.
fn line_distance(p: Point3, u: Vec3, q: Point3, v: Vec3) -> f64 {
    let between = q - p;
    let normal = u.cross(v);
    let n = normal.length();
    if n > 1e-12 {
        (between.dot(normal) / n).abs()
    } else {
        // parallel: the part of `between` square to the lines
        (between - u * between.dot(u)).length()
    }
}
