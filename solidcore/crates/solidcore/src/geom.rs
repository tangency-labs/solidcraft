//! Core geometric value types and tolerance helpers: 2D/3D vectors, extents
//! (bounding boxes and intervals), pseudo boxes, transforms and the `SmTol`
//! tolerance policy.
//!
//! Upstream: `source/SMLib/{inc,src}/SmVector2d`, `SmVector3d`, `SmExtent*`,
//! `SmPseudoBox`, `SmTol`, `SmTransform`, `SmMatrix` and related `geom.*` units of
//! `plan/modules.toml`.

pub mod a_object;
pub mod affine_fit;
pub mod assert_array;
pub mod attribute;
pub mod axial_section;
pub mod axis2_placement;
pub mod certified;
pub mod content;
pub mod context;
pub(crate) mod expansion;
pub mod extent1d;
pub mod extent2d;
pub mod extent3d;
pub mod facing_trough;
pub mod form_cage;
pub mod form_curves;
pub mod form_fill;
pub mod form_match;
pub mod form_pipe;
pub mod form_subdivision;
pub mod form_surface_replace;
pub mod geom_utility;
pub mod global_solver;
pub mod grid;
pub mod hash_table;
pub(crate) mod interval_vector;
pub mod krawczyk;
pub mod local_solve1d;
pub mod local_solve_nd;
pub mod matrix;
pub mod mesh_edit;
pub mod object;
pub mod ordered_solve;
pub mod paper_gaps;
pub mod parameter_line;
pub mod periodic_extent1d;
pub mod plane_height;
pub mod point_set;
pub mod polar_box;
pub mod polynomial;
pub mod pseudo_box;
pub mod regularized_fit;
pub mod rim_coincidence;
pub mod solution_array;
pub mod spline_interpolation;
pub mod spline_space;
pub mod step_matrix;
pub mod tol;
pub mod tools;
pub mod transform;
pub mod tree;
pub mod vector2d;
pub mod vector3d;
