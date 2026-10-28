//! The B-rep kernel boundary.
//!
//! Everything SolidCraft does with solids goes through this crate: building bodies from planar
//! regions (extrude, revolve) and primitives, booleans, fillets and chamfers, tessellation,
//! measurement and STEP export and import. The implementation uses the `truck` crates (Apache-2.0; see
//! `plan/adr/0001-geometry-kernel.md`) plus our own local operations (edge fillets and
//! chamfers in `blend.rs`), but no truck type appears in the public API, so the kernel can be
//! replaced without touching the rest of the workspace.
//!
//! Every call into the third-party kernel runs under [`guard`]: a panic inside it becomes a
//! [`KernelError::Internal`] instead of taking the application down.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

mod assemble;
mod blend;
mod blend_corner;
mod blend_smlib;
mod blend_variable;
#[cfg(test)]
mod blend_wrong_way_tests;
mod body;
mod boolean_record;
mod boundary;
mod boundary_network;
mod bridge;
mod build;
#[cfg(test)]
mod chamfer_sides_curved_tests;
mod continuity;
mod continuity_order;
#[cfg(test)]
mod coplanar_tests;
mod corner_g2;
mod delete_face;
mod distance;
mod draft_corners;
mod emboss;
#[cfg(test)]
mod face_source_tests;
mod fair;
mod fill;
#[cfg(test)]
mod fillet_setback_tests;
mod freeform;
pub mod fuzz;
mod heal;
mod helix;
pub mod history;
mod iges_in;
mod iges_out;
mod intersection;
#[cfg(test)]
mod intersection_curve_tests;
mod iso;
#[cfg(test)]
mod issue_1038_tests;
#[cfg(test)]
mod issue_1080_tests;
#[cfg(test)]
mod issue_1510_tests;
#[cfg(test)]
mod issue_2005_tests;
#[cfg(test)]
mod issue_324_tests;
#[cfg(test)]
mod issue_616_tests;
#[cfg(test)]
mod issue_674_tests;
#[cfg(test)]
mod issue_679_tests;
#[cfg(test)]
mod issue_788_tests;
#[cfg(test)]
mod issue_851_tests;
#[cfg(test)]
mod issue_871_tests;
#[cfg(test)]
mod join_between_caps_tests;
mod lip;
pub mod loft_blend;
mod loft_rails;
#[cfg(test)]
mod loft_rails_tests;
#[cfg(test)]
mod loft_region_tests;
mod matchsurf;
mod measure;
mod mesh_convert;
mod meshbody;
#[cfg(test)]
mod modify_history_tests;
mod native;
mod numbered;
mod offset;
mod offset_backend;
mod ops;
#[cfg(test)]
mod outline_tests;
mod par;
pub use par::{for_each_on_threads, map_on_threads, map_with_local_pools};
#[cfg(test)]
mod pinch_tests;
#[cfg(test)]
mod planar_faces_tests;
mod plane_fit;
mod point_path;
mod polybool;
mod polyhedron;
mod prism;
mod prism_curve;
mod prism_measure;
pub mod profile;
pub mod provenance;
mod record;
#[cfg(test)]
mod seam_faces_tests;
mod sew;
mod smlib_boolean;
mod smlib_sweeps;
#[cfg(test)]
mod smlib_sweeps_tests;
mod smooth_edges;
#[cfg(test)]
mod sphere_lens_tests;
mod splitface;
mod step;
mod step_in;
mod step_out;
mod surface_extend;
mod surface_kind;
mod surface_offset;
mod surface_ruled;
#[cfg(test)]
mod surface_second_derivative_tests;
mod surface_trim;
mod surface_untrim;
mod surfaces;
mod sweep_path;
#[cfg(test)]
mod sweep_rails_tests;
mod tess;
#[cfg(test)]
mod thin_face_tests;
#[cfg(test)]
mod thread_display_tests;
#[cfg(test)]
mod thread_tests;
mod topo;
mod tspline;
#[cfg(test)]
mod untouched_faces_tests;
mod validity;
pub mod watch;

pub use blend::{
    ChamferSide, chamfer, chamfer_sides, fillet, fillet_each, fillet_g2, fillet_g2_corners, fillet_least_setback, fillet_max, fillet_max_each,
    fillet_setback, tangent_chain,
};
pub use blend_corner::fillet_radii;
pub use blend_variable::{RadiusSpline, Taper, fillet_chord, fillet_tapered, fillet_variable, fillet_variable_points, fillet_variable_spline, taper};
pub use body::{Body, CapSignature, DISPLAY_ANGLE, DisplayShare, EdgeInfo, FaceInfo, FacePaint, FaceSource, MeshTolerance, Paint};
pub use boundary::{Boundary, boundary_patch};
pub use boundary_network::{BoundaryCurve, CurveSource, MAX_CHAIN_CURVES, MAX_EDGE_PICKS, boundary_join, boundary_surface};
pub use bridge::{Frame, blend_between, bridge, chain, chain_closes, frames, frames_across, sample_grid};
pub use build::{
    PathSeg, box_solid, canonical_revolve_axis, cylinder, extrude, extrude_tapered, extrude_tapered_between, extrude_whole_sides, loft, loft_regions,
    loft_with_rails, path_polyline, planar_face_record, revolve, revolve_whole, sphere, sweep, sweep_with, torus,
};
pub use continuity::{
    ContinuityReport, ContinuityTolerances, EdgePick, MAX_SAMPLES, MIN_SAMPLES, SeamReport, SeamStation, SeamVerdict, SeamWorst, pick_edges,
    seam_diagnostics,
};
pub use continuity_order::{Construction, Continuity};
pub use corner_g2::{CURVATURE_SHARE, DEFAULT_SETBACK_FACTOR};
pub use delete_face::delete_faces;
pub use distance::{BodyDistance, DistanceShape, body_distance};
pub use draft_corners::{CornerFill, CornerOption, CornerRefusal, CornerReport, DraftCorner, draft_corners};
pub use emboss::{cylinder_emboss_anchor, emboss_cylinder, emboss_cylinder_at, has_cylinder_emboss_wall, infer_cylinder_emboss};
pub use fair::{
    FairCurve, FairNet, FairOptions, FairReport, MAX_FAIR_ITERATIONS, MAX_FAIR_POINTS, fair_bspline_curve, fair_bspline_surface, fair_curve,
    fair_faces, fair_net, fair_nurbs_curve, fair_nurbs_surface, fair_surface,
};
pub use fill::{PATCH_ANGLE_TOL_DEG, PatchReport, PatchSpec, patch_edges_crowned, patch_edges_report, patch_edges_with};
pub use helix::{loft_to_point, sweep_helix, sweep_helix_mesh, sweep_helix_radial};
pub use iges_in::{MAX_IGES_BYTES, iges_import, iges_to_step};
pub use iges_out::iges_export_bodies;
pub use intersection::{MeetingEdge, meeting_edges};
pub use iso::{IsoLine, MAX_ISO_COUNT, iso_curves, iso_grid};
pub use lip::{raise_band, rim_band};
pub use loft_blend::loft_blend;
pub use matchsurf::{match_surface, seam_continuity};
pub use measure::{BodyMeasure, MassMeasure, measure, measure_mass};
pub use mesh_convert::convert_prismatic_mesh;
pub use meshbody::{MAX_TRIANGLES, mesh_body};
pub use native::{body_from_smlib, smlib_body};
pub use native::{brep_record, decode_native_body, encode_native_body, native_preview_mesh};
pub use offset::with_same_surface;
pub use ops::{BoolOp, boolean, half_space, join_apart, place_matrix, split_by_plane, transform, transform_matrix, union_all, union_apart};
pub use point_path::{
    PathPiece, PointPath, SpaceBezier, curve_path, edge_path, face_section_along, project_onto_face, sketch_record_curve, sweep_point_path_with,
};
pub use polybool::faceted_boolean;
pub use polyhedron::{HalfSpace, convex_polyhedron, draft, offset_faces, shell};
pub use prism::{PLANAR_FIT_TOL, extrude_planar_regions, planar_regions_boolean};
pub use prism_measure::certified_prism_equivalent;
pub use sew::{EdgeSpec, FaceSpec, SurfSpec, sew, sew_touching};
pub use splitface::{SplitTool, split_body, split_faces, split_faces_with_map};
pub use step::step_export;
pub use step_in::{
    ImportedBody, StepImport, StepNode, StepReading, step_cache_alive, step_import, step_import_shared, step_orientation_errors, step_validate,
};
pub use step_out::{ExportBody, ExportProduct, StepHeader, step_export_bodies, step_export_products};
pub use surface_extend::{ExtendLimit, ExtendMode, ExtendOptions, extend_surface};
pub use surface_kind::{EdgeCircle, FaceSurface, edge_circle, face_surface};
pub use surface_offset::offset_surface;
pub use surface_ruled::{Ruled, RuledCorners, RuledMode, ruled_far_handles, ruled_surface, ruled_surface_with};
pub use surface_trim::{TrimKeep, mutual_trim, trim, trim_keep};
pub use surface_untrim::{UntrimHoles, untrim};
pub use surfaces::{
    copy_faces, extend, extrude_sheet, loft_sheet, loft_sheet_with_rails, patch_edges, patch_region, remove_faces, reverse_faces, revolve_sheet,
    stitch, sweep_sheet, sweep_sheet_points, sweep_sheet_points_with, sweep_sheet_with, thicken, unstitch,
};
pub use sweep_path::{RailFit, SweepControl, SweepOrientation, sweep_path, sweep_path_with};
pub use topo::{CylinderFace, cylinder_end_at, cylinder_face, cylinder_face_at, material_in_ring};
pub use topo::{TopoCounts, merged_topology, seam_flags};
pub use tspline::tspline_sheet;
pub use validity::{SmlibCheck, Validity, smlib_built, smlib_dump_dir, validity};

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum KernelError {
    #[error("invalid input: {0}")]
    Invalid(String),
    #[error("the operation failed: {0}")]
    Failed(String),
    /// A blend or other size too large for the geometry around it (a smaller size may work);
    /// shown like any feature that isn't supported yet.
    #[error("the operation failed: not supported yet: {0}")]
    TooLarge(String),
    /// The third-party kernel panicked; the model is unchanged.
    #[error("kernel internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;

/// The native kernel call running right now (SMLib), for crash reports.
pub fn native_call_in_flight() -> Option<String> {
    solidcraft_smlib::current_call()
}

thread_local! {
    static GUARDED: std::cell::Cell<u32> = const { std::cell::Cell::new(0) };
}

/// Is this thread inside [`guard`]? (A panic now will be caught and turned into an error; the
/// engine's crash reports mark it recovered.)
pub fn guarded() -> bool {
    GUARDED.with(std::cell::Cell::get) > 0
}

/// Run third-party kernel code, converting a panic into an error.
pub fn guard<T>(what: &str, f: impl FnOnce() -> Result<T>) -> Result<T> {
    struct Depth;
    impl Drop for Depth {
        fn drop(&mut self) {
            GUARDED.with(|g| g.set(g.get().saturating_sub(1)));
        }
    }
    GUARDED.with(|g| g.set(g.get() + 1));
    let depth = Depth;
    let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(f));
    drop(depth);
    match r {
        Ok(r) => r,
        Err(p) => {
            let msg = p.downcast_ref::<&str>().map(|s| s.to_string()).or_else(|| p.downcast_ref::<String>().cloned()).unwrap_or_default();
            log::warn!("kernel panic in {what}: {msg}");
            Err(KernelError::Internal(format!("{what}: {msg}")))
        }
    }
}

#[cfg(test)]
mod tests;

#[cfg(test)]
mod trimmed_face_mesh_tests;
