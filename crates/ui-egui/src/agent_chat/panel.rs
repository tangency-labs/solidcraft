//! The agent chat panel: docked left or right (switched by dragging its header to the other side,
//! or in Preferences), resizable, folded to a thin strip with Minimise, closed with ×. Its side,
//! width and fold are preferences, so they come back next time.

use crate::fonts::RichTextExt as _;
use crate::ui_kit::BackingTooltipExt as _;
use crate::ui_kit::{Container, Edge, Surface};
use egui::{Align, Align2, Color32, Id, Layout, Rect, RichText, Sense, Stroke, pos2, vec2};

use super::*;
use crate::theme::Tokens;
use crate::ui_kit;

pub const MIN_WIDTH: f32 = 280.0;
pub const MAX_WIDTH: f32 = 760.0;
/// The folded panel's width.
const STRIP: f32 = 30.0;
/// The keyboard shortcut that shows and hides the panel (also from its own text field).
pub const SHORTCUT: egui::KeyboardShortcut = egui::KeyboardShortcut::new(egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT), egui::Key::A);

fn panel_id(left: bool) -> Id {
    Id::new(if left { "sc_agent_chat_left" } else { "sc_agent_chat_right" })
}

/// The largest thumbnail of a message's picture, and its largest preview while hovered (points).
#[cfg(not(target_arch = "wasm32"))]
const THUMB_MAX: (f32, f32) = (150.0, 56.0);
#[cfg(not(target_arch = "wasm32"))]
const PREVIEW_MAX: (f32, f32) = (560.0, 380.0);

/// Forget the panel's own idea of its width, so the preference applies on the next frame.
pub fn forget_width(ctx: &egui::Context) {
    ctx.data_mut(|d| {
        for left in [true, false] {
            d.remove::<egui::containers::panel::PanelState>(panel_id(left));
        }
    });
}

/// Show or hide the panel (a folded panel unfolds first).
pub fn toggle(app: &mut SolveApp) {
    let c = &mut app.agent_chat;
    if !c.open {
        c.open = true;
        app.preferences.agent_chat_minimized = false;
    } else if app.preferences.agent_chat_minimized {
        app.preferences.agent_chat_minimized = false;
    } else {
        c.open = false;
    }
}

/// The tooltip of the chat's buttons: its name and shortcut.
pub fn tip() -> String {
    let key = if cfg!(target_os = "macos") { "Cmd+Shift+A" } else { "Ctrl+Shift+A" };
    format!("{} ({key})", tr("ui.agent_chat"))
}

/// The panel, when the chat is turned on and open. Call before the central viewport is laid out.
pub fn show(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    poll(app, &ctx);
    // Set again below if the chat is drawn this frame.
    app.agent_chat.drop_rect = None;
    // Retain the previous card for shortcut hover routing below; clear it when hidden.
    if !shown(app) || !app.agent_chat.open {
        app.agent_chat.panel_rect = None;
    }
    // With AI features off nothing of the chat is drawn, and the shortcut does nothing.
    if !shown(app) {
        return;
    }
    if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT)) {
        toggle(app);
    }
    // A window that isn't drawn this frame closes; the next one opens at the saved place.
    if !(app.agent_chat.open && app.preferences.agent_chat_popped) {
        app.agent_chat.window_placed = false;
    }
    // New chat, when the chat has focus (its input, or the pointer over the panel).
    let in_chat = ctx.memory(|m| m.has_focus(Id::new("sc_agent_chat_input")))
        || ctx.pointer_hover_pos().is_some_and(|p| app.agent_chat.panel_rect.is_some_and(|r| r.contains(p)));
    if app.agent_chat.open && in_chat && ctx.input_mut(|i| i.consume_shortcut(&NEW_CHAT_SHORTCUT)) {
        request_new_chat(app);
    }
    if !app.agent_chat.open {
        return;
    }
    // Look for installed agents the first time the panel opens (in the background).
    #[cfg(not(target_arch = "wasm32"))]
    if app.agent_chat.detection.is_none() && app.agent_chat.detecting.is_none() {
        refresh(app, &ctx);
    }
    #[cfg(not(target_arch = "wasm32"))]
    if app.preferences.agent_chat_popped {
        window(app, &ctx);
        show_drop_hint(app, &ctx);
        return;
    }
    if Container::for_layout(app.ui.layout, Surface::Panel).floating() {
        floating(app, ui);
        show_drop_hint(app, &ctx);
        return;
    }
    let available = ui.available_rect_before_wrap();
    let screen = ctx.content_rect();
    app.agent_chat.dock_space = Some(Rect::from_x_y_ranges(screen.x_range(), available.y_range()));
    let t = Tokens::get();
    let left = app.preferences.agent_chat_side == "left";
    let host = Container::new(ui.ctx(), Surface::Panel);
    let side = |id: Id| host.edge(id, if left { Edge::Left } else { Edge::Right });
    if app.preferences.agent_chat_minimized {
        let r = host
            .show_edge(ui, side(Id::new("sc_agent_chat_strip")).exact_size(STRIP).resizable(false), host.edge_frame(t.panel), |ui| strip(app, ui));
        app.agent_chat.drop_rect = Some(r.response.rect);
        show_drop_hint(app, &ctx);
        return;
    }
    let width = app.preferences.agent_chat_width.clamp(MIN_WIDTH, MAX_WIDTH);
    let r = host.show_edge(
        ui,
        side(panel_id(left)).default_size(width).size_range(MIN_WIDTH..=MAX_WIDTH).resizable(true),
        host.edge_frame(t.dialog_body),
        |ui| ui_kit::scope(ui, |ui| body(app, ui, left)),
    );
    crate::scenario::publish_rect("agent_chat:panel", r.response.rect);
    app.agent_chat.panel_rect = Some(r.response.rect);
    #[cfg(not(target_arch = "wasm32"))]
    gallery_window(app, &ctx);
    app.agent_chat.drop_rect = Some(r.response.rect);
    show_drop_hint(app, &ctx);
    let w = r.response.rect.width();
    if w.is_finite() && (MIN_WIDTH..=MAX_WIDTH).contains(&w) {
        app.preferences.agent_chat_width = w;
    }
}

/// Horizontal room reserved before the history and browser when Assistant is on the left.
pub(crate) fn left_inset(app: &SolveApp, available: Rect) -> f32 {
    if Container::for_layout(app.ui.layout, Surface::Panel).floating()
        && shown(app)
        && app.agent_chat.open
        && !app.preferences.agent_chat_popped
        && app.preferences.agent_chat_side == "left"
    {
        let width = if app.preferences.agent_chat_minimized { STRIP } else { app.preferences.agent_chat_width.clamp(MIN_WIDTH, MAX_WIDTH) };
        (width + crate::shell::SHELL_GAP).min(available.width().max(0.0))
    } else {
        0.0
    }
}

/// Assistant-specific precedence over tool sheets. Keep this out of the shared container:
/// ordinary tool panels never reserve an outside rail for themselves.
pub(crate) fn tool_space(app: &SolveApp, mut available: Rect) -> Rect {
    if Container::for_layout(app.ui.layout, Surface::Panel).floating()
        && shown(app)
        && app.agent_chat.open
        && !app.preferences.agent_chat_popped
        && let Some(chat) = app.agent_chat.panel_rect.filter(|r| r.is_finite() && r.is_positive())
    {
        if app.preferences.agent_chat_side == "left" {
            available.min.x = crate::shell::within(chat.right() + crate::shell::SHELL_GAP, available.left(), available.right());
        } else {
            available.max.x = crate::shell::within(chat.left() - crate::shell::SHELL_GAP, available.left(), available.right());
        }
    }
    available
}

/// The same outer rectangle is used for the floating sheet and its docking preview.
fn floating_bounds(app: &SolveApp, available: Rect, left: bool) -> Rect {
    let t = Tokens::get();
    let gap = crate::shell::SHELL_GAP;
    let mut bounds = available.shrink(gap);
    // The ribbon already supplies the top gap shared by browser and history cards.
    bounds.min.y = available.top();
    if let Some(history) = app.shell.history.filter(|_| app.ui.show_timeline && !app.preferences.timeline_left) {
        bounds.max.y = (history.top() - gap).min(bounds.bottom()).max(bounds.top());
    }
    // Assistant owns the outside edge on either side. Tool sheets reserve space beside it.
    if !left {
        // Match the shared Panel's right-edge clearance.
        bounds.max.x = bounds.right().min(available.right() - t.space_8).max(bounds.left());
    }
    let width = if app.preferences.agent_chat_minimized { STRIP } else { app.preferences.agent_chat_width.clamp(MIN_WIDTH, MAX_WIDTH) };
    let height = if app.preferences.agent_chat_minimized { (t.space_24 * 3.0 + t.space_16).min(bounds.height()) } else { bounds.height() };
    let width = width.min(bounds.width().max(0.0));
    Rect::from_min_size(pos2(if left { bounds.left() } else { bounds.right() - width }, bounds.top()), vec2(width, height.max(0.0)))
}

/// Use the same bounded, rounded floating surface as feature panels. The sheet overlays
/// the viewport, leaving the model's rendering and picking projection unchanged.
fn floating(app: &mut SolveApp, ui: &mut egui::Ui) {
    let ctx = ui.ctx().clone();
    let t = Tokens::get();
    let available = ui.available_rect_before_wrap();
    app.agent_chat.dock_space = Some(available);
    let left = app.preferences.agent_chat_side == "left";
    let side = if left { crate::frame::Side::Left } else { crate::frame::Side::Right };
    let minimized = app.preferences.agent_chat_minimized;
    let bounds = floating_bounds(app, available, left);
    if !bounds.is_positive() {
        app.agent_chat.panel_rect = None;
        return;
    }
    let width = bounds.width();
    let height = bounds.height();
    let anchor = if left { bounds.left_top() } else { bounds.right_top() };
    let frame = ui_kit::Panel::feature_frame(&ctx, app.ui.layout, side);
    let inner = (vec2(width, height) - frame.total_margin().sum()).max(egui::Vec2::ZERO);
    let panel = ui_kit::Panel::new(tr("ui.assistant"), Id::new("sc_agent_chat_floating"), anchor, available, side, frame).width(width);
    if let Some(card) = panel.fixed_window(&ctx, inner).show(&ctx, |ui| {
        ui.set_min_size(inner);
        ui.set_max_size(inner);
        ui.set_clip_rect(ui.clip_rect().intersect(Rect::from_min_size(ui.max_rect().min, inner)));
        ui_kit::scope(ui, |ui| if minimized { strip(app, ui) } else { body(app, ui, left) });
        if !minimized {
            let rect = ui.max_rect();
            let rail = if left {
                Rect::from_x_y_ranges(rect.right() - t.space_4..=rect.right(), rect.y_range())
            } else {
                Rect::from_x_y_ranges(rect.left()..=rect.left() + t.space_4, rect.y_range())
            };
            let response = ui.interact(rail, Id::new("sc_agent_chat_resize"), Sense::drag()).on_hover_cursor(egui::CursorIcon::ResizeHorizontal);
            crate::scenario::publish_handle("agent_chat:resize", rail.center());
            if response.dragged() {
                let delta = ui.input(|i| i.pointer.delta().x) * if left { 1.0 } else { -1.0 };
                if delta.is_finite() {
                    app.preferences.agent_chat_width = (width + delta).clamp(MIN_WIDTH, MAX_WIDTH);
                }
            }
        }
    }) {
        let rect = card.response.rect;
        app.agent_chat.panel_rect = Some(rect);
        app.agent_chat.drop_rect = Some(rect);
        crate::scenario::publish_rect("agent_chat:panel", rect);
    }
    #[cfg(not(target_arch = "wasm32"))]
    gallery_window(app, &ctx);
}

/// The most chips one group shows before "+N more".
const MAX_CHIPS: usize = 6;
/// A chip's longest label (characters), and what its frame adds to the text's width.
const CHIP_CHARS: usize = 26;
const CHIP_PAD: f32 = 13.0;
/// What a chip's × adds.
const CHIP_CROSS: f32 = 14.0;

/// One thing in a group's rows: its heading, a chip for an item, or the "+N more" note.
#[cfg(not(target_arch = "wasm32"))]
enum ChipPart<'a> {
    Heading(String),
    /// An item and its place among all of the context's items.
    Chip(usize, &'a context::Entry),
    More(String),
}

/// What was selected when a message was sent, grouped (elements, features, bodies) with a count
/// each and a chip per item; hovering a chip tells what is known about it. The rows are filled by
/// hand: egui wraps plain widgets, not framed ones, and one row wider than the chat would widen
/// the whole panel.
#[cfg(not(target_arch = "wasm32"))]
fn context_chips(ui: &mut egui::Ui, context: &Context, removable: bool) -> Option<usize> {
    let mut removed = None;
    if context.is_empty() {
        return removed;
    }
    let t = Tokens::get();
    let font = egui::FontId::proportional(t.font_small);
    let (avail, gap) = (ui.available_width(), ui.spacing().item_spacing.x);
    let text_width = |ui: &egui::Ui, text: &str| ui.painter().layout_no_wrap(text.to_string(), font.clone(), t.text).size().x;
    let more = |n: usize| trf("ui.agent_chat_ctx_more", &[("n", &n.to_string())]);
    for kind in context::Kind::ALL {
        let items: Vec<(usize, &context::Entry)> = context.items.iter().enumerate().filter(|(_, i)| i.kind == kind).collect();
        if items.is_empty() {
            continue;
        }
        let mut parts = vec![ChipPart::Heading(format!("{} {}", tr(kind.heading()), items.len()))];
        parts.extend(items.iter().take(MAX_CHIPS).map(|(n, i)| ChipPart::Chip(*n, i)));
        if items.len() > MAX_CHIPS {
            parts.push(ChipPart::More(more(items.len() - MAX_CHIPS)));
        }
        let widths: Vec<f32> = parts
            .iter()
            .map(|p| match p {
                ChipPart::Heading(s) | ChipPart::More(s) => text_width(ui, s),
                ChipPart::Chip(_, i) => text_width(ui, &shorten(&i.label, CHIP_CHARS)) + CHIP_PAD + if removable { CHIP_CROSS } else { 0.0 },
            })
            .collect();
        let mut rows: Vec<Vec<usize>> = vec![Vec::new()];
        let mut used = 0.0_f32;
        for (n, w) in widths.iter().enumerate() {
            let fits = used + if used > 0.0 { gap } else { 0.0 } + w <= avail;
            if !fits && used > 0.0 {
                rows.push(Vec::new());
                used = 0.0;
            }
            used += if used > 0.0 { gap } else { 0.0 } + w;
            if let Some(row) = rows.last_mut() {
                row.push(n);
            }
        }
        for row in rows {
            ui.horizontal(|ui| {
                for n in row {
                    match &parts[n] {
                        ChipPart::Heading(s) | ChipPart::More(s) => {
                            ui.label(RichText::new(s).color(t.text_dim).size(t.font_small));
                        }
                        ChipPart::Chip(place, item) => {
                            let chip = egui::Frame::NONE
                                .stroke(Stroke::new(1.0, t.text_dim.gamma_multiply(0.5)))
                                .corner_radius(8.0)
                                .inner_margin(egui::Margin::symmetric(5, 1))
                                .show(ui, |ui| {
                                    ui.spacing_mut().item_spacing.x = 3.0;
                                    ui.horizontal(|ui| {
                                        let label = ui.add(
                                            egui::Label::new(RichText::new(shorten(&item.label, CHIP_CHARS)).color(t.text).size(t.font_small))
                                                .wrap_mode(egui::TextWrapMode::Extend),
                                        );
                                        if removable {
                                            let cross = ui_kit::button(ui, "×", ui_kit::Button::Small);
                                            crate::scenario::publish_handle(&format!("agent_chat:picked_remove:{place}"), cross.rect.center());
                                            if cross.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                                                removed = Some(*place);
                                            }
                                        }
                                        label
                                    })
                                    .inner
                                })
                                .response;
                            if !item.detail.is_empty() {
                                chip.backing_on_hover_text(&item.detail);
                            }
                        }
                    }
                }
            });
        }
    }
    if context.omitted > 0 {
        ui.label(RichText::new(more(context.omitted)).color(t.text_dim).size(t.font_small));
    }
    removed
}

/// `s` cut to `max` characters, with an ellipsis.
#[cfg(not(target_arch = "wasm32"))]
fn shorten(s: &str, max: usize) -> String {
    if s.chars().count() <= max { s.to_string() } else { format!("{}…", s.chars().take(max.saturating_sub(1)).collect::<String>()) }
}

/// The pictures of a message as small thumbnails in a row. Hovering one shows it bigger for as long
/// as the pointer stays; a click returns its place in the row, to open the viewer there.
#[cfg(not(target_arch = "wasm32"))]
fn picture_strip(ui: &mut egui::Ui, n: usize, pictures: &[Picture]) -> Option<usize> {
    let t = Tokens::get();
    let mut clicked = None;
    ui.horizontal_wrapped(|ui| {
        for (i, picture) in pictures.iter().enumerate() {
            let texture = match picture.texture(ui.ctx()) {
                Shown::Ready(texture) => texture,
                // Being decoded: a spinner the size of a thumbnail's corner, until it is in.
                Shown::Loading => {
                    ui.add(egui::Spinner::new().size(t.space_24));
                    continue;
                }
                // Not a readable picture: its name, or the plain caption.
                Shown::Failed => {
                    let caption = if picture.note.is_empty() { tr("ui.agent_chat_view_attached").to_string() } else { picture.note.clone() };
                    ui.label(RichText::new(caption).color(t.text_dim).size(t.font_small));
                    continue;
                }
            };
            let size = fit(texture.size_vec2(), vec2(THUMB_MAX.0, THUMB_MAX.1));
            let thumb = ui.add(egui::Image::new((texture.id(), size)).corner_radius(t.radius_sm).sense(Sense::click()));
            crate::scenario::publish_handle(&format!("agent_chat:picture:{n}:{i}"), thumb.rect.center());
            let hover_max = (ui.ctx().content_rect().size() * 0.5).min(vec2(PREVIEW_MAX.0, PREVIEW_MAX.1));
            let thumb = thumb.on_hover_cursor(egui::CursorIcon::PointingHand).backing_on_hover_ui(|ui| {
                ui.add(egui::Image::new((texture.id(), fit(texture.size_vec2(), hover_max))).corner_radius(t.radius_sm));
                if !picture.note.is_empty() {
                    ui.label(&picture.note);
                }
            });
            if thumb.clicked() {
                clicked = Some(i);
            }
        }
    });
    clicked
}

/// `size` scaled to fit in `max`, keeping its shape (never below one point).
#[cfg(not(target_arch = "wasm32"))]
fn fit(size: egui::Vec2, max: egui::Vec2) -> egui::Vec2 {
    let scale = (max.x / size.x.max(1.0)).min(max.y / size.y.max(1.0)).min(2.0);
    vec2((size.x * scale).max(1.0), (size.y * scale).max(1.0))
}

/// The viewer: a message's pictures over the app, one at a time with a counter, Previous and Next
/// (also the arrow keys), closed with the button, Esc or a click outside.
#[cfg(not(target_arch = "wasm32"))]
fn gallery_window(app: &mut SolveApp, ctx: &egui::Context) {
    let Some(mut gallery) = app.agent_chat.gallery.take() else { return };
    let count = gallery.pictures.len();
    gallery.at = gallery.at.min(count.saturating_sub(1));
    let (left, right) =
        ctx.input_mut(|i| (i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowLeft), i.consume_key(egui::Modifiers::NONE, egui::Key::ArrowRight)));
    let mut step: isize = if left { -1 } else { isize::from(right) };
    let t = Tokens::get();
    let max = ctx.content_rect().size() * vec2(0.8, 0.7);
    let modal = ui_kit::Container::new(ctx, ui_kit::Surface::Window).modal(Id::new("sc_agent_chat_gallery")).show(ctx, |ui| {
        ui.set_max_width(max.x);
        ui.horizontal(|ui| {
            let prev = ui_kit::button(ui, tr("ui.agent_chat_previous_picture"), ui_kit::Button::Enabled(count > 1));
            crate::scenario::publish_handle("agent_chat:gallery_prev", prev.rect.center());
            let next = ui_kit::button(ui, tr("ui.agent_chat_next_picture"), ui_kit::Button::Enabled(count > 1));
            crate::scenario::publish_handle("agent_chat:gallery_next", next.rect.center());
            if prev.clicked() {
                step = -1;
            }
            if next.clicked() {
                step = 1;
            }
            ui.label(RichText::new(format!("{} / {}", gallery.at + 1, count)).color(t.text_dim));
        });
        if let Some(picture) = gallery.pictures.get(gallery.at) {
            match picture.texture(ui.ctx()) {
                Shown::Ready(texture) => {
                    let resp = ui.add(egui::Image::new((texture.id(), fit(texture.size_vec2(), max))).corner_radius(t.radius_sm));
                    crate::scenario::publish_rect("agent_chat:gallery_picture", resp.rect);
                }
                Shown::Loading => {
                    ui.add(egui::Spinner::new().size(t.space_24));
                }
                Shown::Failed => {
                    ui.label(RichText::new(tr("ui.agent_chat_view_attached")).color(t.text_dim));
                }
            }
            if !picture.note.is_empty() {
                ui.add(egui::Label::new(&picture.note).wrap());
            }
        }
    });
    if count > 0 && step != 0 {
        gallery.at = (gallery.at as isize + step).rem_euclid(count as isize) as usize;
    }
    if !modal.should_close() {
        app.agent_chat.gallery = Some(gallery);
    }
}

/// While files are dragged over the window: if they would drop on the chat, say so over it.
fn show_drop_hint(app: &SolveApp, ctx: &egui::Context) {
    #[cfg(not(target_arch = "wasm32"))]
    if let Some(rect) = app.agent_chat.drop_rect
        && ctx.input(|i| !i.raw.hovered_files.is_empty())
        && drop_is_ours(app, ctx)
    {
        drop_hint(ctx, rect);
    }
    #[cfg(target_arch = "wasm32")]
    let _ = (app, ctx);
}

/// Where dragged files will go: a tinted zone over the chat with a line of text.
#[cfg(not(target_arch = "wasm32"))]
fn drop_hint(ctx: &egui::Context, area: Rect) {
    let t = Tokens::get();
    let painter = ctx.layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("sc_agent_chat_drop_hint")));
    let zone = area.shrink(6.0);
    painter.rect(zone, t.radius_xl, t.accent.gamma_multiply(0.16), Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
    let font = egui::FontId::proportional(Tokens::get().font_heading);
    let text = painter.layout(tr("ui.agent_chat_drop_hint").to_string(), font, t.text, (zone.width() - 24.0).max(40.0));
    let at = zone.center() - text.size() / 2.0;
    painter.galley(at, text, t.text);
    crate::scenario::publish_rect("agent_chat:drop_hint", zone);
}

/// New chat (when the chat has focus).
pub const NEW_CHAT_SHORTCUT: egui::KeyboardShortcut =
    egui::KeyboardShortcut::new(egui::Modifiers::COMMAND.plus(egui::Modifiers::SHIFT), egui::Key::L);

/// Clear the chat, asking first when something would be lost.
fn request_new_chat(app: &mut SolveApp) {
    if new_chat_needs_confirm(app) {
        app.agent_chat.confirm_new = true;
    } else {
        new_chat(app);
    }
}

/// A saved window place (`[x, y, width, height]`) that can be used: finite, a usable size, and
/// not absurdly far away. `x` and `y` may be negative: a screen left of or above the main one.
pub fn window_place(saved: Option<[f32; 4]>) -> Option<([f32; 2], [f32; 2])> {
    let [x, y, w, h] = saved?;
    let usable = [x, y, w, h].iter().all(|v| v.is_finite())
        && (MIN_WIDTH..=10_000.0).contains(&w)
        && (200.0..=10_000.0).contains(&h)
        && x.abs() <= 100_000.0
        && y.abs() <= 100_000.0;
    usable.then_some(([x, y], [w, h]))
}

/// The chat window's settings. The saved place and size go only to a window that is opening:
/// egui turns a changed place into a move, and the system rounds what it is given, so giving it
/// every frame made the window chase its own rounding (it shook a pixel up and down).
pub fn window_builder(app: &SolveApp) -> egui::ViewportBuilder {
    let title = format!("{} — {}", app.session.doc.name, tr("ui.agent_chat"));
    let builder = egui::ViewportBuilder::default().with_title(title).with_min_inner_size([MIN_WIDTH, 320.0]);
    if app.agent_chat.window_placed {
        return builder;
    }
    match window_place(app.preferences.agent_chat_window) {
        Some((pos, size)) => builder.with_position(pos).with_inner_size(size),
        None => builder.with_inner_size([app.preferences.agent_chat_width.clamp(MIN_WIDTH, MAX_WIDTH), 720.0]),
    }
}

/// The chat in its own window, named after the design: a native window on the desktop, which can
/// go to another screen, or a window floating inside the app where the system can't open one.
/// The system's close button closes the chat (it opens there again); Dock puts it back.
#[cfg(not(target_arch = "wasm32"))]
fn window(app: &mut SolveApp, ctx: &egui::Context) {
    let builder = window_builder(app);
    app.agent_chat.window_placed = true;
    // The side panel isn't drawn: its place no longer counts as "in the chat".
    app.agent_chat.panel_rect = None;
    ctx.show_viewport_immediate(egui::ViewportId::from_hash_of("sc_agent_chat_window"), builder, |ui, class| {
        let t = Tokens::get();
        let frame = Container::new(ui.ctx(), Surface::Panel).edge_frame(t.dialog_body);
        let panel = Container::new(ui.ctx(), Surface::Viewport).central();
        let r = Container::show_central(ui, panel, frame, |ui| ui_kit::scope(ui, |ui| body(app, ui, false)));
        // The picture viewer opens over this window, where the click was, not over the main one.
        gallery_window(app, ui.ctx());
        if class == egui::ViewportClass::EmbeddedWindow {
            // Floating inside the app: its handles are the main window's, as the panel's are. The
            // app's own drop handling sees the drops, so the window counts as the chat's place
            // (the hint is drawn by the caller).
            crate::scenario::publish_rect("agent_chat:window", r.response.rect);
            app.agent_chat.panel_rect = Some(r.response.rect);
            app.agent_chat.drop_rect = Some(r.response.rect);
            return;
        }
        let ctx = ui.ctx().clone();
        // A window of its own gets the drops made on it: the whole window is the chat, so there
        // is no pointer to ask about (the main window's handling never sees them).
        let dropped = ctx.input_mut(|i| std::mem::take(&mut i.raw.dropped_files));
        if !dropped.is_empty() {
            attach_dropped(app, dropped.iter().map(|f| f.path().to_path_buf()));
        }
        if ctx.input(|i| !i.raw.hovered_files.is_empty()) {
            drop_hint(&ctx, r.response.rect);
        }
        if ctx.input_mut(|i| i.consume_shortcut(&SHORTCUT)) {
            toggle(app);
        }
        // The whole window is the chat, so New chat works anywhere in it.
        if ctx.input_mut(|i| i.consume_shortcut(&NEW_CHAT_SHORTCUT)) {
            request_new_chat(app);
        }
        let (outer, inner, close) = ctx.input(|i| (i.viewport().outer_rect, i.viewport().inner_rect, i.viewport().close_requested()));
        if let (Some(o), Some(n)) = (outer, inner) {
            app.preferences.agent_chat_window = Some([o.min.x, o.min.y, n.width(), n.height()]);
        }
        if close {
            app.agent_chat.open = false;
        }
    });
}

/// Whether the chat's own window should become active because the pointer is over its text box:
/// only when it is in a window of its own that isn't active and the app's main window is (never
/// taking the focus from another app).
pub fn take_focus_for_text(popped: bool, over_text: bool, chat_active: Option<bool>, main_active: Option<bool>) -> bool {
    popped && over_text && chat_active == Some(false) && main_active == Some(true)
}

/// A small drawn button: `kind` is `minimize`, `close`, `newchat`, `stopturn`, `pop_out` or `dock`.
fn glyph_button(ui: &mut egui::Ui, kind: &str, tip: &str) -> egui::Response {
    let t = Tokens::get();
    let (r, resp) = ui.allocate_exact_size(vec2(t.form_close_size, t.form_close_size), Sense::click());
    if resp.hovered() {
        ui.painter().rect_filled(r, t.radius_sm, t.hover);
    }
    let c = r.center();
    let s = Stroke::new(1.4, t.text);
    let p = ui.painter();
    match kind {
        "minimize" => {
            ui_kit::disclosure(p, c, true, t.text);
        }
        "close" => {
            p.line_segment([c + vec2(-4.5, -4.5), c + vec2(4.5, 4.5)], s);
            p.line_segment([c + vec2(-4.5, 4.5), c + vec2(4.5, -4.5)], s);
        }
        // A window with an arrow leaving it by its top right corner.
        "pop_out" => {
            p.line_segment([c + vec2(-1.0, -5.0), c + vec2(-5.0, -5.0)], s);
            p.line_segment([c + vec2(-5.0, -5.0), c + vec2(-5.0, 5.0)], s);
            p.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, 5.0)], s);
            p.line_segment([c + vec2(5.0, 5.0), c + vec2(5.0, 1.0)], s);
            p.line_segment([c + vec2(-1.0, 1.0), c + vec2(5.5, -5.5)], s);
            p.line_segment([c + vec2(1.5, -5.5), c + vec2(5.5, -5.5)], s);
            p.line_segment([c + vec2(5.5, -5.5), c + vec2(5.5, -1.5)], s);
        }
        // A window with its right side filled: the panel docked.
        "dock" => {
            p.rect_stroke(Rect::from_center_size(c, vec2(11.0, 10.0)), 1.0, s, egui::StrokeKind::Middle);
            p.rect_filled(Rect::from_min_max(c + vec2(1.5, -5.0), c + vec2(5.5, 5.0)), 0.0, t.text);
        }
        "newchat" => {
            // A speech bubble with a plus.
            ui.painter().rect_stroke(Rect::from_center_size(c, vec2(14.0, 11.0)), 2.5, s, egui::StrokeKind::Inside);
            ui.painter().line_segment([c + vec2(-2.5, 0.0), c + vec2(2.5, 0.0)], Stroke::new(1.2, t.text));
            ui.painter().line_segment([c + vec2(0.0, -2.5), c + vec2(0.0, 2.5)], Stroke::new(1.2, t.text));
        }
        "stopturn" => {
            ui.painter().rect_filled(Rect::from_center_size(c, vec2(9.0, 9.0)), 1.5, t.error);
        }
        _ => {}
    }
    crate::scenario::publish_rect(&format!("agent_chat:{kind}"), r);
    crate::scenario::publish_handle(&format!("agent_chat:{kind}"), r.center());
    resp.backing_on_hover_text(tip)
}

fn state_color(state: State) -> Color32 {
    let t = Tokens::get();
    match state {
        State::Ready | State::Busy => t.accent,
        State::NeedsAuth | State::Exited | State::Failed => t.warning,
        _ => t.text_dim,
    }
}

/// The folded panel: the chat icon (unfolds it), the agent's state, and a mark while something
/// waits for the user.
fn strip(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let r = ui.max_rect();
    let button = Rect::from_center_size(pos2(r.center().x, r.top() + 22.0), vec2(26.0, 26.0));
    let resp = ui.interact(button, Id::new("sc_agent_chat_unfold"), Sense::click());
    crate::scenario::publish_handle("agent_chat:unfold", button.center());
    if resp.hovered() {
        ui.painter().rect_filled(button, t.radius_sm, t.hover);
    }
    ui_kit::disclosure(ui.painter(), button.center(), false, t.text);
    let state = app.agent_chat.state();
    ui.painter().circle_filled(pos2(r.center().x, button.bottom() + 10.0), 3.5, state_color(state));
    let waiting = app.agent_chat.waiting();
    if waiting > 0 {
        let at = button.right_top() + vec2(-3.0, 3.0);
        ui.painter().circle_filled(at, 6.5, t.warning);
        ui.painter().text(
            at,
            Align2::CENTER_CENTER,
            waiting.min(9).to_string(),
            egui::FontId::proportional(crate::theme::Tokens::get().font_badge),
            t.panel,
        );
    }
    ui.painter().line_segment(
        if app.preferences.agent_chat_side == "left" { [r.right_top(), r.right_bottom()] } else { [r.left_top(), r.left_bottom()] },
        t.primary_border(t.border),
    );
    let tip = format!("{} · {}", tip(), state.label());
    if resp.backing_on_hover_text(tip).clicked() {
        app.preferences.agent_chat_minimized = false;
    }
}

/// The header: a grip to drag the panel to the other side, the title and state, Connect or Stop,
/// Pop out (or Dock, in its own window), Minimise and Close. In its own window the system moves
/// and minimises it, so there is no grip and no Minimise.
fn header(app: &mut SolveApp, ui: &mut egui::Ui, left: bool) {
    let t = Tokens::get();
    let state = app.agent_chat.state();
    let popped = app.preferences.agent_chat_popped;
    ui_kit::assistant_header(ui, |ui| {
        ui.horizontal(|ui| {
            let title = {
                let grip =
                    if popped { ui.allocate_exact_size(vec2(0.0, t.space_16 + t.space_2), Sense::hover()).0 } else { ui_kit::panel_grip(ui).rect };
                let l = ui.label(RichText::new(tr("ui.assistant")).semibold().color(t.text).size(t.font_heading));
                grip.union(l.rect)
            };
            let waiting = app.agent_chat.waiting();
            let s = if waiting > 0 { tr("ui.agent_chat_waiting").to_string() } else { state.label().to_string() };
            if ui.available_width() > t.space_24 * 9.0 {
                ui_kit::assistant_badge(
                    ui,
                    &s,
                    if waiting > 0 {
                        ui_kit::AssistantTone::Warning
                    } else {
                        match state {
                            State::Ready => ui_kit::AssistantTone::Success,
                            State::Busy => ui_kit::AssistantTone::Working,
                            State::NeedsAuth => ui_kit::AssistantTone::Warning,
                            State::Exited | State::Failed => ui_kit::AssistantTone::Error,
                            _ => ui_kit::AssistantTone::Neutral,
                        }
                    },
                );
            }
            // Drag the header across the window to dock on the other side.
            if !popped {
                let drag = ui.interact(title, Id::new("sc_agent_chat_grip"), Sense::drag());
                crate::scenario::publish_handle("agent_chat:grip", title.center());
                dock_drag(app, ui, &drag, left);
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if glyph_button(ui, "close", tr("ui.agent_chat_close")).clicked() {
                    app.agent_chat.open = false;
                }
                if !popped && glyph_button(ui, "minimize", tr("ui.agent_chat_minimize")).clicked() {
                    app.preferences.agent_chat_minimized = true;
                }
                #[cfg(not(target_arch = "wasm32"))]
                if glyph_button(
                    ui,
                    "newchat",
                    &format!("{} ({})", tr("ui.agent_chat_new"), if cfg!(target_os = "macos") { "Cmd+Shift+L" } else { "Ctrl+Shift+L" }),
                )
                .clicked()
                {
                    request_new_chat(app);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    let (kind, tip) = if popped { ("dock", "ui.agent_chat_dock") } else { ("pop_out", "ui.agent_chat_pop_out") };
                    if glyph_button(ui, kind, tr(tip)).clicked() {
                        app.preferences.agent_chat_popped = !popped;
                    }
                }
            });
        });
    });
}

/// While the header is dragged: the cursor grabs and the side it would dock on lights up; on
/// release over the other half of the window the panel moves there.
fn dock_drag(app: &mut SolveApp, ui: &egui::Ui, drag: &egui::Response, left: bool) {
    if drag.hovered() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grab);
    }
    let screen = ui.ctx().content_rect();
    let target = |x: f32| x < screen.center().x;
    if drag.dragged()
        && let Some(p) = ui.ctx().pointer_interact_pos()
    {
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
        let to_left = target(p.x);
        let available = app.agent_chat.dock_space.unwrap_or(screen);
        let zone = if Container::for_layout(app.ui.layout, Surface::Panel).floating() {
            floating_bounds(app, available, to_left)
        } else {
            let w = app.preferences.agent_chat_width.clamp(MIN_WIDTH, MAX_WIDTH).min(available.width());
            Rect::from_min_size(pos2(if to_left { available.left() } else { available.right() - w }, available.top()), vec2(w, available.height()))
        };
        crate::scenario::publish_rect("agent_chat:dock_hint", zone);
        let t = Tokens::get();
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Foreground, Id::new("sc_agent_chat_dock_hint")));
        painter.rect(zone, t.radius_xl, t.accent.gamma_multiply(0.18), Stroke::new(2.0, t.accent), egui::StrokeKind::Inside);
        painter.text(
            zone.center(),
            Align2::CENTER_CENTER,
            tr(if to_left { "ui.agent_chat_dock_left" } else { "ui.agent_chat_dock_right" }),
            egui::FontId::proportional(crate::theme::Tokens::get().font_heading),
            t.text,
        );
    }
    if drag.drag_stopped()
        && let Some(p) = ui.ctx().pointer_interact_pos().or_else(|| ui.ctx().pointer_latest_pos())
        && target(p.x) != left
    {
        app.preferences.agent_chat_side = if left { "right" } else { "left" }.into();
    }
}

fn body(app: &mut SolveApp, ui: &mut egui::Ui, left: bool) {
    header(app, ui, left);
    #[cfg(not(target_arch = "wasm32"))]
    ui_kit::assistant_controls(ui, |ui| switcher(app, ui));
    #[cfg(target_arch = "wasm32")]
    {
        ui.label(tr("ui.agent_chat_desktop_only"));
    }
    #[cfg(not(target_arch = "wasm32"))]
    desktop_body(app, ui);
}

#[cfg(not(target_arch = "wasm32"))]
fn desktop_body(app: &mut SolveApp, ui: &mut egui::Ui) {
    if app.agent_chat.detection.is_none() && app.agent_chat.detecting.is_none() {
        refresh(app, ui.ctx());
    }
    let state = app.agent_chat.state();
    if app.agent_chat.confirm_new {
        ui_kit::scope(ui, |ui| {
            inset(ui, |ui| {
                ui.set_width(ui.available_width());
                ui_kit::text(ui, tr("ui.agent_chat_new_confirm"), ui_kit::Text::Report("new_chat", false));
                ui.horizontal(|ui| {
                    let yes = ui_kit::button(ui, tr("ui.agent_chat_new"), ui_kit::Button::Normal);
                    crate::scenario::publish_handle("agent_chat:new_confirm", yes.rect.center());
                    if yes.clicked() {
                        new_chat(app);
                    }
                    if ui_kit::button(ui, tr("ui.agent_chat_cancel"), ui_kit::Button::Normal).clicked() {
                        app.agent_chat.confirm_new = false;
                    }
                });
            });
        });
    }
    // Discovery/install guidance must leave room for the pinned composer even when
    // several agents are listed, or when a long message grows the editor.
    egui::ScrollArea::vertical()
        .id_salt("sc_agent_chat_picker_scroll")
        .max_height(ui.available_height() / 3.0)
        .auto_shrink([false, true])
        .show(ui, |ui| picker(app, ui));
    if state == State::NeedsAuth {
        inset(ui, |ui| auth_box(app, ui));
    }
    // The input sits at the bottom of the panel and grows upwards: a long message makes its box
    // taller up to `edit_max`, then it scrolls, and the conversation takes what is left above. It
    // is a panel of its own at the bottom, as high as its content this frame, so the buttons under
    // the box are always in view.
    let edit_max = (ui.available_height() * 0.4).clamp(48.0, 160.0);
    let busy = state == State::Busy;
    // Enter sends (queues while the agent works); Ctrl/Cmd+Enter sends now; Esc stops. They are
    // read before the field sees them so no newline is typed.
    let focused = ui.memory(|m| m.has_focus(Id::new("sc_agent_chat_input")));
    let (steer, esc) = if focused {
        ui.input_mut(|i| (i.consume_key(egui::Modifiers::COMMAND, egui::Key::Enter), busy && i.consume_key(egui::Modifiers::NONE, egui::Key::Escape)))
    } else {
        (false, false)
    };
    let (mut send_clicked, mut stop_clicked, mut enter, mut pick_clicked) = (false, false, false, false);
    let mut remove_picked_at = None;
    let frame = Container::new(ui.ctx(), Surface::Viewport).edge_frame(egui::Color32::TRANSPARENT);
    let composer = Container::new(ui.ctx(), Surface::Viewport).edge(Id::new("sc_agent_chat_composer"), Edge::Bottom);
    composer.resizable(false).show_separator_line(false).frame(frame).show(ui, |ui| {
        let footer = ui_kit::assistant_composer_footer(ui, |ui| {
            let boxed = ui_kit::assistant_composer(ui, focused, |ui| {
                // What Pick context chose for this message, each item with an ×.
                if !app.agent_chat.picked_context.is_empty() {
                    let tray = app.agent_chat.picked_context.clone();
                    remove_picked_at = context_chips(ui, &tray, true);
                }
                // The files for the next message (#1744), as chips of their own.
                attachment_chips(app, ui);
                // Ctrl/Cmd+V: files and pictures on the clipboard attach; text goes to the box below.
                #[cfg(not(target_arch = "wasm32"))]
                if focused {
                    super::paste::handle(app, ui);
                }
                let r = egui::ScrollArea::vertical()
                    .id_salt("sc_agent_chat_input_scroll")
                    .max_height(edit_max)
                    // Grow the bottom panel to the editor's scroll limit instead of letting
                    // its previous short-message height clip the action rows below it.
                    .min_scrolled_height(edit_max)
                    .auto_shrink([false, true])
                    .show(ui, |ui| {
                        ui_kit::assistant_input(
                            ui,
                            Id::new("sc_agent_chat_input"),
                            &mut app.agent_chat.input,
                            tr(if busy { "ui.agent_chat_placeholder_busy" } else { "ui.agent_chat_placeholder" }),
                        )
                    })
                    .inner;
                crate::scenario::publish_rect("agent_chat:input", r.rect);
                // macOS shows a window's cursors only while it is the active window: over the text box of a
                // chat window that isn't, the pointer stays an arrow. Coming from the main window, the chat
                // window becomes active, so the text cursor shows and the first click places the caret.
                let (chat_active, main_active) =
                    (ui.ctx().input(|i| i.viewport().focused), ui.ctx().input_for(egui::ViewportId::ROOT, |i| i.viewport().focused));
                if take_focus_for_text(app.preferences.agent_chat_popped, r.hovered(), chat_active, main_active) {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::Focus);
                }
                enter = r.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter) && !i.modifiers.shift);
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), Tokens::get().control_height + Tokens::get().space_2),
                    Layout::left_to_right(Align::Center),
                    |ui| {
                        ui.set_min_height(Tokens::get().control_height + Tokens::get().space_2);
                        add_menu(app, ui);
                        let agent =
                            if state.running() { Some(app.agent_chat.running_id.clone()) } else { agents::current(app).map(|entry| entry.id) };
                        let mode = agent.as_deref().map_or(app.preferences.ai_permissions, |id| agents::mode_for(&app.preferences, id));
                        let permissions = ui
                            .allocate_ui(vec2(Tokens::get().space_24 * 4.0, Tokens::get().control_height), |ui| {
                                ui_kit::dropdown_menu(
                                    ui,
                                    "sc_agent_permissions",
                                    tr(match mode {
                                        agents::Mode::Ask => "ui.agent_chat_mode_ask",
                                        agents::Mode::Edits => "ui.agent_chat_mode_edits",
                                        agents::Mode::All => "ui.agent_chat_mode_all",
                                    }),
                                    |ui| {
                                        for choice in agents::Mode::ALL {
                                            if ui_kit::list_picker(ui, choice.id(), choice.label(), mode == choice).clicked() {
                                                if let Some(id) = &agent {
                                                    app.preferences.agent_permissions.insert(id.clone(), choice);
                                                } else {
                                                    app.preferences.ai_permissions = choice;
                                                }
                                            }
                                        }
                                    },
                                )
                            })
                            .inner;
                        let permissions = permissions.backing_on_hover_text(mode.label());
                        crate::scenario::publish_rect("agent_chat:permissions", permissions.rect);
                        crate::scenario::publish_handle("agent_chat:permissions", permissions.rect.center());
                        // A message waits for files still being read.
                        let reading = !app.agent_chat.loading.is_empty();
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let action = ui_kit::assistant_send(
                                ui,
                                busy || !reading,
                                busy,
                                tr(if busy { "ui.agent_chat_stop_turn_tip" } else { "ui.agent_chat_send" }),
                            );
                            if busy {
                                crate::scenario::publish_handle("agent_chat:stopturn", action.rect.center());
                                stop_clicked = action.clicked();
                                let queued = ui_kit::button(ui, tr("ui.agent_chat_queue"), ui_kit::Button::Enabled(!reading));
                                crate::scenario::publish_rect("agent_chat:send", queued.rect);
                                crate::scenario::publish_handle("agent_chat:send", queued.rect.center());
                                send_clicked = queued.clicked();
                            } else {
                                crate::scenario::publish_rect("agent_chat:send", action.rect);
                                crate::scenario::publish_handle("agent_chat:send", action.rect.center());
                                send_clicked = action.clicked();
                            }
                        });
                    },
                );
                ui.allocate_ui_with_layout(
                    vec2(ui.available_width(), Tokens::get().control_height),
                    Layout::left_to_right(Align::Center).with_main_wrap(true),
                    |ui| {
                        ui.set_min_height(Tokens::get().control_height);
                        // A picture of the view goes with the message to agents that take pictures.
                        let images = app.agent_chat.images || !state.running();
                        let attach = ui
                            .add_enabled_ui(images, |ui| ui_kit::checkbox(ui, tr("ui.agent_chat_attach_view"), &mut app.agent_chat.attach_view))
                            .inner
                            .backing_on_hover_text(tr("ui.agent_chat_attach_view_tip"))
                            .backing_on_disabled_hover_text(tr("ui.agent_chat_no_images"));
                        crate::scenario::publish_handle("agent_chat:attach", attach.rect.center());
                        // What is selected in the model goes with the message: text for the agent,
                        // chips in the conversation.
                        let selected = selected_count(app);
                        let label = if selected > 0 {
                            format!("{} ({selected})", tr("ui.agent_chat_attach_selection"))
                        } else {
                            tr("ui.agent_chat_attach_selection").to_string()
                        };
                        let pick = ui_kit::checkbox(ui, &label, &mut app.agent_chat.attach_selection)
                            .backing_on_hover_text(tr("ui.agent_chat_attach_selection_tip"));
                        crate::scenario::publish_handle("agent_chat:attach_selection", pick.rect.center());
                        // Choose faces, edges, features and bodies on purpose, then OK.
                        let picker = ui_kit::button(ui, tr("ui.agent_chat_pick_context"), ui_kit::Button::Enabled(app.dialog.is_none()))
                            .backing_on_hover_text(tr("ui.agent_chat_pick_context_tip"));
                        crate::scenario::publish_handle("agent_chat:pick_context", picker.rect.center());
                        pick_clicked = picker.clicked();
                        if let Some(turn) = &app.agent_chat.turn {
                            let n = turn.changes.to_string();
                            let count = match app.preferences.agent_chat_turn_limit {
                                0 => trf("ui.agent_chat_changes_n", &[("n", &n)]),
                                limit => trf("ui.agent_chat_changes_of", &[("n", &n), ("limit", &limit.to_string())]),
                            };
                            ui.label(RichText::new(count).color(Tokens::get().text_dim).size(Tokens::get().font_small));
                        }
                    },
                );
            });
            crate::scenario::publish_rect("agent_chat:composer_box", boxed.response.rect);
        });
        crate::scenario::publish_rect("agent_chat:composer_footer", footer.response.rect);
    });
    if let Some(at) = remove_picked_at {
        remove_picked(app, at);
    }
    if pick_clicked {
        start_pick(app);
    }
    egui::ScrollArea::vertical()
        .id_salt("sc_agent_chat_scroll")
        .max_height(ui.available_height())
        .auto_shrink([false, false])
        .stick_to_bottom(true)
        .show(ui, |ui| {
            egui::Frame::NONE.inner_margin(egui::Margin::same(Tokens::get().space_12 as i8)).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = Tokens::get().space_10;
                conversation(app, ui);
            });
        });
    if esc || stop_clicked {
        cancel(app);
    }
    // A message waits for files still being read, and files alone make a message.
    let reading = !app.agent_chat.loading.is_empty();
    let something = !app.agent_chat.input.trim().is_empty() || !app.agent_chat.attachments.is_empty();
    if reading {
        // Enter waits with the button.
    } else if (send_clicked || enter || steer) && something {
        let text = std::mem::take(&mut app.agent_chat.input).trim_end_matches('\n').to_string();
        send(app, ui.ctx(), text);
        if steer {
            send_now(app, ui.ctx());
        }
    } else if steer {
        // Nothing typed: Ctrl+Enter sends what is already queued.
        send_now(app, ui.ctx());
    }
}

/// The header's second row: the agent this conversation uses (a compact switcher), the agent's
/// own mode when it has modes, and the state of its login.
#[cfg(not(target_arch = "wasm32"))]
fn switcher(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let all = agents::entries(app);
    let current = agents::current(app);
    // An advertised (even empty) config list replaces the older ACP model/mode pickers.
    let modern = app.agent_chat.config_options.is_some();
    ui.horizontal_wrapped(|ui| {
        let label = current.as_ref().map_or_else(
            || tr(if app.agent_chat.detecting.is_some() { "ui.agent_chat_detecting" } else { "ui.agent_chat_none_found_short" }).to_string(),
            |e| e.name.clone(),
        );
        let models = !modern && models_shown(app);
        let providers = if models { model_providers(app) } else { Vec::new() };
        let w = (ui.available_width()
            - if modern || app.agent_chat.modes.is_empty() { 4.0 } else { 116.0 }
            - if models { 146.0 } else { 0.0 }
            - if providers.is_empty() { 0.0 } else { 146.0 })
        .clamp(90.0, 260.0);
        let mut picked: Option<String> = None;
        let mut add = false;
        let combo = ui
            .allocate_ui(vec2(w, t.control_height), |ui| {
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                ui_kit::dropdown_menu(ui, "sc_agent_switch", &label, |ui| {
                    for e in &all {
                        let sel = current.as_ref().is_some_and(|c| c.id == e.id);
                        let r = ui_kit::list_picker(ui, &e.id, &e.name, sel).backing_on_hover_text(format!("{} {}", e.detail(), e.login));
                        crate::scenario::publish_handle(&format!("agent_chat:agent:{}", e.id), r.rect.center());
                        if r.clicked() {
                            picked = Some(e.id.clone());
                        }
                    }
                    ui.separator();
                    if ui_kit::list_picker(ui, "add", tr("ui.ai_add_agent"), false).clicked() {
                        add = true;
                    }
                    if ui_kit::list_picker(ui, "settings", tr("ui.ai_settings"), false).clicked() {
                        crate::prefs::open_at(app, crate::prefs::PAGE_ASSISTANT);
                    }
                })
            })
            .inner;
        crate::scenario::publish_rect("agent_chat:switcher", combo.rect);
        crate::scenario::publish_handle("agent_chat:switcher", combo.rect.center());
        combo.backing_on_hover_text(tr("ui.ai_switch_tip"));
        if add {
            super::settings::open_add(app);
        }
        if let Some(id) = picked
            && current.as_ref().is_none_or(|c| c.id != id)
        {
            disconnect(app);
            app.agent_chat.agent_id = Some(id);
        }
        // The agent's own modes (ACP `modes`): its own tools follow them.
        if !modern && !app.agent_chat.modes.is_empty() {
            let modes = app.agent_chat.modes.clone();
            let cur = modes.iter().find(|(id, _)| *id == app.agent_chat.mode).map_or_else(|| app.agent_chat.mode.clone(), |(_, n)| n.clone());
            let mut pick: Option<String> = None;
            let r = ui
                .allocate_ui(vec2(t.form_label_width, t.control_height), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui_kit::dropdown_menu(ui, "sc_agent_mode", &cur, |ui| {
                        for (id, name) in &modes {
                            if ui_kit::list_picker(ui, id, name, *id == app.agent_chat.mode).clicked() {
                                pick = Some(id.clone());
                            }
                        }
                    })
                })
                .inner;
            crate::scenario::publish_rect("agent_chat:mode", r.rect);
            crate::scenario::publish_handle("agent_chat:mode", r.rect.center());
            r.backing_on_hover_text(tr("ui.ai_agent_mode_tip"));
            if let Some(m) = pick {
                set_mode(app, &m);
            }
        }
        // Models from several providers: pick the provider first, then one of its models.
        if let Some(shown) = shown_provider(app).filter(|_| !providers.is_empty()) {
            let mut pick: Option<String> = None;
            let width = picker_width(ui, &shown);
            let r = ui
                .allocate_ui(vec2(width, t.control_height), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui_kit::dropdown_menu(ui, "sc_agent_provider", &shown, |ui| {
                        for (name, count) in &providers {
                            let row = ui_kit::list_picker(ui, &format!("provider:{name}"), name, *name == shown)
                                .backing_on_hover_text(trf("ui.agent_chat_provider_models", &[("count", &count.to_string())]));
                            crate::scenario::publish_handle(&format!("agent_chat:provider:{name}"), row.rect.center());
                            if row.clicked() {
                                pick = Some(name.clone());
                            }
                        }
                    })
                })
                .inner;
            crate::scenario::publish_rect("agent_chat:provider", r.rect);
            crate::scenario::publish_handle("agent_chat:provider", r.rect.center());
            r.backing_on_hover_text(tr("ui.agent_chat_provider_tip"));
            if let Some(p) = pick {
                set_model_provider(app, &p);
            }
        }
        // The agent's models (ACP `models`, or an endpoint's list): shown only when it reports some.
        if models {
            let list = listed_models(app);
            let cur = list.iter().find(|(m, _)| m.id == app.agent_chat.model).map_or_else(
                || if providers.is_empty() { app.agent_chat.model.clone() } else { tr("ui.agent_chat_choose_model").to_string() },
                |(_, name)| name.clone(),
            );
            let mut pick: Option<String> = None;
            let width = if providers.is_empty() { t.form_label_width } else { picker_width(ui, &cur) };
            let r = ui
                .allocate_ui(vec2(width, t.control_height), |ui| {
                    ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Truncate);
                    ui_kit::dropdown_menu(ui, "sc_agent_model", &cur, |ui| {
                        for (m, name) in &list {
                            let row = ui_kit::list_picker(ui, &m.id, name, m.id == app.agent_chat.model);
                            let row = if m.description.is_empty() { row } else { row.backing_on_hover_text(&m.description) };
                            crate::scenario::publish_handle(&format!("agent_chat:model:{}", m.id), row.rect.center());
                            if row.clicked() {
                                pick = Some(m.id.clone());
                            }
                        }
                    })
                })
                .inner;
            crate::scenario::publish_rect("agent_chat:model", r.rect);
            crate::scenario::publish_handle("agent_chat:model", r.rect.center());
            r.backing_on_hover_text(tr("ui.agent_chat_model_tip"));
            if let Some(m) = pick {
                set_model(app, &m);
            }
        }
    });
    if let Some(warning) = settings::model_warning(app) {
        ui.add(egui::Label::new(RichText::new(warning).color(t.warning).size(t.font_small)).wrap());
    }
    if let Some(options) = app.agent_chat.config_options.clone() {
        config_selects(app, ui, &options);
    }
    ui.horizontal(|ui| {
        let b = if app.agent_chat.state().running() {
            ui_kit::button(ui, tr("ui.agent_chat_stop"), ui_kit::Button::Normal).backing_on_hover_text(tr("ui.agent_chat_stop_tip"))
        } else {
            let label = if app.agent_chat.state() == State::Failed { "ui.agent_chat_retry" } else { "ui.agent_chat_connect" };
            ui_kit::button(ui, tr(label), ui_kit::Button::Normal)
        };
        crate::scenario::publish_handle("agent_chat:connect", b.rect.center());
        if b.clicked() {
            if app.agent_chat.state().running() {
                disconnect(app);
            } else {
                connect(app, ui.ctx());
            }
        }
    });
    if let Some(a) = &app.agent_chat.auth_status {
        ui.label(RichText::new(a).color(t.text_dim).size(crate::theme::Tokens::get().font_small));
    }
}

#[cfg(not(target_arch = "wasm32"))]
const FLATPAK_OVERRIDE: &str = "flatpak override --user io.tangencylabs.solidcraft --filesystem=~/.nvm:ro --filesystem=~/.local/bin:ro --filesystem=~/.local/share/claude:ro --filesystem=~/.claude --filesystem=~/.claude.json --share=network";

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn no_agents_message() -> String {
    if std::env::var_os("FLATPAK_ID").is_some() { tr("ui.agent_chat_flatpak_none_found") } else { tr("ui.agent_chat_none_found") }.to_string()
}

#[cfg(not(target_arch = "wasm32"))]
pub(super) fn no_agents(ui: &mut egui::Ui) {
    let flatpak = std::env::var_os("FLATPAK_ID").is_some();
    let style = if flatpak { ui_kit::Text::Report("none", false) } else { ui_kit::Text::Note("none") };
    ui_kit::text(ui, &no_agents_message(), style);
    if flatpak {
        // The Copy button comes before the long command so it shows without scrolling.
        let copy = ui_kit::button(ui, tr("ui.agent_chat_copy"), ui_kit::Button::Small);
        crate::scenario::publish_handle("agent_chat:flatpak_override", copy.rect.center());
        if copy.clicked() {
            ui.ctx().copy_text(FLATPAK_OVERRIDE.to_string());
        }
        ui_kit::text(ui, FLATPAK_OVERRIDE, ui_kit::Text::Report("flatpak_override", false));
    }
}

/// The width of a provider or model picker: its label's, from the usual picker width up to 200
/// points and never wider than the panel, so a short name is read whole and a long one truncates.
#[cfg(not(target_arch = "wasm32"))]
fn picker_width(ui: &egui::Ui, label: &str) -> f32 {
    let t = Tokens::get();
    let label: String = label.chars().take(64).collect();
    let text = ui.painter().layout_no_wrap(label, egui::FontId::proportional(t.font_label), t.text).size().x + 34.0;
    let most = ui.max_rect().width().clamp(t.form_label_width, 200.0);
    text.clamp(t.form_label_width, most)
}

/// An agent's select options are authoritative. Compact value pickers share a strip below the
/// agent switcher; only a narrow chat wraps them, rather than spending two full rows per option.
#[cfg(not(target_arch = "wasm32"))]
fn config_selects(app: &mut SolveApp, ui: &mut egui::Ui, options: &[acp::ConfigOption]) {
    let t = Tokens::get();
    let pending = app.agent_chat.config_pending;
    let strip_width = ui.available_width().min(ui.max_rect().width()).max(0.0);
    let mut row = Vec::new();
    let mut used = 0.0;
    for option in options {
        let current = option.options.iter().find(|choice| choice.value == option.current_value);
        let selected = current.map_or(option.current_value.as_str(), |choice| choice.name.as_str());
        let preview = compact_text(selected, 32);
        let preferred = ui.painter().layout_no_wrap(preview, egui::FontId::proportional(t.font_label), t.text).size().x + 34.0;
        let width = preferred.clamp(76.0, strip_width.clamp(76.0, 200.0));
        let spacing = if row.is_empty() { 0.0 } else { ui.spacing().item_spacing.x };
        if !row.is_empty() && used + spacing + width > strip_width {
            ui.horizontal(|ui| {
                for (option, width) in row.drain(..) {
                    config_select(app, ui, option, width, pending);
                }
            });
            used = 0.0;
        }
        used += if used == 0.0 { width } else { spacing + width };
        row.push((option, width));
    }
    if !row.is_empty() {
        ui.horizontal(|ui| {
            for (option, width) in row {
                config_select(app, ui, option, width, pending);
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn config_select(app: &mut SolveApp, ui: &mut egui::Ui, option: &acp::ConfigOption, width: f32, pending: bool) {
    let t = Tokens::get();
    let current = option.options.iter().find(|choice| choice.value == option.current_value);
    let selected = current.map_or(option.current_value.as_str(), |choice| choice.name.as_str());
    // Bound both the measurement and the visible label: agent-provided text may be huge.
    // Width follows the current value, leaving short choices room to sit on one line.
    let font = egui::FontId::proportional(t.font_label);
    let mut chars = 32;
    let displayed = loop {
        let text = compact_text(selected, chars);
        if chars == 1 || ui.painter().layout_no_wrap(text.clone(), font.clone(), t.text).size().x <= width - 36.0 {
            break text;
        }
        chars -= 1;
    };
    let name = if option.name.is_empty() { &option.id } else { &option.name };
    let tip = [name.as_str(), option.description.as_str(), current.map_or("", |choice| choice.description.as_str())]
        .into_iter()
        .filter(|part| !part.is_empty())
        .collect::<Vec<_>>()
        .join("\n");
    let mut picked = None;
    let response = ui
        .add_enabled_ui(!pending && !option.options.is_empty(), |ui| {
            ui.allocate_ui(vec2(width, t.control_height), |ui| {
                ui_kit::dropdown_menu(ui, ("sc_agent_config", &option.id), &displayed, |ui| {
                    let mut previous_group: Option<&str> = None;
                    for choice in &option.options {
                        let group = choice.group.as_deref().filter(|g| !g.is_empty());
                        if let Some(group) = group
                            && previous_group != Some(group)
                        {
                            ui.label(RichText::new(compact_text(group, 32)).color(t.text_dim).size(t.font_small)).backing_on_hover_text(group);
                        }
                        previous_group = group;
                        let row = ui_kit::list_picker(ui, &choice.value, &compact_text(&choice.name, 32), choice.value == option.current_value);
                        let row = row.backing_on_hover_text(
                            [choice.name.as_str(), choice.description.as_str()]
                                .into_iter()
                                .filter(|part| !part.is_empty())
                                .collect::<Vec<_>>()
                                .join("\n"),
                        );
                        crate::scenario::publish_handle(&format!("agent_chat:config:{}:{}", option.id, choice.value), row.rect.center());
                        if option.category == "model" {
                            crate::scenario::publish_handle(&format!("agent_chat:model:{}", choice.value), row.rect.center());
                        }
                        if row.clicked() {
                            picked = Some(choice.value.clone());
                        }
                    }
                })
            })
            .inner
        })
        .inner;
    crate::scenario::publish_handle(&format!("agent_chat:config:{}", option.id), response.rect.center());
    crate::scenario::publish_rect(&format!("agent_chat:config:{}", option.id), response.rect);
    if option.category == "model" {
        crate::scenario::publish_handle("agent_chat:model", response.rect.center());
    } else if option.category == "mode" {
        crate::scenario::publish_handle("agent_chat:mode", response.rect.center());
    }
    response
        .widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::ComboBox, !pending && !option.options.is_empty(), format!("{name}: {selected}")));
    response.backing_on_hover_text(tip);
    if let Some(value) = picked {
        super::set_config_option(app, &option.id, &value);
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn compact_text(text: &str, max_chars: usize) -> String {
    let mut chars = text.chars();
    let short: String = chars.by_ref().take(max_chars).collect();
    if chars.next().is_some() { format!("{short}…") } else { short }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod config_ui_tests {
    use super::*;

    fn option(id: &str, category: &str, choices: &[(&str, &str, Option<&str>)]) -> acp::ConfigOption {
        acp::ConfigOption {
            id: id.into(),
            name: id.into(),
            description: format!("Choose {id}"),
            category: category.into(),
            current_value: choices.first().map_or("", |choice| choice.0).into(),
            options: choices
                .iter()
                .map(|(value, name, group)| acp::ConfigChoice {
                    value: (*value).into(),
                    name: (*name).into(),
                    description: format!("Description for {name}"),
                    group: group.map(str::to_owned),
                })
                .collect(),
        }
    }

    fn draw_switcher(app: &mut SolveApp, ctx: &egui::Context, width: f32) {
        crate::scenario::clear_handles();
        let screen = Rect::from_min_size(pos2(0.0, 0.0), vec2(width, 700.0));
        let _ = ctx.run_ui(egui::RawInput { screen_rect: Some(screen), ..Default::default() }, |ui| {
            ui.allocate_ui(vec2(width - 16.0, 600.0), |ui| {
                ui_kit::scope(ui, |ui| switcher(app, ui));
            });
        });
    }

    #[test]
    fn config_options_share_one_row_when_they_fit_and_wrap_without_legacy_duplicates() {
        assert_eq!(compact_text("Énergie très élevée", 7), "Énergie…");
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        // Make legacy provider/model pickers eligible so their absence is meaningful.
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.modes = vec![("legacy".into(), "Legacy mode".into())];
        app.agent_chat.models = vec![
            acp::ModelInfo { id: "old".into(), name: "North · Old model".into(), description: String::new() },
            acp::ModelInfo { id: "other".into(), name: "South · Other model".into(), description: String::new() },
        ];
        assert!(models_shown(&app));
        assert_eq!(model_providers(&app).len(), 2);
        app.agent_chat.config_options = Some(vec![
            option(
                "engine",
                "model",
                &[("fast", "Énergie très élevée pour une conception détaillée", Some("Standard")), ("large", "Large", Some("Advanced"))],
            ),
            option("thought", "thought_level", &[("low", "Max", None), ("high", "High", None)]),
            option("workflow", "mode", &[("plan", "Build", None)]),
        ]);
        let ctx = crate::fonts::test_context();
        draw_switcher(&mut app, &ctx, 760.0);
        let at = |name| crate::scenario::handle_at(name).expect("config picker handle");
        let agent = at("agent_chat:switcher");
        let model = at("agent_chat:config:engine");
        let effort = at("agent_chat:config:thought");
        let mode = at("agent_chat:config:workflow");
        assert!(agent.y < model.y && (model.y - effort.y).abs() < 2.0 && (effort.y - mode.y).abs() < 2.0);
        assert!(model.x < effort.x && effort.x < mode.x && mode.x < 760.0);
        assert_eq!(at("agent_chat:model"), model);
        assert_eq!(at("agent_chat:mode"), mode);
        assert_eq!(crate::scenario::handles("agent_chat:mode").iter().filter(|(name, _)| name == "agent_chat:mode").count(), 1);
        assert_eq!(crate::scenario::handles("agent_chat:model").iter().filter(|(name, _)| name == "agent_chat:model").count(), 1);
        assert!(crate::scenario::handle_at("agent_chat:provider").is_none());
        assert!(crate::scenario::handle_at("agent_chat:model:old").is_none());
        assert!(crate::scenario::handle_at("agent_chat:model:other").is_none());

        draw_switcher(&mut app, &ctx, 280.0);
        let model = at("agent_chat:config:engine");
        let effort = at("agent_chat:config:thought");
        let mode = at("agent_chat:config:workflow");
        assert!(model.y < effort.y && (effort.y - mode.y).abs() < 2.0, "only the long model needs its own line: {model:?}, {effort:?}, {mode:?}");
        for id in ["engine", "thought", "workflow"] {
            let rect = crate::scenario::rect_of(&format!("agent_chat:config:{id}")).expect("config control bounds");
            assert!(rect.left() >= 0.0 && rect.right() <= 280.0, "{id} exceeds the narrow panel: {rect:?}");
        }

        app.agent_chat.config_options = Some(vec![option("engine", "model", &[("fast", "Fast", None)])]);
        app.agent_chat.config_pending = true;
        draw_switcher(&mut app, &ctx, 280.0);
        assert!(crate::scenario::handle_at("agent_chat:config:engine").is_some());
        assert!(crate::scenario::handle_at("agent_chat:config:thought").is_none());
        assert!(crate::scenario::handle_at("agent_chat:config:workflow").is_none());
        assert!(crate::scenario::handle_at("agent_chat:switcher").is_some());

        app.agent_chat.config_options = Some(vec![]);
        draw_switcher(&mut app, &ctx, 280.0);
        assert!(crate::scenario::handle_at("agent_chat:switcher").is_some());
        assert!(crate::scenario::handle_at("agent_chat:config:engine").is_none());
        assert!(crate::scenario::handle_at("agent_chat:mode").is_none());
        assert!(crate::scenario::handle_at("agent_chat:provider").is_none());
        assert!(crate::scenario::handle_at("agent_chat:model").is_none());
    }
}

/// What is drawn under the agent switcher keeps the switcher's side margin (#10172): it starts
/// where the switcher's fields start, not at the panel's edge.
#[cfg(not(target_arch = "wasm32"))]
fn inset<R>(ui: &mut egui::Ui, add: impl FnOnce(&mut egui::Ui) -> R) -> R {
    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(Tokens::get().space_12 as i8, 0)).show(ui, add).inner
}

/// Before the first connection: what's installed, how to install the rest, and Add agent.
#[cfg(not(target_arch = "wasm32"))]
fn picker(app: &mut SolveApp, ui: &mut egui::Ui) {
    let open = app.agent_chat.picking.unwrap_or(!app.agent_chat.state().running() && app.agent_chat.items.is_empty());
    if !open {
        return;
    }
    inset(ui, |ui| agent_list(app, ui));
}

/// [`picker`]'s content: the agents found, Refresh and Add agent, and what isn't installed.
#[cfg(not(target_arch = "wasm32"))]
fn agent_list(app: &mut SolveApp, ui: &mut egui::Ui) {
    let all = agents::entries(app);
    let current = agents::current(app);
    ui.horizontal(|ui| {
        ui_kit::text(ui, tr("ui.agent_chat_pick"), ui_kit::Text::Inline);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            let refreshing = app.agent_chat.detecting.is_some();
            if ui_kit::button(ui, tr("ui.agent_chat_refresh"), ui_kit::Button::Enabled(!refreshing)).clicked() {
                refresh(app, ui.ctx());
            }
            if ui_kit::button(ui, tr("ui.ai_add_agent"), ui_kit::Button::Small).clicked() {
                super::settings::open_add(app);
            }
        });
    });
    if app.agent_chat.detecting.is_some() {
        ui_kit::text(ui, tr("ui.agent_chat_detecting"), ui_kit::Text::Note("detecting"));
    }
    for e in &all {
        let selected = current.as_ref().is_some_and(|c| c.id == e.id);
        let resp = ui_kit::list_picker(ui, &e.id, &e.name, selected).backing_on_hover_text(format!("{} {}", e.detail(), e.login));
        crate::scenario::publish_handle(&format!("agent_chat:pick:{}", e.id), resp.rect.center());
        if resp.clicked() {
            app.agent_chat.agent_id = Some(e.id.clone());
        }
    }
    // In the Flatpak the hint already says nothing was found, so the install list below is
    // headed "Not installed" instead of repeating it.
    let flatpak_hint = all.is_empty() && app.agent_chat.detecting.is_none() && std::env::var_os("FLATPAK_ID").is_some();
    if flatpak_hint {
        no_agents(ui);
    }
    let missing: Vec<discover::Missing> = app.agent_chat.detection.as_ref().map(|d| d.missing.clone()).unwrap_or_default();
    if !missing.is_empty() {
        let heading = if all.is_empty() && !flatpak_hint { tr("ui.agent_chat_none_found") } else { tr("ui.agent_chat_not_installed") };
        ui_kit::section(ui, "sc_agent_missing", heading, all.is_empty(), |ui| {
            for m in missing {
                ui.horizontal(|ui| {
                    ui_kit::text(ui, m.name, ui_kit::Text::Inline);
                    if ui_kit::button(ui, tr("ui.agent_chat_copy"), ui_kit::Button::Small).backing_on_hover_text(m.install).clicked() {
                        ui.ctx().copy_text(m.install.to_string());
                    }
                });
                ui_kit::text(ui, m.install, ui_kit::Text::Report(m.name, false));
            }
        });
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn auth_box(app: &mut SolveApp, ui: &mut egui::Ui) {
    ui_kit::section(ui, "authentication", tr("ui.agent_chat_auth_title"), true, |ui| {
        ui.set_width(ui.available_width());
        ui_kit::text(ui, tr("ui.agent_chat_auth_needed"), ui_kit::Text::Note("auth_needed"));
        let methods = app.agent_chat.auth.clone();
        let base = app.agent_chat.argv.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" ");
        if methods.is_empty() {
            ui_kit::text(ui, tr("ui.agent_chat_auth_none"), ui_kit::Text::Note("auth_none"));
        }
        for m in methods {
            ui.add_space(crate::theme::Tokens::get().space_4);
            let desc = m.description.trim();
            ui_kit::text(
                ui,
                &if desc.is_empty() || desc == m.name { m.name.clone() } else { format!("{} — {desc}", m.name) },
                ui_kit::Text::Report(&m.id, false),
            );
            match &m.kind {
                acp::AuthKind::Terminal { args, .. } => {
                    ui_kit::text(ui, tr("ui.agent_chat_auth_terminal"), ui_kit::Text::Note("terminal"));
                    let cmd = format!("{base} {}", args.iter().map(|a| quote(a)).collect::<Vec<_>>().join(" "));
                    ui.horizontal_wrapped(|ui| {
                        ui_kit::text(ui, &cmd, ui_kit::Text::Report("auth_command", false));
                        if ui_kit::button(ui, tr("ui.agent_chat_copy"), ui_kit::Button::Small).clicked() {
                            ui.ctx().copy_text(cmd.clone());
                        }
                    });
                }
                acp::AuthKind::Agent => {
                    let b = ui_kit::button(ui, trf("ui.agent_chat_auth_agent", &[("name", &m.name)]), ui_kit::Button::Normal);
                    crate::scenario::publish_handle(&format!("agent_chat:auth:{}", m.id), b.rect.center());
                    if b.clicked() {
                        app.agent_chat.authenticate(&m.id);
                    }
                }
                acp::AuthKind::EnvVar { vars } => {
                    ui_kit::text(ui, &trf("ui.agent_chat_auth_env", &[("vars", &vars.join(", "))]), ui_kit::Text::Report("auth_environment", false));
                }
            }
        }
        ui.add_space(crate::theme::Tokens::get().space_4);
        if ui_kit::button(ui, tr("ui.agent_chat_auth_retry"), ui_kit::Button::Normal).clicked() {
            connect(app, ui.ctx());
        }
    });
}

#[cfg(not(target_arch = "wasm32"))]
fn conversation(app: &mut SolveApp, ui: &mut egui::Ui) {
    let t = Tokens::get();
    let mut answer: Option<(usize, Option<String>)> = None;
    // Allow (true), Deny (false), Always allow (None).
    let mut approval: Option<(u64, Option<bool>)> = None;
    let mut undo: Option<usize> = None;
    let mut open_gallery: Option<Gallery> = None;
    let current_doc = app.session.doc.clone();
    let units = summary::Units { units: &app.session.doc.units, decimals: app.preferences.length_decimals };
    for (n, item) in app.agent_chat.items.iter().enumerate() {
        match item {
            Item::User { text, image, pictures, context, files } => {
                ui_kit::assistant_bubble(ui, |ui| {
                    if !text.is_empty() {
                        ui.add(egui::Label::new(RichText::new(text).color(t.text)).selectable(true).wrap());
                    }
                    if !pictures.is_empty() {
                        if let Some(at) = picture_strip(ui, n, pictures) {
                            open_gallery = Some(Gallery { pictures: pictures.clone(), at });
                        }
                    } else if *image {
                        // Taken when the agent starts on the message: nothing to show yet.
                        ui.label(RichText::new(tr("ui.agent_chat_view_attached")).color(t.text_dim).size(t.font_small));
                    }
                    context_chips(ui, context, false);
                    files_line(ui, files.iter().map(String::as_str));
                });
            }
            Item::Agent { thought, text } => {
                if *thought {
                    ui.add(egui::Label::new(RichText::new(text).italics().color(t.text_dim)).selectable(true).wrap());
                } else {
                    ui_kit::assistant_prose(ui, |ui| super::markdown_view::show(ui, n, text));
                }
            }
            Item::Tool { .. } => {
                if n > 0 && matches!(app.agent_chat.items.get(n - 1), Some(Item::Tool { .. })) {
                    continue;
                }
                ui_kit::assistant_tool_frame(ui, false, |ui| {
                    ui.spacing_mut().item_spacing.y = Tokens::get().space_2;
                    for (offset, next) in app.agent_chat.items.iter().skip(n).take_while(|next| matches!(next, Item::Tool { .. })).enumerate() {
                        if let Item::Tool { title, kind, status, input, output, .. } = next {
                            let words = summary::summary(title, input.as_ref(), output.as_deref(), *status == Some(ToolStatus::Completed), units);
                            let call = ToolCall {
                                title,
                                summary: words.as_deref(),
                                kind,
                                status: *status,
                                input: input.as_ref(),
                                output: output.as_deref(),
                            };
                            tool_card(ui, n + offset, &call);
                        }
                    }
                });
            }
            Item::Plan(entries) => {
                for (c, s) in entries {
                    let (color, filled) = match s.as_str() {
                        "completed" => (t.accent, true),
                        "in_progress" => (t.warning, true),
                        _ => (t.text_dim, false),
                    };
                    ui.horizontal(|ui| {
                        status_dot(ui, color, filled);
                        ui.label(RichText::new(c).color(t.text_dim));
                    });
                }
            }
            Item::Permission { title, options, answer: given, .. } => {
                ui_kit::assistant_approval(ui, &trf("ui.agent_chat_permission", &[("title", title)]), |ui| match given {
                    Some(a) => {
                        ui.label(RichText::new(trf("ui.agent_chat_answered", &[("option", a)])).color(t.text_dim));
                    }
                    None => {
                        ui_kit::assistant_actions(ui, |ui| {
                            for o in options.iter().filter(|o| o.kind == "allow_once").chain(options.iter().filter(|o| o.kind == "allow_always")) {
                                let r = if o.kind == "allow_once" {
                                    ui_kit::primary_button(ui, &o.name)
                                } else {
                                    ui_kit::button(ui, &o.name, ui_kit::Button::Normal)
                                };
                                crate::scenario::publish_handle(&format!("agent_chat:permission:{}", o.id), r.rect.center());
                                if r.clicked() {
                                    answer = Some((n, Some(o.id.clone())));
                                }
                            }
                            ui.with_layout(Layout::left_to_right(Align::Center).with_main_wrap(true), |ui| {
                                for o in options.iter().filter(|o| !matches!(o.kind.as_str(), "allow_once" | "allow_always")) {
                                    let r = ui_kit::button(ui, &o.name, ui_kit::Button::Normal);
                                    crate::scenario::publish_handle(&format!("agent_chat:permission:{}", o.id), r.rect.center());
                                    if r.clicked() {
                                        answer = Some((n, Some(o.id.clone())));
                                    }
                                }
                            });
                        });
                    }
                });
            }
            Item::Approval { id, title, tool, answer: given } => {
                ui_kit::assistant_approval(ui, tr("ui.agent_chat_approval"), |ui| {
                    ui_kit::assistant_tool_detail(ui, |ui| {
                        ui.add(egui::Label::new(RichText::new(title).monospace().size(t.font_caption)).selectable(true).wrap());
                    });
                    match given {
                        Some(true) => {
                            ui.label(RichText::new(tr("ui.agent_chat_allowed")).color(t.text_dim));
                        }
                        Some(false) => {
                            ui.label(RichText::new(tr("ui.agent_chat_denied")).color(t.text_dim));
                        }
                        None => {
                            ui_kit::assistant_actions(ui, |ui| {
                                let allow = ui_kit::primary_button(ui, tr("ui.agent_chat_allow_once"));
                                let always = ui_kit::button(ui, tr("ui.ai_always_allow"), ui_kit::Button::Normal)
                                    .backing_on_hover_text(trf("ui.ai_always_allow_tip", &[("tool", tool)]));
                                let deny = ui
                                    .with_layout(Layout::left_to_right(Align::Center), |ui| {
                                        ui_kit::button(ui, tr("ui.agent_chat_deny"), ui_kit::Button::Normal)
                                    })
                                    .inner;
                                crate::scenario::publish_handle("agent_chat:allow", allow.rect.center());
                                crate::scenario::publish_handle("agent_chat:always", always.rect.center());
                                crate::scenario::publish_handle("agent_chat:deny", deny.rect.center());
                                if allow.clicked() {
                                    approval = Some((*id, Some(true)));
                                }
                                if always.clicked() {
                                    approval = Some((*id, None));
                                }
                                if deny.clicked() {
                                    approval = Some((*id, Some(false)));
                                }
                            });
                        }
                    }
                });
            }
            Item::Changes { steps, doc, undone } => {
                ui.horizontal(|ui| {
                    let text = trf("ui.agent_chat_changes", &[("n", &steps.to_string())]);
                    ui.label(RichText::new(text).color(t.text_dim).size(crate::theme::Tokens::get().font_caption));
                    if *undone {
                        ui.label(RichText::new(tr("ui.agent_chat_undone")).color(t.text_dim).size(crate::theme::Tokens::get().font_caption));
                    } else if Arc::ptr_eq(doc, &current_doc) {
                        let b =
                            ui_kit::button(ui, tr("ui.agent_chat_undo_changes"), ui_kit::Button::Normal).backing_on_hover_text(tr("ui.ai_undo_tip"));
                        crate::scenario::publish_handle("agent_chat:undo", b.rect.center());
                        if b.clicked() {
                            undo = Some(n);
                        }
                    }
                });
            }
            Item::Notice { text, error } => {
                ui_kit::assistant_banner(ui, if *error { ui_kit::AssistantTone::Error } else { ui_kit::AssistantTone::Neutral }, |ui| {
                    ui.add(egui::Label::new(RichText::new(text).color(if *error { t.error } else { t.text_dim })).wrap());
                });
            }
        }
    }
    let (mut send_now_clicked, mut send_queue_clicked) = (false, false);
    let mut remove: Option<usize> = None;
    let mut edit_done: Option<(usize, Option<String>)> = None;
    let busy = app.agent_chat.state() == State::Busy;
    let stopped = app.agent_chat.stopped;
    let editing = app.agent_chat.pending_edit.clone();
    let mut start_edit: Option<(usize, String)> = None;
    let mut edit_buf = editing.as_ref().map(|(_, b)| b.clone());
    for (n, p) in app.agent_chat.pending.iter().enumerate() {
        ui.add_space(6.0);
        let frame = Container::new(ui.ctx(), Surface::Panel)
            .edge_frame(t.panel)
            .stroke(Stroke::new(1.0, t.accent))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::same(6));
        Container::show_backing(ui, frame, |ui| {
            ui.set_width(ui.available_width());
            ui.horizontal(|ui| {
                let tag = if stopped && !busy { "ui.agent_chat_queued_stopped" } else { "ui.agent_chat_queued" };
                ui.label(RichText::new(tr(tag)).color(t.accent).semibold().size(crate::theme::Tokens::get().font_small));
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    let x = ui_kit::button(ui, "×", ui_kit::Button::Small).backing_on_hover_text(tr("ui.agent_chat_remove"));
                    crate::scenario::publish_handle(&format!("agent_chat:pending_remove:{n}"), x.rect.center());
                    if x.clicked() {
                        remove = Some(n);
                    }
                    if editing.as_ref().is_none_or(|(i, _)| *i != n) {
                        let e = ui_kit::button(ui, tr("ui.agent_chat_edit"), ui_kit::Button::Small);
                        crate::scenario::publish_handle(&format!("agent_chat:pending_edit:{n}"), e.rect.center());
                        if e.clicked() {
                            start_edit = Some((n, p.text.clone()));
                        }
                    }
                });
            });
            match (&editing, edit_buf.as_mut()) {
                (Some((i, _)), Some(buf)) if *i == n => {
                    let te = ui_kit::message_field(ui, Id::new(("sc_agent_pending_input", n)), buf, "");
                    crate::scenario::publish_rect("agent_chat:pending_input", te.rect);
                    ui.horizontal(|ui| {
                        if ui_kit::button(ui, tr("ui.agent_chat_save"), ui_kit::Button::Normal).clicked() {
                            edit_done = Some((n, Some(buf.clone())));
                        }
                        if ui_kit::button(ui, tr("ui.agent_chat_cancel"), ui_kit::Button::Normal).clicked() {
                            edit_done = Some((n, None));
                        }
                    });
                }
                _ => {
                    if !p.text.is_empty() {
                        ui.add(egui::Label::new(RichText::new(&p.text).color(t.text)).selectable(true).wrap());
                    }
                    if p.image {
                        ui.label(RichText::new(tr("ui.agent_chat_view_attached")).color(t.text_dim).size(crate::theme::Tokens::get().font_small));
                    }
                    context_chips(ui, &p.context, false);
                    files_line(ui, p.files.iter().map(|f| f.name.as_str()));
                }
            }
        });
    }
    if !app.agent_chat.pending.is_empty() && editing.is_none() {
        ui.horizontal(|ui| {
            if busy {
                let b =
                    ui_kit::button(ui, tr("ui.agent_chat_send_now"), ui_kit::Button::Normal).backing_on_hover_text(tr("ui.agent_chat_send_now_tip"));
                crate::scenario::publish_handle("agent_chat:send_now", b.rect.center());
                send_now_clicked = b.clicked();
            } else {
                let b = ui_kit::primary_button(ui, tr("ui.agent_chat_send"));
                crate::scenario::publish_handle("agent_chat:send_queue", b.rect.center());
                send_queue_clicked = b.clicked();
            }
        });
    }
    if let Some(start) = start_edit {
        app.agent_chat.pending_edit = Some(start);
    } else if let (Some(buf), Some(slot)) = (edit_buf, app.agent_chat.pending_edit.as_mut()) {
        slot.1 = buf;
    }
    if app.agent_chat.items.is_empty() && app.agent_chat.pending.is_empty() {
        if agents::entries(app).is_empty() && app.agent_chat.detecting.is_none() {
            ui_kit::assistant_empty(ui, tr("ui.agent_chat_setup_title"), tr("ui.agent_chat_setup_body"), |ui| {
                if ui_kit::primary_button(ui, tr("ui.agent_chat_open_settings")).clicked() {
                    crate::prefs::open_at(app, crate::prefs::PAGE_ASSISTANT);
                }
            });
        } else {
            ui_kit::assistant_prose(ui, |ui| {
                ui.label(RichText::new(tr("ui.agent_chat_intro")).color(t.text_dim));
                ui.label(RichText::new(tr("ui.agent_chat_privacy")).color(t.text_dim).size(t.font_small));
            });
        }
    }
    if let Some((n, text)) = edit_done {
        if let (Some(text), Some(p)) = (text.filter(|t| !t.trim().is_empty()), app.agent_chat.pending.get_mut(n)) {
            p.text = text;
        }
        app.agent_chat.pending_edit = None;
    }
    if let Some(n) = remove {
        remove_pending(app, n);
    }
    if send_now_clicked {
        send_now(app, ui.ctx());
    }
    if send_queue_clicked {
        send_now(app, ui.ctx());
    }
    if open_gallery.is_some() {
        app.agent_chat.gallery = open_gallery;
    }
    if let Some((n, o)) = answer {
        answer_permission(app, n, o.as_deref());
    }
    match approval {
        Some((id, Some(allow))) => approve(app, id, allow),
        Some((id, None)) => approve_always(app, id),
        None => {}
    }
    if let Some(n) = undo {
        undo_changes(app, n);
    }
}

/// The files that went (or will go) with a message, by name.
#[cfg(not(target_arch = "wasm32"))]
fn files_line<'a>(ui: &mut egui::Ui, names: impl Iterator<Item = &'a str>) {
    let list = names.collect::<Vec<_>>().join(", ");
    if !list.is_empty() {
        let t = Tokens::get();
        ui.add(egui::Label::new(RichText::new(trf("ui.agent_chat_files", &[("files", &list)])).color(t.text_dim).size(t.font_small)).wrap());
    }
}

/// The widest a file chip gets; a narrower chat makes them narrower.
const CHIP_W: f32 = 170.0;

/// The files for the next message, as chips in rows that fit the chat: a picture's thumbnail (or
/// a file mark), the name cut to fit, and a × that takes the file off. Laid out by hand: egui
/// wraps plain widgets, not framed groups, and one row wider than the chat widens all below it.
#[cfg(not(target_arch = "wasm32"))]
fn attachment_chips(app: &mut SolveApp, ui: &mut egui::Ui) {
    let chat = &mut app.agent_chat;
    // Thumbnails of files no longer on the message are let go.
    let keep: Vec<u64> = chat.attachments.iter().filter(|f| f.thumb.is_some()).map(|f| f.id).collect();
    chat.thumbs.retain(|k, _| keep.contains(k));
    if chat.files() == 0 {
        return;
    }
    let gap = ui.spacing().item_spacing.x;
    let avail = ui.available_width().max(1.0);
    let w = CHIP_W.min(avail);
    let per_row = (((avail + gap) / (w + gap)).floor() as usize).max(1);
    let (mut remove, mut stop) = (None, None);
    // The files read, then those still being read (each takes its place once it is done).
    let read = chat.attachments.len();
    let indices: Vec<usize> = (0..chat.files()).collect();
    for row in indices.chunks(per_row) {
        ui.horizontal(|ui| {
            for &i in row {
                if let Some(f) = chat.attachments.get(i) {
                    if chip(ui, &mut chat.thumbs, f, i, w) {
                        remove = Some(i);
                    }
                } else if let Some(l) = chat.loading.get(i - read)
                    && loading_chip(ui, &l.name, l.importing(), i - read, w)
                {
                    stop = Some(i - read);
                }
            }
        });
    }
    if let Some(i) = remove
        && i < chat.attachments.len()
    {
        chat.attachments.remove(i);
    }
    // Taken off while it is read: its reader's answer is dropped with it.
    if let Some(i) = stop
        && i < chat.loading.len()
    {
        chat.loading.remove(i);
    }
}

/// What a chip's × says: an imported part stays in the design when it comes off the message.
#[cfg(not(target_arch = "wasm32"))]
pub(super) fn remove_tip(f: &attach::Attachment) -> &'static str {
    tr(if matches!(f.content, attach::Content::Imported) { "ui.agent_chat_remove_imported" } else { "ui.agent_chat_remove_file" })
}

/// The chip of a file still being read or imported: a spinner, the name, and a × that takes it
/// off (an import goes on: its part lands in the design).
#[cfg(not(target_arch = "wasm32"))]
fn loading_chip(ui: &mut egui::Ui, name: &str, importing: bool, i: usize, w: f32) -> bool {
    let t = Tokens::get();
    let mut clicked = false;
    let r = crate::ui_kit::Container::show_backing(
        ui,
        egui::Frame::NONE
            .fill(t.toolbar)
            .stroke(t.control_border_stroke(t.border))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::symmetric(4, 3)),
        |ui| {
            let inner = (w - 10.0).max(40.0);
            ui.set_width(inner);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let doing = tr(if importing { "ui.agent_chat_file_importing" } else { "ui.agent_chat_file_loading" });
                let mark = ui.add(egui::Spinner::new().size(t.form_icon_size)).backing_on_hover_text(doing).rect.width();
                let name_w = (inner - mark - 18.0 - 8.0).max(8.0);
                ui.allocate_ui(vec2(name_w, 20.0), |ui| {
                    ui.set_width(name_w);
                    ui.add(egui::Label::new(RichText::new(name).size(t.font_small).color(t.text_dim)).truncate())
                })
                .inner
                .backing_on_hover_text(name);
                let (r, x) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let c = r.center();
                let s = Stroke::new(1.3, if x.hovered() { t.text } else { t.text_dim });
                ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], s);
                ui.painter().line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
                crate::scenario::publish_handle(&format!("agent_chat:file_loading_remove:{i}"), c);
                clicked =
                    x.backing_on_hover_text(tr(if importing { "ui.agent_chat_remove_imported" } else { "ui.agent_chat_remove_file" })).clicked();
            });
        },
    );
    crate::scenario::publish_handle(&format!("agent_chat:file_loading:{i}"), r.response.rect.center());
    clicked
}

/// One file chip, `w` wide in all; true when its × was clicked.
#[cfg(not(target_arch = "wasm32"))]
fn chip(ui: &mut egui::Ui, thumbs: &mut std::collections::HashMap<u64, egui::TextureHandle>, f: &attach::Attachment, i: usize, w: f32) -> bool {
    let t = Tokens::get();
    let mut clicked = false;
    let r = crate::ui_kit::Container::show_backing(
        ui,
        egui::Frame::NONE
            .fill(t.toolbar)
            .stroke(t.control_border_stroke(t.border))
            .corner_radius(t.control_radius)
            .inner_margin(egui::Margin::symmetric(4, 3)),
        |ui| {
            let inner = (w - 10.0).max(40.0);
            ui.set_width(inner);
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 4.0;
                let mark = match &f.thumb {
                    Some(img) => {
                        let tex = thumbs
                            .entry(f.id)
                            .or_insert_with(|| ui.ctx().load_texture(format!("sc_attach_{}", f.id), (**img).clone(), egui::TextureOptions::LINEAR));
                        let size = tex.size_vec2();
                        ui.image((tex.id(), size * (22.0 / size.x.max(size.y).max(1.0)))).rect.width()
                    }
                    None => {
                        // A page marked with the file's type.
                        let (r, _) = ui.allocate_exact_size(vec2(28.0, 20.0), Sense::hover());
                        let color = if matches!(f.content, attach::Content::Pdf { .. }) { t.error } else { t.text_dim };
                        ui.painter().rect_stroke(r.shrink2(vec2(1.0, 1.0)), 2.0, Stroke::new(1.2, color), egui::StrokeKind::Inside);
                        ui.painter().text(
                            r.center(),
                            Align2::CENTER_CENTER,
                            f.mark(),
                            egui::FontId::proportional(crate::theme::Tokens::get().font_micro),
                            color,
                        );
                        r.width()
                    }
                };
                let name_w = (inner - mark - 18.0 - 8.0).max(8.0);
                ui.allocate_ui(vec2(name_w, 20.0), |ui| {
                    ui.set_width(name_w);
                    ui.add(egui::Label::new(RichText::new(&f.name).size(t.font_small)).truncate())
                })
                .inner
                .backing_on_hover_text(&f.name);
                let (r, x) = ui.allocate_exact_size(vec2(18.0, 18.0), Sense::click());
                let c = r.center();
                let s = Stroke::new(1.3, if x.hovered() { t.text } else { t.text_dim });
                ui.painter().line_segment([c + vec2(-3.5, -3.5), c + vec2(3.5, 3.5)], s);
                ui.painter().line_segment([c + vec2(-3.5, 3.5), c + vec2(3.5, -3.5)], s);
                crate::scenario::publish_handle(&format!("agent_chat:file_remove:{i}"), c);
                clicked = x.backing_on_hover_text(remove_tip(f)).clicked();
            });
        },
    );
    crate::scenario::publish_target(&format!("agent_chat:file:{i}"), r.response.rect);
    clicked
}

/// The + button: choose a file with the app's picker (pictures, text and CAD files), or paste
/// the clipboard's picture.
#[cfg(not(target_arch = "wasm32"))]
fn add_menu(app: &mut SolveApp, ui: &mut egui::Ui) {
    let (mut choose, mut paste) = (false, false);
    let r = ui_kit::action_menu(ui, "+", |ui| {
        let a = ui_kit::button(ui, tr("ui.agent_chat_add_file"), ui_kit::Button::Normal);
        crate::scenario::publish_handle("agent_chat:add_file", a.rect.center());
        let b = ui_kit::button(ui, tr("ui.agent_chat_paste_picture"), ui_kit::Button::Normal);
        crate::scenario::publish_handle("agent_chat:paste_picture", b.rect.center());
        if a.clicked() {
            choose = true;
            ui.close();
        }
        if b.clicked() {
            paste = true;
            ui.close();
        }
    });
    crate::scenario::publish_handle("agent_chat:add", r.response.rect.center());
    r.response.backing_on_hover_text(tr("ui.agent_chat_add"));
    if choose {
        app.pick_file(tr("ui.agent_chat_attachable"), &attach::extensions(), |app, path| {
            if let Some(path) = path
                && let Err(e) = attach_path(app, &path)
            {
                app.agent_chat.notice(e, true);
            }
        });
    }
    if paste && let Err(e) = paste_picture(app) {
        app.agent_chat.notice(e, true);
    }
}

/// A tool call as its card shows it.
#[cfg(not(target_arch = "wasm32"))]
struct ToolCall<'a> {
    /// The agent's title, shown when there are no words for the call.
    title: &'a str,
    /// What it did in words (#1457).
    summary: Option<&'a str>,
    kind: &'a str,
    status: Option<ToolStatus>,
    input: Option<&'a Value>,
    output: Option<&'a str>,
}

/// Longest raw text shown under a card's Details (characters).
#[cfg(not(target_arch = "wasm32"))]
const MAX_DETAIL: usize = 8000;

/// A tool call as a card: its state and what it did in words on one line (the agent's own title
/// when there are none); a click on the line unfolds Details, the raw arguments and what it
/// returned.
#[cfg(not(target_arch = "wasm32"))]
fn tool_card(ui: &mut egui::Ui, n: usize, call: &ToolCall) {
    let t = Tokens::get();
    let status = call.status;
    let (color, mark, state) = match status {
        Some(ToolStatus::Completed) => (t.form_success, ui_kit::AssistantToolState::Done, "ui.ai_tool_done"),
        Some(ToolStatus::Failed) => (t.error, ui_kit::AssistantToolState::Failed, "ui.ai_tool_failed"),
        Some(ToolStatus::InProgress) => (t.accent, ui_kit::AssistantToolState::Running, "ui.ai_tool_running"),
        _ => (t.text_dim, ui_kit::AssistantToolState::Pending, "ui.ai_tool_waiting"),
    };
    let details = call.input.is_some() || call.output.is_some();
    ui_kit::assistant_tool_row(ui, status == Some(ToolStatus::Failed), |ui| {
        ui.set_width(ui.available_width());
        let id = ui.make_persistent_id(("sc_agent_tool", n));
        let mut state_ = egui::collapsing_header::CollapsingState::load_with_default_open(ui.ctx(), id, false);
        let head = ui.allocate_ui_with_layout(vec2(ui.available_width(), t.control_height + t.space_2), Layout::left_to_right(Align::Center), |ui| {
            ui.spacing_mut().item_spacing.x = t.space_8;
            ui_kit::assistant_tool_status(ui, mark);
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if details {
                    let (rect, _) = ui.allocate_exact_size(vec2(t.space_14, t.space_14), Sense::hover());
                    ui_kit::disclosure(ui.painter(), rect.center(), state_.is_open(), t.text_dim);
                    ui.label(RichText::new(tr("ui.ai_tool_details")).size(t.font_small).color(t.text_dim));
                }
                ui.label(RichText::new(tr(state)).size(t.font_small).color(color));
                if !call.kind.is_empty() && call.kind != "other" {
                    ui.label(RichText::new(call.kind).size(t.font_small).color(t.text_dim));
                }
                ui.with_layout(Layout::left_to_right(Align::Center), |ui| {
                    let text = match call.summary {
                        Some(words) => RichText::new(words).size(t.font_caption).color(t.text),
                        None => RichText::new(summary::plain_title(call.title)).monospace().size(t.font_caption).color(t.text),
                    };
                    ui.add(egui::Label::new(text).truncate().selectable(false)).backing_on_hover_text(call.title);
                });
            });
        });
        let click = ui.interact(head.response.rect, id.with("head"), Sense::click());
        crate::scenario::publish_handle(&format!("agent_chat:tool:{n}"), head.response.rect.center());
        if details && click.clicked() {
            state_.toggle(ui);
        }
        if details {
            click.backing_on_hover_text(tr("ui.ai_tool_unfold"));
        }
        state_.show_body_unindented(ui, |ui| {
            ui.indent(id.with("detail"), |ui| {
                if let Some(input) = call.input {
                    let text = serde_json::to_string_pretty(input).unwrap_or_default();
                    tool_detail(ui, tr("ui.ai_tool_arguments"), &text, id.with("in"));
                }
                if let Some(o) = call.output {
                    tool_detail(ui, tr("ui.ai_tool_result"), o, id.with("out"));
                }
            });
        });
    });
}

/// One part of a card's Details: its heading and the raw text, cut short and scrolling.
#[cfg(not(target_arch = "wasm32"))]
fn tool_detail(ui: &mut egui::Ui, heading: &str, text: &str, id: Id) {
    let t = Tokens::get();
    ui.label(RichText::new(heading).size(t.font_small).color(t.text_dim));
    let shown: String =
        if text.chars().count() > MAX_DETAIL { text.chars().take(MAX_DETAIL).chain(std::iter::once('…')).collect() } else { text.to_string() };
    ui_kit::assistant_tool_detail(ui, |ui| {
        ui.set_min_width(ui.available_width());
        egui::ScrollArea::vertical().id_salt(id).max_height(180.0).show(ui, |ui| {
            ui.add(egui::Label::new(RichText::new(shown).monospace().size(t.font_small)).selectable(true).wrap());
        });
    });
}

/// A small status mark drawn as a shape (no glyph a system font might lack): filled when the
/// step is running or done, hollow while it waits.
#[cfg(not(target_arch = "wasm32"))]
fn status_dot(ui: &mut egui::Ui, color: Color32, filled: bool) {
    let (r, _) = ui.allocate_exact_size(vec2(10.0, 10.0), Sense::hover());
    if filled {
        ui.painter().circle_filled(r.center(), 4.0, color);
    } else {
        ui.painter().circle_stroke(r.center(), 3.5, Stroke::new(1.2, color));
    }
}

/// A custom command as the picker shows it: the program's file name and its arguments, cut short.
pub(super) fn short_command(line: &str) -> String {
    let argv = acp::split_command(line);
    let mut parts: Vec<String> = argv.iter().map(|a| a.rsplit(['/', '\\']).next().unwrap_or(a).to_string()).collect();
    if parts.is_empty() {
        parts.push(line.to_string());
    }
    let s = parts.join(" ");
    if s.chars().count() > 40 { format!("{}…", s.chars().take(40).collect::<String>()) } else { s }
}

/// Quote an argument for the command field when it holds spaces.
pub(super) fn quote(a: &str) -> String {
    if a.is_empty() || a.contains(char::is_whitespace) || a.contains('"') { format!("\"{}\"", a.replace('"', "\\\"")) } else { a.to_string() }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_discovery_explains_flatpak_permissions() {
        const CHILD: &str = "SOLIDCRAFT_TEST_FLATPAK_MESSAGE";
        if let Ok(case) = std::env::var(CHILD) {
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.agent_chat.detection = Some(discover::Detection::default());
            assert!(agents::entries(&app).is_empty());
            let message = no_agents_message();
            if case == "flatpak" {
                assert!(message.contains("sandbox"), "{message}");
                assert!(message.contains("restart"), "{message}");
                let ctx = crate::fonts::test_context();
                let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(300.0, 900.0))), ..Default::default() };
                for _ in 0..2 {
                    let mut output = ctx.run_ui(input.clone(), |ui| picker(&mut app, ui));
                    assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == message));
                    assert!(crate::test_support::text_shapes(&output).any(|text| text.galley.text() == FLATPAK_OVERRIDE));
                    output.textures_delta.clear();
                }
                let at = crate::scenario::handle_at("agent_chat:flatpak_override").expect("copy override");
                let mut copied = false;
                for pressed in [true, false] {
                    let mut output =
                        ctx.run_ui(egui::RawInput { events: crate::test_support::pointer(at, pressed), ..input.clone() }, |ui| picker(&mut app, ui));
                    copied |= output
                        .platform_output
                        .commands
                        .iter()
                        .any(|command| matches!(command, egui::OutputCommand::CopyText(text) if text == FLATPAK_OVERRIDE));
                    output.textures_delta.clear();
                }
                assert!(copied, "Copy preserves the complete override command");
            } else {
                assert_eq!(message, tr("ui.agent_chat_none_found"));
            }
            return;
        }
        for case in ["flatpak", "native"] {
            let mut child = std::process::Command::new(std::env::current_exe().unwrap());
            child.args(["--exact", "agent_chat::panel::tests::empty_discovery_explains_flatpak_permissions", "--nocapture"]);
            child.env(CHILD, case);
            if case == "flatpak" {
                child.env("FLATPAK_ID", "io.tangencylabs.solidcraft");
            } else {
                child.env_remove("FLATPAK_ID");
            }
            assert!(child.status().unwrap().success(), "{case}");
        }
    }

    #[test]
    fn custom_commands_show_short_in_the_picker() {
        assert_eq!(short_command("python3 -I /very/long/path/to/mock_agent.py"), "python3 -I mock_agent.py");
        assert!(short_command(&"x".repeat(100)).ends_with('…'));
    }

    #[test]
    fn the_shortcut_shows_folds_and_hides() {
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        toggle(&mut app);
        assert!(app.agent_chat.open);
        app.preferences.agent_chat_minimized = true;
        toggle(&mut app);
        assert!(app.agent_chat.open && !app.preferences.agent_chat_minimized, "a folded panel unfolds");
        toggle(&mut app);
        assert!(!app.agent_chat.open);
    }

    #[test]
    fn assistant_header_and_composer_keep_actions_inside_each_panel_width() {
        for dark in [false, true] {
            crate::theme::set_dark(dark);
            for width in [280.0, 440.0, 760.0] {
                let ctx = crate::fonts::test_context();
                crate::theme::apply(&ctx);
                let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
                app.agent_chat.detection = Some(discover::Detection::default());
                app.agent_chat.picking = Some(false);
                app.agent_chat.state = Some(State::Ready);
                app.agent_chat.input = "Make a plate".to_string();
                let input =
                    egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width + 16.0, 720.0))), ..Default::default() };
                for _ in 0..2 {
                    crate::scenario::clear_handles();
                    let mut output = ctx.run_ui(input.clone(), |ui| {
                        ui.set_width(width);
                        ui_kit::scope(ui, |ui| {
                            header(&mut app, ui, false);
                            desktop_body(&mut app, ui);
                        });
                    });
                    output.textures_delta.clear();
                }
                let controls: Vec<_> = ["close", "minimize", "newchat", "pop_out", "permissions", "send"]
                    .into_iter()
                    .map(|name| {
                        let key = format!("agent_chat:{name}");
                        let rect = crate::scenario::rect_of(&key).unwrap_or_else(|| panic!("missing {key}"));
                        assert!(rect.left() >= 0.0 && rect.right() <= width + 16.0, "{key} outside {width}: {rect:?}");
                        rect
                    })
                    .collect();
                for (i, a) in controls.iter().enumerate() {
                    for b in controls.iter().skip(i + 1) {
                        assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "overlapping Assistant controls at {width}: {a:?} {b:?}");
                    }
                }
            }
        }
    }

    #[test]
    fn assistant_permissions_keep_provider_choices_and_responsive_approval_actions() {
        for width in [280.0, 440.0, 760.0] {
            let ctx = crate::fonts::test_context();
            let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
            app.agent_chat.items.push(Item::Permission {
                key: 4,
                title: "Run solid.fillet".into(),
                answer: None,
                options: vec![
                    PermissionOption { id: "reject".into(), name: "Deny".into(), kind: "reject_once".into() },
                    PermissionOption { id: "remember".into(), name: "Always allow".into(), kind: "allow_always".into() },
                    PermissionOption { id: "once".into(), name: "Allow once".into(), kind: "allow_once".into() },
                    PermissionOption { id: "custom".into(), name: "Review details".into(), kind: "provider_specific".into() },
                ],
            });
            let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(width + 16.0, 720.0))), ..Default::default() };
            crate::scenario::clear_handles();
            let mut output = ctx.run_ui(input, |ui| {
                ui.set_width(width);
                ui_kit::scope(ui, |ui| conversation(&mut app, ui));
            });
            output.textures_delta.clear();
            for id in ["reject", "remember", "once", "custom"] {
                let at = crate::scenario::handle_at(&format!("agent_chat:permission:{id}")).expect("provider option retained");
                assert!(at.x > 0.0 && at.x < width + 16.0, "choice outside {width}: {id} {at:?}");
            }
            let once = crate::scenario::handle_at("agent_chat:permission:once").expect("once");
            let always = crate::scenario::handle_at("agent_chat:permission:remember").expect("always");
            assert!(once.x > always.x, "primary once choice remains on the right");
            assert_eq!(app.agent_chat.waiting(), 1, "drawing never answers an approval");
        }
    }

    #[test]
    fn assistant_agent_mode_and_model_wrap_without_escaping_narrow_width() {
        let ctx = crate::fonts::test_context();
        let mut app = SolveApp::new(solidcraft_engine::Session::default(), crate::Services::default());
        app.agent_chat.state = Some(State::Ready);
        app.agent_chat.detection = Some(discover::Detection::default());
        app.agent_chat.agent_id = Some("fixture".into());
        app.preferences.agents.push(agents::AgentConfig {
            id: "fixture".into(),
            name: "A coding agent with a long descriptive name".into(),
            command: "fixture".into(),
            ..Default::default()
        });
        app.agent_chat.modes = vec![("edit".into(), "A provider mode with a long descriptive label".into())];
        app.agent_chat.mode = "edit".into();
        app.agent_chat.models = vec![
            acp::ModelInfo {
                id: "model".into(),
                name: "A provider with a long name · A model with a long descriptive label".into(),
                description: String::new(),
            },
            acp::ModelInfo { id: "other".into(), name: "Another provider · model".into(), description: String::new() },
        ];
        app.agent_chat.model = "model".into();
        let input = egui::RawInput { screen_rect: Some(Rect::from_min_size(egui::Pos2::ZERO, vec2(296.0, 720.0))), ..Default::default() };
        crate::scenario::clear_handles();
        let mut output = ctx.run_ui(input, |ui| {
            ui.set_width(280.0);
            ui_kit::scope(ui, |ui| switcher(&mut app, ui));
        });
        output.textures_delta.clear();
        let rects: Vec<_> = ["switcher", "mode", "provider", "model"]
            .into_iter()
            .map(|name| {
                let rect = crate::scenario::rect_of(&format!("agent_chat:{name}")).expect("picker retained");
                assert!(rect.left() >= 0.0 && rect.right() <= 296.0, "{name} outside narrow panel: {rect:?}");
                rect
            })
            .collect();
        for (n, a) in rects.iter().enumerate() {
            for b in rects.iter().skip(n + 1) {
                assert!(!a.shrink(0.5).intersects(b.shrink(0.5)), "picker overlap: {a:?} {b:?}");
            }
        }
    }
}
