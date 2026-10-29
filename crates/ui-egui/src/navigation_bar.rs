//! Stored navigation slots and their catalogue. Painting lives in `navigation_bar_view`.
use crate::{SolveApp, navigation::NavCaps};
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

pub(crate) fn visual_style_icon(style: u8) -> &'static str {
    match style {
        1 => "style_shaded",
        2 => "style_wireframe",
        3 => "style_hidden_edges",
        _ => "style_shaded_edges",
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Kind {
    Action,
    Tool,
    Toggle,
    Menu,
    Separator,
}

pub(crate) struct Slot {
    pub id: &'static str,
    pub kind: Kind,
    pub icon: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub command: Option<&'static str>,
}

macro_rules! slot {
    ($id:literal, $kind:ident, $icon:literal, $command:expr) => {
        Slot {
            id: $id,
            kind: Kind::$kind,
            icon: $icon,
            label: concat!("navbar.", $id),
            description: concat!("navbar.description.", $id),
            command: $command,
        }
    };
}

pub(crate) const CATALOGUE: &[Slot] = &[
    slot!("orbit", Tool, "orbit", Some("view.tool")),
    slot!("pan", Tool, "pan", Some("view.tool")),
    slot!("zoom", Tool, "zoom", Some("view.tool")),
    slot!("spin", Toggle, "spin", Some("view.spin")),
    slot!("fit", Action, "fit", Some("view.fit")),
    slot!("look_at", Action, "lookat", Some("view.normal_to")),
    slot!("home", Action, "home", Some("view.home")),
    slot!("visual_style", Menu, "style_shaded_edges", Some("view.visual_style")),
    slot!("projection", Toggle, "perspective", None),
    slot!("display", Menu, "display", None),
    slot!("grid", Toggle, "grid", Some("view.grid")),
    slot!("ground_shadow", Toggle, "ground_shadow", Some("view.ground_shadow")),
    slot!("ambient_occlusion", Toggle, "ambient_occlusion", Some("view.ambient_occlusion")),
    slot!("separator", Separator, "", None),
];

pub(crate) fn find(id: &str) -> Option<&'static Slot> {
    CATALOGUE.iter().find(|slot| slot.id == id)
}

impl Slot {
    pub(crate) fn current_icon(&self, visual_style: u8) -> &'static str {
        if self.id == "visual_style" { visual_style_icon(visual_style) } else { self.icon }
    }

    pub(crate) fn available(&self, caps: NavCaps) -> bool {
        if !caps.orbit {
            return !matches!(self.id, "orbit" | "spin" | "look_at" | "projection" | "visual_style");
        }
        if !caps.pick && matches!(self.id, "look_at" | "visual_style") {
            return false;
        }
        true
    }
}

pub(crate) fn canvas_caps(app: &SolveApp) -> NavCaps {
    if crate::dialogs_sheet::flat_active(app) || crate::render_workspace::result_active(app) {
        NavCaps { orbit: false, pick: false }
    } else if crate::render_workspace::active(app) {
        NavCaps::RENDER
    } else {
        NavCaps::MODEL
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

/// The edge of the 3D view the bar sits on (#8183): the bottom, or the top, under the ribbon.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Edge {
    #[default]
    Bottom,
    Top,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Size {
    Compact,
    #[default]
    Regular,
}

impl Size {
    // Logical points: egui's UI zoom applies the preference's scale to these dimensions.
    pub(crate) fn points(self) -> f32 {
        crate::ui_kit::NavigationMetrics::for_size(self).button
    }
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
pub enum Idle {
    Always,
    #[default]
    Fade,
    Hide,
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
    pub edge: Edge,
    pub size: Size,
    pub labels: Labels,
    pub idle: Idle,
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
            edge: Edge::default(),
            size: Size::default(),
            labels: Labels::default(),
            idle: Idle::default(),
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
                if id.len() <= 128 && find(id).is_some_and(|slot| slot.kind == Kind::Menu) {
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
            "edge" => serde_json::from_value::<Edge>(value.clone()).is_ok(),
            "size" => serde_json::from_value::<Size>(value.clone()).is_ok(),
            "labels" => serde_json::from_value::<Labels>(value.clone()).is_ok(),
            "idle" => serde_json::from_value::<Idle>(value.clone()).is_ok(),
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
