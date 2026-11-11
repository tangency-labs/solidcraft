//! The B-rep kernel boundary.
//!
//! Everything SolidCraft does with solids goes through this crate: building bodies from planar
//! regions (extrude, revolve) and primitives, booleans, fillets and chamfers, tessellation,
//! measurement and STEP export and import. The implementation uses the vendored `solidcore`
//! kernel (Apache-2.0, `solidcore`) plus our own local operations (edge fillets and
//! chamfers in `blend.rs`), but no solidcore type appears in the public API, so the kernel can
//! be replaced without touching the rest of the workspace.
//!
//! Every call into the vendored kernel runs under [`guard`]: a panic inside it becomes a
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
mod curvature_comb;
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
mod issue_12126_tests;
#[cfg(test)]
mod issue_1510_tests;
#[cfg(test)]
mod issue_2005_tests;
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
pub use par::{KERNEL_STACK, for_each_on_threads, kernel_thread, map_on_threads, map_with_local_pools};
#[cfg(test)]
mod display_plan_tests;
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
mod repair;
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
mod tangent_edge;
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
mod validation;
mod validity;
pub mod watch;

pub use blend::{
    ChamferSide, FilletTurn, FitSearch, SETBACK_AT_LEAST, blend_edges, chamfer, chamfer_carrying_face, chamfer_max, chamfer_max_why, chamfer_sides,
    chamfer_sides_carrying_face, fillet, fillet_each, fillet_g2, fillet_g2_corners, fillet_g2_with_g2_corners, fillet_least_setback, fillet_max,
    fillet_max_each, fillet_max_why, fillet_removing, fillet_setback, largest_fit_note, named_fit, names_largest_fit, tangent_chain,
    tangent_chain_edges, with_fit_note, without_fit_note,
};
pub use blend_corner::fillet_radii;
pub use blend_variable::{RadiusSpline, Taper, fillet_chord, fillet_tapered, fillet_variable, fillet_variable_points, fillet_variable_spline, taper};
pub use body::{
    Body, COLOUR_NAME_MAX, CapSignature, DISPLAY_ANGLE, DisplayShare, EdgeInfo, FaceInfo, FacePaint, FaceSource, MeshTolerance, Paint, PointClass,
    Refusal, SurfaceKind, colour_name,
};
pub use boundary::{Boundary, boundary_patch};
pub use boundary_network::{BoundaryCurve, CurveSource, MAX_CHAIN_CURVES, MAX_EDGE_PICKS, boundary_join, boundary_surface};
pub use bridge::{Frame, blend_between, bridge, chain, chain_closes, frames, frames_across, sample_grid};
pub use build::{
    PathSeg, box_solid, canonical_revolve_axis, cylinder, extrude, extrude_tapered, extrude_tapered_between, extrude_whole_sides, loft, loft_regions,
    loft_with_rails, path_polyline, planar_face_record, revolve, revolve_whole, sphere, sweep, sweep_with, torus,
};
pub use continuity::{
    ContinuityReport, ContinuityTolerances, DEFAULT_SAMPLES, EdgePick, MAX_SAMPLES, MIN_SAMPLES, SeamReport, SeamStation, SeamVerdict, SeamWorst,
    pick_edges, seam_diagnostics,
};
pub use continuity_order::{Construction, Continuity};
pub use corner_g2::{CURVATURE_SHARE, DEFAULT_SETBACK_FACTOR};
pub use curvature_comb::{CombTooth, MAX_COMB_TEETH, MIN_COMB_TEETH, comb_edge_near, edge_comb, edge_distance, iso_comb};
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
pub use meshbody::{MAX_TRIANGLES, mesh_body, mesh_body_with_normals};
pub use native::{body_from_smlib, smlib_body};
pub use native::{brep_record, decode_native_body, encode_native_body, native_preview_mesh};
pub use offset::with_same_surface;
pub use ops::{BoolOp, apart, boolean, half_space, join_apart, place_matrix, split_by_plane, transform, transform_matrix, union_all, union_apart};
pub use point_path::{
    PathPiece, PointPath, SpaceBezier, along_path, curve_path, edge_path, face_section_along, project_onto_face, sketch_record_curve,
    sweep_point_path_with,
};
pub use polybool::faceted_boolean;
pub use polyhedron::{HalfSpace, convex_polyhedron, draft, offset_faces, shell};
pub use prism::{PLANAR_FIT_TOL, extrude_planar_regions, planar_regions_boolean};
pub use prism_curve::{PlanarPiece, PlanarSpline};
pub use prism_measure::{PrismSection, certified_prism_equivalent, certified_prism_section};
pub use repair::{Hole, RebuiltFace, RefusedFace, RepairOptions, RepairReport, SewnGap, repair};
pub use sew::{EdgeSpec, FaceSpec, SurfSpec, sew, sew_touching};
pub use solidcore_ops::member_turn::{ParallelTurnError, parallel_turn};
pub use solidcraft_smlib::hold_a_kernel;
pub use solidcraft_smlib::stop::Stop;
pub use solidcraft_smlib::trace as smlib_trace;
pub use splitface::{SplitTool, split_body, split_faces, split_faces_with_map};
pub use step::step_export;
/// The largest STEP text the importer parses; `solidcraft_io::limits` keeps it at the limit in force (#13329).
pub use step_in::p21::{max_bytes as step_max_bytes, set_max_bytes as set_step_max_bytes};
pub use step_in::{
    ImportIssue, ImportReport, ImportedBody, IssueKind, StepImport, StepNode, StepReading, step_cache_alive, step_import, step_import_shared,
    step_orientation_errors, step_validate,
};
pub use step_out::{ExportBody, ExportProduct, StepHeader, step_export_bodies, step_export_products};
pub use surface_extend::{ExtendLimit, ExtendMode, ExtendOptions, extend_surface};
pub use surface_kind::{EdgeCircle, FaceSurface, edge_circle, face_edge_indices, face_surface};
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
pub use topo::{TopoCounts, merged_topology, seam_flags, whole_edge};
pub use tspline::tspline_sheet;
pub use validation::{KeptCheck, Validation, check_kept, detach_kept, validating};
pub use validity::{SmlibCheck, Validity, smlib_dump_dir, validity};

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
    /// The vendored kernel panicked; the model is unchanged.
    #[error("kernel internal error: {0}")]
    Internal(String),
}

pub type Result<T> = std::result::Result<T, KernelError>;

/// The mesher's error as the kernel's: the same words.
impl From<solidcore_tess::TessError> for KernelError {
    fn from(e: solidcore_tess::TessError) -> KernelError {
        match e {
            solidcore_tess::TessError::Invalid(m) => KernelError::Invalid(m),
            solidcore_tess::TessError::Failed(m) => KernelError::Failed(m),
            solidcore_tess::TessError::Internal(m) => KernelError::Internal(m),
        }
    }
}

/// Why a kernel step failed: a message, or an error of the solidcore wrapper, which may be a
/// panic the wrapper caught ([`solidcraft_smlib::Error::Internal`]).
pub(crate) trait Cause: std::fmt::Display {
    /// The panic's message, when the cause is one.
    fn panic(&self) -> Option<&str> {
        None
    }
}

impl Cause for solidcraft_smlib::Error {
    fn panic(&self) -> Option<&str> {
        match self {
            solidcraft_smlib::Error::Internal(m) => Some(m),
            _ => None,
        }
    }
}

impl Cause for str {}
impl Cause for String {}
impl Cause for solidcraft_smlib::CheckReport {}
impl Cause for solidcraft_smlib::export::ExportError {}
impl Cause for solidcore_record::brep::history::Failed {}
impl Cause for solidcore_ops::pcurve::Error {}
impl<T: Cause + ?Sized> Cause for &T {
    fn panic(&self) -> Option<&str> {
        (**self).panic()
    }
}

impl KernelError {
    /// A kernel step's failure, `context` before its cause: a panic the solidcore wrapper caught
    /// (its kernel replaced by a fresh one) stays an internal error ([`KernelError::Internal`]),
    /// so the engine reports it as one; any other cause is the operation's failure.
    pub(crate) fn smlib(context: impl std::fmt::Display, e: impl Cause) -> KernelError {
        match e.panic() {
            Some(m) => KernelError::Internal(format!("{context}: {m}")),
            None => KernelError::Failed(format!("{context}: {e}")),
        }
    }
}

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
    GUARDED.with(std::cell::Cell::get) > 0 || solidcraft_smlib::in_call()
}

/// A panic inside a solidcore call, caught there and reported as the kernel's internal error
/// ([`KernelError::Internal`]): the crash-report check (`diagnostics.crash_test`'s kernel mode).
pub fn crash_test() -> Result<()> {
    solidcraft_smlib::crash_test().map_err(|e| KernelError::smlib("crash test", e))
}

/// Is this thread a Rayon pool thread?
///
/// Pool workers do not inherit the calling thread's [`guarded`] flag. Crash reports use this so
/// a panic there during a command is held with that command, instead of being written at once.
/// The browser build has no pool.
pub fn on_pool_thread() -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        rayon::current_thread_index().is_some()
    }
    #[cfg(target_arch = "wasm32")]
    {
        false
    }
}

/// Panic on a Rayon worker and catch it here, as parallel meshing does: the worker runs the
/// panic hook, then the panic resumes on this thread inside [`guard`] and becomes
/// [`KernelError::Internal`].
///
/// `diagnostics.crash_test` mode `worker` uses this. Not a modelling operation.
#[doc(hidden)]
pub fn crash_test_worker_panic(message: &str) -> Result<()> {
    let caught = catch_worker_panic(message)?;
    guard("crash test", || match caught {
        Ok(()) => Ok(()),
        Err(payload) => std::panic::resume_unwind(payload),
    })
}

/// Panic on a Rayon worker and resume that panic on this thread, outside [`guard`].
///
/// `diagnostics.crash_test` mode `worker_raw` uses this. Not a modelling operation.
#[doc(hidden)]
pub fn crash_test_worker_panic_raw(message: &str) -> Result<()> {
    let caught = catch_worker_panic(message)?;
    match caught {
        Ok(()) => Ok(()),
        Err(payload) => std::panic::resume_unwind(payload),
    }
}

/// Run `message` as a panic on a private one-thread pool and return what the worker caught.
/// The panic hook runs on that worker before this returns.
#[allow(clippy::panic)]
fn catch_worker_panic(message: &str) -> Result<std::thread::Result<()>> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = message;
        Err(KernelError::Failed("no worker pool in this build".into()))
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let message = message.to_string();
        let pool = rayon::ThreadPoolBuilder::new()
            .num_threads(1)
            .thread_name(|i| format!("solidcraft-kernel-{i}"))
            .build()
            .map_err(|e| KernelError::Failed(format!("cannot start worker: {e}")))?;
        let (tx, rx) = std::sync::mpsc::channel();
        pool.spawn(move || {
            let caught = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                std::panic::panic_any(message);
            }));
            let _ = tx.send(caught);
        });
        rx.recv().map_err(|_| KernelError::Failed("worker ended without a result".into()))
    }
}

/// Run vendored kernel code, converting a panic into an error.
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
