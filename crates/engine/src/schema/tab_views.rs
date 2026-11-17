//! Contracts and result types for the commands in `cmd/tab_views.rs`: the views of a design
//! (Model, Sketch, Sheet, Render, Animation), shown as sub-tabs of the design's tab. The engine
//! holds one design and no tabs: it checks the request and echoes it, and the window, which owns
//! the views, makes the change and fills in what it did.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Longest view name, in characters.
pub const MAX_NAME: usize = 64;

/// What a view shows of its design.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TabViewKind {
    Model,
    Sketch,
    Sheet,
    Render,
    Animation,
}

/// `tab.view.open`: open a view of the shown design and make it the active one.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewOpenParams {
    /// What the view shows. Default: the kind of the view that is active now.
    #[serde(default)]
    pub kind: Option<TabViewKind>,
    /// The sketch or sheet the view shows (its id, at most 256 bytes); none for the other kinds.
    #[serde(default)]
    pub subject: Option<String>,
    /// True shows a view that already has this kind and subject instead of opening another.
    /// Default: true when a subject is given, else false.
    #[serde(default)]
    pub reuse: Option<bool>,
}

/// `tab.view.close`, `tab.view.duplicate`: act on one view.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabViewIdParams {
    /// The view's id (`tab.view.list`). Default: the active view.
    #[serde(default)]
    pub id: Option<u64>,
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

/// One view in a `tab.view.list` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabViewInfo {
    /// Unique within the design while it is open.
    pub id: u64,
    pub kind: TabViewKind,
    pub name: String,
    /// The sketch or sheet it shows, when it shows one.
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
    /// The name asked for, trimmed (`tab.view.rename`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The place asked to move from (`tab.view.move`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub from: Option<u64>,
    /// The place asked to move to (`tab.view.move`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub to: Option<u64>,
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
