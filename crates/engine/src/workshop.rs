//! The Workshop catalog (workshop.solidcraft.io): what the signed index lists, as the host last
//! read and checked it, for `workshop.list` and the app's Workshop window.
//!
//! The engine does no network work and checks no signature: the host fetches the index, verifies
//! it with a key built into the app and gives the session a [`WorkshopHost`] that answers with
//! the checked listing ([`Catalog`]) and starts a new fetch on request. A session without a host
//! (the command line, the browser build) has no Workshop. Nothing here downloads, opens or
//! installs an item: that always goes through the app's question first.

use std::sync::Arc;

use schemars::JsonSchema;
use serde::{Deserialize, Serialize};

/// The most items one `workshop.list` returns.
pub const MAX_LIMIT: usize = 500;
/// How many items `workshop.list` returns when `limit` is left out.
pub const DEFAULT_LIMIT: usize = 100;
/// The longest search text read, in characters.
pub const MAX_QUERY: usize = 200;

/// What the host offers the session.
pub trait WorkshopHost: Send + Sync {
    /// The catalog as it is now.
    fn snapshot(&self) -> Catalog;
    /// Start reading the index again and return at once. `Err`: it couldn't start.
    fn refresh(&self) -> std::result::Result<(), String>;
}

/// The session's way to the Workshop; `None`: this app has none.
pub type Workshop = Arc<dyn WorkshopHost>;

/// Where the catalog is.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum State {
    /// Not read yet (`workshop.refresh` reads it).
    #[default]
    Empty,
    /// Being read.
    Loading,
    /// Read and checked: `items` is what it lists.
    Ready,
    /// The last read failed (`error` says why); any items are from the last good read.
    Failed,
}

/// An item's kind, as the Workshop's links spell it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    /// A design, opened like File › Open.
    Model,
    /// A plugin package, installed through Install plugin….
    Plugin,
    /// A parts library, unpacked for the Parts Library panel.
    Part,
}

impl Kind {
    pub const ALL: &[Kind] = &[Kind::Plugin, Kind::Model, Kind::Part];

    pub fn id(self) -> &'static str {
        match self {
            Kind::Model => "model",
            Kind::Plugin => "plugin",
            Kind::Part => "part",
        }
    }
}

/// The tier badge: the same three words the Workshop shows.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Tier {
    /// Made by Tangency Labs.
    Official,
    /// Reviewed and signed by the SolidCraft team.
    Community,
    /// Listed after machine checks only (also any tier this SolidCraft doesn't know).
    Unverified,
}

/// One listed item.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Item {
    /// The stable id: it never changes.
    pub id: String,
    pub kind: Kind,
    /// The owner's username (a parts library's publisher).
    pub owner: String,
    /// The item's name in addresses, unique per owner and kind.
    pub slug: String,
    /// The display name.
    pub name: String,
    pub summary: String,
    pub tier: Tier,
    /// The author as shown: name and handle.
    pub author: String,
    /// An SPDX id ("" when the index gives none).
    pub licence: String,
    /// The newest version this SolidCraft can run; `null` when every version needs a newer one.
    pub version: Option<String>,
    /// The SolidCraft version the newest version needs, when this one is too old for all of them.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub needs_app: Option<String>,
    /// For a plugin, the permissions `version` asks for.
    pub permissions: Vec<String>,
    /// The download size of `version`, in bytes (0 with no usable version).
    pub size: u64,
    /// The Workshop's warning about `version` (a `warn` revocation), when there is one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub warning: Option<String>,
    /// The item's page on the Workshop.
    pub page_url: String,
}

/// The checked catalog.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, JsonSchema)]
pub struct Catalog {
    pub state: State,
    /// Why the last read failed.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    /// The index's sequence number (0 before the first good read).
    pub sequence: u64,
    /// When the index was issued (UTC, as the index writes it).
    pub issued_at: String,
    /// When it stops being good for installs.
    pub expires_at: String,
    pub items: Vec<Item>,
}

/// What `workshop.list` keeps.
#[derive(Clone, Debug, Default)]
pub struct Filter {
    pub kind: Option<Kind>,
    pub tier: Option<Tier>,
    /// Words that must each appear in the name, summary, owner, author, slug or id (any case).
    pub query: String,
}

impl Filter {
    /// Whether `item` is kept.
    pub fn keeps(&self, item: &Item) -> bool {
        if self.kind.is_some_and(|k| k != item.kind) || self.tier.is_some_and(|t| t != item.tier) {
            return false;
        }
        let haystack = [&item.name, &item.summary, &item.owner, &item.author, &item.slug, &item.id]
            .iter()
            .map(|s| s.to_lowercase())
            .collect::<Vec<_>>()
            .join("\n");
        self.query.split_whitespace().take(32).all(|word| haystack.contains(&word.to_lowercase()))
    }

    /// The kept items, in the catalog's order.
    pub fn apply<'a>(&'a self, items: &'a [Item]) -> impl Iterator<Item = &'a Item> + 'a {
        items.iter().filter(move |item| self.keeps(item))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn item(id: &str, kind: Kind, tier: Tier, name: &str, summary: &str) -> Item {
        Item {
            id: id.into(),
            kind,
            owner: "maker".into(),
            slug: id.into(),
            name: name.into(),
            summary: summary.into(),
            tier,
            author: "A Maker (@maker)".into(),
            licence: "MIT".into(),
            version: Some("1.0.0".into()),
            needs_app: None,
            permissions: Vec::new(),
            size: 10,
            warning: None,
            page_url: String::new(),
        }
    }

    #[test]
    fn the_filter_keeps_kind_tier_and_every_word() {
        let items = vec![
            item("gears", Kind::Plugin, Tier::Community, "Gears", "Spur and helical gear generator."),
            item("shelf", Kind::Model, Tier::Unverified, "Wall shelf", "A bracket for a shelf."),
            item("fasteners", Kind::Part, Tier::Official, "Fasteners", "Bolts, nuts and washers."),
        ];
        let ids = |f: &Filter| f.apply(&items).map(|i| i.id.clone()).collect::<Vec<String>>();
        assert_eq!(ids(&Filter::default()), ["gears", "shelf", "fasteners"]);
        assert_eq!(ids(&Filter { kind: Some(Kind::Model), ..Filter::default() }), ["shelf"]);
        assert_eq!(ids(&Filter { tier: Some(Tier::Official), ..Filter::default() }), ["fasteners"]);
        assert_eq!(ids(&Filter { query: "HELICAL gear".into(), ..Filter::default() }), ["gears"]);
        assert_eq!(ids(&Filter { query: "maker".into(), ..Filter::default() }), ["gears", "shelf", "fasteners"]);
        assert!(ids(&Filter { query: "gear shelf".into(), ..Filter::default() }).is_empty());
        assert!(ids(&Filter { kind: Some(Kind::Plugin), tier: Some(Tier::Official), ..Filter::default() }).is_empty());
    }
}
