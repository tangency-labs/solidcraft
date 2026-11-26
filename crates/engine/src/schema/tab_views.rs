//! Contracts and result types for the commands in `cmd/tab_views.rs`: the views of a design
//! (Model, Sheet, Render, Animation), shown as sub-tabs of the design's tab. The engine
//! holds one design and no tabs: it checks the request and echoes it, and the window, which owns
//! the views, makes the change and fills in what it did.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Longest view name, in characters.
pub const MAX_NAME: usize = solidcraft_views::MAX_NAME;

/// What a view shows of its design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TabViewKind {
    Model,
    Sheet,
    Render,
    Animation,
}

/// How the window lays its Model views out side by side.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabViewLayout {
    /// One view at a time.
    Single,
    SideBySide,
    Stacked,
    /// One large view and two small ones.
    LargeAndTwo,
    /// Two by two.
    Grid,
}

/// Which side of the active view another view is shown on.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TabViewEdge {
    Left,
    Right,
    Top,
    Bottom,
}

/// The destination of a view within the window showing the design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum TabViewPlacement {
    /// An empty cell, numbered from zero in reading order.
    Empty { index: usize },
    /// Replace the view in this cell or swap positions if both views are shown.
    Replace { target: u64 },
    /// Split this cell and place the view on the given edge.
    Split { target: u64, edge: TabViewEdge },
}

impl From<TabViewPlacement> for solidcraft_views::Placement {
    fn from(placement: TabViewPlacement) -> Self {
        use solidcraft_views::Edge;
        match placement {
            TabViewPlacement::Empty { index } => Self::Empty(index),
            TabViewPlacement::Replace { target } => Self::Replace(target),
            TabViewPlacement::Split { target, edge } => {
                let edge = match edge {
                    TabViewEdge::Left => Edge::Left,
                    TabViewEdge::Right => Edge::Right,
                    TabViewEdge::Top => Edge::Top,
                    TabViewEdge::Bottom => Edge::Bottom,
                };
                Self::Split { target, edge }
            }
        }
    }
}

impl From<solidcraft_views::Placement> for TabViewPlacement {
    fn from(placement: solidcraft_views::Placement) -> Self {
        use solidcraft_views::{Edge, Placement};
        match placement {
            Placement::Empty(index) => Self::Empty { index },
            Placement::Replace(target) => Self::Replace { target },
            Placement::Split { target, edge } => {
                let edge = match edge {
                    Edge::Left => TabViewEdge::Left,
                    Edge::Right => TabViewEdge::Right,
                    Edge::Top => TabViewEdge::Top,
                    Edge::Bottom => TabViewEdge::Bottom,
                };
                Self::Split { target, edge }
            }
        }
    }
}

/// `tab.view.layout`: show Model, Sheet and Render views of the design together.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewLayoutParams {
    /// How to lay them out. `single` shows one view at a time again.
    pub layout: TabViewLayout,
    /// The target window. Defaults to the last focused window.
    #[serde(default)]
    pub window: Option<u32>,
}

/// `tab.view.equal_sizes` and `tab.view.reset_cameras` act on the tiles in one window.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabViewWindowParams {
    /// The target window. Defaults to the last focused window.
    #[serde(default)]
    pub window: Option<u32>,
}

/// Which camera components are linked across the visible Model views.
#[derive(Clone, Copy, Debug, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum TabViewLinkMode {
    Off,
    Orientation,
    Zoom,
    Both,
}

impl From<TabViewLinkMode> for solidcraft_views::CameraLinkMode {
    fn from(mode: TabViewLinkMode) -> Self {
        match mode {
            TabViewLinkMode::Off => Self::Off,
            TabViewLinkMode::Orientation => Self::Orientation,
            TabViewLinkMode::Zoom => Self::Zoom,
            TabViewLinkMode::Both => Self::Both,
        }
    }
}

/// `tab.view.link`: choose which camera components the visible Model views share.
#[derive(Debug, Deserialize, Serialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabViewLinkParams {
    /// The target window. Defaults to the last focused window.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    pub mode: TabViewLinkMode,
}

/// `tab.view.beside`: show another view beside the active one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewBesideParams {
    /// The Model view to show (`tab.view.list`). Default: a new view that starts as a copy of the
    /// active one, so a second angle is one call.
    #[serde(default)]
    pub id: Option<u64>,
    /// The side of the active view it goes on. Default: right.
    #[serde(default)]
    pub edge: Option<TabViewEdge>,
    /// An explicit cell and position. Requires `id` and replaces the top-level `edge` option.
    #[serde(default)]
    pub placement: Option<TabViewPlacement>,
    /// The target window. Defaults to the last focused window.
    #[serde(default)]
    pub window: Option<u32>,
}

/// `tab.view.open`: open a view of the shown design and make it the active one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewOpenParams {
    /// What the view shows. Default: the kind of the view that is active now.
    #[serde(default)]
    pub kind: Option<TabViewKind>,
    /// Sheet subject: encoded drawing id / encoded sheet id (at most 256 bytes).
    /// A legacy bare sheet id is accepted when its drawing is unambiguous.
    /// None selects the current drawing's first sheet; unused for other kinds.
    #[serde(default)]
    pub subject: Option<String>,
    /// True shows a view that already has this kind and subject instead of opening another.
    /// Default: true when a subject is given, else false.
    #[serde(default)]
    pub reuse: Option<bool>,
    /// The target window. Defaults to the last focused window.
    #[serde(default)]
    pub window: Option<u32>,
}

/// `tab.view.close`, `tab.view.duplicate`: act on one view.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewIdParams {
    /// The view's id (`tab.view.list`). Default: the active view.
    #[serde(default)]
    pub id: Option<u64>,
}

/// `tab.view.close_others` or `tab.view.close_right`: close views in one window's strip.
#[derive(Debug, Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
pub struct TabViewCloseGroupParams {
    /// The view to leave open. Defaults to the active view in the chosen window.
    #[serde(default)]
    pub id: Option<u64>,
    /// The target window. Defaults to the named view's window or the last focused window.
    #[serde(default)]
    pub window: Option<u32>,
}

/// `tab.view.activate`: show one view, by id or by place.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewActivateParams {
    /// The view's id (`tab.view.list`). Give this or `index`.
    #[serde(default)]
    pub id: Option<u64>,
    /// The view's place in tab order, 0 first. Give this or `id`.
    #[serde(default)]
    pub index: Option<u64>,
    /// The window containing the view. Defaults to the last focused window.
    #[serde(default)]
    pub window: Option<u32>,
}

/// `tab.view.rename`: give a view a new name.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewRenameParams {
    /// The view's id (`tab.view.list`). Default: the active view.
    #[serde(default)]
    pub id: Option<u64>,
    /// The new name: 1 to 64 characters after trimming, no control characters. Left out, the window
    /// opens the name field of the view for typing.
    #[serde(default)]
    pub name: Option<String>,
}

/// `tab.view.move`: move a view to another place among the views of its design.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewMoveParams {
    /// Its place now, 0 first.
    pub from: u64,
    /// Its place after the move, 0 first.
    pub to: u64,
}

/// `tab.view.reopen` and `tab.view.list` take no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewNoParams {}

/// `tab.view.collapse`: fold the sub-tabs of the shown design into its tab, or show them again.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewCollapseParams {
    /// True folds the views into the design's tab (its name and how many views it has), false
    /// shows them as sub-tabs again. Default: the other way round from now.
    #[serde(default)]
    pub collapsed: Option<bool>,
}

/// One view in a `tab.view.list` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabViewInfo {
    /// Unique within the design while it is open.
    pub id: u64,
    pub kind: TabViewKind,
    pub name: String,
    /// The sheet it shows, when it shows one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Whether it is the view shown.
    pub active: bool,
}

/// What the `tab.view.*` commands return. The engine echoes the request it checked (the keys
/// the call gave); the window then fills in `id` (the view it opened, closed or now shows) and,
/// for `tab.view.list`, `views`. The design is not changed and nothing is added to undo.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TabViewReply {
    /// Whether the shown design's views are folded into its tab: asked for (`tab.view.collapse`),
    /// then (from the window) as they are after the call; `tab.view.list` says it too.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    /// The kind asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<TabViewKind>,
    /// The subject asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub subject: Option<String>,
    /// Whether reuse was asked for (after the default).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub reuse: Option<bool>,
    /// The view acted on: asked for, or (from the window) the one opened, reopened, closed or
    /// now shown.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<u64>,
    /// The place asked for (`tab.view.activate`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub index: Option<u64>,
    /// The window asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub window: Option<u32>,
    /// The name asked for, trimmed (`tab.view.rename`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The place asked to move from (`tab.view.move`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    /// The place asked to move to (`tab.view.move`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<u64>,
    /// The layout asked for (`tab.view.layout`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub layout: Option<TabViewLayout>,
    /// The side asked for (`tab.view.beside`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edge: Option<TabViewEdge>,
    /// An explicit cell and position asked for by `tab.view.beside`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub placement: Option<TabViewPlacement>,
    /// How many views the window shows together after the call (from the window).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub shown: Option<u64>,
    /// The views of the shown design in tab order (`tab.view.list`, from the window).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub views: Option<Vec<TabViewInfo>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "tab_views_tests.rs"]
mod tests;

/// Set visibility of the orientation compass in one 3D view.
#[derive(Debug, Deserialize, JsonSchema, Serialize)]
#[serde(deny_unknown_fields)]
pub struct ViewCompassParams {
    pub view: u64,
    pub on: bool,
}
