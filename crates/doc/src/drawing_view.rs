//! A drawing view's settings (W07, #5866): which way it looks, its scale and how it draws.
//! `drawing.view.snapshot` takes them whole; a saved view (W12) keeps the same fields, each one
//! optional so that a projected view can follow its parent.

use std::sync::Arc;

use serde::{Deserialize, Deserializer, Serialize};
use solidcore_record::Plane;
use solidcraft_geom::drawing_view::{Invalid, NamedView, Scale, basis_looking};
use solidcraft_geom::paper::line::LineGroup;
use solidcraft_render::VisualStyle;

/// The most of its reference parts a view shows, as a percent.
pub const MAX_REFERENCE_SHOWN_PERCENT: f64 = 100.0;

/// Occurrence paths from the design's root, each the subtree it places. Shared, so resolving a view
/// on every redraw copies none of them.
pub type OccurrencePaths = Arc<[Vec<u64>]>;

/// Which way a view looks: by name, or along any direction with an up.
#[derive(Clone, Copy, Debug, PartialEq, Serialize)]
#[serde(untagged)]
pub enum ViewDirection {
    Named(NamedView),
    Looking(Looking),
}

/// A view along `direction` with `up` toward the top of the paper, through `origin` (the world
/// origin when not given).
#[derive(Clone, Copy, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Looking {
    pub direction: [f64; 3],
    pub up: [f64; 3],
    #[serde(default)]
    pub origin: [f64; 3],
}

impl<'de> Deserialize<'de> for ViewDirection {
    /// One message for anything that is neither, rather than serde's "did not match any variant".
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<ViewDirection, D::Error> {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Either {
            Named(NamedView),
            Looking(Looking),
        }
        let message = "view is front, back, top, bottom, left, right, iso, iso_front_right, iso_front_left, iso_back_right, iso_back_left, or {direction, up, origin?}";
        Ok(match Either::deserialize(d).map_err(|_| serde::de::Error::custom(message))? {
            Either::Named(view) => ViewDirection::Named(view),
            Either::Looking(looking) => ViewDirection::Looking(looking),
        })
    }
}

impl Default for ViewDirection {
    fn default() -> Self {
        ViewDirection::Named(NamedView::Front)
    }
}

impl ViewDirection {
    /// The view's basis: a named view's exact one, or one built from the direction and up.
    pub fn basis(&self) -> Result<Plane, Invalid> {
        match *self {
            ViewDirection::Named(view) => Ok(view.basis()),
            ViewDirection::Looking(Looking { direction, up, origin }) => basis_looking(direction.into(), up.into(), origin.into()),
        }
    }
}

/// Tangent edges, where faces meet smoothly: left out (the default) or drawn.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TangentEdges {
    #[default]
    Hide,
    Show,
}

/// Which state of its assembly a view draws (#13048): as the design places its parts (the default),
/// or one of the design's exploded views, by its id. The design is never changed: the view draws a
/// copy. An id the design no longer has draws a placeholder, never refuses the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Exploded {
    #[default]
    Assembled,
    View(u64),
}

/// Which configuration of the design a view draws (#13048): the design's active one, as the design
/// is now (the default), or a row of its configuration table by the row's id
/// ([`crate::config::ConfigRow`]), which a rename keeps. The design is never changed: the view draws a
/// copy. A row the design no longer has draws a placeholder, never refuses the file.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ViewConfiguration {
    #[default]
    Active,
    Row(u64),
}

/// Most joints a view's position sets.
pub const MAX_POSE_JOINTS: usize = 64;
/// Most values one joint has in a position: at least as many as any joint kind has
/// ([`crate::joints::JointKind::dofs`]).
pub const MAX_JOINT_VALUES: usize = 6;
/// The largest value a position saves, either way, in radians or millimetres.
pub const MAX_POSE_VALUE: f64 = 1e9;

/// One joint's values in a view's position, by the joint's id, in the units and order joints save them
/// ([`crate::joints::JointKind::dofs`]: angles in radians, distances in mm).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct JointPose {
    pub joint: u64,
    pub values: Vec<f64>,
}

/// A step of one of the design's motion studies, by the study's id: a whole step, as studies count
/// them ([`crate::motion::MotionStudy::steps`]).
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct StudyStep {
    pub id: u64,
    pub step: u32,
}

impl StudyStep {
    /// Its study among `studies` and the step a view draws of it: a step past the study's end draws
    /// its last. None when the design no longer has the study.
    pub fn drawn<'a>(&self, studies: &'a [crate::motion::MotionStudy]) -> Option<(&'a crate::motion::MotionStudy, u32)> {
        studies.iter().find(|m| m.id == self.id).map(|m| (m, self.step.min(m.steps)))
    }
}

/// The position of its assembly a view draws (#13048): as the design places its parts (the
/// default), with some of its joints at values of its own, or at a step of one of its motion
/// studies. The design is never changed: the view draws a copy. A joint or study the design no
/// longer has is skipped, never refuses the file.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Pose {
    #[default]
    Designed,
    Joints(Arc<[JointPose]>),
    Study(StudyStep),
}

/// What a view shows and how. Every field has a default: the front view at the scale that fits
/// its page, visible lines only, tangent edges hidden.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewSettings {
    pub view: ViewDirection,
    /// None: the largest preset scale at which the view fits its page ([`Scale::fit`]). Not
    /// written when None: a snapshot refuses a null scale.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<Scale>,
    pub visual_style: VisualStyle,
    /// How reference parts draw (bill of materials type `reference`).
    pub reference_style: VisualStyle,
    /// Whether reference parts hide the model's lines behind them.
    pub reference_hides_model: bool,
    /// How far the view's boundary grows from the model's box towards the box that also holds its
    /// reference parts, each side on its own: 0 to [`MAX_REFERENCE_SHOWN_PERCENT`].
    pub reference_shown_percent: f64,
    pub tangent_edges: TangentEdges,
}

/// The settings a saved view sets itself, each on its own: one left out follows the view's parent,
/// then [`ViewSettings`]'s default ([`crate::drawing::Drawing::resolve_view`]). None is written.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<Scale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub visual_style: Option<VisualStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_style: Option<VisualStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_hides_model: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tangent_edges: Option<TangentEdges>,
    /// The subtrees it draws, with `[]` for the design's own bodies: every body the design places
    /// when no view of its chain sets them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub occurrences: Option<OccurrencePaths>,
    /// The subtrees it leaves out: none when no view of its chain sets them.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hide: Option<OccurrencePaths>,
    /// The ISO 128 line group its lines are drawn in: its sheet's, then its drawing's, when no view
    /// of its chain sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub line_group: Option<LineGroup>,
    /// The state of its assembly it draws: assembled when no view of its chain sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exploded: Option<Exploded>,
    /// The position of its assembly it draws: the design's when no view of its chain sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pose: Option<Pose>,
    /// The configuration of the design it draws: the active one when no view of its chain sets one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub configuration: Option<ViewConfiguration>,
}

impl ViewOverrides {
    /// Sets nothing: every setting follows.
    pub fn is_empty(&self) -> bool {
        self == &ViewOverrides::default()
    }
}

impl ViewSettings {
    /// What reading alone can't check: the percent's range and the view's basis.
    pub fn validate(&self) -> Result<Plane, Invalid> {
        let percent = self.reference_shown_percent;
        if !(percent.is_finite() && (0.0..=MAX_REFERENCE_SHOWN_PERCENT).contains(&percent)) {
            return Err(Invalid("reference_shown_percent is a number from 0 to 100"));
        }
        self.view.basis()
    }
}

#[cfg(test)]
#[path = "drawing_view_tests.rs"]
mod tests;
