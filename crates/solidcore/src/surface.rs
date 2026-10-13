//! Surface geometry: plane, cylinder, cone, sphere, torus, B-spline, extrusion,
//! revolution, offset and blend surfaces, as a concrete `enum` with evaluation,
//! normals, derivatives and closest point.
//!
//! Upstream: `SmSurface` and its subclasses (`SmPlane`, `SmCylinder`, `SmCone`,
//! `SmSphere`, `SmTorus`, `SmBSplineSurface`, `SmSurfOfExtrusion`,
//! `SmSurfOfRevolution`, `SmOffsetSurface`, `SmBlendSurface`, ...) in `source/SMLib`,
//! and `SM_API/SmApiSurfaces`.

pub mod twist_volume;
pub mod volume;
