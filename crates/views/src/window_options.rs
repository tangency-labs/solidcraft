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
#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowOptions {
    pub card_headers: CardHeaders,
    pub keep_on_top: bool,
}
