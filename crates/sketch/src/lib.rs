//! Sketches: points and curves on a plane, geometric constraints and driving dimensions, a
//! Levenberg–Marquardt constraint solver with degree-of-freedom analysis, and closed-profile
//! (region) detection for features.
//!
//! The solver is our own clean-room implementation (residual functions per constraint, local
//! finite-difference Jacobians, damped Gauss–Newton per connected component, rank analysis by
//! Gaussian elimination). See `plan/architecture.md` §sketch.
//!
//! Before iterating, a constructive seed places every point that two loci on already placed
//! points pin down (circles, rows and columns, lines through placed points, offsets from
//! placed lines, tangency, midpoints, mirrored points, coincidence), on the branch that keeps
//! the orientation the point had before the solve, so dimension edits do not fold the sketch
//! into a mirrored solution or flip an arc out of its corner. The seed only moves the starting
//! point: the iteration still verifies every constraint and the rank analysis still decides
//! the degrees of freedom.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod construct;
mod curves;
mod decompose;
mod dims;
mod ends;
mod equation;
mod fonts;
mod freeform_offset;
pub mod limits;
pub mod linalg;
mod link;
mod loci;
mod model;
mod nurbs;
mod offset;
mod offset_topology;
mod paper_text;
mod pattern;
mod periodic;
pub mod pierce;
mod profiles;
mod profiles3d;
#[cfg(feature = "solver-profiling")]
pub mod profiling;
mod shape;
mod solver;
mod spline_points;
mod text;
mod thin;
mod union_find;
mod wire;
mod wire_solve;

pub use curves::MAX_SPLINE_POINTS;
pub use curves::ToleranceFit;
pub use curves::bezier_distance;
pub use curves::bspline_beziers;
pub use curves::conic_point;
pub use curves::conic_rho_from_point;
pub use curves::ellipse_point;
pub use curves::end_curvature;
pub use curves::fit_beziers;
pub use curves::fit_beziers_mode;
pub use curves::fit_beziers_with_handles;
pub use curves::fit_tangent_handles;
pub use curves::fit_within_tolerance;
pub use curves::spline_end_tangent;
pub use curves::spline_end_tangent_mode;
pub use curves::spline_end_tangent_with_handles;
pub use curves::spline_point;
pub use curves::spline_point_mode;
pub use curves::spline_point_with_handles;
pub use curves::spline_polyline;
pub use curves::spline_polyline_mode;
pub use curves::spline_polyline_with_handles;
pub use decompose::Budget;
pub use dims::DimFrame;
pub use dims::DimLayout;
pub use dims::DimSizes;
pub use dims::DimText;
pub use dims::LinearAxis;
pub use dims::chain_centres;
pub use dims::default_text;
pub use dims::default_text_offset;
pub use dims::dim_frame;
pub use dims::dim_frame_at;
pub use dims::dim_layout;
pub use dims::dim_layout_reaching;
pub use dims::dim_layout_sized;
pub use dims::dim_reach;
pub use dims::encode_text;
pub use dims::in_sector;
pub use dims::linear_axis;
pub use ends::END_TOL;
pub use ends::EndReport;
pub use ends::EndTouch;
pub use ends::UntiedEnd;
pub use ends::loose_ends;
pub use equation::EquationCurve;
pub use equation::MAX_EQUATION_TEXT;
pub use equation::VariableUnit;
pub use fonts::DEFAULT_FONT_FAMILY;
pub use fonts::FontData;
pub use fonts::check_family;
pub use fonts::fallback;
pub use fonts::families_in;
pub use fonts::find_style_in;
pub use fonts::font_data;
pub use fonts::font_data_saved;
pub use fonts::font_data_styled;
pub use fonts::font_dirs;
pub use fonts::font_file_path;
pub use fonts::font_override_dirs;
pub use fonts::installed_families;
pub use fonts::set_font_file_check;
pub use freeform_offset::FreeformOffset;
pub use freeform_offset::FreeformOffsetOutput;
pub use freeform_offset::OFFSET_TOLERANCE;
pub use link::Link;
pub use link::LinkGeom;
pub use link::LinkKind;
pub use link::LinkSource;
pub use link::MAX_LINK_ENTITIES;
pub use link::SKETCH_AXIS_HALF;
pub use link::SketchPlacement;
pub use link::TextExprs;
pub use link::sketch_axis_geom;
pub use model::CEILING_CONSTRAINTS;
pub use model::CEILING_CURVES;
pub use model::CEILING_POINTS;
pub use model::Constraint;
pub use model::ConstraintKind;
pub use model::Curve;
pub use model::CurveKind;
pub use model::MAX_CONSTRAINTS;
pub use model::MAX_COORD;
pub use model::MAX_CURVES;
pub use model::MAX_POINTS;
pub use model::PointExprs;
pub use model::SPoint;
pub use model::Sketch;
pub use model::SketchBatch;
pub use model::SketchError;
pub use model::SketchView;
pub use model::TwoCircles;
pub use nurbs::Nurbs;
pub use offset::OffsetChain;
pub use offset_topology::OffsetCreated;
pub use offset_topology::OffsetRole;
pub use offset_topology::OffsetRoleFlags;
pub use offset_topology::OffsetRoleKind;
pub use offset_topology::OffsetTopology;
pub use paper_text::FlowedPaperText;
pub use paper_text::Outcome;
pub use paper_text::PaperFonts;
pub use paper_text::PaperLines;
pub use paper_text::PaperSourceFlow;
pub use paper_text::PaperTextFlow;
pub use paper_text::paper_fallback_notes;
pub use paper_text::paper_font_note;
pub use paper_text::paper_lines;
pub use paper_text::paper_lines_in;
pub use paper_text::paper_lines_now;
pub use paper_text::paper_text;
pub use paper_text::paper_text_in;
pub use paper_text::paper_text_now;
pub use paper_text::paper_text_with_flow;
pub use paper_text::paper_text_with_flow_in;
pub use paper_text::paper_text_with_flow_now;
pub use pattern::PatternAlignment;
pub use pattern::PatternInstance;
pub use pattern::SketchPattern;
pub use pattern::SketchPatternKind;
pub use periodic::periodic_beziers;
pub use periodic::validate_closed_control;
pub use profiles::CurvePick;
pub use profiles::ENDPOINT_JOIN_TOL;
pub use profiles::MAX_PROFILE_EDGES;
pub use profiles::Profile;
pub use profiles::ProfileSearch;
pub use profiles::distinct_curves;
pub use profiles::enclosed_gaps;
pub use profiles::find_drawn_profiles;
pub use profiles::find_profiles;
pub use profiles::find_profiles_checked;
pub use profiles::merge_regions;
pub use profiles::profile_at_point;
pub use profiles::profile_by_curves;
pub use profiles::without_text_counters;
pub use profiles3d::Profile3D;
pub use profiles3d::find_planar_3d_profiles;
pub use shape::Shape;
pub use shape::intersections;
pub use shape::intersections_within;
pub use shape::nearest_crossing;
pub use shape::nearest_curve_intersection;
pub use solver::Analysis;
pub use solver::CONSISTENT;
pub use solver::DrivingValues;
pub use solver::Hold;
pub use solver::MAX_COMPONENT_VARS;
pub use solver::SolveReport;
pub use solver::SolveStatus;
pub use solver::constraint_residual;
pub use solver::determined_by;
pub use solver::fillets_kept;
pub use solver::flips_from_stored;
pub use solver::keep_point_order;
pub use solver::keep_sides_at_zero;
pub use solver::measure_dimension;
pub use solver::move_line_towards;
pub use solver::move_point_towards;
pub use solver::point_along;
pub use solver::side_flips_from_stored;
pub use solver::solve;
pub use solver::solve_after_edit;
pub use solver::solve_after_edit_with_analysis;
pub use solver::solve_after_link_move;
pub use solver::solve_after_value_edit;
pub use solver::solve_holding;
pub use solver::solve_with;
pub use text::MAX_TEXT_CHARS;
pub use text::TextAlign;
pub use text::TextMetrics;
pub use text::TextStyle;
pub use text::TextVerticalAlign;
pub use text::saved_text_geometry;
pub use text::text_geometry;
pub use text::text_geometry_in;
pub use text::text_geometry_styled;
pub use thin::MAX_THIN_SEGMENTS;
pub use thin::thin_wall_regions;
pub use wire::MAX_FIT_POINTS;
pub use wire::MAX_WIRE_POINTS;
pub use wire::MIN_CONTROL_POINTS;
pub use wire::Wire;
pub use wire::WireArc;
pub use wire::WireControl;
pub use wire::WireCurve;
pub use wire::WireSpace;
pub use wire::arc_from_3_points;
pub use wire::arc_polyline;
pub use wire::arc_sweep;
pub use wire::control_curve;
pub use wire::control_polyline;
pub use wire::fit_polyline;
pub use wire_solve::Axis;
pub use wire_solve::MAX_WIRE_LENGTH;
pub use wire_solve::MAX_WIRE_ROWS;
pub use wire_solve::MAX_WIRE_VARS;
pub use wire_solve::OriginPlane;
pub use wire_solve::PointExprs3;
pub use wire_solve::WireConstraint;
pub use wire_solve::WireConstraintKind;
pub use wire_solve::WirePoint;
pub use wire_solve::WireReport;

#[cfg(test)]
mod circle_edge_tests;
#[cfg(test)]
mod coincident_tangent_tests;
#[cfg(test)]
mod construct_rounds_tests;
#[cfg(test)]
mod construct_tests;
#[cfg(test)]
mod continuation_tests;
#[cfg(test)]
mod degenerate_line_tests;
#[cfg(test)]
mod ends_tests;
#[cfg(test)]
mod font_tests;
#[cfg(test)]
mod least_motion_tests;
#[cfg(test)]
mod line_circle_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod thin_tests;

#[cfg(test)]
mod virtual_corner_tests;
