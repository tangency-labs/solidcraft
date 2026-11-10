//! Stored navigation slots, checked against the host's slot catalogue. Their layout is
//! [`crate::nav_bar_layout`] and the bar component [`crate::nav_bar`]; SolidCraft's catalogue is
//! `navigation_slots`.
use crate::{NavigationEdge, NavigationIdle, NavigationKind, NavigationSize};
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub const MAX_SLOTS: usize = 16;
pub const MAX_SEPARATORS: usize = 4;

/// One slot the bar can hold, as the host's catalogue describes it.
#[derive(Debug, PartialEq)]
pub struct Slot {
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
    /// The icons a menu slot shows by its current value (`value_icons[value]`); `icon` for a
    /// value past the end and for a slot without any.
    pub value_icons: &'static [&'static str],
}

/// The host's navigation bar: the slots a bar may hold and how a new bar looks.
#[derive(Debug, PartialEq)]
pub struct Catalogue {
    /// The slots, in the order the customiser lists them.
    pub slots: &'static [Slot],
    /// The slot ids a new bar holds, in order.
    pub default_bar: &'static [&'static str],
    /// Menu slots that show their value beside the icon until the user chooses otherwise.
    pub value_menus: &'static [&'static str],
    /// Menu slots that show only their icon, whatever the stored options say.
    pub icon_menus: &'static [&'static str],
}

/// The host's catalogue, installed once per process ([`install_catalogue`]).
static CATALOGUE: std::sync::OnceLock<&'static Catalogue> = std::sync::OnceLock::new();

/// Installs the host's catalogue. The first call installs it; a later call with the same table
/// does nothing and one with another table is ignored. Returns whether `catalogue` is the
/// installed one.
pub fn install_catalogue(catalogue: &'static Catalogue) -> bool {
    *CATALOGUE.get_or_init(|| catalogue) == catalogue
}

fn installed() -> Option<&'static Catalogue> {
    CATALOGUE.get().copied()
}

/// The installed slots, or none before [`install_catalogue`].
pub fn catalogue() -> &'static [Slot] {
    installed().map_or(&[], |catalogue| catalogue.slots)
}

pub fn find(id: &str) -> Option<&'static Slot> {
    catalogue().iter().find(|slot| slot.id == id)
}

impl Slot {
    /// The icon the slot shows while its menu's current value is `value`.
    pub fn current_icon(&self, value: u8) -> &'static str {
        self.value_icons.get(usize::from(value)).copied().unwrap_or(self.icon)
    }

    /// Whether the slot works on the current canvas: `orbit` when its camera may orbit, `pick`
    /// when what is under the pointer can be picked.
    pub fn available(&self, orbit: bool, pick: bool) -> bool {
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
            slots: installed().map_or(&[][..], |catalogue| catalogue.default_bar).iter().map(|id| (*id).to_owned()).collect(),
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
pub fn validate(value: &Value) -> Result<(), String> {
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
    /// What the menu slot `id` shows: the stored choice, else the host's default for it.
    pub fn menu_display(&self, id: &str) -> MenuDisplay {
        let listed = |menus: fn(&Catalogue) -> &'static [&'static str]| installed().is_some_and(|catalogue| menus(catalogue).contains(&id));
        if listed(|catalogue| catalogue.icon_menus) {
            return MenuDisplay::IconOnly;
        }
        self.slot_options
            .get(id)
            .map(|option| option.shows)
            .unwrap_or_else(|| if listed(|catalogue| catalogue.value_menus) { MenuDisplay::IconAndValue } else { MenuDisplay::IconOnly })
    }

    /// Apply a drag only to the list it began on, including repeated separator occurrences.
    pub fn drop_slot(&mut self, original: &[String], from: Option<usize>, id: &str, gap: Option<usize>) -> Result<Option<usize>, &'static str> {
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

    pub fn can_insert(&self, id: &str) -> Result<(), &'static str> {
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

    pub fn insert(&mut self, index: usize, id: &str) -> Result<(), &'static str> {
        self.can_insert(id)?;
        if index > self.slots.len() {
            return Err("navbar.custom.stale");
        }
        self.slots.insert(index, id.to_owned());
        Ok(())
    }

    pub fn remove(&mut self, index: usize) -> Result<(), &'static str> {
        if self.slots.get(index).is_none() {
            return Err("navbar.custom.stale");
        }
        self.slots.remove(index);
        Ok(())
    }

    /// Destination is a gap in the original list, including the gap after its last slot.
    pub fn move_to(&mut self, from: usize, gap: usize) -> Result<usize, &'static str> {
        if self.slots.get(from).is_none() || gap > self.slots.len() {
            return Err("navbar.custom.stale");
        }
        let to = if gap > from { gap - 1 } else { gap };
        let id = self.slots.remove(from);
        self.slots.insert(to, id);
        Ok(to)
    }
}
