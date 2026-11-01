//! Navigation bar editor. The shared viewport painter owns the preview's appearance.
use crate::navigation::NavCaps;
use crate::navigation_bar::{self, Edge, Idle, Kind, Labels, MenuDisplay, NavigationBar, Position, Size, SlotOptions};
use crate::navigation_bar_view;
use crate::ui_kit::BackingTooltipExt as _;
use crate::{SolveApp, scenario, theme::Tokens, ui_kit};
use egui::{Rect, Sense, vec2};
use serde_json::json;

#[derive(Default)]
pub(super) struct State {
    selected: Option<usize>,
    render: bool,
    notice: Option<&'static str>,
    slots: Vec<String>,
}

#[derive(Clone, Debug)]
struct Source {
    slot: String,
    index: Option<usize>,
    slots: Vec<String>,
}
type Drag = ui_kit::ReorderPayload<Source>;

pub(super) fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    // The bar editor is drawn by hand, not through the kit rows the search hides, so while a
    // search runs it stays out unless the section's own title or a setting's label matches.
    if crate::prefs_search::querying(ui) {
        let labels: Vec<String> = ["title", "preview_as", "position", "size", "buttons", "idle", "show", "reset_default"]
            .iter()
            .map(|key| app.tr(&format!("navbar.custom.{key}")).to_owned())
            .collect();
        let texts: Vec<&str> = labels.iter().map(String::as_str).collect();
        if !crate::prefs_search::texts_hit(ui, &texts) {
            return;
        }
    }
    let scroll = app.prefs_window.navbar_scroll;
    let mut state = std::mem::take(&mut app.prefs_window.navbar);
    if state.slots != app.preferences.navigation_bar.slots {
        state.selected = None;
    }
    let section = ui.scope(|ui| {
        if scroll {
            let id = ui.make_persistent_id("navigation_bar");
            ui.ctx().data_mut(|data| data.insert_persisted(id, true));
        }
        ui_kit::settings_section(ui, "navigation_bar", crate::i18n::tr("navbar.custom.title"), |ui| {
            let width = ui.available_width();
            ui_kit::text(ui, app.tr("navbar.custom.intro"), ui_kit::Text::Hint("navbar_intro"));
            segments(ui, "preview_as", &mut usize::from(state.render), &["navbar.custom.design", "navbar.custom.render"], |i| state.render = i == 1);
            preview(app, ui, &mut state, width);
            let card = ui_kit::settings_card(ui, |ui| inspector(app, ui, &mut state));
            scenario::publish_rect("prefs:navbar:inspector", card.response.rect);
            available(app, ui, &mut state);
            ui_kit::divider(ui, "navbar_appearance");
            appearance(app, ui);
            ui_kit::settings_inline_note(
                ui,
                &crate::i18n::format("navbar.custom.count", &[("count", &app.preferences.navigation_bar.slots.len().to_string())]),
            );
            if let Some(message) =
                state.notice.or_else(|| (app.preferences.navigation_bar.slots.len() >= navigation_bar::MAX_SLOTS).then_some("navbar.custom.full"))
            {
                ui.label(egui::RichText::new(app.tr(message)).color(Tokens::get().warning));
            }
            footer(app, ui, &mut state);
        });
        if let Some(rect) = scenario::rect_of(&format!("dialog:section:{}", app.tr("navbar.custom.title"))) {
            scenario::publish_rect("prefs:section:navigation_bar", rect);
        }
    });
    if scroll {
        ui.scroll_to_rect(Rect::from_min_size(section.response.rect.min, vec2(1.0, 1.0)), Some(egui::Align::Min));
        app.prefs_window.navbar_scroll = false;
    }
    state.slots = app.preferences.navigation_bar.slots.clone();
    app.prefs_window.navbar = state;
}

fn segments(ui: &mut egui::Ui, key: &str, selected: &mut usize, options: &[&str], mut change: impl FnMut(usize)) {
    let label_key = format!("navbar.custom.{key}");
    let label = crate::i18n::tr(&label_key);
    let labels: Vec<_> = options.iter().map(|key| crate::i18n::tr(key)).collect();
    ui_kit::row(ui, label, |ui| {
        if ui_kit::segmented(ui, selected, &labels, ui_kit::Segments { intrinsic: true, ..Default::default() }) {
            change(*selected);
        }
        if let Some(rect) = scenario::rect_of(&format!("dialog:segments:{label}")) {
            scenario::publish_target(&format!("prefs:navbar:{key}"), rect);
        } else if let Some(rect) = scenario::rect_of(&format!("dialog:widget:{label}")) {
            scenario::publish_target(&format!("prefs:navbar:{key}"), rect);
        }
        for (index, option) in labels.iter().enumerate() {
            if let Some(rect) = scenario::rect_of(&format!("dialog:{label}:{option}")) {
                scenario::publish_target(&format!("prefs:navbar:{key}:{index}"), rect);
            }
        }
    });
}

fn footer(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State) {
    let t = Tokens::get();
    let label = app.tr("navbar.custom.reset_default");
    let width = ui_kit::ContentWidth::button(ui, label).max(t.control_height);
    let (rect, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), Sense::hover());
    let reset = Rect::from_min_max(egui::pos2((rect.right() - width).max(rect.left()), rect.top()), rect.max);
    let check = Rect::from_min_max(rect.min, egui::pos2((reset.left() - t.space_8).max(rect.left()), rect.bottom()));
    ui.scope_builder(egui::UiBuilder::new().max_rect(check).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        let mut visible = app.ui.show_navigation_bar;
        let response = ui_kit::checkbox(ui, app.tr("navbar.custom.show"), &mut visible);
        scenario::publish_target("prefs:navbar:show", response.rect);
        if response.changed() {
            let _ = app.run("view.navigation_bar", json!({"visible":visible}));
        }
    });
    let response = ui_kit::button_at(ui, "navbar_reset", reset, label, ui_kit::Button::Normal);
    scenario::publish_target("prefs:navbar:reset", response.rect);
    scenario::publish_rect("prefs:navbar:status", rect);
    if response.clicked() {
        app.preferences.navigation_bar = NavigationBar::default();
        let _ = app.run("view.navigation_bar", json!({"visible":true}));
        state.selected = None;
        state.notice = None;
    }
}

fn preview(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State, width: f32) {
    let t = Tokens::get();
    let stage = ui_kit::navigation_stage(ui, width);
    scenario::publish_target("prefs:navbar:stage", stage);
    scenario::publish_target("prefs:navbar:preview", stage);
    let bar = navigation_bar_view::editor_layout(stage, &app.preferences.navigation_bar, |slot| {
        ui.painter().layout_no_wrap(navigation_bar_view::slot_text(app, slot).to_owned(), crate::fonts::medium(ui_kit::NAV_TEXT), t.form_ink).size().x
    });
    scenario::publish_rect("prefs:navbar:bar", bar.rect);
    let mut cells = Vec::new();
    let caps = if state.render { NavCaps::RENDER } else { NavCaps::MODEL };
    {
        let mut preview_ui = ui.new_child(egui::UiBuilder::new().id_salt("navigation_stage_content").max_rect(stage));
        let ui = &mut preview_ui;
        ui.set_clip_rect(ui.clip_rect().intersect(stage.shrink(ui_kit::NAV_BORDER)));
        for (index, slot, response) in navigation_bar_view::paint_row(app, ui, &bar, "", Some((state.selected, caps, state.render))) {
            cells.push((index, response.rect));
            scenario::publish_target(&format!("prefs:navbar:slot:{}", slot.id), response.rect);
            scenario::publish_target(&format!("prefs:navbar:slot:{index}"), response.rect);
            if response.clicked() {
                state.selected = Some(index);
            }
            if response.drag_started() {
                response.dnd_set_drag_payload(Drag {
                    id: Source { slot: slot.id.to_owned(), index: Some(index), slots: app.preferences.navigation_bar.slots.clone() },
                    from_menu: false,
                });
            }
            ui_kit::navigation_context(&response, |ui| {
                for (key, enabled) in [("left", index > 0), ("right", index + 1 < app.preferences.navigation_bar.slots.len()), ("remove", true)] {
                    let label = format!("navbar.custom.{key}");
                    let mut item = ui_kit::NavigationMenuRow::new(key, app.tr(&label), "");
                    item.enabled = enabled;
                    if item.show(ui).clicked() {
                        edit(app, state, index, key);
                        ui.close();
                    }
                }
            });
        }
    }
    let target = if egui::DragAndDrop::payload::<Drag>(ui.ctx()).is_some_and(|payload| payload.id.index.is_some()) {
        bar.rect.intersect(stage)
    } else {
        stage
    };
    drop_on_preview(app, ui, state, target.intersect(ui.clip_rect()), &cells);
}

fn drop_on_preview(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State, target: Rect, cells: &[(usize, Rect)]) {
    let Some(payload) = egui::DragAndDrop::payload::<Drag>(ui.ctx()) else { return };
    let Some(pointer) = ui.input(|i| i.pointer.interact_pos()) else { return };
    let over = target.contains(pointer) && ui.rect_contains_pointer(target);
    let gap = ui_kit::drop_index(pointer.x, cells.iter().map(|(_, rect)| rect.center().x));
    let index = cells.get(gap).map(|(index, _)| *index).unwrap_or(app.preferences.navigation_bar.slots.len());
    if over {
        let x = cells
            .get(gap)
            .map(|(_, rect)| rect.left() - ui_kit::NAV_GAP / 2.0)
            .or_else(|| cells.last().map(|(_, rect)| rect.right() + ui_kit::NAV_GAP / 2.0))
            .unwrap_or(target.left() + Tokens::get().space_4);
        let marker = ui_kit::insertion_marker_with_ink(
            ui.painter(),
            x,
            target.center().y - ui_kit::NAV_MARKER_HEIGHT / 2.0,
            target.center().y + ui_kit::NAV_MARKER_HEIGHT / 2.0,
            Tokens::get().form_primary,
        );
        scenario::publish_target("prefs:navbar:marker", marker);
    }
    if !ui.input(|i| i.pointer.any_released()) {
        if let Some(slot) = navigation_bar::find(&payload.id.slot) {
            ui_kit::navigation_drag_ghost(
                ui,
                pointer,
                ui_kit::NavigationButton {
                    id: slot.id,
                    label: app.tr(slot.label),
                    icon: slot.current_icon(app.ui.visual_style),
                    selected: false,
                    kind: slot.kind,
                    labels: false,
                    menu: slot.kind == Kind::Menu,
                },
                app.preferences.navigation_bar.size,
            );
        }
        return;
    }
    egui::DragAndDrop::clear_payload(ui.ctx());
    let result = app.preferences.navigation_bar.drop_slot(&payload.id.slots, payload.id.index, &payload.id.slot, over.then_some(index));
    match result {
        Ok(selected) if !payload.from_menu || over => state.selected = selected,
        _ => {}
    }
    state.notice = result.err();
}

fn edit(app: &mut SolveApp, state: &mut State, index: usize, action: &str) {
    let bar = &mut app.preferences.navigation_bar;
    let result = match action {
        "left" if index > 0 => bar.move_to(index, index - 1).map(|to| state.selected = Some(to)),
        "right" => bar.move_to(index, index.saturating_add(2)).map(|to| state.selected = Some(to)),
        "remove" => bar.remove(index).map(|()| state.selected = None),
        _ => Ok(()),
    };
    state.notice = result.err();
}

fn inspector(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State) {
    let Some((index, slot)) = state
        .selected
        .and_then(|index| app.preferences.navigation_bar.slots.get(index).and_then(|id| navigation_bar::find(id)).map(|slot| (index, slot)))
    else {
        ui_kit::text(ui, app.tr("navbar.custom.select"), ui_kit::Text::Hint("navbar_select"));
        return;
    };
    inspector_header(app, ui, state, index, slot);
    ui_kit::text(ui, app.tr(slot.description), ui_kit::Text::Hint("navbar_description"));
    if slot.id == "visual_style" {
        let mut selected = usize::from(app.preferences.navigation_bar.menu_display(slot.id) == MenuDisplay::IconOnly);
        segments(ui, "shows", &mut selected, &["navbar.custom.icon_value", "navbar.custom.icon_only"], |index| {
            app.preferences
                .navigation_bar
                .slot_options
                .insert(slot.id.to_owned(), SlotOptions { shows: if index == 0 { MenuDisplay::IconAndValue } else { MenuDisplay::IconOnly } });
        });
    }
}

/// Keep the arrow actions fixed; truncate the name before collapsing Remove to its icon.
pub(super) fn inspector_header_layout(bounds: Rect, remove_width: f32, t: &Tokens) -> (Rect, [Rect; 3], bool) {
    let gap = t.space_4.min(bounds.width().max(0.0) / 2.0);
    let minimum_name = t.form_icon_size + t.space_8 + t.space_24;
    let widths = [t.control_height, t.control_height, remove_width];
    let compact = bounds.width() < widths.iter().sum::<f32>() + gap * 3.0 + minimum_name;
    let widths = if compact { [t.control_height.min(((bounds.width() - gap * 2.0) / 3.0).max(0.0)); 3] } else { widths };
    let total = widths.iter().sum::<f32>() + gap * 2.0;
    let start = (bounds.right() - total).max(bounds.left());
    let name = Rect::from_min_max(bounds.min, egui::pos2((start - gap).max(bounds.left()), bounds.bottom()));
    let mut x = start;
    let buttons = widths.map(|width| {
        let rect = Rect::from_min_size(egui::pos2(x, bounds.top()), vec2(width, bounds.height()));
        x += width + gap;
        rect
    });
    (name, buttons, compact)
}

fn inspector_header(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State, index: usize, slot: &navigation_bar::Slot) {
    let t = Tokens::get();
    let remove = app.tr_label(app.tr("navbar.custom.remove"));
    let remove_width = ui.painter().layout_no_wrap(remove.to_owned(), egui::FontId::proportional(t.font_label), t.form_ink).size().x.ceil()
        + t.space_10 * 2.0
        + t.space_14
        + t.space_6;
    let (bounds, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.control_height), Sense::hover());
    let (name, buttons, compact) = inspector_header_layout(bounds, remove_width, &t);
    ui.scope_builder(egui::UiBuilder::new().max_rect(name).layout(egui::Layout::left_to_right(egui::Align::Center)), |ui| {
        if !slot.icon.is_empty() && name.width() >= t.form_icon_size {
            let (icon, _) = ui.allocate_exact_size(vec2(t.form_icon_size, t.form_icon_size), Sense::hover());
            crate::icons::paint_ink(ui.painter(), icon, slot.current_icon(app.ui.visual_style), t.form_dim);
        }
        ui_kit::text(ui, app.tr(slot.label), ui_kit::Text::NameCell(true));
    });
    scenario::publish_rect("prefs:navbar:inspector:name", name);
    scenario::publish_rect("prefs:navbar:inspector:header", bounds);
    let mut actions_ui = ui.new_child(egui::UiBuilder::new().id_salt("slot_actions").max_rect(bounds));
    for ((key, icon, enabled), rect) in [
        ("left", "chevron_right", index > 0),
        ("right", "chevron_right", index + 1 < app.preferences.navigation_bar.slots.len()),
        ("remove", "close", true),
    ]
    .into_iter()
    .zip(buttons)
    {
        let label_key = format!("navbar.custom.{key}");
        let label = app.tr_label(app.tr(&label_key));
        let response = actions_ui
            .scope_builder(egui::UiBuilder::new().id_salt(key).max_rect(rect), |ui| {
                ui.add_enabled_ui(enabled, |ui| {
                    if key == "left" {
                        ui_kit::mirrored_icon_button_at(ui, rect, icon, label)
                    } else if key == "right" || compact {
                        ui_kit::icon_button_at(ui, rect, icon, label)
                    } else {
                        ui_kit::icon_label_button(ui, label, icon, true, false)
                    }
                })
                .inner
            })
            .inner
            .backing_on_hover_text(label);
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled && ui.is_enabled(), label));
        scenario::publish_target(&format!("prefs:navbar:{key}"), response.rect);
        if response.clicked() {
            edit(app, state, index, key);
        }
    }
}

fn available(app: &mut SolveApp, ui: &mut egui::Ui, state: &mut State) {
    ui.scope(|ui| {
        ui.spacing_mut().item_spacing.y = Tokens::get().space_6;
        let caption = crate::i18n::format("navbar.custom.available", &[("available", app.tr("ui.workspace_available"))]);
        ui_kit::text(ui, &caption, ui_kit::Text::Hint("navbar_available"));
        ui.horizontal_wrapped(|ui| {
            ui.spacing_mut().item_spacing = vec2(Tokens::get().space_8, Tokens::get().space_8);
            for slot in navigation_bar::CATALOGUE {
                if slot.kind != Kind::Separator && app.preferences.navigation_bar.slots.iter().any(|id| id == slot.id) {
                    continue;
                }
                let dragging = egui::DragAndDrop::payload::<Drag>(ui.ctx()).is_some_and(|payload| payload.from_menu && payload.id.slot == slot.id);
                let (tile, add) = ui_kit::navigation_tile(ui, slot.id, app.tr(slot.label), slot.current_icon(app.ui.visual_style), dragging);
                scenario::publish_target(&format!("prefs:navbar:tile:{}", slot.id), tile.rect);
                scenario::publish_target(&format!("prefs:navbar:add:{}", slot.id), add.rect);
                if tile.drag_started() {
                    tile.dnd_set_drag_payload(Drag {
                        id: Source { slot: slot.id.to_owned(), index: None, slots: app.preferences.navigation_bar.slots.clone() },
                        from_menu: true,
                    });
                }
                if add.clicked() {
                    let index = app.preferences.navigation_bar.slots.len();
                    state.notice = app.preferences.navigation_bar.insert(index, slot.id).err();
                    if state.notice.is_none() {
                        state.selected = Some(index);
                    }
                }
            }
        });
    });
}

fn appearance(app: &mut SolveApp, ui: &mut egui::Ui) {
    let bar = &mut app.preferences.navigation_bar;
    ui_kit::settings_form(ui, Tokens::get().form_label_width, |ui| {
        segments(
            ui,
            "position",
            &mut match bar.position {
                Position::Left => 0,
                Position::Centre => 1,
                Position::Right => 2,
            },
            &["navbar.custom.position_left", "navbar.custom.position_centre", "navbar.custom.position_right"],
            |i| {
                bar.position = match i {
                    0 => Position::Left,
                    2 => Position::Right,
                    _ => Position::Centre,
                }
            },
        );
        segments(ui, "edge", &mut usize::from(bar.edge == Edge::Top), &["navbar.custom.edge_bottom", "navbar.custom.edge_top"], |i| {
            bar.edge = if i == 1 { Edge::Top } else { Edge::Bottom }
        });
        segments(ui, "size", &mut usize::from(bar.size == Size::Regular), &["navbar.custom.compact", "navbar.custom.regular"], |i| {
            bar.size = if i == 1 { Size::Regular } else { Size::Compact }
        });
        segments(ui, "buttons", &mut usize::from(bar.labels == Labels::Labels), &["navbar.custom.icons", "navbar.custom.labels"], |i| {
            bar.labels = if i == 1 { Labels::Labels } else { Labels::Icons }
        });
        segments(
            ui,
            "idle",
            &mut match bar.idle {
                Idle::Always => 0,
                Idle::Fade => 1,
                Idle::Hide => 2,
            },
            &["navbar.custom.always", "navbar.custom.fade", "navbar.custom.hide_idle"],
            |i| {
                bar.idle = match i {
                    0 => Idle::Always,
                    2 => Idle::Hide,
                    _ => Idle::Fade,
                }
            },
        );
    });
}
