//! Custom tool orders for the existing eight-slot context ring, independent of mouse gestures.
use crate::SolveApp;
use crate::ui_kit::{self, BackingTooltipExt, Item};
use serde::{Deserialize, Serialize};

pub(crate) const MAX_TOOLS: usize = 8;
const MODEL: &[&str] = &["repeat", "delete", "solid.press_pull", "solid.hole", "sketch.create", "solid.extrude", "undo", "solid.move"];
const SKETCH: &[&str] =
    &["repeat", "delete", "sketch.line", "sketch.rectangle.two_point", "ui.finishSketch", "sketch.circle.center", "undo", "sketch.dimension"];

/// Missing lists use the shipped order; an explicit empty list removes every tool.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub model: Option<Vec<String>>,
    pub sketch: Option<Vec<String>>,
}

impl Config {
    pub(crate) fn tools(&self, sketch: bool) -> Vec<String> {
        let custom = if sketch { &self.sketch } else { &self.model };
        let Some(custom) = custom else {
            return if sketch { SKETCH } else { MODEL }.iter().map(|id| (*id).to_string()).collect();
        };
        let mut tools = Vec::new();
        for id in custom.iter().take(256) {
            if tools.len() == MAX_TOOLS {
                break;
            }
            if crate::mouse_gesture::label_of(id).is_some() && !tools.contains(id) {
                tools.push(id.clone());
            }
        }
        tools
    }

    pub(crate) fn set_tools(&mut self, sketch: bool, tools: Vec<String>) {
        let list = if sketch { &mut self.sketch } else { &mut self.model };
        *list = Some(tools);
        let cleaned = self.tools(sketch);
        *if sketch { &mut self.sketch } else { &mut self.model } = Some(cleaned);
    }

    pub(crate) fn reset(&mut self, sketch: bool) {
        *if sketch { &mut self.sketch } else { &mut self.model } = None;
    }
}

pub(crate) fn item(app: &SolveApp, id: &str) -> Option<Item> {
    let mut item = crate::mouse_gesture::item(app, id)?;
    if id == "repeat" {
        item.shortcut = crate::keymap::display_for(app, "edit.repeat").unwrap_or_default();
    }
    if id == "ui.workspace.design" {
        item.params = Some(serde_json::json!({"workspace":"design"}));
    }
    if id == "ui.finishSketch" {
        item.enabled = app.session.active_sketch.is_some();
    }
    // Keep the existing ring's captions for its shipped tools.
    let caption = match id {
        "solid.press_pull" => Some("Press Pull"),
        "solid.hole" => Some("Hole"),
        "sketch.create" => Some("Create Sketch"),
        "solid.extrude" => Some("Extrude"),
        "solid.move" => Some("Move/Copy"),
        "sketch.line" => Some("Line"),
        "sketch.rectangle.two_point" => Some("Rectangle"),
        "sketch.circle.center" => Some("Circle"),
        "sketch.dimension" => Some("Sketch Dimension"),
        _ => None,
    };
    if let Some(caption) = caption {
        item.label = caption.into();
    }
    Some(item)
}

pub(crate) fn show_editor(app: &mut SolveApp, ctx: &egui::Context) {
    if !app.prefs_window.shell.open {
        app.menu.radial_editor = false;
    }
    if !app.menu.radial_editor {
        return;
    }
    let mut sketch = app.menu.radial_editor_sketch;
    let mut config = app.preferences.radial_tools.clone();
    let title = app.tr("radial.title").to_owned();
    let tokens = solidcraft_theme::Tokens::get();
    let closed = ui_kit::Window::new(egui::Id::unique("sc_radial_editor"), "settings", &title).cards(3.0).show_body(ctx, |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(app.tr("radial.context"));
            let mut selected = usize::from(sketch);
            ui_kit::segmented(
                ui,
                &mut selected,
                &[app.tr("radial.model"), app.tr("radial.sketch")],
                ui_kit::Segments { intrinsic: true, targets: &["radial:context:model", "radial:context:sketch"], ..Default::default() },
            );
            sketch = selected == 1;
        });
        ui.label(app.tr("radial.limit"));
        let restore = ui_kit::button(ui, app.tr("radial.restore"), ui_kit::Button::Normal);
        crate::scenario::publish_target("radial:restore", restore.rect);
        if restore.clicked() {
            config.reset(sketch);
        }
        ui.separator();
        ui.horizontal(|ui| {
            ui.label(app.tr("radial.tools"));
            ui.label(format!("{}/{}", config.tools(sketch).len(), MAX_TOOLS));
        });
        let mut list = config.tools(sketch);
        let mut changed = false;
        let mut remove = None;
        let mut swap = None;
        let catalog = crate::mouse_gesture::catalog();
        ui.horizontal_wrapped(|ui| {
            ui.vertical(|ui| {
                ui.set_width(tokens.space_24 * 16.0);

                if list.is_empty() {
                    ui.label(app.tr("radial.empty"));
                }
                for index in 0..list.len() {
                    ui.push_id(index, |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(format!("{}", index + 1));
                            let current = list.get(index).cloned().unwrap_or_default();
                            let label = crate::mouse_gesture::label_of(&current).unwrap_or_default();
                            ui.scope(|ui| {
                                ui.set_max_width(tokens.space_24 * 9.0 + tokens.space_4);
                                let picker = ui_kit::dropdown_menu_with_close_behavior(
                                    ui,
                                    ("radial_tool", index),
                                    solidcraft_ui_text::tr_label(&label),
                                    egui::PopupCloseBehavior::CloseOnClickOutside,
                                    |ui| {
                                        let filter_id = ui.scope_id().with("filter");
                                        let mut filter: String = ui.data_mut(|d| d.get_temp(filter_id).unwrap_or_default());
                                        ui_kit::text_field(ui, &mut filter, app.tr("radial.filter"));
                                        ui.data_mut(|d| d.insert_temp(filter_id, filter.clone()));
                                        let needle = filter.trim().to_lowercase();

                                        egui::ScrollArea::vertical().max_height(tokens.space_24 * 10.0 + tokens.space_16 + tokens.space_4).show(
                                            ui,
                                            |ui| {
                                                for (id, name) in &catalog {
                                                    if !needle.is_empty()
                                                        && !solidcraft_ui_text::tr_label(name).to_lowercase().contains(&needle)
                                                        && !id.to_lowercase().contains(&needle)
                                                    {
                                                        continue;
                                                    }
                                                    let taken = list.contains(id) && *id != current;
                                                    let response = ui
                                                        .add_enabled_ui(!taken, |ui| {
                                                            ui_kit::list_picker(ui, id, solidcraft_ui_text::tr_label(name), *id == current)
                                                        })
                                                        .inner;
                                                    crate::scenario::publish_target(&format!("radial:choose:{index}:{id}"), response.rect);
                                                    if response.clicked() {
                                                        if let Some(slot) = list.get_mut(index) {
                                                            *slot = id.clone();
                                                            changed = true;
                                                        }
                                                        ui.close();
                                                    }
                                                }
                                            },
                                        );
                                    },
                                );
                                crate::scenario::publish_target(&format!("radial:tool:{index}"), picker.rect);
                            });
                            for (key, glyph, enabled, next) in
                                [("radial.up", "↑", index > 0, index.saturating_sub(1)), ("radial.down", "↓", index + 1 < list.len(), index + 1)]
                            {
                                let response = ui_kit::button(ui, glyph, ui_kit::Button::Enabled(enabled)).backing_on_hover_text(app.tr(key));
                                crate::scenario::publish_target(&format!("{key}:{index}"), response.rect);
                                if response.clicked() {
                                    swap = Some((index, next));
                                }
                            }
                            let response = ui_kit::button(ui, "×", ui_kit::Button::Normal).backing_on_hover_text(app.tr("radial.remove"));
                            crate::scenario::publish_target(&format!("radial:remove:{index}"), response.rect);
                            if response.clicked() {
                                remove = Some(index);
                            }
                        });
                    });
                }
                if let Some(index) = remove.filter(|index| *index < list.len()) {
                    list.remove(index);
                    changed = true;
                }
                if let Some((from, to)) = swap.filter(|(from, to)| *from < list.len() && *to < list.len()) {
                    list.swap(from, to);
                    changed = true;
                }
                let add = ui_kit::button(ui, app.tr("radial.add"), ui_kit::Button::Enabled(list.len() < MAX_TOOLS));
                crate::scenario::publish_target("radial:add", add.rect);
                if add.clicked()
                    && list.len() < MAX_TOOLS
                    && let Some((id, _)) = catalog.iter().find(|(id, _)| !list.contains(id))
                {
                    list.push(id.clone());
                    changed = true;
                }
                if changed {
                    config.set_tools(sketch, list.clone());
                }
            });
            let items: Vec<_> = config.tools(sketch).iter().filter_map(|id| item(app, id)).collect();
            let size = egui::Vec2::splat(ui_kit::RADIAL_RADIUS * 2.0 + tokens.space_8);
            let (rect, _) = ui.allocate_exact_size(size, egui::Sense::hover());
            crate::scenario::publish_target("radial:preview", rect);
            ui_kit::radial_menu_preview(ui, rect.center(), &items);
        });
    });
    // Keep this child above Preferences, including when its previous position is restored.
    ctx.move_to_top(egui::LayerId::new(egui::Order::Middle, egui::Id::unique("sc_radial_editor")));
    app.preferences.radial_tools = config;
    app.menu.radial_editor_sketch = sketch;
    app.menu.radial_editor = !closed;
}
