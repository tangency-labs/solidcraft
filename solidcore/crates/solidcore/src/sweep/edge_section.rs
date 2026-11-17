//! Exact corner-section tools along straight or circular B-rep edges.
//!
//! Original construction: a closed section in the two support faces' inward frame,
//! translated along a line or revolved about a circle's axis. No fitted path or mesh.
//! The caller checks the swept tool against the stock before subtracting it.

use crate::curve::curve::CurveKind;
use crate::eval::Frame;
use crate::math::Point2;
use crate::nurbs::b_spline_curve_n_lib::TrimMode;
use crate::prism::profile::{curve_on_frame, Seg2};
use crate::query::api_queries::edgeuse_face_frame;
use crate::surface::surface::{Surface, SurfaceKind};
use crate::sweep::api_brep::{create_linear_sweep, create_rotational_sweep};
use crate::topology::edgeuse::p3::{evaluate_binormal, BinormalOptions};
use crate::topology::{edge, face, EdgeId, FaceuseId};
use crate::{Brep, Kernel, Result, SolidError};

const OP: &str = "sweep::edge_section";

/// The section removed from a square outside corner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CornerProfile {
    /// Equal setbacks.
    Chamfer,
    /// Width and angle.
    Bevel,
    /// Convex quarter round with an optional top step.
    Roundover,
    /// Concave quarter circle.
    Cove,
    /// Rectangular step.
    Rabbet,
    /// Two tangent quarter conics, concave above convex.
    Ogee,
    /// Quarter round with a step on both sides.
    Bead,
}

/// Dimensions in mm and radians. Only a profile's own dimensions are used.
#[derive(Clone, Copy, Debug)]
pub struct CornerSizes {
    /// Circular radius.
    pub radius: f64,
    /// Setback across the first support face.
    pub width: f64,
    /// Setback down the second support face.
    pub depth: f64,
    /// Bevel angle, measured from the first support face.
    pub angle: f64,
    /// Bead step on each side.
    pub step: f64,
    /// Roundover's flat top step.
    pub fillet: f64,
}

/// The exact closed removal section, counter-clockwise in inward coordinates.
///
/// # Errors
/// Invalid sizes, including a bevel outside (0, pi/2).
pub fn corner_section(profile: CornerProfile, s: CornerSizes) -> Result<Vec<Seg2>> {
    use std::f64::consts::{FRAC_1_SQRT_2, FRAC_PI_2};
    if ![s.radius, s.width, s.depth, s.angle, s.step, s.fillet]
        .iter()
        .all(|v| v.is_finite())
    {
        return Err(SolidError::invalid_input(
            OP,
            "profile sizes must be finite",
        ));
    }
    if !(s.radius > 0.0 && s.width > 0.0 && s.depth > 0.0 && s.step >= 0.0 && s.fillet >= 0.0)
        || [s.radius, s.width, s.depth, s.step, s.fillet]
            .iter()
            .any(|v| *v > 1e6)
        || !(s.angle > 0.0 && s.angle < FRAC_PI_2)
    {
        return Err(SolidError::invalid_input(OP, "positive profile sizes up to 1000000 mm and an angle between 0 and 90 degrees are required"));
    }
    let p = |x, y| Point2::new(x, y);
    let mut out = Vec::new();
    let mut at = p(0.0, 0.0);
    let mut line = |to| {
        if at != to {
            out.push(Seg2::Line { a: at, b: to });
        }
        at = to;
    };
    match profile {
        CornerProfile::Chamfer | CornerProfile::Bevel => {
            let d = if profile == CornerProfile::Bevel {
                s.width * s.angle.tan()
            } else {
                s.width
            };
            if !(d.is_finite() && d > 0.0 && d <= 1e6) {
                return Err(SolidError::invalid_input(OP, "bevel depth is out of range"));
            }
            line(p(s.width, 0.0));
            line(p(0.0, d));
            line(p(0.0, 0.0));
        }
        CornerProfile::Rabbet => {
            line(p(s.width, 0.0));
            line(p(s.width, s.depth));
            line(p(0.0, s.depth));
            line(p(0.0, 0.0));
        }
        CornerProfile::Roundover | CornerProfile::Bead => {
            let (a, b) = if profile == CornerProfile::Bead {
                (s.step, s.step)
            } else {
                (s.fillet, 0.0)
            };
            let r = s.radius;
            line(p(r + a + b, 0.0));
            line(p(r + a + b, a));
            line(p(r + b, a));
            out.push(Seg2::Arc {
                center: p(r + b, r + a),
                radius: r,
                start: -FRAC_PI_2,
                sweep: -FRAC_PI_2,
            });
            let end = p(b, r + a);
            if b > 0.0 {
                out.push(Seg2::Line {
                    a: end,
                    b: p(b, r + a + b),
                });
            }
            out.push(Seg2::Line {
                a: p(b, r + a + b),
                b: p(0.0, r + a + b),
            });
            out.push(Seg2::Line {
                a: p(0.0, r + a + b),
                b: p(0.0, 0.0),
            });
        }
        CornerProfile::Cove => {
            line(p(s.radius, 0.0));
            out.push(Seg2::Arc {
                center: p(0.0, 0.0),
                radius: s.radius,
                start: 0.0,
                sweep: FRAC_PI_2,
            });
            out.push(Seg2::Line {
                a: p(0.0, s.radius),
                b: p(0.0, 0.0),
            });
        }
        CornerProfile::Ogee => {
            let (w, d) = (s.width, s.depth);
            line(p(w, 0.0));
            out.push(Seg2::Conic {
                a: p(w, 0.0),
                apex: p(w, d / 2.0),
                b: p(w / 2.0, d / 2.0),
                w: FRAC_1_SQRT_2,
            });
            out.push(Seg2::Conic {
                a: p(w / 2.0, d / 2.0),
                apex: p(0.0, d / 2.0),
                b: p(0.0, d),
                w: FRAC_1_SQRT_2,
            });
            out.push(Seg2::Line {
                a: p(0.0, d),
                b: p(0.0, 0.0),
            });
        }
    }
    // Drop zero-length step segments when the step is zero.
    out.retain(|e| !matches!(e, Seg2::Line { a, b } if a == b));
    Ok(out)
}

/// Sweep a section along an edge between perpendicular support faces.
/// `uses` are the two outward face uses, with the first face as the section's top.
///
/// # Errors
/// A non-square corner, a non-line/non-circle path, unsupported supports, or sweep failure.
pub fn edge_section_tool(
    k: &mut Kernel,
    b: &mut Brep,
    e: EdgeId,
    uses: [FaceuseId; 2],
    section: &[Seg2],
) -> Result<Brep> {
    let interval = b.edge(e)?.interval;
    let mut curve = edge::curve(b, e)?
        .ok_or_else(|| SolidError::invalid_input(OP, "edge has no curve"))?
        .clone();
    let mut trim = interval;
    curve.trim(k, &mut trim, TrimMode::Silent)?;
    let origin = curve.evaluate_point(k, interval.min())?;
    let mut normals = Vec::new();
    let mut inward = Vec::new();
    for fu in uses {
        let f = b
            .faceuse(fu)?
            .face
            .ok_or_else(|| SolidError::invalid_input(OP, "face use has no face"))?;
        let eu = edge::edgeuse_of_face(b, e, Some(f))?
            .ok_or_else(|| SolidError::invalid_input(OP, "edge does not bound face"))?;
        normals.push(
            edgeuse_face_frame(k, b, eu, interval.min(), fu)?
                .normal
                .value,
        );
        inward.push(
            evaluate_binormal(k, b, eu, interval.min(), false, &BinormalOptions::default())?
                .binormal,
        );
    }
    let (Some(&top), Some(&side)) = (normals.first(), normals.get(1)) else {
        return Err(SolidError::invalid_input(OP, "two support faces required"));
    };
    if top.dot(side).abs() > 1e-9 {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "all router profiles require two faces meeting at 90 degrees",
        });
    }
    if inward.first().is_none_or(|v| v.dot(side) >= 0.0) {
        return Err(SolidError::Unsupported {
            operation: OP,
            reason: "router profiles require a convex outside edge",
        });
    }
    let x = -side;
    let y = -top;
    let frame = Frame {
        origin: [origin.x, origin.y, origin.z],
        x_axis: [x.x, x.y, x.z],
        y_axis: [y.x, y.y, y.z],
    };
    let curves = section
        .iter()
        .map(|s| curve_on_frame(k, &frame, 0.0, s))
        .collect::<Result<Vec<_>>>()?;
    let refs = curves.iter().map(Some).collect::<Vec<_>>();
    match curve.kind() {
        CurveKind::Line(_) => {
            for fu in uses {
                let f = b
                    .faceuse(fu)?
                    .face
                    .ok_or_else(|| SolidError::invalid_input(OP, "missing support"))?;
                if !matches!(
                    face::surface(b, f)?.map(Surface::kind),
                    Some(SurfaceKind::Plane(_))
                ) {
                    return Err(SolidError::Unsupported {
                        operation: OP,
                        reason: "straight router edges require planar supports",
                    });
                }
            }
            let end = curve.evaluate_point(k, interval.max())?;
            let dir = end - origin;
            create_linear_sweep(k, &refs, dir.unitize()?, dir.length(), true)
        }
        CurveKind::Ellipse(c) if c.is_circle() => {
            let data = c.canonical();
            let axis = data.position.z_axis();
            // A planar circular edge must be between its cap plane and a coaxial cylinder.
            if top.cross(axis).length() > 1e-9 || side.dot(axis).abs() > 1e-9 {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "circular router edges require a cap plane and cylindrical side",
                });
            }
            let [top_use, side_use] = uses;
            let top_face = b
                .faceuse(top_use)?
                .face
                .ok_or_else(|| SolidError::invalid_input(OP, "missing support"))?;
            let side_face = b
                .faceuse(side_use)?
                .face
                .ok_or_else(|| SolidError::invalid_input(OP, "missing support"))?;
            if !matches!(
                face::surface(b, top_face)?.map(Surface::kind),
                Some(SurfaceKind::Plane(_))
            ) || !matches!(
                face::surface(b, side_face)?.map(Surface::kind),
                Some(SurfaceKind::Cone(_))
            ) {
                return Err(SolidError::Unsupported {
                    operation: OP,
                    reason: "circular router edges require a planar cap and cylindrical side",
                });
            }
            let axis = if c.inside_out() { -axis } else { axis };
            create_rotational_sweep(
                k,
                &refs,
                data.position.origin(),
                axis,
                data.domain.length(),
                true,
            )
        }
        _ => Err(SolidError::Unsupported {
            operation: OP,
            reason: "router edge must be straight or a planar circular arc",
        }),
    }
}

#[cfg(test)]
#[path = "edge_section_tests.rs"]
mod tests;
