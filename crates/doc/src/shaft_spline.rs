//! Shaft Spline (#17693): the numbers of a spline on a shaft and in its hub, checked and planned by the kernel
//! (`solidcore_ops::shaft_spline`), before any body is touched. The rebuild of a `shaft_spline` feature
//! (`eval_shaft_spline.rs`), the command that adds one and the dialog's drawing all go through [`plan`], so they
//! cannot disagree about a value, and every refusal names the parameter to change in the same words.
//!
//! The kinds of teeth, what a new part is, the finish of a straight-sided tooth's tip corners and the master
//! spline are the enums of the feature ([`SplineForm`], [`SplineMake`], [`SplineCorner`], [`SplineMaster`]): the
//! kernel's own, as data. This file computes no geometry: the tooth outlines, the checks of the numbers and the
//! numbers they come to are the kernel's, and so is every point, direction, length and angle the rebuild works with
//! (the seat on a round face, the outlines as profiles, the sections of the tools).

pub(crate) use solidcore_ops::shaft_spline::{
    angles_apart, axis_gap, centre_angle, chamfer_plane, chamfer_section, chord_gap, cutter_radius, cutter_section, into_frame, moved_out,
    off_cylinder, outline_loop, points_apart, reach_along, seat_frame, start_candidates, tool_margin,
};
pub use solidcore_ops::shaft_spline::{bezier_pieces, fit_to_shaft_within, module_for_major_radius, outline_points, same_size, space_section};
pub use solidcore_record::shaft_spline::{
    MasterArc, MasterPlaced, OutlinePiece, SpaceSection, SplineCorner, SplineForm, SplineMade, SplineMake, SplineMaster, SplineNumbers, SplinePlan,
    SplineRefusal, ToothOutline, ToothRole,
};

/// What the numbers make, with the last few answers kept: the dialog's drawing, the command and the rebuild ask
/// for the same numbers one after another, and the kernel's work on involute flanks is not small.
pub fn plan(n: &SplineNumbers) -> Result<SplinePlan, SplineRefusal> {
    use std::sync::{Mutex, PoisonError};
    type Kept = Vec<(String, Result<SplinePlan, SplineRefusal>)>;
    static KEPT: Mutex<Kept> = Mutex::new(Vec::new());
    // Every number in full.
    let key = format!("{n:?}");
    let hit = KEPT.lock().unwrap_or_else(PoisonError::into_inner).iter().find(|(k, _)| *k == key).map(|(_, made)| made.clone());
    if let Some(made) = hit {
        return made;
    }
    let made = solidcore_ops::shaft_spline::plan(n);
    let mut kept = KEPT.lock().unwrap_or_else(PoisonError::into_inner);
    if kept.len() >= 8 {
        drop(kept.remove(0));
    }
    kept.push((key, made.clone()));
    made
}
