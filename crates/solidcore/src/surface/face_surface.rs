//! A face's surface made from a record face's numbers: its label (what the face is), the NURBS
//! form it is parameterised on when it has one, and its parameter box. The one constructor of
//! a record face's surface, for the conversion of a body's faces; beside
//! [`from_parameters`](crate::surface::from_parameters), which it calls for the labels with
//! parameters of their own.
//!
//! Not upstream: SolidCraft's flat-array import of faces (a revolution's exact NURBS, a ball
//! patch on its net, an analytic face on its NURBS form, the recognition of revolutions and
//! extruded circles as analytic surfaces), moved into the kernel (docs/layers.md §1).
//!
//! **Exact classes at bound 0** (owner rule 13564): an analytic face is made only once its
//! NURBS is proved to lie on it (`lies_on`, through
//! [`AnalyticSupport::build`](crate::surface::implicit::AnalyticSupport::build) or
//! [`from_parameters`](crate::surface::from_parameters)); a ball patch only once its net is
//! certified on its sphere ([`BallPatch::create_certified`]). A revolution or a rational spline
//! is recognised as a plane, cylinder, cone, sphere or torus only when the kernel derives that
//! surface from its exact NURBS form and proves the form on it: the analytic surface then holds
//! the form as its own NURBS, so the face's parameters and pcurves stay as they are. A
//! recognition the proof refuses leaves the face its NURBS (#2540: never a class the check
//! refuses).
//!
//! Deviations: none (no upstream counterpart). No recursion.

use crate::error::{Result, SolidError};
use crate::eval::form;
use crate::eval::Surface as Record;
use crate::geom::extent2d::Extent2d;
use crate::geom::vector3d::Point3;
use crate::kernel::Kernel;
use crate::nurbs::b_spline_surface::BSplineSurface;
use crate::surface::ball_patch::BallPatch;
use crate::surface::from_parameters::from_parameters;
use crate::surface::implicit::{AnalyticSupport, SupportKind};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::face_surface::face_surface";

/// A face's surface in the kernel.
#[derive(Debug, Clone)]
pub struct FaceSurface {
    /// The surface.
    pub surface: Surface,
    /// The face's parameter box in the surface's parameters (the face's uv domain).
    pub uv_domain: Extent2d,
    /// Whether the record's pcurves are in these parameters, so that they are the face's UV
    /// trims: true where the face's parameters are a NURBS's the record holds (a spline, a NURBS
    /// form, a ball patch's net), false where the kernel's parameters are its own (an analytic
    /// label's, a revolution's exact form), whose trims the kernel builds from the edges.
    pub trims: bool,
}

/// The surface of a record face: `label`, parameterised on `nurbs` when given, over `domain`
/// (the face's parameter box: `nurbs`'s parameters when given, else `label`'s).
///
/// - no form: an analytic label or a spline is [`from_parameters`]'s (a rational spline that is
///   an extruded circle recognised as its cylinder); a placed revolution is its exact NURBS form
///   ([`form::surface`]), recognised as an analytic surface when one is proved on it;
/// - a ball patch's form is its net, certified on its sphere;
/// - a revolution's form is that NURBS, recognised as above;
/// - an analytic label's form is that NURBS, which must be proved on the analytic surface of the
///   label's kind the kernel derives from it.
///
/// # Errors
///
/// `InvalidInput` for a form that is not a spline, a ball patch without its form, a ball
/// patch's net off its sphere, an analytic label not proved on its form, or a revolution with no
/// exact form; those of [`from_parameters`] and the constructors.
pub fn face_surface(
    k: &Kernel,
    label: &Record,
    nurbs: Option<&Record>,
    domain: [[f64; 2]; 2],
) -> Result<FaceSurface> {
    let bad = |reason: &'static str| SolidError::invalid_input(OP, reason);
    let Some(n) = nurbs else {
        return match label {
            Record::BallPatch { .. } => Err(bad("a ball patch without its NURBS form")),
            Record::PlacedRevolution { .. } => {
                let (f, fdom) = form::surface(label, domain)
                    .ok_or_else(|| bad("a revolution with no exact NURBS form"))?;
                let (net, uv_domain) = spline(k, &f, fdom)?;
                Ok(FaceSurface {
                    surface: recognised(k, net, &REVOLVED)?,
                    uv_domain,
                    trims: false,
                })
            }
            Record::RationalSpline { .. } => {
                let (net, uv_domain) = spline(k, label, domain)?;
                let surface = if extruded(&net) {
                    recognised(k, net, &[SupportKind::Cylinder])?
                } else {
                    Surface::from(net)
                };
                Ok(FaceSurface {
                    surface,
                    uv_domain,
                    trims: true,
                })
            }
            Record::Spline { .. } => {
                let (surface, uv_domain) = from_parameters(k, label, domain)?;
                Ok(FaceSurface {
                    surface,
                    uv_domain,
                    trims: true,
                })
            }
            _ => {
                let (surface, uv_domain) = from_parameters(k, label, domain)?;
                Ok(FaceSurface {
                    surface,
                    uv_domain,
                    trims: false,
                })
            }
        };
    };
    let (net, uv_domain) = spline(k, n, domain)?;
    let surface = match label {
        Record::BallPatch { center, radius } => {
            Surface::from(BallPatch::create_certified(&net, Point3::new(center[0], center[1], center[2]), *radius)?)
        }
        Record::PlacedRevolution { .. } => recognised(k, net, &REVOLVED)?,
        Record::Cylinder { .. } => on_form(k, SupportKind::Cylinder, net)?,
        Record::Cone { .. } => on_form(k, SupportKind::Cone, net)?,
        Record::Sphere { .. } => on_form(k, SupportKind::Sphere, net)?,
        Record::Torus { .. } => on_form(k, SupportKind::Torus, net)?,
        _ => return Err(bad("a NURBS form is given for a cylinder, cone, sphere, torus, ball patch or revolution only")),
    };
    Ok(FaceSurface {
        surface,
        uv_domain,
        trims: true,
    })
}

/// The kinds a revolution may be, in the order they are tried.
const REVOLVED: [SupportKind; 5] = [
    SupportKind::Plane,
    SupportKind::Cylinder,
    SupportKind::Cone,
    SupportKind::Sphere,
    SupportKind::Torus,
];

/// A spline record as the kernel's B-spline surface ([`from_parameters`]) and its box.
fn spline(k: &Kernel, s: &Record, domain: [[f64; 2]; 2]) -> Result<(BSplineSurface, Extent2d)> {
    if !matches!(s, Record::Spline { .. } | Record::RationalSpline { .. }) {
        return Err(SolidError::invalid_input(
            OP,
            "a NURBS form is not a spline",
        ));
    }
    let (surface, uv_domain) = from_parameters(k, s, domain)?;
    match surface.kind() {
        SurfaceKind::BSpline(b) => Ok((b.copy(), uv_domain)),
        _ => Err(SolidError::invalid_input(
            OP,
            "a spline is not the kernel's B-spline",
        )),
    }
}

/// Whether a rational spline is an extruded curve: one direction of degree 1 on two rows (the
/// structure of an extruded circle; the kernel's derivation decides whether it is one).
fn extruded(s: &BSplineSurface) -> bool {
    let Some(n) = s.gw_nurb() else { return false };
    let (p, q) = n.degrees();
    let (nu, nv) = (n.rows().count(), n.rows().next().map_or(0, <[_]>::len));
    (q == 1 && nv == 2 && p >= 2) || (p == 1 && nu == 2 && q >= 2)
}

/// The analytic surface of one of `kinds` proved on the NURBS `net` and holding it, the first
/// that is; otherwise the NURBS itself.
fn recognised(k: &Kernel, net: BSplineSurface, kinds: &[SupportKind]) -> Result<Surface> {
    for &kind in kinds {
        let Ok(support) = AnalyticSupport::derive(k, kind, &net) else {
            continue;
        };
        match support.build(k, net.copy()) {
            Ok(s) => return Ok(s),
            Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => return Err(e),
            Err(_) => {}
        }
    }
    Ok(Surface::from(net))
}

/// The analytic surface of `kind` the NURBS form `net` is, proved and holding it.
fn on_form(k: &Kernel, kind: SupportKind, net: BSplineSurface) -> Result<Surface> {
    AnalyticSupport::derive(k, kind, &net)?.build(k, net)
}

#[cfg(test)]
mod tests;
