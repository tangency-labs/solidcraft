//! # solidcore-convert
//!
//! The one conversion between the record's plain data ([`solidcore_record`]) and the kernel's
//! values ([`solidcore`]): vectors, frames, planes, curves, surfaces and pcurves, both ways. The
//! kernel never sees the record (docs/layers.md §3), so every layer that holds records and calls
//! the kernel (the modelling layer, the mesher, the applications' boundary until the modelling
//! layer holds it) converts here, and nowhere else.
//!
//! Every function copies fields from one form to the other, and none computes: no square root,
//! trigonometry, product or solve (clippy `disallowed-methods`, this crate's `clippy.toml`, as
//! the record's). Each record kind has its kernel kind with the same fields, so every conversion
//! is total and its round trip is the identity. The bare name converts the record to the
//! kernel, by reference (the caller keeps its record); `record_*` converts the kernel's value
//! back to the record, by value where it holds vectors that are moved rather than copied (the
//! caller has the value from the kernel), by reference for frames and planes.
//!
//! Arithmetic operators cannot be banned by clippy: review is the guard that none is used here.
//!
//! | Record | Kernel | To the kernel | Back |
//! |---|---|---|---|
//! | [`Vec3`] | [`solidcore::Vec3`] | [`vec3`] | [`record_vec3`], [`record_pair`], [`record_triple`], [`record_list`], [`record_table`] |
//! | [`Frame`] | [`k::Frame`] | [`frame`] | [`record_frame`] |
//! | [`Plane`] (origin and in-plane axes) | [`k::Frame`] | [`plane`] | [`record_plane`] |
//! | [`Curve`] | [`k::Curve`] | [`curve`] | [`record_curve`] |
//! | [`Surface`] | [`k::Surface`] | [`surface`] | [`record_surface`] |
//! | [`Curve2`] | [`k::Curve2`] | [`curve2`] | [`record_curve2`] |
#![forbid(unsafe_code)]
#![deny(clippy::disallowed_methods)]

use solidcore::eval as k;
use solidcore_record::brep::{Curve, Curve2, Frame, Surface};
use solidcore_record::{Plane, Vec3};

/// The record's vector as the kernel's.
#[must_use]
pub fn vec3(v: Vec3) -> solidcore::Vec3 {
    solidcore::Vec3::new(v.x, v.y, v.z)
}

/// A kernel vector as the record's.
#[must_use]
pub fn record_vec3(v: solidcore::Vec3) -> Vec3 {
    Vec3::new(v.x, v.y, v.z)
}

/// Two kernel vectors (a point and a derivative, or a point and a normal) as the record's.
#[must_use]
pub fn record_pair((a, b): (solidcore::Vec3, solidcore::Vec3)) -> (Vec3, Vec3) {
    (record_vec3(a), record_vec3(b))
}

/// Three kernel vectors (a point and two derivatives, or three second derivatives) as the
/// record's.
#[must_use]
pub fn record_triple(
    (a, b, c): (solidcore::Vec3, solidcore::Vec3, solidcore::Vec3),
) -> (Vec3, Vec3, Vec3) {
    (record_vec3(a), record_vec3(b), record_vec3(c))
}

/// Kernel vectors as the record's, in order.
#[must_use]
pub fn record_list(v: &[solidcore::Vec3]) -> Vec<Vec3> {
    v.iter().copied().map(record_vec3).collect()
}

/// A table of partial derivatives in the record's vectors: `d[i][j]` is `∂^(i+j) S / ∂u^i ∂v^j`.
pub type Table = [[Vec3; k::MAX_PARTIAL_ORDER + 1]; k::MAX_PARTIAL_ORDER + 1];

/// The kernel's table of partial derivatives ([`k::Partials`]) in the record's vectors.
#[must_use]
pub fn record_table(d: &k::Partials) -> Table {
    d.map(|row| row.map(record_vec3))
}

/// A record frame as the kernel's.
#[must_use]
pub fn frame(f: &Frame) -> k::Frame {
    k::Frame {
        origin: f.origin,
        x_axis: f.x_axis,
        y_axis: f.y_axis,
    }
}

/// A kernel frame as the record's.
#[must_use]
pub fn record_frame(f: &k::Frame) -> Frame {
    Frame {
        origin: f.origin,
        x_axis: f.x_axis,
        y_axis: f.y_axis,
    }
}

/// A sketch plane (its origin and in-plane axes) as the kernel's frame of the same three.
#[must_use]
pub fn plane(p: &Plane) -> k::Frame {
    k::Frame {
        origin: p.origin.into(),
        x_axis: p.x.into(),
        y_axis: p.y.into(),
    }
}

/// A kernel frame as the sketch plane of its origin and axes.
#[must_use]
pub fn record_plane(f: &k::Frame) -> Plane {
    Plane {
        origin: f.origin.into(),
        x: f.x_axis.into(),
        y: f.y_axis.into(),
    }
}

/// A record curve as the kernel's.
#[must_use]
pub fn curve(c: &Curve) -> k::Curve {
    match c {
        Curve::Line { start, end } => k::Curve::Line {
            start: *start,
            end: *end,
        },
        Curve::Circle { frame: f, radius } => k::Curve::Circle {
            frame: frame(f),
            radius: *radius,
        },
        Curve::Ellipse { frame: f, radii } => k::Curve::Ellipse {
            frame: frame(f),
            radii: *radii,
        },
        Curve::Polyline { points } => k::Curve::Polyline {
            points: points.clone(),
        },
        Curve::Spline { knots, points } => k::Curve::Spline {
            knots: knots.clone(),
            points: points.clone(),
        },
        Curve::RationalSpline { knots, homogeneous } => k::Curve::RationalSpline {
            knots: knots.clone(),
            homogeneous: homogeneous.clone(),
        },
        Curve::SurfaceIntersection {
            surfaces: [a, b],
            leader,
            tolerance,
        } => k::Curve::SurfaceIntersection {
            surfaces: [Box::new(surface(a)), Box::new(surface(b))],
            leader: Box::new(curve(leader)),
            tolerance: *tolerance,
        },
    }
}

/// A kernel curve as the record's.
#[must_use]
pub fn record_curve(c: k::Curve) -> Curve {
    match c {
        k::Curve::Line { start, end } => Curve::Line { start, end },
        k::Curve::Circle { frame: f, radius } => Curve::Circle {
            frame: record_frame(&f),
            radius,
        },
        k::Curve::Ellipse { frame: f, radii } => Curve::Ellipse {
            frame: record_frame(&f),
            radii,
        },
        k::Curve::Polyline { points } => Curve::Polyline { points },
        k::Curve::Spline { knots, points } => Curve::Spline { knots, points },
        k::Curve::RationalSpline { knots, homogeneous } => {
            Curve::RationalSpline { knots, homogeneous }
        }
        k::Curve::SurfaceIntersection {
            surfaces: [a, b],
            leader,
            tolerance,
        } => Curve::SurfaceIntersection {
            surfaces: [Box::new(record_surface(*a)), Box::new(record_surface(*b))],
            leader: Box::new(record_curve(*leader)),
            tolerance,
        },
    }
}

/// A record surface as the kernel's.
#[must_use]
pub fn surface(s: &Surface) -> k::Surface {
    match s {
        Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => k::Surface::Plane {
            origin: *origin,
            u_axis: *u_axis,
            v_axis: *v_axis,
        },
        Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => k::Surface::PlaneThrough {
            origin: *origin,
            u_point: *u_point,
            v_point: *v_point,
        },
        Surface::Cylinder { frame: f, radius } => k::Surface::Cylinder {
            frame: frame(f),
            radius: *radius,
        },
        Surface::Cone {
            frame: f,
            radius_at_origin,
            half_angle,
        } => k::Surface::Cone {
            frame: frame(f),
            radius_at_origin: *radius_at_origin,
            half_angle: *half_angle,
        },
        Surface::Sphere { frame: f, radius } => k::Surface::Sphere {
            frame: frame(f),
            radius: *radius,
        },
        Surface::Torus {
            frame: f,
            major_radius,
            minor_radius,
            tube_start,
        } => k::Surface::Torus {
            frame: frame(f),
            major_radius: *major_radius,
            minor_radius: *minor_radius,
            tube_start: *tube_start,
        },
        Surface::BallPatch { center, radius } => k::Surface::BallPatch {
            center: *center,
            radius: *radius,
        },
        Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => k::Surface::Spline {
            u_knots: u_knots.clone(),
            v_knots: v_knots.clone(),
            points: points.clone(),
        },
        Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => k::Surface::RationalSpline {
            u_knots: u_knots.clone(),
            v_knots: v_knots.clone(),
            homogeneous: homogeneous.clone(),
        },
        Surface::PlacedRevolution {
            profile,
            origin,
            axis,
            matrix,
            forward,
        } => k::Surface::PlacedRevolution {
            profile: Box::new(curve(profile)),
            origin: *origin,
            axis: *axis,
            matrix: *matrix,
            forward: *forward,
        },
    }
}

/// A kernel surface as the record's.
#[must_use]
pub fn record_surface(s: k::Surface) -> Surface {
    match s {
        k::Surface::Plane {
            origin,
            u_axis,
            v_axis,
        } => Surface::Plane {
            origin,
            u_axis,
            v_axis,
        },
        k::Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        } => Surface::PlaneThrough {
            origin,
            u_point,
            v_point,
        },
        k::Surface::Cylinder { frame: f, radius } => Surface::Cylinder {
            frame: record_frame(&f),
            radius,
        },
        k::Surface::Cone {
            frame: f,
            radius_at_origin,
            half_angle,
        } => Surface::Cone {
            frame: record_frame(&f),
            radius_at_origin,
            half_angle,
        },
        k::Surface::Sphere { frame: f, radius } => Surface::Sphere {
            frame: record_frame(&f),
            radius,
        },
        k::Surface::Torus {
            frame: f,
            major_radius,
            minor_radius,
            tube_start,
        } => Surface::Torus {
            frame: record_frame(&f),
            major_radius,
            minor_radius,
            tube_start,
        },
        k::Surface::BallPatch { center, radius } => Surface::BallPatch { center, radius },
        k::Surface::Spline {
            u_knots,
            v_knots,
            points,
        } => Surface::Spline {
            u_knots,
            v_knots,
            points,
        },
        k::Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        } => Surface::RationalSpline {
            u_knots,
            v_knots,
            homogeneous,
        },
        k::Surface::PlacedRevolution {
            profile,
            origin,
            axis,
            matrix,
            forward,
        } => Surface::PlacedRevolution {
            profile: Box::new(record_curve(*profile)),
            origin,
            axis,
            matrix,
            forward,
        },
    }
}

/// A record pcurve as the kernel's.
#[must_use]
pub fn curve2(c: &Curve2) -> k::Curve2 {
    match c {
        Curve2::Spline { knots, points } => k::Curve2::Spline {
            knots: knots.clone(),
            points: points.clone(),
        },
        Curve2::RationalSpline { knots, homogeneous } => k::Curve2::RationalSpline {
            knots: knots.clone(),
            homogeneous: homogeneous.clone(),
        },
    }
}

/// A kernel pcurve as the record's.
#[must_use]
pub fn record_curve2(c: k::Curve2) -> Curve2 {
    match c {
        k::Curve2::Spline { knots, points } => Curve2::Spline { knots, points },
        k::Curve2::RationalSpline { knots, homogeneous } => {
            Curve2::RationalSpline { knots, homogeneous }
        }
    }
}
