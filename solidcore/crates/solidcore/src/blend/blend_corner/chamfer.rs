//! The blend corner's chamfer event: where every edge at a vertex is chamfered and every
//! chamfer is a plane between two planes, the corner is closed by the chamfer planes
//! themselves, exactly. The Mitre (the default) runs each chamfer plane on until it
//! meets the others: three meet at one point `M`, pairwise along the lines from the rails'
//! crossings `X_i` to `M`, and there is no corner face. The Facet cuts the corner off with
//! the plane through the crossings; its edges are the chamfers' end sections
//! `X_i`-`X_(i+1)`, lines in their chamfers' planes.
//!
//! Upstream: none. Pinned SMLib builds such a vertex as `SmFilletConvexNxNCorner` with
//! `SetChamfer(TRUE)` (`SmFilletCorner::Create`, convex and concave alike): with
//! `SetBevel` its mitre edges are the chamfer surfaces' marched intersection and four or
//! more chamfers are refused with no reason; without it the corner face goes through the
//! corner's vertices, a bilinear B-spline where there are four, which is not a plane
//! where the chamfers are unequal. Own design, from smlib#89's brief (tangency-labs/
//! solidcore#2345); no fork code.
//!
//! The geometry is the construction's own, with no sampling. At the vertex `V` the edges
//! `e_i` leave along the unit lines `t_i` and face `F_i` lies between `e_(i-1)` and `e_i`.
//! The chamfer of `e_i` touches `F_i` and `F_(i+1)` on its rails, the edge's line moved
//! into each face by its setback; the blend's side of the edge is the wedge between the
//! faces' directions `q` away from the edge (towards the face's other edge), of angle
//! `phi`. A constant ball of offsets `r_a`, `r_b` touches them at the setbacks
//! `s_a = (r_b + r_a cos phi) / sin phi` and `s_b = (r_a + r_b cos phi) / sin phi`; a
//! constant chord `d` at `s = d / (2 sin(phi / 2))` on both. Each plane is held as
//! `n . (x - V) = h`, its normal towards the kept side (`h > 0`); scaling a chamfer's size
//! by a factor scales its `h` alone. `X_i = F_i ^ C_(i-1) ^ C_i` and
//! `M = C_0 ^ C_1 ^ C_2` are 3x3 solves. A face plane's sign is positive on the blend's
//! side (the side of the edges not in it).
//!
//! Validity is decided by signs of these linear forms, at the part-scale rounding gate
//! (1e-13 of the part's size). The signs decide only where the points are determined to
//! that rounding: where the chamfer planes are nearly coplanar (a very flat corner) the
//! solves' error bound, which grows as one over the angle between the planes, passes the
//! gate, and the corner is refused, naming that. The mitre holds where `M` is strictly on the blend's side
//! of every face and each `X_i` is strictly on the kept side of the third chamfer. Each
//! test is affine in each chamfer's size factor, so the largest size of the chamfer that
//! is too wide comes in closed form; the bound is open (at it a mitre edge has no length),
//! so the fit is it less the fit margin, a thousand of the edge's tolerances. At three
//! chamfers the facet holds where `M` is strictly on the vertex's side of its plane: `M`
//! leaves a face's plane only through that face's `X_i` (`M` on `F_i` and on `C_(i-1)`,
//! `C_i` is `X_i`), where the tetrahedron `M X_0 X_1 X_2` flips, so the facet fails where
//! the mitre does and is refused with the same fit. Four or more chamfers build the facet
//! where their crossings lie in one plane, each crossing is on the kept side of the
//! chamfers not through it and the line of its two chamfers runs from it to the vertex's
//! side of the plane; the mitre of four or more is refused (not designed: the planes
//! meet in one point only by coincidence). Where their crossings are not in one plane, the
//! corner is a patch, whichever type was asked (tangency-labs/solidcore#2345, the owner's
//! answers): the chamfers stop at their crossings, and n bilinear quadrilaterals round the
//! crossings' centroid C close the skew polygon of their end sections, quadrilateral m
//! between C, the middle of the side before `X_m`, `X_m` and the middle of the side after
//! it. A chamfer corner is G0, as its chamfers meet their faces: the quadrilaterals meet
//! each other along the straight spokes from C and each chamfer along its end section,
//! edge to edge. Where the crossings are in one plane they would be flat (the facet builds
//! there). At four they are the quarters of the crossings' own bilinear patch (the
//! hyperbolic paraboloid through them), and are that one face. No smooth patch can carry
//! the straight end sections past four: one polynomial over a polygon is one value where
//! two of its sides' lines meet, and past four sides non-adjacent sides' lines meet. Every
//! corner of every quadrilateral (C, the sides' middles and the crossings) is in the
//! crossings' convex hull, which is in the corner (each crossing on its face and its two
//! chamfers, on the blend's side of the other faces and the kept side of the other
//! chamfers), so the patch never stands past a face or into a chamfer: it is refused only
//! where a crossing is not on the kept side of a chamfer not through it. The crossings run
//! round so that the faces' normals point out of the material, chosen from the polygon's
//! own normal.
//!
//! The chamfers' own surfaces are the solvers': [`chamfer_corner_edge_geom`] requires
//! them to hold the exact lines to the corner's tolerance, as [`place_chamfer_vertices`]
//! requires of their crossings. A constant ball's are its planes to rounding; a constant
//! chord's are traced, and past their edges' ends (where a mitre's `M` lies) off their
//! planes by up to about 1e-9.
//!
//! | Rust                         | Role                                                   |
//! |------------------------------|--------------------------------------------------------|
//! | [`ChamferCorner`]            | the event on its corner: its definition, the facet      |
//! | [`ChamferCornerDefinition`]  | the corner's chamfers, its exact points, its type       |
//! | [`ChamferSolvers`]           | the solvers as the event reads them                     |
//! | [`classify_chamfer`]         | Classify: the event, a decline (why) or a refusal       |
//! | [`place_chamfer_vertices`]   | the corner's vertices at their exact points             |
//! | [`chamfer_corner_edge_geom`] | the corner's edges as exact lines; the facet's plane    |
//! | [`create_chamfer_patch`]     | the facet's face or the corner patch's faces            |
//! | `MakeTopology`               | the 3x3 corner's (`make_convex_nxn_corner_topology`): the bevel's for the Mitre, the ring's for the Facet and the corner patch |
//!
//! Deviations: not a port. Opt-in: `Kernel::fixes.fillet_chamfer_corner` (WR-716). No
//! recursion.

use super::{foot_on_surface, swept::fmt_g, BlendChamferCorner};
use crate::blend::fillet_corner::p1::face_by_two_edgeuses;
use crate::blend::fillet_corner::p3::{CornerPatchEnv, FilletCorner};
use crate::blend::fillet_executive::FitKind;
use crate::blend::fillet_geom::p1::{FilStatus, FilletBrep, FilletEdgeType, FilletErrorType};
use crate::blend::fillet_geom::p4::set_edge_curve;
use crate::blend::fillet_solver::p1::FilletSolver;
use crate::blend::fillet_solver::p5::is_concave_edge;
use crate::curve::curve::Curve;
use crate::curve::line::Line;
use crate::error::{EntityRef, Result, SolidError};
use crate::geom::extent2d::Extent2d;
use crate::kernel::Kernel;
use crate::math::{Point2, Point3, Vec2, Vec3, EFF_ZERO, EFF_ZERO_SQRT};
use crate::nurbs::b_spline_curve::p1::{BSplineCurveForm, CanonicalData, KnotType};
use crate::nurbs::b_spline_curve::BSplineCurve;
use crate::nurbs::b_spline_surface::{BSplineSurface, BSplineSurfaceForm, CanonicalSurfaceData};
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::surface::plane::Plane;
use crate::surface::surface::Surface;
use crate::topology::brep::p7::{make_face_with_curves, FaceCurvesInput};
use crate::topology::edgeuse::p1 as eu1;
use crate::topology::face::p2 as face2;
use crate::topology::{edge, vertex, Brep, EdgeId, EdgeuseId, VertexId};

const OP_CLASSIFY: &str = "blend::blend_corner::chamfer::classify_chamfer";
const OP_PLACE: &str = "blend::blend_corner::chamfer::place_chamfer_vertices";
const OP_EDGES: &str = "blend::blend_corner::chamfer::chamfer_corner_edge_geom";
const OP_PATCH: &str = "blend::blend_corner::chamfer::create_chamfer_patch";

/// The part-scale rounding gate, relative to the part's size.
const GATE: f64 = 1e-13;

/// The most chamfers a corner is classified for (a bound on input-driven work).
const MAX_CHAMFERS: usize = 64;

/// What sets a chamfer's width: a constant ball's offsets from its faces (`prev` from
/// `F_i`, `next` from `F_(i+1)`), or a constant chord between its rails.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferWidth {
    /// A constant-radius solver's offsets (absolute).
    Offsets {
        /// From the face before the edge in the ring.
        prev: f64,
        /// From the face after it.
        next: f64,
    },
    /// A constant-distance solver's chord.
    Chord(f64),
}

/// The solvers as the chamfer event reads them.
pub trait ChamferSolvers {
    /// The live solver of the blended edge use `eu` (else of its mate), with what sets its
    /// chamfer's width: `None` for no solver, `Some((solver, None))` for a chamfer whose
    /// width can vary along its edge.
    ///
    /// # Errors
    ///
    /// Where the lookup is not available.
    fn chamfer_solver(
        &self,
        b: &Brep,
        eu: EdgeuseId,
    ) -> Result<Option<(&FilletSolver, Option<ChamferSizeKind>)>>;
}

/// What a solver's constant chamfer width is.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum ChamferSizeKind {
    /// Constant radius: the width is the offsets of its surfaces.
    Offsets,
    /// Constant distance: the chord.
    Chord(f64),
}

/// One chamfer of a corner's ring as the corner reads it: its edge, what sets its width,
/// and its plane.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct CornerChamfer {
    /// The chamfered edge.
    pub edge: EdgeId,
    /// Its constant width.
    pub width: ChamferWidth,
    /// Its plane's unit normal, towards the kept side.
    pub normal: Vec3,
    /// Its plane is `normal . x = offset`.
    pub offset: f64,
}

/// What defines a chamfer corner, as plain data: its type, the vertex, the ring's chamfers
/// and the points their planes meet in. A Mitre is the construction of its mitre lines
/// `X_i`-`M` (no facet); a Facet is the plane through the `X_i`, its normal out of the
/// material. This is what the corner-definition type of tangency-labs/solidcore#2488 is to
/// take for a chamfer corner, so a rebuild knows the corner's faces and edges as this
/// corner's.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferCornerDefinition {
    /// Mitre or Facet.
    pub kind: BlendChamferCorner,
    /// The vertex V.
    pub vertex: Point3,
    /// The chamfers in ring order: chamfer i is the ring's edge use i, between face i and
    /// face i + 1.
    pub chamfers: Vec<CornerChamfer>,
    /// `X_i`, on face i (between chamfers i - 1 and i), in ring order.
    pub crossings: Vec<Point3>,
    /// M, at a mitre of three.
    pub mitre: Option<Point3>,
    /// At a Facet its unit normal, out of the material; `None` at a Mitre and at a corner
    /// patch.
    pub facet_normal: Option<Vec3>,
    /// The corner patch of four or more chamfers whose crossings are not in one plane,
    /// whichever type was asked: bilinear quadrilaterals round the crossings' centroid.
    pub patch: Option<ChamferPatch>,
}

impl ChamferCornerDefinition {
    /// The Mitre's construction (the bevel's topology): asked for, and no corner patch.
    #[must_use]
    pub fn is_mitre(&self) -> bool {
        self.kind == BlendChamferCorner::Mitre && self.patch.is_none()
    }
}

/// A chamfer corner's patch, as plain data with the definition's crossings: its centre and
/// the order its faces run in.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferPatch {
    /// C, the crossings' centroid: every quadrilateral's first corner.
    pub centre: Point3,
    /// The crossings in turn (indices into the definition's), counterclockwise about the
    /// patch's normal out of the material.
    pub around: Vec<usize>,
}

/// The chamfer event on its corner: its definition, and the facet's face or the corner
/// patch's faces from the edge geometry until the patch takes them.
#[derive(Debug, Clone)]
pub struct ChamferCorner {
    /// What defines the corner.
    pub definition: ChamferCornerDefinition,
    /// The corner's faces: the facet's, or the corner patch's.
    pub faces: Vec<CornerFace>,
}

/// One face of a chamfer corner, before it is made: its surface and its loop.
#[derive(Debug, Clone)]
pub struct CornerFace {
    /// The facet's plane, or a bilinear patch of the corner patch.
    pub surface: Surface,
    /// Its loop's lines.
    pub curves_3d: Vec<Curve>,
    /// Their UV lines on the surface.
    pub uv_curves: Vec<Curve>,
    /// Their orientations in the loop.
    pub orients: Vec<OrientType>,
}

/// A chamfer corner refused, with the error and the reason the executive notes at the
/// vertex and, where a size of one chamfer builds it, that chamfer's edge and its fit.
#[derive(Debug, Clone, PartialEq)]
pub struct ChamferRefusal {
    /// [`FilletErrorType::NoRoom`] (a chamfer too wide) or
    /// [`FilletErrorType::CornerUnsupported`].
    pub error: FilletErrorType,
    /// The reason.
    pub comment: String,
    /// The edge whose chamfer is too wide.
    pub fit_edge: Option<EdgeId>,
    /// Its largest size that builds (less the fit margin), and what it sizes.
    pub fit: Option<(f64, FitKind)>,
}

/// What [`classify_chamfer`] finds.
#[derive(Debug, Clone)]
pub enum ChamferClass {
    /// The event builds the corner.
    Event(Box<ChamferCorner>),
    /// Not the event's configuration, and why; the corner is the pinned one.
    Declined(&'static str),
    /// The event's configuration, but the corner type does not exist there.
    Refused(ChamferRefusal),
}

// ---- the closed forms ------------------------------------------------------------------

/// A chamfer's plane relative to V: `n . y = scale * h`, n unit towards the kept side.
#[derive(Debug, Clone, Copy, PartialEq)]
struct ChamferPlane {
    n: Vec3,
    h: f64,
}

/// The solution of `a_k . y = b_k`, k = 0..3 (Cramer's rule), `None` where the planes
/// do not meet in one point.
fn solve3(a: [Vec3; 3], b: [f64; 3]) -> Option<Vec3> {
    let [a0, a1, a2] = a;
    let det = a0.dot(a1.cross(a2));
    if det.abs() <= EFF_ZERO {
        return None;
    }
    Some((a1.cross(a2) * b[0] + a2.cross(a0) * b[1] + a0.cross(a1) * b[2]) * (1.0 / det))
}

/// The rounding of one row of a corner's 3x3 solve, in units of `f64::EPSILON`: a unit
/// normal comes to a few (its rails' difference, the cross product with the edge, the
/// unitize), and Cramer's rule's own cofactors, each component a difference of two products
/// of unit components, add as much again.
const ROW_ULPS: f64 = 8.0;

/// A bound on the rounding error of `y`, the solution of `a_k . y = b_k` for unit `a_k`
/// (by [`solve3`]), where each `b_k` is computed from a point within `s` of V; infinite
/// where the rows are dependent.
///
/// The rows are known to their rounding: `|da_k| <= u` and `|db_k| <= u s` with
/// `u = ROW_ULPS * EPSILON` (an offset `h = n . p` rounds relative to `|p| <= s`, not to
/// `h`). To first order `y` moves by `A^-1 (db - dA y)`. With `c_k` the cofactors
/// (`c_0 = a_1 x a_2` and its turns) and `D = a_0 . c_0`, `a_j . c_k = D` where `j = k`
/// and 0 otherwise, so `A^-1`'s columns are `c_k / D` and
/// `|dy| <= sum_k |c_k| (|db_k| + |da_k| |y|) / |D| <= u (s + |y|) sum_k |c_k| / |D|`.
/// Where the planes meet at a large angle the sum over `|D|` is a few; where they are
/// nearly coplanar `|D|` is of the second order in the angle between them and each `|c_k|`
/// of the first, so the bound grows as one over that angle.
fn solve3_error(a: [Vec3; 3], y: Vec3, s: f64) -> f64 {
    let [a0, a1, a2] = a;
    let det = a0.dot(a1.cross(a2)).abs();
    if det <= 0.0 || !det.is_finite() {
        return f64::INFINITY;
    }
    let cofactors = a1.cross(a2).length() + a2.cross(a0).length() + a0.cross(a1).length();
    ROW_ULPS * f64::EPSILON * (s + y.length()) * cofactors / det
}

/// The rows of `X_i`'s solve: face i and the chamfers through it, `C_(i-1)` and `C_i`.
fn crossing_rows(mu: &[Vec3], planes: &[ChamferPlane], i: usize) -> Option<[Vec3; 3]> {
    let n = mu.len();
    let p = (i + n - 1) % n;
    Some([*mu.get(i)?, planes.get(p)?.n, planes.get(i)?.n])
}

/// The rows of M's solve: the first three chamfers.
fn mitre_rows(planes: &[ChamferPlane]) -> Option<[Vec3; 3]> {
    Some([planes.first()?.n, planes.get(1)?.n, planes.get(2)?.n])
}

/// The unit part of `v` square to the unit `t`, `None` where it has none.
fn square_to(v: Vec3, t: Vec3) -> Option<Vec3> {
    (v - t * v.dot(t)).unitize().ok()
}

/// The ring's chamfer planes relative to V, from the edges' unit directions `t` and the
/// widths; with each, its setbacks on `F_i` and `F_(i+1)`. `Err` names why there is none.
fn chamfer_planes(
    t: &[Vec3],
    widths: &[ChamferWidth],
) -> core::result::Result<Vec<(ChamferPlane, [f64; 2])>, &'static str> {
    let n = t.len();
    let mut out = Vec::new();
    for (i, width) in widths.iter().enumerate() {
        let (Some(&ti), Some(&tp), Some(&tn)) =
            (t.get(i), t.get((i + n - 1) % n), t.get((i + 1) % n))
        else {
            return Err("the ring is incomplete");
        };
        // into each face, away from the edge: towards the face's other edge
        let (Some(qa), Some(qb)) = (square_to(tp, ti), square_to(tn, ti)) else {
            return Err("two edges at the vertex leave it along one line");
        };
        let cos = qa.dot(qb);
        let sin = qa.cross(qb).length();
        if sin <= EFF_ZERO_SQRT {
            return Err("two faces at a chamfered edge are tangent");
        }
        let (sa, sb) = match *width {
            ChamferWidth::Offsets { prev, next } => {
                ((next + prev * cos) / sin, (prev + next * cos) / sin)
            }
            ChamferWidth::Chord(d) => {
                // sin(phi / 2) from the half-angle formula
                let half = (0.5 * (1.0 - cos)).max(0.0).sqrt();
                if half <= EFF_ZERO {
                    return Err("two faces at a chamfered edge are tangent");
                }
                (d / (2.0 * half), d / (2.0 * half))
            }
        };
        if sa <= 0.0 || sb <= 0.0 || !sa.is_finite() || !sb.is_finite() {
            return Err("a chamfer's setback is not positive");
        }
        let (pa, pb) = (qa * sa, qb * sb);
        let Ok(mut nrm) = (pb - pa).cross(ti).unitize() else {
            return Err("a chamfer's rails coincide");
        };
        let mut h = nrm.dot(pa);
        if h < 0.0 {
            nrm = -nrm;
            h = -h;
        }
        out.push((ChamferPlane { n: nrm, h }, [sa, sb]));
    }
    Ok(out)
}

/// `X_i = F_i ^ C_(i-1) ^ C_i` relative to V, for the face normals `mu` and the planes with
/// their size factors `scale`.
fn crossings(mu: &[Vec3], planes: &[ChamferPlane], scale: &[f64]) -> Option<Vec<Vec3>> {
    let n = mu.len();
    let mut out = Vec::new();
    for i in 0..n {
        let p = (i + n - 1) % n;
        let (cp, ci) = (planes.get(p)?, planes.get(i)?);
        let (sp, si) = (scale.get(p)?, scale.get(i)?);
        out.push(solve3(
            crossing_rows(mu, planes, i)?,
            [0.0, sp * cp.h, si * ci.h],
        )?);
    }
    Some(out)
}

/// `M = C_0 ^ C_1 ^ C_2` relative to V.
fn mitre_point(planes: &[ChamferPlane], scale: &[f64]) -> Option<Vec3> {
    let (c0, c1, c2) = (planes.first()?, planes.get(1)?, planes.get(2)?);
    let (s0, s1, s2) = (scale.first()?, scale.get(1)?, scale.get(2)?);
    solve3(mitre_rows(planes)?, [s0 * c0.h, s1 * c1.h, s2 * c2.h])
}

/// The largest rounding error bound ([`solve3_error`]) of the corner's points relative to
/// V: the crossings `xs` and, at three chamfers, M; infinite where a solve has none.
fn points_error(mu: &[Vec3], planes: &[ChamferPlane], xs: &[Vec3], s: f64) -> f64 {
    let mut worst = 0.0_f64;
    for (i, &x) in xs.iter().enumerate() {
        worst = worst.max(
            crossing_rows(mu, planes, i).map_or(f64::INFINITY, |rows| solve3_error(rows, x, s)),
        );
    }
    if planes.len() == 3 {
        let ones = [1.0; 3];
        worst = worst.max(
            mitre_rows(planes)
                .zip(mitre_point(planes, &ones))
                .map_or(f64::INFINITY, |(rows, m)| solve3_error(rows, m, s)),
        );
    }
    worst
}

/// The mitre's linear forms at three chamfers, each positive where it holds: M on the
/// blend's side of each face, and each `X_i` on the kept side of the third chamfer.
fn mitre_tests(mu: &[Vec3], planes: &[ChamferPlane], scale: &[f64]) -> Option<Vec<f64>> {
    let m = mitre_point(planes, scale)?;
    let xs = crossings(mu, planes, scale)?;
    let mut out: Vec<f64> = mu.iter().map(|&mj| mj.dot(m)).collect();
    for (i, x) in xs.iter().enumerate() {
        let k = (i + 1) % 3;
        let (ck, sk) = (planes.get(k)?, scale.get(k)?);
        out.push(ck.n.dot(*x) - sk * ck.h);
    }
    Some(out)
}

/// The chamfer whose size is too large for the mitre (its tests fail at its size and
/// hold below some bound with the others fixed), and that bound as a factor on its size;
/// the least bound where several are. `None` where no one chamfer's size is the cause.
fn too_wide(mu: &[Vec3], planes: &[ChamferPlane]) -> Option<(usize, f64)> {
    let mut best: Option<(usize, f64)> = None;
    for i in 0..planes.len() {
        let mut scale = vec![1.0; planes.len()];
        let at1 = mitre_tests(mu, planes, &scale)?;
        if let Some(slot) = scale.get_mut(i) {
            *slot = 0.0;
        }
        let at0 = mitre_tests(mu, planes, &scale)?;
        // each test is a + b s in this chamfer's factor s
        let (mut lo, mut hi) = (f64::NEG_INFINITY, f64::INFINITY);
        let mut empty = false;
        for (&g1, &g0) in at1.iter().zip(&at0) {
            let (a, b) = (g0, g1 - g0);
            if b > 0.0 {
                lo = lo.max(-a / b);
            } else if b < 0.0 {
                hi = hi.min(-a / b);
            } else if a <= 0.0 {
                empty = true;
            }
        }
        if empty || hi <= lo || hi <= 0.0 || hi > 1.0 || hi.is_nan() {
            continue;
        }
        if best.is_none_or(|(_, b)| hi < b) {
            best = Some((i, hi));
        }
    }
    best
}

/// The plane through the points `xs` relative to V (Newell's normal), oriented with V on
/// its negative side: `(n, d)` with `n . y = d`, d > 0. `None` where the points span no
/// plane or it holds V.
fn facet_plane(xs: &[Vec3], gate: f64) -> Option<(Vec3, f64)> {
    let n = xs.len();
    let mut nrm = Vec3::default();
    let mut centre = Vec3::default();
    for (i, a) in xs.iter().enumerate() {
        let b = xs.get((i + 1) % n)?;
        nrm.x += (a.y - b.y) * (a.z + b.z);
        nrm.y += (a.z - b.z) * (a.x + b.x);
        nrm.z += (a.x - b.x) * (a.y + b.y);
        centre += *a;
    }
    let mut nrm = nrm.unitize().ok()?;
    let centre = centre * (1.0 / n as f64);
    let mut d = nrm.dot(centre);
    if d.abs() <= gate {
        return None;
    }
    if d < 0.0 {
        nrm = -nrm;
        d = -d;
    }
    Some((nrm, d))
}

/// Why the facet does not close the corner, `None` where it does: its crossings off one
/// plane (`spread`, at four or more), a crossing on the removed side of a chamfer not
/// through it, or the line of a crossing's two chamfers running from it away from the
/// vertex's side of the plane (at three: M off the vertex's side).
fn facet_fails(
    mu: &[Vec3],
    planes: &[ChamferPlane],
    xs: &[Vec3],
    plane: (Vec3, f64),
    gate: f64,
) -> Option<&'static str> {
    let n = xs.len();
    let (pn, pd) = plane;
    let ones = vec![1.0; n];
    if xs.iter().any(|x| (pn.dot(*x) - pd).abs() > gate) {
        return Some("spread");
    }
    if n == 3 {
        let Some(m) = mitre_point(planes, &ones) else {
            return Some("cuts");
        };
        if pn.dot(m) - pd >= -gate {
            return Some("cuts");
        }
        return None;
    }
    for (i, x) in xs.iter().enumerate() {
        let p = (i + n - 1) % n;
        for (j, c) in planes.iter().enumerate() {
            if j != i && j != p && c.n.dot(*x) - c.h <= gate {
                return Some("cuts");
            }
        }
        let (Some(cp), Some(ci), Some(&m)) = (planes.get(p), planes.get(i), mu.get(i)) else {
            return Some("cuts");
        };
        let mut dir = cp.n.cross(ci.n);
        if dir.dot(m) < 0.0 {
            dir = -dir;
        }
        if pn.dot(dir) >= -EFF_ZERO {
            return Some("cuts");
        }
    }
    None
}

// ---- Classify ---------------------------------------------------------------------------

/// The ring's faces: face i between edge uses i - 1 and i.
fn ring_faces(b: &Brep, all_eus: &[EdgeuseId]) -> Result<Vec<Option<crate::topology::FaceId>>> {
    let n = all_eus.len();
    let mut out = Vec::new();
    for i in 0..n {
        let prev = all_eus.get((i + n - 1) % n).copied();
        out.push(face_by_two_edgeuses(
            b,
            prev,
            all_eus.get(i).copied(),
            None,
        )?);
    }
    Ok(out)
}

/// The refusal of a chamfer too wide for the corner: its edge and its fit, the bound
/// `hi` on its size factor less the fit margin.
fn too_wide_refusal(
    b: &Brep,
    kind: BlendChamferCorner,
    edge_of: Option<(EdgeId, ChamferWidth)>,
    hi: f64,
) -> Result<ChamferRefusal> {
    let what = match kind {
        BlendChamferCorner::Mitre => "the chamfers' mitre point falls outside the corner",
        BlendChamferCorner::Facet => {
            "the chamfers' mitre point falls outside the corner, so the facet through their rails' crossings would cut them instead of closing them"
        }
    };
    let Some((e, width)) = edge_of else {
        return Ok(ChamferRefusal {
            error: FilletErrorType::NoRoom,
            comment: format!("Fillet Error: {what}; no size of one chamfer closes it"),
            fit_edge: None,
            fit: None,
        });
    };
    let margin = 1000.0 * edge::tolerance(b, e)?;
    let (size, fit_kind) = match width {
        ChamferWidth::Offsets { prev, next }
            if (prev - next).abs() <= EFF_ZERO * prev.max(next) =>
        {
            (hi * prev - margin, FitKind::Radius)
        }
        ChamferWidth::Offsets { prev, next } => (hi - margin / prev.max(next), FitKind::Scale),
        ChamferWidth::Chord(d) => (hi * d - margin, FitKind::Chord),
    };
    let size = size.max(0.0);
    let named = match fit_kind {
        FitKind::Scale => format!("scale that chamfer by {} at most", fmt_g(size)),
        _ => format!("that chamfer builds up to {}", fmt_g(size)),
    };
    Ok(ChamferRefusal {
        error: FilletErrorType::NoRoom,
        comment: format!("Fillet Error: {what} (one chamfer is too wide for the others); {named}"),
        fit_edge: Some(e),
        fit: Some((size, fit_kind)),
    })
}

fn unsupported(why: String) -> ChamferClass {
    ChamferClass::Refused(ChamferRefusal {
        error: FilletErrorType::CornerUnsupported,
        comment: why,
        fit_edge: None,
        fit: None,
    })
}

/// Classify: whether the chamfer corner at `vertex` (every edge there chamfered: the
/// edge uses `all_eus` in ring order, each blended with a linear section) is the chamfer
/// event, and of type `kind`. It is where every edge is a straight line and every face a
/// plane, the faces distinct, the chamfers of one convexity, each of constant width
/// (a constant ball or a constant chord), and at three chamfers the faces' normals
/// independent; else it declines, naming why, and the corner is the pinned one. The event
/// is refused where its type does not exist: a mitre point outside the corner (with the
/// too wide chamfer's fit), a mitre of four or more chamfers, a facet whose crossings are
/// not in one plane or that would cut the chamfers; and it is refused where the chamfer
/// planes are so nearly coplanar that the points where they meet are not determined to the
/// part's rounding ([`solve3_error`] above the gate). The event's corner keeps its
/// [`ChamferCornerDefinition`].
///
/// # Errors
///
/// [`SolidError::null_pointer`] for missing topology or geometry; those of the queries.
#[allow(clippy::too_many_lines)] // one classification
pub fn classify_chamfer(
    k: &Kernel,
    b: &mut Brep,
    vertex: VertexId,
    all_eus: &[EdgeuseId],
    kind: BlendChamferCorner,
    tol: f64,
    solvers: &dyn ChamferSolvers,
) -> Result<ChamferClass> {
    let nul = || {
        SolidError::blend_missing(
            OP_CLASSIFY,
            "topology or a solver the chamfer corner's classification reads",
        )
    };
    let n = all_eus.len();
    if !(3..=MAX_CHAMFERS).contains(&n) {
        return Ok(ChamferClass::Declined(
            "fewer than three or too many chamfers",
        ));
    }
    let v = vertex::point(b, vertex)?;

    // the edges: straight lines leaving V, of one convexity
    let mut t = Vec::new();
    let mut edges = Vec::new();
    let mut concave0 = None;
    for &eu in all_eus {
        let e = eu1::edge(b, eu)?.ok_or_else(nul)?;
        let curve = edge::curve(b, e)?.ok_or_else(nul)?;
        let Some((_, dir)) = curve.is_line(k, 10, tol)? else {
            return Ok(ChamferClass::Declined(
                "an edge at the vertex is not a straight line",
            ));
        };
        let Ok(mut dir) = dir.unitize() else {
            return Ok(ChamferClass::Declined(
                "an edge at the vertex has no direction",
            ));
        };
        let start = edge::vertex(b, e)?.ok_or_else(nul)?;
        let other = if start == vertex {
            edge::other_vertex(b, e, Some(start))?.ok_or_else(nul)?
        } else {
            start
        };
        if dir.dot(vertex::point(b, other)? - v) < 0.0 {
            dir = -dir;
        }
        let concave = is_concave_edge(k, b, e)?;
        if concave0.is_some_and(|c| c != concave) {
            return Ok(ChamferClass::Declined(
                "chamfers of both convexities meet at the vertex",
            ));
        }
        concave0 = Some(concave);
        t.push(dir);
        edges.push(e);
    }

    // the faces: distinct planes, each normal positive on the blend's side
    let faces = ring_faces(b, all_eus)?;
    let mut mu = Vec::new();
    for (i, face) in faces.iter().enumerate() {
        let Some(face) = *face else {
            return Ok(ChamferClass::Declined(
                "two edges at the vertex share no face",
            ));
        };
        if faces.iter().filter(|f| **f == Some(face)).count() > 1 {
            return Ok(ChamferClass::Declined("a face meets the vertex twice"));
        }
        let surface = face2::surface(b, face)?.ok_or_else(nul)?;
        let planar = surface.is_planar(k, tol)?;
        if !planar.is_planar {
            return Ok(ChamferClass::Declined(
                "a face at the vertex is not a plane",
            ));
        }
        let Ok(mut m) = planar.normal.unitize() else {
            return Ok(ChamferClass::Declined("a face at the vertex has no normal"));
        };
        // the edges not in it point to the blend's side
        let mut side = Vec3::default();
        for (j, tj) in t.iter().enumerate() {
            if j != i && j != (i + n - 1) % n {
                side += *tj;
            }
        }
        if m.dot(side) < 0.0 {
            m = -m;
        }
        mu.push(m);
    }
    if let (3, Some(&m0), Some(&m1), Some(&m2)) = (n, mu.first(), mu.get(1), mu.get(2)) {
        if m0.dot(m1.cross(m2)).abs() <= EFF_ZERO_SQRT {
            return Ok(ChamferClass::Declined(
                "the three faces do not meet at one point",
            ));
        }
    }

    // the chamfers: constant widths
    let mut widths = Vec::new();
    for (i, &eu) in all_eus.iter().enumerate() {
        let Some((solver, Some(size))) = solvers.chamfer_solver(b, eu)? else {
            return Ok(ChamferClass::Declined(
                "a chamfer's width varies along its edge",
            ));
        };
        widths.push(match size {
            ChamferSizeKind::Chord(d) => ChamferWidth::Chord(d),
            ChamferSizeKind::Offsets => {
                let face = faces.get(i).copied().flatten();
                let rail = solver.find_index_of_rail_on_face(b, face)?;
                let offset = |r: usize| -> Result<f64> {
                    Ok(solver
                        .surface(r)?
                        .ok_or_else(nul)?
                        .surface
                        .offset_distance()
                        .abs())
                };
                ChamferWidth::Offsets {
                    prev: offset(rail)?,
                    next: offset(1 - rail.min(1))?,
                }
            }
        });
    }
    let planes = match chamfer_planes(&t, &widths) {
        Ok(planes) => planes,
        Err(why) => return Ok(ChamferClass::Declined(why)),
    };
    let setback = planes
        .iter()
        .flat_map(|(_, s)| s.iter().copied())
        .fold(0.0_f64, f64::max);
    let planes: Vec<ChamferPlane> = planes.into_iter().map(|(p, _)| p).collect();
    let gate = GATE * (1.0 + v.max_dimension() + setback);
    let ones = vec![1.0; n];
    let Some(xs) = crossings(&mu, &planes, &ones) else {
        return Ok(ChamferClass::Declined(
            "a face and its two chamfers do not meet at one point",
        ));
    };
    // The chamfer planes nearly coplanar (a very flat corner: the faces' normals
    // independent, but only just): the points where they meet are not determined to the
    // part's rounding, so the sign tests below, at that rounding, would decide nothing
    let error = points_error(&mu, &planes, &xs, setback);
    if error > gate {
        return Ok(unsupported(format!(
            "Fillet Error: the chamfer planes at the vertex are nearly coplanar, so the points where they meet are not determined to the part's rounding (to {} where {} is needed)",
            fmt_g(error),
            fmt_g(gate)
        )));
    }

    if n == 3 {
        let Some(tests) = mitre_tests(&mu, &planes, &ones) else {
            return Ok(ChamferClass::Declined(
                "the three chamfers do not meet at one point",
            ));
        };
        if tests.iter().any(|&g| g <= gate) {
            let edge_of = too_wide(&mu, &planes)
                .and_then(|(i, hi)| Some(((*edges.get(i)?, *widths.get(i)?), hi)));
            let hi = edge_of.map_or(0.0, |(_, hi)| hi);
            return Ok(ChamferClass::Refused(too_wide_refusal(
                b,
                kind,
                edge_of.map(|(e, _)| e),
                hi,
            )?));
        }
    } else if spread(&xs).is_none_or(|s| s > gate) {
        // four or more whose crossings are not in one plane: the corner patch
        let convex = concave0 != Some(true);
        return Ok(corner_patch(
            kind, v, &planes, xs, &edges, &widths, convex, gate,
        ));
    } else if kind == BlendChamferCorner::Mitre {
        return Ok(unsupported(
            "Fillet Error: a mitre of four or more chamfers at a vertex is not built yet; their rails' crossings lie in one plane, which the facet closes"
                .to_owned(),
        ));
    }

    let mut facet_normal = None;
    if kind == BlendChamferCorner::Facet {
        let fails = match facet_plane(&xs, gate) {
            None => Some("cuts"),
            Some(plane) => {
                let fails = facet_fails(&mu, &planes, &xs, plane, gate);
                // out of the material: towards V at a convex corner (the chamfers remove
                // material), away from it at a concave one (they add it)
                facet_normal = Some(if concave0 == Some(true) {
                    plane.0
                } else {
                    -plane.0
                });
                fails
            }
        };
        if fails.is_some() {
            return Ok(unsupported(
                "Fillet Error: the facet through the chamfers' rail crossings would cut the chamfers instead of closing them"
                    .to_owned(),
            ));
        }
    }
    let mitre = if n == 3 && kind == BlendChamferCorner::Mitre {
        mitre_point(&planes, &ones).map(|m| v + m)
    } else {
        None
    };
    Ok(ChamferClass::Event(Box::new(ChamferCorner {
        definition: ChamferCornerDefinition {
            kind,
            vertex: v,
            chamfers: corner_chamfers(v, &edges, &widths, &planes),
            crossings: xs.into_iter().map(|x| v + x).collect(),
            mitre,
            facet_normal,
            patch: None,
        },
        faces: Vec::new(),
    })))
}

/// The ring's chamfers as the definition keeps them.
fn corner_chamfers(
    v: Point3,
    edges: &[EdgeId],
    widths: &[ChamferWidth],
    planes: &[ChamferPlane],
) -> Vec<CornerChamfer> {
    edges
        .iter()
        .zip(widths)
        .zip(planes)
        .map(|((&edge, &width), plane)| CornerChamfer {
            edge,
            width,
            normal: plane.n,
            offset: plane.n.dot(v) + plane.h,
        })
        .collect()
}

/// The polygon's normal by Newell's rule (unnormalized), round `xs` in turn.
fn newell(xs: &[Vec3]) -> Vec3 {
    let n = xs.len();
    let mut nrm = Vec3::default();
    for (i, a) in xs.iter().enumerate() {
        let b = xs.get((i + 1) % n).copied().unwrap_or(*a);
        nrm.x += (a.y - b.y) * (a.z + b.z);
        nrm.y += (a.z - b.z) * (a.x + b.x);
        nrm.z += (a.x - b.x) * (a.y + b.y);
    }
    nrm
}

/// How far the points `xs` are from one plane: the most any is from the plane through
/// their centre square to their Newell normal; `None` where they have no normal.
fn spread(xs: &[Vec3]) -> Option<f64> {
    let nrm = newell(xs).unitize().ok()?;
    let mut centre = Vec3::default();
    for x in xs {
        centre += *x;
    }
    let centre = centre * (1.0 / xs.len() as f64);
    Some(
        xs.iter()
            .map(|x| nrm.dot(*x - centre).abs())
            .fold(0.0, f64::max),
    )
}

/// The corner patch of four or more chamfers whose crossings `xs` (relative to V) are not
/// in one plane (module docs): the event with the patch, or its refusal where a crossing is
/// not on the kept side of a chamfer not through it.
#[allow(clippy::too_many_arguments)]
fn corner_patch(
    kind: BlendChamferCorner,
    v: Point3,
    planes: &[ChamferPlane],
    xs: Vec<Vec3>,
    edges: &[EdgeId],
    widths: &[ChamferWidth],
    convex: bool,
    gate: f64,
) -> ChamferClass {
    let n = xs.len();
    // each crossing strictly on the kept side of the chamfers not through it (as the
    // facet's): else the chamfers' ends overlap, and no patch closes them
    for (i, x) in xs.iter().enumerate() {
        let p = (i + n - 1) % n;
        if planes
            .iter()
            .enumerate()
            .any(|(j, c)| j != i && j != p && c.n.dot(*x) - c.h <= gate)
        {
            return unsupported(
                "Fillet Error: a rail crossing of the chamfers at the vertex lies past another chamfer, so their ends overlap and no corner patch closes them"
                    .to_owned(),
            );
        }
    }
    // round the polygon so that its normal points out of the material: towards V at a
    // convex corner (the chamfers remove material), away from it at a concave one
    let mut sum = Vec3::default();
    for x in &xs {
        sum += *x;
    }
    let out = if convex { -sum } else { sum };
    let mut around: Vec<usize> = (0..n).collect();
    if newell(&xs).dot(out) < 0.0 {
        around.reverse();
    }
    ChamferClass::Event(Box::new(ChamferCorner {
        definition: ChamferCornerDefinition {
            kind,
            vertex: v,
            chamfers: corner_chamfers(v, edges, widths, planes),
            crossings: xs.into_iter().map(|x| v + x).collect(),
            mitre: None,
            facet_normal: None,
            patch: Some(ChamferPatch {
                centre: v + sum * (1.0 / n as f64),
                around,
            }),
        },
        faces: Vec::new(),
    }))
}

// ---- the corner's geometry ---------------------------------------------------------------

/// The corner's vertices at their exact points (after the base's vertex geometry): the
/// crossings `X_i` (`corner.vertices[i]`, on face i), each solved as its rails' crossing
/// and required within the corner's tolerance of its exact point (the solver and the
/// closed form build the same chamfers); and at a mitre M (the bevel topology's centre,
/// after them), which nothing else computes (the pinned corner finds it while marching
/// the mitres), set and processed.
///
/// # Errors
///
/// `SM_ERR` where the corner is not as its topology made it or a solved crossing is not
/// at its exact point; [`SolidError::null_pointer`] without the event.
pub fn place_chamfer_vertices(pseudo: &mut FilletBrep, corner: &FilletCorner) -> Result<()> {
    let err = || SolidError::blend_failed(OP_PLACE, "a chamfer vertex could not be placed");
    let event = &corner
        .chamfer_corner
        .as_ref()
        .ok_or_else(|| SolidError::blend_missing(OP_PLACE, "the chamfer corner's event"))?
        .definition;
    let count = event.crossings.len();
    if corner.vertices.len() != count + usize::from(event.mitre.is_some()) {
        return Err(err());
    }
    for (&fv, &x) in corner.vertices.iter().zip(&event.crossings) {
        if !pseudo.fillet_vertex(fv)?.is_processed()
            || pseudo.vertex_point(fv)?.distance(x) > corner.approx_tol_3d
        {
            return Err(err());
        }
        pseudo.set_vertex_point(fv, x)?;
    }
    if let (Some(m), Some(&fv)) = (event.mitre, corner.vertices.get(count)) {
        pseudo.set_vertex_point(fv, m)?;
        pseudo
            .fillet_vertex_mut(fv)?
            .set_status(FilStatus::Processed);
    }
    Ok(())
}

/// The straight UV line on `surface` from `p` to `q` over `ivl`, exact where the surface
/// is affine in its parameters (a plane): `None` unless the surface holds both ends and
/// their middle within `tol`.
fn uv_line(
    k: &Kernel,
    surface: &Surface,
    p: Point3,
    q: Point3,
    ivl: [f64; 2],
    tol: f64,
) -> Result<Option<Curve>> {
    let mid = surface.natural_uv_domain_with_nurb(k)?.mid();
    let (Some(a), Some(b)) = (
        foot_on_surface(k, surface, p, mid)?,
        foot_on_surface(k, surface, q, mid)?,
    ) else {
        return Ok(None);
    };
    let half = Point2::new(0.5 * (a.x + b.x), 0.5 * (a.y + b.y));
    if surface.evaluate_point(k, a)?.distance(p) > tol
        || surface.evaluate_point(k, b)?.distance(q) > tol
        || surface.evaluate_point(k, half)?.distance(p + (q - p) * 0.5) > tol
    {
        return Ok(None);
    }
    let points = [Point3::new(a.x, a.y, 0.0), Point3::new(b.x, b.y, 0.0)];
    let line = BSplineCurve::create_canonical(&CanonicalData {
        dim: 2,
        degree: 1,
        control_points: &points,
        form: BSplineCurveForm::Unspecified,
        knot_multiplicities: &[2, 2],
        knots: &ivl,
        knot_type: KnotType::Unspecified,
        weights: None,
        trim: None,
    })?;
    Ok(Some(Curve::from(line)))
}

/// The corner's edges as exact lines (before the edges' own kinds compute theirs): each
/// edge of the corner (the mitres X_i-M, or the facet's sides X_i-X_(i+1)) is the line
/// between its exact vertices, its UV curve on each chamfer it bounds the exact line in
/// that chamfer's plane, and it is precomputed. At a facet its plane, oriented out of the
/// material, and its loop are kept in [`FilletCorner::chamfer_corner`] for the patch.
///
/// # Errors
///
/// `SM_ERR` where the corner is not as its topology made it or a chamfer's surface is not
/// affine in its parameters along an edge; [`SolidError::null_pointer`] for missing
/// topology or geometry; those of the curve and surface constructions.
#[allow(clippy::too_many_lines)] // one construction
pub fn chamfer_corner_edge_geom(
    k: &Kernel,
    pseudo: &mut FilletBrep,
    corner: &mut FilletCorner,
) -> Result<()> {
    let nul = || {
        SolidError::blend_missing(
            OP_EDGES,
            "topology or geometry the chamfer corner's edges are built from",
        )
    };
    let err =
        || SolidError::blend_failed(OP_EDGES, "the chamfer corner's edges could not be built");
    let event = &corner.chamfer_corner.as_ref().ok_or_else(nul)?.definition;
    let mitre = event.is_mitre();
    let mut size = 1.0_f64;
    for p in event.crossings.iter().chain(event.mitre.iter()) {
        size = size.max(1.0 + p.max_dimension());
    }
    // the facet's plane is ours, exact to rounding; a chamfer's surface is the solver's,
    // which holds its plane to the corner's tolerance (as its crossings, which
    // place_chamfer_vertices holds to the same)
    let exact = GATE * size;
    let solved = corner.approx_tol_3d.max(exact);
    let edge_type = if mitre {
        FilletEdgeType::FilletXFillet
    } else {
        FilletEdgeType::CrossSection
    };
    for &e in &corner.edges {
        if pseudo.fillet_edge(e)?.fillet_edge_type() != edge_type {
            return Err(err());
        }
        let start = edge::vertex(pseudo.brep(), e)?.ok_or_else(nul)?;
        let end = edge::other_vertex(pseudo.brep(), e, Some(start))?.ok_or_else(nul)?;
        let (p, q) = (pseudo.vertex_point(start)?, pseudo.vertex_point(end)?);
        let line = Curve::from(Line::create_line_segment(3, p, q, None)?);
        let ivl = line.natural_interval();
        let span = [ivl.min(), ivl.max()];
        let prim = eu1::primary_edgeuse(pseudo.brep(), e)?.ok_or_else(nul)?;
        let mate = eu1::mate(pseudo.brep(), prim)?.ok_or_else(nul)?;
        for eu in [prim, mate] {
            let Some(g) = pseudo.fillet_edgeuse(eu)?.fillet_geom() else {
                continue; // the facet's side of a facet edge: the patch's own loop
            };
            let surface = pseudo
                .geom(g)?
                .fillet_surface(None)?
                .ok_or_else(nul)?
                .clone();
            let uv = uv_line(k, &surface, p, q, span, solved)?.ok_or_else(err)?;
            pseudo.set_edgeuse_uv_curve(eu, Some(uv), true, true)?;
            pseudo.geom_mut(g)?.add_side_fillet_edgeuse(eu);
        }
        set_edge_curve(pseudo.brep_mut(), e, Some(line))?;
        pseudo
            .fillet_edge_mut(e)?
            .set_fillet_edge_type(FilletEdgeType::Precomputed);
    }
    if mitre {
        return Ok(());
    }
    let event = corner.chamfer_corner.as_mut().ok_or_else(nul)?;
    if let Some(patch) = event.definition.patch.clone() {
        return patch_faces(event, &patch);
    }

    // the facet: its plane through the crossings, its normal out of the material, the
    // loop counterclockwise about it
    let xs = &event.definition.crossings;
    let normal = event
        .definition
        .facet_normal
        .ok_or_else(err)?
        .unitize()
        .map_err(|_| err())?;
    let count = xs.len();
    let mut centre = Vec3::default();
    for x in xs {
        centre += *x;
    }
    let centre = centre * (1.0 / count as f64);
    let first = xs.first().copied().ok_or_else(err)?;
    let x_axis = (first - centre - normal * (first - centre).dot(normal))
        .unitize()
        .map_err(|_| err())?;
    let y_axis = normal.cross(x_axis);
    let uv_of = |p: Point3| Point2::new((p - centre).dot(x_axis), (p - centre).dot(y_axis));
    let (mut lo, mut hi) = (
        Point2::new(f64::MAX, f64::MAX),
        Point2::new(f64::MIN, f64::MIN),
    );
    for x in xs {
        let w = uv_of(*x);
        lo = Point2::new(lo.x.min(w.x), lo.y.min(w.y));
        hi = Point2::new(hi.x.max(w.x), hi.y.max(w.y));
    }
    let pad = 0.1 * (hi.x - lo.x).max(hi.y - lo.y);
    let domain = Extent2d::from_bounds(lo.x - pad, lo.y - pad, hi.x + pad, hi.y + pad);
    let mut plane = Plane::new(centre, x_axis, y_axis, Vec2::new(1.0, 1.0), &domain);
    plane.make_nurb()?;
    let surface = Surface::from(plane);

    // the loop: the facet's sides in ring order, turned counterclockwise about the normal
    let mut area = 0.0;
    for (i, x) in xs.iter().enumerate() {
        let (a, b) = (uv_of(*x), uv_of(*xs.get((i + 1) % count).ok_or_else(err)?));
        area += a.x * b.y - b.x * a.y;
    }
    let mut order: Vec<usize> = (0..count).collect();
    if area < 0.0 {
        order.reverse();
    }
    let mut curves_3d = Vec::new();
    let mut uv_curves = Vec::new();
    let mut orients = Vec::new();
    for (j, &i) in order.iter().enumerate() {
        let next = *order.get((j + 1) % count).ok_or_else(err)?;
        let (p, q) = (*xs.get(i).ok_or_else(err)?, *xs.get(next).ok_or_else(err)?);
        // the side's line, as the corner's edge from p to q, run as the loop runs
        let curve = Curve::from(Line::create_line_segment(3, p, q, None)?);
        let ivl = curve.natural_interval();
        let uv = uv_line(k, &surface, p, q, [ivl.min(), ivl.max()], exact)?.ok_or_else(err)?;
        curves_3d.push(curve);
        uv_curves.push(uv);
        orients.push(OrientType::Same);
    }
    event.faces = vec![CornerFace {
        surface,
        curves_3d,
        uv_curves,
        orients,
    }];
    Ok(())
}

/// A straight UV line from `a` to `b` over `ivl`.
fn uv_segment(a: Point2, b: Point2, ivl: [f64; 2]) -> Result<Curve> {
    let points = [Point3::new(a.x, a.y, 0.0), Point3::new(b.x, b.y, 0.0)];
    Ok(Curve::from(BSplineCurve::create_canonical(
        &CanonicalData {
            dim: 2,
            degree: 1,
            control_points: &points,
            form: BSplineCurveForm::Unspecified,
            knot_multiplicities: &[2, 2],
            knots: &ivl,
            knot_type: KnotType::Unspecified,
            weights: None,
            trim: None,
        },
    )?))
}

/// The bilinear patch over `span x span` with corners `p00` (`(lo, lo)`), `p10` (u at its
/// top), `p11` and `p01` (v at its top), bounded by its four sides counterclockwise in its
/// parameters from `p00`: each side the line between its corners and, in the parameters,
/// the straight side of the square (exact: a bilinear patch is linear along its sides).
fn bilinear_face(
    p00: Point3,
    p10: Point3,
    p11: Point3,
    p01: Point3,
    span: [f64; 2],
) -> Result<CornerFace> {
    let (lo, hi) = (span[0], span[1]);
    let net = BSplineSurface::create_canonical(&CanonicalSurfaceData {
        u_degree: 1,
        v_degree: 1,
        control_points: &[p00, p01, p10, p11],
        form: BSplineSurfaceForm::Unspecified,
        u_multiplicities: &[2, 2],
        v_multiplicities: &[2, 2],
        u_knots: &span,
        v_knots: &span,
        knot_type: KnotType::Unspecified,
        weights: None,
        uv_domain: None,
    })?;
    let corners = [
        (p00, Point2::new(lo, lo)),
        (p10, Point2::new(hi, lo)),
        (p11, Point2::new(hi, hi)),
        (p01, Point2::new(lo, hi)),
    ];
    let mut curves_3d = Vec::new();
    let mut uv_curves = Vec::new();
    for (i, &(p, a)) in corners.iter().enumerate() {
        let (q, b) = *corners.get((i + 1) % 4).ok_or_else(|| {
            SolidError::blend_failed(OP_EDGES, "a corner past the bilinear face's four")
        })?;
        let curve = Curve::from(Line::create_line_segment(3, p, q, None)?);
        let ivl = curve.natural_interval();
        uv_curves.push(uv_segment(a, b, [ivl.min(), ivl.max()])?);
        curves_3d.push(curve);
    }
    Ok(CornerFace {
        surface: Surface::from(net),
        curves_3d,
        uv_curves,
        orients: vec![OrientType::Same; 4],
    })
}

/// The corner patch's faces (module docs): n bilinear quadrilaterals round the crossings'
/// centroid C, quadrilateral m between C, the middle of the side before its crossing `X_m`,
/// `X_m` and the middle of the side after it, the crossings in turn counterclockwise about
/// the normal out of the material ([`ChamferPatch::around`]), so each face's normal points
/// out of it. At four the quadrilaterals are the quarters of one bilinear patch, the
/// crossings' own, and are that one face, over `[-1, 1]^2` with C at its centre.
///
/// # Errors
///
/// `SM_ERR` where the order does not cover the crossings; those of the surface and curve
/// constructions.
fn patch_faces(event: &mut ChamferCorner, patch: &ChamferPatch) -> Result<()> {
    let err = || SolidError::blend_failed(OP_EDGES, "the chamfer patch's faces could not be built");
    let xs = &event.definition.crossings;
    let n = patch.around.len();
    if n != xs.len() || n < 4 {
        return Err(err());
    }
    let at = |m: usize| -> Result<Point3> {
        patch
            .around
            .get(m % n)
            .and_then(|&i| xs.get(i))
            .copied()
            .ok_or_else(err)
    };
    let mut faces = Vec::new();
    if n == 4 {
        faces.push(bilinear_face(at(0)?, at(1)?, at(2)?, at(3)?, [-1.0, 1.0])?);
    } else {
        for m in 0..n {
            let (prev, x, next) = (at(m + n - 1)?, at(m)?, at(m + 1)?);
            faces.push(bilinear_face(
                patch.centre,
                (prev + x) * 0.5,
                x,
                (x + next) * 0.5,
                [0.0, 1.0],
            )?);
        }
    }
    event.faces = faces;
    Ok(())
}

/// The corner's faces: the facet's plane, or the corner patch's bilinear faces, each bounded
/// by its loop (`MakeFaceWithCurves` in the fillet B-rep's infinite region, its surface's
/// natural UV domain, `SM_OT_SAME`); the surfaces join `corner.surfaces` and the faces
/// `corner.surface_faces` (the corner's patch faces for the stitch, which glues them to
/// each other and to the chamfers' end sections, split at the patch's vertices on them).
///
/// # Errors
///
/// [`SolidError::null_pointer`] without a face, the fillet B-rep or a new face; those of the
/// face construction.
pub fn create_chamfer_patch(
    corner: &mut FilletCorner,
    env: &mut CornerPatchEnv<'_, '_>,
) -> Result<()> {
    let nul = || SolidError::blend_missing(OP_PATCH, "the chamfer corner's event or topology");
    let event = corner.chamfer_corner.as_mut().ok_or_else(nul)?;
    let faces = std::mem::take(&mut event.faces);
    if faces.is_empty() {
        return Err(nul());
    }
    let fillet = env.fillet.as_mut().ok_or_else(nul)?;
    for face in faces {
        let input = FaceCurvesInput {
            loops: vec![face.curves_3d.len()],
            curves_3d: Some(face.curves_3d.into_iter().map(Some).collect()),
            uv_curves: Some(face.uv_curves),
            orientations: face.orients,
            loop_points: Vec::new(),
        };
        let uv_domain = face.surface.natural_uv_domain_with_nurb(env.k)?;
        let region = fillet.brep.infinite_region();
        let key_of = fillet.notify.key_of;
        let face_ref = |f| key_of(EntityRef::Face(f));
        let made = make_face_with_curves(
            env.k,
            fillet.brep,
            region,
            input,
            face.surface.clone(),
            &uv_domain,
            OrientType::Same,
            None,
            &face_ref,
            &mut fillet.notify,
        )?;
        let made = made.face.ok_or_else(nul)?;
        corner.surfaces.push(face.surface);
        corner.surface_faces.push(made);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The square corner's frame: the edges along u, v, w; face i between edges i - 1
    /// and i, so face 0 (edges w, u) is v = 0, face 1 (u, v) is w = 0, face 2 (v, w) is
    /// u = 0, each normal positive into the corner.
    fn square(widths: [f64; 3]) -> (Vec<Vec3>, Vec<ChamferPlane>) {
        let t = [Vec3::X, Vec3::Y, Vec3::Z];
        let w: Vec<ChamferWidth> = widths
            .iter()
            .map(|&d| ChamferWidth::Offsets { prev: d, next: d })
            .collect();
        let planes = chamfer_planes(&t, &w).unwrap();
        for ((_, s), d) in planes.iter().zip(widths) {
            assert!(
                (s[0] - d).abs() < 1e-15 && (s[1] - d).abs() < 1e-15,
                "{s:?}"
            );
        }
        (
            vec![Vec3::Y, Vec3::Z, Vec3::X],
            planes.into_iter().map(|(p, _)| p).collect(),
        )
    }

    /// `unequal_chamfers_at_a_corner`: setbacks a = 2 (along u), b = 3 (v), c = 4 (w):
    /// M = ((b + c - a) / 2, (a + c - b) / 2, (a + b - c) / 2) = (2.5, 1.5, 0.5).
    #[test]
    fn mitre_point_is_the_three_planes_solve() {
        let (mu, planes) = square([2.0, 3.0, 4.0]);
        let m = mitre_point(&planes, &[1.0; 3]).unwrap();
        assert!((m - Vec3::new(2.5, 1.5, 0.5)).length() < 1e-14, "{m:?}");
        // X on v = 0 (chamfers w: u + v = 4 and u: v + w = 2): (4, 0, 2); on w = 0:
        // (3, 2, 0); on u = 0: (0, 4, 3)
        let xs = crossings(&mu, &planes, &[1.0; 3]).unwrap();
        let want = [
            Vec3::new(4.0, 0.0, 2.0),
            Vec3::new(3.0, 2.0, 0.0),
            Vec3::new(0.0, 4.0, 3.0),
        ];
        for (x, w) in xs.iter().zip(want) {
            assert!((*x - w).length() < 1e-14, "{x:?} {w:?}");
        }
        assert!(mitre_tests(&mu, &planes, &[1.0; 3])
            .unwrap()
            .iter()
            .all(|&g| g > 0.0));
        // the facet 10u + 9v + 4w = 48 holds V on its negative side and M on V's side
        let (n, d) = facet_plane(&xs, 1e-12).unwrap();
        let want = Vec3::new(10.0, 9.0, 4.0) * (1.0 / 197.0_f64.sqrt());
        assert!((n - want).length() < 1e-14 && (d - 48.0 / 197.0_f64.sqrt()).abs() < 1e-13);
        assert_eq!(facet_fails(&mu, &planes, &xs, (n, d), 1e-12), None);
    }

    /// a = b = 1, c = 3: M = (1.5, 1.5, -0.5) is outside; c (chamfer 2) is too wide,
    /// up to a factor (a + b) / c = 2/3, and the facet cuts the chamfers.
    #[test]
    fn a_chamfer_too_wide_has_its_bound() {
        let (mu, planes) = square([1.0, 1.0, 3.0]);
        assert!(mitre_tests(&mu, &planes, &[1.0; 3])
            .unwrap()
            .iter()
            .any(|&g| g < 0.0));
        let (i, hi) = too_wide(&mu, &planes).unwrap();
        assert_eq!(i, 2);
        assert!((hi - 2.0 / 3.0).abs() < 1e-15, "{hi}");
        let xs = crossings(&mu, &planes, &[1.0; 3]).unwrap();
        let plane = facet_plane(&xs, 1e-12).unwrap();
        assert_eq!(facet_fails(&mu, &planes, &xs, plane, 1e-12), Some("cuts"));
    }

    /// A constant chord d on a square edge sets back d / sqrt 2; on a 120 degree edge
    /// d / sqrt 3.
    #[test]
    fn a_chord_sets_back_by_the_half_angle() {
        let t = [Vec3::X, Vec3::Y, Vec3::Z];
        let planes = chamfer_planes(&t, &[ChamferWidth::Chord(1.0); 3]).unwrap();
        for (_, s) in &planes {
            assert!((s[0] - std::f64::consts::FRAC_1_SQRT_2).abs() < 1e-15);
        }
        // a ball of offsets r on a wedge of angle phi touches r cot(phi / 2) from the edge
        let phi = 2.0 * std::f64::consts::PI / 3.0;
        let qa = Vec3::new(1.0, 0.0, 0.0);
        let qb = Vec3::new(phi.cos(), phi.sin(), 0.0);
        let t = [qa, Vec3::Z, qb];
        let planes = chamfer_planes(
            &t,
            &[ChamferWidth::Offsets {
                prev: 2.0,
                next: 2.0,
            }; 3],
        )
        .unwrap();
        let s = planes.get(1).unwrap().1;
        assert!((s[0] - 2.0 / 3.0_f64.sqrt()).abs() < 1e-14, "{s:?}");
    }

    #[test]
    fn solve3_refuses_parallel_planes() {
        assert!(solve3([Vec3::X, Vec3::X, Vec3::Z], [1.0, 2.0, 3.0]).is_none());
    }

    /// The 2/3/4 square corner's planes meet at large angles: the chamfer normals
    /// (0, 1, 1), (1, 0, 1), (1, 1, 0) over sqrt 2 have D = 2 / (2 sqrt 2) = 1/sqrt 2 and
    /// cofactors (-1, 1, 1) / 2 and its turns, each sqrt 3 / 2, so M's bound is
    /// u (s + |M|) 3 sqrt 6 / 2 with s = 4 and |M| = sqrt 8.75: about 4.5e-14. Each `X_i`'s
    /// rows, a face's axis e and the two chamfer normals through it, (e + f) / sqrt 2 and
    /// (e + g) / sqrt 2, have D = 1/2 and cofactors sqrt 3 / 2, 1/sqrt 2 and 1/sqrt 2, so
    /// 2 (sqrt 3 / 2 + sqrt 2) = 4.56; with `|X_i|` at most `|(0, 4, 3)|` = 5 their bound is at
    /// most u 9 x 4.56 = 7.3e-14. Both are under the gate 1e-13 (1 + s) even with V at the
    /// origin (at the box's corner (40, 30, 20) it is 4.5e-12).
    #[test]
    fn a_square_corner_is_determined() {
        let (mu, planes) = square([2.0, 3.0, 4.0]);
        let m = mitre_point(&planes, &[1.0; 3]).unwrap();
        let want = ROW_ULPS * f64::EPSILON * (4.0 + 8.75_f64.sqrt()) * 1.5 * 6.0_f64.sqrt();
        let got = solve3_error(mitre_rows(&planes).unwrap(), m, 4.0);
        assert!((got - want).abs() <= 1e-12 * want, "{got} {want}");
        let xs = crossings(&mu, &planes, &[1.0; 3]).unwrap();
        let cofactors = 2.0 * (0.75_f64.sqrt() + std::f64::consts::SQRT_2);
        let bound = ROW_ULPS * f64::EPSILON * 9.0 * cofactors;
        let error = points_error(&mu, &planes, &xs, 4.0);
        assert!(error <= bound * (1.0 + 1e-12), "{error} {bound}");
        assert!(bound < GATE * 5.0);
    }

    /// A low apex of three planes at V = 0: faces `z = -eps u_k . (x, y)`, the `u_k` unit at 0,
    /// 120 and 240 degrees, the ridge between the two faces other than k along
    /// `(-u_k, -eps/2)`, each chamfered by a ball of offsets r. The face normals (towards
    /// the blend's side) and chamfer planes in ring order (ridge l is edge use l; face i,
    /// between ridges i - 1 and i, is face i + 1), and the setback.
    fn low_apex(eps: f64, r: f64) -> (Vec<Vec3>, Vec<ChamferPlane>, f64) {
        let u: Vec<Vec3> = (0..3)
            .map(|k| {
                let a = f64::from(k) * 2.0 * std::f64::consts::PI / 3.0;
                Vec3::new(a.cos(), a.sin(), 0.0)
            })
            .collect();
        let t: Vec<Vec3> = u
            .iter()
            .map(|uk| Vec3::new(-uk.x, -uk.y, -eps / 2.0).unitize().unwrap())
            .collect();
        let planes = chamfer_planes(&t, &[ChamferWidth::Offsets { prev: r, next: r }; 3]).unwrap();
        let s = planes[0].1[0];
        let mu = (0..3)
            .map(|i| {
                let k = (i + 1) % 3;
                let m = Vec3::new(eps * u[k].x, eps * u[k].y, 1.0)
                    .unitize()
                    .unwrap();
                // the ridge not in face k is ridge k
                if m.dot(t[k]) < 0.0 {
                    -m
                } else {
                    m
                }
            })
            .collect();
        (mu, planes.into_iter().map(|(p, _)| p).collect(), s)
    }

    /// The low apex at eps = 1/100, r = 400 (`a_nearly_flat_apex_is_refused` in
    /// `wr697_ball_corner.rs` has the closed forms): the existing checks pass it (the face
    /// normals' determinant (3 sqrt 3 / 2) eps^2 / (1 + eps^2)^(3/2) = 2.6e-4 is far above
    /// the 1e-6 the event declines at; M is 0.03 inside every face and each `X_k` as far on
    /// the kept side of the third chamfer), but the points' bound, about
    /// 8 EPSILON (s + |X|) 4 / eps = 5.3e-12, is over the gate 1e-13 (1 + s) = 4.5e-13.
    /// At eps = 5/12, r = 13/5 it is about 3.4e-14 (to first order in eps), under the gate,
    /// and M = (0, 0, -5/16).
    #[test]
    fn a_nearly_flat_apex_passes_the_signs_but_is_not_determined() {
        let eps = 1.0 / 100.0;
        let (mu, planes, s) = low_apex(eps, 400.0);
        let gate = GATE * (1.0 + s);
        let det = mu[0].dot(mu[1].cross(mu[2])).abs();
        assert!(det > 100.0 * EFF_ZERO_SQRT, "{det}");
        let ones = [1.0; 3];
        let tests = mitre_tests(&mu, &planes, &ones).unwrap();
        assert!(tests.iter().all(|&g| g > 0.02), "{tests:?}");
        let xs = crossings(&mu, &planes, &ones).unwrap();
        let error = points_error(&mu, &planes, &xs, s);
        assert!(error > 5.0 * gate, "{error} {gate}");

        let (mu, planes, s) = low_apex(5.0 / 12.0, 13.0 / 5.0);
        let xs = crossings(&mu, &planes, &ones).unwrap();
        let error = points_error(&mu, &planes, &xs, s);
        assert!(error < 0.5 * GATE * (1.0 + s), "{error}");
        let m = mitre_point(&planes, &ones).unwrap();
        assert!(
            (m - Vec3::new(0.0, 0.0, -5.0 / 16.0)).length() < 1e-14,
            "{m:?}"
        );
    }
}
