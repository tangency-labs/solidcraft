//! Intersections: curve/curve, curve/surface, surface/surface (marching and
//! tracing), face/face and B-rep/B-rep intersection graphs.
//!
//! Upstream: `SmSurfaceIntersector`, `SmSurfaceTracer`, `SmTopologyIntersector`,
//! `SmPolyIntersector`, `SmSSIData` in `source/SMLib`, and
//! `SM_API/SmApiIntersectors`.

pub mod point_class;
pub mod surface_intersector;
pub mod surface_silhouette;
pub mod surface_tracer;
