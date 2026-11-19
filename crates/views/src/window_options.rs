//! Presentation settings that belong to a pop-out and travel with its remembered arrangement.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum CardHeaders {
    #[default]
    Always,
    Hover,
    Hidden,
}

impl CardHeaders {
    pub const ALL: [Self; 3] = [Self::Always, Self::Hover, Self::Hidden];
}

/// A missing value in an older remembered arrangement gets the classic pop-out behaviour.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowOptions {
    pub card_headers: CardHeaders,
    pub keep_on_top: bool,
    pub ribbon: bool,
    pub ribbon_auto_hide: bool,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self { card_headers: CardHeaders::Always, keep_on_top: false, ribbon: false, ribbon_auto_hide: true }
    }
}

impl WindowOptions {
    /// The initial preference for newly opened pop-outs. Deserialize remembered windows
    /// through Default so a window saved before the ribbon keeps its appearance.
    pub fn new_pop_out() -> Self {
        Self { ribbon: true, ..Self::default() }
    }
}
