//! Saved, movable application-bar items. Unknown ids survive a downgrade without being drawn.

use serde::{Deserialize, Serialize};

pub const MAX_ITEMS: usize = 64;
pub const MAX_ID_CHARS: usize = 100;

/// Stable ids, independent of the measured layout engine's slot ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemId {
    Home,
    Save,
    Undo,
    Redo,
    Quick(String),
    Search,
    Assistant,
    /// Reserved transient identity; Update is locked and has no default placement.
    Update,
    Donate,
    Discord,
    Report,
    Settings,
    Help,
    Version,
}

impl ItemId {
    pub fn parse(id: &str) -> Option<Self> {
        if !valid_id(id) {
            return None;
        }
        Some(match id {
            "home" => Self::Home,
            "save" => Self::Save,
            "undo" => Self::Undo,
            "redo" => Self::Redo,
            "search" => Self::Search,
            "assistant" => Self::Assistant,
            "update" => Self::Update,
            "donate" => Self::Donate,
            "discord" => Self::Discord,
            "report" => Self::Report,
            "settings" => Self::Settings,
            "help" => Self::Help,
            "version" => Self::Version,
            _ => Self::Quick(id.strip_prefix("quick:").filter(|s| !s.is_empty())?.to_string()),
        })
    }

    pub fn id(&self) -> String {
        match self {
            Self::Quick(id) => format!("quick:{id}"),
            Self::Home => "home".into(),
            Self::Save => "save".into(),
            Self::Undo => "undo".into(),
            Self::Redo => "redo".into(),
            Self::Search => "search".into(),
            Self::Assistant => "assistant".into(),
            Self::Update => "update".into(),
            Self::Donate => "donate".into(),
            Self::Discord => "discord".into(),
            Self::Report => "report".into(),
            Self::Settings => "settings".into(),
            Self::Help => "help".into(),
            Self::Version => "version".into(),
        }
    }
}

#[derive(Clone, Copy, Debug, Deserialize, Serialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum Zone {
    Start,
    End,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Placement {
    pub id: String,
    pub zone: Zone,
    pub shown: bool,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq, Eq)]
pub struct Layout {
    pub items: Vec<Placement>,
}

impl Default for Layout {
    fn default() -> Self {
        Self::default_for(std::iter::empty::<&str>())
    }
}

impl Layout {
    /// Today's actual drawing order. Update and Managed are transient, locked slots, never
    /// placements. Legacy visibility preferences remain an additional draw-time condition.
    pub fn default_for<'a>(quick: impl IntoIterator<Item = &'a str>) -> Self {
        let mut items = Vec::new();
        for id in ["home", "save", "undo", "redo"] {
            items.push(Placement { id: id.into(), zone: Zone::Start, shown: true });
        }
        for id in quick.into_iter().take(24) {
            if !valid_id(id) || id.chars().count() > MAX_ID_CHARS - "quick:".len() {
                continue;
            }
            let id = ItemId::Quick(id.to_string()).id();
            if valid_id(&id) && !items.iter().any(|p| p.id == id) {
                items.push(Placement { id, zone: Zone::Start, shown: true });
            }
        }
        for id in ["donate", "discord", "assistant", "search", "settings", "help", "report", "version"] {
            items.push(Placement { id: id.into(), zone: Zone::End, shown: true });
        }
        Self { items }
    }

    /// Keep the first occurrence and reserve room for all known items before retaining unknown
    /// ones. Missing known items go next to their default neighbours without moving saved ones.
    /// Work and output are bounded even for malformed files and ids.
    pub fn sanitize(&mut self, defaults: &Self) {
        let known: Vec<_> = defaults.items.iter().filter(|p| placement_id(&p.id)).take(MAX_ITEMS).collect();
        let mut kept: Vec<Placement> = Vec::new();
        let mut unknown = 0;
        for p in self.items.iter().take(4096) {
            if !placement_id(&p.id) || kept.iter().any(|k| k.id == p.id) {
                continue;
            }
            if !known.iter().any(|k| k.id == p.id) {
                if unknown >= MAX_ITEMS.saturating_sub(known.len()) {
                    continue;
                }
                unknown += 1;
            }
            kept.push(p.clone());
        }
        for (i, p) in known.iter().enumerate() {
            if kept.iter().any(|k| k.id == p.id) {
                continue;
            }
            let before = known.iter().skip(i + 1).find_map(|next| kept.iter().position(|k| k.id == next.id && k.zone == p.zone));
            let after = known.iter().take(i).rev().find_map(|prev| kept.iter().position(|k| k.id == prev.id && k.zone == p.zone));
            let at = before.or_else(|| after.map(|at| at + 1)).unwrap_or(kept.len());
            kept.insert(at, (*p).clone());
        }
        self.items = kept;
    }

    /// Parse entries independently: one hostile row must not discard the rest of the list.
    pub fn from_value(value: &serde_json::Value, defaults: &Self) -> Self {
        let items = value
            .get("items")
            .and_then(serde_json::Value::as_array)
            .into_iter()
            .flatten()
            .take(4096)
            .filter_map(|v| {
                let id = v.get("id")?.as_str()?;
                if !placement_id(id) {
                    return None;
                }
                let zone = match v.get("zone")?.as_str()? {
                    "start" => Zone::Start,
                    "end" => Zone::End,
                    _ => return None,
                };
                Some(Placement { id: id.into(), zone, shown: v.get("shown")?.as_bool()? })
            })
            .collect();
        let mut layout = Self { items };
        layout.sanitize(defaults);
        layout
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::json!({"items": self.items})
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().take(MAX_ID_CHARS + 1).count() <= MAX_ID_CHARS && !id.chars().any(char::is_control)
}

fn placement_id(id: &str) -> bool {
    valid_id(id) && !matches!(id, "logo" | "file" | "tabs" | "more" | "overflow" | "update" | "managed" | "leading")
}
