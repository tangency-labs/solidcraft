//! (not upstream) How far a face's outward normals turn from a direction: a proved upper
//! bound of the angle between the direction and the face's outward normal at every point
//! of it, from the surface's definition, rounding included, never samples.
//!
//! Upstream: none.
//!
//! | Rust                   | Kinds                                                        |
//! |------------------------|--------------------------------------------------------------|
//! | [`normal_angle_bound`] | plane, cylinder, cone, sphere, ring torus, B-spline, extrusion |
//!
//! - **Plane:** the angle between its outward normal and the direction.
//! - **Cylinder, cone, sphere, ring torus:** the cone holding the surface's normal lines
//!   over the ball that holds the face's proved box
//!   ([`Surface::normal_cone_in_ball`](crate::surface::surface::Surface), closed form): the
//!   angle between the cone's axis, taken in the outward sense, and the direction, plus its
//!   half angle. The sense is the outward normal at the centre of the face's UV domain,
//!   which must lie in that ball (within it the surface's normals are within the half angle
//!   of one sense of the axis).
//! - **B-spline nets (a ball patch's too), extrusions of B-spline generators:** the face's
//!   UV domain in pieces, each with a cone of its exact normals
//!   (`surface::surface::fold_certificate::normal_angle_bound`).
//!
//! The B-spline bound is taken over the face's UV domain (its parameter box), not its
//! trimmed region: a face that covers little of its box gets a looser bound, never a
//! smaller one. Nothing is proved (`None`) for another surface kind, a cone a quarter turn
//! or wider (a ball that reaches the axis, centre or apex height: a whole tube, a large
//! cap), a domain centre outside the ball, a faceuse of another face or of unknown
//! orientation, or a direction of no length.
//!
//! Uses: a face turns toward a line's direction everywhere (it is single-valued over the
//! planes square to the line) when its bound is below a quarter turn; draft and undercut
//! checks against a pull direction; a notice naming how far a face tilts.
//!
//! Deviations: not a port. No recursion.

use crate::geom::certified::above;
use crate::nurbs::b_spline_surface_n_lib::p2::OrientType;
use crate::nurbs::math::gamma;
use crate::query::api_queries::face_normal_at_uv;
use crate::query::brep as qbrep;
use crate::surface::surface::p2::SurfaceKind;
use crate::topology::face::p2 as fp2;
use crate::topology::FaceuseId;
use crate::{Brep, FaceId, Kernel, Point2, Result, SolidError, Vec3};

/// A proved upper bound, in radians (`0` to pi), of the angle between `direction` and the
/// outward normal of face `f` (the sense of its use `fu`) at every point of it (module
/// docs); `None` where nothing is proved.
///
/// # Errors
///
/// Cancellation; those of the face's surface, its UV domain, the evaluations at its centre
/// and the proved box; those of the B-spline pieces' constructions.
pub fn normal_angle_bound(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    fu: FaceuseId,
    direction: Vec3,
) -> Result<Option<f64>> {
    let len = direction.length();
    if !(len.is_finite() && above(len, 0.0)) {
        return Ok(None);
    }
    let d = direction / len;
    let use_ = b.faceuse(fu)?;
    if use_.face != Some(f) {
        return Ok(None);
    }
    let reversed = match use_.orientation {
        OrientType::Same => false,
        OrientType::Opposite => true,
        OrientType::Unknown => return Ok(None),
    };
    let Some(surface) = fp2::surface(b, f)? else {
        return Ok(None);
    };
    let domain = fp2::uv_domain(b, f)?;
    let uv = domain.evaluate(0.5, 0.5);
    match surface.kind() {
        SurfaceKind::Plane(_) => {
            let Some(n) = outward_at(k, b, f, uv, fu)? else {
                return Ok(None);
            };
            // the evaluated normal is within `n.bound` of the exact unit normal
            let off = (n.bound * (1.0 + gamma(4))).min(1.0).asin();
            Ok(Some(
                (angle_up(n.value, d) + off).min(core::f64::consts::PI),
            ))
        }
        SurfaceKind::Sphere(_)
        | SurfaceKind::Cone(_)
        | SurfaceKind::Torus(_)
        | SurfaceKind::Step(_) => {
            let Some(bx) = crate::query::face::p1::proved_bounding_box(k, b, f, &mut None)? else {
                return Ok(None);
            };
            let centre = bx.mid();
            let radius = bx.max_distance_to_point(centre) * (1.0 + gamma(4));
            let Some(cone) = surface.normal_cone_in_ball(centre, radius) else {
                return Ok(None);
            };
            // the sense: the outward normal at a point of the surface inside the ball
            let p = qbrep::evaluate_surface_point(k, Some(surface), uv)?;
            if !above(radius, (p - centre).length() * (1.0 + gamma(4))) {
                return Ok(None);
            }
            let Some(n) = outward_at(k, b, f, uv, fu)? else {
                return Ok(None);
            };
            let n = n.value;
            let axis = if cone.axis.dot(n) < 0.0 {
                cone.axis * -1.0
            } else {
                cone.axis
            };
            Ok(Some(
                (angle_up(axis, d) + cone.half_angle).min(core::f64::consts::PI),
            ))
        }
        _ => crate::surface::surface::fold_certificate::normal_angle_bound(
            k, surface, &domain, reversed, d,
        ),
    }
}

/// The outward normal of `f` at `uv` ([`face_normal_at_uv`]); `None` where it is not
/// defined there (a zero normal, a pole).
fn outward_at(
    k: &Kernel,
    b: &Brep,
    f: FaceId,
    uv: Point2,
    fu: FaceuseId,
) -> Result<Option<crate::query::normal::UnitVector>> {
    match face_normal_at_uv(k, b, f, uv, fu) {
        Ok(n) => Ok(Some(n)),
        Err(SolidError::InvalidInput { .. }) => Ok(None),
        Err(e) => Err(e),
    }
}

/// The angle between the unit vectors `a` and `b`, by `atan2` of the cross and the dot
/// products, rounded up.
fn angle_up(a: Vec3, b: Vec3) -> f64 {
    a.cross(b).length().atan2(a.dot(b)) * (1.0 + gamma(8)) + gamma(8)
}
