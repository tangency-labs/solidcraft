//! Panel elements for dense cards; see `docs/ui-kit.md` for tokens and usage.
use super::{actions::icon_button_at, metrics::*, text::paint_text};
use crate::{scenario, theme::Tokens};
use egui::{Ui, vec2};

/// Shared directions and cursor mapping for embedded and detached resize handles.
pub(super) const RESIZE_DIRECTIONS: [(&str, i32, i32); 8] = [
    ("top_left", -1, -1),
    ("top_right", 1, -1),
    ("bottom_left", -1, 1),
    ("bottom_right", 1, 1),
    ("left", -1, 0),
    ("right", 1, 0),
    ("top", 0, -1),
    ("bottom", 0, 1),
];

/// Embedded targets straddle the border; native targets stay inside the content area.
pub(super) fn resize_hit(rect: egui::Rect, x: i32, y: i32, corner: f32, side: f32, centered: bool) -> egui::Rect {
    let at = rect.lerp_inside((vec2(x as f32, y as f32) + egui::Vec2::splat(1.0)) * 0.5);
    let inset = if centered { corner * 0.5 } else { corner };
    let size = if x != 0 && y != 0 {
        vec2(corner, corner)
    } else if x == 0 {
        vec2(rect.width() - 2.0 * inset, side)
    } else {
        vec2(side, rect.height() - 2.0 * inset)
    };
    if centered {
        egui::Rect::from_center_size(at, size.max(egui::Vec2::ZERO))
    } else {
        egui::Rect::from_center_size(at - vec2(x as f32 * size.x, y as f32 * size.y) * 0.5, size)
    }
}

pub(super) fn resize_icon(x: i32, y: i32) -> egui::CursorIcon {
    match (x, y) {
        (0, _) => egui::CursorIcon::ResizeVertical,
        (_, 0) => egui::CursorIcon::ResizeHorizontal,
        _ if x == y => egui::CursorIcon::ResizeNwSe,
        _ => egui::CursorIcon::ResizeNeSw,
    }
}

#[derive(Clone)]
struct ResizeCursor {
    id: egui::Id,
    hit: egui::Rect,
    layer: egui::LayerId,
    cursor: egui::CursorIcon,
    active: bool,
    enabled: bool,
}

#[derive(Clone)]
struct ResizeCursorHandler(std::sync::Arc<dyn Fn(&egui::Context, egui::CursorIcon) + Send + Sync>);

/// Install an optional desktop cursor refresh for internal resize controls only.
/// It runs for the selected viewport after cursor resolution, including repeated
/// identical icons which an integration's cursor cache may otherwise skip.
pub fn set_resize_cursor_handler(ctx: &egui::Context, handler: impl Fn(&egui::Context, egui::CursorIcon) + Send + Sync + 'static) {
    ctx.data_mut(|d| d.insert_temp(egui::Id::new("sc_resize_cursor_handler"), ResizeCursorHandler(std::sync::Arc::new(handler))));
}

fn cursor_key(ctx: &egui::Context) -> egui::Id {
    egui::Id::new("sc_resize_cursors").with(ctx.viewport_id())
}

/// The scene draws before floating controls, so retain their preceding pass's
/// grab rectangles for input ownership, just as egui retains window hit tests.
pub(crate) fn resize_target_contains(ctx: &egui::Context, point: egui::Pos2) -> bool {
    let key = cursor_key(ctx);
    ctx.data(|d| {
        d.get_temp::<Vec<ResizeCursor>>(key)
            .unwrap_or_default()
            .into_iter()
            .chain(d.get_temp::<Vec<ResizeCursor>>(key.with("previous")).unwrap_or_default())
            .any(|target| target.enabled && target.hit.contains(point))
    })
}

/// Resolve after all content, so later widget hover cannot overwrite a resize
/// arrow. Keep each native viewport's targets separate from its parent's.
fn install_resize_cursor(ctx: &egui::Context) {
    let install = ctx.data_mut(|d| {
        let key = egui::Id::new("sc_resize_cursor_installed");
        if d.get_temp::<bool>(key).unwrap_or(false) {
            false
        } else {
            d.insert_temp(key, true);
            true
        }
    });
    if !install {
        return;
    }
    ctx.on_begin_pass(
        "resize cursor targets",
        std::sync::Arc::new(|ui| {
            let ctx = ui.ctx();
            let key = cursor_key(ctx);
            ctx.data_mut(|d| {
                let previous = d.remove_temp::<Vec<ResizeCursor>>(key).unwrap_or_default();
                d.insert_temp(key.with("previous"), previous);
            });
            if ctx.data(|d| d.get_temp::<bool>(key.with("owned"))).unwrap_or(false) {
                ctx.set_cursor_icon(egui::CursorIcon::Default);
            }
        }),
    );
    ctx.on_end_pass(
        "resize cursor feedback",
        std::sync::Arc::new(|ui| {
            let ctx = ui.ctx();
            let key = cursor_key(ctx);
            let pointer = ctx.input(|i| i.pointer.hover_pos());
            let targets = ctx.data(|d| d.get_temp::<Vec<ResizeCursor>>(key)).unwrap_or_default();
            let resize = targets
                .into_iter()
                .filter(|target| {
                    target.enabled
                        && (target.active
                            || (!ctx.egui_is_using_pointer()
                                && (ctx.interaction_snapshot(|i| i.hovered.contains(&target.id))
                                    || pointer.is_some_and(|p| {
                                        target.hit.contains(p)
                                            && ctx.layer_id_at(p).is_none_or(|layer| layer == target.layer || layer.order < target.layer.order)
                                    }))))
                })
                .max_by_key(|target| {
                    (target.active, target.layer.order, matches!(target.cursor, egui::CursorIcon::ResizeNwSe | egui::CursorIcon::ResizeNeSw))
                });
            if let Some(target) = &resize {
                ctx.set_cursor_icon(target.cursor);
                if let Some(handler) = ctx.data(|d| d.get_temp::<ResizeCursorHandler>(egui::Id::new("sc_resize_cursor_handler"))) {
                    (handler.0)(ctx, target.cursor);
                }
            }
            ctx.data_mut(|d| d.insert_temp(key.with("owned"), resize.is_some()));
        }),
    );
    ctx.request_repaint();
}

/// Register the whole grab rectangle, including its outside half, for final
/// cursor selection. Do not depend on the smaller window area's hover flag.
pub(super) fn resize_cursor(ui: &Ui, hit: egui::Rect, response: &egui::Response, cursor: egui::CursorIcon) {
    let ctx = ui.ctx();
    install_resize_cursor(ctx);
    let target = ResizeCursor {
        id: response.id,
        hit: hit.intersect(ui.clip_rect()),
        layer: ui.layer_id(),
        cursor,
        active: response.dragged() || response.is_pointer_button_down_on(),
        enabled: ui.is_enabled(),
    };
    let key = cursor_key(ctx);
    ctx.data_mut(|d| d.get_temp_mut_or_default::<Vec<ResizeCursor>>(key).push(target));
}

/// Shared active-tab hover tint for the browser and collapsible panel chrome.
pub(crate) fn panel_header_hover(painter: &egui::Painter, rect: egui::Rect) {
    let t = Tokens::get();
    painter.rect_filled(rect, t.radius_lg.saturating_sub(t.space_4 as u8), crate::theme::TabAccent::current(painter.ctx()).soft);
}

/// Feature title/icon/close band. Tokens: `form_header_height, form_icon_size, form_close_size, form_inset, font_label, text, dialog_rule, space_8/10; icon_button_at tokens`.
pub(crate) fn header(ui: &mut Ui, icon: &str, title: &str, on_close: impl FnOnce()) {
    header_impl(ui, icon, title, None, true, on_close);
}

/// Optional fold action for managers which historically hide their inputs without losing edits.
/// Tokens: header tokens and space_4; retains the dialog:collapse scenario handle.
pub(crate) fn collapsible_header(ui: &mut Ui, icon: &str, title: &str, collapsed: &mut bool, on_close: impl FnOnce()) {
    header_impl(ui, icon, title, Some(collapsed), true, on_close);
}

fn header_impl(ui: &mut Ui, icon: &str, title: &str, collapsed: Option<&mut bool>, closeable: bool, on_close: impl FnOnce()) {
    let t = Tokens::get();
    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), header_height()), egui::Sense::hover());
    // Shared card chrome: 12 pt left inset, 19 pt icon, 8 pt gap and 26 pt close target.
    let icon_rect = egui::Rect::from_center_size(
        egui::pos2(rect.left() + t.space_12 + t.form_icon_size / 2.0, rect.center().y),
        vec2(t.form_icon_size, t.form_icon_size),
    );
    crate::icons::paint_icon(ui.painter(), icon_rect, icon, 1.0);
    let close_rect = egui::Rect::from_center_size(
        egui::pos2((rect.right() - t.space_8 - t.form_close_size / 2.0).max(rect.left()), rect.center().y),
        vec2(t.form_close_size, t.form_close_size),
    );
    if closeable {
        let close = icon_button_at(ui, close_rect, "close", "Close");
        scenario::publish_handle("dialog:close", close.rect.center());
        if close.clicked() || (ui.ctx().viewport_id() != egui::ViewportId::ROOT && ui.input(|i| i.viewport().close_requested())) {
            super::floating::close_handled(ui.ctx());
            on_close();
        }
    }
    let right = if let Some(collapsed) = collapsed {
        let fold = egui::Rect::from_min_size(egui::pos2(close_rect.left() - t.form_close_size - t.space_4, close_rect.top()), close_rect.size());
        let response = super::disclosure_button_at(ui, fold, !*collapsed, crate::i18n::tr_label(if *collapsed { "Expand" } else { "Collapse" }));
        scenario::publish_handle("dialog:collapse", response.rect.center());
        if response.clicked() {
            let frame = ui.ctx().cumulative_frame_nr();
            let key = response.id.with("toggle_frame");
            if ui.ctx().data(|d| d.get_temp::<u64>(key)) != Some(frame) {
                *collapsed = !*collapsed;
                ui.ctx().data_mut(|d| d.insert_temp(key, frame));
            }
        }
        fold.left()
    } else if closeable {
        close_rect.left()
    } else {
        rect.right() - t.space_8
    };
    let left = (icon_rect.right() + t.space_8).min(right);
    let title_rect = egui::Rect::from_min_max(egui::pos2(left, rect.top()), egui::pos2(right.max(left), rect.bottom()));
    paint_text(ui, title_rect, title, crate::fonts::semibold(t.font_heading), t.form_ink, false);
    let title_response = ui.interact(title_rect, ui.make_persistent_id("title_text"), egui::Sense::hover());
    title_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), title));
    scenario::publish_rect("dialog:header", rect);
    t.secondary_hline(ui.painter(), rect.x_range(), rect.bottom(), t.dialog_rule);
}

/// Where a feature panel's OK, Cancel and Apply sit (Preferences › Interface › Dialog buttons,
/// #6708). The app publishes the preference each frame, so every `Panel::feature` follows it.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DialogButtons {
    /// In a footer row under the inputs (how it always was). Also what a value this version
    /// doesn't know reads as (`prefs::lenient`).
    #[default]
    Bottom,
    /// In the header row, right of the title, where they stay while sections open and close.
    Top,
}

impl DialogButtons {
    pub(crate) fn publish(self, ctx: &egui::Context) {
        ctx.data_mut(|data| data.insert_temp(egui::Id::new("sc_dialog_buttons"), self));
    }

    pub(crate) fn current(ctx: &egui::Context) -> Self {
        ctx.data(|data| data.get_temp(egui::Id::new("sc_dialog_buttons"))).unwrap_or_default()
    }
}

/// The header row of a feature panel whose actions sit at the top. It is laid out before the
/// body, and its icon, title and buttons are drawn after it, once the compute status and whether
/// OK is enabled are known. Same tokens as `header`, plus `HeaderActions`'.
struct TopHeader<'a> {
    icon: egui::Rect,
    title: egui::Rect,
    actions: super::HeaderActions<'a>,
}

impl<'a> TopHeader<'a> {
    fn new(ui: &mut Ui, ok: &'a str, apply: Option<&'a str>, cancel: &'a str, collapsed: Option<&mut bool>, close: &mut bool) -> Self {
        let t = Tokens::get();
        let (band, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), header_height()), egui::Sense::hover());
        let icon = egui::Rect::from_center_size(
            egui::pos2(band.left() + t.space_12 + t.form_icon_size / 2.0, band.center().y),
            vec2(t.form_icon_size, t.form_icon_size),
        );
        // The buttons end where the fields below do.
        let end = (band.right() - t.space_12).max(band.left());
        let actions = super::HeaderActions::new(ui, band, end, ok, apply, cancel);
        let buttons = actions.left(end);
        // Nothing placed yet beside the buttons: the next control keeps clear of them.
        let beside_buttons = |right: f32| buttons < end && right >= buttons;
        // An icon target to the left of what is already placed, `space_4` clear of the buttons.
        let target = |right: f32| {
            let right = if beside_buttons(right) { right - t.space_4 } else { right };
            egui::Rect::from_center_size(
                egui::pos2((right - t.form_close_size / 2.0).max(band.left()), band.center().y),
                vec2(t.form_close_size, t.form_close_size),
            )
        };
        let mut right = buttons;
        // The header's close does what Cancel does in every panel, so with Cancel up here it is
        // left out: one control for one action, and its width goes to the title. Its handle
        // moves to Cancel, so automation that closes a panel keeps working.
        if let Some(cancel_rect) = actions.rect(super::Action::Cancel) {
            scenario::publish_handle("dialog:close", cancel_rect.center());
        } else {
            let close_rect = target(right);
            let response = icon_button_at(ui, close_rect, "close", "Close");
            scenario::publish_handle("dialog:close", response.rect.center());
            if response.clicked() {
                *close = true;
            }
            right = close_rect.left();
        }
        if let Some(collapsed) = collapsed {
            let fold = target(right);
            let response = super::disclosure_button_at(ui, fold, !*collapsed, crate::i18n::tr_label(if *collapsed { "Expand" } else { "Collapse" }));
            scenario::publish_handle("dialog:collapse", response.rect.center());
            if response.clicked() {
                *collapsed = !*collapsed;
            }
            right = fold.left();
        }
        // Text next to a text button keeps a gap; an icon target carries its own padding.
        if beside_buttons(right) {
            right = (right - t.space_6).max(band.left());
        }
        let left = (icon.right() + t.space_8).min(right);
        let title = egui::Rect::from_min_max(egui::pos2(left, band.top()), egui::pos2(right.max(left), band.bottom()));
        // Dragging the panel by its title must not start on a button.
        scenario::publish_handle("dialog:title", title.center());
        scenario::publish_rect("dialog:header", band);
        t.secondary_hline(ui.painter(), band.x_range(), band.bottom(), t.dialog_rule);
        Self { icon, title, actions }
    }

    /// The icon (the compute spinner while busy), the title (the compute status while there is
    /// one: the footer that showed it is gone) and the buttons. Returns the action asked for.
    fn finish(&self, ui: &mut Ui, icon: &str, title: &str, state: &ComputeStatus, ok_enabled: bool) -> super::Action {
        let t = Tokens::get();
        if state.busy {
            egui::Spinner::new().size(t.space_14).paint_at(ui, self.icon);
            let status = state.text.as_deref().unwrap_or_else(|| crate::i18n::tr_label("Updating the preview"));
            ui.interact(self.icon, ui.id().with("compute_status"), egui::Sense::hover()).on_hover_text(status);
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
        } else {
            crate::icons::paint_icon(ui.painter(), self.icon, icon, 1.0);
        }
        let heading = crate::fonts::semibold(t.font_heading);
        match state.text.as_deref() {
            Some(status) => paint_text(ui, self.title, status, crate::fonts::medium(t.font_label), t.form_dim, false),
            None => paint_text(ui, self.title, title, heading.clone(), t.form_ink, false),
        }
        let title_response = ui.interact(self.title, ui.make_persistent_id("title_text"), egui::Sense::hover());
        title_response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Label, ui.is_enabled(), title));
        // The buttons take the row's width first; a title that no longer fits says itself on hover.
        if ui.painter().layout_no_wrap(title.to_owned(), heading, t.form_ink).size().x > self.title.width() {
            title_response.on_hover_text(title);
        }
        self.actions.show(ui, ok_enabled)
    }
}

/// A bounded docked or floating card. Uses form_card_width and the caller's tokenised frame.
/// Window IDs and content scopes are retained so moving a card does not reset field focus.
#[derive(Clone)]
pub(crate) struct Panel<'a> {
    title: &'a str,
    id: egui::Id,
    anchor: egui::Pos2,
    viewport: egui::Rect,
    side: crate::frame::Side,
    frame: egui::Frame,
    width: f32,
    body_rows: Option<f32>,
    ok: &'a str,
    cancel: &'a str,
    reset_scroll: bool,
    resizable: bool,
    auto_size: bool,
    native_content: bool,
}

pub(crate) struct FeatureOptions<'a> {
    pub icon: &'a str,
    pub title: &'a str,
    pub drag_key: &'a str,
    pub moved: egui::Vec2,
    pub docked: egui::Pos2,
    pub apply_label: Option<&'a str>,
}

/// State read after rendering the fields, just as in the original feature card.
pub(crate) struct ComputeStatus {
    pub text: Option<String>,
    pub busy: bool,
    pub applying: bool,
}

pub(crate) enum Part {
    Header,
    Body,
    Footer,
}

impl<'a> Panel<'a> {
    /// Pinned inline actions inside an existing feature/card surface. This is not
    /// a second window or painted card: the parent owns the chrome. Tokens:
    /// control_height, space_24/8; bounded row count preserves finite geometry.
    pub(crate) fn pinned_footer<R>(ui: &mut Ui, key: &'static str, rows: usize, add: impl FnOnce(&mut Ui) -> R) -> egui::InnerResponse<R> {
        let t = Tokens::get();
        // This band is content inside the enclosing card, not a second adaptive card.
        let host = super::Container::new(ui.ctx(), super::Surface::Viewport);
        host.edge(key, super::Edge::Bottom)
            .exact_size(t.control_height * rows.clamp(1, 8) as f32 + t.space_24 + t.space_8)
            .resizable(false)
            .frame(host.edge_frame(egui::Color32::TRANSPARENT))
            .show(ui, add)
    }

    /// Shared floating card surface for panels, windows and modal confirmations.
    pub(super) fn card_frame(ctx: &egui::Context) -> egui::Frame {
        let t = Tokens::get();
        super::Container::new(ctx, super::Surface::Window).frame(ctx, t.dialog_body, crate::frame::Side::Right).inner_margin(0.0)
    }

    /// Feature surface; shared container policy supplies the border and shadow.
    pub(crate) fn feature_frame(ctx: &egui::Context, layout: crate::shell::LayoutMode, side: crate::frame::Side) -> egui::Frame {
        super::Container::for_layout(layout, super::Surface::Feature).frame(ctx, Tokens::get().dialog_body, side).inner_margin(0.0)
    }

    /// Sketch palette uses the same layout policy as every other workspace panel.
    pub(crate) fn palette_frame(ctx: &egui::Context, layout: crate::shell::LayoutMode) -> egui::Frame {
        let t = Tokens::get();
        super::Container::for_layout(layout, super::Surface::Palette)
            .frame(ctx, t.panel.gamma_multiply(t.sketch_palette_alpha), crate::frame::Side::Right)
            .inner_margin(0.0)
    }

    pub(crate) fn new(title: &'a str, id: egui::Id, anchor: egui::Pos2, viewport: egui::Rect, side: crate::frame::Side, frame: egui::Frame) -> Self {
        Self {
            title,
            id,
            anchor,
            viewport,
            side,
            frame,
            width: Tokens::get().form_card_width,
            body_rows: None,
            ok: "OK",
            cancel: "Cancel",
            reset_scroll: false,
            resizable: false,
            auto_size: false,
            native_content: false,
        }
    }

    /// A newly opened manager starts at its first section. Later frames retain user scrolling.
    pub(crate) fn reset_scroll(mut self, reset: bool) -> Self {
        self.reset_scroll = reset;
        self
    }

    pub(crate) fn width(mut self, width: f32) -> Self {
        self.width = width;
        self
    }

    /// Allow edge and corner resizing while retaining the card's header drag and pinned actions.
    pub(crate) fn resizable(mut self) -> Self {
        self.resizable = true;
        self
    }

    /// Fit a new tool to its content until the user chooses a window size.
    pub(crate) fn resizable_to_content(mut self) -> Self {
        self.resizable = true;
        self.auto_size = true;
        self
    }

    fn sized(&self, ctx: &egui::Context) -> bool {
        self.resizable
            && (super::floating::drag_size(ctx, self.id).is_some()
                || !self.auto_size
                || ctx.data_mut(|d| d.get_persisted::<egui::Vec2>(self.id.with("size"))).is_some())
    }

    fn active_viewport(&self, ctx: &egui::Context) -> egui::Rect {
        if self.native_content {
            return self.viewport;
        }
        let moving = ctx
            .data(|d| d.get_temp::<super::floating::Title>(self.id.with("floating_title")))
            .is_some_and(|title| crate::frame::title_dragging(ctx, &title.key));
        if moving { ctx.content_rect().expand(self.requested_size(ctx).max_elem().max(128.0)) } else { self.viewport }
    }

    /// Common embedded/native host for tool panels. Menus and modal prompts use other hosts.
    pub(crate) fn show<R>(&self, ctx: &egui::Context, add: impl FnMut(&mut Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
        super::floating::show(ctx, self.id, self.title, self.window(ctx), add)
    }

    pub(crate) fn show_open<R>(&self, ctx: &egui::Context, open: &mut bool, add: impl FnMut(&mut Ui) -> R) -> Option<egui::InnerResponse<Option<R>>> {
        super::floating::show(ctx, self.id, self.title, self.window(ctx).open(open), add)
    }

    /// Fill a native viewport; its system frame owns moving and resizing.
    #[cfg(not(target_arch = "wasm32"))]
    pub(crate) fn native_content(mut self) -> Self {
        self.native_content = true;
        self
    }

    fn requested_size(&self, ctx: &egui::Context) -> egui::Vec2 {
        if let Some(size) = super::floating::drag_size(ctx, self.id) {
            return size;
        }
        let t = Tokens::get();
        let initial = vec2(self.width, self.body_rows.unwrap_or(16.0) * t.control_height + t.space_12 * 2.0 + header_height() + t.form_footer_height);
        ctx.data_mut(|d| d.get_persisted::<egui::Vec2>(self.id.with("size"))).filter(|s| s.is_finite() && s.x > 0.0 && s.y > 0.0).unwrap_or(initial)
    }

    fn panel_width(&self, ctx: &egui::Context) -> f32 {
        if self.resizable { self.requested_size(ctx).x } else { self.width }
    }

    /// The footer's primary and closing actions when they are not OK and Cancel. An empty `ok`
    /// leaves the primary action out (a card that only closes, e.g. Measure).
    pub(crate) fn actions(mut self, ok: &'a str, cancel: &'a str) -> Self {
        (self.ok, self.cancel) = (ok, cancel);
        self
    }

    /// Table surface width derives from the same card-width token and remains viewport bounded.
    pub(crate) fn wide(self) -> Self {
        self.width(Tokens::get().form_card_width * 3.0)
    }

    /// Cap a manager's scrolling body independently of its position, in control rows.
    /// Short viewports still take precedence so the header and actions remain reachable.
    pub(crate) fn body_rows(mut self, rows: f32) -> Self {
        if rows.is_finite() {
            self.body_rows = Some(rows.clamp(1.0, 32.0));
        }
        self
    }

    /// The painted anchor also drives dragging, including the shell's edge clearance.
    fn bounded_anchor(&self, ctx: &egui::Context) -> egui::Pos2 {
        let viewport = self.active_viewport(ctx);
        let gap = super::Container::new(ctx, super::Surface::Feature).gap();
        let width = self.panel_width(ctx).min(viewport.width().max(0.0));
        let raw_anchor = ctx
            .data(|d| d.get_temp::<super::floating::Title>(self.id.with("floating_title")))
            .filter(|title| crate::frame::title_dragging(ctx, &title.key))
            .map_or(self.anchor, |title| title.docked + crate::frame::dock_offset(ctx, &title.key));
        let x = match self.side {
            crate::frame::Side::Right => raw_anchor.x.clamp(
                viewport.left() + width,
                (viewport.right() - if gap > 0.0 { Tokens::get().space_8 } else { 0.0 }).max(viewport.left() + width),
            ),
            crate::frame::Side::Left => raw_anchor.x.clamp(viewport.left(), viewport.right() - width),
        };
        let t = Tokens::get();
        let chrome = t.form_header_height + t.form_footer_height + t.form_gap_y * 2.0 + self.frame.total_margin().sum().y + gap;
        egui::pos2(x, raw_anchor.y.clamp(viewport.top(), (viewport.bottom() - chrome).max(viewport.top())))
    }

    /// One positioned frame supplies both the outer surface and its footer corners.
    fn positioned_frame(&self, ctx: &egui::Context) -> egui::Frame {
        let anchor = self.bounded_anchor(ctx);
        let right = match self.side {
            crate::frame::Side::Right => anchor.x,
            crate::frame::Side::Left => anchor.x + self.panel_width(ctx).min(self.viewport.width().max(0.0)),
        };
        super::Container::new(ctx, super::Surface::Feature).tool_frame(self.frame, right >= self.viewport.right() - 0.5)
    }

    pub(crate) fn window(&self, ctx: &egui::Context) -> egui::Window<'a> {
        let dock_position = ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(self.id.with("floating_dock_position")));
        if let Some(position) = dock_position {
            let mut panel = self.clone();
            panel.anchor = if self.side == crate::frame::Side::Right { position + vec2(self.panel_width(ctx), 0.0) } else { position };
            if let Some(title) = ctx.data(|d| d.get_temp::<super::floating::Title>(self.id.with("floating_title"))) {
                ctx.data_mut(|d| d.insert_temp(egui::Id::new(("sc_dock_offset", title.key.as_str())), panel.anchor - title.docked));
            }
            return panel.window(ctx);
        }
        let gap = super::Container::new(ctx, super::Surface::Feature).gap();
        let viewport = self.active_viewport(ctx);
        let anchor = self.bounded_anchor(ctx);
        let side = self.side;
        let frame = self.positioned_frame(ctx);
        let margin = frame.total_margin();
        let width = self.panel_width(ctx).min(viewport.width().max(0.0));
        let inner_width = (width - margin.sum().x).max(0.0);
        let height = (viewport.bottom() - anchor.y - gap).max(0.0);
        // Keep drawing safe when even the header/footer cannot fit. egui clips the contents to
        // the remaining viewport; the normal small-height case still keeps both actions visible.
        let window = super::Container::new(ctx, super::Surface::Feature)
            .window(self.title)
            .id(self.id)
            .title_bar(false)
            .pivot(match side {
                crate::frame::Side::Right => egui::Align2::RIGHT_TOP,
                crate::frame::Side::Left => egui::Align2::LEFT_TOP,
            })
            .fixed_pos(anchor)
            .constrain(false)
            .collapsible(false)
            .resizable(false)
            .min_width(inner_width)
            .max_width(inner_width)
            .default_width(inner_width)
            .min_height(0.0)
            .max_height((height - margin.sum().y).max(0.0))
            .scroll(false)
            .frame(frame)
            .default_height(0.0)
            .fade_in(false)
            .fade_out(false)
            .order(egui::Order::Middle);
        if self.sized(ctx) { window.fixed_size(vec2(width, self.requested_size(ctx).y.min(height).max(0.0))) } else { window }
    }

    /// Feature header, scrolling padded body, pinned actions, drag target and compute status.
    /// Tokens: form_header_height, form_footer_height, space_6/8/10/14/16, dialog_body.
    pub(crate) fn feature(
        &self,
        ui: &mut Ui,
        heading: FeatureOptions<'_>,
        disabled: bool,
        valid: bool,
        body: impl FnOnce(&mut Ui) -> ComputeStatus,
    ) -> (bool, super::Action) {
        self.feature_impl(ui, heading, disabled, valid, None, body)
    }

    /// Same card with a persistent fold state owned by its caller.
    pub(crate) fn feature_collapsible(
        &self,
        ui: &mut Ui,
        heading: FeatureOptions<'_>,
        disabled: bool,
        valid: bool,
        collapsed: &mut bool,
        body: impl FnOnce(&mut Ui) -> ComputeStatus,
    ) -> (bool, super::Action) {
        self.feature_impl(ui, heading, disabled, valid, Some(collapsed), body)
    }

    fn feature_impl(
        &self,
        ui: &mut Ui,
        heading: FeatureOptions<'_>,
        disabled: bool,
        valid: bool,
        mut collapsed: Option<&mut bool>,
        body: impl FnOnce(&mut Ui) -> ComputeStatus,
    ) -> (bool, super::Action) {
        if !self.native_content && super::floating::native_content(ui.ctx(), self.id) {
            let mut panel = self.clone();
            panel.native_content = true;
            panel.viewport = ui.max_rect();
            panel.frame = super::Container::new(ui.ctx(), super::Surface::Panel).edge_frame(Tokens::get().dialog_body);
            panel.resizable = false;
            return panel.feature_impl(ui, heading, disabled, valid, collapsed, body);
        }
        super::scope(ui, |ui| {
            let t = Tokens::get();
            let min = ui.cursor().min;
            let band = egui::Rect::from_min_size(min, vec2(ui.available_width(), super::header_height()));
            if self.native_content {
                scenario::publish_handle("dialog:title", band.center());
                let drag = ui.interact(band, ui.id().with("native_title_drag"), egui::Sense::drag());
                if drag.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            } else {
                crate::frame::drag_dock(ui, band, heading.drag_key, heading.moved, self.bounded_anchor(ui.ctx()) - heading.docked);
                super::floating::register(ui.ctx(), self.id, heading.drag_key, heading.docked);
            }
            let mut close = false;
            let top = if DialogButtons::current(ui.ctx()) == DialogButtons::Top {
                Some(TopHeader::new(ui, self.ok, heading.apply_label, self.cancel, collapsed.as_deref_mut(), &mut close))
            } else {
                if let Some(collapsed) = collapsed.as_deref_mut() {
                    collapsible_header(ui, heading.icon, heading.title, collapsed, || close = true);
                } else {
                    header(ui, heading.icon, heading.title, || close = true);
                }
                None
            };
            let bottom = self.active_viewport(ui.ctx()).bottom()
                - super::Container::new(ui.ctx(), super::Surface::Feature).gap()
                - self.frame.inner_margin.bottomf()
                - self.frame.stroke.width;
            let footer_height = if top.is_some() { 0.0 } else { t.form_footer_height };
            let fields_height = if self.sized(ui.ctx()) || self.native_content {
                (ui.available_height() - footer_height - ui.spacing().item_spacing.y).max(0.0)
            } else {
                let available = (bottom - ui.cursor().top() - footer_height - ui.spacing().item_spacing.y).max(0.0);
                self.body_rows.map_or(available, |rows| available.min(rows * t.control_height + t.space_12 * 2.0))
            };
            // Leave a resize gutter outside the vertical scrollbar, so dragging its thumb
            // cannot also grab the card's right edge.
            let fields_width = (ui.available_width() - if self.resizable { t.space_4 } else { 0.0 }).max(0.0);
            let fields_rect = egui::Rect::from_min_size(ui.cursor().min, vec2(fields_width, fields_height));
            let state = if collapsed.as_deref().copied().unwrap_or(false) {
                scenario::publish_count("dialog:body:content_height", 0.0);
                ComputeStatus { text: None, busy: disabled, applying: disabled }
            } else {
                ui.scope_builder(egui::UiBuilder::new().max_rect(fields_rect), |ui| {
                    if self.reset_scroll {
                        // Clear persisted offset, animation and momentum once per opening.
                        egui::scroll_area::State::default().store(ui.ctx(), ui.make_persistent_id(egui::IdSalt::new("feature_fields")));
                    }
                    let scroll = egui::ScrollArea::vertical()
                        .id_salt("feature_fields")
                        .auto_shrink([!(self.sized(ui.ctx()) || self.native_content), !(self.sized(ui.ctx()) || self.native_content)])
                        .min_scrolled_height(0.0)
                        .max_height(fields_height)
                        .show(ui, |ui| {
                            if disabled {
                                ui.disable();
                            }
                            egui::Frame::NONE
                                .inner_margin(egui::Margin {
                                    left: t.space_12 as i8,
                                    right: t.space_12 as i8,
                                    top: t.space_12 as i8,
                                    bottom: t.space_12 as i8,
                                })
                                .show(ui, body)
                                .inner
                        });
                    scenario::publish_count("dialog:body:content_height", f64::from(scroll.content_size.y));
                    scenario::publish_count("dialog:body:scroll_y", f64::from(scroll.state.offset.y));
                    scenario::publish_rect("dialog:body:viewport", scroll.inner_rect);
                    scroll.inner
                })
                .inner
            };
            if let Some(top) = &top {
                let action = top.finish(ui, heading.icon, heading.title, &state, valid && !state.applying);
                super::Container::finish_content(ui, self.positioned_frame(ui.ctx()));
                if self.resizable {
                    self.resize_handles(ui, &heading);
                }
                return (close, action);
            }
            let corners = self.positioned_frame(ui.ctx()).corner_radius;
            let action = super::footer(
                ui,
                self.ok,
                self.cancel,
                valid && !state.applying,
                super::Footer { apply: heading.apply_label, status: state.text.as_deref(), corners: Some(corners), ..Default::default() },
            );
            if state.busy {
                let rect = egui::Rect::from_min_size(
                    egui::pos2(ui.min_rect().left(), ui.min_rect().bottom() - super::row_height()),
                    vec2(t.space_16, super::row_height()),
                );
                egui::Spinner::new().size(t.space_14).paint_at(ui, rect);
                let status = state.text.as_deref().unwrap_or_else(|| crate::i18n::tr_label("Updating the preview"));
                ui.interact(rect, ui.id().with("compute_status"), egui::Sense::hover()).on_hover_text(status);
                ui.ctx().request_repaint_after(std::time::Duration::from_millis(100));
            }
            super::Container::finish_content(ui, self.positioned_frame(ui.ctx()));
            if self.resizable {
                self.resize_handles(ui, &heading);
            }
            (close, action)
        })
    }

    fn resize_handles(&self, ui: &mut Ui, heading: &FeatureOptions<'_>) {
        let t = Tokens::get();
        let rect = self.positioned_frame(ui.ctx()).widget_rect(ui.min_rect());
        scenario::publish_rect(&format!("dialog:resize:{}:bounds", heading.drag_key), rect);
        let min = vec2(t.form_card_width, header_height() + t.form_footer_height + t.control_height * 3.0);
        // Resize targets straddle the card, including the half outside its content clip.
        let clip = ui.clip_rect();
        ui.set_clip_rect(ui.ctx().content_rect());
        for (name, x, y) in RESIZE_DIRECTIONS {
            let hit = resize_hit(rect, x, y, t.space_16, t.space_8, true);
            let at = hit.center();
            let id = self.id.with(("resize", name));
            let response = ui.interact(hit, id, egui::Sense::drag());
            resize_cursor(ui, hit, &response, resize_icon(x, y));
            scenario::publish_handle(&format!("dialog:resize:{}:{name}", heading.drag_key), at);
            scenario::publish_rect(&format!("dialog:resize:{}:{name}", heading.drag_key), hit);
            let base = id.with("base");
            if response.drag_started()
                && let Some(origin) = ui.input(|i| i.pointer.press_origin())
            {
                ui.ctx().data_mut(|d| d.insert_temp(base, (rect, origin)));
            }
            if (response.dragged() || response.drag_stopped())
                && let Some((start, from)) = ui.ctx().data(|d| d.get_temp::<(egui::Rect, egui::Pos2)>(base))
                && let Some(now) = ui.input(|i| i.pointer.latest_pos())
            {
                let delta = now - from;
                let mut next = start;
                if x < 0 {
                    next.min.x = (start.left() + delta.x).clamp(self.viewport.left(), (start.right() - min.x).max(self.viewport.left()));
                } else if x > 0 {
                    next.max.x = (start.right() + delta.x).clamp((start.left() + min.x).min(self.viewport.right()), self.viewport.right());
                }
                if y < 0 {
                    next.min.y = (start.top() + delta.y).clamp(self.viewport.top(), (start.bottom() - min.y).max(self.viewport.top()));
                } else if y > 0 {
                    next.max.y = (start.bottom() + delta.y).clamp((start.top() + min.y).min(self.viewport.bottom()), self.viewport.bottom());
                }
                let anchor = match self.side {
                    crate::frame::Side::Right => next.right_top(),
                    crate::frame::Side::Left => next.left_top(),
                };
                let frame = ui.ctx().cumulative_frame_nr();
                ui.ctx().data_mut(|d| {
                    d.insert_persisted(self.id.with("size"), next.size());
                    d.insert_temp(egui::Id::new(("sc_dock_offset", heading.drag_key)), anchor - heading.docked);
                    d.insert_temp(egui::Id::new("sc_dock_drag_frame"), frame);
                });
                ui.ctx().request_repaint();
            }
            // A small grip makes the resize affordance visible without competing with the actions.
            if x == 1 && y == 1 {
                for offset in [t.space_2, t.space_4] {
                    ui.painter().line_segment(
                        [at - vec2(offset + t.space_2, t.space_2), at - vec2(t.space_2, offset + t.space_2)],
                        t.tertiary_border(t.dialog_rule),
                    );
                }
            }
        }
        ui.set_clip_rect(clip);
    }

    /// Palette composition retains its compact padding and whole-card scrolling on short views.
    /// Tokens: space_24 and form_footer_height; header/body/footer own their historical padding.
    pub(crate) fn palette(&self, ui: &mut Ui, bottom: f32, top: f32, mut add: impl FnMut(&mut Ui, Part)) {
        if !self.native_content && super::floating::native_content(ui.ctx(), self.id) {
            let mut panel = self.clone();
            panel.native_content = true;
            panel.viewport = ui.max_rect();
            panel.resizable = false;
            return panel.palette(ui, bottom, top, add);
        }
        let (bottom, top) = if self.sized(ui.ctx()) || self.native_content { (ui.max_rect().bottom(), ui.max_rect().top()) } else { (bottom, top) };
        let t = Tokens::get();
        ui.set_max_height((bottom - ui.cursor().top()).max(0.0));
        super::scope(ui, |ui| {
            ui.spacing_mut().item_spacing.y = 0.0;
            let band = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), header_height()));
            let moved = crate::frame::dock_offset(ui.ctx(), "SKETCH_PALETTE");
            let docked = self.anchor - moved;
            if self.native_content {
                let response = ui.interact(band, self.id.with("native_title_drag"), egui::Sense::drag());
                if response.drag_started() {
                    ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                }
            } else {
                crate::frame::drag_dock(ui, band, "SKETCH_PALETTE", moved, self.bounded_anchor(ui.ctx()) - docked);
                super::floating::register(ui.ctx(), self.id, "SKETCH_PALETTE", docked);
            }
            let height = (bottom - top - self.frame.total_margin().sum().y).max(0.0);
            let short = height < t.space_24 * 6.0;
            let mut contents = |ui: &mut Ui| {
                add(ui, Part::Header);
                let body_height = (bottom - ui.cursor().top() - t.form_footer_height).max(0.0);
                let mut body = |ui: &mut Ui| {
                    egui::Frame::NONE.inner_margin(egui::Margin::symmetric(t.space_10 as i8, t.space_4 as i8)).show(ui, |ui| add(ui, Part::Body));
                };
                if short {
                    body(ui);
                } else {
                    let body_rect = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), body_height));
                    let scroll = ui
                        .scope_builder(egui::UiBuilder::new().max_rect(body_rect), |ui| {
                            egui::ScrollArea::vertical()
                                .id_salt("sc_palette_options")
                                .max_height(body_height)
                                .min_scrolled_height(0.0)
                                .auto_shrink([false, true])
                                .show(ui, body)
                        })
                        .inner;
                    scenario::publish_rect("sketch_palette:options", scroll.inner_rect);
                }
                add(ui, Part::Footer);
            };
            if short {
                let scroll = egui::ScrollArea::vertical()
                    .id_salt("sc_palette_content")
                    .max_height(height)
                    .min_scrolled_height(0.0)
                    .auto_shrink([false, true])
                    .show(ui, contents);
                scenario::publish_rect("sketch_palette:options", scroll.inner_rect);
            } else {
                contents(ui);
            }
            super::Container::finish_content(ui, self.positioned_frame(ui.ctx()));
            if self.resizable && !self.native_content {
                self.resize_handles(
                    ui,
                    &FeatureOptions { icon: "sketch", title: self.title, drag_key: "SKETCH_PALETTE", moved, docked, apply_label: None },
                );
            }
        });
    }
}

/// System window built on Panel's shared floating card: 40 pt header, bounded content,
/// optional page list, and 48 pt actions with a left slot. Existing IDs and editor policies
/// survive the migration. All geometry and colours derive from Tokens.
pub struct Window<'a> {
    id: egui::Id,
    icon: &'a str,
    title: &'a str,
    anchor: Option<(egui::Align2, egui::Vec2)>,
    placed: Option<(egui::Align2, egui::Pos2)>,
    ok: &'a str,
    cancel: &'a str,
    apply: Option<&'a str>,
    enter_submits: bool,
    escape_closes: bool,
    closeable: bool,
    width: f32,
    body_padding: bool,
    page_rows: f32,
    page_padding: egui::Vec2,
    left_slot: bool,
}

impl<'a> Window<'a> {
    /// A card with OK and Cancel, centred and movable.
    pub fn new(id: egui::Id, icon: &'a str, title: &'a str) -> Self {
        Self {
            id,
            icon,
            title,
            anchor: None,
            placed: None,
            ok: "OK",
            cancel: "Cancel",
            apply: None,
            enter_submits: true,
            escape_closes: true,
            closeable: true,
            width: Tokens::get().form_card_width * 1.5,
            body_padding: true,
            page_rows: 16.0,
            page_padding: vec2(Tokens::get().space_16, Tokens::get().space_16),
            left_slot: false,
        }
    }

    /// Pin the card at `align` of the screen, `offset` away (it then no longer moves).
    pub(crate) fn anchor(mut self, align: egui::Align2, offset: egui::Vec2) -> Self {
        self.anchor = Some((align, offset));
        self
    }

    /// Start with `pivot` of the card at `pos`; the card can then be moved.
    pub(crate) fn placed(mut self, pivot: egui::Align2, pos: egui::Pos2) -> Self {
        self.placed = Some((pivot, pos));
        self
    }

    /// The primary and closing actions, and an optional third one between them. An empty `ok`
    /// leaves the primary action out.
    pub(crate) fn actions(mut self, ok: &'a str, cancel: &'a str, apply: Option<&'a str>) -> Self {
        (self.ok, self.cancel, self.apply) = (ok, cancel, apply);
        self
    }

    /// A card of editable fields: Enter belongs to the fields and never submits (confirmations
    /// let Enter give the primary action when nothing has focus).
    pub(crate) fn fields(mut self) -> Self {
        self.enter_submits = false;
        self
    }

    /// Esc does not close the card (the header's close and the closing action still do): for a
    /// card holding a long text the user is editing, where Esc must not discard the work.
    pub(crate) fn keep_open_on_escape(mut self) -> Self {
        self.escape_closes = false;
        self
    }

    /// `cards` times the card width instead of one and a half, for wide footers.
    pub fn cards(mut self, cards: f32) -> Self {
        if cards.is_finite() && cards > 0.0 {
            self.width = Tokens::get().form_card_width * cards.min(16.0);
        }
        self
    }

    /// Maximum page-body height in control rows, still bounded by the viewport.
    pub(crate) fn page_rows(mut self, rows: f32) -> Self {
        if rows.is_finite() {
            self.page_rows = rows.clamp(1.0, 32.0);
        }
        self
    }

    /// Content inset for a page-based window, independent of its navigation rail.
    pub(crate) fn page_padding(mut self, padding: egui::Vec2) -> Self {
        if padding.is_finite() {
            self.page_padding = padding.max(egui::Vec2::ZERO);
        }
        self
    }

    /// A required choice, such as web recovery, has no dismiss action. Existing data remains
    /// pending until the body explicitly handles a choice.
    pub fn required(mut self) -> Self {
        self.closeable = false;
        self.escape_closes = false;
        self
    }

    /// A standard closable window for platform content and reports. Uses the same header,
    /// bounded scrolling body and fixed outer chrome as every other system window.
    /// Returns whether the shared header's close action was clicked.
    pub fn show_body(self, ctx: &egui::Context, body: impl FnMut(&mut Ui)) -> bool {
        self.fields().actions("", "", None).show(ctx, true, body).0
    }

    /// Draw shared header, content and optional footer, retaining the caller's IDs.
    pub(crate) fn show(self, ctx: &egui::Context, valid: bool, mut body: impl FnMut(&mut Ui)) -> (bool, super::Action) {
        self.movable_surface(ctx, |ui| self.content(ui, valid, |ui| body(ui), |_| {}))
    }

    /// A pinned left action slot, independent of the right-aligned footer buttons.
    pub(crate) fn show_with_left(
        mut self,
        ctx: &egui::Context,
        valid: bool,
        mut body: impl FnMut(&mut Ui),
        mut left: impl FnMut(&mut Ui),
    ) -> (bool, super::Action) {
        self.left_slot = true;
        self.movable_surface(ctx, |ui| self.content(ui, valid, |ui| body(ui), |ui| left(ui)))
    }

    pub(super) fn content(&self, ui: &mut Ui, valid: bool, body: impl FnOnce(&mut Ui), left: impl FnOnce(&mut Ui)) -> (bool, super::Action) {
        super::scope(ui, |ui| {
            let t = Tokens::get();
            ui.spacing_mut().item_spacing.y = 0.0;
            let mut close = false;
            if self.closeable && self.anchor.is_none() {
                let band = egui::Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), header_height()));
                if super::floating::native_content(ui.ctx(), self.id) {
                    let response = ui.interact(band, self.id.with("native_title_drag"), egui::Sense::drag());
                    if response.drag_started() {
                        ui.ctx().send_viewport_cmd(egui::ViewportCommand::StartDrag);
                    }
                } else {
                    let key = format!("window_{:?}", self.id);
                    let rect = ui.ctx().memory(|m| m.area_rect(self.id)).unwrap_or(ui.max_rect());
                    let moved =
                        ui.ctx().data(|d| d.get_temp::<egui::Vec2>(egui::Id::new(("sc_dock_offset", key.as_str())))).unwrap_or(rect.min.to_vec2());
                    crate::frame::drag_dock(ui, band, &key, moved, rect.min.to_vec2());
                    super::floating::register(ui.ctx(), self.id, &key, egui::Pos2::ZERO);
                }
            }
            header_impl(ui, self.icon, self.title, None, self.closeable, || close = true);
            let fill = super::floating::native_content(ui.ctx(), self.id)
                || ui.ctx().data_mut(|d| d.get_persisted::<egui::Vec2>(self.id.with("size"))).is_some();
            egui::Frame::NONE.inner_margin(if self.body_padding { t.space_12 } else { 0.0 }).show(ui, |ui| {
                ui.spacing_mut().item_spacing.y = t.form_gap_y;
                if self.body_padding {
                    let fill = super::floating::native_content(ui.ctx(), self.id)
                        || ui.ctx().data_mut(|d| d.get_persisted::<egui::Vec2>(self.id.with("size"))).is_some();
                    let footer = if self.ok.is_empty() && self.cancel.is_empty() && self.apply.is_none() && !self.left_slot {
                        0.0
                    } else {
                        t.form_footer_height
                    };
                    let height = if fill {
                        (ui.available_height() - footer - t.space_24).max(0.0)
                    } else {
                        (dimension(ui.ctx().content_rect().height()) - t.form_header_height - t.form_footer_height - t.space_24 * 2.0).max(0.0)
                    };
                    egui::ScrollArea::vertical()
                        .id_salt("window_body")
                        .max_height(height)
                        .min_scrolled_height(0.0)
                        .auto_shrink([false, !fill])
                        .show(ui, body);
                } else {
                    body(ui);
                }
            });
            if !self.enter_submits {
                ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Enter));
            }
            if !self.escape_closes {
                ui.ctx().input_mut(|i| i.consume_key(egui::Modifiers::NONE, egui::Key::Escape));
            }
            let has_footer = !(self.ok.is_empty() && self.cancel.is_empty() && self.apply.is_none() && !self.left_slot);
            if fill && has_footer {
                ui.add_space((ui.available_height() - t.form_footer_height).max(0.0));
            }
            let action = if !has_footer {
                super::Action::None
            } else {
                super::actions::footer_with_left(ui, self.ok, self.cancel, valid, super::Footer { apply: self.apply, ..Default::default() }, left)
            };
            (close, action)
        })
    }

    /// Declarative page navigation: the kit owns geometry, selection and page scrolling.
    pub(crate) fn show_pages<S>(
        mut self,
        ctx: &egui::Context,
        selected: &mut usize,
        pages: &[WindowPage<S>],
        state: &mut S,
        left: impl FnMut(&mut Ui),
    ) -> (bool, super::Action) {
        self.body_padding = false;
        let rows = self.page_rows;
        let padding = self.page_padding;
        let id = self.id;
        self.show_with_left(
            ctx,
            true,
            |ui| {
                let fill =
                    super::floating::native_content(ui.ctx(), id) || ui.ctx().data_mut(|d| d.get_persisted::<egui::Vec2>(id.with("size"))).is_some();
                let rows = if fill {
                    rows.min((ui.available_height() - Tokens::get().form_footer_height).max(0.0) / Tokens::get().control_height)
                } else {
                    rows
                };
                page_list_sized(ui, selected, pages, state, rows, padding)
            },
            left,
        )
    }

    fn movable_surface(&self, ctx: &egui::Context, mut add: impl FnMut(&mut Ui) -> (bool, super::Action)) -> (bool, super::Action) {
        if !self.closeable || self.anchor.is_some() {
            return self.surface(ctx, false, add);
        }
        let frame = Panel::card_frame(ctx);
        let key = format!("window_{:?}", self.id);
        if let Some(position) = ctx.data_mut(|d| d.remove_temp::<egui::Pos2>(self.id.with("floating_dock_position"))) {
            ctx.data_mut(|d| d.insert_temp(egui::Id::new(("sc_dock_offset", key.as_str())), position.to_vec2()));
        }
        let saved = super::floating::drag_size(ctx, self.id)
            .or_else(|| ctx.data_mut(|d| d.get_persisted::<egui::Vec2>(self.id.with("size"))))
            .filter(|s| s.is_finite() && s.x > 0.0 && s.y > 0.0);
        let width = saved.map_or(self.width, |s| s.x).min(dimension(ctx.content_rect().width()));
        let inner_width = (width - frame.total_margin().sum().x).max(0.0);
        let moved = ctx.data(|d| d.get_temp::<egui::Vec2>(egui::Id::new(("sc_dock_offset", key.as_str()))));
        let mut window = super::Container::new(ctx, super::Surface::Window)
            .window(self.title)
            .id(self.id)
            .title_bar(false)
            .movable(false)
            .collapsible(false)
            .resizable(false)
            .constrain(!crate::frame::title_dragging(ctx, &key))
            .min_width(inner_width)
            .max_width(inner_width)
            .default_width(inner_width)
            .min_height(0.0)
            .scroll(false)
            .frame(frame)
            .fade_in(false)
            .fade_out(false);
        if let Some(size) = saved {
            window = window.fixed_size(size.min(ctx.content_rect().size()));
        }
        window = if let Some(moved) = moved {
            let mut position = moved.to_pos2();
            if !crate::frame::title_dragging(ctx, &key) {
                let size = saved
                    .or_else(|| ctx.memory(|m| m.area_rect(self.id)).map(|r| r.size()))
                    .unwrap_or(vec2(width, Tokens::get().control_height * 8.0));
                let vp = ctx.content_rect();
                position.x = position.x.clamp(vp.left(), (vp.right() - size.x).max(vp.left()));
                position.y = position.y.clamp(vp.top(), (vp.bottom() - size.y).max(vp.top()));
            }
            window.pivot(egui::Align2::LEFT_TOP).fixed_pos(position)
        } else if let Some((pivot, pos)) = self.placed {
            window.pivot(pivot).default_pos(pos)
        } else {
            window.pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center() - vec2(0.0, row_height() * 2.5))
        };
        let shown = super::floating::show(ctx, self.id, self.title, window, |ui| {
            let result = add(ui);
            if !super::floating::native_content(ui.ctx(), self.id) {
                let panel = Panel::new(self.title, self.id, ui.min_rect().min, ctx.content_rect(), crate::frame::Side::Left, frame)
                    .width(width)
                    .resizable_to_content();
                panel.resize_handles(
                    ui,
                    &FeatureOptions {
                        icon: self.icon,
                        title: self.title,
                        drag_key: &key,
                        moved: moved.unwrap_or_default(),
                        docked: egui::Pos2::ZERO,
                        apply_label: None,
                    },
                );
            }
            super::Container::finish_content(ui, frame);
            result
        });
        if let Some(shown) = &shown {
            scenario::publish_rect("dialog:window", shown.response.rect);
        }
        shown.and_then(|r| r.inner).unwrap_or((false, super::Action::None))
    }

    /// The same bounded card frame, either movable or an input-blocking modal.
    pub(super) fn surface(&self, ctx: &egui::Context, modal: bool, add: impl FnOnce(&mut Ui) -> (bool, super::Action)) -> (bool, super::Action) {
        let frame = Panel::card_frame(ctx);
        let inner_width = (self.width.min(dimension(ctx.content_rect().width())) - frame.total_margin().sum().x).max(0.0);
        if modal {
            let shown = super::Container::new(ctx, super::Surface::Window)
                .modal(self.id)
                .backdrop_color(Tokens::get().tab_on_fill_dark.gamma_multiply(0.35))
                .frame(frame)
                .show(ctx, |ui| {
                    ui.set_width(inner_width);
                    let result = add(ui);
                    super::Container::finish_content(ui, frame);
                    result
                });
            scenario::publish_rect("dialog:window", shown.response.rect);
            return shown.inner;
        }
        let window = super::Container::new(ctx, super::Surface::Window)
            .window(self.title)
            .id(self.id)
            .title_bar(false)
            .collapsible(false)
            .resizable(false)
            .min_width(inner_width)
            .max_width(inner_width)
            .default_width(inner_width)
            .scroll(false)
            .frame(frame)
            .fade_in(false)
            .fade_out(false);
        let window = match (self.anchor, self.placed) {
            (Some((align, offset)), _) => window.anchor(align, offset),
            (None, Some((pivot, pos))) => window.pivot(pivot).default_pos(pos),
            (None, None) => window.pivot(egui::Align2::CENTER_CENTER).default_pos(ctx.content_rect().center() - vec2(0.0, row_height() * 2.5)),
        };
        let shown = window.show(ctx, |ui| {
            let result = add(ui);
            super::Container::finish_content(ui, frame);
            result
        });
        if let Some(shown) = &shown {
            scenario::publish_rect("dialog:window", shown.response.rect);
        }
        shown.and_then(|r| r.inner).unwrap_or((false, super::Action::None))
    }
}

/// A page's existing icon, translated label key, stable handle and draw callback.
pub(crate) struct WindowPage<S> {
    pub icon: &'static str,
    pub label: &'static str,
    pub group: Option<&'static str>,
    pub handle: &'static str,
    pub scroll: bool,
    pub draw: fn(&mut Ui, &mut S),
}

#[cfg(test)]
pub(super) fn page_list<S>(ui: &mut Ui, selected: &mut usize, pages: &[WindowPage<S>], state: &mut S) {
    page_list_sized(ui, selected, pages, state, 16.0, vec2(Tokens::get().space_16, Tokens::get().space_16));
}

fn page_list_sized<S>(ui: &mut Ui, selected: &mut usize, pages: &[WindowPage<S>], state: &mut S, rows: f32, padding: egui::Vec2) {
    let t = Tokens::get();
    let height = (dimension(ui.ctx().content_rect().height()) - t.form_header_height - t.form_footer_height - t.space_24 * 3.0)
        .clamp(0.0, t.control_height * rows);
    let (body, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), height), egui::Sense::hover());
    let width = (t.form_label_width * 2.0 + t.space_24 * 2.0).min(body.width());
    let nav = egui::Rect::from_min_size(body.min, vec2(width, body.height()));
    ui.painter().rect_filled(nav, 0.0, t.dialog_footer);
    t.secondary_vline(ui.painter(), nav.right(), nav.y_range(), t.dialog_rule);
    scenario::publish_rect("dialog:pages", nav);
    *selected = (*selected).min(pages.len().saturating_sub(1));
    ui.scope_builder(egui::UiBuilder::new().max_rect(nav.shrink2(vec2(t.space_8, t.space_6))), |ui| {
        ui.set_clip_rect(ui.clip_rect().intersect(nav));
        ui.spacing_mut().item_spacing.y = t.space_2;
        egui::ScrollArea::vertical().id_salt("window_pages").show(ui, |ui| {
            for (index, page) in pages.iter().enumerate() {
                if let Some(group) = page.group {
                    ui.add_space(t.space_8);
                    let (rect, _) = ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.space_24), egui::Sense::hover());
                    let rect = rect.shrink2(vec2(t.space_10, 0.0));
                    paint_text(ui, rect, &crate::i18n::tr(group).to_uppercase(), crate::fonts::semibold(t.font_small), t.form_dim, true);
                }
                let label = crate::i18n::tr(page.label);
                let (rect, response) =
                    ui.allocate_exact_size(vec2(dimension(ui.available_width()), t.control_height + t.space_4), egui::Sense::click());
                if response.clicked() {
                    *selected = index;
                }
                let on = *selected == index;
                if on || response.hovered() || response.has_focus() {
                    ui.painter().rect_filled(rect, t.control_radius, if on { t.selected_bg } else { t.hover });
                }
                let ink = if on { t.selected_ink } else { t.form_ink };
                let icon =
                    egui::Rect::from_center_size(egui::pos2(rect.left() + t.space_10 + t.space_8, rect.center().y), vec2(t.space_16, t.space_16));
                crate::icons::paint_ink(ui.painter(), icon, page.icon, if on { ink } else { t.form_dim });
                let text_rect = egui::Rect::from_min_max(egui::pos2(icon.right() + t.space_8, rect.top()), rect.right_bottom());
                paint_text(
                    ui,
                    text_rect,
                    label,
                    if on { crate::fonts::semibold(t.font_label) } else { egui::FontId::proportional(t.font_label) },
                    ink,
                    false,
                );
                response.widget_info(|| egui::WidgetInfo::selected(egui::WidgetType::SelectableLabel, ui.is_enabled(), on, label));
                if response.hovered() {
                    ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
                }
                scenario::publish_target(page.handle, rect);
                response.on_hover_text(label);
            }
        });
    });
    let content = egui::Rect::from_min_max(egui::pos2(nav.right(), body.top()), body.max);
    let content = content.shrink2(vec2(padding.x.min(content.width() / 2.0), padding.y.min(content.height() / 2.0)));
    ui.scope_builder(
        egui::UiBuilder::new()
            .max_rect(content)
            .ui_stack_info(egui::UiStackInfo::default().with_tag_value(super::preferences::PAGE_SCROLL_GUTTER, padding.x)),
        |ui| {
            let clip = egui::Rect::from_min_max(content.min, egui::pos2(body.right(), content.bottom()));
            ui.set_clip_rect(ui.clip_rect().intersect(clip));
            ui.spacing_mut().item_spacing.y = t.form_gap_y;
            if let Some(page) = pages.get(*selected) {
                if page.scroll {
                    let scroll = super::settings_scroll(ui, ("sc_prefs_page", *selected), |ui| (page.draw)(ui, state));
                    scenario::publish_rect("dialog:page_body", scroll.inner_rect);
                } else {
                    (page.draw)(ui, state);
                    scenario::publish_rect("dialog:page_body", content);
                }
            }
        },
    );
}

#[cfg(test)]
#[path = "panel_tests.rs"]
mod tests;
