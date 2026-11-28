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
//!
//! A concave edge's fill ends where the faces at the edge's ends, carried on, close it. Where
//! those faces are extrusions along one of the edge's faces' normals (a section extruded, its
//! coves split between a block and an upright on it among them), the part within a slab just
//! behind that face, proved a uniform prism along the normal ([`slab_is_prism`]), moved past
//! the face by its depth is the part with them carried on exactly ([`RunOut::backing_slabs`],
//! tangency-labs/solidcraft#22821).

use crate::blend::carried_face::plane_normal;
use crate::blend::straight_wedge::{straight_wedge, StraightWedge};
use crate::math::det;
use crate::math::{EFF_ZERO, EFF_ZERO_SQRT};
use crate::query::brep::{calculate_bounding_box, BoundingBoxOptions};
use crate::query::face::p1::point_on_face;
use crate::topology::{edge, vertex, EdgeId, VertexId};
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

/// A slab of the part behind one of a concave edge's faces, square to that face's outward
/// normal (tangency-labs/solidcraft#22821): the region over the rectangle `rect` in the plane
/// through `origin` spanned by `x` (the edge) and `y`, between the `levels` along `x × y` (the
/// face's outward normal) from `origin`, just behind the face. Where the part
/// within it is a uniform prism along the normal ([`slab_is_prism`]), the faces at the edge's
/// ends are extrusions along it, and that piece of the part moved by `shift` (the normal times
/// the depth) is the part with those faces carried on past the face, as far as the blend
/// reaches: the fill of the edge's air ends where they close it.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BackingSlab {
    /// The slab's plane's origin: the edge's start, a thousandth of the reach behind the face.
    pub origin: Point3,
    /// Along the edge.
    pub x: Vec3,
    /// Square to the edge and the normal: `x × y` is the face's outward normal.
    pub y: Vec3,
    /// The slab's rectangle in `(x, y)`, counterclockwise: along `x` the prism's extent, along
    /// `y` the prism's length each way.
    pub rect: [[f64; 2]; 4],
    /// The slab's levels along the normal, lower first: its depth behind the plane, twice the
    /// blend's reach so that moved by it the slab covers the blend's section (which lies within
    /// one reach of the edge), and 0.
    pub levels: [f64; 2],
    /// The move that carries the slab past the face: the normal times the depth.
    pub shift: Vec3,
    /// How far a face of the slab may stray from running along the normal, or a cap's vertex
    /// from its level, for [`slab_is_prism`]: a thousand times the plan's tolerance.
    pub limit: f64,
}

impl RunOut {
    /// The slabs behind a concave edge's two faces ([`BackingSlab`]), the first face's first;
    /// none for a convex edge, and none for a face whose normal is not found.
    #[must_use]
    pub fn backing_slabs(&self) -> Vec<BackingSlab> {
        if !self.concave {
            return Vec::new();
        }
        let Some((_, along)) = self.x.cross(self.y).length_and_unit() else {
            return Vec::new();
        };
        // the second face's direction in from the edge, in the section's frame
        let second = match self.section {
            RunOutSection::Triangle(corners) => corners[2],
            RunOutSection::Round { t1, .. } => t1,
        };
        let Some((_, t2)) = (self.x * second[0] + self.y * second[1]).length_and_unit() else {
            return Vec::new();
        };
        // the faces' outward normals, out of the material into the air between them: the
        // first's towards the second face's side, the second's towards the first's
        let mut normals = vec![self.y];
        if let Some((_, n2)) = (self.x - t2 * self.x.dot(t2)).length_and_unit() {
            normals.push(n2);
        }
        let depth = 2.0 * self.reach;
        let gap = self.reach * 1e-3;
        normals
            .into_iter()
            .map(|d| BackingSlab {
                origin: self.origin - d * gap,
                x: along,
                y: d.cross(along),
                rect: {
                    let across = (self.to - self.from).abs();
                    [
                        [self.from, -across],
                        [self.to, -across],
                        [self.to, across],
                        [self.from, across],
                    ]
                },
                levels: [-depth, 0.0],
                shift: d * depth,
                limit: self.tolerance * 1e3,
            })
            .collect()
    }
}

/// Whether `b`, the part within slab `s`, is a uniform prism along the slab's normal: each of
/// its faces runs straight along the normal, within `s.limit` (the kernel's bound on how far it
/// strays across it, [`crate::prism::measure::face_straying`]), or has all its vertices within
/// `s.limit` of the same one of the slab's two levels and lies in a plane square to the normal
/// (a cap). `false` for a body with no faces.
///
/// # Errors
/// Those of the topology walks.
pub fn slab_is_prism(k: &Kernel, b: &Brep, s: &BackingSlab) -> Result<bool> {
    let normal = s.x.cross(s.y);
    let caps = s.levels.map(|level| s.origin + normal * level);
    let faces = b.face_list();
    if faces.is_empty() {
        return Ok(false);
    }
    for &f in faces {
        if crate::prism::measure::face_straying(k, b, f, normal).is_some_and(|x| x <= s.limit) {
            continue;
        }
        // a cap: a plane square to the normal, every vertex at the one level
        let square = plane_normal(k, b, f, s.limit)?
            .is_some_and(|n| n.cross(normal).length() <= EFF_ZERO_SQRT);
        if !square {
            return Ok(false);
        }
        let mut off = [0.0_f64; 2];
        for v in crate::topology::face::p4::vertices(b, f, None, false, None)? {
            let at = vertex::point(b, v)?;
            for (o, &c) in off.iter_mut().zip(&caps) {
                *o = o.max((at - c).dot(normal).abs());
            }
        }
        if off.iter().all(|&o| o > s.limit) {
            return Ok(false);
        }
    }
    Ok(true)
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
    let mut wedges = Vec::new();
    for &e in edges {
        match straight_wedge(k, b, e)? {
            Some(w) => wedges.push(w),
            None => return Ok(None),
        }
    }
    let mut plans = Vec::new();
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
            let half = 0.5 * det::acos(c.clamp(-1.0, 1.0));
            let (sin_h, tan_h) = (det::sin(half), det::tan(half));
            if !(sin_h > 1e-9 && tan_h > 1e-9) {
                return None;
            }
            let t = d / tan_h;
            let centre_at = d / sin_h;
            let centre = [centre_at * bisector.dot(i0), centre_at * bisector.dot(y)];
            let (t0, t1) = ([t, 0.0], [t * c, t * s]);
            let angle = |p: [f64; 2]| det::atan2(p[1] - centre[1], p[0] - centre[0]);
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
    // an edge another edge goes on from tangentially (the section of an earlier round running
    // out along a face of it) is not one a narrow round or cove cuts across: the prism would
    // cross that round in a crease, so it is the executives' to settle or refuse
    for (v, on) in w.vertices.into_iter().zip([w.along * -1.0, w.along]) {
        for other in vertex::edges(b, v, None)? {
            if other == w.edge {
                continue;
            }
            if let Some(out) = leaving_tangent(k, b, other, v)? {
                if out.dot(on) >= TANGENT {
                    return Ok(false);
                }
            }
        }
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

/// The least cosine between an edge's line and another edge leaving its end for [`fits`] to
/// take the other edge as going on from it tangentially: within about half a degree.
const TANGENT: f64 = 0.99996;

/// The unit tangent of `e` at its end on `v`, pointing away from `v`, or `None` where the edge
/// has no curve or its derivative there vanishes.
fn leaving_tangent(k: &Kernel, b: &Brep, e: EdgeId, v: VertexId) -> Result<Option<Vec3>> {
    let Some(curve) = edge::curve(b, e)? else {
        return Ok(None);
    };
    let interval = edge::interval(b, e)?;
    let at_start = edge::start_vertex(b, e)? == Some(v);
    let t = if at_start {
        interval.min()
    } else {
        interval.max()
    };
    let derivatives = curve.evaluate(k, t, 1, true, true)?;
    let Some(&d) = derivatives.get(1) else {
        return Ok(None);
    };
    let d = if at_start { d } else { d * -1.0 };
    let len = d.length();
    if !(len > EFF_ZERO && len.is_finite()) {
        return Ok(None);
    }
    Ok(Some(d * (1.0 / len)))
}

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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::primitive::api_primitives::create_box;

    /// A slab through `origin` along the normal `x × y`, between levels -10 and 0.
    fn slab(origin: Point3, x: Vec3, y: Vec3) -> BackingSlab {
        BackingSlab {
            origin,
            x,
            y,
            rect: [[-50.0, -50.0], [50.0, -50.0], [50.0, 50.0], [-50.0, 50.0]],
            levels: [-10.0, 0.0],
            shift: x.cross(y) * 10.0,
            limit: 1e-6,
        }
    }

    #[test]
    fn a_box_is_a_prism_along_its_edges_and_not_askew() -> Result<()> {
        let mut k = Kernel::default();
        let b = create_box(&mut k, Point3::new(0.0, 0.0, 0.0), 10.0, 10.0, 10.0)?;
        // along z: four walls straight, the top and bottom at the slab's two levels
        let along_z = slab(
            Point3::new(0.0, 0.0, 10.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!(slab_is_prism(&k, &b, &along_z)?);
        // tilted between x and z: the walls stray across it, the caps are not square to it
        let h = 0.5_f64.sqrt();
        let askew = slab(
            Point3::new(10.0, 0.0, 10.0),
            Vec3::new(0.0, 1.0, 0.0),
            Vec3::new(-h, 0.0, h),
        );
        assert!(!slab_is_prism(&k, &b, &askew)?);
        // along z, but levels that miss the box's top and bottom: not this slab's prism
        let shifted = slab(
            Point3::new(0.0, 0.0, 12.0),
            Vec3::new(1.0, 0.0, 0.0),
            Vec3::new(0.0, 1.0, 0.0),
        );
        assert!(!slab_is_prism(&k, &b, &shifted)?);
        Ok(())
    }
}
