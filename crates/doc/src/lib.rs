//! The SolidCraft document: user and model parameters, and a parametric timeline of features
//! (sketches, extrudes, revolves, fillets, primitives, combines…). Evaluating the timeline with
//! the kernel produces the model (bodies and solved sketches). Evaluation is incremental: a
//! feature is recomputed only when it, a parameter it uses, or an earlier feature changed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod appearance;
pub mod appearance_pack;
mod assembly;
pub mod blob;
mod body_transfer;
mod body_transfer_sheet;
pub mod bom;
mod browser_tree;
pub mod newer_file;
pub mod pbr_appearance;
pub use body_transfer::valid_transfer_matrix;
pub use body_transfer::validate_body_transfer;
pub mod canvas;
mod clipboard;
pub use clipboard::clone_expression;
pub use clipboard::remap_topology;
pub use clipboard::remap_topology_faces;
pub mod component_color;
mod component_removal;
pub use component_removal::RemovedTree;
pub mod config;
pub mod construct;
pub mod csv;
pub mod display_quality;
pub mod display_units;
mod document;
pub mod drafting_profile;
pub mod drawing;
pub mod drawing_view;
pub mod end_cap;
mod envelope;
mod eval;
pub mod expr;
pub mod face_anchor;
mod feature_geometry;
mod feature_refs;
mod flush_join;
pub use face_anchor::FaceAnchor;
pub use face_anchor::FaceAttachmentStatus;
pub mod format;
mod format_offsets;
pub mod frame;
pub mod frame_corners;
pub mod frame_layout;
mod frame_multi_trim;
mod frame_plate;
pub mod gpu_plan;
pub mod gusset;
mod history_names;
pub mod hole_reference;
pub mod ids;
pub mod import_defects;
mod inline_params;
pub use format_offsets::whole_profile_selection_from_value;
pub use inline_params::Defined;
pub mod joints;
pub mod json_layout;
mod lineage;
pub mod metadata;
pub use metadata::Metadata;
pub mod motion;
pub mod notes;
pub use notes::Note;
mod name_lookup;
mod names;
pub mod naming;
mod occurrence_tree;
mod params;
pub mod persistent_name;
pub mod physical_material;
mod piece_names;
pub mod pipe_thread_catalog;
pub mod pipe_thread_sizes;
pub mod plugin_data;
pub mod point_cloud;
mod point_cloud_complex;
pub mod point_cloud_distribution;
mod point_expr;
mod points;
mod profile_refs;
pub mod progress;
mod thread_catalog;
pub use thread_catalog::DRILL_ALLOWANCE;
pub use thread_catalog::TaperDrill;
pub use thread_catalog::TappedThread;
pub use thread_catalog::TappedThreadFamily;
pub use thread_catalog::metric_tap_drill;
pub use thread_catalog::parse_tapped_thread;
pub use thread_catalog::tapped_thread_sizes;
pub use thread_catalog::untabulated_taper_tap;
mod walk;
pub use profile_refs::WHOLE_PROFILE_TOO_MANY_CURVES;
pub use profile_refs::capture_profile_selection;
pub use profile_refs::profile_fallback_notes;
pub use profile_refs::profile_selection_indices;
pub use profile_refs::profile_selection_indices_lenient;
pub mod plastic;
mod print_face;
pub use print_face::PrintFace;
pub mod project;
mod project3d;
pub mod render_aging;
pub mod render_coat;
pub mod render_job;
pub mod render_optics;
pub mod render_resources;
pub mod render_settings;
pub mod render_surface;
pub mod render_texture_resources;
pub mod sheet;
pub mod sheet_library;
mod sheet_surface;
mod sheet_surface_bead;
mod sheet_surface_convert;
mod sheet_surface_cuts;
mod sheet_surface_features;
mod sheet_surface_fold;
mod sheet_surface_perforation;
mod sheet_surface_shadow;
mod sheet_surface_tab;
pub mod sketch_equation;
pub use sheet_surface_bead::MAX_BEADS;
pub use sheet_surface_bead::SurfaceBead;
pub use sheet_surface_convert::ConvertRule;
pub use sheet_surface_convert::RollEdge;
pub use sheet_surface_convert::RollInfo;
pub use sheet_surface_convert::RollLine;
pub use sheet_surface_convert::SheetFaces;
pub use sheet_surface_convert::SheetMeasure;
pub use sheet_surface_convert::SurfaceBendLine;
pub use sheet_surface_convert::SurfaceConversion;
pub use sheet_surface_convert::SurfaceSheet;
pub use sheet_surface_convert::convert_surfaces;
pub use sheet_surface_convert::measure_sheet_body;
pub use sheet_surface_cuts::Loose;
pub use sheet_surface_cuts::Recess;
pub use sheet_surface_features::OUTSIDE_BEND_OPEN;
pub mod text_params;
pub mod thread_forms;
pub mod tolerance;
pub mod weld_spot;

pub use assembly::BomType;
pub use assembly::ExpressedAt;
pub use assembly::ExpressedMove;
pub use assembly::ExpressedShift;
pub use assembly::IDENTITY;
pub use assembly::MAX_EXPRESSED_MOVES;
pub use assembly::MAX_PLACEMENT_DEPTH;
pub use assembly::MAX_PLACEMENTS;
pub use assembly::Mat;
pub use assembly::Occurrence;
pub use assembly::WorldNames;
pub use assembly::apply_point;
pub use assembly::apply_vector;
pub use assembly::capture_sketch_plane_placement;
pub use assembly::is_identity;
pub use assembly::is_placement;
pub use assembly::map_dir_expr;
pub use assembly::map_plane;
pub use assembly::map_plane_ref;
pub use assembly::map_point_expr;
pub use assembly::map_vector_expr;
pub use assembly::mat_inverse;
pub use assembly::mat_mul;
pub use assembly::mat_volume_scale;
pub use assembly::reframe_plane_placements;
pub use assembly::rigid;
pub use document::*;
pub use eval::EdgeRef;
pub use eval::EvalCache;
pub use eval::ExtrudeEnds;
pub use eval::FeatureResult;
pub use eval::KeptResults;
pub use eval::KeptSeparate;
pub use eval::Model;
pub use eval::ModelBody;
pub use eval::ModelState;
pub use eval::PatternRow;
pub use eval::PendingChecks;
pub use eval::PlacedBodies;
pub use eval::PreparedMeshImport;
pub use eval::RevolveExtent;
pub use eval::SavedModel;
pub use eval::SketchCopy;
pub use eval::SolvedSketch;
pub use eval::ThreadInfo;
pub use eval::blend_target;
pub use eval::cell_grid::MAX_CUT_CELLS;
pub use eval::cell_grid::SLOW_CUT_CELLS;
pub use eval::cell_grid::cell_profile;
pub use eval::curve_pick_error;
pub use eval::draft_corner_options;
pub use eval::edge_names_for;
pub use eval::edge_refs;
pub use eval::face_point_names;
pub use eval::face_point_state;
pub use eval::feature_operation;
pub use eval::from_slope_height;
pub use eval::hole_direction_at;
pub use eval::hole_on_surface;
pub use eval::hole_start;
pub use eval::import_body_names;
pub use eval::join_pieces_added;
pub use eval::keep_one_sided_direction;
pub use eval::map_wire;
pub use eval::metric_thread_sizes;
pub use eval::object_height;
pub use eval::parse_metric_thread;
pub use eval::pattern_matrices;
pub use eval::pattern_row;
pub use eval::reconcile_component_patterns;
pub use eval::revolve_extent;
pub use eval::revolve_extent_for_handle;
pub use eval::revolve_extent_with_convention;
pub use eval::same_extrude_from_face;
pub use eval::select_profiles;
pub use eval::sketch_hole_direction;
pub use eval::sketch_line_plane;
pub use eval::state_in_frame;
pub use eval::state_in_frame_at;
pub use eval::suggested_thread;
pub use eval::suggested_thread_in;
pub use eval::tangent_face_chain;
pub use eval::through_all_distance;
pub use eval::to_end_is_flat;
pub use eval::with_placed_copies;
pub use eval::world_body_paths;
pub use eval::world_state;
pub use eval::world_state_moved_reusing;
pub use eval::world_state_reusing;
pub use expr::Expr;
pub use feature_geometry::restored_thread_pick;
pub use names::BodyName;
pub use names::FeatureId;
pub use names::FeatureName;
pub use names::SketchId;
pub use names::copy_name;
pub use params::ParamRow;
pub use params::ParamUser;
pub use params::cycles;
pub use params::name_words_match;
pub use points::Dir;
pub use points::FacePick;
pub use points::Position;
pub use points::plain;
pub use solidcraft_develop as develop;
pub use solidcraft_kernel as kernel;
pub use solidcraft_sketch as sketch;

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum DocError {
    #[error("expression: {0}")]
    Expr(String),
    #[error("{0}")]
    Invalid(String),
    #[error("unknown {0}")]
    Unknown(String),
    #[error(transparent)]
    Sketch(#[from] solidcraft_sketch::SketchError),
    #[error(transparent)]
    Kernel(#[from] solidcraft_kernel::KernelError),
    /// A stored reference (a face or edge name, a piece of a split face) no longer finds exactly
    /// what it named: the feature using it fails, and the user picks it again. `feature` is
    /// empty until the evaluation of the feature fills it in ([`DocError::in_feature`]).
    #[error("{}", reselect_message(feature, reference))]
    Reselect { feature: String, reference: String },
    /// An id or index read from a body's record or a sheet's own lists names no entry there:
    /// `what` (say "face") number `index`, where the list holds `count`. The data naming it is
    /// stale or corrupt; the operation fails instead of reading past the list.
    #[error("{what} {index} doesn't exist (there are {count}): the data naming it is stale or corrupt")]
    Missing { what: &'static str, index: usize, count: usize },
    /// A sketch's projections whose face or edge, kept by its persistent name, finds nothing any
    /// more: the sketch fails, naming them as the user sees them. `feature` as for `Reselect`.
    #[error("{}", lost_projections_message(feature, links))]
    LostProjections { feature: String, links: Vec<LostProjection> },
}

/// A projection of a sketch whose named face or edge is gone ([`DocError::LostProjections`]).
#[derive(Debug, Clone, PartialEq)]
pub struct LostProjection {
    pub link: String,
    /// What it projects: `projected face of Tray`.
    pub what: String,
    /// The ids of the sketch curves it gives.
    pub curves: Vec<String>,
    /// The ids of its points, named when it gives no curve.
    pub points: Vec<String>,
    /// The persistent name that finds nothing, never shown.
    pub name: String,
    /// The names of the features in the design that made the lost face, edge or vertex, as
    /// `name` gives them, each once: where to look for the edit that lost it (#5048).
    pub from: Vec<String>,
}

impl LostProjection {
    /// `projected face of Tray (curve l1; made by Extrude1, Fillet2)`.
    pub fn label(&self) -> String {
        const SHOWN: usize = 4;
        let (ids, one, several) = if self.curves.is_empty() { (&self.points, "point", "points") } else { (&self.curves, "curve", "curves") };
        let listed = ids.iter().take(SHOWN).map(String::as_str).collect::<Vec<_>>().join(", ");
        let entities = match ids.len() {
            0 => None,
            1 => Some(format!("{one} {listed}")),
            n if n > SHOWN => Some(format!("{several} {listed}, …")),
            _ => Some(format!("{several} {listed}")),
        };
        let from = (!self.from.is_empty()).then(|| format!("made by {}", self.from.join(", ")));
        match (entities, from) {
            (Some(e), Some(f)) => format!("{} ({e}; {f})", self.what),
            (Some(x), None) | (None, Some(x)) => format!("{} ({x})", self.what),
            (None, None) => self.what.clone(),
        }
    }

    /// `projected face of Tray (curve l1) is lost; delete the projection or project it again`.
    pub fn warning(&self) -> String {
        format!("{}{LOST_ONE}", self.label())
    }
}

impl DocError {
    /// A feature's `reference` that no longer finds what it named ([`DocError::Reselect`]), for
    /// the feature being evaluated.
    pub fn reselect(reference: impl Into<String>) -> Self {
        DocError::Reselect { feature: String::new(), reference: reference.into() }
    }

    /// This error, raised while feature `feature` was evaluated: a [`DocError::Reselect`] or
    /// [`DocError::LostProjections`] names it.
    pub fn in_feature(self, feature: &str) -> Self {
        match self {
            DocError::Reselect { feature: f, reference } if f.is_empty() => DocError::Reselect { feature: feature.into(), reference },
            DocError::LostProjections { feature: f, links } if f.is_empty() => DocError::LostProjections { feature: feature.into(), links },
            other => other,
        }
    }
}

/// What a feature that lost a reference says ([`DocError::Reselect`]).
fn reselect_message(feature: &str, reference: &str) -> String {
    let feature = if feature.is_empty() { "this feature" } else { feature };
    format!("{feature}: the reference `{reference}` no longer exists as it was picked; reselect it")
}

/// The feature and the reference of a [`DocError::Reselect`] message, read back from its text
/// (a feature's error is kept as text): `None` for any other message.
pub fn reselect_parts(text: &str) -> Option<(&str, &str)> {
    let (feature, rest) = text.split_once(": the reference `")?;
    let reference = rest.strip_suffix("` no longer exists as it was picked; reselect it")?;
    Some((feature, reference))
}

const LOST_ONE: &str = " is lost; delete the projection or project it again";
const LOST_SEVERAL: &str = " projected references are lost: ";
const LOST_MORE: &str = " and ";
const LOST_SEVERAL_END: &str = "; delete each projection or project it again";
/// The projections a lost-projections message lists by name; the rest are counted.
const LOST_LISTED: usize = 3;

/// What a sketch whose projections are lost says ([`DocError::LostProjections`]).
fn lost_projections_message(feature: &str, links: &[LostProjection]) -> String {
    let feature = if feature.is_empty() { "this sketch" } else { feature };
    match links {
        [one] => format!("{feature}: {}", one.warning()),
        _ => {
            let listed = links.iter().take(LOST_LISTED).map(LostProjection::label).collect::<Vec<_>>().join(", ");
            let more = match links.len().saturating_sub(LOST_LISTED) {
                0 => String::new(),
                n => format!("{LOST_MORE}{n} more"),
            };
            format!("{feature}: {}{LOST_SEVERAL}{listed}{more}{LOST_SEVERAL_END}", links.len())
        }
    }
}

/// A [`DocError::LostProjections`] message read back ([`lost_projection_parts`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct LostProjectionsText<'a> {
    /// What comes before the projections: the sketch's name, after whatever a downstream feature
    /// put in front of it (`sketch `Outline` failed: Outline`).
    pub feature: &'a str,
    pub count: usize,
    /// The projections it names.
    pub listed: &'a str,
    /// How many more it only counts.
    pub more: usize,
}

/// A [`DocError::LostProjections`] message read back from its text, found by its own wording, so
/// a sketch name holding ": " or the words of a projection's kind, and a feature failing because
/// of that sketch, still read back (a body name holding them as well does not). `None` for any
/// other message.
pub fn lost_projection_parts(text: &str) -> Option<LostProjectionsText<'_>> {
    if let Some(rest) = text.strip_suffix(LOST_SEVERAL_END) {
        let (head, listed) = rest.split_once(LOST_SEVERAL)?;
        let (feature, count) = head.rsplit_once(": ")?;
        let counted = listed.rsplit_once(LOST_MORE).and_then(|(named, n)| Some((named, n.strip_suffix(" more")?.parse().ok()?)));
        let (listed, more) = counted.unwrap_or((listed, 0));
        return Some(LostProjectionsText { feature, count: count.parse().ok()?, listed, more });
    }
    let rest = text.strip_suffix(LOST_ONE)?;
    let at = [": projected ", ": intersected ", ": included "].iter().filter_map(|kind| rest.rfind(kind)).max()?;
    Some(LostProjectionsText { feature: rest.get(..at)?, count: 1, listed: rest.get(at + 2..)?, more: 0 })
}

pub type Result<T> = std::result::Result<T, DocError>;

#[cfg(test)]
mod coil_tests;
#[cfg(test)]
mod drafting_profile_tests;
#[cfg(test)]
mod feature_walk_tests;
#[cfg(test)]
mod named_projection_tests;
#[cfg(test)]
mod old_cap_anchor_tests;
#[cfg(test)]
mod saved_text_font_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tolerance_tests;

mod anchor_frame;
pub use anchor_frame::state_in_face_anchor_frame;
