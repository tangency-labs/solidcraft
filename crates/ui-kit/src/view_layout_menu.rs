//! The view strip's per-window layout menu. The app bar owns the popup and its dismissal.

use crate::app_bar::{TabMenuRow, TabStrip, layout_name, paint_layout_glyph, paint_view_glyph};
use crate::choice::{Segments, segmented};
use crate::menu::{Menu, MenuItem};
use crate::{BackingTooltipExt as _, Button};
use egui::{Align2, Color32, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_ui_text::{format as message, tr, tr_label};

pub(crate) const WIDTH: f32 = 360.0;

pub struct LayoutMenu {
    pub design: String,
    pub window: u32,
    pub native: bool,
    pub visible: usize,
    pub link: String,
    pub can_link: bool,
    pub clean: bool,
    pub windows: Vec<LayoutWindow>,
}

pub struct LayoutWindow {
    pub id: u32,
    pub name: String,
    pub display: Option<String>,
    pub views: Vec<LayoutView>,
}

pub struct LayoutView {
    pub id: u64,
    pub name: String,
    pub kind: String,
    pub accent: Color32,
}

#[derive(Default)]
pub(crate) struct MenuOut {
    pub(crate) action: Option<String>,
    pub(crate) submenu: Option<Rect>,
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Flyout {
    Fill,
    Detach,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct Sections {
    pub(crate) equal: bool,
    pub(crate) windows: bool,
    pub(crate) link: bool,
    pub(crate) detach: bool,
}

pub(crate) fn sections(data: &LayoutMenu) -> Sections {
    let windows = data.windows.len() > 1;
    Sections { equal: data.visible >= 2, windows, link: data.visible >= 2, detach: data.native && !windows }
}

pub(crate) fn keep_open(action: &str) -> bool {
    action.starts_with("win.focus.")
        || action.starts_with("win.bring.")
        || action.starts_with("fill.")
        || action.starts_with("layout.link.")
        || matches!(action, "layout.single" | "layout.side_by_side" | "layout.stacked" | "layout.large_and_two" | "layout.grid")
}

fn action(ui: &mut Ui, id: &str, label: &str, icon: &str, shortcut: &str, checked: Option<bool>) -> Response {
    let mut row = MenuItem::new(id, label, icon);
    row.shortcut = shortcut;
    row.checked = checked;
    let response = row.show(ui);
    if let Some(checked) = checked {
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, checked, label));
    }
    response
}

fn flyout_row(ui: &mut Ui, id: &str, label: &str, icon: &str, value: &str, target: Flyout, open: &mut Option<Flyout>) -> Response {
    let mut row = MenuItem::new(id, label, icon);
    row.shortcut = value;
    row.submenu = true;
    row.highlighted = *open == Some(target);
    let response = row.show(ui);
    let arrow = response.has_focus() && ui.input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight));
    ui.ctx().accesskit_node_builder(response.id, |node| node.set_expanded(*open == Some(target)));
    if arrow {
        ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
    }
    if response.clicked() || arrow {
        *open = if *open == Some(target) && !arrow { None } else { Some(target) };
    }
    response
}

fn glyphs(ui: &mut Ui, strip: &TabStrip, present: bool) -> Option<String> {
    let t = Tokens::get();
    let mut chosen = None;
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        ui.add_space(2.0);
        for row in strip.layout_menu.iter().filter(|row| !row.heading && row.id.starts_with("layout.")) {
            let label = tr_label(row.label);
            let current = row.id == strip.layout_current;
            let (rect, response) = ui.allocate_exact_size(vec2(48.0, 38.0), Sense::click());
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, current, label));
            if present && current {
                response.request_focus();
            }
            let fill = if current || response.hovered() { t.accent_soft } else { t.dialog_body };
            let stroke = if current || response.has_focus() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
            ui.painter().rect(rect, 7, fill, stroke, egui::StrokeKind::Inside);
            paint_layout_glyph(
                ui.painter(),
                Rect::from_center_size(rect.center(), vec2(24.0, 24.0)),
                row.id,
                if current { t.accent } else { t.text_dim },
            );
            solidcraft_ui_registry::publish_target(row.id, rect);
            if response.backing_on_hover_text(label).clicked() {
                chosen = Some(row.id.to_owned());
            }
        }
    });
    chosen
}

fn summary(ui: &mut Ui, strip: &TabStrip, data: &LayoutMenu) {
    let layout = layout_name(strip);
    let label = if data.visible == 1 {
        message("view_layout.summary_single", &[("layout", layout)])
    } else {
        message("view_layout.summary", &[("layout", layout), ("count", &data.visible.to_string())])
    };
    ui.add_space(4.0);
    ui.horizontal(|ui| {
        ui.add_space(8.0);
        let mut summary = egui::text::LayoutJob::default();
        let label_style = egui::TextFormat::simple(egui::FontId::proportional(12.0), Tokens::get().text_dim);
        if let Some(rest) = label.strip_prefix(layout) {
            summary.append(layout, 0.0, egui::TextFormat::simple(crate::type_face::semibold(13.0), Tokens::get().text));
            summary.append(rest, 0.0, label_style);
        } else {
            summary.append(&label, 0.0, label_style);
        }
        ui.label(summary);
    });
    ui.add_space(4.0);
}

fn view_chip(ui: &mut Ui, view: &LayoutView) {
    let t = Tokens::get();
    let font = crate::type_face::semibold(11.5);
    let text = ui.painter().layout_no_wrap(view.name.clone(), font.clone(), t.text);
    let width = (text.size().x + 30.0).min(ui.available_width()).max(24.0);
    let (rect, response) = ui.allocate_exact_size(vec2(width, 20.0), Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 10, t.chip);
    let icon = Rect::from_center_size(pos2(rect.left() + 12.0, rect.center().y), vec2(12.0, 12.0));
    paint_view_glyph(&painter, icon, &view.kind, view.accent);
    painter.text(pos2(icon.right() + 4.0, rect.center().y), Align2::LEFT_CENTER, &view.name, font, t.text);
    response.backing_on_hover_text(&view.name);
}

fn window_block(ui: &mut Ui, data: &LayoutMenu, window: &LayoutWindow) -> Option<String> {
    let t = Tokens::get();
    let local = window.id == data.window;
    let title = if local { tr("view_layout.this_window") } else { &window.name };
    let detail = if local { Some(window.display.as_deref().unwrap_or(tr("view_layout.main"))) } else { window.display.as_deref() };
    let mut picked = None;
    ui.push_id(window.id, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(8.0);
            let (icon, _) = ui.allocate_exact_size(vec2(16.0, 16.0), Sense::hover());
            paint_view_glyph(ui.painter(), icon, "window", t.text_dim);
            let title_width = if local { ui.available_width() } else { (ui.available_width() - 145.0).max(80.0) };
            ui.allocate_ui_with_layout(vec2(title_width, 24.0), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).font(crate::type_face::semibold(13.0)).color(t.text)).truncate())
                    .backing_on_hover_text(title);
                if let Some(detail) = detail {
                    ui.add(egui::Label::new(egui::RichText::new(detail).size(11.0).color(t.text_dim)).truncate());
                }
            });
            if !local {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.add_enabled_ui(window.id != 0, |ui| crate::button(ui, tr("view_layout.bring"), Button::Primary)).inner.clicked() {
                        picked = Some(format!("win.bring.{}", window.id));
                    }
                    if crate::button(ui, tr("view_layout.show"), Button::Small).clicked() {
                        picked = Some(format!("win.focus.{}", window.id));
                    }
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(28.0);
            for view in &window.views {
                view_chip(ui, view);
            }
        });
        ui.add_space(6.0);
    });
    picked
}

fn camera_link(ui: &mut Ui, data: &LayoutMenu) -> Option<String> {
    let keys = ["off", "orientation", "zoom", "both"];
    let labels = [tr("view_layout.off"), tr("view_layout.orientation"), tr("view_layout.zoom"), tr("view_layout.both")];
    let mut selected = keys.iter().position(|mode| *mode == data.link).unwrap_or(0);
    Menu::heading(ui, tr("view_layout.link"));
    let enabled = [data.can_link; 4];
    let config = Segments { enabled: &enabled, compact: true, ..Segments::default() };
    segmented(ui, &mut selected, &labels, config).then(|| format!("layout.link.{}", keys[selected]))
}

fn fill_action(ui: &mut Ui, strip: &TabStrip, fills: &[&TabMenuRow<'_>], focus_first: bool) -> Option<String> {
    Menu::heading(ui, tr("view_layout.new_cells"));
    let mut chosen = None;
    for (index, row) in fills.iter().enumerate() {
        let current = row.id == strip.layout_fill;
        let response = action(ui, row.id, tr_label(row.label), "", "", Some(current));
        if focus_first && (current || index == 0) {
            response.request_focus();
        }
        if response.clicked() {
            chosen = Some(row.id.to_owned());
        }
    }
    chosen
}

fn detach_action(ui: &mut Ui, data: &LayoutMenu, focus_first: bool) -> Option<String> {
    Menu::heading(ui, tr("view_layout.detach"));
    let mut chosen = None;
    if let Some(window) = data.windows.iter().find(|window| window.id == data.window) {
        for (index, view) in window.views.iter().enumerate() {
            let id = format!("win.detach.{}", view.id);
            let glyph = |p: &egui::Painter, rect, opacity| paint_view_glyph(p, rect, &view.kind, view.accent.gamma_multiply(opacity));
            let row = MenuItem::new(&id, &view.name, crate::Icon::Paint(&glyph));
            let response = row.show(ui);
            if focus_first && index == 0 {
                response.request_focus();
            }
            if response.clicked() {
                chosen = Some(id);
            }
        }
    }
    chosen
}

pub(crate) fn show(ui: &mut Ui, strip: &TabStrip, data: &LayoutMenu, opened: u64) -> MenuOut {
    let flags = sections(data);
    let id = strip.id.with(("layout_menu", opened));
    let first_frame = ui.is_visible()
        && ui.ctx().data_mut(|memory| {
            let first = memory.get_temp::<bool>(id).is_none();
            memory.insert_temp(id, true);
            first
        });
    let mut open = ui.ctx().data(|memory| memory.get_temp::<Flyout>(id.with("flyout")));
    let old_open = open;
    Menu::heading(ui, tr("view_layout.heading"));
    let mut picked = glyphs(ui, strip, first_frame);
    summary(ui, strip, data);
    if flags.equal && action(ui, "layout.equal", tr("view_layout.equal"), "", "", None).clicked() {
        picked = Some("layout.equal".into());
    }
    let fills: Vec<_> = strip.layout_menu.iter().filter(|row| row.id.starts_with("fill.")).collect();
    let fill = fills.iter().find(|row| row.id == strip.layout_fill).map_or("", |row| tr_label(row.label));
    let fill_row = flyout_row(ui, "layout.fill", tr("view_layout.new_cells"), "", fill, Flyout::Fill, &mut open);
    if flags.windows {
        Menu::separator(ui);
        Menu::heading(ui, &message("view_layout.windows", &[("design", &data.design)]));
        for window in
            data.windows.iter().filter(|window| window.id == data.window).chain(data.windows.iter().filter(|window| window.id != data.window))
        {
            picked = window_block(ui, data, window).or(picked);
        }
    }
    if flags.link {
        Menu::separator(ui);
        picked = camera_link(ui, data).or(picked);
    }
    Menu::separator(ui);
    let detach_row = flags.detach.then(|| flyout_row(ui, "layout.detach", tr("view_layout.detach"), "window", "", Flyout::Detach, &mut open));
    if action(ui, "layout.reset", tr("view_layout.reset"), "", "", None).clicked() {
        picked = Some("layout.reset".into());
    }
    if action(ui, "layout.clean", tr("view_layout.clean"), "", "F11", Some(data.clean)).clicked() {
        picked = Some("layout.clean".into());
    }
    let mut submenu = None;
    if let Some((flyout, anchor)) = open.and_then(|flyout| match flyout {
        Flyout::Fill => Some((flyout, &fill_row)),
        Flyout::Detach => detach_row.as_ref().map(|row| (flyout, row)),
    }) {
        let child = Menu::submenu(ui.ctx(), id.with("flyout_area"), anchor.rect, 100.0, false, |ui| {
            let focus_first = old_open != open;
            match flyout {
                Flyout::Fill => fill_action(ui, strip, &fills, focus_first),
                Flyout::Detach => detach_action(ui, data, focus_first),
            }
        });
        submenu = Some(child.response.rect);
        let dismiss = ui.input_mut(|input| {
            input.consume_key(egui::Modifiers::NONE, egui::Key::Escape) || input.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft)
        });
        if child.inner.is_some() || dismiss {
            open = None;
            ui.memory_mut(|memory| memory.move_focus(egui::FocusDirection::None));
            anchor.request_focus();
        }
        picked = child.inner.or(picked);
    }
    ui.ctx().data_mut(|memory| {
        if let Some(open) = open {
            memory.insert_temp(id.with("flyout"), open);
        } else {
            memory.remove::<Flyout>(id.with("flyout"));
        }
    });
    MenuOut { action: picked, submenu }
}
