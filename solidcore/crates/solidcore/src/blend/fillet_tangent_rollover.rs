//! Tangent rollover of a whole rail (not upstream; solidcore-native).
//!
//! A rolling ball's support on each side is a chain of faces meeting smoothly, not one face.
//! Where a face beside a round is narrower than the ball reaches and its far edge is smooth
//! (G1) to the face beyond, the ball does not leave the body there: it rolls on across that
//! edge onto the face beyond, and the face it rolled off lies wholly inside the round's band
//! (it is consumed). A concave round beside a strip whose far rim is already rounded is the
//! case: rolling on the strip's plane continued, the ball's contact lies past the strip, over
//! the rim's round, and is refused as a blend leaving its face; rolling on the rim's round, it
//! is an ordinary round between the wall and that round.
//!
//! SMLib's rollover recovery (`ProcessRollOver`) switches a rail's support where it crosses a
//! tangent edge partway along (`FilletGeomType::TangentRollover`); a rail that never touches its
//! face is a full rollover there, which upstream builds only round a cliff
//! ("Unimplemented Tangent Rollover Case", [`FilletSolver::process_full_roll_over`]). Here the
//! full tangent rollover is decided before the trace, in the round's section, where it is exact
//! ([`tangent_rollover_supports`]), and the solver rolls the ball on the face beyond from the
//! start, as [`crate::blend::fillet_outline`] gives the solver its rim supports. The same
//! section decides a lone round along a straight edge between faces each a plane or a circular
//! cylinder parallel to the edge (#15767: two joined cylinders, the ball rolling over one's
//! seam), whose section is the same all along the edge (`cylinder_rollover_supports`). It decides
//! a round of several the same way where no other round's edge meets the round's edge or a face
//! it may roll off or onto (#18411: both rims of a pocket). Any other round (one of a chain,
//! #15767) is decided from its ball's sections along its edge (the session's
//! `roll_over_tangent_edges`).
//!
//! | Rust | Upstream |
//! |------|----------|
//! | [`tangent_rollover_supports`] | none (solidcore-native) |
//! | `cylinder_rollover_supports`, `cylinder_section`, `CylinderSection`, `prismatic_profile`, `Profile`, `offset_meet`, `full_span_line` (private) | none (solidcore-native) |
//! | [`rolled_edge_point`] | none (solidcore-native) |
//! | `section_seeds`, `SectionSeed` (crate) | none (solidcore-native) |
//! | `planar_section`, `PlanarSection` (crate) | none (solidcore-native) |
//!
//! Deviations: none of upstream's; there is no recursion.
//!
//! [`FilletSolver::process_full_roll_over`]: crate::blend::fillet_solver::p1::FilletSolver::process_full_roll_over

use crate::blend::fillet_solver::p1::{test_manifold_tangent_rollover, FilletSolver};
use crate::error::{Result, SolidError};
use crate::geom::global_solver::SolverOperation;
use crate::kernel::Kernel;
use crate::math::{Point3, Vec3, BIG_DOUBLE};
use crate::nurbs::knots::Side as KnotSide;
use crate::surface::surface::p4::NoObject;
use crate::surface::surface::p5::DropPointOptions;
use crate::topology::face::p4 as face_p4;
use crate::topology::{edge, Brep, EdgeId, FaceId};

const OP: &str = "blend::fillet_tangent_rollover";

/// The tolerance the edges' curves are recognised as lines within.
const CURVE_TOL: f64 = 1.0e-8;

/// The sine of the angle two directions may make and still count as parallel.
const DIR_TOL: f64 = 1.0e-9;

/// A face the ball rolls onto across a smooth edge of the face beside the round.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Rollover {
    /// The smooth edge the ball rolls over (the far edge of the face beside the round).
    pub edge: EdgeId,
    /// The face beyond it, which the ball rolls on.
    pub face: FaceId,
    /// Whether that face's surface normal is opposite the rolled-off face's across the edge
    /// (`sm_TestManifoldTangentRollover`'s second answer): the offset changes sign.
    pub opposite_normals: bool,
    /// Whether the face beyond is a plane or a circular cylinder parallel to the filleted
    /// straight edge, so that the closed form's one section ([`tangent_rollover_supports`])
    /// is the round's section all along the edge on it too (#18411). `false` for a rollover
    /// proposed from the ball's sections.
    pub prismatic_beyond: bool,
}

/// The faces a constant round's ball rolls onto across a smooth far edge of the face on each
/// side, or `None` on a side where it touches its own face.
///
/// `filleted` is the round's edge, `faces[i]` the face of its rail `i` and `offsets[i]` the
/// solver's signed offset of that face's surface (the ball's centre lies at the face's point
/// plus its surface normal times the offset). Decided in the section across the middle of a
/// straight edge between two planes: the ball's centre is where the offset lines meet, and its
/// contact on a face is that centre's foot on the face's line. A side rolls over when the face's
/// other edge that runs the filleted edge's whole length (a straight edge parallel to it whose
/// span covers it) is tangent to the face beyond (`sm_TestManifoldTangentRollover` within
/// `tangency_tol_rad`) and the contact lies past it, by more than the body's tolerance. A
/// straight edge where a face is a circular cylinder whose axis is parallel to the edge (the
/// other a plane or such a cylinder) is decided in the same section by
/// `cylinder_rollover_supports`. A round with no such side gives `[None, None]`; any other
/// round, outside these sections, `None` (#18411: the session tells a round the closed form
/// decides from one it does not).
///
/// # Errors
///
/// Those of the topology queries, the curves' recognisers, the evaluations and the tangency
/// test; `SM_ERR` for a face of a straight edge without a surface (the planes' section,
/// `planar_section`, reads each face in turn and stops at the first that is not a plane).
pub fn tangent_rollover_supports(
    k: &Kernel,
    target: &mut Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
    tangency_tol_rad: f64,
) -> Result<Option<[Option<Rollover>; 2]>> {
    let none = [None, None];
    let tol = target.tolerance();
    let Some(PlanarSection {
        d,
        ends,
        m,
        x,
        y,
        n2,
        centre,
    }) = planar_section(k, target, filleted, faces, offsets)?
    else {
        return cylinder_rollover_supports(k, target, filleted, faces, offsets, tangency_tol_rad);
    };
    let to2 = |p: Point3| {
        let v = p - m;
        (v.dot(x), v.dot(y))
    };
    let mut out = none;
    for (((slot, &(na, nb)), &offset), &face) in out.iter_mut().zip(&n2).zip(&offsets).zip(&faces) {
        // the contact: the centre's foot on the face's line
        let contact = (centre.0 - na * offset, centre.1 - nb * offset);
        for e in face_p4::edges(target, face, None, false, None)? {
            if e == filleted {
                continue;
            }
            let Some(far_curve) = edge::curve(target, e)?.cloned() else {
                continue;
            };
            let Some((point, far_vector)) = far_curve.is_line(k, 3, CURVE_TOL)? else {
                continue;
            };
            let Ok(fd) = far_vector.unitize() else {
                continue;
            };
            if d.cross(fd).length() > DIR_TOL {
                continue;
            }
            // its span along the filleted edge covers the filleted edge's
            let far_ivl = edge::interval(target, e)?;
            let along = |p: Point3| (p - point).dot(d);
            let r0 = along(far_curve.evaluate_point(k, far_ivl.min())?);
            let r1 = along(far_curve.evaluate_point(k, far_ivl.max())?);
            let [a, b] = ends.map(along);
            if r0.min(r1) > a.min(b) + tol || r0.max(r1) < a.max(b) - tol {
                continue;
            }
            // the far edge's point in the section, and how far along the face it lies
            let t = to2(point);
            let width = (t.0 * t.0 + t.1 * t.1).sqrt();
            if width <= tol {
                continue;
            }
            let reach = (contact.0 * t.0 + contact.1 * t.1) / width;
            if reach.is_nan() || reach <= width + tol {
                continue;
            }
            let (tangent, opposite_normals) =
                test_manifold_tangent_rollover(k, target, e, tangency_tol_rad)?;
            if !tangent {
                continue;
            }
            let Some(&beyond) = edge::faces(target, e, None)?.iter().find(|&&f| f != face) else {
                continue;
            };
            // the face beyond must carry the ball: its offset by the ball's offset (signed by the
            // normals across the edge) may not fold across the edge, or no ball rolls on it (a
            // convex round larger than the convex round beyond); the side is then left to the
            // refusals as before
            let offset_beyond = if opposite_normals { -offset } else { offset };
            let far_point = m + x * t.0 + y * t.1;
            if offset_folds(k, target, beyond, far_point, d, offset_beyond)? {
                continue;
            }
            let frame = [d, x, y];
            let prismatic_beyond =
                prismatic_profile(k, target, beyond, far_point, m, frame, tol)?.is_some();
            *slot = Some(Rollover {
                edge: e,
                face: beyond,
                opposite_normals,
                prismatic_beyond,
            });
            break;
        }
    }
    Ok(Some(out))
}

/// A constant round's section across the middle of a straight edge between two planes, in
/// closed form ([`planar_section`]).
#[derive(Debug, Clone, Copy)]
pub(crate) struct PlanarSection {
    /// The edge's unit direction.
    pub d: Vec3,
    /// The edge's ends.
    pub ends: [Point3; 2],
    /// The edge's middle, the section's origin.
    pub m: Point3,
    /// The section's x axis: the first face's surface normal there, square to the edge.
    pub x: Vec3,
    /// The section's y axis, `d x x`.
    pub y: Vec3,
    /// Each face's surface normal in the section's frame.
    pub n2: [(f64, f64); 2],
    /// The ball's centre in the section's frame, where the offset lines meet.
    pub centre: (f64, f64),
}

impl PlanarSection {
    /// The ball's contact on face `rail`'s plane (the centre's foot on its line), in space,
    /// for the offset `offset` of that face; `None` for a rail other than 0 or 1.
    pub(crate) fn contact(&self, rail: usize, offset: f64) -> Option<Point3> {
        let &(na, nb) = self.n2.get(rail)?;
        let c = (self.centre.0 - na * offset, self.centre.1 - nb * offset);
        Some(self.m + self.x * c.0 + self.y * c.1)
    }
}

/// The section of the constant round of signed offsets `offsets` on `faces` across the middle
/// of the straight edge `filleted`, both faces planes: the ball's centre is where the offset
/// lines meet (the ball's centre lies at a face's point plus its surface normal times the
/// offset). `None` where the edge has no curve or is not a line, a face is not a plane or its
/// normal does not drop or is along the edge, or the faces' lines are parallel in the section.
///
/// # Errors
///
/// Those of the curve's recogniser, the evaluations and the drops; `SM_ERR` for a face without
/// a surface.
pub(crate) fn planar_section(
    k: &Kernel,
    target: &Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
) -> Result<Option<PlanarSection>> {
    let tol = target.tolerance();
    let Some(curve) = edge::curve(target, filleted)?.cloned() else {
        return Ok(None);
    };
    let Some((_, vector)) = curve.is_line(k, 3, CURVE_TOL)? else {
        return Ok(None);
    };
    let Ok(d) = vector.unitize() else {
        return Ok(None);
    };
    let ivl = edge::interval(target, filleted)?;
    let ends = [
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    ];
    let m = curve.evaluate_point(k, ivl.evaluate(0.5))?;
    // each face a plane, its surface normal at the section, square to the edge
    let mut normals = [Vec3::UNDEF; 2];
    for (normal, &face) in normals.iter_mut().zip(&faces) {
        let surface = crate::topology::face::p2::surface(target, face)?
            .ok_or_else(|| SolidError::blend_failed(OP, "a face of the edge has no surface"))?
            .clone_for_evaluation()?;
        if !surface.is_planar(k, tol)?.is_planar {
            return Ok(None);
        }
        let domain = surface.natural_uv_domain_with_nurb(k)?;
        let dropped =
            surface.drop_point::<NoObject>(k, m, &domain, &DropPointOptions::default())?;
        if !dropped.success {
            return Ok(None);
        }
        let n = surface.evaluate_normal(k, dropped.result_uv, KnotSide::Left, KnotSide::Left)?;
        let Ok(n) = (n - d * n.dot(d)).unitize() else {
            return Ok(None);
        };
        *normal = n;
    }
    // the section's frame: x along the first face's normal, y square to it and the edge
    let x = normals[0];
    let y = d.cross(x);
    let n2 = normals.map(|n| (n.dot(x), n.dot(y)));
    // the ball's centre: n_i . p = offset_i (both face lines pass through the section origin)
    let [(a0, b0), (a1, b1)] = n2;
    let det = a0 * b1 - b0 * a1;
    if det.is_nan() || det.abs() <= DIR_TOL {
        return Ok(None);
    }
    let [o0, o1] = offsets;
    let centre = ((o0 * b1 - b0 * o1) / det, (a0 * o1 - o0 * a1) / det);
    Ok(Some(PlanarSection {
        d,
        ends,
        m,
        x,
        y,
        n2,
        centre,
    }))
}

/// A face's section square to a straight edge along `d`, in the section's frame (its origin the
/// edge's middle): the line of a plane that contains the edge's direction, or the circle of a
/// circular cylinder whose axis is parallel to the edge ([`prismatic_profile`]). Either surface
/// is the same in every such section, so a round between two of them has the same section at
/// every point of its edge.
#[derive(Debug, Clone, Copy, PartialEq)]
enum Profile {
    /// A plane: the points `q` with `n . q = h`.
    Line {
        /// The surface's unit normal.
        n: (f64, f64),
        /// The line's signed distance from the origin along `n`.
        h: f64,
    },
    /// A circular cylinder.
    Circle {
        /// Its axis.
        c: (f64, f64),
        /// Its radius.
        radius: f64,
        /// `+1` where the surface's normal points away from the axis, `-1` where toward it.
        side: f64,
    },
}

impl Profile {
    /// Whether `q` lies on the profile within `tol`.
    fn holds(self, q: (f64, f64), tol: f64) -> bool {
        let miss = match self {
            Self::Line { n, h } => n.0 * q.0 + n.1 * q.1 - h,
            Self::Circle { c, radius, .. } => {
                let v = (q.0 - c.0, q.1 - c.1);
                (v.0 * v.0 + v.1 * v.1).sqrt() - radius
            }
        };
        miss.abs() <= tol
    }

    /// The point of the profile the ball of centre `centre` touches, the profile's surface
    /// offset by `offset` passing through `centre`: the centre's foot on a line, its radial
    /// point on a circle. `None` for a centre on a circle's axis.
    fn contact(self, offset: f64, centre: (f64, f64)) -> Option<(f64, f64)> {
        match self {
            Self::Line { n, .. } => Some((centre.0 - n.0 * offset, centre.1 - n.1 * offset)),
            Self::Circle { c, radius, .. } => {
                let v = (centre.0 - c.0, centre.1 - c.1);
                let l = (v.0 * v.0 + v.1 * v.1).sqrt();
                (l > 0.0).then(|| (c.0 + v.0 * (radius / l), c.1 + v.1 * (radius / l)))
            }
        }
    }

    /// How far along the profile from the origin (the edge's point, which the profile holds)
    /// the point `q` of it lies, in the sense the point `towards` of it lies from the origin:
    /// along a line the signed distance, round a circle the arc in that sense, in `[0, 2 pi
    /// radius)`. `None` where `towards` names no sense: within `tol` of the origin across the
    /// profile's direction there, or (on a circle) of the point opposite it.
    fn travel(self, towards: (f64, f64), q: (f64, f64), tol: f64) -> Option<f64> {
        match self {
            Self::Line { n, .. } => {
                let u = (-n.1, n.0);
                let s = towards.0 * u.0 + towards.1 * u.1;
                (s.abs() > tol).then(|| s.signum() * (q.0 * u.0 + q.1 * u.1))
            }
            Self::Circle { c, radius, .. } => {
                let angle = |p: (f64, f64)| (p.1 - c.1).atan2(p.0 - c.0);
                let origin = angle((0.0, 0.0));
                let s = (angle(towards) - origin).sin();
                (s.abs() * radius > tol).then(|| {
                    radius
                        * (s.signum() * (angle(q) - origin)).rem_euclid(2.0 * std::f64::consts::PI)
                })
            }
        }
    }

    /// Whether the profile, leaving the origin in the sense the point `towards` of it lies in
    /// ([`Profile::travel`]), runs along `direction` (a unit vector in the section): its unit
    /// tangent there in that sense makes an angle below 60 degrees with `direction`. `false`
    /// where `towards` names no sense, and for a non-finite value.
    fn leads_into(self, towards: (f64, f64), direction: (f64, f64), tol: f64) -> bool {
        let tangent = match self {
            Self::Line { n, .. } => {
                let u = (-n.1, n.0);
                let s = towards.0 * u.0 + towards.1 * u.1;
                if s.is_nan() || s.abs() <= tol {
                    return false;
                }
                (u.0 * s.signum(), u.1 * s.signum())
            }
            Self::Circle { c, radius, .. } => {
                let angle = |p: (f64, f64)| (p.1 - c.1).atan2(p.0 - c.0);
                let origin = angle((0.0, 0.0));
                let s = (angle(towards) - origin).sin();
                if s.is_nan() || s.abs() * radius <= tol {
                    return false;
                }
                // counterclockwise round the axis at the origin, turned by the sense
                let r = (-c.0 / radius, -c.1 / radius);
                (-r.1 * s.signum(), r.0 * s.signum())
            }
        };
        let cos = tangent.0 * direction.0 + tangent.1 * direction.1;
        cos > 0.5
    }

    /// The profile's surface offset by `offset` along its normal (where the ball's centre lies):
    /// a line moved along its normal, a circle of radius `radius + side offset`.
    fn offset(self, offset: f64) -> Self {
        match self {
            Self::Line { n, h } => Self::Line { n, h: h + offset },
            Self::Circle { c, radius, side } => Self::Circle {
                c,
                radius: radius + side * offset,
                side,
            },
        }
    }
}

/// The ball's centre in a section of the round between the profiles `profiles`, of offsets
/// `offsets`: of the two points where the offset profiles meet (each pair of a line and a
/// circle, or of two circles, meets in two points, mirror images across the line through the
/// circles' centres, or square to the line through the circle's centre), the one on the
/// origin's side of that mirror, which is the one nearer the origin (the edge's point, which
/// both profiles hold: the branch that starts there at radius 0). `None` for two lines (the
/// planes' closed form, [`planar_section`]), an offset circle of no radius above `tol`, offset
/// profiles that do not meet, and where the two points lie within `tol` of the same distance
/// from the origin (the edge's point on the mirror: faces tangent there). With the centre, the
/// sine of the angle the offset profiles cross at there (`None` where it is not above 0).
fn offset_meet(profiles: [Profile; 2], offsets: [f64; 2], tol: f64) -> Option<((f64, f64), f64)> {
    let [p0, p1] = profiles;
    let [o0, o1] = offsets;
    let (q0, q1) = (p0.offset(o0), p1.offset(o1));
    let points = match (q0, q1) {
        (Profile::Line { .. }, Profile::Line { .. }) => return None,
        (Profile::Line { n, h }, Profile::Circle { c, radius, .. })
        | (Profile::Circle { c, radius, .. }, Profile::Line { n, h }) => {
            if radius.is_nan() || radius <= tol {
                return None;
            }
            // the line's signed distance from the centre, and its foot there
            let along = h - (n.0 * c.0 + n.1 * c.1);
            if along.is_nan() || along.abs() > radius {
                return None;
            }
            let half = (radius * radius - along * along).sqrt();
            let foot = (c.0 + n.0 * along, c.1 + n.1 * along);
            let t = (-n.1, n.0);
            [
                (foot.0 + t.0 * half, foot.1 + t.1 * half),
                (foot.0 - t.0 * half, foot.1 - t.1 * half),
            ]
        }
        (
            Profile::Circle {
                c: c0, radius: r0, ..
            },
            Profile::Circle {
                c: c1, radius: r1, ..
            },
        ) => {
            if !(r0 > tol && r1 > tol) {
                return None;
            }
            let v = (c1.0 - c0.0, c1.1 - c0.1);
            let dist = (v.0 * v.0 + v.1 * v.1).sqrt();
            if dist.is_nan() || dist <= tol {
                return None;
            }
            let along = (dist * dist + r0 * r0 - r1 * r1) / (2.0 * dist);
            let across = r0 * r0 - along * along;
            if across.is_nan() || across < 0.0 {
                return None;
            }
            let half = across.sqrt();
            let u = (v.0 / dist, v.1 / dist);
            let foot = (c0.0 + u.0 * along, c0.1 + u.1 * along);
            [
                (foot.0 - u.1 * half, foot.1 + u.0 * half),
                (foot.0 + u.1 * half, foot.1 - u.0 * half),
            ]
        }
    };
    let [a, b] = points.map(|p| (p.0 * p.0 + p.1 * p.1).sqrt());
    let apart = (a - b).abs();
    if apart.is_nan() || apart <= tol {
        return None;
    }
    let [pa, pb] = points;
    let centre = if a < b { pa } else { pb };
    // the sine of the angle the offset profiles cross at there: a move of either profile by
    // `e` moves their meeting point by at most `e / sine`
    let normal = |q: Profile| match q {
        Profile::Line { n, .. } => Some(n),
        Profile::Circle { c, .. } => {
            let v = (centre.0 - c.0, centre.1 - c.1);
            let l = (v.0 * v.0 + v.1 * v.1).sqrt();
            (l > 0.0).then(|| (v.0 / l, v.1 / l))
        }
    };
    let (n0, n1) = (normal(q0)?, normal(q1)?);
    let sine = (n0.0 * n1.1 - n0.1 * n1.0).abs();
    if sine.is_nan() || sine <= 0.0 {
        return None;
    }
    Some((centre, sine))
}

/// The profile of `face` in the section through `m` square to the unit `d`, with axes `x` and
/// `y` ([`Profile`]), read at the point of its surface nearest `at`: its surface a circular
/// cylinder (a cone of the kernel's cylinder kind, read as its quadric) whose axis is parallel
/// to `d` within [`DIR_TOL`], or planar within `tol` with its normal square to `d` within
/// [`DIR_TOL`]. `None` for any other surface, a face without one, or where `at` does not drop on it.
///
/// # Errors
///
/// Those of the evaluations, the drop and the planarity test; [`SolidError::Unported`] and a
/// cancellation from reading the quadric.
fn prismatic_profile(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    at: Point3,
    m: Point3,
    [d, x, y]: [Vec3; 3],
    tol: f64,
) -> Result<Option<Profile>> {
    use crate::blend::fillet_exact::{BlendQuadric, QuadricKind};
    use crate::surface::surface::SurfaceKind;
    let Some(surface) = crate::topology::face::p2::surface(target, face)? else {
        return Ok(None);
    };
    let surface = surface.clone_for_evaluation()?;
    // the kind first: no other surface is dropped on
    let cylinder = if let SurfaceKind::Cone(cone) = surface.kind() {
        if !cone.is_cylinder() {
            return Ok(None);
        }
        true
    } else {
        if !surface.is_planar(k, tol)?.is_planar {
            return Ok(None);
        }
        false
    };
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped = surface.drop_point::<NoObject>(k, at, &domain, &DropPointOptions::default())?;
    if !dropped.success {
        return Ok(None);
    }
    let to2 = |v: Vec3| (v.dot(x), v.dot(y));
    if cylinder {
        let q = match BlendQuadric::from_surface(k, &surface, dropped.result_uv, tol) {
            Ok(q) => q,
            Err(e) if e.is_unported_or_cancelled() => return Err(e),
            Err(_) => return Ok(None),
        };
        let skew = q.dir.cross(d).length();
        if q.kind != QuadricKind::Cylinder
            || q.radius.is_nan()
            || q.radius <= tol
            || skew.is_nan()
            || skew > DIR_TOL
        {
            return Ok(None);
        }
        return Ok(Some(Profile::Circle {
            c: to2(q.point - m),
            radius: q.radius,
            side: q.side,
        }));
    }
    let n = surface.evaluate_normal(k, dropped.result_uv, KnotSide::Left, KnotSide::Left)?;
    let Ok(n) = n.unitize() else {
        return Ok(None);
    };
    let along_edge = n.dot(d).abs();
    if along_edge.is_nan() || along_edge > DIR_TOL {
        return Ok(None);
    }
    let Ok(n) = (n - d * n.dot(d)).unitize() else {
        return Ok(None);
    };
    let n = to2(n);
    let foot = to2(surface.evaluate_point(k, dropped.result_uv)? - m);
    Ok(Some(Profile::Line {
        n,
        h: n.0 * foot.0 + n.1 * foot.1,
    }))
}

/// A section of a round along a straight edge between faces each a plane or a circular
/// cylinder parallel to it, at least one a cylinder ([`cylinder_section`]).
struct CylinderSection {
    /// The edge's unit direction.
    d: Vec3,
    /// The edge's ends.
    ends: [Point3; 2],
    /// The edge's middle, the section's origin.
    m: Point3,
    /// The edge's parameter there.
    t_mid: f64,
    /// The section's axes, square to the edge and to each other.
    x: Vec3,
    /// See `x`.
    y: Vec3,
    /// Each face's profile ([`Profile`]), through the origin.
    profiles: [Profile; 2],
}

/// The section across the middle of the straight edge `filleted` whose faces are each a plane
/// containing its direction or a circular cylinder whose axis is parallel to it, at least one a
/// cylinder ([`prismatic_profile`]), each holding the edge's middle within `tol`. Its x axis is
/// the unit vector square to the edge nearest the coordinate axis least along it. `None` for
/// any other edge.
///
/// # Errors
///
/// Those of the curve's recogniser, the evaluations and [`prismatic_profile`].
fn cylinder_section(
    k: &Kernel,
    target: &Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    tol: f64,
) -> Result<Option<CylinderSection>> {
    let Some(curve) = edge::curve(target, filleted)?.cloned() else {
        return Ok(None);
    };
    let Some((_, vector)) = curve.is_line(k, 3, CURVE_TOL)? else {
        return Ok(None);
    };
    let Ok(d) = vector.unitize() else {
        return Ok(None);
    };
    let ivl = edge::interval(target, filleted)?;
    let ends = [
        curve.evaluate_point(k, ivl.min())?,
        curve.evaluate_point(k, ivl.max())?,
    ];
    let t_mid = ivl.evaluate(0.5);
    let m = curve.evaluate_point(k, t_mid)?;
    let axis = if d.x.abs() <= d.y.abs() && d.x.abs() <= d.z.abs() {
        Vec3::new(1.0, 0.0, 0.0)
    } else if d.y.abs() <= d.z.abs() {
        Vec3::new(0.0, 1.0, 0.0)
    } else {
        Vec3::new(0.0, 0.0, 1.0)
    };
    let Ok(x) = (axis - d * axis.dot(d)).unitize() else {
        return Ok(None);
    };
    let y = d.cross(x);
    let mut profiles = Vec::with_capacity(2);
    for &face in &faces {
        let Some(profile) = prismatic_profile(k, target, face, m, m, [d, x, y], tol)? else {
            return Ok(None);
        };
        if !profile.holds((0.0, 0.0), tol) {
            return Ok(None);
        }
        profiles.push(profile);
    }
    let Ok(profiles) = <[Profile; 2]>::try_from(profiles) else {
        return Ok(None);
    };
    if profiles.iter().all(|p| matches!(p, Profile::Line { .. })) {
        return Ok(None);
    }
    Ok(Some(CylinderSection {
        d,
        ends,
        m,
        t_mid,
        x,
        y,
        profiles,
    }))
}

/// [`tangent_rollover_supports`] for a straight edge between faces each a plane or a circular
/// cylinder parallel to it, at least one a cylinder ([`cylinder_section`]). Both surfaces are
/// the same in every section square to the edge, so the round's section at the edge's middle
/// is its section everywhere along it: the ball's centre is where the faces' offset profiles
/// meet ([`offset_meet`]), and its contact on a face the centre's foot on its profile. A side
/// rolls over when, of the face's other edges that run the filleted edge's whole length (a
/// straight edge parallel to it whose span covers it) and lie on its profile, the one nearest
/// the filleted edge along the profile in the sense the contact lies in ([`Profile::travel`])
/// is tangent to the face beyond (`sm_TestManifoldTangentRollover` within `tangency_tol_rad`),
/// the contact lies past it by more than the body's tolerance, and the face beyond is a plane
/// or a circular cylinder parallel to the edge too whose offset by the ball (signed by the
/// normals across the edge) does not fold across the edge there, which then holds all along
/// it. A round with no such side gives `[None, None]`; a round outside the section
/// ([`cylinder_section`]), `None`.
///
/// # Errors
///
/// Those of the topology queries, the curves' recognisers, the evaluations, the binormals and
/// the tangency test.
fn cylinder_rollover_supports(
    k: &Kernel,
    target: &mut Brep,
    filleted: EdgeId,
    faces: [FaceId; 2],
    offsets: [f64; 2],
    tangency_tol_rad: f64,
) -> Result<Option<[Option<Rollover>; 2]>> {
    let none = [None, None];
    let tol = target.tolerance();
    let Some(CylinderSection {
        d,
        ends,
        m,
        t_mid,
        x,
        y,
        profiles,
    }) = cylinder_section(k, target, filleted, faces, tol)?
    else {
        return Ok(None);
    };
    let Some((centre, sine)) = offset_meet(profiles, offsets, tol) else {
        return Ok(Some(none));
    };
    let to2 = |p: Point3| {
        let v = p - m;
        (v.dot(x), v.dot(y))
    };
    // a margin, to first order, for the skew the section's lines are allowed (the edge, each
    // face's axis or plane direction, the far edge, each within DIR_TOL of the edge's
    // direction): over the edge's length each moves at most `length DIR_TOL` in the section,
    // the ball's centre, where the two offset profiles meet at the angle of sine `sine`, about
    // twice that over `sine`, and a contact and the far edge by about that and their own line's
    // move. It is not a strict bound (a circle's contact moves with the centre scaled by its
    // radius over the offset one's, and near tangency the meet moves as the root of the skew),
    // but every term is orders of magnitude under the body's tolerance it is added to
    let [e0, e1] = ends;
    let length = e1.distance(e0);
    let skew = length * DIR_TOL;
    let drift = skew * (3.0 + 2.0 / sine);
    let mut out = none;
    for (((slot, &profile), &offset), &face) in
        out.iter_mut().zip(&profiles).zip(&offsets).zip(&faces)
    {
        let Some(contact) = profile.contact(offset, centre) else {
            continue;
        };
        let Some(reach) = profile.travel(contact, contact, tol) else {
            continue;
        };
        // the face lies on the side of the edge the contact moves to: the profile's direction at
        // the edge's point, in the contact's sense, is the face's binormal there (its normal x
        // the edge use's tangent, into the face), not the other way round the profile
        let Some(binormal) = face_binormal(k, target, filleted, face, t_mid)? else {
            continue;
        };
        if !profile.leads_into(contact, to2(m + binormal), tol) {
            continue;
        }
        // the face's nearest other edge along its profile, in the contact's sense, of those that
        // run the filleted edge's whole length
        let mut nearest: Option<(EdgeId, f64, (f64, f64))> = None;
        for e in face_p4::edges(target, face, None, false, None)? {
            if e == filleted {
                continue;
            }
            let Some(point) = full_span_line(k, target, e, d, ends, m, tol)? else {
                continue;
            };
            let t = to2(point);
            if !profile.holds(t, tol) {
                continue;
            }
            let Some(along) = profile.travel(contact, t, tol) else {
                continue;
            };
            if along > tol && nearest.is_none_or(|(_, a, _)| along < a) {
                nearest = Some((e, along, t));
            }
        }
        let Some((e, along, t)) = nearest else {
            continue;
        };
        if reach.is_nan() || reach <= along + tol + drift {
            continue;
        }
        let (tangent, opposite_normals) =
            test_manifold_tangent_rollover(k, target, e, tangency_tol_rad)?;
        if !tangent {
            continue;
        }
        let Some(&beyond) = edge::faces(target, e, None)?.iter().find(|&&f| f != face) else {
            continue;
        };
        // the face beyond carries the ball all along the edge: a plane or a circular cylinder
        // parallel to it (the same in every section), whose offset by the ball's offset (signed
        // by the normals across the edge) does not fold across the edge
        let far_point = m + x * t.0 + y * t.1;
        if prismatic_profile(k, target, beyond, far_point, m, [d, x, y], tol)?.is_none() {
            continue;
        }
        let offset_beyond = if opposite_normals { -offset } else { offset };
        if offset_folds(k, target, beyond, far_point, d, offset_beyond)? {
            continue;
        }
        *slot = Some(Rollover {
            edge: e,
            face: beyond,
            opposite_normals,
            prismatic_beyond: true,
        });
    }
    Ok(Some(out))
}

/// The unit binormal of `face`'s edge use of `filleted` at the edge's parameter `t` (the face
/// use's normal x the edge use's tangent, into the face, `SmEdgeuse::EvaluateBinormal` on the 3D
/// curve), or `None` where the face has no edge use of it or the binormal is not found.
///
/// # Errors
///
/// [`SolidError::Unported`] and a cancellation from the evaluation; those of the topology
/// queries.
fn face_binormal(
    k: &Kernel,
    target: &mut Brep,
    filleted: EdgeId,
    face: FaceId,
    t: f64,
) -> Result<Option<Vec3>> {
    for eu in edge::edgeuses(target, filleted, None, None)? {
        if crate::topology::edgeuse::p1::face(target, eu)? != Some(face) {
            continue;
        }
        let eval = crate::topology::edgeuse::p3::evaluate_binormal(
            k,
            target,
            eu,
            t,
            true,
            &crate::topology::edgeuse::p3::BinormalOptions::default(),
        );
        return match eval {
            Ok(eval) => Ok(eval.binormal.unitize().ok()),
            Err(e) if e.is_unported_or_cancelled() => Err(e),
            Err(_) => Ok(None),
        };
    }
    Ok(None)
}

/// The point of edge `e` at the station of `m` along the unit `d` (where it meets the plane
/// square to `d` through `m`) when it is a line parallel to `d` (within [`DIR_TOL`]) whose
/// span along `d` covers that of the points `ends` (within `tol`), else `None`.
///
/// # Errors
///
/// Those of the curve's recogniser and the evaluations.
fn full_span_line(
    k: &Kernel,
    target: &Brep,
    e: EdgeId,
    d: Vec3,
    ends: [Point3; 2],
    m: Point3,
    tol: f64,
) -> Result<Option<Point3>> {
    let Some(curve) = edge::curve(target, e)?.cloned() else {
        return Ok(None);
    };
    let Some((point, vector)) = curve.is_line(k, 3, CURVE_TOL)? else {
        return Ok(None);
    };
    let Ok(fd) = vector.unitize() else {
        return Ok(None);
    };
    let skew = d.cross(fd).length();
    if skew.is_nan() || skew > DIR_TOL {
        return Ok(None);
    }
    let ivl = edge::interval(target, e)?;
    let along = |p: Point3| (p - point).dot(d);
    let r0 = along(curve.evaluate_point(k, ivl.min())?);
    let r1 = along(curve.evaluate_point(k, ivl.max())?);
    let [a, b] = ends.map(along);
    if !(r0.min(r1) <= a.min(b) + tol && r0.max(r1) >= a.max(b) - tol) {
        return Ok(None);
    }
    // where the line meets the plane square to `d` through `m`, along its own direction (within
    // DIR_TOL of `d`, so the divisor is within 1e-18 of +-1)
    let across = fd.dot(d);
    if across.is_nan() || across.abs() <= 0.5 {
        return Ok(None);
    }
    Ok(Some(point + fd * ((m - point).dot(d) / across)))
}

/// The nearest point to `point` on the smooth edge a rail rolled over: where a ball of radius 0
/// rolling on the face beyond touches it, so the section of the rolled ball is searched from
/// there (the filleted edge's point lies off the face beyond, and a drop onto that face's
/// surface may land on another sheet of it, the far side of a cylinder). The edges are those
/// `from` (the face the rail rolled off) and `onto` (the face beyond) share that meet tangentially
/// within `tangency_tol_rad`; of these, the one whose nearest point is nearest `point` (two
/// half cylinders share two seams), the first of equally near ones in `onto`'s edge order.
/// `None` where they share no such edge, or the point drops onto none of them.
///
/// # Errors
///
/// Those of the topology queries, the tangency test and the drops.
pub fn rolled_edge_point(
    k: &Kernel,
    target: &mut Brep,
    from: FaceId,
    onto: FaceId,
    point: Point3,
    tangency_tol_rad: f64,
) -> Result<Option<Point3>> {
    let mut best: Option<(Point3, f64)> = None;
    for e in face_p4::edges(target, onto, None, false, None)? {
        if !edge::faces(target, e, None)?.contains(&from) {
            continue;
        }
        if !test_manifold_tangent_rollover(k, target, e, tangency_tol_rad)?.0 {
            continue;
        }
        let Some(curve) = edge::curve(target, e)?.cloned() else {
            continue;
        };
        let ivl = edge::interval(target, e)?;
        let dropped = curve.drop_point(
            k,
            &ivl,
            point,
            None,
            BIG_DOUBLE,
            None,
            SolverOperation::Minimize,
        )?;
        if !dropped.success {
            continue;
        }
        let on = curve.evaluate_point(k, dropped.parameter)?;
        let distance = (on - point).length();
        if best.is_none_or(|(_, d)| distance < d) {
            best = Some((on, distance));
        }
    }
    Ok(best.map(|(p, _)| p))
}

/// A guess of a rolling ball's section: the point dropped onto a face's surface for it, and
/// for a rail rolled over a smooth edge, the direction the ball rolls across that edge in.
#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct SectionSeed {
    /// The point dropped onto the face's surface.
    pub point: Point3,
    /// For a rolled rail, the direction from the filleted edge's point to the rolled-over
    /// edge's point: the side of a seam of the surface (a closed boss's, running up the
    /// rolled-over edge) on which the face beyond lies, as the drop's
    /// `DropPointOptions::tgt_seam_side`. `None` for every other rail, and where the two points
    /// coincide within the body's tolerance (no direction: [`seam_side_hint`]).
    pub seam_side: Option<Vec3>,
    /// Whether `point` is the rolled-over edge's point of a rolled rail (with or without a
    /// `seam_side`): the section is then solved on the surface extended past that edge.
    pub rolled: bool,
}

/// The seam-side hint of a rolled rail's seed ([`SectionSeed::seam_side`]): the direction from
/// the filleted edge's point `point` to the rolled-over edge's point `on`, `None` where the two
/// lie within `tol` of each other (a zero or near-zero vector names no side), or the distance
/// is not a number.
fn seam_side_hint(on: Point3, point: Point3, tol: f64) -> Option<Vec3> {
    let side = on - point;
    (side.length() > tol).then_some(side)
}

/// Where a ball of radius 0 touches each of `solver`'s faces at the point `point` of its
/// filleted edge, the guesses of the ball's section there: the edge point, or for a rail
/// rolled over a smooth far edge of its face (`solver.rolled_faces`) the point of that edge
/// nearest it ([`rolled_edge_point`]), on the face beyond. The edge point lies off the face
/// beyond, and dropped on its surface it may land where the section solve finds the ball
/// rolling on another sheet of that surface (the far side of a boss). The rolled-over edge's
/// point may lie on the surface's seam (a boss whose seam runs up that edge), where it has two
/// parameters: the one on the side the ball rolls onto is the guess
/// ([`SectionSeed::seam_side`]).
///
/// # Errors
///
/// Those of the topology queries and of [`rolled_edge_point`].
pub(crate) fn section_seeds(
    k: &Kernel,
    target: &mut Brep,
    solver: &FilletSolver,
    point: Point3,
) -> Result<[SectionSeed; 2]> {
    let mut seeds = [SectionSeed {
        point,
        seam_side: None,
        rolled: false,
    }; 2];
    let tol = target.tolerance();
    for (rail, seed) in seeds.iter_mut().enumerate() {
        let Some(onto) = solver.rolled_faces.get(rail).copied().flatten() else {
            continue;
        };
        let Some(own) = solver.edgeuse(rail)? else {
            continue;
        };
        let Some(from) = crate::topology::edgeuse::p1::face(target, own)? else {
            continue;
        };
        if let Some(p) = rolled_edge_point(k, target, from, onto, point, solver.tangency_tolerance)?
        {
            *seed = SectionSeed {
                point: p,
                seam_side: seam_side_hint(p, point, tol),
                rolled: true,
            };
        }
    }
    Ok(seeds)
}

/// Whether the offset of `face`'s surface by `offset` (along its surface normal) folds across the
/// direction square to `along` at the point of the face nearest `at`: `1 - kappa offset <= 0`, with
/// `kappa` the surface's normal curvature in that direction (`II(t, t) / I(t, t)`). A ball of that
/// signed offset cannot roll on the face there. A point the face does not take, a degenerate
/// frame, a factor `1 - kappa offset` not proved above its rounding (`gamma_16 (1 + |kappa
/// offset|)`), and a non-finite one count as folding (the rollover is then not
/// attempted).
fn offset_folds(
    k: &Kernel,
    target: &Brep,
    face: FaceId,
    at: Point3,
    along: Vec3,
    offset: f64,
) -> Result<bool> {
    let surface = crate::topology::face::p2::surface(target, face)?
        .ok_or_else(|| SolidError::blend_failed(OP, "the face has no surface"))?
        .clone_for_evaluation()?;
    let domain = surface.natural_uv_domain_with_nurb(k)?;
    let dropped = surface.drop_point::<NoObject>(k, at, &domain, &DropPointOptions::default())?;
    if !dropped.success {
        return Ok(true);
    }
    let uv = dropped.result_uv;
    let [_, du, dv, duv, duu, dvv] = surface.evaluate_2nd_derivatives(k, uv, true, true)?;
    let n = surface.evaluate_normal(k, uv, KnotSide::Left, KnotSide::Left)?;
    let Ok(t) = n.cross(along).unitize() else {
        return Ok(true);
    };
    // t = a du + b dv, by the first fundamental form
    let (g11, g12, g22) = (du.dot(du), du.dot(dv), dv.dot(dv));
    let det = g11 * g22 - g12 * g12;
    if det.is_nan() || det <= 0.0 {
        return Ok(true);
    }
    let (r1, r2) = (t.dot(du), t.dot(dv));
    let a = (g22 * r1 - g12 * r2) / det;
    let b = (g11 * r2 - g12 * r1) / det;
    let kappa = (duu * (a * a) + duv * (2.0 * a * b) + dvv * (b * b)).dot(n);
    // the factor proved positive by more than its rounding (a few operations on the
    // derivatives, relative to the larger of 1 and the product), so NaN folds too
    let product = kappa * offset;
    let margin = crate::nurbs::math::gamma(16) * (1.0 + product.abs());
    let factor = 1.0 - product;
    Ok(factor.is_nan() || margin.is_nan() || factor <= margin)
}

#[cfg(test)]
mod tests {
    use super::{seam_side_hint, section_seeds, Profile, SectionSeed};
    use crate::blend::fillet_solver::p1::FilletSolver;
    use crate::curve::curve::Curve;
    use crate::kernel::Kernel;
    use crate::math::{Point3, Vec3};
    use crate::primitive::api_curves::create_line_segment;
    use crate::sweep::api_brep::create_linear_sweep;
    use crate::topology::{edge, Brep, EdgeId, FaceId};

    /// The one edge of `b` from `p` to `q` (either way round).
    fn edge_from_to(b: &Brep, p: Point3, q: Point3) -> EdgeId {
        let at = |v: Option<crate::topology::VertexId>| b.vertex(v.unwrap()).unwrap().point;
        let found: Vec<EdgeId> = b
            .edge_list()
            .iter()
            .copied()
            .filter(|&e| {
                let (s, t) = (
                    at(edge::start_vertex(b, e).unwrap()),
                    at(edge::end_vertex(b, e).unwrap()),
                );
                (s.distance(p) < 1e-9 && t.distance(q) < 1e-9)
                    || (s.distance(q) < 1e-9 && t.distance(p) < 1e-9)
            })
            .collect();
        assert_eq!(found.len(), 1, "edges {p:?} to {q:?}: {found:?}");
        found[0]
    }

    /// A rolled rail's seed where the filleted edge's point lies on the rolled-over edge, old to
    /// new (#16354's zero seam hint). The prism 10 tall on the polygon (0, -20), (40, -20),
    /// (40, 0.1), (20, 0), (0, 0): its side faces S (y = 0) and X (on to (40, 0.1)) meet along
    /// the vertical edge T at (20, 0), smooth (a crease of atan(0.1 / 20) = 0.005 rad within
    /// the solver's tangency tolerance 0.01). The round is on S's top edge from (0, 0, 10) to
    /// T's top V = (20, 0, 10), its rail rolled from S onto X. At V itself and 0.5 tol short of
    /// it along the edge (tol the body's tolerance), the rolled-over edge's nearest point is V,
    /// within tol of the edge's point: the seed is V with no seam-side hint (before #16354 the
    /// hint was the zero or near-zero vector `V - point`), still solved on the extended surface
    /// (`rolled`). At (10, 0, 10) the hint is the direction to V, (10, 0, 0). The other rail is
    /// does not roll; its seed is the edge's point, with no hint.
    #[test]
    fn a_rolled_seed_on_the_rolled_over_edge_has_no_seam_side_hint() {
        let mut k = Kernel::default();
        let p = |x: f64, y: f64| Point3::new(x, y, 0.0);
        let poly = [
            p(0.0, -20.0),
            p(40.0, -20.0),
            p(40.0, 0.1),
            p(20.0, 0.0),
            p(0.0, 0.0),
        ];
        let sides: Vec<Curve> = (0..poly.len())
            .map(|i| Curve::from(create_line_segment(poly[i], poly[(i + 1) % poly.len()]).unwrap()))
            .collect();
        let refs: Vec<Option<&Curve>> = sides.iter().map(Some).collect();
        let mut b =
            create_linear_sweep(&mut k, &refs, Vec3::new(0.0, 0.0, 1.0), 10.0, true).unwrap();
        let v = Point3::new(20.0, 0.0, 10.0);
        let filleted = edge_from_to(&b, Point3::new(0.0, 0.0, 10.0), v);
        let t = edge_from_to(&b, Point3::new(20.0, 0.0, 0.0), v);
        let t_faces = edge::faces(&b, t, None).unwrap();
        let s = *edge::faces(&b, filleted, None)
            .unwrap()
            .iter()
            .find(|f| t_faces.contains(f))
            .unwrap();
        let x: FaceId = *t_faces.iter().find(|&&f| f != s).unwrap();
        let on_s = edge::edgeuses(&b, filleted, None, None)
            .unwrap()
            .into_iter()
            .find(|&eu| crate::topology::edgeuse::p1::face(&b, eu).unwrap() == Some(s))
            .unwrap();
        let mut solver = FilletSolver::new(1e-6, 0.1, 0.01);
        solver.edgeuses = [Some(on_s), None];
        solver.rolled_faces = [Some(x), None];
        let tol = b.tolerance();
        assert!(tol > 0.0);
        let hint = Point3::new(10.0, 0.0, 10.0);
        for (point, seam_side) in [
            (v, None),
            (v - Vec3::new(0.5 * tol, 0.0, 0.0), None),
            (hint, Some(v - hint)),
        ] {
            let seeds = section_seeds(&k, &mut b, &solver, point).unwrap();
            assert!(
                seeds[0].point.distance(v) <= 1e-12,
                "{point:?}: seed {:?}",
                seeds[0].point
            );
            match (seeds[0].seam_side, seam_side) {
                (None, None) => {}
                (Some(got), Some(want)) => {
                    assert!((got - want).length() <= 1e-12, "{point:?}: hint {got:?}");
                }
                (got, want) => panic!("{point:?}: hint {got:?}, want {want:?}"),
            }
            assert!(seeds[0].rolled, "{point:?}");
            assert_eq!(
                seeds[1],
                SectionSeed {
                    point,
                    seam_side: None,
                    rolled: false,
                },
                "{point:?}"
            );
        }
    }

    /// A rolled rail's seam-side hint is the direction to the rolled-over edge's point, and no
    /// hint at all where that point is the filleted edge's own within the tolerance (the zero
    /// vector the hint was before names no side of a seam), or the distance is not a number.
    #[test]
    fn a_seam_side_hint_is_never_a_zero_vector() {
        let tol = 1e-6;
        let point = Point3::new(1.0, 2.0, 3.0);
        assert_eq!(
            seam_side_hint(Point3::new(1.0, 2.5, 3.0), point, tol),
            Some(Vec3::new(0.0, 0.5, 0.0))
        );
        assert_eq!(seam_side_hint(point, point, tol), None);
        assert_eq!(
            seam_side_hint(Point3::new(1.0, 2.0, 3.0 + 0.5 * tol), point, tol),
            None
        );
        assert_eq!(
            seam_side_hint(Point3::new(1.0, 2.0, 3.0 + 2.0 * tol), point, tol),
            Some(Vec3::new(0.0, 0.0, 3.0 + 2.0 * tol - 3.0))
        );
        let nan = Point3::new(f64::NAN, 2.0, 3.0);
        assert_eq!(seam_side_hint(nan, point, tol), None);
    }

    /// The side check of the cylinders' closed form: a face whose arc leaves the edge's point
    /// the other way round its circle from the contact (the shorter arc to the contact is not
    /// the face's, which may run more than half way round) is not judged from that arc. On the
    /// circle of radius 10 about (0, -10) through the origin (its top), the contact at 60 deg
    /// about the axis lies clockwise of the origin (at 90 deg): the profile leads into a face
    /// whose binormal at the origin is +x (clockwise), not into one whose binormal is -x
    /// (counterclockwise, the long way round to the contact). Likewise on a line, and a
    /// contact at the origin or opposite it names no sense.
    #[test]
    fn the_cylinder_closed_form_judges_only_a_face_on_the_contact_s_side() {
        let circle = Profile::Circle {
            c: (0.0, -10.0),
            radius: 10.0,
            side: 1.0,
        };
        let at = |deg: f64| {
            let a = deg.to_radians();
            (10.0 * a.cos(), -10.0 + 10.0 * a.sin())
        };
        let tol = 1e-9;
        assert!(circle.leads_into(at(60.0), (1.0, 0.0), tol));
        assert!(!circle.leads_into(at(60.0), (-1.0, 0.0), tol));
        assert!(circle.leads_into(at(120.0), (-1.0, 0.0), tol));
        assert!(!circle.leads_into(at(120.0), (1.0, 0.0), tol));
        // the shorter arc to the contact, whichever way the face runs
        let travel = circle.travel(at(60.0), at(60.0), tol).unwrap();
        assert!(
            (travel - 10.0 * 30.0_f64.to_radians()).abs() < 1e-12,
            "{travel}"
        );
        // no sense: the contact at the origin or opposite it
        assert!(!circle.leads_into(at(90.0), (1.0, 0.0), tol));
        assert!(!circle.leads_into(at(270.0), (1.0, 0.0), tol));
        assert!(!circle.leads_into((f64::NAN, 0.0), (1.0, 0.0), tol));
        let line = Profile::Line {
            n: (0.0, 1.0),
            h: 0.0,
        };
        assert!(line.leads_into((3.0, 0.0), (1.0, 0.0), tol));
        assert!(!line.leads_into((3.0, 0.0), (-1.0, 0.0), tol));
        assert!(!line.leads_into((0.0, 0.0), (1.0, 0.0), tol));
    }
}
