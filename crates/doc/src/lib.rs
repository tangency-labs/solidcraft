//! The SolidCraft document: user and model parameters, and a parametric timeline of features
//! (sketches, extrudes, revolves, fillets, primitives, combines…). Evaluating the timeline with
//! the kernel produces the model (bodies and solved sketches). Evaluation is incremental: a
//! feature is recomputed only when it, a parameter it uses, or an earlier feature changed.
#![deny(clippy::unwrap_used, clippy::expect_used, clippy::panic, clippy::unimplemented, clippy::todo, clippy::unreachable)]
#![forbid(unsafe_code)]

pub mod appearance;
pub mod appearance_pack;
mod assembly;
mod body_transfer;
mod body_transfer_sheet;
mod browser_tree;
pub mod newer_file;
pub mod pbr_appearance;
pub use body_transfer::{valid_transfer_matrix, validate_body_transfer};
pub mod canvas;
mod clipboard;
pub use clipboard::{clone_expression, remap_topology};
pub mod component_color;
mod component_removal;
pub use component_removal::RemovedTree;
pub mod config;
pub mod construct;
pub mod csv;
pub mod display_quality;
pub mod display_units;
mod document;
pub mod drawing;
pub mod end_cap;
mod eval;
pub mod expr;
pub mod face_anchor;
mod feature_geometry;
mod feature_refs;
pub use face_anchor::{FaceAnchor, FaceAttachmentStatus};
pub mod format;
mod format_offsets;
pub mod frame;
pub mod gpu_plan;
mod history_names;
pub mod hole_reference;
pub mod ids;
mod inline_params;
pub use format_offsets::whole_profile_selection_from_value;
pub use inline_params::Defined;
pub mod joints;
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
mod points;
mod profile_refs;
pub mod progress;
mod thread_catalog;
pub use thread_catalog::{
    DRILL_ALLOWANCE, TaperDrill, TappedThread, TappedThreadFamily, metric_tap_drill, parse_tapped_thread, tapped_thread_sizes, untabulated_taper_tap,
};
mod walk;
pub use profile_refs::{capture_profile_selection, profile_fallback_notes, profile_selection_indices, profile_selection_indices_lenient};
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
pub mod sketch_equation;
pub mod text_params;
pub mod thread_forms;
pub mod tolerance;

pub use assembly::{
    BomType, ExpressedMove, ExpressedShift, IDENTITY, MAX_EXPRESSED_MOVES, MAX_PLACEMENT_DEPTH, MAX_PLACEMENTS, Mat, Occurrence, WorldNames,
    apply_point, apply_vector, is_identity, is_placement, map_point_expr, map_vector_expr, mat_inverse, mat_mul, rigid,
};
pub use document::*;
pub use eval::cell_grid::{MAX_CUT_CELLS, SLOW_CUT_CELLS};
pub use eval::{
    EdgeRef, FeatureResult, KeptSeparate, Model, ModelBody, ModelState, PlacedBodies, RevolveExtent, SavedModel, SolvedSketch, ThreadInfo,
    blend_target, curve_pick_error, draft_corner_options, edge_names_for, edge_refs, face_point_names, face_point_state, hole_direction_at,
    hole_on_surface, import_body_names, join_pieces_added, map_wire, metric_thread_sizes, object_height, parse_metric_thread, pattern_matrices,
    revolve_extent, revolve_extent_for_handle, revolve_extent_with_convention, select_profiles, sketch_hole_direction, sketch_line_plane,
    state_in_frame, state_in_frame_at, suggested_thread, suggested_thread_in, tangent_face_chain, through_all_distance, with_placed_copies,
    world_state, world_state_reusing,
};
pub use eval::{EvalCache, KeptResults};
pub use expr::Expr;
pub use names::{BodyName, FeatureId, FeatureName, SketchId, copy_name};
pub use params::{ParamRow, ParamUser, cycles, name_words_match};
pub use points::{Dir, FacePick, Position, plain};
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
}

impl LostProjection {
    /// `projected face of Tray (curve l1)`.
    pub fn label(&self) -> String {
        const SHOWN: usize = 4;
        let (ids, one, several) = if self.curves.is_empty() { (&self.points, "point", "points") } else { (&self.curves, "curve", "curves") };
        let listed = ids.iter().take(SHOWN).map(String::as_str).collect::<Vec<_>>().join(", ");
        match ids.len() {
            0 => self.what.clone(),
            1 => format!("{} ({one} {listed})", self.what),
            n if n > SHOWN => format!("{} ({several} {listed}, …)", self.what),
            _ => format!("{} ({several} {listed})", self.what),
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
mod feature_walk_tests;
#[cfg(test)]
mod named_projection_tests;
#[cfg(test)]
mod old_cap_anchor_tests;
#[cfg(test)]
mod tests;
#[cfg(test)]
mod tolerance_tests;

mod anchor_frame;
pub use anchor_frame::state_in_face_anchor_frame;
