//! NURBS basis functions, knot vectors, and B-spline/NURBS curve and surface
//! evaluation, refinement, fitting and approximation.
//!
//! Upstream: `source/SMLib/{inc,src}/NL_*` (the "NLib" layer: `NL_BasisBasic`,
//! `NL_BasisAdv`, `NL_CrvBasic`, `NL_SrfBasic`, `NL_CrvFit`, ...) and the
//! `SmNurbs*` / `SmBSpline*` wrappers. Units are listed in `plan/modules.toml`.

pub mod basis_adv;
pub mod basis_basic;
pub mod bezier;
pub mod crv_basic;
pub mod funcs_basic;
pub mod geom;
pub mod iges;
pub mod knots;
pub mod math;
pub mod srf_basic;
