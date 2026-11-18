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

#[derive(Clone, Copy, Debug, Default, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum TopBar {
    #[default]
    Always,
    Edge,
}

impl TopBar {
    pub const ALL: [Self; 2] = [Self::Always, Self::Edge];
}

/// Logical interface size as a percentage of the workspace scale.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(try_from = "u16", into = "u16")]
#[schemars(extend("enum" = [90, 100, 125, 150, 200]))]
pub struct InterfaceSize(#[schemars(with = "u16")] u16);

impl InterfaceSize {
    pub const ALL: [Self; 5] = [Self(90), Self(100), Self(125), Self(150), Self(200)];

    pub fn factor(self) -> f32 {
        f32::from(self.0) / 100.0
    }
}

impl Default for InterfaceSize {
    fn default() -> Self {
        Self(100)
    }
}

impl TryFrom<u16> for InterfaceSize {
    type Error = &'static str;

    fn try_from(percent: u16) -> Result<Self, Self::Error> {
        let size = Self(percent);
        Self::ALL.contains(&size).then_some(size).ok_or("interface size must be 90, 100, 125, 150, or 200")
    }
}

impl From<InterfaceSize> for u16 {
    fn from(size: InterfaceSize) -> Self {
        size.0
    }
}

/// A missing value in an older remembered arrangement gets the classic pop-out behaviour.
#[derive(Clone, Copy, Debug, Deserialize, Eq, JsonSchema, PartialEq, Serialize)]
#[serde(default)]
pub struct WindowOptions {
    pub card_headers: CardHeaders,
    pub keep_on_top: bool,
    pub interface_size: InterfaceSize,
    pub panels: bool,
    pub ribbon: bool,
    pub ribbon_auto_hide: bool,
    pub top_bar: TopBar,
}

impl Default for WindowOptions {
    fn default() -> Self {
        Self {
            card_headers: CardHeaders::Always,
            keep_on_top: false,
            interface_size: InterfaceSize::default(),
            panels: false,
            ribbon: false,
            ribbon_auto_hide: true,
            top_bar: TopBar::Always,
        }
    }
}

impl WindowOptions {
    /// The initial preference for newly opened pop-outs. Deserialize remembered windows
    /// through Default so a window saved before the ribbon keeps its appearance.
    pub fn new_pop_out() -> Self {
        Self { ribbon: true, ..Self::default() }
    }
}
