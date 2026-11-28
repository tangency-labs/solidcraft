//! Where the lines along a plane's normal meet a surface carried on without end: the kernel's
//! [`solidcore::geom::extended_levels::extended_levels`] on records' numbers. Ops converts.

use solidcore::geom::extended_levels::{self as k, Carried as KCarried};
use solidcore::geom::vector3d::Vec3;

/// A surface carried on without end ([`k::Carried`]).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Carried {
    /// A plane: a point on it and its unit normal.
    Plane {
        /// A point on the plane.
        point: [f64; 3],
        /// The plane's normal (unit).
        normal: [f64; 3],
    },
    /// A cylinder: a point on its axis, the unit axis and the radius.
    Cylinder {
        /// A point on the axis.
        origin: [f64; 3],
        /// The axis direction (unit).
        axis: [f64; 3],
        /// The radius.
        radius: f64,
    },
    /// A cone: its apex, the unit axis and the half angle (radians).
    Cone {
        /// The apex.
        apex: [f64; 3],
        /// The axis direction (unit).
        axis: [f64; 3],
        /// The angle between the axis and each line of the cone (radians).
        half_angle: f64,
    },
    /// A sphere.
    Sphere {
        /// The centre.
        center: [f64; 3],
        /// The radius.
        radius: f64,
    },
    /// A torus.
    Torus {
        /// The centre.
        center: [f64; 3],
        /// The distance from the centre to the middle of the tube.
        major: f64,
        /// The tube's radius.
        minor: f64,
    },
}

fn vec(a: [f64; 3]) -> Vec3 {
    Vec3 {
        x: a[0],
        y: a[1],
        z: a[2],
    }
}

/// The levels along the unit normal of the plane through `plane_origin` spanned by `plane_x`
/// and `plane_y` where the line through each of `points` along it meets `surface` carried on
/// without end (the kernel's [`k::extended_levels`]). `None` for axes that span no plane.
#[must_use]
pub fn extended_levels(
    surface: &Carried,
    plane_origin: [f64; 3],
    plane_x: [f64; 3],
    plane_y: [f64; 3],
    points: &[[f64; 3]],
) -> Option<Vec<f64>> {
    let surface = match *surface {
        Carried::Plane { point, normal } => KCarried::Plane {
            point: vec(point),
            normal: vec(normal),
        },
        Carried::Cylinder {
            origin,
            axis,
            radius,
        } => KCarried::Cylinder {
            origin: vec(origin),
            axis: vec(axis),
            radius,
        },
        Carried::Cone {
            apex,
            axis,
            half_angle,
        } => KCarried::Cone {
            apex: vec(apex),
            axis: vec(axis),
            half_angle,
        },
        Carried::Sphere { center, radius } => KCarried::Sphere {
            center: vec(center),
            radius,
        },
        Carried::Torus {
            center,
            major,
            minor,
        } => KCarried::Torus {
            center: vec(center),
            major,
            minor,
        },
    };
    let points: Vec<Vec3> = points.iter().map(|p| vec(*p)).collect();
    k::extended_levels(
        &surface,
        vec(plane_origin),
        vec(plane_x),
        vec(plane_y),
        &points,
    )
}
