//! Saved, movable application-bar items. Unknown ids survive a downgrade without being drawn.

use serde::{Deserialize, Serialize};

pub const MAX_ITEMS: usize = 64;
pub const MAX_ID_CHARS: usize = 100;

/// Stable ids, independent of the measured layout engine's slot ids.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ItemId {
    Documents,
    Separator(u64),
    Space(u64),
    File,
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
            "documents" => Self::Documents,
            "file" => Self::File,
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
            _ if id.starts_with("separator:") => Self::Separator(id.strip_prefix("separator:")?.parse().ok()?),
            _ if id.starts_with("space:") => Self::Space(id.strip_prefix("space:")?.parse().ok()?),
            _ => Self::Quick(id.strip_prefix("quick:").filter(|s| !s.is_empty())?.to_string()),
        })
    }

    pub fn id(&self) -> String {
        match self {
            Self::Documents => "documents".into(),
            Self::Separator(n) => format!("separator:{n}"),
            Self::Space(n) => format!("space:{n}"),
            Self::File => "file".into(),
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
    Custom,
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

/// Removal is a separate capability from moving. File is always reachable.
pub fn can_remove(id: &str) -> bool {
    matches!(ItemId::parse(id), Some(item) if item != ItemId::File && item != ItemId::Update)
}

pub fn drag_out(pointer_y: f32, bar_bottom: f32) -> bool {
    pointer_y.is_finite() && bar_bottom.is_finite() && pointer_y > bar_bottom
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DragRelease {
    Insert,
    Remove,
    Return,
}

/// Tray drops outside the bar return. File also returns instead of being removed.
pub fn drag_release(id: &str, from_bar: bool, below_bar: bool, over_slot: bool, cancelled: bool) -> DragRelease {
    if cancelled {
        DragRelease::Return
    } else if over_slot {
        DragRelease::Insert
    } else if from_bar && below_bar && can_remove(id) {
        DragRelease::Remove
    } else {
        DragRelease::Return
    }
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
        if !self.editable(id) || (!shown && !can_remove(id)) {
            return false;
        }
        if !shown && matches!(ItemId::parse(id), Some(ItemId::Separator(_) | ItemId::Space(_))) {
            self.items.retain(|p| p.id != id);
            return true;
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
        if preset == Preset::Custom {
            return;
        }
        self.sanitize(defaults);
        let mut used: std::collections::HashSet<_> = self.items.iter().chain(&defaults.items).map(|p| p.id.clone()).collect();
        let future: Vec<_> = self.items.iter().filter(|p| ItemId::parse(&p.id).is_none()).cloned().collect();
        *self = defaults.clone();
        for item in &mut self.items {
            item.shown = match preset {
                Preset::Default | Preset::Custom => item.shown,
                Preset::Full => true,
                Preset::Minimal => matches!(item.id.as_str(), "file" | "save" | "undo" | "redo" | "settings" | "help"),
                Preset::Compact => item.shown && !matches!(item.id.as_str(), "donate" | "discord" | "report" | "version"),
            };
        }
        self.items.extend(future);
        if matches!(preset, Preset::Minimal | Preset::Compact) {
            self.items.retain(|p| !p.id.starts_with("separator:"));
        }
        if preset == Preset::Minimal {
            let mut rows = Vec::new();
            for (zone, ids) in [
                (Zone::Start, &["space", "file", "separator", "save", "undo", "redo", "separator", "space"][..]),
                (Zone::End, &["space", "help", "settings"][..]),
            ] {
                for &kind in ids {
                    if matches!(kind, "separator" | "space") {
                        if let Some(id) = (0..=MAX_ITEMS * 2).map(|n| format!("{kind}:{n}")).find(|id| !used.contains(id)) {
                            used.insert(id.clone());
                            rows.push(Placement { id, zone, shown: true });
                        }
                    } else if let Some(item) = self.items.iter().find(|p| p.id == kind) {
                        rows.push(Placement { zone, shown: true, ..item.clone() });
                    }
                }
            }
            rows.extend(self.items.drain(..).filter(|p| !matches!(p.id.as_str(), "file" | "save" | "undo" | "redo" | "help" | "settings")));
            self.items = rows;
        }
        self.sanitize(defaults);
    }

    /// Derived selection: unknown future ids do not affect a preset match.
    pub fn preset(&self, defaults: &Self) -> Preset {
        let mut current = self.clone();
        current.sanitize(defaults);
        let known = |layout: &Self| {
            layout
                .items
                .iter()
                .filter_map(|p| {
                    let item = ItemId::parse(&p.id)?;
                    let kind = match item {
                        ItemId::Separator(_) => "separator".to_owned(),
                        ItemId::Space(_) => "space".to_owned(),
                        _ => p.id.clone(),
                    };
                    Some((kind, p.zone, p.shown))
                })
                .collect::<Vec<_>>()
        };
        for preset in [Preset::Default, Preset::Minimal, Preset::Full, Preset::Compact] {
            let mut expected = defaults.clone();
            expected.set_preset(preset, defaults);
            if known(&current) == known(&expected) {
                return preset;
            }
        }
        Preset::Custom
    }

    /// Allocate a repeatable layout object without reusing any live identity.
    pub fn add_separator(&mut self, zone: Zone, before: Option<&str>, defaults: &Self) -> Option<String> {
        self.add_object("separator", zone, before, defaults)
    }

    pub fn add_space(&mut self, zone: Zone, before: Option<&str>, defaults: &Self) -> Option<String> {
        self.add_object("space", zone, before, defaults)
    }

    /// Repeatable layout objects share validation, capacity and identity rules.
    pub fn add_object(&mut self, kind: &str, zone: Zone, before: Option<&str>, defaults: &Self) -> Option<String> {
        if !matches!(kind, "separator" | "space") {
            return None;
        }
        self.sanitize(defaults);
        if self.items.len() >= MAX_ITEMS || before.is_some_and(|id| !self.items.iter().any(|p| p.id == id && p.zone == zone && self.editable(id))) {
            return None;
        }
        let id = (0..=MAX_ITEMS).map(|n| format!("{kind}:{n}")).find(|id| !self.items.iter().any(|p| &p.id == id))?;
        self.items.push(Placement { id: id.clone(), zone, shown: true });
        self.move_before(&id, zone, before, defaults);
        Some(id)
    }

    /// Today's actual drawing order. Update and Managed are transient, locked slots, never
    /// placements. Legacy visibility preferences remain an additional draw-time condition.
    pub fn default_for<'a>(quick: impl IntoIterator<Item = &'a str>) -> Self {
        let mut items = Vec::new();
        for id in ["documents", "home", "separator:1", "file", "save", "undo", "redo"] {
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
        items.push(Placement { id: "separator:2".into(), zone: Zone::Start, shown: true });
        for id in ["donate", "discord", "assistant", "search", "settings", "help", "report", "version"] {
            items.push(Placement { id: id.into(), zone: Zone::End, shown: true });
        }
        Self { items }
    }

    /// Keep the first occurrence and reserve room for all known items before retaining unknown
    /// ones. Missing known items go next to their default neighbours without moving saved ones.
    /// Work and output are bounded even for malformed files and ids.
    pub fn sanitize(&mut self, defaults: &Self) {
        // Separator absence is intentional in current lists. Only the old-format loader
        // inserts default separators; otherwise removing one would resurrect it on save.
        let known: Vec<_> = defaults
            .items
            .iter()
            .filter(|p| placement_id(&p.id) && !p.id.starts_with("separator:") && !p.id.starts_with("space:"))
            .take(MAX_ITEMS)
            .collect();
        let mut kept: Vec<Placement> = Vec::new();
        let mut unknown = 0;
        for p in self.items.iter().take(4096) {
            if !placement_id(&p.id) {
                continue;
            }
            let mut p = p.clone();
            if p.id.starts_with("separator:") || p.id.starts_with("space:") {
                let kind = if p.id.starts_with("space:") { "space" } else { "separator" };
                if let Some(item @ (ItemId::Separator(_) | ItemId::Space(_))) = ItemId::parse(&p.id) {
                    p.id = item.id();
                }
                if !matches!(ItemId::parse(&p.id), Some(ItemId::Separator(_) | ItemId::Space(_))) || kept.iter().any(|k| k.id == p.id) {
                    let Some(id) = (0..=MAX_ITEMS).map(|n| format!("{kind}:{n}")).find(|id| !kept.iter().any(|p| &p.id == id)) else { continue };
                    p.id = id;
                }
                if !p.shown {
                    continue;
                }
            } else if kept.iter().any(|k| k.id == p.id) {
                continue;
            }
            if !known.iter().any(|k| k.id == p.id) {
                if unknown >= MAX_ITEMS.saturating_sub(known.len()) {
                    continue;
                }
                unknown += 1;
            }
            kept.push(p);
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
        if !kept.iter().any(|p| p.id == "file") {
            let at = kept.iter().position(|p| p.id == "home" && p.zone == Zone::Start).map_or(0, |i| i + 1);
            kept.insert(at, Placement { id: "file".into(), zone: Zone::Start, shown: true });
        }
        for p in &mut kept {
            if p.id == "file" {
                p.shown = true;
            }
        }
        while kept.len() > MAX_ITEMS {
            if let Some(at) = kept.iter().rposition(|p| p.id != "file") {
                kept.remove(at);
            } else {
                break;
            }
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
        // Version 1's canonical quick-access layout contained a third divider. Only
        // remove it when the entire known list still matches that old default.
        if value.get("version").and_then(serde_json::Value::as_u64) == Some(1) && defaults.items.iter().any(|p| p.id.starts_with("quick:")) {
            let mut old = defaults.clone();
            if let Some(at) = old.items.iter().position(|p| p.id.starts_with("quick:")) {
                old.items.insert(at, Placement { id: "separator:3".into(), zone: Zone::Start, shown: true });
            }
            if layout.items.iter().filter(|p| ItemId::parse(&p.id).is_some()).eq(old.items.iter()) {
                layout.items.retain(|p| p.id != "separator:3");
            }
        }
        if value.get("version").and_then(serde_json::Value::as_u64).unwrap_or(0) < 1 {
            layout.migrate_separators(defaults);
        }
        layout
    }

    fn migrate_separators(&mut self, defaults: &Self) {
        // Previously these ids could be tray-created objects. Preserve them with fresh ids
        // rather than mistaking a user's placement for a newly introduced default divider.
        let collisions: Vec<_> = self
            .items
            .iter()
            .enumerate()
            .filter(|(_, item)| defaults.items.iter().any(|p| p.id == item.id && p.id.starts_with("separator:")))
            .map(|(i, _)| i)
            .collect();
        for at in collisions {
            let id = (0..=MAX_ITEMS + 3).map(|n| format!("separator:{n}")).find(|id| !self.items.iter().chain(&defaults.items).any(|p| &p.id == id));
            if let (Some(item), Some(id)) = (self.items.get_mut(at), id) {
                item.id = id;
            }
        }
        for (i, p) in defaults.items.iter().enumerate().filter(|(_, p)| p.id.starts_with("separator:")) {
            if self.items.len() >= MAX_ITEMS
                && let Some(at) = self.items.iter().rposition(|p| !defaults.items.iter().any(|d| d.id == p.id))
            {
                self.items.remove(at);
            }
            let before = defaults.items.iter().skip(i + 1).find_map(|next| self.items.iter().position(|k| k.id == next.id && k.zone == p.zone));
            let after = defaults.items.iter().take(i).rev().find_map(|prev| self.items.iter().position(|k| k.id == prev.id && k.zone == p.zone));
            let at = before.or_else(|| after.map(|at| at + 1)).unwrap_or(self.items.len());
            self.items.insert(at, p.clone());
        }
        self.sanitize(defaults);
    }

    pub fn to_value(&self) -> serde_json::Value {
        serde_json::json!({"version": 2, "items": self.items})
    }
}

fn valid_id(id: &str) -> bool {
    !id.is_empty() && id.chars().take(MAX_ID_CHARS + 1).count() <= MAX_ID_CHARS && !id.chars().any(char::is_control)
}

fn placement_id(id: &str) -> bool {
    valid_id(id) && !matches!(id, "logo" | "tabs" | "more" | "overflow" | "update" | "managed" | "leading")
}
