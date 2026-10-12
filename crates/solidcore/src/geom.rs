//! Core geometric value types and tolerance helpers: 2D/3D vectors, extents
//! (bounding boxes and intervals), pseudo boxes, transforms and the `SmTol`
//! tolerance policy.
//!
//! Upstream: `source/SMLib/{inc,src}/SmVector2d`, `SmVector3d`, `SmExtent*`,
//! `SmPseudoBox`, `SmTol`, `SmTransform`, `SmMatrix` and related `geom.*` units of
//! `plan/modules.toml`.

pub mod extent3d;
pub mod periodic_extent1d;
pub mod tol;
pub mod tree;
pub mod vector2d;
