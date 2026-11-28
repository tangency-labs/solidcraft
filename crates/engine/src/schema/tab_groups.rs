//! Contracts and result types for the commands in `cmd/tab_groups.rs`: tab groups, open designs
//! gathered under a named, coloured label in the window's tab strip. The engine holds one design
//! and no tabs: it checks the request and echoes it, and the window, which owns the tabs and their
//! groups, makes the change and fills in what it did.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// Longest group name, in characters.
pub const MAX_NAME: usize = 64;

/// A group's colour: one of the theme's accent colours, so labels stay readable in every theme.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize, JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum TabGroupColour {
    Blue,
    Green,
    Orange,
    Yellow,
    Purple,
    Cyan,
    Pink,
}

/// `tab.group.create`: put a design in a new group.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabGroupCreateParams {
    /// The design's tab, 0 first. Default: the design shown.
    #[serde(default)]
    pub tab: Option<u64>,
    /// The group's name: at most 64 characters after trimming, no control characters. Default:
    /// none (the label is a coloured dot).
    #[serde(default)]
    pub name: Option<String>,
    /// Default: the first colour no group has yet.
    #[serde(default)]
    pub colour: Option<TabGroupColour>,
}

/// `tab.group.add`: put a design in a group (out of any other), next to the group's designs.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabGroupAddParams {
    /// The group's id (`tab.group.list`).
    pub group: u64,
    /// The design's tab, 0 first. Default: the design shown.
    #[serde(default)]
    pub tab: Option<u64>,
}

/// `tab.group.remove`: take a design out of its group.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabGroupTabParams {
    /// The design's tab, 0 first. Default: the design shown.
    #[serde(default)]
    pub tab: Option<u64>,
}

/// `tab.group.update`: rename or recolour a group.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabGroupUpdateParams {
    /// The group's id (`tab.group.list`).
    pub group: u64,
    /// The new name: at most 64 characters after trimming, no control characters; empty for none.
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub colour: Option<TabGroupColour>,
    /// True folds the group's designs away to its label, false shows them again. A group whose
    /// design is on screen shows its designs until another is shown.
    #[serde(default)]
    pub collapsed: Option<bool>,
}

/// `tab.group.ungroup`: take a group away; its designs stay open.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabGroupIdParams {
    /// The group's id (`tab.group.list`).
    pub group: u64,
}

/// `tab.group.list` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct TabGroupNoParams {}

/// One group in a `tab.group.list` reply.
#[derive(Debug, Serialize, JsonSchema)]
pub struct TabGroupInfo {
    /// Unique while the window is open.
    pub id: u64,
    pub name: String,
    pub colour: TabGroupColour,
    /// Its designs are folded away to the label.
    pub collapsed: bool,
    /// Its designs' tabs, 0 first, in strip order.
    pub tabs: Vec<u64>,
}

/// What the `tab.group.*` commands return. The engine echoes the request it checked; the window
/// then fills in `group` (the group made or acted on) and, for `tab.group.list`, `groups`. The
/// design is not changed and nothing is added to undo.
#[derive(Debug, Default, Serialize, JsonSchema)]
pub struct TabGroupReply {
    /// The group asked for, or (from the window) the one made.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub group: Option<u64>,
    /// The tab asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tab: Option<u64>,
    /// The name asked for, trimmed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    /// The colour asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub colour: Option<TabGroupColour>,
    /// The folded state asked for.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub collapsed: Option<bool>,
    /// Every group, in the order they were made (`tab.group.list`, from the window).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub groups: Option<Vec<TabGroupInfo>>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "tab_groups_tests.rs"]
mod tests;
