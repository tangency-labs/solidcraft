//! How a view draws the model: its lines, its shading, or both. One list for drawing views
//! (W07, W17b) and the viewport's styles (#5219, #5307) to map to.

use serde::{Deserialize, Serialize};

/// A view's visual style.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VisualStyle {
    /// The visible lines only.
    #[default]
    Visible,
    /// The visible lines and the hidden ones.
    VisibleHidden,
    /// The shaded model with no lines.
    Shaded,
    /// The shaded model under its visible lines.
    ShadedVisible,
    /// The shaded model under its visible and hidden lines.
    ShadedVisibleHidden,
}

impl VisualStyle {
    pub const ALL: [VisualStyle; 5] =
        [VisualStyle::Visible, VisualStyle::VisibleHidden, VisualStyle::Shaded, VisualStyle::ShadedVisible, VisualStyle::ShadedVisibleHidden];

    /// Whether the model is drawn as a picture.
    pub fn is_shaded(self) -> bool {
        matches!(self, VisualStyle::Shaded | VisualStyle::ShadedVisible | VisualStyle::ShadedVisibleHidden)
    }

    /// Whether hidden lines are drawn.
    pub fn draws_hidden(self) -> bool {
        matches!(self, VisualStyle::VisibleHidden | VisualStyle::ShadedVisibleHidden)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The five styles, by their snake-case names, and what each draws.
    #[test]
    fn each_style_reads_by_its_name_and_says_what_it_draws() {
        let names = ["visible", "visible_hidden", "shaded", "shaded_visible", "shaded_visible_hidden"];
        for (style, name) in VisualStyle::ALL.into_iter().zip(names) {
            assert_eq!(serde_json::to_value(style).unwrap(), serde_json::json!(name));
            assert_eq!(serde_json::from_value::<VisualStyle>(serde_json::json!(name)).unwrap(), style);
        }
        let shaded: Vec<bool> = VisualStyle::ALL.iter().map(|s| s.is_shaded()).collect();
        let hidden: Vec<bool> = VisualStyle::ALL.iter().map(|s| s.draws_hidden()).collect();
        assert_eq!(shaded, [false, false, true, true, true]);
        assert_eq!(hidden, [false, true, false, false, true]);
        assert_eq!(VisualStyle::default(), VisualStyle::Visible);
        for not_a_style in ["Shaded", "hidden", "", "visible-hidden"] {
            assert!(serde_json::from_value::<VisualStyle>(serde_json::json!(not_a_style)).is_err(), "{not_a_style}");
        }
    }
}
