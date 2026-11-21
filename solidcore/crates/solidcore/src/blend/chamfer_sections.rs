//! A chamfer edge's two supports' sections at its middle, for its distances as straight
//! setbacks (#13710, phase 1): where every station's sections are congruent to the middle's,
//! the constant offsets that hold the setbacks there hold them all along the edge.
//!
//! Two classes of edge are known to have congruent sections, both read off the supports'
//! natural quadrics ([`BlendQuadric::from_surface`]):
//! - translation: both supports planes or cylinders, each plane holding the edge's
//!   direction and each cylinder's axis along it. The edge is a line, every section the
//!   middle's moved along it.
//! - rim: two quadrics of revolution sharing an axis (`fillet_exact`'s `coaxial`), the edge point off the
//!   axis and the edge square to it. The edge is a circle about the axis, every section the
//!   middle's turned about it.
//!
//! In the plane square to the edge each section is then a line (a plane; a cylinder or cone
//! on a rim) or a circle (a cylinder along the edge; a sphere; a torus's tube on a rim), whose
//! signed curvature is [`ChamferSection::k`]. Anything else, every recogniser's error
//! included, is `None`: the caller keeps today's conversion. Evaluated at the station the
//! wedge is measured at ([`super::fillet_executive`]'s `wedge_at`), at the edge's tolerance.
//!
//! Not upstream (solidcore's). Deviations: none. No recursion.

use crate::blend::fillet_exact::{coaxial, BlendQuadric, QuadricKind};
use crate::surface::procedural_blend::chamfer::ChamferSection;
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, EdgeId, EdgeuseId};
use crate::{Brep, Kernel, Point3, Vec3};

/// The sections of an edge's two supports at its middle, and how to place them.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct EdgeSections {
    /// Supports 1 and 2 (the blend edge use's face, then its radial mate's), in the frame
    /// `origin`, `x`, `y`.
    pub sections: [ChamferSection; 2],
    /// The edge point the sections are at.
    pub origin: Point3,
    /// The frame's first axis: support 1's tangent into its face.
    pub x: Vec3,
    /// The frame's second axis: support 1's normal toward support 2.
    pub y: Vec3,
    /// The angle between the supports' tangents into their faces.
    pub wedge: f64,
    /// A rim's axis (a point of it and its unit direction); `None` for a translation.
    pub axis: Option<(Point3, Vec3)>,
    /// The edge's tolerance.
    pub tol: f64,
}

impl EdgeSections {
    /// The point of the frame's plane at `p`.
    #[must_use]
    pub fn place(&self, p: [f64; 2]) -> Point3 {
        self.origin + self.x * p[0] + self.y * p[1]
    }

    /// Whether `p` stands off a rim's axis by more than the tolerance (always for a
    /// translation).
    #[must_use]
    pub fn off_axis(&self, p: [f64; 2]) -> bool {
        let Some((point, dir)) = self.axis else {
            return true;
        };
        let v = self.place(p) - point;
        (v - dir * v.dot(dir)).length() > self.tol
    }

    /// Whether both sections are lines: the straight-section formula holds as it is.
    #[must_use]
    pub fn straight(&self) -> bool {
        self.sections.iter().all(|s| s.k == 0.0)
    }
}

/// One support at the station: its quadric, its tangent into the face.
struct Support {
    quadric: BlendQuadric,
    t: Vec3,
}

fn support(k: &Kernel, b: &mut Brep, u: EdgeuseId, param: f64, tol: f64) -> Option<Support> {
    let at = evaluate_binormal(k, b, u, param, false, &BinormalOptions::default()).ok()?;
    let face = match eu1::faceuse(b, u).ok()? {
        Some(fu) => b.faceuse(fu).ok()?.face,
        None => None,
    }?;
    let surface = b.face(face).ok()?.surface.as_ref()?;
    let quadric = BlendQuadric::from_surface(k, surface, at.uv, tol).ok()?;
    Some(Support {
        quadric,
        t: at.binormal.unitize().ok()?,
    })
}

/// The point off `p` across `dir`.
fn across(v: Vec3, dir: Vec3) -> Vec3 {
    v - dir * v.dot(dir)
}

/// A support's section in the plane square to the edge.
enum Section {
    /// A line.
    Line,
    /// A circle about the centre, of the radius.
    Circle(Point3, f64),
}

/// `q`'s section at `e` in the plane square to the edge, in its class (`rim` whether the edge
/// is a rim, else a translation); `None` for a quadric the class does not hold.
fn section_of(q: &BlendQuadric, e: Point3, rim: bool) -> Option<Section> {
    use QuadricKind as Q;
    match (q.kind, rim) {
        (Q::Plane, _) | (Q::Cylinder | Q::Cone, true) => Some(Section::Line),
        (Q::Cylinder, false) => {
            let c = q.point + q.dir * (e - q.point).dot(q.dir);
            Some(Section::Circle(c, q.radius))
        }
        (Q::Sphere, true) => Some(Section::Circle(q.point, q.radius)),
        (Q::Torus, true) => {
            let radial = across(e - q.point, q.dir).unitize().ok()?;
            Some(Section::Circle(q.point + radial * q.radius, q.tube))
        }
        _ => None,
    }
}

/// The sections of edge `id` (blend edge use `u0`, its mate `u1`) at its middle, where it is
/// a translation or a rim (the module's page); `reach` the sizes' scale, for the angle the
/// tolerance allows at that distance. `None` otherwise, and for any error.
#[must_use]
pub fn edge_sections(
    k: &Kernel,
    b: &mut Brep,
    id: EdgeId,
    u0: EdgeuseId,
    u1: EdgeuseId,
    reach: f64,
) -> Option<EdgeSections> {
    use QuadricKind as Q;
    let tol = edge::tolerance(b, id).ok()?;
    if !(tol.is_finite() && tol > 0.0 && reach.is_finite() && reach > 0.0) {
        return None;
    }
    let ang = tol / reach.max(tol);
    let param = b.edge(id).ok()?.interval.evaluate(0.5);
    let at = evaluate_binormal(k, b, u0, param, false, &BinormalOptions::default()).ok()?;
    let e = at.point;
    let tangent = at.tangent.unitize().ok()?;
    let s = [
        support(k, b, u0, param, tol)?,
        support(k, b, u1, param, tol)?,
    ];
    let kinds_ok = |rim: bool| {
        s.iter().all(|x| match x.quadric.kind {
            Q::Plane => x.quadric.dir.dot(tangent).abs() <= ang,
            Q::Cylinder if !rim => x.quadric.dir.cross(tangent).length() <= ang,
            Q::Cylinder | Q::Cone | Q::Sphere | Q::Torus => rim,
            _ => false,
        })
    };
    let axis = if kinds_ok(false) {
        None
    } else {
        let (point, dir) = coaxial(&s[0].quadric, &s[1].quadric, tol, reach)?;
        let dir = dir.unitize().ok()?;
        if !(kinds_ok(true)
            && across(e - point, dir).length() > tol
            && tangent.dot(dir).abs() <= ang)
        {
            return None;
        }
        Some((point, dir))
    };
    // each support's normal toward the other, in the plane square to the edge
    let toward = |i: usize, j: usize| -> Option<Vec3> {
        let (a, b) = (s.get(i)?, s.get(j)?);
        let w = tangent.cross(a.t).unitize().ok()?;
        Some(if w.dot(b.t) < 0.0 { w * -1.0 } else { w })
    };
    let w = [toward(0, 1)?, toward(1, 0)?];
    let (x, y) = (s[0].t, w[0]);
    let mut sections = [ChamferSection {
        t: [1.0, 0.0],
        w: [0.0, 1.0],
        k: 0.0,
    }; 2];
    for (i, (sup, wi)) in s.iter().zip(w).enumerate() {
        let curvature = match section_of(&sup.quadric, e, axis.is_some())? {
            Section::Line => 0.0,
            Section::Circle(c, rho) => {
                let to = c - e;
                // the edge point on the circle, its centre along the normal
                if !(rho.is_finite()
                    && rho > tol
                    && (to.length() - rho).abs() <= tol
                    && across(to, wi).length() <= tol)
                {
                    return None;
                }
                to.dot(wi).signum() / rho
            }
        };
        let slot = sections.get_mut(i)?;
        *slot = ChamferSection {
            t: [sup.t.dot(x), sup.t.dot(y)],
            w: [wi.dot(x), wi.dot(y)],
            k: curvature,
        };
    }
    Some(EdgeSections {
        sections,
        origin: e,
        x,
        y,
        wedge: s[0].t.dot(s[1].t).clamp(-1.0, 1.0).acos(),
        axis,
        tol,
    })
}
