//! Contracts for managing the windows of the active design. The command engine validates the
//! request. A host with design tabs applies it and returns the resulting window state.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The destination can be a new pop-out or the id of an open window.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabWindowTarget {
    #[default]
    New,
    #[serde(untagged)]
    Existing(u32),
}

/// The supported level of a new pop-out.
#[derive(Clone, Copy, Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabNewWindowLevel {
    #[default]
    ViewsOnly,
}

/// A display by name or zero-based index.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TabWindowDisplay {
    Name(String),
    Index(u32),
}

/// Move a view to another window of its design.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabViewMoveToWindowParams {
    /// The open view's id, as reported by `tab.view.list`.
    pub view: u64,
    /// Open a new window by default, or move to an existing window (main is 0).
    #[serde(default)]
    pub window: TabWindowTarget,
    /// Zero-based insertion position in an existing strip, after removing the view if it was there.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub index: Option<usize>,
    /// A tile placement in an existing destination window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub placement: Option<super::tab_views::TabViewPlacement>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub display: Option<TabWindowDisplay>,
    /// Horizontal position in logical points relative to the selected display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub x: Option<f32>,
    /// Vertical position in logical points relative to the selected display.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub y: Option<f32>,
    /// Width in logical points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub w: Option<f32>,
    /// Height in logical points.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub h: Option<f32>,
    /// The chrome level of a new window. Defaults to Views only.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<TabNewWindowLevel>,
}

/// A request naming a window of the active design. The main window has id 0.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabWindowIdParams {
    pub window: u32,
}

/// List the windows of the active design.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabWindowListParams {}

/// The geometry of a native window in logical points.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabWindowGeometry {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
    pub maximised: bool,
    pub fullscreen: bool,
}

/// The result of moving a view in a host with design tabs.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabWindowMoveResult {
    pub view: u64,
    pub window: u32,
    pub created: bool,
    pub from: u32,
    pub closed_source: bool,
    pub placement: Option<TabWindowGeometry>,
}

/// The checked request from the engine or the completed move from its host.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TabWindowMoveReply {
    Completed(TabWindowMoveResult),
    Requested(TabViewMoveToWindowParams),
}

/// One window in `tab.window.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabWindowInfo {
    pub id: u32,
    /// The window's chrome level.
    pub level: String,
    pub views: Vec<u64>,
    pub active: u64,
    /// The layout name, or `custom` for a layout that is not a preset.
    pub layout: String,
    pub display: Option<String>,
    pub placement: Option<TabWindowGeometry>,
    pub tool_window: bool,
}

/// The windows that currently belong to the active design.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabWindowListResult {
    pub windows: Vec<TabWindowInfo>,
}

/// The checked request from the engine or the current windows from its host.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TabWindowListReply {
    Completed(TabWindowListResult),
    Requested(TabWindowListParams),
}

/// The views a closed window returned to the main window, in strip order.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabWindowCloseResult {
    pub returned: Vec<u64>,
}

/// The checked request from the engine or the returned views from its host.
#[derive(Debug, Serialize, JsonSchema)]
#[serde(untagged)]
pub enum TabWindowCloseReply {
    Completed(TabWindowCloseResult),
    Requested(TabWindowIdParams),
}
