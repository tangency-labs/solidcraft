//! (not upstream) A closed surface's seam turned into the widest gap of its face's
//! boundary: the surface placed again about its own axis so that its angle parameter
//! starts where the face does not reach, or on the face's own seam.
//!
//! Upstream: none (solidcore's STEP reader, #2578). The idea is SolidCraft's reader
//! (`crates/step/src/topology/swept.rs` `widest_gap`, `common_angle`, `span`,
//! `crates/step/src/topology/frames.rs` `about_axis`, `quarters_turned`,
//! `turn_frames`, and `crates/geom/src/brep/form.rs` `tube_turned`, at a5b71ca2;
//! MIT OR Apache-2.0) and the frame step-brep's `seam_frame` and `face_seam_point`
//! choose for SMLib's import (left as they are for #2585). The computations are the
//! kernel's own: the face's exact parameter window
//! ([`crate::surface::step_window::face_window`]), [`Axis2Placement`] and
//! [`Surface::transform`].
//!
//! | Rust                      | SolidCraft                                                   |
//! |---------------------------|--------------------------------------------------------------|
//! | [`seam_into_widest_gap`]  | `swept::span` (a closed conic's start), `frames::turn_frames` (the turn), `form::tube_turned` |
//! | [`PlacedSeam`]            | the turned frame and `Span::Turn { start }`                  |
//! | [`ClosedSeam`]            | `swept::widest_gap`'s `(start, width)`, `common_angle`        |
//!
//! Deviations: not a port. See [`seam_into_widest_gap`] for the rules. SolidCraft and
//! step-brep take the boundary's angles from sampled points (step-brep 17 an edge, for
//! cylinders and cones only); here they are the face's window from each edge's
//! extrema, for every closed kind. SolidCraft starts a closed profile where the face
//! starts (the end of the gap); the seam here goes in the gap's middle, as SMLib's
//! import puts it, so the face keeps half the gap on either side of it.

use crate::error::{Result, SolidError};
use crate::geom::axis2_placement::Axis2Placement;
use crate::geom::extent1d::Extent1d;
use crate::kernel::Kernel;
use crate::math::{rad_to_deg, Point2, Point3, Vec2, PI};
use crate::surface::step_window::{face_window, BoundaryEdge, WindowMargin};
use crate::surface::surface::{Surface, SurfaceKind};

const OP: &str = "surface::step_seam::seam_into_widest_gap";

/// One turn, in degrees (the STEP angles' period).
const TURN: f64 = 360.0;

/// A quarter turn, in degrees.
const QUARTER: f64 = 90.0;

/// Where the seam of one closed direction of a surface goes.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ClosedSeam {
    /// The seam's angle in degrees, in `[0, 360)`, in the input surface's STEP
    /// parameter of that direction; `None` when the seam stays where it is (the face
    /// covers the full turn and no seam point is given).
    pub angle: Option<f64>,
    /// The gap the face leaves in that direction, in degrees: its minimum in
    /// `[0, 360)` is where the gap starts, its length is the gap's width (its maximum
    /// may pass 360). `None` when the face covers the full turn.
    pub gap: Option<Extent1d>,
}

/// A closed surface with its seam in the widest gap of a face's boundary.
#[derive(Debug, Clone)]
pub struct PlacedSeam {
    /// The input surface turned about its own axis so that its STEP u (the angle
    /// about the axis) is 0 at [`Self::u`]'s angle: the same geometry, its point at
    /// `(u, v)` the input's at `(u + angle, v)`. The input itself (a copy) when the
    /// seam stays where it is.
    pub surface: Surface,
    /// The seam about the axis (STEP u).
    pub u: ClosedSeam,
    /// For a torus closed round its tube, the seam round the tube (STEP v): the
    /// face's v lies within `[angle - 360, angle]`. `None` for the other surfaces.
    pub v: Option<ClosedSeam>,
}

/// `surface` placed so that its seam lies in the widest gap of the face bounded by
/// `loops` and `vertices`, or through `seam_point` where the face covers the full
/// turn; `tol` is the distance within which the boundary lies on the surface.
///
/// `loops` and `vertices` are the face's edge loops, oriented as
/// [`BoundaryEdge`] says, and the points of its vertex loops (an apex, a pole), as
/// [`face_window`] takes them.
///
/// **Closed surfaces.** A cone or cylinder, sphere, torus or surface of revolution
/// whose STEP u domain is a full turn: u is the angle in degrees about the Z axis of
/// the surface's position, from its X axis, or for a surface of revolution from its
/// profile (which the kernel keeps on the position's X/Z plane, on either side of the
/// axis; a profile off such a plane is not taken). A torus whose STEP v domain is a
/// full turn is closed round its tube too: v is the tube angle in degrees from the
/// outer equator.
///
/// **The gap.** A face is connected, so the angles it covers in a closed direction
/// are one arc: its window there ([`face_window`] without margin: each edge's
/// extremes found exactly, unwound along the loops). The gap is the rest of the turn,
/// from the window's end to its start one period on; a window of a full period (a
/// loop that goes round, or a boundary that spans the turn) leaves none.
///
/// **The seam** goes in the middle of the gap. The surface is turned about its own
/// axis by that angle ([`Surface::transform`] with the rigid motion that takes its
/// position onto the turned one, so every kind keeps its own data): the new X axis is
/// `cos a X + sin a Y`, with the sine and cosine taken in degrees after an exact
/// reduction to the octant, so a seam on a quarter line gets exact axes. Where the
/// face covers the full turn, the seam stays where it is, unless `seam_point` is
/// given: then the new X axis is exactly the unit radial direction of `seam_point`
/// (normally a vertex of the face's seam edge). A seam point is not used where the
/// face leaves a gap.
///
/// **Round a torus's tube.** STEP measures the tube angle from the outer equator,
/// and the kernel's torus keeps that (its evaluation has no tube start), so the seam
/// in v is no placement: [`PlacedSeam::v`] gives its angle, and the face's v lies
/// within `[angle - 360, angle]` (for
/// [`crate::surface::torus::Torus::adjust_step_uv_domain`]). A sphere's v (latitude)
/// and the v of the other surfaces are not closed.
///
/// # Errors
///
/// [`SolidError::InvalidInput`] for a `tol` that is negative or not finite, a
/// `seam_point` that is not finite or (where it is used) within `tol` of the axis or
/// of the centre circle of a torus's tube, an edge interval that is not finite, a
/// surface that is not one of the closed kinds or is not closed about its axis, or a
/// surface of revolution whose profile's middle point is within `tol` of the axis.
/// [`SolidError::Unsupported`] for a surface of revolution whose profile is not on a
/// plane through its axis. Those of [`face_window`], [`Surface::evaluate_step_point`]
/// and [`Surface::transform`].
pub fn seam_into_widest_gap(
    k: &Kernel,
    surface: &Surface,
    loops: &[Vec<BoundaryEdge<'_>>],
    vertices: &[Point3],
    seam_point: Option<Point3>,
    tol: f64,
) -> Result<PlacedSeam> {
    if !(tol.is_finite() && tol >= 0.0) {
        return Err(SolidError::invalid_input(
            OP,
            "tolerance negative or not finite",
        ));
    }
    if seam_point.is_some_and(|p| !(p.x.is_finite() && p.y.is_finite() && p.z.is_finite())) {
        return Err(SolidError::invalid_input(OP, "seam point not finite"));
    }
    if loops
        .iter()
        .flatten()
        .any(|e| !(e.interval.min().is_finite() && e.interval.max().is_finite()))
    {
        return Err(SolidError::invalid_input(OP, "edge interval not finite"));
    }
    let closed = closed(k, surface, tol)?;
    let frame = &closed.frame;
    let window = face_window(k, surface, loops, vertices, WindowMargin::default(), tol)?;

    // About the axis, with the seam point's radial direction where it is used.
    let (u, through) = match gap(window.u_interval()) {
        Some(gap) => (gap_seam(gap), None),
        None => match seam_point {
            None => (
                ClosedSeam {
                    angle: None,
                    gap: None,
                },
                None,
            ),
            Some(p) => {
                let q = in_plane(frame, p);
                if q.length() <= tol {
                    return Err(SolidError::invalid_input(OP, "seam point on the axis"));
                }
                let radial = (frame.x_axis() * q.x + frame.y_axis() * q.y).unitize()?;
                (
                    ClosedSeam {
                        angle: Some(angle_of(q)),
                        gap: None,
                    },
                    Some(radial * closed.side),
                )
            }
        },
    };

    let v = match closed.tube {
        None => None,
        Some(major) => Some(match gap(window.v_interval()) {
            Some(gap) => gap_seam(gap),
            None => ClosedSeam {
                angle: match seam_point {
                    None => None,
                    Some(p) => {
                        let g = in_tube(frame, major, p);
                        if g.length() <= tol {
                            return Err(SolidError::invalid_input(
                                OP,
                                "seam point on the centre circle of the torus's tube",
                            ));
                        }
                        Some(angle_of(g))
                    }
                },
                gap: None,
            },
        }),
    };

    let mut placed = surface.clone();
    if let Some(angle) = u.angle {
        // The position turned by the seam's angle about its own axis.
        let x = through.unwrap_or_else(|| {
            let (sin, cos) = sin_cos_deg(angle);
            (frame.x_axis() * cos + frame.y_axis() * sin) * closed.side
        });
        let position = &closed.position;
        let turned = Axis2Placement::new(position.origin(), x, position.z_axis().cross(x));
        placed.transform(k, &Axis2Placement::from_to(position, &turned), None)?;
    }
    Ok(PlacedSeam {
        surface: placed,
        u,
        v,
    })
}

/// A closed surface's position, the frame its STEP u is measured in (the position's
/// origin and Z axis, u = 0 along its X axis, or for a surface of revolution whose
/// profile lies on the other side of the axis, along -X: `side` -1), and for a torus
/// closed round its tube, its major radius.
struct Closed {
    position: Axis2Placement,
    frame: Axis2Placement,
    side: f64,
    tube: Option<f64>,
}

fn closed(k: &Kernel, surface: &Surface, tol: f64) -> Result<Closed> {
    let (rev, major) = match surface.kind() {
        SurfaceKind::Cone(c) => (c.rev(), None),
        SurfaceKind::Sphere(s) => (s.base(), None),
        SurfaceKind::Torus(t) => (t.base(), Some(t.major_radius())),
        SurfaceKind::SurfOfRevolution(r) => (r.as_ref(), None),
        _ => {
            return Err(SolidError::invalid_input(
                OP,
                "not a cone, cylinder, sphere, torus or surface of revolution",
            ))
        }
    };
    let domain = rev.anal_uv_domain;
    if domain.u_interval().length() < TURN {
        return Err(SolidError::invalid_input(
            OP,
            "the surface is not closed about its axis",
        ));
    }
    let position = rev.position;
    let side = if let SurfaceKind::SurfOfRevolution(r) = surface.kind() {
        // u = 0 is the profile, which lies on the position's X/Z plane, on one side.
        if !r.planar_generator() {
            return Err(SolidError::Unsupported {
                operation: OP,
                reason: "a surface of revolution whose profile is not on a plane through its axis",
            });
        }
        let middle = surface.evaluate_step_point(k, Point2::new(0.0, domain.v_interval().mid()))?;
        let x = (middle - position.origin()).dot(position.x_axis());
        if x.abs() <= tol {
            return Err(SolidError::invalid_input(
                OP,
                "the profile's middle point is on the axis",
            ));
        }
        x.signum()
    } else {
        1.0
    };
    let frame = Axis2Placement::new(
        position.origin(),
        position.x_axis() * side,
        position.y_axis() * side,
    );
    Ok(Closed {
        position,
        frame,
        side,
        tube: major.filter(|_| domain.v_interval().length() >= TURN),
    })
}

/// The gap a face's window `covered` leaves in a closed direction: from its end to
/// its start one period on, or `None` for a full period.
fn gap(covered: Extent1d) -> Option<Extent1d> {
    let width = TURN - covered.length();
    if width <= 0.0 {
        return None;
    }
    let start = wrap(covered.max());
    Some(Extent1d::from_min_max(start, start + width))
}

/// The seam in the middle of `gap`.
fn gap_seam(gap: Extent1d) -> ClosedSeam {
    ClosedSeam {
        angle: Some(wrap(gap.min() + 0.5 * gap.length())),
        gap: Some(gap),
    }
}

/// A point's coordinates on the X and Y axes of `frame`, from its origin.
fn in_plane(frame: &Axis2Placement, p: Point3) -> Vec2 {
    let d = p - frame.origin();
    Vec2::new(d.dot(frame.x_axis()), d.dot(frame.y_axis()))
}

/// A point's place in its torus's tube: its distance from the axis less the major
/// radius, and its height (the tube angle is its angle).
fn in_tube(frame: &Axis2Placement, major: f64, p: Point3) -> Vec2 {
    let d = p - frame.origin();
    Vec2::new(in_plane(frame, p).length() - major, d.dot(frame.z_axis()))
}

/// `a` in `[0, 360)`.
fn wrap(a: f64) -> f64 {
    let r = a.rem_euclid(TURN);
    if r >= TURN {
        0.0
    } else {
        r
    }
}

/// The angle of `q` (not zero) in degrees in `[0, 360)`: the arc tangent of the
/// smaller coordinate over the larger, placed in its octant, so the angle of a point
/// on an axis is an exact multiple of 90.
fn angle_of(q: Vec2) -> f64 {
    let (ax, ay) = (q.x.abs(), q.y.abs());
    let a = if ay <= ax {
        rad_to_deg((ay / ax).atan())
    } else {
        QUARTER - rad_to_deg((ax / ay).atan())
    };
    wrap(if q.x >= 0.0 {
        if q.y >= 0.0 {
            a
        } else {
            TURN - a
        }
    } else if q.y >= 0.0 {
        0.5 * TURN - a
    } else {
        0.5 * TURN + a
    })
}

/// `(sin a, cos a)` of `a` in degrees: `a` reduced exactly to within 45 degrees of a
/// quarter line, so a quarter line gets exact values.
fn sin_cos_deg(a: f64) -> (f64, f64) {
    let a = wrap(a);
    let quarter = (a / QUARTER).round();
    // (exact: below 45 degrees nothing is taken off, and above it a and the quarter
    // line are within a factor of two)
    let r = a - QUARTER * quarter;
    let (s, c) = (r * (PI / 180.0)).sin_cos();
    if quarter == 1.0 {
        (c, -s)
    } else if quarter == 2.0 {
        (-s, -c)
    } else if quarter == 3.0 {
        (-c, s)
    } else {
        (s, c)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::curve::curve::Curve;
    use crate::curve::ellipse::Ellipse;
    use crate::curve::line::Line;
    use crate::geom::extent2d::Extent2d;
    use crate::math::Vec3;
    use crate::nurbs::b_spline_curve::{BSplineCurve, BSplineCurveForm, CanonicalData, KnotType};
    use crate::surface::cone::Cone;
    use crate::surface::plane::Plane;
    use crate::surface::sphere::Sphere;
    use crate::surface::surf_of_revolution::SurfOfRevolution;
    use crate::surface::torus::{Torus, TorusOptions};
    use std::f64::consts::FRAC_1_SQRT_2;

    /// The face tolerance of the cases (a usual STEP distance uncertainty).
    const TOL: f64 = 1e-6;

    const KEPT: ClosedSeam = ClosedSeam {
        angle: None,
        gap: None,
    };

    fn k() -> Kernel {
        Kernel::default()
    }

    fn p(x: f64, y: f64, z: f64) -> Point3 {
        Point3::new(x, y, z)
    }

    fn v3(x: f64, y: f64, z: f64) -> Vec3 {
        Vec3::new(x, y, z)
    }

    /// The circle of `radius` about `centre`, its 0 degree point along `x` and its 90
    /// degree point along `y` (the full curve; edges take their intervals on it).
    fn circle(k: &Kernel, centre: Point3, x: Vec3, y: Vec3, radius: f64) -> Curve {
        let frame = Axis2Placement::new(centre, x, y);
        Curve::from(Ellipse::create_canonical_circle(k, &frame, radius, None, None).unwrap())
    }

    /// The segment from `a` to `b`, its STEP parameter 0 at `a` and 1 at `b`.
    fn line(a: Point3, b: Point3) -> Curve {
        Curve::from(Line::create_line_segment(3, a, b, None).unwrap())
    }

    fn edge(curve: &Curve, lo: f64, hi: f64, forward: bool) -> BoundaryEdge<'_> {
        BoundaryEdge {
            curve,
            interval: Extent1d::from_min_max(lo, hi),
            forward,
        }
    }

    /// A line used from its start to its end (`forward`) or back.
    fn along(curve: &Curve, forward: bool) -> BoundaryEdge<'_> {
        edge(curve, 0.0, 1.0, forward)
    }

    fn seam(k: &Kernel, s: &Surface, loops: &[Vec<BoundaryEdge<'_>>]) -> PlacedSeam {
        seam_into_widest_gap(k, s, loops, &[], None, TOL).unwrap()
    }

    fn position(s: &Surface) -> Axis2Placement {
        match s.kind() {
            SurfaceKind::Cone(c) => *c.rev().position(),
            SurfaceKind::Sphere(s) => *s.base().position(),
            SurfaceKind::Torus(t) => *t.base().position(),
            SurfaceKind::SurfOfRevolution(r) => *r.position(),
            _ => panic!("not a closed surface"),
        }
    }

    /// The placed surface has the input's points: its point at `(u, v)` is the input's
    /// at `(u + turn, v)`, for 16 angles round the turn and each `v`. Both are
    /// evaluated with their own `cos`/`sin` of the angle in radians (each within an
    /// ulp) on placements that agree to rounding, so the points agree within a few
    /// ulps of the coordinates' size `scale`: the bound is 8 ulps of it.
    #[track_caller]
    fn assert_same_points(
        k: &Kernel,
        old: &Surface,
        new: &Surface,
        turn: f64,
        vs: &[f64],
        scale: f64,
    ) {
        let bound = 8.0 * f64::EPSILON * scale;
        for i in 0..16 {
            let u = 22.5 * f64::from(i) + 3.25;
            for &v in vs {
                let a = new.evaluate_step_point(k, Point2::new(u, v)).unwrap();
                let b = old
                    .evaluate_step_point(k, Point2::new((u + turn).rem_euclid(360.0), v))
                    .unwrap();
                assert!(
                    a.distance(b) <= bound,
                    "at ({u}, {v}): {a:?} is not {b:?} (distance {}, bound {bound})",
                    a.distance(b)
                );
            }
        }
    }

    /// `a` equals the closed form `b` to rounding (the window's angles are exact to a
    /// few ulps of 360 degrees).
    #[track_caller]
    fn assert_angle(a: f64, b: f64) {
        assert!((a - b).abs() <= 1e-12, "{a} is not {b}");
    }

    /// The vector `a` is the closed form `b` to rounding (two ulps of 1).
    #[track_caller]
    fn assert_unit(a: Vec3, b: Vec3) {
        assert!(a.distance(b) <= 2.0 * f64::EPSILON, "{a:?} is not {b:?}");
    }

    fn cylinder(k: &Kernel, frame: &Axis2Placement, r: f64) -> Surface {
        Surface::from(Cone::create_canonical_cylinder(k, frame, r).unwrap())
    }

    /// The face of a cylinder of radius 2 about the frame's Z between the heights 0
    /// and 3 and the angles 0 and 180 degrees (its arcs split at 90): boundary points
    /// at 0, 90 and 180 degrees.
    struct HalfCylinder {
        bottom: Curve,
        top: Curve,
        at0: Curve,
        at180: Curve,
    }

    impl HalfCylinder {
        fn new(k: &Kernel, frame: &Axis2Placement) -> Self {
            let (o, x, y, z) = (
                frame.origin(),
                frame.x_axis(),
                frame.y_axis(),
                frame.z_axis(),
            );
            Self {
                bottom: circle(k, o, x, y, 2.0),
                top: circle(k, o + 3.0 * z, x, y, 2.0),
                at0: line(o + 2.0 * x, o + 2.0 * x + 3.0 * z),
                at180: line(o - 2.0 * x, o - 2.0 * x + 3.0 * z),
            }
        }

        /// Counterclockwise in (u, v): along the bottom, up at 180, back along the top,
        /// down at 0.
        fn loops(&self) -> Vec<Vec<BoundaryEdge<'_>>> {
            vec![vec![
                edge(&self.bottom, 0.0, 90.0, true),
                edge(&self.bottom, 90.0, 180.0, true),
                along(&self.at180, true),
                edge(&self.top, 90.0, 180.0, false),
                edge(&self.top, 0.0, 90.0, false),
                along(&self.at0, false),
            ]]
        }
    }

    #[test]
    fn cylinder_seam_into_its_widest_gap() {
        let k = k();
        let frame = Axis2Placement::IDENTITY;
        let s = cylinder(&k, &frame, 2.0);
        let face = HalfCylinder::new(&k, &frame);
        let placed = seam(&k, &s, &face.loops());

        // The boundary covers 0..180 degrees: the gap is 180..360, its middle 270.
        assert_eq!(placed.u.angle, Some(270.0));
        let gap = placed.u.gap.unwrap();
        assert_eq!((gap.min(), gap.length()), (180.0, 180.0));
        assert!(placed.v.is_none());

        // The new X axis is exactly the direction at 270 degrees.
        let pos = position(&placed.surface);
        assert_eq!(pos.x_axis(), v3(0.0, -1.0, 0.0));
        assert_eq!(pos.y_axis(), v3(1.0, 0.0, 0.0));
        assert_eq!(pos.z_axis(), v3(0.0, 0.0, 1.0));
        assert_eq!(pos.origin(), p(0.0, 0.0, 0.0));
        assert_same_points(&k, &s, &placed.surface, 270.0, &[-1.0, 0.0, 1.5, 3.0], 4.0);
    }

    #[test]
    fn cylinder_seam_in_a_general_frame() {
        // The same face on a cylinder whose axis is (4/5, -3/5, 0) through (1, 2, 3).
        let k = k();
        let frame = Axis2Placement::new(p(1.0, 2.0, 3.0), v3(0.6, 0.8, 0.0), v3(0.0, 0.0, 1.0));
        let s = cylinder(&k, &frame, 2.0);
        let face = HalfCylinder::new(&k, &frame);
        let placed = seam(&k, &s, &face.loops());

        assert_angle(placed.u.angle.unwrap(), 270.0);
        let pos = position(&placed.surface);
        // At 270 degrees: -Y of the frame.
        assert_unit(pos.x_axis(), v3(0.0, 0.0, -1.0));
        assert_unit(pos.z_axis(), frame.z_axis());
        let turn = placed.u.angle.unwrap();
        assert_same_points(&k, &s, &placed.surface, turn, &[-1.0, 0.0, 1.5, 3.0], 8.0);
    }

    #[test]
    fn cylinder_full_band_keeps_its_seam() {
        let k = k();
        let frame = Axis2Placement::IDENTITY;
        let s = cylinder(&k, &frame, 2.0);
        let bottom = circle(&k, p(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 2.0);
        let top = circle(&k, p(0.0, 0.0, 3.0), Vec3::X, Vec3::Y, 2.0);
        let loops = vec![
            vec![edge(&bottom, 0.0, 360.0, true)],
            vec![edge(&top, 0.0, 360.0, false)],
        ];
        let placed = seam(&k, &s, &loops);

        assert_eq!(placed.u, KEPT);
        assert_eq!(position(&placed.surface), frame);
        assert_same_points(&k, &s, &placed.surface, 0.0, &[0.0, 3.0], 4.0);

        // The circles as two half edges each: still the full turn.
        let halves = vec![
            vec![
                edge(&bottom, 0.0, 180.0, true),
                edge(&bottom, 180.0, 360.0, true),
            ],
            vec![
                edge(&top, 180.0, 360.0, false),
                edge(&top, 0.0, 180.0, false),
            ],
        ];
        assert_eq!(seam(&k, &s, &halves).u, KEPT);
    }

    #[test]
    fn cylinder_full_band_seam_through_its_seam_point() {
        // A band of radius 5 with a seam vertex at (3, 4, 1): the X axis is exactly
        // its radial direction (3/5, 4/5, 0).
        let k = k();
        let s = cylinder(&k, &Axis2Placement::IDENTITY, 5.0);
        let bottom = circle(&k, p(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 5.0);
        let top = circle(&k, p(0.0, 0.0, 3.0), Vec3::X, Vec3::Y, 5.0);
        let loops = vec![
            vec![edge(&bottom, 0.0, 360.0, true)],
            vec![edge(&top, 0.0, 360.0, false)],
        ];
        let placed =
            seam_into_widest_gap(&k, &s, &loops, &[], Some(p(3.0, 4.0, 1.0)), TOL).unwrap();

        let angle = placed.u.angle.unwrap();
        assert_angle(angle, 4.0_f64.atan2(3.0).to_degrees());
        assert!(placed.u.gap.is_none());
        let pos = position(&placed.surface);
        assert_eq!(pos.x_axis(), v3(3.0 / 5.0, 4.0 / 5.0, 0.0));
        assert_unit(pos.y_axis(), v3(-4.0 / 5.0, 3.0 / 5.0, 0.0));
        assert_same_points(&k, &s, &placed.surface, angle, &[0.0, 3.0], 8.0);
    }

    #[test]
    fn cylinder_oblique_ellipse_across_the_seam() {
        // The cylinder of radius 1 cut by the plane z = x: the ellipse (cos t, sin t,
        // cos t) over -90..90 degrees (across the seam). The face below it, down to the
        // arc at z = -2, covers -90..90: the gap is 90..270, the seam at 180.
        let k = k();
        let s = cylinder(&k, &Axis2Placement::IDENTITY, 1.0);
        let ellipse = Curve::from(
            Ellipse::create_canonical(
                &k,
                &Axis2Placement::new(p(0.0, 0.0, 0.0), v3(1.0, 0.0, 1.0), Vec3::Y),
                2.0_f64.sqrt(),
                1.0,
                None,
                None,
            )
            .unwrap(),
        );
        let bottom = circle(&k, p(0.0, 0.0, -2.0), Vec3::X, Vec3::Y, 1.0);
        let up = line(p(0.0, 1.0, -2.0), p(0.0, 1.0, 0.0));
        let down = line(p(0.0, -1.0, 0.0), p(0.0, -1.0, -2.0));
        let loops = vec![vec![
            edge(&bottom, -90.0, 90.0, true),
            along(&up, true),
            edge(&ellipse, -90.0, 90.0, false),
            along(&down, true),
        ]];
        let placed = seam(&k, &s, &loops);

        let angle = placed.u.angle.unwrap();
        assert_angle(angle, 180.0);
        let gap = placed.u.gap.unwrap();
        assert_angle(gap.min(), 90.0);
        assert_angle(gap.length(), 180.0);
        assert_unit(position(&placed.surface).x_axis(), v3(-1.0, 0.0, 0.0));
        assert_same_points(&k, &s, &placed.surface, angle, &[-2.0, 0.0, 1.0], 4.0);
    }

    #[test]
    fn cylinder_with_a_spline_edge() {
        // The quarter circle 0..90 degrees at z = 0 as a rational quadratic spline,
        // up the line at 90, back along the circle at z = 1, down the line at 0: the
        // gap is 90..360, the seam at 225.
        let k = k();
        let s = cylinder(&k, &Axis2Placement::IDENTITY, 1.0);
        let points = [p(1.0, 0.0, 0.0), p(1.0, 1.0, 0.0), p(0.0, 1.0, 0.0)];
        let quarter = Curve::from(
            BSplineCurve::create_canonical(&CanonicalData {
                dim: 3,
                degree: 2,
                control_points: &points,
                form: BSplineCurveForm::Unspecified,
                knot_multiplicities: &[3, 3],
                knots: &[0.0, 1.0],
                knot_type: KnotType::Unspecified,
                weights: Some(&[1.0, FRAC_1_SQRT_2, 1.0]),
                trim: None,
            })
            .unwrap(),
        );
        let top = circle(&k, p(0.0, 0.0, 1.0), Vec3::X, Vec3::Y, 1.0);
        let up = line(p(0.0, 1.0, 0.0), p(0.0, 1.0, 1.0));
        let down = line(p(1.0, 0.0, 1.0), p(1.0, 0.0, 0.0));
        let loops = vec![vec![
            edge(&quarter, 0.0, 1.0, true),
            along(&up, true),
            edge(&top, 0.0, 90.0, false),
            along(&down, true),
        ]];
        let placed = seam(&k, &s, &loops);

        let angle = placed.u.angle.unwrap();
        assert_angle(angle, 225.0);
        assert_unit(
            position(&placed.surface).x_axis(),
            v3(-FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0),
        );
        assert_same_points(&k, &s, &placed.surface, angle, &[0.0, 1.0], 2.0);
    }

    #[test]
    fn cone_seam_with_its_apex_on_the_boundary() {
        // A cone of semi-angle 30 degrees and radius 1 at height 0: its apex at height
        // -sqrt(3). The face runs from the apex up the line at 180 degrees, back along
        // the arc 180..90 at height 0 and down the line at 90: the face covers 90..180,
        // the gap is 180..450, its middle 315.
        let k = k();
        let s = Surface::from(
            Cone::create_canonical_with_semi_angle(
                &k,
                &Axis2Placement::IDENTITY,
                1.0,
                30.0,
                None,
                false,
                false,
            )
            .unwrap(),
        );
        let apex = p(0.0, 0.0, -(3.0_f64.sqrt()));
        let arc = circle(&k, p(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 1.0);
        let up180 = line(apex, p(-1.0, 0.0, 0.0));
        let down90 = line(p(0.0, 1.0, 0.0), apex);
        let loops = vec![vec![
            along(&up180, true),
            edge(&arc, 90.0, 180.0, false),
            along(&down90, true),
        ]];
        let placed = seam(&k, &s, &loops);

        let angle = placed.u.angle.unwrap();
        assert_angle(angle, 315.0);
        let gap = placed.u.gap.unwrap();
        assert_angle(gap.min(), 180.0);
        assert_angle(gap.length(), 270.0);
        assert_unit(
            position(&placed.surface).x_axis(),
            v3(FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0),
        );
        assert_same_points(&k, &s, &placed.surface, angle, &[-1.0, 0.0, 2.0], 4.0);
    }

    fn sphere(k: &Kernel, r: f64) -> Surface {
        Surface::from(Sphere::create_canonical(k, &Axis2Placement::IDENTITY, r).unwrap())
    }

    #[test]
    fn sphere_lune_seam_into_its_longitude_gap() {
        // The lune of a sphere of radius 2 between the meridians at 0 and 90 degrees:
        // north along the one at 90, south along the one at 0, through both poles
        // (where the longitude is undefined), each meridian as two edges meeting at the
        // equator. The gap is 90..360, its middle 225.
        let k = k();
        let s = sphere(&k, 2.0);
        let o = p(0.0, 0.0, 0.0);
        let at0 = circle(&k, o, Vec3::X, Vec3::Z, 2.0);
        let at90 = circle(&k, o, Vec3::Y, Vec3::Z, 2.0);
        let loops = vec![vec![
            edge(&at90, -90.0, 0.0, true),
            edge(&at90, 0.0, 90.0, true),
            edge(&at0, 0.0, 90.0, false),
            edge(&at0, -90.0, 0.0, false),
        ]];
        let placed = seam(&k, &s, &loops);

        let angle = placed.u.angle.unwrap();
        assert_angle(angle, 225.0);
        let gap = placed.u.gap.unwrap();
        assert_angle(gap.min(), 90.0);
        assert_angle(gap.length(), 270.0);
        assert!(placed.v.is_none());
        assert_unit(
            position(&placed.surface).x_axis(),
            v3(-FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0),
        );
        let vs = [-90.0, -45.0, 0.0, 30.0, 90.0];
        assert_same_points(&k, &s, &placed.surface, angle, &vs, 2.0);
    }

    #[test]
    fn sphere_cap_at_the_pole() {
        // The cap above latitude 60 of a sphere of radius 2: its circle (radius 1 at
        // height sqrt(3)) covers every longitude, so the seam stays, or goes through
        // the seam point (0, 1, sqrt(3)) at exactly 90 degrees.
        let k = k();
        let s = sphere(&k, 2.0);
        let h = 3.0_f64.sqrt();
        let rim = circle(&k, p(0.0, 0.0, h), Vec3::X, Vec3::Y, 1.0);
        let loops = vec![vec![edge(&rim, 0.0, 360.0, true)]];

        let kept = seam(&k, &s, &loops);
        assert_eq!(kept.u, KEPT);
        assert_eq!(position(&kept.surface), Axis2Placement::IDENTITY);

        let through = seam_into_widest_gap(&k, &s, &loops, &[], Some(p(0.0, 1.0, h)), TOL).unwrap();
        assert_eq!(through.u.angle, Some(90.0));
        assert_eq!(position(&through.surface).x_axis(), v3(0.0, 1.0, 0.0));
        assert_same_points(&k, &s, &through.surface, 90.0, &[60.0, 75.0, 90.0], 2.0);
    }

    #[test]
    fn a_face_without_boundary_keeps_its_seam() {
        // A whole sphere: no edges, every angle.
        let k = k();
        let s = sphere(&k, 1.0);
        let placed = seam(&k, &s, &[]);
        assert_eq!(placed.u, KEPT);
        assert_eq!(position(&placed.surface), Axis2Placement::IDENTITY);
    }

    fn torus(k: &Kernel) -> Surface {
        Surface::from(
            Torus::create_canonical(
                k,
                &Axis2Placement::IDENTITY,
                3.0,
                1.0,
                TorusOptions::default(),
                None,
            )
            .unwrap(),
        )
    }

    #[test]
    fn torus_seam_about_its_axis() {
        // Half a torus (major radius 3, minor 1): along the outer equator 0..180, round
        // the tube circle at 180, back along the equator (its seam round the tube, used
        // twice) and round the tube circle at 0 against it. About the axis the gap is
        // 180..360; round the tube the face covers the full turn, so that seam stays.
        let k = k();
        let s = torus(&k);
        let tube0 = circle(&k, p(3.0, 0.0, 0.0), Vec3::X, Vec3::Z, 1.0);
        let tube180 = circle(&k, p(-3.0, 0.0, 0.0), -Vec3::X, Vec3::Z, 1.0);
        let equator = circle(&k, p(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 4.0);
        let loops = vec![vec![
            edge(&equator, 0.0, 180.0, true),
            edge(&tube180, 0.0, 360.0, true),
            edge(&equator, 0.0, 180.0, false),
            edge(&tube0, 0.0, 360.0, false),
        ]];
        let placed = seam(&k, &s, &loops);

        assert_eq!(placed.u.angle, Some(270.0));
        assert_eq!(position(&placed.surface).x_axis(), v3(0.0, -1.0, 0.0));
        assert_eq!(placed.v, Some(KEPT));
        let vs = [0.0, 45.0, 180.0, 300.0];
        assert_same_points(&k, &s, &placed.surface, 270.0, &vs, 4.0);
    }

    #[test]
    fn torus_seam_round_its_tube() {
        // The outer band of the torus between the tube angles -30 and 30 degrees: its
        // circles of radius 3 + cos 30 at heights -/+ 1/2 cover every angle about the
        // axis (that seam stays); round the tube the gap is 30..330, its middle 180
        // (the inner equator).
        let k = k();
        let s = torus(&k);
        let r = 3.0 + 3.0_f64.sqrt() / 2.0;
        let low = circle(&k, p(0.0, 0.0, -0.5), Vec3::X, Vec3::Y, r);
        let high = circle(&k, p(0.0, 0.0, 0.5), Vec3::X, Vec3::Y, r);
        let loops = vec![
            vec![edge(&low, 0.0, 360.0, true)],
            vec![edge(&high, 0.0, 360.0, false)],
        ];
        let placed = seam(&k, &s, &loops);

        assert_eq!(placed.u, KEPT);
        let v = placed.v.unwrap();
        assert_angle(v.angle.unwrap(), 180.0);
        let gap = v.gap.unwrap();
        assert_angle(gap.min(), 30.0);
        assert_angle(gap.length(), 300.0);
        assert_eq!(position(&placed.surface), Axis2Placement::IDENTITY);
    }

    #[test]
    fn torus_v_seam_through_its_seam_point() {
        // The whole tube between the tube circles at 0 and 90 degrees about the axis,
        // its seam round the tube on the top circle (tube angle 90), whose vertex
        // (3, 0, 1) is the seam point: round the tube the seam goes through it.
        let k = k();
        let s = torus(&k);
        let tube0 = circle(&k, p(3.0, 0.0, 0.0), Vec3::X, Vec3::Z, 1.0);
        let tube90 = circle(&k, p(0.0, 3.0, 0.0), Vec3::Y, Vec3::Z, 1.0);
        let top = circle(&k, p(0.0, 0.0, 1.0), Vec3::X, Vec3::Y, 3.0);
        let loops = vec![vec![
            edge(&top, 0.0, 90.0, true),
            edge(&tube90, -270.0, 90.0, true),
            edge(&top, 0.0, 90.0, false),
            edge(&tube0, -270.0, 90.0, false),
        ]];
        let placed =
            seam_into_widest_gap(&k, &s, &loops, &[], Some(p(3.0, 0.0, 1.0)), TOL).unwrap();

        assert_angle(placed.u.angle.unwrap(), 225.0);
        assert_eq!(
            placed.v,
            Some(ClosedSeam {
                angle: Some(90.0),
                gap: None
            })
        );
    }

    #[test]
    fn revolution_of_a_line_profile() {
        // The line from (2, 0, 0) to (3, 0, 2) about Z; the face along the circle of
        // radius 2 from 0 to 180 degrees, up the profile's copy at 180, back along the
        // circle of radius 3 and down the profile: the gap is 180..360.
        let k = k();
        let profile = line(p(2.0, 0.0, 0.0), p(3.0, 0.0, 2.0));
        let s = Surface::from(
            SurfOfRevolution::create_canonical(
                &k,
                profile.clone(),
                p(0.0, 0.0, 0.0),
                Vec3::Z,
                false,
            )
            .unwrap(),
        );
        let at180 = line(p(-2.0, 0.0, 0.0), p(-3.0, 0.0, 2.0));
        let bottom = circle(&k, p(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 2.0);
        let top = circle(&k, p(0.0, 0.0, 2.0), Vec3::X, Vec3::Y, 3.0);
        let loops = vec![vec![
            edge(&bottom, 0.0, 180.0, true),
            along(&at180, true),
            edge(&top, 0.0, 180.0, false),
            along(&profile, false),
        ]];
        let placed = seam(&k, &s, &loops);

        assert_eq!(placed.u.angle, Some(270.0));
        assert_eq!(position(&placed.surface).x_axis(), v3(0.0, -1.0, 0.0));
        assert!(placed.v.is_none());
        assert_same_points(&k, &s, &placed.surface, 270.0, &[0.0, 0.5, 1.0], 4.0);
    }

    #[test]
    fn revolution_of_an_arc_profile() {
        // The half circle of radius 1 about (3, 0, 0) in the XZ plane, from -90 to 90
        // degrees, about Z; the face along the circle of radius 3 at height -1 from 0
        // to 90 degrees, up the profile's copy at 90, back along the circle at height 1
        // and down the profile: the gap is 90..360, its middle 225.
        let k = k();
        let profile = circle(&k, p(3.0, 0.0, 0.0), Vec3::X, Vec3::Z, 1.0);
        let half = Curve::from(
            Ellipse::create_canonical_circle(
                &k,
                &Axis2Placement::new(p(3.0, 0.0, 0.0), Vec3::X, Vec3::Z),
                1.0,
                Some(&Extent1d::from_min_max(-90.0, 90.0)),
                None,
            )
            .unwrap(),
        );
        let s = Surface::from(
            SurfOfRevolution::create_canonical(&k, half, p(0.0, 0.0, 0.0), Vec3::Z, false).unwrap(),
        );
        let at90 = circle(&k, p(0.0, 3.0, 0.0), Vec3::Y, Vec3::Z, 1.0);
        let low = circle(&k, p(0.0, 0.0, -1.0), Vec3::X, Vec3::Y, 3.0);
        let high = circle(&k, p(0.0, 0.0, 1.0), Vec3::X, Vec3::Y, 3.0);
        let loops = vec![vec![
            edge(&low, 0.0, 90.0, true),
            edge(&at90, -90.0, 90.0, true),
            edge(&high, 0.0, 90.0, false),
            edge(&profile, -90.0, 90.0, false),
        ]];
        let placed = seam(&k, &s, &loops);

        let angle = placed.u.angle.unwrap();
        assert_angle(angle, 225.0);
        assert_unit(
            position(&placed.surface).x_axis(),
            v3(-FRAC_1_SQRT_2, -FRAC_1_SQRT_2, 0.0),
        );
        let vs = [-90.0, 0.0, 45.0, 90.0];
        assert_same_points(&k, &s, &placed.surface, angle, &vs, 4.0);
    }

    #[test]
    fn revolution_of_an_unbounded_line_measures_from_its_profile() {
        // An unbounded line profile through (2, 0, 0): the kernel places the
        // revolution from the line's far start, its X axis (-1, 0, 0), while its u = 0
        // is the profile at +X (u = 90 at +Y): the frame of its angles is turned half a
        // turn from its position.
        let k = k();
        let profile = Curve::from(Line::create_canonical(p(2.0, 0.0, 0.0), v3(1.0, 0.0, 2.0)));
        let s = Surface::from(
            SurfOfRevolution::create_canonical(&k, profile, p(0.0, 0.0, 0.0), Vec3::Z, false)
                .unwrap(),
        );
        assert_eq!(position(&s).x_axis(), v3(-1.0, 0.0, 0.0));
        let at90 = s.evaluate_step_point(&k, Point2::new(90.0, 0.0)).unwrap();
        assert!(at90.distance(p(0.0, 2.0, 0.0)) <= 1e-15);
        let c = closed(&k, &s, TOL).unwrap();
        assert_eq!(c.side, -1.0);
        assert_eq!(c.frame.x_axis(), v3(1.0, 0.0, 0.0));
        assert_eq!(c.frame.z_axis(), v3(0.0, 0.0, 1.0));
    }

    #[test]
    fn invalid_inputs_are_refused() {
        let k = k();
        let frame = Axis2Placement::IDENTITY;
        let s = cylinder(&k, &frame, 2.0);
        let face = HalfCylinder::new(&k, &frame);
        let loops = face.loops();
        let invalid = |r: Result<PlacedSeam>| {
            assert!(
                matches!(r, Err(SolidError::InvalidInput { operation: OP, .. })),
                "{r:?}"
            );
        };
        let run = |s: &Surface, loops: &[Vec<BoundaryEdge<'_>>], at: Option<Point3>, tol| {
            seam_into_widest_gap(&k, s, loops, &[], at, tol)
        };

        // a tolerance negative or not finite
        invalid(run(&s, &loops, None, -1.0));
        invalid(run(&s, &loops, None, f64::NAN));
        // a seam point not finite, or on the axis where the band needs it
        let bottom = circle(&k, p(0.0, 0.0, 0.0), Vec3::X, Vec3::Y, 2.0);
        let top = circle(&k, p(0.0, 0.0, 3.0), Vec3::X, Vec3::Y, 2.0);
        let band = vec![
            vec![edge(&bottom, 0.0, 360.0, true)],
            vec![edge(&top, 0.0, 360.0, false)],
        ];
        invalid(run(&s, &band, Some(p(f64::INFINITY, 0.0, 0.0)), TOL));
        invalid(run(&s, &band, Some(p(0.0, 0.0, 1.0)), TOL));
        // an edge interval not finite
        let open = vec![vec![edge(&bottom, 0.0, f64::INFINITY, true)]];
        invalid(run(&s, &open, None, TOL));
        // a surface that is not closed
        let plane = Surface::from(Plane::create_canonical(&frame).unwrap());
        invalid(run(&plane, &loops, None, TOL));
        // a cylinder over less than a full turn
        let mut part = Cone::create_canonical_cylinder(&k, &frame, 2.0).unwrap();
        part.adjust_step_uv_domain(&k, &Extent2d::from_bounds(0.0, 0.0, 180.0, 3.0))
            .unwrap();
        invalid(run(&Surface::from(part), &loops, None, TOL));
    }
}
