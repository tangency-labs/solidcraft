//! A drawing view's settings (W07, #5866): which way it looks, its scale and how it draws.
//! `drawing.view.snapshot` takes them whole; a saved view (W12) keeps the same fields, each one
//! optional so that a projected view can follow its parent.

use serde::{Deserialize, Deserializer, Serialize};
use solidcraft_geom::Plane;
use solidcraft_geom::drawing_view::{Invalid, NamedView, Scale, basis_looking};
use solidcraft_render::VisualStyle;

/// The most of its reference parts a view shows, as a percent.
pub const MAX_REFERENCE_SHOWN_PERCENT: f64 = 100.0;

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
    pub display_style: VisualStyle,
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
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct ViewOverrides {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scale: Option<Scale>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub display_style: Option<VisualStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_style: Option<VisualStyle>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reference_hides_model: Option<bool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tangent_edges: Option<TangentEdges>,
}

impl ViewOverrides {
    /// Sets nothing: every setting follows.
    pub fn is_empty(&self) -> bool {
        *self == ViewOverrides::default()
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
