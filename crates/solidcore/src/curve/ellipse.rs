//! Ellipses and circles (`SmEllipse`): the analytic curve `O + rx cos(s) X + ry sin(s)
//! Y` over an angular (STEP) domain in degrees, kept together with its rational
//! quadratic B-spline and the polar conversion table between the two
//! parameterisations (`SmPolarConversion`).
//!
//! Upstream: `source/SMLib/src/SmEllipse.cpp`, `source/SMLib/inc/SmEllipse.h`.
//! The file is split over several plan units (`curve.ellipse_p1`, `_p2`); each lives
//! in its own submodule and adds methods to [`Ellipse`] and [`PolarConversion`].

pub mod p2;

pub use p2::{
    Ellipse, EllipseCanonical, EllipseIntersection, EllipseOptions, EllipseStartPlanePoint,
    PolarConversion,
};
