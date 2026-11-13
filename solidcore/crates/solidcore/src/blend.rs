//! Blends: constant-radius fillets, chamfers, fillet removal on solid edges, and
//! defeaturing.
//!
//! Upstream: `SmFilletExecutive`, `SmFilletSolver`, `SmFilletStandardSolver`,
//! `SmFilletCustomSolver`, `SmFilletCorner`, `SmFilletGeom`, `SmFilletIntersector`,
//! `SmFilletRemoval`, `SmCurveFilletExecutive`, `SmFeatureExecutive` in `source/SMLib`, and
//! `SM_API/SmApiFillets`.

pub mod band_room;
pub mod blend_corner;
pub mod blend_refusal;
pub mod blend_setback;
pub mod carried_face;
pub mod chamfer_side;
pub mod definition_check;
pub mod feature_executive;
pub mod fillet_classification;
pub mod fillet_cliff_overflow;
pub mod fillet_corner;
pub mod fillet_custom_solver;
pub mod fillet_exact;
pub mod fillet_executive;
pub mod fillet_geom;
pub mod fillet_intersector;
pub mod fillet_outline;
pub mod fillet_overflow;
pub mod fillet_removal;
pub mod fillet_section;
pub mod fillet_solver;
pub mod fillet_standard_solver;
pub mod fillet_tangent_rollover;
pub mod hcr;
pub mod identify_fillet;
pub mod law_corners;
pub mod round_room;
