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
    /// form, a ball patch's net) or a plane's (scaled by [`FaceSurface::scale`]), false where
    /// the kernel's parameters are its own (an analytic label's, a revolution's exact form),
    /// whose trims the kernel builds from the edges.
    pub trims: bool,
    /// The kernel's (u, v) of the record's, `[su u, sv v]`: a plane's parameters run along its
    /// axes made unit, so they are the record's times the axes' lengths; `[1, 1]` elsewhere.
    pub scale: [f64; 2],
}

/// The surface of a record face: `label`, parameterised on `nurbs` when given, over `domain`
/// (the face's parameter box: `nurbs`'s parameters when given, else `label`'s).
///
/// - no form: an analytic label or a spline is [`from_parameters`]'s (a rational spline that is
///   an extruded circle recognised as its cylinder); a placed revolution is its exact NURBS form
///   ([`form::surface`]), recognised as an analytic surface when one is proved on it;
/// - a ball patch's form is its net, certified on its sphere;
/// - a revolution's form is that NURBS, recognised as above;
/// - an analytic label's form is that NURBS, proved on the analytic surface of the label's own
///   numbers.
///
/// A label its form does not prove (a ball patch's net off its sphere, an analytic label's
/// numbers off its form) is dropped, and the face is its exact NURBS (owner 22254: a valid
/// body is never refused, and the form's numbers are never put under the label).
///
/// # Errors
///
/// `InvalidInput` for a form that is not a spline, a ball patch without its form, or a
/// revolution with no exact form; those of [`from_parameters`] and the constructors.
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
                    scale: [1.0, 1.0],
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
                    scale: [1.0, 1.0],
                })
            }
            Record::Spline { .. } => {
                let (surface, uv_domain) = from_parameters(k, label, domain)?;
                Ok(FaceSurface {
                    surface,
                    uv_domain,
                    trims: true,
                    scale: [1.0, 1.0],
                })
            }
            Record::Plane { u_axis, v_axis, .. } => plane(k, label, domain, *u_axis, *v_axis, None),
            Record::PlaneThrough {
                origin,
                u_point,
                v_point,
            } => plane(k, label, domain, *u_point, *v_point, Some(*origin)),
            _ => {
                let (surface, uv_domain) = from_parameters(k, label, domain)?;
                Ok(FaceSurface {
                    surface,
                    uv_domain,
                    trims: false,
                    scale: [1.0, 1.0],
                })
            }
        };
    };
    let (net, uv_domain) = spline(k, n, domain)?;
    let surface = match label {
        Record::BallPatch { center, radius } => {
            let centre = Point3::new(center[0], center[1], center[2]);
            match BallPatch::create_certified(&net, centre, *radius) {
                Ok(b) => Surface::from(b),
                Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                    return Err(e)
                }
                // owner 22254: a label its form does not prove is dropped; the face is its
                // exact NURBS
                Err(_) => Surface::from(net),
            }
        }
        Record::PlacedRevolution { .. } => recognised(k, net, &REVOLVED)?,
        Record::Cylinder { .. }
        | Record::Cone { .. }
        | Record::Sphere { .. }
        | Record::Torus { .. } => labelled(k, label, net)?,
        _ => return Err(bad("a NURBS form is given for a cylinder, cone, sphere, torus, ball patch or revolution only")),
    };
    Ok(FaceSurface {
        surface,
        uv_domain,
        trims: true,
        scale: [1.0, 1.0],
    })
}

/// A plane label's surface ([`from_parameters`]), its record pcurves its trims scaled by its
/// axes' scales ([`axis_scale`](crate::surface::from_parameters::axis_scale)): the axes `du`, `dv`, or `u_point - origin`, `v_point - origin` for a plane
/// through three points.
fn plane(
    k: &Kernel,
    label: &Record,
    domain: [[f64; 2]; 2],
    du: [f64; 3],
    dv: [f64; 3],
    through: Option<[f64; 3]>,
) -> Result<FaceSurface> {
    let (surface, uv_domain) = from_parameters(k, label, domain)?;
    let o = through.unwrap_or([0.0; 3]);
    let length = |a: [f64; 3]| {
        crate::surface::from_parameters::axis_scale(
            Point3::new(a[0], a[1], a[2]) - Point3::new(o[0], o[1], o[2]),
        )
    };
    Ok(FaceSurface {
        surface,
        uv_domain,
        trims: true,
        scale: [length(du), length(dv)],
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

/// The analytic surface of a cylinder, cone, sphere or torus label on its NURBS form `net`:
/// the label's own numbers (its frame, radii and half angle, the angle in the kernel's
/// degrees), proved on the form ([`AnalyticSupport::build`]: the form lies on the label's
/// implicit surface and the analytic surface on the form's parametrisation evaluates as it),
/// holding the form. A label the form does not prove is dropped (owner 22254): the face is its
/// exact NURBS, never the analytic surface of numbers recomputed from the form under the
/// label, and never a refused body.
///
/// # Errors
///
/// Only [`SolidError::Unported`] and cancellation from the proof.
fn labelled(k: &Kernel, label: &Record, net: BSplineSurface) -> Result<Surface> {
    let Some(support) = label_support(label) else {
        return Ok(Surface::from(net));
    };
    match support.build(k, net.copy()) {
        Ok(s) => Ok(s),
        Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => Err(e),
        Err(_) => Ok(Surface::from(net)),
    }
}

/// A cylinder, cone, sphere or torus label's numbers as the kernel's support: the frame copied
/// as stored (its axes are checked by [`AnalyticSupport::build`]), a cone's half angle in
/// degrees. A torus's tube start moves only its parametrisation, which is its form's here.
fn label_support(label: &Record) -> Option<AnalyticSupport> {
    let placement = |f: &crate::eval::Frame| {
        let v = |a: [f64; 3]| crate::geom::vector3d::Vec3::new(a[0], a[1], a[2]);
        crate::geom::axis2_placement::Axis2Placement::from_stored(
            Point3::new(f.origin[0], f.origin[1], f.origin[2]),
            v(f.x_axis),
            v(f.y_axis),
        )
    };
    Some(match label {
        Record::Cylinder { frame, radius } => AnalyticSupport::Cylinder {
            placement: placement(frame),
            radius: *radius,
        },
        Record::Cone {
            frame,
            radius_at_origin,
            half_angle,
        } => AnalyticSupport::Cone {
            placement: placement(frame),
            base_radius: *radius_at_origin,
            semi_angle_deg: half_angle.to_degrees(),
        },
        Record::Sphere { frame, radius } => AnalyticSupport::Sphere {
            placement: placement(frame),
            radius: *radius,
        },
        Record::Torus {
            frame,
            major_radius,
            minor_radius,
            ..
        } => AnalyticSupport::Torus {
            placement: placement(frame),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
        },
        _ => return None,
    })
}

const OP_BACK: &str = "surface::face_surface::record_face";

/// A kernel face's surface as the record holds it.
#[derive(Debug, Clone, PartialEq)]
pub struct RecordFace {
    /// What the face is: an analytic or ball-patch label, or the spline itself.
    pub surface: Record,
    /// The NURBS form the face's parameters are on, when it differs from `surface`'s own.
    pub nurbs: Option<Record>,
    /// The face's parameter box in the record's parameters.
    pub domain: [[f64; 2]; 2],
    /// The record's (u, v) of the kernel's: `[su u, sv v]` (a plane's distances along its unit
    /// axes; `[1, 1]` elsewhere, where the record's parameters are the kernel's NURBS's).
    pub scale: [f64; 2],
    /// The record's natural normal (∂S/∂u × ∂S/∂v of the record's parameters) is the
    /// kernel's, the side the face's loops are kept relative to; false where it is the
    /// other side (a plane whose scales have opposite signs), so the loops turn over.
    pub agrees: bool,
}

/// The record of the kernel's face surface `s` over `uv_domain` (the face's box in the
/// kernel's parameters), the inverse of [`face_surface`]:
///
/// - **Plane:** its placement's origin and unit axes, copied (no cross product), its box
///   and pcurves its kernel parameters times its scales: the kernel's plane is
///   `origin + su u x + sv v y` in its NURBS parameters, so the record's parameters are an
///   exact image of the kernel's, and their normals agree when `su sv > 0`.
/// - **Cylinder, cone, sphere, torus:** the label of its own kind and its own numbers
///   ([`AnalyticSupport::of`], as [`face_surface`] reads the label back, so a round trip
///   keeps them), frame copied (no cross product), angles in radians, with its NURBS as its
///   form, over the kernel's box. Exact class at bound 0: proved on the form
///   ([`AnalyticSupport::build`]: `lies_on` and the form's own parametrisation); numbers the
///   form does not prove give no label, and the face goes as its NURBS. The record has
///   no cone of half angle under 10⁻¹² nor a torus whose tube reaches its axis: those go as
///   their NURBS, which is the face exactly.
/// - **Ball patch:** its sphere's label on its net, certified on it
///   ([`BallPatch::create_certified`]); a net the certificate refuses goes as its NURBS.
/// - **B-spline, revolution, extrusion, procedural blend:** its NURBS (a procedural blend's
///   net, until the record has its kind).
///
/// The box ends on the NURBS's natural domain where it overshoots it by the rounding of the
/// kernel's parameters (10⁻¹² of its size: a sphere's latitudes, a cone's apex).
///
/// # Errors
///
/// `InvalidInput` for a plane with a zero or non-finite scale; `Unsupported` for a surface with no NURBS form (an
/// offset, a curve-bounded or STEP surface), which the record cannot carry exactly.
pub fn record_face(k: &Kernel, s: &Surface, uv_domain: &Extent2d) -> Result<RecordFace> {
    use crate::blend::fillet_section::station::as_b_spline;
    use crate::surface::implicit::AnalyticSupport as A;
    let bad = |reason: &'static str| SolidError::invalid_input(OP_BACK, reason);
    let box_ = [
        [uv_domain.u_min(), uv_domain.u_max()],
        [uv_domain.v_min(), uv_domain.v_max()],
    ];
    if let SurfaceKind::Plane(p) = s.kind_with_nurb(k)? {
        A::check(k, s)?;
        let (o, x, y) = p.position().canonical();
        let sc = p.uv_scale();
        if !(sc.x.is_finite() && sc.y.is_finite() && sc.x != 0.0 && sc.y != 0.0) {
            return Err(bad("a plane's scale is zero or not finite"));
        }
        let scaled = |[a, b]: [f64; 2], by: f64| {
            let (a, b) = (a * by, b * by);
            if a <= b {
                [a, b]
            } else {
                [b, a]
            }
        };
        return Ok(RecordFace {
            surface: Record::Plane {
                origin: [o.x, o.y, o.z],
                u_axis: [x.x, x.y, x.z],
                v_axis: [y.x, y.y, y.z],
            },
            nurbs: None,
            domain: [scaled(box_[0], sc.x), scaled(box_[1], sc.y)],
            scale: [sc.x, sc.y],
            agrees: (sc.x > 0.0) == (sc.y > 0.0),
        });
    }
    let label = match s.kind_with_nurb(k)? {
        SurfaceKind::Cone(_) | SurfaceKind::Sphere(_) | SurfaceKind::Torus(_) => {
            let frame = |p: &crate::geom::axis2_placement::Axis2Placement| {
                let (o, x, y) = p.canonical();
                crate::eval::Frame {
                    origin: [o.x, o.y, o.z],
                    x_axis: [x.x, x.y, x.z],
                    y_axis: [y.x, y.y, y.z],
                }
            };
            // The surface's own numbers, proved on its form (`lies_on` and the form's own
            // parametrisation), as [`face_surface`] reads the label back; numbers the form
            // does not prove give no label (owner 22254): the face goes as its exact NURBS.
            let own = A::of(s).ok_or_else(|| bad("an analytic surface of no analytic kind"))?;
            let form = as_b_spline(k, s)?.ok_or_else(|| bad("the surface has no NURBS form"))?;
            let proved = match own.build(k, form.into_owned()) {
                Ok(_) => Some(own),
                Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                    return Err(e)
                }
                Err(_) => None,
            };
            match proved {
                None => None,
                Some(proved) => match proved {
                    A::Cylinder { placement, radius } => Some(Record::Cylinder {
                        frame: frame(&placement),
                        radius,
                    }),
                    A::Cone {
                        placement,
                        base_radius,
                        semi_angle_deg,
                    } => {
                        let half_angle = semi_angle_deg.to_radians();
                        (half_angle.abs() >= 1e-12).then(|| Record::Cone {
                            frame: frame(&placement),
                            radius_at_origin: base_radius,
                            half_angle,
                        })
                    }
                    A::Sphere { placement, radius } => Some(Record::Sphere {
                        frame: frame(&placement),
                        radius,
                    }),
                    A::Torus {
                        placement,
                        major_radius,
                        minor_radius,
                    } => (major_radius > minor_radius).then(|| Record::Torus {
                        frame: frame(&placement),
                        major_radius,
                        minor_radius,
                        tube_start: 0.0,
                    }),
                    A::Plane { .. } => return Err(bad("a plane read as another kind")),
                },
            }
        }
        SurfaceKind::BallPatch(bp) => {
            let c = bp.centre();
            match BallPatch::create_certified(bp.net(), c, bp.radius()) {
                Ok(_) => Some(Record::BallPatch {
                    center: [c.x, c.y, c.z],
                    radius: bp.radius(),
                }),
                Err(e @ (SolidError::Unported { .. } | SolidError::Cancelled { .. })) => {
                    return Err(e)
                }
                Err(_) => None,
            }
        }
        SurfaceKind::BSpline(_)
        | SurfaceKind::SurfOfRevolution(_)
        | SurfaceKind::SurfOfExtrusion(_)
        | SurfaceKind::ProceduralBlend(_) => None,
        _ => {
            return Err(SolidError::Unsupported {
                operation: OP_BACK,
                reason: "a face surface with no NURBS form has no exact record",
            })
        }
    };
    let form = as_b_spline(k, s)?.ok_or_else(|| bad("the surface has no NURBS form"))?;
    let nurbs = spline_record(&form)?;
    let natural = form.natural_uv_domain();
    let onto = |[a, b]: [f64; 2], lo: f64, hi: f64| {
        let slack = 1e-12 * (1.0 + a.abs().max(b.abs()));
        let snap = |t: f64| {
            if t < lo && t >= lo - slack {
                lo
            } else if t > hi && t <= hi + slack {
                hi
            } else {
                t
            }
        };
        [snap(a), snap(b)]
    };
    let domain = [
        onto(box_[0], natural.u_min(), natural.u_max()),
        onto(box_[1], natural.v_min(), natural.v_max()),
    ];
    let (surface, nurbs) = match label {
        Some(l) => (l, Some(nurbs)),
        None => (nurbs, None),
    };
    Ok(RecordFace {
        surface,
        nurbs,
        domain,
        scale: [1.0, 1.0],
        agrees: true,
    })
}

/// A B-spline surface's NURBS as the record's spline (every weight 1) or rational spline
/// (homogeneous points copied as NLib holds them), rows along u.
fn spline_record(s: &BSplineSurface) -> Result<Record> {
    use crate::nurbs::math::NOW;
    let nurb = s
        .gw_nurb()
        .ok_or_else(|| SolidError::invalid_input(OP_BACK, "a B-spline without its NURB"))?;
    let (u_knots, v_knots) = (nurb.u_knots().to_vec(), nurb.v_knots().to_vec());
    let polynomial = nurb.rows().flatten().all(|p| p.w == NOW || p.w == 1.0);
    Ok(if polynomial {
        Record::Spline {
            u_knots,
            v_knots,
            points: nurb
                .rows()
                .map(|r| r.iter().map(|p| [p.x, p.y, p.z]).collect())
                .collect(),
        }
    } else {
        Record::RationalSpline {
            u_knots,
            v_knots,
            homogeneous: nurb
                .rows()
                .map(|r| {
                    r.iter()
                        .map(|p| [p.x, p.y, p.z, if p.w == NOW { 1.0 } else { p.w }])
                        .collect()
                })
                .collect(),
        }
    })
}

#[cfg(test)]
mod tests;
