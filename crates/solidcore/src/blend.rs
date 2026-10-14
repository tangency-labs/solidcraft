//! Blends: constant-radius fillets, chamfers, and fillet removal on solid edges.
//!
//! Upstream: `SmFilletExecutive`, `SmFilletSolver`, `SmFilletStandardSolver`,
//! `SmFilletCustomSolver`, `SmFilletCorner`, `SmFilletGeom`, `SmFilletIntersector`,
//! `SmFilletRemoval`, `SmCurveFilletExecutive` in `source/SMLib`, and
//! `SM_API/SmApiFillets`.

pub mod fillet_custom_solver;
