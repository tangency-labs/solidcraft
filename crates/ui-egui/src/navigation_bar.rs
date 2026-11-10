//! Stored navigation slots, checked against the host's slot catalogue. Painting lives in
//! `navigation_bar_view`; SolidCraft's catalogue is `navigation_slots`.
use crate::ui_kit::{NavigationEdge, NavigationIdle, NavigationKind, NavigationSize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_SLOTS: usize = 16;
pub const MAX_SEPARATORS: usize = 4;

// Reserved for the selection-mode slot in #6326; no runtime consumer yet.
#[cfg(test)]
pub(crate) const SELECTION_MODE_ICONS: &[(&str, &str)] = &[
    ("faces_edges", "select_faces_edges"),
    ("bodies", "select_bodies"),
    ("features", "select_features"),
    ("parts", "select_parts"),
    ("components", "select_components"),
    ("sketch", "select_sketch"),
];

/// The engine's style ids in index order (`ui.visual_style` 0..3), with the English menu label.
pub(crate) const VISUAL_STYLES: [(&str, &str); 4] = [
    ("shaded_with_edges", "Shaded with edges"),
    ("shaded", "Shaded"),
    ("wireframe", "Wireframe"),
    ("shaded_with_hidden_edges", "Shaded with hidden edges"),
];

pub(crate) fn visual_style_icon(style: u8) -> &'static str {
    match style {
        1 => "style_shaded",
        2 => "style_wireframe",
        3 => "style_hidden_edges",
        _ => "style_shaded_edges",
    }
}

/// One slot the bar can hold, as the host's catalogue describes it.
#[derive(Debug, PartialEq)]
pub(crate) struct Slot {
    pub id: &'static str,
    pub kind: NavigationKind,
    pub icon: &'static str,
    /// Message ids of the slot's name and of what it does.
    pub label: &'static str,
    pub description: &'static str,
    /// The command the slot runs, if any.
    pub command: Option<&'static str>,
    /// The slot works only on a canvas whose camera may orbit.
    pub needs_orbit: bool,
    /// The slot works only where what is under the pointer can be picked.
    pub needs_pick: bool,
}

/// The host's slot catalogue, installed once per process ([`install_catalogue`]).
static CATALOGUE: std::sync::OnceLock<&'static [Slot]> = std::sync::OnceLock::new();

/// Installs the host's slot catalogue: the slots a bar may hold, in the order the customiser
/// lists them. The first call installs it; a later call with the same table does nothing and one
/// with another table is ignored. Returns whether `catalogue` is the installed one.
pub(crate) fn install_catalogue(catalogue: &'static [Slot]) -> bool {
    *CATALOGUE.get_or_init(|| catalogue) == catalogue
}

/// The installed slot catalogue, or none before [`install_catalogue`].
pub(crate) fn catalogue() -> &'static [Slot] {
    CATALOGUE.get().copied().unwrap_or_default()
}

pub(crate) fn find(id: &str) -> Option<&'static Slot> {
    catalogue().iter().find(|slot| slot.id == id)
}

impl Slot {
    pub(crate) fn current_icon(&self, visual_style: u8) -> &'static str {
        if self.id == "visual_style" { visual_style_icon(visual_style) } else { self.icon }
    }

    /// Whether the slot works on the current canvas: `orbit` when its camera may orbit, `pick`
    /// when what is under the pointer can be picked.
    pub(crate) fn available(&self, orbit: bool, pick: bool) -> bool {
        (orbit || !self.needs_orbit) && (pick || !self.needs_pick)
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Position {
    #[default]
    #[serde(alias = "bottom")]
    Centre,
    #[serde(alias = "bottom_left")]
    Left,
    #[serde(alias = "bottom_right")]
    Right,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Labels {
    #[default]
    Icons,
    Labels,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MenuDisplay {
    IconAndValue,
    #[default]
    IconOnly,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
pub struct SlotOptions {
    pub shows: MenuDisplay,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct NavigationBar {
    pub slots: Vec<String>,
    pub slot_options: std::collections::BTreeMap<String, SlotOptions>,
    pub position: Position,
    pub edge: NavigationEdge,
    pub size: NavigationSize,
    pub labels: Labels,
    pub idle: NavigationIdle,
}

impl Default for NavigationBar {
    fn default() -> Self {
        Self {
            slots: ["orbit", "pan", "zoom", "separator", "fit", "look_at", "home", "separator", "visual_style", "display"]
                .into_iter()
                .map(str::to_owned)
                .collect(),
            slot_options: Default::default(),
            position: Position::default(),
            edge: NavigationEdge::default(),
            size: NavigationSize::default(),
            labels: Labels::default(),
            idle: NavigationIdle::default(),
        }
    }
}

impl<'de> Deserialize<'de> for NavigationBar {
    fn deserialize<D: serde::Deserializer<'de>>(deserializer: D) -> Result<Self, D::Error> {
        let value = Value::deserialize(deserializer)?;
        let mut bar = Self::default();
        if let Some(ids) = value.get("slots").and_then(Value::as_array) {
            let mut seen = std::collections::BTreeSet::new();
            let mut separators = 0;
            bar.slots = ids
                .iter()
                .filter_map(Value::as_str)
                .filter(|id| {
                    if *id == "separator" {
                        if separators == MAX_SEPARATORS {
                            return false;
                        }
                        separators += 1;
                        true
                    } else {
                        seen.insert(*id)
                    }
                })
                .take(MAX_SLOTS)
                .map(str::to_owned)
                .collect();
        }
        if let Some(options) = value.get("slot_options").and_then(Value::as_object) {
            for (id, option) in options.iter().take(MAX_SLOTS) {
                if id.len() <= 128 && find(id).is_some_and(|slot| slot.kind == NavigationKind::Menu) {
                    let shows = option.get("shows").cloned().and_then(|v| serde_json::from_value(v).ok()).unwrap_or_default();
                    bar.slot_options.insert(id.clone(), SlotOptions { shows });
                }
            }
        }
        if let Some(v) = value.get("position") {
            bar.position = serde_json::from_value(v.clone()).unwrap_or_default();
        }
        if let Some(v) = value.get("edge") {
            bar.edge = serde_json::from_value(v.clone()).unwrap_or_default();
        }
        if let Some(v) = value.get("size") {
            bar.size = serde_json::from_value(v.clone()).unwrap_or_default();
        }
        if let Some(v) = value.get("labels") {
            bar.labels = serde_json::from_value(v.clone()).unwrap_or_default();
        }
        if let Some(v) = value.get("idle") {
            bar.idle = serde_json::from_value(v.clone()).unwrap_or_default();
        }
        Ok(bar)
    }
}

/// Interactive callers get errors for malformed values; saved files recover field by field.
pub(crate) fn validate(value: &Value) -> Result<(), String> {
    let object = value.as_object().ok_or("`navigation_bar` must be an object")?;
    for (key, value) in object {
        let valid = match key.as_str() {
            "slots" => value.as_array().is_some_and(|ids| ids.iter().all(Value::is_string)),
            "slot_options" => value.as_object().is_some_and(|options| {
                options.len() <= MAX_SLOTS
                    && options.iter().all(|(id, option)| {
                        id.len() <= 128
                            && option.as_object().is_some_and(|fields| {
                                fields.iter().all(|(key, v)| key == "shows" && serde_json::from_value::<MenuDisplay>(v.clone()).is_ok())
                            })
                    })
            }),
            "position" => serde_json::from_value::<Position>(value.clone()).is_ok(),
            "edge" => serde_json::from_value::<NavigationEdge>(value.clone()).is_ok(),
            "size" => serde_json::from_value::<NavigationSize>(value.clone()).is_ok(),
            "labels" => serde_json::from_value::<Labels>(value.clone()).is_ok(),
            "idle" => serde_json::from_value::<NavigationIdle>(value.clone()).is_ok(),
            _ => return Err(format!("unknown navigation_bar field `{key}`")),
        };
        if !valid {
            return Err(format!("invalid navigation_bar `{key}`"));
        }
    }
    Ok(())
}

impl NavigationBar {
    pub(crate) fn menu_display(&self, id: &str) -> MenuDisplay {
        if id == "display" {
            return MenuDisplay::IconOnly;
        }
        self.slot_options
            .get(id)
            .map(|option| option.shows)
            .unwrap_or_else(|| if matches!(id, "selection_mode" | "render_mode") { MenuDisplay::IconAndValue } else { MenuDisplay::IconOnly })
    }

    /// Apply a drag only to the list it began on, including repeated separator occurrences.
    pub(crate) fn drop_slot(
        &mut self,
        original: &[String],
        from: Option<usize>,
        id: &str,
        gap: Option<usize>,
    ) -> Result<Option<usize>, &'static str> {
        if original != self.slots.as_slice() || from.is_some_and(|index| self.slots.get(index).is_none_or(|slot| slot != id)) {
            return Err("navbar.custom.stale");
        }
        match (from, gap) {
            (Some(from), Some(gap)) => self.move_to(from, gap).map(Some),
            (Some(from), None) => self.remove(from).map(|()| None),
            (None, Some(gap)) => self.insert(gap, id).map(|()| Some(gap)),
            (None, None) => Ok(None),
        }
    }

    pub(crate) fn can_insert(&self, id: &str) -> Result<(), &'static str> {
        if find(id).is_none() {
            return Err("navbar.custom.unknown");
        }
        if self.slots.len() >= MAX_SLOTS {
            return Err("navbar.custom.full");
        }
        if id == "separator" {
            if self.slots.iter().filter(|s| s.as_str() == id).count() >= MAX_SEPARATORS {
                return Err("navbar.custom.separators_full");
            }
        } else if self.slots.iter().any(|s| s == id) {
            return Err("navbar.custom.duplicate");
        }
        Ok(())
    }

    pub(crate) fn insert(&mut self, index: usize, id: &str) -> Result<(), &'static str> {
        self.can_insert(id)?;
        if index > self.slots.len() {
            return Err("navbar.custom.stale");
        }
        self.slots.insert(index, id.to_owned());
        Ok(())
    }

    pub(crate) fn remove(&mut self, index: usize) -> Result<(), &'static str> {
        if self.slots.get(index).is_none() {
            return Err("navbar.custom.stale");
        }
        self.slots.remove(index);
        Ok(())
    }

    /// Destination is a gap in the original list, including the gap after its last slot.
    pub(crate) fn move_to(&mut self, from: usize, gap: usize) -> Result<usize, &'static str> {
        if self.slots.get(from).is_none() || gap > self.slots.len() {
            return Err("navbar.custom.stale");
        }
        let to = if gap > from { gap - 1 } else { gap };
        let id = self.slots.remove(from);
        self.slots.insert(to, id);
        Ok(to)
    }
}

#[cfg(test)]
#[path = "navigation_bar_tests.rs"]
mod tests;
