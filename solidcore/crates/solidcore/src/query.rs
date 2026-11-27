//! Read-only queries on B-reps and geometry: closest point, point
//! classification, bounding boxes, adjacency lookups, manifold checks.
//!
//! Upstream: `SmPointClass`, `SmRayTracer`, `SmExtent*`, `SmPseudoBox`,
//! `SmTopologyTraverser` in `source/SMLib`, and `SM_API/SmApiQueries`.

pub mod api_queries;
pub mod brep;
pub mod contact;
pub mod continuity;
pub mod curve;
pub mod curve_surface_gap;
pub mod deviation;
pub mod distance_bound;
pub mod edge;
pub mod edge_turn;
#[cfg(test)]
pub(crate) mod exact;
pub mod face;
mod face_surface;
pub mod faces_continue;
pub mod flush_contact;
mod line_meeting;
pub mod r#loop;
pub mod normal;
pub mod path_plane;
mod planar_disjoint;
pub mod point_identity;
pub mod poly;
pub mod poly_properties;
pub mod same_surface;
pub mod seam_bound;
pub mod section_corners;
pub mod tangent_edge;
pub mod tess;
pub mod tube_cope;
pub mod vertex_meet;

pub use face_surface::{
    face_line_hits, face_surface_feet, face_surface_intersect, FaceLineHit, SurfaceFoot,
};
pub use line_meeting::{
    distance_across_surface, line_unit_direction, nearer_line_meeting, nearest_line_hit,
    point_along_line,
};
