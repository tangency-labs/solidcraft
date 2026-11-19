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

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Preset {
    Default,
    Minimal,
    Full,
    Compact,
}

/// A measured hit region. Locked regions reject drops rather than shifting fixed controls.
#[derive(Clone, Debug)]
pub struct DropItem {
    pub id: String,
    pub zone: Zone,
    pub left: f32,
    pub right: f32,
    pub locked: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct DropSlot {
    pub zone: Zone,
    pub before: Option<String>,
    pub x: f32,
}

/// Ignore pointer jitter before beginning a direct bar edit.
pub fn is_drag(dx: f32, dy: f32) -> bool {
    dx.is_finite() && dy.is_finite() && dx.hypot(dy) >= 5.0
}

/// Find the insertion point in physical bar order, excluding the item being dragged.
/// The centre's midpoint divides Start from End; tabs and other locked regions reject drops.
pub fn drop_slot(x: f32, centre: f32, items: &[DropItem], from: &str) -> Option<DropSlot> {
    if !x.is_finite() || !centre.is_finite() {
        return None;
    }
    let valid = |p: &&DropItem| p.left.is_finite() && p.right.is_finite() && p.right >= p.left;
    if items.iter().take(256).filter(valid).any(|p| p.locked && x >= p.left && x <= p.right) {
        return None;
    }
    let zone = if x < centre { Zone::Start } else { Zone::End };
    let mut targets: Vec<_> = items.iter().take(256).filter(valid).filter(|p| !p.locked && p.zone == zone && p.id != from).collect();
    targets.sort_by(|a, b| a.left.total_cmp(&b.left));
    let before = targets.iter().find(|p| x < p.left + (p.right - p.left) / 2.0);
    Some(DropSlot { zone, before: before.map(|p| p.id.clone()), x: before.map_or_else(|| targets.last().map_or(x, |p| p.right), |p| p.left) })
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
    /// Unknown entries are preserved for newer versions, but cannot be edited by this one.
    pub fn editable(&self, id: &str) -> bool {
        matches!(ItemId::parse(id), Some(item) if item != ItemId::Update) && self.items.iter().any(|p| p.id == id)
    }

    /// Move before an item in the destination zone, or to that zone's end. Validation happens
    /// before removal, so stale menu targets and locked/unknown ids are harmless.
    pub fn move_before(&mut self, id: &str, zone: Zone, before: Option<&str>, defaults: &Self) -> bool {
        self.sanitize(defaults);
        if !self.editable(id)
            || before == Some(id)
            || before.is_some_and(|b| !self.editable(b) || !self.items.iter().any(|p| p.id == b && p.zone == zone))
        {
            return false;
        }
        let old = self.clone();
        if let Some(at) = self.items.iter().position(|p| p.id == id) {
            let mut item = self.items.remove(at);
            item.zone = zone;
            let at = before
                .and_then(|b| self.items.iter().position(|p| p.id == b))
                .or_else(|| self.items.iter().rposition(|p| p.zone == zone).map(|i| i + 1))
                .unwrap_or(self.items.len());
            self.items.insert(at, item);
        }
        *self != old
    }

    pub fn move_after(&mut self, id: &str, after: &str, defaults: &Self) -> bool {
        self.sanitize(defaults);
        if id == after || !self.editable(after) {
            return false;
        }
        let Some(target) = self.items.iter().find(|p| p.id == after) else { return false };
        let zone = target.zone;
        let before = self
            .items
            .iter()
            .skip_while(|p| p.id != after)
            .skip(1)
            .find(|p| p.zone == zone && p.id != id && self.editable(&p.id))
            .map(|p| p.id.clone());
        self.move_before(id, zone, before.as_deref(), defaults)
    }

    /// Move one visible neighbour in the same zone (left/up or right/down).
    pub fn move_step(&mut self, id: &str, forward: bool, defaults: &Self) -> bool {
        self.sanitize(defaults);
        let Some(item) = self.items.iter().find(|p| p.id == id && self.editable(id)) else { return false };
        let rows: Vec<_> =
            self.items.iter().filter(|p| p.zone == item.zone && p.shown == item.shown && self.editable(&p.id)).map(|p| p.id.clone()).collect();
        let Some(at) = rows.iter().position(|p| p == id) else { return false };
        let neighbour = if forward { rows.get(at + 1) } else { at.checked_sub(1).and_then(|i| rows.get(i)) };
        let Some(neighbour) = neighbour else { return false };
        if forward { self.move_after(id, neighbour, defaults) } else { self.move_before(id, item.zone, Some(neighbour), defaults) }
    }

    pub fn set_shown(&mut self, id: &str, shown: bool, defaults: &Self) -> bool {
        self.sanitize(defaults);
        if !self.editable(id) {
            return false;
        }
        if let Some(item) = self.items.iter_mut().find(|p| p.id == id) {
            let changed = item.shown != shown;
            item.shown = shown;
            return changed;
        }
        false
    }

    /// Presets only edit the saved list. Unknown future entries keep their state.
    pub fn set_preset(&mut self, preset: Preset, defaults: &Self) {
        self.sanitize(defaults);
        let future: Vec<_> = self.items.iter().filter(|p| !defaults.items.iter().any(|d| d.id == p.id)).cloned().collect();
        *self = defaults.clone();
        for item in &mut self.items {
            item.shown = match preset {
                Preset::Default => item.shown,
                Preset::Full => true,
                Preset::Minimal => matches!(item.id.as_str(), "save" | "undo" | "redo" | "search" | "settings" | "help"),
                Preset::Compact => item.shown && !matches!(item.id.as_str(), "donate" | "discord" | "report" | "version"),
            };
        }
        self.items.extend(future);
        self.sanitize(defaults);
    }

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
