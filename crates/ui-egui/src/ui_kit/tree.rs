//! Shared Browser tree rows; feature modules supply content and dispatch commands.
use crate::icons::glyphs::badge;
use crate::{icons, theme::Tokens};
use egui::{Color32, FontId, Pos2, Rect, Sense, Stroke, Ui, pos2, vec2};
const ROW_H: f32 = super::TREE_ROW_HEIGHT;
const INDENT: f32 = 14.0;
const DIM_ICON_OPACITY: f32 = 0.4;
/// A drag over the eyes scrolls the rows when the pointer is this near their top or bottom edge,
/// by this many points a frame (#9680).
const DRAG_EDGE: f32 = 24.0;
const DRAG_SCROLL: f32 = 6.0;

/// Trailing tree action; disabled actions retain their reason and a scenario rectangle.
pub(crate) struct TreeAction<'a> {
    pub icon: &'a str,
    pub hover_only: bool,
    pub label: &'a str,
    pub reason: Option<&'a str>,
    pub target: &'a str,
}

#[derive(Default)]
pub(crate) struct Row<'a> {
    pub(crate) depth: usize,
    /// Compact single-line scene rows, with inset content and a trailing eye.
    pub(crate) compact: bool,
    /// Some(open) for rows that fold.
    pub(crate) fold: Option<bool>,
    /// Some(visible) for rows with an eye.
    pub(crate) eye: Option<bool>,
    /// The eye between the fold arrow and the radio, as before #8718, instead of at the row's end.
    pub(crate) eye_left: bool,
    pub(crate) icon: &'a str,
    pub(crate) label: &'a str,
    /// The label is a name someone gave (a body, sketch, component, …): it is shown as typed,
    /// not translated when it happens to match a UI label.
    pub(crate) name: bool,
    pub(crate) selected: bool,
    pub(crate) dim: bool,
    /// What Cut has marked, until Paste moves it: the row has a dashed outline.
    pub(crate) cut: bool,
    pub(crate) color: Option<Color32>,
    /// Draw the icon in this one colour instead of its artwork (a sketch's solve state, #7245).
    pub(crate) icon_ink: Option<Color32>,
    /// The row's tooltip (a sketch's solve state), after its full name when that is cut short.
    pub(crate) tip: Option<&'a str>,
    /// A marked feature's or folder's tooltip, in place of [`Row::tip`].
    pub(crate) status_tip: Option<&'a StatusTip>,
    /// Component rows: is it the active one?
    pub(crate) radio: Option<bool>,
    /// Trailing badges: "lock", "pin".
    pub(crate) badges: &'a [&'a str],
    /// A button right after the name: (icon, tooltip label), e.g. a feature's profile sketch.
    pub(crate) action: Option<(&'a str, &'a str)>,
    /// A message id the row's tooltip names it by (a body row: mesh, surface or solid body).
    pub(crate) kind_tip: Option<&'a str>,
    /// Lit by the selection or the 3D view's pointer: the item's own row, or a row leading to it.
    pub(crate) mark: Mark,
    /// A feature that failed or built with a warning, or a folded row holding one: its mark at
    /// the row's end, the message in [`Row::tip`].
    pub(crate) status: Option<Status>,
    /// Optional document appearance and shared trailing text slots.
    pub(crate) swatch: Option<Color32>,
    pub(crate) secondary: &'a str,
    pub(crate) count: &'a str,
    pub(crate) modified: bool,
    pub(crate) subtitle: &'a str,
    pub(crate) actions: &'a [TreeAction<'a>],
    pub(crate) radio_hint: &'a str,
}

/// How a row relates to what the selection and the 3D view's pointer light.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub(crate) enum Mark {
    #[default]
    None,
    /// A row a lit item is inside (its component, placement, folder or group): a faint bar.
    Branch,
    /// A lit item's own row: a solid bar, and the hover colour unless it is selected.
    Here,
}

/// A feature's build state as its row marks it; the worse one sorts last.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Status {
    Warning,
    Error,
}

/// The tooltip of a feature's mark: its name and type, then the error in the error colour and
/// the warning, each wrapping on lines of their own.
#[derive(Clone, Debug, Default, PartialEq)]
pub(crate) struct StatusTip {
    pub(crate) head: String,
    pub(crate) error: Option<String>,
    pub(crate) warning: Option<String>,
}

impl StatusTip {
    pub(crate) fn show(&self, ui: &mut Ui) {
        super::text(ui, &self.head, super::Text::Report("status_head", false));
        self.messages(ui);
    }

    /// The error and the warning alone, under a head of the caller's own.
    pub(crate) fn messages(&self, ui: &mut Ui) {
        if let Some(error) = &self.error {
            super::text(ui, error, super::Text::Report("status_error", true));
        }
        if let Some(warning) = &self.warning {
            super::text(ui, warning, super::Text::Report("status_warning", false));
        }
    }
}

impl Status {
    /// The icon and the scenario handle's word.
    pub(crate) fn icon(self) -> &'static str {
        match self {
            Status::Warning => "warning",
            Status::Error => "error",
        }
    }
}

/// What happened on a row.
#[derive(Default)]
pub(crate) struct RowResp {
    pub(crate) rect: Option<Rect>,
    pub(crate) clicked: bool,
    pub(crate) double: bool,
    pub(crate) secondary: Option<Pos2>,
    pub(crate) fold: bool,
    /// The eye was clicked, or a drag over the eyes sets it (#9680, see [`eye_drag_id`]).
    pub(crate) eye: bool,
    pub(crate) eye_rect: Option<Rect>,
    pub(crate) radio: bool,
    /// The row's [`Row::action`] button was clicked.
    pub(crate) action: bool,
    pub(crate) hovered: bool,
    /// The row's response (drag and drop).
    pub(crate) resp: Option<egui::Response>,
    /// The press began on the fold arrow, eye or radio (not a drag of the row).
    pub(crate) on_control: bool,
    /// Centres of the fold arrow, eye and radio, for rows that publish them under other handles.
    pub(crate) fold_at: Option<Pos2>,
    pub(crate) eye_at: Option<Pos2>,
    pub(crate) radio_at: Option<Pos2>,
    /// Where the label starts.
    pub(crate) label_left: Option<f32>,
    pub(crate) actions: Vec<egui::Response>,
}

/// The state a drag over the eyes sets them to: the first eye's new state, while the button is
/// down (egui temporary data). Absent when no eye drag is under way.
pub(crate) fn eye_drag_id() -> egui::Id {
    egui::Id::new("sc_tree_eye_drag")
}

/// Geometry used by Browser culling and the shared row painter.
pub(crate) fn tree_row_rect(ui: &egui::Ui, row: &Row<'_>) -> Rect {
    let t = Tokens::get();
    let row_height = if row.compact { t.control_height } else { ROW_H };
    let height = row_height + if row.compact || row.subtitle.is_empty() { 0.0 } else { t.font_small + t.space_4 };
    Rect::from_min_size(ui.cursor().min, vec2(ui.available_width(), height))
}

pub(crate) fn tree_row(ui: &mut egui::Ui, id: egui::Id, row: &Row) -> RowResp {
    let t = Tokens::get();
    // Rows scrolled out of view only take their space: no widgets, no painting.
    let r = tree_row_rect(ui, row);
    let height = r.height();
    let row_height = if row.compact { t.control_height } else { ROW_H };
    if !ui.is_rect_visible(r) {
        ui.allocate_space(r.size());
        return RowResp { rect: Some(r), ..Default::default() };
    }
    // A stable id per row (not the layout position), so a press and its release stay on the
    // same row while rows above open or close.
    let (r, _) = ui.allocate_exact_size(vec2(ui.available_width(), height), Sense::hover());
    let resp = ui.interact(r, id, Sense::click_and_drag());
    // While something is dragged (an appearance swatch, drawn under the pointer) `hovered` is
    // off; the row under the pointer still counts, so a drop lands on it.
    let dragged_over = egui::DragAndDrop::has_any_payload(ui.ctx()) && ui.input(|i| i.pointer.hover_pos()).is_some_and(|p| r.contains(p));
    let mut out = RowResp { rect: Some(r), hovered: resp.hovered() || dragged_over, ..Default::default() };
    if row.selected {
        ui.painter().rect_filled(r, if row.compact { 0 } else { t.radius_sm.saturating_sub(1) }, crate::theme::TabAccent::current(ui.ctx()).soft);
    } else if resp.hovered() || row.mark == Mark::Here {
        ui.painter().rect_filled(r, t.radius_sm.saturating_sub(1), t.hover);
    }
    // A lit item (selected, or under the pointer in the 3D view): a bar down the left of its row,
    // and a fainter, thinner one on each row leading to it, so the bars trace its branch.
    let bar = match if row.compact { Mark::None } else { row.mark } {
        Mark::None => None,
        Mark::Branch => Some((2.0, t.accent.gamma_multiply(0.4))),
        Mark::Here => Some((3.0, t.accent)),
    };
    if let Some((w, colour)) = bar {
        ui.painter().rect_filled(Rect::from_min_size(r.left_top() + vec2(0.0, t.space_2), vec2(w, r.height() - 4.0)), t.radius_sm / 4, colour);
    }
    let mut x = r.left() + if row.compact { t.space_8 + row.depth as f32 * t.space_14 } else { 2.0 + row.depth as f32 * INDENT };
    let cy = r.top() + row_height / 2.0;
    // Where the row's depth puts it, whatever controls it has.
    crate::scenario::publish_handle(&format!("indent:{}", row.label), pos2(x, cy));
    let action_width = if row.compact { t.space_16 } else { t.form_close_size };
    let action_left = (r.right() - if row.compact { t.space_8 } else { t.space_4 } - action_width * row.actions.len() as f32).max(r.left());
    for (index, action) in row.actions.iter().enumerate() {
        let rect = Rect::from_center_size(pos2(action_left + (index as f32 + 0.5) * action_width, cy), vec2(action_width, t.space_24));
        // Hover actions keep a stable hit target and disabled tooltip without boxed letters.
        let show = !action.hover_only || r.contains(ui.input(|i| i.pointer.hover_pos()).unwrap_or_default()) || row.selected;
        let response = ui
            .push_id(action.target, |ui| {
                ui.add_enabled_ui(action.reason.is_none(), |ui| {
                    if show {
                        super::quiet_icon_button_at(ui, rect, action.icon, action.label)
                    } else {
                        ui.interact(rect, ui.id().with(action.target), Sense::hover())
                    }
                })
                .inner
            })
            .inner;
        response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, response.enabled(), action.label));
        let response = match action.reason {
            Some(reason) => response.on_disabled_hover_text(format!("{}\n{reason}", action.label)),
            None => response.on_hover_text(action.label),
        };
        crate::scenario::publish_rect(action.target, rect);
        if action.reason.is_none() {
            crate::scenario::publish_target(action.target, rect);
        }
        out.actions.push(response);
    }
    // The row is one widget: where a click lands decides what it does (the fold arrow with
    // padding, the eye, the radio, or the row itself), so a click on the arrow never also
    // selects the row or starts a drag.
    let fold_r = Rect::from_min_max(pos2(x - 4.0, r.top()), pos2(x + 15.0, r.bottom()));
    // At the row's end the eyes form one column, clear of a floating scroll bar laid over the rows.
    let eye_end = !row.compact && !row.eye_left;
    let scroll = &ui.spacing().scroll;
    let bar = if scroll.floating { (scroll.bar_width + scroll.bar_outer_margin - scroll.floating_allocated_width).max(0.0) } else { 0.0 };
    let eye_width = t.space_16 + t.space_3;
    let eye_x = if row.compact {
        action_left - t.space_6
    } else if eye_end {
        action_left.min(r.right() - bar) - eye_width / 2.0
    } else {
        x + 14.0 + 8.0
    };
    let eye_r = Rect::from_center_size(pos2(eye_x, cy), vec2(eye_width, r.height()));
    // After the fold arrow: the eye's slot, or a gap that keeps the arrow and the radio apart.
    let slot = if eye_end { t.space_10 } else { 18.0 };
    let radio_x = x + 14.0 + slot + 6.0;
    let radio_r = Rect::from_center_size(pos2(radio_x, cy), vec2(t.space_12 + t.space_3, r.height()));
    // The button right after the name takes room from the label; it is placed once the label is laid out.
    let (name_action_gap, name_action_size) = (t.space_8, t.space_16 + t.space_4);
    let name_action_width = if row.action.is_some() { name_action_gap + name_action_size } else { 0.0 };
    let at = |p: Pos2| -> u8 {
        if !row.actions.is_empty() && p.x >= action_left {
            4
        } else if row.fold.is_some() && fold_r.contains(p) {
            1
        } else if row.eye.is_some() && eye_r.contains(p) {
            2
        } else if row.radio.is_some() && radio_r.contains(p) {
            3
        } else {
            0
        }
    };
    let press_on = ui.input(|i| i.pointer.press_origin()).map_or(0, at);
    let click_on = resp.interact_pointer_pos().map_or(0, at);
    let hover_on = resp.hover_pos().map_or(0, at);
    // Fold arrow.
    if let Some(open) = row.fold {
        crate::ui_kit::disclosure(ui.painter(), pos2(x + 6.0, cy), open, if hover_on == 1 { t.text } else { t.text_dim });
        out.fold = resp.clicked() && click_on == 1;
        out.fold_at = Some(fold_r.center());
        crate::scenario::publish_handle(&format!("fold:{}", row.label), fold_r.center());
    }
    x += if row.compact { t.space_16 } else { 14.0 };
    // Eye.
    if let Some(vis) = row.eye {
        let eye_size = if row.compact { t.space_14 } else { t.space_12 + t.space_3 };
        let er = Rect::from_center_size(eye_r.center(), vec2(eye_size, eye_size));
        // Hidden or faded rows keep main's dimming, with the rounded suppression asset.
        icons::paint_icon(ui.painter(), er, if vis { "eye" } else { "suppress" }, if vis && !row.dim { 0.85 } else { DIM_ICON_OPACITY });
        out.eye = resp.clicked() && click_on == 2;
        // A press on an eye that turns into a drag takes the opposite state, and each eye the
        // pointer then passes over takes that same state (#9680).
        if resp.drag_started() && press_on == 2 {
            ui.data_mut(|d| d.insert_temp(eye_drag_id(), !vis));
            out.eye = true;
        } else if let Some(p) = ui.input(|i| i.pointer.primary_down().then(|| i.pointer.hover_pos()).flatten())
            && at(p) == 2
            && ui.rect_contains_pointer(eye_r)
            && let Some(target) = ui.data(|d| d.get_temp::<bool>(eye_drag_id()))
        {
            out.eye |= target != vis;
            // Within a few points of the rows' top or bottom edge, the drag scrolls them.
            let clip = ui.clip_rect();
            if p.y < clip.top() + DRAG_EDGE {
                ui.scroll_to_rect(Rect::from_min_size(pos2(r.left(), clip.top() - DRAG_SCROLL), vec2(1.0, 1.0)), None);
            } else if p.y > clip.bottom() - DRAG_EDGE {
                ui.scroll_to_rect(Rect::from_min_size(pos2(r.left(), clip.bottom() + DRAG_SCROLL), vec2(1.0, 1.0)), None);
            }
            ui.ctx().request_repaint();
        }
        out.eye_rect = Some(eye_r.intersect(ui.clip_rect()));
        out.eye_at = Some(er.center());
        crate::scenario::publish_handle(&format!("eye:{}", row.label), er.center());
    }
    if !row.compact {
        x += slot;
    }
    // Activation radio (components).
    if let Some(active) = row.radio {
        let rc = pos2(x + 6.0, cy);
        ui.painter().circle_stroke(rc, 5.0, Stroke::new(1.2, if active { t.accent } else { t.text_dim }));
        if active {
            ui.painter().circle_filled(rc, 2.8, t.accent);
        }
        out.radio = resp.clicked() && click_on == 3;
        out.radio_at = Some(radio_r.center());
        x += 15.0;
    }
    if !row.icon.is_empty() && !(row.compact && row.swatch.is_some()) {
        let ir = Rect::from_min_size(pos2(x, cy - 8.0), vec2(t.space_16, t.space_16));
        // Icons are full-colour images (a tint colour only sets their opacity): a hidden or
        // faded row draws its icon faint, like its label.
        let opacity = if row.dim { DIM_ICON_OPACITY } else { 1.0 };
        match row.icon_ink {
            Some(ink) => icons::paint_ink(ui.painter(), ir, row.icon, ink.gamma_multiply(opacity)),
            None => icons::paint_icon(ui.painter(), ir, row.icon, opacity),
        }
        x += 21.0;
    }
    if let Some(color) = row.swatch {
        if row.compact {
            x += t.space_3;
        }
        let size = if row.compact { t.space_14 } else { t.space_12 };
        let swatch = Rect::from_center_size(pos2(x + size / 2.0, cy), vec2(size, size));
        let radius = if row.compact { (size / 2.0) as u8 } else { t.radius_sm };
        ui.painter().rect_filled(swatch, radius, color);
        ui.painter().rect_stroke(swatch, radius, t.control_border_stroke(t.control_border), egui::StrokeKind::Inside);
        x += if row.compact { t.space_14 + t.space_6 } else { t.space_16 };
    }
    let col =
        if row.selected { crate::theme::TabAccent::current(ui.ctx()).ink } else { row.color.unwrap_or(if row.dim { t.text_dim } else { t.text }) };
    let label_display = if row.name { crate::i18n::tr_name(row.label) } else { crate::i18n::tr_dyn(row.label) };
    let font = if row.selected { crate::fonts::semibold(t.font_label) } else { FontId::proportional(t.font_label) };
    // The label stops before the badges and the card's edge; a longer name ends in an ellipsis
    // and its full text is in the tooltip.
    let badges_width = row.badges.len() as f32 * 15.0;
    let end = if (row.compact || eye_end) && row.eye.is_some() { eye_r.left() - t.space_4 } else { action_left };
    let mut right = end - badges_width;
    if let Some(status) = row.status {
        right -= t.space_16;
        let sr = Rect::from_center_size(pos2(right + t.space_8, cy), vec2(t.space_14, t.space_14));
        icons::paint_icon(ui.painter(), sr, status.icon(), 1.0);
        crate::scenario::publish_handle(&format!("status:{}:{}", status.icon(), row.label), sr.center());
        right -= t.space_4;
    }
    for text in [row.count, row.secondary, if row.compact { row.subtitle } else { "" }] {
        if text.is_empty() {
            continue;
        }
        let max_width = (right - x).max(0.0) * if row.compact { 0.65 } else { 0.45 };
        let mut job = egui::text::LayoutJob::simple(text.to_string(), FontId::proportional(t.font_small), t.text_dim, max_width);
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('\u{2026}');
        let galley = ui.painter().layout_job(job);
        right -= galley.size().x;
        ui.painter().with_clip_rect(ui.clip_rect().intersect(r)).galley(pos2(right, cy - galley.size().y / 2.0), galley, t.text_dim);
        right -= t.space_6;
    }
    if row.modified {
        right -= t.space_8;
        ui.painter().circle_filled(pos2(right, cy), t.space_2, t.accent);
    }
    if !row.compact && !row.subtitle.is_empty() {
        let mut job = egui::text::LayoutJob::simple(row.subtitle.to_string(), FontId::proportional(t.font_small), t.text_dim, (right - x).max(0.0));
        job.wrap.max_rows = 1;
        job.wrap.overflow_character = Some('\u{2026}');
        let galley = ui.painter().layout_job(job);
        ui.painter().with_clip_rect(ui.clip_rect().intersect(r)).galley(pos2(x, r.top() + ROW_H), galley, t.text_dim);
    }
    let room = (right - name_action_width - x).max(0.0);
    let mut job = egui::text::LayoutJob::simple(label_display.to_string(), font, col, room);
    job.wrap.max_rows = 1;
    job.wrap.break_anywhere = true;
    job.wrap.overflow_character = Some('\u{2026}');
    let galley = ui.painter().layout_job(job);
    let lw = galley.size().x;
    let elided = galley.elided;
    crate::scenario::publish_rect(&format!("row_label:{}", row.label), Rect::from_min_size(pos2(x, cy - galley.size().y / 2.0), galley.size()));
    ui.painter().with_clip_rect(ui.painter().clip_rect().intersect(r)).galley(pos2(x, cy - galley.size().y / 2.0), galley, col);
    if row.cut {
        let inside = r.shrink2(vec2(t.space_2, t.space_2));
        let corners = [inside.left_top(), inside.right_top(), inside.right_bottom(), inside.left_bottom(), inside.left_top()];
        ui.painter().add(egui::Shape::dashed_line(&corners, Stroke::new(1.2, t.text_dim), 4.0, 3.0));
    }
    out.label_left = Some(x);
    let mut bx = (x + lw + 6.0 + name_action_width).min(if eye_end && row.eye.is_some() { end } else { r.right() } - 16.0);
    for b in row.badges {
        let br = Rect::from_center_size(pos2(bx + 6.0, cy), vec2(t.space_12, t.space_12));
        badge(ui.painter(), br, b, t.text_dim);
        bx += 15.0;
    }
    // The name's action button sits right after the label; a press on it is not a press on the row.
    // The gap before it belongs to the row: a click that lands just past the end of the name, or
    // a press on the name that slips a few points before its release, selects the row instead of
    // opening the feature's sketch (#10079). The gap is wider than egui's click slop (6 points).
    let action_r = Rect::from_min_size(pos2(x + lw + name_action_gap, r.top()), vec2(name_action_size, r.height()));
    let over_action = |p: Option<Pos2>| row.action.is_some() && p.is_some_and(|p| action_r.contains(p));
    let action_hover = over_action(resp.hover_pos());
    let action_click = over_action(resp.interact_pointer_pos());
    if let Some((icon, _)) = row.action {
        if action_hover {
            ui.painter().rect_filled(action_r.shrink2(vec2(1.0, t.space_3)), t.radius_sm.saturating_sub(1), t.hover);
        }
        let ir = Rect::from_center_size(action_r.center(), vec2(t.space_14, t.space_14));
        icons::paint_icon(ui.painter(), ir, icon, if action_hover { 1.0 } else { 0.6 });
        out.action = resp.clicked() && action_click;
        crate::scenario::publish_handle(&format!("action:{}", row.label), action_r.center());
    }
    crate::scenario::publish_handle(&format!("row:{}", row.label), r.center());
    let tip = match hover_on {
        0 if action_hover => row.action.map(|(_, tip)| crate::i18n::tr_label(tip)),
        1 => Some(if row.fold == Some(true) { crate::i18n::tr("ui.collapse") } else { crate::i18n::tr("ui.expand") }),
        2 => Some(if row.eye == Some(true) { crate::i18n::tr("ui.hide") } else { crate::i18n::tr("ui.show") }),
        3 if !row.radio_hint.is_empty() => Some(row.radio_hint),
        3 => Some(if row.radio == Some(true) { crate::i18n::tr("ui.active_component") } else { crate::i18n::tr("ui.activate") }),
        _ => None,
    };
    let state = row.kind_tip.map(crate::i18n::tr).or(row.tip);
    let resp = match (tip, state, row.status_tip) {
        // The eye at the row's end shows its tip there, not under the name.
        (Some(t), _, _) if hover_on == 2 && eye_end => {
            let mut at_eye = resp.clone();
            (at_eye.rect, at_eye.interact_rect) = (eye_r, eye_r);
            at_eye.on_hover_text(t);
            resp
        }
        (Some(t), _, _) => resp.on_hover_text(t),
        (None, _, Some(status)) => resp.on_hover_ui(|ui| status.show(ui)),
        (None, Some(state), None) if elided => resp.on_hover_text(format!("{label_display}\n{state}")),
        (None, Some(state), None) => resp.on_hover_text(state),
        (None, None, None) if !row.secondary.is_empty() || !row.count.is_empty() => {
            resp.on_hover_text(format!("{label_display}\n{}\n{}", row.secondary, row.count))
        }
        (None, None, None) if elided => resp.on_hover_text(label_display.to_string()),
        (None, None, None) => resp,
    };
    out.on_control = press_on != 0 || over_action(ui.input(|i| i.pointer.press_origin()));
    out.clicked = resp.clicked() && click_on == 0 && !action_click;
    out.double = resp.double_clicked() && click_on == 0 && !action_click;
    out.resp = Some(resp.clone());
    if resp.secondary_clicked() {
        out.secondary = resp.interact_pointer_pos().or(Some(r.left_bottom()));
    }
    out
}

/// Tree-card title band with compact lens controls and a quiet search action.
pub(crate) fn tree_header(ui: &mut egui::Ui, title: &str, lens: &mut usize, labels: &[&str], search_label: &str) -> bool {
    let t = Tokens::get();
    let (band, _) = ui.allocate_exact_size(vec2(ui.available_width(), t.form_header_height), Sense::hover());
    let search = Rect::from_center_size(pos2(band.right() - t.space_8 - t.space_12, band.center().y), vec2(t.space_24, t.space_24));
    let response = super::quiet_icon_button_at(ui, search, "zoom", search_label);
    crate::scenario::publish_target("render:tree:search", search);
    let lens_width = labels
        .iter()
        .map(|label| ui.painter().layout_no_wrap((*label).into(), crate::fonts::semibold(t.font_small), t.text).size().x + t.space_16 + t.space_4)
        .sum::<f32>();
    let lens_rect = Rect::from_min_max(
        pos2((search.left() - t.space_8 - lens_width).max(band.left() + t.space_24 * 2.0), band.center().y - (t.control_height - t.space_4) / 2.0),
        pos2(search.left() - t.space_8, band.bottom()),
    );
    // The header band already owns this space. A scoped layout would advance the
    // parent's cursor to the shorter lens control and pull the first row into the header.
    let mut lens_ui = ui.new_child(egui::UiBuilder::new().max_rect(lens_rect).id_salt("tree_header_lens"));
    super::segmented(
        &mut lens_ui,
        lens,
        labels,
        super::Segments {
            small: true,
            compact: true,
            workspace_accent: true,
            proportional: true,
            targets: &["render:tree:by-object", "render:tree:by-look"],
            ..Default::default()
        },
    );
    let title_rect =
        Rect::from_min_max(band.min + vec2(t.space_12, 0.0), pos2((lens_rect.left() - t.space_8).max(band.left() + t.space_12), band.bottom()));
    super::text::paint_text(ui, title_rect, title, crate::fonts::semibold(t.font_heading), t.text, false);
    t.secondary_hline(ui.painter(), band.x_range(), band.bottom(), t.dialog_rule);
    response.clicked()
}
