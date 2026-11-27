//! Contracts and result types for the commands in `cmd/workshop.rs` that read the Workshop
//! catalog (workshop.solidcraft.io): `workshop.list` and `workshop.refresh`.

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

use crate::workshop::{Item, Kind, State, Tier};

/// `workshop.list`: the items the checked index lists, filtered.
#[derive(Debug, Default, Deserialize, Serialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WorkshopListParams {
    /// Only items of this kind.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kind: Option<Kind>,
    /// Only items with this tier badge.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub tier: Option<Tier>,
    /// Words that must each appear in the item's name, summary, owner, author, slug or id (any
    /// case); at most 200 characters.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub query: Option<String>,
    /// The most items to return, 1 to 500 (default 100). `total` counts them all.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<u64>,
}

/// What `workshop.list` returns.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkshopListResult {
    /// `empty` (not read yet: run `workshop.refresh`), `loading`, `ready` or `failed`.
    pub state: State,
    /// Why the last read failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The index's sequence number (0 before the first good read).
    pub sequence: u64,
    pub issued_at: String,
    pub expires_at: String,
    /// How many items match, before `limit`.
    pub total: u64,
    /// The matching items, newest listing first as the index orders them. Open or install one in
    /// the app's Workshop window, which asks first.
    pub items: Vec<Item>,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

/// `workshop.refresh` takes no parameters.
#[derive(Debug, Deserialize, JsonSchema)]
#[schemars(deny_unknown_fields)]
pub struct WorkshopRefreshParams {}

/// What `workshop.refresh` returns: the read started (or one was already running); read the
/// result with `workshop.list`.
#[derive(Debug, Serialize, JsonSchema)]
pub struct WorkshopRefreshResult {
    pub refreshing: bool,
    /// In the warn parameter-check mode: what the call's parameters got wrong.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub param_warnings: Vec<String>,
}

#[cfg(test)]
#[path = "workshop_tests.rs"]
mod tests;
