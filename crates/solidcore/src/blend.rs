//! Blends: constant-radius fillets, chamfers, and fillet removal on solid edges.
//!
//! Upstream: `SmFilletExecutive`, `SmFilletSolver`, `SmFilletStandardSolver`,
//! `SmFilletCustomSolver`, `SmFilletCorner`, `SmFilletGeom`, `SmFilletIntersector`,
//! `SmFilletRemoval`, `SmCurveFilletExecutive` in `source/SMLib`, and
//! `SM_API/SmApiFillets`.

pub mod fillet_custom_solver;
pub mod fillet_geom;
pub mod fillet_intersector;
pub mod fillet_standard_solver;
pub mod hcr;
