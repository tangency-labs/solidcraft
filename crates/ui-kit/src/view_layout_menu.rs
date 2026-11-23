//! The view strip's per-window layout menu. The app bar owns the popup and its dismissal.

use crate::BackingTooltipExt as _;
use crate::app_bar::{TabMenuRow, TabStrip, layout_name, paint_layout_glyph, paint_view_glyph};
use crate::choice::{Segments, segmented};
use crate::menu::{Menu, MenuItem};
use egui::{Align2, Color32, Rect, Response, Sense, Stroke, Ui, pos2, vec2};
use solidcraft_theme::Tokens;
use solidcraft_ui_text::tr_label;

pub(crate) const WIDTH: f32 = 360.0;
const GLYPH_BUTTON: egui::Vec2 = egui::Vec2::new(48.0, 38.0);
const GLYPH_SIZE: f32 = 24.0;
const VIEW_GLYPH_SIZE: f32 = 12.0;
const WINDOW_GLYPH_SIZE: f32 = 16.0;
const CHIP_HEIGHT: f32 = 20.0;
const CHIP_PADDING: f32 = 30.0;
const CHIP_MIN_WIDTH: f32 = 24.0;
const CHIP_ICON_INSET: f32 = 12.0;
const BLOCK_INDENT: f32 = 28.0;
const FLYOUT_WIDTH: f32 = 100.0;

/// The menu's words, read by the host from its message catalog. The two templates hold `{layout}` and
/// `{count}`, and `{design}`, for the kit to fill in.
#[derive(Default)]
pub struct LayoutText {
    pub heading: String,
    pub summary_single: String,
    pub summary: String,
    pub this_window: String,
    pub show: String,
    pub bring: String,
    pub windows: String,
    pub link: String,
    pub off: String,
    pub orientation: String,
    pub zoom: String,
    pub both: String,
    pub new_cells: String,
    pub detach: String,
    pub equal: String,
    pub reset: String,
    pub clean: String,
    pub save: String,
    pub saved: String,
    pub manage: String,
}

pub struct LayoutMenu {
    pub text: LayoutText,
    pub design: String,
    pub window: u32,
    pub native: bool,
    pub visible: usize,
    pub link: String,
    pub can_link: bool,
    pub clean: bool,
    pub windows: Vec<LayoutWindow>,
    pub saved: Vec<String>,
    pub matching_saved: Option<String>,
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
    Saved,
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

fn action<'a>(ui: &mut Ui, id: &'a str, label: &'a str, icon: impl Into<crate::Icon<'a>>, shortcut: &'a str, checked: Option<bool>) -> Response {
    let mut row = MenuItem::new(id, label, icon);
    row.shortcut = shortcut;
    row.checked = checked;
    let response = row.show(ui);
    if let Some(checked) = checked {
        response.widget_info(|| egui::WidgetInfo::selected(egui::Role::CheckBox, true, checked, label));
    }
    response
}

fn flyout_row<'a>(
    ui: &mut Ui,
    id: &'a str,
    label: &'a str,
    icon: impl Into<crate::Icon<'a>>,
    value: &'a str,
    target: Flyout,
    open: &mut Option<Flyout>,
) -> Response {
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
        ui.spacing_mut().item_spacing.x = t.space_6;
        ui.add_space(t.space_2);
        for row in strip.layout_menu.iter().filter(|row| !row.heading && row.id.starts_with("layout.")) {
            let label = tr_label(row.label);
            let current = row.id == strip.layout_current;
            let (rect, response) = ui.allocate_exact_size(GLYPH_BUTTON, Sense::click());
            response.widget_info(|| egui::WidgetInfo::selected(egui::Role::RadioButton, true, current, label));
            if present && current {
                response.request_focus();
            }
            let fill = if current || response.hovered() { t.accent_soft } else { t.dialog_body };
            let stroke = if current || response.has_focus() { Stroke::new(2.0, t.accent) } else { Stroke::new(1.0, t.border) };
            ui.painter().rect(rect, 7, fill, stroke, egui::StrokeKind::Inside);
            paint_layout_glyph(
                ui.painter(),
                Rect::from_center_size(rect.center(), egui::Vec2::splat(GLYPH_SIZE)),
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
    let t = Tokens::get();
    let layout = layout_name(strip);
    let template = if data.visible == 1 { &data.text.summary_single } else { &data.text.summary };
    let label = template.replace("{layout}", layout).replace("{count}", &data.visible.to_string());
    ui.add_space(t.space_4);
    ui.horizontal(|ui| {
        ui.add_space(t.space_8);
        let mut summary = egui::text::LayoutJob::default();
        let label_style = egui::TextFormat::simple(egui::FontId::proportional(t.font_body), t.text_dim);
        if let Some(rest) = label.strip_prefix(layout) {
            summary.append(layout, 0.0, egui::TextFormat::simple(crate::type_face::semibold(t.font_button), t.text));
            summary.append(rest, 0.0, label_style);
        } else {
            summary.append(&label, 0.0, label_style);
        }
        ui.label(summary);
    });
    ui.add_space(t.space_4);
}

fn view_chip(ui: &mut Ui, view: &LayoutView) {
    let t = Tokens::get();
    let font = crate::type_face::semibold(t.font_caption);
    let text = ui.painter().layout_no_wrap(view.name.clone(), font.clone(), t.text);
    let width = (text.size().x + CHIP_PADDING).min(ui.available_width()).max(CHIP_MIN_WIDTH);
    let (rect, response) = ui.allocate_exact_size(vec2(width, CHIP_HEIGHT), Sense::hover());
    let painter = ui.painter().with_clip_rect(rect);
    painter.rect_filled(rect, 10, t.panel_header);
    let icon = Rect::from_center_size(pos2(rect.left() + CHIP_ICON_INSET, rect.center().y), egui::Vec2::splat(VIEW_GLYPH_SIZE));
    paint_view_glyph(&painter, icon, &view.kind, view.accent);
    painter.text(pos2(icon.right() + t.space_4, rect.center().y), Align2::LEFT_CENTER, &view.name, font, t.text);
    response.backing_on_hover_text(&view.name);
}

fn window_block(ui: &mut Ui, data: &LayoutMenu, window: &LayoutWindow) -> Option<String> {
    let t = Tokens::get();
    let local = window.id == data.window;
    let title = if local { data.text.this_window.as_str() } else { window.name.as_str() };
    let detail = if local { Some(window.name.as_str()) } else { window.display.as_deref() };
    let mut picked = None;
    ui.push_id(window.id, |ui| {
        ui.horizontal(|ui| {
            ui.add_space(t.space_8);
            let (icon, _) = ui.allocate_exact_size(egui::Vec2::splat(WINDOW_GLYPH_SIZE), Sense::hover());
            paint_view_glyph(ui.painter(), icon, crate::icons::window_icon(local), t.text_dim);
            let show = data.text.show.as_str();
            let bring = data.text.bring.as_str();
            let action_width = crate::icon_label_width(ui, show, None, false)
                + crate::icon_label_width(ui, bring, None, false)
                + t.space_3 * ui.spacing().item_spacing.x;
            let title_width = if local { ui.available_width() } else { (ui.available_width() - action_width).max(0.0) };
            ui.allocate_ui_with_layout(vec2(title_width, t.control_height), egui::Layout::left_to_right(egui::Align::Center), |ui| {
                ui.add(egui::Label::new(egui::RichText::new(title).font(crate::type_face::semibold(t.font_button)).color(t.text)).truncate())
                    .backing_on_hover_text(title);
                if let Some(detail) = detail {
                    ui.add(egui::Label::new(egui::RichText::new(detail).size(t.font_small).color(t.text_dim)).truncate());
                }
            });
            if !local {
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if crate::icon_label_action(ui, bring, None, window.id != 0, false).clicked() {
                        picked = Some(format!("win.bring.{}", window.id));
                    }
                    if crate::icon_label_action(ui, show, None, true, false).clicked() {
                        picked = Some(format!("win.focus.{}", window.id));
                    }
                });
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.add_space(BLOCK_INDENT);
            for view in &window.views {
                view_chip(ui, view);
            }
        });
        ui.add_space(t.space_6);
    });
    picked
}

fn camera_link(ui: &mut Ui, data: &LayoutMenu) -> Option<String> {
    let keys = ["off", "orientation", "zoom", "both"];
    let text = &data.text;
    let labels = [text.off.as_str(), text.orientation.as_str(), text.zoom.as_str(), text.both.as_str()];
    let mut selected = keys.iter().position(|mode| *mode == data.link).unwrap_or(0);
    Menu::heading(ui, &data.text.link);
    let enabled = [data.can_link; 4];
    let config = Segments { enabled: &enabled, compact: true, ..Segments::default() };
    segmented(ui, &mut selected, &labels, config).then(|| format!("layout.link.{}", keys[selected]))
}

fn fill_action(ui: &mut Ui, strip: &TabStrip, data: &LayoutMenu, fills: &[&TabMenuRow<'_>], focus_first: bool) -> Option<String> {
    Menu::heading(ui, &data.text.new_cells);
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
    Menu::heading(ui, &data.text.detach);
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

fn saved_action(ui: &mut Ui, data: &LayoutMenu, focus_first: bool) -> Option<String> {
    Menu::heading(ui, &data.text.saved);
    let mut picked = None;
    for (index, name) in data.saved.iter().enumerate() {
        let id = format!("layout.saved.{name}");
        let checked = data.matching_saved.as_ref() == Some(name);
        let response = action(ui, &id, name, "", "", Some(checked));
        if focus_first && (checked || index == 0) {
            response.request_focus();
        }
        if response.clicked() {
            picked = Some(id);
        }
    }
    Menu::separator(ui);
    if action(ui, "layout.manage", &data.text.manage, "", "", None).clicked() {
        picked = Some("layout.manage".into());
    }
    picked
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
    Menu::heading(ui, &data.text.heading);
    let mut picked = glyphs(ui, strip, first_frame);
    summary(ui, strip, data);
    if flags.equal && action(ui, "layout.equal", &data.text.equal, "", "", None).clicked() {
        picked = Some("layout.equal".into());
    }
    let fills: Vec<_> = strip.layout_menu.iter().filter(|row| row.id.starts_with("fill.")).collect();
    let fill = fills.iter().find(|row| row.id == strip.layout_fill).map_or("", |row| tr_label(row.label));
    let fill_row = flyout_row(ui, "layout.fill", &data.text.new_cells, "", fill, Flyout::Fill, &mut open);
    if flags.windows {
        Menu::separator(ui);
        Menu::heading(ui, &data.text.windows.replace("{design}", &data.design));
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
    let detach_row =
        flags.detach.then(|| flyout_row(ui, "layout.detach", &data.text.detach, crate::icons::window_icon(false), "", Flyout::Detach, &mut open));
    if action(ui, "layout.reset_cameras", &data.text.reset, "", "", None).clicked() {
        picked = Some("layout.reset_cameras".into());
    }
    if action(ui, "layout.clean", &data.text.clean, "", "F11", Some(data.clean)).clicked() {
        picked = Some("layout.clean".into());
    }
    Menu::separator(ui);
    let glyph = |p: &egui::Painter, rect, opacity| paint_view_glyph(p, rect, "tiles", Tokens::get().text_dim.gamma_multiply(opacity));
    let icon = crate::Icon::Paint(&glyph);
    if action(ui, "layout.save_view", &data.text.save, icon, "", None).clicked() {
        picked = Some("layout.save_view".into());
    }
    let saved_row = (!data.saved.is_empty()).then(|| flyout_row(ui, "layout.saved", &data.text.saved, icon, "", Flyout::Saved, &mut open));
    let mut submenu = None;
    if let Some((flyout, anchor)) = open.and_then(|flyout| match flyout {
        Flyout::Fill => Some((flyout, &fill_row)),
        Flyout::Detach => detach_row.as_ref().map(|row| (flyout, row)),
        Flyout::Saved => saved_row.as_ref().map(|row| (flyout, row)),
    }) {
        let child = Menu::submenu(ui.ctx(), id.with("flyout_area"), anchor.rect, FLYOUT_WIDTH, false, |ui| {
            let focus_first = old_open != open;
            match flyout {
                Flyout::Fill => fill_action(ui, strip, data, &fills, focus_first),
                Flyout::Detach => detach_action(ui, data, focus_first),
                Flyout::Saved => saved_action(ui, data, focus_first),
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
