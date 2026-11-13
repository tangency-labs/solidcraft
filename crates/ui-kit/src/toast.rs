//! The view toast: a one-message card over the view's free part, for a hint, a result, a refusal
//! or work in progress. Its text is led by a dot (red for an error) or a spinner; below the text,
//! a wrapping row of kit buttons; a close (×) at the top right, with a ring that shrinks over the
//! time left of a toast that goes by itself. It sits at the bottom centre of the free part, as wide
//! as its text, or in a column at its bottom right, every toast [`WIDTH`] wide, with its buttons
//! placed as the dialogs place theirs. Everything comes from the caller's plain data and
//! `solidcraft_theme::Tokens`; the toast returns what was clicked and where it drew.
use crate::BackingTooltipExt as _;
use crate::{Container, Surface};
use egui::{Pos2, Rect, Sense, Stroke, pos2, vec2};
use solidcraft_theme::Tokens;

/// Every toast's width in the corner column: room for three buttons on one row; longer text wraps.
pub const WIDTH: f32 = 360.0;

/// Where a toast goes in the free part it is given.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum Place {
    /// At the bottom right, every toast [`WIDTH`] wide (or the room there is), its buttons as in
    /// the dialogs: right-aligned, dismissals first and the main action right-most.
    #[default]
    Corner,
    /// At the bottom centre, each as wide as its text, up to 440 points.
    Centre,
}

/// What a toast's button is for, which sets its look and where it goes: the main action first
/// (the accent button), then an alternative, then the quiet dismissals.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Action {
    Primary,
    Secondary,
    Dismiss,
}

/// What leads a toast's text: a dot (red when `true`, for an error) or a spinner.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Lead {
    Dot(bool),
    Spinner,
}

/// What a toast has besides its text and buttons.
#[derive(Clone, Copy, Default)]
pub struct Extras<'a> {
    /// A close (×) in its top-right corner, published as this control-channel handle
    /// (`toast:<name>:close`). Every toast the user may close has one, in place of a Close button.
    pub close: Option<&'a str>,
    /// For a toast that goes by itself: the share of its time left (1 to 0) and its whole time in
    /// seconds, drawn as a thin ring around the × that shrinks over its time.
    pub countdown: Option<(f32, f64)>,
    /// Hover texts of the buttons, by index; an empty one has none.
    pub tips: &'a [&'a str],
    /// The whole card answers a click, under its buttons and ×, with this hover text.
    pub card_hint: Option<&'a str>,
}

/// A toast as drawn: its card, which button was clicked, if any, where each button and the × are
/// (for handles), and whether the × or the card itself was clicked.
pub struct Shown {
    pub rect: Rect,
    pub clicked: Option<usize>,
    pub buttons: Vec<Rect>,
    pub close: Option<Rect>,
    pub closed: bool,
    pub card_clicked: bool,
}

/// Where each button of a toast goes, relative to the row's top left: `buttons` are (width, kind) in
/// the order given; the primary and the alternative form one group and the dismissals another.
/// Groups go side by side while they fit in `row_width` and wrap as a whole, so a button is not
/// left alone on a row while its group-mate fits beside it; a group wider than the row wraps
/// button by button.
pub fn action_spots(buttons: &[(f32, Action)], row_width: f32, gap: f32, height: f32) -> Vec<Rect> {
    let mut spots = vec![Rect::NOTHING; buttons.len()];
    let groups: [Vec<usize>; 2] = [
        (0..buttons.len()).filter(|&i| buttons[i].1 != Action::Dismiss).collect(),
        (0..buttons.len()).filter(|&i| buttons[i].1 == Action::Dismiss).collect(),
    ];
    let (mut x, mut y) = (0.0_f32, 0.0_f32);
    for group in groups.iter().filter(|g| !g.is_empty()) {
        let whole: f32 = group.iter().map(|&i| buttons[i].0).sum::<f32>() + gap * (group.len() - 1) as f32;
        if x > 0.0 && x + whole > row_width {
            (x, y) = (0.0, y + height + gap);
        }
        for &i in group {
            let width = buttons[i].0;
            // Only a group wider than a whole row wraps inside itself.
            if x > 0.0 && x + width > row_width.max(width) && whole > row_width {
                (x, y) = (0.0, y + height + gap);
            }
            spots[i] = Rect::from_min_size(pos2(x, y), vec2(width, height));
            x += width + gap;
        }
        x += gap;
    }
    spots
}

/// The corner column's buttons, as the dialogs place theirs: right-aligned, dismissals first, then
/// the alternative, then the main action right-most. Rows fill from the main action leftwards; a
/// button that doesn't fit starts a row above, so the row with the main action is the bottom one.
/// Positions are relative to the rows' top left, within `row_width`.
pub fn action_spots_trailing(buttons: &[(f32, Action)], row_width: f32, gap: f32, height: f32) -> Vec<Rect> {
    let rank = |kind: Action| match kind {
        Action::Dismiss => 0,
        Action::Secondary => 1,
        Action::Primary => 2,
    };
    let mut order: Vec<usize> = (0..buttons.len()).collect();
    order.sort_by_key(|&i| rank(buttons[i].1));
    let mut rows: Vec<Vec<usize>> = Vec::new();
    let mut used = 0.0_f32;
    for &i in order.iter().rev() {
        let width = buttons[i].0;
        match rows.last_mut() {
            Some(row) if used + gap + width <= row_width => {
                row.push(i);
                used += gap + width;
            }
            _ => {
                rows.push(vec![i]);
                used = width;
            }
        }
    }
    let mut spots = vec![Rect::NOTHING; buttons.len()];
    let count = rows.len();
    for (k, row) in rows.iter().enumerate() {
        let y = (count - 1 - k) as f32 * (height + gap);
        let mut x = row_width;
        for &i in row {
            x -= buttons[i].0;
            if let Some(spot) = spots.get_mut(i) {
                *spot = Rect::from_min_size(pos2(x, y), vec2(buttons[i].0, height));
            }
            x -= gap;
        }
    }
    spots
}

/// Where a toast button's drawn width is remembered: by its text and kind, which set its look.
fn button_key(label: &str, kind: Action) -> egui::Id {
    egui::Id::unique(("sc_toast_button_width", label, kind))
}

/// The width a toast button with this text and kind was last drawn at, if it has been drawn.
fn button_width(ctx: &egui::Context, label: &str, kind: Action) -> Option<f32> {
    ctx.data(|data| data.get_temp::<f32>(button_key(label, kind))).filter(|width| width.is_finite() && *width > 0.0)
}

/// The share of a countdown of `lifetime` seconds left at `now` when it started at `since`: 1 when
/// it starts, 0 when it is over, and 1 for times no clock gives (non-finite, or a start in the future).
pub fn countdown_left(since: f64, now: f64, lifetime: f64) -> f32 {
    let left = 1.0 - (now - since) / lifetime;
    if left.is_finite() { left.clamp(0.0, 1.0) as f32 } else { 1.0 }
}

/// The countdown ring's radius around a toast's ×; the × is drawn inside it and its hit target is
/// the ring's square.
fn close_radius() -> f32 {
    Tokens::get().space_10
}

/// The points of the countdown ring: an arc of `left` of a turn round `centre`, from twelve o'clock
/// clockwise, so it shrinks back towards twelve o'clock as the time goes. Empty when nothing is left.
pub fn countdown_arc(centre: Pos2, radius: f32, left: f32) -> Vec<Pos2> {
    let left = if left.is_finite() { left.clamp(0.0, 1.0) } else { 0.0 };
    if left <= 0.0 || !radius.is_finite() || radius <= 0.0 {
        return Vec::new();
    }
    // One segment per 7.5 degrees of the arc, at least one.
    let segments = ((48.0 * left).ceil() as usize).max(1);
    (0..=segments)
        .map(|i| {
            let angle = -std::f32::consts::FRAC_PI_2 + std::f32::consts::TAU * left * i as f32 / segments as f32;
            centre + radius * vec2(angle.cos(), angle.sin())
        })
        .collect()
}

/// A toast's close (×) at `rect`, with its countdown ring when `left` is given: a kit-drawn button
/// with an accessible name and hover text (`ui.close`), reachable by Tab and taken with Enter or Space.
fn close_button(ui: &egui::Ui, rect: Rect, id: egui::Id, left: Option<f32>) -> egui::Response {
    let t = Tokens::get();
    let response = ui.interact(rect, id, Sense::click());
    let painter = ui.painter();
    let centre = rect.center();
    let radius = rect.width().min(rect.height()) * 0.5;
    let lit = response.hovered() || response.has_focus();
    if lit && ui.is_enabled() {
        painter.circle_filled(centre, radius, t.hover);
    }
    if response.has_focus() {
        painter.circle_stroke(centre, radius + t.space_2, Stroke::new(1.0, t.form_primary));
    }
    if let Some(left) = left {
        let arc = countdown_arc(centre, radius - 0.75, left);
        if arc.len() >= 2 {
            painter.add(egui::Shape::line(arc, Stroke::new(1.5, t.text_dim)));
        }
    }
    let ink = if lit && ui.is_enabled() { t.text } else { t.text_dim };
    crate::icons::paint_ink(painter, Rect::from_center_size(centre, vec2(t.space_10, t.space_10)), "close", ink);
    let label = solidcraft_ui_text::tr("ui.close");
    response.widget_info(|| egui::WidgetInfo::labeled(egui::Role::Button, ui.is_enabled(), label));
    if response.hovered() && ui.is_enabled() {
        ui.ctx().set_cursor_icon(egui::CursorIcon::PointingHand);
    }
    response.backing_on_hover_text(label)
}

/// The toast: its text in a card at the bottom of `free` (`place` says where), led by `lead`, and,
/// below the text, a wrapping row of buttons (`actions`, kit buttons), with what `extras` adds (a
/// close (×) at the top right with its countdown ring, hover texts, a click on the card). Returns
/// the card, which button was clicked, if any, and where each button is (for handles); `None`
/// where `free` has no room for it. The buttons and the × are not live while input is blocked
/// ([`crate::input_blocked`]); none is laid outside the card, whose width is bounded by `free`.
pub fn show(ui: &egui::Ui, free: Rect, place: Place, text: String, lead: Lead, actions: &[(&str, Action)], extras: Extras<'_>) -> Option<Shown> {
    // Rects no screen has are rejected too: the corner layout anchors to `free`'s right edge.
    if !free.is_finite() || !free.is_positive() || free.width() <= 40.0 || free.max.x.abs().max(free.max.y.abs()) > 1.0e7 {
        return None;
    }
    let t = Tokens::get();
    let corner = place == Place::Corner;
    // The lead (dot or spinner) is centred in the first `space_14` of a `2 × space_14` column, the
    // text follows it and `space_12` closes the right side; `space_8` above and below.
    let inset = vec2(2.0 * t.space_14, t.space_8);
    let padding = vec2(inset.x + t.space_12, 2.0 * inset.y);
    let painter = ui.painter().with_clip_rect(free);
    // In the corner every toast is `WIDTH` wide (or the room there is); at the centre each is as
    // wide as its text, up to 440 points.
    let wrap = if corner { (WIDTH.min(free.width()) - padding.x).max(0.0) } else { (free.width() - padding.x).min(440.0) };
    // The × keeps the top-right corner: the text stops short of its ring.
    let radius = close_radius();
    let close_room = if extras.close.is_some() { 2.0 * radius + t.space_4 + t.space_8 - t.space_12 } else { 0.0 };
    let galley = painter.layout(text, crate::type_face::medium(t.font_label), t.text, (wrap - close_room).max(0.0));
    // The buttons: each as wide as its text and a margin, in rows no wider than the text may be.
    let gap = t.space_8;
    let button_height = crate::row_height();
    let row_width = if corner { wrap } else { (free.width() - padding.x).clamp(0.0, 440.0) };
    // Each button's width as the kit drew it: the same text, font and padding, measured from the
    // button itself. Before a button was ever drawn, an estimate stands in for one frame.
    let sizes: Vec<(f32, Action)> = actions
        .iter()
        .map(|(label, kind)| {
            let width = button_width(ui.ctx(), label, *kind).unwrap_or_else(|| {
                let padding = if *kind == Action::Primary { t.space_16 } else { t.space_12 };
                painter.layout_no_wrap((*label).to_string(), crate::type_face::medium(t.font_label), t.text).size().x + 2.0 * padding
            });
            (width.min(row_width), *kind)
        })
        .collect();
    let spots =
        if corner { action_spots_trailing(&sizes, row_width, gap, button_height) } else { action_spots(&sizes, row_width, gap, button_height) };
    let widest = spots.iter().map(|spot| spot.right()).fold(0.0, f32::max);
    let y = spots.iter().map(|spot| spot.top()).fold(0.0, f32::max);
    let actions_height = if spots.is_empty() { 0.0 } else { y + button_height + gap };
    let content = vec2((galley.size().x + close_room).max(widest), galley.size().y + actions_height);
    let mut size = content + padding;
    if corner {
        size.x = row_width + padding.x;
    }
    if extras.close.is_some() {
        size.y = size.y.max(2.0 * radius + 12.0);
    }
    if !size.is_finite() || size.x > free.width() || size.y > free.height() {
        return None;
    }
    let x = if corner { free.right() - size.x * 0.5 } else { free.center().x };
    let r = Rect::from_center_size(pos2(x, free.bottom() - size.y * 0.5), size);
    if !r.is_finite() {
        return None;
    }
    let frame = Container::new(ui.ctx(), Surface::Toast).frame(ui.ctx(), t.panel, crate::frame::Side::Left);
    Container::paint_frame(ui.painter(), frame, r.shrink(frame.stroke.width));
    let dot = pos2(r.left() + t.space_14, r.center().y.min(r.top() + inset.y + galley.size().y / 2.0));
    match lead {
        Lead::Spinner => egui::Spinner::new().size(t.space_12).color(t.text).paint_at(ui, Rect::from_center_size(dot, vec2(t.space_12, t.space_12))),
        Lead::Dot(error) => drop(painter.circle_filled(dot, 3.0, if error { t.error } else { t.text })),
    }
    let text_top = r.min + inset;
    painter.galley(text_top, galley.clone(), t.text);
    // The card's own click goes first, so its buttons and × (added after it) take theirs. With
    // input blocked it is dead too, so a click on the dead × doesn't fall through to it.
    let blocked = crate::input_blocked();
    let card_clicked = extras.card_hint.is_some_and(|hint| {
        let sense = if blocked { Sense::hover() } else { Sense::click() };
        ui.interact(r, egui::Id::unique(("sc_toast_card", extras.close.unwrap_or_default())), sense).backing_on_hover_text(hint).clicked()
    });
    let mut clicked = None;
    let mut buttons = Vec::with_capacity(actions.len());
    let origin = text_top + vec2(0.0, galley.size().y + gap);
    for (index, ((label, kind), spot)) in actions.iter().zip(&spots).enumerate() {
        let at = spot.translate(origin.to_vec2());
        // A UI of its own on the caller's layer, so the toast needs no `&mut Ui`. It has the
        // row's width, so a button is never squeezed into a spot laid out for a smaller one and
        // its measured width is its own.
        let room = Rect::from_min_size(at.min, vec2(row_width.max(at.width()), at.height()));
        let mut button_ui = egui::Ui::new(
            ui.ctx().clone(),
            egui::Id::unique(("sc_toast_action", extras.close, label, index)),
            egui::UiBuilder::new().layer_id(ui.layer_id()).max_rect(room).layout(egui::Layout::left_to_right(egui::Align::Center)),
        );
        button_ui.set_clip_rect(button_ui.clip_rect().intersect(r));
        let response = button_ui
            .add_enabled_ui(!blocked, |ui| match kind {
                Action::Primary => crate::primary_button(ui, label),
                Action::Secondary => crate::button(ui, *label, crate::Button::Normal),
                Action::Dismiss => crate::button(ui, *label, crate::Button::Small),
            })
            .inner;
        let response = match extras.tips.get(index).filter(|tip| !tip.is_empty()) {
            Some(tip) => response.backing_on_hover_text(*tip),
            None => response,
        };
        if response.clicked() {
            clicked = Some(index);
        }
        // Remember the width it was drawn at; laid out at another, the toast is laid out again next frame.
        let drawn = response.rect.width();
        if drawn.is_finite() && drawn > 0.0 {
            if sizes.get(index).is_some_and(|(used, _)| (used - drawn).abs() > 0.5 && drawn < row_width) {
                ui.ctx().request_repaint();
            }
            ui.ctx().data_mut(|data| data.insert_temp(button_key(label, *kind), drawn));
        }
        buttons.push(response.rect);
    }
    // The close (×), top right, level with the text's first line.
    let mut close = None;
    let mut closed = false;
    if let Some(handle) = extras.close {
        let line = painter.layout_no_wrap("A".into(), crate::type_face::medium(t.font_label), t.text).size().y;
        let centre = pos2(r.right() - t.space_8 - radius, (text_top.y + line * 0.5).max(r.top() + 6.0 + radius));
        let at = Rect::from_center_size(centre, vec2(2.0 * radius, 2.0 * radius));
        let mut close_ui = egui::Ui::new(
            ui.ctx().clone(),
            egui::Id::unique(("sc_toast_close_ui", handle)),
            egui::UiBuilder::new().layer_id(ui.layer_id()).max_rect(at),
        );
        close_ui.set_clip_rect(close_ui.clip_rect().intersect(r));
        if blocked {
            close_ui.disable();
        }
        let left = extras.countdown.map(|(left, _)| left);
        closed = close_button(&close_ui, at, egui::Id::unique(("sc_toast_close", handle)), left).clicked();
        solidcraft_ui_registry::publish_handle(handle, centre);
        close = Some(at);
        // A frame each time the ring's end moves about a point, not one per display refresh.
        if let Some((_, lifetime)) = extras.countdown {
            let every = lifetime / (std::f64::consts::TAU * f64::from(radius));
            let every = if every.is_finite() { every.clamp(0.05, 1.0) } else { 1.0 };
            ui.ctx().request_repaint_after(std::time::Duration::from_secs_f64(every));
        }
    }
    Some(Shown { rect: r, clicked, buttons, close, closed, card_clicked })
}

#[cfg(test)]
#[path = "toast_tests.rs"]
mod tests;
