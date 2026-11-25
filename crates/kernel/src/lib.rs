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
mod backend;
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
mod dome;
mod draft_corners;
pub mod drawing_centre_mark;
mod emboss;
mod face_flux;
#[cfg(test)]
mod face_source_tests;
pub mod failure;
mod fair;
mod fill;
#[cfg(test)]
mod fillet_setback_tests;
mod form_cell;
mod freeform;
pub mod fuzz;
mod heal;
mod helix;
pub mod history;
pub mod hlr;
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
mod issue_13455_tests;
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
pub mod network;
mod numbered;
mod offset;
mod offset_backend;
mod ops;
#[cfg(test)]
mod outline_tests;
mod par;
#[cfg(test)]
mod split_cone_tests;
pub use par::KERNEL_STACK;
pub use par::for_each_on_threads;
pub use par::kernel_thread;
pub use par::map_on_threads;
pub use par::map_with_local_pools;
#[cfg(test)]
mod display_plan_tests;
#[cfg(test)]
mod pinch_tests;
mod placement;
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
mod record_eval_tests;
mod repair;
mod route;
#[cfg(test)]
mod seam_faces_tests;
pub mod services;
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
mod tolerance_heal;
mod topo;
mod tspline;
#[cfg(test)]
mod untouched_faces_tests;
mod validation;
mod validity;
pub mod watch;

pub use backend::Solidcore;
pub use blend::ChamferSide;
pub use blend::FilletTurn;
pub use blend::FitSearch;
pub use blend::SETBACK_AT_LEAST;
pub use blend::blend_edge_middles;
pub use blend::blend_edges;
pub use blend::chamfer_carrying_face;
pub use blend::chamfer_max;
pub use blend::chamfer_max_why;
pub use blend::chamfer_sides_carrying_face;
pub use blend::chamfer_sides_max_why;
pub use blend::fillet_each;
pub use blend::fillet_g2;
pub use blend::fillet_g2_corners;
pub use blend::fillet_g2_with_g2_corners;
pub use blend::fillet_least_setback;
pub use blend::fillet_max;
pub use blend::fillet_max_each;
pub use blend::fillet_max_why;
pub use blend::fillet_removing;
pub use blend::fillet_setback;
pub use blend::largest_fit_note;
pub use blend::named_fit;
pub use blend::names_largest_fit;
pub use blend::scale_fit_note;
pub use blend::smooth_picks;
pub use blend::tangent_chain;
pub use blend::tangent_chain_beyond_points;
pub use blend::tangent_chain_edges;
pub use blend::with_fit_note;
pub use blend::without_fit_note;
pub use blend_corner::fillet_radii;
pub use blend_variable::RadiusSpline;
pub use blend_variable::Taper;
pub use blend_variable::fillet_chord;
pub use blend_variable::fillet_tapered;
pub use blend_variable::fillet_variable;
pub use blend_variable::fillet_variable_points;
pub use blend_variable::fillet_variable_spline;
pub use blend_variable::taper;
pub use body::Body;
pub use body::COLOUR_NAME_MAX;
pub use body::CapSignature;
pub use body::DISPLAY_ANGLE;
pub use body::DisplayShare;
pub use body::EdgeInfo;
pub use body::EdgeLines;
pub use body::FaceInfo;
pub use body::FacePaint;
pub use body::FaceSource;
pub use body::KernelForms;
pub use body::MESH_MIN_ANGLE;
pub use body::MeshSpec;
pub use body::MeshTolerance;
pub use body::OUTLINE_TRIM_SAMPLES;
pub use body::Paint;
pub use body::PlanStats;
pub use body::PointClass;
pub use body::RayPick;
pub use body::Refusal;
pub use body::SurfaceKind;
pub use body::colour_name;
pub use body::display_tolerance;
pub use body::placed_mesh;
pub use boundary::Boundary;
pub use boundary::boundary_patch;
pub use boundary_network::BoundaryCurve;
pub use boundary_network::CurveSource;
pub use boundary_network::MAX_CHAIN_CURVES;
pub use boundary_network::MAX_EDGE_PICKS;
pub use boundary_network::boundary_join;
pub use boundary_network::boundary_surface;
pub use bridge::Frame;
pub use bridge::blend_between;
pub use bridge::bridge;
pub use bridge::chain;
pub use bridge::chain_closes;
pub use bridge::frames;
pub use bridge::frames_across;
pub use bridge::sample_grid;
pub use build::PathSeg;
pub use build::canonical_revolve_axis;
pub use build::extrude_tapered;
pub use build::extrude_tapered_between;
pub use build::extrude_whole_sides;
pub use build::loft;
pub use build::loft_regions;
pub use build::loft_with_rails;
pub use build::offset_loop;
pub use build::path_polyline;
pub use build::planar_face_record;
pub use build::sweep;
pub use build::sweep_with;
pub use build::symmetric_offset_intervals;
pub use build::symmetric_offset_probe_points;
pub use continuity::ContinuityReport;
pub use continuity::ContinuityTolerances;
pub use continuity::DEFAULT_SAMPLES;
pub use continuity::EdgePick;
pub use continuity::MAX_SAMPLES;
pub use continuity::MIN_SAMPLES;
pub use continuity::SeamReport;
pub use continuity::SeamStation;
pub use continuity::SeamVerdict;
pub use continuity::SeamWorst;
pub use continuity::pick_edges;
pub use continuity::seam_diagnostics;
pub use continuity_order::Construction;
pub use continuity_order::Continuity;
pub use corner_g2::CURVATURE_SHARE;
pub use corner_g2::DEFAULT_SETBACK_FACTOR;
pub use curvature_comb::CombTooth;
pub use curvature_comb::MAX_COMB_TEETH;
pub use curvature_comb::MIN_COMB_TEETH;
pub use curvature_comb::comb_edge_near;
pub use curvature_comb::edge_comb;
pub use curvature_comb::edge_distance;
pub use curvature_comb::edge_middle;
pub use curvature_comb::iso_comb;
pub use delete_face::delete_faces;
pub use distance::BodyDistance;
pub use distance::DistanceShape;
pub use distance::Members;
pub use distance::body_distance;
pub use distance::edges_closest;
pub use distance::faces_closest;
pub use distance::interiors_disjoint;
pub use distance::point_closest;
pub use dome::dome;
pub use draft_corners::CornerFill;
pub use draft_corners::CornerOption;
pub use draft_corners::CornerRefusal;
pub use draft_corners::CornerReport;
pub use draft_corners::DraftCorner;
pub use draft_corners::draft_corners;
pub use emboss::cylinder_emboss_anchor;
pub use emboss::emboss_anchor;
pub use emboss::emboss_cylinder;
pub use emboss::emboss_cylinder_at;
pub use emboss::emboss_project_face;
pub use emboss::has_cylinder_emboss_wall;
pub use emboss::infer_cylinder_emboss;
pub use failure::ErrorCode;
pub use fair::FairCurve;
pub use fair::FairNet;
pub use fair::FairOptions;
pub use fair::FairReport;
pub use fair::MAX_FAIR_ITERATIONS;
pub use fair::MAX_FAIR_POINTS;
pub use fair::fair_bspline_curve;
pub use fair::fair_bspline_surface;
pub use fair::fair_curve;
pub use fair::fair_faces;
pub use fair::fair_net;
pub use fair::fair_nurbs_curve;
pub use fair::fair_nurbs_surface;
pub use fair::fair_surface;
pub use fill::PATCH_ANGLE_TOL_DEG;
pub use fill::PatchReport;
pub use fill::PatchSpec;
pub use fill::patch_edges_crowned;
pub use fill::patch_edges_report;
pub use fill::patch_edges_with;
pub use helix::loft_to_point;
pub use helix::loft_to_point_with_rails;
pub use helix::sweep_helix;
pub use helix::sweep_helix_mesh;
pub use helix::sweep_helix_radial;
pub use helix::sweep_helix_tapered;
pub use iges_in::MAX_IGES_BYTES;
pub use iges_in::iges_import;
pub use iges_in::iges_native_system;
pub use iges_in::iges_to_step;
pub use iges_out::iges_export_bodies;
pub use intersection::MeetingEdge;
pub use intersection::meeting_edges;
pub use iso::IsoLine;
pub use iso::MAX_ISO_COUNT;
pub use iso::iso_curves;
pub use iso::iso_grid;
pub use lip::LipRefusal;
pub use lip::raise_band;
pub use lip::rim_band;
pub use loft_blend::loft_blend;
pub use matchsurf::match_surface;
pub use matchsurf::seam_continuity;
pub use measure::BodyMeasure;
pub use measure::MassMeasure;
pub use measure::levels_along;
pub use measure::measure;
pub use measure::measure_mass;
pub use mesh_convert::convert_prismatic_mesh;
pub use meshbody::MAX_TRIANGLES;
pub use meshbody::mesh_body;
pub use meshbody::mesh_body_with_normals;
pub use meshbody::reduce_mesh;
pub use native::body_from_smlib;
pub use native::brep_record;
pub use native::decode_native_body;
pub use native::encode_native_body;
pub use native::native_preview_mesh;
pub use native::smlib_body;
pub use offset::with_same_surface;
pub use ops::BoolOp;
pub use ops::apart;
pub use ops::half_space;
pub use ops::join_apart;
pub use ops::place_matrix;
pub use ops::split_by_plane;
pub use ops::transform;
pub use ops::transform_matrix;
pub use ops::union_all;
pub use ops::union_apart;
pub use point_path::PathPiece;
pub use point_path::PointPath;
pub use point_path::SpaceBezier;
pub use point_path::along_path;
pub use point_path::curve_path;
pub use point_path::edge_path;
pub use point_path::face_section_along;
pub use point_path::project_onto_face;
pub use point_path::sketch_record_curve;
pub use point_path::sweep_point_path_with;
pub use polybool::faceted_boolean;
pub use polyhedron::HalfSpace;
pub use polyhedron::convex_polyhedron;
pub use polyhedron::draft;
pub use prism::PLANAR_FIT_TOL;
pub use prism::extrude_planar_regions;
pub use prism::planar_regions_boolean;
pub use prism_curve::PlanarPiece;
pub use prism_curve::PlanarSpline;
pub use prism_measure::PrismSection;
pub use prism_measure::certified_prism_equivalent;
pub use prism_measure::certified_prism_section;
pub use repair::Hole;
pub use repair::RebuiltFace;
pub use repair::RefusedFace;
pub use repair::RepairOptions;
pub use repair::RepairRefusal;
pub use repair::RepairReport;
pub use repair::SewnGap;
pub use repair::repair;
pub use route::CELL_GRID_LIMITS;
pub use route::Which;
pub use route::boolean;
pub use route::box_solid;
pub use route::cell_regions;
pub use route::chamfer;
pub use route::chamfer_sides;
pub use route::cylinder;
pub use route::default_cell_grid;
pub use route::extrude;
pub use route::fillet;
pub use route::offset_faces;
pub use route::plan_cell_grid;
pub use route::read_cadir;
pub use route::revolve;
pub use route::revolve_whole;
pub use route::shell;
pub use route::sphere;
pub use route::step_string;
pub use route::thicken;
pub use route::torus;
pub use route::which;
pub use route::write_cadir;
pub use sew::EdgeSpec;
pub use sew::FaceSpec;
pub use sew::MAX_CHAIN_POINTS;
pub use sew::SurfSpec;
pub use sew::sew;
pub use sew::sew_touching;
pub use sew::sew_touching_traced;
pub use solidcore_ops::dome::DomeRefusal;
pub use solidcore_ops::dome::DomeShape;
pub use solidcore_ops::measure::ParallelTurnError;
pub use solidcore_ops::measure::turn_to_parallel;
/// SolidCraft's kernel interface: the trait each kernel's adapter implements, its operations,
/// inputs and errors.
pub use solidcraft_kernel_api as api;
pub use solidcraft_kernel_api::{CadirBodies, CadirFailure, CadirRead, CadirRefused, CadirWritten};
pub use solidcraft_smlib::hold_a_kernel;
pub use solidcraft_smlib::start;
pub use solidcraft_smlib::stop::Stop;
/// The cancel token over this thread's work ([`Stop::within`]), for work outside the kernel that
/// stops with it (a sketch solve).
pub use solidcraft_smlib::stop::token as stop_token;
pub use solidcraft_smlib::trace as smlib_trace;
pub use splitface::SplitTool;
pub use splitface::face_split_if_on_face;
pub use splitface::has_cap_at;
pub use splitface::split_at_facing_side;
pub use splitface::split_body;
pub use splitface::split_body_by_faces;
pub use splitface::split_faces;
pub use splitface::split_faces_with_map;
pub use splitface::{FieldKind, PieceSides, split_piece_sides};
pub use step::step_export;
pub use step::step_export_with_face_ids;
pub use step_in::HealedBody;
pub use step_in::ImportChecks;
pub use step_in::ImportHeals;
pub use step_in::ImportIssue;
pub use step_in::ImportReport;
pub use step_in::ImportedBody;
pub use step_in::IssueKind;
pub use step_in::LeftOut;
/// An import's limits, which every reader of an assembly applies alike: the deepest nesting
/// followed, the most bodies, and the most occurrences visited.
pub use step_in::MAX_ASSEMBLY_DEPTH as IMPORT_MAX_DEPTH;
pub use step_in::MAX_BODIES as IMPORT_MAX_BODIES;
pub use step_in::MAX_VISITS as IMPORT_MAX_VISITS;
pub use step_in::StepImport;
pub use step_in::StepNode;
pub use step_in::StepReading;
/// The memory a STEP file may need, checked before it is opened to warn about it (#15588).
pub use step_in::p21::check_memory as step_check_memory;
/// The largest STEP text the importer parses; `solidcraft_io::limits` keeps it at the limit in force (#13329).
pub use step_in::p21::max_bytes as step_max_bytes;
pub use step_in::p21::memory_to_spare as step_memory_to_spare;
pub use step_in::p21::set_max_bytes as set_step_max_bytes;
pub use step_in::p21::with_memory_to_spare as step_with_memory_to_spare;
pub use step_in::records::ImportRecords;
pub use step_in::records::RecordBody;
pub use step_in::records::RecordMesh;
pub use step_in::records::import_trims;
pub use step_in::records::records_import;
pub use step_in::records::records_import_shared;
pub use step_in::records::records_noted;
pub use step_in::records::records_text;
pub use step_in::step_cache_alive;
pub use step_in::step_import;
pub use step_in::step_import_shared;
pub use step_in::step_orientation_errors;
pub use step_in::step_validate;
pub use step_out::ExportBody;
pub use step_out::ExportProduct;
pub use step_out::StepExport;
pub use step_out::StepHeader;
pub use step_out::step_export_bodies;
pub use step_out::step_export_bodies_with_face_ids;
pub use step_out::step_export_products;
pub use surface_extend::ExtendLimit;
pub use surface_extend::ExtendMode;
pub use surface_extend::ExtendOptions;
pub use surface_extend::extend_surface;
pub use surface_kind::AxialPiece;
pub use surface_kind::EdgeCircle;
pub use surface_kind::FaceSurface;
pub use surface_kind::axial_face_section;
pub use surface_kind::edge_circle;
pub use surface_kind::face_edge_indices;
pub use surface_kind::face_surface;
pub use surface_offset::offset_surface;
pub use surface_ruled::Ruled;
pub use surface_ruled::RuledCorners;
pub use surface_ruled::RuledMode;
pub use surface_ruled::ruled_far_handles;
pub use surface_ruled::ruled_surface;
pub use surface_ruled::ruled_surface_with;
pub use surface_trim::TrimKeep;
pub use surface_trim::mutual_trim;
pub use surface_trim::trim;
pub use surface_trim::trim_dropped_keep;
pub use surface_trim::trim_keep;
pub use surface_trim::trim_sheet_keep;
pub use surface_untrim::UntrimHoles;
pub use surface_untrim::untrim;
pub use surfaces::copy_faces;
pub use surfaces::extend;
pub use surfaces::extrude_sheet;
pub use surfaces::loft_sheet;
pub use surfaces::loft_sheet_with_rails;
pub use surfaces::loft_sheet_with_rails_legacy_sides;
pub use surfaces::patch_edges;
pub use surfaces::patch_region;
pub use surfaces::remove_faces;
pub use surfaces::reverse_faces;
pub use surfaces::revolve_sheet;
pub use surfaces::revolve_sheet_legacy_sides;
pub use surfaces::stitch;
pub use surfaces::sweep_sheet;
pub use surfaces::sweep_sheet_point_path_with;
pub use surfaces::sweep_sheet_points;
pub use surfaces::sweep_sheet_points_with;
pub use surfaces::sweep_sheet_with;
pub use surfaces::unstitch;
pub use sweep_path::RailFit;
pub use sweep_path::SweepControl;
pub use sweep_path::SweepOrientation;
pub use sweep_path::sweep_path;
pub use sweep_path::sweep_path_with;
pub use tolerance_heal::HealFailure;
pub use tolerance_heal::HealPreferenceForTests;
pub use tolerance_heal::ToleranceHeal;
pub use tolerance_heal::heal_preference_for_tests;
pub use tolerance_heal::heal_tolerance_defects;
pub use tolerance_heal::heals_tolerance_defects;
pub use tolerance_heal::set_heals_tolerance_defects;
pub use topo::CylinderFace;
pub use topo::TopoCounts;
pub use topo::cylinder_between_rims;
pub use topo::cylinder_end_at;
pub use topo::cylinder_face;
pub use topo::cylinder_face_at;
pub use topo::material_in_ring;
pub use topo::merged_topology;
pub use topo::seam_flags;
pub use topo::whole_edge;
pub use tspline::tspline_sheet;
pub use validation::CheckFailure;
pub use validation::FailedTest;
pub use validation::KeptCheck;
pub use validation::Validation;
pub use validation::check_kept;
pub use validation::detach_kept;
pub use validation::failing_check;
pub use validation::validating;
pub use validity::SmlibCheck;
pub use validity::Validity;
pub use validity::smlib_dump_dir;
pub use validity::validity;

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
    /// A lip's band that leaves no sound face beside it.
    #[error("the operation failed: raising a band: {0}")]
    Lip(#[from] LipRefusal),
    /// A repair that has nothing it can do: the part needs none, or SMLib's check fails it for
    /// what the repair does not fix.
    #[error("the operation failed: {0}")]
    Repair(#[from] RepairRefusal),
    /// A heal of a part's tolerance-scale defects that gave no answer: stopped, or it could not
    /// run ([`heal_tolerance_defects`]).
    #[error("the operation failed: {0}")]
    Heal(#[from] HealFailure),
    /// A dome the face or the height cannot take.
    #[error("the operation failed: {0}")]
    Dome(#[from] DomeRefusal),
}

pub type Result<T> = std::result::Result<T, KernelError>;

/// Most guide rails a loft takes. solidcore states the same limit (`solidcore::sweep::loft_rails`);
/// the kernel does not depend on solidcore, so the number is repeated here.
pub const MAX_LOFT_RAILS: usize = 16;

/// The mesher's error as the kernel's: the same words.
impl From<solidcore_tess::TessError> for KernelError {
    fn from(e: solidcore_tess::TessError) -> KernelError {
        match e {
            solidcore_tess::TessError::Invalid(m) => KernelError::Invalid(m),
            solidcore_tess::TessError::Failed(m) => KernelError::Failed(m),
            solidcore_tess::TessError::Internal(m) => KernelError::Internal(m),
            e @ solidcore_tess::TessError::TooManyVertices { .. } => KernelError::TooLarge(e.to_string()),
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
impl Cause for solidcraft_geom::brep::eval::Error {}
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

/// Is this thread inside [`guard`], an SMLib call or the mesher's own guard? (A panic now will be
/// caught and turned into an error; the engine's crash reports mark it recovered.)
pub fn guarded() -> bool {
    GUARDED.with(std::cell::Cell::get) > 0 || solidcraft_smlib::in_call() || solidcore_tess::guarded()
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
